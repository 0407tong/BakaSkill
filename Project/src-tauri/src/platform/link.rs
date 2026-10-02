//! 目录链接（junction）服务 —— **本项目安全等级最高的模块**。
//!
//! # 为什么用 junction
//!
//! `mklink /J` 创建的目录 junction 不需要管理员权限，而目录符号链接
//! （`mklink /D`）需要管理员或开发者模式。本项目一律使用 junction。
//! 详细对比见 `docs/LINKING.md`。
//!
//! # 安全红线
//!
//! 递归删除一个 junction 会**穿透到它指向的真实目录**，把中央库里的
//! Skill 内容一起删掉。因此：
//!
//! 1. 模块外**禁止**对任何可能为链接的路径调用 `remove_dir_all` /
//!    `remove_dir` / `RemoveDirectoryW`。
//! 2. [`delete_junction`] 在删除前必须确认目标**确实是** junction，
//!    否则返回 [`AppError::NotAJunction`] 而不是继续。
//!
//! 这两条由 `tests/link_safety.rs` 的回归测试守护。

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// 目录项的类型判定结果
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkKind {
    /// 目录 junction（本项目使用的唯一链接形式）
    Junction,
    /// 目录符号链接（不是本项目创建的，只读展示）
    SymlinkDir,
    /// 文件符号链接
    SymlinkFile,
    /// 真实目录或文件，非链接
    NotALink,
    /// 路径不存在
    Missing,
}

/// 创建目录 junction：`link` 指向 `target`。
///
/// **注意参数顺序**：`junction` crate 的签名是 `create(target, junction)`，
/// 即目标在前、链接在后——与多数人的直觉相反，写反会得到难以理解的报错。
///
/// 创建后会**回读校验**：杀毒软件偶尔会拦截新建的 reparse point，
/// 此时函数会"成功"返回但链接实际不可用，因此必须读回确认。
pub fn create_junction(target: &Path, link: &Path) -> AppResult<()> {
    if !target.exists() {
        return Err(AppError::NotFound(format!(
            "链接目标不存在：{}",
            display(target)
        )));
    }
    if !target.is_dir() {
        return Err(AppError::Config(format!(
            "链接目标不是目录：{}",
            display(target)
        )));
    }
    if link.exists() || path_entry_exists(link) {
        return Err(AppError::Conflict(format!(
            "链接位置已被占用：{}",
            display(link)
        )));
    }
    if let Some(parent) = link.parent() {
        if !parent.exists() {
            return Err(AppError::NotFound(format!(
                "链接位置的上级目录不存在：{}",
                display(parent)
            )));
        }
    }

    platform_create(target, link)?;

    // 回读校验
    let actual = junction_target(link)?;
    match actual {
        Some(found) if paths_equal(&found, target) => Ok(()),
        Some(found) => Err(AppError::Internal(format!(
            "创建后回读的目标与预期不符：预期 {}，实际 {}",
            display(target),
            display(&found)
        ))),
        None => Err(AppError::Internal(format!(
            "创建后回读失败，链接可能被杀毒软件拦截：{}",
            display(link)
        ))),
    }
}

/// 删除目录 junction，**只删除链接本体，绝不动目标内容**。
///
/// 若 `link` 不是 junction（例如是真实目录、或是符号链接），返回
/// [`AppError::NotAJunction`] 并拒绝执行——这是防止误删用户数据的关键闸门。
pub fn delete_junction(link: &Path) -> AppResult<()> {
    match classify(link)? {
        LinkKind::Junction => {
            platform_delete(link)?;
            // 校验链接确实已消失
            if is_junction(link) {
                return Err(AppError::Internal(format!(
                    "删除后链接仍然存在：{}",
                    display(link)
                )));
            }
            Ok(())
        }
        LinkKind::Missing => Err(AppError::NotFound(format!("路径不存在：{}", display(link)))),
        LinkKind::SymlinkDir | LinkKind::SymlinkFile => Err(AppError::NotAJunction(format!(
            "{} 是符号链接而非 junction。本项目不代管符号链接，拒绝删除。",
            display(link)
        ))),
        LinkKind::NotALink => Err(AppError::NotAJunction(format!(
            "{} 是真实目录，不是链接。删除它会丢失真实数据，已拒绝。",
            display(link)
        ))),
    }
}

