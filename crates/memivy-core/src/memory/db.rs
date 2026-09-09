use super::{DataError, Result};
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub(super) const SCHEMA: i64 = 7;
pub(super) const APPLICATION_ID: i64 = 0x4d495659;
#[derive(Clone, Debug)]
pub struct MemoryStore {
    pub(super) root: PathBuf,
}

pub(super) fn now() -> Result<i64> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| DataError::Invalid)?
            .as_millis(),
    )
    .map_err(|_| DataError::Invalid)
}
pub(super) fn id() -> String {
    Uuid::new_v4().to_string()
}
pub(super) fn valid_id(id: &str) -> Result<()> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| DataError::Invalid)
}
pub(super) fn valid_text(text: &str, max: usize) -> Result<()> {
    if text.trim().is_empty() || text.len() > max {
        Err(DataError::Invalid)
    } else {
        Ok(())
    }
}
pub(super) fn encode(value: &impl Serialize) -> Result<String> {
    serde_json::to_string(value).map_err(|_| DataError::Invalid)
}
pub(super) fn fingerprint(value: &impl Serialize) -> Result<Vec<u8>> {
    Ok(Sha256::digest(encode(value)?.as_bytes()).to_vec())
}
pub(super) fn private_dir(root: &Path) -> Result<()> {
    if !root.is_absolute() {
        return Err(DataError::Invalid);
    }
    // Avoid accidentally reusing the accepted prototype, even with an explicit path.
    if root.join("phase1.sqlite3").exists() {
        return Err(DataError::Schema);
    }
    fs::create_dir_all(root)?;
    if !fs::symlink_metadata(root)?.is_dir() {
        return Err(DataError::Invalid);
    }
    fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub(super) fn connect(path: &Path, create: bool) -> Result<Connection> {
    if let Ok(meta) = fs::symlink_metadata(path)
        && !meta.is_file()
    {
        return Err(DataError::Invalid);
    }
    // Create privately before SQLite opens it (including crash paths).
    if create && !path.exists() {
        use std::os::unix::fs::OpenOptionsExt;
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
    }
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    db.busy_timeout(Duration::from_millis(750))?;
    db.pragma_update(None, "foreign_keys", true)?;
    db.pragma_update(None, "synchronous", "FULL")?;
    db.pragma_update(None, "secure_delete", true)?;
    Ok(db)
}
pub(super) fn identity(db: &Connection) -> Result<i64> {
    let app: i64 = db.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if app != APPLICATION_ID || !(1..=SCHEMA).contains(&version) {
        return Err(DataError::Schema);
    }
    Ok(version)
}
// The caller holds a transaction. Read sqlite_master first to pin the snapshot
// before reading header pragmas; another opener may be initializing this file.
fn initial_schema(db: &Connection) -> Result<i64> {
    let tables: i64 = db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
        [],
        |r| r.get(0),
    )?;
    let app: i64 = db.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if (app == 0 && version == 0 && tables == 0)
        || (app == APPLICATION_ID && (1..=SCHEMA).contains(&version))
    {
        Ok(version)
    } else {
        Err(DataError::Schema)
    }
}
impl MemoryStore {
    pub fn open_default() -> Result<Self> {
        let home = std::env::var_os("HOME").ok_or(DataError::Invalid)?;
        Self::open(PathBuf::from(home).join("Library/Application Support/com.memivy.app"))
    }
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        private_dir(root.as_ref())?;
        let _guard = super::access::root_lock(root.as_ref(), false)?;
        super::access::available(root.as_ref())?;
        Self::open_unlocked(root.as_ref())
    }
    pub(super) fn open_unlocked(root: &Path) -> Result<Self> {
        let store = Self {
            root: fs::canonicalize(root)?,
        };
        let mut db = connect(&store.database_path(), true)?;
        // Reject unrelated/newer databases before changing their journal or schema.
        {
            let snapshot = db.transaction()?;
            initial_schema(&snapshot)?;
            snapshot.commit()?;
        }
        // Journal mode must change outside a transaction. Recheck the schema
        // after obtaining the writer lock, before executing any migration.
        db.pragma_update(None, "journal_mode", "WAL")?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version = initial_schema(&tx)?;
        if version == 0 {
            tx.execute_batch(include_str!(
                "../../../../migrations/memory/001_records.sql"
            ))?;
            tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        }
        if version < 2 {
            tx.execute_batch(include_str!(
                "../../../../migrations/memory/002_conversations.sql"
            ))?;
        }
        if version < 3 {
            tx.execute_batch(include_str!(
                "../../../../migrations/memory/003_workspace.sql"
            ))?;
        }
        if version < 4 {
            tx.execute_batch(include_str!(
                "../../../../migrations/memory/004_intelligence.sql"
            ))?;
        }
        if version < 5 {
            tx.execute_batch(include_str!(
                "../../../../migrations/memory/005_reviewed_conclusions.sql"
            ))?;
        }
        if version < 6 {
            tx.execute_batch(include_str!(
                "../../../../migrations/memory/006_navigation.sql"
            ))?;
        }
        if version < 7 {
            tx.execute_batch(include_str!(
                "../../../../migrations/memory/007_collection_feedback.sql"
            ))?;
        }
        tx.commit()?;
        if !db
            .prepare("SELECT 1 FROM sqlite_master WHERE name='record_fts' AND type='table'")?
            .exists([])?
        {
            store.rebuild_search_index()?;
        }
        Ok(store)
    }
    pub fn database_path(&self) -> PathBuf {
        self.root.join("memivy.db")
    }
    pub fn model_config_path(&self) -> PathBuf {
        self.root.join("model.json")
    }
    pub(super) fn connection(&self) -> Result<Connection> {
        let db = connect(&self.database_path(), false)?;
        if identity(&db)? != SCHEMA {
            return Err(DataError::Schema);
        }
        Ok(db)
    }
    pub fn check_integrity(&self) -> Result<()> {
        check(&self.connection()?)
    }
}
pub(super) fn check(db: &Connection) -> Result<()> {
    let result: String = db.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if result != "ok" || db.prepare("PRAGMA foreign_key_check")?.exists([])? {
        return Err(DataError::Integrity);
    }
    Ok(())
}
