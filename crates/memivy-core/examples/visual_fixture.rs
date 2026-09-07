//! Seed only a newly created explicit directory for repeatable native visual QA.
#[allow(dead_code)]
#[path = "../tests/support/corpus.rs"]
mod corpus;
use corpus::*;
use memivy_core::memory::*;
use serde_json::json;
use std::{fs, path::PathBuf};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 2, "visual_fixture FRESH_ABSOLUTE_DIR");
    let dir = PathBuf::from(&args[1]);
    assert!(dir.is_absolute());
    fs::create_dir(&dir).expect("must be a fresh directory");
    let store = MemoryStore::open(&dir).unwrap();
    let fixture: Corpus = serde_json::from_str(SEARCH).unwrap();
    let seeded = seed(&store, &fixture);
    let topic = id();
    store
        .create_conversation(&topic, "木桥 · 固定讨论")
        .unwrap();
    let source = SourceRef::Version(store.memory(&seeded.records["history"]).unwrap().current.id);
    let turn = store
        .start_turn(
            &id(),
            &topic,
            "木桥现在先做什么？",
            std::slice::from_ref(&source),
        )
        .unwrap();
    store
        .finish_turn(
            &turn.id,
            "木桥现在先做桌面端。\n\n接着想：可以先验证首次记录后能否找回。",
            &[source],
        )
        .unwrap();
    store.check_integrity().unwrap();
    fs::write(dir.join("fixture.json"),serde_json::to_vec_pretty(&json!({"fixture_version":fixture.version,"records":seeded.records,"captures":seeded.captures,"topic":topic,"note":"Synthetic baseline evidence, not user visual acceptance. MCP off; no model configuration."})).unwrap()).unwrap();
    println!("Synthetic fixture ready; MCP off, no model configuration.");
}
