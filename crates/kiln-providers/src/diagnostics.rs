use std::time::{Duration, Instant};

use futures_util::StreamExt;
use kiln_core::{
    ConnectionProbe, ConnectionProbeKind, ConnectionProbeStatus, ConnectionTestOverall,
    ConnectionTestRequest, ConnectionTestResponse, ProviderCredentials, ProviderProtocol,
};
use reqwest::{header::CONTENT_TYPE, Client, RequestBuilder, StatusCode};
use serde_json::{json, Value};

use super::{
    apply_custom_headers, endpoint, resolve_destination, validate_credential_destination,
    ProviderAdapter, ResolvedProviderDestination, SseFramer,
};
use crate::error::ProviderError;
use crate::tool_turn::{ProviderTurnEvent, ToolTurnCodec};

const MODELS_TIMEOUT: Duration = Duration::from_secs(15);
const INFERENCE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_DIAGNOSTIC_BYTES: usize = 256 * 1024;
const MAX_DISCOVERED_MODELS: usize = 100;
const MAX_MODEL_ID_CHARS: usize = 200;
const CAPABILITY_TOOL: &str = "kiln_capability_probe";

pub(super) async fn test_connection(
    client: &Client,
    adapter: &'static dyn ProviderAdapter,
    request: &ConnectionTestRequest,
) -> Result<ConnectionTestResponse, ProviderError> {
    let capabilities = adapter.capabilities();
    let destination = resolve_destination(request.base_url.as_deref(), &capabilities)?;
    validate_credential_destination(&request.credentials, &destination.origin)?;
    let selected_model = request
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_owned);

    let models = probe_models(
        client,
        adapter,
        &destination,
        &request.credentials,
        capabilities.custom_headers,
    )
    .await;

    let mut authentication = models.authentication;
    let (streaming, tool_compatibility) = if !models.endpoint_responded
        || authentication.status == ConnectionProbeStatus::Failed
    {
        (
            skipped(
                ConnectionProbeKind::Streaming,
                "Streaming was not tested because the endpoint or credential is unavailable.",
            ),
            skipped(
                ConnectionProbeKind::ToolCompatibility,
                "Tool compatibility was not tested because the endpoint or credential is unavailable.",
            ),
        )
    } else if let Some(model) = selected_model.as_deref() {
        let streaming = if capabilities.streaming {
            probe_inference(
                client,
                adapter,
                &destination,
                &request.credentials,
                capabilities.custom_headers,
                model,
                InferenceProbe::TextStreaming,
            )
            .await
        } else {
            unsupported(
                ConnectionProbeKind::Streaming,
                "Kiln's adapter does not support streaming for this protocol.",
            )
        };
        if streaming.status == ConnectionProbeStatus::Passed
            && authentication.status == ConnectionProbeStatus::Skipped
        {
            authentication = passed(
                ConnectionProbeKind::Authentication,
                streaming.latency_ms,
                None,
                "The selected model request accepted the configured authentication.",
            );
        }

        let tools = if capabilities.tool_calling {
            probe_inference(
                client,
                adapter,
                &destination,
                &request.credentials,
                capabilities.custom_headers,
                model,
                InferenceProbe::ToolStreaming,
            )
            .await
        } else {
            unsupported(
                ConnectionProbeKind::ToolCompatibility,
                "Kiln's adapter does not support tool calls for this protocol.",
            )
        };
        if tools.status == ConnectionProbeStatus::Passed
            && authentication.status == ConnectionProbeStatus::Skipped
        {
            authentication = passed(
                ConnectionProbeKind::Authentication,
                tools.latency_ms,
                None,
                "The selected model request accepted the configured authentication.",
            );
        }
        (streaming, tools)
    } else {
        (
            skipped(
                ConnectionProbeKind::Streaming,
                "Choose a model to verify streamed generation.",
            ),
            skipped(
                ConnectionProbeKind::ToolCompatibility,
                "Choose a model to verify structured tool calls.",
            ),
        )
    };

    let probes = vec![
        models.reachability,
        authentication,
        models.discovery,
        streaming,
        tool_compatibility,
    ];
    let overall = overall(&probes);

    Ok(ConnectionTestResponse {
        provider: adapter.kind(),
        origin: destination.origin,
        model: selected_model,
        overall,
        models: models.models,
        models_truncated: models.models_truncated,
        probes,
    })
}

