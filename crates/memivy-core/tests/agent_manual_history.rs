use memivy_core::memory::*;
use serde_json::{Value, json};

#[path = "support/agent_fixture.rs"]
mod agent_fixture;
use agent_fixture::{Response, fixture, sse_text, sse_tool};

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn begin(store: &MemoryStore, conversation: &str, text: &str) -> AgentExecution {
    store
        .begin_agent_input(&id(), &id(), conversation, text, &[], None)
        .unwrap()
}
fn finish(store: &MemoryStore, run: &AgentExecution, text: &str) {
    store
        .append_agent_text(&run.input_id, &run.attempt_id, text)
        .unwrap();
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
}
fn content(request: &Value) -> Value {
    serde_json::from_str(
        request["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn saving_a_covered_answer_invalidates_summary_but_an_uncovered_answer_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let conversation = id();
    store
        .create_conversation(&conversation, "方案讨论")
        .unwrap();
    let first = begin(&store, &conversation, "分析方案甲。");
    finish(&store, &first, "方案甲需要先验证成本。");
    let second = begin(&store, &conversation, "分析方案乙。");
    finish(&store, &second, "方案乙需要先验证需求。");
    let current = begin(&store, &conversation, "继续讨论。");
    let before = store.agent_history_snapshot(&current.input_id).unwrap();
    let through = store.turn(&first.input_id).unwrap().assistant.seq;
    store
        .save_agent_summary(
            &current.input_id,
            &current.attempt_id,
            "方案甲尚未保存。",
            through,
            &before.context.receipt_revision,
        )
        .unwrap();

    let second_save = id();
    store
        .save_agent_text(
            &second_save,
            &second.input_id,
            "验证方案乙需求",
            "方案乙",
            &Destination::New,
        )
        .unwrap();
    let uncovered = store.agent_history_snapshot(&current.input_id).unwrap();
    assert_eq!(uncovered.context.summary, "方案甲尚未保存。");
    assert_eq!(uncovered.context.summary_through_seq, through);
    assert_eq!(uncovered.messages.len(), 2);
    assert_eq!(
        uncovered.messages[1]["manual_saves"]["items"][0]["input_id"],
        second_save
    );

    let first_save = id();
    store
        .save_agent_text(
            &first_save,
            &first.input_id,
            "验证方案甲成本",
            "方案甲",
            &Destination::New,
        )
        .unwrap();
    let covered = store.agent_history_snapshot(&current.input_id).unwrap();
    assert!(covered.context.summary.is_empty());
    assert_eq!(covered.context.summary_through_seq, 0);
    assert_eq!(covered.messages.len(), 4);
    assert_eq!(
        covered.messages[1]["manual_saves"]["items"][0]["input_id"],
        first_save
    );
    assert_eq!(
        covered.messages[3]["manual_saves"]["items"][0]["input_id"],
        second_save
    );
    assert_eq!(
        store
            .save_agent_summary(
                &current.input_id,
                &current.attempt_id,
                "迟到的尚未保存摘要",
                through,
                &uncovered.context.receipt_revision
            )
            .unwrap_err(),
        DataError::Conflict
    );

    store
        .save_agent_summary(
            &current.input_id,
            &current.attempt_id,
            "方案甲已手动保存。",
            through,
            &covered.context.receipt_revision,
        )
        .unwrap();
    store
        .save_agent_text(
            &first_save,
            &first.input_id,
            "验证方案甲成本",
            "方案甲",
            &Destination::New,
        )
        .unwrap();
    assert_eq!(
        store
            .agent_conversation_context(&conversation)
            .unwrap()
            .summary,
        "方案甲已手动保存。"
    );
    store.check_integrity().unwrap();
}

#[tokio::test]
async fn many_manual_saves_continue_normally_and_page_every_group_without_changing_message_cursor()
{
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let conversation = id();
    store
        .create_conversation(&conversation, "想法讨论")
        .unwrap();
    let first = begin(&store, &conversation, "帮我分析这个想法。");
    let answer = "可以先验证实际需求。";
    finish(&store, &first, answer);
    let mut saved_groups = Vec::new();
    for n in 0..160 {
        let request = id();
        store
            .save_agent_text(
                &request,
                &first.input_id,
                &format!("待验证想法 {n}"),
                "Idea",
                &Destination::New,
            )
            .unwrap();
        saved_groups.push(request);
    }
    let last_group = saved_groups.last().unwrap().clone();
    store.undo_agent_input(&id(), &last_group).unwrap();
    let seq = store.turn(&first.input_id).unwrap().assistant.seq;
    let current = begin(&store, &conversation, "继续查看这条回答的保存记录。");
    let snapshot = store.agent_history_snapshot(&current.input_id).unwrap();
    assert_eq!(snapshot.messages.len(), 2);
    assert!(serde_json::to_vec(&snapshot.messages).unwrap().len() < 6000);
    let message_id = first.assistant_message_id.clone();
    let mut seen = Vec::new();
    let (config, requests, server) = fixture(10, move |index, request| {
        if index == 9 {
            return Response::stream(sse_text(
                "[\"哪些想法还需要验证？\",\"继续讨论最早的想法\"]",
            ));
        }
        let page = if index == 0 {
            let initial = content(request);
            let message = &initial["recent_messages"][1];
            assert_eq!(message["id"], message_id);
            assert!(
                request["messages"][0]["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("manual_saves_offset")
            );
            message["manual_saves"].clone()
        } else {
            let result = content(request);
            assert_eq!(result["messages"].as_array().unwrap().len(), 1);
            let message = &result["messages"][0];
            assert_eq!(message["id"], message_id);
            assert_eq!(message["seq"], seq);
            assert_eq!(message["text"], answer);
            assert_eq!(result["next_after_seq"], seq);
            message["manual_saves"].clone()
        };
        assert_eq!(page["total"], 160);
        let items = page["items"].as_array().unwrap();
        if index == 8 {
            assert!(items.is_empty());
            assert!(page["next_offset"].is_null());
            assert_eq!(seen, saved_groups);
            return Response::stream(sse_text("已读到全部 160 条保存记录，最后一条已经撤销。"));
        }
        assert_eq!(items.len(), 20);
        for (position, item) in items.iter().enumerate() {
            let expected = index * 20 + position;
            assert_eq!(item["input_id"], saved_groups[expected]);
            assert_eq!(
                item["status"],
                if expected == 159 { "undone" } else { "applied" }
            );
            assert!(item["memory_id"].is_string());
            seen.push(item["input_id"].as_str().unwrap().to_owned());
        }
        if index < 7 {
            assert_eq!(page["next_offset"], (index + 1) * 20);
        } else {
            assert!(page["next_offset"].is_null());
        }
        Response::stream(sse_tool(
            &format!("page-{}", index + 1),
            "read_conversation",
            json!({"conversation_id":conversation,"after_seq":seq-1,"limit":1,"manual_saves_offset":(index+1)*20}),
        ))
    });

    store
        .run_discussion(
            &config,
            &current.input_id,
            &current.attempt_id,
            "zh-CN",
            |_| {},
        )
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 10);
    assert_eq!(
        store.agent_execution(&current.input_id).unwrap().state,
        "complete"
    );
    assert_eq!(
        store.agent_input_receipts(&last_group).unwrap()[0].status,
        "undone"
    );
    assert!(
        store
            .agent_input_receipts(&current.input_id)
            .unwrap()
            .is_empty()
    );
    store.check_integrity().unwrap();
}

#[tokio::test]
async fn manual_save_undo_invalidates_summary_and_remains_a_distinct_group_in_history_and_pages() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let conversation = id();
    store
        .create_conversation(&conversation, "方案讨论")
        .unwrap();
    let first = begin(&store, &conversation, "帮我比较甲乙两个方案。");
    finish(&store, &first, "可以先比较成本，再验证需求。");
    let manual = id();
    let saved = store
        .save_agent_text(
            &manual,
            &first.input_id,
            "比较成本和需求",
            "备选方案",
            &Destination::New,
        )
        .unwrap();
    let first_turn = store.turn(&first.input_id).unwrap();
    let second = begin(&store, &conversation, "继续分析这个想法。");
    let before = store.agent_history_snapshot(&second.input_id).unwrap();
    assert_eq!(
        before.messages[1]["manual_saves"]["items"][0]["input_id"],
        manual
    );
    assert_eq!(
        before.messages[1]["manual_saves"]["items"][0]["status"],
        "applied"
    );
    store
        .save_agent_summary(
            &second.input_id,
            &second.attempt_id,
            "已手动保存比较成本和需求的记忆",
            first_turn.assistant.seq,
            &before.context.receipt_revision,
        )
        .unwrap();
    let result = store.undo_agent_input(&id(), &manual).unwrap();
    assert!(result.receipt.is_some() && result.conflicts.is_empty());
    assert_eq!(
        store.memory(saved.memory_id.as_ref().unwrap()).unwrap_err(),
        DataError::Unavailable
    );
    let after = store.agent_history_snapshot(&second.input_id).unwrap();
    assert!(after.context.summary.is_empty());
    assert_eq!(after.context.summary_through_seq, 0);
    assert_ne!(
        before.context.receipt_revision,
        after.context.receipt_revision
    );
    assert_eq!(after.messages.len(), 2);
    assert_eq!(
        after.messages[1]["manual_saves"]["items"][0]["input_id"],
        manual
    );
    assert_eq!(
        after.messages[1]["manual_saves"]["items"][0]["status"],
        "undone"
    );
    assert!(
        after.messages[0]["manual_saves"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(after.messages[1]["logical_input_id"], first.input_id);
    // The manual save must not get folded into the automatic turn's UI undo.
    assert!(
        store
            .turn(&first.input_id)
            .unwrap()
            .assistant
            .receipts
            .is_empty()
    );
    assert_eq!(
        store
            .save_agent_summary(
                &second.input_id,
                &second.attempt_id,
                "迟到的已保存摘要",
                first_turn.assistant.seq,
                &before.context.receipt_revision
            )
            .unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(
        store.agent_execution(&second.input_id).unwrap().state,
        "processing"
    );
    finish(&store, &second, "手动保存已撤销，可以继续比较。");

    let current = begin(&store, &conversation, "回读第一轮的保存状态。");
    let wanted_message = first.assistant_message_id.clone();
    let group = manual.clone();
    let (config, requests, server) = fixture(3, move |index, request| {
        Response::stream(match index {
            0 => {
                let initial = content(request);
                let message = initial["recent_messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|m| m["id"] == wanted_message)
                    .unwrap();
                assert_eq!(message["manual_saves"]["items"][0]["input_id"], group);
                assert_eq!(message["manual_saves"]["items"][0]["status"], "undone");
                assert!(
                    request["messages"][0]["content"][0]["text"]
                        .as_str()
                        .unwrap()
                        .contains("manual_saves")
                );
                sse_tool(
                    "read-first-answer",
                    "read_conversation",
                    json!({"conversation_id":conversation,"after_seq":first_turn.user.seq,"limit":1}),
                )
            }
            1 => {
                let page = content(request);
                assert_eq!(page["messages"].as_array().unwrap().len(), 1);
                assert_eq!(page["messages"][0]["id"], wanted_message);
                assert_eq!(
                    page["messages"][0]["manual_saves"]["items"][0]["input_id"],
                    group
                );
                assert_eq!(
                    page["messages"][0]["manual_saves"]["items"][0]["status"],
                    "undone"
                );
                assert!(
                    page["messages"][0]["receipts"]
                        .as_array()
                        .unwrap()
                        .is_empty()
                );
                assert_eq!(page["next_after_seq"], first_turn.assistant.seq);
                sse_text("第一轮手动保存已经撤销，原讨论仍保留。")
            }
            _ => sse_text("[\"帮我比较两个方案的成本\",\"哪些假设还需要验证？\"]"),
        })
    });

    store
        .run_discussion(
            &config,
            &current.input_id,
            &current.attempt_id,
            "zh-CN",
            |_| {},
        )
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 3);
    assert_eq!(
        store.agent_execution(&current.input_id).unwrap().state,
        "complete"
    );
    assert_eq!(
        store.agent_input_receipts(&manual).unwrap()[0].status,
        "undone"
    );
    store.check_integrity().unwrap();
}
