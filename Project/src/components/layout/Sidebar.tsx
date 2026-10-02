import {
  Boxes,
  ChevronLeft,
  ChevronRight,
  Loader2,
  Network,
  Settings,
  Tags,
  WifiOff,
} from "lucide-react";

import appIcon from "@/assets/app-icon.png";
import { cn } from "@/lib/cn";
import { Button } from "@/components/ui/Button";
import { Tooltip } from "@/components/ui/Tooltip";
import { usePing } from "@/lib/queries";
import { useViewStore, type AppView } from "@/store/viewStore";

interface NavItem {
  id: AppView;
  label: string;
  icon: React.ElementType;
  hint?: string;
}

const NAV_ITEMS: NavItem[] = [
  { id: "skills", label: "我的 Skills", icon: Boxes },
  { id: "tags", label: "标签", icon: Tags },
  {
    id: "mapping",
    label: "映射矩阵",
    icon: Network,
    hint: "查看每个 Skill 在各 Agent 中的链接状态",
  },
  {
    id: "settings",
    label: "设置",
    icon: Settings,
    hint: "中央库路径 / Agent / 同步",
  },
];

export function Sidebar() {
  const collapsed = useViewStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useViewStore((s) => s.toggleSidebar);
  const activeView = useViewStore((s) => s.activeView);
  const setActiveView = useViewStore((s) => s.setActiveView);

  return (
    <aside
      className={cn(
        // sh-frost：铺了背景图时给这一栏加磨砂（见 globals.css）。
        // 不铺背景时这条规则不生效，等同于什么都不做。
        "sh-frost flex shrink-0 flex-col border-r border-border bg-bg-subtle",
        "transition-[width] duration-150 ease-out",
        collapsed ? "w-14" : "w-56",
      )}
    >
      <div
        className={cn(
          "flex h-12 shrink-0 items-center border-b border-border",
          collapsed ? "justify-center px-2" : "justify-between px-3",
        )}
      >
        {!collapsed ? (
          <div className="flex min-w-0 items-center gap-2">
            {/* 用应用图标的 128px 缩略图（不是 1.4 MB 的原图——
                为一个 24px 的位置背一整张原图不划算）。
                原图是圆角方形插画，这里再加上圆角与细细的描边收一收边。 */}
            <img
              src={appIcon}
              alt=""
              className="size-6 shrink-0 rounded-sm object-cover ring-1 ring-border"
            />
            <span className="truncate text-sm font-semibold text-fg">
              BakaSkill
            </span>
          </div>
        ) : null}
        <Tooltip label={collapsed ? "展开侧边栏" : "折叠侧边栏"} side="right">
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={toggleSidebar}
            aria-label={collapsed ? "展开侧边栏" : "折叠侧边栏"}
            aria-expanded={!collapsed}
          >
            {collapsed ? (
              <ChevronRight className="size-4" />
            ) : (
              <ChevronLeft className="size-4" />
            )}
          </Button>
        </Tooltip>
      </div>

      <nav className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto p-2">
        {NAV_ITEMS.map((item) => {
          const Icon = item.icon;
          const active = activeView === item.id;
          const button = (
            <button
              key={item.id}
              type="button"
              onClick={() => setActiveView(item.id)}
              aria-current={active ? "page" : undefined}
              className={cn(
                "flex h-8.5 w-full items-center gap-2.5 rounded-md text-sm transition-colors",
                collapsed ? "justify-center px-0" : "px-2.5",
                active
                  ? "bg-accent-subtle font-medium text-accent"
                  : "text-fg-muted hover:bg-surface-hover hover:text-fg",
              )}
            >
              <Icon className="size-4 shrink-0" />
              {!collapsed ? (
                <span className="truncate">{item.label}</span>
              ) : null}
            </button>
          );

          if (!collapsed) return button;
          return (
            <Tooltip
              key={item.id}
              label={item.hint ? `${item.label} — ${item.hint}` : item.label}
              side="right"
            >
              {button}
            </Tooltip>
          );
        })}

        {/* 这里不挂「Agent 来源」分组计数：**「我的 Skills」顶上那条横栏已经能选来源**，
            侧边栏这份是重复的，且点击行为（顺带切到 skills 视图）与横栏不一致，
            两处入口反而让人不确定哪个才算数。 */}
      </nav>

      <BackendStatus collapsed={collapsed} />
    </aside>
  );
}

/**
 * 后端连通性指示。它是「React -> Tauri invoke -> Rust 命令」
 * 整条链路可用的可见证明。
 */
function BackendStatus({ collapsed }: { collapsed: boolean }) {
  const { data, isPending, isError, error, refetch, isFetching } = usePing();

  let icon = <Loader2 className="size-3.5 shrink-0 animate-spin" />;
  let text = "连接中…";
  let tone = "text-fg-subtle";

  if (isError) {
    icon = <WifiOff className="size-3.5 shrink-0" />;
    text = "后端未响应";
    tone = "text-danger";
  } else if (data) {
    icon = <span className="size-1.5 shrink-0 rounded-full bg-success" />;
    text = `后端就绪 · v${data.appVersion}`;
    tone = "text-fg-subtle";
  }

  const detail =
    isError && error instanceof Error
      ? error.message
      : data
        ? `pong · ${data.platform} · ${new Date(data.timestampMs).toLocaleTimeString("zh-CN")}`
        : "正在探测后端命令通道";

  const chip = (
    <button
      type="button"
      onClick={() => void refetch()}
      className={cn(
        "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[11px] transition-colors hover:bg-surface-hover",
        collapsed && "justify-center px-0",
        tone,
      )}
    >
      {icon}
      {!collapsed ? (
        <span className="truncate">
          {isFetching && !isPending ? "检查中…" : text}
        </span>
      ) : null}
    </button>
  );

  return (
    <div className="shrink-0 border-t border-border p-2">
      <Tooltip label={collapsed ? text : detail} side="right">
        {chip}
      </Tooltip>
    </div>
  );
}
