use memivy_core::memory::*;
use rusqlite::{Connection, params};
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn setup() -> (tempfile::TempDir, MemoryStore) {
    let d = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(d.path()).unwrap();
    (d, s)
}
fn capture(s: &MemoryStore, text: &str, kind: &str, project: Option<&str>) -> CaptureResult {
    let origin = if kind == "agent" {
        Origin::Agent {
            app: "Test Agent".into(),
            project: project.map(Into::into),
            uri: Some("https://example.com/中文路径?q=a_b%20".into()),
        }
    } else {
        Origin::User {
            app: "Memivy".into(),
            project: project.map(Into::into),
            uri: None,
        }
    };
    s.capture(&CaptureRequest {
        request_id: id(),
        text: text.into(),
        origin,
    })
    .unwrap()
}
fn organize(s: &MemoryStore, c: &CaptureResult, title: &str, body: &str) -> Receipt {
    s.edit_memory(&EditRequest {
        request_id: id(),
        memory_id: c.memory_id.clone(),
        expected_version: c.version_id.clone(),
        title: title.into(),
        body: body.into(),
    })
    .unwrap()
}
fn query(s: &MemoryStore, q: &str) -> LibraryPage {
    s.library(&LibraryQuery {
        query: q.into(),
        ..Default::default()
    })
    .unwrap()
}
fn key(r: &Receipt) -> RecordKey {
    RecordKey {
        kind: "memory".into(),
        id: r.memory_id.clone().unwrap(),
    }
}

