//! 链接启停服务：把中央库中的 Skill 映射到 / 撤销映射出各 Agent 的技能目录。
//!
//! # 安全约束（与 `platform::link` 同级）
//!
//! 本模块的每个写操作都必须先判定目标当前**是什么**，再决定做什么：
//!
//! | 当前状态 | 启用 | 禁用 |
//! | --- | --- | --- |
//! | 不存在 | 建链接 | 幂等成功（本就是禁用） |
//! | junction → 中央库 | 幂等成功（已启用） | **删链接** |
//! | junction → 中央库外 | 拒绝（不覆盖别人的链接） | **拒绝**（不是我们建的） |
//! | junction 断链 | 重建 | 允许清理 |
//! | 真实目录 | 按冲突策略处理（默认跳过） | **拒绝**（删了就丢数据） |
//! | 符号链接 | 拒绝（本项目不代管） | 拒绝 |
//!
//! 「真实目录」那一行是红线：禁用操作永远不会碰它。

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::AppError;
use crate::library;
use crate::platform::link::{self, LinkKind};

/// 目标位置已被真实目录占用时的处理策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicy {
    /// 跳过并提示用户（默认）。**不静默覆盖用户的数据。**
    #[default]
    Skip,
    /// 把占位目录重命名为 `<name>.bak.<时间戳>` 后再建链接
    BackupAndReplace,
    /// 直接放弃本次操作
    Abort,
}

/// 单次操作实际做了什么
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkAction {
    /// 新建了链接
    Enabled,
    /// 删除了链接
    Disabled,
    /// 目标已是期望状态，无需操作（幂等）
    AlreadyInState,
    /// 因冲突策略或安全约束被跳过
    Skipped,
    /// 执行失败
    Failed,
}

/// 单个「Skill × Agent」的操作结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkResult {
    pub skill_id: String,
    pub agent_id: String,
    pub link_path: String,
    pub target_path: String,
    pub action: LinkAction,
    /// 与 `AppError::kind()` 一致；仅 `action == Failed` 时有值
    pub error_kind: Option<String>,
    pub message: Option<String>,
}

impl LinkResult {
    fn ok(
        skill_id: &str,
        agent_id: &str,
        link_path: &Path,
        target: &Path,
        action: LinkAction,
    ) -> Self {
        Self {
            skill_id: skill_id.to_string(),
            agent_id: agent_id.to_string(),
            link_path: link_path.display().to_string(),
            target_path: target.display().to_string(),
            action,
            error_kind: None,
            message: None,
        }
    }

    fn from_error(skill_id: &str, agent_id: &str, link_path: &Path, err: &AppError) -> Self {
        Self {
            skill_id: skill_id.to_string(),
            agent_id: agent_id.to_string(),
            link_path: link_path.display().to_string(),
            target_path: String::new(),
            action: LinkAction::Failed,
            error_kind: Some(err.kind().to_string()),
            message: Some(err.to_string()),
        }
    }

    fn skipped(
        skill_id: &str,
        agent_id: &str,
        link_path: &Path,
        target: &Path,
        message: impl Into<String>,
    ) -> Self {
        Self {
            message: Some(message.into()),
            ..Self::ok(skill_id, agent_id, link_path, target, LinkAction::Skipped)
        }
    }
}

/// skill 在某个 Agent 中的链接状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkState {
    /// 链接有效且指向中央库
    Valid,
    /// 尚未建立链接
    Absent,
    /// 链接存在但目标不可达
    Dangling,
    /// 位置被真实目录占用
    Conflict,
    /// 是链接但目标不在中央库内
    ForeignLink,
    /// 是符号链接（本项目不代管）
    ForeignSymlink,
}

/// 矩阵中的一个单元格
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkCell {
    pub skill_id: String,
    pub agent_id: String,
    pub state: LinkState,
    pub link_path: String,
    pub target_path: Option<String>,
}

/// 计算某个 Skill 在某个 Agent 中的预期链接位置与目标位置
fn paths_for(
    central_root: &Path,
    agent_skill_dir: &Path,
    skill_dir_name: &str,
) -> (PathBuf, PathBuf) {
    (
        agent_skill_dir.join(skill_dir_name),
        library::skills_dir(central_root).join(skill_dir_name),
    )
}

/// 判断一个链接的目标是否位于中央库内
fn target_in_library(target: &Path, central_root: &Path) -> bool {
    let t = normalize(target);
    let lib = normalize(&library::skills_dir(central_root));
    t.starts_with(&lib)
}

fn normalize(p: &Path) -> String {
    dunce::simplified(p)
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
}

/// 判定单个单元格的状态（只读，不改动文件系统）
pub fn inspect(
    central_root: &Path,
    agent_id: &str,
    agent_skill_dir: &Path,
    skill_id: &str,
    skill_dir_name: &str,
) -> LinkCell {
    let (link_path, target) = paths_for(central_root, agent_skill_dir, skill_dir_name);

    let kind = link::classify(&link_path).unwrap_or(LinkKind::Missing);

    let state = match kind {
        LinkKind::Missing => {
            if !target.is_dir() {
                // 中央库里没有这个 Skill，谈不上启用
                LinkState::Absent
            } else {
                LinkState::Absent
            }
        }
        LinkKind::NotALink => LinkState::Conflict,
        LinkKind::SymlinkDir | LinkKind::SymlinkFile => LinkState::ForeignSymlink,
        LinkKind::Junction => match link::junction_target(&link_path) {
            Ok(Some(actual)) => {
                if !target_in_library(&actual, central_root) {
                    LinkState::ForeignLink
                } else if actual.exists() {
                    LinkState::Valid
                } else {
                    LinkState::Dangling
                }
            }
            // 读不出目标：当作外部链接处理，不代管
            _ => LinkState::ForeignLink,
        },
    };

    LinkCell {
        skill_id: skill_id.to_string(),
        agent_id: agent_id.to_string(),
        state,
        link_path: link_path.display().to_string(),
        target_path: (kind != LinkKind::Missing && kind != LinkKind::NotALink)
            .then(|| target.display().to_string()),
    }
}

