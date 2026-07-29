use kiln_core::{
    ChatRequest, ChatResponse, ChatRole, ProviderCapabilities, ProviderCredentials, ProviderKind,
    ProviderProtocol, TokenUsage,
};
use reqwest::{
    header::{HeaderName, HeaderValue, AUTHORIZATION},
    RequestBuilder,
};
use serde::{Deserialize, Serialize};
use serde_json::{to_value, Value};

use super::StreamState;
use super::{bearer_header, require_api_key, ProviderAdapter};
use crate::error::ProviderError;

pub(crate) struct OpenAiAdapter;

impl ProviderAdapter for OpenAiAdapter {
    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenAi
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider: self.kind(),
            display_name: "OpenAI",
            protocol: ProviderProtocol::OpenAiResponses,
            default_base_url: "https://api.openai.com/v1",
            api_key_required: true,
            custom_base_url: false,
            custom_headers: false,
            model_discovery: true,
            streaming: true,
            tool_calling: true,
            system_messages: true,
            temperature: true,
        }
    }

    fn chat_path(&self) -> &'static str {
        "responses"
    }

    fn apply_provider_headers(
        &self,
        mut request: RequestBuilder,
        credentials: &ProviderCredentials,
    ) -> Result<RequestBuilder, ProviderError> {
        let key = require_api_key(credentials, "OpenAI")?;
        request = request.header(AUTHORIZATION, bearer_header(key)?);

        if let Some(organization) = credentials.organization.as_deref() {
            request = request.header(
                HeaderName::from_static("openai-organization"),
                regular_header(organization, "organization")?,
            );
        }
        if let Some(project) = credentials.project.as_deref() {
            request = request.header(
                HeaderName::from_static("openai-project"),
                regular_header(project, "project")?,
            );
        }

        Ok(request)
    }

    fn chat_payload(&self, request: &ChatRequest) -> Result<Value, ProviderError> {
        let input = request
            .messages
            .iter()
            .map(|message| OpenAiInputMessage {
                role: match message.role {
                    ChatRole::System => "system",
                    ChatRole::Developer => "developer",
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                },
                content: &message.content,
            })
            .collect();
        let payload = OpenAiRequestPayload {
            model: request.model.trim(),
            input,
            max_output_tokens: request.max_output_tokens,
            temperature: request.temperature,
            stream: false,
        };
        to_value(payload).map_err(|error| {
            ProviderError::InvalidRequest(format!(
                "The OpenAI request could not be encoded: {error}"
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
                "OpenAI returned a stream event Kiln could not parse: {error}"
            ))
        })?;
        match value.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => Ok(value
                .get("delta")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .and_then(|delta| state.delta(delta))
                .into_iter()
                .collect()),
            Some("response.completed") => {
                let response = value.get("response").ok_or_else(|| {
                    ProviderError::MalformedResponse(
                        "OpenAI completed a stream without a response object.".to_owned(),
                    )
                })?;
                let response = parse_response(&response.to_string(), &state.requested_model)?;
                Ok(state.complete_response(response).into_iter().collect())
            }
            Some("response.failed") | Some("error") => Err(ProviderError::Upstream {
                status: 500,
                message: "OpenAI reported a failed stream.".to_owned(),
            }),
            _ => Ok(Vec::new()),
        }
    }
}

fn regular_header(value: &str, field_name: &str) -> Result<HeaderValue, ProviderError> {
    HeaderValue::from_str(value).map_err(|_| {
        ProviderError::InvalidConfiguration(format!(
            "The OpenAI {field_name} contains invalid header characters."
        ))
    })
}

#[derive(Serialize)]
struct OpenAiRequestPayload<'a> {
    model: &'a str,
    input: Vec<OpenAiInputMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
}

#[derive(Serialize)]
struct OpenAiInputMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Deserialize)]
struct OpenAiResponsePayload {
    id: Option<String>,
    model: Option<String>,
    status: Option<String>,
    output_text: Option<String>,
    #[serde(default)]
    output: Vec<OpenAiOutputItem>,
    usage: Option<OpenAiUsage>,
    incomplete_details: Option<IncompleteDetails>,
}

