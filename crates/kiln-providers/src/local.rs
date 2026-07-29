use kiln_core::{
    ChatRequest, ChatResponse, ChatRole, ProviderCapabilities, ProviderCredentials, ProviderKind,
    ProviderProtocol, TokenUsage,
};
use reqwest::{header::AUTHORIZATION, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{to_value, Value};

use super::{bearer_header, ProviderAdapter, StreamState};
use crate::error::ProviderError;

pub(crate) struct LocalAdapter;

impl ProviderAdapter for LocalAdapter {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Local
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider: self.kind(),
            display_name: "Local AI server",
            protocol: ProviderProtocol::OpenAiChatCompletions,
            default_base_url: "http://127.0.0.1:1234/v1",
            api_key_required: false,
            custom_base_url: true,
            custom_headers: true,
            model_discovery: true,
            streaming: true,
            system_messages: true,
            temperature: true,
        }
    }

    fn chat_path(&self) -> &'static str {
        "chat/completions"
    }

    fn apply_provider_headers(
        &self,
        request: RequestBuilder,
        credentials: &ProviderCredentials,
    ) -> Result<RequestBuilder, ProviderError> {
        match credentials.api_key.as_ref() {
            Some(key) if !key.is_blank() => {
                Ok(request.header(AUTHORIZATION, bearer_header(key.expose_secret())?))
            }
            _ => Ok(request),
        }
    }

    fn chat_payload(&self, request: &ChatRequest) -> Result<Value, ProviderError> {
        let messages = request
            .messages
            .iter()
            .map(|message| LocalMessage {
                role: match message.role {
                    ChatRole::System | ChatRole::Developer => "system",
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                },
                content: &message.content,
            })
            .collect();
        let payload = LocalRequestPayload {
            model: request.model.trim(),
            messages,
            max_tokens: request.max_output_tokens,
            temperature: request.temperature,
            stream: false,
        };
        to_value(payload).map_err(|error| {
            ProviderError::InvalidRequest(format!(
                "The local server request could not be encoded: {error}"
            ))
        })
    }

    fn parse_chat_response(
        &self,
        body: &str,
        requested_model: &str,
    ) -> Result<ChatResponse, ProviderError> {
        parse_response(body, requested_model)
    }

    fn parse_stream_data(
        &self,
        data: &str,
        state: &mut StreamState,
    ) -> Result<Vec<kiln_core::ChatStreamEvent>, ProviderError> {
        if data == "[DONE]" {
            return Ok(state.complete()?.into_iter().collect());
        }
        let value: Value = serde_json::from_str(data).map_err(|error| {
            ProviderError::MalformedResponse(format!(
                "The local server returned a stream event Kiln could not parse: {error}"
            ))
        })?;
        state.id = state
            .id
            .take()
            .or_else(|| value.get("id").and_then(Value::as_str).map(str::to_owned));
        state.model = state.model.take().or_else(|| {
            value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
        if let Some(usage) = value.get("usage") {
            state.usage.input_tokens = usage.get("prompt_tokens").and_then(Value::as_u64);
            state.usage.output_tokens = usage.get("completion_tokens").and_then(Value::as_u64);
            state.usage.total_tokens = usage.get("total_tokens").and_then(Value::as_u64);
        }

        let choice = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first());
        let delta = choice
            .and_then(|choice| choice.pointer("/delta/content"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        state.finish_reason = choice
            .and_then(|choice| choice.get("finish_reason"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| state.finish_reason.clone());

        let mut events = state.delta(delta).into_iter().collect::<Vec<_>>();
        if state.finish_reason.is_some() {
            events.extend(state.complete()?);
        }
        Ok(events)
    }
}

#[derive(Serialize)]
struct LocalRequestPayload<'a> {
    model: &'a str,
    messages: Vec<LocalMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
}

#[derive(Serialize)]
struct LocalMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Deserialize)]
struct LocalResponsePayload {
    id: Option<String>,
    model: Option<String>,
    #[serde(default)]
    choices: Vec<LocalChoice>,
    usage: Option<LocalUsage>,
}

