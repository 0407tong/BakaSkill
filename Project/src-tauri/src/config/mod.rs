//! 应用配置模型与持久化。
//!
//! 落盘位置：`%APPDATA%\BakaSkill\config.json`（而非 Tauri 默认的
//! `%APPDATA%\<identifier>\`）。
//!
//! # 文件格式约定
//!
//! 配置文件使用 **snake_case** 字段名（固定格式，且是用户可能手工编辑的
//! 兼容面）；IPC 传输则沿用本项目的 camelCase 约定。两者通过
//! [`ConfigView`] 转换，见 `commands::config`。
//!
//! # 安全
//!
//! **配置文件中禁止存放任何凭据。** GitHub Token 走 Windows 凭据管理器。
//! 本模块的 `GitConfig` 只存仓库地址与分支名。
//!
//! # 为什么配置里没有 UI 偏好
//!
//! UI 偏好存于 localStorage
//! （`index.html` 的防闪烁内联脚本必须在任何 IPC 之前同步读到主题）。
//! 两处并存会造成失同步，因此本项目**不在配置文件中重复存储 UI 偏好**。
//! 详见 `docs/ARCHITECTURE.md` §8。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// 配置目录名（位于 `%APPDATA%` 下）
pub const CONFIG_DIR_NAME: &str = "BakaSkill";
/// 改名前用过的目录名。仅用于把老用户的数据搬过来，见 [`migrate_legacy_layout`]。
pub const LEGACY_DIR_NAME: &str = "SkillHub";
pub const CONFIG_FILE_NAME: &str = "config.json";

/// 当前配置结构版本。结构发生不兼容变更时递增，并在 `load` 中做迁移。
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// 单个 Agent 的配置
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    /// 是否纳入统一管理
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 该 Agent 的技能目录。`None` 表示尚未配置（首次运行时会自动探测后填充）。
    #[serde(default)]
    pub skill_dir: Option<String>,
    /// 展示名。仅对**用户自定义 Agent**（不在内置注册表中的）需要填写；
    /// 内置 Agent 的名称来自注册表。
    #[serde(default)]
    pub display_name: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Git 同步配置。
///
/// **不含 Token**——凭据存 Windows 凭据管理器。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GitConfig {
    #[serde(default)]
    pub remote_url: Option<String>,
    #[serde(default = "default_branch")]
    pub branch: String,
    #[serde(default)]
    pub auto_push: bool,
}

fn default_branch() -> String {
    "main".to_string()
}

/// 应用配置（即 config.json 的完整结构）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    pub schema_version: u32,
    /// 中央库根目录。`None` 表示用户尚未选择。
    #[serde(default)]
    pub central_library_path: Option<String>,
    #[serde(default)]
    pub agents: Vec<AgentConfig>,
    #[serde(default)]
    pub git: GitConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            central_library_path: None,
            agents: Vec::new(),
            git: GitConfig {
                remote_url: None,
                branch: default_branch(),
                auto_push: false,
            },
        }
    }
}

impl AppConfig {
    /// 中央库路径。空字符串按"未设置"处理——手工编辑配置文件时常见。
    pub fn central_library(&self) -> Option<&str> {
        self.central_library_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }
}

/// 把旧目录名下的数据搬到新目录名下。**只在启动时调用一次。**
///
/// # 为什么必须做
///
/// 应用原来叫 SkillHub，配置与背景图都在 `%APPDATA%\SkillHub\`，
/// 同步工作区在 `%LOCALAPPDATA%\SkillHub\`。改名之后如果只是换个常量、
/// 不管旧目录，老用户会看到**配置全丢**——中央库路径、Agent 目录、背景图
/// 全都没了。文件其实还躺在旁边那个目录里，但用户不可能知道这件事。
///
/// # 幂等
///
/// 只在"**新目录不存在、旧目录存在**"时才动手，因此重复调用是安全的。
/// 迁移用 `rename` 而不是复制：同一个盘内是原子操作，不会留下搬了一半的状态。
/// 万一 rename 失败（例如被占用），**只记日志、不中断启动**——
/// 旧数据还在原处，用户至少可以手工搬。
pub fn migrate_legacy_layout() {
    migrate_dir("APPDATA", "配置目录");
    // 同步工作区同理。它本身是可重建的派生物（重新 clone 即可），
    // 但重建要联网，顺手搬过来对用户更省事。
    migrate_dir("LOCALAPPDATA", "同步工作区");
    migrate_library_meta_dir();
}

/// 中央库里的元数据目录（旧名 `.skillhub`）也要一并搬过来。
///
/// # 为什么这个不能省
///
/// 索引本身是可重建的，丢了无所谓。但那个目录下还有 `removed/`——
/// **用户卸载 Skill 时被移到一旁的东西**（卸载走的是"移走不销毁"）。
/// 只换个常量、不管旧目录的话，用户会以为那些东西丢了。
///
/// 这一段必须在读配置**之后**跑：中央库路径是用户自己配的，
/// 不读配置根本不知道要去哪儿搬。
fn migrate_library_meta_dir() {
    let Ok(cfg) = load() else {
        return;
    };
    let Some(root) = cfg.central_library().map(PathBuf::from) else {
        return;
    };

    let old = root.join(crate::library::LEGACY_META_DIR);
    let new = root.join(crate::library::META_DIR);

    if new.exists() || !old.is_dir() {
        return;
    }

    match std::fs::rename(&old, &new) {
        Ok(()) => tracing::info!(
            from = %old.display(),
            to = %new.display(),
            "改名迁移：中央库的元数据目录已搬到新名下"
        ),
        Err(err) => tracing::error!(
            from = %old.display(),
            error = %err,
            "改名迁移失败，中央库的旧元数据目录仍在原处（索引会自动重建，\
             但 removed/ 里若有被卸载的 Skill 需手工取回）"
        ),
    }
}

