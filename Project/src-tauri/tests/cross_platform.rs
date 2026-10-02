//! 跨平台预留检查（作为真实交叉编译的替代取证）。
//!
//! # 为什么不是真实交叉编译
//!
//! 理想情况下应保证 `cargo check --target x86_64-unknown-linux-gnu` 通过。
//! 本机**做不到**：Rust 由 Scoop 安装、没有 rustup，`rustlib/` 下只有
//! `x86_64-pc-windows-msvc`，实测 `cargo check --target x86_64-unknown-linux-gnu`
//! 报 E0463（找不到 `std`），提示正是 `rustup target add`。
//! `ARCHITECTURE.md` §6.6 早已预告此点。
//!
//! # 本测试证明什么、不证明什么
//!
//! 跨平台预留真正要防的是"非 Windows 上一编译就崩、一运行就 panic"。
//! 本测试用**机械核对**覆盖它的两个可机械化的部分：
//!
//! 1. 平台专有调用是否**全部收敛在 `platform/`**（漏一个出去，别的平台就编译不过）；
//! 2. 非 Windows 分支是否有**显式实现**，且不出现 `unimplemented!`/`todo!`
//!    这类"编译过了但一跑就崩"的占位（`platform/link.rs` 里那条红线）。
//!
//! **它证明不了**：类型错误、依赖本身的跨平台性、链接阶段的问题。
//! 也就是说，这是一份**审计**，不是一次编译。真实结论必须以某天能在
//! 非 Windows 上跑一次 `cargo check` 为准——在此之前，跨平台结论
//! 都应如实写明"未真实交叉编译"。

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// 递归收集所有 .rs 文件
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// 剥掉注释与测试代码，只留**产品代码**。
///
/// 两个都必须做，否则审计会读到自己想禁止的那些词：
///
/// - **注释**：`platform/link.rs` 里正是一条注释在写"不得用
///   `unimplemented!()`/`panic!()`"。不剥注释的话，这条"禁止占位宏"的
///   检查会被它自己的说明文字触发——审计里最典型的假阳性。
/// - **测试代码**：测试里出现 `panic!`/`unwrap` 是正常的。
///
/// 剥注释用一个小状态机而不是正则：字符串字面量里的 `//`（比如 URL）
/// 不能被当成注释起点。
fn production_part(source: &str) -> String {
    let source = match source.find("#[cfg(test)]") {
        Some(index) => &source[..index],
        None => source,
    };
    strip_comments(source)
}

/// 去掉行注释与块注释，保留字符串字面量里的内容
fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    // 0 = 普通, 1 = 行注释, 2 = 块注释（含嵌套层数）, 3 = 字符串字面量
    let mut block_depth = 0usize;
    let mut in_string = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                // 转义：下一个字符原样吃掉，连 `\"` 也不会提前结束字符串
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }

        if block_depth > 0 {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                block_depth -= 1;
            } else if c == '/' && chars.peek() == Some(&'*') {
                chars.next();
                block_depth += 1;
            }
            continue;
        }

        if c == '/' && chars.peek() == Some(&'/') {
            // 行注释：吃到行尾（换行保留，避免把两行粘起来）
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }

        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            block_depth += 1;
            continue;
        }

        if c == '"' {
            in_string = true;
        }
        out.push(c);
    }

    out
}

