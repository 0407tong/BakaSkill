# BakaSkill 架构说明

本文档记录**实际落地**的技术选型与架构约定。文中与早期规划若有出入，
以本文档为准，并在「偏离与决策记录」中登记原因。

---

## 1. 技术栈与落地版本

版本为本项目 `pnpm-lock.yaml` / `Cargo.lock` 中的实际解析结果，非"最新可用版本"。

### 前端

| 组件 | 版本 | 说明 |
| --- | --- | --- |
| React / React DOM | 19.3.0 | 函数组件 + Hooks；不再使用 `forwardRef`（React 19 支持 ref 作为普通 prop） |
| TypeScript | 6.0.3 | `strict` + `noUncheckedIndexedAccess` |
| Vite | 8.3.1 | 构建与 HMR，Tauri 固定端口 1420 |
| Tailwind CSS | 4.3.3 | CSS-first 配置（无 `tailwind.config.js`） |
| @tailwindcss/vite | 4.3.3 | Vite 插件接入方式 |
| Zustand | 5.0.15 | UI 状态 + `persist` 中间件 |
| TanStack Query | 5.104.0 | IPC 数据缓存 |
| Radix UI | dialog 1.1.23 / slot 1.3.3 / switch 1.3.7 / toggle-group 1.1.19 / tooltip 1.2.16 | **无样式**原语，视觉由本项目 Tailwind 实现 |
| react-resizable-panels | 4.14.1 | 面板拖拽（**v4 API，见 §6**） |
| lucide-react | 1.49.0 | 图标 |
| sonner | 2.0.8 | Toast |
| clsx + tailwind-merge | 2.1.1 / 3.7.0 | `cn()` 类名合并 |
| ESLint / Prettier | 10.11.0 / 3.9.9 | 扁平配置 `eslint.config.js` |

### 后端

| 组件 | 版本 | 说明 |
| --- | --- | --- |
| Rust | 1.98.1 | target `x86_64-pc-windows-msvc` |
| tauri | 2.12.1 | |
| tauri-plugin-log | 2.10.0 | 启用 `tracing` feature，全项目统一用 `tracing` 宏 |
| tauri-plugin-dialog | 2.8.1 | 目录选择器 |
| tauri-plugin-opener | 2.7.0 | 用系统程序打开路径 |
| thiserror | 2.0.21 | 错误类型派生 |
| tracing | 0.1.44 | 启用 `log` feature 做桥接（见 §6.5） |
| serde / serde_json | 1.0.229 | |
| junction | 2.1.0 | 目录链接。**API 有三个非直觉行为，见 §6.8** |
| rusqlite | 0.40.2 | `bundled` feature（自带 SQLite，不依赖系统库） |
| serde_yaml | 0.9.34 | frontmatter。crates.io 上标记 deprecated，但其 `Mapping` 底层是 `IndexMap`，**天然保序**，是当前满足"保序 + 无损往返"最省事的选择 |
| windows-sys | 0.61.2 | 卷信息与重解析标签查询（`Win32_Foundation` + `Win32_Storage_FileSystem`） |
| dunce | 1.0.5 | 路径规范化（去 `\\?\` 前缀） |
| tempfile | 3.27.0 | 仅测试用 |

---

## 2. 分层架构

```
┌─────────────────────────────────────────────────────────┐
│  前端 React                                              │
│  components/  ← viewStore(Zustand) + TanStack Query      │
├─────────────────────────────────────────────────────────┤
│  lib/ipc.ts   ← 所有 invoke 的唯一出口，错误归一化        │
├──────────────────────── IPC ────────────────────────────┤
│  commands/    ← 薄适配层：参数校验 + 结果转换             │
├─────────────────────────────────────────────────────────┤
│  领域模块（library / links / scanner / index / git ...）  │
├─────────────────────────────────────────────────────────┤
│  platform/    ← 平台专有调用的唯一收敛点                  │
│                 Windows 实现 + 非 Windows stub            │
└─────────────────────────────────────────────────────────┘
```

**硬性约定**

1. 前端组件**不得直接 `invoke`**，必须经 `src/lib/ipc.ts` 的 `call<T>()`。
   绕过它会丢失错误归一化，UI 将拿到无法识别的错误对象。
2. `commands/` 保持薄。业务逻辑放领域模块，以便脱离 Tauri 运行时做单元测试。
3. 所有平台专有调用（junction、卷信息、长路径处理）**只能**出现在 `platform/`。
   非 Windows 分支返回 `AppError::Unsupported`——必须是显式 `Err`，**不得用
   `unimplemented!()` / `panic!`**，否则跨平台"编译通过但一跑就崩"。

---

## 3. IPC 契约

### 错误形状

Rust 侧 `AppError`（`src-tauri/src/error.rs`）序列化为：

```json
{ "kind": "PermissionDenied", "message": "权限不足：拒绝访问 D:\\..." }
```

`kind` 取值与前端 `src/types/ipc.ts` 的 `AppErrorKind` **一一对应**：

`Internal` · `Unsupported` · `Io` · `Config` · `NotFound` · `PermissionDenied` · `Conflict`

> ⚠️ 改动任一侧的取值集合时，必须同步另一侧。这是目前唯一靠人工保证的契约，
> 尚无编译期约束（`ts-rs` / `tauri-specta` 可在后续引入）。

### 字段命名

Rust 结构体统一加 `#[serde(rename_all = "camelCase")]`，与 TypeScript 习惯一致。
例：`app_version` ↔ `appVersion`，`timestamp_ms` ↔ `timestampMs`。

