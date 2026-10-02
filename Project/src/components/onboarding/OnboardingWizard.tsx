import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  FolderOpen,
  HardDrive,
  Info,
  Loader2,
  PackagePlus,
  PlugZap,
  XCircle,
} from "lucide-react";

import { Button } from "@/components/ui/Button";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/Dialog";
import { ImportDialog } from "@/components/transfer/ImportDialog";
import { cn } from "@/lib/cn";
import { formatBytes, formatVolumeKind } from "@/lib/format";
import {
  useAgentsDetect,
  useConfig,
  useCreateAgentSkillDir,
  useInitializeLibrary,
  useSetConfig,
  useSetLinkEnabled,
  useSkillsScan,
  useValidateLibraryPath,
} from "@/lib/queries";
import type {
  AgentDetection,
  DiagnosisIssue,
  IssueSeverity,
  PathDiagnosis,
  SkillSummary,
} from "@/types/ipc";

/**
 * 首次运行引导（三步）。
 *
 * # 触发条件
 *
 * **中央库路径未配置即视为首次运行**。这样它只在
 * 真的全新环境出现，老用户升级时不会突然弹出来——他们的路径早就配好了。
 *
 * # 为什么用"step > 0"而不只看配置
 *
 * 第 1 步一完成就把路径写进了配置。若只用"路径未配置"作为显示条件，
 * 引导会在用户眼前**当场消失**——他刚点完"创建并设为中央库"，还没走到
 * 第 2 步。因此一旦开走（step > 0）就与配置解耦，走完或显式关闭才结束。
 *
 * # 为什么"稍后再说"不落盘
 *
 * 跳过只在本次运行内生效。用户没配中央库时应用什么也做不了，
 * 下次启动再问一次是合理的；反过来把它持久化，会让真正的新用户
 * 在误点一次之后**再也见不到引导**。
 */
export function OnboardingWizard() {
  const { data: config } = useConfig();

  const [step, setStep] = useState(0);
  const [dismissed, setDismissed] = useState(false);
  const [completed, setCompleted] = useState(false);
  const [importing, setImporting] = useState(false);

  const firstRun = Boolean(config && !config.centralLibraryPath);
  const open = !dismissed && !completed && (firstRun || step > 0);

  const libraryPath = config?.centralLibraryPath ?? null;

  return (
    <>
      <Dialog
        open={open}
        onOpenChange={(next) => {
          if (!next) setDismissed(true);
        }}
      >
        <DialogContent className="max-w-xl" showClose={false}>
          <header className="mb-4 space-y-3">
            <div className="flex items-center gap-2">
              <span className="flex size-7 items-center justify-center rounded-full bg-accent-subtle text-accent">
                <HardDrive className="size-4" />
              </span>
              <DialogTitle>欢迎使用 BakaSkill</DialogTitle>
            </div>
            <p className="text-sm leading-relaxed text-fg-muted">
              用三步把 Skill 集中到一处，再映射给各个 AI Agent。 以后同一个
              Skill 只需要维护一份。
            </p>
            <Stepper current={step} />
          </header>

          {step === 0 ? (
            <LibraryStep
              onDone={() => setStep(1)}
              onSkip={() => setDismissed(true)}
            />
          ) : null}

          {step === 1 ? (
            <AgentsStep
              onBack={() => setStep(0)}
              onDone={() => setStep(2)}
              onSkip={() => setDismissed(true)}
            />
          ) : null}

          {step === 2 && libraryPath ? (
            <EnableStep
              libraryPath={libraryPath}
              onBack={() => setStep(1)}
              onDone={() => setCompleted(true)}
              onImport={() => setImporting(true)}
            />
          ) : null}
        </DialogContent>
      </Dialog>

      {/* 导入向导挂在这里、而不是嵌在引导的 DialogContent 里：
          两层 Radix Dialog 嵌套会让焦点陷阱互相争抢；而且导入完成后
          引导必须仍留在第 3 步（好让用户接着挑刚导进来的 Skill），
          嵌进去就容易跟着一起被关掉。 */}
      {libraryPath ? (
        <ImportDialog
          open={importing}
          onOpenChange={setImporting}
          libraryPath={libraryPath}
        />
      ) : null}
    </>
  );
}

