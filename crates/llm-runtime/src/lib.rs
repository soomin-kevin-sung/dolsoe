use std::ffi::CStr;
use std::path::Path;
use std::ptr;

use llm_runtime_sys as sys;
use thiserror::Error;

const MAX_DEVICE_ATTEMPTS: usize = 4;
const MAX_DEVICE_COUNT: usize = 256;

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

struct RuntimeGuard {
    runtime: *mut sys::Runtime,
    destroy: sys::RuntimeDestroyFn,
}

impl RuntimeGuard {
    fn new(runtime: *mut sys::Runtime, destroy: sys::RuntimeDestroyFn) -> Self {
        Self { runtime, destroy }
    }

    fn as_ptr(&self) -> *mut sys::Runtime {
        self.runtime
    }

    fn into_raw(mut self) -> *mut sys::Runtime {
        let runtime = self.runtime;
        self.runtime = ptr::null_mut();
        runtime
    }
}

impl Drop for RuntimeGuard {
    fn drop(&mut self) {
        if !self.runtime.is_null() {
            unsafe { (self.destroy)(self.runtime) };
        }
    }
}

impl Drop for RuntimeLibrary {
    fn drop(&mut self) {
        if !self.runtime.is_null() {
            unsafe { (self.api.runtime_destroy)(self.runtime) };
        }
    }
}

impl RuntimeLibrary {
    /// Loads and probes a trusted runtime library.
    ///
    /// # Safety
    ///
    /// `path` must identify a trusted DLL that conforms to the declared LLW ABI. Its initializers
    /// and finalizers must be safe to execute in this process, every loaded symbol must use the
    /// declared signature and calling convention, and exported functions must not unwind across
    /// the FFI boundary. Version functions must return valid static NUL-terminated UTF-8 strings
    /// that remain readable while the library is loaded. Runtime functions must return valid
    /// handles and must honor all output buffer, capacity, count, and lifetime contracts. Any
    /// non-null handle returned from `runtime_create`, including on failure, must be valid to pass
    /// exactly once to `runtime_destroy`.
    pub unsafe fn load(path: &Path) -> Result<Self, Error> {
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
        let mut raw_error = sys::Error::default();
        let code = unsafe { (api.runtime_create)(&create, &mut runtime, &mut raw_error) };
        let runtime = finish_runtime_create(code, runtime, &raw_error, |runtime| unsafe {
            (api.runtime_destroy)(runtime)
        })?;
        let runtime = RuntimeGuard::new(runtime, api.runtime_destroy);

        let mut capabilities = sys::Capabilities {
            struct_size: std::mem::size_of::<sys::Capabilities>() as u32,
            ..Default::default()
        };
        let mut raw_error = sys::Error::default();
        let code = unsafe {
            (api.runtime_get_capabilities)(runtime.as_ptr(), &mut capabilities, &mut raw_error)
        };
        check_result(code, &raw_error)?;

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
        Ok(Self {
            api,
            runtime: runtime.into_raw(),
            info,
        })
    }

    pub fn info(&self) -> &RuntimeInfo {
        &self.info
    }

