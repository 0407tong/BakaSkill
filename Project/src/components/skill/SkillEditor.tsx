import { useMemo, useState } from "react";
import Editor from "@monaco-editor/react";
import { Eye, Loader2, Pencil, Save, X } from "lucide-react";

import { cn } from "@/lib/cn";
import { Button } from "@/components/ui/Button";
import { Tooltip } from "@/components/ui/Tooltip";
import "@/lib/monaco"; // 必须在使用 Editor 之前引入：配置本地 Monaco 与 worker
import { useSaveSkillFile, useSkillFile } from "@/lib/queries";
import { useViewStore } from "@/store/viewStore";

export interface SkillEditorProps {
  /** Skill 的目录路径（后端会定位其中的 SKILL.md） */
  dirPath: string;
  className?: string;
}

/**
 * SKILL.md 编辑器（Monaco 封装）。
 *
 * 两个关键点：
 * 1. **Monaco 本地打包** —— 见 `src/lib/monaco.ts`。默认走 CDN 的加载方式
 *    会让桌面应用在离线时白屏。
 * 2. **保存前由后端校验** —— 内容必须先能被解析成合法 frontmatter 才落盘，
 *    避免一次误编辑就把用户的 Skill 变成索引无法识别的坏文件。
 */
export function SkillEditor({ dirPath, className }: SkillEditorProps) {
  const themeMode = useViewStore((s) => s.themeMode);
  const { data: file, isPending, isError, error } = useSkillFile(dirPath);
  const save = useSaveSkillFile();

  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<string | null>(null);

  // 切换 Skill 时重置编辑态由父组件通过 key 触发组件重挂载完成，
  // 不在这里用 effect 同步 setState——那会引发级联渲染
  // （eslint react-hooks 规则会直接报错）。

  const original = file?.content ?? "";
  const current = draft ?? original;
  const dirty = draft !== null && draft !== original;

  const monacoTheme = useMemo(
    () => (themeMode === "dark" ? "vs-dark" : "vs"),
    [themeMode],
  );

  if (isPending) {
    return (
      <div
        className={cn(
          "flex items-center justify-center gap-2 text-sm text-fg-muted",
          className,
        )}
      >
        <Loader2 className="size-4 animate-spin" />
        正在读取 SKILL.md…
      </div>
    );
  }

  if (isError) {
    return (
      <div className={cn("p-3 text-sm text-danger", className)}>
        {error instanceof Error ? error.message : "读取 SKILL.md 失败"}
      </div>
    );
  }

  return (
    <div className={cn("flex min-h-0 flex-1 flex-col", className)}>
      <div className="flex h-9 shrink-0 items-center justify-between gap-2 border-b border-border px-2">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate text-xs text-fg-subtle" title={file?.path}>
            {file?.path}
          </span>
          {dirty ? (
            <span className="shrink-0 text-[11px] text-warning">未保存</span>
          ) : null}
        </div>

        <div className="flex shrink-0 items-center gap-1">
          {editing ? (
            <>
              <Tooltip label="放弃修改">
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="放弃修改"
                  onClick={() => {
                    setDraft(null);
                    setEditing(false);
                  }}
                >
                  <X className="size-3.5" />
                </Button>
              </Tooltip>
              <Button
                size="sm"
                disabled={!dirty || save.isPending}
                onClick={() => save.mutate({ dirPath, content: current })}
              >
                {save.isPending ? (
                  <Loader2 className="size-3.5 animate-spin" />
                ) : (
                  <Save className="size-3.5" />
                )}
                保存
              </Button>
            </>
          ) : (
            <Tooltip label="编辑">
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="编辑"
                onClick={() => setEditing(true)}
              >
                <Pencil className="size-3.5" />
              </Button>
            </Tooltip>
          )}
        </div>
      </div>

      <div className="min-h-0 flex-1">
        {editing ? (
          <Editor
            height="100%"
            language="markdown"
            theme={monacoTheme}
            value={current}
            onChange={(value) => setDraft(value ?? "")}
            options={{
              fontSize: 12.5,
              minimap: { enabled: false },
              lineNumbers: "on",
              wordWrap: "on",
              scrollBeyondLastLine: false,
              renderWhitespace: "selection",
              tabSize: 2,
              automaticLayout: true,
            }}
          />
        ) : (
          <div className="flex h-full flex-col">
            <div className="flex items-center gap-1.5 border-b border-border px-2 py-1 text-[11px] text-fg-subtle">
              <Eye className="size-3" />
              只读预览
            </div>
            <div className="min-h-0 flex-1">
              <Editor
                height="100%"
                language="markdown"
                theme={monacoTheme}
                value={current}
                options={{
                  readOnly: true,
                  fontSize: 12.5,
                  minimap: { enabled: false },
                  wordWrap: "on",
                  scrollBeyondLastLine: false,
                  automaticLayout: true,
                  renderLineHighlight: "none",
                }}
              />
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
