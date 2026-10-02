//! 命令不得占用 UI 线程。
//!
//! # 这条守的是什么
//!
//! Tauri 的 `#[tauri::command]` **默认在主线程上执行**同步函数体
//! （`tauri-macros` 的文档：`(async)` 是 "for synchronous functions that should
//! not run on the main thread"）。主线程同时负责窗口的消息泵，所以一个不返回的
//! 命令会让**整个窗口**变成"未响应"。
//!
//! 实测撞到过：`git_login_browser` 会一直等浏览器回调，用户把授权页直接关掉时
//! 它没有任何信号可等，于是应用直接卡死。修法是给命令加 `(async)`，
//! 让它跑到异步运行时上。
//!
//! 这类缺陷的危险在于：**平时看不出来**。快命令占主线程几毫秒，没人会察觉；
//! 只有当某条路径变慢（网络、等待用户、大目录）时才炸，而那时已经很难联想起
//! "是因为少写了一对括号"。
//!
//! 所以这里做一条机械检查：本项目的命令**一律**要写 `(async)`。
//! 不区分快慢——统一规则才守得住，判断"这条够不够快"迟早会判错。

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

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

/// 剥掉注释，避免审计读到自己说明里的字面量（本文件正文里就写着 `#[tauri::command]`）。
fn without_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut in_block = false;

    while let Some(c) = chars.next() {
        if in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block = false;
            }
            continue;
        }

        if c == '/' && chars.peek() == Some(&'/') {
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
            in_block = true;
            continue;
        }

        out.push(c);
    }

    out
}

#[test]
fn every_command_runs_off_the_main_thread() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    assert!(!files.is_empty(), "没找到任何源文件，审计本身失效了");

    let mut bare = Vec::new();
    let mut total = 0usize;

    for file in &files {
        let source = without_comments(&std::fs::read_to_string(file).unwrap_or_default());
        let relative = file
            .strip_prefix(src_dir())
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");

        for (index, _) in source.match_indices("#[tauri::command]") {
            // 定位到它所在的行，报错时给出行号
            let line = source[..index].lines().count() + 1;
            bare.push(format!("{relative}:{line}"));
        }

        total += source.matches("#[tauri::command").count();
    }

    assert!(
        bare.is_empty(),
        "这些命令没有写 `(async)`，会跑在 UI 线程上——一旦某条路径变慢，\
         整个窗口就会「未响应」：\n{}\n\n\
         写法：`#[tauri::command(async)]`（本项目 46 条命令全部如此）",
        bare.join("\n")
    );

    assert!(
        total >= 40,
        "只找到 {total} 条命令，比预期少得多——审计的正则可能失效了"
    );
}
