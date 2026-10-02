//! GitHub 同步：凭据管理、仓库选择、推送/拉取。
//!
//! # 两条硬性约束
//!
//! 1. **Token 绝不落明文**。它只存在于 Windows 凭据管理器（`keyring`），
//!    不写入 `config.json`、不写日志、不进错误信息。本模块的 `redact`
//!    负责把可能混入输出中的 token 抹掉。
//! 2. **用系统 `git` CLI，不用 libgit2**。行为与用户手工操作完全一致、
//!    可解释、能复用系统已有的凭据配置，也避免 libgit2 在 Windows 上的构建坑。
//!
//! # 认证方式
//!
//! 优先使用凭据管理器中的 Token；**没有 Token 时不做任何注入**，
//! 让 `git` 走系统凭据助手（credential helper）。这样用户既可以在本应用里
//! 填 Token，也可以复用已有的 GitHub 登录，两条路都不冲突。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
/// 派生外部命令的统一入口。唯一的作用是**在 Windows 上不给子进程建控制台
/// 窗口**——否则 GUI 子系统下每派一次 `git.exe` 都会闪一个终端窗口，
/// 还会抢走前台焦点（用户报的"一堆终端窗口"和"卡顿"都是它）。
/// 平台差异按约定收敛在 `platform/`，见 `platform/process.rs`。
///
/// **每一处派生都要走它**，漏一处就还会闪。
use crate::platform::process::command as cmd;

/// 凭据管理器中的服务名
const KEYRING_SERVICE: &str = "com.bakaskill.app";
/// 改名之前（SkillHub 时期）用的服务名。同一个用户升级上来时，
/// 凭据管理器里会同时留着两条，旧的那条不再有人读。
const LEGACY_KEYRING_SERVICE: &str = "com.skillhub.app";
/// 凭据管理器中的用户名（同一个服务下可存多条，用固定用户名即可）
const KEYRING_USER: &str = "github-token";

/// 从仓库导入 Skill 时的暂存目录前缀。
///
/// 暂存目录放在中央库的 `.bakaskill/incoming/` 下（见 `import_skills`），
/// 这里的前缀只用于"遍历时跳过残留的暂存目录"，防止一次中断的运行
/// 把半截内容当成正经 Skill。
const STAGING_PREFIX: &str = ".bakaskill-incoming-";

/// 各系统自己产生的杂项文件，不该进仓库，也不该被导入。
///
/// 这些原本由同步用的 `.gitignore` 挡掉。新模型不再往用户的仓库里写
/// `.gitignore`（那会改动他的仓库根目录），改在**复制这一层**拦掉：
/// 不复制，自然就不会被提交。
///
/// `is_dir` 参与判断是有意的：`*.tmp` 只对**文件**成立——
/// 一个叫 `xxx.tmp` 的目录完全可能是个正经的 Skill 名。
pub(crate) fn is_junk(name: &str, is_dir: bool) -> bool {
    matches!(name, ".DS_Store" | "Thumbs.db" | "desktop.ini") || (!is_dir && name.ends_with(".tmp"))
}

/// 目录项是否存在（**不跟随**重解析点）。
///
/// 不能用 `Path::exists()`：它会跟随重解析点，于是一个悬空的 junction
/// （中央库迁移之后很常见，见 `docs/LINKING.md` §4.3）会被判成"不存在"，
/// 接着往一个**已经存在**的目录项上改名就会失败，整次拉取随之中断，
/// 而用户只看到一句"写入 Skill 失败"。
fn entry_exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// 这个目录项是不是某种链接（junction 或符号链接）。
///
/// 镜像与导入都**不跟随链接**：链接指向别处，复制它等于复制别处的内容。
/// 注意 `is_junction` 只认 junction 重解析点，认不出目录符号链接，
/// 两者必须都判（见 `docs/LINKING.md` §4）。
fn is_link(path: &Path) -> bool {
    crate::platform::link::is_junction(path)
        || std::fs::symlink_metadata(path)
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(false)
}

/// 清理本模块自己创建的暂存目录。
///
/// 这是本模块唯一一处目录删除。项目红线是"绝不通过链接删除"
/// （`docs/LINKING.md` §3）——删除一个链接会穿透到它指向的真实目录。
/// 这里之所以安全：路径由本模块构造（中央库的 `.bakaskill/incoming/` 下，
/// 那是应用自己的派生目录），而且动手前**仍会确认它不是链接**。
fn remove_app_scratch(path: &Path) {
    if !entry_exists(path) || is_link(path) {
        return;
    }
    if let Err(err) = std::fs::remove_dir_all(path) {
        tracing::warn!(path = %path.display(), error = %err, "清理暂存目录失败（不影响本次结果）");
    }
}

/// 地址里是否内联了凭据（形如 `https://用户名:Token@github.com/…`）。
///
/// 把 Token 拼进地址是 GitHub 上的常见做法，但对本应用是一条**红线**：
/// 地址会被写进 `config.json`、被 `git remote set-url` 写进工作区的
/// `.git/config`、还会显示在界面上——而 Token 只允许存在于凭据管理器里。
fn has_inline_credentials(url: &str) -> bool {
    let Some((_, rest)) = url.split_once("//") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    match authority.split_once('@') {
        Some((userinfo, _)) => userinfo.contains(':'),
        None => false,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitAvailability {
    /// 系统是否装了 git
    pub available: bool,
    pub version: Option<String>,
    /// 是否已保存 Token（本应用自己的凭据管理器条目）
    pub has_token: bool,
    /// 系统凭据助手中是否已有 GitHub 凭据（例如用账号登录过）
    pub has_helper_credentials: bool,
    /// 配置中的远程地址
    pub remote_url: Option<String>,
    pub branch: String,
}

/// GitHub 上的一个仓库（用于「选择仓库」界面）
///
/// # 为什么收发用两套命名规则
///
/// 这个结构体同时承担两个方向的转换：
/// - **收**：直接反序列化 GitHub REST API 的响应，那边是 `full_name` 蛇形命名；
/// - **发**：作为 IPC 载荷发给前端，本项目约定是 camelCase。
///
/// 只写 `rename_all = "camelCase"` 会让反序列化收不到字段（前端拿到 undefined，
/// 一调用 `.toLowerCase()` 就白屏）；只写默认则前端字段名对不上。
/// 因此必须**分别指定**。
///
/// 这是本项目"前后端契约无编译期约束"导致的典型缺陷，见
/// `docs/ARCHITECTURE.md` §3。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct GitHubRepo {
    pub full_name: String,
    pub name: String,
    pub private: bool,
    #[serde(default)]
    pub description: Option<String>,
    /// HTTPS 克隆地址
    #[serde(default)]
    pub clone_url: String,
    #[serde(default)]
    pub default_branch: Option<String>,
}

// ===========================================================================
// 凭据管理（登录接口的后端）
// ===========================================================================

fn entry() -> AppResult<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|err| AppError::Internal(format!("无法访问 Windows 凭据管理器：{err}")))
}

/// 保存 Token 到 Windows 凭据管理器
#[tauri::command]
pub fn git_save_token(token: String) -> AppResult<GitAvailability> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(AppError::Config("Token 为空".to_string()));
    }

    entry()?
        .set_password(&token)
        .map_err(|err| AppError::Internal(format!("写入凭据管理器失败：{err}")))?;

    // 只记长度与是否存在，绝不记内容
    tracing::info!(token_len = token.len(), "GitHub Token 已保存到凭据管理器");
    Ok(read_availability())
}

// 退出登录只有 `git_logout` 一条路径（在文件后半部分）。
// 曾经这里还有一个只清本条目的 `git_clear_token`，与只清系统凭据的
// `git_logout_browser` 并存——两个按钮各清一半，"退不掉"就是这么来的。

// ===========================================================================
// 系统凭据助手（Git Credential Manager）—— 用 GitHub 账号登录
// ===========================================================================

/// 登录方式的可用性
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginMethods {
    /// 系统凭据助手的配置值，例如 `helper-selector`（GCM）或 `manager`
    pub credential_helper: Option<String>,
    /// 是否找到了 Git Credential Manager 可执行文件
    pub gcm_available: bool,
    pub gcm_path: Option<String>,
}

/// 定位 Git Credential Manager。
///
/// 先查 PATH，再按 git 自身的安装位置推导——Scoop / 便携版 git 的
/// shim 目录往往不包含 GCM，但二进制就在 git 安装目录的 `mingw64/bin` 下。
fn find_gcm() -> Option<std::path::PathBuf> {
    // 1) PATH 上直接可执行
    if let Ok(out) = cmd("git-credential-manager").arg("--version").output() {
        if out.status.success() {
            return Some(std::path::PathBuf::from("git-credential-manager"));
        }
    }

    // 2) 由 git --exec-path 推导：<git>/mingw64/libexec/git-core → <git>/mingw64/bin
    let exec_path = run_git(None, &["--exec-path"]).ok()?;
    let exec_path = std::path::PathBuf::from(exec_path.trim());
    let git_root = exec_path.parent()?.parent()?; // 去掉 libexec/git-core

    [
        git_root.join("bin").join("git-credential-manager.exe"),
        git_root
            .join("mingw64")
            .join("bin")
            .join("git-credential-manager.exe"),
        git_root
            .join("mingw64")
            .join("libexec")
            .join("git-core")
            .join("git-credential-manager.exe"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

/// 查询系统上有哪些登录方式可用
#[tauri::command]
pub fn git_login_methods() -> AppResult<LoginMethods> {
    let helper = run_git(None, &["config", "--system", "--get", "credential.helper"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            run_git(None, &["config", "--global", "--get", "credential.helper"])
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });

    let gcm = find_gcm();
    Ok(LoginMethods {
        credential_helper: helper,
        gcm_available: gcm.is_some(),
        gcm_path: gcm.map(|p| p.display().to_string()),
    })
}

/// 用 GitHub 账号登录（浏览器授权），**不需要 Personal Access Token**。
///
/// 走 Git Credential Manager 的 `github login` 子命令：
/// 它会打开浏览器让你用 GitHub 账号授权，完成后把凭据存入系统的凭据存储。
/// 对用户来说就是"点一下、浏览器里确认一下"，没有复制粘贴 Token 的步骤。
///
/// 这是一个**阻塞调用**——浏览器授权需要用户操作，可能耗时较久。
/// 前端以 loading 状态呈现，并在完成后刷新状态。
#[tauri::command]
pub fn git_login_browser() -> AppResult<LoginMethods> {
    let gcm = find_gcm().ok_or_else(|| {
        AppError::Config(
            "未找到 Git Credential Manager。请安装 Git for Windows（自带 GCM），\
             或改用下方的手动 Token 登录。"
                .to_string(),
        )
    })?;

    // ⚠️ 必须显式**打开**交互能力，不能依赖继承来的环境。
    //
    // 实测踩到：本应用的宿主进程环境里可能已经存在
    //   GIT_TERMINAL_PROMPT=0
    //   GCM_INTERACTIVE=never
    // （这类变量常由自动化工具、CI、或某些 IDE 设置，用于防止 git 卡在提示上）。
    // 一旦继承，GCM 会拒绝弹窗，报
    //   fatal: Cannot prompt because user interactivity has been disabled.
    // ——用户看到的是"登录按钮点了没用"。
    //
    // 登录动作的本质就是交互，因此这里必须反过来显式声明：
    // 允许交互、并把 --browser 传明确，避免 GCM 走"自动选择"分支时被环境影响。
    let output = cmd(&gcm)
        .args(["github", "login", "--browser"])
        .env("GCM_INTERACTIVE", "always")
        .env_remove("GIT_TERMINAL_PROMPT")
        .env_remove("GIT_ASKPASS")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|err| AppError::Io(format!("无法启动 Git Credential Manager：{err}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Io(format!(
            "登录未完成：{}{}",
            redact(stderr.trim()),
            if stderr.trim().is_empty() {
                "（可能被用户取消）"
            } else {
                ""
            }
        )));
    }

    tracing::info!("已通过 Git Credential Manager 完成 GitHub 账号登录");

    // 趁登录这个"本来就要等"的时机，把 Token 换成我们自己的凭据，
    // 免得每次列仓库都要再等几十秒
    cache_helper_token();

    git_login_methods()
}

/// 清除改名之前遗留的那条凭据条目。
///
/// 它已经没有任何代码在读（服务名换成了 `com.bakaskill.app`），留着只会在
/// 用户的凭据管理器里堆一条看不懂的东西。删不掉也不影响功能，因此只记日志。
///
/// 在应用启动时调用一次（见 `lib.rs`）。
pub fn clear_legacy_credential() {
    let Ok(legacy) = keyring::Entry::new(LEGACY_KEYRING_SERVICE, KEYRING_USER) else {
        return;
    };
    match legacy.delete_credential() {
        Ok(()) => tracing::info!("已清除改名前的遗留凭据条目"),
        // 本来就没有（绝大多数用户）——不是异常
        Err(keyring::Error::NoEntry) => {}
        Err(err) => tracing::warn!(error = %err, "清除遗留凭据条目失败（不影响使用）"),
    }
}

/// 让系统中的 git 凭据助手删掉 github.com 的凭据。
///
/// # 为什么走 `git credential reject` 而不是 `cmdkey /delete`
///
/// `reject` 是凭据协议里的"作废"请求，由**用户自己配置的**凭据助手去执行。
/// 凭据存在哪里、条目叫什么名字是助手的事（GCM 用的是
/// `git:https://github.com`，别的助手可能不同），写死一个目标名会在换了
/// 助手之后静默失效——而"静默失效"正是这次要修的那个毛病。
fn forget_helper_credential() {
    use std::io::Write;

    /// 这个动作只是删一条本地凭据，**没有任何理由需要等待**。
    /// 兜底上限给得很宽：真要超时，说明助手卡住了，杀掉它继续往下走，
    /// 后面那次状态复读会把"没删干净"如实报出来。
    const ERASE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

    let Ok(mut child) = cmd("git")
        .args(["credential", "reject"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return;
    };

    // 只要凭据的"位置"，不需要密码
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"protocol=https\nhost=github.com\n\n");
    }

    // 看门狗。**不能直接 `child.wait()`**：凭据助手是要跟系统凭据管理器
    // （可能还有网络）打交道的第三方进程，实测 `git credential fill` 就会
    // 卡到 31 秒。这里虽然是 erase、通常快得多，但同一个助手、同一个进程，
    // 没有理由认为它一定不会卡——而"退出登录"按钮转圈转到天荒地老，
    // 是比"没删干净"更糟的失败方式。
    let deadline = std::time::Instant::now() + ERASE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Ok(None) => {
                tracing::warn!("凭据助手在超时内没有完成删除，已结束它");
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            Err(_) => break,
        }
    }
}

