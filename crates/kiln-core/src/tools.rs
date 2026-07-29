use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

pub const DEFAULT_READ_LINE_COUNT: u32 = 200;
pub const MAX_READ_LINE_COUNT: u32 = 1_000;
pub const DEFAULT_SEARCH_RESULTS: u32 = 100;
pub const MAX_SEARCH_RESULTS: u32 = 500;
pub const MAX_WRITE_BYTES: usize = 256 * 1024;
pub const MAX_TOOL_ARGUMENT_BYTES: usize = MAX_WRITE_BYTES + 64 * 1024;
pub const MAX_TOOL_PATH_CHARS: usize = 4_096;
pub const MAX_TOOL_TEXT_CHARS: usize = 16_384;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

pub fn repository_tool_definitions() -> Vec<RepositoryToolDefinition> {
    vec![
        RepositoryToolDefinition {
            name: "read_file",
            description: "Read a bounded range of UTF-8 lines from a repository file.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "minLength": 1, "maxLength": MAX_TOOL_PATH_CHARS },
                    "startLine": { "type": "integer", "minimum": 1 },
                    "lineCount": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_READ_LINE_COUNT
                    }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        },
        RepositoryToolDefinition {
            name: "search_files",
            description: "Find repository files by a bounded path pattern.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "minLength": 1, "maxLength": MAX_TOOL_TEXT_CHARS },
                    "maxResults": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_SEARCH_RESULTS
                    }
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
        },
        RepositoryToolDefinition {
            name: "search_text",
            description: "Search bounded repository text without exposing unrestricted filesystem access.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "minLength": 1, "maxLength": MAX_TOOL_TEXT_CHARS },
                    "path": { "type": "string", "minLength": 1, "maxLength": MAX_TOOL_PATH_CHARS },
                    "caseSensitive": { "type": "boolean" },
                    "maxResults": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_SEARCH_RESULTS
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        },
        RepositoryToolDefinition {
            name: "write_file",
            description: "Replace one UTF-8 repository file atomically after policy approval and version checking.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "minLength": 1, "maxLength": MAX_TOOL_PATH_CHARS },
                    "content": { "type": "string", "maxLength": MAX_WRITE_BYTES },
                    "expectedSha256": {
                        "type": "string",
                        "pattern": "^[A-Fa-f0-9]{64}$"
                    }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
        },
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "tool",
    content = "input",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RepositoryToolRequest {
    ReadFile(ReadFileRequest),
    SearchFiles(SearchFilesRequest),
    SearchText(SearchTextRequest),
    WriteFile(WriteFileRequest),
}

