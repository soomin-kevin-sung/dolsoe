use tauri::State;

use crate::llm_dto::{
    LlmMetricsDto, LlmStatusDto, LoadModelRequest, SubmitRequest, SubmitResponse,
};
use crate::runtime_host::RuntimeHost;

pub fn parse_request_handle(value: &str) -> Result<u64, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("request handle must be a decimal unsigned 64-bit integer".into());
    }
    value
        .parse::<u64>()
        .map_err(|_| "request handle is outside the unsigned 64-bit range".into())
}

async fn blocking<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| format!("LLM command task failed: {error}"))?
}

#[tauri::command]
pub async fn llm_get_status(state: State<'_, RuntimeHost>) -> Result<LlmStatusDto, String> {
    let worker = state.inner().clone();
    blocking(move || worker.status()).await
}

#[tauri::command]
pub async fn llm_load_model(
    state: State<'_, RuntimeHost>,
    request: LoadModelRequest,
) -> Result<LlmStatusDto, String> {
    let worker = state.inner().clone();
    blocking(move || worker.load_model(request)).await
}

#[tauri::command]
pub async fn llm_unload_model(state: State<'_, RuntimeHost>) -> Result<LlmStatusDto, String> {
    let worker = state.inner().clone();
    blocking(move || worker.unload_model()).await
}

#[tauri::command]
pub async fn llm_submit(
    state: State<'_, RuntimeHost>,
    request: SubmitRequest,
) -> Result<SubmitResponse, String> {
    let worker = state.inner().clone();
    blocking(move || worker.submit(request)).await
}

#[tauri::command]
pub async fn llm_cancel(
    state: State<'_, RuntimeHost>,
    request_handle: String,
) -> Result<(), String> {
    let handle = parse_request_handle(&request_handle)?;
    let worker = state.inner().clone();
    blocking(move || worker.cancel(handle)).await
}

#[tauri::command]
pub async fn llm_get_metrics(state: State<'_, RuntimeHost>) -> Result<LlmMetricsDto, String> {
    let worker = state.inner().clone();
    blocking(move || worker.metrics()).await
}

#[cfg(test)]
mod tests {
    use super::parse_request_handle;

    #[test]
    fn parses_full_width_decimal_request_handles() {
        assert_eq!(
            parse_request_handle("18446744073709551615").unwrap(),
            u64::MAX
        );
    }

    #[test]
    fn rejects_non_decimal_request_handles() {
        assert!(parse_request_handle("42.0").is_err());
        assert!(parse_request_handle("-1").is_err());
        assert!(parse_request_handle("").is_err());
    }
}
