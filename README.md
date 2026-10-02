<div align="center">
  <img src="Project/src-tauri/icons/128x128.png" width="104" alt="BakaSkill" />
  <h1>BakaSkill</h1>
  <p><b>一份 Skill，多个 AI 编程 Agent 共用——只维护一次</b></p>
  <p>
    把 Skill 集中放在你自己选的目录，再用目录链接映射给各个 Agent。<br />
    不是复制，是同一份文件在多个地方同时可用。
  </p>
  <p>
    <a href="https://github.com/0407tong/BakaSkill/releases/latest"><b>⬇ 下载</b></a>
    &nbsp;·&nbsp;
    <a href="#快速开始">快速开始</a>
    &nbsp;·&nbsp;
    <a href="#常见问题">常见问题</a>
    &nbsp;·&nbsp;
    <a href="#参与开发">参与开发</a>
  </p>
  <p>
    <img alt="Windows 10/11" src="https://img.shields.io/badge/platform-Windows%2010%2F11-0078D6?logo=windows&logoColor=white" />
    <img alt="version 0.1.0" src="https://img.shields.io/badge/version-0.1.0-blue" />
    <img alt="license MIT" src="https://img.shields.io/badge/license-MIT-green" />
  </p>
</div>

<img src="Project/src/assets/default-background.png" alt="" />

---

## 这是什么

BakaSkill 是一个 **Windows 桌面工具**，用来统一管理散落在各个 AI 编程 Agent 里的 Skill。

你在本地挑一个目录当**中央库**，把 Skill 都放进去；BakaSkill 再用 **junction（目录链接）**
把它们映射到 Claude Code、Codex、Cursor 等 Agent 各自约定的技能目录。
对 Agent 来说文件就在老地方，对你来说只有一份。

> **它只管"整理"，不管"内容"。** BakaSkill 不内置 Skill 市场、不提供 Skill 下载，
> 也不替你写 Skill——你的 Skill 从哪来、写什么，它一概不关心。

**适合你，如果：**

- 你在用两个以上 AI 编程 Agent，同名的 Skill 得在每个 Agent 目录里各放一份
- 你的 C 盘很紧张，而 Skill 目录默认都落在 C 盘用户目录下
- 你改了一份 Skill，却不确定别的 Agent 那边是不是还停在旧版本

## 它解决什么问题

AI 编程 Agent 这两年快速分化，每家都有自己的技能目录约定，且默认都落在 C 盘用户目录下。

| 问题 | 表现 |
| --- | --- |
| **重复存储** | 同一个 Skill 要在 Claude Code、Codex、Cursor 各放一份。改一处，其余几份就过期了 |
| **C 盘被挤** | Skill 常带脚本、模板、参考文档，C 盘越来越满，而它往往是最紧张的那个 |
| **状态不透明** | 看不出某个"像文件夹"的条目是**实体副本**还是**链接**，于是"我改了它怎么没生效"反复出现 |

## 它是怎么做的

```
        中央库（位置由你指定）
        D:\BakaSkillLibrary\skills\
        ├─ pdf-tools\SKILL.md
        └─ code-review\SKILL.md
                 │
        ┌────────┼────────┐   junction（目录链接）
        ▼        ▼        ▼
  ~\.claude\  ~\.codex\  ~\.cursor\
   skills\     skills\    skills\
```

Agent 目录里放的不是复制品，而是 **junction（NTFS 目录链接）**：

- **不需要管理员权限**，全程不弹 UAC
- 对 Agent **完全透明**——它们根本感知不到这是链接
- 在任意一处修改，**其余各处立刻同步**（物理上就是同一份）
- 删掉某个 Agent 里的条目，**只是删掉入口**，中央库分毫不动

## 功能

**中央库与链接**
- 中央库位置完全可配置（含盘符），选择时当场给出体检报告：文件系统是否支持链接、
  是否云同步目录、是否网络路径、剩余空间、路径长度风险
- 一键启用 / 禁用（单条、批量、按 Agent）
- **映射矩阵**：一张 Skill × Agent 的表，逐格显示真实链接状态与两侧绝对路径
- 断链修复、冲突处理、中央库迁移向导、把 Agent 目录里的 Skill「纳入中央库」

