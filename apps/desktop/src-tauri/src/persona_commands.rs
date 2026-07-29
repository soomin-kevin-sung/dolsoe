use serde::Serialize;
use tauri::State;

use crate::agent_mode::{compile_agent_runtime_system_prompt, AgentMode};
use crate::conversation_store::{ConversationPromptSnapshot, ConversationStore};
use crate::llm_dto::SubmitChatMessage;
use crate::persona_prompt::{PersonaPromptDraft, PersonaPromptStateDto, PersonaPromptStore};
use crate::runtime_host::RuntimeHost;

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
    pub structured_prompt: String,
    pub final_prompt: Option<String>,
    pub final_prompt_error: Option<String>,
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
    runtime_state: State<'_, RuntimeHost>,
    conversation_id: String,
) -> Result<ConversationPromptPreviewDto, String> {
    let persona = state.inner().clone();
    let conversations = conversation_state.inner().clone();
    let runtime = runtime_state.inner().clone();
    blocking(move || {
        let context = conversations.model_prompt_context(&conversation_id)?;
        let agent_mode = AgentMode::parse(&context.agent_mode)?;
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
        let system_prompt = compile_agent_runtime_system_prompt(
            agent_mode,
            &snapshot.system_prompt,
            &context.workspace_path,
        );
        if !system_prompt.is_empty() {
            messages.push(PromptPreviewMessageDto {
                role: "system".into(),
                content: system_prompt,
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
        let structured_prompt = format_prompt_messages(&messages);
        let runtime_messages = messages
            .iter()
            .map(|message| SubmitChatMessage {
                role: message.role.clone(),
                content: message.content.clone(),
            })
            .collect();
        let (final_prompt, final_prompt_error) = match runtime.format_chat(runtime_messages) {
            Ok(prompt) => (Some(prompt), None),
            Err(error) => (None, Some(error)),
        };
        let measured_prompt = final_prompt.as_deref().unwrap_or(&structured_prompt);
        Ok(ConversationPromptPreviewDto {
            persona_id: snapshot.persona_id,
            revision: snapshot.persona_revision,
            source: source.into(),
            character_count: measured_prompt.chars().count(),
            estimated_tokens: if measured_prompt.is_empty() {
                0
            } else {
                measured_prompt.len().div_ceil(4)
            },
            structured_prompt,
            final_prompt,
            final_prompt_error,
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
