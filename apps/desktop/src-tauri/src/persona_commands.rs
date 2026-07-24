use serde::Serialize;
use tauri::State;

use crate::conversation_store::{ConversationPromptSnapshot, ConversationStore};
use crate::persona_prompt::{PersonaPromptDraft, PersonaPromptStateDto, PersonaPromptStore};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PromptPreviewMessageDto {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPromptPreviewDto {
    pub persona_id: String,
    pub revision: String,
    pub source: String,
    pub messages: Vec<PromptPreviewMessageDto>,
    pub formatted_prompt: String,
    pub character_count: usize,
    pub estimated_tokens: usize,
}

async fn blocking<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| format!("persona command task failed: {error}"))?
}

#[tauri::command]
pub async fn persona_get_state(
    state: State<'_, PersonaPromptStore>,
) -> Result<PersonaPromptStateDto, String> {
    let store = state.inner().clone();
    blocking(move || store.state()).await
}

#[tauri::command]
pub async fn persona_preview(
    state: State<'_, PersonaPromptStore>,
    request: PersonaPromptDraft,
) -> Result<PersonaPromptStateDto, String> {
    let store = state.inner().clone();
    blocking(move || store.preview(request)).await
}

#[tauri::command]
pub async fn persona_save(
    state: State<'_, PersonaPromptStore>,
    request: PersonaPromptDraft,
) -> Result<PersonaPromptStateDto, String> {
    let store = state.inner().clone();
    blocking(move || store.save(request)).await
}

#[tauri::command]
pub async fn persona_reset_defaults(
    state: State<'_, PersonaPromptStore>,
) -> Result<PersonaPromptStateDto, String> {
    let store = state.inner().clone();
    blocking(move || store.reset_defaults()).await
}

#[tauri::command]
pub async fn persona_preview_conversation(
    state: State<'_, PersonaPromptStore>,
    conversation_state: State<'_, ConversationStore>,
    conversation_id: String,
) -> Result<ConversationPromptPreviewDto, String> {
    let persona = state.inner().clone();
    let conversations = conversation_state.inner().clone();
    blocking(move || {
        let context = conversations.model_prompt_context(&conversation_id)?;
        let (snapshot, source) = match context.snapshot {
            Some(snapshot) => (snapshot, "conversation-snapshot"),
            None => {
                let compiled = persona.compiled()?;
                (
                    ConversationPromptSnapshot {
                        persona_id: compiled.persona_id,
                        persona_revision: compiled.revision,
                        system_prompt: compiled.content,
                    },
                    "active-persona",
                )
            }
        };
        let mut messages = Vec::with_capacity(context.messages.len() + 1);
        if !snapshot.system_prompt.is_empty() {
            messages.push(PromptPreviewMessageDto {
                role: "system".into(),
                content: snapshot.system_prompt,
            });
        }
        messages.extend(
            context
                .messages
                .into_iter()
                .map(|message| PromptPreviewMessageDto {
                    role: message.role,
                    content: message.content,
                }),
        );
        let formatted_prompt = format_prompt_messages(&messages);
        Ok(ConversationPromptPreviewDto {
            persona_id: snapshot.persona_id,
            revision: snapshot.persona_revision,
            source: source.into(),
            character_count: formatted_prompt.chars().count(),
            estimated_tokens: if formatted_prompt.is_empty() {
                0
            } else {
                formatted_prompt.len().div_ceil(4)
            },
            formatted_prompt,
            messages,
        })
    })
    .await
}

fn format_prompt_messages(messages: &[PromptPreviewMessageDto]) -> String {
    messages
        .iter()
        .map(|message| format!("[{}]\n{}", message.role.to_uppercase(), message.content))
        .collect::<Vec<_>>()
        .join("\n\n")
}
