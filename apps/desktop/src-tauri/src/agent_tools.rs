use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ToolCapability {
    Compute,
    FileRead,
    FileWrite,
    FileDelete,
    ProcessExecute,
    NetworkAccess,
}

impl ToolCapability {
    fn requires_external_authority(self) -> bool {
        self != Self::Compute
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub arguments: &'static str,
    pub capabilities: &'static [ToolCapability],
}

const COMPUTE_ONLY: &[ToolCapability] = &[ToolCapability::Compute];

const CALCULATOR: ToolDescriptor = ToolDescriptor {
    name: "calculator",
    description:
        "Evaluates arithmetic expressions with parentheses and common mathematical functions.",
    arguments: r#"{"expression":"a mathematical expression"}"#,
    capabilities: COMPUTE_ONLY,
};

const REGISTERED_TOOLS: &[ToolDescriptor] = &[CALCULATOR];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub model_content: String,
    pub action_digest: String,
    pub result_digest: String,
    pub successful: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolApprovalRequest {
    pub tool_name: String,
    pub action_digest: String,
    pub capabilities: Vec<ToolCapability>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedToolCall {
    descriptor: &'static ToolDescriptor,
    arguments: Value,
    action_digest: String,
}

impl PreparedToolCall {
    pub fn action_digest(&self) -> &str {
        &self.action_digest
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolPreparation {
    Ready(PreparedToolCall),
    ApprovalRequired(ToolApprovalRequest),
    Rejected(ToolResult),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolAuthorization {
    Allow,
    RequireApproval,
}

#[derive(Debug, Clone, Default)]
pub struct ToolGateway;

impl ToolGateway {
    pub fn builtin() -> Self {
        Self
    }

    pub fn prepare(&self, name: &str, arguments: &Value) -> ToolPreparation {
        let action_digest = tool_action_digest(name, arguments);
        let Some(descriptor) = tool_descriptor(name) else {
            return ToolPreparation::Rejected(failed_result(
                format!("Tool error: unknown tool `{name}`."),
                action_digest,
            ));
        };

        match authorize(descriptor) {
            ToolAuthorization::Allow => ToolPreparation::Ready(PreparedToolCall {
                descriptor,
                arguments: arguments.clone(),
                action_digest,
            }),
            ToolAuthorization::RequireApproval => {
                ToolPreparation::ApprovalRequired(ToolApprovalRequest {
                    tool_name: descriptor.name.into(),
                    action_digest,
                    capabilities: descriptor.capabilities.to_vec(),
                    reason: "This tool requires workspace or external access approval.".into(),
                })
            }
        }
    }

    pub fn execute(&self, call: PreparedToolCall) -> ToolResult {
        execute_registered_tool(call.descriptor, &call.arguments, call.action_digest)
    }
}

impl ToolApprovalRequest {
    pub fn into_blocked_result(self) -> ToolResult {
        failed_result(format!("Tool blocked: {}", self.reason), self.action_digest)
    }
}

pub fn react_tool_definitions() -> String {
    let mut output = String::from("Available tools:\n");
    for (index, descriptor) in REGISTERED_TOOLS.iter().enumerate() {
        output.push_str(&format!(
            "\n{}. {}\n   Arguments: {}\n   {}\n",
            index + 1,
            descriptor.name,
            descriptor.arguments,
            descriptor.description,
        ));
    }
    output.push_str("\nCall a tool only when its result is needed to answer accurately.");
    output
}

pub fn tool_action_digest(name: &str, arguments: &Value) -> String {
    let canonical_arguments = serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_string());
    digest(&format!("{name}\n{canonical_arguments}"))
}

fn tool_descriptor(name: &str) -> Option<&'static ToolDescriptor> {
    REGISTERED_TOOLS
        .iter()
        .find(|descriptor| descriptor.name == name)
}

fn authorize(descriptor: &ToolDescriptor) -> ToolAuthorization {
    if descriptor
        .capabilities
        .iter()
        .copied()
        .any(ToolCapability::requires_external_authority)
    {
        ToolAuthorization::RequireApproval
    } else {
        ToolAuthorization::Allow
    }
}

fn execute_registered_tool(
    descriptor: &ToolDescriptor,
    arguments: &Value,
    action_digest: String,
) -> ToolResult {
    match descriptor.name {
        "calculator" => calculator(arguments, action_digest),
        _ => failed_result(
            format!("Tool error: `{}` has no executor.", descriptor.name),
            action_digest,
        ),
    }
}

fn calculator(arguments: &Value, action_digest: String) -> ToolResult {
    let expression = arguments
        .get("expression")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let result = if expression.is_empty() {
        Err("the `expression` argument is required".to_string())
    } else if expression.chars().count() > 512 {
        Err("the expression is too long".to_string())
    } else {
        let mut namespace = fasteval::EmptyNamespace;
        fasteval::ez_eval(expression, &mut namespace)
            .map_err(|error| format!("invalid expression: {error}"))
            .and_then(|value| {
                value
                    .is_finite()
                    .then_some(value)
                    .ok_or_else(|| "the result is not finite".to_string())
            })
    };
    match result {
        Ok(value) => {
            let rendered = format!("{value:.15}")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string();
            let model_content = format!("Calculator result: {rendered}");
            ToolResult {
                result_digest: digest(&model_content),
                model_content,
                action_digest,
                successful: true,
            }
        }
        Err(error) => failed_result(format!("Calculator error: {error}"), action_digest),
    }
}

fn failed_result(model_content: String, action_digest: String) -> ToolResult {
    ToolResult {
        result_digest: digest(&model_content),
        model_content,
        action_digest,
        successful: false,
    }
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        authorize, react_tool_definitions, ToolAuthorization, ToolCapability, ToolDescriptor,
        ToolGateway, ToolPreparation,
    };

    #[test]
    fn calculator_runs_through_the_gateway() {
        let gateway = ToolGateway::builtin();
        let ToolPreparation::Ready(call) =
            gateway.prepare("calculator", &json!({ "expression": "(2 + 3) * 4" }))
        else {
            panic!("calculator must be prepared");
        };
        let result = gateway.execute(call);
        assert!(result.successful);
        assert_eq!(result.model_content, "Calculator result: 20");
    }

    #[test]
    fn invalid_and_unknown_tools_are_failed_results() {
        let gateway = ToolGateway::builtin();
        let ToolPreparation::Ready(invalid_call) = gateway.prepare("calculator", &json!({})) else {
            panic!("calculator validation must run in its executor");
        };
        let invalid = gateway.execute(invalid_call);
        let ToolPreparation::Rejected(unknown) = gateway.prepare("missing", &json!({})) else {
            panic!("unknown tools must be rejected before execution");
        };
        assert!(!invalid.successful);
        assert!(!unknown.successful);
    }

    #[test]
    fn external_capabilities_require_an_approval_boundary() {
        let descriptor = ToolDescriptor {
            name: "read-file",
            description: "test",
            arguments: r#"{"path":"a path"}"#,
            capabilities: &[ToolCapability::FileRead],
        };
        assert_eq!(authorize(&descriptor), ToolAuthorization::RequireApproval);
    }

    #[test]
    fn prompt_definitions_are_rendered_from_the_registry() {
        let definitions = react_tool_definitions();
        assert!(definitions.contains("calculator"));
        assert!(definitions.contains(r#"{"expression":"a mathematical expression"}"#));
    }
}
