use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{self, Debug, Formatter},
};

use kiln_core::{
    repository_tool_definitions, ProviderProtocol, RepositoryToolOutcome, RepositoryToolRequest,
    ToolContractError, MAX_TOOL_ARGUMENT_BYTES,
};
use serde_json::{json, Value};
use thiserror::Error;

pub const MAX_TOOL_CALLS_PER_TURN: usize = 16;
pub const MAX_TOOL_NAME_CHARS: usize = 64;
pub const MAX_PROVIDER_TOOL_HANDLE_CHARS: usize = 256;
pub const MAX_TOOL_TURN_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderToolCallHandle(String);

impl Debug for ProviderToolCallHandle {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProviderToolCallHandle([OPAQUE])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderToolCall {
    handle: ProviderToolCallHandle,
    name: String,
    arguments: String,
}

impl ProviderToolCall {
    pub fn handle(&self) -> &ProviderToolCallHandle {
        &self.handle
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn arguments(&self) -> &str {
        &self.arguments
    }

    pub fn repository_request(&self) -> Result<RepositoryToolRequest, ToolContractError> {
        RepositoryToolRequest::from_provider_call(&self.name, &self.arguments)
    }
}

impl Debug for ProviderToolCall {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderToolCall")
            .field("handle", &self.handle)
            .field("name", &self.name)
            .field("argument_bytes", &self.arguments.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderTurnEvent {
    MessageDelta { delta: String },
    ToolCall { call: ProviderToolCall },
    Completed { finish_reason: Option<String> },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ToolTurnCodecError {
    #[error("the provider tool stream exceeded {MAX_TOOL_TURN_BYTES} bytes")]
    TurnTooLarge,
    #[error("the provider emitted more than {MAX_TOOL_CALLS_PER_TURN} tool calls")]
    TooManyCalls,
    #[error("the provider emitted an invalid or ambiguous tool-call sequence")]
    InvalidSequence,
    #[error("the provider emitted a malformed tool-call event")]
    MalformedEvent,
    #[error("the provider emitted an invalid tool-call handle")]
    InvalidHandle,
    #[error("the provider emitted an invalid tool name")]
    InvalidName,
    #[error("the provider tool arguments exceeded {MAX_TOOL_ARGUMENT_BYTES} bytes")]
    ArgumentsTooLarge,
    #[error("the provider stream ended before a complete terminal event")]
    Incomplete,
    #[error("Kiln could not encode a provider tool continuation")]
    ContinuationEncoding,
}

#[derive(Clone)]
struct PendingToolCall {
    handle: String,
    name: String,
    arguments: String,
}

#[derive(Clone)]
pub struct ToolTurnCodec {
    protocol: ProviderProtocol,
    pending: BTreeMap<String, PendingToolCall>,
    order: Vec<String>,
    completed: BTreeSet<String>,
    total_bytes: usize,
    terminal: bool,
    finish_reason: Option<String>,
}

impl Debug for ToolTurnCodec {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolTurnCodec")
            .field("protocol", &self.protocol)
            .field("pending_calls", &self.pending.len())
            .field("completed_calls", &self.completed.len())
            .field("total_bytes", &self.total_bytes)
            .field("terminal", &self.terminal)
            .field("has_finish_reason", &self.finish_reason.is_some())
            .finish()
    }
}

impl ToolTurnCodec {
    pub fn new(protocol: ProviderProtocol) -> Self {
        Self {
            protocol,
            pending: BTreeMap::new(),
            order: Vec::new(),
            completed: BTreeSet::new(),
            total_bytes: 0,
            terminal: false,
            finish_reason: None,
        }
    }

    pub fn push(&mut self, data: &str) -> Result<Vec<ProviderTurnEvent>, ToolTurnCodecError> {
        self.total_bytes = self
            .total_bytes
            .checked_add(data.len())
            .ok_or(ToolTurnCodecError::TurnTooLarge)?;
        if self.total_bytes > MAX_TOOL_TURN_BYTES {
            return Err(ToolTurnCodecError::TurnTooLarge);
        }

        if data == "[DONE]" {
            return if self.terminal {
                Ok(Vec::new())
            } else {
                Err(ToolTurnCodecError::Incomplete)
            };
        }
        if self.terminal {
            return Err(ToolTurnCodecError::InvalidSequence);
        }

        let value: Value =
            serde_json::from_str(data).map_err(|_| ToolTurnCodecError::MalformedEvent)?;
        match self.protocol {
            ProviderProtocol::OpenAiResponses => self.push_openai(&value),
            ProviderProtocol::AnthropicMessages => self.push_anthropic(&value),
            ProviderProtocol::OpenAiChatCompletions => self.push_compatible(&value),
        }
    }

    pub fn finish(&self) -> Result<(), ToolTurnCodecError> {
        if self.terminal && self.pending.is_empty() {
            Ok(())
        } else {
            Err(ToolTurnCodecError::Incomplete)
        }
    }

    fn push_openai(&mut self, value: &Value) -> Result<Vec<ProviderTurnEvent>, ToolTurnCodecError> {
        match value.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => {
                let delta = value
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or(ToolTurnCodecError::MalformedEvent)?;
                if delta.is_empty() {
                    Ok(Vec::new())
                } else {
                    Ok(vec![ProviderTurnEvent::MessageDelta {
                        delta: delta.to_owned(),
                    }])
                }
            }
            Some("response.output_item.added") => {
                let item = value
                    .get("item")
                    .filter(|item| {
                        item.get("type").and_then(Value::as_str) == Some("function_call")
                    })
                    .ok_or(ToolTurnCodecError::MalformedEvent)?;
                let key = openai_item_key(item).ok_or(ToolTurnCodecError::InvalidHandle)?;
                let handle = item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .ok_or(ToolTurnCodecError::InvalidHandle)?;
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or(ToolTurnCodecError::InvalidName)?;
                let arguments = item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                self.register(key, handle, name, arguments)?;
                Ok(Vec::new())
            }
            Some("response.function_call_arguments.delta") => {
                let key = self.openai_event_key(value)?;
                let delta = value
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or(ToolTurnCodecError::MalformedEvent)?;
                self.append_arguments(&key, delta)?;
                Ok(Vec::new())
            }
            Some("response.function_call_arguments.done") => {
                let key = self.openai_event_key(value)?;
                if let Some(name) = value.get("name").and_then(Value::as_str) {
                    let pending = self
                        .pending
                        .get(&key)
                        .ok_or(ToolTurnCodecError::InvalidSequence)?;
                    if pending.name != name {
                        return Err(ToolTurnCodecError::InvalidSequence);
                    }
                }
                if let Some(arguments) = value.get("arguments").and_then(Value::as_str) {
                    let pending = self
                        .pending
                        .get_mut(&key)
                        .ok_or(ToolTurnCodecError::InvalidSequence)?;
                    if !pending.arguments.is_empty()
                        && !arguments.starts_with(pending.arguments.as_str())
                    {
                        return Err(ToolTurnCodecError::InvalidSequence);
                    }
                    ensure_argument_bound(arguments)?;
                    pending.arguments = arguments.to_owned();
                }
                Ok(vec![ProviderTurnEvent::ToolCall {
                    call: self.complete_call(&key)?,
                }])
            }
            Some("response.output_item.done") => {
                let item = value
                    .get("item")
                    .filter(|item| {
                        item.get("type").and_then(Value::as_str) == Some("function_call")
                    })
                    .ok_or(ToolTurnCodecError::MalformedEvent)?;
                let key = openai_item_key(item).ok_or(ToolTurnCodecError::InvalidHandle)?;
                if self.completed.contains(&key) {
                    return Ok(Vec::new());
                }
                let handle = item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .ok_or(ToolTurnCodecError::InvalidHandle)?;
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or(ToolTurnCodecError::InvalidName)?;
                let arguments = item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .ok_or(ToolTurnCodecError::MalformedEvent)?;
                if let Some(pending) = self.pending.get_mut(&key) {
                    if pending.handle != handle || pending.name != name {
                        return Err(ToolTurnCodecError::InvalidSequence);
                    }
                    if pending.arguments.is_empty() {
                        ensure_argument_bound(arguments)?;
                        pending.arguments = arguments.to_owned();
                    } else if pending.arguments != arguments {
                        return Err(ToolTurnCodecError::InvalidSequence);
                    }
                } else {
                    self.register(key.clone(), handle, name, arguments)?;
                }
                Ok(vec![ProviderTurnEvent::ToolCall {
                    call: self.complete_call(&key)?,
                }])
            }
            Some("response.completed") => self.complete_turn(Some("completed".to_owned())),
            Some("response.failed") | Some("error") => Err(ToolTurnCodecError::MalformedEvent),
            Some(_) | None => Ok(Vec::new()),
        }
    }

    fn push_anthropic(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ProviderTurnEvent>, ToolTurnCodecError> {
        match value.get("type").and_then(Value::as_str) {
            Some("content_block_start")
                if value.pointer("/content_block/type").and_then(Value::as_str)
                    == Some("tool_use") =>
            {
                let index = required_index(value)?;
                let block = value
                    .get("content_block")
                    .ok_or(ToolTurnCodecError::MalformedEvent)?;
                let handle = block
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or(ToolTurnCodecError::InvalidHandle)?;
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or(ToolTurnCodecError::InvalidName)?;
                let arguments = match block.get("input") {
                    Some(Value::Object(object)) if !object.is_empty() => {
                        serde_json::to_string(object)
                            .map_err(|_| ToolTurnCodecError::MalformedEvent)?
                    }
                    Some(Value::Object(_)) | None => String::new(),
                    Some(_) => return Err(ToolTurnCodecError::MalformedEvent),
                };
                self.register(index_key(index), handle, name, &arguments)?;
                Ok(Vec::new())
            }
            Some("content_block_delta")
                if value.pointer("/delta/type").and_then(Value::as_str)
                    == Some("input_json_delta") =>
            {
                let index = required_index(value)?;
                let delta = value
                    .pointer("/delta/partial_json")
                    .and_then(Value::as_str)
                    .ok_or(ToolTurnCodecError::MalformedEvent)?;
                self.append_arguments(&index_key(index), delta)?;
                Ok(Vec::new())
            }
            Some("content_block_delta")
                if value.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta") =>
            {
                let delta = value
                    .pointer("/delta/text")
                    .and_then(Value::as_str)
                    .ok_or(ToolTurnCodecError::MalformedEvent)?;
                if delta.is_empty() {
                    Ok(Vec::new())
                } else {
                    Ok(vec![ProviderTurnEvent::MessageDelta {
                        delta: delta.to_owned(),
                    }])
                }
            }
            Some("content_block_stop") => {
                let key = index_key(required_index(value)?);
                if self.pending.contains_key(&key) {
                    Ok(vec![ProviderTurnEvent::ToolCall {
                        call: self.complete_call(&key)?,
                    }])
                } else {
                    Ok(Vec::new())
                }
            }
            Some("message_delta") => {
                self.finish_reason = value
                    .pointer("/delta/stop_reason")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                Ok(Vec::new())
            }
            Some("message_stop") => self.complete_turn(self.finish_reason.clone()),
            Some("error") => Err(ToolTurnCodecError::MalformedEvent),
            Some(_) | None => Ok(Vec::new()),
        }
    }

    fn push_compatible(
        &mut self,
        value: &Value,
    ) -> Result<Vec<ProviderTurnEvent>, ToolTurnCodecError> {
        let Some(choice) = value.pointer("/choices/0") else {
            return Ok(Vec::new());
        };
        let mut events = Vec::new();
        if let Some(delta) = choice.pointer("/delta/content").and_then(Value::as_str) {
            if !delta.is_empty() {
                events.push(ProviderTurnEvent::MessageDelta {
                    delta: delta.to_owned(),
                });
            }
        }

        if let Some(calls) = choice
            .pointer("/delta/tool_calls")
            .and_then(Value::as_array)
        {
            for call in calls {
                let index = call
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or(ToolTurnCodecError::MalformedEvent)?;
                let key = index_key(index);
                if !self.pending.contains_key(&key) {
                    let handle = call
                        .get("id")
                        .and_then(Value::as_str)
                        .ok_or(ToolTurnCodecError::InvalidHandle)?;
                    self.register(key.clone(), handle, "", "")?;
                } else if let Some(handle) = call.get("id").and_then(Value::as_str) {
                    let pending = self
                        .pending
                        .get(&key)
                        .ok_or(ToolTurnCodecError::InvalidSequence)?;
                    if pending.handle != handle {
                        return Err(ToolTurnCodecError::InvalidSequence);
                    }
                }

                if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
                    self.append_name(&key, name)?;
                }
                if let Some(arguments) = call.pointer("/function/arguments").and_then(Value::as_str)
                {
                    self.append_arguments(&key, arguments)?;
                }
            }
        }

        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = Some(reason.to_owned());
            if reason == "tool_calls" {
                for call in self.complete_all()? {
                    events.push(ProviderTurnEvent::ToolCall { call });
                }
            } else if !self.pending.is_empty() {
                return Err(ToolTurnCodecError::InvalidSequence);
            }
            events.extend(self.complete_turn(Some(reason.to_owned()))?);
        }
        Ok(events)
    }

