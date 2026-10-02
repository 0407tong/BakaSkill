# 链接机制说明（junction）

本文档说明 BakaSkill 如何使用 Windows 目录 junction，以及为什么必须这样做。
**故障排查章节记录了三个只有实际踩过才会知道的陷阱**，改动链接相关代码前请先读完。

---

## 1. 为什么是 junction

| 维度 | 目录 junction（`mklink /J`） | 目录符号链接（`mklink /D`） | 硬链接 | 复制副本 |
| --- | --- | --- | --- | --- |
| 需要管理员权限 | **否** | 是（或开启开发者模式） | 否 | 否 |
| 可否跨卷 | **可以** | 可以 | 否 | 可以 |
| 适用对象 | 仅目录 | 文件与目录 | 仅文件 | 任意 |
| 目标不存在时 | 链接悬空（仍可创建） | 链接悬空 | — | — |
| 网络路径 | 不可靠 / 受限 | 支持有限 | 否 | 可以 |
| 删除行为 | 仅删链接本体 | 仅删链接本体 | 删一个名字 | **删数据** |
| 程序可见性 | 对绝大多数程序透明 | 透明 | 透明 | 透明 |

**结论**：junction 不需要提权，满足"开箱即用、不弹 UAC"的产品要求，因此
本项目**一律使用 junction**，不使用符号链接。两种链接不混用。

## 2. 数据流

```
中央库（用户自选位置）                    Agent 技能目录
D:\BakaSkillLibrary\skills\pdf-tools  ◄──  C:\Users\me\.claude\skills\pdf-tools
                                          （junction，指向左边）

两份路径指向**同一份数据**：
  - 通过链接改文件 == 改中央库里的文件
  - 删除链接 == 只摘掉链接，中央库内容不受影响
```

Agent 侧完全不需要适配：对任何程序来说，junction 与真实目录没有区别。

## 3. 安全红线

> **递归删除一个 junction 会穿透到它指向的真实目录，把中央库里的内容一起删掉。**

具体地说，对 junction 调用 `remove_dir_all` 会删除**目标目录里的文件**，
而不只是链接本身。对 BakaSkill 而言，这等于"用户点了一下禁用，Skill 没了"。

因此项目规定：

1. 删除链接**只能**调用 `platform::link::delete_junction`。
2. `delete_junction` 在动手前必须确认目标**确实是** junction；否则返回
   `AppError::NotAJunction` 并拒绝执行。
3. 代码库中**禁止**对任何用户目录调用 `remove_dir_all` / `remove_dir`，
   唯一例外是 `platform::link` 内部对已确认摘除重解析点的**空目录**调用
   `remove_dir`（见 §4.2）。

这些约束由 `src-tauri/tests/link_safety.rs` 中的回归测试守护。
**这些测试失败时不得通过修改测试来"修复"** —— 那说明实现里出现了数据丢失级的缺陷。

---

## 4. 故障排查：三个实测陷阱

以下每一条都是本项目在真实文件系统上验证出来的，不是推测。

### 4.1 Windows 上 junction 也满足 `FileType::is_symlink()`

junction 属于 name-surrogate 重解析点，Rust 的 `std::fs::FileType::is_symlink()`
对 **junction 同样返回 `true`**。

因此判断顺序**不能**是先 symlink 后 junction——那样 junction 会被误判成
`SymlinkFile`，进而导致 `delete_junction` 对每个真实链接都返回
`NotAJunction` 而拒绝执行，表现为**"禁用按钮点了没反应"**。

正确顺序（见 `platform/link.rs::classify`）：

```rust
if is_junction(path) { return Ok(LinkKind::Junction); }   // 必须先判这个
if metadata.file_type().is_symlink() { /* 符号链接 */ }
```

### 4.2 `junction::delete` 会留下一个空目录

`junction` crate 的 `delete()` 底层使用 `FSCTL_DELETE_REPARSE_POINT`：
它只是把**重解析点**从目录项上摘掉，**目录项本身保留**，于是原地留下一个
真实的空目录。

若不处理，用户会看到：

- Agent 目录里残留一个空文件夹；
- 下次对该 Skill 点"启用"时，因为"位置已被占用"而失败。

因此 `platform/link.rs` 在摘除重解析点之后，会再调用 `std::fs::remove_dir`
删掉这个空壳。这里用 `remove_dir` 而非 `remove_dir_all` 是刻意的——
它**只能删除空目录**，遇到非空目录会直接报错，因此即便前面的判断出错，
也不可能删除任何用户数据。

### 4.3 `junction::exists()` 识别不了断链

`junction` crate 的 `exists()` 实现开头是：

```rust
if !junction.exists() { return Ok(false); }   // ← Path::exists() 会跟随重解析点
```

`Path::exists()` 跟随链接，因此当**目标不存在时**（中央库被移动、盘符变化、
移动硬盘未接入），`exists()` 返回 `false` —— 断链的链接会被判定为"不是 junction"，
**断链检测直接失效**。

因此本项目不使用 `junction::exists()`，而是自己读**重解析标签**
（`WIN32_FIND_DATAW.dwReserved0 == IO_REPARSE_TAG_MOUNT_POINT`）。
该判定只描述目录项自身、不跟随目标，对悬空链接同样有效。

### 4.4 其他已知问题

| 现象 | 原因 | 处理 |
| --- | --- | --- |
| 创建后链接不可用 | 杀毒软件拦截新建 reparse point | `create_junction` 创建后会**回读校验**，不通过即报错，不会假装成功 |
| `FileInUse` | Agent 正在运行并锁定其技能目录 | 提示"请关闭 XXX 后重试" |
| 链接随机失效 | 中央库放在网络盘 | 路径校验阶段即拒绝（`network_volume`） |
| 同步后链接异常 | 中央库位于 OneDrive 等云同步目录 | 路径校验阶段给出警告（`cloud_sync`） |
| 全部链接失效 | 中央库在可移动盘，盘符变化 | 路径校验阶段警告（`removable_volume`）；失效后可迁移中央库并重写链接 |

---

## 5. 路径校验的判定项

`library_validate` 会返回一份诊断报告，前端据此决定是否放行"初始化"按钮。

| 检查项 | 结论代码 | 严重级别 |
| --- | --- | --- |
| 路径指向文件而非目录 | `not_a_directory` | error |
| 上级目录不存在 | `parent_missing` | error |
| 网络位置 | `network_volume` | error |
| 卷类型无法确认 | `unsupported_volume` | error |
| 文件系统非 NTFS / ReFS | `unsupported_filesystem` | error |
| 位置不可写 | `not_writable` | error |
| 可移动磁盘 | `removable_volume` | warning |
| 云同步目录 | `cloud_sync` | warning |
| 剩余空间偏低 | `low_space` | warning |
| 路径过长 | `long_path` | warning |
| 目录尚不存在（将创建） | `will_create` | info |

**云同步检测是启发式判断**：依据 `%OneDrive%` 等环境变量与常见目录名，
用户可以把同步目录重定向到任意位置，因此只作为警告、不阻断。

## 6. 相关代码

| 位置 | 职责 |
| --- | --- |
| `src-tauri/src/platform/link.rs` | **唯一**允许操作链接的模块 |
| `src-tauri/src/platform/volume.rs` | 卷类型 / 文件系统 / 可用空间查询 |
| `src-tauri/src/library/mod.rs` | 路径诊断、目录骨架初始化、统计 |
| `src-tauri/tests/link_safety.rs` | 数据安全回归测试（14 项） |
