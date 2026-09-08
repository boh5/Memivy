//! Bounded OpenAI-compatible requests. This module cannot mutate memories.
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    #[serde(default)]
    pub disable_reasoning: bool,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ProbeError {
    #[error("模型配置无法读取或权限不是 0600")]
    Configuration,
    #[error("端点无效：远程必须 HTTPS，本地允许 loopback HTTP")]
    Endpoint,
    #[error("模型请求超时或连接失败")]
    Network,
    #[error("模型 HTTP 状态异常：{0}")]
    Status(u16),
    #[error("模型响应超过 64 KB")]
    TooLarge,
    #[error("模型响应不符合约定的完整 JSON")]
    InvalidResponse,
}

#[derive(Serialize, Debug)]
pub struct ProbeReport {
    pub endpoint_kind: &'static str,
    pub elapsed_ms: f64,
    pub valid: bool,
}

impl ModelConfig {
    pub fn endpoint(&self) -> Result<(reqwest::Url, bool), ProbeError> {
        let mut url =
            reqwest::Url::parse(self.base_url.trim()).map_err(|_| ProbeError::Endpoint)?;
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
        url.set_path(&format!(
            "{}/chat/completions",
            url.path().trim_end_matches('/')
        ));
        Ok((url, local))
    }

    pub fn save(&self, path: &Path) -> Result<(), ProbeError> {
        use std::io::Write;
        self.endpoint()?;
        let parent = path.parent().ok_or(ProbeError::Configuration)?;
        fs::create_dir_all(parent).map_err(|_| ProbeError::Configuration)?;
        let mut file =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| ProbeError::Configuration)?;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| ProbeError::Configuration)?;
        file.write_all(&serde_json::to_vec(self).map_err(|_| ProbeError::Configuration)?)
            .map_err(|_| ProbeError::Configuration)?;
        file.as_file()
            .sync_all()
            .map_err(|_| ProbeError::Configuration)?;
        file.persist(path).map_err(|_| ProbeError::Configuration)?;
        Ok(())
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

/// Two bounded calls per question: query planning, then a structured answer.
pub async fn complete(
    config: &ModelConfig,
    messages: serde_json::Value,
    name: &str,
    schema: serde_json::Value,
) -> Result<serde_json::Value, ProbeError> {
    let choice = request(config, messages, json!({
        "response_format":{"type":"json_schema","json_schema":{"name":name,"strict":true,"schema":schema}}
    })).await?;
    if choice["finish_reason"] != "stop" {
        return Err(ProbeError::InvalidResponse);
    }
    serde_json::from_str(
        choice["message"]["content"]
            .as_str()
            .ok_or(ProbeError::InvalidResponse)?,
    )
    .map_err(|_| ProbeError::InvalidResponse)
}

/// A single proposed call; only the domain layer may validate and execute it.
pub struct FunctionCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

pub async fn call_function(
    config: &ModelConfig,
    messages: serde_json::Value,
    tools: serde_json::Value,
) -> Result<FunctionCall, ProbeError> {
    let choice = request(
        config,
        messages,
        json!({
            "tools":tools,"tool_choice":"required","parallel_tool_calls":false
        }),
    )
    .await?;
    if choice["finish_reason"] != "tool_calls" {
        return Err(ProbeError::InvalidResponse);
    }
    let calls = choice["message"]["tool_calls"]
        .as_array()
        .ok_or(ProbeError::InvalidResponse)?;
    if calls.len() != 1 || calls[0]["type"] != "function" {
        return Err(ProbeError::InvalidResponse);
    }
    let f = &calls[0]["function"];
    Ok(FunctionCall {
        name: f["name"]
            .as_str()
            .ok_or(ProbeError::InvalidResponse)?
            .into(),
        arguments: serde_json::from_str(
            f["arguments"].as_str().ok_or(ProbeError::InvalidResponse)?,
        )
        .map_err(|_| ProbeError::InvalidResponse)?,
    })
}

