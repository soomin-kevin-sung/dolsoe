#include "llw_runtime.h"
#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <map>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#define CHECK(condition) do { if (!(condition)) { \
    std::fprintf(stderr, "%s:%d failed: %s\n", __FILE__, __LINE__, #condition); return 1; \
} } while (false)

#ifdef LLW_RUNTIME_TESTING
LLW_EXTERN_C LLW_EXPORT void LLW_CALL LLWTestSetFlushEnqueuedHook(
    llw_runtime_t* runtime, void (LLW_CALL *hook)(void*), void* user_data);
LLW_EXTERN_C LLW_EXPORT void LLW_CALL LLWTestFailNextPublishOfType(
    llw_runtime_t* runtime, int32_t event_type);
#endif

struct RequestResult { std::vector<uint8_t> bytes; uint32_t terminals{}; bool error{}; };
struct Events {
    std::mutex mutex;
    std::condition_variable changed;
    std::map<llw_handle_t, RequestResult> requests;
    bool block_callbacks{};
    bool callback_entered{};
    bool release_callbacks{};
    bool flush_barrier_enqueued{};
};

#ifdef LLW_RUNTIME_TESTING
void LLW_CALL mark_flush_barrier_enqueued(void* user_data) {
    auto& events = *static_cast<Events*>(user_data);
    {
        std::lock_guard lock(events.mutex);
        events.flush_barrier_enqueued = true;
    }
    events.changed.notify_all();
}
#endif

void LLW_CALL collect_backend_event(const llw_event_t* event, void* user_data) {
    if (!event) return;
    auto& events = *static_cast<Events*>(user_data);
    {
        std::unique_lock lock(events.mutex);
        if (event->request_handle != 0) {
            RequestResult& result = events.requests[event->request_handle];
            if (event->event_type == LLW_EVENT_TOKEN && event->data)
                result.bytes.insert(result.bytes.end(), event->data, event->data + event->data_len);
            if (event->event_type == LLW_EVENT_DONE || event->event_type == LLW_EVENT_CANCELLED ||
                event->event_type == LLW_EVENT_ERROR) ++result.terminals;
            if (event->event_type == LLW_EVENT_ERROR) result.error = true;
        }
        if (events.block_callbacks && event->event_type == LLW_EVENT_MODEL_PROGRESS) {
            events.callback_entered = true;
            events.changed.notify_all();
            events.changed.wait(lock, [&events] { return events.release_callbacks; });
        }
    }
    events.changed.notify_all();
}

int32_t selected_backend() {
    const char* value = std::getenv("LLW_TEST_BACKEND");
    if (!value || std::string(value) == "CPU") return LLW_BACKEND_CPU;
    if (std::string(value) == "CUDA") return LLW_BACKEND_CUDA;
    if (std::string(value) == "VULKAN") return LLW_BACKEND_VULKAN;
    return -1;
}

llw_request_params_t generation(llw_handle_t model, const std::string& prompt) {
    llw_request_params_t params{};
    params.struct_size = sizeof(params);
    params.model_handle = model;
    params.prompt = reinterpret_cast<const uint8_t*>(prompt.data());
    params.prompt_len = prompt.size();
    params.max_new_tokens = 8;
    params.seed = 7;
    params.temperature = 0;
    params.top_k = 40;
    params.top_p = 0.95f;
    params.min_p = 0.05f;
    params.repeat_last_n = 64;
    params.repeat_penalty = 1.1f;
    return params;
}

