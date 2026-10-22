use memivy_core::memory::*;
use rusqlite::Connection;

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn setup() -> (tempfile::TempDir, MemoryStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    (dir, store)
}
fn request(files: &[(&str, &str)]) -> ImportRequest {
    ImportRequest {
        request_id: id(),
        files: files
            .iter()
            .map(|(name, text)| ImportFile {
                file_name: (*name).into(),
                text: (*text).into(),
            })
            .collect(),
    }
}
fn counts(store: &MemoryStore) -> Vec<i64> {
    let db = Connection::open(store.database_path()).unwrap();
    [
        "captures",
        "memories",
        "memory_versions",
        "receipts",
        "receipt_changes",
        "import_batches",
        "import_sources",
        "turns",
        "messages",
    ]
    .iter()
    .map(|table| {
        db.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    })
    .collect()
}

#[test]
fn preserves_exact_unicode_bom_crlf_originals_and_filename_titles_without_ai() {
    let (_dir, store) = setup();
    let text = "\u{feff}# 原始标题\r\n\r\n也许十一月上线，不是已决定。\r\n\t- [ ] 保留表情 🙂\r\n";
    let long_name = format!("{}.MD", "项目".repeat(37));
    let result = store
        .import_files(&request(&[
            ("项目想法.md", text),
            (&long_name, "Another source"),
        ]))
        .unwrap();
    assert_eq!(result.items.len(), 2);
    let first = store.memory(&result.items[0].memory_id).unwrap();
    assert_eq!(first.current.title, "项目想法");
    assert_eq!(first.current.body, text);
    let raw = store.capture_by_id(&first.current.capture_ids[0]).unwrap();
    assert_eq!(raw.text.as_bytes(), text.as_bytes());
    assert_eq!(
        raw.origin,
        Origin::Imported {
            file_name: "项目想法.md".into()
        }
    );
    let title = store
        .memory(&result.items[1].memory_id)
        .unwrap()
        .current
        .title;
    assert!(title.len() <= 200);
    assert!(long_name.starts_with(&title));
    assert_eq!(store.receipt_changes(&result.request_id).unwrap().len(), 2);
    for item in &result.items {
        let receipts = store
            .memory_receipts(&RecordKey {
                kind: "memory".into(),
                id: item.memory_id.clone(),
            })
            .unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].receipt.action, "import");
    }
    assert_eq!(&counts(&store)[7..], &[0, 0]);
    store.check_integrity().unwrap();
}

#[test]
fn receipt_projection_tracks_each_imported_version_without_changing_grouped_undo() {
    let (_dir, store) = setup();
    let result = store
        .import_files(&request(&[
            ("one.md", "First source"),
            ("two.md", "Second source"),
            ("three.md", "Third source"),
        ]))
        .unwrap();
    let canonical = result.receipt.as_ref().unwrap();
    let before_reads = counts(&store);
    for item in &result.items {
        let receipts = store
            .memory_receipts(&RecordKey {
                kind: "memory".into(),
                id: item.memory_id.clone(),
            })
            .unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(&receipts[0].receipt, canonical);
        assert_eq!(
            receipts[0].record_after_version.as_deref(),
            Some(item.version_id.as_str())
        );
        assert_eq!(receipts[0].logical_input_id, None);
        let serialized = serde_json::to_value(&receipts[0]).unwrap();
        assert_eq!(serialized["record_after_version"], item.version_id);
        assert_eq!(serialized["after_version"], result.items[0].version_id);
        assert!(serialized["logical_input_id"].is_null());
    }
    assert_eq!(counts(&store), before_reads);

    let edited = &result.items[1];
    let edit = store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: edited.memory_id.clone(),
            expected_version: edited.version_id.clone(),
            title: "Edited second source".into(),
            body: "Later work".into(),
        })
        .unwrap();
    let key = RecordKey {
        kind: "memory".into(),
        id: edited.memory_id.clone(),
    };
    let receipts = store.memory_receipts(&key).unwrap();
    let import = receipts
        .iter()
        .find(|receipt| receipt.receipt.request_id == result.request_id)
        .unwrap();
    assert_eq!(&import.receipt, canonical);
    assert_eq!(
        import.record_after_version.as_deref(),
        Some(edited.version_id.as_str())
    );
    let current = store.memory(&edited.memory_id).unwrap().current;
    assert_ne!(
        import.record_after_version.as_deref(),
        Some(current.id.as_str())
    );
    assert_eq!(receipts[0].receipt, edit);
    assert_eq!(
        receipts[0].record_after_version.as_deref(),
        Some(current.id.as_str())
    );
    assert_eq!(
        store.undo(&id(), &import.receipt.request_id).unwrap_err(),
        DataError::Conflict
    );
    for item in &result.items {
        let expected = if item.memory_id == edited.memory_id {
            &current.id
        } else {
            &item.version_id
        };
        assert_eq!(&store.memory(&item.memory_id).unwrap().current.id, expected);
    }

    store.undo(&id(), &edit.request_id).unwrap();
    assert_eq!(
        store.undo(&id(), &import.receipt.request_id).unwrap_err(),
        DataError::Conflict
    );
    let clean = store
        .import_files(&request(&[
            ("clean-one.md", "First clean source"),
            ("clean-two.md", "Second clean source"),
            ("clean-three.md", "Third clean source"),
        ]))
        .unwrap();
    let later_member_receipt = store
        .memory_receipts(&RecordKey {
            kind: "memory".into(),
            id: clean.items[2].memory_id.clone(),
        })
        .unwrap();
    store
        .undo(&id(), &later_member_receipt[0].receipt.request_id)
        .unwrap();
    assert_eq!(store.receipt(&clean.request_id).unwrap().status, "undone");
    for item in &clean.items {
        assert!(store.memory(&item.memory_id).is_err());
        assert!(
            store
                .memory_receipts(&RecordKey {
                    kind: "memory".into(),
                    id: item.memory_id.clone(),
                })
                .unwrap()
                .is_empty()
        );
    }
    store.check_integrity().unwrap();
}

