import { useState } from "react";
import {
  Check,
  FolderPlus,
  Loader2,
  Pencil,
  Plus,
  RotateCcw,
  Trash2,
} from "lucide-react";

import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import {
  useAgentsDetect,
  useConfig,
  useCreateAgentSkillDir,
  useSetConfig,
} from "@/lib/queries";
import { cn } from "@/lib/cn";
import type { AgentDetection, ConfigView } from "@/types/ipc";

/** Agent 状态 → 徽章文案与样式 */
const STATUS_META: Record<
  AgentDetection["status"],
  { label: string; variant: "success" | "warning" | "outline" }
> = {
  detected: { label: "已就绪", variant: "success" },
  installedNoSkillDir: { label: "技能目录尚未创建", variant: "warning" },
  notInstalled: { label: "未安装", variant: "outline" },
};

/**
 * Agent 技能目录。
 *
 * # 这里解开的是哪个死角
 *
 * 应用只把**技能目录已存在**的 Agent 视为可用目标（矩阵与启用路径都只认
 * `Detected`）。有些 Agent 的技能目录是**按需创建**的——本机的 Claude Code 就是：
 * `~/.claude` 在、`~/.claude/skills` 不在。
 *
 * 在这张卡片存在之前，那种状态是**没有出路**的：用户点了"启用"，只有别的 Agent
 * 生效，界面也不说为什么；Trae 那种连路径都探不到的（候选路径当初实测证伪、
 * 故意留空）更是完全接不进来。
 *
 * 因此这里做两件事：**能创建的给出创建按钮，不能探到的让人自己填**。
 *
 * # 两条红线
 *
 * - **未安装的 Agent 不提供创建**：那会在你机器上凭空多出一堆属于你没装的软件的目录。
 * - **路径未知的不猜**：Trae 的位置必须由你给出，应用不替它编一个。
 */
