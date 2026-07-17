#ifndef LLW_RUNTIME_H
#define LLW_RUNTIME_H

#include <stddef.h>
#include <stdint.h>

#if UINTPTR_MAX != UINT64_MAX
#error "Local LLM runtime ABI supports only 64-bit targets"
#endif

#ifdef _WIN32
#define LLW_CALL __cdecl
#ifdef LLW_RUNTIME_BUILD
#define LLW_EXPORT __declspec(dllexport)
#else
#define LLW_EXPORT __declspec(dllimport)
#endif
#else
#define LLW_CALL
#ifdef LLW_RUNTIME_BUILD
#define LLW_EXPORT __attribute__((visibility("default")))
#else
#define LLW_EXPORT
#endif
#endif

#ifdef __cplusplus
#define LLW_EXTERN_C extern "C"
#else
#define LLW_EXTERN_C
#endif

#define LLW_ABI_MAJOR 1u
#define LLW_ABI_MINOR 0u

typedef uint64_t llw_handle_t;
typedef int32_t llw_result_t;

#define LLW_OK ((llw_result_t)0)
#define LLW_ERR_INVALID_ARGUMENT ((llw_result_t)1)
#define LLW_ERR_ABI_MISMATCH ((llw_result_t)2)
#define LLW_ERR_BUFFER_TOO_SMALL ((llw_result_t)3)
#define LLW_ERR_INTERNAL ((llw_result_t)1000)

#define LLW_BACKEND_AUTO ((int32_t)0)
#define LLW_BACKEND_CPU ((int32_t)1)
#define LLW_BACKEND_CUDA ((int32_t)2)
#define LLW_BACKEND_VULKAN ((int32_t)3)

#define LLW_EVENT_MODEL_PROGRESS ((int32_t)1)
#define LLW_EVENT_QUEUED ((int32_t)2)
#define LLW_EVENT_TOKEN ((int32_t)3)
#define LLW_EVENT_METRICS ((int32_t)4)
#define LLW_EVENT_DONE ((int32_t)5)
#define LLW_EVENT_CANCELLED ((int32_t)6)
#define LLW_EVENT_ERROR ((int32_t)7)
#define LLW_EVENT_LOG ((int32_t)8)

/*
 * Before passing any input or output structure, zero-initialize the entire
 * structure and set struct_size to sizeof(structure type). Flags, reserved
 * fields, and fields unknown to the caller must remain zero unless documented.
 */
#pragma pack(push, 8)

typedef struct llw_error_t {
    uint32_t struct_size;
    int32_t code;
    uint32_t flags;
    char message[512];
    uint64_t reserved[8];
} llw_error_t;

typedef struct llw_abi_query_t {
    uint32_t struct_size;
    uint32_t flags;
    uint32_t requested_major;
    uint32_t requested_minor;
    uint64_t reserved[8];
} llw_abi_query_t;

typedef struct llw_abi_info_t {
    uint32_t struct_size;
    uint32_t flags;
    uint32_t abi_major;
    uint32_t abi_minor;
    uint32_t min_supported_major;
    uint32_t min_supported_minor;
    uint64_t feature_flags;
    uint64_t reserved[8];
} llw_abi_info_t;

typedef struct llw_capabilities_t {
    uint32_t struct_size;
    uint32_t flags;
    uint32_t supports_cpu;
    uint32_t supports_cuda;
    uint32_t supports_vulkan;
    uint32_t supports_streaming;
    uint32_t supports_cancellation;
    uint32_t max_parallel_slots;
    uint64_t reserved[8];
} llw_capabilities_t;

typedef struct llw_device_info_t {
    uint32_t struct_size;
    uint32_t flags;
    int32_t backend;
    uint32_t device_index;
    char id[64];
    char name[128];
    char vendor[64];
    uint64_t reserved[8];
} llw_device_info_t;

/*
 * Device enumeration uses two calls. For the size query, set devices to NULL
 * and capacity to zero; required_count receives the total number of matching
 * devices and LLW_ERR_BUFFER_TOO_SMALL is returned when storage is required.
 * For the fill call, devices points to a caller-owned array, capacity is its
 * element count, and element_size is sizeof(llw_device_info_t). The runtime
 * writes at most capacity entries, sets count to the entries written and
 * required_count to the total required, and never retains devices.
 */
typedef struct llw_device_list_t {
    uint32_t struct_size;
    uint32_t flags;
    uint32_t capacity;
    uint32_t count;
    uint32_t element_size;
    uint32_t reserved0;
    llw_device_info_t* devices;
    uint64_t required_count;
    uint64_t reserved[8];
} llw_device_list_t;

/*
 * The event and event->data are valid only for the duration of the callback;
 * copy data before returning to retain it. Callbacks may run on
 * runtime-managed threads, so consumers must be thread-safe. Do not call
 * runtime functions reentrantly from a callback unless explicitly documented.
 */
typedef struct llw_event_t {
    uint32_t struct_size;
    uint32_t flags;
    int32_t event_type;
    int32_t error_code;
    llw_handle_t model_handle;
    llw_handle_t request_handle;
    uint32_t slot_id;
    uint32_t reserved0;
    uint64_t sequence_number;
    const uint8_t* data;
    uint64_t data_len;
    void* request_user_data;
    uint64_t reserved[8];
} llw_event_t;

typedef void(LLW_CALL* llw_event_callback_t)(const llw_event_t* event, void* user_data);

typedef struct llw_callback_table_t {
    uint32_t struct_size;
    uint32_t flags;
    llw_event_callback_t on_event;
    void* user_data;
    uint64_t reserved[8];
} llw_callback_table_t;

typedef struct llw_runtime_create_params_t {
    uint32_t struct_size;
    uint32_t flags;
    llw_callback_table_t callbacks;
    uint64_t reserved[8];
} llw_runtime_create_params_t;

#pragma pack(pop)

typedef struct llw_runtime_t llw_runtime_t;

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_get_abi_info(
    const llw_abi_query_t* query,
    llw_abi_info_t* out_info,
    llw_error_t* out_error);
/* Returned strings are runtime-owned, static, UTF-8, null-terminated, and valid while the DLL is loaded. */
LLW_EXTERN_C LLW_EXPORT const char* LLW_CALL llw_runtime_version(void);
LLW_EXTERN_C LLW_EXPORT const char* LLW_CALL llw_llama_cpp_commit(void);
/* On success, out_runtime receives a caller-owned handle; destroy it exactly once. */
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_create(
    const llw_runtime_create_params_t* params,
    llw_runtime_t** out_runtime,
    llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT void LLW_CALL llw_runtime_destroy(llw_runtime_t* runtime);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_get_capabilities(
    llw_runtime_t* runtime,
    llw_capabilities_t* out_capabilities,
    llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_list_devices(
    llw_runtime_t* runtime,
    int32_t backend,
    llw_device_list_t* out_devices,
    llw_error_t* out_error);

#endif
