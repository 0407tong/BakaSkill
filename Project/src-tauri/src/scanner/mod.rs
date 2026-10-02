//! 扫描 Agent 技能目录，并把结果归并成统一清单。
//!
//! # 三条来自实测的硬约束（见 `docs/AGENT_PATHS_MEASURED.md`）
//!
//! 1. **必须支持嵌套分组**。Codex 的技能位于 `skills/.system/<技能名>/SKILL.md`，
//!    只扫一层会把它们全部漏掉。深度由注册表的 `max_scan_depth` 决定。
//! 2. **不跟随重解析点**。否则已纳管的 Skill 会被按 Agent 数量重复计入，
//!    表现为"Skill 数量暴涨"。
//! 3. **主键必须规范化**。Windows 文件名大小写不敏感，`PDF-Tools` 与 `pdf-tools`
//!    必须归并为同一个 Skill，否则跨平台行为会不一致。

use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde::Serialize;

use crate::agents::{AgentDetection, AgentStatus};
use crate::error::AppResult;
use crate::index;
use crate::platform::link;
use crate::skill::SkillDocument;

/// 一个 Skill 在某个 Agent 中的存在形态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstanceKind {
    /// 是 junction 且目标位于中央库内 —— 已纳管
    Managed,
    /// 普通目录，中央库中无同名 Skill
    External,
    /// 普通目录，但中央库中已有同名 Skill —— 冲突
    ExternalDuplicate,
    /// 是 junction 但目标不可达 —— 断链
    Dangling,
    /// 是 junction 但目标不在中央库内 —— 外部链接，不代管
    ForeignLink,
}

/// 扫描到的单个实例
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedInstance {
    pub agent_id: String,
    pub agent_name: String,
    /// 规范化后的 Skill 标识，用于跨 Agent 归并
    pub skill_id: String,
    pub dir_name: String,
    pub path: String,
    /// 嵌套分组名（Codex 的 `.system` 等）
    pub group: Option<String>,
    /// 分组是否为该 Agent 的内置资源
    pub is_system_group: bool,
    pub kind: InstanceKind,
    pub link_target: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub mtime: Option<i64>,
    pub size: Option<i64>,
    /// 解析失败时的原因（此时 name 回退为目录名）
    pub parse_error: Option<String>,
}

/// 统一清单中的一个 Skill（可能来自多个 Agent）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    /// 用于建立链接的目录名。
    ///
    /// 优先取中央库中的实际目录名——链接名必须与中央库内的目录名一致，
    /// 否则链接会指向不存在的路径。仅在中央库中没有该 Skill 时
    /// 才回退到某个 Agent 侧的目录名。
    pub dir_name: String,
    /// 是否存在于中央库
    pub in_central_library: bool,
    pub central_path: Option<String>,
    /// 各 Agent 中的实例
    pub instances: Vec<ScannedInstance>,
    /// `instances` 中 kind == Managed 的数量
    pub managed_count: usize,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub skills: Vec<SkillSummary>,
    pub agents: Vec<AgentDetection>,
    pub scanned_dirs: usize,
    pub duration_ms: u64,
    /// 扫描过程中跳过的条目及原因，不静默忽略
    pub skipped: Vec<String>,
}

/// 扫描全部已检测到的 Agent，并与中央库归并成统一清单。
pub fn scan(agents: &[AgentDetection], central_root: Option<&Path>) -> AppResult<ScanReport> {
    let started = std::time::Instant::now();

    // 中央库中已有的 Skill 标识，用于判定 Managed / ExternalDuplicate
    let central_skills: Vec<index::SkillRecord> = match central_root {
        Some(root) if root.join(crate::library::SKILLS_DIR).is_dir() => {
            index::list_all(root).unwrap_or_default()
        }
        _ => Vec::new(),
    };

    let central_root_norm = central_root.map(normalize_path);

    // 并行扫描各 Agent
    let active: Vec<&AgentDetection> = agents
        .iter()
        .filter(|a| a.status == AgentStatus::Detected)
        .collect();

    let per_agent: Vec<(Vec<ScannedInstance>, Vec<String>, usize)> = active
        .par_iter()
        .map(|agent| scan_one_agent(agent, central_root_norm.as_deref()))
        .collect();

    let mut instances: Vec<ScannedInstance> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut scanned_dirs = 0usize;
    for (mut found, mut skips, dirs) in per_agent {
        instances.append(&mut found);
        skipped.append(&mut skips);
        scanned_dirs += dirs;
    }

    let skills = merge(instances, &central_skills);

    Ok(ScanReport {
        skills,
        agents: agents.to_vec(),
        scanned_dirs,
        duration_ms: started.elapsed().as_millis() as u64,
        skipped,
    })
}

