//! 配置读写命令。
//!
//! # 为什么有 DTO
//!
//! `config.json` 的字段名是 **snake_case**（固定的文件格式，且是用户
//! 可能手工编辑的兼容面），而本项目 IPC 约定是 **camelCase**。两者不能
//! 同时满足于同一个结构体，因此这里用 [`ConfigView`] 做显式转换。
//!
//! `ConfigView` 与 `config::AppConfig` 的字段必须一一对应；
//! `tests` 里的 `config_view_matches_file_model` 用于防止二者漂移。

use serde::{Deserialize, Serialize};

use crate::config::{self, AgentConfig, AppConfig, GitConfig};
use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    pub id: String,
    pub enabled: bool,
    pub skill_dir: Option<String>,
    /// 展示名。仅用户自定义 Agent 需要，内置 Agent 的名称来自注册表。
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitView {
    pub remote_url: Option<String>,
    pub branch: String,
    pub auto_push: bool,
}

/// 配置的 IPC 视图（camelCase）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    pub schema_version: u32,
    pub central_library_path: Option<String>,
    pub agents: Vec<AgentView>,
    pub git: GitView,
}

impl From<&AppConfig> for ConfigView {
    fn from(c: &AppConfig) -> Self {
        Self {
            schema_version: c.schema_version,
            central_library_path: c.central_library_path.clone(),
            agents: c
                .agents
                .iter()
                .map(|a| AgentView {
                    id: a.id.clone(),
                    enabled: a.enabled,
                    skill_dir: a.skill_dir.clone(),
                    display_name: a.display_name.clone(),
                })
                .collect(),
            git: GitView {
                remote_url: c.git.remote_url.clone(),
                branch: c.git.branch.clone(),
                auto_push: c.git.auto_push,
            },
        }
    }
}

impl From<&ConfigView> for AppConfig {
    fn from(v: &ConfigView) -> Self {
        Self {
            schema_version: v.schema_version,
            central_library_path: v.central_library_path.clone(),
            agents: v
                .agents
                .iter()
                .map(|a| AgentConfig {
                    id: a.id.clone(),
                    enabled: a.enabled,
                    skill_dir: a.skill_dir.clone(),
                    display_name: a.display_name.clone(),
                })
                .collect(),
            git: GitConfig {
                remote_url: v.git.remote_url.clone(),
                branch: v.git.branch.clone(),
                auto_push: v.git.auto_push,
            },
        }
    }
}

/// 读取当前配置。配置文件不存在时返回默认配置，而不是报错——
/// "首次启动"不是异常情况。
#[tauri::command]
pub fn config_get() -> AppResult<ConfigView> {
    let config = config::load()?;
    Ok(ConfigView::from(&config))
}

/// 整体替换配置并落盘（原子写）
#[tauri::command]
pub fn config_set(config: ConfigView) -> AppResult<ConfigView> {
    let mut model = AppConfig::from(&config);
    model.schema_version = config::CURRENT_SCHEMA_VERSION;
    config::save(&model)?;
    tracing::info!(path = ?model.central_library(), "配置已保存");
    Ok(ConfigView::from(&model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn view_serializes_to_camel_case() {
        let view = ConfigView::from(&AppConfig::default());
        let value = serde_json::to_value(&view).unwrap();
        assert!(
            value.get("schemaVersion").is_some(),
            "IPC 视图应为 camelCase"
        );
        assert!(value.get("centralLibraryPath").is_some());
        assert!(value.get("schema_version").is_none());
    }

    #[test]
    fn file_model_serializes_to_snake_case() {
        let value = serde_json::to_value(AppConfig::default()).unwrap();
        assert!(
            value.get("schema_version").is_some(),
            "配置文件的键应为 snake_case"
        );
        assert!(value.get("schemaVersion").is_none());
    }

    /// 防止两个结构体字段漂移
    #[test]
    fn config_view_matches_file_model() {
        let model = AppConfig {
            central_library_path: Some(r"D:\Lib".to_string()),
            agents: vec![AgentConfig {
                id: "claude-code".to_string(),
                enabled: false,
                skill_dir: Some(r"C:\x".to_string()),
                display_name: None,
            }],
            git: GitConfig {
                remote_url: Some("https://example.com/r.git".to_string()),
                branch: "main".to_string(),
                auto_push: true,
            },
            ..AppConfig::default()
        };

        let round_tripped = AppConfig::from(&ConfigView::from(&model));
        assert_eq!(round_tripped, model, "ConfigView 往返丢失或篡改了字段");
    }

    #[test]
    fn view_round_trips_through_json() {
        let view = ConfigView::from(&AppConfig::default());
        let json: Value = serde_json::to_value(&view).unwrap();
        let parsed: ConfigView = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, view);
    }

    #[test]
    fn agents_default_enabled_when_omitted() {
        // 配置文件里 {"id": "x"} 应得到 enabled = true
        let parsed: AgentConfig = serde_json::from_value(json!({"id": "x"})).unwrap();
        assert!(parsed.enabled);
        assert!(parsed.skill_dir.is_none());
    }
}
