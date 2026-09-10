#[path = "support/corpus.rs"]
mod corpus;
use corpus::*;
use memivy_core::memory::*;
use std::collections::BTreeSet;

#[test]
fn fixed_search_corpus_matches_library_and_mcp_before_and_after_rebuild() {
    let fixture: Corpus = serde_json::from_str(SEARCH).unwrap();
    assert_eq!(fixture.version, 2);
    let temp = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(temp.path()).unwrap();
    let data = seed(&store, &fixture);
    store.set_mcp_enabled(true).unwrap();
    for rebuilding in [false, true] {
        if rebuilding {
            store.rebuild_search_index().unwrap();
        }
        for case in &fixture.queries {
            let q = LibraryQuery {
                query: case.query.clone(),
                origin: case.origin.clone(),
                project: case.project.clone(),
                since: case.since_at.as_ref().map(|k| data.updated[k]),
                until: case.until_at.as_ref().map(|k| data.updated[k]),
                ..Default::default()
            };
            let expected: BTreeSet<_> = case
                .expected
                .iter()
                .map(|k| data.records[k].clone())
                .collect();
            let page = store.library(&q).unwrap();
            let actual: BTreeSet<_> = page.items.iter().map(|r| r.key.id.clone()).collect();
            assert_eq!(actual, expected, "{} rebuild={rebuilding}", case.id);
            let mcp = store
                .mcp_search(&McpSearchQuery {
                    query: q.query,
                    origin: q.origin,
                    project: q.project,
                    since: q.since,
                    until: q.until,
                    limit: Some(8),
                })
                .unwrap();
            assert_eq!(
                mcp.items
                    .iter()
                    .map(|r| r.record.id.clone())
                    .collect::<BTreeSet<_>>(),
                expected,
                "MCP {}",
                case.id
            );
            for hit in mcp.items {
                assert!(
                    !store
                        .resolve_source(&hit.source, 1800)
                        .unwrap()
                        .text
                        .is_empty()
                );
            }
        }
    }
    store.check_integrity().unwrap();
    println!(
        "{} fixed queries x library/MCP x original/rebuilt; SQLite {}",
        fixture.queries.len(),
        rusqlite::version()
    );
}

#[test]
fn fixed_near_duplicates_rank_title_and_filter_before_bounded_results() {
    let fixture: Corpus = serde_json::from_str(SEARCH).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(temp.path()).unwrap();
    let data = seed(&store, &fixture);
    store.set_mcp_enabled(true).unwrap();
    for n in 0..55 {
        let saved = store
            .capture(&CaptureRequest {
                request_id: id(),
                text: format!("相近结果 正文干扰 {n}"),
                origin: Origin::User {
                    app: "fixture".into(),
                    project: Some("干扰项目".into()),
                    uri: None,
                },
            })
            .unwrap();
        store
            .edit_memory(&EditRequest {
                request_id: id(),
                memory_id: saved.memory_id,
                expected_version: saved.version_id,
                title: format!("正文干扰项 {n}"),
                body: format!("相近结果 正文干扰 {n}"),
            })
            .unwrap();
    }
    let page = store
        .library(&LibraryQuery {
            query: "相近结果".into(),
            limit: 7,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.items[0].key.id, data.records["ranking"]);
    assert!(page.next_offset.is_some());
    let page = store
        .mcp_search(&McpSearchQuery {
            query: "相近结果".into(),
            limit: Some(8),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.items.len(), 8);
    assert!(page.has_more);
    assert_eq!(page.items[0].record.id, data.records["ranking"]);
    let filtered = store
        .mcp_search(&McpSearchQuery {
            query: "相近结果".into(),
            project: Some("唯一项目".into()),
            origin: Some("agent".into()),
            limit: Some(1),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(filtered.items.len(), 1);
    assert!(!filtered.has_more);
    assert_eq!(filtered.items[0].record.id, data.records["ranking"]);
}
