# Agent Runtime Foundation Design

## 1. Goal

Build a stable foundation for selectable agent modes without changing the
current chat behavior in the first milestone.

The foundation must support:

- a default mode for new conversations;
- a persistent per-conversation mode;
- a run-time snapshot that cannot change during an active run;
- mode-specific, multi-stage prompt pipelines;
- ReAct and future Plan-and-Solve strategies;
- tool execution, approval, and observations;
- a progress window that resets only after a novel successful tool result;
- an absolute run limit that never resets;
- exact run and step correlation for streaming events;
- prompt inspection for every model step;
- cancellation, crash recovery, and deterministic terminal persistence.

The first shipped strategy was `chat`. It performs one model step and remains
behaviorally equivalent to the original conversation flow. ReAct v1 is now
available as the first multi-step strategy.

### 1.1 Implementation status: ReAct v1

The runtime foundation and first selectable multi-step mode are implemented:

- every newly prepared turn atomically creates a durable `agent_run` and its
  first `agent_step`;
- `chat` remains the default, while `react` can schedule bounded model and tool
  steps;
- ReAct decisions use a strict JSON contract with one repair attempt;
- the first read-only tool is a calculator;
- a novel successful tool result resets only the progress window, while
  absolute model-step and tool-call limits never reset;
- a persisted nonzero correlation ID travels through native
  `request_user_data`;
- `AgentController` owns correlated runtime events, accumulates output, and
  commits terminal run, step, and assistant-message state before emitting the
  terminal event to the frontend;
- startup marks prepared or running runs and steps as `interrupted`;
- the settings default applies to new conversations, and each conversation
  persists its own selectable mode.

The controller keeps synchronized in-process ownership because the native
runtime currently permits one active request. Follow-up ReAct model steps run
outside the runtime event callback so persistence and resubmission do not block
event relay.

## 2. Non-goals

This foundation does not implement Plan-and-Solve or production-capability
tools. It does not expose unrestricted chain-of-thought, run multiple model
requests in parallel, or add remote inference.

Mode prompt editing is not part of the first implementation milestone. The
contracts allow validated user guidance later without making the parser-critical
protocol text editable.

## 3. Terms

- **Agent mode**: a named strategy such as `chat`, `react`, or
  `plan-and-solve`.
- **Conversation profile**: the mode and policy used for future runs in one
  conversation.
- **Agent run**: the complete handling of one user message.
- **Agent step**: one model invocation or one tool invocation inside a run.
- **Stage**: a strategy-defined purpose for a model step, such as
  `chat-response`, `react-decision`, or `plan-create`.
- **Decision**: the parsed result of a model step: final answer, tool call,
  continue, plan, replan, or failure.
- **Observation**: a normalized tool result made available to later steps.
- **Progress window**: the number of consecutive model steps since meaningful
  progress.
- **Synthetic assistant message**: application-authored conversation text that
  is rendered like an assistant response but excluded from model context.

## 4. Product Semantics

### 4.1 Default mode

The setting is named **Default mode for new conversations**.

Changing it:

- persists the application default;
- does not modify existing conversations;
- does not create a conversation message or success notification;
- initializes later conversation drafts with the selected mode.

### 4.2 New conversation

A new conversation does not exist in SQLite until its first user message, which
matches current behavior.

Opening a draft copies the current default profile into frontend draft state.
The user may override the mode in the draft header. The first send passes the
profile currently displayed by the draft. The backend validates and stores the
conversation, prompt snapshot, conversation profile, messages, run, and first
step before inference starts.

If the application default changes while an empty draft is open, the draft
updates only when the user has not manually overridden its mode.

### 4.3 Existing conversation

Changing the header mode:

- is allowed only when no run is active;
- persists the new conversation profile;
- affects the next user message and every later run until changed again;
- never rewrites previous runs or their prompt snapshots;
- adds a synthetic assistant message after the database update succeeds.

Example:

```text
Good. I will continue this conversation in ReAct mode.
```

The exact Korean product copy is chosen in the UI implementation. It must be
application-authored, deterministic, and stored with explicit provenance.

