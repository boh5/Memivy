use memivy_core::memory::*;
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}

#[test]
fn system_reasons_are_codes_and_historical_explanations_stay_literal() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let raw = capture(&store, "Original English capture 原始记录");
    let task = store.claim_organization().unwrap().unwrap();
    store
        .fail_organization(&task.attempt_id, "invalid")
        .unwrap();
    let failed = &store.organization_jobs(&key(&raw)).unwrap()[0];
    assert_eq!(failed.reason_code.as_deref(), Some("organization_invalid"));
    assert_eq!(failed.reason, "");
    store.retry_organization(&raw.memory_id).unwrap();
    let task = store.claim_organization().unwrap().unwrap();
    let generated = proposal(&task);
    store.apply_organization(&task, &generated).unwrap();
    let job = &store.organization_jobs(&key(&raw)).unwrap()[0];
    assert_eq!(job.reason, "");
    assert_eq!(job.reason_code, None);

    // Exercise this migration's historical literal preservation independently
    // of unrelated later schema additions.
    let db = rusqlite::Connection::open(store.database_path()).unwrap();
    let current_schema: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    db.execute(
        "UPDATE organization_jobs SET reason='历史系统提示，不应反推代码'",
        [],
    )
    .unwrap();
    db.execute_batch("ALTER TABLE organization_jobs DROP COLUMN reason_code;")
        .unwrap();
    db.execute_batch(include_str!(
        "../../../migrations/memory/015_organization_reason.sql"
    ))
    .unwrap();
    db.pragma_update(None, "user_version", current_schema)
        .unwrap();
    drop(db);
    drop(store);
    let reopened = MemoryStore::open(dir.path()).unwrap();
    let history = &reopened.organization_jobs(&key(&raw)).unwrap()[0];
    assert_eq!(history.reason, "历史系统提示，不应反推代码");
    assert_eq!(history.reason_code, None);
    assert_eq!(
        reopened.capture_by_id(&raw.capture_id).unwrap().text,
        "Original English capture 原始记录"
    );
}
fn capture(store: &MemoryStore, text: &str) -> CaptureResult {
    store
        .capture(&CaptureRequest {
            request_id: id(),
            text: text.into(),
            origin: Origin::User {
                app: "Synthetic QA".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap()
}
fn key(raw: &CaptureResult) -> RecordKey {
    RecordKey {
        kind: "memory".into(),
        id: raw.memory_id.clone(),
    }
}
fn proposal(task: &OrganizationTask) -> MemoryWriteArgs {
    MemoryWriteArgs {
        destination: Destination::New,
        title: "测试记忆".into(),
        parts: vec![MemoryWritePart {
            text: "保留这段合成记录".into(),
            sources: vec![MemorySourceQuote {
                source_id: task.capture_id.clone(),
                quote: task.memory.body.trim().chars().take(512).collect(),
            }],
        }],
    }
}
fn keep(s: &MemoryStore, request: &str, saved: &CaptureResult) -> Result<Receipt> {
    s.edit_memory(&EditRequest {
        request_id: request.into(),
        memory_id: saved.memory_id.clone(),
        expected_version: saved.version_id.clone(),
        title: "测试记忆".into(),
        body: s.capture_by_id(&saved.capture_id)?.text,
    })
}
fn seeded(store: &MemoryStore, text: &str) -> Receipt {
    let raw = capture(store, text);
    keep(store, &id(), &raw).unwrap()
}
#[test]
fn capture_transaction_enqueues_once_and_restart_application_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let req = CaptureRequest {
        request_id: id(),
        text: "  原话\n逐字保留  ".into(),
        origin: Origin::User {
            app: "QA".into(),
            project: None,
            uri: None,
        },
    };
    let raw = store.capture(&req).unwrap();
    assert_eq!(store.capture(&req).unwrap().memory_id, raw.memory_id);
    let task = store.claim_organization().unwrap().unwrap();
    assert!(store.claim_organization().unwrap().is_none());
    let reopened = MemoryStore::open(dir.path()).unwrap();
    reopened.recover_organization().unwrap();
    let resumed = reopened.claim_organization().unwrap().unwrap();
    assert_eq!(task.attempt_id, resumed.attempt_id);
    let r = reopened
        .apply_organization(&resumed, &proposal(&resumed))
        .unwrap();
    assert_eq!(
        reopened
            .apply_organization(&resumed, &proposal(&resumed))
            .unwrap(),
        r
    );
    assert_eq!(
        reopened.capture_by_id(&raw.capture_id).unwrap().text,
        req.text
    );
    reopened.recover_organization().unwrap();
    assert!(reopened.claim_organization().unwrap().is_none());
    assert_eq!(
        reopened.organization_jobs(&key(&raw)).unwrap()[0].status,
        "done"
    );
}
#[test]
fn organization_preserves_complete_target_and_undo_does_not_restart_ai() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let original =
        "首次体验要快。\n\n默认先注册。\n\n保留这些手工细节：不收集网页地址，不弹出额外窗口。";
    let old = seeded(&s, original);
    let raw = capture(&s, "2026年9月确认：首次体验改成先试用，保留其他原则");
    let mut task = s.claim_organization().unwrap().unwrap();
    s.prepare_organization(&mut task).unwrap();
    let target = s.memory(old.memory_id.as_ref().unwrap()).unwrap().current;
    let mut p = proposal(&task);
    p.destination = Destination::Existing {
        memory_id: target.memory_id,
        expected_version: target.id,
    };
    p.title = target.title;
    p.parts = vec![
        MemoryWritePart {
            text: "首次体验要快。\n\n".into(),
            sources: vec![],
        },
        MemoryWritePart {
            text: "现在决定先试用。".into(),
            sources: vec![MemorySourceQuote {
                source_id: task.capture_id.clone(),
                quote: "首次体验改成先试用".into(),
            }],
        },
        MemoryWritePart {
            text: "\n\n保留这些手工细节：不收集网页地址，不弹出额外窗口。".into(),
            sources: vec![],
        },
        MemoryWritePart {
            text: "\n\n2026年9月确认本次变化。".into(),
            sources: vec![MemorySourceQuote {
                source_id: task.capture_id.clone(),
                quote: "2026年9月确认".into(),
            }],
        },
    ];
    let r = s.apply_organization(&task, &p).unwrap();
    let body = s
        .memory(r.memory_id.as_ref().unwrap())
        .unwrap()
        .current
        .body;
    assert_eq!(
        body,
        format!(
            "{}\n\n{}",
            original.replace("默认先注册。", "现在决定先试用。"),
            "2026年9月确认本次变化。"
        )
    );
    s.undo(&id(), &r.request_id).unwrap();
    assert_eq!(
        s.memory(r.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        original
    );
    assert_eq!(
        s.memory(&raw.memory_id).unwrap().current.body,
        "2026年9月确认：首次体验改成先试用，保留其他原则"
    );
    s.recover_organization().unwrap();
    assert!(s.claim_organization().unwrap().is_none());
    assert_eq!(
        s.retry_organization(&raw.memory_id).unwrap_err(),
        DataError::Conflict
    );
}
#[test]
fn invalid_sources_stale_targets_and_manual_edits_never_change_memory() {
    for mode in ["unknown", "empty", "source", "stale", "deleted", "manual"] {
        let dir = tempfile::tempdir().unwrap();
        let s = MemoryStore::open(dir.path()).unwrap();
        let original = "原则甲，原则乙。其余内容保持原样。";
        let old = seeded(&s, original);
        let raw = capture(&s, "原则甲需要补充");
        let task = s.claim_organization().unwrap().unwrap();
        let target = s.memory(old.memory_id.as_ref().unwrap()).unwrap().current;
        let mut p = proposal(&task);
        p.destination = Destination::Existing {
            memory_id: target.memory_id.clone(),
            expected_version: target.id.clone(),
        };
        p.title = target.title;
        p.parts = vec![
            MemoryWritePart {
                text: format!("{original}\n\n"),
                sources: vec![],
            },
            MemoryWritePart {
                text: "补充记录".into(),
                sources: vec![MemorySourceQuote {
                    source_id: task.capture_id.clone(),
                    quote: "原则甲需要补充".into(),
                }],
            },
        ];
        match mode {
            "unknown" => {
                p.destination = Destination::Existing {
                    memory_id: id(),
                    expected_version: id(),
                }
            }
            "empty" => p.parts.clear(),
            "source" => p.parts[1].sources[0].source_id = id(),
            "stale" => {
                s.edit_memory(&EditRequest {
                    request_id: id(),
                    memory_id: target.memory_id,
                    expected_version: target.id,
                    title: "更新".into(),
                    body: "用户已编辑".into(),
                })
                .unwrap();
            }
            "deleted" => {
                s.trash_memory(&raw.memory_id, &raw.version_id).unwrap();
            }
            "manual" => {
                keep(&s, &id(), &raw).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(s.apply_organization(&task, &p).is_err(), "{mode}");
        assert_eq!(
            s.memory(old.memory_id.as_ref().unwrap())
                .unwrap()
                .current
                .body,
            if mode == "stale" {
                "用户已编辑"
            } else {
                original
            }
        );
        s.check_integrity().unwrap();
    }
}
#[test]
fn unchanged_completion_and_failed_retry_preserve_raw_without_background_loops() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let raw = capture(&s, "那个方案再说吧");
    let task = s.claim_organization().unwrap().unwrap();
    s.complete_organization_unchanged(&task).unwrap();
    assert!(s.claim_organization().unwrap().is_none());
    assert_eq!(s.organization_jobs(&key(&raw)).unwrap()[0].status, "done");
    assert!(!s.organization_jobs(&key(&raw)).unwrap()[0].can_retry);
    assert_eq!(
        s.capture_by_id(&raw.capture_id).unwrap().text,
        "那个方案再说吧"
    );
    let next_raw = capture(&s, "第二段原话");
    let first = s.claim_organization().unwrap().unwrap();
    s.fail_organization(&first.attempt_id, "unavailable")
        .unwrap();
    assert!(s.claim_organization().unwrap().is_none());
    s.retry_organization(&next_raw.memory_id).unwrap();
    let retry = s.claim_organization().unwrap().unwrap();
    assert!(s.apply_organization(&first, &proposal(&first)).is_err());
    s.apply_organization(&retry, &proposal(&retry)).unwrap();
    assert_eq!(
        s.capture_by_id(&next_raw.capture_id).unwrap().text,
        "第二段原话"
    );
}
#[test]
fn organized_body_rebuild_and_erasure_follow_source_availability() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let raw = capture(&s, "保存合成输入");
    let task = s.claim_organization().unwrap().unwrap();
    s.apply_organization(&task, &proposal(&task)).unwrap();
    let query = LibraryQuery {
        query: "合成记录".into(),
        ..Default::default()
    };
    assert_eq!(s.library(&query).unwrap().items.len(), 1);
    s.rebuild_search_index().unwrap();
    assert_eq!(s.library(&query).unwrap().items.len(), 1);
    s.trash_memory(
        &raw.memory_id,
        &s.memory(&raw.memory_id).unwrap().current.id,
    )
    .unwrap();
    assert!(s.library(&query).unwrap().items.is_empty());
    s.purge_memory(&raw.memory_id).unwrap();
    s.rebuild_search_index().unwrap();
    assert!(s.library(&query).unwrap().items.is_empty());
    s.check_integrity().unwrap();
}
#[test]
fn manual_new_wins_against_pending_ai_and_retries_do_not_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let raw = capture(&s, "我选择另存原话");
    let task = s.claim_organization().unwrap().unwrap();
    let request = id();
    let r = keep(&s, &request, &raw).unwrap();
    assert_eq!(keep(&s, &request, &raw).unwrap(), r);
    assert!(keep(&s, &id(), &raw).is_err());
    assert!(s.apply_organization(&task, &proposal(&task)).is_err());
}
#[test]
fn explicit_selected_text_appends_exactly_and_never_schedules_organization() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let old = seeded(&store, "旧正文绝不能被一句结论替换。");
    let topic = id();
    store.create_conversation(&topic, "合成讨论").unwrap();
    let run = store
        .begin_agent_input(&id(), &id(), &topic, "继续讨论", &[], None)
        .unwrap();
    store
        .append_agent_text(&run.input_id, &run.attempt_id, "尚未保存的建议")
        .unwrap();
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, false, &[])
        .unwrap();
    let request = id();
    let destination = Destination::Existing {
        memory_id: old.memory_id.clone().unwrap(),
        expected_version: old.after_version.clone().unwrap(),
    };
    let text = "  用户修改后选择的文字\n  ";
    let saved = store
        .save_agent_text(&request, &run.input_id, text, "确认标题", &destination)
        .unwrap();
    assert_eq!(
        store
            .save_agent_text(&request, &run.input_id, text, "确认标题", &destination)
            .unwrap(),
        saved
    );
    assert_eq!(
        store
            .memory(saved.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        format!("旧正文绝不能被一句结论替换。\n\n{text}")
    );
    let raw = store
        .capture_by_id(saved.capture_id.as_ref().unwrap())
        .unwrap();
    assert_eq!(raw.text, text);
    assert!(
        store
            .organization_jobs(&RecordKey {
                kind: "capture".into(),
                id: raw.id.clone()
            })
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .save_agent_text(
                &request,
                &run.input_id,
                "迟到的另一段文字",
                "确认标题",
                &destination
            )
            .unwrap_err(),
        DataError::RequestConflict
    );
    store.undo_agent_input(&id(), &request).unwrap();
    assert_eq!(
        store
            .memory(saved.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        "旧正文绝不能被一句结论替换。"
    );
    assert_eq!(store.capture_by_id(&raw.id).unwrap().text, text);
    assert!(store.claim_organization().unwrap().is_none());
}

#[test]
fn organization_keeps_project_provenance_and_rejects_cross_project_targets() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let scoped = |project: &str, text: &str| {
        s.capture(&CaptureRequest {
            request_id: id(),
            text: text.into(),
            origin: Origin::User {
                app: "QA".into(),
                project: Some(project.into()),
                uri: None,
            },
        })
        .unwrap()
    };
    let a = scoped("项目 A", "SQLite 连接池默认 4 个连接");
    let memory_a = keep(&s, &id(), &a).unwrap();
    let b = scoped("项目 B", "SQLite 连接池默认 6 个连接");
    let memory_b = keep(&s, &id(), &b).unwrap();
    let raw = scoped("项目 B", "SQLite 连接池改为 8 个连接");
    let mut task = s.claim_organization().unwrap().unwrap();
    assert_eq!(task.capture_id, raw.capture_id);
    s.prepare_organization(&mut task).unwrap();
    assert!(
        task.candidates
            .iter()
            .all(|v| Some(&v.memory_id) != memory_a.memory_id.as_ref())
    );
    assert!(
        task.candidates
            .iter()
            .any(|hit| Some(&hit.memory_id) == memory_b.memory_id.as_ref())
    );
    let forbidden = s
        .memory(memory_a.memory_id.as_ref().unwrap())
        .unwrap()
        .current;
    let mut p = proposal(&task);
    p.destination = Destination::Existing {
        memory_id: forbidden.memory_id,
        expected_version: forbidden.id,
    };
    p.title = forbidden.title;
    p.parts[0].text = "SQLite 连接池改为 8 个连接".into();
    assert_eq!(s.apply_organization(&task, &p), Err(DataError::Conflict));
    assert_eq!(
        s.memory(memory_a.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        "SQLite 连接池默认 4 个连接"
    );
}