fn migrate_dir(env_var: &str, what: &str) {
    let Ok(base) = std::env::var(env_var) else {
        return;
    };
    let old = Path::new(&base).join(LEGACY_DIR_NAME);
    let new = Path::new(&base).join(CONFIG_DIR_NAME);

    if new.exists() || !old.is_dir() {
        return;
    }

    match std::fs::rename(&old, &new) {
        Ok(()) => tracing::info!(
            from = %old.display(),
            to = %new.display(),
            "改名迁移：{what}已搬到新目录"
        ),
        Err(err) => tracing::error!(
            from = %old.display(),
            to = %new.display(),
            error = %err,
            "改名迁移失败，{what}仍留在旧位置。程序会按新用户启动，\
             如需找回数据请手工把该目录改名或移动"
        ),
    }
}

/// 配置目录：`%APPDATA%\BakaSkill`
pub fn config_dir() -> AppResult<PathBuf> {
    let appdata = std::env::var_os("APPDATA")
        .ok_or_else(|| AppError::Config("未找到 APPDATA 环境变量，无法定位配置目录".to_string()))?;
    Ok(Path::new(&appdata).join(CONFIG_DIR_NAME))
}

/// 配置文件完整路径
pub fn config_path() -> AppResult<PathBuf> {
    Ok(config_dir()?.join(CONFIG_FILE_NAME))
}

/// 从指定路径读取配置（便于测试）
pub fn load_from(path: &Path) -> AppResult<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let raw = std::fs::read_to_string(path)
        .map_err(|err| AppError::from_io(&format!("读取配置失败：{}", path.display()), err))?;

    if raw.trim().is_empty() {
        return Ok(AppConfig::default());
    }

    let mut config: AppConfig = serde_json::from_str(&raw)
        .map_err(|err| AppError::Config(format!("配置文件格式错误：{err}")))?;

    // 结构版本迁移入口。当前只有 v1，占位以便后续版本在此分支处理。
    if config.schema_version > CURRENT_SCHEMA_VERSION {
        return Err(AppError::Config(format!(
            "配置文件版本 {} 高于本程序支持的 {}，请升级 SkillHub",
            config.schema_version, CURRENT_SCHEMA_VERSION
        )));
    }
    config.schema_version = CURRENT_SCHEMA_VERSION;

    Ok(config)
}

/// 原子写入：先写临时文件再 rename，避免掉电/崩溃产生半截配置。
///
/// Windows 上 `std::fs::rename` 使用 `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`，
/// 可以覆盖已存在的目标文件。
pub fn save_to(path: &Path, config: &AppConfig) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            AppError::from_io(&format!("创建配置目录失败：{}", parent.display()), err)
        })?;
    }

    let json = serde_json::to_string_pretty(config)
        .map_err(|err| AppError::Config(format!("序列化配置失败：{err}")))?;

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes())
        .map_err(|err| AppError::from_io(&format!("写入临时配置失败：{}", tmp.display()), err))?;
    std::fs::rename(&tmp, path)
        .map_err(|err| AppError::from_io(&format!("替换配置文件失败：{}", path.display()), err))?;

    Ok(())
}

/// 读取当前用户的配置
pub fn load() -> AppResult<AppConfig> {
    load_from(&config_path()?)
}

/// 保存当前用户的配置
pub fn save(config: &AppConfig) -> AppResult<()> {
    save_to(&config_path()?, config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_no_library() {
        let c = AppConfig::default();
        assert!(c.central_library().is_none());
        assert_eq!(c.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(c.git.branch, "main");
    }

    #[test]
    fn blank_library_path_counts_as_unset() {
        let c = AppConfig {
            central_library_path: Some("   ".to_string()),
            ..AppConfig::default()
        };
        assert!(c.central_library().is_none());
    }

    /// 配置文件必须是 snake_case（固定的文件格式兼容面）
    #[test]
    fn serializes_with_snake_case_keys() {
        let json = serde_json::to_string(&AppConfig::default()).unwrap();
        assert!(json.contains("\"schema_version\""), "实际输出：{json}");
        assert!(!json.contains("schemaVersion"));
    }

    #[test]
    fn missing_file_yields_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        assert_eq!(load_from(&path).unwrap(), AppConfig::default());
    }

    #[test]
    fn empty_file_yields_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "   \n").unwrap();
        assert_eq!(load_from(&path).unwrap(), AppConfig::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.json");

        let config = AppConfig {
            central_library_path: Some(r"D:\SkillHubLibrary".to_string()),
            agents: vec![AgentConfig {
                id: "claude-code".to_string(),
                enabled: true,
                skill_dir: Some(r"C:\Users\me\.claude\skills".to_string()),
                display_name: None,
            }],
            ..AppConfig::default()
        };

        save_to(&path, &config).unwrap();
        assert_eq!(load_from(&path).unwrap(), config);
    }

    #[test]
    fn save_is_atomic_and_leaves_no_tmp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        save_to(&path, &AppConfig::default()).unwrap();

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "残留临时文件：{leftovers:?}");
    }

    #[test]
    fn newer_schema_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"schema_version": 999, "agents": [], "git": {"branch": "main"}}"#,
        )
        .unwrap();
        assert!(load_from(&path).is_err());
    }

    #[test]
    fn malformed_json_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(load_from(&path).is_err());
    }
}
