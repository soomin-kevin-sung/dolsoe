use tauri::State;

use crate::agent_loop::AgentController;
use crate::conversation_store::ConversationStore;
use crate::llm_dto::{
    LlmMetricsDto, LlmStatusDto, LoadModelRequest, SubmitChatMessage, SubmitRequest, SubmitResponse,
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
    runtime_state: State<'_, RuntimeHost>,
    conversation_state: State<'_, ConversationStore>,
    agent_state: State<'_, AgentController>,
    mut request: SubmitRequest,
) -> Result<SubmitResponse, String> {
    if request.conversation_id.trim().is_empty() {
        return Err("conversationId must not be empty".into());
    }
    let conversation_id = request.conversation_id.clone();
    let conversations = conversation_state.inner().clone();
    let snapshot = blocking(move || conversations.prompt_snapshot(&conversation_id))
        .await?
        .ok_or_else(|| "conversation system prompt snapshot was not initialized".to_string())?;
    inject_system_message(&mut request.messages, &snapshot.system_prompt)?;
    let runtime = runtime_state.inner().clone();
    let agent = agent_state.inner().clone();
    blocking(move || agent.submit(&runtime, request)).await
}

fn inject_system_message(
    messages: &mut Vec<SubmitChatMessage>,
    system_prompt: &str,
) -> Result<(), String> {
    if messages.iter().any(|message| message.role == "system") {
        return Err("system messages are managed by the persona prompt pipeline".into());
    }
    if !system_prompt.is_empty() {
        messages.insert(
            0,
            SubmitChatMessage {
                role: "system".into(),
                content: system_prompt.into(),
            },
        );
    }
    Ok(())
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
    use super::{inject_system_message, parse_request_handle};
    use crate::llm_dto::SubmitChatMessage;

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

    #[test]
    fn injects_exactly_one_managed_system_message_first() {
        let mut messages = vec![SubmitChatMessage {
            role: "user".into(),
            content: "hello".into(),
        }];
        inject_system_message(&mut messages, "persona").unwrap();

        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[0].content, "persona");
        assert_eq!(messages[1].role, "user");
        assert!(inject_system_message(&mut messages, "duplicate").is_err());
    }
}
