//! SKILL.md 文档模型：YAML frontmatter + Markdown 正文。
//!
//! # 设计要点
//!
//! 1. **元数据以文件为唯一事实来源**，SQLite 索引是派生物，可随时重建。
//!    因此本模块的读-写往返必须无损，否则一次"编辑保存"就会损坏用户数据。
//! 2. **未知字段必须原样保留**。SKILL.md 是多 Agent 共用的格式，其他 Agent
//!    可能有本项目不认识的键（`allowed-tools` 等），丢弃它们等于破坏配置。
//! 3. **字段顺序必须稳定**。`serde_yaml::Mapping` 底层是 `IndexMap`，
//!    天然保留插入顺序，往返不会重排用户的 frontmatter。
//!
//! 依赖 `serde_yaml` 0.9.34（crates.io 上标记为 deprecated 但功能完整）。
//! 它仍是当前能同时满足"保序 + 无损往返"的最省事选择；若日后迁移，
//! 本模块是唯一需要改动的地方。

use serde_yaml::{Mapping, Value};

use crate::error::{AppError, AppResult};

/// frontmatter 分隔符
const DELIMITER: &str = "---";

/// 一份 SKILL.md 的结构化表示
#[derive(Debug, Clone)]
pub struct SkillDocument {
    /// 完整的 frontmatter 映射（保序，含未识别字段）
    frontmatter: Mapping,
    /// Markdown 正文，逐字节保留
    body: String,
    /// 原文件是否带 frontmatter 块。
    /// 用于避免给一个本来就没有 frontmatter 的文件凭空加上 `---` 块。
    had_frontmatter: bool,
}

impl SkillDocument {
    /// 解析 SKILL.md 原文
    pub fn parse(raw: &str) -> AppResult<Self> {
        // 去掉 UTF-8 BOM：带 BOM 的文件在 Windows 上很常见
        let text = raw.strip_prefix('\u{feff}').unwrap_or(raw);

        match split_frontmatter(text)? {
            Some((yaml_text, body)) => {
                let frontmatter = parse_yaml_mapping(yaml_text)?;
                Ok(Self {
                    frontmatter,
                    body: body.to_string(),
                    had_frontmatter: true,
                })
            }
            None => Ok(Self {
                frontmatter: Mapping::new(),
                body: text.to_string(),
                had_frontmatter: false,
            }),
        }
    }

    /// 序列化回 SKILL.md 文本
    pub fn render(&self) -> AppResult<String> {
        if self.frontmatter.is_empty() && !self.had_frontmatter {
            return Ok(self.body.clone());
        }

        let yaml = serde_yaml::to_string(&Value::Mapping(self.frontmatter.clone()))
            .map_err(|err| AppError::Config(format!("序列化 frontmatter 失败：{err}")))?;
        // serde_yaml 的产出末尾带换行，统一裁掉后由我们控制分隔符格式
        let yaml = yaml.trim_end_matches(['\n', '\r']);

        Ok(format!("{DELIMITER}\n{yaml}\n{DELIMITER}\n{}", self.body))
    }