#[derive(Deserialize)]
struct LocalChoice {
    finish_reason: Option<String>,
    message: Option<LocalResponseMessage>,
    text: Option<String>,
}

#[derive(Deserialize)]
struct LocalResponseMessage {
    content: Option<CompatibleContent>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CompatibleContent {
    Text(String),
    Blocks(Vec<CompatibleContentBlock>),
}

impl CompatibleContent {
    fn into_text(self) -> String {
        match self {
            Self::Text(text) => text,
            Self::Blocks(blocks) => blocks
                .into_iter()
                .filter(|block| block.kind.as_deref().map_or(true, |kind| kind == "text"))
                .filter_map(|block| block.text)
                .collect(),
        }
    }
}

#[derive(Deserialize)]
struct CompatibleContentBlock {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

#[derive(Deserialize)]
struct LocalUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

fn parse_response(body: &str, requested_model: &str) -> Result<ChatResponse, ProviderError> {
    let parsed: LocalResponsePayload = serde_json::from_str(body).map_err(|error| {
        ProviderError::MalformedResponse(format!(
            "The local server returned a response Kiln could not parse: {error}"
        ))
    })?;
    let mut choices = parsed.choices.into_iter();
    let choice = choices.next().ok_or_else(|| {
        ProviderError::MalformedResponse(
            "The local server response did not contain a completion choice.".to_owned(),
        )
    })?;
    let content = choice
        .message
        .and_then(|message| message.content)
        .map(CompatibleContent::into_text)
        .or(choice.text)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| {
            ProviderError::MalformedResponse(
                "The local server completion did not contain text.".to_owned(),
            )
        })?;
    let usage = parsed
        .usage
        .map(|usage| TokenUsage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
        })
        .unwrap_or_default();

    Ok(ChatResponse {
        provider: ProviderKind::Local,
        id: parsed.id,
        model: parsed.model.unwrap_or_else(|| requested_model.to_owned()),
        content,
        finish_reason: choice.finish_reason,
        usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_compatible_chat_completion() {
        let fixture = r#"{
          "id": "chatcmpl-local",
          "model": "qwen3-coder",
          "choices": [{
            "finish_reason": "stop",
            "message": {"role": "assistant", "content": "Hello locally"}
          }],
          "usage": {"prompt_tokens": 7, "completion_tokens": 2, "total_tokens": 9}
        }"#;

        let response = parse_response(fixture, "fallback").expect("fixture should parse");
        assert_eq!(response.provider, ProviderKind::Local);
        assert_eq!(response.content, "Hello locally");
        assert_eq!(response.usage.total_tokens, Some(9));
    }

    #[test]
    fn parses_compatible_content_block_arrays() {
        let fixture = r#"{
          "choices": [{
            "finish_reason": "stop",
            "message": {"content": [
              {"type": "text", "text": "block "},
              {"type": "text", "text": "content"}
            ]}
          }]
        }"#;

        let response = parse_response(fixture, "local-model").expect("fixture should parse");
        assert_eq!(response.content, "block content");
        assert_eq!(response.model, "local-model");
    }

    #[test]
    fn normalizes_compatible_stream_events() {
        let adapter = LocalAdapter;
        let mut state = StreamState::new(ProviderKind::Local, "fallback".to_owned());
        let first = adapter
            .parse_stream_data(
                r#"{"id":"chat_stream","model":"qwen","choices":[{"delta":{"content":"Hello "},"finish_reason":null}]}"#,
                &mut state,
            )
            .unwrap();
        let second = adapter
            .parse_stream_data(
                r#"{"id":"chat_stream","model":"qwen","choices":[{"delta":{"content":"locally"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}}"#,
                &mut state,
            )
            .unwrap();

        assert!(matches!(
            first.as_slice(),
            [kiln_core::ChatStreamEvent::MessageDelta { delta }]
                if delta == "Hello "
        ));
        assert!(matches!(
            second.as_slice(),
            [
                kiln_core::ChatStreamEvent::MessageDelta { delta },
                kiln_core::ChatStreamEvent::MessageCompleted { response }
            ] if delta == "locally"
                && response.content == "Hello locally"
                && response.usage.total_tokens == Some(7)
        ));
    }
}
