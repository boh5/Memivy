use super::{db::*, *};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub(super) const RECEIPT_COLUMNS: &str = "request_id,action,capture_id,memory_id,before_version,after_version,status,reason,collection_changes";
pub(super) fn read_receipt(r: &rusqlite::Row<'_>) -> rusqlite::Result<Receipt> {
    Ok(Receipt {
        collection_changes: serde_json::from_str(&r.get::<_, String>(8)?).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
        })?,
        reason: r.get(7)?,
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
    db.execute("INSERT INTO receipts(request_id,fingerprint,action,capture_id,memory_id,before_version,after_version,status,created_at,reason,collection_changes) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![receipt.request_id,hash,receipt.action,receipt.capture_id,receipt.memory_id,receipt.before_version,receipt.after_version,receipt.status,now()?,receipt.reason,encode(&receipt.collection_changes)?])?;
    if receipt.status == "applied"
        && receipt.action != "undo"
        && receipt.memory_id.is_some()
        && receipt.after_version.is_some()
    {
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
                navigation_before: None,
                navigation_after: None,
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
    let (id,text,source,created_at):(String,String,String,i64)=db.query_row("SELECT c.id,c.text,c.source,c.created_at FROM captures c JOIN capture_state s ON s.capture_id=c.id WHERE c.id=? AND s.availability='active' AND c.text IS NOT NULL",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    Ok(RawCapture {
        id,
        text,
        origin: serde_json::from_str(&source).map_err(|_| DataError::Integrity)?,
        created_at,
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
    Ok(())
}
pub(super) fn navigation_snapshot(db: &Connection, memory: &str) -> Result<NavigationSnapshot> {
    let collections = db.prepare("SELECT collection_id FROM collection_entries WHERE kind='memory' AND record_id=? ORDER BY collection_id")?
        .query_map([memory], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let pinned = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM record_pins WHERE kind='memory' AND record_id=?)",
        [memory],
        |r| r.get(0),
    )?;
    Ok(NavigationSnapshot {
        collections,
        pinned: Some(pinned),
    })
}
pub(super) fn navigation_matches(
    db: &Connection,
    memory: &str,
    expected: &NavigationSnapshot,
) -> Result<bool> {
    let current = navigation_snapshot(db, memory)?;
    Ok(current.collections == expected.collections
        && expected
            .pinned
            .is_none_or(|pin| current.pinned == Some(pin)))
}
pub(super) fn restore_navigation(
    db: &Connection,
    memory: &str,
    value: &NavigationSnapshot,
) -> Result<()> {
    let current = navigation_snapshot(db, memory)?;
    for collection in current
        .collections
        .iter()
        .filter(|c| !value.collections.contains(c))
    {
        agent_collections::set_collection_member(db, collection, memory, false)?;
    }
    for collection in value
        .collections
        .iter()
        .filter(|c| !current.collections.contains(c))
    {
        agent_collections::set_collection_member(db, collection, memory, true)?;
    }
    if let Some(pinned) = value.pinned {
        db.execute(
            "DELETE FROM record_pins WHERE kind='memory' AND record_id=?",
            [memory],
        )?;
        if pinned {
            db.execute(
                "INSERT INTO record_pins(kind,record_id,created_at) VALUES('memory',?1,?2)",
                params![memory, now()?],
            )?;
        }
    }
    Ok(())
}
pub(super) fn changes(db: &Connection, request: &str) -> Result<Vec<ReceiptChange>> {
    db.prepare("SELECT memory_id,before_version,after_version,before_state,after_state,navigation_before,navigation_after FROM receipt_changes WHERE request_id=? ORDER BY memory_id")?
        .query_map([request], |r| Ok((ReceiptChange { memory_id:r.get(0)?, before_version:r.get(1)?, after_version:r.get(2)?, before_state:r.get(3)?, after_state:r.get(4)?, navigation_before:None, navigation_after:None }, r.get::<_, Option<String>>(5)?,r.get::<_, Option<String>>(6)?)))?
        .map(|row| { let (mut change, before, after) = row?;
            change.navigation_before = before.as_deref().map(decode).transpose()?;
            change.navigation_after = after.as_deref().map(decode).transpose()?;
            Ok(change)
        }).collect()
}
pub(super) fn save_changes(
    db: &Connection,
    request: &str,
    changes: &[ReceiptChange],
) -> Result<()> {
    for c in changes {
        let after = c
            .navigation_after
            .clone()
            .unwrap_or(navigation_snapshot(db, &c.memory_id)?);
        let before = c.navigation_before.clone().unwrap_or_else(|| {
            if c.before_state == "undone" {
                NavigationSnapshot {
                    collections: vec![],
                    pinned: Some(false),
                }
            } else {
                after.clone()
            }
        });
        db.execute("INSERT INTO receipt_changes(request_id,memory_id,before_version,after_version,before_state,after_state,navigation_before,navigation_after) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![request,c.memory_id,c.before_version,c.after_version,c.before_state,c.after_state,encode(&before)?,encode(&after)?])?;
    }
    Ok(())
}
/// Every affected head is checked before any compensation is committed.
fn undo_inner(db: &Connection, original: &Receipt) -> Result<Vec<ReceiptChange>> {
    if original.status != "applied"
        || !matches!(
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
        )
    {
        return Err(DataError::Conflict);
    }
    let effects = changes(db, &original.request_id)?;
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
        if let Some(expected) = &effect.navigation_after
            && !navigation_matches(db, &effect.memory_id, expected)?
        {
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
        if let Some(before) = &effect.navigation_before {
            restore_navigation(db, &effect.memory_id, before)?;
        }
        inverses.push(ReceiptChange {
            navigation_before: effect.navigation_after,
            navigation_after: effect.navigation_before,
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
/// Hidden merge sources remain citeable only while an effective merge leads to
/// an active memory. Trash and purged owners never become a historical backdoor.
pub(super) fn memory_reference_visible(db: &Connection, memory: &str) -> Result<bool> {
    Ok(db.query_row("WITH RECURSIVE owners(id) AS (SELECT ?1 UNION SELECT r.memory_id FROM owners o JOIN memories m ON m.id=o.id AND m.state='merged' JOIN receipt_changes c ON c.memory_id=m.id AND c.after_state='merged' AND c.after_version=m.current_version_id JOIN receipts r ON r.request_id=c.request_id AND r.action='merge' AND r.status='applied') SELECT EXISTS(SELECT 1 FROM owners o JOIN memories m ON m.id=o.id WHERE m.state='active')", [memory], |r| r.get(0))?)
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
        SourceRef::Version(id) => {
            let v = version(db, id)?;
            if !memory_reference_visible(db, &v.memory_id)? {
                return Err(DataError::Unavailable);
            }
            let current: bool = db.query_row(
                "SELECT state='active' AND current_version_id=?2 FROM memories WHERE id=?1",
                params![v.memory_id, id],
                |r| r.get(0),
            )?;
            (v.title, v.body, v.created_at, current)
        }
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
    /// Current body, original input and receipt commit together.
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
        let saved = create_captured_memory(&tx, &c)?;
        save_receipt(
            &tx,
            &Receipt {
                collection_changes: vec![],
                reason: None,
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
        tx.commit()?;
        Ok(saved)
    }
    pub fn capture_by_id(&self, id: &str) -> Result<RawCapture> {
        raw(&self.connection()?, id)
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
            collection_changes: vec![],
            reason: None,
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
            collection_changes: vec![],
            reason: None,
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
        let restoring_active = changes(&tx, original_request)?
            .into_iter()
            .filter(|change| change.before_state == "active")
            .map(|change| change.memory_id)
            .collect();
        if original.status != "applied"
            || !agent_collections::collection_undo_conflicts(
                &tx,
                &original.collection_changes,
                &restoring_active,
            )?
            .is_empty()
        {
            return Err(DataError::Conflict);
        }
        let collections_before = agent_collections::collection_before(
            &tx,
            original
                .collection_changes
                .iter()
                .map(|change| change.collection_id.clone()),
        )?;
        let mut inverse = if original.memory_id.is_some() {
            undo_inner(&tx, &original)?
        } else if !original.collection_changes.is_empty() {
            tx.execute(
                "UPDATE receipts SET status='undone' WHERE request_id=?",
                [&original.request_id],
            )?;
            Vec::new()
        } else {
            return Err(DataError::Conflict);
        };
        agent_collections::undo_collection_effects(&tx, &original.collection_changes)?;
        for change in &mut inverse {
            change.navigation_after = Some(navigation_snapshot(&tx, &change.memory_id)?);
        }
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
        let main = restored.or_else(|| {
            inverse
                .iter()
                .find(|c| Some(&c.memory_id) == original.memory_id.as_ref())
        });
        let receipt = Receipt {
            collection_changes: agent_collections::collection_effects(&tx, &collections_before)?,
            reason: None,
            request_id: request.into(),
            action: "undo".into(),
            capture_id: original.capture_id,
            memory_id: main.map(|change| change.memory_id.clone()),
            before_version: main.and_then(|change| change.before_version.clone()),
            after_version: main.map(|change| change.after_version.clone()),
            status: "applied".into(),
        };
        save_receipt(&tx, &receipt, &hash)?;
        save_changes(&tx, &receipt.request_id, &inverse)?;
        let input: Option<String> = tx.query_row(
            "SELECT logical_input_id FROM receipts WHERE request_id=?",
            [original_request],
            |r| r.get(0),
        )?;
        if let Some(input) = input {
            // Match grouped undo: invalidate compacted decisions and fence late writes.
            tx.execute("UPDATE conversations SET summary='',summary_through_seq=0 WHERE id IN (SELECT conversation_id FROM turns WHERE id=?1 UNION SELECT json_extract(c.source,'$.conversation_id') FROM receipts r JOIN captures c ON c.id=r.capture_id WHERE r.logical_input_id=?1)",[&input])?;
            tx.execute("UPDATE messages SET status='cancelled',error_code='changes_undone' WHERE turn_id=? AND role='assistant' AND status='processing'",[&input])?;
            tx.execute(
                "UPDATE turns SET active_attempt=NULL,progress=NULL WHERE id=?",
                [&input],
            )?;
        }
        tx.commit()?;
        Ok(receipt)
    }
    pub fn memory_receipts(&self, key: &RecordKey) -> Result<Vec<MemoryReceipt>> {
        key.validate()?;
        let db = self.connection()?;
        let sql = format!(
            "SELECT {RECEIPT_COLUMNS},logical_input_id FROM receipts r WHERE action NOT IN ('capture','undo') AND EXISTS(SELECT 1 FROM memories WHERE id=?1 AND state IN ('active','merged')) AND (EXISTS(SELECT 1 FROM receipt_changes c WHERE c.request_id=r.request_id AND c.memory_id=?1) OR EXISTS(SELECT 1 FROM json_each(r.collection_changes) c WHERE EXISTS(SELECT 1 FROM json_each(json_extract(c.value,'$.added_memory_ids')) m WHERE m.value=?1) OR EXISTS(SELECT 1 FROM json_each(json_extract(c.value,'$.removed_memory_ids')) m WHERE m.value=?1))) ORDER BY rowid DESC LIMIT 20"
        );
        Ok(db
            .prepare(&sql)?
            .query_map([&key.id], |row| {
                Ok(MemoryReceipt {
                    receipt: read_receipt(row)?,
                    logical_input_id: row.get(9)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
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
        // Include historical sources of every effective hidden merge source.
        // Shared sources and independently trashed sources retain their own state.
        trash_memory_sources(&tx, memory, &memory_and_merged_sources(&tx, memory)?)?;
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
        let family = encode(&memory_and_merged_sources(&tx, memory)?)?;
        tx.execute("UPDATE capture_state SET availability='active',trash_owner=NULL WHERE availability='trashed' AND trash_owner IS NOT NULL AND capture_id IN (SELECT vc.capture_id FROM version_captures vc JOIN memory_versions v ON v.id=vc.version_id WHERE v.memory_id IN (SELECT value FROM json_each(?)))",[family])?;
        tx.commit()?;
        Ok(())
    }
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
        if state != "trashed" {
            return Err(DataError::Conflict);
        }
        let family = memory_and_merged_sources(&tx, memory)?;
        // Only erase sources owned by this trash operation; other sources may be shared.
        let sources:Vec<String>=tx.prepare("SELECT capture_id FROM capture_state WHERE availability='trashed' AND trash_owner=?")?.query_map([memory],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        for source in sources {
            if let Some((other, state)) = other_capture_owner(&tx, &source, memory, true)? {
                tx.execute(
                    "UPDATE capture_state SET availability=?2,trash_owner=?3 WHERE capture_id=?1",
                    params![source, state, (state == "trashed").then_some(other)],
                )?;
            } else {
                erase_capture(&tx, &source)?;
            }
        }
        for input in family {
            erase_memory(&tx, &input)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn resolve_source(&self, source: &SourceRef, max_chars: usize) -> Result<Evidence> {
        resolve(&self.connection()?, source, max_chars)
    }
}
fn memory_and_merged_sources(db: &Connection, memory: &str) -> Result<Vec<String>> {
    Ok(db.prepare("WITH RECURSIVE merged(id) AS (SELECT ?1 UNION SELECT c.memory_id FROM merged parent JOIN receipts r ON r.memory_id=parent.id AND r.action='merge' AND r.status='applied' JOIN receipt_changes c ON c.request_id=r.request_id AND c.after_state='merged' JOIN memories m ON m.id=c.memory_id AND m.state='merged' AND m.current_version_id=c.after_version) SELECT id FROM merged")?
        .query_map([memory], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
}

fn trash_memory_sources(db: &Connection, memory: &str, family: &[String]) -> Result<()> {
    let sources: Vec<String> = db.prepare("SELECT DISTINCT vc.capture_id FROM version_captures vc JOIN memory_versions v ON v.id=vc.version_id JOIN capture_state s ON s.capture_id=vc.capture_id WHERE v.memory_id IN (SELECT value FROM json_each(?)) AND s.availability='active'")?
        .query_map([encode(&family)?], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    for source in sources {
        if other_capture_owner(db, &source, memory, false)?.is_none() {
            db.execute("UPDATE capture_state SET availability='trashed',trash_owner=?2 WHERE capture_id=?1", params![source,memory])?;
        }
    }
    Ok(())
}

// A historical source can be shared through another hidden merge source. Transfer
// trash ownership to its surviving visible owner, never to the hidden intermediary.
fn other_capture_owner(
    db: &Connection,
    capture: &str,
    excluded: &str,
    include_trash: bool,
) -> Result<Option<(String, String)>> {
    Ok(db.query_row("WITH RECURSIVE owners(id) AS (SELECT v.memory_id FROM version_captures vc JOIN memory_versions v ON v.id=vc.version_id WHERE vc.capture_id=?1 UNION SELECT r.memory_id FROM owners o JOIN memories m ON m.id=o.id AND m.state='merged' JOIN receipt_changes c ON c.memory_id=m.id AND c.after_state='merged' AND c.after_version=m.current_version_id JOIN receipts r ON r.request_id=c.request_id AND r.action='merge' AND r.status='applied') SELECT m.id,m.state FROM owners o JOIN memories m ON m.id=o.id WHERE m.id!=?2 AND (m.state='active' OR (?3 AND m.state='trashed')) ORDER BY m.state='active' DESC,m.id LIMIT 1", params![capture,excluded,include_trash], |r| Ok((r.get(0)?,r.get(1)?))).optional()?)
}

fn erase_capture(db: &Connection, capture: &str) -> Result<()> {
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
        "UPDATE receipts SET reason=NULL WHERE reason IS NOT NULL AND (memory_id=?1 OR request_id IN (SELECT request_id FROM receipt_changes WHERE memory_id=?1))",
        [memory],
    )?;
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
