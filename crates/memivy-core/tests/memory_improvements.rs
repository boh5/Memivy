use memivy_core::memory::*;
use std::{fs, time::Instant};
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn capture(s: &MemoryStore, text: &str) -> CaptureResult {
    s.capture(&CaptureRequest {
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
fn memory(s: &MemoryStore, title: &str, body: &str) -> Receipt {
    let raw = capture(s, body);
    s.edit_memory(&EditRequest {
        request_id: id(),
        memory_id: raw.memory_id,
        expected_version: raw.version_id,
        title: title.into(),
        body: body.into(),
    })
    .unwrap()
}
#[test]
fn retrieves_only_current_facts_while_historical_citations_remain_readable() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let r = memory(&s, "同步计划", "一月同步计划：成本太高，暂缓。");
    let mid = r.memory_id.unwrap();
    let oldest = r.after_version.unwrap();
    let mut version = oldest.clone();
    for body in [
        "二月进展：梳理输入界面。",
        "三月进展：完善本地备份。",
        "四月进展：完善阅读体验。",
        "七月同步计划：决定开始实施。",
    ] {
        version = s
            .edit_memory(&EditRequest {
                request_id: id(),
                memory_id: mid.clone(),
                expected_version: version,
                title: "项目进展".into(),
                body: body.into(),
            })
            .unwrap()
            .after_version
            .unwrap();
    }
    let found = s.discussion_sources(&["同步计划".into()], &[]).unwrap();
    let texts: Vec<_> = found
        .iter()
        .map(|r| s.resolve_source(r, 1800).unwrap())
        .collect();
    assert!(texts.iter().all(|e| !e.text.contains("一月") && e.current));
    assert!(texts.iter().any(|e| e.text.contains("七月")));
    assert!(texts.iter().all(|e| e.recorded_at > 0));
    assert!(!found.contains(&SourceRef::Version(oldest.clone())));
    assert!(
        s.resolve_source(&SourceRef::Version(oldest), 1800)
            .unwrap()
            .text
            .contains("一月")
    );
    s.trash_memory(&mid, &version).unwrap();
    assert!(
        s.discussion_sources(&["同步计划".into()], &[])
            .unwrap()
            .is_empty()
    );
}
#[test]
fn related_excludes_self_trash_and_old_versions_and_checks_the_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let origin = memory(&s, "蓝鲸旅行", "蓝鲸旅行准备路线和住宿。");
    for i in 0..5 {
        memory(&s, &format!("蓝鲸旅行资料{i}"), "蓝鲸旅行的其他路线资料。");
    }
    let removed = memory(&s, "蓝鲸旅行丢弃", "蓝鲸旅行的无用资料。");
    s.trash_memory(
        removed.memory_id.as_ref().unwrap(),
        removed.after_version.as_ref().unwrap(),
    )
    .unwrap();
    let stale = memory(&s, "蓝鲸旅行旧标题", "蓝鲸旅行旧内容");
    // Original capture remains related and visible, so this current memory may legitimately appear.
    let results = s
        .related_memories(
            origin.memory_id.as_ref().unwrap(),
            origin.after_version.as_ref().unwrap(),
        )
        .unwrap();
    assert_eq!(results.len(), 3);
    assert!(
        results
            .iter()
            .all(|r| Some(&r.memory_id) != origin.memory_id.as_ref()
                && Some(&r.memory_id) != removed.memory_id.as_ref())
    );
    assert_eq!(
        results
            .iter()
            .map(|r| &r.memory_id)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        results.len()
    );
    let _ = stale;
    assert!(matches!(
        s.related_memories(origin.memory_id.as_ref().unwrap(), &id()),
        Err(DataError::Conflict)
    ));
}
#[test]
fn whole_restore_preserves_latest_library_and_private_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    memory(&s, "备份时的记忆", "旧内容，备份时存在。");
    let backup = dir.path().join("saved.db");
    s.backup(&backup).unwrap();
    let prepared = s.prepare_restore(&backup).unwrap();
    assert_eq!(prepared.memories, 1);
    memory(&s, "后来新建的记忆", "暂存校验之后又写入的内容。");
    fs::write(s.model_config_path(), "synthetic-secret-preserve").unwrap();
    s.set_mcp_enabled(true).unwrap();
    s.arm_restore(&prepared.id).unwrap();
    assert!(
        matches!(MemoryStore::open(dir.path()), Err(DataError::Busy)),
        "MCP startup must not bypass pending restore"
    );
    assert!(matches!(
        s.mcp_capture(&CaptureRequest {
            request_id: id(),
            text: "must not write".into(),
            origin: Origin::Agent {
                app: "QA".into(),
                project: None,
                uri: None
            }
        }),
        Err(DataError::Busy)
    ));
    let restored = MemoryStore::open_application(dir.path()).unwrap();
    assert_eq!(
        restored
            .library(&LibraryQuery::default())
            .unwrap()
            .items
            .len(),
        1
    );
    assert!(restored.mcp_enabled());
    assert_eq!(
        fs::read_to_string(restored.model_config_path()).unwrap(),
        "synthetic-secret-preserve"
    );
    let outcome = restored.last_restore_result().unwrap().unwrap();
    assert!(outcome.restored);
    assert_eq!(outcome.message_code.as_deref(), Some("restore_completed"));
    assert!(outcome.error_code.is_none());
    let previous = outcome.previous_backup.unwrap();
    assert!(
        !fs::read(&previous)
            .unwrap()
            .windows(b"synthetic-secret-preserve".len())
            .any(|s| s == b"synthetic-secret-preserve")
    );
    let olddir = tempfile::tempdir().unwrap();
    let previous = MemoryStore::restore_backup(&previous, olddir.path()).unwrap();
    assert_eq!(
        previous
            .library(&LibraryQuery::default())
            .unwrap()
            .items
            .len(),
        2
    );
    restored.check_integrity().unwrap();
    previous.check_integrity().unwrap();
}
#[test]
fn cancelling_or_rejecting_backup_never_arms_a_restore() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    memory(&s, "仍然保留", "原记忆");
    let backup = dir.path().join("saved.db");
    s.backup(&backup).unwrap();
    let prepared = s.prepare_restore(&backup).unwrap();
    s.discard_prepared_restore(&prepared.id).unwrap();
    assert!(s.arm_restore(&prepared.id).is_err());
    let corrupt = dir.path().join("broken.db");
    fs::write(&corrupt, b"not sqlite").unwrap();
    assert!(s.prepare_restore(&corrupt).is_err());
    let opened = MemoryStore::open_application(dir.path()).unwrap();
    assert_eq!(
        opened
            .library(&LibraryQuery::default())
            .unwrap()
            .items
            .len(),
        1
    );
    assert!(opened.last_restore_result().unwrap().is_none());
}