    /// 读取一个字符串字段（仅当值确实是字符串时）
    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.frontmatter.get(Value::String(key.to_string())) {
            Some(Value::String(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// 设置一个字符串字段；`value` 为 `None` 时删除该键
    pub fn set_str(&mut self, key: &str, value: Option<&str>) {
        let key = Value::String(key.to_string());
        match value {
            Some(v) => {
                self.frontmatter.insert(key, Value::String(v.to_string()));
            }
            None => {
                self.frontmatter.remove(&key);
            }
        }
    }

    /// Skill 名称。缺省时回退到 `None`，由调用方用目录名兜底。
    pub fn name(&self) -> Option<&str> {
        self.get_str("name")
    }

    pub fn description(&self) -> Option<&str> {
        self.get_str("description")
    }

    /// 标签。同时兼容 YAML 序列与单个字符串两种写法：
    /// `tags: [a, b]` 与 `tags: a` 都应能读出。
    pub fn tags(&self) -> Vec<String> {
        match self.frontmatter.get(Value::String("tags".to_string())) {
            Some(Value::Sequence(items)) => items
                .iter()
                .filter_map(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    // 标签写成数字等标量时也接受
                    Value::Number(n) => Some(n.to_string()),
                    Value::Bool(b) => Some(b.to_string()),
                    _ => None,
                })
                .collect(),
            Some(Value::String(s)) => s
                .split(',')
                .map(|t| t.trim())
                .filter(|t| !t.is_empty())
                .map(|t| t.to_string())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// 设置标签（始终写为 YAML 序列，规范化为单一形态）
    pub fn set_tags(&mut self, tags: &[String]) {
        let key = Value::String("tags".to_string());
        if tags.is_empty() {
            self.frontmatter.remove(&key);
            return;
        }
        let seq = tags
            .iter()
            .map(|t| Value::String(t.clone()))
            .collect::<Vec<_>>();
        self.frontmatter.insert(key, Value::Sequence(seq));
    }

    /// 供索引模块使用的只读访问
    pub fn frontmatter(&self) -> &Mapping {
        &self.frontmatter
    }

    pub fn body(&self) -> &str {
        &self.body
    }
}

/// 切出 frontmatter 文本与正文。
///
/// 返回 `None` 表示文件没有 frontmatter（此时整份文本都是正文）。
/// 只有**第一行**恰为 `---` 时才认为存在 frontmatter——这是避免把正文里的
/// 水平分割线误判为 frontmatter 起始的关键。
fn split_frontmatter(text: &str) -> AppResult<Option<(&str, &str)>> {
    let after_open = text
        .strip_prefix("---\r\n")
        .or_else(|| text.strip_prefix("---\n"));

    let Some(rest) = after_open else {
        return Ok(None);
    };

    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        if content.trim() == DELIMITER {
            let yaml_text = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return Ok(Some((yaml_text, body)));
        }
        offset += line.len();
    }

    // 有起始分隔符但没有结束分隔符：视为格式损坏，而不是静默当作正文。
    // 静默处理会导致"编辑保存后 frontmatter 消失"这类难排查的数据损坏。
    Err(AppError::Config(
        "SKILL.md 的 frontmatter 缺少结束分隔符 `---`".to_string(),
    ))
}

fn parse_yaml_mapping(yaml_text: &str) -> AppResult<Mapping> {
    if yaml_text.trim().is_empty() {
        return Ok(Mapping::new());
    }
    match serde_yaml::from_str::<Value>(yaml_text) {
        Ok(Value::Mapping(m)) => Ok(m),
        Ok(Value::Null) => Ok(Mapping::new()),
        Ok(_) => Err(AppError::Config(
            "SKILL.md 的 frontmatter 必须是键值映射".to_string(),
        )),
        Err(err) => Err(AppError::Config(format!("解析 frontmatter 失败：{err}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "---\nname: pdf-tools\ndescription: 处理 PDF 的工具集\n\
         allowed-tools: Read, Write\ncustom-unknown-key: 保留我\ntags:\n  - pdf\n  - 文档\n---\n\n# PDF Tools\n\n正文内容。\n";

    #[test]
    fn parses_known_fields() {
        let doc = SkillDocument::parse(SAMPLE).unwrap();
        assert_eq!(doc.name(), Some("pdf-tools"));
        assert_eq!(doc.description(), Some("处理 PDF 的工具集"));
        assert_eq!(doc.tags(), vec!["pdf".to_string(), "文档".to_string()]);
    }

    /// 未知字段在读取-写回后不丢失
    #[test]
    fn round_trip_preserves_unknown_fields_and_body() {
        let doc = SkillDocument::parse(SAMPLE).unwrap();
        let rendered = doc.render().unwrap();
        let reparsed = SkillDocument::parse(&rendered).unwrap();

        assert_eq!(
            reparsed.get_str("allowed-tools"),
            Some("Read, Write"),
            "其他 Agent 的专属键被丢弃了"
        );
        assert_eq!(reparsed.get_str("custom-unknown-key"), Some("保留我"));
        assert_eq!(reparsed.body(), doc.body(), "正文被改动了");
    }

    /// 字段顺序稳定：避免每次保存都给用户制造无意义的 git diff
    #[test]
    fn round_trip_preserves_key_order() {
        let doc = SkillDocument::parse(SAMPLE).unwrap();
        let rendered = doc.render().unwrap();
        let first_line = rendered.lines().nth(1).unwrap();
        assert_eq!(first_line, "name: pdf-tools", "frontmatter 首个键不是 name");
        assert!(
            rendered.find("name:").unwrap() < rendered.find("description:").unwrap(),
            "键顺序被重排"
        );
    }

    #[test]
    fn file_without_frontmatter_is_untouched() {
        let raw = "# 纯正文\n\n没有 frontmatter。\n";
        let doc = SkillDocument::parse(raw).unwrap();
        assert!(doc.name().is_none());
        assert_eq!(
            doc.render().unwrap(),
            raw,
            "无 frontmatter 的文件不应被加上分隔符"
        );
    }

    #[test]
    fn handles_crlf_line_endings() {
        let raw = "---\r\nname: crlf-skill\r\n---\r\n正文\r\n";
        let doc = SkillDocument::parse(raw).unwrap();
        assert_eq!(doc.name(), Some("crlf-skill"));
        assert_eq!(doc.body(), "正文\r\n");
    }

    #[test]
    fn handles_utf8_bom() {
        let raw = "\u{feff}---\nname: bom-skill\n---\nbody\n";
        let doc = SkillDocument::parse(raw).unwrap();
        assert_eq!(doc.name(), Some("bom-skill"));
    }

    #[test]
    fn horizontal_rule_in_body_is_not_frontmatter() {
        let raw = "# 标题\n\n---\n\n分割线之后的正文\n";
        let doc = SkillDocument::parse(raw).unwrap();
        assert!(doc.name().is_none());
        assert_eq!(doc.body(), raw);
    }

    #[test]
    fn unterminated_frontmatter_is_an_error() {
        let raw = "---\nname: broken\n正文没有结束分隔符\n";
        assert!(SkillDocument::parse(raw).is_err());
    }

    #[test]
    fn tags_accepts_scalar_and_sequence() {
        let scalar = SkillDocument::parse("---\ntags: a, b , c\n---\n").unwrap();
        assert_eq!(scalar.tags(), vec!["a", "b", "c"]);

        let seq = SkillDocument::parse("---\ntags: [x, y]\n---\n").unwrap();
        assert_eq!(seq.tags(), vec!["x", "y"]);
    }

    #[test]
    fn set_tags_normalizes_to_sequence() {
        let mut doc = SkillDocument::parse("---\nname: t\n---\n").unwrap();
        doc.set_tags(&["one".into(), "two".into()]);
        let rendered = doc.render().unwrap();
        let reparsed = SkillDocument::parse(&rendered).unwrap();
        assert_eq!(reparsed.tags(), vec!["one", "two"]);
    }
}
