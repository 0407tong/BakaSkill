//! 导入与导出：用户之间的 Skill 文件交换。
//!
//! # 边界守点
//!
//! 这是**用户自己搬运文件**的能力：从文件夹、ZIP、Git 仓库、单个 SKILL.md
//! 拿进来，或者打包成 ZIP 递出去。它**不是**技能市场——没有中心化的索引、
//! 没有在线检索、没有任何"从服务端拉取"的路径。这条边界在本模块的落点就是：
//! 本模块只认用户给出的本地路径与仓库地址，不认识任何平台。
//!
//! # 两段式：先预览，再应用
//!
//! 导入分两步：`preview` 把来源**收拢到一个暂存目录**并列出将被导入的东西
//! （含同名冲突与非法项），`apply` 再按用户逐项的选择落进中央库。
//!
//! 之所以不是"边扫边导"：同名冲突必须让用户先看到再决定，而"导入到一半
//! 停下来问"会留下一半的中间状态。
//!
//! 暂存目录放在系统临时目录下，`apply` 结束后删除；token 是一个十六进制串，
//! 用来防止前端把预览结果与另一次导入的暂存区张冠李戴。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::library;
use crate::skill::SkillDocument;

/// 导入来源
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ImportSource {
    /// 一个文件夹，递归查找其中的 SKILL.md
    Folder { path: String },
    /// 一个 ZIP，解压后按文件夹处理
    Zip { path: String },
    /// 一个 Git 仓库，克隆后按文件夹处理
    Git { url: String },
    /// 单个 SKILL.md，以文件名建目录
    SkillFile { path: String },
}

/// 一个将被导入的 Skill
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCandidate {
    /// 相对暂存根的路径，`apply` 时原样回传
    pub relative_path: String,
    /// 计划落进中央库时用的目录名
    pub dir_name: String,
    pub name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    /// 与谁重名；非空即冲突
    pub conflict: Option<ConflictInfo>,
    /// 冲突的另一方是谁。与 `conflict` 同生共死。
    pub conflict_kind: Option<ConflictKind>,
}

/// 冲突的另一方是谁。
///
/// 之所以要区分：`apply` 时"目标已存在"的处理是一样的，但**用户看到的话不一样**。
/// 说"中央库里有同名的"而实际上只是这次拖进来的两个文件夹重名，会把人指错方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictKind {
    /// 中央库里已经有了同名的 Skill
    Library,
    /// 与**本次导入的另一个条目**同名
    Batch,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictInfo {
    pub dir_name: String,
    pub name: String,
    /// 已有那份的简介，供用户判断"是不是同一个东西"
    pub description: Option<String>,
}

/// 拖入的路径里无法识别、因而整个没参与导入的项。
///
/// 单独列出来而不是静默丢弃：用户拖了 3 样东西、只有 2 样被导入，
/// 界面必须说得出第 3 样为什么没进来。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RejectedPath {
    pub path: String,
    pub reason: String,
}

/// 解析不通过、因而不参与导入的条目
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvalidCandidate {
    pub relative_path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    /// 应用时回传，用来确认操作的是同一次预览
    pub token: String,
    /// 暂存根目录（界面用来告诉用户"实际读的是哪儿"）
    pub staging_root: String,
    pub candidates: Vec<ImportCandidate>,
    pub invalid: Vec<InvalidCandidate>,
    /// 只有「拖拽导入」会产生它：拖进来的路径里无法识别、整个没参与导入的项。
    /// 走「选来源」导入时恒为空。
    pub rejected: Vec<RejectedPath>,
}

/// 用户对某个冲突的选择
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportAction {
    /// 不导入这一项
    Skip,
    /// 导入，但自动改一个不冲突的目录名
    Rename,
    /// 用这一份替换中央库里已有的同名 Skill
    Overwrite,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDecision {
    pub relative_path: String,
    pub action: ImportAction,
}

/// 单个条目的导入结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub relative_path: String,
    pub action: ImportAction,
    /// 落进中央库后的目录名；跳过或失败时为 `None`
    pub dir_name: Option<String>,
    /// 「替换」时，被替换掉的那一份被**移到**了哪里（不是删除）
    pub replaced_to: Option<String>,
    /// 跳过或失败的原因
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub outcomes: Vec<ImportOutcome>,
    pub imported: usize,
    pub indexed: usize,
}

// ===========================================================================
// 暂存区
// ===========================================================================

/// 所有导入暂存区的父目录
fn staging_base() -> AppResult<PathBuf> {
    let temp = std::env::temp_dir();
    Ok(temp.join("bakaskill-import"))
}

/// 新建一个本次导入专属的暂存目录，返回 (token, 目录)
fn new_staging() -> AppResult<(String, PathBuf)> {
    let base = staging_base()?;
    std::fs::create_dir_all(&base).map_err(|err| AppError::from_io("创建导入暂存目录失败", err))?;

    // token 只由十六进制字符构成：它会被拼进路径，也正因为它受限，
    // 前端回传时无法借此跳出暂存区（见 `staging_for`）。
    let token = format!(
        "{:x}{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        std::process::id()
    );

    let root = base.join(&token);
    // 时间戳 + pid 理论上可能撞上（同一纳秒同一进程），撞了就再取一次
    if root.exists() {
        return Err(AppError::Internal(
            "导入暂存目录重名，请重试一次".to_string(),
        ));
    }
    std::fs::create_dir_all(&root).map_err(|err| AppError::from_io("创建导入暂存目录失败", err))?;

    Ok((token, root))
}

/// 由 token 反查暂存目录，并确保它确实在暂存区之内
fn staging_for(token: &str) -> AppResult<PathBuf> {
    if token.is_empty() || !token.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::Config("导入凭据无效".to_string()));
    }
    let root = staging_base()?.join(token);
    if !root.is_dir() {
        return Err(AppError::NotFound(
            "导入暂存目录已不存在——请重新选择来源后再试".to_string(),
        ));
    }
    Ok(root)
}

/// 删除暂存区。失败只记日志：它本来就在系统临时目录里，留着无害。
fn discard_staging(root: &Path) {
    if let Err(err) = std::fs::remove_dir_all(root) {
        tracing::warn!(path = %root.display(), error = %err, "清理导入暂存目录失败（不影响导入结果）");
    }
}

// ===========================================================================
// ZIP 路径穿越防护
// ===========================================================================

/// 判断 ZIP 条目名是否是**安全的相对路径**。
///
/// 恶意 ZIP 靠条目名逃出解压目录，手法就那么几种，逐一挡掉：
///
/// | 手法 | 例子 | 这里怎么挡 |
/// | --- | --- | --- |
/// | 上级目录 | `../../evil` | 任何一段是 `..` 即拒 |
/// | 绝对路径 | `/etc/passwd`、`\Windows\x` | 开头是分隔符即拒 |
/// | 盘符 / UNC | `C:\x`、`\\server\share` | 含 `:` 即拒 |
/// | 空段 | `a//b` | 空段即拒（`a/` 这种目录条目先去掉末尾斜杠） |
///
/// **不做"清洗后继续"**：能清洗掉就说明这个包本身有问题，正确的反应是
/// 整包拒绝并告诉用户哪几个条目越界，而不是替他猜他想装什么。
fn is_safe_zip_entry(name: &str) -> bool {
    if name.is_empty() || name.contains(':') {
        return false;
    }
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/') {
        return false;
    }
    // 目录条目的名字以 `/` 结尾，去掉再逐段检查
    let trimmed = normalized.trim_end_matches('/');
    if trimmed.is_empty() {
        return false;
    }
    trimmed
        .split('/')
        .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

