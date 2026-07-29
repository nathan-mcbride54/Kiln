use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use kiln_core::{
    ApplicationEvent, ApprovalDecision, ProjectDefaults, ProjectSnapshot, ProviderKind,
    ProviderProtocol, ReceiptOutcome, RepositoryStatus, RepositoryToolFailureCode,
    RepositoryToolOutcome, RepositoryToolResult, StreamId, TaskId, TaskProjection, TokenUsage,
    TurnId,
};
use kiln_orchestrator::{
    ApprovalGate, ApprovalGateError, ApprovalPrompt, ProviderTaskSession, ProviderTaskSessionError,
    ProviderTaskTurn, ProviderToolContinuation, TaskLoopError, TaskLoopRequest, TaskOrchestrator,
    MAX_PROVIDER_EVENTS_PER_TURN,
};
use kiln_platform::{CancellationToken, Clock};
use kiln_providers::{ProviderTurnEvent, ToolTurnCodec};
use kiln_storage::SqliteEventStore;
use kiln_workspace::WorkspaceToolService;
use serde_json::{json, Value};

const ORIGINAL: &str = "pub fn kiln() -> &'static str { \"before\" }\n";
const UPDATED: &str = "pub fn kiln() -> &'static str { \"after\" }\n";

#[derive(Debug, Clone, Copy)]
struct FixedClock;

impl Clock for FixedClock {
    fn now_unix_ms(&self) -> u64 {
        1_785_363_600_000
    }
}

struct TestRepository {
    root: PathBuf,
}

impl TestRepository {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiln-orchestrator-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        assert!(Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["init", "--quiet"])
            .status()
            .unwrap()
            .success());
        fs::write(root.join("src/lib.rs"), ORIGINAL).unwrap();
        assert!(Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["add", "src/lib.rs"])
            .status()
            .unwrap()
            .success());
        Self { root }
    }

    fn project(&self, label: &str) -> ProjectSnapshot {
        ProjectSnapshot {
            project_id: format!("project-{label}"),
            display_name: label.to_owned(),
            root: self.root.to_string_lossy().into_owned(),
            branch: None,
            head: None,
            status: RepositoryStatus::default(),
            defaults: ProjectDefaults::default(),
        }
    }

    fn content(&self) -> String {
        fs::read_to_string(self.root.join("src/lib.rs")).unwrap()
    }
}

impl Drop for TestRepository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct FixedApproval(ApprovalDecision);

#[async_trait]
impl ApprovalGate for FixedApproval {
    async fn decide(
        &self,
        _prompt: &ApprovalPrompt,
        _cancellation: &CancellationToken,
    ) -> Result<ApprovalDecision, ApprovalGateError> {
        Ok(self.0)
    }
}

struct CancellingApproval;

#[async_trait]
impl ApprovalGate for CancellingApproval {
    async fn decide(
        &self,
        _prompt: &ApprovalPrompt,
        cancellation: &CancellationToken,
    ) -> Result<ApprovalDecision, ApprovalGateError> {
        cancellation.cancel();
        tokio::task::yield_now().await;
        Ok(ApprovalDecision::Approved)
    }
}

struct ReadEditSession {
    provider: ProviderKind,
    protocol: ProviderProtocol,
    model: String,
    stage: usize,
    saw_write_failure: bool,
}

impl ReadEditSession {
    fn new(provider: ProviderKind, protocol: ProviderProtocol) -> Self {
        Self {
            provider,
            protocol,
            model: format!("fixture-{}", provider.as_str()),
            stage: 0,
            saw_write_failure: false,
        }
    }
}

struct EventFloodSession;

#[async_trait]
impl ProviderTaskSession for EventFloodSession {
    fn provider(&self) -> ProviderKind {
        ProviderKind::Local
    }

    fn model(&self) -> &str {
        "fixture-flood"
    }

    async fn next_turn(
        &mut self,
        _continuations: Vec<ProviderToolContinuation>,
        _cancellation: &CancellationToken,
    ) -> Result<ProviderTaskTurn, ProviderTaskSessionError> {
        let mut events = (0..MAX_PROVIDER_EVENTS_PER_TURN)
            .map(|_| ProviderTurnEvent::MessageDelta {
                delta: "x".to_owned(),
            })
            .collect::<Vec<_>>();
        events.push(ProviderTurnEvent::Completed {
            finish_reason: Some("stop".to_owned()),
        });
        Ok(ProviderTaskTurn {
            events,
            model: "fixture-flood".to_owned(),
            usage: TokenUsage::default(),
        })
    }
}

