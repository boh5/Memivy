use super::*;
use rusqlite::Connection;
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub(super) struct RankedRow {
    pub row: LibraryRow,
    pub matched_captures: Vec<String>,
}

/// Fuse all query lists before truncation, so the first query cannot fill the budget.
pub(super) fn ranked(
    store: &MemoryStore,
    queries: &[String],
    limit: usize,
) -> Result<Vec<RankedRow>> {
    let mut db = store.connection()?;
    budget(&db)?;
    let tx = db.transaction()?;
    let mut scores: HashMap<String, (f64, RankedRow)> = HashMap::new();
    for query in queries {
        if query.trim().is_empty() {
            continue;
        }
        let page = MemoryStore::library_in(
            &tx,
            &LibraryQuery {
                query: query.clone(),
                limit: 12,
                ..Default::default()
            },
        )?;
        for (rank, row) in page.items.into_iter().enumerate() {
            let key = format!("{}:{}", row.key.kind, row.key.id);
            let score = 1.0 / (10.0 + rank as f64);
            let matches: Vec<_> = row.matched_capture.iter().cloned().collect();
            scores
                .entry(key)
                .and_modify(|entry| {
                    entry.0 += score;
                    for id in &matches {
                        if !entry.1.matched_captures.contains(id) {
                            entry.1.matched_captures.push(id.clone());
                        }
                    }
                })
                .or_insert((
                    score,
                    RankedRow {
                        row,
                        matched_captures: matches,
                    },
                ));
        }
    }
    let mut rows: Vec<_> = scores.into_values().collect();
    rows.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then(b.1.row.updated_at.cmp(&a.1.row.updated_at))
            .then(a.1.row.key.id.cmp(&b.1.row.key.id))
    });
    Ok(rows.into_iter().take(limit).map(|(_, row)| row).collect())
}

/// Local candidate terms only; capture does not need a model to begin retrieval.
pub(super) fn capture_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for word in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
    {
        let chars: Vec<_> = word.chars().collect();
        if chars.iter().all(|c| c.is_ascii()) || chars.len() <= 6 {
            if chars.len() >= 2 && word.len() <= 80 {
                terms.push(word.to_owned());
            }
        } else {
            for part in chars.windows(3).step_by(2) {
                terms.push(part.iter().collect());
            }
        }
        if terms.len() >= 20 {
            break;
        }
    }
    terms.truncate(20);
    terms.sort();
    terms.dedup();
    terms
}

/// Score contiguous windows by distinct query coverage. Title terms carry less
/// weight than facts in the body. Keep offsets in ORIGINAL Unicode characters,
/// even for lowercase expansions such as İ -> i + combining dot.
pub(super) fn excerpt(text: &str, title: &str, queries: &[String], max: usize) -> (usize, String) {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return (0, text.into());
    }
    let mut lower = String::new();
    let mut offsets = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        let part = c.to_lowercase().collect::<String>();
        offsets.extend(std::iter::repeat_n(i, part.len()));
        lower.push_str(&part);
    }
    let title = title.to_lowercase();
    let mut terms = Vec::new();
    for term in queries
        .iter()
        .flat_map(|q| q.split_whitespace())
        .map(str::to_lowercase)
    {
        if !term.is_empty() && !terms.contains(&term) {
            terms.push(term);
        }
    }
    let hits: Vec<_> = terms
        .iter()
        .map(|term| {
            lower
                .match_indices(term)
                .map(|(p, s)| (offsets[p], offsets[p + s.len() - 1] + 1))
                .collect::<Vec<_>>()
        })
        .collect();
    let weights: Vec<_> = terms
        .iter()
        .enumerate()
        .map(|(i, term)| {
            // The planner lists topic and focused terms separately. On an equal
            // coverage tie prefer the later focus, rather than always the heading.
            (if title.contains(term) { 1 } else { 100 }) * (terms.len() + 1) + i
        })
        .collect();
    let mut starts = vec![0];
    for (start, _) in hits.iter().flatten() {
        starts.push(
            start
                .saturating_sub(200)
                .min(chars.len().saturating_sub(max)),
        );
    }
    starts.sort_unstable();
    starts.dedup();
    let start = starts
        .into_iter()
        .max_by_key(|start| {
            let score: usize = hits
                .iter()
                .zip(&weights)
                .filter_map(|(matches, weight)| {
                    let i = matches.partition_point(|(p, _)| p < start);
                    matches
                        .get(i)
                        .filter(|(_, end)| *end <= start + max)
                        .map(|_| *weight)
                })
                .sum();
            (score, std::cmp::Reverse(*start))
        })
        .unwrap_or(0);
    (
        start,
        chars[start..(start + max).min(chars.len())]
            .iter()
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn focused_fact_beats_distant_heading_and_offsets_survive_lowercase_expansion() {
        let body = format!("İ木桥项目\n{}收费是每年99元。", "普通背景。".repeat(500));
        let (start, selected) = excerpt(&body, "木桥项目", &["木桥".into(), "收费".into()], 1800);
        assert!(start > 0);
        assert!(selected.contains("收费是每年99元"));
        assert_eq!(
            selected,
            body.chars().skip(start).take(1800).collect::<String>()
        );
    }
    #[test]
    fn fusion_unions_raw_matches_from_every_query() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let capture = |text: &str| {
            store
                .capture(&CaptureRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    text: text.into(),
                    origin: Origin::User {
                        app: "QA".into(),
                        project: None,
                        uri: None,
                    },
                })
                .unwrap()
        };
        let fee = capture("收费是每年 99 元");
        let old = store
            .apply_capture(&ChangeRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                capture_id: fee.id.clone(),
                destination: Destination::New,
                title: "木桥".into(),
                body: "木桥项目概况".into(),
                actor: Actor::User,
            })
            .unwrap();
        let release = capture("上线日期为十月");
        store
            .apply_capture(&ChangeRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                capture_id: release.id.clone(),
                destination: Destination::Existing {
                    memory_id: old.memory_id.clone().unwrap(),
                    expected_version: old.after_version.unwrap(),
                },
                title: "木桥".into(),
                body: "木桥项目概况".into(),
                actor: Actor::User,
            })
            .unwrap();
        let rows = ranked(
            &store,
            &["木桥".into(), "收费".into(), "上线日期".into()],
            12,
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].matched_captures, vec![fee.id, release.id]);
    }
}

