import * as ToggleGroup from "@radix-ui/react-toggle-group";
import { useQueryClient } from "@tanstack/react-query";
import {
  LayoutGrid,
  List,
  Monitor,
  Moon,
  PanelRightOpen,
  RefreshCw,
  Search,
  Sun,
  X,
} from "lucide-react";

import { cn } from "@/lib/cn";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Tooltip } from "@/components/ui/Tooltip";
import { useConfig, useSkillsScan } from "@/lib/queries";
import {
  useSkillStore,
  type SortKey,
  type SourceFilter,
} from "@/store/skillStore";
import { useViewStore, type ThemeMode, type ViewMode } from "@/store/viewStore";

const VIEW_TITLE: Record<string, string> = {
  skills: "我的 Skills",
  tags: "标签",
  mapping: "映射矩阵",
  settings: "设置",
};

const THEME_CYCLE: ThemeMode[] = ["system", "light", "dark"];
const THEME_META: Record<
  ThemeMode,
  { icon: React.ElementType; label: string }
> = {
  system: { icon: Monitor, label: "主题：跟随系统" },
  light: { icon: Sun, label: "主题：亮色" },
  dark: { icon: Moon, label: "主题：暗色" },
};
const VIEW_MODE_META: Record<
  ViewMode,
  { icon: React.ElementType; label: string }
> = {
  grid: { icon: LayoutGrid, label: "网格视图" },
  list: { icon: List, label: "列表视图" },
};

const SORT_LABEL: Record<SortKey, string> = {
  name: "按名称",
  updated: "按更新时间",
  managed: "按启用数",
};

