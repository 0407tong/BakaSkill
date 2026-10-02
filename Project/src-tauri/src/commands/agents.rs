//! Agent 检测与统一扫描命令。

use std::path::{Path, PathBuf};

use crate::agents::{self, AgentDetection};
use crate::config;
use crate::error::{AppError, AppResult};
use crate::scanner::{self, ScanReport};

/// 检测各 Agent 的安装状态与技能目录。
///
/// 用户在配置中手动指定的 `skillDir` 优先于内置候选路径。
/// 未安装的 Agent 也会返回（状态为 `notInstalled`），由前端决定是否隐藏——
/// 把"是否展示"的判断放在 UI 层，后端只负责如实报告。
#[tauri::command(async)]
pub fn agents_detect() -> AppResult<Vec<AgentDetection>> {
    let cfg = config::load()?;
    let descriptors = agents::registry()?;

    let detections: Vec<AgentDetection> = descriptors
        .iter()
        .map(|descriptor| {
            let override_dir = cfg
                .agents
                .iter()
                .find(|a| a.id == descriptor.id)
                .and_then(|a| a.skill_dir.as_deref());
            agents::detect(descriptor, override_dir)
        })
        .collect();

    tracing::debug!(
        detected = detections
            .iter()
            .filter(|d| d.status == agents::AgentStatus::Detected)
            .count(),
        installed_no_dir = detections
            .iter()
            .filter(|d| d.status == agents::AgentStatus::InstalledNoSkillDir)
            .count(),
        "Agent 检测完成"
    );

    // 追加用户自定义的 Agent：配置文件里有、但内置注册表里没有的条目。
    // 这样新出现的工具或内部自研工具无需改代码即可纳入管理。
    let mut all = detections;
    for cfg_agent in &cfg.agents {
        if descriptors.iter().any(|d| d.id == cfg_agent.id) {
            continue;
        }
        let Some(dir) = cfg_agent
            .skill_dir
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        else {
            continue;
        };
        let descriptor =
            agents::custom_descriptor(&cfg_agent.id, cfg_agent.display_name.as_deref(), dir);
        all.push(agents::detect(&descriptor, Some(dir)));
    }

    Ok(all)
}

/// 为「装了、路径已知、但技能目录还没建」的 Agent 创建它的技能目录。
///
/// # 为什么需要它
///
/// 有些 Agent 的技能目录是**按需创建**的。本机的 Claude Code 就是：
/// `~/.claude` 在、`~/.claude/skills` 不在。这类 Agent 的检测状态是
/// `InstalledNoSkillDir`——**路径是已知的**（`detect()` 会把候选路径回传），
/// 只是目录还没建出来。
///
/// 而矩阵与启用路径都只认 `Detected`，于是在目录出现之前，这个 Agent
/// **永远不在可启用的目标列表里**。用户点了"启用"，只有别的 Agent 生效，
/// 界面也不说为什么——他实际撞到过这个死角。
///
/// # 两条红线
///
/// - **没安装的 Agent 绝不建**：那会在用户机器上凭空多出一堆属于他根本没装的
///   软件的目录（`~/.cursor/skills`、`~/.codeium/windsurf/skills`……）。
/// - **路径未知的绝不猜**：Trae 的候选路径当初实测证伪、故意留空，
///   其技能目录必须由用户在设置页指定，不能替它编一个。
///
/// 返回重新检测后的完整结果，调用方直接拿新状态渲染即可。
#[tauri::command(async)]
pub fn agent_create_skill_dir(agent_id: String) -> AppResult<Vec<AgentDetection>> {
    let detections = agents_detect()?;
    let target = detections
        .iter()
        .find(|d| d.id == agent_id)
        .ok_or_else(|| AppError::NotFound(format!("未知的 Agent：{agent_id}")))?;

    if target.status != agents::AgentStatus::InstalledNoSkillDir {
        return Err(AppError::Config(format!(
            "{} 当前不需要创建技能目录（状态：{:?}）",
            target.display_name, target.status
        )));
    }

    let dir = target.skill_dir.as_deref().ok_or_else(|| {
        AppError::Config(format!(
            "{} 的技能目录尚未探明，请在设置页手动指定一个位置。",
            target.display_name
        ))
    })?;

    std::fs::create_dir_all(dir)
        .map_err(|err| AppError::from_io(&format!("创建技能目录失败：{dir}"), err))?;

    tracing::info!(agent = %agent_id, dir = %dir, "按用户要求创建了该 Agent 的技能目录");

    agents_detect()
}