/// 校验一组 ZIP 条目名，把所有越界的挑出来
fn unsafe_zip_entries(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|name| !is_safe_zip_entry(name))
        .cloned()
        .collect()
}

/// 解压 ZIP 到 `dest`。
///
/// **先整体校验、再解压**：不先校验就边解边写，等于先把恶意文件落盘再后悔。
fn extract_zip(archive_path: &Path, dest: &Path) -> AppResult<()> {
    let file = std::fs::File::open(archive_path).map_err(|err| {
        AppError::from_io(&format!("打开 ZIP 失败：{}", archive_path.display()), err)
    })?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|err| AppError::Io(format!("不是有效的 ZIP 文件：{err}")))?;

    let names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();

    let unsafe_names = unsafe_zip_entries(&names);
    if !unsafe_names.is_empty() {
        let shown: Vec<&str> = unsafe_names.iter().take(5).map(String::as_str).collect();
        return Err(AppError::Config(format!(
            "这个 ZIP 里有 {} 个条目会写到解压目录之外，已整体拒绝导入。\n\n\
             越界的条目：{}{}\n\n\
             正常的压缩包不该包含这类路径，请确认来源是否可信。",
            unsafe_names.len(),
            shown.join("、"),
            if unsafe_names.len() > shown.len() {
                " …"
            } else {
                ""
            }
        )));
    }

    std::fs::create_dir_all(dest).map_err(|err| AppError::from_io("创建解压目录失败", err))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|err| AppError::Io(format!("读取 ZIP 条目失败：{err}")))?;
        let target = dest.join(entry.name().replace('\\', "/"));

        if entry.is_dir() {
            std::fs::create_dir_all(&target)
                .map_err(|err| AppError::from_io("创建目录失败", err))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| AppError::from_io("创建目录失败", err))?;
        }
        let mut out = std::fs::File::create(&target)
            .map_err(|err| AppError::from_io(&format!("写入 {} 失败", target.display()), err))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|err| AppError::from_io(&format!("解压 {} 失败", target.display()), err))?;
    }

    Ok(())
}

// ===========================================================================
// 拖拽导入
// ===========================================================================

/// 把一条被拖进来的**真实文件系统路径**归类成导入来源。
///
/// 前端给不出这个判断：它拿到的是路径字符串，而"这是目录还是 ZIP"要问文件系统。
/// 因此分类放在后端做。
///
/// 认不出来的一律**拒绝并说明**，绝不按扩展名猜——猜错的结果是把
/// 一个不相干的文件当成 Skill 导进去，或者更糟：什么都不做还不吭声。
fn classify_path(raw: &str) -> Result<ImportSource, String> {
    let path = Path::new(raw);

    // 用 `symlink_metadata` 而不是 `Path::exists()`：后者**跟随重解析点**，
    // 一个悬空的 junction 会被判成"不存在"，于是错误提示会指向完全错的方向
    // （与 LINKING.md §4.3 同一处坑）。
    let meta = std::fs::symlink_metadata(path)
        .map_err(|_| "路径不存在或无法读取（可能已被移动或删除）".to_string())?;

    // 链接不做"跟随"处理：拖进来的若是链接，用户想导入的到底是它还是它的目标？
    // 猜错会复制进别处的内容，所以直接说清楚，让他拖实体。
    if crate::platform::link::is_junction(path) || meta.file_type().is_symlink() {
        return Err("这是一个链接（junction / 符号链接），请拖实体文件或文件夹".to_string());
    }

    if meta.file_type().is_dir() {
        return Ok(ImportSource::Folder {
            path: raw.to_string(),
        });
    }

    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());

    match extension.as_deref() {
        Some("zip") => Ok(ImportSource::Zip {
            path: raw.to_string(),
        }),
        Some("md") => Ok(ImportSource::SkillFile {
            path: raw.to_string(),
        }),
        _ => Err("只支持文件夹、.zip 与 .md（SKILL.md）".to_string()),
    }
}

/// 拖拽入口：把一批真实路径直接变成预览。
///
/// 与「选来源」那条路的区别只有一处——这里是**一批**路径，且其中可能有
/// 认不出来的。认不出来的进 `rejected` 如实回报，其余照常导入。
pub fn preview_paths(lib: &Path, paths: &[String]) -> AppResult<ImportPreview> {
    let mut sources = Vec::new();
    let mut rejected = Vec::new();

    for raw in paths {
        match classify_path(raw) {
            Ok(source) => sources.push(source),
            Err(reason) => rejected.push(RejectedPath {
                path: raw.clone(),
                reason,
            }),
        }
    }

    if sources.is_empty() {
        return Err(AppError::Config(
            "拖入的内容里没有可导入的 Skill（支持文件夹、.zip 与 .md 文件）".to_string(),
        ));
    }

    tracing::info!(
        total = paths.len(),
        accepted = sources.len(),
        rejected = rejected.len(),
        "拖拽预览：路径分类完成"
    );

    // 单个来源走与「选文件夹导入」**完全相同**的代码路径。
    // 单拖是最常见的情形，不该为多选支持承担回归风险。
    if sources.len() == 1 {
        let mut preview = preview(lib, &sources[0])?;
        preview.rejected = rejected;
        return Ok(preview);
    }

    preview_many(lib, &sources, rejected)
}

/// 多个来源：各自收进**独立子目录**，再统一扫描。
///
/// # 为什么不直接全收进同一个暂存区
///
/// `collect_source` 的 folder / zip 分支走的是 `copy_tree`，而它复制的是源目录的
/// **内容**、不是目录本身。两个同名目录先后收进来会**互相覆盖**——而且覆盖发生在
/// 用户看到预览**之前**，属于静默丢数据，正是本项目最忌讳的一类缺陷。
///
/// 各占一个以序号命名的子目录即可根除：`find_manifest_dirs` 的深度上限是 8，
/// 多这一层不影响扫描（多出来的这一层也不会出现在给用户看的信息里，
/// `relative_path` 只用作 `apply` 的身份标识）。
fn preview_many(
    lib: &Path,
    sources: &[ImportSource],
    rejected: Vec<RejectedPath>,
) -> AppResult<ImportPreview> {
    let (token, staging) = new_staging()?;

    for (index, source) in sources.iter().enumerate() {
        let slot = staging.join(index.to_string());
        if let Err(err) = collect_source(source, &slot) {
            discard_staging(&staging);
            return Err(err);
        }
    }

    let scan = match scan_candidates(lib, &staging) {
        Ok(scan) => scan,
        Err(err) => {
            discard_staging(&staging);
            return Err(err);
        }
    };

    if scan.candidates.is_empty() && scan.invalid.is_empty() {
        discard_staging(&staging);
        return Err(AppError::NotFound(
            "拖入的内容里没有找到任何 SKILL.md".to_string(),
        ));
    }

    Ok(ImportPreview {
        token,
        staging_root: staging.display().to_string(),
        candidates: scan.candidates,
        invalid: scan.invalid,
        rejected,
    })
}

// ===========================================================================
// 预览
// ===========================================================================