### 命令清单

| 命令 | 输入 | 输出 | 用途 |
| --- | --- | --- | --- |
| `ping` | — | `PongPayload` | 连通性探针，用于快速定位 capabilities 权限漏配 |

---

## 4. 设计令牌

单一来源是 `src/styles/globals.css` 中的 CSS 变量：

```
:root { --sh-bg, --sh-fg, --sh-accent, ... }   /* 亮色 */
.dark { ... }                                   /* 暗色覆盖 */

@theme inline {
  --color-bg: var(--sh-bg);   /* 生成 bg-bg / text-bg 等工具类 */
  ...
}
```

**关键点**

- 用 `@theme inline`（而非 `@theme`），颜色工具类才会直接引用变量，
  从而支持运行时切换主题。
- Tailwind v4 的 `dark:` 变体默认跟随 `prefers-color-scheme`。本项目需要
  「跟随系统 + 手动覆盖」，因此在 globals.css 中改为 class 驱动：
  `@custom-variant dark (&:where(.dark, .dark *));`
- **防闪烁**：主题在 `index.html` 的内联脚本中于首屏渲染前应用，
  读取 zustand persist 写入 `localStorage["bakaskill.ui"]` 的 `themeMode`。
  改动该 storage key 时必须同步修改内联脚本。

命名约定：`--color-fg` / `--color-fg-muted` / `--color-fg-subtle` 构成文字层次，
`--color-bg` < `--color-bg-subtle` < `--color-surface` 构成背景层次。

---

## 5. 状态管理职责划分

| 状态类型 | 归属 | 例子 |
| --- | --- | --- |
| 界面状态 | `store/viewStore.ts`（Zustand） | 当前视图、网格/列表、主题、侧边栏折叠、选中项 |
| 后端数据 | TanStack Query（`lib/queries.ts`） | Skill 列表、链接状态、配置 |
| 组件内瞬态 | `useState` | 搜索框输入、对话框开合 |

`viewStore` 用 `partialize` 只持久化界面偏好，选中项与详情面板开合不跨会话保留
（避免重开应用后详情面板指向一个已不存在的 Skill）。

面板宽度单独持久化在 `localStorage["bakaskill.layout.workspace"]`，由 AppShell 管理。

---

## 6. 已知陷阱（本项目实测记录）

以下每条都是实际踩到或明确验证过的，不是推测：

### 6.1 react-resizable-panels v4 API 与 v2/v3 完全不同

| v2 / v3（旧记忆） | v4（本项目实际） |
| --- | --- |
| `PanelGroup` | **`Group`** |
| `PanelResizeHandle` | **`Separator`** |
| `direction="horizontal"` | **`orientation="horizontal"`** |
| `autoSaveId`（自动持久化） | **已移除**，需手动存取 `Layout` |

`Layout` 类型为 `{ [panelId: string]: number }`（百分比 0–100）。
保存时优先用 `onLayoutChanged` 回调的第二参数 `meta.requestedLayout`，
文档明确说明约束重算后的校验值不应被持久化。

### 6.2 TypeScript 6 已弃用 `baseUrl`

`tsconfig.json` 中只写 `paths` 即可，解析基准是 tsconfig 所在目录。
写了 `baseUrl` 会直接报 `TS5101` 错误（不是警告）。

