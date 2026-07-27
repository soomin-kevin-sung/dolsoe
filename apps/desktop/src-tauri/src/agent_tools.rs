use serde_json::Value;
use sha2::{Digest, Sha256};

pub const REACT_TOOL_DEFINITIONS: &str = r#"
Available read-only tools:

1. calculator
   Arguments: {"expression":"a mathematical expression"}
   Evaluates arithmetic expressions with parentheses and common mathematical functions.

Call a tool only when its result is needed to answer accurately.
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub model_content: String,
    pub action_digest: String,
    pub result_digest: String,
    pub successful: bool,
}

pub fn execute_read_only_tool(name: &str, arguments: &Value) -> ToolResult {
    let action_digest = tool_action_digest(name, arguments);
    match name {
        "calculator" => calculator(arguments, action_digest),
        _ => ToolResult {
            model_content: format!("Tool error: unknown tool `{name}`."),
            action_digest,
            result_digest: digest("unknown-tool"),
            successful: false,
        },
    }
}

pub fn tool_action_digest(name: &str, arguments: &Value) -> String {
    let canonical_arguments = serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_string());
    digest(&format!("{name}\n{canonical_arguments}"))
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
        Err(error) => {
            let model_content = format!("Calculator error: {error}");
            ToolResult {
                result_digest: digest(&model_content),
                model_content,
                action_digest,
                successful: false,
            }
        }
    }
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::execute_read_only_tool;

    #[test]
    fn calculator_returns_normalized_results() {
        let result = execute_read_only_tool("calculator", &json!({ "expression": "(2 + 3) * 4" }));
        assert!(result.successful);
        assert_eq!(result.model_content, "Calculator result: 20");
    }

    #[test]
    fn invalid_and_unknown_tools_do_not_report_progress() {
        assert!(!execute_read_only_tool("calculator", &json!({})).successful);
        assert!(!execute_read_only_tool("missing", &json!({})).successful);
    }
}