/// 准备导入：把来源收拢到暂存区，扫描出候选，并检测同名冲突。
pub fn preview(lib: &Path, source: &ImportSource) -> AppResult<ImportPreview> {
    let (token, staging) = new_staging()?;

    // 任何一步失败都要把暂存区收拾掉，否则系统临时目录里会攒下一堆半成品
    match collect_source(source, &staging) {
        Ok(()) => {}
        Err(err) => {
            discard_staging(&staging);
            return Err(err);
        }
    }

    let candidates = match scan_candidates(lib, &staging) {
        Ok(c) => c,
        Err(err) => {
            discard_staging(&staging);
            return Err(err);
        }
    };

    if candidates.candidates.is_empty() && candidates.invalid.is_empty() {
        discard_staging(&staging);
        return Err(AppError::NotFound(
            "这个来源里没有找到任何 SKILL.md".to_string(),
        ));
    }

    tracing::info!(
        candidates = candidates.candidates.len(),
        invalid = candidates.invalid.len(),
        conflicts = candidates
            .candidates
            .iter()
            .filter(|c| c.conflict.is_some())
            .count(),
        "导入预览已生成"
    );

    Ok(ImportPreview {
        token,
        staging_root: staging.display().to_string(),
        candidates: candidates.candidates,
        invalid: candidates.invalid,
        // 走「选来源」导入没有"拖进来的路径"这回事，恒为空
        rejected: Vec::new(),
    })
}

struct ScanResult {
    candidates: Vec<ImportCandidate>,
    invalid: Vec<InvalidCandidate>,
}

/// 把来源收拢到暂存目录
fn collect_source(source: &ImportSource, staging: &Path) -> AppResult<()> {
    match source {
        ImportSource::Folder { path } => {
            let src = PathBuf::from(path);
            if !src.is_dir() {
                return Err(AppError::NotFound(format!(
                    "文件夹不存在：{}",
                    src.display()
                )));
            }
            copy_tree(&src, staging)
        }
        ImportSource::Zip { path } => {
            let archive = PathBuf::from(path);
            if !archive.is_file() {
                return Err(AppError::NotFound(format!(
                    "ZIP 文件不存在：{}",
                    archive.display()
                )));
            }
            extract_zip(&archive, staging)
        }
        ImportSource::Git { url } => {
            let dest = staging.join("repo");
            crate::git::clone_for_import(url, &dest)
        }
        ImportSource::SkillFile { path } => {
            let file = PathBuf::from(path);
            if !file.is_file() {
                return Err(AppError::NotFound(format!(
                    "文件不存在：{}",
                    file.display()
                )));
            }
            let stem = file
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| AppError::Config("无法从文件名推导出 Skill 目录名".to_string()))?;

            let dir = staging.join(&stem);
            std::fs::create_dir_all(&dir)
                .map_err(|err| AppError::from_io("创建暂存目录失败", err))?;
            std::fs::copy(&file, dir.join(library::SKILL_MANIFEST))
                .map_err(|err| AppError::from_io("复制 SKILL.md 失败", err))?;
            Ok(())
        }
    }
}

/// 递归复制（不跟随链接、跳过杂项）
fn copy_tree(from: &Path, to: &Path) -> AppResult<()> {
    std::fs::create_dir_all(to).map_err(|err| AppError::from_io("创建目标目录失败", err))?;

    for entry in std::fs::read_dir(from)
        .map_err(|err| AppError::from_io("读取源目录失败", err))?
        .filter_map(Result::ok)
    {
        let src = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = src.is_dir();

        if crate::git::is_junk(&name, is_dir) || crate::platform::link::is_junction(&src) {
            continue;
        }
        // `.git` 目录对导入没有意义，克隆来的仓库尤其大
        if is_dir && name == ".git" {
            continue;
        }

        let dst = to.join(&name);
        if is_dir {
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)
                .map_err(|err| AppError::from_io(&format!("复制 {} 失败", src.display()), err))?;
        }
    }
    Ok(())
}

/// 在暂存区里递归找出所有含 SKILL.md 的目录
fn scan_candidates(lib: &Path, staging: &Path) -> AppResult<ScanResult> {
    let existing = existing_skills(lib)?;

    let mut dirs = Vec::new();
    find_manifest_dirs(staging, staging, &mut dirs, 0)?;
    dirs.sort();

    let mut candidates = Vec::new();
    let mut invalid = Vec::new();

    for (relative, manifest) in dirs {
        let raw = match std::fs::read_to_string(&manifest) {
            Ok(r) => r,
            Err(err) => {
                invalid.push(InvalidCandidate {
                    relative_path: relative,
                    reason: format!("读取 SKILL.md 失败：{err}"),
                });
                continue;
            }
        };
        let doc = match SkillDocument::parse(&raw) {
            Ok(d) => d,
            Err(err) => {
                invalid.push(InvalidCandidate {
                    relative_path: relative,
                    reason: format!("frontmatter 解析失败：{err}"),
                });
                continue;
            }
        };

        let dir_name = manifest
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "skill".to_string());

        let conflict = existing
            .get(&dir_name.to_lowercase())
            .map(|e| ConflictInfo {
                dir_name: e.dir_name.clone(),
                name: e.name.clone(),
                description: e.description.clone(),
            });

        candidates.push(ImportCandidate {
            relative_path: relative,
            dir_name,
            name: doc.name().unwrap_or("（未命名）").to_string(),
            description: doc.description().map(str::to_string),
            tags: doc.tags(),
            conflict_kind: conflict.as_ref().map(|_| ConflictKind::Library),
            conflict,
        });
    }

    flag_batch_duplicates(&mut candidates);

    Ok(ScanResult {
        candidates,
        invalid,
    })
}

/// 标出**同一批次内部**的重名。
///
/// 为什么必须标：`apply_one` 是见到"目标已存在"才处理冲突的，而同批次的两个
/// 同名条目里，第一个会先落盘、第二个才发现目标已存在——在默认动作（跳过）
/// 下它就被**静默跳过**了。预览若不说，用户看到的是
/// "明明拖进来两个，怎么只进来一个"。
///
/// 与**中央库**同名的优先保留：那是更强的信号，也是用户更需要先看到的信息。
fn flag_batch_duplicates(candidates: &mut [ImportCandidate]) {
    let mut seen: std::collections::HashMap<String, (String, Option<String>)> =
        std::collections::HashMap::new();

    for candidate in candidates.iter_mut() {
        let key = candidate.dir_name.to_lowercase();
        match seen.get(&key) {
            Some((name, description)) => {
                if candidate.conflict.is_none() {
                    candidate.conflict = Some(ConflictInfo {
                        dir_name: candidate.dir_name.clone(),
                        name: name.clone(),
                        description: description.clone(),
                    });
                    candidate.conflict_kind = Some(ConflictKind::Batch);
                }
            }
            None => {
                seen.insert(key, (candidate.name.clone(), candidate.description.clone()));
            }
        }
    }
}

/// 中央库里已有的一个 Skill（只留判冲突要用的几项）
struct ExistingSkill {
    dir_name: String,
    name: String,
    description: Option<String>,
}

/// 中央库现有 Skill：目录名（小写）→ 它的信息
fn existing_skills(lib: &Path) -> AppResult<std::collections::HashMap<String, ExistingSkill>> {
    let mut map = std::collections::HashMap::new();
    for record in crate::index::list_all(lib).unwrap_or_default() {
        let dir_name = record
            .dir_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .to_string();
        map.insert(
            dir_name.to_lowercase(),
            ExistingSkill {
                dir_name,
                name: record.name,
                description: record.description,
            },
        );
    }
    Ok(map)
}

