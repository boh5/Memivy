//! Current-memory evidence for Q&A and ingestion. No mutation tools or archives.
use super::{db::*, records::*, *};
use crate::model::{self, FunctionCall, ProbeError, tools};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone)]
pub(super) struct SeenMemory {
    pub memory: String,
    pub source: SourceRef,
}
#[derive(Clone)]
pub(super) struct AgentEvidence {
    pub store: MemoryStore,
    pub scope: SearchScope,
    pub known: Vec<SeenMemory>,
    pub spans: Vec<Evidence>,
    pub turn: Option<String>,
    pub input: Option<(String, String)>,
    pub char_budget: usize,
    attempted: std::collections::HashSet<String>,
}
impl AgentEvidence {
    pub fn new(store: MemoryStore, scope: SearchScope, turn: Option<String>) -> Self {
        Self {
            store,
            scope,
            known: vec![],
            spans: vec![],
            turn,
            input: None,
            char_budget: 12_000,
            attempted: Default::default(),
        }
    }
    fn active(&self) -> Result<()> {
        if let Some(turn) = &self.turn
            && self.store.turn(turn)?.assistant.status != "processing"
        {
            return Err(DataError::Conflict);
        }
        if let Some((memory, version)) = &self.input
            && self.store.memory(memory)?.current.id != *version
        {
            return Err(DataError::Conflict);
        }
        if let Some(collection) = &self.scope.collection_id {
            super::navigation::active_collection(&self.store.connection()?, collection)?;
        }
        Ok(())
    }
    pub fn seed(&mut self, memory: String, evidence: Evidence) -> Result<Option<Value>> {
        self.active()?;
        let db = self.store.connection()?;
        if !super::search::current_source(&db, &evidence.source)? {
            return Err(DataError::Unavailable);
        }
        if let Some(collection) = &self.scope.collection_id
            && !super::navigation::source_in_collection(&db, collection, &evidence.source)?
        {
            return Err(DataError::Unavailable);
        }
        let is_new = !self.known.iter().any(|s| s.memory == memory);
        let index = if let Some(index) = self.known.iter().position(|s| s.memory == memory) {
            if self.known[index].source != evidence.source {
                return Err(DataError::Unavailable);
            }
            index
        } else {
            if self.known.len() == 8 {
                return Ok(None);
            }
            self.known.len()
        };
        let mut ranges: Vec<_> = self
            .spans
            .iter()
            .filter(|e| e.source == evidence.source)
            .map(|e| (e.start, e.start + e.text.chars().count()))
            .collect();
        ranges.push((
            evidence.start,
            evidence.start + evidence.text.chars().count(),
        ));
        ranges.sort_unstable();
        let mut merged: Vec<(usize, usize)> = vec![];
        for (start, end) in ranges {
            if let Some(last) = merged.last_mut()
                && start <= last.1
            {
                last.1 = last.1.max(end);
                continue;
            }
            merged.push((start, end));
        }
        let old = self
            .spans
            .iter()
            .filter(|e| e.source != evidence.source)
            .map(|e| e.text.chars().count())
            .sum::<usize>();
        if old + merged.iter().map(|(a, b)| b - a).sum::<usize>() > self.char_budget {
            return Ok(None);
        }
        let mut rebuilt = vec![];
        for (start, end) in merged {
            rebuilt.push(resolve_excerpt(
                &db,
                &evidence.source,
                end - start,
                &[],
                Some(start),
            )?);
        }
        let bytes = self
            .spans
            .iter()
            .filter(|e| e.source != evidence.source)
            .map(|e| e.text.len())
            .sum::<usize>()
            + rebuilt.iter().map(|e| e.text.len()).sum::<usize>();
        if bytes > 48 * 1024 {
            return Ok(None);
        }
        if is_new {
            self.known.push(SeenMemory {
                memory,
                source: evidence.source.clone(),
            });
        }
        self.spans.retain(|e| e.source != evidence.source);
        self.spans.extend(rebuilt);
        let next = evidence.start + evidence.text.chars().count();
        let version = self.store.memory(&self.known[index].memory)?.current;
        if SourceRef::Version(version.id) != evidence.source {
            return Err(DataError::Unavailable);
        }
        Ok(Some(
            json!({"id":format!("M{}",index+1),"memory_id":self.known[index].memory,"source":evidence.source,"title":evidence.title,"text":evidence.text,"start_char":evidence.start,"next_start":if next<version.body.chars().count(){Some(next)}else{None},"truncated":evidence.truncated,"recorded_at_ms":evidence.recorded_at}),
        ))
    }
    pub fn source(&self, label: &str) -> Result<&SeenMemory> {
        let index = label
            .strip_prefix('M')
            .and_then(|s| s.parse::<usize>().ok())
            .and_then(|i| i.checked_sub(1))
            .ok_or(DataError::Invalid)?;
        self.known.get(index).ok_or(DataError::Invalid)
    }
    pub fn validate(&self) -> Result<()> {
        self.active()?;
        let db = self.store.connection()?;
        for e in &self.spans {
            if !super::search::current_source(&db, &e.source)? {
                return Err(DataError::Unavailable);
            }
            if let Some(collection) = &self.scope.collection_id
                && !super::navigation::source_in_collection(&db, collection, &e.source)?
            {
                return Err(DataError::Unavailable);
            }
        }
        Ok(())
    }
    fn execute(&mut self, call: FunctionCall) -> Result<(Value, bool)> {
        self.active()?;
        let new_search = call.name == "search_memories"
            && self
                .attempted
                .insert(serde_json::to_string(&call.arguments).map_err(|_| DataError::Invalid)?);
        let mut before: Vec<_> = self
            .spans
            .iter()
            .map(|e| {
                (
                    e.source.parts().1.to_owned(),
                    e.start,
                    e.text.chars().count(),
                )
            })
            .collect();
        before.sort();
        let result = match call.name.as_str() {
            "search_memories" => {
                let args: SearchArgs =
                    serde_json::from_value(call.arguments).map_err(|_| DataError::Invalid)?;
                if args.limit == 0 || args.limit > 8 {
                    return Err(DataError::Invalid);
                }
                let found = self.store.search(&SearchRequest {
                    query: args.query,
                    variants: args.variants,
                    scope: self.scope.clone(),
                    limit: args.limit,
                    excerpt_chars: 1200,
                    ..Default::default()
                })?;
                let mut items = vec![];
                let mut budget = false;
                for hit in found.items {
                    match self.seed(hit.memory_id, hit.evidence)? {
                        Some(v) => items.push(v),
                        None => {
                            budget = true;
                            break;
                        }
                    }
                }
                json!({"items":items,"truncated":found.truncated||found.has_more||budget,"budget_reached":budget,"mode":found.mode,"degraded_reason":found.degraded_reason})
            }
            "read_memory" => {
                let args: ReadArgs =
                    serde_json::from_value(call.arguments).map_err(|_| DataError::Invalid)?;
                if args.max_chars == 0 || args.max_chars > 3000 {
                    return Err(DataError::Invalid);
                }
                let seen = self.source(&args.id)?;
                let memory = self.store.memory(&seen.memory)?;
                if seen.source != SourceRef::Version(memory.current.id) {
                    return Err(DataError::Unavailable);
                }
                if args.start_char >= memory.current.body.chars().count() {
                    return Ok((json!({"end_of_memory":true}), false));
                }
                let e = resolve_excerpt(
                    &self.store.connection()?,
                    &seen.source,
                    args.max_chars,
                    &[],
                    Some(args.start_char),
                )?;
                self.seed(seen.memory.clone(), e)?
                    .unwrap_or_else(|| json!({"budget_reached":true}))
            }
            _ => return Err(DataError::Invalid),
        };
        let mut after: Vec<_> = self
            .spans
            .iter()
            .map(|e| {
                (
                    e.source.parts().1.to_owned(),
                    e.start,
                    e.text.chars().count(),
                )
            })
            .collect();
        after.sort();
        Ok((result, before != after || new_search))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    query: String,
    #[serde(default)]
    variants: Vec<String>,
    #[serde(default = "search_limit")]
    limit: usize,
}
fn search_limit() -> usize {
    4
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    id: String,
    start_char: usize,
    #[serde(default = "read_limit")]
    max_chars: usize,
}
fn read_limit() -> usize {
    1800
}
pub(super) fn evidence_tools() -> Vec<Value> {
    vec![
        tools::function(
            "search_memories",
            "Search current memories within the fixed task scope. query is the natural-language question. Always supply up to four short literal variants, including the exact project/person/topic name in its original language. Each variant is an alternative search; words within one variant must all match. For a mixed-language question about 木桥 use variants [木桥], not a translation or a long sentence. If no results, retry with fewer terms or a different exact entity. An empty result does not prove the memory is absent.",
            json!({"query":{"type":"string"},"variants":{"type":"array","items":{"type":"string"},"maxItems":4},"limit":{"type":"integer","minimum":1,"maximum":8}}),
        ),
        tools::function(
            "read_memory",
            "Read another current-body window of an M reference already supplied. Continue at next_start; archives are unavailable.",
            json!({"id":{"type":"string"},"start_char":{"type":"integer","minimum":0},"max_chars":{"type":"integer","minimum":1,"maximum":3000}}),
        ),
    ]
}
pub(super) async fn handle(
    mut context: AgentEvidence,
    call: FunctionCall,
) -> std::result::Result<(AgentEvidence, Value, bool), ProbeError> {
    tokio::task::spawn_blocking(move || {
        let (value, progress) = context
            .execute(call)
            .map_err(|_| ProbeError::InvalidResponse)?;
        Ok((context, value, progress))
    })
    .await
    .map_err(|_| ProbeError::InvalidResponse)?
}

impl MemoryStore {
    pub fn model_capabilities(&self, config: &model::ModelConfig) -> Option<tools::Capabilities> {
        tools::cached(&self.root, config)
    }
    pub async fn test_model_capabilities(
        &self,
        config: &model::ModelConfig,
    ) -> std::result::Result<tools::Capabilities, ProbeError> {
        tools::probe_and_save(&self.root, config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn capture(s: &MemoryStore, text: &str) -> CaptureResult {
        s.capture(&CaptureRequest {
            request_id: id(),
            text: text.into(),
            origin: Origin::User {
                app: "QA".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap()
    }
    fn search(query: &str) -> FunctionCall {
        FunctionCall {
            name: "search_memories".into(),
            arguments: json!({"query":query,"variants":[],"limit":8}),
        }
    }
    #[test]
    fn scope_is_fixed_and_removed_members_cannot_be_read() {
        let d = tempfile::tempdir().unwrap();
        let s = MemoryStore::open(d.path()).unwrap();
        let inside = capture(&s, "anchor inside");
        capture(&s, "anchor outside");
        let collection = id();
        s.save_collection(&collection, "scope", "", None).unwrap();
        let key = RecordKey {
            kind: "memory".into(),
            id: inside.memory_id.clone(),
        };
        s.collect_record(&collection, &key, true).unwrap();
        let mut context = AgentEvidence::new(
            s.clone(),
            SearchScope {
                collection_id: Some(collection.clone()),
                ..Default::default()
            },
            None,
        );
        let (result, _) = context.execute(search("anchor")).unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 1);
        assert_eq!(context.known[0].memory, inside.memory_id);
        assert!(
            context
                .execute(FunctionCall {
                    name: "read_memory".into(),
                    arguments: json!({"id":"M2","start_char":0,"max_chars":1800})
                })
                .is_err()
        );
        let mut escape = search("anchor");
        escape.arguments["scope"] = json!({});
        assert!(context.execute(escape).is_err());
        s.collect_record(&collection, &key, false).unwrap();
        assert!(
            context
                .execute(FunctionCall {
                    name: "read_memory".into(),
                    arguments: json!({"id":"M1","start_char":0,"max_chars":1800})
                })
                .is_err()
        );
        assert!(context.validate().is_err());
    }
    #[test]
    fn overlap_budget_and_duplicate_reads_count_unique_ranges_not_vector_order() {
        let d = tempfile::tempdir().unwrap();
        let s = MemoryStore::open(d.path()).unwrap();
        capture(&s, &format!("anchor first {}", "x".repeat(4000)));
        capture(&s, "anchor second");
        let mut context = AgentEvidence::new(s, SearchScope::default(), None);
        context.execute(search("anchor")).unwrap();
        for label in ["M1", "M2", "M1"] {
            let (_, progress) = context
                .execute(FunctionCall {
                    name: "read_memory".into(),
                    arguments: json!({"id":label,"start_char":0,"max_chars":1200}),
                })
                .unwrap();
            assert!(!progress);
        }
        let long = context
            .known
            .iter()
            .position(|s| {
                context
                    .spans
                    .iter()
                    .any(|e| e.source == s.source && e.text.len() == 1200)
            })
            .unwrap()
            + 1;
        let (_, progress) = context
            .execute(FunctionCall {
                name: "read_memory".into(),
                arguments: json!({"id":format!("M{long}"),"start_char":1000,"max_chars":1800}),
            })
            .unwrap();
        assert!(progress);
        assert_eq!(
            context
                .spans
                .iter()
                .filter(|e| e.source == context.known[long - 1].source)
                .count(),
            1
        );
        let before = context
            .spans
            .iter()
            .map(|e| e.text.chars().count())
            .sum::<usize>();
        context.char_budget = before;
        let (result, _) = context
            .execute(FunctionCall {
                name: "read_memory".into(),
                arguments: json!({"id":format!("M{long}"),"start_char":2800,"max_chars":1800}),
            })
            .unwrap();
        assert_eq!(result["budget_reached"], true);
        assert_eq!(
            context
                .spans
                .iter()
                .map(|e| e.text.chars().count())
                .sum::<usize>(),
            before
        );
    }
}