impl RepositoryToolRequest {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ReadFile(_) => "read_file",
            Self::SearchFiles(_) => "search_files",
            Self::SearchText(_) => "search_text",
            Self::WriteFile(_) => "write_file",
        }
    }

    pub fn validate(&self) -> Result<(), ToolContractError> {
        match self {
            Self::ReadFile(request) => request.validate(),
            Self::SearchFiles(request) => request.validate(),
            Self::SearchText(request) => request.validate(),
            Self::WriteFile(request) => request.validate(),
        }
    }

    /// Safe, bounded text for a durable tool proposal. Search terms and
    /// replacement content deliberately remain transient provider context.
    pub fn proposal_summary(&self) -> String {
        match self {
            Self::ReadFile(request) => {
                format!("Read {} inside the selected workspace.", request.path)
            }
            Self::SearchFiles(_) => "Search file paths inside the selected workspace.".to_owned(),
            Self::SearchText(_) => "Search text inside the selected workspace.".to_owned(),
            Self::WriteFile(request) => {
                format!("Write {} after explicit approval.", request.path)
            }
        }
    }

    pub fn from_provider_call(name: &str, arguments: &str) -> Result<Self, ToolContractError> {
        if arguments.len() > MAX_TOOL_ARGUMENT_BYTES {
            return Err(ToolContractError::InvalidField {
                field: "tool.arguments",
                message: format!("arguments cannot exceed {MAX_TOOL_ARGUMENT_BYTES} UTF-8 bytes"),
            });
        }
        if !arguments.trim_start().starts_with('{') {
            return Err(ToolContractError::InvalidField {
                field: "tool.arguments",
                message: "arguments must be one JSON object".to_owned(),
            });
        }

        let request = match name {
            "read_file" => Self::ReadFile(parse_arguments(arguments)?),
            "search_files" => Self::SearchFiles(parse_arguments(arguments)?),
            "search_text" => Self::SearchText(parse_arguments(arguments)?),
            "write_file" => Self::WriteFile(parse_arguments(arguments)?),
            _ => {
                return Err(ToolContractError::InvalidField {
                    field: "tool.name",
                    message: "tool name is not in Kiln's repository allowlist".to_owned(),
                })
            }
        };
        request.validate()?;
        Ok(request)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteFileRequest {
    pub path: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_sha256: Option<String>,
}

impl WriteFileRequest {
    pub fn validate(&self) -> Result<(), ToolContractError> {
        validate_path("writeFile.path", &self.path)?;
        if self.content.len() > MAX_WRITE_BYTES {
            return Err(ToolContractError::InvalidField {
                field: "writeFile.content",
                message: format!("content cannot exceed {MAX_WRITE_BYTES} UTF-8 bytes"),
            });
        }
        if self
            .expected_sha256
            .as_ref()
            .is_some_and(|hash| !is_sha256(hash))
        {
            return Err(ToolContractError::InvalidField {
                field: "writeFile.expectedSha256",
                message: "expected hash must be 64 hexadecimal characters".to_owned(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadFileRequest {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_count: Option<u32>,
}

impl ReadFileRequest {
    pub fn validate(&self) -> Result<(), ToolContractError> {
        validate_path("readFile.path", &self.path)?;
        if self.start_line == Some(0) {
            return Err(ToolContractError::InvalidField {
                field: "readFile.startLine",
                message: "line numbers start at 1".to_owned(),
            });
        }
        if self.line_count == Some(0)
            || self
                .line_count
                .is_some_and(|count| count > MAX_READ_LINE_COUNT)
        {
            return Err(ToolContractError::InvalidField {
                field: "readFile.lineCount",
                message: format!("line count must be between 1 and {MAX_READ_LINE_COUNT}"),
            });
        }
        Ok(())
    }

    pub fn effective_start_line(&self) -> u32 {
        self.start_line.unwrap_or(1)
    }

    pub fn effective_line_count(&self) -> u32 {
        self.line_count.unwrap_or(DEFAULT_READ_LINE_COUNT)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchFilesRequest {
    pub pattern: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
}

impl SearchFilesRequest {
    pub fn validate(&self) -> Result<(), ToolContractError> {
        validate_text("searchFiles.pattern", &self.pattern)?;
        validate_result_limit("searchFiles.maxResults", self.max_results)
    }

    pub fn effective_max_results(&self) -> u32 {
        self.max_results.unwrap_or(DEFAULT_SEARCH_RESULTS)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchTextRequest {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
}

impl SearchTextRequest {
    pub fn validate(&self) -> Result<(), ToolContractError> {
        validate_text("searchText.query", &self.query)?;
        if let Some(path) = &self.path {
            validate_path("searchText.path", path)?;
        }
        validate_result_limit("searchText.maxResults", self.max_results)
    }

    pub fn effective_max_results(&self) -> u32 {
        self.max_results.unwrap_or(DEFAULT_SEARCH_RESULTS)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "tool",
    content = "result",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RepositoryToolResult {
    ReadFile(ReadFileResult),
    SearchFiles(SearchFilesResult),
    SearchText(SearchTextResult),
    WriteFile(WriteFileResult),
}

impl RepositoryToolResult {
    /// Safe, bounded text for the durable activity timeline. Raw file content
    /// and search previews remain transient tool results.
    pub fn activity_summary(&self) -> String {
        match self {
            Self::ReadFile(result) => format!(
                "Read lines {}–{} from {}{}",
                result.start_line,
                result.end_line,
                result.path,
                if result.truncated {
                    " (more available)."
                } else {
                    "."
                }
            ),
            Self::SearchFiles(result) => format!(
                "Found {} workspace file{}{}",
                result.matches.len(),
                if result.matches.len() == 1 { "" } else { "s" },
                if result.truncated {
                    " (result limit reached)."
                } else {
                    "."
                }
            ),
            Self::SearchText(result) => format!(
                "Found {} text match{} across {} file{}{}",
                result.matches.len(),
                if result.matches.len() == 1 { "" } else { "es" },
                result.files_searched,
                if result.files_searched == 1 { "" } else { "s" },
                if result.truncated {
                    " (result limit reached)."
                } else {
                    "."
                }
            ),
            Self::WriteFile(result) => format!(
                "{} {} with an atomic workspace edit ({} UTF-8 bytes).",
                if result.created { "Created" } else { "Updated" },
                result.path,
                result.bytes_written,
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryToolExecution {
    pub result: RepositoryToolResult,
    pub activity_summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryToolFailureCode {
    Denied,
    ApprovalDeclined,
    Cancelled,
    InvalidRequest,
    Conflict,
    ExecutionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RepositoryToolOutcome {
    Success {
        result: RepositoryToolResult,
    },
    Failure {
        code: RepositoryToolFailureCode,
        message: String,
    },
}

impl RepositoryToolOutcome {
    pub fn success(result: RepositoryToolResult) -> Self {
        Self::Success { result }
    }

    pub fn failure(
        code: RepositoryToolFailureCode,
        message: impl Into<String>,
    ) -> Result<Self, ToolContractError> {
        let message = message.into();
        validate_text("toolOutcome.message", &message)?;
        Ok(Self::Failure { code, message })
    }

    pub const fn is_failure(&self) -> bool {
        matches!(self, Self::Failure { .. })
    }
}

impl RepositoryToolExecution {
    pub fn new(result: RepositoryToolResult) -> Self {
        let activity_summary = result.activity_summary();
        Self {
            result,
            activity_summary,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadFileResult {
    pub path: String,
    pub content: String,
    pub start_line: u32,
    pub end_line: u32,
    pub truncated: bool,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteFileResult {
    pub path: String,
    pub created: bool,
    pub bytes_written: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_sha256: Option<String>,
    pub after_sha256: String,
    pub unified_diff: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchFilesResult {
    pub pattern: String,
    pub matches: Vec<FileMatch>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMatch {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchTextResult {
    pub query: String,
    pub matches: Vec<TextMatch>,
    pub files_searched: u32,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMatch {
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ToolContractError {
    #[error("{field}: {message}")]
    InvalidField {
        field: &'static str,
        message: String,
    },
}

fn validate_result_limit(field: &'static str, value: Option<u32>) -> Result<(), ToolContractError> {
    if value == Some(0) || value.is_some_and(|count| count > MAX_SEARCH_RESULTS) {
        Err(ToolContractError::InvalidField {
            field,
            message: format!("result limit must be between 1 and {MAX_SEARCH_RESULTS}"),
        })
    } else {
        Ok(())
    }
}

fn validate_text(field: &'static str, value: &str) -> Result<(), ToolContractError> {
    if value.trim().is_empty() {
        Err(ToolContractError::InvalidField {
            field,
            message: "value cannot be blank".to_owned(),
        })
    } else if value.chars().count() > MAX_TOOL_TEXT_CHARS {
        Err(ToolContractError::InvalidField {
            field,
            message: format!("value cannot exceed {MAX_TOOL_TEXT_CHARS} characters"),
        })
    } else if value.chars().any(char::is_control) {
        Err(ToolContractError::InvalidField {
            field,
            message: "value cannot contain control characters".to_owned(),
        })
    } else {
        Ok(())
    }
}

fn validate_path(field: &'static str, value: &str) -> Result<(), ToolContractError> {
    validate_text(field, value)?;
    if value.chars().count() > MAX_TOOL_PATH_CHARS {
        Err(ToolContractError::InvalidField {
            field,
            message: format!("path cannot exceed {MAX_TOOL_PATH_CHARS} characters"),
        })
    } else {
        Ok(())
    }
}

fn parse_arguments<T: DeserializeOwned>(arguments: &str) -> Result<T, ToolContractError> {
    serde_json::from_str(arguments).map_err(|_| ToolContractError::InvalidField {
        field: "tool.arguments",
        message: "arguments do not match the selected repository tool schema".to_owned(),
    })
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_tool_schema_uses_stable_tagged_names() {
        let request = RepositoryToolRequest::SearchText(SearchTextRequest {
            query: "PermissionEngine".to_owned(),
            path: Some("crates/kiln-core".to_owned()),
            case_sensitive: true,
            max_results: Some(25),
        });

        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["tool"], "search_text");
        assert_eq!(value["input"]["caseSensitive"], true);
        assert_eq!(value["input"]["maxResults"], 25);
        request.validate().unwrap();
    }

    #[test]
    fn provider_calls_use_strict_allowlisted_schemas() {
        let request = RepositoryToolRequest::from_provider_call(
            "read_file",
            r#"{"path":"src/lib.rs","startLine":2,"lineCount":25}"#,
        )
        .unwrap();
        assert_eq!(request.name(), "read_file");

        assert!(RepositoryToolRequest::from_provider_call(
            "read_file",
            r#"{"path":"src/lib.rs","unexpected":true}"#
        )
        .is_err());
        assert!(
            RepositoryToolRequest::from_provider_call("shell", r#"{"command":"whoami"}"#).is_err()
        );
        assert!(RepositoryToolRequest::from_provider_call("read_file", "[]").is_err());
    }

    #[test]
    fn repository_tool_catalog_is_strict_and_complete() {
        let definitions = repository_tool_definitions();
        assert_eq!(definitions.len(), 4);
        assert_eq!(
            definitions.iter().map(|tool| tool.name).collect::<Vec<_>>(),
            vec!["read_file", "search_files", "search_text", "write_file"]
        );
        assert!(definitions
            .iter()
            .all(|tool| tool.input_schema["additionalProperties"] == false));
    }

    #[test]
    fn request_bounds_reject_unbounded_outputs() {
        let request = RepositoryToolRequest::ReadFile(ReadFileRequest {
            path: "src/lib.rs".to_owned(),
            start_line: Some(1),
            line_count: Some(MAX_READ_LINE_COUNT + 1),
        });

        assert!(matches!(
            request.validate(),
            Err(ToolContractError::InvalidField {
                field: "readFile.lineCount",
                ..
            })
        ));
    }

    #[test]
    fn write_requests_require_valid_optimistic_hashes_and_bounded_content() {
        let request = RepositoryToolRequest::WriteFile(WriteFileRequest {
            path: "src/lib.rs".to_owned(),
            content: "updated\n".to_owned(),
            expected_sha256: Some("not-a-hash".to_owned()),
        });
        assert!(request.validate().is_err());

        let request = RepositoryToolRequest::WriteFile(WriteFileRequest {
            path: "src/lib.rs".to_owned(),
            content: "updated\n".to_owned(),
            expected_sha256: Some("a".repeat(64)),
        });
        request.validate().unwrap();
        assert_eq!(request.name(), "write_file");

        let oversized = RepositoryToolRequest::WriteFile(WriteFileRequest {
            path: "src/lib.rs".to_owned(),
            content: "x".repeat(MAX_WRITE_BYTES + 1),
            expected_sha256: Some("a".repeat(64)),
        });
        assert!(oversized.validate().is_err());
    }

    #[test]
    fn durable_summaries_exclude_raw_content_and_queries() {
        let result = RepositoryToolResult::SearchText(SearchTextResult {
            query: "secret-token".to_owned(),
            matches: vec![TextMatch {
                path: "src/lib.rs".to_owned(),
                line: 4,
                column: 2,
                preview: "secret-token".to_owned(),
            }],
            files_searched: 3,
            truncated: false,
        });

        let summary = result.activity_summary();
        assert!(!summary.contains("secret-token"));
        assert!(!summary.contains("preview"));
        assert_eq!(summary, "Found 1 text match across 3 files.");

        let execution = RepositoryToolExecution::new(result);
        let value = serde_json::to_value(execution).unwrap();
        assert_eq!(value["activitySummary"], summary);
        assert_eq!(value["result"]["result"]["query"], "secret-token");

        let search = RepositoryToolRequest::SearchText(SearchTextRequest {
            query: "private needle".to_owned(),
            path: None,
            case_sensitive: false,
            max_results: None,
        });
        assert!(!search.proposal_summary().contains("private needle"));

        let write = RepositoryToolRequest::WriteFile(WriteFileRequest {
            path: "src/lib.rs".to_owned(),
            content: "private replacement".to_owned(),
            expected_sha256: None,
        });
        assert_eq!(
            write.proposal_summary(),
            "Write src/lib.rs after explicit approval."
        );
        assert!(!write.proposal_summary().contains("private replacement"));
    }
}