#[test]
fn invalid_batch_and_mid_transaction_failure_never_leave_partial_imports() {
    let (_dir, store) = setup();
    let baseline = counts(&store);
    let invalid = [
        request(&[]),
        request(&[("../secret.md", "Text")]),
        request(&[("C:\\secret.txt", "Text")]),
        request(&[(".md", "Text")]),
        request(&[("image.png", "Text")]),
        request(&[("bad\nname.md", "Text")]),
        request(&[("empty.txt", "\u{feff} \r\n\t")]),
        request(&[("binary.txt", "bad\0text")]),
        request(&[("binary.txt", "bad\u{1b}text")]),
        request(&[("valid.md", "First"), ("invalid.txt", "")]),
        request(&[(&format!("{}.md", "x".repeat(253)), "Long filename")]),
    ];
    for invalid in invalid {
        assert_eq!(
            store.import_files(&invalid).unwrap_err(),
            DataError::Invalid
        );
        assert_eq!(counts(&store), baseline);
    }
    let mut invalid = request(&[("valid.md", "Text")]);
    invalid.request_id = "invalid-id".into();
    assert_eq!(
        store.import_files(&invalid).unwrap_err(),
        DataError::Invalid
    );
    let db = Connection::open(store.database_path()).unwrap();
    db.execute_batch("CREATE TRIGGER fail_second_import BEFORE INSERT ON captures WHEN NEW.text='Reject transaction' BEGIN SELECT RAISE(ABORT,'Synthetic failure'); END;").unwrap();
    assert_eq!(
        store
            .import_files(&request(&[
                ("first.md", "First success"),
                ("second.md", "Reject transaction")
            ]))
            .unwrap_err(),
        DataError::Database
    );
    assert_eq!(counts(&store), baseline);
    store.check_integrity().unwrap();
}

#[test]
fn enforces_file_count_byte_and_total_limits_at_boundaries() {
    let (_dir, store) = setup();
    let mut many = request(&[]);
    for index in 0..IMPORT_MAX_FILES {
        many.files.push(ImportFile {
            file_name: format!("source-{index}.txt"),
            text: format!("Unique source {index}"),
        });
    }
    assert_eq!(
        store.import_files(&many).unwrap().items.len(),
        IMPORT_MAX_FILES
    );
    many.request_id = id();
    many.files.push(ImportFile {
        file_name: "extra.txt".into(),
        text: "Extra".into(),
    });
    assert_eq!(store.import_files(&many).unwrap_err(), DataError::Invalid);
    let maximum = "x".repeat(IMPORT_MAX_FILE_BYTES);
    assert!(
        store
            .import_files(&request(&[("maximum.txt", &maximum)]))
            .is_ok()
    );
    assert_eq!(
        store
            .import_files(&request(&[("large.txt", &format!("{maximum}x"))]))
            .unwrap_err(),
        DataError::Invalid
    );
    let mut total = request(&[]);
    for index in 0..16 {
        total.files.push(ImportFile {
            file_name: format!("large-{index}.txt"),
            text: maximum.clone(),
        });
    }
    assert_eq!(
        total
            .files
            .iter()
            .map(|file| file.text.len())
            .sum::<usize>(),
        IMPORT_MAX_TOTAL_BYTES
    );
    assert!(store.import_files(&total).is_ok());
    total.request_id = id();
    total.files.push(ImportFile {
        file_name: "over.txt".into(),
        text: "x".into(),
    });
    assert_eq!(store.import_files(&total).unwrap_err(), DataError::Invalid);
}

