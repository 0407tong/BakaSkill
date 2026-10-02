//! 链接启停与映射矩阵命令。

use std::path::PathBuf;

use serde::Serialize;

use crate::agents::{self, AgentDetection, AgentStatus};
use crate::config;
use crate::error::{AppError, AppResult};
use crate::links::{self, AdoptResult, ConflictPolicy, LinkCell, LinkResult, LinkState};
use crate::scanner;

/// 主机名/路径快照，供映射矩阵渲染表头
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixAgent {
    pub id: String,
    pub display_name: String,
    pub skill_dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixRow {
    pub skill_id: String,
    pub name: String,
    pub dir_name: String,
    pub in_central_library: bool,
    pub cells: Vec<LinkCell>,
}

/// 链接映射矩阵 —— **链接透明化的数据源**。
///
/// 与 `skills_scan` 的区别：扫描给出的是"磁盘上实际存在什么"，
/// 矩阵给出的是"每个 Skill × 每个 Agent 的预期位置当前处于什么状态"，
/// 包含尚未建立链接的空格。后者才是用户排查"为什么某个 Agent 里没有这个 Skill"
/// 时需要的视图。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkMatrix {
    /// 中央库根路径（绝对路径），页面上必须明确展示（链接透明化要求）
    pub central_library_path: Option<String>,
    pub agents: Vec<MatrixAgent>,
    pub rows: Vec<MatrixRow>,
    /// 各状态的单元格计数，用于顶部的概览与"问题优先"排序
    pub summary: MatrixSummary,
    pub generated_at: i64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MatrixSummary {
    pub valid: usize,
    pub absent: usize,
    pub dangling: usize,
    pub conflict: usize,
    pub foreign_link: usize,
    pub foreign_symlink: usize,
}

/// 只有还存在于中央库或某个 Agent 中的 Skill 才值得出现在矩阵里
fn resolve_central_root() -> AppResult<Option<PathBuf>> {
    Ok(config::load()?.central_library().map(PathBuf::from))
}

#[tauri::command]
pub fn link_matrix() -> AppResult<LinkMatrix> {
    let central_root = resolve_central_root()?;

    let cfg = config::load()?;
    let detections: Vec<AgentDetection> = agents::registry()?
        .iter()
        .map(|d| {
            let override_dir = cfg
                .agents
                .iter()
                .find(|a| a.id == d.id)
                .and_then(|a| a.skill_dir.as_deref());
            agents::detect(d, override_dir)
        })
        .collect();

    // 矩阵只列已检测到技能目录的 Agent
    let active: Vec<&AgentDetection> = detections
        .iter()
        .filter(|a| a.status == AgentStatus::Detected && a.skill_dir.is_some())
        .collect();

    let scan = scanner::scan(&detections, central_root.as_deref())?;

    let Some(root) = central_root.as_deref() else {
        return Ok(LinkMatrix {
            central_library_path: None,
            agents: Vec::new(),
            rows: Vec::new(),
            summary: MatrixSummary::default(),
            generated_at: now_millis(),
        });
    };

    let mut summary = MatrixSummary::default();
    let mut rows = Vec::new();

    for skill in &scan.skills {
        // 用于建链接的目录名：优先取中央库中的实际目录名，否则取某个实例的目录名
        let dir_name = skill
            .central_path
            .as_deref()
            .and_then(|p| {
                PathBuf::from(p)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
            })
            .or_else(|| skill.instances.first().map(|i| i.dir_name.clone()))
            .unwrap_or_else(|| skill.id.clone());

        let mut cells = Vec::new();
        for agent in &active {
            let Some(skill_dir) = agent.skill_dir.as_deref() else {
                continue;
            };

            let cell = links::inspect(
                root,
                &agent.id,
                &PathBuf::from(skill_dir),
                &skill.id,
                &dir_name,
            );

            match cell.state {
                LinkState::Valid => summary.valid += 1,
                LinkState::Absent => summary.absent += 1,
                LinkState::Dangling => summary.dangling += 1,
                LinkState::Conflict => summary.conflict += 1,
                LinkState::ForeignLink => summary.foreign_link += 1,
                LinkState::ForeignSymlink => summary.foreign_symlink += 1,
            }
            cells.push(cell);
        }

        rows.push(MatrixRow {
            skill_id: skill.id.clone(),
            name: skill.name.clone(),
            dir_name,
            in_central_library: skill.in_central_library,
            cells,
        });
    }

    // 把这次校验的结果落盘。矩阵本身就是"校验"操作，
    // 顺手记录状态与时间，供后续诊断"某状态是什么时候看到的"。
    let all_cells: Vec<LinkCell> = rows.iter().flat_map(|r| r.cells.clone()).collect();
    let persisted = links::persist_states(root, &all_cells);
    tracing::debug!(persisted, "链接状态已落盘");

    Ok(LinkMatrix {
        central_library_path: Some(root.display().to_string()),
        agents: active
            .iter()
            .map(|a| MatrixAgent {
                id: a.id.clone(),
                display_name: a.display_name.clone(),
                skill_dir: a.skill_dir.clone().unwrap_or_default(),
            })
            .collect(),
        rows,
        summary,
        generated_at: now_millis(),
    })
}

/// 启用/禁用一个 Skill 在指定 Agent 上的链接。
///
/// `agent_ids` 为空表示对**所有已检测到的 Agent** 操作——
/// 这对应 UI 上卡片的三态主开关。
///
/// **逐条独立结果，部分失败不回滚已成功项**：回滚本身也是文件系统操作，
/// 有可能再失败一次，把状态弄得更乱。失败项会在结果里明确标出。
#[tauri::command]
pub fn link_set_enabled(
    skill_dir_name: String,
    agent_ids: Vec<String>,
    enabled: bool,
    policy: Option<ConflictPolicy>,
) -> AppResult<Vec<LinkResult>> {
    let Some(root) = resolve_central_root()? else {
        return Err(AppError::Config("尚未配置中央库路径".to_string()));
    };

    let cfg = config::load()?;
    let detections: Vec<AgentDetection> = agents::registry()?
        .iter()
        .map(|d| {
            let override_dir = cfg
                .agents
                .iter()
                .find(|a| a.id == d.id)
                .and_then(|a| a.skill_dir.as_deref());
            agents::detect(d, override_dir)
        })
        .collect();

    let policy = policy.unwrap_or_default();
    let mut results = Vec::new();

    for detection in &detections {
        if detection.status != AgentStatus::Detected {
            continue;
        }
        if !agent_ids.is_empty() && !agent_ids.contains(&detection.id) {
            continue;
        }
        let Some(skill_dir) = detection.skill_dir.as_deref() else {
            continue;
        };

        let result = if enabled {
            links::enable(
                &root,
                &detection.id,
                &PathBuf::from(skill_dir),
                &skill_dir_name,
                &skill_dir_name,
                policy,
            )
        } else {
            links::disable(
                &root,
                &detection.id,
                &PathBuf::from(skill_dir),
                &skill_dir_name,
                &skill_dir_name,
            )
        };

        tracing::info!(
            agent = %detection.id,
            skill = %skill_dir_name,
            enabled,
            action = ?result.action,
            "链接操作"
        );
        results.push(result);
    }

    Ok(results)
}

/// 把 Agent 目录中的一个真实 Skill 目录纳入中央库，
/// 并在原位置建立指向它的链接。
///
/// 这是用户把**已有** Skill 收进中央库的路径。在此之前，
/// 中央库只能靠手工拷贝文件来填充。
#[tauri::command]
pub fn skill_adopt(agent_id: String, dir_name: String) -> AppResult<AdoptResult> {
    let Some(root) = resolve_central_root()? else {
        return Err(AppError::Config("尚未配置中央库路径".to_string()));
    };

    let cfg = config::load()?;
    let detection = agents::registry()?
        .into_iter()
        .find(|d| d.id == agent_id)
        .ok_or_else(|| AppError::NotFound(format!("未知的 Agent：{agent_id}")))?;

    let override_dir = cfg
        .agents
        .iter()
        .find(|a| a.id == agent_id)
        .and_then(|a| a.skill_dir.as_deref());

    let detected = agents::detect(&detection, override_dir);
    let Some(skill_dir) = detected.skill_dir else {
        return Err(AppError::Config(format!(
            "{} 尚未确定技能目录，请先在设置中指定。",
            detection.display_name
        )));
    };

    links::adopt(&root, &agent_id, &PathBuf::from(skill_dir), &dir_name)
}

/// 迁移中央库到新位置，并重写所有指向它的链接。
///
/// **执行顺序是安全关键**：先复制数据 → 再重写链接 → 最后才删旧数据。
/// 详见 `links::relocate` 的文档。
#[tauri::command]
pub fn library_relocate(
    new_path: String,
    move_data: bool,
) -> AppResult<crate::links::RelocateResult> {
    let cfg = config::load()?;
    let old_path = cfg
        .central_library()
        .ok_or_else(|| AppError::Config("尚未配置中央库路径".to_string()))?
        .to_string();

    // 收集所有已检测 Agent 的技能目录，它们是链接可能存在的地方
    let detections = crate::commands::agents::agents_detect()?;
    let agent_dirs: Vec<PathBuf> = detections
        .iter()
        .filter(|d| d.status == AgentStatus::Detected)
        .filter_map(|d| d.skill_dir.as_deref().map(PathBuf::from))
        .collect();

    let result = links::relocate(
        &PathBuf::from(&old_path),
        &PathBuf::from(&new_path),
        &agent_dirs,
        move_data,
    )?;

    // 链接重写全部成功才更新配置；有失败项则保持旧配置，
    // 让用户能看懂"当前到底在哪"，并重试。
    if result.failed.is_empty() {
        let mut updated = cfg.clone();
        updated.central_library_path = Some(new_path);
        config::save(&updated)?;
        tracing::info!("中央库配置已更新");
    } else {
        tracing::warn!(
            failed = result.failed.len(),
            "存在未完成的链接，配置未更新，旧数据已保留"
        );
    }

    Ok(result)
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
