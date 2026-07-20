use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{bounded, select, Receiver, RecvTimeoutError, Sender, TryRecvError};
use llm_runtime::{
    Backend, ChatMessage, EventKind, GenerationOptions, InferenceRuntime, Model, ModelOptions,
    RequestStream, RuntimeEvent, RuntimeOptions,
};

use crate::llm_dto::{
    LlmEventDto, LlmMetricsDto, LlmPhase, LlmStatusDto, LoadModelRequest, SubmitRequest,
    SubmitResponse,
};
use crate::runtime_path::RuntimePackResolver;

pub const WORKER_COMMAND_CAPACITY: usize = 32;
const RELAY_CONTROL_CAPACITY: usize = 8;
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(5);

type EventSink = Arc<dyn Fn(LlmEventDto) -> Result<(), String> + Send + Sync>;
type WorkerResult<T> = Result<T, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerError {
    Busy,
    ModelNotLoaded,
    RequestNotFound,
    InvalidState,
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Busy => "runtime is busy",
            Self::ModelNotLoaded => "model is not loaded",
            Self::RequestNotFound => "request handle was not found",
            Self::InvalidState => "runtime state transition is invalid",
        };
        formatter.write_str(message)
    }
}

#[derive(Debug, Default)]
pub struct WorkerGuard {
    phase: LlmPhase,
    active_request_handle: Option<u64>,
    submit_in_progress: bool,
    cancel_requested: bool,
}

impl WorkerGuard {
    pub fn phase(&self) -> LlmPhase {
        self.phase
    }

    pub fn active_request_handle(&self) -> Option<u64> {
        self.active_request_handle
    }

    pub fn begin_load(&mut self) -> Result<(), WorkerError> {
        if matches!(self.phase, LlmPhase::Loading | LlmPhase::Streaming) {
            return Err(WorkerError::Busy);
        }
        self.phase = LlmPhase::Loading;
        Ok(())
    }

    pub fn finish_load(&mut self) -> Result<(), WorkerError> {
        if self.phase != LlmPhase::Loading {
            return Err(WorkerError::InvalidState);
        }
        self.phase = LlmPhase::Ready;
        Ok(())
    }

    pub fn fail(&mut self) {
        self.phase = LlmPhase::Error;
        self.submit_in_progress = false;
        self.active_request_handle = None;
        self.cancel_requested = false;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn begin_submit(&mut self) -> Result<(), WorkerError> {
        if self.phase == LlmPhase::NoModel {
            return Err(WorkerError::ModelNotLoaded);
        }
        if self.phase != LlmPhase::Ready
            || self.submit_in_progress
            || self.active_request_handle.is_some()
        {
            return Err(WorkerError::Busy);
        }
        self.submit_in_progress = true;
        self.phase = LlmPhase::Streaming;
        Ok(())
    }

    pub fn assign_request_handle(&mut self, handle: u64) -> Result<(), WorkerError> {
        if !self.submit_in_progress || handle == 0 {
            return Err(WorkerError::InvalidState);
        }
        self.submit_in_progress = false;
        self.active_request_handle = Some(handle);
        Ok(())
    }

    pub fn abort_submit(&mut self) {
        self.submit_in_progress = false;
        self.phase = LlmPhase::Ready;
    }

    pub fn cancel(&mut self, handle: u64) -> Result<bool, WorkerError> {
        if self.active_request_handle != Some(handle) {
            return Err(WorkerError::RequestNotFound);
        }
        if self.cancel_requested {
            return Ok(false);
        }
        self.cancel_requested = true;
        Ok(true)
    }

    pub fn finish_request(&mut self, handle: u64, failed: bool) -> Result<(), WorkerError> {
        if self.active_request_handle != Some(handle) {
            return Err(WorkerError::RequestNotFound);
        }
        self.active_request_handle = None;
        self.cancel_requested = false;
        self.phase = if failed {
            LlmPhase::Error
        } else {
            LlmPhase::Ready
        };
        Ok(())
    }
}

pub fn validate_model_path(path: &Path) -> WorkerResult<PathBuf> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize model path: {error}"))?;
    if !canonical.is_file() {
        return Err(format!("model path is not a file: {}", canonical.display()));
    }
    let is_gguf = canonical
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("gguf"));
    if !is_gguf {
        return Err("model file must use the .gguf extension".into());
    }
    Ok(canonical)
}

