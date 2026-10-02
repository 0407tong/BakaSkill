//! SQLite 派生索引。
//!
//! # 定位：索引是派生物，不是事实来源
//!
//! 元数据的唯一事实来源是各 Skill 目录里的 `SKILL.md`。本模块的数据库
//! 只是一份**可随时删除并重建**的加速缓存，用于支撑搜索性能要求。
//! 因此：
//!
//! - 数据库损坏或结构变更时，直接重建即可，无需数据迁移的顾虑；
//! - 任何"以索引为准去修正文件"的逻辑都是错误设计。
//!
//! 索引放在 `<central_library>/.bakaskill/index.db`，随中央库一起移动，
//! 删掉后下次启动会按需重建。

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::library;
use crate::skill::SkillDocument;

/// 索引结构版本。与 `config::CURRENT_SCHEMA_VERSION` 无关，独立演进。
///
/// v2：`skills` 增加 `body` 列，并新增全文搜索虚表 `skills_fts`。
pub const INDEX_SCHEMA_VERSION: i64 = 2;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillRecord {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub dir_path: String,
    pub tags: Vec<String>,
    pub content_hash: Option<String>,
    pub mtime: Option<i64>,
    pub size: Option<i64>,
    /// SKILL.md 正文。
    ///
    /// **不进 IPC**：`index_list` 一次会带上千条记录，把正文一并发给前端
    /// 没有意义（前端一个字节都用不上）。它只服务于全文搜索，以及
    /// "文件没变就跳过重新解析"这条增量路径——命中缓存时正需要它来
    /// 重新灌满搜索索引，否则每次重建都得把全部文件重读一遍。
    #[serde(skip)]
    pub body: String,
}

/// 一条全文搜索命中
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub skill_id: String,
    /// 命中处的正文片段（已截断，带省略号）。前端负责把关键词高亮出来——
    /// 后端不插控制字符或标签，免得"内容"和"标记"混在一起。
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RebuildReport {
    pub skill_count: usize,
    /// 命中增量缓存、跳过了重新解析的条目数
    pub cached_hits: usize,
    /// 实际重新解析的条目数
    pub parsed: usize,
    /// 无法解析而被跳过的目录（含原因），不应静默忽略
    pub skipped: Vec<SkippedEntry>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedEntry {
    pub dir_name: String,
    pub reason: String,
}

/// 打开（并在需要时初始化）索引数据库
pub fn open(root: &Path) -> AppResult<Connection> {
    let meta = library::meta_dir(root);
    if !meta.is_dir() {
        std::fs::create_dir_all(&meta)
            .map_err(|err| AppError::from_io("创建 .bakaskill 目录失败", err))?;
    }

    let db_path = library::index_db_path(root);
    let conn = Connection::open(&db_path)
        .map_err(|err| AppError::Io(format!("打开索引数据库失败：{err}")))?;

    // WAL 提升并发读性能；外键约束为后续 links 表做准备
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|err| AppError::Io(format!("设置 journal_mode 失败：{err}")))?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|err| AppError::Io(format!("启用外键失败：{err}")))?;

    migrate(&conn)?;
    Ok(conn)
}

/// 建表与结构迁移
///
/// # 为什么是"顺序执行"而不是"递归重来"
///
/// 早先的写法是：版本不匹配时 `DROP TABLE` 然后**递归调用 `migrate`**。
/// 那段代码一直没被跑过（索引结构从未变过），于是藏着一个
/// 致命缺陷——它**没有把 `meta.schema_version` 改写成新值**，
/// 递归进去看到的还是旧版本号，于是再判定不匹配、再删、再递归……
/// 把版本从 1 升到 2 后，第一次真跑到这条路径就是
/// `STATUS_STACK_OVERFLOW`（实测：应用启动即崩溃）。
///
/// 现在改成一条直线：建表 → 比对版本 → 需要就删掉派生的两张表、重建、写回版本号。
/// **没有任何递归**，也就没有"递归终止条件写错"这种可能。
fn migrate(conn: &Connection) -> AppResult<()> {
    create_tables(conn)?;

    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| AppError::Io(format!("读取 schema_version 失败：{err}")))?;

    match stored {
        Some(v) if v == INDEX_SCHEMA_VERSION.to_string() => {}
        Some(v) => {
            // 结构落后：索引是派生物，直接丢弃重建最省事也最安全。
            // 先删虚表再删内容表——虚表是外部内容表，反过来会导致内容表
            // 已经没了而虚表还在引用它。
            tracing::warn!(from = %v, to = INDEX_SCHEMA_VERSION, "索引结构版本不匹配，将重建");
            conn.execute_batch(
                "DROP TABLE IF EXISTS skills_fts;
                 DROP TABLE IF EXISTS skills;",
            )
            .map_err(|err| AppError::Io(format!("重建索引失败：{err}")))?;

            create_tables(conn)?;
            write_schema_version(conn)?;
        }
        None => write_schema_version(conn)?,
    }

    Ok(())
}

