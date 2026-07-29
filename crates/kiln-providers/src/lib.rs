mod anthropic;
mod diagnostics;
mod error;
mod local;
mod openai;
mod tool_turn;

use std::time::Duration;

use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
use kiln_core::{
    ChatRequest, ChatResponse, ChatRole, ChatStreamEvent, CommandError, ConnectionTestRequest,
    ConnectionTestResponse, ProviderCapabilities, ProviderCredentials, ProviderKind,
    ProviderOrigin, RepositoryToolOutcome, TokenUsage,
};
use kiln_platform::CancellationToken;
use reqwest::{
    header::{HeaderName, HeaderValue, CONTENT_TYPE},
    redirect::Policy,
    Client, RequestBuilder, StatusCode, Url,
};
use serde_json::Value;
use tokio::sync::mpsc;

use self::{anthropic::AnthropicAdapter, local::LocalAdapter, openai::OpenAiAdapter};
use crate::error::ProviderError;

pub use tool_turn::{
    encode_tool_outcome, repository_tool_catalog, ProviderToolCall, ProviderToolCallHandle,
    ProviderTurnEvent, ToolTurnCodec, ToolTurnCodecError, MAX_PROVIDER_TOOL_HANDLE_CHARS,
    MAX_TOOL_CALLS_PER_TURN, MAX_TOOL_NAME_CHARS, MAX_TOOL_TURN_BYTES,
};

static OPENAI: OpenAiAdapter = OpenAiAdapter;
static ANTHROPIC: AnthropicAdapter = AnthropicAdapter;
static LOCAL: LocalAdapter = LocalAdapter;

/// Tauri-free entry point for provider discovery, diagnostics, and chat.
///
/// The service owns transport concerns and returns only normalized application
/// contracts from `kiln-core`.
#[derive(Clone)]
pub struct ProviderService {
    http: Client,
}

pub type ChatStreamReceiver = mpsc::Receiver<Result<ChatStreamEvent, CommandError>>;