    fn register(
        &mut self,
        key: String,
        handle: &str,
        name: &str,
        arguments: &str,
    ) -> Result<(), ToolTurnCodecError> {
        validate_handle(handle)?;
        ensure_argument_bound(arguments)?;
        if !name.is_empty() {
            validate_name(name)?;
        }
        if self.pending.contains_key(&key)
            || self.completed.contains(&key)
            || self.order.len() >= MAX_TOOL_CALLS_PER_TURN
        {
            return if self.order.len() >= MAX_TOOL_CALLS_PER_TURN {
                Err(ToolTurnCodecError::TooManyCalls)
            } else {
                Err(ToolTurnCodecError::InvalidSequence)
            };
        }
        self.pending.insert(
            key.clone(),
            PendingToolCall {
                handle: handle.to_owned(),
                name: name.to_owned(),
                arguments: arguments.to_owned(),
            },
        );
        self.order.push(key);
        Ok(())
    }

    fn append_name(&mut self, key: &str, delta: &str) -> Result<(), ToolTurnCodecError> {
        let pending = self
            .pending
            .get_mut(key)
            .ok_or(ToolTurnCodecError::InvalidSequence)?;
        pending.name.push_str(delta);
        if pending.name.chars().count() > MAX_TOOL_NAME_CHARS {
            return Err(ToolTurnCodecError::InvalidName);
        }
        Ok(())
    }

