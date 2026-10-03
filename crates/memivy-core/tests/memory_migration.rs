use memivy_core::memory::*;
use rusqlite::{Connection, params};
use serde_json::json;

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[test]
fn schema_one_upgrade_preserves_originals_receipts_and_both_undo_paths() {
    for grouped in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memivy.db");
        let db = Connection::open(&path).unwrap();
        db.pragma_update(None, "foreign_keys", true).unwrap();
        db.execute_batch(include_str!("../../../migrations/memory/001_initial.sql"))
            .unwrap();
        db.pragma_update(None, "application_id", 0x4d454d59_i64)
            .unwrap();
        db.pragma_update(None, "user_version", 1).unwrap();
        let memory = id();
        let capture = id();
        let topic = id();
        let versions = [id(), id(), id(), id()];
        let receipts = [id(), id(), id(), id()];
        let old_input = id();
        let input = id();
        let original = "  Original words: 保留原话与否定，不是已完成。\n";
        let origin = Origin::User {
            app: "Schema one app".into(),
            project: Some("old-project".into()),
            uri: None,
        };
        db.execute("INSERT INTO captures(id,request_id,fingerprint,text,source,created_at) VALUES(?1,?2,X'01',?3,?4,10)", params![capture,id(),original,serde_json::to_string(&origin).unwrap()]).unwrap();
        db.execute(
            "INSERT INTO capture_state(capture_id,understanding) VALUES(?,'attached')",
            [&capture],
        )
        .unwrap();
        db.execute(
            "INSERT INTO memories(id,created_at,updated_at) VALUES(?,10,40)",
            [&memory],
        )
        .unwrap();
        for (index, (body, reason, actor)) in [
            (original, "create", "user"),
            ("Earlier AI wording", "edit", "ai"),
            (original, "undo", "user"),
            ("Current AI wording", "edit", "ai"),
        ]
        .into_iter()
        .enumerate()
        {
            db.execute("INSERT INTO memory_versions(id,memory_id,parent_id,title,body,actor,reason,created_at) VALUES(?1,?2,?3,'Saved note',?4,?5,?6,?7)", params![versions[index],memory,index.checked_sub(1).map(|i| &versions[i]),body,actor,reason,(index as i64 + 1)*10]).unwrap();
            db.execute(
                "INSERT INTO version_captures VALUES(?1,?2)",
                params![versions[index], capture],
            )
            .unwrap();
        }
        db.execute(
            "UPDATE memories SET current_version_id=?2 WHERE id=?1",
            params![memory, versions[3]],
        )
        .unwrap();
        db.execute(
            "INSERT INTO collections(id,name,created_at) VALUES(?,'Old topic',10)",
            [&topic],
        )
        .unwrap();
        db.execute(
            "INSERT INTO collection_entries VALUES(?1,'memory',?2)",
            params![topic, memory],
        )
        .unwrap();
        db.execute("INSERT INTO record_pins VALUES('memory',?,50)", [&memory])
            .unwrap();
        let membership = json!([topic]).to_string();
        for (index, (action, status, logical_input)) in [
            ("capture", "applied", None),
            ("edit", "undone", Some(&old_input)),
            ("undo", "applied", None),
            ("edit", "applied", Some(&input)),
        ]
        .into_iter()
        .enumerate()
        {
            db.execute("INSERT INTO receipts(request_id,fingerprint,action,capture_id,memory_id,before_version,after_version,status,created_at,logical_input_id,before_memberships,after_memberships) VALUES(?1,X'01',?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)", params![receipts[index],action,capture,memory,index.checked_sub(1).map(|i| &versions[i]),versions[index],status,(index as i64+1)*10,logical_input,logical_input.map(|_| &membership)]).unwrap();
            db.execute(
                "INSERT INTO receipt_changes VALUES(?1,?2,?3,?4,?5,'active')",
                params![
                    receipts[index],
                    memory,
                    index.checked_sub(1).map(|i| &versions[i]),
                    versions[index],
                    if index == 0 { "undone" } else { "active" }
                ],
            )
            .unwrap();
        }
        // Preserve a real unsent draft unrelated to the note being undone.
        let draft = WorkspaceDraft {
            key: "input".into(),
            title: "Unsent".into(),
            body: "Keep my draft".into(),
            request_id: id(),
            expected_version: None,
            destination: None,
            origin: None,
            context: vec![],
        };
        let draft_json = serde_json::to_string(&draft).unwrap();
        db.execute(
            "INSERT INTO workspace_drafts VALUES(?1,?2)",
            params![draft.key, draft_json],
        )
        .unwrap();
        drop(db);

        let store = MemoryStore::open_application(dir.path()).unwrap();
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(store.capture_by_id(&capture).unwrap().text, original);
        assert_eq!(store.capture_by_id(&capture).unwrap().origin, origin);
        assert_eq!(
            store
                .history(&memory)
                .unwrap()
                .iter()
                .map(|v| v.id.clone())
                .collect::<Vec<_>>(),
            versions
        );
        assert_eq!(store.receipt(&receipts[1]).unwrap().status, "undone");
        assert_eq!(store.receipt(&receipts[2]).unwrap().action, "undo");
        for receipt in &receipts {
            assert!(
                store
                    .receipt(receipt)
                    .unwrap()
                    .collection_changes
                    .is_empty()
            );
        }
        assert_eq!(
            store.receipt_changes(&receipts[2]).unwrap()[0].after_version,
            versions[2]
        );
        let snapshot = store.receipt_changes(&receipts[3]).unwrap().remove(0);
        assert_eq!(
            snapshot.navigation_before.as_ref().unwrap().collections,
            vec![topic.clone()]
        );
        assert_eq!(snapshot.navigation_before.as_ref().unwrap().pinned, None);
        assert_eq!(snapshot.navigation_before, snapshot.navigation_after);
        assert_eq!(
            db.query_row(
                "SELECT payload FROM workspace_drafts WHERE key=?",
                [&draft.key],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            draft_json
        );
        assert_eq!(
            store.workspace_draft(&draft.key).unwrap().unwrap().body,
            draft.body
        );
        let groups = store.memory_agent_changes(&memory).unwrap();
        assert_eq!(
            groups
                .iter()
                .find(|group| group.input_id == old_input)
                .unwrap()
                .receipts[0]
                .status,
            "undone"
        );
        assert_eq!(
            store
                .resolve_source(&SourceRef::Version(versions[2].clone()), 3000)
                .unwrap()
                .text,
            original
        );

        let key = RecordKey {
            kind: "memory".into(),
            id: memory.clone(),
        };
        let later_topic = id();
        store
            .save_collection(&later_topic, "Later topic", "", None)
            .unwrap();
        let revision = store.read_agent_collection(&later_topic).unwrap().revision;
        store.collect_record(&later_topic, &key, true).unwrap();
        assert_eq!(
            store.read_agent_collection(&later_topic).unwrap().revision,
            revision + 1
        );
        if grouped {
            assert_eq!(
                store.undo_agent_input(&id(), &input).unwrap().conflicts,
                vec![memory.clone()]
            );
        } else {
            assert_eq!(store.undo(&id(), &receipts[3]), Err(DataError::Conflict));
        }
        assert_eq!(store.memory(&memory).unwrap().current.id, versions[3]);
        store.collect_record(&later_topic, &key, false).unwrap();
        if grouped {
            assert!(
                store
                    .undo_agent_input(&id(), &input)
                    .unwrap()
                    .conflicts
                    .is_empty()
            );
        } else {
            store.undo(&id(), &receipts[3]).unwrap();
        }
        assert_eq!(store.memory(&memory).unwrap().current.body, original);
        let navigation = store.record_navigation(&key).unwrap();
        assert_eq!(navigation.collections, vec![topic]);
        assert!(
            navigation.pinned,
            "Legacy receipt snapshots must preserve pins they did not record"
        );
        assert_eq!(store.receipt(&receipts[3]).unwrap().status, "undone");
        let backups = std::fs::read_dir(dir.path().join("recovery"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        assert!(
            backups[0]
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("before-migration-1-to-2-")
        );
        let backup = Connection::open(&backups[0]).unwrap();
        assert_eq!(
            backup
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            backup
                .query_row(
                    "SELECT after_memberships FROM receipts WHERE request_id=?",
                    [&receipts[3]],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            membership
        );
        assert_eq!(
            backup
                .query_row("SELECT text FROM captures WHERE id=?", [&capture], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
            original
        );
        store.check_integrity().unwrap();
    }

    // Existing schema-one Trash cannot distinguish missed historical originals
    // from explicit user restores. Only originals with trash ownership are erased.
    let dir = tempfile::tempdir().unwrap();
    let db = Connection::open(dir.path().join("memivy.db")).unwrap();
    db.pragma_update(None, "foreign_keys", true).unwrap();
    db.execute_batch(include_str!("../../../migrations/memory/001_initial.sql"))
        .unwrap();
    db.pragma_update(None, "application_id", 0x4d454d59_i64)
        .unwrap();
    db.pragma_update(None, "user_version", 1).unwrap();
    let memory = id();
    let original = id();
    let current = id();
    let versions = [id(), id()];
    let origin = Origin::User {
        app: "Schema one app".into(),
        project: None,
        uri: None,
    };
    db.execute(
        "INSERT INTO memories(id,state,created_at,updated_at) VALUES(?,'trashed',1,2)",
        [&memory],
    )
    .unwrap();
    for (index, (capture, text)) in [
        (&original, "Historical original with no deletion ownership"),
        (&current, "Current original owned by the trashed memory"),
    ]
    .into_iter()
    .enumerate()
    {
        db.execute(
            "INSERT INTO captures VALUES(?1,?2,X'01',?3,?4,1)",
            params![capture, id(), text, serde_json::to_string(&origin).unwrap()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO capture_state VALUES(?1,?2,'attached',?3)",
            params![
                capture,
                if index == 0 { "active" } else { "trashed" },
                if index == 0 { None } else { Some(&memory) }
            ],
        )
        .unwrap();
        db.execute("INSERT INTO memory_versions(id,memory_id,parent_id,title,body,actor,reason,created_at) VALUES(?1,?2,?3,'Old note',?4,'user',?5,?6)", params![versions[index],memory,index.checked_sub(1).map(|i| &versions[i]),text,if index==0 {"create"} else {"edit"},index as i64+1]).unwrap();
        db.execute(
            "INSERT INTO version_captures VALUES(?1,?2)",
            params![versions[index], capture],
        )
        .unwrap();
    }
    db.execute(
        "UPDATE memories SET current_version_id=?2 WHERE id=?1",
        params![memory, versions[1]],
    )
    .unwrap();
    drop(db);

    let store = MemoryStore::open_application(dir.path()).unwrap();
    assert_eq!(store.capture_by_id(&original).unwrap().origin, origin);
    store.purge_memory(&memory).unwrap();
    let retained = store.capture_by_id(&original).unwrap();
    assert_eq!(
        retained.text,
        "Historical original with no deletion ownership"
    );
    assert_eq!(retained.origin, origin);
    assert!(store.capture_by_id(&current).is_err());
    assert!(matches!(store.memory(&memory), Err(DataError::Unavailable)));
    store.check_integrity().unwrap();
}
