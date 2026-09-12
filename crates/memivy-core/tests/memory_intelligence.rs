use memivy_core::memory::*;
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}

#[test]
fn system_reasons_are_codes_and_generated_or_historical_reasons_stay_literal() {
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
    let mut generated = proposal("keep");
    generated.reason = "An explanation generated in the source language".into();
    store.apply_organization(&task, &generated).unwrap();
    let job = &store.organization_jobs(&key(&raw)).unwrap()[0];
    assert_eq!(job.reason, generated.reason);
    assert_eq!(job.reason_code, None);

    // Exercise the ordinary schema upgrade with an old literal explanation.
    let db = rusqlite::Connection::open(store.database_path()).unwrap();
    db.execute(
        "UPDATE organization_jobs SET reason='历史系统提示，不应反推代码'",
        [],
    )
    .unwrap();
    db.execute_batch(
        "ALTER TABLE organization_jobs DROP COLUMN reason_code; PRAGMA user_version=14;",
    )
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
fn proposal(action: &str) -> OrganizationProposal {
    OrganizationProposal {
        action: action.into(),
        target: String::new(),
        title: if action == "keep" {
            "测试记忆".into()
        } else {
            String::new()
        },
        addition: if action == "keep" {
            "保留这段合成记录".into()
        } else {
            String::new()
        },
        changes: vec![],
        keywords: vec!["同义索引".into()],
        reason: "依据本次原话整理".into(),
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
        .apply_organization(&resumed, &proposal("keep"))
        .unwrap();
    assert_eq!(
        reopened
            .apply_organization(&resumed, &proposal("keep"))
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
fn patches_preserve_unaffected_text_and_undo_does_not_restart_ai() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let original =
        "首次体验要快。\n\n默认先注册。\n\n保留这些手工细节：不收集网页地址，不弹出额外窗口。";
    let old = seeded(&s, original);
    let raw = capture(&s, "首次体验改成先试用，保留其他原则");
    let mut task = s.claim_organization().unwrap().unwrap();
    s.prepare_organization(&mut task).unwrap();
    let index = task
        .candidates
        .iter()
        .position(|v| Some(&v.memory_id) == old.memory_id.as_ref())
        .unwrap();
    let mut p = proposal("merge");
    p.target = format!("M{}", index + 1);
    p.changes = vec![LocalChange {
        before: "默认先注册。".into(),
        after: "现在决定先试用。".into(),
    }];
    p.addition = "2026年9月确认本次变化。".into();
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
            p.addition
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
        "首次体验改成先试用，保留其他原则"
    );
    s.recover_organization().unwrap();
    assert!(s.claim_organization().unwrap().is_none());
    assert_eq!(
        s.retry_organization(&raw.memory_id).unwrap_err(),
        DataError::Conflict
    );
}
#[test]
fn stale_unknown_and_destructive_proposals_never_change_memory() {
    for mode in [
        "unknown",
        "whole",
        "ambiguous",
        "empty",
        "overlap",
        "stale",
        "deleted",
        "manual",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let s = MemoryStore::open(dir.path()).unwrap();
        let original = "原则甲，原则乙。重复。重复。其余内容保持原样，不能被自动压缩。";
        let old = seeded(&s, original);
        let raw = capture(&s, "原则甲需要补充");
        let mut task = s.claim_organization().unwrap().unwrap();
        s.prepare_organization(&mut task).unwrap();
        let mut p = proposal("merge");
        p.target = "M1".into();
        p.addition = "补充记录".into();
        match mode {
            "unknown" => p.target = "M99".into(),
            "whole" => {
                p.changes = vec![LocalChange {
                    before: original.into(),
                    after: "缩略总结".into(),
                }]
            }
            "ambiguous" => {
                p.changes = vec![LocalChange {
                    before: "重复。".into(),
                    after: "改了".into(),
                }]
            }
            "empty" => {
                p.changes = vec![LocalChange {
                    before: "原则甲".into(),
                    after: String::new(),
                }]
            }
            "overlap" => {
                p.changes = vec![
                    LocalChange {
                        before: "原则甲".into(),
                        after: "更新甲".into(),
                    },
                    LocalChange {
                        before: "原则甲，".into(),
                        after: "新甲".into(),
                    },
                ]
            }
            "stale" => {
                s.edit_memory(&EditRequest {
                    request_id: id(),
                    memory_id: old.memory_id.clone().unwrap(),
                    expected_version: old.after_version.clone().unwrap(),
                    title: "更新".into(),
                    body: "用户已编辑".into(),
                })
                .unwrap();
            }
            "deleted" => {
                s.trash_memory(
                    &raw.memory_id,
                    &s.memory(&raw.memory_id).unwrap().current.id,
                )
                .unwrap();
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
fn deferral_failure_and_retry_keep_raw_without_automatic_loops() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let raw = capture(&s, "那个方案再说吧");
    let task = s.claim_organization().unwrap().unwrap();
    let r = s.apply_organization(&task, &proposal("defer")).unwrap();
    assert_eq!(r.action, "defer");
    assert!(s.claim_organization().unwrap().is_none());
    s.retry_organization(&raw.memory_id).unwrap();
    let next = s.claim_organization().unwrap().unwrap();
    s.fail_organization(&next.attempt_id, "unavailable")
        .unwrap();
    assert_eq!(s.organization_jobs(&key(&raw)).unwrap()[0].status, "failed");
    assert!(s.claim_organization().unwrap().is_none());
    assert_eq!(
        s.capture_by_id(&raw.capture_id).unwrap().text,
        "那个方案再说吧"
    );
    assert!(s.apply_organization(&task, &proposal("keep")).is_err());
}
#[test]
fn keywords_rebuild_and_erasure_follow_source_availability() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let raw = capture(&s, "保存合成输入");
    let task = s.claim_organization().unwrap().unwrap();
    s.apply_organization(&task, &proposal("keep")).unwrap();
    let query = LibraryQuery {
        query: "同义索引".into(),
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
    assert!(s.apply_organization(&task, &proposal("keep")).is_err());
}
fn completed(s: &MemoryStore, source: &Receipt) -> Turn {
    let topic = id();
    s.create_conversation(&topic, "合成讨论").unwrap();
    let t = s
        .start_turn(
            &id(),
            &topic,
            "继续讨论",
            &[SourceRef::Version(source.after_version.clone().unwrap())],
        )
        .unwrap();
    s.finish_turn(
        &t.id,
        "尚未保存的建议",
        &[SourceRef::Version(source.after_version.clone().unwrap())],
    )
    .unwrap();
    s.turn(&t.id).unwrap()
}
#[test]
fn conclusions_append_or_save_exact_reviewed_merge_without_auto_organization() {
    for merged in [None, Some("审核过的融合正文，保留用户特别说明。")] {
        let dir = tempfile::tempdir().unwrap();
        let s = MemoryStore::open(dir.path()).unwrap();
        let old = seeded(&s, "旧正文绝不能被一句结论替换。");
        let turn = completed(&s, &old);
        let request = ConclusionRequest {
            request_id: id(),
            message_id: turn.assistant.id,
            destination: Destination::Existing {
                memory_id: old.memory_id.clone().unwrap(),
                expected_version: old.after_version.clone().unwrap(),
            },
            title: "确认标题".into(),
            text: "  经用户修改确认的结论\n  ".into(),
        };
        let r = s.save_reviewed_conclusion(&request, merged).unwrap();
        assert_eq!(s.save_reviewed_conclusion(&request, merged).unwrap(), r);
        assert_eq!(
            s.memory(r.memory_id.as_ref().unwrap())
                .unwrap()
                .current
                .body,
            merged
                .map(str::to_owned)
                .unwrap_or_else(|| format!("旧正文绝不能被一句结论替换。\n\n{}", request.text))
        );
        let raw = s.capture_by_id(r.capture_id.as_ref().unwrap()).unwrap();
        assert_eq!(raw.text, request.text);
        assert!(
            s.organization_jobs(&RecordKey {
                kind: "capture".into(),
                id: raw.id.clone()
            })
            .unwrap()
            .is_empty()
        );
        assert!(
            s.save_reviewed_conclusion(&request, Some("迟到的其他融合文本"))
                .is_err()
        );
        s.undo(&id(), &r.request_id).unwrap();
        assert_eq!(
            s.memory(r.memory_id.as_ref().unwrap())
                .unwrap()
                .current
                .body,
            "旧正文绝不能被一句结论替换。"
        );
        assert_eq!(s.capture_by_id(&raw.id).unwrap().text, request.text);
        assert!(s.claim_organization().unwrap().is_none());
    }
}
#[test]
fn stale_merge_keeps_confirmation_without_overwriting_and_can_be_corrected() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let old = seeded(&s, "旧正文");
    let t = completed(&s, &old);
    s.edit_memory(&EditRequest {
        request_id: id(),
        memory_id: old.memory_id.clone().unwrap(),
        expected_version: old.after_version.clone().unwrap(),
        title: "新标题".into(),
        body: "别处刚刚编辑的正文".into(),
    })
    .unwrap();
    let r = s
        .save_reviewed_conclusion(
            &ConclusionRequest {
                request_id: id(),
                message_id: t.assistant.id,
                destination: Destination::Existing {
                    memory_id: old.memory_id.clone().unwrap(),
                    expected_version: old.after_version.unwrap(),
                },
                title: "审核标题".into(),
                text: "确认原话".into(),
            },
            Some("过时融合结果"),
        )
        .unwrap();
    assert_eq!(r.status, "needs_review");
    assert_eq!(
        s.memory(old.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        "别处刚刚编辑的正文"
    );
    assert!(r.capture_id.is_none());
    assert!(s.claim_organization().unwrap().is_none());
}

#[test]
fn conflict_retains_complete_review_after_restart_until_explicit_save_or_erasure() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let old = seeded(&s, "原来正文");
    let t = completed(&s, &old);
    s.edit_memory(&EditRequest {
        request_id: id(),
        memory_id: old.memory_id.clone().unwrap(),
        expected_version: old.after_version.clone().unwrap(),
        title: "最新标题".into(),
        body: "并发修改".into(),
    })
    .unwrap();
    let request = ConclusionRequest {
        request_id: id(),
        message_id: t.assistant.id,
        destination: Destination::Existing {
            memory_id: old.memory_id.clone().unwrap(),
            expected_version: old.after_version.unwrap(),
        },
        title: "审核标题".into(),
        text: "短结论".into(),
    };
    let merged = "  完整审核稿，包含另外手工修改的段落。\n ";
    let draft = WorkspaceDraft {
        conclusion: Some(ConclusionDraft {
            destination: request.destination.clone(),
            merged_body: Some(merged.into()),
        }),
        key: format!("conclusion:{}", request.message_id),
        request_id: request.request_id.clone(),
        title: request.title.clone(),
        body: request.text.clone(),
        expected_version: None,
        origin: None,
        context: vec![],
    };
    s.save_workspace_draft(&draft).unwrap();
    let r = s.save_reviewed_conclusion(&request, Some(merged)).unwrap();
    assert_eq!(r.status, "needs_review");
    assert!(r.capture_id.is_none());
    let s = MemoryStore::open(dir.path()).unwrap();
    let restored = s.workspace_draft(&draft.key).unwrap().unwrap();
    assert_eq!(restored.body, request.text);
    assert_eq!(
        restored.conclusion.unwrap().merged_body.as_deref(),
        Some(merged)
    );
    assert!(
        s.search(&SearchRequest::text("完整审核稿", 8))
            .unwrap()
            .items
            .is_empty()
    );
    // Explicitly rechecking the target allows the exact reviewed text to save.
    let current = s.memory(old.memory_id.as_ref().unwrap()).unwrap().current;
    let confirmed = ConclusionRequest {
        request_id: id(),
        destination: Destination::Existing {
            memory_id: current.memory_id,
            expected_version: current.id,
        },
        ..request
    };
    let saved = s
        .save_reviewed_conclusion(&confirmed, Some(merged))
        .unwrap();
    assert_eq!(
        s.memory(saved.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        merged
    );
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
    let i = task
        .candidates
        .iter()
        .position(|v| Some(&v.memory_id) == memory_b.memory_id.as_ref())
        .unwrap();
    assert_eq!(
        task.candidate_projects[&task.candidates[i].memory_id],
        vec!["项目 B"]
    );
    // Even a forged/stale task cannot bypass the transactional project check.
    task.candidates = vec![
        s.memory(memory_a.memory_id.as_ref().unwrap())
            .unwrap()
            .current,
    ];
    let mut p = proposal("merge");
    p.target = "M1".into();
    p.addition = "连接池改为 8 个连接".into();
    assert_eq!(s.apply_organization(&task, &p), Err(DataError::Conflict));
    assert_eq!(
        s.memory(memory_a.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        "SQLite 连接池默认 4 个连接"
    );
}
