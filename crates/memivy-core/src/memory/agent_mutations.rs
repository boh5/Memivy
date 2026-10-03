//! Agent memory changes use the same versions, source archives and receipts as
//! manual editing. No capture promotion or background organization is scheduled.
use super::{agent_collections::*, agent_state::*, db::*, records::*, *};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Actor {
    User,
    Ai,
}
impl Actor {
    fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Ai => "ai",
        }
    }
}

struct ChangeRequest {
    request_id: String,
    destination: Destination,
    title: String,
    body: String,
    actor: Actor,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentUndoResult {
    pub receipt: Option<Receipt>,
    pub conflicts: Vec<String>,
    pub collection_conflicts: Vec<String>,
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

fn draft_exists(db: &Connection, memory: &str) -> Result<bool> {
    Ok(db
        .prepare("SELECT 1 FROM workspace_drafts WHERE key=?")?
        .exists([format!("memory:{memory}")])?)
}

fn archive_sources(db: &Connection, input: &str, requested: &[String]) -> Result<Vec<String>> {
    if requested.len() > 16 {
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
        if db.prepare("SELECT 1 FROM captures WHERE id=?")?.exists([message])? {
            return Ok(raw(db, message)?.id);
        }
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
    let receipt = Receipt {
        collection_changes: vec![],
        reason: None,
        request_id: write.request_id.clone(),
        action: v.reason.clone(),
        capture_id: source_ids.first().cloned(),
        memory_id: Some(memory.clone()),
        before_version: v.parent_id.clone(),
        after_version: Some(v.id),
        status: "applied".into(),
    };
    save_receipt(db, &receipt, hash)?;
    db.execute(
        "UPDATE receipts SET logical_input_id=?2 WHERE request_id=?1",
        params![write.request_id, input],
    )?;
    Ok(receipt)
}

#[derive(Clone)]
struct GroupEffect {
    memory_id: String,
    before_version: Option<String>,
    after_version: String,
    before_state: String,
    after_state: String,
    navigation_before: Option<NavigationSnapshot>,
    navigation_after: Option<NavigationSnapshot>,
    continuous: bool,
}

#[derive(Clone)]
struct CollectionGroupEffect {
    change: CollectionChange,
    continuous: bool,
}

fn extend_collection_group(
    group: &mut CollectionGroupEffect,
    effect: CollectionChange,
    valid: bool,
) {
    group.continuous &= valid && effect.before.as_ref() == Some(&group.change.after);
    for memory in effect.added_memory_ids {
        if let Some(index) = group
            .change
            .removed_memory_ids
            .iter()
            .position(|id| id == &memory)
        {
            group.change.removed_memory_ids.remove(index);
        } else if !group.change.added_memory_ids.contains(&memory) {
            group.change.added_memory_ids.push(memory);
        }
    }
    for memory in effect.removed_memory_ids {
        if let Some(index) = group
            .change
            .added_memory_ids
            .iter()
            .position(|id| id == &memory)
        {
            group.change.added_memory_ids.remove(index);
        } else if !group.change.removed_memory_ids.contains(&memory) {
            group.change.removed_memory_ids.push(memory);
        }
    }
    group.change.after = effect.after;
}

fn undo_input(db: &Connection, request: &str, input: &str) -> Result<AgentUndoResult> {
    valid_id(request)?;
    valid_id(input)?;
    let hash = fingerprint(&("agent_undo", input))?;
    if let Some(receipt) = replay(db, request, &hash)? {
        return Ok(AgentUndoResult {
            receipt: Some(receipt),
            conflicts: vec![],
            collection_conflicts: vec![],
        });
    }
    let mut groups: BTreeMap<String, GroupEffect> = BTreeMap::new();
    let mut collection_groups: BTreeMap<String, CollectionGroupEffect> = BTreeMap::new();
    let mut originals = Vec::new();
    let receipts = db.prepare(&format!("SELECT {RECEIPT_COLUMNS} FROM receipts WHERE logical_input_id=? AND action!='undo' ORDER BY rowid"))?
        .query_map([input], read_receipt)?.collect::<rusqlite::Result<Vec<_>>>()?;
    if receipts.is_empty() {
        return Err(DataError::Unavailable);
    }
    for receipt in receipts {
        for effect in changes(db, &receipt.request_id)? {
            let valid = receipt.status == "applied";
            if let Some(group) = groups.get_mut(&effect.memory_id) {
                group.continuous &= valid
                    && effect.before_version.as_ref() == Some(&group.after_version)
                    && effect.before_state == group.after_state
                    && effect.navigation_before == group.navigation_after;
                group.after_version = effect.after_version;
                group.after_state = effect.after_state;
                group.navigation_after = effect.navigation_after;
            } else {
                groups.insert(
                    effect.memory_id.clone(),
                    GroupEffect {
                        memory_id: effect.memory_id,
                        before_version: effect.before_version,
                        after_version: effect.after_version,
                        before_state: effect.before_state,
                        after_state: effect.after_state,
                        navigation_before: effect.navigation_before,
                        navigation_after: effect.navigation_after,
                        continuous: valid,
                    },
                );
            }
        }
        for effect in receipt.collection_changes {
            // A later relationship tool in the same input extends the expected
            // navigation of a body change without creating another body version.
            for memory in &effect.added_memory_ids {
                if let Some(navigation) = groups
                    .get_mut(memory)
                    .and_then(|g| g.navigation_after.as_mut())
                    && !navigation.collections.contains(&effect.collection_id)
                {
                    navigation.collections.push(effect.collection_id.clone());
                    navigation.collections.sort();
                }
            }
            for memory in &effect.removed_memory_ids {
                if let Some(navigation) = groups
                    .get_mut(memory)
                    .and_then(|g| g.navigation_after.as_mut())
                {
                    navigation
                        .collections
                        .retain(|id| id != &effect.collection_id);
                }
            }
            let valid = receipt.status == "applied";
            if let Some(group) = collection_groups.get_mut(&effect.collection_id) {
                extend_collection_group(group, effect, valid);
            } else {
                collection_groups.insert(
                    effect.collection_id.clone(),
                    CollectionGroupEffect {
                        change: effect,
                        continuous: valid,
                    },
                );
            }
        }
        originals.push(receipt.request_id);
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
            || group
                .navigation_after
                .as_ref()
                .map(|expected| navigation_matches(db, &group.memory_id, expected))
                .transpose()?
                .is_some_and(|matches| !matches)
        {
            conflicts.push(group.memory_id.clone());
        }
    }
    let collection_changes: Vec<_> = collection_groups
        .values()
        .map(|group| group.change.clone())
        .collect();
    let restoring_active = groups
        .values()
        .filter(|group| group.before_state == "active")
        .map(|group| group.memory_id.clone())
        .collect();
    let mut collection_conflicts =
        collection_undo_conflicts(db, &collection_changes, &restoring_active)?;
    for (id, group) in &collection_groups {
        if !group.continuous && !collection_conflicts.contains(id) {
            collection_conflicts.push(id.clone());
        }
    }
    if !conflicts.is_empty() || !collection_conflicts.is_empty() {
        return Ok(AgentUndoResult {
            receipt: None,
            conflicts,
            collection_conflicts,
        });
    }
    let undo_collections_before = collection_before(db, collection_groups.keys().cloned())?;
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
        if let Some(before) = &group.navigation_before {
            restore_navigation(db, &group.memory_id, before)?;
        }
        inverses.push(ReceiptChange {
            navigation_before: group.navigation_after.clone(),
            navigation_after: group.navigation_before.clone(),
            memory_id: group.memory_id.clone(),
            before_version: Some(group.after_version.clone()),
            after_version: after,
            before_state: group.after_state.clone(),
            after_state: group.before_state.clone(),
        });
    }
    undo_collection_effects(db, &collection_changes)?;
    for inverse in &mut inverses {
        inverse.navigation_after = Some(navigation_snapshot(db, &inverse.memory_id)?);
    }
    for original in originals {
        db.execute(
            "UPDATE receipts SET status='undone' WHERE request_id=?",
            [original],
        )?;
    }
    let first = inverses.first();
    let receipt = Receipt {
        collection_changes: collection_effects(db, &undo_collections_before)?,
        reason: None,
        request_id: request.into(),
        action: "undo".into(),
        capture_id: None,
        memory_id: first.map(|v| v.memory_id.clone()),
        before_version: first.and_then(|v| v.before_version.clone()),
        after_version: first.map(|v| v.after_version.clone()),
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
        collection_conflicts: vec![],
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
        let staged: MemoryWriteArgs =
            serde_json::from_value(op.arguments.clone()).map_err(|_| DataError::Invalid)?;
        if serde_json::to_value(&staged).map_err(|_| DataError::Invalid)?
            != serde_json::to_value(write).map_err(|_| DataError::Invalid)?
        {
            return Err(DataError::RequestConflict);
        }
        // The staged operation owns the original wire arguments. Once its
        // atomic result exists, replay that result after semantic validation;
        // new optional fields must not change a completed operation's identity.
        if op.result.is_some() {
            if op.receipt.is_none() {
                return Err(DataError::RequestConflict);
            }
            return Ok(op);
        }
        let hash = fingerprint(&("agent_memory", input, write, Actor::Ai))?;
        if let Some(receipt) = replay(&tx, operation_id, &hash)? {
            if op.receipt.as_ref() != Some(&receipt) || op.result.is_none() {
                return Err(DataError::Integrity);
            }
            return Ok(op);
        }
        if !matches!(write.destination, Destination::New) && !write.initial_collections.is_empty() {
            return Err(DataError::Invalid);
        }
        validate_initial_collections(&tx, &write.initial_collections)?;
        let collections_before =
            collection_before(&tx, write.initial_collections.iter().map(|r| r.id.clone()))?;
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
                if tx
                    .prepare("SELECT 1 FROM captures WHERE id=?")?
                    .exists([source])?
                {
                    return raw(&tx, source).map(|capture| capture.text);
                }
                tx.query_row(
                    "SELECT text FROM messages WHERE id=?1 AND conversation_id=?2 AND role='user'",
                    params![source, conversation],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or(DataError::SourceAttribution)
            })?;
        let sources = archive_sources(&tx, input, &message_ids)?;
        if sources.is_empty() && previous.as_ref().is_none_or(|v| v.capture_ids.is_empty()) {
            return Err(DataError::SourceAttribution);
        }
        let change = ChangeRequest {
            request_id: operation_id.into(),
            destination: write.destination.clone(),
            title: write.title.clone(),
            body,
            actor: Actor::Ai,
        };
        let mut receipt = write_memory(&tx, input, &change, &sources, &hash)?;
        let memory_id = receipt.memory_id.clone().ok_or(DataError::Integrity)?;
        for collection in &write.initial_collections {
            set_collection_member(&tx, &collection.id, &memory_id, true)?;
        }
        attach_collection_effects(&tx, &mut receipt, &collections_before)?;
        if !write.initial_collections.is_empty() {
            tx.execute(
                "UPDATE receipt_changes SET navigation_after=?2 WHERE request_id=?1",
                params![
                    operation_id,
                    encode(&navigation_snapshot(&tx, &memory_id)?)?
                ],
            )?;
        }
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
        result["receipt"] = agent_receipt_value(&receipt);
        complete_operation(&tx, input, operation_id, &result, Some(&receipt.request_id))?;
        let result = operation(&tx, input, operation_id)?;
        tx.commit()?;
        Ok(result)
    }

    pub fn merge_agent_memories(
        &self,
        input: &str,
        attempt: &str,
        operation_id: &str,
        merge: &MemoryMergeArgs,
    ) -> Result<AgentOperation> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        agent_fence(&tx, input, attempt)?;
        let op = operation(&tx, input, operation_id)?;
        if op.name != "merge_memories"
            || op.arguments != serde_json::to_value(merge).map_err(|_| DataError::Invalid)?
        {
            return Err(DataError::RequestConflict);
        }
        let hash = fingerprint(&("merge_memories", input, merge))?;
        if let Some(receipt) = replay(&tx, operation_id, &hash)? {
            if op.receipt.as_ref() != Some(&receipt) || op.result.is_none() {
                return Err(DataError::Integrity);
            }
            return Ok(op);
        }
        if op.result.is_some() || merge.target_memory_id == merge.source_memory_id {
            return Err(DataError::Invalid);
        }
        valid_text(&merge.reason, 240)?;
        let target = head(&tx, &merge.target_memory_id, &merge.target_version)?;
        let source = head(&tx, &merge.source_memory_id, &merge.source_version)?;
        if draft_exists(&tx, &target.memory_id)? || draft_exists(&tx, &source.memory_id)? {
            return Err(DataError::Conflict);
        }
        let target_navigation = navigation_snapshot(&tx, &target.memory_id)?;
        let source_navigation = navigation_snapshot(&tx, &source.memory_id)?;
        let collections_before = collection_before(
            &tx,
            target_navigation
                .collections
                .iter()
                .chain(source_navigation.collections.iter())
                .cloned(),
        )?;
        let mut captures = target.capture_ids.clone();
        for capture in &source.capture_ids {
            if !captures.contains(capture) {
                captures.push(capture.clone());
            }
        }
        let write = MemoryWriteArgs {
            destination: Destination::Existing {
                memory_id: target.memory_id.clone(),
                expected_version: target.id.clone(),
            },
            title: merge.title.clone(),
            parts: merge.parts.clone(),
            initial_collections: vec![],
        };
        let (body, _) = super::agent::resolve_memory_write(&write, Some(&target), |id| {
            if id == source.id {
                return Ok(source.body.clone());
            }
            if !captures.iter().any(|capture| capture == id) {
                return Err(DataError::SourceAttribution);
            }
            Ok(raw(&tx, id)?.text)
        })?;
        let merged = Version {
            id: id(),
            memory_id: target.memory_id.clone(),
            parent_id: Some(target.id.clone()),
            title: merge.title.clone(),
            body,
            actor: "ai".into(),
            reason: "append".into(),
            created_at: now()?,
            capture_ids: captures,
        };
        write_version(&tx, &merged)?;
        let mut target_after = target_navigation.clone();
        target_after
            .collections
            .extend(source_navigation.collections.iter().cloned());
        target_after.collections.sort();
        target_after.collections.dedup();
        target_after.pinned =
            Some(target_navigation.pinned == Some(true) || source_navigation.pinned == Some(true));
        restore_navigation(&tx, &target.memory_id, &target_after)?;
        let source_after = NavigationSnapshot {
            collections: vec![],
            pinned: Some(false),
        };
        restore_navigation(&tx, &source.memory_id, &source_after)?;
        tx.execute(
            "UPDATE memories SET state='merged',updated_at=?2 WHERE id=?1",
            params![source.memory_id, now()?],
        )?;
        let mut receipt = Receipt {
            collection_changes: vec![],
            reason: Some(merge.reason.clone()),
            request_id: operation_id.into(),
            action: "merge".into(),
            capture_id: source.capture_ids.first().cloned(),
            memory_id: Some(target.memory_id.clone()),
            before_version: Some(target.id),
            after_version: Some(merged.id.clone()),
            status: "applied".into(),
        };
        save_receipt(&tx, &receipt, &hash)?;
        tx.execute(
            "UPDATE receipts SET logical_input_id=?2 WHERE request_id=?1",
            params![operation_id, input],
        )?;
        attach_collection_effects(&tx, &mut receipt, &collections_before)?;
        tx.execute("UPDATE receipt_changes SET navigation_before=?2,navigation_after=?3 WHERE request_id=?1 AND memory_id=?4",params![operation_id,encode(&target_navigation)?,encode(&target_after)?,target.memory_id])?;
        save_changes(
            &tx,
            operation_id,
            &[ReceiptChange {
                memory_id: source.memory_id.clone(),
                before_version: Some(source.id.clone()),
                after_version: source.id,
                before_state: "active".into(),
                after_state: "merged".into(),
                navigation_before: Some(source_navigation),
                navigation_after: Some(source_after),
            }],
        )?;
        let evidence = resolve_excerpt(
            &tx,
            &SourceRef::Version(merged.id.clone()),
            3000,
            &[],
            Some(0),
        )?;
        let mut result =
            super::agent::evidence_value(&merged.memory_id, evidence, merged.body.chars().count());
        result["receipt"] = agent_receipt_value(&receipt);
        result["merged_memory_id"] = serde_json::json!(source.memory_id);
        complete_operation(&tx, input, operation_id, &result, Some(operation_id))?;
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
        let inputs:Vec<String>=tx.prepare("SELECT r.logical_input_id FROM receipts r WHERE r.logical_input_id IS NOT NULL AND (EXISTS(SELECT 1 FROM receipt_changes c WHERE c.request_id=r.request_id AND c.memory_id=?1) OR EXISTS(SELECT 1 FROM json_each(r.collection_changes) c WHERE EXISTS(SELECT 1 FROM json_each(json_extract(c.value,'$.added_memory_ids')) m WHERE m.value=?1) OR EXISTS(SELECT 1 FROM json_each(json_extract(c.value,'$.removed_memory_ids')) m WHERE m.value=?1))) GROUP BY r.logical_input_id ORDER BY max(r.rowid) DESC")?.query_map([memory_id],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
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
            &serde_json::json!({"receipt":result.receipt.as_ref().map(agent_receipt_value),"conflicts":result.conflicts,"collection_conflicts":result.collection_conflicts}),
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
