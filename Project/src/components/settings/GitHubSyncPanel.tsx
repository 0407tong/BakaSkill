import { useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  GitBranch,
  KeyRound,
  Loader2,
  Lock,
  LogOut,
  RefreshCw,
  Search,
} from "lucide-react";

import { cn } from "@/lib/cn";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";
import { Tooltip } from "@/components/ui/Tooltip";
import {
  useCleanupStaleRepo,
  useConfig,
  useGitLoginMethods,
  useGitLogout,
  useGitRepos,
  useGitStatus,
  useGitSyncStatus,
  useLoginWithBrowser,
  usePull,
  useSaveGitToken,
  useSetGitRemote,
  useStaleRepo,
  useSyncNow,
} from "@/lib/queries";
import type { GitAvailability, GitHubRepo } from "@/types/ipc";

/**
 * GitHub 同步面板：**登录**与**选择仓库**两个面向用户的接口。
 *
 * # 关于 Token 的处理
 *
 * Token 只经由一次 IPC 调用送入后端，随后立即写入 **Windows 凭据管理器**。
 * 它**不会**被写进 `config.json`、不会出现在日志里，
 * 也不会从后端读回前端（`git_status` 只返回"有没有"，不返回内容）。
 */
export function GitHubSyncPanel() {
  const status = useGitStatus();
  const loggedIn = Boolean(
    status.data?.hasToken || status.data?.hasHelperCredentials,
  );

  return (
    <section
      className="rounded-lg border border-border bg-surface"
      aria-labelledby="github-sync-heading"
    >
      <header className="flex items-center gap-2 border-b border-border px-4 py-3">
        <GitBranch className="size-4 text-fg-subtle" />
        <h2 id="github-sync-heading" className="text-sm font-medium text-fg">
          GitHub 同步
        </h2>
        {loggedIn ? (
          <Badge variant="success">
            <CheckCircle2 />
            已登录
            {status.data?.hasHelperCredentials && !status.data?.hasToken
              ? "（账号）"
              : ""}
          </Badge>
        ) : (
          <Badge variant="outline">未登录</Badge>
        )}
        {/* 退出登录只有这一处入口。早先分散在两个区块里，各自只清一半凭据，
            用户两个都按了界面还是"已登录" */}
        {loggedIn ? <LogoutButton className="ml-auto" /> : null}
      </header>

      <div className="space-y-4 p-4">
        {status.isPending ? (
          <div className="flex items-center gap-2 text-sm text-fg-muted">
            <Loader2 className="size-3.5 animate-spin" />
            正在检查 git 环境…
          </div>
        ) : status.data && !status.data.available ? (
          /* git 未安装：给出明确阻断原因，而不是让后续操作静默失败 */
          <div className="flex items-start gap-2 rounded-md border border-danger/40 bg-danger-subtle p-3 text-sm text-danger">
            <AlertTriangle className="mt-0.5 size-4 shrink-0" />
            <div className="space-y-1">
              <p className="font-medium">未检测到 git</p>
              <p className="text-xs leading-relaxed">
                GitHub 同步依赖系统上的 git 命令行工具。 请先安装（
                <code className="font-mono">winget install Git.Git</code>
                ）后重开本应用。
              </p>
            </div>
          </div>
        ) : status.data ? (
          <>
            <LoginSection status={status.data} />
            <StaleRepoNotice />
            {status.data.hasToken || status.data.hasHelperCredentials ? (
              <>
                <RepoSection status={status.data} />
                {status.data.remoteUrl ? <SyncSection /> : null}
              </>
            ) : null}
          </>
        ) : null}
      </div>
    </section>
  );
}

/**
 * 旧版本曾在**中央库目录本身**里 `git init`。新模型下中央库只是数据目录，
 * 那个 `.git` 不再被任何代码使用，留着只会让用户以为这里是代码仓库。
 *
 * 后端做的是**重命名**而不是删除，所以这里可以放心地把它做成一个按钮——
 * 最坏情况也只是多出一个 `.git.bakaskill-old-<时间戳>` 目录，用户自己处理即可。
 * （改名之前产生的那批残留叫 `.git.skillhub-old-*`，它们只是被挪到一旁的目录，不受影响。）
 */
