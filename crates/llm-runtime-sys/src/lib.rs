#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(target_pointer_width = "64"))]
compile_error!("llm-runtime-sys supports only 64-bit targets");

use std::ffi::{c_char, c_void};

pub const ABI_MAJOR: u32 = 1;
pub const ABI_MINOR: u32 = 2;
pub const OK: i32 = 0;
pub const ERR_INVALID_ARGUMENT: i32 = 1;
pub const ERR_ABI_MISMATCH: i32 = 2;
pub const ERR_BUFFER_TOO_SMALL: i32 = 3;
pub const ERR_BUSY: i32 = 4;
pub const ERR_QUEUE_FULL: i32 = 5;
pub const ERR_NOT_FOUND: i32 = 6;
pub const ERR_INVALID_STATE: i32 = 7;
pub const ERR_CANCELLED: i32 = 8;
pub const ERR_UNSUPPORTED: i32 = 9;
pub const ERR_INTERNAL: i32 = 1000;
pub const BACKEND_AUTO: i32 = 0;
pub const BACKEND_CPU: i32 = 1;
pub const BACKEND_CUDA: i32 = 2;
pub const BACKEND_VULKAN: i32 = 3;
pub const EVENT_DATA_NONE: u32 = 0;
pub const EVENT_DATA_BYTES: u32 = 1;
pub const EVENT_DATA_UTF8: u32 = 2;
pub const EVENT_DATA_JSON_UTF8: u32 = 3;
pub const EVENT_MODEL_PROGRESS: i32 = 1;
pub const EVENT_QUEUED: i32 = 2;
pub const EVENT_TOKEN: i32 = 3;
pub const EVENT_METRICS: i32 = 4;
pub const EVENT_DONE: i32 = 5;
pub const EVENT_CANCELLED: i32 = 6;
pub const EVENT_ERROR: i32 = 7;
pub const EVENT_LOG: i32 = 8;
pub const REQUEST_STATE_QUEUED: i32 = 1;
pub const REQUEST_STATE_PREPROCESSING: i32 = 2;
pub const REQUEST_STATE_RUNNING: i32 = 3;
pub const REQUEST_STATE_DONE: i32 = 4;
pub const REQUEST_STATE_CANCELLED: i32 = 5;
pub const REQUEST_STATE_ERROR: i32 = 6;
pub const MAX_SLOTS: u32 = 4;
pub const MAX_QUEUE_CAPACITY: u32 = 1024;
pub const MAX_EVENT_QUEUE_CAPACITY: u32 = 65_536;
pub const MAX_MODEL_PATH_BYTES: u32 = 32_768;
pub const MAX_DEVICE_INDEX: u32 = 255;
pub const MAX_PROMPT_BYTES: u32 = 16 * 1024 * 1024;
pub const MAX_STOP_SEQUENCES: u32 = 8;
pub const MAX_STOP_BYTES: u32 = 256;
pub const MAX_STOP_TOTAL_BYTES: u32 = 2048;

pub type Handle = u64;

#[repr(C)]
pub struct Runtime {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Error {
    pub struct_size: u32,
    pub code: i32,
    pub flags: u32,
    pub message: [c_char; 512],
    pub reserved: [u64; 8],
}

impl Default for Error {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            code: 0,
            flags: 0,
            message: [0; 512],
            reserved: [0; 8],
        }
    }
}

#[repr(C)]
pub struct AbiQuery {
    pub struct_size: u32,
    pub flags: u32,
    pub requested_major: u32,
    pub requested_minor: u32,
    pub reserved: [u64; 8],
}

impl Default for AbiQuery {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            flags: 0,
            requested_major: ABI_MAJOR,
            requested_minor: ABI_MINOR,
            reserved: [0; 8],
        }
    }
}

#[repr(C)]
pub struct AbiInfo {
    pub struct_size: u32,
    pub flags: u32,
    pub abi_major: u32,
    pub abi_minor: u32,
    pub min_supported_major: u32,
    pub min_supported_minor: u32,
    pub feature_flags: u64,
    pub reserved: [u64; 8],
}