struct ModelProbeOutcome {
    reachability: ConnectionProbe,
    authentication: ConnectionProbe,
    discovery: ConnectionProbe,
    models: Vec<String>,
    models_truncated: bool,
    endpoint_responded: bool,
}

async fn probe_models(
    client: &Client,
    adapter: &'static dyn ProviderAdapter,
    destination: &ResolvedProviderDestination,
    credentials: &ProviderCredentials,
    custom_headers_allowed: bool,
) -> ModelProbeOutcome {
    let builder = client
        .get(endpoint(&destination.base_url, adapter.models_path()))
        .timeout(MODELS_TIMEOUT)
        .header(CONTENT_TYPE, "application/json");
    let builder = authenticated_builder(adapter, builder, credentials, custom_headers_allowed);
    let builder = match builder {
        Ok(builder) => builder,
        Err(()) => {
            return ModelProbeOutcome {
                reachability: skipped(
                    ConnectionProbeKind::Reachability,
                    "Reachability was not tested because the credential configuration is incomplete.",
                ),
                authentication: failed(
                    ConnectionProbeKind::Authentication,
                    None,
                    None,
                    "The credential configuration is incomplete or invalid.",
                ),
                discovery: skipped(
                    ConnectionProbeKind::ModelDiscovery,
                    "Model discovery was not tested because the credential configuration is incomplete.",
                ),
                models: Vec::new(),
                models_truncated: false,
                endpoint_responded: false,
            };
        }
    };

    let started = Instant::now();
    let response = match builder.send().await {
        Ok(response) => response,
        Err(_) => {
            return ModelProbeOutcome {
                reachability: failed(
                    ConnectionProbeKind::Reachability,
                    elapsed_ms(started),
                    None,
                    "Kiln could not reach the endpoint before the diagnostic timeout.",
                ),
                authentication: skipped(
                    ConnectionProbeKind::Authentication,
                    "Authentication was not tested because no HTTP response arrived.",
                ),
                discovery: skipped(
                    ConnectionProbeKind::ModelDiscovery,
                    "Model discovery was not tested because no HTTP response arrived.",
                ),
                models: Vec::new(),
                models_truncated: false,
                endpoint_responded: false,
            };
        }
    };

    let latency = elapsed_ms(started);
    let status = response.status();
    let reachability = passed(
        ConnectionProbeKind::Reachability,
        latency,
        Some(status.as_u16()),
        if status.is_redirection() {
            "The endpoint responded with a redirect. Kiln did not follow it with credentials."
        } else {
            "The endpoint returned an HTTP response."
        },
    );
    let authentication = classify_authentication(status, latency);

    if status.is_success() {
        match read_bounded_json(response).await {
            Ok(value) => match discovered_model_ids(&value) {
                Some((models, models_truncated)) => ModelProbeOutcome {
                    reachability,
                    authentication,
                    discovery: passed(
                        ConnectionProbeKind::ModelDiscovery,
                        latency,
                        Some(status.as_u16()),
                        if models.is_empty() {
                            "The endpoint returned a valid model list with no entries."
                        } else if models_truncated {
                            "Model discovery passed. Kiln displayed the first 100 model identifiers."
                        } else {
                            "Model discovery returned valid model identifiers."
                        },
                    ),
                    models,
                    models_truncated,
                    endpoint_responded: true,
                },
                None => ModelProbeOutcome {
                    reachability,
                    authentication,
                    discovery: failed(
                        ConnectionProbeKind::ModelDiscovery,
                        latency,
                        Some(status.as_u16()),
                        "The endpoint returned success without a valid, bounded model list.",
                    ),
                    models: Vec::new(),
                    models_truncated: false,
                    endpoint_responded: true,
                },
            },
            Err(()) => ModelProbeOutcome {
                reachability,
                authentication,
                discovery: failed(
                    ConnectionProbeKind::ModelDiscovery,
                    latency,
                    Some(status.as_u16()),
                    "The model-list response was malformed or exceeded Kiln's diagnostic limit.",
                ),
                models: Vec::new(),
                models_truncated: false,
                endpoint_responded: true,
            },
        }
    } else {
        let discovery = match status {
            StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED => unsupported(
                ConnectionProbeKind::ModelDiscovery,
                "This endpoint does not expose a compatible model-list route.",
            ),
            status if status.is_redirection() => failed(
                ConnectionProbeKind::ModelDiscovery,
                latency,
                Some(status.as_u16()),
                "Model discovery redirected to an unapproved destination.",
            ),
            _ => skipped(
                ConnectionProbeKind::ModelDiscovery,
                "Model discovery could not be evaluated from this response.",
            ),
        };
        ModelProbeOutcome {
            reachability,
            authentication,
            discovery,
            models: Vec::new(),
            models_truncated: false,
            endpoint_responded: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum InferenceProbe {
    TextStreaming,
    ToolStreaming,
}

impl InferenceProbe {
    fn kind(self) -> ConnectionProbeKind {
        match self {
            Self::TextStreaming => ConnectionProbeKind::Streaming,
            Self::ToolStreaming => ConnectionProbeKind::ToolCompatibility,
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn probe_inference(
    client: &Client,
    adapter: &'static dyn ProviderAdapter,
    destination: &ResolvedProviderDestination,
    credentials: &ProviderCredentials,
    custom_headers_allowed: bool,
    model: &str,
    probe: InferenceProbe,
) -> ConnectionProbe {
    let builder = client
        .post(endpoint(&destination.base_url, adapter.chat_path()))
        .timeout(INFERENCE_TIMEOUT)
        .header(CONTENT_TYPE, "application/json")
        .json(&diagnostic_payload(
            adapter.capabilities().protocol,
            model,
            probe,
        ));
    let builder = match authenticated_builder(adapter, builder, credentials, custom_headers_allowed)
    {
        Ok(builder) => builder,
        Err(()) => {
            return failed(
                probe.kind(),
                None,
                None,
                "The credential configuration is incomplete or invalid.",
            );
        }
    };

    let started = Instant::now();
    let response = match builder.send().await {
        Ok(response) => response,
        Err(_) => {
            return failed(
                probe.kind(),
                elapsed_ms(started),
                None,
                "The model probe did not receive a response before the timeout.",
            );
        }
    };
    let latency = elapsed_ms(started);
    let status = response.status();
    if !status.is_success() {
        return match status {
            StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED => unsupported(
                probe.kind(),
                "The endpoint does not expose the protocol route required for this probe.",
            ),
            status if status.is_redirection() => failed(
                probe.kind(),
                latency,
                Some(status.as_u16()),
                "The model probe redirected to an unapproved destination.",
            ),
            _ => failed(
                probe.kind(),
                latency,
                Some(status.as_u16()),
                "The endpoint rejected the synthetic model probe.",
            ),
        };
    }

    let events = match read_bounded_sse(response).await {
        Ok(events) => events,
        Err(()) => {
            return failed(
                probe.kind(),
                latency,
                Some(status.as_u16()),
                "The streamed response was malformed, incomplete, or exceeded Kiln's diagnostic limit.",
            );
        }
    };
    if validate_diagnostic_stream(adapter.capabilities().protocol, probe, &events) {
        passed(
            probe.kind(),
            latency,
            Some(status.as_u16()),
            match probe {
                InferenceProbe::TextStreaming => {
                    "The selected model completed a valid streamed response."
                }
                InferenceProbe::ToolStreaming => {
                    "The selected model emitted a valid synthetic tool call. Kiln did not execute it."
                }
            },
        )
    } else {
        failed(
            probe.kind(),
            latency,
            Some(status.as_u16()),
            match probe {
                InferenceProbe::TextStreaming => {
                    "The endpoint did not complete the streamed response contract."
                }
                InferenceProbe::ToolStreaming => {
                    "The endpoint did not emit a complete compatible synthetic tool call."
                }
            },
        )
    }
}

fn authenticated_builder(
    adapter: &'static dyn ProviderAdapter,
    builder: RequestBuilder,
    credentials: &ProviderCredentials,
    custom_headers_allowed: bool,
) -> Result<RequestBuilder, ()> {
    let builder =
        apply_custom_headers(builder, credentials, custom_headers_allowed).map_err(|_| ())?;
    adapter
        .apply_provider_headers(builder, credentials)
        .map_err(|_| ())
}

fn diagnostic_payload(protocol: ProviderProtocol, model: &str, probe: InferenceProbe) -> Value {
    let tool_parameters = json!({
        "type": "object",
        "properties": {
            "value": { "type": "string", "enum": ["ok"] }
        },
        "required": ["value"],
        "additionalProperties": false
    });
    match (protocol, probe) {
        (ProviderProtocol::OpenAiResponses, InferenceProbe::TextStreaming) => json!({
            "model": model,
            "input": "Reply only with OK.",
            "max_output_tokens": 16,
            "store": false,
            "stream": true
        }),
        (ProviderProtocol::OpenAiResponses, InferenceProbe::ToolStreaming) => json!({
            "model": model,
            "input": "Call kiln_capability_probe once with value ok. Do not answer otherwise.",
            "max_output_tokens": 64,
            "store": false,
            "stream": true,
            "parallel_tool_calls": false,
            "tools": [{
                "type": "function",
                "name": CAPABILITY_TOOL,
                "description": "Synthetic no-op used only to verify the provider protocol.",
                "parameters": tool_parameters,
                "strict": true
            }],
            "tool_choice": { "type": "function", "name": CAPABILITY_TOOL }
        }),
        (ProviderProtocol::AnthropicMessages, InferenceProbe::TextStreaming) => json!({
            "model": model,
            "max_tokens": 8,
            "messages": [{ "role": "user", "content": "Reply only with OK." }],
            "stream": true
        }),
        (ProviderProtocol::AnthropicMessages, InferenceProbe::ToolStreaming) => json!({
            "model": model,
            "max_tokens": 64,
            "messages": [{
                "role": "user",
                "content": "Call kiln_capability_probe once with value ok. Do not answer otherwise."
            }],
            "stream": true,
            "tools": [{
                "name": CAPABILITY_TOOL,
                "description": "Synthetic no-op used only to verify the provider protocol.",
                "input_schema": tool_parameters
            }],
            "tool_choice": { "type": "tool", "name": CAPABILITY_TOOL }
        }),
        (ProviderProtocol::OpenAiChatCompletions, InferenceProbe::TextStreaming) => json!({
            "model": model,
            "messages": [{ "role": "user", "content": "Reply only with OK." }],
            "max_tokens": 8,
            "stream": true
        }),
        (ProviderProtocol::OpenAiChatCompletions, InferenceProbe::ToolStreaming) => json!({
            "model": model,
            "messages": [{
                "role": "user",
                "content": "Call kiln_capability_probe once with value ok. Do not answer otherwise."
            }],
            "max_tokens": 64,
            "stream": true,
            "parallel_tool_calls": false,
            "tools": [{
                "type": "function",
                "function": {
                    "name": CAPABILITY_TOOL,
                    "description": "Synthetic no-op used only to verify the provider protocol.",
                    "parameters": tool_parameters,
                    "strict": true
                }
            }],
            "tool_choice": {
                "type": "function",
                "function": { "name": CAPABILITY_TOOL }
            }
        }),
    }
}

fn validate_diagnostic_stream(
    protocol: ProviderProtocol,
    probe: InferenceProbe,
    events: &[String],
) -> bool {
    match (protocol, probe) {
        (ProviderProtocol::OpenAiResponses, InferenceProbe::TextStreaming) => {
            validate_openai_text(events)
        }
        (ProviderProtocol::OpenAiResponses, InferenceProbe::ToolStreaming)
        | (ProviderProtocol::AnthropicMessages, InferenceProbe::ToolStreaming)
        | (ProviderProtocol::OpenAiChatCompletions, InferenceProbe::ToolStreaming) => {
            validate_tool_stream(protocol, events)
        }
        (ProviderProtocol::AnthropicMessages, InferenceProbe::TextStreaming) => {
            validate_anthropic_text(events)
        }
        (ProviderProtocol::OpenAiChatCompletions, InferenceProbe::TextStreaming) => {
            validate_compatible_text(events)
        }
    }
}

fn validate_tool_stream(protocol: ProviderProtocol, events: &[String]) -> bool {
    let mut codec = ToolTurnCodec::new(protocol);
    let mut calls = Vec::new();
    for event in events {
        let decoded = match codec.push(event) {
            Ok(decoded) => decoded,
            Err(_) => return false,
        };
        for event in decoded {
            if let ProviderTurnEvent::ToolCall { call } = event {
                calls.push(call);
            }
        }
    }
    codec.finish().is_ok()
        && calls.len() == 1
        && calls[0].name() == CAPABILITY_TOOL
        && valid_probe_arguments(Some(calls[0].arguments()))
}

fn json_events(events: &[String]) -> Option<Vec<Value>> {
    events
        .iter()
        .filter(|event| event.as_str() != "[DONE]")
        .map(|event| serde_json::from_str(event).ok())
        .collect()
}

fn validate_openai_text(events: &[String]) -> bool {
    let Some(values) = json_events(events) else {
        return false;
    };
    let delta = values.iter().any(|value| {
        value.get("type").and_then(Value::as_str) == Some("response.output_text.delta")
            && value
                .get("delta")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.is_empty())
    });
    let completed = values
        .iter()
        .any(|value| value.get("type").and_then(Value::as_str) == Some("response.completed"));
    delta && completed
}

fn validate_anthropic_text(events: &[String]) -> bool {
    let Some(values) = json_events(events) else {
        return false;
    };
    let started = values
        .iter()
        .any(|value| value.get("type").and_then(Value::as_str) == Some("message_start"));
    let delta = values.iter().any(|value| {
        value.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta")
            && value
                .pointer("/delta/text")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.is_empty())
    });
    let stopped = values
        .iter()
        .any(|value| value.get("type").and_then(Value::as_str) == Some("message_stop"));
    started && delta && stopped
}

fn validate_compatible_text(events: &[String]) -> bool {
    let Some(values) = json_events(events) else {
        return false;
    };
    let delta = values.iter().any(|value| {
        value
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty())
    });
    let terminal = events.iter().any(|event| event == "[DONE]")
        || values.iter().any(|value| {
            value
                .pointer("/choices/0/finish_reason")
                .is_some_and(|reason| !reason.is_null())
        });
    delta && terminal
}

fn valid_probe_arguments(arguments: Option<&str>) -> bool {
    arguments
        .and_then(|arguments| serde_json::from_str(arguments).ok())
        .as_ref()
        .is_some_and(|value| valid_probe_value(Some(value)))
}

fn valid_probe_value(value: Option<&Value>) -> bool {
    value.and_then(Value::as_object).is_some_and(|object| {
        object.len() == 1 && object.get("value").and_then(Value::as_str) == Some("ok")
    })
}

async fn read_bounded_json(response: reqwest::Response) -> Result<Value, ()> {
    let bytes = read_bounded_bytes(response).await?;
    serde_json::from_slice(&bytes).map_err(|_| ())
}

async fn read_bounded_sse(response: reqwest::Response) -> Result<Vec<String>, ()> {
    let mut stream = response.bytes_stream();
    let mut total = 0_usize;
    let mut framer = SseFramer::default();
    let mut events = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ())?;
        total = total.checked_add(chunk.len()).ok_or(())?;
        if total > MAX_DIAGNOSTIC_BYTES {
            return Err(());
        }
        events.extend(framer.push(&chunk).map_err(|_| ())?);
    }
    events.extend(framer.finish().map_err(|_| ())?);
    Ok(events)
}

async fn read_bounded_bytes(response: reqwest::Response) -> Result<Vec<u8>, ()> {
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ())?;
        let next_len = bytes.len().checked_add(chunk.len()).ok_or(())?;
        if next_len > MAX_DIAGNOSTIC_BYTES {
            return Err(());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn discovered_model_ids(value: &Value) -> Option<(Vec<String>, bool)> {
    let values = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array))
        .or_else(|| value.as_array())?;
    let mut models = Vec::with_capacity(values.len().min(MAX_DISCOVERED_MODELS));
    let mut truncated = values.len() > MAX_DISCOVERED_MODELS;
    for value in values {
        let id = value.as_str().or_else(|| {
            value
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| value.get("name").and_then(Value::as_str))
                .or_else(|| value.get("model").and_then(Value::as_str))
        })?;
        let id = id.trim();
        if id.is_empty() {
            return None;
        }
        if models.len() == MAX_DISCOVERED_MODELS {
            truncated = true;
            continue;
        }
        let bounded = id.chars().take(MAX_MODEL_ID_CHARS).collect::<String>();
        if bounded.chars().count() != id.chars().count() {
            truncated = true;
        }
        models.push(bounded);
    }
    models.sort();
    models.dedup();
    Some((models, truncated))
}

