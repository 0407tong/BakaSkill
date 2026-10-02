//! 卸载：把 Skill 从中央库移除。
//!
//! # 与「禁用」的区别
//!
//! 禁用只摘链接，中央库纹丝不动；**卸载是把目录搬离中央库**，并连带清掉它的所有链接。
//! 用户的原话把这条界线划得很清楚——"全部启用就是全部接入，全部禁用就是取消接入"，
//! 而卸载是另一件事。
//!
//! # 为什么是"移到回收站"而不是直接删
//!
//! 「全部卸载」是**批量动作**，一次误点可能清掉整个库。项目里其他所有破坏性操作
//! （导入时替换、清理陈旧 `.git`）走的都是"移到一旁、不就地销毁"，卸载没有理由更激进。
//!
//! 这个选择还带来一个额外好处：整个目录**一次搬走**，不需要我们自己递归遍历，
//! 于是 `platform/link.rs` 那条红线（"模块外禁止对可能为链接的路径调用
//! `remove_dir_all`"）在本模块**根本不适用**——没有递归删除这回事。
//!
//! # 顺序：先摘链接，后搬目录
//!
//! 反过来的话，目录搬走了、链接还在，那些链接就全成了悬空链接（指向不存在的目标），
//! 用户会在 Agent 目录里看到一堆坏条目，还会以为是卸载没做干净。
//! 与迁移向导里"先重写链接再删旧数据"是同一条原则。
//!
//! 进一步：**只要有一条链接摘不掉，这一项就不搬目录**。摘不掉多半是 Agent 正锁着
//! 那个目录，此时搬走目录等于亲手制造悬空链接。

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::agents::{self, AgentDetection, AgentStatus};
use crate::config;
use crate::error::{AppError, AppResult};
use crate::index;
use crate::library;
use crate::links::{self, LinkState};
use crate::platform::link;

