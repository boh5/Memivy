use memivy_core::memory::*;
use rusqlite::{Connection, params};
use serde_json::json;
use uuid::Uuid;

fn id() -> String {
    Uuid::new_v4().to_string()
}
fn capture(store: &MemoryStore, text: &str) -> CaptureResult {
    let request = CaptureRequest {
        request_id: id(),
        text: text.into(),
        origin: Origin::User {
            app: "Synthetic activity".into(),
            project: None,
            uri: None,
        },
    };
    let saved = store.capture(&request).unwrap();
    assert_eq!(
        saved.capture_id,
        store.capture(&request).unwrap().capture_id
    );
    saved
}

#[test]
fn recording_counts_ignore_retries_edits_and_respect_trash_restore() {
    let root = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(root.path()).unwrap();
    let saved = capture(&store, "One original input");
    let edit = store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: saved.memory_id.clone(),
            expected_version: saved.version_id,
            title: "Edited title".into(),
            body: "Edited body".into(),
        })
        .unwrap();
    let summary = store.activity_summary().unwrap();
    assert_eq!(summary.memory_count, 1);
    assert_eq!(summary.days.iter().map(|day| day.count).sum::<i64>(), 1);
    store
        .trash_memory(&saved.memory_id, edit.after_version.as_ref().unwrap())
        .unwrap();
    assert!(store.activity_summary().unwrap().days.is_empty());
    store.restore_memory(&saved.memory_id).unwrap();
    assert_eq!(store.activity_summary().unwrap().days[0].count, 1);
    let reopened = MemoryStore::open(root.path()).unwrap();
    assert_eq!(
        reopened.activity_summary().unwrap().days[0].date,
        summary.days[0].date
    );
}

#[test]
fn one_discussion_input_split_into_two_memories_is_one_historical_recording() {
    let root = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(root.path()).unwrap();
    let conversation = id();
    store
        .create_conversation(&conversation, "Synthetic discussion")
        .unwrap();
    let run = store
        .begin_agent_input(
            &id(),
            &id(),
            &conversation,
            "I decided to save two ideas.",
            &[],
            None,
        )
        .unwrap();
    assert!(
        store.activity_summary().unwrap().days.is_empty(),
        "Unsaved conversation is not activity"
    );
    let db = Connection::open(store.database_path()).unwrap();
    let timestamp: i64 = db
        .query_row(
            "SELECT unixepoch('2024-02-29 12:00:00','utc')*1000",
            [],
            |row| row.get(0),
        )
        .unwrap();
    db.execute(
        "UPDATE messages SET created_at=? WHERE id=?",
        params![timestamp, run.user_message_id],
    )
    .unwrap();
    for title in ["First idea", "Second idea"] {
        let write = MemoryWriteArgs {
            destination: Destination::New,
            title: title.into(),
            parts: vec![MemoryWritePart {
                text: title.into(),
                sources: vec![MemorySourceQuote {
                    source_id: run.user_message_id.clone(),
                    quote: run.input_text.clone(),
                }],
            }],
        };
        let args = serde_json::to_value(&write).unwrap();
        let call = id();
        let mut protocol = store.agent_execution(&run.input_id).unwrap().protocol;
        protocol.push(json!({"role":"assistant","content":null,"tool_calls":[{"id":call,"type":"function","function":{"name":"write_memory","arguments":args.to_string()}}]}));
        store
            .checkpoint_agent(&run.input_id, &run.attempt_id, &protocol)
            .unwrap();
        let op = store
            .stage_agent_operation(&run.input_id, &run.attempt_id, &call, "write_memory", &args)
            .unwrap();
        store
            .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &write)
            .unwrap();
        store
            .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &write)
            .unwrap();
    }
    let summary = store.activity_summary().unwrap();
    assert_eq!(summary.memory_count, 2);
    assert_eq!(summary.days.len(), 1);
    assert_eq!(summary.days[0].date, "2024-02-29");
    assert_eq!(summary.days[0].count, 1);
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
    store.delete_conversation(&conversation).unwrap();
    assert_eq!(
        store.activity_summary().unwrap().days[0].count,
        1,
        "Durable provenance outlives the discussion"
    );
    let records = store.activity_records(timestamp, timestamp + 1, 0).unwrap();
    assert_eq!(records.items.len(), 1);
    assert_eq!(records.items[0].row.key.kind, "memory");
    assert_eq!(records.items[0].row.updated_at, timestamp);
}

