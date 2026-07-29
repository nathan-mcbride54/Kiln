use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_READ_LINE_COUNT: u32 = 200;
pub const MAX_READ_LINE_COUNT: u32 = 1_000;
pub const DEFAULT_SEARCH_RESULTS: u32 = 100;
pub const MAX_SEARCH_RESULTS: u32 = 500;

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
}

impl RepositoryToolRequest {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ReadFile(_) => "read_file",
            Self::SearchFiles(_) => "search_files",
            Self::SearchText(_) => "search_text",
        }
    }

    pub fn validate(&self) -> Result<(), ToolContractError> {
        match self {
            Self::ReadFile(request) => request.validate(),
            Self::SearchFiles(request) => request.validate(),
            Self::SearchText(request) => request.validate(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadFileRequest {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_count: Option<u32>,
}

impl ReadFileRequest {
    pub fn validate(&self) -> Result<(), ToolContractError> {
        validate_text("readFile.path", &self.path)?;
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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
            validate_text("searchText.path", path)?;
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryToolExecution {
    pub result: RepositoryToolResult,
    pub activity_summary: String,
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
    } else {
        Ok(())
    }
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
    }
}
