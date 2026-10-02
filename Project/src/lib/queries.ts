import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { call, IpcError } from "@/lib/ipc";
import type {
  AdoptResult,
  AgentDetection,
  BackgroundState,
  ConfigView,
  ConflictPolicy,
  GitAvailability,
  GitHubRepo,
  LinkMatrix,
  LinkResult,
  LoginMethods,
  InitializeResult,
  LibraryStats,
  PathDiagnosis,
  PongPayload,
  RebuildReport,
  RelocateResult,
  ScanReport,
  SearchHit,
  SkillFile,
  SkillRecord,
  SyncStatus,
  ExportFormat,
  ExportReport,
  ImportDecision,
  ImportPreview,
  ImportReport,
  ImportSource,
  TagCount,
  TagReport,
  UninstallReport,
} from "@/types/ipc";

/**
 * TanStack Query 的 key 工厂。集中管理便于失效控制。
 */
export const queryKeys = {
  ping: ["ping"] as const,
  config: ["config"] as const,
  libraryValidation: (path: string) => ["library", "validate", path] as const,
  libraryStats: (path: string | null) => ["library", "stats", path] as const,
  skillList: (path: string | null) => ["index", "skills", path] as const,
  skillSearch: (path: string | null, keywords: string) =>
    ["index", "search", path, keywords] as const,
  agents: ["agents", "detect"] as const,
  scan: (path: string | null) => ["scan", path] as const,
  skillFile: (dir: string) => ["skill", "file", dir] as const,
  matrix: ["links", "matrix"] as const,
  gitStatus: ["git", "status"] as const,
  gitRepos: ["git", "repos"] as const,
  // 同步状态只取决于"当前绑定了哪个仓库"，与中央库路径无关
  gitSync: ["git", "sync"] as const,
  tagStats: (path: string | null) => ["tags", "stats", path] as const,
};

/** 连通性探针：验证前端 -> Tauri 命令 -> Rust 的整条链路 */
export function usePing() {
  return useQuery({
    queryKey: queryKeys.ping,
    queryFn: () => call<PongPayload>("ping"),
    staleTime: 0,
    retry: 0,
  });
}

// ===========================================================================
// 配置
// ===========================================================================

export function useConfig() {
  return useQuery({
    queryKey: queryKeys.config,
    queryFn: () => call<ConfigView>("config_get"),
  });
}

export function useSetConfig() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (config: ConfigView) =>
      call<ConfigView>("config_set", { config }),
    onSuccess: (saved) => {
      queryClient.setQueryData(queryKeys.config, saved);
      // 中央库路径可能变了，相关查询需要重新取数
      void queryClient.invalidateQueries({ queryKey: ["library"] });
      void queryClient.invalidateQueries({ queryKey: ["index"] });
    },
  });
}

// ===========================================================================
// 中央库
// ===========================================================================

/**
 * 诊断候选路径。
 *
 * `path` 为 null 时不发请求——避免对空字符串做一次无意义的 IPC 往返。
 */
export function useValidateLibraryPath(path: string | null) {
  return useQuery({
    queryKey: queryKeys.libraryValidation(path ?? ""),
    queryFn: () => call<PathDiagnosis>("library_validate", { path }),
    enabled: Boolean(path && path.trim()),
    // 校验结果依赖文件系统状态，不缓存过久
    staleTime: 5_000,
    retry: 0,
  });
}

export function useLibraryStats(path: string | null) {
  return useQuery({
    queryKey: queryKeys.libraryStats(path),
    queryFn: () => call<LibraryStats>("library_stats", { path }),
    enabled: Boolean(path && path.trim()),
    retry: 0,
  });
}

export function useInitializeLibrary() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (path: string) =>
      call<InitializeResult>("library_init", { path }),
    onSuccess: (result) => {
      void queryClient.invalidateQueries({ queryKey: ["library"] });
      void queryClient.invalidateQueries({ queryKey: ["index"] });
      if (result.alreadyInitialized) {
        toast.info(`该位置已是中央库，现有 ${result.skillCount} 个 Skill`);
      } else {
        toast.success(
          result.created.length > 0
            ? `中央库已创建，共 ${result.created.length} 个目录`
            : "中央库已就绪",
        );
      }
    },
    onError: (error) => {
      toast.error(
        error instanceof IpcError ? error.message : "初始化中央库失败",
      );
    },
  });
}

// ===========================================================================
// 索引
// ===========================================================================