/// 单项卸载的结局
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UninstallAction {
    /// 已搬离中央库
    Removed,
    /// 没动它（例如它根本不在中央库里）
    Skipped,
    /// 尝试了但没成
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallOutcome {
    pub dir_name: String,
    pub action: UninstallAction,
    /// 中央库里的绝对路径
    pub path: String,
    /// 已摘掉的链接（绝对路径）
    pub removed_links: Vec<String>,
    /// 摘链接失败的项，形如「Claude Code：<原因>」
    pub failed_links: Vec<String>,
    /// 目录最终去了哪：回收站，或回落目录的绝对路径
    pub moved_to: Option<String>,
    /// Removed 时是"为什么回落"，Skipped/Failed 时是原因
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallReport {
    pub outcomes: Vec<UninstallOutcome>,
}

/// 回收站送不进时的落脚处（中央库内，仍在用户手里）
const FALLBACK_DIR: &str = "removed";

/// 校验目录名是**单个正常目录段**。
///
/// 这是路径穿越的第一道闸门。`dir_name` 由前端回传，虽然来自我们自己的扫描结果，
/// 中间仍可能被改过——而这里一旦被绕过，删的就是中央库外面的东西。
fn validate_segment(dir_name: &str) -> AppResult<()> {
    let bad = dir_name.is_empty()
        || dir_name == "."
        || dir_name == ".."
        || dir_name.contains(['/', '\\'])
        || dir_name.contains(':');

    if bad {
        return Err(AppError::Config(format!(
            "不是合法的 Skill 目录名：{dir_name:?}"
        )));
    }
    Ok(())
}

/// 定位中央库里的这个 Skill，并确认它确实落在 `<中央库>/skills/` 之内。
fn resolve_skill_dir(root: &Path, dir_name: &str) -> AppResult<PathBuf> {
    validate_segment(dir_name)?;

    let skills_root = library::skills_dir(root);
    let candidate = skills_root.join(dir_name);

    // 第二道闸门：目录名已限定为单段，此外再按规范化后的真实路径核一次包含关系。
    // 只在它确实存在时才能规范化，不存在的情况由调用方按"不在中央库"处理。
    if candidate.exists() {
        let real = dunce::canonicalize(&candidate).map_err(|err| {
            AppError::from_io(&format!("无法解析路径：{}", candidate.display()), err)
        })?;
        let real_root = dunce::canonicalize(&skills_root).unwrap_or_else(|_| skills_root.clone());
        if !real.starts_with(&real_root) {
            return Err(AppError::Config(format!(
                "路径不在中央库内，已拒绝：{}",
                real.display()
            )));
        }
    }

    Ok(candidate)
}

/// 本机所有"已经能接 Skill"的 Agent
fn active_agents() -> AppResult<Vec<AgentDetection>> {
    let cfg = config::load()?;
    Ok(agents::registry()?
        .iter()
        .map(|descriptor| {
            let override_dir = cfg
                .agents
                .iter()
                .find(|a| a.id == descriptor.id)
                .and_then(|a| a.skill_dir.as_deref());
            agents::detect(descriptor, override_dir)
        })
        .filter(|a| a.status == AgentStatus::Detected && a.skill_dir.is_some())
        .collect())
}

/// 摘掉这个 Skill 在所有 Agent 里的链接。
///
/// 返回 (已摘掉的, 摘失败的)。**只碰指向中央库的链接**：外部链接（指向别处）
/// 是用户自己的东西，不在我们的处置范围内。
fn unlink_everywhere(
    root: &Path,
    dir_name: &str,
    agents: &[AgentDetection],
) -> (Vec<String>, Vec<String>) {
    let mut removed = Vec::new();
    let mut failed = Vec::new();

    for agent in agents {
        let Some(skill_dir) = agent.skill_dir.as_deref() else {
            continue;
        };

        // 复用矩阵那套判定，不另写一份"这个格子是不是我们的链接"
        let cell = links::inspect(root, &agent.id, Path::new(skill_dir), dir_name, dir_name);

        let ours = matches!(cell.state, LinkState::Valid | LinkState::Dangling);
        if !ours {
            continue;
        }

        match link::delete_junction(Path::new(&cell.link_path)) {
            Ok(()) => removed.push(cell.link_path),
            Err(err) => failed.push(format!("{}：{}", agent.display_name, err)),
        }
    }

    (removed, failed)
}

/// 把目录搬去回收站；送不进就移进中央库内的回落目录。
///
/// **绝不退化成硬删。** 回收站送不进去（该卷没有回收站、被策略禁用、超出回收站容量）
/// 时，落在 `<中央库>/.bakaskill/removed/` 里，仍在用户手里，并由调用方如实报告位置。
fn move_out(root: &Path, path: &Path, dir_name: &str) -> AppResult<(String, Option<String>)> {
    // `trash` 内部有一处 `CoCreateInstance(...).unwrap()`。在这条破坏性路径上，
    // 一个 panic 会让"链接已摘掉、目录却还留在库里"变成一句崩溃报告，
    // 所以这里连 panic 一起接住，让它走同一条回落。
    let trashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| trash::delete(path)));

    match trashed {
        Ok(Ok(())) => Ok(("回收站".to_string(), None)),
        Ok(Err(err)) => fallback(root, path, dir_name, &format!("送进回收站失败（{err}）")),
        Err(_) => fallback(root, path, dir_name, "调用回收站时发生异常"),
    }
}

fn fallback(
    root: &Path,
    path: &Path,
    dir_name: &str,
    why: &str,
) -> AppResult<(String, Option<String>)> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let dir = library::meta_dir(root).join(FALLBACK_DIR);
    std::fs::create_dir_all(&dir).map_err(|err| AppError::from_io("创建回落目录失败", err))?;

    let target = dir.join(format!("{dir_name}-{stamp}"));
    std::fs::rename(path, &target).map_err(|err| {
        AppError::from_io(
            &format!(
                "{why}，且移入回落目录也失败。原始内容仍在：{}",
                path.display()
            ),
            err,
        )
    })?;

    Ok((
        target.display().to_string(),
        Some(format!(
            "{why}，已改放到中央库内的 removed/ 目录（没有删除）"
        )),
    ))
}