/// 递归找"含 SKILL.md 的目录"，**找到就不再往下钻**（避免把 skill 内部的子目录也当成 Skill）
fn find_manifest_dirs(
    current: &Path,
    staging: &Path,
    out: &mut Vec<(String, PathBuf)>,
    depth: usize,
) -> AppResult<()> {
    // 深度上限：导入的是别人的目录树，递归无上限等于把栈交给对方
    const MAX_DEPTH: usize = 8;
    if depth > MAX_DEPTH {
        return Ok(());
    }

    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    let mut children = Vec::new();
    let mut manifest = None;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            if crate::platform::link::is_junction(&path) {
                continue;
            }
            children.push(path);
        } else if path
            .file_name()
            .map(|n| {
                n.to_string_lossy()
                    .eq_ignore_ascii_case(library::SKILL_MANIFEST)
            })
            .unwrap_or(false)
        {
            manifest = Some(path);
        }
    }

    if let Some(manifest) = manifest {
        let relative = current
            .strip_prefix(staging)
            .unwrap_or(current)
            .to_string_lossy()
            .replace('\\', "/");
        let relative = if relative.is_empty() {
            ".".to_string()
        } else {
            relative
        };
        out.push((relative, manifest));
        return Ok(());
    }

    for child in children {
        find_manifest_dirs(&child, staging, out, depth + 1)?;
    }
    Ok(())
}

// ===========================================================================
// 应用导入
// ===========================================================================

/// 按用户的选择把暂存区里的内容落进中央库。
pub fn apply(lib: &Path, token: &str, decisions: &[ImportDecision]) -> AppResult<ImportReport> {
    let staging = staging_for(token)?;
    let skills_root = library::skills_dir(lib);
    std::fs::create_dir_all(&skills_root)
        .map_err(|err| AppError::from_io("创建中央库 skills 目录失败", err))?;

    let mut outcomes = Vec::new();
    let mut imported = 0usize;

    for decision in decisions {
        match apply_one(&staging, &skills_root, decision) {
            Ok(outcome) => {
                if outcome.dir_name.is_some() {
                    imported += 1;
                }
                outcomes.push(outcome);
            }
            Err(err) => outcomes.push(ImportOutcome {
                relative_path: decision.relative_path.clone(),
                action: decision.action,
                dir_name: None,
                replaced_to: None,
                reason: Some(err.to_string()),
            }),
        }
    }

    // 索引必须跟上，否则导进来的 Skill 在界面上看不见
    let indexed = if imported == 0 {
        crate::index::count(lib).unwrap_or(0)
    } else {
        crate::index::rebuild(lib)?.skill_count
    };

    // 暂存区用完即弃。放在最后：中途出错时留着它，用户重试还能用同一个 token。
    discard_staging(&staging);

    tracing::info!(imported, total = decisions.len(), "导入完成");

    Ok(ImportReport {
        outcomes,
        imported,
        indexed,
    })
}

fn apply_one(
    staging: &Path,
    skills_root: &Path,
    decision: &ImportDecision,
) -> AppResult<ImportOutcome> {
    let source = resolve_in_staging(staging, &decision.relative_path)?;

    let dir_name = source
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| AppError::Config("无法确定 Skill 的目录名".to_string()))?;

    if decision.action == ImportAction::Skip {
        return Ok(ImportOutcome {
            relative_path: decision.relative_path.clone(),
            action: ImportAction::Skip,
            dir_name: None,
            replaced_to: None,
            reason: Some("按你的选择跳过".to_string()),
        });
    }

    let target = skills_root.join(&dir_name);
    let mut replaced_to = None;

    if target.exists() {
        match decision.action {
            ImportAction::Rename => {
                // 自动找一个不冲突的名字，结果如实回报给用户
                let unique = unique_dir_name(skills_root, &dir_name);
                return finish_import(&source, &skills_root.join(&unique), decision, unique, None);
            }
            ImportAction::Overwrite => {
                // **移走、不删除**：与"清理陈旧 .git"、"标签改写"一致的处理方式。
                // 用户选择"替换"是想让新的生效，不是想把旧的就地销毁——
                // 万一选错了，东西还在。
                let aside = unique_dir_name(
                    skills_root,
                    &format!(
                        "{}.replaced-{}",
                        dir_name,
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0)
                    ),
                );
                let aside_path = skills_root.join(&aside);
                std::fs::rename(&target, &aside_path).map_err(|err| {
                    AppError::from_io(&format!("把已有的 {} 移到一旁失败", target.display()), err)
                })?;
                replaced_to = Some(aside_path.display().to_string());
            }
            ImportAction::Skip => unreachable!("已在上面提前返回"),
        }
    }

    finish_import(&source, &target, decision, dir_name, replaced_to)
}

fn finish_import(
    source: &Path,
    target: &Path,
    decision: &ImportDecision,
    dir_name: String,
    replaced_to: Option<String>,
) -> AppResult<ImportOutcome> {
    copy_tree(source, target)?;
    Ok(ImportOutcome {
        relative_path: decision.relative_path.clone(),
        action: decision.action,
        dir_name: Some(dir_name),
        replaced_to,
        reason: None,
    })
}

/// 把预览里给出的相对路径解析成暂存区里的真实路径，并确认它没跑出去。
///
/// 前端回传的字符串不可信——即便它来自我们自己的预览，中间也可能被改过。
/// 这里复用 ZIP 那套路径检查，规则只有一处。
fn resolve_in_staging(staging: &Path, relative: &str) -> AppResult<PathBuf> {
    if relative == "." {
        return Ok(staging.to_path_buf());
    }
    if !is_safe_zip_entry(relative) {
        return Err(AppError::Config(format!("非法的导入路径：{relative}")));
    }
    let resolved = staging.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !resolved.is_dir() {
        return Err(AppError::NotFound(format!(
            "暂存区里没有这个目录：{relative}"
        )));
    }
    Ok(resolved)
}

/// 在 `parent` 下找一个与 `wanted` 不冲突的目录名
fn unique_dir_name(parent: &Path, wanted: &str) -> String {
    if !parent.join(wanted).exists() {
        return wanted.to_string();
    }
    for suffix in 2..1000 {
        let candidate = format!("{wanted}-{suffix}");
        if !parent.join(&candidate).exists() {
            return candidate;
        }
    }
    format!("{wanted}-{}", std::process::id())
}

// ===========================================================================
// 导出
// ===========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    /// 打成 ZIP
    Zip,
    /// 直接写成目录结构
    Folder,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub dest: String,
    pub skill_count: usize,
    pub file_count: usize,
    /// 没有导出成功的 Skill 及原因，**不静默忽略**
    pub failed: Vec<String>,
}

/// 导出清单文件的内容
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportManifest<'a> {
    /// 导出它的 SkillHub 版本
    app_version: &'a str,
    exported_at: i64,
    skill_count: usize,
    skills: Vec<ManifestSkill>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestSkill {
    dir_name: String,
    name: String,
    description: Option<String>,
    tags: Vec<String>,
}

