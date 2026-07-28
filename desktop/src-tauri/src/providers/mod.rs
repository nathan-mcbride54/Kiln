mod anthropic;
mod local;
mod openai;

use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::{
    header::{HeaderName, HeaderValue, CONTENT_TYPE},
    Client, RequestBuilder, StatusCode, Url,
};
use serde_json::Value;

use crate::{
    error::ProviderError,
    types::{
        ChatRequest, ChatResponse, ConnectionTestRequest, ConnectionTestResponse,
        ProviderCapabilities, ProviderCredentials, ProviderKind,
    },
};

use self::{anthropic::AnthropicAdapter, local::LocalAdapter, openai::OpenAiAdapter};

static OPENAI: OpenAiAdapter = OpenAiAdapter;
static ANTHROPIC: AnthropicAdapter = AnthropicAdapter;
static LOCAL: LocalAdapter = LocalAdapter;

#[async_trait]
pub(crate) trait ProviderAdapter: Sync {
    fn kind(&self) -> ProviderKind;
    fn capabilities(&self) -> ProviderCapabilities;
    fn chat_path(&self) -> &'static str;
    fn models_path(&self) -> &'static str {
        "models"
    }

    fn apply_provider_headers(
        &self,
        request: RequestBuilder,
        credentials: &ProviderCredentials,
    ) -> Result<RequestBuilder, ProviderError>;

    fn chat_payload(&self, request: &ChatRequest) -> Result<Value, ProviderError>;

    fn parse_chat_response(
        &self,
        body: &str,
        requested_model: &str,
    ) -> Result<ChatResponse, ProviderError>;

    async fn send_chat(
        &self,
        client: &Client,
        request: &ChatRequest,
    ) -> Result<ChatResponse, ProviderError> {
        validate_chat_request(request)?;
        let base_url = resolve_base_url(request.base_url.as_deref(), &self.capabilities())?;
        let endpoint = endpoint(&base_url, self.chat_path());
        let payload = self.chat_payload(request)?;
        let builder = client
            .post(endpoint)
            .header(CONTENT_TYPE, "application/json")
            .json(&payload);
        let builder = apply_custom_headers(builder, &request.credentials)?;
        let builder = self.apply_provider_headers(builder, &request.credentials)?;

        let response = builder
            .send()
            .await
            .map_err(|error| network_error(error, self.kind()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| network_error(error, self.kind()))?;

        ensure_success(status, &body, self.kind())?;
        self.parse_chat_response(&body, &request.model)
    }

    async fn test_connection(
        &self,
        client: &Client,
        request: &ConnectionTestRequest,
    ) -> Result<ConnectionTestResponse, ProviderError> {
        let base_url = resolve_base_url(request.base_url.as_deref(), &self.capabilities())?;
        let endpoint = endpoint(&base_url, self.models_path());
        let builder = client
            .get(endpoint)
            .timeout(Duration::from_secs(15))
            .header(CONTENT_TYPE, "application/json");
        let builder = apply_custom_headers(builder, &request.credentials)?;
        let builder = self.apply_provider_headers(builder, &request.credentials)?;

        let started = Instant::now();
        let response = builder
            .send()
            .await
            .map_err(|error| network_error(error, self.kind()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| network_error(error, self.kind()))?;
        ensure_success(status, &body, self.kind())?;

        let discovered_models = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|value| model_count(&value));

        Ok(ConnectionTestResponse {
            provider: self.kind(),
            connected: true,
            latency_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            discovered_models,
            message: match discovered_models {
                Some(1) => "Connected and discovered 1 model.".to_owned(),
                Some(count) => format!("Connected and discovered {count} models."),
                None => "Connected. This server did not return a standard model list.".to_owned(),
            },
        })
    }
}

pub(crate) fn all_capabilities() -> Vec<ProviderCapabilities> {
    vec![
        OPENAI.capabilities(),
        ANTHROPIC.capabilities(),
        LOCAL.capabilities(),
    ]
}

