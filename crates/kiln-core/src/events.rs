use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ChatRole, ProjectDefaults, ProviderKind, RepositoryStatus, TokenUsage};

/// Current major version of Kiln's transport-independent application contract.
pub const APPLICATION_CONTRACT_VERSION: u16 = 1;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(ContractError::InvalidField {
                        field: stringify!($name),
                        message: "identifier cannot be blank".to_owned(),
                    });
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            fn is_blank(&self) -> bool {
                self.0.trim().is_empty()
            }
        }
    };
}

string_id!(EventId);
string_id!(StreamId);
string_id!(TaskId);
string_id!(SessionId);
string_id!(TurnId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandEnvelope<T> {
    pub schema_version: u16,
    pub command_id: String,
    pub issued_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    pub payload: T,
}

impl<T> CommandEnvelope<T> {
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_version(self.schema_version)?;
        validate_text("commandId", &self.command_id)
    }
}

/// Immutable application event. `sequence` is strictly monotonic within
/// `stream_id`; task events also carry `task_id` for efficient projection.
///
/// Serde intentionally ignores unknown object fields, allowing additive
/// contract changes within a major schema version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEnvelope {
    pub schema_version: u16,
    pub event_id: EventId,
    pub stream_id: StreamId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    pub sequence: u64,
    pub occurred_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    pub payload: ApplicationEvent,
}

