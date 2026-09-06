//! A synthetic protocol probe only. It has no database or memory mutation access.
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
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
    let body = json!({
        "model": config.model,
        "messages": [{"role":"user","content":"Return exactly this JSON object, without markdown: {\"ok\":true,\"echo\":\"先留住原话\"}"}],
        "stream": false,
        "response_format": {"type":"json_schema","json_schema": {
            "name":"memivy_probe", "strict":true,
            "schema":{"type":"object","properties":{"ok":{"type":"boolean"},"echo":{"type":"string"}},"required":["ok","echo"],"additionalProperties":false}
        }}
    });
    let start = Instant::now();
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
