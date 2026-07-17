#![deny(unsafe_op_in_unsafe_fn)]

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
#[derive(Default)]
pub struct AbiQuery {
    pub struct_size: u32,
    pub flags: u32,
    pub requested_major: u32,
    pub requested_minor: u32,
    pub reserved: [u64; 8],
}

#[repr(C)]
#[derive(Default)]
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
#[derive(Default)]
pub struct RuntimeCreateParams {
    pub struct_size: u32,
    pub flags: u32,
    pub callbacks: CallbackTable,
    pub reserved: [u64; 8],
}

#[repr(C)]
#[derive(Default)]
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

impl Api {
    /// # Safety
    ///
    /// The library at `path` must implement the declared LLW ABI for all loaded symbols.
    pub unsafe fn load(path: &std::path::Path) -> Result<Self, libloading::Error> {
        let library = unsafe { libloading::Library::new(path)? };
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

    #[test]
    fn ffi_layout_starts_with_struct_size() {
        assert_eq!(std::mem::offset_of!(AbiInfo, struct_size), 0);
        assert_eq!(std::mem::size_of::<Handle>(), 8);
    }
}