impl ProviderService {
    pub fn new() -> Self {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            // Never let a secret-bearing request follow a redirect to a
            // destination that was not explicitly approved.
            .redirect(Policy::none())
            .user_agent(concat!("Kiln/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("the platform TLS and HTTP client should initialize");

        Self { http }
    }

    pub fn capabilities(&self) -> Vec<ProviderCapabilities> {
        all_capabilities()
    }

    pub async fn test_connection(
        &self,
        request: &ConnectionTestRequest,
    ) -> Result<ConnectionTestResponse, CommandError> {
        let provider = request.provider;
        let redactor = request.credentials.redactor();
        test_connection(&self.http, request)
            .await
            .map_err(|error| error.into_command(provider, &redactor))
    }

    /// Resolves the exact origin that may receive a stored credential.
    ///
    /// Provider HTTP requests reuse the same parser, so a frontend cannot
    /// normalize one destination for the vault and send to another.
    pub fn credential_origin(
        &self,
        provider: ProviderKind,
        base_url: Option<&str>,
    ) -> Result<ProviderOrigin, CommandError> {
        let redactor = kiln_core::SensitiveDataRedactor::default();
        let destination = resolve_destination(base_url, &adapter(provider).capabilities())
            .map_err(|error| error.into_command(provider, &redactor))?;
        if !destination.origin.is_safe_for_credentials() {
            return Err(ProviderError::InvalidConfiguration(
                "Credentials require HTTPS unless the compatible endpoint is on this device."
                    .to_owned(),
            )
            .into_command(provider, &redactor));
        }
        Ok(destination.origin)
    }

    pub async fn send_chat(&self, request: &ChatRequest) -> Result<ChatResponse, CommandError> {
        let provider = request.provider;
        let redactor = request.credentials.redactor();
        send_chat(&self.http, request)
            .await
            .map_err(|error| error.into_command(provider, &redactor))
    }

    pub fn tool_turn_codec(&self, provider: ProviderKind) -> ToolTurnCodec {
        ToolTurnCodec::new(adapter(provider).capabilities().protocol)
    }

    pub fn repository_tool_catalog(&self, provider: ProviderKind) -> Value {
        tool_turn::repository_tool_catalog(adapter(provider).capabilities().protocol)
    }

    pub fn encode_tool_outcome(
        &self,
        provider: ProviderKind,
        call: &ProviderToolCall,
        outcome: &RepositoryToolOutcome,
    ) -> Result<Value, ToolTurnCodecError> {
        tool_turn::encode_tool_outcome(adapter(provider).capabilities().protocol, call, outcome)
    }

    /// Starts a provider stream and returns immediately after validating and
    /// constructing the request. Network I/O runs inside the cancellation
    /// domain and emits only normalized stream events.
    pub fn stream_chat(
        &self,
        request: ChatRequest,
        cancellation: CancellationToken,
    ) -> Result<ChatStreamReceiver, CommandError> {
        let provider = request.provider;
        let redactor = request.credentials.redactor();
        let adapter = adapter(provider);
        let builder = adapter
            .stream_request(&self.http, &request)
            .map_err(|error| error.into_command(provider, &redactor))?;
        let requested_model = request.model;
        let (sender, receiver) = mpsc::channel(32);

        tokio::spawn(async move {
            run_provider_stream(
                adapter,
                builder,
                requested_model,
                cancellation,
                sender,
                redactor,
            )
            .await;
        });

        Ok(receiver)
    }
}

impl Default for ProviderService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
trait ProviderAdapter: Sync {
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

    fn parse_stream_data(
        &self,
        data: &str,
        state: &mut StreamState,
    ) -> Result<Vec<ChatStreamEvent>, ProviderError>;

    fn stream_payload(&self, request: &ChatRequest) -> Result<Value, ProviderError> {
        let mut payload = self.chat_payload(request)?;
        let object = payload.as_object_mut().ok_or_else(|| {
            ProviderError::InvalidRequest(
                "The provider streaming request must encode as an object.".to_owned(),
            )
        })?;
        object.insert("stream".to_owned(), Value::Bool(true));
        Ok(payload)
    }

    fn stream_request(
        &self,
        client: &Client,
        request: &ChatRequest,
    ) -> Result<RequestBuilder, ProviderError> {
        validate_chat_request(request)?;
        let destination = resolve_destination(request.base_url.as_deref(), &self.capabilities())?;
        validate_credential_destination(&request.credentials, &destination.origin)?;
        let endpoint = endpoint(&destination.base_url, self.chat_path());
        let builder = client
            .post(endpoint)
            .header(CONTENT_TYPE, "application/json")
            .json(&self.stream_payload(request)?);
        let builder = apply_custom_headers(
            builder,
            &request.credentials,
            self.capabilities().custom_headers,
        )?;
        self.apply_provider_headers(builder, &request.credentials)
    }

    async fn send_chat(
        &self,
        client: &Client,
        request: &ChatRequest,
    ) -> Result<ChatResponse, ProviderError> {
        validate_chat_request(request)?;
        let destination = resolve_destination(request.base_url.as_deref(), &self.capabilities())?;
        validate_credential_destination(&request.credentials, &destination.origin)?;
        let endpoint = endpoint(&destination.base_url, self.chat_path());
        let payload = self.chat_payload(request)?;
        let builder = client
            .post(endpoint)
            .header(CONTENT_TYPE, "application/json")
            .json(&payload);
        let builder = apply_custom_headers(
            builder,
            &request.credentials,
            self.capabilities().custom_headers,
        )?;
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
}

#[derive(Debug)]
struct StreamState {
    provider: ProviderKind,
    requested_model: String,
    id: Option<String>,
    model: Option<String>,
    content: String,
    finish_reason: Option<String>,
    usage: TokenUsage,
    completed: bool,
}

impl StreamState {
    fn new(provider: ProviderKind, requested_model: String) -> Self {
        Self {
            provider,
            requested_model,
            id: None,
            model: None,
            content: String::new(),
            finish_reason: None,
            usage: TokenUsage::default(),
            completed: false,
        }
    }

    fn delta(&mut self, delta: String) -> Option<ChatStreamEvent> {
        if self.completed || delta.is_empty() {
            return None;
        }
        self.content.push_str(&delta);
        Some(ChatStreamEvent::MessageDelta { delta })
    }

    fn complete(&mut self) -> Result<Option<ChatStreamEvent>, ProviderError> {
        if self.completed {
            return Ok(None);
        }
        if self.content.is_empty() {
            return Err(ProviderError::MalformedResponse(
                "The provider stream completed without returning text.".to_owned(),
            ));
        }
        self.completed = true;
        Ok(Some(ChatStreamEvent::MessageCompleted {
            response: ChatResponse {
                provider: self.provider,
                id: self.id.clone(),
                model: self
                    .model
                    .clone()
                    .unwrap_or_else(|| self.requested_model.clone()),
                content: self.content.clone(),
                finish_reason: self.finish_reason.clone(),
                usage: self.usage.clone(),
            },
        }))
    }

    fn complete_response(&mut self, response: ChatResponse) -> Option<ChatStreamEvent> {
        if self.completed {
            return None;
        }
        self.completed = true;
        self.id.clone_from(&response.id);
        self.model = Some(response.model.clone());
        self.content.clone_from(&response.content);
        self.finish_reason.clone_from(&response.finish_reason);
        self.usage.clone_from(&response.usage);
        Some(ChatStreamEvent::MessageCompleted { response })
    }
}

async fn run_provider_stream(
    adapter: &'static dyn ProviderAdapter,
    builder: RequestBuilder,
    requested_model: String,
    cancellation: CancellationToken,
    sender: mpsc::Sender<Result<ChatStreamEvent, CommandError>>,
    redactor: kiln_core::SensitiveDataRedactor,
) {
    let response = match cancellation.run(builder.send()).await {
        Err(_) => {
            send_cancelled(&sender).await;
            return;
        }
        Ok(Err(error)) => {
            send_provider_error(
                &sender,
                network_error(error, adapter.kind()),
                adapter.kind(),
                &redactor,
            )
            .await;
            return;
        }
        Ok(Ok(response)) => response,
    };

    let status = response.status();
    if !status.is_success() {
        let body = match cancellation.run(response.text()).await {
            Err(_) => {
                send_cancelled(&sender).await;
                return;
            }
            Ok(Err(error)) => {
                send_provider_error(
                    &sender,
                    network_error(error, adapter.kind()),
                    adapter.kind(),
                    &redactor,
                )
                .await;
                return;
            }
            Ok(Ok(body)) => body,
        };
        if let Err(error) = ensure_success(status, &body, adapter.kind()) {
            send_provider_error(&sender, error, adapter.kind(), &redactor).await;
        }
        return;
    }

    let provider = adapter.kind();
    let chunks = response.bytes_stream().map(move |chunk| {
        chunk
            .map(|bytes| bytes.to_vec())
            .map_err(|error| network_error(error, provider))
    });
    pump_stream(
        adapter,
        chunks,
        requested_model,
        cancellation,
        sender,
        &redactor,
    )
    .await;
}

async fn pump_stream<S>(
    adapter: &'static dyn ProviderAdapter,
    mut chunks: S,
    requested_model: String,
    cancellation: CancellationToken,
    sender: mpsc::Sender<Result<ChatStreamEvent, CommandError>>,
    redactor: &kiln_core::SensitiveDataRedactor,
) where
    S: Stream<Item = Result<Vec<u8>, ProviderError>> + Unpin,
{
    let mut framer = SseFramer::default();
    let mut state = StreamState::new(adapter.kind(), requested_model);

    loop {
        let chunk = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                send_cancelled(&sender).await;
                return;
            }
            chunk = chunks.next() => chunk,
        };

        let stream_ended = chunk.is_none();
        let data_events = match chunk {
            Some(Ok(chunk)) => match framer.push(&chunk) {
                Ok(events) => events,
                Err(error) => {
                    send_provider_error(&sender, error, adapter.kind(), redactor).await;
                    return;
                }
            },
            Some(Err(error)) => {
                send_provider_error(&sender, error, adapter.kind(), redactor).await;
                return;
            }
            None => match framer.finish() {
                Ok(events) => events,
                Err(error) => {
                    send_provider_error(&sender, error, adapter.kind(), redactor).await;
                    return;
                }
            },
        };

        for data in data_events {
            let events = match adapter.parse_stream_data(&data, &mut state) {
                Ok(events) => events,
                Err(error) => {
                    send_provider_error(&sender, error, adapter.kind(), redactor).await;
                    return;
                }
            };
            for event in events {
                if cancellation.is_cancelled() {
                    send_cancelled(&sender).await;
                    return;
                }
                if sender.send(Ok(event)).await.is_err() {
                    return;
                }
            }
        }

        if stream_ended {
            if !state.completed {
                send_provider_error(
                    &sender,
                    ProviderError::MalformedResponse(
                        "The provider stream ended without a completion event.".to_owned(),
                    ),
                    adapter.kind(),
                    redactor,
                )
                .await;
            }
            return;
        }
    }
}

async fn send_cancelled(sender: &mpsc::Sender<Result<ChatStreamEvent, CommandError>>) {
    let _ = sender
        .send(Ok(ChatStreamEvent::Cancelled {
            reason: "The turn was cancelled.".to_owned(),
        }))
        .await;
}

async fn send_provider_error(
    sender: &mpsc::Sender<Result<ChatStreamEvent, CommandError>>,
    error: ProviderError,
    provider: ProviderKind,
    redactor: &kiln_core::SensitiveDataRedactor,
) {
    let _ = sender
        .send(Err(error.into_command(provider, redactor)))
        .await;
}

#[derive(Debug, Default)]
struct SseFramer {
    pending: Vec<u8>,
    data_lines: Vec<String>,
}

impl SseFramer {
    fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, ProviderError> {
        self.pending.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(index) = self.pending.iter().position(|byte| *byte == b'\n') {
            let mut line = self.pending.drain(..=index).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            self.push_line(&line, &mut events)?;
        }
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<String>, ProviderError> {
        let mut events = Vec::new();
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            self.push_line(&line, &mut events)?;
        }
        self.flush(&mut events);
        Ok(events)
    }

