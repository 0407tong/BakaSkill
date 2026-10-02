//! SKILL.md 读写命令。
//!
//! **这是唯一会修改用户 Skill 文件的命令族**，因此规则从严：
//! - 只认目录下的 `SKILL.md`，不接受任意文件路径；
//! - 写入前必须先解析成功，格式损坏的文本不会被落盘；
//! - 写入采用「先写临时文件再替换」，避免中途失败留下半个文件。

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::library;
use crate::skill::SkillDocument;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFile {
    /// SKILL.md 的绝对路径
    pub path: String,
    pub content: String,
    /// 解析出的元数据，便于前端直接展示而无需再解析一遍
    pub name: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
}

/// 定位技能目录下的 SKILL.md
fn manifest_of(dir: &str) -> AppResult<PathBuf> {
    let dir_path = Path::new(dir);
    if !dir_path.is_dir() {
        return Err(AppError::NotFound(format!("技能目录不存在：{dir}")));
    }

    let entries = std::fs::read_dir(dir_path)
        .map_err(|err| AppError::from_io(&format!("读取目录失败：{dir}"), err))?;

    for entry in entries.filter_map(Result::ok) {
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(library::SKILL_MANIFEST)
        {
            return Ok(entry.path());
        }
    }
    Err(AppError::NotFound(format!("该目录下没有 SKILL.md：{dir}")))
}

/// 读取一个 Skill 的 SKILL.md
#[tauri::command(async)]
pub fn skill_read(dir_path: String) -> AppResult<SkillFile> {
    let manifest = manifest_of(&dir_path)?;
    let content = std::fs::read_to_string(&manifest)
        .map_err(|err| AppError::from_io(&format!("读取失败：{}", manifest.display()), err))?;

    let (name, description, tags) = match SkillDocument::parse(&content) {
        Ok(doc) => (
            doc.name().map(str::to_string),
            doc.description().map(str::to_string),
            doc.tags(),
        ),
        // 解析失败不阻断读取：用户可能正想打开它来修好
        Err(_) => (None, None, Vec::new()),
    };

    Ok(SkillFile {
        path: manifest.display().to_string(),
        content,
        name,
        description,
        tags,
    })
}

/// 写回 SKILL.md。
///
/// 写入前校验：内容必须能被解析（frontmatter 结构完整）。
/// 拒绝把损坏的内容落盘——那会破坏用户的 Skill，且索引重建时会被跳过。
#[tauri::command(async)]
pub fn skill_write(dir_path: String, content: String) -> AppResult<SkillFile> {
    let manifest = manifest_of(&dir_path)?;

    // 校验：解析不通过就不写
    SkillDocument::parse(&content)
        .map_err(|err| AppError::Config(format!("内容校验未通过，未写入：{err}")))?;

    // 原子写：临时文件 + 替换，避免中途失败留下半个文件
    let tmp = manifest.with_extension("md.tmp");
    std::fs::write(&tmp, content.as_bytes())
        .map_err(|err| AppError::from_io(&format!("写入临时文件失败：{}", tmp.display()), err))?;
    std::fs::rename(&tmp, &manifest).map_err(|err| {
        AppError::from_io(&format!("替换 SKILL.md 失败：{}", manifest.display()), err)
    })?;

    tracing::info!(path = %manifest.display(), "SKILL.md 已保存");

    skill_read(dir_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_skill(dir: &Path, content: &str) {
        let skill = dir.join("demo");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn reads_skill_and_extracts_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        make_skill(
            tmp.path(),
            "---\nname: demo\ndescription: 演示\ntags: [a, b]\n---\n正文\n",
        );

        let file = skill_read(tmp.path().join("demo").display().to_string()).unwrap();
        assert_eq!(file.name.as_deref(), Some("demo"));
        assert_eq!(file.description.as_deref(), Some("演示"));
        assert_eq!(file.tags, vec!["a", "b"]);
        assert!(file.content.contains("正文"));
    }

    #[test]
    fn missing_dir_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let err = skill_read(tmp.path().join("nope").display().to_string()).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn dir_without_manifest_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();

        let err = skill_read(empty.display().to_string()).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn write_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        make_skill(tmp.path(), "---\nname: demo\n---\n旧正文\n");
        let dir = tmp.path().join("demo").display().to_string();

        let updated = skill_write(
            dir.clone(),
            "---\nname: demo\ndescription: 新描述\n---\n新正文\n".to_string(),
        )
        .unwrap();

        assert_eq!(updated.description.as_deref(), Some("新描述"));
        let on_disk = std::fs::read_to_string(tmp.path().join("demo").join("SKILL.md")).unwrap();
        assert!(on_disk.contains("新正文"));
        assert!(!on_disk.contains("旧正文"));
    }

    /// 格式损坏的内容必须被拒绝，不能落盘破坏用户的文件
    #[test]
    fn write_rejects_unparseable_content() {
        let tmp = tempfile::tempdir().unwrap();
        make_skill(tmp.path(), "---\nname: demo\n---\n原正文\n");
        let dir = tmp.path().join("demo").display().to_string();

        let err = skill_write(dir, "---\nname: broken\n没有结束分隔符\n".to_string()).unwrap_err();
        assert!(matches!(err, AppError::Config(_)));

        // 原文件必须保持原样
        let on_disk = std::fs::read_to_string(tmp.path().join("demo").join("SKILL.md")).unwrap();
        assert!(on_disk.contains("原正文"), "校验失败时不应改动原文件");
    }

    #[test]
    fn write_leaves_no_tmp_file() {
        let tmp = tempfile::tempdir().unwrap();
        make_skill(tmp.path(), "---\nname: demo\n---\n");
        let dir = tmp.path().join("demo").display().to_string();

        skill_write(dir, "---\nname: demo\n---\n新\n".to_string()).unwrap();

        let leftovers: Vec<_> = std::fs::read_dir(tmp.path().join("demo"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "残留临时文件：{leftovers:?}");
    }

    #[test]
    fn manifest_match_is_case_insensitive() {
        let tmp = tempfile::tempdir().unwrap();
        let skill = tmp.path().join("c");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("skill.md"), "---\nname: c\n---\n").unwrap();

        assert!(skill_read(skill.display().to_string()).is_ok());
    }
}