### 6.3 lucide-react 是 1.x，不是 0.x

`pnpm add lucide-react` 解析到 **1.49.0**。手写 `"^0.5"` 之类的版本范围会装不上。

### 6.4 eslint-plugin-react-hooks v7 的扁平配置入口

`configs` 对象包含 `recommended`、`recommended-latest` 与 `flat`。
扁平配置应使用 **`configs.flat.recommended`**；`configs.recommended` 是
eslintrc 时代的形状（`plugins` 为字符串数组），直接放进 flat config 会报错。

### 6.5 tauri-plugin-log 的 `tracing` feature 是个陷阱

**插件自身的 `tracing` feature 与「Rust 侧能否用 tracing 宏」毫无关系。**
它只让**前端 JS 侧**的 log 命令额外向 tracing 系统发事件，
对 Rust 侧 `tracing::info!` 是否输出没有任何影响。

插件安装的是 `log` 门面的 logger，其 `Builder::level()` 接受的也是
`log::LevelFilter`（不是 `tracing::Level`，传错会报
`trait bound ... From<tracing::Level> is not satisfied`）。

**正确做法**：给 `tracing` crate 启用 `log` feature 做桥接，
使 tracing 事件同时产生 log 记录，从而被插件捕获。

```toml
tauri-plugin-log = "2"
tracing = { version = "0.1", features = ["log"] }
```

```rust
use tauri_plugin_log::log::LevelFilter;   // 插件公开再导出了 log

tauri_plugin_log::Builder::new()
    .level(LevelFilter::Info)
    .build()
```

### 6.6 本机 rust 由 Scoop 安装，无 rustup、无 clippy/rustfmt shim

- 无 `rustup`，因此**无法用 `rustup target add` 添加交叉编译目标**
  （影响跨平台构建，届时需另寻方案）。
- `cargo-clippy.exe` / `rustfmt.exe` 存在于
  `D:\Scoop\apps\rust\current\bin`，但 Scoop 未为其生成 shim，
  PATH 里找不到。临时把该目录加入 PATH 即可调用。
- `link.exe` 不在 PATH 属正常：MSVC 靠 vcvars 注入环境变量，
  rustc 会自行探测 VS 安装位置（**已实测编译验证有效**）。
- 本机同时装了 MinGW（Scoop `mingw` 包），与 MSVC 工具链并存。
  构建时以 `rustc -vV` 的 host 为准，当前为 `x86_64-pc-windows-msvc`。

### 6.7 capabilities 靠窗口 label 匹配，label 不匹配时 invoke 静默失败


`capabilities/default.json` 的 `"windows": ["main"]` 是按**窗口 label** 匹配的。
本项目 `tauri.conf.json` 未显式设置 `label`，Tauri 的 `default_window_label()`
返回 `"main"`（源码 `tauri-utils/src/config.rs`），因此能匹配上。

**风险**：若日后新增窗口却忘记设 `label`，或改了现有窗口的 label 而没同步
capabilities，`invoke` 会以晦涩错误失败——表现为"后端命令不存在"而非"权限不足"。
这正是保留 `ping` 探针的意义：它是这类问题的最快定位手段。

### 6.8 `junction` crate 的三个非直觉行为

完整分析见 [`LINKING.md`](LINKING.md) §4，此处只列结论：

1. **`FileType::is_symlink()` 对 junction 返回 `true`**（junction 是 name-surrogate
   重解析点）。因此 `classify()` 必须先判 junction 再判 symlink，顺序颠倒会让
   每个链接都被误判为符号链接，`delete_junction` 全面拒绝执行。
2. **`junction::delete()` 留下一个真实空目录**：底层 `FSCTL_DELETE_REPARSE_POINT`
   只摘除重解析点、不删目录项。必须补一次 `remove_dir`（只能删空目录，安全）。
3. **`junction::exists()` 识别不了断链**：其实现开头是 `Path::exists()`，会跟随
   链接，目标不存在时返回 `false`。本项目改用重解析标签自行判定。

另外 `junction::create(target, junction)` 的**参数顺序是目标在前**，与直觉相反。

### 6.10 react-resizable-panels v4 的尺寸单位：数字是**像素**不是百分比

这是本项目踩得最实的一个坑，症状与根因隔得很远：

