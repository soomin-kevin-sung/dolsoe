use std::ffi::CStr;
use std::path::Path;
use std::ptr;

use llm_runtime_sys as sys;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to load runtime DLL: {0}")]
    Load(#[from] libloading::Error),
    #[error("runtime ABI mismatch: expected {expected}, got {actual}")]
    AbiMismatch { expected: u32, actual: u32 },
    #[error("runtime call failed with code {code}: {message}")]
    Runtime { code: i32, message: String },
    #[error("runtime returned invalid UTF-8")]
    InvalidUtf8,
}

#[derive(Debug, Clone, Copy)]
pub enum Backend {
    Auto,
    Cpu,
    Cuda,
    Vulkan,
}

impl Backend {
    fn raw(self) -> i32 {
        match self {
            Self::Auto => sys::BACKEND_AUTO,
            Self::Cpu => sys::BACKEND_CPU,
            Self::Cuda => sys::BACKEND_CUDA,
            Self::Vulkan => sys::BACKEND_VULKAN,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub supports_cpu: bool,
    pub supports_cuda: bool,
    pub supports_vulkan: bool,
    pub supports_streaming: bool,
    pub supports_cancellation: bool,
    pub max_parallel_slots: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub backend: i32,
    pub device_index: u32,
    pub id: String,
    pub name: String,
    pub vendor: String,
}

#[derive(Debug, Clone)]
pub struct RuntimeInfo {
    pub abi_major: u32,
    pub abi_minor: u32,
    pub runtime_version: String,
    pub llama_cpp_commit: String,
    pub capabilities: Capabilities,
}

pub struct RuntimeLibrary {
    api: sys::Api,
    runtime: *mut sys::Runtime,
    info: RuntimeInfo,
}

impl Drop for RuntimeLibrary {
    fn drop(&mut self) {
        if !self.runtime.is_null() {
            unsafe { (self.api.runtime_destroy)(self.runtime) };
        }
    }
}

impl RuntimeLibrary {
    pub fn load(path: &Path) -> Result<Self, Error> {
        let api = unsafe { sys::Api::load(path)? };
        let query = sys::AbiQuery {
            struct_size: std::mem::size_of::<sys::AbiQuery>() as u32,
            flags: 0,
            requested_major: sys::ABI_MAJOR,
            requested_minor: sys::ABI_MINOR,
            reserved: [0; 8],
        };
        let mut abi = sys::AbiInfo {
            struct_size: std::mem::size_of::<sys::AbiInfo>() as u32,
            ..Default::default()
        };
        let mut raw_error = sys::Error::default();
        let code = unsafe { (api.get_abi_info)(&query, &mut abi, &mut raw_error) };
        check_result(code, &raw_error)?;
        if abi.abi_major != sys::ABI_MAJOR {
            return Err(Error::AbiMismatch {
                expected: sys::ABI_MAJOR,
                actual: abi.abi_major,
            });
        }

        let runtime_version = unsafe { read_static_string((api.runtime_version)())? };
        let llama_cpp_commit = unsafe { read_static_string((api.llama_commit)())? };

        let create = sys::RuntimeCreateParams {
            struct_size: std::mem::size_of::<sys::RuntimeCreateParams>() as u32,
            flags: 0,
            callbacks: sys::CallbackTable::default(),
            reserved: [0; 8],
        };
        let mut runtime = ptr::null_mut();
        let code = unsafe { (api.runtime_create)(&create, &mut runtime, &mut raw_error) };
        check_result(code, &raw_error)?;
        if runtime.is_null() {
            return Err(Error::Runtime {
                code: -1,
                message: "runtime returned a null handle".into(),
            });
        }

        let mut capabilities = sys::Capabilities {
            struct_size: std::mem::size_of::<sys::Capabilities>() as u32,
            ..Default::default()
        };
        let code =
            unsafe { (api.runtime_get_capabilities)(runtime, &mut capabilities, &mut raw_error) };
        if let Err(error) = check_result(code, &raw_error) {
            unsafe { (api.runtime_destroy)(runtime) };
            return Err(error);
        }

        let info = RuntimeInfo {
            abi_major: abi.abi_major,
            abi_minor: abi.abi_minor,
            runtime_version,
            llama_cpp_commit,
            capabilities: Capabilities {
                supports_cpu: capabilities.supports_cpu != 0,
                supports_cuda: capabilities.supports_cuda != 0,
                supports_vulkan: capabilities.supports_vulkan != 0,
                supports_streaming: capabilities.supports_streaming != 0,
                supports_cancellation: capabilities.supports_cancellation != 0,
                max_parallel_slots: capabilities.max_parallel_slots,
            },
        };
        Ok(Self { api, runtime, info })
    }

    pub fn info(&self) -> &RuntimeInfo {
        &self.info
    }

    pub fn devices(&self, backend: Backend) -> Result<Vec<DeviceInfo>, Error> {
        let mut raw_error = sys::Error::default();
        let mut list = sys::DeviceList::default();
        let first = unsafe {
            (self.api.runtime_list_devices)(self.runtime, backend.raw(), &mut list, &mut raw_error)
        };
        if first != sys::OK && first != sys::ERR_BUFFER_TOO_SMALL {
            return Err(runtime_error(first, &raw_error));
        }

        let required_count = list.required_count;
        let required_len = validate_device_counts(required_count, 0, 0)?;
        if required_len == 0 {
            return Ok(Vec::new());
        }

        let mut storage = Vec::new();
        storage
            .try_reserve_exact(required_len)
            .map_err(|error| Error::Runtime {
                code: -1,
                message: format!("failed to allocate device list: {error}"),
            })?;
        storage.resize_with(required_len, sys::DeviceInfo::default);
        list.capacity = u32::try_from(required_len).map_err(|_| malformed_device_count())?;
        list.devices = storage.as_mut_ptr();
        let second = unsafe {
            (self.api.runtime_list_devices)(self.runtime, backend.raw(), &mut list, &mut raw_error)
        };
        check_result(second, &raw_error)?;
        validate_device_counts(required_count, list.capacity, list.count)?;

        storage
            .into_iter()
            .take(list.count as usize)
            .map(|raw| {
                Ok(DeviceInfo {
                    backend: raw.backend,
                    device_index: raw.device_index,
                    id: read_fixed_string(&raw.id)?,
                    name: read_fixed_string(&raw.name)?,
                    vendor: read_fixed_string(&raw.vendor)?,
                })
            })
            .collect()
    }
}

fn check_result(code: i32, error: &sys::Error) -> Result<(), Error> {
    if code == sys::OK {
        Ok(())
    } else {
        Err(runtime_error(code, error))
    }
}

fn runtime_error(code: i32, error: &sys::Error) -> Error {
    Error::Runtime {
        code,
        message: read_fixed_string(&error.message)
            .unwrap_or_else(|_| "unknown runtime error".into()),
    }
}

fn malformed_device_count() -> Error {
    Error::Runtime {
        code: -1,
        message: "runtime returned invalid device counts".into(),
    }
}

fn validate_device_counts(required_count: u64, capacity: u32, count: u32) -> Result<usize, Error> {
    let required_capacity = u32::try_from(required_count).map_err(|_| malformed_device_count())?;
    let required_len = usize::try_from(required_count).map_err(|_| malformed_device_count())?;
    if capacity > required_capacity || count > capacity || count as usize > required_len {
        return Err(malformed_device_count());
    }
    Ok(required_len)
}

fn read_fixed_string(value: &[std::ffi::c_char]) -> Result<String, Error> {
    let bytes: Vec<u8> = value
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect();
    String::from_utf8(bytes).map_err(|_| Error::InvalidUtf8)
}

unsafe fn read_static_string(value: *const std::ffi::c_char) -> Result<String, Error> {
    if value.is_null() {
        return Err(Error::Runtime {
            code: -1,
            message: "runtime returned a null string".into(),
        });
    }
    unsafe { CStr::from_ptr(value) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| Error::InvalidUtf8)
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use super::{read_fixed_string, read_static_string, validate_device_counts};

    #[test]
    fn reads_fixed_string_until_first_nul() {
        let value = [b'f' as _, b'a' as _, b'k' as _, b'e' as _, 0, b'x' as _];

        assert_eq!(read_fixed_string(&value).unwrap(), "fake");
    }

    #[test]
    fn rejects_invalid_utf8_in_fixed_string() {
        let value = [-1, 0];

        assert!(read_fixed_string(&value).is_err());
    }

    #[test]
    fn reads_non_null_static_string() {
        let value = CString::new("runtime").unwrap();

        assert_eq!(
            unsafe { read_static_string(value.as_ptr()) }.unwrap(),
            "runtime"
        );
    }

    #[test]
    fn rejects_null_static_string() {
        assert!(unsafe { read_static_string(std::ptr::null()) }.is_err());
    }

    #[test]
    fn rejects_required_device_count_larger_than_abi_capacity() {
        assert!(validate_device_counts(u64::from(u32::MAX) + 1, 0, 0).is_err());
    }

    #[test]
    fn rejects_returned_device_count_larger_than_capacity() {
        assert!(validate_device_counts(1, 1, 2).is_err());
    }
}