Reopening the conversation or restarting the application restores its saved
mode. Changing the application default does not affect it.

### 4.4 Active run

An active run owns an immutable profile snapshot. Mode and policy controls are
disabled while it runs. A run cannot switch strategy halfway through.

### 4.5 Clear and delete

Clearing a conversation deletes normal and synthetic timeline messages and run
history but preserves the conversation profile. Deleting a conversation removes
its profile, runs, steps, and messages by foreign-key cascade.

## 5. Architecture

```text
React UI
  |
  | agent commands / agent events
  v
AgentController actor (Rust)
  |-- AgentModeRegistry
  |-- PromptCompiler
  |-- AgentStore / ConversationStore
  |-- ToolRegistry and ToolExecutor (future)
  |
  | one low-level inference request at a time
  v
RuntimeHost -> llm-worker -> llm-runtime -> native runtime
```

### 5.1 Ownership

The `AgentController` owns:

- run and step state machines;
- mode selection and immutable run snapshots;
- prompt compilation;
- model re-submission;
- output parsing;
- tool dispatch and approval state;
- budgets, timeout, and cancellation;
- terminal database updates;
- conversion of low-level runtime events to `agent://event`.

The low-level LLM worker owns:

- runtime, model, and native request lifetimes;
- one inference submission;
- token and terminal event delivery;
- native cancellation;
- inference metrics.

React owns:

- default and conversation mode controls;
- draft override state;
- timeline and run-status rendering;
- developer prompt inspector rendering;
- commands initiated directly by the user.

React must not decide whether the loop should continue and must not submit the
next model step.

### 5.2 Actor boundary

`AgentController` is a dedicated Rust actor with a bounded command channel. Its
event loop selects over:

- UI commands;
- low-level runtime events;
- tool completions;
- approval decisions;
- cancellation;
- deadline checks.

This prevents a model step or tool operation from blocking cancellation and
avoids concurrent mutation of run state.

The first version preserves the current global one-active-run limit because the
runtime loads one model with one slot. The types use run IDs and step IDs so a
future multi-slot scheduler does not require a protocol redesign.

## 6. Core Invariants

1. A conversation has one persistent profile for future runs.
2. A run snapshots its mode, policy, persona revision, mode protocol revision,
   generation settings, and tool registry revision before its first model step.
3. Run snapshots are immutable.
4. A step belongs to exactly one run and has a monotonically increasing index.
5. At most one step in a run is active.
6. Every accepted model request maps to exactly one run and one model step.
7. Every model step receives exactly one terminal outcome.
8. A native terminal event may finalize only its mapped step.
9. Intermediate model output, plans, tool calls, and observations are not normal
   conversation messages.
10. Only the final user-visible answer finalizes the run's assistant message.
11. Synthetic assistant messages never enter model context.
12. A successful tool call resets the progress window only when its result is
    classified as novel progress.
13. Absolute model-step, tool-call, token, time, and repetition limits never
    reset.
14. Cancellation is idempotent.
15. A run receives one durable terminal status.

## 7. Agent Mode Contract

### 7.1 Registry definition

Each mode is registered in code:

```rust
pub struct AgentModeDefinition {
    pub id: AgentModeId,
    pub display_name: &'static str,
    pub protocol_revision: &'static str,
    pub available: bool,
    pub supports_tools: bool,
    pub supports_planning: bool,
    pub default_policy: AgentRunPolicy,
    pub stages: &'static [AgentStage],
    pub output_contracts: &'static [StageOutputContract],
}
```

Persisted mode IDs are stable lowercase kebab-case values. Display labels are
not persisted.

Unknown persisted modes fail closed to `chat` during migration or bootstrap and
produce a diagnostic record. They are never passed directly to prompt paths or
dynamic code loading.

### 7.2 Strategy interface

