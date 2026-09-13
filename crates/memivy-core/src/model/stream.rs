//! One streamed model turn. The memory runtime owns the loop and every effect.
use super::*;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum StreamDelta {
    Text(String),
    /// Partial arguments are for progress/checkpoints only, never execution.
    ToolArguments {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamTurn {
    /// Complete provider assistant message, ready to persist before tool effects.
    pub message: Value,
    pub text: String,
    pub calls: Vec<ToolCall>,
}

/// A callback error aborts this request immediately; dropping this future also
/// drops the HTTP response. No tool is ever executed by this transport layer.
pub async fn stream_turn(
    config: &ModelConfig,
    messages: &[Value],
    tools: &[Value],
    mut on_delta: impl FnMut(StreamDelta) -> Result<(), ProbeError>,
) -> Result<StreamTurn, ProbeError> {
    let (url, _) = config.endpoint()?;
    let mut options = json!({"stream":true});
    if !tools.is_empty() {
        options["tools"] = json!(tools);
        options["tool_choice"] = json!("auto");
        options["parallel_tool_calls"] = json!(false);
    }
    let body = request_body(config, json!(messages), options);
    if serde_json::to_vec(&body)
        .map_err(|_| ProbeError::InvalidResponse)?
        .len()
        > 1_048_576
    {
        return Err(ProbeError::TooLarge);
    }
    let mut request = http_client()?
        .post(url)
        .timeout(OutputPolicy::FullText.timeout())
        .json(&body);
    if let Some(key) = config.api_key.as_ref().filter(|s| !s.is_empty()) {
        request = request.bearer_auth(key);
    }
    let mut response = request.send().await.map_err(|_| ProbeError::Network)?;
    if !response.status().is_success() {
        return Err(ProbeError::Status(response.status().as_u16()));
    }
    if !response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|s| s.split(';').next() == Some("text/event-stream"))
    {
        return Err(ProbeError::ToolsUnsupported);
    }
    let mut parser = StreamParser::default();
    let mut bytes = Vec::new();
    let mut data = String::new();
    let mut total = 0usize;
    while let Some(chunk) = response.chunk().await.map_err(|_| ProbeError::Network)? {
        total = total.saturating_add(chunk.len());
        if total > OutputPolicy::FullText.bytes() {
            return Err(ProbeError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
        while let Some(end) = bytes.iter().position(|b| *b == b'\n') {
            let line = bytes.drain(..=end).collect::<Vec<_>>();
            let line = std::str::from_utf8(&line[..line.len() - 1])
                .map_err(|_| ProbeError::InvalidResponse)?
                .trim_end_matches('\r');
            if line.is_empty() {
                if !data.is_empty() {
                    parser.event(data.trim_end_matches('\n'), &mut on_delta)?;
                    data.clear();
                    if parser.done {
                        return parser.finish(tools);
                    }
                }
            } else if let Some(value) = line.strip_prefix("data:") {
                data.push_str(value.strip_prefix(' ').unwrap_or(value));
                data.push('\n');
            }
        }
    }
    // A dropped connection after partial text/tool arguments is not completion.
    Err(ProbeError::InvalidResponse)
}

struct StreamParser {
    message: Value,
    tools: BTreeMap<usize, Value>,
    finish_reason: Option<String>,
    done: bool,
}
impl Default for StreamParser {
    fn default() -> Self {
        Self {
            message: json!({"role":"assistant","content":""}),
            tools: BTreeMap::new(),
            finish_reason: None,
            done: false,
        }
    }
}
impl StreamParser {
    fn event(
        &mut self,
        data: &str,
        on_delta: &mut impl FnMut(StreamDelta) -> Result<(), ProbeError>,
    ) -> Result<(), ProbeError> {
        if data == "[DONE]" {
            if self.finish_reason.is_none() {
                return Err(ProbeError::InvalidResponse);
            }
            self.done = true;
            return Ok(());
        }
        let value: Value = serde_json::from_str(data).map_err(|_| ProbeError::InvalidResponse)?;
        let choices = value["choices"]
            .as_array()
            .ok_or(ProbeError::InvalidResponse)?;
        if choices.is_empty() && value.get("usage").is_some() {
            return Ok(());
        }
        if choices.len() != 1 || choices[0]["index"].as_u64().is_some_and(|n| n != 0) {
            return Err(ProbeError::InvalidResponse);
        }
        let choice = &choices[0];
        let delta = choice["delta"]
            .as_object()
            .ok_or(ProbeError::InvalidResponse)?;
        if self.finish_reason.is_some() && !delta.is_empty() {
            return Err(ProbeError::InvalidResponse);
        }
        if delta.get("role").is_some_and(|r| r != "assistant")
            || delta
                .get("refusal")
                .is_some_and(|v| !v.is_null() && v != "")
        {
            return Err(ProbeError::InvalidResponse);
        }
        if let Some(content) = delta.get("content").filter(|v| !v.is_null()) {
            let text = content.as_str().ok_or(ProbeError::InvalidResponse)?;
            append_string(&mut self.message["content"], text)?;
            if !text.is_empty() {
                on_delta(StreamDelta::Text(text.into()))?;
            }
        }
        if let Some(calls) = delta.get("tool_calls").filter(|v| !v.is_null()) {
            for item in calls.as_array().ok_or(ProbeError::InvalidResponse)? {
                let index = item["index"].as_u64().ok_or(ProbeError::InvalidResponse)? as usize;
                if index >= 8 {
                    return Err(ProbeError::InvalidResponse);
                }
                let target = self.tools.entry(index).or_insert_with(
                    || json!({"id":"","type":"function","function":{"name":"","arguments":""}}),
                );
                if item.get("type").is_some_and(|t| t != "function") {
                    return Err(ProbeError::InvalidResponse);
                }
                let id = optional_string(item.get("id"))?;
                let name = optional_string(item.get("function").and_then(|f| f.get("name")))?;
                let arguments =
                    optional_string(item.get("function").and_then(|f| f.get("arguments")))?
                        .unwrap_or_default();
                if let Some(id) = &id {
                    append_string(&mut target["id"], id)?;
                }
                if let Some(name) = &name {
                    append_string(&mut target["function"]["name"], name)?;
                }
                append_string(&mut target["function"]["arguments"], &arguments)?;
                for (key, value) in item.as_object().ok_or(ProbeError::InvalidResponse)? {
                    if !matches!(key.as_str(), "index" | "id" | "type" | "function") {
                        merge_protocol_delta(&mut target[key], value)?;
                    }
                }
                if let Some(fields) = item.get("function").and_then(Value::as_object) {
                    for (key, value) in fields {
                        if !matches!(key.as_str(), "name" | "arguments") {
                            merge_protocol_delta(&mut target["function"][key], value)?;
                        }
                    }
                }
                on_delta(StreamDelta::ToolArguments {
                    index,
                    id,
                    name,
                    arguments,
                })?;
            }
        }
        // Reasoning fields required by some providers must survive a tool turn.
        // They stay in the protocol message, never in user-visible text events.
        for (key, value) in delta {
            if !matches!(key.as_str(), "role" | "content" | "tool_calls" | "refusal") {
                merge_protocol_delta(&mut self.message[key], value)?;
            }
        }
        if let Some(reason) = choice.get("finish_reason").filter(|v| !v.is_null()) {
            let reason = reason.as_str().ok_or(ProbeError::InvalidResponse)?;
            if self.finish_reason.is_some() {
                return Err(ProbeError::InvalidResponse);
            }
            self.finish_reason = Some(reason.into());
        }
        Ok(())
    }

    fn finish(mut self, available: &[Value]) -> Result<StreamTurn, ProbeError> {
        let text = self.message["content"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        match self.finish_reason.as_deref() {
            Some("length") => return Err(ProbeError::Truncated),
            Some("stop") if self.tools.is_empty() && !text.trim().is_empty() => {}
            Some("tool_calls") if !self.tools.is_empty() => {}
            _ => return Err(ProbeError::InvalidResponse),
        }
        let mut ids = HashSet::new();
        let mut calls = Vec::new();
        for (expected, (index, raw)) in self.tools.iter().enumerate() {
            let id = raw["id"].as_str().ok_or(ProbeError::InvalidResponse)?;
            let name = raw["function"]["name"]
                .as_str()
                .ok_or(ProbeError::InvalidResponse)?;
            let arguments: Value = serde_json::from_str(
                raw["function"]["arguments"]
                    .as_str()
                    .ok_or(ProbeError::InvalidResponse)?,
            )
            .map_err(|_| ProbeError::InvalidResponse)?;
            if expected != *index
                || id.is_empty()
                || id.len() > 128
                || name.is_empty()
                || name.len() > 80
                || !arguments.is_object()
                || !ids.insert(id)
                || !available.iter().any(|t| t["function"]["name"] == name)
            {
                return Err(ProbeError::InvalidResponse);
            }
            calls.push(ToolCall {
                id: id.into(),
                name: name.into(),
                arguments,
            });
        }
        if !calls.is_empty() {
            self.message["tool_calls"] = json!(self.tools.into_values().collect::<Vec<_>>());
        }
        Ok(StreamTurn {
            message: self.message,
            text,
            calls,
        })
    }
}

fn optional_string(value: Option<&Value>) -> Result<Option<String>, ProbeError> {
    value
        .filter(|v| !v.is_null())
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or(ProbeError::InvalidResponse)
        })
        .transpose()
}
fn append_string(target: &mut Value, text: &str) -> Result<(), ProbeError> {
    if target.is_null() {
        *target = json!("");
    }
    let old = target.as_str().ok_or(ProbeError::InvalidResponse)?;
    *target = json!(format!("{old}{text}"));
    Ok(())
}

/// Preserve bounded provider extensions needed when sending the assistant turn
/// back after a tool result. They are opaque to the memory runtime and UI.
fn merge_protocol_delta(target: &mut Value, delta: &Value) -> Result<(), ProbeError> {
    if delta.is_null() {
        return Ok(());
    }
    if target.is_null() {
        *target = delta.clone();
        return Ok(());
    }
    match (target, delta) {
        (Value::String(target), Value::String(delta)) => target.push_str(delta),
        (Value::Array(target), Value::Array(delta)) => target.extend(delta.iter().cloned()),
        (Value::Object(target), Value::Object(delta)) => {
            for (key, value) in delta {
                merge_protocol_delta(target.entry(key.clone()).or_insert(Value::Null), value)?;
            }
        }
        (target, delta) if target == delta => {}
        _ => return Err(ProbeError::InvalidResponse),
    }
    Ok(())
}
