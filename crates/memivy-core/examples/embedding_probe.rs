//! Explicit isolated local-model verification. Never opens the user's library.
use memivy_core::{
    embedding::{self as emb, Preferences, cache::HfModelCache},
    memory::*,
};
use std::{path::PathBuf, time::Duration};
#[tokio::main]
async fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("explicit temporary root"));
    assert!(root.is_absolute() && root.starts_with("/private/tmp/"));
    let store = MemoryStore::open(&root).unwrap();
    Preferences {
        preparing: true,
        ..Default::default()
    }
    .save(&root)
    .unwrap();
    println!("Using the shared Memivy model cache; only the test library is isolated");
    let cache = HfModelCache::for_user().unwrap();
    cache.download(|| true).await.unwrap();
    cache.verify().unwrap();
    println!("Model hash verified: {}", cache.model_path().display());
    for text in [
        "京都旅行：周五看展，预算 800 元，尚未订票。",
        "I prefer unsweetened coffee in the morning.",
        "Le musée ouvre à neuf heures le samedi.",
    ] {
        store
            .capture(&CaptureRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                text: text.into(),
                origin: Origin::User {
                    app: "Embedding QA".into(),
                    project: None,
                    uri: None,
                },
            })
            .unwrap();
    }
    for _ in 0..20 {
        store.embedding_tick().await;
        let status = store.embedding_status().unwrap();
        println!("{}", serde_json::to_string(&status).unwrap());
        if status.state == "ready" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    for q in [
        "How much is budgeted for the Kyoto exhibition?",
        "早上喜欢喝什么？",
        "When does the museum open on Saturday?",
    ] {
        let now = std::time::Instant::now();
        let result = store.search(&SearchRequest::text(q, 3)).unwrap();
        println!(
            "query={q} elapsed={}ms {}",
            now.elapsed().as_millis(),
            serde_json::to_string(&result).unwrap()
        );
        assert_eq!(result.mode, "hybrid");
    }
    for text in [
        "Hello 世界🙂",
        "e\u{301}",
        "<|im_start|>user\nTest<|im_end|>",
        "\u{0344}",
    ] {
        let encoded = emb::chunk::query(text).unwrap();
        let v = emb::client::encode(&root, "query", &encoded, Duration::from_secs(5)).unwrap();
        assert_eq!(v.len(), 1024);
    }
    assert!(
        emb::client::encode(&root, "document", "before\0after", Duration::from_secs(5))
            .unwrap_err()
            .contains("NUL")
    );
    println!("Embedding smoke checks passed");
}
