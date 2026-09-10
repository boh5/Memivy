use memivy_core::memory::*;
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn draft(key: &str, text: &str) -> WorkspaceDraft {
    WorkspaceDraft {
        conclusion: None,
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
    let first = draft("quick_capture", " \n来自快捷入口的原话\n ");
    assert!(a.compare_workspace_draft(&first, None).unwrap());
    // Idempotent after a lost acknowledgement.
    assert!(a.compare_workspace_draft(&first, None).unwrap());
    let mut next = draft("quick_capture", "另一窗口编辑的内容");
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
        !a.consume_workspace_draft("quick_capture", &first.request_id)
            .unwrap()
    );
    assert_eq!(
        a.workspace_draft("quick_capture").unwrap().unwrap().body,
        next.body
    );
    drop(a);
    drop(b);
    let restarted = MemoryStore::open(dir.path()).unwrap();
    let restored = restarted.workspace_draft("quick_capture").unwrap().unwrap();
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
            .consume_workspace_draft("quick_capture", &next.request_id)
            .unwrap()
    );
}
#[test]
fn capture_and_question_stay_separate_and_retries_preserve_exact_input_and_origin() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let mut capture = draft("quick_capture", " \n第一行\n第二行  ");
    capture.origin = Some(Origin::User {
        app: "Notes".into(),
        project: None,
        uri: None,
    });
    let question = draft("quick_question", "先问一下，不能自动记住这句话");
    for d in [&capture, &question] {
        assert!(s.compare_workspace_draft(d, None).unwrap());
    }
    let request = CaptureRequest {
        request_id: capture.request_id.clone(),
        text: capture.body.clone(),
        origin: capture.origin.unwrap(),
    };
    let saved = s.capture(&request).unwrap();
    for _ in 0..10 {
        assert_eq!(s.capture(&request).unwrap().memory_id, saved.memory_id);
    }
    assert_eq!(
        s.memory(&saved.memory_id).unwrap().current.body,
        capture.body
    );
    assert!(matches!(
        s.capture_by_id(&saved.capture_id).unwrap().origin,
        Origin::User { uri: None, .. }
    ));
    assert_eq!(s.library(&LibraryQuery::default()).unwrap().items.len(), 1);
    assert!(
        s.consume_workspace_draft("quick_capture", &capture.request_id)
            .unwrap()
    );
    assert_eq!(
        s.workspace_draft("quick_question").unwrap().unwrap().body,
        question.body
    );
}
