//! Agent 注册表与检测。
//!
//! # 为什么注册表是数据而非代码
//!
//! 各 Agent 的技能目录约定不稳定、且各家文档常滞后于实现。把注册表做成内嵌的
//! JSON 数据文件意味着：新增 Agent、修正路径都不需要改代码，用户也可以覆盖。
//!
//! # 注册表的路径来自实测，不是猜测
//!
//! 每条 `notes` 都注明该路径是**实测确认**、**实测证伪**还是**未实测**。
//! 实测报告见 `docs/AGENT_PATHS_MEASURED.md`。**不要把未验证的路径写成确定事实。**

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// 内嵌的注册表数据
const REGISTRY_JSON: &str = include_str!("agents.json");

/// 检测规则
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DetectionRule {
    /// 给定路径中任意一个存在即视为已安装
    AnyExists { paths: Vec<String> },
    /// 给定命令中任意一个在 PATH 上即视为已安装
    ExeOnPath { commands: Vec<String> },
}

/// 一个 Agent 的注册信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDescriptor {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub icon: Option<String>,
    /// 候选技能目录。**按顺序探测，取第一个存在的**。
    /// 为空表示该 Agent 的用户技能目录尚未探明（如 Trae）。
    #[serde(default)]
    pub skill_dir_candidates: Vec<String>,
    /// 检测规则。**多条规则之间是「或」的关系**——
    /// 因为有的 Agent 装了 CLI 却没有配置目录（Claude Code 本机就是如此），
    /// 只靠一种判据会把它误判为未安装。
    #[serde(default)]
    pub detection: Vec<DetectionRule>,
    /// 技能目录下的最大扫描深度。
    /// 1 = `skills/<技能名>/SKILL.md`；2 = 额外支持 `skills/<分组>/<技能名>/SKILL.md`。
    #[serde(default = "default_depth")]
    pub max_scan_depth: u8,
    /// 分组名以此前缀开头时视为该 Agent 的内置资源（如 Codex 的 `.system`）
    #[serde(default)]
    pub system_group_prefix: Option<String>,
    /// 实测备注，会展示在 UI 的 Agent 详情中
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub docs_url: Option<String>,
}

fn default_depth() -> u8 {
    1
}

/// Agent 的检测状态。
///
/// **刻意不是布尔值**：实测发现 Claude Code 会出现"CLI 已装但技能目录不存在"
/// 的中间状态，用布尔表示会把已安装的 Agent 判成未安装。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentStatus {
    /// 根目录与命令都不存在 —— UI 隐藏
    NotInstalled,
    /// Agent 已安装，但技能目录尚未创建 —— UI 显示并提示用户指定
    InstalledNoSkillDir,
    /// 技能目录已确定 —— 正常扫描
    Detected,
}

/// 一个 Agent 的检测结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDetection {
    pub id: String,
    pub display_name: String,
    pub icon: Option<String>,
    pub status: AgentStatus,
    /// 最终确定的技能目录（`Detected` 时非空）
    pub skill_dir: Option<String>,
    /// 命中的检测依据，用于向用户解释"为什么认为它装了"
    pub detected_by: Vec<String>,
    /// 是否是用户在设置里手动指定的路径
    pub is_user_override: bool,
    pub notes: Option<String>,
    pub docs_url: Option<String>,
}

/// 解析内嵌注册表
pub fn registry() -> AppResult<Vec<AgentDescriptor>> {
    serde_json::from_str(REGISTRY_JSON)
        .map_err(|err| AppError::Config(format!("内置 Agent 注册表解析失败：{err}")))
}

/// 展开路径中的环境变量占位符。
///
/// 支持 `%VAR%`（Windows 习惯）与 `%USERPROFILE%` 等常见变量。
/// 变量不存在时返回 `None`，由调用方跳过该候选——
/// **不要把未展开的路径当成有效路径**，那会产生形如
/// `%USERPROFILE%\.cursor\skills` 的"合法但永不存在的"目录。
pub fn expand_vars(raw: &str) -> Option<PathBuf> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;

    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            // 落单的 % 视为普通字符
            out.push_str(&rest[start..]);
            return Some(PathBuf::from(out));
        };
        let name = &after[..end];
        let value = std::env::var(name).ok()?;
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);

    Some(PathBuf::from(out))
}