#[test]
fn platform_specific_calls_are_confined_to_the_platform_module() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    assert!(!files.is_empty(), "没找到任何源文件，审计本身失效了");

    // 这些记号一出现在 platform/ 之外，就意味着别的平台编译不过
    const FORBIDDEN: [&str; 5] = [
        "windows_sys",
        "std::os::windows",
        "junction::",
        "Win32::",
        "windows::",
    ];

    let mut violations = Vec::new();
    for file in &files {
        let relative = file
            .strip_prefix(src_dir())
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");

        // platform/ 是这些调用的唯一合法去处
        if relative.starts_with("platform/") {
            continue;
        }

        let source = production_part(&std::fs::read_to_string(file).unwrap_or_default());
        for marker in FORBIDDEN {
            if source.contains(marker) {
                violations.push(format!("{relative} 里出现了 {marker}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "平台专有调用漏到了 platform/ 之外，非 Windows 平台会编译失败：\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_unimplemented_placeholders_in_production_code() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);

    let mut violations = Vec::new();
    for file in &files {
        let source = production_part(&std::fs::read_to_string(file).unwrap_or_default());
        for marker in ["unimplemented!", "todo!"] {
            if source.contains(marker) {
                violations.push(format!(
                    "{} 里出现了 {marker}",
                    file.strip_prefix(src_dir())
                        .unwrap_or(file)
                        .to_string_lossy()
                        .replace('\\', "/")
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "非 Windows 分支必须是显式的错误，不能用占位宏（编译过了但一跑就崩）：\n{}",
        violations.join("\n")
    );
}

/// 链接能力在非 Windows 上必须**明确报不支持**，且不能是"静默成功"
#[test]
fn link_stub_reports_unsupported_rather_than_succeeding_silently() {
    let source = std::fs::read_to_string(src_dir().join("platform/link.rs")).unwrap();

    let stub_start = source
        .find("#[cfg(not(windows))]")
        .expect("platform/link.rs 里没有非 Windows 分支——别的平台会编译失败");
    let stub = &source[stub_start..];

    assert!(
        stub.contains("ErrorKind::Unsupported"),
        "非 Windows 的链接实现没有返回 Unsupported"
    );
    // 三个会产生副作用的操作都必须是显式错误
    for op in ["pub fn create", "pub fn delete", "pub fn target"] {
        let index = stub
            .find(op)
            .unwrap_or_else(|| panic!("非 Windows 分支缺少 {op}"));
        let body = &stub[index..];
        let end = body.find("\n    }").unwrap_or(body.len());
        assert!(
            body[..end].contains("Err("),
            "{op} 的非 Windows 实现没有返回错误——静默成功比报错危险得多"
        );
    }
}

/// 卷信息查询在非 Windows 上返回 Unknown（它是一个没有错误路径的查询）
#[test]
fn volume_stub_degrades_to_unknown() {
    let source = std::fs::read_to_string(src_dir().join("platform/volume.rs")).unwrap();
    let stub_start = source
        .find("#[cfg(not(windows))]")
        .expect("platform/volume.rs 里没有非 Windows 分支");
    let stub = &source[stub_start..];

    assert!(
        stub.contains("VolumeKind::Unknown"),
        "非 Windows 的卷查询应当优雅降级为 Unknown"
    );
}

/// 派生外部命令必须走 `platform::process::command`。
///
/// # 这条守的是什么
///
/// 发布构建是 GUI 子系统进程（进程自己没有控制台）。此时 Windows 会给**每一个**
/// 控制台子进程（`git.exe`、`cmdkey.exe`…）新建一个终端窗口，而每个新窗口都会
/// 抢走前台焦点——用户看到的是"一点按钮就闪出一串终端窗口，同时卡一下"。
/// 只有 `platform::process::command` 会加上 `CREATE_NO_WINDOW`。
///
/// 所以这里直接禁止 `platform/` 之外出现 `Command::new`：**漏一处就还会闪**，
/// 而这种漏是机械可查的，没理由靠人记得。
#[test]
fn external_processes_go_through_the_platform_helper() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    assert!(!files.is_empty(), "没找到任何源文件，审计本身失效了");

    let mut violations = Vec::new();
    for file in &files {
        let relative = file
            .strip_prefix(src_dir())
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");

        if relative.starts_with("platform/") {
            continue;
        }

        // `Command::new` 也算：直接用标准库就等于绕过了 CREATE_NO_WINDOW
        let source = production_part(&std::fs::read_to_string(file).unwrap_or_default());
        for marker in ["Command::new", "process::Command"] {
            if source.contains(marker) {
                violations.push(format!("{relative} 里出现了 {marker}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "有地方绕过了 platform::process::command，发布构建里会闪终端窗口：\n{}",
        violations.join("\n")
    );
}

/// `platform::process::command` 在 Windows 上确实禁用了控制台窗口。
///
/// 上一条测试保证"都走这里"，这一条保证"这里真的做了那件事"——
/// 两半都成立，闪窗才不会回来。
#[test]
fn the_process_helper_actually_suppresses_the_console_window() {
    let source = std::fs::read_to_string(src_dir().join("platform/process.rs")).unwrap();

    let windows_start = source
        .find("#[cfg(windows)]")
        .expect("platform/process.rs 里没有 Windows 分支");
    let not_windows_start = source
        .find("#[cfg(not(windows))]")
        .expect("platform/process.rs 里没有非 Windows 分支");

    let windows_branch = &source[windows_start..not_windows_start];
    assert!(
        windows_branch.contains("creation_flags"),
        "Windows 分支没有设置 creation_flags——终端窗口还会闪"
    );
    assert!(
        windows_branch.contains("0x0800_0000") || windows_branch.contains("CREATE_NO_WINDOW"),
        "Windows 分支没有使用 CREATE_NO_WINDOW"
    );

    let not_windows_branch = &source[not_windows_start..];
    assert!(
        not_windows_branch.contains("Command::new"),
        "非 Windows 分支没有真正建出命令"
    );
}

/// 每个 `#[cfg(windows)]` 都要有对应的 `#[cfg(not(windows))]`，否则别的平台缺实现
#[test]
fn every_windows_branch_has_a_non_windows_counterpart() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);

    let mut missing = Vec::new();
    for file in &files {
        let source = production_part(&std::fs::read_to_string(file).unwrap_or_default());
        let windows = source.matches("#[cfg(windows)]").count();
        let not_windows = source.matches("#[cfg(not(windows))]").count();

        if windows > not_windows {
            missing.push(format!(
                "{}：{} 个 cfg(windows) 只配了 {} 个 cfg(not(windows))",
                file.strip_prefix(src_dir())
                    .unwrap_or(file)
                    .to_string_lossy()
                    .replace('\\', "/"),
                windows,
                not_windows
            ));
        }
    }

    assert!(
        missing.is_empty(),
        "存在没有非 Windows 实现的 Windows 分支：\n{}",
        missing.join("\n")
    );
}
