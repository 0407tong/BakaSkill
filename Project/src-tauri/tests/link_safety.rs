//! **本项目最高优先级的回归测试。**
//!
//! junction 的删除语义有一个致命陷阱：递归删除（`remove_dir_all`）会
//! **穿透重解析点，删掉链接指向的真实目录内容**。对一个管理用户 Skill 的
//! 工具来说，这意味着"用户点了一下禁用按钮，中央库里的 Skill 没了"。
//!
//! 本文件用真实文件系统验证 `platform::link` 的安全约束。这些测试一旦失败，
//! 不得通过修改测试来"修复"——那说明实现里出现了数据丢失级的缺陷。
//!
//! 仅在 Windows 上有意义：其他平台 junction 不可用，`platform::link`
//! 返回 `Unsupported`。

#![cfg(windows)]

use std::path::{Path, PathBuf};

use bakaskill_lib::error::AppError;
use bakaskill_lib::platform::link::{self, LinkKind};

/// 造一个"中央库里的 Skill"：目录 + SKILL.md + 一个资源文件
fn make_skill_library(base: &Path) -> PathBuf {
    let library = base.join("library").join("pdf-tools");
    std::fs::create_dir_all(&library).unwrap();
    std::fs::write(
        library.join("SKILL.md"),
        "---\nname: pdf-tools\ndescription: 处理 PDF\n---\n\n正文\n",
    )
    .unwrap();
    std::fs::create_dir_all(library.join("assets")).unwrap();
    std::fs::write(library.join("assets").join("template.txt"), "模板内容").unwrap();
    library
}

fn assert_library_intact(library: &Path) {
    assert!(library.is_dir(), "源目录本身没了：{}", library.display());
    assert!(
        library.join("SKILL.md").is_file(),
        "SKILL.md 被删除了：{}",
        library.display()
    );
    assert!(
        library.join("assets").join("template.txt").is_file(),
        "资源文件被删除了"
    );
}

// ===========================================================================
// 删除链接后源目录完好无损
// ===========================================================================

#[test]
fn deleting_junction_leaves_target_contents_intact() {
    let tmp = tempfile::tempdir().unwrap();
    let library = make_skill_library(tmp.path());
    let link_path = tmp.path().join("agent-skills").join("pdf-tools");
    std::fs::create_dir_all(link_path.parent().unwrap()).unwrap();

    // 建链
    link::create_junction(&library, &link_path).unwrap();
    assert_eq!(link::classify(&link_path).unwrap(), LinkKind::Junction);

    // 通过链接读到的就是源目录的内容（同一份数据，不是副本）
    let via_link = std::fs::read_to_string(link_path.join("SKILL.md")).unwrap();
    assert!(via_link.contains("pdf-tools"));

    // 删除链接
    link::delete_junction(&link_path).unwrap();

    // 链接消失
    assert!(!link_path.exists());
    assert_eq!(link::classify(&link_path).unwrap(), LinkKind::Missing);

    // **源目录必须分毫未动** —— 这是本文件存在的理由
    assert_library_intact(&library);
}

/// 回归：`junction::delete` 底层是 `FSCTL_DELETE_REPARSE_POINT`，
/// 它只把重解析点从目录项上摘掉，**会留下一个真实空目录**。
///
/// 若不清理这个空壳：
/// - 用户会在 Agent 目录里看到一个残留的空文件夹；
/// - 再次"启用"该 Skill 时会因"位置已被占用"而失败。
#[test]
fn delete_removes_the_leftover_empty_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let library = make_skill_library(tmp.path());
    let link_path = tmp.path().join("skills").join("pdf-tools");
    std::fs::create_dir_all(link_path.parent().unwrap()).unwrap();

    link::create_junction(&library, &link_path).unwrap();
    link::delete_junction(&link_path).unwrap();

    assert!(
        !link_path.exists(),
        "删除链接后残留了空目录，会导致下次启用失败"
    );
    assert_eq!(link::classify(&link_path).unwrap(), LinkKind::Missing);
    assert_library_intact(&library);
}

