use super::*;
use std::collections::HashMap;

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
    let mut scores: HashMap<String, (f64, RankedRow)> = HashMap::new();
    for query in queries {
        if query.trim().is_empty() {
            continue;
        }
        let page = store.library(&LibraryQuery {
            query: query.clone(),
            limit: 12,
            ..Default::default()
        })?;
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