/// 检测单个 Agent。
///
/// `user_override` 是用户在设置页手动指定的技能目录，优先级最高。
pub fn detect(descriptor: &AgentDescriptor, user_override: Option<&str>) -> AgentDetection {
    let mut detected_by = Vec::new();

    // 1) 用户手动指定优先
    if let Some(dir) = user_override.map(str::trim).filter(|s| !s.is_empty()) {
        let path = PathBuf::from(dir);
        detected_by.push("用户在设置中指定".to_string());
        let status = if path.is_dir() {
            AgentStatus::Detected
        } else {
            AgentStatus::InstalledNoSkillDir
        };
        return AgentDetection {
            id: descriptor.id.clone(),
            display_name: descriptor.display_name.clone(),
            icon: descriptor.icon.clone(),
            status,
            skill_dir: Some(dir.to_string()),
            detected_by,
            is_user_override: true,
            notes: descriptor.notes.clone(),
            docs_url: descriptor.docs_url.clone(),
        };
    }

    // 2) 判定 Agent 本身是否安装
    let mut installed = false;
    for rule in &descriptor.detection {
        match rule {
            DetectionRule::AnyExists { paths } => {
                for raw in paths {
                    if let Some(p) = expand_vars(raw) {
                        if p.exists() {
                            installed = true;
                            detected_by.push(format!("目录存在：{}", p.display()));
                        }
                    }
                }
            }
            DetectionRule::ExeOnPath { commands } => {
                for cmd in commands {
                    if let Some(found) = find_on_path(cmd) {
                        installed = true;
                        detected_by.push(format!("命令可用：{}", found.display()));
                    }
                }
            }
        }
    }

    if !installed {
        return AgentDetection {
            id: descriptor.id.clone(),
            display_name: descriptor.display_name.clone(),
            icon: descriptor.icon.clone(),
            status: AgentStatus::NotInstalled,
            skill_dir: None,
            detected_by,
            is_user_override: false,
            notes: descriptor.notes.clone(),
            docs_url: descriptor.docs_url.clone(),
        };
    }

    // 3) 在候选中找第一个真实存在的技能目录
    let mut first_candidate: Option<PathBuf> = None;
    for raw in &descriptor.skill_dir_candidates {
        let Some(candidate) = expand_vars(raw) else {
            continue;
        };
        if first_candidate.is_none() {
            first_candidate = Some(candidate.clone());
        }
        if candidate.is_dir() {
            detected_by.push(format!("技能目录存在：{}", candidate.display()));
            return AgentDetection {
                id: descriptor.id.clone(),
                display_name: descriptor.display_name.clone(),
                icon: descriptor.icon.clone(),
                status: AgentStatus::Detected,
                skill_dir: Some(candidate.display().to_string()),
                detected_by,
                is_user_override: false,
                notes: descriptor.notes.clone(),
                docs_url: descriptor.docs_url.clone(),
            };
        }
    }

    // 已安装但没有技能目录：把首个候选路径作为"即将创建的位置"回传，
    // 让用户知道 SkillHub 打算往哪里放东西。
    AgentDetection {
        id: descriptor.id.clone(),
        display_name: descriptor.display_name.clone(),
        icon: descriptor.icon.clone(),
        status: AgentStatus::InstalledNoSkillDir,
        skill_dir: first_candidate.map(|p| p.display().to_string()),
        detected_by,
        is_user_override: false,
        notes: descriptor.notes.clone(),
        docs_url: descriptor.docs_url.clone(),
    }
}

/// 由用户配置合成一个 Agent 描述符。
///
/// 用于内置注册表里没有的 Agent——新出现的工具、内部自研工具，
/// 或注册表路径已被证明不正确的情况。用户只需填写技能目录，
/// 其余字段给出安全默认值。
///
/// **不加检测规则**：既然用户手工指定了目录，就直接以该目录是否存在为准，
/// 不再去猜"装没装"。
pub fn custom_descriptor(id: &str, display_name: Option<&str>, skill_dir: &str) -> AgentDescriptor {
    AgentDescriptor {
        id: id.to_string(),
        display_name: display_name.unwrap_or(id).to_string(),
        icon: None,
        skill_dir_candidates: vec![skill_dir.to_string()],
        detection: Vec::new(),
        max_scan_depth: 2,
        system_group_prefix: Some(".".to_string()),
        notes: Some("用户自定义 Agent（不在内置注册表中）".to_string()),
        docs_url: None,
    }
}

