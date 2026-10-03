//! Collection operations change navigation relationships, never memory bodies.
use super::{agent_state::*, db::*, records::*, *};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionRef {
    pub id: String,
    pub revision: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionListArgs {
    pub query: Option<String>,
    pub memory_id: Option<String>,
    pub offset: usize,
    pub limit: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollectionPage {
    pub items: Vec<Collection>,
    pub total: usize,
    pub next_offset: Option<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionCreateArgs {
    pub name: String,
    pub description: String,
    pub initial_memory_ids: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionUpdateArgs {
    pub collection_id: String,
    pub expected_revision: i64,
    pub name: String,
    pub description: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionMemberEdit {
    pub collection_id: String,
    pub expected_revision: i64,
    pub add_memory_ids: Vec<String>,
    pub remove_memory_ids: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionMembersArgs {
    pub changes: Vec<CollectionMemberEdit>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionSnapshot {
    pub name: String,
    pub description: String,
    pub revision: i64,
    pub archived: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionChange {
    pub collection_id: String,
    pub before: Option<CollectionSnapshot>,
    pub after: CollectionSnapshot,
    pub added_memory_ids: Vec<String>,
    pub removed_memory_ids: Vec<String>,
}

pub(super) fn collection_snapshot(db: &Connection, id: &str) -> Result<Option<CollectionSnapshot>> {
    Ok(db
        .query_row(
            "SELECT name,description,revision,archived FROM collections WHERE id=?",
            [id],
            |r| {
                Ok(CollectionSnapshot {
                    name: r.get(0)?,
                    description: r.get(1)?,
                    revision: r.get(2)?,
                    archived: r.get(3)?,
                })
            },
        )
        .optional()?)
}
pub(super) fn require_collection_revision(
    db: &Connection,
    reference: &CollectionRef,
) -> Result<()> {
    valid_id(&reference.id)?;
    let current = collection_snapshot(db, &reference.id)?
        .filter(|s| !s.archived)
        .ok_or(DataError::Unavailable)?;
    if current.revision != reference.revision {
        return Err(DataError::Conflict);
    }
    Ok(())
}
pub(super) fn require_active_memory(db: &Connection, memory: &str) -> Result<()> {
    valid_id(memory)?;
    if !db
        .prepare("SELECT 1 FROM memories WHERE id=? AND state='active'")?
        .exists([memory])?
    {
        return Err(DataError::Unavailable);
    }
    Ok(())
}
fn validate_memory_ids(db: &Connection, memories: &[String]) -> Result<()> {
    if memories.len() > 50 || memories.iter().collect::<BTreeSet<_>>().len() != memories.len() {
        return Err(DataError::Invalid);
    }
    for memory in memories {
        require_active_memory(db, memory)?;
    }
    Ok(())
}
pub(super) fn set_collection_member(
    db: &Connection,
    collection: &str,
    memory: &str,
    included: bool,
) -> Result<()> {
    if included {
        db.execute("INSERT OR IGNORE INTO collection_entries(collection_id,kind,record_id) VALUES(?1,'memory',?2)",params![collection,memory])?;
    } else {
        db.execute("DELETE FROM collection_entries WHERE collection_id=?1 AND kind='memory' AND record_id=?2",params![collection,memory])?;
    }
    Ok(())
}
pub(super) fn save_collection_metadata(
    db: &Connection,
    id: &str,
    name: &str,
    description: &str,
    expected: Option<i64>,
) -> Result<()> {
    valid_id(id)?;
    valid_text(name.trim(), 240)?;
    if description.len() > 2400 {
        return Err(DataError::Invalid);
    }
    let old = collection_snapshot(db, id)?;
    if let Some(old) = &old {
        if old.archived {
            return Err(DataError::Unavailable);
        }
        if old.name == name.trim() && old.description == description.trim() {
            return Ok(());
        }
        if expected != Some(old.revision) {
            return Err(DataError::Conflict);
        }
    } else if expected.is_some() {
        return Err(DataError::Unavailable);
    } else if db.query_row(
        "SELECT count(*) FROM collections WHERE archived=0",
        [],
        |r| r.get::<_, i64>(0),
    )? >= 100
    {
        return Err(DataError::NavigationLimit);
    }
    if db.query_row("SELECT EXISTS(SELECT 1 FROM collections WHERE name=?1 COLLATE NOCASE AND id!=?2 AND archived=0)",params![name.trim(),id],|r|r.get::<_,bool>(0))? { return Err(DataError::CollectionName); }
    db.execute("INSERT INTO collections(id,name,description,created_at) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET name=excluded.name,description=excluded.description,revision=collections.revision+1",params![id,name.trim(),description.trim(),now()?])?;
    Ok(())
}
fn read_collection(db: &Connection, id: &str) -> Result<Collection> {
    navigation::active_collection(db, id)?;
    Ok(db.query_row("SELECT c.id,c.name,c.description,c.revision,(SELECT count(*) FROM collection_entries ce JOIN memories m ON m.id=ce.record_id AND ce.kind='memory' WHERE ce.collection_id=c.id AND m.state='active') FROM collections c WHERE c.id=?",[id],|r|Ok(Collection{id:r.get(0)?,name:r.get(1)?,description:r.get(2)?,revision:r.get(3)?,count:r.get(4)?}))?)
}
type CollectionBefore = BTreeMap<String, (Option<CollectionSnapshot>, BTreeSet<String>)>;
pub(super) fn collection_before(
    db: &Connection,
    ids: impl IntoIterator<Item = String>,
) -> Result<CollectionBefore> {
    ids.into_iter().map(|id| {
        let snapshot=collection_snapshot(db,&id)?;
        let members=db.prepare("SELECT record_id FROM collection_entries WHERE collection_id=? AND kind='memory'")?.query_map([&id],|r|r.get(0))?.collect::<rusqlite::Result<BTreeSet<_>>>()?;
        Ok((id,(snapshot,members)))
    }).collect()
}
pub(super) fn collection_effects(
    db: &Connection,
    before: &CollectionBefore,
) -> Result<Vec<CollectionChange>> {
    let mut effects = Vec::new();
    for (id, (snapshot, members)) in before {
        let after = collection_snapshot(db, id)?.ok_or(DataError::Integrity)?;
        if snapshot.as_ref() == Some(&after) {
            continue;
        }
        let current = db
            .prepare(
                "SELECT record_id FROM collection_entries WHERE collection_id=? AND kind='memory'",
            )?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<BTreeSet<String>>>()?;
        effects.push(CollectionChange {
            collection_id: id.clone(),
            before: snapshot.clone(),
            after,
            added_memory_ids: current.difference(members).cloned().collect(),
            removed_memory_ids: members.difference(&current).cloned().collect(),
        });
    }
    Ok(effects)
}
pub(super) fn attach_collection_effects(
    db: &Connection,
    receipt: &mut Receipt,
    before: &CollectionBefore,
) -> Result<()> {
    receipt.collection_changes = collection_effects(db, before)?;
    db.execute(
        "UPDATE receipts SET collection_changes=?2 WHERE request_id=?1",
        params![receipt.request_id, encode(&receipt.collection_changes)?],
    )?;
    Ok(())
}
pub(super) fn collection_undo_conflicts(
    db: &Connection,
    changes: &[CollectionChange],
    restoring_active: &BTreeSet<String>,
) -> Result<Vec<String>> {
    let mut conflicts = Vec::new();
    for change in changes {
        if collection_snapshot(db, &change.collection_id)?.as_ref() != Some(&change.after) {
            conflicts.push(change.collection_id.clone());
            continue;
        }
        let mut unavailable_member = false;
        for memory in &change.removed_memory_ids {
            if !restoring_active.contains(memory)
                && !db
                    .prepare("SELECT 1 FROM memories WHERE id=? AND state='active'")?
                    .exists([memory])?
            {
                unavailable_member = true;
                break;
            }
        }
        if unavailable_member {
            conflicts.push(change.collection_id.clone());
            continue;
        }
        if let Some(before) = change.before.as_ref().filter(|before| !before.archived) {
            let names: Vec<String> = db.prepare("SELECT id FROM collections WHERE id!=?1 AND name=?2 COLLATE NOCASE AND archived=0")?
                .query_map(params![change.collection_id,before.name], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            if names.iter().any(|other| {
                changes
                    .iter()
                    .find(|effect| &effect.collection_id == other)
                    .is_none_or(|effect| {
                        effect.before.as_ref().is_some_and(|snapshot| {
                            !snapshot.archived && snapshot.name.eq_ignore_ascii_case(&before.name)
                        })
                    })
            }) {
                conflicts.push(change.collection_id.clone());
            }
        }
    }
    Ok(conflicts)
}

pub(super) fn undo_collection_effects(db: &Connection, changes: &[CollectionChange]) -> Result<()> {
    let metadata_changes: Vec<_> = changes
        .iter()
        .filter(|change| {
            change.before.as_ref().is_none_or(|before| {
                before.name != change.after.name
                    || before.description != change.after.description
                    || before.archived != change.after.archived
            })
        })
        .collect();
    // The unique active-name index also applies within a transaction. Temporarily
    // release every affected name before restoring any final name, so swaps and
    // a rename followed by creation can be reversed without ordering assumptions.
    for change in &metadata_changes {
        db.execute(
            "UPDATE collections SET archived=1 WHERE id=? AND archived=0",
            [&change.collection_id],
        )?;
    }
    for change in changes {
        for memory in &change.added_memory_ids {
            set_collection_member(db, &change.collection_id, memory, false)?;
        }
        for memory in &change.removed_memory_ids {
            // Memory undo runs first, so a source restored by this same atomic
            // operation is active here. Later trash, purge and merge are conflicts.
            require_active_memory(db, memory).map_err(|error| {
                if error == DataError::Unavailable {
                    DataError::Conflict
                } else {
                    error
                }
            })?;
            set_collection_member(db, &change.collection_id, memory, true)?;
        }
    }
    for change in metadata_changes {
        if let Some(before) = &change.before {
            db.execute("UPDATE collections SET name=?2,description=?3,archived=?4,revision=revision+1 WHERE id=?1",params![change.collection_id,before.name,before.description,before.archived])?;
        } else {
            // Keep the existing discussion references and durable history intact.
            db.execute(
                "UPDATE collections SET revision=revision+1 WHERE id=?",
                [&change.collection_id],
            )?;
        }
    }
    Ok(())
}

pub(super) fn validate_initial_collections(
    db: &Connection,
    references: &[CollectionRef],
) -> Result<()> {
    if references.len() > 20
        || references
            .iter()
            .map(|r| &r.id)
            .collect::<BTreeSet<_>>()
            .len()
            != references.len()
    {
        return Err(DataError::Invalid);
    }
    for reference in references {
        require_collection_revision(db, reference)?;
    }
    Ok(())
}
fn start_operation(
    db: &Connection,
    input: &str,
    attempt: &str,
    operation_id: &str,
    name: &str,
    args: &impl Serialize,
) -> Result<AgentOperation> {
    agent_fence(db, input, attempt)?;
    let op = operation(db, input, operation_id)?;
    if op.name != name
        || op.arguments != serde_json::to_value(args).map_err(|_| DataError::Invalid)?
    {
        return Err(DataError::RequestConflict);
    }
    Ok(op)
}
/// Tool results need enough facts to verify a write and continue with current
/// revisions. Full before/after snapshots stay in the durable UI receipt.
pub(super) fn agent_receipt_value(receipt: &Receipt) -> serde_json::Value {
    json!({
        "request_id":receipt.request_id,"action":receipt.action,"status":receipt.status,
        "memory_id":receipt.memory_id,"before_version":receipt.before_version,"after_version":receipt.after_version,
        "collection_changes":receipt.collection_changes.iter().map(|change| json!({
            "collection_id":change.collection_id,"revision":change.after.revision,
            "added_count":change.added_memory_ids.len(),"removed_count":change.removed_memory_ids.len()
        })).collect::<Vec<_>>()
    })
}

fn finish_operation(
    db: &Connection,
    input: &str,
    operation_id: &str,
    name: &str,
    args: &impl Serialize,
    before: &CollectionBefore,
) -> Result<AgentOperation> {
    let effects = collection_effects(db, before)?;
    let collections: Vec<_> = before
        .keys()
        .map(|id| read_collection(db, id))
        .collect::<Result<_>>()?;
    let receipt = if effects.is_empty() {
        None
    } else {
        let receipt = Receipt {
            collection_changes: effects,
            request_id: operation_id.into(),
            action: name.into(),
            reason: None,
            capture_id: None,
            memory_id: None,
            before_version: None,
            after_version: None,
            status: "applied".into(),
        };
        save_receipt(db, &receipt, &fingerprint(&(name, input, args))?)?;
        db.execute(
            "UPDATE receipts SET logical_input_id=?2 WHERE request_id=?1",
            params![operation_id, input],
        )?;
        Some(receipt)
    };
    let summaries: Vec<_> = collections.iter().map(|collection| json!({"id":collection.id,"name":collection.name,"revision":collection.revision,"count":collection.count})).collect();
    let result = json!({"changed":receipt.is_some(),"collections":summaries,"receipt":receipt.as_ref().map(agent_receipt_value)});
    complete_operation(
        db,
        input,
        operation_id,
        &result,
        receipt.as_ref().map(|r| r.request_id.as_str()),
    )?;
    operation(db, input, operation_id)
}
impl MemoryStore {
    pub fn list_agent_collections(&self, args: &CollectionListArgs) -> Result<CollectionPage> {
        if args.limit == 0
            || args.limit > 20
            || args.offset > 10000
            || args.query.as_ref().is_some_and(|q| q.len() > 240)
        {
            return Err(DataError::Invalid);
        }
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        if let Some(memory) = &args.memory_id {
            require_active_memory(&tx, memory)?;
        }
        let query = args.query.as_deref().unwrap_or("").trim();
        let ids:Vec<String>=tx.prepare("SELECT c.id FROM collections c WHERE c.archived=0 AND (?1='' OR instr(lower(c.name),lower(?1))>0 OR instr(lower(c.description),lower(?1))>0) AND (?2 IS NULL OR EXISTS(SELECT 1 FROM collection_entries ce WHERE ce.collection_id=c.id AND ce.kind='memory' AND ce.record_id=?2)) ORDER BY CASE WHEN c.name=?1 COLLATE NOCASE THEN 0 ELSE 1 END,c.name COLLATE NOCASE,c.id")?.query_map(params![query,args.memory_id],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        let total = ids.len();
        let items = ids
            .iter()
            .skip(args.offset)
            .take(args.limit)
            .map(|id| read_collection(&tx, id))
            .collect::<Result<_>>()?;
        let next = args.offset + args.limit;
        Ok(CollectionPage {
            items,
            total,
            next_offset: (next < total).then_some(next),
        })
    }
    pub fn read_agent_collection(&self, id: &str) -> Result<Collection> {
        read_collection(&self.connection()?, id)
    }
    pub fn create_agent_collection(
        &self,
        input: &str,
        attempt: &str,
        operation_id: &str,
        args: &CollectionCreateArgs,
    ) -> Result<AgentOperation> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let op = start_operation(&tx, input, attempt, operation_id, "create_collection", args)?;
        if op.result.is_some() {
            return Ok(op);
        }
        validate_memory_ids(&tx, &args.initial_memory_ids)?;
        let collection = id();
        let before = collection_before(&tx, [collection.clone()])?;
        save_collection_metadata(&tx, &collection, &args.name, &args.description, None)?;
        for memory in &args.initial_memory_ids {
            set_collection_member(&tx, &collection, memory, true)?;
        }
        let result =
            finish_operation(&tx, input, operation_id, "create_collection", args, &before)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn update_agent_collection(
        &self,
        input: &str,
        attempt: &str,
        operation_id: &str,
        args: &CollectionUpdateArgs,
    ) -> Result<AgentOperation> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let op = start_operation(&tx, input, attempt, operation_id, "update_collection", args)?;
        if op.result.is_some() {
            return Ok(op);
        }
        let before = collection_before(&tx, [args.collection_id.clone()])?;
        save_collection_metadata(
            &tx,
            &args.collection_id,
            &args.name,
            &args.description,
            Some(args.expected_revision),
        )?;
        let result =
            finish_operation(&tx, input, operation_id, "update_collection", args, &before)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn update_agent_collection_members(
        &self,
        input: &str,
        attempt: &str,
        operation_id: &str,
        args: &CollectionMembersArgs,
    ) -> Result<AgentOperation> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let op = start_operation(
            &tx,
            input,
            attempt,
            operation_id,
            "update_collection_members",
            args,
        )?;
        if op.result.is_some() {
            return Ok(op);
        }
        if args.changes.is_empty()
            || args.changes.len() > 20
            || args
                .changes
                .iter()
                .map(|c| &c.collection_id)
                .collect::<BTreeSet<_>>()
                .len()
                != args.changes.len()
        {
            return Err(DataError::Invalid);
        }
        for change in &args.changes {
            require_collection_revision(
                &tx,
                &CollectionRef {
                    id: change.collection_id.clone(),
                    revision: change.expected_revision,
                },
            )?;
            validate_memory_ids(&tx, &change.add_memory_ids)?;
            validate_memory_ids(&tx, &change.remove_memory_ids)?;
            if change
                .add_memory_ids
                .iter()
                .any(|id| change.remove_memory_ids.contains(id))
            {
                return Err(DataError::Invalid);
            }
        }
        let before = collection_before(&tx, args.changes.iter().map(|c| c.collection_id.clone()))?;
        for change in &args.changes {
            for memory in &change.remove_memory_ids {
                set_collection_member(&tx, &change.collection_id, memory, false)?;
            }
            for memory in &change.add_memory_ids {
                set_collection_member(&tx, &change.collection_id, memory, true)?;
            }
        }
        let result = finish_operation(
            &tx,
            input,
            operation_id,
            "update_collection_members",
            args,
            &before,
        )?;
        tx.commit()?;
        Ok(result)
    }
    pub fn collection_agent_changes(&self, collection: &str) -> Result<Vec<AgentChangeGroup>> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        valid_id(collection)?;
        if collection_snapshot(&tx, collection)?.is_none() {
            return Err(DataError::Unavailable);
        }
        let inputs:Vec<String>=tx.prepare("SELECT r.logical_input_id FROM receipts r,json_each(r.collection_changes) c WHERE json_extract(c.value,'$.collection_id')=? AND r.logical_input_id IS NOT NULL GROUP BY r.logical_input_id ORDER BY max(r.rowid) DESC")?.query_map([collection],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        inputs.into_iter().map(|input_id|{
            let receipts=tx.prepare(&format!("SELECT {RECEIPT_COLUMNS} FROM receipts WHERE logical_input_id=? ORDER BY rowid"))?.query_map([&input_id],read_receipt)?.collect::<rusqlite::Result<_>>()?;
            Ok(AgentChangeGroup{input_id,receipts})
        }).collect()
    }
}