fn write_schema_version(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
        [INDEX_SCHEMA_VERSION.to_string()],
    )
    .map_err(|err| AppError::Io(format!("写入 schema_version 失败：{err}")))?;
    Ok(())
}

/// 建表（幂等）。`migrate` 与"版本不匹配后重建"两处都要用，因此独立出来。
fn create_tables(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS skills (
            id           TEXT PRIMARY KEY,
            name         TEXT NOT NULL,
            description  TEXT,
            dir_path     TEXT NOT NULL UNIQUE,
            tags_json    TEXT,
            content_hash TEXT,
            mtime        INTEGER,
            size         INTEGER,
            body         TEXT,
            updated_at   INTEGER NOT NULL
        );

        -- 全文搜索虚表：外部内容（正文只存一份，就在上面的 skills.body 里），
        -- 由重建流程用 `INSERT INTO skills_fts(skills_fts) VALUES('rebuild')`
        -- 一次性同步。
        --
        -- # 为什么是 trigram 而不是 unicode61
        --
        -- 实测（SQLite 3.53.2）：
        --   unicode61 把**一整段连续中文当成一个 token**，因此正文里的
        --   "中文技能说明" 既匹配不上 "技能" 也匹配不上 "中文技能"——中文搜索
        --   会**完全失效**。"对中文按字切分效果尚可"是一种常见误判。
        --   trigram 做的是**子串**匹配："技能"能命中"中文技能说明"，
        --   英文里 "tool" 也能命中 "tools"。
        --
        -- 代价：trigram 的 MATCH 要求查询至少 3 个字符，所以本模块的搜索
        -- 走 `LIKE` 而不是 `MATCH`——trigram 分词器本来就是为加速 LIKE/GLOB
        -- 而存在的（见 SQLite 文档），短查询退化成扫描，但依然**正确**。
        CREATE VIRTUAL TABLE IF NOT EXISTS skills_fts USING fts5(
            name, description, body,
            content='skills',
            content_rowid='rowid',
            tokenize='trigram'
        );

        CREATE TABLE IF NOT EXISTS agents (
            id           TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            skill_dir    TEXT,
            enabled      INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS links (
            skill_id    TEXT NOT NULL,
            agent_id    TEXT NOT NULL,
            link_path   TEXT NOT NULL,
            target_path TEXT,
            state       TEXT NOT NULL,
            checked_at  INTEGER,
            PRIMARY KEY (skill_id, agent_id)
        );

        CREATE INDEX IF NOT EXISTS idx_skills_name ON skills(name);
        "#,
    )
    .map_err(|err| AppError::Io(format!("初始化数据库结构失败：{err}")))?;

    Ok(())
}