/// 扫描单个 Agent 的技能目录
fn scan_one_agent(
    agent: &AgentDetection,
    central_root: Option<&str>,
) -> (Vec<ScannedInstance>, Vec<String>, usize) {
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    let mut scanned = 0usize;

    let Some(skill_dir) = agent.skill_dir.as_deref() else {
        return (found, skipped, scanned);
    };
    let root = PathBuf::from(skill_dir);
    if !root.is_dir() {
        return (found, skipped, scanned);
    }

    let Ok(entries) = std::fs::read_dir(&root) else {
        skipped.push(format!("{skill_dir}：无法读取目录"));
        return (found, skipped, scanned);
    };

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let dir_name = entry.file_name().to_string_lossy().to_string();

        // 跳过重解析点：跟随它会把中央库内容当成 Agent 本地内容重复计数
        if link::is_junction(&path) {
            if let Some(instance) =
                classify_link(agent, &path, &dir_name, None, false, central_root)
            {
                found.push(instance);
            }
            scanned += 1;
            continue;
        }

        if !path.is_dir() {
            continue;
        }
        scanned += 1;

        // 深度 1：目录本身就是技能
        if has_manifest(&path) {
            if let Some(instance) = read_skill(agent, &path, &dir_name, None, false, &mut skipped) {
                found.push(instance);
            }
            continue;
        }

        // 深度 2：把该目录当作分组，再向下一层找技能。
        // Codex 的 `skills/.system/<技能名>/SKILL.md` 走这条路径，
        // 不下探这一层会漏掉它的全部内置技能（实测结论见 AGENT_PATHS_MEASURED.md）。
        // 下探固定为一层，深度有界，不会在异常目录结构上失控。
        let is_system = dir_name.starts_with('.');

        let Ok(inner) = std::fs::read_dir(&path) else {
            continue;
        };
        for child in inner.filter_map(Result::ok) {
            let child_path = child.path();
            // 同样不跟随重解析点
            if !child_path.is_dir() || link::is_junction(&child_path) {
                continue;
            }
            let child_name = child.file_name().to_string_lossy().to_string();
            scanned += 1;
            if has_manifest(&child_path) {
                if let Some(instance) = read_skill(
                    agent,
                    &child_path,
                    &child_name,
                    Some(dir_name.clone()),
                    is_system,
                    &mut skipped,
                ) {
                    found.push(instance);
                }
            }
        }
    }

    (found, skipped, scanned)
}

/// 读取一个技能目录的元数据
fn read_skill(
    agent: &AgentDetection,
    path: &Path,
    dir_name: &str,
    group: Option<String>,
    is_system_group: bool,
    skipped: &mut Vec<String>,
) -> Option<ScannedInstance> {
    let manifest = find_manifest(path)?;
    let (name, description, tags, parse_error) = match std::fs::read_to_string(&manifest) {
        Ok(raw) => match SkillDocument::parse(&raw) {
            Ok(doc) => (
                doc.name().unwrap_or(dir_name).to_string(),
                doc.description().map(str::to_string),
                doc.tags(),
                None,
            ),
            Err(err) => (
                dir_name.to_string(),
                None,
                Vec::new(),
                Some(err.to_string()),
            ),
        },
        Err(err) => {
            skipped.push(format!("{}：读取 SKILL.md 失败（{err}）", path.display()));
            (
                dir_name.to_string(),
                None,
                Vec::new(),
                Some(err.to_string()),
            )
        }
    };

    let metadata = std::fs::metadata(&manifest).ok();

    Some(ScannedInstance {
        agent_id: agent.id.clone(),
        agent_name: agent.display_name.clone(),
        skill_id: normalize_id(&name),
        dir_name: dir_name.to_string(),
        path: path.display().to_string(),
        group,
        is_system_group,
        kind: InstanceKind::External,
        link_target: None,
        name,
        description,
        tags,
        mtime: metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .map(to_millis),
        size: metadata.as_ref().map(|m| m.len() as i64),
        parse_error,
    })
}

