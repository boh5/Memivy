//! Fixed synthetic Q&A corpus. Semantic correctness is reviewed, never inferred from JSON.
use memivy_core::{memory::*, model::ModelConfig};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::PathBuf,
};
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
const FIXTURE: &str = include_str!("../tests/fixtures/core_discussion.json");
#[derive(Deserialize)]
struct Fixture {
    version: u32,
    seeds: Vec<Seed>,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Seed {
    id: String,
    title: String,
    raw: String,
    body: String,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    seeds: Vec<String>,
    topic: String,
    question: String,
    keywords: Vec<String>,
    expected_sources: Vec<String>,
    rubric: String,
    #[serde(default)]
    reopen: bool,
}
fn counts(s: &MemoryStore) -> Vec<i64> {
    let db = rusqlite::Connection::open(s.database_path()).unwrap();
    ["captures", "memories", "memory_versions", "receipts"]
        .iter()
        .map(|t| {
            db.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
                .unwrap()
        })
        .collect()
}

// Read-only inspection of this probe's synthetic database. Keep uncited evidence
// too, so a retrieval miss can be distinguished from an answer omitting a source.
fn retrieved_evidence(
    s: &MemoryStore,
    message: &str,
    mapping: &HashMap<String, String>,
) -> Vec<serde_json::Value> {
    let db = rusqlite::Connection::open(s.database_path()).unwrap();
    db.prepare("SELECT c.kind,c.source_id,c.cited,c.excerpt_start,c.excerpt_length,CASE c.kind WHEN 'capture' THEN r.text ELSE v.body END FROM message_citations c LEFT JOIN captures r ON c.kind='capture' AND r.id=c.source_id LEFT JOIN memory_versions v ON c.kind='version' AND v.id=c.source_id WHERE c.message_id=? ORDER BY c.kind,c.source_id")
        .unwrap().query_map([message], |r| {
            let kind: String = r.get(0)?;
            let source: String = r.get(1)?;
            let cited: bool = r.get(2)?;
            let start = usize::try_from(r.get::<_, i64>(3)?).unwrap();
            let length = usize::try_from(r.get::<_, i64>(4)?).unwrap();
            let text: String = r.get(5)?;
            Ok(json!({"kind":kind,"source_id":source,"seed_id":mapping.get(&source),"cited":cited,"start":start,"length":length,"text":text.chars().skip(start).take(length).collect::<String>()}))
        }).unwrap().collect::<rusqlite::Result<_>>().unwrap()
}
#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        3,
        "discussion_probe PRIVATE_CONFIG FRESH_OUTPUT_DIR"
    );
    let config =
        ModelConfig::read(std::path::Path::new(&args[1])).expect("private model configuration");
    let output = PathBuf::from(&args[2]);
    fs::create_dir(&output).expect("output must not exist");
    fs::write(output.join("cases.json"), FIXTURE).unwrap();
    let code = concat!(
        include_str!("../src/memory/discussion.rs"),
        include_str!("../src/memory/retrieval.rs"),
        include_str!("../src/memory/library.rs"),
        include_str!("../src/memory/conversations.rs"),
        include_str!("../src/model.rs")
    );
    fs::write(output.join("manifest.json"),serde_json::to_vec_pretty(&json!({"model":config.model,"disable_reasoning":config.disable_reasoning,"endpoint_sha256":format!("{:x}",Sha256::digest(config.base_url.as_bytes())),"fixture_sha256":format!("{:x}",Sha256::digest(FIXTURE.as_bytes())),"implementation_sha256":format!("{:x}",Sha256::digest(code.as_bytes())),"semantic_review":"pending"})).unwrap()).unwrap();
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(fixture.version, 1);
    let root = tempfile::tempdir().unwrap();
    let mut topics: HashMap<String, (String, HashMap<String, String>)> = HashMap::new();
    let mut complete = 0;
    let mut structural = 0;
    let mut review = vec![];
    for case in &fixture.cases {
        let data = root.path().join(&case.topic);
        let mut s = MemoryStore::open(&data).unwrap();
        let (topic, mapping) = topics.entry(case.topic.clone()).or_insert_with(|| {
            let mut mapping = HashMap::new();
            for key in &case.seeds {
                let row = fixture.seeds.iter().find(|r| &r.id == key).unwrap();
                let raw = s
                    .capture(&CaptureRequest {
                        request_id: id(),
                        text: row.raw.clone(),
                        origin: Origin::User {
                            app: "Synthetic Q&A".into(),
                            project: None,
                            uri: None,
                        },
                    })
                    .unwrap();
                let first = s
                    .apply_capture(&ChangeRequest {
                        request_id: id(),
                        capture_id: raw.id.clone(),
                        destination: Destination::New,
                        title: row.title.clone(),
                        body: row.raw.clone(),
                        actor: Actor::User,
                    })
                    .unwrap();
                let memory = first.memory_id.unwrap();
                mapping.insert(memory.clone(), key.clone());
                mapping.insert(raw.id, key.clone());
                mapping.insert(first.after_version.clone().unwrap(), key.clone());
                if row.body != row.raw {
                    let edit = s
                        .edit_memory(&EditRequest {
                            request_id: id(),
                            memory_id: memory,
                            expected_version: first.after_version.unwrap(),
                            title: row.title.clone(),
                            body: row.body.clone(),
                        })
                        .unwrap();
                    mapping.insert(edit.after_version.unwrap(), key.clone());
                }
            }
            let topic = id();
            s.create_conversation(&topic, "固定合成讨论").unwrap();
            (topic, mapping)
        });
        let before = counts(&s);
        if case.reopen {
            let warm = s.start_turn(&id(), topic, "接着讨论木桥项目", &[]).unwrap();
            s.cancel_turn(&warm.id).unwrap();
            drop(s);
            s = MemoryStore::open(&data).unwrap();
        }
        let keyword_hits:Vec<_>=case.keywords.iter().map(|q|{let p=s.library(&LibraryQuery{query:q.clone(),limit:8,..Default::default()}).unwrap();json!({"query":q,"records":p.items.iter().filter_map(|r|mapping.get(&r.key.id)).collect::<Vec<_>>()})}).collect();
        let turn = s.start_turn(&id(), topic, &case.question, &[]).unwrap();
        let started = std::time::Instant::now();
        let result = s.answer_discussion(&config, topic, &turn, &[]).await;
        if let Err(f) = &result {
            s.fail_turn(&turn.id, *f).unwrap();
        }
        let message = s.turn(&turn.id).unwrap().assistant;
        let excerpts: Vec<_> = message
            .citations
            .iter()
            .map(|c| s.discussion_excerpt(&message.id, &c.source).unwrap())
            .collect();
        let cited: BTreeSet<_> = message
            .citations
            .iter()
            .filter_map(|c| {
                let key = match &c.source {
                    SourceRef::Capture(k) | SourceRef::Version(k) => k,
                };
                mapping.get(key).cloned()
            })
            .collect();
        let expected: BTreeSet<_> = case.expected_sources.iter().cloned().collect();
        let retrieved = retrieved_evidence(&s, &message.id, mapping);
        let no_writes = counts(&s) == before;
        assert!(no_writes, "{} unexpected memory write", case.id);
        let completed = message.status == "complete";
        complete += usize::from(completed);
        let source_coverage = if expected.is_empty() {
            cited.is_empty()
        } else {
            expected.is_subset(&cited)
        };
        let structural_pass = completed && source_coverage && no_writes;
        structural += usize::from(structural_pass);
        let artifact = json!({"id":case.id,"question":case.question,"rubric":case.rubric,"result":result.map_err(|e|format!("{e:?}")),"message":message,"evidence":excerpts,"retrieved_evidence":retrieved,"cited_seed_ids":cited,"expected_seed_ids":expected,"keyword_control":keyword_hits,"no_memory_writes":no_writes,"source_coverage":source_coverage,"structural_pass":structural_pass,"elapsed_ms":started.elapsed().as_millis(),"semantic_review":"pending"});
        fs::write(
            output.join(format!("{}.json", case.id)),
            serde_json::to_vec_pretty(&artifact).unwrap(),
        )
        .unwrap();
        review.push(json!({"id":case.id,"rubric":case.rubric,"retrieval":"pending","answer_support":"pending","uncertainty":"pending","notes":"","reviewer":""}));
        println!(
            "{} complete={completed} source_coverage={source_coverage} no_memory_writes={no_writes}",
            case.id
        );
    }
    fs::write(
        output.join("review.json"),
        serde_json::to_vec_pretty(&review).unwrap(),
    )
    .unwrap();
    fs::write(output.join("summary.json"),serde_json::to_vec_pretty(&json!({"cases":fixture.cases.len(),"complete":complete,"structural_pass":structural,"semantic_review":"pending","note":"Source ID coverage is not semantic correctness. Review each claim against its frozen excerpt; record retrieval misses separately."})).unwrap()).unwrap();
    if complete != fixture.cases.len() || structural != fixture.cases.len() {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retrieval_trace_keeps_bound_but_uncited_sources() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let raw = store
            .capture(&CaptureRequest {
                request_id: id(),
                text: "模型看到了但没有引用的依据".into(),
                origin: Origin::User {
                    app: "fixture".into(),
                    project: None,
                    uri: None,
                },
            })
            .unwrap();
        let topic = id();
        store.create_conversation(&topic, "trace").unwrap();
        let source = SourceRef::Capture(raw.id.clone());
        let turn = store
            .start_turn(&id(), &topic, "问题", std::slice::from_ref(&source))
            .unwrap();
        store
            .bind_discussion_evidence(&turn.id, std::slice::from_ref(&source))
            .unwrap();
        let mapping = HashMap::from([(raw.id, "seed".into())]);
        let bound = retrieved_evidence(&store, &turn.assistant.id, &mapping);
        assert_eq!(bound.len(), 1);
        assert_eq!(bound[0]["cited"], false);
        assert_eq!(bound[0]["text"], "模型看到了但没有引用的依据");
        store.finish_turn(&turn.id, "答案", &[source]).unwrap();
        assert_eq!(
            retrieved_evidence(&store, &turn.assistant.id, &mapping)[0]["cited"],
            true
        );
    }
}
