# Agent 技能目录 —— 实测报告

> 生成方式：对 `%USERPROFILE%` 下各 Agent 根目录做**只读**探测（列目录 + 有界递归查找 `skills` 目录 + 查 PATH）。
> **未创建、未修改任何 Agent 目录。**
>
> 侦察时间：2026-10-01　机器：Windows 11 Home 22631

---

## 1. 结论速览

| Agent | 候选路径 | 实测结论 | 置信度变化 |
| --- | --- | --- | --- |
| **Codex** | `%USERPROFILE%\.codex\skills` | ✅ **确认存在且有效**，内含真实 SKILL.md | 低 → **已证实** |
| **Claude Code** | `%USERPROFILE%\.claude\skills` | ⚠️ **本机不存在**（Agent 已装，但尚未添加过用户 Skill） | 高 → **待观察** |
| **Trae** | `%USERPROFILE%\.trae\skills` | ❌ **不存在**。`~/.trae` 是 VS Code 系布局 | 低 → **已证伪** |
| Cursor | `%USERPROFILE%\.cursor\skills` | ➖ 未安装，无法验证 | 未变 |
| Windsurf | `%USERPROFILE%\.codeium\windsurf\skills` | ➖ 未安装，无法验证 | 未变 |

> **原有判断中"Claude Code 置信度高"在本机未获实证** —— 目录只有在用户添加过 Skill 后才会被创建。
> 同理"Codex 置信度低"反而被证实。这正是"禁止把未验证路径写成确定事实"的价值所在。

## 2. 逐项实测记录

### 2.1 Codex —— ✅ 确认

```
C:\Users\Zt248\.codex\
├─ skills\
│  └─ .system\
│     ├─ .codex-system-skills.marker
│     ├─ imagegen\          ├─ SKILL.md  ├─ agents\  ├─ assets\
│     │                     ├─ references\ └─ scripts\
│     ├─ openai-docs\       └─ SKILL.md
│     ├─ plugin-creator\
│     ├─ review-agent\
│     ├─ skill-creator\
│     └─ skill-installer\
├─ config.toml
├─ auth.json
└─ (多个 sqlite 状态库)
```

**关键观察**

1. 技能目录**确实**是 `~/.codex/skills`，与本项目约定一致。
2. ⚠️ **技能并非直接位于 `skills/` 下，而是嵌套在分组目录 `.system/` 中**，
   即 `skills/<分组>/<技能名>/SKILL.md`。
   **朴素的 `skills/*/SKILL.md` 扫描会完全漏掉它们**（扫到的 `.system` 目录本身没有 SKILL.md）。
3. `.system` 以点开头，是 Codex 的内置技能分组。
4. 文件结构与本项目完全兼容：每个技能一个目录 + `SKILL.md` + 可选 `assets/` `references/` `scripts/`。

### 2.2 Claude Code —— ⚠️ 目录不存在

```
C:\Users\Zt248\.claude\
├─ backups\  cache\  file-history\  paste-cache\  plugins\
├─ projects\  session-env\  sessions\  shell-snapshots\
├─ settings.json  history.jsonl
└─ （没有 skills\ 子目录）
```

- `claude` CLI **已安装**（`D:\Scoop\shims\claude.exe`），说明 Agent 存在。
- `.claude/plugins/` 存在，但在深度 3 内**未发现任何 `skills` 目录**。
- 结论：`~/.claude/skills` 是**按需创建**的目录。检测逻辑必须区分
  「Agent 已安装」与「技能目录已存在」两件事，否则会把已装的 Claude Code 判为未安装。

### 2.3 Trae —— ❌ 候选路径证伪

```
C:\Users\Zt248\.trae\        C:\Users\Zt248\.trae-cn\
├─ builtin\                  ├─ builtin\
│  ├─ global\skills\   (8)   │  └─ global\skills\   (8)
│  └─ trae\<变体>\skills\ (2~3)  ├─ extensions\
├─ extensions\               └─ toolhost\
├─ toolhost\
└─ argv.json
```

