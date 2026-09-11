//! User-owned connections and capability bindings. Secrets never enter the memory database.
use crate::{
    embedding,
    model::{ModelConfig, OutputTokenParameter},
};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, os::unix::fs::PermissionsExt, path::Path, time::Duration};
pub type Result<T> = std::result::Result<T, String>;
const FILE: &str = "models.json";
#[derive(Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
}
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    Local,
    Service,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Binding {
    pub source: Source,
    pub connection: String,
    pub model: String,
    pub dimensions: Option<usize>,
    pub query_prefix: String,
    pub disable_reasoning: bool,
    pub max_output_tokens: Option<u32>,
    pub output_token_parameter: OutputTokenParameter,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Registry {
    pub revision: String,
    pub connections: Vec<Connection>,
    pub llm: Option<Binding>,
    pub embedding: Binding,
    pub voice: Binding,
    pub auto_organize: bool,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            revision: "initial".into(),
            connections: vec![],
            llm: None,
            embedding: Binding::default(),
            voice: Binding::default(),
            auto_organize: true,
        }
    }
}
impl Registry {
    pub fn exists(root: &Path) -> bool {
        root.join(FILE).exists()
    }
    pub fn read(root: &Path) -> Result<Self> {
        let p = root.join(FILE);
        if !p.exists() {
            return Ok(Self::default());
        }
        let m = fs::symlink_metadata(&p).map_err(|_| "模型配置无法读取")?;
        if !m.is_file() || m.permissions().mode() & 0o777 != 0o600 || m.len() > 256 * 1024 {
            return Err("模型配置无效或权限不是 0600".into());
        }
        serde_json::from_slice(&fs::read(p).map_err(|_| "模型配置无法读取")?)
            .map_err(|_| "模型配置无法读取，请检查配置文件".into())
    }
    pub fn with_legacy(root: &Path, legacy: &Path) -> Result<Self> {
        let mut r = Self::read(root)?;
        if !Self::exists(root) && legacy.exists() {
            let m = ModelConfig::read(legacy).map_err(|e| e.to_string())?;
            r.connections.push(Connection {
                id: "existing".into(),
                name: "已有模型连接".into(),
                base_url: m.base_url,
                api_key: m.api_key,
            });
            r.llm = Some(Binding {
                source: Source::Service,
                connection: "existing".into(),
                model: m.model,
                disable_reasoning: m.disable_reasoning,
                max_output_tokens: m.max_output_tokens,
                output_token_parameter: m.output_token_parameter,
                ..Binding::default()
            });
        }
        Ok(r)
    }
    pub fn save(&mut self, root: &Path, expected: &str) -> Result<()> {
        let _lock = embedding::lock(root, "models.lock")?;
        if Self::read(root)?.revision != expected {
            return Err("模型设置已在其他位置修改，请重新打开设置".into());
        }
        self.validate()?;
        if serde_json::to_vec(self)
            .map_err(|_| "模型配置无法保存")?
            .len()
            > 250 * 1024
        {
            return Err("模型配置过大，请检查地址、模型名称和 Key".into());
        }
        self.revision = uuid::Uuid::new_v4().to_string();
        embedding::write_json(&root.join(FILE), self)
    }
    pub fn connection(&self, id: &str) -> Result<&Connection> {
        self.connections
            .iter()
            .find(|s| s.id == id)
            .ok_or("模型连接不存在".into())
    }
    pub fn resolve(&self, b: &Binding) -> Result<ModelConfig> {
        if b.source != Source::Service {
            return Err("当前使用内置本地模型".into());
        }
        let c = self.connection(&b.connection)?;
        let m = ModelConfig {
            base_url: c.base_url.clone(),
            model: b.model.clone(),
            api_key: c.api_key.clone(),
            disable_reasoning: b.disable_reasoning,
            max_output_tokens: b.max_output_tokens,
            output_token_parameter: b.output_token_parameter,
        };
        m.endpoint().map_err(|e| e.to_string())?;
        Ok(m)
    }
    pub fn llm_config(&self) -> Result<ModelConfig> {
        self.resolve(self.llm.as_ref().ok_or("请先配置问答模型")?)
    }
    pub fn fingerprint(&self) -> Result<String> {
        if self.embedding.source == Source::Local {
            return Ok(embedding::fingerprint());
        }
        let m = self.resolve(&self.embedding)?;
        Ok(embedding::hash(
            format!(
                "api-v1:{}:{}:{:?}:{}:nfc-l2-chunk-v1",
                m.base_url.trim_end_matches('/'),
                m.model,
                self.embedding.dimensions,
                self.embedding.query_prefix
            )
            .as_bytes(),
        ))
    }
    fn validate(&self) -> Result<()> {
        if self.connections.len() > 32 {
            return Err("最多保存 32 个连接".into());
        }
        let mut ids = std::collections::HashSet::new();
        for c in &self.connections {
            if c.id.is_empty()
                || c.id.len() > 64
                || !ids.insert(&c.id)
                || c.name.trim().is_empty()
                || c.name.len() > 200
            {
                return Err("连接名称或标识无效".into());
            }
            let b = Binding {
                source: Source::Service,
                connection: c.id.clone(),
                model: "probe".into(),
                ..Binding::default()
            };
            self.resolve(&b)?;
        }
        for b in [self.llm.as_ref(), Some(&self.embedding), Some(&self.voice)]
            .into_iter()
            .flatten()
        {
            if b.source == Source::Service {
                self.resolve(b)?;
            }
            if b.query_prefix.chars().count() > 500 {
                return Err("检索指令过长".into());
            }
        }
        if self.embedding.source == Source::Service
            && !self
                .embedding
                .dimensions
                .is_some_and(|n| (1..=16384).contains(&n))
        {
            return Err("请先测试 Embedding 模型，确认向量维度".into());
        }
        Ok(())
    }
}
/// Shared bounded HTTP transport. Call only from blocking worker threads.
fn client(timeout: Duration) -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(5).min(timeout))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "模型连接初始化失败".into())
}
fn endpoint(m: &ModelConfig, path: &str) -> Result<String> {
    m.endpoint().map_err(|e| e.to_string())?;
    Ok(format!("{}/{}", m.base_url.trim_end_matches('/'), path))
}
fn response(r: reqwest::blocking::Response, limit: usize) -> Result<serde_json::Value> {
    if !r.status().is_success() {
        return Err(match r.status().as_u16() {
            401 | 403 => "模型认证失败，请检查 Key 和权限".into(),
            429 => "模型服务限流，请稍后重试".into(),
            n => format!("模型服务返回 HTTP {n}，请检查地址和模型名称"),
        });
    }
    let mut bytes = Vec::new();
    r.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "模型响应中断")?;
    if bytes.len() > limit {
        return Err("模型响应超过大小限制".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "模型返回的格式无效".into())
}
pub fn embed(
    m: &ModelConfig,
    text: &str,
    dimensions: Option<usize>,
    timeout: Duration,
) -> Result<Vec<f32>> {
    if text.is_empty() || text.chars().count() > 4000 {
        return Err("待编码文本长度无效".into());
    }
    let mut req = client(timeout)?
        .post(endpoint(m, "embeddings")?)
        .json(&serde_json::json!({"model":m.model,"input":[text],"encoding_format":"float"}));
    if let Some(key) = &m.api_key {
        req = req.bearer_auth(key)
    }
    let value = response(
        req.send().map_err(|_| "语义模型连接失败或超时")?,
        1024 * 1024,
    )?;
    let data = value["data"]
        .as_array()
        .filter(|a| a.len() == 1)
        .ok_or("语义模型返回条数无效")?;
    if data[0]["index"].as_u64() != Some(0) {
        return Err("语义模型返回顺序无效".into());
    }
    let mut v: Vec<f32> = serde_json::from_value(data[0]["embedding"].clone())
        .map_err(|_| "语义模型未返回有效向量")?;
    if v.is_empty()
        || v.len() > 16384
        || dimensions.is_some_and(|n| n != v.len())
        || v.iter().any(|x| !x.is_finite())
    {
        return Err("向量维度或数值不匹配，请重新测试模型并重建索引".into());
    }
    let norm = v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    if norm <= 0. || !norm.is_finite() {
        return Err("语义模型返回了无效向量".into());
    }
    for x in &mut v {
        *x = (*x as f64 / norm) as f32
    }
    Ok(v)
}
pub fn bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|v| v.to_le_bytes()).collect()
}
pub fn transcribe(m: &ModelConfig, samples: &[f32]) -> Result<String> {
    if samples.is_empty() || samples.len() > 16000 * 20 || samples.iter().any(|n| !n.is_finite()) {
        return Err("录音片段无效".into());
    }
    let pcm: Vec<u8> = samples
        .iter()
        .flat_map(|s| ((s.clamp(-1., 1.) * 32767.) as i16).to_le_bytes())
        .collect();
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend((36 + pcm.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(16000u32.to_le_bytes());
    wav.extend(32000u32.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((pcm.len() as u32).to_le_bytes());
    wav.extend(pcm);
    let form = reqwest::blocking::multipart::Form::new()
        .text("model", m.model.clone())
        .text("response_format", "json")
        .part(
            "file",
            reqwest::blocking::multipart::Part::bytes(wav)
                .file_name("segment.wav")
                .mime_str("audio/wav")
                .map_err(|_| "音频格式无效")?,
        );
    let mut req = client(Duration::from_secs(60))?
        .post(endpoint(m, "audio/transcriptions")?)
        .multipart(form);
    if let Some(key) = &m.api_key {
        req = req.bearer_auth(key)
    }
    let v = response(
        req.send()
            .map_err(|_| "语音服务连接失败或超时，录音已保留")?,
        64 * 1024,
    )?;
    v["text"]
        .as_str()
        .filter(|t| t.len() <= 32000)
        .map(str::to_owned)
        .ok_or("语音服务未返回有效文字".into())
}
