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
#define LLW_ABI_MINOR 1u

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

#define LLW_ERR_BUSY ((llw_result_t)4)
#define LLW_ERR_QUEUE_FULL ((llw_result_t)5)
#define LLW_ERR_NOT_FOUND ((llw_result_t)6)
#define LLW_ERR_INVALID_STATE ((llw_result_t)7)
#define LLW_ERR_CANCELLED ((llw_result_t)8)
#define LLW_ERR_UNSUPPORTED ((llw_result_t)9)

#define LLW_EVENT_DATA_NONE ((uint32_t)0)
#define LLW_EVENT_DATA_BYTES ((uint32_t)1)
#define LLW_EVENT_DATA_UTF8 ((uint32_t)2)
#define LLW_EVENT_DATA_JSON_UTF8 ((uint32_t)3)

#define LLW_REQUEST_STATE_QUEUED ((int32_t)1)
#define LLW_REQUEST_STATE_PREPROCESSING ((int32_t)2)
#define LLW_REQUEST_STATE_RUNNING ((int32_t)3)
#define LLW_REQUEST_STATE_DONE ((int32_t)4)
#define LLW_REQUEST_STATE_CANCELLED ((int32_t)5)
#define LLW_REQUEST_STATE_ERROR ((int32_t)6)

#define LLW_MAX_SLOTS 4u
#define LLW_MAX_QUEUE_CAPACITY 1024u
#define LLW_MAX_EVENT_QUEUE_CAPACITY 65536u
#define LLW_MAX_MODEL_PATH_BYTES 32768u
#define LLW_MAX_DEVICE_INDEX 255u
#define LLW_MAX_PROMPT_BYTES (16u * 1024u * 1024u)
#define LLW_MAX_STOP_SEQUENCES 8u
#define LLW_MAX_STOP_BYTES 256u
#define LLW_MAX_STOP_TOTAL_BYTES 2048u

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

typedef struct llw_bytes_t {
    uint32_t struct_size;
    uint32_t flags;
    const uint8_t* data;
    uint64_t len;
    uint64_t reserved[8];
} llw_bytes_t;

typedef struct llw_buffer_t {
    uint32_t struct_size;
    uint32_t flags;
    uint8_t* data;
    uint64_t capacity;
    uint64_t len;
    uint64_t reserved[8];
} llw_buffer_t;

typedef struct llw_scheduler_config_t {
    uint32_t struct_size;
    uint32_t flags;
    uint32_t slot_count; /* 1..4 */
    uint32_t request_queue_capacity; /* 1..1024 */
    uint32_t event_queue_capacity; /* 16..65536 */
    uint32_t reserved0;
    uint64_t reserved[8];
} llw_scheduler_config_t;

typedef struct llw_runtime_create_params_t {
    uint32_t struct_size;
    uint32_t flags;
    llw_callback_table_t callbacks;
    uint64_t reserved[8];
    llw_scheduler_config_t scheduler;
    uint64_t reserved_v1[8];
} llw_runtime_create_params_t;

typedef struct llw_model_load_params_t {
    uint32_t struct_size;
    uint32_t flags;
    const uint8_t* path_utf8; /* 1..32768 UTF-8 bytes with no NUL */
    uint64_t path_len;
    int32_t backend; /* AUTO, CPU, CUDA, or VULKAN */
    uint32_t device_index; /* 0..255 */
    uint32_t context_tokens_per_slot; /* 512..262144 */
    uint32_t logical_batch_tokens; /* 1..8192 */
    uint32_t physical_batch_tokens; /* 1..logical_batch_tokens */
    int32_t n_threads; /* 1..256 */
    int32_t n_threads_batch; /* 1..256 */
    int32_t n_gpu_layers; /* -1..65535 */
    uint32_t use_mmap;
    uint32_t use_mlock;
    uint32_t check_tensors;
    uint32_t reserved0;
    uint64_t reserved[12];
} llw_model_load_params_t;