    fn push_line(&mut self, line: &[u8], events: &mut Vec<String>) -> Result<(), ProviderError> {
        let line = std::str::from_utf8(line).map_err(|_| {
            ProviderError::MalformedResponse(
                "The provider stream contained invalid UTF-8.".to_owned(),
            )
        })?;
        if line.is_empty() {
            self.flush(events);
        } else if let Some(data) = line.strip_prefix("data:") {
            self.data_lines
                .push(data.strip_prefix(' ').unwrap_or(data).to_owned());
        }
        Ok(())
    }

    fn flush(&mut self, events: &mut Vec<String>) {
        if !self.data_lines.is_empty() {
            events.push(self.data_lines.join("\n"));
            self.data_lines.clear();
        }
    }
}

fn all_capabilities() -> Vec<ProviderCapabilities> {
    vec![
        OPENAI.capabilities(),
        ANTHROPIC.capabilities(),
        LOCAL.capabilities(),
    ]
}

async fn send_chat(client: &Client, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
    adapter(request.provider).send_chat(client, request).await
}

async fn test_connection(
    client: &Client,
    request: &ConnectionTestRequest,
) -> Result<ConnectionTestResponse, ProviderError> {
    diagnostics::test_connection(client, adapter(request.provider), request).await
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
        .any(|message| matches!(message.role, ChatRole::User))
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

#[derive(Debug, Clone)]
struct ResolvedProviderDestination {
    base_url: String,
    origin: ProviderOrigin,
}

fn resolve_destination(
    provided: Option<&str>,
    capabilities: &ProviderCapabilities,
) -> Result<ResolvedProviderDestination, ProviderError> {
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

    let base_url = parsed.as_str().trim_end_matches('/').to_owned();
    if !capabilities.custom_base_url {
        let expected = Url::parse(capabilities.default_base_url)
            .expect("built-in provider base URLs must be valid");
        if base_url != expected.as_str().trim_end_matches('/') {
            return Err(ProviderError::InvalidConfiguration(format!(
                "{} connections are pinned to the official API origin.",
                capabilities.display_name
            )));
        }
    }

    let origin = ProviderOrigin::from_base_url(&base_url).map_err(|_| {
        ProviderError::InvalidConfiguration(
            "The provider base URL does not have a valid HTTP origin.".to_owned(),
        )
    })?;
    Ok(ResolvedProviderDestination { base_url, origin })
}

fn endpoint(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn validate_credential_destination(
    credentials: &ProviderCredentials,
    origin: &ProviderOrigin,
) -> Result<(), ProviderError> {
    let carries_secret = credentials.api_key.is_some() || !credentials.custom_headers.is_empty();
    if carries_secret && !origin.is_safe_for_credentials() {
        return Err(ProviderError::InvalidConfiguration(
            "Credentials require HTTPS unless the compatible endpoint is on this device."
                .to_owned(),
        ));
    }
    Ok(())
}

fn require_api_key<'a>(
    credentials: &'a ProviderCredentials,
    provider_name: &str,
) -> Result<&'a str, ProviderError> {
    match credentials.api_key.as_ref() {
        Some(key) if !key.is_blank() => Ok(key.expose_secret()),
        _ => Err(ProviderError::InvalidConfiguration(format!(
            "Enter an API key for {provider_name}."
        ))),
    }
}

fn bearer_header(api_key: &str) -> Result<HeaderValue, ProviderError> {
    sensitive_header(&format!("Bearer {api_key}"))
}

fn sensitive_header(value: &str) -> Result<HeaderValue, ProviderError> {
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
    allowed: bool,
) -> Result<RequestBuilder, ProviderError> {
    if !allowed && !credentials.custom_headers.is_empty() {
        return Err(ProviderError::InvalidConfiguration(
            "Custom headers are available only for compatible custom endpoints.".to_owned(),
        ));
    }
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
        let value = sensitive_header(value.expose_secret())?;
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

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        pin::Pin,
        task::{Context, Poll},
        thread,
        time::Duration,
    };