impl Default for AbiInfo {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            flags: 0,
            abi_major: 0,
            abi_minor: 0,
            min_supported_major: 0,
            min_supported_minor: 0,
            feature_flags: 0,
            reserved: [0; 8],
        }
    }
}

pub type EventCallback = unsafe extern "C" fn(event: *const Event, user_data: *mut c_void);

#[repr(C)]
pub struct Event {
    pub struct_size: u32,
    pub flags: u32,
    pub event_type: i32,
    pub error_code: i32,
    pub model_handle: Handle,
    pub request_handle: Handle,
    pub slot_id: u32,
    pub reserved0: u32,
    pub sequence_number: u64,
    pub data: *const u8,
    pub data_len: u64,
    pub request_user_data: *mut c_void,
    pub reserved: [u64; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CallbackTable {
    pub struct_size: u32,
    pub flags: u32,
    pub on_event: Option<EventCallback>,
    pub user_data: *mut c_void,
    pub reserved: [u64; 8],
}

impl Default for CallbackTable {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            flags: 0,
            on_event: None,
            user_data: std::ptr::null_mut(),
            reserved: [0; 8],
        }
    }
}

#[repr(C)]
pub struct Capabilities {
    pub struct_size: u32,
    pub flags: u32,
    pub supports_cpu: u32,
    pub supports_cuda: u32,
    pub supports_vulkan: u32,
    pub supports_streaming: u32,
    pub supports_cancellation: u32,
    pub max_parallel_slots: u32,
    pub reserved: [u64; 8],
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            flags: 0,
            supports_cpu: 0,
            supports_cuda: 0,
            supports_vulkan: 0,
            supports_streaming: 0,
            supports_cancellation: 0,
            max_parallel_slots: 0,
            reserved: [0; 8],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DeviceInfo {
    pub struct_size: u32,
    pub flags: u32,
    pub backend: i32,
    pub device_index: u32,
    pub id: [c_char; 64],
    pub name: [c_char; 128],
    pub vendor: [c_char; 64],
    pub reserved: [u64; 8],
}

impl Default for DeviceInfo {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            flags: 0,
            backend: BACKEND_AUTO,
            device_index: 0,
            id: [0; 64],
            name: [0; 128],
            vendor: [0; 64],
            reserved: [0; 8],
        }
    }
}

#[repr(C)]
pub struct DeviceList {
    pub struct_size: u32,
    pub flags: u32,
    pub capacity: u32,
    pub count: u32,
    pub element_size: u32,
    pub reserved0: u32,
    pub devices: *mut DeviceInfo,
    pub required_count: u64,
    pub reserved: [u64; 8],
}

impl Default for DeviceList {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            flags: 0,
            capacity: 0,
            count: 0,
            element_size: std::mem::size_of::<DeviceInfo>() as u32,
            reserved0: 0,
            devices: std::ptr::null_mut(),
            required_count: 0,
            reserved: [0; 8],
        }
    }
}

#[repr(C)]
pub struct Bytes {
    pub struct_size: u32,
    pub flags: u32,
    pub data: *const u8,
    pub len: u64,
    pub reserved: [u64; 8],
}

#[repr(C)]
pub struct ChatMessage {
    pub struct_size: u32,
    pub flags: u32,
    pub role: Bytes,
    pub content: Bytes,
    pub reserved: [u64; 8],
}

#[repr(C)]
pub struct Buffer {
    pub struct_size: u32,
    pub flags: u32,
    pub data: *mut u8,
    pub capacity: u64,
    pub len: u64,
    pub reserved: [u64; 8],
}

#[repr(C)]
pub struct SchedulerConfig {
    pub struct_size: u32,
    pub flags: u32,
    pub slot_count: u32,
    pub request_queue_capacity: u32,
    pub event_queue_capacity: u32,
    pub reserved0: u32,
    pub reserved: [u64; 8],
}

#[repr(C)]
pub struct RuntimeCreateParams {
    pub struct_size: u32,
    pub flags: u32,
    pub callbacks: CallbackTable,
    pub reserved: [u64; 8],
    pub scheduler: SchedulerConfig,
    pub reserved_v1: [u64; 8],
}

