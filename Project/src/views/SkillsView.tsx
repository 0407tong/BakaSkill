import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import {
  AlertTriangle,
  Boxes,
  Download,
  FolderCog,
  Import,
  Loader2,
  Search,
  WifiOff,
} from "lucide-react";

import { Button } from "@/components/ui/Button";
import { EmptyState } from "@/components/EmptyState";
import { BulkActionBar } from "@/components/skill/BulkActionBar";
import { ImportDialog } from "@/components/transfer/ImportDialog";
import { SkillGrid } from "@/components/skill/SkillGrid";
import { SkillList } from "@/components/skill/SkillList";
import { applyFilters, isPureSystemSkill, parseQuery } from "@/lib/skillFilter";
import {
  useConfig,
  useImportPreviewPaths,
  usePing,
  useSkillSearch,
  useSkillsScan,
} from "@/lib/queries";
import { useFileDrop } from "@/lib/useFileDrop";
import { useSkillStore } from "@/store/skillStore";
import { useViewStore } from "@/store/viewStore";
import type { ImportPreview } from "@/types/ipc";

/** 输入停顿多久才去问后端。太短会把每个字符都变成一次 IPC，太长则显得迟钝。 */
const SEARCH_DEBOUNCE_MS = 180;

/**
 * 防抖：返回一个滞后于输入的副本。
 *
 * 只有需要**发起 IPC** 的那部分（关键词）才走防抖；筛选与排序都是
 * 纯内存计算，跟着输入即时生效——用户敲 `is:enabled` 时不该感觉卡顿。
 */
function useDebounced<T>(value: T, delay: number): T {
  const [settled, setSettled] = useState(value);

  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), delay);
    return () => clearTimeout(timer);
  }, [value, delay]);

  return settled;
}

