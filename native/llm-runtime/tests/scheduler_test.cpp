#include "event_dispatcher.h"
#include "fake_engine.h"
#include "scheduler.h"
#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <map>
#include <mutex>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

struct SeenEvent {
    int32_t type{};
    llw_handle_t request{};
    uint32_t slot{};
    uint64_t sequence{};
    void* request_user_data{};
};
struct Collector {
    std::mutex mutex;
    std::condition_variable changed;
    std::vector<SeenEvent> events;
    int32_t block_type{};
    bool callback_entered{};
    bool release_callback{};
    bool throw_terminal{};
};

void LLW_CALL collect_scheduler_event(const llw_event_t* event, void* user_data) {
    auto& collector = *static_cast<Collector*>(user_data);
    bool throw_terminal = false;
    {
        std::unique_lock lock(collector.mutex);
        collector.events.push_back(SeenEvent{event->event_type, event->request_handle,
                                              event->slot_id, event->sequence_number,
                                              event->request_user_data});
        if (event->event_type == collector.block_type && !collector.callback_entered) {
            collector.callback_entered = true;
            collector.changed.notify_all();
            collector.changed.wait(lock, [&collector] { return collector.release_callback; });
        }
        throw_terminal = collector.throw_terminal &&
            (event->event_type == LLW_EVENT_DONE || event->event_type == LLW_EVENT_CANCELLED ||
             event->event_type == LLW_EVENT_ERROR);
    }
    collector.changed.notify_all();
    if (throw_terminal) throw std::runtime_error("injected terminal callback failure");
}

llw_callback_table_t callbacks(Collector& collector) {
    llw_callback_table_t result{};
    result.struct_size = sizeof(result);
    result.on_event = collect_scheduler_event;
    result.user_data = &collector;
    return result;
}

llw_request_params_t request_params(const std::string& prompt) {
    llw_request_params_t result{};
    result.struct_size = sizeof(result);
    result.model_handle = 1;
    result.prompt = reinterpret_cast<const uint8_t*>(prompt.data());
    result.prompt_len = prompt.size();
    result.max_new_tokens = 3;
    result.seed = 7;
    result.temperature = 0;
    result.top_k = 40;
    result.top_p = 0.95f;
    result.min_p = 0.05f;
    result.repeat_last_n = 64;
    result.repeat_penalty = 1.1f;
    return result;
}

void wait_for_terminals(Collector& collector, size_t count) {
    std::unique_lock lock(collector.mutex);
    const bool ready = collector.changed.wait_for(lock, std::chrono::seconds(5), [&] {
        return std::count_if(collector.events.begin(), collector.events.end(), [](const SeenEvent& event) {
            return event.type == LLW_EVENT_DONE || event.type == LLW_EVENT_CANCELLED ||
                   event.type == LLW_EVENT_ERROR;
        }) >= static_cast<std::ptrdiff_t>(count);
    });
    if (!ready) throw std::runtime_error("terminal event timeout");
}

void wait_for_callback_entry(Collector& collector) {
    std::unique_lock lock(collector.mutex);
    if (!collector.changed.wait_for(lock, std::chrono::seconds(5), [&collector] {
            return collector.callback_entered;
        }))
        throw std::runtime_error("callback entry timeout");
}

void release_callback(Collector& collector) {
    {
        std::lock_guard lock(collector.mutex);
        collector.release_callback = true;
    }
    collector.changed.notify_all();
}

OwnedEvent telemetry_event(uint64_t sequence) {
    OwnedEvent event;
    event.type = LLW_EVENT_LOG;
    event.data_format = LLW_EVENT_DATA_JSON_UTF8;
    event.sequence = sequence;
    return event;
}

void assert_sequences(const Collector& collector, llw_handle_t request) {
    uint64_t expected = 1;
    size_t terminals = 0;
    for (const SeenEvent& event : collector.events) {
        if (event.request != request) continue;
        if (event.sequence != expected++) throw std::runtime_error("non-monotonic request sequence");
        if (event.type == LLW_EVENT_DONE || event.type == LLW_EVENT_CANCELLED ||
            event.type == LLW_EVENT_ERROR) ++terminals;
    }
    if (terminals != 1) throw std::runtime_error("request did not have exactly one terminal");
}

