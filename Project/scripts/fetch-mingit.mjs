/**
 * 下载随应用分发的便携版 Git（MinGit）到 `src-tauri/mingit/`。
 *
 * # 为什么需要这个脚本
 *
 * 应用**自带**一份 git，用来让没装 git 的电脑也能用 GitHub 同步。它体积大
 * （约 93 MB、上千个文件），所以**不进源码仓库**——新克隆下来的仓库里没有它，
 * 而缺了它打包会直接失败（`resource path 'mingit' doesn't exist`）。
 *
 * 与 Tauri 自己的 NSIS/WiX 工具链是同一类东西：构建期依赖，按需拉取。
 *
 * 幂等：已经存在且看起来完整就跳过，不会重复下载。想强制重下用 `--force`。
 *
 * 用法：`pnpm fetch:mingit`
 */
import { createWriteStream } from "node:fs";
import { mkdir, rm, stat } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";

/** 与其它地方引用的是同一个版本；升级时改这里一处。 */
const MINGIT_VERSION = "2.56.0.windows.1";
const ARCHIVE = `MinGit-2.56.0-64-bit.zip`;
const URL = `https://github.com/git-for-windows/git/releases/download/v${MINGIT_VERSION}/${ARCHIVE}`;

const projectDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const targetDir = join(projectDir, "src-tauri", "mingit");

/** 判断一份 MinGit 是否已经就位（看两个最关键的入口在不在）。 */
async function isPresent() {
  const probes = [
    join(targetDir, "cmd", "git.exe"),
    join(targetDir, "LICENSE.txt"),
  ];
  for (const probe of probes) {
    try {
      await stat(probe);
    } catch {
      return false;
    }
  }
  return true;
}

async function main() {
  const force = process.argv.includes("--force");

  if (!force && (await isPresent())) {
    console.log(`[mingit] 已就位，跳过：${targetDir}`);
    return;
  }

  console.log(`[mingit] 下载 ${ARCHIVE}`);
  console.log(`[mingit]   ${URL}`);
  console.log(`[mingit] 约 38 MB，取决于网络，可能要等一会儿`);

  const zipPath = join(projectDir, "src-tauri", "mingit.zip");

  const response = await fetch(URL, { redirect: "follow" });
  if (!response.ok || !response.body) {
    throw new Error(
      `下载失败：HTTP ${response.status}。\n` +
        `本机直连 github.com 可能不通，开代理后重试；或者手动下载这个地址：\n  ${URL}`,
    );
  }
  await pipeline(Readable.fromWeb(response.body), createWriteStream(zipPath));

  // 解压用系统自带的 Expand-Archive，不引第三方依赖——这个项目只用 Windows。
  await rm(targetDir, { recursive: true, force: true });
  await mkdir(targetDir, { recursive: true });
  console.log(`[mingit] 解压到 ${targetDir}`);
  execFileSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      `Expand-Archive -LiteralPath '${zipPath}' -DestinationPath '${targetDir}' -Force`,
    ],
    { stdio: "inherit" },
  );

  await rm(zipPath, { force: true });

  if (!(await isPresent())) {
    throw new Error(`解压后仍未找到 cmd/git.exe，请检查 ${targetDir}`);
  }
  console.log("[mingit] 完成");
}

main().catch((error) => {
  console.error(`[mingit] ${error.message}`);
  process.exit(1);
});
