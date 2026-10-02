/**
 * 前后端 IPC 契约类型。
 *
 * 这些类型与 `src-tauri/src/` 下各命令的返回结构一一对应。
 * 修改任一侧时必须同步另一侧——目前尚无编译期约束。
 */

/** 与 Rust `AppError::kind()` 的返回值保持一致 */
export type AppErrorKind =
  | "Internal"
  | "Unsupported"
  | "Io"
  | "Config"
  | "NotFound"
  | "PermissionDenied"
  | "Conflict"
  | "NotAJunction"
  | "TargetOutsideLibrary";

/** Rust 侧 AppError 序列化后的形状 */
export interface AppErrorPayload {
  kind: AppErrorKind;
  message: string;
  detail?: string | null;
}

/** `ping` 命令的返回结构 */
export interface PongPayload {
  message: string;
  appVersion: string;
  platform: string;
  timestampMs: number;
}

// ===========================================================================
// 配置（`commands/config.rs`）
// ===========================================================================

export interface AgentView {
  id: string;
  enabled: boolean;
  skillDir: string | null;
  /** 展示名。仅用户自定义 Agent 需要填写，内置 Agent 的名称来自注册表。 */
  displayName: string | null;
}

export interface GitView {
  remoteUrl: string | null;
  branch: string;
  autoPush: boolean;
}

/**
 * 配置的 IPC 视图。
 *
 * 注意：**落盘的 config.json 使用 snake_case**（用户可手工编辑的兼容面），
 * 这里收到的 IPC 载荷是 camelCase。两者的转换在 Rust 侧的
 * `commands/config.rs` 完成。
 */
export interface ConfigView {
  schemaVersion: number;
  centralLibraryPath: string | null;
  agents: AgentView[];
  git: GitView;
}

// ===========================================================================
// 中央库（`commands/library.rs`）
// ===========================================================================

export type VolumeKind =
  "fixed" | "removable" | "network" | "cdRom" | "ramDisk" | "unknown";

export type IssueSeverity = "error" | "warning" | "info";

export interface DiagnosisIssue {
  severity: IssueSeverity;
  /** 稳定的机器可读标识，便于定向处理 */
  code: string;
  message: string;
}

/** `library_validate` 的返回：中央库候选路径的体检报告 */
export interface PathDiagnosis {
  path: string;
  normalizedPath: string;
  exists: boolean;
  isDir: boolean;
  writable: boolean;
  filesystem: string | null;
  volumeKind: VolumeKind | null;
  /** 检测到的云同步目录名（启发式判断，非权威结论） */
  cloudSync: string | null;
  freeBytes: number | null;
  totalBytes: number | null;
  pathLength: number;
  isInitialized: boolean;
  skillCount: number | null;
  issues: DiagnosisIssue[];
  /** 无 error 级问题时为 true，决定"初始化"按钮是否可用 */
  canInitialize: boolean;
}

export interface InitializeResult {
  path: string;
  created: string[];
  alreadyInitialized: boolean;
  skillCount: number;
}

export interface LibraryStats {
  path: string;
  skillCount: number;
  totalBytes: number;
  initialized: boolean;
}

// ===========================================================================
// 目录链接（`commands/link.rs`）
// ===========================================================================

export type LinkKind =
  "junction" | "symlinkDir" | "symlinkFile" | "notALink" | "missing";

export interface LinkStatus {
  path: string;
  kind: LinkKind;
  /** junction 指向的目标。悬空链接仍会返回原始目标路径。 */
  target: string | null;
  /** 目标当前是否可达。`kind === "junction" && !targetExists` 即断链。 */
  targetExists: boolean;
}

// ===========================================================================
// 索引（`commands/index.rs`）
// ===========================================================================

export interface SkippedEntry {
  dirName: string;
  reason: string;
}

