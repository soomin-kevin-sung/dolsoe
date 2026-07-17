#include "llw_runtime.h"

#include <cassert>
#include <cstddef>
#include <cstdint>
#include <type_traits>

#define LLW_ASSERT_LAYOUT(type, expected_size) \
    static_assert(sizeof(type) == expected_size); \
    static_assert(alignof(type) == 8u)

#define LLW_ASSERT_FIELD(type, field, expected_type, expected_offset) \
    static_assert(std::is_same_v<decltype(type::field), expected_type>); \
    static_assert(offsetof(type, field) == expected_offset)

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
    assert(info.struct_size >= sizeof(std::uint32_t));

    llw_abi_query_t query{};
    query.struct_size = sizeof(query);
    query.requested_major = LLW_ABI_MAJOR;
    query.requested_minor = LLW_ABI_MINOR;
    llw_error_t error{};
    error.struct_size = sizeof(error);
    assert(llw_get_abi_info(&query, &info, &error) == LLW_OK);
    assert(info.abi_major == LLW_ABI_MAJOR);
    assert(info.abi_minor == LLW_ABI_MINOR);

    llw_runtime_create_params_t create{};
    create.struct_size = sizeof(create);
    llw_runtime_t* runtime = nullptr;
    assert(llw_runtime_create(&create, &runtime, &error) == LLW_OK);
    assert(runtime != nullptr);

    llw_capabilities_t capabilities{};
    capabilities.struct_size = sizeof(capabilities);
    assert(llw_runtime_get_capabilities(runtime, &capabilities, &error) == LLW_OK);
    assert(capabilities.supports_cpu == 1u);
    assert(capabilities.max_parallel_slots == 4u);

    llw_device_list_t devices{};
    devices.struct_size = sizeof(devices);
    assert(llw_runtime_list_devices(runtime, LLW_BACKEND_CPU, &devices, &error) == LLW_ERR_BUFFER_TOO_SMALL);
    assert(devices.required_count == 1u);

    llw_device_info_t storage[1]{};
    storage[0].struct_size = sizeof(llw_device_info_t);
    devices.capacity = 1u;
    devices.devices = storage;
    devices.element_size = sizeof(llw_device_info_t);
    assert(llw_runtime_list_devices(runtime, LLW_BACKEND_CPU, &devices, &error) == LLW_OK);
    assert(devices.count == 1u);
    assert(storage[0].backend == LLW_BACKEND_CPU);

    llw_runtime_destroy(runtime);
    return 0;
}
