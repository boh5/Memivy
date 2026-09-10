//! Formal local data model, independent of the Phase 1 database and UI.
//! All writes, including future UI/MCP writes, must pass through this module.
//! Model requests are bounded and run outside database transactions.
mod access;
mod agent;
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
mod records;
mod related;
mod retrieval;
mod search;
mod transfer;
mod types;

pub use cleanup::*;
pub use collection_recommendations::*;
pub use db::MemoryStore;
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
    #[error("MCP 已关闭，请在 Memivy 设置中开启后重试")]
    McpDisabled,
    #[error("本地文件操作失败，请检查权限和剩余空间")]
    Io,
    #[error("数据库操作失败，本次写入未确认完成")]
    Database,
    #[error("数据库正忙，本次写入未确认完成，请使用原请求重试")]
    Busy,
    #[error("已有同名专题，请换一个名称")]
    CollectionName,
    #[error("置顶或专题已达到 100 项，请先整理后再添加")]
    NavigationLimit,
    #[error("输入无效或超过长度限制")]
    Invalid,
    #[error("内容不存在、已删除或不可用")]
    Unavailable,
    #[error("内容已有新版本，请重新核对后再操作")]
    Conflict,
    #[error("同一请求标识对应不同内容，已拒绝写入")]
    RequestConflict,
    #[error("数据库格式不匹配或版本过新，已拒绝写入")]
    Schema,
    #[error("备份校验失败，未恢复数据")]
    Integrity,
    #[error("目标已存在，请选择新的导出或恢复位置")]
    DestinationExists,
    #[error("这个词匹配范围太大，请增加关键词或缩小时间范围")]
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