export function AgentDirSettings() {
  const { data: config } = useConfig();
  const detections = useAgentsDetect();
  const setConfig = useSetConfig();
  const createDir = useCreateAgentSkillDir();

  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [adding, setAdding] = useState(false);
  const [newAgent, setNewAgent] = useState({
    id: "",
    displayName: "",
    skillDir: "",
  });

  if (!config || !detections.data) return null;

  const agents = detections.data;
  // 未安装的排在最后并折叠：它们是"这台机器上没有"，不是需要用户处理的事
  const visible = agents.filter((a) => a.status !== "notInstalled");
  const notInstalled = agents.filter((a) => a.status === "notInstalled");

  /** 写入 config.agents 的一条（没有就追加） */
  const patchAgent = (
    id: string,
    patch: {
      skillDir?: string | null;
      displayName?: string | null;
      enabled?: boolean;
    },
  ) => {
    const existing = config.agents.find((a) => a.id === id);
    const next = existing
      ? config.agents.map((a) => (a.id === id ? { ...a, ...patch } : a))
      : [
          ...config.agents,
          {
            id,
            enabled: patch.enabled ?? true,
            skillDir: patch.skillDir ?? null,
            displayName: patch.displayName ?? null,
          },
        ];
    save({ ...config, agents: next });
  };

  const removeAgent = (id: string) => {
    save({ ...config, agents: config.agents.filter((a) => a.id !== id) });
  };

  const save = (next: ConfigView) => {
    setConfig.mutate(next, { onSuccess: () => setEditing(null) });
  };

  const startEdit = (agent: AgentDetection) => {
    setEditing(agent.id);
    setDraft(agent.skillDir ?? "");
  };

  const busy = setConfig.isPending || createDir.isPending;

  return (
    <section
      className="rounded-lg border border-border bg-surface"
      aria-labelledby="agent-dir-heading"
    >
      <header className="flex items-center gap-2 border-b border-border px-4 py-3">
        <FolderPlus className="size-4 text-fg-subtle" />
        <h2 id="agent-dir-heading" className="text-sm font-medium text-fg">
          Agent 技能目录
        </h2>
      </header>

      <div className="space-y-2 p-4">
        <p className="text-xs leading-relaxed text-fg-muted">
          只有<strong>技能目录已存在</strong>的 Agent
          才会出现在「启用」的目标里。 有些 Agent
          的目录是按需创建的——没建之前它接不进来，这里可以建；
          探不到位置的可以自己填。
        </p>

        <ul className="space-y-1.5">
          {visible.map((agent) => {
            const meta = STATUS_META[agent.status];
            const override =
              config.agents.find((a) => a.id === agent.id)?.skillDir ?? null;
            const isBuiltin = agent.detectedBy.length > 0 || !override;

            return (
              <li
                key={agent.id}
                className="rounded-md border border-border bg-bg-subtle p-2.5"
              >
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-xs font-medium text-fg">
                    {agent.displayName}
                  </span>
                  <Badge variant={meta.variant}>{meta.label}</Badge>
                  {override ? (
                    <Badge variant="accent">你指定的</Badge>
                  ) : !isBuiltin ? (
                    <Badge variant="outline">自定义</Badge>
                  ) : null}

                  <div className="ml-auto flex items-center gap-1.5">
                    {agent.status === "installedNoSkillDir" &&
                    agent.skillDir ? (
                      <Button
                        size="sm"
                        variant="secondary"
                        disabled={busy}
                        onClick={() => createDir.mutate(agent.id)}
                      >
                        {createDir.isPending ? (
                          <Loader2 className="size-3.5 animate-spin" />
                        ) : (
                          <Plus className="size-3.5" />
                        )}
                        创建此目录
                      </Button>
                    ) : null}

                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={busy}
                      onClick={() => startEdit(agent)}
                    >
                      <Pencil className="size-3.5" />
                      {override ? "改目录" : "指定目录"}
                    </Button>

                    {override ? (
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={busy}
                        onClick={() => patchAgent(agent.id, { skillDir: null })}
                        aria-label="恢复默认路径"
                      >
                        <RotateCcw className="size-3.5" />
                      </Button>
                    ) : null}

                    {!isBuiltin && override ? (
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={busy}
                        onClick={() => removeAgent(agent.id)}
                        aria-label="移除这个自定义 Agent"
                      >
                        <Trash2 className="size-3.5" />
                      </Button>
                    ) : null}
                  </div>
                </div>

                <div className="mt-1 truncate font-mono text-[11px] text-fg-subtle">
                  {agent.skillDir ?? "（位置尚未探明）"}
                </div>

                {/* 探不到路径时的解释：这是 Trae 那一类 */}
                {!agent.skillDir && agent.notes ? (
                  <p className="mt-1 text-[11px] leading-relaxed text-fg-subtle">
                    {agent.notes}
                  </p>
                ) : null}

                {editing === agent.id ? (
                  <div className="mt-2 flex gap-2">
                    <Input
                      value={draft}
                      onChange={(e) => setDraft(e.target.value)}
                      placeholder="例如 C:\Users\你\.claude\skills"
                      aria-label={`${agent.displayName} 的技能目录`}
                      spellCheck={false}
                      autoFocus
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && draft.trim()) {
                          patchAgent(agent.id, { skillDir: draft.trim() });
                        }
                        if (e.key === "Escape") setEditing(null);
                      }}
                    />
                    <Button
                      disabled={!draft.trim() || busy}
                      onClick={() =>
                        patchAgent(agent.id, { skillDir: draft.trim() })
                      }
                    >
                      {busy ? (
                        <Loader2 className="size-4 animate-spin" />
                      ) : (
                        <Check className="size-4" />
                      )}
                      保存
                    </Button>
                    <Button variant="ghost" onClick={() => setEditing(null)}>
                      取消
                    </Button>
                  </div>
                ) : null}
              </li>
            );
          })}
        </ul>

        {/* 新增自定义 Agent */}
        {adding ? (
          <div className="space-y-2 rounded-md border border-border bg-bg-subtle p-2.5">
            <div className="flex gap-2">
              <Input
                value={newAgent.id}
                onChange={(e) =>
                  setNewAgent({ ...newAgent, id: e.target.value })
                }
                placeholder="id（英文，唯一）"
                aria-label="新 Agent 的 id"
                spellCheck={false}
              />
              <Input
                value={newAgent.displayName}
                onChange={(e) =>
                  setNewAgent({ ...newAgent, displayName: e.target.value })
                }
                placeholder="显示名"
                aria-label="新 Agent 的显示名"
              />
            </div>
            <Input
              value={newAgent.skillDir}
              onChange={(e) =>
                setNewAgent({ ...newAgent, skillDir: e.target.value })
              }
              placeholder="技能目录，例如 D:\my-agent\skills"
              aria-label="新 Agent 的技能目录"
              spellCheck={false}
            />
            <div className="flex gap-2">
              <Button
                size="sm"
                disabled={
                  !newAgent.id.trim() || !newAgent.skillDir.trim() || busy
                }
                onClick={() => {
                  patchAgent(newAgent.id.trim(), {
                    skillDir: newAgent.skillDir.trim(),
                    displayName: newAgent.displayName.trim() || null,
                  });
                  setNewAgent({ id: "", displayName: "", skillDir: "" });
                  setAdding(false);
                }}
              >
                添加
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => setAdding(false)}
              >
                取消
              </Button>
            </div>
            <p className="text-[11px] text-fg-subtle">
              用于注册表里还没有的 Agent：填上它放 Skill
              的目录即可纳入统一管理。
            </p>
          </div>
        ) : (
          <Button size="sm" variant="secondary" onClick={() => setAdding(true)}>
            <Plus className="size-3.5" />
            新增自定义 Agent
          </Button>
        )}

        {/* 未安装的折起来：它们是"这台机器上没有"，不是待办 */}
        {notInstalled.length > 0 ? (
          <details className="pt-1">
            <summary className="cursor-pointer text-[11px] text-fg-subtle">
              {notInstalled.length} 个 Agent 未安装（
              {notInstalled.map((a) => a.displayName).join("、")}）
            </summary>
            <p className="mt-1 text-[11px] leading-relaxed text-fg-subtle">
              未安装的 Agent
              不会创建技能目录——那会在你机器上凭空多出属于这些软件的目录。
              装好之后回到这一页即可。
            </p>
          </details>
        ) : null}

        {setConfig.isError ? (
          <p className={cn("text-[11px] text-danger")}>
            保存失败：
            {setConfig.error instanceof Error ? setConfig.error.message : ""}
          </p>
        ) : null}
      </div>
    </section>
  );
}
