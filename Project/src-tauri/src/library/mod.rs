//! 中央库：路径诊断、目录骨架初始化、统计。
//!
//! # 中央库结构
//!
//! ```text
//! <central_library>/
//! ├─ skills/
//! │  └─ <skill-id>/
//! │     ├─ SKILL.md
//! │     └─ assets/          （可选）
//! └─ .bakaskill/
//!    └─ index.db             （派生索引，删除后可重建）
//! ```
//!
//! # 为什么路径校验是"隐形重点"
//!
//! junction 的可靠性依赖卷的特性。用户把中央库放到 FAT32 或 OneDrive 目录，
//! 创建链接时不会报错，问题会在之后的某次同步或重启时才暴露——那时排查成本
//! 极高。因此在**选择路径的当下**就把风险讲清楚，是本模块的主要价值。

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::platform::volume::{self, VolumeKind};

/// 中央库中存放 Skill 的子目录名
pub const SKILLS_DIR: &str = "skills";
/// 中央库中存放派生数据的子目录名
pub const META_DIR: &str = ".bakaskill";
/// 改名前的元数据目录名。仅用于把老用户的索引与暂存区搬过来，
/// 见 `config::migrate_legacy_layout`。
pub const LEGACY_META_DIR: &str = ".skillhub";
/// 派生索引文件名
pub const INDEX_DB: &str = "index.db";
/// SKILL.md 的文件名（大小写不敏感匹配）
pub const SKILL_MANIFEST: &str = "SKILL.md";

/// 达到该长度时提示长路径风险。Windows 传统上限是 260 字符，
/// 而实际路径还要再拼上 `<Agent技能目录>\<skill-id>`，因此留足余量。
const LONG_PATH_WARN_THRESHOLD: usize = 160;
/// 剩余空间低于此值时提示
const LOW_SPACE_THRESHOLD_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueSeverity {
    /// 阻断性问题，不允许初始化
    Error,
    /// 不阻断，但必须让用户知情
    Warning,
    /// 中性信息
    Info,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisIssue {
    pub severity: IssueSeverity,
    /// 稳定的机器可读标识，便于前端做定向处理或测试断言
    pub code: String,
    /// 面向用户的中文说明
    pub message: String,
}

impl DiagnosisIssue {
    fn error(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Error,
            code: code.to_string(),
            message: message.into(),
        }
    }
    fn warning(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Warning,
            code: code.to_string(),
            message: message.into(),
        }
    }
    fn info(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Info,
            code: code.to_string(),
            message: message.into(),
        }
    }
}

/// 路径诊断结果。前端据此决定是否放行"初始化中央库"按钮。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathDiagnosis {
    /// 用户输入的原始路径
    pub path: String,
    /// 去掉 `\\?\` 前缀等之后的规范化路径，用于展示与落盘
    pub normalized_path: String,
    pub exists: bool,
    pub is_dir: bool,
    pub writable: bool,
    pub filesystem: Option<String>,
    pub volume_kind: Option<VolumeKind>,
    /// 检测到的云同步目录名（启发式，见 `detect_cloud_sync`）
    pub cloud_sync: Option<String>,
    pub free_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub path_length: usize,
    /// 该路径是否已经是初始化过的中央库
    pub is_initialized: bool,
    /// 库内 Skill 数量（仅当已初始化时有意义）
    pub skill_count: Option<usize>,
    pub issues: Vec<DiagnosisIssue>,
    /// 是否可以执行初始化（无 `Error` 级问题）
    pub can_initialize: bool,
}

impl PathDiagnosis {
    fn has_error(&self) -> bool {
        self.issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Error)
    }
}

