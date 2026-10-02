# 打包与分发

本文记录 BakaSkill 的打包方式、产物位置、卸载行为与数据落点。
**"卸载会不会删掉我的 Skill"这类问题以本文为准**，不要凭经验推断。

---

## 1. 打包

```bash
pnpm tauri:build
```

产物：

| 格式 | 路径 |
| --- | --- |
| NSIS 安装包（`.exe`） | `src-tauri/target/release/bundle/nsis/` |
| MSI 安装包（`.msi`） | `src-tauri/target/release/bundle/msi/` |

配置见 `src-tauri/tauri.conf.json` 的 `bundle.targets`（当前为 `["nsis", "msi"]`）。

### 首次打包需要联网

NSIS 与 WiX 工具链**不随 Tauri CLI 分发**，而是在首次打包时下载到
`%LOCALAPPDATA%\tauri`。这个目录在全新机器上是空的，因此**第一次
`tauri:build` 必须能访问 `github.com`**，否则会在打包阶段失败。

这一次性下载完成后，后续打包可以离线进行（除非升级 Tauri 版本导致工具链变更）。

要下载的三件东西（取自 `tauri-cli-v2.12.0` 的 `tauri-bundler` 源码）：

| 文件 | 来源 | 落到 |
| --- | --- | --- |
| `nsis-3.11.zip` | `github.com/tauri-apps/binary-releases/releases/download/nsis-3.11/` | `%LOCALAPPDATA%\tauri\NSIS\`（解压后应直接见 `makensis.exe`） |
| `nsis_tauri_utils.dll` | `github.com/tauri-apps/nsis-tauri-utils/releases/download/nsis_tauri_utils-v0.5.3/` | `%LOCALAPPDATA%\tauri\NSIS\Plugins\x86-unicode\additional\` |
| `wix314-binaries.zip` | `github.com/wixtoolset/wix3/releases/download/wix3141rtm/` | `%LOCALAPPDATA%\tauri\WixTools314\`（解压后应直接见 `candle.exe`） |

下载后会做哈希校验（`nsis_tauri_utils.dll` 的 SHA1 为
`75197FEE3C6A814FE035788D1C34EAD39349B860`），**手动放置时必须是原件**。

> **实测踩到过**：本机 `github.com:443` 曾不可达（21 秒超时），
> 报错是 `failed to bundle project: 'timeout: global'`。
> 值得注意的干扰现象是——同一时刻 **`api.github.com` 却是通的**，
> 所以很容易误判成"网络没问题，是 Tauri 的 bug"。
> Rust 代码的编译此时**已经成功**（`target\release\bakaskill.exe` 已产出），
> 只有打包这一步卡住。开启代理后重跑即通过。

### MSI 的文件名带 `en-US`

`BakaSkill_0.1.0_x64_en-US.msi` 里的 `en-US` 是 **WiX 的默认语言**，不是笔误。
它指的是**安装界面**的语言，与应用界面语言无关（应用自身是中文的）。
若要改变安装界面语言，需要配置 `bundle.windows.wix.language` 并准备对应的本地化文件。

### 版本号

版本号有三处，必须保持一致：

| 文件 | 字段 |
| --- | --- |
| `package.json` | `version` |
| `src-tauri/Cargo.toml` | `[package] version` |
| `src-tauri/tauri.conf.json` | `version` |

当前三处均为 `0.1.0`。

### 便携版 Git 从哪来（打包的必备前提）

应用**随包带一份便携版 Git（MinGit）**，同步功能优先用它，找不到才回退到系统
PATH 上的 `git`。原因很直接：同步建立在 git 之上，但"用户装了 git"不是能替他
保证的事——一个只想整理 Skill 的人不该为了备份去装一套 git。

它放在 `src-tauri/mingit/`，**被 `.gitignore` 排除**（93 MB、约 1000 个文件），
与 Tauri 自己的 NSIS/WiX 工具链同理：属于构建期依赖，不随源码仓库分发。
**源码仓库里没有它，打包前必须自己放一份**，否则打出来的包缺少自带 git，
所有用户都会看到「没找到可用的 git」。

获取方式（版本可换，与 `git --version` 无关，应用不依赖特定版本）：

```bash
curl -L -o mingit.zip \
  https://github.com/git-for-windows/git/releases/download/v2.56.0.windows.1/MinGit-2.56.0-64-bit.zip