fn classify_authentication(status: StatusCode, latency_ms: Option<u64>) -> ConnectionProbe {
    match status {
        status if status.is_success() => passed(
            ConnectionProbeKind::Authentication,
            latency_ms,
            Some(status.as_u16()),
            "The endpoint accepted the configured authentication.",
        ),
        StatusCode::UNAUTHORIZED => failed(
            ConnectionProbeKind::Authentication,
            latency_ms,
            Some(status.as_u16()),
            "The endpoint rejected the configured credential.",
        ),
        StatusCode::FORBIDDEN => failed(
            ConnectionProbeKind::Authentication,
            latency_ms,
            Some(status.as_u16()),
            "The credential does not have permission to inspect this endpoint.",
        ),
        _ => ConnectionProbe {
            kind: ConnectionProbeKind::Authentication,
            status: ConnectionProbeStatus::Skipped,
            latency_ms,
            http_status: Some(status.as_u16()),
            message: "Authentication could not be isolated from this endpoint response.".to_owned(),
        },
    }
}

fn overall(probes: &[ConnectionProbe]) -> ConnectionTestOverall {
    let status = |kind| {
        probes
            .iter()
            .find(|probe| probe.kind == kind)
            .map(|probe| probe.status)
    };
    if status(ConnectionProbeKind::Reachability) == Some(ConnectionProbeStatus::Failed)
        || status(ConnectionProbeKind::Authentication) == Some(ConnectionProbeStatus::Failed)
    {
        ConnectionTestOverall::Unavailable
    } else if probes
        .iter()
        .all(|probe| probe.status == ConnectionProbeStatus::Passed)
    {
        ConnectionTestOverall::Ready
    } else {
        ConnectionTestOverall::Degraded
    }
}