export function useSkillList(path: string | null) {
  return useQuery({
    queryKey: queryKeys.skillList(path),
    queryFn: () => call<SkillRecord[]>("index_list", { path }),
    enabled: Boolean(path && path.trim()),
    retry: 0,
  });
}

/**
 * 全文搜索中央库的 SKILL.md 正文。
 *
 * # 为什么查询键挂在 `["index"]` 下面
 *
 * 搜索命中来自**索引**（一张 FTS5 虚表），而项目里所有会改变索引内容的操作
 * （保存 SKILL.md、纳入 Skill、迁移中央库、重建索引）都已经在失效 `["index"]`。
 * 挂在同一个前缀下，这些失效就自动覆盖了搜索结果——否则每加一处写操作，
 * 就得记得再去失效一次搜索，漏掉一处就是"搜出来的东西已经不存在了"。
 *
 * 关键词为空时不发请求：那不是"搜索"，是"列出全部"，由扫描结果负责。
 */
export function useSkillSearch(keywords: string, path: string | null) {
  const trimmed = keywords.trim();
  return useQuery({
    queryKey: queryKeys.skillSearch(path, trimmed),
    queryFn: () =>
      call<SearchHit[]>("skills_search", { keywords: trimmed, path }),
    enabled: Boolean(trimmed && path && path.trim()),
    // 输入过程中保留上一次结果，避免结果区反复闪空
    placeholderData: (prev) => prev,
    staleTime: 30_000,
    retry: 0,
  });
}

// ===========================================================================
// Agent 检测与扫描
// ===========================================================================

/** 检测各 Agent 的安装状态与技能目录 */
export function useAgentsDetect() {
  return useQuery({
    queryKey: queryKeys.agents,
    queryFn: () => call<AgentDetection[]>("agents_detect"),
    // 检测结果依赖磁盘与 PATH，不长期缓存
    staleTime: 10_000,
    retry: 0,
  });
}

/**
 * 扫描全部 Agent 与中央库，返回统一清单。
 *
 * 扫描是重操作，结果缓存在 query 里；用户点「刷新」时 invalidate 即可。
 */
export function useSkillsScan(libraryPath: string | null) {
  return useQuery({
    queryKey: queryKeys.scan(libraryPath),
    queryFn: () => call<ScanReport>("skills_scan", { libraryPath }),
    staleTime: 30_000,
    retry: 0,
  });
}

// ===========================================================================
// SKILL.md 读写
// ===========================================================================

export function useSkillFile(dirPath: string | null) {
  return useQuery({
    queryKey: queryKeys.skillFile(dirPath ?? ""),
    queryFn: () => call<SkillFile>("skill_read", { dirPath }),
    enabled: Boolean(dirPath),
    retry: 0,
  });
}

export function useSaveSkillFile() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (vars: { dirPath: string; content: string }) =>
      call<SkillFile>("skill_write", vars),
    onSuccess: (file, vars) => {
      queryClient.setQueryData(queryKeys.skillFile(vars.dirPath), file);
      // 内容变了，清单与索引都需要重新取数
      void queryClient.invalidateQueries({ queryKey: ["scan"] });
      void queryClient.invalidateQueries({ queryKey: ["index"] });
      toast.success("已保存");
    },
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "保存失败", {
        description: "文件未被修改。",
      });
    },
  });
}

// ===========================================================================
// 链接启停与映射矩阵
// ===========================================================================

/** 读取链接映射矩阵 */
export function useLinkMatrix() {
  return useQuery({
    queryKey: queryKeys.matrix,
    queryFn: () => call<LinkMatrix>("link_matrix"),
    staleTime: 5_000,
    retry: 0,
  });
}

/**
 * 启用/禁用一个 Skill 在所有（或指定）Agent 上的链接。
 *
 * # 为什么不做乐观更新
 *
 * 列表里看到的 `kind` 来自扫描结果，而链接操作会改动文件系统。
 * 乐观更新需要前端准确预测"操作后的状态"，但真实结果取决于磁盘
 * （例如目标被占用、Agent 正在锁定目录）——预测错了会给用户
 * 一个比"转一下再刷新"更糟的体验：界面显示成功而实际失败。
 *
 * 因此这里如实等待结果，成功后失效相关查询重新取数。
 * 单次操作目标 < 200ms，这个等待是可接受的。
 */
