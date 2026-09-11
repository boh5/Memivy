//! Shared Rust core. `memory` is the formal Phase 2 data layer.
//! The root Store/conversation APIs remain the isolated Phase 1 experiment.
mod config;
pub mod conversation;
pub mod embedding;
pub mod memory;
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
pub mod speech;

pub mod models;