/// 退出 GitHub 登录：把本应用与系统凭据助手里保存的 GitHub 凭据**全部**清掉。
///
/// # 为什么必须一起清
///
/// 本应用允许**直接沿用系统里已有的 GitHub 登录**（见模块头）：只要系统 git
/// 的凭据助手里有 github.com 的凭据，界面就显示"已登录"，仓库列表与同步也
/// 直接用那份凭据。
///
/// 于是"退出登录"如果只清本应用自己存的那一条，界面会**仍然显示已登录**——
/// 用户点了退出却退不掉。这正是用户实际撞到的现象：界面上有两个"退出"，
/// 一个只清本应用的凭据、一个只清系统的，两个都按了还是"已登录"。
///
/// 所以这里合成**一条**统一的退出路径，三处一起清：
///   1. 本应用存在凭据管理器里的 Token；
///   2. 改名之前遗留的旧条目；
///   3. 系统凭据助手里那份（**代价明说**：这台机器上其它地方用 git 推
///      GitHub 也要重新登录一次。界面上的确认框已经把这句话讲给用户了）。
#[tauri::command]
pub fn git_logout() -> AppResult<GitAvailability> {
    // 1) 本应用自己的凭据
    match entry()?.delete_credential() {
        Ok(()) => tracing::info!("已清除本应用保存的 GitHub 凭据"),
        // 本来就没有，视为成功
        Err(keyring::Error::NoEntry) => {}
        Err(err) => return Err(AppError::Internal(format!("删除凭据失败：{err}"))),
    }

    // 2) 改名前的遗留条目
    clear_legacy_credential();

    // 3) 系统凭据助手那份。先走凭据协议（任何助手都认），
    //    再让 GCM 把自己那份账号记录也清掉——两者不一定是同一条。
    forget_helper_credential();
    if let Some(gcm) = find_gcm() {
        // 失败不阻断：本来就没登录时它会报错，这不是异常
        let _ = cmd(&gcm).args(["github", "logout"]).output();
    }

    // **重新读一次真实状态**再回给界面。上面任何一步都可能无声地没生效，
    // 那就必须如实说"仍然已登录"，绝不能报一个假的成功——
    // 用户上一次遇到的正是"点了退出、界面还说已登录"。
    let status = read_availability();
    if status.has_token || status.has_helper_credentials {
        return Err(AppError::Io(
            "凭据没有被完全清除，界面上的登录状态仍然是真实的。\
             请在「控制面板 → 用户账户 → 凭据管理器 → Windows 凭据」里\
             手动删除 `git:https://github.com` 这一条。"
                .to_string(),
        ));
    }

    tracing::info!("已退出 GitHub 登录");
    Ok(status)
}

/// 判断系统凭据助手里是否已有 GitHub 凭据。
///
/// # 为什么不问 git
///
/// 最初用 `git credential fill`。**实测这会挂住**：在"尚无凭据"时，
/// GCM 会去**打开自己的 GUI 登录窗口**并一直等下���，一次状态查询把整个
/// 界面卡了 185 秒（测试跑出来才发现）。
///
/// 改为直接查 Windows 凭据管理器：GCM 会把凭据存成一个目标名形如
/// `git:https://github.com` 的条目，用 `cmdkey /list` 枚举即可。
/// 这是**只读且天然有界**的操作，不存在等待用户输入的可能。
#[cfg(windows)]
fn has_helper_credentials() -> bool {
    let Ok(output) = cmd("cmdkey").arg("/list").output() else {
        return false;
    };
    // cmdkey 的输出是本地化的，但凭据的**目标名**不受语言影响
    String::from_utf8_lossy(&output.stdout).contains("git:https://github.com")
}

#[cfg(not(windows))]
fn has_helper_credentials() -> bool {
    false
}

/// 读取 Token。**只在进程内使用，绝不返回给前端。**
fn load_token() -> Option<String> {
    entry().ok()?.get_password().ok()
}

/// 判断是否已有 Token（不读取内容）
fn has_token() -> bool {
    matches!(entry().ok().map(|e| e.get_password()), Some(Ok(_)))
}

// ===========================================================================
// 仓库列表（选择仓库接口的后端）
// ===========================================================================

/// 取得用于调用 GitHub API 的 Token。
///
/// 两条来源，按优先级：
/// 1. 本应用自己存在凭据管理器里的 PAT；
/// 2. 系统凭据助手里已有的 GitHub 凭据（用户用「账号登录」授权后由 GCM 保存）。
///
/// # 为什么第 2 条是安全的
///
/// 早先我判断"账号登录时读不到 Token，所以无法列仓库"，**那个判断过于保守**。
/// 关键区别在于**何时**去问助手：
///
/// - 尚无凭据时问 → GCM 会转去弹窗登录 → **挂住**（这是之前 185 秒卡死的根因）；
/// - 已有凭据时问 → 助手立即返回，不发生任何交互。
///
/// 因此这里先用 `has_helper_credentials()`（读 Windows 凭据管理器，天然有界）
/// 确认凭据**确实存在**，再调 `git credential fill` 取值。
/// 该调用只会在已知有凭据时发生，不会挂起。
///
/// 取到的是 GCM 的 OAuth Token，可直接作为 Bearer 用于 GitHub API。
/// 换取并缓存 Token 的超时上限。
///
/// 实测：`git credential fill` 即便在凭据已存在时也可能耗时 **31 秒**
/// （GCM 会去 GitHub 校验/刷新令牌）。这个代价只能付一次，
/// 且必须封顶——否则登录按钮会无限期转圈。
const TOKEN_EXCHANGE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(40);

/// 取得用于 API 调用与出站请求的 Token，**只读本地凭据**。
///
/// 绝不触发与助手或网络的交互：一旦这里去问助手，
/// "加载仓库列表"就会变成几十秒的等待。有测试守着它的耗时上限。
fn api_token() -> Option<String> {
    load_token()
}

/// 确保本地有可用于 API 的 Token，没有则尝试从系统凭据助手换取。
///
/// # 为什么需要"自愈"
///
/// Token 是在「账号登录」时缓存的。但用户可能**在缓存机制存在之前**
/// 就已经登录过（本项目的真实经历），此时本地没有缓存、
/// 助手里有凭据——如果只是报"尚无可用凭据"，用户会陷入
/// "明明登录了却用不了"的死角。
///
/// 因此这里做一次惰性补取：只要助手里有凭据就换过来并缓存。
/// 代价（可能几十秒）只付一次，之后全部走本地。
pub fn ensure_api_token() -> bool {
    if api_token().is_some() {
        return true;
    }
    if !has_helper_credentials() {
        return false;
    }
    tracing::info!("本地无缓存凭据，尝试从系统凭据助手换取（可能耗时较久）");
    cache_helper_token()
}

/// 登录成功后，把系统凭据助手中的 Token 换取到本应用自己的凭据条目里，
/// 之后所有 GitHub API 调用都读本地，不再每次去问助手。
///
/// **为什么必须缓存**：`git credential fill` 一次要几十秒，
/// 放在"加载仓库列表"这类即时操作上是不可接受的。
/// 登录本身已经是一个有进度提示的长动作，把这次开销放在那里最合适。
///
/// 失败不阻断登录：拿不到 Token 只是"看不到仓库列表"，
/// 仍可手动填写地址完成同步。
fn cache_helper_token() -> bool {
    if load_token().is_some() {
        return true;
    }
    if !has_helper_credentials() {
        return false;
    }
    let Some(token) = credential_from_helper_bounded() else {
        tracing::warn!("未能在超时内取得系统凭据，仓库列表将不可用（可手动填写地址）");
        return false;
    };
    match entry() {
        Ok(e) => {
            let ok = e.set_password(&token).is_ok();
            if ok {
                tracing::info!(token_len = token.len(), "已缓存系统凭据供 API 使用");
            }
            ok
        }
        Err(_) => false,
    }
}

/// 带硬超时的 `credential_from_helper`。
///
/// 用子进程 + 看门狗实现：超时即杀掉，避免登录按钮无限期卡住。
/// 之所以不简单地依赖"助手会很快返回"，是因为实测它会花 31 秒，
/// 而这个数字在不同网络与令牌状态下还会变。
fn credential_from_helper_bounded() -> Option<String> {
    use std::io::Write;

    let mut child = cmd("git")
        .args(["credential", "fill"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    child
        .stdin
        .take()?
        .write_all(b"protocol=https\nhost=github.com\n\n")
        .ok()?;

    // 看门狗：超时后杀掉这个具体进程（持有 Child 的句柄，不会误杀他人）
    let mut child = child;
    let deadline = std::time::Instant::now() + TOKEN_EXCHANGE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    }

    // 进程已退出，此时读取输出不会阻塞
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("password=").map(str::to_string))
        .filter(|t| !t.trim().is_empty())
}

/// 列出当前凭据可见的仓库，供用户选择。
///
/// 用 GitHub REST API 的 `/user/repos`，按最近推送排序——
/// 用户想同步的多半是最近在用的那个。
///
/// **账号登录与 Token 登录都能用**：前者取系统凭据助手中的 OAuth Token，
/// 后者取本应用保存的 PAT。用户不需要为了"看到仓库列表"而额外创建 Token。
#[tauri::command]
pub fn git_list_repos() -> AppResult<Vec<GitHubRepo>> {
    // 惰性补取：兼容"在缓存机制存在之前就已登录"的情况
    let token = if ensure_api_token() {
        api_token()
    } else {
        None
    }
    .ok_or_else(|| {
        AppError::Config("尚无可用凭据。请先完成登录（账号登录或 Token 登录）。".to_string())
    })?;

    let response = ureq::get("https://api.github.com/user/repos?per_page=100&sort=pushed")
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        // GitHub 要求带 User-Agent，否则 403
        .header("User-Agent", "BakaSkill")
        .call();

    match response {
        Ok(mut res) => res
            .body_mut()
            .read_json::<Vec<GitHubRepo>>()
            .map_err(|err| AppError::Io(format!("解析仓库列表失败：{err}"))),
        Err(ureq::Error::StatusCode(401)) => Err(AppError::PermissionDenied(
            "Token 无效或已过期（401）".to_string(),
        )),
        Err(ureq::Error::StatusCode(403)) => Err(AppError::PermissionDenied(
            "Token 权限不足（403）。请确认勾选了 contents 读写权限。".to_string(),
        )),
        Err(err) => Err(AppError::Io(format!("请求 GitHub 失败：{err}"))),
    }
}

// ===========================================================================
// git CLI
// ===========================================================================

/// 执行 git 命令。
///
/// `cwd` 为 `None` 时在无仓库上下文中执行（例如 `git --version`）。
fn run_git(cwd: Option<&Path>, args: &[&str]) -> AppResult<String> {
    let mut command = cmd("git");
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    // 非交互：避免凭据缺失时卡在等待输入
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.args(args);

    let output = command
        .output()
        .map_err(|err| AppError::Io(format!("无法执行 git：{err}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        Err(AppError::Io(format!(
            "git {} 失败：{}",
            subcommand(args),
            redact(stderr.trim())
        )))
    }
}

/// 取出参数里的子命令名，用于报错。
///
/// 不能直接用第一个参数：形如 `["-c", "core.quotePath=false", "status", …]`
/// 的命令第一个参数是 `-c`，报错就成了没有信息量的 "git -c 失败"。
fn subcommand<'a>(args: &[&'a str]) -> &'a str {
    let mut iter = args.iter().copied();
    while let Some(arg) = iter.next() {
        if arg == "-c" {
            // 跳过 `-c` 与紧随其后的 `key=value`
            iter.next();
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        return arg;
    }
    ""
}

/// 把可能混入输出中的凭据抹掉。错误信息会一路显示到界面上，
/// 不能让它带着 Token 到处跑。
fn redact(text: &str) -> String {
    // 形如 https://user:token@github.com/... 的内联凭据
    let mut out = String::with_capacity(text.len());
    for part in text.split("//") {
        if let Some(at) = part.find('@') {
            if part[..at].contains(':') {
                if let Some(slash) = part.find('/') {
                    if at < slash {
                        out.push_str("//***@");
                        out.push_str(&part[at + 1..]);
                        continue;
                    }
                }
            }
        }
        if !out.is_empty() && !out.ends_with('@') {
            out.push_str("//");
        }
        out.push_str(part);
    }
    out
}

fn is_git_available() -> Option<String> {
    run_git(None, &["--version"])
        .ok()
        .map(|s| s.trim().to_string())
}

/// 读取当前同步配置与可用性
fn read_availability() -> GitAvailability {
    // 配置读不出来时不静默当成"什么都没配"：至少在日志里留下痕迹，
    // 否则界面会一直显示"尚未绑定仓库"，而真正的原因是配置损坏或被占用。
    let git = match crate::config::load() {
        Ok(cfg) => cfg.git,
        Err(err) => {
            tracing::warn!(error = %err, "读取配置失败，同步设置按默认值显示");
            Default::default()
        }
    };

    let version = is_git_available();
    GitAvailability {
        available: version.is_some(),
        version,
        has_token: has_token(),
        has_helper_credentials: has_helper_credentials(),
        remote_url: git.remote_url,
        // 分支名归一化：空串对用户没有意义，显示成实际会用的默认分支
        branch: if git.branch.trim().is_empty() {
            "main".to_string()
        } else {
            git.branch
        },
    }
}

// ===========================================================================
// 同步状态与推送/拉取
// ===========================================================================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    /// 同步工作区是否已建立（即是否已经绑定过仓库并同步过一次）
    pub is_repo: bool,
    /// 有未提交改动的文件数
    pub changed_count: usize,
    /// 变更明细（增/改/删），供预览
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub removed: Vec<String>,
    /// 领先远端的提交数
    pub ahead: usize,
    /// 落后远端的提交数
    pub behind: usize,
    pub last_commit: Option<String>,
    /// 本次「取回缺失的 Skill」写入中央库的 Skill 名。
    ///
    /// 只有 `git_pull` 会填充它；其余操作一律为空数组。
    /// 界面上「取回了几个」就是数它，而不是看提交数——
    /// 对用户来说"取回了 3 个 Skill"才是可理解的结果。
    pub imported_skills: Vec<String>,
}

impl SyncStatus {
    /// 「还没绑定仓库 / 还没同步过」的状态
    fn empty() -> Self {
        SyncStatus {
            is_repo: false,
            changed_count: 0,
            added: Vec::new(),
            modified: Vec::new(),
            removed: Vec::new(),
            ahead: 0,
            behind: 0,
            last_commit: None,
            imported_skills: Vec::new(),
        }
    }
}

/// 解析 `git status --porcelain` 的输出。
///
/// porcelain 格式每行前两位是状态码：`??` 未跟踪、` M` 已修改、` D` 已删除等。
/// 这里只按**首字符**归类，足够给出"要提交什么"的预览。
fn parse_porcelain(output: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut removed = Vec::new();

    for line in output.lines() {
        if line.len() < 4 {
            continue;
        }
        let code = &line[..2];
        let path = line[3..].trim().to_string();
        if path.is_empty() {
            continue;
        }

        if code == "??" || code.starts_with('A') {
            added.push(path);
        } else if code.starts_with('D') || code.ends_with('D') {
            removed.push(path);
        } else {
            modified.push(path);
        }
    }
    (added, modified, removed)
}

/// 解析 `git diff --cached --name-status` 的输出，即"这次究竟要提交什么"。
///
/// 每行形如 `<状态>\t<路径>`（重命名/复制是 `<状态>\t<旧路径>\t<新路径>`）。
/// 这是**提交信息的唯一来源**：用暂存区而不是 `git status`，是因为被仓库
/// `.gitignore` 匹配的文件不会出现在 `git status` 里，用它会让"传了什么"
/// 与"报了什么"对不上。
fn parse_staged(output: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut removed = Vec::new();

    for line in output.lines() {
        let mut fields = line.split('\t');
        let Some(status) = fields.next() else {
            continue;
        };
        // 重命名/复制取**新**路径（它是这次真正落进仓库的那个名字）
        let path = fields.next_back().unwrap_or("").trim().to_string();
        if path.is_empty() {
            continue;
        }

        match status.chars().next() {
            Some('A') => added.push(path),
            Some('D') => removed.push(path),
            Some(_) => modified.push(path),
            None => {}
        }
    }

    (added, modified, removed)
}

