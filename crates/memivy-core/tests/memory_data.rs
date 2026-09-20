use memivy_core::memory::*;
use memivy_core::model::{AssistantContent, Message, ToolCall};
use rig_core::message::ToolFunction;
use rusqlite::{Connection, params};
use serde_json::json;
use std::sync::{Arc, Barrier};
use uuid::Uuid;

fn id() -> String {
    Uuid::new_v4().to_string()
}
fn setup() -> (tempfile::TempDir, MemoryStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    (dir, store)
}
fn capture_request(text: &str) -> CaptureRequest {
    CaptureRequest {
        request_id: id(),
        text: text.into(),
        origin: Origin::User {
            app: "Test app".into(),
            project: None,
            uri: None,
        },
    }
}
fn new_memory(store: &MemoryStore, text: &str) -> (RawCapture, Receipt) {
    let request = capture_request(text);
    let saved = store.capture(&request).unwrap();
    (
        store.capture_by_id(&saved.capture_id).unwrap(),
        store.receipt(&request.request_id).unwrap(),
    )
}
fn edit(store: &MemoryStore, r: &Receipt, text: &str) -> Receipt {
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: r.memory_id.clone().unwrap(),
            expected_version: r.after_version.clone().unwrap(),
            title: "Edited".into(),
            body: text.into(),
        })
        .unwrap()
}
const SAVED_TEXT: &str = "  确认后修改的结论\n逐字保留🙂  ";
const SAVED_TITLE: &str = "User-approved name";
fn finished_turn(store: &MemoryStore, evidence: &[SourceRef]) -> (String, Turn) {
    let conversation = id();
    store
        .create_conversation(&conversation, "Continue discussion")
        .unwrap();
    let run = store
        .begin_agent_input(&id(), &id(), &conversation, "只是一个假设", &[], None)
        .unwrap();
    store
        .append_agent_text(&run.input_id, &run.attempt_id, "AI 建议，不自动保存")
        .unwrap();
    // Fixed database evidence fixture; agent protocol/citation validation is
    // exercised by the discussion tests rather than duplicated here.
    let db = Connection::open(store.database_path()).unwrap();
    for source in evidence {
        let (kind, source_id) = match source {
            SourceRef::Capture(id) => ("capture", id),
            SourceRef::Version(id) => ("version", id),
        };
        let length = store
            .resolve_source(source, 1000)
            .unwrap()
            .text
            .chars()
            .count();
        db.execute("INSERT INTO message_citations(message_id,kind,source_id,cited,excerpt_start,excerpt_length) VALUES(?1,?2,?3,1,0,?4)",params![run.assistant_message_id,kind,source_id,length as i64]).unwrap();
    }
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
    (conversation, store.turn(&run.input_id).unwrap())
}

#[test]
fn raw_and_versions_are_immutable_exact_and_request_scoped() {
    let (_dir, store) = setup();
    let request = capture_request(" \n保留空格与中文：_% OR ` ``` 🙂\n ");
    let saved = store.capture(&request).unwrap();
    let c = store.capture_by_id(&saved.capture_id).unwrap();
    assert_eq!(c.text, request.text);
    assert_eq!(c.id, store.capture(&request).unwrap().capture_id);
    let mut conflicting = request.clone();
    conflicting.text = "different".into();
    assert_eq!(
        store.capture(&conflicting).unwrap_err(),
        DataError::RequestConflict
    );
    let identical_text_new_request = store.capture(&capture_request(&request.text)).unwrap();
    assert_ne!(
        identical_text_new_request.capture_id, c.id,
        "separate intentional saves are not content-deduplicated"
    );
    let db = Connection::open(store.database_path()).unwrap();
    assert!(
        db.execute(
            "UPDATE captures SET text='AI rewrote this' WHERE id=?",
            [&c.id]
        )
        .is_err()
    );
    let r = store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: saved.memory_id,
            expected_version: saved.version_id,
            title: "版本一".into(),
            body: "当前理解".into(),
        })
        .unwrap();
    assert!(
        db.execute(
            "UPDATE memory_versions SET body='overwrite' WHERE id=?",
            [r.after_version.unwrap()]
        )
        .is_err()
    );
    assert_eq!(store.capture_by_id(&c.id).unwrap().text, request.text);
    store.check_integrity().unwrap();
}

