//! Stable cross-process gate shared by MCP initialization, tool calls and restore.
use super::{DataError, Result};
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    time::{Duration, Instant},
};
// File locks are cross-process. Releasing the exclusive switch lock is the
// boundary after which no new data request can pass an old enabled value.
fn lock(file: &File, exclusive: bool) -> Result<()> {
    let start = Instant::now();
    loop {
        let result = if exclusive {
            file.try_lock()
        } else {
            file.try_lock_shared()
        };
        match result {
            Ok(()) => return Ok(()),
            Err(std::fs::TryLockError::WouldBlock)
                if start.elapsed() < Duration::from_millis(750) =>
            {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(std::fs::TryLockError::WouldBlock) => return Err(DataError::Busy),
            Err(_) => return Err(DataError::Io),
        }
    }
}

pub(super) fn root_lock(root: &Path, exclusive: bool) -> Result<File> {
    let path = root.join("mcp.lock");
    if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file()) {
        return Err(DataError::Invalid);
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    lock(&file, exclusive)?;
    Ok(file)
}
pub(super) fn available(root: &Path) -> Result<()> {
    if root.join("restore-pending.json").exists() {
        Err(DataError::Busy)
    } else {
        Ok(())
    }
}