impl Default for RuntimeCreateParams {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            flags: 0,
            callbacks: CallbackTable::default(),
            reserved: [0; 8],
            scheduler: SchedulerConfig::default(),
            reserved_v1: [0; 8],
        }
    }
}

#[repr(C)]
pub struct ModelLoadParams {
    pub struct_size: u32,
    pub flags: u32,
    pub path_utf8: *const u8,
    pub path_len: u64,
    pub backend: i32,
    pub device_index: u32,
    pub context_tokens_per_slot: u32,
    pub logical_batch_tokens: u32,
    pub physical_batch_tokens: u32,
    pub n_threads: i32,
    pub n_threads_batch: i32,
    pub n_gpu_layers: i32,
    pub use_mmap: u32,
    pub use_mlock: u32,
    pub check_tensors: u32,
    pub reserved0: u32,
    pub reserved: [u64; 12],
}

#[repr(C)]
pub struct RequestParams {
    pub struct_size: u32,
    pub flags: u32,
    pub model_handle: Handle,
    pub prompt: *const u8,
    pub prompt_len: u64,
    pub max_new_tokens: u32,
    pub seed: u32,
    pub temperature: f32,
    pub top_k: i32,
    pub top_p: f32,
    pub min_p: f32,
    pub repeat_last_n: i32,
    pub repeat_penalty: f32,
    pub frequency_penalty: f32,
    pub presence_penalty: f32,
    pub stop_count: u32,
    pub reserved0: u32,
    pub stop_sequences: *const Bytes,
    pub request_user_data: *mut c_void,
    pub chat_messages: *const ChatMessage,
    pub chat_message_count: u32,
    pub reserved1: u32,
    pub reserved: [u64; 10],
}

#[repr(C)]
pub struct SchedulerSnapshot {
    pub struct_size: u32,
    pub flags: u32,
    pub slot_count: u32,
    pub active_count: u32,
    pub queued_count: u32,
    pub queue_capacity: u32,
    pub accepted_requests: u64,
    pub terminal_requests: u64,
    pub reserved: [u64; 8],
}

#[repr(C)]
pub struct Metrics {
    pub struct_size: u32,
    pub flags: u32,
    pub prompt_tokens: u64,
    pub generated_tokens: u64,
    pub decode_calls: u64,
    pub cancelled_requests: u64,
    pub failed_requests: u64,
    pub queue_wait_ns: u64,
    pub decode_ns: u64,
    pub reserved: [u64; 8],
}

macro_rules! zero_default {
    ($type:ty, $value:expr) => {
        impl Default for $type {
            fn default() -> Self {
                $value
            }
        }
    };
}

