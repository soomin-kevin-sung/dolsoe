#include "llw_runtime.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <type_traits>

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

int main() {
    static_assert(sizeof(void*) == 8);
    static_assert(LLW_ABI_MAJOR == 1u);
    static_assert(LLW_ABI_MINOR == 1u);
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
    LLW_ASSERT_FIELD(llw_request_params_t, reserved, std::uint64_t[12], 96u);
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

    CHECK(std::strcmp(llw_runtime_version(), "0.1.0-fake") == 0);
    CHECK(std::strcmp(llw_llama_cpp_commit(), "not-linked") == 0);
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

    // Callback copying is not externally observable through the current opaque seven-export ABI.
    reset_error();
    CHECK(llw_runtime_create(&create, &runtime, &error) == LLW_OK);
    CHECK(runtime != nullptr);

    llw_capabilities_t capabilities{};
    capabilities.struct_size = sizeof(capabilities);
    CHECK(llw_runtime_get_capabilities(runtime, &capabilities, &error) == LLW_OK);
    CHECK(capabilities.supports_cpu == 1u);
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
    CHECK(devices.required_count == 1u);

    llw_device_info_t storage[1]{};
    storage[0].struct_size = sizeof(llw_device_info_t);
    devices.capacity = 1u;
    devices.devices = storage;
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
    devices.devices = &undersized_device;
    devices.element_size = sizeof(llw_device_info_t);
    reset_error();
    CHECK(llw_runtime_list_devices(runtime, LLW_BACKEND_CPU, &devices, &error) ==
          LLW_ERR_INVALID_ARGUMENT);
    CHECK(error.code == LLW_ERR_INVALID_ARGUMENT);
    CHECK(error.message[sizeof(error.message) - 1u] == '\0');
    CHECK(std::strcmp(error.message, "device element struct_size is too small") == 0);
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

    storage[0] = {};
    storage[0].struct_size = sizeof(llw_device_info_t);
    devices.devices = storage;
    devices.element_size = sizeof(llw_device_info_t);
    reset_error();
    CHECK(llw_runtime_list_devices(runtime, LLW_BACKEND_CPU, &devices, &error) == LLW_OK);
    CHECK(devices.count == 1u);
    CHECK(storage[0].backend == LLW_BACKEND_CPU);
    CHECK(std::strcmp(storage[0].id, "cpu:0") == 0);

    std::uint8_t option_storage[1]{0xffu};
    llw_buffer_t option_schema{};
    option_schema.struct_size = sizeof(option_schema);
    option_schema.data = option_storage;
    option_schema.capacity = sizeof(option_storage);
    option_schema.len = 1u;
    reset_error();
    CHECK(llw_runtime_get_option_schema(runtime, &option_schema, &error) == LLW_ERR_UNSUPPORTED);
    CHECK(option_schema.len == 0u);
    CHECK(error.code == LLW_ERR_UNSUPPORTED);

    llw_model_load_params_t load_params{};
    load_params.struct_size = sizeof(load_params);
    llw_handle_t model = 123u;
    reset_error();
    CHECK(llw_model_load(runtime, &load_params, &model, &error) == LLW_ERR_UNSUPPORTED);
    CHECK(model == 0u);
    CHECK(error.code == LLW_ERR_UNSUPPORTED);

    reset_error();
    CHECK(llw_model_unload(runtime, 123u, &error) == LLW_ERR_UNSUPPORTED);
    CHECK(error.code == LLW_ERR_UNSUPPORTED);

    llw_request_params_t request_params{};
    request_params.struct_size = sizeof(request_params);
    llw_handle_t request = 456u;
    reset_error();
    CHECK(llw_request_submit(runtime, &request_params, &request, &error) == LLW_ERR_UNSUPPORTED);
    CHECK(request == 0u);
    CHECK(error.code == LLW_ERR_UNSUPPORTED);

    reset_error();
    CHECK(llw_request_cancel(runtime, 456u, &error) == LLW_ERR_UNSUPPORTED);
    CHECK(error.code == LLW_ERR_UNSUPPORTED);

    llw_scheduler_snapshot_t snapshot{};
    snapshot.struct_size = sizeof(snapshot);
    snapshot.flags = 1u;
    snapshot.slot_count = 1u;
    snapshot.active_count = 1u;
    snapshot.queued_count = 1u;
    snapshot.queue_capacity = 1u;
    snapshot.accepted_requests = 1u;
    snapshot.terminal_requests = 1u;
    std::fill_n(snapshot.reserved, 8u, std::uint64_t{1u});
    reset_error();
    CHECK(llw_get_scheduler_snapshot(runtime, &snapshot, &error) == LLW_ERR_UNSUPPORTED);
    CHECK(snapshot.flags == 0u);
    CHECK(snapshot.slot_count == 0u);
    CHECK(snapshot.active_count == 0u);
    CHECK(snapshot.queued_count == 0u);
    CHECK(snapshot.queue_capacity == 0u);
    CHECK(snapshot.accepted_requests == 0u);
    CHECK(snapshot.terminal_requests == 0u);
    CHECK(std::all_of(snapshot.reserved, snapshot.reserved + 8u,
                      [](std::uint64_t value) { return value == 0u; }));
    CHECK(error.code == LLW_ERR_UNSUPPORTED);

    llw_metrics_t metrics{};
    metrics.struct_size = sizeof(metrics);
    metrics.flags = 1u;
    metrics.prompt_tokens = 1u;
    metrics.generated_tokens = 1u;
    metrics.decode_calls = 1u;
    metrics.cancelled_requests = 1u;
    metrics.failed_requests = 1u;
    metrics.queue_wait_ns = 1u;
    metrics.decode_ns = 1u;
    std::fill_n(metrics.reserved, 8u, std::uint64_t{1u});
    reset_error();
    CHECK(llw_get_metrics(runtime, &metrics, &error) == LLW_ERR_UNSUPPORTED);
    CHECK(metrics.flags == 0u);
    CHECK(metrics.prompt_tokens == 0u);
    CHECK(metrics.generated_tokens == 0u);
    CHECK(metrics.decode_calls == 0u);
    CHECK(metrics.cancelled_requests == 0u);
    CHECK(metrics.failed_requests == 0u);
    CHECK(metrics.queue_wait_ns == 0u);
    CHECK(metrics.decode_ns == 0u);
    CHECK(std::all_of(metrics.reserved, metrics.reserved + 8u,
                      [](std::uint64_t value) { return value == 0u; }));
    CHECK(error.code == LLW_ERR_UNSUPPORTED);

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
    return 0;
}
