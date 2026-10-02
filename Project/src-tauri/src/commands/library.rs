//! 中央库相关命令：路径诊断、初始化、统计。

use crate::config;
use crate::error::{AppError, AppResult};
use crate::library::{self, InitializeResult, LibraryStats, PathDiagnosis};

/// 诊断候选的中央库路径。**不修改文件系统**（除了一次可写性探测）。
///
/// 这是中央库路径选择器的后端支撑：用户在设置页选定
/// 文件夹后立即调用它，把"能不能用、有什么风险"当场讲清楚。
#[tauri::command]
pub fn library_validate(path: String) -> AppResult<PathDiagnosis> {
    let diagnosis = library::diagnose(&path)?;
    // 记录结论而非仅记录"被调用过"：这样事后核对与故障排查都能从日志还原
    // 用户当时看到的判定依据。
    tracing::debug!(
        path = %diagnosis.normalized_path,
        filesystem = ?diagnosis.filesystem,
        volume_kind = ?diagnosis.volume_kind,
        cloud_sync = ?diagnosis.cloud_sync,
        can_initialize = diagnosis.can_initialize,
        issue_codes = ?diagnosis.issues.iter().map(|i| i.code.as_str()).collect::<Vec<_>>(),
        "路径校验"
    );
    Ok(diagnosis)
}

/// 创建中央库目录骨架（幂等）
#[tauri::command]
pub fn library_init(path: String) -> AppResult<InitializeResult> {
    library::initialize(&path)
}

/// 读取中央库统计。
///
/// `path` 为 `None` 时使用配置中已保存的中央库路径；两者都没有则报错，
/// 而不是静默返回零值——"未配置"和"空库"是两种不同状态，UI 需要区分。
#[tauri::command]
pub fn library_stats(path: Option<String>) -> AppResult<LibraryStats> {
    let target = match path {
        Some(p) if !p.trim().is_empty() => p,
        _ => {
            let cfg = config::load()?;
            match cfg.central_library() {
                Some(p) => p.to_string(),
                None => {
                    return Err(AppError::Config("尚未配置中央库路径".to_string()));
                }
            }
        }
    };
    library::stats(&target)
}