export function useSetLinkEnabled() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (vars: {
      skillDirName: string;
      agentIds: string[];
      enabled: boolean;
      policy?: ConflictPolicy;
    }) =>
      call<LinkResult[]>("link_set_enabled", {
        skillDirName: vars.skillDirName,
        agentIds: vars.agentIds,
        enabled: vars.enabled,
        policy: vars.policy,
      }),
    onSuccess: (results, vars) => {
      void queryClient.invalidateQueries({ queryKey: ["scan"] });
      void queryClient.invalidateQueries({ queryKey: queryKeys.matrix });

      if (results.length === 0) {
        toast.warning("没有可操作的 Agent");
        return;
      }

      // 部分失败必须明确告知，不能让用户以为全部成功
      const failed = results.filter((r) => r.action === "failed");
      const skipped = results.filter((r) => r.action === "skipped");
      const done = results.filter(
        (r) => r.action === "enabled" || r.action === "disabled",
      );

      if (failed.length > 0) {
        toast.error(`${failed.length}/${results.length} 个操作失败`, {
          description: failed.map((f) => f.message ?? f.errorKind).join("\n"),
        });
      }
      if (skipped.length > 0) {
        toast.warning(`${skipped.length} 个被跳过`, {
          description: skipped.map((s) => s.message ?? "").join("\n"),
        });
      }
      if (done.length > 0 && failed.length === 0 && skipped.length === 0) {
        toast.success(vars.enabled ? "已启用" : "已禁用");
      }
    },
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "操作失败");
    },
  });
}

/**
 * 把 Agent 目录中的外部 Skill 纳入中央库。
 *
 * 成功后原位置会变成指向中央库的链接——这是「接入」的前置动作。
 */
export function useAdoptSkill() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (vars: { agentId: string; dirName: string }) =>
      call<AdoptResult>("skill_adopt", vars),
    onSuccess: (result) => {
      void queryClient.invalidateQueries({ queryKey: ["scan"] });
      void queryClient.invalidateQueries({ queryKey: queryKeys.matrix });
      void queryClient.invalidateQueries({ queryKey: ["index"] });

      toast.success("已纳入中央库", {
        description: result.copiedAcrossVolumes
          ? `跨卷复制完成（较慢）。内容现位于 ${result.libraryPath}`
          : `内容已移至 ${result.libraryPath}，原位置已替换为链接`,
      });
    },
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "纳入失败");
    },
  });
}

/**
 * 迁移中央库到新位置并重写全部链接。
 *
 * 后端保证了安全顺序：先复制数据 → 再重写链接 → 最后才删旧数据。
 * 只要有链接重写失败，**旧数据与旧配置都会保留**。
 */
export function useRelocateLibrary() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (vars: { newPath: string; moveData: boolean }) =>
      call<RelocateResult>("library_relocate", vars),
    onSuccess: (result) => {
      void queryClient.invalidateQueries({ queryKey: ["scan"] });
      void queryClient.invalidateQueries({ queryKey: queryKeys.matrix });
      void queryClient.invalidateQueries({ queryKey: ["library"] });
      void queryClient.invalidateQueries({ queryKey: ["index"] });
      void queryClient.invalidateQueries({ queryKey: queryKeys.config });

      if (result.failed.length > 0) {
        toast.error(`${result.failed.length} 个链接未能重写`, {
          description:
            "旧数据已保留，配置未改动。可修复后重试。\n" +
            result.failed.slice(0, 3).join("\n"),
        });
        return;
      }
      toast.success(`中央库已迁移，重写了 ${result.relinked.length} 个链接`, {
        description: result.oldDataRemoved
          ? `旧位置的数据已清除。新位置：${result.to}`
          : `旧位置数据已保留。新位置：${result.to}`,
      });
    },
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "迁移失败");
    },
  });
}

// ===========================================================================
// GitHub 同步
// ===========================================================================

/** git 可用性、登录状态与已绑定仓库 */
export function useGitStatus() {
  return useQuery({
    queryKey: queryKeys.gitStatus,
    queryFn: () => call<GitAvailability>("git_status"),
    staleTime: 5_000,
    retry: 0,
  });
}

/**
 * 拉取当前 Token 可见的仓库列表。
 *
 * 只在用户点「加载仓库列表」时才请求（`enabled` 由调用方控制）——
 * 打开设置页就自动发一次 GitHub API 请求是多余的，也会让未登录的用户
 * 看到一条无意义的错误。
 */
