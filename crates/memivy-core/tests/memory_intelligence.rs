use memivy_core::memory::*;
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn capture(store: &MemoryStore, text: &str) -> CaptureResult {
    store
        .capture(&CaptureRequest {
            request_id: id(),
            text: text.into(),
            origin: Origin::User {
                app: "Synthetic QA".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap()
}
fn keep(s: &MemoryStore, request: &str, saved: &CaptureResult) -> Result<Receipt> {
    s.edit_memory(&EditRequest {
        request_id: request.into(),
        memory_id: saved.memory_id.clone(),
        expected_version: saved.version_id.clone(),
        title: "测试记忆".into(),
        body: s.capture_by_id(&saved.capture_id)?.text,
    })
}
fn seeded(store: &MemoryStore, text: &str) -> Receipt {
    let raw = capture(store, text);
    keep(store, &id(), &raw).unwrap()
}
#[test]
fn explicit_selected_text_appends_exactly_and_remains_independently_undoable() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let old = seeded(&store, "旧正文绝不能被一句结论替换。");
    let topic = id();
    store.create_conversation(&topic, "合成讨论").unwrap();
    let run = store
        .begin_agent_input(&id(), &id(), &topic, "Continue discussion", &[], None)
        .unwrap();
    store
        .append_agent_text(&run.input_id, &run.attempt_id, "尚未保存的建议")
        .unwrap();
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
    let request = id();
    let destination = Destination::Existing {
        memory_id: old.memory_id.clone().unwrap(),
        expected_version: old.after_version.clone().unwrap(),
    };
    let text = "  用户修改后选择的文字\n  ";
    let saved = store
        .save_agent_text(&request, &run.input_id, text, "确认标题", &destination)
        .unwrap();
    assert_eq!(
        store
            .save_agent_text(&request, &run.input_id, text, "确认标题", &destination)
            .unwrap(),
        saved
    );
    assert_eq!(
        store
            .memory(saved.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        format!("旧正文绝不能被一句结论替换。\n\n{text}")
    );
    let raw = store
        .capture_by_id(saved.capture_id.as_ref().unwrap())
        .unwrap();
    assert_eq!(raw.text, text);
    assert_eq!(
        store
            .save_agent_text(
                &request,
                &run.input_id,
                "迟到的另一段文字",
                "确认标题",
                &destination
            )
            .unwrap_err(),
        DataError::RequestConflict
    );
    store.undo_agent_input(&id(), &request).unwrap();
    assert_eq!(
        store
            .memory(saved.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        "旧正文绝不能被一句结论替换。"
    );
    assert_eq!(store.capture_by_id(&raw.id).unwrap().text, text);
}