int main(int argc, char** argv) {
    CHECK(argc == 2);
    const int32_t backend = selected_backend();
    CHECK(backend >= LLW_BACKEND_CPU && backend <= LLW_BACKEND_VULKAN);
    Events events;
    llw_runtime_create_params_t create{};
    create.struct_size = sizeof(create);
    create.callbacks.struct_size = sizeof(create.callbacks);
    create.callbacks.on_event = collect_backend_event;
    create.callbacks.user_data = &events;
    create.scheduler.struct_size = sizeof(create.scheduler);
    create.scheduler.slot_count = 2;
    create.scheduler.request_queue_capacity = 4;
    create.scheduler.event_queue_capacity = 1024;
    llw_error_t error{};
    error.struct_size = sizeof(error);
    llw_runtime_t* runtime{};
    CHECK(llw_runtime_create(&create, &runtime, &error) == LLW_OK);

    const std::string path = argv[1];
    llw_model_load_params_t model_params{};
    model_params.struct_size = sizeof(model_params);
    model_params.path_utf8 = reinterpret_cast<const uint8_t*>(path.data());
    model_params.path_len = path.size();
    model_params.backend = backend;
    model_params.device_index = 0;
    model_params.context_tokens_per_slot = 512;
    model_params.logical_batch_tokens = 128;
    model_params.physical_batch_tokens = 64;
    const unsigned hardware = std::thread::hardware_concurrency();
    model_params.n_threads = static_cast<int32_t>(std::clamp(hardware == 0 ? 1u : hardware, 1u, 8u));
    model_params.n_threads_batch = model_params.n_threads;
    model_params.n_gpu_layers = backend == LLW_BACKEND_CPU ? 0 : -1;
    model_params.use_mmap = 1;
    llw_handle_t model{};
    CHECK(llw_model_load(runtime, &model_params, &model, &error) == LLW_OK);
    CHECK(model != 0);

    const std::string first_prompt = "Once";
    const std::string second_prompt = "The";
    llw_request_params_t first_params = generation(model, first_prompt);
    llw_request_params_t second_params = generation(model, second_prompt);
    llw_handle_t first{};
    llw_handle_t second{};
    CHECK(llw_request_submit(runtime, &first_params, &first, &error) == LLW_OK);
    CHECK(llw_request_submit(runtime, &second_params, &second, &error) == LLW_OK);

    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(60);
    for (;;) {
        {
            std::unique_lock lock(events.mutex);
            if (events.requests[first].terminals == 1 && events.requests[second].terminals == 1) break;
            if (events.changed.wait_until(lock, deadline) == std::cv_status::timeout) {
                lock.unlock();
                llw_request_cancel(runtime, first, &error);
                llw_request_cancel(runtime, second, &error);
                CHECK(false);
            }
        }
    }
    {
        std::lock_guard lock(events.mutex);
        CHECK(!events.requests[first].bytes.empty());
        CHECK(!events.requests[second].bytes.empty());
        CHECK(events.requests[first].terminals == 1);
        CHECK(events.requests[second].terminals == 1);
        CHECK(!events.requests[first].error && !events.requests[second].error);
    }

    std::string oversized_prompt;
    oversized_prompt.reserve(32768);
    for (size_t index = 0; index < 32768; ++index)
        oversized_prompt.push_back(static_cast<char>('!' + (index % 90)));
    const std::string isolated_prompt = "Healthy peer";
    llw_request_params_t oversized_params = generation(model, oversized_prompt);
    llw_request_params_t isolated_params = generation(model, isolated_prompt);
    llw_handle_t oversized{}, isolated{};
    CHECK(llw_request_submit(runtime, &oversized_params, &oversized, &error) == LLW_OK);
    CHECK(llw_request_submit(runtime, &isolated_params, &isolated, &error) == LLW_OK);
    const auto isolation_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(60);
    {
        std::unique_lock lock(events.mutex);
        CHECK(events.changed.wait_until(lock, isolation_deadline, [&] {
            return events.requests[oversized].terminals == 1 &&
                   events.requests[isolated].terminals == 1;
        }));
        CHECK(events.requests[oversized].error);
        CHECK(!events.requests[isolated].error);
        CHECK(!events.requests[isolated].bytes.empty());
    }
    llw_metrics_t metrics{};
    metrics.struct_size = sizeof(metrics);
    CHECK(llw_get_metrics(runtime, &metrics, &error) == LLW_OK);
    CHECK(metrics.prompt_tokens > 0);
    CHECK(metrics.generated_tokens > 0);
    CHECK(metrics.decode_calls > 0);
    CHECK(llw_model_unload(runtime, model, &error) == LLW_OK);

#ifdef LLW_RUNTIME_TESTING
    LLWTestFailNextPublishOfType(runtime, LLW_EVENT_LOG);
    llw_handle_t failed_model{99};
    CHECK(llw_model_load(runtime, &model_params, &failed_model, &error) == LLW_ERR_INTERNAL);
    CHECK(failed_model == 0);
    llw_scheduler_snapshot_t failed_snapshot{};
    failed_snapshot.struct_size = sizeof(failed_snapshot);
    CHECK(llw_get_scheduler_snapshot(runtime, &failed_snapshot, &error) == LLW_ERR_INVALID_STATE);
    CHECK(llw_model_unload(runtime, model, &error) == LLW_ERR_NOT_FOUND);
    {
        std::lock_guard lock(events.mutex);
        events.block_callbacks = true;
        events.callback_entered = false;
        events.release_callbacks = false;
        events.flush_barrier_enqueued = false;
    }
#endif
    llw_handle_t lifecycle_model{};
    CHECK(llw_model_load(runtime, &model_params, &lifecycle_model, &error) == LLW_OK);
#ifdef LLW_RUNTIME_TESTING
    {
        std::unique_lock lock(events.mutex);
        CHECK(events.changed.wait_for(lock, std::chrono::seconds(10), [&] {
            return events.callback_entered;
        }));
    }
#endif
    llw_request_params_t long_params = generation(lifecycle_model, first_prompt);
    long_params.max_new_tokens = 1024;
    llw_handle_t active_one{}, active_two{}, queued{};
    CHECK(llw_request_submit(runtime, &long_params, &active_one, &error) == LLW_OK);
    long_params.seed = 8;
    CHECK(llw_request_submit(runtime, &long_params, &active_two, &error) == LLW_OK);
    long_params.seed = 9;
    CHECK(llw_request_submit(runtime, &long_params, &queued, &error) == LLW_OK);
    const auto lifecycle_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(10);
    for (;;) {
        llw_scheduler_snapshot_t snapshot{};
        snapshot.struct_size = sizeof(snapshot);
        CHECK(llw_get_scheduler_snapshot(runtime, &snapshot, &error) == LLW_OK);
        if (snapshot.active_count >= 1 && snapshot.queued_count >= 1) break;
        CHECK(std::chrono::steady_clock::now() < lifecycle_deadline);
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    llw_result_t unload_result{LLW_ERR_INTERNAL};
#ifdef LLW_RUNTIME_TESTING
    LLWTestSetFlushEnqueuedHook(runtime, mark_flush_barrier_enqueued, &events);
    std::atomic<bool> unload_done{false};
    std::thread unload_thread([&] {
        llw_error_t unload_error{};
        unload_error.struct_size = sizeof(unload_error);
        unload_result = llw_model_unload(runtime, lifecycle_model, &unload_error);
        unload_done.store(true, std::memory_order_release);
    });
    llw_error_t cancel_error{};
    cancel_error.struct_size = sizeof(cancel_error);
    const llw_result_t cancel_result = llw_request_cancel(runtime, active_one, &cancel_error);
    bool flush_barrier_was_enqueued{};
    {
        std::unique_lock lock(events.mutex);
        flush_barrier_was_enqueued = events.changed.wait_for(
            lock, std::chrono::seconds(30), [&] { return events.flush_barrier_enqueued; });
    }
    const bool unload_returned_while_callback_blocked =
        unload_done.load(std::memory_order_acquire);
    {
        std::lock_guard lock(events.mutex);
        events.release_callbacks = true;
    }
    events.changed.notify_all();
    unload_thread.join();
    CHECK(flush_barrier_was_enqueued);
    CHECK(!unload_returned_while_callback_blocked);
#else
    llw_result_t cancel_result{LLW_ERR_INTERNAL};
    std::thread cancel_thread([&] {
        llw_error_t cancel_error{};
        cancel_error.struct_size = sizeof(cancel_error);
        cancel_result = llw_request_cancel(runtime, active_one, &cancel_error);
    });
    unload_result = llw_model_unload(runtime, lifecycle_model, &error);
    cancel_thread.join();
#endif
    CHECK(unload_result == LLW_OK);
    CHECK(cancel_result == LLW_OK || cancel_result == LLW_ERR_INVALID_STATE ||
          cancel_result == LLW_ERR_NOT_FOUND);
    const auto terminal_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(30);
    {
        std::unique_lock lock(events.mutex);
        CHECK(events.changed.wait_until(lock, terminal_deadline, [&] {
            return events.requests[active_one].terminals == 1 &&
                   events.requests[active_two].terminals == 1 &&
                   events.requests[queued].terminals == 1;
        }));
    }
    CHECK(llw_request_cancel(runtime, queued, &error) == LLW_ERR_INVALID_STATE);
    llw_runtime_destroy(runtime);
    return 0;
}

