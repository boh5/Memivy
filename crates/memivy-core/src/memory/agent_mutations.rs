//! Agent memory changes use the same versions, source archives and receipts as
//! manual editing. No capture promotion or background organization is scheduled.
use super::{agent_state::*, db::*, records::*, *};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentUndoResult {
    pub receipt: Option<Receipt>,
    pub conflicts: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentChangeGroup {
    pub input_id: String,
    pub receipts: Vec<Receipt>,
}

#[derive(Debug, Serialize)]
pub(super) struct AgentManualSave {
    input_id: String,
    status: String,
    memory_id: Option<String>,
    action: String,
}

#[derive(Debug, Default, Serialize)]
pub(super) struct AgentManualSavePage {
    items: Vec<AgentManualSave>,
    total: usize,
    next_offset: Option<usize>,
}

/// Manual saves use their own undo group, distinct from the automatic turn.
/// Associate them with the reviewed assistant message through its durable source.
pub(super) fn messages_manual_saves(
    db: &Connection,
    messages: &[String],
    offset: usize,
) -> Result<BTreeMap<String, AgentManualSavePage>> {
    let start = i64::try_from(offset).map_err(|_| DataError::Invalid)?;
    let end = start.checked_add(20).ok_or(DataError::Invalid)?;
    let mut groups: BTreeMap<String, AgentManualSavePage> = BTreeMap::new();
    if messages.is_empty() {
        return Ok(groups);
    }
    // Count and page in one query, ordered by immutable receipt insertion order.
    // Retain the first row for totals even if the requested page is past the end.
    let mut statement = db.prepare(
        "WITH manual AS (SELECT r.request_id,r.status,r.memory_id,r.action,json_extract(c.source,'$.message_id') AS message_id,ROW_NUMBER() OVER (PARTITION BY json_extract(c.source,'$.message_id') ORDER BY r.rowid) AS position,COUNT(*) OVER (PARTITION BY json_extract(c.source,'$.message_id')) AS total FROM receipts r JOIN captures c ON c.id=r.capture_id WHERE r.logical_input_id=r.request_id AND r.action!='undo' AND json_extract(c.source,'$.kind')='conversation' AND json_extract(c.source,'$.message_id') IN (SELECT value FROM json_each(?1))) SELECT request_id,status,memory_id,action,message_id,position,total FROM manual WHERE position=1 OR (position>?2 AND position<=?3) ORDER BY message_id,position"
    )?;
    let rows = statement.query_map(params![encode(&messages)?, start, end], |r| {
        Ok((
            AgentManualSave {
                input_id: r.get(0)?,
                status: r.get(1)?,
                memory_id: r.get(2)?,
                action: r.get(3)?,
            },
            r.get::<_, String>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, i64>(6)?,
        ))
    })?;
    for row in rows {
        let (item, message, position, total) = row?;
        let group = groups.entry(message).or_default();
        group.total = usize::try_from(total).map_err(|_| DataError::Integrity)?;
        group.next_offset = (end < total).then_some(end as usize);
        if position > start && position <= end {
            group.items.push(item);
        }
    }
    Ok(groups)
}

fn memberships(db: &Connection, memory: &str) -> Result<Vec<String>> {
    Ok(db.prepare("SELECT collection_id FROM collection_entries WHERE kind='memory' AND record_id=? ORDER BY collection_id")?.query_map([memory],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?)
}

fn draft_exists(db: &Connection, memory: &str) -> Result<bool> {
    Ok(db
        .prepare("SELECT 1 FROM workspace_drafts WHERE key=?")?
        .exists([format!("memory:{memory}")])?)
}

fn archive_messages(db: &Connection, input: &str, requested: &[String]) -> Result<Vec<String>> {
    if requested.is_empty() || requested.len() > 16 {
        return Err(DataError::Invalid);
    }
    let mut message_ids = requested.to_vec();
    message_ids.sort();
    message_ids.dedup();
    let conversation: String = db.query_row(
        "SELECT conversation_id FROM turns WHERE id=?",
        [input],
        |r| r.get(0),
    )?;
    message_ids.iter().map(|message| {
        valid_id(message)?;
        let (text,origin,created_at):(String,Option<String>,i64)=db.query_row("SELECT m.text,t.input_origin,m.created_at FROM messages m JOIN turns t ON t.id=m.turn_id WHERE m.id=?1 AND m.conversation_id=?2 AND m.role='user'",params![message,conversation],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        let raw_origin=origin.as_deref().map(decode::<Origin>).transpose()?.unwrap_or(Origin::User{app:"Memivy".into(),project:None,uri:None});
        let Origin::User{app,project,uri}=raw_origin else {return Err(DataError::Integrity)};
        let archived=insert_capture_at(db,message,&text,&Origin::Discussion{conversation_id:conversation.clone(),message_id:message.clone(),app,project,uri},created_at)?;
        Ok(archived.id)
    }).collect()
}

fn write_memory(
    db: &Connection,
    input: &str,
    write: &ChangeRequest,
    source_ids: &[String],
    hash: &[u8],
) -> Result<Receipt> {
    valid_text(&write.title, 200)?;
    valid_text(&write.body, 128 * 1024)?;
    let (memory, previous) = match &write.destination {
        Destination::New => {
            let memory = id();
            db.execute(
                "INSERT INTO memories(id,created_at,updated_at) VALUES(?1,?2,?2)",
                params![memory, now()?],
            )?;
            (memory, None)
        }
        Destination::Existing {
            memory_id,
            expected_version,
        } => {
            valid_id(memory_id)?;
            valid_id(expected_version)?;
            if draft_exists(db, memory_id)? {
                return Err(DataError::Conflict);
            }
            (
                memory_id.clone(),
                Some(head(db, memory_id, expected_version)?),
            )
        }
    };
    let before_memberships = memberships(db, &memory)?;
    let mut captures = previous
        .as_ref()
        .map(|v| v.capture_ids.clone())
        .unwrap_or_default();
    for source in source_ids {
        if !captures.contains(source) {
            captures.push(source.clone());
        }
    }
    let v = Version {
        id: id(),
        memory_id: memory.clone(),
        parent_id: previous.as_ref().map(|v| v.id.clone()),
        title: write.title.clone(),
        body: write.body.clone(),
        actor: write.actor.as_str().into(),
        reason: if previous.is_some() { "edit" } else { "create" }.into(),
        created_at: now()?,
        capture_ids: captures,
    };
    write_version(db, &v)?;
    if previous.is_none() {
        // Archived topics do not block remembering, nor silently acquire notes.
        let collection:Option<String>=db.query_row("SELECT cc.collection_id FROM turns t JOIN conversation_collections cc ON cc.conversation_id=t.conversation_id JOIN collections c ON c.id=cc.collection_id WHERE t.id=? AND c.archived=0",[input],|r|r.get(0)).optional()?;
        if let Some(collection) = collection {
            db.execute("INSERT INTO collection_entries(collection_id,kind,record_id) VALUES(?1,'memory',?2)",params![collection,memory])?;
        }
    }
    let receipt = Receipt {
        request_id: write.request_id.clone(),
        action: v.reason.clone(),
        capture_id: source_ids.first().cloned(),
        memory_id: Some(memory.clone()),
        before_version: v.parent_id.clone(),
        after_version: Some(v.id),
        status: "applied".into(),
    };
    save_receipt(db, &receipt, hash)?;
    db.execute("UPDATE receipts SET logical_input_id=?2,before_memberships=?3,after_memberships=?4 WHERE request_id=?1",params![write.request_id,input,encode(&before_memberships)?,encode(&memberships(db,&memory)?)?])?;
    Ok(receipt)
}

#[derive(Clone)]
struct GroupEffect {
    memory_id: String,
    before_version: Option<String>,
    after_version: String,
    before_state: String,
    after_state: String,
    before_memberships: Vec<String>,
    after_memberships: Vec<String>,
    continuous: bool,
}

fn undo_input(db: &Connection, request: &str, input: &str) -> Result<AgentUndoResult> {
    valid_id(request)?;
    valid_id(input)?;
    let hash = fingerprint(&("agent_undo", input))?;
    if let Some(receipt) = replay(db, request, &hash)? {
        return Ok(AgentUndoResult {
            receipt: Some(receipt),
            conflicts: vec![],
        });
    }
    let mut groups: BTreeMap<String, GroupEffect> = BTreeMap::new();
    let mut originals = Vec::new();
    let rows=db.prepare("SELECT r.request_id,r.status,c.memory_id,c.before_version,c.after_version,c.before_state,c.after_state,r.before_memberships,r.after_memberships FROM receipts r JOIN receipt_changes c ON c.request_id=r.request_id WHERE r.logical_input_id=? AND r.action!='undo' ORDER BY r.rowid,c.memory_id")?.query_map([input],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    if rows.is_empty() {
        return Err(DataError::Unavailable);
    }
    for (
        receipt,
        status,
        memory,
        before,
        after,
        before_state,
        after_state,
        before_memberships,
        after_memberships,
    ) in rows
    {
        let before_memberships: Vec<String> = decode(&before_memberships)?;
        let after_memberships: Vec<String> = decode(&after_memberships)?;
        let valid = status == "applied";
        if let Some(group) = groups.get_mut(&memory) {
            group.continuous &= valid
                && before.as_ref() == Some(&group.after_version)
                && before_state == group.after_state
                && before_memberships == group.after_memberships;
            group.after_version = after;
            group.after_state = after_state;
            group.after_memberships = after_memberships;
        } else {
            groups.insert(
                memory.clone(),
                GroupEffect {
                    memory_id: memory,
                    before_version: before,
                    after_version: after,
                    before_state,
                    after_state,
                    before_memberships,
                    after_memberships,
                    continuous: valid,
                },
            );
        }
        if !originals.contains(&receipt) {
            originals.push(receipt);
        }
    }
    let mut conflicts = Vec::new();
    for group in groups.values() {
        let current: Option<(String, String)> = db
            .query_row(
                "SELECT current_version_id,state FROM memories WHERE id=?",
                [&group.memory_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let valid = current.as_ref().is_some_and(|(version, state)| {
            version == &group.after_version && state == &group.after_state
        });
        if !group.continuous
            || !valid
            || draft_exists(db, &group.memory_id)?
            || memberships(db, &group.memory_id)? != group.after_memberships
        {
            conflicts.push(group.memory_id.clone());
        }
    }
    if !conflicts.is_empty() {
        return Ok(AgentUndoResult {
            receipt: None,
            conflicts,
        });
    }
    let mut inverses = Vec::new();
    for group in groups.values() {
        let after = if group.before_state == "undone" {
            db.execute(
                "UPDATE memories SET state='undone',updated_at=?2 WHERE id=?1",
                params![group.memory_id, now()?],
            )?;
            group.after_version.clone()
        } else {
            let mut restored = version(
                db,
                group
                    .before_version
                    .as_deref()
                    .ok_or(DataError::Integrity)?,
            )?;
            restored.id = id();
            restored.parent_id = Some(group.after_version.clone());
            restored.actor = "user".into();
            restored.reason = "undo".into();
            restored.created_at = now()?;
            db.execute(
                "UPDATE memories SET state=?2 WHERE id=?1",
                params![group.memory_id, group.before_state],
            )?;
            write_version(db, &restored)?;
            restored.id
        };
        db.execute(
            "DELETE FROM collection_entries WHERE kind='memory' AND record_id=?",
            [&group.memory_id],
        )?;
        for collection in &group.before_memberships {
            db.execute("INSERT INTO collection_entries(collection_id,kind,record_id) VALUES(?1,'memory',?2)",params![collection,group.memory_id])?;
        }
        inverses.push(ReceiptChange {
            memory_id: group.memory_id.clone(),
            before_version: Some(group.after_version.clone()),
            after_version: after,
            before_state: group.after_state.clone(),
            after_state: group.before_state.clone(),
        });
    }
    for original in originals {
        db.execute(
            "UPDATE receipts SET status='undone' WHERE request_id=?",
            [original],
        )?;
    }
    let first = inverses.first().ok_or(DataError::Integrity)?;
    let receipt = Receipt {
        request_id: request.into(),
        action: "undo".into(),
        capture_id: None,
        memory_id: Some(first.memory_id.clone()),
        before_version: first.before_version.clone(),
        after_version: Some(first.after_version.clone()),
        status: "applied".into(),
    };
    save_receipt(db, &receipt, &hash)?;
    save_changes(db, request, &inverses)?;
    // A compacted old decision cannot survive the user's explicit reversal.
    db.execute("UPDATE conversations SET summary='',summary_through_seq=0 WHERE id IN (SELECT conversation_id FROM turns WHERE id=?1 UNION SELECT json_extract(c.source,'$.conversation_id') FROM receipts r JOIN captures c ON c.id=r.capture_id WHERE r.logical_input_id=?1)",[input])?;
    // Undo also fences any still-running producer of these changes. Otherwise a
    // late tool could immediately write back the content the user just reversed.
    db.execute("UPDATE messages SET status='cancelled',error_code='changes_undone' WHERE turn_id=? AND role='assistant' AND status='processing'",[input])?;
    db.execute(
        "UPDATE turns SET active_attempt=NULL,progress=NULL WHERE id=?",
        [input],
    )?;
    Ok(AgentUndoResult {
        receipt: Some(receipt),
        conflicts: vec![],
    })
}

impl MemoryStore {
    pub fn apply_agent_memory(
        &self,
        input: &str,
        attempt: &str,
        operation_id: &str,
        write: &MemoryWriteArgs,
    ) -> Result<AgentOperation> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        agent_fence(&tx, input, attempt)?;
        let op = operation(&tx, input, operation_id)?;
        if op.name != "write_memory" {
            return Err(DataError::Invalid);
        }
        if op.arguments != serde_json::to_value(write).map_err(|_| DataError::Invalid)? {
            return Err(DataError::RequestConflict);
        }
        let hash = fingerprint(&("agent_memory", input, write, Actor::Ai))?;
        if let Some(receipt) = replay(&tx, operation_id, &hash)? {
            if op.receipt.as_ref() != Some(&receipt) || op.result.is_none() {
                return Err(DataError::Integrity);
            }
            return Ok(op);
        }
        if op.result.is_some() {
            return Err(DataError::RequestConflict);
        }
        let previous = match &write.destination {
            Destination::New => None,
            Destination::Existing {
                memory_id,
                expected_version,
            } => Some(head(&tx, memory_id, expected_version)?),
        };
        let conversation: String = tx.query_row(
            "SELECT conversation_id FROM turns WHERE id=?",
            [input],
            |row| row.get(0),
        )?;
        let (body, message_ids) =
            super::agent::resolve_memory_write(write, previous.as_ref(), |source| {
                tx.query_row(
                    "SELECT text FROM messages WHERE id=?1 AND conversation_id=?2 AND role='user'",
                    params![source, conversation],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or(DataError::SourceAttribution)
            })?;
        let sources = archive_messages(&tx, input, &message_ids)?;
        let change = ChangeRequest {
            request_id: operation_id.into(),
            capture_id: sources
                .first()
                .cloned()
                .ok_or(DataError::SourceAttribution)?,
            destination: write.destination.clone(),
            title: write.title.clone(),
            body,
            actor: Actor::Ai,
        };
        let receipt = write_memory(&tx, input, &change, &sources, &hash)?;
        let committed = version(
            &tx,
            receipt
                .after_version
                .as_deref()
                .ok_or(DataError::Integrity)?,
        )?;
        let evidence = resolve_excerpt(
            &tx,
            &SourceRef::Version(committed.id.clone()),
            3000,
            &[],
            Some(0),
        )?;
        let mut result = super::agent::evidence_value(
            &committed.memory_id,
            evidence,
            committed.body.chars().count(),
        );
        result["receipt"] = serde_json::to_value(&receipt).map_err(|_| DataError::Invalid)?;
        complete_operation(&tx, input, operation_id, &result, Some(&receipt.request_id))?;
        let result = operation(&tx, input, operation_id)?;
        tx.commit()?;
        Ok(result)
    }

    pub fn agent_input_receipts(&self, input: &str) -> Result<Vec<Receipt>> {
        Ok(self
            .connection()?
            .prepare(&format!(
                "SELECT {RECEIPT_COLUMNS} FROM receipts WHERE logical_input_id=? ORDER BY rowid"
            ))?
            .query_map([input], read_receipt)?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// A memory retains the change-group handle even when its conversation is
    /// deleted. Return each complete group, not a misleading single-note undo.
    pub fn memory_agent_changes(&self, memory_id: &str) -> Result<Vec<AgentChangeGroup>> {
        valid_id(memory_id)?;
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        if !tx
            .prepare("SELECT 1 FROM memories WHERE id=? AND state='active'")?
            .exists([memory_id])?
        {
            return Err(DataError::Unavailable);
        }
        let inputs:Vec<String>=tx.prepare("SELECT r.logical_input_id FROM receipts r JOIN receipt_changes c ON c.request_id=r.request_id WHERE c.memory_id=? AND r.logical_input_id IS NOT NULL GROUP BY r.logical_input_id ORDER BY max(r.rowid) DESC")?.query_map([memory_id],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        inputs.into_iter().map(|input_id|{
            let receipts=tx.prepare(&format!("SELECT {RECEIPT_COLUMNS} FROM receipts WHERE logical_input_id=? ORDER BY rowid"))?.query_map([&input_id],read_receipt)?.collect::<rusqlite::Result<_>>()?;
            Ok(AgentChangeGroup{input_id,receipts})
        }).collect()
    }

    /// This explicit UI operation remains available after its conversation was deleted.
    pub fn undo_agent_input(&self, request: &str, original_input: &str) -> Result<AgentUndoResult> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = undo_input(&tx, request, original_input)?;
        tx.commit()?;
        Ok(result)
    }

    /// Tool invocation of the same operation, fenced with the current attempt.
    pub fn undo_agent_operation(
        &self,
        input: &str,
        attempt: &str,
        operation_id: &str,
        original_input: &str,
    ) -> Result<AgentOperation> {
        if input == original_input {
            return Err(DataError::Invalid);
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        agent_fence(&tx, input, attempt)?;
        let op = operation(&tx, input, operation_id)?;
        if op.name != "undo_changes" {
            return Err(DataError::Invalid);
        }
        if op.result.is_some() {
            return Ok(op);
        }
        let result = undo_input(&tx, operation_id, original_input)?;
        complete_operation(
            &tx,
            input,
            operation_id,
            &serde_json::to_value(&result).map_err(|_| DataError::Invalid)?,
            result.receipt.as_ref().map(|r| r.request_id.as_str()),
        )?;
        let op = operation(&tx, input, operation_id)?;
        tx.commit()?;
        Ok(op)
    }

    /// Explicitly saving selected text does not invoke a model or organization job.
    /// Existing destinations append the reviewed text and preserve their old body.
    pub fn save_agent_text(
        &self,
        request: &str,
        input: &str,
        text: &str,
        title: &str,
        destination: &Destination,
    ) -> Result<Receipt> {
        valid_id(request)?;
        valid_text(text, 128 * 1024)?;
        valid_text(title, 200)?;
        let hash = fingerprint(&("save_agent_text", input, text, title, destination))?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(receipt) = replay(&tx, request, &hash)? {
            return Ok(receipt);
        }
        let (conversation, message): (String, String) = tx.query_row(
            "SELECT conversation_id,id FROM messages WHERE turn_id=? AND role='assistant' AND status!='processing' AND length(trim(text))>0",
            [input],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let draft_key = format!("save:{message}");
        let draft_payload: Option<String> = tx
            .query_row(
                "SELECT payload FROM workspace_drafts WHERE key=?",
                [&draft_key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(payload) = &draft_payload {
            let draft: WorkspaceDraft = decode(payload)?;
            if draft.request_id != request
                || draft.body != text
                || draft.title != title
                || draft.destination.as_ref() != Some(destination)
            {
                return Err(DataError::Conflict);
            }
        }
        let capture = insert_capture(
            &tx,
            request,
            text,
            &Origin::Conversation {
                conversation_id: conversation.clone(),
                message_id: message.clone(),
                message_role: "assistant".into(),
                confirmed_by: "user".into(),
            },
        )?;
        tx.execute("INSERT INTO capture_citations(capture_id,kind,source_id) SELECT ?1,kind,source_id FROM message_citations WHERE message_id=?2 AND cited=1",params![capture.id,message])?;
        let body = match destination {
            Destination::New => text.to_owned(),
            Destination::Existing {
                memory_id,
                expected_version,
            } => format!(
                "{}\n\n{}",
                head(&tx, memory_id, expected_version)?.body,
                text
            ),
        };
        let write = ChangeRequest {
            request_id: request.into(),
            capture_id: capture.id.clone(),
            destination: destination.clone(),
            title: title.into(),
            body,
            actor: Actor::User,
        };
        let receipt = write_memory(&tx, input, &write, &[capture.id], &hash)?;
        // Explicit save has its own request identity, not the automatic turn group.
        tx.execute(
            "UPDATE receipts SET logical_input_id=?1 WHERE request_id=?1",
            [request],
        )?;
        // Saving an already-compacted answer changes the state of that history.
        // Uncovered messages will carry their new receipt in the normal tail.
        tx.execute(
            "UPDATE conversations SET summary='',summary_through_seq=0 WHERE id=?1 AND summary_through_seq>=(SELECT seq FROM messages WHERE id=?2)",
            params![conversation, message],
        )?;
        if let Some(payload) = draft_payload {
            tx.execute(
                "DELETE FROM workspace_drafts WHERE key=?1 AND payload=?2",
                params![draft_key, payload],
            )?;
        }
        tx.commit()?;
        Ok(receipt)
    }
}