/// 写入穿透性验证：通过链接写入的内容，删除链接后仍留在源目录
#[test]
fn writing_through_link_persists_after_link_removal() {
    let tmp = tempfile::tempdir().unwrap();
    let library = make_skill_library(tmp.path());
    let link_path = tmp.path().join("skills").join("pdf-tools");
    std::fs::create_dir_all(link_path.parent().unwrap()).unwrap();

    link::create_junction(&library, &link_path).unwrap();
    std::fs::write(link_path.join("written-through-link.txt"), "内容").unwrap();
    link::delete_junction(&link_path).unwrap();

    assert!(
        library.join("written-through-link.txt").is_file(),
        "通过链接写入的数据随链接一起丢失了——说明删除穿透到了目标"
    );
}

// ===========================================================================
// 安全闸门：拒绝删除任何非 junction 的路径
// ===========================================================================

#[test]
fn refuses_to_delete_a_real_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("i-am-real");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("important.txt"), "重要数据").unwrap();

    let err = link::delete_junction(&real).unwrap_err();
    assert!(
        matches!(err, AppError::NotAJunction(_)),
        "真实目录必须被拒绝，实际错误：{err:?}"
    );

    // 拒绝之后目录与其内容必须原样存在
    assert!(real.is_dir());
    assert!(real.join("important.txt").is_file());
}

#[test]
fn refuses_to_delete_a_missing_path() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("nothing-here");
    let err = link::delete_junction(&missing).unwrap_err();
    assert!(
        matches!(err, AppError::NotFound(_)),
        "不存在的路径应返回 NotFound，实际：{err:?}"
    );
}

#[test]
fn refuses_to_delete_a_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    std::fs::create_dir_all(&target).unwrap();
    let symlink = tmp.path().join("a-symlink");

    // 创建目录符号链接需要管理员权限或开发者模式；无权限时跳过而不是失败
    if std::os::windows::fs::symlink_dir(&target, &symlink).is_err() {
        eprintln!("跳过：当前环境无法创建符号链接（需要管理员或开发者模式）");
        return;
    }

    let err = link::delete_junction(&symlink).unwrap_err();
    assert!(
        matches!(err, AppError::NotAJunction(_)),
        "符号链接不应被当作 junction 删除，实际：{err:?}"
    );
    assert!(target.is_dir(), "目标目录被误删");
}

// ===========================================================================
// 创建侧的安全约束
// ===========================================================================

#[test]
fn refuses_to_create_when_link_path_is_occupied() {
    let tmp = tempfile::tempdir().unwrap();
    let library = make_skill_library(tmp.path());
    let occupied = tmp.path().join("occupied");
    std::fs::create_dir_all(&occupied).unwrap();
    std::fs::write(occupied.join("existing.txt"), "已存在").unwrap();

    let err = link::create_junction(&library, &occupied).unwrap_err();
    assert!(
        matches!(err, AppError::Conflict(_)),
        "已占用的位置应返回 Conflict，实际：{err:?}"
    );
    // 原有内容不得被动过
    assert!(occupied.join("existing.txt").is_file());
}

#[test]
fn refuses_to_create_when_target_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let missing_target = tmp.path().join("no-such-target");
    let link_path = tmp.path().join("link");

    let err = link::create_junction(&missing_target, &link_path).unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)), "实际：{err:?}");
    assert!(!link_path.exists());
}

#[test]
fn refuses_to_create_when_target_is_a_file() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("a-file.txt");
    std::fs::write(&file, "x").unwrap();
    let link_path = tmp.path().join("link");

    let err = link::create_junction(&file, &link_path).unwrap_err();
    assert!(matches!(err, AppError::Config(_)), "实际：{err:?}");
}

// ===========================================================================
// 状态查询
// ===========================================================================

#[test]
fn reports_target_and_detects_dangling_link() {
    let tmp = tempfile::tempdir().unwrap();
    let library = make_skill_library(tmp.path());
    let link_path = tmp.path().join("skills").join("pdf-tools");
    std::fs::create_dir_all(link_path.parent().unwrap()).unwrap();
    link::create_junction(&library, &link_path).unwrap();

    let target = link::junction_target(&link_path).unwrap().unwrap();
    assert_eq!(link::classify(&link_path).unwrap(), LinkKind::Junction);
    assert!(target.exists(), "目标应可达");

    // 制造断链：把目标目录改名（模拟中央库被移动/盘符变化）
    let moved = tmp.path().join("library-moved");
    std::fs::rename(tmp.path().join("library"), &moved).unwrap();

    assert_eq!(
        link::classify(&link_path).unwrap(),
        LinkKind::Junction,
        "目标不可达时，链接本身仍应被识别为 junction（这是断链检测的前提）"
    );
    let target = link::junction_target(&link_path).unwrap().unwrap();
    assert!(
        !target.exists(),
        "目标已不可达，target_exists 应为 false，从而被判定为断链"
    );

    // 断链状态下仍必须能安全删除该链接（"修复断链"依赖这一点）
    link::delete_junction(&link_path).expect("断链也应当可以被安全移除");
    assert!(!link_path.exists());
    assert!(moved.is_dir(), "断链删除不应影响链接指向的位置");
}

