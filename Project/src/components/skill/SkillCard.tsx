import { AlertTriangle, FileText } from "lucide-react";

import { cn } from "@/lib/cn";
import { Badge } from "@/components/ui/Badge";
import { SkillBadges } from "@/components/skill/SkillBadges";
import { SkillToggle } from "@/components/skill/SkillToggle";
import { Highlight } from "@/components/skill/Highlight";
import { formatTimestamp } from "@/lib/format";
import type { SkillSummary } from "@/types/ipc";

export interface SkillCardProps {
  skill: SkillSummary;
  selected: boolean;
  /** 是否被勾选参与批量操作（与 `selected` 是两件事） */
  checked: boolean;
  onSelect: (id: string) => void;
  onToggleChecked: (id: string) => void;
  /** 全文搜索命中时的正文片段；有则**替代**描述显示 */
  snippet?: string;
  /** 用于高亮的关键词 */
  keywords?: string;
}

/**
 * 网格视图中的单个 Skill 卡片。
 *
 * 卡片上并排放三样各司其职的信息：来源徽章（在哪）、启停开关（状态）、
 * 更新时间的次要信息。**必须显示来源**——只显示名称的话，用户无法区分
 * 中央库中的 Skill 与 Agent 目录里的外部副本，这正是链接透明化要解决的问题。
 */
export function SkillCard({
  skill,
  selected,
  checked,
  onSelect,
  onToggleChecked,
  snippet,
  keywords,
}: SkillCardProps) {
  const hasProblem = skill.instances.some(
    (i) => i.kind === "dangling" || i.kind === "externalDuplicate",
  );

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => onSelect(skill.id)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelect(skill.id);
        }
      }}
      className={cn(
        // **不设固定高度**：卡片高度由内容决定，虚拟化通过 measureElement
        // 实测行高。写成固定高度会让"选中后多出的徽章换行"把内容挤出去，
        // 而绝对定位的行不会给溢出让位，结果是盖住下一行。
        //
        // min-w-0：网格项默认 min-width:auto，内容（如长路径）能把单元撑宽。
        "group flex min-w-0 cursor-pointer flex-col gap-2 overflow-hidden rounded-lg border bg-surface p-3",
        "transition-colors outline-none",
        "focus-visible:border-accent",
        // 选中态必须带 `sh-card-selected`：`bg-accent-subtle/40` 是**半透明**的，
        // 它会替换掉本来不透明的 `bg-surface`。铺了背景图时（图模式下面板本来就
        // 半透明），卡片里的字就直接压在插画上，看不清——用户报的正是这个。
        // `globals.css` 里给列表行补过同一条规则（`.sh-row-selected`），卡片漏了。
        selected
          ? "sh-card-selected border-accent bg-accent-subtle/40"
          : "border-border hover:border-border-strong hover:bg-surface-hover",
        checked && !selected && "border-accent/50",
      )}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="flex min-w-0 items-start gap-2">
          {/* 勾选框：默认隐藏，悬停或已勾选时出现。
              始终显示会让卡片看起来很吵；hover 才出现又不好发现，
              因此已勾选时也保持可见作为反馈。 */}
          <input
            type="checkbox"
            checked={checked}
            onChange={(e) => {
              e.stopPropagation();
              onToggleChecked(skill.id);
            }}
            onClick={(e) => e.stopPropagation()}
            aria-label={`选择 ${skill.name} 以进行批量操作`}
            className={cn(
              "mt-0.5 size-3.5 shrink-0 cursor-pointer accent-[var(--sh-accent)]",
              "opacity-0 transition-opacity group-hover:opacity-100",
              checked && "opacity-100",
              "focus-visible:opacity-100",
            )}
          />
          <FileText
            className={cn(
              "mt-0.5 size-4 shrink-0",
              hasProblem ? "text-warning" : "text-fg-subtle",
            )}
          />
          <div className="min-w-0">
            <div
              className="truncate text-sm font-medium text-fg"
              title={skill.name}
            >
              <Highlight text={skill.name} keywords={keywords ?? ""} />
            </div>
            <div className="truncate font-mono text-[11px] text-fg-subtle">
              {skill.id}
            </div>
          </div>
        </div>

        <SkillToggle skill={skill} />
      </div>

      {/* 搜索时显示正文片段（那才是"为什么命中"），平时显示描述 */}
      <p className="line-clamp-2 min-h-8 text-xs leading-4 text-fg-muted">
        {snippet ? (
          <Highlight text={snippet} keywords={keywords ?? ""} />
        ) : (
          (skill.description ?? (
            <span className="text-fg-subtle">（无描述）</span>
          ))
        )}
      </p>

      {skill.tags.length > 0 ? (
        <div className="flex flex-wrap gap-1">
          {skill.tags.slice(0, 4).map((tag) => (
            <Badge key={tag} variant="default">
              {tag}
            </Badge>
          ))}
          {skill.tags.length > 4 ? (
            <Badge variant="outline">+{skill.tags.length - 4}</Badge>
          ) : null}
        </div>
      ) : null}

      <div className="mt-auto flex items-center justify-between gap-2 pt-1">
        <SkillBadges skill={skill} />
        {skill.updatedAt ? (
          <span className="shrink-0 text-[11px] text-fg-subtle">
            {formatTimestamp(skill.updatedAt)}
          </span>
        ) : null}
      </div>

      {skill.instances.some((i) => i.parseError) ? (
        <div className="flex items-start gap-1 text-[11px] text-warning">
          <AlertTriangle className="mt-0.5 size-3 shrink-0" />
          <span className="line-clamp-2">
            SKILL.md 解析异常，元数据可能不完整
          </span>
        </div>
      ) : null}

      {skill.inCentralLibrary && skill.centralPath ? (
        <div
          className="truncate font-mono text-[10px] text-fg-subtle"
          title={skill.centralPath}
        >
          {skill.centralPath}
        </div>
      ) : null}
    </div>
  );
}
