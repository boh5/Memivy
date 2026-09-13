//! Function descriptions and observed protocol capabilities. Tool effects and
//! the conversation loop belong to the memory runtime, never the transport.
use super::*;
use serde_json::Value;

pub use super::stream::{StreamDelta, StreamTurn, ToolCall, stream_turn};

pub fn function(name: &str, description: &str, properties: Value) -> Value {
    let required: Vec<_> = properties.as_object().unwrap().keys().cloned().collect();
    json!({"type":"function","function":{"name":name,"description":description,"strict":true,
        "parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}})
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
#[derive(Serialize, Deserialize)]
struct Cached {
    fingerprint: String,
    capabilities: Capabilities,
}
fn fingerprint(config: &ModelConfig) -> Result<String, ProbeError> {
    use sha2::{Digest, Sha256};
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(config).map_err(|_| ProbeError::Configuration)?)
    ))
}
pub fn cached(root: &Path, config: &ModelConfig) -> Option<Capabilities> {
    // Missing streaming capability is unverified, not compatible by assumption.
    let data: Cached =
        serde_json::from_slice(&fs::read(root.join("model-capabilities.json")).ok()?).ok()?;
    (data.fingerprint == fingerprint(config).ok()?).then_some(data.capabilities)
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
    messages: &[Value],
    tools: &[Value],
) -> Result<StreamTurn, ProbeError> {
    tokio::time::timeout(
        Duration::from_secs(45),
        stream_turn(config, messages, tools, |_| Ok(())),
    )
    .await
    .map_err(|_| ProbeError::Network)?
}
fn exact_tool(turn: &StreamTurn, name: &str, arguments: Value) -> bool {
    turn.calls.len() == 1 && turn.calls[0].name == name && turn.calls[0].arguments == arguments
}
/// Probe the same natural streaming protocol used by the Agent: read a random
/// synthetic value, pass it to another tool, then finish with ordinary text.
/// This does not mutate either memory or the current capability cache.
pub async fn probe(config: &ModelConfig) -> Result<Capabilities, ProbeError> {
    let structured_json = supported(super::probe(config.clone(), Duration::from_secs(45)).await)?;
    let mut result = Capabilities {
        structured_json,
        streaming_text: false,
        single_tool: false,
        multi_turn: false,
    };
    let text = probe_turn(config, &[json!({"role":"user","content":"Reply exactly MEMIVY-STREAM-READY as ordinary text. This is a synthetic protocol test."})], &[]).await;
    result.streaming_text = supported(text.and_then(|t| {
        if t.text.trim() == "MEMIVY-STREAM-READY" && t.calls.is_empty() {
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
    let mut messages = vec![
        json!({"role":"user","content":"Synthetic protocol test. First call probe_lookup with key synthetic. Then call probe_echo with the exact value returned by probe_lookup. Finally reply with that value as ordinary text, without further tool calls. Do not invent the value."}),
    ];
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
    messages.push(first.message);
    messages.push(json!({"role":"tool","tool_call_id":first.calls[0].id,"content":json!({"value":value}).to_string()}));
    let second = match probe_turn(config, &messages, &tools).await {
        Ok(t) if exact_tool(&t, "probe_echo", json!({"value":value})) => t,
        Ok(_) => return Ok(result),
        Err(e) => {
            supported::<()>(Err(e))?;
            return Ok(result);
        }
    };
    messages.push(second.message);
    messages.push(json!({"role":"tool","tool_call_id":second.calls[0].id,"content":json!({"value":value}).to_string()}));
    result.multi_turn = supported(probe_turn(config, &messages, &tools).await.and_then(|t| {
        if t.calls.is_empty() && t.text.contains(&value) {
            Ok(())
        } else {
            Err(ProbeError::InvalidResponse)
        }
    }))?;
    Ok(result)
}
pub fn save_capabilities(
    root: &Path,
    config: &ModelConfig,
    capabilities: &Capabilities,
) -> Result<(), ProbeError> {
    save_private_json(
        &root.join("model-capabilities.json"),
        &Cached {
            fingerprint: fingerprint(config)?,
            capabilities: capabilities.clone(),
        },
    )
}
pub async fn probe_and_save(root: &Path, config: &ModelConfig) -> Result<Capabilities, ProbeError> {
    let capabilities = probe(config).await?;
    save_capabilities(root, config, &capabilities)?;
    Ok(capabilities)
}