/// 启用：在 Agent 目录中建立指向中央库的链接（幂等）
pub fn enable(
    central_root: &Path,
    agent_id: &str,
    agent_skill_dir: &Path,
    skill_id: &str,
    skill_dir_name: &str,
    policy: ConflictPolicy,
) -> LinkResult {
    let (link_path, target) = paths_for(central_root, agent_skill_dir, skill_dir_name);

    if !target.is_dir() {
        return LinkResult::skipped(
            skill_id,
            agent_id,
            &link_path,
            &target,
            format!(
                "中央库中不存在该 Skill：{}。请先把它纳入中央库。",
                target.display()
            ),
        );
    }

    let kind = link::classify(&link_path).unwrap_or(LinkKind::Missing);

    match kind {
        LinkKind::Missing => {}
        LinkKind::Junction => {
            match link::junction_target(&link_path) {
                Ok(Some(actual)) if target_in_library(&actual, central_root) => {
                    if actual.exists() {
                        // 已启用，幂等
                        return LinkResult::ok(
                            skill_id,
                            agent_id,
                            &link_path,
                            &target,
                            LinkAction::AlreadyInState,
                        );
                    }
                    // 断链：清掉重建
                    if let Err(err) = link::delete_junction(&link_path) {
                        return LinkResult::from_error(skill_id, agent_id, &link_path, &err);
                    }
                }
                Ok(Some(actual)) => {
                    return LinkResult::skipped(
                        skill_id,
                        agent_id,
                        &link_path,
                        &target,
                        format!(
                            "该位置已是指向中央库外部的链接（{}），不覆盖。",
                            actual.display()
                        ),
                    );
                }
                _ => {
                    return LinkResult::skipped(
                        skill_id,
                        agent_id,
                        &link_path,
                        &target,
                        "无法读取现有链接的目标，出于安全考虑不覆盖。",
                    );
                }
            }
        }
        LinkKind::SymlinkDir | LinkKind::SymlinkFile => {
            return LinkResult::skipped(
                skill_id,
                agent_id,
                &link_path,
                &target,
                "该位置是符号链接，本项目只管理 junction，不覆盖。",
            );
        }
        LinkKind::NotALink => {
            // 真实目录：按冲突策略处理。**默认不动用户数据。**
            match policy {
                ConflictPolicy::Abort => {
                    return LinkResult::skipped(
                        skill_id,
                        agent_id,
                        &link_path,
                        &target,
                        "该位置已存在同名目录，已按设置放弃本次操作。",
                    );
                }
                ConflictPolicy::Skip => {
                    return LinkResult::skipped(
                        skill_id,
                        agent_id,
                        &link_path,
                        &target,
                        "该位置已存在同名目录（不是链接）。已跳过，未改动该目录。",
                    );
                }
                ConflictPolicy::BackupAndReplace => {
                    let backup = backup_path(&link_path);
                    if let Err(err) = std::fs::rename(&link_path, &backup) {
                        return LinkResult::from_error(
                            skill_id,
                            agent_id,
                            &link_path,
                            &AppError::from_io("备份占位目录失败", err),
                        );
                    }
                    tracing::warn!(
                        from = %link_path.display(),
                        to = %backup.display(),
                        "冲突目录已重命名备份"
                    );
                }
            }
        }
    }

    match link::create_junction(&target, &link_path) {
        Ok(()) => LinkResult::ok(skill_id, agent_id, &link_path, &target, LinkAction::Enabled),
        Err(err) => LinkResult::from_error(skill_id, agent_id, &link_path, &err),
    }
}

/// 禁用：删除指向中央库的链接。
///
/// **只删除链接本体。** 若目标位置不是「指向中央库的 junction」，
/// 一律拒绝——包括真实目录、外部链接、符号链接。
pub fn disable(
    central_root: &Path,
    agent_id: &str,
    agent_skill_dir: &Path,
    skill_id: &str,
    skill_dir_name: &str,
) -> LinkResult {
    let (link_path, target) = paths_for(central_root, agent_skill_dir, skill_dir_name);
    let kind = link::classify(&link_path).unwrap_or(LinkKind::Missing);

    match kind {
        LinkKind::Missing => LinkResult::ok(
            skill_id,
            agent_id,
            &link_path,
            &target,
            LinkAction::AlreadyInState,
        ),
        LinkKind::NotALink => LinkResult::skipped(
            skill_id,
            agent_id,
            &link_path,
            &target,
            "该位置是真实目录而不是链接。删除它会丢失真实数据，已拒绝。",
        ),
        LinkKind::SymlinkDir | LinkKind::SymlinkFile => LinkResult::skipped(
            skill_id,
            agent_id,
            &link_path,
            &target,
            "该位置是符号链接，本项目不代管，已拒绝。",
        ),
        LinkKind::Junction => {
            match link::junction_target(&link_path) {
                Ok(Some(actual)) if target_in_library(&actual, central_root) => {}
                Ok(Some(actual)) => {
                    return LinkResult::skipped(
                        skill_id,
                        agent_id,
                        &link_path,
                        &target,
                        format!(
                            "该链接指向中央库外部（{}），不是 SkillHub 建立的，已拒绝删除。",
                            actual.display()
                        ),
                    );
                }
                // 断链且读不出目标：无法确认是我们建的，拒绝
                _ => {
                    return LinkResult::skipped(
                        skill_id,
                        agent_id,
                        &link_path,
                        &target,
                        "无法确认该链接是否由 SkillHub 建立，已拒绝删除。",
                    );
                }
            }

            match link::delete_junction(&link_path) {
                Ok(()) => LinkResult::ok(
                    skill_id,
                    agent_id,
                    &link_path,
                    &target,
                    LinkAction::Disabled,
                ),
                Err(err) => LinkResult::from_error(skill_id, agent_id, &link_path, &err),
            }
        }
    }
}