| 写法 | 实际含义 |
| --- | --- |
| `defaultSize={34}` | **34 像素** |
| `defaultSize="34"` / `defaultSize="34%"` | 34% |
| `Group` 的 `defaultLayout={{main: 66, detail: 34}}` | **66% / 34%**（`Layout` 类型一律是百分比） |

**症状**：详情面板显示为一条窄缝，且拖拽分隔条几乎没有反应
（只能在 22–60 像素之间移动）。

**根因**：`Panel` 的 `defaultSize` / `minSize` / `maxSize` 传了裸数字，
被当成像素而非百分比。

**教训**：`Group` 与 `Panel` 对同一件事（尺寸）使用**不同的单位约定**。
跨库、跨版本时不要凭直觉假设单位——查 `.d.ts` 上的注释比读实现代码快得多。

### 6.11 虚拟化列表：可变内容必须用 `measureElement` 实测行高

`@tanstack/react-virtual` 把每一行**绝对定位**在纵向偏移上，
行与行之间**不会互相让位**。因此只要某一行的实际高度超过
`estimateSize` 给出的估值，多出来的部分就会盖住下一行。

**本项目的踩坑过程值得记下来，因为第一次修复是错的方向：**

1. 现象：点击卡片后，该卡片多出一个徽章并换行 → 卡片变高 →
   但下一行不动 → 重叠。
2. 第一次修复：给卡片加 `overflow-hidden` + 把估值从 188 调到 208。
   **这是治标**：只是把内容裁掉，卡片高度实际上仍随内容变化
   （有无描述、有无标签、有无告警都会改变高度），换一批数据仍会出问题。
3. 正确修复：用 `measureElement` 把每行的**真实高度**量出来反馈给虚拟化器，
   行高随内容自适应，偏移量随之重算。同时**移除卡片的固定高度**
   （`h-full` 会让卡片被估值绑死）。

**约定**：参与虚拟化的行，只要其内容高度**可能变化**，就必须：

```tsx
const virtualizer = useVirtualizer({
  count,
  getScrollElement,
  estimateSize: () => /* 仅作首次渲染估值 */,
  measureElement: (el) => el.getBoundingClientRect().height,   // ← 关键
});
```

并在每行上挂 `data-index` 与 `ref={virtualizer.measureElement}`。

另两条相关约束：

- 行内容**不要**设固定高度（`h-full` / 写死的 height），那会让测量失去意义；
- 网格项加 `min-w-0`：CSS Grid 的项默认 `min-width: auto`，
  内容（如长路径）能把单元撑宽并压到相邻列上。

### 6.12 PowerShell 5.1 会把原生程序 stderr 包装成 ErrorRecord

`pnpm` / `cargo` 输出到 stderr 的正常进度信息会被显示为 `NativeCommandError`。
**判断成败要看退出码**，不要被红色文字误导。

### 6.13 git 的七条实测行为

完整的分析与取舍见 [`GITHUB_SYNC.md`](GITHUB_SYNC.md) §5，此处只列结论：

1. **`git clone` 一个空仓库是成功的**（只给一句 warning，退出码 0）。因此不能
   用"clone 失败"判断远端为空——那是把网络/凭据/地址错误一并吞掉。要主动
   `git ls-remote` 看有没有 ref。
2. **`git ls-remote --exit-code` 对空仓库返回 2**，会把"可访问但为空"误判成
   "无法访问"。**绝不能加该选项**（空仓库是本项目最推荐的备份用法）。
3. **Git for Windows 的系统级配置默认 `core.autocrlf=true`**。同步工作区是
   派生目录，必须显式设成 `false`，否则镜像不是字节忠实的，"有没有改动"
   要靠规范化去猜。
4. **未出生的 HEAD 不是分支名**：`git init -b main` 后还没有提交时，
   `git rev-parse --abbrev-ref HEAD` 退出码 128，而不是回显 `main`。
5. **`core.quotePath` 默认把非 ASCII 路径转义成八进制**
   （`"skills/\344\270\255..."`）。这个字符串会进到界面和提交信息里，
   对中文应用是致命的。工作区要显式设 `core.quotePath false`。
6. **被 `.gitignore` 匹配的文件不出现在 `git status` 里**。用状态判断
   "有没有东西要传"会变成静默失败（界面报成功、实际没传）。
   同步必须先 `git add -f --no-all`、再读 `git diff --cached`。
7. **`git add -A` 会暂存删除**。任何"从工作区消失了"的追踪文件都会被提交成
   一次删除并推到用户的远端——而文件消失的原因可能根本不是用户想删。
   同步一律用 `--no-all`。

