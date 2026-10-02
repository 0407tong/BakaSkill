//! 标签的统计与批量改写。
//!
//! # 单一事实来源是文件
//!
//! 标签只有一处真相：各 Skill 的 `SKILL.md` frontmatter 里的 `tags` 字段。
//! 索引里的 `tags_json` 只是它的**派生副本**，因此本模块的所有写操作都是
//! "改文件 → 重建索引"，而不是"改索引"。反过来的话，一次索引重建就会把
//! 用户的修改抹掉。
//!
//! # 只作用于中央库
//!
//! 标签统计与改写都只认中央库里的 Skill。Agent 目录里的外部副本没有独立的
//! frontmatter 可改（它要么是指向中央库的链接，要么是一份还没纳入的副本），
//! 对它们做"重命名标签"要么改不动、要么改出一份分叉的数据。
//!
//! # 大小写
//!
//! 标签**按大小写不敏感归并**：`Rust` 与 `rust` 在统计里是同一条，改写时
//! 一起改。理由是这个页面本质上是个"整理"工具——把同一个标签的几种写法
//! 收拢成一种，正是用户来这里要做的事。展示用的是**首次遇到的拼写**。

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::library;
use crate::skill::SkillDocument;

/// 一个标签及其出现次数
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    /// 展示用的拼写（首次遇到的那一种）
    pub tag: String,
    pub count: usize,
}

/// 单个 Skill 的改写结果
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TagChange {
    pub dir_name: String,
    pub name: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

/// 标签改写的整体结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagReport {
    pub changed: Vec<TagChange>,
    /// 改写失败的 Skill 及原因。**不静默忽略**：批量改写里"失败了几个"
    /// 是用户必须知道的，否则他以为已经改完了。
    pub failed: Vec<TagFailure>,
    /// 重建索引后的 Skill 总数（供界面确认索引已跟上）
    pub indexed: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagFailure {
    pub dir_name: String,
    pub reason: String,
}

/// 统计中央库里的标签。
///
/// 数据取自**索引**（它已经是 tags 的一份派生副本），因此调用前索引必须是最新的；
/// 各写操作结束时会自动重建，正常流程下不必额外处理。
pub fn stats(root: &Path) -> AppResult<Vec<TagCount>> {
    let conn = crate::index::open(root)?;
    let mut stmt = conn
        .prepare("SELECT tags_json FROM skills")
        .map_err(|err| AppError::Io(format!("准备标签统计失败：{err}")))?;

    let rows = stmt
        .query_map([], |row| row.get::<_, Option<String>>(0))
        .map_err(|err| AppError::Io(format!("统计标签失败：{err}")))?;

    // key 用小写归并，value 保留首次遇到的拼写
    let mut counts: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for raw in rows.filter_map(Result::ok).flatten() {
        let tags: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
        for tag in tags {
            let trimmed = tag.trim();
            if trimmed.is_empty() {
                continue;
            }
            let key = trimmed.to_lowercase();
            counts
                .entry(key)
                .and_modify(|(_, count)| *count += 1)
                .or_insert_with(|| (trimmed.to_string(), 1));
        }
    }

    let mut out: Vec<TagCount> = counts
        .into_values()
        .map(|(tag, count)| TagCount { tag, count })
        .collect();
    // 多的在前；同数量按名称，保证顺序稳定（否则每次刷新都在跳）
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.tag.to_lowercase().cmp(&b.tag.to_lowercase()))
    });
    Ok(out)
}