```rust
pub trait AgentModeStrategy: Send + Sync {
    fn definition(&self) -> &'static AgentModeDefinition;

    fn initial_stage(&self, context: &AgentRunContext) -> AgentStage;

    fn compile(
        &self,
        stage: AgentStage,
        context: &AgentRunContext,
    ) -> Result<CompiledModelInput, AgentError>;

    fn parse(
        &self,
        stage: AgentStage,
        output: &CompletedModelOutput,
    ) -> Result<AgentDecision, AgentError>;

    fn transition(
        &self,
        state: &AgentRunState,
        decision: &AgentDecision,
    ) -> Result<AgentTransition, AgentError>;
}
```

A strategy describes stages and transitions. It cannot execute a tool, write the
database, emit UI events, or bypass budgets.

### 7.3 Decisions

```rust
pub enum AgentDecision {
    Final { content: String },
    ToolCall { name: String, arguments: serde_json::Value },
    Continue { summary: String },
    Plan { items: Vec<PlanItem> },
    Replan { reason: String, items: Vec<PlanItem> },
}
```

`Final` content is the only decision eligible to become the normal assistant
message. Detailed hidden reasoning is neither required nor stored.

### 7.4 Output contracts

Each stage declares one output contract:

```rust
pub enum StageOutputContract {
    FreeText,
    JsonSchema {
        schema_id: &'static str,
        schema_version: u32,
    },
}
```

`chat-response` uses free text. Agent decision stages use application-owned JSON
schemas. A future grammar-constrained decoder may enforce these schemas. Until
then, parsing is strict and a parse repair attempt consumes both progress and
absolute budgets.

## 8. Prompt Pipeline

### 8.1 Layering

Every model step is compiled from explicit layers:

```text
1. conversation persona snapshot
2. immutable mode protocol for the current stage
3. validated user mode guidance (future)
4. available tool definitions
5. run state: goal, plan, observations, limits
6. normal conversation history
7. current step input
```

The compiler produces structured chat messages. It does not concatenate an
untyped mega-string before the model's GGUF chat template is applied.

### 8.2 Protocol resources

Default protocol resources are bundled and versioned:

```text
resources/agent-modes/
  chat/
    manifest.json
    response.md
  react/
    manifest.json
    decision.md
    observation.md
  plan-and-solve/
    manifest.json
    planner.md
    executor.md
    reviewer.md
    replanner.md
```

Parser-critical protocol templates are immutable application resources. A later
settings UI may edit a separate guidance document, which is inserted into a
clearly delimited layer and receives its own revision digest.

### 8.3 Prompt snapshot

Every model step stores:

- persona ID and revision;
- mode ID and protocol revision;
- stage ID;
- user guidance revision when present;
- tool registry revision;
- generation settings snapshot;
- exact ordered model messages;
- output contract ID and version;
- raw model output and parsed decision;
- character and estimated token counts.

The stored prompt is the actual input, not a reconstruction from current
settings. Size limits are validated before submission.

### 8.4 Mode switching

Conversation history remains shared when a conversation changes mode. The new
run uses the new strategy and stage pipeline. Previous plans, observations, and
synthetic mode-change messages are not automatically injected.

A strategy may explicitly import a normalized summary from an earlier run in a
future feature, but it must never consume another strategy's private step log by
default.

## 9. Persistence

SQLite remains the source of truth. Agent settings are stored in the existing
application database rather than only in browser local storage.

### 9.1 Migration 3: Agent Loop Zero

The shipped migration adds the minimal durable run and step records:

```sql
CREATE TABLE agent_runs (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    user_message_id TEXT NOT NULL,
    assistant_message_id TEXT NOT NULL,
    mode TEXT NOT NULL,
    protocol_revision TEXT NOT NULL,
    policy_json TEXT NOT NULL,
    status TEXT NOT NULL,
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

CREATE TABLE agent_steps (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    step_index INTEGER NOT NULL,
    kind TEXT NOT NULL,
    stage TEXT NOT NULL,
    status TEXT NOT NULL,
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
```

Indexes cover conversation run order and run step order. Correlation lookup is
covered by its unique index.

### 9.2 Planned profile and provenance migration

A later migration adds `agent_preferences`, per-conversation mode and policy,
message `kind` and `source`, prompt snapshots, tool counters, and structured
step input/output/decision payloads. Existing conversations receive the
one-step `chat` policy at that point.

