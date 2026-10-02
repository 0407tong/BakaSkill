/**
 * 打包前的守卫：确认 `src-tauri/mingit/` 就位。
 *
 * # 为什么需要它
 *
 * 缺了这份便携版 Git，Tauri 报的是一句
 * `resource path 'mingit' doesn't exist`——它说的是"资源路径不存在"，
 * 但没说"这是什么、从哪来、怎么补"。新克隆的仓库必然缺（它被 .gitignore
 * 排除），所以每个第一次构建的人都会撞上，包括未来的自己。
 *
 * 这里把它换成一句能照着做的话。退出码非 0，构建不会继续。
 */
import { stat } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const projectDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const gitExe = join(projectDir, "src-tauri", "mingit", "cmd", "git.exe");

try {
  await stat(gitExe);
} catch {
  console.error(
    [
      "",
      "✗ 缺少便携版 Git（src-tauri/mingit/）——应用随包分发的那份 git。",
      "",
      "  它体积大（约 93 MB、上千个文件），不进源码仓库，所以新克隆下来没有。",
      "  缺了它打出来的包会让所有用户都看到「没找到可用的 git」。",
      "",
      "  补上它：",
      "      pnpm fetch:mingit",
      "",
      "  （或者手动下载 MinGit 解压到 src-tauri/mingit/，",
      "    地址见 docs/DISTRIBUTION.md「便携版 Git 从哪来」）",
      "",
    ].join("\n"),
  );
  process.exit(1);
}
