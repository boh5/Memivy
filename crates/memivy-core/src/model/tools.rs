//! Function descriptions and observed protocol capabilities. Tool effects and
//! the conversation loop belong to the memory runtime, never the transport.
use super::*;
use serde_json::Value;

pub use super::ToolCall;
pub use super::stream::{StreamDelta, stream_turn};

pub fn function(name: &str, description: &str, properties: Value) -> ToolDefinition {
    let required: Vec<_> = properties.as_object().unwrap().keys().cloned().collect();
    ToolDefinition {
        name: name.into(),
        description: description.into(),
        parameters: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    pub structured_json: bool,
    pub streaming_text: bool,
    pub single_tool: bool,
    pub multi_turn: bool,
}
impl Capabilities {
    pub fn supports_agent(&self) -> bool {
        self.streaming_text && self.multi_turn
    }
}
fn supported<T>(result: Result<T, ProbeError>) -> Result<bool, ProbeError> {
    match result {
        Ok(_) => Ok(true),
        Err(
            ProbeError::InvalidResponse
            | ProbeError::ToolsUnsupported
            | ProbeError::Status(400 | 422),
        ) => Ok(false),
        Err(e) => Err(e),
    }
}
async fn probe_turn(
    config: &ModelConfig,
    messages: &[Message],
    tools: &[ToolDefinition],
) -> Result<CompletionResponse, ProbeError> {
    tokio::time::timeout(
        Duration::from_secs(45),
        stream_turn(config, messages, tools, |_| Ok(())),
    )
    .await
    .map_err(|_| ProbeError::Network)?
}
fn exact_tool(turn: &CompletionResponse, name: &str, arguments: Value) -> bool {
    let calls: Vec<_> = calls(turn).collect();
    calls.len() == 1 && calls[0].function.name == name && calls[0].function.arguments == arguments
}
/// Probe the same natural streaming protocol used by the Agent: read a random
/// synthetic value, pass it to another tool, then finish with ordinary text.
/// This diagnostic does not save settings or mutate memory.
pub async fn probe(config: &ModelConfig) -> Result<Capabilities, ProbeError> {
    let structured_json = supported(super::probe(config.clone(), Duration::from_secs(45)).await)?;
    let mut result = Capabilities {
        structured_json,
        streaming_text: false,
        single_tool: false,
        multi_turn: false,
    };
    let text_probe = probe_turn(config, &[crate::model::Message::user("Reply exactly MEMIVY-STREAM-READY as ordinary text. This is a synthetic protocol test.")], &[]).await;
    result.streaming_text = supported(text_probe.and_then(|t| {
        if text(&t).trim() == "MEMIVY-STREAM-READY" && calls(&t).next().is_none() {
            Ok(())
        } else {
            Err(ProbeError::InvalidResponse)
        }
    }))?;
    if !result.streaming_text {
        return Ok(result);
    }
    let tools = vec![
        function(
            "probe_lookup",
            "Read a synthetic protocol value",
            json!({"key":{"type":"string"}}),
        ),
        function(
            "probe_echo",
            "Echo exactly the value returned by probe_lookup",
            json!({"value":{"type":"string"}}),
        ),
    ];
    let mut messages = vec![crate::model::Message::user(
        "Synthetic protocol test. First call probe_lookup with key synthetic. Then call probe_echo with the exact value returned by probe_lookup. Finally reply with that value as ordinary text, without further tool calls. Do not invent the value.",
    )];
    let first = match probe_turn(config, &messages, &tools).await {
        Ok(t) if exact_tool(&t, "probe_lookup", json!({"key":"synthetic"})) => t,
        Ok(_) => return Ok(result),
        Err(e) => {
            supported::<()>(Err(e))?;
            return Ok(result);
        }
    };
    result.single_tool = true;
    let value = format!("MEMIVY-SYNTHETIC-{}", uuid::Uuid::new_v4());
    let reply = tool_result(
        calls(&first).next().ok_or(ProbeError::InvalidResponse)?,
        &json!({"value":value}),
    );
    messages.push(assistant(first));
    messages.push(reply);
    let second = match probe_turn(config, &messages, &tools).await {
        Ok(t) if exact_tool(&t, "probe_echo", json!({"value":value})) => t,
        Ok(_) => return Ok(result),
        Err(e) => {
            supported::<()>(Err(e))?;
            return Ok(result);
        }
    };
    let reply = tool_result(
        calls(&second).next().ok_or(ProbeError::InvalidResponse)?,
        &json!({"value":value}),
    );
    messages.push(assistant(second));
    messages.push(reply);
    result.multi_turn = supported(probe_turn(config, &messages, &tools).await.and_then(|t| {
        if calls(&t).next().is_none() && text(&t).contains(&value) {
            Ok(())
        } else {
            Err(ProbeError::InvalidResponse)
        }
    }))?;
    Ok(result)
}