/// 纳入中央库的结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptResult {
    pub skill_id: String,
    pub agent_id: String,
    /// 移动前的原位置
    pub source_path: String,
    /// 中央库中的新位置
    pub library_path: String,
    /// 原位置重建的链接
    pub link_path: String,
    /// 是否发生了跨卷复制（跨卷无法原子移动）
    pub copied_across_volumes: bool,
}

/// 把一个位于 Agent 目录中的**真实目录**纳入中央库：
/// 移动到 `<central>/skills/<dir_name>`，再在原位置建立指向它的链接。
///
/// # 为什么需要它
///
/// 用户手上已有的 Skill 通常躺在各个 Agent 目录里。没有这一步，
/// 中央库就只能靠手工拷贝文件来填充。
///
/// # 安全约束
///
/// 1. 只处理**真实目录**。已经是链接、或不是目录的路径一律拒绝。
/// 2. 中央库中已有同名目录时拒绝，不覆盖——由调用方决定改名还是跳过。
/// 3. 跨卷时无法用 `rename` 原子移动，改为「复制 → 校验 → 删除原件」。
///    **校验不通过就中止且不删原件**，宁可留下副本也不能丢数据。
pub fn adopt(
    central_root: &Path,
    agent_id: &str,
    agent_skill_dir: &Path,
    dir_name: &str,
) -> Result<AdoptResult, AppError> {
    let source = agent_skill_dir.join(dir_name);
    let destination = library::skills_dir(central_root).join(dir_name);

    if !source.is_dir() {
        return Err(AppError::NotFound(format!(
            "要纳入的位置不是目录：{}",
            source.display()
        )));
    }
    if link::is_junction(&source) {
        return Err(AppError::NotAJunction(format!(
            "{} 已经是链接，无需重复纳入。",
            source.display()
        )));
    }
    if !has_manifest(&source) {
        return Err(AppError::Config(format!(
            "{} 下没有 SKILL.md，不是有效的 Skill 目录。",
            source.display()
        )));
    }
    if destination.exists() {
        return Err(AppError::Conflict(format!(
            "中央库中已存在同名目录：{}。请先重命名，或改用「替换为链接」。",
            destination.display()
        )));
    }

    std::fs::create_dir_all(library::skills_dir(central_root))
        .map_err(|err| AppError::from_io("创建中央库 skills 目录失败", err))?;

    // 先试原子移动；跨卷时 rename 会失败，退回复制
    let copied_across_volumes = match std::fs::rename(&source, &destination) {
        Ok(()) => false,
        Err(_) => {
            copy_tree(&source, &destination)?;
            // 校验副本可用后，才删除原目录
            if !has_manifest(&destination) {
                // 副本不完整：清掉半成品，保留原件
                let _ = std::fs::remove_dir_all(&destination);
                return Err(AppError::Io(format!(
                    "复制到中央库后校验失败，已中止并保留原目录：{}",
                    source.display()
                )));
            }
            std::fs::remove_dir_all(&source).map_err(|err| {
                AppError::from_io(
                    &format!(
                        "复制成功但无法删除原目录（{}）。中央库中已有一份，请手工处理原目录。",
                        source.display()
                    ),
                    err,
                )
            })?;
            true
        }
    };

    // 在原位置建立链接，让它重新指回中央库
    match link::create_junction(&destination, &source) {
        Ok(()) => {}
        Err(err) => {
            // 链接失败则把数据放回原处，避免用户"东西不见了"
            let rollback = std::fs::rename(&destination, &source);
            return Err(AppError::Io(match rollback {
                Ok(()) => format!("建立链接失败，已把目录移回原处：{err}"),
                Err(rb) => format!(
                    "建立链接失败（{err}），且自动回滚也失败（{rb}）。\
                     内容现在位于 {}，请手工移回。",
                    destination.display()
                ),
            }));
        }
    }

    tracing::info!(
        agent = agent_id,
        dir = dir_name,
        from = %source.display(),
        to = %destination.display(),
        copied_across_volumes,
        "已纳入中央库"
    );

    Ok(AdoptResult {
        skill_id: dir_name.to_string(),
        agent_id: agent_id.to_string(),
        source_path: source.display().to_string(),
        library_path: destination.display().to_string(),
        link_path: source.display().to_string(),
        copied_across_volumes,
    })
}