**多 Agent 支持**
- 自动检测安装状态与技能目录，分**三态**：未安装 / **已安装但技能目录不存在** / 已就绪
  （有些 Agent 的技能目录是**按需创建**的，用布尔判断会误判成"没装"）
- 一键为这类 Agent 创建目录；路径探测不到的由用户手填；支持注册表之外的自定义 Agent

**搜索、分类、导入导出**
- 全文搜索（索引 SKILL.md 正文）+ 语法糖 `tag:` / `agent:` / `is:enabled` / `is:external` / `is:dangling`
- 标签云、重命名 / 合并 / 删除，改动**写回 SKILL.md 的 frontmatter**
- 导入：文件夹 / ZIP / Git 仓库地址 / 单个 SKILL.md，带预览与逐项冲突处理
- **ZIP 路径穿越防护**：含越界条目的压缩包整包拒绝，一个文件都不解压
- 导出为 ZIP（含 manifest）或纯目录结构
- **拖拽安装**：把文件或文件夹拖进「我的 Skills」即可导入

**GitHub 同步**
- 备份到**你自己已有的**仓库（仓库里原本有别的文件也没关系，不会被删除也不会被提交）
- 同步 = 只增改不删除；取回 = 只补齐不覆盖本地
- Token 存放在 **Windows 凭据管理器**，不落配置文件、不进日志

**其它**
- 卸载 Skill：先摘掉链接，再把目录**送进 Windows 回收站**（可还原）
- 首次运行引导（三步）；亮 / 暗主题；自定义背景图；默认隐藏 Agent 自带的 Skill

---

## 快速开始

首次启动会走三步引导：

1. **选择中央库位置** —— 挑一个空间宽裕的盘，比如 `D:\BakaSkillLibrary`。
   选完会立刻给出体检结论（是否支持链接、是否云同步目录、剩余空间等）
2. **连接 Agent** —— BakaSkill 自动扫描本机的 Agent 与技能目录，
   列出的每一条都能一键启用；没扫到的可以手动指定路径
3. **启用第一个 Skill** —— 选中一个 Skill，勾上要让它生效的 Agent 即可

之后日常使用就两件事：在「映射矩阵」里看哪一格还没连上，以及用搜索框找 Skill。

## 安装

**系统要求**：Windows 10 / 11（64 位）。链接功能依赖 NTFS junction，**目前仅支持 Windows**。

