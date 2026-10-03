use super::{db::*, *};
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};

fn copy_database(source: &Connection, destination: &mut Connection) -> Result<()> {
    let started = Instant::now();
    let copy = Backup::new(source, destination)?;
    loop {
        match copy.step(128)? {
            StepResult::Done => return Ok(()),
            StepResult::More => (),
            StepResult::Busy | StepResult::Locked => std::thread::sleep(Duration::from_millis(10)),
            _ => return Err(DataError::Database),
        }
        if started.elapsed() > Duration::from_secs(30) {
            return Err(DataError::Busy);
        }
    }
}
pub(super) fn publish_database(db: &Connection, target: &Path) -> Result<()> {
    if !target.is_absolute() {
        return Err(DataError::Invalid);
    }
    if target.exists() || fs::symlink_metadata(target).is_ok() {
        return Err(DataError::DestinationExists);
    }
    let parent = target.parent().ok_or(DataError::Invalid)?;
    let file = tempfile::NamedTempFile::new_in(parent)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    {
        let mut output = Connection::open(file.path())?;
        copy_database(db, &mut output)?;
        output.pragma_update(None, "journal_mode", "DELETE")?;
        check(&output)?;
        output.close().map_err(|_| DataError::Database)?;
    }
    file.as_file().sync_all()?;
    file.persist_noclobber(target).map_err(|e| {
        if e.error.kind() == std::io::ErrorKind::AlreadyExists {
            DataError::DestinationExists
        } else {
            DataError::Io
        }
    })?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}
impl MemoryStore {
    /// Produces one consistent content database; credentials are never included.
    pub fn backup(&self, target: impl AsRef<Path>) -> Result<()> {
        publish_database(&self.connection()?, target.as_ref())
    }

