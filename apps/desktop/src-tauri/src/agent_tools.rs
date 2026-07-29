use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use ignore::WalkBuilder;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::workspace_guard::WorkspaceGuard;

const MAX_LIST_ENTRIES: usize = 200;
const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;
const MAX_READ_LINES: usize = 400;
const DEFAULT_READ_LINES: usize = 200;
const MAX_READ_OUTPUT_BYTES: usize = 128 * 1024;
const MAX_SEARCH_FILE_BYTES: u64 = 1024 * 1024;
const MAX_SEARCH_FILES: usize = 5_000;
const MAX_SEARCH_RESULTS: usize = 100;
const MAX_MATCHES_PER_FILE: usize = 3;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub arguments: &'static str,
    pub capabilities: &'static [ToolCapability],
}

const COMPUTE_ONLY: &[ToolCapability] = &[ToolCapability::Compute];
const FILE_READ_ONLY: &[ToolCapability] = &[ToolCapability::FileRead];

const CALCULATOR: ToolDescriptor = ToolDescriptor {
    name: "calculator",
    description:
        "Evaluates arithmetic expressions with parentheses and common mathematical functions.",
    arguments: r#"{"expression":"a mathematical expression"}"#,
    capabilities: COMPUTE_ONLY,
};

const LIST_FILES: ToolDescriptor = ToolDescriptor {
    name: "list_files",
    description:
        "Lists files and subdirectories directly inside a directory in the current workspace.",
    arguments: r#"{"path":"a workspace-relative directory path, or . for the workspace root"}"#,
    capabilities: FILE_READ_ONLY,
};

const READ_FILE: ToolDescriptor = ToolDescriptor {
    name: "read_file",
    description: "Reads a UTF-8 text file in the current workspace. Line offsets are 1-based.",
    arguments: r#"{"path":"a workspace-relative file path","offset":1,"limit":200}"#,
    capabilities: FILE_READ_ONLY,
};

const SEARCH_FILES: ToolDescriptor = ToolDescriptor {
    name: "search_files",
    description:
        "Recursively searches file names and UTF-8 text contents in the current workspace.",
    arguments: r#"{"query":"literal text to find","path":"a workspace-relative directory path, or ."}"#,
    capabilities: FILE_READ_ONLY,
};

const GET_FILE_INFO: ToolDescriptor = ToolDescriptor {
    name: "get_file_info",
    description: "Gets metadata for a file or directory in the current workspace.",
    arguments: r#"{"path":"a workspace-relative file or directory path"}"#,
    capabilities: FILE_READ_ONLY,
};