export function useGitRepos(enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.gitRepos,
    queryFn: () => call<GitHubRepo[]>("git_list_repos"),
    enabled,
    retry: 0,
    staleTime: 60_000,
  });
}

/** 查询系统上有哪些登录方式可用 */
export function useGitLoginMethods() {
  return useQuery({
    queryKey: ["git", "loginMethods"] as const,
    queryFn: () => call<LoginMethods>("git_login_methods"),
    staleTime: 60_000,
    retry: 0,
  });
}

/**
 * 用 GitHub 账号登录（浏览器授权）。
 *
 * 这是一个**阻塞调用**——会打开浏览器等待用户确认，可能耗时数十秒。
 * 因此界面必须给出明确的进行中状态，不能让用户以为卡住了。
 */
export function useLoginWithBrowser() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => call<LoginMethods>("git_login_browser"),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.gitStatus });
      void queryClient.invalidateQueries({ queryKey: queryKeys.gitRepos });
      toast.success("已通过 GitHub 账号登录");
    },
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "登录未完成", {
        description: "如果浏览器没有自动打开，可改用下方的 Token 登录。",
      });
    },
  });
}

/**
 * 退出 GitHub 登录。
 *
 * **只有这一条退出路径**，它会把三处凭据一起清掉：本应用存在凭据管理器里的、
 * 改名之前遗留的、以及系统凭据助手里那份。
 *
 * 早先分成两个按钮（一个清本应用的、一个清系统的），结果两个都按了界面还是
 * 显示"已登录"——用户实际撞到的就是这个。合并成一条才可能真的退干净。
 *
 * 后端清完会**重新读一次真实状态**：万一系统那份没能删掉，它会返回错误，
 * 这里就必须把错误原样显示出来，不能让用户以为退成功了。
 */
export function useGitLogout() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => call<GitAvailability>("git_logout"),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.gitStatus });
      void queryClient.invalidateQueries({ queryKey: queryKeys.gitRepos });
      void queryClient.invalidateQueries({ queryKey: queryKeys.gitSync });
      toast.success("已退出登录");
    },
    onError: (error) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.gitStatus });
      toast.error("没能完全退出登录", {
        description:
          error instanceof IpcError
            ? error.message
            : "请在凭据管理器里手动删除 GitHub 凭据。",
      });
    },
  });
}

/**
 * 读取同步状态：本地改动、与远端的领先/落后。
 *
 * 不接收中央库路径：状态取自「当前绑定仓库」对应的同步工作区，
 * 与中央库路径无关。带上它只会让查询键看起来区分了不同的库。
 */
export function useGitSyncStatus() {
  return useQuery({
    queryKey: queryKeys.gitSync,
    queryFn: () => call<SyncStatus>("git_sync_status"),
    staleTime: 5_000,
    retry: 0,
  });
}

/** 一键同步：把中央库里仓库还没有的 Skill 推上去（首次同步也是它） */
export function useSyncNow() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (libraryPath: string) =>
      call<SyncStatus>("git_sync_now", { libraryPath }),
    onSuccess: (status) => {
      void queryClient.invalidateQueries({ queryKey: ["git"] });
      if (status.ahead > 0) {
        toast.warning(`已提交，但仍有 ${status.ahead} 个提交未推送`, {
          description: "推送可能因权限或网络失败，请查看仓库设置。",
        });
      } else {
        toast.success("中央库已同步到仓库");
      }
    },
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "同步失败");
    },
  });
}

/**
 * 取回仓库里、中央库还没有的 Skill。
 *
 * 语义是**补齐**而非覆盖：中央库已有的同名 Skill 一律跳过。
 */
export function usePull() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (libraryPath: string) =>
      call<SyncStatus>("git_pull", { libraryPath }),
    onSuccess: (status) => {
      void queryClient.invalidateQueries({ queryKey: ["git"] });
      void queryClient.invalidateQueries({ queryKey: ["scan"] });
      void queryClient.invalidateQueries({ queryKey: ["index"] });

      const imported = status.importedSkills;
      if (imported.length === 0) {
        toast.info("仓库里没有中央库缺少的 Skill");
        return;
      }
      toast.success(`已取回 ${imported.length} 个 Skill`, {
        description: imported.slice(0, 5).join("、"),
      });
    },
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "取回失败");
    },
  });
}

