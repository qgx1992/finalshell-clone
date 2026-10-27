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

### 2.4 SFTP 右键打包(zip / tar.gz)

命令:`sftp_create_archive(session_id, paths: Vec<String>, format: "zip" | "tar.gz" | "tar.zst")` → `ArchiveInfo`。

- **两条路径**。`same_parent`(所选条目同属一个父目录)且脚本不超长时,走 **服务器端打包**:
  脚本通过 `exec("sh -s")` + stdin 传入,文件名以 `set --` 位置参数携带,**全程相对路径 +
  先 `cd "$D"`**,因此不含任何路径拼接,空格 / 单引号 / 中文 / `$` 都不需要额外转义
  (`shell_single_quote` 只用于父目录与基名的单引号包裹)。压缩包名先在远端探测重名,
  冲突则用 `名字 (1).zip`。服务器缺 `zip`/`tar`、条目过多或所选不在同一目录时,
  `fallback_reason` 记下原因并回落到 **本地 SFTP 流式打包**(逐块 `read` → 写本地临时 zip
  → 上传回同目录 → 删临时文件,`method = "sftp"`)。
- **进度**:`sftp-archive-progress` 推 6 元组
  `(session_id, percent, done, total, phase, unit)`。`unit = "files"` 时 done/total 是条目数
  (服务器端脚本每压完一项打一行 `FINA-PROGRESS <万分比> <序号>`),`unit = "bytes"` 时是字节数
  (本地打包按已读字节,上传阶段按已写入字节)。整段进度里服务器端占 5%~95%,
  本地打包 0%~80%、上传 80%~98%。
- **空目录也进包**:本地路径走目录栈,空目录补一条末尾带 `/` 的条目;服务器端靠 `zip -r` / `tar`。
- 前端:文件列表右键菜单,`Ctrl/⌘+点击` 增减选中、`Shift+点击` 从锚点连选;
  多选时菜单文案变成「生成 ZIP 压缩包(N 项)」。右键某行会先把选择收敛到该行,
  避免"点了 A 却把 B 一起打包"。

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

### 3.4 无 MSVC 的机器:MinGW(GNU)工具链出包

> 适用场景:开发机上**没装 Visual Studio Build Tools**,只有 MinGW-w64
> (本机实测 `D:\mingw\mingw64`,`gcc 16.2.0 posix-seh`),又需要真的产出安装包。
> 产出的安装包功能与 MSVC 版一致,已实测"静默安装 → 启动 → 静默卸载"全通过。

三件事必须同时满足,少一件就编不过:

1. **目标换成 `x86_64-pc-windows-gnu`**(rustup 需先 `rustup target add x86_64-pc-windows-gnu`)。
2. **MinGW 的 `bin` 要排在 `PATH` 最前**(`gcc.exe` / `windres.exe` 从这里找)。
3. **`CARGO_TARGET_DIR` 必须指到不含空格的目录** —— 这是绕开 `windres` 空格截断问题的
   关键:把 target 目录挪出 `D:\A-my project\...` 后,GNU 工具链**不需要**任何额外改动
   就能正常出包(仓库路径本身含空格也没关系,因为传给 `windres` 的 `--include-dir`
   根在 target 目录下)。

```powershell
$env:CARGO_HOME   = 'D:\rust\cargo'
$env:RUSTUP_HOME  = 'D:\rust\rustup'
$env:PATH         = "D:\mingw\mingw64\bin;$env:CARGO_HOME\bin;$env:PATH"
$env:CARGO_TARGET_DIR = 'D:\rust-build\finalshell-clone'   # 关键:无空格的 target 目录

Set-Location 'D:\A-my project\finalshell-clone\src-tauri'
npx tauri build --target x86_64-pc-windows-gnu --bundles nsis
```

产物(与 MSVC 版路径结构相同,只是根目录换成了 `$env:CARGO_TARGET_DIR`):

- 安装包:`D:\rust-build\finalshell-clone\x86_64-pc-windows-gnu\release\bundle\nsis\FinalShell-X_<版本>_x64-setup.exe`
- 便携 exe:`D:\rust-build\finalshell-clone\x86_64-pc-windows-gnu\release\finalshell-x.exe`