    pub fn devices(&self, backend: Backend) -> Result<Vec<DeviceInfo>, Error> {
        enumerate_devices(|list, raw_error| unsafe {
            (self.api.runtime_list_devices)(self.runtime, backend.raw(), list, raw_error)
        })?
        .into_iter()
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

fn finish_runtime_create<F>(
    code: i32,
    runtime: *mut sys::Runtime,
    error: &sys::Error,
    mut destroy: F,
) -> Result<*mut sys::Runtime, Error>
where
    F: FnMut(*mut sys::Runtime),
{
    if code != sys::OK {
        // A conforming runtime returns null on failure. The unsafe load contract additionally
        // requires any non-null failure output to be a valid owned handle so it can be reclaimed.
        if !runtime.is_null() {
            destroy(runtime);
        }
        return Err(runtime_error(code, error));
    }
    if runtime.is_null() {
        return Err(Error::Runtime {
            code: -1,
            message: "runtime returned a null handle".into(),
        });
    }
    Ok(runtime)
}

fn enumerate_devices<F>(mut call: F) -> Result<Vec<sys::DeviceInfo>, Error>
where
    F: FnMut(&mut sys::DeviceList, &mut sys::Error) -> i32,
{
    let mut storage = Vec::new();

    for _ in 0..MAX_DEVICE_ATTEMPTS {
        storage.fill(sys::DeviceInfo::default());
        let mut list = sys::DeviceList {
            capacity: storage.len() as u32,
            devices: if storage.is_empty() {
                ptr::null_mut()
            } else {
                storage.as_mut_ptr()
            },
            ..Default::default()
        };
        let mut raw_error = sys::Error::default();
        let code = call(&mut list, &mut raw_error);
        let required_len = validate_device_response(&list, storage.len())?;

        match code {
            sys::OK if required_len <= storage.len() => {
                storage.truncate(list.count as usize);
                return Ok(storage);
            }
            sys::OK | sys::ERR_BUFFER_TOO_SMALL => {
                grow_device_storage(&mut storage, required_len)?;
            }
            _ => return Err(runtime_error(code, &raw_error)),
        }
    }

    Err(Error::Runtime {
        code: -1,
        message: format!(
            "device enumeration did not stabilize after {MAX_DEVICE_ATTEMPTS} attempts"
        ),
    })
}

fn validate_device_response(list: &sys::DeviceList, storage_len: usize) -> Result<usize, Error> {
    let required_len =
        usize::try_from(list.required_count).map_err(|_| malformed_device_count())?;
    let capacity = list.capacity as usize;
    let count = list.count as usize;
    if required_len > MAX_DEVICE_COUNT
        || capacity > storage_len
        || count > capacity
        || count > storage_len
        || count > required_len
    {
        return Err(malformed_device_count());
    }
    Ok(required_len)
}

fn grow_device_storage(
    storage: &mut Vec<sys::DeviceInfo>,
    required_len: usize,
) -> Result<(), Error> {
    if required_len <= storage.len() {
        return Ok(());
    }
    storage
        .try_reserve_exact(required_len - storage.len())
        .map_err(|error| Error::Runtime {
            code: -1,
            message: format!("failed to allocate device list: {error}"),
        })?;
    storage.resize_with(required_len, sys::DeviceInfo::default);
    Ok(())
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
    use std::ptr::NonNull;

    use llm_runtime_sys as sys;

    use super::{
        enumerate_devices, finish_runtime_create, read_fixed_string, read_static_string, Error,
        MAX_DEVICE_ATTEMPTS, MAX_DEVICE_COUNT,
    };

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
    fn retries_device_enumeration_when_required_count_grows() {
        let mut calls = 0;

        let devices = enumerate_devices(|list, _| {
            calls += 1;
            list.count = 0;
            match calls {
                1 => {
                    assert_eq!(list.capacity, 0);
                    list.required_count = 1;
                    sys::ERR_BUFFER_TOO_SMALL
                }
                2 => {
                    assert_eq!(list.capacity, 1);
                    list.required_count = 2;
                    sys::ERR_BUFFER_TOO_SMALL
                }
                3 => {
                    assert_eq!(list.capacity, 2);
                    list.required_count = 2;
                    list.count = 2;
                    sys::OK
                }
                _ => unreachable!(),
            }
        })
        .unwrap();

        assert_eq!(calls, 3);
        assert_eq!(devices.len(), 2);
    }

    #[test]
    fn retries_when_success_reports_required_count_larger_than_capacity() {
        let mut calls = 0;

        let devices = enumerate_devices(|list, _| {
            calls += 1;
            list.count = 0;
            match calls {
                1 => {
                    list.required_count = 1;
                    sys::ERR_BUFFER_TOO_SMALL
                }
                2 => {
                    assert_eq!(list.capacity, 1);
                    list.required_count = 2;
                    list.count = 1;
                    sys::OK
                }
                3 => {
                    assert_eq!(list.capacity, 2);
                    list.required_count = 2;
                    list.count = 2;
                    sys::OK
                }
                _ => unreachable!(),
            }
        })
        .unwrap();

        assert_eq!(calls, 3);
        assert_eq!(devices.len(), 2);
    }

    #[test]
    fn rejects_device_enumeration_after_retry_exhaustion() {
        let mut calls = 0;

        let error = enumerate_devices(|list, _| {
            calls += 1;
            list.count = 0;
            list.required_count = 1;
            sys::ERR_BUFFER_TOO_SMALL
        })
        .err()
        .expect("retry exhaustion must fail");

        assert_eq!(calls, MAX_DEVICE_ATTEMPTS);
        assert!(matches!(error, Error::Runtime { code: -1, .. }));
    }

    #[test]
    fn rejects_required_device_count_above_limit() {
        let error = enumerate_devices(|list, _| {
            list.count = 0;
            list.required_count = (MAX_DEVICE_COUNT + 1) as u64;
            sys::ERR_BUFFER_TOO_SMALL
        })
        .err()
        .expect("absurd required count must fail");

        assert!(matches!(error, Error::Runtime { code: -1, .. }));
    }

    #[test]
    fn rejects_returned_device_count_larger_than_capacity() {
        let mut calls = 0;

        let error = enumerate_devices(|list, _| {
            calls += 1;
            list.required_count = 1;
            if calls == 1 {
                list.count = 0;
                sys::ERR_BUFFER_TOO_SMALL
            } else {
                assert_eq!(list.capacity, 1);
                list.count = 2;
                sys::OK
            }
        })
        .err()
        .expect("count above capacity must fail");

        assert!(matches!(error, Error::Runtime { code: -1, .. }));
    }

    #[test]
    fn resets_runtime_error_before_each_enumeration_call() {
        let mut calls = 0;

        enumerate_devices(|list, error| {
            calls += 1;
            assert_eq!(error.code, 0);
            assert_eq!(error.message[0], 0);
            list.count = 0;
            list.required_count = 1;
            if calls == 1 {
                error.code = 99;
                error.message[0] = b'x' as _;
                sys::ERR_BUFFER_TOO_SMALL
            } else {
                sys::OK
            }
        })
        .unwrap();

        assert_eq!(calls, 2);
    }

    #[test]
    fn destroys_non_null_runtime_once_when_create_fails() {
        let runtime = NonNull::<sys::Runtime>::dangling().as_ptr();
        let mut destroyed = Vec::new();

        let result = finish_runtime_create(7, runtime, &sys::Error::default(), |handle| {
            destroyed.push(handle)
        });

        assert!(result.is_err());
        assert_eq!(destroyed, vec![runtime]);
    }

    #[test]
    fn rejects_null_runtime_when_create_succeeds_without_destroying() {
        let mut destroy_count = 0;

        let result = finish_runtime_create(
            sys::OK,
            std::ptr::null_mut(),
            &sys::Error::default(),
            |_| destroy_count += 1,
        );

        assert!(result.is_err());
        assert_eq!(destroy_count, 0);
    }

    #[test]
    fn accepts_non_null_runtime_when_create_succeeds_without_destroying() {
        let runtime = NonNull::<sys::Runtime>::dangling().as_ptr();
        let mut destroy_count = 0;

        let result = finish_runtime_create(sys::OK, runtime, &sys::Error::default(), |_| {
            destroy_count += 1
        });

        assert_eq!(result.unwrap(), runtime);
        assert_eq!(destroy_count, 0);
    }
}