function StaleRepoNotice() {
  const { data: config } = useConfig();
  const libraryPath = config?.centralLibraryPath ?? null;
  const stale = useStaleRepo(libraryPath);
  const cleanup = useCleanupStaleRepo();

  if (!libraryPath || !stale.data) return null;

  return (
    <div className="flex items-start gap-2 rounded-md border border-warning/40 bg-warning-subtle p-3">
      <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" />
      <div className="space-y-1.5">
        <p className="text-xs font-medium text-warning">
          中央库里有一个旧的 git 仓库
        </p>
        <p className="text-xs leading-relaxed text-fg-muted">
          旧版本把中央库本身当作 git 工作区，因此在这里留下了{" "}
          <code className="font-mono">.git</code>
          。现在同步改用独立的同步工作区，
          中央库已经不需要它，留着只会让人误以为这里是代码仓库。
        </p>
        <Button
          size="sm"
          variant="secondary"
          disabled={cleanup.isPending}
          onClick={() => cleanup.mutate(libraryPath)}
        >
          {cleanup.isPending ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : null}
          移到一旁（不删除）
        </Button>
      </div>
    </div>
  );
}

/* ==========================================================================
   退出登录
   ========================================================================== */

/**
 * 退出 GitHub 登录。
 *
 * # 为什么要有确认框，而且要把代价说清楚
 *
 * 本应用允许直接沿用**系统里已有的** GitHub 登录（系统的 git 凭据助手里
 * 有 github.com 的凭据，就显示"已登录"）。因此"退干净"就必然要连系统那份
 * 一起删——而那份是这台电脑上**其它** git 操作用着的。
 *
 * 不说清楚就删，用户会以为只是退出了这个应用，下次在命令行 push 时
 * 突然被要求登录，却不知道为什么。所以确认框里把这句话明写出来。
 */
function LogoutButton({ className }: { className?: string }) {
  const [open, setOpen] = useState(false);
  const logout = useGitLogout();

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="ghost" size="sm" className={className}>
          <LogOut className="size-3.5" />
          退出登录
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>退出 GitHub 登录？</DialogTitle>
          <DialogDescription>
            会把保存在本应用与<strong>系统凭据管理器</strong>里的 GitHub
            凭据一起清掉。 清掉之后，这台电脑上其它地方用 git 推送 GitHub
            <strong>也需要重新登录一次</strong>。
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button
            variant="danger"
            disabled={logout.isPending}
            onClick={() =>
              logout.mutate(undefined, {
                onSettled: () => setOpen(false),
              })
            }
          >
            {logout.isPending ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <LogOut className="size-4" />
            )}
            退出登录
          </Button>
          <DialogClose asChild>
            <Button variant="ghost" disabled={logout.isPending}>
              取消
            </Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/* ==========================================================================
   接口一：登录
   ========================================================================== */