/// 卸载一个 Skill。
fn uninstall_one(root: &Path, dir_name: &str, agents: &[AgentDetection]) -> UninstallOutcome {
    let mut outcome = UninstallOutcome {
        dir_name: dir_name.to_string(),
        action: UninstallAction::Failed,
        path: String::new(),
        removed_links: Vec::new(),
        failed_links: Vec::new(),
        moved_to: None,
        reason: None,
    };

    let path = match resolve_skill_dir(root, dir_name) {
        Ok(p) => p,
        Err(err) => {
            outcome.action = UninstallAction::Skipped;
            outcome.reason = Some(err.to_string());
            return outcome;
        }
    };
    outcome.path = path.display().to_string();

    // 判存在**不跟随重解析点**：一个悬空的 junction 用 `exists()` 会判成不存在，
    // 于是我们会把"这里有个链接"误报成"这里什么都没有"。
    let Ok(meta) = std::fs::symlink_metadata(&path) else {
        outcome.action = UninstallAction::Skipped;
        outcome.reason = Some("中央库里没有这个 Skill".to_string());
        return outcome;
    };

    // 正常情况下它必须是一个**真实目录**。若它本身是链接，删它等于删别处的内容。
    if link::is_junction(&path) || meta.file_type().is_symlink() {
        outcome.action = UninstallAction::Skipped;
        outcome.reason = Some(
            "这个条目本身是链接而非真实目录。卸载它会波及链接指向的地方，已拒绝。".to_string(),
        );
        return outcome;
    }

    if !meta.file_type().is_dir() {
        outcome.action = UninstallAction::Skipped;
        outcome.reason = Some("这不是一个目录".to_string());
        return outcome;
    }

    // 先摘链接。有一条摘不掉就整个中止——此时目录还在，状态是干净的。
    let (removed, failed) = unlink_everywhere(root, dir_name, agents);
    outcome.removed_links = removed;
    outcome.failed_links = failed.clone();

    if !failed.is_empty() {
        outcome.action = UninstallAction::Failed;
        outcome.reason = Some(format!(
            "有 {} 个链接没能摘掉，因此没有搬动目录（否则会留下悬空链接）。\
             多半是该 Agent 正开着——关掉它再试。",
            failed.len()
        ));
        return outcome;
    }

    match move_out(root, &path, dir_name) {
        Ok((moved_to, note)) => {
            outcome.action = UninstallAction::Removed;
            outcome.moved_to = Some(moved_to);
            outcome.reason = note;
        }
        Err(err) => {
            outcome.action = UninstallAction::Failed;
            outcome.reason = Some(err.to_string());
        }
    }

    outcome
}

