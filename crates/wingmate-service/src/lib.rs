//! DSH 后台管理库；不依赖 Tauri，桌面和 CLI 共用同一套生命周期实现。
mod process;
mod runtime;
pub mod service;

pub use runtime::directory;
