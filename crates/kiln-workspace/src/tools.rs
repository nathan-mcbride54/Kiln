use std::{
    collections::HashMap,
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};

use kiln_core::{
    ActionOrigin, ActionProposal, FileMatch, GuardedExecutionError, OriginMatcher, PathOperation,
    PermissionDecision, PermissionEngine, PermissionResource, PolicyContext, PolicyEffect,
    PolicyRule, PolicyTarget, ProjectSnapshot, ReadFileRequest, ReadFileResult,
    RepositoryToolRequest, RepositoryToolResult, ResourceMatcher, SearchFilesRequest,
    SearchFilesResult, SearchTextRequest, SearchTextResult, TextMatch, ToolContractError,
};
use kiln_platform::CancellationToken;
use thiserror::Error;

use super::{GitRepositoryInspector, RepositoryError};

const MAX_READ_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_READ_RESULT_BYTES: usize = 256 * 1024;
const MAX_SEARCH_FILE_BYTES: u64 = 1024 * 1024;
const MAX_SEARCH_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PREVIEW_CHARS: usize = 400;

#[derive(Debug, Clone)]
pub struct WorkspaceToolHost {
    project_id: String,
    root: PathBuf,
    root_text: String,
    repositories: GitRepositoryInspector,
}

impl WorkspaceToolHost {
    pub fn new(
        project_id: impl Into<String>,
        root: impl AsRef<Path>,
    ) -> Result<Self, WorkspaceToolError> {
        let project_id = project_id.into();
        if project_id.trim().is_empty() {
            return Err(WorkspaceToolError::InvalidWorkspace);
        }
        let root = root.as_ref();
        if !root.is_absolute() {
            return Err(WorkspaceToolError::InvalidWorkspace);
        }
        let root = dunce::canonicalize(root).map_err(WorkspaceToolError::Io)?;
        if !root.is_dir() || root.parent().is_none() {
            return Err(WorkspaceToolError::InvalidWorkspace);
        }
        let root_text = root
            .to_str()
            .ok_or(WorkspaceToolError::NonUnicodePath)?
            .to_owned();

        Ok(Self {
            project_id,
            root,
            root_text,
            repositories: GitRepositoryInspector::default(),
        })
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn default_policy(&self) -> Result<PermissionEngine, WorkspaceToolError> {
        let target = PolicyTarget::Project {
            project_id: self.project_id.clone(),
        };
        let mut rules = ["read_file", "search_files", "search_text"]
            .into_iter()
            .map(|name| PolicyRule {
                rule_id: format!("{}:{name}", self.project_id),
                target: target.clone(),
                origin: OriginMatcher::Core,
                resource: ResourceMatcher::Tool {
                    name: name.to_owned(),
                },
                effect: PolicyEffect::Allow,
                reason: "Read-only repository tools are allowed for the selected project."
                    .to_owned(),
            })
            .collect::<Vec<_>>();
        rules.push(PolicyRule {
            rule_id: format!("{}:workspace-read", self.project_id),
            target,
            origin: OriginMatcher::Core,
            resource: ResourceMatcher::PathPrefix {
                path: self.root_text.clone(),
                operations: vec![PathOperation::Read, PathOperation::Search],
            },
            effect: PolicyEffect::Allow,
            reason: "Reads and searches are allowed inside the selected workspace.".to_owned(),
        });
        PermissionEngine::new(rules).map_err(WorkspaceToolError::Policy)
    }

    pub fn execute(
        &self,
        permissions: &mut PermissionEngine,
        tool_call_id: &str,
        request: RepositoryToolRequest,
        cancellation: &CancellationToken,
    ) -> Result<RepositoryToolResult, WorkspaceToolError> {
        if tool_call_id.trim().is_empty() {
            return Err(WorkspaceToolError::InvalidToolCall);
        }
        request.validate()?;
        if cancellation.is_cancelled() {
            return Err(WorkspaceToolError::Cancelled);
        }

        let context = PolicyContext {
            project_id: Some(self.project_id.clone()),
            ..PolicyContext::default()
        };
        let tool = ActionProposal::new(
            format!("{tool_call_id}:tool"),
            ActionOrigin::Core,
            PermissionResource::Tool {
                name: request.name().to_owned(),
            },
            "Inspect the selected repository with a read-only tool.",
        )
        .map_err(WorkspaceToolError::Policy)?;
        permissions.execute(&tool, &context, || ())?;

        match request {
            RepositoryToolRequest::ReadFile(request) => {
                let target = self.resolve_existing(&request.path)?;
                if !target.is_file() {
                    return Err(WorkspaceToolError::NotFile);
                }
                self.execute_path(
                    permissions,
                    &context,
                    tool_call_id,
                    PathOperation::Read,
                    &target,
                    || self.read_file(request, &target, cancellation),
                )
            }
            RepositoryToolRequest::SearchFiles(request) => self.execute_path(
                permissions,
                &context,
                tool_call_id,
                PathOperation::Search,
                &self.root,
                || self.search_files(request, cancellation),
            ),
            RepositoryToolRequest::SearchText(request) => {
                let scope = match request.path.as_deref() {
                    Some(path) => self.resolve_existing(path)?,
                    None => self.root.clone(),
                };
                self.execute_path(
                    permissions,
                    &context,
                    tool_call_id,
                    PathOperation::Search,
                    &scope,
                    || self.search_text(request, &scope, cancellation),
                )
            }
        }
    }

    fn execute_path(
        &self,
        permissions: &mut PermissionEngine,
        context: &PolicyContext,
        tool_call_id: &str,
        operation: PathOperation,
        path: &Path,
        action: impl FnOnce() -> Result<RepositoryToolResult, WorkspaceToolError>,
    ) -> Result<RepositoryToolResult, WorkspaceToolError> {
        let path = path
            .to_str()
            .ok_or(WorkspaceToolError::NonUnicodePath)?
            .to_owned();
        let proposal = ActionProposal::new(
            format!("{tool_call_id}:path"),
            ActionOrigin::Core,
            PermissionResource::Path { operation, path },
            "Access a path inside the selected repository.",
        )
        .map_err(WorkspaceToolError::Policy)?;
        permissions.execute(&proposal, context, action)?
    }

    fn resolve_existing(&self, relative: &str) -> Result<PathBuf, WorkspaceToolError> {
        let relative = Path::new(relative);
        if relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(WorkspaceToolError::OutsideWorkspace);
        }
        let candidate =
            dunce::canonicalize(self.root.join(relative)).map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => WorkspaceToolError::PathMissing,
                _ => WorkspaceToolError::Io(error),
            })?;
        if !path_is_within(&candidate, &self.root) {
            return Err(WorkspaceToolError::OutsideWorkspace);
        }
        Ok(candidate)
    }

    fn read_file(
        &self,
        request: ReadFileRequest,
        path: &Path,
        cancellation: &CancellationToken,
    ) -> Result<RepositoryToolResult, WorkspaceToolError> {
        if cancellation.is_cancelled() {
            return Err(WorkspaceToolError::Cancelled);
        }
        let metadata = fs::metadata(path).map_err(WorkspaceToolError::Io)?;
        if metadata.len() > MAX_READ_FILE_BYTES {
            return Err(WorkspaceToolError::FileTooLarge);
        }
        let bytes = read_bounded(path, MAX_READ_FILE_BYTES)?;
        if bytes.contains(&0) {
            return Err(WorkspaceToolError::BinaryFile);
        }
        let text = String::from_utf8(bytes).map_err(|_| WorkspaceToolError::BinaryFile)?;
        let start_line = request.effective_start_line();
        let line_count = request.effective_line_count();
        let mut content = String::new();
        let mut end_line = start_line.saturating_sub(1);
        let mut selected = 0_u32;
        let mut truncated = false;

        for (index, line) in text.split_inclusive('\n').enumerate() {
            if cancellation.is_cancelled() {
                return Err(WorkspaceToolError::Cancelled);
            }
            let line_number = u32::try_from(index + 1).unwrap_or(u32::MAX);
            if line_number < start_line {
                continue;
            }
            if selected >= line_count {
                truncated = true;
                break;
            }
            if content.len().saturating_add(line.len()) > MAX_READ_RESULT_BYTES {
                truncated = true;
                break;
            }
            content.push_str(line);
            end_line = line_number;
            selected += 1;
        }

        if selected == 0 && (start_line > 1 || !text.is_empty()) {
            return Err(WorkspaceToolError::StartPastEnd);
        }
        let path = relative_text(&self.root, path)?;
        Ok(RepositoryToolResult::ReadFile(ReadFileResult {
            path,
            content,
            start_line,
            end_line,
            truncated,
        }))
    }

    fn search_files(
        &self,
        request: SearchFilesRequest,
        cancellation: &CancellationToken,
    ) -> Result<RepositoryToolResult, WorkspaceToolError> {
        let files = self.repository_files(cancellation)?;
        let max_results = request.effective_max_results() as usize;
        let mut matches = Vec::new();
        let mut truncated = false;

        for path in files {
            if cancellation.is_cancelled() {
                return Err(WorkspaceToolError::Cancelled);
            }
            if file_pattern_matches(&request.pattern, &path) {
                if matches.len() == max_results {
                    truncated = true;
                    break;
                }
                matches.push(FileMatch { path });
            }
        }
        Ok(RepositoryToolResult::SearchFiles(SearchFilesResult {
            pattern: request.pattern,
            matches,
            truncated,
        }))
    }

    fn search_text(
        &self,
        request: SearchTextRequest,
        scope: &Path,
        cancellation: &CancellationToken,
    ) -> Result<RepositoryToolResult, WorkspaceToolError> {
        if !scope.is_dir() && !scope.is_file() {
            return Err(WorkspaceToolError::InvalidSearchScope);
        }
        let files = self.repository_files(cancellation)?;
        let max_results = request.effective_max_results() as usize;
        let mut matches = Vec::new();
        let mut files_searched = 0_u32;
        let mut bytes_searched = 0_u64;
        let mut truncated = false;
        let needle = if request.case_sensitive {
            request.query.clone()
        } else {
            request.query.to_ascii_lowercase()
        };

        for relative in files {
            if cancellation.is_cancelled() {
                return Err(WorkspaceToolError::Cancelled);
            }
            let candidate = self
                .root
                .join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
            let metadata = match fs::symlink_metadata(&candidate) {
                Ok(metadata) if metadata.file_type().is_symlink() => continue,
                Ok(metadata) if metadata.is_file() => metadata,
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(WorkspaceToolError::Io(error)),
            };
            if metadata.len() > MAX_SEARCH_FILE_BYTES {
                continue;
            }
            if bytes_searched.saturating_add(metadata.len()) > MAX_SEARCH_TOTAL_BYTES {
                truncated = true;
                break;
            }
            let canonical = dunce::canonicalize(&candidate).map_err(WorkspaceToolError::Io)?;
            if !path_is_within(&canonical, &self.root)
                || (scope.is_file() && canonical != scope)
                || (scope.is_dir() && !path_is_within(&canonical, scope))
            {
                continue;
            }
            let bytes = match read_bounded(&canonical, MAX_SEARCH_FILE_BYTES) {
                Ok(bytes) => bytes,
                Err(WorkspaceToolError::FileTooLarge) => continue,
                Err(error) => return Err(error),
            };
            bytes_searched = bytes_searched.saturating_add(bytes.len() as u64);
            if bytes.contains(&0) {
                continue;
            }
            let Ok(text) = std::str::from_utf8(&bytes) else {
                continue;
            };
            files_searched = files_searched.saturating_add(1);

            for (line_index, line) in text.lines().enumerate() {
                let haystack = if request.case_sensitive {
                    line.to_owned()
                } else {
                    line.to_ascii_lowercase()
                };
                let Some(byte_column) = haystack.find(&needle) else {
                    continue;
                };
                if matches.len() == max_results {
                    truncated = true;
                    break;
                }
                let column = line[..byte_column].chars().count().saturating_add(1);
                matches.push(TextMatch {
                    path: relative.clone(),
                    line: u32::try_from(line_index + 1).unwrap_or(u32::MAX),
                    column: u32::try_from(column).unwrap_or(u32::MAX),
                    preview: truncate_chars(line.trim(), MAX_PREVIEW_CHARS),
                });
            }
            if truncated {
                break;
            }
        }

        Ok(RepositoryToolResult::SearchText(SearchTextResult {
            query: request.query,
            matches,
            files_searched,
            truncated,
        }))
    }

    fn repository_files(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Vec<String>, WorkspaceToolError> {
        let output = self
            .repositories
            .required_output_cancellable(
                &self.root,
                [
                    "ls-files",
                    "-z",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                ],
                cancellation,
            )
            .map_err(|error| match error {
                RepositoryError::InspectionCancelled => WorkspaceToolError::Cancelled,
                other => WorkspaceToolError::RepositoryIndex(other),
            })?;
        let mut files = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                let path =
                    std::str::from_utf8(entry).map_err(|_| WorkspaceToolError::NonUnicodePath)?;
                validate_git_relative_path(path)?;
                Ok(path.replace('\\', "/"))
            })
            .collect::<Result<Vec<_>, WorkspaceToolError>>()?;
        files.sort();
        files.dedup();
        Ok(files)
    }
}