#[test]
#[ignore = "explicit large-library performance measurement"]
fn retrieval_performance_10000_memories() {
    use rusqlite::{Connection, params};
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let mut db = Connection::open(s.database_path()).unwrap();
    let tx = db.transaction().unwrap();
    let mut last = (String::new(), String::new());
    for i in 0..10_000 {
        let m = id();
        let v = id();
        let c = id();
        let title = if i % 100 == 0 {
            "蓝鲸旅行住宿"
        } else {
            "项目开发记录"
        };
        let body = format!(
            "{title} 第{i}条。{}",
            "记下产品开发过程中的问题和解决办法。".repeat(12)
        );
        tx.execute(
            "INSERT INTO captures VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                c,
                id(),
                vec![0u8; 32],
                body,
                r#"{"kind":"user","app":"QA"}"#,
                i
            ],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO capture_state(capture_id,understanding) VALUES(?,'attached')",
            [&c],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO memories(id,state,created_at,updated_at) VALUES(?,'active',0,0)",
            [&m],
        )
        .unwrap();
        tx.execute("INSERT INTO memory_versions(id,memory_id,title,body,actor,reason,created_at) VALUES(?1,?2,?3,?4,'user','create',?5)",params![v,m,title,body,i]).unwrap();
        tx.execute(
            "UPDATE memories SET current_version_id=?1 WHERE id=?2",
            params![v, m],
        )
        .unwrap();
        tx.execute("INSERT INTO version_captures VALUES(?1,?2)", params![v, c])
            .unwrap();
        let request = id();
        tx.execute("INSERT INTO receipts(request_id,fingerprint,action,capture_id,memory_id,after_version,status,created_at) VALUES(?1,?2,'new',?3,?4,?5,'applied',?6)",params![request,vec![0u8;32],c,m,v,i]).unwrap();
        tx.execute("INSERT INTO receipt_changes(request_id,memory_id,after_version,before_state,after_state) VALUES(?1,?2,?3,'undone','active')",params![request,m,v]).unwrap();
        if i == 9900 {
            last = (m, v);
        }
    }
    tx.commit().unwrap();
    drop(db);
    for (name, queries) in [
        ("FTS", vec!["蓝鲸旅行".into()]),
        ("short", vec!["蓝鲸".into(), "住宿".into()]),
    ] {
        let mut times = Vec::new();
        for _ in 0..20 {
            let start = Instant::now();
            let found = s.discussion_sources(&queries, &[]).unwrap();
            assert!(!found.is_empty());
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        println!(
            "{name} 10000 memories p50={:.1}ms p95={:.1}ms",
            times[10], times[18]
        );
    }
    let mut times = Vec::new();
    for _ in 0..20 {
        let start = Instant::now();
        assert!(!s.related_memories(&last.0, &last.1).unwrap().is_empty());
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "related 10000 memories p50={:.1}ms p95={:.1}ms",
        times[10], times[18]
    );
}

#[test]
fn undone_ai_versions_do_not_reappear_as_automatic_rag_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    let r = memory(&s, "原计划", "原话没有新增承诺。");
    let edited = s
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: r.memory_id.clone().unwrap(),
            expected_version: r.after_version.unwrap(),
            title: "被撤销的内容".into(),
            body: "错误新增的星际旅行承诺".into(),
        })
        .unwrap();
    s.undo(&id(), &edited.request_id).unwrap();
    assert!(
        s.discussion_sources(&["星际旅行".into()], &[])
            .unwrap()
            .is_empty()
    );
}
#[test]
fn corrupted_staging_or_newer_backup_leaves_the_current_library_intact() {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    memory(&s, "保留", "保留当前内容。");
    let saved = dir.path().join("saved.db");
    s.backup(&saved).unwrap();
    let prepared = s.prepare_restore(&saved).unwrap();
    s.arm_restore(&prepared.id).unwrap();
    fs::write(
        dir.path()
            .join(format!("restore-staged-{}.db", prepared.id)),
        b"broken",
    )
    .unwrap();
    let reopened = MemoryStore::open_application(dir.path()).unwrap();
    assert_eq!(
        reopened
            .library(&LibraryQuery::default())
            .unwrap()
            .items
            .len(),
        1
    );
    assert!(!reopened.last_restore_result().unwrap().unwrap().restored);
    let db = rusqlite::Connection::open(&saved).unwrap();
    db.pragma_update(None, "user_version", 999).unwrap();
    drop(db);
    assert!(matches!(
        reopened.prepare_restore(&saved),
        Err(DataError::Schema)
    ));
    reopened.check_integrity().unwrap();
}
