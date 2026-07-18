#include "event_dispatcher.h"
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <cstdio>
#include <future>
#include <mutex>
#include <thread>
#include <utility>
#include <vector>

struct Received {
    uint64_t sequence{};
    std::vector<uint8_t> data;
    std::thread::id thread;
};
struct Collector {
    std::mutex mutex;
    std::condition_variable changed;
    std::vector<Received> events;
    bool block{};
    bool entered{};
    bool release{};
};

void LLW_CALL collect(const llw_event_t* event, void* user_data) {
    auto& collector = *static_cast<Collector*>(user_data);
    Received received;
    received.sequence = event->sequence_number;
    received.thread = std::this_thread::get_id();
    if (event->data && event->data_len)
        received.data.assign(event->data, event->data + event->data_len);
    std::unique_lock lock(collector.mutex);
    collector.events.push_back(std::move(received));
    if (collector.block) {
        collector.entered = true;
        collector.changed.notify_all();
        collector.changed.wait(lock, [&collector] { return collector.release; });
    }
}

int main() {
    const std::thread::id test_thread = std::this_thread::get_id();
    Collector collector;
    llw_callback_table_t callbacks{};
    callbacks.struct_size = sizeof(callbacks);
    callbacks.on_event = collect;
    callbacks.user_data = &collector;
    EventDispatcher dispatcher(callbacks, 16);
    std::vector<uint8_t> source = {0xf0, 0x9f, 0x92, 0xa1};
    OwnedEvent first;
    first.type = LLW_EVENT_TOKEN;
    first.data_format = LLW_EVENT_DATA_BYTES;
    first.sequence = 41;
    first.data = source;
    if (!dispatcher.publish(std::move(first))) return 1;
    source.assign(4, 0);
    OwnedEvent second;
    second.type = LLW_EVENT_DONE;
    second.data_format = LLW_EVENT_DATA_JSON_UTF8;
    second.sequence = 42;
    second.data = {'{', '}'};
    if (!dispatcher.publish(std::move(second))) return 1;
    dispatcher.drain_for_test();
    {
        std::lock_guard lock(collector.mutex);
        if (collector.events.size() != 2) return 1;
        if (collector.events[0].sequence != 41 || collector.events[1].sequence != 42) return 1;
        if (collector.events[0].data != std::vector<uint8_t>({0xf0, 0x9f, 0x92, 0xa1})) return 1;
        if (collector.events[0].thread == test_thread ||
            collector.events[1].thread != collector.events[0].thread) return 1;
        collector.block = true;
    }
    OwnedEvent slow;
    slow.type = LLW_EVENT_LOG;
    slow.data_format = LLW_EVENT_DATA_UTF8;
    slow.data = {'s', 'l', 'o', 'w'};
    if (!dispatcher.publish(std::move(slow))) return 1;
    {
        std::unique_lock lock(collector.mutex);
        if (!collector.changed.wait_for(lock, std::chrono::seconds(5),
                                        [&collector] { return collector.entered; })) return 1;
    }
    std::promise<void> barrier_enqueued_promise;
    std::future<void> barrier_enqueued = barrier_enqueued_promise.get_future();
    std::atomic<bool> flushed{false};
    std::thread flusher([&] {
        dispatcher.flush_for_test([&] { barrier_enqueued_promise.set_value(); });
        flushed.store(true, std::memory_order_release);
    });
    const bool marker_was_enqueued =
        barrier_enqueued.wait_for(std::chrono::seconds(5)) == std::future_status::ready;
    const bool returned_while_callback_blocked = flushed.load(std::memory_order_acquire);
    {
        std::lock_guard lock(collector.mutex);
        collector.release = true;
    }
    collector.changed.notify_all();
    flusher.join();
    if (!marker_was_enqueued || returned_while_callback_blocked ||
        !flushed.load(std::memory_order_acquire)) return 1;
    return 0;
}
