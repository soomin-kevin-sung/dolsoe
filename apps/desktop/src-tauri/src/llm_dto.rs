use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LlmPhase {
    #[default]
    NoModel,
    Loading,
    Ready,
    Streaming,
    Error,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LlmStatusDto {
    pub phase: LlmPhase,
    pub runtime_pack_id: Option<String>,
    pub model_path: Option<String>,
    pub model_name: Option<String>,
    pub backend: Option<String>,
    pub loading_progress: Option<f32>,
    pub active_request_handle: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoadModelRequest {
    pub runtime_pack_id: String,
    pub model_path: String,
    pub context_size: u32,
    pub batch_size: u32,
    pub physical_batch_size: u32,
    pub threads: i32,
    pub use_mmap: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubmitRequest {
    pub prompt: String,
    pub max_new_tokens: u32,
    pub temperature: f32,
    pub top_p: f32,
    pub seed: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubmitResponse {
    pub request_handle: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LlmMetricsDto {
    pub prompt_tokens: String,
    pub generated_tokens: String,
    pub cancelled_requests: String,
    pub failed_requests: String,
    pub queue_wait_nanoseconds: String,
    pub decode_nanoseconds: String,
    pub tokens_per_second: f64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LlmEventKind {
    ModelProgress,
    Queued,
    Token,
    Metrics,
    Done,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LlmEventDto {
    pub kind: LlmEventKind,
    pub request_handle: Option<String>,
    pub sequence_number: String,
    pub bytes: Vec<u8>,
    pub error_code: i32,
    pub metrics: Option<LlmMetricsDto>,
}

#[cfg(test)]
mod tests {
    use super::{LlmEventDto, LlmEventKind, LlmMetricsDto};

    #[test]
    fn serializes_native_counters_as_decimal_strings() {
        let dto = LlmEventDto {
            kind: LlmEventKind::Metrics,
            request_handle: Some(u64::MAX.to_string()),
            sequence_number: u64::MAX.to_string(),
            bytes: vec![0xed, 0x95, 0x9c],
            error_code: 0,
            metrics: Some(LlmMetricsDto {
                prompt_tokens: "12".into(),
                generated_tokens: u64::MAX.to_string(),
                cancelled_requests: "1".into(),
                failed_requests: "0".into(),
                queue_wait_nanoseconds: "25".into(),
                decode_nanoseconds: "50".into(),
                tokens_per_second: 20.0,
            }),
        };

        let value = serde_json::to_value(dto).expect("serialize event DTO");

        assert_eq!(value["requestHandle"], u64::MAX.to_string());
        assert_eq!(value["sequenceNumber"], u64::MAX.to_string());
        assert_eq!(value["metrics"]["generatedTokens"], u64::MAX.to_string());
        assert_eq!(value["bytes"], serde_json::json!([237, 149, 156]));
    }
}
