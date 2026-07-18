#include "event_dispatcher.h"
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <cstdint>
#include <future>
#include <mutex>
#include <stdexcept>
#include <string>
#include <thread>
#include <utility>
#include <vector>

namespace {

using namespace std::chrono_literals;

#define CHECK(condition) \
    do { \
        if (!(condition)) \
            throw std::runtime_error(std::string("check failed: ") + #condition); \
    } while (false)

struct Received {
    int32_t type{};
    uint64_t sequence{};
    std::vector<uint8_t> data;
    std::thread::id thread;
};

struct Collector {
    std::mutex mutex;
    std::condition_variable changed;
    std::vector<Received> events;
    EventDispatcher* dispatcher{};
    bool block{};
    bool entered{};
    bool release{};
    bool throw_next{};
    uint64_t reentrant_sequence{};
    bool reentrant_publish_result{};
    bool reentrant_flush_rejected{};
    bool reentrant_stop_returned{};
    bool reentrant_stop_threw{};
};

OwnedEvent make_event(int32_t type, uint64_t sequence, llw_handle_t request = 0) {
    OwnedEvent event;
    event.type = type;
    event.data_format = type == LLW_EVENT_TOKEN ? LLW_EVENT_DATA_BYTES : LLW_EVENT_DATA_JSON_UTF8;
    event.request = request;
    event.sequence = sequence;
    event.data = {static_cast<uint8_t>(sequence)};
    return event;
}

void LLW_CALL collect(const llw_event_t* event, void* user_data) {
    auto& collector = *static_cast<Collector*>(user_data);
    bool throw_now = false;
    bool run_reentrant_actions = false;
    {
        Received received;
        received.type = event->event_type;
        received.sequence = event->sequence_number;
        received.thread = std::this_thread::get_id();
        if (event->data && event->data_len)
            received.data.assign(event->data, event->data + event->data_len);

        std::unique_lock lock(collector.mutex);
        collector.events.push_back(std::move(received));
        throw_now = std::exchange(collector.throw_next, false);
        run_reentrant_actions = collector.reentrant_sequence == event->sequence_number;
        if (collector.block) {
            collector.entered = true;
            collector.changed.notify_all();
            collector.changed.wait(lock, [&collector] { return collector.release; });
        }
    }

    if (run_reentrant_actions) {
        const bool published = collector.dispatcher->publish(make_event(LLW_EVENT_LOG, 2));
        bool flush_rejected = false;
        try {
            collector.dispatcher->flush();
        } catch (const std::logic_error&) {
            flush_rejected = true;
        }
        bool stop_returned = false;
        bool stop_threw = false;
        try {
            collector.dispatcher->stop();
            stop_returned = true;
        } catch (...) {
            stop_threw = true;
        }
        {
            std::lock_guard lock(collector.mutex);
            collector.reentrant_publish_result = published;
            collector.reentrant_flush_rejected = flush_rejected;
            collector.reentrant_stop_returned = stop_returned;
            collector.reentrant_stop_threw = stop_threw;
        }
        collector.changed.notify_all();
    }

    if (throw_now) throw std::runtime_error("injected callback failure");
}

llw_callback_table_t callbacks_for(Collector& collector) {
    llw_callback_table_t callbacks{};
    callbacks.struct_size = sizeof(callbacks);
    callbacks.on_event = collect;
    callbacks.user_data = &collector;
    return callbacks;
}

void wait_until_entered(Collector& collector) {
    std::unique_lock lock(collector.mutex);
    if (collector.changed.wait_for(lock, 5s, [&collector] { return collector.entered; })) return;
    collector.release = true;
    lock.unlock();
    collector.changed.notify_all();
    throw std::runtime_error("callback entry timeout");
}

void release_callback(Collector& collector) {
    {
        std::lock_guard lock(collector.mutex);
        collector.release = true;
    }
    collector.changed.notify_all();
}

struct ReleaseCallbackOnExit {
    explicit ReleaseCallbackOnExit(Collector& value) : collector(value) {}
    ~ReleaseCallbackOnExit() { release_callback(collector); }
    Collector& collector;
};

template <typename T>
bool ready(std::future<T>& future) {
    return future.wait_for(5s) == std::future_status::ready;
}

void ownership_and_callback_thread_test() {
    const std::thread::id test_thread = std::this_thread::get_id();
    Collector collector;
    EventDispatcher dispatcher(callbacks_for(collector), 16);
    std::vector<uint8_t> source = {0xf0, 0x9f, 0x92, 0xa1};
    OwnedEvent first = make_event(LLW_EVENT_TOKEN, 41);
    first.data = source;
    CHECK(dispatcher.publish(std::move(first)));
    source.assign(4, 0);
    OwnedEvent second = make_event(LLW_EVENT_DONE, 42, 1);
    second.data = {'{', '}'};
    CHECK(dispatcher.publish(std::move(second)));
    dispatcher.flush();

    std::lock_guard lock(collector.mutex);
    CHECK(collector.events.size() == 2);
    CHECK(collector.events[0].sequence == 41 && collector.events[1].sequence == 42);
    CHECK(collector.events[0].data == std::vector<uint8_t>({0xf0, 0x9f, 0x92, 0xa1}));
    CHECK(collector.events[0].thread != test_thread);
    CHECK(collector.events[1].thread == collector.events[0].thread);
}

void saturation_and_terminal_reserve_test() {
    Collector collector;
    collector.block = true;
    EventDispatcher dispatcher(callbacks_for(collector), 2);
    CHECK(dispatcher.publish(make_event(LLW_EVENT_LOG, 1)));
    wait_until_entered(collector);
    ReleaseCallbackOnExit release_on_exit(collector);
    CHECK(dispatcher.publish(make_event(LLW_EVENT_TOKEN, 2)));
    CHECK(dispatcher.publish(make_event(LLW_EVENT_METRICS, 3)));

    auto overflow = std::async(std::launch::async, [&dispatcher] {
        return dispatcher.publish(make_event(LLW_EVENT_LOG, 99));
    });
    auto terminal = std::async(std::launch::async, [&dispatcher] {
        return dispatcher.publish(make_event(LLW_EVENT_DONE, 4, 7));
    });
    const bool overflow_returned = ready(overflow);
    const bool terminal_returned = ready(terminal);
    release_callback(collector);
    const bool overflow_result = overflow.get();
    const bool terminal_result = terminal.get();
    dispatcher.drain_for_test();

    CHECK(overflow_returned);
    CHECK(!overflow_result);
    CHECK(terminal_returned);
    CHECK(terminal_result);
    std::lock_guard lock(collector.mutex);
    CHECK(collector.events.size() == 4);
    CHECK(collector.events[0].sequence == 1);
    CHECK(collector.events[1].sequence == 2);
    CHECK(collector.events[2].sequence == 3);
    CHECK(collector.events[3].sequence == 4);
}

void saturated_flush_and_stop_wakeup_test() {
    Collector collector;
    collector.block = true;
    EventDispatcher dispatcher(callbacks_for(collector), 1);
    CHECK(dispatcher.publish(make_event(LLW_EVENT_LOG, 1)));
    wait_until_entered(collector);
    ReleaseCallbackOnExit release_on_exit(collector);
    CHECK(dispatcher.publish(make_event(LLW_EVENT_TOKEN, 2)));

    std::promise<void> first_enqueued_promise;
    auto first_enqueued = first_enqueued_promise.get_future();
    auto first_flush = std::async(std::launch::async, [&] {
        try {
            dispatcher.flush_for_test([&] { first_enqueued_promise.set_value(); });
            return true;
        } catch (...) {
            return false;
        }
    });
    const bool first_had_control_priority = ready(first_enqueued);

    std::promise<void> second_started_promise;
    auto second_started = second_started_promise.get_future();
    std::promise<void> second_waiting_promise;
    auto second_waiting = second_waiting_promise.get_future();
    std::promise<void> second_enqueued_promise;
    auto second_enqueued = second_enqueued_promise.get_future();
    auto second_flush = std::async(std::launch::async, [&] {
        second_started_promise.set_value();
        try {
            dispatcher.flush_for_test([&] { second_enqueued_promise.set_value(); },
                                      [&] { second_waiting_promise.set_value(); });
            return false;
        } catch (const std::runtime_error&) {
            return true;
        }
    });
    const bool second_did_start = ready(second_started);
    const bool second_did_wait = second_did_start && ready(second_waiting);

    auto stopper = std::async(std::launch::async, [&dispatcher] { dispatcher.stop(); });
    const bool waiting_flush_woke = ready(second_flush);
    const bool second_never_enqueued = second_enqueued.wait_for(0s) != std::future_status::ready;
    release_callback(collector);
    const bool first_completed = first_flush.get();
    const bool second_stopped = second_flush.get();
    stopper.get();

    CHECK(first_had_control_priority);
    CHECK(first_completed);
    CHECK(second_did_start);
    CHECK(second_did_wait);
    CHECK(waiting_flush_woke);
    CHECK(second_stopped);
    CHECK(second_never_enqueued);
    std::lock_guard lock(collector.mutex);
    CHECK(collector.events.size() == 2);
    CHECK(collector.events[0].sequence == 1 && collector.events[1].sequence == 2);
}

void callback_exception_does_not_stop_worker_test() {
    Collector collector;
    collector.throw_next = true;
    EventDispatcher dispatcher(callbacks_for(collector), 4);
    CHECK(dispatcher.publish(make_event(LLW_EVENT_LOG, 1)));
    CHECK(dispatcher.publish(make_event(LLW_EVENT_LOG, 2)));
    dispatcher.flush();
    CHECK(dispatcher.publish(make_event(LLW_EVENT_LOG, 3)));
    dispatcher.flush();

    std::lock_guard lock(collector.mutex);
    CHECK(collector.events.size() == 3);
    CHECK(collector.events[0].sequence == 1);
    CHECK(collector.events[1].sequence == 2);
    CHECK(collector.events[2].sequence == 3);
}

void callback_reentrancy_and_deferred_join_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks_for(collector), 2);
    collector.dispatcher = &dispatcher;
    collector.reentrant_sequence = 1;
    CHECK(dispatcher.publish(make_event(LLW_EVENT_LOG, 1)));
    bool reentrant_completed = false;
    {
        std::unique_lock lock(collector.mutex);
        reentrant_completed = collector.changed.wait_for(lock, 5s, [&collector] {
            return collector.reentrant_stop_returned || collector.reentrant_stop_threw;
        });
    }
    dispatcher.stop();

    CHECK(reentrant_completed);
    CHECK(!dispatcher.publish(make_event(LLW_EVENT_LOG, 3)));
    bool stopped_flush_rejected = false;
    try {
        dispatcher.flush();
    } catch (const std::runtime_error&) {
        stopped_flush_rejected = true;
    }
    CHECK(stopped_flush_rejected);
    std::lock_guard lock(collector.mutex);
    CHECK(collector.reentrant_publish_result);
    CHECK(collector.reentrant_flush_rejected);
    CHECK(collector.reentrant_stop_returned);
    CHECK(!collector.reentrant_stop_threw);
    CHECK(collector.events.size() == 2);
    CHECK(collector.events[0].sequence == 1 && collector.events[1].sequence == 2);
}

} // namespace

int main() {
    try {
        ownership_and_callback_thread_test();
        saturation_and_terminal_reserve_test();
        saturated_flush_and_stop_wakeup_test();
        callback_exception_does_not_stop_worker_test();
        callback_reentrancy_and_deferred_join_test();
        return 0;
    } catch (const std::exception& exception) {
        std::fprintf(stderr, "%s\n", exception.what());
        return 1;
    }
}