const STEP_LABELS = ["选择中央库", "连接 Agent", "启用第一个 Skill"] as const;

function Stepper({ current }: { current: number }) {
  return (
    <ol className="flex items-center gap-2">
      {STEP_LABELS.map((label, index) => {
        const done = index < current;
        const active = index === current;
        return (
          <li key={label} className="flex min-w-0 items-center gap-2">
            <span
              className={cn(
                "flex size-5 shrink-0 items-center justify-center rounded-full text-[11px] font-medium",
                done && "bg-success text-white",
                active && "bg-accent text-accent-fg",
                !done && !active && "bg-bg-subtle text-fg-subtle",
              )}
            >
              {done ? <CheckCircle2 className="size-3.5" /> : index + 1}
            </span>
            <span
              className={cn(
                "truncate text-xs",
                active ? "font-medium text-fg" : "text-fg-subtle",
              )}
            >
              {label}
            </span>
            {index < STEP_LABELS.length - 1 ? (
              <ArrowRight className="size-3 shrink-0 text-fg-subtle" />
            ) : null}
          </li>
        );
      })}
    </ol>
  );
}

// ===========================================================================
// 第 1 步：中央库位置
// ===========================================================================

/**
 * 第 1 步。与 `CentralPathPicker` 做的是同一件事，但只保留引导需要的部分
 * （没有迁移、没有重建索引——那些在设置页里，不是新用户此刻要做的决定）。
 *
 * **应用顺序在这里是承重的**：先初始化目录骨架，成功后才写配置。
 * 反过来会在初始化失败时留下一个指向空目录的配置，应用随后会认为
 * 中央库已配好而实际上什么也没有。`CentralPathPicker` 里是同一条顺序。
 */
function LibraryStep({
  onDone,
  onSkip,
}: {
  onDone: () => void;
  onSkip: () => void;
}) {
  const { data: config } = useConfig();
  const initialize = useInitializeLibrary();
  const setConfig = useSetConfig();

  const [candidate, setCandidate] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  const diagnosis = useValidateLibraryPath(candidate);
  const applying = initialize.isPending || setConfig.isPending;

  const pickFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "选择中央库位置",
    });
    if (typeof selected === "string") {
      setFailure(null);
      setCandidate(selected);
    }
  };

  const apply = async () => {
    if (!candidate) return;
    setFailure(null);
    try {
      await initialize.mutateAsync(candidate);
      if (!config) throw new Error("配置尚未加载完成，请稍后重试");
      await setConfig.mutateAsync({
        ...config,
        centralLibraryPath: candidate,
      });
      onDone();
    } catch (error) {
      // 这里必须自己接住：失败了不能往前走到第 2 步，
      // 否则用户会在一个不存在的中央库上继续操作。
      setFailure(error instanceof Error ? error.message : "无法使用该位置");
    }
  };

  return (
    <div className="space-y-4">
      <p className="text-sm leading-relaxed text-fg-muted">
        选一个存放 Skill 的文件夹。
        <strong className="text-fg">可以放在任意盘符</strong>
        ，不必挤占 C 盘——这是 BakaSkill 与同类工具最主要的区别。
      </p>

      <div className="flex flex-wrap gap-2">
        <Button variant="secondary" onClick={() => void pickFolder()}>
          <FolderOpen className="size-4" />
          {candidate ? "换一个文件夹…" : "选择文件夹…"}
        </Button>
      </div>

      {candidate ? (
        <div className="space-y-3 rounded-md border border-border bg-bg-subtle p-3">
          <code
            className="block truncate font-mono text-xs text-fg"
            title={candidate}
          >
            {candidate}
          </code>

          {diagnosis.isPending ? (
            <div className="flex items-center gap-2 text-xs text-fg-muted">
              <Loader2 className="size-3.5 animate-spin" />
              正在检查该位置…
            </div>
          ) : null}

          {diagnosis.data ? (
            <DiagnosisSummary diagnosis={diagnosis.data} />
          ) : null}

          {diagnosis.error ? (
            <div className="flex items-start gap-2 text-xs text-danger">
              <XCircle className="mt-0.5 size-3.5 shrink-0" />
              <span>
                {diagnosis.error instanceof Error
                  ? diagnosis.error.message
                  : "无法检查该位置"}
              </span>
            </div>
          ) : null}

          {failure ? (
            <div className="flex items-start gap-2 rounded-sm border border-danger/40 bg-danger-subtle p-2 text-xs text-danger">
              <XCircle className="mt-0.5 size-3.5 shrink-0" />
              <span>{failure}</span>
            </div>
          ) : null}

          <div className="flex flex-wrap gap-2">
            <Button
              onClick={() => void apply()}
              disabled={!diagnosis.data?.canInitialize || applying}
            >
              {applying ? <Loader2 className="size-4 animate-spin" /> : null}
              创建并设为中央库
            </Button>
            <Button
              variant="ghost"
              onClick={() => setCandidate(null)}
              disabled={applying}
            >
              取消
            </Button>
          </div>
        </div>
      ) : null}

      <div className="flex items-center justify-between border-t border-border pt-4">
        <Button variant="ghost" onClick={onSkip}>
          稍后再说
        </Button>
      </div>
    </div>
  );
}

