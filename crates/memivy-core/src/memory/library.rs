use super::{db::*, records, *};
use rusqlite::{OptionalExtension, TransactionBehavior, params, types::Value};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LibraryQuery {
    pub query: String,
    pub trash: bool,
    pub origin: Option<String>,
    pub project: Option<String>,
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub offset: usize,
    pub limit: usize,
    pub pinned: bool,
    pub collection_id: Option<String>,
    pub exclude_collection_id: Option<String>,
    pub oldest: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordKey {
    pub kind: String,
    pub id: String,
}
impl RecordKey {
    pub(super) fn validate(&self) -> Result<()> {
        valid_id(&self.id)?;
        if !matches!(self.kind.as_str(), "memory" | "capture") {
            return Err(DataError::Invalid);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct LibraryRow {
    pub key: RecordKey,
    pub title: String,
    pub snippet: String,
    pub updated_at: i64,
    pub origin: Option<Origin>,
}
#[derive(Debug, Serialize)]
pub struct LibraryPage {
    pub degraded_reason: Option<String>,
    pub items: Vec<LibraryRow>,
    pub next_offset: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct LibrarySource {
    pub id: String,
    pub capture: Option<RawCapture>,
    pub conversation_available: Option<bool>,
}
#[derive(Debug, Serialize)]
pub struct LibraryDetail {
    pub key: RecordKey,
    pub state: String,
    pub title: String,
    pub body: String,
    pub current: Option<Version>,
    pub history: Vec<Version>,
    pub history_count: usize,
    pub source_count: usize,
    pub sources: Vec<LibrarySource>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<Destination>,
    pub key: String,
    pub request_id: String,
    pub title: String,
    pub body: String,
    pub expected_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    #[serde(default)]
    pub context: Vec<SourceRef>,
}

fn snippet(text: &str, query: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let lower = text.to_lowercase();
    let position = query
        .split_whitespace()
        .filter_map(|word| lower.find(&word.to_lowercase()))
        .min();
    // Translate through the lowercased string: no byte slicing of original Unicode.
    let start = position
        .map(|p| lower[..p].chars().count().saturating_sub(35))
        .unwrap_or(0)
        .min(chars.len());
    let end = (start + 160).min(chars.len());
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        chars[start..end].iter().collect::<String>(),
        if end < chars.len() { "…" } else { "" }
    )
}
fn raw_title(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("Input archive")
        .trim()
        .chars()
        .take(45)
        .collect()
}

impl MemoryStore {
    pub fn save_library_edit(&self, draft: &WorkspaceDraft) -> Result<Receipt> {
        validate_draft_key(&draft.key)?;
        let (kind, id) = draft.key.split_once(':').ok_or(DataError::Invalid)?;
        if kind == "memory" {
            self.edit_memory(&EditRequest {
                request_id: draft.request_id.clone(),
                memory_id: id.into(),
                expected_version: draft.expected_version.clone().ok_or(DataError::Invalid)?,
                title: draft.title.clone(),
                body: draft.body.clone(),
            })
        } else {
            self.apply_capture(&ChangeRequest {
                request_id: draft.request_id.clone(),
                capture_id: id.into(),
                destination: Destination::New,
                title: draft.title.clone(),
                body: draft.body.clone(),
                actor: Actor::User,
            })
        }
    }
    /// Paged library reads. Filter against fact tables before ranking/limiting.
    /// Search terms match only the active current Memory.
    pub fn library(&self, q: &LibraryQuery) -> Result<LibraryPage> {
        if !q.trash && !q.query.trim().is_empty() {
            let result = self.search(&SearchRequest {
                query: q.query.clone(),
                scope: SearchScope {
                    project: q.project.clone(),
                    origin: q.origin.clone(),
                    collection_id: q.collection_id.clone(),
                    exclude_collection_id: q.exclude_collection_id.clone(),
                    since: q.since,
                    until: q.until,
                    pinned: q.pinned,
                    ..Default::default()
                },
                limit: if q.limit == 0 { 40 } else { q.limit.min(100) },
                offset: q.offset,
                excerpt_chars: 160,
                ..Default::default()
            })?;
            return Ok(LibraryPage {
                degraded_reason: result.degraded_reason,
                items: result
                    .items
                    .into_iter()
                    .map(SearchHit::into_library_row)
                    .collect(),
                next_offset: result.next_offset,
            });
        }
        let mut db = self.connection()?;
        let started = Instant::now();
        db.progress_handler(
            1000,
            Some(move || started.elapsed() > Duration::from_millis(500)),
        )?;
        let tx = db.transaction()?;
        Self::library_in(&tx, q)
    }

    pub(super) fn library_in(tx: &rusqlite::Connection, q: &LibraryQuery) -> Result<LibraryPage> {
        if q.query.len() > 512
            || q.query.split_whitespace().count() > 16
            || q.offset > 1_000_000
            || q.origin
                .as_ref()
                .is_some_and(|s| !matches!(s.as_str(), "user" | "agent" | "conversation"))
            || q.project.as_ref().is_some_and(|s| s.len() > 200)
            || matches!((q.since,q.until), (Some(a),Some(b)) if a > b)
        {
            return Err(DataError::Invalid);
        }
        for collection in [&q.collection_id, &q.exclude_collection_id]
            .into_iter()
            .flatten()
        {
            super::navigation::active_collection(tx, collection)?;
        }
        let state = if q.trash { "trashed" } else { "active" };
        let mut values: Vec<Value> = vec![];
        let mut bind = |value: Value| {
            values.push(value);
            format!("?{}", values.len())
        };
        let mut filters = vec!["1".to_string()];
        for term in q.query.split_whitespace() {
            let p = bind(Value::Text(term.into()));
            filters.push(format!("instr(lower(title||' '||body),lower({p}))>0"));
        }
        for (field, value) in [("kind", &q.origin), ("project", &q.project)] {
            if let Some(value) = value {
                let p = bind(Value::Text(value.clone()));
                filters.push(format!("EXISTS(SELECT 1 FROM captures c JOIN version_captures vc ON vc.capture_id=c.id WHERE vc.version_id=items.version_id AND json_extract(c.source,'$.{field}')={p})"));
            }
        }
        if let Some(since) = q.since {
            let p = bind(since.into());
            filters.push(format!("updated_at>={p}"));
        }
        if let Some(until) = q.until {
            let p = bind(until.into());
            filters.push(format!("updated_at<{p}"));
        }
        if q.pinned {
            filters.push("EXISTS(SELECT 1 FROM record_pins p WHERE p.kind=items.kind AND p.record_id=items.id)".into());
        }
        for (collection, exclude) in [(&q.collection_id, false), (&q.exclude_collection_id, true)] {
            if let Some(collection) = collection {
                let p = bind(Value::Text(collection.clone()));
                filters.push(format!("{}EXISTS(SELECT 1 FROM collection_entries ce WHERE ce.collection_id={p} AND ce.kind=items.kind AND ce.record_id=items.id)", if exclude { "NOT " } else { "" }));
            }
        }
        let limit = if q.limit == 0 { 40 } else { q.limit.min(100) };
        let chronological = if q.oldest { "ASC" } else { "DESC" };
        let sql = format!("WITH items AS (
          SELECT 'memory' kind,m.id,v.id version_id,v.title,v.body,m.updated_at FROM memories m JOIN memory_versions v ON v.id=m.current_version_id WHERE m.state='{state}'
        ) SELECT kind,id,version_id,title,body,updated_at FROM items WHERE {} ORDER BY updated_at {chronological},kind,id LIMIT {} OFFSET {}", filters.join(" AND "), limit+1,q.offset);
        type Row = (String, String, Option<String>, String, String, i64);
        let rows: Vec<Row> = tx
            .prepare(&sql)?
            .query_map(rusqlite::params_from_iter(values), |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let has_more = rows.len() > limit;
        let mut items = vec![];
        for (kind, id, version, title, body, updated_at) in rows.into_iter().take(limit) {
            let origin: Option<String> = tx.query_row("SELECT c.source FROM version_captures vc JOIN captures c ON c.id=vc.capture_id WHERE vc.version_id=? AND c.source IS NOT NULL ORDER BY c.created_at DESC,c.id LIMIT 1",[&version],|r|r.get(0)).optional()?;
            let origin = origin
                .map(|s| serde_json::from_str(&s).map_err(|_| DataError::Integrity))
                .transpose()?;
            items.push(LibraryRow {
                key: RecordKey { kind, id },
                title,
                snippet: snippet(&body, &q.query),
                updated_at,
                origin,
            });
        }
        Ok(LibraryPage {
            degraded_reason: None,
            items,
            next_offset: has_more.then_some(q.offset + limit),
        })
    }

    pub fn library_detail(&self, key: &RecordKey) -> Result<LibraryDetail> {
        self.library_detail_view(key, true)
    }
    pub fn library_detail_view(&self, key: &RecordKey, archives: bool) -> Result<LibraryDetail> {
        key.validate()?;
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let history_count: usize = if key.kind == "memory" {
            tx.query_row(
                "SELECT COUNT(*) FROM memory_versions WHERE memory_id=? AND body IS NOT NULL",
                [&key.id],
                |r| r.get::<_, i64>(0).map(|count| count as usize),
            )?
        } else {
            0
        };
        let source_count: usize = if key.kind == "memory" {
            tx.query_row("SELECT COUNT(DISTINCT vc.capture_id) FROM version_captures vc JOIN memory_versions v ON v.id=vc.version_id WHERE v.memory_id=? AND v.body IS NOT NULL", [&key.id], |r| r.get::<_, i64>(0).map(|count| count as usize))?
        } else {
            1
        };
        let (state, current, history, source_ids, title, body) = if key.kind == "memory" {
            let (state,head):(String,String)=tx.query_row("SELECT state,current_version_id FROM memories WHERE id=? AND state IN ('active','trashed','merged')",[&key.id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            let current = records::version(&tx, &head)?;
            let ids: Vec<String> = if archives {
                tx.prepare("SELECT id FROM memory_versions WHERE memory_id=? AND body IS NOT NULL ORDER BY rowid DESC")?.query_map([&key.id],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?
            } else {
                vec![]
            };
            let history = ids
                .iter()
                .map(|id| records::version(&tx, id))
                .collect::<Result<Vec<_>>>()?;
            let mut sources: Vec<String> =
                history.iter().flat_map(|v| v.capture_ids.clone()).collect();
            sources.sort();
            sources.dedup();
            (
                state,
                Some(current.clone()),
                history,
                sources,
                current.title,
                current.body,
            )
        } else {
            let (state,text):(String,String)=tx.query_row("SELECT s.availability,c.text FROM captures c JOIN capture_state s ON s.capture_id=c.id WHERE c.id=? AND s.availability IN ('active','trashed')",[&key.id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            (
                state,
                None,
                vec![],
                if archives {
                    vec![key.id.clone()]
                } else {
                    vec![]
                },
                raw_title(&text),
                text,
            )
        };
        let mut sources = vec![];
        for id in source_ids {
            let raw:Option<(String,String,i64,String)>=tx.query_row("SELECT c.text,c.source,c.created_at,s.understanding FROM captures c JOIN capture_state s ON s.capture_id=c.id WHERE c.id=?1 AND (s.availability='active' OR (?2='trashed' AND s.availability='trashed' AND (s.trash_owner=?3 OR ?4='capture')))",params![id,state,key.id,key.kind],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
            let capture = raw
                .map(
                    |(text, origin, created_at, understanding)| -> Result<RawCapture> {
                        Ok(RawCapture {
                            id: id.clone(),
                            text,
                            origin: serde_json::from_str(&origin)
                                .map_err(|_| DataError::Integrity)?,
                            created_at,
                            understanding,
                        })
                    },
                )
                .transpose()?;
            let conversation_available = match capture.as_ref().map(|raw| &raw.origin) {
                Some(
                    Origin::Conversation {
                        conversation_id, ..
                    }
                    | Origin::Discussion {
                        conversation_id, ..
                    },
                ) => Some(
                    tx.prepare("SELECT 1 FROM conversations WHERE id=?")?
                        .exists([conversation_id])?,
                ),
                _ => None,
            };
            sources.push(LibrarySource {
                id,
                capture,
                conversation_available,
            });
        }
        Ok(LibraryDetail {
            key: key.clone(),
            state,
            title,
            body,
            current,
            history,
            history_count,
            source_count,
            sources,
        })
    }

    pub fn library_projects(&self) -> Result<Vec<String>> {
        Ok(self.connection()?.prepare("SELECT DISTINCT json_extract(c.source,'$.project') project FROM captures c JOIN version_captures vc ON vc.capture_id=c.id JOIN memories m ON m.current_version_id=vc.version_id WHERE m.state='active' AND project IS NOT NULL AND project!='' ORDER BY project LIMIT 200")?.query_map([],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }
    pub fn workspace_draft(&self, key: &str) -> Result<Option<WorkspaceDraft>> {
        validate_draft_key(key)?;
        let value: Option<String> = self
            .connection()?
            .query_row(
                "SELECT payload FROM workspace_drafts WHERE key=?",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        value
            .map(|s| serde_json::from_str(&s).map_err(|_| DataError::Integrity))
            .transpose()
    }
    pub fn save_workspace_draft(&self, draft: &WorkspaceDraft) -> Result<()> {
        self.write_workspace_draft(draft, None).map(|_| ())
    }
    /// A WebView may only replace the draft version it actually read.
    pub fn compare_workspace_draft(
        &self,
        draft: &WorkspaceDraft,
        expected_request: Option<&str>,
    ) -> Result<bool> {
        self.write_workspace_draft(draft, Some(expected_request))
    }
    fn write_workspace_draft(
        &self,
        draft: &WorkspaceDraft,
        expected: Option<Option<&str>>,
    ) -> Result<bool> {
        validate_draft_key(&draft.key)?;
        valid_id(&draft.request_id)?;
        if draft.title.len() > 200 || draft.body.len() > 128 * 1024 || draft.context.len() > 32 {
            return Err(DataError::Invalid);
        }
        if let Some(destination) = &draft.destination {
            if !draft.key.starts_with("save:") {
                return Err(DataError::Invalid);
            }
            if let Destination::Existing {
                memory_id,
                expected_version,
            } = destination
            {
                valid_id(memory_id)?;
                valid_id(expected_version)?;
            }
        } else if draft.key.starts_with("save:") {
            return Err(DataError::Invalid);
        }
        for source in &draft.context {
            valid_id(source.parts().1)?;
        }
        if let Some(v) = &draft.expected_version {
            valid_id(v)?;
        }
        if let Some(origin) = &draft.origin {
            if !matches!(origin, Origin::User { .. }) {
                return Err(DataError::Invalid);
            }
            super::records::validate_origin(origin)?;
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(expected) = expected {
            let old: Option<String> = tx
                .query_row(
                    "SELECT payload FROM workspace_drafts WHERE key=?",
                    [&draft.key],
                    |r| r.get(0),
                )
                .optional()?;
            // Retrying the same acknowledged-or-not write is idempotent.
            if old.as_deref() == Some(encode(draft)?.as_str()) {
                return Ok(true);
            }
            let old = old
                .map(|s| {
                    serde_json::from_str::<WorkspaceDraft>(&s).map_err(|_| DataError::Integrity)
                })
                .transpose()?;
            if old.as_ref().map(|d| d.request_id.as_str()) != expected {
                return Ok(false);
            }
        }
        if let Some((kind, id)) = draft.key.split_once(':') {
            let sql = if kind == "save" {
                "SELECT EXISTS(SELECT 1 FROM messages WHERE id=? AND role='assistant' AND status!='processing' AND length(trim(text))>0)"
            } else if kind == "discussion" {
                "SELECT EXISTS(SELECT 1 FROM conversations WHERE id=?)"
            } else if kind == "memory" {
                "SELECT EXISTS(SELECT 1 FROM memories WHERE id=? AND state IN ('active','trashed'))"
            } else {
                "SELECT EXISTS(SELECT 1 FROM capture_state WHERE capture_id=? AND availability!='purged')"
            };
            if !tx.query_row(sql, [id], |r| r.get::<_, bool>(0))? {
                return Err(DataError::Unavailable);
            }
        }
        tx.execute("INSERT INTO workspace_drafts(key,payload) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET payload=excluded.payload",params![draft.key,encode(draft)?])?;
        tx.commit()?;
        Ok(true)
    }
    pub fn consume_workspace_draft(&self, key: &str, request: &str) -> Result<bool> {
        validate_draft_key(key)?;
        valid_id(request)?;
        let changed = self.connection()?.execute(
            "DELETE FROM workspace_drafts WHERE key=? AND json_extract(payload,'$.request_id')=?",
            params![key, request],
        )?;
        Ok(changed == 1)
    }
    pub fn delete_workspace_draft(&self, key: &str) -> Result<()> {
        validate_draft_key(key)?;
        self.connection()?
            .execute("DELETE FROM workspace_drafts WHERE key=?", [key])?;
        Ok(())
    }
    /// Rebuild from the immutable fact tables even when the FTS table is gone.
    /// Transactions make both the new index and its triggers visible together.
    pub fn rebuild_search_index(&self) -> Result<()> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(include_str!("sql/rebuild_search.sql"))?;
        tx.commit()?;
        Ok(())
    }
}
fn validate_draft_key(key: &str) -> Result<()> {
    if matches!(key, "input" | "quick_input") {
        return Ok(());
    }
    let (kind, id) = key.split_once(':').ok_or(DataError::Invalid)?;
    if matches!(kind, "discussion" | "save") {
        return valid_id(id);
    }
    RecordKey {
        kind: kind.into(),
        id: id.into(),
    }
    .validate()
}