unzip -q mingit.zip -d src-tauri/mingit
```

三个需要知道的事实：

1. **MinGit 自带 Git Credential Manager**（`ucrt64/bin/git-credential-manager.exe`），
   所以"用 GitHub 账号浏览器授权登录"这条路在没装 git 的机器上照样能走。
2. 但那份 GCM **不含 .NET 运行时**，依赖系统安装的 .NET 桌面运行时。
   机器上没有 .NET 时，浏览器登录会失败，**手动填 Token 仍然可用**。
3. Git 是 **GPLv2**。`mingit/LICENSE.txt` 随包分发，打包时不要把它排除掉。

留尾：安装包因此从约 8 MB 涨到约 45 MB。**免安装版不再是单个文件**——
`bakaskill.exe` 需要与 `mingit/` 目录放在一起（`exe` 旁边或资源目录下都认）。

---

## 2. 安装包未签名

本项目**没有代码签名证书**，因此安装包是未签名的。用户双击安装时，
Windows SmartScreen 会拦一道：

> Windows 已保护你的电脑 —— 未知发布者

绕过方式：点「更多信息」→「仍要运行」。

这不是"安装包损坏"，是未签名软件的正常表现。分发时**应当主动告诉用户这一步**，
否则多数人会以为文件被篡改而放弃。

MSI 安装包同样未签名，且部分环境下 MSI 的 SmartScreen 提示无法用上述方式绕过
（企业策略可能直接禁止），**推荐优先分发 NSIS 版本**。

---

## 3. 卸载行为：不会删除你的数据

**结论：卸载 BakaSkill 不会删除中央库、不会删除配置，也不会删除同步工作区。**
用户重新安装后，一切照旧。

### 3.1 为什么中央库一定不会被删

中央库位于**用户自己选的绝对路径**（例如 `D:\BakaSkillLibrary`），
在安装目录之外。安装器与卸载器只认得安装目录、快捷方式与注册表项，
**结构上不可能碰到它**。

### 3.2 卸载界面上那个勾选框对本应用无效

Tauri 的 NSIS 卸载器会在确认页加一个 **「删除应用数据」勾选框**
（默认不勾）。勾上它会执行：

```nsis
RmDir /r "$APPDATA\${BUNDLEID}"
RmDir /r "$LOCALAPPDATA\${BUNDLEID}"
```

其中 `${BUNDLEID}` 是 `tauri.conf.json` 的 `identifier`，即 **`com.bakaskill.app`**。

而 BakaSkill 的数据**不在那里**——它放在字面量 `BakaSkill` 目录下
（见 §4）。于是：

| 卸载器要删的 | 本应用实际数据所在 | 结果 |
| --- | --- | --- |
| `%APPDATA%\com.bakaskill.app` | `%APPDATA%\BakaSkill\` | 删不到 |
| `%LOCALAPPDATA%\com.bakaskill.app` | `%LOCALAPPDATA%\BakaSkill\` | 删不到 |

也就是说，**即使勾上那个框，BakaSkill 的数据也不会被删除**。
这一点必须写清楚，否则会出现两种误解：用户以为勾了就干净了（其实没有），
或者用户不敢勾（其实勾不勾都一样）。

> **依据**（逐条取证，非推断）：
>
> 1. 模板：`crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi`，
>    取自 tag **`tauri-cli-v2.12.0`**（与本机 `tauri-cli 2.12.0` 对应）。
>    关键行：`!define BUNDLEID "{{bundle_id}}"`、
>    `RmDir /r "$APPDATA\${BUNDLEID}"`、`RmDir /r "$LOCALAPPDATA\${BUNDLEID}"`，
>    且整块被 `${If} $DeleteAppDataCheckboxState = 1` 包住。
> 2. `{{bundle_id}}` 的取值来自 `src-tauri/tauri.conf.json` 的 `identifier`
>    = **`com.bakaskill.app`**。
> 3. 本应用数据的实际落点来自源码：`src/config/mod.rs` 的
>    `CONFIG_DIR_NAME = "BakaSkill"`、`src/git/mod.rs` 的 `sync_workspace()`
>    （`LOCALAPPDATA` + `BakaSkill`）。
>
> Tauri 2.12 的 `NsisConfig` **没有** `deleteAppDataOnUninstall` 这类配置项
> （已核对 `config.schema.json` 的 `NsisConfig` 属性表）——行为由上述勾选框
> 决定，**不可通过配置改变**。

### 3.3 一处需要手动清理：凭据管理器里的 Token

卸载器**不会**清除 Windows 凭据管理器中的条目。如果你曾保存过 GitHub Token，
它仍然留在凭据管理器里（服务名 `com.bakaskill.app`，账户名 `github-token`）。

需要彻底清理时：

> 控制面板 → 用户账户 → 凭据管理器 → Windows 凭据 →
> 找到 `com.bakaskill.app` → 删除

---

## 4. 数据落点

| 内容 | 位置 |
| --- | --- |
| 中央库（你的 Skill） | **用户选择的位置**，如 `D:\BakaSkillLibrary` |
| 配置 | `%APPDATA%\BakaSkill\config.json` |
| 同步工作区（仓库的本地副本） | `%LOCALAPPDATA%\BakaSkill\sync\<仓库名>-<地址摘要>\` |
| GitHub Token | Windows 凭据管理器（服务 `com.bakaskill.app`） |

**注意配置目录名的这个细节**：它是字面量 `BakaSkill`，不是 Tauri 默认的
`<identifier>`（`com.bakaskill.app`）。这一点已在 `src/config/mod.rs`
的 `CONFIG_DIR_NAME` 中固定下来。

同步工作区是**可重建的派生目录**：里面是某个仓库的本地副本，内容随时可以从
「远端仓库 ∪ 中央库」重新构造出来。删除它不会丢失任何原始数据。

---

## 5. 打包前检查清单

- [ ] 三处版本号一致（§1）
- [ ] `CHANGELOG.md` 已补上本版本的条目
- [ ] `cargo clippy --all-targets -- -D warnings` 退出码 0
- [ ] `cargo fmt --check` 退出码 0
- [ ] `cargo test` 全绿
- [ ] `pnpm typecheck` / `pnpm lint` / `pnpm build` 均退出码 0
- [ ] 首次打包的机器可访问网络（§1）
- [ ] 已确认卸载行为与本文一致（§3）
