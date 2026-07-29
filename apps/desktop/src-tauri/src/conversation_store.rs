use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent_mode::AgentMode;

pub type StoreResult<T> = Result<T, String>;

const MIGRATION_1: &str = r#"
CREATE TABLE conversations (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('complete', 'streaming', 'cancelled', 'interrupted', 'error')
    ),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);
CREATE INDEX messages_conversation_created
ON messages(conversation_id, created_at, id);
"#;

const MIGRATION_2: &str = r#"
ALTER TABLE conversations ADD COLUMN persona_id TEXT;
ALTER TABLE conversations ADD COLUMN persona_revision TEXT;
ALTER TABLE conversations ADD COLUMN system_prompt TEXT;
"#;

const MIGRATION_3: &str = r#"
CREATE TABLE agent_runs (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    user_message_id TEXT NOT NULL,
    assistant_message_id TEXT NOT NULL,
    mode TEXT NOT NULL,
    protocol_revision TEXT NOT NULL,
    policy_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('prepared', 'running', 'complete', 'cancelled', 'interrupted', 'error')
    ),
    progress_step_count INTEGER NOT NULL DEFAULT 0,
    total_step_count INTEGER NOT NULL DEFAULT 0,
    started_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    finished_at INTEGER,
    terminal_reason TEXT,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE,
    FOREIGN KEY (user_message_id) REFERENCES messages(id) ON DELETE CASCADE,
    FOREIGN KEY (assistant_message_id) REFERENCES messages(id) ON DELETE CASCADE
);
CREATE INDEX agent_runs_conversation_started
ON agent_runs(conversation_id, started_at, id);

CREATE TABLE agent_steps (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    step_index INTEGER NOT NULL,
    kind TEXT NOT NULL,
    stage TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('prepared', 'running', 'complete', 'cancelled', 'interrupted', 'error')
    ),
    correlation_id INTEGER NOT NULL UNIQUE,
    request_handle TEXT,
    output_content TEXT NOT NULL DEFAULT '',
    started_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    finished_at INTEGER,
    terminal_reason TEXT,
    FOREIGN KEY (run_id) REFERENCES agent_runs(id) ON DELETE CASCADE,
    UNIQUE (run_id, step_index)
);
CREATE INDEX agent_steps_run_index
ON agent_steps(run_id, step_index);
"#;

const MIGRATION_4: &str = r#"
ALTER TABLE conversations ADD COLUMN agent_mode TEXT NOT NULL DEFAULT 'chat';

ALTER TABLE messages ADD COLUMN kind TEXT NOT NULL DEFAULT 'chat';
ALTER TABLE messages ADD COLUMN source TEXT NOT NULL DEFAULT 'model';
ALTER TABLE messages ADD COLUMN metadata_json TEXT;
UPDATE messages SET source = 'user' WHERE role = 'user';

CREATE TABLE agent_preferences (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    default_mode TEXT NOT NULL
);
INSERT INTO agent_preferences(singleton, default_mode) VALUES (1, 'chat');

ALTER TABLE agent_runs ADD COLUMN total_tool_calls INTEGER NOT NULL DEFAULT 0;
ALTER TABLE agent_steps ADD COLUMN decision_json TEXT;
"#;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MessageRole {
    User,
    Assistant,
}

