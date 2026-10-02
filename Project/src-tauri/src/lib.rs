//! BakaSkill 后端入口。
//!
//! # 模块分层
//!
//! - `platform/` —— **平台专有能力的唯一收敛点**。任何依赖 Windows API 的
//!   调用都必须放在这里，并为非 Windows 提供返回 `Unsupported` 的 stub。
//! - `commands/` —— 薄适配层：参数校验 + 调用领域模块 + 转 IPC 形状。
//! - 领域模块（`config` / `library` / `skill` / `index`）—— 业务逻辑，
//!   可脱离 Tauri 运行时做单元测试。
//!
//! # 日志
//!
//! 全项目使用 `tracing` 宏。tauri-plugin-log 安装的是 `log` 门面的 logger，
//! 因此 Cargo.toml 中为 tracing 启用了 `log` feature 做桥接。

pub mod agents;
pub mod background;
pub mod commands;
pub mod config;
pub mod error;
pub mod git;
pub mod index;
pub mod library;
pub mod links;
pub mod platform;
pub mod scanner;
pub mod skill;
pub mod tags;
pub mod transfer;
pub mod uninstall;

use tauri::Manager;
use tauri_plugin_log::log::LevelFilter;

use commands::{
    agents as agents_cmd, background as background_cmd, config as config_cmd, fs as fs_cmd,
    index as index_cmd, library as library_cmd, link as link_cmd, links as links_cmd,
    skill as skill_cmd, system::ping, tags as tags_cmd, transfer as transfer_cmd,
    uninstall as uninstall_cmd,
};

/// 开发构建记 Debug，发布构建只记 Info，避免日志噪音进入用户环境。
fn log_level() -> LevelFilter {
    if cfg!(debug_assertions) {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // **第一件事**：把旧目录名（SkillHub）下的数据搬到新目录名（BakaSkill）下。
    // 必须早于任何读配置的代码，否则第一次读到的会是"空配置"，
    // 用户的中央库路径看起来就像凭空没了。幂等，重复启动无副作用。
    config::migrate_legacy_layout();

    // 同一件事的另一半：改名前的凭据条目还在 Windows 凭据管理器里。
    // 它已经没有任何代码会读，但留着会在用户的凭据管理器里堆一条看不懂的东西，
    // 而且会让"到底登录了没有"更难判断。这里清一次，幂等。
    git::clear_legacy_credential();

    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().level(log_level()).build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            ping,
            config_cmd::config_get,
            config_cmd::config_set,
            library_cmd::library_validate,
            library_cmd::library_init,
            library_cmd::library_stats,
            link_cmd::link_create,
            link_cmd::link_delete,
            link_cmd::link_status,
            link_cmd::link_list,
            index_cmd::index_rebuild,
            index_cmd::index_list,
            index_cmd::skills_search,
            tags_cmd::tags_stats,
            tags_cmd::tags_apply,
            transfer_cmd::import_preview,
            transfer_cmd::import_preview_paths,
            transfer_cmd::import_apply,
            transfer_cmd::export_skills,
            uninstall_cmd::skills_uninstall,
            background_cmd::background_get,
            background_cmd::background_set,
            background_cmd::background_clear,
            background_cmd::background_image,
            agents_cmd::agents_detect,
            agents_cmd::agent_create_skill_dir,
            agents_cmd::skills_scan,
            skill_cmd::skill_read,
            skill_cmd::skill_write,
            links_cmd::link_matrix,
            links_cmd::link_set_enabled,
            links_cmd::skill_adopt,
            links_cmd::library_relocate,
            git::git_status,
            git::git_save_token,
            git::git_list_repos,
            git::git_set_remote,
            git::git_login_methods,
            git::git_login_browser,
            git::git_logout,
            git::git_sync_status,
            git::git_sync_now,
            git::git_pull,
            git::git_stale_repo,
            git::git_cleanup_stale_repo,
            fs_cmd::write_text_file,
        ])
        .setup(|app| {
            // 定下本进程用哪个 git：**随包自带的优先，系统装的兜底**。
            // 同步功能建立在系统 git 之上，但"用户装了 git"不是能替他保证的事——
            // 一个只想整理 Skill 的人不该为了备份去装 git。找不到自带的那份就
            // 什么都不设，`git_program()` 会回退到 PATH 上的 `git`，与内置之前一样。
            //
            // 必须在任何 git 命令之前完成（`OnceLock` 只认第一次写入，晚了就改不了）。
            #[allow(unused_mut)]
            let mut resource_dir = app.path().resource_dir().ok();
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf));

            // 开发模式下资源不会被拷进 target，改看源码目录下那份。
            #[cfg(debug_assertions)]
            if git::find_bundled_git(resource_dir.as_deref(), exe_dir.as_deref()).is_none() {
                resource_dir = Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
            }

            if let Some(bundled) =
                git::find_bundled_git(resource_dir.as_deref(), exe_dir.as_deref())
            {
                git::init_git_program(bundled);
            } else {
                tracing::info!("未找到随包自带的 git，改用系统 PATH 上的 git");
            }

            // 这条日志同时验证两件事：日志插件已初始化，且
            // tracing -> log 的桥接确实生效（见 docs/ARCHITECTURE.md §6.5）。
            tracing::info!(
                version = env!("CARGO_PKG_VERSION"),
                platform = std::env::consts::OS,
                "BakaSkill 后端启动"
            );
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("启动 BakaSkill 失败");
}
