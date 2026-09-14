use super::{db::*, *};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub(super) const RECEIPT_COLUMNS: &str =
    "request_id,action,capture_id,memory_id,before_version,after_version,status";
pub(super) fn read_receipt(r: &rusqlite::Row<'_>) -> rusqlite::Result<Receipt> {
    Ok(Receipt {
        request_id: r.get(0)?,
        action: r.get(1)?,
        capture_id: r.get(2)?,
        memory_id: r.get(3)?,
        before_version: r.get(4)?,
        after_version: r.get(5)?,
        status: r.get(6)?,
    })
}
pub(super) fn replay(db: &Connection, request: &str, hash: &[u8]) -> Result<Option<Receipt>> {
    valid_id(request)?;
    let old: Option<Vec<u8>> = db
        .query_row(
            "SELECT fingerprint FROM receipts WHERE request_id=?",
            [request],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(old) = old {
        if old != hash {
            return Err(DataError::RequestConflict);
        }
        return Ok(Some(db.query_row(
            &format!("SELECT {RECEIPT_COLUMNS} FROM receipts WHERE request_id=?"),
            [request],
            read_receipt,
        )?));
    }
    Ok(None)
}
pub(super) fn save_receipt(db: &Connection, receipt: &Receipt, hash: &[u8]) -> Result<()> {
    db.execute("INSERT INTO receipts(request_id,fingerprint,action,capture_id,memory_id,before_version,after_version,status,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![receipt.request_id,hash,receipt.action,receipt.capture_id,receipt.memory_id,receipt.before_version,receipt.after_version,receipt.status,now()?])?;
    if receipt.status == "applied" && receipt.action != "undo" {
        save_changes(
            db,
            &receipt.request_id,
            &[ReceiptChange {
                memory_id: receipt.memory_id.clone().ok_or(DataError::Integrity)?,
                before_version: receipt.before_version.clone(),
                after_version: receipt.after_version.clone().ok_or(DataError::Integrity)?,
                before_state: if receipt.before_version.is_some() {
                    "active"
                } else {
                    "undone"
                }
                .into(),
                after_state: "active".into(),
            }],
        )?;
    }
    Ok(())
}
pub(super) fn validate_origin(origin: &Origin) -> Result<()> {
    match origin {
        Origin::User { app, project, uri } | Origin::Agent { app, project, uri } => {
            valid_text(app, 200)?;
            if project.as_ref().is_some_and(|s| s.len() > 200)
                || uri.as_ref().is_some_and(|s| s.len() > 2048)
            {
                return Err(DataError::Invalid);
            }
        }
        Origin::Conversation {
            conversation_id,
            message_id,
            message_role,
            confirmed_by,
        } => {
            valid_id(conversation_id)?;
            valid_id(message_id)?;
            if !matches!(message_role.as_str(), "user" | "assistant") || confirmed_by != "user" {
                return Err(DataError::Invalid);
            }
        }
        Origin::Discussion {
            conversation_id,
            message_id,
            app,
            project,
            uri,
        } => {
            valid_id(conversation_id)?;
            valid_id(message_id)?;
            valid_text(app, 200)?;
            if project.as_ref().is_some_and(|s| s.len() > 200)
                || uri.as_ref().is_some_and(|s| s.len() > 2048)
            {
                return Err(DataError::Invalid);
            }
        }
    }
    Ok(())
}
pub(super) fn raw(db: &Connection, id: &str) -> Result<RawCapture> {
    let (id,text,source,created_at,understanding):(String,String,String,i64,String)=db.query_row("SELECT c.id,c.text,c.source,c.created_at,s.understanding FROM captures c JOIN capture_state s ON s.capture_id=c.id WHERE c.id=? AND s.availability='active' AND c.text IS NOT NULL",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    Ok(RawCapture {
        id,
        text,
        origin: serde_json::from_str(&source).map_err(|_| DataError::Integrity)?,
        created_at,
        understanding,
    })
}
pub(super) fn insert_capture(
    db: &Connection,
    request: &str,
    text: &str,
    origin: &Origin,
) -> Result<RawCapture> {
    insert_capture_at(db, request, text, origin, now()?)
}
/// Preserve the original expression time when a conversation is archived later.
pub(super) fn insert_capture_at(
    db: &Connection,
    request: &str,
    text: &str,
    origin: &Origin,
    created_at: i64,
) -> Result<RawCapture> {
    valid_id(request)?;
    valid_text(text, 128 * 1024)?;
    validate_origin(origin)?;
    if created_at < 0 {
        return Err(DataError::Invalid);
    }
    let hash = fingerprint(&("capture", text, origin))?;
    let old: Option<(String, Vec<u8>)> = db
        .query_row(
            "SELECT id,fingerprint FROM captures WHERE request_id=?",
            [request],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((id, old)) = old {
        if old != hash {
            return Err(DataError::RequestConflict);
        }
        return raw(db, &id);
    }
    let id = id();
    db.execute("INSERT INTO captures(id,request_id,fingerprint,text,source,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![id,request,hash,text,encode(origin)?,created_at])?;
    db.execute("INSERT INTO capture_state(capture_id) VALUES(?)", [&id])?;
    raw(db, &id)
}

/// Archive insertion is deliberately separate: confirmed conclusions append to
/// their chosen target without creating a temporary Memory or an AI job.
pub(super) fn create_captured_memory(
    db: &Connection,
    capture: &RawCapture,
) -> Result<CaptureResult> {
    let memory_id = id();
    db.execute(
        "INSERT INTO memories(id,created_at,updated_at) VALUES(?1,?2,?2)",
        params![memory_id, capture.created_at],
    )?;
    let title: String = capture
        .text
        .lines()
        .find(|s| !s.trim().is_empty())
        .unwrap_or("New memory")
        .trim()
        .chars()
        .take(45)
        .collect();
    let v = Version {
        id: id(),
        memory_id: memory_id.clone(),
        parent_id: None,
        title,
        body: capture.text.clone(),
        actor: "user".into(),
        reason: "create".into(),
        created_at: capture.created_at,
        capture_ids: vec![capture.id.clone()],
    };
    write_version(db, &v)?;
    Ok(CaptureResult {
        memory_id,
        version_id: v.id,
        capture_id: capture.id.clone(),
        created_at: capture.created_at,
    })
}

pub(super) fn version(db: &Connection, id: &str) -> Result<Version> {
    let mut v=db.query_row("SELECT id,memory_id,parent_id,title,body,actor,COALESCE(review_kind,reason),created_at FROM memory_versions WHERE id=? AND body IS NOT NULL",[id],|r|Ok(Version{id:r.get(0)?,memory_id:r.get(1)?,parent_id:r.get(2)?,title:r.get(3)?,body:r.get(4)?,actor:r.get(5)?,reason:r.get(6)?,created_at:r.get(7)?,capture_ids:vec![]}))?;
    v.capture_ids = db
        .prepare("SELECT capture_id FROM version_captures WHERE version_id=? ORDER BY capture_id")?
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(v)
}
pub(super) fn head(db: &Connection, memory_id: &str, expected: &str) -> Result<Version> {
    let current: Option<String> = db
        .query_row(
            "SELECT current_version_id FROM memories WHERE id=? AND state='active'",
            [memory_id],
            |r| r.get(0),
        )
        .optional()?;
    match current {
        Some(current) if current == expected => version(db, &current),
        _ => Err(DataError::Conflict),
    }
}
pub(super) fn write_version(db: &Connection, v: &Version) -> Result<()> {
    valid_text(&v.title, 200)?;
    valid_text(&v.body, 128 * 1024)?;
    db.execute("INSERT INTO memory_versions(id,memory_id,parent_id,title,body,actor,reason,created_at,review_kind) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![v.id,v.memory_id,v.parent_id,v.title,v.body,v.actor,if v.reason == "cleanup" { "edit" } else { &v.reason },v.created_at,if v.reason == "cleanup" { Some("cleanup") } else { None }])?;
    for source in &v.capture_ids {
        db.execute(
            "INSERT OR IGNORE INTO version_captures(version_id,capture_id) VALUES(?1,?2)",
            params![v.id, source],
        )?;
    }
    db.execute(
        "UPDATE memories SET current_version_id=?2,updated_at=?3 WHERE id=?1",
        params![v.memory_id, v.id, v.created_at],
    )?;
    db.execute("UPDATE capture_state SET understanding='attached' WHERE capture_id IN (SELECT capture_id FROM version_captures WHERE version_id=?) AND understanding!='attached'",[&v.id])?;
    Ok(())
}
pub(super) fn apply(db: &Connection, r: &ChangeRequest) -> Result<Receipt> {
    valid_id(&r.capture_id)?;
    if !db
        .prepare("SELECT 1 FROM captures WHERE id=?")?
        .exists([&r.capture_id])?
    {
        return Err(DataError::Unavailable);
    }
    valid_text(&r.title, 200)?;
    valid_text(&r.body, 128 * 1024)?;
    let (memory_id, previous) = match &r.destination {
        Destination::New => {
            let memory_id = id();
            db.execute(
                "INSERT INTO memories(id,created_at,updated_at) VALUES(?1,?2,?2)",
                params![memory_id, now()?],
            )?;
            (memory_id, None)
        }
        Destination::Existing {
            memory_id,
            expected_version,
        } => (
            memory_id.clone(),
            Some(head(db, memory_id, expected_version)?),
        ),
    };
    let mut sources = previous
        .as_ref()
        .map(|v| v.capture_ids.clone())
        .unwrap_or_default();
    if !sources.contains(&r.capture_id) {
        sources.push(r.capture_id.clone());
    }
    let v = Version {
        id: id(),
        memory_id: memory_id.clone(),
        parent_id: previous.as_ref().map(|v| v.id.clone()),
        title: r.title.clone(),
        body: r.body.clone(),
        actor: r.actor.as_str().into(),
        reason: if previous.is_some() {
            "append"
        } else {
            "create"
        }
        .into(),
        created_at: now()?,
        capture_ids: sources,
    };
    write_version(db, &v)?;
    Ok(Receipt {
        request_id: r.request_id.clone(),
        action: v.reason.clone(),
        capture_id: Some(r.capture_id.clone()),
        memory_id: Some(memory_id),
        before_version: v.parent_id.clone(),
        after_version: Some(v.id),
        status: "applied".into(),
    })
}
fn changes(db: &Connection, request: &str) -> Result<Vec<ReceiptChange>> {
    Ok(db.prepare("SELECT memory_id,before_version,after_version,before_state,after_state FROM receipt_changes WHERE request_id=? ORDER BY memory_id")?
        .query_map([request], |r| Ok(ReceiptChange { memory_id:r.get(0)?, before_version:r.get(1)?, after_version:r.get(2)?, before_state:r.get(3)?, after_state:r.get(4)? }))?
        .collect::<rusqlite::Result<_>>()?)
}
pub(super) fn save_changes(
    db: &Connection,
    request: &str,
    changes: &[ReceiptChange],
) -> Result<()> {
    for c in changes {
        db.execute("INSERT INTO receipt_changes(request_id,memory_id,before_version,after_version,before_state,after_state) VALUES(?1,?2,?3,?4,?5,?6)",
            params![request,c.memory_id,c.before_version,c.after_version,c.before_state,c.after_state])?;
    }
    Ok(())
}
/// Every affected head is checked before any compensation is committed.
fn undo_inner(
    db: &Connection,
    original: &Receipt,
    only_assignment: bool,
) -> Result<Vec<ReceiptChange>> {
    let effects = changes(db, &original.request_id)?;
    // An undo of a two-memory correction identifies the restored assignment.
    // A later correction removes just that assignment, never redoes the old
    // target change. Ordinary undo receipts are not generic redo operations.
    let restored_assignment = only_assignment
        && original.action == "undo"
        && effects.len() == 2
        && effects.iter().any(|effect| {
            Some(&effect.memory_id) == original.memory_id.as_ref() && effect.after_state == "active"
        });
    if original.status != "applied"
        || !(restored_assignment
            || matches!(
                original.action.as_str(),
                "create"
                    | "append"
                    | "edit"
                    | "restore"
                    | "conclusion"
                    | "correct"
                    | "organize"
                    | "merge"
                    | "capture"
            ))
    {
        return Err(DataError::Conflict);
    }
    let effects: Vec<_> = effects
        .into_iter()
        .filter(|effect| !only_assignment || Some(&effect.memory_id) == original.memory_id.as_ref())
        .collect();
    if effects.is_empty() {
        return Err(DataError::Integrity);
    }
    let mut inverses = Vec::new();
    for effect in effects {
        let (current, state): (String, String) = db.query_row(
            "SELECT current_version_id,state FROM memories WHERE id=?",
            [&effect.memory_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if current != effect.after_version || state != effect.after_state {
            return Err(DataError::Conflict);
        }
        if db
            .prepare("SELECT 1 FROM workspace_drafts WHERE key=?")?
            .exists([format!("memory:{}", effect.memory_id)])?
        {
            return Err(DataError::Conflict);
        }
        if original.action == "merge" && effect.after_state == "merged" && db.query_row("SELECT EXISTS(SELECT 1 FROM record_pins WHERE kind='memory' AND record_id=?1) OR EXISTS(SELECT 1 FROM collection_entries WHERE kind='memory' AND record_id=?1)",[&effect.memory_id],|r|r.get::<_,bool>(0))? {
            return Err(DataError::Conflict);
        }
        let after = if effect.before_state == "undone" {
            db.execute(
                "UPDATE memories SET state='undone',updated_at=?2 WHERE id=?1",
                params![effect.memory_id, now()?],
            )?;
            current.clone()
        } else {
            let mut v = version(
                db,
                effect
                    .before_version
                    .as_deref()
                    .ok_or(DataError::Integrity)?,
            )?;
            v.id = id();
            v.parent_id = Some(current.clone());
            v.actor = "user".into();
            v.reason = "undo".into();
            v.created_at = now()?;
            db.execute(
                "UPDATE memories SET state='active' WHERE id=?",
                [&effect.memory_id],
            )?;
            write_version(db, &v)?;
            v.id
        };
        inverses.push(ReceiptChange {
            memory_id: effect.memory_id,
            before_version: Some(current),
            after_version: after,
            before_state: effect.after_state,
            after_state: effect.before_state,
        });
    }
    db.execute(
        "UPDATE receipts SET status='undone' WHERE request_id=?",
        [&original.request_id],
    )?;
    Ok(inverses)
}
pub(super) fn resolve(db: &Connection, source: &SourceRef, max_chars: usize) -> Result<Evidence> {
    resolve_excerpt(db, source, max_chars, &[], None)
}
pub(super) fn resolve_excerpt(
    db: &Connection,
    source: &SourceRef,
    max_chars: usize,
    queries: &[String],
    start: Option<usize>,
) -> Result<Evidence> {
    let (title, text, recorded_at, current): (String, String, i64, bool) = match source {
        SourceRef::Capture(id) => {
            let c = raw(db, id)?;
            ("Original capture".into(), c.text, c.created_at, true)
        }
        SourceRef::Version(id) => db.query_row(
            "SELECT v.title,v.body,v.created_at,v.id=m.current_version_id FROM memory_versions v JOIN memories m ON m.id=v.memory_id WHERE v.id=? AND m.state='active' AND v.body IS NOT NULL",
            [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?,r.get(3)?))
        )?,
    };
    let max_chars = max_chars.clamp(1, 12_000);
    let total = text.chars().count();
    let (start, text) = if let Some(start) = start {
        (start, text.chars().skip(start).take(max_chars).collect())
    } else {
        super::retrieval::excerpt(&text, &title, queries, max_chars)
    };
    Ok(Evidence {
        source: source.clone(),
        title,
        truncated: start > 0 || total > start + max_chars,
        text,
        recorded_at,
        current,
        start,
        additional_spans: vec![],
    })
}
impl MemoryStore {
    /// Current body, input archive, receipt and organization task commit together.
    pub fn capture(&self, r: &CaptureRequest) -> Result<CaptureResult> {
        if matches!(
            r.origin,
            Origin::Conversation { .. } | Origin::Discussion { .. }
        ) {
            return Err(DataError::Invalid);
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let hash = fingerprint(&("capture", &r.text, &r.origin))?;
        if let Some(receipt) = replay(&tx, &r.request_id, &hash)? {
            let capture_id = receipt.capture_id.ok_or(DataError::Integrity)?;
            let created_at = tx.query_row(
                "SELECT created_at FROM captures WHERE id=?",
                [&capture_id],
                |row| row.get(0),
            )?;
            return Ok(CaptureResult {
                memory_id: receipt.memory_id.ok_or(DataError::Integrity)?,
                version_id: receipt.after_version.ok_or(DataError::Integrity)?,
                capture_id,
                created_at,
            });
        }
        let c = insert_capture(&tx, &r.request_id, &r.text, &r.origin)?;
        // Old capture request IDs still replay without promoting an archive twice.
        let linked: Option<(String,String)> = tx.query_row("SELECT m.id,m.current_version_id FROM version_captures vc JOIN memory_versions v ON v.id=vc.version_id JOIN memories m ON m.id=v.memory_id WHERE vc.capture_id=? ORDER BY v.created_at,v.rowid LIMIT 1",[&c.id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((memory_id, version_id)) = linked {
            tx.commit()?;
            return Ok(CaptureResult {
                memory_id,
                version_id,
                capture_id: c.id,
                created_at: c.created_at,
            });
        }
        let saved = create_captured_memory(&tx, &c)?;
        save_receipt(
            &tx,
            &Receipt {
                request_id: r.request_id.clone(),
                action: "capture".into(),
                capture_id: Some(c.id.clone()),
                memory_id: Some(saved.memory_id.clone()),
                before_version: None,
                after_version: Some(saved.version_id.clone()),
                status: "applied".into(),
            },
            &hash,
        )?;
        if crate::models::Registry::read(&self.root).is_ok_and(|r| r.auto_organize) {
            tx.execute("INSERT INTO organization_jobs(memory_id,input_version_id,capture_id,attempt_id,status,created_at) VALUES(?1,?2,?3,?4,'pending',?5)",params![saved.memory_id,saved.version_id,c.id,id(),c.created_at])?;
        }
        tx.commit()?;
        Ok(saved)
    }
    pub fn capture_by_id(&self, id: &str) -> Result<RawCapture> {
        raw(&self.connection()?, id)
    }
    pub fn defer_capture(&self, id: &str) -> Result<()> {
        let db = self.connection()?;
        if db.execute("UPDATE capture_state SET understanding='deferred' WHERE capture_id=? AND availability='active' AND understanding IN ('pending','deferred')",[id])?==0 { return Err(DataError::Conflict) }
        Ok(())
    }
    pub fn apply_capture(&self, r: &ChangeRequest) -> Result<Receipt> {
        let hash = fingerprint(&("apply", r))?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = replay(&tx, &r.request_id, &hash)? {
            return Ok(old);
        }
        let receipt = apply(&tx, r)?;
        save_receipt(&tx, &receipt, &hash)?;
        tx.commit()?;
        Ok(receipt)
    }
    pub fn edit_memory(&self, r: &EditRequest) -> Result<Receipt> {
        let hash = fingerprint(&("edit", r))?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = replay(&tx, &r.request_id, &hash)? {
            return Ok(old);
        }
        let mut v = head(&tx, &r.memory_id, &r.expected_version)?;
        v.id = id();
        v.parent_id = Some(r.expected_version.clone());
        v.title = r.title.clone();
        v.body = r.body.clone();
        v.actor = "user".into();
        v.reason = "edit".into();
        v.created_at = now()?;
        write_version(&tx, &v)?;
        let receipt = Receipt {
            request_id: r.request_id.clone(),
            action: "edit".into(),
            capture_id: None,
            memory_id: Some(r.memory_id.clone()),
            before_version: v.parent_id,
            after_version: Some(v.id),
            status: "applied".into(),
        };
        save_receipt(&tx, &receipt, &hash)?;
        tx.commit()?;
        Ok(receipt)
    }
    pub fn restore_version(
        &self,
        request: &str,
        memory: &str,
        expected: &str,
        old_version: &str,
    ) -> Result<Receipt> {
        self.restore_source(
            request,
            memory,
            expected,
            &SourceRef::Version(old_version.into()),
        )
    }
    /// Manual archive recovery only; callers must show the selected text before confirmation.
    pub fn restore_archive(
        &self,
        request: &str,
        memory: &str,
        expected: &str,
        capture: &str,
    ) -> Result<Receipt> {
        self.restore_source(
            request,
            memory,
            expected,
            &SourceRef::Capture(capture.into()),
        )
    }
    fn restore_source(
        &self,
        request: &str,
        memory: &str,
        expected: &str,
        source: &SourceRef,
    ) -> Result<Receipt> {
        let hash = match source {
            SourceRef::Version(v) => fingerprint(&("restore", memory, expected, v))?,
            SourceRef::Capture(c) => fingerprint(&("restore_archive", memory, expected, c))?,
        };
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = replay(&tx, request, &hash)? {
            return Ok(old);
        }
        let current = head(&tx, memory, expected)?;
        let mut v = match source {
            SourceRef::Version(old) => {
                let version = version(&tx, old)?;
                if version.memory_id != memory {
                    return Err(DataError::Invalid);
                }
                version
            }
            SourceRef::Capture(capture) => {
                let linked: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM version_captures vc JOIN memory_versions v ON v.id=vc.version_id WHERE v.memory_id=? AND vc.capture_id=?)", params![memory,capture], |r|r.get(0))?;
                if !linked {
                    return Err(DataError::Invalid);
                }
                let archive = raw(&tx, capture)?;
                let mut version = current;
                version.body = archive.text;
                if !version.capture_ids.contains(capture) {
                    version.capture_ids.push(capture.clone());
                }
                version
            }
        };
        v.id = id();
        v.parent_id = Some(expected.into());
        v.actor = "user".into();
        v.reason = "restore".into();
        v.created_at = now()?;
        write_version(&tx, &v)?;
        let receipt = Receipt {
            request_id: request.into(),
            action: "restore".into(),
            capture_id: None,
            memory_id: Some(memory.into()),
            before_version: Some(expected.into()),
            after_version: Some(v.id),
            status: "applied".into(),
        };
        save_receipt(&tx, &receipt, &hash)?;
        tx.commit()?;
        Ok(receipt)
    }
    pub fn receipt(&self, request: &str) -> Result<Receipt> {
        Ok(self.connection()?.query_row(
            &format!("SELECT {RECEIPT_COLUMNS} FROM receipts WHERE request_id=?"),
            [request],
            read_receipt,
        )?)
    }
    pub fn undo(&self, request: &str, original_request: &str) -> Result<Receipt> {
        let hash = fingerprint(&("undo", original_request))?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = replay(&tx, request, &hash)? {
            return Ok(old);
        }
        let original = tx.query_row(
            &format!("SELECT {RECEIPT_COLUMNS} FROM receipts WHERE request_id=?"),
            [original_request],
            read_receipt,
        )?;
        let inverse = undo_inner(&tx, &original, false)?;
        // Undoing a correction moves the capture back to its former memory.
        // Point the receipt there, with its new head and removal baseline, so
        // the user can correct that assignment again without stale receipts.
        let restored = (original.action == "correct")
            .then(|| {
                inverse.iter().find(|c| {
                    Some(&c.memory_id) != original.memory_id.as_ref() && c.after_state == "active"
                })
            })
            .flatten();
        let main = restored
            .or_else(|| {
                inverse
                    .iter()
                    .find(|c| Some(&c.memory_id) == original.memory_id.as_ref())
            })
            .ok_or(DataError::Integrity)?;
        let receipt = Receipt {
            request_id: request.into(),
            action: "undo".into(),
            capture_id: original.capture_id,
            memory_id: Some(main.memory_id.clone()),
            before_version: main.before_version.clone(),
            after_version: Some(main.after_version.clone()),
            status: "applied".into(),
        };
        save_receipt(&tx, &receipt, &hash)?;
        save_changes(&tx, &receipt.request_id, &inverse)?;
        tx.commit()?;
        Ok(receipt)
    }
    /// Correct a mistaken assignment atomically. Both target versions are checked.
    pub fn correct_assignment(&self, original_request: &str, r: &ChangeRequest) -> Result<Receipt> {
        if r.actor != Actor::User {
            return Err(DataError::Invalid);
        }
        let hash = fingerprint(&("correct", original_request, r))?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = replay(&tx, &r.request_id, &hash)? {
            return Ok(old);
        }
        let original = tx.query_row(
            &format!("SELECT {RECEIPT_COLUMNS} FROM receipts WHERE request_id=?"),
            [original_request],
            read_receipt,
        )?;
        if original.capture_id.as_deref() != Some(&r.capture_id) {
            return Err(DataError::Invalid);
        }
        if let Destination::Existing { memory_id, .. } = &r.destination
            && original.memory_id.as_ref() == Some(memory_id)
        {
            return Err(DataError::Invalid);
        }
        let inverse = if original.status == "applied" {
            undo_inner(&tx, &original, true)?
        } else if original.status == "needs_review" {
            vec![]
        } else {
            return Err(DataError::Conflict);
        };
        let mut receipt = apply(&tx, r)?;
        receipt.action = "correct".into();
        tx.execute(
            "UPDATE receipts SET status='undone' WHERE request_id=?",
            [original_request],
        )?;
        save_receipt(&tx, &receipt, &hash)?;
        save_changes(&tx, &receipt.request_id, &inverse)?;
        tx.execute("UPDATE organization_jobs SET receipt_id=?2,status='done',reason='',reason_code='organization_corrected' WHERE capture_id=?1",params![r.capture_id,receipt.request_id])?;
        tx.commit()?;
        Ok(receipt)
    }
    pub fn receipt_changes(&self, request: &str) -> Result<Vec<ReceiptChange>> {
        changes(&self.connection()?, request)
    }
    pub fn memory(&self, id: &str) -> Result<Memory> {
        let mut connection = self.connection()?;
        let db = connection.transaction()?;
        let head: String = db.query_row(
            "SELECT current_version_id FROM memories WHERE id=? AND state='active'",
            [id],
            |r| r.get(0),
        )?;
        Ok(Memory {
            id: id.into(),
            state: "active".into(),
            current: version(&db, &head)?,
        })
    }
    pub fn memories(&self, include_trash: bool, limit: usize) -> Result<Vec<Memory>> {
        let mut connection = self.connection()?;
        let db = connection.transaction()?;
        let ids:Vec<(String,String,String)>=db.prepare("SELECT id,state,current_version_id FROM memories WHERE state='active' OR (?1 AND state='trashed') ORDER BY updated_at DESC,id LIMIT ?2")?.query_map(params![include_trash,limit.clamp(1,100) as i64],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        ids.into_iter()
            .map(|(id, state, head)| {
                Ok(Memory {
                    id,
                    state,
                    current: version(&db, &head)?,
                })
            })
            .collect()
    }
    pub fn history(&self, memory: &str) -> Result<Vec<Version>> {
        let mut connection = self.connection()?;
        let db = connection.transaction()?;
        let ids:Vec<String>=db.prepare("SELECT v.id FROM memory_versions v JOIN memories m ON m.id=v.memory_id WHERE m.id=? AND m.state IN ('active','trashed') AND v.body IS NOT NULL ORDER BY v.rowid")?.query_map([memory],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        ids.iter().map(|id| version(&db, id)).collect()
    }
    pub fn trash_memory(&self, memory: &str, expected: &str) -> Result<()> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let already:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM memories WHERE id=?1 AND current_version_id=?2 AND state='trashed')",params![memory,expected],|r|r.get(0))?;
        if already {
            return Ok(());
        }
        head(&tx, memory, expected)?;
        tx.execute(
            "UPDATE memories SET state='trashed',updated_at=?2 WHERE id=?1",
            params![memory, now()?],
        )?;
        // All historical sources exclusive to this memory enter trash with it.
        // Shared sources and independently trashed sources retain their own state.
        tx.execute("UPDATE capture_state SET availability='trashed',trash_owner=?1 WHERE availability='active' AND capture_id IN (SELECT vc.capture_id FROM version_captures vc JOIN memory_versions v ON v.id=vc.version_id WHERE v.memory_id=?1) AND NOT EXISTS(SELECT 1 FROM version_captures vc JOIN memory_versions v ON v.id=vc.version_id JOIN memories m ON m.id=v.memory_id WHERE vc.capture_id=capture_state.capture_id AND m.state='active')",[memory])?;
        tx.commit()?;
        Ok(())
    }
    pub fn restore_memory(&self, memory: &str) -> Result<()> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: String =
            tx.query_row("SELECT state FROM memories WHERE id=?", [memory], |r| {
                r.get(0)
            })?;
        if state == "active" {
            return Ok(());
        }
        if state != "trashed" {
            return Err(DataError::Unavailable);
        }
        tx.execute(
            "UPDATE memories SET state='active',updated_at=?2 WHERE id=?1",
            params![memory, now()?],
        )?;
        // A shared source may have entered trash with a different memory later.
        tx.execute("UPDATE capture_state SET availability='active',trash_owner=NULL WHERE availability='trashed' AND trash_owner IS NOT NULL AND capture_id IN (SELECT vc.capture_id FROM version_captures vc JOIN memory_versions v ON v.id=vc.version_id WHERE v.memory_id=?)",[memory])?;
        tx.commit()?;
        Ok(())
    }
    pub fn trash_capture(&self, capture: &str) -> Result<()> {
        let db = self.connection()?;
        if db.execute("UPDATE capture_state SET availability='trashed',trash_owner=NULL WHERE capture_id=? AND availability!='purged'",[capture])?==0 { return Err(DataError::Unavailable) }
        Ok(())
    }
    pub fn restore_capture(&self, capture: &str) -> Result<()> {
        let db = self.connection()?;
        if db.execute("UPDATE capture_state SET availability='active',trash_owner=NULL WHERE capture_id=? AND availability!='purged'",[capture])?==0 { return Err(DataError::Unavailable) }
        Ok(())
    }
    pub fn purge_capture(&self, capture: &str) -> Result<()> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        erase_capture(&tx, capture)?;
        tx.commit()?;
        Ok(())
    }
    /// Explicit erasure accepts trash and withdrawn memories addressable by a
    /// receipt. Purging a withdrawn snapshot leaves retained raw input intact.
    pub fn purge_memory(&self, memory: &str) -> Result<()> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: String =
            tx.query_row("SELECT state FROM memories WHERE id=?", [memory], |r| {
                r.get(0)
            })?;
        if state == "purged" {
            return Ok(());
        }
        if !matches!(state.as_str(), "trashed" | "undone") {
            return Err(DataError::Conflict);
        }
        let sources:Vec<String>=tx.prepare("SELECT capture_id FROM capture_state WHERE availability='trashed' AND trash_owner=?")?.query_map([memory],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        for source in sources {
            let other:Option<String>=tx.query_row("SELECT m.id FROM version_captures vc JOIN memory_versions v ON v.id=vc.version_id JOIN memories m ON m.id=v.memory_id WHERE vc.capture_id=?1 AND m.id!=?2 AND m.state IN ('active','trashed') ORDER BY m.id LIMIT 1",params![source,memory],|r|r.get(0)).optional()?;
            if let Some(other) = other {
                tx.execute(
                    "UPDATE capture_state SET trash_owner=?2 WHERE capture_id=?1",
                    params![source, other],
                )?;
            } else {
                erase_capture(&tx, &source)?;
            }
        }
        let merged: Vec<String> = tx.prepare("SELECT c.memory_id FROM receipts r JOIN receipt_changes c ON c.request_id=r.request_id JOIN memories m ON m.id=c.memory_id WHERE r.memory_id=? AND r.action='merge' AND r.status='applied' AND c.after_state='merged' AND m.state='merged' AND m.current_version_id=c.after_version")?
            .query_map([memory],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        for input in merged {
            erase_memory(&tx, &input)?;
        }
        erase_memory(&tx, memory)?;
        tx.commit()?;
        Ok(())
    }
    pub fn resolve_source(&self, source: &SourceRef, max_chars: usize) -> Result<Evidence> {
        resolve(&self.connection()?, source, max_chars)
    }
}
fn erase_capture(db: &Connection, capture: &str) -> Result<()> {
    db.execute(
        "DELETE FROM workspace_drafts WHERE key=?",
        [format!("capture:{capture}")],
    )?;
    let state: String = db.query_row(
        "SELECT availability FROM capture_state WHERE capture_id=?",
        [capture],
        |r| r.get(0),
    )?;
    if !matches!(state.as_str(), "trashed" | "purged") {
        return Err(DataError::Conflict);
    }
    db.execute(
        "UPDATE captures SET text=NULL,source=NULL WHERE id=?",
        [capture],
    )?;
    db.execute(
        "UPDATE capture_state SET availability='purged',trash_owner=NULL WHERE capture_id=?",
        [capture],
    )?;
    // Withdrawn memories have no library/trash entry. Erase their snapshots
    // with an explicitly purged source so hidden copies cannot survive in the
    // FTS index or a new backup. Other memories and their raw sources stay put.
    let abandoned: Vec<String> = db.prepare(
        "SELECT DISTINCT m.id FROM memories m JOIN memory_versions v ON v.memory_id=m.id JOIN version_captures vc ON vc.version_id=v.id WHERE m.state='undone' AND vc.capture_id=?"
    )?.query_map([capture], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    for memory in abandoned {
        erase_memory(db, &memory)?;
    }
    Ok(())
}
fn erase_memory(db: &Connection, memory: &str) -> Result<()> {
    db.execute(
        "DELETE FROM workspace_drafts WHERE key=?",
        [format!("memory:{memory}")],
    )?;
    db.execute(
        "UPDATE memory_versions SET title=NULL,body=NULL WHERE memory_id=?",
        [memory],
    )?;
    db.execute(
        "UPDATE memories SET state='purged',updated_at=?2 WHERE id=?1",
        params![memory, now()?],
    )?;
    Ok(())
}