    /// Export one saved record as a readable article, without its source/history
    /// archive, other records, conversations, or unsubmitted editing drafts.
    pub fn export_record_markdown(
        &self,
        key: &RecordKey,
        expected_version: &str,
        target: impl AsRef<Path>,
    ) -> Result<()> {
        let target = target.as_ref();
        if !target.is_absolute()
            || target
                .extension()
                .is_none_or(|e| !e.eq_ignore_ascii_case("md"))
        {
            return Err(DataError::Invalid);
        }
        let detail = self.library_detail(key)?;
        if detail.state != "active" {
            return Err(DataError::Unavailable);
        }
        if detail.current.id != expected_version {
            return Err(DataError::Conflict);
        }
        let title = detail.title.lines().collect::<Vec<_>>().join(" ");
        let content = format!("# {title}\n\n{}\n", detail.body);
        let parent = target.parent().ok_or(DataError::Invalid)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(content.as_bytes())?;
        file.as_file().sync_all()?;
        // The native save panel handles confirmation before replacing a file.
        file.persist(target).map_err(|_| DataError::Io)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedRestore {
    pub id: String,
    pub memories: i64,
    pub captures: i64,
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreResult {
    pub restored: bool,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    pub previous_backup: Option<PathBuf>,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingRestore {
    id: String,
    phase: String,
}

use std::path::PathBuf;
fn write_state(root: &Path, name: &str, value: &impl serde::Serialize) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(&serde_json::to_vec(value).map_err(|_| DataError::Invalid)?)?;
    file.as_file().sync_all()?;
    file.persist(root.join(name)).map_err(|_| DataError::Io)?;
    fs::File::open(root)?.sync_all()?;
    Ok(())
}
fn read_state<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.len() > 4096 {
        return Err(DataError::Invalid);
    }
    serde_json::from_slice(&fs::read(path)?).map_err(|_| DataError::Invalid)
}
fn backup_source(path: &Path) -> Result<Connection> {
    if !path.is_absolute() || !fs::symlink_metadata(path)?.is_file() {
        return Err(DataError::Invalid);
    }
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    super::migrations::supported_version(&db, SCHEMA)?;
    check(&db)?;
    Ok(db)
}

fn publish_upgraded_backup(source: &Connection, target: &Path, scripts: &[&str]) -> Result<()> {
    let temp = tempfile::tempdir_in(target.parent().ok_or(DataError::Invalid)?)?;
    let copy = temp.path().join("restore.db");
    publish_database(source, &copy)?;
    let mut db = connect(&copy, false)?;
    super::migrations::upgrade(&mut db, None, scripts)?;
    publish_database(&db, target)
}
fn staged_path(root: &Path, id: &str) -> Result<PathBuf> {
    valid_id(id)?;
    Ok(root.join(format!("restore-staged-{id}.db")))
}
fn previous_path(root: &Path, id: &str) -> PathBuf {
    root.join("recovery")
        .join(format!("before-restore-{id}.db"))
}
fn restore_error_code(error: &DataError) -> &'static str {
    match error {
        DataError::Io => "io",
        DataError::Busy => "busy",
        DataError::Database => "database",
        DataError::Conflict | DataError::RequestConflict => "conflict",
        DataError::Schema => "schema",
        DataError::Integrity => "integrity",
        DataError::DestinationExists => "destination_exists",
        DataError::Unavailable => "unavailable",
        _ => "operation_failed",
    }
}

fn finish_restore(root: &Path, result: &RestoreResult) -> Result<()> {
    write_state(root, "restore-result.json", result)?;
    fs::remove_file(root.join("restore-pending.json"))?;
    fs::File::open(root)?.sync_all()?;
    Ok(())
}
fn rollback_restore(root: &Path, pending: &PendingRestore) -> Result<()> {
    let backup = previous_path(root, &pending.id);
    let source = backup_source(&backup)?;
    let temp = tempfile::tempdir_in(root)?;
    let replacement = temp.path().join("memivy.db");
    publish_database(&source, &replacement)?;
    // No connections can exist here: application startup and MCP share the gate.
    for suffix in ["memivy.db-wal", "memivy.db-shm"] {
        match fs::remove_file(root.join(suffix)) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    fs::rename(replacement, root.join("memivy.db"))?;
    fs::File::open(root)?.sync_all()?;
    finish_restore(
        root,
        &RestoreResult {
            restored: false,
            message: "Restore did not complete; the previous content was recovered.".into(),
            message_code: Some("restore_rolled_back".into()),
            error_code: None,
            previous_backup: Some(backup),
        },
    )
}

impl MemoryStore {
    /// Validate and stage without changing the current library or arming a restart.
    pub fn prepare_restore(&self, source: &Path) -> Result<PreparedRestore> {
        let db = backup_source(source)?;
        let id = id();
        let target = staged_path(&self.root, &id)?;
        publish_upgraded_backup(&db, &target, super::migrations::MIGRATIONS)?;
        let db = backup_source(&target)?;
        Ok(PreparedRestore {
            id,
            memories: db.query_row(
                "SELECT count(*) FROM memories WHERE state='active'",
                [],
                |r| r.get(0),
            )?,
            captures: db.query_row(
                "SELECT count(*) FROM capture_state WHERE availability='active'",
                [],
                |r| r.get(0),
            )?,
        })
    }
    pub fn discard_prepared_restore(&self, id: &str) -> Result<()> {
        let _gate = super::access::root_lock(&self.root, true)?;
        super::access::available(&self.root)?;
        let path = staged_path(&self.root, id)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
    /// Called only after the host has successfully flushed every window's drafts.
    pub fn arm_restore(&self, id: &str) -> Result<()> {
        let _gate = super::access::root_lock(&self.root, true)?;
        super::access::available(&self.root)?;
        let path = staged_path(&self.root, id)?;
        // Full integrity was checked while staging; don't hold the gate for that work again.
        if !fs::symlink_metadata(path)?.is_file() {
            return Err(DataError::Invalid);
        }
        write_state(
            &self.root,
            "restore-pending.json",
            &PendingRestore {
                id: id.into(),
                phase: "armed".into(),
            },
        )
    }
    pub fn last_restore_result(&self) -> Result<Option<RestoreResult>> {
        let path = self.root.join("restore-result.json");
        if !path.exists() {
            return Ok(None);
        }
        read_state(&path).map(Some)
    }
    /// Application-only startup. Call before starting UI work, model jobs or watchers.
    pub fn open_application(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        private_dir(root)?;
        let _gate = super::access::root_lock(root, true)?;
        Self::finish_pending_restore(root)?;
        Self::open_unlocked(root, true)
    }
    fn finish_pending_restore(root: &Path) -> Result<()> {
        if !root.join("restore-pending.json").exists() {
            return Ok(());
        }
        let pending: PendingRestore = read_state(&root.join("restore-pending.json"))?;
        valid_id(&pending.id)?;
        match pending.phase.as_str() {
            "ready" => rollback_restore(root, &pending),
            "complete" => finish_restore(
                root,
                &RestoreResult {
                    restored: true,
                    message: "Backup restored. The previous content has also been preserved."
                        .into(),
                    message_code: Some("restore_completed".into()),
                    error_code: None,
                    previous_backup: Some(previous_path(root, &pending.id)),
                },
            ),
            "armed" => {
                if let Err(error) = Self::replace_from_staged(root, &pending.id) {
                    let state: PendingRestore = read_state(&root.join("restore-pending.json"))?;
                    if state.phase == "ready" {
                        return rollback_restore(root, &state);
                    }
                    if state.phase == "complete" {
                        return Self::finish_pending_restore(root);
                    }
                    finish_restore(
                        root,
                        &RestoreResult {
                            restored: false,
                            message: format!(
                                "Restore did not complete; the original library was preserved. {error}"
                            ),
                            message_code: Some("restore_failed_preserved".into()),
                            error_code: Some(restore_error_code(&error).into()),
                            previous_backup: None,
                        },
                    )?;
                }
                Ok(())
            }
            _ => Err(DataError::Invalid),
        }
    }
    fn replace_from_staged(root: &Path, id: &str) -> Result<()> {
        let staged = staged_path(root, id)?;
        drop(backup_source(&staged)?);
        // A restore may have been prepared by an older app before this upgrade.
        let mut staged_db = connect(&staged, false)?;
        super::migrations::upgrade(&mut staged_db, None, super::migrations::MIGRATIONS)?;
        staged_db.close().map_err(|_| DataError::Busy)?;
        let db = connect(&root.join("memivy.db"), false)?;
        super::migrations::supported_version(&db, SCHEMA)?;
        let recovery = root.join("recovery");
        private_dir(&recovery)?;
        let backup = previous_path(root, id);
        if !backup.exists() {
            publish_database(&db, &backup)?;
        }
        drop(backup_source(&backup)?);
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")?;
        db.close().map_err(|_| DataError::Busy)?;
        write_state(
            root,
            "restore-pending.json",
            &PendingRestore {
                id: id.into(),
                phase: "ready".into(),
            },
        )?;
        fs::rename(staged, root.join("memivy.db"))?;
        // A restored snapshot never reuses an in-flight encoder's generation.
        let mut restored = connect(&root.join("memivy.db"), false)?;
        identity(&restored)?;
        let tx = restored.transaction()?;
        super::embedding::reset(&tx)?;
        tx.commit()?;
        drop(restored);
        fs::File::open(root)?.sync_all()?;
        write_state(
            root,
            "restore-pending.json",
            &PendingRestore {
                id: id.into(),
                phase: "complete".into(),
            },
        )?;
        finish_restore(
            root,
            &RestoreResult {
                restored: true,
                message: "Backup restored. The previous content has also been preserved.".into(),
                message_code: Some("restore_completed".into()),
                error_code: None,
                previous_backup: Some(backup),
            },
        )
    }
}

#[cfg(test)]
mod restore_tests {
    use super::*;
    #[test]
    fn old_backup_is_upgraded_on_a_private_copy_and_failure_publishes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let backup = dir.path().join("old.db");
        store.backup(&backup).unwrap();
        let original = fs::read(&backup).unwrap();
        let source = backup_source(&backup).unwrap();
        let target = dir.path().join("upgraded.db");
        let mut future_steps = super::super::migrations::MIGRATIONS.to_vec();
        future_steps.push("CREATE TABLE restore_probe(id INTEGER);");
        publish_upgraded_backup(&source, &target, &future_steps).unwrap();
        let upgraded = Connection::open(&target).unwrap();
        assert_eq!(
            super::super::migrations::supported_version(
                &upgraded,
                super::super::migrations::CURRENT + 1
            )
            .unwrap(),
            super::super::migrations::CURRENT + 1
        );
        check(&upgraded).unwrap();
        let failed = dir.path().join("failed.db");
        *future_steps.last_mut().unwrap() = "INVALID SQL";
        assert!(publish_upgraded_backup(&source, &failed, &future_steps).is_err());
        assert!(!failed.exists());
        assert_eq!(fs::read(&backup).unwrap(), original);
        store.check_integrity().unwrap();
    }
    #[test]
    fn interrupted_database_replacement_rolls_back_before_opening_the_library() {
        for replaced in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let store = MemoryStore::open(dir.path()).unwrap();
            let capture = |s: &MemoryStore, text: &str| {
                s.capture(&CaptureRequest {
                    request_id: id(),
                    text: text.into(),
                    origin: Origin::User {
                        app: "QA".into(),
                        project: None,
                        uri: None,
                    },
                })
                .unwrap()
            };
            capture(&store, "Older content in the backup");
            let backup = dir.path().join("chosen.db");
            store.backup(&backup).unwrap();
            let prepared = store.prepare_restore(&backup).unwrap();
            capture(&store, "Latest content that must be recovered");
            store.arm_restore(&prepared.id).unwrap();
            private_dir(&dir.path().join("recovery")).unwrap();
            store
                .backup(previous_path(dir.path(), &prepared.id))
                .unwrap();
            write_state(
                dir.path(),
                "restore-pending.json",
                &PendingRestore {
                    id: prepared.id.clone(),
                    phase: "ready".into(),
                },
            )
            .unwrap();
            if replaced {
                fs::rename(
                    staged_path(dir.path(), &prepared.id).unwrap(),
                    store.database_path(),
                )
                .unwrap();
            }
            let reopened = MemoryStore::open_application(dir.path()).unwrap();
            assert_eq!(
                reopened
                    .library(&LibraryQuery::default())
                    .unwrap()
                    .items
                    .len(),
                2
            );
            assert!(!reopened.last_restore_result().unwrap().unwrap().restored);
            reopened.check_integrity().unwrap();
        }
    }
    #[test]
    fn exclusive_gate_blocks_mcp_initialization_and_existing_requests() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        store.set_mcp_enabled(true).unwrap();
        let gate = super::super::access::root_lock(dir.path(), true).unwrap();
        assert!(matches!(
            MemoryStore::open(dir.path()),
            Err(DataError::Busy)
        ));
        assert!(matches!(
            store.mcp_capture(&CaptureRequest {
                request_id: id(),
                text: "blocked".into(),
                origin: Origin::Agent {
                    app: "QA".into(),
                    project: None,
                    uri: None
                }
            }),
            Err(DataError::Busy)
        ));
        drop(gate);
        assert!(MemoryStore::open(dir.path()).is_ok());
    }
}

#[cfg(test)]
mod restore_message_tests {
    use super::*;
    #[test]
    fn old_restore_messages_are_preserved_without_guessing_their_code() {
        let old =
            r#"{"restored":false,"message":"historical original text","previous_backup":null}"#;
        let value: RestoreResult = serde_json::from_str(old).unwrap();
        assert_eq!(value.message, "historical original text");
        assert!(value.message_code.is_none());
        assert_eq!(restore_error_code(&DataError::Busy), "busy");
        assert_eq!(
            restore_error_code(&DataError::McpDisabled),
            "operation_failed"
        );
    }
}