/** 「我的 Skills」主视图 */
export function SkillsView() {
  const setActiveView = useViewStore((s) => s.setActiveView);
  const viewMode = useViewStore((s) => s.viewMode);
  const openDetail = useViewStore((s) => s.openDetail);
  const showSystemSkills = useViewStore((s) => s.showSystemSkills);
  const setShowSystemSkills = useViewStore((s) => s.setShowSystemSkills);

  const search = useSkillStore((s) => s.search);
  const sourceFilter = useSkillStore((s) => s.sourceFilter);
  const sort = useSkillStore((s) => s.sort);
  const selectedId = useSkillStore((s) => s.selectedId);
  const select = useSkillStore((s) => s.select);
  const [importOpen, setImportOpen] = useState(false);
  // 拖拽那条路自己准备好预览，再交给对话框——对话框不参与"选来源"
  const [presetPreview, setPresetPreview] = useState<ImportPreview | null>(
    null,
  );

  const ping = usePing();
  const { data: config, isPending: configLoading } = useConfig();
  const libraryPath = config?.centralLibraryPath ?? null;
  const scan = useSkillsScan(libraryPath);

  const previewPaths = useImportPreviewPaths();

  // 把 Skill 拖进这一页即导入。落盘前仍然走预览——用户拖的可能是一个
  // 装着 5 个 Skill 的文件夹，而他以为拖的是 1 个。
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

  // 语法糖（tag:/agent:/is:）在前端就地筛选，只有**关键词**需要走全文检索。
  // 因此先解析一遍，把关键词单独拿出来防抖后再发给后端。
  const parsed = useMemo(() => parseQuery(search), [search]);
  const debouncedKeywords = useDebounced(parsed.keywords, SEARCH_DEBOUNCE_MS);
  const fullTextQuery = useSkillSearch(debouncedKeywords, libraryPath);

  // 后端结果还没到时传 null（退回本地子串匹配），到了就传命中表
  const fullText = useMemo(() => {
    const hits = fullTextQuery.data;
    if (!hits) return null;
    return new Map(hits.map((hit) => [hit.skillId, hit.snippet]));
  }, [fullTextQuery.data]);

  // 过滤是纯函数，交给 useMemo 避免每次渲染重算 1000 条
  const { skills: visible, snippets } = useMemo(
    () =>
      applyFilters(scan.data?.skills ?? [], {
        search,
        sourceFilter,
        sort,
        showSystemSkills,
        fullText,
      }),
    [scan.data, search, sourceFilter, sort, showSystemSkills, fullText],
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

  // 未配置中央库：仍允许浏览 Agent 中已有的 Skill，但提示去设置
  if (!libraryPath) {
    return (
      <EmptyState
        icon={<Boxes />}
        title="还没有配置中央库"
        description={
          <div className="space-y-2">
            <p>
              中央库是 Skill 的统一存放位置。指定一个盘符与文件夹后， BakaSkill
              会把 Skill 集中存放在那里，再映射到各个 Agent。
            </p>
            <p className="text-xs text-fg-subtle">
              未配置中央库时仍可浏览各 Agent 中已有的 Skill，
              但无法进行接入与启停操作。
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

  // 被"系统 Skill"开关挡掉了几个。摘要栏要把它说出来——
  // 藏了东西却不说，用户只会以为 Skill 丢了。
  const hiddenSystemCount = showSystemSkills
    ? 0
    : report.skills.filter(isPureSystemSkill).length;

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      {/* 拖拽蒙层。它同时是"松手会发生什么"的说明。
          松手后仍然要留着（`isPending`）——读一个几百兆的文件夹要好几秒，
          这段时间界面若毫无反应，用户会以为没生效而再拖一次。 */}
      {dragging || previewPaths.isPending ? (
        <div className="pointer-events-none absolute inset-0 z-40 flex items-center justify-center bg-accent-subtle/80 backdrop-blur-[1px]">
          <div className="flex flex-col items-center gap-2 rounded-lg border-2 border-dashed border-accent bg-surface px-6 py-5 text-center shadow-lg">
            {previewPaths.isPending ? (
              <Loader2 className="size-5 animate-spin text-accent" />
            ) : (
              <Download className="size-5 text-accent" />
            )}
            <div className="text-sm font-medium text-fg">
              {previewPaths.isPending ? "正在读取…" : "松手即可导入到中央库"}
            </div>
            <div className="text-xs text-fg-muted">
              支持文件夹、ZIP 与 SKILL.md，可一次拖多个
            </div>
          </div>
        </div>
      ) : null}

      {/* 扫描摘要 —— 让"数字从哪来"对用户可见。
          sh-plate：这一条本来就没有自己的底色，铺了背景图之后文字会直接压在
          插画上而看不清（见 globals.css 的说明）。 */}
      <div className="sh-plate flex shrink-0 flex-wrap items-center gap-x-4 gap-y-1 border-b border-border px-3 py-1.5 text-[11px] text-fg-subtle">
        <span>
          共 <strong className="text-fg">{report.skills.length}</strong> 个
          Skill
          {visible.length !== report.skills.length ? (
            <> · 当前显示 {visible.length}</>
          ) : null}
        </span>
        <span>
          扫描 {report.scannedDirs} 个目录 · 耗时 {report.durationMs}ms
        </span>

        {/* 藏起来的东西必须说得出有几个、还得能一键找回。
            判定规则是"分组的目录名以点开头"（Codex 的 .system 就是），
            因此用户自己放一个 `.某个目录/` 进去也会被归为内置——
            若只是默默不显示，他会以为 Skill 丢了。 */}
        {!showSystemSkills && hiddenSystemCount > 0 ? (
          <button
            type="button"
            onClick={() => setShowSystemSkills(true)}
            className="text-accent underline-offset-2 hover:underline"
            title="Agent 安装时自带的 Skill（分组目录名以点开头）。点此显示。"
          >
            已隐藏 {hiddenSystemCount} 个 Agent 自带的 Skill
          </button>
        ) : null}
        {report.skipped.length > 0 ? (
          <span
            className="flex items-center gap-1 text-warning"
            title={report.skipped.join("\n")}
          >
            <AlertTriangle className="size-3" />
            {report.skipped.length} 项被跳过
          </span>
        ) : null}

        {/* 把解析出来的筛选条件回显出来：用户写了 tag:foo 却看到没过滤，
            至少能一眼看出"应用读到的是什么"，而不是对着空结果猜。 */}
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
            className="flex items-center gap-1 text-warning"
            title={parsed.malformed.join("\n")}
          >
            <AlertTriangle className="size-3" />
            筛选条件写法不完整：{parsed.malformed.join("、")}
          </span>
        ) : null}

        <Button
          variant="ghost"
          size="sm"
          className="ml-auto"
          onClick={() => setImportOpen(true)}
        >
          <Import className="size-3.5" />
          导入
        </Button>
      </div>

      <ImportDialog
        open={importOpen}
        onOpenChange={(next) => {
          setImportOpen(next);
          // 预览归调用方持有，关掉就要一起清掉；否则下次打开会拿
          // 上一次拖拽的预览冒充新预览
          if (!next) setPresetPreview(null);
        }}
        libraryPath={libraryPath}
        presetPreview={presetPreview}
      />

      <BulkActionBar visible={visible} allSkills={report.skills} />

      {report.skills.length === 0 ? (
        <EmptyState
          icon={<Boxes />}
          title="还没有发现任何 Skill"
          description={
            <div className="space-y-1">
              <p>中央库与已检测到的 Agent 技能目录中都没有 SKILL.md。</p>
              <p className="text-xs text-fg-subtle">
                把 Skill 文件夹放进中央库的 skills 目录，或在 Agent 中添加 Skill
                后点刷新。
              </p>
            </div>
          }
        />
      ) : visible.length === 0 ? (
        <EmptyState
          icon={<Search />}
          title="没有匹配的 Skill"
          description="试试更换关键词，或把来源筛选改回「全部来源」。"
        />
      ) : viewMode === "grid" ? (
        <SkillGrid
          skills={visible}
          selectedId={selectedId}
          onSelect={handleSelect}
          snippets={snippets}
          keywords={parsed.keywords}
        />
      ) : (
        <SkillList
          skills={visible}
          selectedId={selectedId}
          onSelect={handleSelect}
          snippets={snippets}
          keywords={parsed.keywords}
        />
      )}
    </div>
  );
}