/// 为链接类条目构造实例记录
fn classify_link(
    agent: &AgentDetection,
    path: &Path,
    dir_name: &str,
    group: Option<String>,
    is_system_group: bool,
    central_root: Option<&str>,
) -> Option<ScannedInstance> {
    let target = link::junction_target(path).ok().flatten();
    let target_str = target.as_ref().map(|t| t.display().to_string());

    let inside_central = match (&target, central_root) {
        (Some(t), Some(root)) => normalize_path(t).starts_with(root),
        _ => false,
    };
    let target_exists = target.as_ref().map(|t| t.exists()).unwrap_or(false);

    let kind = if !inside_central {
        InstanceKind::ForeignLink
    } else if !target_exists {
        InstanceKind::Dangling
    } else {
        InstanceKind::Managed
    };

    // 链接目标可达时，从目标里读元数据；否则仅用目录名占位
    let (name, description, tags) = match target.as_ref().filter(|t| t.exists()) {
        Some(real) => match find_manifest(real).and_then(|m| std::fs::read_to_string(m).ok()) {
            Some(raw) => match SkillDocument::parse(&raw) {
                Ok(doc) => (
                    doc.name().unwrap_or(dir_name).to_string(),
                    doc.description().map(str::to_string),
                    doc.tags(),
                ),
                Err(_) => (dir_name.to_string(), None, Vec::new()),
            },
            None => (dir_name.to_string(), None, Vec::new()),
        },
        None => (dir_name.to_string(), None, Vec::new()),
    };

    Some(ScannedInstance {
        agent_id: agent.id.clone(),
        agent_name: agent.display_name.clone(),
        skill_id: normalize_id(&name),
        dir_name: dir_name.to_string(),
        path: path.display().to_string(),
        group,
        is_system_group,
        kind,
        link_target: target_str,
        name,
        description,
        tags,
        mtime: None,
        size: None,
        parse_error: None,
    })
}

/// 归并：中央库 Skill 与各 Agent 实例合成统一清单
fn merge(
    instances: Vec<ScannedInstance>,
    central_skills: &[index::SkillRecord],
) -> Vec<SkillSummary> {
    use std::collections::HashMap;

    let mut map: HashMap<String, SkillSummary> = HashMap::new();

    // 先放中央库的，保证即使没有任何 Agent 也出现在清单里
    for record in central_skills {
        map.insert(
            record.id.clone(),
            SkillSummary {
                id: record.id.clone(),
                name: record.name.clone(),
                description: record.description.clone(),
                tags: record.tags.clone(),
                dir_name: PathBuf::from(&record.dir_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| record.id.clone()),
                in_central_library: true,
                central_path: Some(record.dir_path.clone()),
                instances: Vec::new(),
                managed_count: 0,
                updated_at: record.mtime,
            },
        );
    }

    for instance in instances {
        let entry = map
            .entry(instance.skill_id.clone())
            .or_insert_with(|| SkillSummary {
                id: instance.skill_id.clone(),
                name: instance.name.clone(),
                description: instance.description.clone(),
                tags: instance.tags.clone(),
                dir_name: instance.dir_name.clone(),
                in_central_library: false,
                central_path: None,
                instances: Vec::new(),
                managed_count: 0,
                updated_at: None,
            });

        if entry.description.is_none() {
            entry.description = instance.description.clone();
        }
        for tag in &instance.tags {
            if !entry.tags.contains(tag) {
                entry.tags.push(tag.clone());
            }
        }
        if instance.kind == InstanceKind::Managed {
            entry.managed_count += 1;
        }
        entry.updated_at = entry.updated_at.max(instance.mtime);
        entry.instances.push(instance);
    }

    let mut list: Vec<SkillSummary> = map.into_values().collect();
    // 稳定排序，避免每次扫描顺序抖动导致界面跳动
    list.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    list
}

/// 与 `index::rebuild` 保持一致的标识规范化规则
fn normalize_id(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_dash = true;
    for ch in raw.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "unnamed".to_string()
    } else {
        trimmed
    }
}

fn normalize_path(path: &Path) -> String {
    dunce::simplified(path)
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
}

fn has_manifest(dir: &Path) -> bool {
    find_manifest(dir).is_some()
}

fn find_manifest(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.filter_map(Result::ok) {
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(crate::library::SKILL_MANIFEST)
        {
            return Some(entry.path());
        }
    }
    None
}

