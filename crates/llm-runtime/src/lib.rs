use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::{c_void, CStr};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError};
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
    #[error("invalid runtime input: {0}")]
    InvalidInput(String),
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
            scheduler: sys::SchedulerConfig::default(),
            reserved_v1: [0; 8],
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
        if code != sys::OK && code != sys::ERR_BUFFER_TOO_SMALL {
            return Err(runtime_error(code, &raw_error));
        }
        let required_len = validate_device_response(&list, storage.len())?;

        if code == sys::OK && required_len <= storage.len() {
            storage.truncate(list.count as usize);
            return Ok(storage);
        }
        grow_device_storage(&mut storage, required_len)?;
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crossbeam_channel::Receiver;

    use llm_runtime_sys as sys;

    use super::{
        enumerate_devices, event_trampoline, finish_runtime_create, read_fixed_string,
        read_static_string, run_cancellation_worker, CallbackState, CancellationRegistry, Error,
        EventKind, RequestRegistry, RequestState, RuntimeEvent, MAX_DEVICE_ATTEMPTS,
        MAX_DEVICE_COUNT,
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
    fn copies_owned_request_correlation_from_native_events() {
        let mut correlation = 42_u64;
        let raw = sys::Event {
            struct_size: std::mem::size_of::<sys::Event>() as u32,
            flags: 0,
            event_type: 3,
            error_code: 0,
            model_handle: 1,
            request_handle: 2,
            slot_id: 0,
            reserved0: 0,
            sequence_number: 3,
            data: std::ptr::null(),
            data_len: 0,
            request_user_data: (&mut correlation as *mut u64).cast(),
            reserved: [0; 8],
        };

        let event = RuntimeEvent::from_raw(&raw, Vec::new()).unwrap();

        assert_eq!(event.request_user_data, 42);
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
    fn preserves_unexpected_runtime_error_when_counts_are_malformed() {
        let error = enumerate_devices(|list, error| {
            list.required_count = u64::MAX;
            list.count = u32::MAX;
            for (destination, source) in error
                .message
                .iter_mut()
                .zip(b"enumeration failed\0".iter().copied())
            {
                *destination = source as _;
            }
            77
        })
        .err()
        .expect("unexpected runtime error must fail");

        assert!(matches!(
            error,
            Error::Runtime { code: 77, message } if message == "enumeration failed"
        ));
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

    fn callback_state(
        capacity: usize,
        max_outstanding: usize,
    ) -> (CallbackState, Receiver<RuntimeEvent>, Receiver<()>) {
        let (regular_sender, regular) = crossbeam_channel::bounded(capacity);
        let invariant_violations = Arc::new(AtomicUsize::new(0));
        let (cancellations, cancel_wake_receiver) =
            CancellationRegistry::new(max_outstanding, invariant_violations.clone());
        (
            CallbackState {
                regular_sender,
                registry: Mutex::new(RequestRegistry::default()),
                cancellations,
                max_outstanding,
                invariant_violations,
                test_hook: None,
            },
            regular,
            cancel_wake_receiver,
        )
    }

    fn raw_event(event_type: i32, request: u64, sequence: u64, data: &[u8]) -> sys::Event {
        sys::Event {
            struct_size: std::mem::size_of::<sys::Event>() as u32,
            flags: if event_type == sys::EVENT_TOKEN {
                sys::EVENT_DATA_BYTES
            } else {
                sys::EVENT_DATA_JSON_UTF8
            },
            event_type,
            error_code: 0,
            model_handle: 1,
            request_handle: request,
            slot_id: 0,
            reserved0: 0,
            sequence_number: sequence,
            data: data.as_ptr(),
            data_len: data.len() as u64,
            request_user_data: std::ptr::null_mut(),
            reserved: [0; 8],
        }
    }

    fn invoke(state: &CallbackState, event: &sys::Event) {
        unsafe { event_trampoline(event, (state as *const CallbackState).cast_mut().cast()) };
    }

    #[test]
    fn callback_copies_stack_backed_payload_before_return() {
        let (state, events, _cancellations) = callback_state(4, 2);
        let mut payload = [0xf0, 0x9f, 0x92, 0xa1];
        let event = raw_event(sys::EVENT_TOKEN, 2, 2, &payload);
        invoke(&state, &event);
        payload.fill(0);
        assert_eq!(
            events.recv_timeout(Duration::from_secs(1)).unwrap().payload,
            vec![0xf0, 0x9f, 0x92, 0xa1]
        );
    }

    #[test]
    fn callback_contains_panics_from_test_consumer() {
        let (mut state, _events, _cancellations) = callback_state(1, 1);
        state.test_hook = Some(Arc::new(|_| panic!("test panic")));
        let event = raw_event(sys::EVENT_DONE, 2, 3, &[]);
        let escaped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            event_trampoline(&event, (&state as *const CallbackState).cast_mut().cast())
        }));
        assert!(escaped.is_ok());
    }

    #[test]
    fn terminal_before_registration_is_atomic_and_removed() {
        let (state, _events, _cancellations) = callback_state(2, 2);
        invoke(&state, &raw_event(sys::EVENT_DONE, 7, 2, &[]));
        let request_state = Arc::new(RequestState::default());
        let (terminal_sender, terminal) = crossbeam_channel::bounded(1);
        state.register(7, terminal_sender, request_state.clone());
        assert!(request_state.native_done.load(Ordering::Acquire));
        assert_eq!(
            terminal
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .request_handle,
            7
        );
        let registry = state.registry.lock().unwrap();
        assert!(registry.entries.is_empty());
    }

    #[test]
    fn overflow_worker_cancels_and_native_terminal_cleans_without_duplicate() {
        let (state, events, cancel_wake_receiver) = callback_state(1, 2);
        assert_eq!(state.regular_sender.capacity(), Some(1));
        assert_eq!(cancel_wake_receiver.capacity(), Some(1));
        let request_state = Arc::new(RequestState::default());
        let (terminal_sender, terminal) = crossbeam_channel::bounded(1);
        state.register(9, terminal_sender, request_state.clone());
        let (cancelled_sender, cancelled) = crossbeam_channel::bounded(1);
        let cancellations = state.cancellations.clone();
        let worker = std::thread::spawn(move || {
            run_cancellation_worker(cancellations, cancel_wake_receiver, move |handle| {
                let _ = cancelled_sender.try_send(handle);
            })
        });
        invoke(&state, &raw_event(sys::EVENT_QUEUED, 9, 1, &[]));
        invoke(&state, &raw_event(sys::EVENT_TOKEN, 9, 2, b"a"));
        invoke(&state, &raw_event(sys::EVENT_TOKEN, 9, 3, b"b"));
        assert!(request_state.delivery_failed.load(Ordering::Acquire));
        assert!(!request_state.native_done.load(Ordering::Acquire));
        assert!(request_state
            .native_cancel_requested
            .load(Ordering::Acquire));
        let first = events.recv_timeout(Duration::from_secs(1)).unwrap();
        let overflow = terminal.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(first.kind, EventKind::Queued);
        assert_eq!(overflow.kind, EventKind::Error);
        assert!(String::from_utf8_lossy(&overflow.payload).contains("rustEventOverflow"));
        assert_eq!(cancelled.recv_timeout(Duration::from_secs(1)).unwrap(), 9);
        invoke(&state, &raw_event(sys::EVENT_DONE, 9, 4, &[]));
        assert!(request_state.native_done.load(Ordering::Acquire));
        assert!(terminal.recv_timeout(Duration::from_millis(20)).is_err());
        let registry = state.registry.lock().unwrap();
        assert!(registry.entries.is_empty());
        drop(registry);
        state.cancellations.close();
        worker.join().unwrap();
        assert_eq!(state.invariant_violations.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn abandoned_terminal_receiver_never_blocks_or_retains_registry() {
        let (state, _events, _cancellations) = callback_state(1, 1);
        let request_state = Arc::new(RequestState::default());
        let (terminal_sender, terminal) = crossbeam_channel::bounded(1);
        state.register(1, terminal_sender, request_state);
        drop(terminal);
        invoke(&state, &raw_event(sys::EVENT_DONE, 1, 2, &[]));
        assert!(state.registry.lock().unwrap().entries.is_empty());
    }

    #[test]
    fn abandoned_regular_receiver_reports_overflow_and_queues_cancel() {
        let (state, events, cancel_wake_receiver) = callback_state(1, 1);
        drop(events);
        let request_state = Arc::new(RequestState::default());
        let (terminal_sender, terminal) = crossbeam_channel::bounded(1);
        state.register(4, terminal_sender, request_state.clone());
        invoke(&state, &raw_event(sys::EVENT_TOKEN, 4, 2, b"lost"));
        let overflow = terminal.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(overflow.kind, EventKind::Error);
        assert!(request_state.delivery_failed.load(Ordering::Acquire));
        assert!(!request_state.native_done.load(Ordering::Acquire));
        assert_eq!(state.cancellations.pending_len(), 1);
        assert_eq!(
            cancel_wake_receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            ()
        );
        invoke(&state, &raw_event(sys::EVENT_CANCELLED, 4, 3, &[]));
        assert!(request_state.native_done.load(Ordering::Acquire));
        assert_eq!(state.cancellations.pending_len(), 0);
        assert!(state.registry.lock().unwrap().entries.is_empty());
    }

    #[test]
    fn saturated_wake_channel_drains_every_deduplicated_cancellation() {
        let (state, events, cancel_wake_receiver) = callback_state(1, 2);
        let (terminal_sender_one, terminal_one) = crossbeam_channel::bounded(1);
        let (terminal_sender_two, terminal_two) = crossbeam_channel::bounded(1);
        state.register(1, terminal_sender_one, Arc::new(RequestState::default()));
        state.register(2, terminal_sender_two, Arc::new(RequestState::default()));
        invoke(&state, &raw_event(sys::EVENT_QUEUED, 1, 1, &[]));
        invoke(&state, &raw_event(sys::EVENT_TOKEN, 1, 2, b"a"));
        invoke(&state, &raw_event(sys::EVENT_TOKEN, 2, 1, b"b"));
        assert_eq!(state.cancellations.pending_len(), 2);
        assert_eq!(cancel_wake_receiver.len(), 1);

        let cancellations = state.cancellations.clone();
        let (cancelled_sender, cancelled) = crossbeam_channel::bounded(2);
        let worker = std::thread::spawn(move || {
            run_cancellation_worker(cancellations, cancel_wake_receiver, move |handle| {
                cancelled_sender.send(handle).unwrap();
            })
        });
        assert_eq!(cancelled.recv_timeout(Duration::from_secs(1)).unwrap(), 1);
        assert_eq!(cancelled.recv_timeout(Duration::from_secs(1)).unwrap(), 2);
        assert_eq!(state.cancellations.pending_len(), 0);
        assert_eq!(
            terminal_one
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .kind,
            EventKind::Error
        );
        assert_eq!(
            terminal_two
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .kind,
            EventKind::Error
        );
        invoke(&state, &raw_event(sys::EVENT_CANCELLED, 1, 3, &[]));
        invoke(&state, &raw_event(sys::EVENT_CANCELLED, 2, 2, &[]));
        assert!(state.registry.lock().unwrap().entries.is_empty());
        state.cancellations.close();
        worker.join().unwrap();
        assert_eq!(state.invariant_violations.load(Ordering::Relaxed), 0);
        drop(events);
    }

    #[test]
    fn sequential_overflow_terminals_remove_stale_pending_cancellations() {
        let (state, events, cancel_wake_receiver) = callback_state(1, 1);
        drop(events);
        for handle in 1..=100 {
            let request_state = Arc::new(RequestState::default());
            let (terminal_sender, terminal) = crossbeam_channel::bounded(1);
            state.register(handle, terminal_sender, request_state.clone());
            invoke(&state, &raw_event(sys::EVENT_TOKEN, handle, 1, b"lost"));
            assert_eq!(state.cancellations.pending_len(), 1);
            assert_eq!(
                terminal.recv_timeout(Duration::from_secs(1)).unwrap().kind,
                EventKind::Error
            );
            invoke(&state, &raw_event(sys::EVENT_CANCELLED, handle, 2, &[]));
            assert!(request_state.native_done.load(Ordering::Acquire));
            assert!(terminal.try_recv().is_err());
            assert_eq!(state.cancellations.pending_len(), 0);
            assert!(state.registry.lock().unwrap().entries.is_empty());
        }
        assert_eq!(cancel_wake_receiver.len(), 1);
        assert_eq!(cancel_wake_receiver.try_recv().unwrap(), ());
        assert!(cancel_wake_receiver.try_recv().is_err());
        assert_eq!(state.invariant_violations.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn explicit_cancel_claim_prevents_duplicate_worker_cancellation() {
        let (state, events, cancel_wake_receiver) = callback_state(1, 1);
        drop(events);
        let request_state = Arc::new(RequestState::default());
        let (terminal_sender, terminal) = crossbeam_channel::bounded(1);
        state.register(5, terminal_sender, request_state.clone());
        request_state
            .native_cancel_requested
            .store(true, Ordering::Release);
        invoke(&state, &raw_event(sys::EVENT_TOKEN, 5, 1, b"lost"));
        assert_eq!(
            terminal.recv_timeout(Duration::from_secs(1)).unwrap().kind,
            EventKind::Error
        );
        assert_eq!(state.cancellations.pending_len(), 0);
        assert!(cancel_wake_receiver.try_recv().is_err());
        invoke(&state, &raw_event(sys::EVENT_CANCELLED, 5, 2, &[]));
        assert!(state.registry.lock().unwrap().entries.is_empty());
    }

    #[test]
    fn sequential_terminals_exceed_max_without_shared_queue_loss() {
        let (state, _events, _cancellations) = callback_state(2, 4);
        let mut terminals = Vec::new();
        for handle in 1..=100 {
            let request_state = Arc::new(RequestState::default());
            let (terminal_sender, terminal) = crossbeam_channel::bounded(1);
            state.register(handle, terminal_sender, request_state.clone());
            invoke(&state, &raw_event(sys::EVENT_DONE, handle, 1, &[]));
            assert!(request_state.native_done.load(Ordering::Acquire));
            assert!(state.registry.lock().unwrap().entries.is_empty());
            terminals.push((handle, terminal));
        }
        for (handle, terminal) in terminals {
            assert_eq!(
                terminal
                    .recv_timeout(Duration::from_secs(1))
                    .unwrap()
                    .request_handle,
                handle
            );
        }
        assert_eq!(state.invariant_violations.load(Ordering::Relaxed), 0);
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeOptions {
    pub slot_count: u32,
    pub request_queue_capacity: u32,
    pub event_queue_capacity: u32,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            slot_count: 1,
            request_queue_capacity: 16,
            event_queue_capacity: 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelOptions {
    pub backend: Backend,
    pub device_index: u32,
    pub context_tokens_per_slot: u32,
    pub logical_batch_tokens: u32,
    pub physical_batch_tokens: u32,
    pub n_threads: i32,
    pub n_threads_batch: i32,
    pub n_gpu_layers: i32,
    pub use_mmap: bool,
    pub use_mlock: bool,
    pub check_tensors: bool,
}

impl Default for ModelOptions {
    fn default() -> Self {
        Self {
            backend: Backend::Auto,
            device_index: 0,
            context_tokens_per_slot: 4096,
            logical_batch_tokens: 512,
            physical_batch_tokens: 128,
            n_threads: 8,
            n_threads_batch: 8,
            n_gpu_layers: 0,
            use_mmap: true,
            use_mlock: false,
            check_tensors: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GenerationOptions {
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
    pub stop_sequences: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl Default for GenerationOptions {
    fn default() -> Self {
        Self {
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
            stop_sequences: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    ModelProgress,
    Queued,
    Token,
    Metrics,
    Done,
    Cancelled,
    Error,
    Log,
}

#[derive(Debug, Clone)]
pub struct RuntimeEvent {
    pub kind: EventKind,
    pub data_format: u32,
    pub error_code: i32,
    pub model_handle: u64,
    pub request_handle: u64,
    pub slot_id: u32,
    pub sequence_number: u64,
    pub request_user_data: u64,
    pub payload: Vec<u8>,
}

impl RuntimeEvent {
    fn from_raw(raw: &sys::Event, payload: Vec<u8>) -> Option<Self> {
        let kind = match raw.event_type {
            1 => EventKind::ModelProgress,
            2 => EventKind::Queued,
            3 => EventKind::Token,
            4 => EventKind::Metrics,
            5 => EventKind::Done,
            6 => EventKind::Cancelled,
            7 => EventKind::Error,
            8 => EventKind::Log,
            _ => return None,
        };
        Some(Self {
            kind,
            data_format: raw.flags,
            error_code: raw.error_code,
            model_handle: raw.model_handle,
            request_handle: raw.request_handle,
            slot_id: raw.slot_id,
            sequence_number: raw.sequence_number,
            request_user_data: if raw.request_user_data.is_null() {
                0
            } else {
                // Correlated submissions own this u64 until the terminal callback returns.
                unsafe { *raw.request_user_data.cast::<u64>() }
            },
            payload,
        })
    }

    fn terminal(&self) -> bool {
        matches!(
            self.kind,
            EventKind::Done | EventKind::Cancelled | EventKind::Error
        )
    }

    fn overflow_error(dropped: &Self) -> Self {
        Self {
            kind: EventKind::Error,
            data_format: sys::EVENT_DATA_JSON_UTF8,
            error_code: sys::ERR_INTERNAL,
            model_handle: dropped.model_handle,
            request_handle: dropped.request_handle,
            slot_id: dropped.slot_id,
            sequence_number: dropped.sequence_number,
            request_user_data: dropped.request_user_data,
            payload: br#"{"state":"error","reason":"rustEventOverflow"}"#.to_vec(),
        }
    }

    fn invariant_error(handle: u64) -> Self {
        Self {
            kind: EventKind::Error,
            data_format: sys::EVENT_DATA_JSON_UTF8,
            error_code: sys::ERR_INTERNAL,
            model_handle: 0,
            request_handle: handle,
            slot_id: u32::MAX,
            sequence_number: 0,
            request_user_data: 0,
            payload: br#"{"state":"error","reason":"requestRegistryInvariant"}"#.to_vec(),
        }
    }
}

#[derive(Default)]
struct RequestState {
    native_done: AtomicBool,
    delivery_failed: AtomicBool,
    native_cancel_requested: AtomicBool,
    request_correlation: Option<Box<u64>>,
}

enum TerminalRoute {
    Unregistered,
    Sender(Sender<RuntimeEvent>),
    Early(RuntimeEvent),
    Delivered,
}

struct RegistryEntry {
    route: TerminalRoute,
    state: Option<Arc<RequestState>>,
    native_done: bool,
    delivery_failed: bool,
    cancel_requested: bool,
}

impl Default for RegistryEntry {
    fn default() -> Self {
        Self {
            route: TerminalRoute::Unregistered,
            state: None,
            native_done: false,
            delivery_failed: false,
            cancel_requested: false,
        }
    }
}

#[derive(Default)]
struct RequestRegistry {
    entries: HashMap<u64, RegistryEntry>,
}

struct CancellationState {
    order: VecDeque<u64>,
    members: HashSet<u64>,
}

struct CancellationRegistry {
    state: Mutex<CancellationState>,
    wake_sender: Mutex<Option<Sender<()>>>,
    capacity: usize,
    invariant_violations: Arc<AtomicUsize>,
}

impl CancellationRegistry {
    fn new(capacity: usize, invariant_violations: Arc<AtomicUsize>) -> (Arc<Self>, Receiver<()>) {
        let (wake_sender, wake_receiver) = crossbeam_channel::bounded(1);
        let state = CancellationState {
            order: VecDeque::with_capacity(capacity),
            members: HashSet::with_capacity(capacity),
        };
        (
            Arc::new(Self {
                state: Mutex::new(state),
                wake_sender: Mutex::new(Some(wake_sender)),
                capacity,
                invariant_violations,
            }),
            wake_receiver,
        )
    }

    fn request(&self, handle: u64, request_state: Option<&RequestState>) {
        {
            let mut pending = self.state.lock().expect("cancellation registry poisoned");
            if pending.members.contains(&handle) {
                return;
            }
            if request_state
                .is_some_and(|state| state.native_cancel_requested.swap(true, Ordering::AcqRel))
            {
                return;
            }
            pending.members.insert(handle);
            pending.order.push_back(handle);
            debug_assert!(pending.members.len() <= self.capacity);
        }
        let sender = self
            .wake_sender
            .lock()
            .expect("cancel wake sender poisoned")
            .clone();
        match sender.map(|sender| sender.try_send(())) {
            Some(Ok(())) | Some(Err(TrySendError::Full(()))) => {}
            Some(Err(TrySendError::Disconnected(()))) | None => {
                self.invariant_violations.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn remove(&self, handle: u64) {
        let mut pending = self.state.lock().expect("cancellation registry poisoned");
        if pending.members.remove(&handle) {
            pending.order.retain(|queued| *queued != handle);
        }
    }

    fn pop(&self) -> Option<u64> {
        let mut pending = self.state.lock().expect("cancellation registry poisoned");
        while let Some(handle) = pending.order.pop_front() {
            if pending.members.remove(&handle) {
                return Some(handle);
            }
        }
        None
    }

    #[cfg(test)]
    fn pending_len(&self) -> usize {
        self.state
            .lock()
            .expect("cancellation registry poisoned")
            .members
            .len()
    }

    fn close(&self) {
        let sender = self
            .wake_sender
            .lock()
            .expect("cancel wake sender poisoned")
            .take();
        if let Some(sender) = sender {
            let _ = sender.try_send(());
        }
    }
}

#[allow(clippy::type_complexity)]
struct CallbackState {
    regular_sender: Sender<RuntimeEvent>,
    registry: Mutex<RequestRegistry>,
    cancellations: Arc<CancellationRegistry>,
    max_outstanding: usize,
    invariant_violations: Arc<AtomicUsize>,
    test_hook: Option<Arc<dyn Fn(&RuntimeEvent) + Send + Sync>>,
}

impl CallbackState {
    fn send_or_store(&self, route: &mut TerminalRoute, event: RuntimeEvent) -> bool {
        match std::mem::replace(route, TerminalRoute::Delivered) {
            TerminalRoute::Sender(sender) => {
                if matches!(sender.try_send(event), Err(TrySendError::Full(_))) {
                    self.invariant_violations.fetch_add(1, Ordering::Relaxed);
                }
                true
            }
            TerminalRoute::Unregistered => {
                *route = TerminalRoute::Early(event);
                false
            }
            TerminalRoute::Early(existing) => {
                *route = TerminalRoute::Early(existing);
                false
            }
            TerminalRoute::Delivered => true,
        }
    }

    fn register(&self, handle: u64, sender: Sender<RuntimeEvent>, state: Arc<RequestState>) {
        let mut registry = self.registry.lock().expect("request registry poisoned");
        if !registry.entries.contains_key(&handle) && registry.entries.len() >= self.max_outstanding
        {
            self.invariant_violations.fetch_add(1, Ordering::Relaxed);
            state.native_done.store(true, Ordering::Release);
            let _ = sender.try_send(RuntimeEvent::invariant_error(handle));
            return;
        }
        let entry = registry.entries.entry(handle).or_default();
        entry.state = Some(state.clone());
        state
            .native_done
            .store(entry.native_done, Ordering::Release);
        state
            .delivery_failed
            .store(entry.delivery_failed, Ordering::Release);
        state
            .native_cancel_requested
            .store(entry.cancel_requested, Ordering::Release);
        match std::mem::replace(&mut entry.route, TerminalRoute::Sender(sender)) {
            TerminalRoute::Early(event) => {
                if let TerminalRoute::Sender(sender) =
                    std::mem::replace(&mut entry.route, TerminalRoute::Delivered)
                {
                    let _ = sender.try_send(event);
                }
            }
            TerminalRoute::Unregistered => {}
            TerminalRoute::Sender(existing) => {
                entry.route = TerminalRoute::Sender(existing);
                self.invariant_violations.fetch_add(1, Ordering::Relaxed);
            }
            TerminalRoute::Delivered => entry.route = TerminalRoute::Delivered,
        }
        if entry.native_done {
            registry.entries.remove(&handle);
        }
    }

    fn native_terminal(&self, event: RuntimeEvent) {
        let handle = event.request_handle;
        {
            let mut registry = self.registry.lock().expect("request registry poisoned");
            if !registry.entries.contains_key(&handle)
                && registry.entries.len() >= self.max_outstanding
            {
                self.invariant_violations.fetch_add(1, Ordering::Relaxed);
            } else {
                let entry = registry.entries.entry(handle).or_default();
                entry.native_done = true;
                if let Some(state) = &entry.state {
                    state.native_done.store(true, Ordering::Release);
                }
                if !entry.delivery_failed {
                    self.send_or_store(&mut entry.route, event);
                }
                if entry.state.is_some() {
                    registry.entries.remove(&handle);
                }
            }
        }
        self.cancellations.remove(handle);
    }

    fn overflow(&self, dropped: RuntimeEvent) {
        let handle = dropped.request_handle;
        {
            let mut registry = self.registry.lock().expect("request registry poisoned");
            if !registry.entries.contains_key(&handle)
                && registry.entries.len() >= self.max_outstanding
            {
                self.invariant_violations.fetch_add(1, Ordering::Relaxed);
                return;
            }
            let entry = registry.entries.entry(handle).or_default();
            if entry.delivery_failed {
                return;
            }
            entry.delivery_failed = true;
            if let Some(state) = &entry.state {
                state.delivery_failed.store(true, Ordering::Release);
            }
            let overflow = RuntimeEvent::overflow_error(&dropped);
            self.send_or_store(&mut entry.route, overflow);
            if !entry.cancel_requested {
                self.cancellations.request(handle, entry.state.as_deref());
                entry.cancel_requested = true;
            }
        }
    }

    fn deliver(&self, event: RuntimeEvent) {
        if event.terminal() && event.request_handle != 0 {
            self.native_terminal(event);
            return;
        }
        if event.request_handle != 0 {
            let registry = self.registry.lock().expect("request registry poisoned");
            if registry
                .entries
                .get(&event.request_handle)
                .is_some_and(|entry| entry.delivery_failed)
            {
                return;
            }
        }
        match self.regular_sender.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(dropped) | TrySendError::Disconnected(dropped))
                if dropped.request_handle != 0 =>
            {
                self.overflow(dropped);
            }
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.invariant_violations.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

unsafe extern "C" fn event_trampoline(event: *const sys::Event, user_data: *mut c_void) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(raw) = (unsafe { event.as_ref() }) else {
            return;
        };
        if raw.struct_size < std::mem::size_of::<sys::Event>() as u32 {
            return;
        }
        let payload = if raw.data.is_null() || raw.data_len == 0 {
            Vec::new()
        } else {
            let Ok(len) = usize::try_from(raw.data_len) else {
                return;
            };
            unsafe { std::slice::from_raw_parts(raw.data, len) }.to_vec()
        };
        let state = unsafe { &*(user_data.cast::<CallbackState>()) };
        let Some(event) = RuntimeEvent::from_raw(raw, payload) else {
            return;
        };
        if let Some(hook) = &state.test_hook {
            hook(&event);
        }
        state.deliver(event);
    }));
}

fn run_cancellation_worker<F>(
    cancellations: Arc<CancellationRegistry>,
    wake_receiver: Receiver<()>,
    mut cancel: F,
) where
    F: FnMut(u64),
{
    while wake_receiver.recv().is_ok() {
        while let Some(handle) = cancellations.pop() {
            cancel(handle);
        }
    }
}

struct RuntimeInner {
    api: sys::Api,
    runtime: *mut sys::Runtime,
    callback_state: Box<CallbackState>,
    call_lock: Mutex<()>,
    cancel_worker: Option<std::thread::JoinHandle<()>>,
}

// Arc ownership keeps the runtime alive through every Model and Request. No Send or Sync
// implementation is provided; call_lock serializes native calls on the owner thread.
impl Drop for RuntimeInner {
    fn drop(&mut self) {
        self.callback_state.cancellations.close();
        if let Some(worker) = self.cancel_worker.take() {
            let _ = worker.join();
        }
        let _guard = self.call_lock.lock().expect("runtime call lock poisoned");
        if !self.runtime.is_null() {
            unsafe { (self.api.runtime_destroy)(self.runtime) };
            self.runtime = std::ptr::null_mut();
        }
    }
}

pub struct InferenceRuntime {
    inner: Arc<RuntimeInner>,
    events: Receiver<RuntimeEvent>,
}

struct ModelState {
    runtime: Arc<RuntimeInner>,
    handle: u64,
}

impl Drop for ModelState {
    fn drop(&mut self) {
        let _guard = self
            .runtime
            .call_lock
            .lock()
            .expect("runtime call lock poisoned");
        let mut error = sys::Error::default();
        unsafe { (self.runtime.api.model_unload)(self.runtime.runtime, self.handle, &mut error) };
    }
}

pub struct Model {
    state: Arc<ModelState>,
}

pub struct RequestStream {
    model: Arc<ModelState>,
    handle: u64,
    state: Arc<RequestState>,
    terminal: Receiver<RuntimeEvent>,
}

impl RequestStream {
    fn request_native_cancel(&self) -> Result<(), Error> {
        if self.state.native_done.load(Ordering::Acquire)
            || self
                .state
                .native_cancel_requested
                .swap(true, Ordering::AcqRel)
        {
            return Ok(());
        }
        let _guard = self
            .model
            .runtime
            .call_lock
            .lock()
            .expect("runtime call lock poisoned");
        let mut error = sys::Error::default();
        check_result(
            unsafe {
                (self.model.runtime.api.request_cancel)(
                    self.model.runtime.runtime,
                    self.handle,
                    &mut error,
                )
            },
            &error,
        )
    }

    pub fn handle(&self) -> u64 {
        self.handle
    }

    pub fn terminal_receiver(&self) -> Receiver<RuntimeEvent> {
        self.terminal.clone()
    }

    pub fn recv_terminal_timeout(
        &self,
        timeout: Duration,
    ) -> Result<RuntimeEvent, RecvTimeoutError> {
        self.terminal.recv_timeout(timeout)
    }

    pub fn delivery_failed(&self) -> bool {
        self.state.delivery_failed.load(Ordering::Acquire)
    }

    pub fn cancel(&self) -> Result<(), Error> {
        self.request_native_cancel()
    }
}

impl Drop for RequestStream {
    fn drop(&mut self) {
        let _ = self.request_native_cancel();
    }
}

impl InferenceRuntime {
    /// # Safety
    /// `path` must be a trusted project-managed runtime pack DLL implementing LLW ABI 1.1.
    #[allow(clippy::arc_with_non_send_sync)]
    pub unsafe fn load(path: &Path, options: RuntimeOptions) -> Result<Self, Error> {
        if !(1..=4).contains(&options.slot_count)
            || !(1..=1024).contains(&options.request_queue_capacity)
            || !(16..=65536).contains(&options.event_queue_capacity)
        {
            return Err(Error::Runtime {
                code: sys::ERR_INVALID_ARGUMENT,
                message: "runtime queue options are outside ABI bounds".into(),
            });
        }
        let api = unsafe { sys::Api::load(path)? };
        let query = sys::AbiQuery::default();
        let mut info = sys::AbiInfo::default();
        let mut raw_error = sys::Error::default();
        check_result(
            unsafe { (api.get_abi_info)(&query, &mut info, &mut raw_error) },
            &raw_error,
        )?;
        if info.abi_major != sys::ABI_MAJOR {
            return Err(Error::AbiMismatch {
                expected: sys::ABI_MAJOR,
                actual: info.abi_major,
            });
        }
        if info.abi_minor < 1 {
            return Err(Error::Runtime {
                code: sys::ERR_UNSUPPORTED,
                message: "runtime does not support inference ABI 1.1".into(),
            });
        }
        let max_outstanding = usize::try_from(options.slot_count + options.request_queue_capacity)
            .expect("runtime bounds fit usize");
        let regular_capacity =
            usize::try_from(options.event_queue_capacity).expect("runtime bounds fit usize");
        let (regular_sender, events) = crossbeam_channel::bounded(regular_capacity);
        let invariant_violations = Arc::new(AtomicUsize::new(0));
        let (cancellations, cancel_wake_receiver) =
            CancellationRegistry::new(max_outstanding, invariant_violations.clone());
        let mut callback_state = Box::new(CallbackState {
            regular_sender,
            registry: Mutex::new(RequestRegistry::default()),
            cancellations: cancellations.clone(),
            max_outstanding,
            invariant_violations: invariant_violations.clone(),
            test_hook: None,
        });
        let callbacks = sys::CallbackTable {
            struct_size: std::mem::size_of::<sys::CallbackTable>() as u32,
            flags: 0,
            on_event: Some(event_trampoline),
            user_data: (&mut *callback_state as *mut CallbackState).cast(),
            reserved: [0; 8],
        };
        let create = sys::RuntimeCreateParams {
            struct_size: std::mem::size_of::<sys::RuntimeCreateParams>() as u32,
            flags: 0,
            callbacks,
            reserved: [0; 8],
            scheduler: sys::SchedulerConfig {
                struct_size: std::mem::size_of::<sys::SchedulerConfig>() as u32,
                flags: 0,
                slot_count: options.slot_count,
                request_queue_capacity: options.request_queue_capacity,
                event_queue_capacity: options.event_queue_capacity,
                reserved0: 0,
                reserved: [0; 8],
            },
            reserved_v1: [0; 8],
        };
        let mut runtime = std::ptr::null_mut();
        let mut raw_error = sys::Error::default();
        let code = unsafe { (api.runtime_create)(&create, &mut runtime, &mut raw_error) };
        let runtime = finish_runtime_create(code, runtime, &raw_error, |value| unsafe {
            (api.runtime_destroy)(value)
        })?;
        let runtime_address = runtime as usize;
        let cancel = api.request_cancel;
        let worker_violations = invariant_violations.clone();
        let cancel_worker = std::thread::spawn(move || {
            run_cancellation_worker(cancellations, cancel_wake_receiver, move |handle| {
                let runtime = runtime_address as *mut sys::Runtime;
                let mut error = sys::Error::default();
                let result = unsafe { cancel(runtime, handle, &mut error) };
                if result != sys::OK
                    && result != sys::ERR_NOT_FOUND
                    && result != sys::ERR_INVALID_STATE
                {
                    worker_violations.fetch_add(1, Ordering::Relaxed);
                }
            })
        });
        Ok(Self {
            inner: Arc::new(RuntimeInner {
                api,
                runtime,
                callback_state,
                call_lock: Mutex::new(()),
                cancel_worker: Some(cancel_worker),
            }),
            events,
        })
    }

    pub fn events(&self) -> Receiver<RuntimeEvent> {
        self.events.clone()
    }

    #[allow(clippy::arc_with_non_send_sync)]
    pub fn load_model(&self, path: &Path, options: ModelOptions) -> Result<Model, Error> {
        let canonical = path.canonicalize().map_err(|error| Error::Runtime {
            code: -1,
            message: format!("failed to canonicalize model path: {error}"),
        })?;
        let utf8 = canonical.to_str().ok_or_else(|| Error::Runtime {
            code: -1,
            message: "model path is not representable as UTF-8".into(),
        })?;
        let params = sys::ModelLoadParams {
            struct_size: std::mem::size_of::<sys::ModelLoadParams>() as u32,
            flags: 0,
            path_utf8: utf8.as_ptr(),
            path_len: utf8.len() as u64,
            backend: options.backend.raw(),
            device_index: options.device_index,
            context_tokens_per_slot: options.context_tokens_per_slot,
            logical_batch_tokens: options.logical_batch_tokens,
            physical_batch_tokens: options.physical_batch_tokens,
            n_threads: options.n_threads,
            n_threads_batch: options.n_threads_batch,
            n_gpu_layers: options.n_gpu_layers,
            use_mmap: u32::from(options.use_mmap),
            use_mlock: u32::from(options.use_mlock),
            check_tensors: u32::from(options.check_tensors),
            reserved0: 0,
            reserved: [0; 12],
        };
        let _guard = self
            .inner
            .call_lock
            .lock()
            .expect("runtime call lock poisoned");
        let mut handle = 0;
        let mut error = sys::Error::default();
        check_result(
            unsafe {
                (self.inner.api.model_load)(self.inner.runtime, &params, &mut handle, &mut error)
            },
            &error,
        )?;
        Ok(Model {
            state: Arc::new(ModelState {
                runtime: self.inner.clone(),
                handle,
            }),
        })
    }

    pub fn scheduler_snapshot(&self) -> Result<sys::SchedulerSnapshot, Error> {
        let _guard = self
            .inner
            .call_lock
            .lock()
            .expect("runtime call lock poisoned");
        let mut value = sys::SchedulerSnapshot::default();
        let mut error = sys::Error::default();
        check_result(
            unsafe {
                (self.inner.api.get_scheduler_snapshot)(self.inner.runtime, &mut value, &mut error)
            },
            &error,
        )?;
        Ok(value)
    }

    pub fn metrics(&self) -> Result<sys::Metrics, Error> {
        let _guard = self
            .inner
            .call_lock
            .lock()
            .expect("runtime call lock poisoned");
        let mut value = sys::Metrics::default();
        let mut error = sys::Error::default();
        check_result(
            unsafe { (self.inner.api.get_metrics)(self.inner.runtime, &mut value, &mut error) },
            &error,
        )?;
        Ok(value)
    }
}

impl Model {
    pub fn handle(&self) -> u64 {
        self.state.handle
    }

    pub fn submit(
        &self,
        prompt: &[u8],
        options: GenerationOptions,
    ) -> Result<RequestStream, Error> {
        self.submit_inner(prompt, &[], options, None)
    }

    pub fn submit_with_correlation(
        &self,
        prompt: &[u8],
        options: GenerationOptions,
        correlation_id: u64,
    ) -> Result<RequestStream, Error> {
        self.submit_inner(prompt, &[], options, Some(correlation_id))
    }

    pub fn submit_chat(
        &self,
        messages: &[ChatMessage],
        options: GenerationOptions,
    ) -> Result<RequestStream, Error> {
        let prompt = messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content.as_bytes())
            .ok_or_else(|| Error::InvalidInput("chat requires a user message".into()))?;
        self.submit_inner(prompt, messages, options, None)
    }

    pub fn submit_chat_with_correlation(
        &self,
        messages: &[ChatMessage],
        options: GenerationOptions,
        correlation_id: u64,
    ) -> Result<RequestStream, Error> {
        let prompt = messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content.as_bytes())
            .ok_or_else(|| Error::InvalidInput("chat requires a user message".into()))?;
        self.submit_inner(prompt, messages, options, Some(correlation_id))
    }

    fn submit_inner(
        &self,
        prompt: &[u8],
        messages: &[ChatMessage],
        options: GenerationOptions,
        correlation_id: Option<u64>,
    ) -> Result<RequestStream, Error> {
        if correlation_id == Some(0) {
            return Err(Error::InvalidInput(
                "request correlation id must be non-zero".into(),
            ));
        }
        let state = Arc::new(RequestState {
            request_correlation: correlation_id.map(Box::new),
            ..RequestState::default()
        });
        let stop_storage = options.stop_sequences;
        let stop_ffi: Vec<sys::Bytes> = stop_storage
            .iter()
            .map(|stop| sys::Bytes {
                struct_size: std::mem::size_of::<sys::Bytes>() as u32,
                flags: 0,
                data: stop.as_ptr(),
                len: stop.len() as u64,
                reserved: [0; 8],
            })
            .collect();
        let chat_ffi: Vec<sys::ChatMessage> = messages
            .iter()
            .map(|message| sys::ChatMessage {
                struct_size: std::mem::size_of::<sys::ChatMessage>() as u32,
                flags: 0,
                role: sys::Bytes {
                    struct_size: std::mem::size_of::<sys::Bytes>() as u32,
                    flags: 0,
                    data: message.role.as_ptr(),
                    len: message.role.len() as u64,
                    reserved: [0; 8],
                },
                content: sys::Bytes {
                    struct_size: std::mem::size_of::<sys::Bytes>() as u32,
                    flags: 0,
                    data: message.content.as_ptr(),
                    len: message.content.len() as u64,
                    reserved: [0; 8],
                },
                reserved: [0; 8],
            })
            .collect();
        let params = sys::RequestParams {
            struct_size: std::mem::size_of::<sys::RequestParams>() as u32,
            flags: 0,
            model_handle: self.state.handle,
            prompt: prompt.as_ptr(),
            prompt_len: prompt.len() as u64,
            max_new_tokens: options.max_new_tokens,
            seed: options.seed,
            temperature: options.temperature,
            top_k: options.top_k,
            top_p: options.top_p,
            min_p: options.min_p,
            repeat_last_n: options.repeat_last_n,
            repeat_penalty: options.repeat_penalty,
            frequency_penalty: options.frequency_penalty,
            presence_penalty: options.presence_penalty,
            stop_count: stop_ffi.len() as u32,
            reserved0: 0,
            stop_sequences: if stop_ffi.is_empty() {
                std::ptr::null()
            } else {
                stop_ffi.as_ptr()
            },
            request_user_data: state
                .request_correlation
                .as_deref()
                .map_or(std::ptr::null_mut(), |value| {
                    (value as *const u64 as *mut u64).cast()
                }),
            chat_messages: if chat_ffi.is_empty() {
                std::ptr::null()
            } else {
                chat_ffi.as_ptr()
            },
            chat_message_count: chat_ffi.len() as u32,
            reserved1: 0,
            reserved: [0; 10],
        };
        let _guard = self
            .state
            .runtime
            .call_lock
            .lock()
            .expect("runtime call lock poisoned");
        let mut handle = 0;
        let mut error = sys::Error::default();
        let (terminal_sender, terminal) = crossbeam_channel::bounded(1);
        check_result(
            unsafe {
                (self.state.runtime.api.request_submit)(
                    self.state.runtime.runtime,
                    &params,
                    &mut handle,
                    &mut error,
                )
            },
            &error,
        )?;
        self.state
            .runtime
            .callback_state
            .register(handle, terminal_sender, state.clone());
        Ok(RequestStream {
            model: self.state.clone(),
            handle,
            state,
            terminal,
        })
    }
}