/// 把一个或多个标签改写成一个新标签，或删除它们（`target` 为 `None`）。
///
/// 三个界面动作共用这一个入口：
///
/// | 动作 | 参数 |
/// | --- | --- |
/// | 重命名 | 一个来源 + 新名字 |
/// | 合并 | 多个来源 + 新名字 |
/// | 删除 | 一个或多个来源 + `None` |
///
/// 它们本就是同一件事的不同参数化，分成三条命令只会让三处各自演化出
/// 细微不同的行为。
pub fn apply(root: &Path, sources: &[String], target: Option<&str>) -> AppResult<TagReport> {
    let source_keys: HashSet<String> = sources
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if source_keys.is_empty() {
        return Err(AppError::Config("没有指定要改写的标签".to_string()));
    }

    let target = target.map(str::trim).filter(|t| !t.is_empty());
    if let Some(t) = target {
        if source_keys.contains(&t.to_lowercase()) {
            return Err(AppError::Config(format!(
                "新标签与来源标签相同（{t}），这次改写不会改变任何东西"
            )));
        }
    }

    let skills = crate::index::list_all(root)?;
    let mut changed = Vec::new();
    let mut failed = Vec::new();

    for skill in skills {
        let affected = skill
            .tags
            .iter()
            .any(|t| source_keys.contains(&t.trim().to_lowercase()));
        if !affected {
            continue;
        }

        match retag_skill(&skill.dir_path, &source_keys, target) {
            Ok((before, after)) => changed.push(TagChange {
                dir_name: skill
                    .dir_path
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or_default()
                    .to_string(),
                name: skill.name,
                before,
                after,
            }),
            Err(err) => failed.push(TagFailure {
                dir_name: skill
                    .dir_path
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or_default()
                    .to_string(),
                reason: err.to_string(),
            }),
        }
    }

    // 文件改完才重建索引：索引是派生物，任何时候重建都应当反映磁盘的真实状态。
    // 即使上面有几个失败项也要重建——成功的那部分必须立刻能被搜到，
    // 否则用户会看到"文件改了但搜索还是旧的"。
    let indexed = if changed.is_empty() {
        crate::index::count(root).unwrap_or(0)
    } else {
        crate::index::rebuild(root)?.skill_count
    };

    tracing::info!(
        sources = ?source_keys,
        target = ?target,
        changed = changed.len(),
        failed = failed.len(),
        "标签改写完成"
    );

    Ok(TagReport {
        changed,
        failed,
        indexed,
    })
}

/// 改写单个 Skill 的标签，返回 (改前, 改后)。
fn retag_skill(
    dir_path: &str,
    source_keys: &HashSet<String>,
    target: Option<&str>,
) -> AppResult<(Vec<String>, Vec<String>)> {
    let dir = Path::new(dir_path);
    let manifest = find_manifest(dir)
        .ok_or_else(|| AppError::NotFound(format!("目录中没有 SKILL.md：{}", dir.display())))?;

    let raw = std::fs::read_to_string(&manifest)
        .map_err(|err| AppError::from_io(&format!("读取 {} 失败", manifest.display()), err))?;
    let mut doc = SkillDocument::parse(&raw)?;

    let before = doc.tags();
    let after = retag(&before, source_keys, target);
    if before == after {
        return Ok((before, after));
    }

    doc.set_tags(&after);
    let rendered = doc.render()?;

    // 原子写：临时文件 + 替换。中途失败不会留下半个 SKILL.md。
    let tmp = manifest.with_extension("md.tmp");
    std::fs::write(&tmp, rendered.as_bytes())
        .map_err(|err| AppError::from_io(&format!("写入临时文件失败：{}", tmp.display()), err))?;
    std::fs::rename(&tmp, &manifest).map_err(|err| {
        AppError::from_io(&format!("替换 SKILL.md 失败：{}", manifest.display()), err)
    })?;

    Ok((before, after))
}

/// 把 `sources` 里的标签换成 `target`（或删除），其余标签原样保留。
///
/// 新标签放在**第一个被替换的位置**上，而不是追加到末尾——这样用户看到的
/// 标签顺序不会因为一次重命名而整体跳动。
fn retag(existing: &[String], source_keys: &HashSet<String>, target: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut target_placed = false;

    for tag in existing {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }

        if source_keys.contains(&trimmed.to_lowercase()) {
            if let Some(new_tag) = target {
                if !target_placed {
                    out.push(new_tag.to_string());
                    target_placed = true;
                }
            }
            // target 为 None 时就是删除：什么都不推
            continue;
        }

        // 顺带去掉完全重复的标签（大小写不敏感）
        if !out.iter().any(|x| x.eq_ignore_ascii_case(trimmed)) {
            out.push(trimmed.to_string());
        }
    }

    if let Some(new_tag) = target {
        if !target_placed && !out.iter().any(|x| x.eq_ignore_ascii_case(new_tag)) {
            out.push(new_tag.to_string());
        }
    }

    out
}

