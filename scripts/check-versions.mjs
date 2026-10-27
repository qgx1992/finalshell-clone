#!/usr/bin/env node
/**
 * 校验三处版本号是否一致:
 *   package.json              -> 前端 npm 包
 *   src-tauri/Cargo.toml      -> Rust crate
 *   src-tauri/tauri.conf.json -> 安装包/应用显示版本
 *
 * 三处不一致会导致「界面显示 0.4.0、安装包却是 0.3.9」这类问题,
 * 在 CI 里直接失败比发版后才发现更省事。
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function readVersion(file, re) {
  const text = readFileSync(join(root, file), "utf8");
  const m = text.match(re);
  if (!m) {
    console.error(`无法在 ${file} 中找到 version 字段`);
    process.exit(1);
  }
  return m[1];
}

const versions = {
  "package.json": readVersion("package.json", /"version"\s*:\s*"([^"]+)"/),
  "src-tauri/Cargo.toml": readVersion("src-tauri/Cargo.toml", /^version\s*=\s*"([^"]+)"/m),
  "src-tauri/tauri.conf.json": readVersion(
    "src-tauri/tauri.conf.json",
    /"version"\s*:\s*"([^"]+)"/,
  ),
};

const values = Object.values(versions);
const unique = new Set(values);

for (const [file, v] of Object.entries(versions)) {
  console.log(`${v.padEnd(10)} ${file}`);
}

if (unique.size !== 1) {
  console.error("\n版本号不一致,请统一后再提交:");
  console.error(JSON.stringify(versions, null, 2));
  process.exit(1);
}

const [version] = values;
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`\n版本号 "${version}" 不是合法的语义化版本`);
  process.exit(1);
}

console.log(`\n版本号一致: ${version}`);