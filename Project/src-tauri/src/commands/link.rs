//! 目录链接命令。
//!
//! 这一层刻意保持极薄：所有安全判断都在 `platform::link` 内，
//! 避免"某个命令自己实现了删除逻辑"从而绕开安全闸门。

use std::path::PathBuf;

use serde::Serialize;

use crate::error::AppResult;
use crate::platform::link::{self, LinkKind};

/// 单个链接的状态快照
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkStatus {
    pub path: String,
    pub kind: LinkKind,
    /// junction 指向的目标路径。悬空链接也会返回原始目标。
    pub target: Option<String>,
    /// 目标当前是否可达。`kind == Junction && !target_exists` 即"断链"。
    pub target_exists: bool,
}

/// 创建目录 junction：让 `link` 指向 `target`。
///
/// 参数顺序是 **(link, target)** —— 面向用户的直觉顺序。
/// 底层 `junction` crate 的签名恰好相反，转换在 `platform::link` 内完成，
/// 调用方不需要关心。
#[tauri::command]
pub fn link_create(link_path: String, target_path: String) -> AppResult<LinkStatus> {
    let link_path = PathBuf::from(link_path);
    let target_path = PathBuf::from(target_path);

    link::create_junction(&target_path, &link_path)?;
    tracing::info!(link = %link_path.display(), target = %target_path.display(), "已创建链接");

    link_status(link_path.display().to_string())
}

/// 删除目录 junction（**只删链接，不动目标内容**）。
///
/// 若目标不是 junction，返回 `NotAJunction` 错误并拒绝执行。
#[tauri::command]
pub fn link_delete(link_path: String) -> AppResult<()> {
    let path = PathBuf::from(&link_path);
    link::delete_junction(&path)?;
    tracing::info!(link = %path.display(), "已删除链接");
    Ok(())
}

/// 查询单个路径的链接状态
#[tauri::command]
pub fn link_status(path: String) -> AppResult<LinkStatus> {
    let path = PathBuf::from(&path);
    let kind = link::classify(&path)?;
    let target = link::junction_target(&path)?;
    let target_exists = target.as_ref().map(|t| t.exists()).unwrap_or(false);

    Ok(LinkStatus {
        path: path.display().to_string(),
        kind,
        target: target.map(|t| t.display().to_string()),
        target_exists,
    })
}

/// 列出某个目录下的全部链接及其目标（**不跟随**重解析点）
#[tauri::command]
pub fn link_list(dir: String) -> AppResult<Vec<LinkStatus>> {
    let dir = PathBuf::from(&dir);
    let entries = link::list_links_in(&dir)?;

    Ok(entries
        .into_iter()
        .map(|(link_path, target)| LinkStatus {
            path: link_path.display().to_string(),
            kind: LinkKind::Junction,
            target_exists: target.exists(),
            target: Some(target.display().to_string()),
        })
        .collect())
}
