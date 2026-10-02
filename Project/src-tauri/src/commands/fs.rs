//! 通用文本文件写入。
//!
//! 仅用于「导出报告」这类由用户显式选择目标路径的场景。
//! **不提供任意删除/移动能力**——那类操作必须走各自的领域模块。

use crate::error::{AppError, AppResult};

/// 把文本写入用户指定的路径。
///
/// 安全约束：目标是用户通过系统保存对话框亲自选定的路径，
/// 后端不做目录遍历，也不覆盖目录（只写文件）。
#[tauri::command(async)]
pub fn write_text_file(path: String, content: String) -> AppResult<String> {
    let target = std::path::PathBuf::from(&path);

    if target.is_dir() {
        return Err(AppError::Config(format!(
            "目标是目录而不是文件：{}",
            target.display()
        )));
    }
    if let Some(parent) = target.parent() {
        if !parent.is_dir() {
            return Err(AppError::NotFound(format!(
                "上级目录不存在：{}",
                parent.display()
            )));
        }
    }

    std::fs::write(&target, content.as_bytes())
        .map_err(|err| AppError::from_io(&format!("写入失败：{}", target.display()), err))?;

    tracing::info!(path = %target.display(), "已导出文件");
    Ok(target.display().to_string())
}