const REGISTERED_TOOLS: &[ToolDescriptor] = &[
    CALCULATOR,
    LIST_FILES,
    READ_FILE,
    SEARCH_FILES,
    GET_FILE_INFO,
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolContext {
    workspace_path: Option<String>,
}

impl ToolContext {
    pub fn for_workspace(workspace_path: impl Into<String>) -> Self {
        Self {
            workspace_path: Some(workspace_path.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub model_content: String,
    pub display_content: String,
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
    context: ToolContext,
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

    pub fn prepare(&self, name: &str, arguments: &Value, context: &ToolContext) -> ToolPreparation {
        let action_digest = tool_action_digest(name, arguments);
        let Some(descriptor) = tool_descriptor(name) else {
            return ToolPreparation::Rejected(failed_result(
                format!("Tool error: unknown tool `{name}`."),
                action_digest,
            ));
        };

        match authorize(descriptor, context) {
            ToolAuthorization::Allow => ToolPreparation::Ready(PreparedToolCall {
                descriptor,
                arguments: arguments.clone(),
                action_digest,
                context: context.clone(),
            }),
            ToolAuthorization::RequireApproval => {
                ToolPreparation::ApprovalRequired(ToolApprovalRequest {
                    tool_name: descriptor.name.into(),
                    action_digest,
                    capabilities: descriptor.capabilities.to_vec(),
                    reason:
                        "This tool requires access that is not granted by the current workspace."
                            .into(),
                })
            }
        }
    }

    pub fn execute(&self, call: PreparedToolCall) -> ToolResult {
        execute_registered_tool(
            call.descriptor,
            &call.arguments,
            call.action_digest,
            &call.context,
        )
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
    output.push_str(
        "\nUse workspace-relative paths. Call a tool only when its result is needed to answer accurately.",
    );
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

fn authorize(descriptor: &ToolDescriptor, context: &ToolContext) -> ToolAuthorization {
    let allowed = descriptor
        .capabilities
        .iter()
        .all(|capability| match capability {
            ToolCapability::Compute => true,
            ToolCapability::FileRead => context.workspace_path.is_some(),
            ToolCapability::FileWrite
            | ToolCapability::FileDelete
            | ToolCapability::ProcessExecute
            | ToolCapability::NetworkAccess => false,
        });
    if allowed {
        ToolAuthorization::Allow
    } else {
        ToolAuthorization::RequireApproval
    }
}

fn execute_registered_tool(
    descriptor: &ToolDescriptor,
    arguments: &Value,
    action_digest: String,
    context: &ToolContext,
) -> ToolResult {
    let result = match descriptor.name {
        "calculator" => return calculator(arguments, action_digest),
        "list_files" => with_workspace(context, |guard| list_files(guard, arguments)),
        "read_file" => with_workspace(context, |guard| read_file(guard, arguments)),
        "search_files" => with_workspace(context, |guard| search_files(guard, arguments)),
        "get_file_info" => with_workspace(context, |guard| get_file_info(guard, arguments)),
        _ => Err(format!("`{}` has no executor", descriptor.name)),
    };
    match result {
        Ok((model_content, display_content)) => {
            successful_result(model_content, display_content, action_digest)
        }
        Err(error) => failed_result(format!("Tool error: {error}"), action_digest),
    }
}

fn with_workspace<F>(context: &ToolContext, execute: F) -> Result<(String, String), String>
where
    F: FnOnce(&WorkspaceGuard) -> Result<(String, String), String>,
{
    let workspace_path = context
        .workspace_path
        .as_deref()
        .ok_or_else(|| "the current conversation has no workspace".to_string())?;
    let guard = WorkspaceGuard::new(workspace_path)?;
    execute(&guard)
}

fn list_files(guard: &WorkspaceGuard, arguments: &Value) -> Result<(String, String), String> {
    let requested = required_string(arguments, "path")?;
    let directory = guard.resolve_existing(requested)?;
    if !directory.is_dir() {
        return Err(format!("`{requested}` is not a directory"));
    }
    let mut entries = fs::read_dir(&directory)
        .map_err(|error| format!("failed to list `{requested}`: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to list `{requested}`: {error}"))?;
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());
    let truncated = entries.len() > MAX_LIST_ENTRIES;
    let entries = entries
        .into_iter()
        .take(MAX_LIST_ENTRIES)
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let symlink_metadata = entry
                .metadata()
                .map_err(|error| format!("failed to inspect `{name}`: {error}"))?;
            let resolved = match guard.resolve_existing_path(&entry.path()) {
                Ok(path) => path,
                Err(_) => {
                    return Ok(json!({
                        "name": name,
                        "type": "external_link",
                        "accessible": false,
                    }));
                }
            };
            let metadata = fs::metadata(&resolved)
                .map_err(|error| format!("failed to inspect `{name}`: {error}"))?;
            let kind = if metadata.is_dir() {
                "directory"
            } else if metadata.is_file() {
                "file"
            } else {
                "other"
            };
            Ok(json!({
                "name": name,
                "type": kind,
                "size": metadata.is_file().then_some(metadata.len()),
                "link": symlink_metadata.file_type().is_symlink(),
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let relative = guard.relative_display(&directory)?;
    let model = json!({
        "path": relative,
        "entries": entries,
        "truncated": truncated,
    })
    .to_string();
    Ok((
        model,
        format!(
            "{}개 항목 확인{}",
            entries.len(),
            if truncated { " (일부만 표시)" } else { "" }
        ),
    ))
}

fn read_file(guard: &WorkspaceGuard, arguments: &Value) -> Result<(String, String), String> {
    let requested = required_string(arguments, "path")?;
    let path = guard.resolve_existing(requested)?;
    if !path.is_file() {
        return Err(format!("`{requested}` is not a file"));
    }
    let metadata =
        fs::metadata(&path).map_err(|error| format!("failed to inspect `{requested}`: {error}"))?;
    if metadata.len() > MAX_READ_BYTES {
        return Err(format!(
            "`{requested}` is larger than the {} MiB read limit",
            MAX_READ_BYTES / (1024 * 1024)
        ));
    }
    let bytes =
        fs::read(&path).map_err(|error| format!("failed to read `{requested}`: {error}"))?;
    if bytes.contains(&0) {
        return Err(format!("`{requested}` appears to be a binary file"));
    }
    let content =
        String::from_utf8(bytes).map_err(|_| format!("`{requested}` is not valid UTF-8 text"))?;
    let offset = optional_usize(arguments, "offset", 1)?;
    if offset == 0 {
        return Err("`offset` must be at least 1".into());
    }
    let limit = optional_usize(arguments, "limit", DEFAULT_READ_LINES)?;
    if limit == 0 || limit > MAX_READ_LINES {
        return Err(format!("`limit` must be between 1 and {MAX_READ_LINES}"));
    }
    let lines = content.lines().collect::<Vec<_>>();
    let start_index = offset.saturating_sub(1).min(lines.len());
    let end_index = start_index.saturating_add(limit).min(lines.len());
    let selected = lines[start_index..end_index].join("\n");
    let (selected, output_truncated) = truncate_utf8(&selected, MAX_READ_OUTPUT_BYTES);
    let has_more = end_index < lines.len() || output_truncated;
    let relative = guard.relative_display(&path)?;
    let model = json!({
        "path": relative,
        "offset": offset,
        "returnedLines": end_index.saturating_sub(start_index),
        "totalLines": lines.len(),
        "hasMore": has_more,
        "content": selected,
    })
    .to_string();
    Ok((
        model,
        format!(
            "{}줄 읽음{}",
            end_index.saturating_sub(start_index),
            if has_more {
                " (이어서 읽을 수 있음)"
            } else {
                ""
            }
        ),
    ))
}

fn search_files(guard: &WorkspaceGuard, arguments: &Value) -> Result<(String, String), String> {
    let query = required_string(arguments, "query")?;
    if query.chars().count() > 256 {
        return Err("`query` is too long".into());
    }
    let requested = arguments
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(".");
    let directory = guard.resolve_existing(requested)?;
    if !directory.is_dir() {
        return Err(format!("`{requested}` is not a directory"));
    }

    let query_lower = query.to_lowercase();
    let mut builder = WalkBuilder::new(&directory);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .follow_links(false)
        .filter_entry(|entry| entry.file_name() != ".git");
    let mut results = Vec::new();
    let mut scanned_files = 0usize;
    let mut truncated = false;

    for entry in builder.build() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.depth() == 0 || !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        if scanned_files >= MAX_SEARCH_FILES || results.len() >= MAX_SEARCH_RESULTS {
            truncated = true;
            break;
        }
        let resolved = match guard.resolve_existing_path(entry.path()) {
            Ok(path) => path,
            Err(_) => continue,
        };
        scanned_files += 1;
        let relative = guard.relative_display(&resolved)?;
        if entry
            .file_name()
            .to_string_lossy()
            .to_lowercase()
            .contains(&query_lower)
        {
            results.push(json!({
                "path": relative,
                "match": "name",
            }));
            if results.len() >= MAX_SEARCH_RESULTS {
                truncated = true;
                break;
            }
        }
        match fs::metadata(&resolved) {
            Ok(metadata) if metadata.len() <= MAX_SEARCH_FILE_BYTES => {}
            _ => continue,
        }
        let bytes = match fs::read(&resolved) {
            Ok(bytes) if !bytes.contains(&0) => bytes,
            _ => continue,
        };
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => continue,
        };
        let mut file_matches = 0usize;
        for (index, line) in text.lines().enumerate() {
            if !line.to_lowercase().contains(&query_lower) {
                continue;
            }
            let (preview, _) = truncate_utf8(line.trim(), 320);
            results.push(json!({
                "path": relative,
                "match": "content",
                "line": index + 1,
                "preview": preview,
            }));
            file_matches += 1;
            if file_matches >= MAX_MATCHES_PER_FILE || results.len() >= MAX_SEARCH_RESULTS {
                break;
            }
        }
    }

    let relative = guard.relative_display(&directory)?;
    let model = json!({
        "query": query,
        "path": relative,
        "matches": results,
        "scannedFiles": scanned_files,
        "truncated": truncated,
    })
    .to_string();
    Ok((
        model,
        format!(
            "{}개 결과{}",
            results.len(),
            if truncated { " (일부만 표시)" } else { "" }
        ),
    ))
}

fn get_file_info(guard: &WorkspaceGuard, arguments: &Value) -> Result<(String, String), String> {
    let requested = required_string(arguments, "path")?;
    let path = guard.resolve_existing(requested)?;
    let metadata =
        fs::metadata(&path).map_err(|error| format!("failed to inspect `{requested}`: {error}"))?;
    let kind = if metadata.is_file() {
        "file"
    } else if metadata.is_dir() {
        "directory"
    } else {
        "other"
    };
    let relative = guard.relative_display(&path)?;
    let model = json!({
        "path": relative,
        "type": kind,
        "size": metadata.is_file().then_some(metadata.len()),
        "readOnly": metadata.permissions().readonly(),
        "modifiedAt": system_time_millis(metadata.modified().ok()),
        "createdAt": system_time_millis(metadata.created().ok()),
    })
    .to_string();
    Ok((model, format!("{kind} 정보 확인")))
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
            successful_result(model_content, rendered, action_digest)
        }
        Err(error) => failed_result(format!("Calculator error: {error}"), action_digest),
    }
}

fn required_string<'a>(arguments: &'a Value, name: &str) -> Result<&'a str, String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("the `{name}` argument is required"))
}

fn optional_usize(arguments: &Value, name: &str, default: usize) -> Result<usize, String> {
    match arguments.get(name) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| format!("`{name}` must be a non-negative integer")),
    }
}