fn elapsed_ms(started: Instant) -> Option<u64> {
    Some(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
}

fn passed(
    kind: ConnectionProbeKind,
    latency_ms: Option<u64>,
    http_status: Option<u16>,
    message: &str,
) -> ConnectionProbe {
    ConnectionProbe {
        kind,
        status: ConnectionProbeStatus::Passed,
        latency_ms,
        http_status,
        message: message.to_owned(),
    }
}

fn failed(
    kind: ConnectionProbeKind,
    latency_ms: Option<u64>,
    http_status: Option<u16>,
    message: &str,
) -> ConnectionProbe {
    ConnectionProbe {
        kind,
        status: ConnectionProbeStatus::Failed,
        latency_ms,
        http_status,
        message: message.to_owned(),
    }
}

fn unsupported(kind: ConnectionProbeKind, message: &str) -> ConnectionProbe {
    ConnectionProbe {
        kind,
        status: ConnectionProbeStatus::Unsupported,
        latency_ms: None,
        http_status: None,
        message: message.to_owned(),
    }
}

fn skipped(kind: ConnectionProbeKind, message: &str) -> ConnectionProbe {
    ConnectionProbe {
        kind,
        status: ConnectionProbeStatus::Skipped,
        latency_ms: None,
        http_status: None,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(value: Value) -> String {
        value.to_string()
    }

    #[test]
    fn discovers_and_bounds_common_model_list_shapes() {
        assert_eq!(
            discovered_model_ids(&json!({"data": [{"id": "gpt-b"}, {"id": "gpt-a"}]}))
                .unwrap()
                .0,
            vec!["gpt-a", "gpt-b"]
        );
        assert_eq!(
            discovered_model_ids(&json!({"models": [{"name": "local-model"}]}))
                .unwrap()
                .0,
            vec!["local-model"]
        );
        assert!(discovered_model_ids(&json!({"data": [{}]})).is_none());
    }

    #[test]
    fn validates_openai_text_and_tool_streams() {
        let text = vec![
            event(json!({"type": "response.output_text.delta", "delta": "OK"})),
            event(json!({"type": "response.completed", "response": {}})),
        ];
        assert!(validate_openai_text(&text));

        let tool = vec![
            event(json!({
                "type": "response.output_item.added",
                "item": {
                    "type": "function_call",
                    "id": "fc_probe",
                    "call_id": "call_probe",
                    "name": CAPABILITY_TOOL,
                    "arguments": ""
                }
            })),
            event(json!({
                "type": "response.function_call_arguments.done",
                "item_id": "fc_probe",
                "arguments": "{\"value\":\"ok\"}"
            })),
            event(json!({"type": "response.completed", "response": {}})),
        ];
        assert!(validate_tool_stream(
            ProviderProtocol::OpenAiResponses,
            &tool
        ));
    }

    #[test]
    fn validates_anthropic_text_and_tool_streams() {
        let text = vec![
            event(json!({"type": "message_start", "message": {}})),
            event(json!({
                "type": "content_block_delta",
                "delta": {"type": "text_delta", "text": "OK"}
            })),
            event(json!({"type": "message_stop"})),
        ];
        assert!(validate_anthropic_text(&text));

        let tool = vec![
            event(json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "tool_use",
                    "id": "toolu_probe",
                    "name": CAPABILITY_TOOL,
                    "input": {"value": "ok"}
                }
            })),
            event(json!({"type": "content_block_stop", "index": 0})),
            event(json!({
                "type": "message_delta",
                "delta": {"stop_reason": "tool_use"}
            })),
            event(json!({"type": "message_stop"})),
        ];
        assert!(validate_tool_stream(
            ProviderProtocol::AnthropicMessages,
            &tool
        ));
    }

    #[test]
    fn validates_compatible_text_and_tool_streams() {
        let text = vec![
            event(json!({"choices": [{"delta": {"content": "OK"}}]})),
            event(json!({"choices": [{"finish_reason": "stop"}]})),
            "[DONE]".to_owned(),
        ];
        assert!(validate_compatible_text(&text));

        let tool = vec![
            event(json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_probe",
                            "function": {
                                "name": CAPABILITY_TOOL,
                                "arguments": "{\"value\":"
                            }
                        }]
                    }
                }]
            })),
            event(json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "function": {"arguments": "\"ok\"}"}
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            })),
            "[DONE]".to_owned(),
        ];
        assert!(validate_tool_stream(
            ProviderProtocol::OpenAiChatCompletions,
            &tool
        ));
    }

    #[test]
    fn rejects_incomplete_or_wrong_tool_streams() {
        assert!(!validate_openai_text(&[event(json!({
            "type": "response.output_text.delta",
            "delta": "OK"
        }))]));
        assert!(!validate_tool_stream(
            ProviderProtocol::AnthropicMessages,
            &[
                event(json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_wrong",
                        "name": "something_else",
                        "input": {"value": "ok"}
                    }
                })),
                event(json!({"type": "content_block_stop", "index": 0})),
                event(json!({"type": "message_stop"})),
            ]
        ));
        assert!(!validate_tool_stream(
            ProviderProtocol::OpenAiChatCompletions,
            &[event(json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_bad",
                            "function": {
                                "name": CAPABILITY_TOOL,
                                "arguments": "not-json"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }))]
        ));
        assert!(!valid_probe_value(Some(&json!({
            "value": "ok",
            "unexpected": true
        }))));
    }

    #[test]
    fn overall_requires_reachability_and_authentication() {
        let unavailable = vec![
            failed(ConnectionProbeKind::Reachability, None, None, "unreachable"),
            skipped(ConnectionProbeKind::Authentication, "not run"),
        ];
        assert_eq!(overall(&unavailable), ConnectionTestOverall::Unavailable);
        let degraded = vec![
            passed(ConnectionProbeKind::Reachability, None, Some(200), "ok"),
            passed(ConnectionProbeKind::Authentication, None, Some(200), "ok"),
            unsupported(ConnectionProbeKind::ModelDiscovery, "unsupported"),
        ];
        assert_eq!(overall(&degraded), ConnectionTestOverall::Degraded);
    }
}