impl MessageRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }

    fn parse(value: &str) -> StoreResult<Self> {
        match value {
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            _ => Err(format!("invalid stored message role: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MessageStatus {
    Complete,
    Streaming,
    Cancelled,
    Interrupted,
    Error,
}

impl MessageStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Streaming => "streaming",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::Error => "error",
        }
    }

    fn parse(value: &str) -> StoreResult<Self> {
        match value {
            "complete" => Ok(Self::Complete),
            "streaming" => Ok(Self::Streaming),
            "cancelled" => Ok(Self::Cancelled),
            "interrupted" => Ok(Self::Interrupted),
            "error" => Ok(Self::Error),
            _ => Err(format!("invalid stored message status: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub agent_mode: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredMessage {
    pub id: String,
    pub conversation_id: String,
    pub role: MessageRole,
    pub content: String,
    pub status: MessageStatus,
    pub kind: String,
    pub source: String,
    pub metadata_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolTrace {
    pub activity_id: String,
    pub tool_name: String,
    pub status: String,
    pub input: String,
    pub output: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunTrace {
    pub run_id: String,
    pub assistant_message_id: String,
    pub mode: String,
    pub status: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub tools: Vec<AgentToolTrace>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDetail {
    pub id: String,
    pub title: String,
    pub agent_mode: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub messages: Vec<StoredMessage>,
    pub agent_runs: Vec<AgentRunTrace>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationBootstrap {
    pub conversations: Vec<ConversationSummary>,
    pub selected: Option<ConversationDetail>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartedTurn {
    pub conversation: ConversationSummary,
    pub user: StoredMessage,
    pub assistant: StoredMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_step_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationPromptSnapshot {
    pub persona_id: String,
    pub persona_revision: String,
    pub system_prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPromptMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationPromptContext {
    pub agent_mode: String,
    pub snapshot: Option<ConversationPromptSnapshot>,
    pub messages: Vec<ModelPromptMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSubmission {
    pub run_id: String,
    pub step_id: String,
    pub correlation_id: u64,
    pub mode: String,
    pub conversation_id: String,
    pub assistant_message_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentPreferences {
    pub default_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedAgentStep {
    pub run_id: String,
    pub step_id: String,
    pub correlation_id: u64,
}

#[derive(Clone)]
pub struct ConversationStore {
    connection: Arc<Mutex<Connection>>,
}

impl ConversationStore {
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        let connection = Connection::open(path).map_err(store_error)?;
        Self::from_connection(connection)
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory() -> StoreResult<Self> {
        let connection = Connection::open_in_memory().map_err(store_error)?;
        Self::from_connection(connection)
    }

    fn from_connection(connection: Connection) -> StoreResult<Self> {
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(store_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(store_error)?;
        let _ = connection.pragma_update(None, "journal_mode", "WAL");
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn bootstrap(&self) -> StoreResult<ConversationBootstrap> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        apply_migrations(&transaction)?;
        let timestamp = now_millis()?;
        transaction
            .execute(
                "UPDATE messages SET status = 'interrupted', updated_at = ?1 WHERE status = 'streaming'",
                [timestamp],
            )
            .map_err(store_error)?;
        transaction
            .execute(
                "UPDATE agent_steps
                 SET status = 'interrupted', updated_at = ?1, finished_at = ?1,
                     terminal_reason = COALESCE(terminal_reason, 'application-restarted')
                 WHERE status IN ('prepared', 'running')",
                [timestamp],
            )
            .map_err(store_error)?;
        transaction
            .execute(
                "UPDATE agent_runs
                 SET status = 'interrupted', updated_at = ?1, finished_at = ?1,
                     terminal_reason = COALESCE(terminal_reason, 'application-restarted')
                 WHERE status IN ('prepared', 'running')",
                [timestamp],
            )
            .map_err(store_error)?;

        let conversations = list_conversations(&transaction)?;
        let selected = conversations
            .first()
            .map(|conversation| load_conversation(&transaction, &conversation.id))
            .transpose()?;
        transaction.commit().map_err(store_error)?;
        Ok(ConversationBootstrap {
            conversations,
            selected,
        })
    }

    pub fn load_conversation(&self, id: &str) -> StoreResult<ConversationDetail> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let detail = load_conversation(&transaction, id)?;
        transaction.commit().map_err(store_error)?;
        Ok(detail)
    }

    pub fn agent_preferences(&self) -> StoreResult<AgentPreferences> {
        let connection = self.lock()?;
        let default_mode = connection
            .query_row(
                "SELECT default_mode FROM agent_preferences WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .map_err(store_error)?;
        AgentMode::parse(&default_mode)?;
        Ok(AgentPreferences { default_mode })
    }

    pub fn set_default_agent_mode(&self, mode: &str) -> StoreResult<AgentPreferences> {
        let mode = AgentMode::parse(mode)?;
        let connection = self.lock()?;
        connection
            .execute(
                "UPDATE agent_preferences SET default_mode = ?1 WHERE singleton = 1",
                [mode.as_str()],
            )
            .map_err(store_error)?;
        Ok(AgentPreferences {
            default_mode: mode.as_str().into(),
        })
    }

    pub fn set_conversation_agent_mode(
        &self,
        conversation_id: &str,
        mode: &str,
    ) -> StoreResult<ConversationDetail> {
        let mode = AgentMode::parse(mode)?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let current = conversation_summary(&transaction, conversation_id)?;
        if current.agent_mode == mode.as_str() {
            let detail = load_conversation(&transaction, conversation_id)?;
            transaction.commit().map_err(store_error)?;
            return Ok(detail);
        }
        let active_runs: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM agent_runs
                 WHERE conversation_id = ?1 AND status IN ('prepared', 'running')",
                [conversation_id],
                |row| row.get(0),
            )
            .map_err(store_error)?;
        if active_runs != 0 {
            return Err("agent mode cannot change while a run is active".into());
        }
        let previous = AgentMode::parse(&current.agent_mode)?;
        let timestamp = now_millis()?;
        transaction
            .execute(
                "UPDATE conversations SET agent_mode = ?1, updated_at = ?2 WHERE id = ?3",
                params![mode.as_str(), timestamp, conversation_id],
            )
            .map_err(store_error)?;
        let content = format!(
            "좋습니다. 이제부터 이 대화는 {} 모드로 이어갑니다.",
            mode.label()
        );
        let metadata = serde_json::json!({
            "previousMode": previous.as_str(),
            "mode": mode.as_str(),
        })
        .to_string();
        insert_message_with_provenance(
            &transaction,
            conversation_id,
            MessageRole::Assistant,
            &content,
            MessageStatus::Complete,
            "agent-mode-change",
            "application",
            Some(&metadata),
            timestamp,
        )?;
        let detail = load_conversation(&transaction, conversation_id)?;
        transaction.commit().map_err(store_error)?;
        Ok(detail)
    }

    pub fn rename_conversation(&self, id: &str, title: &str) -> StoreResult<ConversationSummary> {
        let title = normalize_title(title, 80)?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        conversation_summary(&transaction, id)?;
        let timestamp = now_millis()?;
        transaction
            .execute(
                "UPDATE conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
                params![title, timestamp, id],
            )
            .map_err(store_error)?;
        let summary = conversation_summary(&transaction, id)?;
        transaction.commit().map_err(store_error)?;
        Ok(summary)
    }

    pub fn clear_conversation(&self, id: &str) -> StoreResult<ConversationDetail> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        conversation_summary(&transaction, id)?;
        let timestamp = now_millis()?;
        transaction
            .execute("DELETE FROM messages WHERE conversation_id = ?1", [id])
            .map_err(store_error)?;
        transaction
            .execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                params![timestamp, id],
            )
            .map_err(store_error)?;
        let detail = load_conversation(&transaction, id)?;
        transaction.commit().map_err(store_error)?;
        Ok(detail)
    }

    pub fn delete_conversation(&self, id: &str) -> StoreResult<Option<ConversationDetail>> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        conversation_summary(&transaction, id)?;
        transaction
            .execute("DELETE FROM conversations WHERE id = ?1", [id])
            .map_err(store_error)?;
        let conversations = list_conversations(&transaction)?;
        let fallback = conversations
            .first()
            .map(|conversation| load_conversation(&transaction, &conversation.id))
            .transpose()?;
        transaction.commit().map_err(store_error)?;
        Ok(fallback)
    }

    #[cfg(test)]
    pub fn start_turn(&self, conversation_id: &str, prompt: &str) -> StoreResult<StartedTurn> {
        self.start_turn_with_prompt(conversation_id, prompt, None)
    }

    #[cfg(test)]
    pub fn start_turn_with_prompt(
        &self,
        conversation_id: &str,
        prompt: &str,
        prompt_snapshot: Option<&ConversationPromptSnapshot>,
    ) -> StoreResult<StartedTurn> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("prompt must not be empty".into());
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let conversation = conversation_summary(&transaction, conversation_id)?;
        if let Some(snapshot) = prompt_snapshot {
            bind_prompt_snapshot(&transaction, conversation_id, snapshot)?;
        }
        let timestamp = now_millis()?;
        let turn = insert_turn(&transaction, conversation, prompt, timestamp)?;
        transaction.commit().map_err(store_error)?;
        Ok(turn)
    }

    pub fn start_agent_turn_with_prompt(
        &self,
        conversation_id: &str,
        prompt: &str,
        prompt_snapshot: Option<&ConversationPromptSnapshot>,
    ) -> StoreResult<StartedTurn> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("prompt must not be empty".into());
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let conversation = conversation_summary(&transaction, conversation_id)?;
        if let Some(snapshot) = prompt_snapshot {
            bind_prompt_snapshot(&transaction, conversation_id, snapshot)?;
        }
        let timestamp = now_millis()?;
        let mut turn = insert_turn(&transaction, conversation, prompt, timestamp)?;
        let mode = AgentMode::parse(&turn.conversation.agent_mode)?;
        attach_agent_run(&transaction, &mut turn, mode, timestamp)?;
        transaction.commit().map_err(store_error)?;
        Ok(turn)
    }

    #[cfg(test)]
    pub fn start_new_turn(&self, prompt: &str) -> StoreResult<StartedTurn> {
        self.start_new_turn_with_prompt(prompt, None)
    }

    #[cfg(test)]
    pub fn start_new_turn_with_prompt(
        &self,
        prompt: &str,
        prompt_snapshot: Option<&ConversationPromptSnapshot>,
    ) -> StoreResult<StartedTurn> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("prompt must not be empty".into());
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let timestamp = now_millis()?;
        let conversation = insert_empty_conversation(&transaction, AgentMode::Chat, timestamp)?;
        if let Some(snapshot) = prompt_snapshot {
            bind_prompt_snapshot(&transaction, &conversation.id, snapshot)?;
        }
        let turn = insert_turn(&transaction, conversation, prompt, timestamp)?;
        transaction.commit().map_err(store_error)?;
        Ok(turn)
    }

    #[cfg(test)]
    pub fn start_new_agent_turn_with_prompt(
        &self,
        prompt: &str,
        prompt_snapshot: Option<&ConversationPromptSnapshot>,
    ) -> StoreResult<StartedTurn> {
        self.start_new_agent_turn_with_mode(prompt, AgentMode::Chat.as_str(), prompt_snapshot)
    }

    pub fn start_new_agent_turn_with_mode(
        &self,
        prompt: &str,
        mode: &str,
        prompt_snapshot: Option<&ConversationPromptSnapshot>,
    ) -> StoreResult<StartedTurn> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("prompt must not be empty".into());
        }
        let mode = AgentMode::parse(mode)?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let timestamp = now_millis()?;
        let conversation = insert_empty_conversation(&transaction, mode, timestamp)?;
        if let Some(snapshot) = prompt_snapshot {
            bind_prompt_snapshot(&transaction, &conversation.id, snapshot)?;
        }
        let mut turn = insert_turn(&transaction, conversation, prompt, timestamp)?;
        attach_agent_run(&transaction, &mut turn, mode, timestamp)?;
        transaction.commit().map_err(store_error)?;
        Ok(turn)
    }

    pub fn agent_submission(
        &self,
        run_id: &str,
        step_id: &str,
        conversation_id: &str,
    ) -> StoreResult<AgentSubmission> {
        let correlation = self
            .lock()?
            .query_row(
                "SELECT r.id, s.id, s.correlation_id, r.mode,
                        r.conversation_id, r.assistant_message_id
                 FROM agent_runs r
                 JOIN agent_steps s ON s.run_id = r.id
                 WHERE r.id = ?1 AND s.id = ?2 AND r.conversation_id = ?3
                   AND r.status = 'prepared' AND s.status = 'prepared'",
                params![run_id, step_id, conversation_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(store_error)?
            .ok_or_else(|| "prepared agent run was not found".to_string())?;
        Ok(AgentSubmission {
            run_id: correlation.0,
            step_id: correlation.1,
            correlation_id: u64::try_from(correlation.2)
                .map_err(|_| "stored agent correlation id is invalid".to_string())?,
            mode: correlation.3,
            conversation_id: correlation.4,
            assistant_message_id: correlation.5,
        })
    }

    pub fn bind_agent_request(
        &self,
        run_id: &str,
        step_id: &str,
        request_handle: &str,
    ) -> StoreResult<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let timestamp = now_millis()?;
        let updated = transaction
            .execute(
                "UPDATE agent_steps
                 SET status = 'running', request_handle = ?1, updated_at = ?2
                 WHERE id = ?3 AND run_id = ?4 AND status = 'prepared'",
                params![request_handle, timestamp, step_id, run_id],
            )
            .map_err(store_error)?;
        if updated == 0 {
            let status = transaction
                .query_row(
                    "SELECT status FROM agent_steps WHERE id = ?1 AND run_id = ?2",
                    params![step_id, run_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(store_error)?;
            if status
                .as_deref()
                .is_some_and(|value| matches!(value, "complete" | "cancelled" | "error"))
            {
                transaction.commit().map_err(store_error)?;
                return Ok(());
            }
            return Err("prepared agent step was not found".into());
        }
        transaction
            .execute(
                "UPDATE agent_runs
                 SET status = 'running',
                     total_step_count = total_step_count + 1,
                     progress_step_count = progress_step_count + 1,
                     updated_at = ?1
                 WHERE id = ?2 AND status IN ('prepared', 'running')",
                params![timestamp, run_id],
            )
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(())
    }

    pub fn finish_agent_step(
        &self,
        correlation_id: u64,
        content: &str,
        status: MessageStatus,
        terminal_reason: Option<&str>,
    ) -> StoreResult<bool> {
        if !matches!(
            status,
            MessageStatus::Complete
                | MessageStatus::Cancelled
                | MessageStatus::Interrupted
                | MessageStatus::Error
        ) {
            return Err("agent step requires a terminal status".into());
        }
        let correlation_id = i64::try_from(correlation_id)
            .map_err(|_| "agent correlation id exceeds SQLite integer range".to_string())?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let current = transaction
            .query_row(
                "SELECT r.id, r.conversation_id, r.assistant_message_id, s.id, s.status
                 FROM agent_steps s
                 JOIN agent_runs r ON r.id = s.run_id
                 WHERE s.correlation_id = ?1",
                [correlation_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(store_error)?
            .ok_or_else(|| "agent step correlation was not found".to_string())?;
        if !matches!(current.4.as_str(), "prepared" | "running") {
            transaction.commit().map_err(store_error)?;
            return Ok(false);
        }
        let timestamp = now_millis()?;
        transaction
            .execute(
                "UPDATE messages
                 SET content = ?1, status = ?2, updated_at = ?3
                 WHERE id = ?4 AND role = 'assistant' AND status = 'streaming'",
                params![content, status.as_str(), timestamp, current.2],
            )
            .map_err(store_error)?;
        transaction
            .execute(
                "UPDATE agent_steps
                 SET output_content = ?1, status = ?2, updated_at = ?3,
                     finished_at = ?3, terminal_reason = ?4
                 WHERE id = ?5",
                params![
                    content,
                    status.as_str(),
                    timestamp,
                    terminal_reason,
                    current.3
                ],
            )
            .map_err(store_error)?;
        transaction
            .execute(
                "UPDATE agent_runs
                 SET status = ?1, updated_at = ?2, finished_at = ?2,
                     terminal_reason = ?3
                 WHERE id = ?4",
                params![status.as_str(), timestamp, terminal_reason, current.0],
            )
            .map_err(store_error)?;
        transaction
            .execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                params![timestamp, current.1],
            )
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(true)
    }

    pub fn finish_agent_final_decision(
        &self,
        correlation_id: u64,
        model_output: &str,
        decision_json: &str,
        final_content: &str,
    ) -> StoreResult<bool> {
        let correlation_id = i64::try_from(correlation_id)
            .map_err(|_| "agent correlation id exceeds SQLite integer range".to_string())?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let current = transaction
            .query_row(
                "SELECT r.id, r.conversation_id, r.assistant_message_id, r.status, s.id, s.status
                 FROM agent_steps s
                 JOIN agent_runs r ON r.id = s.run_id
                 WHERE s.correlation_id = ?1",
                [correlation_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(store_error)?
            .ok_or_else(|| "agent step correlation was not found".to_string())?;
        if !matches!(current.3.as_str(), "prepared" | "running")
            || !matches!(current.5.as_str(), "prepared" | "running")
        {
            transaction.commit().map_err(store_error)?;
            return Ok(false);
        }
        let timestamp = now_millis()?;
        transaction
            .execute(
                "UPDATE messages
                 SET content = ?1, status = 'complete', updated_at = ?2
                 WHERE id = ?3 AND role = 'assistant' AND status = 'streaming'",
                params![final_content, timestamp, current.2],
            )
            .map_err(store_error)?;
        transaction
            .execute(
                "UPDATE agent_steps
                 SET output_content = ?1, decision_json = ?2, status = 'complete',
                     updated_at = ?3, finished_at = ?3, terminal_reason = 'final-answer'
                 WHERE id = ?4",
                params![model_output, decision_json, timestamp, current.4],
            )
            .map_err(store_error)?;
        transaction
            .execute(
                "UPDATE agent_runs
                 SET status = 'complete', updated_at = ?1, finished_at = ?1,
                     terminal_reason = 'strategy-complete'
                 WHERE id = ?2",
                params![timestamp, current.0],
            )
            .map_err(store_error)?;
        transaction
            .execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                params![timestamp, current.1],
            )
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(true)
    }

    pub fn complete_agent_model_step(
        &self,
        correlation_id: u64,
        output: &str,
        decision_json: &str,
        terminal_reason: &str,
    ) -> StoreResult<String> {
        let correlation_id = i64::try_from(correlation_id)
            .map_err(|_| "agent correlation id exceeds SQLite integer range".to_string())?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let current = transaction
            .query_row(
                "SELECT r.id, s.id, s.status
                 FROM agent_steps s
                 JOIN agent_runs r ON r.id = s.run_id
                 WHERE s.correlation_id = ?1 AND r.status = 'running'",
                [correlation_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(store_error)?
            .ok_or_else(|| "active agent model step was not found".to_string())?;
        if !matches!(current.2.as_str(), "prepared" | "running") {
            return Err("agent model step is already terminal".into());
        }
        let timestamp = now_millis()?;
        transaction
            .execute(
                "UPDATE agent_steps
                 SET output_content = ?1, decision_json = ?2, status = 'complete',
                     updated_at = ?3, finished_at = ?3, terminal_reason = ?4
                 WHERE id = ?5",
                params![output, decision_json, timestamp, terminal_reason, current.1],
            )
            .map_err(store_error)?;
        transaction
            .execute(
                "UPDATE agent_runs SET updated_at = ?1 WHERE id = ?2",
                params![timestamp, current.0],
            )
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(current.0)
    }

    pub fn prepare_agent_model_step(
        &self,
        run_id: &str,
        stage: &str,
    ) -> StoreResult<PreparedAgentStep> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let run_exists = transaction
            .query_row(
                "SELECT 1 FROM agent_runs WHERE id = ?1 AND status = 'running'",
                [run_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(store_error)?
            .is_some();
        if !run_exists {
            return Err("running agent run was not found".into());
        }
        let step_index: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(step_index), -1) + 1 FROM agent_steps WHERE run_id = ?1",
                [run_id],
                |row| row.get(0),
            )
            .map_err(store_error)?;
        let step_id = Uuid::new_v4().to_string();
        let correlation_id = correlation_id_for(&step_id)?;
        let timestamp = now_millis()?;
        transaction
            .execute(
                "INSERT INTO agent_steps(
                    id, run_id, step_index, kind, stage, status, correlation_id,
                    output_content, started_at, updated_at
                 ) VALUES (?1, ?2, ?3, 'model', ?4, 'prepared', ?5, '', ?6, ?6)",
                params![
                    step_id,
                    run_id,
                    step_index,
                    stage,
                    correlation_id,
                    timestamp
                ],
            )
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(PreparedAgentStep {
            run_id: run_id.into(),
            step_id,
            correlation_id: u64::try_from(correlation_id)
                .map_err(|_| "stored agent correlation id is invalid".to_string())?,
        })
    }

    pub fn record_agent_tool_step(
        &self,
        run_id: &str,
        tool_name: &str,
        arguments_json: &str,
        output: &str,
        successful: bool,
        reset_progress: bool,
        duration_ms: u64,
    ) -> StoreResult<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let run_exists = transaction
            .query_row(
                "SELECT 1 FROM agent_runs WHERE id = ?1 AND status = 'running'",
                [run_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(store_error)?
            .is_some();
        if !run_exists {
            return Err("running agent run was not found".into());
        }
        let step_index: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(step_index), -1) + 1 FROM agent_steps WHERE run_id = ?1",
                [run_id],
                |row| row.get(0),
            )
            .map_err(store_error)?;
        let step_id = Uuid::new_v4().to_string();
        let correlation_id = correlation_id_for(&step_id)?;
        let timestamp = now_millis()?;
        let duration_ms = i64::try_from(duration_ms).unwrap_or(i64::MAX);
        let started_at = timestamp.saturating_sub(duration_ms);
        transaction
            .execute(
                "INSERT INTO agent_steps(
                    id, run_id, step_index, kind, stage, status, correlation_id,
                    output_content, started_at, updated_at, finished_at,
                    terminal_reason, decision_json
                 ) VALUES (?1, ?2, ?3, 'tool', ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10, ?11)",
                params![
                    step_id,
                    run_id,
                    step_index,
                    format!("tool:{tool_name}"),
                    if successful { "complete" } else { "error" },
                    correlation_id,
                    output,
                    started_at,
                    timestamp,
                    if successful {
                        "tool-complete"
                    } else {
                        "tool-error"
                    },
                    arguments_json,
                ],
            )
            .map_err(store_error)?;
        transaction
            .execute(
                "UPDATE agent_runs
                 SET total_tool_calls = total_tool_calls + 1,
                     progress_step_count = CASE WHEN ?1 THEN 0 ELSE progress_step_count END,
                     updated_at = ?2
                 WHERE id = ?3",
                params![reset_progress, timestamp, run_id],
            )
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(())
    }

    pub fn finish_agent_run(
        &self,
        run_id: &str,
        content: &str,
        status: MessageStatus,
        terminal_reason: &str,
    ) -> StoreResult<()> {
        if !matches!(
            status,
            MessageStatus::Complete
                | MessageStatus::Cancelled
                | MessageStatus::Interrupted
                | MessageStatus::Error
        ) {
            return Err("agent run requires a terminal status".into());
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let current = transaction
            .query_row(
                "SELECT conversation_id, assistant_message_id, status
                 FROM agent_runs WHERE id = ?1",
                [run_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(store_error)?
            .ok_or_else(|| "agent run was not found".to_string())?;
        if !matches!(current.2.as_str(), "prepared" | "running") {
            transaction.commit().map_err(store_error)?;
            return Ok(());
        }
        let timestamp = now_millis()?;
        transaction
            .execute(
                "UPDATE messages SET content = ?1, status = ?2, updated_at = ?3
                 WHERE id = ?4 AND role = 'assistant' AND status = 'streaming'",
                params![content, status.as_str(), timestamp, current.1],
            )
            .map_err(store_error)?;
        transaction
            .execute(
                "UPDATE agent_steps
                 SET status = CASE WHEN status IN ('prepared', 'running') THEN ?1 ELSE status END,
                     updated_at = ?2,
                     finished_at = CASE WHEN status IN ('prepared', 'running') THEN ?2 ELSE finished_at END,
                     terminal_reason = CASE WHEN status IN ('prepared', 'running') THEN ?3 ELSE terminal_reason END
                 WHERE run_id = ?4",
                params![status.as_str(), timestamp, terminal_reason, run_id],
            )
            .map_err(store_error)?;
        transaction
            .execute(
                "UPDATE agent_runs
                 SET status = ?1, updated_at = ?2, finished_at = ?2, terminal_reason = ?3
                 WHERE id = ?4",
                params![status.as_str(), timestamp, terminal_reason, run_id],
            )
            .map_err(store_error)?;
        transaction
            .execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                params![timestamp, current.0],
            )
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(())
    }

    #[cfg(test)]
    pub fn prompt_snapshot(
        &self,
        conversation_id: &str,
    ) -> StoreResult<Option<ConversationPromptSnapshot>> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        conversation_summary(&transaction, conversation_id)?;
        let snapshot = prompt_snapshot(&transaction, conversation_id)?;
        transaction.commit().map_err(store_error)?;
        Ok(snapshot)
    }

    pub fn model_prompt_context(
        &self,
        conversation_id: &str,
    ) -> StoreResult<ConversationPromptContext> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let detail = load_conversation(&transaction, conversation_id)?;
        let snapshot = prompt_snapshot(&transaction, conversation_id)?;
        let messages = model_prompt_messages(&detail.messages);
        let agent_mode = detail.agent_mode;
        transaction.commit().map_err(store_error)?;
        Ok(ConversationPromptContext {
            agent_mode,
            snapshot,
            messages,
        })
    }

    pub fn finish_turn(
        &self,
        assistant_id: &str,
        content: &str,
        status: MessageStatus,
    ) -> StoreResult<bool> {
        if !matches!(
            status,
            MessageStatus::Complete
                | MessageStatus::Cancelled
                | MessageStatus::Interrupted
                | MessageStatus::Error
        ) {
            return Err("assistant message must be finalized with a terminal status".into());
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let current = transaction
            .query_row(
                "SELECT conversation_id, role, status FROM messages WHERE id = ?1",
                [assistant_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(store_error)?
            .ok_or_else(|| "assistant message was not found".to_string())?;
        if current.1 != MessageRole::Assistant.as_str() {
            return Err("only assistant messages can be finalized".into());
        }
        if current.2 != MessageStatus::Streaming.as_str() {
            transaction.commit().map_err(store_error)?;
            return Ok(false);
        }
        let timestamp = now_millis()?;
        let updated = transaction
            .execute(
                "UPDATE messages SET content = ?1, status = ?2, updated_at = ?3 WHERE id = ?4 AND role = 'assistant' AND status = 'streaming'",
                params![content, status.as_str(), timestamp, assistant_id],
            )
            .map_err(store_error)?;
        if updated == 1 {
            transaction
                .execute(
                    "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                    params![timestamp, current.0],
                )
                .map_err(store_error)?;
        }
        transaction.commit().map_err(store_error)?;
        Ok(updated == 1)
    }

    #[cfg(test)]
    fn migration_count(&self) -> StoreResult<i64> {
        self.lock()?
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .map_err(store_error)
    }

    fn lock(&self) -> StoreResult<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| "conversation database lock is poisoned".into())
    }
}

fn apply_migrations(transaction: &Transaction<'_>) -> StoreResult<()> {
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);",
        )
        .map_err(store_error)?;
    let applied = transaction
        .query_row(
            "SELECT 1 FROM schema_migrations WHERE version = 1",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(store_error)?
        .is_some();
    if !applied {
        transaction
            .execute_batch(MIGRATION_1)
            .map_err(store_error)?;
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (1, ?1)",
                [now_millis()?],
            )
            .map_err(store_error)?;
    }
    let applied = transaction
        .query_row(
            "SELECT 1 FROM schema_migrations WHERE version = 2",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(store_error)?
        .is_some();
    if !applied {
        transaction
            .execute_batch(MIGRATION_2)
            .map_err(store_error)?;
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (2, ?1)",
                [now_millis()?],
            )
            .map_err(store_error)?;
    }
    let applied = transaction
        .query_row(
            "SELECT 1 FROM schema_migrations WHERE version = 3",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(store_error)?
        .is_some();
    if !applied {
        transaction
            .execute_batch(MIGRATION_3)
            .map_err(store_error)?;
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (3, ?1)",
                [now_millis()?],
            )
            .map_err(store_error)?;
    }
    let applied = transaction
        .query_row(
            "SELECT 1 FROM schema_migrations WHERE version = 4",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(store_error)?
        .is_some();
    if !applied {
        transaction
            .execute_batch(MIGRATION_4)
            .map_err(store_error)?;
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (4, ?1)",
                [now_millis()?],
            )
            .map_err(store_error)?;
    }
    Ok(())
}

fn bind_prompt_snapshot(
    transaction: &Transaction<'_>,
    conversation_id: &str,
    snapshot: &ConversationPromptSnapshot,
) -> StoreResult<()> {
    transaction
        .execute(
            "UPDATE conversations
             SET persona_id = ?1, persona_revision = ?2, system_prompt = ?3
             WHERE id = ?4 AND system_prompt IS NULL",
            params![
                snapshot.persona_id,
                snapshot.persona_revision,
                snapshot.system_prompt,
                conversation_id
            ],
        )
        .map_err(store_error)?;
    Ok(())
}

fn prompt_snapshot(
    transaction: &Transaction<'_>,
    conversation_id: &str,
) -> StoreResult<Option<ConversationPromptSnapshot>> {
    let values = transaction
        .query_row(
            "SELECT persona_id, persona_revision, system_prompt
             FROM conversations WHERE id = ?1",
            [conversation_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .map_err(store_error)?;
    Ok(values.2.map(|system_prompt| ConversationPromptSnapshot {
        persona_id: values.0.unwrap_or_default(),
        persona_revision: values.1.unwrap_or_default(),
        system_prompt,
    }))
}

fn model_prompt_messages(messages: &[StoredMessage]) -> Vec<ModelPromptMessage> {
    let messages = messages
        .iter()
        .filter(|message| message.kind == "chat")
        .collect::<Vec<_>>();
    let mut prompt_messages = Vec::new();
    let mut index = 0;
    while index + 1 < messages.len() {
        let user = &messages[index];
        let assistant = &messages[index + 1];
        if user.role == MessageRole::User
            && user.status == MessageStatus::Complete
            && assistant.role == MessageRole::Assistant
        {
            if assistant.status == MessageStatus::Complete {
                prompt_messages.push(ModelPromptMessage {
                    role: "user".into(),
                    content: user.content.clone(),
                });
                prompt_messages.push(ModelPromptMessage {
                    role: "assistant".into(),
                    content: assistant.content.clone(),
                });
                index += 2;
                continue;
            }
            if assistant.status == MessageStatus::Streaming && index + 2 == messages.len() {
                prompt_messages.push(ModelPromptMessage {
                    role: "user".into(),
                    content: user.content.clone(),
                });
                break;
            }
        }
        index += 1;
    }
    prompt_messages
}

fn insert_empty_conversation(
    transaction: &Transaction<'_>,
    mode: AgentMode,
    timestamp: i64,
) -> StoreResult<ConversationSummary> {
    let conversation = ConversationSummary {
        id: Uuid::new_v4().to_string(),
        title: "새 대화".into(),
        agent_mode: mode.as_str().into(),
        created_at: timestamp,
        updated_at: timestamp,
    };
    transaction
        .execute(
            "INSERT INTO conversations(id, title, agent_mode, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                conversation.id,
                conversation.title,
                conversation.agent_mode,
                conversation.created_at,
                conversation.updated_at
            ],
        )
        .map_err(store_error)?;
    Ok(conversation)
}

fn insert_turn(
    transaction: &Transaction<'_>,
    mut conversation: ConversationSummary,
    prompt: &str,
    timestamp: i64,
) -> StoreResult<StartedTurn> {
    let user = insert_message(
        transaction,
        &conversation.id,
        MessageRole::User,
        prompt,
        MessageStatus::Complete,
        timestamp,
    )?;
    let assistant = insert_message(
        transaction,
        &conversation.id,
        MessageRole::Assistant,
        "",
        MessageStatus::Streaming,
        timestamp,
    )?;
    let user_count: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1 AND role = 'user'",
            [&conversation.id],
            |row| row.get(0),
        )
        .map_err(store_error)?;
    if user_count == 1 {
        conversation.title = automatic_title(prompt);
    }
    conversation.updated_at = timestamp;
    transaction
        .execute(
            "UPDATE conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![conversation.title, timestamp, conversation.id],
        )
        .map_err(store_error)?;
    Ok(StartedTurn {
        conversation,
        user,
        assistant,
        agent_run_id: None,
        agent_step_id: None,
    })
}

fn attach_agent_run(
    transaction: &Transaction<'_>,
    turn: &mut StartedTurn,
    mode: AgentMode,
    timestamp: i64,
) -> StoreResult<()> {
    let run_id = Uuid::new_v4().to_string();
    let step_id = Uuid::new_v4().to_string();
    let correlation_id = correlation_id_for(&step_id)?;
    transaction
        .execute(
            "INSERT INTO agent_runs(
                id, conversation_id, user_message_id, assistant_message_id,
                mode, protocol_revision, policy_json, status,
                progress_step_count, total_step_count, started_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'prepared',
                       0, 0, ?8, ?8)",
            params![
                run_id,
                turn.conversation.id,
                turn.user.id,
                turn.assistant.id,
                mode.as_str(),
                mode.protocol_revision(),
                mode.policy_json(),
                timestamp
            ],
        )
        .map_err(store_error)?;
    transaction
        .execute(
            "INSERT INTO agent_steps(
                id, run_id, step_index, kind, stage, status, correlation_id,
                output_content, started_at, updated_at
             ) VALUES (?1, ?2, 0, 'model', ?3, 'prepared', ?4, '', ?5, ?5)",
            params![
                step_id,
                run_id,
                mode.initial_stage(),
                correlation_id,
                timestamp
            ],
        )
        .map_err(store_error)?;
    turn.agent_run_id = Some(run_id);
    turn.agent_step_id = Some(step_id);
    Ok(())
}

fn correlation_id_for(step_id: &str) -> StoreResult<i64> {
    let uuid =
        Uuid::parse_str(step_id).map_err(|error| format!("invalid agent step id: {error}"))?;
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&uuid.as_bytes()[..8]);
    let value = u64::from_be_bytes(bytes) & i64::MAX as u64;
    Ok(i64::try_from(value.max(1)).expect("masked correlation id fits i64"))
}

fn list_conversations(transaction: &Transaction<'_>) -> StoreResult<Vec<ConversationSummary>> {
    let mut statement = transaction
        .prepare(
            "SELECT id, title, agent_mode, created_at, updated_at
             FROM conversations ORDER BY updated_at DESC, id DESC",
        )
        .map_err(store_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok(ConversationSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                agent_mode: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })
        .map_err(store_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(store_error)
}

fn conversation_summary(
    transaction: &Transaction<'_>,
    id: &str,
) -> StoreResult<ConversationSummary> {
    transaction
        .query_row(
            "SELECT id, title, agent_mode, created_at, updated_at
             FROM conversations WHERE id = ?1",
            [id],
            |row| {
                Ok(ConversationSummary {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    agent_mode: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(store_error)?
        .ok_or_else(|| "conversation was not found".into())
}

fn load_conversation(transaction: &Transaction<'_>, id: &str) -> StoreResult<ConversationDetail> {
    let summary = conversation_summary(transaction, id)?;
    let mut statement = transaction
        .prepare(
            "SELECT id, conversation_id, role, content, status, kind, source,
                    metadata_json, created_at, updated_at
             FROM messages WHERE conversation_id = ?1 ORDER BY created_at, rowid",
        )
        .map_err(store_error)?;
    let rows = statement
        .query_map([id], |row| {
            let role: String = row.get(2)?;
            let status: String = row.get(4)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                role,
                row.get::<_, String>(3)?,
                status,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, i64>(9)?,
            ))
        })
        .map_err(store_error)?;
    let messages = rows
        .map(|row| {
            let (
                id,
                conversation_id,
                role,
                content,
                status,
                kind,
                source,
                metadata_json,
                created_at,
                updated_at,
            ) = row.map_err(store_error)?;
            Ok(StoredMessage {
                id,
                conversation_id,
                role: MessageRole::parse(&role)?,
                content,
                status: MessageStatus::parse(&status)?,
                kind,
                source,
                metadata_json,
                created_at,
                updated_at,
            })
        })
        .collect::<StoreResult<Vec<_>>>()?;
    let agent_runs = load_agent_runs(transaction, id)?;
    Ok(ConversationDetail {
        id: summary.id,
        title: summary.title,
        agent_mode: summary.agent_mode,
        created_at: summary.created_at,
        updated_at: summary.updated_at,
        messages,
        agent_runs,
    })
}

fn load_agent_runs(
    transaction: &Transaction<'_>,
    conversation_id: &str,
) -> StoreResult<Vec<AgentRunTrace>> {
    let mut run_statement = transaction
        .prepare(
            "SELECT id, assistant_message_id, mode, status, started_at, finished_at
             FROM agent_runs
             WHERE conversation_id = ?1 AND mode = 'react'
             ORDER BY started_at, rowid",
        )
        .map_err(store_error)?;
    let run_rows = run_statement
        .query_map([conversation_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        })
        .map_err(store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(store_error)?;

    run_rows
        .into_iter()
        .map(
            |(run_id, assistant_message_id, mode, status, started_at, finished_at)| {
                let mut tool_statement = transaction
                    .prepare(
                        "SELECT id, stage, status, decision_json, output_content,
                                started_at, finished_at
                         FROM agent_steps
                         WHERE run_id = ?1 AND kind = 'tool'
                         ORDER BY step_index",
                    )
                    .map_err(store_error)?;
                let tools = tool_statement
                    .query_map([run_id.as_str()], |row| {
                        let stage = row.get::<_, String>(1)?;
                        let arguments_json = row
                            .get::<_, Option<String>>(3)?
                            .unwrap_or_else(|| "{}".into());
                        let input = serde_json::from_str::<serde_json::Value>(&arguments_json)
                            .ok()
                            .and_then(|value| {
                                value
                                    .get("expression")
                                    .and_then(serde_json::Value::as_str)
                                    .map(str::to_string)
                            })
                            .unwrap_or(arguments_json);
                        let tool_started_at = row.get::<_, i64>(5)?;
                        let tool_finished_at = row.get::<_, Option<i64>>(6)?;
                        let duration_ms = tool_finished_at
                            .unwrap_or(tool_started_at)
                            .saturating_sub(tool_started_at);
                        Ok(AgentToolTrace {
                            activity_id: row.get(0)?,
                            tool_name: stage
                                .strip_prefix("tool:")
                                .unwrap_or(stage.as_str())
                                .to_string(),
                            status: row.get(2)?,
                            input,
                            output: row.get(4)?,
                            duration_ms: u64::try_from(duration_ms).unwrap_or_default(),
                        })
                    })
                    .map_err(store_error)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(store_error)?;
                Ok(AgentRunTrace {
                    run_id,
                    assistant_message_id,
                    mode,
                    status,
                    started_at,
                    finished_at,
                    tools,
                })
            },
        )
        .collect()
}

fn insert_message(
    transaction: &Transaction<'_>,
    conversation_id: &str,
    role: MessageRole,
    content: &str,
    status: MessageStatus,
    timestamp: i64,
) -> StoreResult<StoredMessage> {
    let source = match role {
        MessageRole::User => "user",
        MessageRole::Assistant => "model",
    };
    insert_message_with_provenance(
        transaction,
        conversation_id,
        role,
        content,
        status,
        "chat",
        source,
        None,
        timestamp,
    )
}

#[allow(clippy::too_many_arguments)]
fn insert_message_with_provenance(
    transaction: &Transaction<'_>,
    conversation_id: &str,
    role: MessageRole,
    content: &str,
    status: MessageStatus,
    kind: &str,
    source: &str,
    metadata_json: Option<&str>,
    timestamp: i64,
) -> StoreResult<StoredMessage> {
    let message = StoredMessage {
        id: Uuid::new_v4().to_string(),
        conversation_id: conversation_id.to_string(),
        role,
        content: content.to_string(),
        status,
        kind: kind.into(),
        source: source.into(),
        metadata_json: metadata_json.map(str::to_string),
        created_at: timestamp,
        updated_at: timestamp,
    };
    transaction
        .execute(
            "INSERT INTO messages(
                id, conversation_id, role, content, status, kind, source,
                metadata_json, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                message.id,
                message.conversation_id,
                role.as_str(),
                message.content,
                status.as_str(),
                message.kind,
                message.source,
                message.metadata_json,
                timestamp
            ],
        )
        .map_err(store_error)?;
    Ok(message)
}

fn automatic_title(prompt: &str) -> String {
    prompt
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(40)
        .collect()
}

fn normalize_title(title: &str, max_chars: usize) -> StoreResult<String> {
    let normalized = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err("conversation title must not be empty".into());
    }
    if normalized.chars().count() > max_chars {
        return Err(format!(
            "conversation title must not exceed {max_chars} characters"
        ));
    }
    Ok(normalized)
}

fn now_millis() -> StoreResult<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before Unix epoch: {error}"))?;
    i64::try_from(duration.as_millis()).map_err(|_| "system timestamp exceeds i64".into())
}

fn store_error(error: rusqlite::Error) -> String {
    format!("conversation database operation failed: {error}")
}

#[cfg(test)]
mod tests {
    use rusqlite::params;

    use super::{ConversationPromptSnapshot, ConversationStore, MessageRole, MessageStatus};

    #[test]
    fn bootstrap_migrates_without_creating_an_empty_conversation_and_recovers_turns() {
        let store = ConversationStore::open_in_memory().unwrap();
        let first = store.bootstrap().unwrap();

        assert!(first.conversations.is_empty());
        assert!(first.selected.is_none());

        let turn = store.start_new_turn("first prompt").unwrap();
        let second = store.bootstrap().unwrap();
        let selected = second.selected.unwrap();
        let recovered = selected
            .messages
            .iter()
            .find(|message| message.id == turn.assistant.id)
            .unwrap();

        assert_eq!(recovered.status, MessageStatus::Interrupted);
    }

    #[test]
    fn bootstrap_is_repeatable_and_records_all_migrations() {
        let store = ConversationStore::open_in_memory().unwrap();

        store.bootstrap().unwrap();
        store.bootstrap().unwrap();

        assert_eq!(store.migration_count().unwrap(), 4);
    }

    #[test]
    fn agent_preferences_and_conversation_modes_are_independent() {
        let store = ConversationStore::open_in_memory().unwrap();
        store.bootstrap().unwrap();

        assert_eq!(store.agent_preferences().unwrap().default_mode, "chat");
        store.set_default_agent_mode("react").unwrap();
        assert_eq!(store.agent_preferences().unwrap().default_mode, "react");

        let turn = store
            .start_new_agent_turn_with_mode("question", "chat", None)
            .unwrap();
        assert_eq!(turn.conversation.agent_mode, "chat");
        let submission = store
            .agent_submission(
                turn.agent_run_id.as_deref().unwrap(),
                turn.agent_step_id.as_deref().unwrap(),
                &turn.conversation.id,
            )
            .unwrap();
        assert_eq!(submission.mode, "chat");
    }

    #[test]
    fn mode_change_message_is_visible_but_excluded_from_model_context() {
        let store = ConversationStore::open_in_memory().unwrap();
        store.bootstrap().unwrap();
        let turn = store
            .start_new_agent_turn_with_prompt("question", None)
            .unwrap();
        let submission = store
            .agent_submission(
                turn.agent_run_id.as_deref().unwrap(),
                turn.agent_step_id.as_deref().unwrap(),
                &turn.conversation.id,
            )
            .unwrap();
        store
            .finish_agent_step(
                submission.correlation_id,
                "answer",
                MessageStatus::Complete,
                Some("strategy-complete"),
            )
            .unwrap();

        let changed = store
            .set_conversation_agent_mode(&turn.conversation.id, "react")
            .unwrap();
        let notice = changed.messages.last().unwrap();
        assert_eq!(changed.agent_mode, "react");
        assert_eq!(notice.kind, "agent-mode-change");
        assert_eq!(notice.source, "application");
        assert!(notice.content.contains("ReAct"));
        let context = store.model_prompt_context(&changed.id).unwrap();
        assert_eq!(context.agent_mode, "react");
        assert_eq!(context.messages.len(), 2);
        assert!(context
            .messages
            .iter()
            .all(|message| !message.content.contains("ReAct 모드")));

        let next = store
            .start_agent_turn_with_prompt(&changed.id, "next", None)
            .unwrap();
        let next_submission = store
            .agent_submission(
                next.agent_run_id.as_deref().unwrap(),
                next.agent_step_id.as_deref().unwrap(),
                &changed.id,
            )
            .unwrap();
        assert_eq!(next_submission.mode, "react");

        store
            .finish_agent_step(
                next_submission.correlation_id,
                "done",
                MessageStatus::Complete,
                Some("strategy-complete"),
            )
            .unwrap();
        let cleared = store.clear_conversation(&changed.id).unwrap();
        assert_eq!(cleared.agent_mode, "react");
        assert!(cleared.messages.is_empty());
    }

    #[test]
    fn agent_turn_preparation_and_terminal_commit_are_atomic() {
        let store = ConversationStore::open_in_memory().unwrap();
        store.bootstrap().unwrap();
        let turn = store
            .start_new_agent_turn_with_prompt("question", None)
            .unwrap();
        let submission = store
            .agent_submission(
                turn.agent_run_id.as_deref().unwrap(),
                turn.agent_step_id.as_deref().unwrap(),
                &turn.conversation.id,
            )
            .unwrap();

        store
            .bind_agent_request(&submission.run_id, &submission.step_id, "17")
            .unwrap();
        assert!(store
            .finish_agent_step(
                submission.correlation_id,
                "answer",
                MessageStatus::Complete,
                Some("strategy-complete"),
            )
            .unwrap());
        assert!(!store
            .finish_agent_step(
                submission.correlation_id,
                "duplicate",
                MessageStatus::Complete,
                Some("strategy-complete"),
            )
            .unwrap());

        let detail = store.load_conversation(&turn.conversation.id).unwrap();
        let assistant = detail
            .messages
            .iter()
            .find(|message| message.id == turn.assistant.id)
            .unwrap();
        assert_eq!(assistant.content, "answer");
        assert_eq!(assistant.status, MessageStatus::Complete);
    }

    #[test]
    fn react_final_decision_keeps_raw_step_output_and_user_facing_answer() {
        let store = ConversationStore::open_in_memory().unwrap();
        store.bootstrap().unwrap();
        let turn = store
            .start_new_agent_turn_with_mode("question", "react", None)
            .unwrap();
        let submission = store
            .agent_submission(
                turn.agent_run_id.as_deref().unwrap(),
                turn.agent_step_id.as_deref().unwrap(),
                &turn.conversation.id,
            )
            .unwrap();
        store
            .bind_agent_request(&submission.run_id, &submission.step_id, "17")
            .unwrap();
        let raw = r#"{"type":"final","content":"answer"}"#;
        assert!(store
            .finish_agent_final_decision(submission.correlation_id, raw, raw, "answer")
            .unwrap());

        let detail = store.load_conversation(&turn.conversation.id).unwrap();
        assert_eq!(detail.messages.last().unwrap().content, "answer");
        assert_eq!(
            detail.messages.last().unwrap().status,
            MessageStatus::Complete
        );
        let connection = store.lock().unwrap();
        let stored = connection
            .query_row(
                "SELECT s.output_content, s.decision_json, r.status
                 FROM agent_steps s JOIN agent_runs r ON r.id = s.run_id
                 WHERE s.correlation_id = ?1",
                [i64::try_from(submission.correlation_id).unwrap()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(stored, (raw.into(), raw.into(), "complete".into()));
    }

    #[test]
    fn conversation_prompt_snapshot_is_bound_once_and_drives_model_context() {
        let store = ConversationStore::open_in_memory().unwrap();
        store.bootstrap().unwrap();
        let initial = ConversationPromptSnapshot {
            persona_id: "dolsoe".into(),
            persona_revision: "revision-1".into(),
            system_prompt: "system one".into(),
        };
        let changed = ConversationPromptSnapshot {
            persona_id: "dolsoe".into(),
            persona_revision: "revision-2".into(),
            system_prompt: "system two".into(),
        };
        let first = store
            .start_new_turn_with_prompt("first", Some(&initial))
            .unwrap();
        store
            .finish_turn(&first.assistant.id, "answer", MessageStatus::Complete)
            .unwrap();
        store
            .start_turn_with_prompt(&first.conversation.id, "second", Some(&changed))
            .unwrap();

        assert_eq!(
            store.prompt_snapshot(&first.conversation.id).unwrap(),
            Some(initial.clone())
        );
        let context = store.model_prompt_context(&first.conversation.id).unwrap();
        assert_eq!(context.agent_mode, "chat");
        assert_eq!(context.snapshot, Some(initial));
        assert_eq!(
            context
                .messages
                .iter()
                .map(|message| message.role.as_str())
                .collect::<Vec<_>>(),
            vec!["user", "assistant", "user"]
        );
    }

    #[test]
    fn conversation_crud_and_turn_lifecycle_are_atomic() {
        let store = ConversationStore::open_in_memory().unwrap();
        store.bootstrap().unwrap();
        let initial = store.start_new_turn("initial prompt").unwrap().conversation;
        let created = store.start_new_turn("created prompt").unwrap().conversation;
        store.clear_conversation(&created.id).unwrap();

        let renamed = store
            .rename_conversation(&created.id, "  renamed   chat ")
            .unwrap();
        assert_eq!(renamed.title, "renamed chat");

        let turn = store
            .start_turn(&created.id, "  한국어   첫 질문  ")
            .unwrap();
        assert_eq!(turn.conversation.title, "한국어 첫 질문");
        assert!(store
            .finish_turn(&turn.assistant.id, "answer", MessageStatus::Complete)
            .unwrap());
        assert!(!store
            .finish_turn(&turn.assistant.id, "duplicate", MessageStatus::Complete)
            .unwrap());

        let cleared = store.clear_conversation(&created.id).unwrap();
        assert!(cleared.messages.is_empty());
        let fallback = store.delete_conversation(&created.id).unwrap().unwrap();
        assert_eq!(fallback.id, initial.id);
    }

    #[test]
    fn deleting_last_conversation_leaves_the_store_empty() {
        let store = ConversationStore::open_in_memory().unwrap();
        store.bootstrap().unwrap();
        let only = store.start_new_turn("only prompt").unwrap().conversation;

        let fallback = store.delete_conversation(&only.id).unwrap();

        assert!(fallback.is_none());
        assert!(store.bootstrap().unwrap().conversations.is_empty());
    }

    #[test]
    fn titles_are_unicode_safe_and_validated() {
        let store = ConversationStore::open_in_memory().unwrap();
        let prompt = "한".repeat(45);
        store.bootstrap().unwrap();
        let turn = store.start_new_turn(&prompt).unwrap();
        let conversation = turn.conversation.clone();

        assert_eq!(turn.conversation.title.chars().count(), 40);
        assert!(store
            .rename_conversation(&conversation.id, &"가".repeat(81))
            .is_err());
        assert!(store.rename_conversation(&conversation.id, "  ").is_err());
    }

    #[test]
    fn lifecycle_rejects_unknown_ids_and_non_terminal_status() {
        let store = ConversationStore::open_in_memory().unwrap();
        store.bootstrap().unwrap();

        assert!(store.load_conversation("missing").is_err());
        assert!(store.clear_conversation("missing").is_err());
        assert!(store.delete_conversation("missing").is_err());
        assert!(store
            .finish_turn("missing", "content", MessageStatus::Streaming)
            .is_err());
    }

    #[test]
    fn messages_with_the_same_timestamp_keep_insertion_order() {
        let store = ConversationStore::open_in_memory().unwrap();
        store.bootstrap().unwrap();
        let conversation = store.start_new_turn("placeholder").unwrap().conversation;
        store.clear_conversation(&conversation.id).unwrap();
        {
            let connection = store.lock().unwrap();
            connection
                .execute(
                    "INSERT INTO messages(id, conversation_id, role, content, status, created_at, updated_at) VALUES (?1, ?2, 'user', 'prompt', 'complete', 1, 1)",
                    params!["z-user", conversation.id],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO messages(id, conversation_id, role, content, status, created_at, updated_at) VALUES (?1, ?2, 'assistant', 'answer', 'complete', 1, 1)",
                    params!["a-assistant", conversation.id],
                )
                .unwrap();
        }

        let loaded = store.load_conversation(&conversation.id).unwrap();

        assert_eq!(loaded.messages[0].role, MessageRole::User);
        assert_eq!(loaded.messages[1].role, MessageRole::Assistant);
    }
}