#[async_trait]
impl ProviderTaskSession for ReadEditSession {
    fn provider(&self) -> ProviderKind {
        self.provider
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn next_turn(
        &mut self,
        continuations: Vec<ProviderToolContinuation>,
        _cancellation: &CancellationToken,
    ) -> Result<ProviderTaskTurn, ProviderTaskSessionError> {
        let events = match self.stage {
            0 => {
                assert!(continuations.is_empty());
                decoded_tool_turn(
                    self.protocol,
                    "read",
                    "read_file",
                    json!({"path": "src/lib.rs"}),
                )
            }
            1 => {
                let continuation = only_continuation(continuations);
                let sha256 = match continuation.outcome() {
                    RepositoryToolOutcome::Success {
                        result: RepositoryToolResult::ReadFile(result),
                    } => result.sha256.clone(),
                    other => panic!("expected successful read continuation, got {other:?}"),
                };
                decoded_tool_turn(
                    self.protocol,
                    "write",
                    "write_file",
                    json!({
                        "path": "src/lib.rs",
                        "content": UPDATED,
                        "expectedSha256": sha256
                    }),
                )
            }
            2 => {
                let continuation = only_continuation(continuations);
                self.saw_write_failure = continuation.outcome().is_failure();
                let content = if self.saw_write_failure {
                    "The repository write was not applied."
                } else {
                    "Updated the repository and prepared the diff for review."
                };
                vec![
                    ProviderTurnEvent::MessageDelta {
                        delta: content.to_owned(),
                    },
                    ProviderTurnEvent::Completed {
                        finish_reason: Some("stop".to_owned()),
                    },
                ]
            }
            _ => return Err(ProviderTaskSessionError::InvalidResponse),
        };
        self.stage += 1;
        Ok(ProviderTaskTurn {
            events,
            model: self.model.clone(),
            usage: TokenUsage {
                input_tokens: Some(self.stage as u64),
                output_tokens: Some(1),
                total_tokens: Some(self.stage as u64 + 1),
            },
        })
    }
}

fn only_continuation(continuations: Vec<ProviderToolContinuation>) -> ProviderToolContinuation {
    assert_eq!(continuations.len(), 1);
    continuations.into_iter().next().unwrap()
}

fn decoded_tool_turn(
    protocol: ProviderProtocol,
    suffix: &str,
    name: &str,
    arguments: Value,
) -> Vec<ProviderTurnEvent> {
    let arguments = serde_json::to_string(&arguments).unwrap();
    let raw = match protocol {
        ProviderProtocol::OpenAiResponses => vec![
            json!({
                "type": "response.output_item.added",
                "item": {
                    "type": "function_call",
                    "id": format!("fc_{suffix}"),
                    "call_id": format!("call_{suffix}_private"),
                    "name": name,
                    "arguments": ""
                }
            })
            .to_string(),
            json!({
                "type": "response.function_call_arguments.done",
                "item_id": format!("fc_{suffix}"),
                "name": name,
                "arguments": arguments
            })
            .to_string(),
            json!({"type": "response.completed", "response": {}}).to_string(),
        ],
        ProviderProtocol::AnthropicMessages => vec![
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "tool_use",
                    "id": format!("toolu_{suffix}_private"),
                    "name": name,
                    "input": arguments.parse::<Value>().unwrap()
                }
            })
            .to_string(),
            json!({"type": "content_block_stop", "index": 0}).to_string(),
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": "tool_use"}
            })
            .to_string(),
            json!({"type": "message_stop"}).to_string(),
        ],
        ProviderProtocol::OpenAiChatCompletions => vec![
            json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": format!("call_{suffix}_private"),
                            "function": {
                                "name": name,
                                "arguments": arguments
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            })
            .to_string(),
            "[DONE]".to_owned(),
        ],
    };
    let mut codec = ToolTurnCodec::new(protocol);
    let mut events = Vec::new();
    for data in raw {
        events.extend(codec.push(&data).unwrap());
    }
    codec.finish().unwrap();
    events
}

