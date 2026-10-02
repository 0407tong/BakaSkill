//! 扫描与索引的性能基准。
//!
//! 默认被 `#[ignore]` 跳过（生成 1000 个目录会拖慢常规测试），
//! 需要时显式运行：
//!
//! ```text
//! cargo test --release --test perf_scan -- --ignored --nocapture
//! ```
//!
//! 结果登记在 `docs/PERF.md`。

use std::path::Path;
use std::time::Instant;

use bakaskill_lib::agents::{AgentDetection, AgentStatus};
use bakaskill_lib::library;
use bakaskill_lib::scanner;

const SKILL_COUNT: usize = 1000;

fn write_fixture_skills(root: &Path) {
    let skills = library::skills_dir(root);
    std::fs::create_dir_all(&skills).unwrap();

    for i in 1..=SKILL_COUNT {
        let name = format!("perf-skill-{i:04}");
        let dir = skills.join(&name);
        std::fs::create_dir_all(&dir).unwrap();

        // 约 1/5 用长描述，标签数量 0~5，避免用等长假数据测出偏乐观的结果
        let description = if i % 5 == 0 {
            "这是一段刻意写得很长的描述，用于验证卡片截断行为，并让索引占用更接近真实场景"
                .to_string()
        } else {
            format!("基准 Skill 第 {i} 号")
        };
        let tags: Vec<String> = (0..i % 6).map(|t| format!("tag-{t}")).collect();
        let tags_block = if tags.is_empty() {
            String::new()
        } else {
            format!(
                "tags:\n{}\n",
                tags.iter()
                    .map(|t| format!("  - {t}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };

        let content = format!(
            "---\nname: {name}\ndescription: {description}\n{tags_block}---\n\n# {name}\n\n{body}",
            body = fixture_body(i)
        );
        std::fs::write(dir.join(library::SKILL_MANIFEST), content).unwrap();
    }
}

/// 一段接近真实 SKILL.md 长度的正文（约 1.5 KB）。
///
/// 刻意写得有内容：全文搜索的耗时**强烈依赖正文长度**，用二十个字节的假正文
/// 量出来的数字会好看到没有意义。里面同时埋了三类可搜索文本——
/// 中文短语、英文子串、以及每个 Skill 独有的标记，分别用来量不同查询形态。
fn fixture_body(index: usize) -> String {
    let paragraph = "本段用于撑起正文长度。真实场景里的 SKILL.md 往往包含用法说明、\
                     参数表与示例，长度在千字节量级。搜索引擎的代价与这个长度直接相关，\
                     因此基准数据必须尽可能接近它，否则量出来的数字只是自欺。\n";
    let mut body = String::with_capacity(1600);
    body.push_str("性能基准正文。\n\n");
    for _ in 0..6 {
        body.push_str(paragraph);
    }
    body.push_str(&format!("\n统一标记：基准检索词。\n英文说明：helper utility toolkit。\n专属标记：marker-{index:04}\n"));
    body
}

fn detection_for(id: &str, dir: &Path) -> AgentDetection {
    AgentDetection {
        id: id.to_string(),
        display_name: id.to_string(),
        icon: None,
        status: AgentStatus::Detected,
        skill_dir: Some(dir.display().to_string()),
        detected_by: Vec::new(),
        is_user_override: false,
        notes: None,
        docs_url: None,
    }
}

#[test]
#[ignore = "性能基准，按需运行"]
fn scan_and_index_1000_skills() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("library");
    library::initialize(&root.display().to_string()).unwrap();

    let gen_started = Instant::now();
    write_fixture_skills(&root);
    println!(
        "生成 {SKILL_COUNT} 个 Skill 耗时 {:?}",
        gen_started.elapsed()
    );

    // 冷启动：索引为空，需要全量解析
    let cold = Instant::now();
    let report = bakaskill_lib::index::rebuild(&root).unwrap();
    let cold_ms = cold.elapsed().as_millis();
    assert_eq!(report.skill_count, SKILL_COUNT);
    println!("索引全量重建（冷）: {cold_ms}ms");

    // 读取索引
    let read = Instant::now();
    let list = bakaskill_lib::index::list_all(&root).unwrap();
    let read_ms = read.elapsed().as_millis();
    assert_eq!(list.len(), SKILL_COUNT);
    println!("索引读取 {SKILL_COUNT} 条: {read_ms}ms");

    // 扫描一个包含全部 1000 个 Skill 的「Agent 目录」
    let agent = detection_for("perf-agent", &library::skills_dir(&root));
    let scan_started = Instant::now();
    let scan = scanner::scan(&[agent], Some(&root)).unwrap();
    let scan_ms = scan_started.elapsed().as_millis();
    assert_eq!(scan.skills.len(), SKILL_COUNT);
    println!("扫描 + 归并 {SKILL_COUNT} 个 Skill: {scan_ms}ms");
    println!("  扫描目录数: {}", scan.scanned_dirs);
    println!("  跳过: {}", scan.skipped.len());

    // 性能阈值见 docs/PERF.md：列表首屏 < 1s
    assert!(scan_ms < 1000, "扫描耗时 {scan_ms}ms 超出 1s 目标");

    // ---- 全文搜索（1000 条下 < 100ms）----
    //
    // 三种查询形态分别量，因为它们走的是不同的执行路径：
    //   ① 两字中文：短于 trigram 的 3 字符门槛 → 退化成内容表扫描
    //   ② 长中文短语：能吃 trigram 索引
    //   ③ 英文子串：同样吃索引，且能顺带确认大小写不敏感不会额外变慢
    //   ④ 命中全部 1000 条：最坏情况，结果数被封顶但扫描范围不变
    for (label, query) in [
        ("两字中文（退化为扫描）", "基准"),
        ("长中文短语（走索引）", "基准检索词"),
        ("英文子串（走索引）", "toolkit"),
        ("专属标记（单条命中）", "marker-0500"),
        ("命中全部 1000 条", "本段用于撑起正文长度"),
    ] {
        let started = Instant::now();
        let hits =
            bakaskill_lib::index::search(&root, query, bakaskill_lib::index::SEARCH_LIMIT).unwrap();
        let ms = started.elapsed().as_millis();
        println!(
            "全文搜索「{query}」（{label}）: {ms}ms，命中 {} 条",
            hits.len()
        );
        assert!(ms < 100, "搜索「{query}」耗时 {ms}ms 超出 100ms 目标");
    }
}
