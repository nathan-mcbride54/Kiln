//! Tauri-free Git repository discovery for direct Kiln workspaces.
//!
//! The inspector invokes only built-in, read-only Git commands. Repository
//! hooks and filesystem monitors are disabled, Git's ownership checks remain
//! enabled, and no remote configuration is read into the application model.

mod tools;

use std::{
    ffi::{OsStr, OsString},
    io::{Read, Result as IoResult},
    path::Path,
    process::{Command, ExitStatus, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use kiln_core::{ProjectDefaults, ProjectSnapshot, RepositoryStatus};
use kiln_platform::CancellationToken;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub use tools::{WorkspaceToolError, WorkspaceToolHost, WorkspaceToolService};

#[derive(Debug, Clone)]
pub struct GitRepositoryInspector {
    executable: OsString,
    timeout: Duration,
}

impl Default for GitRepositoryInspector {
    fn default() -> Self {
        Self {
            executable: OsString::from("git"),
            timeout: Duration::from_secs(15),
        }
    }
}

impl GitRepositoryInspector {
    pub fn with_executable(executable: impl Into<OsString>) -> Self {
        Self {
            executable: executable.into(),
            timeout: Duration::from_secs(15),
        }
    }

    pub fn inspect(
        &self,
        selection: impl AsRef<Path>,
        defaults: ProjectDefaults,
    ) -> Result<ProjectSnapshot, RepositoryError> {
        defaults
            .validate()
            .map_err(|_| RepositoryError::InvalidDefaults)?;
        let selection = selection.as_ref();
        if !selection.is_absolute() {
            return Err(RepositoryError::SelectionNotAbsolute);
        }
        if !selection.exists() {
            return Err(RepositoryError::SelectionMissing);
        }
        if !selection.is_dir() {
            return Err(RepositoryError::SelectionNotDirectory);
        }
        let selection =
            dunce::canonicalize(selection).map_err(RepositoryError::SelectionInaccessible)?;

        let inside = self.required_text(&selection, ["rev-parse", "--is-inside-work-tree"])?;
        if inside.trim() != "true" {
            let bare = self.required_text(&selection, ["rev-parse", "--is-bare-repository"])?;
            return if bare.trim() == "true" {
                Err(RepositoryError::BareRepository)
            } else {
                Err(RepositoryError::NotRepository)
            };
        }

        let root_output = self.required_text(
            &selection,
            ["rev-parse", "--path-format=absolute", "--show-toplevel"],
        )?;
        let root_text = strip_one_line_ending(&root_output);
        let root = dunce::canonicalize(root_text)
            .map_err(|_| RepositoryError::MalformedGitOutput("repository root"))?;
        if root.parent().is_none() {
            return Err(RepositoryError::FilesystemRoot);
        }
        let root_text = root
            .to_str()
            .ok_or(RepositoryError::NonUnicodePath)?
            .to_owned();
        let display_name = root
            .file_name()
            .and_then(OsStr::to_str)
            .filter(|name| !name.trim().is_empty())
            .ok_or(RepositoryError::NonUnicodePath)?
            .to_owned();

        let branch =
            self.optional_text(&root, ["symbolic-ref", "--quiet", "--short", "HEAD"], &[1])?;
        let head = self.optional_text(&root, ["rev-parse", "--verify", "HEAD"], &[128])?;
        let status_output = self.required_output(
            &root,
            [
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=normal",
            ],
        )?;
        let status = parse_porcelain_v2(&status_output.stdout)?;

        Ok(ProjectSnapshot {
            project_id: project_id(&root_text),
            display_name,
            root: root_text,
            branch: branch.map(|value| strip_one_line_ending(&value).to_owned()),
            head: head.map(|value| strip_one_line_ending(&value).to_owned()),
            status,
            defaults,
        })
    }

    pub fn refresh(&self, project: &ProjectSnapshot) -> Result<ProjectSnapshot, RepositoryError> {
        self.inspect(&project.root, project.defaults.clone())
    }

    fn required_text<const N: usize>(
        &self,
        repository: &Path,
        args: [&str; N],
    ) -> Result<String, RepositoryError> {
        let output = self.required_output(repository, args)?;
        String::from_utf8(output.stdout).map_err(|_| RepositoryError::NonUnicodePath)
    }

    fn required_output<const N: usize>(
        &self,
        repository: &Path,
        args: [&str; N],
    ) -> Result<Output, RepositoryError> {
        let output = self.output(repository, args)?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(classify_git_failure(&output))
        }
    }

    fn required_output_cancellable<const N: usize>(
        &self,
        repository: &Path,
        args: [&str; N],
        cancellation: &CancellationToken,
    ) -> Result<Output, RepositoryError> {
        let output = self.output_inner(repository, args, Some(cancellation))?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(classify_git_failure(&output))
        }
    }

    fn optional_text<const N: usize>(
        &self,
        repository: &Path,
        args: [&str; N],
        empty_statuses: &[i32],
    ) -> Result<Option<String>, RepositoryError> {
        let output = self.output(repository, args)?;
        if output.status.success() {
            let value =
                String::from_utf8(output.stdout).map_err(|_| RepositoryError::NonUnicodePath)?;
            return Ok(Some(value));
        }
        if output
            .status
            .code()
            .is_some_and(|code| empty_statuses.contains(&code))
        {
            return Ok(None);
        }
        Err(classify_git_failure(&output))
    }

    fn output<const N: usize>(
        &self,
        repository: &Path,
        args: [&str; N],
    ) -> Result<Output, RepositoryError> {
        self.output_inner(repository, args, None)
    }

    fn output_inner<const N: usize>(
        &self,
        repository: &Path,
        args: [&str; N],
        cancellation: Option<&CancellationToken>,
    ) -> Result<Output, RepositoryError> {
        let mut command = Command::new(&self.executable);
        command
            .arg("-C")
            .arg(repository)
            .arg("--no-optional-locks")
            .arg("-c")
            .arg("color.ui=false")
            .arg("-c")
            .arg("core.fsmonitor=false")
            .arg("-c")
            .arg("core.hooksPath=")
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_PAGER", "cat")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command
            .spawn()
            .map_err(|_| RepositoryError::GitUnavailable)?;
        let stdout = child
            .stdout
            .take()
            .ok_or(RepositoryError::GitCommandFailed)?;
        let stderr = child
            .stderr
            .take()
            .ok_or(RepositoryError::GitCommandFailed)?;
        let stdout_reader = thread::Builder::new()
            .name("kiln-git-stdout".to_owned())
            .spawn(move || read_limited(stdout, 8 * 1024 * 1024))
            .map_err(|_| RepositoryError::GitCommandFailed)?;
        let stderr_reader = thread::Builder::new()
            .name("kiln-git-stderr".to_owned())
            .spawn(move || read_limited(stderr, 64 * 1024))
            .map_err(|_| RepositoryError::GitCommandFailed)?;

        let deadline = Instant::now() + self.timeout;
        let status = loop {
            if cancellation.is_some_and(CancellationToken::is_cancelled) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RepositoryError::InspectionCancelled);
            }
            if let Some(status) = child
                .try_wait()
                .map_err(|_| RepositoryError::GitCommandFailed)?
            {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RepositoryError::InspectionTimedOut);
            }
            thread::sleep(Duration::from_millis(10));
        };
        assemble_output(status, stdout_reader, stderr_reader)
    }
}