另有一条**设计前提**（不是 git 行为，但同样容易踩）：同步工作区之所以能在
分叉时直接 `reset --hard origin/<branch>`，是因为它的内容**全部可重建**
（远端内容可重新 clone，镜像内容可重新从中央库复制）。这依赖"推送前一定先
镜像"这个顺序，改动同步流程时必须一起复核。

还有一条与 git 无关但同类：**`Path::exists()` / `Path::is_dir()` 会跟随重解析点**，
对悬空 junction 返回 `false`。凡是要判断"这个目录项在不在"，用
`std::fs::symlink_metadata(...).is_ok()`。这与 §6.8 第 3 条是同一个坑的另一面。

---

## 7. 扩展点与预留位置

| 位置 | 预留内容 | 状态 |
| --- | --- | --- |
| `Sidebar` 导航项 | 「映射矩阵」入口已就位 | 入口已建，内容待填充 |
| `SettingsView` 分区卡 | 中央库位置已实现；Agent 目录 / 链接与迁移 / GitHub 同步三块待填 | 部分实现 |
| `DetailPanel` | 已具备元数据区、SKILL.md 内容区（Monaco 位） | 待填充内容 |
| `GlobalBanner` | 已接入实际检测（`LibraryHealthBanner`：中央库不可达时告警） | 待补恢复动作 |
| `platform/` 模块 | 已建立：`link.rs` + `volume.rs`，所有平台专有调用收敛于此 | 持续维护 |
| `Dialog` / `Switch` | Radix 基座已封装 | 待用于启停与向导 |
| `index/` 索引 | 已建立：4 张表（`skills` / `agents` / `links` / `meta`），其中 `agents`、`links` 尚未启用 | 待启用 |
| `library_stats` | 已实现；将扩展为按 Agent 维度的统计 | 待扩展 |

### 设计约束落点

| 约束 | 当前状态 |
| --- | --- |
| 中央库路径选择器 | ✅ **已实现**（`components/settings/CentralPathPicker.tsx`，设置页一级入口，含 11 项风险诊断） |
| 链接映射可视化 | 入口已建（实现于 `components/settings/LinkMappingView.tsx`） |
| 不内置 Skill 市场 | ✅ 无任何市场入口、无相关网络请求 |
| 不复制参考项目品牌文案 | ✅ 代码库无参考项目名称 / Logo |

---

## 8. 偏离与决策记录

| 日期 | 变更 | 原因 |
| --- | --- | --- |
| 2026-10-01 | 新增 `src/views/` 目录 | 主内容区需要独立的视图层文件，放在 `components/` 下语义不符 |
| 2026-10-01 | 新增 `src/lib/theme.ts`、`src/Root.tsx`、`src/components/StagePlaceholder.tsx` | 主题副作用、Provider 组合、占位视图各自独立成文件，避免入口文件职责过载 |
| 2026-10-01 | 尚未实现的视图使用 `StagePlaceholder` 显式标注 | 不渲染"看似可用实则无数据"的假界面 |
| 2026-10-01 | `bundle.targets` 由脚手架的 `"all"` 收窄为 `["nsis", "msi"]` | 只需要 NSIS + MSI 两种安装包 |
| 2026-10-01 | **配置文件不含 `ui` 段** | UI 偏好必须能在任何 IPC 之前**同步**读到（`index.html` 的防主题闪烁内联脚本），因此归属 localStorage。两处并存会造成失同步。`git` 段保留（无替代存储方案） |
| 2026-10-01 | 新增 `src-tauri/src/skill/` 模块 | frontmatter 解析被 `library` 与 `index` 共用，且是"读-写无损"这一硬约束的所在地，独立成模块更清晰 |
| 2026-10-01 | 新增 `src-tauri/src/platform/volume.rs` | 卷信息查询属平台专有能力，按架构约定必须收敛在 `platform/` 内 |
| 2026-10-01 | 配置的 IPC 载荷用 `ConfigView`（camelCase）转换，而非直接复用 `AppConfig`（snake_case） | 配置文件采用 snake_case 格式，而本项目 IPC 约定是 camelCase。用显式 DTO 同时守住两个契约，并配往返测试防漂移 |
| 2026-10-01 | 新增 `src/lib/format.ts`、`LibraryHealthBanner.tsx`、`components/settings/` 目录 | 界面需要；`settings/` 目录集中放置设置相关组件 |
