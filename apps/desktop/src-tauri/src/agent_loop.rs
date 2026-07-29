use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    thread,
    time::Instant,
};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::{
    agent_mode::{
        compile_agent_runtime_system_prompt, parse_react_decision, AgentDecision, AgentMode,
    },
    agent_tools::{ToolContext, ToolGateway, ToolPreparation, ToolResult},
    conversation_store::{ConversationStore, MessageStatus, PreparedAgentStep},
    llm_dto::{LlmEventDto, LlmEventKind, SubmitChatMessage, SubmitRequest, SubmitResponse},
    runtime_host::RuntimeHost,
};

type EventSink = Arc<dyn Fn(LlmEventDto) -> Result<(), String> + Send + Sync>;
type ActivitySink = Arc<dyn Fn(AgentActivityEventDto) -> Result<(), String> + Send + Sync>;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AgentActivityKind {
    Thinking,
    ChoosingTool,
    ToolStarted,
    ToolCompleted,
    ToolFailed,
    Writing,
    AnswerReset,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentActivityEventDto {
    pub kind: AgentActivityKind,
    pub run_id: String,
    pub conversation_id: String,
    pub assistant_message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

trait AgentStrategy: Send {
    fn mode(&self) -> AgentMode;
    fn decide(&self, output: &str) -> Result<AgentDecision, String>;
}

struct ChatStrategy;

impl AgentStrategy for ChatStrategy {
    fn mode(&self) -> AgentMode {
        AgentMode::Chat
    }

    fn decide(&self, output: &str) -> Result<AgentDecision, String> {
        Ok(AgentDecision::Final {
            content: output.into(),
        })
    }
}

struct ReactStrategy;

impl AgentStrategy for ReactStrategy {
    fn mode(&self) -> AgentMode {
        AgentMode::React
    }

    fn decide(&self, output: &str) -> Result<AgentDecision, String> {
        parse_react_decision(output)
    }
}

fn strategy_for(mode: AgentMode) -> Box<dyn AgentStrategy> {
    match mode {
        AgentMode::Chat => Box::new(ChatStrategy),
        AgentMode::React => Box::new(ReactStrategy),
    }
}

struct AgentRunLoop {
    strategy: Box<dyn AgentStrategy>,
    total_steps: u32,
    progress_steps: u32,
    total_tool_calls: u32,
    max_total_steps: u32,
    max_progress_steps: u32,
    max_tool_calls: u32,
    max_repeated_actions: u32,
    repeated_actions: HashMap<String, u32>,
    progress_results: HashSet<String>,
}

impl AgentRunLoop {
    fn for_mode(mode: AgentMode) -> Self {
        let (max_total_steps, max_progress_steps, max_tool_calls, max_repeated_actions) = match mode
        {
            AgentMode::Chat => (1, 1, 0, 0),
            AgentMode::React => (16, 6, 10, 2),
        };
        Self {
            strategy: strategy_for(mode),
            total_steps: 0,
            progress_steps: 0,
            total_tool_calls: 0,
            max_total_steps,
            max_progress_steps,
            max_tool_calls,
            max_repeated_actions,
            repeated_actions: HashMap::new(),
            progress_results: HashSet::new(),
        }
    }

    fn mode(&self) -> AgentMode {
        self.strategy.mode()
    }

    fn before_model(&mut self) -> Result<(), String> {
        if self.total_steps >= self.max_total_steps {
            return Err("ReAct가 전체 단계 제한에 도달했습니다.".into());
        }
        if self.progress_steps >= self.max_progress_steps {
            return Err("ReAct가 새 결과 없이 반복되어 중단했습니다.".into());
        }
        self.total_steps += 1;
        self.progress_steps += 1;
        Ok(())
    }

    fn decide(&self, output: &str) -> Result<AgentDecision, String> {
        self.strategy.decide(output)
    }

    fn before_tool(&mut self, action_digest: &str) -> Result<(), String> {
        if self.total_tool_calls >= self.max_tool_calls {
            return Err("ReAct가 도구 호출 제한에 도달했습니다.".into());
        }
        self.total_tool_calls += 1;
        let repeats = self
            .repeated_actions
            .entry(action_digest.into())
            .or_insert(0);
        *repeats += 1;
        if *repeats > self.max_repeated_actions {
            return Err("ReAct가 같은 도구 요청을 반복해 중단했습니다.".into());
        }
        Ok(())
    }

    fn record_tool_result(&mut self, result: &ToolResult) -> bool {
        let progress_key = format!("{}:{}", result.action_digest, result.result_digest);
        let novel_success = result.successful && self.progress_results.insert(progress_key);
        if novel_success {
            self.progress_steps = 0;
        }
        novel_success
    }
}

struct ActiveRun {
    run_id: String,
    conversation_id: String,
    assistant_message_id: String,
    workspace_path: String,
    step_id: String,
    output: Vec<u8>,
    public_answer: String,
    public_phase: Option<AgentActivityKind>,
    run_loop: AgentRunLoop,
    request_template: SubmitRequest,
    messages: Vec<SubmitChatMessage>,
    runtime: RuntimeHost,
    public_request_handle: Option<String>,
    current_request_handle: Option<String>,
    parse_repairs: u32,
    cancel_requested: bool,
}

impl ActiveRun {
    fn public_handle(&self, fallback: Option<&str>) -> Option<String> {
        self.public_request_handle
            .clone()
            .or_else(|| fallback.map(str::to_string))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ReactProjection {
    Pending,
    ToolCall,
    Final(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrefixMatch {
    Complete,
    Pending,
    Invalid,
}

fn match_literal(bytes: &[u8], cursor: usize, literal: &[u8]) -> PrefixMatch {
    let remaining = &bytes[cursor..];
    if remaining.len() >= literal.len() {
        if &remaining[..literal.len()] == literal {
            PrefixMatch::Complete
        } else {
            PrefixMatch::Invalid
        }
    } else if literal.starts_with(remaining) {
        PrefixMatch::Pending
    } else {
        PrefixMatch::Invalid
    }
}

fn consume_literal(bytes: &[u8], cursor: &mut usize, literal: &[u8]) -> PrefixMatch {
    let result = match_literal(bytes, *cursor, literal);
    if result == PrefixMatch::Complete {
        *cursor += literal.len();
    }
    result
}

fn skip_json_whitespace(bytes: &[u8], cursor: &mut usize) {
    while bytes
        .get(*cursor)
        .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
    {
        *cursor += 1;
    }
}

fn decode_hex_quad(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 4 {
        return None;
    }
    bytes[..4].iter().try_fold(0u16, |value, byte| {
        let digit = match byte {
            b'0'..=b'9' => u16::from(*byte - b'0'),
            b'a'..=b'f' => u16::from(*byte - b'a' + 10),
            b'A'..=b'F' => u16::from(*byte - b'A' + 10),
            _ => return None,
        };
        Some(value * 16 + digit)
    })
}

fn decode_json_string_prefix(bytes: &[u8]) -> Option<String> {
    let mut decoded = String::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' => return Some(decoded),
            b'\\' => {
                let escaped = *bytes.get(cursor + 1)?;
                match escaped {
                    b'"' => decoded.push('"'),
                    b'\\' => decoded.push('\\'),
                    b'/' => decoded.push('/'),
                    b'b' => decoded.push('\u{0008}'),
                    b'f' => decoded.push('\u{000c}'),
                    b'n' => decoded.push('\n'),
                    b'r' => decoded.push('\r'),
                    b't' => decoded.push('\t'),
                    b'u' => {
                        let first = decode_hex_quad(bytes.get(cursor + 2..)?)?;
                        cursor += 6;
                        let codepoint = if (0xd800..=0xdbff).contains(&first) {
                            if bytes.get(cursor..cursor + 2) != Some(b"\\u") {
                                return None;
                            }
                            let second = decode_hex_quad(bytes.get(cursor + 2..)?)?;
                            if !(0xdc00..=0xdfff).contains(&second) {
                                return None;
                            }
                            cursor += 6;
                            0x10000
                                + ((u32::from(first) - 0xd800) << 10)
                                + (u32::from(second) - 0xdc00)
                        } else {
                            u32::from(first)
                        };
                        decoded.push(char::from_u32(codepoint)?);
                        continue;
                    }
                    _ => return None,
                }
                cursor += 2;
            }
            byte if byte < 0x20 => return None,
            _ => {
                let start = cursor;
                while cursor < bytes.len()
                    && !matches!(bytes[cursor], b'"' | b'\\')
                    && bytes[cursor] >= 0x20
                {
                    cursor += 1;
                }
                match std::str::from_utf8(&bytes[start..cursor]) {
                    Ok(value) => decoded.push_str(value),
                    Err(error) if error.error_len().is_none() => {
                        let valid = &bytes[start..start + error.valid_up_to()];
                        decoded.push_str(std::str::from_utf8(valid).ok()?);
                        return Some(decoded);
                    }
                    Err(_) => return None,
                }
            }
        }
    }
    Some(decoded)
}

fn project_react_output(bytes: &[u8]) -> ReactProjection {
    let mut cursor = 0;
    let required = [b"{".as_slice(), br#""type""#.as_slice(), b":".as_slice()];
    for literal in required {
        skip_json_whitespace(bytes, &mut cursor);
        match consume_literal(bytes, &mut cursor, literal) {
            PrefixMatch::Complete => {}
            PrefixMatch::Pending | PrefixMatch::Invalid => return ReactProjection::Pending,
        }
    }
    skip_json_whitespace(bytes, &mut cursor);
    match match_literal(bytes, cursor, br#""tool_call""#) {
        PrefixMatch::Complete => return ReactProjection::ToolCall,
        PrefixMatch::Pending => return ReactProjection::Pending,
        PrefixMatch::Invalid => {}
    }
    match consume_literal(bytes, &mut cursor, br#""final""#) {
        PrefixMatch::Complete => {}
        PrefixMatch::Pending | PrefixMatch::Invalid => return ReactProjection::Pending,
    }
    for literal in [
        b",".as_slice(),
        br#""content""#.as_slice(),
        b":".as_slice(),
        b"\"".as_slice(),
    ] {
        skip_json_whitespace(bytes, &mut cursor);
        match consume_literal(bytes, &mut cursor, literal) {
            PrefixMatch::Complete => {}
            PrefixMatch::Pending | PrefixMatch::Invalid => return ReactProjection::Pending,
        }
    }
    decode_json_string_prefix(&bytes[cursor..])
        .map(ReactProjection::Final)
        .unwrap_or(ReactProjection::Pending)
}

struct AgentControllerInner {
    store: ConversationStore,
    tool_gateway: ToolGateway,
    sink: EventSink,
    activity_sink: ActivitySink,
    active: Mutex<HashMap<u64, ActiveRun>>,
}

#[derive(Clone)]
pub struct AgentController {
    inner: Arc<AgentControllerInner>,
}

impl AgentController {
    pub fn for_app(store: ConversationStore, app: AppHandle) -> Self {
        let event_app = app.clone();
        Self::new_with_activity(
            store,
            move |event| {
                event_app
                    .emit("llm://event", event)
                    .map_err(|error| error.to_string())
            },
            move |event| {
                app.emit("agent://activity", event)
                    .map_err(|error| error.to_string())
            },
        )
    }

    #[cfg(test)]
    fn new<F>(store: ConversationStore, sink: F) -> Self
    where
        F: Fn(LlmEventDto) -> Result<(), String> + Send + Sync + 'static,
    {
        Self::new_with_activity(store, sink, |_| Ok(()))
    }

    fn new_with_activity<F, A>(store: ConversationStore, sink: F, activity_sink: A) -> Self
    where
        F: Fn(LlmEventDto) -> Result<(), String> + Send + Sync + 'static,
        A: Fn(AgentActivityEventDto) -> Result<(), String> + Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(AgentControllerInner {
                store,
                tool_gateway: ToolGateway::builtin(),
                sink: Arc::new(sink),
                activity_sink: Arc::new(activity_sink),
                active: Mutex::new(HashMap::new()),
            }),
        }
    }

    fn activity(
        run: &ActiveRun,
        kind: AgentActivityKind,
        activity_id: Option<String>,
        tool_name: Option<String>,
        input: Option<String>,
        output: Option<String>,
        duration_ms: Option<u64>,
    ) -> AgentActivityEventDto {
        AgentActivityEventDto {
            kind,
            run_id: run.run_id.clone(),
            conversation_id: run.conversation_id.clone(),
            assistant_message_id: run.assistant_message_id.clone(),
            activity_id,
            tool_name,
            input,
            output,
            duration_ms,
        }
    }

    pub fn submit(
        &self,
        runtime: &RuntimeHost,
        mut request: SubmitRequest,
    ) -> Result<SubmitResponse, String> {
        let run_id = request
            .agent_run_id
            .as_deref()
            .ok_or_else(|| "agentRunId is required".to_string())?;
        let step_id = request
            .agent_step_id
            .as_deref()
            .ok_or_else(|| "agentStepId is required".to_string())?;
        let submission =
            self.inner
                .store
                .agent_submission(run_id, step_id, &request.conversation_id)?;
        let mode = AgentMode::parse(&submission.mode)?;
        let mut run_loop = AgentRunLoop::for_mode(mode);
        run_loop.before_model()?;
        if mode == AgentMode::React {
            apply_agent_protocol(mode, &mut request.messages, &submission.workspace_path);
        }
        apply_agent_output_constraint(mode, &mut request);
        request.correlation_id = submission.correlation_id;

        {
            let mut active = self
                .inner
                .active
                .lock()
                .map_err(|_| "agent controller lock is poisoned")?;
            if active.contains_key(&submission.correlation_id) {
                return Err("agent correlation id is already active".into());
            }
            active.insert(
                submission.correlation_id,
                ActiveRun {
                    run_id: submission.run_id.clone(),
                    conversation_id: submission.conversation_id.clone(),
                    assistant_message_id: submission.assistant_message_id.clone(),
                    workspace_path: submission.workspace_path.clone(),
                    step_id: submission.step_id.clone(),
                    output: Vec::new(),
                    public_answer: String::new(),
                    public_phase: None,
                    run_loop,
                    request_template: request.clone(),
                    messages: request.messages.clone(),
                    runtime: runtime.clone(),
                    public_request_handle: None,
                    current_request_handle: None,
                    parse_repairs: 0,
                    cancel_requested: false,
                },
            );
        }
        if mode == AgentMode::React {
            let activity = {
                let mut active = self
                    .inner
                    .active
                    .lock()
                    .map_err(|_| "agent controller lock is poisoned")?;
                active.get_mut(&submission.correlation_id).map(|run| {
                    run.public_phase = Some(AgentActivityKind::Thinking);
                    Self::activity(
                        run,
                        AgentActivityKind::Thinking,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                })
            };
            if let Some(activity) = activity {
                (self.inner.activity_sink)(activity)?;
            }
        }

        let response = match runtime.submit(request) {
            Ok(response) => response,
            Err(error) => {
                self.remove_active(submission.correlation_id);
                let _ = self.inner.store.finish_agent_step(
                    submission.correlation_id,
                    &error,
                    MessageStatus::Error,
                    Some("model-submit-failed"),
                );
                return Err(error);
            }
        };
        if let Err(error) = self.inner.store.bind_agent_request(
            &submission.run_id,
            &submission.step_id,
            &response.request_handle,
        ) {
            if let Ok(handle) = response.request_handle.parse::<u64>() {
                let _ = runtime.cancel(handle);
            }
            self.remove_active(submission.correlation_id);
            let _ = self.inner.store.finish_agent_run(
                &submission.run_id,
                &error,
                MessageStatus::Error,
                "request-bind-failed",
            );
            return Err(error);
        }
        if let Ok(mut active) = self.inner.active.lock() {
            if let Some(run) = active.get_mut(&submission.correlation_id) {
                run.public_request_handle
                    .get_or_insert_with(|| response.request_handle.clone());
                run.current_request_handle = Some(response.request_handle.clone());
            }
        }
        Ok(response)
    }

    pub fn cancel(&self, runtime: &RuntimeHost, public_handle: u64) -> Result<(), String> {
        let public_handle = public_handle.to_string();
        let current = {
            let mut active = self
                .inner
                .active
                .lock()
                .map_err(|_| "agent controller lock is poisoned")?;
            let run = active
                .values_mut()
                .find(|run| {
                    run.public_request_handle.as_deref() == Some(public_handle.as_str())
                        || run.current_request_handle.as_deref() == Some(public_handle.as_str())
                })
                .ok_or_else(|| "active agent run was not found".to_string())?;
            run.cancel_requested = true;
            run.current_request_handle.clone()
        };
        match current {
            Some(handle) => runtime.cancel(
                handle
                    .parse::<u64>()
                    .map_err(|_| "agent request handle is invalid".to_string())?,
            ),
            None => Ok(()),
        }
    }

    pub fn route_event(&self, event: LlmEventDto) -> Result<(), String> {
        let Some(correlation_id) = event
            .correlation_id
            .as_deref()
            .map(str::parse::<u64>)
            .transpose()
            .map_err(|_| "runtime emitted an invalid agent correlation id")?
        else {
            return (self.inner.sink)(event);
        };
        let mode = {
            let active = self
                .inner
                .active
                .lock()
                .map_err(|_| "agent controller lock is poisoned")?;
            active.get(&correlation_id).map(|run| run.run_loop.mode())
        };
        match mode {
            Some(AgentMode::Chat) => self.route_chat_event(correlation_id, event),
            Some(AgentMode::React) => self.route_react_event(correlation_id, event),
            None => Ok(()),
        }
    }

    fn route_chat_event(&self, correlation_id: u64, mut event: LlmEventDto) -> Result<(), String> {
        let terminal = is_terminal(event.kind);
        let mut finished = None;
        {
            let mut active = self
                .inner
                .active
                .lock()
                .map_err(|_| "agent controller lock is poisoned")?;
            let Some(run) = active.get_mut(&correlation_id) else {
                return Ok(());
            };
            if event.kind == LlmEventKind::Queued {
                run.current_request_handle = event.request_handle.clone();
                if run.public_request_handle.is_none() {
                    run.public_request_handle = event.request_handle.clone();
                }
            }
            if event.kind == LlmEventKind::Token {
                run.output.extend_from_slice(&event.bytes);
            }
            if terminal {
                finished = active.remove(&correlation_id);
            }
        }

        if let Some(run) = finished {
            let status = terminal_status(event.kind);
            let mut content = run.output;
            if status == MessageStatus::Error && content.is_empty() {
                content.extend_from_slice(&event.bytes);
            }
            let content = String::from_utf8_lossy(&content).into_owned();
            let reason = terminal_reason(status);
            if let Err(error) =
                self.inner
                    .store
                    .finish_agent_step(correlation_id, &content, status, Some(reason))
            {
                event.kind = LlmEventKind::Error;
                event.error_code = -1;
                event.bytes = format!("agent persistence failed: {error}").into_bytes();
            }
        }
        (self.inner.sink)(event)
    }

    fn route_react_event(&self, correlation_id: u64, mut event: LlmEventDto) -> Result<(), String> {
        let terminal = is_terminal(event.kind);
        let mut finished = None;
        let mut emit_queued = false;
        let mut activities = Vec::new();
        let mut projected_token = None;
        {
            let mut active = self
                .inner
                .active
                .lock()
                .map_err(|_| "agent controller lock is poisoned")?;
            let Some(run) = active.get_mut(&correlation_id) else {
                return Ok(());
            };
            if event.kind == LlmEventKind::Queued {
                run.current_request_handle = event.request_handle.clone();
                emit_queued = run.public_request_handle.is_none();
                if run.public_request_handle.is_none() {
                    run.public_request_handle = event.request_handle.clone();
                }
                event.request_handle = run.public_handle(event.request_handle.as_deref());
            }
            if event.kind == LlmEventKind::Token {
                run.output.extend_from_slice(&event.bytes);
                match project_react_output(&run.output) {
                    ReactProjection::Pending => {}
                    ReactProjection::ToolCall => {
                        if run.public_phase != Some(AgentActivityKind::ChoosingTool) {
                            run.public_phase = Some(AgentActivityKind::ChoosingTool);
                            activities.push(Self::activity(
                                run,
                                AgentActivityKind::ChoosingTool,
                                None,
                                None,
                                None,
                                None,
                                None,
                            ));
                        }
                    }
                    ReactProjection::Final(content) => {
                        if run.public_phase != Some(AgentActivityKind::Writing) {
                            run.public_phase = Some(AgentActivityKind::Writing);
                            activities.push(Self::activity(
                                run,
                                AgentActivityKind::Writing,
                                None,
                                None,
                                None,
                                None,
                                None,
                            ));
                        }
                        if !content.starts_with(&run.public_answer) {
                            run.public_answer.clear();
                            activities.push(Self::activity(
                                run,
                                AgentActivityKind::AnswerReset,
                                None,
                                None,
                                None,
                                None,
                                None,
                            ));
                        }
                        let delta = content[run.public_answer.len()..].to_string();
                        if !delta.is_empty() {
                            run.public_answer.push_str(&delta);
                            projected_token = Some(LlmEventDto {
                                kind: LlmEventKind::Token,
                                request_handle: run.public_handle(event.request_handle.as_deref()),
                                correlation_id: event.correlation_id.clone(),
                                sequence_number: event.sequence_number.clone(),
                                bytes: delta.into_bytes(),
                                error_code: 0,
                                metrics: None,
                            });
                        }
                    }
                }
            }
            if terminal {
                finished = active.remove(&correlation_id);
            }
        }

        for activity in activities {
            (self.inner.activity_sink)(activity)?;
        }
        if emit_queued {
            return (self.inner.sink)(event);
        }
        if let Some(token) = projected_token {
            (self.inner.sink)(token)?;
        }
        if !terminal {
            return Ok(());
        }
        let Some(run) = finished else {
            return Ok(());
        };
        if event.kind != LlmEventKind::Done {
            return self.finish_react_error(correlation_id, run, event);
        }
        let output = String::from_utf8_lossy(&run.output).into_owned();
        match run.run_loop.decide(&output) {
            Ok(AgentDecision::Final { content }) => {
                self.finish_react_final(correlation_id, run, event, output, content)
            }
            Ok(AgentDecision::ToolCall { name, arguments }) => {
                self.continue_after_tool(correlation_id, run, output, name, arguments)
            }
            Err(error) if run.parse_repairs == 0 => {
                self.continue_after_parse_error(correlation_id, run, output, error)
            }
            Err(error) => {
                event.kind = LlmEventKind::Error;
                event.error_code = -1;
                event.bytes =
                    format!("ReAct 응답 형식을 해석하지 못했습니다: {error}").into_bytes();
                self.finish_react_error(correlation_id, run, event)
            }
        }
    }

    fn finish_react_final(
        &self,
        correlation_id: u64,
        run: ActiveRun,
        mut terminal: LlmEventDto,
        model_output: String,
        content: String,
    ) -> Result<(), String> {
        let public_handle = run.public_handle(terminal.request_handle.as_deref());
        let decision_json = json!({
            "type": "final",
            "content": content.clone(),
        })
        .to_string();
        if let Err(error) = self.inner.store.finish_agent_final_decision(
            correlation_id,
            &model_output,
            &decision_json,
            &content,
        ) {
            terminal.kind = LlmEventKind::Error;
            terminal.request_handle = public_handle;
            terminal.error_code = -1;
            terminal.bytes = format!("agent persistence failed: {error}").into_bytes();
            let _ = (self.inner.activity_sink)(Self::activity(
                &run,
                AgentActivityKind::Failed,
                None,
                None,
                None,
                None,
                None,
            ));
            return (self.inner.sink)(terminal);
        }
        let (reset, tail) = if content.starts_with(&run.public_answer) {
            (false, content[run.public_answer.len()..].to_string())
        } else {
            (true, content)
        };
        if reset {
            (self.inner.activity_sink)(Self::activity(
                &run,
                AgentActivityKind::AnswerReset,
                None,
                None,
                None,
                None,
                None,
            ))?;
        }
        if !tail.is_empty() {
            (self.inner.sink)(LlmEventDto {
                kind: LlmEventKind::Token,
                request_handle: public_handle.clone(),
                correlation_id: terminal.correlation_id.clone(),
                sequence_number: terminal.sequence_number.clone(),
                bytes: tail.into_bytes(),
                error_code: 0,
                metrics: None,
            })?;
        }
        (self.inner.activity_sink)(Self::activity(
            &run,
            AgentActivityKind::Completed,
            None,
            None,
            None,
            None,
            None,
        ))?;
        terminal.request_handle = public_handle;
        terminal.bytes.clear();
        (self.inner.sink)(terminal)
    }

    fn finish_react_error(
        &self,
        correlation_id: u64,
        run: ActiveRun,
        mut event: LlmEventDto,
    ) -> Result<(), String> {
        let status = terminal_status(event.kind);
        let public_handle = run.public_handle(event.request_handle.as_deref());
        let content = if status == MessageStatus::Error {
            String::from_utf8_lossy(&event.bytes).into_owned()
        } else {
            run.public_answer.clone()
        };
        if let Err(error) = self.inner.store.finish_agent_step(
            correlation_id,
            &content,
            status,
            Some(terminal_reason(status)),
        ) {
            event.kind = LlmEventKind::Error;
            event.error_code = -1;
            event.bytes = format!("agent persistence failed: {error}").into_bytes();
        }
        let activity_kind = match status {
            MessageStatus::Cancelled => AgentActivityKind::Cancelled,
            MessageStatus::Error => AgentActivityKind::Failed,
            _ => AgentActivityKind::Completed,
        };
        (self.inner.activity_sink)(Self::activity(
            &run,
            activity_kind,
            None,
            None,
            None,
            None,
            None,
        ))?;
        event.request_handle = public_handle;
        (self.inner.sink)(event)
    }

    fn continue_after_tool(
        &self,
        correlation_id: u64,
        mut run: ActiveRun,
        output: String,
        name: String,
        arguments: Value,
    ) -> Result<(), String> {
        let decision = json!({
            "type": "tool_call",
            "name": name,
            "arguments": arguments,
        });
        if let Err(error) = self.inner.store.complete_agent_model_step(
            correlation_id,
            &output,
            &decision.to_string(),
            "tool-call",
        ) {
            return self.fail_between_steps(run, error);
        }
        let tool_context = ToolContext::for_workspace(run.workspace_path.clone());
        let preparation = self
            .inner
            .tool_gateway
            .prepare(&name, &arguments, &tool_context);
        let action_digest = match &preparation {
            ToolPreparation::Ready(call) => call.action_digest(),
            ToolPreparation::ApprovalRequired(request) => &request.action_digest,
            ToolPreparation::Rejected(result) => &result.action_digest,
        }
        .to_string();
        if let Err(error) = run.run_loop.before_tool(&action_digest) {
            return self.fail_between_steps(run, error);
        }
        let activity_id = format!("{}:tool:{}", run.run_id, run.run_loop.total_tool_calls);
        let input = tool_input_label(&name, &arguments);
        if let Err(error) = (self.inner.activity_sink)(Self::activity(
            &run,
            AgentActivityKind::ToolStarted,
            Some(activity_id.clone()),
            Some(name.clone()),
            Some(input),
            None,
            None,
        )) {
            return self.fail_between_steps(run, error);
        }
        let started = Instant::now();
        let result = match preparation {
            ToolPreparation::Ready(call) => self.inner.tool_gateway.execute(call),
            ToolPreparation::ApprovalRequired(request) => request.into_blocked_result(),
            ToolPreparation::Rejected(result) => result,
        };
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let novel_success = run.run_loop.record_tool_result(&result);
        if let Err(error) = self.inner.store.record_agent_tool_step(
            &run.run_id,
            &name,
            &arguments.to_string(),
            &result.display_content,
            result.successful,
            novel_success,
            duration_ms,
        ) {
            return self.fail_between_steps(run, error);
        }
        let tool_kind = if result.successful {
            AgentActivityKind::ToolCompleted
        } else {
            AgentActivityKind::ToolFailed
        };
        if let Err(error) = (self.inner.activity_sink)(Self::activity(
            &run,
            tool_kind,
            Some(activity_id),
            Some(name.clone()),
            None,
            Some(result.display_content.clone()),
            Some(duration_ms),
        )) {
            return self.fail_between_steps(run, error);
        }
        run.messages.push(SubmitChatMessage {
            role: "assistant".into(),
            content: decision.to_string(),
        });
        run.messages.push(SubmitChatMessage {
            role: "user".into(),
            content: format!(
                "Tool observation for `{name}`:\n{}\n\nReturn the next ReAct JSON decision.",
                result.model_content
            ),
        });
        self.schedule_follow_up(run)
    }

    fn continue_after_parse_error(
        &self,
        correlation_id: u64,
        mut run: ActiveRun,
        output: String,
        error: String,
    ) -> Result<(), String> {
        if let Err(persistence_error) = self.inner.store.complete_agent_model_step(
            correlation_id,
            &output,
            &json!({ "type": "parse_error", "error": error }).to_string(),
            "parse-repair",
        ) {
            return self.fail_between_steps(run, persistence_error);
        }
        if !run.public_answer.is_empty() {
            if let Err(activity_error) = (self.inner.activity_sink)(Self::activity(
                &run,
                AgentActivityKind::AnswerReset,
                None,
                None,
                None,
                None,
                None,
            )) {
                return self.fail_between_steps(run, activity_error);
            }
        }
        run.parse_repairs += 1;
        run.messages.push(SubmitChatMessage {
            role: "assistant".into(),
            content: output,
        });
        run.messages.push(SubmitChatMessage {
            role: "user".into(),
            content: format!(
                "The previous response was invalid: {error}\nReturn exactly one JSON object matching the ReAct contract."
            ),
        });
        self.schedule_follow_up(run)
    }

    fn schedule_follow_up(&self, mut run: ActiveRun) -> Result<(), String> {
        if let Err(error) = run.run_loop.before_model() {
            return self.fail_between_steps(run, error);
        }
        let prepared = match self
            .inner
            .store
            .prepare_agent_model_step(&run.run_id, "react-decision")
        {
            Ok(prepared) => prepared,
            Err(error) => return self.fail_between_steps(run, error),
        };
        run.step_id = prepared.step_id.clone();
        run.output.clear();
        run.public_answer.clear();
        run.current_request_handle = None;
        run.public_phase = Some(AgentActivityKind::Thinking);
        (self.inner.activity_sink)(Self::activity(
            &run,
            AgentActivityKind::Thinking,
            None,
            None,
            None,
            None,
            None,
        ))?;
        let mut request = run.request_template.clone();
        request.agent_run_id = Some(prepared.run_id.clone());
        request.agent_step_id = Some(prepared.step_id.clone());
        request.correlation_id = prepared.correlation_id;
        request.messages = run.messages.clone();
        request.prompt = run
            .messages
            .last()
            .map(|message| message.content.clone())
            .unwrap_or_default();
        let runtime = run.runtime.clone();
        {
            let mut active = self
                .inner
                .active
                .lock()
                .map_err(|_| "agent controller lock is poisoned")?;
            active.insert(prepared.correlation_id, run);
        }
        let controller = self.clone();
        let correlation_id = prepared.correlation_id;
        if let Err(error) = thread::Builder::new()
            .name("react-follow-up".into())
            .spawn(move || controller.submit_follow_up(runtime, prepared, request))
        {
            self.fail_follow_up(
                correlation_id,
                format!("failed to schedule ReAct follow-up: {error}"),
            );
        }
        Ok(())
    }

    fn submit_follow_up(
        &self,
        runtime: RuntimeHost,
        prepared: PreparedAgentStep,
        request: SubmitRequest,
    ) {
        match runtime.submit(request) {
            Ok(response) => {
                let bind = self.inner.store.bind_agent_request(
                    &prepared.run_id,
                    &prepared.step_id,
                    &response.request_handle,
                );
                let cancel = if let Ok(mut active) = self.inner.active.lock() {
                    active.get_mut(&prepared.correlation_id).map(|run| {
                        run.current_request_handle = Some(response.request_handle.clone());
                        run.cancel_requested
                    })
                } else {
                    None
                };
                if let Err(error) = bind {
                    if let Ok(handle) = response.request_handle.parse::<u64>() {
                        let _ = runtime.cancel(handle);
                    }
                    self.fail_follow_up(prepared.correlation_id, error);
                } else if cancel == Some(true) {
                    if let Ok(handle) = response.request_handle.parse::<u64>() {
                        let _ = runtime.cancel(handle);
                    }
                }
            }
            Err(error) => self.fail_follow_up(prepared.correlation_id, error),
        }
    }

    fn fail_follow_up(&self, correlation_id: u64, error: String) {
        let run = self.remove_active(correlation_id);
        let _ = self.inner.store.finish_agent_step(
            correlation_id,
            &error,
            MessageStatus::Error,
            Some("model-submit-failed"),
        );
        if let Some(run) = run {
            let _ = (self.inner.activity_sink)(Self::activity(
                &run,
                AgentActivityKind::Failed,
                None,
                None,
                None,
                None,
                None,
            ));
            let _ = (self.inner.sink)(LlmEventDto {
                kind: LlmEventKind::Error,
                request_handle: run.public_request_handle,
                correlation_id: Some(correlation_id.to_string()),
                sequence_number: "0".into(),
                bytes: error.into_bytes(),
                error_code: -1,
                metrics: None,
            });
        }
    }

    fn fail_between_steps(&self, run: ActiveRun, error: String) -> Result<(), String> {
        let persistence_error = self
            .inner
            .store
            .finish_agent_run(&run.run_id, &error, MessageStatus::Error, "strategy-limit")
            .err();
        let error = persistence_error
            .map(|persistence| format!("{error}\nagent persistence failed: {persistence}"))
            .unwrap_or(error);
        (self.inner.activity_sink)(Self::activity(
            &run,
            AgentActivityKind::Failed,
            None,
            None,
            None,
            None,
            None,
        ))?;
        (self.inner.sink)(LlmEventDto {
            kind: LlmEventKind::Error,
            request_handle: run.public_request_handle,
            correlation_id: None,
            sequence_number: "0".into(),
            bytes: error.into_bytes(),
            error_code: -1,
            metrics: None,
        })
    }

    fn remove_active(&self, correlation_id: u64) -> Option<ActiveRun> {
        self.inner
            .active
            .lock()
            .ok()
            .and_then(|mut active| active.remove(&correlation_id))
    }
}

fn apply_agent_protocol(
    mode: AgentMode,
    messages: &mut Vec<SubmitChatMessage>,
    workspace_path: &str,
) {
    if let Some(system) = messages.iter_mut().find(|message| message.role == "system") {
        system.content = compile_agent_runtime_system_prompt(mode, &system.content, workspace_path);
    } else {
        let content = compile_agent_runtime_system_prompt(mode, "", workspace_path);
        if content.is_empty() {
            return;
        }
        messages.insert(
            0,
            SubmitChatMessage {
                role: "system".into(),
                content,
            },
        );
    }
}

fn tool_input_label(name: &str, arguments: &Value) -> String {
    match name {
        "calculator" => arguments
            .get("expression")
            .and_then(Value::as_str)
            .map(str::to_string),
        "search_files" => arguments.get("query").and_then(Value::as_str).map(|query| {
            let path = arguments.get("path").and_then(Value::as_str).unwrap_or(".");
            format!("{query} · {path}")
        }),
        _ => arguments
            .get("path")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
    .unwrap_or_else(|| arguments.to_string())
}

fn apply_agent_output_constraint(mode: AgentMode, request: &mut SubmitRequest) {
    request.output_grammar = mode.output_grammar().map(str::to_owned);
}

fn is_terminal(kind: LlmEventKind) -> bool {
    matches!(
        kind,
        LlmEventKind::Done | LlmEventKind::Cancelled | LlmEventKind::Error
    )
}

fn terminal_status(kind: LlmEventKind) -> MessageStatus {
    match kind {
        LlmEventKind::Done => MessageStatus::Complete,
        LlmEventKind::Cancelled => MessageStatus::Cancelled,
        LlmEventKind::Error => MessageStatus::Error,
        _ => unreachable!("terminal status requires a terminal event"),
    }
}

fn terminal_reason(status: MessageStatus) -> &'static str {
    match status {
        MessageStatus::Complete => "strategy-complete",
        MessageStatus::Cancelled => "user-cancelled",
        MessageStatus::Error => "model-error",
        _ => unreachable!("agent terminal status is exhaustive"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use super::{
        apply_agent_output_constraint, apply_agent_protocol, project_react_output,
        AgentActivityKind, AgentController, AgentMode, AgentRunLoop, AgentStrategy, ChatStrategy,
        ReactProjection,
    };
    use crate::{
        agent_mode::AgentDecision,
        agent_tools::{ToolContext, ToolGateway, ToolPreparation},
        conversation_store::{ConversationStore, MessageStatus},
        llm_dto::{LlmEventDto, LlmEventKind, SubmitChatMessage},
    };

    #[test]
    fn chat_strategy_is_exactly_one_model_step() {
        let mut run_loop = AgentRunLoop::for_mode(AgentMode::Chat);
        run_loop.before_model().unwrap();
        assert!(run_loop.before_model().is_err());
        assert_eq!(
            ChatStrategy.decide("answer").unwrap(),
            AgentDecision::Final {
                content: "answer".into()
            }
        );
    }

    #[test]
    fn react_success_resets_only_novel_progress() {
        let mut run_loop = AgentRunLoop::for_mode(AgentMode::React);
        run_loop.before_model().unwrap();
        let gateway = ToolGateway::builtin();
        let ToolPreparation::Ready(call) = gateway.prepare(
            "calculator",
            &json!({ "expression": "2 + 2" }),
            &ToolContext::default(),
        ) else {
            panic!("calculator must be prepared");
        };
        let result = gateway.execute(call);
        run_loop.before_tool(&result.action_digest).unwrap();
        assert!(run_loop.record_tool_result(&result));
        assert_eq!(run_loop.progress_steps, 0);
        run_loop.before_model().unwrap();
        run_loop.before_tool(&result.action_digest).unwrap();
        assert!(!run_loop.record_tool_result(&result));
        assert_eq!(run_loop.progress_steps, 1);
        assert!(run_loop.before_tool(&result.action_digest).is_err());
    }

    #[test]
    fn react_protocol_is_a_managed_system_layer() {
        let mut messages = vec![SubmitChatMessage {
            role: "user".into(),
            content: "question".into(),
        }];
        apply_agent_protocol(AgentMode::React, &mut messages, "/workspace");
        assert_eq!(messages[0].role, "system");
        assert!(messages[0].content.contains("\"type\":\"tool_call\""));
        assert!(messages[0].content.contains("calculator"));
        assert!(messages[0].content.contains("/workspace"));
    }

    #[test]
    fn structured_output_is_applied_only_to_react_requests() {
        let mut request = serde_json::from_value(json!({
            "conversationId": "conversation",
            "prompt": "question",
            "messages": [],
            "maxNewTokens": 64,
            "temperature": 0.7,
            "seed": -1
        }))
        .unwrap();

        apply_agent_output_constraint(AgentMode::React, &mut request);
        assert!(request
            .output_grammar
            .as_deref()
            .is_some_and(|grammar| grammar.contains(r#"\"tool_call\""#)));

        apply_agent_output_constraint(AgentMode::Chat, &mut request);
        assert_eq!(request.output_grammar, None);
    }

    #[test]
    fn react_projection_reveals_only_final_content() {
        assert_eq!(
            project_react_output(br#"{"type":"tool_call","name":"calculator""#),
            ReactProjection::ToolCall
        );
        assert_eq!(
            project_react_output(br#"{"type":"final","content":"line\n"#),
            ReactProjection::Final("line\n".into())
        );

        let korean = r#"{"type":"final","content":"돌쇠입니다."}"#.as_bytes();
        let split = korean
            .windows("돌".len())
            .position(|window| window == "돌".as_bytes())
            .unwrap()
            + 1;
        assert_eq!(
            project_react_output(&korean[..split]),
            ReactProjection::Final(String::new())
        );
        assert_eq!(
            project_react_output(korean),
            ReactProjection::Final("돌쇠입니다.".into())
        );
    }

    #[test]
    fn terminal_chat_event_is_persisted_before_it_is_emitted() {
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
            .bind_agent_request(&submission.run_id, &submission.step_id, "7")
            .unwrap();
        let observed_status = Arc::new(Mutex::new(None));
        let status_for_sink = observed_status.clone();
        let store_for_sink = store.clone();
        let assistant_id = turn.assistant.id.clone();
        let controller = AgentController::new(store.clone(), move |_| {
            let detail = store_for_sink.load_conversation(&turn.conversation.id)?;
            let status = detail
                .messages
                .iter()
                .find(|message| message.id == assistant_id)
                .map(|message| message.status);
            *status_for_sink.lock().unwrap() = status;
            Ok(())
        });
        controller.inner.active.lock().unwrap().insert(
            submission.correlation_id,
            super::ActiveRun {
                run_id: submission.run_id,
                conversation_id: submission.conversation_id,
                assistant_message_id: submission.assistant_message_id,
                workspace_path: submission.workspace_path,
                step_id: submission.step_id,
                output: b"answer".to_vec(),
                public_answer: String::new(),
                public_phase: None,
                run_loop: AgentRunLoop::for_mode(AgentMode::Chat),
                request_template: serde_json::from_value(json!({
                    "conversationId": "conversation",
                    "agentRunId": "run",
                    "agentStepId": "step",
                    "prompt": "question",
                    "messages": [],
                    "maxNewTokens": 8,
                    "temperature": 0.7,
                    "seed": -1
                }))
                .unwrap(),
                messages: vec![],
                runtime: crate::runtime_host::RuntimeHost::recovery("unused"),
                public_request_handle: Some("7".into()),
                current_request_handle: Some("7".into()),
                parse_repairs: 0,
                cancel_requested: false,
            },
        );

        controller
            .route_event(LlmEventDto {
                kind: LlmEventKind::Done,
                request_handle: Some("7".into()),
                correlation_id: Some(submission.correlation_id.to_string()),
                sequence_number: "2".into(),
                bytes: Vec::new(),
                error_code: 0,
                metrics: None,
            })
            .unwrap();

        assert_eq!(
            *observed_status.lock().unwrap(),
            Some(MessageStatus::Complete)
        );
    }

    #[test]
    fn react_emits_only_final_content_to_the_conversation() {
        let store = ConversationStore::open_in_memory().unwrap();
        store.bootstrap().unwrap();
        let turn = store
            .start_new_agent_turn_with_mode("calculate", "react", None)
            .unwrap();
        let submission = store
            .agent_submission(
                turn.agent_run_id.as_deref().unwrap(),
                turn.agent_step_id.as_deref().unwrap(),
                &turn.conversation.id,
            )
            .unwrap();
        store
            .bind_agent_request(&submission.run_id, &submission.step_id, "11")
            .unwrap();
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let emitted_for_sink = emitted.clone();
        let activities = Arc::new(Mutex::new(Vec::new()));
        let activities_for_sink = activities.clone();
        let controller = AgentController::new_with_activity(
            store.clone(),
            move |event| {
                emitted_for_sink.lock().unwrap().push(event);
                Ok(())
            },
            move |event| {
                activities_for_sink.lock().unwrap().push(event);
                Ok(())
            },
        );
        let mut run_loop = AgentRunLoop::for_mode(AgentMode::React);
        run_loop.before_model().unwrap();
        controller.inner.active.lock().unwrap().insert(
            submission.correlation_id,
            super::ActiveRun {
                run_id: submission.run_id,
                conversation_id: submission.conversation_id,
                assistant_message_id: submission.assistant_message_id,
                workspace_path: submission.workspace_path,
                step_id: submission.step_id,
                output: Vec::new(),
                public_answer: String::new(),
                public_phase: None,
                run_loop,
                request_template: serde_json::from_value(json!({
                    "conversationId": turn.conversation.id,
                    "agentRunId": turn.agent_run_id,
                    "agentStepId": turn.agent_step_id,
                    "prompt": "calculate",
                    "messages": [],
                    "maxNewTokens": 8,
                    "temperature": 0.7,
                    "seed": -1
                }))
                .unwrap(),
                messages: vec![],
                runtime: crate::runtime_host::RuntimeHost::recovery("unused"),
                public_request_handle: Some("11".into()),
                current_request_handle: Some("11".into()),
                parse_repairs: 0,
                cancel_requested: false,
            },
        );

        for (sequence, bytes) in [
            br#"{"type":"final","content":"4"#.as_slice(),
            "입니다.\"}".as_bytes(),
        ]
        .into_iter()
        .enumerate()
        {
            controller
                .route_event(LlmEventDto {
                    kind: LlmEventKind::Token,
                    request_handle: Some("11".into()),
                    correlation_id: Some(submission.correlation_id.to_string()),
                    sequence_number: sequence.to_string(),
                    bytes: bytes.to_vec(),
                    error_code: 0,
                    metrics: None,
                })
                .unwrap();
        }
        controller
            .route_event(LlmEventDto {
                kind: LlmEventKind::Done,
                request_handle: Some("11".into()),
                correlation_id: Some(submission.correlation_id.to_string()),
                sequence_number: "9".into(),
                bytes: vec![],
                error_code: 0,
                metrics: None,
            })
            .unwrap();

        let events = emitted.lock().unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].kind, LlmEventKind::Token);
        assert_eq!(String::from_utf8_lossy(&events[0].bytes), "4");
        assert_eq!(String::from_utf8_lossy(&events[1].bytes), "입니다.");
        assert_eq!(events[2].kind, LlmEventKind::Done);
        assert_eq!(
            activities
                .lock()
                .unwrap()
                .iter()
                .map(|event| event.kind)
                .collect::<Vec<_>>(),
            vec![AgentActivityKind::Writing, AgentActivityKind::Completed]
        );
        let detail = store.load_conversation(&turn.conversation.id).unwrap();
        assert_eq!(detail.messages.last().unwrap().content, "4입니다.");
    }
}
