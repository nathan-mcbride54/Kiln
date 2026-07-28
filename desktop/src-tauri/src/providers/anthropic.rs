use reqwest::{header::HeaderName, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{to_value, Value};

use crate::{
    error::ProviderError,
    types::{
        ChatRequest, ChatResponse, ChatRole, ProviderCapabilities, ProviderCredentials,
        ProviderKind, ProviderProtocol, TokenUsage,
    },
};

use super::{require_api_key, sensitive_header, ProviderAdapter};

pub(crate) struct AnthropicAdapter;

impl ProviderAdapter for AnthropicAdapter {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Anthropic
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider: self.kind(),
            display_name: "Anthropic",
            protocol: ProviderProtocol::AnthropicMessages,
            default_base_url: "https://api.anthropic.com/v1",
            api_key_required: true,
            custom_base_url: true,
            custom_headers: true,
            model_discovery: true,
            streaming: false,
            system_messages: true,
            temperature: true,
        }
    }

    fn chat_path(&self) -> &'static str {
        "messages"
    }

    fn apply_provider_headers(
        &self,
        request: RequestBuilder,
        credentials: &ProviderCredentials,
    ) -> Result<RequestBuilder, ProviderError> {
        let key = require_api_key(credentials, "Anthropic")?;
        Ok(request
            .header(HeaderName::from_static("x-api-key"), sensitive_header(key)?)
            .header(HeaderName::from_static("anthropic-version"), "2023-06-01"))
    }

    fn chat_payload(&self, request: &ChatRequest) -> Result<Value, ProviderError> {
        if request
            .temperature
            .is_some_and(|temperature| temperature > 1.0)
        {
            return Err(ProviderError::InvalidRequest(
                "Anthropic temperature must be between 0 and 1.".to_owned(),
            ));
        }

        let system = request
            .messages
            .iter()
            .filter(|message| matches!(message.role, ChatRole::System | ChatRole::Developer))
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let messages = collapse_messages(
            request
                .messages
                .iter()
                .filter(|message| matches!(message.role, ChatRole::User | ChatRole::Assistant)),
        );

        let payload = AnthropicRequestPayload {
            model: request.model.trim(),
            system: (!system.is_empty()).then_some(system),
            messages,
            max_tokens: request.max_output_tokens.unwrap_or(1024),
            temperature: request.temperature,
        };
        to_value(payload).map_err(|error| {
            ProviderError::InvalidRequest(format!(
                "The Anthropic request could not be encoded: {error}"
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
}

#[derive(Serialize)]
struct AnthropicRequestPayload<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: &'static str,
    content: String,
}

fn collapse_messages<'a>(
    messages: impl Iterator<Item = &'a crate::types::ChatMessage>,
) -> Vec<AnthropicMessage> {
    let mut collapsed: Vec<AnthropicMessage> = Vec::new();
    for message in messages {
        let role = match message.role {
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
            ChatRole::System | ChatRole::Developer => continue,
        };
        if let Some(previous) = collapsed.last_mut().filter(|item| item.role == role) {
            previous.content.push_str("\n\n");
            previous.content.push_str(&message.content);
        } else {
            collapsed.push(AnthropicMessage {
                role,
                content: message.content.clone(),
            });
        }
    }
    collapsed
}

#[derive(Deserialize)]
struct AnthropicResponsePayload {
    id: Option<String>,
    model: Option<String>,
    stop_reason: Option<String>,
    #[serde(default)]
    content: Vec<AnthropicContentBlock>,
    usage: Option<AnthropicUsage>,
}

#[derive(Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

fn parse_response(body: &str, requested_model: &str) -> Result<ChatResponse, ProviderError> {
    let parsed: AnthropicResponsePayload = serde_json::from_str(body).map_err(|error| {
        ProviderError::MalformedResponse(format!(
            "Anthropic returned a response Kiln could not parse: {error}"
        ))
    })?;
    let content = parsed
        .content
        .iter()
        .filter(|block| block.kind.as_deref().is_none_or(|kind| kind == "text"))
        .filter_map(|block| block.text.as_deref())
        .collect::<String>();
    if content.is_empty() {
        return Err(ProviderError::MalformedResponse(
            "Anthropic completed the request without returning a text block.".to_owned(),
        ));
    }
    let usage = parsed
        .usage
        .map(|usage| TokenUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: match (usage.input_tokens, usage.output_tokens) {
                (Some(input), Some(output)) => input.checked_add(output),
                _ => None,
            },
        })
        .unwrap_or_default();

    Ok(ChatResponse {
        provider: ProviderKind::Anthropic,
        id: parsed.id,
        model: parsed.model.unwrap_or_else(|| requested_model.to_owned()),
        content,
        finish_reason: parsed.stop_reason,
        usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_messages_api_text_blocks_and_usage() {
        let fixture = r#"{
          "id": "msg_123",
          "model": "claude-sonnet-4-5",
          "stop_reason": "end_turn",
          "content": [
            {"type": "text", "text": "Hello "},
            {"type": "text", "text": "from Anthropic"}
          ],
          "usage": {"input_tokens": 9, "output_tokens": 5}
        }"#;

        let response = parse_response(fixture, "fallback").expect("fixture should parse");
        assert_eq!(response.id.as_deref(), Some("msg_123"));
        assert_eq!(response.content, "Hello from Anthropic");
        assert_eq!(response.finish_reason.as_deref(), Some("end_turn"));
        assert_eq!(response.usage.total_tokens, Some(14));
    }

    #[test]
    fn collapses_adjacent_roles_for_anthropic() {
        let source = [
            crate::types::ChatMessage {
                role: ChatRole::User,
                content: "one".to_owned(),
            },
            crate::types::ChatMessage {
                role: ChatRole::User,
                content: "two".to_owned(),
            },
            crate::types::ChatMessage {
                role: ChatRole::Assistant,
                content: "three".to_owned(),
            },
        ];
        let collapsed = collapse_messages(source.iter());
        assert_eq!(collapsed.len(), 2);
        assert_eq!(collapsed[0].content, "one\n\ntwo");
    }
}
