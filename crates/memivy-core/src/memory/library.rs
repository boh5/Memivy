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
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordKey {
    pub kind: String,
    pub id: String,
}
impl RecordKey {
    fn validate(&self) -> Result<()> {
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
    pub matched_capture: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct LibraryPage {
    pub items: Vec<LibraryRow>,
    pub next_offset: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct LibrarySource {
    pub id: String,
    pub capture: Option<RawCapture>,
}
#[derive(Debug, Serialize)]
pub struct LibraryDetail {
    pub key: RecordKey,
    pub state: String,
    pub title: String,
    pub body: String,
    pub current: Option<Version>,
    pub history: Vec<Version>,
    pub sources: Vec<LibrarySource>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDraft {
    pub key: String,
    pub request_id: String,
    pub title: String,
    pub body: String,
    pub expected_version: Option<String>,
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
        .find(|l| !l.trim().is_empty())
        .unwrap_or("原始记录")
        .trim()
        .chars()
        .take(45)
        .collect()
}
fn search_error(e: rusqlite::Error) -> DataError {
    if matches!(&e, rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::OperationInterrupted)
    {
        DataError::SearchBudget
    } else {
        e.into()
    }
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
    /// Each term may match the current version or one of its available originals.
    pub fn library(&self, q: &LibraryQuery) -> Result<LibraryPage> {
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
        let mut db = self.connection()?;
        let started = Instant::now();
        db.progress_handler(
            1000,
            Some(move || started.elapsed() > Duration::from_millis(500)),
        )?;
        let tx = db.transaction()?;
        let state = if q.trash { "trashed" } else { "active" };
        let availability = if q.trash { "trashed" } else { "active" };
        let source_visibility = if q.trash {
            "(cs.availability='active' OR (cs.availability='trashed' AND (cs.trash_owner=items.id OR items.kind='capture')))"
        } else {
            "cs.availability='active'"
        };
        let mut values: Vec<Value> = vec![];
        let mut bind = |value: Value| {
            values.push(value);
            format!("?{}", values.len())
        };
        let mut filters = vec!["1".to_string()];
        let mut score = vec!["0.0".to_string()];
        for term in q.query.split_whitespace() {
            let literal = bind(Value::Text(term.into()));
            score.push(format!(
                "CASE WHEN instr(lower(title),lower({literal}))>0 THEN 12 ELSE 0 END"
            ));
            if term.chars().count() >= 3 {
                let ft = bind(Value::Text(format!("\"{}\"", term.replace('"', "\"\""))));
                filters.push(format!("(version_id IN (SELECT source_id FROM record_fts WHERE kind='version' AND record_fts MATCH {ft}) OR EXISTS(SELECT 1 FROM captures c JOIN capture_state cs ON cs.capture_id=c.id WHERE {source_visibility} AND (c.id=items.id AND items.kind='capture' OR c.id IN (SELECT capture_id FROM version_captures WHERE version_id=items.version_id)) AND c.id IN (SELECT source_id FROM record_fts WHERE kind='capture' AND record_fts MATCH {ft})))"));
            } else {
                filters.push(format!("(instr(lower(title||' '||body),lower({literal}))>0 OR EXISTS(SELECT 1 FROM captures c JOIN capture_state cs ON cs.capture_id=c.id WHERE {source_visibility} AND (c.id=items.id AND items.kind='capture' OR c.id IN (SELECT capture_id FROM version_captures WHERE version_id=items.version_id)) AND instr(lower(c.text||' '||c.source),lower({literal}))>0))"));
            }
        }
        for (field, value) in [("kind", &q.origin), ("project", &q.project)] {
            if let Some(value) = value {
                let p = bind(Value::Text(value.clone()));
                filters.push(format!("EXISTS(SELECT 1 FROM captures c JOIN capture_state cs ON cs.capture_id=c.id WHERE {source_visibility} AND (c.id=items.id AND items.kind='capture' OR c.id IN (SELECT capture_id FROM version_captures WHERE version_id=items.version_id)) AND json_extract(c.source,'$.{field}')={p})"));
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
        let limit = if q.limit == 0 { 40 } else { q.limit.min(100) };
        let raw_filter = if q.trash {
            "s.trash_owner IS NULL"
        } else {
            "s.understanding!='attached'"
        };
        let sql = format!("WITH items AS (
          SELECT 'memory' kind,m.id,v.id version_id,v.title,v.body,m.updated_at FROM memories m JOIN memory_versions v ON v.id=m.current_version_id WHERE m.state='{state}'
          UNION ALL SELECT 'capture',c.id,NULL,'',c.text,c.created_at FROM captures c JOIN capture_state s ON s.capture_id=c.id WHERE s.availability='{availability}' AND {raw_filter}
        ) SELECT kind,id,version_id,title,body,updated_at FROM items WHERE {} ORDER BY ({}) DESC,updated_at DESC,kind,id LIMIT {} OFFSET {}", filters.join(" AND "), score.join("+"), limit+1,q.offset);
        type Row = (String, String, Option<String>, String, String, i64);
        let rows: Vec<Row> = tx
            .prepare(&sql)
            .map_err(search_error)?
            .query_map(rusqlite::params_from_iter(values), |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .map_err(search_error)?
            .collect::<rusqlite::Result<_>>()
            .map_err(search_error)?;
        let has_more = rows.len() > limit;
        let mut items = vec![];
        for (kind, id, version, title, body, updated_at) in rows.into_iter().take(limit) {
            let sources:Vec<(String,String,String)> = tx.prepare("SELECT c.id,c.text,c.source FROM captures c JOIN capture_state cs ON cs.capture_id=c.id WHERE (cs.availability='active' OR (?3 AND cs.availability='trashed' AND (cs.trash_owner=?4 OR ?5='capture'))) AND (c.id=?1 OR c.id IN (SELECT capture_id FROM version_captures WHERE version_id=?2)) ORDER BY c.created_at DESC,c.id")?.query_map(params![if kind=="capture" {Some(&id)} else {None},version,q.trash,id,kind],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<rusqlite::Result<_>>().map_err(search_error)?;
            let matched = sources.iter().find(|(_, text, origin)| {
                q.query.split_whitespace().any(|w| {
                    format!("{text} {origin}")
                        .to_lowercase()
                        .contains(&w.to_lowercase())
                })
            });
            let body_matches = q.query.split_whitespace().any(|w| {
                format!("{title} {body}")
                    .to_lowercase()
                    .contains(&w.to_lowercase())
            });
            let source_match = if !body_matches && !q.query.trim().is_empty() {
                matched
            } else {
                None
            };
            let origin = sources
                .first()
                .map(|(_, _, s)| serde_json::from_str(s))
                .transpose()
                .map_err(|_| DataError::Integrity)?;
            items.push(LibraryRow {
                key: RecordKey { kind, id },
                title: if title.is_empty() {
                    raw_title(&body)
                } else {
                    title
                },
                snippet: snippet(
                    source_match.map(|(_, t, _)| t.as_str()).unwrap_or(&body),
                    &q.query,
                ),
                updated_at,
                origin,
                matched_capture: source_match.map(|(id, _, _)| id.clone()),
            });
        }
        Ok(LibraryPage {
            items,
            next_offset: has_more.then_some(q.offset + limit),
        })
    }

    pub fn library_detail(&self, key: &RecordKey) -> Result<LibraryDetail> {
        key.validate()?;
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let (state, current, history, source_ids, title, body) = if key.kind == "memory" {
            let (state,head):(String,String)=tx.query_row("SELECT state,current_version_id FROM memories WHERE id=? AND state IN ('active','trashed')",[&key.id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            let current = records::version(&tx, &head)?;
            let ids:Vec<String>=tx.prepare("SELECT id FROM memory_versions WHERE memory_id=? AND body IS NOT NULL ORDER BY rowid DESC")?.query_map([&key.id],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
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
                vec![key.id.clone()],
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
            sources.push(LibrarySource { id, capture });
        }
        Ok(LibraryDetail {
            key: key.clone(),
            state,
            title,
            body,
            current,
            history,
            sources,
        })
    }

    pub fn library_projects(&self) -> Result<Vec<String>> {
        Ok(self.connection()?.prepare("SELECT DISTINCT json_extract(c.source,'$.project') project FROM captures c JOIN capture_state s ON s.capture_id=c.id WHERE s.availability='active' AND project IS NOT NULL AND project!='' ORDER BY project LIMIT 200")?.query_map([],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?)
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
        validate_draft_key(&draft.key)?;
        valid_id(&draft.request_id)?;
        if draft.title.len() > 200 || draft.body.len() > 128 * 1024 || draft.context.len() > 4 {
            return Err(DataError::Invalid);
        }
        for source in &draft.context {
            valid_id(source.parts().1)?;
        }
        if let Some(v) = &draft.expected_version {
            valid_id(v)?;
        }
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some((kind, id)) = draft.key.split_once(':') {
            let sql = if kind == "discussion" {
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
        Ok(())
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
        tx.execute_batch("DROP TRIGGER IF EXISTS capture_search_insert; DROP TRIGGER IF EXISTS version_search_insert; DROP TRIGGER IF EXISTS capture_search_erase; DROP TRIGGER IF EXISTS version_search_erase; DROP TABLE IF EXISTS record_fts;
        CREATE VIRTUAL TABLE record_fts USING fts5(kind UNINDEXED,source_id UNINDEXED,title,body,origin,tokenize='trigram');
        INSERT INTO record_fts(record_fts,rank) VALUES('secure-delete',1);
        INSERT INTO record_fts(kind,source_id,title,body,origin) SELECT 'capture',id,'',text,source FROM captures WHERE text IS NOT NULL;
        INSERT INTO record_fts(kind,source_id,title,body,origin) SELECT 'version',id,title,body,'' FROM memory_versions WHERE body IS NOT NULL;
        CREATE TRIGGER capture_search_insert AFTER INSERT ON captures WHEN NEW.text IS NOT NULL BEGIN INSERT INTO record_fts(kind,source_id,title,body,origin) VALUES('capture',NEW.id,'',NEW.text,NEW.source); END;
        CREATE TRIGGER version_search_insert AFTER INSERT ON memory_versions WHEN NEW.body IS NOT NULL BEGIN INSERT INTO record_fts(kind,source_id,title,body,origin) VALUES('version',NEW.id,NEW.title,NEW.body,''); END;
        CREATE TRIGGER capture_search_erase AFTER UPDATE ON captures WHEN NEW.text IS NULL BEGIN DELETE FROM record_fts WHERE kind='capture' AND source_id=NEW.id; END;
        CREATE TRIGGER version_search_erase AFTER UPDATE ON memory_versions WHEN NEW.body IS NULL BEGIN DELETE FROM record_fts WHERE kind='version' AND source_id=NEW.id; END;")?;
        tx.commit()?;
        Ok(())
    }
}
fn validate_draft_key(key: &str) -> Result<()> {
    if matches!(key, "capture" | "question") {
        return Ok(());
    }
    let (kind, id) = key.split_once(':').ok_or(DataError::Invalid)?;
    if kind == "discussion" {
        return valid_id(id);
    }
    RecordKey {
        kind: kind.into(),
        id: id.into(),
    }
    .validate()
}
