// MSVC 环境下的 cargo 包装脚本。
//
// 用法: node scripts/cargo-msvc.mjs <cargo 参数...>
// 例:   node scripts/cargo-msvc.mjs test
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { resolveMsvcEnv } from "./msvc-env.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const crateDir = path.join(root, "src-tauri");
const cargoBin = path.join(process.env.USERPROFILE ?? "", ".cargo", "bin", "cargo.exe");

if (!fs.existsSync(cargoBin)) {
  console.error(`找不到 cargo: ${cargoBin},请先安装 Rust。`);
  process.exit(1);
}

const res = spawnSync(cargoBin, process.argv.slice(2), {
  cwd: crateDir,
  env: resolveMsvcEnv(),
  stdio: "inherit",
  windowsHide: true,
});
process.exit(res.status ?? 1);
