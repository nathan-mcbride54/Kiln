use kiln_core::{CommandError, ErrorCode, ProviderKind, SensitiveDataRedactor};
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
    pub(crate) fn into_command(
        self,
        provider: ProviderKind,
        redactor: &SensitiveDataRedactor,
    ) -> CommandError {
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
            message: redactor.redact(&message),
            provider: Some(provider),
            status,
            retryable,
        }
    }
}

#[cfg(test)]
mod tests {
    use kiln_core::SecretString;

    use super::*;

    #[test]
    fn provider_errors_are_redacted_before_crossing_the_command_boundary() {
        let redactor = SensitiveDataRedactor::new([SecretString::new("opaque-upstream-secret")]);
        let error = ProviderError::Upstream {
            status: 400,
            message: "OpenAI echoed opaque-upstream-secret in an error".to_owned(),
        }
        .into_command(ProviderKind::OpenAi, &redactor);

        assert!(!error.message.contains("opaque-upstream-secret"));
        assert!(error.message.contains("[REDACTED]"));
    }
}