const SEVERITY_ICON: Record<IssueSeverity, React.ElementType> = {
  error: XCircle,
  warning: AlertTriangle,
  info: Info,
};

const SEVERITY_CLASS: Record<IssueSeverity, string> = {
  error: "text-danger",
  warning: "text-warning",
  info: "text-fg-subtle",
};

/** 体检结论的紧凑版。完整报告（含每一项检查的定义）在设置页。 */
function DiagnosisSummary({ diagnosis }: { diagnosis: PathDiagnosis }) {
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-fg-muted">
        <span>文件系统 {diagnosis.filesystem ?? "未知"}</span>
        <span>卷类型 {formatVolumeKind(diagnosis.volumeKind)}</span>
        <span>可用空间 {formatBytes(diagnosis.freeBytes)}</span>
      </div>

      {diagnosis.canInitialize ? (
        <div className="flex items-center gap-2 text-xs text-success">
          <CheckCircle2 className="size-3.5" />
          这个位置可以用作中央库
        </div>
      ) : null}

      {diagnosis.issues.length > 0 ? (
        <ul className="space-y-1">
          {diagnosis.issues.map((issue) => (
            <IssueLine key={issue.code} issue={issue} />
          ))}
        </ul>
      ) : null}
    </div>
  );
}

function IssueLine({ issue }: { issue: DiagnosisIssue }) {
  const Icon = SEVERITY_ICON[issue.severity];
  return (
    <li className="flex items-start gap-2 text-xs leading-relaxed">
      <Icon
        className={cn(
          "mt-0.5 size-3.5 shrink-0",
          SEVERITY_CLASS[issue.severity],
        )}
      />
      <span className="text-fg-muted">{issue.message}</span>
    </li>
  );
}

// ===========================================================================
// 第 2 步：连接 Agent
// ===========================================================================

/**
 * 第 2 步：把"哪些 Agent 能接入"这件事**说清楚**。
 *
 * 这一步的存在本身就是一条教训：只有**技能目录已存在**的 Agent 才会出现在
 * 「启用」的目标里。有些 Agent 的技能目录是按需创建的（Claude Code 就是），
 * 在那之前它接不进来——而界面原本对此**一句话都不说**，用户会合理地以为
 * "全部启用"覆盖了他所有的 Agent。所以这里显式列出做不到的那些，并给出原因。
 */
