//! Local memory data model shared by the desktop application and MCP.
//! All writes, including future UI/MCP writes, must pass through this module.
//! Model requests are bounded and run outside database transactions.
mod changes;
pub use changes::*;
mod access;
mod agent;
mod agent_mutations;
mod agent_state;
mod cleanup;
mod collection_recommendations;
mod conversations;
mod db;
mod discussion;
mod embedding;
mod library;
mod mcp;
mod navigation;
mod organization;
mod protocol;
mod records;
mod related;
mod retrieval;
mod search;
mod transfer;
mod types;

pub use agent::{MemorySourceQuote, MemoryWriteArgs, MemoryWritePart};
pub use agent_mutations::*;
pub use agent_state::*;
pub use cleanup::*;
pub use collection_recommendations::*;
pub use db::MemoryStore;
pub use discussion::AgentInputChange;
pub use embedding::EmbeddingStatus;
pub use library::*;
pub use mcp::*;
pub use navigation::*;
pub use organization::*;
pub use related::*;
pub use search::*;
pub use transfer::*;
pub use types::*;

/// Display and Debug deliberately omit SQL, paths, content and provider errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DataError {
    #[error("MCP is disabled; enable it in Memivy Settings and try again")]
    McpDisabled,
    #[error("Local file operation failed; check permissions and available space")]
    Io,
    #[error("Database operation failed; this write was not confirmed")]
    Database,
    #[error("The database is busy; this write was not confirmed. Retry the original request")]
    Busy,
    #[error("A collection with this name already exists; choose another name")]
    CollectionName,
    #[error(
        "The limit of 100 pins or collections has been reached; remove some before adding more"
    )]
    NavigationLimit,
    #[error("Input is invalid or exceeds the length limit")]
    Invalid,
    #[error(
        "Cite actual user words for each part. Parts without citations must preserve complete existing lines without removing negation or changing content"
    )]
    SourceAttribution,
    #[error("Content does not exist, was deleted, or is unavailable")]
    Unavailable,
    #[error("Content has a newer version; review it before continuing")]
    Conflict,
    #[error("This request ID was used with different content; the write was rejected")]
    RequestConflict,
    #[error("The database format is incompatible or too new; the write was rejected")]
    Schema,
    #[error("Backup validation failed; no data was restored")]
    Integrity,
    #[error("The destination already exists; choose a new export or restore location")]
    DestinationExists,
    #[error("The query matches too much content; add keywords or narrow the time range")]
    SearchBudget,
}
impl From<std::io::Error> for DataError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}
impl From<rusqlite::Error> for DataError {
    fn from(error: rusqlite::Error) -> Self {
        match error {
            rusqlite::Error::SqliteFailure(e, _)
                if matches!(
                    e.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                ) =>
            {
                Self::Busy
            }
            rusqlite::Error::QueryReturnedNoRows => Self::Unavailable,
            rusqlite::Error::SqliteFailure(e, _)
                if e.code == rusqlite::ErrorCode::OperationInterrupted =>
            {
                Self::SearchBudget
            }
            _ => Self::Database,
        }
    }
}
pub type Result<T> = std::result::Result<T, DataError>;