export interface RebuildReport {
  skillCount: number;
  /** 命中增量缓存、跳过重新解析的条目数 */
  cachedHits: number;
  /** 实际重新解析的条目数 */
  parsed: number;
  /** 无法解析而被跳过的目录。不应静默忽略。 */
  skipped: SkippedEntry[];
  durationMs: number;
}

export interface SkillRecord {
  id: string;
  name: string;
  description: string | null;
  dirPath: string;
  tags: string[];
  contentHash: string | null;
  mtime: number | null;
  size: number | null;
}

// ===========================================================================
// Agent 检测与扫描（`commands/agents.rs`）
// ===========================================================================

/**
 * Agent 的检测状态。
 *
 * 刻意是三态而非布尔：实测发现 Claude Code 会出现「CLI 已安装但技能目录
 * 尚未创建」的中间状态，用布尔会把已装的 Agent 判成未安装。
 */
export type AgentStatus = "notInstalled" | "installedNoSkillDir" | "detected";

export interface AgentDetection {
  id: string;
  displayName: string;
  icon: string | null;
  status: AgentStatus;
  /** 最终确定的技能目录（`detected` 时非空） */
  skillDir: string | null;
  /** 命中的检测依据，向用户解释"为什么认为它装了" */
  detectedBy: string[];
  isUserOverride: boolean;
  /** 实测备注：该路径是实测确认、实测证伪还是未实测 */
  notes: string | null;
  docsUrl: string | null;
}

/** Skill 在某个 Agent 中的存在形态 */
export type InstanceKind =
  "managed" | "external" | "externalDuplicate" | "dangling" | "foreignLink";

export interface ScannedInstance {
  agentId: string;
  agentName: string;
  skillId: string;
  dirName: string;
  path: string;
  /** 嵌套分组名（Codex 的 `.system` 等） */
  group: string | null;
  /** 分组是否为该 Agent 的内置资源 */
  isSystemGroup: boolean;
  kind: InstanceKind;
  linkTarget: string | null;
  name: string;
  description: string | null;
  tags: string[];
  mtime: number | null;
  size: number | null;
  /** 解析 SKILL.md 失败时的原因（此时 name 回退为目录名） */
  parseError: string | null;
}

/** 统一清单中的一个 Skill（可能来自多个 Agent） */
export interface SkillSummary {
  id: string;
  name: string;
  description: string | null;
  tags: string[];
  /** 用于建立链接的目录名（必须与中央库内的实际目录名一致） */
  dirName: string;
  inCentralLibrary: boolean;
  centralPath: string | null;
  instances: ScannedInstance[];
  /** instances 中 kind === "managed" 的数量 */
  managedCount: number;
  updatedAt: number | null;
}

// ===========================================================================
// 链接启停与映射矩阵（`commands/links.rs`）
// ===========================================================================

/** 目标位置已被真实目录占用时的处理策略 */
export type ConflictPolicy = "skip" | "backupAndReplace" | "abort";

/** 单次操作实际做了什么 */
export type LinkAction =
  "enabled" | "disabled" | "alreadyInState" | "skipped" | "failed";

export interface LinkResult {
  skillId: string;
  agentId: string;
  linkPath: string;
  targetPath: string;
  action: LinkAction;
  /** 与 AppErrorKind 一致；仅 failed 时有值 */
  errorKind: AppErrorKind | null;
  message: string | null;
}

/** Skill 在某个 Agent 中的链接状态 */
export type LinkState =
  | "valid"
  | "absent"
  | "dangling"
  | "conflict"
  | "foreignLink"
  | "foreignSymlink";

export interface LinkCell {
  skillId: string;
  agentId: string;
  state: LinkState;
  linkPath: string;
  targetPath: string | null;
}

export interface MatrixAgent {
  id: string;
  displayName: string;
  skillDir: string;
}

export interface MatrixRow {
  skillId: string;
  name: string;
  dirName: string;
  inCentralLibrary: boolean;
  cells: LinkCell[];
}

