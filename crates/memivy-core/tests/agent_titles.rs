use memivy_core::memory::*;
use rusqlite::Connection;
use serde_json::json;
use std::sync::{Arc, Barrier};

#[path = "support/agent_fixture.rs"]
mod agent_fixture;
use agent_fixture::{Response, fixture, sse_text, sse_tool};

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn setup() -> (tempfile::TempDir, MemoryStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let conversation = id();
    store
        .create_conversation(&conversation, "Initial title")
        .unwrap();
    (dir, store, conversation)
}

fn begin(store: &MemoryStore, conversation: &str, text: &str) -> AgentExecution {
    store
        .begin_agent_input(&id(), &id(), conversation, text, &[], None)
        .unwrap()
}

fn finish(store: &MemoryStore, run: &AgentExecution) {
    store
        .append_agent_text(&run.input_id, &run.attempt_id, "ASSISTANT_PRIVATE_MARKER")
        .unwrap();
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, false, &[])
        .unwrap();
}

fn generated(store: &MemoryStore, conversation: &str) -> bool {
    Connection::open(store.database_path())
        .unwrap()
        .query_row(
            "SELECT title_generated FROM conversations WHERE id=?",
            [conversation],
            |r| r.get(0),
        )
        .unwrap()
}

fn commit_memory(store: &MemoryStore, run: &AgentExecution) -> Receipt {
    let args = MemoryWriteArgs {
        destination: Destination::New,
        title: "用户计划".into(),
        parts: vec![MemoryWritePart {
            text: run.input_text.clone(),
            sources: vec![MemorySourceQuote {
                source_id: run.user_message_id.clone(),
                quote: run.input_text.clone(),
            }],
        }],
    };
    let call = id();
    let value = serde_json::to_value(&args).unwrap();
    let protocol = vec![json!({"role":"assistant","content":null,"tool_calls":[{
        "id":call,"type":"function","function":{"name":"write_memory","arguments":value.to_string()}
    }]})];
    store
        .checkpoint_agent(&run.input_id, &run.attempt_id, &protocol)
        .unwrap();
    let operation = store
        .stage_agent_operation(
            &run.input_id,
            &run.attempt_id,
            &call,
            "write_memory",
            &value,
        )
        .unwrap();
    store
        .apply_agent_memory(
            &run.input_id,
            &run.attempt_id,
            &operation.operation_id,
            &args,
        )
        .unwrap()
        .receipt
        .unwrap()
}

#[tokio::test]
async fn chinese_and_english_titles_are_saved_once_across_later_turns() {
    for (input, title) in [
        ("我应该怎么安排每周的时间？", "每周时间安排"),
        ("How should I plan my week?", "Weekly time planning"),
    ] {
        let (_dir, store, conversation) = setup();
        let run = begin(&store, &conversation, input);
        finish(&store, &run);
        // Keep a second response available so an unwanted rename is observable.
        let (config, requests, server) = fixture(2, move |_, _| Response::stream(sse_text(title)));
        assert!(
            store
                .generate_agent_title(&config, &run.input_id, &run.attempt_id)
                .await
                .unwrap()
        );
        assert_eq!(store.conversation(&conversation).unwrap().title, title);
        assert!(generated(&store, &conversation));
        assert_eq!(
            store
                .create_conversation(&conversation, "Initial title")
                .unwrap()
                .title,
            title,
            "redelivering the original create request must preserve the generated title"
        );
        let collection = id();
        store
            .save_collection(&collection, "Another scope", "", None)
            .unwrap();
        assert_eq!(
            store
                .create_scoped_conversation(&conversation, "Initial title", Some(&collection))
                .unwrap_err(),
            DataError::RequestConflict,
            "a generated title must not weaken the original conversation scope"
        );
        assert!(
            store
                .conversation(&conversation)
                .unwrap()
                .collection_id
                .is_none()
        );
        let later = begin(&store, &conversation, "Now discuss a different subject.");
        finish(&store, &later);
        assert!(
            !store
                .generate_agent_title(&config, &later.input_id, &later.attempt_id)
                .await
                .unwrap()
        );
        assert_eq!(store.conversation(&conversation).unwrap().title, title);
        assert_eq!(requests.lock().unwrap().len(), 1);
        drop(server);
    }
}