/// 读回 junction 的目标路径。非 junction 或不存在时返回 `None`。
///
/// 注意：目标不存在（悬空链接）时仍会返回 `Some(目标路径)`，
/// 调用方需自行判断目标是否可达。
pub fn junction_target(link: &Path) -> AppResult<Option<PathBuf>> {
    if !is_junction(link) {
        return Ok(None);
    }
    match platform_target(link) {
        Ok(target) => Ok(Some(target)),
        Err(err) => Err(AppError::from_io(
            &format!("读取链接目标失败：{}", display(link)),
            err,
        )),
    }
}

/// 判定目录项类型
pub fn classify(path: &Path) -> AppResult<LinkKind> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(LinkKind::Missing),
        Err(err) => {
            return Err(AppError::from_io(
                &format!("读取路径属性失败：{}", display(path)),
                err,
            ))
        }
    };

    // ⚠️ 判断顺序不能颠倒。
    //
    // Windows 上 junction 也是 name-surrogate 重解析点，Rust 的
    // `FileType::is_symlink()` 对 junction **同样返回 true**。
    // 若先判断 is_symlink，junction 会被误判成 SymlinkFile，
    // 进而导致 delete_junction 对每个真实链接都返回 NotAJunction 而拒绝执行
    // ——表现为"禁用按钮点了没反应"。因此必须先判 junction。
    if is_junction(path) {
        return Ok(LinkKind::Junction);
    }

    if metadata.file_type().is_symlink() {
        // 注意：对符号链接而言 `symlink_metadata().file_type().is_dir()`
        // 恒为 false（它描述的是链接本身而非目标），因此只能跟随判断。
        return Ok(if path.is_dir() {
            LinkKind::SymlinkDir
        } else {
            LinkKind::SymlinkFile
        });
    }

    Ok(LinkKind::NotALink)
}

/// 路径是否为一个 junction。
///
/// **对悬空链接同样返回 `true`**：判定依据是目录项自身的重解析标签，
/// 不跟随目标。这是断链检测能够工作的前提。
pub fn is_junction(path: &Path) -> bool {
    platform_is_junction(path)
}

/// 列出 `dir` 下的所有链接及其目标。
///
/// 遍历时**不跟随**重解析点——跟随会把中央库的内容当成 Agent 本地内容
/// 重复计数（扫描时表现为 Skill 数量暴涨）。
pub fn list_links_in(dir: &Path) -> AppResult<Vec<(PathBuf, PathBuf)>> {
    if !dir.is_dir() {
        return Err(AppError::NotFound(format!("目录不存在：{}", display(dir))));
    }

    let entries = std::fs::read_dir(dir)
        .map_err(|err| AppError::from_io(&format!("读取目录失败：{}", display(dir)), err))?;

    let mut links = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|err| AppError::from_io(&format!("遍历目录失败：{}", display(dir)), err))?;
        let path = entry.path();
        if is_junction(&path) {
            if let Some(target) = junction_target(&path)? {
                links.push((path, target));
            }
        }
    }
    links.sort();
    Ok(links)
}

