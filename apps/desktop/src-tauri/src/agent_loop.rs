use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    thread,
};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::{
    agent_mode::{compile_agent_system_prompt, parse_react_decision, AgentDecision, AgentMode},
    agent_tools::{execute_read_only_tool, tool_action_digest, ToolResult},
    conversation_store::{ConversationStore, MessageStatus, PreparedAgentStep},
    llm_dto::{LlmEventDto, LlmEventKind, SubmitChatMessage, SubmitRequest, SubmitResponse},
    runtime_host::RuntimeHost,
};

type EventSink = Arc<dyn Fn(LlmEventDto) -> Result<(), String> + Send + Sync>;

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
    step_id: String,
    output: Vec<u8>,
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

struct AgentControllerInner {
    store: ConversationStore,
    sink: EventSink,
    active: Mutex<HashMap<u64, ActiveRun>>,
}

#[derive(Clone)]
pub struct AgentController {
    inner: Arc<AgentControllerInner>,
}

impl AgentController {
    pub fn for_app(store: ConversationStore, app: AppHandle) -> Self {
        Self::new(store, move |event| {
            app.emit("llm://event", event)
                .map_err(|error| error.to_string())
        })
    }

    fn new<F>(store: ConversationStore, sink: F) -> Self
    where
        F: Fn(LlmEventDto) -> Result<(), String> + Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(AgentControllerInner {
                store,
                sink: Arc::new(sink),
                active: Mutex::new(HashMap::new()),
            }),
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
            apply_agent_protocol(mode, &mut request.messages);
        }
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
                    step_id: submission.step_id.clone(),
                    output: Vec::new(),
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
            }
            if terminal {
                finished = active.remove(&correlation_id);
            }
        }

        if emit_queued {
            return (self.inner.sink)(event);
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
            "content": content,
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
            return (self.inner.sink)(terminal);
        }
        let token = LlmEventDto {
            kind: LlmEventKind::Token,
            request_handle: public_handle.clone(),
            correlation_id: terminal.correlation_id.clone(),
            sequence_number: terminal.sequence_number.clone(),
            bytes: content.into_bytes(),
            error_code: 0,
            metrics: None,
        };
        (self.inner.sink)(token)?;
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
            String::new()
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
        let action_digest = tool_action_digest(&name, &arguments);
        if let Err(error) = run.run_loop.before_tool(&action_digest) {
            return self.fail_between_steps(run, error);
        }
        let result = execute_read_only_tool(&name, &arguments);
        let novel_success = run.run_loop.record_tool_result(&result);
        if let Err(error) = self.inner.store.record_agent_tool_step(
            &run.run_id,
            &name,
            &arguments.to_string(),
            &result.model_content,
            result.successful,
            novel_success,
        ) {
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
        run.current_request_handle = None;
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

fn apply_agent_protocol(mode: AgentMode, messages: &mut Vec<SubmitChatMessage>) {
    if let Some(system) = messages.iter_mut().find(|message| message.role == "system") {
        system.content = compile_agent_system_prompt(mode, &system.content);
    } else {
        let content = compile_agent_system_prompt(mode, "");
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
        apply_agent_protocol, AgentController, AgentMode, AgentRunLoop, AgentStrategy, ChatStrategy,
    };
    use crate::{
        agent_mode::AgentDecision,
        agent_tools::execute_read_only_tool,
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
        let result = execute_read_only_tool("calculator", &json!({ "expression": "2 + 2" }));
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
        apply_agent_protocol(AgentMode::React, &mut messages);
        assert_eq!(messages[0].role, "system");
        assert!(messages[0].content.contains("\"type\":\"tool_call\""));
        assert!(messages[0].content.contains("calculator"));
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
                step_id: submission.step_id,
                output: b"answer".to_vec(),
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
        let controller = AgentController::new(store.clone(), move |event| {
            emitted_for_sink.lock().unwrap().push(event);
            Ok(())
        });
        let mut run_loop = AgentRunLoop::for_mode(AgentMode::React);
        run_loop.before_model().unwrap();
        controller.inner.active.lock().unwrap().insert(
            submission.correlation_id,
            super::ActiveRun {
                run_id: submission.run_id,
                step_id: submission.step_id,
                output: r#"{"type":"final","content":"4입니다."}"#.as_bytes().to_vec(),
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
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, LlmEventKind::Token);
        assert_eq!(String::from_utf8_lossy(&events[0].bytes), "4입니다.");
        assert_eq!(events[1].kind, LlmEventKind::Done);
        let detail = store.load_conversation(&turn.conversation.id).unwrap();
        assert_eq!(detail.messages.last().unwrap().content, "4입니다.");
    }
}