pub fn generation_seed(seed: i64) -> WorkerResult<u32> {
    if seed == -1 {
        return Ok(u32::MAX);
    }
    u32::try_from(seed).map_err(|_| "seed must be -1 or an unsigned 32-bit integer".into())
}

fn model_target(
    backend: &str,
    device_index: u32,
) -> WorkerResult<(Backend, u32, i32, &'static str)> {
    match backend {
        "cpu" => Ok((Backend::Cpu, device_index, 0, "CPU")),
        "cuda" => Ok((Backend::Cuda, device_index, -1, "CUDA")),
        "vulkan" => Ok((Backend::Vulkan, device_index, -1, "Vulkan")),
        _ => Err("backend must be cpu, cuda, or vulkan".into()),
    }
}

enum RelayCommand {
    Terminal(RuntimeEvent, Sender<()>),
    Shutdown(Sender<()>),
}

pub struct EventRelayHandle {
    control: Sender<RelayCommand>,
    failures: Receiver<u64>,
    join: Option<JoinHandle<()>>,
}

impl EventRelayHandle {
    pub fn send_terminal(&self, event: RuntimeEvent) -> WorkerResult<()> {
        let (ack_tx, ack_rx) = bounded(1);
        self.control
            .send(RelayCommand::Terminal(event, ack_tx))
            .map_err(|_| "event relay is unavailable".to_string())?;
        ack_rx
            .recv()
            .map_err(|_| "event relay dropped terminal acknowledgement".to_string())
    }

    pub fn failure_receiver(&self) -> Receiver<u64> {
        self.failures.clone()
    }

