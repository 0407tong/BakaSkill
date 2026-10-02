//! 导入导出命令。
//!
//! 导入分两步（`import_preview` → `import_apply`），因为同名冲突必须让用户
//! 先看到再决定。详见 `crate::transfer` 的模块说明。

use std::path::PathBuf;

use crate::config;
use crate::error::{AppError, AppResult};
use crate::transfer::{
    self, ExportFormat, ExportReport, ImportDecision, ImportPreview, ImportReport, ImportSource,
};

fn resolve_root(path: Option<String>) -> AppResult<PathBuf> {
    match path {
        Some(p) if !p.trim().is_empty() => Ok(PathBuf::from(p)),
        _ => {
            let cfg = config::load()?;
            cfg.central_library()
                .map(PathBuf::from)
                .ok_or_else(|| AppError::Config("尚未配置中央库路径".to_string()))
        }
    }
}

/// 第一步：准备导入并返回预览（候选、同名冲突、非法项）
#[tauri::command(async)]
pub fn import_preview(source: ImportSource, path: Option<String>) -> AppResult<ImportPreview> {
    transfer::preview(&resolve_root(path)?, &source)
}

/// 拖拽入口：把用户拖进窗口的**真实路径**直接变成预览。
///
/// 与 `import_preview` 的差别：这里收的是一批路径，且必须由后端来判断
/// 每一条是目录、ZIP 还是单个 SKILL.md——前端只有路径字符串，问不了文件系统。
/// 认不出来的项会出现在预览的 `rejected` 里，不会被静默丢掉。
#[tauri::command(async)]
pub fn import_preview_paths(paths: Vec<String>, path: Option<String>) -> AppResult<ImportPreview> {
    transfer::preview_paths(&resolve_root(path)?, &paths)
}

/// 第二步：按逐项选择把内容落进中央库
#[tauri::command(async)]
pub fn import_apply(
    token: String,
    decisions: Vec<ImportDecision>,
    path: Option<String>,
) -> AppResult<ImportReport> {
    transfer::apply(&resolve_root(path)?, &token, &decisions)
}

/// 导出选中的 Skill 为 ZIP 或纯目录结构
#[tauri::command(async)]
pub fn export_skills(
    dir_names: Vec<String>,
    dest: String,
    format: ExportFormat,
    include_manifest: bool,
    path: Option<String>,
) -> AppResult<ExportReport> {
    transfer::export(
        &resolve_root(path)?,
        &dir_names,
        &dest,
        format,
        include_manifest,
    )
}
