//! 派生外部命令的统一入口。
//!
//! 本应用不自己实现 git，而是调用系统上的 `git` CLI（见 `git/mod.rs` 的说明）。
//! 那条路必然要派生外部进程，而"派生外部进程"在 Windows 上有一个非默认的
//! 正确姿势——这个模块就是那个姿势的唯一落点。
//!
//! 放在 `platform/` 而不是各调用点：这是平台差异，而平台差异按约定只能收敛
//! 在这里（见 `platform/mod.rs`）。散在各个 `Command::new` 上，任何一处漏加
//! 都会重新出现"闪黑框"。

use std::ffi::OsStr;
use std::process::Command;

/// 转出 `Command` 类型本身。
///
/// 别的模块要**声明**一个返回 `Command` 的辅助函数时就得写这个类型名，而
/// 审计测试（`tests/cross_platform.rs`）禁止 `platform/` 之外出现
/// `std::process::Command` 字样——那是在防"绕过 `command()` 直接 `Command::new`"。
/// 从这个模块转出去，既能写类型，又不会被误判为绕过。
pub use std::process::Command as ProcessCommand;

/// 建一个用于派生外部命令的 `Command`。
///
/// # Windows：必须显式禁止为子进程创建控制台窗口
///
/// 发布构建是 GUI 子系统进程（`main.rs` 的 `windows_subsystem = "windows"`，
/// 进程自己没有控制台）。这种情况下 Windows 会为**每一个**控制台子进程——
/// `git.exe`、`cmdkey.exe`、`git-credential-manager.exe`——新建一个控制台窗口。
/// 用户看到的是"一打开应用、或者一点按钮，就有一串终端窗口闪一下"。
///
/// 危害不止是难看：每个新窗口都会**抢走前台焦点**，正在滚动或点击的那一下
/// 被吃掉，手感就是"卡顿"；焦点变化还会让界面认为窗口重新获得焦点、
/// 触发一轮数据重取，于是再派一批子进程——一个自我放大的循环。
///
/// `CREATE_NO_WINDOW` 只影响窗口：子进程照常运行，stdout/stderr 照常从管道读到。
///
/// # 其余平台
///
/// 没有"控制台窗口"这回事，直接建命令即可。
#[cfg(windows)]
pub fn command(program: impl AsRef<OsStr>) -> Command {
    use std::os::windows::process::CommandExt;

    /// `CREATE_NO_WINDOW`（winbase.h）：不为子进程创建控制台窗口
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

/// 见 Windows 版本的说明：其余平台不需要额外处理。
#[cfg(not(windows))]
pub fn command(program: impl AsRef<OsStr>) -> Command {
    Command::new(program)
}
