use crate::{
    llm_dto::{
        LlmMetricsDto, LlmPhase, LlmStatusDto, LoadModelRequest, SubmitRequest, SubmitResponse,
    },
    llm_worker::WorkerHandle,
};

#[derive(Clone)]
pub enum RuntimeHost {
    Ready(WorkerHandle),
    RecoveryRequired(String),
}

impl RuntimeHost {
    pub fn ready(worker: WorkerHandle) -> Self {
        Self::Ready(worker)
    }
    pub fn recovery(error: impl Into<String>) -> Self {
        Self::RecoveryRequired(error.into())
    }

    pub fn status(&self) -> Result<LlmStatusDto, String> {
        match self {
            Self::Ready(worker) => worker.status(),
            Self::RecoveryRequired(error) => Ok(LlmStatusDto {
                phase: LlmPhase::Error,
                last_error: Some(error.clone()),
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

    fn worker(&self) -> Result<&WorkerHandle, String> {
        match self {
            Self::Ready(worker) => Ok(worker),
            Self::RecoveryRequired(error) => Err(error.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_host_reports_status_without_starting_a_worker() {
        let host = RuntimeHost::recovery("CPU runtime recovery required");
        let status = host.status().unwrap();
        assert_eq!(status.phase, crate::llm_dto::LlmPhase::Error);
        assert_eq!(
            status.last_error.as_deref(),
            Some("CPU runtime recovery required")
        );
        assert!(host.metrics().unwrap_err().contains("CPU runtime"));
    }
}
