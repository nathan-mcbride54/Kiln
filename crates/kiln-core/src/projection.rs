use serde::{Deserialize, Serialize};

use crate::{
    ApplicationEvent, ApprovalDecision, ApprovalScope, ArtifactKind, ChatRole, ContractError,
    EventEnvelope, ProjectSnapshot, ReceiptOutcome, SessionId, StreamId, TaskId, TaskStatus,
    TokenUsage, TurnId,
};

pub const TASK_PROJECTION_VERSION: u16 = 1;
pub const PROJECT_PROJECTION_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProjection {
    pub schema_version: u16,
    pub stream_id: Option<StreamId>,
    pub last_sequence: u64,
    pub project: Option<ProjectSnapshot>,
    pub workspace: Option<WorkspaceProjection>,
}

impl Default for ProjectProjection {
    fn default() -> Self {
        Self {
            schema_version: PROJECT_PROJECTION_VERSION,
            stream_id: None,
            last_sequence: 0,
            project: None,
            workspace: None,
        }
    }
}

impl ProjectProjection {
    pub fn rebuild(events: &[EventEnvelope]) -> Result<Self, ContractError> {
        let mut projection = Self::default();
        for event in events {
            projection.apply(event)?;
        }
        Ok(projection)
    }

    pub fn apply(&mut self, event: &EventEnvelope) -> Result<(), ContractError> {
        event.validate()?;
        let expected = self.last_sequence + 1;
        if event.sequence != expected {
            return Err(ContractError::UnexpectedSequence {
                expected,
                found: event.sequence,
            });
        }
        if let Some(stream_id) = &self.stream_id {
            if stream_id != &event.stream_id {
                return Err(ContractError::StreamMismatch {
                    expected: stream_id.as_str().to_owned(),
                    found: event.stream_id.as_str().to_owned(),
                });
            }
        } else {
            self.stream_id = Some(event.stream_id.clone());
        }

        match &event.payload {
            ApplicationEvent::ProjectOpened {
                project_id,
                root,
                display_name,
                branch,
                head,
                status,
                defaults,
            } => {
                self.project = Some(ProjectSnapshot {
                    project_id: project_id.clone(),
                    display_name: display_name.clone(),
                    root: root.clone(),
                    branch: branch.clone(),
                    head: head.clone(),
                    status: status.clone(),
                    defaults: defaults.clone(),
                });
            }
            ApplicationEvent::WorkspaceReady {
                workspace_id,
                project_id,
                path,
                isolated,
            } => {
                self.workspace = Some(WorkspaceProjection {
                    workspace_id: workspace_id.clone(),
                    project_id: project_id.clone(),
                    path: path.clone(),
                    isolated: *isolated,
                });
            }
            _ => {
                return Err(ContractError::InvalidField {
                    field: "payload",
                    message: "project streams accept only project and workspace events".to_owned(),
                });
            }
        }

        self.last_sequence = event.sequence;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceProjection {
    pub workspace_id: String,
    pub project_id: String,
    pub path: String,
    pub isolated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProjection {
    pub schema_version: u16,
    pub stream_id: Option<StreamId>,
    pub task_id: Option<TaskId>,
    pub last_sequence: u64,
    pub title: Option<String>,
    pub status: TaskStatus,
    pub session: Option<SessionProjection>,
    pub active_turn_id: Option<TurnId>,
    pub messages: Vec<MessageProjection>,
    pub pending_approval: Option<ApprovalProjection>,
    pub tools: Vec<ToolProjection>,
    pub artifacts: Vec<ArtifactProjection>,
    pub last_receipt: Option<ReceiptProjection>,
}

impl Default for TaskProjection {
    fn default() -> Self {
        Self {
            schema_version: TASK_PROJECTION_VERSION,
            stream_id: None,
            task_id: None,
            last_sequence: 0,
            title: None,
            status: TaskStatus::Queued,
            session: None,
            active_turn_id: None,
            messages: Vec::new(),
            pending_approval: None,
            tools: Vec::new(),
            artifacts: Vec::new(),
            last_receipt: None,
        }
    }
}

impl TaskProjection {
    pub fn rebuild(events: &[EventEnvelope]) -> Result<Self, ContractError> {
        let mut projection = Self::default();
        for event in events {
            projection.apply(event)?;
        }
        Ok(projection)
    }

    pub fn apply(&mut self, event: &EventEnvelope) -> Result<(), ContractError> {
        event.validate()?;
        let expected = self.last_sequence + 1;
        if event.sequence != expected {
            return Err(ContractError::UnexpectedSequence {
                expected,
                found: event.sequence,
            });
        }
        if let Some(stream_id) = &self.stream_id {
            if stream_id != &event.stream_id {
                return Err(ContractError::StreamMismatch {
                    expected: stream_id.as_str().to_owned(),
                    found: event.stream_id.as_str().to_owned(),
                });
            }
        } else {
            self.stream_id = Some(event.stream_id.clone());
        }
        if self.task_id.is_none() {
            self.task_id.clone_from(&event.task_id);
        }

        self.apply_payload(&event.payload);
        self.last_sequence = event.sequence;
        Ok(())
    }

    fn apply_payload(&mut self, payload: &ApplicationEvent) {
        if self.status == TaskStatus::Cancelled && is_late_mutation(payload) {
            return;
        }

        match payload {
            ApplicationEvent::TaskCreated { title } => {
                self.title = Some(title.clone());
                self.status = TaskStatus::Queued;
            }
            ApplicationEvent::TaskStatusChanged { status } => self.status = *status,
            ApplicationEvent::SessionStarted {
                session_id,
                provider,
                model,
            } => {
                self.session = Some(SessionProjection {
                    session_id: session_id.clone(),
                    provider: *provider,
                    model: model.clone(),
                });
            }
            ApplicationEvent::TurnStarted { turn_id } => {
                self.active_turn_id = Some(turn_id.clone());
                self.status = TaskStatus::Running;
            }
            ApplicationEvent::MessageAdded {
                message_id,
                role,
                content,
            } => {
                self.messages.push(MessageProjection {
                    message_id: message_id.clone(),
                    role: *role,
                    model: None,
                    content: content.clone(),
                    completed: true,
                    usage: TokenUsage::default(),
                });
            }
            ApplicationEvent::MessageDelta { message_id, delta } => {
                if let Some(message) = self
                    .messages
                    .iter_mut()
                    .find(|message| message.message_id == *message_id)
                {
                    message.content.push_str(delta);
                } else {
                    self.messages.push(MessageProjection {
                        message_id: message_id.clone(),
                        role: ChatRole::Assistant,
                        model: None,
                        content: delta.clone(),
                        completed: false,
                        usage: TokenUsage::default(),
                    });
                }
            }
            ApplicationEvent::MessageCompleted {
                message_id,
                model,
                content,
                usage,
                ..
            } => {
                if let Some(message) = self
                    .messages
                    .iter_mut()
                    .find(|message| message.message_id == *message_id)
                {
                    message.role = ChatRole::Assistant;
                    message.model = Some(model.clone());
                    message.content = content.clone();
                    message.completed = true;
                    message.usage = usage.clone();
                } else {
                    self.messages.push(MessageProjection {
                        message_id: message_id.clone(),
                        role: ChatRole::Assistant,
                        model: Some(model.clone()),
                        content: content.clone(),
                        completed: true,
                        usage: usage.clone(),
                    });
                }
            }
            ApplicationEvent::ApprovalRequested {
                approval_id,
                action,
                resource,
                reason,
            } => {
                self.pending_approval = Some(ApprovalProjection {
                    approval_id: approval_id.clone(),
                    action: action.clone(),
                    resource: resource.clone(),
                    reason: reason.clone(),
                    decision: None,
                    scope: None,
                });
                self.status = TaskStatus::WaitingForApproval;
            }
            ApplicationEvent::ApprovalDecided {
                approval_id,
                decision,
                scope,
            } => {
                if let Some(approval) = self
                    .pending_approval
                    .as_mut()
                    .filter(|approval| approval.approval_id == *approval_id)
                {
                    approval.decision = Some(*decision);
                    approval.scope = Some(*scope);
                }
                self.pending_approval = None;
                self.status = TaskStatus::Running;
            }
            ApplicationEvent::ToolProposed {
                tool_call_id,
                name,
                summary,
            } => self.tools.push(ToolProjection {
                tool_call_id: tool_call_id.clone(),
                name: name.clone(),
                summary: summary.clone(),
                status: ToolProjectionStatus::Proposed,
                output_chunks: 0,
                exit_code: None,
            }),
            ApplicationEvent::ToolStarted { tool_call_id } => {
                if let Some(tool) = self.tool_mut(tool_call_id) {
                    tool.status = ToolProjectionStatus::Running;
                }
            }
            ApplicationEvent::ToolOutput { tool_call_id, .. } => {
                if let Some(tool) = self.tool_mut(tool_call_id) {
                    tool.output_chunks += 1;
                }
            }
            ApplicationEvent::ToolCompleted {
                tool_call_id,
                success,
                exit_code,
            } => {
                if let Some(tool) = self.tool_mut(tool_call_id) {
                    tool.status = if *success {
                        ToolProjectionStatus::Completed
                    } else {
                        ToolProjectionStatus::Failed
                    };
                    tool.exit_code = *exit_code;
                }
            }
            ApplicationEvent::ArtifactPublished {
                artifact_id,
                kind,
                label,
            } => self.artifacts.push(ArtifactProjection {
                artifact_id: artifact_id.clone(),
                kind: *kind,
                label: label.clone(),
            }),
            ApplicationEvent::TurnReceipt {
                turn_id,
                outcome,
                summary,
            } => {
                self.active_turn_id = None;
                self.status = match outcome {
                    ReceiptOutcome::Completed => TaskStatus::Completed,
                    ReceiptOutcome::Cancelled => TaskStatus::Cancelled,
                    ReceiptOutcome::Failed => TaskStatus::Failed,
                };
                self.last_receipt = Some(ReceiptProjection {
                    turn_id: turn_id.clone(),
                    outcome: *outcome,
                    summary: summary.clone(),
                });
            }
            ApplicationEvent::ProjectOpened { .. } | ApplicationEvent::WorkspaceReady { .. } => {}
        }
    }

    fn tool_mut(&mut self, tool_call_id: &str) -> Option<&mut ToolProjection> {
        self.tools
            .iter_mut()
            .find(|tool| tool.tool_call_id == tool_call_id)
    }
}

fn is_late_mutation(payload: &ApplicationEvent) -> bool {
    matches!(
        payload,
        ApplicationEvent::MessageDelta { .. }
            | ApplicationEvent::MessageCompleted { .. }
            | ApplicationEvent::ToolStarted { .. }
            | ApplicationEvent::ToolOutput { .. }
            | ApplicationEvent::ToolCompleted { .. }
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionProjection {
    pub session_id: SessionId,
    pub provider: crate::ProviderKind,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageProjection {
    pub message_id: String,
    pub role: ChatRole,
    pub model: Option<String>,
    pub content: String,
    pub completed: bool,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalProjection {
    pub approval_id: String,
    pub action: String,
    pub resource: String,
    pub reason: String,
    pub decision: Option<ApprovalDecision>,
    pub scope: Option<ApprovalScope>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolProjectionStatus {
    Proposed,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolProjection {
    pub tool_call_id: String,
    pub name: String,
    pub summary: String,
    pub status: ToolProjectionStatus,
    pub output_chunks: u64,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactProjection {
    pub artifact_id: String,
    pub kind: ArtifactKind,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptProjection {
    pub turn_id: TurnId,
    pub outcome: ReceiptOutcome,
    pub summary: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EventId, APPLICATION_CONTRACT_VERSION};

    fn event(sequence: u64, payload: ApplicationEvent) -> EventEnvelope {
        EventEnvelope {
            schema_version: APPLICATION_CONTRACT_VERSION,
            event_id: EventId::new(format!("event-{sequence}")).unwrap(),
            stream_id: StreamId::new("task:test").unwrap(),
            task_id: Some(TaskId::new("test").unwrap()),
            sequence,
            occurred_at_ms: sequence,
            causation_id: None,
            correlation_id: None,
            payload,
        }
    }

    #[test]
    fn message_deltas_collapse_into_the_completed_message() {
        let events = [
            event(
                1,
                ApplicationEvent::MessageDelta {
                    message_id: "message-1".to_owned(),
                    delta: "Hello ".to_owned(),
                },
            ),
            event(
                2,
                ApplicationEvent::MessageDelta {
                    message_id: "message-1".to_owned(),
                    delta: "world".to_owned(),
                },
            ),
            event(
                3,
                ApplicationEvent::MessageCompleted {
                    message_id: "message-1".to_owned(),
                    model: "local".to_owned(),
                    content: "Hello world".to_owned(),
                    finish_reason: Some("stop".to_owned()),
                    usage: TokenUsage::default(),
                },
            ),
        ];

        let projection = TaskProjection::rebuild(&events).unwrap();
        assert_eq!(projection.messages.len(), 1);
        assert_eq!(projection.messages[0].content, "Hello world");
        assert!(projection.messages[0].completed);
    }

    #[test]
    fn late_provider_events_cannot_mutate_a_cancelled_turn() {
        let events = [
            event(
                1,
                ApplicationEvent::TurnStarted {
                    turn_id: TurnId::new("turn-cancelled").unwrap(),
                },
            ),
            event(
                2,
                ApplicationEvent::MessageDelta {
                    message_id: "message-1".to_owned(),
                    delta: "Visible".to_owned(),
                },
            ),
            event(
                3,
                ApplicationEvent::TurnReceipt {
                    turn_id: TurnId::new("turn-cancelled").unwrap(),
                    outcome: ReceiptOutcome::Cancelled,
                    summary: "Stopped by the user.".to_owned(),
                },
            ),
            event(
                4,
                ApplicationEvent::MessageDelta {
                    message_id: "message-1".to_owned(),
                    delta: " late".to_owned(),
                },
            ),
            event(
                5,
                ApplicationEvent::MessageCompleted {
                    message_id: "message-1".to_owned(),
                    model: "ignored".to_owned(),
                    content: "Visible late".to_owned(),
                    finish_reason: Some("stop".to_owned()),
                    usage: TokenUsage::default(),
                },
            ),
        ];

        let projection = TaskProjection::rebuild(&events).unwrap();
        assert_eq!(projection.status, TaskStatus::Cancelled);
        assert_eq!(projection.messages[0].content, "Visible");
        assert!(!projection.messages[0].completed);
        assert_eq!(projection.last_sequence, 5);
    }

    #[test]
    fn repository_identity_status_defaults_and_workspace_are_projected() {
        let stream_id = StreamId::new("project:kiln").unwrap();
        let events = [
            EventEnvelope {
                schema_version: APPLICATION_CONTRACT_VERSION,
                event_id: EventId::new("project-event-1").unwrap(),
                stream_id: stream_id.clone(),
                task_id: None,
                sequence: 1,
                occurred_at_ms: 1,
                causation_id: Some("command:open-project".to_owned()),
                correlation_id: None,
                payload: ApplicationEvent::ProjectOpened {
                    project_id: "project-kiln".to_owned(),
                    root: "/work/kiln".to_owned(),
                    display_name: "kiln".to_owned(),
                    branch: Some("main".to_owned()),
                    head: Some("0123456789abcdef".to_owned()),
                    status: crate::RepositoryStatus {
                        modified: 2,
                        untracked: 1,
                        ..crate::RepositoryStatus::default()
                    },
                    defaults: crate::ProjectDefaults {
                        provider: Some(crate::ProviderKind::OpenAi),
                        model: Some("gpt-5".to_owned()),
                        verification_profile: Some("quick".to_owned()),
                    },
                },
            },
            EventEnvelope {
                schema_version: APPLICATION_CONTRACT_VERSION,
                event_id: EventId::new("project-event-2").unwrap(),
                stream_id,
                task_id: None,
                sequence: 2,
                occurred_at_ms: 2,
                causation_id: Some("command:open-project".to_owned()),
                correlation_id: None,
                payload: ApplicationEvent::WorkspaceReady {
                    workspace_id: "workspace:direct:project-kiln".to_owned(),
                    project_id: "project-kiln".to_owned(),
                    path: "/work/kiln".to_owned(),
                    isolated: false,
                },
            },
        ];

        let projection = ProjectProjection::rebuild(&events).unwrap();
        let project = projection.project.unwrap();
        assert_eq!(project.branch.as_deref(), Some("main"));
        assert_eq!(project.status.modified, 2);
        assert_eq!(project.defaults.model.as_deref(), Some("gpt-5"));
        assert_eq!(
            projection.workspace.unwrap().workspace_id,
            "workspace:direct:project-kiln"
        );
    }
}
