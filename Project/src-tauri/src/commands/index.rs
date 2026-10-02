//! 索引命令。索引是派生物，这些命令都不修改用户的 Skill 文件。

use std::path::PathBuf;

use crate::config;
use crate::error::{AppError, AppResult};
use crate::index::{self, RebuildReport, SearchHit, SkillRecord};

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

/// 全量重建索引，返回实际索引到的 Skill 与跳过项。
///
/// `skillCount` 必须等于磁盘上 `skills/*/SKILL.md` 的数量。
#[tauri::command]
pub fn index_rebuild(path: Option<String>) -> AppResult<RebuildReport> {
    index::rebuild(&resolve_root(path)?)
}

/// 读取索引中的 Skill 列表
#[tauri::command]
pub fn index_list(path: Option<String>) -> AppResult<Vec<SkillRecord>> {
    index::list_all(&resolve_root(path)?)
}

/// 全文搜索中央库里的 Skill，返回命中的 id 与正文片段。
///
/// **只搜中央库**：索引里只有中央库的内容。Agent 目录里的外部 Skill 没有
/// 索引、也就没有正文可搜，它们由前端在已扫描到的清单里按名称/描述匹配
/// （见 `src/lib/skillFilter.ts`）。两边合起来才是用户在搜索框里看到的完整结果。
#[tauri::command]
pub fn skills_search(keywords: String, path: Option<String>) -> AppResult<Vec<SearchHit>> {
    index::search(&resolve_root(path)?, &keywords, index::SEARCH_LIMIT)
}