#[test]
fn ranking_filters_and_pagination_apply_before_limit() {
    let (_d, s) = setup();
    let a = capture(&s, "最初原话 独有证据", "agent", Some("项目 A"));
    let r = organize(&s, &a, "中文检索决定", "当前摘要");
    for n in 0..55 {
        let saved = capture(
            &s,
            &format!("中文检索决定 正文记录 {n}"),
            "user",
            Some("项目 B"),
        );
        organize(
            &s,
            &saved,
            &format!("干扰项 {n}"),
            &format!("中文检索决定 正文记录 {n}"),
        );
    }
    let result = query(&s, "中文检索决定");
    assert_eq!(result.items[0].key.id, r.memory_id.unwrap());
    let filtered = s
        .library(&LibraryQuery {
            query: "当前摘要".into(),
            origin: Some("agent".into()),
            project: Some("项目 A".into()),
            limit: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(filtered.items.len(), 1);
    assert!(filtered.next_offset.is_none());
    let mut offset = 0;
    let mut all = std::collections::HashSet::new();
    loop {
        let page = s
            .library(&LibraryQuery {
                limit: 7,
                offset,
                ..Default::default()
            })
            .unwrap();
        for row in page.items {
            assert!(all.insert(row.key.id));
        }
        match page.next_offset {
            Some(n) => offset = n,
            None => break,
        }
    }
    assert_eq!(all.len(), 56);
    assert_eq!(s.library_projects().unwrap(), vec!["项目 A", "项目 B"]);
    assert!(
        s.library(&LibraryQuery {
            origin: Some("'; DROP TABLE captures;--".into()),
            ..Default::default()
        })
        .is_err()
    );
}
#[test]
fn excerpts_use_current_body_and_never_recover_archive_hits() {
    let (_d, s) = setup();
    let text = format!("{}这里出现 needleword 中文短语", "很长的开头".repeat(300));
    let saved = capture(&s, &text, "user", None);
    let page = query(&s, "needleword");
    assert_eq!(page.items[0].key.id, saved.memory_id);
    assert!(page.items[0].snippet.contains("needleword"));
    organize(&s, &saved, "其他标题", "现在的摘要");
    assert!(query(&s, "needleword").items.is_empty());
    assert!(query(&s, "中文短语").items.is_empty());
}

#[test]
fn chinese_short_words_urls_and_operators_are_literal() {
    let (_d, s) = setup();
    let raw = capture(&s, "中文 短词 SQLite _% \"引号\" 🙂", "agent", None);
    for term in [
        "中文",
        "词",
        "SQLite",
        "_%",
        "\"引号\"",
        "🙂",
        "https://example.com/中文路径?q=a_b%20",
    ] {
        let page = query(&s, term);
        assert_eq!(page.items.len(), 1, "{term}");
        assert_eq!(page.items[0].key.id, raw.memory_id);
    }
    assert!(query(&s, "NOTEXIST OR").items.is_empty());
    assert!(
        s.library(&LibraryQuery {
            since: Some(i64::MAX),
            ..Default::default()
        })
        .unwrap()
        .items
        .is_empty()
    );
    assert!(
        s.library(&LibraryQuery {
            until: Some(1),
            ..Default::default()
        })
        .unwrap()
        .items
        .is_empty()
    );
}
#[test]
fn edits_are_guarded_preserve_raw_and_restore_adds_history() {
    let (_d, s) = setup();
    let raw = capture(&s, " \n原文要逐字保留\n ", "user", None);
    let mut draft = WorkspaceDraft {
        destination: None,
        context: vec![],
        key: format!("memory:{}", raw.memory_id),
        request_id: id(),
        title: "Manual title".into(),
        body: "当前编辑的内容".into(),
        expected_version: Some(raw.version_id.clone()),
        origin: None,
    };
    let r = s.save_library_edit(&draft).unwrap();
    assert_eq!(
        s.save_library_edit(&draft).unwrap().after_version,
        r.after_version
    );
    assert_eq!(
        s.capture_by_id(&raw.capture_id).unwrap().text,
        " \n原文要逐字保留\n "
    );
    draft.key = format!("memory:{}", key(&r).id);
    draft.expected_version = r.after_version.clone();
    draft.request_id = id();
    draft.body = "新内容".into();
    let second = s.save_library_edit(&draft).unwrap();
    draft.request_id = id();
    assert_eq!(
        s.save_library_edit(&draft).unwrap_err(),
        DataError::Conflict
    );
    s.restore_version(
        &id(),
        &key(&r).id,
        second.after_version.as_ref().unwrap(),
        r.after_version.as_ref().unwrap(),
    )
    .unwrap();
    let d = s.library_detail(&key(&r)).unwrap();
    assert_eq!(d.history.len(), 4);
    assert_eq!(d.body, "当前编辑的内容");
    assert_eq!(
        d.sources[0].capture.as_ref().unwrap().text,
        s.capture_by_id(&raw.capture_id).unwrap().text
    );
}
#[test]
fn drafts_are_private_to_editing_and_survive_restart() {
    let (d, s) = setup();
    let raw = capture(&s, "可搜索记录", "user", None);
    let draft = WorkspaceDraft {
        destination: None,
        context: vec![],
        key: format!("memory:{}", raw.memory_id),
        request_id: id(),
        title: "未保存的草稿标题".into(),
        body: "draft_secret_NOT_MEMORY".into(),
        expected_version: Some(raw.version_id.clone()),
        origin: None,
    };
    s.save_workspace_draft(&draft).unwrap();
    let reopened = MemoryStore::open(d.path()).unwrap();
    assert_eq!(
        reopened.workspace_draft(&draft.key).unwrap().unwrap().body,
        draft.body
    );
    assert!(query(&s, "draft_secret_NOT_MEMORY").items.is_empty());
    assert!(
        s.search(&SearchRequest::text("draft_secret_NOT_MEMORY", 20))
            .unwrap()
            .items
            .is_empty()
    );
    let export = d.path().join("export");
    s.export_markdown(&export).unwrap();
    for entry in std::fs::read_dir(export).unwrap() {
        let entry = entry.unwrap();
        if entry.path().is_file() {
            assert!(
                !std::fs::read_to_string(entry.path())
                    .unwrap()
                    .contains("draft_secret_NOT_MEMORY")
            );
        }
    }
    s.trash_memory(&raw.memory_id, &raw.version_id).unwrap();
    s.purge_memory(&raw.memory_id).unwrap();
    assert!(s.workspace_draft(&draft.key).unwrap().is_none());
    // A delayed UI write must not resurrect text after permanent erasure.
    assert_eq!(
        s.save_workspace_draft(&draft).unwrap_err(),
        DataError::Unavailable
    );
}
#[test]
fn trash_groups_exclusive_originals_and_never_returns_them_in_normal_search() {
    let (_d, s) = setup();
    let raw = capture(&s, "回收站专属标记", "user", None);
    let r = organize(&s, &raw, "回收标题", "回收正文");
    s.trash_memory(&key(&r).id, r.after_version.as_ref().unwrap())
        .unwrap();
    assert!(query(&s, "回收站专属标记").items.is_empty());
    assert!(query(&s, "").items.is_empty());
    let trash = s
        .library(&LibraryQuery {
            trash: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(trash.items.len(), 1);
    assert_eq!(trash.items[0].key.kind, "memory");
    let d = s.library_detail(&key(&r)).unwrap();
    assert_eq!(d.state, "trashed");
    assert_eq!(
        d.sources[0].capture.as_ref().unwrap().text,
        "回收站专属标记"
    );
    s.restore_memory(&key(&r).id).unwrap();
    assert!(query(&s, "回收站专属标记").items.is_empty());
    assert_eq!(query(&s, "回收正文").items.len(), 1);
    s.trash_capture(&raw.capture_id).unwrap();
    assert!(
        s.library_detail(&key(&r)).unwrap().sources[0]
            .capture
            .is_none()
    );
    assert!(query(&s, "回收站专属标记").items.is_empty());
    s.trash_memory(&key(&r).id, r.after_version.as_ref().unwrap())
        .unwrap();
    let deleted_source = s
        .library(&LibraryQuery {
            query: "回收正文".into(),
            trash: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(deleted_source.items.len(), 1);
    assert_eq!(deleted_source.items[0].key.id, raw.memory_id);
    assert_eq!(deleted_source.items[0].key.kind, "memory");
}
#[test]
fn rebuilding_missing_index_recovers_facts_and_preserves_erasure() {
    let (d, s) = setup();
    let keep = capture(&s, "仍然存在的中文", "user", None);
    let erased = capture(&s, "永久擦除的秘密", "user", None);
    let old = organize(&s, &erased, "旧秘密", "永久擦除的秘密");
    s.trash_memory(&key(&old).id, old.after_version.as_ref().unwrap())
        .unwrap();
    s.purge_memory(&key(&old).id).unwrap();
    Connection::open(s.database_path())
        .unwrap()
        .execute_batch("DROP TABLE record_fts")
        .unwrap();
    let reopened = MemoryStore::open(d.path()).unwrap();
    assert_eq!(query(&reopened, "仍然存在").items[0].key.id, keep.memory_id);
    assert!(query(&reopened, "永久擦除").items.is_empty());
    capture(&reopened, "重建后写入", "user", None);
    assert_eq!(query(&reopened, "重建后").items.len(), 1);
    reopened.rebuild_search_index().unwrap();
    reopened.check_integrity().unwrap();
    let n: i64 = Connection::open(s.database_path())
        .unwrap()
        .query_row(
            "SELECT count(*) FROM record_fts WHERE record_fts MATCH ?",
            params!["\"永久擦除\""],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
}
#[test]
fn conversation_text_stays_outside_library_and_project_filter_checks_all_sources() {
    let (_d, s) = setup();
    let conversation = id();
    s.create_conversation(&conversation, "unsaved_chat_marker")
        .unwrap();
    s.save_conversation_draft(&conversation, "unsaved_chat_marker")
        .unwrap();
    let a = capture(&s, "原始来源 A", "agent", Some("工程"));
    let r = organize(&s, &a, "示例记忆", "当前记忆");
    let b = capture(&s, "原始来源 B", "user", Some("生活"));
    s.apply_capture(&ChangeRequest {
        request_id: id(),
        capture_id: b.capture_id,
        destination: Destination::Existing {
            memory_id: key(&r).id.clone(),
            expected_version: r.after_version.unwrap(),
        },
        title: "示例记忆".into(),
        body: "当前记忆".into(),
        actor: Actor::Ai,
    })
    .unwrap();
    assert!(query(&s, "unsaved_chat_marker").items.is_empty());
    for p in ["工程", "生活"] {
        assert_eq!(
            s.library(&LibraryQuery {
                project: Some(p.into()),
                query: "当前记忆".into(),
                ..Default::default()
            })
            .unwrap()
            .items
            .len(),
            1
        );
    }
}

#[test]
fn single_article_export_contains_only_the_selected_saved_title_and_body() {
    use std::os::unix::fs::PermissionsExt;
    let (d, s) = setup();
    let raw = capture(&s, "raw_source_private", "user", None);
    let first = organize(&s, &raw, "历史标题", "old_version_private");
    let current = s
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: key(&first).id,
            expected_version: first.after_version.clone().unwrap(),
            title: "单篇导出 · 中文".into(),
            body: "当前正文\n\n- Markdown 保留\n  缩进与末尾空格  ".into(),
        })
        .unwrap();
    let selected = key(&current);
    s.save_workspace_draft(&WorkspaceDraft {
        destination: None,
        context: vec![],
        key: format!("memory:{}", selected.id),
        request_id: id(),
        title: "draft_title_private".into(),
        body: "draft_body_private".into(),
        expected_version: current.after_version.clone(),
        origin: None,
    })
    .unwrap();
    let other = capture(&s, "other_article_private", "user", None);
    organize(&s, &other, "另一篇", "other_memory_private");
    let chat = id();
    s.create_conversation(&chat, "conversation_private")
        .unwrap();
    s.save_conversation_draft(&chat, "conversation_draft_private")
        .unwrap();
    let folder = d.path().join("article-export");
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("一篇.md");
    s.export_record_markdown(&selected, current.after_version.as_deref(), &file)
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "# 单篇导出 · 中文\n\n当前正文\n\n- Markdown 保留\n  缩进与末尾空格  \n"
    );
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    // No archive/manifest is created, and an approved overwrite stays one file.
    std::fs::write(&file, "previous export").unwrap();
    s.export_record_markdown(&selected, current.after_version.as_deref(), &file)
        .unwrap();
    let before = std::fs::read(&file).unwrap();
    assert_eq!(
        s.export_record_markdown(&selected, first.after_version.as_deref(), &file)
            .unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(std::fs::read(&file).unwrap(), before);
    s.trash_memory(&selected.id, current.after_version.as_ref().unwrap())
        .unwrap();
    assert_eq!(
        s.export_record_markdown(&selected, current.after_version.as_deref(), &file)
            .unwrap_err(),
        DataError::Unavailable
    );
    assert_eq!(std::fs::read(&file).unwrap(), before);
}

#[test]
fn single_raw_export_preserves_text_without_exporting_other_records() {
    let (d, s) = setup();
    let raw = capture(
        &s,
        "原始记录\n\n 中文、https://example.com/a_b%20\n ",
        "user",
        None,
    );
    capture(&s, "无关记录", "user", None);
    let selected = RecordKey {
        kind: "capture".into(),
        id: raw.capture_id.clone(),
    };
    let file = d.path().join("原话.md");
    s.export_record_markdown(&selected, None, &file).unwrap();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        format!(
            "# 原始记录\n\n{}\n",
            s.capture_by_id(&raw.capture_id).unwrap().text
        )
    );
    assert_eq!(
        s.export_record_markdown(&selected, None, d.path().join("memivy.db"))
            .unwrap_err(),
        DataError::Invalid
    );
    s.check_integrity().unwrap();
}
