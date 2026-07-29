//! Tauri-free orchestration for one provider-driven repository task.
//!
//! Provider-native tool handles and payloads remain transient. This crate
//! mints application identities, appends every transition before continuing,
//! pauses at an injected approval boundary, and routes repository operations
//! through the existing workspace policy service.

use std::fmt::{self, Debug, Formatter};

use async_trait::async_trait;
use kiln_core::{
    ApplicationEvent, ApprovalDecision, ApprovalScope, ArtifactKind, ChatRole, ContractError,
    EventEnvelope, EventId, PermissionDecision, ReceiptOutcome, RepositoryToolFailureCode,
    RepositoryToolOutcome, RepositoryToolRequest, RepositoryToolResult, SessionId, StreamId,
    TaskId, TaskStatus, TokenUsage, ToolContractError, ToolOutputStream, TurnId,
    APPLICATION_CONTRACT_VERSION,
};
use kiln_platform::{CancellationToken, Clock, SystemClock};
use kiln_providers::{ProviderToolCall, ProviderTurnEvent, MAX_TOOL_CALLS_PER_TURN};
use kiln_storage::{SqliteEventStore, StorageError};
use kiln_workspace::{WorkspaceToolAuthorization, WorkspaceToolError, WorkspaceToolService};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_PROVIDER_STEPS_PER_TURN: usize = 32;
pub const MAX_PROVIDER_EVENTS_PER_TURN: usize = 4_096;
pub const MAX_REPOSITORY_CALLS_PER_TURN: usize = 64;
pub const MAX_TASK_TEXT_BYTES: usize = 1024 * 1024;
pub const MAX_TASK_TITLE_BYTES: usize = 16 * 1024;
pub const MAX_TASK_IDENTIFIER_CHARS: usize = 256;
pub const MAX_PROVIDER_MODEL_CHARS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskLoopRequest {
    pub command_id: String,
    pub stream_id: StreamId,
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub turn_id: TurnId,
    pub project_id: String,
    pub title: String,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTaskTurn {
    pub events: Vec<ProviderTurnEvent>,
    pub model: String,
    pub usage: TokenUsage,
}

pub struct ProviderToolContinuation {
    call: ProviderToolCall,
    outcome: RepositoryToolOutcome,
}

impl ProviderToolContinuation {
    pub fn new(call: ProviderToolCall, outcome: RepositoryToolOutcome) -> Self {
        Self { call, outcome }
    }

    pub fn call(&self) -> &ProviderToolCall {
        &self.call
    }

    pub fn outcome(&self) -> &RepositoryToolOutcome {
        &self.outcome
    }

    pub fn into_parts(self) -> (ProviderToolCall, RepositoryToolOutcome) {
        (self.call, self.outcome)
    }
}

impl Debug for ProviderToolContinuation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderToolContinuation")
            .field("call", &self.call)
            .field("failed", &self.outcome.is_failure())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProviderTaskSessionError {
    #[error("the provider task session is unavailable")]
    Unavailable,
    #[error("the provider task session returned an invalid response")]
    InvalidResponse,
    #[error("the provider task session was cancelled")]
    Cancelled,
}

#[async_trait]
pub trait ProviderTaskSession: Send {
    fn provider(&self) -> kiln_core::ProviderKind;
    fn model(&self) -> &str;

    async fn next_turn(
        &mut self,
        continuations: Vec<ProviderToolContinuation>,
        cancellation: &CancellationToken,
    ) -> Result<ProviderTaskTurn, ProviderTaskSessionError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalPrompt {
    pub approval_id: String,
    pub tool_call_id: String,
    pub action: String,
    pub resource: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ApprovalGateError {
    #[error("the approval service is unavailable")]
    Unavailable,
}

#[async_trait]
pub trait ApprovalGate: Send + Sync {
    async fn decide(
        &self,
        prompt: &ApprovalPrompt,
        cancellation: &CancellationToken,
    ) -> Result<ApprovalDecision, ApprovalGateError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskLoopResult {
    pub outcome: ReceiptOutcome,
    pub summary: String,
    pub total_usage: TokenUsage,
    pub provider_steps: usize,
    pub repository_calls: usize,
}

#[derive(Debug, Error)]
pub enum TaskLoopError {
    #[error("the task-loop request is invalid")]
    InvalidRequest,
    #[error("the task stream already contains events")]
    StreamAlreadyExists,
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    ToolContract(#[from] ToolContractError),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

pub struct TaskOrchestrator<C = SystemClock> {
    storage: SqliteEventStore,
    workspace_tools: WorkspaceToolService,
    clock: C,
}

impl TaskOrchestrator<SystemClock> {
    pub fn new(storage: SqliteEventStore, workspace_tools: WorkspaceToolService) -> Self {
        Self::with_clock(storage, workspace_tools, SystemClock)
    }
}

impl<C> TaskOrchestrator<C>
where
    C: Clock,
{
    pub fn with_clock(
        storage: SqliteEventStore,
        workspace_tools: WorkspaceToolService,
        clock: C,
    ) -> Self {
        Self {
            storage,
            workspace_tools,
            clock,
        }
    }

    pub async fn run<S, A>(
        &self,
        request: TaskLoopRequest,
        session: &mut S,
        approvals: &A,
        cancellation: CancellationToken,
    ) -> Result<TaskLoopResult, TaskLoopError>
    where
        S: ProviderTaskSession,
        A: ApprovalGate,
    {
        validate_request(&request)?;
        let provider = session.provider();
        let initial_model = session.model().to_owned();
        validate_short_text(&initial_model, MAX_PROVIDER_MODEL_CHARS)?;
        if !self
            .storage
            .load_stream(&request.stream_id)
            .await?
            .is_empty()
        {
            return Err(TaskLoopError::StreamAlreadyExists);
        }

        let mut writer = EventWriter::new(
            &self.storage,
            &self.clock,
            request.stream_id.clone(),
            request.task_id.clone(),
            request.turn_id.clone(),
        );
        let user_message_id = format!("message:{}:user", request.turn_id.as_str());
        let mut cause = writer
            .append(
                request.command_id.clone(),
                vec![
                    ApplicationEvent::TaskCreated {
                        title: request.title.clone(),
                    },
                    ApplicationEvent::TaskStatusChanged {
                        status: TaskStatus::Running,
                    },
                    ApplicationEvent::SessionStarted {
                        session_id: request.session_id.clone(),
                        provider,
                        model: initial_model,
                    },
                    ApplicationEvent::TurnStarted {
                        turn_id: request.turn_id.clone(),
                    },
                    ApplicationEvent::MessageAdded {
                        message_id: user_message_id,
                        role: ChatRole::User,
                        content: request.prompt.clone(),
                    },
                ],
            )
            .await?;

        let mut continuations = Vec::new();
        let mut total_usage = TokenUsage::default();
        let mut repository_calls = 0usize;

        for provider_step in 1..=MAX_PROVIDER_STEPS_PER_TURN {
            if cancellation.is_cancelled() {
                return writer
                    .finish(
                        cause,
                        ReceiptOutcome::Cancelled,
                        "The task was cancelled before the next provider step.",
                        total_usage,
                        provider_step.saturating_sub(1),
                        repository_calls,
                    )
                    .await;
            }

            let provider_turn = match cancellation
                .run(session.next_turn(continuations, &cancellation))
                .await
            {
                Err(_) | Ok(Err(ProviderTaskSessionError::Cancelled)) => {
                    return writer
                        .finish(
                            cause,
                            ReceiptOutcome::Cancelled,
                            "The provider task was cancelled.",
                            total_usage,
                            provider_step.saturating_sub(1),
                            repository_calls,
                        )
                        .await;
                }
                Ok(Err(_)) => {
                    return writer
                        .finish(
                            cause,
                            ReceiptOutcome::Failed,
                            "The provider could not continue this task.",
                            total_usage,
                            provider_step.saturating_sub(1),
                            repository_calls,
                        )
                        .await;
                }
                Ok(Ok(turn)) => turn,
            };

            merge_usage(&mut total_usage, &provider_turn.usage);
            let parsed = match ParsedProviderTurn::parse(provider_turn) {
                Ok(parsed) => parsed,
                Err(()) => {
                    return writer
                        .finish(
                            cause,
                            ReceiptOutcome::Failed,
                            "The provider returned an invalid task turn.",
                            total_usage,
                            provider_step,
                            repository_calls,
                        )
                        .await;
                }
            };

            if !parsed.content.trim().is_empty() {
                let message_id = format!(
                    "message:{}:assistant:{provider_step}",
                    request.turn_id.as_str()
                );
                let mut message_events = parsed
                    .deltas
                    .iter()
                    .filter(|delta| !delta.trim().is_empty())
                    .map(|delta| ApplicationEvent::MessageDelta {
                        message_id: message_id.clone(),
                        delta: delta.clone(),
                    })
                    .collect::<Vec<_>>();
                message_events.push(ApplicationEvent::MessageCompleted {
                    message_id,
                    model: parsed.model.clone(),
                    content: parsed.content.clone(),
                    finish_reason: parsed.finish_reason.clone(),
                    usage: parsed.usage.clone(),
                });
                cause = writer.append(cause, message_events).await?;
            }

            if parsed.calls.is_empty() {
                if parsed.content.trim().is_empty() {
                    return writer
                        .finish(
                            cause,
                            ReceiptOutcome::Failed,
                            "The provider completed without a message or repository call.",
                            total_usage,
                            provider_step,
                            repository_calls,
                        )
                        .await;
                }
                if cancellation.is_cancelled() {
                    return writer
                        .finish(
                            cause,
                            ReceiptOutcome::Cancelled,
                            "The task was cancelled before completion.",
                            total_usage,
                            provider_step,
                            repository_calls,
                        )
                        .await;
                }
                return writer
                    .finish(
                        cause,
                        ReceiptOutcome::Completed,
                        "The provider task completed and is ready for review.",
                        total_usage,
                        provider_step,
                        repository_calls,
                    )
                    .await;
            }

            continuations = Vec::with_capacity(parsed.calls.len());
            for call in parsed.calls {
                repository_calls = repository_calls.saturating_add(1);
                if repository_calls > MAX_REPOSITORY_CALLS_PER_TURN {
                    return writer
                        .finish(
                            cause,
                            ReceiptOutcome::Failed,
                            "The provider exceeded the repository-call budget.",
                            total_usage,
                            provider_step,
                            repository_calls.saturating_sub(1),
                        )
                        .await;
                }
                if cancellation.is_cancelled() {
                    return writer
                        .finish(
                            cause,
                            ReceiptOutcome::Cancelled,
                            "The task was cancelled before the repository call.",
                            total_usage,
                            provider_step,
                            repository_calls.saturating_sub(1),
                        )
                        .await;
                }

                let tool_call_id =
                    format!("tool:{}:{}", request.turn_id.as_str(), repository_calls);
                let approval_id = format!("approval:{tool_call_id}");
                let repository_request = call.repository_request();
                cause = writer
                    .append(
                        cause,
                        vec![ApplicationEvent::ToolProposed {
                            tool_call_id: tool_call_id.clone(),
                            name: call.name().to_owned(),
                            summary: repository_request.as_ref().map_or_else(
                                |_| "Validate a provider repository-tool proposal.".to_owned(),
                                RepositoryToolRequest::proposal_summary,
                            ),
                        }],
                    )
                    .await?;

                let repository_request = match repository_request {
                    Ok(request) => request,
                    Err(_) => {
                        let outcome = failure_outcome(
                            RepositoryToolFailureCode::InvalidRequest,
                            "The provider repository-tool request was invalid.",
                        )?;
                        cause = record_tool_outcome(&mut writer, cause, &tool_call_id, &outcome)
                            .await?;
                        continuations.push(ProviderToolContinuation::new(call, outcome));
                        continue;
                    }
                };

                let authorization = workspace_authorization(
                    self.workspace_tools.clone(),
                    request.project_id.clone(),
                    tool_call_id.clone(),
                    repository_request.clone(),
                )
                .await;
                let authorization = match authorization {
                    Ok(authorization) => authorization,
                    Err(error) => {
                        let outcome = workspace_failure_outcome(&error)?;
                        cause = record_tool_outcome(&mut writer, cause, &tool_call_id, &outcome)
                            .await?;
                        continuations.push(ProviderToolContinuation::new(call, outcome));
                        continue;
                    }
                };

                match authorization {
                    WorkspaceToolAuthorization::Denied { .. } => {
                        let outcome = failure_outcome(
                            RepositoryToolFailureCode::Denied,
                            "Repository policy denied this call.",
                        )?;
                        cause = record_tool_outcome(&mut writer, cause, &tool_call_id, &outcome)
                            .await?;
                        continuations.push(ProviderToolContinuation::new(call, outcome));
                        continue;
                    }
                    WorkspaceToolAuthorization::ApprovalRequired {
                        action,
                        resource,
                        reason,
                    } => {
                        let prompt = ApprovalPrompt {
                            approval_id: approval_id.clone(),
                            tool_call_id: tool_call_id.clone(),
                            action: action.clone(),
                            resource: resource.clone(),
                            reason: reason.clone(),
                        };
                        cause = writer
                            .append(
                                cause,
                                vec![ApplicationEvent::ApprovalRequested {
                                    approval_id: approval_id.clone(),
                                    action,
                                    resource,
                                    reason,
                                }],
                            )
                            .await?;
                        let decision = cancellation
                            .run(approvals.decide(&prompt, &cancellation))
                            .await;
                        let decision = match decision {
                            Err(_) => {
                                let outcome = failure_outcome(
                                    RepositoryToolFailureCode::Cancelled,
                                    "The repository call was cancelled while awaiting approval.",
                                )?;
                                cause = record_tool_outcome(
                                    &mut writer,
                                    cause,
                                    &tool_call_id,
                                    &outcome,
                                )
                                .await?;
                                return writer
                                    .finish(
                                        cause,
                                        ReceiptOutcome::Cancelled,
                                        "The task was cancelled while awaiting approval.",
                                        total_usage,
                                        provider_step,
                                        repository_calls,
                                    )
                                    .await;
                            }
                            Ok(Err(_)) => {
                                let outcome = failure_outcome(
                                    RepositoryToolFailureCode::ExecutionFailed,
                                    "The approval service could not decide this repository call.",
                                )?;
                                cause = record_tool_outcome(
                                    &mut writer,
                                    cause,
                                    &tool_call_id,
                                    &outcome,
                                )
                                .await?;
                                return writer
                                    .finish(
                                        cause,
                                        ReceiptOutcome::Failed,
                                        "The approval service became unavailable.",
                                        total_usage,
                                        provider_step,
                                        repository_calls,
                                    )
                                    .await;
                            }
                            Ok(Ok(decision)) => decision,
                        };
                        cause = writer
                            .append(
                                cause,
                                vec![ApplicationEvent::ApprovalDecided {
                                    approval_id,
                                    decision,
                                    scope: ApprovalScope::Once,
                                }],
                            )
                            .await?;
                        if decision == ApprovalDecision::Denied {
                            let outcome = failure_outcome(
                                RepositoryToolFailureCode::ApprovalDeclined,
                                "The repository write was not approved.",
                            )?;
                            cause =
                                record_tool_outcome(&mut writer, cause, &tool_call_id, &outcome)
                                    .await?;
                            continuations.push(ProviderToolContinuation::new(call, outcome));
                            continue;
                        }

                        if let Err(error) = workspace_approve_once(
                            self.workspace_tools.clone(),
                            request.project_id.clone(),
                            tool_call_id.clone(),
                            repository_request.clone(),
                        )
                        .await
                        {
                            let outcome = workspace_failure_outcome(&error)?;
                            cause =
                                record_tool_outcome(&mut writer, cause, &tool_call_id, &outcome)
                                    .await?;
                            continuations.push(ProviderToolContinuation::new(call, outcome));
                            continue;
                        }
                    }
                    WorkspaceToolAuthorization::Allowed => {}
                }

                cause = writer
                    .append(
                        cause,
                        vec![ApplicationEvent::ToolStarted {
                            tool_call_id: tool_call_id.clone(),
                        }],
                    )
                    .await?;
                let execution = workspace_execute(
                    self.workspace_tools.clone(),
                    request.project_id.clone(),
                    tool_call_id.clone(),
                    repository_request,
                    cancellation.clone(),
                )
                .await;
                let outcome = match execution {
                    Ok(result) => RepositoryToolOutcome::success(result),
                    Err(WorkspaceOperationError::Host(WorkspaceToolError::Cancelled))
                    | Err(WorkspaceOperationError::Runtime)
                        if cancellation.is_cancelled() =>
                    {
                        let outcome = failure_outcome(
                            RepositoryToolFailureCode::Cancelled,
                            "The repository call was cancelled.",
                        )?;
                        cause = record_tool_outcome(&mut writer, cause, &tool_call_id, &outcome)
                            .await?;
                        return writer
                            .finish(
                                cause,
                                ReceiptOutcome::Cancelled,
                                "The task was cancelled during repository execution.",
                                total_usage,
                                provider_step,
                                repository_calls,
                            )
                            .await;
                    }
                    Err(error) => workspace_failure_outcome(&error)?,
                };
                cause = record_tool_outcome(&mut writer, cause, &tool_call_id, &outcome).await?;
                continuations.push(ProviderToolContinuation::new(call, outcome));
            }
        }

        writer
            .finish(
                cause,
                ReceiptOutcome::Failed,
                "The provider exceeded the task-step budget.",
                total_usage,
                MAX_PROVIDER_STEPS_PER_TURN,
                repository_calls,
            )
            .await
    }
}

struct ParsedProviderTurn {
    calls: Vec<ProviderToolCall>,
    deltas: Vec<String>,
    content: String,
    finish_reason: Option<String>,
    model: String,
    usage: TokenUsage,
}

impl ParsedProviderTurn {
    fn parse(turn: ProviderTaskTurn) -> Result<Self, ()> {
        if !is_valid_short_text(&turn.model, MAX_PROVIDER_MODEL_CHARS)
            || turn.events.len() > MAX_PROVIDER_EVENTS_PER_TURN
        {
            return Err(());
        }
        let mut calls = Vec::new();
        let mut deltas = Vec::new();
        let mut content = String::new();
        let mut finish_reason = None;
        let mut completed = false;

        for event in turn.events {
            if completed {
                return Err(());
            }
            match event {
                ProviderTurnEvent::MessageDelta { delta } => {
                    if content.len().saturating_add(delta.len()) > MAX_TASK_TEXT_BYTES {
                        return Err(());
                    }
                    if delta.chars().any(is_disallowed_control) {
                        return Err(());
                    }
                    content.push_str(&delta);
                    deltas.push(delta);
                }
                ProviderTurnEvent::ToolCall { call } => {
                    if calls.len() >= MAX_TOOL_CALLS_PER_TURN {
                        return Err(());
                    }
                    calls.push(call);
                }
                ProviderTurnEvent::Completed {
                    finish_reason: reason,
                } => {
                    if reason
                        .as_deref()
                        .is_some_and(|reason| !is_valid_short_text(reason, 64))
                    {
                        return Err(());
                    }
                    completed = true;
                    finish_reason = reason;
                }
            }
        }
        if !completed {
            return Err(());
        }
        Ok(Self {
            calls,
            deltas,
            content,
            finish_reason,
            model: turn.model,
            usage: turn.usage,
        })
    }
}

struct EventWriter<'a, C> {
    storage: &'a SqliteEventStore,
    clock: &'a C,
    stream_id: StreamId,
    task_id: TaskId,
    turn_id: TurnId,
    next_sequence: u64,
}

impl<'a, C> EventWriter<'a, C>
where
    C: Clock,
{
    fn new(
        storage: &'a SqliteEventStore,
        clock: &'a C,
        stream_id: StreamId,
        task_id: TaskId,
        turn_id: TurnId,
    ) -> Self {
        Self {
            storage,
            clock,
            stream_id,
            task_id,
            turn_id,
            next_sequence: 1,
        }
    }

    async fn append(
        &mut self,
        causation_id: String,
        payloads: Vec<ApplicationEvent>,
    ) -> Result<String, TaskLoopError> {
        let mut events = Vec::with_capacity(payloads.len());
        let mut direct_cause = causation_id;
        for payload in payloads {
            let event_id = format!("event:{}:{}", self.task_id.as_str(), self.next_sequence);
            events.push(EventEnvelope {
                schema_version: APPLICATION_CONTRACT_VERSION,
                event_id: EventId::new(event_id.clone())?,
                stream_id: self.stream_id.clone(),
                task_id: Some(self.task_id.clone()),
                sequence: self.next_sequence,
                occurred_at_ms: self.clock.now_unix_ms(),
                causation_id: Some(direct_cause),
                correlation_id: Some(self.turn_id.as_str().to_owned()),
                payload,
            });
            direct_cause = event_id;
            self.next_sequence = self.next_sequence.saturating_add(1);
        }
        self.storage.append(&events).await?;
        Ok(direct_cause)
    }

    async fn finish(
        &mut self,
        causation_id: String,
        outcome: ReceiptOutcome,
        summary: &str,
        total_usage: TokenUsage,
        provider_steps: usize,
        repository_calls: usize,
    ) -> Result<TaskLoopResult, TaskLoopError> {
        self.append(
            causation_id,
            vec![ApplicationEvent::TurnReceipt {
                turn_id: self.turn_id.clone(),
                outcome,
                summary: summary.to_owned(),
            }],
        )
        .await?;
        Ok(TaskLoopResult {
            outcome,
            summary: summary.to_owned(),
            total_usage,
            provider_steps,
            repository_calls,
        })
    }
}

#[derive(Debug)]
enum WorkspaceOperationError {
    Host(WorkspaceToolError),
    Runtime,
}

async fn workspace_authorization(
    tools: WorkspaceToolService,
    project_id: String,
    tool_call_id: String,
    request: RepositoryToolRequest,
) -> Result<WorkspaceToolAuthorization, WorkspaceOperationError> {
    tokio::task::spawn_blocking(move || tools.authorization(&project_id, &tool_call_id, &request))
        .await
        .map_err(|_| WorkspaceOperationError::Runtime)?
        .map_err(WorkspaceOperationError::Host)
}

async fn workspace_approve_once(
    tools: WorkspaceToolService,
    project_id: String,
    tool_call_id: String,
    request: RepositoryToolRequest,
) -> Result<(), WorkspaceOperationError> {
    tokio::task::spawn_blocking(move || tools.approve_once(&project_id, &tool_call_id, &request))
        .await
        .map_err(|_| WorkspaceOperationError::Runtime)?
        .map_err(WorkspaceOperationError::Host)
}

async fn workspace_execute(
    tools: WorkspaceToolService,
    project_id: String,
    tool_call_id: String,
    request: RepositoryToolRequest,
    cancellation: CancellationToken,
) -> Result<RepositoryToolResult, WorkspaceOperationError> {
    tokio::task::spawn_blocking(move || {
        tools.execute(&project_id, &tool_call_id, request, &cancellation)
    })
    .await
    .map_err(|_| WorkspaceOperationError::Runtime)?
    .map_err(WorkspaceOperationError::Host)
}

async fn record_tool_outcome<C>(
    writer: &mut EventWriter<'_, C>,
    causation_id: String,
    tool_call_id: &str,
    outcome: &RepositoryToolOutcome,
) -> Result<String, TaskLoopError>
where
    C: Clock,
{
    let mut events = match outcome {
        RepositoryToolOutcome::Success { result } => vec![
            ApplicationEvent::ToolOutput {
                tool_call_id: tool_call_id.to_owned(),
                stream: ToolOutputStream::Structured,
                chunk: result.activity_summary(),
            },
            ApplicationEvent::ToolCompleted {
                tool_call_id: tool_call_id.to_owned(),
                success: true,
                exit_code: None,
            },
        ],
        RepositoryToolOutcome::Failure { message, .. } => vec![
            ApplicationEvent::ToolOutput {
                tool_call_id: tool_call_id.to_owned(),
                stream: ToolOutputStream::Stderr,
                chunk: message.clone(),
            },
            ApplicationEvent::ToolCompleted {
                tool_call_id: tool_call_id.to_owned(),
                success: false,
                exit_code: None,
            },
        ],
    };
    if let RepositoryToolOutcome::Success {
        result: RepositoryToolResult::WriteFile(result),
    } = outcome
    {
        events.push(ApplicationEvent::ArtifactPublished {
            artifact_id: format!("artifact:{tool_call_id}"),
            kind: ArtifactKind::Diff,
            label: result.path.clone(),
        });
    }
    writer.append(causation_id, events).await
}

fn failure_outcome(
    code: RepositoryToolFailureCode,
    message: impl Into<String>,
) -> Result<RepositoryToolOutcome, ToolContractError> {
    RepositoryToolOutcome::failure(code, message)
}

fn workspace_failure_outcome(
    error: &WorkspaceOperationError,
) -> Result<RepositoryToolOutcome, ToolContractError> {
    match error {
        WorkspaceOperationError::Host(error) => {
            let code = match error {
                WorkspaceToolError::Cancelled => RepositoryToolFailureCode::Cancelled,
                WorkspaceToolError::ApprovalDeclined => RepositoryToolFailureCode::ApprovalDeclined,
                WorkspaceToolError::VersionMismatch
                | WorkspaceToolError::ExpectedVersionRequired
                | WorkspaceToolError::NoChanges => RepositoryToolFailureCode::Conflict,
                WorkspaceToolError::NotAuthorized(
                    kiln_core::GuardedExecutionError::NotAuthorized(PermissionDecision::Deny {
                        ..
                    }),
                ) => RepositoryToolFailureCode::Denied,
                WorkspaceToolError::InvalidWorkspace
                | WorkspaceToolError::InvalidToolCall
                | WorkspaceToolError::ApprovalNotApplicable
                | WorkspaceToolError::InvalidRequest(_)
                | WorkspaceToolError::OutsideWorkspace
                | WorkspaceToolError::PathMissing
                | WorkspaceToolError::ParentMissing
                | WorkspaceToolError::ProtectedPath
                | WorkspaceToolError::SymlinkWrite
                | WorkspaceToolError::NotFile
                | WorkspaceToolError::InvalidSearchScope
                | WorkspaceToolError::StartPastEnd
                | WorkspaceToolError::FileTooLarge
                | WorkspaceToolError::BinaryFile
                | WorkspaceToolError::NonUnicodePath => RepositoryToolFailureCode::InvalidRequest,
                _ => RepositoryToolFailureCode::ExecutionFailed,
            };
            failure_outcome(code, error.user_message())
        }
        WorkspaceOperationError::Runtime => failure_outcome(
            RepositoryToolFailureCode::ExecutionFailed,
            "The repository worker could not complete the tool.",
        ),
    }
}

fn validate_request(request: &TaskLoopRequest) -> Result<(), TaskLoopError> {
    for value in [
        request.command_id.as_str(),
        request.project_id.as_str(),
        request.stream_id.as_str(),
        request.task_id.as_str(),
        request.session_id.as_str(),
        request.turn_id.as_str(),
    ] {
        validate_short_text(value, MAX_TASK_IDENTIFIER_CHARS)?;
    }
    if request.stream_id.as_str() != format!("task:{}", request.task_id.as_str()) {
        return Err(TaskLoopError::InvalidRequest);
    }
    validate_text(&request.title, MAX_TASK_TITLE_BYTES)?;
    validate_text(&request.prompt, MAX_TASK_TEXT_BYTES)?;
    Ok(())
}

fn validate_text(value: &str, max_bytes: usize) -> Result<(), TaskLoopError> {
    if value.trim().is_empty()
        || value.len() > max_bytes
        || value.chars().any(is_disallowed_control)
    {
        Err(TaskLoopError::InvalidRequest)
    } else {
        Ok(())
    }
}

fn validate_short_text(value: &str, max_chars: usize) -> Result<(), TaskLoopError> {
    if is_valid_short_text(value, max_chars) {
        Ok(())
    } else {
        Err(TaskLoopError::InvalidRequest)
    }
}

fn is_valid_short_text(value: &str, max_chars: usize) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= max_chars
        && !value.chars().any(char::is_control)
}

fn is_disallowed_control(character: char) -> bool {
    character.is_control() && !matches!(character, '\n' | '\r' | '\t')
}

fn merge_usage(total: &mut TokenUsage, next: &TokenUsage) {
    total.input_tokens = merge_optional_count(total.input_tokens, next.input_tokens);
    total.output_tokens = merge_optional_count(total.output_tokens, next.output_tokens);
    total.total_tokens = merge_optional_count(total.total_tokens, next.total_tokens);
}

fn merge_optional_count(current: Option<u64>, next: Option<u64>) -> Option<u64> {
    match (current, next) {
        (Some(current), Some(next)) => Some(current.saturating_add(next)),
        (Some(current), None) => Some(current),
        (None, Some(next)) => Some(next),
        (None, None) => None,
    }
}