#[test]
fn editing_restoring_and_undo_preserve_versions_and_refuse_stale_writes() {
    let (_dir, store) = setup();
    let (c, r) = new_memory(&store, "过去的想法");
    let edited = edit(&store, &r, "新的理解");
    let memory = r.memory_id.as_ref().unwrap();
    assert_eq!(store.history(memory).unwrap().len(), 2);
    assert_eq!(store.capture_by_id(&c.id).unwrap().text, "过去的想法");
    assert_eq!(
        store.undo(&id(), &r.request_id).unwrap_err(),
        DataError::Conflict
    );
    let stale = EditRequest {
        request_id: id(),
        memory_id: memory.clone(),
        expected_version: r.after_version.clone().unwrap(),
        title: "Stale edit".into(),
        body: "不得覆盖".into(),
    };
    assert_eq!(store.edit_memory(&stale).unwrap_err(), DataError::Conflict);
    assert_eq!(store.memory(memory).unwrap().current.body, "新的理解");
    let undo_id = id();
    let undone = store.undo(&undo_id, &edited.request_id).unwrap();
    assert_eq!(undone, store.undo(&undo_id, &edited.request_id).unwrap());
    assert_eq!(store.memory(memory).unwrap().current.body, "过去的想法");
    assert_eq!(store.history(memory).unwrap().len(), 3);
    let restored = store
        .restore_version(
            &id(),
            memory,
            undone.after_version.as_ref().unwrap(),
            edited.after_version.as_ref().unwrap(),
        )
        .unwrap();
    assert_eq!(store.memory(memory).unwrap().current.body, "新的理解");
    assert_eq!(store.history(memory).unwrap().len(), 4);
    assert_ne!(restored.after_version, edited.after_version);
}

#[test]
fn trash_hides_history_and_exclusive_raw_but_can_restore_everything() {
    let (_dir, store) = setup();
    let (c, r) = new_memory(&store, "独占原话");
    let changed = edit(&store, &r, "当前正文");
    let memory = r.memory_id.as_ref().unwrap();
    let head = changed.after_version.as_ref().unwrap();
    store.trash_memory(memory, head).unwrap();
    store.trash_memory(memory, head).unwrap();
    assert!(
        store
            .library(&LibraryQuery::default())
            .unwrap()
            .items
            .is_empty()
    );
    assert!(store.capture_by_id(&c.id).is_err());
    assert!(
        store
            .resolve_source(&SourceRef::Version(r.after_version.clone().unwrap()), 100)
            .is_err()
    );
    assert_eq!(store.memories(true, 50).unwrap()[0].state, "trashed");
    assert_eq!(store.history(memory).unwrap().len(), 2);
    store.restore_memory(memory).unwrap();
    assert_eq!(store.capture_by_id(&c.id).unwrap().text, "独占原话");
    assert_eq!(store.memory(memory).unwrap().current.body, "当前正文");
}

