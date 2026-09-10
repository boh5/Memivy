//! Explicit synthetic regression and timing, using an already verified local model.
use memivy_core::{
    embedding::{self as emb, Preferences},
    memory::*,
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};
#[derive(Deserialize)]
struct Fixture {
    notes: Vec<Note>,
    queries: Vec<Query>,
}
#[derive(Deserialize)]
struct Note {
    id: String,
    body: String,
}
#[derive(Deserialize)]
struct Query {
    id: String,
    query: String,
    expected: Vec<String>,
    group: String,
}
#[tokio::main]
async fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("isolated root"));
    assert!(root.starts_with("/private/tmp/") && root.is_absolute());
    let resume = std::env::args().any(|a| a == "--resume");
    assert!(
        resume || !root.join("memivy.db").exists(),
        "use a fresh QA root or explicitly resume an interrupted evaluation"
    );
    let store = MemoryStore::open(&root).unwrap();
    let fixture: Fixture =
        serde_json::from_str(include_str!("../tests/fixtures/embedding_retrieval.json")).unwrap();
    let mut ids = BTreeMap::new();
    for n in &fixture.notes {
        if resume {
            let page = store
                .library(&LibraryQuery {
                    project: Some(n.id.clone()),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(page.items.len(), 1);
            ids.insert(n.id.clone(), page.items[0].key.id.clone());
            continue;
        }
        let request = uuid::Uuid::new_v4().to_string();
        let c = store
            .capture(&CaptureRequest {
                request_id: request,
                text: n.body.clone(),
                origin: Origin::User {
                    app: "Embedding regression".into(),
                    project: Some(n.id.clone()),
                    uri: None,
                },
            })
            .unwrap();
        ids.insert(n.id.clone(), c.memory_id);
    }
    Preferences {
        preparing: true,
        ..Default::default()
    }
    .save(&root)
    .unwrap();
    if resume {
        store.embedding_control("retry").unwrap();
    }
    for _ in 0..(fixture.notes.len() + 10) {
        store.embedding_tick().await;
        let s = store.embedding_status().unwrap();
        if let Some(e) = s.error {
            panic!("{e}");
        }
        if s.state == "ready" {
            break;
        }
    }
    assert_eq!(store.embedding_status().unwrap().state, "ready");
    let mut summary = BTreeMap::new();
    let mut failures = vec![];
    for hybrid in [false, true] {
        Preferences {
            enabled: hybrid,
            ..Default::default()
        }
        .save(&root)
        .unwrap();
        if hybrid {
            emb::client::warmup(&root).unwrap();
            let start = Instant::now();
            while !emb::client::ready(&root) {
                assert!(start.elapsed() < Duration::from_secs(60));
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        for q in &fixture.queries {
            let began = Instant::now();
            let found = store.search(&SearchRequest::text(&q.query, 8)).unwrap();
            let elapsed = began.elapsed().as_secs_f64() * 1000.;
            if hybrid {
                assert_eq!(found.mode, "hybrid", "{} {:?}", q.id, found.degraded_reason);
            }
            let rank = found
                .items
                .iter()
                .position(|r| q.expected.iter().any(|id| ids[id] == r.memory_id))
                .map(|n| n + 1);
            let key = format!("{}:{}", if hybrid { "hybrid" } else { "lexical" }, q.group);
            let entry = summary.entry(key).or_insert((0, 0, 0.0, vec![]));
            entry.0 += 1;
            entry.1 += usize::from(rank.is_some());
            entry.2 += rank.map_or(0.0, |n| 1.0 / n as f64);
            entry.3.push(elapsed);
            if hybrid && rank.is_none() {
                failures.push(q.id.clone());
            }
        }
    }
    let mut out = BTreeMap::new();
    for (k, (n, hit, mrr, mut time)) in summary {
        time.sort_by(f64::total_cmp);
        out.insert(k,serde_json::json!({"queries":n,"recall_at_8":hit as f64/n as f64,"mrr_at_8":mrr/n as f64,"p95_ms":time[(time.len()*95/100).min(time.len()-1)]}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({"groups":out,"hybrid_failures":failures}))
            .unwrap()
    );
    // A real long-document encoding unit, not prefilled vector fixtures.
    let long = store
        .capture(&CaptureRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            text: format!(
                "# 长文建库\n\n{}\n# Final\nThe receipt number is END-4792.",
                "This paragraph records a local experiment. 中文内容保留不确定性。\n\n".repeat(400)
            ),
            origin: Origin::User {
                app: "Long document QA".into(),
                project: Some("long-document".into()),
                uri: None,
            },
        })
        .unwrap();
    let start = Instant::now();
    store.embedding_tick().await;
    println!("long_document_index_ms={}", start.elapsed().as_millis());
    let result = store
        .search(&SearchRequest {
            query: "What is the receipt number in the final section?".into(),
            scope: SearchScope {
                project: Some("long-document".into()),
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    assert_eq!(result.items[0].memory_id, long.memory_id);
    assert!(result.items[0].evidence.text.contains("END-4792"));
    // Negative scope/history eligibility is exact even when a vector is similar.
    let source = &fixture.notes[0];
    let memory = &ids[&source.id];
    let current = store.memory(memory).unwrap().current;
    store
        .edit_memory(&EditRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            memory_id: memory.clone(),
            expected_version: current.id,
            title: "changed".into(),
            body: "Completely different current fact.".into(),
        })
        .unwrap();
    let result = store
        .search(&SearchRequest {
            query: "offline drive".into(),
            scope: SearchScope {
                project: Some(source.id.clone()),
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    assert!(result.items.is_empty());
    // Preserve model readiness for the independent process/performance checks.
    assert!(
        emb::client::encode(
            &root,
            "query",
            &emb::chunk::query("中文 English 日本語").unwrap(),
            Duration::from_secs(2)
        )
        .is_ok()
    );
    println!("Scope, stale-index and long-document checks passed");
}