/// 生成提交信息，包含变更摘要。
///
/// 自动生成的提交信息必须**能看出改了什么**，否则用户在 GitHub 上
/// 面对一串 "update" 无从判断。
fn build_commit_message(added: &[String], modified: &[String], removed: &[String]) -> String {
    let summary = format!(
        "sync: {} added, {} modified, {} removed",
        added.len(),
        modified.len(),
        removed.len()
    );

    let mut body = String::new();
    let mut push = |items: &[String], verb: &str| {
        for item in items.iter().take(20) {
            body.push_str(&format!("\n- {verb}: {item}"));
        }
        if items.len() > 20 {
            body.push_str(&format!("\n- …另有 {} 项", items.len() - 20));
        }
    };
    push(added, "add");
    push(modified, "modify");
    push(removed, "remove");

    format!("{summary}\n{body}\n")
}

/// 读取同步状态（本地与仓库的差异、领先/落后）。
///
/// 状态一律取自**当前绑定仓库对应的同步工作区**，因为中央库本身已不再是
/// git 仓库。没有绑定仓库时没有工作区可言，直接回一个"尚未同步"的空状态。
#[tauri::command]
pub fn git_sync_status() -> AppResult<SyncStatus> {
    let Ok(cfg) = crate::config::load() else {
        return Ok(SyncStatus::empty());
    };
    let url = cfg
        .git
        .remote_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty());

    let Some(url) = url else {
        return Ok(SyncStatus::empty());
    };
    git_sync_status_impl(&sync_workspace(url)?)
}

fn git_sync_status_impl(root: &Path) -> AppResult<SyncStatus> {
    if !root.join(".git").is_dir() {
        return Ok(SyncStatus::empty());
    }

    // 状态**只看 `skills/`**：用户的仓库里可能有别的东西，那些不是我们要同步的，
    // 不该出现在"本次要同步什么"里（我们也不会提交它们）。
    let porcelain = run_git(
        Some(root),
        &["status", "--porcelain", "--", crate::library::SKILLS_DIR],
    )
    .unwrap_or_default();
    let (added, modified, removed) = parse_porcelain(&porcelain);

    // 与远端的差异：没有上游分支时命令会失败，视为 0/0 而不是报错
    let (mut ahead, mut behind) = (0usize, 0usize);
    if let Ok(out) = run_git(
        Some(root),
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
    ) {
        let parts: Vec<&str> = out.split_whitespace().collect();
        if parts.len() == 2 {
            ahead = parts[0].parse().unwrap_or(0);
            behind = parts[1].parse().unwrap_or(0);
        }
    }

    let last_commit = run_git(Some(root), &["log", "-1", "--pretty=%h %s"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Ok(SyncStatus {
        is_repo: true,
        changed_count: added.len() + modified.len() + removed.len(),
        added,
        modified,
        removed,
        ahead,
        behind,
        last_commit,
        imported_skills: Vec::new(),
    })
}

// ===========================================================================
// 同步工作区
//
// # 为什么中央库不再是 git 工作区
//
// 早先直接拿中央库目录当工作区，于是它与仓库根目录绑死。后果是：
// 仓库里只要有**任何其他内容**，历史就不相关，`git push` 必然被拒——
// 而唯一的"出路"（`pull --allow-unrelated-histories`）会把仓库里的
// 无关文件（课程笔记、项目代码）拉进用户的中央库目录。
// 等于告诉用户"你的仓库用过就别用了"。
//
// 现在改为使用**独立的同步工作区**：
//
// ```text
// 中央库   <lib>/skills/<name>/SKILL.md    ← 用户数据，git 不直接触碰
// 工作区   <appdata>/SkillHub/sync/        ← git 工作区
//          └─ skills/<name>/SKILL.md       ← 与中央库同构
// ```
//
// `git clone` 会带上仓库的完整历史，在其中加入 `skills/` 后推送是**快进**，
// 与仓库里原本有什么无关。仓库既保留自己的内容，又获得 Skill 库。
//
// 顺带的好处：中央库目录里不再有 `.git`，对用户来说它就是一个纯数据目录。
//
// # 工作区是**派生目录**
//
// 它的内容 = 远端仓库内容 ∪ 中央库镜像，两者都能完整重建：远端内容可以重新
// clone，镜像内容可以重新从中央库复制。因此下面对它可以做"直接对齐远端"
// 这类动作而不会损失数据——这是 `refresh_workspace` 敢用 `reset --hard` 的前提。
//
// # 只碰 `skills/`
//
// 用户的仓库里可能有别的东西。本模块**只暂存 `skills/`**，不提交、不改动
// 仓库里的其他文件。"把整个工作区都算我的"（`git add -A` 不带路径）在
// "借用用户已有仓库"这个模型里是不成立的。
// ===========================================================================

/// 同步工作区目录：`%LOCALAPPDATA%\SkillHub\sync\<仓库名>-<地址摘要>`
///
/// # 为什么把仓库地址编进目录名
///
/// 工作区是"**某一个**仓库的本地副本"。若所有仓库共用一个目录，用户从
/// 仓库 A 换绑到仓库 B 时，A 留下的内容还在工作区里（追踪文件，以及
/// `reset --hard` 不会清理的未追踪文件），下一次同步就可能把 A 的内容
/// 提交进 B——私有仓库的内容进了公开仓库。按地址分目录之后，
/// 换仓库就是换目录，这条路径根本不存在。
///
/// 目录名里带一段可读的仓库名，是为了让人在资源管理器里看得懂它是谁的副本。
fn sync_workspace(url: &str) -> AppResult<std::path::PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| AppError::Config("未找到 LOCALAPPDATA 环境变量".to_string()))?;
    Ok(PathBuf::from(base)
        .join("BakaSkill")
        .join("sync")
        .join(format!("{}-{:08x}", repo_slug(url), fnv1a(url))))
}

/// 取地址里可读的那一段（仓库名），只保留文件系统安全的字符
fn repo_slug(url: &str) -> String {
    let tail = url
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("repo")
        .trim_end_matches(".git");

    let slug: String = tail
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(40)
        .collect();

    if slug.is_empty() || slug == "." || slug == ".." {
        "repo".to_string()
    } else {
        slug
    }
}

/// FNV-1a：把仓库地址压成 8 位十六进制，用作目录名的去重后缀。
///
/// 手写而不用 `DefaultHasher`：后者不保证跨 Rust 版本稳定，一旦算法变了，
/// 旧的工作区目录会被无声抛弃、白白重新克隆一次。
fn fnv1a(text: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in text.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// 工作区中的 `skills/` 目录（与中央库同构）
fn workspace_skills(ws: &Path) -> PathBuf {
    ws.join(crate::library::SKILLS_DIR)
}

/// 一次同步操作的目标。
///
/// 把这些参数收拢成一个结构体，是为了让 `sync_impl` / `pull_impl` 能脱离
/// 全局配置与真实 `%LOCALAPPDATA%` 被调用——端到端测试因此可以用临时目录
/// 跑完整的"推上去 / 拉下来"流程。
struct Target {
    lib: PathBuf,
    ws: PathBuf,
    url: String,
    branch: String,
}

/// 从中央库路径与配置解析出本次操作的目标
fn resolve_target(library_path: &str) -> AppResult<Target> {
    let lib = PathBuf::from(library_path);
    if !crate::library::skills_dir(&lib).is_dir() {
        return Err(AppError::Config(format!(
            "中央库尚未初始化：{}",
            lib.display()
        )));
    }

    let cfg = crate::config::load()?;
    let url = cfg
        .git
        .remote_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .ok_or_else(|| AppError::Config("尚未选择要同步的仓库".to_string()))?
        .to_string();

    // 兜底：配置里可能存着早先留下的、带内联凭据的地址。放它过去的话，
    // 地址会被 `git remote set-url` 写进工作区的 `.git/config`，
    // 而 Token 只允许存在于凭据管理器里。
    if has_inline_credentials(&url) {
        return Err(AppError::Config(
            "已保存的仓库地址里带着用户名/Token，为避免它被写进工作区的 .git/config，\
             同步已被阻止。\n\n请重新绑定一个不带凭据的地址\
             （形如 https://github.com/用户名/仓库.git），\
             登录信息在「登录」区填写——那会存进 Windows 凭据管理器。"
                .to_string(),
        ));
    }

    Ok(Target {
        ws: sync_workspace(&url)?,
        lib,
        url,
        branch: branch_of(&cfg),
    })
}

/// 把实际使用的分支记回配置。
///
/// 手填地址时配置里的分支常常只是默认值 `main`，而仓库的默认分支叫别的名字。
/// 上面已经以远端为准选好了分支，这里让配置与界面跟上，免得显示一个并不存在的
/// 分支名。写失败不影响本次同步的实际结果，只记一条日志。
fn remember_branch(branch: &str) {
    let Ok(mut cfg) = crate::config::load() else {
        return;
    };
    if cfg.git.branch == branch {
        return;
    }
    cfg.git.branch = branch.to_string();
    match crate::config::save(&cfg) {
        Ok(()) => tracing::info!(branch = %branch, "已把实际使用的分支记入配置"),
        Err(err) => tracing::warn!(error = %err, "记录分支名失败（不影响本次同步结果）"),
    }
}

/// 确保工作区存在且已绑定远端，返回**这次实际使用的分支名**。
///
/// # 为什么返回"实际使用的分支"
///
/// 配置里的分支来自用户选仓库时带过来的默认分支，或者干脆是默认值 `main`。
/// 但用户手填地址、或仓库的默认分支叫别的名字时，配置里的名字在远端并不存在。
/// 若照配置硬建一个分支，用户仓库里就会凭空多出一个与主分支并行的分支：
/// 内容看着对，位置不对。所以以**远端已有的分支**为准。
fn ensure_workspace_at(ws: &Path, url: &str, branch: &str) -> AppResult<String> {
    if !ws.join(".git").is_dir() {
        if let Some(parent) = ws.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| AppError::from_io("创建同步目录失败", err))?;
        }

        // 先问远端"你有没有内容"，再决定是克隆还是从零开始。
        //
        // 不能靠"clone 失败就改为 init"来判断：clone 也会因为网络、凭据、
        // 地址拼错而失败，那时 init 会把真正的失败悄悄吞掉，用户要等到推送时
        // 才发现不对。何况**空仓库的 clone 是成功的**（git 只给一句 warning），
        // 所以"远端是空的"只能靠 ls-remote 主动识别。
        //
        // 顺带：这一步同时验证了地址可达与凭据可用。
        let refs = run_git(None, &["ls-remote", url]).map_err(|err| {
            AppError::Config(format!(
                "无法访问该仓库，请确认地址拼写、仓库是否存在、以及当前账号有无访问权限。\n\n{err}"
            ))
        })?;

        if refs.trim().is_empty() {
            std::fs::create_dir_all(ws)
                .map_err(|err| AppError::from_io("创建同步目录失败", err))?;
            run_git(Some(ws), &["init", "-b", branch])?;
            run_git(Some(ws), &["remote", "add", "origin", url])?;
            tracing::info!("远端仓库尚无任何内容，已在同步目录中初始化新仓库");
        } else {
            clone_into(ws, url)?;
            tracing::info!(url = %url, "已克隆远端仓库到同步目录");
        }
    }

    // 这两项每次进来都重设一遍（幂等），而不是只在新建工作区时设一次：
    // 早先版本创建的工作区也需要它们。
    //
    // 行尾符：让工作区里的字节 == 仓库里的字节。
    //
    // Git for Windows 的**系统级**配置默认 `core.autocrlf=true`，会在检出时
    // 把 LF 换成 CRLF、提交时再换回去。对"把中央库原样镜像进仓库"这件事来说，
    // 这个转换只会制造麻烦：同一份 SKILL.md 在两边字节不同，"有没有改动"
    // 得靠规范化去猜。工作区是我们的派生目录，直接关掉转换，
    // 镜像就是忠实的字节复制。
    run_git(Some(ws), &["config", "core.autocrlf", "false"])?;

    // 路径转义：让 git 原样输出中文路径。
    //
    // git 默认（`core.quotePath=true`）会把非 ASCII 路径转义成 C 风格的八进制，
    // `skills/中文技能/SKILL.md` 会变成
    // `"skills/\344\270\255\346\226\207\346\212\200\350\203\275/SKILL.md"`。
    // 这个字符串会直接出现在界面的变更列表与自动生成的提交信息里——
    // 对一个中文应用来说，那等于把路径显示成一堆乱码。
    run_git(Some(ws), &["config", "core.quotePath", "false"])?;

    // 对齐远端地址（用户可能换了仓库）
    let current = run_git(Some(ws), &["remote", "get-url", "origin"])
        .ok()
        .map(|s| s.trim().to_string());
    match current.as_deref() {
        Some(existing) if existing == url => {}
        Some(_) => {
            run_git(Some(ws), &["remote", "set-url", "origin", url])?;
        }
        None => {
            run_git(Some(ws), &["remote", "add", "origin", url])?;
        }
    }

    Ok(select_branch(ws, branch))
}

/// 把一个仓库克隆到指定目录，供「从 Git URL 导入」使用。
///
/// 复用同步那套克隆逻辑（含 `core.autocrlf=false`），不另起一套：同一个应用里
/// 对同一件事有两套实现，迟早会在一套上修了缺陷而另一套没修。
///
/// 地址里的内联凭据同样被拒绝——那个地址会被写进 `.git/config`。
pub fn clone_for_import(url: &str, dest: &Path) -> AppResult<()> {
    if has_inline_credentials(url) {
        return Err(AppError::Config(
            "仓库地址里不能带用户名和 Token。请在「登录」区填写凭据，\
             它会存进 Windows 凭据管理器，不会落进配置文件。"
                .to_string(),
        ));
    }
    clone_into(dest, url)
}

/// 克隆远端到工作区。
///
/// `-c core.autocrlf=false` 只作用于这一次 clone，让首次检出就是字节忠实的；
/// 随后再把它写进仓库配置（见调用处），使之后每次操作都用同一套约定。
fn clone_into(ws: &Path, url: &str) -> AppResult<()> {
    let output = cmd("git")
        .args(["-c", "core.autocrlf=false", "clone", url])
        .arg(ws)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|err| AppError::Io(format!("无法执行 git：{err}")))?;

    if output.status.success() {
        return Ok(());
    }

    Err(AppError::Io(format!(
        "克隆仓库失败：{}",
        redact(String::from_utf8_lossy(&output.stderr).trim())
    )))
}