#[derive(Debug)]
struct RegisteredWorkspace {
    host: WorkspaceToolHost,
    permissions: PermissionEngine,
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceToolService {
    workspaces: Arc<Mutex<HashMap<String, Arc<Mutex<RegisteredWorkspace>>>>>,
}

impl WorkspaceToolService {
    pub fn register(&self, project: &ProjectSnapshot) -> Result<(), WorkspaceToolError> {
        let host = WorkspaceToolHost::new(&project.project_id, &project.root)?;
        let permissions = host.default_policy()?;
        let registered = Arc::new(Mutex::new(RegisteredWorkspace { host, permissions }));
        self.workspaces
            .lock()
            .map_err(|_| WorkspaceToolError::Unavailable)?
            .insert(project.project_id.clone(), registered);
        Ok(())
    }

    pub fn execute(
        &self,
        project_id: &str,
        tool_call_id: &str,
        request: RepositoryToolRequest,
        cancellation: &CancellationToken,
    ) -> Result<RepositoryToolResult, WorkspaceToolError> {
        let workspace = self
            .workspaces
            .lock()
            .map_err(|_| WorkspaceToolError::Unavailable)?
            .get(project_id)
            .cloned()
            .ok_or(WorkspaceToolError::WorkspaceNotRegistered)?;
        let mut workspace = workspace
            .lock()
            .map_err(|_| WorkspaceToolError::Unavailable)?;
        let RegisteredWorkspace { host, permissions } = &mut *workspace;
        host.execute(permissions, tool_call_id, request, cancellation)
    }
}

#[derive(Debug, Error)]
pub enum WorkspaceToolError {
    #[error("the workspace is invalid")]
    InvalidWorkspace,
    #[error("the tool call identifier is invalid")]
    InvalidToolCall,
    #[error(transparent)]
    InvalidRequest(#[from] ToolContractError),
    #[error("the requested path escapes the selected workspace")]
    OutsideWorkspace,
    #[error("the requested path does not exist")]
    PathMissing,
    #[error("the requested path is not a file")]
    NotFile,
    #[error("the requested search scope is not a file or directory")]
    InvalidSearchScope,
    #[error("the requested line range starts after the end of the file")]
    StartPastEnd,
    #[error("the file is too large for a bounded read")]
    FileTooLarge,
    #[error("the file is binary or is not valid UTF-8 text")]
    BinaryFile,
    #[error("a repository path cannot be represented safely")]
    NonUnicodePath,
    #[error("the workspace tool service is unavailable")]
    Unavailable,
    #[error("the selected workspace has not been registered")]
    WorkspaceNotRegistered,
    #[error("the tool was cancelled")]
    Cancelled,
    #[error("Git could not list repository files")]
    RepositoryIndex(#[source] RepositoryError),
    #[error("the filesystem operation failed")]
    Io(#[source] std::io::Error),
    #[error(transparent)]
    Policy(#[from] kiln_core::PolicyError),
    #[error(transparent)]
    NotAuthorized(#[from] GuardedExecutionError),
}

impl WorkspaceToolError {
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::InvalidWorkspace => "The selected workspace is no longer valid.",
            Self::InvalidToolCall | Self::InvalidRequest(_) => {
                "The repository tool request is invalid."
            }
            Self::OutsideWorkspace => {
                "That path is outside the selected workspace and cannot be accessed."
            }
            Self::PathMissing => "That workspace path no longer exists.",
            Self::NotFile => "Choose a file for the read-file tool.",
            Self::InvalidSearchScope => {
                "Choose a file or directory inside the workspace to search."
            }
            Self::StartPastEnd => "The requested starting line is past the end of the file.",
            Self::FileTooLarge => "That file is too large for a bounded tool read.",
            Self::BinaryFile => "That file is binary or is not valid UTF-8 text.",
            Self::NonUnicodePath => {
                "A repository path cannot be represented safely by the desktop interface."
            }
            Self::Unavailable => "Repository tools are temporarily unavailable.",
            Self::WorkspaceNotRegistered => {
                "Open this repository again before using repository tools."
            }
            Self::Cancelled => "The repository tool was cancelled.",
            Self::RepositoryIndex(_) => {
                "Git could not produce a safe list of files for this repository."
            }
            Self::Io(_) => "Kiln could not read that workspace path.",
            Self::Policy(_) => "Kiln could not evaluate the repository tool policy.",
            Self::NotAuthorized(GuardedExecutionError::NotAuthorized(
                PermissionDecision::Ask { .. },
            )) => "This repository tool requires approval before it can run.",
            Self::NotAuthorized(_) => "Repository policy denied this tool action.",
        }
    }
}

fn validate_git_relative_path(path: &str) -> Result<(), WorkspaceToolError> {
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        Err(WorkspaceToolError::OutsideWorkspace)
    } else {
        Ok(())
    }
}

fn relative_text(root: &Path, path: &Path) -> Result<String, WorkspaceToolError> {
    path.strip_prefix(root)
        .map_err(|_| WorkspaceToolError::OutsideWorkspace)?
        .to_str()
        .ok_or(WorkspaceToolError::NonUnicodePath)
        .map(|path| path.replace('\\', "/"))
}

fn path_is_within(candidate: &Path, root: &Path) -> bool {
    if cfg!(windows) {
        let candidate = candidate
            .to_string_lossy()
            .replace('\\', "/")
            .to_lowercase();
        let root = root.to_string_lossy().replace('\\', "/").to_lowercase();
        candidate == root || candidate.starts_with(&format!("{root}/"))
    } else {
        candidate == root || candidate.starts_with(root)
    }
}

fn file_pattern_matches(pattern: &str, path: &str) -> bool {
    let normalized_pattern = pattern.replace('\\', "/");
    if normalized_pattern.contains(['*', '?']) {
        wildcard_match(&normalized_pattern, path)
    } else if cfg!(windows) {
        path.to_ascii_lowercase()
            .contains(&normalized_pattern.to_ascii_lowercase())
    } else {
        path.contains(&normalized_pattern)
    }
}

fn wildcard_match(pattern: &str, candidate: &str) -> bool {
    let pattern = if cfg!(windows) {
        pattern.to_ascii_lowercase()
    } else {
        pattern.to_owned()
    };
    let candidate = if cfg!(windows) {
        candidate.to_ascii_lowercase()
    } else {
        candidate.to_owned()
    };
    let pattern = pattern.chars().collect::<Vec<_>>();
    let candidate = candidate.chars().collect::<Vec<_>>();
    let mut previous = vec![false; candidate.len() + 1];
    previous[0] = true;

    for token in pattern {
        let mut current = vec![false; candidate.len() + 1];
        if token == '*' {
            current[0] = previous[0];
        }
        for index in 1..=candidate.len() {
            current[index] = if token == '*' {
                previous[index] || current[index - 1]
            } else {
                previous[index - 1] && (token == '?' || token == candidate[index - 1])
            };
        }
        previous = current;
    }
    previous[candidate.len()]
}

fn truncate_chars(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(limit).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, WorkspaceToolError> {
    let file = File::open(path).map_err(WorkspaceToolError::Io)?;
    let mut bytes = Vec::with_capacity(usize::try_from(limit.min(64 * 1024)).unwrap_or(64 * 1024));
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(WorkspaceToolError::Io)?;
    if bytes.len() as u64 > limit {
        Err(WorkspaceToolError::FileTooLarge)
    } else {
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    use kiln_core::{PolicyEffect, ResourceMatcher};

    use super::*;

    fn temporary_repository(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("kiln-tools-{label}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(root.join("src")).unwrap();
        run(&root, ["init", "--quiet"]);
        fs::write(
            root.join("src/lib.rs"),
            "pub struct Kiln;\nimpl Kiln {\n    pub fn ready() {}\n}\n",
        )
        .unwrap();
        fs::write(root.join("README.md"), "# Kiln tools\n").unwrap();
        fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(root.join("ignored.txt"), "do not search\n").unwrap();
        run(&root, ["add", "src/lib.rs", "README.md", ".gitignore"]);
        root
    }

    fn run<const N: usize>(root: &Path, args: [&str; N]) {
        assert!(Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap()
            .success());
    }

    fn remove_repository(root: &Path) {
        if root.starts_with(std::env::temp_dir()) {
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn reads_and_searches_only_repository_files_with_typed_results() {
        let root = temporary_repository("happy");
        let project = ProjectSnapshot {
            project_id: "project-tools".to_owned(),
            display_name: "tools".to_owned(),
            root: dunce::canonicalize(&root)
                .unwrap()
                .to_string_lossy()
                .to_string(),
            branch: None,
            head: None,
            status: kiln_core::RepositoryStatus::default(),
            defaults: kiln_core::ProjectDefaults::default(),
        };
        let service = WorkspaceToolService::default();
        service.register(&project).unwrap();
        let cancellation = CancellationToken::default();

        let read = service
            .execute(
                &project.project_id,
                "tool-read",
                RepositoryToolRequest::ReadFile(ReadFileRequest {
                    path: "src/lib.rs".to_owned(),
                    start_line: Some(2),
                    line_count: Some(2),
                }),
                &cancellation,
            )
            .unwrap();
        let RepositoryToolResult::ReadFile(read) = read else {
            panic!("expected read result");
        };
        assert_eq!(read.path, "src/lib.rs");
        assert!(read.content.contains("impl Kiln"));
        assert_eq!((read.start_line, read.end_line), (2, 3));
        assert!(read.truncated);

        let files = service
            .execute(
                &project.project_id,
                "tool-files",
                RepositoryToolRequest::SearchFiles(SearchFilesRequest {
                    pattern: "*.rs".to_owned(),
                    max_results: None,
                }),
                &cancellation,
            )
            .unwrap();
        let RepositoryToolResult::SearchFiles(files) = files else {
            panic!("expected file search result");
        };
        assert_eq!(
            files.matches,
            vec![FileMatch {
                path: "src/lib.rs".to_owned()
            }]
        );
        assert!(!files
            .matches
            .iter()
            .any(|entry| entry.path == "ignored.txt"));

        let text = service
            .execute(
                &project.project_id,
                "tool-text",
                RepositoryToolRequest::SearchText(SearchTextRequest {
                    query: "ready".to_owned(),
                    path: Some("src".to_owned()),
                    case_sensitive: true,
                    max_results: None,
                }),
                &cancellation,
            )
            .unwrap();
        let RepositoryToolResult::SearchText(text) = text else {
            panic!("expected text search result");
        };
        assert_eq!(text.files_searched, 1);
        assert_eq!(text.matches[0].line, 3);
        assert_eq!(text.matches[0].path, "src/lib.rs");
        remove_repository(&root);
    }

    #[test]
    fn rejects_workspace_traversal_before_reading() {
        let root = temporary_repository("traversal");
        let host = WorkspaceToolHost::new("project-tools", &root).unwrap();
        let mut permissions = host.default_policy().unwrap();
        let error = host
            .execute(
                &mut permissions,
                "tool-escape",
                RepositoryToolRequest::ReadFile(ReadFileRequest {
                    path: "../outside.txt".to_owned(),
                    start_line: None,
                    line_count: None,
                }),
                &CancellationToken::default(),
            )
            .unwrap_err();

        assert!(matches!(error, WorkspaceToolError::OutsideWorkspace));
        remove_repository(&root);
    }

    #[test]
    fn denied_tool_never_reaches_the_filesystem_operation() {
        let root = temporary_repository("deny");
        let host = WorkspaceToolHost::new("project-tools", &root).unwrap();
        let mut permissions = PermissionEngine::new(vec![PolicyRule {
            rule_id: "deny-read".to_owned(),
            target: PolicyTarget::Project {
                project_id: "project-tools".to_owned(),
            },
            origin: OriginMatcher::Core,
            resource: ResourceMatcher::Tool {
                name: "read_file".to_owned(),
            },
            effect: PolicyEffect::Deny,
            reason: "test denial".to_owned(),
        }])
        .unwrap();
        let error = host
            .execute(
                &mut permissions,
                "tool-denied",
                RepositoryToolRequest::ReadFile(ReadFileRequest {
                    path: "src/lib.rs".to_owned(),
                    start_line: None,
                    line_count: None,
                }),
                &CancellationToken::default(),
            )
            .unwrap_err();

        assert!(matches!(
            error,
            WorkspaceToolError::NotAuthorized(GuardedExecutionError::NotAuthorized(
                PermissionDecision::Deny { .. }
            ))
        ));
        remove_repository(&root);
    }

    #[test]
    fn cancelled_search_stops_before_returning_results() {
        let root = temporary_repository("cancel");
        let host = WorkspaceToolHost::new("project-tools", &root).unwrap();
        let mut permissions = host.default_policy().unwrap();
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        let error = host
            .execute(
                &mut permissions,
                "tool-cancelled",
                RepositoryToolRequest::SearchFiles(SearchFilesRequest {
                    pattern: "*".to_owned(),
                    max_results: None,
                }),
                &cancellation,
            )
            .unwrap_err();

        assert!(matches!(error, WorkspaceToolError::Cancelled));
        remove_repository(&root);
    }

    #[test]
    fn wildcard_matching_is_bounded_and_predictable() {
        assert!(wildcard_match("*.rs", "src/lib.rs"));
        assert!(wildcard_match("src/?.s", "src/a.s"));
        assert!(!wildcard_match("src/*.ts", "src/lib.rs"));
    }
}