/// 递归复制目录（含子目录与文件）
fn copy_tree(from: &Path, to: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(to)
        .map_err(|err| AppError::from_io(&format!("创建目标目录失败：{}", to.display()), err))?;

    let entries = std::fs::read_dir(from)
        .map_err(|err| AppError::from_io(&format!("读取源目录失败：{}", from.display()), err))?;

    for entry in entries.filter_map(Result::ok) {
        let src = entry.path();
        let dst = to.join(entry.file_name());

        if src.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|err| {
                AppError::from_io(&format!("复制文件失败：{}", src.display()), err)
            })?;
        }
    }
    Ok(())
}

/// 目录下是否存在 SKILL.md（大小写不敏感）
fn has_manifest(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.filter_map(Result::ok).any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(crate::library::SKILL_MANIFEST)
            })
        })
        .unwrap_or(false)
}

/// 把一批单元格状态写入索引的 `links` 表。
///
/// 为什么需要落盘而不是每次现算：链接状态会因为**外部原因**变化
/// （用户手工删了链接、Agent 升级改了目录、磁盘被拔）。
/// 存下最后一次校验的结果与时间，才能回答"这个状态是什么时候看到的"，
/// 也才能区分「从未校验过」与「校验过且当时正常」。
///
/// 索引是派生物：表损坏或不存在时只记警告，不影响调用方。
pub fn persist_states(root: &Path, cells: &[LinkCell]) -> usize {
    let Ok(mut conn) = crate::index::open(root) else {
        tracing::warn!("无法打开索引，跳过链接状态落盘");
        return 0;
    };
    let Ok(tx) = conn.transaction() else {
        return 0;
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let mut written = 0usize;
    for cell in cells {
        let state = match cell.state {
            LinkState::Valid => "valid",
            LinkState::Absent => "absent",
            LinkState::Dangling => "dangling",
            LinkState::Conflict => "conflict",
            LinkState::ForeignLink => "foreign_link",
            LinkState::ForeignSymlink => "foreign_symlink",
        };

        let ok = tx
            .execute(
                r#"INSERT INTO links (skill_id, agent_id, link_path, target_path, state, checked_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                   ON CONFLICT(skill_id, agent_id) DO UPDATE SET
                     link_path = excluded.link_path,
                     target_path = excluded.target_path,
                     state = excluded.state,
                     checked_at = excluded.checked_at"#,
                rusqlite::params![
                    cell.skill_id,
                    cell.agent_id,
                    cell.link_path,
                    cell.target_path,
                    state,
                    now
                ],
            )
            .is_ok();
        if ok {
            written += 1;
        }
    }

    if tx.commit().is_err() {
        return 0;
    }
    written
}

/// 中央库整体迁移的结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocateResult {
    pub from: String,
    pub to: String,
    /// 数据是否被移动（false 表示保留原位置，只做复制）
    pub moved: bool,
    /// 重写成功的链接
    pub relinked: Vec<String>,
    /// 重写失败的链接及原因
    pub failed: Vec<String>,
    /// 旧数据是否已删除
    pub old_data_removed: bool,
}

/// 迁移中央库到新位置，并重写所有指向它的链接。
///
/// # 执行顺序（**不可调换**）
///
/// 1. 校验新位置可用且不为旧位置本身；
/// 2. **复制**数据到新位置（不用 rename —— 旧数据必须保持可用，
///    直到所有链接都改指成功，否则中途失败会让用户既没有旧库也没有新库）；
/// 3. 重写所有指向旧库的链接，使其指向新库；
/// 4. **只有在全部链接重写成功之后**，才删除旧位置的数据（仅当 `move_data`）。
///
/// 顺序若反了（先删旧数据再重写链接），一旦中途失败，
/// 用户会同时失去数据和链接。
///
/// # 失败时的行为
///
/// 若存在重写失败的链接，**不删除旧数据**并把失败清单返回，
/// 由用户决定是重试还是手工处理。
pub fn relocate(
    old_root: &Path,
    new_root: &Path,
    agent_dirs: &[PathBuf],
    move_data: bool,
) -> Result<RelocateResult, AppError> {
    if normalize(old_root) == normalize(new_root) {
        return Err(AppError::Config("新旧位置相同，无需迁移。".to_string()));
    }
    if !library::skills_dir(old_root).is_dir() {
        return Err(AppError::NotFound(format!(
            "旧位置不是已初始化的中央库：{}",
            old_root.display()
        )));
    }
    if new_root.exists() && library::skills_dir(new_root).exists() {
        return Err(AppError::Conflict(format!(
            "新位置已经是一个中央库：{}。请换一个空文件夹。",
            new_root.display()
        )));
    }

    // 1) 初始化新库骨架
    library::initialize(&new_root.display().to_string())?;

    // 2) 复制全部 Skill（复制而非移动，保证旧库在链接改指完成前仍然完整）
    let old_skills = library::skills_dir(old_root);
    let new_skills = library::skills_dir(new_root);
    for entry in std::fs::read_dir(&old_skills)
        .map_err(|err| AppError::from_io("读取旧中央库失败", err))?
        .filter_map(Result::ok)
    {
        let src = entry.path();
        if !src.is_dir() || link::is_junction(&src) {
            continue;
        }
        let dst = new_skills.join(entry.file_name());
        if dst.exists() {
            continue;
        }
        copy_tree(&src, &dst)?;
    }

    // 3) 重写链接：先删旧链接再建新链接。
    //    此刻新库数据已就位、旧库也仍在，任一步失败都不会丢数据。
    let mut relinked = Vec::new();
    let mut failed = Vec::new();

    for agent_dir in agent_dirs {
        let Ok(links_in_agent) = link::list_links_in(agent_dir) else {
            continue;
        };

        for (link_path, target) in links_in_agent {
            if !target_in_library(&target, old_root) {
                continue; // 外部链接，不代管
            }
            let Some(dir_name) = link_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
            else {
                continue;
            };
            let new_target = new_skills.join(&dir_name);

            if !new_target.is_dir() {
                failed.push(format!(
                    "{}：新中央库中没有对应的 {}",
                    link_path.display(),
                    dir_name
                ));
                continue;
            }

            if let Err(err) = link::delete_junction(&link_path) {
                failed.push(format!("{}：删除旧链接失败（{err}）", link_path.display()));
                continue;
            }
            match link::create_junction(&new_target, &link_path) {
                Ok(()) => relinked.push(link_path.display().to_string()),
                Err(err) => {
                    failed.push(format!("{}：重建链接失败（{err}）", link_path.display()));
                }
            }
        }
    }

    // 4) 只有全部成功才删除旧数据
    let mut old_data_removed = false;
    if move_data && failed.is_empty() {
        // 只删 skills 与 .bakaskill，保留旧根目录本身（用户可能把别的文件放在那儿）
        for sub in [library::SKILLS_DIR, library::META_DIR] {
            let dir = old_root.join(sub);
            if dir.is_dir() {
                std::fs::remove_dir_all(&dir).map_err(|err| {
                    AppError::from_io(&format!("删除旧数据失败：{}", dir.display()), err)
                })?;
            }
        }
        old_data_removed = true;
    }

    tracing::info!(
        from = %old_root.display(),
        to = %new_root.display(),
        relinked = relinked.len(),
        failed = failed.len(),
        old_data_removed,
        "中央库迁移完成"
    );

    Ok(RelocateResult {
        from: old_root.display().to_string(),
        to: new_root.display().to_string(),
        moved: move_data,
        relinked,
        failed,
        old_data_removed,
    })
}