async fn request(
    config: &ModelConfig,
    messages: serde_json::Value,
    options: serde_json::Value,
) -> Result<serde_json::Value, ProbeError> {
    let (url, _) = config.endpoint()?;
    // Reuse the HTTP connection pool. Credentials remain per request, never defaults.
    static CLIENT: std::sync::OnceLock<Result<reqwest::Client, reqwest::Error>> =
        std::sync::OnceLock::new();
    let client = CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(90))
                .redirect(reqwest::redirect::Policy::none())
                .build()
        })
        .as_ref()
        .map_err(|_| ProbeError::Network)?;
    let mut body = json!({"model":config.model,"messages":messages,"stream":false,
        "temperature":0.2,"max_tokens":2500});
    body.as_object_mut()
        .unwrap()
        .extend(options.as_object().unwrap().clone());
    if config.disable_reasoning {
        body["reasoning_effort"] = json!("none");
    }
    let mut request = client.post(url).json(&body);
    if let Some(key) = config.api_key.as_ref().filter(|s| !s.is_empty()) {
        request = request.bearer_auth(key);
    }
    let mut response = request.send().await.map_err(|_| ProbeError::Network)?;
    if !response.status().is_success() {
        return Err(ProbeError::Status(response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ProbeError::Network)? {
        if bytes.len() + chunk.len() > 65_536 {
            return Err(ProbeError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    let mut response: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| ProbeError::InvalidResponse)?;
    let choices = response["choices"]
        .as_array_mut()
        .ok_or(ProbeError::InvalidResponse)?;
    if choices.len() != 1 {
        return Err(ProbeError::InvalidResponse);
    }
    let choice = choices.remove(0);
    if choice["message"]["refusal"]
        .as_str()
        .is_some_and(|s| !s.is_empty())
    {
        return Err(ProbeError::InvalidResponse);
    }
    Ok(choice)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    ok: bool,
    echo: String,
}

pub async fn probe(config: ModelConfig, timeout: Duration) -> Result<ProbeReport, ProbeError> {
    let mut base = reqwest::Url::parse(&config.base_url).map_err(|_| ProbeError::Endpoint)?;
    let local = matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(base.scheme() == "https" || (local && base.scheme() == "http"))
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
        || config.model.trim().is_empty()
        || config.model.len() > 200
    {
        return Err(ProbeError::Endpoint);
    }
    base.set_path(&format!(
        "{}/chat/completions",
        base.path().trim_end_matches('/')
    ));
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ProbeError::Network)?;
    let mut body = json!({
        "model": config.model,
        "messages": [{"role":"user","content":"Return exactly this JSON object, without markdown: {\"ok\":true,\"echo\":\"先留住原话\"}"}],
        "stream": false,
        "response_format": {"type":"json_schema","json_schema": {
            "name":"memivy_probe", "strict":true,
            "schema":{"type":"object","properties":{"ok":{"type":"boolean"},"echo":{"type":"string"}},"required":["ok","echo"],"additionalProperties":false}
        }}
    });
    let start = Instant::now();
    if config.disable_reasoning {
        body["reasoning_effort"] = json!("none");
    }
    let mut request = client.post(base).json(&body);
    if let Some(key) = config.api_key.filter(|key| !key.is_empty()) {
        request = request.bearer_auth(key);
    }
    let mut response = request.send().await.map_err(|_| ProbeError::Network)?;
    if !response.status().is_success() {
        return Err(ProbeError::Status(response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ProbeError::Network)? {
        if bytes.len() + chunk.len() > 65_536 {
            return Err(ProbeError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| ProbeError::InvalidResponse)?;
    let choice = &value["choices"][0];
    if choice["finish_reason"] != "stop"
        || choice["message"]["refusal"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    {
        return Err(ProbeError::InvalidResponse);
    }
    let answer: Answer = serde_json::from_str(
        choice["message"]["content"]
            .as_str()
            .ok_or(ProbeError::InvalidResponse)?,
    )
    .map_err(|_| ProbeError::InvalidResponse)?;
    if !answer.ok || answer.echo != "先留住原话" {
        return Err(ProbeError::InvalidResponse);
    }
    Ok(ProbeReport {
        endpoint_kind: if local { "local" } else { "remote" },
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
        valid: true,
    })
}
