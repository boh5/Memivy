//! Forward-only migrations. The caller owns the exclusive library gate.
use super::{DataError, Result, db::*, transfer::publish_database};
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};
use std::path::Path;

// Append include_str! entries for schema 2 onward. Never edit a shipped migration.
// Scripts contain only transactional SQL; this module owns version and commit.
pub(super) const MIGRATIONS: &[&str] = &[];
pub(super) const CURRENT: i64 = 1 + MIGRATIONS.len() as i64;

pub(super) fn supported_version(db: &Connection, target: i64) -> Result<i64> {
    let app: i64 = db.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if app != APPLICATION_ID || !(1..=target).contains(&version) {
        return Err(DataError::Schema);
    }
    Ok(version)
}

pub(super) fn apply(tx: &Transaction<'_>, from: i64, scripts: &[&str]) -> Result<()> {
    for (index, sql) in scripts.iter().enumerate().skip((from - 1) as usize) {
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", index as i64 + 2)?;
    }
    check(tx)
}

/// A private restore copy needs no additional backup: its source remains intact.
pub(super) fn upgrade(
    db: &mut Connection,
    backup_root: Option<&Path>,
    scripts: &[&str],
) -> Result<()> {
    let target = 1 + scripts.len() as i64;
    let from = supported_version(db, target)?;
    if from == target {
        return Ok(());
    }
    let source_path = db.path().ok_or(DataError::Invalid)?.to_owned();
    // Block other writers before taking the backup; use a separate read connection
    // because SQLite backup cannot read from its own active write transaction.
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let from = supported_version(&tx, target)?;
    if from == target {
        return Ok(());
    }
    check(&tx)?;
    if let Some(root) = backup_root {
        let recovery = root.join("recovery");
        private_dir(&recovery)?;
        let source = Connection::open_with_flags(source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        publish_database(
            &source,
            &recovery.join(format!("before-migration-{from}-to-{target}-{}.db", id())),
        )?;
    }
    apply(&tx, from, scripts)?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const STEPS: &[&str] = &[
        "CREATE TABLE migration_probe(id INTEGER PRIMARY KEY, text TEXT NOT NULL); INSERT INTO migration_probe VALUES (1, '保留原话');",
        "ALTER TABLE migration_probe ADD COLUMN reviewed INTEGER NOT NULL DEFAULT 0; UPDATE migration_probe SET reviewed=1;",
    ];

    fn setup() -> (tempfile::TempDir, MemoryStore, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open_application(dir.path()).unwrap();
        let db = connect(&store.database_path(), false).unwrap();
        (dir, store, db)
    }

    #[test]
    fn upgrades_all_steps_once_and_preserves_a_valid_private_backup() {
        let (dir, store, mut db) = setup();
        fs::write(dir.path().join("models.json"), "private configuration").unwrap();
        upgrade(&mut db, Some(dir.path()), STEPS).unwrap();
        assert_eq!(supported_version(&db, 3).unwrap(), 3);
        assert_eq!(
            db.query_row("SELECT text, reviewed FROM migration_probe", [], |r| Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?
            )))
            .unwrap(),
            ("保留原话".into(), 1)
        );
        upgrade(&mut db, Some(dir.path()), STEPS).unwrap();
        let backups: Vec<_> = fs::read_dir(dir.path().join("recovery"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(backups.len(), 1);
        let backup = Connection::open(&backups[0]).unwrap();
        assert_eq!(supported_version(&backup, 3).unwrap(), 1);
        check(&backup).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&backups[0]).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("models.json")).unwrap(),
            "private configuration"
        );
        assert_eq!(
            store.check_integrity(),
            Err(DataError::Schema),
            "Old clients must reject a migrated library"
        );
    }

    #[test]
    fn failed_later_step_rolls_back_every_step_and_allows_retry() {
        let (dir, _, mut db) = setup();
        assert!(
            upgrade(
                &mut db,
                Some(dir.path()),
                &[STEPS[0], "INSERT INTO missing_table VALUES(1);"]
            )
            .is_err()
        );
        assert_eq!(supported_version(&db, 3).unwrap(), 1);
        assert!(
            !db.prepare("SELECT 1 FROM sqlite_master WHERE name='migration_probe'")
                .unwrap()
                .exists([])
                .unwrap()
        );
        check(&db).unwrap();
        upgrade(&mut db, Some(dir.path()), STEPS).unwrap();
        assert_eq!(supported_version(&db, 3).unwrap(), 3);
    }

    #[test]
    fn backup_failure_prevents_any_migration() {
        let (dir, _, mut db) = setup();
        fs::write(dir.path().join("recovery"), "not a directory").unwrap();
        assert!(upgrade(&mut db, Some(dir.path()), STEPS).is_err());
        assert_eq!(supported_version(&db, 3).unwrap(), 1);
        check(&db).unwrap();
    }

    #[test]
    fn current_schema_needs_no_backup_and_future_or_foreign_schema_is_rejected() {
        let (dir, _, mut db) = setup();
        upgrade(&mut db, Some(dir.path()), &[]).unwrap();
        assert!(!dir.path().join("recovery").exists());
        db.pragma_update(None, "user_version", 99).unwrap();
        assert_eq!(
            upgrade(&mut db, Some(dir.path()), STEPS),
            Err(DataError::Schema)
        );
        db.pragma_update(None, "user_version", 1).unwrap();
        db.pragma_update(None, "application_id", 123).unwrap();
        assert_eq!(
            upgrade(&mut db, Some(dir.path()), STEPS),
            Err(DataError::Schema)
        );
        assert!(!dir.path().join("recovery").exists());
    }

    #[test]
    fn existing_intermediate_schema_runs_only_missing_steps() {
        let (dir, _, mut db) = setup();
        upgrade(&mut db, Some(dir.path()), &STEPS[..1]).unwrap();
        upgrade(&mut db, Some(dir.path()), STEPS).unwrap();
        assert_eq!(supported_version(&db, 3).unwrap(), 3);
    }
    #[test]
    fn deferred_foreign_key_failure_rolls_back_schema_and_data() {
        let (dir, _, mut db) = setup();
        let sql = "CREATE TABLE migration_parent(id INTEGER PRIMARY KEY); CREATE TABLE migration_child(parent INTEGER REFERENCES migration_parent(id) DEFERRABLE INITIALLY DEFERRED); INSERT INTO migration_child VALUES(7);";
        assert!(upgrade(&mut db, Some(dir.path()), &[sql]).is_err());
        assert_eq!(supported_version(&db, 2).unwrap(), 1);
        assert!(
            !db.prepare("SELECT 1 FROM sqlite_master WHERE name='migration_child'")
                .unwrap()
                .exists([])
                .unwrap()
        );
        check(&db).unwrap();
    }

    #[test]
    fn interrupted_migration_child() {
        let Some(root) = std::env::var_os("MEMIVY_MIGRATION_CRASH_TEST") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let mut db = connect(&root.join("memivy.db"), false).unwrap();
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        apply(&tx, 1, STEPS).unwrap();
        fs::write(root.join("transaction-ready"), "ready").unwrap();
        // The parent kills this process while schema and version are uncommitted.
        std::thread::sleep(std::time::Duration::from_secs(30));
        panic!("Parent did not interrupt the migration transaction");
    }

    #[test]
    fn interrupted_transaction_recovers_on_the_next_migration() {
        let (dir, _, db) = setup();
        drop(db);
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "memory::migrations::tests::interrupted_migration_child",
                "--nocapture",
            ])
            .env("MEMIVY_MIGRATION_CRASH_TEST", dir.path())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let start = std::time::Instant::now();
        while !dir.path().join("transaction-ready").exists() {
            if start.elapsed() > std::time::Duration::from_secs(10) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("Child did not reach the migration boundary");
            }
            assert!(child.try_wait().unwrap().is_none());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child.kill().unwrap();
        child.wait().unwrap();
        let mut db = connect(&dir.path().join("memivy.db"), false).unwrap();
        assert_eq!(supported_version(&db, 3).unwrap(), 1);
        check(&db).unwrap();
        upgrade(&mut db, Some(dir.path()), STEPS).unwrap();
        assert_eq!(supported_version(&db, 3).unwrap(), 3);
    }
}