/// 卸载若干个 Skill（逐项独立结果，部分失败不回滚已成功项）。
pub fn uninstall(root: &Path, dir_names: &[String]) -> AppResult<UninstallReport> {
    if !root.is_dir() {
        return Err(AppError::NotFound(format!(
            "中央库不存在：{}",
            root.display()
        )));
    }

    let agents = active_agents()?;
    let mut outcomes = Vec::new();
    let mut removed_any = false;

    for dir_name in dir_names {
        let outcome = uninstall_one(root, dir_name, &agents);
        if outcome.action == UninstallAction::Removed {
            removed_any = true;
        }

        tracing::info!(
            dir_name = %dir_name,
            action = ?outcome.action,
            removed_links = outcome.removed_links.len(),
            moved_to = ?outcome.moved_to,
            "卸载"
        );

        outcomes.push(outcome);
    }

    // 内容变了，索引必须跟上，否则列表里还留着已经不在的东西
    if removed_any {
        if let Err(err) = index::rebuild(root) {
            tracing::warn!(error = %err, "卸载后重建索引失败");
        }
    }

    Ok(UninstallReport { outcomes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("library");
        library::initialize(&root.display().to_string()).unwrap();
        (dir, root)
    }

    fn write_skill(root: &Path, name: &str, body: &str) -> PathBuf {
        let dir = library::skills_dir(root).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(library::SKILL_MANIFEST), body).unwrap();
        dir
    }

    /// 恶意的目录名必须在碰任何文件之前就被拒绝
    #[test]
    fn hostile_directory_names_are_refused_before_touching_anything() {
        let (_tmp, root) = setup();
        let innocent = write_skill(&root, "无辜", "---\nname: 无辜\n---\n");

        for bad in [
            "..",
            ".",
            "",
            "../外面",
            "..\\外面",
            "a/b",
            "a\\b",
            "C:\\Windows",
            "C:外面",
        ] {
            let report = uninstall(&root, &[bad.to_string()]).unwrap();
            assert_eq!(
                report.outcomes[0].action,
                UninstallAction::Skipped,
                "{bad:?} 应当被拒绝"
            );
        }

        // 旁边那个无辜的 Skill 必须完好
        assert!(innocent.join(library::SKILL_MANIFEST).is_file());
    }

    #[test]
    fn uninstalling_something_not_in_the_library_is_skipped_with_a_reason() {
        let (_tmp, root) = setup();
        let report = uninstall(&root, &["根本没有这个".to_string()]).unwrap();

        assert_eq!(report.outcomes[0].action, UninstallAction::Skipped);
        let reason = report.outcomes[0].reason.as_deref().unwrap_or("");
        assert!(reason.contains("中央库里没有"), "{reason}");
    }

    /// 目录必须真的离开中央库，且**没有从磁盘上消失**
    /// （走回收站意味着它应当还能被找回来；测试里至少断言它不在原处、
    /// 又在回落目录或回收站里留下了痕迹——此处直接断言原处已空）
    #[test]
    fn uninstalling_removes_the_directory_from_the_library() {
        let (_tmp, root) = setup();
        let skill = write_skill(&root, "要卸载的", "---\nname: 要卸载的\n---\n正文\n");

        let report = uninstall(&root, &["要卸载的".to_string()]).unwrap();

        assert_eq!(report.outcomes[0].action, UninstallAction::Removed);
        assert!(!skill.exists(), "目录应当已离开中央库");
        assert!(report.outcomes[0].moved_to.is_some(), "必须报告它去了哪");
    }

    fn fake_agent(skill_dir: &Path) -> AgentDetection {
        AgentDetection {
            id: "fake".to_string(),
            display_name: "假 Agent".to_string(),
            icon: None,
            status: AgentStatus::Detected,
            skill_dir: Some(skill_dir.display().to_string()),
            detected_by: vec![],
            is_user_override: true,
            notes: None,
            docs_url: None,
        }
    }

    /// 中央库里有 Skill、某个 Agent 里是**指向它的链接**，
    /// 卸载后两边都要干净——链接消失、目录离开中央库。
    #[test]
    fn uninstalling_also_removes_the_links_pointing_at_it() {
        let (tmp, root) = setup();
        let skill = write_skill(&root, "接了三处", "---\nname: 接了三处\n---\n正文\n");

        let agent_dir = tmp.path().join("agent-skills");
        std::fs::create_dir_all(&agent_dir).unwrap();
        let link_path = agent_dir.join("接了三处");
        link::create_junction(&skill, &link_path).unwrap();
        assert!(link::is_junction(&link_path));

        let agent = fake_agent(&agent_dir);
        let outcome = uninstall_one(&root, "接了三处", std::slice::from_ref(&agent));

        assert_eq!(outcome.action, UninstallAction::Removed, "{outcome:?}");
        assert_eq!(outcome.removed_links.len(), 1, "应当摘掉那一条链接");
        assert!(!skill.exists(), "目录应当已离开中央库");
        // 判存在不能跟随重解析点：悬空链接用 exists() 会说"不存在"
        assert!(
            std::fs::symlink_metadata(&link_path).is_err(),
            "链接本体应当已消失"
        );
    }

    /// Agent 目录里是**真实目录**（不是我们建的链接）时，一律不动它。
    /// 那份可能是用户自己放的，也可能是 Agent 自己的东西。
    #[test]
    fn a_real_copy_in_an_agent_directory_is_left_alone() {
        let (tmp, root) = setup();
        let skill = write_skill(&root, "同名", "---\nname: 同名\n---\n正文\n");

        let agent_dir = tmp.path().join("agent-skills");
        let copy = agent_dir.join("同名");
        std::fs::create_dir_all(&copy).unwrap();
        std::fs::write(copy.join(library::SKILL_MANIFEST), "---\nname: 副本\n---\n").unwrap();

        let agent = fake_agent(&agent_dir);
        let outcome = uninstall_one(&root, "同名", std::slice::from_ref(&agent));

        assert_eq!(outcome.action, UninstallAction::Removed);
        assert!(
            outcome.removed_links.is_empty(),
            "真实目录不是链接，不该被摘"
        );
        assert!(!skill.exists(), "中央库那份照常卸载");
        assert!(
            copy.join(library::SKILL_MANIFEST).is_file(),
            "实体副本必须完好"
        );
    }

    /// 指向**别处**的链接（`ForeignLink`）不是我们的，不许碰
    #[test]
    fn a_link_pointing_outside_the_library_is_not_touched() {
        let (tmp, root) = setup();
        write_skill(&root, "有外链的", "---\nname: 有外链的\n---\n");

        // 某个无关目录，被当作链接目标
        let elsewhere = tmp.path().join("别处的东西");
        std::fs::create_dir_all(&elsewhere).unwrap();

        let agent_dir = tmp.path().join("agent-skills");
        std::fs::create_dir_all(&agent_dir).unwrap();
        let foreign = agent_dir.join("有外链的");
        link::create_junction(&elsewhere, &foreign).unwrap();

        let agent = fake_agent(&agent_dir);
        let outcome = uninstall_one(&root, "有外链的", std::slice::from_ref(&agent));

        assert_eq!(outcome.action, UninstallAction::Removed);
        assert!(
            outcome.removed_links.is_empty(),
            "指向中央库之外的链接不是我们的，不该摘"
        );
        assert!(foreign.exists(), "那条外链必须还在");
        assert!(elsewhere.is_dir(), "它的目标更不能被动");
    }

    /// 批量里部分成功时，成功的那些不回滚
    #[test]
    fn partial_success_does_not_roll_back_the_successful_items() {
        let (_tmp, root) = setup();
        let good = write_skill(&root, "好的", "---\nname: 好的\n---\n");

        let report = uninstall(
            &root,
            &[
                "好的".to_string(),
                "不存在的".to_string(),
                "../越界".to_string(),
            ],
        )
        .unwrap();

        assert_eq!(report.outcomes[0].action, UninstallAction::Removed);
        assert!(!good.exists());
        assert_eq!(report.outcomes[1].action, UninstallAction::Skipped);
        assert_eq!(report.outcomes[2].action, UninstallAction::Skipped);
    }

    /// 索引要跟上：卸载后它不该还留在清单里
    #[test]
    fn the_index_no_longer_lists_the_uninstalled_skill() {
        let (_tmp, root) = setup();
        write_skill(&root, "会消失的", "---\nname: 会消失的\n---\n");
        index::rebuild(&root).unwrap();

        let before = index::list_all(&root).unwrap();
        assert_eq!(before.len(), 1);

        uninstall(&root, &["会消失的".to_string()]).unwrap();

        let after = index::list_all(&root).unwrap();
        assert!(after.is_empty(), "卸载后索引里不该还有它：{after:?}");
    }
}
