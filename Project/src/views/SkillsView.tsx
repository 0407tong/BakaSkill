import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import {
  AlertTriangle,
  Boxes,
  Download,
  FolderCog,
  FolderPlus,
  Import,
  Loader2,
  Search,
  WifiOff,
} from "lucide-react";

import { EmptyState } from "@/components/EmptyState";
import { BulkActionBar } from "@/components/skill/BulkActionBar";
import { SkillGrid } from "@/components/skill/SkillGrid";
import { SkillList } from "@/components/skill/SkillList";
import { ImportDialog } from "@/components/transfer/ImportDialog";
import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import {
  useConfig,
  useCreateAgentSkillDir,
  useImportPreviewPaths,
  useLinkMatrix,
  usePing,
  useSkillSearch,
  useSkillsScan,
} from "@/lib/queries";
import { applyFilters, isPureSystemSkill, parseQuery } from "@/lib/skillFilter";
import { useFileDrop } from "@/lib/useFileDrop";
import { useSkillStore, type SourceFilter } from "@/store/skillStore";
import { useViewStore } from "@/store/viewStore";
import type { AgentDetection, ImportPreview, LinkState } from "@/types/ipc";

/** 输入停顿多久才去问后端。太短会把每个字符都变成一次 IPC，太长则显得迟钝。 */
const SEARCH_DEBOUNCE_MS = 180;

function useDebounced<T>(value: T, delay: number): T {
  const [settled, setSettled] = useState(value);

  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), delay);
    return () => clearTimeout(timer);
  }, [value, delay]);

  return settled;
}