### 9.3 Synthetic assistant messages

A mode change inserts:

```text
role     = assistant
kind     = agent-mode-change
source   = application
content  = deterministic product copy
metadata = previousMode, mode
```

Normal prompt history includes only `kind='chat'` messages. This filter is
enforced in Rust, with the TypeScript history builder removed from the
authoritative submission path.

### 9.4 Atomic run creation

The frontend no longer calls `conversation_start_*` and `llm_submit` as separate
authoritative operations.

`agent_start_run` performs one SQLite transaction that:

1. creates a conversation when needed;
2. binds the persona and conversation profile snapshots;
3. inserts the user message;
4. inserts the empty streaming assistant message;
5. inserts the run and first step;
6. commits.

It then submits the low-level inference request. Submission failure marks the
step, run, and assistant message as failed in a second transaction. External
inference cannot be part of a SQLite transaction, so durable failed state is the
required compensation.

## 10. Run and Step State Machines

### 10.1 Run status

```text
created
  -> running
  -> waiting-approval
  -> running
  -> completed
  -> cancelled
  -> failed
  -> interrupted
  -> limit-reached
```

Terminal states cannot transition.

### 10.2 Model step status

```text
created
  -> compiling
  -> submitted
  -> streaming
  -> parsing
  -> completed
  -> cancelled
  -> failed
  -> interrupted
```

### 10.3 Tool step status

```text
created
  -> waiting-approval
  -> running
  -> completed
  -> denied
  -> cancelled
  -> failed
  -> interrupted
```

State transitions are centralized and unit tested. UI reducers do not infer a
terminal state from missing events.

## 11. Event Correlation

### 11.1 Native correlation

The native ABI already copies `request_user_data` into every request event.
The safe Rust runtime adds an opaque nonzero `u64 correlation_id` to generation
options and transports it through this field. It is an integer token, never a
dereferenced pointer.

The agent actor allocates and persists the correlation ID before submission.
Every low-level event therefore identifies its step even if `queued` arrives
before the submit response is processed.

### 11.2 Agent event envelope

```ts
interface AgentEvent {
  schemaVersion: 1;
  eventId: string;
  runId: string;
  stepId: string;
  conversationId: string;
  assistantMessageId: string;
  requestHandle: string | null;
  runSequence: number;
  requestSequence: string | null;
  kind:
    | "run-started"
    | "step-started"
    | "token"
    | "decision"
    | "tool-started"
    | "tool-completed"
    | "waiting-approval"
    | "run-completed"
    | "run-cancelled"
    | "run-failed"
    | "run-limit-reached";
  payload: unknown;
}
```

`runSequence` is monotonically increasing for one run. React rejects an event
whose run ID or step ID does not match the active snapshot and deduplicates by
event ID.

Low-level `llm://event` becomes internal. Product UI consumes `agent://event`.

### 11.3 Persistence ordering

Run and step creation is persisted before the first start event. Terminal state
is persisted before the terminal event is emitted.

Tokens remain primarily in memory for responsive streaming. The agent actor
checkpoints accumulated output at bounded intervals and always persists the full
terminal output. Reopening the app reads the database snapshot rather than
assuming every emitted event was received.

## 12. Budgets and Progress

The policy schema is versioned:

```json
{
  "schemaVersion": 1,
  "maxProgressSteps": 8,
  "maxTotalSteps": 50,
  "maxToolCalls": 30,
  "maxRepeatedActions": 3,
  "timeoutSeconds": 600,
  "resetProgressOnToolSuccess": true
}
```

Definitions:

- `totalStepCount` increments before every model submission and never resets.
- `progressStepCount` increments before every model submission.
- `totalToolCalls` increments before every tool execution and never resets.
- denied, cancelled, failed, empty, or no-op tool results do not reset progress.
- a successful result resets progress only after `ProgressClassifier` accepts it
  as novel.

The progress classifier uses:

- tool name and canonical argument digest;
- normalized result digest;
- tool-declared outcome (`changed`, `unchanged`, `not-found`, or `failed`);
- earlier calls in the same run.