fn request(label: &str, project_id: &str) -> TaskLoopRequest {
    TaskLoopRequest {
        command_id: format!("command:{label}"),
        stream_id: StreamId::new(format!("task:{label}")).unwrap(),
        task_id: TaskId::new(label).unwrap(),
        session_id: kiln_core::SessionId::new(format!("session:{label}")).unwrap(),
        turn_id: TurnId::new(format!("turn:{label}")).unwrap(),
        project_id: project_id.to_owned(),
        title: "Update the fixture repository".to_owned(),
        prompt: "Read src/lib.rs, update it, and report the result.".to_owned(),
    }
}

async fn setup(
    label: &str,
) -> (
    TestRepository,
    SqliteEventStore,
    WorkspaceToolService,
    TaskLoopRequest,
) {
    let repository = TestRepository::new(label);
    let project = repository.project(label);
    let tools = WorkspaceToolService::default();
    tools.register(&project).unwrap();
    let storage = SqliteEventStore::in_memory().await.unwrap();
    let request = request(label, &project.project_id);
    (repository, storage, tools, request)
}

#[tokio::test]
async fn approved_read_edit_review_is_causal_across_all_provider_protocols() {
    for (provider, protocol) in [
        (ProviderKind::OpenAi, ProviderProtocol::OpenAiResponses),
        (ProviderKind::Anthropic, ProviderProtocol::AnthropicMessages),
        (ProviderKind::Local, ProviderProtocol::OpenAiChatCompletions),
    ] {
        let label = provider.as_str();
        let (repository, storage, tools, request) = setup(label).await;
        let orchestrator = TaskOrchestrator::with_clock(storage.clone(), tools, FixedClock);
        let mut session = ReadEditSession::new(provider, protocol);
        let result = orchestrator
            .run(
                request.clone(),
                &mut session,
                &FixedApproval(ApprovalDecision::Approved),
                CancellationToken::default(),
            )
            .await
            .unwrap();

        assert_eq!(result.outcome, ReceiptOutcome::Completed);
        assert_eq!(result.provider_steps, 3);
        assert_eq!(result.repository_calls, 2);
        assert_eq!(result.total_usage.input_tokens, Some(6));
        assert_eq!(repository.content(), UPDATED);
        assert!(!session.saw_write_failure);

        let events = storage.load_stream(&request.stream_id).await.unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event.payload, ApplicationEvent::TurnReceipt { .. }))
                .count(),
            1
        );
        assert!(events.windows(2).all(|pair| {
            pair[1].sequence == pair[0].sequence + 1
                && pair[1].causation_id.as_deref() == Some(pair[0].event_id.as_str())
        }));
        let tool_names = events
            .iter()
            .filter_map(|event| match &event.payload {
                ApplicationEvent::ToolProposed { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(tool_names, vec!["read_file", "write_file"]);
        let serialized = serde_json::to_string(&events).unwrap();
        assert!(!serialized.contains("call_read_private"));
        assert!(!serialized.contains("toolu_read_private"));
        assert!(!serialized.contains(ORIGINAL));
        assert!(!serialized.contains(UPDATED));
        assert!(events.iter().any(|event| matches!(
            event.payload,
            ApplicationEvent::ArtifactPublished {
                kind: kiln_core::ArtifactKind::Diff,
                ..
            }
        )));
    }
}

#[tokio::test]
async fn declined_write_returns_a_causal_failure_without_a_side_effect() {
    let (repository, storage, tools, request) = setup("declined").await;
    let orchestrator = TaskOrchestrator::with_clock(storage.clone(), tools, FixedClock);
    let mut session = ReadEditSession::new(ProviderKind::OpenAi, ProviderProtocol::OpenAiResponses);
    let result = orchestrator
        .run(
            request.clone(),
            &mut session,
            &FixedApproval(ApprovalDecision::Denied),
            CancellationToken::default(),
        )
        .await
        .unwrap();

    assert_eq!(result.outcome, ReceiptOutcome::Completed);
    assert_eq!(repository.content(), ORIGINAL);
    assert!(session.saw_write_failure);
    let events = storage.load_stream(&request.stream_id).await.unwrap();
    assert!(events.iter().any(|event| matches!(
        event.payload,
        ApplicationEvent::ApprovalDecided {
            decision: ApprovalDecision::Denied,
            ..
        }
    )));
    let write_id = events
        .iter()
        .find_map(|event| match &event.payload {
            ApplicationEvent::ToolProposed {
                tool_call_id, name, ..
            } if name == "write_file" => Some(tool_call_id.clone()),
            _ => None,
        })
        .unwrap();
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        ApplicationEvent::ToolStarted { tool_call_id } if tool_call_id == &write_id
    )));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        ApplicationEvent::ToolCompleted {
            tool_call_id,
            success: false,
            ..
        } if tool_call_id == &write_id
    )));
}