void concurrent_requests_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 64);
    FakeEngine engine;
    Scheduler scheduler(2, 4, engine, dispatcher);
    const std::string first_prompt = "first";
    const std::string second_prompt = "second";
    llw_handle_t first{};
    llw_handle_t second{};
    std::string error;
    if (scheduler.submit(request_params(first_prompt), first, error) != LLW_OK) throw std::runtime_error(error);
    if (scheduler.submit(request_params(second_prompt), second, error) != LLW_OK) throw std::runtime_error(error);
    engine.wait_for_batch_size(2);
    engine.release();
    wait_for_terminals(collector, 2);
    dispatcher.drain_for_test();
    std::lock_guard lock(collector.mutex);
    assert_sequences(collector, first);
    assert_sequences(collector, second);
    if (engine.cleanup_count(first) != 1 || engine.cleanup_count(second) != 1)
        throw std::runtime_error("completed sequences were not cleaned exactly once");
    std::map<llw_handle_t, uint32_t> slots;
    for (const SeenEvent& event : collector.events)
        if (event.request != 0 && event.slot != UINT32_MAX) slots[event.request] = event.slot;
    if (slots[first] == slots[second]) throw std::runtime_error("requests shared a slot");
}

void queue_full_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 64);
    FakeEngine engine;
    Scheduler scheduler(1, 1, engine, dispatcher);
    const std::string first_prompt = "first";
    const std::string second_prompt = "second";
    const std::string third_prompt = "third";
    llw_handle_t first{}, second{}, third{};
    std::string error;
    if (scheduler.submit(request_params(first_prompt), first, error) != LLW_OK) throw std::runtime_error(error);
    engine.wait_for_started(1);
    if (scheduler.submit(request_params(second_prompt), second, error) != LLW_OK) throw std::runtime_error(error);
    if (scheduler.submit(request_params(third_prompt), third, error) != LLW_ERR_QUEUE_FULL || third != 0)
        throw std::runtime_error("queue-full contract failed");
    engine.release();
    wait_for_terminals(collector, 2);
}

void per_slot_failure_isolation_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 64);
    FakeEngine engine;
    const std::string oversized_prompt = "oversized";
    const std::string healthy_prompt = "healthy";
    engine.reject_prompt(std::vector<uint8_t>(oversized_prompt.begin(), oversized_prompt.end()));
    Scheduler scheduler(2, 2, engine, dispatcher);
    llw_handle_t oversized{}, healthy{};
    std::string error;
    if (scheduler.submit(request_params(oversized_prompt), oversized, error) != LLW_OK)
        throw std::runtime_error(error);
    if (scheduler.submit(request_params(healthy_prompt), healthy, error) != LLW_OK)
        throw std::runtime_error(error);
    engine.wait_for_started(1);
    engine.release();
    wait_for_terminals(collector, 2);
    dispatcher.drain_for_test();
    std::lock_guard lock(collector.mutex);
    assert_sequences(collector, oversized);
    assert_sequences(collector, healthy);
    const auto terminal_type = [&collector](llw_handle_t handle) {
        for (const SeenEvent& event : collector.events) {
            if (event.request == handle && (event.type == LLW_EVENT_DONE ||
                event.type == LLW_EVENT_CANCELLED || event.type == LLW_EVENT_ERROR))
                return event.type;
        }
        return int32_t{0};
    };
    if (terminal_type(oversized) != LLW_EVENT_ERROR || terminal_type(healthy) != LLW_EVENT_DONE)
        throw std::runtime_error("per-slot failure affected a healthy peer");
    if (engine.cleanup_count(oversized) != 0 || engine.cleanup_count(healthy) != 1)
        throw std::runtime_error("per-slot cleanup counts are incorrect");
}

void cancellation_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 64);
    FakeEngine engine;
    Scheduler scheduler(1, 2, engine, dispatcher);
    const std::string active_prompt = "active";
    const std::string queued_prompt = "queued";
    llw_handle_t active{}, queued{};
    std::string error;
    if (scheduler.submit(request_params(active_prompt), active, error) != LLW_OK) throw std::runtime_error(error);
    engine.wait_for_started(1);
    if (scheduler.submit(request_params(queued_prompt), queued, error) != LLW_OK) throw std::runtime_error(error);
    if (scheduler.cancel(queued, error) != LLW_OK) throw std::runtime_error(error);
    if (scheduler.cancel(active, error) != LLW_OK) throw std::runtime_error(error);
    engine.release();
    wait_for_terminals(collector, 2);
    if (scheduler.cancel(active, error) != LLW_ERR_NOT_FOUND ||
        scheduler.cancel(queued, error) != LLW_ERR_NOT_FOUND)
        throw std::runtime_error("erased terminal handles must return not-found");
    dispatcher.drain_for_test();
    std::lock_guard lock(collector.mutex);
    assert_sequences(collector, active);
    assert_sequences(collector, queued);
    if (engine.cleanup_count(active) != 1 || engine.cleanup_count(queued) != 0)
        throw std::runtime_error("active and queued cancellation cleanup counts differ");
    if (scheduler.tracked_request_count_for_test() != 0)
        throw std::runtime_error("cancelled requests remained tracked");
}

