//! Synthetic scan benchmark; duplicated real vectors measure storage/query cost,
//! not semantic quality. Run only on an isolated evaluation database.
use memivy_core::{
    embedding::{self as emb, Preferences},
    memory::*,
};
use rusqlite::{Connection, params};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("isolated evaluation root"));
    assert!(root.is_absolute() && root.starts_with("/private/tmp/"));
    let store = MemoryStore::open(&root).unwrap();
    Preferences {
        enabled: true,
        ..Default::default()
    }
    .save(&root)
    .unwrap();
    emb::client::warmup(&root).unwrap();
    let start = Instant::now();
    while !emb::client::ready(&root) {
        assert!(start.elapsed() < Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut db = Connection::open(store.database_path()).unwrap();
    let vectors: Vec<Vec<u8>> = db
        .prepare("SELECT vector FROM embedding_chunks LIMIT 5")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(vectors.len(), 5);
    let count: i64 = db
        .query_row("SELECT count(*) FROM memories", [], |r| r.get(0))
        .unwrap();
    let tx = db.transaction().unwrap();
    for n in count..10_000 {
        let m = id();
        let v = id();
        let body = format!(
            "扫描合成条目 {n}：This is a synthetic scan fixture, not a semantic-quality example. Budget 400 yuan; unconfirmed."
        );
        tx.execute(
            "INSERT INTO memories(id,state,created_at,updated_at) VALUES(?1,'active',?2,?2)",
            params![m, n],
        )
        .unwrap();
        tx.execute("INSERT INTO memory_versions(id,memory_id,title,body,actor,reason,created_at) VALUES(?1,?2,?3,?3,'user','create',?4)",params![v,m,body,n]).unwrap();
        tx.execute(
            "UPDATE memories SET current_version_id=?1 WHERE id=?2",
            params![v, m],
        )
        .unwrap();
        for (j, vector) in vectors.iter().enumerate() {
            tx.execute(
                "INSERT INTO embedding_chunks VALUES(?1,?2,?3,0,100,'scan-fixture',?4)",
                params![m, v, j as i64, vector],
            )
            .unwrap();
        }
    }
    tx.commit().unwrap();
    for target in [50_000, 100_000] {
        let tx = db.transaction().unwrap();
        let current: i64 = tx
            .query_row("SELECT count(*) FROM embedding_chunks", [], |r| r.get(0))
            .unwrap();
        if current < target {
            tx.execute("INSERT INTO embedding_chunks SELECT memory_id,version_id,ordinal+?2,start_char,end_char,input_hash,vector FROM embedding_chunks LIMIT ?1",params![target-current,target]).unwrap();
        }
        tx.commit().unwrap();
        let count: i64 = db
            .query_row("SELECT count(*) FROM embedding_chunks", [], |r| r.get(0))
            .unwrap();
        let mut times = vec![];
        let mut degraded = 0;
        for _ in 0..25 {
            let now = Instant::now();
            let result = store
                .search(&SearchRequest::text(
                    "Where is my offline backup stored?",
                    8,
                ))
                .unwrap();
            if result.mode != "hybrid" {
                degraded += 1;
            }
            times.push(now.elapsed().as_secs_f64() * 1000.);
        }
        times.sort_by(f64::total_cmp);
        println!(
            "memories=10000 chunks={count} warm_search_p50_ms={:.1} p95_ms={:.1} degraded={degraded}/25",
            times[12], times[23]
        );
        assert_eq!(degraded, 0);
    }
}