/// 诊断一个候选的中央库路径。
///
/// **不修改文件系统**（可写性探测会创建并立即删除一个临时文件）。
pub fn diagnose(input: &str) -> AppResult<PathDiagnosis> {
    let trimmed = input.trim();
    let mut issues = Vec::new();

    if trimmed.is_empty() {
        return Err(AppError::Config("路径为空".to_string()));
    }

    let raw_path = Path::new(trimmed);
    let path = dunce::simplified(raw_path).to_path_buf();
    let normalized = path.display().to_string();
    let path_length = normalized.chars().count();

    let exists = path.exists();
    let is_dir = path.is_dir();

    if exists && !is_dir {
        issues.push(DiagnosisIssue::error(
            "not_a_directory",
            "该路径指向一个文件，中央库必须是文件夹。",
        ));
    }
    if !exists {
        // 允许"尚未创建"——会在初始化时创建。但上级目录必须存在且可写。
        match path.parent() {
            Some(parent) if !parent.exists() => issues.push(DiagnosisIssue::error(
                "parent_missing",
                format!("上级目录不存在：{}", parent.display()),
            )),
            None => issues.push(DiagnosisIssue::error(
                "no_parent",
                "无法确定该路径的上级目录。",
            )),
            _ => issues.push(DiagnosisIssue::info(
                "will_create",
                "该文件夹尚不存在，初始化时会自动创建。",
            )),
        }
    }

    // ---- 卷信息 ----
    let mut volume_kind = None;
    let mut filesystem = None;
    let mut free_bytes = None;
    let mut total_bytes = None;

    match volume::query(&path) {
        Ok(info) => {
            volume_kind = Some(info.kind);
            filesystem = info.filesystem.clone();
            free_bytes = info.free_bytes;
            total_bytes = info.total_bytes;

            match info.kind {
                VolumeKind::Network => issues.push(DiagnosisIssue::error(
                    "network_volume",
                    "中央库不能放在网络位置。junction 对网络路径不可靠，链接会随机失效。",
                )),
                VolumeKind::CdRom | VolumeKind::Unknown => issues.push(DiagnosisIssue::error(
                    "unsupported_volume",
                    "无法确认该卷的类型，出于安全考虑不支持作为中央库。",
                )),
                VolumeKind::Removable => issues.push(DiagnosisIssue::warning(
                    "removable_volume",
                    "这是可移动磁盘。盘符一旦变化，所有 Skill 链接都会失效，需要重新定位。",
                )),
                VolumeKind::Fixed | VolumeKind::RamDisk => {}
            }

            if !info.supports_junction() {
                let fs = info.filesystem.as_deref().unwrap_or("未知");
                issues.push(DiagnosisIssue::error(
                    "unsupported_filesystem",
                    format!("文件系统为 {fs}，不支持目录链接。请选择 NTFS 或 ReFS 卷上的文件夹。"),
                ));
            }

            if let Some(free) = info.free_bytes {
                if free < LOW_SPACE_THRESHOLD_BYTES {
                    issues.push(DiagnosisIssue::warning(
                        "low_space",
                        format!(
                            "剩余空间仅 {}，可能不足以存放较多 Skill。",
                            human_bytes(free)
                        ),
                    ));
                }
            }
        }
        Err(_) => issues.push(DiagnosisIssue::warning(
            "volume_unknown",
            "无法读取该路径所在卷的信息，请确认路径格式正确。",
        )),
    }

    // ---- 云同步目录 ----
    let cloud_sync = detect_cloud_sync(&path);
    if let Some(name) = &cloud_sync {
        issues.push(DiagnosisIssue::warning(
            "cloud_sync",
            format!(
                "{name} 会同步该目录。云同步的占位文件与冲突副本可能破坏链接语义，建议改选本地普通文件夹。"
            ),
        ));
    }

    // ---- 可写性 ----
    let probe_target = if exists && is_dir {
        Some(path.as_path())
    } else {
        path.parent().filter(|p| p.is_dir())
    };
    let writable = match probe_target {
        Some(dir) => probe_writable(dir).unwrap_or(false),
        None => false,
    };
    if !writable {
        issues.push(DiagnosisIssue::error(
            "not_writable",
            "该位置不可写，请检查权限或换一个文件夹。",
        ));
    }

    // ---- 长路径 ----
    if path_length > LONG_PATH_WARN_THRESHOLD {
        issues.push(DiagnosisIssue::warning(
            "long_path",
            format!(
                "路径较长（{path_length} 字符）。再拼上 Agent 技能目录后可能触及 Windows 260 字符上限。"
            ),
        ));
    }

    // ---- 是否已初始化 ----
    let is_initialized = is_dir && path.join(SKILLS_DIR).is_dir();
    let skill_count = if is_initialized {
        Some(count_skills(&path).unwrap_or(0))
    } else {
        None
    };

    let mut diagnosis = PathDiagnosis {
        path: trimmed.to_string(),
        normalized_path: normalized,
        exists,
        is_dir,
        writable,
        filesystem,
        volume_kind,
        cloud_sync,
        free_bytes,
        total_bytes,
        path_length,
        is_initialized,
        skill_count,
        issues,
        can_initialize: true,
    };
    diagnosis.can_initialize = !diagnosis.has_error();
    Ok(diagnosis)
}