/// 导出选中的 Skill。
///
/// 目录布局统一为 `skills/<目录名>/…`，与中央库同构——这样导出再导入
/// （无论哪种格式）都能原样还原，`manifest.json` 放在最外层。
pub fn export(
    lib: &Path,
    dir_names: &[String],
    dest: &str,
    format: ExportFormat,
    include_manifest: bool,
) -> AppResult<ExportReport> {
    if dir_names.is_empty() {
        return Err(AppError::Config("没有选中任何 Skill".to_string()));
    }
    let skills_root = library::skills_dir(lib);
    let records = crate::index::list_all(lib)?;

    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut failed = Vec::new();
    let mut manifest_skills = Vec::new();

    for dir_name in dir_names {
        // 目录名来自前端，先确认它确实是中央库里的一个 Skill 目录，
        // 免得 `..` 之类把别处的东西打进包里
        if dir_name.is_empty()
            || dir_name.contains("..")
            || dir_name.contains('/')
            || dir_name.contains('\\')
        {
            failed.push(format!("{dir_name}：目录名不合法"));
            continue;
        }
        let dir = skills_root.join(dir_name);
        if !dir.is_dir() {
            failed.push(format!("{dir_name}：中央库里没有这个目录"));
            continue;
        }

        let record = records.iter().find(|r| {
            r.dir_path
                .rsplit(['/', '\\'])
                .next()
                .map(|n| n == dir_name)
                .unwrap_or(false)
        });
        manifest_skills.push(ManifestSkill {
            dir_name: dir_name.clone(),
            name: record
                .map(|r| r.name.clone())
                .unwrap_or_else(|| dir_name.clone()),
            description: record.and_then(|r| r.description.clone()),
            tags: record.map(|r| r.tags.clone()).unwrap_or_default(),
        });

        collect_files(&dir, &format!("skills/{dir_name}"), &mut files)?;
    }

    if files.is_empty() {
        // 把逐项的原因说出来：只报"没有文件"会让用户对着一个明明存在的
        // Skill 目录发愣
        return Err(AppError::NotFound(if failed.is_empty() {
            "选中的 Skill 里没有任何文件可导出".to_string()
        } else {
            format!(
                "选中的 Skill 都不可导出：
{}",
                failed.join(
                    "
"
                )
            )
        }));
    }

    let manifest = ExportManifest {
        app_version: env!("CARGO_PKG_VERSION"),
        exported_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
        skill_count: manifest_skills.len(),
        skills: manifest_skills,
    };
    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|err| AppError::Internal(format!("生成 manifest.json 失败：{err}")))?;

    let file_count = match format {
        ExportFormat::Zip => write_zip(dest, &files, include_manifest.then_some(&manifest_json))?,
        ExportFormat::Folder => {
            write_folder(dest, &files, include_manifest.then_some(&manifest_json))?
        }
    };

    tracing::info!(
        dest = %dest,
        skills = manifest.skill_count,
        files = file_count,
        "导出完成"
    );

    Ok(ExportReport {
        dest: dest.to_string(),
        skill_count: manifest.skill_count,
        file_count,
        failed,
    })
}