#[test]
fn undoing_a_capture_keeps_its_archive_outside_search() {
    let (_dir, store) = setup();
    let (capture, r) = new_memory(&store, "撤销后仅供恢复的原话");
    store.undo(&id(), &r.request_id).unwrap();
    assert_eq!(store.capture_by_id(&capture.id).unwrap().text, capture.text);
    assert!(
        store
            .search(&SearchRequest::text("仅供恢复", 20))
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn keyword_search_handles_chinese_short_literal_and_combined_version_sources() {
    let (_dir, store) = setup();
    let raw = store
        .capture(&capture_request(
            "中文短语 收费计划 SQLite foo_bar 100% OR \"quoted\" 🙂",
        ))
        .unwrap();
    for query in [
        "中文短语",
        "收费",
        "SQLite",
        "foo_bar",
        "100%",
        "OR",
        "\"quoted\"",
        "🙂",
    ] {
        assert_eq!(
            store
                .search(&SearchRequest::text(query, 10))
                .unwrap()
                .items
                .len(),
            1,
            "{query}"
        );
    }
    assert!(
        store
            .search(&SearchRequest::text("不存在", 10))
            .unwrap()
            .items
            .is_empty()
    );
    let r = store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: raw.memory_id.clone(),
            expected_version: raw.version_id.clone(),
            title: "首次体验".into(),
            body: "另一种当前理解".into(),
        })
        .unwrap();
    let hits = store
        .search(&SearchRequest::text("首次体验 中文短语", 10))
        .unwrap()
        .items;
    assert!(
        hits.is_empty(),
        "archive terms cannot combine with current title"
    );
    let edited = edit(&store, &r, "修改后理解");
    assert!(
        store
            .search(&SearchRequest::text("另一种当前理解", 10))
            .unwrap()
            .items
            .is_empty(),
        "superseded summaries must not masquerade as current memories"
    );
    store
        .trash_memory(
            r.memory_id.as_ref().unwrap(),
            edited.after_version.as_ref().unwrap(),
        )
        .unwrap();
    assert!(
        store
            .search(&SearchRequest::text("中文短语", 10))
            .unwrap()
            .items
            .is_empty()
    );
    store.purge_memory(r.memory_id.as_ref().unwrap()).unwrap();
    let db = Connection::open(store.database_path()).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM record_fts WHERE record_fts MATCH '中文短语'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn shared_sources_survive_purging_another_memory_and_its_history() {
    let (_dir, store) = setup();
    let (c, a) = new_memory(&store, "共享来源");
    let conversation = id();
    store
        .create_conversation(&conversation, "Shared source")
        .unwrap();
    let run = store
        .begin_agent_input(
            &id(),
            &id(),
            &conversation,
            "Save another memory from this source",
            &[],
            None,
        )
        .unwrap();
    let args = MemoryWriteArgs {
        destination: Destination::New,
        title: "Shared source".into(),
        parts: vec![MemoryWritePart {
            text: c.text.clone(),
            sources: vec![MemorySourceQuote {
                source_id: c.id.clone(),
                quote: c.text.clone(),
            }],
        }],
    };
    let call = id();
    let protocol = vec![json!(Message::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(ToolCall::from_wire(
            &call,
            ToolFunction::new("write_memory".into(), json!(args))
        ))]
    })];
    store
        .checkpoint_agent(&run.input_id, &run.attempt_id, &protocol)
        .unwrap();
    let op = store
        .stage_agent_operation(
            &run.input_id,
            &run.attempt_id,
            &call,
            "write_memory",
            &json!(args),
        )
        .unwrap();
    let b = store
        .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap()
        .receipt
        .unwrap();
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
    store
        .trash_memory(
            a.memory_id.as_ref().unwrap(),
            a.after_version.as_ref().unwrap(),
        )
        .unwrap();
    assert!(store.capture_by_id(&c.id).is_ok());
    store
        .trash_memory(
            b.memory_id.as_ref().unwrap(),
            b.after_version.as_ref().unwrap(),
        )
        .unwrap();
    store.purge_memory(b.memory_id.as_ref().unwrap()).unwrap();
    store.restore_memory(a.memory_id.as_ref().unwrap()).unwrap();
    assert_eq!(store.capture_by_id(&c.id).unwrap().text, "共享来源");
}