typedef struct llw_request_params_t {
    uint32_t struct_size;
    uint32_t flags;
    llw_handle_t model_handle;
    const uint8_t* prompt; /* 1..LLW_MAX_PROMPT_BYTES */
    uint64_t prompt_len;
    uint32_t max_new_tokens; /* 1..1048576 */
    uint32_t seed;
    float temperature; /* 0.0..10.0 */
    int32_t top_k; /* 0..100000 */
    float top_p; /* 0.0..1.0 */
    float min_p; /* 0.0..1.0 */
    int32_t repeat_last_n; /* 0..262144 */
    float repeat_penalty; /* 0.0..10.0 */
    float frequency_penalty; /* -2.0..2.0 */
    float presence_penalty; /* -2.0..2.0 */
    uint32_t stop_count; /* 0..8 */
    uint32_t reserved0;
    const llw_bytes_t* stop_sequences; /* each 1..256 bytes; combined 0..2048 bytes */
    void* request_user_data;
    uint64_t reserved[12];
} llw_request_params_t;

typedef struct llw_scheduler_snapshot_t {
    uint32_t struct_size;
    uint32_t flags;
    uint32_t slot_count;
    uint32_t active_count;
    uint32_t queued_count;
    uint32_t queue_capacity;
    uint64_t accepted_requests;
    uint64_t terminal_requests;
    uint64_t reserved[8];
} llw_scheduler_snapshot_t;

typedef struct llw_metrics_t {
    uint32_t struct_size;
    uint32_t flags;
    uint64_t prompt_tokens;
    uint64_t generated_tokens;
    uint64_t decode_calls;
    uint64_t cancelled_requests;
    uint64_t failed_requests;
    uint64_t queue_wait_ns;
    uint64_t decode_ns;
    uint64_t reserved[8];
} llw_metrics_t;

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

/*
 * All input byte pointers are borrowed only for the call. llw_request_submit copies the prompt,
 * stop arrays, stop bytes, and request_user_data value before returning. Event data uses event.flags:
 * TOKEN is BYTES; LOG is UTF8; QUEUED, MODEL_PROGRESS, METRICS, DONE, CANCELLED, and ERROR are
 * JSON_UTF8. event and data are valid only during the callback and must be copied before return.
 * Only the dispatcher thread invokes on_event; callbacks are serialized, may not call llw_* reentrantly,
 * and must not block indefinitely. Each accepted request emits increasing sequence_number values and
 * exactly one of DONE, CANCELLED, or ERROR. After that terminal event is copied into the bounded event
 * queue and sequence cleanup completes, the scheduler erases the request and later
 * llw_request_cancel calls for that handle deterministically return LLW_ERR_NOT_FOUND.
 * DONE JSON uses `reason:"stop"` for EOS/configured-stop completion and `reason:"length"` when the
 * effective per-slot generation budget is exhausted; per-slot length completion is not an ERROR.
 * The caller must externally exclude llw_runtime_destroy from every other llw_* call and callback;
 * no thread may retain or use the raw llw_runtime_t pointer once destruction begins. Load, unload,
 * submit, and cancel are internally serialized while the runtime remains alive. Under this precondition,
 * model-progress callbacks finish before unload/destroy returns and cannot outlive the runtime.
 * callback_table.user_data pointee must remain alive until llw_runtime_destroy returns.
 * request_user_data pointee must remain alive from an accepted llw_request_submit until its terminal callback returns.
 * On submit failure, the runtime retains neither request_user_data nor its pointee.
 * llw_runtime_destroy is a quiescence barrier for every callback: no callback is executing or can begin
 * after it returns. Successful llw_model_unload is a quiescence barrier for that model's progress callbacks.
 * A successful model load may have pending MODEL_PROGRESS callbacks; successful unload or runtime destroy
 * waits for them. Failed model load and failed model unload return only after callbacks started by that call have completed.
 */
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_get_option_schema(
    llw_runtime_t* runtime, llw_buffer_t* out_json, llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_model_load(
    llw_runtime_t* runtime, const llw_model_load_params_t* params,
    llw_handle_t* out_model, llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_model_unload(
    llw_runtime_t* runtime, llw_handle_t model, llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_request_submit(
    llw_runtime_t* runtime, const llw_request_params_t* params,
    llw_handle_t* out_request, llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_request_cancel(
    llw_runtime_t* runtime, llw_handle_t request, llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_get_scheduler_snapshot(
    llw_runtime_t* runtime, llw_scheduler_snapshot_t* out_snapshot,
    llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_get_metrics(
    llw_runtime_t* runtime, llw_metrics_t* out_metrics, llw_error_t* out_error);

#endif
