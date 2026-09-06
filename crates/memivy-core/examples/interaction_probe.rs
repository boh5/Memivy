//! Real model smoke test with synthetic text and an ephemeral isolated database.
use memivy_core::{
    CaptureInput, DataPaths, Store, conversation::answer_question, model::ModelConfig,
};
use uuid::Uuid;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("pass a model config outside the repository")?;
    let config = ModelConfig::read(std::path::Path::new(&path))?;
    let dir = tempfile::tempdir()?;
    let store = Store::open(DataPaths::new(dir.path().into())?)?;
    let capture=store.capture(CaptureInput{request_id:Uuid::new_v4().to_string(),text:"体验样例：我决定先给新用户看示例结果，再让他们配置模型，因为第一次打开就填 API Key 容易让人放弃。".into(),source_app:"合成测试".into(),project:None,session_uri:None})?;
    let topic = Uuid::new_v4().to_string();
    store.create_topic(&topic, "首次体验原则")?;
    for question in [
        "我之前为什么决定先给新用户看示例结果？",
        "那模型配置这一步应该放在哪里？给我一个具体建议。",
    ] {
        let id = Uuid::new_v4().to_string();
        let turn = store.begin_turn(&id, &topic, question)?;
        let selected = if question.starts_with("我之前") {
            Vec::new()
        } else {
            vec![capture.id.clone()]
        };
        let (answer, evidence) = answer_question(&store, &config, &turn, &selected)
            .await
            .map_err(std::io::Error::other)?;
        store.finish_turn(&id, &answer, &evidence)?;
        assert!(
            !evidence.is_empty(),
            "the first question must retrieve its source without pinning"
        );
        println!(
            "{}",
            serde_json::json!({"question":question,"answer":answer,"evidence_count":evidence.len()})
        );
        assert_eq!(store.search("", 50)?.items.len(), 1);
        if question.starts_with("那模型") {
            let receipt = store.save_conclusion(
                &Uuid::new_v4().to_string(),
                &id,
                "首次体验原则",
                "体验样例：先解释价值，首次使用真实问答时再配置模型。",
            )?;
            assert_eq!(store.search("", 50)?.items.len(), 2);
            store.undo_conclusion(&receipt.id)?;
            assert_eq!(store.search("", 50)?.items.len(), 1);
        }
    }
    println!("Real model capture / ask / follow-up / confirm / undo passed; synthetic data only.");
    Ok(())
}
