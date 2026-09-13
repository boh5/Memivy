use super::{db::*, records::*, *};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

fn sources(db: &Connection, message: &str, cited_only: bool) -> Result<Vec<SourceRef>> {
    Ok(db.prepare("SELECT kind,source_id FROM message_citations WHERE message_id=?1 AND (?2=0 OR cited=1) ORDER BY kind,source_id")?.query_map(params![message,cited_only],|r|SourceRef::from_parts(r.get(0)?,r.get(1)?))?.collect::<rusqlite::Result<_>>()?)
}
fn message(db: &Connection, id: &str) -> Result<Message> {
    let mut m = db.query_row(
        "SELECT m.seq,m.id,m.turn_id,m.role,m.text,m.status,m.error_code,t.follow_ups,t.progress,t.record_only,m.created_at FROM messages m JOIN turns t ON t.id=m.turn_id WHERE m.id=?",
        [id],
        |r| {
            Ok(Message {
                created_at: r.get(10)?,
                seq: r.get(0)?,
                id: r.get(1)?,
                turn_id: r.get(2)?,
                role: r.get(3)?,
                text: r.get(4)?,
                status: r.get(5)?,
                error_code: r.get(6)?,
                citations: vec![],
                followups: serde_json::from_str(&r.get::<_, String>(7)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                receipts: vec![],
                progress: r.get(8)?,
                record_only: r.get(9)?,
            })
        },
    )?;
    if m.role == "assistant" {
        m.receipts = db.prepare(&format!("SELECT {RECEIPT_COLUMNS} FROM receipts WHERE logical_input_id=? AND action!='undo' ORDER BY created_at,request_id"))?
            .query_map([&m.turn_id], read_receipt)?.collect::<rusqlite::Result<_>>()?;
    } else {
        m.followups.clear();
        m.progress = None;
        m.record_only = false;
    }
    m.citations = sources(db, id, true)?
        .into_iter()
        .map(|source| {
            let available = match resolve(db, &source, 1) {
                Ok(_) => true,
                Err(DataError::Unavailable) => false,
                Err(e) => return Err(e),
            };
            Ok(Citation { source, available })
        })
        .collect::<Result<_>>()?;
    Ok(m)
}
fn turn(db: &Connection, id: &str) -> Result<Turn> {
    let user: String = db.query_row(
        "SELECT id FROM messages WHERE turn_id=? AND role='user'",
        [id],
        |r| r.get(0),
    )?;
    let assistant: String = db.query_row(
        "SELECT id FROM messages WHERE turn_id=? AND role='assistant'",
        [id],
        |r| r.get(0),
    )?;
    Ok(Turn {
        id: id.into(),
        user: message(db, &user)?,
        assistant: message(db, &assistant)?,
    })
}
fn read_messages(
    db: &Connection,
    conversation: &str,
    after_seq: i64,
    limit: usize,
) -> Result<Vec<Message>> {
    let ids: Vec<String> = db
        .prepare(
            "SELECT id FROM messages WHERE conversation_id=?1 AND seq>?2 ORDER BY seq LIMIT ?3",
        )?
        .query_map(
            params![conversation, after_seq, limit.clamp(1, 100) as i64],
            |r| r.get(0),
        )?
        .collect::<rusqlite::Result<_>>()?;
    ids.iter().map(|id| message(db, id)).collect()
}
impl MemoryStore {
    pub(super) fn agent_conversation_messages(
        &self,
        conversation: &str,
        after_seq: i64,
        limit: usize,
        manual_saves_offset: usize,
    ) -> Result<Vec<serde_json::Value>> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let messages = read_messages(&tx, conversation, after_seq, limit)?;
        let message_ids = messages
            .iter()
            .map(|message| message.id.clone())
            .collect::<Vec<_>>();
        let mut manual_saves =
            super::agent_mutations::messages_manual_saves(&tx, &message_ids, manual_saves_offset)?;
        messages.into_iter().map(|m| {
            let manual_saves = manual_saves.remove(&m.id).unwrap_or_default();
            Ok(serde_json::json!({"id":m.id,"logical_input_id":m.turn_id,"seq":m.seq,"role":m.role,"text":m.text,"status":m.status,"created_at_ms":m.created_at,"receipts":m.receipts,"manual_saves":manual_saves}))
        }).collect()
    }

    /// Conversation naming gets only bounded, literal user messages, never the
    /// answer, compressed context or retrieved memories used by the main Agent.
    pub(super) fn agent_title_messages(
        &self,
        input: &str,
        attempt: &str,
    ) -> Result<Option<Vec<String>>> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let source: Option<(String, i64)> = tx.query_row(
            "SELECT t.conversation_id,u.seq FROM turns t JOIN conversations c ON c.id=t.conversation_id JOIN messages a ON a.turn_id=t.id AND a.role='assistant' JOIN messages u ON u.turn_id=t.id AND u.role='user' WHERE t.id=?1 AND t.active_attempt=?2 AND a.status='complete' AND c.title_generated=0",
            params![input, attempt], |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        let Some((conversation, through_seq)) = source else {
            return Ok(None);
        };
        let mut messages: Vec<String> = tx.prepare(
            "SELECT substr(text,1,1000) FROM messages WHERE conversation_id=?1 AND role='user' AND seq<=?2 ORDER BY seq DESC LIMIT 6",
        )?.query_map(params![conversation, through_seq], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        messages.reverse();
        Ok(Some(messages))
    }

    pub(super) fn save_generated_agent_title(
        &self,
        input: &str,
        attempt: &str,
        title: &str,
    ) -> Result<bool> {
        valid_text(title, 200)?;
        if title.chars().count() > 40 || title.chars().any(char::is_control) {
            return Err(DataError::Invalid);
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // The same fence covers late cancellation/deletion, and the flag makes
        // concurrent generations a single metadata change. Do not reorder the
        // conversation merely because its auxiliary title arrived later.
        let changed = tx.execute(
            "UPDATE conversations SET title=?3,title_generated=1 WHERE title_generated=0 AND id=(SELECT t.conversation_id FROM turns t JOIN messages a ON a.turn_id=t.id AND a.role='assistant' WHERE t.id=?1 AND t.active_attempt=?2 AND a.status='complete')",
            params![input, attempt, title],
        )? > 0;
        tx.commit()?;
        Ok(changed)
    }

    pub fn create_conversation(&self, conversation: &str, title: &str) -> Result<Conversation> {
        self.create_scoped_conversation(conversation, title, None)
    }
    pub fn create_scoped_conversation(
        &self,
        conversation: &str,
        title: &str,
        collection: Option<&str>,
    ) -> Result<Conversation> {
        valid_id(conversation)?;
        valid_text(title, 200)?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(collection) = collection {
            super::navigation::active_collection(&tx, collection)?;
        }
        if let Some((old, generated)) = tx
            .query_row(
                "SELECT title,title_generated FROM conversations WHERE id=?",
                [conversation],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)),
            )
            .optional()?
        {
            let old_scope: Option<String> = tx
                .query_row(
                    "SELECT collection_id FROM conversation_collections WHERE conversation_id=?",
                    [conversation],
                    |r| r.get(0),
                )
                .optional()?;
            // The creation request may be redelivered after asynchronous naming.
            // Its placeholder no longer equals the mutable generated title.
            if (!generated && old != title) || old_scope.as_deref() != collection {
                return Err(DataError::RequestConflict);
            }
        } else {
            tx.execute(
                "INSERT INTO conversations(id,title,created_at,updated_at) VALUES(?1,?2,?3,?3)",
                params![conversation, title, now()?],
            )?;
        }
        if let Some(collection) = collection {
            tx.execute("INSERT OR IGNORE INTO conversation_collections(conversation_id,collection_id) VALUES(?1,?2)",params![conversation,collection])?;
        }
        tx.commit()?;
        self.conversation(conversation)
    }
    pub fn conversation(&self, conversation: &str) -> Result<Conversation> {
        Ok(self.connection()?.query_row(
            "SELECT id,title,draft,updated_at,(SELECT collection_id FROM conversation_collections cc WHERE cc.conversation_id=conversations.id) FROM conversations WHERE id=?",
            [conversation],
            |r| {
                Ok(Conversation {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    draft: r.get(2)?,
                    updated_at: r.get(3)?,
                    collection_id: r.get(4)?,
                })
            },
        )?)
    }
    pub fn conversations(&self, limit: usize) -> Result<Vec<Conversation>> {
        Ok(self.connection()?.prepare("SELECT id,title,draft,updated_at,(SELECT collection_id FROM conversation_collections cc WHERE cc.conversation_id=conversations.id) FROM conversations ORDER BY updated_at DESC,id LIMIT ?")?.query_map([limit.clamp(1,100) as i64],|r|Ok(Conversation{id:r.get(0)?,title:r.get(1)?,draft:r.get(2)?,updated_at:r.get(3)?,collection_id:r.get(4)?}))?.collect::<rusqlite::Result<_>>()?)
    }
    pub fn save_conversation_draft(&self, conversation: &str, text: &str) -> Result<()> {
        if text.len() > 32 * 1024 {
            return Err(DataError::Invalid);
        }
        if self.connection()?.execute(
            "UPDATE conversations SET draft=?2,updated_at=?3 WHERE id=?1",
            params![conversation, text, now()?],
        )? == 0
        {
            return Err(DataError::Unavailable);
        }
        Ok(())
    }
    pub fn turn(&self, id: &str) -> Result<Turn> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        turn(&tx, id)
    }
    /// Call once when the owning application restarts, after confirming its
    /// previous process has exited. Opening a store (e.g. MCP) never cancels work.
    pub fn recover_interrupted_turns(&self) -> Result<usize> {
        Ok(self.connection()?.execute("UPDATE messages SET status='interrupted',error_code='interrupted' WHERE status='processing'",[])?)
    }
    /// Ordered cursor pagination; older discussion remains available after 100 messages.
    pub fn messages(
        &self,
        conversation: &str,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let mut connection = self.connection()?;
        let db = connection.transaction()?;
        read_messages(&db, conversation, after_seq, limit)
    }
    pub fn recent_messages(
        &self,
        conversation: &str,
        before_seq: Option<i64>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let mut connection = self.connection()?;
        let db = connection.transaction()?;
        let mut ids:Vec<String>=db.prepare("SELECT id FROM messages WHERE conversation_id=?1 AND (?2 IS NULL OR seq<?2) ORDER BY seq DESC LIMIT ?3")?.query_map(params![conversation,before_seq,limit.clamp(1,100) as i64],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        ids.reverse();
        ids.iter().map(|id| message(&db, id)).collect()
    }
    pub fn delete_conversation(&self, conversation: &str) -> Result<()> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM workspace_drafts WHERE key=?",
            [format!("discussion:{conversation}")],
        )?;
        tx.execute(
            "DELETE FROM workspace_drafts WHERE key IN (SELECT 'save:'||id FROM messages WHERE conversation_id=?)",
            [conversation],
        )?;
        // Source availability changes only for memories linked to this
        // conversation; streaming discussion updates must not refresh editors.
        tx.execute(
            "INSERT INTO ui_changes(domain,entity) SELECT DISTINCT 'memory','memory:'||v.memory_id FROM captures c JOIN version_captures vc ON vc.capture_id=c.id JOIN memory_versions v ON v.id=vc.version_id WHERE json_extract(c.source,'$.conversation_id')=?1 AND EXISTS(SELECT 1 FROM conversations WHERE id=?1)",
            [conversation],
        )?;
        tx.execute("DELETE FROM conversations WHERE id=?", [conversation])?;
        tx.commit()?;
        Ok(())
    }
    pub fn capture_citations(&self, capture: &str) -> Result<Vec<Citation>> {
        let mut connection = self.connection()?;
        let db = connection.transaction()?;
        raw(&db, capture)?;
        let references:Vec<SourceRef>=db.prepare("SELECT kind,source_id FROM capture_citations WHERE capture_id=? ORDER BY kind,source_id")?.query_map([capture],|r|SourceRef::from_parts(r.get(0)?,r.get(1)?))?.collect::<rusqlite::Result<_>>()?;
        references
            .into_iter()
            .map(|source| {
                let available = match resolve(&db, &source, 1) {
                    Ok(_) => true,
                    Err(DataError::Unavailable) => false,
                    Err(e) => return Err(e),
                };
                Ok(Citation { source, available })
            })
            .collect()
    }
}
