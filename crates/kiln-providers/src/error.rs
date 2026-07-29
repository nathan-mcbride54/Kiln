use kiln_core::{CommandError, ErrorCode, ProviderKind};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ProviderError {
    #[error("{0}")]
    InvalidRequest(String),
    #[error("{0}")]
    InvalidConfiguration(String),
    #[error("{0}")]
    Authentication(String),
    #[error("{0}")]
    RateLimited(String),
    #[error("{0}")]
    Network(String),
    #[error("{message}")]
    Upstream { status: u16, message: String },
    #[error("{0}")]
    MalformedResponse(String),
}

impl ProviderError {
    pub(crate) fn into_command(self, provider: ProviderKind) -> CommandError {
        let (code, message, status, retryable) = match self {
            Self::InvalidRequest(message) => (ErrorCode::InvalidRequest, message, None, false),
            Self::InvalidConfiguration(message) => {
                (ErrorCode::InvalidConfiguration, message, None, false)
            }
            Self::Authentication(message) => {
                (ErrorCode::AuthenticationFailed, message, Some(401), false)
            }
            Self::RateLimited(message) => (ErrorCode::RateLimited, message, Some(429), true),
            Self::Network(message) => (ErrorCode::NetworkFailure, message, None, true),
            Self::Upstream { status, message } => (
                ErrorCode::ProviderFailure,
                message,
                Some(status),
                status == 408 || status == 409 || status >= 500,
            ),
            Self::MalformedResponse(message) => {
                (ErrorCode::MalformedResponse, message, None, false)
            }
        };

        CommandError {
            code,
            message,
            provider: Some(provider),
            status,
            retryable,
        }
    }
}
