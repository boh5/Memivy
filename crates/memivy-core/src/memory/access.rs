//! Cross-process gates shared by MCP sessions, tool calls, restore and app updates.
use super::{DataError, Result};
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
const LOCK_TIMEOUT: Duration = Duration::from_millis(750);
const ADMISSION_LOCK: &str = "mcp-admission.lock";
const REQUEST_LOCK: &str = "mcp.lock";
const UPDATE_LOCK: &str = "mcp-update.lock";
const SESSION_LOCK: &str = "mcp-sessions.lock";

// File locks are cross-process. Releasing the exclusive switch lock is the
// boundary after which no new data request can pass an old enabled value.
fn lock(file: &File, exclusive: bool) -> Result<()> {
    lock_until(file, exclusive, Instant::now() + LOCK_TIMEOUT)
}

fn lock_until(file: &File, exclusive: bool, deadline: Instant) -> Result<()> {
    loop {
        let result = if exclusive {
            file.try_lock()
        } else {
            file.try_lock_shared()
        };
        match result {
            Ok(()) => return Ok(()),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
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
    let admission = open_lock(root, ADMISSION_LOCK)?;
    lock(&admission, exclusive)?;
    let file = open_lock(root, REQUEST_LOCK)?;
    lock(&file, exclusive)?;
    Ok(file)
}
fn session_lock(root: &Path, exclusive: bool) -> Result<File> {
    let file = open_lock(root, SESSION_LOCK)?;
    lock(&file, exclusive)?;
    Ok(file)
}

#[derive(Debug)]
pub struct AppUpdateGuard {
    root: PathBuf,
    _update: File,
    _requests: File,
    _admission: File,
}

impl AppUpdateGuard {
    pub fn session_lock_path(&self) -> PathBuf {
        self.root.join(SESSION_LOCK)
    }

    /// A cooperative MCP sidecar keeps this file open while it drains responses.
    pub fn update_lock_path(&self) -> PathBuf {
        self.root.join(UPDATE_LOCK)
    }

    /// Return the lifetime gate only after all existing MCP sessions leave.
    /// Keep both this guard and the returned file alive through installation.
    pub fn wait_for_sessions(&self) -> Result<File> {
        session_lock(&self.root, true)
    }
}

pub(super) fn prepare_app_update(root: &Path, timeout: Duration) -> Result<AppUpdateGuard> {
    let deadline = Instant::now() + timeout;
    let admission = open_lock(root, ADMISSION_LOCK)?;
    lock_until(&admission, true, deadline)?;
    let requests = open_lock(root, REQUEST_LOCK)?;
    lock_until(&requests, true, deadline)?;
    available(root)?;
    // Notify sessions only after their admitted requests have drained. Keep
    // admission closed to old helpers until the replacement has finished.
    let update = open_lock(root, UPDATE_LOCK)?;
    lock_until(&update, true, deadline)?;
    Ok(AppUpdateGuard {
        root: root.to_path_buf(),
        _update: update,
        _requests: requests,
        _admission: admission,
    })
}

#[derive(Debug)]
pub struct McpSession {
    _lifetime: File,
    update: File,
}

impl McpSession {
    /// Check whether an updater has drained requests and asked sessions to close.
    pub fn update_requested(&self) -> Result<bool> {
        match self.update.try_lock_shared() {
            Ok(()) => {
                self.update.unlock()?;
                Ok(false)
            }
            Err(std::fs::TryLockError::WouldBlock) => Ok(true),
            Err(_) => Err(DataError::Io),
        }
    }
}

pub(super) fn open_mcp_session(root: &Path) -> Result<McpSession> {
    let update = open_lock(root, UPDATE_LOCK)?;
    // A process launched during replacement must not wait and then open the
    // updated library using an executable that was loaded before the swap.
    lock_until(&update, false, Instant::now())?;
    let lifetime = session_lock(root, false)?;
    // The caller must open the database after releasing this lock: the updater
    // acquires request admission before the update lock.
    update.unlock()?;
    Ok(McpSession {
        _lifetime: lifetime,
        update,
    })
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
    fn app_update_signals_existing_sessions_after_draining_requests() {
        let dir = tempfile::tempdir().unwrap();
        let idle = open_mcp_session(dir.path()).unwrap();
        let reader = root_lock(dir.path(), false).unwrap();
        let admission = open_lock(dir.path(), ADMISSION_LOCK).unwrap();
        let root = dir.path().to_path_buf();
        let (tx, rx) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            tx.send(prepare_app_update(&root, Duration::from_secs(3)))
                .unwrap();
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
        assert!(!idle.update_requested().unwrap());
        assert!(rx.try_recv().is_err());
        drop(reader);
        let update = rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();
        writer.join().unwrap();
        assert!(idle.update_requested().unwrap());
        assert!(matches!(
            admission.try_lock_shared(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        assert!(matches!(open_mcp_session(dir.path()), Err(DataError::Busy)));
        assert!(matches!(update.wait_for_sessions(), Err(DataError::Busy)));
        drop(idle);
        let sessions = update.wait_for_sessions().unwrap();
        assert_eq!(update.session_lock_path(), dir.path().join(SESSION_LOCK));
        assert_eq!(update.update_lock_path(), dir.path().join(UPDATE_LOCK));
        drop((sessions, update));
        assert!(
            !open_mcp_session(dir.path())
                .unwrap()
                .update_requested()
                .unwrap()
        );
        root_lock(dir.path(), false).unwrap();
    }

    #[test]
    fn app_update_drain_failure_releases_admission_without_notifying_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let idle = open_mcp_session(dir.path()).unwrap();
        let reader = root_lock(dir.path(), false).unwrap();
        assert!(matches!(
            prepare_app_update(dir.path(), Duration::from_millis(30)),
            Err(DataError::Busy)
        ));
        assert!(!idle.update_requested().unwrap());
        root_lock(dir.path(), false).unwrap();
        drop(reader);

        fs::write(dir.path().join("restore-pending.json"), b"{}").unwrap();
        assert!(matches!(
            prepare_app_update(dir.path(), Duration::from_millis(30)),
            Err(DataError::Busy)
        ));
        assert!(!idle.update_requested().unwrap());
        root_lock(dir.path(), true).unwrap();
    }

    #[test]
    fn app_update_rejects_new_sessions_without_waiting_for_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let update = prepare_app_update(dir.path(), Duration::from_secs(1)).unwrap();
        let root = dir.path().to_path_buf();
        let (tx, rx) = mpsc::channel();
        let startup = std::thread::spawn(move || tx.send(open_mcp_session(&root)).unwrap());
        // The ordinary lock wait is 750 ms. Startup must report Busy while the
        // update still owns its gate, without entering that wait.
        let result = rx.recv_timeout(Duration::from_millis(500));
        drop(update);
        startup.join().unwrap();
        assert!(matches!(result.unwrap(), Err(DataError::Busy)));
    }
}
