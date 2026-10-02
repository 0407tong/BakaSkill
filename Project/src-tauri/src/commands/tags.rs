//! 标签命令。改的是 Skill 文件本身（frontmatter 里的 `tags`），
//! 索引随后重建——标签的单一事实来源始终是文件。

use std::path::PathBuf;

use crate::config;
use crate::error::{AppError, AppResult};
use crate::tags::{self, TagCount, TagReport};

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

/// 统计中央库里的标签及各自的 Skill 数
#[tauri::command]
pub fn tags_stats(path: Option<String>) -> AppResult<Vec<TagCount>> {
    tags::stats(&resolve_root(path)?)
}

/// 改写标签。`target` 为 `None` 表示删除。
///
/// 一个入口覆盖三个界面动作（重命名 / 合并 / 删除）——它们本就是同一件事
/// 的不同参数化，分成三条命令只会让三处各自演化出细微不同的行为。
#[tauri::command]
pub fn tags_apply(
    sources: Vec<String>,
    target: Option<String>,
    path: Option<String>,
) -> AppResult<TagReport> {
    tags::apply(&resolve_root(path)?, &sources, target.as_deref())
}
