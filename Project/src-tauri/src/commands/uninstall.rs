//! 卸载命令。
//!
//! 与 `link_set_enabled` 的界线：那条命令只管链接的增删，中央库不动；
//! 这条把目录**搬离中央库**（送去回收站），并连带摘掉它的全部链接。
//! 详见 `crate::uninstall` 的模块说明。

use std::path::PathBuf;

use crate::config;
use crate::error::{AppError, AppResult};
use crate::uninstall::{self, UninstallReport};

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

/// 把若干个 Skill 从中央库卸载（送到 Windows 回收站）。
///
/// 逐项独立结果，部分失败不回滚已成功项——回滚本身也是文件系统操作，
/// 可能再失败一次，把状态弄得更乱。调用方须把每一项的结果如实说给用户。
#[tauri::command(async)]
pub fn skills_uninstall(
    dir_names: Vec<String>,
    path: Option<String>,
) -> AppResult<UninstallReport> {
    uninstall::uninstall(&resolve_root(path)?, &dir_names)
}
