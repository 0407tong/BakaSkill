import { create } from "zustand";
import { persist } from "zustand/middleware";

/** 排序维度 */
export type SortKey = "name" | "updated" | "managed";

/**
 * 清晰的 Skill 状态分类：
 * - `all`：全部
 * - `enabled`：已启用（当前视角下至少接入了一个 Agent，或在所选 Agent 中已启用）
 * - `disabled`：未启用（在中央库中，但尚未接入）
 * - `unmanaged`：未纳管（仅存在于 Agent 目录中的实体副本）
 * - `abnormal`：异常（断链或同名目录冲突）
 */
export type SourceFilter =
  "all" | "enabled" | "disabled" | "unmanaged" | "abnormal";

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
      sourceFilter: "all",
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