fn read_limited(mut reader: impl Read, limit: usize) -> IoResult<(Vec<u8>, bool)> {
    let mut captured = Vec::with_capacity(limit.min(16 * 1024));
    let mut truncated = false;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let available = limit.saturating_sub(captured.len());
        let kept = available.min(read);
        captured.extend_from_slice(&buffer[..kept]);
        truncated |= kept < read;
    }
    Ok((captured, truncated))
}

fn assemble_output(
    status: ExitStatus,
    stdout_reader: thread::JoinHandle<IoResult<(Vec<u8>, bool)>>,
    stderr_reader: thread::JoinHandle<IoResult<(Vec<u8>, bool)>>,
) -> Result<Output, RepositoryError> {
    let (stdout, stdout_truncated) = stdout_reader
        .join()
        .map_err(|_| RepositoryError::GitCommandFailed)?
        .map_err(|_| RepositoryError::GitCommandFailed)?;
    let (stderr, stderr_truncated) = stderr_reader
        .join()
        .map_err(|_| RepositoryError::GitCommandFailed)?
        .map_err(|_| RepositoryError::GitCommandFailed)?;
    if stdout_truncated || stderr_truncated {
        return Err(RepositoryError::GitOutputTooLarge);
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn strip_one_line_ending(value: &str) -> &str {
    value
        .strip_suffix("\r\n")
        .or_else(|| value.strip_suffix('\n'))
        .unwrap_or(value)
}

fn project_id(root: &str) -> String {
    let normalized = if cfg!(windows) {
        root.replace('\\', "/").to_lowercase()
    } else {
        root.to_owned()
    };
    let digest = Sha256::digest(normalized.as_bytes());
    format!("project-{}", hex::encode(&digest[..16]))
}

fn classify_git_failure(output: &Output) -> RepositoryError {
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    if stderr.contains("dubious ownership") || stderr.contains("safe.directory") {
        RepositoryError::UnsafeOwnership
    } else if stderr.contains("not a git repository") {
        RepositoryError::NotRepository
    } else {
        RepositoryError::GitCommandFailed
    }
}

fn parse_porcelain_v2(output: &[u8]) -> Result<RepositoryStatus, RepositoryError> {
    let mut status = RepositoryStatus::default();
    let mut skip_rename_source = false;

    for record in output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        if skip_rename_source {
            skip_rename_source = false;
            continue;
        }
        if record.starts_with(b"# branch.ab ") {
            parse_ahead_behind(record, &mut status)?;
            continue;
        }
        match record.first().copied() {
            Some(b'1' | b'2') => {
                if record.len() < 4 || record[1] != b' ' {
                    return Err(RepositoryError::MalformedGitOutput("working-tree status"));
                }
                let index = record[2];
                let worktree = record[3];
                if index != b'.' {
                    status.staged = status.staged.saturating_add(1);
                }
                if worktree != b'.' {
                    status.modified = status.modified.saturating_add(1);
                }
                if record[0] == b'2' {
                    skip_rename_source = true;
                }
            }
            Some(b'u') => {
                status.conflicts = status.conflicts.saturating_add(1);
            }
            Some(b'?') => {
                status.untracked = status.untracked.saturating_add(1);
            }
            Some(b'#') | Some(b'!') => {}
            _ => return Err(RepositoryError::MalformedGitOutput("working-tree status")),
        }
    }
    Ok(status)
}

fn parse_ahead_behind(record: &[u8], status: &mut RepositoryStatus) -> Result<(), RepositoryError> {
    let text = std::str::from_utf8(record)
        .map_err(|_| RepositoryError::MalformedGitOutput("branch status"))?;
    let mut parts = text
        .strip_prefix("# branch.ab ")
        .ok_or(RepositoryError::MalformedGitOutput("branch status"))?
        .split_whitespace();
    status.ahead = parse_distance(parts.next(), '+')?;
    status.behind = parse_distance(parts.next(), '-')?;
    Ok(())
}

fn parse_distance(value: Option<&str>, prefix: char) -> Result<u32, RepositoryError> {
    value
        .and_then(|value| value.strip_prefix(prefix))
        .and_then(|value| value.parse().ok())
        .ok_or(RepositoryError::MalformedGitOutput("branch status"))
}

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("the repository path must be absolute")]
    SelectionNotAbsolute,
    #[error("the selected path does not exist")]
    SelectionMissing,
    #[error("the selected path is not a folder")]
    SelectionNotDirectory,
    #[error("the selected folder could not be read")]
    SelectionInaccessible(#[source] std::io::Error),
    #[error("opening an entire filesystem root is not allowed")]
    FilesystemRoot,
    #[error("Git is not installed or could not be started")]
    GitUnavailable,
    #[error("the selected folder is not inside a Git working tree")]
    NotRepository,
    #[error("bare Git repositories cannot be opened as workspaces")]
    BareRepository,
    #[error("Git rejected the repository because its ownership is unsafe")]
    UnsafeOwnership,
    #[error("the repository path cannot be represented safely")]
    NonUnicodePath,
    #[error("Git returned malformed {0}")]
    MalformedGitOutput(&'static str),
    #[error("Git could not inspect the selected repository")]
    GitCommandFailed,
    #[error("Git repository inspection exceeded its time limit")]
    InspectionTimedOut,
    #[error("Git repository inspection was cancelled")]
    InspectionCancelled,
    #[error("Git repository inspection produced too much output")]
    GitOutputTooLarge,
    #[error("the supplied project defaults are invalid")]
    InvalidDefaults,
}

impl RepositoryError {
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::SelectionNotAbsolute => {
                "Enter an absolute repository path, including the drive or filesystem root."
            }
            Self::SelectionMissing => {
                "That folder no longer exists. Choose its new location or enter another path."
            }
            Self::SelectionNotDirectory => "Choose a folder, not an individual file.",
            Self::SelectionInaccessible(_) => {
                "Kiln cannot read that folder. Check its permissions and try again."
            }
            Self::FilesystemRoot => {
                "Choose a repository folder instead of an entire drive or filesystem root."
            }
            Self::GitUnavailable => {
                "Kiln could not start Git. Install Git or make it available on PATH, then try again."
            }
            Self::NotRepository => {
                "That folder is not inside a Git working tree. Choose a cloned or initialized repository."
            }
            Self::BareRepository => {
                "Bare repositories have no working tree. Choose a checked-out Git repository."
            }
            Self::UnsafeOwnership => {
                "Git does not trust this folder’s ownership. Review Git’s safe.directory guidance before opening it."
            }
            Self::NonUnicodePath => {
                "This repository path cannot be represented safely by the desktop interface."
            }
            Self::InspectionTimedOut => {
                "Git did not finish inspecting this repository within 15 seconds. Check the repository or storage and try again."
            }
            Self::InspectionCancelled => "Repository inspection was cancelled.",
            Self::GitOutputTooLarge => {
                "This repository produced too much status output to open safely."
            }
            Self::MalformedGitOutput(_) | Self::GitCommandFailed => {
                "Git could not describe this repository. Verify it with “git status” and try again."
            }
            Self::InvalidDefaults => {
                "The selected provider, model, or verification default is invalid."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::*;

    fn temporary_repository(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiln-workspace-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        run(&root, ["init", "--quiet"]);
        fs::write(root.join("tracked.txt"), "first\n").unwrap();
        run(&root, ["add", "tracked.txt"]);
        run(
            &root,
            [
                "-c",
                "user.name=Kiln Tests",
                "-c",
                "user.email=kiln@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "initial",
            ],
        );
        root
    }

    fn run<const N: usize>(root: &Path, args: [&str; N]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git command failed: {args:?}");
    }

    fn remove_repository(root: &Path) {
        if root.starts_with(std::env::temp_dir()) {
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn projects_identity_branch_and_truthful_status() {
        let root = temporary_repository("status");
        fs::write(root.join("tracked.txt"), "changed\n").unwrap();
        fs::write(root.join("staged.txt"), "staged\n").unwrap();
        fs::write(root.join("untracked.txt"), "untracked\n").unwrap();
        run(&root, ["add", "staged.txt"]);
        let nested = root.join("nested");
        fs::create_dir(&nested).unwrap();

        let inspector = GitRepositoryInspector::default();
        let project = inspector
            .inspect(
                &nested,
                ProjectDefaults {
                    model: Some("test-model".to_owned()),
                    ..ProjectDefaults::default()
                },
            )
            .unwrap();

        assert_eq!(
            Path::new(&project.root),
            dunce::canonicalize(&root).unwrap()
        );
        assert!(project.project_id.starts_with("project-"));
        assert!(project.branch.is_some());
        assert_eq!(project.status.staged, 1);
        assert_eq!(project.status.modified, 1);
        assert_eq!(project.status.untracked, 1);
        assert!(!project.status.is_clean());
        assert_eq!(project.defaults.model.as_deref(), Some("test-model"));

        let same = inspector
            .inspect(&root, ProjectDefaults::default())
            .unwrap();
        assert_eq!(same.project_id, project.project_id);
        remove_repository(&root);
    }

    #[test]
    fn rejects_missing_relative_and_non_repository_paths() {
        let inspector = GitRepositoryInspector::default();
        assert!(matches!(
            inspector.inspect("relative", ProjectDefaults::default()),
            Err(RepositoryError::SelectionNotAbsolute)
        ));

        let missing =
            std::env::temp_dir().join(format!("kiln-workspace-missing-{}", std::process::id()));
        assert!(matches!(
            inspector.inspect(&missing, ProjectDefaults::default()),
            Err(RepositoryError::SelectionMissing)
        ));

        let plain =
            std::env::temp_dir().join(format!("kiln-workspace-plain-{}", std::process::id()));
        fs::create_dir_all(&plain).unwrap();
        assert!(matches!(
            inspector.inspect(&plain, ProjectDefaults::default()),
            Err(RepositoryError::NotRepository)
        ));
        remove_repository(&plain);
    }

    #[test]
    fn missing_git_has_an_actionable_error() {
        let root = temporary_repository("no-git");
        let inspector = GitRepositoryInspector::with_executable(
            root.join("definitely-not-git").into_os_string(),
        );
        let error = inspector
            .inspect(&root, ProjectDefaults::default())
            .unwrap_err();

        assert!(matches!(error, RepositoryError::GitUnavailable));
        assert!(error.user_message().contains("PATH"));
        remove_repository(&root);
    }

    #[test]
    fn output_capture_is_bounded_while_the_reader_is_drained() {
        let input = vec![b'x'; 32];
        let (captured, truncated) = read_limited(input.as_slice(), 8).unwrap();

        assert_eq!(captured, vec![b'x'; 8]);
        assert!(truncated);
    }
}