fn to_millis(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{AgentDetection, AgentStatus};

    fn detection(id: &str, dir: &Path) -> AgentDetection {
        AgentDetection {
            id: id.to_string(),
            display_name: id.to_string(),
            icon: None,
            status: AgentStatus::Detected,
            skill_dir: Some(dir.display().to_string()),
            detected_by: Vec::new(),
            is_user_override: false,
            notes: None,
            docs_url: None,
        }
    }

    fn write_skill(dir: &Path, name: &str, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), body).unwrap();
        let _ = name;
    }

    #[test]
    fn scans_flat_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");
        write_skill(
            &skills.join("pdf-tools"),
            "pdf-tools",
            "---\nname: pdf-tools\ndescription: PDF\ntags: [pdf]\n---\n",
        );

        let report = scan(&[detection("a", &skills)], None).unwrap();
        assert_eq!(report.skills.len(), 1);
        assert_eq!(report.skills[0].name, "pdf-tools");
        assert_eq!(report.skills[0].tags, vec!["pdf"]);
        assert_eq!(report.skills[0].instances[0].kind, InstanceKind::External);
    }

    /// Codex 的结构：技能嵌套在分组目录里，深度 1 会全部漏掉
    #[test]
    fn scans_nested_group_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");
        write_skill(
            &skills.join(".system").join("imagegen"),
            "imagegen",
            "---\nname: imagegen\ndescription: 生成图片\n---\n",
        );
        write_skill(
            &skills.join(".system").join("review-agent"),
            "review-agent",
            "---\nname: review-agent\n---\n",
        );

        let report = scan(&[detection("codex", &skills)], None).unwrap();
        assert_eq!(report.skills.len(), 2, "嵌套分组的技能被漏掉了");

        let inst = &report.skills[0].instances[0];
        assert_eq!(inst.group.as_deref(), Some(".system"));
        assert!(inst.is_system_group, "内置分组应被标记");
    }

    #[test]
    fn ignores_directories_without_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");
        std::fs::create_dir_all(skills.join("not-a-skill")).unwrap();
        write_skill(&skills.join("real"), "real", "---\nname: real\n---\n");

        let report = scan(&[detection("a", &skills)], None).unwrap();
        assert_eq!(report.skills.len(), 1);
        assert_eq!(report.skills[0].name, "real");
    }

    /// 同一 Skill 出现在多个 Agent 中应归并为一条，而不是多条
    #[test]
    fn merges_same_skill_across_agents() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("agent-a");
        let b = tmp.path().join("agent-b");
        write_skill(
            &a.join("pdf-tools"),
            "pdf-tools",
            "---\nname: pdf-tools\n---\n",
        );
        write_skill(
            &b.join("pdf-tools"),
            "pdf-tools",
            "---\nname: pdf-tools\n---\n",
        );

        let report = scan(&[detection("a", &a), detection("b", &b)], None).unwrap();
        assert_eq!(report.skills.len(), 1, "同名 Skill 未归并");
        assert_eq!(report.skills[0].instances.len(), 2);
    }

    /// 大小写不同必须归并为同一个 Skill
    #[test]
    fn merge_is_case_insensitive() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");
        write_skill(&skills.join("A"), "A", "---\nname: PDF-Tools\n---\n");
        write_skill(&skills.join("B"), "B", "---\nname: pdf tools\n---\n");

        let report = scan(&[detection("a", &skills)], None).unwrap();
        assert_eq!(
            report.skills.len(),
            1,
            "PDF-Tools 与 pdf tools 应归并为同一 Skill"
        );
    }

    #[test]
    fn unparseable_skill_still_listed_with_error() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");
        // 缺少结束分隔符
        write_skill(
            &skills.join("broken"),
            "broken",
            "---\nname: broken\n正文\n",
        );

        let report = scan(&[detection("a", &skills)], None).unwrap();
        assert_eq!(
            report.skills.len(),
            1,
            "解析失败的技能也应出现，而不是静默消失"
        );
        assert!(report.skills[0].instances[0].parse_error.is_some());
    }

    #[test]
    fn not_installed_agents_are_not_scanned() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");
        write_skill(&skills.join("x"), "x", "---\nname: x\n---\n");

        let mut d = detection("a", &skills);
        d.status = AgentStatus::NotInstalled;

        let report = scan(&[d], None).unwrap();
        assert!(report.skills.is_empty());
    }

    #[test]
    fn empty_skill_dir_yields_empty_report() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");
        std::fs::create_dir_all(&skills).unwrap();

        let report = scan(&[detection("a", &skills)], None).unwrap();
        assert!(report.skills.is_empty());
        assert!(report.skipped.is_empty());
    }
}
