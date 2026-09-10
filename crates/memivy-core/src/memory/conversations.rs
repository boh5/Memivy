use super::{db::*, records::*, *};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

fn sources(db: &Connection, message: &str, cited_only: bool) -> Result<Vec<SourceRef>> {
    Ok(db.prepare("SELECT kind,source_id FROM message_citations WHERE message_id=?1 AND (?2=0 OR cited=1) ORDER BY kind,source_id")?.query_map(params![message,cited_only],|r|SourceRef::from_parts(r.get(0)?,r.get(1)?))?.collect::<rusqlite::Result<_>>()?)
}
fn message(db: &Connection, id: &str) -> Result<Message> {
    let mut m = db.query_row(
        "SELECT seq,id,turn_id,role,text,status,error_code,answer FROM messages WHERE id=?",
        [id],
        |r| {
            Ok(Message {
                seq: r.get(0)?,
                id: r.get(1)?,
                turn_id: r.get(2)?,
                role: r.get(3)?,
                text: r.get(4)?,
                status: r.get(5)?,
                error_code: r.get(6)?,
                citations: vec![],
                answer: r
                    .get::<_, Option<String>>(7)?
                    .map(|s| serde_json::from_str(&s).map_err(|_| rusqlite::Error::InvalidQuery))
                    .transpose()?,
            })
        },
    )?;
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
impl MemoryStore {
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
        if let Some(old) = tx
            .query_row(
                "SELECT title FROM conversations WHERE id=?",
                [conversation],
                |r| r.get::<_, String>(0),
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
            if old != title || old_scope.as_deref() != collection {
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
    /// A new turn ID is also the attempt ID. Retrying delivery reuses it; retrying
    /// a failed model call uses a new ID so late completions cannot replace it.
    pub fn start_turn(
        &self,
        request: &str,
        conversation: &str,
        question: &str,
        evidence: &[SourceRef],
    ) -> Result<Turn> {
        valid_id(request)?;
        valid_text(question, 32 * 1024)?;
        if evidence.len() > 8 {
            return Err(DataError::Invalid);
        }
        let hash = fingerprint(&(conversation, question, evidence))?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = tx
            .query_row("SELECT fingerprint FROM turns WHERE id=?", [request], |r| {
                r.get::<_, Vec<u8>>(0)
            })
            .optional()?
        {
            if old != hash {
                return Err(DataError::RequestConflict);
            }
            return turn(&tx, request);
        }
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE id=?)",
            [conversation],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(DataError::Unavailable);
        }
        let running: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE conversation_id=? AND status='processing')",
            [conversation],
            |r| r.get(0),
        )?;
        if running {
            return Err(DataError::Conflict);
        }
        for source in evidence {
            if !super::search::current_source(&tx, source)? {
                return Err(DataError::Unavailable);
            }
            resolve(&tx, source, 1)?;
        }
        tx.execute(
            "INSERT INTO turns(id,conversation_id,fingerprint) VALUES(?1,?2,?3)",
            params![request, conversation, hash],
        )?;
        let user = id();
        let assistant = id();
        let created = now()?;
        tx.execute("INSERT INTO messages(id,turn_id,conversation_id,role,text,status,created_at) VALUES(?1,?2,?3,'user',?4,'complete',?5)",params![user,request,conversation,question,created])?;
        tx.execute("INSERT INTO messages(id,turn_id,conversation_id,role,text,status,created_at) VALUES(?1,?2,?3,'assistant','','processing',?4)",params![assistant,request,conversation,created])?;
        for source in evidence {
            let (kind, id) = source.parts();
            tx.execute("INSERT OR IGNORE INTO message_citations(message_id,kind,source_id) VALUES(?1,?2,?3)",params![assistant,kind,id])?;
        }
        tx.execute(
            "UPDATE conversations SET draft='',updated_at=?2 WHERE id=?1",
            params![conversation, created],
        )?;
        let result = turn(&tx, request)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn turn(&self, id: &str) -> Result<Turn> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        turn(&tx, id)
    }
    /// Persist only citations from the bounded evidence actually supplied to the
    /// request. Citation text is resolved on reads, never copied into a cache.
    pub fn finish_turn(&self, request: &str, text: &str, citations: &[SourceRef]) -> Result<bool> {
        self.finish_answer(request, text, citations, None)
    }
    pub(super) fn finish_answer(
        &self,
        request: &str,
        text: &str,
        citations: &[SourceRef],
        answer: Option<&DiscussionAnswer>,
    ) -> Result<bool> {
        valid_text(text, 128 * 1024)?;
        if citations.len() > 8 {
            return Err(DataError::Invalid);
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = turn(&tx, request)?;
        if current.assistant.status != "processing" {
            return Ok(false);
        }
        let allowed = sources(&tx, &current.assistant.id, false)?;
        if citations.iter().any(|s| !allowed.contains(s)) {
            return Err(DataError::Invalid);
        }
        let scope: Option<String> = tx.query_row("SELECT cc.collection_id FROM conversation_collections cc JOIN turns t ON t.conversation_id=cc.conversation_id WHERE t.id=?",[request],|r|r.get(0)).optional()?;
        if let Some(collection) = &scope {
            super::navigation::active_collection(&tx, collection)?;
            // Recheck all supplied evidence, including facts used for ideas/conclusion.
            for source in &allowed {
                if !super::navigation::source_in_collection(&tx, collection, source)? {
                    return Err(DataError::Unavailable);
                }
            }
        }
        for source in citations {
            if !super::search::current_source(&tx, source)? {
                tx.execute("UPDATE messages SET status='failed',error_code='source_unavailable' WHERE id=?",[&current.assistant.id])?;
                tx.commit()?;
                return Err(DataError::Unavailable);
            }
            match resolve(&tx, source, 1) {
                Ok(_) => (),
                Err(DataError::Unavailable) => {
                    tx.execute("UPDATE messages SET status='failed',error_code='source_unavailable' WHERE id=?",[&current.assistant.id])?;
                    tx.commit()?;
                    return Ok(false);
                }
                Err(e) => return Err(e),
            }
        }
        tx.execute(
            "UPDATE messages SET text=?2,status='complete',answer=?3 WHERE id=?1",
            params![current.assistant.id, text, answer.map(encode).transpose()?],
        )?;
        for source in citations {
            let (kind, id) = source.parts();
            tx.execute("UPDATE message_citations SET cited=1 WHERE message_id=?1 AND kind=?2 AND source_id=?3",params![current.assistant.id,kind,id])?;
        }
        tx.execute("UPDATE conversations SET updated_at=?2 WHERE id=(SELECT conversation_id FROM turns WHERE id=?1)",params![request,now()?])?;
        tx.commit()?;
        Ok(true)
    }
    pub fn cancel_turn(&self, id: &str) -> Result<()> {
        self.stop_turn(id, "cancelled", None)
    }
    pub fn fail_turn(&self, id: &str, failure: Failure) -> Result<()> {
        self.stop_turn(id, "failed", Some(failure.code()))
    }
    fn stop_turn(&self, id: &str, status: &str, error: Option<&str>) -> Result<()> {
        self.connection()?.execute("UPDATE messages SET status=?2,error_code=?3 WHERE turn_id=?1 AND role='assistant' AND status='processing'",params![id,status,error])?;
        Ok(())
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
        let ids: Vec<String> = db
            .prepare(
                "SELECT id FROM messages WHERE conversation_id=?1 AND seq>?2 ORDER BY seq LIMIT ?3",
            )?
            .query_map(
                params![conversation, after_seq, limit.clamp(1, 100) as i64],
                |r| r.get(0),
            )?
            .collect::<rusqlite::Result<_>>()?;
        ids.iter().map(|id| message(&db, id)).collect()
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
            "DELETE FROM workspace_drafts WHERE key IN (SELECT 'conclusion:'||id FROM messages WHERE conversation_id=?)",
            [conversation],
        )?;
        tx.execute("DELETE FROM conversations WHERE id=?", [conversation])?;
        tx.commit()?;
        Ok(())
    }
    /// Save exactly the reviewed text to its selected destination. A stale target
    /// rolls back the write; the caller's review draft remains available.
    /// Deleting a conversation never erases its successfully saved memories.
    pub fn save_conclusion(&self, r: &ConclusionRequest) -> Result<Receipt> {
        self.save_reviewed_conclusion(r, None)
    }
    pub fn save_reviewed_conclusion(
        &self,
        r: &ConclusionRequest,
        merged_body: Option<&str>,
    ) -> Result<Receipt> {
        valid_text(&r.title, 200)?;
        valid_text(&r.text, 128 * 1024)?;
        if let Some(body) = merged_body {
            valid_text(body, 128 * 1024)?;
            if matches!(r.destination, Destination::New) {
                return Err(DataError::Invalid);
            }
        }
        let hash = fingerprint(&("conclusion", r, merged_body))?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = replay(&tx, &r.request_id, &hash)? {
            return Ok(old);
        }
        let source = message(&tx, &r.message_id)?;
        if source.status != "complete" {
            return Err(DataError::Conflict);
        }
        let conversation_id: String = tx.query_row(
            "SELECT conversation_id FROM messages WHERE id=?",
            [&r.message_id],
            |row| row.get(0),
        )?;
        let origin = Origin::Conversation {
            conversation_id,
            message_id: r.message_id.clone(),
            message_role: source.role,
            confirmed_by: "user".into(),
        };
        let capture = insert_capture(&tx, &r.request_id, &r.text, &origin)?;
        tx.execute(
            "INSERT INTO conclusion_intents(capture_id,title,destination,merged_body) VALUES(?1,?2,?3,?4)",
            params![capture.id, r.title, encode(&r.destination)?, merged_body],
        )?;
        for reference in source.citations {
            let (kind, id) = reference.source.parts();
            tx.execute(
                "INSERT INTO capture_citations(capture_id,kind,source_id) VALUES(?1,?2,?3)",
                params![capture.id, kind, id],
            )?;
        }
        let mut receipt = match (|| {
            let body = match &r.destination {
                Destination::New => r.text.clone(),
                Destination::Existing {
                    memory_id,
                    expected_version,
                } => {
                    let previous = head(&tx, memory_id, expected_version)?;
                    merged_body
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("{}\n\n{}", previous.body, r.text))
                }
            };
            apply(
                &tx,
                &ChangeRequest {
                    request_id: r.request_id.clone(),
                    capture_id: capture.id.clone(),
                    destination: r.destination.clone(),
                    title: r.title.clone(),
                    body,
                    actor: Actor::User,
                },
            )
        })() {
            Ok(receipt) => receipt,
            Err(DataError::Conflict | DataError::Unavailable) => {
                // Dropping this transaction also discards its archive writes.
                return Ok(Receipt {
                    request_id: r.request_id.clone(),
                    action: "conclusion".into(),
                    capture_id: None,
                    memory_id: None,
                    before_version: None,
                    after_version: None,
                    status: "needs_review".into(),
                });
            }
            Err(error) => return Err(error),
        };
        receipt.action = "conclusion".into();
        // A conflict rolls this transaction back, including the archive.
        // The caller retains the complete review in its local workspace draft.
        save_receipt(&tx, &receipt, &hash)?;
        tx.execute(
            "DELETE FROM workspace_drafts WHERE key=? AND json_extract(payload,'$.request_id')=?",
            params![format!("conclusion:{}", r.message_id), r.request_id],
        )?;
        tx.commit()?;
        Ok(receipt)
    }
    pub fn conclusion_intent(&self, capture: &str) -> Result<(String, Destination)> {
        let mut connection = self.connection()?;
        let db = connection.transaction()?;
        raw(&db, capture)?;
        let (title, target): (String, String) = db.query_row(
            "SELECT title,destination FROM conclusion_intents WHERE capture_id=?",
            [capture],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((
            title,
            serde_json::from_str(&target).map_err(|_| DataError::Integrity)?,
        ))
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
