use memivy_core::{
    memory::*,
    model::{AssistantContent, Message, ToolCall},
};
use rig_core::message::ToolFunction;
use rusqlite::Connection;
use serde_json::json;
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn capture(store: &MemoryStore, text: &str) -> CaptureResult {
    store
        .capture(&CaptureRequest {
            request_id: id(),
            text: text.into(),
            origin: Origin::User {
                app: "QA".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap()
}
fn key(memory: &str) -> RecordKey {
    RecordKey {
        kind: "memory".into(),
        id: memory.into(),
    }
}
fn prepare(
    store: &MemoryStore,
    target: &str,
    source: &str,
) -> (AgentExecution, AgentOperation, MemoryMergeArgs) {
    let target = store.memory(target).unwrap().current;
    let source = store.memory(source).unwrap().current;
    let conversation = id();
    store
        .create_conversation(&conversation, "Trip notes")
        .unwrap();
    let run = store
        .begin_agent_input(
            &id(),
            &id(),
            &conversation,
            "Combine these notes for the same trip",
            &[],
            None,
        )
        .unwrap();
    let args = MemoryMergeArgs {
        target_memory_id: target.memory_id,
        target_version: target.id.clone(),
        source_memory_id: source.memory_id,
        source_version: source.id.clone(),
        title: "Trip plan".into(),
        reason: "Two preparations for the same trip".into(),
        parts: vec![
            MemoryWritePart {
                text: format!("{}\n", target.body),
                sources: vec![MemorySourceQuote {
                    source_id: target.id,
                    quote: target.body,
                }],
            },
            MemoryWritePart {
                text: source.body.clone(),
                sources: vec![MemorySourceQuote {
                    source_id: source.id,
                    quote: source.body,
                }],
            },
        ],
    };
    let call = id();
    let protocol = vec![json!(Message::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(ToolCall::from_wire(
            &call,
            ToolFunction::new("merge_memories".into(), json!(args))
        ))]
    })];
    store
        .checkpoint_agent(&run.input_id, &run.attempt_id, &protocol)
        .unwrap();
    let operation = store
        .stage_agent_operation(
            &run.input_id,
            &run.attempt_id,
            &call,
            "merge_memories",
            &json!(args),
        )
        .unwrap();
    (run, operation, args)
}
fn merge(store: &MemoryStore, target: &str, source: &str) -> (AgentExecution, Receipt) {
    let (run, op, args) = prepare(store, target, source);
    let result = store
        .merge_agent_memories(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap();
    assert_eq!(
        store
            .merge_agent_memories(&run.input_id, &run.attempt_id, &op.operation_id, &args)
            .unwrap()
            .receipt,
        result.receipt
    );
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
    (run, result.receipt.unwrap())
}

fn append_original(store: &MemoryStore, memory: &str, text: &str) -> Receipt {
    let conversation = id();
    store
        .create_conversation(&conversation, "Selected text")
        .unwrap();
    let run = store
        .begin_agent_input(&id(), &id(), &conversation, "Review this text", &[], None)
        .unwrap();
    store
        .append_agent_text(&run.input_id, &run.attempt_id, text)
        .unwrap();
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
    let current = store.memory(memory).unwrap().current;
    store
        .save_agent_text(
            &id(),
            &run.input_id,
            text,
            "Reviewed note",
            &Destination::Existing {
                memory_id: memory.into(),
                expected_version: current.id,
            },
        )
        .unwrap()
}

fn share_original(store: &MemoryStore, original: &str, destination: Destination) -> Receipt {
    let original = store.capture_by_id(original).unwrap();
    let conversation = id();
    store
        .create_conversation(&conversation, "Shared original")
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
        initial_collections: vec![],
        destination,
        title: "Shared original".into(),
        parts: vec![MemoryWritePart {
            text: original.text.clone(),
            sources: vec![MemorySourceQuote {
                source_id: original.id,
                quote: original.text,
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
    let result = store
        .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap();
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
    result.receipt.unwrap()
}

fn restore_initial(store: &MemoryStore, saved: &CaptureResult) {
    let current = store.memory(&saved.memory_id).unwrap().current;
    store
        .restore_version(&id(), &saved.memory_id, &current.id, &saved.version_id)
        .unwrap();
}

fn original_state(store: &MemoryStore, capture: &str) -> (Option<String>, String, Option<String>) {
    Connection::open(store.database_path()).unwrap().query_row(
        "SELECT c.text,s.availability,s.trash_owner FROM captures c JOIN capture_state s ON s.capture_id=c.id WHERE c.id=?",
        [capture], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).unwrap()
}
#[test]
fn merge_inherits_navigation_and_both_undo_paths_restore_each_memory() {
    for grouped in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let target = capture(&store, "Shanghai trip: bring the adapter.");
        let source = capture(&store, "Shanghai trip: keep the original ticket.");
        let first = id();
        let second = id();
        store.save_collection(&first, "Travel", "", None).unwrap();
        store
            .save_collection(&second, "Preparation", "", None)
            .unwrap();
        store
            .collect_record(&first, &key(&target.memory_id), true)
            .unwrap();
        store
            .collect_record(&second, &key(&source.memory_id), true)
            .unwrap();
        store.pin_record(&key(&source.memory_id), true).unwrap();
        let (run, receipt) = merge(&store, &target.memory_id, &source.memory_id);
        let nav = store.record_navigation(&key(&target.memory_id)).unwrap();
        assert!(nav.pinned);
        assert_eq!(nav.collections.len(), 2);
        assert!(store.memory(&source.memory_id).is_err());
        assert_eq!(
            store
                .search(&SearchRequest::text("Shanghai", 8))
                .unwrap()
                .items
                .len(),
            1
        );
        let historical = store
            .resolve_source(&SourceRef::Version(source.version_id.clone()), 3000)
            .unwrap();
        assert!(historical.text.contains("original ticket"));
        assert!(!historical.current);
        if grouped {
            assert!(
                store
                    .undo_agent_input(&id(), &run.input_id)
                    .unwrap()
                    .conflicts
                    .is_empty()
            );
        } else {
            store.undo(&id(), &receipt.request_id).unwrap();
        }
        let nav = store.record_navigation(&key(&target.memory_id)).unwrap();
        assert!(!nav.pinned);
        assert_eq!(nav.collections, vec![first]);
        let nav = store.record_navigation(&key(&source.memory_id)).unwrap();
        assert!(nav.pinned);
        assert_eq!(nav.collections, vec![second]);
        assert_eq!(
            store.memory(&target.memory_id).unwrap().current.body,
            "Shanghai trip: bring the adapter."
        );
        assert_eq!(
            store.memory(&source.memory_id).unwrap().current.body,
            "Shanghai trip: keep the original ticket."
        );
        store.check_integrity().unwrap();
    }
}
#[test]
fn undo_conflicts_preserve_later_navigation_and_both_committed_heads() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let a = capture(&store, "Trip preparation A");
    let b = capture(&store, "Trip preparation B");
    let (run, receipt) = merge(&store, &a.memory_id, &b.memory_id);
    store.pin_record(&key(&a.memory_id), true).unwrap();
    assert_eq!(
        store.undo(&id(), &receipt.request_id),
        Err(DataError::Conflict)
    );
    assert_eq!(
        store
            .undo_agent_input(&id(), &run.input_id)
            .unwrap()
            .conflicts,
        vec![a.memory_id.clone()]
    );
    assert_eq!(
        store.memory(&a.memory_id).unwrap().current.id,
        receipt.after_version.unwrap()
    );
    assert!(store.memory(&b.memory_id).is_err());
}
#[test]
fn stale_heads_and_drafts_reject_the_entire_merge() {
    for draft in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let a = capture(&store, "Trip preparation A");
        let b = capture(&store, "Trip preparation B");
        let (run, op, args) = prepare(&store, &a.memory_id, &b.memory_id);
        if draft {
            store
                .save_workspace_draft(&WorkspaceDraft {
                    key: format!("memory:{}", b.memory_id),
                    title: "Draft".into(),
                    body: "Unsaved words".into(),
                    expected_version: Some(b.version_id.clone()),
                    request_id: id(),
                    destination: None,
                    origin: None,
                    context: vec![],
                })
                .unwrap();
        } else {
            store
                .edit_memory(&EditRequest {
                    request_id: id(),
                    memory_id: b.memory_id.clone(),
                    expected_version: b.version_id.clone(),
                    title: "Changed".into(),
                    body: "Different trip".into(),
                })
                .unwrap();
        }
        assert_eq!(
            store
                .merge_agent_memories(&run.input_id, &run.attempt_id, &op.operation_id, &args)
                .unwrap_err(),
            DataError::Conflict
        );
        assert_eq!(store.memory(&a.memory_id).unwrap().current.id, a.version_id);
        assert!(store.memory(&b.memory_id).is_ok());
        assert!(
            store
                .agent_input_receipts(&run.input_id)
                .unwrap()
                .is_empty()
        );
    }
}
#[test]
fn late_operation_failure_rolls_back_versions_receipts_navigation_and_search() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let a = capture(&store, "Trip preparation A");
    let b = capture(&store, "Trip preparation B");
    let (run, op, args) = prepare(&store, &a.memory_id, &b.memory_id);
    let db = Connection::open(store.database_path()).unwrap();
    db.execute_batch("CREATE TRIGGER inject_late_failure BEFORE UPDATE ON agent_operations WHEN NEW.result IS NOT NULL BEGIN SELECT RAISE(ABORT,'synthetic_api_key_never_export_47291'); END;").unwrap();
    let error = store
        .merge_agent_memories(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap_err();
    assert_eq!(error, DataError::Database);
    assert!(!format!("{error:?} {error}").contains("synthetic_api_key_never_export_47291"));
    assert_eq!(store.memory(&a.memory_id).unwrap().current.id, a.version_id);
    assert_eq!(store.memory(&b.memory_id).unwrap().current.id, b.version_id);
    assert_eq!(
        store
            .search(&SearchRequest::text("Trip", 8))
            .unwrap()
            .items
            .len(),
        2
    );
    assert!(
        store
            .agent_input_receipts(&run.input_id)
            .unwrap()
            .is_empty()
    );
    db.execute_batch("DROP TRIGGER inject_late_failure")
        .unwrap();
    store
        .merge_agent_memories(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap();
    store.check_integrity().unwrap();
}
#[test]
fn purge_follows_transitive_effective_merges_and_preserves_shared_sources() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let b = capture(&store, "Trip preparation B");
    let a = capture(&store, "Trip preparation A");
    let c = capture(&store, "Trip preparation C");
    let shared = share_original(&store, &b.capture_id, Destination::New);
    merge(&store, &a.memory_id, &b.memory_id);
    merge(&store, &c.memory_id, &a.memory_id);
    assert!(
        store
            .resolve_source(&SourceRef::Version(b.version_id.clone()), 3000)
            .is_ok()
    );
    let current = store.memory(&c.memory_id).unwrap().current;
    store.trash_memory(&c.memory_id, &current.id).unwrap();
    assert!(matches!(
        store.resolve_source(&SourceRef::Version(b.version_id.clone()), 3000),
        Err(DataError::Unavailable)
    ));
    store.purge_memory(&c.memory_id).unwrap();
    let db = Connection::open(store.database_path()).unwrap();
    for memory in [&a.memory_id, &b.memory_id, &c.memory_id] {
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM memory_versions WHERE memory_id=? AND body IS NOT NULL",
                [memory],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
    assert!(store.memory(shared.memory_id.as_ref().unwrap()).is_ok());
    assert_eq!(
        store.capture_by_id(&b.capture_id).unwrap().text,
        "Trip preparation B"
    );
    store.check_integrity().unwrap();
}

#[test]
fn historical_merge_originals_follow_trash_restore_and_purge_without_crossing_source_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let b = capture(&store, "Trip B");
    let a = capture(&store, "Trip A");
    let c = capture(&store, "Trip C");
    let exclusive = append_original(&store, &b.memory_id, "Historical original to erase")
        .capture_id
        .unwrap();
    let shared = append_original(&store, &b.memory_id, "Shared historical original")
        .capture_id
        .unwrap();
    let survivor = share_original(&store, &shared, Destination::New);
    restore_initial(&store, &b);
    // An undone merge must not drag its restored source into a later purge.
    let restored = capture(&store, "Separately restored memory");
    let (_, old_merge) = merge(&store, &b.memory_id, &restored.memory_id);
    store.undo(&id(), &old_merge.request_id).unwrap();
    merge(&store, &a.memory_id, &b.memory_id);
    merge(&store, &c.memory_id, &a.memory_id);
    let head = store.memory(&c.memory_id).unwrap().current.id;
    store.trash_memory(&c.memory_id, &head).unwrap();
    assert_eq!(
        original_state(&store, &exclusive),
        (
            Some("Historical original to erase".into()),
            "trashed".into(),
            Some(c.memory_id.clone())
        )
    );
    assert!(store.capture_by_id(&shared).is_ok());
    store.restore_memory(&c.memory_id).unwrap();
    assert_eq!(
        store.capture_by_id(&exclusive).unwrap().text,
        "Historical original to erase"
    );
    store.trash_memory(&c.memory_id, &head).unwrap();
    store.purge_memory(&c.memory_id).unwrap();
    assert_eq!(
        original_state(&store, &exclusive),
        (None, "purged".into(), None)
    );
    assert_eq!(
        store.capture_by_id(&shared).unwrap().text,
        "Shared historical original"
    );
    assert!(store.memory(survivor.memory_id.as_ref().unwrap()).is_ok());
    assert!(store.memory(&restored.memory_id).is_ok());
    assert!(store.capture_by_id(&restored.capture_id).is_ok());
    store.check_integrity().unwrap();
}

#[test]
fn shared_historical_original_transfers_to_the_other_merge_owner_until_its_purge() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let source = capture(&store, "First source");
    let owner = capture(&store, "First owner");
    let other = capture(&store, "Other source");
    let other_owner = capture(&store, "Other owner");
    let original = append_original(
        &store,
        &source.memory_id,
        "Shared only through hidden history",
    )
    .capture_id
    .unwrap();
    share_original(
        &store,
        &original,
        Destination::Existing {
            memory_id: other.memory_id.clone(),
            expected_version: other.version_id.clone(),
        },
    );
    restore_initial(&store, &source);
    restore_initial(&store, &other);
    merge(&store, &owner.memory_id, &source.memory_id);
    merge(&store, &other_owner.memory_id, &other.memory_id);
    let head = store.memory(&owner.memory_id).unwrap().current.id;
    store.trash_memory(&owner.memory_id, &head).unwrap();
    assert!(
        store.capture_by_id(&original).is_ok(),
        "The other active merge owner still owns the historical source"
    );
    let other_head = store.memory(&other_owner.memory_id).unwrap().current.id;
    store
        .trash_memory(&other_owner.memory_id, &other_head)
        .unwrap();
    store.purge_memory(&other_owner.memory_id).unwrap();
    assert_eq!(
        original_state(&store, &original),
        (
            Some("Shared only through hidden history".into()),
            "trashed".into(),
            Some(owner.memory_id.clone())
        )
    );
    store.restore_memory(&owner.memory_id).unwrap();
    assert!(store.capture_by_id(&original).is_ok());
    store.trash_memory(&owner.memory_id, &head).unwrap();
    store.purge_memory(&owner.memory_id).unwrap();
    assert_eq!(
        original_state(&store, &original),
        (None, "purged".into(), None)
    );
    store.check_integrity().unwrap();
}

#[test]
fn single_merge_undo_clears_compacted_context_and_fences_pending_operations() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let target = capture(&store, "Target note");
    let source = capture(&store, "Source note");
    let (run, op, args) = prepare(&store, &target.memory_id, &source.memory_id);
    let receipt = store
        .merge_agent_memories(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap()
        .receipt
        .unwrap();
    let db = Connection::open(store.database_path()).unwrap();
    db.execute("UPDATE conversations SET summary='These notes were combined',summary_through_seq=1 WHERE id=?", [&run.conversation_id]).unwrap();
    let request = id();
    let undone = store.undo(&request, &receipt.request_id).unwrap();
    assert_eq!(store.undo(&request, &receipt.request_id).unwrap(), undone);
    let context = store
        .agent_conversation_context(&run.conversation_id)
        .unwrap();
    assert_eq!(
        (context.summary.as_str(), context.summary_through_seq),
        ("", 0)
    );
    let execution = store.agent_execution(&run.input_id).unwrap();
    assert_eq!(execution.state, "cancelled");
    assert!(execution.attempt_id.is_empty());
    assert_eq!(
        store
            .merge_agent_memories(&run.input_id, &run.attempt_id, &op.operation_id, &args)
            .unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(
        store.retry_agent_input(&run.input_id, &id()).unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(
        store.memory(&target.memory_id).unwrap().current.body,
        "Target note"
    );
    assert_eq!(
        store.memory(&source.memory_id).unwrap().current.body,
        "Source note"
    );
    store.check_integrity().unwrap();
}

#[test]
fn purging_either_memory_erases_the_merge_reason_even_after_undo() {
    for purge_source in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let target = capture(&store, "Target note");
        let source = capture(&store, "Source note");
        let (_, receipt) = merge(&store, &target.memory_id, &source.memory_id);
        store.undo(&id(), &receipt.request_id).unwrap();
        let memory = if purge_source {
            &source.memory_id
        } else {
            &target.memory_id
        };
        let current = store.memory(memory).unwrap().current;
        store.trash_memory(memory, &current.id).unwrap();
        assert!(store.receipt(&receipt.request_id).unwrap().reason.is_some());
        store.purge_memory(memory).unwrap();
        assert!(store.receipt(&receipt.request_id).unwrap().reason.is_none());
        let survivor = if purge_source {
            &target.memory_id
        } else {
            &source.memory_id
        };
        assert!(store.memory(survivor).is_ok());
        assert!(
            store
                .memory_receipts(&key(survivor))
                .unwrap()
                .iter()
                .all(|r| r.receipt.reason.is_none())
        );
        store.check_integrity().unwrap();
    }
}
