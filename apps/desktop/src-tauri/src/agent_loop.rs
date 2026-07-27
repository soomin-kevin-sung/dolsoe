use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use tauri::{AppHandle, Emitter};

use crate::{
    conversation_store::{ConversationStore, MessageStatus},
    llm_dto::{LlmEventDto, LlmEventKind, SubmitRequest, SubmitResponse},
    runtime_host::RuntimeHost,
};

type EventSink = Arc<dyn Fn(LlmEventDto) -> Result<(), String> + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentMode {
    Chat,
}

impl AgentMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "chat" => Ok(Self::Chat),
            _ => Err(format!("unsupported agent mode: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StrategyAction {
    SubmitModel,
    Complete,
}

trait AgentStrategy: Send {
    fn start(&mut self) -> StrategyAction;
    fn after_model(&mut self, completed_steps: u32) -> StrategyAction;
}

struct ChatStrategy;

impl AgentStrategy for ChatStrategy {
    fn start(&mut self) -> StrategyAction {
        StrategyAction::SubmitModel
    }

    fn after_model(&mut self, _completed_steps: u32) -> StrategyAction {
        StrategyAction::Complete
    }
}

fn strategy_for(mode: AgentMode) -> Box<dyn AgentStrategy> {
    match mode {
        AgentMode::Chat => Box::new(ChatStrategy),
    }
}

struct AgentRunLoop {
    strategy: Box<dyn AgentStrategy>,
    total_steps: u32,
    max_total_steps: u32,
}

impl AgentRunLoop {
    fn new(strategy: Box<dyn AgentStrategy>, max_total_steps: u32) -> Result<Self, String> {
        if max_total_steps == 0 {
            return Err("agent max total steps must be positive".into());
        }
        Ok(Self {
            strategy,
            total_steps: 0,
            max_total_steps,
        })
    }

    fn start(&mut self) -> Result<StrategyAction, String> {
        let action = self.strategy.start();
        self.accept(action)
    }

    fn after_model(&mut self) -> Result<StrategyAction, String> {
        let action = self.strategy.after_model(self.total_steps);
        self.accept(action)
    }

    fn accept(&mut self, action: StrategyAction) -> Result<StrategyAction, String> {
        if action == StrategyAction::SubmitModel {
            if self.total_steps >= self.max_total_steps {
                return Err("agent reached its absolute model step limit".into());
            }
            self.total_steps += 1;
        }
        Ok(action)
    }
}

struct ActiveRun {
    output: Vec<u8>,
    run_loop: AgentRunLoop,
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
        let mut run_loop = AgentRunLoop::new(strategy_for(mode), 1)?;
        if run_loop.start()? != StrategyAction::SubmitModel {
            return Err("agent strategy did not produce an initial model step".into());
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
                    output: Vec::new(),
                    run_loop,
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
        self.inner.store.bind_agent_request(
            &submission.run_id,
            &submission.step_id,
            &response.request_handle,
        )?;
        Ok(response)
    }

    pub fn route_event(&self, mut event: LlmEventDto) -> Result<(), String> {
        let Some(correlation_id) = event
            .correlation_id
            .as_deref()
            .map(str::parse::<u64>)
            .transpose()
            .map_err(|_| "runtime emitted an invalid agent correlation id")?
        else {
            return (self.inner.sink)(event);
        };

        let terminal = matches!(
            event.kind,
            LlmEventKind::Done | LlmEventKind::Cancelled | LlmEventKind::Error
        );
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
            if event.kind == LlmEventKind::Token {
                run.output.extend_from_slice(&event.bytes);
            }
            if terminal {
                finished = active.remove(&correlation_id);
            }
        }

        if let Some(mut run) = finished {
            if run.run_loop.after_model()? != StrategyAction::Complete {
                return Err("agent strategy requested an unsupported follow-up step".into());
            }
            let status = match event.kind {
                LlmEventKind::Done => MessageStatus::Complete,
                LlmEventKind::Cancelled => MessageStatus::Cancelled,
                LlmEventKind::Error => MessageStatus::Error,
                _ => unreachable!("only terminal events remove active runs"),
            };
            let mut content = run.output;
            if status == MessageStatus::Error && content.is_empty() {
                content.extend_from_slice(&event.bytes);
            }
            let content = String::from_utf8_lossy(&content).into_owned();
            let reason = match status {
                MessageStatus::Complete => "strategy-complete",
                MessageStatus::Cancelled => "user-cancelled",
                MessageStatus::Error => "model-error",
                _ => unreachable!("agent terminal status is exhaustive"),
            };
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

    fn remove_active(&self, correlation_id: u64) {
        if let Ok(mut active) = self.inner.active.lock() {
            active.remove(&correlation_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{
        strategy_for, AgentController, AgentMode, AgentRunLoop, AgentStrategy, ChatStrategy,
        StrategyAction,
    };
    use crate::{
        conversation_store::{ConversationStore, MessageStatus},
        llm_dto::{LlmEventDto, LlmEventKind},
    };

    #[test]
    fn chat_strategy_is_exactly_one_model_step() {
        let mut run_loop = AgentRunLoop::new(Box::new(ChatStrategy), 1).unwrap();

        assert_eq!(run_loop.start().unwrap(), StrategyAction::SubmitModel);
        assert_eq!(run_loop.after_model().unwrap(), StrategyAction::Complete);
    }

    #[test]
    fn unknown_modes_are_rejected_instead_of_falling_back() {
        assert!(AgentMode::parse("react").is_err());
        let mut strategy = strategy_for(AgentMode::Chat);
        assert_eq!(strategy.start(), StrategyAction::SubmitModel);
    }

    struct ThreeStepStrategy;

    impl AgentStrategy for ThreeStepStrategy {
        fn start(&mut self) -> StrategyAction {
            StrategyAction::SubmitModel
        }

        fn after_model(&mut self, completed_steps: u32) -> StrategyAction {
            if completed_steps < 3 {
                StrategyAction::SubmitModel
            } else {
                StrategyAction::Complete
            }
        }
    }

    #[test]
    fn loop_contract_can_drive_multiple_model_steps_with_an_absolute_limit() {
        let mut run_loop = AgentRunLoop::new(Box::new(ThreeStepStrategy), 3).unwrap();

        assert_eq!(run_loop.start().unwrap(), StrategyAction::SubmitModel);
        assert_eq!(run_loop.after_model().unwrap(), StrategyAction::SubmitModel);
        assert_eq!(run_loop.after_model().unwrap(), StrategyAction::SubmitModel);
        assert_eq!(run_loop.after_model().unwrap(), StrategyAction::Complete);

        let mut limited = AgentRunLoop::new(Box::new(ThreeStepStrategy), 2).unwrap();
        assert_eq!(limited.start().unwrap(), StrategyAction::SubmitModel);
        assert_eq!(limited.after_model().unwrap(), StrategyAction::SubmitModel);
        assert!(limited.after_model().is_err());
    }

    #[test]
    fn terminal_event_is_persisted_before_it_is_emitted() {
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
                output: b"answer".to_vec(),
                run_loop: AgentRunLoop::new(Box::new(ChatStrategy), 1).unwrap(),
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
}
