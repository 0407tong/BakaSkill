//! 自定义背景图。
//!
//! # 为什么是"复制进来"而不是记住原路径
//!
//! 用户选的图可能放在桌面、U 盘、下载目录里——那些地方的文件随时会被移走或删掉。
//! 记路径的做法看着省事，代价是**某天背景会莫名其妙变成空白**，而用户不知道为什么。
//! 复制一份进应用数据目录之后：原图怎么处置都不影响背景，且 asset 协议的放行范围
//! 得以收窄到这一个目录，不必为了"支持任意位置选图"开放整个文件系统。
//!
//! # 为什么还要查文件头
//!
//! 只认扩展名是不够的：把 `notes.txt` 改名成 `back.png` 也能过。那种文件塞给
//! 浏览器只会渲染失败，表现为"选了图之后背景变成一片空白"——一个说不清原因的坏结果。
//! 因此在**设置的时候**就把关，当场告诉用户"这不是一张图片"，而不是等到界面上出问题。

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config;
use crate::error::{AppError, AppResult};

/// 允许的背景图格式
const ALLOWED: [&str; 6] = ["png", "jpg", "jpeg", "webp", "bmp", "gif"];

/// 复制进来之后统一叫这个名字（扩展名保留原样）
const STEM: &str = "custom";

/// 上限：超过这个大小就不是"背景图"了，多半是选错了文件。
///
/// 定得比一般图片宽、但不放到很松，是因为前端**要经 IPC 拿到它的 base64**
/// （约放大三分之一）。一张 16 MB 的图会变成 21 MB 的字符串，已经很勉强了。
const MAX_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundState {
    /// 自定义背景图的绝对路径；`null` 表示用内置的默认背景。
    pub path: Option<String>,
}

fn background_dir() -> AppResult<PathBuf> {
    Ok(config::config_dir()?.join("background"))
}

/// 取现有的自定义背景（`custom.*`）。同一时刻至多有一个。
fn current() -> AppResult<Option<PathBuf>> {
    let dir = background_dir()?;
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        // 目录还不存在 = 从没设过自定义背景，不是错误
        Err(_) => return Ok(None),
    };

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let matches_stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().eq_ignore_ascii_case(STEM))
            .unwrap_or(false);
        if matches_stem && path.is_file() {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// 清掉所有 `custom.*`（换格式时要把旧的删掉，否则 `current()` 可能取到旧的）
fn remove_all_custom() -> AppResult<()> {
    let dir = background_dir()?;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(());
    };

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let matches_stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().eq_ignore_ascii_case(STEM))
            .unwrap_or(false);
        if matches_stem && path.is_file() {
            std::fs::remove_file(&path)
                .map_err(|err| AppError::from_io("删除旧的背景图失败", err))?;
        }
    }
    Ok(())
}

/// 按文件头判断这是不是一张能用的图片。
///
/// 只做"认得出来"这一层：认不出就拒。不做完整解码——那是浏览器的事，
/// 这里要挡的是"改了扩展名的普通文件"这种最常见的情况。
fn looks_like_image(bytes: &[u8]) -> bool {
    if bytes.len() < 12 {
        return false;
    }
    // PNG
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return true;
    }
    // JPEG
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return true;
    }
    // BMP
    if bytes.starts_with(b"BM") {
        return true;
    }
    // GIF
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return true;
    }
    // WebP: "RIFF" .... "WEBP"
    if bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return true;
    }
    false
}

fn extension_of(path: &Path) -> AppResult<String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    if !ALLOWED.contains(&ext.as_str()) {
        return Err(AppError::Config(format!(
            "只支持这些图片格式：{}。这个文件是 .{}",
            ALLOWED.join(" / "),
            if ext.is_empty() {
                "（无扩展名）"
            } else {
                &ext
            }
        )));
    }
    Ok(ext)
}

/// 读取当前自定义背景的状态
pub fn get() -> AppResult<BackgroundState> {
    Ok(BackgroundState {
        path: current()?.map(|p| p.display().to_string()),
    })
}

/// 扩展名 → MIME。只处理 [`ALLOWED`] 里的几种。
fn mime_of(path: &Path) -> AppResult<&'static str> {
    match extension_of(path)?.as_str() {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "webp" => Ok("image/webp"),
        "bmp" => Ok("image/bmp"),
        "gif" => Ok("image/gif"),
        // extension_of 已经挡过一遍，走到这里说明两份清单不同步
        other => Err(AppError::Internal(format!(
            "没有为 .{other} 定义 MIME 类型"
        ))),
    }
}

/// 读出当前自定义背景，编成 **data URL** 交给前端。
///
/// # 为什么不用 asset 协议
///
/// Tauri 正规做法是开 `assetProtocol` 并用 `convertFileSrc`。但它的 scope
/// 只能靠路径变量写，而 **`$APPDATA` 指的是 `%APPDATA%\\<identifier>`
/// （`com.bakaskill.app`），本项目的数据却放在字面量 `%APPDATA%\\BakaSkill` 下**
/// ——两者对不上，写不出一条既准确又可移植的 scope 规则。
///
/// scope 写错的失败形态是**图片静默 403、背景变成一片空白**，正是本项目最忌讳的
/// 那类失败。data URL 没有这层耦合：不依赖任何路径变量、不依赖运行目录，
/// 且能直接用单元测试覆盖。代价是 base64 放大约三分之一、且只在**设了自定义背景时**
/// 才有这笔开销（默认背景是打包进前端的，不走 IPC）。
pub fn image_data_url() -> AppResult<Option<String>> {
    use base64::Engine;

    let Some(path) = current()? else {
        return Ok(None);
    };

    let bytes = std::fs::read(&path).map_err(|err| AppError::from_io("读取背景图失败", err))?;
    let mime = mime_of(&path)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);

    Ok(Some(format!("data:{mime};base64,{encoded}")))
}

