//! Phase 1 only: an isolated capture/search store and an explicit model probe.
//! No memory editing, AI actions, history, export, or background worker.
mod config;
pub mod model;
mod store;

pub use config::DataPaths;
pub use store::{Capture, CaptureInput, Diagnostics, SearchPage, Store};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("本地文件操作失败，请检查目录权限")]
    Io(#[from] std::io::Error),
    #[error("数据库操作失败，原话未确认保存，请重试")]
    Database(#[from] rusqlite::Error),
    #[error("{0}")]
    Invalid(&'static str),
    #[error("此数据库版本比样机更新，已拒绝写入")]
    NewerSchema,
    #[error("MCP 已关闭或配置不可读取")]
    McpDisabled,
    #[error("同一请求标识对应不同内容，已拒绝覆盖")]
    RequestConflict,
}

pub type Result<T> = std::result::Result<T, Error>;