/// 路径是否存在（**不跟随**重解析点）。
///
/// 用于判断"这个位置是否已被占用"：悬空的链接其 `Path::exists()` 为 false，
/// 但它仍然占据着目录项，不能在其上再建链接。
fn path_entry_exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// 比较两个路径是否指向同一位置，忽略 Windows 的大小写差异与 8.3 短名。
fn paths_equal(a: &Path, b: &Path) -> bool {
    let na = dunce::simplified(a).to_string_lossy().to_lowercase();
    let nb = dunce::simplified(b).to_string_lossy().to_lowercase();
    na.trim_end_matches(['\\', '/']) == nb.trim_end_matches(['\\', '/'])
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

// ===========================================================================
// Windows 实现
// ===========================================================================

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        FindClose, FindFirstFileW, FILE_ATTRIBUTE_REPARSE_POINT, WIN32_FIND_DATAW,
    };

    /// 重解析点标签：junction（即"目录挂载点"）
    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;

    pub fn create(target: &Path, link: &Path) -> io::Result<()> {
        // junction 2.x 的签名是 create(target, junction)
        junction::create(target, link)
    }

    /// 删除 junction。
    ///
    /// ⚠️ `junction::delete` 内部使用的是 `FSCTL_DELETE_REPARSE_POINT`：
    /// 它只是把重解析点从目录项上**摘掉**，会留下一个真实的空目录，而不是
    /// 删除目录项本身。若不处理，用户会在 Agent 目录里看到一个残留的空文件夹，
    /// 而且下次"启用"会因为"位置已被占用"而失败。
    ///
    /// 因此摘除重解析点后必须再删掉这个空壳。这里使用 `std::fs::remove_dir`：
    /// 它**只能删除空目录**，遇到非空目录会报错而不是递归删除，
    /// 因此哪怕前面的判断出错，也不可能删除任何用户数据。
    pub fn delete(link: &Path) -> io::Result<()> {
        junction::delete(link)?;

        // 确认重解析点确实已摘除，再动目录项
        if is_junction(link) {
            return Err(io::Error::other("重解析点未被移除，已中止以免误删目录"));
        }

        match std::fs::remove_dir(link) {
            Ok(()) => Ok(()),
            // 某些情况下 FSCTL 已连带移除目录项
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }

    pub fn target(link: &Path) -> io::Result<PathBuf> {
        junction::get_target(link)
    }

    /// 判定是否为 junction。
    ///
    /// **不使用 `junction::exists()`**：该函数开头会做 `Path::exists()`，
    /// 而 `Path::exists()` 会跟随重解析点，导致**断链（目标不存在）被判为
    /// 不是 junction**——那样就无法检测"中央库被移动/盘符变化"造成的断链。
    ///
    /// 这里改为直接读重解析标签（`WIN32_FIND_DATAW.dwReserved0`）：
    /// 它只描述目录项自身，不跟随目标，因此对悬空链接同样有效。
    pub fn is_junction(path: &Path) -> bool {
        reparse_tag(path) == Some(IO_REPARSE_TAG_MOUNT_POINT)
    }

    /// 读取路径的重解析标签；不是重解析点或路径不存在时返回 `None`
    fn reparse_tag(path: &Path) -> Option<u32> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: WIN32_FIND_DATAW 是 POD，全零初始化合法；
        // wide 是以 NUL 结尾的宽字符串，在调用期间有效。
        let mut data: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
        let handle = unsafe { FindFirstFileW(wide.as_ptr(), &mut data) };

        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        // SAFETY: handle 由 FindFirstFileW 返回且未关闭，此处关闭且不再使用
        unsafe { FindClose(handle) };

        if data.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
            return None;
        }
        // dwReserved0 对重解析点存放的就是标签
        Some(data.dwReserved0)
    }
}

// ===========================================================================
// 非 Windows stub
//
// 必须是显式返回 Err(Unsupported)，不得用 unimplemented!()/panic!()：
// 后者会让"跨平台编译通过"变成"一运行就崩"。
// ===========================================================================

#[cfg(not(windows))]
mod imp {
    use std::io;
    use std::path::{Path, PathBuf};

    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "当前平台不支持目录 junction（仅 Windows 提供）",
        )
    }

    pub fn create(_target: &Path, _link: &Path) -> io::Result<()> {
        Err(unsupported())
    }

    pub fn delete(_link: &Path) -> io::Result<()> {
        Err(unsupported())
    }

    pub fn target(_link: &Path) -> io::Result<PathBuf> {
        Err(unsupported())
    }

    pub fn is_junction(_path: &Path) -> bool {
        false
    }
}

fn platform_create(target: &Path, link: &Path) -> AppResult<()> {
    imp::create(target, link).map_err(|err| {
        AppError::from_io(
            &format!("创建链接失败 {} -> {}", display(link), display(target)),
            err,
        )
    })
}

fn platform_delete(link: &Path) -> AppResult<()> {
    imp::delete(link)
        .map_err(|err| AppError::from_io(&format!("删除链接失败：{}", display(link)), err))
}

fn platform_target(link: &Path) -> std::io::Result<PathBuf> {
    imp::target(link)
}

fn platform_is_junction(path: &Path) -> bool {
    imp::is_junction(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_missing_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        assert_eq!(classify(&missing).unwrap(), LinkKind::Missing);
    }

    #[test]
    fn classify_real_directory_is_not_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        assert_eq!(classify(&real).unwrap(), LinkKind::NotALink);
        assert!(!is_junction(&real));
    }

    #[test]
    fn junction_target_of_non_link_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        assert!(junction_target(&real).unwrap().is_none());
    }
}
