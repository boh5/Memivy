//! Deterministic Markdown boundaries and Unicode-scalar source coordinates.
use super::*;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use unicode_normalization::UnicodeNormalization;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chunk {
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub hash: String,
}
fn normalized(s: &str) -> String {
    s.nfc().collect()
}
fn prefix(title: &str, heading: &str, header: &str) -> String {
    let parts: Vec<_> = [title, heading, header]
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        return String::new();
    }
    let allowance = (200 - parts.len()) / parts.len();
    parts
        .into_iter()
        .map(|p| normalized(p).chars().take(allowance).collect::<String>() + "\n")
        .collect()
}
pub fn query(text: &str) -> Result<String> {
    let s = normalized(text);
    if s.chars().count() > MAX_CHARS {
        return Err("The query is too long; using keyword search".into());
    }
    Ok(s)
}
pub fn split(title: &str, body: &str) -> Vec<Chunk> {
    if body.is_empty() {
        return vec![];
    }
    let whole = format!("{}{}", prefix(title, "", ""), normalized(body));
    if whole.chars().count() <= MAX_CHARS {
        return vec![Chunk {
            start: 0,
            end: body.chars().count(),
            hash: hash(whole.as_bytes()),
            text: whole,
        }];
    }
    let mut offsets: Vec<usize> = body.char_indices().map(|(p, _)| p).collect();
    offsets.push(body.len());
    let scalar = |byte: usize| offsets.binary_search(&byte).unwrap_or_else(|i| i);
    let mut boundaries = std::collections::BTreeSet::from([0, offsets.len() - 1]);
    let mut sections = vec![(0, String::new())];
    let mut heading_start = None;
    let mut table_start = None;
    let mut table_header = String::new();
    let mut tables = vec![];
    for (event, range) in
        Parser::new_ext(body, Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS).into_offset_iter()
    {
        match event {
            Event::Start(Tag::Table(_)) => {
                table_start = Some(scalar(range.start));
                table_header.clear();
            }
            Event::End(TagEnd::TableHead) => {
                table_header = body[range.clone()].trim().to_owned();
                boundaries.insert(scalar(range.end));
            }
            Event::End(TagEnd::Table) => {
                if let Some(start) = table_start.take() {
                    tables.push((start, scalar(range.end), table_header.clone()));
                }
            }
            Event::Start(Tag::Heading { .. }) => {
                heading_start = Some(range.start);
                boundaries.insert(scalar(range.start));
            }
            Event::End(TagEnd::Heading(_)) => {
                let a = heading_start.take().unwrap_or(range.start);
                sections.push((scalar(a), body[a..range.end].trim().to_owned()));
                boundaries.insert(scalar(range.end));
            }
            Event::End(TagEnd::Paragraph | TagEnd::Item | TagEnd::TableRow | TagEnd::CodeBlock) => {
                boundaries.insert(scalar(range.end));
            }
            _ => {}
        }
    }
    // Newlines and sentence endings split overlong structural units without
    // stripping source text; every source scalar remains covered.
    let mut natural = std::collections::BTreeSet::new();
    for (i, c) in body.chars().enumerate() {
        if matches!(c, '\n' | '。' | '！' | '？' | '.' | '!' | '?') {
            natural.insert(i + 1);
        }
    }
    let total = offsets.len() - 1;
    let mut at = 0;
    let mut result = vec![];
    while at < total {
        let section = sections.iter().rfind(|(p, _)| *p <= at).unwrap();
        let mut section_end = sections
            .iter()
            .find(|(p, _)| *p > at)
            .map_or(total, |(p, _)| *p);
        for (start, end, _) in &tables {
            for boundary in [*start, *end] {
                if boundary > at {
                    section_end = section_end.min(boundary);
                }
            }
        }
        let header = tables
            .iter()
            .find(|(start, end, _)| *start <= at && at < *end)
            .map_or("", |(_, _, h)| h.as_str());
        let pre = prefix(title, &section.1, header);
        let capacity = MAX_CHARS - pre.chars().count();
        let mut end = (at + capacity).min(section_end);
        // NFC can expand a few Unicode scalars. Enforce the bound on the actual
        // encoded copy, without changing the source or adding token counting.
        while normalized(&body[offsets[at]..offsets[end]]).chars().count() > capacity {
            end -= 1;
        }
        let mut forced = end < section_end;
        if forced {
            if let Some(bound) = boundaries.range((at + 1)..=end).next_back() {
                end = *bound;
                forced = false;
            } else if let Some(bound) = natural.range((at + 1)..=end).next_back() {
                end = *bound;
                forced = false;
            }
        }
        let text = format!("{pre}{}", normalized(&body[offsets[at]..offsets[end]]));
        result.push(Chunk {
            start: at,
            end,
            hash: hash(text.as_bytes()),
            text,
        });
        let next = if forced {
            end.saturating_sub(100).max(at + 1)
        } else {
            end
        };
        at = next;
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_markdown_is_bounded_and_fully_covered() {
        for body in [
            "中English🙂e\u{301}".repeat(800),
            format!(
                "# A\n\n{}\n# B\n\n| 列 | value |\n|--|--|\n{}\n```rust\n{}\n```",
                "句子。\n".repeat(400),
                "|内容|value|\n".repeat(400),
                "let x = 1;\n".repeat(400)
            ),
            "\u{0344}".repeat(3000),
        ] {
            let chunks = split(&"标题".repeat(200), &body);
            let mut covered = vec![false; body.chars().count()];
            for c in chunks {
                assert!(c.text.chars().count() <= 1000);
                assert!(c.end > c.start);
                for v in &mut covered[c.start..c.end] {
                    *v = true;
                }
            }
            assert!(covered.iter().all(|v| *v));
        }
    }
    #[test]
    fn long_tables_carry_headers_after_the_first_chunk() {
        let body = format!(
            "# Budget\n\n| Item | Price |\n|---|---|\n{}",
            "| Pencil | 3 euro |\n".repeat(200)
        );
        let chunks = split("Purchases", &body);
        assert!(chunks.len() > 2);
        for c in chunks.iter().skip(1) {
            assert!(c.text.contains("| Item | Price |"), "{}", c.text);
        }
    }
    #[test]
    fn short_notes_stay_whole_and_sections_do_not_overlap() {
        assert_eq!(split("Title", "Short note.").len(), 1);
        let b = format!(
            "# 第一节\n{}\n# 第二节\n{}",
            "甲".repeat(1500),
            "乙".repeat(1500)
        );
        let boundary = b[..b.find("# 第二节").unwrap()].chars().count();
        assert!(
            split("Title", &b)
                .iter()
                .all(|c| c.end <= boundary || c.start >= boundary)
        );
        assert_eq!(query(&"a".repeat(MAX_CHARS)).unwrap().len(), MAX_CHARS);
        assert!(query(&"a".repeat(MAX_CHARS + 1)).is_err());
        assert_eq!(query("e\u{301}").unwrap(), "é");
    }
}
