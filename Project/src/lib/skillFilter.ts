import type { SkillSummary } from "@/types/ipc";
import type { SortKey, SourceFilter } from "@/store/skillStore";

/** `is:` 语法糖支持的取值 */
export type IsFlag = "enabled" | "external" | "dangling";

export const IS_FLAGS: readonly IsFlag[] = ["enabled", "external", "dangling"];

export interface ParsedQuery {
  /**
   * 全文关键词（已剔除语法糖）。
   *
   * **这一份才是要发给后端做全文检索的**：语法糖的筛选条件都基于前端已有的
   * 扫描数据（标签、实例、链接状态），没有必要为了它们多跑一趟后端——
   * 而且 `is:external` 筛选的正是**不在索引里**的那些 Skill，
   * 后端根本无从判断。
   */
  keywords: string;
  tags: string[];
  agents: string[];
  flags: IsFlag[];
  /** 写法不完整的片段（如光秃秃的 `tag:`），用于提示用户而不是静默忽略 */
  malformed: string[];
}

/**
 * 解析搜索框里的一行输入。
 *
 * 语法：`tag:xxx`、`agent:claude-code`、`is:enabled`（可写多个），
 * 其余一律当作全文关键词。值可以用双引号包起来以容纳空格：
 * `tag:"code review"`。
 */
export function parseQuery(raw: string): ParsedQuery {
  const result: ParsedQuery = {
    keywords: "",
    tags: [],
    agents: [],
    flags: [],
    malformed: [],
  };
  const words: string[] = [];

  for (const token of tokenize(raw)) {
    const colon = token.indexOf(":");
    if (colon <= 0) {
      words.push(token);
      continue;
    }

    const field = token.slice(0, colon).toLowerCase();
    const value = unquote(token.slice(colon + 1));

    switch (field) {
      case "tag":
        if (value) result.tags.push(value.toLowerCase());
        else result.malformed.push(token);
        break;
      case "agent":
        if (value) result.agents.push(value.toLowerCase());
        else result.malformed.push(token);
        break;
      case "is": {
        const flag = value.toLowerCase() as IsFlag;
        if (IS_FLAGS.includes(flag)) result.flags.push(flag);
        else result.malformed.push(token);
        break;
      }
      default:
        // 不是已知的语法糖 → 按普通关键词处理。用户搜 "http://..." 这类
        // 带冒号的文本时不该被吞掉。
        words.push(token);
    }
  }

  result.keywords = words.join(" ").trim();
  return result;
}

/** 按空白切分，但双引号内的空白不算分隔符 */
function tokenize(raw: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let quoted = false;

  for (const char of raw.trim()) {
    if (char === '"') {
      quoted = !quoted;
      current += char;
    } else if (!quoted && /\s/.test(char)) {
      if (current) tokens.push(current);
      current = "";
    } else {
      current += char;
    }
  }
  if (current) tokens.push(current);
  return tokens;
}

/** 去掉包裹的双引号（只去成对的、位于首尾的那种） */
function unquote(value: string): string {
  const trimmed = value.trim();
  if (trimmed.length >= 2 && trimmed.startsWith('"') && trimmed.endsWith('"')) {
    return trimmed.slice(1, -1).trim();
  }
  return trimmed;
}

export interface FilterOptions {
  search: string;
  sourceFilter: SourceFilter;
  sort: SortKey;
  /**
   * 是否显示 **Agent 自带的 Skill**（Codex 的 `.system` 分组等）。**缺省为不显示**。
   *
   * 写成可选，是为了让只关心性能的调用方（`scripts/bench-filter.mts`）不必跟着改。
   */
  showSystemSkills?: boolean;
  /**
   * 全文检索命中结果：`skillId -> 正文片段`。
   *
   * `null` 表示**尚未拿到**（还没搜、或搜索失败）——此时退回本地子串匹配。
   * 用一个显式的 `null` 而不是空 Map 来区分这两种情况很重要：
   * 空 Map 意味着"后端说一条都没命中"，那时不该再拿本地匹配把结果放回来。
   */
  fullText?: ReadonlyMap<string, string> | null;
}

export interface FilterResult {
  skills: SkillSummary[];
  /** 本次结果中每个 Skill 的正文片段（只有全文命中才有） */
  snippets: ReadonlyMap<string, string>;
  parsed: ParsedQuery;
}

/**
 * 搜索、筛选、排序 Skill 清单。
 *
 * 抽成纯函数（而不是写在组件里）有三个原因：便于直接测、
 * 能在不启动界面的情况下直接量性能，以及**筛选规则集中在一处**——
 * 语法糖、来源筛选、排序三者的组合关系只在这里定义一次。
 */
