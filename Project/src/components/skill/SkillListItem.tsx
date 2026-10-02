import { AlertTriangle, FileText } from "lucide-react";

import { cn } from "@/lib/cn";
import { Badge } from "@/components/ui/Badge";
import { SkillBadges } from "@/components/skill/SkillBadges";
import { SkillToggle } from "@/components/skill/SkillToggle";
import { Highlight } from "@/components/skill/Highlight";
import { formatTimestamp } from "@/lib/format";
import type { SkillSummary } from "@/types/ipc";

export interface SkillListItemProps {
  skill: SkillSummary;
  selected: boolean;
  checked: boolean;
  onSelect: (id: string) => void;
  onToggleChecked: (id: string) => void;
  /** 全文搜索命中时的正文片段；有则**替代**描述显示 */
  snippet?: string;
  /** 用于高亮的关键词 */
  keywords?: string;
}

/** 列表视图中的单行。信息密度高于卡片，用于快速扫读大量 Skill。 */
export function SkillListItem({
  skill,
  selected,
  checked,
  onSelect,
  onToggleChecked,
  snippet,
  keywords,
}: SkillListItemProps) {
  const hasParseError = skill.instances.some((i) => i.parseError);

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
        // overflow-hidden 与网格卡片同理：虚拟化的固定行高一旦被内容撑破，
        // 溢出的部分会盖住下一行（行是绝对定位的，不会互相让位）。
        "group/row flex cursor-pointer items-center gap-3 overflow-hidden border-b border-border px-3 py-2",
        "transition-colors outline-none",
        "focus-visible:bg-surface-hover",
        // sh-row：铺了背景图时给行加上与网格卡片同款的半透明底（见 globals.css）。
        // 行本来只有一条下边框、不带背景，图模式下会完全透出插画而看不清；
        // 选中态另用 sh-row-selected，因为它在图模式下要盖过那层底。
        "sh-row",
        selected
          ? "sh-row-selected bg-accent-subtle/40"
          : "hover:bg-surface-hover",
      )}
    >
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
          "size-3.5 shrink-0 cursor-pointer accent-[var(--sh-accent)]",
          "opacity-0 transition-opacity group-hover/row:opacity-100",
          checked && "opacity-100",
          "focus-visible:opacity-100",
        )}
      />

      <FileText
        className={cn(
          "size-4 shrink-0",
          hasParseError ? "text-warning" : "text-fg-subtle",
        )}
      />

      <div className="flex min-w-0 flex-[2] items-baseline gap-2">
        <span
          className="truncate text-sm font-medium text-fg"
          title={skill.name}
        >
          <Highlight text={skill.name} keywords={keywords ?? ""} />
        </span>
        <span className="shrink-0 font-mono text-[11px] text-fg-subtle">
          {skill.id}
        </span>
      </div>

      {/* 搜索时显示正文片段（那才是"为什么命中"），平时显示描述 */}
      <div className="min-w-0 flex-[3] truncate text-xs text-fg-muted">
        {snippet ? (
          <Highlight text={snippet} keywords={keywords ?? ""} />
        ) : (
          (skill.description ?? (
            <span className="text-fg-subtle">（无描述）</span>
          ))
        )}
      </div>

      <div className="flex shrink-0 items-center gap-1">
        {skill.tags.slice(0, 2).map((tag) => (
          <Badge key={tag} variant="default">
            {tag}
          </Badge>
        ))}
      </div>

      <div className="flex w-32 shrink-0 items-center gap-1">
        <SkillBadges skill={skill} />
      </div>

      <span className="w-28 shrink-0 text-right text-[11px] text-fg-subtle">
        {skill.updatedAt ? formatTimestamp(skill.updatedAt) : "—"}
      </span>

      <SkillToggle skill={skill} />

      {hasParseError ? (
        <AlertTriangle
          className="size-3.5 shrink-0 text-warning"
          aria-label="SKILL.md 解析异常"
        />
      ) : null}
    </div>
  );
}
