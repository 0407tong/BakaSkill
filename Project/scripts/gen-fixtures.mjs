#!/usr/bin/env node
/**
 * 生成用于性能基准的假 Skill。
 *
 * 用法：
 *   node scripts/gen-fixtures.mjs <目标中央库路径> [数量]
 *
 * 例：
 *   node scripts/gen-fixtures.mjs D:\PerfLibrary 1000
 *
 * 生成的内容刻意包含长短不一的描述与数量不等的标签，
 * 避免用"全部等长"的假数据测出偏乐观的结果。
 */

import { mkdir, writeFile, rm } from "node:fs/promises";
import { existsSync } from "node:fs";
import { join } from "node:path";

const SHORT_DESCRIPTIONS = [
  "处理 PDF",
  "代码评审",
  "写文档",
  "数据清洗",
  "接口调试",
];

const LONG_DESCRIPTION =
  "这是一个描述相当长的 Skill，用来验证卡片在描述很长时的截断行为是否正确，" +
  "同时也会占用更多索引空间，让性能基准更接近真实使用场景而不是理想情况。";

const TAG_POOL = [
  "pdf",
  "文档",
  "工程",
  "review",
  "自动化",
  "数据",
  "写作",
  "前端",
  "后端",
  "运维",
];

/** 简单的确定性伪随机，保证每次生成同样的数据，便于对比基准 */
function makeRandom(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0xffffffff;
  };
}

function buildSkillMarkdown(index, random) {
  const name = `perf-skill-${String(index).padStart(4, "0")}`;

  // 约 1/5 用长描述，其余用短描述
  const useLong = random() < 0.2;
  const description = useLong
    ? LONG_DESCRIPTION
    : `${SHORT_DESCRIPTIONS[index % SHORT_DESCRIPTIONS.length]}（第 ${index} 号）`;

  // 标签数量 0~5 个
  const tagCount = Math.floor(random() * 6);
  const tags = [];
  for (let i = 0; i < tagCount; i += 1) {
    const tag = TAG_POOL[Math.floor(random() * TAG_POOL.length)];
    if (!tags.includes(tag)) tags.push(tag);
  }

  const tagsBlock =
    tags.length > 0
      ? `tags:\n${tags.map((t) => `  - ${t}`).join("\n")}\n`
      : "";

  return `---
name: ${name}
description: ${description}
${tagsBlock}---

# ${name}

这是用于性能基准的假 Skill，编号 ${index}。

## 正文

正文长度也会影响全文索引的大小，因此这里放一段有实际长度的文字，
而不是一行占位符。重复的正文内容能在一定程度上模拟真实 Skill 的索引开销。
`;
}

async function main() {
  const [, , targetArg, countArg] = process.argv;
  const target = targetArg;
  const count = Number.parseInt(countArg ?? "1000", 10);

  if (!target) {
    console.error("用法：node scripts/gen-fixtures.mjs <目标中央库路径> [数量]");
    process.exit(1);
  }
  if (!Number.isFinite(count) || count <= 0) {
    console.error("数量必须是正整数");
    process.exit(1);
  }

  const skillsDir = join(target, "skills");
  const random = makeRandom(20261001);

  console.log(`目标：${skillsDir}`);
  console.log(`数量：${count}`);

  // 先清掉上一次基准生成的目录，避免累加
  if (existsSync(skillsDir)) {
    const entries = await import("node:fs/promises").then((fs) =>
      fs.readdir(skillsDir),
    );
    const stale = entries.filter((e) => e.startsWith("perf-skill-"));
    for (const name of stale) {
      await rm(join(skillsDir, name), { recursive: true, force: true });
    }
    console.log(`清理了 ${stale.length} 个旧基准目录`);
  }

  await mkdir(skillsDir, { recursive: true });

  const started = Date.now();
  for (let i = 1; i <= count; i += 1) {
    const name = `perf-skill-${String(i).padStart(4, "0")}`;
    const dir = join(skillsDir, name);
    await mkdir(dir, { recursive: true });
    await writeFile(join(dir, "SKILL.md"), buildSkillMarkdown(i, random), "utf8");
  }
  const elapsed = Date.now() - started;

  console.log(`已生成 ${count} 个 Skill，耗时 ${elapsed}ms`);
  console.log("接下来：在 SkillHub 中把中央库指向该路径并点「重建索引」");
}

main().catch((err) => {
  console.error("生成失败：", err);
  process.exit(1);
});