export function applyFilters(
  skills: readonly SkillSummary[],
  options: FilterOptions,
): FilterResult {
  const parsed = parseQuery(options.search);
  const fullText = options.fullText ?? null;
  const hasQuery =
    parsed.keywords.length > 0 ||
    parsed.tags.length > 0 ||
    parsed.agents.length > 0 ||
    parsed.flags.length > 0;

  const showSystem = options.showSystemSkills ?? false;

  const snippets = new Map<string, string>();
  const filtered = skills.filter((skill) => {
    if (!showSystem && isPureSystemSkill(skill)) return false;
    if (!matchesSource(skill, options.sourceFilter)) return false;
    if (!hasQuery) return true;

    if (!matchesTags(skill, parsed.tags)) return false;
    if (!matchesAgents(skill, parsed.agents)) return false;
    if (!matchesFlags(skill, parsed.flags)) return false;

    if (!parsed.keywords) return true;

    // 全文部分：
    // - 后端给了结果 → 以它为准，同时记下片段
    // - 后端还没回 → 退回本地子串匹配（保证输入时不会先空一下）
    // - Skill 不在中央库里（索引里没有它）→ 只能本地匹配名称与描述，
    //   它的正文压根没有被索引过
    if (fullText) {
      const snippet = fullText.get(skill.id);
      if (snippet !== undefined) {
        if (snippet) snippets.set(skill.id, snippet);
        return true;
      }
      return !skill.inCentralLibrary && matchesLocally(skill, parsed.keywords);
    }
    return matchesLocally(skill, parsed.keywords);
  });

  const sorted = [...filtered];
  switch (options.sort) {
    case "updated":
      // 无时间戳的排在最后，而不是被当成 0 排到最前
      sorted.sort((a, b) => (b.updatedAt ?? -1) - (a.updatedAt ?? -1));
      break;
    case "managed":
      sorted.sort(
        (a, b) =>
          b.managedCount - a.managedCount || a.name.localeCompare(b.name, "zh"),
      );
      break;
    case "name":
    default:
      sorted.sort((a, b) => a.name.localeCompare(b.name, "zh"));
      break;
  }

  return { skills: sorted, snippets, parsed };
}

/** 本地子串匹配：名称、描述、id、标签 */
function matchesLocally(skill: SkillSummary, keywords: string): boolean {
  // 多个关键词按「都要命中」处理，与后端 FTS 的隐式 AND 保持一致
  return keywords
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every(
      (word) =>
        skill.name.toLowerCase().includes(word) ||
        (skill.description?.toLowerCase().includes(word) ?? false) ||
        skill.id.includes(word) ||
        skill.tags.some((t) => t.toLowerCase().includes(word)),
    );
}

function matchesTags(skill: SkillSummary, tags: readonly string[]): boolean {
  if (tags.length === 0) return true;
  const own = skill.tags.map((t) => t.toLowerCase());
  // 多个 tag: 之间是「或」：标签是分类维度，要求同时具备多个标签
  // 在筛选场景里几乎总是过窄，用户想表达的是"看这几类里的"。
  return tags.some((t) => own.includes(t));
}

function matchesAgents(
  skill: SkillSummary,
  agents: readonly string[],
): boolean {
  if (agents.length === 0) return true;
  return skill.instances.some((i) => agents.includes(i.agentId.toLowerCase()));
}

function matchesFlags(skill: SkillSummary, flags: readonly IsFlag[]): boolean {
  return flags.every((flag) => {
    switch (flag) {
      case "enabled":
        // 「已启用」= 至少在一个 Agent 上处于已接入状态
        return skill.managedCount > 0;
      case "external":
        return !skill.inCentralLibrary;
      case "dangling":
        return skill.instances.some((i) => i.kind === "dangling");
      default:
        return true;
    }
  });
}

/**
 * 这是不是一个"**纯粹是 Agent 自带**"的 Skill。
 *
 * 判定是 **所有实例都属于 Agent 的内置分组**，而不是"存在某个内置实例"。
 * 差别很关键：同一个 Skill 完全可能既在中央库里、又恰好是 Codex 的内置项，
 * 或者用户在别处也放了一份——那种是**用户自己的东西**，不该被藏起来。
 * 一句话：只要它有一处不是自带的，它就是用户的。
 *
 * `inCentralLibrary` 单独再判一次：**在中央库里有实体，就永远是用户的**。
 */
export function isPureSystemSkill(skill: SkillSummary): boolean {
  if (skill.inCentralLibrary) return false;
  return (
    skill.instances.length > 0 && skill.instances.every((i) => i.isSystemGroup)
  );
}

function matchesSource(skill: SkillSummary, filter: SourceFilter): boolean {
  switch (filter) {
    case "all":
      return true;
    case "central":
      return skill.inCentralLibrary;
    case "external":
      return skill.instances.some((i) => i.kind === "external");
    case "dangling":
      return skill.instances.some((i) => i.kind === "dangling");
    default:
      // 其余取值按 Agent id 处理
      return skill.instances.some((i) => i.agentId === filter);
  }
}
