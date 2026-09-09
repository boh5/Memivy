//! On-demand local related memories; no model calls or persistent recommendation state.
use super::{db::valid_id, records, retrieval, *};
use rusqlite::params;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub struct RelatedMemory {
    pub memory_id: String,
    pub version_id: String,
    pub title: String,
    pub snippet: String,
    pub source: SourceRef,
}

impl MemoryStore {
    pub fn related_memories(
        &self,
        memory_id: &str,
        expected_version: &str,
    ) -> Result<Vec<RelatedMemory>> {
        self.related_memories_in_collection(memory_id, expected_version, None)
    }
    pub fn related_memories_in_collection(
        &self,
        memory_id: &str,
        expected_version: &str,
        collection: Option<&str>,
    ) -> Result<Vec<RelatedMemory>> {
        valid_id(memory_id)?;
        valid_id(expected_version)?;
        let mut db = self.connection()?;
        retrieval::budget(&db)?;
        let tx = db.transaction()?;
        let current = records::head(&tx, memory_id, expected_version)?;
        if let Some(collection) = collection
            && !super::navigation::source_in_collection(
                &tx,
                collection,
                &SourceRef::Version(expected_version.into()),
            )?
        {
            return Err(DataError::Unavailable);
        }
        let mut terms: Vec<String> = tx.prepare("SELECT k.terms FROM capture_keywords k JOIN version_captures vc ON vc.capture_id=k.capture_id JOIN capture_state cs ON cs.capture_id=k.capture_id WHERE vc.version_id=? AND cs.availability='active' ORDER BY k.capture_id LIMIT 8")?
            .query_map([expected_version], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?
            .into_iter().flat_map(|s| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>()).collect();
        terms.extend(retrieval::capture_terms(&current.title));
        let mut seen = std::collections::HashSet::new();
        terms.retain(|s| s.chars().count() >= 2 && s.len() <= 80 && seen.insert(s.to_lowercase()));
        terms.truncate(4);
        let mut fallback =
            retrieval::capture_terms(&current.body.chars().take(2000).collect::<String>());
        fallback.retain(|s| !terms.contains(s));
        fallback.truncate(4);
        let mut ranked: HashMap<String, (f64, String, SourceRef)> = HashMap::new();
        // Start from indexed version/capture IDs, never scan all memory heads per hit.
        let mut targets = tx.prepare_cached("SELECT m.id,m.current_version_id FROM memory_versions v JOIN memories m ON m.id=v.memory_id WHERE v.id=?1 AND m.current_version_id=v.id AND m.state='active' AND m.id!=?3 AND (?4 IS NULL OR EXISTS(SELECT 1 FROM collection_entries ce WHERE ce.collection_id=?4 AND ce.kind='memory' AND ce.record_id=m.id))
            UNION ALL SELECT m.id,m.current_version_id FROM version_captures vc INDEXED BY versions_by_capture CROSS JOIN memory_versions v CROSS JOIN memories m WHERE vc.capture_id=?2 AND v.id=vc.version_id AND m.id=v.memory_id AND m.current_version_id=v.id AND m.state='active' AND m.id!=?3 AND (?4 IS NULL OR EXISTS(SELECT 1 FROM collection_entries ce WHERE ce.collection_id=?4 AND ce.kind='memory' AND ce.record_id=m.id)) LIMIT 8")?;
        for (pass, queries) in [terms.clone(), fallback].into_iter().enumerate() {
            if pass == 1 {
                if ranked.len() >= 3 {
                    break;
                }
                terms.extend(queries.clone());
            }
            let hits = retrieval::source_hits_in_collection(
                &tx,
                &queries,
                retrieval::VersionScope::Current,
                collection,
            )?;
            for hit in hits {
                let (version, capture) = match &hit.source {
                    SourceRef::Version(id) => (Some(id), None),
                    SourceRef::Capture(id) => (None, Some(id)),
                };
                for row in targets
                    .query_map(params![version, capture, memory_id, collection], |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                    })?
                {
                    let (id, version) = row?;
                    ranked
                        .entry(id)
                        .and_modify(|r| {
                            if hit.score > r.0 {
                                r.0 = hit.score;
                                r.2 = hit.source.clone();
                            }
                        })
                        .or_insert((hit.score, version, hit.source.clone()));
                }
            }
        }
        let mut ranked: Vec<_> = ranked.into_iter().collect();
        ranked.sort_by(|a, b| b.1.0.total_cmp(&a.1.0).then(a.0.cmp(&b.0)));
        ranked
            .into_iter()
            .take(3)
            .map(|(memory_id, (_, version_id, source))| {
                let e = records::resolve_excerpt(&tx, &source, 160, &terms, None)?;
                Ok(RelatedMemory {
                    memory_id,
                    version_id: version_id.clone(),
                    title: tx.query_row(
                        "SELECT title FROM memory_versions WHERE id=?",
                        [&version_id],
                        |r| r.get(0),
                    )?,
                    snippet: e.text,
                    source,
                })
            })
            .collect()
    }
}
