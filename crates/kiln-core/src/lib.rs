//! Transport-independent domain contracts for Kiln.
//!
//! This crate is deliberately unaware of Tauri, HTTP clients, UI frameworks,
//! and operating-system services. Desktop, CLI, and future headless
//! transports all share these serialized application types.

mod error;
mod events;
mod policy;
mod projection;
mod projects;
mod tools;
mod types;

pub use error::{CommandError, ErrorCode};
pub use events::{
    ApplicationEvent, ApprovalDecision, ApprovalScope, ArtifactKind, CommandEnvelope,
    ContractError, EventEnvelope, EventId, EventSequence, ReceiptOutcome, SessionId, StreamId,
    TaskId, TaskStatus, ToolOutputStream, TurnId, APPLICATION_CONTRACT_VERSION,
};
pub use policy::{
    ActionOrigin, ActionProposal, GuardedExecutionError, OriginMatcher, PathOperation,
    PermissionDecision, PermissionEngine, PermissionResource, PolicyContext, PolicyEffect,
    PolicyError, PolicyRule, PolicyTarget, ResourceMatcher,
};
pub use projection::{
    ApprovalProjection, ArtifactProjection, MessageProjection, ProjectProjection,
    ReceiptProjection, SessionProjection, TaskProjection, ToolProjection, ToolProjectionStatus,
    WorkspaceProjection, PROJECT_PROJECTION_VERSION, TASK_PROJECTION_VERSION,
};
pub use projects::{
    OpenProjectRequest, ProjectDefaults, ProjectSnapshot, RememberedProject, RepositoryStatus,
};
pub use tools::{
    FileMatch, ReadFileRequest, ReadFileResult, RepositoryToolExecution, RepositoryToolRequest,
    RepositoryToolResult, SearchFilesRequest, SearchFilesResult, SearchTextRequest,
    SearchTextResult, TextMatch, ToolContractError, WriteFileRequest, WriteFileResult,
    DEFAULT_READ_LINE_COUNT, DEFAULT_SEARCH_RESULTS, MAX_READ_LINE_COUNT, MAX_SEARCH_RESULTS,
    MAX_WRITE_BYTES,
};
pub use types::{
    ChatMessage, ChatRequest, ChatResponse, ChatRole, ChatStreamEvent, ConnectionTestRequest,
    ConnectionTestResponse, ProviderCapabilities, ProviderCredentials, ProviderKind,
    ProviderProtocol, SecretString, TokenUsage,
};