/** 中央库中是否残留着旧版本留下的 `.git`（新模型下不该有） */
export function useStaleRepo(libraryPath: string | null) {
  return useQuery({
    queryKey: ["git", "staleRepo", libraryPath] as const,
    queryFn: () => call<string | null>("git_stale_repo", { libraryPath }),
    enabled: Boolean(libraryPath && libraryPath.trim()),
    retry: 0,
    // 不缓存：这是"磁盘上有没有那个目录"的查询，而用户完全可能在应用之外
    // （资源管理器里）把它删掉。缓存住旧答案的结果是一张**已经过时的告警卡**——
    // 提示一个并不存在的问题，比不提示更让人困惑。
    staleTime: 0,
  });
}

/**
 * 把中央库中残留的 `.git` 移到一旁。
 *
 * 后端做的是**重命名，不是删除**，因此这是一个可逆操作。
 */
export function useCleanupStaleRepo() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (libraryPath: string) =>
      call<string | null>("git_cleanup_stale_repo", { libraryPath }),
    onSuccess: (movedTo) => {
      void queryClient.invalidateQueries({ queryKey: ["git"] });
      if (!movedTo) {
        toast.info("中央库里没有残留的 .git");
        return;
      }
      toast.success("已把残留的 .git 移到一旁", {
        description: "没有删除任何东西，位置：" + movedTo,
      });
    },
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "清理失败");
    },
  });
}

/** 保存 Token 到 Windows 凭据管理器 */
export function useSaveGitToken() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (token: string) =>
      call<GitAvailability>("git_save_token", { token }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.gitStatus });
      // 换了凭据，仓库列表需要重新拉
      void queryClient.invalidateQueries({ queryKey: queryKeys.gitRepos });
      toast.success("已保存到 Windows 凭据管理器");
    },
    onError: (error) => {
      toast.error(
        error instanceof IpcError ? error.message : "保存 Token 失败",
      );
    },
  });
}

/** 绑定要同步的远程仓库并校验可达性 */
export function useSetGitRemote() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (vars: { url: string; branch?: string }) =>
      call<GitAvailability>("git_set_remote", vars),
    onSuccess: (status) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.gitStatus });
      toast.success(`已绑定仓库：${status.remoteUrl}`);
    },
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "绑定仓库失败");
    },
  });
}

// ===========================================================================
// 标签
// ===========================================================================

/** 中央库里的标签统计（含各自的 Skill 数） */
export function useTagStats(path: string | null) {
  return useQuery({
    queryKey: queryKeys.tagStats(path),
    queryFn: () => call<TagCount[]>("tags_stats", { path }),
    enabled: Boolean(path && path.trim()),
    staleTime: 5_000,
    retry: 0,
  });
}

/**
 * 改写标签（重命名 / 合并 / 删除共用）。
 *
 * 后端会改 SKILL.md 文件本身并重建索引，因此成功后要失效三处：
 * 标签统计、索引（搜索结果也挂在 `["index"]` 下）、以及扫描清单。
 */
export function useApplyTags() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (vars: {
      sources: string[];
      target: string | null;
      path: string;
    }) => call<TagReport>("tags_apply", vars),
    onSuccess: (report) => {
      void queryClient.invalidateQueries({ queryKey: ["tags"] });
      void queryClient.invalidateQueries({ queryKey: ["index"] });
      void queryClient.invalidateQueries({ queryKey: ["scan"] });

      // 部分失败必须明说，不能让用户以为全改完了
      if (report.failed.length > 0) {
        toast.error(`${report.failed.length} 个 Skill 未改写`, {
          description: report.failed
            .slice(0, 3)
            .map((f) => `${f.dirName}：${f.reason}`)
            .join("\n"),
        });
        return;
      }
      toast.success(`已改写 ${report.changed.length} 个 Skill 的标签`);
    },
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "标签改写失败");
    },
  });
}

// ===========================================================================
// 导入导出
// ===========================================================================

/**
 * 第一步：准备导入并拿到预览。
 *
 * 由调用方用 `mutateAsync` 驱动（选完来源立刻要结果去渲染预览），
 * 因此这里不失效任何查询——`useImportApply` 才是改变数据的那一步。
 */
export function useImportPreview() {
  return useMutation({
    mutationFn: (vars: { source: ImportSource; path: string }) =>
      call<ImportPreview>("import_preview", vars),
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "无法读取该来源");
    },
  });
}