void decode_failure_cleanup_precedes_slot_reuse_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 64);
    FakeEngine engine;
    engine.set_decode_failure(true);
    Scheduler scheduler(2, 2, engine, dispatcher);
    const std::string first_prompt = "first";
    const std::string second_prompt = "second";
    const std::string reuse_prompt = "reuse";
    llw_handle_t first{}, second{}, reuse{};
    std::string error;
    if (scheduler.submit(request_params(first_prompt), first, error) != LLW_OK)
        throw std::runtime_error(error);
    if (scheduler.submit(request_params(second_prompt), second, error) != LLW_OK)
        throw std::runtime_error(error);
    engine.wait_for_batch_size(2);
    engine.release();
    wait_for_terminals(collector, 2);
    if (engine.cleanup_count(first) != 1 || engine.cleanup_count(second) != 1 ||
        scheduler.tracked_request_count_for_test() != 0)
        throw std::runtime_error("failed shared decode did not clean and erase every request");
    engine.set_decode_failure(false);
    if (scheduler.submit(request_params(reuse_prompt), reuse, error) != LLW_OK)
        throw std::runtime_error(error);
    wait_for_terminals(collector, 3);
    const auto operations = engine.operation_log();
    const auto first_cleanup = std::find(operations.begin(), operations.end(),
                                         "cleanup:" + std::to_string(first));
    const auto second_cleanup = std::find(operations.begin(), operations.end(),
                                          "cleanup:" + std::to_string(second));
    const auto reused = std::find(operations.begin(), operations.end(),
                                  "start:" + std::to_string(reuse));
    if (first_cleanup == operations.end() || second_cleanup == operations.end() ||
        reused == operations.end() || first_cleanup > reused || second_cleanup > reused)
        throw std::runtime_error("slot was reused before all failed-sequence cleanup");
    if (engine.cleanup_count(reuse) != 1)
        throw std::runtime_error("reused slot completion was not cleaned exactly once");
}

void bounded_terminal_storage_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 256);
    FakeEngine engine;
    Scheduler scheduler(1, 2, engine, dispatcher);
    engine.release();
    std::string error;
    for (size_t index = 0; index < 100; ++index) {
        const std::string prompt = "request-" + std::to_string(index);
        llw_handle_t handle{};
        if (scheduler.submit(request_params(prompt), handle, error) != LLW_OK)
            throw std::runtime_error(error);
        wait_for_terminals(collector, index + 1);
        if (scheduler.tracked_request_count_for_test() != 0 || engine.cleanup_count(handle) != 1)
            throw std::runtime_error("terminal request storage was retained");
        if (scheduler.cancel(handle, error) != LLW_ERR_NOT_FOUND)
            throw std::runtime_error("terminal request cancel must return not-found");
    }
}

void deterministic_metrics_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 64);
    FakeEngine engine;
    engine.set_empty_token_bytes(true);
    std::atomic<uint64_t> ticks{0};
    Scheduler scheduler(1, 2, engine, dispatcher, [&ticks] {
        return Scheduler::TimePoint(std::chrono::nanoseconds(ticks.fetch_add(10)));
    });
    const std::string prompt = "four";
    llw_handle_t handle{};
    std::string error;
    if (scheduler.submit(request_params(prompt), handle, error) != LLW_OK)
        throw std::runtime_error(error);
    engine.wait_for_started(1);
    engine.release();
    wait_for_terminals(collector, 1);
    dispatcher.drain_for_test();
    const llw_metrics_t metrics = scheduler.metrics();
    if (metrics.prompt_tokens != prompt.size() || metrics.generated_tokens != 3 ||
        metrics.queue_wait_ns != 10)
        throw std::runtime_error("sample, prompt, or queue-wait metrics are not deterministic");
    std::lock_guard lock(collector.mutex);
    if (std::count_if(collector.events.begin(), collector.events.end(), [handle](const SeenEvent& event) {
            return event.request == handle && event.type == LLW_EVENT_TOKEN;
        }) != 0)
        throw std::runtime_error("empty sampled pieces emitted token events");
}

