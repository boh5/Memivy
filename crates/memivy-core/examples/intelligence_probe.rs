//! Explicit, synthetic-only real-model evaluation. Never reads the user's library.
use memivy_core::{memory::*, model::ModelConfig};
use serde::Deserialize;
use serde_json::json;
use std::{fs, path::PathBuf};
use uuid::Uuid;
#[derive(Deserialize)]
struct Case {
    id: String,
    expected: String,
    target: String,
    seeds: Vec<(String, String)>,
    capture: String,
}
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn capture(store: &MemoryStore, text: &str) -> CaptureResult {
    store
        .capture(&CaptureRequest {
            request_id: id(),
            text: text.into(),
            origin: Origin::User {
                app: "Synthetic evaluation".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap()
}
#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.len() == 3,
        "intelligence_probe PRIVATE_CONFIG OUTPUT_DIR"
    );
    let config =
        ModelConfig::read(std::path::Path::new(&args[1])).expect("private model configuration");
    let output = PathBuf::from(&args[2]);
    // Never relabel cached results with another model or overwrite an earlier run.
    // Atomic directory creation also excludes concurrent writers.
    create_run_directory(&output).expect("OUTPUT_DIR must be a fresh, non-existent directory");
    let fixture = include_str!("../tests/fixtures/phase5_cases.json");
    fs::write(output.join("cases.json"), fixture).unwrap();
    use sha2::{Digest, Sha256};
    let code = concat!(
        include_str!("../src/memory/organization.rs"),
        include_str!("../src/memory/retrieval.rs"),
        include_str!("../src/memory/library.rs"),
        include_str!("../src/model.rs")
    );
    fs::write(
        output.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "model": config.model, "disable_reasoning": config.disable_reasoning,
            "fixture_sha256": format!("{:x}", Sha256::digest(fixture.as_bytes())),
            "implementation_sha256": format!("{:x}", Sha256::digest(code.as_bytes())),
            "endpoint_sha256": format!("{:x}", Sha256::digest(config.base_url.as_bytes()))
        }))
        .unwrap(),
    )
    .unwrap();
    let cases: Vec<Case> = serde_json::from_str(fixture).unwrap();
    let rubric = include_str!("../tests/fixtures/organization_review.json");
    fs::write(output.join("rubric.json"), rubric).unwrap();
    let mut review = vec![];
    let mut passed = 0;
    for case in &cases {
        let path = output.join(format!("{}.json", case.id));
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        for (title, body) in &case.seeds {
            let c = capture(&store, body);
            store
                .edit_memory(&EditRequest {
                    request_id: id(),
                    memory_id: c.memory_id,
                    expected_version: c.version_id,
                    title: title.clone(),
                    body: body.clone(),
                })
                .unwrap();
        }
        let raw = capture(&store, &case.capture);
        let mut task = store.claim_organization().unwrap().unwrap();
        assert_eq!(task.capture_id, raw.capture_id);
        store.prepare_organization(&mut task).unwrap();
        let started = std::time::Instant::now();
        let result = match store.propose_organization(&config, &task).await {
            Ok(proposal) => {
                let target = proposal
                    .target
                    .strip_prefix('M')
                    .and_then(|s| s.parse::<usize>().ok())
                    .and_then(|i| i.checked_sub(1))
                    .and_then(|i| task.candidates.get(i))
                    .map(|v| v.title.clone())
                    .unwrap_or_default();
                let applied = store.apply_organization(&task, &proposal);
                let raw_preserved =
                    store.capture_by_id(&raw.capture_id).unwrap().text == case.capture;
                let replay_pass = applied.as_ref().is_ok_and(|r| {
                    store
                        .apply_organization(&task, &proposal)
                        .is_ok_and(|replay| &replay == r)
                });
                let final_body = applied
                    .as_ref()
                    .ok()
                    .and_then(|r| r.memory_id.as_ref())
                    .map(|m| store.memory(m).unwrap().current.body);
                let pass = raw_preserved
                    && replay_pass
                    && proposal.action == case.expected
                    && (case.expected != "merge" || target == case.target)
                    && applied.is_ok();
                passed += usize::from(pass);
                json!({"id":case.id,"proposal":proposal,"selected_title":target,"candidates":task.candidates,"routing_pass":pass,"raw_preserved":raw_preserved,"replay_pass":replay_pass,"final_body":final_body,"apply":applied.map_err(|e|e.to_string()),"elapsed_ms":started.elapsed().as_millis()})
            }
            Err(error) => {
                json!({"id":case.id,"routing_pass":false,"error":error.to_string(),"elapsed_ms":started.elapsed().as_millis()})
            }
        };
        review.push(json!({"id":case.id,"meaning_preserved":"pending","uncertainty_preserved":"pending","unaffected_text_preserved":"pending","notes":"","reviewer":""}));
        fs::write(path, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
        println!(
            "{} routing_pass={} elapsed_ms={}",
            case.id, result["routing_pass"], result["elapsed_ms"]
        );
    }
    fs::write(output.join("summary.json"),serde_json::to_vec_pretty(&json!({"model":config.model,"cases":cases.len(),"routing_pass":passed,"note":"Routing and core validation only; inspect prose separately for meaning and uncertainty."})).unwrap()).unwrap();
    fs::write(
        output.join("review.json"),
        serde_json::to_vec_pretty(&review).unwrap(),
    )
    .unwrap();
    println!("Routing/core validation: {passed}/{}", cases.len());
    if passed != cases.len() {
        std::process::exit(1);
    }
}

fn create_run_directory(output: &std::path::Path) -> std::io::Result<()> {
    fs::create_dir(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_results_are_rejected_without_relabeling_or_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("run");
        create_run_directory(&output).unwrap();
        let old = br#"{"model":"original-model","routing_pass":40}"#;
        fs::write(output.join("summary.json"), old).unwrap();
        fs::write(output.join("cases.json"), "original fixtures").unwrap();
        assert_eq!(
            create_run_directory(&output).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(output.join("summary.json")).unwrap(), old);
        assert_eq!(
            fs::read_to_string(output.join("cases.json")).unwrap(),
            "original fixtures"
        );
    }
}
