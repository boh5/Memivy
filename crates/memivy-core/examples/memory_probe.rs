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
    let store = MemoryStore::open(PathBuf::from(root))?;
    match command.as_str() {
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
                let r = store.apply_capture(&ChangeRequest {
                    request_id: Uuid::new_v4().to_string(),
                    capture_id: capture.id,
                    destination: Destination::New,
                    title: "强杀前版本".into(),
                    body: request.text,
                    actor: Actor::User,
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