export interface MatrixSummary {
  valid: number;
  absent: number;
  dangling: number;
  conflict: number;
  foreignLink: number;
  foreignSymlink: number;
}

// ===========================================================================
// 导入导出（`transfer/mod.rs`）
// ===========================================================================

/** 导入来源。`kind` 决定其余字段的取值。 */
export type ImportSource =
  | { kind: "folder"; path: string }
  | { kind: "zip"; path: string }
  | { kind: "git"; url: string }
  | { kind: "skillFile"; path: string };

export interface ConflictInfo {
  dirName: string;
  name: string;
  description: string | null;
}

/**
 * 冲突的另一方是谁。
 *
 * `library` = 中央库里已有同名；`batch` = 与**本次导入的另一个条目**重名。
 * 落盘时的处理一样，但措辞必须分开——把批次内的重名说成"中央库里已有"，
 * 会把用户指到错误的方向去排查。
 */
export type ConflictKind = "library" | "batch";

export interface ImportCandidate {
  /** 相对暂存根的路径，应用时原样回传 */
  relativePath: string;
  /** 计划落进中央库的目录名 */
  dirName: string;
  name: string;
  description: string | null;
  tags: string[];
  /** 非 null 即有同名冲突 */
  conflict: ConflictInfo | null;
  /** 与 `conflict` 同生共死 */
  conflictKind: ConflictKind | null;
}

export interface InvalidCandidate {
  relativePath: string;
  reason: string;
}

/** 拖入的路径里无法识别、因而整个没参与导入的项 */
export interface RejectedPath {
  path: string;
  reason: string;
}

export interface ImportPreview {
  /** 应用时回传，用来确认操作的是同一次预览 */
  token: string;
  stagingRoot: string;
  candidates: ImportCandidate[];
  /** 解析不通过、不参与导入的条目 */
  invalid: InvalidCandidate[];
  /** 只有「拖拽导入」会产生它；走「选来源」时恒为空 */
  rejected: RejectedPath[];
}

export type ImportAction = "skip" | "rename" | "overwrite";

export interface ImportDecision {
  relativePath: string;
  action: ImportAction;
}

export interface ImportOutcome {
  relativePath: string;
  action: ImportAction;
  dirName: string | null;
  /** 「替换」时旧的那一份被**移到**了哪里（不是删除） */
  replacedTo: string | null;
  reason: string | null;
}

export interface ImportReport {
  outcomes: ImportOutcome[];
  imported: number;
  indexed: number;
}

export type ExportFormat = "zip" | "folder";

export interface ExportReport {
  dest: string;
  skillCount: number;
  fileCount: number;
  /** 没有导出成功的 Skill 及原因，非空时必须告知用户 */
  failed: string[];
}

/** `tags_stats` 的一条统计 */
export interface TagCount {
  /** 展示用的拼写（大小写不同的写法已归并成一条） */
  tag: string;
  count: number;
}

/** 单个 Skill 的标签改写结果 */
export interface TagChange {
  dirName: string;
  name: string;
  before: string[];
  after: string[];
}

export interface TagFailure {
  dirName: string;
  reason: string;
}

/** `tags_apply` 的整体结果 */
export interface TagReport {
  changed: TagChange[];
  /** 改写失败的 Skill 及原因，**非空时必须告知用户** */
  failed: TagFailure[];
  /** 重建索引后的 Skill 总数 */
  indexed: number;
}

/** `skills_search` 的一条全文命中 */
export interface SearchHit {
  skillId: string;
  /** 命中处的正文片段，前端负责高亮关键词 */
  snippet: string;
}

/** 映射矩阵 —— 链接透明化的数据源 */
export interface LinkMatrix {
  /** 中央库根路径（绝对路径），页面上必须明确展示 */
  centralLibraryPath: string | null;
  agents: MatrixAgent[];
  rows: MatrixRow[];
  summary: MatrixSummary;
  generatedAt: number;
}

