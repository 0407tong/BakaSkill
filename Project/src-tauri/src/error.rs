//! 统一错误类型。
//!
//! 所有 Tauri 命令返回 `AppResult<T>`。`AppError` 会被序列化成
//! `{ kind, message }` 形状，前端 `src/lib/ipc.ts` 依赖该形状做归一化。
//!
//! **改动本文件的 kind 取值时，必须同步修改 `src/types/ipc.ts` 的 AppErrorKind。**

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Internal(String),

    /// 当前平台不支持该能力（非 Windows 平台的链接操作会返回此项）
    #[error("当前平台暂不支持该操作：{0}")]
    Unsupported(String),

    #[error("文件系统错误：{0}")]
    Io(String),

    #[error("配置错误：{0}")]
    Config(String),

    #[error("未找到：{0}")]
    NotFound(String),

    #[error("权限不足：{0}")]
    PermissionDenied(String),

    /// 目标已存在等冲突情形
    #[error("冲突：{0}")]
    Conflict(String),

    /// 目标路径不是目录链接。
    ///
    /// **安全关键**：删除链接前必须确认它确实是链接。若返回此项而调用方仍执行
    /// 删除，就会删掉用户的真实目录。见 `platform::link::delete_junction`。
    #[error("不是目录链接：{0}")]
    NotAJunction(String),

    /// 链接指向的目标不在本项目中央库内，拒绝代为删除
    #[error("链接目标不在中央库内：{0}")]
    TargetOutsideLibrary(String),
}

impl AppError {
    /// 与前端 AppErrorKind 联合类型一一对应
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::Internal(_) => "Internal",
            AppError::Unsupported(_) => "Unsupported",
            AppError::Io(_) => "Io",
            AppError::Config(_) => "Config",
            AppError::NotFound(_) => "NotFound",
            AppError::PermissionDenied(_) => "PermissionDenied",
            AppError::Conflict(_) => "Conflict",
            AppError::NotAJunction(_) => "NotAJunction",
            AppError::TargetOutsideLibrary(_) => "TargetOutsideLibrary",
        }
    }

    /// 供日志与诊断使用：把操作系统错误细化成更准确的分类
    pub fn from_io(context: &str, err: std::io::Error) -> Self {
        use std::io::ErrorKind;
        let detail = format!("{context}：{err}");
        match err.kind() {
            ErrorKind::NotFound => AppError::NotFound(detail),
            ErrorKind::PermissionDenied => AppError::PermissionDenied(detail),
            ErrorKind::AlreadyExists => AppError::Conflict(detail),
            _ => AppError::Io(detail),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::from_io("文件操作失败", err)
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;
