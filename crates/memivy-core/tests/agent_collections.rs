use memivy_core::memory::*;
use memivy_core::model::{AssistantContent, Message, ToolCall};
use rig_core::message::ToolFunction;
use rusqlite::Connection;
use serde::Serialize;
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
        .create_conversation(&conversation, "Collection operations")
        .unwrap();
    (root, store, conversation)
}
fn begin(store: &MemoryStore, conversation: &str) -> AgentExecution {
    store
        .begin_agent_input(
            &id(),
            &id(),
            conversation,
            "我决定本周准备访谈。",
            &[],
            None,
        )
        .unwrap()
}
fn stage(
    store: &MemoryStore,
    run: &AgentExecution,
    name: &str,
    args: &impl Serialize,
) -> AgentOperation {
    let args = serde_json::to_value(args).unwrap();
    let call = id();
    let mut protocol = store.agent_execution(&run.input_id).unwrap().protocol;
    protocol.push(json!(Message::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(ToolCall::from_wire(
            &call,
            ToolFunction::new(name.into(), args.clone())
        ))]
    }));
    store
        .checkpoint_agent(&run.input_id, &run.attempt_id, &protocol)
        .unwrap();
    store
        .stage_agent_operation(&run.input_id, &run.attempt_id, &call, name, &args)
        .unwrap()
}
fn memory(store: &MemoryStore, text: &str) -> CaptureResult {
    store
        .capture(&CaptureRequest {
            request_id: id(),
            text: text.into(),
            origin: Origin::User {
                app: "Synthetic test".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap()
}
fn collection(store: &MemoryStore, name: &str) -> String {
    let collection = id();
    store.save_collection(&collection, name, "", None).unwrap();
    collection
}
fn revision(store: &MemoryStore, collection: &str) -> i64 {
    store.read_agent_collection(collection).unwrap().revision
}
fn memberships(store: &MemoryStore, memory: &str) -> Vec<String> {
    store
        .record_navigation(&RecordKey {
            kind: "memory".into(),
            id: memory.into(),
        })
        .unwrap()
        .collections
}
fn count(store: &MemoryStore, table: &str) -> i64 {
    Connection::open(store.database_path())
        .unwrap()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn edit(
    store: &MemoryStore,
    collection: &str,
    add: &[String],
    remove: &[String],
) -> CollectionMemberEdit {
    CollectionMemberEdit {
        collection_id: collection.into(),
        expected_revision: revision(store, collection),
        add_memory_ids: add.to_vec(),
        remove_memory_ids: remove.to_vec(),
    }
}
fn apply_members(
    store: &MemoryStore,
    run: &AgentExecution,
    changes: Vec<CollectionMemberEdit>,
) -> AgentOperation {
    let args = CollectionMembersArgs { changes };
    let op = stage(store, run, "update_collection_members", &args);
    store
        .update_agent_collection_members(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap()
}
fn new_memory_args(
    run: &AgentExecution,
    initial_collections: Vec<CollectionRef>,
) -> MemoryWriteArgs {
    MemoryWriteArgs {
        destination: Destination::New,
        title: "Interview preparation".into(),
        parts: vec![MemoryWritePart {
            text: run.input_text.clone(),
            sources: vec![MemorySourceQuote {
                source_id: run.user_message_id.clone(),
                quote: run.input_text.clone(),
            }],
        }],
        initial_collections,
    }
}

#[test]
fn collection_directory_resolves_names_memberships_and_pagination_without_mutations() {
    let (_root, store, _) = setup();
    let target = memory(&store, "Shared interview constraints");
    let names = ["松果计划", "松果复盘", "空专题", "Travel"];
    let ids: Vec<_> = names.iter().map(|name| collection(&store, name)).collect();
    for c in &ids[..2] {
        store
            .collect_record(
                c,
                &RecordKey {
                    kind: "memory".into(),
                    id: target.memory_id.clone(),
                },
                true,
            )
            .unwrap();
    }
    let before = (count(&store, "receipts"), count(&store, "memory_versions"));
    let mut all = vec![];
    let mut offset = 0;
    loop {
        let page = store
            .list_agent_collections(&CollectionListArgs {
                query: None,
                memory_id: None,
                offset,
                limit: 2,
            })
            .unwrap();
        assert_eq!(page.total, 4);
        all.extend(page.items.into_iter().map(|c| c.id));
        match page.next_offset {
            Some(next) => offset = next,
            None => break,
        }
    }
    all.sort();
    let mut expected = ids.clone();
    expected.sort();
    assert_eq!(all, expected);
    let matched = store
        .list_agent_collections(&CollectionListArgs {
            query: Some("松果".into()),
            memory_id: Some(target.memory_id.clone()),
            offset: 0,
            limit: 20,
        })
        .unwrap();
    assert_eq!(matched.total, 2);
    assert!(matched.items.iter().all(|c| c.count == 1));
    assert_eq!(store.read_agent_collection(&ids[2]).unwrap().count, 0);
    assert_eq!(
        store.read_agent_collection(&id()).unwrap_err(),
        DataError::Unavailable
    );
    let absent = store
        .list_agent_collections(&CollectionListArgs {
            query: Some("不存在的名字".into()),
            memory_id: None,
            offset: 0,
            limit: 20,
        })
        .unwrap();
    assert_eq!(absent.total, 0);
    store
        .archive_collection(&ids[0], true, revision(&store, &ids[0]))
        .unwrap();
    assert_eq!(
        store.read_agent_collection(&ids[0]).unwrap_err(),
        DataError::Unavailable
    );
    assert_eq!(
        store
            .list_agent_collections(&CollectionListArgs {
                query: None,
                memory_id: Some(id()),
                offset: 0,
                limit: 20
            })
            .unwrap_err(),
        DataError::Unavailable
    );
    assert_eq!(
        (count(&store, "receipts"), count(&store, "memory_versions")),
        before
    );
}

#[test]
fn create_with_initial_members_is_atomic_idempotent_and_undoable() {
    let (_root, store, conversation) = setup();
    let first = memory(&store, "First source");
    let second = memory(&store, "Second source");
    let run = begin(&store, &conversation);
    let args = CollectionCreateArgs {
        name: "执行准备".into(),
        description: "开始前核对".into(),
        initial_memory_ids: vec![first.memory_id.clone(), second.memory_id.clone()],
    };
    let op = stage(&store, &run, "create_collection", &args);
    let saved = store
        .create_agent_collection(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap();
    assert_eq!(saved.receipt.as_ref().unwrap().collection_changes.len(), 1);
    let created = store.collections().unwrap().pop().unwrap();
    assert_eq!(created.name, args.name);
    assert_eq!(created.description, args.description);
    assert_eq!(created.count, 2);
    let replay = store
        .create_agent_collection(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap();
    assert_eq!(replay.result, saved.result);
    assert_eq!(store.collections().unwrap().len(), 1);
    assert_eq!(count(&store, "memory_versions"), 2);
    let undone = store.undo_agent_input(&id(), &run.input_id).unwrap();
    assert!(undone.conflicts.is_empty() && undone.collection_conflicts.is_empty());
    assert!(store.collections().unwrap().is_empty());
    assert!(memberships(&store, &first.memory_id).is_empty());
    assert_eq!(
        store.memory(&second.memory_id).unwrap().current.body,
        "Second source"
    );
}

#[test]
fn invalid_initial_member_rolls_back_collection_and_receipt() {
    let (_root, store, conversation) = setup();
    let first = memory(&store, "Existing memory");
    let run = begin(&store, &conversation);
    let before = count(&store, "receipts");
    let args = CollectionCreateArgs {
        name: "Invalid batch".into(),
        description: "".into(),
        initial_memory_ids: vec![first.memory_id.clone(), id()],
    };
    let op = stage(&store, &run, "create_collection", &args);
    assert_eq!(
        store
            .create_agent_collection(&run.input_id, &run.attempt_id, &op.operation_id, &args)
            .unwrap_err(),
        DataError::Unavailable
    );
    assert!(store.collections().unwrap().is_empty());
    assert!(memberships(&store, &first.memory_id).is_empty());
    assert_eq!(count(&store, "receipts"), before);
}

#[test]
fn membership_move_preserves_other_collections_and_never_versions_body() {
    let (_root, store, conversation) = setup();
    let memory = memory(&store, "The original evidence");
    let first = collection(&store, "First");
    let second = collection(&store, "Second");
    let third = collection(&store, "Third");
    let key = RecordKey {
        kind: "memory".into(),
        id: memory.memory_id.clone(),
    };
    store.collect_record(&first, &key, true).unwrap();
    store.collect_record(&third, &key, true).unwrap();
    let run = begin(&store, &conversation);
    let before_versions = count(&store, "memory_versions");
    let op = apply_members(
        &store,
        &run,
        vec![
            edit(&store, &first, &[], std::slice::from_ref(&memory.memory_id)),
            edit(
                &store,
                &second,
                std::slice::from_ref(&memory.memory_id),
                &[],
            ),
        ],
    );
    let mut expected = vec![second.clone(), third.clone()];
    expected.sort();
    assert_eq!(memberships(&store, &memory.memory_id), expected);
    assert_eq!(op.receipt.unwrap().collection_changes.len(), 2);
    assert_eq!(
        store.memory(&memory.memory_id).unwrap().current.id,
        memory.version_id
    );
    assert_eq!(count(&store, "memory_versions"), before_versions);
    let undo = store.undo_agent_input(&id(), &run.input_id).unwrap();
    assert!(undo.conflicts.is_empty() && undo.collection_conflicts.is_empty());
    let mut expected = vec![first, third];
    expected.sort();
    assert_eq!(memberships(&store, &memory.memory_id), expected);
    assert_eq!(count(&store, "memory_versions"), before_versions);
}

#[test]
fn invalid_member_in_one_change_prevents_every_change_in_batch() {
    let (_root, store, conversation) = setup();
    let m = memory(&store, "Evidence");
    let a = collection(&store, "A");
    let b = collection(&store, "B");
    store
        .collect_record(
            &a,
            &RecordKey {
                kind: "memory".into(),
                id: m.memory_id.clone(),
            },
            true,
        )
        .unwrap();
    let versions = (revision(&store, &a), revision(&store, &b));
    let run = begin(&store, &conversation);
    let args = CollectionMembersArgs {
        changes: vec![
            edit(&store, &a, &[], std::slice::from_ref(&m.memory_id)),
            edit(&store, &b, &[m.memory_id.clone(), id()], &[]),
        ],
    };
    let op = stage(&store, &run, "update_collection_members", &args);
    assert_eq!(
        store
            .update_agent_collection_members(
                &run.input_id,
                &run.attempt_id,
                &op.operation_id,
                &args
            )
            .unwrap_err(),
        DataError::Unavailable
    );
    assert_eq!(memberships(&store, &m.memory_id), vec![a.clone()]);
    assert_eq!((revision(&store, &a), revision(&store, &b)), versions);
    assert!(
        store
            .agent_input_receipts(&run.input_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn member_noop_and_replay_do_not_issue_receipts_or_advance_revision() {
    let (_root, store, conversation) = setup();
    let m = memory(&store, "Evidence");
    let c = collection(&store, "A");
    let key = RecordKey {
        kind: "memory".into(),
        id: m.memory_id.clone(),
    };
    store.collect_record(&c, &key, true).unwrap();
    let before = revision(&store, &c);
    let run = begin(&store, &conversation);
    let args = CollectionMembersArgs {
        changes: vec![edit(&store, &c, std::slice::from_ref(&m.memory_id), &[])],
    };
    let op = stage(&store, &run, "update_collection_members", &args);
    let result = store
        .update_agent_collection_members(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap();
    assert_eq!(result.result.as_ref().unwrap()["changed"], false);
    assert!(result.receipt.is_none());
    assert_eq!(revision(&store, &c), before);
    assert!(
        store
            .agent_input_receipts(&run.input_id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .update_agent_collection_members(
                &run.input_id,
                &run.attempt_id,
                &op.operation_id,
                &args
            )
            .unwrap()
            .result,
        result.result
    );
}

#[test]
fn stale_revision_detects_manual_membership_changes_and_undo_preserves_later_edits() {
    let (_root, store, conversation) = setup();
    let first = memory(&store, "First");
    let second = memory(&store, "Second");
    let c = collection(&store, "Plan");
    let run = begin(&store, &conversation);
    let stale = CollectionUpdateArgs {
        collection_id: c.clone(),
        expected_revision: revision(&store, &c),
        name: "Stale title".into(),
        description: "".into(),
    };
    store
        .collect_record(
            &c,
            &RecordKey {
                kind: "memory".into(),
                id: first.memory_id.clone(),
            },
            true,
        )
        .unwrap();
    let op = stage(&store, &run, "update_collection", &stale);
    assert_eq!(
        store
            .update_agent_collection(&run.input_id, &run.attempt_id, &op.operation_id, &stale)
            .unwrap_err(),
        DataError::Conflict
    );
    let update = CollectionUpdateArgs {
        collection_id: c.clone(),
        expected_revision: revision(&store, &c),
        name: "Renamed".into(),
        description: "Approved description".into(),
    };
    let op = stage(&store, &run, "update_collection", &update);
    store
        .update_agent_collection(&run.input_id, &run.attempt_id, &op.operation_id, &update)
        .unwrap();
    store
        .collect_record(
            &c,
            &RecordKey {
                kind: "memory".into(),
                id: second.memory_id.clone(),
            },
            true,
        )
        .unwrap();
    let undone = store.undo_agent_input(&id(), &run.input_id).unwrap();
    assert_eq!(undone.collection_conflicts, vec![c.clone()]);
    let current = store.read_agent_collection(&c).unwrap();
    assert_eq!(current.name, "Renamed");
    assert_eq!(current.count, 2);
}

#[test]
fn new_memory_initial_membership_is_atomic_and_current_topic_never_implies_membership() {
    let (_root, store, conversation) = setup();
    let c = collection(&store, "松果计划");
    let scoped = id();
    store
        .create_scoped_conversation(&scoped, "Topic", Some(&c))
        .unwrap();
    let run = begin(&store, &scoped);
    let plain = new_memory_args(&run, vec![]);
    let op = stage(&store, &run, "write_memory", &plain);
    let saved = store
        .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &plain)
        .unwrap();
    assert!(memberships(&store, saved.receipt.unwrap().memory_id.as_ref().unwrap()).is_empty());
    let run = begin(&store, &conversation);
    let args = new_memory_args(
        &run,
        vec![CollectionRef {
            id: c.clone(),
            revision: revision(&store, &c),
        }],
    );
    let op = stage(&store, &run, "write_memory", &args);
    let saved = store
        .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap();
    let receipt = saved.receipt.unwrap();
    assert_eq!(
        memberships(&store, receipt.memory_id.as_ref().unwrap()),
        vec![c]
    );
    assert_eq!(receipt.collection_changes.len(), 1);
    let before = (
        count(&store, "memories"),
        count(&store, "memory_versions"),
        count(&store, "receipts"),
    );
    let invalid = new_memory_args(
        &run,
        vec![CollectionRef {
            id: id(),
            revision: 1,
        }],
    );
    let op = stage(&store, &run, "write_memory", &invalid);
    assert_eq!(
        store
            .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &invalid)
            .unwrap_err(),
        DataError::Unavailable
    );
    assert_eq!(
        (
            count(&store, "memories"),
            count(&store, "memory_versions"),
            count(&store, "receipts")
        ),
        before
    );
}

#[test]
fn initial_memberships_are_rejected_for_existing_memory_writes() {
    let (_root, store, conversation) = setup();
    let m = memory(&store, "Existing evidence");
    let c = collection(&store, "Plan");
    let run = begin(&store, &conversation);
    let mut args = new_memory_args(&run, vec![CollectionRef { id: c, revision: 1 }]);
    args.destination = Destination::Existing {
        memory_id: m.memory_id.clone(),
        expected_version: m.version_id.clone(),
    };
    let op = stage(&store, &run, "write_memory", &args);
    assert_eq!(
        store
            .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &args)
            .unwrap_err(),
        DataError::Invalid
    );
    assert_eq!(store.memory(&m.memory_id).unwrap().current.id, m.version_id);
    assert!(memberships(&store, &m.memory_id).is_empty());
}

#[test]
fn cancelled_or_superseded_attempts_cannot_apply_collection_mutations() {
    for cancelled in [false, true] {
        let (_root, store, conversation) = setup();
        let run = begin(&store, &conversation);
        let args = CollectionCreateArgs {
            name: "Must not appear".into(),
            description: "".into(),
            initial_memory_ids: vec![],
        };
        let op = stage(&store, &run, "create_collection", &args);
        if cancelled {
            store
                .stop_agent_input(&run.input_id, &run.attempt_id, "cancelled", None)
                .unwrap();
        } else {
            store
                .begin_agent_input(
                    &run.input_id,
                    &id(),
                    &conversation,
                    &run.input_text,
                    &[],
                    None,
                )
                .unwrap();
        }
        assert!(
            store
                .create_agent_collection(&run.input_id, &run.attempt_id, &op.operation_id, &args)
                .is_err()
        );
        assert!(store.collections().unwrap().is_empty());
        assert!(
            store
                .agent_input_receipts(&run.input_id)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn retry_after_committed_create_keeps_one_collection_and_receipt() {
    let (_root, store, conversation) = setup();
    let run = begin(&store, &conversation);
    let args = CollectionCreateArgs {
        name: "One only".into(),
        description: "".into(),
        initial_memory_ids: vec![],
    };
    let op = stage(&store, &run, "create_collection", &args);
    let first = store
        .create_agent_collection(&run.input_id, &run.attempt_id, &op.operation_id, &args)
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
    let staged = store
        .stage_agent_operation(
            &retry.input_id,
            &retry.attempt_id,
            &op.call_id,
            "create_collection",
            &serde_json::to_value(&args).unwrap(),
        )
        .unwrap();
    let replay = store
        .create_agent_collection(
            &retry.input_id,
            &retry.attempt_id,
            &staged.operation_id,
            &args,
        )
        .unwrap();
    assert_eq!(replay.result, first.result);
    assert_eq!(store.collections().unwrap().len(), 1);
    assert_eq!(store.agent_input_receipts(&run.input_id).unwrap().len(), 1);
}

#[test]
fn grouped_undo_handles_collection_creation_memory_creation_and_membership_changes() {
    let (_root, store, conversation) = setup();
    let existing = memory(&store, "Existing source");
    let run = begin(&store, &conversation);
    let create = CollectionCreateArgs {
        name: "Combined".into(),
        description: "".into(),
        initial_memory_ids: vec![existing.memory_id.clone()],
    };
    let op = stage(&store, &run, "create_collection", &create);
    store
        .create_agent_collection(&run.input_id, &run.attempt_id, &op.operation_id, &create)
        .unwrap();
    let c = store.collections().unwrap().pop().unwrap();
    let args = new_memory_args(
        &run,
        vec![CollectionRef {
            id: c.id.clone(),
            revision: c.revision,
        }],
    );
    let op = stage(&store, &run, "write_memory", &args);
    let saved = store
        .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap()
        .receipt
        .unwrap();
    apply_members(
        &store,
        &run,
        vec![edit(
            &store,
            &c.id,
            &[],
            std::slice::from_ref(&existing.memory_id),
        )],
    );
    let undo = store.undo_agent_input(&id(), &run.input_id).unwrap();
    assert!(undo.conflicts.is_empty() && undo.collection_conflicts.is_empty());
    assert!(store.collections().unwrap().is_empty());
    assert!(memberships(&store, &existing.memory_id).is_empty());
    assert_eq!(
        store.memory(saved.memory_id.as_ref().unwrap()).unwrap_err(),
        DataError::Unavailable
    );
    assert_eq!(
        store.memory(&existing.memory_id).unwrap().current.id,
        existing.version_id
    );
}

#[path = "support/agent_fixture.rs"]
mod agent_fixture;
use agent_fixture::{Response, fixture, sse_text as text, sse_tool as call};
fn last_content(request: &Value) -> Value {
    serde_json::from_str(
        request["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn wire_tools_discover_and_read_collection_and_membership_without_writes() {
    let (_root, store, conversation) = setup();
    let m = memory(&store, "Budget is 3600 yuan");
    let pine = collection(&store, "松果计划");
    let review = collection(&store, "本周复盘");
    for c in [&pine, &review] {
        store
            .collect_record(
                c,
                &RecordKey {
                    kind: "memory".into(),
                    id: m.memory_id.clone(),
                },
                true,
            )
            .unwrap();
    }
    let before = (
        count(&store, "receipts"),
        count(&store, "memory_versions"),
        count(&store, "collection_entries"),
    );
    let run = begin(&store, &conversation);
    let pine_id = pine.clone();
    let memory_id = m.memory_id.clone();
    let (config, requests, server) = fixture(5, move |index, request| {
        Response::stream(match index {
            0 => {
                assert!(last_content(request)["memory_context"]["topic"].is_null());
                let definitions = request["tools"].as_array().unwrap();
                for name in [
                    "list_collections",
                    "read_collection",
                    "create_collection",
                    "update_collection",
                    "update_collection_members",
                ] {
                    assert!(
                        definitions.iter().any(|d| d["function"]["name"] == name),
                        "missing callable tool {name}"
                    );
                }
                call(
                    "find-pine",
                    "list_collections",
                    json!({"query":"松果","memory_id":null,"offset":0,"limit":20}),
                )
            }
            1 => {
                let result = last_content(request);
                assert_eq!(result["total"], 1);
                assert_eq!(result["items"][0]["id"], pine_id);
                assert_eq!(result["items"][0]["count"], 1);
                call(
                    "read-pine",
                    "read_collection",
                    json!({"collection_id":pine_id}),
                )
            }
            2 => {
                let result = last_content(request);
                assert_eq!(result["name"], "松果计划");
                assert_eq!(result["directory"]["directory"][0]["memory_id"], memory_id);
                assert_eq!(result["directory"]["collection_id"], pine_id);
                call(
                    "membership",
                    "list_collections",
                    json!({"query":null,"memory_id":memory_id,"offset":0,"limit":20}),
                )
            }
            3 => {
                let result = last_content(request);
                assert_eq!(result["total"], 2);
                let names: Vec<_> = result["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v["name"].as_str().unwrap())
                    .collect();
                assert!(names.contains(&"松果计划") && names.contains(&"本周复盘"));
                text("这条记忆在松果计划和本周复盘中。")
            }
            _ => text("[]"),
        })
    });
    store
        .run_discussion(&config, &run.input_id, &run.attempt_id, "zh-CN", |_| {})
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 5);
    assert_eq!(
        (
            count(&store, "receipts"),
            count(&store, "memory_versions"),
            count(&store, "collection_entries")
        ),
        before
    );
}

#[tokio::test]
async fn scoped_conversation_search_uses_explicit_scope_and_global_default() {
    let (_root, store, _) = setup();
    let inside = memory(&store, "Budget inside is 3600 yuan");
    let outside = memory(&store, "Budget outside is 5000 yuan");
    let collection = collection(&store, "松果计划");
    store
        .collect_record(
            &collection,
            &RecordKey {
                kind: "memory".into(),
                id: inside.memory_id.clone(),
            },
            true,
        )
        .unwrap();
    let conversation = id();
    store
        .create_scoped_conversation(&conversation, "Scoped discussion", Some(&collection))
        .unwrap();
    let run = begin(&store, &conversation);
    let revision = revision(&store, &collection);
    let (config, _, server) = fixture(4, move |index, request| {
        Response::stream(match index {
            0 => {
                let context = last_content(request);
                assert_eq!(context["memory_context"]["topic"]["id"], collection);
                assert_eq!(context["memory_context"]["topic"]["name"], "松果计划");
                assert_eq!(context["memory_context"]["topic"]["revision"], revision);
                call(
                    "scoped",
                    "search_memories",
                    json!({"queries":[{"text":"Budget","keywords":["Budget"]}],"collection_id":collection,"origin":null,"project":null,"since":null,"until":null,"offset":0,"limit":8}),
                )
            }
            1 => {
                let result = last_content(request);
                assert_eq!(result["collection_id"], collection);
                assert_eq!(result["items"].as_array().unwrap().len(), 1);
                assert_eq!(result["items"][0]["memory_id"], inside.memory_id);
                call(
                    "global",
                    "search_memories",
                    json!({"queries":[{"text":"Budget","keywords":["Budget"]}],"collection_id":null,"origin":null,"project":null,"since":null,"until":null,"offset":0,"limit":8}),
                )
            }
            2 => {
                let result = last_content(request);
                assert!(result["collection_id"].is_null());
                let ids: Vec<_> = result["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v["memory_id"].as_str().unwrap())
                    .collect();
                assert_eq!(ids.len(), 2);
                assert!(
                    ids.contains(&inside.memory_id.as_str())
                        && ids.contains(&outside.memory_id.as_str())
                );
                text("已分别查询专题和全库。")
            }
            _ => text("[]"),
        })
    });
    store
        .run_discussion(&config, &run.input_id, &run.attempt_id, "zh-CN", |_| {})
        .await
        .unwrap();
    server.join().unwrap();
    assert!(
        store
            .agent_input_receipts(&run.input_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn collection_receipts_survive_conversation_deletion_and_remain_undoable_from_memory() {
    let (_root, store, conversation) = setup();
    let m = memory(&store, "Persistent evidence");
    let c = collection(&store, "Persistent collection");
    let run = begin(&store, &conversation);
    let op = apply_members(
        &store,
        &run,
        vec![edit(&store, &c, std::slice::from_ref(&m.memory_id), &[])],
    );
    let receipt = op.receipt.unwrap();
    let key = RecordKey {
        kind: "memory".into(),
        id: m.memory_id.clone(),
    };
    let history = store.memory_receipts(&key).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].receipt.request_id, receipt.request_id);
    assert_eq!(
        history[0].logical_input_id.as_deref(),
        Some(run.input_id.as_str())
    );
    assert_eq!(
        store.collection_agent_changes(&c).unwrap()[0].input_id,
        run.input_id
    );
    // Simulate removal of the whole conversation, including cascading execution rows.
    let db = Connection::open(store.database_path()).unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    db.execute("DELETE FROM conversations WHERE id=?", [&conversation])
        .unwrap();
    assert_eq!(
        store.memory_receipts(&key).unwrap()[0].receipt.request_id,
        receipt.request_id
    );
    assert_eq!(
        store.collection_agent_changes(&c).unwrap()[0].input_id,
        run.input_id
    );
    let undo = store.undo_agent_input(&id(), &run.input_id).unwrap();
    assert!(undo.conflicts.is_empty() && undo.collection_conflicts.is_empty());
    assert!(memberships(&store, &m.memory_id).is_empty());
    assert_eq!(store.memory(&m.memory_id).unwrap().current.id, m.version_id);
    store.check_integrity().unwrap();
}

#[test]
fn cancellation_after_committed_membership_keeps_receipt_and_blocks_pending_write() {
    let (_root, store, conversation) = setup();
    let m = memory(&store, "Original evidence");
    let c = collection(&store, "Plan");
    let run = begin(&store, &conversation);
    apply_members(
        &store,
        &run,
        vec![edit(&store, &c, std::slice::from_ref(&m.memory_id), &[])],
    );
    let args = CollectionUpdateArgs {
        collection_id: c.clone(),
        expected_revision: revision(&store, &c),
        name: "Never committed".into(),
        description: "".into(),
    };
    let pending = stage(&store, &run, "update_collection", &args);
    store
        .stop_agent_input(&run.input_id, &run.attempt_id, "cancelled", None)
        .unwrap();
    assert!(
        store
            .update_agent_collection(&run.input_id, &run.attempt_id, &pending.operation_id, &args)
            .is_err()
    );
    assert_eq!(memberships(&store, &m.memory_id), vec![c.clone()]);
    assert_eq!(store.read_agent_collection(&c).unwrap().name, "Plan");
    assert_eq!(store.agent_input_receipts(&run.input_id).unwrap().len(), 1);
    let undo = store.undo_agent_input(&id(), &run.input_id).unwrap();
    assert!(undo.conflicts.is_empty() && undo.collection_conflicts.is_empty());
    assert!(memberships(&store, &m.memory_id).is_empty());
}

#[test]
fn persisted_collection_call_arguments_cannot_be_changed_when_applying() {
    let (_root, store, conversation) = setup();
    let run = begin(&store, &conversation);
    let mut args = CollectionCreateArgs {
        name: "Approved".into(),
        description: "".into(),
        initial_memory_ids: vec![],
    };
    let op = stage(&store, &run, "create_collection", &args);
    args.name = "Altered after staging".into();
    assert_eq!(
        store
            .create_agent_collection(&run.input_id, &run.attempt_id, &op.operation_id, &args)
            .unwrap_err(),
        DataError::RequestConflict
    );
    assert!(store.collections().unwrap().is_empty());
    assert!(
        store
            .agent_input_receipts(&run.input_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn single_membership_receipt_undo_preserves_body_and_refuses_later_collection_changes() {
    for later_change in [false, true] {
        let (_root, store, conversation) = setup();
        let first = memory(&store, "First source");
        let second = memory(&store, "Second source");
        let c = collection(&store, "Plan");
        let run = begin(&store, &conversation);
        let receipt = apply_members(
            &store,
            &run,
            vec![edit(
                &store,
                &c,
                std::slice::from_ref(&first.memory_id),
                &[],
            )],
        )
        .receipt
        .unwrap();
        if later_change {
            store
                .collect_record(
                    &c,
                    &RecordKey {
                        kind: "memory".into(),
                        id: second.memory_id.clone(),
                    },
                    true,
                )
                .unwrap();
            assert_eq!(
                store.undo(&id(), &receipt.request_id).unwrap_err(),
                DataError::Conflict
            );
            assert_eq!(memberships(&store, &first.memory_id), vec![c.clone()]);
            assert_eq!(memberships(&store, &second.memory_id), vec![c]);
        } else {
            let request = id();
            let undone = store.undo(&request, &receipt.request_id).unwrap();
            assert_eq!(store.undo(&request, &receipt.request_id).unwrap(), undone);
            assert!(memberships(&store, &first.memory_id).is_empty());
            assert_eq!(store.receipt(&receipt.request_id).unwrap().status, "undone");
        }
        assert_eq!(
            store.memory(&first.memory_id).unwrap().current.id,
            first.version_id
        );
        assert_eq!(count(&store, "memory_versions"), 2);
    }
}

#[test]
fn mixed_group_undo_receipt_records_final_navigation_after_reversing_membership_and_body() {
    let (_root, store, conversation) = setup();
    let m = memory(&store, "Original body");
    let c = collection(&store, "Plan");
    let run = begin(&store, &conversation);
    apply_members(
        &store,
        &run,
        vec![edit(&store, &c, std::slice::from_ref(&m.memory_id), &[])],
    );
    let mut args = new_memory_args(&run, vec![]);
    args.destination = Destination::Existing {
        memory_id: m.memory_id.clone(),
        expected_version: m.version_id.clone(),
    };
    let op = stage(&store, &run, "write_memory", &args);
    store
        .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap();
    let undone = store.undo_agent_input(&id(), &run.input_id).unwrap();
    assert!(undone.conflicts.is_empty() && undone.collection_conflicts.is_empty());
    let actual = memberships(&store, &m.memory_id);
    assert!(actual.is_empty());
    assert_eq!(
        store.memory(&m.memory_id).unwrap().current.body,
        "Original body"
    );
    let changes = store
        .receipt_changes(&undone.receipt.unwrap().request_id)
        .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0].navigation_after.as_ref().unwrap().collections,
        actual
    );
    store.check_integrity().unwrap();
}

#[test]
fn omitted_initial_collections_normalizes_to_empty_without_relaxing_persisted_arguments() {
    let (_root, store, conversation) = setup();
    let run = begin(&store, &conversation);
    let raw = json!({
        "destination":{"kind":"new"},"title":"Interview preparation",
        "parts":[{"text":run.input_text,"sources":[{"source_id":run.user_message_id,"quote":run.input_text}]}]
    });
    let op = stage(&store, &run, "write_memory", &raw);
    let args: MemoryWriteArgs = serde_json::from_value(raw).unwrap();
    assert!(args.initial_collections.is_empty());
    let saved = store
        .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap();
    let memory_id = saved.receipt.unwrap().memory_id.unwrap();
    assert!(memberships(&store, &memory_id).is_empty());
    let mut altered = args;
    altered.title = "A different operation".into();
    assert_eq!(
        store
            .apply_agent_memory(&run.input_id, &run.attempt_id, &op.operation_id, &altered)
            .unwrap_err(),
        DataError::RequestConflict
    );
    assert_eq!(count(&store, "memories"), 1);
}

fn rename_collection(store: &MemoryStore, run: &AgentExecution, collection: &str, name: &str) {
    let args = CollectionUpdateArgs {
        collection_id: collection.into(),
        expected_revision: revision(store, collection),
        name: name.into(),
        description: String::new(),
    };
    let op = stage(store, run, "update_collection", &args);
    store
        .update_agent_collection(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap();
}

#[test]
fn group_undo_releases_all_names_before_restoring_a_swap_or_reused_name() {
    for create_replacement in [false, true] {
        let (_root, store, conversation) = setup();
        let first = "00000000-0000-4000-8000-000000000001";
        store.save_collection(first, "Alpha", "", None).unwrap();
        let run = begin(&store, &conversation);
        let second = if create_replacement {
            rename_collection(&store, &run, first, "Beta");
            let args = CollectionCreateArgs {
                name: "Alpha".into(),
                description: String::new(),
                initial_memory_ids: vec![],
            };
            let op = stage(&store, &run, "create_collection", &args);
            store
                .create_agent_collection(&run.input_id, &run.attempt_id, &op.operation_id, &args)
                .unwrap();
            store
                .collections()
                .unwrap()
                .into_iter()
                .find(|c| c.name == "Alpha")
                .unwrap()
                .id
        } else {
            let second = collection(&store, "Beta");
            rename_collection(&store, &run, first, "Temporary");
            rename_collection(&store, &run, &second, "Alpha");
            rename_collection(&store, &run, first, "Beta");
            second
        };
        let undone = store.undo_agent_input(&id(), &run.input_id).unwrap();
        assert!(undone.conflicts.is_empty() && undone.collection_conflicts.is_empty());
        assert_eq!(store.read_agent_collection(first).unwrap().name, "Alpha");
        if create_replacement {
            assert_eq!(
                store.read_agent_collection(&second).unwrap_err(),
                DataError::Unavailable
            );
            assert_eq!(store.collections().unwrap().len(), 1);
        } else {
            assert_eq!(store.read_agent_collection(&second).unwrap().name, "Beta");
        }
        store.check_integrity().unwrap();
    }
}

#[test]
fn undoing_a_removed_membership_respects_later_memory_lifecycle_but_keeps_later_body_edits() {
    for later_state in ["trashed", "purged", "merged", "edited"] {
        let (_root, store, conversation) = setup();
        let source = memory(&store, "Original source");
        let c = collection(&store, "Plan");
        store
            .collect_record(
                &c,
                &RecordKey {
                    kind: "memory".into(),
                    id: source.memory_id.clone(),
                },
                true,
            )
            .unwrap();
        let run = begin(&store, &conversation);
        let receipt = apply_members(
            &store,
            &run,
            vec![edit(
                &store,
                &c,
                &[],
                std::slice::from_ref(&source.memory_id),
            )],
        )
        .receipt
        .unwrap();
        let removed_revision = revision(&store, &c);
        store
            .stop_agent_input(&run.input_id, &run.attempt_id, "cancelled", None)
            .unwrap();
        match later_state {
            "trashed" | "purged" => {
                store
                    .trash_memory(&source.memory_id, &source.version_id)
                    .unwrap();
                if later_state == "purged" {
                    store.purge_memory(&source.memory_id).unwrap();
                }
            }
            "merged" => {
                let target = memory(&store, "Destination body");
                let later = begin(&store, &conversation);
                let args = MemoryMergeArgs {
                    target_memory_id: target.memory_id,
                    target_version: target.version_id.clone(),
                    source_memory_id: source.memory_id.clone(),
                    source_version: source.version_id.clone(),
                    title: "Combined source".into(),
                    reason: "Explicit test merge".into(),
                    parts: vec![
                        MemoryWritePart {
                            text: "Destination body\n".into(),
                            sources: vec![MemorySourceQuote {
                                source_id: target.version_id,
                                quote: "Destination body".into(),
                            }],
                        },
                        MemoryWritePart {
                            text: "Original source".into(),
                            sources: vec![MemorySourceQuote {
                                source_id: source.version_id.clone(),
                                quote: "Original source".into(),
                            }],
                        },
                    ],
                };
                let op = stage(&store, &later, "merge_memories", &args);
                store
                    .merge_agent_memories(
                        &later.input_id,
                        &later.attempt_id,
                        &op.operation_id,
                        &args,
                    )
                    .unwrap();
            }
            "edited" => {
                let later = begin(&store, &conversation);
                let mut args = new_memory_args(&later, vec![]);
                args.destination = Destination::Existing {
                    memory_id: source.memory_id.clone(),
                    expected_version: source.version_id.clone(),
                };
                let op = stage(&store, &later, "write_memory", &args);
                store
                    .apply_agent_memory(&later.input_id, &later.attempt_id, &op.operation_id, &args)
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            revision(&store, &c),
            removed_revision,
            "The removed edge cannot invalidate this collection through its member trigger"
        );
        if later_state == "edited" {
            let current = store.memory(&source.memory_id).unwrap().current;
            let undone = store.undo_agent_input(&id(), &run.input_id).unwrap();
            assert!(undone.conflicts.is_empty() && undone.collection_conflicts.is_empty());
            assert_eq!(memberships(&store, &source.memory_id), vec![c]);
            assert_eq!(
                store.memory(&source.memory_id).unwrap().current.id,
                current.id
            );
        } else {
            assert_eq!(
                store.undo(&id(), &receipt.request_id).unwrap_err(),
                DataError::Conflict
            );
            let undone = store.undo_agent_input(&id(), &run.input_id).unwrap();
            assert!(undone.receipt.is_none());
            assert_eq!(undone.collection_conflicts, vec![c.clone()]);
            assert_eq!(store.read_agent_collection(&c).unwrap().count, 0);
            let db = Connection::open(store.database_path()).unwrap();
            let edges: i64 = db.query_row("SELECT count(*) FROM collection_entries WHERE collection_id=?1 AND record_id=?2", rusqlite::params![c, source.memory_id], |r| r.get(0)).unwrap();
            assert_eq!(
                edges, 0,
                "Undo must not recreate a hidden or purged member edge"
            );
            assert_eq!(
                store.receipt(&receipt.request_id).unwrap().status,
                "applied"
            );
        }
        store.check_integrity().unwrap();
    }
}

#[test]
fn undone_collection_creation_keeps_readable_history_after_conversation_deletion() {
    let (_root, store, conversation) = setup();
    let run = begin(&store, &conversation);
    let args = CollectionCreateArgs {
        name: "Plan".into(),
        description: String::new(),
        initial_memory_ids: vec![],
    };
    let op = stage(&store, &run, "create_collection", &args);
    let created = store
        .create_agent_collection(&run.input_id, &run.attempt_id, &op.operation_id, &args)
        .unwrap();
    let collection = created.receipt.unwrap().collection_changes[0]
        .collection_id
        .clone();
    assert_eq!(
        store.collection_agent_changes(&collection).unwrap()[0].receipts[0].status,
        "applied"
    );
    store.undo_agent_input(&id(), &run.input_id).unwrap();
    assert!(store.collections().unwrap().is_empty());
    assert_eq!(
        store.read_agent_collection(&collection).unwrap_err(),
        DataError::Unavailable
    );
    assert_eq!(
        store.collection_agent_changes(&collection).unwrap()[0].receipts[0].status,
        "undone"
    );
    let db = Connection::open(store.database_path()).unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    db.execute("DELETE FROM conversations WHERE id=?", [&conversation])
        .unwrap();
    assert_eq!(
        store.collection_agent_changes(&collection).unwrap()[0].receipts[0].status,
        "undone"
    );
    assert_eq!(
        store.collection_agent_changes(&id()).unwrap_err(),
        DataError::Unavailable
    );
}

#[test]
fn maximum_membership_batch_keeps_tool_output_bounded_and_full_durable_receipts() {
    let (_root, store, conversation) = setup();
    let memories: Vec<_> = (0..50)
        .map(|index| memory(&store, &format!("Source {index}")).memory_id)
        .collect();
    let collections: Vec<_> = (0..20)
        .map(|index| {
            let collection = id();
            let name = format!("{index:02}{}", "\\".repeat(238));
            store
                .save_collection(&collection, &name, &"\\".repeat(2400), None)
                .unwrap();
            collection
        })
        .collect();
    let run = begin(&store, &conversation);
    let saved = apply_members(
        &store,
        &run,
        collections
            .iter()
            .map(|collection| edit(&store, collection, &memories, &[]))
            .collect(),
    );
    let output = serde_json::to_vec(saved.result.as_ref().unwrap()).unwrap();
    assert!(
        output.len() < 16 * 1024,
        "Maximum legal tool output was {} bytes",
        output.len()
    );
    let receipt = saved.receipt.unwrap();
    assert_eq!(receipt.collection_changes.len(), 20);
    for change in &receipt.collection_changes {
        assert_eq!(change.added_memory_ids.len(), 50);
        assert_eq!(change.before.as_ref().unwrap().description.len(), 2400);
        assert_eq!(change.after.description.len(), 2400);
    }
    assert_eq!(store.receipt(&receipt.request_id).unwrap(), receipt);
    let undo_conversation = id();
    store
        .create_conversation(&undo_conversation, "Undo batch")
        .unwrap();
    let undo_run = begin(&store, &undo_conversation);
    let op = stage(
        &store,
        &undo_run,
        "undo_changes",
        &json!({"input_id":run.input_id}),
    );
    let undone = store
        .undo_agent_operation(
            &undo_run.input_id,
            &undo_run.attempt_id,
            &op.operation_id,
            &run.input_id,
        )
        .unwrap();
    assert!(
        serde_json::to_vec(undone.result.as_ref().unwrap())
            .unwrap()
            .len()
            < 16 * 1024
    );
    assert_eq!(undone.receipt.unwrap().collection_changes.len(), 20);
    for collection in collections {
        assert_eq!(store.read_agent_collection(&collection).unwrap().count, 0);
    }
}

#[tokio::test]
async fn maximum_plain_user_input_reaches_model_with_complete_tool_definitions() {
    let (_root, store, conversation) = setup();
    let input = "a".repeat(32 * 1024);
    let run = store
        .begin_agent_input(&id(), &id(), &conversation, &input, &[], None)
        .unwrap();
    let expected = input.clone();
    let (config, requests, server) = fixture(2, move |index, request| {
        Response::stream(if index == 0 {
            let context = last_content(request);
            assert_eq!(context["current_message"], expected);
            assert_eq!(context["recent_messages"], json!([]));
            assert!(
                request["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|d| d["function"]["name"] == "update_collection_members")
            );
            text("Received the complete input.")
        } else {
            text("[]")
        })
    });
    store
        .run_discussion(&config, &run.input_id, &run.attempt_id, "en", |_| {})
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert_eq!(
        store.agent_execution(&run.input_id).unwrap().state,
        "complete"
    );
    assert_eq!(store.turn(&run.input_id).unwrap().user.text, input);
    assert!(
        store
            .agent_input_receipts(&run.input_id)
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn longest_collection_descriptions_leave_list_results_bounded_and_details_readable() {
    let (_root, store, conversation) = setup();
    let description = "\\".repeat(2400);
    for index in 0..20 {
        store
            .save_collection(
                &id(),
                &format!("{index:02}{}", "\\".repeat(238)),
                &description,
                None,
            )
            .unwrap();
    }
    let run = begin(&store, &conversation);
    let expected = description;
    let (config, requests, server) = fixture(4, move |index, request| {
        Response::stream(match index {
            0 => call(
                "list-long",
                "list_collections",
                json!({"query":null,"memory_id":null,"offset":0,"limit":20}),
            ),
            1 => {
                let result = last_content(request);
                assert_eq!(result["total"], 20);
                assert_eq!(result["items"].as_array().unwrap().len(), 20);
                assert!(result["next_offset"].is_null());
                assert!(serde_json::to_vec(&result).unwrap().len() < 16 * 1024);
                for item in result["items"].as_array().unwrap() {
                    assert_eq!(item["name"].as_str().unwrap().len(), 240);
                    assert!(item["revision"].as_i64().unwrap() > 0);
                    assert_eq!(item["count"], 0);
                }
                call(
                    "read-long",
                    "read_collection",
                    json!({"collection_id":result["items"][0]["id"]}),
                )
            }
            2 => {
                let result = last_content(request);
                assert_eq!(result["description"], expected);
                assert_eq!(result["count"], 0);
                assert!(
                    result["directory"]["directory"]
                        .as_array()
                        .unwrap()
                        .is_empty()
                );
                text("The directory and full description are available.")
            }
            _ => text("[]"),
        })
    });
    store
        .run_discussion(&config, &run.input_id, &run.attempt_id, "en", |_| {})
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 4);
    assert!(
        store
            .agent_input_receipts(&run.input_id)
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn consecutive_maximum_inputs_summarize_only_history_within_the_request_budget() {
    let (_root, store, conversation) = setup();
    let previous_input = "A".repeat(32 * 1024);
    let current_input = "B".repeat(32 * 1024);
    let earlier = store
        .begin_agent_input(&id(), &id(), &conversation, &previous_input, &[], None)
        .unwrap();
    store
        .append_agent_text(
            &earlier.input_id,
            &earlier.attempt_id,
            "Earlier input received.",
        )
        .unwrap();
    store
        .finish_agent_input(&earlier.input_id, &earlier.attempt_id, &[])
        .unwrap();
    let current = store
        .begin_agent_input(&id(), &id(), &conversation, &current_input, &[], None)
        .unwrap();
    let previous_expected = previous_input.clone();
    let current_expected = current_input.clone();
    let summary = "The user supplied a long earlier passage of A characters.";
    let (config, requests, server) = fixture(3, move |index, request| {
        Response::stream(match index {
            0 => {
                assert!(
                    request
                        .get("tools")
                        .and_then(Value::as_array)
                        .is_none_or(Vec::is_empty)
                );
                let serialized = serde_json::to_vec(request).unwrap();
                assert!(
                    serialized.len() <= 57_344,
                    "History summary request was {} bytes",
                    serialized.len()
                );
                let content = &request["messages"].as_array().unwrap().last().unwrap()["content"];
                let content = content
                    .as_str()
                    .or_else(|| content[0]["text"].as_str())
                    .unwrap();
                let context: Value = serde_json::from_str(content).unwrap();
                assert_eq!(context["messages"][0]["text"], previous_expected);
                assert_eq!(context["messages"][1]["text"], "Earlier input received.");
                assert!(context.get("current_message").is_none());
                assert!(
                    !content.contains(&current_expected),
                    "The current input must not be duplicated in the history summary request"
                );
                text(summary)
            }
            1 => {
                let context = last_content(request);
                assert_eq!(context["current_message"], current_expected);
                assert_eq!(context["earlier_summary"], summary);
                assert!(context["summary_through_seq"].as_i64().unwrap() > 0);
                assert!(
                    request["tools"]
                        .as_array()
                        .is_some_and(|tools| !tools.is_empty())
                );
                assert!(serde_json::to_vec(request).unwrap().len() <= 57_344);
                text("Both inputs remain available, and the earlier passage was summarized.")
            }
            _ => text("[]"),
        })
    });
    store
        .run_discussion(
            &config,
            &current.input_id,
            &current.attempt_id,
            "en",
            |_| {},
        )
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 3);
    assert_eq!(
        store.turn(&earlier.input_id).unwrap().user.text,
        previous_input
    );
    assert_eq!(
        store.turn(&current.input_id).unwrap().user.text,
        current_input
    );
    assert_eq!(
        store.agent_execution(&current.input_id).unwrap().state,
        "complete"
    );
    assert_eq!(
        store
            .agent_conversation_context(&conversation)
            .unwrap()
            .summary,
        summary
    );
    assert!(
        store
            .agent_input_receipts(&current.input_id)
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn search_pages_account_for_large_membership_metadata_without_losing_or_repeating_hits() {
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};
    let (_root, store, _) = setup();
    let collections: Vec<_> = (0..10)
        .map(|index| {
            let collection = id();
            store
                .save_collection(
                    &collection,
                    &format!("{index:02}{}", "\\".repeat(238)),
                    "",
                    None,
                )
                .unwrap();
            collection
        })
        .collect();
    let expected: BTreeSet<_> = (0..8)
        .map(|index| {
            let saved = memory(
                &store,
                &format!(
                    "松果访谈 {index}：{}",
                    "需要保护录音隐私并保留原始资料。".repeat(120)
                ),
            );
            for collection in &collections {
                store
                    .collect_record(
                        collection,
                        &RecordKey {
                            kind: "memory".into(),
                            id: saved.memory_id.clone(),
                        },
                        true,
                    )
                    .unwrap();
            }
            saved.memory_id
        })
        .collect();
    let mut seen = BTreeSet::new();
    let mut offset = 0;
    let mut pages = 0;
    loop {
        let conversation = id();
        store
            .create_conversation(&conversation, "Search page acceptance")
            .unwrap();
        let run = begin(&store, &conversation);
        let result = Arc::new(Mutex::new(Value::Null));
        let recorded = result.clone();
        let (config, requests, server) = fixture(3, move |index, request| {
            Response::stream(match index {
                0 => call(
                    "search-page",
                    "search_memories",
                    json!({
                        "queries":[{"text":"松果访谈","keywords":["松果访谈"]}],
                        "collection_id":null,"origin":null,"project":null,"since":null,"until":null,
                        "offset":offset,"limit":8
                    }),
                ),
                1 => {
                    let page = last_content(request);
                    let size = serde_json::to_vec(&page).unwrap().len();
                    assert!(
                        size < 17 * 1024,
                        "Search result plus status envelope was {size} bytes"
                    );
                    assert!(!page["items"].as_array().unwrap().is_empty());
                    for item in page["items"].as_array().unwrap() {
                        assert_eq!(item["current_collections"]["total"], 10);
                        assert_eq!(
                            item["current_collections"]["items"]
                                .as_array()
                                .unwrap()
                                .len(),
                            10
                        );
                    }
                    *recorded.lock().unwrap() = page;
                    text("This page was read.")
                }
                _ => text("[]"),
            })
        });
        store
            .run_discussion(&config, &run.input_id, &run.attempt_id, "en", |_| {})
            .await
            .unwrap();
        server.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 3);
        let result = result.lock().unwrap();
        let items = result["items"].as_array().unwrap();
        for item in items {
            let memory = item["memory_id"].as_str().unwrap().to_owned();
            assert!(expected.contains(&memory));
            assert!(
                seen.insert(memory),
                "A continuation page must not repeat a prior hit"
            );
        }
        pages += 1;
        assert!(
            pages <= expected.len(),
            "Every continuation must make progress"
        );
        if let Some(next) = result["next_offset"].as_u64() {
            assert_eq!(next as usize, offset + items.len());
            assert!(next as usize > offset);
            assert_eq!(result["truncated"], true);
            offset = next as usize;
        } else {
            break;
        }
    }
    assert!(
        pages > 1,
        "The maximum result metadata must exercise the byte-budget page boundary"
    );
    assert_eq!(seen, expected);
}