zero_default!(
    Bytes,
    Self {
        struct_size: std::mem::size_of::<Self>() as u32,
        flags: 0,
        data: std::ptr::null(),
        len: 0,
        reserved: [0; 8],
    }
);
zero_default!(
    ChatMessage,
    Self {
        struct_size: std::mem::size_of::<Self>() as u32,
        flags: 0,
        role: Bytes::default(),
        content: Bytes::default(),
        reserved: [0; 8],
    }
);
zero_default!(
    Buffer,
    Self {
        struct_size: std::mem::size_of::<Self>() as u32,
        flags: 0,
        data: std::ptr::null_mut(),
        capacity: 0,
        len: 0,
        reserved: [0; 8],
    }
);
zero_default!(
    SchedulerConfig,
    Self {
        struct_size: std::mem::size_of::<Self>() as u32,
        flags: 0,
        slot_count: 1,
        request_queue_capacity: 16,
        event_queue_capacity: 1024,
        reserved0: 0,
        reserved: [0; 8],
    }
);
zero_default!(
    ModelLoadParams,
    Self {
        struct_size: std::mem::size_of::<Self>() as u32,
        flags: 0,
        path_utf8: std::ptr::null(),
        path_len: 0,
        backend: BACKEND_AUTO,
        device_index: 0,
        context_tokens_per_slot: 4096,
        logical_batch_tokens: 512,
        physical_batch_tokens: 128,
        n_threads: 8,
        n_threads_batch: 8,
        n_gpu_layers: 0,
        use_mmap: 1,
        use_mlock: 0,
        check_tensors: 0,
        reserved0: 0,
        reserved: [0; 12],
    }
);
zero_default!(
    RequestParams,
    Self {
        struct_size: std::mem::size_of::<Self>() as u32,
        flags: 0,
        model_handle: 0,
        prompt: std::ptr::null(),
        prompt_len: 0,
        max_new_tokens: 256,
        seed: u32::MAX,
        temperature: 0.8,
        top_k: 40,
        top_p: 0.95,
        min_p: 0.05,
        repeat_last_n: 64,
        repeat_penalty: 1.1,
        frequency_penalty: 0.0,
        presence_penalty: 0.0,
        stop_count: 0,
        reserved0: 0,
        stop_sequences: std::ptr::null(),
        request_user_data: std::ptr::null_mut(),
        chat_messages: std::ptr::null(),
        chat_message_count: 0,
        reserved1: 0,
        reserved: [0; 10],
    }
);
zero_default!(
    SchedulerSnapshot,
    Self {
        struct_size: std::mem::size_of::<Self>() as u32,
        flags: 0,
        slot_count: 0,
        active_count: 0,
        queued_count: 0,
        queue_capacity: 0,
        accepted_requests: 0,
        terminal_requests: 0,
        reserved: [0; 8],
    }
);
zero_default!(
    Metrics,
    Self {
        struct_size: std::mem::size_of::<Self>() as u32,
        flags: 0,
        prompt_tokens: 0,
        generated_tokens: 0,
        decode_calls: 0,
        cancelled_requests: 0,
        failed_requests: 0,
        queue_wait_ns: 0,
        decode_ns: 0,
        reserved: [0; 8],
    }
);

pub type GetAbiInfoFn = unsafe extern "C" fn(*const AbiQuery, *mut AbiInfo, *mut Error) -> i32;
pub type RuntimeVersionFn = unsafe extern "C" fn() -> *const c_char;
pub type LlamaCommitFn = unsafe extern "C" fn() -> *const c_char;
pub type RuntimeCreateFn =
    unsafe extern "C" fn(*const RuntimeCreateParams, *mut *mut Runtime, *mut Error) -> i32;
pub type RuntimeDestroyFn = unsafe extern "C" fn(*mut Runtime);
pub type RuntimeGetCapabilitiesFn =
    unsafe extern "C" fn(*mut Runtime, *mut Capabilities, *mut Error) -> i32;
pub type RuntimeListDevicesFn =
    unsafe extern "C" fn(*mut Runtime, i32, *mut DeviceList, *mut Error) -> i32;
pub type RuntimeGetOptionSchemaFn =
    unsafe extern "C" fn(*mut Runtime, *mut Buffer, *mut Error) -> i32;
pub type ModelLoadFn =
    unsafe extern "C" fn(*mut Runtime, *const ModelLoadParams, *mut Handle, *mut Error) -> i32;
pub type ModelUnloadFn = unsafe extern "C" fn(*mut Runtime, Handle, *mut Error) -> i32;
pub type RequestSubmitFn =
    unsafe extern "C" fn(*mut Runtime, *const RequestParams, *mut Handle, *mut Error) -> i32;
pub type RequestCancelFn = unsafe extern "C" fn(*mut Runtime, Handle, *mut Error) -> i32;
pub type GetSchedulerSnapshotFn =
    unsafe extern "C" fn(*mut Runtime, *mut SchedulerSnapshot, *mut Error) -> i32;
pub type GetMetricsFn = unsafe extern "C" fn(*mut Runtime, *mut Metrics, *mut Error) -> i32;