#[test]
fn junction_target_of_non_link_is_none() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real");
    std::fs::create_dir_all(&real).unwrap();
    assert!(link::junction_target(&real).unwrap().is_none());
}

// ===========================================================================
// 列举：不跟随重解析点
// ===========================================================================

#[test]
fn list_links_does_not_follow_reparse_points() {
    let tmp = tempfile::tempdir().unwrap();
    let library = make_skill_library(tmp.path());
    let agent_dir = tmp.path().join("agent-skills");
    std::fs::create_dir_all(&agent_dir).unwrap();

    // 一个真实目录（不是链接）
    let real_skill = agent_dir.join("external-skill");
    std::fs::create_dir_all(&real_skill).unwrap();
    std::fs::write(real_skill.join("SKILL.md"), "---\nname: external\n---\n").unwrap();

    // 一个链接
    link::create_junction(&library, &agent_dir.join("pdf-tools")).unwrap();

    let links = link::list_links_in(&agent_dir).unwrap();
    assert_eq!(links.len(), 1, "只应列出链接，不应把真实目录算进来");
    assert!(links[0].0.ends_with("pdf-tools"));

    // `junction::get_target` 返回的是不带 `\\?\` 前缀的普通路径（便于展示），
    // 而 `std::fs::canonicalize` 会带上该前缀，因此比较前先规范化。
    assert_eq!(normalize(&links[0].1), normalize(&library));
}

/// 去掉 `\\?\` 前缀与末尾分隔符，统一大小写后比较
fn normalize(path: &Path) -> String {
    let text = path.to_string_lossy().to_lowercase();
    let stripped = text.strip_prefix(r"\\?\").unwrap_or(&text);
    stripped.trim_end_matches('\\').to_string()
}

/// 回归：junction 在 Windows 上也满足 `FileType::is_symlink()`，
/// 因此它**绝不能**被归类为符号链接。
///
/// 如果这条测试失败，`delete_junction` 会拒绝删除任何真实链接，
/// 表现为"一键禁用"完全不工作。
#[test]
fn junction_is_never_classified_as_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let library = make_skill_library(tmp.path());
    let link_path = tmp.path().join("skills").join("pdf-tools");
    std::fs::create_dir_all(link_path.parent().unwrap()).unwrap();

    link::create_junction(&library, &link_path).unwrap();

    // 前提确认：这正是会误导判断顺序的那个属性
    let meta = std::fs::symlink_metadata(&link_path).unwrap();
    assert!(
        meta.file_type().is_symlink(),
        "前提失效：junction 不再满足 is_symlink，判断顺序的注释需要更新"
    );

    assert_eq!(
        link::classify(&link_path).unwrap(),
        LinkKind::Junction,
        "junction 被判成了符号链接"
    );
    assert!(link::is_junction(&link_path));

    // 而且必须能正常删除
    link::delete_junction(&link_path).expect("合法链接应当可以被删除");
    assert_library_intact(&library);
}

#[test]
fn multiple_links_to_same_target_are_independent() {
    let tmp = tempfile::tempdir().unwrap();
    let library = make_skill_library(tmp.path());
    let agent_a = tmp.path().join("agent-a");
    let agent_b = tmp.path().join("agent-b");
    std::fs::create_dir_all(&agent_a).unwrap();
    std::fs::create_dir_all(&agent_b).unwrap();

    let link_a = agent_a.join("pdf-tools");
    let link_b = agent_b.join("pdf-tools");
    link::create_junction(&library, &link_a).unwrap();
    link::create_junction(&library, &link_b).unwrap();

    // 移除其中一个，另一个与源目录都不受影响
    link::delete_junction(&link_a).unwrap();

    assert!(!link_a.exists());
    assert!(link_b.is_dir(), "另一个 Agent 的链接被误删");
    assert_library_intact(&library);
}