#[tokio::test]
async fn title_request_contains_only_six_bounded_user_messages_through_its_input() {
    let (_dir, store, conversation) = setup();
    let long = "语".repeat(1000) + "TRUNCATED_PRIVATE_MARKER";
    let inputs = [
        "OLD_EXCLUDED_PRIVATE_MARKER",
        long.as_str(),
        "保留的第二条用户原话",
        "保留的第三条用户原话",
        "Retained fourth user expression",
        "Retained fifth user expression",
        "CURRENT_USER_EXPRESSION",
    ];
    let mut runs: Vec<String> = vec![];
    for input in inputs {
        let run = begin(&store, &conversation, input);
        if input == "CURRENT_USER_EXPRESSION" {
            let previous = runs.last().unwrap();
            let through = store.turn(previous).unwrap().assistant.seq;
            let context = store.agent_conversation_context(&conversation).unwrap();
            store
                .save_agent_summary(
                    &run.input_id,
                    &run.attempt_id,
                    "SUMMARY_PRIVATE_MARKER",
                    through,
                    &context.receipt_revision,
                )
                .unwrap();
        }
        finish(&store, &run);
        runs.push(run.input_id);
    }
    let target = store.agent_execution(runs.last().unwrap()).unwrap();
    let future = begin(&store, &conversation, "FUTURE_PRIVATE_MARKER");
    finish(&store, &future);
    store
        .capture(&CaptureRequest {
            request_id: id(),
            text: "GLOBAL_MEMORY_PRIVATE_MARKER".into(),
            origin: Origin::User {
                app: "Fixture".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap();
    let (config, requests, server) = fixture(1, |_, _| Response::stream(sse_text("用户想法")));
    assert!(
        store
            .generate_agent_title(&config, &target.input_id, &target.attempt_id)
            .await
            .unwrap()
    );
    server.join().unwrap();
    let requests = requests.lock().unwrap();
    let request = &requests[0];
    assert_eq!(request["stream"], true);
    assert!(
        request
            .get("tools")
            .is_none_or(|v| v.as_array().is_some_and(Vec::is_empty))
    );
    let messages = request["messages"].as_array().unwrap();
    assert!(
        messages
            .iter()
            .all(|m| m["role"] == "system" || m["role"] == "user")
    );
    let content = messages
        .iter()
        .filter(|m| m["role"] == "user")
        .map(|m| m["content"].as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    for included in &inputs[2..] {
        assert!(
            content.contains(included),
            "missing user expression {included}"
        );
    }
    assert!(content.contains(&"语".repeat(1000)));
    for excluded in [
        "OLD_EXCLUDED_PRIVATE_MARKER",
        "TRUNCATED_PRIVATE_MARKER",
        "ASSISTANT_PRIVATE_MARKER",
        "SUMMARY_PRIVATE_MARKER",
        "GLOBAL_MEMORY_PRIVATE_MARKER",
        "FUTURE_PRIVATE_MARKER",
    ] {
        assert!(!request.to_string().contains(excluded), "leaked {excluded}");
    }
}

#[tokio::test]
async fn title_failure_preserves_completed_answer_and_receipt_then_next_turn_retries() {
    let (_dir, store, conversation) = setup();
    let run = begin(&store, &conversation, "我决定每周投入八小时。");
    let receipt = commit_memory(&store, &run);
    finish(&store, &run);
    let answer = serde_json::to_value(store.turn(&run.input_id).unwrap()).unwrap();
    let memory =
        serde_json::to_value(store.memory(receipt.memory_id.as_ref().unwrap()).unwrap()).unwrap();
    let (config, requests, server) = fixture(2, |index, _| {
        if index == 0 {
            Response {
                status: 503,
                parts: vec!["unavailable".into()],
            }
        } else {
            Response::stream(sse_text("每周投入计划"))
        }
    });
    assert!(
        store
            .generate_agent_title(&config, &run.input_id, &run.attempt_id)
            .await
            .is_err()
    );
    assert_eq!(
        store.conversation(&conversation).unwrap().title,
        "Initial title"
    );
    assert!(!generated(&store, &conversation));
    assert_eq!(
        serde_json::to_value(store.turn(&run.input_id).unwrap()).unwrap(),
        answer
    );
    assert_eq!(
        store.agent_input_receipts(&run.input_id).unwrap(),
        vec![receipt.clone()]
    );
    assert_eq!(
        serde_json::to_value(store.memory(receipt.memory_id.as_ref().unwrap()).unwrap()).unwrap(),
        memory
    );
    let next = begin(&store, &conversation, "帮我安排这八小时。");
    finish(&store, &next);
    assert!(
        store
            .generate_agent_title(&config, &next.input_id, &next.attempt_id)
            .await
            .unwrap()
    );
    assert_eq!(
        store.conversation(&conversation).unwrap().title,
        "每周投入计划"
    );
    assert_eq!(
        serde_json::to_value(store.turn(&run.input_id).unwrap()).unwrap(),
        answer
    );
    server.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn invalid_title_output_never_marks_generation_complete() {
    let (_dir, store, conversation) = setup();
    let run = begin(&store, &conversation, "Discuss my weekly schedule.");
    finish(&store, &run);
    let invalid = [
        sse_text("   "),
        sse_text("First line\nSecond line"),
        sse_text(&"字".repeat(41)),
        sse_tool("unexpected", "write_memory", json!({})),
    ];
    let count = invalid.len();
    let (config, requests, server) = fixture(count, move |index, _| {
        Response::stream(invalid[index].clone())
    });
    for _ in 0..count {
        assert!(
            store
                .generate_agent_title(&config, &run.input_id, &run.attempt_id)
                .await
                .is_err()
        );
        assert!(!generated(&store, &conversation));
        assert_eq!(
            store.conversation(&conversation).unwrap().title,
            "Initial title"
        );
        assert_eq!(
            store.agent_execution(&run.input_id).unwrap().state,
            "complete"
        );
    }
    server.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), count);
}

#[tokio::test]
async fn simultaneous_title_responses_allow_exactly_one_save() {
    let (_dir, store, conversation) = setup();
    let run = begin(&store, &conversation, "Compare my options.");
    finish(&store, &run);
    let barrier = Arc::new(Barrier::new(2));
    let first_barrier = barrier.clone();
    let (first, first_requests, first_server) = fixture(1, move |_, _| {
        first_barrier.wait();
        Response::stream(sse_text("First generated title"))
    });
    let (second, second_requests, second_server) = fixture(1, move |_, _| {
        barrier.wait();
        Response::stream(sse_text("Second generated title"))
    });
    let (a, b) = tokio::join!(
        store.generate_agent_title(&first, &run.input_id, &run.attempt_id),
        store.generate_agent_title(&second, &run.input_id, &run.attempt_id),
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_ne!(a, b);
    assert_eq!(
        store.conversation(&conversation).unwrap().title,
        if a {
            "First generated title"
        } else {
            "Second generated title"
        }
    );
    assert!(generated(&store, &conversation));
    first_server.join().unwrap();
    second_server.join().unwrap();
    assert_eq!(first_requests.lock().unwrap().len(), 1);
    assert_eq!(second_requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn late_title_response_cannot_restore_a_deleted_conversation() {
    let (_dir, store, conversation) = setup();
    let run = begin(&store, &conversation, "Discuss this plan.");
    finish(&store, &run);
    let deleting_store = store.clone();
    let deleted = conversation.clone();
    let (config, requests, server) = fixture(1, move |_, _| {
        deleting_store.delete_conversation(&deleted).unwrap();
        Response::stream(sse_text("Arrived after deletion"))
    });
    assert!(
        !store
            .generate_agent_title(&config, &run.input_id, &run.attempt_id)
            .await
            .unwrap()
    );
    server.join().unwrap();
    assert!(store.conversation(&conversation).is_err());
    assert!(store.agent_execution(&run.input_id).is_err());
    assert!(store.conversations(100).unwrap().is_empty());
    assert_eq!(requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn processing_and_stale_attempts_do_not_issue_title_requests() {
    let (_dir, store, conversation) = setup();
    let run = begin(&store, &conversation, "Discuss this plan.");
    let (config, requests, server) =
        fixture(1, |_, _| Response::stream(sse_text("Unexpected title")));
    assert!(
        !store
            .generate_agent_title(&config, &run.input_id, &run.attempt_id)
            .await
            .unwrap()
    );
    store
        .stop_agent_input(&run.input_id, &run.attempt_id, "failed", Some("network"))
        .unwrap();
    let retry = store.retry_agent_input(&run.input_id, &id()).unwrap();
    finish(&store, &retry);
    assert!(
        !store
            .generate_agent_title(&config, &run.input_id, &run.attempt_id)
            .await
            .unwrap()
    );
    assert_eq!(
        store.conversation(&conversation).unwrap().title,
        "Initial title"
    );
    assert!(!generated(&store, &conversation));
    assert!(requests.lock().unwrap().is_empty());
    drop(server);
}