/// 收集一个 Skill 目录下的全部文件，返回 (包内相对路径, 源路径)
///
/// 跳过杂项文件，与同步那边的规则一致——导出是给人分享的，
/// 里面掺着 `.DS_Store` 只会让人困惑。
fn collect_files(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> AppResult<()> {
    for entry in std::fs::read_dir(dir)
        .map_err(|err| AppError::from_io("读取 Skill 目录失败", err))?
        .filter_map(Result::ok)
    {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = path.is_dir();

        if crate::git::is_junk(&name, is_dir) || crate::platform::link::is_junction(&path) {
            continue;
        }

        let relative = format!("{prefix}/{name}");
        if is_dir {
            collect_files(&path, &relative, out)?;
        } else {
            out.push((relative, path));
        }
    }
    Ok(())
}

fn write_zip(
    dest: &str,
    files: &[(String, PathBuf)],
    manifest: Option<&String>,
) -> AppResult<usize> {
    let file = std::fs::File::create(dest)
        .map_err(|err| AppError::from_io(&format!("创建 {dest} 失败"), err))?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut count = 0usize;
    if let Some(json) = manifest {
        zip.start_file("manifest.json", options)
            .map_err(|err| AppError::Io(format!("写入 manifest.json 失败：{err}")))?;
        use std::io::Write;
        zip.write_all(json.as_bytes())
            .map_err(|err| AppError::Io(format!("写入 manifest.json 失败：{err}")))?;
        count += 1;
    }

    for (name, path) in files {
        zip.start_file(name.as_str(), options)
            .map_err(|err| AppError::Io(format!("写入 {name} 失败：{err}")))?;
        let bytes = std::fs::read(path)
            .map_err(|err| AppError::from_io(&format!("读取 {} 失败", path.display()), err))?;
        use std::io::Write;
        zip.write_all(&bytes)
            .map_err(|err| AppError::Io(format!("写入 {name} 失败：{err}")))?;
        count += 1;
    }

    zip.finish()
        .map_err(|err| AppError::Io(format!("完成 ZIP 写入失败：{err}")))?;
    Ok(count)
}

fn write_folder(
    dest: &str,
    files: &[(String, PathBuf)],
    manifest: Option<&String>,
) -> AppResult<usize> {
    let root = PathBuf::from(dest);
    std::fs::create_dir_all(&root).map_err(|err| AppError::from_io("创建导出目录失败", err))?;

    let mut count = 0usize;
    if let Some(json) = manifest {
        std::fs::write(root.join("manifest.json"), json.as_bytes())
            .map_err(|err| AppError::from_io("写入 manifest.json 失败", err))?;
        count += 1;
    }

    for (name, path) in files {
        let target = root.join(name.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| AppError::from_io("创建导出子目录失败", err))?;
        }
        std::fs::copy(path, &target)
            .map_err(|err| AppError::from_io(&format!("复制到 {} 失败", target.display()), err))?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    // =======================================================================
    // ZIP 路径穿越防护
    // =======================================================================

    #[test]
    fn safe_zip_entries_are_accepted() {
        for name in [
            "SKILL.md",
            "skills/foo/SKILL.md",
            "skills/foo/assets/a.bin",
            "a/b/c/d.txt",
            "dir/",
            "中文目录/SKILL.md",
            "./SKILL.md",
        ] {
            // "./SKILL.md" 里的 "." 段被拒是有意的：正常压缩包不会这么写
            if name == "./SKILL.md" {
                assert!(!is_safe_zip_entry(name), "{name} 不该被接受");
            } else {
                assert!(is_safe_zip_entry(name), "{name} 应当被接受");
            }
        }
    }

    #[test]
    fn traversal_zip_entries_are_rejected() {
        for name in [
            "../evil.txt",
            "../../evil.txt",
            "a/../../evil.txt",
            "a/b/../../../evil.txt",
            "..\\evil.txt",
            "a\\..\\..\\evil.txt",
            "/etc/passwd",
            "\\Windows\\System32\\evil.dll",
            "C:\\Windows\\evil.dll",
            "c:/windows/evil.dll",
            "//server/share/evil.txt",
            "a//b.txt",
            "",
            "..",
            "a/..",
        ] {
            assert!(
                !is_safe_zip_entry(name),
                "{name:?} 会写到解压目录之外，必须被拒"
            );
        }
    }

    /// 构造一个真含 `../` 条目的恶意 ZIP，导入必须被整体拒绝，
    /// 且目标位置上不留任何文件。
    #[test]
    fn malicious_zip_is_rejected_without_writing_anything() {
        let tmp = tempfile::tempdir().unwrap();
        let archive_path = tmp.path().join("evil.zip");

        // 直接构造 ZIP：一个正常条目 + 一个试图跳出解压目录的条目
        {
            let file = std::fs::File::create(&archive_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
            use std::io::Write;
            zip.start_file("skills/good/SKILL.md", options).unwrap();
            zip.write_all(b"---\nname: good\n---\n").unwrap();
            zip.start_file("../../escaped.txt", options).unwrap();
            zip.write_all(b"pwned").unwrap();
            zip.finish().unwrap();
        }

        let escape_target = tmp.path().join("escaped.txt");
        let extract_to = tmp.path().join("out");

        let err = extract_zip(&archive_path, &extract_to).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("解压目录之外"), "报错没说清原因：{text}");
        assert!(text.contains("escaped.txt"), "报错没指出越界条目：{text}");

        // 关键断言：什么都没落盘
        assert!(!escape_target.exists(), "文件逃出了解压目录");
        assert!(
            !extract_to.join("skills/good/SKILL.md").exists(),
            "整体拒绝的意思是连正常条目也不解压"
        );
    }

    // =======================================================================
    // 预览与导入
    // =======================================================================

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("library");
        library::initialize(&root.display().to_string()).unwrap();
        (dir, root)
    }

    fn write_skill(dir: &Path, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(library::SKILL_MANIFEST), body).unwrap();
    }

    fn preview_source(lib: &Path, source: &ImportSource) -> AppResult<ImportPreview> {
        preview(lib, source)
    }

    fn preview_folder_result(lib: &Path, folder: &Path) -> AppResult<ImportPreview> {
        preview_source(
            lib,
            &ImportSource::Folder {
                path: folder.display().to_string(),
            },
        )
    }

    fn preview_folder(lib: &Path, folder: &Path) -> ImportPreview {
        preview_source(
            lib,
            &ImportSource::Folder {
                path: folder.display().to_string(),
            },
        )
        .unwrap()
    }

    #[test]
    fn preview_finds_nested_skills_and_skips_non_skills() {
        let (tmp, lib) = setup();
        let source = tmp.path().join("incoming");
        write_skill(&source.join("a"), "---\nname: 甲\n---\n正文\n");
        write_skill(&source.join("nested/deep/b"), "---\nname: 乙\n---\n正文\n");
        // 没有 SKILL.md 的目录不算 Skill
        std::fs::create_dir_all(source.join("not-a-skill")).unwrap();
        std::fs::write(source.join("not-a-skill/readme.md"), "x").unwrap();

        let preview = preview_folder(&lib, &source);

        let names: Vec<&str> = preview.candidates.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(preview.candidates.len(), 2, "找到的候选不对：{names:?}");
        assert!(names.contains(&"甲"));
        assert!(names.contains(&"乙"));
        assert!(preview.invalid.is_empty());
    }

    #[test]
    fn preview_reports_conflicts_against_the_library() {
        let (tmp, lib) = setup();
        write_skill(&lib.join("skills/同名"), "---\nname: 库里那份\n---\n正文\n");
        crate::index::rebuild(&lib).unwrap();

        let source = tmp.path().join("incoming");
        write_skill(&source.join("同名"), "---\nname: 外来那份\n---\n正文\n");
        write_skill(&source.join("新的"), "---\nname: 新的\n---\n正文\n");

        let preview = preview_folder(&lib, &source);
        let conflicted = preview
            .candidates
            .iter()
            .find(|c| c.dir_name == "同名")
            .unwrap();

        let conflict = conflicted.conflict.as_ref().expect("没有识别出同名冲突");
        assert_eq!(conflict.name, "库里那份");
        assert!(preview
            .candidates
            .iter()
            .find(|c| c.dir_name == "新的")
            .unwrap()
            .conflict
            .is_none());
    }

    #[test]
    fn preview_reports_invalid_frontmatter_separately() {
        let (tmp, lib) = setup();
        let source = tmp.path().join("incoming");
        write_skill(&source.join("good"), "---\nname: 好的\n---\n正文\n");
        // YAML 语法错误
        write_skill(&source.join("bad"), "---\nname: [未闭合\n---\n正文\n");

        let preview = preview_folder(&lib, &source);

        assert_eq!(preview.candidates.len(), 1);
        assert_eq!(preview.invalid.len(), 1);
        assert!(
            preview.invalid[0].reason.contains("解析失败"),
            "{:?}",
            preview.invalid
        );
    }

    #[test]
    fn preview_of_an_empty_source_is_an_error() {
        let (tmp, lib) = setup();
        let source = tmp.path().join("empty");
        std::fs::create_dir_all(&source).unwrap();

        let result = preview_folder_result(&lib, &source);
        assert!(result.is_err(), "空来源应当报错而不是给一个空预览");
    }

    #[test]
    fn import_adds_new_skills_and_refreshes_the_index() {
        let (tmp, lib) = setup();
        let source = tmp.path().join("incoming");
        write_skill(
            &source.join("甲"),
            "---\nname: 甲\ndescription: 说明\n---\n正文\n",
        );

        let preview = preview_folder(&lib, &source);
        let report = apply(
            &lib,
            &preview.token,
            &[ImportDecision {
                relative_path: preview.candidates[0].relative_path.clone(),
                action: ImportAction::Overwrite,
            }],
        )
        .unwrap();

        assert_eq!(report.imported, 1);
        assert_eq!(report.indexed, 1);
        assert!(lib.join("skills/甲/SKILL.md").is_file());
    }

    #[test]
    fn import_skip_leaves_the_library_untouched() {
        let (tmp, lib) = setup();
        let source = tmp.path().join("incoming");
        write_skill(&source.join("新"), "---\nname: 新\n---\n正文\n");

        let preview = preview_folder(&lib, &source);
        let report = apply(
            &lib,
            &preview.token,
            &[ImportDecision {
                relative_path: preview.candidates[0].relative_path.clone(),
                action: ImportAction::Skip,
            }],
        )
        .unwrap();

        assert_eq!(report.imported, 0);
        assert!(report.outcomes[0].dir_name.is_none());
        assert!(!lib.join("skills/新").exists());
    }

    /// 「改名」要给出一个不冲突的名字，并如实回报
    #[test]
    fn import_rename_picks_a_free_name() {
        let (tmp, lib) = setup();
        write_skill(&lib.join("skills/同名"), "---\nname: 老的\n---\n正文\n");
        crate::index::rebuild(&lib).unwrap();

        let source = tmp.path().join("incoming");
        write_skill(&source.join("同名"), "---\nname: 新的\n---\n正文\n");

        let preview = preview_folder(&lib, &source);
        let report = apply(
            &lib,
            &preview.token,
            &[ImportDecision {
                relative_path: preview.candidates[0].relative_path.clone(),
                action: ImportAction::Rename,
            }],
        )
        .unwrap();

        assert_eq!(report.outcomes[0].dir_name.as_deref(), Some("同名-2"));
        assert!(lib.join("skills/同名-2/SKILL.md").is_file());
        // 原来那份原样保留
        let original = std::fs::read_to_string(lib.join("skills/同名/SKILL.md")).unwrap();
        assert!(original.contains("老的"));
    }

    /// 「替换」必须**只移不删**：旧的那份要还在，只是挪到一旁
    #[test]
    fn import_overwrite_moves_the_old_one_aside_instead_of_deleting() {
        let (tmp, lib) = setup();
        write_skill(&lib.join("skills/同名"), "---\nname: 老的\n---\n旧正文\n");
        crate::index::rebuild(&lib).unwrap();

        let source = tmp.path().join("incoming");
        write_skill(&source.join("同名"), "---\nname: 新的\n---\n新正文\n");

        let preview = preview_folder(&lib, &source);
        let report = apply(
            &lib,
            &preview.token,
            &[ImportDecision {
                relative_path: preview.candidates[0].relative_path.clone(),
                action: ImportAction::Overwrite,
            }],
        )
        .unwrap();

        let outcome = &report.outcomes[0];
        let replaced_to = outcome.replaced_to.as_ref().expect("没有回报旧目录的去向");
        assert!(
            std::path::Path::new(replaced_to).join("SKILL.md").is_file(),
            "旧的那份被删掉了，而不是移走"
        );

        let now = std::fs::read_to_string(lib.join("skills/同名/SKILL.md")).unwrap();
        assert!(now.contains("新正文"), "替换后的内容不对：{now}");
    }

    /// 前端回传的路径不可信：越界的 relative_path 必须被拒
    #[test]
    fn import_rejects_a_relative_path_that_escapes_the_staging_area() {
        let (_tmp, lib) = setup();
        let (token, staging) = new_staging().unwrap();
        write_skill(&staging.join("ok"), "---\nname: ok\n---\n正文\n");

        let report = apply(
            &lib,
            &token,
            &[ImportDecision {
                relative_path: "../../something".to_string(),
                action: ImportAction::Overwrite,
            }],
        )
        .unwrap();

        assert_eq!(report.imported, 0);
        assert!(
            report.outcomes[0]
                .reason
                .as_deref()
                .unwrap_or("")
                .contains("非法"),
            "越界路径没有被拒：{:?}",
            report.outcomes[0]
        );
    }

    #[test]
    fn import_rejects_a_bogus_token() {
        let (_tmp, lib) = setup();
        assert!(apply(&lib, "", &[]).is_err());
        assert!(apply(&lib, "../../etc", &[]).is_err());
        assert!(
            apply(&lib, "deadbeef", &[]).is_err(),
            "不存在的 token 应当报错"
        );
    }

    /// 单个 SKILL.md：以文件名建目录
    #[test]
    fn single_file_source_uses_the_file_name_as_the_directory() {
        let (tmp, lib) = setup();
        let file = tmp.path().join("我的技能.md");
        std::fs::write(&file, "---\nname: 我的技能\n---\n正文\n").unwrap();

        let preview = preview(
            &lib,
            &ImportSource::SkillFile {
                path: file.display().to_string(),
            },
        )
        .unwrap();

        assert_eq!(preview.candidates.len(), 1);
        assert_eq!(preview.candidates[0].dir_name, "我的技能");

        let report = apply(
            &lib,
            &preview.token,
            &[ImportDecision {
                relative_path: preview.candidates[0].relative_path.clone(),
                action: ImportAction::Overwrite,
            }],
        )
        .unwrap();
        assert_eq!(report.imported, 1);
        assert!(lib.join("skills/我的技能/SKILL.md").is_file());
    }

    // =======================================================================
    // 导出
    // =======================================================================

    #[test]
    fn export_folder_writes_skills_and_manifest() {
        let (tmp, lib) = setup();
        write_skill(
            &lib.join("skills/甲"),
            "---\nname: 甲\ntags:\n  - 标签\n---\n正文\n",
        );
        std::fs::create_dir_all(lib.join("skills/甲/assets")).unwrap();
        std::fs::write(lib.join("skills/甲/assets/a.bin"), "bin").unwrap();
        crate::index::rebuild(&lib).unwrap();

        let dest = tmp.path().join("out");
        let report = export(
            &lib,
            &["甲".to_string()],
            &dest.display().to_string(),
            ExportFormat::Folder,
            true,
        )
        .unwrap();

        assert_eq!(report.skill_count, 1);
        assert!(dest.join("skills/甲/SKILL.md").is_file());
        assert!(dest.join("skills/甲/assets/a.bin").is_file());

        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dest.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["skillCount"], 1);
        assert_eq!(manifest["skills"][0]["name"], "甲");
        assert!(manifest["appVersion"].is_string());
    }

    #[test]
    fn export_without_manifest_omits_it() {
        let (tmp, lib) = setup();
        write_skill(&lib.join("skills/甲"), "---\nname: 甲\n---\n正文\n");
        crate::index::rebuild(&lib).unwrap();

        let dest = tmp.path().join("plain");
        export(
            &lib,
            &["甲".to_string()],
            &dest.display().to_string(),
            ExportFormat::Folder,
            false,
        )
        .unwrap();

        assert!(dest.join("skills/甲/SKILL.md").is_file());
        assert!(!dest.join("manifest.json").exists(), "不该生成 manifest");
    }

    #[test]
    fn export_zip_contains_the_same_layout() {
        let (tmp, lib) = setup();
        write_skill(&lib.join("skills/甲"), "---\nname: 甲\n---\n正文\n");
        crate::index::rebuild(&lib).unwrap();

        let dest = tmp.path().join("out.zip");
        export(
            &lib,
            &["甲".to_string()],
            &dest.display().to_string(),
            ExportFormat::Zip,
            true,
        )
        .unwrap();

        let file = std::fs::File::open(&dest).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();

        assert!(names.contains(&"manifest.json".to_string()));
        assert!(
            names.contains(&"skills/甲/SKILL.md".to_string()),
            "{names:?}"
        );
    }

    /// 导出 → 清空中央库 → 再导入 → 内容完整还原。
    #[test]
    fn export_then_import_restores_everything() {
        let (tmp, lib) = setup();
        write_skill(
            &lib.join("skills/甲"),
            "---\nname: 甲\ndescription: 说明\ntags:\n  - 标签\n---\n正文一\n",
        );
        std::fs::create_dir_all(lib.join("skills/甲/assets")).unwrap();
        std::fs::write(lib.join("skills/甲/assets/a.bin"), "binary-content").unwrap();
        write_skill(&lib.join("skills/乙"), "---\nname: 乙\n---\n正文二\n");
        crate::index::rebuild(&lib).unwrap();

        let dest = tmp.path().join("backup.zip");
        export(
            &lib,
            &["甲".to_string(), "乙".to_string()],
            &dest.display().to_string(),
            ExportFormat::Zip,
            true,
        )
        .unwrap();

        // 清空中央库，模拟"另一台机器"
        std::fs::remove_dir_all(library::skills_dir(&lib)).unwrap();
        std::fs::create_dir_all(library::skills_dir(&lib)).unwrap();
        crate::index::rebuild(&lib).unwrap();

        let preview = preview(
            &lib,
            &ImportSource::Zip {
                path: dest.display().to_string(),
            },
        )
        .unwrap();
        assert_eq!(preview.candidates.len(), 2, "导出的包里应当有两个 Skill");

        let decisions: Vec<ImportDecision> = preview
            .candidates
            .iter()
            .map(|c| ImportDecision {
                relative_path: c.relative_path.clone(),
                action: ImportAction::Overwrite,
            })
            .collect();
        let report = apply(&lib, &preview.token, &decisions).unwrap();

        assert_eq!(report.imported, 2);
        assert_eq!(report.indexed, 2);

        let restored = std::fs::read_to_string(lib.join("skills/甲/SKILL.md")).unwrap();
        assert!(restored.contains("正文一"));
        assert!(
            restored.contains("标签"),
            "frontmatter 没有还原：{restored}"
        );
        assert_eq!(
            std::fs::read_to_string(lib.join("skills/甲/assets/a.bin")).unwrap(),
            "binary-content",
            "附件没有还原"
        );
        assert!(lib.join("skills/乙/SKILL.md").is_file());
    }

    #[test]
    fn export_rejects_suspicious_directory_names() {
        let (tmp, lib) = setup();
        crate::index::rebuild(&lib).unwrap();

        let dest = tmp.path().join("out");
        let err = export(
            &lib,
            &["../escape".to_string()],
            &dest.display().to_string(),
            ExportFormat::Folder,
            false,
        )
        .unwrap_err();

        // 只有这一项且它不合法 → 最终以"没有可导出的文件"收场，但绝不能真去读 ../escape
        assert!(err.to_string().contains("没有任何文件") || err.to_string().contains("不合法"));
        assert!(!dest.exists());
    }

    // =======================================================================
    // 拖拽导入
    // =======================================================================

    #[test]
    fn classify_recognizes_folders_zips_and_markdown() {
        let tmp = tempfile::tempdir().unwrap();

        let folder = tmp.path().join("a-folder");
        std::fs::create_dir_all(&folder).unwrap();
        assert!(matches!(
            classify_path(&folder.display().to_string()),
            Ok(ImportSource::Folder { .. })
        ));

        let zip = tmp.path().join("pack.zip");
        std::fs::write(&zip, b"PK\x03\x04").unwrap();
        assert!(matches!(
            classify_path(&zip.display().to_string()),
            Ok(ImportSource::Zip { .. })
        ));

        let md = tmp.path().join("SKILL.md");
        std::fs::write(&md, b"---\nname: x\n---\n").unwrap();
        assert!(matches!(
            classify_path(&md.display().to_string()),
            Ok(ImportSource::SkillFile { .. })
        ));

        // 大小写不敏感：用户在资源管理器里看到的是 .ZIP
        let upper = tmp.path().join("PACK.ZIP");
        std::fs::write(&upper, b"PK\x03\x04").unwrap();
        assert!(matches!(
            classify_path(&upper.display().to_string()),
            Ok(ImportSource::Zip { .. })
        ));
    }

    #[test]
    fn classify_rejects_what_it_cannot_recognize() {
        let tmp = tempfile::tempdir().unwrap();

        // 认不出的扩展名：绝不按内容猜
        let txt = tmp.path().join("notes.txt");
        std::fs::write(&txt, b"hello").unwrap();
        assert!(classify_path(&txt.display().to_string()).is_err());

        // 没有扩展名
        let bare = tmp.path().join("README");
        std::fs::write(&bare, b"hi").unwrap();
        assert!(classify_path(&bare.display().to_string()).is_err());

        // 路径根本不存在
        let missing = tmp.path().join("not-there");
        let reason = classify_path(&missing.display().to_string()).unwrap_err();
        assert!(reason.contains("不存在"), "{reason}");
    }

    /// 一批里混着认不出的东西时，认得出的照常导入，
    /// 认不出的如实回报——绝不静默丢掉。
    #[test]
    fn preview_paths_reports_rejected_items_without_dropping_the_rest() {
        let (tmp, lib) = setup();

        let good = tmp.path().join("good");
        write_skill(&good.join("甲"), "---\nname: 甲\n---\n正文\n");

        let junk = tmp.path().join("notes.txt");
        std::fs::write(&junk, b"not a skill").unwrap();

        let missing = tmp.path().join("gone");

        let preview = preview_paths(
            &lib,
            &[
                good.display().to_string(),
                junk.display().to_string(),
                missing.display().to_string(),
            ],
        )
        .unwrap();

        // 认得的那份照常进候选
        assert_eq!(preview.candidates.len(), 1);
        assert_eq!(preview.candidates[0].dir_name, "甲");

        // 认不出的两项都在 rejected 里，且带原因
        assert_eq!(preview.rejected.len(), 2);
        assert!(preview.rejected[0].reason.contains("只支持"));
        assert!(preview.rejected[1].reason.contains("不存在"));
    }

    #[test]
    fn preview_paths_errors_when_nothing_is_recognizable() {
        let (tmp, lib) = setup();
        let junk = tmp.path().join("a.txt");
        std::fs::write(&junk, b"x").unwrap();

        let err = preview_paths(&lib, &[junk.display().to_string()]).unwrap_err();
        assert!(err.to_string().contains("没有可导入"), "{err}");
    }

    /// 单个来源必须走与「选文件夹导入」完全相同的路径。
    #[test]
    fn preview_paths_with_one_source_matches_the_normal_flow() {
        let (tmp, lib) = setup();
        let source = tmp.path().join("incoming");
        write_skill(&source.join("甲"), "---\nname: 甲\n---\n正文\n");

        let dragged = preview_paths(&lib, &[source.display().to_string()]).unwrap();
        let normal = preview_folder(&lib, &source);

        assert_eq!(dragged.candidates.len(), normal.candidates.len());
        assert_eq!(
            dragged.candidates[0].dir_name,
            normal.candidates[0].dir_name
        );
        // relativePath 不该被加上任何"多来源"的包装
        assert_eq!(
            dragged.candidates[0].relative_path,
            normal.candidates[0].relative_path
        );
        assert!(dragged.rejected.is_empty());
    }

    /// 一次拖入两个各自含同名 Skill 的文件夹。
    ///
    /// 若各来源不隔离，`copy_tree` 会把后一个的内容覆盖到前一个上——
    /// 覆盖发生在用户看到预览**之前**，是静默丢数据。
    /// 这里断言两份**都还在**（内容各自可辨），且都被标成"同批次重名"。
    #[test]
    fn multi_source_keeps_same_named_skills_apart_and_flags_the_clash() {
        let (tmp, lib) = setup();

        let first = tmp.path().join("first");
        write_skill(&first.join("同名"), "---\nname: 甲\n---\n正文甲\n");

        let second = tmp.path().join("second");
        write_skill(&second.join("同名"), "---\nname: 乙\n---\n正文乙\n");

        let preview = preview_paths(
            &lib,
            &[first.display().to_string(), second.display().to_string()],
        )
        .unwrap();

        assert_eq!(preview.candidates.len(), 2, "两份都必须还在");

        // 内容没有被覆盖：两份的 name 各自是自己那份
        let mut names: Vec<&str> = preview.candidates.iter().map(|c| c.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["乙", "甲"]);

        // 两者的 relativePath 必须不同，否则 decisions 会张冠李戴
        assert_ne!(
            preview.candidates[0].relative_path,
            preview.candidates[1].relative_path
        );

        // 只标**后一个**：先到的那个会成为落盘者，后到的才发现"目标已存在"。
        // 两个都标反而不对——用户全选"跳过"就一个都进不来了。
        let flagged: Vec<&ImportCandidate> = preview
            .candidates
            .iter()
            .filter(|c| c.conflict.is_some())
            .collect();
        assert_eq!(flagged.len(), 1, "只应标出后到的那个：{flagged:?}");
        assert_eq!(flagged[0].conflict_kind, Some(ConflictKind::Batch));
        assert_eq!(
            flagged[0].name, "乙",
            "被标出的应当是后到的那个（第二次拖入的那份）"
        );
    }

    /// 与中央库同名的，优先级高于"同批次重名"——那是用户更需要先看到的信息。
    #[test]
    fn library_conflict_wins_over_batch_conflict() {
        let (tmp, lib) = setup();
        write_skill(&lib.join("skills/同名"), "---\nname: 库里那份\n---\n正文\n");
        crate::index::rebuild(&lib).unwrap();

        let first = tmp.path().join("first");
        write_skill(&first.join("同名"), "---\nname: 甲\n---\n正文甲\n");
        let second = tmp.path().join("second");
        write_skill(&second.join("同名"), "---\nname: 乙\n---\n正文乙\n");

        let preview = preview_paths(
            &lib,
            &[first.display().to_string(), second.display().to_string()],
        )
        .unwrap();

        for candidate in &preview.candidates {
            assert_eq!(
                candidate.conflict_kind,
                Some(ConflictKind::Library),
                "与中央库同名应当盖过批次内的重名"
            );
            assert_eq!(
                candidate.conflict.as_ref().unwrap().name,
                "库里那份",
                "冲突信息应当指向中央库里那一份"
            );
        }
    }
}