#[test]
fn purging_erases_original_content_and_retries_never_resurrect_it() {
    let (_dir, store) = setup();
    let (c, r) = new_memory(&store, "将被彻底清除的原文");
    let memory = r.memory_id.as_ref().unwrap();
    store
        .trash_memory(memory, r.after_version.as_ref().unwrap())
        .unwrap();
    store.purge_memory(memory).unwrap();
    store.purge_memory(memory).unwrap();
    assert!(store.restore_memory(memory).is_err());
    let db = Connection::open(store.database_path()).unwrap();
    assert_eq!(
        db.query_row("SELECT text FROM captures WHERE id=?", [&c.id], |r| r
            .get::<_, Option<String>>(0))
            .unwrap(),
        None
    );
    assert_eq!(
        db.query_row(
            "SELECT body FROM memory_versions WHERE id=?",
            [r.after_version.unwrap()],
            |r| r.get::<_, Option<String>>(0)
        )
        .unwrap(),
        None
    );
    assert!(
        store
            .library(&LibraryQuery::default())
            .unwrap()
            .items
            .is_empty()
    );
    store.check_integrity().unwrap();
}

#[test]
fn fresh_concurrent_open_never_misclassifies_a_valid_database() {
    let parent = tempfile::tempdir().unwrap();
    for round in 0..100 {
        let root = parent.path().join(round.to_string());
        let barrier = Arc::new(Barrier::new(4));
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let root = root.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    // A real lock conflict is retryable; format errors are not.
                    for _ in 0..20 {
                        match MemoryStore::open(&root) {
                            Err(DataError::Busy) => {
                                std::thread::sleep(std::time::Duration::from_millis(10))
                            }
                            result => return result,
                        }
                    }
                    Err(DataError::Busy)
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap().unwrap().check_integrity().unwrap();
        }
        MemoryStore::open(&root).unwrap().check_integrity().unwrap();
    }
}

