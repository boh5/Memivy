use super::{db::*, records::*, *};
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
};
use std::{
    fmt::Write as _,
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
fn publish_database(db: &Connection, target: &Path) -> Result<()> {
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
/// Fence arbitrary user text verbatim, including Markdown, newlines and backticks.
fn block(out: &mut String, text: &str) {
    let run = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat((run + 1).max(3));
    let _ = write!(out, "\n{fence}text\n{text}\n{fence}\n\n");
}
fn refs(
    out: &mut String,
    db: &Connection,
    table: &str,
    owner_column: &str,
    owner: &str,
) -> Result<()> {
    // table and owner_column are internal constants, never caller input.
    let sql = format!(
        "SELECT kind,source_id FROM {table} WHERE {owner_column}=? {} ORDER BY kind,source_id",
        if table == "message_citations" {
            "AND cited=1"
        } else {
            ""
        }
    );
    let references: Vec<SourceRef> = db
        .prepare(&sql)?
        .query_map([owner], |r| SourceRef::from_parts(r.get(0)?, r.get(1)?))?
        .collect::<rusqlite::Result<_>>()?;
    for source in references {
        let (kind, id) = source.parts();
        let available = match resolve(db, &source, 1) {
            Ok(_) => true,
            Err(DataError::Unavailable) => false,
            Err(e) => return Err(e),
        };
        let _ = writeln!(
            out,
            "- 来源 {kind}:{id}（{}）",
            if available {
                "可用"
            } else {
                "来源已删除或不可用"
            }
        );
    }
    Ok(())
}
fn write_file(dir: &Path, name: &str, text: &str) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(text.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist_noclobber(dir.join(name))
        .map_err(|_| DataError::Io)?;
    Ok(())
}
impl MemoryStore {
    /// Produces one consistent content database; credentials are never included.
    pub fn backup(&self, target: impl AsRef<Path>) -> Result<()> {
        publish_database(&self.connection()?, target.as_ref())
    }

    /// Restores into a new empty directory only. Never replaces a running store.
    /// A restored app therefore starts with no model credentials or MCP enablement.
    pub fn restore_backup(source: impl AsRef<Path>, root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        if !root.is_absolute() || !source.as_ref().is_absolute() {
            return Err(DataError::Invalid);
        }
        if root.exists()
            && (!fs::symlink_metadata(root)?.is_dir() || fs::read_dir(root)?.next().is_some())
        {
            return Err(DataError::DestinationExists);
        }
        if !fs::symlink_metadata(source.as_ref())?.is_file() {
            return Err(DataError::Invalid);
        }
        let db = Connection::open_with_flags(source.as_ref(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        identity(&db)?;
        check(&db)?;
        private_dir(root)?;
        publish_database(&db, &root.join("memivy.db"))?;
        let store = Self::open(root)?;
        store.check_integrity()?;
        Ok(store)
    }

    /// Export one saved record as a readable article, without its source/history
    /// archive, other records, conversations, or unsubmitted editing drafts.
    pub fn export_record_markdown(
        &self,
        key: &RecordKey,
        expected_version: Option<&str>,
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
        if detail.current.as_ref().map(|v| v.id.as_str()) != expected_version {
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

    /// Markdown contains active records, all their versions and source metadata.
    /// Conversations are a separate file and never become searchable memories.
    /// Trash and erased content are excluded; the database backup includes trash.
    pub fn export_markdown(&self, target: impl AsRef<Path>) -> Result<()> {
        let target = target.as_ref();
        if !target.is_absolute() {
            return Err(DataError::Invalid);
        }
        if target.exists() || fs::symlink_metadata(target).is_ok() {
            return Err(DataError::DestinationExists);
        }
        let mut db = self.connection()?;
        let tx = db.transaction()?; // One read snapshot across every exported file.
        let mut captures =
            String::from("# 原始记录\n\n原话逐字保留；不包含回收站和已永久删除的内容。\n");
        let ids:Vec<String>=tx.prepare("SELECT c.id FROM captures c JOIN capture_state s ON s.capture_id=c.id WHERE s.availability='active' ORDER BY c.created_at,c.id")?.query_map([],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        for id in &ids {
            let c = raw(&tx, id)?;
            let _ = write!(
                captures,
                "\n## Capture {id}\n\n时间：{}；整理状态：{}\n\n来源：\n",
                c.created_at, c.understanding
            );
            block(&mut captures, &encode(&c.origin)?);
            block(&mut captures, &c.text);
            refs(&mut captures, &tx, "capture_citations", "capture_id", id)?;
            if let Some((title, target, merged)) = tx
                .query_row(
                    "SELECT title,destination,merged_body FROM conclusion_intents WHERE capture_id=?",
                    [id],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)),
                )
                .optional()?
            {
                captures.push_str("\n用户确认的名称与去向：\n");
                block(&mut captures, &title);
                block(&mut captures, &target);
                if let Some(body) = merged {
                    captures.push_str("\n用户审核的完整融合正文：\n");
                    block(&mut captures, &body);
                }
            }
        }
        let mut memories = String::from(
            "# 记忆与完整版本历史\n\n只包含有效记忆。恢复旧版本也会留下新的历史记录。\n",
        );
        let memory_ids:Vec<(String,String)>=tx.prepare("SELECT id,current_version_id FROM memories WHERE state='active' ORDER BY created_at,id")?.query_map([],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        for (memory, head) in &memory_ids {
            let _ = write!(memories, "\n## Memory {memory}\n\n当前版本：{head}\n");
            let versions:Vec<String>=tx.prepare("SELECT id FROM memory_versions WHERE memory_id=? AND body IS NOT NULL ORDER BY rowid")?.query_map([memory],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
            for id in versions {
                let v = version(&tx, &id)?;
                let _ = write!(
                    memories,
                    "\n### Version {id}\n\n上版：{}；时间：{}；作者：{}；动作：{}\n",
                    v.parent_id.as_deref().unwrap_or("无"),
                    v.created_at,
                    v.actor,
                    v.reason
                );
                block(&mut memories, &v.title);
                block(&mut memories, &v.body);
                for source in v.capture_ids {
                    let status = match raw(&tx, &source) {
                        Ok(_) => "可用",
                        Err(DataError::Unavailable) => "来源已删除或不可用",
                        Err(e) => return Err(e),
                    };
                    let _ = writeln!(
                        memories,
                        "- 原话 Capture {source}（{status}；见 captures.md）"
                    );
                }
            }
        }
        let mut conversations = String::from(
            "# 本地会话（不是长期记忆）\n\n问题、回答和草稿不会自动进入记忆搜索。引用固定到当时使用的版本。\n",
        );
        let conversations_ids: Vec<(String, String, String)> = tx
            .prepare("SELECT id,title,draft FROM conversations ORDER BY created_at,id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (conversation, title, draft) in &conversations_ids {
            let _ = write!(conversations, "\n## Conversation {conversation}\n");
            block(&mut conversations, title);
            if !draft.is_empty() {
                conversations.push_str("草稿：\n");
                block(&mut conversations, draft);
            }
            let mut rows = tx.prepare(
                "SELECT id,role,status,text FROM messages WHERE conversation_id=? ORDER BY seq",
            )?;
            for row in rows.query_map([conversation], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })? {
                let (id, role, status, text) = row?;
                let _ = write!(
                    conversations,
                    "\n### Message {id}\n\n角色：{role}；状态：{status}\n"
                );
                block(&mut conversations, &text);
                refs(
                    &mut conversations,
                    &tx,
                    "message_citations",
                    "message_id",
                    &id,
                )?;
            }
        }
        tx.commit()?;
        // Reserve the output path without replacing an existing directory.
        fs::create_dir(target).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                DataError::DestinationExists
            } else {
                DataError::Io
            }
        })?;
        fs::set_permissions(target, fs::Permissions::from_mode(0o700))?;
        write_file(target, "captures.md", &captures)?;
        write_file(target, "memories.md", &memories)?;
        write_file(target, "conversations.md", &conversations)?;
        // Written last: absence means this export did not complete successfully.
        write_file(
            target,
            "manifest.json",
            &encode(
                &serde_json::json!({"format":"memivy-markdown-v1","captures":ids.len(),"memories":memory_ids.len(),"conversations":conversations_ids.len(),"includes_trash":false,"complete":true}),
            )?,
        )?;
        fs::File::open(target)?.sync_all()?;
        fs::File::open(target.parent().ok_or(DataError::Invalid)?)?.sync_all()?;
        Ok(())
    }
}
use rusqlite::OptionalExtension;

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
    if identity(&db)? != SCHEMA {
        return Err(DataError::Schema);
    }
    check(&db)?;
    Ok(db)
}
fn staged_path(root: &Path, id: &str) -> Result<PathBuf> {
    valid_id(id)?;
    Ok(root.join(format!("restore-staged-{id}.db")))
}
fn previous_path(root: &Path, id: &str) -> PathBuf {
    root.join("recovery")
        .join(format!("before-restore-{id}.db"))
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
            message: "恢复未完成，已退回恢复前的内容。".into(),
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
        publish_database(&db, &target)?;
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
        Self::open_unlocked(root)
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
                    message: "已恢复备份，恢复前的内容也已保留。".into(),
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
                            message: format!("恢复未完成，原记忆库保留。{error}"),
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
        let db = connect(&root.join("memivy.db"), false)?;
        identity(&db)?;
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
                message: "已恢复备份，恢复前的内容也已保留。".into(),
                previous_backup: Some(backup),
            },
        )
    }
}

#[cfg(test)]
mod restore_tests {
    use super::*;
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
            capture(&store, "备份中的旧内容");
            let backup = dir.path().join("chosen.db");
            store.backup(&backup).unwrap();
            let prepared = store.prepare_restore(&backup).unwrap();
            capture(&store, "必须找回的最新内容");
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