**下载**：到 [Releases](https://github.com/0407tong/BakaSkill/releases/latest) 页面取最新版。

| 文件 | 说明 |
| --- | --- |
| `BakaSkill_<版本>_x64-setup.exe` | **推荐**。NSIS 安装程序，双击即装 |
| `BakaSkill_<版本>_x64_en-US.msi` | MSI 安装包。文件名里的 `en-US` 指**安装界面**语言，与应用界面无关 |

> **安装时 Windows 会拦一道**：「Windows 已保护你的电脑 —— 未知发布者」。
> 本项目没有代码签名证书，这是未签名软件的正常表现，**不是安装包损坏**。
> 点「更多信息」→「仍要运行」即可继续。
> 部分企业策略下 MSI 的提示可能无法绕过，所以推荐用 NSIS 版本。

**卸载是安全的**：卸载 BakaSkill **不会**删除中央库、不会删除配置，
也不会删除同步工作区——它们都在安装目录之外，安装器结构上碰不到。
唯一需要手动清理的是 Windows 凭据管理器里的 GitHub Token
（凭据管理器 → Windows 凭据 → 找到 `com.bakaskill.app` → 删除）。

<details>
<summary>数据都放在哪</summary>

| 内容 | 位置 |
| --- | --- |
| 中央库（你的 Skill） | **你选择的位置**，如 `D:\BakaSkillLibrary` |
| 配置 | `%APPDATA%\BakaSkill\config.json` |
| 同步工作区（仓库的本地副本） | `%LOCALAPPDATA%\BakaSkill\sync\` |
| GitHub Token | Windows 凭据管理器（服务名 `com.bakaskill.app`） |

同步工作区是可重建的派生目录，删掉不会丢任何原始数据。
</details>

## 常见问题

**Q：BakaSkill 会乱动我现有的 Skill 文件吗？**

不会。你不点，它就不动。启用一个 Skill 只是在 Agent 目录里**建一个链接**；
禁用则是把链接摘掉。只有你主动用「纳入中央库」时，它才会把 Skill 移进中央库、
并在原位置留下链接——这是你亲手触发的操作，且界面会先说明。

**Q：删掉某个 Agent 里的 Skill，中央库里的会跟着没吗？**

不会。删掉的只是那个 Agent 目录里的**入口**，中央库分毫不动。
反过来，在中央库里删掉 Skill 时，会先摘掉所有链接，再把目录**送进回收站**，可以还原。

**Q：需要管理员权限吗？会不会一直弹 UAC？**

不需要，也不会。Windows 上普通的目录链接（symlink）需要管理员权限，
但本项目用的 **junction 不需要**，所以全程静默。

**Q：它会把我的 Skill 上传到网上吗？**

不会。BakaSkill 是纯本地工具，没有任何遥测或云端服务。
只有你在「GitHub 同步」里主动点同步时，内容才会推送到**你自己指定的**仓库。

**Q：我的 GitHub Token 安全吗？**

Token 存放在 **Windows 凭据管理器**里，不写进配置文件、不写进日志。
登录支持两种方式：GitHub 账号浏览器授权，或手动填 Personal Access Token。
同步模型是"只增改不删除"，**取回时也只补齐、不覆盖**你本地已有的 Skill。

**Q：为什么我的 Agent 没被检测到？**

有些 Agent 的技能目录是**按需创建**的——Agent 装了，目录却要等你添加第一个 Skill 才出现。
所以 BakaSkill 把检测分成三态，这种情况会显示「已安装，但尚无技能目录」并允许你一键创建。
如果连 Agent 都没扫到，可以在设置里手动指定技能目录，也能添加自定义 Agent。

**Q：我在中央库改了文件，需要再去各个 Agent 目录里重新同步吗？**

不需要。链接的两头是**同一份文件**，改一处就是改全部，没有同步这一步。

**Q：中文搜索为什么两字查询比较慢？**

索引对中文采用 trigram 子串匹配，两字查询会退化为扫描。
**结果是正确的**，只是排序语义不如专业中文分词。

## 已知限制

| 限制 | 说明 |
| --- | --- |
| **仅支持 Windows** | 链接功能依赖 NTFS junction。平台层已为其它系统留了占位实现，但**尚未真实编译验证** |
| **安装包未签名** | 没有代码签名证书，Windows SmartScreen 会提示"未知发布者"。点「更多信息 → 仍要运行」即可 |
| **中文搜索用 trigram** | 两字中文查询会退化为扫描（结果正确，排序语义弱于专用中文分词） |
| 卸载靠回收站 | 送不进回收站时会回退到库内 `removed/` 目录并如实告知；但"系统悄悄真删"这种情况程序无从判定 |
| 删除 Skill 后仓库副本保留 | 同步"绝不删除"的必然代价，需手动在 GitHub 上清理 |

## 参与开发

欢迎 Issue 与 PR。用户向的问题、界面建议、新 Agent 的路径实测报告尤其有价值
（`Project/docs/AGENT_PATHS_MEASURED.md` 记录了当前各 Agent 技能目录的实测结论，
其中 Cursor、Windsurf、Trae 仍待有环境的同学补充）。

技术栈：**Tauri 2**（Rust 后端）+ **React 19** + **TypeScript** + **Tailwind CSS v4** +
**Monaco Editor**，SQLite（rusqlite bundled + FTS5）做可重建的派生索引。

```bash
cd Project
pnpm install
pnpm tauri:dev     # 开发模式，弹出窗口
pnpm tauri:build   # 打包安装程序（需要 Node ≥ 20、pnpm ≥ 9、Rust stable ≥ 1.80 + MSVC 工具链）
```

代码层面的说明见 [`Project/README.md`](Project/README.md)，
架构、链接机制、同步模型、打包分发的细节见 [`Project/docs/`](Project/docs/)。

## 许可证

[MIT](LICENSE) © 2026 0407tong