/// 为冲突目录生成备份名：`<name>.bak.<时间戳>`
fn backup_path(link_path: &Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = link_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".to_string());
    link_path.with_file_name(format!("{name}.bak.{stamp}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _tmp: tempfile::TempDir,
        central: PathBuf,
        agent_dir: PathBuf,
    }

    fn setup() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let central = tmp.path().join("library");
        library::initialize(&central.display().to_string()).unwrap();

        let agent_dir = tmp.path().join("agent-skills");
        std::fs::create_dir_all(&agent_dir).unwrap();

        Fixture {
            _tmp: tmp,
            central,
            agent_dir,
        }
    }

    fn add_central_skill(f: &Fixture, name: &str) {
        let dir = library::skills_dir(&f.central).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(library::SKILL_MANIFEST),
            format!("---\nname: {name}\n---\n正文\n"),
        )
        .unwrap();
    }

    // ---- 启用 ----

    #[test]
    fn enable_creates_junction() {
        let f = setup();
        add_central_skill(&f, "pdf-tools");

        let r = enable(
            &f.central,
            "a",
            &f.agent_dir,
            "pdf-tools",
            "pdf-tools",
            ConflictPolicy::Skip,
        );

        assert_eq!(r.action, LinkAction::Enabled, "{:?}", r.message);
        assert!(link::is_junction(&f.agent_dir.join("pdf-tools")));
    }

    #[test]
    fn enable_is_idempotent() {
        let f = setup();
        add_central_skill(&f, "pdf-tools");

        enable(
            &f.central,
            "a",
            &f.agent_dir,
            "pdf-tools",
            "pdf-tools",
            ConflictPolicy::Skip,
        );
        let second = enable(
            &f.central,
            "a",
            &f.agent_dir,
            "pdf-tools",
            "pdf-tools",
            ConflictPolicy::Skip,
        );
        assert_eq!(second.action, LinkAction::AlreadyInState);
    }

    #[test]
    fn enable_skips_when_skill_not_in_library() {
        let f = setup();
        let r = enable(
            &f.central,
            "a",
            &f.agent_dir,
            "ghost",
            "ghost",
            ConflictPolicy::Skip,
        );
        assert_eq!(r.action, LinkAction::Skipped);
        assert!(!f.agent_dir.join("ghost").exists());
    }

    /// 默认策略下绝不覆盖真实目录 —— 这是数据安全红线
    #[test]
    fn enable_does_not_touch_real_directory_by_default() {
        let f = setup();
        add_central_skill(&f, "pdf-tools");

        let occupied = f.agent_dir.join("pdf-tools");
        std::fs::create_dir_all(&occupied).unwrap();
        std::fs::write(occupied.join("user-data.txt"), "用户自己的数据").unwrap();

        let r = enable(
            &f.central,
            "a",
            &f.agent_dir,
            "pdf-tools",
            "pdf-tools",
            ConflictPolicy::Skip,
        );

        assert_eq!(r.action, LinkAction::Skipped);
        assert!(occupied.join("user-data.txt").is_file(), "用户数据被动了");
        assert!(!link::is_junction(&occupied));
    }

    #[test]
    fn backup_and_replace_preserves_user_directory() {
        let f = setup();
        add_central_skill(&f, "pdf-tools");

        let occupied = f.agent_dir.join("pdf-tools");
        std::fs::create_dir_all(&occupied).unwrap();
        std::fs::write(occupied.join("user-data.txt"), "用户自己的数据").unwrap();

        let r = enable(
            &f.central,
            "a",
            &f.agent_dir,
            "pdf-tools",
            "pdf-tools",
            ConflictPolicy::BackupAndReplace,
        );

        assert_eq!(r.action, LinkAction::Enabled, "{:?}", r.message);
        assert!(link::is_junction(&occupied), "应已建为链接");

        // 原内容必须还在某个 .bak 目录里
        let backup = std::fs::read_dir(&f.agent_dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| p.file_name().unwrap().to_string_lossy().contains(".bak."))
            .expect("未生成备份目录");
        assert!(backup.join("user-data.txt").is_file(), "用户数据丢失");
    }

    #[test]
    fn abort_policy_does_nothing() {
        let f = setup();
        add_central_skill(&f, "pdf-tools");
        let occupied = f.agent_dir.join("pdf-tools");
        std::fs::create_dir_all(&occupied).unwrap();

        let r = enable(
            &f.central,
            "a",
            &f.agent_dir,
            "pdf-tools",
            "pdf-tools",
            ConflictPolicy::Abort,
        );
        assert_eq!(r.action, LinkAction::Skipped);
        assert!(!link::is_junction(&occupied));
    }

    #[test]
    fn enable_rebuilds_dangling_link() {
        let f = setup();
        add_central_skill(&f, "pdf-tools");

        // 造一个断链：建好后把中央库移走再移回来
        enable(
            &f.central,
            "a",
            &f.agent_dir,
            "pdf-tools",
            "pdf-tools",
            ConflictPolicy::Skip,
        );
        let moved = f.central.with_file_name("library-moved");
        std::fs::rename(&f.central, &moved).unwrap();

        let link_path = f.agent_dir.join("pdf-tools");
        assert_eq!(link::classify(&link_path).unwrap(), LinkKind::Junction);
        assert!(
            !link::junction_target(&link_path).unwrap().unwrap().exists(),
            "目标应已不可达（构成断链）"
        );

        // 移回来，再启用一次应能恢复正常
        std::fs::rename(&moved, &f.central).unwrap();
        let r = enable(
            &f.central,
            "a",
            &f.agent_dir,
            "pdf-tools",
            "pdf-tools",
            ConflictPolicy::Skip,
        );
        assert!(matches!(
            r.action,
            LinkAction::Enabled | LinkAction::AlreadyInState
        ));
        assert!(link_path.join("SKILL.md").is_file());
    }

    // ---- 禁用 ----

    /// 禁用只删链接，源目录内容分毫不动
    #[test]
    fn disable_removes_link_and_keeps_source_intact() {
        let f = setup();
        add_central_skill(&f, "pdf-tools");
        enable(
            &f.central,
            "a",
            &f.agent_dir,
            "pdf-tools",
            "pdf-tools",
            ConflictPolicy::Skip,
        );

        let r = disable(&f.central, "a", &f.agent_dir, "pdf-tools", "pdf-tools");

        assert_eq!(r.action, LinkAction::Disabled, "{:?}", r.message);
        assert!(!f.agent_dir.join("pdf-tools").exists(), "残留了空目录");

        let source = library::skills_dir(&f.central).join("pdf-tools");
        assert!(source.join("SKILL.md").is_file(), "源目录内容丢失");
    }

    /// **红线**：禁用绝不删真实目录
    #[test]
    fn disable_never_removes_a_real_directory() {
        let f = setup();
        let real = f.agent_dir.join("external-skill");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("important.txt"), "重要数据").unwrap();

        let r = disable(
            &f.central,
            "a",
            &f.agent_dir,
            "external-skill",
            "external-skill",
        );

        assert_eq!(r.action, LinkAction::Skipped);
        assert!(real.is_dir(), "真实目录被删了");
        assert!(real.join("important.txt").is_file());
    }

    /// 指向中央库外部的链接不代管
    #[test]
    fn disable_refuses_foreign_link() {
        let f = setup();
        let outside = f._tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();

        link::create_junction(&outside, &f.agent_dir.join("foreign")).unwrap();

        let r = disable(&f.central, "a", &f.agent_dir, "foreign", "foreign");
        assert_eq!(r.action, LinkAction::Skipped);
        assert!(
            link::is_junction(&f.agent_dir.join("foreign")),
            "外部链接被删了"
        );
    }

    #[test]
    fn disable_is_idempotent_when_absent() {
        let f = setup();
        let r = disable(&f.central, "a", &f.agent_dir, "nothing", "nothing");
        assert_eq!(r.action, LinkAction::AlreadyInState);
    }

    // ---- 状态判定 ----

    #[test]
    fn inspect_reports_states() {
        let f = setup();
        add_central_skill(&f, "pdf-tools");

        // 未建立
        let cell = inspect(&f.central, "a", &f.agent_dir, "pdf-tools", "pdf-tools");
        assert_eq!(cell.state, LinkState::Absent);

        // 建立后
        enable(
            &f.central,
            "a",
            &f.agent_dir,
            "pdf-tools",
            "pdf-tools",
            ConflictPolicy::Skip,
        );
        let cell = inspect(&f.central, "a", &f.agent_dir, "pdf-tools", "pdf-tools");
        assert_eq!(cell.state, LinkState::Valid);
        assert!(cell.target_path.is_some());

        // 冲突（真实目录）
        std::fs::create_dir_all(f.agent_dir.join("occupied")).unwrap();
        let cell = inspect(&f.central, "a", &f.agent_dir, "occupied", "occupied");
        assert_eq!(cell.state, LinkState::Conflict);

        // 断链
        let moved = f.central.with_file_name("moved-away");
        std::fs::rename(&f.central, &moved).unwrap();
        let cell = inspect(&f.central, "a", &f.agent_dir, "pdf-tools", "pdf-tools");
        assert_eq!(cell.state, LinkState::Dangling, "断链未被识别");
        std::fs::rename(&moved, &f.central).unwrap();
    }

    // ---- 纳入中央库 ----

    #[test]
    fn adopt_moves_skill_into_library_and_links_back() {
        let f = setup();

        // Agent 目录里有一个真实的外部 Skill
        let external = f.agent_dir.join("my-skill");
        std::fs::create_dir_all(external.join("assets")).unwrap();
        std::fs::write(
            external.join("SKILL.md"),
            "---\nname: my-skill\n---\n正文\n",
        )
        .unwrap();
        std::fs::write(external.join("assets").join("t.txt"), "资源").unwrap();

        let result = adopt(&f.central, "a", &f.agent_dir, "my-skill").unwrap();

        assert!(result.library_path.contains("skills"));
        assert!(!result.copied_across_volumes, "同卷应走原子移动");

        // 原位置变成链接
        assert!(link::is_junction(&external), "原位置未建链接");

        // 内容完整地出现在中央库
        let in_library = library::skills_dir(&f.central).join("my-skill");
        assert!(in_library.join("SKILL.md").is_file());
        assert!(in_library.join("assets").join("t.txt").is_file());

        // 通过链接仍能读到同一份内容
        assert!(external.join("SKILL.md").is_file());
        assert_eq!(
            std::fs::read_to_string(external.join("assets").join("t.txt")).unwrap(),
            "资源"
        );
    }

    #[test]
    fn adopt_refuses_when_already_a_link() {
        let f = setup();
        add_central_skill(&f, "pdf-tools");
        enable(
            &f.central,
            "a",
            &f.agent_dir,
            "pdf-tools",
            "pdf-tools",
            ConflictPolicy::Skip,
        );

        let err = adopt(&f.central, "a", &f.agent_dir, "pdf-tools").unwrap_err();
        assert!(matches!(err, AppError::NotAJunction(_)));
    }

    #[test]
    fn adopt_refuses_when_name_already_in_library() {
        let f = setup();
        add_central_skill(&f, "dup");

        let external = f.agent_dir.join("dup");
        std::fs::create_dir_all(&external).unwrap();
        std::fs::write(external.join("SKILL.md"), "---\nname: dup\n---\n").unwrap();

        let err = adopt(&f.central, "a", &f.agent_dir, "dup").unwrap_err();
        assert!(matches!(err, AppError::Conflict(_)));
        // 双方都必须完好
        assert!(external.join("SKILL.md").is_file());
        assert!(library::skills_dir(&f.central)
            .join("dup")
            .join("SKILL.md")
            .is_file());
    }

    #[test]
    fn adopt_refuses_directory_without_manifest() {
        let f = setup();
        let not_a_skill = f.agent_dir.join("random-folder");
        std::fs::create_dir_all(&not_a_skill).unwrap();
        std::fs::write(not_a_skill.join("notes.txt"), "x").unwrap();

        let err = adopt(&f.central, "a", &f.agent_dir, "random-folder").unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
        assert!(not_a_skill.is_dir(), "被拒绝的目录不应被动过");
    }

    #[test]
    fn adopt_refuses_missing_path() {
        let f = setup();
        assert!(adopt(&f.central, "a", &f.agent_dir, "nothing").is_err());
    }

    /// 纳入后该 Skill 应当立即处于「已接入」状态
    #[test]
    fn adopted_skill_reports_as_valid_link() {
        let f = setup();
        let external = f.agent_dir.join("adopted");
        std::fs::create_dir_all(&external).unwrap();
        std::fs::write(external.join("SKILL.md"), "---\nname: adopted\n---\n").unwrap();

        adopt(&f.central, "a", &f.agent_dir, "adopted").unwrap();

        let cell = inspect(&f.central, "a", &f.agent_dir, "adopted", "adopted");
        assert_eq!(cell.state, LinkState::Valid);
    }

    #[test]
    fn copy_tree_copies_nested_content() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("from");
        std::fs::create_dir_all(from.join("a").join("b")).unwrap();
        std::fs::write(from.join("top.txt"), "1").unwrap();
        std::fs::write(from.join("a").join("b").join("deep.txt"), "2").unwrap();

        let to = tmp.path().join("to");
        copy_tree(&from, &to).unwrap();

        assert_eq!(std::fs::read_to_string(to.join("top.txt")).unwrap(), "1");
        assert_eq!(
            std::fs::read_to_string(to.join("a").join("b").join("deep.txt")).unwrap(),
            "2"
        );
    }

    /// 单条启停端到端 < 200ms（不含 IPC 传输）。
    ///
    /// 取多次运行的最大值而非平均——关心的是"最坏情况下用户等多久"。
    #[test]
    fn single_toggle_round_trip_under_200ms() {
        let f = setup();
        add_central_skill(&f, "timing");

        let mut worst = std::time::Duration::ZERO;

        for _ in 0..20 {
            let t0 = std::time::Instant::now();
            enable(
                &f.central,
                "a",
                &f.agent_dir,
                "timing",
                "timing",
                ConflictPolicy::Skip,
            );
            worst = worst.max(t0.elapsed());

            let t1 = std::time::Instant::now();
            disable(&f.central, "a", &f.agent_dir, "timing", "timing");
            worst = worst.max(t1.elapsed());
        }

        println!("单条启停最坏耗时: {}µs", worst.as_micros());
        assert!(
            worst.as_millis() < 200,
            "单条启停最坏耗时 {}ms，超出 200ms 目标",
            worst.as_millis()
        );
    }

    // ---- 中央库迁移 ----

    #[test]
    fn relocate_moves_data_and_rewrites_links() {
        let f = setup();
        add_central_skill(&f, "pdf-tools");
        add_central_skill(&f, "code-review");
        enable(
            &f.central,
            "a",
            &f.agent_dir,
            "pdf-tools",
            "pdf-tools",
            ConflictPolicy::Skip,
        );
        enable(
            &f.central,
            "a",
            &f.agent_dir,
            "code-review",
            "code-review",
            ConflictPolicy::Skip,
        );

        let new_root = f._tmp.path().join("library-new");
        let result = relocate(
            &f.central,
            &new_root,
            std::slice::from_ref(&f.agent_dir),
            true,
        )
        .unwrap();

        assert_eq!(result.relinked.len(), 2);
        assert!(result.failed.is_empty(), "{:?}", result.failed);
        assert!(result.old_data_removed);

        // 链接现在指向新库，且可用
        for name in ["pdf-tools", "code-review"] {
            let link_path = f.agent_dir.join(name);
            assert_eq!(link::classify(&link_path).unwrap(), LinkKind::Junction);
            let target = link::junction_target(&link_path).unwrap().unwrap();
            assert!(target.starts_with(&new_root), "链接未指向新库");
            assert!(link_path.join("SKILL.md").is_file(), "通过链接读不到内容");
        }

        // 旧数据已清除，新库内容完整
        assert!(!library::skills_dir(&f.central).join("pdf-tools").exists());
        assert!(library::skills_dir(&new_root)
            .join("pdf-tools")
            .join("SKILL.md")
            .is_file());
    }

    /// 保留原数据模式下不应删除旧内容
    #[test]
    fn relocate_without_move_keeps_old_data() {
        let f = setup();
        add_central_skill(&f, "keep-me");
        enable(
            &f.central,
            "a",
            &f.agent_dir,
            "keep-me",
            "keep-me",
            ConflictPolicy::Skip,
        );

        let new_root = f._tmp.path().join("library-copy");
        let result = relocate(
            &f.central,
            &new_root,
            std::slice::from_ref(&f.agent_dir),
            false,
        )
        .unwrap();

        assert!(!result.old_data_removed);
        assert!(
            library::skills_dir(&f.central)
                .join("keep-me")
                .join("SKILL.md")
                .is_file(),
            "保留模式下旧数据被删了"
        );
        // 链接仍应指向新库（迁移的目的就是改指）
        let target = link::junction_target(&f.agent_dir.join("keep-me"))
            .unwrap()
            .unwrap();
        assert!(target.starts_with(&new_root));
    }

    #[test]
    fn relocate_rejects_same_path() {
        let f = setup();
        let err = relocate(&f.central, &f.central, &[], true).unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
    }

    #[test]
    fn relocate_rejects_uninitialized_source() {
        let f = setup();
        let empty = f._tmp.path().join("not-a-library");
        std::fs::create_dir_all(&empty).unwrap();

        let err = relocate(&empty, &f._tmp.path().join("x"), &[], true).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn relocate_rejects_existing_library_as_target() {
        let f = setup();
        let other = f._tmp.path().join("other-library");
        library::initialize(&other.display().to_string()).unwrap();

        let err = relocate(&f.central, &other, &[], true).unwrap_err();
        assert!(matches!(err, AppError::Conflict(_)));
    }

    /// 外部链接不应被迁移逻辑改动
    #[test]
    fn relocate_leaves_foreign_links_alone() {
        let f = setup();
        add_central_skill(&f, "mine");
        enable(
            &f.central,
            "a",
            &f.agent_dir,
            "mine",
            "mine",
            ConflictPolicy::Skip,
        );

        let outside = f._tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        link::create_junction(&outside, &f.agent_dir.join("foreign")).unwrap();

        let new_root = f._tmp.path().join("library-new2");
        let result = relocate(
            &f.central,
            &new_root,
            std::slice::from_ref(&f.agent_dir),
            true,
        )
        .unwrap();

        assert_eq!(result.relinked.len(), 1, "只应迁移自己的链接");
        let foreign_target = link::junction_target(&f.agent_dir.join("foreign"))
            .unwrap()
            .unwrap();
        assert_eq!(
            normalize(&foreign_target),
            normalize(&outside),
            "外部链接被改动了"
        );
    }

    #[test]
    fn inspect_reports_foreign_link() {
        let f = setup();
        let outside = f._tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        link::create_junction(&outside, &f.agent_dir.join("foreign")).unwrap();

        let cell = inspect(&f.central, "a", &f.agent_dir, "foreign", "foreign");
        assert_eq!(cell.state, LinkState::ForeignLink);
    }
}