pub(crate) async fn send_chat(
    client: &Client,
    request: &ChatRequest,
) -> Result<ChatResponse, ProviderError> {
    adapter(request.provider).send_chat(client, request).await
}

pub(crate) async fn test_connection(
    client: &Client,
    request: &ConnectionTestRequest,
) -> Result<ConnectionTestResponse, ProviderError> {
    adapter(request.provider)
        .test_connection(client, request)
        .await
}

fn adapter(kind: ProviderKind) -> &'static dyn ProviderAdapter {
    match kind {
        ProviderKind::OpenAi => &OPENAI,
        ProviderKind::Anthropic => &ANTHROPIC,
        ProviderKind::Local => &LOCAL,
    }
}

fn validate_chat_request(request: &ChatRequest) -> Result<(), ProviderError> {
    if request.model.trim().is_empty() {
        return Err(ProviderError::InvalidRequest(
            "Choose or enter a model before sending a message.".to_owned(),
        ));
    }
    if request.messages.is_empty() {
        return Err(ProviderError::InvalidRequest(
            "At least one message is required.".to_owned(),
        ));
    }
    if request
        .messages
        .iter()
        .any(|message| message.content.trim().is_empty())
    {
        return Err(ProviderError::InvalidRequest(
            "Messages cannot be empty.".to_owned(),
        ));
    }
    if !request
        .messages
        .iter()
        .any(|message| matches!(message.role, crate::types::ChatRole::User))
    {
        return Err(ProviderError::InvalidRequest(
            "At least one user message is required.".to_owned(),
        ));
    }
    if request.max_output_tokens == Some(0) {
        return Err(ProviderError::InvalidRequest(
            "maxOutputTokens must be greater than zero.".to_owned(),
        ));
    }
    if request
        .temperature
        .is_some_and(|temperature| !(0.0..=2.0).contains(&temperature))
    {
        return Err(ProviderError::InvalidRequest(
            "temperature must be between 0 and 2.".to_owned(),
        ));
    }
    Ok(())
}

fn resolve_base_url(
    provided: Option<&str>,
    capabilities: &ProviderCapabilities,
) -> Result<String, ProviderError> {
    let candidate = provided
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(capabilities.default_base_url)
        .trim();
    let parsed = Url::parse(candidate).map_err(|_| {
        ProviderError::InvalidConfiguration(
            "The provider base URL is not a valid absolute URL.".to_owned(),
        )
    })?;

    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ProviderError::InvalidConfiguration(
            "The provider base URL must use HTTP or HTTPS.".to_owned(),
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(ProviderError::InvalidConfiguration(
            "Put credentials in the credential fields, not in the provider URL.".to_owned(),
        ));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(ProviderError::InvalidConfiguration(
            "The provider base URL cannot contain a query string or fragment.".to_owned(),
        ));
    }

    Ok(parsed.as_str().trim_end_matches('/').to_owned())
}

fn endpoint(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

pub(super) fn require_api_key<'a>(
    credentials: &'a ProviderCredentials,
    provider_name: &str,
) -> Result<&'a str, ProviderError> {
    match credentials.api_key.as_ref() {
        Some(key) if !key.is_blank() => Ok(key.expose()),
        _ => Err(ProviderError::InvalidConfiguration(format!(
            "Enter an API key for {provider_name}."
        ))),
    }
}

pub(super) fn bearer_header(api_key: &str) -> Result<HeaderValue, ProviderError> {
    sensitive_header(&format!("Bearer {api_key}"))
}

pub(super) fn sensitive_header(value: &str) -> Result<HeaderValue, ProviderError> {
    let mut header = HeaderValue::from_str(value).map_err(|_| {
        ProviderError::InvalidConfiguration(
            "A credential contains characters that cannot be sent in an HTTP header.".to_owned(),
        )
    })?;
    header.set_sensitive(true);
    Ok(header)
}

