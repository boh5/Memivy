use memivy_core::memory::*;
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn capture(store: &MemoryStore, text: &str) -> RawCapture {
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
fn key(raw: &RawCapture) -> RecordKey {
    RecordKey {
        kind: "capture".into(),
        id: raw.id.clone(),
    }
}
fn proposal(action: &str) -> OrganizationProposal {
    OrganizationProposal {
        action: action.into(),
        target: String::new(),
        title: if action == "new" {
            "测试记忆".into()
        } else {
            String::new()
        },
        addition: if action == "new" {
            "保留这段合成记录".into()
        } else {
            String::new()
        },
        changes: vec![],
        keywords: vec!["同义索引".into()],
        reason: "依据本次原话整理".into(),
    }
}
fn seeded(store: &MemoryStore, text: &str) -> Receipt {
    let raw = capture(store, text);
    store.capture_as_new(&id(), &raw.id).unwrap()
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
    assert_eq!(store.capture(&req).unwrap().id, raw.id);
    let task = store.claim_organization().unwrap().unwrap();
    assert!(store.claim_organization().unwrap().is_none());
    let reopened = MemoryStore::open(dir.path()).unwrap();
    reopened.recover_organization().unwrap();
    let resumed = reopened.claim_organization().unwrap().unwrap();
    assert_eq!(task.attempt_id, resumed.attempt_id);
    let r = reopened
        .apply_organization(&resumed, &proposal("new"))
        .unwrap();
    assert_eq!(
        reopened
            .apply_organization(&resumed, &proposal("new"))
            .unwrap(),
        r
    );
    assert_eq!(reopened.capture_by_id(&raw.id).unwrap().text, req.text);
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
    let mut p = proposal("append");
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
    assert_eq!(s.capture_by_id(&raw.id).unwrap().understanding, "pending");
    s.recover_organization().unwrap();
    assert!(s.claim_organization().unwrap().is_none());
    s.retry_organization(&raw.id).unwrap();
    assert_ne!(
        s.claim_organization().unwrap().unwrap().attempt_id,
        task.attempt_id
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
        let mut p = proposal("append");
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
                s.trash_capture(&raw.id).unwrap();
            }
            "manual" => {
                s.capture_as_new(&id(), &raw.id).unwrap();
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
    s.retry_organization(&raw.id).unwrap();
    let next = s.claim_organization().unwrap().unwrap();
    s.fail_organization(&next.attempt_id, "unavailable")
        .unwrap();
    assert_eq!(s.organization_jobs(&key(&raw)).unwrap()[0].status, "failed");
    assert!(s.claim_organization().unwrap().is_none());
    assert_eq!(s.capture_by_id(&raw.id).unwrap().text, raw.text);
    assert!(s.apply_organization(&task, &proposal("new")).is_err());
}
#[test]
fn keywords_rebuild_and_erasure_follow_source_availability() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let raw = capture(&s, "保存合成输入");
    let task = s.claim_organization().unwrap().unwrap();
    s.apply_organization(&task, &proposal("new")).unwrap();
    let query = LibraryQuery {
        query: "同义索引".into(),
        ..Default::default()
    };
    assert_eq!(s.library(&query).unwrap().items.len(), 1);
    s.rebuild_search_index().unwrap();
    assert_eq!(s.library(&query).unwrap().items.len(), 1);
    s.trash_capture(&raw.id).unwrap();
    assert!(s.library(&query).unwrap().items.is_empty());
    s.purge_capture(&raw.id).unwrap();
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
    let r = s.capture_as_new(&request, &raw.id).unwrap();
    assert_eq!(s.capture_as_new(&request, &raw.id).unwrap(), r);
    assert!(s.capture_as_new(&id(), &raw.id).is_err());
    assert!(s.apply_organization(&task, &proposal("new")).is_err());
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
        assert!(s.organization_jobs(&key(&raw)).unwrap().is_empty());
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
    assert_eq!(
        s.capture_by_id(r.capture_id.as_ref().unwrap())
            .unwrap()
            .text,
        "确认原话"
    );
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
    let r = s.save_reviewed_conclusion(&request, Some(merged)).unwrap();
    let capture = r.capture_id.clone().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    assert_eq!(
        s.save_reviewed_conclusion(&request, Some(merged)).unwrap(),
        r
    );
    let key = RecordKey {
        kind: "capture".into(),
        id: capture.clone(),
    };
    let detail = s.library_detail(&key).unwrap();
    let reviewed = detail.reviewed_conclusion.unwrap();
    assert_eq!(reviewed.body, merged);
    assert_eq!(reviewed.title, request.title);
    assert_eq!(detail.body, request.text);
    assert!(
        s.library(&LibraryQuery {
            query: "完整审核稿".into(),
            ..Default::default()
        })
        .unwrap()
        .items
        .is_empty()
    );
    let saved = s
        .save_library_edit(&WorkspaceDraft {
            key: format!("capture:{capture}"),
            request_id: id(),
            title: reviewed.title,
            body: reviewed.body,
            expected_version: None,
            origin: None,
            context: vec![],
        })
        .unwrap();
    assert_eq!(
        s.memory(saved.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        merged
    );
    assert_eq!(s.capture_by_id(&capture).unwrap().text, request.text);
    s.undo(&id(), &saved.request_id).unwrap();
    s.trash_capture(&capture).unwrap();
    assert!(
        s.library_detail(&key)
            .unwrap()
            .reviewed_conclusion
            .is_none()
    );
    s.purge_capture(&capture).unwrap();
    let db = rusqlite::Connection::open(s.database_path()).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM conclusion_intents WHERE capture_id=?",
            [&capture],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
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
    let memory_a = s.capture_as_new(&id(), &a.id).unwrap();
    let b = scoped("项目 B", "SQLite 连接池默认 6 个连接");
    let memory_b = s.capture_as_new(&id(), &b.id).unwrap();
    let raw = scoped("项目 B", "SQLite 连接池改为 8 个连接");
    let mut task = s.claim_organization().unwrap().unwrap();
    assert_eq!(task.capture.id, raw.id);
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
    let mut p = proposal("append");
    p.target = "M1".into();
    p.addition = "连接池改为 8 个连接".into();
    assert_eq!(s.apply_organization(&task, &p), Err(DataError::Conflict));
    assert_eq!(
        s.memory(memory_a.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        a.text
    );
}