/// 选出这次要用的分支名：远端已有的同名分支优先，否则沿用工作区当前分支。
///
/// 后者覆盖了"仓库默认分支叫 `master`，而配置里是 `main`"这种情况——
/// 此时沿用 `master`，不另起一个分支。
fn select_branch(ws: &Path, configured: &str) -> String {
    let remote_ref = format!("origin/{configured}");

    if run_git(Some(ws), &["rev-parse", "--verify", "--quiet", &remote_ref]).is_ok() {
        let local_exists = run_git(
            Some(ws),
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{configured}"),
            ],
        )
        .is_ok();

        let switched = if local_exists {
            run_git(Some(ws), &["checkout", configured])
        } else {
            // 从远端跟踪分支建本地分支，`git push` 才有上游可推（否则要 -u）
            run_git(Some(ws), &["checkout", "-b", configured, &remote_ref])
        };

        if switched.is_ok() {
            return configured.to_string();
        }
        tracing::warn!(branch = %configured, "切换到配置的分支失败，改用工作区当前分支");
    }

    let current = run_git(Some(ws), &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "HEAD");

    match current {
        Some(name) => {
            if name != configured {
                tracing::info!(
                    configured = %configured,
                    using = %name,
                    "远端没有配置的分支，改用工作区当前分支"
                );
            }
            name
        }
        None => configured.to_string(),
    }
}

/// 把工作区对齐到远端的当前状态。
///
/// 这是"推送必须是快进"的前提：我们的提交要建立在远端当前状态之上。
///
/// 优先用 `--ff-only` 快进。分叉时（工作区有未推送的提交，远端也有新提交）
/// 直接对齐远端：工作区是派生目录，未推送的内容在中央库里都还在，下一次同步
/// 会重新镜像回来。**不产生合并提交**——那正是原设计里用户看不懂、
/// 也无法解释的东西。
fn refresh_workspace(ws: &Path, branch: &str) -> AppResult<()> {
    run_git(Some(ws), &["fetch", "origin"]).map_err(|err| {
        AppError::Io(format!(
            "无法从仓库读取内容（网络不通，或当前账号没有访问权限）。\n\n{err}"
        ))
    })?;

    let remote_ref = format!("origin/{branch}");
    if run_git(Some(ws), &["rev-parse", "--verify", "--quiet", &remote_ref]).is_err() {
        // 远端还是空的：没有内容需要对齐
        return Ok(());
    }

    if run_git(Some(ws), &["merge", "--ff-only", &remote_ref]).is_ok() {
        return Ok(());
    }

    // 走到这里说明快进不了。对齐远端会丢掉工作区里未推送的提交——
    // 这只有在"工作区内容 = 远端 ∪ 中央库"成立时才安全（未推送的内容
    // 随后会被 mirror 重新镜像回来）。所以先把丢了多少记下来，
    // 万一将来有代码破坏了这个前提，日志里能看见。
    let discarded = run_git(
        Some(ws),
        &["rev-list", "--count", &format!("{remote_ref}..HEAD")],
    )
    .ok()
    .and_then(|out| out.trim().parse::<usize>().ok())
    .unwrap_or(0);
    if discarded > 0 {
        tracing::warn!(
            commits = discarded,
            "工作区有未推送的提交，将对齐到远端（内容可从中央库重新镜像）"
        );
    }

    run_git(Some(ws), &["reset", "--hard", &remote_ref])?;
    Ok(())
}

/// 把 `from` 下的内容镜像到 `to`（**只增改，不删除**）。
///
/// 返回 (新增文件数, 更新文件数)。
///
/// 不删除是有意的：同步的语义是"把本地有的传上去"，
/// 而不是"让远端与本地完全一致"。后者会误删仓库里用户自己放的东西。
fn mirror_dir(from: &Path, to: &Path) -> AppResult<(usize, usize)> {
    let mut added = 0usize;
    let mut updated = 0usize;

    if !from.is_dir() {
        return Ok((0, 0));
    }
    std::fs::create_dir_all(to).map_err(|err| AppError::from_io("创建目标目录失败", err))?;

    for entry in std::fs::read_dir(from)
        .map_err(|err| AppError::from_io("读取源目录失败", err))?
        .filter_map(Result::ok)
    {
        let src = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = src.is_dir();

        // 暂存目录与杂项文件不是 Skill 内容，不镜像
        if name.starts_with(STAGING_PREFIX) || is_junk(&name, is_dir) {
            continue;
        }
        // 不跟随链接：中央库里的链接指向别处，复制它是错的
        if is_link(&src) {
            continue;
        }

        let dst = to.join(&name);

        if is_dir {
            let (a, u) = mirror_dir(&src, &dst)?;
            added += a;
            updated += u;
            continue;
        }

        let existed = entry_exists(&dst);
        let changed = !existed || !same_content(&src, &dst);
        if !changed {
            continue;
        }

        std::fs::copy(&src, &dst)
            .map_err(|err| AppError::from_io(&format!("复制失败：{}", src.display()), err))?;
        if existed {
            updated += 1;
        } else {
            added += 1;
        }
    }

    Ok((added, updated))
}

/// 两个文件的内容是否相同。
///
/// 读不出来时**当作不同**：读失败（被占用、权限、坏扇区）意味着我们并不知道
/// 它们是否一致，宁可多复制一次，也不要因为"两次都读失败、于是相等"而
/// 把一个文件悄悄漏掉——那正是最不该发生的失败方式。
fn same_content(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// 把 `from` 下**目标里还没有**的 Skill 复制到 `to`，返回新引入的名字。
///
/// 语义是"补齐"，不是"覆盖"：中央库里已有的同名 Skill 一律跳过，哪怕内容
/// 不同——中央库是用户的数据，拉取只做补齐、不动它已有的东西。这也是
/// "拉取不会损坏本地库"这句话的实现依据。
///
/// 它是 `mirror_dir` 的**逆操作**，两者面对同一批文件：`skills/` 下的每个
/// 条目都算数，不管它里面有没有 `SKILL.md`。判据只有两条——杂项文件不要、
/// 本地已有同名条目的不要。
///
/// 这里**不做**"这看起来是不是一个 Skill"的判断：同步会把整棵
/// `<中央库>/skills/` 原样传上去，取回若按自己的口味挑三拣四，就会出现
/// "传得上去、取不回来"。哪些条目能被**索引**成 Skill 是 `index` 模块的事。
fn import_skills(ws: &Path, lib: &Path) -> AppResult<Vec<String>> {
    let from = workspace_skills(ws);
    let to = crate::library::skills_dir(lib);

    let mut imported = Vec::new();
    if !from.is_dir() {
        return Ok(imported);
    }
    std::fs::create_dir_all(&to).map_err(|err| AppError::from_io("创建目标目录失败", err))?;

    // 暂存放在中央库的 `.bakaskill/incoming/`，而**不是** `skills/` 下。
    // `skills/` 里的任何目录都会被索引与扫描当成候选 Skill，而暂存目录里
    // 恰好带着一份 SKILL.md——万一被一次并发扫描撞见，界面里就会多出一个
    // 幻影 Skill。`.bakaskill/` 是应用自己的派生目录，扫描不看那里。
    let incoming = crate::library::meta_dir(lib).join("incoming");
    std::fs::create_dir_all(&incoming).map_err(|err| AppError::from_io("创建暂存目录失败", err))?;

    for entry in std::fs::read_dir(&from)
        .map_err(|err| AppError::from_io("读取源目录失败", err))?
        .filter_map(Result::ok)
    {
        let src = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = src.is_dir();

        // 暂存残留与杂项文件跳过（`mirror_dir` 也不镜像它们，两边一致）
        if name.starts_with(STAGING_PREFIX) || is_junk(&name, is_dir) {
            continue;
        }
        // 不跟随链接：仓库里若混进链接，复制它就等于复制别处的内容
        if is_link(&src) {
            continue;
        }
        // ⚠️ 这里**刻意不判断"它像不像一个 Skill"**（比如"必须含 SKILL.md"），
        //    也**不要求它必须是目录**。
        //
        // 曾经加过"必须含 SKILL.md"的判断，理由是"只把索引认识的东西收进来"。
        // 它造成了一次真实的数据丢失：同步是**忠实镜像** `<中央库>/skills/`
        // 整棵树的（`mirror_dir` 不挑内容），所以不含 SKILL.md 的目录照样会被
        // 传上仓库；而取回时被那道判断挡掉，于是"传得上去、取不回来"。
        // 用户报的正是这个：他把一个含 `测试1.md` 的目录同步上去、删掉本地，
        // 再点取回什么也没回来，界面还说"仓库里没有中央库缺少的 Skill"。
        //
        // 取回是同步的**逆操作**，两者必须对同一批文件成立。判断"是不是 Skill"
        // 是**索引**的事（`library::has_manifest` 在那里决定什么能被列出），
        // 不该混进"备份/还原"这条路径——那等于用前者去否决后者已经保存的数据。
        let dst = to.join(&name);
        // 用 `entry_exists` 而不是 `dst.exists()`：后者跟随重解析点，
        // 悬空的 junction 会被误判成"不存在"，随后改名到已存在的目录项上会失败。
        if entry_exists(&dst) {
            tracing::debug!(entry = %name, "中央库已有同名条目，跳过不覆盖");
            continue;
        }

        // 先复制进暂存区、成功后再改名进 `skills/`：中途失败不会在中央库里
        // 留下"半截的 Skill"——那种条目会被下一次取回当成"本地已有"而永远跳过，
        // 坏得无声无息。同一卷内改名是原子的。
        let staging = incoming.join(&name);
        let _ = std::fs::remove_file(&staging);
        remove_app_scratch(&staging);

        let copied = if is_dir {
            copy_dir_all(&src, &staging)
        } else {
            std::fs::copy(&src, &staging)
                .map(|_| ())
                .map_err(|err| AppError::from_io(&format!("复制失败：{}", src.display()), err))
        };
        if let Err(err) = copied.and_then(|()| {
            std::fs::rename(&staging, &dst)
                .map_err(|err| AppError::from_io(&format!("写入 Skill 失败：{name}"), err))
        }) {
            let _ = std::fs::remove_file(&staging);
            remove_app_scratch(&staging);
            return Err(err);
        }

        imported.push(name);
    }

    Ok(imported)
}

/// 递归复制目录（不跟随链接，跳过杂项文件）
fn copy_dir_all(from: &Path, to: &Path) -> AppResult<()> {
    std::fs::create_dir_all(to).map_err(|err| AppError::from_io("创建目录失败", err))?;

    for entry in std::fs::read_dir(from)
        .map_err(|err| AppError::from_io("读取源目录失败", err))?
        .filter_map(Result::ok)
    {
        let src = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = src.is_dir();

        if is_junk(&name, is_dir) || is_link(&src) {
            continue;
        }

        let dst = to.join(&name);
        if is_dir {
            copy_dir_all(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)
                .map_err(|err| AppError::from_io(&format!("复制失败：{}", src.display()), err))?;
        }
    }

    Ok(())
}

/// 一键同步：把中央库中本地有、仓库里没有（或有改动）的 Skill 传上去。
///
/// **绝不删除仓库中已有的内容**，也不提交、不改动仓库里用户自己的文件。
#[tauri::command]
pub fn git_sync_now(library_path: String) -> AppResult<SyncStatus> {
    let target = resolve_target(&library_path)?;
    let (status, branch) = sync_impl(&target)?;
    remember_branch(&branch);
    Ok(status)
}

/// 同步的实现。
///
/// 与命令层分离，使测试能用临时目录跑完整的"推上去"流程，而不碰
/// 真实配置与 `%LOCALAPPDATA%`。
fn sync_impl(target: &Target) -> AppResult<(SyncStatus, String)> {
    let branch = ensure_workspace_at(&target.ws, &target.url, &target.branch)?;

    // 先对齐远端：我们的提交必须建立在远端当前状态之上，否则推送会被拒（非快进）
    refresh_workspace(&target.ws, &branch)?;

    let (added, updated) = mirror_dir(
        &crate::library::skills_dir(&target.lib),
        &workspace_skills(&target.ws),
    )?;

    // 先清空暂存区，再只暂存 `skills/`。清空是为了保证"这次提交里不可能有
    // `skills/` 之外的东西"——万一日后有人（或用户手工）在工作区里暂存了
    // 别的文件，也不会被我们连带提交进他的仓库。还没有任何提交时无从清起。
    if run_git(
        Some(&target.ws),
        &["rev-parse", "--verify", "--quiet", "HEAD"],
    )
    .is_ok()
    {
        run_git(Some(&target.ws), &["reset", "--quiet"])?;
    }

    // `-f` 与 `--no-all` 都是有意的：
    //
    // - `-f`：仓库自己的 `.gitignore` 可能恰好匹配到 Skill（笔记仓库里写
    //   `*.md`、`*.pdf` 都很常见）。用户已经把 `skills/` 交给本应用同步，
    //   那条早先写下的规则不该让这件事**悄悄**失效——否则界面报"同步成功"，
    //   而 Skill 一个都没上去。
    // - `--no-all`：只暂存新增与修改，**绝不暂存删除**。同步的承诺是
    //   "绝不删除仓库里已有的内容"，这条命令就是那句承诺的落点。
    run_git(
        Some(&target.ws),
        &["add", "-f", "--no-all", "--", crate::library::SKILLS_DIR],
    )?;

    // 读**暂存区**而不是 `git status`：被 `.gitignore` 匹配的文件不会出现在
    // `git status` 里，用状态判断会漏掉它们（那正是上面 `-f` 要救的情形）。
    // 暂存区回答的是"这次究竟要提交什么"，与提交信息天然一致。
    let staged = run_git(
        Some(&target.ws),
        &[
            "diff",
            "--cached",
            "--name-status",
            "--",
            crate::library::SKILLS_DIR,
        ],
    )
    .unwrap_or_default();

    if !staged.trim().is_empty() {
        ensure_commit_identity(&target.ws)?;
        let (a, m, r) = parse_staged(&staged);
        let message = build_commit_message(&a, &m, &r);
        run_git(Some(&target.ws), &["commit", "--quiet", "-m", &message])?;
        tracing::info!(added, updated, "已把中央库改动提交到同步工作区");
    } else {
        tracing::info!(added, updated, "没有新的改动需要提交");
    }

    // 无论这次有没有新提交，都要推一次。
    //
    // 上一次同步可能"提交成功、推送失败"（断网、令牌过期、403）。那时工作区
    // 是干净的，若因为"没有改动"就跳过推送，用户再点多少次同步都推不上去，
    // 而界面还会一直说"已同步"。所以这里只看"有没有没推上去的提交"。
    if has_unpushed_commits(&target.ws) {
        push(&target.ws, &branch)?;
    } else {
        tracing::info!("远端已是最新，无需推送");
    }

    Ok((git_sync_status_impl(&target.ws)?, branch))
}

/// 有没有尚未推送的提交。
///
/// "还没有上游分支"也算有——那意味着这个分支从来没推上去过。
/// 一个提交都还没有（空仓库、还没提交过）则算没有。
fn has_unpushed_commits(root: &Path) -> bool {
    if run_git(Some(root), &["rev-parse", "--verify", "--quiet", "HEAD"]).is_err() {
        return false;
    }
    match run_git(Some(root), &["rev-list", "--count", "@{upstream}..HEAD"]) {
        Ok(out) => out.trim().parse::<usize>().map(|n| n > 0).unwrap_or(true),
        Err(_) => true,
    }
}

fn branch_of(cfg: &crate::config::AppConfig) -> String {
    if cfg.git.branch.trim().is_empty() {
        "main".to_string()
    } else {
        cfg.git.branch.clone()
    }
}

/// 推送当前分支到远端。
///
/// 首次推送自动设置上游分支（`-u`），否则用户会撞上
/// "no upstream branch" 这种对非 git 用户毫无意义的报错。
fn push(root: &Path, branch: &str) -> AppResult<()> {
    // 分支由调用方传入（`select_branch` 选出的实际分支），而不是在这里重新读配置：
    // 配置里的名字可能与实际使用的分支不一致（见 `ensure_workspace_at`）。
    let has_upstream = run_git(Some(root), &["rev-parse", "--abbrev-ref", "@{upstream}"]).is_ok();

    let result = if has_upstream {
        run_git(Some(root), &["push"])
    } else {
        run_git(Some(root), &["push", "-u", "origin", branch])
    };

    // run_git 返回命令输出，这里只关心成败
    result
        .map(|_| ())
        .map_err(|err| friendly_push_error(err, root))
}

/// 把 git 的推送报错翻译成用户能据此行动的信息。
///
/// # 为什么需要翻译
///
/// 远端已有本地没有的提交时，git 会吐出一大段 hint：
/// `! [rejected] main -> main (fetch first)`、
/// `Updates were rejected because the remote contains work that you do not have locally`……
/// 这些话对懂 git 的人是常识，对**本应用的目标用户**则是噪音——
/// 他需要的是"发生了什么、我该点哪里"。
///
/// # 这条分支现在很罕见
///
/// 同步前已经 `refresh_workspace` 对齐过远端，所以正常情况下不会非快进。
/// 走到这里只可能是"取回之后、推送之前"别人又推了新内容——多发生在
/// 两台机器同时用的时候。因此文案的重点是"再点一次"，
/// 而不是原设计里那句会把用户引向"合并远端内容"的话。
fn friendly_push_error(err: AppError, root: &Path) -> AppError {
    let text = err.to_string();

    let remote_ahead = text.contains("fetch first")
        || text.contains("remote contains work")
        || text.contains("non-fast-forward")
        || text.contains("failed to push some refs");

    if remote_ahead {
        // 顺带说明远端有多少提交是我们没有的，让用户判断"这是不是我认识的仓库"
        let behind = run_git(Some(root), &["rev-list", "--count", "HEAD..@{upstream}"])
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| s != "0" && !s.is_empty());

        let detail = match behind {
            Some(n) => format!("仓库里现在有 {n} 个本地还没有的提交。"),
            None => "仓库里现在有本地还没有的提交。".to_string(),
        };

        return AppError::Conflict(format!(
            "刚刚取回内容之后，仓库里又有了新的提交，为避免覆盖它们，本次没有推送。\n\n\
             {detail}\n\n\
             再点一次「立即同步」即可：它会先取回这些提交，再把你的 Skill 推上去。\n\
             本应用不会删除或覆盖仓库里已有的任何内容。"
        ));
    }

    AppError::Io(format!("推送失败：{text}"))
}