/// 刷新中央库索引后再扫描。
///
/// # 为什么必须先刷新索引
///
/// `library_init` 只创建目录骨架、**不建索引**，而扫描是从索引读中央库的。
/// 若用户刚设置完中央库就直接看列表，索引为空会导致**中央库里的 Skill
/// 一个都不显示**，看上去像功能坏了。让索引依赖用户手动点「重建索引」
/// 是不合理的设计——那不是用户该操心的内部细节。
///
/// 代价可接受：1000 个 Skill 全量重建约 166ms（见 `docs/PERF.md`），
/// 且前端对扫描结果有缓存，不会每次交互都触发。
///
/// 索引刷新失败**不阻断扫描**：Agent 侧的数据仍然有价值，
/// 此时沿用现有索引并在日志中告警。
pub(crate) fn scan_with_root(
    detections: &[AgentDetection],
    central_root: Option<&Path>,
) -> AppResult<ScanReport> {
    if let Some(root) = central_root {
        if root.join(crate::library::SKILLS_DIR).is_dir() {
            match crate::index::rebuild(root) {
                Ok(report) => tracing::debug!(
                    indexed = report.skill_count,
                    duration_ms = report.duration_ms,
                    "扫描前已刷新中央库索引"
                ),
                Err(err) => {
                    tracing::warn!(error = %err, "刷新中央库索引失败，将使用现有索引")
                }
            }
        }
    }

    scanner::scan(detections, central_root)
}

/// 扫描全部已检测到的 Agent，与中央库归并成统一清单。
///
/// 中央库路径取自配置；未配置时只扫描 Agent 目录，不报错——
/// "还没设置中央库"是正常的初始状态，不是错误。
#[tauri::command(async)]
pub fn skills_scan(library_path: Option<String>) -> AppResult<ScanReport> {
    let central_root: Option<PathBuf> = match library_path {
        Some(p) if !p.trim().is_empty() => Some(PathBuf::from(p)),
        _ => config::load()?.central_library().map(PathBuf::from),
    };

    let detections = agents_detect()?;
    let report = scan_with_root(&detections, central_root.as_deref())?;

    tracing::info!(
        skills = report.skills.len(),
        scanned_dirs = report.scanned_dirs,
        skipped = report.skipped.len(),
        duration_ms = report.duration_ms,
        "扫描完成"
    );

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library;

    fn write_skill(root: &Path, name: &str) {
        let dir = library::skills_dir(root).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(library::SKILL_MANIFEST),
            format!("---\nname: {name}\ndescription: 测试用\n---\n正文\n"),
        )
        .unwrap();
    }

    /// 回归：刚初始化中央库（索引还是空的）就直接扫描时，
    /// 中央库里的 Skill 必须出现在结果里。
    ///
    /// 修复前的行为：扫描从索引读中央库，索引为空 → 中央库的 Skill
    /// 一个都不显示，用户会以为功能坏了，得先手动点「重建索引」才对。
    #[test]
    fn scan_refreshes_index_so_fresh_library_skills_appear() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("library");
        library::initialize(&root.display().to_string()).unwrap();
        write_skill(&root, "alpha");
        write_skill(&root, "beta");

        // 前置断言：此时索引确实是空的（initialize 不建索引）
        assert_eq!(crate::index::count(&root).unwrap(), 0);

        // 没有任何 Agent，只靠中央库
        let report = scan_with_root(&[], Some(&root)).unwrap();

        let names: Vec<&str> = report.skills.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"alpha") && names.contains(&"beta"),
            "中央库的 Skill 未出现在扫描结果中：{names:?}"
        );
        assert!(report.skills.iter().all(|s| s.in_central_library));
    }

    /// 索引刷新失败不应阻断扫描
    #[test]
    fn scan_still_works_when_central_root_is_not_initialized() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("not-a-library");
        std::fs::create_dir_all(&root).unwrap();

        let report = scan_with_root(&[], Some(&root)).unwrap();
        assert!(report.skills.is_empty());
    }

    #[test]
    fn scan_without_central_root_is_allowed() {
        let report = scan_with_root(&[], None).unwrap();
        assert!(report.skills.is_empty());
    }
}
