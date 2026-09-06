use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, os::unix::fs::PermissionsExt, path::PathBuf};

/// Shared by the app and the stdio binary. Never use the caller's working directory.
#[derive(Clone, Debug)]
pub struct DataPaths {
    pub root: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct McpConfig {
    enabled: bool,
}

impl DataPaths {
    pub fn resolve() -> Result<Self> {
        let root = match std::env::var_os("MEMIVY_PHASE1_DATA_DIR") {
            Some(path) => PathBuf::from(path),
            None => PathBuf::from(std::env::var_os("HOME").ok_or(Error::Invalid("缺少用户目录"))?)
                .join("Library/Application Support/com.memivy.phase1"),
        };
        Self::new(root)
    }

    pub fn new(root: PathBuf) -> Result<Self> {
        if !root.is_absolute() {
            return Err(Error::Invalid("样机数据目录必须为绝对路径"));
        }
        fs::create_dir_all(&root)?;
        if fs::symlink_metadata(&root)?.file_type().is_symlink() {
            return Err(Error::Invalid("样机数据目录不能是符号链接"));
        }
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        Ok(Self { root })
    }

    pub fn database(&self) -> PathBuf {
        self.root.join("phase1.sqlite3")
    }
    pub fn model_config(&self) -> PathBuf {
        self.root.join("model.json")
    }

    /// Re-read on every request. Missing, malformed, or unreadable means disabled.
    pub fn mcp_enabled(&self) -> bool {
        fs::read(self.root.join("mcp.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<McpConfig>(&bytes).ok())
            .is_some_and(|config| config.enabled)
    }

    pub fn set_mcp_enabled(&self, enabled: bool) -> Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(&self.root)?;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(if enabled {
            b"{\"enabled\":true}\n"
        } else {
            b"{\"enabled\":false}\n"
        })?;
        file.as_file().sync_all()?;
        file.persist(self.root.join("mcp.json"))
            .map_err(|e| e.error)?;
        fs::File::open(&self.root)?.sync_all()?;
        Ok(())
    }
}
