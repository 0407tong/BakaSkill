import { useEffect, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";

import { SkillCard } from "@/components/skill/SkillCard";
import { useSkillStore } from "@/store/skillStore";
import type { SkillSummary } from "@/types/ipc";

const ROW_GAP = 12;
/** 首次渲染时的行高估值，真实行高由 measureElement 实测后覆盖 */
const ESTIMATED_CARD_HEIGHT = 208;
/**
 * 每列的最小宽度，决定窗口宽度变化时排几列。
 * 取 240 而非更大值：详情面板开合会改变主区宽度，阈值太大时列数会突变。
 */
const MIN_COLUMN_WIDTH = 240;

export interface SkillGridProps {
  skills: readonly SkillSummary[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  /** 全文搜索命中：skillId -> 正文片段 */
  snippets?: ReadonlyMap<string, string>;
  /** 用于高亮的关键词 */
  keywords?: string;
}

/**
 * 网格视图（虚拟化）。
 *
 * # 为什么用动态测量而不是固定行高
 *
 * 虚拟化把每一行**绝对定位**在纵向偏移上，行与行之间不会互相让位。
 * 先前用固定行高（`estimateSize`）时，只要某张卡片的实际内容比估值高
 * ——例如选中后多出的徽章换了一行——它就会溢出并**盖住下一行**。
 *
 * 给卡片加 `overflow-hidden` 只能把问题藏起来（内容被裁切），
 * 而且卡片高度实际上是随内容变化的（有无描述、有无标签、有无告警）。
 *
 * 正确做法是把每行的真实高度量出来反馈给虚拟化器：
 * 行高随内容自适应，偏移量随之重算，不会重叠也不会裁切内容。
 */
export function SkillGrid({
  skills,
  selectedId,
  onSelect,
  snippets,
  keywords,
}: SkillGridProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [columns, setColumns] = useState(3);

  // 批量勾选状态直接从 store 取，避免逐层透传
  const checkedIds = useSkillStore((s) => s.selectedIds);
  const toggleChecked = useSkillStore((s) => s.toggleSelected);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;

    const measure = () => {
      const width = el.clientWidth - 24; // 减去左右 padding
      setColumns(Math.max(1, Math.floor(width / MIN_COLUMN_WIDTH)));
    };

    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const rowCount = Math.ceil(skills.length / columns);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ESTIMATED_CARD_HEIGHT + ROW_GAP,
    // 实测每行的真实高度，覆盖估值
    measureElement: (el) => el.getBoundingClientRect().height,
    overscan: 3,
    // 列数变化会让每行装的内容不同，行高随之改变，必须按新列数重新计数
    getItemKey: (index) => `${columns}:${index}`,
  });

  // 列数变化时丢弃旧的测量结果，避免用上一套列数下的行高去排布新布局
  useEffect(() => {
    virtualizer.measure();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [columns]);

  return (
    <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
      <div
        className="relative w-full"
        style={{ height: virtualizer.getTotalSize() }}
      >
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const start = virtualRow.index * columns;
          const rowSkills = skills.slice(start, start + columns);

          return (
            <div
              key={virtualRow.key}
              data-index={virtualRow.index}
              ref={virtualizer.measureElement}
              className="absolute top-0 left-0 grid w-full"
              style={{
                transform: `translateY(${virtualRow.start}px)`,
                gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
                gap: `${ROW_GAP}px`,
              }}
            >
              {rowSkills.map((skill) => (
                <SkillCard
                  key={skill.id}
                  skill={skill}
                  selected={skill.id === selectedId}
                  checked={checkedIds.includes(skill.id)}
                  onSelect={onSelect}
                  onToggleChecked={toggleChecked}
                  snippet={snippets?.get(skill.id)}
                  keywords={keywords}
                />
              ))}
            </div>
          );
        })}
      </div>
    </div>
  );
}