Repeating the same tool, canonical arguments, and normalized result cannot reset
progress. The absolute limits and deadline are checked before every transition.

`chat` uses:

```text
maxProgressSteps = 1
maxTotalSteps = 1
maxToolCalls = 0
```

## 13. Tool Foundation

The initial milestone defines interfaces but registers no production tools.

```rust
pub trait AgentTool: Send + Sync {
    fn descriptor(&self) -> ToolDescriptor;
    fn approval_requirement(&self, args: &Value) -> ApprovalRequirement;
    fn execute(&self, context: ToolContext, args: Value) -> ToolResult;
}
```

A tool result contains:

- a machine-readable outcome;
- model-facing normalized content;
- user-facing summary;
- a digest used for repetition and progress detection;
- optional artifacts;
- explicit changed/unchanged state.

Tool execution never occurs inside a strategy. Tools receive a cancellation
token and bounded context. Write-capable tools require approval according to a
central policy.

## 14. Cancellation and Recovery

Cancellation:

1. marks the run `cancelling` in memory;
2. cancels the active model request or tool token;
3. ignores nonterminal late events except for diagnostics;
4. waits for or synthesizes the step terminal outcome;
5. persists step, run, and assistant message terminal state;
6. emits one terminal agent event.

On application startup:

- `created`, `running`, `waiting-approval`, and cancelling runs become
  `interrupted`;
- their active steps become `interrupted`;
- their streaming assistant messages become `interrupted`;
- no run resumes automatically;
- the user may send a new message in the conversation.

Automatic run resumption is deferred until tools have explicit idempotency and
recovery contracts.

## 15. Commands

Initial Tauri commands:

```text
agent_get_preferences()
agent_save_preferences(preferences)
agent_set_conversation_profile(conversationId, profile)
agent_start_run(request)
agent_cancel_run(runId)
agent_get_run(runId)
agent_list_conversation_runs(conversationId)
agent_get_step_prompt(stepId)
```

`agent_start_run` accepts either an existing conversation ID or a new-draft
profile. For an existing conversation, the backend ignores client-supplied mode
values and loads the persisted conversation profile.

The old conversation CRUD commands remain. Direct product use of `llm_submit`
and `conversation_finish_turn` is removed after the chat strategy migration.
Low-level commands may remain behind test or diagnostic boundaries.

## 16. Frontend State and UI

Frontend state adds:

```ts
interface DraftAgentProfile {
  mode: AgentModeId;
  policy: AgentRunPolicy;
  source: "default" | "manual";
}

interface ActiveAgentRun {
  runId: string;
  conversationId: string;
  assistantMessageId: string;
  mode: AgentModeId;
  stepId: string | null;
  stage: AgentStage | null;
  progressStepCount: number;
  totalStepCount: number;
  status: AgentRunStatus;
}
```

Settings:

- label: `Default mode for new conversations`;
- save silently;
- render registry modes, but disable definitions whose `available` flag is false;
- disable loop-only controls when the selected mode does not use them.

Conversation header:

- label: `This conversation's mode`;
- persist on selection;
- disable while a run is active;
- render the synthetic assistant line after success;
- revert selection and show an error only when persistence fails.

The developer prompt inspector adds a run/step selector and shows:

- mode and stage;
- snapshot revisions;
- exact structured model messages;
- output contract;
- raw output and parsed decision;
- tool calls and observations when present;
- budget counters and terminal reason.

## 17. Compatibility and Migration

- Existing preferences remain valid; missing agent preferences receive `chat`.
- Existing conversations and messages are preserved.
- Existing conversations receive the `chat` profile and one-step policy.
- Existing persona snapshots remain conversation-bound.
- Mode protocol and tool layers are compiled per run, not written into the
  conversation persona snapshot.
- Conversation search indexes only visible timeline content, including
  synthetic mode-change text if product search requires it.
- Model prompt history includes only complete `kind='chat'` user/assistant pairs.

## 18. Testing

### 18.1 Store and migration