function AgentsStep({
  onBack,
  onDone,
  onSkip,
}: {
  onBack: () => void;
  onDone: () => void;
  onSkip: () => void;
}) {
  const { data: detections, isPending } = useAgentsDetect();
  const createDir = useCreateAgentSkillDir();

  const detected = detections?.filter((d) => d.status === "detected") ?? [];

  return (
    <div className="space-y-4">
      <p className="text-sm leading-relaxed text-fg-muted">
        下面是这台机器上检测到的 Agent。只有技能目录已经存在的 Agent 才能接收
        Skill——Claude Code 这类按需创建目录的，可以在这里一键建出来。
      </p>

      {isPending ? (
        <div className="flex items-center gap-2 text-sm text-fg-muted">
          <Loader2 className="size-4 animate-spin" />
          正在检测…
        </div>
      ) : null}

      {detections ? (
        <ul className="max-h-72 space-y-2 overflow-y-auto">
          {detections.map((detection) => (
            <AgentRow
              key={detection.id}
              detection={detection}
              creating={
                createDir.isPending && createDir.variables === detection.id
              }
              onCreate={() => createDir.mutate(detection.id)}
            />
          ))}
        </ul>
      ) : null}

      {detections && detected.length === 0 ? (
        <div className="flex items-start gap-2 rounded-md border border-warning/40 bg-warning-subtle p-3 text-xs leading-relaxed text-fg-muted">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-warning" />
          <span>
            目前没有任何 Agent 可以接收 Skill。你可以先去安装一个 Agent，
            或直接进入下一步——中央库本身随时可以开始使用。
          </span>
        </div>
      ) : null}

      <div className="flex items-center justify-between border-t border-border pt-4">
        <div className="flex gap-2">
          <Button variant="ghost" onClick={onBack}>
            上一步
          </Button>
          <Button variant="ghost" onClick={onSkip}>
            稍后再说
          </Button>
        </div>
        <Button onClick={onDone}>
          下一步
          <ArrowRight className="size-4" />
        </Button>
      </div>
    </div>
  );
}

function AgentRow({
  detection,
  creating,
  onCreate,
}: {
  detection: AgentDetection;
  creating: boolean;
  onCreate: () => void;
}) {
  return (
    <li className="flex items-start gap-3 rounded-md border border-border bg-bg-subtle p-3">
      <StatusIcon status={detection.status} />
      <div className="min-w-0 flex-1 space-y-1">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium text-fg">
            {detection.displayName}
          </span>
          {detection.isUserOverride ? (
            <span className="text-[11px] text-fg-subtle">（手动指定）</span>
          ) : null}
        </div>

        {detection.status === "detected" && detection.skillDir ? (
          <code
            className="block truncate font-mono text-[11px] text-fg-muted"
            title={detection.skillDir}
          >
            {detection.skillDir}
          </code>
        ) : null}

        {detection.status === "installedNoSkillDir" ? (
          <div className="space-y-2">
            <p className="text-xs leading-relaxed text-fg-muted">
              已安装，但技能目录还不存在，暂时无法接收 Skill。
            </p>
            {detection.skillDir ? (
              <code
                className="block truncate font-mono text-[11px] text-fg-subtle"
                title={detection.skillDir}
              >
                {detection.skillDir}
              </code>
            ) : (
              <p className="text-xs text-fg-subtle">
                且未能确定它的技能目录位置，需在设置页手动指定。
              </p>
            )}
          </div>
        ) : null}

        {detection.status === "notInstalled" ? (
          <p className="text-xs leading-relaxed text-fg-subtle">
            未检测到安装，已跳过。
          </p>
        ) : null}
      </div>

      {detection.status === "installedNoSkillDir" && detection.skillDir ? (
        <Button
          size="sm"
          variant="secondary"
          disabled={creating}
          onClick={onCreate}
        >
          {creating ? <Loader2 className="size-3.5 animate-spin" /> : null}
          创建目录
        </Button>
      ) : null}
    </li>
  );
}