void queued_overflow_rejects_transactionally_test() {
    Collector collector;
    collector.block_type = LLW_EVENT_LOG;
    EventDispatcher dispatcher(callbacks(collector), 1);
    if (!dispatcher.publish(telemetry_event(1))) throw std::runtime_error("prefill publish failed");
    wait_for_callback_entry(collector);
    if (!dispatcher.publish(telemetry_event(2))) throw std::runtime_error("queue fill failed");

    FakeEngine engine;
    Scheduler scheduler(1, 1, engine, dispatcher);
    const std::string prompt = "overflow";
    llw_handle_t handle{};
    std::string error;
    if (scheduler.submit(request_params(prompt), handle, error) != LLW_ERR_QUEUE_FULL || handle != 0)
        throw std::runtime_error("regular overflow was not rejected transactionally");
    const llw_scheduler_snapshot_t snapshot = scheduler.snapshot();
    if (scheduler.tracked_request_count_for_test() != 0 || snapshot.accepted_requests != 0 ||
        snapshot.terminal_requests != 0 || dispatcher.terminal_permit_count_for_test() != 0)
        throw std::runtime_error("queued overflow retained acceptance state");
    release_callback(collector);
    dispatcher.drain_for_test();
    std::lock_guard lock(collector.mutex);
    if (std::any_of(collector.events.begin(), collector.events.end(), [](const SeenEvent& event) {
            return event.request != 0;
        }))
        throw std::runtime_error("rejected overflow emitted a request callback");
}

void token_overflow_does_not_throw_worker_test() {
    Collector collector;
    collector.block_type = LLW_EVENT_QUEUED;
    EventDispatcher dispatcher(callbacks(collector), 1);
    FakeEngine engine;
    Scheduler scheduler(1, 1, engine, dispatcher);
    const std::string prompt = "token-overflow";
    llw_handle_t handle{};
    std::string error;
    if (scheduler.submit(request_params(prompt), handle, error) != LLW_OK)
        throw std::runtime_error(error);
    wait_for_callback_entry(collector);
    engine.wait_for_started(1);
    engine.release();
    engine.wait_for_cleanup(handle);
    release_callback(collector);
    wait_for_terminals(collector, 1);
    dispatcher.drain_for_test();
    std::lock_guard lock(collector.mutex);
    const auto errors = std::count_if(collector.events.begin(), collector.events.end(), [handle](const SeenEvent& event) {
        return event.request == handle && event.type == LLW_EVENT_ERROR;
    });
    if (errors != 1) throw std::runtime_error("token overflow did not finish with one error");
}

void metrics_overflow_only_drops_telemetry_test() {
    Collector collector;
    collector.block_type = LLW_EVENT_QUEUED;
    EventDispatcher dispatcher(callbacks(collector), 1);
    FakeEngine engine;
    engine.set_empty_token_bytes(true);
    Scheduler scheduler(1, 1, engine, dispatcher);
    const std::string prompt = "metrics-overflow";
    llw_handle_t handle{};
    std::string error;
    if (scheduler.submit(request_params(prompt), handle, error) != LLW_OK)
        throw std::runtime_error(error);
    wait_for_callback_entry(collector);
    engine.wait_for_started(1);
    engine.release();
    engine.wait_for_cleanup(handle);
    release_callback(collector);
    wait_for_terminals(collector, 1);
    dispatcher.drain_for_test();
    std::lock_guard lock(collector.mutex);
    const auto done = std::count_if(collector.events.begin(), collector.events.end(), [handle](const SeenEvent& event) {
        return event.request == handle && event.type == LLW_EVENT_DONE;
    });
    if (done != 1 || scheduler.metrics().generated_tokens != 3)
        throw std::runtime_error("metrics overflow affected request execution");
}

void terminal_permit_bound_rejects_before_acceptance_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 8);
    FakeEngine engine;
    Scheduler scheduler(LLW_MAX_SLOTS, LLW_MAX_QUEUE_CAPACITY, engine, dispatcher);
    constexpr size_t bound = LLW_MAX_QUEUE_CAPACITY + LLW_MAX_SLOTS;
    std::string error;
    for (llw_handle_t handle = 1; handle <= bound; ++handle)
        if (!dispatcher.reserve_terminal(handle))
            throw std::runtime_error("terminal permit bound rejected too early");
    llw_handle_t rejected = 99;
    const std::string prompt = "rejected";
    if (scheduler.submit(request_params(prompt), rejected, error) != LLW_ERR_QUEUE_FULL || rejected != 0)
        throw std::runtime_error("terminal permit exhaustion did not reject before acceptance");
    const llw_scheduler_snapshot_t snapshot = scheduler.snapshot();
    if (snapshot.accepted_requests != 0 || snapshot.terminal_requests != 0 ||
        scheduler.tracked_request_count_for_test() != 0 ||
        dispatcher.terminal_permit_count_for_test() != bound)
        throw std::runtime_error("permit exhaustion changed scheduler acceptance state");
    for (llw_handle_t handle = 1; handle <= bound; ++handle)
        if (!dispatcher.release_terminal(handle))
            throw std::runtime_error("reserved terminal permit did not release");
    if (dispatcher.terminal_permit_count_for_test() != 0)
        throw std::runtime_error("terminal permits remained after explicit release");
}

