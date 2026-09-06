use memivy_core::memory::*;
use rusqlite::{Connection, params};
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
            app: "测试".into(),
            project: None,
            uri: None,
        },
    }
}
fn new_memory(store: &MemoryStore, text: &str) -> (RawCapture, Receipt) {
    let c = store.capture(&capture_request(text)).unwrap();
    let receipt = store
        .apply_capture(&ChangeRequest {
            request_id: id(),
            capture_id: c.id.clone(),
            destination: Destination::New,
            title: "首次体验".into(),
            body: text.into(),
            actor: Actor::Ai,
        })
        .unwrap();
    (c, receipt)
}
fn edit(store: &MemoryStore, r: &Receipt, text: &str) -> Receipt {
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: r.memory_id.clone().unwrap(),
            expected_version: r.after_version.clone().unwrap(),
            title: "修改后".into(),
            body: text.into(),
        })
        .unwrap()
}
fn finished_turn(store: &MemoryStore, evidence: &[SourceRef]) -> (String, Turn) {
    let conversation = id();
    store
        .create_conversation(&conversation, "继续讨论")
        .unwrap();
    let turn = store
        .start_turn(&id(), &conversation, "只是一个假设", evidence)
        .unwrap();
    assert!(
        store
            .finish_turn(&turn.id, "AI 建议，不自动保存", evidence)
            .unwrap()
    );
    (conversation, store.turn(&turn.id).unwrap())
}
fn conclusion(turn: &Turn, destination: Destination) -> ConclusionRequest {
    ConclusionRequest {
        request_id: id(),
        message_id: turn.assistant.id.clone(),
        destination,
        title: "用户确认的名称".into(),
        text: "  确认后修改的结论\n逐字保留🙂  ".into(),
    }
}