#[test]
fn deduplicates_only_active_imported_content_across_names_and_inside_batches() {
    let (_dir, store) = setup();
    let text = "Exact document content";
    let ordinary = store
        .capture(&CaptureRequest {
            request_id: id(),
            text: text.into(),
            origin: Origin::User {
                app: "Test".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap();
    let first = store
        .import_files(&request(&[
            ("first.md", text),
            ("renamed.txt", text),
            ("first.md", "Different content"),
        ]))
        .unwrap();
    assert_eq!(
        first
            .items
            .iter()
            .map(|item| item.status)
            .collect::<Vec<_>>(),
        vec![
            ImportStatus::Imported,
            ImportStatus::Duplicate,
            ImportStatus::Imported
        ]
    );
    assert_ne!(first.items[0].memory_id, ordinary.memory_id);
    assert_eq!(first.items[1].memory_id, first.items[0].memory_id);
    assert_eq!(store.receipt_changes(&first.request_id).unwrap().len(), 2);
    let duplicate_request = request(&[("another.md", text)]);
    let duplicate = store.import_files(&duplicate_request).unwrap();
    assert_eq!(duplicate.items[0].status, ImportStatus::Duplicate);
    assert!(duplicate.receipt.is_none());
    assert_eq!(store.import_files(&duplicate_request).unwrap(), duplicate);
    assert_eq!(
        store
            .capture(&CaptureRequest {
                request_id: duplicate_request.request_id,
                text: "Reuse".into(),
                origin: Origin::User {
                    app: "Test".into(),
                    project: None,
                    uri: None
                }
            })
            .unwrap_err(),
        DataError::RequestConflict
    );
    store
        .trash_memory(&first.items[0].memory_id, &first.items[0].version_id)
        .unwrap();
    let reimport = store.import_files(&request(&[("first.md", text)])).unwrap();
    assert_eq!(reimport.items[0].status, ImportStatus::Imported);
    assert_ne!(reimport.items[0].memory_id, first.items[0].memory_id);
    assert_eq!(
        store
            .capture(&CaptureRequest {
                request_id: id(),
                text: "Do not bypass import".into(),
                origin: Origin::Imported {
                    file_name: "file.md".into()
                }
            })
            .unwrap_err(),
        DataError::Invalid
    );
}

#[test]
fn retry_is_idempotent_conflicts_on_change_and_never_resurrects_undo_or_trash() {
    let (_dir, store) = setup();
    let original = request(&[("one.md", "First document"), ("two.txt", "Second document")]);
    let result = store.import_files(&original).unwrap();
    let before = counts(&store);
    assert_eq!(store.import_files(&original).unwrap(), result);
    assert_eq!(counts(&store), before);
    let mut changed = original.clone();
    changed.files[0].file_name = "renamed.md".into();
    assert_eq!(
        store.import_files(&changed).unwrap_err(),
        DataError::RequestConflict
    );
    store.undo(&id(), &result.request_id).unwrap();
    let after_undo = counts(&store);
    let replay = store.import_files(&original).unwrap();
    assert_eq!(replay.receipt.unwrap().status, "undone");
    assert_eq!(replay.items, result.items);
    assert_eq!(counts(&store), after_undo);
    for item in &result.items {
        assert!(store.memory(&item.memory_id).is_err());
    }
    let trash_request = request(&[("trash.md", "Trash document")]);
    let trash = store.import_files(&trash_request).unwrap().items.remove(0);
    store
        .trash_memory(&trash.memory_id, &trash.version_id)
        .unwrap();
    let before = counts(&store);
    store.import_files(&trash_request).unwrap();
    assert!(store.memory(&trash.memory_id).is_err());
    assert_eq!(counts(&store), before);
}

#[test]
fn grouped_undo_protects_later_edits_and_preserves_unrelated_duplicate_memories() {
    let (_dir, store) = setup();
    let existing = store
        .import_files(&request(&[("existing.md", "Existing import")]))
        .unwrap()
        .items
        .remove(0);
    let result = store
        .import_files(&request(&[
            ("copy.md", "Existing import"),
            ("one.md", "First new"),
            ("two.md", "Second new"),
        ]))
        .unwrap();
    let first = &result.items[1];
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: first.memory_id.clone(),
            expected_version: first.version_id.clone(),
            title: "Edited".into(),
            body: "Later work".into(),
        })
        .unwrap();
    assert_eq!(
        store.undo(&id(), &result.request_id).unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(
        store.memory(&first.memory_id).unwrap().current.body,
        "Later work"
    );
    assert_eq!(
        store.memory(&result.items[2].memory_id).unwrap().current.id,
        result.items[2].version_id
    );
    assert_eq!(store.receipt(&result.request_id).unwrap().status, "applied");
    let clean = store
        .import_files(&request(&[
            ("copy.md", "Existing import"),
            ("new.md", "Undo only this new memory"),
        ]))
        .unwrap();
    store.undo(&id(), &clean.request_id).unwrap();
    assert!(store.memory(&clean.items[1].memory_id).is_err());
    assert_eq!(
        store.memory(&existing.memory_id).unwrap().current.id,
        existing.version_id
    );
}

#[test]
fn grouped_undo_preserves_unsaved_drafts_on_any_imported_memory() {
    let (_dir, store) = setup();
    let result = store
        .import_files(&request(&[
            ("one.md", "First source"),
            ("two.md", "Second source"),
        ]))
        .unwrap();
    let item = &result.items[1];
    let draft = WorkspaceDraft {
        key: format!("memory:{}", item.memory_id),
        request_id: id(),
        title: "Draft title".into(),
        body: "Unsubmitted later work".into(),
        expected_version: Some(item.version_id.clone()),
        destination: None,
        origin: None,
        context: vec![],
    };
    store.save_workspace_draft(&draft).unwrap();
    assert_eq!(
        store.undo(&id(), &result.request_id).unwrap_err(),
        DataError::Conflict
    );
    for item in &result.items {
        assert_eq!(
            store.memory(&item.memory_id).unwrap().current.id,
            item.version_id
        );
    }
    assert_eq!(
        store.workspace_draft(&draft.key).unwrap().unwrap().body,
        draft.body
    );
}

#[test]
fn purge_erases_imported_text_and_filename_from_backup_without_retry_resurrection() {
    let (dir, store) = setup();
    let file_name = "ConfidentialImportFilename-502e.md";
    let text = "ConfidentialImportBody-502e Original text to erase";
    let request = request(&[(file_name, text)]);
    let result = store.import_files(&request).unwrap();
    let item = &result.items[0];
    store
        .trash_memory(&item.memory_id, &item.version_id)
        .unwrap();
    store.purge_memory(&item.memory_id).unwrap();
    let before = counts(&store);
    assert_eq!(store.import_files(&request).unwrap().items, result.items);
    assert_eq!(counts(&store), before);
    assert!(store.memory(&item.memory_id).is_err());
    let backup = dir.path().join("purged-import.db");
    store.backup(&backup).unwrap();
    let bytes = std::fs::read(backup).unwrap();
    for secret in [file_name, "ConfidentialImportFilename-502e", text] {
        assert!(
            !bytes
                .windows(secret.len())
                .any(|bytes| bytes == secret.as_bytes())
        );
    }
    store.check_integrity().unwrap();
}

#[test]
fn imports_are_searchable_and_backup_restore_keeps_sources_deduplication_and_undo() {
    let (dir, store) = setup();
    let request = request(&[("archive.md", "ImportSearchToken 原始材料，不代表用户观点。")]);
    let result = store.import_files(&request).unwrap();
    let mut query = SearchRequest::text("ImportSearchToken", 10);
    query.scope.origin = Some("imported".into());
    let found = store.search(&query).unwrap();
    assert_eq!(found.items.len(), 1);
    assert!(
        matches!(&found.items[0].origins[0],Origin::Imported {file_name} if file_name=="archive.md")
    );
    assert_eq!(
        store
            .library(&LibraryQuery {
                origin: Some("imported".into()),
                ..Default::default()
            })
            .unwrap()
            .items
            .len(),
        1
    );
    query.scope.origin = Some("user".into());
    assert!(store.search(&query).unwrap().items.is_empty());
    let backup = dir.path().join("import-backup.db");
    store.backup(&backup).unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let target = MemoryStore::open(target_dir.path()).unwrap();
    let prepared = target.prepare_restore(&backup).unwrap();
    target.arm_restore(&prepared.id).unwrap();
    let restored = MemoryStore::open_application(target_dir.path()).unwrap();
    assert_eq!(restored.import_files(&request).unwrap(), result);
    let raw_id = restored
        .memory(&result.items[0].memory_id)
        .unwrap()
        .current
        .capture_ids
        .remove(0);
    assert_eq!(
        restored.capture_by_id(&raw_id).unwrap().text,
        request.files[0].text
    );
    restored.undo(&id(), &result.request_id).unwrap();
    assert!(
        restored
            .search(&SearchRequest::text("ImportSearchToken", 10))
            .unwrap()
            .items
            .is_empty()
    );
    restored.check_integrity().unwrap();
}
