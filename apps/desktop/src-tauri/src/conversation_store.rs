use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use uuid::Uuid;

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

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
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
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDetail {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub messages: Vec<StoredMessage>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationBootstrap {
    pub conversations: Vec<ConversationSummary>,
    pub selected: ConversationDetail,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartedTurn {
    pub conversation: ConversationSummary,
    pub user: StoredMessage,
    pub assistant: StoredMessage,
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
    fn open_in_memory() -> StoreResult<Self> {
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

        let mut conversations = list_conversations(&transaction)?;
        if conversations.is_empty() {
            insert_empty_conversation(&transaction, timestamp)?;
            conversations = list_conversations(&transaction)?;
        }
        let selected = load_conversation(&transaction, &conversations[0].id)?;
        transaction.commit().map_err(store_error)?;
        Ok(ConversationBootstrap {
            conversations,
            selected,
        })
    }

    pub fn create_conversation(&self) -> StoreResult<ConversationDetail> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let conversation = insert_empty_conversation(&transaction, now_millis()?)?;
        let detail = load_conversation(&transaction, &conversation.id)?;
        transaction.commit().map_err(store_error)?;
        Ok(detail)
    }

    pub fn load_conversation(&self, id: &str) -> StoreResult<ConversationDetail> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let detail = load_conversation(&transaction, id)?;
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

    pub fn delete_conversation(&self, id: &str) -> StoreResult<ConversationDetail> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        conversation_summary(&transaction, id)?;
        transaction
            .execute("DELETE FROM conversations WHERE id = ?1", [id])
            .map_err(store_error)?;
        let mut conversations = list_conversations(&transaction)?;
        if conversations.is_empty() {
            insert_empty_conversation(&transaction, now_millis()?)?;
            conversations = list_conversations(&transaction)?;
        }
        let fallback = load_conversation(&transaction, &conversations[0].id)?;
        transaction.commit().map_err(store_error)?;
        Ok(fallback)
    }

    pub fn start_turn(&self, conversation_id: &str, prompt: &str) -> StoreResult<StartedTurn> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("prompt must not be empty".into());
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let mut conversation = conversation_summary(&transaction, conversation_id)?;
        let timestamp = now_millis()?;
        let user = insert_message(
            &transaction,
            conversation_id,
            MessageRole::User,
            prompt,
            MessageStatus::Complete,
            timestamp,
        )?;
        let assistant = insert_message(
            &transaction,
            conversation_id,
            MessageRole::Assistant,
            "",
            MessageStatus::Streaming,
            timestamp,
        )?;
        let user_count: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1 AND role = 'user'",
                [conversation_id],
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
                params![conversation.title, timestamp, conversation_id],
            )
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(StartedTurn {
            conversation,
            user,
            assistant,
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
    Ok(())
}

fn insert_empty_conversation(
    transaction: &Transaction<'_>,
    timestamp: i64,
) -> StoreResult<ConversationSummary> {
    let conversation = ConversationSummary {
        id: Uuid::new_v4().to_string(),
        title: "새 대화".into(),
        created_at: timestamp,
        updated_at: timestamp,
    };
    transaction
        .execute(
            "INSERT INTO conversations(id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                conversation.id,
                conversation.title,
                conversation.created_at,
                conversation.updated_at
            ],
        )
        .map_err(store_error)?;
    Ok(conversation)
}

fn list_conversations(transaction: &Transaction<'_>) -> StoreResult<Vec<ConversationSummary>> {
    let mut statement = transaction
        .prepare(
            "SELECT id, title, created_at, updated_at FROM conversations ORDER BY updated_at DESC, id DESC",
        )
        .map_err(store_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok(ConversationSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
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
            "SELECT id, title, created_at, updated_at FROM conversations WHERE id = ?1",
            [id],
            |row| {
                Ok(ConversationSummary {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
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
            "SELECT id, conversation_id, role, content, status, created_at, updated_at FROM messages WHERE conversation_id = ?1 ORDER BY created_at, id",
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
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .map_err(store_error)?;
    let messages = rows
        .map(|row| {
            let (id, conversation_id, role, content, status, created_at, updated_at) =
                row.map_err(store_error)?;
            Ok(StoredMessage {
                id,
                conversation_id,
                role: MessageRole::parse(&role)?,
                content,
                status: MessageStatus::parse(&status)?,
                created_at,
                updated_at,
            })
        })
        .collect::<StoreResult<Vec<_>>>()?;
    Ok(ConversationDetail {
        id: summary.id,
        title: summary.title,
        created_at: summary.created_at,
        updated_at: summary.updated_at,
        messages,
    })
}

fn insert_message(
    transaction: &Transaction<'_>,
    conversation_id: &str,
    role: MessageRole,
    content: &str,
    status: MessageStatus,
    timestamp: i64,
) -> StoreResult<StoredMessage> {
    let message = StoredMessage {
        id: Uuid::new_v4().to_string(),
        conversation_id: conversation_id.to_string(),
        role,
        content: content.to_string(),
        status,
        created_at: timestamp,
        updated_at: timestamp,
    };
    transaction
        .execute(
            "INSERT INTO messages(id, conversation_id, role, content, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![message.id, message.conversation_id, role.as_str(), message.content, status.as_str(), timestamp, timestamp],
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
    use super::{ConversationStore, MessageStatus};

    #[test]
    fn bootstrap_migrates_recovers_and_creates_initial_conversation() {
        let store = ConversationStore::open_in_memory().unwrap();
        let first = store.bootstrap().unwrap();

        assert_eq!(first.conversations.len(), 1);
        assert!(first.selected.messages.is_empty());

        let turn = store
            .start_turn(&first.selected.id, "first prompt")
            .unwrap();
        let second = store.bootstrap().unwrap();
        let recovered = second
            .selected
            .messages
            .iter()
            .find(|message| message.id == turn.assistant.id)
            .unwrap();

        assert_eq!(recovered.status, MessageStatus::Interrupted);
    }

    #[test]
    fn bootstrap_is_repeatable_and_records_one_migration() {
        let store = ConversationStore::open_in_memory().unwrap();

        store.bootstrap().unwrap();
        store.bootstrap().unwrap();

        assert_eq!(store.migration_count().unwrap(), 1);
    }

    #[test]
    fn conversation_crud_and_turn_lifecycle_are_atomic() {
        let store = ConversationStore::open_in_memory().unwrap();
        let initial = store.bootstrap().unwrap().selected;
        let created = store.create_conversation().unwrap();

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
        let fallback = store.delete_conversation(&created.id).unwrap();
        assert_eq!(fallback.id, initial.id);
    }

    #[test]
    fn deleting_last_conversation_creates_a_fallback() {
        let store = ConversationStore::open_in_memory().unwrap();
        let only = store.bootstrap().unwrap().selected;

        let fallback = store.delete_conversation(&only.id).unwrap();

        assert_ne!(fallback.id, only.id);
        assert!(fallback.messages.is_empty());
    }

    #[test]
    fn titles_are_unicode_safe_and_validated() {
        let store = ConversationStore::open_in_memory().unwrap();
        let conversation = store.bootstrap().unwrap().selected;
        let prompt = "한".repeat(45);

        let turn = store.start_turn(&conversation.id, &prompt).unwrap();

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
}
