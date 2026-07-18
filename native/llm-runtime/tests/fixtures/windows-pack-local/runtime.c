#include "llw_runtime.h"

#include <string.h>

const char* llw_pack_local_version(void);

LLW_EXPORT llw_result_t LLW_CALL llw_get_abi_info(
    const llw_abi_query_t* query, llw_abi_info_t* out_info, llw_error_t* out_error) {
    (void)out_error;
    if (query == NULL || out_info == NULL) {
        return LLW_ERR_INVALID_ARGUMENT;
    }
    out_info->abi_major = LLW_ABI_MAJOR;
    out_info->abi_minor = LLW_ABI_MINOR;
    return LLW_OK;
}

LLW_EXPORT const char* LLW_CALL llw_runtime_version(void) {
    return llw_pack_local_version();
}

LLW_EXPORT const char* LLW_CALL llw_llama_cpp_commit(void) { return "fixture"; }

LLW_EXPORT llw_result_t LLW_CALL llw_runtime_create(
    const llw_runtime_create_params_t* params, llw_runtime_t** out_runtime,
    llw_error_t* out_error) {
    (void)params;
    (void)out_runtime;
    (void)out_error;
    return LLW_ERR_UNSUPPORTED;
}

LLW_EXPORT void LLW_CALL llw_runtime_destroy(llw_runtime_t* runtime) { (void)runtime; }

LLW_EXPORT llw_result_t LLW_CALL llw_runtime_get_capabilities(
    llw_runtime_t* runtime, llw_capabilities_t* out_capabilities, llw_error_t* out_error) {
    (void)runtime;
    (void)out_capabilities;
    (void)out_error;
    return LLW_ERR_UNSUPPORTED;
}

LLW_EXPORT llw_result_t LLW_CALL llw_runtime_list_devices(
    llw_runtime_t* runtime, int32_t backend, llw_device_list_t* out_devices,
    llw_error_t* out_error) {
    (void)runtime;
    (void)backend;
    (void)out_devices;
    (void)out_error;
    return LLW_ERR_UNSUPPORTED;
}

LLW_EXPORT llw_result_t LLW_CALL llw_runtime_get_option_schema(
    llw_runtime_t* runtime, llw_buffer_t* out_json, llw_error_t* out_error) {
    (void)runtime;
    (void)out_json;
    (void)out_error;
    return LLW_ERR_UNSUPPORTED;
}

LLW_EXPORT llw_result_t LLW_CALL llw_model_load(
    llw_runtime_t* runtime, const llw_model_load_params_t* params,
    llw_handle_t* out_model, llw_error_t* out_error) {
    (void)runtime;
    (void)params;
    (void)out_model;
    (void)out_error;
    return LLW_ERR_UNSUPPORTED;
}

LLW_EXPORT llw_result_t LLW_CALL llw_model_unload(
    llw_runtime_t* runtime, llw_handle_t model, llw_error_t* out_error) {
    (void)runtime;
    (void)model;
    (void)out_error;
    return LLW_ERR_UNSUPPORTED;
}

LLW_EXPORT llw_result_t LLW_CALL llw_request_submit(
    llw_runtime_t* runtime, const llw_request_params_t* params,
    llw_handle_t* out_request, llw_error_t* out_error) {
    (void)runtime;
    (void)params;
    (void)out_request;
    (void)out_error;
    return LLW_ERR_UNSUPPORTED;
}

LLW_EXPORT llw_result_t LLW_CALL llw_request_cancel(
    llw_runtime_t* runtime, llw_handle_t request, llw_error_t* out_error) {
    (void)runtime;
    (void)request;
    (void)out_error;
    return LLW_ERR_UNSUPPORTED;
}

LLW_EXPORT llw_result_t LLW_CALL llw_get_scheduler_snapshot(
    llw_runtime_t* runtime, llw_scheduler_snapshot_t* out_snapshot,
    llw_error_t* out_error) {
    (void)runtime;
    (void)out_snapshot;
    (void)out_error;
    return LLW_ERR_UNSUPPORTED;
}

LLW_EXPORT llw_result_t LLW_CALL llw_get_metrics(
    llw_runtime_t* runtime, llw_metrics_t* out_metrics, llw_error_t* out_error) {
    (void)runtime;
    (void)out_metrics;
    (void)out_error;
    return LLW_ERR_UNSUPPORTED;
}