export function TopBar() {
  const activeView = useViewStore((s) => s.activeView);
  const viewMode = useViewStore((s) => s.viewMode);
  const setViewMode = useViewStore((s) => s.setViewMode);
  const themeMode = useViewStore((s) => s.themeMode);
  const setThemeMode = useViewStore((s) => s.setThemeMode);
  const detailPanelOpen = useViewStore((s) => s.detailPanelOpen);
  const closeDetail = useViewStore((s) => s.closeDetail);
  const showSystemSkills = useViewStore((s) => s.showSystemSkills);
  const setShowSystemSkills = useViewStore((s) => s.setShowSystemSkills);

  const search = useSkillStore((s) => s.search);
  const setSearch = useSkillStore((s) => s.setSearch);
  const sourceFilter = useSkillStore((s) => s.sourceFilter);
  const setSourceFilter = useSkillStore((s) => s.setSourceFilter);
  const sort = useSkillStore((s) => s.sort);
  const setSort = useSkillStore((s) => s.setSort);

  const queryClient = useQueryClient();
  const { data: config } = useConfig();
  const scan = useSkillsScan(config?.centralLibraryPath ?? null);

  const ThemeIcon = THEME_META[themeMode].icon;
  const showSkillControls = activeView === "skills";

  const cycleTheme = () => {
    const idx = THEME_CYCLE.indexOf(themeMode);
    const next = THEME_CYCLE[(idx + 1) % THEME_CYCLE.length];
    if (next) setThemeMode(next);
  };

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ["scan"] });
    void queryClient.invalidateQueries({ queryKey: ["agents"] });
    void queryClient.invalidateQueries({ queryKey: ["index"] });
  };

  // 来源筛选的候选项来自实际检测到的 Agent，而不是写死的列表
  const agentOptions = (scan.data?.agents ?? []).filter(
    (a) => a.status !== "notInstalled",
  );

  // sh-plate：铺了背景图时顶栏要有自己不透明的底，否则文字直接压在插画上
  // （见 globals.css 里 .sh-plate 的说明）。
  return (
    <header className="sh-frost sh-plate flex h-12 shrink-0 items-center gap-3 border-b border-border bg-bg px-3">
      <h1 className="shrink-0 text-sm font-semibold text-fg">
        {VIEW_TITLE[activeView] ?? "BakaSkill"}
      </h1>

      {showSkillControls ? (
        <>
          <div className="mx-auto w-full max-w-sm">
            <Input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="搜索正文，或用 tag: / agent: / is: 过滤"
              aria-label="搜索 Skill"
              title={
                "搜索 SKILL.md 正文、名称与描述。\n" +
                "语法糖（可组合）：\n" +
                "  tag:xxx         按标签过滤\n" +
                "  agent:claude-code  按所在 Agent 过滤\n" +
                "  is:enabled      已在某个 Agent 上启用\n" +
                "  is:external     只在 Agent 目录里，尚未纳入中央库\n" +
                "  is:dangling     链接已断\n" +
                '带空格的值用引号：tag:"code review"'
              }
              leading={<Search />}
              trailing={
                search ? (
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    onClick={() => setSearch("")}
                    aria-label="清除搜索"
                  >
                    <X className="size-3.5" />
                  </Button>
                ) : null
              }
            />
          </div>

          <select
            value={sourceFilter}
            onChange={(e) => setSourceFilter(e.target.value as SourceFilter)}
            aria-label="来源筛选"
            className={cn(
              "h-8.5 shrink-0 rounded-md border border-border bg-surface px-2 text-xs text-fg",
              "outline-none focus-visible:border-accent",
            )}
          >
            {/* 中央库排第一：它是本应用管理的主体，
                其余几项都是"另外还存在于别处"的补充视角 */}
            <option value="central">仅中央库</option>
            <option value="all">全部来源</option>
            <option value="external">含外部副本</option>
            <option value="dangling">含断链</option>
            {agentOptions.map((agent) => (
              <option key={agent.id} value={agent.id}>
                {agent.displayName}
              </option>
            ))}
          </select>

          {/* 与 Windows 的「显示隐藏文件」同位同义：默认藏起来保持干净，
              但开关就在旁边，随时能找回来。 */}
          <label
            className="flex h-8.5 shrink-0 cursor-pointer items-center gap-1.5 rounded-md border border-border bg-surface px-2 text-xs text-fg-muted"
            title="Agent 安装时自带的 Skill（如 Codex 的 .system 分组）。默认不显示，也不纳入管理。"
          >
            <input
              type="checkbox"
              checked={showSystemSkills}
              onChange={(e) => setShowSystemSkills(e.target.checked)}
              className="size-3.5 accent-[var(--color-accent)]"
            />
            系统 Skill
          </label>

          <select
            value={sort}
            onChange={(e) => setSort(e.target.value as SortKey)}
            aria-label="排序方式"
            className={cn(
              "h-8.5 shrink-0 rounded-md border border-border bg-surface px-2 text-xs text-fg",
              "outline-none focus-visible:border-accent",
            )}
          >
            {(Object.keys(SORT_LABEL) as SortKey[]).map((key) => (
              <option key={key} value={key}>
                {SORT_LABEL[key]}
              </option>
            ))}
          </select>
        </>
      ) : (
        <div className="mx-auto" />
      )}

      <div className="flex shrink-0 items-center gap-1">
        <ToggleGroup.Root
          type="single"
          value={viewMode}
          onValueChange={(value) => {
            if (value) setViewMode(value as ViewMode);
          }}
          className="flex items-center gap-0.5 rounded-md border border-border bg-surface p-0.5"
          aria-label="视图切换"
        >
          {(["grid", "list"] as const).map((mode) => {
            const Icon = VIEW_MODE_META[mode].icon;
            return (
              <Tooltip key={mode} label={VIEW_MODE_META[mode].label}>
                <ToggleGroup.Item
                  value={mode}
                  aria-label={VIEW_MODE_META[mode].label}
                  className={cn(
                    "flex size-6.5 items-center justify-center rounded-sm transition-colors",
                    "text-fg-subtle hover:text-fg",
                    "data-[state=on]:bg-accent-subtle data-[state=on]:text-accent",
                  )}
                >
                  <Icon className="size-3.5" />
                </ToggleGroup.Item>
              </Tooltip>
            );
          })}
        </ToggleGroup.Root>

        {showSkillControls ? (
          <Tooltip
            label={
              scan.data
                ? `重新扫描（上次 ${scan.data.durationMs}ms，${scan.data.scannedDirs} 个目录）`
                : "重新扫描"
            }
          >
            <Button
              variant="ghost"
              size="icon"
              onClick={refresh}
              disabled={scan.isFetching}
              aria-label="重新扫描"
            >
              <RefreshCw
                className={cn("size-4", scan.isFetching && "animate-spin")}
              />
            </Button>
          </Tooltip>
        ) : null}

        <Tooltip label={THEME_META[themeMode].label}>
          <Button
            variant="ghost"
            size="icon"
            onClick={cycleTheme}
            aria-label={THEME_META[themeMode].label}
          >
            <ThemeIcon className="size-4" />
          </Button>
        </Tooltip>

        {detailPanelOpen ? (
          <Tooltip label="关闭详情面板">
            <Button
              variant="ghost"
              size="icon"
              onClick={closeDetail}
              aria-label="关闭详情面板"
            >
              <PanelRightOpen className="size-4" />
            </Button>
          </Tooltip>
        ) : null}
      </div>
    </header>
  );
}
