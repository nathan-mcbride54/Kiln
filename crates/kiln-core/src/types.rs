use std::{
    collections::BTreeMap,
    fmt::{self, Debug, Formatter},
};

use serde::{de, Deserialize, Deserializer, Serialize};
use thiserror::Error;
use url::{Host, ParseError, Url};
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

    pub const fn fixed_official_base_url(self) -> Option<&'static str> {
        match self {
            Self::OpenAi => Some("https://api.openai.com/v1"),
            Self::Anthropic => Some("https://api.anthropic.com/v1"),
            Self::Local => None,
        }
    }

    pub const fn fixed_official_origin_str(self) -> Option<&'static str> {
        match self {
            Self::OpenAi => Some("https://api.openai.com"),
            Self::Anthropic => Some("https://api.anthropic.com"),
            Self::Local => None,
        }
    }

    pub fn fixed_official_origin(self) -> Option<ProviderOrigin> {
        self.fixed_official_origin_str()
            .map(|origin| ProviderOrigin(origin.to_owned()))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProviderOriginError {
    #[error("the provider base URL must be a valid absolute URL")]
    InvalidUrl,
    #[error("the provider base URL must use HTTP or HTTPS")]
    UnsupportedScheme,
    #[error("the provider base URL must include a host")]
    MissingHost,
    #[error("the provider base URL cannot contain credentials")]
    EmbeddedCredentials,
    #[error("the provider base URL cannot contain a query string or fragment")]
    QueryOrFragment,
}

/// A canonical credential destination derived from an HTTP(S) provider base URL.
///
/// Paths intentionally do not participate in credential binding. A destination
/// is the normalized scheme, host, and effective non-default port.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ProviderOrigin(String);

impl ProviderOrigin {
    pub fn from_base_url(value: &str) -> Result<Self, ProviderOriginError> {
        let parsed = Url::parse(value.trim()).map_err(|error| match error {
            ParseError::EmptyHost => ProviderOriginError::MissingHost,
            _ => ProviderOriginError::InvalidUrl,
        })?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(ProviderOriginError::UnsupportedScheme);
        }
        if parsed.host().is_none() {
            return Err(ProviderOriginError::MissingHost);
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(ProviderOriginError::EmbeddedCredentials);
        }
        if parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(ProviderOriginError::QueryOrFragment);
        }

        Ok(Self(parsed.origin().ascii_serialization()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns whether credentials can be transported without cleartext over
    /// a non-loopback network.
    pub fn is_https_or_loopback(&self) -> bool {
        let parsed =
            Url::parse(&self.0).expect("ProviderOrigin values are validated before construction");
        if parsed.scheme() == "https" {
            return true;
        }

        match parsed.host() {
            Some(Host::Ipv4(address)) => address.is_loopback(),
            Some(Host::Ipv6(address)) => address.is_loopback(),
            Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
            None => false,
        }
    }

    pub fn is_safe_for_credentials(&self) -> bool {
        self.is_https_or_loopback()
    }
}

impl TryFrom<&str> for ProviderOrigin {
    type Error = ProviderOriginError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::from_base_url(value)
    }
}

impl<'de> Deserialize<'de> for ProviderOrigin {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_base_url(&String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
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
    #[serde(default)]
    pub base_url: Option<String>,
    pub secret: SecretString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialBindingState {
    Bound,
    RebindRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCredentialProfile {
    pub provider: ProviderKind,
    pub credential_ref: CredentialProfileRef,
    pub backend: CredentialBackendKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ProviderOrigin>,
    pub binding_state: CredentialBindingState,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionProbeKind {
    Reachability,
    Authentication,
    ModelDiscovery,
    Streaming,
    ToolCompatibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionProbeStatus {
    Passed,
    Failed,
    Unsupported,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionTestOverall {
    Ready,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProbe {
    pub kind: ConnectionProbeKind,
    pub status: ConnectionProbeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    pub message: String,
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
    pub tool_calling: bool,
    pub system_messages: bool,
    pub temperature: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResponse {
    pub provider: ProviderKind,
    pub origin: ProviderOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub overall: ConnectionTestOverall,
    pub models: Vec<String>,
    pub models_truncated: bool,
    pub probes: Vec<ConnectionProbe>,
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
    fn provider_origin_normalizes_scheme_host_default_port_and_discards_path() {
        let origin =
            ProviderOrigin::from_base_url("  HTTPS://Example.COM:443/v1/../responses  ").unwrap();
        assert_eq!(origin.as_str(), "https://example.com");
        assert_eq!(
            ProviderOrigin::from_base_url("https://example.com/other")
                .unwrap()
                .as_str(),
            origin.as_str()
        );
        assert_eq!(
            ProviderOrigin::from_base_url("https://example.com:8443/v1")
                .unwrap()
                .as_str(),
            "https://example.com:8443"
        );
    }

    #[test]
    fn provider_origin_normalizes_ip_literals_and_idna_hosts() {
        assert_eq!(
            ProviderOrigin::from_base_url("http://[0:0:0:0:0:0:0:1]:80/v1")
                .unwrap()
                .as_str(),
            "http://[::1]"
        );
        assert_eq!(
            ProviderOrigin::from_base_url("https://bücher.example/v1")
                .unwrap()
                .as_str(),
            "https://xn--bcher-kva.example"
        );
    }

    #[test]
    fn provider_origin_rejects_ambiguous_or_non_http_destinations() {
        assert_eq!(
            ProviderOrigin::from_base_url("not a url"),
            Err(ProviderOriginError::InvalidUrl)
        );
        assert_eq!(
            ProviderOrigin::from_base_url("file:///tmp/models"),
            Err(ProviderOriginError::UnsupportedScheme)
        );
        assert_eq!(
            ProviderOrigin::from_base_url("https://"),
            Err(ProviderOriginError::MissingHost)
        );
        assert_eq!(
            ProviderOrigin::from_base_url("https://user:pass@example.com/v1"),
            Err(ProviderOriginError::EmbeddedCredentials)
        );
        assert_eq!(
            ProviderOrigin::from_base_url("https://example.com/v1?key=value"),
            Err(ProviderOriginError::QueryOrFragment)
        );
        assert_eq!(
            ProviderOrigin::from_base_url("https://example.com/v1#models"),
            Err(ProviderOriginError::QueryOrFragment)
        );
    }

    #[test]
    fn credential_transport_safety_requires_https_or_a_literal_loopback_origin() {
        assert!(ProviderOrigin::from_base_url("https://example.com/v1")
            .unwrap()
            .is_https_or_loopback());
        assert!(ProviderOrigin::from_base_url("http://localhost:11434/v1")
            .unwrap()
            .is_https_or_loopback());
        assert!(ProviderOrigin::from_base_url("http://127.0.0.42:11434/v1")
            .unwrap()
            .is_https_or_loopback());
        assert!(ProviderOrigin::from_base_url("http://[::1]:11434/v1")
            .unwrap()
            .is_https_or_loopback());
        assert!(!ProviderOrigin::from_base_url("http://example.com/v1")
            .unwrap()
            .is_https_or_loopback());
    }

    #[test]
    fn first_party_provider_origins_are_fixed_and_serializable() {
        assert_eq!(
            ProviderKind::OpenAi
                .fixed_official_origin()
                .unwrap()
                .as_str(),
            "https://api.openai.com"
        );
        assert_eq!(
            ProviderKind::Anthropic.fixed_official_base_url(),
            Some("https://api.anthropic.com/v1")
        );
        assert_eq!(ProviderKind::Local.fixed_official_origin(), None);

        let origin = ProviderOrigin::from_base_url("HTTPS://EXAMPLE.COM:443/v1").unwrap();
        let encoded = serde_json::to_string(&origin).unwrap();
        assert_eq!(encoded, r#""https://example.com""#);
        assert_eq!(
            serde_json::from_str::<ProviderOrigin>(&encoded).unwrap(),
            origin
        );
    }

    #[test]
    fn credential_save_request_accepts_an_optional_base_url() {
        let request: CredentialSaveRequest = serde_json::from_value(serde_json::json!({
            "provider": "local",
            "baseUrl": "http://127.0.0.1:11434/v1",
            "secret": "local-secret"
        }))
        .unwrap();
        assert_eq!(
            request.base_url.as_deref(),
            Some("http://127.0.0.1:11434/v1")
        );

        let request: CredentialSaveRequest = serde_json::from_value(serde_json::json!({
            "provider": "openai",
            "secret": "openai-secret"
        }))
        .unwrap();
        assert_eq!(request.base_url, None);
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
            model: None,
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
