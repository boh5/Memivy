//! Local encoding infrastructure. It owns neither Memory rules nor search scope.
pub mod cache;
pub mod chunk;
pub mod client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub const DIMENSIONS: usize = 1024;
pub const MAX_CHARS: usize = 1000;
#[derive(Clone, Debug, Serialize)]
pub struct ModelDescriptor {
    pub repo: &'static str,
    pub revision: &'static str,
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}
pub const MODEL: ModelDescriptor = ModelDescriptor {
    repo: "Qwen/Qwen3-Embedding-0.6B-GGUF",
    revision: "370f27d7550e0def9b39c1f16d3fbaa13aa67728",
    file: "Qwen3-Embedding-0.6B-Q8_0.gguf",
    bytes: 639150592,
    sha256: "06507c7b42688469c4e7298b0a1e16deff06caf291cf0a5b278c308249c3e439",
};
pub fn fingerprint() -> String {
    hash(
        format!(
            "{}:qwen-last-eos151643-no-bos-nfc-l2-1024-ctx8192:chunk-v1-1000-200-100",
            MODEL.sha256
        )
        .as_bytes(),
    )
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub type Result<T> = std::result::Result<T, String>;
pub(crate) fn io(_: impl std::fmt::Display) -> String {
    "model_cache_io".into()
}
pub fn private_dir(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err("model_cache_path".into());
    }
    fs::create_dir_all(path).map_err(io)?;
    if !fs::symlink_metadata(path).map_err(io)?.is_dir() {
        return Err("model_cache_path".into());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io)
}
pub fn lock(root: &Path, name: &str) -> Result<File> {
    private_dir(root)?;
    let p = root.join(name);
    if fs::symlink_metadata(&p).is_ok_and(|m| !m.is_file()) {
        return Err("embedding_busy".into());
    }
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(p)
        .map_err(io)?;
    f.try_lock().map_err(|_| "embedding_busy".to_string())?;
    Ok(f)
}
pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut f =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("model_cache_path")?).map_err(io)?;
    serde_json::to_writer(&mut f, value).map_err(io)?;
    f.flush().map_err(io)?;
    f.as_file().sync_all().map_err(io)?;
    f.persist(path).map_err(io)?;
    File::open(path.parent().unwrap())
        .and_then(|f| f.sync_all())
        .map_err(io)
}
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub enabled: bool,
    pub preparing: bool,
    pub paused: bool,
    pub clear_requested: bool,
    pub error: Option<String>,
}
impl Preferences {
    pub fn read(root: &Path) -> Result<Self> {
        let p = root.join("embedding.json");
        if !p.exists() {
            return Ok(Self::default());
        }
        if fs::metadata(&p).map_err(io)?.len() > 4096 {
            return Err("embedding_settings_invalid".into());
        }
        serde_json::from_slice(&fs::read(p).map_err(io)?)
            .map_err(|_| "embedding_settings_invalid".into())
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        write_json(&root.join("embedding.json"), self)
    }
    pub fn wanted(&self) -> bool {
        (self.enabled || self.preparing) && !self.paused
    }
}
pub fn socket_dir(root: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(root).map_err(io)?;
    // A previous app's worker can outlive its parent. Keep its socket and locks
    // separate when the encoding contract changes, without moving model files.
    let mut identity = canonical.as_os_str().as_encoded_bytes().to_vec();
    identity.push(0);
    identity.extend_from_slice(fingerprint().as_bytes());
    let d = std::env::temp_dir().join(format!("memivy-emb-{}", &hash(&identity)[..24]));
    private_dir(&d)?;
    Ok(d)
}
pub fn vector_bytes(vector: &[f32]) -> Result<Vec<u8>> {
    if vector.len() != DIMENSIONS || vector.iter().any(|v| !v.is_finite()) {
        return Err("embedding_response_invalid".into());
    }
    let norm = vector
        .iter()
        .map(|v| (*v as f64).powi(2))
        .sum::<f64>()
        .sqrt();
    if !(0.999..=1.001).contains(&norm) {
        return Err("embedding_response_invalid".into());
    }
    Ok(vector.iter().flat_map(|v| v.to_le_bytes()).collect())
}

#[cfg(test)]
mod worker_namespace_tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn an_old_worker_socket_does_not_make_the_current_encoder_ready() {
        let root = tempfile::tempdir().unwrap();
        let canonical = fs::canonicalize(root.path()).unwrap();
        // The previous encoder addressed workers by library alone.
        let old = std::env::temp_dir().join(format!(
            "memivy-emb-{}",
            &hash(canonical.as_os_str().as_encoded_bytes())[..24]
        ));
        private_dir(&old).unwrap();
        let old_listener = UnixListener::bind(old.join("worker.sock")).unwrap();
        assert!(!client::ready(root.path()));

        let current = socket_dir(root.path()).unwrap();
        assert_eq!(current, socket_dir(&root.path().join(".")).unwrap());
        let current_listener = UnixListener::bind(current.join("worker.sock")).unwrap();
        assert!(client::ready(root.path()));

        drop(current_listener);
        drop(old_listener);
        fs::remove_dir_all(current).unwrap();
        fs::remove_dir_all(old).unwrap();
    }
}