fn apply_custom_headers(
    mut builder: RequestBuilder,
    credentials: &ProviderCredentials,
) -> Result<RequestBuilder, ProviderError> {
    for (name, value) in &credentials.custom_headers {
        let normalized = name.to_ascii_lowercase();
        if matches!(
            normalized.as_str(),
            "authorization"
                | "content-length"
                | "content-type"
                | "host"
                | "openai-organization"
                | "openai-project"
                | "x-api-key"
                | "anthropic-version"
        ) {
            return Err(ProviderError::InvalidConfiguration(format!(
                "The {name} header is managed by Kiln and cannot be overridden."
            )));
        }
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            ProviderError::InvalidConfiguration(format!("{name} is not a valid HTTP header name."))
        })?;
        let value = sensitive_header(value.expose())?;
        builder = builder.header(name, value);
    }
    Ok(builder)
}

fn network_error(error: reqwest::Error, provider: ProviderKind) -> ProviderError {
    let action = if error.is_timeout() {
        "timed out"
    } else if error.is_connect() {
        "could not connect"
    } else {
        "failed"
    };
    ProviderError::Network(format!(
        "The {} request {action}. Check the endpoint and your network connection.",
        provider_label(provider)
    ))
}

fn ensure_success(
    status: StatusCode,
    body: &str,
    provider: ProviderKind,
) -> Result<(), ProviderError> {
    if status.is_success() {
        return Ok(());
    }

    let fallback = match status.as_u16() {
        401 | 403 => "The provider rejected the supplied credentials.",
        404 => "The provider endpoint was not found. Check the base URL.",
        408 => "The provider timed out while handling the request.",
        429 => "The provider rate limit has been reached.",
        500..=599 => "The provider is temporarily unavailable.",
        _ => "The provider rejected the request.",
    };
    let message = extract_error_message(body).unwrap_or_else(|| fallback.to_owned());
    let message = format!("{}: {message}", provider_label(provider));

    match status.as_u16() {
        401 | 403 => Err(ProviderError::Authentication(message)),
        429 => Err(ProviderError::RateLimited(message)),
        value => Err(ProviderError::Upstream {
            status: value,
            message,
        }),
    }
}

fn extract_error_message(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let message = value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| value.get("message").and_then(Value::as_str))
        .or_else(|| {
            value
                .pointer("/error/error/message")
                .and_then(Value::as_str)
        })?;
    let compact = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return None;
    }
    Some(compact.chars().take(500).collect())
}

fn provider_label(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::OpenAi => "OpenAI",
        ProviderKind::Anthropic => "Anthropic",
        ProviderKind::Local => "Local server",
    }
}

fn model_count(value: &Value) -> Option<usize> {
    value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array))
        .or_else(|| value.as_array())
        .map(Vec::len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_rejects_embedded_credentials() {
        let error = resolve_base_url(
            Some("http://secret:password@localhost:1234/v1"),
            &LOCAL.capabilities(),
        )
        .expect_err("credentials in URLs should be rejected");
        assert!(matches!(error, ProviderError::InvalidConfiguration(_)));
    }

    #[test]
    fn endpoint_preserves_a_versioned_base_path() {
        assert_eq!(
            endpoint("http://127.0.0.1:1234/v1/", "/chat/completions"),
            "http://127.0.0.1:1234/v1/chat/completions"
        );
    }

    #[test]
    fn counts_common_model_list_shapes() {
        assert_eq!(model_count(&serde_json::json!({"data": [{}, {}]})), Some(2));
        assert_eq!(model_count(&serde_json::json!({"models": [{}]})), Some(1));
        assert_eq!(model_count(&serde_json::json!([{}, {}, {}])), Some(3));
    }

    #[test]
    fn extracts_structured_error_without_returning_raw_body() {
        let body = r#"{"error":{"message":"  Invalid   model  "},"secret":"do-not-return"}"#;
        let message = extract_error_message(body).expect("message should parse");
        assert_eq!(message, "Invalid model");
        assert!(!message.contains("do-not-return"));
    }
}
