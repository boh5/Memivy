use memivy_core::memory::*;
use serde::Deserialize;
use std::collections::HashMap;
use uuid::Uuid;

pub const SEARCH: &str = include_str!("../fixtures/core_search.json");
pub fn id() -> String {
    Uuid::new_v4().to_string()
}
#[derive(Deserialize)]
pub struct Corpus {
    pub version: u32,
    pub seeds: Vec<Seed>,
    pub queries: Vec<Query>,
}
#[derive(Deserialize)]
pub struct Seed {
    pub id: String,
    pub raw: String,
    pub title: String,
    pub body: String,
    pub old_body: Option<String>,
    pub origin: Option<String>,
    pub project: Option<String>,
    pub uri: Option<String>,
    #[serde(default)]
    pub trashed: bool,
}
#[derive(Deserialize)]
pub struct Query {
    pub id: String,
    pub query: String,
    pub expected: Vec<String>,
    pub origin: Option<String>,
    pub project: Option<String>,
    pub since_at: Option<String>,
    pub until_at: Option<String>,
    pub raw_hit: Option<String>,
}
pub struct Seeded {
    pub records: HashMap<String, String>,
    pub captures: HashMap<String, String>,
    pub updated: HashMap<String, i64>,
}
pub fn seed(store: &MemoryStore, corpus: &Corpus) -> Seeded {
    let mut result = Seeded {
        records: HashMap::new(),
        captures: HashMap::new(),
        updated: HashMap::new(),
    };
    for row in &corpus.seeds {
        let origin = if row.origin.as_deref() == Some("agent") {
            Origin::Agent {
                app: "Synthetic Agent".into(),
                project: row.project.clone(),
                uri: row.uri.clone(),
            }
        } else {
            Origin::User {
                app: "Synthetic fixture".into(),
                project: row.project.clone(),
                uri: row.uri.clone(),
            }
        };
        let raw = store
            .capture(&CaptureRequest {
                request_id: id(),
                text: row.raw.clone(),
                origin,
            })
            .unwrap();
        let mut receipt = store
            .apply_capture(&ChangeRequest {
                request_id: id(),
                capture_id: raw.id.clone(),
                destination: Destination::New,
                title: row.title.clone(),
                body: row.old_body.as_ref().unwrap_or(&row.body).clone(),
                actor: Actor::User,
            })
            .unwrap();
        if row.old_body.is_some() {
            receipt = store
                .edit_memory(&EditRequest {
                    request_id: id(),
                    memory_id: receipt.memory_id.clone().unwrap(),
                    expected_version: receipt.after_version.unwrap(),
                    title: row.title.clone(),
                    body: row.body.clone(),
                })
                .unwrap();
        }
        let memory = receipt.memory_id.unwrap();
        if row.trashed {
            store
                .trash_memory(&memory, receipt.after_version.as_ref().unwrap())
                .unwrap();
        }
        result.updated.insert(
            row.id.clone(),
            store
                .memory(&memory)
                .map(|m| m.current.created_at)
                .unwrap_or(raw.created_at),
        );
        result.records.insert(row.id.clone(), memory);
        result.captures.insert(row.id.clone(), raw.id);
    }
    store
        .save_workspace_draft(&WorkspaceDraft {
            key: "capture".into(),
            request_id: id(),
            title: String::new(),
            body: "draft_only_sentinel 草稿不是记忆".into(),
            expected_version: None,
            origin: None,
            context: vec![],
        })
        .unwrap();
    let topic = id();
    store.create_conversation(&topic, "固定测试话题").unwrap();
    let turn = store
        .start_turn(
            &id(),
            &topic,
            "question_only_sentinel 假设换个方案呢？",
            &[],
        )
        .unwrap();
    store.cancel_turn(&turn.id).unwrap();
    result
}
