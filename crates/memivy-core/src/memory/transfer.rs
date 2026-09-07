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
