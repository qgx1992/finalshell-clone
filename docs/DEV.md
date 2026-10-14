# 开发笔记 (DEV)

面向本项目(`finalshell-clone`)维护者的开发说明。架构总览、关键实现机制与构建细节。

## 1. 概览

Tauri 2 + Rust + React 的桌面 SSH 工具。前后端通过 **Tauri IPC** 通信:
- **invoke 请求**：前端 `src/api.ts` 调用 Rust 命令(如 `connect_server` / `write_terminal` / `list_sessions`)。
- **事件推送**：后端 `emit` → 前端 `listen`(如 `term-data` / `term-exit` / `sftp-progress`)。

详见主 `README.md` 的「架构与数据流」。

## 2. 关键实现细节

### 2.1 快捷命令在终端窗口执行

- 入口：`src/components/QuickCommandPanel.tsx`(右上角「⚡ 命令」按钮呼出,挂载在侧边栏底部)。
- 机制：点击命令时不再用后端的 `exec_command`(后端 exec 通道返回结果、在前端面板里显示),而是把命令
  **base64 后通过 `writeTerminal(sessionId, b64)` 写入当前 SSH 会话的 shell**,由 shell 真正执行,输出
  经 `term-data` 事件流式回显到 xterm.js —— 体验上就像在终端里手敲并回车执行。
- 命令末尾会追加 `\n` 回车,确保 shell 立即执行。
- 未连接(`sessionId == null`)时无法写入,面板给出「请先连接服务器」提示,并保留一条轻提示「已发送到终端:<命令>」。
- 相关后端命令：`write_terminal`(`ssh/manager.rs`)。旧的 `exec_command` 命令仍保留在后端,前端已不调用。

### 2.2 顶栏连接 IP + 复制

- 入口：`src/App.tsx` 的 `<header class="app-header">` 中部,渲染 `header-conn` 胶囊。
- 数据：取当前活动标签 `activeTab.host`,条件为 `status !== "closed"`。
- 复制：优先 `navigator.clipboard.writeText`;旧 WebView2 无 Clipboard API 时降级到
  `document.execCommand("copy")`(`fallbackCopy`,创建一个透明 textarea 选中后复制)。
- 样式：`styles.css` 中 `.header-conn` 绝对居中于顶栏,不干扰两侧 logo 与操作按钮的 `space-between` 布局。

### 2.3 标签页保活(切换不重连)

- 所有标签的 `TerminalPanel` 常驻 `display` 切换可见性,切回活动时重新 `fit()` 并同步远程窗口尺寸。

## 3. 构建与打包

### 3.1 前端

```bash
npm install
npm run build        # tsc + vite build → dist/
npm run dev          # 开发服务器(vite)
```

### 3.2 打包(推荐 MSVC)

> ⚠️ **为何必须用 MSVC?** 项目路径 `D:\A-my project\finalshell-clone` 含空格,默认 GNU 工具链的
> `windres` 会把带空格的 `--include-dir` 在空格处截断,报 `cc1.exe: fatal error: ...\out: No such file or directory`。

```bash
npm run build:msvc   # = node scripts/build-msvc.mjs
```

`scripts/build-msvc.mjs` 做了什么:

1. 固定 `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`。
2. 若无 `CARGO_TARGET_DIR` 则设为 `src-tauri/target-msvc`(与 GNU 的 `src-tauri/target/` 分开,避免混用缓存)。
3. 通过 `npx tauri build`(参数透传,如 `npm run build:msvc -- --bundles nsis`)完成编译 + NSIS 打包。

产物:
- 安装包: `src-tauri/target-msvc/release/bundle/nsis/FinalShell-X_<版本>_x64-setup.exe`
- 便携 exe: `src-tauri/target-msvc/release/finalshell-x.exe`

> NSIS 钩子 `webview2-hook.nsh` 由 `scripts/build-msvc.mjs` 按当前项目路径动态生成,
> 换机器 / 换路径后无需手工修改。

### 3.3 只要可执行 exe,不要安装包

```bash
npx tauri build --no-bundle   # (与工具链无关,如需 MSVC 用 npm run build:msvc -- --no-bundle)
```

## 4. 常见故障