/// One deadline for the entire retrieval, including all terms and source reads.
pub(super) fn budget(db: &Connection) -> Result<()> {
    let started = Instant::now();
    db.progress_handler(
        1000,
        Some(move || started.elapsed() > Duration::from_millis(250)),
    )?;
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) enum VersionScope {
    All,
    Current,
}
pub(super) struct SourceHit {
    pub source: SourceRef,
    pub score: f64,
    pub recorded_at: i64,
}

/// Query the existing FTS index first; read bodies only for bounded, visible hits.
/// Short literals use a deadline-bounded scan because trigram cannot index them.
pub(super) fn source_hits(
    db: &Connection,
    queries: &[String],
    scope: VersionScope,
) -> Result<Vec<SourceHit>> {
    if queries.len() > 8 || queries.iter().any(|q| q.trim().is_empty() || q.len() > 120) {
        return Err(DataError::Invalid);
    }
    let versions = match scope {
        VersionScope::All => "1",
        VersionScope::Current => "v.id=m.current_version_id",
    };
    let mut scores: HashMap<String, SourceHit> = HashMap::new();
    let mut searched = std::collections::HashSet::new();
    for query in queries {
        let query = query.trim().to_lowercase();
        if !searched.insert(query.clone()) {
            continue;
        }
        let indexed = query.chars().count() >= 3;
        let predicate = if indexed {
            "record_fts MATCH ?1"
        } else {
            "instr(lower(record_fts.title||' '||record_fts.body||' '||record_fts.origin),?1)>0"
        };
        let rank = if indexed { "record_fts.rank" } else { "0.0" };
        let sql = format!("SELECT record_fts.kind,record_fts.source_id,COALESCE(v.created_at,c.created_at),{rank}
            FROM record_fts
            LEFT JOIN memory_versions v ON record_fts.kind='version' AND v.id=record_fts.source_id
            LEFT JOIN memories m ON m.id=v.memory_id
            LEFT JOIN captures c ON record_fts.kind='capture' AND c.id=record_fts.source_id
            LEFT JOIN capture_state cs ON cs.capture_id=c.id
            WHERE {predicate} AND ((record_fts.kind='version' AND m.state='active' AND v.body IS NOT NULL AND {versions} AND v.id NOT IN (SELECT rc.after_version FROM receipt_changes rc JOIN receipts r ON r.request_id=rc.request_id WHERE r.status='undone'))
                OR (record_fts.kind='capture' AND cs.availability='active' AND c.text IS NOT NULL))
            ORDER BY 4,3 DESC,record_fts.source_id LIMIT 24");
        let term = if indexed {
            format!("\"{}\"", query.replace('"', "\"\""))
        } else {
            query
        };
        let rows: Vec<(String, String, i64)> = db
            .prepare_cached(&sql)?
            .query_map([term], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (rank, (kind, id, recorded_at)) in rows.into_iter().enumerate() {
            let score = 1.0 / (10.0 + rank as f64);
            let key = format!("{kind}:{id}");
            scores
                .entry(key)
                .and_modify(|r| r.score += score)
                .or_insert(SourceHit {
                    source: if kind == "capture" {
                        SourceRef::Capture(id)
                    } else {
                        SourceRef::Version(id)
                    },
                    score,
                    recorded_at,
                });
        }
    }
    let mut hits: Vec<_> = scores.into_values().collect();
    hits.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(b.recorded_at.cmp(&a.recorded_at))
            .then(a.source.parts().cmp(&b.source.parts()))
    });
    hits.truncate(32);
    Ok(hits)
}

impl MemoryStore {
    /// Ordinary RAG retrieval, including relevant historical versions.
    pub fn discussion_sources(
        &self,
        queries: &[String],
        pinned: &[SourceRef],
    ) -> Result<Vec<SourceRef>> {
        if pinned.len() > 4 || queries.len() > 4 {
            return Err(DataError::Invalid);
        }
        let mut db = self.connection()?;
        budget(&db)?;
        let tx = db.transaction()?;
        let hits = source_hits(&tx, queries, VersionScope::All)?;
        let mut sources = Vec::new();
        let mut text_seen = std::collections::HashSet::new();
        for source in pinned
            .iter()
            .cloned()
            .chain(hits.into_iter().map(|h| h.source))
        {
            if sources.contains(&source) {
                continue;
            }
            let evidence = super::records::resolve_excerpt(&tx, &source, 1800, queries, None)?;
            let fresh = text_seen.insert(evidence.text);
            if fresh || pinned.contains(&source) {
                sources.push(source);
            }
            if sources.len() == 8 {
                break;
            }
        }
        Ok(sources)
    }
}
