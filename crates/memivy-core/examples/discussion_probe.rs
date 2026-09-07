//! Synthetic, versioned Q&A quality probe; output contains no configuration secrets.
use memivy_core::{memory::*, model::ModelConfig};
use serde_json::json;
use std::{fs, path::PathBuf};
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3);
    let config = ModelConfig::read(std::path::Path::new(&args[1])).expect("private configuration");
    let output = PathBuf::from(&args[2]);
    fs::create_dir_all(&output).unwrap();
    let data = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(data.path()).unwrap();
    let capture = store
        .capture(&CaptureRequest {
            request_id: id(),
            text: "木桥项目先做网页端，不支持同步；只是我的初步想法。".into(),
            origin: Origin::User {
                app: "Synthetic probe".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap();
    let first = store.capture_as_new(&id(), &capture.id).unwrap();
    let current=store.edit_memory(&EditRequest {request_id:id(),memory_id:first.memory_id.clone().unwrap(),expected_version:first.after_version.clone().unwrap(),title:"木桥项目方向".into(),body:"木桥项目已经决定先做桌面端，不支持同步。网页优先是早期想法，已经改变。还没决定收费方式。".into()}).unwrap();
    let topic = id();
    store.create_conversation(&topic, "木桥项目讨论").unwrap();
    for (i, question) in [
        "木桥项目现在先做什么？",
        "和以前相比变了什么？",
        "那收费方式已经确定了吗？",
        "根据这些记录，你建议下一步验证什么？",
        "我曾经去过冰岛吗？",
    ]
    .iter()
    .enumerate()
    {
        let turn = store.start_turn(&id(), &topic, question, &[]).unwrap();
        let started = std::time::Instant::now();
        let result = store.answer_discussion(&config, &topic, &turn, &[]).await;
        if let Err(ref failure) = result {
            store.fail_turn(&turn.id, *failure).unwrap();
        }
        let message = store.turn(&turn.id).unwrap().assistant;
        let excerpts: Vec<_> = message
            .citations
            .iter()
            .map(|c| store.discussion_excerpt(&message.id, &c.source).unwrap())
            .collect();
        fs::write(output.join(format!("qa-{}.json",i+1)),serde_json::to_vec_pretty(&json!({"question":question,"result":result.map_err(|e|format!("{e:?}")),"message":message,"evidence":excerpts,"elapsed_ms":started.elapsed().as_millis()})).unwrap()).unwrap();
        assert_eq!(
            store
                .memory(first.memory_id.as_ref().unwrap())
                .unwrap()
                .current
                .id,
            current.after_version.as_ref().unwrap().as_str()
        );
        assert_eq!(
            store
                .library(&LibraryQuery {
                    limit: 50,
                    ..Default::default()
                })
                .unwrap()
                .items
                .len(),
            1
        );
        println!(
            "Q{} complete={}, no memory writes",
            i + 1,
            message.status == "complete"
        );
    }
    let target = Destination::Existing {
        memory_id: first.memory_id.unwrap(),
        expected_version: current.after_version.unwrap(),
    };
    let text = "下一步先验证桌面端首次保存是否容易找到，收费方式继续保持未定。";
    let merged = store
        .preview_conclusion_merge(&config, &target, text)
        .await
        .expect("merge preview");
    fs::write(
        output.join("merge-preview.json"),
        serde_json::to_vec_pretty(&json!({"confirmed_input":text,"preview":merged})).unwrap(),
    )
    .unwrap();
}
