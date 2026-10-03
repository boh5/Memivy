use memivy_core::memory::*;
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn setup() -> (tempfile::TempDir, MemoryStore) {
    let d = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(d.path()).unwrap();
    (d, s)
}
fn capture(s: &MemoryStore, text: &str) -> RecordKey {
    let c = s
        .capture(&CaptureRequest {
            request_id: id(),
            text: text.into(),
            origin: Origin::User {
                app: "test".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap();
    RecordKey {
        kind: "memory".into(),
        id: c.memory_id,
    }
}
fn memory(s: &MemoryStore, text: &str) -> (RecordKey, Receipt) {
    let c = capture(s, text);
    let r = s
        .edit_memory(&EditRequest {
            request_id: id(),
            expected_version: s.memory(&c.id).unwrap().current.id,
            memory_id: c.id,
            title: text.into(),
            body: text.into(),
        })
        .unwrap();
    (
        RecordKey {
            kind: "memory".into(),
            id: r.memory_id.clone().unwrap(),
        },
        r,
    )
}
fn collection(s: &MemoryStore, name: &str) -> String {
    let c = id();
    s.save_collection(&c, name, "", None).unwrap();
    c
}

#[test]
fn pins_collections_and_restore_preserve_content_and_original_identity() {
    let (_d, s) = setup();
    let raw = capture(&s, "原话不可改");
    let c = collection(&s, "产品想法");
    s.pin_record(&raw, true).unwrap();
    s.pin_record(&raw, true).unwrap();
    s.collect_record(&c, &raw, true).unwrap();
    s.collect_record(&c, &raw, true).unwrap();
    let r = s
        .edit_memory(&EditRequest {
            request_id: id(),
            expected_version: s.memory(&raw.id).unwrap().current.id,
            memory_id: raw.id.clone(),
            title: "Organized title".into(),
            body: "整理正文".into(),
        })
        .unwrap();
    assert_eq!(
        s.library(&LibraryQuery {
            pinned: true,
            ..Default::default()
        })
        .unwrap()
        .items[0]
            .key
            .id,
        raw.id
    );
    assert_eq!(s.collections().unwrap()[0].count, 1);
    s.trash_memory(&raw.id, r.after_version.as_ref().unwrap())
        .unwrap();
    assert!(
        s.library(&LibraryQuery {
            pinned: true,
            ..Default::default()
        })
        .unwrap()
        .items
        .is_empty()
    );
    assert_eq!(s.collections().unwrap()[0].count, 0);
    s.restore_memory(&raw.id).unwrap();
    assert!(s.record_navigation(&raw).unwrap().pinned);
    s.collect_record(&c, &raw, false).unwrap();
    assert_eq!(s.collections().unwrap()[0].count, 0);
    assert_eq!(s.library_detail(&raw).unwrap().body, "整理正文");
    assert_eq!(
        s.library_detail(&raw).unwrap().sources[0]
            .capture
            .as_ref()
            .unwrap()
            .text,
        "原话不可改"
    );
    assert_eq!(
        s.library_detail(&RecordKey {
            kind: "memory".into(),
            id: r.memory_id.unwrap()
        })
        .unwrap()
        .history
        .len(),
        2
    );
}
#[test]
fn collection_metadata_conflicts_and_removal_do_not_delete_records_or_broaden_chat() {
    let (_d, s) = setup();
    let (m, _) = memory(&s, "面试原话");
    let c = collection(&s, "面试");
    let other = collection(&s, "产品");
    let created_revision = s.read_agent_collection(&c).unwrap().revision;
    s.save_collection(&c, "面试", "", None).unwrap();
    assert_eq!(s.collections().unwrap().len(), 2);
    assert_eq!(
        s.read_agent_collection(&c).unwrap().revision,
        created_revision
    );
    s.collect_record(&c, &m, true).unwrap();
    s.collect_record(&other, &m, true).unwrap();
    assert_eq!(s.record_navigation(&m).unwrap().collections.len(), 2);
    let revision = s.read_agent_collection(&c).unwrap().revision;
    s.save_collection(&c, "面试", "", Some(created_revision))
        .unwrap();
    assert_eq!(s.read_agent_collection(&c).unwrap().revision, revision);
    s.save_collection(&c, "面试准备", "关注技术问答", Some(revision))
        .unwrap();
    let renamed_revision = s.read_agent_collection(&c).unwrap().revision;
    s.save_collection(&c, "面试准备", "关注技术问答", Some(revision))
        .unwrap();
    assert_eq!(
        s.read_agent_collection(&c).unwrap().revision,
        renamed_revision
    );
    assert!(matches!(
        s.save_collection(&c, "陈旧修改", "", Some(revision)),
        Err(DataError::Conflict)
    ));
    assert!(s.save_collection(&id(), "面试准备", "", None).is_err());
    let topic = id();
    s.create_scoped_conversation(&topic, "Discussion", Some(&c))
        .unwrap();
    assert_eq!(
        s.conversation(&topic).unwrap().collection_id.as_deref(),
        Some(c.as_str())
    );
    assert!(s.create_conversation(&topic, "Discussion").is_err());
    let renamed_revision = s.read_agent_collection(&c).unwrap().revision;
    s.archive_collection(&c, true, renamed_revision).unwrap();
    assert_eq!(
        s.save_collection(&c, "面试准备", "关注技术问答", Some(renamed_revision)),
        Err(DataError::Unavailable)
    );
    assert!(
        s.search(&SearchRequest {
            queries: vec![MemoryQuery::text("面试")],
            scope: SearchScope {
                collection_id: Some(c.clone()),
                ..Default::default()
            },
            ..Default::default()
        })
        .is_err()
    );
    assert_eq!(s.library_detail(&m).unwrap().body, "面试原话");
    s.archive_collection(&c, false, renamed_revision + 1)
        .unwrap();
    assert_eq!(
        s.collections()
            .unwrap()
            .iter()
            .find(|v| v.id == c)
            .unwrap()
            .count,
        1
    );
}
#[test]
fn explicit_topic_search_filters_before_ranking_and_keeps_history_separate() {
    let (_d, s) = setup();
    let (inside, r) = memory(&s, "木桥最初计划离线");
    let c = collection(&s, "木桥");
    s.collect_record(&c, &inside, true).unwrap();
    let current = s
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: inside.id.clone(),
            expected_version: r.after_version.clone().unwrap(),
            title: "木桥新方向".into(),
            body: "木桥决定本地优先".into(),
        })
        .unwrap();
    let mut outsider = None;
    for _ in 0..35 {
        outsider = Some(memory(&s, "木桥外部干扰记录").1);
    }
    let sources = s
        .search(&SearchRequest {
            queries: vec![MemoryQuery::text("木桥")],
            scope: SearchScope {
                collection_id: Some(c.clone()),
                ..Default::default()
            },
            ..Default::default()
        })
        .map(|r| {
            r.items
                .into_iter()
                .map(|h| h.evidence.source)
                .collect::<Vec<_>>()
        })
        .unwrap();
    assert!(!sources.is_empty());
    assert!(sources.contains(&SourceRef::Version(current.after_version.unwrap())));
    assert!(!sources.contains(&SourceRef::Version(r.after_version.clone().unwrap())));
    assert!(sources.iter().all(|v| {
        match v {
            SourceRef::Capture(id) => Some(id) == r.capture_id.as_ref(),
            SourceRef::Version(id) => {
                s.resolve_source(&SourceRef::Version(id.clone()), 1000)
                    .unwrap()
                    .text
                    .contains("木桥")
                    && id != outsider.as_ref().unwrap().after_version.as_ref().unwrap()
            }
        }
    }));
    // Conversation focus can include an outside memory; it is not a read filter.
    let outside = outsider.unwrap();
    let focused = s
        .agent_focused_memories(&[SourceRef::Version(outside.after_version.unwrap())])
        .unwrap();
    assert_eq!(focused, vec![outside.memory_id.unwrap()]);
}
#[test]
fn collection_list_and_recommendation_exclusion_apply_before_limit() {
    let (_d, s) = setup();
    let (member, _) = memory(&s, "相关内容 较早");
    let c = collection(&s, "Collection");
    s.collect_record(&c, &member, true).unwrap();
    for _ in 0..45 {
        memory(&s, "相关内容 无关专题");
    }
    let p = s
        .library(&LibraryQuery {
            collection_id: Some(c.clone()),
            limit: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(p.items[0].key.id, member.id);
    assert!(p.next_offset.is_none());
    let p = s
        .library(&LibraryQuery {
            query: "相关内容".into(),
            exclude_collection_id: Some(c),
            limit: 5,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(p.items.len(), 5);
    assert!(p.items.iter().all(|r| r.key.id != member.id));
    let first = s
        .library(&LibraryQuery {
            oldest: true,
            limit: 3,
            ..Default::default()
        })
        .unwrap();
    let next = s
        .library(&LibraryQuery {
            oldest: true,
            limit: 3,
            offset: first.next_offset.unwrap(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(first.items.len(), 3);
    assert!(
        next.items
            .iter()
            .all(|r| first.items.iter().all(|f| f.key.id != r.key.id))
    );
}
#[test]
fn navigation_survives_backup_and_purge_removes_metadata() {
    let (d, s) = setup();
    let (m, r) = memory(&s, "可恢复");
    let c = collection(&s, "长期专题");
    s.pin_record(&m, true).unwrap();
    s.collect_record(&c, &m, true).unwrap();
    let backup = d.path().join("navigation.backup");
    s.backup(&backup).unwrap();
    let prepared = s.prepare_restore(&backup).unwrap();
    s.arm_restore(&prepared.id).unwrap();
    let s = MemoryStore::open_application(d.path()).unwrap();
    assert!(s.record_navigation(&m).unwrap().pinned);
    assert_eq!(s.collections().unwrap()[0].count, 1);
    s.trash_memory(&m.id, r.after_version.as_ref().unwrap())
        .unwrap();
    s.purge_memory(&m.id).unwrap();
    assert_eq!(s.collections().unwrap()[0].count, 0);
    assert!(
        s.library(&LibraryQuery {
            pinned: true,
            ..Default::default()
        })
        .unwrap()
        .items
        .is_empty()
    );
}

#[test]
#[ignore = "explicit 10000-record navigation performance check"]
fn navigation_performance_10000_records() {
    use rusqlite::{Connection, params};
    use std::time::Instant;
    let (_d, s) = setup();
    let collection = collection(&s, "蓝鲸旅行");
    let mut db = Connection::open(s.database_path()).unwrap();
    let tx = db.transaction().unwrap();
    let mut member = String::new();
    for n in 0..10000 {
        let capture = id();
        let memory = id();
        if n == 1 {
            member = memory.clone();
        }
        tx.execute(
            "INSERT INTO captures VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                capture,
                id(),
                vec![0u8; 32],
                format!("蓝鲸旅行 住宿想法 {n}"),
                r#"{"kind":"user","app":"QA"}"#,
                n
            ],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO capture_state(capture_id) VALUES(?)",
            [&capture],
        )
        .unwrap();
        let version = id();
        tx.execute(
            "INSERT INTO memories(id,state,created_at,updated_at) VALUES(?,'active',0,0)",
            [&memory],
        )
        .unwrap();
        tx.execute("INSERT INTO memory_versions(id,memory_id,title,body,actor,reason,created_at) VALUES(?1,?2,'蓝鲸旅行',?3,'user','create',?4)", params![version, memory, format!("蓝鲸旅行 住宿想法 {n}"), n]).unwrap();
        tx.execute(
            "UPDATE memories SET current_version_id=?1 WHERE id=?2",
            params![version, memory],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO version_captures VALUES(?1,?2)",
            params![version, capture],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    drop(db);
    let key = RecordKey {
        kind: "memory".into(),
        id: member,
    };
    s.pin_record(&key, true).unwrap();
    s.collect_record(&collection, &key, true).unwrap();
    let _status_keys: Vec<_> = s
        .library(&LibraryQuery {
            limit: 100,
            ..Default::default()
        })
        .unwrap()
        .items
        .into_iter()
        .map(|r| r.key)
        .collect();
    for mode in ["pins", "collection", "review", "scoped_rag", "status_100"] {
        let mut times = Vec::new();
        for _ in 0..20 {
            let start = Instant::now();
            if mode == "status_100" {
            } else if mode == "scoped_rag" {
                assert_eq!(
                    s.search(&SearchRequest {
                        queries: vec![MemoryQuery::text("蓝鲸旅行")],
                        scope: SearchScope {
                            collection_id: Some(collection.clone()),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .map(|r| r
                        .items
                        .into_iter()
                        .map(|h| h.evidence.source)
                        .collect::<Vec<_>>())
                    .unwrap()
                    .len(),
                    1
                );
            } else {
                let q = LibraryQuery {
                    pinned: mode == "pins",
                    collection_id: (mode == "collection").then(|| collection.clone()),
                    oldest: mode == "review",
                    limit: if mode == "review" { 3 } else { 100 },
                    ..Default::default()
                };
                let page = s.library(&q).unwrap();
                assert_eq!(page.items.len(), if mode == "review" { 3 } else { 1 });
            }
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        println!(
            "{mode} 10000 records p50={:.2}ms p95={:.2}ms",
            times[10], times[18]
        );
    }
}

#[test]
fn collection_confirmation_is_explicit_idempotent_and_rejects_stale_targets() {
    let (_d, s) = setup();
    let raw = capture(&s, "整理完成后推荐专题，保留这段原话");
    let current = s.memory(&raw.id).unwrap().current;
    let key = RecordKey {
        kind: "memory".into(),
        id: raw.id.clone(),
    };
    let c = collection(&s, "产品设计");
    let revision = s.collections().unwrap()[0].revision;
    assert!(s.record_navigation(&key).unwrap().collections.is_empty());
    for _ in 0..2 {
        s.accept_collection_recommendation(&raw.id, &current.id, &c, revision)
            .unwrap();
    }
    assert_eq!(s.collections().unwrap()[0].count, 1);
    assert_eq!(
        s.library_detail(&raw).unwrap().body,
        "整理完成后推荐专题，保留这段原话"
    );
    s.collect_record(&c, &key, false).unwrap();
    assert_eq!(s.collections().unwrap()[0].count, 0);
    s.save_collection(
        &c,
        "新方向",
        "",
        Some(s.read_agent_collection(&c).unwrap().revision),
    )
    .unwrap();
    assert!(
        s.accept_collection_recommendation(&raw.id, &current.id, &c, revision)
            .is_err()
    );
    let revision = s.collections().unwrap()[0].revision;
    s.trash_memory(&key.id, &current.id).unwrap();
    assert!(
        s.accept_collection_recommendation(&raw.id, &current.id, &c, revision)
            .is_err()
    );
    s.restore_memory(&key.id).unwrap();
    s.edit_memory(&EditRequest {
        request_id: id(),
        memory_id: raw.id.clone(),
        expected_version: current.id.clone(),
        title: "Changed".into(),
        body: "Changed topic".into(),
    })
    .unwrap();
    assert!(
        s.accept_collection_recommendation(&raw.id, &current.id, &c, revision)
            .is_err()
    );
    assert_eq!(s.collections().unwrap()[0].count, 0);
}

#[test]
fn collection_confirmation_rejects_a_newer_memory_version() {
    let (_d, s) = setup();
    let raw = capture(&s, "记录体验需要保持简单");
    let current = s.memory(&raw.id).unwrap().current;
    let c = collection(&s, "产品");
    let revision = s.collections().unwrap()[0].revision;
    s.edit_memory(&EditRequest {
        request_id: id(),
        memory_id: raw.id.clone(),
        expected_version: current.id.clone(),
        title: "新的主题".into(),
        body: "这条记忆已改为另一件事".into(),
    })
    .unwrap();
    assert!(
        s.accept_collection_recommendation(&raw.id, &current.id, &c, revision)
            .is_err()
    );
    assert_eq!(s.collections().unwrap()[0].count, 0);
}

#[tokio::test]
async fn recommendation_skips_model_when_no_eligible_collections_exist() {
    let (_d, s) = setup();
    let raw = capture(&s, "木桥产品记录体验");
    let current = s.memory(&raw.id).unwrap().current;
    // An invalid endpoint would fail immediately if a model call were attempted.
    let config = memivy_core::model::ModelConfig {
        provider: Default::default(),
        base_url: "invalid".into(),
        model: "unused".into(),
        api_key: None,
        max_output_tokens: None,
        output_token_parameter: Default::default(),
        disable_reasoning: false,
    };
    assert!(
        s.recommend_collections(&config, &raw.id, &current.id)
            .await
            .unwrap()
            .is_empty()
    );
    let c = collection(&s, "木桥产品");
    s.collect_record(
        &c,
        &RecordKey {
            kind: "memory".into(),
            id: raw.id.clone(),
        },
        true,
    )
    .unwrap();
    assert!(
        s.recommend_collections(&config, &raw.id, &current.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[test]
fn related_memories_keep_collection_discussions_within_their_scope() {
    let (_d, s) = setup();
    let c = collection(&s, "旅行");
    let (seed, seed_receipt) = memory(&s, "蓝鲸旅行");
    let (inside, _) = memory(&s, "蓝鲸旅行住宿");
    s.collect_record(&c, &seed, true).unwrap();
    s.collect_record(&c, &inside, true).unwrap();
    // Newer global matches must not crowd out the member before the hit limit.
    for _ in 0..26 {
        memory(&s, "蓝鲸旅行酒店");
    }
    let related = s
        .related_memories_in_collection(
            &seed.id,
            seed_receipt.after_version.as_ref().unwrap(),
            Some(&c),
        )
        .unwrap();
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].memory_id, inside.id);
    assert_eq!(
        s.agent_focused_memories(&[
            SourceRef::Version(seed_receipt.after_version.clone().unwrap()),
            related[0].source.clone()
        ])
        .unwrap()
        .len(),
        2
    );
    s.collect_record(&c, &inside, false).unwrap();
    assert!(
        s.related_memories_in_collection(
            &seed.id,
            seed_receipt.after_version.as_ref().unwrap(),
            Some(&c)
        )
        .unwrap()
        .is_empty()
    );
    s.archive_collection(&c, true, s.read_agent_collection(&c).unwrap().revision)
        .unwrap();
    assert!(
        s.related_memories_in_collection(
            &seed.id,
            seed_receipt.after_version.as_ref().unwrap(),
            Some(&c)
        )
        .is_err()
    );
}
