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

fn open_lock(root: &Path, name: &str) -> Result<File> {
    let path = root.join(name);
    if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file()) {
        return Err(DataError::Invalid);
    }
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?)
}

pub(super) fn root_lock(root: &Path, exclusive: bool) -> Result<File> {
    // Close admission while an exclusive operation drains existing requests.
    // Without this gate, continuous MCP traffic can starve restore or disable.
    // OS-owned locks release on every error path and process exit.
    let admission = open_lock(root, "mcp-admission.lock")?;
    lock(&admission, exclusive)?;
    let file = open_lock(root, "mcp.lock")?;
    lock(&file, exclusive)?;
    Ok(file)
}
pub(super) fn session_lock(root: &Path, exclusive: bool) -> Result<File> {
    let file = open_lock(root, "mcp-sessions.lock")?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn waiting_exclusive_request_closes_admission_until_readers_drain() {
        let dir = tempfile::tempdir().unwrap();
        let reader = root_lock(dir.path(), false).unwrap();
        let admission = open_lock(dir.path(), "mcp-admission.lock").unwrap();
        let root = dir.path().to_path_buf();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            let guard = root_lock(&root, true).unwrap();
            acquired_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            drop(guard);
        });
        let started = Instant::now();
        loop {
            match admission.try_lock_shared() {
                Err(std::fs::TryLockError::WouldBlock) => break,
                Ok(()) => admission.unlock().unwrap(),
                Err(error) => panic!("Admission lock failed: {error}"),
            }
            assert!(started.elapsed() < Duration::from_secs(2));
            std::thread::sleep(Duration::from_millis(1));
        }
        // The writer cannot own the data gate yet, but new requests cannot enter.
        assert!(acquired_rx.try_recv().is_err());
        drop(reader);
        acquired_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let next_reader = open_lock(dir.path(), "mcp.lock").unwrap();
        assert!(matches!(
            next_reader.try_lock_shared(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        release_tx.send(()).unwrap();
        writer.join().unwrap();
        root_lock(dir.path(), false).unwrap();
    }

    #[test]
    fn timed_out_exclusive_request_reopens_admission() {
        let dir = tempfile::tempdir().unwrap();
        let reader = root_lock(dir.path(), false).unwrap();
        assert!(matches!(root_lock(dir.path(), true), Err(DataError::Busy)));
        // A timeout must not leave a persistent pending-writer marker.
        root_lock(dir.path(), false).unwrap();
        drop(reader);
        root_lock(dir.path(), true).unwrap();
    }
    #[test]
    fn idle_mcp_sessions_block_install_and_install_blocks_new_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let store = super::super::MemoryStore::open_application(dir.path()).unwrap();
        let idle = session_lock(dir.path(), false).unwrap();
        assert!(matches!(store.lock_for_app_update(), Err(DataError::Busy)));
        drop(idle);
        let installed = store.lock_for_app_update().unwrap();
        assert!(matches!(
            session_lock(dir.path(), false),
            Err(DataError::Busy)
        ));
        assert!(matches!(root_lock(dir.path(), false), Err(DataError::Busy)));
        drop(installed);
        session_lock(dir.path(), false).unwrap();
        store.check_integrity().unwrap();
    }
}
