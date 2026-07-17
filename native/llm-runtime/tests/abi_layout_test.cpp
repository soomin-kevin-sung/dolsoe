#include "llw_runtime.h"

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
    static_assert(LLW_ABI_MINOR == 0u);
    static_assert(sizeof(llw_handle_t) == sizeof(std::uint64_t));
    static_assert(sizeof(llw_result_t) == sizeof(std::int32_t));

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

    LLW_ASSERT_LAYOUT(llw_runtime_create_params_t, 160u);
    LLW_ASSERT_FIELD(llw_runtime_create_params_t, struct_size, std::uint32_t, 0u);
    LLW_ASSERT_FIELD(llw_runtime_create_params_t, flags, std::uint32_t, 4u);
    LLW_ASSERT_FIELD(llw_runtime_create_params_t, callbacks, llw_callback_table_t, 8u);
    LLW_ASSERT_FIELD(llw_runtime_create_params_t, reserved, std::uint64_t[8], 96u);

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
    undersized_create.struct_size = sizeof(undersized_create) - 1u;
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

    llw_runtime_destroy(runtime);
    llw_runtime_destroy(nullptr);
    return 0;
}
