//! One search contract for native workflows and MCP. Callers provide scope, not
//! a backend or their own ranking. Archives never participate in candidate reads.
use super::{db::*, records, retrieval, *};
use rusqlite::{Connection, types::Value};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SearchScope {
    pub project: Option<String>,
    pub origin: Option<String>,
    pub collection_id: Option<String>,
    pub exclude_collection_id: Option<String>,
    pub exclude_memories: Vec<String>,
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub pinned: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SearchRequest {
    pub query: String,
    pub reference_memory_id: Option<String>,
    pub variants: Vec<String>,
    pub scope: SearchScope,
    pub limit: usize,
    pub offset: usize,
    pub excerpt_chars: usize,
}
impl Default for SearchRequest {
    fn default() -> Self {
        Self {
            query: String::new(),
            reference_memory_id: None,
            variants: vec![],
            scope: SearchScope::default(),
            limit: 8,
            offset: 0,
            excerpt_chars: 1200,
        }
    }
}
impl SearchRequest {
    pub fn text(query: impl Into<String>, limit: usize) -> Self {
        Self {
            query: query.into(),
            limit,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchHit {
    pub memory_id: String,
    pub version_id: String,
    pub title: String,
    pub evidence: Evidence,
    pub origins: Vec<Origin>,
    pub updated_at: i64,
    pub score: f64,
}
impl SearchHit {
    pub(super) fn into_library_row(self) -> LibraryRow {
        LibraryRow {
            key: RecordKey {
                kind: "memory".into(),
                id: self.memory_id,
            },
            title: self.title,
            snippet: self.evidence.text,
            updated_at: self.updated_at,
            origin: self.origins.into_iter().next(),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct SearchResult {
    pub mode: String,
    pub degraded_reason: Option<String>,
    pub items: Vec<SearchHit>,
    pub has_more: bool,
    pub truncated: bool,
    pub next_offset: Option<usize>,
}

impl MemoryStore {
    pub fn search(&self, request: &SearchRequest) -> Result<SearchResult> {
        let (vector, reason) = match self.query_vector(request) {
            Ok(v) => (v, None),
            Err(e) => (None, Some(e)),
        };
        let mut db = self.connection()?;
        let started = Instant::now();
        db.progress_handler(
            1000,
            Some(move || started.elapsed() > Duration::from_millis(500)),
        )?;
        let tx = db.transaction()?;
        search_in(&tx, request, vector.as_ref(), reason)
    }
}

pub(super) fn current_source(db: &Connection, source: &SourceRef) -> Result<bool> {
    let SourceRef::Version(version) = source else {
        return Ok(false);
    };
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM memories WHERE current_version_id=? AND state='active')",
        [version],
        |r| r.get(0),
    )?)
}

fn bind(values: &mut Vec<Value>, value: impl Into<Value>) -> String {
    values.push(value.into());
    format!("?{}", values.len())
}

fn search_in(
    db: &Connection,
    request: &SearchRequest,
    vector: Option<&(String, Vec<u8>)>,
    mut degraded_reason: Option<String>,
) -> Result<SearchResult> {
    let scope = &request.scope;
    if request.limit == 0
        || request.limit > 100
        || request.offset > 100_000
        || request.excerpt_chars == 0
        || request.excerpt_chars > 3000
        || request.variants.len() > 4
        || request.query.len() > 32 * 1024
        || request
            .variants
            .iter()
            .any(|q| q.trim().is_empty() || q.len() > 512 || q.split_whitespace().count() > 128)
        || scope.exclude_memories.len() > 100
        || scope.project.as_ref().is_some_and(|v| v.len() > 200)
        || scope
            .origin
            .as_deref()
            .is_some_and(|v| !matches!(v, "user" | "agent" | "conversation"))
        || matches!((scope.since,scope.until),(Some(a),Some(b)) if a > b)
        || (request.query.trim().is_empty() == request.reference_memory_id.is_none())
    {
        return Err(DataError::Invalid);
    }
    let mut queries = request.variants.clone();
    if let Some(memory) = &request.reference_memory_id {
        valid_id(memory)?;
        let version: String = db.query_row(
            "SELECT current_version_id FROM memories WHERE id=? AND state='active'",
            [memory],
            |r| r.get(0),
        )?;
        let v = records::version(db, &version)?;
        queries.extend(
            retrieval::capture_terms(&format!("{} {}", v.title, v.body))
                .into_iter()
                .take(4),
        );
    } else if request.query.len() <= 16 * 1024 && request.query.split_whitespace().count() <= 128 {
        queries.insert(0, request.query.trim().to_owned());
    }
    queries.retain(|q| !q.trim().is_empty());
    queries.sort();
    queries.dedup();
    let mut values = vec![];
    let mut filters = vec!["m.state='active' AND v.body IS NOT NULL".to_owned()];
    for (collection, exclude) in [
        (&scope.collection_id, false),
        (&scope.exclude_collection_id, true),
    ] {
        if let Some(collection) = collection {
            super::navigation::active_collection(db, collection)?;
            let p = bind(&mut values, collection.clone());
            filters.push(format!("{}EXISTS(SELECT 1 FROM collection_entries ce WHERE ce.kind='memory' AND ce.record_id=m.id AND ce.collection_id={p})",if exclude {"NOT "} else {""}));
        }
    }
    for (field, value) in [("project", &scope.project), ("kind", &scope.origin)] {
        if let Some(value) = value {
            let p = bind(&mut values, value.clone());
            filters.push(format!("EXISTS(SELECT 1 FROM version_captures vc JOIN captures c ON c.id=vc.capture_id WHERE vc.version_id=v.id AND json_extract(c.source,'$.{field}')={p})"));
        }
    }
    for memory in scope
        .exclude_memories
        .iter()
        .chain(request.reference_memory_id.iter())
    {
        valid_id(memory)?;
        let p = bind(&mut values, memory.clone());
        filters.push(format!("m.id!={p}"));
    }
    for (time, operator) in [(scope.since, ">="), (scope.until, "<")] {
        if let Some(time) = time {
            let p = bind(&mut values, time);
            filters.push(format!("m.updated_at{operator}{p}"));
        }
    }
    if scope.pinned {
        filters.push(
            "EXISTS(SELECT 1 FROM record_pins p WHERE p.kind='memory' AND p.record_id=m.id)".into(),
        );
    }
    let candidate_limit = (request.offset + request.limit * 3).max(24);
    let mut scores: HashMap<String, (String, i64, f64)> = HashMap::new();
    let mut candidate_truncated = false;
    for query in &queries {
        let mut args = values.clone();
        let mut predicates = filters.clone();
        let mut indexed = vec![];
        // AND within one expression; alternate expressions are fused once in core.
        for word in query.split_whitespace() {
            if word.chars().count() >= 3 {
                indexed.push(format!("\"{}\"", word.replace('"', "\"\"")));
            } else {
                let p = bind(&mut args, word.to_lowercase());
                predicates.push(format!("instr(lower(record_fts.title||' '||record_fts.body||' '||record_fts.origin),{p})>0"));
            }
        }
        let rank = if indexed.is_empty() {
            "0.0"
        } else {
            let p = bind(&mut args, indexed.join(" AND "));
            predicates.push(format!("record_fts MATCH {p}"));
            "bm25(record_fts,0.0,0.0,8.0,1.0,0.5)"
        };
        let sql = format!(
            "SELECT m.id,v.id,m.updated_at FROM record_fts CROSS JOIN memory_versions v ON v.id=record_fts.source_id JOIN memories m ON m.id=v.memory_id AND m.current_version_id=v.id WHERE {} ORDER BY {rank},m.updated_at DESC,m.id LIMIT {}",
            predicates.join(" AND "),
            candidate_limit + 1
        );
        let rows: Vec<(String, String, i64)> = db
            .prepare(&sql)?
            .query_map(rusqlite::params_from_iter(args), |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        candidate_truncated |= rows.len() > candidate_limit;
        for (rank, (memory, version, updated)) in rows.into_iter().take(candidate_limit).enumerate()
        {
            let score = 1.0 / (60.0 + (rank + 1) as f64);
            scores
                .entry(memory)
                .and_modify(|row| row.2 += score)
                .or_insert((version, updated, score));
        }
    }
    let mut windows = HashMap::new();
    let mut hybrid = false;
    if let Some((revision, vector)) = vector {
        // Several lexical reformulations are one retrieval channel.
        let mut lexical: Vec<_> = scores
            .iter()
            .map(|(id, row)| (id.clone(), row.clone()))
            .collect();
        lexical.sort_by(|a, b| {
            b.1.2
                .total_cmp(&a.1.2)
                .then(b.1.1.cmp(&a.1.1))
                .then(a.0.cmp(&b.0))
        });
        let lexical_scores = scores.clone();
        for (rank, (id, _)) in lexical.into_iter().enumerate() {
            scores.get_mut(&id).unwrap().2 = 1.0 / (61.0 + rank as f64);
        }
        let index = super::embedding::meta(db)?;
        if index.is_some_and(|i| i.revision == *revision && i.state == "ready") {
            let mut args = values.clone();
            let p = bind(&mut args, vector.clone());
            // SQLite takes the bare source range from the row providing min().
            // Group before LIMIT so many chunks cannot crowd out other memories.
            // Materialize scalar distances first: grouping raw BLOB rows would
            // spill hundreds of MB into SQLite's temporary sorter.
            let sql = format!(
                "WITH distances AS MATERIALIZED (SELECT m.id memory_id,v.id version_id,m.updated_at,c.start_char,vec_distance_cosine(c.vector,{p}) distance FROM embedding_chunks c JOIN memories m ON m.id=c.memory_id AND m.current_version_id=c.version_id JOIN memory_versions v ON v.id=c.version_id WHERE {}) SELECT memory_id,version_id,updated_at,start_char,min(distance) distance FROM distances GROUP BY memory_id ORDER BY distance,updated_at DESC,memory_id LIMIT {}",
                filters.join(" AND "),
                candidate_limit + 1
            );
            let result = (|| -> Result<Vec<(String, String, i64, usize)>> {
                Ok(db
                    .prepare(&sql)?
                    .query_map(rusqlite::params_from_iter(args), |r| {
                        Ok((
                            r.get(0)?,
                            r.get(1)?,
                            r.get(2)?,
                            r.get::<_, i64>(3)? as usize,
                        ))
                    })?
                    .collect::<rusqlite::Result<_>>()?)
            })();
            match result {
                Ok(rows) => {
                    candidate_truncated |= rows.len() > candidate_limit;
                    for (rank, (memory, version, updated, start)) in
                        rows.into_iter().take(candidate_limit).enumerate()
                    {
                        let score = 1.0 / (61.0 + rank as f64);
                        if !lexical_scores.contains_key(&memory) {
                            windows.insert(memory.clone(), start);
                        }
                        scores
                            .entry(memory)
                            .and_modify(|r| r.2 += score)
                            .or_insert((version, updated, score));
                    }
                    hybrid = true;
                }
                Err(_) => {
                    scores = lexical_scores;
                    degraded_reason = Some("向量检索超过预算或不可用，本次使用字面结果".into());
                    // An interrupted vector scan must not cancel bounded reads
                    // of lexical evidence already selected successfully.
                    db.progress_handler(0, None::<fn() -> bool>)?;
                }
            }
        } else {
            scores = lexical_scores;
            degraded_reason = Some("索引配置已变化，本次使用字面结果".into());
        }
    }
    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|a, b| {
        b.1.2
            .total_cmp(&a.1.2)
            .then(b.1.1.cmp(&a.1.1))
            .then(a.0.cmp(&b.0))
    });
    let mut has_more = ranked.len() > request.offset + request.limit || candidate_truncated;
    let mut chars_left = 12_000;
    let mut bytes_left = 48 * 1024 - 256;
    let mut items = vec![];
    let mut truncated = candidate_truncated;
    for (memory_id, (version_id, updated_at, score)) in
        ranked.into_iter().skip(request.offset).take(request.limit)
    {
        if chars_left == 0 {
            has_more = true;
            truncated = true;
            break;
        }
        let evidence = records::resolve_excerpt(
            db,
            &SourceRef::Version(version_id.clone()),
            request.excerpt_chars.min(chars_left),
            &queries,
            windows.get(&memory_id).copied(),
        )?;
        chars_left -= evidence.text.chars().count();
        let mut origins: Vec<Origin> = db.prepare("SELECT DISTINCT c.source FROM version_captures vc JOIN captures c ON c.id=vc.capture_id WHERE vc.version_id=? AND c.source IS NOT NULL ORDER BY c.source LIMIT 5")?
            .query_map([&version_id],|r|r.get::<_,String>(0))?.map(|v| serde_json::from_str(&v?).map_err(|_|DataError::Integrity)).collect::<Result<Vec<_>>>()?;
        if origins.len() > 4 {
            origins.truncate(4);
            truncated = true;
        }
        let hit = SearchHit {
            memory_id,
            version_id,
            title: evidence.title.clone(),
            evidence,
            origins,
            updated_at,
            score,
        };
        let size = serde_json::to_vec(&hit)
            .map_err(|_| DataError::Invalid)?
            .len();
        if size > bytes_left {
            has_more = true;
            truncated = true;
            break;
        }
        bytes_left -= size;
        items.push(hit);
    }
    Ok(SearchResult {
        mode: if hybrid { "hybrid" } else { "lexical" }.into(),
        degraded_reason,
        next_offset: has_more.then_some(request.offset + items.len()),
        items,
        has_more,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    #[test]
    fn hybrid_keeps_exact_window_filters_before_topk_and_rejects_stale_vectors() {
        let d = tempfile::tempdir().unwrap();
        let s = MemoryStore::open(d.path()).unwrap();
        let mut db = s.connection().unwrap();
        let mut v = vec![0.0; 1024];
        v[0] = 1.0;
        let bytes = crate::embedding::vector_bytes(&v).unwrap();
        let mut target = None;
        for n in 0..30 {
            let capture = s
                .capture(&CaptureRequest {
                    request_id: id(),
                    text: format!(
                        "{} precise-entity-{n} costs 79 euros.",
                        "background ".repeat(200)
                    ),
                    origin: Origin::User {
                        app: "QA".into(),
                        project: Some(if n == 0 { "inside" } else { "outside" }.into()),
                        uri: None,
                    },
                })
                .unwrap();
            db.execute(
                "INSERT INTO embedding_chunks VALUES(?1,?2,0,0,50,'synthetic',?3)",
                params![capture.memory_id, capture.version_id, bytes],
            )
            .unwrap();
            if n == 0 {
                target = Some(capture);
            }
        }
        super::super::embedding::reset(&db).unwrap(); // reset before seeding the final vectors
        for (memory, version) in db
            .prepare("SELECT id,current_version_id FROM memories")
            .unwrap()
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
        {
            db.execute(
                "INSERT INTO embedding_chunks VALUES(?1,?2,0,0,50,'synthetic',?3)",
                params![memory, version, bytes],
            )
            .unwrap();
        }
        db.execute("UPDATE embedding_index_meta SET state='ready'", [])
            .unwrap();
        let meta = super::super::embedding::meta(&db).unwrap().unwrap();
        let request = SearchRequest {
            query: "precise-entity-0".into(),
            scope: SearchScope {
                project: Some("inside".into()),
                ..Default::default()
            },
            excerpt_chars: 160,
            ..Default::default()
        };
        let tx = db.transaction().unwrap();
        let found = search_in(
            &tx,
            &request,
            Some(&(meta.revision.clone(), bytes.clone())),
            None,
        )
        .unwrap();
        assert_eq!(found.items.len(), 1);
        assert!(found.items[0].evidence.text.contains("precise-entity-0"));
        drop(tx);
        let target = target.unwrap();
        s.edit_memory(&EditRequest {
            request_id: id(),
            memory_id: target.memory_id,
            expected_version: target.version_id,
            title: "changed".into(),
            body: "new content".into(),
        })
        .unwrap();
        let found = search_in(&db, &request, Some(&(meta.revision, bytes)), None).unwrap();
        assert!(found.items.is_empty());
    }
}
