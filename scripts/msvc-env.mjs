// 解析 Visual Studio Build Tools 的编译环境变量。
//
// rustup 的 MSVC target 需要 link.exe / INCLUDE / LIB,而这些只在
// "Developer Command Prompt" 里才有。这里执行 vcvars64.bat 并解析它导出的
// 环境变量,让普通的 PowerShell / npm script 也能直接编译。
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const VC_VARS_CANDIDATES = [
  "C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\BuildTools\\VC\\Auxiliary\\Build\\vcvars64.bat",
  "C:\\Program Files\\Microsoft Visual Studio\\2022\\BuildTools\\VC\\Auxiliary\\Build\\vcvars64.bat",
  "C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\Community\\VC\\Auxiliary\\Build\\vcvars64.bat",
  "C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Auxiliary\\Build\\vcvars64.bat",
  "C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\Professional\\VC\\Auxiliary\\Build\\vcvars64.bat",
  "C:\\Program Files\\Microsoft Visual Studio\\2022\\Enterprise\\VC\\Auxiliary\\Build\\vcvars64.bat",
  "C:\\Program Files (x86)\\Microsoft Visual Studio\\18\\BuildTools\\VC\\Auxiliary\\Build\\vcvars64.bat",
  "C:\\Program Files\\Microsoft Visual Studio\\18\\BuildTools\\VC\\Auxiliary\\Build\\vcvars64.bat",
];

/** 找到 vcvars64.bat 的绝对路径,找不到返回 null */
export function findVcVars() {
  return VC_VARS_CANDIDATES.find((p) => fs.existsSync(p)) ?? null;
}

/** 返回注入了 MSVC 环境变量的 process.env 副本 */
export function resolveMsvcEnv() {
  const vcVars = findVcVars();
  if (!vcVars) {
    console.error("找不到 vcvars64.bat,请安装 Visual Studio Build Tools 的 C++ 组件。");
    process.exit(1);
  }
  const probe = spawnSync(`call "${vcVars}" >nul && set`, {
    shell: true,
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
    windowsHide: true,
  });
  if (probe.status !== 0) {
    console.error("vcvars64.bat 执行失败");
    process.exit(1);
  }
  const env = { ...process.env };
  for (const line of (probe.stdout || "").split(/\r?\n/)) {
    const idx = line.indexOf("=");
    if (idx <= 0) continue;
    env[line.slice(0, idx)] = line.slice(idx + 1);
  }

  // rustup 安装后 ~/.cargo/bin 可能尚未进入当前会话的 PATH(需要重开终端),
  // 这里显式补上,保证 cargo / rustc 一定能被找到。
  const cargoBin = path.join(process.env.USERPROFILE ?? "", ".cargo", "bin");
  const pathKey = Object.keys(env).find((k) => k.toLowerCase() === "path") ?? "Path";
  const current = env[pathKey] ?? "";
  if (!current.split(";").some((seg) => seg.replace(/[\\/]+$/, "").toLowerCase() === cargoBin.toLowerCase())) {
    env[pathKey] = `${cargoBin};${current}`;
  }
  return env;
}
