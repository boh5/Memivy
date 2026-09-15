//! Bounded Rig completions. This module cannot mutate memories.
const PROBE_ECHO: &str = "先留住原话";
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};

mod stream;
pub(crate) mod transport;
pub use rig_core::{
    completion::{CompletionResponse, Message, ToolDefinition},
    message::{AssistantContent, ToolCall, ToolResultContent, UserContent},
};
pub mod tools;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    #[serde(default)]
    pub provider: Provider,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub output_token_parameter: OutputTokenParameter,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    #[serde(default)]
    pub disable_reasoning: bool,
}

/// The provider protocol; compatible gateways may use arbitrary model IDs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    OpenaiCompatible,
    OpenaiResponses,
    Anthropic,
    Gemini,
}

/// BYOM endpoints do not all support the same token parameter.
#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputTokenParameter {
    #[default]
    MaxTokens,
    MaxCompletionTokens,
}
#[derive(Clone, Copy)]
pub enum OutputPolicy {
    Structured,
    FullText,
}
impl OutputPolicy {
    fn bytes(self) -> usize {
        match self {
            Self::Structured => 65_536,
            Self::FullText => 1_048_576,
        }
    }
    fn timeout(self) -> Duration {
        Duration::from_secs(match self {
            Self::Structured => 90,
            Self::FullText => 180,
        })
    }
}

#[derive(Clone, Debug, thiserror::Error, PartialEq)]
pub enum ProbeError {
    #[error("Cannot read model configuration or its permissions are not 0600")]
    Configuration,
    #[error("Invalid endpoint: remote services require HTTPS; loopback HTTP is allowed")]
    Endpoint,
    #[error("Model request timed out or failed to connect")]
    Network,
    #[error("Unexpected model HTTP status: {0}")]
    Status(u16),
    #[error("Model response exceeds the size limit for this task")]
    TooLarge,
    #[error("Model response does not satisfy the required protocol")]
    InvalidResponse,
    #[error(
        "Model output was truncated; increase the output limit or use a model with a longer output capacity"
    )]
    Truncated,
    #[error(
        "The model does not support the required tools; test the connection or choose another model"
    )]
    ToolsUnsupported,
}

#[derive(Serialize, Debug)]
pub struct ProbeReport {
    pub endpoint_kind: &'static str,
    pub elapsed_ms: f64,
    pub valid: bool,
}

impl ModelConfig {
    pub fn endpoint(&self) -> Result<(reqwest::Url, bool), ProbeError> {
        if self
            .max_output_tokens
            .is_some_and(|n| n == 0 || n > 1_048_576)
        {
            return Err(ProbeError::Configuration);
        }
        let url = reqwest::Url::parse(self.base_url.trim()).map_err(|_| ProbeError::Endpoint)?;
        let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if !(url.scheme() == "https" || (local && url.scheme() == "http"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || self.model.trim().is_empty()
            || self.model.len() > 200
            || self.api_key.as_ref().is_some_and(|key| key.len() > 8192)
        {
            return Err(ProbeError::Endpoint);
        }
        Ok((url, local))
    }

    pub fn save(&self, path: &Path) -> Result<(), ProbeError> {
        self.endpoint()?;
        save_private_json(path, self)
    }
    pub fn read(path: &Path) -> Result<Self, ProbeError> {
        let metadata = fs::symlink_metadata(path).map_err(|_| ProbeError::Configuration)?;
        if !metadata.is_file()
            || metadata.permissions().mode() & 0o777 != 0o600
            || metadata.len() > 16_384
        {
            return Err(ProbeError::Configuration);
        }
        serde_json::from_slice(&fs::read(path).map_err(|_| ProbeError::Configuration)?)
            .map_err(|_| ProbeError::Configuration)
    }
}

fn save_private_json(path: &Path, value: &impl Serialize) -> Result<(), ProbeError> {
    use std::io::Write;
    let parent = path.parent().ok_or(ProbeError::Configuration)?;
    fs::create_dir_all(parent).map_err(|_| ProbeError::Configuration)?;
    let mut file =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| ProbeError::Configuration)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|_| ProbeError::Configuration)?;
    file.write_all(&serde_json::to_vec(value).map_err(|_| ProbeError::Configuration)?)
        .map_err(|_| ProbeError::Configuration)?;
    file.as_file()
        .sync_all()
        .map_err(|_| ProbeError::Configuration)?;
    file.persist(path).map_err(|_| ProbeError::Configuration)?;
    Ok(())
}

/// Structured output for bounded tasks such as cleanup and collection queries.
pub async fn complete(
    config: &ModelConfig,
    messages: Vec<Message>,
    name: &str,
    schema: serde_json::Value,
) -> Result<serde_json::Value, ProbeError> {
    complete_with_policy(config, messages, name, schema, OutputPolicy::Structured).await
}
pub async fn complete_with_policy(
    config: &ModelConfig,
    messages: Vec<Message>,
    name: &str,
    mut schema: serde_json::Value,
    policy: OutputPolicy,
) -> Result<serde_json::Value, ProbeError> {
    schema["title"] = json!(name);
    let response = stream::complete(config, messages, schema, policy).await?;
    if calls(&response).next().is_some() {
        return Err(ProbeError::InvalidResponse);
    }
    serde_json::from_str(&text(&response)).map_err(|_| ProbeError::InvalidResponse)
}

pub fn text(response: &CompletionResponse) -> String {
    response
        .choice
        .iter()
        .filter_map(|part| match part {
            AssistantContent::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}
pub fn calls(response: &CompletionResponse) -> impl Iterator<Item = &ToolCall> {
    response.choice.iter().filter_map(|part| match part {
        AssistantContent::ToolCall(call) => Some(call),
        _ => None,
    })
}
pub fn assistant(response: CompletionResponse) -> Message {
    Message::Assistant {
        id: response.message_id,
        content: response.choice,
    }
}
pub fn tool_result(call: &ToolCall, value: &serde_json::Value) -> Message {
    Message::User {
        content: vec![UserContent::tool_result_for(
            call.id.clone(),
            call.provider.clone(),
            call.function.name.clone(),
            vec![ToolResultContent::text(value.to_string())],
        )],
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    ok: bool,
    echo: String,
}

pub async fn probe(config: ModelConfig, timeout: Duration) -> Result<ProbeReport, ProbeError> {
    let (_, local) = config.endpoint()?;
    let start = Instant::now();
    let value = tokio::time::timeout(timeout, complete(&config,
        vec![Message::user(format!("Return exactly this JSON object, without markdown: {}", json!({"ok":true,"echo":PROBE_ECHO})))],
        "memivy_probe", json!({"type":"object","properties":{"ok":{"type":"boolean"},"echo":{"type":"string"}},"required":["ok","echo"],"additionalProperties":false}),
    )).await.map_err(|_| ProbeError::Network)??;
    let answer: Answer = serde_json::from_value(value).map_err(|_| ProbeError::InvalidResponse)?;
    if !answer.ok || answer.echo != PROBE_ECHO {
        return Err(ProbeError::InvalidResponse);
    }
    Ok(ProbeReport {
        endpoint_kind: if local { "local" } else { "remote" },
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
        valid: true,
    })
}
