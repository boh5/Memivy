use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    User,
    Ai,
}
impl Actor {
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Ai => "ai",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Origin {
    User {
        app: String,
        project: Option<String>,
        uri: Option<String>,
    },
    Agent {
        app: String,
        project: Option<String>,
        uri: Option<String>,
    },
    Conversation {
        conversation_id: String,
        message_id: String,
        message_role: String,
        confirmed_by: String,
    },
    Discussion {
        conversation_id: String,
        message_id: String,
        app: String,
        project: Option<String>,
        uri: Option<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureRequest {
    pub request_id: String,
    pub text: String,
    pub origin: Origin,
}
/// A successful capture already has an editable, searchable Memory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaptureResult {
    pub memory_id: String,
    pub version_id: String,
    pub capture_id: String,
    pub created_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawCapture {
    pub id: String,
    pub text: String,
    pub origin: Origin,
    pub created_at: i64,
    pub understanding: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Destination {
    New,
    Existing {
        memory_id: String,
        expected_version: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeRequest {
    pub request_id: String,
    pub capture_id: String,
    pub destination: Destination,
    pub title: String,
    pub body: String,
    pub actor: Actor,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditRequest {
    pub request_id: String,
    pub memory_id: String,
    pub expected_version: String,
    pub title: String,
    pub body: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub state: String,
    pub current: Version,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Version {
    pub id: String,
    pub memory_id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub body: String,
    pub actor: String,
    pub reason: String,
    pub created_at: i64,
    pub capture_ids: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub request_id: String,
    pub action: String,
    pub capture_id: Option<String>,
    pub memory_id: Option<String>,
    pub before_version: Option<String>,
    pub after_version: Option<String>,
    pub status: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReceiptChange {
    pub memory_id: String,
    pub before_version: Option<String>,
    pub after_version: String,
    pub before_state: String,
    pub after_state: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum SourceRef {
    Capture(String),
    Version(String),
}
impl SourceRef {
    pub(super) fn parts(&self) -> (&'static str, &str) {
        match self {
            Self::Capture(id) => ("capture", id),
            Self::Version(id) => ("version", id),
        }
    }
    pub(super) fn from_parts(kind: String, id: String) -> rusqlite::Result<Self> {
        match kind.as_str() {
            "capture" => Ok(Self::Capture(id)),
            "version" => Ok(Self::Version(id)),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Citation {
    pub source: SourceRef,
    pub available: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evidence {
    pub source: SourceRef,
    pub title: String,
    pub text: String,
    pub truncated: bool,
    pub recorded_at: i64,
    pub current: bool,
    pub start: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_spans: Vec<EvidenceSpan>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceSpan {
    pub start: usize,
    pub text: String,
    pub truncated: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conversation {
    #[serde(default)]
    pub collection_id: Option<String>,
    pub id: String,
    pub title: String,
    pub draft: String,
    pub updated_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub created_at: i64,
    pub seq: i64,
    pub id: String,
    pub turn_id: String,
    pub role: String,
    pub text: String,
    pub status: String,
    pub error_code: Option<String>,
    pub citations: Vec<Citation>,
    pub followups: Vec<String>,
    pub receipts: Vec<Receipt>,
    pub progress: Option<String>,
    pub record_only: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Turn {
    pub id: String,
    pub user: Message,
    pub assistant: Message,
}
#[derive(Clone, Copy, Debug)]
pub enum Failure {
    Network,
    RateLimit,
    InvalidAnswer,
    SourceUnavailable,
    ToolsUnsupported,
    Budget,
}
