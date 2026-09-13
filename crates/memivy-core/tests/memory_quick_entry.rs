use memivy_core::memory::*;
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn draft(key: &str, text: &str) -> WorkspaceDraft {
    WorkspaceDraft {
        destination: None,
        key: key.into(),
        request_id: id(),
        title: String::new(),
        body: text.into(),
        expected_version: None,
        context: vec![],
        origin: None,
    }
}
#[test]
fn concurrent_drafts_and_late_consumption_are_guarded_across_store_handles() {
    let dir = tempfile::tempdir().unwrap();
    let a = MemoryStore::open(dir.path()).unwrap();
    let b = MemoryStore::open(dir.path()).unwrap();
    let first = draft("quick_input", " \n来自快捷入口的原话\n ");
    assert!(a.compare_workspace_draft(&first, None).unwrap());
    // Idempotent after a lost acknowledgement.
    assert!(a.compare_workspace_draft(&first, None).unwrap());
    let mut next = draft("quick_input", "另一窗口编辑的内容");
    next.origin = Some(Origin::User {
        app: "Safari".into(),
        project: None,
        uri: Some("https://example.com/source".into()),
    });
    assert!(!b.compare_workspace_draft(&next, None).unwrap());
    assert!(
        b.compare_workspace_draft(&next, Some(&first.request_id))
            .unwrap()
    );
    assert!(
        !a.consume_workspace_draft("quick_input", &first.request_id)
            .unwrap()
    );
    assert_eq!(
        a.workspace_draft("quick_input").unwrap().unwrap().body,
        next.body
    );
    drop(a);
    drop(b);
    let restarted = MemoryStore::open(dir.path()).unwrap();
    let restored = restarted.workspace_draft("quick_input").unwrap().unwrap();
    assert!(matches!(
        restored.origin,
        Some(Origin::User { uri: Some(_), .. })
    ));
    assert!(
        restarted
            .library(&LibraryQuery::default())
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        restarted
            .consume_workspace_draft("quick_input", &next.request_id)
            .unwrap()
    );
}
#[test]
fn main_and_quick_drafts_stay_independent_and_retry_keeps_raw_origin() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let mut quick = draft("quick_input", " \n第一行\n第二行  ");
    quick.origin = Some(Origin::User {
        app: "Notes".into(),
        project: None,
        uri: None,
    });
    let main = draft("input", "主窗口尚未发送的另一个想法");
    for draft in [&quick, &main] {
        assert!(s.compare_workspace_draft(draft, None).unwrap());
    }
    let conversation = id();
    s.create_conversation(&conversation, "快捷讨论").unwrap();
    let execution = s
        .begin_agent_input(
            &quick.request_id,
            &id(),
            &conversation,
            &quick.body,
            &[],
            quick.origin.as_ref(),
        )
        .unwrap();
    s.stop_agent_input(
        &execution.input_id,
        &execution.attempt_id,
        "failed",
        Some("network"),
    )
    .unwrap();
    let retried = s.retry_agent_input(&execution.input_id, &id()).unwrap();
    assert_eq!(retried.input_text, quick.body);
    assert_eq!(s.messages(&conversation, 0, 20).unwrap().len(), 2);
    assert!(
        s.library(&LibraryQuery::default())
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        s.consume_workspace_draft("quick_input", &quick.request_id)
            .unwrap()
    );
    assert_eq!(s.workspace_draft("input").unwrap().unwrap().body, main.body);
}
