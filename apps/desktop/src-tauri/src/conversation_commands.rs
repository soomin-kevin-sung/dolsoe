use serde::Deserialize;
use tauri::State;

use crate::conversation_store::{
    ConversationBootstrap, ConversationDetail, ConversationPromptSnapshot, ConversationStore,
    ConversationSummary, MessageStatus, StartedTurn,
};
use crate::persona_prompt::PersonaPromptStore;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishTurnRequest {
    pub assistant_message_id: String,
    pub content: String,
    pub status: MessageStatus,
}

impl FinishTurnRequest {
    fn validate_status(status: MessageStatus) -> Result<(), String> {
        if matches!(
            status,
            MessageStatus::Complete
                | MessageStatus::Cancelled
                | MessageStatus::Interrupted
                | MessageStatus::Error
        ) {
            Ok(())
        } else {
            Err("finish turn requires a terminal message status".into())
        }
    }
}

async fn blocking<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| format!("conversation command task failed: {error}"))?
}

#[tauri::command]
pub async fn conversation_bootstrap(
    state: State<'_, ConversationStore>,
) -> Result<ConversationBootstrap, String> {
    let store = state.inner().clone();
    blocking(move || store.bootstrap()).await
}

#[tauri::command]
pub async fn conversation_load(
    state: State<'_, ConversationStore>,
    conversation_id: String,
) -> Result<ConversationDetail, String> {
    let store = state.inner().clone();
    blocking(move || store.load_conversation(&conversation_id)).await
}

#[tauri::command]
pub async fn conversation_rename(
    state: State<'_, ConversationStore>,
    conversation_id: String,
    title: String,
) -> Result<ConversationSummary, String> {
    let store = state.inner().clone();
    blocking(move || store.rename_conversation(&conversation_id, &title)).await
}

#[tauri::command]
pub async fn conversation_clear(
    state: State<'_, ConversationStore>,
    conversation_id: String,
) -> Result<ConversationDetail, String> {
    let store = state.inner().clone();
    blocking(move || store.clear_conversation(&conversation_id)).await
}

#[tauri::command]
pub async fn conversation_delete(
    state: State<'_, ConversationStore>,
    conversation_id: String,
) -> Result<Option<ConversationDetail>, String> {
    let store = state.inner().clone();
    blocking(move || store.delete_conversation(&conversation_id)).await
}

#[tauri::command]
pub async fn conversation_start_new_turn(
    state: State<'_, ConversationStore>,
    persona_state: State<'_, PersonaPromptStore>,
    prompt: String,
) -> Result<StartedTurn, String> {
    let store = state.inner().clone();
    let persona = persona_state.inner().clone();
    blocking(move || {
        let compiled = persona.compiled()?;
        let snapshot = ConversationPromptSnapshot {
            persona_id: compiled.persona_id,
            persona_revision: compiled.revision,
            system_prompt: compiled.content,
        };
        store.start_new_turn_with_prompt(&prompt, Some(&snapshot))
    })
    .await
}

#[tauri::command]
pub async fn conversation_start_turn(
    state: State<'_, ConversationStore>,
    persona_state: State<'_, PersonaPromptStore>,
    conversation_id: String,
    prompt: String,
) -> Result<StartedTurn, String> {
    let store = state.inner().clone();
    let persona = persona_state.inner().clone();
    blocking(move || {
        let compiled = persona.compiled()?;
        let snapshot = ConversationPromptSnapshot {
            persona_id: compiled.persona_id,
            persona_revision: compiled.revision,
            system_prompt: compiled.content,
        };
        store.start_turn_with_prompt(&conversation_id, &prompt, Some(&snapshot))
    })
    .await
}

#[tauri::command]
pub async fn conversation_finish_turn(
    state: State<'_, ConversationStore>,
    request: FinishTurnRequest,
) -> Result<bool, String> {
    FinishTurnRequest::validate_status(request.status)?;
    let store = state.inner().clone();
    blocking(move || {
        store.finish_turn(
            &request.assistant_message_id,
            &request.content,
            request.status,
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::FinishTurnRequest;
    use crate::conversation_store::{ConversationStore, MessageStatus};

    #[test]
    fn bootstrap_dto_uses_camel_case_contract() {
        let store = ConversationStore::open_in_memory().unwrap();
        store.bootstrap().unwrap();
        let turn = store.start_new_turn("question").unwrap();
        let value = serde_json::to_value(store.bootstrap().unwrap()).unwrap();

        assert!(value["selected"]["createdAt"].is_number());
        assert_eq!(
            value["selected"]["messages"][0]["conversationId"],
            turn.conversation.id
        );
    }

    #[test]
    fn finish_request_rejects_streaming_status() {
        assert!(FinishTurnRequest::validate_status(MessageStatus::Streaming).is_err());
        assert!(FinishTurnRequest::validate_status(MessageStatus::Complete).is_ok());
    }
}
