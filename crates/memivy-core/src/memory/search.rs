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

/// A caller-authored semantic expression and literal keyword constraints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryQuery {
    pub text: String,
    pub keywords: Vec<String>,
}
impl MemoryQuery {
    pub fn text(text: impl Into<String>) -> Self {
        let text = text.into();
        let keywords = text.split_whitespace().map(str::to_owned).collect();
        Self { text, keywords }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SearchRequest {
    pub queries: Vec<MemoryQuery>,
    pub reference_memory_id: Option<String>,
    pub scope: SearchScope,
    pub limit: usize,
    pub offset: usize,
    pub excerpt_chars: usize,
}
impl Default for SearchRequest {
    fn default() -> Self {
        Self {
            queries: vec![],
            reference_memory_id: None,
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
            queries: vec![MemoryQuery::text(query)],
            limit,
            ..Self::default()
        }
    }
    fn validate(&self) -> Result<()> {
        let scope = &self.scope;
        if self.limit == 0
            || self.limit > 100
            || self.offset > 100_000
            || self.excerpt_chars == 0
            || self.excerpt_chars > 3000
            || (self.queries.is_empty() == self.reference_memory_id.is_none())
            || self.queries.len() > 4
            || self.queries.iter().any(|q| {
                q.text.trim().is_empty()
                    || q.text.len() > 512
                    || q.keywords.is_empty()
                    || q.keywords.len() > 128
                    || q.keywords.iter().any(|k| k.trim().is_empty())
                    || q.keywords.iter().map(String::len).sum::<usize>() > 512
            })
            || scope.exclude_memories.len() > 100
            || scope.project.as_ref().is_some_and(|v| v.len() > 200)
            || scope
                .origin
                .as_deref()
                .is_some_and(|v| !matches!(v, "user" | "agent" | "conversation"))
            || matches!((scope.since, scope.until), (Some(a), Some(b)) if a > b)
        {
            return Err(DataError::Invalid);
        }
        for id in scope
            .exclude_memories
            .iter()
            .chain(self.reference_memory_id.iter())
        {
            valid_id(id)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct QueryStatus {
    pub query_index: usize,
    pub keyword_complete: bool,
    /// completed, disabled, or failed; disabled is not a retrieval failure.
    pub semantic: String,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Default)]
pub(super) struct QueryVector {
    pub vector: Option<(String, Vec<u8>)>,
    pub error: Option<String>,
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
    pub matched_queries: Vec<usize>,
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
    pub queries: Vec<QueryStatus>,
    pub mode: String,
    pub degraded_reason: Option<String>,
    pub items: Vec<SearchHit>,
    pub has_more: bool,
    pub truncated: bool,
    pub next_offset: Option<usize>,
}

impl MemoryStore {
    pub fn search(&self, request: &SearchRequest) -> Result<SearchResult> {
        request.validate()?;
        let mut resolved = request.clone();
        if let Some(memory) = &request.reference_memory_id {
            let v = self.memory(memory)?.current;
            let text: String = format!("{} {}", v.title, v.body)
                .chars()
                .take(120)
                .collect();
            resolved.queries = retrieval::capture_terms(&text)
                .into_iter()
                .take(4)
                .map(|term| MemoryQuery {
                    text: text.clone(),
                    keywords: vec![term],
                })
                .collect();
            if resolved.queries.is_empty() {
                return Err(DataError::Invalid);
            }
        }
        for query in &mut resolved.queries {
            query.text = query.text.trim().to_owned();
            query.keywords = query
                .keywords
                .iter()
                .map(|word| word.trim().to_lowercase())
                .collect();
            query.keywords.sort();
            query.keywords.dedup();
        }
        let vectors = self.query_vectors(&resolved.queries);

        let mut db = self.connection()?;
        let started = Instant::now();
        db.progress_handler(
            1000,
            Some(move || started.elapsed() > Duration::from_millis(500)),
        )?;
        let tx = db.transaction()?;
        search_in(&tx, &resolved, &vectors)
    }
}

fn bind(values: &mut Vec<Value>, value: impl Into<Value>) -> String {
    values.push(value.into());
    format!("?{}", values.len())
}

struct LexicalCandidate {
    memory: String,
    version: String,
    updated: i64,
    rank: usize,
    queries: Vec<usize>,
}

struct SemanticCandidate {
    version: String,
    updated: i64,
    score: f64,
    start: usize,
    queries: Vec<usize>,
}

fn lexical_candidates(
    db: &Connection,
    queries: &[MemoryQuery],
    filters: &[String],
    mut values: Vec<Value>,
    budget: usize,
) -> Result<Vec<LexicalCandidate>> {
    let mut keys: Vec<Vec<String>> = vec![];
    let mut query_indices: Vec<Vec<usize>> = vec![];
    let mut definitions = vec![];
    let mut channels = vec![];
    for (query_index, query) in queries.iter().enumerate() {
        let mut key: Vec<_> = query
            .keywords
            .iter()
            .map(|k| k.trim().to_lowercase())
            .collect();
        key.sort();
        key.dedup();
        if let Some(index) = keys.iter().position(|existing| existing == &key) {
            query_indices[index].push(query_index);
            continue;
        }
        let index = keys.len();
        let mut predicates = filters.to_vec();
        let mut indexed = vec![];
        for word in &key {
            if word.chars().count() >= 3 {
                indexed.push(format!("\"{}\"", word.replace('"', "\"\"")));
            } else {
                let p = bind(&mut values, word.clone());
                predicates.push(format!("instr(lower(record_fts.title||' '||record_fts.body||' '||record_fts.origin),{p})>0"));
            }
        }
        let weight = if indexed.is_empty() {
            "0.0"
        } else {
            let p = bind(&mut values, indexed.join(" AND "));
            predicates.push(format!("record_fts MATCH {p}"));
            "bm25(record_fts,0.0,0.0,8.0,1.0,0.5)"
        };
        // Materialize scalar BM25 values before using a window function.
        definitions.push(format!(
            "lexical_{index} AS MATERIALIZED (SELECT m.id memory_id,v.id version_id,m.updated_at,{weight} weight FROM record_fts CROSS JOIN memory_versions v ON v.id=record_fts.source_id JOIN memories m ON m.id=v.memory_id AND m.current_version_id=v.id WHERE {})",
            predicates.join(" AND ")
        ));
        channels.push(format!("SELECT memory_id,version_id,updated_at,ROW_NUMBER() OVER (ORDER BY weight,updated_at DESC,memory_id) position,{index} query_index FROM lexical_{index}"));
        keys.push(key);
        query_indices.push(vec![query_index]);
    }
    // Apply the shared limit AFTER deduplication. Empty or overlapping queries
    // must not consume another query's pagination prefix.
    definitions.push(format!(
        "lexical_matches AS ({})",
        channels.join(" UNION ALL ")
    ));
    let sql = format!(
        "WITH {} SELECT memory_id,version_id,updated_at,min(position),json_group_array(query_index) FROM lexical_matches GROUP BY memory_id ORDER BY min(position),updated_at DESC,memory_id LIMIT {}",
        definitions.join(","),
        budget + 1
    );
    db.prepare(&sql)?
        .query_map(rusqlite::params_from_iter(values), |row| {
            Ok((
                LexicalCandidate {
                    memory: row.get(0)?,
                    version: row.get(1)?,
                    updated: row.get(2)?,
                    rank: row.get::<_, i64>(3)? as usize,
                    queries: vec![],
                },
                row.get::<_, String>(4)?,
            ))
        })?
        .map(|row| {
            let (mut candidate, indices) = row?;
            let indices: Vec<usize> =
                serde_json::from_str(&indices).map_err(|_| DataError::Integrity)?;
            candidate.queries = indices
                .into_iter()
                .flat_map(|i| query_indices[i].iter().copied())
                .collect();
            Ok(candidate)
        })
        .collect()
}

fn search_in(
    db: &Connection,
    request: &SearchRequest,
    vectors: &[QueryVector],
) -> Result<SearchResult> {
    let scope = &request.scope;
    let queries: Vec<String> = request
        .queries
        .iter()
        .flat_map(|q| q.keywords.clone())
        .collect();
    let mut statuses: Vec<_> = vectors
        .iter()
        .enumerate()
        .map(|(i, v)| QueryStatus {
            query_index: i,
            keyword_complete: false,
            semantic: if v.error.is_some() {
                "failed"
            } else if v.vector.is_some() {
                "completed"
            } else {
                "disabled"
            }
            .into(),
            error: v.error.clone(),
        })
        .collect();
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
            if field == "kind" && value == "conversation" {
                filters.push("EXISTS(SELECT 1 FROM version_captures vc JOIN captures c ON c.id=vc.capture_id WHERE vc.version_id=v.id AND json_extract(c.source,'$.kind') IN ('conversation','discussion'))".into());
                continue;
            }

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
    // Keep the two-channel cap at 2 * candidate_budget, independent of query
    // count: one shared window per channel, limited after deduplication.
    let candidate_budget = (request.offset + request.limit * 3).max(48);
    let mut matched: HashMap<String, Vec<usize>> = HashMap::new();
    let mut scores: HashMap<String, (String, i64, f64)> = HashMap::new();
    let rows = lexical_candidates(
        db,
        &request.queries,
        &filters,
        values.clone(),
        candidate_budget,
    )?;
    let mut candidate_truncated = rows.len() > candidate_budget;
    for status in &mut statuses {
        status.keyword_complete = true;
    }
    for row in rows.into_iter().take(candidate_budget) {
        matched.insert(row.memory.clone(), row.queries);
        scores.insert(
            row.memory,
            (row.version, row.updated, 1.0 / (60.0 + row.rank as f64)),
        );
    }
    let mut windows = HashMap::new();
    let mut semantic_scores: HashMap<String, SemanticCandidate> = HashMap::new();
    let mut semantic_queries: Vec<Vec<usize>> = vec![];
    for (index, value) in vectors.iter().enumerate() {
        if value.vector.is_none() {
            continue;
        }
        if let Some(indices) = semantic_queries.iter_mut().find(|indices| {
            request.queries[indices[0]].text.trim() == request.queries[index].text.trim()
        }) {
            indices.push(index);
        } else {
            semantic_queries.push(vec![index]);
        }
    }
    let mut hybrid = false;
    let mut scan_failed = false;
    for query_indices in semantic_queries {
        let value = &vectors[query_indices[0]];
        let Some((revision, vector)) = &value.vector else {
            continue;
        };
        let index = match super::embedding::meta(db) {
            Ok(index) => index,
            Err(_) => {
                scan_failed = true;
                for &index in &query_indices {
                    statuses[index].semantic = "failed".into();
                    statuses[index].error = Some("semantic_search_unavailable".into());
                }
                db.progress_handler(0, None::<fn() -> bool>)?;
                continue;
            }
        };
        if scan_failed || !index.is_some_and(|i| i.revision == *revision && i.state == "ready") {
            for &index in &query_indices {
                statuses[index].semantic = "failed".into();
                statuses[index].error = Some(
                    if scan_failed {
                        "semantic_search_budget"
                    } else {
                        "embedding_configuration_changed"
                    }
                    .into(),
                );
            }
            continue;
        }
        let mut args = values.clone();
        let p = bind(&mut args, vector.clone());
        // Group scalar distances before limiting: chunks must not crowd out memories.
        let sql = format!(
            "WITH distances AS MATERIALIZED (SELECT m.id memory_id,v.id version_id,m.updated_at,c.start_char,vec_distance_cosine(c.vector,{p}) distance FROM embedding_chunks c JOIN memories m ON m.id=c.memory_id AND m.current_version_id=c.version_id JOIN memory_versions v ON v.id=c.version_id WHERE {}) SELECT memory_id,version_id,updated_at,start_char,min(distance) distance FROM distances GROUP BY memory_id ORDER BY distance,updated_at DESC,memory_id LIMIT {}",
            filters.join(" AND "),
            candidate_budget + 1
        );
        let rows = (|| -> Result<Vec<(String, String, i64, usize)>> {
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
        match rows {
            Ok(rows) => {
                candidate_truncated |= rows.len() > candidate_budget;
                for (rank, (memory, version, updated, start)) in
                    rows.into_iter().take(candidate_budget).enumerate()
                {
                    let score = 1.0 / (61.0 + rank as f64);
                    semantic_scores
                        .entry(memory)
                        .and_modify(|row| {
                            if score > row.score {
                                row.score = score;
                                row.start = start;
                            }
                            row.queries.extend(&query_indices);
                        })
                        .or_insert(SemanticCandidate {
                            version,
                            updated,
                            score,
                            start,
                            queries: query_indices.clone(),
                        });
                }
                // Keep the best unique candidates after each successful query,
                // without retaining one full result buffer for every query.
                let mut ranked: Vec<_> = semantic_scores.into_iter().collect();
                ranked.sort_by(|a, b| {
                    b.1.score
                        .total_cmp(&a.1.score)
                        .then(b.1.updated.cmp(&a.1.updated))
                        .then(a.0.cmp(&b.0))
                });
                candidate_truncated |= ranked.len() > candidate_budget;
                semantic_scores = ranked.into_iter().take(candidate_budget).collect();
                hybrid = true;
            }
            Err(_) => {
                scan_failed = true;
                for &index in &query_indices {
                    statuses[index].semantic = "failed".into();
                    statuses[index].error = Some("semantic_search_budget".into());
                }
                // Keep already selected keyword evidence readable after a vector timeout.
                db.progress_handler(0, None::<fn() -> bool>)?;
            }
        }
    }
    // Each channel contributes at most once, irrespective of repeated reformulations.
    for (memory, row) in semantic_scores {
        matched
            .entry(memory.clone())
            .or_default()
            .extend(row.queries);
        if !scores.contains_key(&memory) {
            windows.insert(memory.clone(), row.start);
        }
        scores
            .entry(memory)
            .and_modify(|r| r.2 += row.score)
            .or_insert((row.version, row.updated, row.score));
    }
    db.progress_handler(0, None::<fn() -> bool>)?;
    let degraded_reason = statuses.iter().find_map(|s| s.error.clone());
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
        let mut matched_queries = matched.remove(&memory_id).unwrap_or_default();
        matched_queries.sort_unstable();
        matched_queries.dedup();
        let hit = SearchHit {
            matched_queries,
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
        queries: statuses,
        mode: if hybrid { "hybrid" } else { "lexical" }.into(),
        degraded_reason,
        next_offset: (has_more && !items.is_empty()).then_some(request.offset + items.len()),
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
    fn native_text_search_keeps_every_and_term_beyond_six() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let terms = [
            "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf",
        ];
        let mut expected = String::new();
        for omitted in 0..=terms.len() {
            let body = terms
                .iter()
                .enumerate()
                .rev()
                .filter(|(i, _)| *i != omitted)
                .map(|(_, term)| *term)
                .collect::<Vec<_>>()
                .join(" unrelated ");
            let saved = store
                .capture(&CaptureRequest {
                    request_id: id(),
                    text: body,
                    origin: Origin::User {
                        app: "QA".into(),
                        project: None,
                        uri: None,
                    },
                })
                .unwrap();
            if omitted == terms.len() {
                expected = saved.memory_id;
            }
        }
        let text = terms.join(" ");
        let page = store
            .library(&LibraryQuery {
                query: text.clone(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            page.items.len(),
            1,
            "Every term must match, independent of term order"
        );
        assert_eq!(page.items[0].key.id, expected);
        assert!(
            matches!(
                store.agent_read_tool(
                    "search_memories",
                    &serde_json::json!({
                        "queries":[MemoryQuery::text(text.clone())],"limit":5,"offset":0,
                        "origin":null,"project":null,"since":null,"until":null
                    })
                ),
                Err(DataError::Invalid)
            ),
            "The internal tool retains its six-keyword limit"
        );
        assert!(
            matches!(
                store.mcp_search(&McpSearchQuery {
                    queries: vec![MemoryQuery::text(text)],
                    ..Default::default()
                }),
                Err(DataError::Invalid)
            ),
            "MCP retains its six-keyword limit"
        );
        assert!(matches!(
            store.search(&SearchRequest::text(vec!["x"; 129].join(" "), 8)),
            Err(DataError::Invalid)
        ));
    }

    #[test]
    fn lexical_pagination_reaches_all_rows_with_empty_overlapping_and_duplicate_queries() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        for n in 0..91 {
            store
                .capture(&CaptureRequest {
                    request_id: id(),
                    text: format!(
                        "{} shared item {n:03}",
                        if n < 60 { "alpha" } else { "bravo" }
                    ),
                    origin: Origin::User {
                        app: "QA".into(),
                        project: Some(if n < 90 { "inside" } else { "outside" }.into()),
                        uri: None,
                    },
                })
                .unwrap();
        }
        for (terms, count) in [
            (["alpha", "absent", "missing", "unknown"], 60),
            (["alpha", "bravo", "shared", "unknown"], 90),
            (["alpha", "alpha", "ALPHA", "absent"], 60),
        ] {
            let mut request = SearchRequest {
                queries: terms.into_iter().map(MemoryQuery::text).collect(),
                scope: SearchScope {
                    project: Some("inside".into()),
                    ..Default::default()
                },
                limit: 100,
                excerpt_chars: 160,
                ..Default::default()
            };
            let expected: Vec<_> = store
                .search(&request)
                .unwrap()
                .items
                .into_iter()
                .map(|hit| hit.memory_id)
                .collect();
            assert_eq!(expected.len(), count);
            // The SQL boundary returns one shared candidate window plus one
            // lookahead, regardless of overlap, empty queries or query count.
            let db = store.connection().unwrap();
            for queries in [&request.queries[..1], request.queries.as_slice()] {
                let candidates = lexical_candidates(
                    &db,
                    queries,
                    &["m.state='active' AND v.body IS NOT NULL".into()],
                    vec![],
                    48,
                )
                .unwrap();
                assert_eq!(candidates.len(), 49);
            }
            request.limit = 5;
            let mut actual = vec![];
            loop {
                let page = store.search(&request).unwrap();
                assert!(
                    !page.items.is_empty(),
                    "A continuation must produce another page"
                );
                for hit in page.items {
                    assert!(
                        !actual.contains(&hit.memory_id),
                        "Pagination must not repeat a memory"
                    );
                    actual.push(hit.memory_id);
                }
                let Some(next) = page.next_offset else { break };
                assert!(next > request.offset, "The cursor must advance");
                assert!(actual.len() < count, "The final page must end pagination");
                request.offset = next;
            }
            assert_eq!(
                actual, expected,
                "Every lexical match must remain reachable in stable order"
            );
        }
    }

    #[test]
    fn semantic_pagination_reaches_all_rows_with_identical_query_rankings() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let db = store.connection().unwrap();
        super::super::embedding::reset(&db).unwrap();
        let mut vector = vec![0.0; 1024];
        vector[0] = 1.0;
        let bytes = crate::embedding::vector_bytes(&vector).unwrap();
        for n in 0..70 {
            let saved = store
                .capture(&CaptureRequest {
                    request_id: id(),
                    text: format!("Semantic evidence {n:03}"),
                    origin: Origin::User {
                        app: "QA".into(),
                        project: None,
                        uri: None,
                    },
                })
                .unwrap();
            db.execute(
                "INSERT INTO embedding_chunks VALUES(?1,?2,0,0,20,'synthetic',?3)",
                params![saved.memory_id, saved.version_id, bytes],
            )
            .unwrap();
        }
        db.execute("UPDATE embedding_index_meta SET state='ready'", [])
            .unwrap();
        let revision = super::super::embedding::meta(&db)
            .unwrap()
            .unwrap()
            .revision;
        let mut request = SearchRequest {
            queries: (0..4)
                .map(|n| MemoryQuery {
                    text: format!("Semantic wording {n}"),
                    keywords: vec!["no-lexical-match".into()],
                })
                .collect(),
            limit: 100,
            excerpt_chars: 160,
            ..Default::default()
        };
        let vectors = vec![
            QueryVector {
                vector: Some((revision, bytes)),
                error: None
            };
            4
        ];
        let expected: Vec<_> = search_in(&db, &request, &vectors)
            .unwrap()
            .items
            .into_iter()
            .map(|hit| hit.memory_id)
            .collect();
        assert_eq!(expected.len(), 70);
        request.limit = 5;
        let mut actual = vec![];
        loop {
            let page = search_in(&db, &request, &vectors).unwrap();
            assert!(page.queries.iter().all(|status| status.keyword_complete
                && status.semantic == "completed"
                && status.error.is_none()));
            assert!(
                !page.items.is_empty(),
                "A continuation must produce another page"
            );
            for hit in page.items {
                assert!(
                    !actual.contains(&hit.memory_id),
                    "Pagination must not repeat a memory"
                );
                assert_eq!(hit.matched_queries, vec![0, 1, 2, 3]);
                actual.push(hit.memory_id);
            }
            let Some(next) = page.next_offset else { break };
            assert!(next > request.offset);
            assert!(actual.len() < expected.len());
            request.offset = next;
        }
        assert_eq!(
            actual, expected,
            "All semantic matches must remain reachable after deduplication"
        );
        request.offset = 15;
        request.queries[3].text = request.queries[0].text.clone();
        let repeated = search_in(&db, &request, &vectors).unwrap();
        assert_eq!(
            repeated
                .items
                .iter()
                .map(|hit| hit.memory_id.clone())
                .collect::<Vec<_>>(),
            expected[15..20]
        );
        assert!(
            repeated
                .items
                .iter()
                .all(|hit| hit.matched_queries == [0, 1, 2, 3])
        );
        assert!(
            repeated
                .queries
                .iter()
                .all(|status| status.semantic == "completed")
        );
    }

    #[test]
    fn complementary_semantic_queries_and_partial_failures_share_one_result_limit() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let db = store.connection().unwrap();
        super::super::embedding::reset(&db).unwrap();
        let mut ids = vec![];
        let vector = |x, y| {
            let mut v = vec![0.0; 1024];
            v[0] = x;
            v[1] = y;
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            for value in &mut v {
                *value /= norm;
            }
            crate::embedding::vector_bytes(&v).unwrap()
        };
        for (text, x, y) in [
            ("Alpha first", 1.0, 0.0),
            ("Alpha second", 0.99, 0.01),
            ("Beta constraints", 0.0, 1.0),
        ] {
            let capture = store
                .capture(&CaptureRequest {
                    request_id: id(),
                    text: text.into(),
                    origin: Origin::User {
                        app: "QA".into(),
                        project: None,
                        uri: None,
                    },
                })
                .unwrap();
            db.execute(
                "INSERT INTO embedding_chunks VALUES(?1,?2,0,0,20,'synthetic',?3)",
                params![capture.memory_id, capture.version_id, vector(x, y)],
            )
            .unwrap();
            ids.push(capture.memory_id);
        }
        db.execute("UPDATE embedding_index_meta SET state='ready'", [])
            .unwrap();
        let revision = super::super::embedding::meta(&db)
            .unwrap()
            .unwrap()
            .revision;
        let request = SearchRequest {
            queries: vec![
                MemoryQuery {
                    text: "First aspect".into(),
                    keywords: vec!["missing-one".into()],
                },
                MemoryQuery {
                    text: "Second aspect".into(),
                    keywords: vec!["missing-two".into()],
                },
            ],
            limit: 2,
            ..Default::default()
        };
        let vectors = [
            QueryVector {
                vector: Some((revision.clone(), vector(1.0, 0.0))),
                error: None,
            },
            QueryVector {
                vector: Some((revision, vector(0.0, 1.0))),
                error: None,
            },
        ];
        let found = search_in(&db, &request, &vectors).unwrap();
        assert_eq!(found.items.len(), 2);
        assert!(found.items.iter().any(|hit| hit.memory_id == ids[0]));
        assert!(found.items.iter().any(|hit| hit.memory_id == ids[2]));
        assert!(
            found
                .queries
                .iter()
                .all(|q| q.keyword_complete && q.semantic == "completed")
        );
        let mut partial = request;
        partial.queries[1].keywords = vec!["Beta".into()];
        let found = search_in(
            &db,
            &partial,
            &[
                vectors[0].clone(),
                QueryVector {
                    vector: None,
                    error: Some("embedding_query_budget".into()),
                },
            ],
        )
        .unwrap();
        assert_eq!(found.items.len(), 2);
        assert!(
            found
                .items
                .iter()
                .any(|hit| hit.memory_id == ids[2] && hit.matched_queries.contains(&1))
        );
        assert_eq!(found.queries[1].semantic, "failed");
        assert_eq!(
            found.queries[1].error.as_deref(),
            Some("embedding_query_budget")
        );
        assert_eq!(
            found.degraded_reason.as_deref(),
            Some("embedding_query_budget")
        );
        partial.queries.push(MemoryQuery {
            text: "Third aspect after a scan failure".into(),
            keywords: vec!["missing-three".into()],
        });
        let (revision, _) = vectors[1].vector.as_ref().unwrap();
        let found = search_in(
            &db,
            &partial,
            &[
                vectors[0].clone(),
                QueryVector {
                    vector: Some((revision.clone(), vec![0; 4])),
                    error: None,
                },
                vectors[0].clone(),
            ],
        )
        .unwrap();
        assert_eq!(found.mode, "hybrid");
        assert_eq!(found.queries[0].semantic, "completed");
        assert!(
            found.queries[1..]
                .iter()
                .all(|status| status.semantic == "failed"
                    && status.error.as_deref() == Some("semantic_search_budget"))
        );
        assert!(found.queries.iter().all(|status| status.keyword_complete));
        assert!(
            found
                .items
                .iter()
                .any(|hit| hit.memory_id == ids[2] && hit.matched_queries.contains(&1)),
            "A vector scan failure must preserve independent keyword evidence"
        );
    }
    #[test]
    fn lexical_queries_match_terms_together_and_repeated_queries_do_not_vote_again() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        for text in [
            "Shanghai trip preparation",
            "Shanghai work budget",
            "Travel adapter checklist",
        ] {
            store
                .capture(&CaptureRequest {
                    request_id: id(),
                    text: text.into(),
                    origin: Origin::User {
                        app: "QA".into(),
                        project: None,
                        uri: None,
                    },
                })
                .unwrap();
        }
        let first = MemoryQuery {
            text: "Preparations for Shanghai".into(),
            keywords: vec!["Shanghai".into(), "preparation".into()],
        };
        let second = MemoryQuery {
            text: "Travel equipment".into(),
            keywords: vec!["adapter".into()],
        };
        let mut request = SearchRequest {
            queries: vec![first.clone(), second],
            limit: 8,
            ..Default::default()
        };
        let original = store.search(&request).unwrap();
        assert_eq!(original.items.len(), 2);
        request.queries.push(first);
        let repeated = store.search(&request).unwrap();
        assert_eq!(
            repeated
                .items
                .iter()
                .map(|hit| (&hit.memory_id, hit.score))
                .collect::<Vec<_>>(),
            original
                .items
                .iter()
                .map(|hit| (&hit.memory_id, hit.score))
                .collect::<Vec<_>>()
        );
        assert!(
            repeated
                .queries
                .iter()
                .all(|q| q.semantic == "disabled" && q.keyword_complete)
        );
        assert!(
            repeated
                .items
                .iter()
                .any(|hit| hit.matched_queries == [0, 2])
        );
    }
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
            queries: vec![MemoryQuery::text("precise-entity-0")],
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
            &[QueryVector {
                vector: Some((meta.revision.clone(), bytes.clone())),
                error: None,
            }],
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
        let found = search_in(
            &db,
            &request,
            &[QueryVector {
                vector: Some((meta.revision, bytes)),
                error: None,
            }],
        )
        .unwrap();
        assert!(found.items.is_empty());
    }
}