    fn append_arguments(&mut self, key: &str, delta: &str) -> Result<(), ToolTurnCodecError> {
        let pending = self
            .pending
            .get_mut(key)
            .ok_or(ToolTurnCodecError::InvalidSequence)?;
        if pending.arguments.len().saturating_add(delta.len()) > MAX_TOOL_ARGUMENT_BYTES {
            return Err(ToolTurnCodecError::ArgumentsTooLarge);
        }
        pending.arguments.push_str(delta);
        Ok(())
    }

    fn complete_call(&mut self, key: &str) -> Result<ProviderToolCall, ToolTurnCodecError> {
        let pending = self
            .pending
            .remove(key)
            .ok_or(ToolTurnCodecError::InvalidSequence)?;
        validate_handle(&pending.handle)?;
        validate_name(&pending.name)?;
        ensure_argument_bound(&pending.arguments)?;
        if pending.arguments.trim().is_empty() {
            return Err(ToolTurnCodecError::MalformedEvent);
        }
        self.completed.insert(key.to_owned());
        Ok(ProviderToolCall {
            handle: ProviderToolCallHandle(pending.handle),
            name: pending.name,
            arguments: pending.arguments,
        })
    }

    fn complete_all(&mut self) -> Result<Vec<ProviderToolCall>, ToolTurnCodecError> {
        let keys = self
            .order
            .iter()
            .filter(|key| self.pending.contains_key(*key))
            .cloned()
            .collect::<Vec<_>>();
        keys.iter().map(|key| self.complete_call(key)).collect()
    }