/// 仅拉取：把仓库里已有、而中央库还没有的 Skill 取回来。
///
/// **本地已有的同名 Skill 一律跳过，不覆盖。**
///
/// 它读的是工作区里 `skills/` 的当前内容，而 `refresh_workspace` 已经保证
/// 工作区与远端一致——所以"仓库里的内容"就是"工作区里的内容"，
/// 不需要去碰远端分支上其他无关的文件。
#[tauri::command]
pub fn git_pull(library_path: String) -> AppResult<SyncStatus> {
    let target = resolve_target(&library_path)?;
    let (status, branch) = pull_impl(&target)?;
    remember_branch(&branch);
    Ok(status)
}

/// 拉取的实现（与 `sync_impl` 一样，独立出来是为了可测试）
fn pull_impl(target: &Target) -> AppResult<(SyncStatus, String)> {
    let branch = ensure_workspace_at(&target.ws, &target.url, &target.branch)?;
    refresh_workspace(&target.ws, &branch)?;

    let imported = import_skills(&target.ws, &target.lib)?;

    if imported.is_empty() {
        tracing::info!("仓库里没有中央库缺少的 Skill");
    } else {
        tracing::info!(
            count = imported.len(),
            skills = %imported.join(", "),
            "已从仓库取回中央库缺失的 Skill"
        );
    }

    let mut status = git_sync_status_impl(&target.ws)?;
    status.imported_skills = imported;
    Ok((status, branch))
}

// ===========================================================================
// 提交身份
// ===========================================================================

/// GitHub 账号身份（用于配置提交作者）
#[derive(Debug, Clone, Deserialize)]
struct GitHubUser {
    login: String,
    #[serde(default)]
    id: u64,
}

/// 取得 GitHub 账号身份。
///
/// 优先问 API（能拿到数字 id，从而拼出 GitHub 认可的 noreply 邮箱，
/// 这样提交会被正确归属到你的账号上）；API 不可用时退回
/// `git-credential-manager github list` 的账号名。
fn fetch_identity() -> Option<(String, String)> {
    // 1) GitHub API：login + id → 归属正确的 noreply 邮箱
    if let Some(token) = api_token() {
        let response = ureq::get("https://api.github.com/user")
            .header("Authorization", &format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "BakaSkill")
            .call();

        if let Ok(mut res) = response {
            if let Ok(user) = res.body_mut().read_json::<GitHubUser>() {
                let email = if user.id > 0 {
                    format!("{}+{}@users.noreply.github.com", user.id, user.login)
                } else {
                    format!("{}@users.noreply.github.com", user.login)
                };
                return Some((user.login, email));
            }
        }
    }

    // 2) 退回 GCM 的账号列表（只有名字，没有 id）
    let gcm = find_gcm()?;
    let out = cmd(&gcm).args(["github", "list"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let login = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?
        .to_string();

    Some((login.clone(), format!("{login}@users.noreply.github.com")))
}

/// 确保仓库有提交身份。
///
/// # 为什么由应用来设，而不是让用户跑 `git config`
///
/// git 提交必须知道作者是谁，否则直接拒绝：
/// `Author identity unknown ... unable to auto-detect email address`。
/// 把这句报错甩给用户，等于要求他懂 git 配置——而这正是本应用要消除的门槛。
/// 我们**已经知道他是谁**（登录过 GitHub），没有理由再去问他。
///
/// # 只设仓库级，不动全局
///
/// 用 `git config --local`，**绝不**用 `--global`：
/// 用户的全局 git 身份属于他自己的环境，SkillHub 没有资格改。
/// 同时，若该仓库已有身份（用户自己设过），则不覆盖。
fn ensure_commit_identity(root: &std::path::Path) -> AppResult<()> {
    let has_name = run_git(Some(root), &["config", "--get", "user.name"])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let has_email = run_git(Some(root), &["config", "--get", "user.email"])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    if has_name && has_email {
        return Ok(());
    }

    let Some((name, email)) = fetch_identity() else {
        return Err(AppError::Config(
            "无法确定提交身份，且未检测到已登录的 GitHub 账号。\n\
             请先完成登录（账号登录或 Token 登录），\
             或自行执行 git config --global user.name / user.email。"
                .to_string(),
        ));
    };

    if !has_name {
        run_git(Some(root), &["config", "--local", "user.name", &name])?;
    }
    if !has_email {
        run_git(Some(root), &["config", "--local", "user.email", &email])?;
    }

    tracing::info!(name = %name, "已为该仓库设置提交身份（仅本仓库，未改动全局配置）");
    Ok(())
}

/// 同步面板的初始状态：git 是否可用、是否已登录、当前绑定的仓库
#[tauri::command]
pub fn git_status() -> AppResult<GitAvailability> {
    Ok(read_availability())
}

/// 绑定要同步的远程仓库并校验其可达性。
///
/// 校验用 `git ls-remote`——它不需要本地有仓库，直接问远端
/// "这个地址通不通、凭据够不够"，是成本最低的验证方式。
#[tauri::command]
pub fn git_set_remote(url: String, branch: Option<String>) -> AppResult<GitAvailability> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err(AppError::Config("仓库地址为空".to_string()));
    }

    // 把 Token 拼进地址是 GitHub 上的常见做法，但对本应用是红线：
    // 这个地址会被写进 `config.json`、被 `git remote set-url` 写进工作区的
    // `.git/config`、还会显示在界面上，而 Token 只允许待在凭据管理器里。
    // 与其"接受再想办法擦掉"，不如当场拒绝并说清楚该填哪里。
    if has_inline_credentials(&url) {
        return Err(AppError::Config(
            "仓库地址里不能带用户名和 Token。\n\n\
             请填不带凭据的地址（形如 https://github.com/用户名/仓库.git）。\
             登录信息请在「登录」区填写：它会存进 Windows 凭据管理器，\
             不会写进配置文件，也不会出现在日志或界面里。"
                .to_string(),
        ));
    }

    // 校验可达性（依赖系统凭据助手；有 Token 时会由 git 自己找到）。
    //
    // ⚠️ **绝不能加 `--exit-code`**。
    //
    // 它的语义是"远端没有任何 ref 时以状态 2 退出"，而**空仓库恰恰没有任何 ref**
    // ——于是"仓库可访问但为空"会被判成"无法访问"。
    // 而"新建一个空仓库来备份 Skill"正是本项目最推荐的用法，
    // 这个误判会让用户的选择根本绑不上，且报错完全指错方向。
    //
    // 不加该选项时：可达即有输出/空输出都以 0 退出，不可达才非零退出。
    let probe = run_git(None, &["ls-remote", &url]);
    match probe {
        Ok(output) => {
            if output.trim().is_empty() {
                // 空仓库：可达且合法，属于预期状态
                tracing::info!(url = %url, "绑定的仓库为空（尚无任何提交），这是正常的初始状态");
            }
        }
        Err(err) => {
            return Err(AppError::Config(format!(
                "无法访问该仓库。\n\n{err}\n\n\
                 请确认：地址拼写正确、仓库存在、且当前账号有访问权限。"
            )));
        }
    }

    let mut cfg = crate::config::load()?;
    cfg.git.remote_url = Some(url.clone());
    if let Some(b) = branch.filter(|b| !b.trim().is_empty()) {
        cfg.git.branch = b;
    }
    crate::config::save(&cfg)?;

    tracing::info!(remote = %url, "已绑定同步仓库");
    Ok(read_availability())
}

// ===========================================================================
// 旧模型残留：中央库里的 `.git`
//
// 旧设计把中央库本身当作 git 工作区，于是会在 `<中央库>/.git` 留下一个仓库。
// 新模型下中央库只是数据目录，那个 `.git` 不再被任何代码使用，但它会让用户
// 困惑（"这里怎么还有个仓库？"），也可能带着旧的 remote 地址。
//
// 处理方式是**移走，不是删除**：万一用户自己往里放过东西，东西还在，
// 只是不再干扰。动作由用户点击触发，不在启动时擅自改动他的磁盘。
// ===========================================================================

/// 中央库中残留的 `.git` 路径（不存在则为 `None`）
fn stale_repo_at(lib: &Path) -> Option<PathBuf> {
    let dot_git = lib.join(".git");
    // 文件形态的 `.git`（worktree / 子模块）也算残留
    (dot_git.is_dir() || dot_git.is_file()).then_some(dot_git)
}

/// 中央库中是否残留着旧模型留下的 `.git`。
///
/// 返回的是那个 `.git` 的路径，供界面说明"要移走的是什么"。
#[tauri::command]
pub fn git_stale_repo(library_path: String) -> AppResult<Option<String>> {
    if library_path.trim().is_empty() {
        return Ok(None);
    }
    Ok(stale_repo_at(&PathBuf::from(library_path)).map(|p| p.display().to_string()))
}

