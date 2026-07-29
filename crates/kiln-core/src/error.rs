use serde::Serialize;
use thiserror::Error;

use crate::ProviderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    InvalidConfiguration,
    AuthenticationFailed,
    RateLimited,
    NetworkFailure,
    ProviderFailure,
    MalformedResponse,
    CredentialFailure,
    StorageFailure,
    PermissionDenied,
    Cancelled,
}

/// The stable error envelope exposed over Tauri IPC.
///
/// It intentionally contains no request bodies, response bodies, or credentials.
#[derive(Debug, Clone, Serialize, Error)]
#[error("{message}")]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: ErrorCode,
    pub message: String,
    pub provider: Option<ProviderKind>,
    pub status: Option<u16>,
    pub retryable: bool,
}
