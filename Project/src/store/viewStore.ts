import { create } from "zustand";
import { persist } from "zustand/middleware";

/** 主内容区的视图。桌面端不使用路由库，由本 store 驱动切换。 */
export type AppView = "skills" | "tags" | "mapping" | "settings";

export type ViewMode = "grid" | "list";

/** 主题：跟随系统 / 强制亮色 / 强制暗色 */
export type ThemeMode = "system" | "light" | "dark";

/** 背景图用哪一张 */
export type BackgroundMode = "default" | "custom" | "none";

interface ViewState {
  activeView: AppView;
  viewMode: ViewMode;
  themeMode: ThemeMode;
  sidebarCollapsed: boolean;
  detailPanelOpen: boolean;
  /**
   * 是否在「我的 Skills」里显示 **Agent 自带的 Skill**（Codex 的 `.system` 分组等）。
   *
   * 默认**关**：本应用管理的是"用户自己的 Skill"，Agent 装好就带的那几个
   * 既不该被纳管、也不该在列表里占位置。需要时打开看。
   *
   * 与 Windows 资源管理器里"显示隐藏文件"是同一个位置、同一个默认值取向：
   * 默认藏起来保持干净，但要让人能找到开关，而不是真的消失。
   */
  showSystemSkills: boolean;

  /**
   * 背景图用哪一张：
   * - `default` 内置默认背景（打包在前端里）
   * - `custom`  用户自选的图（后端复制到应用数据目录，经 data URL 交过来）
   * - `none`    不显示背景，界面回到完全不透明的样子
   */
  backgroundMode: BackgroundMode;
  /** 背景的明显程度 0–100。同时也驱动面板有多透明（见 lib/background.ts）。 */
  backgroundOpacity: number;

  setBackgroundMode: (mode: BackgroundMode) => void;
  setBackgroundOpacity: (opacity: number) => void;

  setActiveView: (view: AppView) => void;
  setViewMode: (mode: ViewMode) => void;
  setThemeMode: (mode: ThemeMode) => void;
  setSidebarCollapsed: (collapsed: boolean) => void;
  toggleSidebar: () => void;
  setShowSystemSkills: (show: boolean) => void;
  openDetail: () => void;
  closeDetail: () => void;
}

/**
 * 说明：**当前选中的 Skill 不在这里**，而在 `skillStore.selectedId`。
 * 本 store 只负责窗口外观（视图、主题、面板开合），不持久化业务选中态。
 */

export const useViewStore = create<ViewState>()(
  persist(
    (set) => ({
      activeView: "skills",
      viewMode: "grid",
      themeMode: "system",
      sidebarCollapsed: false,
      detailPanelOpen: false,
      showSystemSkills: false,
      backgroundMode: "default",
      // 默认给到"看得见图、也读得清字"的中间值，用户可自行拉
      backgroundOpacity: 55,

      setBackgroundMode: (backgroundMode) => set({ backgroundMode }),
      setBackgroundOpacity: (backgroundOpacity) => set({ backgroundOpacity }),

      setActiveView: (activeView) => set({ activeView }),
      setViewMode: (viewMode) => set({ viewMode }),
      setThemeMode: (themeMode) => set({ themeMode }),
      setSidebarCollapsed: (sidebarCollapsed) => set({ sidebarCollapsed }),
      toggleSidebar: () =>
        set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
      setShowSystemSkills: (showSystemSkills) => set({ showSystemSkills }),

      openDetail: () => set({ detailPanelOpen: true }),
      closeDetail: () => set({ detailPanelOpen: false }),
    }),
    {
      name: "bakaskill.ui",
      // 只持久化界面偏好；选中项与详情面板开合不跨会话保留。
      partialize: (s) => ({
        activeView: s.activeView,
        viewMode: s.viewMode,
        themeMode: s.themeMode,
        sidebarCollapsed: s.sidebarCollapsed,
        // 与"显示隐藏文件"一样，是个记得住的偏好，不该每次启动都重置
        showSystemSkills: s.showSystemSkills,
        backgroundMode: s.backgroundMode,
        backgroundOpacity: s.backgroundOpacity,
      }),
    },
  ),
);