- migration 1 -> 3 and migration 2 -> 3;
- old messages receive correct source and kind;
- old conversations receive the chat profile;
- new conversation profile, messages, run, and step are atomic;
- mode change and synthetic message are atomic;
- terminal state is idempotent;
- cascade deletion and clear semantics.

### 18.2 Strategy and prompt

- chat produces the same ordered model messages as the current implementation;
- every stage compiles deterministic output for a frozen snapshot;
- synthetic messages and step logs are excluded from model context;
- unknown mode, stage, or schema revision fails closed;
- prompt size and message-count limits.

### 18.3 State machine

- valid and invalid transitions;
- one terminal run state;
- cancellation before submit, during streaming, and after native terminal;
- late event from an earlier step cannot mutate the active step;
- duplicate terminal and duplicate event handling;
- startup interruption recovery.

### 18.4 Budgets

- novel successful result resets progress;
- repeated or unchanged result does not reset;
- failures and denials do not reset;
- total limits never reset;
- timeout, repeated action, token, and tool-call limits;
- parse repair consumes budgets.

### 18.5 Integration

Use a fake model and fake tool registry for:

- one-step chat completion;
- multi-step final decision;
- tool success then continuation;
- repeated tool loop;
- tool failure;
- approval denial;
- cancellation;
- limit reached;
- application restart with an active run.

### 18.6 Frontend

- default mode initializes a new draft;
- manual draft override survives unrelated setting changes;
- existing conversation mode does not follow default changes;
- reopened conversation restores its mode;
- active run locks mode selection;
- synthetic assistant text renders but is excluded from prompt requests;
- stale run and step events are ignored;
- narrow and desktop layouts do not overflow.

## 19. Implementation Sequence

### Milestone 1: contracts and storage

- [x] add minimal run and step types;
- [x] add Migration 3 and store APIs;
- [x] register only `chat`;
- [x] add store and migration tests;
- [x] add conversation profiles and message provenance with the mode UI.

### Milestone 2: preferences and conversation UI

- [x] add agent preferences commands and hook;
- [x] add settings tab and conversation selector;
- [x] implement draft inheritance and conversation profile persistence;
- [x] implement synthetic assistant mode-change messages;
- [x] route inference through the selected strategy.

### Milestone 3: event correlation

- [x] transport owned correlation IDs through `request_user_data`;
- [x] correlate native events to persisted run and step state;
- [x] ignore events with stale correlated ownership;
- [ ] add full duplicate and multi-request ordering tests before concurrency.

### Milestone 4: controller and chat migration

- [x] add `AgentController` and the strategy contract;
- [x] implement one-step `ChatStrategy`;
- [x] move terminal persistence into the controller;
- [x] preserve the existing persona prompt compilation path;
- [ ] replace the frontend prepare/submit and fallback finish sequence with one
  `agent_start_run` command after event-start ordering is explicit;
- [x] preserve existing frontend chat tests and production build.

### Milestone 5: recovery and inspector

- [x] add startup interruption recovery for Loop Zero;
- add run and step prompt inspection;
- add checkpointing and diagnostics;
- complete cancellation and integration tests.

### Milestone 6: next modes

Only after the foundation passes all acceptance criteria:

- [x] implement the ReAct protocol and parser;
- [x] add a small read-only tool set;
- implement Plan-and-Solve planner, executor, and reviewer stages;
- [x] mark each mode available independently.

## 20. Acceptance Criteria

The foundation is complete when:

1. Current chat behavior runs through `AgentRun -> ChatStrategy -> AgentStep`.
2. A new conversation receives the current default mode exactly once.
3. Existing conversations retain and restore their own mode.
4. A mode change renders as a synthetic assistant message and never enters the
   model prompt.
5. Every runtime event is correlated to one persisted run and step.
6. Late or duplicate events cannot corrupt another step.
7. Run and step prompt snapshots are inspectable and reproducible.
8. Progress reset and absolute limits satisfy the documented invariants.
9. Cancellation and startup recovery produce one durable terminal state.
10. ReAct or Plan-and-Solve can be added without changing persistence, event, or
    controller contracts.
