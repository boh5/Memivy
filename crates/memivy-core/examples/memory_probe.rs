//! Phase 2 process test harness. Uses only synthetic content and an explicit root.
use memivy_core::memory::*;
use std::{io::Write, path::PathBuf};
use uuid::Uuid;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        // Let process tests distinguish retryable lock contention from errors.
        std::process::exit(if error == DataError::Busy { 75 } else { 1 });
    }
}
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let root = args.get(1).ok_or(DataError::Invalid)?;
    let command = args.get(2).ok_or(DataError::Invalid)?;
    if command == "open-application" {
        let store = MemoryStore::open_application(root)?;
        println!(
            "{}",
            serde_json::to_string(&store.last_restore_result()?).map_err(|_| DataError::Invalid)?
        );
        return Ok(());
    }
    if command == "restore" {
        MemoryStore::restore_backup(args.get(3).ok_or(DataError::Invalid)?, root)?;
        return Ok(());
    }
    if command == "hold-migration" {
        // Test harness only: pause the same migration SQL inside an uncommitted
        // SQLite transaction. No pause hook or compatibility flow in the app.
        std::fs::create_dir(root)?;
        let mut db = rusqlite::Connection::open(PathBuf::from(root).join("memivy.db"))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        let tx = db.transaction()?;
        tx.execute_batch(include_str!("../../../migrations/memory/001_records.sql"))?;
        tx.pragma_update(None, "application_id", 0x4d495659_i64)?;
        tx.execute_batch(include_str!(
            "../../../migrations/memory/002_conversations.sql"
        ))?;
        println!("migration transaction open; schema 2 not committed");
        std::io::stdout().flush()?;
        loop {
            std::thread::park();
        }
    }
    let store = MemoryStore::open(PathBuf::from(root))?;
    match command.as_str() {
        "edit-current" => {
            let memory_id = args.get(3).ok_or(DataError::Invalid)?.clone();
            let detail = store.library_detail(&RecordKey {
                kind: "memory".into(),
                id: memory_id.clone(),
            })?;
            let current = detail.current.ok_or(DataError::Unavailable)?;
            let receipt = store.edit_memory(&EditRequest {
                request_id: Uuid::new_v4().to_string(),
                memory_id,
                expected_version: current.id,
                title: current.title,
                body: args.get(4).ok_or(DataError::Invalid)?.clone(),
            })?;
            println!(
                "{}",
                serde_json::to_string(&receipt).map_err(|_| DataError::Invalid)?
            );
        }
        "hold-turn" => {
            let topic = Uuid::new_v4().to_string();
            store.create_conversation(&topic, "生成中退出的合成话题")?;
            let turn = store.begin_agent_input(
                &Uuid::new_v4().to_string(),
                &Uuid::new_v4().to_string(),
                &topic,
                "普通问题不能变成记忆",
                &[],
                None,
            )?;
            println!("{}", turn.input_id);
            std::io::stdout().flush()?;
            loop {
                std::thread::park();
            }
        }
        "recover-turn" => {
            store.recover_interrupted_turns()?;
        }
        "capture" | "hold" | "hold-memory" => {
            let request = CaptureRequest {
                request_id: args.get(3).ok_or(DataError::Invalid)?.clone(),
                text: args.get(4).ok_or(DataError::Invalid)?.clone(),
                origin: Origin::User {
                    app: "Phase 2 process test".into(),
                    project: None,
                    uri: None,
                },
            };
            let capture = store.capture(&request)?;
            if command == "hold-memory" {
                let r = store.edit_memory(&EditRequest {
                    request_id: Uuid::new_v4().to_string(),
                    memory_id: capture.memory_id,
                    expected_version: capture.version_id,
                    title: "强杀前版本".into(),
                    body: request.text,
                })?;
                println!(
                    "{}",
                    serde_json::to_string(&r).map_err(|_| DataError::Invalid)?
                );
            } else {
                println!(
                    "{}",
                    serde_json::to_string(&capture).map_err(|_| DataError::Invalid)?
                );
            }
            std::io::stdout().flush()?;
            if command != "capture" {
                loop {
                    std::thread::park();
                }
            }
        }
        "hold-organization" | "finish-organization" => {
            store.recover_organization()?;
            let task = store.claim_organization()?.ok_or(DataError::Unavailable)?;
            if command == "hold-organization" {
                println!("{}", task.attempt_id);
                std::io::stdout().flush()?;
                loop {
                    std::thread::park();
                }
            }
            let r = store.apply_organization(
                &task,
                &MemoryWriteArgs {
                    destination: Destination::New,
                    title: "恢复后整理".into(),
                    parts: vec![MemoryWritePart {
                        text: task.memory.body.clone(),
                        sources: vec![MemorySourceQuote {
                            source_id: task.capture_id.clone(),
                            quote: task.memory.body.trim().chars().take(512).collect(),
                        }],
                    }],
                },
            )?;
            println!(
                "{}",
                serde_json::to_string(&r).map_err(|_| DataError::Invalid)?
            );
        }
        "prepare-restore" => {
            println!(
                "{}",
                serde_json::to_string(&store.prepare_restore(std::path::Path::new(
                    args.get(3).ok_or(DataError::Invalid)?
                ))?)
                .map_err(|_| DataError::Invalid)?
            )
        }
        "arm-restore" => store.arm_restore(args.get(3).ok_or(DataError::Invalid)?)?,
        "backup" => store.backup(PathBuf::from(args.get(3).ok_or(DataError::Invalid)?))?,
        "diagnostics" => {
            store.check_integrity()?;
            // Read-only diagnostics for the harness, never a product write path.
            let db = rusqlite::Connection::open(store.database_path())?;
            let version: String = db.query_row("SELECT sqlite_version()", [], |r| r.get(0))?;
            let count: i64 = db.query_row("SELECT count(*) FROM captures", [], |r| r.get(0))?;
            let versions: i64 =
                db.query_row("SELECT count(*) FROM memory_versions", [], |r| r.get(0))?;
            println!(
                "{}",
                serde_json::json!({"sqlite_version":version,"captures":count,"versions":versions,"integrity":"ok"})
            );
        }
        _ => return Err(DataError::Invalid),
    }
    Ok(())
}