impl EventEnvelope {
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_version(self.schema_version)?;
        validate_id("eventId", self.event_id.is_blank())?;
        validate_id("streamId", self.stream_id.is_blank())?;
        if self.task_id.as_ref().is_some_and(TaskId::is_blank) {
            return Err(ContractError::InvalidField {
                field: "taskId",
                message: "identifier cannot be blank".to_owned(),
            });
        }
        if self.sequence == 0 {
            return Err(ContractError::InvalidField {
                field: "sequence",
                message: "sequence starts at 1".to_owned(),
            });
        }
        if let Some(causation_id) = &self.causation_id {
            validate_text("causationId", causation_id)?;
        }
        if let Some(correlation_id) = &self.correlation_id {
            validate_text("correlationId", correlation_id)?;
        }
        self.payload.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ApplicationEvent {
    ProjectOpened {
        project_id: String,
        root: String,
        display_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        head: Option<String>,
        #[serde(default)]
        status: RepositoryStatus,
        #[serde(default)]
        defaults: ProjectDefaults,
    },
    WorkspaceReady {
        workspace_id: String,
        project_id: String,
        path: String,
        isolated: bool,
    },
    TaskCreated {
        title: String,
    },
    TaskStatusChanged {
        status: TaskStatus,
    },
    SessionStarted {
        session_id: SessionId,
        provider: ProviderKind,
        model: String,
    },
    TurnStarted {
        turn_id: TurnId,
    },
    MessageAdded {
        message_id: String,
        role: ChatRole,
        content: String,
    },
    MessageDelta {
        message_id: String,
        delta: String,
    },
    MessageCompleted {
        message_id: String,
        model: String,
        content: String,
        finish_reason: Option<String>,
        usage: TokenUsage,
    },
    ApprovalRequested {
        approval_id: String,
        action: String,
        resource: String,
        reason: String,
    },
    ApprovalDecided {
        approval_id: String,
        decision: ApprovalDecision,
        scope: ApprovalScope,
    },
    ToolProposed {
        tool_call_id: String,
        name: String,
        summary: String,
    },
    ToolStarted {
        tool_call_id: String,
    },
    ToolOutput {
        tool_call_id: String,
        stream: ToolOutputStream,
        chunk: String,
    },
    ToolCompleted {
        tool_call_id: String,
        success: bool,
        exit_code: Option<i32>,
    },
    ArtifactPublished {
        artifact_id: String,
        kind: ArtifactKind,
        label: String,
    },
    TurnReceipt {
        turn_id: TurnId,
        outcome: ReceiptOutcome,
        summary: String,
    },
}

impl ApplicationEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ProjectOpened { .. } => "project_opened",
            Self::WorkspaceReady { .. } => "workspace_ready",
            Self::TaskCreated { .. } => "task_created",
            Self::TaskStatusChanged { .. } => "task_status_changed",
            Self::SessionStarted { .. } => "session_started",
            Self::TurnStarted { .. } => "turn_started",
            Self::MessageAdded { .. } => "message_added",
            Self::MessageDelta { .. } => "message_delta",
            Self::MessageCompleted { .. } => "message_completed",
            Self::ApprovalRequested { .. } => "approval_requested",
            Self::ApprovalDecided { .. } => "approval_decided",
            Self::ToolProposed { .. } => "tool_proposed",
            Self::ToolStarted { .. } => "tool_started",
            Self::ToolOutput { .. } => "tool_output",
            Self::ToolCompleted { .. } => "tool_completed",
            Self::ArtifactPublished { .. } => "artifact_published",
            Self::TurnReceipt { .. } => "turn_receipt",
        }
    }

    fn validate(&self) -> Result<(), ContractError> {
        match self {
            Self::ProjectOpened {
                project_id,
                root,
                display_name,
                branch,
                head,
                defaults,
                ..
            } => {
                validate_text("projectId", project_id)?;
                validate_text("root", root)?;
                validate_text("displayName", display_name)?;
                if let Some(branch) = branch {
                    validate_text("branch", branch)?;
                }
                if let Some(head) = head {
                    validate_text("head", head)?;
                }
                defaults.validate()
            }
            Self::WorkspaceReady {
                workspace_id,
                project_id,
                path,
                ..
            } => {
                validate_text("workspaceId", workspace_id)?;
                validate_text("projectId", project_id)?;
                validate_text("path", path)
            }
            Self::TaskCreated { title } => validate_text("title", title),
            Self::SessionStarted {
                session_id, model, ..
            } => {
                validate_id("sessionId", session_id.is_blank())?;
                validate_text("model", model)
            }
            Self::TurnStarted { turn_id } => validate_id("turnId", turn_id.is_blank()),
            Self::MessageAdded {
                message_id,
                content,
                ..
            } => {
                validate_text("messageId", message_id)?;
                validate_text("content", content)
            }
            Self::MessageDelta { message_id, delta } => {
                validate_text("messageId", message_id)?;
                validate_text("delta", delta)
            }
            Self::MessageCompleted {
                message_id,
                model,
                content,
                ..
            } => {
                validate_text("messageId", message_id)?;
                validate_text("model", model)?;
                validate_text("content", content)
            }
            Self::ApprovalRequested {
                approval_id,
                action,
                resource,
                reason,
            } => {
                validate_text("approvalId", approval_id)?;
                validate_text("action", action)?;
                validate_text("resource", resource)?;
                validate_text("reason", reason)
            }
            Self::ApprovalDecided { approval_id, .. } => validate_text("approvalId", approval_id),
            Self::ToolProposed {
                tool_call_id,
                name,
                summary,
            } => {
                validate_text("toolCallId", tool_call_id)?;
                validate_text("name", name)?;
                validate_text("summary", summary)
            }
            Self::ToolStarted { tool_call_id }
            | Self::ToolOutput { tool_call_id, .. }
            | Self::ToolCompleted { tool_call_id, .. } => validate_text("toolCallId", tool_call_id),
            Self::ArtifactPublished {
                artifact_id, label, ..
            } => {
                validate_text("artifactId", artifact_id)?;
                validate_text("label", label)
            }
            Self::TurnReceipt {
                turn_id, summary, ..
            } => {
                validate_id("turnId", turn_id.is_blank())?;
                validate_text("summary", summary)
            }
            Self::TaskStatusChanged { .. } => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Running,
    WaitingForApproval,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approved,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalScope {
    Once,
    Session,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOutputStream {
    Stdout,
    Stderr,
    Structured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Diff,
    File,
    Plan,
    CommandOutput,
    Diagnostic,
    TestResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptOutcome {
    Completed,
    Cancelled,
    Failed,
}

/// Incremental validator for one causally ordered application stream.
#[derive(Debug, Clone)]
pub struct EventSequence {
    stream_id: StreamId,
    next_sequence: u64,
}

impl EventSequence {
    pub fn new(stream_id: StreamId) -> Self {
        Self {
            stream_id,
            next_sequence: 1,
        }
    }

    pub fn observe(&mut self, event: &EventEnvelope) -> Result<(), ContractError> {
        event.validate()?;
        if event.stream_id != self.stream_id {
            return Err(ContractError::StreamMismatch {
                expected: self.stream_id.as_str().to_owned(),
                found: event.stream_id.as_str().to_owned(),
            });
        }
        if event.sequence != self.next_sequence {
            return Err(ContractError::UnexpectedSequence {
                expected: self.next_sequence,
                found: event.sequence,
            });
        }
        self.next_sequence += 1;
        Ok(())
    }

    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContractError {
    #[error("unsupported application contract version {found}; this build supports {supported}")]
    UnsupportedVersion { supported: u16, found: u16 },
    #[error("{field}: {message}")]
    InvalidField {
        field: &'static str,
        message: String,
    },
    #[error("event belongs to stream {found}, expected {expected}")]
    StreamMismatch { expected: String, found: String },
    #[error("event sequence is {found}, expected {expected}")]
    UnexpectedSequence { expected: u64, found: u64 },
}

fn validate_version(found: u16) -> Result<(), ContractError> {
    if found == APPLICATION_CONTRACT_VERSION {
        Ok(())
    } else {
        Err(ContractError::UnsupportedVersion {
            supported: APPLICATION_CONTRACT_VERSION,
            found,
        })
    }
}

fn validate_id(field: &'static str, blank: bool) -> Result<(), ContractError> {
    if blank {
        Err(ContractError::InvalidField {
            field,
            message: "identifier cannot be blank".to_owned(),
        })
    } else {
        Ok(())
    }
}

fn validate_text(field: &'static str, value: &str) -> Result<(), ContractError> {
    if value.trim().is_empty() {
        Err(ContractError::InvalidField {
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

    fn event(sequence: u64) -> EventEnvelope {
        EventEnvelope {
            schema_version: APPLICATION_CONTRACT_VERSION,
            event_id: EventId::new(format!("event-{sequence}")).unwrap(),
            stream_id: StreamId::new("task-stream-1").unwrap(),
            task_id: Some(TaskId::new("task-1").unwrap()),
            sequence,
            occurred_at_ms: 1_753_731_600_000 + sequence,
            causation_id: Some("command-1".to_owned()),
            correlation_id: Some("turn-1".to_owned()),
            payload: ApplicationEvent::TaskStatusChanged {
                status: TaskStatus::Running,
            },
        }
    }

    #[test]
    fn additive_fields_are_tolerated() {
        let source = serde_json::json!({
            "schemaVersion": 1,
            "eventId": "event-1",
            "streamId": "task-stream-1",
            "taskId": "task-1",
            "sequence": 1,
            "occurredAtMs": 1753731600001_u64,
            "futureEnvelopeField": {"safe": true},
            "payload": {
                "type": "task_created",
                "data": {
                    "title": "Extract the core",
                    "futurePayloadField": ["ignored"]
                }
            }
        });

        let event: EventEnvelope = serde_json::from_value(source).unwrap();
        assert_eq!(event.sequence, 1);
        assert!(matches!(
            event.payload,
            ApplicationEvent::TaskCreated { .. }
        ));
        event.validate().unwrap();
    }

    #[test]
    fn breaking_versions_fail_explicitly() {
        let mut event = event(1);
        event.schema_version = APPLICATION_CONTRACT_VERSION + 1;

        assert_eq!(
            event.validate(),
            Err(ContractError::UnsupportedVersion {
                supported: APPLICATION_CONTRACT_VERSION,
                found: APPLICATION_CONTRACT_VERSION + 1,
            })
        );
    }

    #[test]
    fn message_events_use_stable_transport_names() {
        let mut event = event(1);
        event.payload = ApplicationEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            model: "local-model".to_owned(),
            content: "Ready for review.".to_owned(),
            finish_reason: Some("stop".to_owned()),
            usage: crate::TokenUsage {
                input_tokens: Some(8),
                output_tokens: Some(4),
                total_tokens: Some(12),
            },
        };

        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["payload"]["type"], "message_completed");
        assert_eq!(value["payload"]["data"]["messageId"], "message-1");
        assert_eq!(value["payload"]["data"]["finishReason"], "stop");
        assert_eq!(value["payload"]["data"]["usage"]["totalTokens"], 12);
    }

    #[test]
    fn stream_sequence_rejects_gaps_and_cross_task_reordering() {
        let mut sequence = EventSequence::new(StreamId::new("task-stream-1").unwrap());
        sequence.observe(&event(1)).unwrap();
        assert_eq!(sequence.next_sequence(), 2);

        assert_eq!(
            sequence.observe(&event(3)),
            Err(ContractError::UnexpectedSequence {
                expected: 2,
                found: 3,
            })
        );

        let mut other = event(2);
        other.stream_id = StreamId::new("task-stream-2").unwrap();
        assert!(matches!(
            sequence.observe(&other),
            Err(ContractError::StreamMismatch { .. })
        ));
    }
}
