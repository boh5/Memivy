//! Durable execution state for a conversation turn, separate from memory content.
use super::{db::*, records::*, *};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentOperation {
    pub operation_id: String,
    pub call_id: String,
    pub name: String,
    pub arguments: Value,
    pub result: Option<Value>,
    pub receipt: Option<Receipt>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentExecution {
    pub input_id: String,
    pub attempt_id: String,
    pub conversation_id: String,
    pub user_message_id: String,
    pub assistant_message_id: String,
    pub input_text: String,
    pub text: String,
    pub state: String,
    pub protocol: Vec<Value>,
    pub focused_memory_ids: Vec<String>,
    pub follow_ups: Vec<String>,
    pub record_only: bool,
    pub maintenance_paused: bool,
    pub operations: Vec<AgentOperation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentConversationContext {
    pub summary: String,
    pub summary_through_seq: i64,
    pub memory_paused: bool,
    pub receipt_revision: String,
}

#[derive(Clone, Debug)]
pub struct AgentHistorySnapshot {
    pub context: AgentConversationContext,
    pub messages: Vec<Value>,
}

fn conversation_context(db: &Connection, conversation: &str) -> Result<AgentConversationContext> {
    let mut context = db.query_row(
        "SELECT summary,summary_through_seq,memory_paused FROM conversations WHERE id=?",
        [conversation],
        |r| {
            Ok(AgentConversationContext {
                summary: r.get(0)?,
                summary_through_seq: r.get(1)?,
                memory_paused: r.get(2)?,
                receipt_revision: String::new(),
            })
        },
    )?;
    context.receipt_revision = receipt_revision(db, conversation)?;
    Ok(context)
}

fn receipt_revision(db: &Connection, conversation: &str) -> Result<String> {
    let states:Vec<(String,String)>=db.prepare("SELECT r.request_id,r.status FROM receipts r LEFT JOIN captures c ON c.id=r.capture_id WHERE r.logical_input_id IN (SELECT id FROM turns WHERE conversation_id=?1) OR json_extract(c.source,'$.conversation_id')=?1 ORDER BY r.request_id")?.query_map([conversation],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok(fingerprint(&states)?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMaintenance {
    ThisTurn,
    PauseConversation,
    ResumeConversation,
}

pub(super) fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T> {
    serde_json::from_str(value).map_err(|_| DataError::Integrity)
}

/// A late model result must pass this check inside the same write transaction.
pub(super) fn agent_fence(db: &Connection, input: &str, attempt: &str) -> Result<()> {
    if db.query_row(
        "SELECT EXISTS(SELECT 1 FROM turns t JOIN messages m ON m.turn_id=t.id AND m.role='assistant' WHERE t.id=?1 AND t.active_attempt=?2 AND m.status='processing')",
        params![input, attempt], |r| r.get::<_, bool>(0),
    )? { Ok(()) } else { Err(DataError::Conflict) }
}

pub(super) fn operation(
    db: &Connection,
    input: &str,
    operation_id: &str,
) -> Result<AgentOperation> {
    let (call_id,name,arguments,result,receipt_id): (String,String,String,Option<String>,Option<String>) = db.query_row(
        "SELECT call_id,name,arguments,result,receipt_id FROM agent_operations WHERE input_id=?1 AND operation_id=?2",
        params![input,operation_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
    )?;
    let receipt = receipt_id
        .map(|receipt_id| {
            db.query_row(
                &format!("SELECT {RECEIPT_COLUMNS} FROM receipts WHERE request_id=?"),
                [&receipt_id],
                read_receipt,
            )
            .map_err(DataError::from)
        })
        .transpose()?;
    Ok(AgentOperation {
        operation_id: operation_id.into(),
        call_id,
        name,
        arguments: decode(&arguments)?,
        result: result.as_deref().map(decode).transpose()?,
        receipt,
    })
}

pub(super) fn complete_operation(
    db: &Connection,
    input: &str,
    operation_id: &str,
    result: &Value,
    receipt: Option<&str>,
) -> Result<()> {
    let encoded = encode(result)?;
    if encoded.len() > 256 * 1024 {
        return Err(DataError::Invalid);
    }
    let old: Option<String> = db.query_row(
        "SELECT result FROM agent_operations WHERE input_id=?1 AND operation_id=?2",
        params![input, operation_id],
        |r| r.get(0),
    )?;
    if let Some(old) = old {
        if decode::<Value>(&old)? != *result {
            return Err(DataError::RequestConflict);
        }
        return Ok(());
    }
    db.execute(
        "UPDATE agent_operations SET result=?3,receipt_id=?4 WHERE input_id=?1 AND operation_id=?2",
        params![input, operation_id, encoded, receipt],
    )?;
    Ok(())
}

fn execution(db: &Connection, input: &str) -> Result<AgentExecution> {
    let mut value = db.query_row(
        "SELECT t.id,COALESCE(t.active_attempt,''),t.conversation_id,u.id,a.id,u.text,a.text,a.status,t.record_only,(t.maintenance_paused OR c.memory_paused) FROM turns t JOIN conversations c ON c.id=t.conversation_id JOIN messages u ON u.turn_id=t.id AND u.role='user' JOIN messages a ON a.turn_id=t.id AND a.role='assistant' WHERE t.id=?",
        [input], |r| Ok(AgentExecution {input_id:r.get(0)?,attempt_id:r.get(1)?,conversation_id:r.get(2)?,user_message_id:r.get(3)?,assistant_message_id:r.get(4)?,input_text:r.get(5)?,text:r.get(6)?,state:r.get(7)?,record_only:r.get(8)?,maintenance_paused:r.get(9)?,protocol:vec![],focused_memory_ids:vec![],follow_ups:vec![],operations:vec![]}),
    )?;
    let (protocol, focus, followups): (String, String, String) = db.query_row(
        "SELECT protocol_messages,focused_memory_ids,follow_ups FROM turns WHERE id=?",
        [input],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    value.protocol = decode(&protocol)?;
    value.focused_memory_ids = decode(&focus)?;
    value.follow_ups = decode(&followups)?;
    let ids: Vec<String> = db
        .prepare("SELECT operation_id FROM agent_operations WHERE input_id=? ORDER BY rowid")?
        .query_map([input], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    value.operations = ids
        .iter()
        .map(|id| operation(db, input, id))
        .collect::<Result<_>>()?;
    Ok(value)
}

impl MemoryStore {
    /// Persist the user's exact expression once, before any model/network work.
    pub fn begin_agent_input(
        &self,
        input: &str,
        attempt: &str,
        conversation: &str,
        text: &str,
        focused_memory_ids: &[String],
        origin: Option<&Origin>,
    ) -> Result<AgentExecution> {
        valid_id(input)?;
        valid_id(attempt)?;
        valid_id(conversation)?;
        valid_text(text, 32 * 1024)?;
        if focused_memory_ids.len() > 32 {
            return Err(DataError::Invalid);
        }
        for memory in focused_memory_ids {
            valid_id(memory)?;
        }
        let default_origin = Origin::User {
            app: "Memivy".into(),
            project: None,
            uri: None,
        };
        let origin = origin.unwrap_or(&default_origin);
        if !matches!(origin, Origin::User { .. }) {
            return Err(DataError::Invalid);
        }
        validate_origin(origin)?;
        let hash = fingerprint(&(conversation, text, focused_memory_ids, origin))?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old: Option<Vec<u8>> = tx
            .query_row("SELECT fingerprint FROM turns WHERE id=?", [input], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(old) = old {
            if old != hash {
                return Err(DataError::RequestConflict);
            }
            let value = execution(&tx, input)?;
            if value.state == "complete" || value.attempt_id == attempt {
                return Ok(value);
            }
            if tx
                .prepare("SELECT 1 FROM receipts WHERE logical_input_id=? AND status='undone'")?
                .exists([input])?
            {
                return Err(DataError::Conflict);
            }
            if tx.prepare("SELECT 1 FROM messages WHERE conversation_id=?1 AND status='processing' AND turn_id!=?2")?.exists(params![conversation,input])? { return Err(DataError::Conflict); }
            // Discard only this failed generation's uncommitted text tail on an
            // explicit retry. Cancel/recovery themselves retain all shown text.
            tx.execute("UPDATE messages SET status='processing',error_code=NULL,text=substr(text,1,(SELECT checkpoint_text_chars FROM turns WHERE id=?1)) WHERE turn_id=?1 AND role='assistant'",[input])?;
            tx.execute(
                "UPDATE turns SET active_attempt=?2,progress=NULL WHERE id=?1",
                params![input, attempt],
            )?;
        } else {
            if !tx
                .prepare("SELECT 1 FROM conversations WHERE id=?")?
                .exists([conversation])?
            {
                return Err(DataError::Unavailable);
            }
            if tx
                .prepare("SELECT 1 FROM messages WHERE conversation_id=? AND status='processing'")?
                .exists([conversation])?
            {
                return Err(DataError::Conflict);
            }
            tx.execute("INSERT INTO turns(id,conversation_id,fingerprint,active_attempt,focused_memory_ids,input_origin) VALUES(?1,?2,?3,?4,?5,?6)",params![input,conversation,hash,attempt,encode(&focused_memory_ids)?,encode(origin)?])?;
            let created = now()?;
            tx.execute("INSERT INTO messages(id,turn_id,conversation_id,role,text,status,created_at) VALUES(?1,?2,?3,'user',?4,'complete',?5)",params![id(),input,conversation,text,created])?;
            tx.execute("INSERT INTO messages(id,turn_id,conversation_id,role,text,status,created_at) VALUES(?1,?2,?3,'assistant','','processing',?4)",params![id(),input,conversation,created])?;
            // Clear only the submitted draft; another window may already hold a
            // different draft. Workspace draft CAS remains owned by its caller.
            tx.execute("UPDATE conversations SET draft=CASE WHEN draft=?2 THEN '' ELSE draft END,updated_at=?3 WHERE id=?1",params![conversation,text,created])?;
        }
        let result = execution(&tx, input)?;
        tx.commit()?;
        Ok(result)
    }

    pub fn agent_execution(&self, input: &str) -> Result<AgentExecution> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        execution(&tx, input)
    }

    pub fn retry_agent_input(&self, input: &str, attempt: &str) -> Result<AgentExecution> {
        let saved = self.agent_execution(input)?;
        let origin: Option<String> = self.connection()?.query_row(
            "SELECT input_origin FROM turns WHERE id=?",
            [input],
            |r| r.get(0),
        )?;
        let origin = origin.as_deref().map(decode::<Origin>).transpose()?;
        self.begin_agent_input(
            input,
            attempt,
            &saved.conversation_id,
            &saved.input_text,
            &saved.focused_memory_ids,
            origin.as_ref(),
        )
    }

    /// Within one input the wire prefix is append-only. Summarization happens
    /// before the next input and cannot delete this input's committed tool calls.
    pub fn checkpoint_agent(&self, input: &str, attempt: &str, protocol: &[Value]) -> Result<()> {
        self.persist_agent_checkpoint(input, attempt, protocol, None)
    }

    /// Accept a prepared first context only if none of its receipt state changed
    /// while retrieval or compression ran. A conflict leaves the prefix empty.
    pub fn checkpoint_agent_start(
        &self,
        input: &str,
        attempt: &str,
        protocol: &[Value],
        expected_revision: &str,
    ) -> Result<()> {
        self.persist_agent_checkpoint(input, attempt, protocol, Some(expected_revision))
    }

    fn persist_agent_checkpoint(
        &self,
        input: &str,
        attempt: &str,
        protocol: &[Value],
        expected_revision: Option<&str>,
    ) -> Result<()> {
        let encoded = encode(&protocol)?;
        if encoded.len() > 2 * 1024 * 1024 {
            return Err(DataError::Invalid);
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        agent_fence(&tx, input, attempt)?;
        let old: String = tx.query_row(
            "SELECT protocol_messages FROM turns WHERE id=?",
            [input],
            |r| r.get(0),
        )?;
        let old: Vec<Value> = decode(&old)?;
        if let Some(expected) = expected_revision {
            let conversation: String = tx.query_row(
                "SELECT conversation_id FROM turns WHERE id=?",
                [input],
                |r| r.get(0),
            )?;
            if !old.is_empty() || receipt_revision(&tx, &conversation)? != expected {
                return Err(DataError::Conflict);
            }
        }
        if !protocol.starts_with(&old) {
            return Err(DataError::RequestConflict);
        }
        tx.execute("UPDATE turns SET protocol_messages=?2,checkpoint_text_chars=(SELECT length(text) FROM messages WHERE turn_id=?1 AND role='assistant') WHERE id=?1",params![input,encoded])?;
        tx.commit()?;
        Ok(())
    }

    pub fn stage_agent_operation(
        &self,
        input: &str,
        attempt: &str,
        call_id: &str,
        name: &str,
        arguments: &Value,
    ) -> Result<AgentOperation> {
        valid_text(call_id, 200)?;
        valid_text(name, 100)?;
        let encoded = encode(arguments)?;
        if encoded.len() > 192 * 1024 || !arguments.is_object() {
            return Err(DataError::Invalid);
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        agent_fence(&tx, input, attempt)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT operation_id FROM agent_operations WHERE input_id=?1 AND call_id=?2",
                params![input, call_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            let old = operation(&tx, input, &existing)?;
            if old.name != name || old.arguments != *arguments {
                return Err(DataError::RequestConflict);
            }
            return Ok(old);
        }
        let protocol: String = tx.query_row(
            "SELECT protocol_messages FROM turns WHERE id=?",
            [input],
            |r| r.get(0),
        )?;
        let protocol: Vec<Value> = decode(&protocol)?;
        let declared = protocol
            .iter()
            .filter(|m| m["role"] == "assistant")
            .filter_map(|m| m["tool_calls"].as_array())
            .flatten()
            .any(|c| {
                c["id"] == call_id
                    && c["function"]["name"] == name
                    && c["function"]["arguments"]
                        .as_str()
                        .and_then(|v| serde_json::from_str::<Value>(v).ok())
                        .as_ref()
                        == Some(arguments)
            });
        if !declared {
            return Err(DataError::Invalid);
        }
        let operation_id = id();
        tx.execute("INSERT INTO agent_operations(operation_id,input_id,call_id,name,arguments,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![operation_id,input,call_id,name,encoded,now()?])?;
        let result = operation(&tx, input, &operation_id)?;
        tx.commit()?;
        Ok(result)
    }

    pub fn complete_agent_operation(
        &self,
        input: &str,
        attempt: &str,
        operation_id: &str,
        result: &Value,
    ) -> Result<AgentOperation> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        agent_fence(&tx, input, attempt)?;
        complete_operation(&tx, input, operation_id, result, None)?;
        let result = operation(&tx, input, operation_id)?;
        tx.commit()?;
        Ok(result)
    }

    pub fn append_agent_text(&self, input: &str, attempt: &str, delta: &str) -> Result<()> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        agent_fence(&tx, input, attempt)?;
        let length: i64 = tx.query_row(
            "SELECT length(CAST(text AS BLOB)) FROM messages WHERE turn_id=? AND role='assistant'",
            [input],
            |r| r.get(0),
        )?;
        if length as usize + delta.len() > 128 * 1024 {
            return Err(DataError::Invalid);
        }
        tx.execute(
            "UPDATE messages SET text=text||?2 WHERE turn_id=?1 AND role='assistant'",
            params![input, delta],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_agent_progress(
        &self,
        input: &str,
        attempt: &str,
        progress: Option<&str>,
    ) -> Result<()> {
        if let Some(progress) = progress {
            valid_text(progress, 200)?;
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        agent_fence(&tx, input, attempt)?;
        tx.execute(
            "UPDATE turns SET progress=?2 WHERE id=?1",
            params![input, progress],
        )?;
        // The existing message update trigger is the UI invalidation boundary.
        tx.execute(
            "UPDATE messages SET error_code=error_code WHERE turn_id=? AND role='assistant'",
            [input],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn finish_agent_input(
        &self,
        input: &str,
        attempt: &str,
        record_only: bool,
        follow_ups: &[String],
    ) -> Result<()> {
        if follow_ups.len() > 3
            || follow_ups
                .iter()
                .any(|s| s.trim().is_empty() || s.len() > 2000)
        {
            return Err(DataError::Invalid);
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        agent_fence(&tx, input, attempt)?;
        if tx
            .prepare("SELECT 1 FROM agent_operations WHERE input_id=? AND result IS NULL")?
            .exists([input])?
        {
            return Err(DataError::Conflict);
        }
        tx.execute(
            "UPDATE turns SET record_only=?2,follow_ups=?3,progress=NULL WHERE id=?1",
            params![input, record_only, encode(&follow_ups)?],
        )?;
        tx.execute("UPDATE messages SET status='complete',error_code=NULL WHERE turn_id=? AND role='assistant'",[input])?;
        tx.execute("UPDATE conversations SET updated_at=?2 WHERE id=(SELECT conversation_id FROM turns WHERE id=?1)",params![input,now()?])?;
        tx.commit()?;
        Ok(())
    }

    /// Suggestions are optional post-completion metadata. A failed generation
    /// never changes the successful answer or the already committed receipts.
    pub fn save_agent_followups(
        &self,
        input: &str,
        attempt: &str,
        follow_ups: &[String],
    ) -> Result<()> {
        if !(2..=3).contains(&follow_ups.len())
            || follow_ups
                .iter()
                .any(|s| s.trim().is_empty() || s.len() > 2000)
        {
            return Err(DataError::Invalid);
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current=tx.prepare("SELECT 1 FROM turns t JOIN messages m ON m.turn_id=t.id AND m.role='assistant' WHERE t.id=?1 AND t.active_attempt=?2 AND m.status='complete'")?.exists(params![input,attempt])?;
        if !current {
            return Err(DataError::Conflict);
        }
        tx.execute(
            "UPDATE turns SET follow_ups=?2 WHERE id=?1",
            params![input, encode(&follow_ups)?],
        )?;
        tx.execute(
            "UPDATE messages SET text=text WHERE turn_id=? AND role='assistant'",
            [input],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn stop_agent_input(
        &self,
        input: &str,
        attempt: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        if !matches!(status, "failed" | "cancelled" | "interrupted")
            || error.is_some_and(|s| s.len() > 100)
        {
            return Err(DataError::Invalid);
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        agent_fence(&tx, input, attempt)?;
        tx.execute(
            "UPDATE messages SET status=?2,error_code=?3 WHERE turn_id=?1 AND role='assistant'",
            params![input, status, error],
        )?;
        tx.execute("UPDATE turns SET progress=NULL WHERE id=?", [input])?;
        tx.commit()?;
        Ok(())
    }

    pub fn agent_conversation_context(
        &self,
        conversation: &str,
    ) -> Result<AgentConversationContext> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        conversation_context(&tx, conversation)
    }

    /// Summary coverage, its remaining source messages, and undo state must all
    /// come from one read snapshot; independently reading them can omit history.
    pub fn agent_history_snapshot(&self, input: &str) -> Result<AgentHistorySnapshot> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let (conversation, before): (String, i64) = tx.query_row(
            "SELECT conversation_id,seq FROM messages WHERE turn_id=? AND role='user'",
            [input],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let context = conversation_context(&tx, &conversation)?;
        let mut messages: Vec<Value> = tx.prepare("SELECT id,seq,role,text,status,turn_id,created_at FROM messages WHERE conversation_id=?1 AND seq>?2 AND seq<?3 ORDER BY seq")?
            .query_map(params![conversation,context.summary_through_seq,before],|r|Ok(json!({"id":r.get::<_,String>(0)?,"seq":r.get::<_,i64>(1)?,"role":r.get::<_,String>(2)?,"text":r.get::<_,String>(3)?,"status":r.get::<_,String>(4)?,"logical_input_id":r.get::<_,String>(5)?,"created_at_ms":r.get::<_,i64>(6)?})))?
            .collect::<rusqlite::Result<_>>()?;
        let mut statement = tx.prepare(&format!(
            "SELECT {RECEIPT_COLUMNS} FROM receipts WHERE logical_input_id=? ORDER BY rowid"
        ))?;
        let message_ids = messages
            .iter()
            .map(|message| {
                message["id"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or(DataError::Integrity)
            })
            .collect::<Result<Vec<_>>>()?;
        let mut manual_saves = super::agent_mutations::messages_manual_saves(&tx, &message_ids, 0)?;
        for message in &mut messages {
            let input = message["logical_input_id"]
                .as_str()
                .ok_or(DataError::Integrity)?;
            let receipts: Vec<Receipt> = statement
                .query_map([input], read_receipt)?
                .collect::<rusqlite::Result<_>>()?;
            message["memory_changes_undone"] = json!(receipts.iter().any(|r| r.status == "undone"));
            message["receipts"] = json!(receipts);
            message["manual_saves"] = json!(
                manual_saves
                    .remove(message["id"].as_str().ok_or(DataError::Integrity)?)
                    .unwrap_or_default()
            );
        }
        Ok(AgentHistorySnapshot { context, messages })
    }

    pub fn save_agent_summary(
        &self,
        input: &str,
        attempt: &str,
        summary: &str,
        through_seq: i64,
        expected_revision: &str,
    ) -> Result<()> {
        valid_text(summary, 32 * 1024)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        agent_fence(&tx, input, attempt)?;
        let conversation: String = tx.query_row(
            "SELECT conversation_id FROM turns WHERE id=?",
            [input],
            |r| r.get(0),
        )?;
        if receipt_revision(&tx, &conversation)? != expected_revision {
            return Err(DataError::Conflict);
        }
        let covered=tx.prepare("SELECT 1 FROM messages WHERE conversation_id=(SELECT conversation_id FROM turns WHERE id=?1) AND seq=?2 AND seq<(SELECT seq FROM messages WHERE turn_id=?1 AND role='user') AND status='complete'")?.exists(params![input,through_seq])?;
        if !covered {
            return Err(DataError::Invalid);
        }
        tx.execute("UPDATE conversations SET summary=?2,summary_through_seq=?3 WHERE id=(SELECT conversation_id FROM turns WHERE id=?1)",params![input,summary,through_seq])?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_agent_maintenance(
        &self,
        input: &str,
        attempt: &str,
        mode: AgentMaintenance,
    ) -> Result<()> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        agent_fence(&tx, input, attempt)?;
        match mode {
            AgentMaintenance::ThisTurn => {
                tx.execute("UPDATE turns SET maintenance_paused=1 WHERE id=?", [input])?;
            }
            AgentMaintenance::PauseConversation => {
                tx.execute("UPDATE conversations SET memory_paused=1 WHERE id=(SELECT conversation_id FROM turns WHERE id=?)",[input])?;
            }
            AgentMaintenance::ResumeConversation => {
                tx.execute("UPDATE conversations SET memory_paused=0 WHERE id=(SELECT conversation_id FROM turns WHERE id=?)",[input])?;
                tx.execute("UPDATE turns SET maintenance_paused=0 WHERE id=?", [input])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

/// A one-time data move. Competing unsent drafts become ordinary conversations
/// so the current UI can recover them without keeping a legacy draft reader.
pub(super) fn migrate_agent_drafts(db: &Connection) -> Result<()> {
    // Imported read-only turns did not mutate memories, so they have no committed memory-tool
    // boundary to replay. Preserve their messages and start the new protocol
    // from the same logical input on explicit retry. Old citations cannot tell
    // selected material apart from automatic retrieval, so do not invent focus.
    let old_inputs:Vec<(String,String,String)>=db.prepare("SELECT t.id,t.conversation_id,m.text FROM turns t JOIN messages m ON m.turn_id=t.id AND m.role='user' WHERE t.input_origin IS NULL")?.query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    let origin = Origin::User {
        app: "Memivy".into(),
        project: None,
        uri: None,
    };
    let focus: Vec<String> = vec![];
    for (input, conversation, text) in old_inputs {
        db.execute(
            "UPDATE turns SET fingerprint=?2,input_origin=?3 WHERE id=?1",
            params![
                input,
                fingerprint(&(&conversation, &text, &focus, &origin))?,
                encode(&origin)?
            ],
        )?;
    }
    for (old, new) in [
        ("capture", "input"),
        ("question", "input"),
        ("quick_capture", "quick_input"),
        ("quick_question", "quick_input"),
    ] {
        let payload: Option<String> = db
            .query_row(
                "SELECT payload FROM workspace_drafts WHERE key=?",
                [old],
                |r| r.get(0),
            )
            .optional()?;
        let Some(payload) = payload else { continue };
        let mut value: Value = decode(&payload)?;
        let has_text = value["body"].as_str().is_some_and(|v| !v.is_empty());
        if has_text {
            if db
                .prepare("SELECT 1 FROM workspace_drafts WHERE key=?")?
                .exists([new])?
            {
                preserve_unsent_draft(db, value)?;
            } else {
                let object = value.as_object_mut().ok_or(DataError::Integrity)?;
                object.remove("conclusion");
                object.insert("key".into(), Value::String(new.into()));
                db.execute(
                    "INSERT INTO workspace_drafts(key,payload) VALUES(?1,?2)",
                    params![new, encode(&value)?],
                )?;
            }
        }
        db.execute("DELETE FROM workspace_drafts WHERE key=?", [old])?;
    }
    let old_saves: Vec<(String, String)> = db
        .prepare(
            "SELECT key,payload FROM workspace_drafts WHERE key LIKE 'conclusion:%' ORDER BY key",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (old, payload) in old_saves {
        let mut value: Value = decode(&payload)?;
        let review = value
            .get("conclusion")
            .cloned()
            .ok_or(DataError::Integrity)?;
        if let Some(merged) = review["merged_body"].as_str().filter(|s| !s.is_empty()) {
            // Old merged text is a whole-document edit. Restoring it as append
            // text would duplicate the target body when the user later saves.
            let target = review["destination"]["memory_id"]
                .as_str()
                .ok_or(DataError::Integrity)?;
            let expected = review["destination"]["expected_version"]
                .as_str()
                .ok_or(DataError::Integrity)?;
            let key = format!("memory:{target}");
            let mut editor = value.clone();
            let object = editor.as_object_mut().ok_or(DataError::Integrity)?;
            object.remove("conclusion");
            object.remove("destination");
            object.insert("key".into(), Value::String(key.clone()));
            object.insert("body".into(), Value::String(merged.into()));
            object.insert("expected_version".into(), Value::String(expected.into()));
            let target_available = db
                .prepare("SELECT 1 FROM memories WHERE id=? AND state='active'")?
                .exists([target])?;
            if !target_available
                || db
                    .prepare("SELECT 1 FROM workspace_drafts WHERE key=?")?
                    .exists([&key])?
            {
                preserve_unsent_draft(db, editor)?;
            } else {
                db.execute(
                    "INSERT INTO workspace_drafts(key,payload) VALUES(?1,?2)",
                    params![key, encode(&editor)?],
                )?;
            }
        }
        let new = format!(
            "save:{}",
            old.strip_prefix("conclusion:")
                .ok_or(DataError::Integrity)?
        );
        let object = value.as_object_mut().ok_or(DataError::Integrity)?;
        object.remove("conclusion");
        object.insert("destination".into(), review["destination"].clone());
        object.insert("key".into(), Value::String(new.clone()));
        if db
            .prepare("SELECT 1 FROM workspace_drafts WHERE key=?")?
            .exists([&new])?
        {
            preserve_unsent_draft(db, value)?;
        } else {
            db.execute(
                "INSERT INTO workspace_drafts(key,payload) VALUES(?1,?2)",
                params![new, encode(&value)?],
            )?;
        }
        db.execute("DELETE FROM workspace_drafts WHERE key=?", [old])?;
    }
    Ok(())
}

fn preserve_unsent_draft(db: &Connection, mut value: Value) -> Result<()> {
    let conversation = id();
    let key = format!("discussion:{conversation}");
    let body = value["body"]
        .as_str()
        .ok_or(DataError::Integrity)?
        .to_owned();
    db.execute("INSERT INTO conversations(id,title,draft,created_at,updated_at) VALUES(?1,'未发送的草稿',?2,?3,?3)",params![conversation,body,now()?])?;
    let object = value.as_object_mut().ok_or(DataError::Integrity)?;
    object.remove("conclusion");
    object.remove("destination");
    object.insert("key".into(), Value::String(key.clone()));
    db.execute(
        "INSERT INTO workspace_drafts(key,payload) VALUES(?1,?2)",
        params![key, encode(&value)?],
    )?;
    Ok(())
}
