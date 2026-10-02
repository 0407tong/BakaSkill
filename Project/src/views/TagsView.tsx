import { useMemo, useState } from "react";
import { Check, Loader2, Pencil, Tags, Trash2, X } from "lucide-react";

import { Button } from "@/components/ui/Button";
import { EmptyState } from "@/components/EmptyState";
import { Input } from "@/components/ui/Input";
import { Tooltip } from "@/components/ui/Tooltip";
import {
  useApplyTags,
  useConfig,
  useSkillsScan,
  useTagStats,
} from "@/lib/queries";
import { cn } from "@/lib/cn";
import { useSkillStore } from "@/store/skillStore";
import { useViewStore } from "@/store/viewStore";
import type { TagCount } from "@/types/ipc";

/** 正在进行的改写动作 */
type PendingAction =
  | { kind: "rename"; tag: string }
  | { kind: "merge"; tags: string[] }
  | { kind: "delete"; tags: string[] }
  | null;

/**
 * 标签与分类。
 *
 * # 标签的事实来源是文件，不是这里
 *
 * 本页所有写操作都会改 `SKILL.md` 的 frontmatter，然后重建索引。
 * 页面上出现的标签只是**索引里的一份派生副本**——因此每次改写成功后都要
 * 重新取统计，不能就地改本地数组（那样一旦重建索引就会露馅）。
 *
 * # 大小写
 *
 * 统计与改写都按大小写不敏感归并（`Rust` 与 `rust` 是同一条）。
 * 这个页面本质上是"整理"工具，"把几种写法收拢成一种"正是用户来这里要做的事。
 */