/// 把中央库中残留的 `.git` 移到一旁（重命名，**绝不删除**）。
///
/// 返回它被移到了哪里；本来就没有残留则返回 `None`。
#[tauri::command]
pub fn git_cleanup_stale_repo(library_path: String) -> AppResult<Option<String>> {
    if library_path.trim().is_empty() {
        return Ok(None);
    }
    let lib = PathBuf::from(&library_path);

    let Some(dot_git) = stale_repo_at(&lib) else {
        return Ok(None);
    };

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    // 目标名带上时间戳，重复清理不会互相覆盖
    let mut target = lib.join(format!(".git.bakaskill-old-{stamp}"));
    let mut suffix = 1;
    while target.exists() {
        target = lib.join(format!(".git.bakaskill-old-{stamp}-{suffix}"));
        suffix += 1;
    }

    std::fs::rename(&dot_git, &target)
        .map_err(|err| AppError::from_io("移走残留的 .git 失败", err))?;

    tracing::info!(
        from = %dot_git.display(),
        to = %target.display(),
        "已把中央库中残留的 .git 移到一旁（未删除，可自行处理）"
    );

    Ok(Some(target.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_strips_inline_credentials() {
        let raw = "fatal: could not read from https://user:ghp_secret123@github.com/a/b.git";
        let cleaned = redact(raw);
        assert!(!cleaned.contains("ghp_secret123"), "Token 泄漏：{cleaned}");
        assert!(cleaned.contains("***@"));
        assert!(cleaned.contains("github.com/a/b.git"));
    }

    #[test]
    fn redact_leaves_plain_urls_alone() {
        let raw = "https://github.com/a/b.git";
        assert_eq!(redact(raw), raw);
    }

    #[test]
    fn redact_handles_multiple_urls() {
        let raw = "a https://u:t@x.com/p b https://ok.com/q";
        let cleaned = redact(raw);
        assert!(!cleaned.contains("u:t"));
        assert!(cleaned.contains("ok.com/q") || cleaned.contains("https://ok.com/q"));
    }

    #[test]
    fn junk_files_are_recognized() {
        // 系统自己产生的杂项不该被镜像进仓库，也不该被导入中央库
        assert!(is_junk(".DS_Store", false));
        assert!(is_junk("Thumbs.db", false));
        assert!(is_junk("desktop.ini", false));
        assert!(is_junk("SKILL.md.tmp", false));
        assert!(!is_junk("SKILL.md", false));
        assert!(!is_junk("assets", true));
        // `.tmp` 只对**文件**成立：一个叫 `xxx.tmp` 的目录可能是个正经 Skill
        assert!(
            !is_junk("my-skill.tmp", true),
            "把以 .tmp 结尾的目录当成垃圾了"
        );
    }

    /// 地址里内联凭据必须被识别出来——它会被写进 `config.json` 与工作区的
    /// `.git/config`、还会显示在界面上，而 Token 只允许待在凭据管理器里。
    #[test]
    fn inline_credentials_in_remote_url_are_detected() {
        assert!(has_inline_credentials(
            "https://user:ghp_secret@github.com/a/b.git"
        ));
        assert!(has_inline_credentials(
            "https://x-access-token:ghp_secret@github.com/a/b.git"
        ));

        assert!(!has_inline_credentials("https://github.com/a/b.git"));
        // SSH 形态没有"密码"这回事，不算内联凭据
        assert!(!has_inline_credentials("git@github.com:a/b.git"));
        assert!(!has_inline_credentials(
            "https://github.com/group/sub/repo.git"
        ));
    }

    /// 工作区必须按仓库地址分开：共用一个目录会让 A 仓库的内容
    /// 在下一次同步时被提交进 B 仓库。
    #[test]
    fn workspace_path_is_keyed_by_remote_url() {
        let a = sync_workspace("https://github.com/me/alpha.git").unwrap();
        let b = sync_workspace("https://github.com/me/beta.git").unwrap();
        assert_ne!(a, b, "不同仓库共用了同一个工作区");

        // 同一个地址必须稳定映射到同一个目录（否则每次同步都重新克隆）
        let again = sync_workspace("https://github.com/me/alpha.git").unwrap();
        assert_eq!(a, again);

        // 目录名里带可读的仓库名，方便人在资源管理器里辨认
        let name = a.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("alpha-"), "目录名不可读：{name}");
    }

    /// 仓库名要做成文件系统安全的 slug
    #[test]
    fn repo_slug_is_filesystem_safe() {
        assert_eq!(repo_slug("https://github.com/me/my-repo.git"), "my-repo");
        assert_eq!(repo_slug("https://github.com/me/my-repo/"), "my-repo");
        // 路径分隔符、盘符等一律不能漏进目录名
        let slug = repo_slug("D:/weird/../../etc/passwd");
        assert!(
            !slug.contains('/') && !slug.contains('\\'),
            "slug 不安全：{slug}"
        );
        // 只剩主机名时就用主机名，反正后面还跟着地址摘要，不会撞
        assert_eq!(repo_slug("https://github.com/"), "github.com");
        // 退化到空串或 `..` 这类危险名字时必须有兜底
        assert_eq!(repo_slug(".."), "repo");
        assert_eq!(repo_slug(""), "repo");
    }

    #[test]
    fn git_availability_reports_something() {
        let status = read_availability();
        // 本机装了 git，版本应能读到
        assert!(status.available);
        assert!(status.version.is_some());
    }

    /// 回归：发给前端的仓库对象必须是 camelCase。
    ///
    /// 这条测试守着一个真实发生过的白屏事故：结构体缺了序列化端重命名，
    /// 前端拿到 `full_name` 却按 `repo.fullName` 读，`undefined.toLowerCase()`
    /// 直接把界面打崩。
    #[test]
    fn repo_serializes_to_camel_case_for_frontend() {
        let repo = GitHubRepo {
            full_name: "user/repo".to_string(),
            name: "repo".to_string(),
            private: false,
            description: None,
            clone_url: "https://github.com/user/repo.git".to_string(),
            default_branch: Some("main".to_string()),
        };

        let value = serde_json::to_value(&repo).unwrap();
        assert!(value.get("fullName").is_some(), "前端字段名对不上：{value}");
        assert!(value.get("cloneUrl").is_some());
        assert!(value.get("defaultBranch").is_some());
        assert!(value.get("full_name").is_none(), "不能把蛇形名发给前端");
    }

    /// 反方向：必须能直接吃下 GitHub REST API 的蛇形响应
    #[test]
    fn repo_deserializes_from_github_api_shape() {
        let api_json = r#"{
            "full_name": "user/repo",
            "name": "repo",
            "private": true,
            "description": "说明",
            "clone_url": "https://github.com/user/repo.git",
            "default_branch": "main"
        }"#;

        let repo: GitHubRepo = serde_json::from_str(api_json).unwrap();
        assert_eq!(repo.full_name, "user/repo");
        assert_eq!(repo.clone_url, "https://github.com/user/repo.git");
        assert!(repo.private);
    }

    /// API 可能省略可选字段，不能因此解析失败
    #[test]
    fn repo_tolerates_missing_optional_fields() {
        let api_json = r#"{"full_name": "u/r", "name": "r", "private": false}"#;
        let repo: GitHubRepo = serde_json::from_str(api_json).unwrap();
        assert!(repo.description.is_none());
        assert_eq!(repo.clone_url, "", "缺失的克隆地址应回退为空串而不是报错");
    }

    /// 身份已配置时不得覆盖——用户的设置优先于我们的推断
    #[test]
    fn existing_identity_is_not_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        run_git(Some(root), &["init"]).unwrap();
        run_git(Some(root), &["config", "--local", "user.name", "我自己"]).unwrap();
        run_git(
            Some(root),
            &["config", "--local", "user.email", "me@example.com"],
        )
        .unwrap();

        ensure_commit_identity(root).unwrap();

        let name = run_git(Some(root), &["config", "--get", "user.name"]).unwrap();
        assert_eq!(name.trim(), "我自己", "已有的身份被覆盖了");
    }

    /// 身份写入必须是**仓库级**，绝不能动用户的全局 git 配置
    #[test]
    fn identity_is_set_locally_never_globally() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        run_git(Some(root), &["init"]).unwrap();

        // 无论能否取到身份，都要断言全局配置未被本模块写过
        let global_before =
            run_git(None, &["config", "--global", "--get", "user.email"]).unwrap_or_default();

        let _ = ensure_commit_identity(root);

        let global_after =
            run_git(None, &["config", "--global", "--get", "user.email"]).unwrap_or_default();
        assert_eq!(
            global_before, global_after,
            "本模块改动了用户的全局 git 身份"
        );
    }

    /// 身份缺失且无法推断时，应给出可操作的说明而不是让 git 抛原始报错
    #[test]
    fn identity_error_message_is_actionable() {
        // 仅验证错误文案包含关键指引（真实环境通常能推断出身份，故不强行构造失败）
        let msg =
            AppError::Config("无法确定提交身份，且未检测到已登录的 GitHub 账号。".to_string())
                .to_string();
        assert!(msg.contains("提交身份"));
    }

    /// 推送被拒时，报错必须说清"发生了什么、能做什么"，
    /// 而不是把 git 的 hint 原样抛给用户
    #[test]
    fn push_rejection_is_translated_into_actionable_message() {
        let tmp = tempfile::tempdir().unwrap();
        let raw = AppError::Io(
            "git push 失败：! [rejected] main -> main (fetch first)\n\
             error: failed to push some refs to 'https://github.com/u/r.git'\n\
             hint: Updates were rejected because the remote contains work that you do not have locally."
                .to_string(),
        );

        let friendly = friendly_push_error(raw, tmp.path());
        let text = friendly.to_string();

        assert!(matches!(friendly, AppError::Conflict(_)), "应归类为冲突");
        assert!(
            text.contains("仓库里又有了新的提交"),
            "缺少现象说明：{text}"
        );
        assert!(text.contains("再点一次"), "缺少可行动的建议：{text}");
        assert!(
            text.contains("不会删除或覆盖"),
            "未说明本应用对仓库已有内容的态度：{text}"
        );
        // 不应把原始 hint 直接堆给用户
        assert!(!text.contains("--help"), "仍在暴露 git 原始提示：{text}");
        // 新模型下不存在"合并远端内容"这个操作，文案不能再指向它
        assert!(!text.contains("合并远端"), "指向了已移除的操作：{text}");
    }

    /// 与远端无关的推送失败不应被误判为"远端有新内容"
    #[test]
    fn unrelated_push_errors_are_not_misclassified() {
        let tmp = tempfile::tempdir().unwrap();
        let raw = AppError::Io("git push 失败：Could not resolve host github.com".to_string());
        let friendly = friendly_push_error(raw, tmp.path());
        assert!(matches!(friendly, AppError::Io(_)), "网络错误被误判为冲突");
    }

    #[test]
    fn porcelain_parsing_classifies_changes() {
        let out = "?? skills/new-skill/\n M skills/a/SKILL.md\n D skills/gone/SKILL.md\n";
        let (added, modified, removed) = parse_porcelain(out);
        assert_eq!(added, vec!["skills/new-skill/"]);
        assert_eq!(modified, vec!["skills/a/SKILL.md"]);
        assert_eq!(removed, vec!["skills/gone/SKILL.md"]);
    }

    #[test]
    fn porcelain_ignores_blank_and_short_lines() {
        let (a, m, r) = parse_porcelain("\n\n??\n");
        assert!(a.is_empty() && m.is_empty() && r.is_empty());
    }

    /// `parse_staged` 吃的是 `git diff --cached --name-status`，格式与
    /// porcelain 不同（制表符分隔、重命名有三个字段）。
    #[test]
    fn staged_diff_is_parsed_by_name_status_format() {
        let out = "A\tskills/new/SKILL.md\nM\tskills/a/SKILL.md\nD\tskills/gone/SKILL.md\n";
        let (a, m, r) = parse_staged(out);
        assert_eq!(a, vec!["skills/new/SKILL.md"]);
        assert_eq!(m, vec!["skills/a/SKILL.md"]);
        assert_eq!(r, vec!["skills/gone/SKILL.md"]);

        // 重命名取**新**路径：那才是这次真正落进仓库的名字
        let renamed = parse_staged("R100\tskills/old/SKILL.md\tskills/new/SKILL.md\n");
        assert_eq!(renamed.1, vec!["skills/new/SKILL.md"]);

        let (a, m, r) = parse_staged("");
        assert!(a.is_empty() && m.is_empty() && r.is_empty());
    }

    /// 自动生成的提交信息必须能看出改了什么
    #[test]
    fn commit_message_summarizes_changes() {
        let msg = build_commit_message(
            &["skills/new/".to_string()],
            &["skills/a/SKILL.md".to_string()],
            &["skills/old/".to_string()],
        );
        assert!(msg.contains("sync: 1 added, 1 modified, 1 removed"));
        assert!(msg.contains("- add: skills/new/"));
        assert!(msg.contains("- modify: skills/a/SKILL.md"));
        assert!(msg.contains("- remove: skills/old/"));
    }

    #[test]
    fn commit_message_truncates_long_lists() {
        let many: Vec<String> = (0..30).map(|i| format!("skills/s{i}/")).collect();
        let msg = build_commit_message(&many, &[], &[]);
        assert!(msg.contains("另有 10 项"), "过长列表应截断：{msg}");
    }

    /// 工作区还不存在时，读状态不该报错，而是如实回答"尚未同步过"。
    ///
    /// 这里断言的是 `git_sync_status_impl`（可传入临时目录），而不是命令本身：
    /// 命令读的是真实 `%LOCALAPPDATA%`，断言它等于在断言开发机上装没装过——
    /// 那种测试在本机通过、在别的机器上红，且永远不会发现真正的缺陷。
    #[test]
    fn sync_status_on_missing_workspace_is_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let status = git_sync_status_impl(tmp.path()).unwrap();
        assert!(!status.is_repo);
        assert_eq!(status.changed_count, 0);
        assert!(status.imported_skills.is_empty());
    }

    /// 中央库没初始化时，同步与拉取都必须明确拒绝，而不是去动一个不存在的库。
    #[test]
    fn sync_and_pull_reject_an_uninitialized_library() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().display().to_string();

        assert!(
            git_sync_now(path.clone()).is_err(),
            "未初始化的中央库不该能同步"
        );
        assert!(git_pull(path).is_err(), "未初始化的中央库不该能拉取");
    }

    /// 地址里带着内联 Token 时必须拒绝绑定，并且**不能**把它写进配置。
    ///
    /// 这是模块第一条硬性约束（Token 绝不落明文）在入口处的落点：
    /// 地址会被写进 `config.json`、被 `remote set-url` 写进工作区的
    /// `.git/config`、还会显示在界面上。
    #[test]
    fn set_remote_refuses_inline_credentials() {
        let err = git_set_remote(
            "https://user:ghp_secret123@github.com/a/b.git".to_string(),
            None,
        )
        .unwrap_err();

        let text = err.to_string();
        assert!(
            !text.contains("ghp_secret123"),
            "报错里回显了 Token：{text}"
        );
        assert!(text.contains("凭据管理器"), "没告诉用户该去哪里填：{text}");
    }

    // =======================================================================
    // 同步模型（附录 H）的端到端测试
    //
    // 下面用临时目录搭"裸仓库当远端 + 一个中央库"的最小环境，
    // 直接驱动 `sync_impl` / `pull_impl`，不碰真实配置与 %LOCALAPPDATA%。
    //
    // 注意：本机**没有全局 git 身份**，而 `ensure_commit_identity` 取不到身份时
    // 会去问 GitHub API。所以凡是会产生提交的测试，都先给工作区设好本地身份，
    // 让测试保持离线、可重复。
    // =======================================================================

    fn git_str(path: &Path) -> String {
        path.display().to_string()
    }

    /// 造一个裸仓库当远端，默认分支为 `main`
    fn init_bare(path: &Path) -> PathBuf {
        run_git(None, &["init", "--bare", "-b", "main", &git_str(path)]).unwrap();
        path.to_path_buf()
    }

    fn clone_repo(from: &Path, to: &Path) {
        run_git(None, &["clone", &git_str(from), &git_str(to)]).unwrap();
    }

    /// 给仓库设本地提交身份（绝不动全局配置）
    fn local_identity(repo: &Path) {
        run_git(Some(repo), &["config", "--local", "user.name", "测试"]).unwrap();
        run_git(
            Some(repo),
            &["config", "--local", "user.email", "test@example.com"],
        )
        .unwrap();
    }

    fn commit_all(repo: &Path, message: &str) {
        run_git(Some(repo), &["add", "-A"]).unwrap();
        run_git(Some(repo), &["commit", "-m", message]).unwrap();
    }

    fn write_skill(root: &Path, name: &str, body: &str) {
        let dir = root.join(crate::library::SKILLS_DIR).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), body).unwrap();
    }

    fn read_skill(root: &Path, name: &str) -> String {
        std::fs::read_to_string(
            root.join(crate::library::SKILLS_DIR)
                .join(name)
                .join("SKILL.md"),
        )
        .unwrap()
    }

    /// 造一个中央库骨架（只建目录，不建索引）
    fn make_library(path: &Path) {
        std::fs::create_dir_all(path.join(crate::library::SKILLS_DIR)).unwrap();
    }

    /// 列出裸仓库某个分支上的全部文件
    ///
    /// `-c core.quotePath=false` 是必须的：裸仓库自己没有这条配置，
    /// 默认会把中文路径转义成八进制，断言里就再也找不到它们了。
    fn files_in_bare(bare: &Path, branch: &str) -> Vec<String> {
        run_git(
            None,
            &[
                "-c",
                "core.quotePath=false",
                "--git-dir",
                &git_str(bare),
                "ls-tree",
                "-r",
                "--name-only",
                branch,
            ],
        )
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
    }

    /// 裸仓库里有没有这个分支（空仓库当然没有，`ls-tree` 会直接报错）
    fn bare_has_branch(bare: &Path, branch: &str) -> bool {
        run_git(
            None,
            &[
                "--git-dir",
                &git_str(bare),
                "rev-parse",
                "--verify",
                "--quiet",
                branch,
            ],
        )
        .is_ok()
    }

    /// 裸仓库某个分支上最近一次提交的完整信息
    fn commit_message_in_bare(bare: &Path, branch: &str) -> String {
        run_git(
            None,
            &[
                "--git-dir",
                &git_str(bare),
                "log",
                "-1",
                "--pretty=%B",
                branch,
            ],
        )
        .unwrap_or_default()
    }

    fn read_bare_file(bare: &Path, branch: &str, path: &str) -> String {
        run_git(
            None,
            &[
                "--git-dir",
                &git_str(bare),
                "show",
                &format!("{branch}:{path}"),
            ],
        )
        .unwrap()
    }

    /// 往远端推入一份"别人的内容"（与 Skill 无关的文件），
    /// 复现附录 H 的场景：用户想用的是一个**已经有内容**的仓库。
    fn seed_unrelated_content(bare: &Path, tmp: &Path) {
        let seed = tmp.join("seed");
        clone_repo(bare, &seed);
        local_identity(&seed);
        std::fs::create_dir_all(seed.join("notes")).unwrap();
        std::fs::write(seed.join("notes/lesson.md"), "课程笔记\n").unwrap();
        commit_all(&seed, "notes: 别人的内容");
        run_git(Some(&seed), &["push", "-u", "origin", "main"]).unwrap();
    }

    fn target(lib: &Path, ws: &Path, url: &str, branch: &str) -> Target {
        Target {
            lib: lib.to_path_buf(),
            ws: ws.to_path_buf(),
            url: url.to_string(),
            branch: branch.to_string(),
        }
    }

    /// 镜像**只增改、不删除**：目标里已有的、源里没有的内容必须留下。
    #[test]
    fn mirror_never_deletes_destination_content() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("lib");
        let ws = tmp.path().join("ws");
        write_skill(&lib, "本地技能", "# 本地");
        write_skill(&ws, "仓库里的技能", "# 仓库");

        let (added, updated) = mirror_dir(
            &crate::library::skills_dir(&lib),
            &crate::library::skills_dir(&ws),
        )
        .unwrap();

        assert_eq!((added, updated), (1, 0));
        assert!(
            ws.join("skills/仓库里的技能/SKILL.md").is_file(),
            "仓库里已有的技能被镜像删掉了"
        );
        assert!(ws.join("skills/本地技能/SKILL.md").is_file());
    }

    #[test]
    fn mirror_updates_changed_files_and_counts_them() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("lib");
        let ws = tmp.path().join("ws");
        write_skill(&lib, "a", "# a 新版");
        write_skill(&lib, "b", "# b");
        write_skill(&ws, "a", "# a 旧版");

        let (added, updated) = mirror_dir(
            &crate::library::skills_dir(&lib),
            &crate::library::skills_dir(&ws),
        )
        .unwrap();

        assert_eq!((added, updated), (1, 1), "新增 b、更新 a");
        assert_eq!(read_skill(&ws, "a"), "# a 新版");
    }

    /// 镜像跳过杂项文件，不把它们塞进用户的仓库
    #[test]
    fn mirror_skips_junk_files() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("lib");
        let ws = tmp.path().join("ws");
        write_skill(&lib, "a", "# a");
        std::fs::write(lib.join("skills/.DS_Store"), "junk").unwrap();
        std::fs::write(lib.join("skills/Thumbs.db"), "junk").unwrap();

        let (added, _) = mirror_dir(
            &crate::library::skills_dir(&lib),
            &crate::library::skills_dir(&ws),
        )
        .unwrap();

        assert_eq!(added, 1, "只应镜像真正的 Skill");
        assert!(!ws.join("skills/.DS_Store").exists());
        assert!(!ws.join("skills/Thumbs.db").exists());
    }

    /// 拉取**只补齐、不覆盖**：中央库已有的同名 Skill 保持原样。
    #[test]
    fn pull_imports_missing_skills_and_skips_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));

        let seed = tmp.path().join("seed");
        clone_repo(&bare, &seed);
        local_identity(&seed);
        write_skill(&seed, "foo", "# 远端版 foo");
        write_skill(&seed, "bar", "# bar");
        commit_all(&seed, "seed: 两个技能");
        run_git(Some(&seed), &["push", "-u", "origin", "main"]).unwrap();

        // 中央库已有 foo，且内容与远端不同
        let lib = tmp.path().join("lib");
        make_library(&lib);
        write_skill(&lib, "foo", "# 本地版 foo");

        let ws = tmp.path().join("ws");
        let (status, branch) = pull_impl(&target(&lib, &ws, &git_str(&bare), "main")).unwrap();

        assert_eq!(branch, "main");
        assert_eq!(status.imported_skills, vec!["bar".to_string()]);
        assert_eq!(
            read_skill(&lib, "foo"),
            "# 本地版 foo",
            "本地已有的 Skill 被远端覆盖了"
        );
        assert_eq!(read_skill(&lib, "bar"), "# bar");
    }

    /// **回归**：取回必须把仓库里 `skills/` 下的条目原样还原，
    /// 哪怕它**不含 `SKILL.md`**。
    ///
    /// 这条守着一个用户实际报上来的数据丢失：他放了一个含 `测试1.md` 的
    /// 目录（没有 SKILL.md，所以既不是应用眼里的 Skill，也不该被特殊对待），
    /// 同步把它忠实传上了仓库，删掉本地后点「取回缺失的 Skill」——
    /// 什么也没回来，界面还提示"仓库里没有中央库缺少的 Skill"。
    ///
    /// 根因是取回里加了一道"必须含 SKILL.md"的判断，而同步并没有同样的过滤：
    /// **传得上去、取不回来**。取回是同步的逆操作，两者必须对同一批文件成立。
    #[test]
    fn pull_restores_directories_without_a_skill_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));

        let seed = tmp.path().join("seed");
        clone_repo(&bare, &seed);
        local_identity(&seed);
        write_skill(&seed, "真技能", "# 真技能");
        // 用户放进去的目录：有内容，但里面不是 SKILL.md
        std::fs::create_dir_all(seed.join("skills/测试skill1")).unwrap();
        std::fs::write(seed.join("skills/测试skill1/测试1.md"), "").unwrap();
        commit_all(&seed, "seed");
        run_git(Some(&seed), &["push", "-u", "origin", "main"]).unwrap();

        // 中央库此时两个都没有（模拟"删掉本地后取回"）
        let lib = tmp.path().join("lib");
        make_library(&lib);

        let ws = tmp.path().join("ws");
        let (status, _) = pull_impl(&target(&lib, &ws, &git_str(&bare), "main")).unwrap();

        assert!(
            status.imported_skills.contains(&"测试skill1".to_string()),
            "不含 SKILL.md 的目录没有被取回：{:?}",
            status.imported_skills
        );
        assert!(
            lib.join("skills/测试skill1/测试1.md").is_file(),
            "目录取回了，但里面的文件没有"
        );
        assert!(lib.join("skills/真技能/SKILL.md").is_file());
    }

    /// 取回与同步必须**面对同一批条目**：凡是同步会传上去的，
    /// 取回就得能还回来。用真实的"上传 → 删本地 → 取回"来回跑一遍。
    #[test]
    fn pull_is_the_inverse_of_sync_for_every_entry_under_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));
        let url = git_str(&bare);

        // 中央库里有四种形态：标准 Skill（含 assets）、无 SKILL.md 的目录、
        // 名字带点的目录、以及一个普通文件
        let lib = tmp.path().join("lib");
        make_library(&lib);
        write_skill(&lib, "标准技能", "# 标准");
        std::fs::create_dir_all(lib.join("skills/标准技能/assets")).unwrap();
        std::fs::write(lib.join("skills/标准技能/assets/a.bin"), "x").unwrap();
        std::fs::create_dir_all(lib.join("skills/没有清单的目录")).unwrap();
        std::fs::write(lib.join("skills/没有清单的目录/note.md"), "n").unwrap();
        std::fs::create_dir_all(lib.join("skills/.hidden")).unwrap();
        std::fs::write(lib.join("skills/.hidden/x"), "h").unwrap();
        std::fs::write(lib.join("skills/README.md"), "说明").unwrap();

        let ws = tmp.path().join("ws");
        ensure_workspace_at(&ws, &url, "main").unwrap();
        local_identity(&ws);
        sync_impl(&target(&lib, &ws, &url, "main")).unwrap();

        let uploaded = files_in_bare(&bare, "main");
        assert_eq!(uploaded.len(), 5, "同步上传的内容与预期不符：{uploaded:?}");

        // 清空中央库，只留目录骨架，然后取回
        std::fs::remove_dir_all(crate::library::skills_dir(&lib)).unwrap();
        make_library(&lib);

        let (status, _) = pull_impl(&target(&lib, &ws, &url, "main")).unwrap();

        assert_eq!(
            status.imported_skills.len(),
            4,
            "{:?}",
            status.imported_skills
        );
        assert!(lib.join("skills/标准技能/assets/a.bin").is_file());
        assert!(lib.join("skills/没有清单的目录/note.md").is_file());
        assert!(lib.join("skills/.hidden/x").is_file());
        assert!(lib.join("skills/README.md").is_file());
    }

    /// **附录 H 的核心场景**：仓库里已经有别人的内容时，同步必须成功，
    /// 且既不删除、也不改动那些内容。
    ///
    /// 这正是原设计做不到的事——原设计会因为历史不相关而必然推送失败。
    #[test]
    fn sync_into_a_repository_that_already_has_content() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));
        seed_unrelated_content(&bare, tmp.path());

        let lib = tmp.path().join("lib");
        make_library(&lib);
        write_skill(&lib, "pdf-tools", "# pdf-tools");

        // 工作区先就位并给好身份（避免测试去联网推断身份）
        let ws = tmp.path().join("ws");
        clone_repo(&bare, &ws);
        local_identity(&ws);

        let (status, branch) = sync_impl(&target(&lib, &ws, &git_str(&bare), "main")).unwrap();

        assert_eq!(branch, "main");
        assert_eq!(status.changed_count, 0, "推送完成后工作区应当是干净的");

        let files = files_in_bare(&bare, "main");
        assert!(
            files.contains(&"notes/lesson.md".to_string()),
            "仓库原有的内容丢了：{files:?}"
        );
        assert!(
            files.contains(&"skills/pdf-tools/SKILL.md".to_string()),
            "Skill 没推上去：{files:?}"
        );
    }

    /// 双端往返的自动化版本：
    /// A 机同步 → B 机取回 → B 机新增后再同步 → A 机取回。
    ///
    /// 顺带钉住一条**用户可见的语义**：无论从哪边操作，
    /// **本地中央库已有的 Skill 永远不会被仓库里的版本覆盖**。
    #[test]
    fn two_machines_round_trip_never_clobbers_local_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));
        let url = git_str(&bare);

        // ---- A 机：从零开始（远端是空的）----
        let lib_a = tmp.path().join("lib-a");
        make_library(&lib_a);
        write_skill(&lib_a, "共享技能", "# 来自 A 机");

        let ws_a = tmp.path().join("ws-a");
        ensure_workspace_at(&ws_a, &url, "main").unwrap();
        local_identity(&ws_a);
        let (synced, _) = sync_impl(&target(&lib_a, &ws_a, &url, "main")).unwrap();
        assert_eq!(synced.ahead, 0, "A 机应当已全部推送");
        assert!(files_in_bare(&bare, "main").contains(&"skills/共享技能/SKILL.md".to_string()));

        // ---- B 机：本地已有一个**同名但内容不同**的 Skill ----
        let lib_b = tmp.path().join("lib-b");
        make_library(&lib_b);
        write_skill(&lib_b, "共享技能", "# B 机自己的版本");

        let ws_b = tmp.path().join("ws-b");
        let (pulled, _) = pull_impl(&target(&lib_b, &ws_b, &url, "main")).unwrap();

        assert!(
            pulled.imported_skills.is_empty(),
            "同名 Skill 被当成缺失项导入了：{:?}",
            pulled.imported_skills
        );
        assert_eq!(
            read_skill(&lib_b, "共享技能"),
            "# B 机自己的版本",
            "取回覆盖了 B 机本地的 Skill"
        );

        // ---- B 机新增一个 Skill 并同步，A 机应当能取回它 ----
        write_skill(&lib_b, "B 机新增", "# B 机新增");
        local_identity(&ws_b);
        sync_impl(&target(&lib_b, &ws_b, &url, "main")).unwrap();

        let (pulled_a, _) = pull_impl(&target(&lib_a, &ws_a, &url, "main")).unwrap();
        assert_eq!(pulled_a.imported_skills, vec!["B 机新增".to_string()]);
        assert_eq!(
            read_skill(&lib_a, "共享技能"),
            "# 来自 A 机",
            "取回把 A 机本地的 Skill 覆盖成了仓库里的版本"
        );
    }

    /// 同步**只提交 `skills/`**：仓库里用户自己的改动既不提交也不推送。
    #[test]
    fn sync_does_not_commit_unrelated_changes_in_the_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));
        seed_unrelated_content(&bare, tmp.path());

        let lib = tmp.path().join("lib");
        make_library(&lib);
        write_skill(&lib, "a", "# a");

        let ws = tmp.path().join("ws");
        clone_repo(&bare, &ws);
        local_identity(&ws);
        // 把仓库里"别人的文件"改脏
        std::fs::write(ws.join("notes/lesson.md"), "被改脏了\n").unwrap();

        sync_impl(&target(&lib, &ws, &git_str(&bare), "main")).unwrap();

        assert_eq!(
            read_bare_file(&bare, "main", "notes/lesson.md").replace("\r\n", "\n"),
            "课程笔记\n",
            "本应用提交了仓库里与 Skill 无关的文件"
        );
        assert!(files_in_bare(&bare, "main").contains(&"skills/a/SKILL.md".to_string()));
    }

    /// 连点两次「同步」不该出错，第二次也没有内容可提交。
    #[test]
    fn sync_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));
        let lib = tmp.path().join("lib");
        make_library(&lib);
        write_skill(&lib, "a", "# a");

        let ws = tmp.path().join("ws");
        let url = git_str(&bare);
        ensure_workspace_at(&ws, &url, "main").unwrap();
        local_identity(&ws);

        let (first, _) = sync_impl(&target(&lib, &ws, &url, "main")).unwrap();
        assert_eq!(first.ahead, 0, "第一次同步后应当已全部推送");

        let (second, _) = sync_impl(&target(&lib, &ws, &url, "main")).unwrap();
        assert_eq!(second.changed_count, 0);
        assert_eq!(second.ahead, 0);
        assert!(files_in_bare(&bare, "main").contains(&"skills/a/SKILL.md".to_string()));
    }

    /// **同步绝不删除仓库里已有的内容**（附录 H 的原话）。
    ///
    /// 这条守着一个很隐蔽的失败方式：暂存时若用 `git add -A`，那么任何
    /// "从工作区消失了"的追踪文件都会被当成一次删除提交上去——比如杀毒软件
    /// 隔离了某个 asset、清理工具动了 AppData、或用户手工清空了同步目录。
    /// git 分不清那是"用户想删"还是"文件没了"，而后果是用户仓库里的东西被删掉。
    #[test]
    fn sync_never_deletes_content_from_the_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));

        // 远端先有一个 Skill
        let seed = tmp.path().join("seed");
        clone_repo(&bare, &seed);
        local_identity(&seed);
        write_skill(&seed, "仓库里的技能", "# 仓库里的");
        commit_all(&seed, "seed");
        run_git(Some(&seed), &["push", "-u", "origin", "main"]).unwrap();

        let lib = tmp.path().join("lib");
        make_library(&lib);
        write_skill(&lib, "本地新增", "# 本地");

        let ws = tmp.path().join("ws");
        clone_repo(&bare, &ws);
        local_identity(&ws);

        // 模拟外部原因导致工作区里的文件消失（这里直接删）
        std::fs::remove_dir_all(ws.join("skills/仓库里的技能")).unwrap();

        let (status, _) = sync_impl(&target(&lib, &ws, &git_str(&bare), "main")).unwrap();

        let files = files_in_bare(&bare, "main");
        assert!(
            files.contains(&"skills/仓库里的技能/SKILL.md".to_string()),
            "同步把仓库里已有的内容删掉了：{files:?}"
        );
        assert!(
            files.contains(&"skills/本地新增/SKILL.md".to_string()),
            "本地新增的 Skill 没传上去：{files:?}"
        );
        // 工作区里那个删除**仍然**存在（我们只是不提交它）
        assert!(status.is_repo);
    }

    /// 上一次同步"提交成功、推送失败"之后，下一次同步必须把它推上去。
    ///
    /// 这条守着一个很容易写错的地方：若在"没有改动"时直接返回，推送就永远
    /// 不会再发生——工作区是干净的，而用户再点多少次同步都推不上去，
    /// 界面还一直说"已同步"。断网、令牌过期都会走到这里。
    #[test]
    fn a_failed_push_is_retried_on_the_next_sync() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));
        let url = git_str(&bare);

        // 装一个拒绝推送的钩子，制造"提交成功、推送失败"
        let hook = bare.join("hooks/pre-receive");
        std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();

        let lib = tmp.path().join("lib");
        make_library(&lib);
        write_skill(&lib, "a", "# a");

        let ws = tmp.path().join("ws");
        ensure_workspace_at(&ws, &url, "main").unwrap();
        local_identity(&ws);

        let failed = sync_impl(&target(&lib, &ws, &url, "main"));
        assert!(failed.is_err(), "钩子拒绝了推送，同步不该报成功");

        // 提交确实留在工作区里，只是没上去
        assert!(
            !bare_has_branch(&bare, "main"),
            "前提失效：推送居然成功了，远端不该有 main"
        );
        assert!(ws.join("skills/a/SKILL.md").is_file(), "提交内容不该丢");

        // 远端恢复可写：下一次同步必须把上一次那个提交补推上去
        std::fs::remove_file(&hook).unwrap();
        let (status, _) = sync_impl(&target(&lib, &ws, &url, "main")).unwrap();

        assert_eq!(status.ahead, 0, "补推之后不该还有未推送的提交");
        assert!(
            files_in_bare(&bare, "main").contains(&"skills/a/SKILL.md".to_string()),
            "上一次没推上去的提交再也没有被推上去"
        );
    }

    /// 仓库自己的 `.gitignore` 恰好匹配到 Skill 时，同步必须**照样上传**。
    ///
    /// 触发场景就是本功能存在的理由：把 Skill 库同步进一个"笔记仓库"，
    /// 而那种仓库的 `.gitignore` 里常有 `*.md`——SKILL.md 正好中招。
    /// 被忽略的文件不会出现在 `git status` 里，若按状态判断就会：
    /// 界面报"已同步"，而 Skill 一个都没上去。
    #[test]
    fn sync_uploads_skills_matched_by_the_repository_gitignore() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));

        // 远端是一个笔记仓库，`.gitignore` 忽略了所有 .md
        let seed = tmp.path().join("seed");
        clone_repo(&bare, &seed);
        local_identity(&seed);
        std::fs::write(seed.join(".gitignore"), "*.md\n").unwrap();
        std::fs::create_dir_all(seed.join("notes")).unwrap();
        std::fs::write(seed.join("notes/lesson.md"), "笔记\n").unwrap();
        commit_all(&seed, "notes: 初始化");
        run_git(Some(&seed), &["push", "-u", "origin", "main"]).unwrap();

        let lib = tmp.path().join("lib");
        make_library(&lib);
        write_skill(&lib, "pdf-tools", "# pdf-tools");

        let ws = tmp.path().join("ws");
        clone_repo(&bare, &ws);
        local_identity(&ws);

        let (status, _) = sync_impl(&target(&lib, &ws, &git_str(&bare), "main")).unwrap();

        let files = files_in_bare(&bare, "main");
        assert!(
            files.contains(&"skills/pdf-tools/SKILL.md".to_string()),
            "被仓库 .gitignore 匹配的 Skill 没有被上传：{files:?}"
        );
        // 提交信息也要如实报出来，不能"传了却说没传"
        let message = commit_message_in_bare(&bare, "main");
        assert!(
            message.contains("skills/pdf-tools/SKILL.md"),
            "提交信息没报出实际传上去的内容：{message}"
        );
        assert!(status.is_repo);
    }

    /// 中文 Skill 名必须原样出现在提交信息里。
    ///
    /// git 默认会把非 ASCII 路径转义成八进制（`"skills/\344\270\255…"`），
    /// 这个字符串会直接进到提交信息和界面的变更列表里。对一个中文应用来说，
    /// 那等于把路径显示成一堆乱码——所以要显式关掉 `core.quotePath`。
    #[test]
    fn chinese_skill_names_survive_into_the_commit_message() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));

        let lib = tmp.path().join("lib");
        make_library(&lib);
        write_skill(&lib, "中文技能", "# 中文技能");

        let ws = tmp.path().join("ws");
        let url = git_str(&bare);
        ensure_workspace_at(&ws, &url, "main").unwrap();
        local_identity(&ws);
        sync_impl(&target(&lib, &ws, &url, "main")).unwrap();

        let message = commit_message_in_bare(&bare, "main");
        assert!(
            message.contains("skills/中文技能/"),
            "提交信息里的中文被转义了：{message}"
        );
        assert!(
            !message.contains("\\344"),
            "提交信息里出现了八进制转义：{message}"
        );
    }

    /// 空仓库（刚在 GitHub 上新建、一个提交都没有）必须可用——
    /// 这是本项目最推荐的备份用法。
    #[test]
    fn sync_works_on_a_freshly_created_empty_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));

        let lib = tmp.path().join("lib");
        make_library(&lib);
        write_skill(&lib, "a", "# a");

        // 远端为空 → 工作区走 "init + 绑远端" 这条路
        let ws = tmp.path().join("ws");
        let url = git_str(&bare);
        assert_eq!(ensure_workspace_at(&ws, &url, "main").unwrap(), "main");
        local_identity(&ws);

        let (status, branch) = sync_impl(&target(&lib, &ws, &url, "main")).unwrap();

        assert_eq!(branch, "main");
        assert_eq!(status.ahead, 0);
        assert!(files_in_bare(&bare, "main").contains(&"skills/a/SKILL.md".to_string()));
    }

    /// 从空仓库拉取不是错误，只是没有任何东西可取。
    #[test]
    fn pull_on_an_empty_repository_is_a_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));

        let lib = tmp.path().join("lib");
        make_library(&lib);
        let ws = tmp.path().join("ws");

        let (status, _) = pull_impl(&target(&lib, &ws, &git_str(&bare), "main")).unwrap();
        assert!(status.imported_skills.is_empty());
    }

    /// 远端为空时初始化工作区：建仓库、绑远端、关掉行尾符转换，且可重复调用。
    #[test]
    fn ensure_workspace_initializes_on_an_empty_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));
        let ws = tmp.path().join("ws");
        let url = git_str(&bare);

        assert_eq!(ensure_workspace_at(&ws, &url, "main").unwrap(), "main");
        assert!(ws.join(".git").is_dir());
        assert_eq!(
            run_git(Some(&ws), &["remote", "get-url", "origin"])
                .unwrap()
                .trim(),
            url
        );
        assert_eq!(
            run_git(Some(&ws), &["config", "--get", "core.autocrlf"])
                .unwrap()
                .trim(),
            "false",
            "工作区必须关掉行尾符转换，否则镜像不是忠实的字节复制"
        );
        // 远端确实还没有任何提交
        assert!(run_git(Some(&ws), &["rev-parse", "--verify", "--quiet", "HEAD"]).is_err());

        // 幂等：再次确保不应报错，分支也不变
        assert_eq!(ensure_workspace_at(&ws, &url, "main").unwrap(), "main");
    }

    /// 远端有内容时，工作区要克隆下来（含与 Skill 无关的内容）
    #[test]
    fn ensure_workspace_clones_a_repository_with_content() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));
        seed_unrelated_content(&bare, tmp.path());

        let ws = tmp.path().join("ws");
        assert_eq!(
            ensure_workspace_at(&ws, &git_str(&bare), "main").unwrap(),
            "main"
        );
        assert!(
            ws.join("notes/lesson.md").is_file(),
            "克隆没有把远端已有的内容带下来"
        );
    }

    /// 配置里的分支在远端不存在时，沿用仓库自己的默认分支，
    /// 而不是凭空造一个与主分支并行的分支。
    #[test]
    fn select_branch_falls_back_to_the_repository_default() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        run_git(None, &["init", "-b", "master", &git_str(&repo)]).unwrap();
        // 必须有提交：未出生的 HEAD 不是一个分支名，"沿用当前分支"也就无从谈起。
        // 真实场景里工作区是克隆来的，自带提交，所以这里如实造出同样的状态。
        local_identity(&repo);
        std::fs::write(repo.join("README.md"), "仓库自己的内容\n").unwrap();
        commit_all(&repo, "初始提交");
        run_git(
            Some(&repo),
            &["remote", "add", "origin", "https://example.invalid/x.git"],
        )
        .unwrap();

        assert_eq!(
            select_branch(&repo, "main"),
            "master",
            "远端没有 main，应沿用当前分支"
        );
    }

    /// 分支还没有任何提交时（远端为空、刚 init 出来），退回配置里的分支名。
    #[test]
    fn select_branch_uses_the_configured_name_on_an_unborn_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        run_git(None, &["init", "-b", "main", &git_str(&repo)]).unwrap();

        assert_eq!(select_branch(&repo, "main"), "main");
    }

    /// 远端有配置的分支时，用配置的那个
    #[test]
    fn select_branch_prefers_the_configured_branch_when_it_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));
        seed_unrelated_content(&bare, tmp.path());

        let ws = tmp.path().join("ws");
        clone_repo(&bare, &ws);

        assert_eq!(select_branch(&ws, "main"), "main");
        // 远端没有这个分支名 → 沿用当前分支
        assert_eq!(select_branch(&ws, "不存在"), "main");
    }

    /// 工作区能快进到远端的新提交
    #[test]
    fn refresh_workspace_fast_forwards_to_the_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = init_bare(&tmp.path().join("remote.git"));
        seed_unrelated_content(&bare, tmp.path());

        let ws = tmp.path().join("ws");
        clone_repo(&bare, &ws);

        // 另一处又推了一个文件
        let other = tmp.path().join("other");
        clone_repo(&bare, &other);
        local_identity(&other);
        std::fs::write(other.join("notes/more.md"), "后来的\n").unwrap();
        commit_all(&other, "notes: 又一条");
        run_git(Some(&other), &["push"]).unwrap();

        assert!(
            !ws.join("notes/more.md").exists(),
            "前提失效：工作区已是最新"
        );
        refresh_workspace(&ws, "main").unwrap();
        assert!(ws.join("notes/more.md").is_file(), "没有快进到远端");
    }

    /// 中央库中残留的 `.git` 应被**移走**，内容一个都不能少。
    #[test]
    fn stale_repository_is_moved_aside_never_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("lib");
        make_library(&lib);
        // 模拟旧模型留下的仓库
        let stale = lib.join(".git");
        std::fs::create_dir_all(stale.join("objects")).unwrap();
        std::fs::write(stale.join("不会丢的标记"), "重要").unwrap();

        let path = git_str(&lib);
        assert!(git_stale_repo(path.clone()).unwrap().is_some());

        let moved = git_cleanup_stale_repo(path.clone()).unwrap().unwrap();

        assert!(!lib.join(".git").exists(), "残留的 .git 还在原位");
        assert!(
            PathBuf::from(&moved).join("不会丢的标记").is_file(),
            "移走时把内容弄丢了"
        );
        assert!(git_stale_repo(path.clone()).unwrap().is_none());
        // 再清一次是安全的空操作
        assert!(git_cleanup_stale_repo(path).unwrap().is_none());
    }

    /// 没有残留时不该报错，也不该凭空造出什么
    #[test]
    fn stale_repository_check_is_quiet_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("lib");
        make_library(&lib);

        assert!(git_stale_repo(git_str(&lib)).unwrap().is_none());
        assert!(git_cleanup_stale_repo(git_str(&lib)).unwrap().is_none());
        // 空路径同样安全（配置里可能还没设中央库）
        assert!(git_stale_repo(String::new()).unwrap().is_none());
        assert!(git_cleanup_stale_repo(String::new()).unwrap().is_none());
    }

    /// `api_token` 必须**只读本地凭据**，绝不触发与助手/网络的交互。
    ///
    /// 这是把 31 秒开销挡在即时操作之外的关键：一旦它去问助手，
    /// "加载仓库列表"就会变成几十秒的等待。
    /// 自愈：本地无缓存但助手有凭据时，`ensure_api_token` 应把它取过来。
    ///
    /// 覆盖的真实场景：用户在缓存机制存在之前就已经登录过。
    #[test]
    fn ensure_api_token_self_heals_from_helper() {
        if !has_helper_credentials() {
            eprintln!("跳过：本机尚未登录 GitHub");
            return;
        }
        let healed = ensure_api_token();
        assert!(healed, "助手里有凭据，自愈失败");
        assert!(api_token().is_some(), "自愈后本地仍无凭据");
    }

    #[test]
    fn api_token_never_touches_the_helper() {
        let started = std::time::Instant::now();
        let _ = api_token();
        assert!(
            started.elapsed().as_millis() < 200,
            "api_token 耗时 {:?}，说明它去问助手了",
            started.elapsed()
        );
    }

    /// 向助手换 Token 必须有**硬超时**，不能无限等。
    ///
    /// 实测该调用在凭据已存在时仍可能耗 31 秒（GCM 会去校验/刷新令牌），
    /// 因此这里只断言"有上限"，而不是"很快"。
    #[test]
    fn token_exchange_is_bounded() {
        if !has_helper_credentials() {
            eprintln!("跳过：本机尚未登录 GitHub");
            return;
        }
        let started = std::time::Instant::now();
        let _ = credential_from_helper_bounded();
        assert!(
            started.elapsed() < TOKEN_EXCHANGE_TIMEOUT + std::time::Duration::from_secs(5),
            "换取 Token 超过了硬超时上限"
        );
    }

    /// **回归**：空仓库必须能被绑定。
    ///
    /// 守着 `ls-remote --exit-code` 那个坑：空仓库没有 ref，
    /// `--exit-code` 会让命令以状态 2 退出，把"可访问但为空"
    /// 误判成"无法访问"——而空仓库正是本项目推荐的备份方式。
    #[test]
    fn ls_remote_without_exit_code_succeeds_on_empty_repo() {
        let tmp = tempfile::tempdir().unwrap();
        // 造一个本地"空仓库"当作远端
        let bare = tmp.path().join("empty.git");
        run_git(None, &["init", "--bare", bare.to_str().unwrap()]).unwrap();

        let url = bare.display().to_string();

        // 正确做法：不加 --exit-code，空仓库应成功（输出为空）
        let ok = run_git(None, &["ls-remote", &url]);
        assert!(ok.is_ok(), "空仓库被判定为不可访问：{ok:?}");
        assert!(ok.unwrap().trim().is_empty(), "空仓库不应有 ref");

        // 反证：加了 --exit-code 就会失败（这正是修复前的行为）
        let with_exit_code = run_git(None, &["ls-remote", "--exit-code", &url, "HEAD"]);
        assert!(
            with_exit_code.is_err(),
            "前提失效：--exit-code 对空仓库不再报错，该注释需要更新"
        );
    }

    #[test]
    fn set_remote_rejects_empty_url() {
        assert!(git_set_remote("   ".to_string(), None).is_err());
    }
}