#[tokio::test]
async fn cancellation_while_waiting_for_approval_has_one_terminal_receipt() {
    let (repository, storage, tools, request) = setup("cancelled").await;
    let orchestrator = TaskOrchestrator::with_clock(storage.clone(), tools, FixedClock);
    let mut session =
        ReadEditSession::new(ProviderKind::Anthropic, ProviderProtocol::AnthropicMessages);
    let result = orchestrator
        .run(
            request.clone(),
            &mut session,
            &CancellingApproval,
            CancellationToken::default(),
        )
        .await
        .unwrap();

    assert_eq!(result.outcome, ReceiptOutcome::Cancelled);
    assert_eq!(repository.content(), ORIGINAL);
    let events = storage.load_stream(&request.stream_id).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, ApplicationEvent::TurnReceipt { .. }))
            .count(),
        1
    );
    assert!(matches!(
        events.last().map(|event| &event.payload),
        Some(ApplicationEvent::TurnReceipt {
            outcome: ReceiptOutcome::Cancelled,
            ..
        })
    ));
    let projection = TaskProjection::rebuild(&events).unwrap();
    assert_eq!(projection.status, kiln_core::TaskStatus::Cancelled);
    assert!(projection.pending_approval.is_none());
}

#[tokio::test]
async fn task_identity_and_provider_event_budgets_fail_closed() {
    let (_repository, storage, tools, mut bad_request) = setup("bounds").await;
    bad_request.stream_id = StreamId::new("task:not-bounds").unwrap();
    let orchestrator = TaskOrchestrator::with_clock(storage.clone(), tools.clone(), FixedClock);
    let mut session =
        ReadEditSession::new(ProviderKind::Local, ProviderProtocol::OpenAiChatCompletions);
    let error = orchestrator
        .run(
            bad_request,
            &mut session,
            &FixedApproval(ApprovalDecision::Approved),
            CancellationToken::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, TaskLoopError::InvalidRequest));
    assert_eq!(storage.event_count().await.unwrap(), 0);

    let repository = TestRepository::new("event-budget");
    let project = repository.project("event-budget");
    tools.register(&project).unwrap();
    let bounded_request = request("event-budget", &project.project_id);
    let mut session = EventFloodSession;
    let result = orchestrator
        .run(
            bounded_request.clone(),
            &mut session,
            &FixedApproval(ApprovalDecision::Approved),
            CancellationToken::default(),
        )
        .await
        .unwrap();
    assert_eq!(result.outcome, ReceiptOutcome::Failed);
    let events = storage
        .load_stream(&bounded_request.stream_id)
        .await
        .unwrap();
    assert!(matches!(
        events.last().map(|event| &event.payload),
        Some(ApplicationEvent::TurnReceipt {
            outcome: ReceiptOutcome::Failed,
            ..
        })
    ));
}

#[test]
fn continuation_debug_output_excludes_transient_results_and_handles() {
    let call = decoded_tool_turn(
        ProviderProtocol::OpenAiResponses,
        "debug",
        "read_file",
        json!({"path": "private.rs"}),
    )
    .into_iter()
    .find_map(|event| match event {
        ProviderTurnEvent::ToolCall { call } => Some(call),
        _ => None,
    })
    .unwrap();
    let outcome = RepositoryToolOutcome::failure(
        RepositoryToolFailureCode::ExecutionFailed,
        "private result",
    )
    .unwrap();
    let debug = format!("{:?}", ProviderToolContinuation::new(call, outcome));

    assert!(!debug.contains("call_debug_private"));
    assert!(!debug.contains("private.rs"));
    assert!(!debug.contains("private result"));
}

#[test]
fn fixture_repository_path_is_absolute() {
    let repository = TestRepository::new("absolute");
    assert!(Path::new(&repository.root).is_absolute());
}
