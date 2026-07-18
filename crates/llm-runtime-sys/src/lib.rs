#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(target_pointer_width = "64"))]
compile_error!("llm-runtime-sys supports only 64-bit targets");

use std::ffi::{c_char, c_void};

pub const ABI_MAJOR: u32 = 1;
pub const ABI_MINOR: u32 = 0;
pub const OK: i32 = 0;
pub const ERR_BUFFER_TOO_SMALL: i32 = 3;
pub const BACKEND_AUTO: i32 = 0;
pub const BACKEND_CPU: i32 = 1;
pub const BACKEND_CUDA: i32 = 2;
pub const BACKEND_VULKAN: i32 = 3;

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
pub struct RuntimeCreateParams {
    pub struct_size: u32,
    pub flags: u32,
    pub callbacks: CallbackTable,
    pub reserved: [u64; 8],
}

impl Default for RuntimeCreateParams {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            flags: 0,
            callbacks: CallbackTable::default(),
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

pub struct Api {
    _library: libloading::Library,
    pub get_abi_info: GetAbiInfoFn,
    pub runtime_version: RuntimeVersionFn,
    pub llama_commit: LlamaCommitFn,
    pub runtime_create: RuntimeCreateFn,
    pub runtime_destroy: RuntimeDestroyFn,
    pub runtime_get_capabilities: RuntimeGetCapabilitiesFn,
    pub runtime_list_devices: RuntimeListDevicesFn,
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
        Ok(Self {
            _library: library,
            get_abi_info,
            runtime_version,
            llama_commit,
            runtime_create,
            runtime_destroy,
            runtime_get_capabilities,
            runtime_list_devices,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        assert_layout!(RuntimeCreateParams, 160);
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
