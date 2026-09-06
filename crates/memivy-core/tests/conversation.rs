use memivy_core::{
    CaptureInput, DataPaths, Error, Store,
    conversation::{Answer, Evidence},
};
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn setup() -> (tempfile::TempDir, Store, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(DataPaths::new(dir.path().into()).unwrap()).unwrap();
    let topic = id();
    store.create_topic(&topic, "讨论原型").unwrap();
    let turn = id();
    store
        .begin_turn(&turn, &topic, "这只是尚未确认的假设")
        .unwrap();
    (dir, store, topic, turn)
}
fn answer() -> Answer {
    Answer {
        recollection: String::new(),
        ideas: "新建议，尚未确认".into(),
        sources: vec![],
        conclusion: "先体验再配置".into(),
    }
}
#[test]
fn conversation_drafts_and_answers_never_enter_memory_search() {
    let (_dir, store, topic, turn) = setup();
    store.save_draft(&topic, "私有草稿不进入记忆").unwrap();
    store.finish_turn(&turn, &answer(), &[]).unwrap();
    assert!(store.search("", 50).unwrap().items.is_empty());
    let reopen = Store::open(store.paths.clone()).unwrap();
    let thread = reopen.thread(&topic).unwrap();
    assert_eq!(thread.topic.draft, "私有草稿不进入记忆");
    assert_eq!(
        thread.turns[0].answer.as_ref().unwrap().ideas,
        "新建议，尚未确认"
    );
}
#[test]
fn confirmation_is_exact_atomic_idempotent_and_undo_keeps_raw_provenance() {
    let (_dir, store, topic, turn) = setup();
    assert!(
        store
            .save_conclusion(&id(), &turn, "目标", "未完成不能存")
            .is_err()
    );
    store.finish_turn(&turn, &answer(), &[]).unwrap();
    let request = id();
    let text = "  用户改过的结论\n逐字保留。🙂";
    let receipt = store
        .save_conclusion(&request, &turn, "新的记忆", text)
        .unwrap();
    assert_eq!(
        receipt.capture_id,
        store
            .save_conclusion(&request, &turn, "新的记忆", text)
            .unwrap()
            .capture_id
    );
    assert!(matches!(
        store.save_conclusion(&request, &turn, "新的记忆", "不同文本"),
        Err(Error::RequestConflict)
    ));
    let c = store.capture_by_id(&receipt.capture_id).unwrap();
    assert_eq!(c.text, text);
    assert!(c.session_uri.unwrap().contains(&turn));
    assert_eq!(store.search("", 50).unwrap().items.len(), 1);
    store.undo_conclusion(&receipt.id).unwrap();
    store.undo_conclusion(&receipt.id).unwrap();
    assert!(store.search("用户改过", 50).unwrap().items.is_empty());
    assert!(store.capture_by_id(&receipt.capture_id).is_err());
    let db = rusqlite::Connection::open(store.paths.database()).unwrap();
    let raw: String = db
        .query_row(
            "SELECT text FROM captures WHERE id=?1",
            [receipt.capture_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(raw, text);
    assert!(store.thread(&topic).unwrap().receipts[0].undone);
}
#[test]
fn cancelled_and_interrupted_turns_reject_late_answers() {
    let (_dir, store, topic, turn) = setup();
    store.stop_turn(&turn, true, "已停止").unwrap();
    assert!(!store.finish_turn(&turn, &answer(), &[]).unwrap());
    assert_eq!(store.turn(&turn).unwrap().status, "cancelled");
    let next = id();
    store.begin_turn(&next, &topic, "新的问题").unwrap();
    store.recover_interrupted().unwrap();
    assert!(!store.finish_turn(&next, &answer(), &[]).unwrap());
    assert_eq!(store.turn(&next).unwrap().status, "interrupted");
    assert_eq!(store.diagnostics().unwrap().count, 0);
}
#[test]
fn unknown_citations_and_unsupported_recollection_fail_closed() {
    let (_dir, store, _topic, turn) = setup();
    let mut a = answer();
    a.recollection = "用户之前说过".into();
    assert!(store.finish_turn(&turn, &a, &[]).is_err());
    a.sources = vec![id()];
    assert!(store.finish_turn(&turn, &a, &[]).is_err());
    let c = store
        .capture(CaptureInput {
            request_id: id(),
            text: "原话作为依据".into(),
            source_app: "测试".into(),
            project: None,
            session_uri: None,
        })
        .unwrap();
    let evidence: Evidence = c.into();
    a.sources = vec![evidence.id.clone()];
    assert!(store.finish_turn(&turn, &a, &[evidence]).unwrap());
}
#[test]
fn duplicate_questions_cannot_spawn_a_second_running_attempt() {
    let (_dir, store, topic, turn) = setup();
    assert!(
        store
            .begin_turn(&turn, &topic, "这只是尚未确认的假设")
            .is_err()
    );
    assert!(store.begin_turn(&id(), &topic, "另一个问题").is_err());
    assert_eq!(store.thread(&topic).unwrap().turns.len(), 1);
}

#[test]
fn upgrading_the_accepted_prototype_preserves_original_captures() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::new(dir.path().into()).unwrap();
    let db = rusqlite::Connection::open(paths.database()).unwrap();
    db.execute_batch(include_str!("../../../migrations/001_phase1.sql"))
        .unwrap();
    db.execute("INSERT INTO captures(id,request_id,text,source_app,created_at) VALUES(?1,?2,'旧样机原话，必须保留','旧样机',1)",[id(),id()]).unwrap();
    drop(db);
    let store = Store::open(paths).unwrap();
    assert_eq!(store.diagnostics().unwrap().schema_version, 2);
    assert_eq!(
        store.search("必须保留", 10).unwrap().items[0].text,
        "旧样机原话，必须保留"
    );
    assert!(store.topics().unwrap().is_empty());
}