/// 全量重建索引：扫描中央库并写入数据库。
///
/// 重建结果必须与磁盘一致。
pub fn rebuild(root: &Path) -> AppResult<RebuildReport> {
    let started = std::time::Instant::now();
    let skills_root = library::skills_dir(root);

    if !skills_root.is_dir() {
        return Err(AppError::NotFound(format!(
            "中央库尚未初始化：{}",
            root.display()
        )));
    }

    let mut conn = open(root)?;

    // 增量策略：先把现有索引读进内存，之后按 (mtime, size) 判断哪些条目
    // 自上次索引以来没有变化，从而跳过「读文件 + 解析 frontmatter」这一最贵的步骤。
    //
    // 注意仍然会重建整张表（先清空再写入），这样"磁盘上已删除的 Skill
    // 会从索引中消失"的性质得以保持。省掉的是解析开销，不是正确性。
    let cached = load_cache(&conn);

    let tx = conn
        .transaction()
        .map_err(|err| AppError::Io(format!("开启事务失败：{err}")))?;

    tx.execute("DELETE FROM skills", [])
        .map_err(|err| AppError::Io(format!("清空索引失败：{err}")))?;

    let entries = std::fs::read_dir(&skills_root)
        .map_err(|err| AppError::from_io("读取 skills 目录失败", err))?;

    let now = now_millis();
    let mut skill_count = 0usize;
    let mut cached_hits = 0usize;
    let mut parsed = 0usize;
    let mut skipped = Vec::new();

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let dir_name = entry.file_name().to_string_lossy().to_string();

        // 不跟随重解析点：中央库内出现链接属于异常状态，不纳入索引
        if crate::platform::link::is_junction(&path) {
            skipped.push(SkippedEntry {
                dir_name,
                reason: "该条目是目录链接，中央库内不应存在链接".to_string(),
            });
            continue;
        }
        if !path.is_dir() {
            continue;
        }

        let manifest = match find_manifest(&path) {
            Some(m) => m,
            None => {
                skipped.push(SkippedEntry {
                    dir_name,
                    reason: "目录中缺少 SKILL.md".to_string(),
                });
                continue;
            }
        };

        let metadata = std::fs::metadata(&manifest).ok();
        let mtime = metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .map(system_time_millis);
        let size = metadata.as_ref().map(|m| m.len() as i64);
        let dir_path = path.display().to_string();

        // 增量命中：mtime 与 size 都没变，沿用上次的解析结果，
        // 跳过「读文件 + 解析 frontmatter」——这是重建过程中最贵的一步。
        //
        // 必须两者都一致才跳过：只看 mtime 会漏掉"同一秒内改写"，
        // 只看 size 会漏掉"等长改写"。
        let record = match cached.get(&dir_path) {
            Some(c) if c.mtime == mtime && c.size == size => {
                cached_hits += 1;
                SkillRecord {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    description: c.description.clone(),
                    dir_path,
                    tags: c.tags.clone(),
                    content_hash: c.content_hash.clone(),
                    mtime,
                    size,
                    // 正文同样沿用缓存。没有它就得为了填搜索索引把文件重读一遍，
                    // 增量缓存也就白做了——省下的正是"读文件"这一步。
                    body: c.body.clone(),
                }
            }
            _ => {
                let raw = match std::fs::read_to_string(&manifest) {
                    Ok(r) => r,
                    Err(err) => {
                        skipped.push(SkippedEntry {
                            dir_name,
                            reason: format!("读取 SKILL.md 失败：{err}"),
                        });
                        continue;
                    }
                };

                let doc = match SkillDocument::parse(&raw) {
                    Ok(d) => d,
                    Err(err) => {
                        skipped.push(SkippedEntry {
                            dir_name,
                            reason: err.to_string(),
                        });
                        continue;
                    }
                };

                parsed += 1;
                SkillRecord {
                    id: normalize_id(doc.name().unwrap_or(&dir_name)),
                    // 名称回退到目录名：SKILL.md 可以没有 name 字段
                    name: doc.name().unwrap_or(&dir_name).to_string(),
                    description: doc.description().map(str::to_string),
                    dir_path,
                    tags: doc.tags(),
                    content_hash: Some(fnv1a_hex(raw.as_bytes())),
                    mtime,
                    size,
                    body: doc.body().to_string(),
                }
            }
        };

        tx.execute(
            r#"INSERT INTO skills
                 (id, name, description, dir_path, tags_json, content_hash, mtime, size, body, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
               ON CONFLICT(dir_path) DO UPDATE SET
                 id = excluded.id, name = excluded.name, description = excluded.description,
                 tags_json = excluded.tags_json, content_hash = excluded.content_hash,
                 mtime = excluded.mtime, size = excluded.size, body = excluded.body,
                 updated_at = excluded.updated_at"#,
            rusqlite::params![
                record.id,
                record.name,
                record.description,
                record.dir_path,
                serde_json::to_string(&record.tags).unwrap_or_else(|_| "[]".to_string()),
                record.content_hash,
                record.mtime,
                record.size,
                record.body,
                now,
            ],
        )
        .map_err(|err| AppError::Io(format!("写入索引失败：{err}")))?;

        skill_count += 1;
    }

    // 全文索引一次性重建：外部内容虚表不认识刚才的 INSERT/UPDATE，
    // 必须显式告诉它"内容表变了，照着重读一遍"。
    // 放在同一个事务里，搜索索引与 skills 表不会出现一个更新了另一个没更新的窗口。
    tx.execute("INSERT INTO skills_fts(skills_fts) VALUES('rebuild')", [])
        .map_err(|err| AppError::Io(format!("重建全文索引失败：{err}")))?;

    tx.commit()
        .map_err(|err| AppError::Io(format!("提交索引事务失败：{err}")))?;

    let duration_ms = started.elapsed().as_millis() as u64;
    tracing::info!(
        skill_count,
        cached_hits,
        parsed,
        skipped = skipped.len(),
        duration_ms,
        "索引重建完成"
    );

    Ok(RebuildReport {
        skill_count,
        cached_hits,
        parsed,
        skipped,
        duration_ms,
    })
}