function LoginSection({ status }: { status: GitAvailability }) {
  const saveToken = useSaveGitToken();
  const [token, setToken] = useState("");
  const [reveal, setReveal] = useState(false);

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-1.5">
        <KeyRound className="size-3.5 text-fg-subtle" />
        <h3 className="text-xs font-medium text-fg">登录</h3>
        {status.version ? (
          <span className="text-[11px] text-fg-subtle">{status.version}</span>
        ) : null}
      </div>

      {/* 方式一：GitHub 账号登录（推荐，无需创建 Token） */}
      <AccountLoginSection status={status} />

      {/* 方式二：Token（兜底） */}
      <div className="text-[11px] font-medium text-fg-subtle">
        方式二 · Personal Access Token
      </div>

      {status.hasToken ? (
        <div className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-bg-subtle p-2.5">
          <Lock className="size-3.5 shrink-0 text-success" />
          <span className="text-xs text-fg-muted">
            凭据已保存在 Windows 凭据管理器（不落明文文件）
          </span>
        </div>
      ) : (
        <div className="space-y-2">
          <p className="text-xs leading-relaxed text-fg-muted">
            需要一个 GitHub Personal Access Token（细粒度即可， 只需勾选{" "}
            <strong>Contents: Read and write</strong>）。 它只会被写入 Windows
            凭据管理器。
          </p>

          <div className="flex gap-2">
            <Input
              type={reveal ? "text" : "password"}
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="github_pat_..."
              aria-label="GitHub Token"
              autoComplete="off"
              spellCheck={false}
              leading={<KeyRound />}
            />
            <Tooltip label={reveal ? "隐藏" : "显示"}>
              <Button
                variant="secondary"
                size="icon"
                onClick={() => setReveal((v) => !v)}
                aria-label={reveal ? "隐藏 Token" : "显示 Token"}
              >
                {reveal ? "隐藏" : "显示"}
              </Button>
            </Tooltip>
            <Button
              disabled={!token.trim() || saveToken.isPending}
              onClick={() => {
                saveToken.mutate(token.trim(), {
                  onSuccess: () => setToken(""),
                });
              }}
            >
              {saveToken.isPending ? (
                <Loader2 className="size-4 animate-spin" />
              ) : null}
              登录
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

/**
 * 方式一：用 GitHub 账号登录（浏览器授权）—— **推荐**。
 *
 * 走系统已装的 Git Credential Manager：点一下会打开浏览器，
 * 用 GitHub 账号确认授权即可，**没有创建/复制 Token 的步骤**。
 *
 * 为什么不提供"账号 + 密码"：GitHub 自 2021 年 8 月起已移除
 * git 操作的密码认证，账号密码这条路在服务端就被拒了，不是实现取舍。
 */
function AccountLoginSection({ status }: { status: GitAvailability }) {
  const methods = useGitLoginMethods();
  const login = useLoginWithBrowser();

  const gcmReady = methods.data?.gcmAvailable ?? false;
  const loggedIn = status.hasHelperCredentials;

  return (
    <div className="space-y-1.5">
      <div className="text-[11px] font-medium text-fg-subtle">
        方式一 · GitHub 账号登录（推荐）
      </div>

      {loggedIn ? (
        <div className="flex flex-wrap items-center gap-2 rounded-md border border-success/40 bg-success-subtle p-2.5">
          <CheckCircle2 className="size-3.5 shrink-0 text-success" />
          <span className="text-xs text-fg-muted">
            已用 GitHub 账号登录，凭据由系统凭据管理器保存
          </span>
        </div>
      ) : (
        <div className="space-y-1.5">
          <p className="text-xs leading-relaxed text-fg-muted">
            点下面的按钮会打开浏览器，用 GitHub 账号确认授权即可，
            <strong>不需要创建或复制 Token</strong>。
          </p>
          <Tooltip label={gcmReady ? "" : "未检测到 Git Credential Manager"}>
            <span>
              <Button
                variant="secondary"
                size="sm"
                disabled={login.isPending || !gcmReady}
                onClick={() => login.mutate()}
              >
                {login.isPending ? (
                  <Loader2 className="size-3.5 animate-spin" />
                ) : (
                  <GitBranch className="size-3.5" />
                )}
                {login.isPending ? "等待浏览器授权…" : "用 GitHub 账号登录"}
              </Button>
            </span>
          </Tooltip>
          {!gcmReady && methods.data ? (
            <p className="text-[11px] leading-relaxed text-warning">
              未找到 Git Credential Manager，账号登录不可用。 请安装 Git for
              Windows（自带 GCM），或改用下方的 Token 方式。
            </p>
          ) : null}
        </div>
      )}
    </div>
  );
}

/* ==========================================================================
   接口二：选择仓库
   ========================================================================== */

function RepoSection({ status }: { status: GitAvailability }) {
  const [loadRepos, setLoadRepos] = useState(false);
  const repos = useGitRepos(loadRepos);
  const setRemote = useSetGitRemote();
  const [filter, setFilter] = useState("");
  const [manualUrl, setManualUrl] = useState("");
  const [showManual, setShowManual] = useState(false);

  const visible = (repos.data ?? []).filter((r) =>
    r.fullName.toLowerCase().includes(filter.trim().toLowerCase()),
  );

  return (
    <div className="space-y-2 border-t border-border pt-3">
      <div className="flex items-center gap-1.5">
        <GitBranch className="size-3.5 text-fg-subtle" />
        <h3 className="text-xs font-medium text-fg">选择仓库</h3>
      </div>

      {status.remoteUrl ? (
        <div className="space-y-1 rounded-md border border-border bg-bg-subtle p-2.5">
          <div className="text-[11px] text-fg-subtle">当前绑定</div>
          <code
            className="block truncate font-mono text-xs text-fg"
            title={status.remoteUrl}
          >
            {status.remoteUrl}
          </code>
          <div className="text-[11px] text-fg-subtle">
            分支：<span className="font-mono">{status.branch}</span>
          </div>
        </div>
      ) : (
        <p className="text-xs text-fg-muted">
          尚未绑定仓库。选择一个用于备份中央库的仓库。
        </p>
      )}

      {/* 两种登录方式都能拉取仓库列表：账号登录时去向系统凭据助手取
          OAuth Token（凭据已存在，取用不会触发交互）。 */}
      <div className="flex flex-wrap gap-2">
        <Button
          variant="secondary"
          size="sm"
          onClick={() => setLoadRepos(true)}
          disabled={repos.isFetching}
        >
          <RefreshCw
            className={cn("size-3.5", repos.isFetching && "animate-spin")}
          />
          {repos.data ? "重新加载仓库列表" : "加载仓库列表"}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => setShowManual((v) => !v)}
        >
          手动填写地址
        </Button>
      </div>

      {showManual ? (
        <div className="flex gap-2">
          <Input
            value={manualUrl}
            onChange={(e) => setManualUrl(e.target.value)}
            placeholder="https://github.com/用户名/仓库.git"
            aria-label="仓库地址"
            spellCheck={false}
          />
          <Button
            disabled={!manualUrl.trim() || setRemote.isPending}
            onClick={() =>
              setRemote.mutate(
                { url: manualUrl.trim() },
                { onSuccess: () => setManualUrl("") },
              )
            }
          >
            绑定
          </Button>
        </div>
      ) : null}

      {repos.isError ? (
        <div className="flex items-start gap-2 rounded-md border border-danger/40 bg-danger-subtle p-2.5 text-xs text-danger">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
          <span>
            {repos.error instanceof Error
              ? repos.error.message
              : "加载仓库列表失败"}
          </span>
        </div>
      ) : null}

      {repos.data ? (
        <>
          <Input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="筛选仓库…"
            aria-label="筛选仓库"
            leading={<Search />}
          />

          {visible.length === 0 ? (
            <p className="py-3 text-center text-xs text-fg-subtle">
              没有匹配的仓库。可以在 GitHub 上新建一个空仓库后重新加载。
            </p>
          ) : (
            <ul className="max-h-64 space-y-1 overflow-y-auto">
              {visible.map((repo) => (
                <RepoRow
                  key={repo.fullName}
                  repo={repo}
                  selected={status.remoteUrl === repo.cloneUrl}
                  busy={setRemote.isPending}
                  onPick={() =>
                    setRemote.mutate({
                      url: repo.cloneUrl,
                      branch: repo.defaultBranch ?? undefined,
                    })
                  }
                />
              ))}
            </ul>
          )}
        </>
      ) : null}
    </div>
  );
}

/* ==========================================================================
   同步操作
   ========================================================================== */

function SyncSection() {
  const { data: config } = useConfig();
  const libraryPath = config?.centralLibraryPath ?? null;
  const status = useGitSyncStatus();
  const syncNow = useSyncNow();
  const pull = usePull();

  if (!libraryPath) return null;

  const busy = syncNow.isPending || pull.isPending;
  const s = status.data;

  return (
    <div className="space-y-2 border-t border-border pt-3">
      <div className="flex items-center gap-1.5">
        <RefreshCw className="size-3.5 text-fg-subtle" />
        <h3 className="text-xs font-medium text-fg">同步</h3>
        <Badge variant="outline">
          {s?.isRepo ? "已建立同步工作区" : "尚未同步过"}
        </Badge>
      </div>

      {status.isPending ? (
        <div className="flex items-center gap-2 text-xs text-fg-muted">
          <Loader2 className="size-3.5 animate-spin" />
          正在读取状态…
        </div>
      ) : (
        <>
          {/* 换行处不会在中文里插入空格：JSX 会丢掉「文本与标签相邻」的那个
              换行。若哪天要给整段加上句子间的空格，得显式写 `{" "}`。 */}
          <p className="text-xs leading-relaxed text-fg-muted">
            同步会把中央库里的 Skill 传到上面这个仓库。仓库里已有的其他内容
            <strong>不会被删除，也不会被改动</strong>。
          </p>

          {/* 状态卡：让"现在有多少东西要传"一目了然 */}
          {s?.isRepo ? (
            <>
              <div className="grid grid-cols-3 gap-3 rounded-md border border-border bg-bg-subtle p-2.5 text-xs">
                <div>
                  <div className="text-[11px] text-fg-subtle">待同步改动</div>
                  <div className="font-medium text-fg">{s.changedCount}</div>
                </div>
                <div>
                  <div className="text-[11px] text-fg-subtle">未推送</div>
                  <div className="font-medium text-fg">{s.ahead}</div>
                </div>
                <div>
                  <div className="text-[11px] text-fg-subtle">仓库新增</div>
                  <div className="font-medium text-fg">{s.behind}</div>
                </div>
              </div>

              {/* 变更明细预览 */}
              {s.changedCount > 0 ? (
                <details className="rounded-md border border-border bg-bg-subtle p-2">
                  <summary className="cursor-pointer text-[11px] text-fg-muted">
                    查看 {s.changedCount} 项变更
                  </summary>
                  <ul className="mt-1.5 max-h-40 space-y-0.5 overflow-y-auto font-mono text-[10px]">
                    {s.added.map((p) => (
                      <li key={`a-${p}`} className="text-success">
                        + {p}
                      </li>
                    ))}
                    {s.modified.map((p) => (
                      <li key={`m-${p}`} className="text-warning">
                        ~ {p}
                      </li>
                    ))}
                    {s.removed.map((p) => (
                      <li key={`d-${p}`} className="text-fg-subtle">
                        − {p}
                      </li>
                    ))}
                  </ul>
                  {/* 本地删掉的 Skill 不会从仓库里删掉——同步的承诺是
                      "绝不删除仓库中已有的内容"，所以这里必须说清楚，
                      否则用户会以为点了同步就能把仓库里那份清掉。 */}
                  {s.removed.length > 0 ? (
                    <p className="mt-1.5 text-[10px] leading-relaxed text-fg-subtle">
                      标 − 的是本地已删除的 Skill。同步
                      <strong>不会</strong>
                      把它们从仓库里删掉（万一删除是误操作，仓库里的那份还在）。
                      如需清理，请在 GitHub 上手动删除。
                    </p>
                  ) : null}
                </details>
              ) : null}
            </>
          ) : null}

          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              disabled={busy}
              onClick={() => syncNow.mutate(libraryPath)}
            >
              {syncNow.isPending ? (
                <Loader2 className="size-3.5 animate-spin" />
              ) : (
                <GitBranch className="size-3.5" />
              )}
              {s?.isRepo ? "立即同步" : "同步到仓库"}
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={busy}
              onClick={() => pull.mutate(libraryPath)}
            >
              {pull.isPending ? (
                <Loader2 className="size-3.5 animate-spin" />
              ) : null}
              取回缺失的 Skill
            </Button>
          </div>

          {/* 仓库里有本地还没有的提交。这在"借用用户已有仓库"的模型里是常态
              （大多来自另一台机器），不是错误——任一按钮都会先取回它们。 */}
          {s?.isRepo && s.behind > 0 ? (
            <p className="text-[11px] leading-relaxed text-fg-subtle">
              仓库里还有 {s.behind}{" "}
              个本地没有的提交。点上面任一按钮都会先取回它们， 再继续你的操作。
            </p>
          ) : null}

          {s?.lastCommit ? (
            <div className="truncate font-mono text-[11px] text-fg-subtle">
              最近提交：{s.lastCommit}
            </div>
          ) : null}

          <p className="text-[11px] leading-relaxed text-fg-subtle">
            「取回缺失的 Skill」只做补齐：中央库里已有的同名 Skill 会被跳过，
            不会被仓库里的版本覆盖。
          </p>
        </>
      )}
    </div>
  );
}

function RepoRow({
  repo,
  selected,
  busy,
  onPick,
}: {
  repo: GitHubRepo;
  selected: boolean;
  busy: boolean;
  onPick: () => void;
}) {
  return (
    <li>
      <button
        type="button"
        disabled={busy}
        onClick={onPick}
        className={cn(
          "flex w-full items-center gap-2 rounded-md border px-2.5 py-2 text-left transition-colors",
          selected
            ? "border-accent bg-accent-subtle"
            : "border-border hover:bg-surface-hover",
        )}
      >
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <span className="truncate text-xs font-medium text-fg">
              {repo.fullName}
            </span>
            {repo.private ? (
              <Badge variant="outline">
                <Lock />
                私有
              </Badge>
            ) : null}
            {selected ? <Badge variant="accent">当前</Badge> : null}
          </div>
          {repo.description ? (
            <div className="truncate text-[11px] text-fg-subtle">
              {repo.description}
            </div>
          ) : null}
        </div>
      </button>
    </li>
  );
}