pub struct Api {
    _library: libloading::Library,
    pub get_abi_info: GetAbiInfoFn,
    pub runtime_version: RuntimeVersionFn,
    pub llama_commit: LlamaCommitFn,
    pub runtime_create: RuntimeCreateFn,
    pub runtime_destroy: RuntimeDestroyFn,
    pub runtime_get_capabilities: RuntimeGetCapabilitiesFn,
    pub runtime_list_devices: RuntimeListDevicesFn,
    pub runtime_get_option_schema: RuntimeGetOptionSchemaFn,
    pub model_load: ModelLoadFn,
    pub model_unload: ModelUnloadFn,
    pub request_submit: RequestSubmitFn,
    pub request_cancel: RequestCancelFn,
    pub get_scheduler_snapshot: GetSchedulerSnapshotFn,
    pub get_metrics: GetMetricsFn,
}

#[cfg(windows)]
unsafe fn load_library(path: &std::path::Path) -> Result<libloading::Library, libloading::Error> {
    use libloading::os::windows::{
        Library, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
    };

    let flags = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32;
    let library = unsafe { Library::load_with_flags(path, flags)? };
    Ok(library.into())
}

#[cfg(not(windows))]
unsafe fn load_library(path: &std::path::Path) -> Result<libloading::Library, libloading::Error> {
    unsafe { libloading::Library::new(path) }
}