/** 「我的 Skills」主视图：集成独立 Agent 切换行 + 清晰状态分类 + 批量管理 */
export function SkillsView() {
  const setActiveView = useViewStore((s) => s.setActiveView);
  const viewMode = useViewStore((s) => s.viewMode);
  const openDetail = useViewStore((s) => s.openDetail);
  const showSystemSkills = useViewStore((s) => s.showSystemSkills);
  const setShowSystemSkills = useViewStore((s) => s.setShowSystemSkills);
  const selectedAgentId = useViewStore((s) => s.selectedAgentId);
  const setSelectedAgentId = useViewStore((s) => s.setSelectedAgentId);

  const search = useSkillStore((s) => s.search);
  const sourceFilter = useSkillStore((s) => s.sourceFilter);
  const setSourceFilter = useSkillStore((s) => s.setSourceFilter);
  const sort = useSkillStore((s) => s.sort);
  const selectedId = useSkillStore((s) => s.selectedId);
  const select = useSkillStore((s) => s.select);
  const clearSelection = useSkillStore((s) => s.clearSelection);

  const [importOpen, setImportOpen] = useState(false);
  const [presetPreview, setPresetPreview] = useState<ImportPreview | null>(
    null,
  );

  const ping = usePing();
  const { data: config, isPending: configLoading } = useConfig();
  const libraryPath = config?.centralLibraryPath ?? null;
  const scan = useSkillsScan(libraryPath);
  const matrix = useLinkMatrix();
  const createDir = useCreateAgentSkillDir();
  const previewPaths = useImportPreviewPaths();

  const dragging = useFileDrop((paths) => {
    if (!libraryPath) {
      toast.error("还没有设置中央库位置", {
        description: "先在设置页选一个文件夹，拖进来的 Skill 才有地方放。",
      });
      return;
    }
    previewPaths.mutate(
      { paths, path: libraryPath },
      {
        onSuccess: (preview) => {
          setPresetPreview(preview);
          setImportOpen(true);
        },
      },
    );
  });

  // 所有已启用且已安装的 Agent（用于顶部独立模型切换栏）
  const installedAgents: AgentDetection[] = useMemo(
    () =>
      (scan.data?.agents ?? []).filter(
        (a) => a.enabled && a.status !== "notInstalled",
      ),
    [scan.data],
  );

  const readyAgents = useMemo(
    () => installedAgents.filter((a) => a.status === "detected"),
    [installedAgents],
  );

  // 当前选中的 Agent；若已被移除则自动回退到「全部」
  const activeAgent = useMemo(
    () =>
      selectedAgentId
        ? (installedAgents.find((a) => a.id === selectedAgentId) ?? null)
        : null,
    [installedAgents, selectedAgentId],
  );
  const activeAgentId = activeAgent?.id ?? null;

  // 每个 Agent 已启用的 Skill 计数
  const enabledCountByAgent = useMemo(() => {
    const map = new Map<string, number>();
    for (const row of matrix.data?.rows ?? []) {
      for (const cell of row.cells) {
        if (cell.state === "valid") {
          map.set(cell.agentId, (map.get(cell.agentId) ?? 0) + 1);
        }
      }
    }
    return map;
  }, [matrix.data]);

  const centralCount = useMemo(
    () => (scan.data?.skills ?? []).filter((s) => s.inCentralLibrary).length,
    [scan.data],
  );

  // 选中具体 Agent 时，构建 skillId -> LinkState 映射
  const agentStates = useMemo(() => {
    if (!activeAgentId) return null;
    const map = new Map<string, LinkState>();
    for (const row of matrix.data?.rows ?? []) {
      const cell = row.cells.find((c) => c.agentId === activeAgentId);
      if (cell) map.set(row.skillId, cell.state);
    }
    return map;
  }, [activeAgentId, matrix.data]);

  // 统计当前视角下各分类数量（用于简洁分类条）
  const categoryCounts = useMemo(() => {
    const all = (scan.data?.skills ?? []).filter((skill) => {
      if (!showSystemSkills && isPureSystemSkill(skill)) return false;
      if (activeAgentId && !skill.inCentralLibrary) {
        return skill.instances.some((i) => i.agentId === activeAgentId);
      }
      return true;
    });

    let enabled = 0;
    let disabled = 0;
    let unmanaged = 0;
    let abnormal = 0;

    for (const skill of all) {
      if (activeAgentId) {
        const st = agentStates?.get(skill.id);
        if (st === "valid") enabled += 1;
        else if (st === "dangling" || st === "conflict") abnormal += 1;
        else if (!skill.inCentralLibrary) unmanaged += 1;
        else disabled += 1;
      } else {
        if (
          skill.instances.some(
            (i) => i.kind === "dangling" || i.kind === "externalDuplicate",
          )
        ) {
          abnormal += 1;
        }
        if (skill.managedCount > 0) enabled += 1;
        else if (skill.inCentralLibrary) disabled += 1;
        if (!skill.inCentralLibrary) unmanaged += 1;
      }
    }
    return { all: all.length, enabled, disabled, unmanaged, abnormal };
  }, [scan.data, showSystemSkills, activeAgentId, agentStates]);

  const parsed = useMemo(() => parseQuery(search), [search]);
  const debouncedKeywords = useDebounced(parsed.keywords, SEARCH_DEBOUNCE_MS);
  const fullTextQuery = useSkillSearch(debouncedKeywords, libraryPath);

  const fullText = useMemo(() => {
    const hits = fullTextQuery.data;
    if (!hits) return null;
    return new Map(hits.map((hit) => [hit.skillId, hit.snippet]));
  }, [fullTextQuery.data]);

  const { skills: visible, snippets } = useMemo(
    () =>
      applyFilters(scan.data?.skills ?? [], {
        search,
        sourceFilter,
        sort,
        showSystemSkills,
        agentId: activeAgentId,
        agentStates,
        fullText,
      }),
    [
      scan.data,
      search,
      sourceFilter,
      sort,
      showSystemSkills,
      activeAgentId,
      agentStates,
      fullText,
    ],
  );

  const handleSelect = (id: string) => {
    select(id);
    openDetail();
  };

  if (ping.isPending || configLoading) {
    return (
      <EmptyState
        icon={<Loader2 className="animate-spin" />}
        title="正在连接后端…"
        description="正在读取配置并扫描 Agent 技能目录。"
      />
    );
  }

  if (ping.isError) {
    return (
      <EmptyState
        icon={<WifiOff />}
        title="后端未响应"
        description={
          <pre className="mx-auto max-w-lg overflow-auto rounded-md border border-border bg-bg-subtle p-2 text-left font-mono text-xs">
            {ping.error instanceof Error
              ? ping.error.message
              : String(ping.error)}
          </pre>
        }
      />
    );
  }

  if (!libraryPath) {
    return (
      <EmptyState
        icon={<Boxes />}
        title="还没有配置中央库"
        description={
          <div className="space-y-2">
            <p>
              中央库是 Skill 的统一存放位置。指定一个盘符与文件夹后，BakaSkill
              会把 Skill 集中存放在那里，再映射到各个 Agent。
            </p>
            <p className="text-xs text-fg-subtle">
              未配置中央库时仍可浏览各 Agent 中已有的
              Skill，但无法进行接入与启停操作。
            </p>
          </div>
        }
        action={
          <Button onClick={() => setActiveView("settings")}>
            <FolderCog className="size-4" />
            前往设置中央库
          </Button>
        }
      />
    );
  }

  if (scan.isPending) {
    return (
      <EmptyState
        icon={<Loader2 className="animate-spin" />}
        title="正在扫描…"
        description="正在读取各 Agent 的技能目录并与中央库比对。"
      />
    );
  }

  if (scan.isError) {
    return (
      <EmptyState
        icon={<WifiOff />}
        title="扫描失败"
        description={
          scan.error instanceof Error ? scan.error.message : "无法完成扫描"
        }
      />
    );
  }

  const report = scan.data;
  if (!report) return null;

  const hiddenSystemCount = showSystemSkills
    ? 0
    : report.skills.filter(isPureSystemSkill).length;

  const categoryTabs: { id: SourceFilter; label: string; count: number }[] = [
    { id: "all", label: "全部", count: categoryCounts.all },
    { id: "enabled", label: "已启用", count: categoryCounts.enabled },
    { id: "disabled", label: "未启用", count: categoryCounts.disabled },
    ...(categoryCounts.unmanaged > 0 || sourceFilter === "unmanaged"
      ? [
          {
            id: "unmanaged" as const,
            label: "未纳管",
            count: categoryCounts.unmanaged,
          },
        ]
      : []),
    ...(categoryCounts.abnormal > 0 || sourceFilter === "abnormal"
      ? [
          {
            id: "abnormal" as const,
            label: "异常",
            count: categoryCounts.abnormal,
          },
        ]
      : []),
  ];

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      {/* 拖拽导入蒙层 */}
      {dragging || previewPaths.isPending ? (
        <div className="pointer-events-none absolute inset-0 z-40 flex items-center justify-center bg-accent-subtle/80 backdrop-blur-[2px]">
          <div className="flex flex-col items-center gap-2 rounded-xl border-2 border-dashed border-accent bg-surface px-6 py-5 text-center shadow-lg">
            {previewPaths.isPending ? (
              <Loader2 className="size-5 animate-spin text-accent" />
            ) : (
              <Download className="size-5 text-accent" />
            )}
            <div className="text-sm font-medium text-fg">
              {previewPaths.isPending ? "正在读取…" : "松手即可导入到中央库"}
            </div>
            <div className="text-xs text-fg-muted">
              支持文件夹、ZIP / 7z / TAR.GZ / .skill 与 SKILL.md
            </div>
          </div>
        </div>
      ) : null}

      {/* Agent 切换栏 */}
      <div className="sh-plate flex shrink-0 items-center gap-1.5 overflow-x-auto border-b border-border px-4 py-2">
        <button
          type="button"
          onClick={() => {
            setSelectedAgentId(null);
            clearSelection();
          }}
          aria-pressed={activeAgentId === null}
          className={cn(
            "flex shrink-0 items-center gap-1.5 rounded-lg border px-3 py-1 text-xs transition-all",
            activeAgentId === null
              ? "border-accent bg-accent-subtle font-medium text-accent shadow-xs"
              : "border-border bg-surface text-fg-muted hover:border-border-strong hover:bg-surface-hover hover:text-fg",
          )}
        >
          <span>全部模型</span>
          <span
            className={cn(
              "rounded-full px-1.5 py-px font-mono text-[10px] tabular-nums",
              activeAgentId === null
                ? "bg-accent/15 text-accent"
                : "bg-bg-subtle text-fg-subtle",
            )}
          >
            {centralCount}
          </span>
        </button>

        {installedAgents.map((agent) => {
          const selected = agent.id === activeAgentId;
          const count = enabledCountByAgent.get(agent.id) ?? 0;
          const ready = agent.status === "detected";
          return (
            <button
              key={agent.id}
              type="button"
              onClick={() => {
                setSelectedAgentId(agent.id);
                clearSelection();
              }}
              aria-pressed={selected}
              className={cn(
                "flex shrink-0 items-center gap-1.5 rounded-lg border px-3 py-1 text-xs transition-all",
                selected
                  ? "border-accent bg-accent-subtle font-medium text-accent shadow-xs"
                  : "border-border bg-surface text-fg-muted hover:border-border-strong hover:bg-surface-hover hover:text-fg",
              )}
            >
              <span
                className={cn(
                  "size-1.5 shrink-0 rounded-full",
                  ready ? "bg-success" : "bg-warning",
                )}
              />
              <span>{agent.displayName}</span>
              <span
                className={cn(
                  "rounded-full px-1.5 py-px font-mono text-[10px] tabular-nums",
                  selected
                    ? "bg-accent/15 text-accent"
                    : "bg-bg-subtle text-fg-subtle",
                )}
              >
                {ready ? `${count}/${centralCount}` : "待创建"}
              </span>
            </button>
          );
        })}

        <div className="ml-auto flex shrink-0 items-center gap-1">
          <Button variant="ghost" size="sm" onClick={() => setImportOpen(true)}>
            <Import className="size-3.5" />
            导入
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setActiveView("settings")}
          >
            <FolderCog className="size-3.5" />
            管理 Agent
          </Button>
        </div>
      </div>

      {/* 若所选 Agent 的技能目录尚未创建，就地给出创建入口 */}
      {activeAgent && activeAgent.status === "installedNoSkillDir" ? (
        <div className="sh-plate flex shrink-0 flex-wrap items-center gap-2 border-b border-warning/30 bg-warning-subtle/80 px-4 py-2 text-xs">
          <AlertTriangle className="size-3.5 shrink-0 text-warning" />
          <span className="text-fg-muted">
            <strong className="text-fg">{activeAgent.displayName}</strong>{" "}
            的技能目录尚未创建
            {activeAgent.skillDir ? (
              <code className="ml-1.5 rounded-xs bg-surface/80 px-1.5 py-0.5 font-mono text-[11px] text-fg">
                {activeAgent.skillDir}
              </code>
            ) : null}
          </span>
          <div className="ml-auto flex items-center gap-1.5">
            {activeAgent.skillDir ? (
              <Button
                size="sm"
                disabled={createDir.isPending}
                onClick={() => createDir.mutate(activeAgent.id)}
              >
                {createDir.isPending ? (
                  <Loader2 className="size-3.5 animate-spin" />
                ) : (
                  <FolderPlus className="size-3.5" />
                )}
                一键创建目录
              </Button>
            ) : (
              <Button
                size="sm"
                variant="secondary"
                onClick={() => setActiveView("settings")}
              >
                前往设置指定目录
              </Button>
            )}
          </div>
        </div>
      ) : null}

      {/* 状态分类条 */}
      <div className="sh-plate flex shrink-0 flex-wrap items-center gap-1.5 border-b border-border px-4 py-1.5 text-xs">
        <div className="flex flex-wrap items-center gap-1">
          {categoryTabs.map((tab) => {
            const active = sourceFilter === tab.id;
            return (
              <button
                key={tab.id}
                type="button"
                onClick={() => setSourceFilter(tab.id)}
                className={cn(
                  "inline-flex items-center gap-1 rounded-md px-2.5 py-1 text-xs transition-colors",
                  active
                    ? "bg-accent-subtle font-medium text-accent"
                    : "text-fg-muted hover:bg-surface-hover hover:text-fg",
                )}
              >
                <span>{tab.label}</span>
                <span className="font-mono text-[10px] tabular-nums opacity-75">
                  {tab.count}
                </span>
              </button>
            );
          })}
        </div>

        {/* 解析出来的搜索语法糖标签 */}
        {[
          ...parsed.tags.map((t) => `tag:${t}`),
          ...parsed.agents.map((a) => `agent:${a}`),
          ...parsed.flags.map((f) => `is:${f}`),
        ].map((chip) => (
          <span
            key={chip}
            className="rounded-sm bg-accent-subtle px-1.5 py-px font-mono text-[10px] text-accent"
          >
            {chip}
          </span>
        ))}

        {parsed.malformed.length > 0 ? (
          <span
            className="flex items-center gap-1 text-[11px] text-warning"
            title={parsed.malformed.join("\n")}
          >
            <AlertTriangle className="size-3" />
            筛选语法不完整：{parsed.malformed.join("、")}
          </span>
        ) : null}

        <div className="ml-auto flex items-center gap-3 text-[11px] text-fg-subtle">
          {!showSystemSkills && hiddenSystemCount > 0 ? (
            <button
              type="button"
              onClick={() => setShowSystemSkills(true)}
              className="text-accent underline-offset-2 hover:underline"
            >
              显示 {hiddenSystemCount} 个系统 Skill
            </button>
          ) : null}
          {report.skipped.length > 0 ? (
            <span
              className="flex items-center gap-1 text-warning"
              title={report.skipped.join("\n")}
            >
              <AlertTriangle className="size-3" />
              {report.skipped.length} 项跳过
            </span>
          ) : null}
        </div>
      </div>

      <ImportDialog
        open={importOpen}
        onOpenChange={(next) => {
          setImportOpen(next);
          if (!next) setPresetPreview(null);
        }}
        libraryPath={libraryPath}
        presetPreview={presetPreview}
      />

      <BulkActionBar
        visible={visible}
        allSkills={report.skills}
        activeAgent={activeAgent}
        readyAgents={readyAgents}
      />

      {report.skills.length === 0 ? (
        <EmptyState
          icon={<Boxes />}
          title="还没有发现任何 Skill"
          description={
            <div className="space-y-1">
              <p>中央库与已检测到的 Agent 技能目录中都没有 SKILL.md。</p>
              <p className="text-xs text-fg-subtle">
                把 Skill 文件夹或压缩包拖进此页面即可导入。
              </p>
            </div>
          }
        />
      ) : visible.length === 0 ? (
        <EmptyState
          icon={<Search />}
          title="没有匹配的 Skill"
          description="试试切换上方分类标签或更换搜索关键词。"
        />
      ) : viewMode === "grid" ? (
        <SkillGrid
          skills={visible}
          selectedId={selectedId}
          onSelect={handleSelect}
          snippets={snippets}
          keywords={parsed.keywords}
          activeAgentId={activeAgentId}
          agentStates={agentStates}
          readyAgents={readyAgents}
        />
      ) : (
        <SkillList
          skills={visible}
          selectedId={selectedId}
          onSelect={handleSelect}
          snippets={snippets}
          keywords={parsed.keywords}
          activeAgentId={activeAgentId}
          agentStates={agentStates}
          readyAgents={readyAgents}
        />
      )}
    </div>
  );
}
