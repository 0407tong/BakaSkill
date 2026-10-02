/**
 * 前端筛选/排序的性能基准（1000 条）。
 *
 * 用法：
 *   node --experimental-strip-types scripts/bench-filter.mts
 *
 * `skillFilter.ts` 只做类型导入，运行时无依赖，因此可以直接被 Node 加载。
 * 这让我们能在不启动界面的情况下量出 A2.4 的数字。
 */

import { applyFilters } from "../src/lib/skillFilter.ts";

interface FakeSkill {
  id: string;
  name: string;
  description: string;
  tags: string[];
  inCentralLibrary: boolean;
  centralPath: string | null;
  instances: never[];
  managedCount: number;
  updatedAt: number;
}

function makeSkills(count: number): FakeSkill[] {
  const tagPool = ["pdf", "文档", "工程", "review", "自动化", "数据", "写作"];
  return Array.from({ length: count }, (_, i) => ({
    id: `perf-skill-${String(i).padStart(4, "0")}`,
    name: `perf-skill-${String(i).padStart(4, "0")}`,
    description:
      i % 5 === 0
        ? "这是一段刻意写得很长的描述，用于验证卡片截断行为并让筛选更接近真实场景"
        : `基准 Skill 第 ${i} 号`,
    tags: tagPool.slice(0, i % 4),
    inCentralLibrary: i % 3 === 0,
    centralPath: i % 3 === 0 ? `D:\\Lib\\skills\\perf-skill-${i}` : null,
    instances: [],
    managedCount: i % 4,
    updatedAt: 1_700_000_000_000 + i * 1000,
  }));
}

function time(label: string, iterations: number, fn: () => void) {
  // 预热，避免把 JIT 编译时间算进去
  for (let i = 0; i < 3; i += 1) fn();

  const started = performance.now();
  for (let i = 0; i < iterations; i += 1) fn();
  const perRun = (performance.now() - started) / iterations;

  console.log(`${label}: ${perRun.toFixed(2)}ms  （${iterations} 次平均）`);
  return perRun;
}

const COUNT = 1000;
const skills = makeSkills(COUNT) as never[];

console.log(`数据规模：${COUNT} 条\n`);

const results: Record<string, number> = {};

results["无筛选（仅排序）"] = time("无筛选（仅排序）", 50, () => {
  applyFilters(skills, { search: "", sourceFilter: "all", sort: "name" });
});

results["按名称搜索命中少量"] = time("按名称搜索命中少量", 50, () => {
  applyFilters(skills, { search: "perf-skill-0999", sourceFilter: "all", sort: "name" });
});

results["按描述全文搜索"] = time("按描述全文搜索", 50, () => {
  applyFilters(skills, { search: "基准", sourceFilter: "all", sort: "name" });
});

results["搜索 + 来源筛选"] = time("搜索 + 来源筛选", 50, () => {
  applyFilters(skills, {
    search: "perf",
    sourceFilter: "central",
    sort: "updated",
  });
});

console.log("");
const worst = Math.max(...Object.values(results));
console.log(`最慢一项：${worst.toFixed(2)}ms`);
console.log(
  worst < 100
    ? `✅ 满足 A2.4（1000 条下 < 100ms）`
    : `❌ 超出 A2.4 阈值 100ms`,
);
