# Conversation Persistence Design

## Goal

Persist conversation sessions and messages in a local SQLite database so the desktop app restores them after restart. This slice covers conversation CRUD, title search, terminal message persistence, and interrupted-generation recovery without expanding into settings, runtime-pack, or detailed inference-run storage.

## Scope

### Included

- Create, select, rename, clear, and delete conversations.
- Restore conversations and messages after app restart.
- Generate a local title from the first user message.
- Persist user messages before native inference starts.
- Create an empty `streaming` assistant message for every submitted turn.
- Finalize assistant content and status on `done`, `cancelled`, or `error`.
- Convert leftover `streaming` messages to `interrupted` during startup.
- Search loaded conversation titles in React.
- Keep the current one-active-generation runtime limit while routing events to the conversation that started the request.

### Excluded

- Settings profiles, runtime-pack records, and inference-run history.
- SQLite full-text search.
- Periodic partial writes during token streaming.
- Multiple concurrent native generations.
- Cloud sync, export, encryption, and retention controls.

Because partial streaming writes are excluded, a process crash can lose assistant text emitted after the last terminal event. The empty assistant row is still recovered as `interrupted`, making the incomplete turn visible.

## Chosen Approach

Use `rusqlite` behind a Rust repository and expose task-oriented Tauri commands. React never opens SQLite directly.

This keeps migrations, transactions, foreign-key behavior, and recovery in one native boundary. It also preserves the same frontend and database contracts when a macOS build is added.

Rejected alternatives:

- `tauri-plugin-sql`: faster initial wiring, but it couples React to the schema and distributes transaction rules across the UI.
- JSON files: simple for one conversation, but weak for atomic delete/clear, migration, ordering, and future query needs.

## Database

The database path is:

```text
<app_local_data_dir>/local-llm-wiki.db
```

The connection enables:

```sql
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;
```

Migration 1 creates:

```sql
CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);

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
```

IDs are UUID v4 strings. Timestamps are Unix milliseconds stored as signed 64-bit integers. Migration application and startup recovery run in one transaction.

## Rust Boundaries

`conversation_store.rs` owns the `rusqlite::Connection` behind a mutex and contains all SQL. Its public methods return domain records rather than database rows.

`conversation_commands.rs` defines serializable DTOs and these Tauri commands:

- `conversation_bootstrap`: migrate, recover interrupted messages, load summaries, create one empty conversation if none exists, and return the selected conversation with messages.
- `conversation_create`: create and return an empty conversation.
- `conversation_load`: load one conversation and its ordered messages.
- `conversation_rename`: validate and update a title.
- `conversation_clear`: delete a conversation's messages while retaining the conversation.
- `conversation_delete`: atomically delete the conversation and return the most recently updated remaining conversation, creating one if needed.
- `conversation_start_turn`: atomically insert the user message and empty streaming assistant message; set the first-message title when applicable.
- `conversation_finish_turn`: update only the expected assistant message with final content and terminal status.

Commands use `spawn_blocking` so SQLite work does not block Tauri's async command executor. Store errors become concise command errors without exposing SQL text or local paths unnecessarily.

## Frontend State

`useConversationWorkspace` composes the native inference service and conversation service. It owns:

- ordered conversation summaries;
- the selected conversation ID and loaded messages;
- a request binding from native request handle to conversation and assistant message IDs;
- title search text;
- pending persistence errors.

The existing runtime reducer remains responsible for model state and telemetry. Conversation messages move to workspace state so native events can update the originating conversation even when another conversation is selected.

Current runtime capacity remains one active generation globally. The user may select another conversation while generation continues, but every composer remains disabled until that request reaches a terminal event. The source conversation keeps its sidebar generation indicator.

## Data Flow

### Startup

1. Tauri creates the database directory and opens the store.
2. `conversation_bootstrap` applies migrations and marks leftover `streaming` messages as `interrupted`.
3. React receives summaries and the most recently updated conversation.
4. The selected conversation's messages render before model inference is used.

### Submit

1. React calls `conversation_start_turn` with the selected conversation and prompt.
2. The transaction inserts the user and assistant rows and updates the local title when this is the first user message.
3. React calls `llm_submit`.
4. Once accepted, React binds the request handle to the assistant message.
5. Token events update only in-memory content for that bound assistant message.
6. A terminal event calls `conversation_finish_turn` with the final content and status.
7. Metrics update remains independent from message persistence.

If native submission fails after the turn is inserted, React finalizes the assistant row as `error` with the native error text.

### Clear And Delete

- Clear asks for confirmation, cancels the conversation's active request if present, then deletes only its messages.
- Delete asks for confirmation, cancels the conversation's active request if present, deletes it with cascading messages, and selects the returned fallback conversation.
- New conversation creates a distinct empty row rather than clearing the current session.

## UI Behavior

- `Ctrl+N` creates and selects a new conversation.
- The sidebar renders persisted sessions ordered by `updated_at DESC`.
- Search filters titles case-insensitively without changing database contents.
- Selecting a session loads its messages and closes diagnostics.
- The session overflow menu provides rename, clear, and delete actions.
- Header title rename and overflow-menu rename use the same command and validation.
- Titles trim surrounding whitespace, collapse internal whitespace, and allow 1 to 80 Unicode characters.
- Automatic titles use the first user message after whitespace normalization, truncated to 40 Unicode characters.
- Delete and clear remain confirmation-gated; rename is reversible and does not require confirmation.

The existing `?state=` mock routes remain isolated from SQLite and deterministic for Playwright visual-state tests.

## Error Handling

- Database open or migration failure stops workspace bootstrap and displays a persistent storage error.
- A failed create, rename, clear, or delete leaves the current in-memory selection unchanged.
- A failed `conversation_start_turn` prevents native submission, avoiding an untracked response.
- A failed terminal write leaves the completed content visible in memory and displays a storage error; the row remains recoverable as interrupted after restart.
- Commands reject unknown conversation/message IDs and invalid roles, statuses, or titles.
- `conversation_finish_turn` updates only assistant rows currently marked `streaming`, making duplicate terminal events idempotent.

## Testing

Rust tests use temporary SQLite files and cover:

- first migration and repeat startup;
- automatic initial conversation creation;
- create, load, rename, clear, and cascade delete;
- first-message title generation with Unicode truncation;
- transactional turn insertion;
- idempotent terminal finalization;
- startup conversion from `streaming` to `interrupted`;
- fallback conversation selection after delete.

Frontend unit tests cover workspace reducer ordering, selection, request routing, search, and persistence failures. Playwright keeps all existing mock tests and adds deterministic session create/select/search/rename/delete flows. A Tauri smoke test verifies that conversations and terminal messages survive an actual app restart.

## Success Criteria

- Conversations and completed/cancelled/error messages survive app restart.
- Incomplete streaming rows appear as interrupted after restart.
- New, select, rename, clear, delete, and search behave consistently from keyboard and pointer input.
- Switching conversations cannot route tokens or terminal state to the wrong assistant message.
- SQLite is accessed only from Rust.
- Existing native CPU inference and all mock-state tests remain passing.