#[derive(Deserialize)]
struct OpenAiOutputItem {
    #[serde(default)]
    content: Vec<OpenAiContentItem>,
}

#[derive(Deserialize)]
struct OpenAiContentItem {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct IncompleteDetails {
    reason: Option<String>,
}

fn parse_response(body: &str, requested_model: &str) -> Result<ChatResponse, ProviderError> {
    let parsed: OpenAiResponsePayload = serde_json::from_str(body).map_err(|error| {
        ProviderError::MalformedResponse(format!(
            "OpenAI returned a response Kiln could not parse: {error}"
        ))
    })?;

    let nested_text = parsed
        .output
        .iter()
        .flat_map(|item| &item.content)
        .filter(|item| {
            item.kind
                .as_deref()
                .map_or(true, |kind| kind == "output_text")
        })
        .filter_map(|item| item.text.as_deref())
        .collect::<String>();
    let content = parsed
        .output_text
        .filter(|text| !text.is_empty())
        .unwrap_or(nested_text);
    if content.is_empty() {
        return Err(ProviderError::MalformedResponse(
            "OpenAI completed the request without returning output text.".to_owned(),
        ));
    }

    let finish_reason = parsed
        .incomplete_details
        .and_then(|details| details.reason)
        .or(parsed.status);
    let usage = parsed
        .usage
        .map(|usage| TokenUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
        })
        .unwrap_or_default();

    Ok(ChatResponse {
        provider: ProviderKind::OpenAi,
        id: parsed.id,
        model: parsed.model.unwrap_or_else(|| requested_model.to_owned()),
        content,
        finish_reason,
        usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_responses_api_output_and_usage() {
        let fixture = r#"{
          "id": "resp_123",
          "model": "gpt-5",
          "status": "completed",
          "output": [{
            "type": "message",
            "role": "assistant",
            "content": [
              {"type": "output_text", "text": "Hello "},
              {"type": "output_text", "text": "from OpenAI"}
            ]
          }],
          "usage": {"input_tokens": 12, "output_tokens": 4, "total_tokens": 16}
        }"#;

        let response = parse_response(fixture, "fallback").expect("fixture should parse");
        assert_eq!(response.id.as_deref(), Some("resp_123"));
        assert_eq!(response.model, "gpt-5");
        assert_eq!(response.content, "Hello from OpenAI");
        assert_eq!(response.finish_reason.as_deref(), Some("completed"));
        assert_eq!(response.usage.total_tokens, Some(16));
    }

    #[test]
    fn reports_missing_output_text() {
        let error = parse_response(r#"{"id":"resp_123","output":[]}"#, "gpt-5")
            .expect_err("missing output should fail");
        assert!(matches!(error, ProviderError::MalformedResponse(_)));
    }

    #[test]
    fn normalizes_responses_api_stream_events() {
        let adapter = OpenAiAdapter;
        let mut state = StreamState::new(ProviderKind::OpenAi, "fallback".to_owned());
        let delta = adapter
            .parse_stream_data(
                r#"{"type":"response.output_text.delta","delta":"Hello "}"#,
                &mut state,
            )
            .unwrap();
        let completed = adapter
            .parse_stream_data(
                r#"{"type":"response.completed","response":{"id":"resp_stream","model":"gpt-5","status":"completed","output_text":"Hello world","usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5}}}"#,
                &mut state,
            )
            .unwrap();

        assert_eq!(
            delta,
            vec![kiln_core::ChatStreamEvent::MessageDelta {
                delta: "Hello ".to_owned()
            }]
        );
        assert!(matches!(
            completed.as_slice(),
            [kiln_core::ChatStreamEvent::MessageCompleted { response }]
                if response.content == "Hello world"
                    && response.usage.total_tokens == Some(5)
        ));
    }
}