参考体积(release profile,`lto=true / codegen-units=1 / strip=true`,v0.4.0):
安装包约 **3.96 MB**,内含 exe 约 11.4 MB + `WebView2Loader.dll` 0.15 MB。
GNU 构建会多带一个 `WebView2Loader.dll`,MSVC 版没有这个文件。

**不需要手工预置 NSIS**:tauri-bundler 会把 NSIS 3.11 与 `nsis_tauri_utils.dll`
下载到 `%LOCALAPPDATA%\tauri\NSIS` 并校验 SHA1。手工往里塞文件会被判定为
`NSIS directory is missing some files. Recreating it.` 并从零重建 —— 所以网络不通时
**设代理给构建进程**(`$env:HTTPS_PROXY`),而不是手工拼缓存目录。

## 4. 常见故障

| 现象 | 原因 | 处理 |
|---|---|---|
| 打包报 `No such file or directory`(windres) | GNU 工具链 + 路径含空格 | 改用 `npm run build:msvc`,或按 §3.4 把 `CARGO_TARGET_DIR` 挪到无空格目录 |
| 安装后无界面 | WebView2 Runtime / VC++ 2022 Runtime 缺失或过旧 | 详见 `README.md` 故障排查 |
| 应用可运行但界面不出现 | WebView2 数据目录被旧进程锁定 | 删除 `%LOCALAPPDATA%\com.finalshell.clone\EBWebView` 后重启 |
| NSIS 报 `no files found` | `webview2-hook.nsh` 内绝对路径与构建机不一致 | 重新运行 `npm run build:msvc` 让脚本重新生成该文件 |
| `failed to bundle project: timeout: global` | 下载 NSIS 时 GitHub 直连超时/被重置 | 给构建进程设 `$env:HTTPS_PROXY` 后重跑;构建已编译完成,重跑只会重新打包 |
| `cargo test` 启动即 `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND` | GNU 工具链 + 本项目依赖组合的静态 CRT 问题(最小 crate 不复现) | 单测请在 MSVC 环境或 CI 上跑;本地用 `cargo check --all-targets` + 实跑产物验证 |
| GNU 构建的 exe 双击无反应/闪退 | 缺 MinGW 运行库(非静态链接时) | 本仓库配置默认静态,C 运行库已内联;若报 DLL 缺失,确认用的是 `x86_64-pc-windows-gnu` 目标产出的 exe |

### 4.1 构建完报路径(约定)

构建/打包成功后,直接回报**安装包绝对路径 + 体积 + SHA256**,不要只说"成功了"。
需要复制的场合也不要复制到别处 —— 产物就留在上表的位置,按路径直接取。

```powershell
$setup = "$env:CARGO_TARGET_DIR\x86_64-pc-windows-gnu\release\bundle\nsis\FinalShell-X_0.4.0_x64-setup.exe"
Get-Item $setup | Select-Object FullName,Length,LastWriteTime
Get-FileHash $setup -Algorithm SHA256
```

安装包自检(不需要 GUI,静默装到临时目录再卸掉):

```powershell
$t = "$env:TEMP\nsis-smoke"; Remove-Item -Recurse -Force $t -ErrorAction SilentlyContinue
(Start-Process $setup -ArgumentList '/S',"/D=$t" -Wait -PassThru).ExitCode   # 0 = 安装成功
Test-Path "$t\finalshell-x.exe"                                              # True = 主程序就位
Start-Process "$t\finalshell-x.exe" -PassThru | Select-Object Id             # 进程存活、标题 FinalShell-X
(Start-Process "$t\uninstall.exe" -ArgumentList '/S' -Wait -PassThru).ExitCode
```

> 卸载器会自行复制一份到临时目录再自删,父进程句柄可能一直等不到 ——
> 看到安装目录消失即为卸载完成,不必死等 `.ExitCode`。

## 5. 路由 / 目录

```
scripts/                    # 构建脚本
  build-msvc.mjs            # MSVC 打包(build:msvc)
  check-versions.mjs        # 三处版本号一致性校验(check:versions)
src/components/
  QuickCommandPanel.tsx     # 快捷命令(点击在终端执行)
  TerminalPanel.tsx         # xterm.js 终端 + SSH 事件订阅
src-tauri/src/ssh/
  archive.rs                # 远程打包(zip / tar.gz,服务器侧命令 + 本地 SFTP 流式回落)
src-tauri/target-msvc/      # MSVC 编译产物(已 gitignore)
src-tauri/tauri.e2e.conf.json  # e2e 专用窗口配置(开 CDP 调试端口)
```

