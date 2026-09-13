use super::*;
use rusqlite::Connection;
use std::time::{Duration, Instant};

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
    let mut seen = std::collections::HashSet::new();
    terms.retain(|term| seen.insert(term.clone()));
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
                .saturating_sub((max / 4).min(200))
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

pub(super) fn budget(db: &Connection) -> Result<()> {
    let started = Instant::now();
    db.progress_handler(
        1000,
        Some(move || started.elapsed() > Duration::from_millis(250)),
    )?;
    Ok(())
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
}