/// 把用户选的图片收进应用数据目录，并返回新状态。
pub fn set(source: &str) -> AppResult<BackgroundState> {
    let src = PathBuf::from(source);

    // 不跟随重解析点：拖进来的若是链接，用户想设得到底是它还是它的目标？
    // 猜错会复制进别处的内容（与导入路径同一条约定）。
    let meta = std::fs::symlink_metadata(&src)
        .map_err(|_| AppError::NotFound(format!("文件不存在或无法读取：{source}")))?;
    if crate::platform::link::is_junction(&src) || meta.file_type().is_symlink() {
        return Err(AppError::Config(
            "这是一个链接，请选择实体图片文件".to_string(),
        ));
    }
    if !meta.file_type().is_file() {
        return Err(AppError::Config(
            "请选择一张图片文件，而不是文件夹".to_string(),
        ));
    }
    if meta.len() > MAX_BYTES {
        return Err(AppError::Config(format!(
            "这张图有 {}，超过 {} 的上限——多半是选错了文件",
            human_size(meta.len()),
            human_size(MAX_BYTES)
        )));
    }

    let ext = extension_of(&src)?;

    let head = read_head(&src)?;
    if !looks_like_image(&head) {
        return Err(AppError::Config(format!(
            "这个文件的内容不像是图片（扩展名是 .{ext}，但文件头对不上）。\
             如果它是别的文件改的名，请换一张真的图片。"
        )));
    }

    let dir = background_dir()?;
    std::fs::create_dir_all(&dir).map_err(|err| AppError::from_io("创建背景图目录失败", err))?;

    // 先清旧的再写新的：否则换了格式之后目录里会有两个 custom.*，
    // `current()` 取到哪个就不确定了
    remove_all_custom()?;

    let dest = dir.join(format!("{STEM}.{ext}"));
    std::fs::copy(&src, &dest).map_err(|err| AppError::from_io("复制背景图失败", err))?;

    tracing::info!(dest = %dest.display(), "背景图已更新");

    Ok(BackgroundState {
        path: Some(dest.display().to_string()),
    })
}

/// 恢复默认背景（清掉自定义的那份）
pub fn clear() -> AppResult<BackgroundState> {
    remove_all_custom()?;
    tracing::info!("已清除自定义背景，回到默认背景");
    Ok(BackgroundState { path: None })
}

fn read_head(path: &Path) -> AppResult<Vec<u8>> {
    use std::io::Read;

    let mut file =
        std::fs::File::open(path).map_err(|err| AppError::from_io("打开图片失败", err))?;
    let mut buf = vec![0u8; 16];
    let read = file
        .read(&mut buf)
        .map_err(|err| AppError::from_io("读取图片失败", err))?;
    buf.truncate(read);
    Ok(buf)
}

fn human_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= MB {
        format!("{:.1} MB", bytes as f64 / MB)
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_headers_are_recognized() {
        let png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
        assert!(looks_like_image(&png));

        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(looks_like_image(&jpeg));

        let bmp = [b'B', b'M', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(looks_like_image(&bmp));

        let gif = *b"GIF89a______";
        assert!(looks_like_image(&gif));

        let mut webp = *b"RIFF____WEBP";
        webp[8..12].copy_from_slice(b"WEBP");
        assert!(looks_like_image(&webp));
    }

    /// 改了名的普通文件必须在**设置的时候**就被挡住，
    /// 而不是等浏览器渲染失败、留下一个说不清原因的白板
    #[test]
    fn a_text_file_renamed_to_png_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path().join("伪装.png");
        std::fs::write(&fake, "这其实是一个文本文件，只是改了扩展名".as_bytes()).unwrap();

        let err = set(&fake.display().to_string()).unwrap_err();
        assert!(
            err.to_string().contains("不像是图片"),
            "应当以'内容不像图片'为由拒绝：{err}"
        );
    }

    #[test]
    fn unsupported_extensions_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let psd = tmp.path().join("设计稿.psd");
        std::fs::write(&psd, b"8BPS").unwrap();

        let err = set(&psd.display().to_string()).unwrap_err();
        assert!(err.to_string().contains("只支持"), "{err}");
    }

    #[test]
    fn a_missing_file_is_refused() {
        let err = set("D:\\绝对不存在的目录\\没有这张图.png").unwrap_err();
        assert!(err.to_string().contains("不存在"), "{err}");
    }

    /// 每种允许的格式都要有 MIME 映射，否则前端会拿到一个定义不全的 data URL
    #[test]
    fn every_allowed_extension_has_a_mime_type() {
        for ext in ALLOWED {
            let path = PathBuf::from(format!("x.{ext}"));
            let mime = mime_of(&path).unwrap_or_else(|e| panic!(".{ext} 没有 MIME 映射：{e}"));
            assert!(mime.starts_with("image/"), ".{ext} → {mime}");
        }
    }

    #[test]
    fn a_directory_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let err = set(&tmp.path().display().to_string()).unwrap_err();
        assert!(err.to_string().contains("而不是文件夹"), "{err}");
    }
}