export function TagsView() {
  const { data: config, isPending: configLoading } = useConfig();
  const libraryPath = config?.centralLibraryPath ?? null;

  const stats = useTagStats(libraryPath);
  const scan = useSkillsScan(libraryPath);
  const applyTags = useApplyTags();

  const setSearch = useSkillStore((s) => s.setSearch);
  const setActiveView = useViewStore((s) => s.setActiveView);

  const [selected, setSelected] = useState<string[]>([]);
  const [pending, setPending] = useState<PendingAction>(null);
  const [draft, setDraft] = useState("");

  // `?? []` 每次渲染都会造一个新数组，若直接进依赖会让下游 useMemo 永远重算
  const tags = useMemo(() => stats.data ?? [], [stats.data]);
  const maxCount = useMemo(
    () => tags.reduce((max, t) => Math.max(max, t.count), 1),
    [tags],
  );

  /** 选中的标签命中的 Skill（用扫描清单算，与「我的 Skills」看到的是同一份数据） */
  const matched = useMemo(() => {
    if (selected.length === 0) return [];
    const wanted = selected.map((t) => t.toLowerCase());
    return (scan.data?.skills ?? []).filter((skill) =>
      skill.tags.some((t) => wanted.includes(t.toLowerCase())),
    );
  }, [scan.data, selected]);

  const toggle = (tag: string) => {
    setPending(null);
    setSelected((current) =>
      current.includes(tag)
        ? current.filter((t) => t !== tag)
        : [...current, tag],
    );
  };

  const startRename = () => {
    const tag = selected[0];
    if (selected.length !== 1 || !tag) return;
    setDraft(tag);
    setPending({ kind: "rename", tag });
  };

  const startMerge = () => {
    if (selected.length < 2) return;
    setDraft("");
    setPending({ kind: "merge", tags: selected });
  };

  const startDelete = () => {
    if (selected.length === 0) return;
    setPending({ kind: "delete", tags: selected });
  };

  const confirm = () => {
    if (!libraryPath || !pending) return;

    const sources = pending.kind === "rename" ? [pending.tag] : pending.tags;
    const target = pending.kind === "delete" ? null : draft.trim();
    if (pending.kind !== "delete" && !target) return;

    applyTags.mutate(
      { sources, target, path: libraryPath },
      {
        onSuccess: () => {
          setPending(null);
          setSelected([]);
        },
      },
    );
  };

  /** 在「我的 Skills」里以这组标签作为筛选条件打开 */
  const openInSkills = () => {
    setSearch(selected.map((t) => `tag:${t}`).join(" "));
    setActiveView("skills");
  };

  if (configLoading) {
    return (
      <EmptyState
        icon={<Loader2 className="animate-spin" />}
        title="正在读取配置…"
        description=""
      />
    );
  }

  if (!libraryPath) {
    return (
      <EmptyState
        icon={<Tags />}
        title="还没有配置中央库"
        description="标签来自中央库里各 Skill 的 SKILL.md，因此需要先指定中央库位置。"
        action={
          <Button onClick={() => setActiveView("settings")}>前往设置</Button>
        }
      />
    );
  }

  if (stats.isPending) {
    return (
      <EmptyState
        icon={<Loader2 className="animate-spin" />}
        title="正在统计标签…"
        description=""
      />
    );
  }

  if (tags.length === 0) {
    return (
      <EmptyState
        icon={<Tags />}
        title="还没有任何标签"
        description={
          <div className="space-y-1">
            <p>
              标签写在每个 Skill 的 <code>SKILL.md</code> frontmatter 里：
            </p>
            <pre className="mx-auto w-fit rounded-md border border-border bg-bg-subtle p-2 text-left font-mono text-xs">
              {"tags:\n  - 写作\n  - 代码审查"}
            </pre>
          </div>
        }
      />
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <div className="sh-plate shrink-0 border-b border-border px-4 py-2 text-[11px] text-fg-subtle">
        共 <strong className="text-fg">{tags.length}</strong> 个标签 ·
        大小写不同的写法已归并 · 改写会写回各 Skill 的 <code>SKILL.md</code>
      </div>

      {/* 标签云：字号随出现次数分档，数量差异一眼可见 */}
      <div className="flex flex-wrap gap-1.5 p-4">
        {tags.map((tag) => (
          <TagChip
            key={tag.tag}
            tag={tag}
            maxCount={maxCount}
            selected={selected.includes(tag.tag)}
            onToggle={() => toggle(tag.tag)}
          />
        ))}
      </div>

      {/* 选中后的操作区 */}
      {selected.length > 0 ? (
        <div className="sh-plate shrink-0 space-y-2 border-t border-border px-4 py-3">
          <div className="flex flex-wrap items-center gap-2 text-xs text-fg-muted">
            <span>
              已选 <strong className="text-fg">{selected.length}</strong> 个标签
              · 匹配 <strong className="text-fg">{matched.length}</strong> 个
              Skill
            </span>
            <Button variant="ghost" size="sm" onClick={() => setSelected([])}>
              <X className="size-3.5" />
              取消选择
            </Button>
          </div>

          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="secondary"
              disabled={matched.length === 0}
              onClick={openInSkills}
            >
              在「我的 Skills」中筛选
            </Button>

            {selected.length === 1 ? (
              <Button
                size="sm"
                variant="secondary"
                disabled={applyTags.isPending}
                onClick={startRename}
              >
                <Pencil className="size-3.5" />
                重命名
              </Button>
            ) : (
              <Button
                size="sm"
                variant="secondary"
                disabled={applyTags.isPending}
                onClick={startMerge}
              >
                合并到…
              </Button>
            )}

            <Button
              size="sm"
              variant="ghost"
              disabled={applyTags.isPending}
              onClick={startDelete}
            >
              <Trash2 className="size-3.5" />
              删除标签
            </Button>
          </div>

          {/* 确认区：改的是用户的文件，动手前把"改成什么"摆清楚 */}
          {pending ? (
            <div className="space-y-2 rounded-md border border-border bg-bg-subtle p-2.5">
              <PendingSummary pending={pending} />

              {pending.kind === "delete" ? (
                <p className="text-xs text-fg-muted">
                  删除只是把标签从各个 Skill 的 frontmatter 里去掉，
                  <strong>不会删除任何 Skill 或文件</strong>。
                </p>
              ) : (
                <Input
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  placeholder="新标签名"
                  aria-label="新标签名"
                  spellCheck={false}
                  autoFocus
                  onKeyDown={(e) => {
                    if (e.key === "Enter") confirm();
                  }}
                />
              )}

              <div className="flex gap-2">
                <Button
                  size="sm"
                  disabled={
                    applyTags.isPending ||
                    (pending.kind !== "delete" && !draft.trim())
                  }
                  onClick={confirm}
                >
                  {applyTags.isPending ? (
                    <Loader2 className="size-3.5 animate-spin" />
                  ) : (
                    <Check className="size-3.5" />
                  )}
                  确认
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={applyTags.isPending}
                  onClick={() => setPending(null)}
                >
                  取消
                </Button>
              </div>
            </div>
          ) : null}
        </div>
      ) : null}

      {/* 匹配到的 Skill：让"这几个标签到底圈住了什么"在动手前就可见 */}
      {selected.length > 0 && matched.length > 0 ? (
        <div className="min-h-0 flex-1 border-t border-border px-4 py-3">
          <div className="mb-2 text-xs font-medium text-fg">
            匹配到的 Skill（{matched.length}）
          </div>
          <ul className="space-y-1">
            {matched.slice(0, 100).map((skill) => (
              <li
                key={skill.id}
                className="flex items-baseline gap-2 text-xs text-fg-muted"
              >
                <span className="truncate text-fg">{skill.name}</span>
                <span className="shrink-0 font-mono text-[10px] text-fg-subtle">
                  {skill.tags.join(" · ")}
                </span>
              </li>
            ))}
          </ul>
          {matched.length > 100 ? (
            <p className="mt-2 text-[11px] text-fg-subtle">
              只列出前 100 个，共 {matched.length} 个。
            </p>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function PendingSummary({ pending }: { pending: NonNullable<PendingAction> }) {
  switch (pending.kind) {
    case "rename":
      return (
        <p className="text-xs text-fg-muted">
          把标签 <code className="font-mono text-fg">{pending.tag}</code> 改成：
        </p>
      );
    case "merge":
      return (
        <p className="text-xs text-fg-muted">
          把这 {pending.tags.length} 个标签合并成一个：
          <span className="ml-1 font-mono text-fg">
            {pending.tags.join("、")}
          </span>
        </p>
      );
    case "delete":
      return (
        <p className="text-xs text-fg-muted">
          删除标签
          <span className="mx-1 font-mono text-fg">
            {pending.tags.join("、")}
          </span>
          ？
        </p>
      );
  }
}

function TagChip({
  tag,
  maxCount,
  selected,
  onToggle,
}: {
  tag: TagCount;
  maxCount: number;
  selected: boolean;
  onToggle: () => void;
}) {
  // 字号按出现次数分三档：不追求连续缩放，否则云端会花得看不清
  const ratio = maxCount > 0 ? tag.count / maxCount : 0;
  const sizeClass =
    ratio > 0.66 ? "text-sm" : ratio > 0.33 ? "text-xs" : "text-[11px]";

  return (
    <Tooltip label={`匹配 ${tag.count} 个 Skill`}>
      <button
        type="button"
        onClick={onToggle}
        aria-pressed={selected}
        className={cn(
          "flex items-center gap-1 rounded-full border px-2.5 py-1 transition-colors",
          sizeClass,
          selected
            ? "border-accent bg-accent-subtle font-medium text-accent"
            : "border-border bg-surface text-fg-muted hover:border-border-strong hover:text-fg",
        )}
      >
        {tag.tag}
        <span className="tabular-nums text-[10px] text-fg-subtle">
          {tag.count}
        </span>
      </button>
    </Tooltip>
  );
}