| 现象 | 原因 | 处理 |
|---|---|---|
| 打包报 `No such file or directory`(windres) | GNU 工具链 + 路径含空格 | 改用 `npm run build:msvc` |
| 安装后无界面 | WebView2 Runtime / VC++ 2022 Runtime 缺失或过旧 | 详见 `README.md` 故障排查 |
| 应用可运行但界面不出现 | WebView2 数据目录被旧进程锁定 | 删除 `%LOCALAPPDATA%\com.finalshell.clone\EBWebView` 后重启 |
| NSIS 报 `no files found` | `webview2-hook.nsh` 内绝对路径与构建机不一致 | 重新运行 `npm run build:msvc` 让脚本重新生成该文件 |

## 5. 路由 / 目录

```
scripts/                    # 构建脚本
  build-msvc.mjs            # MSVC 打包(build:msvc)
src/components/
  QuickCommandPanel.tsx     # 快捷命令(点击在终端执行)
  TerminalPanel.tsx         # xterm.js 终端 + SSH 事件订阅
src-tauri/target-msvc/      # MSVC 编译产物(已 gitignore)
```

## 6. 安全与凭据(0.4.0 起)

### 6.1 主机密钥校验(TOFU)

- 实现在 `src-tauri/src/security/known_hosts.rs`,复用 OpenSSH `known_hosts` 文本格式。
- 文件位置:应用配置目录下的 `known_hosts`(与系统 `~/.ssh/known_hosts` 隔离)。
- 流程:首次连接 → `connect_server` 返回 `host_key_unknown` + SHA256 指纹 →
  前端 `HostKeyDialog` 展示给用户 → 用户确认后带上 `hostKeyDecision`/`hostKeyFingerprint` 重试。
- 指纹不匹配 → `host_key_changed`,同时返回旧指纹;确认接口只接受"当前指纹"本身,
  防止确认后指纹被替换。
- 设置页可查看已信任主机并清除记录。

### 6.2 凭据存储

- 登录密码、私钥口令、代理密码存操作系统凭据库(`keyring` crate)。
- `servers.json` 中不再保存明文;`list_servers` 返回的配置经过 `sanitize`,只带 `has_password` 等标志。
- 保存时前端通过 `SecretsInput` 表达意图:`keep` / `set` / `clear`,
  避免"清空密码"与"未修改密码"混淆。
- 启动时 `migrate_plaintext_secrets` 把旧配置的明文迁入凭据库并清空字段,备份为 `servers.json.bak`。

### 6.3 代理

- `src-tauri/src/ssh/proxy.rs`:`socks5`(经 `tokio-socks`)与 `http`(自实现 CONNECT 握手)。
- `open_transport` 在直连与代理之间选择,返回 `TcpStream` 交给 `connect_stream`。

## 7. 监控与平台适配

- `monitor.rs` 首次轮询执行 `uname -s || ver` 判定平台并缓存。
- Linux:`/proc/stat`、`/proc/meminfo`、`/proc/net/dev`、`df -Pk` 等。
- Windows:PowerShell `Get-CimInstance` / `Get-NetAdapterStatistics`,CPU 取瞬时占用率。
- macOS:`sysctl`、`vm_stat`、`netstat`、`df`。
- 前端 `MonitorPanel` 采用串行轮询 + 指数退避,不会产生重叠请求。

## 8. 测试与 CI

- `npm run typecheck`:前端类型检查。
- `npm run cargo:test`:后端单元测试,同时通过 `ts-rs` 导出 `src/generated/` 类型。
- `npm run verify`:依次执行类型检查、后端测试、前端构建。
- GitHub Actions(`.github/workflows/ci.yml`)在 Windows 下校验以上流程,
  并检查 `src/generated` 是否与 Rust 类型保持同步。
- `npm run types:gen`:`cargo test export_bindings`,只跑 ts-rs 生成的导出测试,重写 `src/generated/`。
- 仓库根 `.gitattributes` 固定 `* text=auto eol=lf`:ts-rs 只写 LF,若检出被转成 CRLF,
  CI 的生成物校验会出现假差异,因此文本文件统一 LF。
- 本地 `cargo` 通过 `scripts/cargo-msvc.mjs` 调用,自动注入 VS Build Tools 环境;
  在普通终端下即可运行,无需打开 Developer Command Prompt。