#[test]
fn discussions_and_drafts_are_not_memories_and_manual_save_retains_provenance() {
    let (_dir, store) = setup();
    let (conversation, turn) = finished_turn(&store, &[]);
    store
        .save_workspace_draft(&WorkspaceDraft {
            key: format!("discussion:{conversation}"),
            request_id: id(),
            title: String::new(),
            body: "尚未确定的草稿".into(),
            expected_version: None,
            origin: None,
            context: vec![],
            destination: None,
        })
        .unwrap();
    assert!(
        store
            .library(&LibraryQuery::default())
            .unwrap()
            .items
            .is_empty()
    );
    let request = id();
    let r = store
        .save_agent_text(
            &request,
            &turn.id,
            SAVED_TEXT,
            SAVED_TITLE,
            &Destination::New,
        )
        .unwrap();
    let raw = store.capture_by_id(r.capture_id.as_ref().unwrap()).unwrap();
    assert_eq!(raw.text, SAVED_TEXT);
    assert!(
        matches!(raw.origin,Origin::Conversation{message_role,confirmed_by,..} if message_role=="assistant" && confirmed_by=="user")
    );
    assert_eq!(
        store
            .save_agent_text(
                &request,
                &turn.id,
                SAVED_TEXT,
                SAVED_TITLE,
                &Destination::New
            )
            .unwrap(),
        r
    );
    assert_eq!(
        store
            .memory(r.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        SAVED_TEXT
    );
    assert_eq!(
        store
            .save_agent_text(
                &request,
                &turn.id,
                "different",
                SAVED_TITLE,
                &Destination::New
            )
            .unwrap_err(),
        DataError::RequestConflict
    );
    store.undo_agent_input(&id(), &request).unwrap();
    assert_eq!(
        store
            .save_agent_text(
                &request,
                &turn.id,
                SAVED_TEXT,
                SAVED_TITLE,
                &Destination::New
            )
            .unwrap()
            .status,
        "undone"
    );
    assert!(store.memory(r.memory_id.as_ref().unwrap()).is_err());
    assert_eq!(
        store
            .capture_by_id(r.capture_id.as_ref().unwrap())
            .unwrap()
            .text,
        SAVED_TEXT
    );
    store.check_integrity().unwrap();
}

#[test]
fn stale_manual_save_target_leaves_no_archive_or_new_memory() {
    let (_dir, store) = setup();
    let (_, target) = new_memory(&store, "Old version");
    let (_, turn) = finished_turn(&store, &[]);
    edit(&store, &target, "已改版");
    for destination in [
        Destination::Existing {
            memory_id: target.memory_id.clone().unwrap(),
            expected_version: target.after_version.clone().unwrap(),
        },
        Destination::Existing {
            memory_id: id(),
            expected_version: id(),
        },
    ] {
        assert_eq!(
            store
                .save_agent_text(&id(), &turn.id, SAVED_TEXT, SAVED_TITLE, &destination)
                .unwrap_err(),
            DataError::Conflict
        );
    }
    assert_eq!(
        store
            .memory(target.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        "已改版"
    );
    assert_eq!(
        Connection::open(store.database_path())
            .unwrap()
            .query_row("SELECT count(*) FROM captures", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn citations_bind_to_fixed_versions_and_deleted_sources_are_unavailable() {
    let (_dir, store) = setup();
    let (c, r) = new_memory(&store, "当时的依据");
    let old = SourceRef::Version(r.after_version.clone().unwrap());
    let (_, turn) = finished_turn(&store, std::slice::from_ref(&old));
    let saved = store
        .save_agent_text(&id(), &turn.id, SAVED_TEXT, SAVED_TITLE, &Destination::New)
        .unwrap();
    let edited = edit(&store, &r, "后来的依据");
    assert_eq!(store.resolve_source(&old, 500).unwrap().text, "当时的依据");
    assert!(store.turn(&turn.id).unwrap().assistant.citations[0].available);
    store
        .trash_memory(
            r.memory_id.as_ref().unwrap(),
            edited.after_version.as_ref().unwrap(),
        )
        .unwrap();
    assert!(!store.turn(&turn.id).unwrap().assistant.citations[0].available);
    assert!(
        !store
            .capture_citations(saved.capture_id.as_ref().unwrap())
            .unwrap()[0]
            .available
    );
    store.purge_memory(r.memory_id.as_ref().unwrap()).unwrap();
    assert!(store.resolve_source(&old, 500).is_err());
    assert!(store.capture_by_id(&c.id).is_err());
    assert!(store.memory(saved.memory_id.as_ref().unwrap()).is_ok());
}

#[test]
fn cancellation_and_explicit_restart_recovery_fence_late_results() {
    let (_dir, store) = setup();
    let conversation = id();
    store
        .create_conversation(&conversation, "Failure recovery")
        .unwrap();
    let run = store
        .begin_agent_input(&id(), &id(), &conversation, "第一次", &[], None)
        .unwrap();
    assert_eq!(
        store
            .begin_agent_input(
                &run.input_id,
                &run.attempt_id,
                &conversation,
                "第一次",
                &[],
                None
            )
            .unwrap()
            .user_message_id,
        run.user_message_id
    );
    assert_eq!(
        store
            .begin_agent_input(&id(), &id(), &conversation, "第二次", &[], None)
            .unwrap_err(),
        DataError::Conflict
    );
    store
        .stop_agent_input(&run.input_id, &run.attempt_id, "cancelled", None)
        .unwrap();
    assert_eq!(
        store
            .finish_agent_input(&run.input_id, &run.attempt_id, &[])
            .unwrap_err(),
        DataError::Conflict
    );
    let next = store
        .begin_agent_input(&id(), &id(), &conversation, "新表达", &[], None)
        .unwrap();
    let reopened = MemoryStore::open(store.database_path().parent().unwrap()).unwrap();
    assert_eq!(
        reopened.turn(&next.input_id).unwrap().assistant.status,
        "processing",
        "opening another reader must not cancel live work"
    );
    assert_eq!(reopened.recover_interrupted_turns().unwrap(), 1);
    assert_eq!(
        store
            .finish_agent_input(&next.input_id, &next.attempt_id, &[])
            .unwrap_err(),
        DataError::Conflict
    );
    assert!(
        store
            .library(&LibraryQuery::default())
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn conversation_cursor_reads_do_not_drop_old_messages() {
    let (_dir, store) = setup();
    let conversation = id();
    store
        .create_conversation(&conversation, "Pagination")
        .unwrap();
    for _ in 0..55 {
        let t = store
            .begin_agent_input(&id(), &id(), &conversation, "问题", &[], None)
            .unwrap();
        store
            .append_agent_text(&t.input_id, &t.attempt_id, "回答")
            .unwrap();
        store
            .finish_agent_input(&t.input_id, &t.attempt_id, &[])
            .unwrap();
    }
    let first = store.messages(&conversation, 0, 100).unwrap();
    assert_eq!(first.len(), 100);
    let second = store
        .messages(&conversation, first.last().unwrap().seq, 100)
        .unwrap();
    assert_eq!(second.len(), 10);
    assert!(second[0].seq > first.last().unwrap().seq);
    assert_eq!(first[0].role, "user");
    assert!(
        store
            .library(&LibraryQuery::default())
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn concurrent_writers_deduplicate_and_only_one_edit_wins() {
    let (_dir, store) = setup();
    let barrier = Arc::new(Barrier::new(8));
    let request = capture_request("相同提交");
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let s = store.clone();
            let b = barrier.clone();
            let r = request.clone();
            std::thread::spawn(move || {
                b.wait();
                s.capture(&r).unwrap().capture_id
            })
        })
        .collect();
    let ids: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(ids.iter().all(|id| id == &ids[0]));
    let (_, r) = new_memory(&store, "共享旧版本");
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|i| {
            let s = store.clone();
            let b = barrier.clone();
            let r = r.clone();
            std::thread::spawn(move || {
                b.wait();
                s.edit_memory(&EditRequest {
                    request_id: id(),
                    memory_id: r.memory_id.unwrap(),
                    expected_version: r.after_version.unwrap(),
                    title: "Concurrent edit".into(),
                    body: format!("作者{i}"),
                })
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(DataError::Conflict)))
            .count(),
        1
    );
    store.check_integrity().unwrap();
}

#[test]
fn lock_timeout_and_database_errors_do_not_expose_content_or_leave_partial_versions() {
    let (_dir, store) = setup();
    let (c, r) = new_memory(&store, "私密原话");
    let db = Connection::open(store.database_path()).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    let request = capture_request("不得出现在错误日志的内容");
    let error = store.capture(&request).unwrap_err();
    assert_eq!(error, DataError::Busy);
    assert!(!format!("{error:?} {error}").contains(&request.text));
    db.execute_batch("ROLLBACK").unwrap();
    assert!(store.capture(&request).is_ok());
    db.execute_batch("CREATE TRIGGER inject_receipt_failure BEFORE INSERT ON receipts BEGIN SELECT RAISE(ABORT, 'sensitive sql content'); END;").unwrap();
    let error = store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: r.memory_id.clone().unwrap(),
            expected_version: r.after_version.clone().unwrap(),
            title: "应回滚".into(),
            body: "不能留下半条版本".into(),
        })
        .unwrap_err();
    assert_eq!(error, DataError::Database);
    assert!(!format!("{error:?} {error}").contains("sensitive"));
    assert_eq!(
        store.history(r.memory_id.as_ref().unwrap()).unwrap().len(),
        1
    );
    assert_eq!(store.capture_by_id(&c.id).unwrap().text, "私密原话");
    db.execute_batch("DROP TRIGGER inject_receipt_failure")
        .unwrap();
    store.check_integrity().unwrap();
}

#[test]
fn database_reopens_and_rejects_unrelated_and_future_schemas() {
    let (dir, store) = setup();
    let saved = store.capture(&capture_request("初始化后保留")).unwrap();
    let reopened = MemoryStore::open(dir.path()).unwrap();
    assert_eq!(
        reopened.capture_by_id(&saved.capture_id).unwrap().text,
        "初始化后保留"
    );
    reopened.check_integrity().unwrap();
    let db = Connection::open(store.database_path()).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    let application_id: i64 = db
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .unwrap();
    db.pragma_update(None, "application_id", 0x12345678_i64)
        .unwrap();
    assert_eq!(
        MemoryStore::open(dir.path()).unwrap_err(),
        DataError::Schema
    );
    assert_eq!(
        store
            .capture(&capture_request("不能写其他格式库"))
            .unwrap_err(),
        DataError::Schema
    );
    let foreign_backup = dir.path().join("foreign.db");
    db.execute("VACUUM INTO ?1", [foreign_backup.to_str().unwrap()])
        .unwrap();
    assert_eq!(
        store.prepare_restore(&foreign_backup).unwrap_err(),
        DataError::Schema
    );
    db.pragma_update(None, "application_id", application_id)
        .unwrap();
    db.pragma_update(None, "user_version", 999_i64).unwrap();
    assert_eq!(
        MemoryStore::open(dir.path()).unwrap_err(),
        DataError::Schema
    );
    assert_eq!(
        store.capture(&capture_request("不能写新版库")).unwrap_err(),
        DataError::Schema
    );
    let unrelated = tempfile::tempdir().unwrap();
    let db = Connection::open(unrelated.path().join("memivy.db")).unwrap();
    db.execute_batch("CREATE TABLE other (id TEXT)").unwrap();
    assert_eq!(
        MemoryStore::open(unrelated.path()).unwrap_err(),
        DataError::Schema
    );
}

#[test]
fn backups_restore_versions_conversations_trash_and_exclude_credentials() {
    let (dir, store) = setup();
    let (c, r) = new_memory(&store, "完整备份");
    let (conversation, turn) = finished_turn(
        &store,
        &[SourceRef::Version(r.after_version.clone().unwrap())],
    );
    let edit = edit(&store, &r, "包含历史");
    let request = id();
    let saved = store
        .save_agent_text(
            &request,
            &turn.id,
            SAVED_TEXT,
            SAVED_TITLE,
            &Destination::New,
        )
        .unwrap();
    store
        .trash_memory(
            r.memory_id.as_ref().unwrap(),
            edit.after_version.as_ref().unwrap(),
        )
        .unwrap();
    let sentinel = "API_KEY_SHOULD_NEVER_APPEAR_3827";
    std::fs::write(store.model_config_path(), sentinel).unwrap();
    let backup = dir.path().join("backup.sqlite3");
    store.backup(&backup).unwrap();
    assert_eq!(
        store.backup(&backup).unwrap_err(),
        DataError::DestinationExists
    );
    assert!(!String::from_utf8_lossy(&std::fs::read(&backup).unwrap()).contains(sentinel));
    let restore = dir.path().join("restored");
    let target = MemoryStore::open(&restore).unwrap();
    let prepared = target.prepare_restore(&backup).unwrap();
    target.arm_restore(&prepared.id).unwrap();
    let restored = MemoryStore::open_application(&restore).unwrap();
    assert!(!restored.model_config_path().exists());
    assert!(!restore.join("mcp.json").exists());
    assert_eq!(
        restored.conversation(&conversation).unwrap().title,
        "Continue discussion"
    );
    assert_eq!(
        restored
            .save_agent_text(
                &request,
                &turn.id,
                SAVED_TEXT,
                SAVED_TITLE,
                &Destination::New
            )
            .unwrap(),
        saved
    );
    restored
        .restore_memory(r.memory_id.as_ref().unwrap())
        .unwrap();
    assert_eq!(
        restored
            .history(r.memory_id.as_ref().unwrap())
            .unwrap()
            .len(),
        2
    );
    assert_eq!(restored.capture_by_id(&c.id).unwrap().text, "完整备份");
    let invalid = dir.path().join("invalid.sqlite3");
    std::fs::write(&invalid, "not a database").unwrap();
    assert!(restored.prepare_restore(&invalid).is_err());
    restored.check_integrity().unwrap();
}
