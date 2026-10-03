//! MemoryStore process test harness. Uses only synthetic content and an explicit root.
use memivy_core::memory::*;
use memivy_core::model::{self, AssistantContent, Message, ToolCall};
use rig_core::message::ToolFunction;
use serde::Serialize;
use serde_json::{Value, json};
use std::{io::Write, path::PathBuf};
use uuid::Uuid;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        // Let process tests distinguish retryable lock contention from errors.
        std::process::exit(if error == DataError::Busy { 75 } else { 1 });
    }
}
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let root = args.get(1).ok_or(DataError::Invalid)?;
    let command = args.get(2).ok_or(DataError::Invalid)?;
    if command == "collection-fixture" {
        let path = std::path::Path::new(root);
        if !path.is_absolute() {
            return Err(DataError::Invalid);
        }
        match std::fs::symlink_metadata(path) {
            Ok(metadata) => {
                if !metadata.is_dir() || std::fs::read_dir(path)?.next().is_some() {
                    return Err(DataError::Invalid);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let store = MemoryStore::open(path)?;
        println!("{}", collection_fixture(&store)?);
        return Ok(());
    }
    if command == "open-application" {
        let store = MemoryStore::open_application(root)?;
        println!(
            "{}",
            serde_json::to_string(&store.last_restore_result()?).map_err(|_| DataError::Invalid)?
        );
        return Ok(());
    }
    if command == "hold-migration" {
        // Test harness only: pause the same migration SQL inside an uncommitted
        // SQLite transaction. No pause hook or compatibility flow in the app.
        std::fs::create_dir(root)?;
        let mut db = rusqlite::Connection::open(PathBuf::from(root).join("memivy.db"))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        let tx = db.transaction()?;
        tx.execute_batch(include_str!("../../../migrations/memory/001_initial.sql"))?;
        tx.pragma_update(None, "application_id", 0x4d454d59_i64)?;
        println!("migration transaction open; schema 1 not committed");
        std::io::stdout().flush()?;
        loop {
            std::thread::park();
        }
    }
    let store = MemoryStore::open(PathBuf::from(root))?;
    match command.as_str() {
        "edit-current" => {
            let memory_id = args.get(3).ok_or(DataError::Invalid)?.clone();
            let detail = store.library_detail(&RecordKey {
                kind: "memory".into(),
                id: memory_id.clone(),
            })?;
            let current = detail.current;
            let receipt = store.edit_memory(&EditRequest {
                request_id: Uuid::new_v4().to_string(),
                memory_id,
                expected_version: current.id,
                title: current.title,
                body: args.get(4).ok_or(DataError::Invalid)?.clone(),
            })?;
            println!(
                "{}",
                serde_json::to_string(&receipt).map_err(|_| DataError::Invalid)?
            );
        }
        "hold-turn" => {
            let topic = Uuid::new_v4().to_string();
            store.create_conversation(
                &topic,
                "Synthetic conversation interrupted during generation",
            )?;
            let turn = store.begin_agent_input(
                &Uuid::new_v4().to_string(),
                &Uuid::new_v4().to_string(),
                &topic,
                "An ordinary question must not become a memory",
                &[],
                None,
            )?;
            println!("{}", turn.input_id);
            std::io::stdout().flush()?;
            loop {
                std::thread::park();
            }
        }
        "recover-turn" => {
            store.recover_interrupted_turns()?;
        }
        "capture" | "hold" | "hold-memory" => {
            let request = CaptureRequest {
                request_id: args.get(3).ok_or(DataError::Invalid)?.clone(),
                text: args.get(4).ok_or(DataError::Invalid)?.clone(),
                origin: Origin::User {
                    app: "MemoryStore process test".into(),
                    project: None,
                    uri: None,
                },
            };
            let capture = store.capture(&request)?;
            if command == "hold-memory" {
                let r = store.edit_memory(&EditRequest {
                    request_id: Uuid::new_v4().to_string(),
                    memory_id: capture.memory_id,
                    expected_version: capture.version_id,
                    title: "Version before forced exit".into(),
                    body: request.text,
                })?;
                println!(
                    "{}",
                    serde_json::to_string(&r).map_err(|_| DataError::Invalid)?
                );
            } else {
                println!(
                    "{}",
                    serde_json::to_string(&capture).map_err(|_| DataError::Invalid)?
                );
            }
            std::io::stdout().flush()?;
            if command != "capture" {
                loop {
                    std::thread::park();
                }
            }
        }

        "prepare-restore" => {
            println!(
                "{}",
                serde_json::to_string(&store.prepare_restore(std::path::Path::new(
                    args.get(3).ok_or(DataError::Invalid)?
                ))?)
                .map_err(|_| DataError::Invalid)?
            )
        }
        "arm-restore" => store.arm_restore(args.get(3).ok_or(DataError::Invalid)?)?,
        "backup" => store.backup(PathBuf::from(args.get(3).ok_or(DataError::Invalid)?))?,
        "diagnostics" => {
            store.check_integrity()?;
            // Read-only diagnostics for the harness, never a product write path.
            let db = rusqlite::Connection::open(store.database_path())?;
            let version: String = db.query_row("SELECT sqlite_version()", [], |r| r.get(0))?;
            let count: i64 = db.query_row("SELECT count(*) FROM captures", [], |r| r.get(0))?;
            let versions: i64 =
                db.query_row("SELECT count(*) FROM memory_versions", [], |r| r.get(0))?;
            println!(
                "{}",
                serde_json::json!({"sqlite_version":version,"captures":count,"versions":versions,"integrity":"ok"})
            );
        }
        _ => return Err(DataError::Invalid),
    }
    Ok(())
}

fn fixture_id() -> String {
    Uuid::new_v4().to_string()
}
fn fixture_memory(store: &MemoryStore, title: &str, body: &str) -> Result<String> {
    let captured = store.capture(&CaptureRequest {
        request_id: fixture_id(),
        text: body.into(),
        origin: Origin::User {
            app: "Synthetic collection acceptance".into(),
            project: None,
            uri: None,
        },
    })?;
    store.edit_memory(&EditRequest {
        request_id: fixture_id(),
        memory_id: captured.memory_id.clone(),
        expected_version: captured.version_id,
        title: title.into(),
        body: body.into(),
    })?;
    Ok(captured.memory_id)
}
fn fixture_turn(store: &MemoryStore, conversation: &str, text: &str) -> Result<AgentExecution> {
    store.begin_agent_input(&fixture_id(), &fixture_id(), conversation, text, &[], None)
}
fn fixture_stage(
    store: &MemoryStore,
    run: &AgentExecution,
    name: &str,
    args: &impl Serialize,
) -> Result<AgentOperation> {
    let arguments = serde_json::to_value(args).map_err(|_| DataError::Invalid)?;
    let call = ToolCall::from_wire(
        fixture_id(),
        ToolFunction::new(name.into(), arguments.clone()),
    );
    let mut protocol = store.agent_execution(&run.input_id)?.protocol;
    protocol.push(json!(Message::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(call.clone())],
    }));
    store.checkpoint_agent(&run.input_id, &run.attempt_id, &protocol)?;
    store.stage_agent_operation(
        &run.input_id,
        &run.attempt_id,
        call.id.as_str(),
        name,
        &arguments,
    )
}
fn fixture_finish(
    store: &MemoryStore,
    run: &AgentExecution,
    operation: &AgentOperation,
    answer: &str,
) -> Result<()> {
    let call = ToolCall::from_wire(
        &operation.call_id,
        ToolFunction::new(operation.name.clone(), operation.arguments.clone()),
    );
    let mut protocol = store.agent_execution(&run.input_id)?.protocol;
    protocol.push(json!(model::tool_result(
        &call,
        operation.result.as_ref().ok_or(DataError::Integrity)?
    )));
    store.checkpoint_agent(&run.input_id, &run.attempt_id, &protocol)?;
    store.append_agent_text(&run.input_id, &run.attempt_id, answer)?;
    store.finish_agent_input(&run.input_id, &run.attempt_id, &[])?;
    Ok(())
}
fn collection_fixture(store: &MemoryStore) -> Result<Value> {
    let budget = fixture_memory(store, "试点预算", "松果工具的试点预算是3600元，尚未上线。")?;
    let privacy = fixture_memory(store, "录音处理", "访谈录音仅在本机处理，禁止上传。")?;
    let hours = fixture_memory(store, "时间安排", "我每周能投入六小时，周六优先。")?;
    let history = fixture_id();
    store.create_conversation(&history, "专题创建与整理")?;
    let create_run = fixture_turn(
        store,
        &history,
        "新建一个叫松果准备的专题，说明写‘离线访谈工具的准备事项’，把试点预算加入。",
    )?;
    let create = CollectionCreateArgs {
        name: "松果准备".into(),
        description: "离线访谈工具的准备事项".into(),
        initial_memory_ids: vec![budget.clone()],
    };
    let op = fixture_stage(store, &create_run, "create_collection", &create)?;
    let created = store.create_agent_collection(
        &create_run.input_id,
        &create_run.attempt_id,
        &op.operation_id,
        &create,
    )?;
    let collection = created
        .receipt
        .as_ref()
        .and_then(|r| r.collection_changes.first())
        .ok_or(DataError::Integrity)?
        .collection_id
        .clone();
    fixture_finish(
        store,
        &create_run,
        &created,
        "已创建“松果准备”，并加入试点预算这条记忆。",
    )?;

    let rename_run = fixture_turn(store, &history, "把松果准备改名为松果计划，说明保留。")?;
    let rename = CollectionUpdateArgs {
        collection_id: collection.clone(),
        expected_revision: store.read_agent_collection(&collection)?.revision,
        name: "松果计划".into(),
        description: create.description,
    };
    let op = fixture_stage(store, &rename_run, "update_collection", &rename)?;
    let renamed = store.update_agent_collection(
        &rename_run.input_id,
        &rename_run.attempt_id,
        &op.operation_id,
        &rename,
    )?;
    fixture_finish(
        store,
        &rename_run,
        &renamed,
        "已将专题改名为“松果计划”，说明保持不变。",
    )?;

    let add_run = fixture_turn(store, &history, "把录音处理这条记忆也加入松果计划。")?;
    let add = CollectionMembersArgs {
        changes: vec![CollectionMemberEdit {
            collection_id: collection.clone(),
            expected_revision: store.read_agent_collection(&collection)?.revision,
            add_memory_ids: vec![privacy.clone()],
            remove_memory_ids: vec![],
        }],
    };
    let op = fixture_stage(store, &add_run, "update_collection_members", &add)?;
    let added = store.update_agent_collection_members(
        &add_run.input_id,
        &add_run.attempt_id,
        &op.operation_id,
        &add,
    )?;
    fixture_finish(
        store,
        &add_run,
        &added,
        "已加入录音处理。“松果计划”现在有两条记忆。",
    )?;

    let review = fixture_id();
    store.save_collection(&review, "本周复盘", "本周需要回顾的事项", None)?;
    store.collect_record(
        &review,
        &RecordKey {
            kind: "memory".into(),
            id: budget.clone(),
        },
        true,
    )?;
    let undo_conversation = fixture_id();
    store.create_scoped_conversation(&undo_conversation, "专题成员撤销验收", Some(&review))?;
    let undo_run = fixture_turn(
        store,
        &undo_conversation,
        "把时间安排加入本周复盘，其他归属保留。",
    )?;
    let add = CollectionMembersArgs {
        changes: vec![CollectionMemberEdit {
            collection_id: review.clone(),
            expected_revision: store.read_agent_collection(&review)?.revision,
            add_memory_ids: vec![hours.clone()],
            remove_memory_ids: vec![],
        }],
    };
    let op = fixture_stage(store, &undo_run, "update_collection_members", &add)?;
    let added = store.update_agent_collection_members(
        &undo_run.input_id,
        &undo_run.attempt_id,
        &op.operation_id,
        &add,
    )?;
    fixture_finish(
        store,
        &undo_run,
        &added,
        "已将时间安排加入“本周复盘”，没有修改记忆正文。",
    )?;
    store.check_integrity()?;
    Ok(json!({
        "synthetic_only":true,"database_path":store.database_path(),
        "memory_ids":{"budget":budget,"privacy":privacy,"hours":hours},
        "collection_ids":{"pine":collection,"review":review},
        "conversation_ids":{"history":history,"undo":undo_conversation},
        "input_ids":{"create":create_run.input_id,"rename":rename_run.input_id,"add":add_run.input_id,"undo_target":undo_run.input_id},
        "undo_expectation":{"collection":"本周复盘","before_member_count":2,"after_member_count":1,"removed_memory_title":"时间安排","memory_body_preserved":true}
    }))
}
