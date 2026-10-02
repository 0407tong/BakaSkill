//! 卷信息查询（文件系统类型、卷类型、可用空间）。
//!
//! 为什么需要它：junction 只在 **NTFS / ReFS 本地卷**上可靠。把中央库放到
//! FAT32、网络盘或 OneDrive 同步目录，会得到"看起来能用、事后出问题"的
//! 结果，因此选择路径时必须前置诊断。

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{AppError, AppResult};

/// 卷类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum VolumeKind {
    Fixed,
    Removable,
    Network,
    CdRom,
    RamDisk,
    Unknown,
}

/// 卷信息
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    pub kind: VolumeKind,
    /// 文件系统名，例如 `NTFS` / `ReFS` / `FAT32`
    pub filesystem: Option<String>,
    /// 调用方可用的剩余字节数
    pub free_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
}

impl VolumeInfo {
    /// 该卷是否满足 junction 的使用前提
    pub fn supports_junction(&self) -> bool {
        let fs_ok = matches!(
            self.filesystem
                .as_deref()
                .map(str::to_ascii_uppercase)
                .as_deref(),
            Some("NTFS") | Some("REFS")
        );
        // 本地固定卷才能给出可靠的文件系统判断；未知时按"不支持"处理，
        // 宁可在选择阶段提示用户，也不要事后出现悬空链接。
        fs_ok && matches!(self.kind, VolumeKind::Fixed | VolumeKind::Removable)
    }
}

/// 查询 `path` 所在卷的信息。
///
/// 路径**不需要存在**——只取盘符/UNC 前缀查询，因此可用于校验用户
/// 尚未创建的目录。
pub fn query(path: &Path) -> AppResult<VolumeInfo> {
    let root = drive_root(path)
        .ok_or_else(|| AppError::Config(format!("无法从路径中解析出盘符：{}", path.display())))?;
    Ok(platform_query(&root))
}

/// 从路径中提取用于卷查询的根（`D:\` 或 `\\server\share`）
fn drive_root(path: &Path) -> Option<PathBuf> {
    let text = path.as_os_str().to_string_lossy();

    // UNC：\\server\share\...
    if let Some(rest) = text.strip_prefix(r"\\") {
        let mut parts = rest.splitn(3, '\\');
        let server = parts.next().filter(|s| !s.is_empty())?;
        let share = parts.next().filter(|s| !s.is_empty())?;
        return Some(PathBuf::from(format!(r"\\{server}\{share}")));
    }

    // 盘符：X:\...
    let mut chars = text.chars();
    let letter = chars.next()?;
    if letter.is_ascii_alphabetic() && chars.next() == Some(':') {
        return Some(PathBuf::from(format!("{letter}:\\")));
    }

    None
}

// ===========================================================================
// Windows 实现
// ===========================================================================

#[cfg(windows)]
fn platform_query(root: &Path) -> VolumeInfo {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetVolumeInformationW,
    };

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    let root_w = wide(root.as_os_str());

    // SAFETY: root_w 是以 NUL 结尾的宽字符串，指针在调用期间有效。
    let drive_type = unsafe { GetDriveTypeW(root_w.as_ptr()) };

    let kind = match drive_type {
        2 => VolumeKind::Removable,
        3 => VolumeKind::Fixed,
        4 => VolumeKind::Network,
        5 => VolumeKind::CdRom,
        6 => VolumeKind::RamDisk,
        _ => VolumeKind::Unknown,
    };

    // 文件系统名与容量对网络盘/CD-ROM 未必可查，失败时保持 None 而不是报错——
    // 诊断应当尽可能给出结论，而不是因为一个字段查不到就整体失败。
    let mut fs_buf = [0u16; 256];
    let mut volume_name = [0u16; 256];
    let mut serial: u32 = 0;
    let mut max_component: u32 = 0;
    let mut flags: u32 = 0;

    // SAFETY: 所有缓冲区都是本栈上有效的定长数组，且长度参数与之匹配。
    let fs_ok = unsafe {
        GetVolumeInformationW(
            root_w.as_ptr(),
            volume_name.as_mut_ptr(),
            volume_name.len() as u32,
            &mut serial,
            &mut max_component,
            &mut flags,
            fs_buf.as_mut_ptr(),
            fs_buf.len() as u32,
        )
    };

    let filesystem = if fs_ok != 0 {
        let len = fs_buf.iter().position(|&c| c == 0).unwrap_or(fs_buf.len());
        let name = String::from_utf16_lossy(&fs_buf[..len]);
        (!name.is_empty()).then_some(name)
    } else {
        None
    };

    let mut free_to_caller: u64 = 0;
    let mut total: u64 = 0;
    let mut total_free: u64 = 0;

    // SAFETY: 三个输出参数都是本栈上有效的 u64，指针在调用期间有效。
    let space_ok = unsafe {
        GetDiskFreeSpaceExW(
            root_w.as_ptr(),
            &mut free_to_caller,
            &mut total,
            &mut total_free,
        )
    };

    VolumeInfo {
        kind,
        filesystem,
        free_bytes: (space_ok != 0).then_some(free_to_caller),
        total_bytes: (space_ok != 0).then_some(total),
    }
}

// ===========================================================================
// 非 Windows stub
// ===========================================================================

#[cfg(not(windows))]
fn platform_query(_root: &Path) -> VolumeInfo {
    VolumeInfo {
        kind: VolumeKind::Unknown,
        filesystem: None,
        free_bytes: None,
        total_bytes: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_drive_letter_root() {
        assert_eq!(
            drive_root(Path::new(r"D:\SkillHubLibrary\skills")).unwrap(),
            PathBuf::from(r"D:\")
        );
        assert_eq!(
            drive_root(Path::new(r"C:\")).unwrap(),
            PathBuf::from(r"C:\")
        );
    }

    #[test]
    fn extracts_unc_root() {
        assert_eq!(
            drive_root(Path::new(r"\\server\share\folder\sub")).unwrap(),
            PathBuf::from(r"\\server\share")
        );
    }

    #[test]
    fn relative_path_has_no_root() {
        assert!(drive_root(Path::new(r"some\relative\path")).is_none());
    }

    #[test]
    fn only_ntfs_and_refs_support_junction() {
        let mk = |fs: Option<&str>, kind: VolumeKind| VolumeInfo {
            kind,
            filesystem: fs.map(str::to_string),
            free_bytes: None,
            total_bytes: None,
        };
        assert!(mk(Some("NTFS"), VolumeKind::Fixed).supports_junction());
        assert!(mk(Some("ntfs"), VolumeKind::Fixed).supports_junction());
        assert!(mk(Some("ReFS"), VolumeKind::Fixed).supports_junction());
        assert!(!mk(Some("FAT32"), VolumeKind::Fixed).supports_junction());
        assert!(!mk(Some("NTFS"), VolumeKind::Network).supports_junction());
        assert!(!mk(None, VolumeKind::Fixed).supports_junction());
    }

    #[cfg(windows)]
    #[test]
    fn queries_current_drive() {
        let info = query(Path::new(r"C:\")).unwrap();
        assert_eq!(info.kind, VolumeKind::Fixed);
        assert_eq!(info.filesystem.as_deref(), Some("NTFS"));
        assert!(info.free_bytes.unwrap() > 0);
    }
}