/**
 * 拖拽入口：把用户拖进窗口的真实路径直接变成预览。
 *
 * 与 `useImportPreview` 分开，是因为拖拽拿到的是**一批路径**，且必须由后端
 * 判断每一条是目录、ZIP 还是单个 SKILL.md——前端只有路径字符串，问不了文件系统。
 * 认不出的项会出现在预览的 `rejected` 里。
 */
export function useImportPreviewPaths() {
  return useMutation({
    mutationFn: (vars: { paths: string[]; path: string }) =>
      call<ImportPreview>("import_preview_paths", vars),
    onError: (error) => {
      toast.error(
        error instanceof IpcError ? error.message : "无法读取拖入的内容",
      );
    },
  });
}

/** 第二步：按逐项选择把内容落进中央库 */
export function useImportApply() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (vars: {
      token: string;
      decisions: ImportDecision[];
      path: string;
    }) => call<ImportReport>("import_apply", vars),
    onSuccess: (report) => {
      void queryClient.invalidateQueries({ queryKey: ["index"] });
      void queryClient.invalidateQueries({ queryKey: ["scan"] });
      void queryClient.invalidateQueries({ queryKey: ["library"] });

      const failed = report.outcomes.filter((o) => o.reason && !o.dirName);
      const skipped = report.outcomes.filter((o) => o.action === "skip");
      if (failed.length > 0) {
        toast.error(`导入结束，有 ${failed.length} 项未成功`, {
          description: failed
            .slice(0, 3)
            .map((o) => `${o.relativePath}：${o.reason}`)
            .join("\n"),
        });
        return;
      }
      toast.success(`已导入 ${report.imported} 个 Skill`, {
        description:
          skipped.length > 0
            ? `另有 ${skipped.length} 项按你的选择跳过`
            : undefined,
      });
    },
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "导入失败");
    },
  });
}

/** 导出选中的 Skill 为 ZIP 或纯目录结构 */
export function useExportSkills() {
  return useMutation({
    mutationFn: (vars: {
      dirNames: string[];
      dest: string;
      format: ExportFormat;
      includeManifest: boolean;
      path: string;
    }) => call<ExportReport>("export_skills", vars),
    onSuccess: (report) => {
      if (report.failed.length > 0) {
        toast.warning(
          `已导出 ${report.skillCount} 个 Skill，有 ${report.failed.length} 项失败`,
          { description: report.failed.slice(0, 3).join("\n") },
        );
        return;
      }
      toast.success(
        `已导出 ${report.skillCount} 个 Skill（${report.fileCount} 个文件）`,
        { description: report.dest },
      );
    },
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "导出失败");
    },
  });
}

/**
 * 卸载：把 Skill 从中央库移除（送到 Windows 回收站），并摘掉它的全部链接。
 *
 * 与 `useSetLinkEnabled(false)` 的界线：那条只摘链接、中央库不动；
 * 这条会把目录也搬走。
 *
 * 结果按「已卸载 / 已跳过 / 失败」三类分别汇报。**绝不能只说一句"完成"**——
 * 批量卸载里"3 个成功 1 个跳过"如果含糊过去，用户会以为库里已经没有那个 Skill 了。
 * 每个成功项还要报出**它去了哪**（回收站，还是送不进去时落到的回落目录）。
 */
export function useUninstallSkills() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (vars: { dirNames: string[]; path: string }) =>
      call<UninstallReport>("skills_uninstall", vars),
    onSuccess: (report) => {
      void queryClient.invalidateQueries({ queryKey: ["scan"] });
      void queryClient.invalidateQueries({ queryKey: ["index"] });
      void queryClient.invalidateQueries({ queryKey: ["library"] });
      void queryClient.invalidateQueries({ queryKey: queryKeys.matrix });

      const removed = report.outcomes.filter((o) => o.action === "removed");
      const skipped = report.outcomes.filter((o) => o.action === "skipped");
      const failed = report.outcomes.filter((o) => o.action === "failed");

      if (failed.length > 0) {
        toast.error(`${failed.length} 个没能卸载`, {
          description: failed
            .slice(0, 3)
            .map((o) => {
              // 「链接已摘掉但目录没搬走」是个中间态，必须说出来——
              // 否则用户只看到"失败"，不知道 Agent 那边其实已经变了
              const partial =
                o.removedLinks.length > 0
                  ? `（已摘掉 ${o.removedLinks.length} 个链接，但目录还在中央库）`
                  : "";
              return `${o.dirName}：${o.reason ?? "未知原因"}${partial}`;
            })
            .join("\n"),
        });
      }
      if (skipped.length > 0) {
        toast.warning(`${skipped.length} 个被跳过`, {
          description: skipped
            .slice(0, 3)
            .map((o) => `${o.dirName}：${o.reason ?? ""}`)
            .join("\n"),
        });
      }
      if (removed.length > 0) {
        // 「去了哪」必须说出来：默认进回收站，送不进去时会落到中央库内的 removed/
        const toBin = removed.filter((o) => o.movedTo === "回收站");
        const elsewhere = removed.filter((o) => o.movedTo !== "回收站");

        toast.success(`已卸载 ${removed.length} 个 Skill`, {
          description:
            elsewhere.length > 0
              ? `其中 ${toBin.length} 个进了回收站；` +
                elsewhere
                  .slice(0, 2)
                  .map((o) => `${o.dirName} 送不进回收站，已放在 ${o.movedTo}`)
                  .join("；")
              : `可在 Windows 回收站里找回。`,
        });
      }
    },
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "卸载失败");
    },
  });
}