    pub fn shutdown(&mut self) {
        if self.join.is_none() {
            return;
        }
        let (ack_tx, ack_rx) = bounded(1);
        let _ = self.control.send(RelayCommand::Shutdown(ack_tx));
        let _ = ack_rx.recv();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for EventRelayHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
fn spawn_event_relay<F>(events: Receiver<RuntimeEvent>, sink: F) -> WorkerResult<EventRelayHandle>
where
    F: Fn(LlmEventDto) -> Result<(), String> + Send + Sync + 'static,
{
    spawn_event_relay_arc(events, Arc::new(sink))
}

fn spawn_event_relay_arc(
    events: Receiver<RuntimeEvent>,
    sink: EventSink,
) -> WorkerResult<EventRelayHandle> {
    let (control_tx, control_rx) = bounded(RELAY_CONTROL_CAPACITY);
    let (failure_tx, failure_rx) = bounded(1);
    let join = std::thread::Builder::new()
        .name("llm-event-relay".into())
        .spawn(move || relay_loop(events, control_rx, failure_tx, sink))
        .map_err(|error| format!("failed to start event relay: {error}"))?;
    Ok(EventRelayHandle {
        control: control_tx,
        failures: failure_rx,
        join: Some(join),
    })
}

fn emit_runtime_event(event: RuntimeEvent, failure_tx: &Sender<u64>, sink: &EventSink) {
    let handle = event.request_handle;
    let Some(dto) = LlmEventDto::from_runtime_event(event) else {
        return;
    };
    if sink(dto).is_err() && handle != 0 {
        let _ = failure_tx.try_send(handle);
    }
}

fn relay_loop(
    events: Receiver<RuntimeEvent>,
    control: Receiver<RelayCommand>,
    failure_tx: Sender<u64>,
    sink: EventSink,
) {
    loop {
        select! {
            recv(control) -> command => match command {
                Ok(RelayCommand::Terminal(terminal, ack)) => {
                    while let Ok(event) = events.try_recv() {
                        emit_runtime_event(event, &failure_tx, &sink);
                    }
                    emit_runtime_event(terminal, &failure_tx, &sink);
                    let _ = ack.send(());
                }
                Ok(RelayCommand::Shutdown(ack)) => {
                    while let Ok(event) = events.try_recv() {
                        emit_runtime_event(event, &failure_tx, &sink);
                    }
                    let _ = ack.send(());
                    break;
                }
                Err(_) => break,
            },
            recv(events) -> event => match event {
                Ok(event) => emit_runtime_event(event, &failure_tx, &sink),
                Err(_) => break,
            },
        }
    }
}

enum WorkerCommand {
    GetStatus(Sender<WorkerResult<LlmStatusDto>>),
    LoadModel(LoadModelRequest, Sender<WorkerResult<LlmStatusDto>>),
    UnloadModel(Sender<WorkerResult<LlmStatusDto>>),
    Submit(SubmitRequest, Sender<WorkerResult<SubmitResponse>>),
    Cancel(u64, Sender<WorkerResult<()>>),
    GetMetrics(Sender<WorkerResult<LlmMetricsDto>>),
    Shutdown,
}

#[derive(Default)]
struct NativeState {
    runtime: Option<InferenceRuntime>,
    model: Option<Model>,
    request: Option<RequestStream>,
    request_terminal: Option<Receiver<RuntimeEvent>>,
    relay: Option<EventRelayHandle>,
    guard: WorkerGuard,
    status: LlmStatusDto,
}

impl NativeState {
    fn status(&self) -> LlmStatusDto {
        let mut status = self.status.clone();
        status.phase = self.guard.phase();
        status.active_request_handle = self
            .guard
            .active_request_handle()
            .map(|value| value.to_string());
        status
    }

    fn set_error(&mut self, error: impl Into<String>) -> String {
        let error = error.into();
        self.guard.fail();
        self.status.last_error = Some(error.clone());
        error
    }

    fn load_model(
        &mut self,
        resolver: &RuntimePackResolver,
        sink: &EventSink,
        request: LoadModelRequest,
    ) -> WorkerResult<LlmStatusDto> {
        if self.guard.phase() == LlmPhase::Loading {
            return Err(WorkerError::Busy.to_string());
        }
        self.cancel_and_finish_active();
        self.unload_native();
        self.guard.reset();
        self.guard.begin_load().map_err(|error| error.to_string())?;

        let result = (|| {
            if !(256..=131_072).contains(&request.context_size) {
                return Err("context size must be between 256 and 131072".into());
            }
            if request.batch_size == 0 || request.physical_batch_size == 0 {
                return Err("batch sizes must be positive".into());
            }
            if !(1..=256).contains(&request.threads) {
                return Err("thread count must be between 1 and 256".into());
            }
            let model_path = validate_model_path(Path::new(&request.model_path))?;
            let runtime_path = resolver.resolve(&request.runtime_pack_id).map_err(|error| {
                format!(
                    "{error}. Prepare the development CPU pack with scripts/prepare-dev-cpu-pack.ps1 (trusted root: {})",
                    resolver.runtime_root().display()
                )
            })?;
            let runtime = unsafe {
                InferenceRuntime::load(
                    &runtime_path,
                    RuntimeOptions {
                        slot_count: 1,
                        request_queue_capacity: 16,
                        event_queue_capacity: 1024,
                    },
                )
            }
            .map_err(|error| error.to_string())?;
            let relay = spawn_event_relay_arc(runtime.events(), sink.clone())?;
            self.runtime = Some(runtime);
            self.relay = Some(relay);

            let (backend, device_index, n_gpu_layers, backend_label) =
                model_target(&request.backend, request.device_index)?;
            let options = ModelOptions {
                backend,
                device_index,
                context_tokens_per_slot: request.context_size,
                logical_batch_tokens: request.batch_size,
                physical_batch_tokens: request.physical_batch_size,
                n_threads: request.threads,
                n_threads_batch: request.threads,
                n_gpu_layers,
                use_mmap: request.use_mmap,
                use_mlock: false,
                check_tensors: false,
            };
            let model = self
                .runtime
                .as_ref()
                .expect("runtime assigned before model load")
                .load_model(&model_path, options)
                .map_err(|error| error.to_string())?;
            self.model = Some(model);
            self.status = LlmStatusDto {
                phase: LlmPhase::Ready,
                runtime_pack_id: Some(request.runtime_pack_id),
                model_path: Some(model_path.display().to_string()),
                model_name: model_path
                    .file_name()
                    .map(|value| value.to_string_lossy().into_owned()),
                backend: Some(backend_label.into()),
                loading_progress: Some(1.0),
                active_request_handle: None,
                last_error: None,
            };
            self.guard
                .finish_load()
                .map_err(|error| error.to_string())?;
            Ok(self.status())
        })();

        if let Err(error) = result {
            self.unload_native();
            return Err(self.set_error(error));
        }
        result
    }

    fn submit(&mut self, request: SubmitRequest) -> WorkerResult<SubmitResponse> {
        self.guard
            .begin_submit()
            .map_err(|error| error.to_string())?;
        let result = (|| {
            if request.prompt.trim().is_empty() {
                return Err("prompt must not be empty".into());
            }
            if request.messages.len() > 128 {
                return Err("chat must contain at most 128 messages".into());
            }
            if request.messages.iter().any(|message| {
                !matches!(message.role.as_str(), "system" | "user" | "assistant")
                    || message.content.is_empty()
            }) {
                return Err(
                    "chat messages must have a supported role and non-empty content".into(),
                );
            }
            if request.max_new_tokens == 0 {
                return Err("max_new_tokens must be positive".into());
            }
            if !request.temperature.is_finite() || !(0.0..=2.0).contains(&request.temperature) {
                return Err("temperature must be between 0 and 2".into());
            }
            if !request.top_p.is_finite() || !(0.0..=1.0).contains(&request.top_p) {
                return Err("top_p must be between 0 and 1".into());
            }
            let options = GenerationOptions {
                max_new_tokens: request.max_new_tokens,
                seed: generation_seed(request.seed)?,
                temperature: request.temperature,
                top_p: request.top_p,
                ..GenerationOptions::default()
            };
            let model = self
                .model
                .as_ref()
                .ok_or_else(|| WorkerError::ModelNotLoaded.to_string())?;
            let stream = if request.messages.is_empty() {
                model.submit(request.prompt.as_bytes(), options)
            } else {
                let messages = request
                    .messages
                    .into_iter()
                    .map(|message| ChatMessage {
                        role: message.role,
                        content: message.content,
                    })
                    .collect::<Vec<_>>();
                model.submit_chat(&messages, options)
            }
            .map_err(|error| error.to_string())?;
            let handle = stream.handle();
            self.request_terminal = Some(stream.terminal_receiver());
            self.request = Some(stream);
            self.guard
                .assign_request_handle(handle)
                .map_err(|error| error.to_string())?;
            Ok(SubmitResponse {
                request_handle: handle.to_string(),
            })
        })();
        if result.is_err() {
            self.guard.abort_submit();
        }
        result
    }

    fn cancel(&mut self, handle: u64) -> WorkerResult<()> {
        let first = self
            .guard
            .cancel(handle)
            .map_err(|error| error.to_string())?;
        if first {
            self.request
                .as_ref()
                .ok_or_else(|| WorkerError::RequestNotFound.to_string())?
                .cancel()
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn metrics(&self) -> WorkerResult<LlmMetricsDto> {
        let raw = self
            .runtime
            .as_ref()
            .ok_or_else(|| WorkerError::ModelNotLoaded.to_string())?
            .metrics()
            .map_err(|error| error.to_string())?;
        Ok(LlmMetricsDto::from_counts(
            raw.prompt_tokens,
            raw.generated_tokens,
            raw.cancelled_requests,
            raw.failed_requests,
            raw.queue_wait_ns,
            raw.decode_ns,
        ))
    }

    fn poll(&mut self) {
        self.poll_relay_failures();
        let terminal = match self.request_terminal.as_ref().map(Receiver::try_recv) {
            Some(Ok(event)) => Some(event),
            Some(Err(TryRecvError::Disconnected)) => Some(RuntimeEvent {
                kind: EventKind::Error,
                data_format: 0,
                error_code: -1,
                model_handle: 0,
                request_handle: self.guard.active_request_handle().unwrap_or(0),
                slot_id: 0,
                sequence_number: 0,
                request_user_data: 0,
                payload: b"request terminal channel disconnected".to_vec(),
            }),
            _ => None,
        };
        if let Some(terminal) = terminal {
            self.finish_terminal(terminal);
        }
    }

    fn poll_relay_failures(&mut self) {
        let failures = self.relay.as_ref().map(EventRelayHandle::failure_receiver);
        let Some(failures) = failures else { return };
        while let Ok(handle) = failures.try_recv() {
            if self.guard.active_request_handle() == Some(handle) {
                let _ = self.cancel(handle);
            }
        }
    }

    fn finish_terminal(&mut self, terminal: RuntimeEvent) {
        let handle = terminal.request_handle;
        let failed = terminal.kind == EventKind::Error;
        if let Some(relay) = self.relay.as_ref() {
            let _ = relay.send_terminal(terminal);
        }
        self.request_terminal = None;
        self.request = None;
        if self.guard.finish_request(handle, failed).is_err() {
            self.guard.fail();
        }
        if failed {
            self.status.last_error = Some("generation failed".into());
        }
    }

    fn unload(&mut self) -> WorkerResult<LlmStatusDto> {
        self.cancel_and_finish_active();
        self.unload_native();
        self.guard.reset();
        self.status = LlmStatusDto::default();
        Ok(self.status())
    }

    fn cancel_and_finish_active(&mut self) {
        let Some(handle) = self.guard.active_request_handle() else {
            return;
        };
        let _ = self.cancel(handle);
        if let Some(receiver) = self.request_terminal.as_ref() {
            if let Ok(terminal) = receiver.recv() {
                self.finish_terminal(terminal);
            }
        }
    }

    fn unload_native(&mut self) {
        self.request_terminal = None;
        self.request = None;
        self.model = None;
        if let Some(mut relay) = self.relay.take() {
            relay.shutdown();
        }
        self.runtime = None;
    }
}

impl Drop for NativeState {
    fn drop(&mut self) {
        self.cancel_and_finish_active();
        self.unload_native();
    }
}

struct WorkerShared {
    sender: Sender<WorkerCommand>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for WorkerShared {
    fn drop(&mut self) {
        let _ = self.sender.send(WorkerCommand::Shutdown);
        if let Some(join) = self.join.get_mut().ok().and_then(Option::take) {
            let _ = join.join();
        }
    }
}

#[derive(Clone)]
pub struct WorkerHandle {
    shared: Arc<WorkerShared>,
}

impl WorkerHandle {
    pub fn spawn<F>(resolver: RuntimePackResolver, sink: F) -> WorkerResult<Self>
    where
        F: Fn(LlmEventDto) -> Result<(), String> + Send + Sync + 'static,
    {
        let (sender, receiver) = bounded(WORKER_COMMAND_CAPACITY);
        let sink: EventSink = Arc::new(sink);
        let join = std::thread::Builder::new()
            .name("llm-worker".into())
            .spawn(move || worker_loop(receiver, resolver, sink))
            .map_err(|error| format!("failed to start LLM worker: {error}"))?;
        Ok(Self {
            shared: Arc::new(WorkerShared {
                sender,
                join: Mutex::new(Some(join)),
            }),
        })
    }

    fn call<T>(
        &self,
        command: impl FnOnce(Sender<WorkerResult<T>>) -> WorkerCommand,
    ) -> WorkerResult<T> {
        let (response_tx, response_rx) = bounded(1);
        self.shared
            .sender
            .send(command(response_tx))
            .map_err(|_| "LLM worker is unavailable".to_string())?;
        response_rx
            .recv()
            .map_err(|_| "LLM worker dropped its response".to_string())?
    }

    pub fn status(&self) -> WorkerResult<LlmStatusDto> {
        self.call(WorkerCommand::GetStatus)
    }

    pub fn load_model(&self, request: LoadModelRequest) -> WorkerResult<LlmStatusDto> {
        self.call(|response| WorkerCommand::LoadModel(request, response))
    }

    pub fn unload_model(&self) -> WorkerResult<LlmStatusDto> {
        self.call(WorkerCommand::UnloadModel)
    }

    pub fn submit(&self, request: SubmitRequest) -> WorkerResult<SubmitResponse> {
        self.call(|response| WorkerCommand::Submit(request, response))
    }

    pub fn cancel(&self, handle: u64) -> WorkerResult<()> {
        self.call(|response| WorkerCommand::Cancel(handle, response))
    }

    pub fn metrics(&self) -> WorkerResult<LlmMetricsDto> {
        self.call(WorkerCommand::GetMetrics)
    }
}

fn worker_loop(receiver: Receiver<WorkerCommand>, resolver: RuntimePackResolver, sink: EventSink) {
    let mut state = NativeState::default();
    loop {
        match receiver.recv_timeout(WORKER_POLL_INTERVAL) {
            Ok(WorkerCommand::GetStatus(response)) => {
                let _ = response.send(Ok(state.status()));
            }
            Ok(WorkerCommand::LoadModel(request, response)) => {
                let result = state.load_model(&resolver, &sink, request);
                let _ = response.send(result);
            }
            Ok(WorkerCommand::UnloadModel(response)) => {
                let result = state.unload();
                let _ = response.send(result);
            }
            Ok(WorkerCommand::Submit(request, response)) => {
                let result = state.submit(request);
                let _ = response.send(result);
            }
            Ok(WorkerCommand::Cancel(handle, response)) => {
                let result = state.cancel(handle);
                let _ = response.send(result);
            }
            Ok(WorkerCommand::GetMetrics(response)) => {
                let _ = response.send(state.metrics());
            }
            Ok(WorkerCommand::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        state.poll();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crossbeam_channel::bounded;
    use llm_runtime::{Backend, EventKind, RuntimeEvent};

    use crate::llm_dto::LlmPhase;

    use super::{
        generation_seed, model_target, spawn_event_relay, validate_model_path, WorkerError,
        WorkerGuard, WORKER_COMMAND_CAPACITY,
    };

    #[test]
    fn model_target_maps_backend_and_gpu_layers() {
        let (cpu, cpu_index, cpu_layers, cpu_label) = model_target("cpu", 0).unwrap();
        let (cuda, cuda_index, cuda_layers, cuda_label) = model_target("cuda", 2).unwrap();
        let (vulkan, vulkan_index, vulkan_layers, vulkan_label) =
            model_target("vulkan", 1).unwrap();

        assert!(matches!(cpu, Backend::Cpu));
        assert_eq!((cpu_index, cpu_layers, cpu_label), (0, 0, "CPU"));
        assert!(matches!(cuda, Backend::Cuda));
        assert_eq!((cuda_index, cuda_layers, cuda_label), (2, -1, "CUDA"));
        assert!(matches!(vulkan, Backend::Vulkan));
        assert_eq!(
            (vulkan_index, vulkan_layers, vulkan_label),
            (1, -1, "Vulkan")
        );
    }

    #[test]
    fn model_target_rejects_unknown_backend() {
        assert!(model_target("metal", 0).is_err());
    }

    fn event(kind: EventKind, handle: u64, sequence: u64, bytes: &[u8]) -> RuntimeEvent {
        RuntimeEvent {
            kind,
            data_format: 1,
            error_code: 0,
            model_handle: 1,
            request_handle: handle,
            slot_id: 0,
            sequence_number: sequence,
            request_user_data: 0,
            payload: bytes.to_vec(),
        }
    }

    #[test]
    fn enforces_model_and_single_request_transitions() {
        let mut guard = WorkerGuard::default();
        assert_eq!(guard.begin_submit(), Err(WorkerError::ModelNotLoaded));

        guard.begin_load().unwrap();
        assert_eq!(guard.phase(), LlmPhase::Loading);
        guard.finish_load().unwrap();
        assert_eq!(guard.phase(), LlmPhase::Ready);

        guard.begin_submit().unwrap();
        guard.assign_request_handle(42).unwrap();
        assert_eq!(guard.begin_submit(), Err(WorkerError::Busy));
        assert_eq!(guard.phase(), LlmPhase::Streaming);
    }

    #[test]
    fn cancellation_is_idempotent_until_terminal_cleanup() {
        let mut guard = WorkerGuard::default();
        guard.begin_load().unwrap();
        guard.finish_load().unwrap();
        guard.begin_submit().unwrap();
        guard.assign_request_handle(42).unwrap();

        assert!(guard.cancel(42).unwrap());
        assert!(!guard.cancel(42).unwrap());
        guard.finish_request(42, false).unwrap();

        assert_eq!(guard.phase(), LlmPhase::Ready);
        assert_eq!(guard.active_request_handle(), None);
    }

    #[test]
    fn terminal_error_moves_worker_to_error() {
        let mut guard = WorkerGuard::default();
        guard.begin_load().unwrap();
        guard.finish_load().unwrap();
        guard.begin_submit().unwrap();
        guard.assign_request_handle(9).unwrap();

        guard.finish_request(9, true).unwrap();

        assert_eq!(guard.phase(), LlmPhase::Error);
    }

    #[test]
    fn command_queue_capacity_is_bounded() {
        assert_eq!(WORKER_COMMAND_CAPACITY, 32);
    }

    #[test]
    fn relay_drains_tokens_before_terminal() {
        let (events_tx, events_rx) = bounded(8);
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let captured = emitted.clone();
        let mut relay = spawn_event_relay(events_rx, move |dto| {
            captured.lock().unwrap().push(dto.kind);
            Ok(())
        })
        .unwrap();
        events_tx.send(event(EventKind::Token, 7, 1, b"a")).unwrap();
        relay
            .send_terminal(event(EventKind::Done, 7, 2, b""))
            .unwrap();
        relay.shutdown();

        assert_eq!(
            *emitted.lock().unwrap(),
            vec![
                crate::llm_dto::LlmEventKind::Token,
                crate::llm_dto::LlmEventKind::Done
            ]
        );
    }

    #[test]
    fn relay_reports_event_sink_failure() {
        let (events_tx, events_rx) = bounded(8);
        let relay = spawn_event_relay(events_rx, |_dto| Err("listener gone".into())).unwrap();
        events_tx
            .send(event(EventKind::Token, 19, 1, b"a"))
            .unwrap();

        assert_eq!(
            relay
                .failure_receiver()
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            19
        );
        drop(relay);
    }

    #[test]
    fn validates_gguf_files_and_generation_seed() {
        let temp = tempfile::tempdir().unwrap();
        let model = temp.path().join("model.gguf");
        fs::write(&model, b"fixture").unwrap();
        let text = temp.path().join("model.txt");
        fs::write(&text, b"fixture").unwrap();

        assert_eq!(
            validate_model_path(&model).unwrap(),
            model.canonicalize().unwrap()
        );
        assert!(validate_model_path(&text).is_err());
        assert_eq!(generation_seed(-1).unwrap(), u32::MAX);
        assert_eq!(generation_seed(42).unwrap(), 42);
        assert!(generation_seed(-2).is_err());
        assert!(generation_seed(i64::from(u32::MAX) + 1).is_err());
    }
}