fn truncate_utf8(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.into(), false);
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].into(), true)
}

fn system_time_millis(value: Option<SystemTime>) -> Option<u128> {
    value?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

fn successful_result(
    model_content: String,
    display_content: String,
    action_digest: String,
) -> ToolResult {
    ToolResult {
        result_digest: digest(&model_content),
        model_content,
        display_content,
        action_digest,
        successful: true,
    }
}

fn failed_result(model_content: String, action_digest: String) -> ToolResult {
    let display_content = model_content
        .strip_prefix("Tool error: ")
        .or_else(|| model_content.strip_prefix("Calculator error: "))
        .unwrap_or(&model_content)
        .to_string();
    ToolResult {
        result_digest: digest(&model_content),
        model_content,
        display_content,
        action_digest,
        successful: false,
    }
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::{
        react_tool_definitions, ToolContext, ToolGateway, ToolPreparation, MAX_SEARCH_RESULTS,
    };

    fn execute(
        gateway: &ToolGateway,
        context: &ToolContext,
        name: &str,
        arguments: Value,
    ) -> super::ToolResult {
        let ToolPreparation::Ready(call) = gateway.prepare(name, &arguments, context) else {
            panic!("{name} must be prepared");
        };
        gateway.execute(call)
    }

    #[test]
    fn calculator_runs_through_the_gateway() {
        let gateway = ToolGateway::builtin();
        let result = execute(
            &gateway,
            &ToolContext::default(),
            "calculator",
            json!({ "expression": "(2 + 3) * 4" }),
        );
        assert!(result.successful);
        assert_eq!(result.model_content, "Calculator result: 20");
        assert_eq!(result.display_content, "20");
    }

    #[test]
    fn invalid_and_unknown_tools_are_failed_results() {
        let gateway = ToolGateway::builtin();
        let invalid = execute(&gateway, &ToolContext::default(), "calculator", json!({}));
        let ToolPreparation::Rejected(unknown) =
            gateway.prepare("missing", &json!({}), &ToolContext::default())
        else {
            panic!("unknown tools must be rejected before execution");
        };
        assert!(!invalid.successful);
        assert!(!unknown.successful);
    }

    #[test]
    fn file_read_requires_a_workspace_authority() {
        let gateway = ToolGateway::builtin();
        assert!(matches!(
            gateway.prepare(
                "read_file",
                &json!({ "path": "README.md" }),
                &ToolContext::default()
            ),
            ToolPreparation::ApprovalRequired(_)
        ));
    }

    #[test]
    fn workspace_tools_read_list_search_and_describe_files() {
        let workspace = tempdir().unwrap();
        fs::create_dir(workspace.path().join("src")).unwrap();
        fs::write(
            workspace.path().join("src/lib.rs"),
            "pub fn answer() -> u32 {\n    42\n}\n",
        )
        .unwrap();
        fs::write(
            workspace.path().join("README.md"),
            "The answer is documented.\n",
        )
        .unwrap();
        let context = ToolContext::for_workspace(workspace.path().to_string_lossy());
        let gateway = ToolGateway::builtin();

        let listed = execute(&gateway, &context, "list_files", json!({ "path": "." }));
        assert!(listed.successful);
        assert!(listed.model_content.contains("README.md"));
        assert!(listed.model_content.contains(r#""type":"directory""#));

        let read = execute(
            &gateway,
            &context,
            "read_file",
            json!({ "path": "src/lib.rs", "offset": 2, "limit": 1 }),
        );
        assert!(read.successful);
        assert!(read.model_content.contains(r#""content":"    42""#));
        assert!(read.model_content.contains(r#""hasMore":true"#));

        let searched = execute(
            &gateway,
            &context,
            "search_files",
            json!({ "query": "answer", "path": "." }),
        );
        assert!(searched.successful);
        assert!(searched.model_content.contains("src/lib.rs"));
        assert!(searched.model_content.contains("README.md"));

        let info = execute(
            &gateway,
            &context,
            "get_file_info",
            json!({ "path": "src/lib.rs" }),
        );
        assert!(info.successful);
        assert!(info.model_content.contains(r#""type":"file""#));
    }

    #[test]
    fn workspace_tools_reject_paths_outside_the_workspace() {
        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let secret = outside.path().join("secret.txt");
        fs::write(&secret, "secret").unwrap();
        let context = ToolContext::for_workspace(workspace.path().to_string_lossy());
        let result = execute(
            &ToolGateway::builtin(),
            &context,
            "read_file",
            json!({ "path": secret }),
        );
        assert!(!result.successful);
        assert!(result
            .model_content
            .contains("outside the current workspace"));
    }

    #[test]
    fn prompt_definitions_are_rendered_from_the_registry() {
        let definitions = react_tool_definitions();
        for name in [
            "calculator",
            "list_files",
            "read_file",
            "search_files",
            "get_file_info",
        ] {
            assert!(definitions.contains(name));
        }
        assert!(definitions.contains(r#"{"expression":"a mathematical expression"}"#));
        assert_eq!(MAX_SEARCH_RESULTS, 100);
    }
}