function StatusIcon({ status }: { status: AgentDetection["status"] }) {
  if (status === "detected") {
    return <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-success" />;
  }
  if (status === "installedNoSkillDir") {
    return <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" />;
  }
  return <Info className="mt-0.5 size-4 shrink-0 text-fg-subtle" />;
}

// ===========================================================================
// 第 3 步：启用第一个 Skill
// ===========================================================================

/**
 * 第 3 步：挑一个中央库里的 Skill，接到选中的 Agent 上。
 *
 * 全新环境里中央库是空的，因此这里必须能就地导入——否则"启用第一个 Skill"
 * 在真正的首次运行中根本走不通（没有可启用的东西）。导入复用导入向导，
 * 不另做一套。
 */
function EnableStep({
  libraryPath,
  onBack,
  onDone,
  onImport,
}: {
  libraryPath: string;
  onBack: () => void;
  onDone: () => void;
  onImport: () => void;
}) {
  const { data: scan } = useSkillsScan(libraryPath);
  const setEnabled = useSetLinkEnabled();

  const [selectedSkill, setSelectedSkill] = useState<string | null>(null);
  const [selectedAgents, setSelectedAgents] = useState<string[] | null>(null);
  const [failed, setFailed] = useState<string[]>([]);

  const inLibrary: SkillSummary[] =
    scan?.skills.filter((s) => s.inCentralLibrary) ?? [];
  const readyAgents = scan?.agents.filter((a) => a.status === "detected") ?? [];
  const blockedAgents =
    scan?.agents.filter((a) => a.status === "installedNoSkillDir") ?? [];

  // 默认全选"能接的" Agent。这是可见的勾选，不是静默决定。
  const agents = selectedAgents ?? readyAgents.map((a) => a.id);
  const skill = selectedSkill ?? inLibrary[0]?.dirName ?? null;

  const toggleAgent = (id: string) => {
    setSelectedAgents(
      agents.includes(id) ? agents.filter((a) => a !== id) : [...agents, id],
    );
  };

  const enable = async () => {
    if (!skill || agents.length === 0) return;
    setFailed([]);
    try {
      const results = await setEnabled.mutateAsync({
        skillDirName: skill,
        agentIds: agents,
        enabled: true,
      });
      // 部分失败必须拦住，不能让用户带着"已经好了"的印象离开引导
      const problems = results
        .filter((r) => r.action === "failed" || r.action === "skipped")
        .map((r) => `${r.agentId}：${r.message ?? r.errorKind ?? r.action}`);
      if (problems.length > 0) {
        setFailed(problems);
        return;
      }
      onDone();
    } catch (error) {
      setFailed([error instanceof Error ? error.message : "启用失败"]);
    }
  };

  return (
    <div className="space-y-4">
      {inLibrary.length === 0 ? (
        <div className="space-y-3 rounded-md border border-border bg-bg-subtle p-4 text-center">
          <PackagePlus className="mx-auto size-5 text-fg-subtle" />
          <p className="text-sm font-medium text-fg">中央库还是空的</p>
          <p className="text-xs leading-relaxed text-fg-muted">
            先放一个 Skill 进来，才能把它接到 Agent 上。 可以从文件夹、ZIP、Git
            仓库或单个 SKILL.md 导入。
          </p>
          <Button variant="secondary" onClick={onImport}>
            <PackagePlus className="size-4" />
            导入 Skill…
          </Button>
        </div>
      ) : (
        <>
          <div className="space-y-2">
            <div className="text-xs font-medium text-fg-subtle">
              选择要启用的 Skill
            </div>
            <ul className="max-h-40 space-y-1 overflow-y-auto">
              {inLibrary.map((item) => (
                <li key={item.dirName}>
                  <label
                    className={cn(
                      "flex cursor-pointer items-start gap-2 rounded-sm border p-2",
                      item.dirName === skill
                        ? "border-accent bg-accent-subtle"
                        : "border-border bg-bg-subtle hover:bg-surface-hover",
                    )}
                  >
                    <input
                      type="radio"
                      name="onboarding-skill"
                      className="mt-0.5"
                      checked={item.dirName === skill}
                      onChange={() => setSelectedSkill(item.dirName)}
                    />
                    <span className="min-w-0">
                      <span className="block truncate text-sm text-fg">
                        {item.name}
                      </span>
                      {item.description ? (
                        <span className="block truncate text-xs text-fg-muted">
                          {item.description}
                        </span>
                      ) : null}
                    </span>
                  </label>
                </li>
              ))}
            </ul>
          </div>

          <div className="space-y-2">
            <div className="text-xs font-medium text-fg-subtle">
              启用到哪些 Agent
            </div>
            {readyAgents.length === 0 ? (
              <p className="text-xs leading-relaxed text-warning">
                没有可用的 Agent。回到上一步创建技能目录，或先去设置页指定目录。
              </p>
            ) : (
              <ul className="space-y-1">
                {readyAgents.map((agent) => (
                  <li key={agent.id}>
                    <label className="flex cursor-pointer items-center gap-2 rounded-sm border border-border bg-bg-subtle p-2">
                      <input
                        type="checkbox"
                        checked={agents.includes(agent.id)}
                        onChange={() => toggleAgent(agent.id)}
                      />
                      <span className="min-w-0 flex-1 truncate text-sm text-fg">
                        {agent.displayName}
                      </span>
                      {agent.skillDir ? (
                        <code
                          className="hidden max-w-[50%] truncate font-mono text-[11px] text-fg-subtle sm:block"
                          title={agent.skillDir}
                        >
                          {agent.skillDir}
                        </code>
                      ) : null}
                    </label>
                  </li>
                ))}
              </ul>
            )}

            {/* 静默地少做一件事比报错危险得多：做不到的那些必须说出来 */}
            {blockedAgents.length > 0 ? (
              <p className="text-xs leading-relaxed text-fg-muted">
                另有 {blockedAgents.length} 个 Agent（
                {blockedAgents.map((a) => a.displayName).join("、")}
                ）技能目录尚未创建，本次不会接入。可在上一步或设置页里创建。
              </p>
            ) : null}
          </div>
        </>
      )}

      {failed.length > 0 ? (
        <div className="space-y-1 rounded-md border border-danger/40 bg-danger-subtle p-3">
          <div className="flex items-center gap-2 text-xs font-medium text-danger">
            <XCircle className="size-3.5" />
            没能完成启用
          </div>
          <ul className="space-y-0.5 text-xs leading-relaxed text-danger">
            {/* 用下标做 key：两条失败的原因文案完全可能一模一样
                （例如两个 Agent 都因目录被占用而失败），拿文本当 key 会撞。 */}
            {failed.map((line, index) => (
              <li key={index}>{line}</li>
            ))}
          </ul>
        </div>
      ) : null}

      <div className="flex items-center justify-between border-t border-border pt-4">
        <Button variant="ghost" onClick={onBack}>
          上一步
        </Button>
        <div className="flex gap-2">
          {inLibrary.length === 0 ? (
            <Button variant="ghost" onClick={onDone}>
              稍后再说
            </Button>
          ) : null}
          <Button
            onClick={() => void enable()}
            disabled={
              !skill ||
              agents.length === 0 ||
              setEnabled.isPending ||
              inLibrary.length === 0
            }
          >
            {setEnabled.isPending ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <PlugZap className="size-4" />
            )}
            启用
          </Button>
        </div>
      </div>
    </div>
  );
}