/// 创建中央库目录骨架。幂等：重复调用不会破坏已有内容。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub path: String,
    pub created: Vec<String>,
    /// 本次调用前该路径是否已经是初始化过的库
    pub already_initialized: bool,
    pub skill_count: usize,
}

pub fn initialize(input: &str) -> AppResult<InitializeResult> {
    let diagnosis = diagnose(input)?;
    if !diagnosis.can_initialize {
        let first_error = diagnosis
            .issues
            .iter()
            .find(|i| i.severity == IssueSeverity::Error)
            .map(|i| i.message.clone())
            .unwrap_or_else(|| "路径校验未通过".to_string());
        return Err(AppError::Config(first_error));
    }

    let root = PathBuf::from(&diagnosis.normalized_path);
    let already_initialized = root.join(SKILLS_DIR).is_dir();
    let mut created = Vec::new();

    for (dir, label) in [
        (root.clone(), "中央库根目录"),
        (skills_dir(&root), "skills"),
        (meta_dir(&root), META_DIR),
    ] {
        if !dir.exists() {
            std::fs::create_dir_all(&dir).map_err(|err| {
                AppError::from_io(&format!("创建{label}失败：{}", dir.display()), err)
            })?;
            created.push(dir.display().to_string());
        }
    }

    let skill_count = count_skills(&root)?;
    tracing::info!(
        path = %root.display(),
        created = created.len(),
        skill_count,
        "中央库已初始化"
    );

    Ok(InitializeResult {
        path: root.display().to_string(),
        created,
        already_initialized,
        skill_count,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub path: String,
    pub skill_count: usize,
    /// 各 Skill 目录占用字节数之和
    pub total_bytes: u64,
    pub initialized: bool,
}

pub fn stats(input: &str) -> AppResult<LibraryStats> {
    let root = PathBuf::from(dunce::simplified(Path::new(input.trim())));
    let initialized = root.join(SKILLS_DIR).is_dir();
    if !initialized {
        return Ok(LibraryStats {
            path: root.display().to_string(),
            skill_count: 0,
            total_bytes: 0,
            initialized: false,
        });
    }

    let mut skill_count = 0usize;
    let mut total_bytes = 0u64;

    for entry in std::fs::read_dir(skills_dir(&root))
        .map_err(|err| AppError::from_io("读取 skills 目录失败", err))?
        .filter_map(Result::ok)
    {
        let path = entry.path();
        // 不跟随重解析点：中央库内不应有链接，若有也不计入体积，
        // 否则会把 Agent 侧的真实目录重复算进来。
        if crate::platform::link::is_junction(&path) {
            continue;
        }
        if !path.is_dir() || !has_manifest(&path) {
            continue;
        }
        skill_count += 1;
        total_bytes += dir_size(&path);
    }

    Ok(LibraryStats {
        path: root.display().to_string(),
        skill_count,
        total_bytes,
        initialized: true,
    })
}

pub fn skills_dir(root: &Path) -> PathBuf {
    root.join(SKILLS_DIR)
}

pub fn meta_dir(root: &Path) -> PathBuf {
    root.join(META_DIR)
}

pub fn index_db_path(root: &Path) -> PathBuf {
    meta_dir(root).join(INDEX_DB)
}

/// 某个 Skill 目录是否符合中央库的约定（含 SKILL.md）
pub fn has_manifest(skill_dir: &Path) -> bool {
    match std::fs::read_dir(skill_dir) {
        Ok(entries) => entries.filter_map(Result::ok).any(|e| {
            e.file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(SKILL_MANIFEST)
        }),
        Err(_) => false,
    }
}

/// 统计中央库内符合约定的 Skill 数量
pub fn count_skills(root: &Path) -> AppResult<usize> {
    let dir = skills_dir(root);
    if !dir.is_dir() {
        return Ok(0);
    }
    let mut count = 0;
    for entry in std::fs::read_dir(&dir)
        .map_err(|err| AppError::from_io("读取 skills 目录失败", err))?
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !crate::platform::link::is_junction(&path) && path.is_dir() && has_manifest(&path) {
            count += 1;
        }
    }
    Ok(count)
}

fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    for entry in entries.filter_map(Result::ok) {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            total += dir_size(&entry.path());
        } else {
            total += meta.len();
        }
    }
    total
}

