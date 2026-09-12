//! User-owned connections and capability bindings. Secrets never enter the memory database.
use crate::{
    embedding,
    model::{ModelConfig, OutputTokenParameter},
};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, os::unix::fs::PermissionsExt, path::Path, time::Duration};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    ConfigurationRead,
    ConfigurationInvalid,
    ConfigurationSave,
    ConfigurationLock,
    Conflict,
    ConfigurationTooLarge,
    ConnectionMissing,
    LocalBinding,
    ModelRequired,
    ConnectionLimit,
    ConnectionInvalid,
    QueryPrefixTooLong,
    DimensionsRequired,
    Endpoint,
    Network,
    Authentication,
    RateLimit,
    HttpStatus,
    ResponseTooLarge,
    InvalidResponse,
    InputInvalid,
    DimensionsMismatch,
    AudioInvalid,
}
impl Error {
    pub const fn code(self) -> &'static str {
        match self {
            Self::ConfigurationRead => "model_configuration_read",
            Self::ConfigurationInvalid => "model_configuration_invalid",
            Self::ConfigurationSave => "model_configuration_save",
            Self::ConfigurationLock => "model_configuration_lock",
            Self::Conflict => "configuration_conflict",
            Self::ConfigurationTooLarge => "model_configuration_too_large",
            Self::ConnectionMissing => "model_connection_missing",
            Self::LocalBinding => "model_local_binding",
            Self::ModelRequired => "model_required",
            Self::ConnectionLimit => "model_connection_limit",
            Self::ConnectionInvalid => "model_connection_invalid",
            Self::QueryPrefixTooLong => "model_query_prefix_too_long",
            Self::DimensionsRequired => "model_dimensions_required",
            Self::Endpoint => "model_endpoint",
            Self::Network => "model_network",
            Self::Authentication => "model_authentication",
            Self::RateLimit => "model_rate_limit",
            Self::HttpStatus => "model_status",
            Self::ResponseTooLarge => "model_too_large",
            Self::InvalidResponse => "model_invalid_response",
            Self::InputInvalid => "model_input_invalid",
            Self::DimensionsMismatch => "model_dimensions_mismatch",
            Self::AudioInvalid => "voice_audio_invalid",
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for Error {}
impl From<Error> for String {
    fn from(value: Error) -> Self {
        value.code().into()
    }
}
impl From<crate::model::ProbeError> for Error {
    fn from(value: crate::model::ProbeError) -> Self {
        use crate::model::ProbeError;
        match value {
            ProbeError::Configuration => Self::ConfigurationInvalid,
            ProbeError::Endpoint => Self::Endpoint,
            ProbeError::Network => Self::Network,
            ProbeError::Status(401 | 403) => Self::Authentication,
            ProbeError::Status(429) => Self::RateLimit,
            ProbeError::Status(_) => Self::HttpStatus,
            ProbeError::TooLarge => Self::ResponseTooLarge,
            ProbeError::InvalidResponse | ProbeError::Truncated | ProbeError::ToolsUnsupported => {
                Self::InvalidResponse
            }
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;
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
        let m = fs::symlink_metadata(&p).map_err(|_| Error::ConfigurationRead)?;
        if !m.is_file() || m.permissions().mode() & 0o777 != 0o600 || m.len() > 256 * 1024 {
            return Err(Error::ConfigurationInvalid);
        }
        serde_json::from_slice(&fs::read(p).map_err(|_| Error::ConfigurationRead)?)
            .map_err(|_| Error::ConfigurationInvalid)
    }
    pub fn with_legacy(root: &Path, legacy: &Path) -> Result<Self> {
        let mut r = Self::read(root)?;
        if !Self::exists(root) && legacy.exists() {
            let m = ModelConfig::read(legacy).map_err(Error::from)?;
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
        let _lock = embedding::lock(root, "models.lock").map_err(|_| Error::ConfigurationLock)?;
        if Self::read(root)?.revision != expected {
            return Err(Error::Conflict);
        }
        self.validate()?;
        if serde_json::to_vec(self)
            .map_err(|_| Error::ConfigurationSave)?
            .len()
            > 250 * 1024
        {
            return Err(Error::ConfigurationTooLarge);
        }
        self.revision = uuid::Uuid::new_v4().to_string();
        embedding::write_json(&root.join(FILE), self).map_err(|_| Error::ConfigurationSave)
    }
    pub fn connection(&self, id: &str) -> Result<&Connection> {
        self.connections
            .iter()
            .find(|s| s.id == id)
            .ok_or(Error::ConnectionMissing)
    }
    pub fn resolve(&self, b: &Binding) -> Result<ModelConfig> {
        if b.source != Source::Service {
            return Err(Error::LocalBinding);
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
        m.endpoint().map_err(Error::from)?;
        Ok(m)
    }
    pub fn llm_config(&self) -> Result<ModelConfig> {
        self.resolve(self.llm.as_ref().ok_or(Error::ModelRequired)?)
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
            return Err(Error::ConnectionLimit);
        }
        let mut ids = std::collections::HashSet::new();
        for c in &self.connections {
            if c.id.is_empty()
                || c.id.len() > 64
                || !ids.insert(&c.id)
                || c.name.trim().is_empty()
                || c.name.len() > 200
            {
                return Err(Error::ConnectionInvalid);
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
                return Err(Error::QueryPrefixTooLong);
            }
        }
        if self.embedding.source == Source::Service
            && !self
                .embedding
                .dimensions
                .is_some_and(|n| (1..=16384).contains(&n))
        {
            return Err(Error::DimensionsRequired);
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
        .map_err(|_| Error::Network)
}
fn endpoint(m: &ModelConfig, path: &str) -> Result<String> {
    m.endpoint().map_err(Error::from)?;
    Ok(format!("{}/{}", m.base_url.trim_end_matches('/'), path))
}
fn response(r: reqwest::blocking::Response, limit: usize) -> Result<serde_json::Value> {
    if !r.status().is_success() {
        return Err(match r.status().as_u16() {
            401 | 403 => Error::Authentication,
            429 => Error::RateLimit,
            _ => Error::HttpStatus,
        });
    }
    let mut bytes = Vec::new();
    r.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Network)?;
    if bytes.len() > limit {
        return Err(Error::ResponseTooLarge);
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::InvalidResponse)
}
pub fn embed(
    m: &ModelConfig,
    text: &str,
    dimensions: Option<usize>,
    timeout: Duration,
) -> Result<Vec<f32>> {
    if text.is_empty() || text.chars().count() > 4000 {
        return Err(Error::InputInvalid);
    }
    let mut req = client(timeout)?
        .post(endpoint(m, "embeddings")?)
        .json(&serde_json::json!({"model":m.model,"input":[text],"encoding_format":"float"}));
    if let Some(key) = &m.api_key {
        req = req.bearer_auth(key)
    }
    let value = response(req.send().map_err(|_| Error::Network)?, 1024 * 1024)?;
    let data = value["data"]
        .as_array()
        .filter(|a| a.len() == 1)
        .ok_or(Error::InvalidResponse)?;
    if data[0]["index"].as_u64() != Some(0) {
        return Err(Error::InvalidResponse);
    }
    let mut v: Vec<f32> =
        serde_json::from_value(data[0]["embedding"].clone()).map_err(|_| Error::InvalidResponse)?;
    if v.is_empty()
        || v.len() > 16384
        || dimensions.is_some_and(|n| n != v.len())
        || v.iter().any(|x| !x.is_finite())
    {
        return Err(Error::DimensionsMismatch);
    }
    let norm = v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    if norm <= 0. || !norm.is_finite() {
        return Err(Error::InvalidResponse);
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
        return Err(Error::AudioInvalid);
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
                .map_err(|_| Error::AudioInvalid)?,
        );
    let mut req = client(Duration::from_secs(60))?
        .post(endpoint(m, "audio/transcriptions")?)
        .multipart(form);
    if let Some(key) = &m.api_key {
        req = req.bearer_auth(key)
    }
    let v = response(req.send().map_err(|_| Error::Network)?, 64 * 1024)?;
    v["text"]
        .as_str()
        .filter(|t| t.len() <= 32000)
        .map(str::to_owned)
        .ok_or(Error::InvalidResponse)
}