void pre_acceptance_failures_release_terminal_permit_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 8);
    FakeEngine engine;
    Scheduler scheduler(1, 1, engine, dispatcher);
    const std::string prompt = "rollback";
    for (const Scheduler::SubmitFailurePoint point : {
             Scheduler::SubmitFailurePoint::RequestInsert,
             Scheduler::SubmitFailurePoint::QueueInsert}) {
        scheduler.fail_next_submit_for_test(point);
        llw_handle_t handle = 99;
        std::string error;
        bool threw = false;
        try {
            scheduler.submit(request_params(prompt), handle, error);
        } catch (const std::runtime_error&) {
            threw = true;
        }
        if (!threw || handle != 0 || scheduler.tracked_request_count_for_test() != 0 ||
            scheduler.snapshot().accepted_requests != 0 ||
            dispatcher.terminal_permit_count_for_test() != 0)
            throw std::runtime_error("pre-acceptance rollback retained state or terminal permit");
    }
}

void queued_publish_throw_rolls_back_acceptance_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 8);
    FakeEngine engine;
    engine.release();
    Scheduler scheduler(1, 1, engine, dispatcher);
    dispatcher.throw_next_publish_of_type_for_test(LLW_EVENT_QUEUED);
    const std::string prompt = "publish-throw";
    llw_request_params_t params = request_params(prompt);
    int failed_request_context{};
    params.request_user_data = &failed_request_context;
    llw_handle_t handle = 99;
    std::string error;
    bool threw = false;
    try {
        scheduler.submit(params, handle, error);
    } catch (const std::bad_alloc&) {
        threw = true;
    }
    dispatcher.drain_for_test();
    const llw_scheduler_snapshot_t snapshot = scheduler.snapshot();
    if (!threw || handle != 0 || scheduler.tracked_request_count_for_test() != 0 ||
        snapshot.queued_count != 0 || snapshot.active_count != 0 ||
        snapshot.accepted_requests != 0 || snapshot.terminal_requests != 0 ||
        dispatcher.terminal_permit_count_for_test() != 0)
        throw std::runtime_error("queued publish throw did not roll back acceptance");
    std::lock_guard lock(collector.mutex);
    if (std::any_of(collector.events.begin(), collector.events.end(),
                    [&failed_request_context](const SeenEvent& event) {
                        return event.request_user_data == &failed_request_context;
                    }))
        throw std::runtime_error("failed submit retained request user data in a callback");
}

void throwing_terminal_callback_releases_scheduler_permit_test() {
    Collector collector;
    collector.throw_terminal = true;
    EventDispatcher dispatcher(callbacks(collector), 8);
    FakeEngine engine;
    engine.release();
    Scheduler scheduler(1, 1, engine, dispatcher);
    const std::string prompt = "throwing-callback";
    llw_handle_t handle{};
    std::string error;
    if (scheduler.submit(request_params(prompt), handle, error) != LLW_OK)
        throw std::runtime_error(error);
    wait_for_terminals(collector, 1);
    dispatcher.drain_for_test();
    if (dispatcher.terminal_permit_count_for_test() != 0)
        throw std::runtime_error("throwing terminal callback retained permit");
}

int main() {
    try {
        concurrent_requests_test();
        queue_full_test();
        per_slot_failure_isolation_test();
        cancellation_test();
        decode_failure_cleanup_precedes_slot_reuse_test();
        bounded_terminal_storage_test();
        deterministic_metrics_test();
        queued_overflow_rejects_transactionally_test();
        token_overflow_does_not_throw_worker_test();
        metrics_overflow_only_drops_telemetry_test();
        terminal_permit_bound_rejects_before_acceptance_test();
        pre_acceptance_failures_release_terminal_permit_test();
        queued_publish_throw_rolls_back_acceptance_test();
        throwing_terminal_callback_releases_scheduler_permit_test();
        return 0;
    } catch (const std::exception& exception) {
        std::fprintf(stderr, "%s\n", exception.what());
        return 1;
    }
}
