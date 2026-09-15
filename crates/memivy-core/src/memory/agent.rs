//! Memory tools shared by discussion and explicit-capture maintenance.
//! This module reads facts; the owning execution applies writes transactionally.
use super::protocol::{self, Checkpoint};
use super::{db::*, records::*, *};
use crate::model::{self, ProbeError, tools};
use crate::model::{Message, ToolDefinition};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryWriteArgs {
    pub destination: Destination,
    pub title: String,
    pub parts: Vec<MemoryWritePart>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryWritePart {
    pub text: String,
    pub sources: Vec<MemorySourceQuote>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemorySourceQuote {
    pub source_id: String,
    pub quote: String,
}

/// Validate item-level attribution inside the owner's transaction. The owner
/// resolves its allowed raw sources; references to the target version inherit
/// that version's existing captures without archiving an AI-written body.
pub(super) fn resolve_memory_write(
    write: &MemoryWriteArgs,
    previous: Option<&Version>,
    mut source_text: impl FnMut(&str) -> Result<String>,
) -> Result<(String, Vec<String>)> {
    valid_text(&write.title, 200)?;
    if write.parts.is_empty() || write.parts.len() > 64 {
        return Err(DataError::Invalid);
    }
    let mut body = String::new();
    let mut sources = vec![];
    let mut inherited_through = 0;
    for part in &write.parts {
        if part.sources.len() > 16 || part.text.len() > 128 * 1024 - body.len() {
            return Err(DataError::Invalid);
        }
        body.push_str(&part.text);
        let text = part.text.trim();
        if part.sources.is_empty() {
            if text.is_empty() {
                continue;
            }
            let old = &previous.ok_or(DataError::SourceAttribution)?.body;
            let matched = old[inherited_through..]
                .match_indices(text)
                .map(|(start, _)| inherited_through + start)
                .find(|&start| {
                    let end = start + text.len();
                    let line_start = old[..start].rfind('\n').map_or(0, |n| n + 1);
                    let line_end = old[end..].find('\n').map_or(old.len(), |n| end + n);
                    old[line_start..start].trim().is_empty() && old[end..line_end].trim().is_empty()
                });
            inherited_through = matched.ok_or(DataError::SourceAttribution)? + text.len();
            continue;
        }
        // A source cannot be attached only to a separator to satisfy the raw
        // source requirement without supporting any newly written content.
        if text.is_empty() {
            return Err(DataError::SourceAttribution);
        }
        for source in &part.sources {
            valid_id(&source.source_id)?;
            if source.quote.len() > 3000 {
                return Err(DataError::Invalid);
            }
            if source.quote.trim().is_empty() {
                return Err(DataError::SourceAttribution);
            }
            if let Some(old) = previous.filter(|old| old.id == source.source_id) {
                if !old.body.contains(&source.quote) {
                    return Err(DataError::SourceAttribution);
                }
                continue;
            }
            if !source_text(&source.source_id)?.contains(&source.quote) {
                return Err(DataError::SourceAttribution);
            }
            if !sources.contains(&source.source_id) {
                if sources.len() == 16 {
                    return Err(DataError::Invalid);
                }
                sources.push(source.source_id.clone());
            }
        }
    }
    valid_text(&body, 128 * 1024)?;
    if sources.is_empty() {
        return Err(DataError::SourceAttribution);
    }
    Ok((body, sources))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    query: String,
    variants: Vec<String>,
    limit: usize,
    offset: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    collection_id: Option<String>,
    offset: usize,
    limit: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    memory_id: String,
    view: String,
    source_id: Option<String>,
    start_char: usize,
    max_chars: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConversationArgs {
    conversation_id: String,
    after_seq: i64,
    limit: usize,
    manual_saves_offset: Option<usize>,
}

fn args<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone()).map_err(|_| DataError::Invalid)
}
pub(super) fn source_url(source: &SourceRef) -> String {
    let (kind, id) = source.parts();
    format!("memivy://source/{kind}/{id}")
}
pub(super) fn evidence_value(memory_id: &str, evidence: Evidence, total: usize) -> Value {
    let next = evidence.start + evidence.text.chars().count();
    json!({"memory_id":memory_id,"citation_url":source_url(&evidence.source),
        "evidence":evidence,"next_start":(next<total).then_some(next),"total_chars":total})
}

pub(super) fn collect_evidence(messages: &[Message]) -> Vec<Evidence> {
    fn collect(value: &Value, out: &mut Vec<Evidence>) {
        match value {
            Value::Object(map) => {
                if let Some(e) = map.get("evidence")
                    && let Ok(e) = serde_json::from_value::<Evidence>(e.clone())
                {
                    out.push(e);
                }
                for (key, value) in map {
                    if key != "evidence" {
                        collect(value, out);
                    }
                }
            }
            Value::Array(items) => {
                for value in items {
                    collect(value, out);
                }
            }
            _ => {}
        }
    }
    let mut evidence = vec![];
    for message in messages {
        for content in protocol::context_texts(message) {
            if let Ok(value) = serde_json::from_str::<Value>(content) {
                collect(&value, &mut evidence);
            }
        }
    }
    evidence
}

// Stored with the assistant checkpoint, never sent to the provider. Hashes bind
// character ranges to the exact text visible in the request producing its calls,
// without duplicating the full bodies in every checkpoint or requiring a table.
#[derive(Serialize, Deserialize)]
struct RequestRead {
    version_id: String,
    start: usize,
    len: usize,
    hash: Vec<u8>,
}
fn request_reads(messages: &[Message]) -> Result<Vec<RequestRead>> {
    let mut reads = vec![];
    for evidence in collect_evidence(messages) {
        let SourceRef::Version(version_id) = evidence.source else {
            continue;
        };
        for (start, text) in std::iter::once((evidence.start, evidence.text)).chain(
            evidence
                .additional_spans
                .into_iter()
                .map(|s| (s.start, s.text)),
        ) {
            reads.push(RequestRead {
                version_id: version_id.clone(),
                start,
                len: text.chars().count(),
                hash: fingerprint(&text)?,
            });
        }
    }
    Ok(reads)
}

pub(super) const INCOMPLETE_WRITE_READ: &str = "The complete target version was not visible in the request that produced this write. Read all missing ranges before replacing its full body. If the full body cannot fit in the context budget, leave the memory unchanged.";

pub(super) fn write_request_fully_read(
    db: &rusqlite::Connection,
    messages: &[Value],
    call_id: &str,
    version_id: &str,
) -> Result<bool> {
    let messages = protocol::decode(messages)?;
    let mut owners = messages
        .iter()
        .filter(|m| protocol::calls(&m.message).any(|c| c.id.as_str() == call_id));
    let Some(owner) = owners.next() else {
        return Ok(false);
    };
    if owners.next().is_some() {
        return Ok(false);
    }
    let Some(value) = owner.reads.clone() else {
        return Ok(false);
    };
    let Ok(reads) = serde_json::from_value::<Vec<RequestRead>>(value) else {
        return Ok(false);
    };
    let body: Vec<char> = version(db, version_id)?.body.chars().collect();
    let mut ranges = vec![];
    for read in reads.into_iter().filter(|r| r.version_id == version_id) {
        let Some(end) = read.start.checked_add(read.len) else {
            return Ok(false);
        };
        let Some(actual) = body.get(read.start..end) else {
            return Ok(false);
        };
        if fingerprint(&actual.iter().collect::<String>())? != read.hash {
            return Ok(false);
        }
        ranges.push((read.start, end));
    }
    if ranges.is_empty() {
        return Ok(false);
    }
    ranges.sort_unstable();
    let mut through = 0;
    for (start, end) in ranges {
        if start > through {
            return Ok(false);
        }
        through = through.max(end);
    }
    Ok(through == body.len())
}

impl MemoryStore {
    pub(super) fn filter_unavailable_evidence(&self, messages: &mut [Message]) -> Result<()> {
        let db = self.connection()?;
        for message in messages {
            protocol::edit_context(message, |content| {
                if let Ok(mut value) = serde_json::from_str::<Value>(content) {
                    redact_unavailable(&db, &mut value)?;
                    *content = value.to_string();
                }
                Ok(())
            })?;
        }
        Ok(())
    }

    pub(super) fn agent_read_tool(&self, name: &str, value: &Value) -> Result<Value> {
        match name {
            "search_memories" => {
                let a: SearchArgs = args(value)?;
                if a.limit == 0 || a.limit > 8 {
                    return Err(DataError::Invalid);
                }
                let found = self.search(&SearchRequest {
                    query: a.query,
                    variants: a.variants,
                    limit: a.limit,
                    offset: a.offset,
                    excerpt_chars: 1200,
                    ..Default::default()
                })?;
                let db = self.connection()?;
                let mut items = vec![];
                for hit in found.items {
                    let total = version(&db, &hit.version_id)?.body.chars().count();
                    items.push(evidence_value(&hit.memory_id, hit.evidence, total));
                }
                Ok(
                    json!({"items":items,"next_offset":found.next_offset,"truncated":found.truncated,
                    "mode":found.mode,"degraded_reason":found.degraded_reason}),
                )
            }
            "list_memories" => {
                let a: ListArgs = args(value)?;
                if a.limit == 0 || a.limit > 20 {
                    return Err(DataError::Invalid);
                }
                let found = self.library(&LibraryQuery {
                    collection_id: a.collection_id,
                    offset: a.offset,
                    limit: a.limit,
                    ..Default::default()
                })?;
                // A directory establishes identity and navigation, never a body citation.
                Ok(
                    json!({"directory":found.items.iter().map(|m|json!({"memory_id":m.key.id,"title":m.title})).collect::<Vec<_>>(),
                    "next_offset":found.next_offset,"truncated":found.next_offset.is_some(),"body_evidence":false}),
                )
            }
            "read_memory" => {
                let a: ReadArgs = args(value)?;
                valid_id(&a.memory_id)?;
                if a.max_chars == 0 || a.max_chars > 3000 || a.start_char > 1_000_000 {
                    return Err(DataError::Invalid);
                }
                let memory = self.memory(&a.memory_id)?;
                let db = self.connection()?;
                if a.view == "history" {
                    let rows:Vec<Value>=db.prepare("SELECT id,title,created_at FROM memory_versions WHERE memory_id=?1 AND body IS NOT NULL ORDER BY created_at DESC,rowid DESC LIMIT 11 OFFSET ?2")?
                        .query_map(params![a.memory_id,a.start_char as i64],|r|Ok(json!({"version_id":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"recorded_at_ms":r.get::<_,i64>(2)?})))?
                        .collect::<rusqlite::Result<_>>()?;
                    return Ok(
                        json!({"history":rows.iter().take(10).collect::<Vec<_>>(),"next_offset":(rows.len()>10).then_some(a.start_char+10),"body_evidence":false}),
                    );
                }
                let (source, total) = match a.view.as_str() {
                    "current" => (
                        SourceRef::Version(memory.current.id.clone()),
                        memory.current.body.chars().count(),
                    ),
                    "version" => {
                        let v = version(&db, a.source_id.as_deref().ok_or(DataError::Invalid)?)?;
                        if v.memory_id != a.memory_id {
                            return Err(DataError::Unavailable);
                        }
                        (SourceRef::Version(v.id), v.body.chars().count())
                    }
                    "source" => {
                        let id = a.source_id.ok_or(DataError::Invalid)?;
                        if !db.prepare("SELECT 1 FROM version_captures vc JOIN memory_versions v ON v.id=vc.version_id WHERE v.memory_id=?1 AND vc.capture_id=?2")?.exists(params![a.memory_id,id])? {return Err(DataError::Unavailable);}
                        let capture = raw(&db, &id)?;
                        (SourceRef::Capture(id), capture.text.chars().count())
                    }
                    _ => return Err(DataError::Invalid),
                };
                let evidence = resolve_excerpt(&db, &source, a.max_chars, &[], Some(a.start_char))?;
                let captures = match &source {
                    SourceRef::Version(id) => version(&db, id)?.capture_ids,
                    _ => vec![],
                };
                let mut result = evidence_value(&a.memory_id, evidence, total);
                result["source_ids"] = json!(captures);
                Ok(result)
            }
            "read_conversation" => {
                let a: ConversationArgs = args(value)?;
                if a.limit == 0 || a.limit > 20 || a.after_seq < 0 {
                    return Err(DataError::Invalid);
                }
                let messages = self.agent_conversation_messages(
                    &a.conversation_id,
                    a.after_seq,
                    a.limit,
                    a.manual_saves_offset.unwrap_or(0),
                )?;
                let mut items = vec![];
                let mut bytes = 0;
                for v in messages {
                    let size = v.to_string().len();
                    if bytes + size > 32 * 1024 && !items.is_empty() {
                        break;
                    }
                    bytes += size;
                    items.push(v);
                }
                let next = items.last().and_then(|v| v["seq"].as_i64());
                Ok(
                    json!({"messages":items,"next_after_seq":next,"note":"Conversation, not durable-memory evidence. Preserve speaker and tentative status."}),
                )
            }
            _ => Err(DataError::Invalid),
        }
    }
}

pub(super) fn memory_read_tools() -> Vec<ToolDefinition> {
    vec![
        tools::function(
            "search_memories",
            "Search all active CURRENT user memories. No topic restriction. Supply the natural question plus up to four short alternative literal entities/terms, not whole sentences. Retry fewer terms if empty; empty results are not proof of absence. Return real body evidence and pagination.",
            json!({"query":{"type":"string"},"variants":{"type":"array","items":{"type":"string"},"maxItems":4},"limit":{"type":"integer","minimum":1,"maximum":8},"offset":{"type":"integer","minimum":0}}),
        ),
        tools::function(
            "list_memories",
            "Page a topic directory, or the whole library with collection_id=null. Titles are navigation, not evidence; read relevant bodies by stable ID.",
            json!({"collection_id":{"type":["string","null"]},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":20}}),
        ),
        tools::function(
            "read_memory",
            "Read by memory ID directly (no prior search required). view=current uses the current version; version uses source_id=version ID; source uses source_id=raw capture ID linked to this memory; history lists version IDs with start_char as page offset. Other views use character positions, continue at next_start. Historical evidence stays historical. Never read trash.",
            json!({"memory_id":{"type":"string"},"view":{"type":"string","enum":["current","version","source","history"]},"source_id":{"type":["string","null"]},"start_char":{"type":"integer","minimum":0},"max_chars":{"type":"integer","minimum":1,"maximum":3000}}),
        ),
        tools::function(
            "read_conversation",
            "Page exact past conversation messages, retaining speaker, sequence and status. Use to resolve an early condition after compaction; messages are not automatically durable facts. Each message.manual_saves is one bounded page with items, total and next_offset; never treat a partial page as all saves. To read more saves for the SAME message, set after_seq=that message.seq-1, limit=1 and manual_saves_offset=its next_offset. This does not change message text or sequence pagination. manual_saves_offset null or omitted means 0. Each save has its own input_id for undo; status=undone means already reversed.",
            json!({"conversation_id":{"type":"string"},"after_seq":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":20},"manual_saves_offset":{"type":["integer","null"],"minimum":0}}),
        ),
    ]
}
pub(super) fn memory_write_tool() -> ToolDefinition {
    tools::function(
        "write_memory",
        "Save a user's meaningful idea, fact, constraint or decision now. parts.text concatenate EXACTLY in order into the full Markdown body; include needed spaces and newlines yourself. Each part states one fact or change, with its actual source_id and a verbatim quote (maximum 3000 UTF-8 bytes). Do not put facts from different sources into one part. Read earlier original user messages before saving their facts; 'other conditions unchanged' alone does not source those conditions. For unchanged target text, sources=[] may preserve only complete original lines in their original order, not arbitrary substrings; whitespace-only parts may also use []. To rephrase existing target content, cite its CURRENT expected_version and an exact quote from that body to inherit its old captures. Other sources must be actual user message IDs in this conversation (the task's capture_id for capture maintenance), with at least one such raw source in the write. Never cite an AI message or summary as a user source. Preserve uncertainty, negation, speaker, time, plans versus execution and useful change reasons. Preserve the subject and scope of each quote: these discussed ideas are undecided does not mean no plan has ever been decided. Do not add unsupported all/any/never claims or broaden a local statement into a global user fact. title is only a neutral summary, with no new facts absent from the body. New creates a memory; existing requires the complete current version to be visible in the request and preserves unaffected content. Writes are atomic, versioned and undoable; report only committed receipts.",
        json!({
            "destination":{"anyOf":[{"type":"object","properties":{"kind":{"type":"string","enum":["new"]}},"required":["kind"],"additionalProperties":false},{"type":"object","properties":{"kind":{"type":"string","enum":["existing"]},"memory_id":{"type":"string"},"expected_version":{"type":"string"}},"required":["kind","memory_id","expected_version"],"additionalProperties":false}]},
            "title":{"type":"string"},
            "parts":{"type":"array","minItems":1,"maxItems":64,"items":{
                "type":"object","properties":{
                    "text":{"type":"string"},
                    "sources":{"type":"array","maxItems":16,"items":{
                        "type":"object","properties":{"source_id":{"type":"string"},"quote":{"type":"string"}},
                        "required":["source_id","quote"],"additionalProperties":false
                    }}
                },"required":["text","sources"],"additionalProperties":false
            }}
        }),
    )
}

// UTF-8 bytes are a deliberately conservative upper bound on input token usage.
// These are internal execution limits, not a second model-settings surface.
pub(super) const CONTEXT_TOKENS: usize = 65_536;
pub(super) const OUTPUT_RESERVE: usize = 8192;
pub(super) const INITIAL_CONTEXT_BUDGET: usize = 48_000;
pub(super) const MAX_MODEL_STEPS: usize = 12;

pub(super) enum AgentEvent<'a> {
    Text(&'a str),
    Checkpoint(&'a [Value]),
    BeforeRequest(&'a mut Vec<Message>),
    Tool(&'a tools::ToolCall),
}

pub(super) enum AgentReply {
    Continue,
    Tool(Value),
    Complete,
}

/// One loop for both task owners. The callback owns durable effects and fencing;
/// the driver owns only provider protocol, streaming and bounded continuation.
pub(super) async fn run_memory_agent(
    config: &model::ModelConfig,
    messages: Vec<Value>,
    tool_defs: &[ToolDefinition],
    mut callback: impl FnMut(AgentEvent<'_>) -> std::result::Result<AgentReply, ProbeError>,
) -> std::result::Result<Vec<Value>, ProbeError> {
    let invalid = |_| ProbeError::InvalidResponse;
    let mut messages = protocol::decode(&messages).map_err(invalid)?;
    let reserve =
        (config.max_output_tokens.unwrap_or(OUTPUT_RESERVE as u32) as usize).max(OUTPUT_RESERVE);
    let budget = CONTEXT_TOKENS
        .checked_sub(reserve)
        .ok_or(ProbeError::TooLarge)?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(240);
    for _ in 0..MAX_MODEL_STEPS {
        // Replay committed effects after a restart before asking for another plan.
        if let Some(index) = messages
            .iter()
            .rposition(|m| matches!(m.message, Message::Assistant { .. }))
        {
            let calls: Vec<_> = protocol::calls(&messages[index].message).cloned().collect();
            if calls.is_empty() && index + 1 == messages.len() {
                return protocol::encode(&messages).map_err(invalid);
            }
            for call in calls {
                if messages[index + 1..]
                    .iter()
                    .any(|m| protocol::answered(&m.message, &call))
                {
                    continue;
                }
                let result = match callback(AgentEvent::Tool(&call))? {
                    AgentReply::Tool(result) => result,
                    AgentReply::Complete => return protocol::encode(&messages).map_err(invalid),
                    AgentReply::Continue => return Err(ProbeError::InvalidResponse),
                };
                messages.push(protocol::result(&call, &result));
                callback(AgentEvent::Checkpoint(
                    &protocol::encode(&messages).map_err(invalid)?,
                ))?;
            }
        }
        let mut wire: Vec<Message> = messages.iter().map(|m| m.message.clone()).collect();
        callback(AgentEvent::BeforeRequest(&mut wire))?;
        let defs_size = serde_json::to_vec(tool_defs)
            .map_err(|_| ProbeError::InvalidResponse)?
            .len();
        // Release only re-readable evidence in the request view; preserve checkpoints.
        for index in 0..wire.len().saturating_sub(1) {
            if json!(wire).to_string().len() + defs_size + 1024 <= budget {
                break;
            }
            if protocol::is_result(&wire[index]) {
                protocol::edit_context(&mut wire[index], |content| {
                    if let Ok(mut value) = serde_json::from_str::<Value>(content) {
                        release_evidence_text(&mut value);
                        *content = value.to_string();
                    }
                    Ok(())
                })
                .map_err(invalid)?;
            }
        }
        if json!(wire).to_string().len() + defs_size + 1024 > budget {
            return Err(ProbeError::TooLarge);
        }
        let reads = request_reads(&wire).map_err(invalid)?;
        let needs_separator =
            messages
                .iter()
                .rev()
                .find_map(|m| match &m.message {
                    Message::Assistant { content, .. } => Some(content.iter().any(
                        |p| matches!(p, model::AssistantContent::Text(t) if !t.text.is_empty()),
                    )),
                    _ => None,
                })
                .unwrap_or(false);
        let mut first_text = true;
        let turn = tokio::time::timeout_at(
            deadline,
            tools::stream_turn(config, &wire, tool_defs, |delta| {
                if let tools::StreamDelta::Text(text) = delta {
                    if first_text && needs_separator {
                        callback(AgentEvent::Text("\n\n"))?;
                    }
                    first_text = false;
                    callback(AgentEvent::Text(&text.text))?;
                }
                Ok(())
            }),
        )
        .await
        .map_err(|_| ProbeError::Network)??;
        let has_calls = model::calls(&turn).next().is_some();
        messages.push(Checkpoint {
            message: model::assistant(turn),
            reads: has_calls.then(|| json!(reads)),
        });
        callback(AgentEvent::Checkpoint(
            &protocol::encode(&messages).map_err(invalid)?,
        ))?;
        if !has_calls {
            return protocol::encode(&messages).map_err(invalid);
        }
    }
    Err(ProbeError::TooLarge)
}

fn release_evidence_text(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.contains_key("source") && map.contains_key("start") && map.contains_key("text") {
                map.remove("text");
                map.insert("read_again".into(), json!(true));
            }
            for v in map.values_mut() {
                release_evidence_text(v);
            }
        }
        Value::Array(items) => {
            for item in items {
                release_evidence_text(item);
            }
        }
        _ => {}
    }
}

fn redact_unavailable(db: &rusqlite::Connection, value: &mut Value) -> Result<()> {
    match value {
        Value::Object(map) => {
            if let Some(source) = map.get("source")
                && map.contains_key("text")
                && let Ok(source) = serde_json::from_value::<SourceRef>(source.clone())
            {
                match resolve(db, &source, 1) {
                    Ok(actual) => {
                        if map.get("current").and_then(Value::as_bool) == Some(true)
                            && !actual.current
                        {
                            map.insert("current".into(), json!(false));
                            map.insert("changed_since_read".into(), json!(true));
                        }
                    }
                    Err(DataError::Unavailable) => {
                        map.remove("text");
                        map.remove("additional_spans");
                        map.insert("unavailable".into(), json!(true));
                    }
                    Err(e) => return Err(e),
                }
            }
            for v in map.values_mut() {
                redact_unavailable(db, v)?;
            }
        }
        Value::Array(items) => {
            for v in items {
                redact_unavailable(db, v)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod request_read_tests {
    use super::*;

    #[test]
    fn replacement_requires_exact_unicode_text_and_gapless_visible_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let captured = store
            .capture(&CaptureRequest {
                request_id: id(),
                text: "甲乙🙂丙丁。禁止上传".into(),
                origin: Origin::User {
                    app: "QA".into(),
                    project: None,
                    uri: None,
                },
            })
            .unwrap();
        let db = store.connection().unwrap();
        let source = SourceRef::Version(captured.version_id.clone());
        let first = resolve_excerpt(&db, &source, 3, &[], Some(0)).unwrap();
        let rest = resolve_excerpt(&db, &source, 100, &[], Some(3)).unwrap();
        let authorize = |evidence: Evidence| {
            let request = vec![Message::user(json!({"evidence":evidence}).to_string())];
            let mut response = json!({"role":"assistant","tool_calls":[{"id":"write","function":{"name":"write_memory","arguments":"{}"}}]});
            response["_memivy_request_reads"] = json!(request_reads(&request).unwrap());
            write_request_fully_read(&db, &[response], "write", &captured.version_id).unwrap()
        };
        let mut complete = first.clone();
        complete.additional_spans.push(EvidenceSpan {
            start: rest.start,
            text: rest.text.clone(),
            truncated: false,
        });
        assert!(authorize(complete.clone()));

        let mut gap = complete.clone();
        gap.additional_spans[0].start += 1;
        gap.additional_spans[0].text = rest.text.chars().skip(1).collect();
        assert!(!authorize(gap));

        let mut changed_text = complete;
        changed_text.text = "甲乙丙".into(); // Same character count is insufficient.
        assert!(!authorize(changed_text));
        assert!(!authorize(first));
    }

    #[test]
    fn deleting_a_memory_removes_all_its_spans_from_context_and_tool_results() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let capture = |text: &str| {
            store
                .capture(&CaptureRequest {
                    request_id: id(),
                    text: text.into(),
                    origin: Origin::User {
                        app: "QA".into(),
                        project: None,
                        uri: None,
                    },
                })
                .unwrap()
        };
        let deleted =
            capture("Main span: uploads forbidden. Additional span: process locally only.");
        let retained = capture("Another memory that is still available.");
        let db = store.connection().unwrap();
        let make_spans = |source: SourceRef| {
            let mut evidence = resolve_excerpt(&db, &source, 9, &[], Some(0)).unwrap();
            let tail = resolve_excerpt(&db, &source, 100, &[], Some(9)).unwrap();
            evidence.additional_spans.push(EvidenceSpan {
                start: tail.start,
                text: tail.text,
                truncated: false,
            });
            evidence
        };
        let version = make_spans(SourceRef::Version(deleted.version_id.clone()));
        let raw = make_spans(SourceRef::Capture(deleted.capture_id.clone()));
        let active = resolve_excerpt(
            &db,
            &SourceRef::Version(retained.version_id.clone()),
            100,
            &[],
            Some(0),
        )
        .unwrap();
        let mut messages = vec![
            Message::user(json!({"items":[{"evidence":version},{"evidence":active}]}).to_string()),
            Message::tool_result(
                "read-source",
                "read_memory",
                json!({"evidence":raw}).to_string(),
            ),
        ];
        store
            .trash_memory(&deleted.memory_id, &deleted.version_id)
            .unwrap();
        store.filter_unavailable_evidence(&mut messages).unwrap();
        let context: Value =
            serde_json::from_str(protocol::context_texts(&messages[0])[0]).unwrap();
        let tool: Value = serde_json::from_str(protocol::context_texts(&messages[1])[0]).unwrap();
        for evidence in [&context["items"][0]["evidence"], &tool["evidence"]] {
            assert_eq!(evidence["unavailable"], true);
            assert!(evidence.get("text").is_none());
            assert!(evidence.get("additional_spans").is_none());
        }
        assert_eq!(
            context["items"][1]["evidence"]["text"],
            "Another memory that is still available."
        );
        let reads = request_reads(&messages).unwrap();
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].version_id, retained.version_id);
        let mut response = json!({"role":"assistant","tool_calls":[{"id":"write","function":{"name":"write_memory","arguments":"{}"}}]});
        response["_memivy_request_reads"] = json!(reads);
        assert!(!write_request_fully_read(&db, &[response], "write", &deleted.version_id).unwrap());
    }
}

#[cfg(test)]
mod write_attribution_tests {
    use super::*;

    fn part(text: &str, source_id: &str, quote: &str) -> MemoryWritePart {
        MemoryWritePart {
            text: text.into(),
            sources: vec![MemorySourceQuote {
                source_id: source_id.into(),
                quote: quote.into(),
            }],
        }
    }
    fn capture(store: &MemoryStore, text: &str) -> CaptureResult {
        store
            .capture(&CaptureRequest {
                request_id: id(),
                text: text.into(),
                origin: Origin::User {
                    app: "attribution QA".into(),
                    project: None,
                    uri: None,
                },
            })
            .unwrap()
    }

    #[test]
    fn quoted_parts_preserve_exact_composition_and_only_return_raw_source_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let old = capture(
            &store,
            "Previously 4 hours per week.\nDo not upload recordings.",
        );
        let source = capture(&store, "Now 8 hours per week, not launched yet.");
        let previous = store.memory(&old.memory_id).unwrap().current;
        let write = MemoryWriteArgs {
            destination: Destination::Existing {
                memory_id: old.memory_id,
                expected_version: previous.id.clone(),
            },
            title: "Current constraints".into(),
            parts: vec![
                part(
                    "Previously 4 hours per week; ",
                    &previous.id,
                    "Previously 4 hours per week.",
                ),
                part(
                    "Now 8 hours per week.\n",
                    &source.capture_id,
                    "Now 8 hours per week",
                ),
                part(
                    "Not launched yet.\n",
                    &source.capture_id,
                    "not launched yet",
                ),
                MemoryWritePart {
                    text: "Do not upload recordings.\n".into(),
                    sources: vec![],
                },
            ],
        };
        let (body, ids) = resolve_memory_write(&write, Some(&previous), |source_id| {
            assert_eq!(source_id, source.capture_id); // Target version never enters the raw resolver.
            Ok(store.capture_by_id(source_id)?.text)
        })
        .unwrap();
        assert_eq!(
            body,
            "Previously 4 hours per week; Now 8 hours per week.\nNot launched yet.\nDo not upload recordings.\n"
        );
        assert_eq!(ids, vec![source.capture_id.clone()]);
        let mut wrong_quote = write;
        wrong_quote.parts[1].sources[0].quote = "Now 80 hours per week".into();
        assert_eq!(
            resolve_memory_write(&wrong_quote, Some(&previous), |id| Ok(store
                .capture_by_id(id)?
                .text)),
            Err(DataError::SourceAttribution)
        );
    }

    #[test]
    fn inheritance_cannot_remove_negation_reorder_lines_or_attach_sources_to_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let old = capture(
            &store,
            "Do not upload recordings.\nOnly considering paid access, not decided.\nKeep offline mode.",
        );
        let source = capture(&store, "Addition: 8 hours per week.");
        let previous = store.memory(&old.memory_id).unwrap().current;
        let write = MemoryWriteArgs {
            destination: Destination::Existing {
                memory_id: old.memory_id,
                expected_version: previous.id.clone(),
            },
            title: "Constraints".into(),
            parts: vec![
                MemoryWritePart {
                    text: "Do not upload recordings.\nOnly considering paid access, not decided.\n"
                        .into(),
                    sources: vec![],
                },
                part(
                    "8 hours per week.\n",
                    &source.capture_id,
                    "8 hours per week",
                ),
                MemoryWritePart {
                    text: "Keep offline mode.\n".into(),
                    sources: vec![],
                },
            ],
        };
        let validate = |write: &MemoryWriteArgs| {
            resolve_memory_write(write, Some(&previous), |id| {
                Ok(store.capture_by_id(id)?.text)
            })
        };
        assert!(validate(&write).is_ok());
        let mut changed = write.clone();
        changed.parts[0].text = "Upload recordings.\n".into();
        assert_eq!(validate(&changed), Err(DataError::SourceAttribution));
        let mut reordered = write.clone();
        reordered.parts.swap(0, 2);
        assert_eq!(validate(&reordered), Err(DataError::SourceAttribution));
        let mut no_raw = write.clone();
        no_raw.parts.remove(1);
        assert_eq!(validate(&no_raw), Err(DataError::SourceAttribution));
        no_raw
            .parts
            .push(part("\n ", &source.capture_id, "8 hours per week"));
        assert_eq!(validate(&no_raw), Err(DataError::SourceAttribution));
        let mut formatting = write;
        formatting.parts.push(MemoryWritePart {
            text: "\n \t".into(),
            sources: vec![],
        });
        assert!(validate(&formatting).unwrap().0.ends_with("\n \t"));
    }

    #[test]
    fn attribution_limits_count_utf8_bytes_and_bound_parts_and_source_lists() {
        let source_id = id();
        let raw = "甲".repeat(1001);
        let mut write = MemoryWriteArgs {
            destination: Destination::New,
            title: "Synthetic text".into(),
            parts: vec![part("Synthetic text", &source_id, &"甲".repeat(1000))],
        };
        assert!(resolve_memory_write(&write, None, |_| Ok(raw.clone())).is_ok());
        write.parts[0].sources[0].quote = raw.clone();
        assert_eq!(
            resolve_memory_write(&write, None, |_| Ok(raw.clone())),
            Err(DataError::Invalid)
        );
        write.parts[0].sources[0].quote = "甲".into();
        write.parts[0].sources = vec![write.parts[0].sources[0].clone(); 17];
        assert_eq!(
            resolve_memory_write(&write, None, |_| Ok(raw.clone())),
            Err(DataError::Invalid)
        );
        write.parts[0].sources.truncate(1);
        write.parts.extend((0..63).map(|_| MemoryWritePart {
            text: "\n".into(),
            sources: vec![],
        }));
        assert!(resolve_memory_write(&write, None, |_| Ok(raw.clone())).is_ok());
        write.parts.push(MemoryWritePart {
            text: "\n".into(),
            sources: vec![],
        });
        assert_eq!(
            resolve_memory_write(&write, None, |_| Ok(raw.clone())),
            Err(DataError::Invalid)
        );
    }
}