    fn complete_turn(
        &mut self,
        finish_reason: Option<String>,
    ) -> Result<Vec<ProviderTurnEvent>, ToolTurnCodecError> {
        if self.terminal || !self.pending.is_empty() {
            return Err(ToolTurnCodecError::InvalidSequence);
        }
        if self.protocol == ProviderProtocol::AnthropicMessages
            && ((!self.completed.is_empty() && finish_reason.as_deref() != Some("tool_use"))
                || (self.completed.is_empty() && finish_reason.as_deref() == Some("tool_use")))
        {
            return Err(ToolTurnCodecError::InvalidSequence);
        }
        self.terminal = true;
        Ok(vec![ProviderTurnEvent::Completed { finish_reason }])
    }

    fn openai_event_key(&self, value: &Value) -> Result<String, ToolTurnCodecError> {
        if let Some(key) = value
            .get("item_id")
            .or_else(|| value.get("call_id"))
            .and_then(Value::as_str)
        {
            return Ok(key.to_owned());
        }
        if self.pending.len() == 1 {
            return self
                .pending
                .keys()
                .next()
                .cloned()
                .ok_or(ToolTurnCodecError::InvalidSequence);
        }
        Err(ToolTurnCodecError::InvalidSequence)
    }
}

pub fn repository_tool_catalog(protocol: ProviderProtocol) -> Value {
    Value::Array(
        repository_tool_definitions()
            .into_iter()
            .map(|definition| match protocol {
                ProviderProtocol::OpenAiResponses => json!({
                    "type": "function",
                    "name": definition.name,
                    "description": definition.description,
                    "parameters": definition.input_schema,
                    "strict": true
                }),
                ProviderProtocol::AnthropicMessages => json!({
                    "name": definition.name,
                    "description": definition.description,
                    "input_schema": definition.input_schema
                }),
                ProviderProtocol::OpenAiChatCompletions => json!({
                    "type": "function",
                    "function": {
                        "name": definition.name,
                        "description": definition.description,
                        "parameters": definition.input_schema,
                        "strict": true
                    }
                }),
            })
            .collect(),
    )
}

pub fn encode_tool_outcome(
    protocol: ProviderProtocol,
    call: &ProviderToolCall,
    outcome: &RepositoryToolOutcome,
) -> Result<Value, ToolTurnCodecError> {
    let content =
        serde_json::to_string(outcome).map_err(|_| ToolTurnCodecError::ContinuationEncoding)?;
    let handle = &call.handle.0;
    Ok(match protocol {
        ProviderProtocol::OpenAiResponses => json!({
            "type": "function_call_output",
            "call_id": handle,
            "output": content
        }),
        ProviderProtocol::AnthropicMessages => json!({
            "type": "tool_result",
            "tool_use_id": handle,
            "content": content,
            "is_error": outcome.is_failure()
        }),
        ProviderProtocol::OpenAiChatCompletions => json!({
            "role": "tool",
            "tool_call_id": handle,
            "content": content
        }),
    })
}

fn openai_item_key(item: &Value) -> Option<String> {
    item.get("id")
        .or_else(|| item.get("call_id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn required_index(value: &Value) -> Result<u64, ToolTurnCodecError> {
    value
        .get("index")
        .and_then(Value::as_u64)
        .ok_or(ToolTurnCodecError::MalformedEvent)
}

fn index_key(index: u64) -> String {
    format!("index:{index}")
}

fn validate_handle(handle: &str) -> Result<(), ToolTurnCodecError> {
    if handle.trim().is_empty()
        || handle.chars().count() > MAX_PROVIDER_TOOL_HANDLE_CHARS
        || handle.chars().any(char::is_control)
    {
        Err(ToolTurnCodecError::InvalidHandle)
    } else {
        Ok(())
    }
}

fn validate_name(name: &str) -> Result<(), ToolTurnCodecError> {
    if name.is_empty()
        || name.chars().count() > MAX_TOOL_NAME_CHARS
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        Err(ToolTurnCodecError::InvalidName)
    } else {
        Ok(())
    }
}

fn ensure_argument_bound(arguments: &str) -> Result<(), ToolTurnCodecError> {
    if arguments.len() > MAX_TOOL_ARGUMENT_BYTES {
        Err(ToolTurnCodecError::ArgumentsTooLarge)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use kiln_core::{
        ProviderProtocol, RepositoryToolFailureCode, RepositoryToolOutcome, RepositoryToolResult,
        SearchFilesResult,
    };
    use serde_json::json;

    use super::*;

    fn decode(
        protocol: ProviderProtocol,
        values: Vec<Value>,
    ) -> Result<Vec<ProviderToolCall>, ToolTurnCodecError> {
        let mut codec = ToolTurnCodec::new(protocol);
        let mut calls = Vec::new();
        for value in values {
            for event in codec.push(&value.to_string())? {
                if let ProviderTurnEvent::ToolCall { call } = event {
                    calls.push(call);
                }
            }
        }
        codec.finish()?;
        Ok(calls)
    }

    #[test]
    fn fragmented_cross_provider_streams_produce_the_same_repository_request() {
        let openai = decode(
            ProviderProtocol::OpenAiResponses,
            vec![
                json!({
                    "type": "response.output_item.added",
                    "item": {
                        "type": "function_call",
                        "id": "fc_1",
                        "call_id": "call_1",
                        "name": "read_file",
                        "arguments": ""
                    }
                }),
                json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": "fc_1",
                    "delta": "{\"path\":\"src/"
                }),
                json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": "fc_1",
                    "arguments": "{\"path\":\"src/lib.rs\"}"
                }),
                json!({"type": "response.completed", "response": {}}),
            ],
        )
        .unwrap();
        let anthropic = decode(
            ProviderProtocol::AnthropicMessages,
            vec![
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_1",
                        "name": "read_file",
                        "input": {}
                    }
                }),
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": "{\"path\":\"src/"
                    }
                }),
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": "lib.rs\"}"
                    }
                }),
                json!({"type": "content_block_stop", "index": 0}),
                json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "tool_use"}
                }),
                json!({"type": "message_stop"}),
            ],
        )
        .unwrap();
        let compatible = decode(
            ProviderProtocol::OpenAiChatCompletions,
            vec![
                json!({
                    "choices": [{
                        "delta": {
                            "tool_calls": [{
                                "index": 0,
                                "id": "call_1",
                                "function": {
                                    "name": "read_file",
                                    "arguments": "{\"path\":\"src/"
                                }
                            }]
                        },
                        "finish_reason": null
                    }]
                }),
                json!({
                    "choices": [{
                        "delta": {
                            "tool_calls": [{
                                "index": 0,
                                "function": {"arguments": "lib.rs\"}"}
                            }]
                        },
                        "finish_reason": "tool_calls"
                    }]
                }),
            ],
        )
        .unwrap();

        let expected = openai[0].repository_request().unwrap();
        assert_eq!(anthropic[0].repository_request().unwrap(), expected);
        assert_eq!(compatible[0].repository_request().unwrap(), expected);
    }

    #[test]
    fn tool_only_turns_complete_without_message_text() {
        let calls = decode(
            ProviderProtocol::OpenAiChatCompletions,
            vec![json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_1",
                            "function": {
                                "name": "search_files",
                                "arguments": "{\"pattern\":\"*.rs\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            })],
        )
        .unwrap();
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn catalogs_and_denied_continuations_are_protocol_specific() {
        for protocol in [
            ProviderProtocol::OpenAiResponses,
            ProviderProtocol::AnthropicMessages,
            ProviderProtocol::OpenAiChatCompletions,
        ] {
            assert_eq!(
                repository_tool_catalog(protocol).as_array().map(Vec::len),
                Some(4)
            );
        }

        let call = ProviderToolCall {
            handle: ProviderToolCallHandle("call_1".to_owned()),
            name: "search_files".to_owned(),
            arguments: "{\"pattern\":\"*.rs\"}".to_owned(),
        };
        let denied = RepositoryToolOutcome::failure(
            RepositoryToolFailureCode::Denied,
            "Policy denied this repository action.",
        )
        .unwrap();
        let anthropic =
            encode_tool_outcome(ProviderProtocol::AnthropicMessages, &call, &denied).unwrap();
        assert_eq!(anthropic["tool_use_id"], "call_1");
        assert_eq!(anthropic["is_error"], true);

        let success =
            RepositoryToolOutcome::success(RepositoryToolResult::SearchFiles(SearchFilesResult {
                pattern: "*.rs".to_owned(),
                matches: Vec::new(),
                truncated: false,
            }));
        let openai =
            encode_tool_outcome(ProviderProtocol::OpenAiResponses, &call, &success).unwrap();
        assert_eq!(openai["type"], "function_call_output");
    }

    #[test]
    fn malformed_unknown_and_oversized_calls_fail_locally() {
        let call = ProviderToolCall {
            handle: ProviderToolCallHandle("call_1".to_owned()),
            name: "shell".to_owned(),
            arguments: "{\"command\":\"whoami\"}".to_owned(),
        };
        assert!(call.repository_request().is_err());

        let mut codec = ToolTurnCodec::new(ProviderProtocol::OpenAiChatCompletions);
        let oversized = "x".repeat(MAX_TOOL_ARGUMENT_BYTES + 1);
        let event = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "function": {
                            "name": "read_file",
                            "arguments": oversized
                        }
                    }]
                },
                "finish_reason": null
            }]
        });
        assert_eq!(
            codec.push(&event.to_string()),
            Err(ToolTurnCodecError::ArgumentsTooLarge)
        );
    }

    #[test]
    fn missing_duplicate_and_out_of_order_events_fail_closed() {
        let mut compatible = ToolTurnCodec::new(ProviderProtocol::OpenAiChatCompletions);
        let missing_handle = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"path\":\"src/lib.rs\"}"
                        }
                    }]
                },
                "finish_reason": null
            }]
        });
        assert_eq!(
            compatible.push(&missing_handle.to_string()),
            Err(ToolTurnCodecError::InvalidHandle)
        );

        let mut openai = ToolTurnCodec::new(ProviderProtocol::OpenAiResponses);
        let out_of_order = json!({
            "type": "response.function_call_arguments.delta",
            "item_id": "fc_missing",
            "delta": "{}"
        });
        assert_eq!(
            openai.push(&out_of_order.to_string()),
            Err(ToolTurnCodecError::InvalidSequence)
        );

        let mut malformed = ToolTurnCodec::new(ProviderProtocol::AnthropicMessages);
        assert_eq!(
            malformed.push("not-json"),
            Err(ToolTurnCodecError::MalformedEvent)
        );

        let mut duplicate = ToolTurnCodec::new(ProviderProtocol::OpenAiResponses);
        let added = json!({
            "type": "response.output_item.added",
            "item": {
                "type": "function_call",
                "id": "fc_duplicate",
                "call_id": "call_duplicate",
                "name": "read_file",
                "arguments": "{\"path\":\"src/lib.rs\"}"
            }
        });
        duplicate.push(&added.to_string()).unwrap();
        assert_eq!(
            duplicate.push(&added.to_string()),
            Err(ToolTurnCodecError::InvalidSequence)
        );
    }

    #[test]
    fn pending_codec_debug_output_redacts_provider_payloads() {
        let mut codec = ToolTurnCodec::new(ProviderProtocol::OpenAiResponses);
        codec
            .push(
                &json!({
                    "type": "response.output_item.added",
                    "item": {
                        "type": "function_call",
                        "id": "fc_sensitive",
                        "call_id": "call_sensitive",
                        "name": "write_file",
                        "arguments": "{\"path\":\"private.txt\",\"content\":\"sensitive\"}"
                    }
                })
                .to_string(),
            )
            .unwrap();

        let debug = format!("{codec:?}");
        assert!(!debug.contains("call_sensitive"));
        assert!(!debug.contains("private.txt"));
        assert!(!debug.contains("sensitive"));
        assert!(debug.contains("pending_calls: 1"));
    }

    #[test]
    fn anthropic_tool_calls_require_a_tool_use_terminal_reason() {
        let result = decode(
            ProviderProtocol::AnthropicMessages,
            vec![
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_wrong_stop",
                        "name": "read_file",
                        "input": {"path": "src/lib.rs"}
                    }
                }),
                json!({"type": "content_block_stop", "index": 0}),
                json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "end_turn"}
                }),
                json!({"type": "message_stop"}),
            ],
        );

        assert_eq!(result.unwrap_err(), ToolTurnCodecError::InvalidSequence);
    }

    #[test]
    fn tool_call_count_is_bounded_before_execution() {
        let mut codec = ToolTurnCodec::new(ProviderProtocol::OpenAiChatCompletions);
        for index in 0..MAX_TOOL_CALLS_PER_TURN {
            let event = json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": index,
                            "id": format!("call_{index}"),
                            "function": {
                                "name": "read_file",
                                "arguments": "{\"path\":\"src/lib.rs\"}"
                            }
                        }]
                    },
                    "finish_reason": null
                }]
            });
            codec.push(&event.to_string()).unwrap();
        }
        let overflow = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": MAX_TOOL_CALLS_PER_TURN,
                        "id": "call_overflow",
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"path\":\"src/lib.rs\"}"
                        }
                    }]
                },
                "finish_reason": null
            }]
        });
        assert_eq!(
            codec.push(&overflow.to_string()),
            Err(ToolTurnCodecError::TooManyCalls)
        );
    }
}
