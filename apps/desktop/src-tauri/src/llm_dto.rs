use serde::{Deserialize, Serialize};

use llm_runtime::{EventKind, RuntimeEvent};

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
    pub backend: String,
    pub device_index: u32,
    pub model_path: String,
    pub context_size: u32,
    pub batch_size: u32,
    pub physical_batch_size: u32,
    pub threads: i32,
    pub use_mmap: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubmitChatMessage {
    pub role: String,
    pub content: String,
}

fn default_top_k() -> i32 {
    40
}

fn default_top_p() -> f32 {
    0.95
}

fn default_min_p() -> f32 {
    0.05
}

fn default_repeat_last_n() -> i32 {
    64
}

fn default_repeat_penalty() -> f32 {
    1.1
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubmitRequest {
    pub conversation_id: String,
    #[serde(default)]
    pub agent_run_id: Option<String>,
    #[serde(default)]
    pub agent_step_id: Option<String>,
    #[serde(skip)]
    pub correlation_id: u64,
    pub prompt: String,
    #[serde(default)]
    pub messages: Vec<SubmitChatMessage>,
    pub max_new_tokens: u32,
    pub temperature: f32,
    #[serde(default = "default_top_k")]
    pub top_k: i32,
    #[serde(default = "default_top_p")]
    pub top_p: f32,
    #[serde(default = "default_min_p")]
    pub min_p: f32,
    #[serde(default = "default_repeat_last_n")]
    pub repeat_last_n: i32,
    #[serde(default = "default_repeat_penalty")]
    pub repeat_penalty: f32,
    #[serde(default)]
    pub frequency_penalty: f32,
    #[serde(default)]
    pub presence_penalty: f32,
    #[serde(default)]
    pub stop_sequences: Vec<String>,
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
    pub correlation_id: Option<String>,
    pub sequence_number: String,
    pub bytes: Vec<u8>,
    pub error_code: i32,
    pub metrics: Option<LlmMetricsDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeMetricsPayload {
    prompt_tokens: u64,
    generated_tokens: u64,
    queue_wait_nanoseconds: u64,
    decode_nanoseconds: u64,
    #[serde(default)]
    cancelled_requests: u64,
    #[serde(default)]
    failed_requests: u64,
}

impl LlmMetricsDto {
    pub fn from_counts(
        prompt_tokens: u64,
        generated_tokens: u64,
        cancelled_requests: u64,
        failed_requests: u64,
        queue_wait_nanoseconds: u64,
        decode_nanoseconds: u64,
    ) -> Self {
        let tokens_per_second = if decode_nanoseconds == 0 {
            0.0
        } else {
            generated_tokens as f64 / (decode_nanoseconds as f64 / 1_000_000_000.0)
        };
        Self {
            prompt_tokens: prompt_tokens.to_string(),
            generated_tokens: generated_tokens.to_string(),
            cancelled_requests: cancelled_requests.to_string(),
            failed_requests: failed_requests.to_string(),
            queue_wait_nanoseconds: queue_wait_nanoseconds.to_string(),
            decode_nanoseconds: decode_nanoseconds.to_string(),
            tokens_per_second,
        }
    }
}

impl LlmEventDto {
    pub fn from_runtime_event(event: RuntimeEvent) -> Option<Self> {
        let kind = match event.kind {
            EventKind::ModelProgress => LlmEventKind::ModelProgress,
            EventKind::Queued => LlmEventKind::Queued,
            EventKind::Token => LlmEventKind::Token,
            EventKind::Metrics => LlmEventKind::Metrics,
            EventKind::Done => LlmEventKind::Done,
            EventKind::Cancelled => LlmEventKind::Cancelled,
            EventKind::Error => LlmEventKind::Error,
            EventKind::Log => return None,
        };
        let metrics = if event.kind == EventKind::Metrics {
            serde_json::from_slice::<RuntimeMetricsPayload>(&event.payload)
                .ok()
                .map(|value| {
                    LlmMetricsDto::from_counts(
                        value.prompt_tokens,
                        value.generated_tokens,
                        value.cancelled_requests,
                        value.failed_requests,
                        value.queue_wait_nanoseconds,
                        value.decode_nanoseconds,
                    )
                })
        } else {
            None
        };
        Some(Self {
            kind,
            request_handle: (event.request_handle != 0).then(|| event.request_handle.to_string()),
            correlation_id: (event.request_user_data != 0)
                .then(|| event.request_user_data.to_string()),
            sequence_number: event.sequence_number.to_string(),
            bytes: event.payload,
            error_code: event.error_code,
            metrics,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{LlmEventDto, LlmEventKind, LlmMetricsDto, SubmitRequest};

    #[test]
    fn serializes_native_counters_as_decimal_strings() {
        let dto = LlmEventDto {
            kind: LlmEventKind::Metrics,
            request_handle: Some(u64::MAX.to_string()),
            correlation_id: Some("7".into()),
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
        assert_eq!(value["correlationId"], "7");
        assert_eq!(value["sequenceNumber"], u64::MAX.to_string());
        assert_eq!(value["metrics"]["generatedTokens"], u64::MAX.to_string());
        assert_eq!(value["bytes"], serde_json::json!([237, 149, 156]));
    }

    #[test]
    fn defaults_advanced_generation_options_for_older_requests() {
        let request: SubmitRequest = serde_json::from_value(serde_json::json!({
            "conversationId": "conversation-1",
            "prompt": "hello",
            "messages": [{ "role": "user", "content": "hello" }],
            "maxNewTokens": 128,
            "temperature": 0.8,
            "topP": 0.95,
            "seed": -1
        }))
        .expect("deserialize submit request");

        assert_eq!(request.top_k, 40);
        assert_eq!(request.min_p, 0.05);
        assert_eq!(request.repeat_last_n, 64);
        assert_eq!(request.repeat_penalty, 1.1);
        assert_eq!(request.frequency_penalty, 0.0);
        assert_eq!(request.presence_penalty, 0.0);
        assert!(request.stop_sequences.is_empty());
    }

    #[test]
    fn deserializes_advanced_generation_options() {
        let request: SubmitRequest = serde_json::from_value(serde_json::json!({
            "conversationId": "conversation-1",
            "prompt": "hello",
            "messages": [],
            "maxNewTokens": 128,
            "temperature": 0.7,
            "topK": 20,
            "topP": 0.9,
            "minP": 0.1,
            "repeatLastN": 128,
            "repeatPenalty": 1.2,
            "frequencyPenalty": 0.2,
            "presencePenalty": 0.1,
            "stopSequences": ["<END>"],
            "seed": 42
        }))
        .expect("deserialize advanced submit request");

        assert_eq!(request.top_k, 20);
        assert_eq!(request.min_p, 0.1);
        assert_eq!(request.repeat_last_n, 128);
        assert_eq!(request.repeat_penalty, 1.2);
        assert_eq!(request.stop_sequences, vec!["<END>"]);
    }
}
