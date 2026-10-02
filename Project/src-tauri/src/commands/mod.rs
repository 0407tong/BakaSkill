//! Tauri 命令层。
//!
//! 本层保持"薄"：只做参数校验、调用领域模块、把结果/错误转成 IPC 形状。
//! 业务逻辑不应写在这里，以便脱离 Tauri 运行时做单元测试。

pub mod agents;
pub mod background;
pub mod config;
pub mod fs;
pub mod index;
pub mod library;
pub mod link;
pub mod links;
pub mod skill;
pub mod system;
pub mod tags;
pub mod transfer;
pub mod uninstall;