impl Api {
    /// # Safety
    ///
    /// The library at `path` must implement the declared LLW ABI for all loaded symbols. Loading
    /// may run library initializers, and dropping this `Api` may run library finalizers; both must
    /// be safe to execute in the current process.
    ///
    /// Function pointers exposed by this type are copyable, so `Api` cannot enforce their
    /// lifetimes. Before dropping the owning `Api`, callers must destroy every runtime created by
    /// it and stop using copied function pointers, runtime-owned static strings, runtime handles,
    /// and callback or user-data relationships associated with the library.
    pub unsafe fn load(path: &std::path::Path) -> Result<Self, libloading::Error> {
        let library = unsafe { load_library(path)? };
        let get_abi_info = unsafe { *library.get::<GetAbiInfoFn>(b"llw_get_abi_info\0")? };
        let runtime_version =
            unsafe { *library.get::<RuntimeVersionFn>(b"llw_runtime_version\0")? };
        let llama_commit = unsafe { *library.get::<LlamaCommitFn>(b"llw_llama_cpp_commit\0")? };
        let runtime_create = unsafe { *library.get::<RuntimeCreateFn>(b"llw_runtime_create\0")? };
        let runtime_destroy =
            unsafe { *library.get::<RuntimeDestroyFn>(b"llw_runtime_destroy\0")? };
        let runtime_get_capabilities =
            unsafe { *library.get::<RuntimeGetCapabilitiesFn>(b"llw_runtime_get_capabilities\0")? };
        let runtime_list_devices =
            unsafe { *library.get::<RuntimeListDevicesFn>(b"llw_runtime_list_devices\0")? };
        let runtime_get_option_schema = unsafe {
            *library.get::<RuntimeGetOptionSchemaFn>(b"llw_runtime_get_option_schema\0")?
        };
        let model_load = unsafe { *library.get::<ModelLoadFn>(b"llw_model_load\0")? };
        let model_unload = unsafe { *library.get::<ModelUnloadFn>(b"llw_model_unload\0")? };
        let request_submit = unsafe { *library.get::<RequestSubmitFn>(b"llw_request_submit\0")? };
        let request_cancel = unsafe { *library.get::<RequestCancelFn>(b"llw_request_cancel\0")? };
        let get_scheduler_snapshot =
            unsafe { *library.get::<GetSchedulerSnapshotFn>(b"llw_get_scheduler_snapshot\0")? };
        let get_metrics = unsafe { *library.get::<GetMetricsFn>(b"llw_get_metrics\0")? };
        Ok(Self {
            _library: library,
            get_abi_info,
            runtime_version,
            llama_commit,
            runtime_create,
            runtime_destroy,
            runtime_get_capabilities,
            runtime_list_devices,
            runtime_get_option_schema,
            model_load,
            model_unload,
            request_submit,
            request_cancel,
            get_scheduler_snapshot,
            get_metrics,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loader_names_each_required_export_once() {
        let source = include_str!("lib.rs");
        let loader = source.split("#[cfg(test)]").next().expect("loader source");
        for symbol in [
            "llw_get_abi_info\\0",
            "llw_runtime_version\\0",
            "llw_llama_cpp_commit\\0",
            "llw_runtime_create\\0",
            "llw_runtime_destroy\\0",
            "llw_runtime_get_capabilities\\0",
            "llw_runtime_list_devices\\0",
            "llw_runtime_get_option_schema\\0",
            "llw_model_load\\0",
            "llw_model_unload\\0",
            "llw_request_submit\\0",
            "llw_request_cancel\\0",
            "llw_get_scheduler_snapshot\\0",
            "llw_get_metrics\\0",
        ] {
            assert_eq!(
                loader.matches(symbol).count(),
                1,
                "symbol count for {symbol}"
            );
        }
    }

    #[test]
    fn ffi_error_constants_match_c_contract() {
        assert_eq!(OK, 0);
        assert_eq!(ERR_INVALID_ARGUMENT, 1);
        assert_eq!(ERR_ABI_MISMATCH, 2);
        assert_eq!(ERR_BUFFER_TOO_SMALL, 3);
        assert_eq!(ERR_BUSY, 4);
        assert_eq!(ERR_QUEUE_FULL, 5);
        assert_eq!(ERR_NOT_FOUND, 6);
        assert_eq!(ERR_INVALID_STATE, 7);
        assert_eq!(ERR_CANCELLED, 8);
        assert_eq!(ERR_UNSUPPORTED, 9);
        assert_eq!(ERR_INTERNAL, 1000);
    }

    macro_rules! assert_layout {
        ($ty:ty, $size:expr) => {
            assert_eq!(std::mem::size_of::<$ty>(), $size);
            assert_eq!(std::mem::align_of::<$ty>(), 8);
        };
    }

    macro_rules! assert_offset {
        ($ty:ty, $field:ident, $offset:expr) => {
            assert_eq!(std::mem::offset_of!($ty, $field), $offset);
        };
    }

    #[test]
    fn ffi_layout_starts_with_struct_size() {
        assert_eq!(std::mem::offset_of!(AbiInfo, struct_size), 0);
        assert_eq!(std::mem::size_of::<Handle>(), 8);
    }

    #[test]
    fn abi_query_default_initializes_header_and_requested_version() {
        let query = AbiQuery::default();

        assert_eq!(query.struct_size, std::mem::size_of::<AbiQuery>() as u32);
        assert_eq!(query.flags, 0);
        assert_eq!(query.requested_major, ABI_MAJOR);
        assert_eq!(query.requested_minor, ABI_MINOR);
        assert_eq!(query.reserved, [0; 8]);
    }

    #[test]
    fn abi_info_default_initializes_header_and_zeroes_fields() {
        let info = AbiInfo::default();

        assert_eq!(info.struct_size, std::mem::size_of::<AbiInfo>() as u32);
        assert_eq!(info.flags, 0);
        assert_eq!(info.abi_major, 0);
        assert_eq!(info.abi_minor, 0);
        assert_eq!(info.min_supported_major, 0);
        assert_eq!(info.min_supported_minor, 0);
        assert_eq!(info.feature_flags, 0);
        assert_eq!(info.reserved, [0; 8]);
    }

    #[test]
    fn runtime_create_params_default_initializes_nested_callback_table() {
        let params = RuntimeCreateParams::default();

        assert_eq!(
            params.struct_size,
            std::mem::size_of::<RuntimeCreateParams>() as u32
        );
        assert_eq!(params.flags, 0);
        assert_eq!(
            params.callbacks.struct_size,
            std::mem::size_of::<CallbackTable>() as u32
        );
        assert_eq!(params.callbacks.flags, 0);
        assert!(params.callbacks.on_event.is_none());
        assert!(params.callbacks.user_data.is_null());
        assert_eq!(params.callbacks.reserved, [0; 8]);
        assert_eq!(params.reserved, [0; 8]);
    }

    #[test]
    fn capabilities_default_initializes_header_and_zeroes_fields() {
        let capabilities = Capabilities::default();

        assert_eq!(
            capabilities.struct_size,
            std::mem::size_of::<Capabilities>() as u32
        );
        assert_eq!(capabilities.flags, 0);
        assert_eq!(capabilities.supports_cpu, 0);
        assert_eq!(capabilities.supports_cuda, 0);
        assert_eq!(capabilities.supports_vulkan, 0);
        assert_eq!(capabilities.supports_streaming, 0);
        assert_eq!(capabilities.supports_cancellation, 0);
        assert_eq!(capabilities.max_parallel_slots, 0);
        assert_eq!(capabilities.reserved, [0; 8]);
    }

    #[test]
    fn ffi_struct_layouts_match_x64_c_contract() {
        assert_eq!(ABI_MINOR, 2);
        assert_layout!(Error, 592);
        assert_offset!(Error, struct_size, 0);
        assert_offset!(Error, code, 4);
        assert_offset!(Error, flags, 8);
        assert_offset!(Error, message, 12);
        assert_offset!(Error, reserved, 528);

        assert_layout!(AbiQuery, 80);
        assert_offset!(AbiQuery, struct_size, 0);
        assert_offset!(AbiQuery, flags, 4);
        assert_offset!(AbiQuery, requested_major, 8);
        assert_offset!(AbiQuery, requested_minor, 12);
        assert_offset!(AbiQuery, reserved, 16);

        assert_layout!(AbiInfo, 96);
        assert_offset!(AbiInfo, struct_size, 0);
        assert_offset!(AbiInfo, flags, 4);
        assert_offset!(AbiInfo, abi_major, 8);
        assert_offset!(AbiInfo, abi_minor, 12);
        assert_offset!(AbiInfo, min_supported_major, 16);
        assert_offset!(AbiInfo, min_supported_minor, 20);
        assert_offset!(AbiInfo, feature_flags, 24);
        assert_offset!(AbiInfo, reserved, 32);

        assert_layout!(Capabilities, 96);
        assert_offset!(Capabilities, struct_size, 0);
        assert_offset!(Capabilities, flags, 4);
        assert_offset!(Capabilities, supports_cpu, 8);
        assert_offset!(Capabilities, supports_cuda, 12);
        assert_offset!(Capabilities, supports_vulkan, 16);
        assert_offset!(Capabilities, supports_streaming, 20);
        assert_offset!(Capabilities, supports_cancellation, 24);
        assert_offset!(Capabilities, max_parallel_slots, 28);
        assert_offset!(Capabilities, reserved, 32);

        assert_layout!(DeviceInfo, 336);
        assert_offset!(DeviceInfo, struct_size, 0);
        assert_offset!(DeviceInfo, flags, 4);
        assert_offset!(DeviceInfo, backend, 8);
        assert_offset!(DeviceInfo, device_index, 12);
        assert_offset!(DeviceInfo, id, 16);
        assert_offset!(DeviceInfo, name, 80);
        assert_offset!(DeviceInfo, vendor, 208);
        assert_offset!(DeviceInfo, reserved, 272);

        assert_layout!(DeviceList, 104);
        assert_offset!(DeviceList, struct_size, 0);
        assert_offset!(DeviceList, flags, 4);
        assert_offset!(DeviceList, capacity, 8);
        assert_offset!(DeviceList, count, 12);
        assert_offset!(DeviceList, element_size, 16);
        assert_offset!(DeviceList, reserved0, 20);
        assert_offset!(DeviceList, devices, 24);
        assert_offset!(DeviceList, required_count, 32);
        assert_offset!(DeviceList, reserved, 40);

        assert_layout!(Event, 136);
        assert_offset!(Event, struct_size, 0);
        assert_offset!(Event, flags, 4);
        assert_offset!(Event, event_type, 8);
        assert_offset!(Event, error_code, 12);
        assert_offset!(Event, model_handle, 16);
        assert_offset!(Event, request_handle, 24);
        assert_offset!(Event, slot_id, 32);
        assert_offset!(Event, reserved0, 36);
        assert_offset!(Event, sequence_number, 40);
        assert_offset!(Event, data, 48);
        assert_offset!(Event, data_len, 56);
        assert_offset!(Event, request_user_data, 64);
        assert_offset!(Event, reserved, 72);

        assert_layout!(CallbackTable, 88);
        assert_offset!(CallbackTable, struct_size, 0);
        assert_offset!(CallbackTable, flags, 4);
        assert_offset!(CallbackTable, on_event, 8);
        assert_offset!(CallbackTable, user_data, 16);
        assert_offset!(CallbackTable, reserved, 24);

        assert_layout!(Bytes, 88);
        assert_layout!(ChatMessage, 248);
        assert_layout!(Buffer, 96);
        assert_layout!(SchedulerConfig, 88);
        assert_layout!(ModelLoadParams, 168);
        assert_layout!(RequestParams, 192);
        assert_layout!(SchedulerSnapshot, 104);
        assert_layout!(Metrics, 128);
        assert_offset!(RuntimeCreateParams, scheduler, 160);
        assert_offset!(RuntimeCreateParams, reserved_v1, 248);
        assert_eq!(std::mem::size_of::<RuntimeCreateParams>(), 312);
        assert_offset!(Bytes, data, 8);
        assert_offset!(Bytes, len, 16);
        assert_offset!(Bytes, reserved, 24);
        assert_offset!(Buffer, data, 8);
        assert_offset!(Buffer, capacity, 16);
        assert_offset!(Buffer, len, 24);
        assert_offset!(SchedulerConfig, slot_count, 8);
        assert_offset!(SchedulerConfig, request_queue_capacity, 12);
        assert_offset!(SchedulerConfig, event_queue_capacity, 16);
        assert_offset!(SchedulerConfig, reserved, 24);
        assert_offset!(ModelLoadParams, path_utf8, 8);
        assert_offset!(ModelLoadParams, path_len, 16);
        assert_offset!(ModelLoadParams, backend, 24);
        assert_offset!(ModelLoadParams, context_tokens_per_slot, 32);
        assert_offset!(ModelLoadParams, n_gpu_layers, 52);
        assert_offset!(ModelLoadParams, reserved, 72);
        assert_offset!(RequestParams, model_handle, 8);
        assert_offset!(RequestParams, prompt, 16);
        assert_offset!(RequestParams, max_new_tokens, 32);
        assert_offset!(RequestParams, temperature, 40);
        assert_offset!(RequestParams, stop_sequences, 80);
        assert_offset!(RequestParams, request_user_data, 88);
        assert_offset!(RequestParams, chat_messages, 96);
        assert_offset!(RequestParams, chat_message_count, 104);
        assert_offset!(RequestParams, reserved, 112);
        assert_offset!(SchedulerSnapshot, accepted_requests, 24);
        assert_offset!(SchedulerSnapshot, reserved, 40);
        assert_offset!(Metrics, prompt_tokens, 8);
        assert_offset!(Metrics, decode_ns, 56);
        assert_offset!(Metrics, reserved, 64);

        assert_layout!(RuntimeCreateParams, 312);
        assert_offset!(RuntimeCreateParams, struct_size, 0);
        assert_offset!(RuntimeCreateParams, flags, 4);
        assert_offset!(RuntimeCreateParams, callbacks, 8);
        assert_offset!(RuntimeCreateParams, reserved, 96);
    }

    #[test]
    fn ffi_function_pointers_use_x64_pointer_representation() {
        assert_eq!(std::mem::size_of::<EventCallback>(), 8);
        assert_eq!(std::mem::size_of::<Option<EventCallback>>(), 8);
        assert_eq!(std::mem::size_of::<GetAbiInfoFn>(), 8);
        assert_eq!(std::mem::size_of::<RuntimeVersionFn>(), 8);
        assert_eq!(std::mem::size_of::<LlamaCommitFn>(), 8);
        assert_eq!(std::mem::size_of::<RuntimeCreateFn>(), 8);
        assert_eq!(std::mem::size_of::<RuntimeDestroyFn>(), 8);
        assert_eq!(std::mem::size_of::<RuntimeGetCapabilitiesFn>(), 8);
        assert_eq!(std::mem::size_of::<RuntimeListDevicesFn>(), 8);
        assert_eq!(std::mem::size_of::<*mut Runtime>(), 8);
    }
}
