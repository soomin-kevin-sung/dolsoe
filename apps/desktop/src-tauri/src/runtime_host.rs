use crate::{
    llm_dto::{
        LlmMetricsDto, LlmPhase, LlmStatusDto, LoadModelRequest, SubmitRequest, SubmitResponse,
    },
    llm_worker::WorkerHandle,
    runtime_path::RuntimePackResolver,
};
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Emitter};

#[derive(Clone)]
enum RuntimeHostState {
    Ready(WorkerHandle),
    RecoveryRequired(String),
}

#[derive(Clone)]
pub struct RuntimeHost {
    state: Arc<RwLock<RuntimeHostState>>,
}

impl RuntimeHost {
    pub fn ready(worker: WorkerHandle) -> Self {
        Self {
            state: Arc::new(RwLock::new(RuntimeHostState::Ready(worker))),
        }
    }

    pub fn recovery(error: impl Into<String>) -> Self {
        Self {
            state: Arc::new(RwLock::new(RuntimeHostState::RecoveryRequired(
                error.into(),
            ))),
        }
    }

    pub fn has_worker(&self) -> Result<bool, String> {
        self.state
            .read()
            .map(|state| matches!(*state, RuntimeHostState::Ready(_)))
            .map_err(|_| "runtime host lock poisoned".into())
    }

    pub fn activate(&self, worker: WorkerHandle) -> Result<(), String> {
        let mut state = self
            .state
            .write()
            .map_err(|_| "runtime host lock poisoned")?;
        if matches!(*state, RuntimeHostState::Ready(_)) {
            return Err("runtime host already has an active worker".into());
        }
        *state = RuntimeHostState::Ready(worker);
        Ok(())
    }

    pub fn status(&self) -> Result<LlmStatusDto, String> {
        match self.snapshot()? {
            RuntimeHostState::Ready(worker) => worker.status(),
            RuntimeHostState::RecoveryRequired(error) => Ok(LlmStatusDto {
                phase: LlmPhase::Error,
                last_error: Some(error),
                ..LlmStatusDto::default()
            }),
        }
    }
    pub fn load_model(&self, request: LoadModelRequest) -> Result<LlmStatusDto, String> {
        self.worker()?.load_model(request)
    }
    pub fn unload_model(&self) -> Result<LlmStatusDto, String> {
        self.worker()?.unload_model()
    }
    pub fn submit(&self, request: SubmitRequest) -> Result<SubmitResponse, String> {
        self.worker()?.submit(request)
    }
    pub fn cancel(&self, handle: u64) -> Result<(), String> {
        self.worker()?.cancel(handle)
    }
    pub fn metrics(&self) -> Result<LlmMetricsDto, String> {
        self.worker()?.metrics()
    }

    fn snapshot(&self) -> Result<RuntimeHostState, String> {
        self.state
            .read()
            .map(|state| state.clone())
            .map_err(|_| "runtime host lock poisoned".into())
    }

    fn worker(&self) -> Result<WorkerHandle, String> {
        match self.snapshot()? {
            RuntimeHostState::Ready(worker) => Ok(worker),
            RuntimeHostState::RecoveryRequired(error) => Err(error),
        }
    }
}

pub fn spawn_runtime_worker(
    app: AppHandle,
    resolver: RuntimePackResolver,
) -> Result<WorkerHandle, String> {
    WorkerHandle::spawn(resolver, move |event| {
        app.emit("llm://event", event)
            .map_err(|error| error.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn recovery_host_reports_status_without_starting_a_worker() {
        let host = RuntimeHost::recovery("CPU runtime recovery required");
        let status = host.status().unwrap();
        assert_eq!(status.phase, crate::llm_dto::LlmPhase::Error);
        assert_eq!(
            status.last_error.as_deref(),
            Some("CPU runtime recovery required")
        );
        assert!(!host.has_worker().unwrap());
        assert!(host.metrics().unwrap_err().contains("CPU runtime"));
    }

    #[test]
    fn recovery_host_can_activate_a_worker_without_process_restart() {
        let root = TempDir::new().unwrap();
        let worker = WorkerHandle::spawn(
            RuntimePackResolver::new(root.path().join("runtime-packs")),
            |_| Ok(()),
        )
        .unwrap();
        let host = RuntimeHost::recovery("CPU runtime recovery required");

        host.activate(worker).unwrap();

        assert!(host.has_worker().unwrap());
        assert_eq!(host.status().unwrap().phase, LlmPhase::NoModel);
        let replacement = WorkerHandle::spawn(
            RuntimePackResolver::new(root.path().join("runtime-packs")),
            |_| Ok(()),
        )
        .unwrap();
        assert!(host.activate(replacement).unwrap_err().contains("already"));
    }
}
