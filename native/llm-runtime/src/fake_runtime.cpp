#include "llw_runtime.h"

#include <algorithm>
#include <cstring>
#include <new>

struct llw_runtime_t {
    llw_callback_table_t callbacks{};
};

namespace {

llw_result_t fail(llw_error_t* error, llw_result_t code, const char* message) {
    if (error && error->struct_size >= sizeof(uint32_t) + sizeof(int32_t)) {
        error->code = code;
        if (error->struct_size >= sizeof(llw_error_t)) {
            std::strncpy(error->message, message, sizeof(error->message) - 1u);
            error->message[sizeof(error->message) - 1u] = '\0';
        }
    }
    return code;
}

void copy_text(char* destination, size_t capacity, const char* source) {
    if (capacity == 0u) {
        return;
    }
    std::strncpy(destination, source, capacity - 1u);
    destination[capacity - 1u] = '\0';
}

}  // namespace

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_get_abi_info(
    const llw_abi_query_t* query,
    llw_abi_info_t* out_info,
    llw_error_t* out_error) {
    if (!query || !out_info || query->struct_size < sizeof(llw_abi_query_t) ||
        out_info->struct_size < sizeof(llw_abi_info_t)) {
        return fail(out_error, LLW_ERR_INVALID_ARGUMENT, "invalid ABI query");
    }
    if (query->requested_major != LLW_ABI_MAJOR) {
        return fail(out_error, LLW_ERR_ABI_MISMATCH, "unsupported ABI major");
    }
    out_info->flags = 0u;
    out_info->abi_major = LLW_ABI_MAJOR;
    out_info->abi_minor = LLW_ABI_MINOR;
    out_info->min_supported_major = LLW_ABI_MAJOR;
    out_info->min_supported_minor = 0u;
    out_info->feature_flags = 0u;
    std::fill_n(out_info->reserved, 8u, uint64_t{0u});
    return LLW_OK;
}

LLW_EXTERN_C LLW_EXPORT const char* LLW_CALL llw_runtime_version(void) {
    return "0.1.0-fake";
}

LLW_EXTERN_C LLW_EXPORT const char* LLW_CALL llw_llama_cpp_commit(void) {
    return "not-linked";
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_create(
    const llw_runtime_create_params_t* params,
    llw_runtime_t** out_runtime,
    llw_error_t* out_error) {
    if (out_runtime) {
        *out_runtime = nullptr;
    }
    if (!params || !out_runtime || params->struct_size < sizeof(llw_runtime_create_params_t)) {
        return fail(out_error, LLW_ERR_INVALID_ARGUMENT, "invalid runtime create parameters");
    }
    auto* runtime = new (std::nothrow) llw_runtime_t{};
    if (!runtime) {
        return fail(out_error, LLW_ERR_INTERNAL, "runtime allocation failed");
    }
    runtime->callbacks = params->callbacks;
    *out_runtime = runtime;
    return LLW_OK;
}

LLW_EXTERN_C LLW_EXPORT void LLW_CALL llw_runtime_destroy(llw_runtime_t* runtime) {
    delete runtime;
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_get_capabilities(
    llw_runtime_t* runtime,
    llw_capabilities_t* out_capabilities,
    llw_error_t* out_error) {
    if (!runtime || !out_capabilities || out_capabilities->struct_size < sizeof(llw_capabilities_t)) {
        return fail(out_error, LLW_ERR_INVALID_ARGUMENT, "invalid capabilities output");
    }
    out_capabilities->flags = 0u;
    out_capabilities->supports_cpu = 1u;
    out_capabilities->supports_cuda = 0u;
    out_capabilities->supports_vulkan = 0u;
    out_capabilities->supports_streaming = 0u;
    out_capabilities->supports_cancellation = 0u;
    out_capabilities->max_parallel_slots = 4u;
    std::fill_n(out_capabilities->reserved, 8u, uint64_t{0u});
    return LLW_OK;
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_list_devices(
    llw_runtime_t* runtime,
    int32_t backend,
    llw_device_list_t* out_devices,
    llw_error_t* out_error) {
    if (!runtime || !out_devices || out_devices->struct_size < sizeof(llw_device_list_t)) {
        return fail(out_error, LLW_ERR_INVALID_ARGUMENT, "invalid device list output");
    }
    out_devices->count = 0u;
    if (backend != LLW_BACKEND_AUTO && backend != LLW_BACKEND_CPU) {
        out_devices->required_count = 0u;
        return LLW_OK;
    }
    out_devices->required_count = 1u;
    if (!out_devices->devices || out_devices->capacity < 1u ||
        out_devices->element_size < sizeof(llw_device_info_t)) {
        return fail(out_error, LLW_ERR_BUFFER_TOO_SMALL, "device buffer is too small");
    }
    auto& device = out_devices->devices[0];
    device = {};
    device.struct_size = sizeof(device);
    device.backend = LLW_BACKEND_CPU;
    device.device_index = 0u;
    copy_text(device.id, sizeof(device.id), "cpu:0");
    copy_text(device.name, sizeof(device.name), "Fake CPU");
    copy_text(device.vendor, sizeof(device.vendor), "Local LLM Wiki");
    out_devices->count = 1u;
    return LLW_OK;
}
