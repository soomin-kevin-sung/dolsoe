#include "llw_runtime.h"

#include <algorithm>
#include <chrono>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <future>
#include <mutex>
#include <string>
#include <type_traits>
#include <vector>

#define LLW_ASSERT_LAYOUT(type, expected_size) \
    static_assert(sizeof(type) == expected_size); \
    static_assert(alignof(type) == 8u)

#define LLW_ASSERT_FIELD(type, field, expected_type, expected_offset) \
    static_assert(std::is_same_v<decltype(type::field), expected_type>); \
    static_assert(offsetof(type, field) == expected_offset)

#define CHECK(condition) \
    do { \
        if (!(condition)) { \
            std::fprintf(stderr, "%s:%d: CHECK failed: %s\n", __FILE__, __LINE__, #condition); \
            return 1; \
        } \
    } while (false)

#ifdef LLW_RUNTIME_TESTING
LLW_EXTERN_C LLW_EXPORT void LLW_CALL LLWTestSetEngineDestroyHook(
    llw_runtime_t* runtime, void (LLW_CALL *hook)(void*), void* user_data);
LLW_EXTERN_C LLW_EXPORT void LLW_CALL LLWTestFailNextUnloadBeforeTransition(
    llw_runtime_t* runtime);
#endif

int test_v11_exports() {
    llw_error_t error{};
    error.struct_size = sizeof(error);
    llw_runtime_create_params_t create{};
    create.struct_size = sizeof(create);
    create.callbacks.struct_size = sizeof(create.callbacks);
    create.scheduler.struct_size = sizeof(create.scheduler);
    create.scheduler.slot_count = 2;
    create.scheduler.request_queue_capacity = 2;
    create.scheduler.event_queue_capacity = 32;
    llw_runtime_t* runtime{};
    CHECK(llw_runtime_create(&create, &runtime, &error) == LLW_OK);
    CHECK(runtime != nullptr);

    llw_buffer_t schema{};
    schema.struct_size = sizeof(schema);
    CHECK(llw_runtime_get_option_schema(runtime, &schema, &error) == LLW_ERR_BUFFER_TOO_SMALL);
    CHECK(schema.len > 0);
    std::vector<uint8_t> schema_bytes(static_cast<size_t>(schema.len));
    schema.data = schema_bytes.data();
    schema.capacity = schema_bytes.size();
    CHECK(llw_runtime_get_option_schema(runtime, &schema, &error) == LLW_OK);
    const std::string schema_text(schema_bytes.begin(), schema_bytes.end());
    CHECK(schema_text.find("\"eventQueueCapacity\"") != std::string::npos);
    CHECK(schema_text.find("\"nThreadsBatch\"") != std::string::npos);
    CHECK(schema_text.find("\"maxTotalBytes\":2048") != std::string::npos);

    llw_request_params_t request{};
    request.struct_size = sizeof(request);
    request.model_handle = 1;
    const uint8_t prompt[] = {'x'};
    request.prompt = prompt;
    request.prompt_len = sizeof(prompt);
    request.max_new_tokens = 1;
    request.temperature = 0;
    request.top_p = 1;
    request.repeat_penalty = 1;
    llw_handle_t request_handle{99};
    CHECK(llw_request_submit(runtime, &request, &request_handle, &error) == LLW_ERR_INVALID_STATE);
    CHECK(request_handle == 0);
    CHECK(llw_model_unload(runtime, 1, &error) == LLW_ERR_NOT_FOUND);

    const std::string missing_model = "llw-test-bad-alloc.gguf";
    llw_model_load_params_t failing_model{};
    failing_model.struct_size = sizeof(failing_model);
    failing_model.path_utf8 = reinterpret_cast<const uint8_t*>(missing_model.data());
    failing_model.path_len = missing_model.size();
    failing_model.backend = LLW_BACKEND_CPU;
    failing_model.context_tokens_per_slot = 512;
    failing_model.logical_batch_tokens = 64;
    failing_model.physical_batch_tokens = 64;
    failing_model.n_threads = 1;
    failing_model.n_threads_batch = 1;
    failing_model.use_mmap = 1;
    llw_handle_t failed_model{};
    CHECK(llw_model_load(runtime, &failing_model, &failed_model, &error) != LLW_OK);
    CHECK(failed_model == 0);
    const llw_result_t retry = llw_model_load(runtime, &failing_model, &failed_model, &error);
    CHECK(retry != LLW_ERR_BUSY);
    CHECK(retry != LLW_OK);

    llw_scheduler_config_t undersized{};
    undersized.struct_size = sizeof(undersized) - 1;
    create.scheduler = undersized;
    llw_runtime_t* rejected{};
    CHECK(llw_runtime_create(&create, &rejected, &error) == LLW_ERR_INVALID_ARGUMENT);
    CHECK(rejected == nullptr);
    llw_runtime_destroy(runtime);
    llw_runtime_destroy(nullptr);
    return 0;
}

int test_v11_reserved_validation() {
    llw_error_t error{};
    error.struct_size = sizeof(error);
    llw_abi_query_t query{};
    query.struct_size = sizeof(query);
    query.requested_major = LLW_ABI_MAJOR;
    llw_abi_info_t info{};
    info.struct_size = sizeof(info);
    query.flags = 1;
    CHECK(llw_get_abi_info(&query, &info, &error) == LLW_ERR_INVALID_ARGUMENT);
    query.flags = 0;
    query.reserved[0] = 1;
    CHECK(llw_get_abi_info(&query, &info, &error) == LLW_ERR_INVALID_ARGUMENT);
    query.reserved[0] = 0;
    info.flags = 1;
    CHECK(llw_get_abi_info(&query, &info, &error) == LLW_ERR_INVALID_ARGUMENT);

    llw_runtime_create_params_t create{};
    create.struct_size = sizeof(create);
    create.callbacks.struct_size = sizeof(create.callbacks);
    create.scheduler.struct_size = sizeof(create.scheduler);
    create.scheduler.slot_count = 1;
    create.scheduler.request_queue_capacity = 1;
    create.scheduler.event_queue_capacity = 16;
    llw_runtime_t* runtime{};
    CHECK(llw_runtime_create(&create, &runtime, &error) == LLW_OK);

    llw_capabilities_t capabilities{};
    capabilities.struct_size = sizeof(capabilities);
    capabilities.reserved[0] = 1;
    CHECK(llw_runtime_get_capabilities(runtime, &capabilities, &error) ==
          LLW_ERR_INVALID_ARGUMENT);

    llw_device_list_t devices{};
    devices.struct_size = sizeof(devices);
    devices.reserved0 = 1;
    CHECK(llw_runtime_list_devices(runtime, LLW_BACKEND_CPU, &devices, &error) ==
          LLW_ERR_INVALID_ARGUMENT);

    llw_buffer_t schema{};
    schema.struct_size = sizeof(schema);
    schema.reserved[0] = 1;
    CHECK(llw_runtime_get_option_schema(runtime, &schema, &error) ==
          LLW_ERR_INVALID_ARGUMENT);

    llw_scheduler_snapshot_t snapshot{};
    snapshot.struct_size = sizeof(snapshot);
    snapshot.flags = 1;
    CHECK(llw_get_scheduler_snapshot(runtime, &snapshot, &error) == LLW_ERR_INVALID_ARGUMENT);

    llw_metrics_t metrics{};
    metrics.struct_size = sizeof(metrics);
    metrics.reserved[0] = 1;
    CHECK(llw_get_metrics(runtime, &metrics, &error) == LLW_ERR_INVALID_ARGUMENT);

    llw_runtime_destroy(runtime);
    return 0;
}

struct BlockingProgressCallback {
    std::mutex mutex;
    std::condition_variable changed;
    bool entered{};
    bool released{};
};

void LLW_CALL block_model_progress(const llw_event_t* event, void* user_data) {
    if (event->event_type != LLW_EVENT_MODEL_PROGRESS) return;
    auto& callback = *static_cast<BlockingProgressCallback*>(user_data);
    std::unique_lock lock(callback.mutex);
    callback.entered = true;
    callback.changed.notify_all();
    callback.changed.wait(lock, [&callback] { return callback.released; });
}

int test_failed_load_callback_quiescence() {
    using namespace std::chrono_literals;
    BlockingProgressCallback callback;
    llw_runtime_create_params_t create{};
    create.struct_size = sizeof(create);
    create.callbacks.struct_size = sizeof(create.callbacks);
    create.callbacks.on_event = block_model_progress;
    create.callbacks.user_data = &callback;
    create.scheduler.struct_size = sizeof(create.scheduler);
    create.scheduler.slot_count = 1;
    create.scheduler.request_queue_capacity = 1;
    create.scheduler.event_queue_capacity = 16;
    llw_error_t error{};
    error.struct_size = sizeof(error);
    llw_runtime_t* runtime{};
    CHECK(llw_runtime_create(&create, &runtime, &error) == LLW_OK);

    const std::string path = "llw-test-progress-then-bad-alloc.gguf";
    llw_model_load_params_t model{};
    model.struct_size = sizeof(model);
    model.path_utf8 = reinterpret_cast<const uint8_t*>(path.data());
    model.path_len = path.size();
    model.backend = LLW_BACKEND_CPU;
    model.context_tokens_per_slot = 512;
    model.logical_batch_tokens = 64;
    model.physical_batch_tokens = 64;
    model.n_threads = 1;
    model.n_threads_batch = 1;
    model.use_mmap = 1;
    llw_handle_t handle{99};
    auto load = std::async(std::launch::async, [&] {
        return llw_model_load(runtime, &model, &handle, &error);
    });
    {
        std::unique_lock lock(callback.mutex);
        CHECK(callback.changed.wait_for(lock, 5s, [&callback] { return callback.entered; }));
    }
    const bool returned_early = load.wait_for(50ms) == std::future_status::ready;
    {
        std::lock_guard lock(callback.mutex);
        callback.released = true;
    }
    callback.changed.notify_all();
    CHECK(load.get() == LLW_ERR_INTERNAL);
    CHECK(!returned_early);
    CHECK(handle == 0);
    llw_runtime_destroy(runtime);
    return 0;
}

struct SaturatedProgressCallback {
    std::mutex mutex;
    std::condition_variable changed;
    bool log_entered{};
    bool release_log{};
    size_t progress_count{};
};

void LLW_CALL saturate_progress_events(const llw_event_t* event, void* user_data) {
    auto& callback = *static_cast<SaturatedProgressCallback*>(user_data);
    std::unique_lock lock(callback.mutex);
    if (event->event_type == LLW_EVENT_LOG && !callback.log_entered) {
        callback.log_entered = true;
        callback.changed.notify_all();
        callback.changed.wait(lock, [&callback] { return callback.release_log; });
    } else if (event->event_type == LLW_EVENT_MODEL_PROGRESS) {
        ++callback.progress_count;
    }
}

int test_progress_saturation_is_bounded() {
    using namespace std::chrono_literals;
    SaturatedProgressCallback callback;
    llw_runtime_create_params_t create{};
    create.struct_size = sizeof(create);
    create.callbacks.struct_size = sizeof(create.callbacks);
    create.callbacks.on_event = saturate_progress_events;
    create.callbacks.user_data = &callback;
    create.scheduler.struct_size = sizeof(create.scheduler);
    create.scheduler.slot_count = 1;
    create.scheduler.request_queue_capacity = 1;
    create.scheduler.event_queue_capacity = 16;
    llw_error_t error{};
    error.struct_size = sizeof(error);
    llw_runtime_t* runtime{};
    CHECK(llw_runtime_create(&create, &runtime, &error) == LLW_OK);
    {
        std::unique_lock lock(callback.mutex);
        CHECK(callback.changed.wait_for(lock, 5s, [&callback] { return callback.log_entered; }));
    }

    const std::string path = "llw-test-saturate-progress.gguf";
    llw_model_load_params_t model{};
    model.struct_size = sizeof(model);
    model.path_utf8 = reinterpret_cast<const uint8_t*>(path.data());
    model.path_len = path.size();
    model.backend = LLW_BACKEND_CPU;
    model.context_tokens_per_slot = 512;
    model.logical_batch_tokens = 64;
    model.physical_batch_tokens = 64;
    model.n_threads = 1;
    model.n_threads_batch = 1;
    model.use_mmap = 1;
    llw_handle_t handle{99};
    auto load = std::async(std::launch::async, [&] {
        return llw_model_load(runtime, &model, &handle, &error);
    });
    CHECK(load.wait_for(50ms) == std::future_status::timeout);
    {
        std::lock_guard lock(callback.mutex);
        callback.release_log = true;
    }
    callback.changed.notify_all();
    CHECK(load.get() == LLW_ERR_INTERNAL);
    CHECK(handle == 0);
    {
        std::lock_guard lock(callback.mutex);
        CHECK(callback.progress_count == 16);
    }
    llw_runtime_destroy(runtime);
    return 0;
}

int test_debug_model_fixture_loads() {
    llw_runtime_create_params_t create{};
    create.struct_size = sizeof(create);
    create.callbacks.struct_size = sizeof(create.callbacks);
    create.scheduler.struct_size = sizeof(create.scheduler);
    create.scheduler.slot_count = 1;
    create.scheduler.request_queue_capacity = 1;
    create.scheduler.event_queue_capacity = 16;
    llw_error_t error{};
    error.struct_size = sizeof(error);
    llw_runtime_t* runtime{};
    CHECK(llw_runtime_create(&create, &runtime, &error) == LLW_OK);
    const std::string path = "llw-test-model.gguf";
    llw_model_load_params_t model{};
    model.struct_size = sizeof(model);
    model.path_utf8 = reinterpret_cast<const uint8_t*>(path.data());
    model.path_len = path.size();
    model.backend = LLW_BACKEND_CPU;
    model.context_tokens_per_slot = 512;
    model.logical_batch_tokens = 64;
    model.physical_batch_tokens = 64;
    model.n_threads = 1;
    model.n_threads_batch = 1;
    model.use_mmap = 1;
    llw_handle_t handle{};
    CHECK(llw_model_load(runtime, &model, &handle, &error) == LLW_OK);
    CHECK(handle != 0);
    CHECK(llw_model_unload(runtime, handle, &error) == LLW_OK);
    llw_runtime_destroy(runtime);
    return 0;
}

#ifdef LLW_RUNTIME_TESTING
struct LifecycleBarrier {
    llw_runtime_t* runtime{};
    std::mutex mutex;
    std::condition_variable changed;
    bool entered{};
    bool released{};
    llw_result_t snapshot_result{LLW_OK};
};

void LLW_CALL block_engine_destruction(void* user_data) {
    auto& barrier = *static_cast<LifecycleBarrier*>(user_data);
    llw_scheduler_snapshot_t snapshot{};
    snapshot.struct_size = sizeof(snapshot);
    llw_error_t error{};
    error.struct_size = sizeof(error);
    const llw_result_t snapshot_result =
        llw_get_scheduler_snapshot(barrier.runtime, &snapshot, &error);
    std::unique_lock lock(barrier.mutex);
    barrier.snapshot_result = snapshot_result;
    barrier.entered = true;
    barrier.changed.notify_all();
    barrier.changed.wait(lock, [&barrier] { return barrier.released; });
}

int test_unload_serializes_concurrent_load() {
    using namespace std::chrono_literals;
    llw_runtime_create_params_t create{};
    create.struct_size = sizeof(create);
    create.callbacks.struct_size = sizeof(create.callbacks);
    create.scheduler.struct_size = sizeof(create.scheduler);
    create.scheduler.slot_count = 1;
    create.scheduler.request_queue_capacity = 1;
    create.scheduler.event_queue_capacity = 16;
    llw_error_t create_error{};
    create_error.struct_size = sizeof(create_error);
    llw_runtime_t* runtime{};
    CHECK(llw_runtime_create(&create, &runtime, &create_error) == LLW_OK);
    const std::string path = "llw-test-model.gguf";
    llw_model_load_params_t model{};
    model.struct_size = sizeof(model);
    model.path_utf8 = reinterpret_cast<const uint8_t*>(path.data());
    model.path_len = path.size();
    model.backend = LLW_BACKEND_CPU;
    model.context_tokens_per_slot = 512;
    model.logical_batch_tokens = 64;
    model.physical_batch_tokens = 64;
    model.n_threads = 1;
    model.n_threads_batch = 1;
    model.use_mmap = 1;
    llw_error_t first_load_error{};
    first_load_error.struct_size = sizeof(first_load_error);
    llw_handle_t first_model{};
    CHECK(llw_model_load(runtime, &model, &first_model, &first_load_error) == LLW_OK);

    LifecycleBarrier barrier;
    barrier.runtime = runtime;
    LLWTestSetEngineDestroyHook(runtime, block_engine_destruction, &barrier);
    llw_error_t unload_error{};
    unload_error.struct_size = sizeof(unload_error);
    auto unload = std::async(std::launch::async, [&] {
        return llw_model_unload(runtime, first_model, &unload_error);
    });
    {
        std::unique_lock lock(barrier.mutex);
        CHECK(barrier.changed.wait_for(lock, 5s, [&barrier] { return barrier.entered; }));
    }

    llw_error_t second_load_error{};
    second_load_error.struct_size = sizeof(second_load_error);
    llw_handle_t second_model{};
    auto load = std::async(std::launch::async, [&] {
        return llw_model_load(runtime, &model, &second_model, &second_load_error);
    });
    const bool returned_while_old_engine_alive =
        load.wait_for(50ms) == std::future_status::ready;
    {
        std::lock_guard lock(barrier.mutex);
        barrier.released = true;
    }
    barrier.changed.notify_all();
    CHECK(unload.get() == LLW_OK);
    CHECK(load.get() == LLW_OK);
    CHECK(!returned_while_old_engine_alive);
    CHECK(barrier.snapshot_result == LLW_ERR_INVALID_STATE);
    CHECK(second_model != 0 && second_model != first_model);
    LLWTestSetEngineDestroyHook(runtime, nullptr, nullptr);
    CHECK(llw_model_unload(runtime, second_model, &unload_error) == LLW_OK);
    llw_runtime_destroy(runtime);
    return 0;
}

int test_unload_failure_is_retryable() {
    llw_runtime_create_params_t create{};
    create.struct_size = sizeof(create);
    create.callbacks.struct_size = sizeof(create.callbacks);
    create.scheduler.struct_size = sizeof(create.scheduler);
    create.scheduler.slot_count = 1;
    create.scheduler.request_queue_capacity = 1;
    create.scheduler.event_queue_capacity = 16;
    llw_error_t error{};
    error.struct_size = sizeof(error);
    llw_runtime_t* runtime{};
    CHECK(llw_runtime_create(&create, &runtime, &error) == LLW_OK);
    const std::string path = "llw-test-model.gguf";
    llw_model_load_params_t model{};
    model.struct_size = sizeof(model);
    model.path_utf8 = reinterpret_cast<const uint8_t*>(path.data());
    model.path_len = path.size();
    model.backend = LLW_BACKEND_CPU;
    model.context_tokens_per_slot = 512;
    model.logical_batch_tokens = 64;
    model.physical_batch_tokens = 64;
    model.n_threads = 1;
    model.n_threads_batch = 1;
    model.use_mmap = 1;
    llw_handle_t handle{};
    CHECK(llw_model_load(runtime, &model, &handle, &error) == LLW_OK);

    LLWTestFailNextUnloadBeforeTransition(runtime);
    CHECK(llw_model_unload(runtime, handle, &error) == LLW_ERR_INTERNAL);
    llw_scheduler_snapshot_t snapshot{};
    snapshot.struct_size = sizeof(snapshot);
    CHECK(llw_get_scheduler_snapshot(runtime, &snapshot, &error) == LLW_OK);
    llw_handle_t second{};
    CHECK(llw_model_load(runtime, &model, &second, &error) == LLW_ERR_BUSY);
    CHECK(second == 0);
    CHECK(llw_model_unload(runtime, handle, &error) == LLW_OK);
    llw_runtime_destroy(runtime);
    return 0;
}
#endif

struct TerminalCallbackBarrier {
    std::mutex mutex;
    std::condition_variable changed;
    bool entered{};
    bool released{};
};

void LLW_CALL block_terminal_callback(const llw_event_t* event, void* user_data) {
    if (event->event_type != LLW_EVENT_DONE && event->event_type != LLW_EVENT_CANCELLED &&
        event->event_type != LLW_EVENT_ERROR) return;
    auto& barrier = *static_cast<TerminalCallbackBarrier*>(user_data);
    std::unique_lock lock(barrier.mutex);
    barrier.entered = true;
    barrier.changed.notify_all();
    barrier.changed.wait(lock, [&barrier] { return barrier.released; });
}

int test_unload_waits_for_terminal_callback_quiescence() {
    using namespace std::chrono_literals;
    TerminalCallbackBarrier barrier;
    llw_runtime_create_params_t create{};
    create.struct_size = sizeof(create);
    create.callbacks.struct_size = sizeof(create.callbacks);
    create.callbacks.on_event = block_terminal_callback;
    create.callbacks.user_data = &barrier;
    create.scheduler.struct_size = sizeof(create.scheduler);
    create.scheduler.slot_count = 1;
    create.scheduler.request_queue_capacity = 1;
    create.scheduler.event_queue_capacity = 16;
    llw_error_t error{};
    error.struct_size = sizeof(error);
    llw_runtime_t* runtime{};
    CHECK(llw_runtime_create(&create, &runtime, &error) == LLW_OK);
    const std::string path = "llw-test-model.gguf";
    llw_model_load_params_t model{};
    model.struct_size = sizeof(model);
    model.path_utf8 = reinterpret_cast<const uint8_t*>(path.data());
    model.path_len = path.size();
    model.backend = LLW_BACKEND_CPU;
    model.context_tokens_per_slot = 512;
    model.logical_batch_tokens = 64;
    model.physical_batch_tokens = 64;
    model.n_threads = 1;
    model.n_threads_batch = 1;
    model.use_mmap = 1;
    llw_handle_t model_handle{};
    CHECK(llw_model_load(runtime, &model, &model_handle, &error) == LLW_OK);
    const uint8_t prompt[] = {'x'};
    llw_request_params_t request{};
    request.struct_size = sizeof(request);
    request.model_handle = model_handle;
    request.prompt = prompt;
    request.prompt_len = sizeof(prompt);
    request.max_new_tokens = 1;
    request.top_p = 1;
    request.repeat_penalty = 1;
    llw_handle_t request_handle{};
    CHECK(llw_request_submit(runtime, &request, &request_handle, &error) == LLW_OK);
    {
        std::unique_lock lock(barrier.mutex);
        CHECK(barrier.changed.wait_for(lock, 5s, [&barrier] { return barrier.entered; }));
    }
    auto unload = std::async(std::launch::async, [&] {
        llw_error_t unload_error{};
        unload_error.struct_size = sizeof(unload_error);
        return llw_model_unload(runtime, model_handle, &unload_error);
    });
    const bool unload_waited_for_callback =
        unload.wait_for(50ms) == std::future_status::timeout;
    {
        std::lock_guard lock(barrier.mutex);
        barrier.released = true;
    }
    barrier.changed.notify_all();
    CHECK(unload.get() == LLW_OK);
    CHECK(unload_waited_for_callback);
    llw_runtime_destroy(runtime);
    return 0;
}

struct ReentrantUnloadCallback {
    llw_runtime_t* runtime{};
    std::mutex mutex;
    std::condition_variable changed;
    bool called{};
    llw_result_t result{LLW_OK};
};

void LLW_CALL attempt_reentrant_unload(const llw_event_t* event, void* user_data) {
    auto& callback = *static_cast<ReentrantUnloadCallback*>(user_data);
    if (event->event_type != LLW_EVENT_LOG || event->model_handle == 0 || !callback.runtime)
        return;
    llw_error_t error{};
    error.struct_size = sizeof(error);
    const llw_result_t result =
        llw_model_unload(callback.runtime, event->model_handle, &error);
    {
        std::lock_guard lock(callback.mutex);
        callback.called = true;
        callback.result = result;
    }
    callback.changed.notify_all();
}

int test_callback_reentrant_unload_is_rejected_before_transition() {
    using namespace std::chrono_literals;
    ReentrantUnloadCallback callback;
    llw_runtime_create_params_t create{};
    create.struct_size = sizeof(create);
    create.callbacks.struct_size = sizeof(create.callbacks);
    create.callbacks.on_event = attempt_reentrant_unload;
    create.callbacks.user_data = &callback;
    create.scheduler.struct_size = sizeof(create.scheduler);
    create.scheduler.slot_count = 1;
    create.scheduler.request_queue_capacity = 1;
    create.scheduler.event_queue_capacity = 16;
    llw_error_t error{};
    error.struct_size = sizeof(error);
    CHECK(llw_runtime_create(&create, &callback.runtime, &error) == LLW_OK);
    const std::string path = "llw-test-model.gguf";
    llw_model_load_params_t model{};
    model.struct_size = sizeof(model);
    model.path_utf8 = reinterpret_cast<const uint8_t*>(path.data());
    model.path_len = path.size();
    model.backend = LLW_BACKEND_CPU;
    model.context_tokens_per_slot = 512;
    model.logical_batch_tokens = 64;
    model.physical_batch_tokens = 64;
    model.n_threads = 1;
    model.n_threads_batch = 1;
    model.use_mmap = 1;
    llw_handle_t handle{};
    CHECK(llw_model_load(callback.runtime, &model, &handle, &error) == LLW_OK);
    {
        std::unique_lock lock(callback.mutex);
        CHECK(callback.changed.wait_for(lock, 5s, [&callback] { return callback.called; }));
        CHECK(callback.result == LLW_ERR_INVALID_STATE);
    }
    llw_scheduler_snapshot_t snapshot{};
    snapshot.struct_size = sizeof(snapshot);
    CHECK(llw_get_scheduler_snapshot(callback.runtime, &snapshot, &error) == LLW_OK);
    CHECK(llw_model_unload(callback.runtime, handle, &error) == LLW_OK);
    llw_runtime_destroy(callback.runtime);
    return 0;
}

int main() {
    static_assert(sizeof(void*) == 8);
    static_assert(LLW_ABI_MAJOR == 1u);
    static_assert(LLW_ABI_MINOR == 2u);
    static_assert(sizeof(llw_handle_t) == sizeof(std::uint64_t));
    static_assert(sizeof(llw_result_t) == sizeof(std::int32_t));
    static_assert(LLW_OK == 0);
    static_assert(LLW_ERR_INVALID_ARGUMENT == 1);
    static_assert(LLW_ERR_ABI_MISMATCH == 2);
    static_assert(LLW_ERR_BUFFER_TOO_SMALL == 3);
    static_assert(LLW_ERR_BUSY == 4);
    static_assert(LLW_ERR_QUEUE_FULL == 5);
    static_assert(LLW_ERR_NOT_FOUND == 6);
    static_assert(LLW_ERR_INVALID_STATE == 7);
    static_assert(LLW_ERR_CANCELLED == 8);
    static_assert(LLW_ERR_UNSUPPORTED == 9);
    static_assert(LLW_ERR_INTERNAL == 1000);

    LLW_ASSERT_LAYOUT(llw_error_t, 592u);
    LLW_ASSERT_FIELD(llw_error_t, struct_size, std::uint32_t, 0u);
    LLW_ASSERT_FIELD(llw_error_t, code, std::int32_t, 4u);
    LLW_ASSERT_FIELD(llw_error_t, flags, std::uint32_t, 8u);
    LLW_ASSERT_FIELD(llw_error_t, message, char[512], 12u);
    LLW_ASSERT_FIELD(llw_error_t, reserved, std::uint64_t[8], 528u);

    LLW_ASSERT_LAYOUT(llw_abi_query_t, 80u);
    LLW_ASSERT_FIELD(llw_abi_query_t, struct_size, std::uint32_t, 0u);
    LLW_ASSERT_FIELD(llw_abi_query_t, flags, std::uint32_t, 4u);
    LLW_ASSERT_FIELD(llw_abi_query_t, requested_major, std::uint32_t, 8u);
    LLW_ASSERT_FIELD(llw_abi_query_t, requested_minor, std::uint32_t, 12u);
    LLW_ASSERT_FIELD(llw_abi_query_t, reserved, std::uint64_t[8], 16u);

    LLW_ASSERT_LAYOUT(llw_abi_info_t, 96u);
    LLW_ASSERT_FIELD(llw_abi_info_t, struct_size, std::uint32_t, 0u);
    LLW_ASSERT_FIELD(llw_abi_info_t, flags, std::uint32_t, 4u);
    LLW_ASSERT_FIELD(llw_abi_info_t, abi_major, std::uint32_t, 8u);
    LLW_ASSERT_FIELD(llw_abi_info_t, abi_minor, std::uint32_t, 12u);
    LLW_ASSERT_FIELD(llw_abi_info_t, min_supported_major, std::uint32_t, 16u);
    LLW_ASSERT_FIELD(llw_abi_info_t, min_supported_minor, std::uint32_t, 20u);
    LLW_ASSERT_FIELD(llw_abi_info_t, feature_flags, std::uint64_t, 24u);
    LLW_ASSERT_FIELD(llw_abi_info_t, reserved, std::uint64_t[8], 32u);

    LLW_ASSERT_LAYOUT(llw_capabilities_t, 96u);
    LLW_ASSERT_FIELD(llw_capabilities_t, struct_size, std::uint32_t, 0u);
    LLW_ASSERT_FIELD(llw_capabilities_t, flags, std::uint32_t, 4u);
    LLW_ASSERT_FIELD(llw_capabilities_t, supports_cpu, std::uint32_t, 8u);
    LLW_ASSERT_FIELD(llw_capabilities_t, supports_cuda, std::uint32_t, 12u);
    LLW_ASSERT_FIELD(llw_capabilities_t, supports_vulkan, std::uint32_t, 16u);
    LLW_ASSERT_FIELD(llw_capabilities_t, supports_streaming, std::uint32_t, 20u);
    LLW_ASSERT_FIELD(llw_capabilities_t, supports_cancellation, std::uint32_t, 24u);
    LLW_ASSERT_FIELD(llw_capabilities_t, max_parallel_slots, std::uint32_t, 28u);
    LLW_ASSERT_FIELD(llw_capabilities_t, reserved, std::uint64_t[8], 32u);

    LLW_ASSERT_LAYOUT(llw_device_info_t, 336u);
    LLW_ASSERT_FIELD(llw_device_info_t, struct_size, std::uint32_t, 0u);
    LLW_ASSERT_FIELD(llw_device_info_t, flags, std::uint32_t, 4u);
    LLW_ASSERT_FIELD(llw_device_info_t, backend, std::int32_t, 8u);
    LLW_ASSERT_FIELD(llw_device_info_t, device_index, std::uint32_t, 12u);
    LLW_ASSERT_FIELD(llw_device_info_t, id, char[64], 16u);
    LLW_ASSERT_FIELD(llw_device_info_t, name, char[128], 80u);
    LLW_ASSERT_FIELD(llw_device_info_t, vendor, char[64], 208u);
    LLW_ASSERT_FIELD(llw_device_info_t, reserved, std::uint64_t[8], 272u);

    LLW_ASSERT_LAYOUT(llw_device_list_t, 104u);
    LLW_ASSERT_FIELD(llw_device_list_t, struct_size, std::uint32_t, 0u);
    LLW_ASSERT_FIELD(llw_device_list_t, flags, std::uint32_t, 4u);
    LLW_ASSERT_FIELD(llw_device_list_t, capacity, std::uint32_t, 8u);
    LLW_ASSERT_FIELD(llw_device_list_t, count, std::uint32_t, 12u);
    LLW_ASSERT_FIELD(llw_device_list_t, element_size, std::uint32_t, 16u);
    LLW_ASSERT_FIELD(llw_device_list_t, reserved0, std::uint32_t, 20u);
    LLW_ASSERT_FIELD(llw_device_list_t, devices, llw_device_info_t*, 24u);
    LLW_ASSERT_FIELD(llw_device_list_t, required_count, std::uint64_t, 32u);
    LLW_ASSERT_FIELD(llw_device_list_t, reserved, std::uint64_t[8], 40u);

    LLW_ASSERT_LAYOUT(llw_event_t, 136u);
    LLW_ASSERT_FIELD(llw_event_t, struct_size, std::uint32_t, 0u);
    LLW_ASSERT_FIELD(llw_event_t, flags, std::uint32_t, 4u);
    LLW_ASSERT_FIELD(llw_event_t, event_type, std::int32_t, 8u);
    LLW_ASSERT_FIELD(llw_event_t, error_code, std::int32_t, 12u);
    LLW_ASSERT_FIELD(llw_event_t, model_handle, llw_handle_t, 16u);
    LLW_ASSERT_FIELD(llw_event_t, request_handle, llw_handle_t, 24u);
    LLW_ASSERT_FIELD(llw_event_t, slot_id, std::uint32_t, 32u);
    LLW_ASSERT_FIELD(llw_event_t, reserved0, std::uint32_t, 36u);
    LLW_ASSERT_FIELD(llw_event_t, sequence_number, std::uint64_t, 40u);
    LLW_ASSERT_FIELD(llw_event_t, data, const std::uint8_t*, 48u);
    LLW_ASSERT_FIELD(llw_event_t, data_len, std::uint64_t, 56u);
    LLW_ASSERT_FIELD(llw_event_t, request_user_data, void*, 64u);
    LLW_ASSERT_FIELD(llw_event_t, reserved, std::uint64_t[8], 72u);

    LLW_ASSERT_LAYOUT(llw_callback_table_t, 88u);
    LLW_ASSERT_FIELD(llw_callback_table_t, struct_size, std::uint32_t, 0u);
    LLW_ASSERT_FIELD(llw_callback_table_t, flags, std::uint32_t, 4u);
    LLW_ASSERT_FIELD(llw_callback_table_t, on_event, llw_event_callback_t, 8u);
    LLW_ASSERT_FIELD(llw_callback_table_t, user_data, void*, 16u);
    LLW_ASSERT_FIELD(llw_callback_table_t, reserved, std::uint64_t[8], 24u);

    LLW_ASSERT_LAYOUT(llw_runtime_create_params_t, 312u);
    LLW_ASSERT_FIELD(llw_runtime_create_params_t, struct_size, std::uint32_t, 0u);
    LLW_ASSERT_FIELD(llw_runtime_create_params_t, flags, std::uint32_t, 4u);
    LLW_ASSERT_FIELD(llw_runtime_create_params_t, callbacks, llw_callback_table_t, 8u);
    LLW_ASSERT_FIELD(llw_runtime_create_params_t, reserved, std::uint64_t[8], 96u);
    static_assert(sizeof(llw_bytes_t) == 88u);
    static_assert(sizeof(llw_chat_message_t) == 248u);
    static_assert(sizeof(llw_buffer_t) == 96u);
    static_assert(sizeof(llw_scheduler_config_t) == 88u);
    static_assert(sizeof(llw_model_load_params_t) == 168u);
    static_assert(sizeof(llw_request_params_t) == 192u);
    static_assert(sizeof(llw_scheduler_snapshot_t) == 104u);
    static_assert(sizeof(llw_metrics_t) == 128u);
    static_assert(offsetof(llw_runtime_create_params_t, scheduler) == 160u);
    static_assert(sizeof(llw_runtime_create_params_t) == 312u);
    LLW_ASSERT_FIELD(llw_bytes_t, data, const std::uint8_t*, 8u);
    LLW_ASSERT_FIELD(llw_bytes_t, len, std::uint64_t, 16u);
    LLW_ASSERT_FIELD(llw_bytes_t, reserved, std::uint64_t[8], 24u);
    LLW_ASSERT_FIELD(llw_chat_message_t, role, llw_bytes_t, 8u);
    LLW_ASSERT_FIELD(llw_chat_message_t, content, llw_bytes_t, 96u);
    LLW_ASSERT_FIELD(llw_chat_message_t, reserved, std::uint64_t[8], 184u);
    LLW_ASSERT_FIELD(llw_buffer_t, data, std::uint8_t*, 8u);
    LLW_ASSERT_FIELD(llw_buffer_t, capacity, std::uint64_t, 16u);
    LLW_ASSERT_FIELD(llw_buffer_t, len, std::uint64_t, 24u);
    LLW_ASSERT_FIELD(llw_scheduler_config_t, slot_count, std::uint32_t, 8u);
    LLW_ASSERT_FIELD(llw_scheduler_config_t, request_queue_capacity, std::uint32_t, 12u);
    LLW_ASSERT_FIELD(llw_scheduler_config_t, event_queue_capacity, std::uint32_t, 16u);
    LLW_ASSERT_FIELD(llw_scheduler_config_t, reserved, std::uint64_t[8], 24u);
    LLW_ASSERT_FIELD(llw_model_load_params_t, path_utf8, const std::uint8_t*, 8u);
    LLW_ASSERT_FIELD(llw_model_load_params_t, path_len, std::uint64_t, 16u);
    LLW_ASSERT_FIELD(llw_model_load_params_t, backend, std::int32_t, 24u);
    LLW_ASSERT_FIELD(llw_model_load_params_t, context_tokens_per_slot, std::uint32_t, 32u);
    LLW_ASSERT_FIELD(llw_model_load_params_t, n_gpu_layers, std::int32_t, 52u);
    LLW_ASSERT_FIELD(llw_model_load_params_t, reserved, std::uint64_t[12], 72u);
    LLW_ASSERT_FIELD(llw_request_params_t, model_handle, llw_handle_t, 8u);
    LLW_ASSERT_FIELD(llw_request_params_t, prompt, const std::uint8_t*, 16u);
    LLW_ASSERT_FIELD(llw_request_params_t, max_new_tokens, std::uint32_t, 32u);
    LLW_ASSERT_FIELD(llw_request_params_t, temperature, float, 40u);
    LLW_ASSERT_FIELD(llw_request_params_t, stop_sequences, const llw_bytes_t*, 80u);
    LLW_ASSERT_FIELD(llw_request_params_t, request_user_data, void*, 88u);
    LLW_ASSERT_FIELD(llw_request_params_t, chat_messages, const llw_chat_message_t*, 96u);
    LLW_ASSERT_FIELD(llw_request_params_t, chat_message_count, std::uint32_t, 104u);
    LLW_ASSERT_FIELD(llw_request_params_t, reserved, std::uint64_t[10], 112u);
    LLW_ASSERT_FIELD(llw_scheduler_snapshot_t, accepted_requests, std::uint64_t, 24u);
    LLW_ASSERT_FIELD(llw_scheduler_snapshot_t, reserved, std::uint64_t[8], 40u);
    LLW_ASSERT_FIELD(llw_metrics_t, prompt_tokens, std::uint64_t, 8u);
    LLW_ASSERT_FIELD(llw_metrics_t, decode_ns, std::uint64_t, 56u);
    LLW_ASSERT_FIELD(llw_metrics_t, reserved, std::uint64_t[8], 64u);

    llw_abi_info_t info{};
    info.struct_size = sizeof(info);
    CHECK(info.struct_size >= sizeof(std::uint32_t));
    llw_abi_query_t query{};
    query.struct_size = sizeof(query);
    query.requested_major = LLW_ABI_MAJOR;
    query.requested_minor = LLW_ABI_MINOR;
    llw_error_t error{};
    error.struct_size = sizeof(error);
    const auto reset_error = [&error]() {
        error = {};
        error.struct_size = sizeof(error);
    };

    CHECK(std::strcmp(llw_runtime_version(), "0.2.0") == 0);
    CHECK(std::strcmp(llw_llama_cpp_commit(), "571d0d540df04f25298d0e159e520d9fc62ed121") == 0);
    CHECK(llw_get_abi_info(&query, &info, &error) == LLW_OK);
    CHECK(info.abi_major == LLW_ABI_MAJOR);
    CHECK(info.abi_minor == LLW_ABI_MINOR);

    reset_error();
    CHECK(llw_get_abi_info(nullptr, &info, &error) == LLW_ERR_INVALID_ARGUMENT);
    CHECK(error.code == LLW_ERR_INVALID_ARGUMENT);
    CHECK(std::strcmp(error.message, "invalid ABI query") == 0);

    reset_error();
    CHECK(llw_get_abi_info(&query, nullptr, &error) == LLW_ERR_INVALID_ARGUMENT);

    llw_abi_query_t undersized_query{};
    undersized_query.struct_size = sizeof(undersized_query) - 1u;
    reset_error();
    CHECK(llw_get_abi_info(&undersized_query, &info, &error) == LLW_ERR_INVALID_ARGUMENT);

    llw_abi_info_t undersized_info{};
    undersized_info.struct_size = sizeof(undersized_info) - 1u;
    reset_error();
    CHECK(llw_get_abi_info(&query, &undersized_info, &error) == LLW_ERR_INVALID_ARGUMENT);

    llw_abi_query_t mismatched_query = query;
    mismatched_query.requested_major = LLW_ABI_MAJOR + 1u;
    reset_error();
    std::memset(error.message, 'x', sizeof(error.message));
    CHECK(llw_get_abi_info(&mismatched_query, &info, &error) == LLW_ERR_ABI_MISMATCH);
    CHECK(error.code == LLW_ERR_ABI_MISMATCH);
    CHECK(error.message[sizeof(error.message) - 1u] == '\0');
    CHECK(std::strcmp(error.message, "unsupported ABI major") == 0);

    llw_runtime_create_params_t create{};
    create.struct_size = sizeof(create);
    create.callbacks.struct_size = sizeof(create.callbacks);
    create.scheduler.struct_size = sizeof(create.scheduler);
    create.scheduler.slot_count = 1;
    create.scheduler.request_queue_capacity = 16;
    create.scheduler.event_queue_capacity = 32;
    llw_runtime_t* runtime = reinterpret_cast<llw_runtime_t*>(std::uintptr_t{1u});

    reset_error();
    CHECK(llw_runtime_create(nullptr, &runtime, &error) == LLW_ERR_INVALID_ARGUMENT);
    CHECK(runtime == nullptr);

    llw_runtime_create_params_t undersized_create{};
    undersized_create.struct_size = offsetof(llw_runtime_create_params_t, scheduler) - 1u;
    runtime = reinterpret_cast<llw_runtime_t*>(std::uintptr_t{1u});
    reset_error();
    CHECK(llw_runtime_create(&undersized_create, &runtime, &error) == LLW_ERR_INVALID_ARGUMENT);
    CHECK(runtime == nullptr);

    reset_error();
    CHECK(llw_runtime_create(&create, nullptr, &error) == LLW_ERR_INVALID_ARGUMENT);

    reset_error();
    CHECK(llw_runtime_create(&create, &runtime, &error) == LLW_OK);
    CHECK(runtime != nullptr);

    llw_capabilities_t capabilities{};
    capabilities.struct_size = sizeof(capabilities);
    CHECK(llw_runtime_get_capabilities(runtime, &capabilities, &error) == LLW_OK);
    CHECK(capabilities.supports_cpu == 1u);
    CHECK(capabilities.supports_streaming == 1u);
    CHECK(capabilities.supports_cancellation == 1u);
    CHECK(capabilities.max_parallel_slots == 4u);

    reset_error();
    CHECK(llw_runtime_get_capabilities(runtime, nullptr, &error) == LLW_ERR_INVALID_ARGUMENT);

    llw_capabilities_t undersized_capabilities{};
    undersized_capabilities.struct_size = sizeof(undersized_capabilities) - 1u;
    reset_error();
    CHECK(llw_runtime_get_capabilities(runtime, &undersized_capabilities, &error) ==
          LLW_ERR_INVALID_ARGUMENT);

    llw_device_list_t devices{};
    devices.struct_size = sizeof(devices);
    reset_error();
    CHECK(llw_runtime_list_devices(runtime, LLW_BACKEND_CPU, &devices, &error) ==
          LLW_ERR_BUFFER_TOO_SMALL);
    CHECK(devices.count == 0u);
    CHECK(devices.required_count >= 1u);

    std::vector<llw_device_info_t> storage(static_cast<size_t>(devices.required_count));
    for (auto& device : storage) device.struct_size = sizeof(device);
    devices.capacity = static_cast<uint32_t>(storage.size());
    devices.devices = storage.data();
    devices.element_size = sizeof(llw_device_info_t) - 1u;
    reset_error();
    CHECK(llw_runtime_list_devices(runtime, LLW_BACKEND_CPU, &devices, &error) ==
          LLW_ERR_BUFFER_TOO_SMALL);
    CHECK(devices.count == 0u);

    llw_device_info_t undersized_device{};
    std::memset(&undersized_device, 0xa5, sizeof(undersized_device));
    undersized_device.struct_size = sizeof(std::uint32_t);
    llw_device_info_t original_undersized_device{};
    std::memcpy(&original_undersized_device, &undersized_device, sizeof(undersized_device));
    devices.capacity = 1u;
    devices.devices = &undersized_device;
    devices.element_size = sizeof(llw_device_info_t);
    reset_error();
    CHECK(llw_runtime_list_devices(runtime, LLW_BACKEND_CPU, &devices, &error) ==
          LLW_ERR_INVALID_ARGUMENT);
    CHECK(error.code == LLW_ERR_INVALID_ARGUMENT);
    CHECK(error.message[sizeof(error.message) - 1u] == '\0');
    CHECK(std::strcmp(error.message, "device element is undersized") == 0);
    CHECK(std::memcmp(&undersized_device, &original_undersized_device, sizeof(undersized_device)) == 0);
    CHECK(devices.count == 0u);

    llw_device_list_t unsupported_devices{};
    unsupported_devices.struct_size = sizeof(unsupported_devices);
    unsupported_devices.count = 7u;
    unsupported_devices.required_count = 9u;
    reset_error();
    CHECK(llw_runtime_list_devices(runtime, LLW_BACKEND_CUDA, &unsupported_devices, &error) == LLW_OK);
    CHECK(unsupported_devices.count == 0u);
    CHECK(unsupported_devices.required_count == 0u);

    for (auto& device : storage) {
        device = {};
        device.struct_size = sizeof(device);
    }
    devices.capacity = static_cast<uint32_t>(storage.size());
    devices.devices = storage.data();
    devices.element_size = sizeof(llw_device_info_t);
    reset_error();
    CHECK(llw_runtime_list_devices(runtime, LLW_BACKEND_CPU, &devices, &error) == LLW_OK);
    CHECK(devices.count >= 1u);
    CHECK(storage[0].backend == LLW_BACKEND_CPU);
    CHECK(storage[0].id[0] != '\0');

    std::uint8_t option_storage[1]{0xffu};
    llw_buffer_t option_schema{};
    option_schema.struct_size = sizeof(option_schema);
    option_schema.data = option_storage;
    option_schema.capacity = sizeof(option_storage);
    option_schema.len = 1u;
    reset_error();
    CHECK(llw_runtime_get_option_schema(runtime, &option_schema, &error) ==
          LLW_ERR_BUFFER_TOO_SMALL);
    CHECK(option_schema.len > option_schema.capacity);
    CHECK(error.code == LLW_ERR_BUFFER_TOO_SMALL);

    llw_model_load_params_t load_params{};
    load_params.struct_size = sizeof(load_params);
    llw_handle_t model = 123u;
    reset_error();
    CHECK(llw_model_load(runtime, &load_params, &model, &error) == LLW_ERR_INVALID_ARGUMENT);
    CHECK(model == 0u);
    CHECK(error.code == LLW_ERR_INVALID_ARGUMENT);

    reset_error();
    CHECK(llw_model_unload(runtime, 123u, &error) == LLW_ERR_NOT_FOUND);
    CHECK(error.code == LLW_ERR_NOT_FOUND);

    llw_request_params_t request_params{};
    request_params.struct_size = sizeof(request_params);
    request_params.model_handle = 1;
    const std::uint8_t prompt[] = {'x'};
    request_params.prompt = prompt;
    request_params.prompt_len = sizeof(prompt);
    request_params.max_new_tokens = 1;
    request_params.top_p = 1;
    request_params.repeat_penalty = 1;
    llw_handle_t request = 456u;
    reset_error();
    CHECK(llw_request_submit(runtime, &request_params, &request, &error) == LLW_ERR_INVALID_STATE);
    CHECK(request == 0u);
    CHECK(error.code == LLW_ERR_INVALID_STATE);

    reset_error();
    CHECK(llw_request_cancel(runtime, 456u, &error) == LLW_ERR_INVALID_STATE);
    CHECK(error.code == LLW_ERR_INVALID_STATE);

    llw_scheduler_snapshot_t snapshot{};
    snapshot.struct_size = sizeof(snapshot);
    reset_error();
    CHECK(llw_get_scheduler_snapshot(runtime, &snapshot, &error) == LLW_ERR_INVALID_STATE);
    CHECK(error.code == LLW_ERR_INVALID_STATE);

    llw_metrics_t metrics{};
    metrics.struct_size = sizeof(metrics);
    reset_error();
    CHECK(llw_get_metrics(runtime, &metrics, &error) == LLW_ERR_INVALID_STATE);
    CHECK(error.code == LLW_ERR_INVALID_STATE);

    llw_runtime_destroy(runtime);

    llw_runtime_create_params_t legacy_create{};
    legacy_create.struct_size = offsetof(llw_runtime_create_params_t, scheduler);
    legacy_create.callbacks.struct_size = sizeof(llw_callback_table_t);
    llw_runtime_t* legacy_runtime = nullptr;
    reset_error();
    CHECK(llw_runtime_create(&legacy_create, &legacy_runtime, &error) == LLW_OK);
    CHECK(legacy_runtime != nullptr);
    llw_runtime_destroy(legacy_runtime);

    llw_runtime_destroy(nullptr);
    CHECK(test_v11_exports() == 0);
    CHECK(test_v11_reserved_validation() == 0);
    CHECK(test_failed_load_callback_quiescence() == 0);
    CHECK(test_progress_saturation_is_bounded() == 0);
    CHECK(test_debug_model_fixture_loads() == 0);
#ifdef LLW_RUNTIME_TESTING
    CHECK(test_unload_serializes_concurrent_load() == 0);
    CHECK(test_unload_failure_is_retryable() == 0);
#endif
    CHECK(test_unload_waits_for_terminal_callback_quiescence() == 0);
    CHECK(test_callback_reentrant_unload_is_rejected_before_transition() == 0);
    return 0;
}