// ===========================================================================
// 背景图
// ===========================================================================

/** 当前有没有自定义背景（`path` 为 null 表示在用内置默认背景） */
export function useBackground() {
  return useQuery({
    queryKey: ["background"],
    queryFn: () => call<BackgroundState>("background_get"),
    staleTime: 5_000,
    retry: 0,
  });
}

/**
 * 取自定义背景的内容（data URL）。
 *
 * `enabled` 由调用方控制：**只有真的在用自定义背景时才去取**。
 * 这个返回值可能有一两 MB（base64），默认背景是打包进前端的、
 * 根本不需要走 IPC，没必要为它付这笔钱。
 *
 * 缓存设成无限：它只在用户换图时变，而换图时会被显式失效。
 */
export function useBackgroundImage(enabled: boolean) {
  return useQuery({
    queryKey: ["background", "image"],
    queryFn: () => call<string | null>("background_image"),
    enabled,
    staleTime: Infinity,
    gcTime: Infinity,
    retry: 0,
  });
}

export function useSetBackground() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (source: string) =>
      call<BackgroundState>("background_set", { source }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["background"] });
      toast.success("背景已更新");
    },
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "设置背景图失败");
    },
  });
}

export function useClearBackground() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => call<BackgroundState>("background_clear"),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["background"] });
      void queryClient.invalidateQueries({ queryKey: queryKeys.config });
      toast.success("已恢复默认背景");
    },
    onError: (error) => {
      toast.error(
        error instanceof IpcError ? error.message : "恢复默认背景失败",
      );
    },
  });
}

/**
 * 为「装了、路径已知、但技能目录还没建」的 Agent 创建技能目录。
 *
 * 成功后重新取检测结果：目录一旦存在，该 Agent 的状态就从
 * `installedNoSkillDir` 变成 `detected`，随之出现在启用目标列表里。
 */
export function useCreateAgentSkillDir() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (agentId: string) =>
      call<AgentDetection[]>("agent_create_skill_dir", { agentId }),
    onSuccess: (detections) => {
      queryClient.setQueryData(queryKeys.agents, detections);
      // 目录出现后，启用目标与矩阵都会变
      void queryClient.invalidateQueries({ queryKey: ["scan"] });
      void queryClient.invalidateQueries({ queryKey: ["links"] });
      toast.success("技能目录已创建");
    },
    onError: (error) => {
      toast.error(
        error instanceof IpcError ? error.message : "创建技能目录失败",
      );
    },
  });
}

export function useRebuildIndex() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (path: string | null) =>
      call<RebuildReport>("index_rebuild", { path }),
    onSuccess: (report) => {
      void queryClient.invalidateQueries({ queryKey: ["index"] });
      void queryClient.invalidateQueries({ queryKey: ["library"] });

      if (report.skipped.length > 0) {
        // 跳过项不能静默——用户需要知道哪些 Skill 没被识别
        toast.warning(
          `索引完成：${report.skillCount} 个 Skill，${report.skipped.length} 个目录被跳过`,
          {
            description: report.skipped
              .slice(0, 3)
              .map((s) => `${s.dirName}：${s.reason}`)
              .join("\n"),
          },
        );
      } else {
        toast.success(
          `索引已重建：${report.skillCount} 个 Skill（${report.durationMs}ms）`,
        );
      }
    },
    onError: (error) => {
      toast.error(error instanceof IpcError ? error.message : "重建索引失败");
    },
  });
}
