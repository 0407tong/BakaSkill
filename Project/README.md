# BakaSkill

统一管理多个 AI 编程 Agent（Claude Code、Cursor、Codex、Windsurf、Trae 等）的 Skill。

**核心能力**

- **自定义存储位置**：中央库可放在任意盘符 / 文件夹，不再挤占 C 盘
- **一键启用 / 禁用**：通过 junction 把中央库中的 Skill 映射到各 Agent 技能目录
- **链接透明可查**：以「Skill × Agent」矩阵展示每一格的真实链接状态与目标路径
- **GitHub 同步**：把中央库备份到自己的仓库

**定位**：轻量化管理工具，**不内置 Skill 市场**。

> 用户向的项目说明见上级目录的 [`README.md`](../README.md)。
> 技术选型与架构约定见 [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)。
> 打包、安装与**卸载行为**见 [`docs/DISTRIBUTION.md`](docs/DISTRIBUTION.md)。
> 版本变更见 [`CHANGELOG.md`](CHANGELOG.md)。

---

## 环境要求

| 依赖 | 版本 | 说明 |
| --- | --- | --- |
| Node.js | ≥ 20 | 本机实测 v26.10.0 |
| pnpm | ≥ 9 | 本机实测 12.8.2 |
| Rust | stable ≥ 1.80 | 本机实测 1.98.1，target `x86_64-pc-windows-msvc` |
| MSVC 生成工具 | VS 2022 Build Tools（含 C++ 工作负载） | Rust 链接必需 |
| WebView2 Runtime | 任意 | Windows 10/11 通常已预装 |

## 开发

```bash
pnpm install          # 安装依赖
pnpm tauri:dev        # 启动桌面应用（开发模式）
pnpm tauri:build      # 打包安装程序（NSIS + MSI）
```

## 脚本

| 命令 | 作用 |
| --- | --- |
| `pnpm dev` | 仅启动前端（Vite，端口 1420） |
| `pnpm tauri:dev` | 启动完整桌面应用 |
| `pnpm build` | 类型检查 + 前端产物构建 |
| `pnpm typecheck` | `tsc --noEmit` |
| `pnpm lint` / `pnpm lint:fix` | ESLint |
| `pnpm format` / `pnpm format:check` | Prettier |
| `pnpm tauri:build` | 打包安装程序 |

Rust 侧检查（本机 Scoop 安装的 rust 未生成 clippy / rustfmt 的 shim，
若提示命令不存在，把 `D:\Scoop\apps\rust\current\bin` 加入 PATH 后再执行）：

```bash
cd src-tauri
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## 目录结构

```
Project/
├─ src/                     # 前端（React 19 + TypeScript）
│  ├─ components/
│  │  ├─ layout/            # AppShell / Sidebar / TopBar / DetailPanel / GlobalBanner
│  │  ├─ settings/          # CentralPathPicker、LinkMappingView、GitHubSyncPanel
│  │  ├─ skill/             # SkillCard、SkillListItem、SkillToggle、SkillEditor
│  │  └─ ui/                # 无样式基座组件（Radix primitives）的 Tailwind 实现
│  ├─ views/                # 主内容区各视图
│  ├─ store/                # Zustand 状态
│  ├─ lib/                  # ipc / queries / theme / cn
│  ├─ types/                # 与 Rust 侧对应的 IPC 契约类型
│  └─ styles/globals.css    # 设计令牌 + Tailwind 入口
└─ src-tauri/               # 后端（Rust）
   ├─ capabilities/         # Tauri 2 权限配置（最小权限原则）
   └─ src/
      ├─ commands/          # 薄命令层
      ├─ platform/          # 平台专有调用的唯一收敛点
      └─ error.rs           # 统一错误类型
```

## 数据流

```
React 组件
  └─ TanStack Query hook (src/lib/queries.ts)
       └─ call<T>() (src/lib/ipc.ts)  ← 统一错误归一化为 IpcError
            └─ invoke("命令名")
                 └─ Rust #[tauri::command]
                      └─ 领域模块 → AppResult<T>
```

约定：**组件不得直接调用 `invoke`**，否则会绕过错误归一化，UI 将拿到无法识别的错误对象。

## 文档

| 文档 | 内容 |
| --- | --- |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | 技术选型、架构约定；**§6 是实测踩过的坑，动手前必读** |
| [`docs/LINKING.md`](docs/LINKING.md) | junction 机制、安全约束、故障排查 |
| [`docs/GITHUB_SYNC.md`](docs/GITHUB_SYNC.md) | 同步模型与"不破坏用户数据"的实现依据 |
| [`docs/DISTRIBUTION.md`](docs/DISTRIBUTION.md) | 打包、安装、**卸载行为** |
| [`docs/AGENT_PATHS_MEASURED.md`](docs/AGENT_PATHS_MEASURED.md) | 各 Agent 技能目录的实测报告 |
| [`docs/PERF.md`](docs/PERF.md) | 性能基准实测数据 |

## 功能一览

中央库与链接管理、多 Agent 扫描与管理界面、一键启停与映射矩阵、
GitHub 同步、搜索与标签、导入导出与打包分发。