/// 实际写一个临时文件再删除，确认目录确实可写。
///
/// 比查权限位可靠：Windows 上 ACL、继承权限、只读属性、受控文件夹访问
/// 都可能让"看起来有权限"的目录实际写不进去。
fn probe_writable(dir: &Path) -> std::io::Result<bool> {
    let probe = dir.join(format!(".bakaskill_write_probe_{}", std::process::id()));
    match std::fs::write(&probe, b"probe") {
        Ok(()) => {
            // 这是本函数自己刚创建的临时文件，删除它不涉及任何用户数据
            let _ = std::fs::remove_file(&probe);
            Ok(true)
        }
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => Ok(false),
        Err(err) => Err(err),
    }
}

/// 启发式识别云同步目录。
///
/// **这是启发式判断，不是权威结论**：用户可以把 OneDrive 文件夹重定向到
/// 任意位置，也可以给同步目录改名。因此只作为警告，不阻断初始化。
fn detect_cloud_sync(path: &Path) -> Option<String> {
    let lower = path.to_string_lossy().to_lowercase();

    for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Ok(value) = std::env::var(var) {
            let value = value.trim().to_lowercase();
            if !value.is_empty() && lower.starts_with(&value) {
                return Some("OneDrive".to_string());
            }
        }
    }

    const KNOWN_DIRS: &[(&str, &str)] = &[
        (r"\dropbox\", "Dropbox"),
        (r"\google drive\", "Google Drive"),
        (r"\icloud drive\", "iCloud Drive"),
        (r"\坚果云\", "坚果云"),
        (r"\nutstore\", "坚果云"),
        (r"\baidunetdisk\", "百度网盘"),
    ];
    for (needle, name) in KNOWN_DIRS {
        if lower.contains(needle) {
            return Some((*name).to_string());
        }
    }

    None
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn existing_temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("创建临时目录失败")
    }

    #[test]
    fn empty_path_is_rejected() {
        assert!(diagnose("   ").is_err());
    }

    #[test]
    fn missing_path_reports_will_create() {
        let dir = existing_temp_dir();
        let target = dir.path().join("brand-new-library");
        let d = diagnose(&target.display().to_string()).unwrap();

        assert!(!d.exists);
        assert!(d.can_initialize, "不存在的路径应允许创建");
        assert!(d.issues.iter().any(|i| i.code == "will_create"));
    }

    #[test]
    fn missing_parent_is_an_error() {
        let dir = existing_temp_dir();
        let target = dir.path().join("no-such-parent").join("library");
        let d = diagnose(&target.display().to_string()).unwrap();

        assert!(!d.can_initialize);
        assert!(d.issues.iter().any(|i| i.code == "parent_missing"));
    }

    #[test]
    fn file_path_is_an_error() {
        let dir = existing_temp_dir();
        let file = dir.path().join("a-file.txt");
        std::fs::write(&file, "x").unwrap();

        let d = diagnose(&file.display().to_string()).unwrap();
        assert!(!d.can_initialize);
        assert!(d.issues.iter().any(|i| i.code == "not_a_directory"));
    }

    #[test]
    fn valid_temp_dir_can_initialize() {
        let dir = existing_temp_dir();
        let d = diagnose(&dir.path().display().to_string()).unwrap();
        assert!(d.writable);
        assert!(d.can_initialize, "问题列表：{:?}", d.issues);
        assert_eq!(d.filesystem.as_deref(), Some("NTFS"));
    }

    #[test]
    fn initialize_creates_skeleton_and_is_idempotent() {
        let dir = existing_temp_dir();
        let root = dir.path().join("library");
        let path_str = root.display().to_string();

        let first = initialize(&path_str).unwrap();
        assert!(!first.already_initialized);
        assert!(skills_dir(&root).is_dir());
        assert!(meta_dir(&root).is_dir());

        // 放一个 Skill 进去，确认重复初始化不会清空内容
        let skill = skills_dir(&root).join("demo");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join(SKILL_MANIFEST), "---\nname: demo\n---\n").unwrap();

        let second = initialize(&path_str).unwrap();
        assert!(second.already_initialized);
        assert_eq!(second.skill_count, 1, "重复初始化后 Skill 丢失");
        assert!(skill.join(SKILL_MANIFEST).exists());
    }

    #[test]
    fn count_skills_ignores_dirs_without_manifest() {
        let dir = existing_temp_dir();
        let root = dir.path().join("library");
        initialize(&root.display().to_string()).unwrap();

        std::fs::create_dir_all(skills_dir(&root).join("good")).unwrap();
        std::fs::write(
            skills_dir(&root).join("good").join(SKILL_MANIFEST),
            "---\nname: good\n---\n",
        )
        .unwrap();

        // 没有 SKILL.md 的目录不算 Skill
        std::fs::create_dir_all(skills_dir(&root).join("incomplete")).unwrap();
        // 文件不算
        std::fs::write(skills_dir(&root).join("stray.txt"), "x").unwrap();

        assert_eq!(count_skills(&root).unwrap(), 1);
    }

    #[test]
    fn manifest_match_is_case_insensitive() {
        let dir = existing_temp_dir();
        let skill = dir.path().join("s");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("skill.md"), "---\nname: s\n---\n").unwrap();
        assert!(has_manifest(&skill), "SKILL.md 匹配应大小写不敏感");
    }

    /// OneDrive 路径必须被识别为云同步目录。
    ///
    /// 依赖本机确实配置了 OneDrive（`%OneDrive%` 环境变量）。未配置时跳过，
    /// 而不是伪造一个结果——那会让测试变成"永远通过"的摆设。
    #[test]
    fn detects_onedrive_when_configured() {
        let Ok(onedrive) = std::env::var("OneDrive") else {
            eprintln!("跳过：本机未设置 %OneDrive% 环境变量");
            return;
        };
        if onedrive.trim().is_empty() {
            eprintln!("跳过：%OneDrive% 为空");
            return;
        }

        let probe = PathBuf::from(&onedrive).join("some-subfolder");
        assert_eq!(
            detect_cloud_sync(&probe).as_deref(),
            Some("OneDrive"),
            "OneDrive 目录未被识别：{onedrive}"
        );
    }

    /// 云同步检测必须只产生 warning，不能阻断初始化
    #[test]
    fn cloud_sync_is_a_warning_not_an_error() {
        let Ok(onedrive) = std::env::var("OneDrive") else {
            eprintln!("跳过：本机未设置 %OneDrive% 环境变量");
            return;
        };
        if onedrive.trim().is_empty() || !Path::new(&onedrive).is_dir() {
            eprintln!("跳过：%OneDrive% 不可用");
            return;
        }

        let d = diagnose(&onedrive).unwrap();
        let cloud_issue = d
            .issues
            .iter()
            .find(|i| i.code == "cloud_sync")
            .expect("应给出 cloud_sync 警告");
        assert_eq!(cloud_issue.severity, IssueSeverity::Warning);
        assert!(
            d.can_initialize,
            "云同步只应警告、不应阻断初始化，issues={:?}",
            d.issues
        );
    }

    #[test]
    fn ordinary_temp_dir_has_no_cloud_sync() {
        let dir = existing_temp_dir();
        assert!(detect_cloud_sync(dir.path()).is_none());
    }

    #[test]
    fn stats_reports_zero_for_uninitialized() {
        let dir = existing_temp_dir();
        let s = stats(&dir.path().display().to_string()).unwrap();
        assert!(!s.initialized);
        assert_eq!(s.skill_count, 0);
    }

    #[test]
    fn stats_counts_skills_and_bytes() {
        let dir = existing_temp_dir();
        let root = dir.path().join("library");
        initialize(&root.display().to_string()).unwrap();

        let skill = skills_dir(&root).join("demo");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join(SKILL_MANIFEST), "---\nname: demo\n---\n").unwrap();

        let s = stats(&root.display().to_string()).unwrap();
        assert!(s.initialized);
        assert_eq!(s.skill_count, 1);
        assert!(s.total_bytes > 0);
    }
}