/// 读取索引中的全部 Skill，按名称排序
pub fn list_all(root: &Path) -> AppResult<Vec<SkillRecord>> {
    let conn = open(root)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, description, dir_path, tags_json, content_hash, mtime, size
             FROM skills ORDER BY name COLLATE NOCASE",
        )
        .map_err(|err| AppError::Io(format!("准备查询失败：{err}")))?;

    let rows = stmt
        .query_map([], |row| {
            let tags_json: Option<String> = row.get(4)?;
            Ok(SkillRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                dir_path: row.get(3)?,
                tags: tags_json
                    .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
                    .unwrap_or_default(),
                content_hash: row.get(5)?,
                mtime: row.get(6)?,
                size: row.get(7)?,
                // 列表不带正文：正文只服务于搜索，见 `SkillRecord::body` 的注释
                body: String::new(),
            })
        })
        .map_err(|err| AppError::Io(format!("查询索引失败：{err}")))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| AppError::Io(format!("读取索引行失败：{err}")))
}

/// 索引中的条目数（用于与磁盘比对）
/// 把现有索引读进内存，供增量比对使用。
///
/// 索引损坏或结构不符时返回空表而不是报错——索引是派生物，
/// 读不出来最多退化为一次全量重建，不该让整个操作失败。
fn load_cache(conn: &Connection) -> std::collections::HashMap<String, SkillRecord> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, name, description, dir_path, tags_json, content_hash, mtime, size, body
           FROM skills",
    ) else {
        return std::collections::HashMap::new();
    };

    let Ok(rows) = stmt.query_map([], |row| {
        let tags_json: Option<String> = row.get(4)?;
        Ok(SkillRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            dir_path: row.get(3)?,
            tags: tags_json
                .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
                .unwrap_or_default(),
            content_hash: row.get(5)?,
            mtime: row.get(6)?,
            size: row.get(7)?,
            body: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
        })
    }) else {
        return std::collections::HashMap::new();
    };

    rows.filter_map(Result::ok)
        .map(|r| (r.dir_path.clone(), r))
        .collect()
}

/// 单次搜索返回的最大命中数
pub const SEARCH_LIMIT: usize = 200;

/// 全文搜索：在 `name` / `description` / 正文里找关键词。
///
/// # 为什么是 `LIKE` 而不是 `MATCH`
///
/// 虚表用的是 trigram 分词器（理由见建表处的注释），而 **trigram 的 MATCH
/// 要求查询至少 3 个字符**——中文里两字词恰恰最常见（"技能""说明"），
/// 用 MATCH 会让它们**静默返回空**。trigram 分词器本来就是为加速
/// `LIKE`/`GLOB` 设计的，所以这里走 `LIKE`：查询够长时吃索引，不够长时
/// 退化成扫描，但两种情况下结果都**正确**。
///
/// 命中片段由 `make_snippet` 在 Rust 侧截取，不用 FTS5 的 `snippet()`：
/// 那个函数只在 `MATCH` 查询下可用。
pub fn search(root: &Path, keywords: &str, limit: usize) -> AppResult<Vec<SearchHit>> {
    let needle = keywords.trim();
    if needle.is_empty() {
        return Ok(Vec::new());
    }

    let conn = open(root)?;
    let pattern = format!("%{}%", escape_like(needle));

    let mut stmt = conn
        .prepare(
            "SELECT f.rowid, f.name, f.description, f.body
               FROM skills_fts f
              WHERE f.name        LIKE ?1 ESCAPE '\\'
                 OR f.description LIKE ?1 ESCAPE '\\'
                 OR f.body        LIKE ?1 ESCAPE '\\'
              LIMIT ?2",
        )
        .map_err(|err| AppError::Io(format!("准备搜索语句失败：{err}")))?;

    let rows = stmt
        .query_map(rusqlite::params![pattern, limit as i64], |row| {
            let name: String = row.get(1)?;
            let description: Option<String> = row.get(2)?;
            let body: Option<String> = row.get(3)?;
            Ok((row.get::<_, i64>(0)?, name, description, body))
        })
        .map_err(|err| AppError::Io(format!("执行搜索失败：{err}")))?;

    let mut hits = Vec::new();
    for row in rows.filter_map(Result::ok) {
        let (rowid, name, description, body) = row;
        let body = body.unwrap_or_default();

        // 片段优先取正文：用户在正文里搜到东西，最需要看到的是那一句上下文。
        // 正文里没有（说明命中的是名称/描述）才退回描述、最后退回名称。
        let snippet = make_snippet(&body, needle)
            .or_else(|| description.as_deref().and_then(|d| make_snippet(d, needle)))
            .unwrap_or_else(|| make_snippet(&name, needle).unwrap_or_else(|| name.clone()));

        // 前端按 id 关联，因此这里必须把 rowid 换回 skill id
        let skill_id: String = conn
            .query_row(
                "SELECT id FROM skills WHERE rowid = ?1",
                rusqlite::params![rowid],
                |r| r.get(0),
            )
            .map_err(|err| AppError::Io(format!("读取命中项失败：{err}")))?;

        hits.push(SearchHit { skill_id, snippet });
    }

    Ok(hits)
}