/// 在 PATH 上查找命令（Windows 上还会尝试常见可执行扩展名）
fn find_on_path(command: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let has_ext = Path::new(command).extension().is_some();

    for dir in std::env::split_paths(&path_var) {
        let direct = dir.join(command);
        if direct.is_file() {
            return Some(direct);
        }
        if !has_ext {
            for ext in ["exe", "cmd", "bat"] {
                let candidate = dir.join(format!("{command}.{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(id: &str, candidates: &[&str], rules: Vec<DetectionRule>) -> AgentDescriptor {
        AgentDescriptor {
            id: id.to_string(),
            display_name: id.to_string(),
            icon: None,
            skill_dir_candidates: candidates.iter().map(|s| s.to_string()).collect(),
            detection: rules,
            max_scan_depth: 1,
            system_group_prefix: Some(".".to_string()),
            notes: None,
            docs_url: None,
        }
    }

    #[test]
    fn embedded_registry_parses() {
        let all = registry().expect("内置注册表必须可解析");
        assert!(all.iter().any(|a| a.id == "claude-code"));
        assert!(all.iter().any(|a| a.id == "codex"));
    }

    /// Codex 的技能嵌套在分组目录里，深度必须为 2
    #[test]
    fn codex_declares_depth_two() {
        let codex = registry()
            .unwrap()
            .into_iter()
            .find(|a| a.id == "codex")
            .unwrap();
        assert_eq!(
            codex.max_scan_depth, 2,
            "Codex 的技能在 skills/<分组>/<技能名>/，深度 1 会全部漏掉"
        );
    }

    /// Trae 的用户技能目录尚未探明，不能给出未经证实的候选路径
    #[test]
    fn trae_has_no_unverified_candidate_paths() {
        let trae = registry()
            .unwrap()
            .into_iter()
            .find(|a| a.id == "trae")
            .unwrap();
        assert!(
            trae.skill_dir_candidates.is_empty(),
            "实测已证伪 ~/.trae/skills，不得保留为候选"
        );
    }

    #[test]
    fn expands_environment_variables() {
        let expanded = expand_vars(r"%USERPROFILE%\.claude\skills").unwrap();
        assert!(!expanded.to_string_lossy().contains('%'));
        assert!(expanded.to_string_lossy().to_lowercase().contains("skills"));
    }

    #[test]
    fn unknown_variable_yields_none() {
        assert!(expand_vars(r"%DEFINITELY_NOT_SET_XYZ%\foo").is_none());
    }

    #[test]
    fn text_without_variables_passes_through() {
        assert_eq!(
            expand_vars(r"D:\plain\path").unwrap(),
            PathBuf::from(r"D:\plain\path")
        );
    }

    #[test]
    fn detects_not_installed() {
        let d = descriptor(
            "ghost",
            &[r"%USERPROFILE%\.ghost-agent\skills"],
            vec![DetectionRule::AnyExists {
                paths: vec![r"%USERPROFILE%\.ghost-agent".to_string()],
            }],
        );
        assert_eq!(detect(&d, None).status, AgentStatus::NotInstalled);
    }

    /// 关键：Agent 装了但技能目录不存在，不能被判成"未安装"
    #[test]
    fn installed_without_skill_dir_is_a_distinct_state() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("agent-root");
        std::fs::create_dir_all(&root).unwrap();

        let d = descriptor(
            "installed",
            &[&format!(r"{}\skills", root.display())],
            vec![DetectionRule::AnyExists {
                paths: vec![root.display().to_string()],
            }],
        );

        let result = detect(&d, None);
        assert_eq!(result.status, AgentStatus::InstalledNoSkillDir);
        assert!(
            result.skill_dir.is_some(),
            "应回传即将创建的位置，让用户知道 SkillHub 会在哪里放东西"
        );
    }

    #[test]
    fn detects_skill_dir_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("agent-root");
        let skills = root.join("skills");
        std::fs::create_dir_all(&skills).unwrap();

        let d = descriptor(
            "ready",
            &[&skills.display().to_string()],
            vec![DetectionRule::AnyExists {
                paths: vec![root.display().to_string()],
            }],
        );

        let result = detect(&d, None);
        assert_eq!(result.status, AgentStatus::Detected);
        assert_eq!(
            result.skill_dir.as_deref(),
            Some(skills.display().to_string().as_str())
        );
    }

    #[test]
    fn user_override_wins() {
        let dir = tempfile::tempdir().unwrap();
        let custom = dir.path().join("my-custom-skills");
        std::fs::create_dir_all(&custom).unwrap();

        let d = descriptor("any", &[], vec![]);
        let result = detect(&d, Some(&custom.display().to_string()));

        assert!(result.is_user_override);
        assert_eq!(result.status, AgentStatus::Detected);
        assert_eq!(
            result.skill_dir.as_deref(),
            Some(custom.display().to_string().as_str())
        );
    }

    #[test]
    fn user_override_to_missing_dir_is_not_detected() {
        let d = descriptor("any", &[], vec![]);
        let result = detect(&d, Some(r"D:\definitely\not\here"));
        assert_eq!(result.status, AgentStatus::InstalledNoSkillDir);
        assert!(result.is_user_override);
    }

    #[test]
    fn exe_on_path_rule_detects_a_real_command() {
        // 本机必然存在的命令
        let d = descriptor(
            "cmd",
            &[],
            vec![DetectionRule::ExeOnPath {
                commands: vec!["cmd".to_string()],
            }],
        );
        let result = detect(&d, None);
        assert_ne!(result.status, AgentStatus::NotInstalled);
    }

    #[test]
    fn custom_descriptor_detects_user_specified_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("my-agent-skills");
        std::fs::create_dir_all(&skills).unwrap();

        let d = custom_descriptor(
            "my-agent",
            Some("我的 Agent"),
            &skills.display().to_string(),
        );
        let result = detect(&d, Some(&skills.display().to_string()));

        assert_eq!(result.status, AgentStatus::Detected);
        assert_eq!(result.display_name, "我的 Agent");
        assert!(result.is_user_override);
    }

    #[test]
    fn custom_descriptor_falls_back_to_id_as_name() {
        let d = custom_descriptor("unnamed-agent", None, r"D:\x\skills");
        assert_eq!(d.display_name, "unnamed-agent");
    }

    #[test]
    fn finds_path_commands_with_extension_fallback() {
        assert!(find_on_path("cmd").is_some());
        assert!(find_on_path("definitely-not-a-real-command-xyz").is_none());
    }
}
