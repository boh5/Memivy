use memivy_core::memory::*;
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn archive(db: &Connection, text: &str, origin: Origin) -> (String, CaptureRequest) {
    let capture = id();
    let request = CaptureRequest {
        request_id: id(),
        text: text.into(),
        origin,
    };
    let fingerprint =
        Sha256::digest(serde_json::to_vec(&("capture", &request.text, &request.origin)).unwrap());
    db.execute("INSERT INTO captures(id,request_id,fingerprint,text,source,created_at) VALUES(?,?,?,?,?,1)", params![capture,request.request_id,fingerprint.as_slice(),request.text,serde_json::to_string(&request.origin).unwrap()]).unwrap();
    db.execute(
        "INSERT INTO capture_state(capture_id) VALUES(?)",
        [&capture],
    )
    .unwrap();
    (capture, request)
}
#[test]
fn schema8_promotes_only_independent_inputs_and_preserves_archive_and_review_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let db = Connection::open(dir.path().join("memivy.db")).unwrap();
    for sql in [
        include_str!("../../../migrations/memory/001_records.sql"),
        include_str!("../../../migrations/memory/002_conversations.sql"),
        include_str!("../../../migrations/memory/003_workspace.sql"),
        include_str!("../../../migrations/memory/004_intelligence.sql"),
        include_str!("../../../migrations/memory/005_reviewed_conclusions.sql"),
        include_str!("../../../migrations/memory/006_navigation.sql"),
        include_str!("../../../migrations/memory/007_collection_feedback.sql"),
        include_str!("../../../migrations/memory/008_cleanup.sql"),
    ] {
        db.execute_batch(sql).unwrap();
    }
    db.pragma_update(None, "application_id", 0x4d495659_i64)
        .unwrap();
    let origin = Origin::User {
        app: "旧版本".into(),
        project: None,
        uri: None,
    };
    let (pending, request) = archive(&db, "待整理独立输入", origin.clone());
    let legacy_draft = WorkspaceDraft {
        key: format!("capture:{pending}"),
        request_id: id(),
        title: "未保存标题".into(),
        body: "未保存的手工内容".into(),
        expected_version: None,
        origin: None,
        context: vec![],
        conclusion: None,
    };
    db.execute(
        "INSERT INTO workspace_drafts(key,payload) VALUES(?,?)",
        params![
            legacy_draft.key,
            serde_json::to_string(&legacy_draft).unwrap()
        ],
    )
    .unwrap();
    let (old, _) = archive(&db, "原始报价 100 EUR", origin);
    let memory = id();
    let version = id();
    db.execute(
        "INSERT INTO memories(id,created_at,updated_at) VALUES(?,1,1)",
        [&memory],
    )
    .unwrap();
    db.execute("INSERT INTO memory_versions(id,memory_id,title,body,actor,reason,created_at) VALUES(?,?,'报价','当前报价 200 EUR','user','create',1)", params![version,memory]).unwrap();
    db.execute(
        "INSERT INTO version_captures(version_id,capture_id) VALUES(?,?)",
        params![version, old],
    )
    .unwrap();
    db.execute(
        "UPDATE memories SET current_version_id=? WHERE id=?",
        params![version, memory],
    )
    .unwrap();
    db.execute(
        "UPDATE capture_state SET understanding='attached' WHERE capture_id=?",
        [&old],
    )
    .unwrap();
    for capture in [&pending, &old] {
        db.execute(
            "INSERT INTO record_pins(kind,record_id,created_at) VALUES('capture',?,1)",
            [capture],
        )
        .unwrap();
    }
    let collection = id();
    db.execute(
        "INSERT INTO collections(id,name,created_at) VALUES(?,'旧专题',1)",
        [&collection],
    )
    .unwrap();
    for capture in [&pending, &old] {
        db.execute(
            "INSERT INTO collection_entries(collection_id,kind,record_id) VALUES(?,'capture',?)",
            params![collection, capture],
        )
        .unwrap();
    }
    let topic = id();
    let turn = id();
    let message = id();
    db.execute(
        "INSERT INTO conversations(id,title,created_at,updated_at) VALUES(?,'讨论',1,1)",
        [&topic],
    )
    .unwrap();
    db.execute(
        "INSERT INTO turns(id,conversation_id,fingerprint) VALUES(?,?,X'00')",
        params![turn, topic],
    )
    .unwrap();
    db.execute("INSERT INTO messages(id,turn_id,conversation_id,role,text,status,created_at) VALUES(?,?,?,'assistant','回答','complete',1)",params![message,turn,topic]).unwrap();
    let (failed, _) = archive(
        &db,
        "旧版保存失败的结论",
        Origin::Conversation {
            conversation_id: topic,
            message_id: message.clone(),
            message_role: "assistant".into(),
            confirmed_by: "user".into(),
        },
    );
    let destination = Destination::Existing {
        memory_id: memory.clone(),
        expected_version: id(),
    };
    db.execute("INSERT INTO conclusion_intents(capture_id,title,destination,merged_body) VALUES(?,'审核标题',?,'完整审核融合稿')",params![failed,serde_json::to_string(&destination).unwrap()]).unwrap();
    drop(db);
    let store = MemoryStore::open(dir.path()).unwrap();
    assert_eq!(
        store.library(&LibraryQuery::default()).unwrap().items.len(),
        2
    );
    let saved = store.capture(&request).unwrap();
    assert_eq!(saved.capture_id, pending);
    let carried = store
        .workspace_draft(&format!("memory:{}", saved.memory_id))
        .unwrap()
        .unwrap();
    assert_eq!(carried.body, legacy_draft.body);
    assert_eq!(carried.request_id, legacy_draft.request_id);
    assert_eq!(
        carried.expected_version.as_deref(),
        Some(saved.version_id.as_str())
    );
    assert!(store.workspace_draft(&legacy_draft.key).unwrap().is_none());
    assert_eq!(store.capture(&request).unwrap().memory_id, saved.memory_id);
    assert_eq!(store.collections().unwrap()[0].count, 1);
    assert_eq!(
        store
            .library(&LibraryQuery {
                pinned: true,
                ..Default::default()
            })
            .unwrap()
            .items[0]
            .key
            .id,
        saved.memory_id
    );
    assert!(
        store
            .search(&SearchRequest::text("100", 8))
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        store
            .search(&SearchRequest::text("保存失败", 8))
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(
        store.search(&SearchRequest::text("200", 8)).unwrap().items[0].memory_id,
        memory
    );
    let draft = store
        .workspace_draft(&format!("conclusion:{message}"))
        .unwrap()
        .unwrap();
    assert_eq!(draft.body, "旧版保存失败的结论");
    let review = draft.conclusion.unwrap();
    assert_eq!(review.destination, destination);
    assert_eq!(review.merged_body.as_deref(), Some("完整审核融合稿"));
    assert!(store.claim_organization().unwrap().is_none());
    assert_eq!(store.capture_by_id(&old).unwrap().text, "原始报价 100 EUR");
    assert!(
        std::fs::read_dir(dir.path().join("backups"))
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("before-schema-9-"))
    );
    store.check_integrity().unwrap();
}
