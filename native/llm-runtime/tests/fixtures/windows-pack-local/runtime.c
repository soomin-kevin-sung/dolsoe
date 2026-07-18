#include "llw_runtime.h"

__declspec(dllimport) const char* llw_pack_local_helper_version(void);

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_get_abi_info(
    const llw_abi_query_t* query,
    llw_abi_info_t* out_info,
    llw_error_t* out_error) {
    (void)out_error;
    if (!query || !out_info || query->struct_size < sizeof(*query) ||
        out_info->struct_size < sizeof(*out_info)) {
        return LLW_ERR_INVALID_ARGUMENT;
    }
    out_info->abi_major = LLW_ABI_MAJOR;
    out_info->abi_minor = LLW_ABI_MINOR;
    out_info->min_supported_major = LLW_ABI_MAJOR;
    out_info->min_supported_minor = 0u;
    return LLW_OK;
}

LLW_EXTERN_C LLW_EXPORT const char* LLW_CALL llw_runtime_version(void) {
    return llw_pack_local_helper_version();
}

LLW_EXTERN_C LLW_EXPORT const char* LLW_CALL llw_llama_cpp_commit(void) {
    return "fixture";
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_create(
    const llw_runtime_create_params_t* params,
    llw_runtime_t** out_runtime,
    llw_error_t* out_error) {
    (void)params;
    (void)out_error;
    if (out_runtime) {
        *out_runtime = 0;
    }
    return LLW_ERR_INVALID_ARGUMENT;
}

LLW_EXTERN_C LLW_EXPORT void LLW_CALL llw_runtime_destroy(llw_runtime_t* runtime) {
    (void)runtime;
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_get_capabilities(
    llw_runtime_t* runtime,
    llw_capabilities_t* out_capabilities,
    llw_error_t* out_error) {
    (void)runtime;
    (void)out_capabilities;
    (void)out_error;
    return LLW_ERR_INVALID_ARGUMENT;
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_list_devices(
    llw_runtime_t* runtime,
    int32_t backend,
    llw_device_list_t* out_devices,
    llw_error_t* out_error) {
    (void)runtime;
    (void)backend;
    (void)out_devices;
    (void)out_error;
    return LLW_ERR_INVALID_ARGUMENT;
}
