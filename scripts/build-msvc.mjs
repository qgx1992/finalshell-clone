// MSVC 工具链打包脚本。
//
// 做三件事:
// 1. 注入 Visual Studio Build Tools 环境(link.exe 等),普通终端也能直接打包
// 2. 固定 stable-x86_64-pc-windows-msvc 工具链
// 3. 按当前项目实际路径重新生成 NSIS 钩子,避免仓库里写死绝对路径
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { resolveMsvcEnv } from "./msvc-env.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const targetDir = path.join(root, "src-tauri", "target-msvc");

// 项目路径含空格时,GNU 工具链的 windres 会解析失败,所以固定 MSVC。
const env = {
  ...resolveMsvcEnv(),
  RUSTUP_TOOLCHAIN: "stable-x86_64-pc-windows-msvc",
};
if (!env.CARGO_TARGET_DIR) {
  env.CARGO_TARGET_DIR = targetDir;
}

// 生成 NSIS 钩子。
//
// MSVC 构建静态链接 WebView2,不需要侧载 DLL;GNU 构建才需要,而 tauri-bundler
// 不提供项目路径变量,只能写绝对路径。这里在打包前按实际情况重写,既能适配
// 任意项目位置,也不会把某个人的路径提交进仓库。
const hookPath = path.join(root, "src-tauri", "webview2-hook.nsh");
// 只看本次构建产物目录,不要回退到旧的 GNU target/:
// MSVC 静态链接 WebView2,若误捡到 GNU 目录里的 DLL 反而会把它打包进去。
const dll = path.join(env.CARGO_TARGET_DIR, "release", "WebView2Loader.dll");
const dllExists = fs.existsSync(dll);

const hookLines = [
  "; 本文件由 scripts/build-msvc.mjs 在打包前自动生成,请勿手工编辑。",
  "; MSVC 构建静态链接 WebView2,通常无需侧载 DLL;",
  "; 仅当 GNU 构建产物中存在 WebView2Loader.dll 时才一并打包。",
  "!macro NSIS_HOOK_PREINSTALL",
];
if (dllExists) {
  hookLines.push(`  File "/oname=WebView2Loader.dll" "${dll}"`);
} else {
  hookLines.push("  ; 未找到 WebView2Loader.dll,跳过(MSVC 静态链接场景属正常)");
}
hookLines.push("!macroend", "");
fs.writeFileSync(hookPath, hookLines.join("\r\n"), "utf8");

console.log(`[build-msvc] 项目路径: ${root}`);
console.log(`[build-msvc] 目标目录: ${env.CARGO_TARGET_DIR}`);
console.log(`[build-msvc] WebView2Loader: ${dllExists ? dll : "不需要(静态链接)"}`);

// 其余参数透传给 tauri build(如 --bundles nsis / --no-bundle)
const res = spawnSync("npx", ["tauri", "build", ...process.argv.slice(2)], {
  cwd: root,
  env,
  stdio: "inherit",
  shell: process.platform === "win32",
});

process.exit(res.status ?? 1);