// ===========================================================================
// GitHub 同步（`git/mod.rs`）
// ===========================================================================

export interface GitAvailability {
  /** 系统是否装了 git */
  available: boolean;
  version: string | null;
  /** 是否已在凭据管理器中保存 Token */
  hasToken: boolean;
  /** 系统凭据助手中是否已有 GitHub 凭据（例如用账号登录过） */
  hasHelperCredentials: boolean;
  remoteUrl: string | null;
  branch: string;
}

export interface SyncStatus {
  /** 同步工作区是否已建立（即是否绑定过仓库并同步过一次） */
  isRepo: boolean;
  changedCount: number;
  added: string[];
  modified: string[];
  removed: string[];
  ahead: number;
  behind: number;
  lastCommit: string | null;
  /** 本次「取回缺失的 Skill」写入中央库的 Skill 名（仅 `git_pull` 会填充） */
  importedSkills: string[];
}

/** 可用的登录方式 */
export interface LoginMethods {
  /** 系统凭据助手配置值，例如 helper-selector（GCM） */
  credentialHelper: string | null;
  gcmAvailable: boolean;
  gcmPath: string | null;
}

export interface GitHubRepo {
  fullName: string;
  name: string;
  private: boolean;
  description: string | null;
  cloneUrl: string;
  defaultBranch: string | null;
}

/** `library_relocate` 的返回：中央库整体迁移的结果 */
export interface RelocateResult {
  from: string;
  to: string;
  /** 数据是否被移动（false 表示保留原位置，只做复制） */
  moved: boolean;
  relinked: string[];
  /** 重写失败的链接及原因。**非空时旧数据会被保留。** */
  failed: string[];
  oldDataRemoved: boolean;
}

/** `skill_adopt` 的返回：把外部 Skill 纳入中央库的结果 */
export interface AdoptResult {
  skillId: string;
  agentId: string;
  sourcePath: string;
  libraryPath: string;
  linkPath: string;
  /** 是否发生了跨卷复制（跨卷无法原子移动） */
  copiedAcrossVolumes: boolean;
}

/** `skill_read` / `skill_write` 的返回 */
export interface SkillFile {
  path: string;
  content: string;
  name: string | null;
  description: string | null;
  tags: string[];
}

export interface ScanReport {
  skills: SkillSummary[];
  agents: AgentDetection[];
  scannedDirs: number;
  durationMs: number;
  /** 扫描时跳过的条目及原因，不静默忽略 */
  skipped: string[];
}

// ===========================================================================
// 背景图（`commands/background.rs`）
// ===========================================================================

export interface BackgroundState {
  /**
   * 自定义背景图的绝对路径；`null` 表示正在用内置的默认背景。
   *
   * 图已复制进应用数据目录，所以这个路径是**我们自己的副本**，
   * 不是用户当初选的那张原图。
   */
  path: string | null;
}

// ===========================================================================
// 卸载（`commands/uninstall.rs`）
// ===========================================================================

/**
 * 卸载的结局。
 *
 * `removed` 不等于"已销毁"——目录是进了 **Windows 回收站**（或送不进去时
 * 落到中央库内的 `removed/`），两种情况都要靠 `movedTo` 如实告诉用户它去了哪。
 */
export type UninstallAction = "removed" | "skipped" | "failed";

export interface UninstallOutcome {
  dirName: string;
  action: UninstallAction;
  /** 中央库里的绝对路径 */
  path: string;
  /** 已摘掉的链接（绝对路径） */
  removedLinks: string[];
  /** 摘链接失败的项，形如「Claude Code：<原因>」 */
  failedLinks: string[];
  /** 目录最终去了哪：`回收站`，或回落目录的绝对路径 */
  movedTo: string | null;
  reason: string | null;
}

export interface UninstallReport {
  outcomes: UninstallOutcome[];
}
