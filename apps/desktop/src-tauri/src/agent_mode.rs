use serde::Deserialize;
use serde_json::Value;

use crate::agent_tools::REACT_TOOL_DEFINITIONS;

const REACT_PROTOCOL: &str = include_str!("../resources/agent-modes/react/decision.md");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    Chat,
    React,
}

impl AgentMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "chat" => Ok(Self::Chat),
            "react" => Ok(Self::React),
            _ => Err(format!("unsupported agent mode: {value}")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::React => "react",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Chat => "Chat",
            Self::React => "ReAct",
        }
    }

    pub fn protocol_revision(self) -> &'static str {
        match self {
            Self::Chat => "chat/v1",
            Self::React => "react/v1",
        }
    }

    pub fn initial_stage(self) -> &'static str {
        match self {
            Self::Chat => "chat-response",
            Self::React => "react-decision",
        }
    }

    pub fn policy_json(self) -> &'static str {
        match self {
            Self::Chat => {
                r#"{"schemaVersion":1,"maxProgressSteps":1,"maxTotalSteps":1,"maxToolCalls":0,"maxRepeatedActions":0,"resetProgressOnToolSuccess":false}"#
            }
            Self::React => {
                r#"{"schemaVersion":1,"maxProgressSteps":6,"maxTotalSteps":16,"maxToolCalls":10,"maxRepeatedActions":2,"resetProgressOnToolSuccess":true}"#
            }
        }
    }
}

pub fn compile_agent_system_prompt(mode: AgentMode, persona_prompt: &str) -> String {
    if mode == AgentMode::Chat {
        return persona_prompt.into();
    }

    let protocol =
        format!("{REACT_PROTOCOL}\n\n{REACT_TOOL_DEFINITIONS}\n\nDo not expose hidden reasoning.");
    if persona_prompt.is_empty() {
        protocol
    } else {
        format!("{persona_prompt}\n\n{protocol}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentDecision {
    Final { content: String },
    ToolCall { name: String, arguments: Value },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ReactDecisionWire {
    Final { content: String },
    ToolCall { name: String, arguments: Value },
}

pub fn parse_react_decision(output: &str) -> Result<AgentDecision, String> {
    let candidate = strip_json_fence(output.trim());
    let decision: ReactDecisionWire = serde_json::from_str(candidate)
        .map_err(|error| format!("ReAct decision must be one valid JSON object: {error}"))?;
    match decision {
        ReactDecisionWire::Final { content } => {
            let content = content.trim();
            if content.is_empty() {
                return Err("ReAct final content must not be empty".into());
            }
            Ok(AgentDecision::Final {
                content: content.into(),
            })
        }
        ReactDecisionWire::ToolCall { name, arguments } => {
            let name = name.trim();
            if name.is_empty() {
                return Err("ReAct tool name must not be empty".into());
            }
            if !arguments.is_object() {
                return Err("ReAct tool arguments must be a JSON object".into());
            }
            Ok(AgentDecision::ToolCall {
                name: name.into(),
                arguments,
            })
        }
    }
}

fn strip_json_fence(value: &str) -> &str {
    let Some(rest) = value
        .strip_prefix("```json")
        .or_else(|| value.strip_prefix("```JSON"))
        .or_else(|| value.strip_prefix("```"))
    else {
        return value;
    };
    rest.trim()
        .strip_suffix("```")
        .map(str::trim)
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::{compile_agent_system_prompt, parse_react_decision, AgentDecision, AgentMode};

    #[test]
    fn only_shipped_modes_are_accepted() {
        assert_eq!(AgentMode::parse("chat"), Ok(AgentMode::Chat));
        assert_eq!(AgentMode::parse("react"), Ok(AgentMode::React));
        assert!(AgentMode::parse("plan-and-solve").is_err());
    }

    #[test]
    fn parses_final_and_tool_decisions() {
        assert_eq!(
            parse_react_decision(r#"{"type":"final","content":"done"}"#).unwrap(),
            AgentDecision::Final {
                content: "done".into()
            }
        );
        assert!(matches!(
            parse_react_decision(
                "```json\n{\"type\":\"tool_call\",\"name\":\"calculator\",\"arguments\":{\"expression\":\"2+2\"}}\n```"
            )
            .unwrap(),
            AgentDecision::ToolCall { name, .. } if name == "calculator"
        ));
    }

    #[test]
    fn rejects_empty_or_ambiguous_decisions() {
        assert!(parse_react_decision(r#"{"type":"final","content":" "}"#).is_err());
        assert!(parse_react_decision("answer before JSON").is_err());
        assert!(parse_react_decision(
            r#"{"type":"tool_call","name":"calculator","arguments":"2+2"}"#
        )
        .is_err());
    }

    #[test]
    fn compiles_the_mode_protocol_after_the_persona() {
        assert_eq!(
            compile_agent_system_prompt(AgentMode::Chat, "persona"),
            "persona"
        );
        let react = compile_agent_system_prompt(AgentMode::React, "persona");
        assert!(react.starts_with("persona\n\n"));
        assert!(react.contains("\"type\":\"tool_call\""));
        assert!(react.contains("calculator"));
    }
}
