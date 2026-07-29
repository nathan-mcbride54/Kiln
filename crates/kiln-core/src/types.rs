use std::{
    collections::BTreeMap,
    fmt::{self, Debug, Formatter},
};

use serde::{de, Deserialize, Deserializer, Serialize};
use thiserror::Error;
use zeroize::Zeroize;

use crate::SensitiveDataRedactor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
    Local,
}

impl ProviderKind {
    pub const ALL: [Self; 3] = [Self::OpenAi, Self::Anthropic, Self::Local];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Local => "local",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProtocol {
    OpenAiResponses,
    AnthropicMessages,
    OpenAiChatCompletions,
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Exposes the wrapped value at a trusted transport boundary.
    ///
    /// Callers should keep the returned value ephemeral and must never place
    /// it in logs, events, diagnostics, or durable state.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    pub fn is_blank(&self) -> bool {
        self.0.trim().is_empty()
    }

    pub fn clear(&mut self) {
        self.0.zeroize();
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.clear();
    }
}

impl Debug for SecretString {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

#[derive(Clone, Default)]
pub struct ProviderCredentials {
    pub api_key: Option<SecretString>,
    pub organization: Option<String>,
    pub project: Option<String>,
    /// Ephemeral per-request headers for compatible gateways.
    ///
    /// Values are redacted by `Debug` and the entire credentials object is
    /// consumed only by the current request.
    pub custom_headers: BTreeMap<String, SecretString>,
}

impl ProviderCredentials {
    pub fn redactor(&self) -> SensitiveDataRedactor {
        let mut secrets = Vec::with_capacity(1 + self.custom_headers.len());
        secrets.extend(self.api_key.iter().cloned());
        secrets.extend(self.custom_headers.values().cloned());
        SensitiveDataRedactor::new(secrets)
    }
}

impl Debug for ProviderCredentials {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderCredentials")
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("organization", &self.organization)
            .field("project", &self.project)
            .field(
                "custom_headers",
                &self
                    .custom_headers
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialBackendKind {
    WindowsCredentialManager,
    LinuxSecretService,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("credential profile reference is invalid")]
pub struct CredentialReferenceError;

#[derive(Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct CredentialProfileRef(String);

impl CredentialProfileRef {
    pub fn new(value: impl Into<String>) -> Result<Self, CredentialReferenceError> {
        let value = value.into();
        let suffix = value
            .strip_prefix("cred_")
            .ok_or(CredentialReferenceError)?;
        if suffix.len() != 32
            || !suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(CredentialReferenceError);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for CredentialProfileRef {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CredentialProfileRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialSaveRequest {
    pub provider: ProviderKind,
    pub secret: SecretString,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCredentialProfile {
    pub provider: ProviderKind,
    pub credential_ref: CredentialProfileRef,
    pub backend: CredentialBackendKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    Developer,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub provider: ProviderKind,
    #[serde(default, skip)]
    pub credentials: ProviderCredentials,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<CredentialProfileRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestRequest {
    pub provider: ProviderKind,
    #[serde(default, skip)]
    pub credentials: ProviderCredentials,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<CredentialProfileRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    pub provider: ProviderKind,
    pub display_name: &'static str,
    pub protocol: ProviderProtocol,
    pub default_base_url: &'static str,
    pub api_key_required: bool,
    pub custom_base_url: bool,
    pub custom_headers: bool,
    pub model_discovery: bool,
    pub streaming: bool,
    pub system_messages: bool,
    pub temperature: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResponse {
    pub provider: ProviderKind,
    pub connected: bool,
    pub latency_ms: u64,
    pub discovered_models: Option<usize>,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResponse {
    pub provider: ProviderKind,
    pub id: Option<String>,
    pub model: String,
    pub content: String,
    pub finish_reason: Option<String>,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ChatStreamEvent {
    MessageDelta { delta: String },
    MessageCompleted { response: ChatResponse },
    Cancelled { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_output_is_redacted() {
        let secret = SecretString::new("definitely-secret");
        assert_eq!(format!("{secret:?}"), "[REDACTED]");

        let credentials = ProviderCredentials {
            api_key: Some(secret),
            ..ProviderCredentials::default()
        };
        let debug = format!("{credentials:?}");
        assert!(!debug.contains("definitely-secret"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn secret_can_be_zeroized_before_drop() {
        let mut secret = SecretString::new("definitely-secret");
        secret.clear();
        assert!(secret.expose_secret().is_empty());
    }

    #[test]
    fn credential_reference_rejects_non_opaque_values() {
        assert!(CredentialProfileRef::new("cred_0123456789abcdef0123456789abcdef").is_ok());
        assert!(CredentialProfileRef::new("sk-proj-not-a-reference").is_err());
        assert!(CredentialProfileRef::new("cred_0123456789ABCDEF0123456789ABCDEF").is_err());
    }

    #[test]
    fn request_serialization_excludes_resolved_credentials() {
        let request = ConnectionTestRequest {
            provider: ProviderKind::OpenAi,
            credentials: ProviderCredentials {
                api_key: Some(SecretString::new("must-not-leak")),
                ..ProviderCredentials::default()
            },
            credential_ref: Some(
                CredentialProfileRef::new("cred_0123456789abcdef0123456789abcdef").unwrap(),
            ),
            base_url: None,
        };

        let serialized = serde_json::to_string(&request).unwrap();
        assert!(!serialized.contains("must-not-leak"));
        assert!(!serialized.contains("credentials"));
        assert!(serialized.contains("credentialRef"));
    }

    #[test]
    fn request_deserialization_ignores_raw_credentials() {
        let request: ConnectionTestRequest = serde_json::from_value(serde_json::json!({
            "provider": "openai",
            "credentialRef": "cred_0123456789abcdef0123456789abcdef",
            "credentials": {
                "apiKey": "injected-secret"
            }
        }))
        .unwrap();

        assert!(request.credentials.api_key.is_none());
        assert_eq!(
            request.credential_ref.unwrap().as_str(),
            "cred_0123456789abcdef0123456789abcdef"
        );
    }
}
