import { useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";

import { SkillListItem } from "@/components/skill/SkillListItem";
import { useSkillStore } from "@/store/skillStore";
import type { SkillSummary } from "@/types/ipc";

/** 首次渲染的行高估值，真实值由 measureElement 实测 */
const ESTIMATED_ROW_HEIGHT = 45;

export interface SkillListProps {
  skills: readonly SkillSummary[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  /** 全文搜索命中：skillId -> 正文片段 */
  snippets?: ReadonlyMap<string, string>;
  /** 用于高亮的关键词 */
  keywords?: string;
}

/**
 * 列表视图（虚拟化）。
 *
 * 与网格一致地使用**动态测量行高**：虽然当前行内各字段都是 `truncate`
 * 的单行显示、行高应当稳定，但"应当稳定"不是保障。测量让行高始终与
 * 实际内容一致，避免未来给行内加内容时重蹈"溢出盖住下一行"的覆辙。
 */
export function SkillList({
  skills,
  selectedId,
  onSelect,
  snippets,
  keywords,
}: SkillListProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const checkedIds = useSkillStore((s) => s.selectedIds);
  const toggleChecked = useSkillStore((s) => s.toggleSelected);

  const virtualizer = useVirtualizer({
    count: skills.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ESTIMATED_ROW_HEIGHT,
    measureElement: (el) => el.getBoundingClientRect().height,
    overscan: 8,
  });

  return (
    <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto">
      <div
        className="relative w-full"
        style={{ height: virtualizer.getTotalSize() }}
      >
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const skill = skills[virtualRow.index];
          if (!skill) return null;

          return (
            <div
              key={virtualRow.key}
              data-index={virtualRow.index}
              ref={virtualizer.measureElement}
              className="absolute top-0 left-0 w-full"
              style={{ transform: `translateY(${virtualRow.start}px)` }}
            >
              <SkillListItem
                skill={skill}
                selected={skill.id === selectedId}
                checked={checkedIds.includes(skill.id)}
                onSelect={onSelect}
                onToggleChecked={toggleChecked}
                snippet={snippets?.get(skill.id)}
                keywords={keywords}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}
