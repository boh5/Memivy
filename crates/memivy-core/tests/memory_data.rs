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
            .search(&SearchRequest::text("应归到其他地方", 20))
            .unwrap()
            .items
            .iter()
            .any(|e| matches!(e.evidence.source, SourceRef::Version(_)))
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
            assert!(
                store
                    .library(&LibraryQuery::default())
                    .unwrap()
                    .items
                    .is_empty()
            );
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
fn discussions_and_drafts_are_not_memories_and_manual_save_retains_provenance() {
    let (_dir, store) = setup();
    let (conversation, turn) = finished_turn(&store, &[]);
    store
        .save_conversation_draft(&conversation, "尚未确定的草稿")
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
    store.delete_conversation(&conversation).unwrap();
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
    assert!(store.conversation(&conversation).is_err());
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
        1
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
    let restored = MemoryStore::restore_backup(&backup, &restore).unwrap();
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
    let (_, turn) = finished_turn(
        &store,
        &[SourceRef::Version(r.after_version.clone().unwrap())],
    );
    let edited = edit(&store, &r, "编辑的正文");
    let request = id();
    store
        .save_agent_text(
            &request,
            &turn.id,
            SAVED_TEXT,
            SAVED_TITLE,
            &Destination::New,
        )
        .unwrap();
    let sentinel = "SECRET_CONFIG_DO_NOT_EXPORT";
    std::fs::write(store.model_config_path(), sentinel).unwrap();
    let export = dir.path().join("export");
    store.export_markdown(&export).unwrap();
    let captures = std::fs::read_to_string(export.join("captures.md")).unwrap();
    assert!(captures.contains(&c.text));
    assert!(captures.contains("confirmed_by"));
    assert!(captures.contains(SAVED_TEXT));
    let memories = std::fs::read_to_string(export.join("memories.md")).unwrap();
    assert!(memories.contains(r.after_version.as_ref().unwrap()));
    assert!(memories.contains(edited.after_version.as_ref().unwrap()));
    let chats = std::fs::read_to_string(export.join("conversations.md")).unwrap();
    assert!(chats.contains("not durable memories"));
    assert!(chats.contains("Role: assistant"));
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
    store
        .trash_memory(
            r.memory_id.as_ref().unwrap(),
            edited.after_version.as_ref().unwrap(),
        )
        .unwrap();
    let export = dir.path().join("deleted-source-export");
    store.export_markdown(&export).unwrap();
    assert!(
        std::fs::read_to_string(export.join("conversations.md"))
            .unwrap()
            .contains("Source deleted or unavailable")
    );
    assert!(
        !std::fs::read_to_string(export.join("captures.md"))
            .unwrap()
            .contains(&c.text)
    );
}
