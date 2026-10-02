import { useCallback, useState, type ReactNode } from "react";
import { Group, Panel, Separator, type Layout } from "react-resizable-panels";
import { Sidebar } from "@/components/layout/Sidebar";
import { TopBar } from "@/components/layout/TopBar";
import { DetailPanel } from "@/components/layout/DetailPanel";
import { useViewStore } from "@/store/viewStore";

const LAYOUT_STORAGE_KEY = "bakaskill.layout.workspace";

/**
 * ⚠️ react-resizable-panels v4 的尺寸单位陷阱（实测踩到，见 docs/ARCHITECTURE.md §6.10）
 *
 * `Panel` 的 `defaultSize` / `minSize` / `maxSize`：
 * - **数字 = 像素**（`defaultSize={34}` 是 34 像素，不是 34%）
 * - 字符串无单位 = 百分比（`defaultSize="34"`）
 * - 也可写显式单位 `"34%"`
 *
 * `Group` 的 `defaultLayout` 则**一律是百分比**（`Layout` 类型即 `{id: 百分比}`）。
 *
 * 早先这里全部传了数字，导致详情面板只有 34 像素宽、且只能在 22–60 像素间拖动，
 * 表现为"右侧是一条窄缝且拖不动"。
 */
const DETAIL_PANEL_DEFAULT_PCT = 34;
const DETAIL_PANEL_MIN_PCT = 22;
const DETAIL_PANEL_MAX_PCT = 60;
const MAIN_PANEL_MIN_PCT = 35;

const DEFAULT_LAYOUT: Layout = {
  main: 100 - DETAIL_PANEL_DEFAULT_PCT,
  detail: DETAIL_PANEL_DEFAULT_PCT,
};

/**
 * 读取已保存的面板布局。
 *
 * **两个键都必须存在且合法才采用**，否则退回默认值。
 * 这一点很关键：详情面板关闭时 Group 里只有 `main`，此时保存下来的布局是
 * `{main: 100}`——若直接拿它当 `defaultLayout`，重新打开面板时 `detail`
 * 没有对应项，面板会拿到库的兜底尺寸（极小），表现为"右侧面板过窄"。
 */
function readStoredLayout(): Layout {
  try {
    const raw = localStorage.getItem(LAYOUT_STORAGE_KEY);
    if (!raw) return DEFAULT_LAYOUT;

    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return DEFAULT_LAYOUT;

    const candidate = parsed as Record<string, unknown>;
    const { main, detail } = candidate;

    const valid =
      typeof main === "number" &&
      typeof detail === "number" &&
      Number.isFinite(main) &&
      Number.isFinite(detail) &&
      main > 0 &&
      detail >= DETAIL_PANEL_MIN_PCT &&
      detail <= DETAIL_PANEL_MAX_PCT;

    return valid ? { main, detail } : DEFAULT_LAYOUT;
  } catch {
    return DEFAULT_LAYOUT;
  }
}

export interface AppShellProps {
  children: ReactNode;
  /** 全局横幅等内容，渲染在侧边栏之上 */
  banner?: ReactNode;
}

/**
 * 应用外壳：侧边栏 + (主内容区 | 详情面板)。
 *
 * 主内容区与详情面板之间用 react-resizable-panels 的 Group 分隔，宽度可拖拽，
 * 并持久化到 localStorage（v4 已移除 autoSaveId，故手动存取）。
 */
export function AppShell({ children, banner }: AppShellProps) {
  const detailPanelOpen = useViewStore((s) => s.detailPanelOpen);

  /**
   * 初始布局只读一次，**不进 state**。
   *
   * 早先的实现把布局放进 state 并回喂给 `defaultLayout`，导致每拖动一次
   * 就触发重渲染 + 重新应用默认布局，分隔条被不断"弹回"，表现为拖不动。
   * 布局是受控于库自身的运行时状态，React 侧只需记住初始值。
   */
  const [initialLayout] = useState<Layout>(readStoredLayout);

  const handleLayoutChanged = useCallback(
    (next: Layout, meta: { requestedLayout?: Layout }) => {
      const toSave = meta.requestedLayout ?? next;

      // 详情面板关闭时不落盘：此时的布局只有 main，存下来会污染下次恢复
      if (!toSave.detail || !toSave.main) return;

      try {
        localStorage.setItem(LAYOUT_STORAGE_KEY, JSON.stringify(toSave));
      } catch {
        /* 存储不可用时仅丢失布局记忆，不影响功能 */
      }
    },
    [],
  );

  return (
    <div className="flex h-full flex-col overflow-hidden bg-bg">
      {banner}

      <div className="flex min-h-0 flex-1">
        <Sidebar />

        <Group
          orientation="horizontal"
          className="min-h-0 min-w-0 flex-1"
          defaultLayout={initialLayout}
          onLayoutChanged={handleLayoutChanged}
        >
          <Panel
            id="main"
            minSize={`${MAIN_PANEL_MIN_PCT}%`}
            className="flex min-w-0 flex-col overflow-hidden"
          >
            {/* 刷新按钮由 TopBar 自己处理：它需要失效的 query key 与
                扫描状态，从上层传回调反而要多绕一层 */}
            <TopBar />
            <main className="flex min-h-0 flex-1 flex-col overflow-hidden">
              {children}
            </main>
          </Panel>

          {detailPanelOpen ? (
            <>
              {/* 分隔条做成 ~9px 的抓取区，中间一条 1px 视觉线。
                  只用 1px 宽的命中区在实际使用中很难抓住。 */}
              <Separator
                className="group relative flex w-2.5 shrink-0 cursor-col-resize items-center justify-center bg-transparent"
                aria-label="调整详情面板宽度"
              >
                <span className="pointer-events-none absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-border transition-colors group-hover:bg-accent" />
              </Separator>
              <Panel
                id="detail"
                defaultSize={`${DETAIL_PANEL_DEFAULT_PCT}%`}
                minSize={`${DETAIL_PANEL_MIN_PCT}%`}
                maxSize={`${DETAIL_PANEL_MAX_PCT}%`}
                className="min-w-0 overflow-hidden"
              >
                <DetailPanel />
              </Panel>
            </>
          ) : null}
        </Group>
      </div>
    </div>
  );
}
