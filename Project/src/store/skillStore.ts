import { create } from "zustand";
import { persist } from "zustand/middleware";

/** 排序维度 */
export type SortKey = "name" | "updated" | "managed";

/**
 * 来源筛选。除固定取值外，也可以是某个 Agent 的 id。
 * `"instance:xxx"` 表示只看「在某个 Agent 中处于某种形态」的 Skill。
 */
export type SourceFilter = "all" | "central" | "external" | "dangling" | string;

interface SkillState {
  search: string;
  sourceFilter: SourceFilter;
  sort: SortKey;
  /** 当前选中的 Skill（详情面板的数据源） */
  selectedId: string | null;
  /** 批量操作的勾选集合。与 `selectedId` 是两件事：前者用于批量，后者用于看详情。 */
  selectedIds: string[];

  setSearch: (v: string) => void;
  setSourceFilter: (v: SourceFilter) => void;
  setSort: (v: SortKey) => void;
  select: (id: string | null) => void;
  toggleSelected: (id: string) => void;
  setSelectedIds: (ids: string[]) => void;
  clearSelection: () => void;
  reset: () => void;
}

export const useSkillStore = create<SkillState>()(
  persist(
    (set) => ({
      search: "",
      // 默认只看中央库：本应用管理的是"用户自己的 Skill"，而中央库才是它的主体。
      // Agent 目录里的外部副本与断链都还在（切一下筛选即可看到），只是不当默认噪音。
      sourceFilter: "central",
      sort: "name",
      selectedId: null,
      selectedIds: [],

      setSearch: (search) => set({ search }),
      setSourceFilter: (sourceFilter) => set({ sourceFilter }),
      setSort: (sort) => set({ sort }),
      select: (selectedId) => set({ selectedId }),

      toggleSelected: (id) =>
        set((s) => ({
          selectedIds: s.selectedIds.includes(id)
            ? s.selectedIds.filter((x) => x !== id)
            : [...s.selectedIds, id],
        })),
      setSelectedIds: (selectedIds) => set({ selectedIds }),
      clearSelection: () => set({ selectedIds: [] }),

      reset: () =>
        set({ search: "", sourceFilter: "all", sort: "name", selectedIds: [] }),
    }),
    {
      name: "bakaskill.skills",
      // 只持久化**排序偏好**。
      //
      // 其余几项都不该跨会话保留：搜索词留着会让用户下次打开时对着一个
      // 被过滤过的清单发愣，却想不起来自己搜过什么；勾选集合尤其危险——
      // 上次勾了 30 个准备批量禁用、关掉应用、下次打开又勾着，
      // 一次误点就是 30 个链接被改动。
      partialize: (s) => ({ sort: s.sort }),
    },
  ),
);