### 5.1 构建产物的默认落位

| 场景 | 命令 | 产物目录 |
|---|---|---|
| MSVC 安装包 | `npm run build:msvc` | `src-tauri/target-msvc/release/bundle/nsis/` |
| MSVC 便携 exe | `npm run build:msvc -- --no-bundle` | `src-tauri/target-msvc/release/finalshell-x.exe` |
| GNU 安装包 | 见 §3.4 | `$env:CARGO_TARGET_DIR\x86_64-pc-windows-gnu\release\bundle\nsis\` |
| GNU debug(给 e2e 用) | `npx tauri build --debug --no-bundle --config tauri.e2e.conf.json --target x86_64-pc-windows-gnu` | `$env:CARGO_TARGET_DIR\x86_64-pc-windows-gnu\debug\finalshell-x.exe` |

`$env:CARGO_TARGET_DIR` 一旦设置,target 就不在仓库里了 —— 找产物时先看自己设到了哪里。

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
- `npm run check:versions`:校验 `package.json` / `src-tauri/Cargo.toml` /
  `src-tauri/tauri.conf.json` 三处版本号一致,CI 与 `npm run verify` 都会跑。

### 8.1 端到端冒烟测试(`e2e_test.py`)

用 **WebView2 CDP** 驱动真实应用窗口,不需要 Playwright 之类的大依赖。

**先解决 CDP 端口的来源**:wry 创建 WebView2 环境时总是显式传
`AdditionalBrowserArguments`,它会**覆盖** `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`
环境变量,所以"设环境变量再启动"这条路在 Tauri 应用上无效。正确做法是用
`src-tauri/tauri.e2e.conf.json` 覆盖窗口配置,把 `--remote-debugging-port=9222`
编译进二进制:

```bash
# 1) 用 e2e 配置启动(dev 模式,vite 由 tauri 自己拉起)
cd src-tauri
npx tauri dev --config tauri.e2e.conf.json

# 2) 冒烟:UI 渲染 / 新建连接对话框 / 快捷命令面板(不需要服务器)
python e2e_test.py

# 3) 完整:另需 127.0.0.1:2222 上有 test/test123 的 SSH 测试机
python e2e_test.py --full     # 等价于 FINA_E2E_SSH=1
```

- 依赖:`pip install websocket-client`。
- 脚本细节(都是踩过的坑):
  - 用 `ProxyHandler({})` 直连 127.0.0.1 — 系统配了 `HTTP_PROXY` 时 urllib 会把
    本地请求也丢给代理,表现成"连接超时"而不是"拒绝连接"。
  - WebSocket 握手要 `suppress_origin=True` — 否则 Chromium 的 DevTools 端点回
    `403 Forbidden`(它只接受无 Origin 的握手,或 `--remote-allow-origins=*`)。
  - 页面选择不写死 URL:`--url-match 1420` 才限制子串,dev(`http://localhost:1420`)
    与打包态(`tauri://localhost`)都能跑。
  - 输出强制 UTF-8,Windows 控制台默认不是 UTF-8,中文日志会乱码。
- 退出码 0/1,失败会打印失败步骤与堆栈,可直接作为 CI 门禁用。
- CI 里的 `e2e` job:`npm run build` → `npx tauri build --debug --no-bundle
  --config tauri.e2e.conf.json` → 启动 exe → 跑冒烟 → 收集
  `%LOCALAPPDATA%\com.finalshell.clone\logs`。
  **该 job 目前标了 `continue-on-error: true`**,先观察 GitHub 图形会话的稳定性;
  稳定后删掉那一行即变成阻塞门禁。
- 本地(无 MSVC、只有 MinGW)跑法:`cargo build` 的 debug 产物是 **dev 模式**,
  会去连 `http://localhost:1420`,因此先起 `npm run dev`,再用
  `tauri.e2e.conf.json` 里的 `additionalBrowserArgs` 临时写进 `tauri.conf.json`
  重编一次(或直接 `cargo build` 后用 dev server 配合),即可跑通冒烟。