- 布局是 **VS Code 系**（`extensions/` + `toolhost/` + `argv.json` + `builtin/`）。
- 找到的 `skills` 目录**全部位于 `builtin/` 之下**，是**随应用分发的内置技能**，
  且按内部代号分多个变体（`trae\default`、`trae\iris`、`trae\medea` 等）。
- **`~/.trae/skills` 不存在**，且**没有找到任何用户可管理的技能目录**。
- 同时存在 `.trae`（国际版）与 `.trae-cn`（中国版）两套安装。

> **决策**：不把 Trae 的 `builtin/**/skills` 作为默认技能目录。
> 那是应用内部资源，把它当作"用户的 Skill"会让用户看到一堆自己从未安装的条目。
> Trae 在本项目中**保留为可手动配置的自定义 Agent**，等待后续实测其用户技能目录。

### 2.4 未安装的 Agent

`.cursor`、`.codeium`、`.windsurf` 均不存在，对应 CLI（`cursor` / `windsurf`）也不在 PATH。
对策：「检测失败即从 UI 隐藏」。

### 2.5 其他观察

| 目录 | 说明 |
| --- | --- |
| `~/.skills-manager` | **参考项目已安装在本机**。因此本项目与其保持区分十分必要。**未读取其内容，也未复制其任何设计。** |
| `~/.copilot` | GitHub Copilot 的 IDE 扩展配置（`ide/` `logs/` `config.json`），无技能目录 |
| `~/.cc-switch`、`~/.cherrystudio`、`~/.workbuddy`、`~/.ai_completion` | 其他工具，与本项目无关 |

---

## 3. 对实现的直接影响

实测结果带来三条必须落到代码里的约束：

### 3.1 扫描必须支持**嵌套分组**

不能只扫 `skills/*/SKILL.md`。设计为：

- 扫描 `skills/` 下**深度 ≤ 2** 的目录，凡是包含 `SKILL.md` 的目录即视为一个 Skill；
- 若 Skill 位于 `skills/<分组>/<技能名>/`，记录 `group = <分组>`；
- 分组名以 `.` 开头时标记 `isSystemGroup = true`（Codex 的 `.system`）。

这样 Codex 的 6 个内置技能能被正确发现，同时 UI 可以按需过滤或单独标注来源。

### 3.2 检测状态必须三分，而不是布尔

「Agent 是否安装」与「技能目录是否存在」是两件事：

| 状态 | 含义 | UI 表现 |
| --- | --- | --- |
| `notInstalled` | 根目录与 CLI 都不存在 | 隐藏 |
| `installedNoSkillDir` | Agent 已装，但技能目录尚未创建 | 显示，提示"尚无技能目录"，允许用户手动指定或等待其自动创建 |
| `detected` | 技能目录存在 | 正常显示并扫描 |

### 3.3 不跟随重解析点

扫描 Agent 目录时必须跳过 junction，否则已纳管的 Skill 会被按 Agent 数量重复计数，
表现为「Skill 数量暴涨」。这与 `platform::link` 的约束一致。

---

## 4. 待后续实测的项

| 项 | 说明 |
| --- | --- |
| Cursor 用户技能目录 | 本机未安装，无法验证；保持候选状态并依赖用户手填 |
| Windsurf 用户技能目录 | 同上 |
| Trae 用户技能目录 | `~/.trae` 为 VS Code 系布局，用户技能目录未知。**需要 Trae 用户添加一个技能后再次侦察** |
| Claude Code 用户技能目录 | 待用户添加首个 Skill 后确认路径确实为 `~/.claude/skills` |

> 复现方法：在任一 Agent 中添加一个 Skill，重新运行本报告第 2 节的探测命令，
> 把结论回填到本节与本文件顶部的速览表。