    use super::*;

    struct ControlledChunks {
        receiver: mpsc::UnboundedReceiver<Result<Vec<u8>, ProviderError>>,
    }

    impl Stream for ControlledChunks {
        type Item = Result<Vec<u8>, ProviderError>;

        fn poll_next(
            mut self: Pin<&mut Self>,
            context: &mut Context<'_>,
        ) -> Poll<Option<Self::Item>> {
            self.receiver.poll_recv(context)
        }
    }

    #[test]
    fn base_url_rejects_embedded_credentials() {
        let error = resolve_destination(
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
    fn cloud_destinations_reject_non_official_base_urls() {
        for capabilities in [OPENAI.capabilities(), ANTHROPIC.capabilities()] {
            let error = resolve_destination(Some("https://gateway.example.test/v1"), &capabilities)
                .expect_err("cloud providers must remain pinned");
            assert!(matches!(error, ProviderError::InvalidConfiguration(_)));

            let official =
                resolve_destination(Some(capabilities.default_base_url), &capabilities).unwrap();
            assert_eq!(
                official.origin.as_str(),
                ProviderKind::fixed_official_origin(capabilities.provider)
                    .unwrap()
                    .as_str()
            );
        }
    }

    #[tokio::test]
    async fn credentialed_diagnostics_never_follow_redirects() {
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let target_url = format!("http://{}/captured", target.local_addr().unwrap());

        let redirect = TcpListener::bind("127.0.0.1:0").unwrap();
        let redirect_address = redirect.local_addr().unwrap();
        let responder = thread::spawn(move || {
            let (mut stream, _) = redirect.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request);
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        });

        let service = ProviderService::new();
        let report = service
            .test_connection(&ConnectionTestRequest {
                provider: ProviderKind::Local,
                credentials: ProviderCredentials {
                    api_key: Some(kiln_core::SecretString::new("redirect-secret")),
                    ..ProviderCredentials::default()
                },
                credential_ref: None,
                base_url: Some(format!("http://{redirect_address}/v1")),
                model: None,
            })
            .await
            .unwrap();
        responder.join().unwrap();

        assert_eq!(
            report.probes[0].http_status,
            Some(StatusCode::FOUND.as_u16())
        );
        thread::sleep(Duration::from_millis(25));
        assert!(
            target.accept().is_err(),
            "the redirected destination must receive no request"
        );
    }

    #[test]
    fn extracts_structured_error_without_returning_raw_body() {
        let body = r#"{"error":{"message":"  Invalid   model  "},"secret":"do-not-return"}"#;
        let message = extract_error_message(body).expect("message should parse");
        assert_eq!(message, "Invalid model");
        assert!(!message.contains("do-not-return"));
    }

    #[test]
    fn sse_framer_preserves_split_unicode_and_crlf_boundaries() {
        let source = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"🔥\"}\r\n\r\n";
        let bytes = source.as_bytes();
        let flame = source.find('🔥').unwrap();
        let mut framer = SseFramer::default();

        assert!(framer.push(&bytes[..flame + 1]).unwrap().is_empty());
        let events = framer.push(&bytes[flame + 1..]).unwrap();

        assert_eq!(events.len(), 1);
        assert!(events[0].contains('🔥'));
    }

    #[tokio::test]
    async fn cancellation_wins_over_late_provider_chunks() {
        let cancellation = CancellationToken::default();
        let (chunk_sender, chunk_receiver) = mpsc::unbounded_channel();
        let (event_sender, mut event_receiver) = mpsc::channel(8);
        let chunks = ControlledChunks {
            receiver: chunk_receiver,
        };
        let worker_cancellation = cancellation.clone();
        let redactor = kiln_core::SensitiveDataRedactor::default();

        let worker = tokio::spawn(async move {
            pump_stream(
                &LOCAL,
                chunks,
                "qwen".to_owned(),
                worker_cancellation,
                event_sender,
                &redactor,
            )
            .await;
        });

        chunk_sender
            .send(Ok(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"first\"},\"finish_reason\":null}]}\n\n"
                    .to_vec(),
            ))
            .unwrap();
        assert!(matches!(
            event_receiver.recv().await,
            Some(Ok(ChatStreamEvent::MessageDelta { delta })) if delta == "first"
        ));

        cancellation.cancel();
        chunk_sender
            .send(Ok(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"late\"},\"finish_reason\":\"stop\"}]}\n\n"
                    .to_vec(),
            ))
            .unwrap();

        assert!(matches!(
            event_receiver.recv().await,
            Some(Ok(ChatStreamEvent::Cancelled { .. }))
        ));
        assert!(event_receiver.recv().await.is_none());
        worker.await.unwrap();
    }
}