#[test]
fn day_records_are_bounded_stable_and_use_half_open_boundaries() {
    let root = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(root.path()).unwrap();
    let db = Connection::open(store.database_path()).unwrap();
    let start = 1_700_000_000_000_i64;
    for n in 0..42 {
        let capture_id = id();
        db.execute(
            "INSERT INTO captures(id,request_id,fingerprint,text,source,created_at) VALUES (?1,?1,x'00',?2,?3,?4)",
            params![capture_id, format!("Synthetic input {n}: {}", "x".repeat(500)), r#"{"kind":"user","app":"Synthetic activity"}"#, start+n],
        ).unwrap();
        db.execute(
            "INSERT INTO capture_state(capture_id) VALUES (?)",
            [&capture_id],
        )
        .unwrap();
    }
    let first = store.activity_records(start, start + 41, 0).unwrap();
    let second = store
        .activity_records(start, start + 41, first.next_offset.unwrap())
        .unwrap();
    assert_eq!(first.items.len(), 40);
    assert_eq!(second.items.len(), 1);
    assert!(second.next_offset.is_none());
    assert_eq!(second.items[0].row.updated_at, start);
    assert_eq!(first.items[0].row.updated_at, start + 40);
    assert!(
        first
            .items
            .iter()
            .all(|row| row.row.snippet.chars().count() <= 160)
    );
    assert!(first.items.iter().all(|row| row.id != second.items[0].id));
    assert_eq!(
        store.activity_records(start, start, 0).unwrap_err(),
        DataError::Invalid
    );
    assert_eq!(
        store.activity_records(i64::MIN, i64::MAX, 0).unwrap_err(),
        DataError::Invalid
    );
    assert_eq!(
        store
            .activity_records(start, start + 1, usize::MAX)
            .unwrap_err(),
        DataError::Invalid
    );
}

#[test]
fn historical_original_still_opens_its_active_memory_after_version_restore() {
    let root = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(root.path()).unwrap();
    let saved = capture(&store, "Initial idea");
    let edit = store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: saved.memory_id.clone(),
            expected_version: saved.version_id.clone(),
            title: "Later idea".into(),
            body: "An added idea".into(),
        })
        .unwrap();
    let db = Connection::open(store.database_path()).unwrap();
    let source = id();
    db.execute("INSERT INTO captures(id,request_id,fingerprint,text,source,created_at) VALUES (?1,?1,x'00','Additional original',?2,1000)", params![source,r#"{"kind":"user","app":"Synthetic activity"}"#]).unwrap();
    db.execute(
        "INSERT INTO capture_state(capture_id) VALUES (?)",
        [&source],
    )
    .unwrap();
    db.execute(
        "INSERT INTO version_captures(version_id,capture_id) VALUES (?,?)",
        params![edit.after_version, source],
    )
    .unwrap();
    store
        .restore_version(
            &id(),
            &saved.memory_id,
            edit.after_version.as_ref().unwrap(),
            &saved.version_id,
        )
        .unwrap();
    let record = &store.activity_records(1000, 1001, 0).unwrap().items[0];
    assert_eq!(record.row.key.kind, "memory");
    assert_eq!(record.row.key.id, saved.memory_id);
}

#[test]
fn large_library_returns_sparse_counts_without_memory_bodies() {
    let root = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(root.path()).unwrap();
    let mut db = Connection::open(store.database_path()).unwrap();
    let tx = db.transaction().unwrap();
    for n in 0..10_000 {
        let source = id();
        tx.execute("INSERT INTO captures(id,request_id,fingerprint,text,source,created_at) VALUES (?1,?1,x'00',?2,?3,?4)", params![source, "Synthetic original. ".repeat(100),r#"{"kind":"user","app":"Synthetic activity"}"#,1_600_000_000_000_i64+(n%1000)*86_400_000]).unwrap();
        tx.execute(
            "INSERT INTO capture_state(capture_id) VALUES (?)",
            [&source],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    let start = std::time::Instant::now();
    let summary = store.activity_summary().unwrap();
    let elapsed = start.elapsed();
    assert_eq!(
        summary.days.iter().map(|day| day.count).sum::<i64>(),
        10_000
    );
    assert_eq!(summary.days.len(), 1000);
    let payload = serde_json::to_vec(&summary).unwrap();
    assert!(payload.len() < 40_000);
    println!(
        "Activity: 10000 originals, {} daily counts, {} bytes, {:?}",
        summary.days.len(),
        payload.len(),
        elapsed
    );
}