/// 转义 `LIKE` 的通配符，避免用户输入的 `%`/`_` 被当成模式
fn escape_like(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// 从文本里截出命中处的一段上下文（命中本身尽量居中）
fn make_snippet(text: &str, needle: &str) -> Option<String> {
    const RADIUS: usize = 40;

    if text.trim().is_empty() {
        return None;
    }
    let hit = find_ignore_ascii_case(text, needle)?;
    let chars: Vec<char> = text.chars().collect();
    let hit_char = text[..hit].chars().count();
    let needle_chars = needle.chars().count();

    let start = hit_char.saturating_sub(RADIUS);
    let end = (hit_char + needle_chars + RADIUS).min(chars.len());

    let mut snippet = String::new();
    if start > 0 {
        snippet.push('…');
    }
    snippet.extend(&chars[start..end]);
    if end < chars.len() {
        snippet.push('…');
    }

    // 片段里的换行会把卡片撑乱，压成空格
    Some(snippet.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// 按 ASCII 大小写不敏感找子串，返回**字节**下标。
///
/// 不用 `to_lowercase()` 再比对：那会改变字符串长度，拿到的下标对不上原文。
/// 这里逐字符取等长子串比较，下标天然正确。搜索词都很短，够用。
fn find_ignore_ascii_case(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    let needle_chars = needle.chars().count();

    haystack.char_indices().find_map(|(index, _)| {
        let candidate: String = haystack[index..].chars().take(needle_chars).collect();
        candidate.eq_ignore_ascii_case(needle).then_some(index)
    })
}

pub fn count(root: &Path) -> AppResult<usize> {
    let conn = open(root)?;
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM skills", [], |row| row.get(0))
        .map_err(|err| AppError::Io(format!("统计索引失败：{err}")))?;
    Ok(count as usize)
}

fn find_manifest(skill_dir: &Path) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(skill_dir).ok()?;
    for entry in entries.filter_map(Result::ok) {
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(library::SKILL_MANIFEST)
        {
            return Some(entry.path());
        }
    }
    None
}

/// 把 Skill 名规范化为稳定的 ID：小写、连字符分隔。
///
/// Windows 上文件名大小写不敏感，跨平台时不敏感假设会失效，
/// 因此主键统一规范化，避免 "PDF-Tools" 与 "pdf-tools" 被当成两个 Skill。
fn normalize_id(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_dash = true; // 抑制开头的连字符
    for ch in raw.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "unnamed".to_string()
    } else {
        trimmed
    }
}

/// FNV-1a 64 位。仅用于变更检测，不需要密码学强度，
/// 因此不引入额外依赖，也避免 `DefaultHasher` 跨版本不稳定的问题。
fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn now_millis() -> i64 {
    system_time_millis(std::time::SystemTime::now())
}

fn system_time_millis(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **回归**：索引结构版本落后时必须能升级，且**不能递归**。
    ///
    /// 守着一个真实发生过的崩溃：升级到 v2 后应用**一启动就栈溢出**
    /// （`STATUS_STACK_OVERFLOW`）。原因是当时的迁移代码在版本不匹配时
    /// `DROP TABLE` 之后**递归调用 `migrate`**，却忘了把
    /// `meta.schema_version` 改写成新值——递归进去看到的还是旧版本，
    /// 于是永远不匹配、永远递归下去。
    ///
    /// 这段代码一直没被执行过，因为索引结构从来没变过。
    /// 换句话说：**一条从未被走过的修复路径，第一次被走到就是它崩的时候。**
    #[test]
    fn migrate_upgrades_an_old_schema_without_recursing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("library");
        library::initialize(&root.display().to_string()).unwrap();

        // 造一个"上一版"的库：v1 的表结构 + v1 的版本号，并且里面**有数据**，
        // 这样还能顺带验证"旧数据被丢弃、没有残留到新结构里"
        let db_path = library::index_db_path(&root);
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                INSERT INTO meta (key, value) VALUES ('schema_version', '1');
                CREATE TABLE skills (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT,
                    dir_path TEXT NOT NULL UNIQUE, tags_json TEXT, content_hash TEXT,
                    mtime INTEGER, size INTEGER, updated_at INTEGER NOT NULL
                );
                INSERT INTO skills (id, name, dir_path, updated_at)
                     VALUES ('old', '旧数据', 'D:\\old', 0);
                "#,
            )
            .unwrap();
        }

        // 打开即触发迁移。修好之前，这一行会无限递归到栈溢出。
        let conn = open(&root).unwrap();

        let version: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            version,
            INDEX_SCHEMA_VERSION.to_string(),
            "版本号没有被写回"
        );

        // 旧数据必须被清掉：索引是派生物，重建比"就地改结构"安全得多
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM skills", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 0, "旧结构的数据不该留在新表里");

        // 新结构必须可用：能建索引、能搜
        let report = rebuild(&root).unwrap();
        assert_eq!(report.skill_count, 0);
        assert!(search(&root, "任意词", SEARCH_LIMIT).unwrap().is_empty());
    }

    // =======================================================================
    // 全文搜索
    //
    // 下面几条同时是**分词器选型的依据**：它们断言的行为用 unicode61
    // 是做不到的（实测见建表处的注释）。
    // =======================================================================

    fn search_in(root: &Path, keywords: &str) -> Vec<SearchHit> {
        search(root, keywords, SEARCH_LIMIT).unwrap()
    }

    /// **两字中文查询必须能命中**。
    ///
    /// 这一条就是不用 `unicode61` 的原因：它把整段连续中文当作**一个 token**，
    /// 于是正文里的"中文技能说明"既匹配不上"技能"也匹配不上"中文技能"，
    /// 中文搜索会完全失效。而两字词恰恰是中文里最常见的查询。
    #[test]
    fn search_finds_two_character_chinese_substring() {
        let (_dir, root) = setup_library();
        add_skill(
            &root,
            "alpha",
            "---\nname: alpha\n---\n\n# 中文技能说明\n正文内容\n",
        );
        add_skill(&root, "beta", "---\nname: beta\n---\n\n# 别的东西\n");
        rebuild(&root).unwrap();

        let hits = search_in(&root, "技能");
        assert_eq!(hits.len(), 1, "两字中文查询没命中：{hits:?}");

        assert!(search_in(&root, "中文技能说明").len() == 1);
        assert!(search_in(&root, "说明").len() == 1);
        assert!(search_in(&root, "不存在的词").is_empty());
    }

    /// 英文按**子串**匹配（"tool" 命中 "tools"），这也正是 trigram 的行为
    #[test]
    fn search_finds_latin_substring() {
        let (_dir, root) = setup_library();
        add_skill(&root, "a", "---\nname: a\n---\npdf-tools helper\n");
        rebuild(&root).unwrap();

        assert_eq!(search_in(&root, "tool").len(), 1, "子串匹配失败");
        assert_eq!(search_in(&root, "PDF").len(), 1, "英文应当大小写不敏感");
        assert_eq!(
            search_in(&root, "pdf-tools").len(),
            1,
            "带连字符的整词也应命中"
        );
    }

    /// 命中片段要带上上下文，供前端做高亮
    #[test]
    fn search_returns_snippet_around_the_match() {
        let (_dir, root) = setup_library();
        let body = format!("{}目标词{}", "前".repeat(80), "后".repeat(80));
        add_skill(&root, "a", &format!("---\nname: a\n---\n{body}\n"));
        rebuild(&root).unwrap();

        let hits = search_in(&root, "目标词");
        assert_eq!(hits.len(), 1);
        let snippet = &hits[0].snippet;
        assert!(snippet.contains("目标词"), "片段里没有命中词：{snippet}");
        assert!(
            snippet.starts_with('…') && snippet.ends_with('…'),
            "片段应标明被截断：{snippet}"
        );
        assert!(
            snippet.chars().count() < 150,
            "片段过长：{}",
            snippet.chars().count()
        );
    }

    /// 只有名字命中时，片段退回名称，而不是给个空字符串
    #[test]
    fn search_falls_back_to_name_when_body_has_no_match() {
        let (_dir, root) = setup_library();
        add_skill(
            &root,
            "special-name",
            "---\nname: special-name\n---\n正文里没有那个词\n",
        );
        rebuild(&root).unwrap();

        let hits = search_in(&root, "special-name");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.contains("special-name"), "{:?}", hits[0]);
    }

    /// 关键词里的 `LIKE` 通配符必须被转义，否则 `%` 会匹配到所有 Skill
    #[test]
    fn search_escapes_like_wildcards() {
        let (_dir, root) = setup_library();
        add_skill(&root, "a", "---\nname: a\n---\n普通正文\n");
        add_skill(&root, "b", "---\nname: b\n---\n正文里有 100% 这个词\n");
        rebuild(&root).unwrap();

        assert!(
            search_in(&root, "%").iter().all(|h| h.skill_id == "b"),
            "% 被当成通配符了"
        );
        assert!(search_in(&root, "___").is_empty(), "_ 被当成通配符了");
    }

    /// 空关键词不该被当成"匹配全部"
    #[test]
    fn search_with_blank_keywords_returns_nothing() {
        let (_dir, root) = setup_library();
        add_skill(&root, "a", "---\nname: a\n---\n正文\n");
        rebuild(&root).unwrap();

        assert!(search_in(&root, "   ").is_empty());
        assert!(search_in(&root, "").is_empty());
    }

    /// 文件没改动时重建（走增量缓存）**不能把搜索索引弄丢**。
    ///
    /// 这条守着一个很容易踩空的地方：正文是通过增量缓存复用的，
    /// 若缓存里没有它，第二次重建就会把搜索索引清成空的。
    #[test]
    fn search_still_works_after_a_cached_rebuild() {
        let (_dir, root) = setup_library();
        add_skill(&root, "a", "---\nname: a\n---\n可被搜到的内容\n");

        let first = rebuild(&root).unwrap();
        assert_eq!(first.parsed, 1);
        assert_eq!(search_in(&root, "可被搜到").len(), 1);

        let second = rebuild(&root).unwrap();
        assert_eq!(second.cached_hits, 1, "第二次应当命中增量缓存");
        assert_eq!(second.parsed, 0);
        assert_eq!(
            search_in(&root, "可被搜到").len(),
            1,
            "增量重建把搜索索引弄丢了"
        );
    }

    #[test]
    fn find_ignore_ascii_case_reports_byte_offset() {
        assert_eq!(find_ignore_ascii_case("Hello World", "world"), Some(6));
        assert_eq!(find_ignore_ascii_case("中文abc", "ABC"), Some("中文".len()));
        assert_eq!(find_ignore_ascii_case("abc", "abcd"), None);
        assert_eq!(find_ignore_ascii_case("abc", ""), None);
    }

    fn setup_library() -> (tempfile::TempDir, std::path::PathBuf) {
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

    #[test]
    fn normalize_id_rules() {
        assert_eq!(normalize_id("PDF Tools"), "pdf-tools");
        assert_eq!(normalize_id("code_review"), "code-review");
        assert_eq!(normalize_id("--Edge--"), "edge");
        assert_eq!(normalize_id("多语言 Skill"), "多语言-skill");
        assert_eq!(normalize_id("!!!"), "unnamed");
    }

    #[test]
    fn rebuild_indexes_skills() {
        let (_dir, root) = setup_library();
        add_skill(
            &root,
            "pdf-tools",
            "---\nname: pdf-tools\ndescription: PDF 工具\ntags: [pdf, 文档]\n---\n正文\n",
        );
        add_skill(&root, "code-review", "---\nname: code-review\n---\n");

        let report = rebuild(&root).unwrap();
        assert_eq!(report.skill_count, 2);
        assert!(report.skipped.is_empty());

        let all = list_all(&root).unwrap();
        assert_eq!(all.len(), 2);
        // 按名称升序：code-review 在 pdf-tools 前
        assert_eq!(all[0].name, "code-review");
        assert_eq!(all[1].name, "pdf-tools");
        assert_eq!(all[1].tags, vec!["pdf", "文档"]);
    }

    /// 重建后的计数必须与磁盘上的 SKILL.md 数量一致
    #[test]
    fn rebuild_count_matches_disk() {
        let (_dir, root) = setup_library();
        for i in 0..5 {
            add_skill(
                &root,
                &format!("skill-{i}"),
                &format!("---\nname: skill-{i}\n---\n"),
            );
        }
        // 干扰项：没有 SKILL.md 的目录
        std::fs::create_dir_all(library::skills_dir(&root).join("no-manifest")).unwrap();

        let report = rebuild(&root).unwrap();
        let on_disk = library::count_skills(&root).unwrap();

        assert_eq!(report.skill_count, on_disk);
        assert_eq!(count(&root).unwrap(), on_disk);
        assert_eq!(on_disk, 5);
    }

    /// 增量：内容没变的条目应命中缓存、跳过重新解析
    #[test]
    fn rebuild_reuses_cache_when_unchanged() {
        let (_dir, root) = setup_library();
        add_skill(&root, "a", "---\nname: a\n---\n");
        add_skill(&root, "b", "---\nname: b\n---\n");

        let first = rebuild(&root).unwrap();
        assert_eq!(first.cached_hits, 0, "首次重建不应有缓存命中");
        assert_eq!(first.parsed, 2);

        let second = rebuild(&root).unwrap();
        assert_eq!(second.skill_count, 2);
        assert_eq!(second.cached_hits, 2, "未变更的条目应命中缓存");
        assert_eq!(second.parsed, 0);
    }

    /// 内容变更后必须重新解析，不能沿用旧结果
    #[test]
    fn rebuild_reparses_changed_skill() {
        let (_dir, root) = setup_library();
        add_skill(&root, "a", "---\nname: a\ndescription: 旧\n---\n");
        rebuild(&root).unwrap();

        // 改写内容（长度不同，size 一定变）
        add_skill(&root, "a", "---\nname: a\ndescription: 新描述\n---\n");
        let report = rebuild(&root).unwrap();

        assert_eq!(report.parsed, 1, "内容变更后应重新解析");
        let all = list_all(&root).unwrap();
        assert_eq!(
            all[0].description.as_deref(),
            Some("新描述"),
            "沿用了过期缓存"
        );
    }

    /// 增量不能破坏「磁盘上删掉的 Skill 要从索引消失」
    #[test]
    fn incremental_still_removes_deleted_skills() {
        let (_dir, root) = setup_library();
        add_skill(&root, "keep", "---\nname: keep\n---\n");
        add_skill(&root, "drop", "---\nname: drop\n---\n");
        rebuild(&root).unwrap();

        std::fs::remove_dir_all(library::skills_dir(&root).join("drop")).unwrap();
        let report = rebuild(&root).unwrap();

        assert_eq!(report.skill_count, 1);
        assert_eq!(count(&root).unwrap(), 1);
    }

    #[test]
    fn rebuild_is_idempotent_and_removes_stale_entries() {
        let (_dir, root) = setup_library();
        add_skill(&root, "keep", "---\nname: keep\n---\n");
        add_skill(&root, "remove-me", "---\nname: remove-me\n---\n");
        rebuild(&root).unwrap();
        assert_eq!(count(&root).unwrap(), 2);

        std::fs::remove_dir_all(library::skills_dir(&root).join("remove-me")).unwrap();
        rebuild(&root).unwrap();
        assert_eq!(count(&root).unwrap(), 1, "陈旧条目未被清除");
    }

    #[test]
    fn unparseable_skill_is_reported_not_silently_dropped() {
        let (_dir, root) = setup_library();
        // frontmatter 缺少结束分隔符
        add_skill(&root, "broken", "---\nname: broken\n正文没有结束符\n");

        let report = rebuild(&root).unwrap();
        assert_eq!(report.skill_count, 0);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].dir_name, "broken");
    }

    #[test]
    fn skill_without_name_falls_back_to_dir_name() {
        let (_dir, root) = setup_library();
        add_skill(&root, "my-skill", "---\ndescription: 没有 name 字段\n---\n");

        rebuild(&root).unwrap();
        let all = list_all(&root).unwrap();
        assert_eq!(all[0].name, "my-skill");
        assert_eq!(all[0].description.as_deref(), Some("没有 name 字段"));
    }

    #[test]
    fn manifest_filename_is_case_insensitive() {
        let (_dir, root) = setup_library();
        let skill_dir = library::skills_dir(&root).join("case-test");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("Skill.md"), "---\nname: case-test\n---\n").unwrap();

        rebuild(&root).unwrap();
        assert_eq!(count(&root).unwrap(), 1);
    }

    #[test]
    fn rebuild_on_uninitialized_library_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(rebuild(dir.path()).is_err());
    }
}