fn find_manifest(dir: &Path) -> Option<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| {
            p.is_file()
                && p.file_name()
                    .map(|n| {
                        n.to_string_lossy()
                            .eq_ignore_ascii_case(library::SKILL_MANIFEST)
                    })
                    .unwrap_or(false)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("library");
        library::initialize(&root.display().to_string()).unwrap();
        (dir, root)
    }

    fn add_skill(root: &Path, dir_name: &str, content: &str) {
        let skill_dir = library::skills_dir(root).join(dir_name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join(library::SKILL_MANIFEST), content).unwrap();
    }

    fn skill_with_tags(name: &str, tags: &[&str]) -> String {
        let tags_yaml = if tags.is_empty() {
            "tags: []".to_string()
        } else {
            format!(
                "tags:\n{}",
                tags.iter()
                    .map(|t| format!("  - {t}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };
        format!("---\nname: {name}\ndescription: 说明\n{tags_yaml}\n---\n\n正文内容。\n")
    }

    fn read_manifest(root: &Path, dir_name: &str) -> String {
        std::fs::read_to_string(
            library::skills_dir(root)
                .join(dir_name)
                .join(library::SKILL_MANIFEST),
        )
        .unwrap()
    }

    fn tags_of(root: &Path, dir_name: &str) -> Vec<String> {
        SkillDocument::parse(&read_manifest(root, dir_name))
            .unwrap()
            .tags()
    }

    #[test]
    fn stats_counts_across_skills_and_merges_case() {
        let (_dir, root) = setup();
        add_skill(&root, "a", &skill_with_tags("a", &["写作", "review"]));
        add_skill(&root, "b", &skill_with_tags("b", &["写作", "Rust"]));
        add_skill(&root, "c", &skill_with_tags("c", &["rust"]));
        crate::index::rebuild(&root).unwrap();

        let stats = stats(&root).unwrap();
        let find = |tag: &str| {
            stats
                .iter()
                .find(|t| t.tag.eq_ignore_ascii_case(tag))
                .map(|t| t.count)
        };

        assert_eq!(find("写作"), Some(2));
        // Rust 与 rust 归并成一条
        assert_eq!(find("rust"), Some(2));
        assert_eq!(
            stats
                .iter()
                .filter(|t| t.tag.eq_ignore_ascii_case("rust"))
                .count(),
            1
        );
        assert_eq!(find("review"), Some(1));
    }

    #[test]
    fn stats_on_empty_library_is_empty() {
        let (_dir, root) = setup();
        crate::index::rebuild(&root).unwrap();
        assert!(stats(&root).unwrap().is_empty());
    }

    /// 重命名：改文件、其余标签与正文一字不动、索引跟上
    #[test]
    fn rename_rewrites_frontmatter_and_refreshes_index() {
        let (_dir, root) = setup();
        add_skill(&root, "a", &skill_with_tags("a", &["旧名", "保留"]));
        crate::index::rebuild(&root).unwrap();

        let report = apply(&root, &["旧名".to_string()], Some("新名")).unwrap();
        assert_eq!(report.changed.len(), 1);
        assert!(report.failed.is_empty());
        assert_eq!(report.changed[0].before, vec!["旧名", "保留"]);
        assert_eq!(report.changed[0].after, vec!["新名", "保留"]);

        // 文件核对：改动必须真的写进了 SKILL.md
        let text = read_manifest(&root, "a");
        assert!(text.contains("- 新名"));
        assert!(!text.contains("旧名"));
        assert!(text.contains("正文内容。"), "正文被改动了：{text}");

        // 索引跟上：搜索/统计立刻能看到新标签
        assert_eq!(report.indexed, 1);
        let names: Vec<String> = stats(&root).unwrap().into_iter().map(|t| t.tag).collect();
        assert!(names.contains(&"新名".to_string()));
        assert!(!names.contains(&"旧名".to_string()));
    }

    /// 合并：多个来源合成一个，且新标签落在**第一个被替换的位置**上
    #[test]
    fn merge_places_target_at_the_first_replaced_position() {
        let (_dir, root) = setup();
        add_skill(
            &root,
            "a",
            &skill_with_tags("a", &["甲", "中间", "乙", "尾部"]),
        );
        crate::index::rebuild(&root).unwrap();

        apply(&root, &["甲".to_string(), "乙".to_string()], Some("合并")).unwrap();

        assert_eq!(tags_of(&root, "a"), vec!["合并", "中间", "尾部"]);
    }

    /// 新标签不重复插入（目标标签已经存在时）
    #[test]
    fn merge_does_not_duplicate_an_existing_target() {
        let (_dir, root) = setup();
        add_skill(&root, "a", &skill_with_tags("a", &["甲", "乙"]));
        crate::index::rebuild(&root).unwrap();

        apply(&root, &["甲".to_string()], Some("乙")).unwrap();

        assert_eq!(tags_of(&root, "a"), vec!["乙"]);
    }

    /// 删除
    #[test]
    fn delete_removes_the_tag() {
        let (_dir, root) = setup();
        add_skill(&root, "a", &skill_with_tags("a", &["删我", "留我"]));
        crate::index::rebuild(&root).unwrap();

        apply(&root, &["删我".to_string()], None).unwrap();

        assert_eq!(tags_of(&root, "a"), vec!["留我"]);
    }

    /// 大小写不敏感：改 `rust` 会连 `Rust` 一起改
    #[test]
    fn rename_is_case_insensitive() {
        let (_dir, root) = setup();
        add_skill(&root, "a", &skill_with_tags("a", &["Rust"]));
        add_skill(&root, "b", &skill_with_tags("b", &["rust"]));
        crate::index::rebuild(&root).unwrap();

        let report = apply(&root, &["rust".to_string()], Some("RustLang")).unwrap();

        assert_eq!(report.changed.len(), 2, "大小写不同的同一个标签应一起改写");
        assert_eq!(tags_of(&root, "a"), vec!["RustLang"]);
        assert_eq!(tags_of(&root, "b"), vec!["RustLang"]);
    }

    /// 没有 Skill 用这个标签时，如实报告"改了 0 个"，而不是报错
    #[test]
    fn renaming_an_unused_tag_changes_nothing() {
        let (_dir, root) = setup();
        add_skill(&root, "a", &skill_with_tags("a", &["甲"]));
        crate::index::rebuild(&root).unwrap();

        let report = apply(&root, &["不存在".to_string()], Some("新")).unwrap();

        assert!(report.changed.is_empty());
        assert!(report.failed.is_empty());
        assert_eq!(tags_of(&root, "a"), vec!["甲"]);
    }

    /// 改成一个相同的名字是空操作，应当明确拒绝而不是假装成功
    #[test]
    fn renaming_to_the_same_name_is_rejected() {
        let (_dir, root) = setup();
        add_skill(&root, "a", &skill_with_tags("a", &["甲"]));
        crate::index::rebuild(&root).unwrap();

        assert!(apply(&root, &["甲".to_string()], Some("甲")).is_err());
        assert!(apply(&root, &["甲".to_string()], Some("  甲  ")).is_err());
    }

    #[test]
    fn empty_sources_are_rejected() {
        let (_dir, root) = setup();
        crate::index::rebuild(&root).unwrap();

        assert!(apply(&root, &[], Some("新")).is_err());
        assert!(apply(&root, &["   ".to_string()], Some("新")).is_err());
    }

    /// 没有 tags 字段的 Skill 不该被迫生成一个空标签列表
    #[test]
    fn skill_without_tags_is_left_alone() {
        let (_dir, root) = setup();
        add_skill(&root, "a", "---\nname: a\n---\n\n没有标签。\n");
        add_skill(&root, "b", &skill_with_tags("b", &["有标签"]));
        crate::index::rebuild(&root).unwrap();

        let report = apply(&root, &["有标签".to_string()], Some("改过")).unwrap();

        assert_eq!(report.changed.len(), 1);
        assert_eq!(report.changed[0].dir_name, "b");
        let untouched = read_manifest(&root, "a");
        assert!(
            !untouched.contains("tags"),
            "无关的 Skill 被写入了 tags 字段：{untouched}"
        );
    }

    /// 重命名要保住**其余 frontmatter 字段**（无损往返）
    #[test]
    fn rename_preserves_other_frontmatter_fields() {
        let (_dir, root) = setup();
        add_skill(
            &root,
            "a",
            "---\nname: a\ndescription: 描述\nversion: 2\ncustom: 值\ntags:\n  - 旧\n---\n\n正文。\n",
        );
        crate::index::rebuild(&root).unwrap();

        apply(&root, &["旧".to_string()], Some("新")).unwrap();

        let text = read_manifest(&root, "a");
        assert!(text.contains("version: 2"), "未知字段被丢掉了：{text}");
        assert!(text.contains("custom: 值"), "未知字段被丢掉了：{text}");
        assert!(text.contains("description: 描述"));
        assert!(text.contains("正文。"));
    }

    #[test]
    fn retag_keeps_order_and_dedupes() {
        let sources: HashSet<String> = ["a".to_string()].into_iter().collect();

        let existing = vec!["a".to_string(), "b".to_string(), "A".to_string()];
        assert_eq!(
            retag(&existing, &sources, Some("z")),
            vec!["z", "b"],
            "重复的来源标签只该留下一个新标签"
        );

        let dup = vec!["b".to_string(), "B".to_string()];
        assert_eq!(retag(&dup, &sources, None), vec!["b"], "自身重复也应去掉");
    }
}
