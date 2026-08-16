// MSVC 工具链打包脚本
// 原因:项目路径含空格(如 "A-my project")时,GNU 工具链(toolchain-gnu)的 windres
// 会解析路径失败。本脚本固定使用 MSVC 工具链并指向独立的 target-msvc 目录,避免踩坑。
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const env = {
  ...process.env,
  RUSTUP_TOOLCHAIN: "stable-x86_64-pc-windows-msvc",
};

if (!env.CARGO_TARGET_DIR) {
  env.CARGO_TARGET_DIR = path.join(root, "src-tauri", "target-msvc");
}

// 其余参数透传给 tauri build(如 --bundles nsis 等)
const res = spawnSync("npx", ["tauri", "build", ...process.argv.slice(2)], {
  cwd: root,
  env,
  stdio: "inherit",
  shell: process.platform === "win32",
});

process.exit(res.status ?? 1);