#[test]
fn raw_and_versions_are_immutable_exact_and_request_scoped() {
    let (_dir, store) = setup();
    let request = capture_request(" \n保留空格与中文：_% OR ` ``` 🙂\n ");
    let c = store.capture(&request).unwrap();
    assert_eq!(c.text, request.text);
    assert_eq!(c.id, store.capture(&request).unwrap().id);
    let mut conflicting = request.clone();
    conflicting.text = "different".into();
    assert_eq!(
        store.capture(&conflicting).unwrap_err(),
        DataError::RequestConflict
    );
    let identical_text_new_request = store.capture(&capture_request(&request.text)).unwrap();
    assert_ne!(
        identical_text_new_request.id, c.id,
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
        .apply_capture(&ChangeRequest {
            request_id: id(),
            capture_id: c.id.clone(),
            destination: Destination::New,
            title: "版本一".into(),
            body: "当前理解".into(),
            actor: Actor::Ai,
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
    let stale = ChangeRequest {
        request_id: id(),
        capture_id: c.id.clone(),
        destination: Destination::Existing {
            memory_id: memory.clone(),
            expected_version: r.after_version.clone().unwrap(),
        },
        title: "迟到的 AI".into(),
        body: "不得覆盖".into(),
        actor: Actor::Ai,
    };
    assert_eq!(
        store.apply_capture(&stale).unwrap_err(),
        DataError::Conflict
    );
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
fn undo_new_memory_keeps_raw_and_correction_is_atomic() {
    let (_dir, store) = setup();
    let (c, r) = new_memory(&store, "应归到其他地方");
    let (_, target) = new_memory(&store, "目标原文");
    let correction = ChangeRequest {
        request_id: id(),
        capture_id: c.id.clone(),
        destination: Destination::Existing {
            memory_id: target.memory_id.clone().unwrap(),
            expected_version: id(),
        },
        title: "更正".into(),
        body: "新归属".into(),
        actor: Actor::User,
    };
    assert_eq!(
        store
            .correct_assignment(&r.request_id, &correction)
            .unwrap_err(),
        DataError::Conflict
    );
    assert!(
        store.memory(r.memory_id.as_ref().unwrap()).is_ok(),
        "failed correction must not undo the source"
    );
    let correction = ChangeRequest {
        destination: Destination::Existing {
            memory_id: target.memory_id.clone().unwrap(),
            expected_version: target.after_version.clone().unwrap(),
        },
        ..correction
    };
    let corrected = store
        .correct_assignment(&r.request_id, &correction)
        .unwrap();
    assert_eq!(
        corrected,
        store
            .correct_assignment(&r.request_id, &correction)
            .unwrap()
    );
    assert!(store.memory(r.memory_id.as_ref().unwrap()).is_err());
    assert_eq!(store.capture_by_id(&c.id).unwrap().text, "应归到其他地方");
    let current = store
        .memory(target.memory_id.as_ref().unwrap())
        .unwrap()
        .current;
    assert_eq!(current.capture_ids.len(), 2);
    store.undo(&id(), &corrected.request_id).unwrap();
    assert_eq!(
        store.capture_by_id(&c.id).unwrap().understanding,
        "attached"
    );
    assert_eq!(
        store
            .memory(r.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        "应归到其他地方"
    );
    assert_eq!(
        store.receipt_changes(&corrected.request_id).unwrap().len(),
        2
    );
    assert_eq!(
        store
            .memory(target.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        "目标原文"
    );
    assert!(
        store
            .search("应归到其他地方", 20)
            .unwrap()
            .iter()
            .any(|e| matches!(e.source, SourceRef::Version(_)))
    );
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
    assert!(store.search("", 50).unwrap().is_empty());
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
fn successive_corrections_do_not_restore_old_mistakes_and_undo_checks_both_memories() {
    let (_dir, store) = setup();
    let (capture, a) = new_memory(&store, "最初误归属");
    let (_, b) = new_memory(&store, "B 的原正文");
    let (_, c) = new_memory(&store, "C 的原正文");
    let move_to = |receipt: &Receipt, target: &Receipt| {
        store
            .correct_assignment(
                &receipt.request_id,
                &ChangeRequest {
                    request_id: id(),
                    capture_id: capture.id.clone(),
                    destination: Destination::Existing {
                        memory_id: target.memory_id.clone().unwrap(),
                        expected_version: target.after_version.clone().unwrap(),
                    },
                    title: "纠正归属".into(),
                    body: "纠正后的当前正文".into(),
                    actor: Actor::User,
                },
            )
            .unwrap()
    };
    let first = move_to(&a, &b);
    let second = move_to(&first, &c);
    assert!(store.memory(a.memory_id.as_ref().unwrap()).is_err());
    assert_eq!(
        store
            .memory(b.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        "B 的原正文"
    );
    let b_head = store
        .memory(b.memory_id.as_ref().unwrap())
        .unwrap()
        .current
        .id;
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: b.memory_id.clone().unwrap(),
            expected_version: b_head,
            title: "B 后来又编辑了".into(),
            body: "后续编辑必须保留".into(),
        })
        .unwrap();
    assert_eq!(
        store.undo(&id(), &second.request_id).unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(
        store
            .memory(c.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .id,
        second.after_version.unwrap()
    );
    assert_eq!(
        store
            .memory(b.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        "后续编辑必须保留"
    );
    store.check_integrity().unwrap();
}

#[test]
fn undoing_a_new_assignment_returns_the_exact_raw_to_pending() {
    let (_dir, store) = setup();
    let (capture, r) = new_memory(&store, "撤销后仍能找到的原话");
    store.undo(&id(), &r.request_id).unwrap();
    assert_eq!(
        store.capture_by_id(&capture.id).unwrap().understanding,
        "pending"
    );
    assert_eq!(
        store.search("仍能找到", 20).unwrap()[0].source,
        SourceRef::Capture(capture.id)
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
        assert_eq!(store.search(query, 10).unwrap().len(), 1, "{query}");
    }
    assert!(store.search("不存在", 10).unwrap().is_empty());
    let r = store
        .apply_capture(&ChangeRequest {
            request_id: id(),
            capture_id: raw.id.clone(),
            destination: Destination::New,
            title: "首次体验".into(),
            body: "另一种当前理解".into(),
            actor: Actor::Ai,
        })
        .unwrap();
    let hits = store.search("首次体验 中文短语", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].source,
        SourceRef::Version(r.after_version.clone().unwrap())
    );
    let edited = edit(&store, &r, "修改后理解");
    assert!(
        store.search("另一种当前理解", 10).unwrap().is_empty(),
        "superseded summaries must not masquerade as current memories"
    );
    store
        .trash_memory(
            r.memory_id.as_ref().unwrap(),
            edited.after_version.as_ref().unwrap(),
        )
        .unwrap();
    assert!(store.search("中文短语", 10).unwrap().is_empty());
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
    let b = store
        .apply_capture(&ChangeRequest {
            request_id: id(),
            capture_id: c.id.clone(),
            destination: Destination::New,
            title: "另一条记忆".into(),
            body: "第二条".into(),
            actor: Actor::User,
        })
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
    store.trash_capture(&c.id).unwrap();
    store
        .trash_memory(
            a.memory_id.as_ref().unwrap(),
            a.after_version.as_ref().unwrap(),
        )
        .unwrap();
    store.restore_memory(a.memory_id.as_ref().unwrap()).unwrap();
    assert!(
        store.capture_by_id(&c.id).is_err(),
        "restoring a memory must not restore independently deleted raw input"
    );
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
    assert!(store.restore_capture(&c.id).is_err());
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
    assert!(store.search("", 50).unwrap().is_empty());
    store.check_integrity().unwrap();
}

#[test]
fn purging_raw_cleans_undone_versions_without_erasing_other_memories() {
    let (dir, store) = setup();
    let sentinel = "ERASE_UNDONE_VERSION_AND_INDEX_20260906";
    let (c, abandoned) = new_memory(&store, sentinel);
    let mut shared = Vec::new();
    for body in ["有效的共享记忆", "回收站的共享记忆"] {
        shared.push(
            store
                .apply_capture(&ChangeRequest {
                    request_id: id(),
                    capture_id: c.id.clone(),
                    destination: Destination::New,
                    title: "共享来源".into(),
                    body: body.into(),
                    actor: Actor::User,
                })
                .unwrap(),
        );
    }
    store
        .trash_memory(
            shared[1].memory_id.as_ref().unwrap(),
            shared[1].after_version.as_ref().unwrap(),
        )
        .unwrap();
    store.undo(&id(), &abandoned.request_id).unwrap();
    // Undo itself must retain the raw input and version for provenance.
    assert_eq!(store.capture_by_id(&c.id).unwrap().text, sentinel);
    let db = Connection::open(store.database_path()).unwrap();
    let hidden = abandoned.after_version.as_ref().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT body FROM memory_versions WHERE id=?",
            [hidden],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        sentinel
    );
    store.trash_capture(&c.id).unwrap();
    // Failure halfway through cascading erasure must roll back the raw too.
    db.execute_batch("CREATE TRIGGER reject_erasure BEFORE UPDATE ON memory_versions BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    assert_eq!(store.purge_capture(&c.id).unwrap_err(), DataError::Database);
    assert_eq!(
        db.query_row("SELECT text FROM captures WHERE id=?", [&c.id], |r| r
            .get::<_, String>(0))
            .unwrap(),
        sentinel
    );
    db.execute_batch("DROP TRIGGER reject_erasure").unwrap();
    store.purge_capture(&c.id).unwrap();
    store.purge_capture(&c.id).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT body FROM memory_versions WHERE id=?",
            [hidden],
            |r| r.get::<_, Option<String>>(0)
        )
        .unwrap(),
        None
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM record_fts WHERE source_id=?",
            [hidden],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        store
            .memory(shared[0].memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        "有效的共享记忆"
    );
    assert_eq!(
        store
            .history(shared[1].memory_id.as_ref().unwrap())
            .unwrap()[0]
            .body,
        "回收站的共享记忆"
    );
    assert!(store.restore_capture(&c.id).is_err());
    assert!(
        store
            .restore_memory(abandoned.memory_id.as_ref().unwrap())
            .is_err()
    );
    let backup = dir.path().join("after-purge.sqlite3");
    store.backup(&backup).unwrap();
    assert!(!String::from_utf8_lossy(&std::fs::read(backup).unwrap()).contains(sentinel));
    store.check_integrity().unwrap();
}

#[test]
fn purging_a_corrected_memory_also_erases_its_abandoned_assignment() {
    let (dir, store) = setup();
    let sentinel = "ERASE_ABANDONED_ASSIGNMENT_20260906";
    let (c, original) = new_memory(&store, sentinel);
    let corrected = store
        .correct_assignment(
            &original.request_id,
            &ChangeRequest {
                request_id: id(),
                capture_id: c.id.clone(),
                destination: Destination::New,
                title: "正确归属".into(),
                body: sentinel.into(),
                actor: Actor::User,
            },
        )
        .unwrap();
    store
        .trash_memory(
            corrected.memory_id.as_ref().unwrap(),
            corrected.after_version.as_ref().unwrap(),
        )
        .unwrap();
    store
        .purge_memory(corrected.memory_id.as_ref().unwrap())
        .unwrap();
    let backup = dir.path().join("after-purge.sqlite3");
    store.backup(&backup).unwrap();
    assert!(!String::from_utf8_lossy(&std::fs::read(backup).unwrap()).contains(sentinel));
    assert!(store.undo(&id(), &corrected.request_id).is_err());
    assert!(store.restore_capture(&c.id).is_err());
    store.check_integrity().unwrap();
}

#[test]
fn withdrawn_memories_can_be_explicitly_purged_without_losing_retained_raw() {
    for purge_raw_first in [false, true] {
        let (_dir, store) = setup();
        let (capture, created) = new_memory(&store, "保留原话，清除撤销的版本");
        let memory = created.memory_id.as_ref().unwrap();
        assert_eq!(store.purge_memory(memory).unwrap_err(), DataError::Conflict);
        if purge_raw_first {
            store.trash_capture(&capture.id).unwrap();
            store.purge_capture(&capture.id).unwrap();
        }
        store.undo(&id(), &created.request_id).unwrap();
        store.purge_memory(memory).unwrap();
        store.purge_memory(memory).unwrap();
        let db = Connection::open(store.database_path()).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM memory_versions WHERE memory_id=? AND body IS NOT NULL",
                [memory],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        if purge_raw_first {
            assert!(store.capture_by_id(&capture.id).is_err());
        } else {
            let raw = store.capture_by_id(&capture.id).unwrap();
            assert_eq!(raw.text, capture.text);
            assert_eq!(raw.understanding, "pending");
        }
        store.check_integrity().unwrap();
    }
}

#[test]
fn undoing_corrections_restores_an_actionable_assignment_receipt() {
    for existing_source in [false, true] {
        for existing_target in [false, true] {
            let (_dir, store) = setup();
            let (capture, initial) = new_memory(&store, "待纠正的原话");
            let mut assignment = if existing_source {
                let (_, target) = new_memory(&store, "来源记忆的原正文");
                store
                    .correct_assignment(
                        &initial.request_id,
                        &ChangeRequest {
                            request_id: id(),
                            capture_id: capture.id.clone(),
                            destination: Destination::Existing {
                                memory_id: target.memory_id.unwrap(),
                                expected_version: target.after_version.unwrap(),
                            },
                            title: "来源记忆".into(),
                            body: "来源记忆加原话".into(),
                            actor: Actor::User,
                        },
                    )
                    .unwrap()
            } else {
                initial
            };
            let source = assignment.memory_id.clone().unwrap();
            let source_body = store.memory(&source).unwrap().current.body;
            let mut edited_targets: Vec<String> = Vec::new();
            for _ in 0..3 {
                let target = existing_target.then(|| new_memory(&store, "目标的原正文").1);
                let request = ChangeRequest {
                    request_id: id(),
                    capture_id: capture.id.clone(),
                    destination: target.as_ref().map_or(Destination::New, |t| {
                        Destination::Existing {
                            memory_id: t.memory_id.clone().unwrap(),
                            expected_version: t.after_version.clone().unwrap(),
                        }
                    }),
                    title: "纠正".into(),
                    body: "目标加原话".into(),
                    actor: Actor::User,
                };
                let moved = store
                    .correct_assignment(&assignment.request_id, &request)
                    .unwrap();
                let undo_request = id();
                let restored = store.undo(&undo_request, &moved.request_id).unwrap();
                assert_eq!(
                    restored,
                    store.undo(&undo_request, &moved.request_id).unwrap()
                );
                assert_eq!(restored.memory_id.as_ref(), Some(&source));
                assert_eq!(
                    restored.after_version.as_ref(),
                    Some(&store.memory(&source).unwrap().current.id)
                );
                assert_eq!(store.memory(&source).unwrap().current.body, source_body);
                for target in &edited_targets {
                    assert_eq!(store.memory(target).unwrap().current.body, "保留目标新内容");
                }
                // Later edits on the departed target must not block another
                // correction of the restored source or get undone by it.
                if let Some(target) = target {
                    let current = store
                        .memory(target.memory_id.as_ref().unwrap())
                        .unwrap()
                        .current;
                    store
                        .edit_memory(&EditRequest {
                            request_id: id(),
                            memory_id: current.memory_id.clone(),
                            expected_version: current.id,
                            title: "目标后来编辑".into(),
                            body: "保留目标新内容".into(),
                        })
                        .unwrap();
                    edited_targets.push(current.memory_id);
                }
                assignment = restored;
            }
            let previous_head = store.memory(&source).unwrap().current.id;
            store
                .edit_memory(&EditRequest {
                    request_id: id(),
                    memory_id: source.clone(),
                    expected_version: previous_head,
                    title: "来源后来编辑".into(),
                    body: "不覆盖来源新内容".into(),
                })
                .unwrap();
            assert_eq!(
                store
                    .correct_assignment(
                        &assignment.request_id,
                        &ChangeRequest {
                            request_id: id(),
                            capture_id: capture.id,
                            destination: Destination::New,
                            title: "迟到纠正".into(),
                            body: "不能覆盖".into(),
                            actor: Actor::User,
                        }
                    )
                    .unwrap_err(),
                DataError::Conflict
            );
            assert_eq!(
                store.memory(&source).unwrap().current.body,
                "不覆盖来源新内容"
            );
            store.check_integrity().unwrap();
        }
    }
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
fn discussions_and_drafts_are_not_memories_and_confirmation_retains_provenance() {
    let (_dir, store) = setup();
    let (conversation, turn) = finished_turn(&store, &[]);
    store
        .save_conversation_draft(&conversation, "尚未确定的草稿")
        .unwrap();
    assert!(store.search("", 50).unwrap().is_empty());
    let request = conclusion(&turn, Destination::New);
    let r = store.save_conclusion(&request).unwrap();
    let raw = store.capture_by_id(r.capture_id.as_ref().unwrap()).unwrap();
    assert_eq!(raw.text, request.text);
    assert!(
        matches!(raw.origin,Origin::Conversation{message_role,confirmed_by,..} if message_role=="assistant" && confirmed_by=="user")
    );
    store.delete_conversation(&conversation).unwrap();
    assert_eq!(
        store.save_conclusion(&request).unwrap(),
        r,
        "delivery retry works after deleting conversation"
    );
    assert!(store.conversation(&conversation).is_err());
    assert_eq!(
        store
            .memory(r.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        request.text
    );
    let mut bad = request.clone();
    bad.text = "different".into();
    assert_eq!(
        store.save_conclusion(&bad).unwrap_err(),
        DataError::RequestConflict
    );
    let undo = id();
    store.undo(&undo, &r.request_id).unwrap();
    assert_eq!(store.save_conclusion(&request).unwrap().status, "undone");
    assert!(store.memory(r.memory_id.as_ref().unwrap()).is_err());
    assert_eq!(
        store
            .capture_by_id(r.capture_id.as_ref().unwrap())
            .unwrap()
            .text,
        request.text
    );
    store.check_integrity().unwrap();
}

#[test]
fn stale_conclusion_target_keeps_exact_text_title_and_destination_for_correction() {
    let (_dir, store) = setup();
    let (_, target) = new_memory(&store, "旧版");
    let (_, turn) = finished_turn(&store, &[]);
    edit(&store, &target, "已改版");
    let destination = Destination::Existing {
        memory_id: target.memory_id.clone().unwrap(),
        expected_version: target.after_version.clone().unwrap(),
    };
    let request = conclusion(&turn, destination.clone());
    let r = store.save_conclusion(&request).unwrap();
    assert_eq!(r.status, "needs_review");
    let capture = r.capture_id.clone().unwrap();
    assert_eq!(store.capture_by_id(&capture).unwrap().text, request.text);
    assert_eq!(
        store.conclusion_intent(&capture).unwrap(),
        (request.title.clone(), destination)
    );
    assert_eq!(
        store
            .memory(target.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        "已改版"
    );
    let corrected = store
        .correct_assignment(
            &r.request_id,
            &ChangeRequest {
                request_id: id(),
                capture_id: capture,
                destination: Destination::New,
                title: request.title,
                body: request.text,
                actor: Actor::User,
            },
        )
        .unwrap();
    assert!(store.memory(corrected.memory_id.as_ref().unwrap()).is_ok());
    let missing = conclusion(
        &turn,
        Destination::Existing {
            memory_id: id(),
            expected_version: id(),
        },
    );
    assert_eq!(
        store.save_conclusion(&missing).unwrap().status,
        "needs_review"
    );
}

#[test]
fn citations_bind_to_fixed_versions_and_deleted_sources_are_unavailable() {
    let (_dir, store) = setup();
    let (c, r) = new_memory(&store, "当时的依据");
    let old = SourceRef::Version(r.after_version.clone().unwrap());
    let (_, turn) = finished_turn(&store, std::slice::from_ref(&old));
    let saved = store
        .save_conclusion(&conclusion(&turn, Destination::New))
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
fn cancellation_recovery_and_invalid_citations_cannot_complete_late_or_write_memory() {
    let (_dir, store) = setup();
    let conversation = id();
    store
        .create_conversation(&conversation, "失败恢复")
        .unwrap();
    let r = id();
    let turn = store.start_turn(&r, &conversation, "第一次", &[]).unwrap();
    assert_eq!(
        store
            .start_turn(&r, &conversation, "第一次", &[])
            .unwrap()
            .assistant
            .id,
        turn.assistant.id
    );
    assert_eq!(
        store
            .start_turn(&id(), &conversation, "第二次", &[])
            .unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(
        store
            .finish_turn(&r, "虚假引用", &[SourceRef::Capture(id())])
            .unwrap_err(),
        DataError::Invalid
    );
    store.cancel_turn(&r).unwrap();
    assert!(!store.finish_turn(&r, "迟到答案", &[]).unwrap());
    let request = conclusion(&turn, Destination::New);
    assert_eq!(
        store.save_conclusion(&request).unwrap_err(),
        DataError::Conflict
    );
    let next = store.start_turn(&id(), &conversation, "重试", &[]).unwrap();
    let reopened = MemoryStore::open(store.database_path().parent().unwrap()).unwrap();
    assert_eq!(
        reopened.turn(&next.id).unwrap().assistant.status,
        "processing",
        "another store reader must not cancel a live request"
    );
    assert_eq!(reopened.recover_interrupted_turns().unwrap(), 1);
    assert!(!store.finish_turn(&next.id, "跨重启迟到答案", &[]).unwrap());
    assert!(store.search("", 50).unwrap().is_empty());
    let (c, _) = new_memory(&store, "处理时删除");
    let reference = SourceRef::Capture(c.id.clone());
    let next = store
        .start_turn(
            &id(),
            &conversation,
            "引用会失效",
            std::slice::from_ref(&reference),
        )
        .unwrap();
    store.trash_capture(&c.id).unwrap();
    assert!(
        !store
            .finish_turn(&next.id, "不应保存为完成", &[reference])
            .unwrap()
    );
    assert_eq!(
        store
            .turn(&next.id)
            .unwrap()
            .assistant
            .error_code
            .as_deref(),
        Some("source_unavailable")
    );
}

#[test]
fn conversation_cursor_reads_do_not_drop_old_messages() {
    let (_dir, store) = setup();
    let conversation = id();
    store.create_conversation(&conversation, "分页").unwrap();
    for _ in 0..55 {
        let t = store.start_turn(&id(), &conversation, "问题", &[]).unwrap();
        store.finish_turn(&t.id, "回答", &[]).unwrap();
    }
    let first = store.messages(&conversation, 0, 100).unwrap();
    assert_eq!(first.len(), 100);
    let second = store
        .messages(&conversation, first.last().unwrap().seq, 100)
        .unwrap();
    assert_eq!(second.len(), 10);
    assert!(second[0].seq > first.last().unwrap().seq);
    assert_eq!(first[0].role, "user");
    assert!(store.search("", 50).unwrap().is_empty());
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
                s.capture(&r).unwrap().id
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
                    title: "并发".into(),
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
fn formal_migration_preserves_v1_and_rejects_prototype_future_and_partial_migrations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memivy.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../../../migrations/memory/001_records.sql"))
        .unwrap();
    db.pragma_update(None, "application_id", 0x4d495659_i64)
        .unwrap();
    let capture = id();
    db.execute("INSERT INTO captures(id,request_id,fingerprint,text,source,created_at) VALUES(?1,?2,x'01','迁移保留',?3,1)",params![capture,id(),serde_json::to_string(&Origin::User{app:"v1".into(),project:None,uri:None}).unwrap()]).unwrap();
    db.execute(
        "INSERT INTO capture_state(capture_id) VALUES(?)",
        [&capture],
    )
    .unwrap();
    // A failing later DDL must roll back every earlier statement in that migration.
    db.execute_batch("CREATE TABLE turns (broken TEXT)")
        .unwrap();
    assert!(MemoryStore::open(dir.path()).is_err());
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='conversations'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    db.execute_batch("DROP TABLE turns").unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    assert_eq!(store.capture_by_id(&capture).unwrap().text, "迁移保留");
    store.check_integrity().unwrap();
    db.pragma_update(None, "user_version", 999_i64).unwrap();
    assert_eq!(
        MemoryStore::open(dir.path()).unwrap_err(),
        DataError::Schema
    );
    assert_eq!(
        store.capture(&capture_request("不能写新版库")).unwrap_err(),
        DataError::Schema
    );
    let prototype = tempfile::tempdir().unwrap();
    std::fs::write(prototype.path().join("phase1.sqlite3"), "do not touch").unwrap();
    assert_eq!(
        MemoryStore::open(prototype.path()).unwrap_err(),
        DataError::Schema
    );
    assert!(!prototype.path().join("memivy.db").exists());
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
    let edit = edit(&store, &r, "包含历史");
    let (conversation, turn) = finished_turn(
        &store,
        &[SourceRef::Version(r.after_version.clone().unwrap())],
    );
    let request = conclusion(&turn, Destination::New);
    let saved = store.save_conclusion(&request).unwrap();
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
    let restored = MemoryStore::restore_backup(&backup, &restore).unwrap();
    assert!(!restored.model_config_path().exists());
    assert!(!restore.join("mcp.json").exists());
    assert_eq!(
        restored.conversation(&conversation).unwrap().title,
        "继续讨论"
    );
    assert_eq!(restored.save_conclusion(&request).unwrap(), saved);
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
    assert_eq!(
        MemoryStore::restore_backup(&backup, &restore).unwrap_err(),
        DataError::DestinationExists
    );
    let invalid = dir.path().join("invalid.sqlite3");
    std::fs::write(&invalid, "not a database").unwrap();
    assert!(MemoryStore::restore_backup(&invalid, dir.path().join("bad-restore")).is_err());
    assert!(!dir.path().join("bad-restore/memivy.db").exists());
    restored.check_integrity().unwrap();
}

#[test]
fn markdown_export_preserves_content_versions_roles_and_unavailable_citations() {
    let (dir, store) = setup();
    let (c, r) = new_memory(&store, "  原文\n```\n含反引号与空白🙂  ");
    let edited = edit(&store, &r, "编辑的正文");
    let (_, turn) = finished_turn(&store, &[SourceRef::Capture(c.id.clone())]);
    let request = conclusion(&turn, Destination::New);
    store.save_conclusion(&request).unwrap();
    let sentinel = "SECRET_CONFIG_DO_NOT_EXPORT";
    std::fs::write(store.model_config_path(), sentinel).unwrap();
    let export = dir.path().join("export");
    store.export_markdown(&export).unwrap();
    let captures = std::fs::read_to_string(export.join("captures.md")).unwrap();
    assert!(captures.contains(&c.text));
    assert!(captures.contains("confirmed_by"));
    assert!(captures.contains(&request.title));
    let memories = std::fs::read_to_string(export.join("memories.md")).unwrap();
    assert!(memories.contains(r.after_version.as_ref().unwrap()));
    assert!(memories.contains(edited.after_version.as_ref().unwrap()));
    let chats = std::fs::read_to_string(export.join("conversations.md")).unwrap();
    assert!(chats.contains("不是长期记忆"));
    assert!(chats.contains("角色：assistant"));
    for entry in std::fs::read_dir(&export).unwrap() {
        assert!(
            !std::fs::read_to_string(entry.unwrap().path())
                .unwrap()
                .contains(sentinel)
        );
    }
    assert_eq!(
        store.export_markdown(&export).unwrap_err(),
        DataError::DestinationExists
    );
    store.trash_capture(&c.id).unwrap();
    let export = dir.path().join("deleted-source-export");
    store.export_markdown(&export).unwrap();
    assert!(
        std::fs::read_to_string(export.join("conversations.md"))
            .unwrap()
            .contains("来源已删除或不可用")
    );
    assert!(
        !std::fs::read_to_string(export.join("captures.md"))
            .unwrap()
            .contains(&c.text)
    );
}
