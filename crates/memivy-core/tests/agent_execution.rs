use memivy_core::memory::*;
use rusqlite::Connection;
use serde_json::{Value, json};
use uuid::Uuid;

fn id() -> String {
    Uuid::new_v4().to_string()
}
fn setup() -> (tempfile::TempDir, MemoryStore, String) {
    let root = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(root.path()).unwrap();
    let conversation = id();
    store
        .create_conversation(&conversation, "Discussion")
        .unwrap();
    (root, store, conversation)
}
fn begin(store: &MemoryStore, conversation: &str, text: &str) -> AgentExecution {
    store
        .begin_agent_input(&id(), &id(), conversation, text, &[], None)
        .unwrap()
}
fn stage(store: &MemoryStore, run: &AgentExecution, name: &str, args: &Value) -> AgentOperation {
    let call = id();
    let mut protocol = store.agent_execution(&run.input_id).unwrap().protocol;
    protocol.push(json!({"role":"assistant","content":null,"tool_calls":[{"id":call,"type":"function","function":{"name":name,"arguments":args.to_string()}}]}));
    store
        .checkpoint_agent(&run.input_id, &run.attempt_id, &protocol)
        .unwrap();
    store
        .stage_agent_operation(&run.input_id, &run.attempt_id, &call, name, args)
        .unwrap()
}
fn sourced_part(text: &str, source: &str, quote: &str) -> MemoryWritePart {
    MemoryWritePart {
        text: text.into(),
        sources: vec![MemorySourceQuote {
            source_id: source.into(),
            quote: quote.into(),
        }],
    }
}
fn write(
    store: &MemoryStore,
    run: &AgentExecution,
    destination: Destination,
    body: &str,
) -> AgentOperation {
    let request = MemoryWriteArgs {
        destination,
        title: "Plan".into(),
        parts: vec![sourced_part(body, &run.user_message_id, &run.input_text)],
    };
    let name = "write_memory";
    let op = stage(store, run, name, &serde_json::to_value(&request).unwrap());
    store
        .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &request)
        .unwrap()
}
fn captured(store: &MemoryStore, text: &str) -> CaptureResult {
    store
        .capture(&CaptureRequest {
            request_id: id(),
            text: text.into(),
            origin: Origin::User {
                app: "Test app".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap()
}
fn count(store: &MemoryStore, sql: &str) -> i64 {
    Connection::open(store.database_path())
        .unwrap()
        .query_row(sql, [], |r| r.get(0))
        .unwrap()
}

#[test]
fn logical_input_retains_one_exact_user_message_across_attempts_and_network_redelivery() {
    let (_root, store, conversation) = setup();
    let text = "  原话：不是 50000，是 5000 元。🙂\n";
    let run = begin(&store, &conversation, text);
    let same = store
        .begin_agent_input(
            &run.input_id,
            &run.attempt_id,
            &conversation,
            text,
            &[],
            None,
        )
        .unwrap();
    assert_eq!(same.user_message_id, run.user_message_id);
    store
        .stop_agent_input(&run.input_id, &run.attempt_id, "failed", Some("network"))
        .unwrap();
    let retry = store
        .begin_agent_input(&run.input_id, &id(), &conversation, text, &[], None)
        .unwrap();
    assert_eq!(retry.user_message_id, run.user_message_id);
    assert_eq!(retry.input_text, text);
    assert_eq!(store.messages(&conversation, 0, 100).unwrap().len(), 2);
    assert_eq!(
        store
            .begin_agent_input(&run.input_id, &id(), &conversation, "changed", &[], None)
            .unwrap_err(),
        DataError::RequestConflict
    );
    assert_eq!(
        count(&store, "SELECT count(*) FROM captures"),
        0,
        "a question/operation alone is not durable memory"
    );
}

#[test]
fn commit_without_delivery_replays_result_and_receipt_in_new_attempt() {
    let (_root, store, conversation) = setup();
    let run = begin(&store, &conversation, "我决定先验证收费，还没有上线");
    let request = MemoryWriteArgs {
        destination: Destination::New,
        title: "收费".into(),
        parts: vec![sourced_part(
            "决定验证收费；尚未上线",
            &run.user_message_id,
            &run.input_text,
        )],
    };
    let args = serde_json::to_value(&request).unwrap();
    let op = stage(&store, &run, "write_memory", &args);
    // Simulate disconnection after SQLite COMMIT, before any UI/tool response.
    let committed = store
        .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &request)
        .unwrap();
    store
        .stop_agent_input(&run.input_id, &run.attempt_id, "failed", Some("network"))
        .unwrap();
    let retry = store
        .begin_agent_input(
            &run.input_id,
            &id(),
            &conversation,
            &run.input_text,
            &[],
            None,
        )
        .unwrap();
    let restaged = store
        .stage_agent_operation(
            &retry.input_id,
            &retry.attempt_id,
            &op.call_id,
            "write_memory",
            &args,
        )
        .unwrap();
    assert_eq!(restaged.operation_id, op.operation_id);
    assert_eq!(restaged.result, committed.result);
    let replay = store
        .apply_agent_memory(
            &retry.input_id,
            &retry.attempt_id,
            &op.operation_id,
            &request,
        )
        .unwrap();
    assert_eq!(replay.receipt, committed.receipt);
    assert_eq!(count(&store, "SELECT count(*) FROM memories"), 1);
    assert_eq!(count(&store, "SELECT count(*) FROM memory_versions"), 1);
    assert_eq!(count(&store, "SELECT count(*) FROM receipts"), 1);
    assert_eq!(count(&store, "SELECT count(*) FROM captures"), 1);
    assert_eq!(count(&store, "SELECT count(*) FROM organization_jobs"), 0);
}

#[test]
fn stale_attempt_cannot_write_complete_read_results_or_append_text() {
    let (_root, store, conversation) = setup();
    let old = begin(&store, &conversation, "我有一个想法");
    let request = MemoryWriteArgs {
        destination: Destination::New,
        title: "Idea".into(),
        parts: vec![sourced_part(
            "New idea",
            &old.user_message_id,
            &old.input_text,
        )],
    };
    let op = stage(
        &store,
        &old,
        "write_memory",
        &serde_json::to_value(&request).unwrap(),
    );
    let current = store
        .begin_agent_input(
            &old.input_id,
            &id(),
            &conversation,
            &old.input_text,
            &[],
            None,
        )
        .unwrap();
    assert_eq!(
        store
            .apply_agent_memory(&old.input_id, &old.attempt_id, &op.operation_id, &request)
            .unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(
        store
            .complete_agent_operation(
                &old.input_id,
                &old.attempt_id,
                &op.operation_id,
                &json!({"ok":true})
            )
            .unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(
        store
            .append_agent_text(&old.input_id, &old.attempt_id, "迟到正文")
            .unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(count(&store, "SELECT count(*) FROM memories"), 0);
    assert_eq!(
        store.agent_execution(&old.input_id).unwrap().attempt_id,
        current.attempt_id
    );
}

#[test]
fn cancellation_and_restart_preserve_visible_text_but_retry_resumes_checkpoint() {
    let (_root, store, conversation) = setup();
    let run = begin(&store, &conversation, "分析这个计划");
    store
        .append_agent_text(&run.input_id, &run.attempt_id, "先查相关记忆。🙂")
        .unwrap();
    let protocol = vec![json!({"role":"assistant","content":"先查相关记忆。🙂"})];
    store
        .checkpoint_agent(&run.input_id, &run.attempt_id, &protocol)
        .unwrap();
    store
        .append_agent_text(&run.input_id, &run.attempt_id, "尚未完成的尾段")
        .unwrap();
    store
        .stop_agent_input(&run.input_id, &run.attempt_id, "cancelled", None)
        .unwrap();
    assert_eq!(
        store.agent_execution(&run.input_id).unwrap().text,
        "先查相关记忆。🙂尚未完成的尾段"
    );
    assert_eq!(
        store
            .append_agent_text(&run.input_id, &run.attempt_id, "迟到")
            .unwrap_err(),
        DataError::Conflict
    );
    let retry = store
        .begin_agent_input(
            &run.input_id,
            &id(),
            &conversation,
            &run.input_text,
            &[],
            None,
        )
        .unwrap();
    assert_eq!(retry.text, "先查相关记忆。🙂");
    assert_eq!(retry.protocol, protocol);
    store
        .append_agent_text(&retry.input_id, &retry.attempt_id, "新的尾段")
        .unwrap();
    store.recover_interrupted_turns().unwrap();
    assert_eq!(
        store.agent_execution(&run.input_id).unwrap().state,
        "interrupted"
    );
    assert_eq!(
        store.agent_execution(&run.input_id).unwrap().text,
        "先查相关记忆。🙂新的尾段"
    );
    assert_eq!(
        store
            .finish_agent_input(&retry.input_id, &retry.attempt_id, &[])
            .unwrap_err(),
        DataError::Conflict
    );
}

#[test]
fn source_archive_and_atomic_undo_outlive_deleted_conversation() {
    let (_root, store, conversation) = setup();
    let origin = Origin::User {
        app: "Editor".into(),
        project: Some("Memivy".into()),
        uri: Some("file:///draft".into()),
    };
    let run = store
        .begin_agent_input(
            &id(),
            &id(),
            &conversation,
            "  决定先不收费\n",
            &[],
            Some(&origin),
        )
        .unwrap();
    let op = write(&store, &run, Destination::New, "决定先不收费");
    let receipt = op.receipt.unwrap();
    let memory = receipt.memory_id.clone().unwrap();
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
    let key = RecordKey {
        kind: "memory".into(),
        id: memory.clone(),
    };
    assert_eq!(
        store.library_detail_view(&key, true).unwrap().sources[0].conversation_available,
        Some(true)
    );
    let unrelated = captured(&store, "不相关的独立记忆");
    let cursor = store.library_changes(None).unwrap().cursor;
    store.delete_conversation(&conversation).unwrap();
    let changes = store.library_changes(Some(&cursor)).unwrap();
    assert!(!changes.reset);
    assert!(
        changes
            .changes
            .iter()
            .any(|c| c.domain == "memory" && c.entity == format!("memory:{memory}"))
    );
    assert!(
        !changes
            .changes
            .iter()
            .any(|c| c.domain == "memory" && c.entity == format!("memory:{}", unrelated.memory_id))
    );
    assert_eq!(
        store.library_detail_view(&key, true).unwrap().sources[0].conversation_available,
        Some(false)
    );
    let source = store
        .capture_by_id(receipt.capture_id.as_ref().unwrap())
        .unwrap();
    assert_eq!(source.text, "  决定先不收费\n");
    assert!(
        matches!(source.origin,Origin::Discussion{app,project,..} if app=="Editor" && project.as_deref()==Some("Memivy"))
    );
    assert_eq!(store.memory(&memory).unwrap().current.actor, "ai");
    assert_eq!(store.agent_input_receipts(&run.input_id).unwrap().len(), 1);
    assert_eq!(count(&store, "SELECT count(*) FROM agent_operations"), 0);
    let undone = store.undo_agent_input(&id(), &run.input_id).unwrap();
    assert!(undone.conflicts.is_empty());
    assert!(undone.receipt.is_some());
    assert_eq!(store.memory(&memory).unwrap_err(), DataError::Unavailable);
    assert!(
        store
            .capture_by_id(receipt.capture_id.as_ref().unwrap())
            .is_ok()
    );
}

#[test]
fn grouped_undo_restores_all_memories_memberships_and_multiple_updates_to_one_memory() {
    let (_root, store, conversation) = setup();
    let collection = id();
    store
        .save_collection(&collection, "Collection", "", None)
        .unwrap();
    // Use the public scoped conversation entry with the same ordinary execution API.
    let scoped = id();
    store
        .create_scoped_conversation(&scoped, "Collection discussion", Some(&collection))
        .unwrap();
    let before = captured(&store, "原先计划");
    let run = begin(&store, &scoped, "修改计划并增加想法");
    let created = write(&store, &run, Destination::New, "New idea")
        .receipt
        .unwrap();
    let updated = write(
        &store,
        &run,
        Destination::Existing {
            memory_id: before.memory_id.clone(),
            expected_version: before.version_id.clone(),
        },
        "第一版调整",
    )
    .receipt
    .unwrap();
    write(
        &store,
        &run,
        Destination::Existing {
            memory_id: before.memory_id.clone(),
            expected_version: updated.after_version.unwrap(),
        },
        "第二版调整",
    );
    assert_eq!(
        store
            .record_navigation(&RecordKey {
                kind: "memory".into(),
                id: created.memory_id.clone().unwrap()
            })
            .unwrap()
            .collections,
        vec![collection]
    );
    let request = id();
    let undone = store.undo_agent_input(&request, &run.input_id).unwrap();
    assert!(undone.conflicts.is_empty());
    assert_eq!(
        store.memory(&before.memory_id).unwrap().current.body,
        "原先计划"
    );
    assert_eq!(
        store
            .memory(created.memory_id.as_ref().unwrap())
            .unwrap_err(),
        DataError::Unavailable
    );
    assert_eq!(count(&store, "SELECT count(*) FROM collection_entries"), 0);
    assert_eq!(
        store
            .undo_agent_input(&request, &run.input_id)
            .unwrap()
            .receipt,
        undone.receipt
    );
    assert!(
        store
            .agent_input_receipts(&run.input_id)
            .unwrap()
            .iter()
            .all(|r| r.status == "undone")
    );
    let _ = conversation;
}

#[test]
fn grouped_undo_conflict_rolls_back_every_memory_and_membership() {
    let (_root, store, conversation) = setup();
    let a = captured(&store, "旧 A");
    let b = captured(&store, "旧 B");
    let run = begin(&store, &conversation, "调整 A 和 B");
    let a_new = write(
        &store,
        &run,
        Destination::Existing {
            memory_id: a.memory_id.clone(),
            expected_version: a.version_id,
        },
        "新 A",
    )
    .receipt
    .unwrap();
    let b_new = write(
        &store,
        &run,
        Destination::Existing {
            memory_id: b.memory_id.clone(),
            expected_version: b.version_id,
        },
        "新 B",
    )
    .receipt
    .unwrap();
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: b.memory_id.clone(),
            expected_version: b_new.after_version.unwrap(),
            title: "B".into(),
            body: "人工编辑".into(),
        })
        .unwrap();
    let versions = count(&store, "SELECT count(*) FROM memory_versions");
    let result = store.undo_agent_input(&id(), &run.input_id).unwrap();
    assert_eq!(result.conflicts, vec![b.memory_id.clone()]);
    assert!(result.receipt.is_none());
    assert_eq!(
        count(&store, "SELECT count(*) FROM memory_versions"),
        versions
    );
    assert_eq!(
        store.memory(&a.memory_id).unwrap().current.id,
        a_new.after_version.unwrap()
    );
    assert_eq!(store.memory(&b.memory_id).unwrap().current.body, "人工编辑");
}

#[test]
fn failure_before_commit_preserves_prior_success_without_archiving_a_second_source() {
    let (_root, store, conversation) = setup();
    let target = captured(&store, "目标");
    let run = begin(&store, &conversation, "增加想法并调整目标");
    write(&store, &run, Destination::New, "成功的新想法");
    let request = MemoryWriteArgs {
        destination: Destination::Existing {
            memory_id: target.memory_id.clone(),
            expected_version: id(),
        },
        title: "目标".into(),
        parts: vec![sourced_part(
            "过期的修改",
            &run.user_message_id,
            &run.input_text,
        )],
    };
    let op = stage(
        &store,
        &run,
        "write_memory",
        &serde_json::to_value(&request).unwrap(),
    );
    assert_eq!(
        store
            .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &request)
            .unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(store.agent_input_receipts(&run.input_id).unwrap().len(), 1);
    assert_eq!(
        store.memory(&target.memory_id).unwrap().current.body,
        "目标"
    );
    assert_eq!(count(&store, "SELECT count(*) FROM captures"), 2);
    assert_eq!(
        store
            .agent_execution(&run.input_id)
            .unwrap()
            .operations
            .last()
            .unwrap()
            .result,
        None
    );
}

#[test]
fn workspace_draft_protects_agent_update_and_whole_group_undo() {
    let (_root, store, conversation) = setup();
    let target = captured(&store, "目标");
    let run = begin(&store, &conversation, "更新目标");
    let db = Connection::open(store.database_path()).unwrap();
    db.execute(
        "INSERT INTO workspace_drafts(key,payload) VALUES(?,'{}')",
        [format!("memory:{}", target.memory_id)],
    )
    .unwrap();
    let request = MemoryWriteArgs {
        destination: Destination::Existing {
            memory_id: target.memory_id.clone(),
            expected_version: target.version_id,
        },
        title: "目标".into(),
        parts: vec![sourced_part("更新", &run.user_message_id, &run.input_text)],
    };
    let op = stage(
        &store,
        &run,
        "write_memory",
        &serde_json::to_value(&request).unwrap(),
    );
    assert_eq!(
        store
            .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &request)
            .unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(count(&store, "SELECT count(*) FROM captures"), 1);
    db.execute("DELETE FROM workspace_drafts", []).unwrap();
    store
        .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &request)
        .unwrap();
    db.execute(
        "INSERT INTO workspace_drafts(key,payload) VALUES(?,'{}')",
        [format!("memory:{}", target.memory_id)],
    )
    .unwrap();
    assert_eq!(
        store
            .undo_agent_input(&id(), &run.input_id)
            .unwrap()
            .conflicts,
        vec![target.memory_id]
    );
}

#[test]
fn persisted_summary_uses_same_conversation_sequence_and_undo_invalidates_it() {
    let (_root, store, conversation) = setup();
    let run = begin(&store, &conversation, "预算 5000，尚未决定上线");
    write(&store, &run, Destination::New, "预算 5000，尚未决定上线");
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
    let through = store.turn(&run.input_id).unwrap().assistant.seq;
    let next = begin(&store, &conversation, "Continue discussion");
    let revision = store
        .agent_conversation_context(&conversation)
        .unwrap()
        .receipt_revision;
    store
        .save_agent_summary(
            &next.input_id,
            &next.attempt_id,
            "预算 5000；上线未决",
            through,
            &revision,
        )
        .unwrap();
    let context = store.agent_conversation_context(&conversation).unwrap();
    assert_eq!(context.summary_through_seq, through);
    assert_eq!(context.summary, "预算 5000；上线未决");
    assert_eq!(
        store
            .save_agent_summary(
                &next.input_id,
                &next.attempt_id,
                "未来",
                through + 2,
                &revision
            )
            .unwrap_err(),
        DataError::Invalid
    );
    store.undo_agent_input(&id(), &run.input_id).unwrap();
    assert_eq!(
        store
            .agent_conversation_context(&conversation)
            .unwrap()
            .summary,
        ""
    );
}

#[test]
fn source_ids_cannot_inject_another_conversation_or_ai_text() {
    let (_root, store, conversation) = setup();
    let other = id();
    store.create_conversation(&other, "其他").unwrap();
    let foreign = begin(&store, &other, "不相关原话");
    let run = begin(&store, &conversation, "当前原话");
    for source in [foreign.user_message_id, run.assistant_message_id.clone()] {
        let request = MemoryWriteArgs {
            destination: Destination::New,
            title: "来源".into(),
            parts: vec![sourced_part("不能伪造", &source, "当前原话")],
        };
        let op = stage(
            &store,
            &run,
            "write_memory",
            &serde_json::to_value(&request).unwrap(),
        );
        assert_eq!(
            store
                .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &request)
                .unwrap_err(),
            DataError::SourceAttribution
        );
    }
    assert_eq!(count(&store, "SELECT count(*) FROM captures"), 0);
    assert_eq!(count(&store, "SELECT count(*) FROM memories"), 0);
}

#[test]
fn item_sources_archive_early_conditions_and_tentative_ideas_after_conversation_deletion() {
    let (_root, store, conversation) = setup();
    let conditions = begin(
        &store,
        &conversation,
        "每周8小时，预算4800元；不上传录音；是否收费尚未决定。",
    );
    store
        .finish_agent_input(&conditions.input_id, &conditions.attempt_id, &[])
        .unwrap();
    let ideas = begin(
        &store,
        &conversation,
        "可以比较访谈标注体验和后续验证问题，只是备选思路，尚未执行或决定。",
    );
    store
        .finish_agent_input(&ideas.input_id, &ideas.attempt_id, &[])
        .unwrap();
    let current = begin(
        &store,
        &conversation,
        "更正：预算3800元，其他条件不变。请记下这些条件和备选思路。",
    );
    let request = MemoryWriteArgs {
        destination: Destination::New,
        title: "访谈工具约束与备选思路".into(),
        parts: vec![
            sourced_part(
                "每周8小时；不上传录音；是否收费尚未决定。\n",
                &conditions.user_message_id,
                &conditions.input_text,
            ),
            sourced_part(
                "预算已更正为3800元。\n",
                &current.user_message_id,
                "更正：预算3800元，其他条件不变。",
            ),
            sourced_part(
                "比较访谈标注体验和后续验证问题只是备选思路，尚未执行或决定。",
                &ideas.user_message_id,
                &ideas.input_text,
            ),
        ],
    };
    let staged = stage(
        &store,
        &current,
        "write_memory",
        &serde_json::to_value(&request).unwrap(),
    );
    let committed = store
        .apply_agent_memory(
            &current.input_id,
            &current.attempt_id,
            &staged.operation_id,
            &request,
        )
        .unwrap();
    let receipt = committed.receipt.unwrap();
    let memory = receipt.memory_id.unwrap();
    store
        .finish_agent_input(&current.input_id, &current.attempt_id, &[])
        .unwrap();
    store.delete_conversation(&conversation).unwrap();
    let version = store.memory(&memory).unwrap().current;
    assert!(version.body.contains("预算已更正为3800元"));
    assert!(version.body.contains("尚未执行或决定"));
    assert_eq!(version.capture_ids.len(), 3);
    let mut sources = version
        .capture_ids
        .iter()
        .map(|id| store.capture_by_id(id).unwrap().text)
        .collect::<Vec<_>>();
    sources.sort();
    let mut expected = vec![conditions.input_text, ideas.input_text, current.input_text];
    expected.sort();
    assert_eq!(sources, expected);
    assert_eq!(count(&store, "SELECT count(*) FROM memories"), 1);
    assert_eq!(count(&store, "SELECT count(*) FROM organization_jobs"), 0);
    assert!(
        store
            .undo_agent_input(&id(), &current.input_id)
            .unwrap()
            .conflicts
            .is_empty()
    );
    assert_eq!(store.memory(&memory).unwrap_err(), DataError::Unavailable);
}

#[test]
fn unmatched_quotes_or_unattributed_new_parts_fail_before_any_source_is_archived() {
    let (_root, store, conversation) = setup();
    let run = begin(&store, &conversation, "每周8小时。预算3800元。");
    let valid = sourced_part("每周8小时。\n", &run.user_message_id, "每周8小时。");
    for invalid in [
        sourced_part("预算3800元。", &run.user_message_id, "预算5000元。"),
        MemoryWritePart {
            text: "预算3800元。".into(),
            sources: vec![],
        },
        sourced_part("预算3800元。", &run.user_message_id, " "),
    ] {
        let request = MemoryWriteArgs {
            destination: Destination::New,
            title: "约束".into(),
            parts: vec![valid.clone(), invalid],
        };
        let op = stage(
            &store,
            &run,
            "write_memory",
            &serde_json::to_value(&request).unwrap(),
        );
        assert_eq!(
            store
                .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &request)
                .unwrap_err(),
            DataError::SourceAttribution
        );
        assert_eq!(count(&store, "SELECT count(*) FROM captures"), 0);
        assert_eq!(count(&store, "SELECT count(*) FROM memories"), 0);
        assert!(
            store
                .agent_input_receipts(&run.input_id)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn inherited_parts_preserve_complete_lines_and_target_version_sources() {
    let (_root, store, conversation) = setup();
    let target = captured(
        &store,
        "不允许上传录音。\n去年因为每周只有2小时而暂停，并非缺预算。",
    );
    let run = begin(&store, &conversation, "现在每周有8小时，决定恢复。");
    let destination = Destination::Existing {
        memory_id: target.memory_id.clone(),
        expected_version: target.version_id.clone(),
    };
    let mut request = MemoryWriteArgs {
        destination,
        title: "访谈工具".into(),
        parts: vec![
            MemoryWritePart {
                text: "允许上传录音。\n".into(),
                sources: vec![],
            },
            sourced_part(
                "现在每周8小时，决定恢复。",
                &run.user_message_id,
                &run.input_text,
            ),
        ],
    };
    let bad = stage(
        &store,
        &run,
        "write_memory",
        &serde_json::to_value(&request).unwrap(),
    );
    assert_eq!(
        store
            .apply_agent_memory(&run.input_id, &run.attempt_id, &bad.operation_id, &request)
            .unwrap_err(),
        DataError::SourceAttribution
    );
    assert_eq!(
        store.memory(&target.memory_id).unwrap().current.id,
        target.version_id
    );
    assert_eq!(count(&store, "SELECT count(*) FROM captures"), 1);

    request.parts[0].text = "不允许上传录音。\n".into();
    request.parts.insert(
        1,
        sourced_part(
            "去年因每周仅2小时暂停，原因不是预算。\n",
            &target.version_id,
            "去年因为每周只有2小时而暂停，并非缺预算。",
        ),
    );
    let good = stage(
        &store,
        &run,
        "write_memory",
        &serde_json::to_value(&request).unwrap(),
    );
    store
        .apply_agent_memory(&run.input_id, &run.attempt_id, &good.operation_id, &request)
        .unwrap();
    let current = store.memory(&target.memory_id).unwrap().current;
    assert_eq!(
        current.body,
        "不允许上传录音。\n去年因每周仅2小时暂停，原因不是预算。\n现在每周8小时，决定恢复。"
    );
    assert!(current.capture_ids.contains(&target.capture_id));
    assert_eq!(current.capture_ids.len(), 2);
    assert_eq!(
        count(&store, "SELECT count(*) FROM captures"),
        2,
        "referencing a target version must not archive the AI-written body as a new user source"
    );
}

#[test]
fn current_head_and_keyword_index_change_in_the_write_transaction() {
    let (_root, store, conversation) = setup();
    let target = captured(&store, "原计划只做免费版本");
    let old = target.version_id.clone();
    let run = begin(&store, &conversation, "决定采用季度订阅");
    let op = write(
        &store,
        &run,
        Destination::Existing {
            memory_id: target.memory_id.clone(),
            expected_version: old.clone(),
        },
        "决定采用季度订阅，尚未上线",
    );
    assert_eq!(
        store.memory(&target.memory_id).unwrap().current.id,
        op.receipt.unwrap().after_version.unwrap()
    );
    let db = Connection::open(store.database_path()).unwrap();
    let found: i64 = db
        .query_row(
            "SELECT count(*) FROM record_fts WHERE body LIKE ?",
            ["%季度订阅%"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(found, 1);
    assert_eq!(
        store
            .resolve_source(&SourceRef::Version(old), 1000)
            .unwrap()
            .text,
        "原计划只做免费版本"
    );
}

#[test]
fn explicit_save_reuses_receipts_and_can_undo_membership_after_conversation_deletion() {
    let (_root, store, _) = setup();
    let collection = id();
    store
        .save_collection(&collection, "Collection", "", None)
        .unwrap();
    let conversation = id();
    store
        .create_scoped_conversation(&conversation, "Discussion", Some(&collection))
        .unwrap();
    let run = begin(&store, &conversation, "给我一个建议");
    store
        .append_agent_text(&run.input_id, &run.attempt_id, "可编辑的建议")
        .unwrap();
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
    let request = id();
    let saved = store
        .save_agent_text(
            &request,
            &run.input_id,
            "用户实际选择并编辑的文字",
            "手动保存",
            &Destination::New,
        )
        .unwrap();
    assert_eq!(
        store
            .save_agent_text(
                &request,
                &run.input_id,
                "用户实际选择并编辑的文字",
                "手动保存",
                &Destination::New
            )
            .unwrap(),
        saved
    );
    assert_eq!(
        store
            .memory(saved.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .actor,
        "user"
    );
    assert_eq!(count(&store, "SELECT count(*) FROM organization_jobs"), 0);
    store.delete_conversation(&conversation).unwrap();
    assert!(
        store
            .undo_agent_input(&id(), &request)
            .unwrap()
            .conflicts
            .is_empty()
    );
    assert_eq!(count(&store, "SELECT count(*) FROM collection_entries"), 0);
}

#[test]
fn undo_fences_an_inflight_producer_and_never_retries_its_reversed_writes() {
    let (_root, store, conversation) = setup();
    let run = begin(&store, &conversation, "记下这个想法");
    write(&store, &run, Destination::New, "Idea");
    let request = MemoryWriteArgs {
        destination: Destination::New,
        title: "第二条".into(),
        parts: vec![sourced_part(
            "迟到的第二条",
            &run.user_message_id,
            &run.input_text,
        )],
    };
    let pending = stage(
        &store,
        &run,
        "write_memory",
        &serde_json::to_value(&request).unwrap(),
    );
    store.undo_agent_input(&id(), &run.input_id).unwrap();
    assert_eq!(
        store.agent_execution(&run.input_id).unwrap().state,
        "cancelled"
    );
    assert_eq!(
        store
            .apply_agent_memory(
                &run.input_id,
                &run.attempt_id,
                &pending.operation_id,
                &request
            )
            .unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(
        store
            .begin_agent_input(
                &run.input_id,
                &id(),
                &conversation,
                &run.input_text,
                &[],
                None
            )
            .unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(
        count(&store, "SELECT count(*) FROM memories WHERE state='active'"),
        0
    );
}

#[test]
fn manual_membership_change_blocks_whole_undo_without_removing_that_change() {
    let (_root, store, conversation) = setup();
    let collection = id();
    store
        .save_collection(&collection, "后来加入", "", None)
        .unwrap();
    let run = begin(&store, &conversation, "New idea");
    let created = write(&store, &run, Destination::New, "New idea")
        .receipt
        .unwrap();
    let memory = created.memory_id.unwrap();
    let key = RecordKey {
        kind: "memory".into(),
        id: memory.clone(),
    };
    store.collect_record(&collection, &key, true).unwrap();
    let result = store.undo_agent_input(&id(), &run.input_id).unwrap();
    assert_eq!(result.conflicts, vec![memory.clone()]);
    assert!(result.receipt.is_none());
    assert_eq!(
        store.record_navigation(&key).unwrap().collections,
        vec![collection]
    );
    assert_eq!(store.memory(&memory).unwrap().state, "active");
}

#[test]
fn execution_requires_the_persisted_tool_boundary_and_preserves_it_for_retry() {
    let (_root, store, conversation) = setup();
    let run = begin(&store, &conversation, "查询记忆");
    assert_eq!(
        store
            .stage_agent_operation(
                &run.input_id,
                &run.attempt_id,
                "unpersisted",
                "search_memories",
                &json!({"query":"Plan"})
            )
            .unwrap_err(),
        DataError::Invalid
    );
    let op = stage(&store, &run, "search_memories", &json!({"query":"Plan"}));
    assert_eq!(
        store
            .checkpoint_agent(&run.input_id, &run.attempt_id, &[])
            .unwrap_err(),
        DataError::RequestConflict
    );
    assert_eq!(
        store
            .finish_agent_input(&run.input_id, &run.attempt_id, &[])
            .unwrap_err(),
        DataError::Conflict
    );
    store
        .complete_agent_operation(
            &run.input_id,
            &run.attempt_id,
            &op.operation_id,
            &json!({"memories":[]}),
        )
        .unwrap();
    assert!(
        store
            .finish_agent_input(&run.input_id, &run.attempt_id, &[])
            .is_ok()
    );
}

#[test]
fn optional_followups_cannot_downgrade_completion_or_accept_stale_attempts() {
    let (_root, store, conversation) = setup();
    let run = begin(&store, &conversation, "讨论新的想法");
    write(&store, &run, Destination::New, "New idea");
    store
        .append_agent_text(&run.input_id, &run.attempt_id, "完整回答")
        .unwrap();
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
    // No callback after a generation failure leaves all successful work intact.
    assert!(
        store
            .agent_execution(&run.input_id)
            .unwrap()
            .follow_ups
            .is_empty()
    );
    assert_eq!(
        store.agent_execution(&run.input_id).unwrap().state,
        "complete"
    );
    assert_eq!(store.agent_input_receipts(&run.input_id).unwrap().len(), 1);
    let suggestions = vec!["之前有哪些类似尝试？".into(), "还需要验证哪些条件？".into()];
    assert_eq!(
        store
            .save_agent_followups(&run.input_id, &id(), &suggestions)
            .unwrap_err(),
        DataError::Conflict
    );
    store
        .save_agent_followups(&run.input_id, &run.attempt_id, &suggestions)
        .unwrap();
    let complete = store.agent_execution(&run.input_id).unwrap();
    assert_eq!(complete.follow_ups, suggestions);
    assert_eq!(complete.text, "完整回答");
    assert_eq!(complete.state, "complete");
    store.undo_agent_input(&id(), &run.input_id).unwrap();
    assert_eq!(
        store
            .save_agent_followups(&run.input_id, &run.attempt_id, &suggestions)
            .unwrap_err(),
        DataError::Conflict
    );
}

#[test]
fn retry_restores_original_quick_source_and_material_fingerprint() {
    let (_root, store, conversation) = setup();
    let origin = Origin::User {
        app: "Browser".into(),
        project: Some("来源".into()),
        uri: Some("https://example.test".into()),
    };
    let focused = vec![id()];
    let run = store
        .begin_agent_input(
            &id(),
            &id(),
            &conversation,
            "精确原话",
            &focused,
            Some(&origin),
        )
        .unwrap();
    store
        .stop_agent_input(&run.input_id, &run.attempt_id, "failed", Some("network"))
        .unwrap();
    let resumed = store.retry_agent_input(&run.input_id, &id()).unwrap();
    assert_eq!(resumed.user_message_id, run.user_message_id);
    assert_eq!(resumed.focused_memory_ids, focused);
    let receipt = write(&store, &resumed, Destination::New, "New idea")
        .receipt
        .unwrap();
    assert!(
        matches!(store.capture_by_id(receipt.capture_id.as_ref().unwrap()).unwrap().origin,Origin::Discussion{app,uri,..} if app=="Browser" && uri.as_deref()==Some("https://example.test"))
    );
}

#[test]
fn late_archiving_preserves_when_the_user_originally_expressed_the_source() {
    let (_root, store, conversation) = setup();
    let earlier = begin(&store, &conversation, "去年讨论过暂缓收费");
    let original_time = 1_700_000_000_000_i64;
    Connection::open(store.database_path())
        .unwrap()
        .execute(
            "UPDATE messages SET created_at=?2 WHERE id=?1",
            rusqlite::params![earlier.user_message_id, original_time],
        )
        .unwrap();
    store
        .finish_agent_input(&earlier.input_id, &earlier.attempt_id, &[])
        .unwrap();
    let current = begin(&store, &conversation, "把刚才提到的原因记下来");
    let args = MemoryWriteArgs {
        destination: Destination::New,
        title: "收费计划".into(),
        parts: vec![sourced_part(
            "之前讨论过暂缓收费",
            &earlier.user_message_id,
            &earlier.input_text,
        )],
    };
    let staged = stage(
        &store,
        &current,
        "write_memory",
        &serde_json::to_value(&args).unwrap(),
    );
    let committed = store
        .apply_agent_memory(
            &current.input_id,
            &current.attempt_id,
            &staged.operation_id,
            &args,
        )
        .unwrap();
    let receipt = committed.receipt.unwrap();
    let capture = store
        .capture_by_id(receipt.capture_id.as_ref().unwrap())
        .unwrap();
    assert_eq!(capture.created_at, original_time);
    assert_eq!(capture.text, "去年讨论过暂缓收费");
    assert!(
        store
            .memory(receipt.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .created_at
            > capture.created_at
    );
}

#[test]
fn summary_compare_and_swap_rejects_pre_undo_model_output_without_stopping_new_input() {
    let (_root, store, conversation) = setup();
    let earlier = begin(&store, &conversation, "决定下个月收费");
    write(&store, &earlier, Destination::New, "决定下个月收费");
    store
        .finish_agent_input(&earlier.input_id, &earlier.attempt_id, &[])
        .unwrap();
    let through = store.turn(&earlier.input_id).unwrap().assistant.seq;
    let current = begin(&store, &conversation, "再考虑一下这个决定");
    let snapshot = store.agent_conversation_context(&conversation).unwrap();
    // The model is still generating from snapshot while the user undoes A.
    store.undo_agent_input(&id(), &earlier.input_id).unwrap();
    assert_eq!(
        store
            .save_agent_summary(
                &current.input_id,
                &current.attempt_id,
                "用户已决定收费",
                through,
                &snapshot.receipt_revision
            )
            .unwrap_err(),
        DataError::Conflict
    );
    let fresh = store.agent_conversation_context(&conversation).unwrap();
    assert_ne!(fresh.receipt_revision, snapshot.receipt_revision);
    assert!(fresh.summary.is_empty());
    assert_eq!(
        store.agent_execution(&current.input_id).unwrap().state,
        "processing"
    );
    store
        .save_agent_summary(
            &current.input_id,
            &current.attempt_id,
            "用户已经撤销此前的收费决定",
            through,
            &fresh.receipt_revision,
        )
        .unwrap();
    assert_eq!(
        store
            .agent_conversation_context(&conversation)
            .unwrap()
            .summary,
        "用户已经撤销此前的收费决定"
    );
}

#[test]
fn first_checkpoint_reloads_history_if_undo_invalidates_a_prepared_summary() {
    // Cover both a preexisting short summary (no model compression needed)
    // and undo immediately after the final compression CAS succeeded.
    for summary_already_saved in [true, false] {
        let (_root, store, conversation) = setup();
        let earlier = begin(&store, &conversation, "决定下个月收费");
        write(&store, &earlier, Destination::New, "决定下个月收费");
        store
            .finish_agent_input(&earlier.input_id, &earlier.attempt_id, &[])
            .unwrap();
        let through = store.turn(&earlier.input_id).unwrap().assistant.seq;
        let later = begin(&store, &conversation, "补充：收入目标仍待验证");
        store
            .finish_agent_input(&later.input_id, &later.attempt_id, &[])
            .unwrap();
        let current = begin(&store, &conversation, "继续评估");
        let mut snapshot = store.agent_history_snapshot(&current.input_id).unwrap();
        store
            .save_agent_summary(
                &current.input_id,
                &current.attempt_id,
                "决定收费；收入目标待验证",
                through,
                &snapshot.context.receipt_revision,
            )
            .unwrap();
        if summary_already_saved {
            snapshot = store.agent_history_snapshot(&current.input_id).unwrap();
        } else {
            // prepare_agent_history retains its own snapshot after its CAS.
            snapshot.context.summary = "决定收费；收入目标待验证".into();
            snapshot.context.summary_through_seq = through;
            snapshot
                .messages
                .retain(|message| message["seq"].as_i64().unwrap() > through);
        }
        assert_eq!(snapshot.messages.len(), 2);
        assert!(
            snapshot
                .messages
                .iter()
                .all(|message| message["logical_input_id"] == later.input_id)
        );

        store.undo_agent_input(&id(), &earlier.input_id).unwrap();
        let stale = vec![
            json!({"role":"user","content":json!({"earlier_summary":snapshot.context.summary,"recent_messages":snapshot.messages}).to_string()}),
        ];
        assert_eq!(
            store
                .checkpoint_agent_start(
                    &current.input_id,
                    &current.attempt_id,
                    &stale,
                    &snapshot.context.receipt_revision
                )
                .unwrap_err(),
            DataError::Conflict
        );
        let ongoing = store.agent_execution(&current.input_id).unwrap();
        assert_eq!(ongoing.state, "processing");
        assert!(ongoing.protocol.is_empty());

        let fresh = store.agent_history_snapshot(&current.input_id).unwrap();
        assert!(fresh.context.summary.is_empty());
        assert_eq!(fresh.context.summary_through_seq, 0);
        assert_eq!(fresh.messages.len(), 4);
        assert_eq!(fresh.messages[0]["text"], "决定下个月收费");
        assert_eq!(fresh.messages[0]["memory_changes_undone"], true);
        assert_eq!(fresh.messages[0]["receipts"][0]["status"], "undone");
        assert_eq!(fresh.messages[2]["text"], "补充：收入目标仍待验证");
        let rebuilt = vec![
            json!({"role":"user","content":json!({"earlier_summary":fresh.context.summary,"recent_messages":fresh.messages}).to_string()}),
        ];
        store
            .checkpoint_agent_start(
                &current.input_id,
                &current.attempt_id,
                &rebuilt,
                &fresh.context.receipt_revision,
            )
            .unwrap();
        assert_eq!(
            store.agent_execution(&current.input_id).unwrap().protocol,
            rebuilt
        );
        assert_eq!(store.messages(&conversation, 0, 100).unwrap().len(), 6);
    }
}

#[test]
fn initial_snapshot_cannot_replace_a_committed_prefix_or_cross_attempts() {
    let (_root, store, conversation) = setup();
    let run = begin(&store, &conversation, "分析想法");
    let snapshot = store.agent_history_snapshot(&run.input_id).unwrap();
    let messages = vec![json!({"role":"user","content":"分析想法"})];
    store
        .stop_agent_input(&run.input_id, &run.attempt_id, "failed", Some("network"))
        .unwrap();
    let resumed = store.retry_agent_input(&run.input_id, &id()).unwrap();
    assert_eq!(
        store
            .checkpoint_agent_start(
                &run.input_id,
                &run.attempt_id,
                &messages,
                &snapshot.context.receipt_revision
            )
            .unwrap_err(),
        DataError::Conflict
    );
    store
        .checkpoint_agent_start(
            &resumed.input_id,
            &resumed.attempt_id,
            &messages,
            &snapshot.context.receipt_revision,
        )
        .unwrap();
    assert_eq!(
        store
            .checkpoint_agent_start(
                &resumed.input_id,
                &resumed.attempt_id,
                &messages,
                &snapshot.context.receipt_revision
            )
            .unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(
        store.agent_execution(&resumed.input_id).unwrap().protocol,
        messages
    );
}

#[test]
fn a_memory_keeps_its_whole_change_group_handle_after_conversation_deletion() {
    let (_root, store, conversation) = setup();
    let target = captured(&store, "原计划");
    let run = begin(&store, &conversation, "调整计划并添加一个想法");
    write(
        &store,
        &run,
        Destination::Existing {
            memory_id: target.memory_id.clone(),
            expected_version: target.version_id,
        },
        "调整后的计划",
    );
    write(&store, &run, Destination::New, "另外一个想法");
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
    store.delete_conversation(&conversation).unwrap();
    let groups = store.memory_agent_changes(&target.memory_id).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].input_id, run.input_id);
    assert_eq!(groups[0].receipts.len(), 2);
    assert!(
        store
            .undo_agent_input(&id(), &groups[0].input_id)
            .unwrap()
            .conflicts
            .is_empty()
    );
    assert_eq!(
        store.memory(&target.memory_id).unwrap().current.body,
        "原计划"
    );
    assert!(
        store.memory_agent_changes(&target.memory_id).unwrap()[0]
            .receipts
            .iter()
            .all(|r| r.status == "undone")
    );
    let current = store.memory(&target.memory_id).unwrap().current.id;
    store.trash_memory(&target.memory_id, &current).unwrap();
    assert_eq!(
        store.memory_agent_changes(&target.memory_id).unwrap_err(),
        DataError::Unavailable
    );
}

#[test]
fn committed_write_result_contains_its_actual_versioned_body_evidence() {
    let (_root, store, conversation) = setup();
    let run = begin(&store, &conversation, "预算改成 8000 元");
    let op = write(
        &store,
        &run,
        Destination::New,
        "预算是 8000 元，尚未决定上线",
    );
    let result = op.result.unwrap();
    let receipt = op.receipt.unwrap();
    let evidence: Evidence = serde_json::from_value(result["evidence"].clone()).unwrap();
    assert_eq!(
        evidence.source,
        SourceRef::Version(receipt.after_version.clone().unwrap())
    );
    assert_eq!(evidence.text, "预算是 8000 元，尚未决定上线");
    assert_eq!(result["memory_id"].as_str(), receipt.memory_id.as_deref());
    assert!(
        result["citation_url"]
            .as_str()
            .unwrap()
            .contains(receipt.after_version.as_ref().unwrap())
    );
}

#[test]
fn manual_save_preserves_reviewed_partial_answers_from_terminal_states() {
    let (_root, store, conversation) = setup();
    for status in ["cancelled", "failed", "interrupted"] {
        let run = begin(&store, &conversation, "帮我想一下");
        store
            .append_agent_text(&run.input_id, &run.attempt_id, "已显示的一部分建议")
            .unwrap();
        let request = id();
        let draft = WorkspaceDraft {
            key: format!("save:{}", run.assistant_message_id),
            request_id: request.clone(),
            title: "手动保留".into(),
            body: "用户编辑后的片段".into(),
            expected_version: None,
            context: vec![],
            origin: None,
            destination: Some(Destination::New),
        };
        assert!(matches!(
            store.save_workspace_draft(&draft),
            Err(DataError::Unavailable)
        ));
        store
            .stop_agent_input(&run.input_id, &run.attempt_id, status, Some("synthetic"))
            .unwrap();
        store.save_workspace_draft(&draft).unwrap();
        let saved = store
            .save_agent_text(
                &request,
                &run.input_id,
                &draft.body,
                &draft.title,
                &Destination::New,
            )
            .unwrap();
        assert_eq!(
            store
                .memory(saved.memory_id.as_deref().unwrap())
                .unwrap()
                .current
                .body,
            draft.body
        );
        assert!(store.workspace_draft(&draft.key).unwrap().is_none());
        assert_eq!(
            store
                .save_agent_text(
                    &request,
                    &run.input_id,
                    &draft.body,
                    &draft.title,
                    &Destination::New
                )
                .unwrap()
                .request_id,
            saved.request_id
        );
    }
}
