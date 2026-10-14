# FinalShell-X 🖥️

基于 **Tauri 2 + Rust + React** 复刻的 FinalShell —— SSH 终端 / SFTP 文件管理 / 服务器监控一体化工具。

## ✨ 已实现功能

### 🖥️ SSH 终端
- 多标签连接管理,密码 / 私钥认证
- xterm.js 渲染,支持中文、鼠标选择、链接点击
- 终端尺寸自适应(窗口缩放自动 resize)
- 标签页保活:切换标签不重连,终端历史保留

### 📂 SFTP 文件管理
- 终端与文件管理同屏显示(底部面板)
- 目录列表、新建、重命名、删除(递归)
- 拖拽式上传 / 下载(带实时进度条)
- **双击远程文件直接编辑**(CodeMirror 语法高亮,保存即写回)

### 📊 服务器监控(无需安装服务端插件)
- CPU / 内存 / 磁盘 / 网络实时图表(自绘 Canvas 折线图)
- 负载、运行时间、主机名
- 每 2 秒自动采样

> 原理:单次 SSH 通道批量执行 `cat /proc/stat`、`/proc/meminfo`、`/proc/net/dev`、
> `df -Pk`、`/proc/loadavg`、`/proc/uptime` 等并解析;CPU / 网络速率靠两次采样差值计算。

### ⚡ 效率工具
- **顶栏连接 IP 显示**:当前活动标签页的 IP / 主机名显示在顶栏中央,一键复制
- **快捷命令面板**(可自定义):点击后**直接在终端窗口执行并回显输出**(写入当前 SSH 会话,由 shell 真正运行),不再在侧边栏面板里显示
- 10 套内置终端配色方案(Monokai / Dracula / Solarized / One Dark / Nord 等)
- 深色 / 浅色界面主题、字号调节
- 最近连接、服务器分组管理

## 🛠 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | Tauri 2(2.11.x / tao 0.35 / wry 0.55) |
| SSH 库 | russh 0.62(纯 Rust,ring 加密) |
| SFTP | russh-sftp |
| 前端 | React 18 + TypeScript + Vite 6 |
| 终端 | xterm.js + addon-fit + addon-web-links |
| 编辑器 | CodeMirror 6(多语言语法高亮) |
| 状态管理 | zustand |
| 运行时 | tokio(异步) / base64 / serde_json |

## 🏗 架构与数据流

```
┌────────────── 前端 (src/, React + zustand) ──────────────┐
│ App ── Sidebar / TabBar + TerminalPanel + SftpPanel / MonitorPanel │
│ store/app.ts ── 全局状态(servers / settings / tabs)       │
│ api.ts ── 25 个 Tauri invoke 封装 + listen 事件订阅        │
└───────────┬───────────────────────────────────────────────┘
            │ Tauri IPC:invoke 请求 + emit 事件推送
┌───────────▼─────────────── Rust 后端 (src-tauri/) ────────┐
│ lib.rs ── 装配命令 + SshManager 全局状态                   │
│ ├─ store.rs     配置持久化(servers.json / settings.json)   │
│ ├─ ssh/manager.rs  russh 会话 / 终端通道 / 读循环 / exec    │
│ ├─ ssh/sftp.rs     SFTP 列目录 / 增删改 / 上传下载 / 编辑   │
│ └─ monitor.rs      监控采样与解析                          │
└────────────────────────────────────────────────────────────┘
```

**一次 SSH 连接的关键数据流:**
1. 前端 `openTab` → `connect_server`(invoke)→ russh 认证 + PTY + shell
2. 后端 spawn 读循环,16ms / 32KB 批量合并 → `emit("term-data", base64)`
3. 前端 `listen("term-data")` 流式解码(`TextDecoder {stream:true}`)写入 xterm.js
4. 键盘输入前端缓冲 12ms → `write_terminal` 批量写入
5. 断开 → `emit("term-exit")` → Tab 状态置 `closed`(历史保留,可重连)

**监听的事件:**
| 事件 | 发送方 | 消费方 | 说明 |
|---|---|---|---|
| `term-data` | 后端读循环 | TerminalPanel | 终端输出(base64,批量) |
| `term-exit` | 后端 | TerminalPanel | 连接状态变更 |
| `sftp-progress` | 后端传输出流 | SftpPanel | 上传/下载进度(节流) |

## ⚡ 性能优化(2026-08)

- **标签页保活**:所有标签的终端 / SFTP 面板常驻挂载,切换标签只切换可见性(`.tab-workspace`),不再断开重连,终端历史保留
- **终端输出批量合并**:后端读循环 16ms / 32KB 批量发送 `term-data`,IPC 事件数降 10~50 倍
- **按键输入批量发送**:前端缓冲 12ms 批量写入,消除每击键一次的 IPC 往返
- **流式解码**:`TextDecoder {stream:true}` 流式解码,跨包多字节 UTF-8 不再乱码
- **SFTP 进度节流**:进度事件 1MB / 100ms 一次(原每 64KB 一次,传 1GB 文件事件数 ~16000 → ~10),结束时补发 100%;传输缓冲 64KB → 256KB
- **前端分包 + 懒加载**:CodeMirror 1.7MB 独立 chunk 且懒加载(仅打开远程编辑器时),首屏只加载 index / react / xterm;Vite `manualChunks` 将 xterm / react / codemirror / tauri / vendor 分包

> 注:曾尝试复用 SSH exec 通道减少监控轮询开销,但 RFC 4254 规定一个 channel 只能 exec 一次(OpenSSH 拒绝重复请求),已回退为每次独立通道。

## 🚀 开发运行

```bash
# 依赖安装
npm install

# 前端 + 后端联调开发
npm run tauri dev
```

> ⚠️ **Windows 构建需要 mingw-w64 工具链**(Rust GNU target),已配置于 `C:\Users\QIU\mingw64`。
> 编译前请确保 PATH 包含其 bin 目录。

## 📦 打包

```bash
# 推荐:MSVC 工具链一键打包(自动设置 RUSTUP_TOOLCHAIN + target-msvc 目录)
npm run build:msvc            # 生成 NSIS 安装包

# 或使用默认(当前 GNU)工具链
npm run tauri build
```

> 💡 `npm run build:msvc` 通过 `scripts/build-msvc.mjs` 固定使用 MSVC 工具链
> (`stable-x86_64-pc-windows-msvc`)并指向独立的 `src-tauri/target-msvc/` 编译目录,
> 有效规避下面「项目路径含空格」导致的 windres 失败 —— **推荐优先使用这条命令**。

### ⚠️ 打包常见问题(必读)

1. **项目路径不能包含空格(仅 GNU 工具链)!**
   若用默认 GNU 工具链,本项目位于 `D:\A-my project\...`(含空格),`windres` 在预处理 rc
   资源文件时会把带空格的 `--include-dir` 路径在空格处截断,导致资源编译失败:
   ```
   cc1.exe: fatal error: ...\out: No such file or directory
   ```
   **解法 A(推荐,已固化):** 直接用 `npm run build:msvc` 走 MSVC 工具链,不受路径空格影响。
   **解法 B(遗留,gNU):** 建一个无空格路径的 junction,再从 junction 路径构建:
   ```powershell
   mklink /J "%TEMP%\fsc" "D:\A-my project\finalshell-clone"
   Set-Location "%TEMP%\fsc"          # 后端在无空格路径下构建,windres 正常
   npm run tauri build
   ```
   > 注意:前端 vite 构建不支持 junction 路径(vite 会解析成真实绝对路径而报错),需在**真实路径**先 `npm run build` 生成 dist,
   > 再在 junction 路径 `npx tauri build --config '{"build":{"beforeBuildCommand":""}}'` 跳过前端构建,直接编后端 + 打包。

2. **`webview2-hook.nsh` 的路径必须与构建机一致。**
   该 hook 负责把 `WebView2Loader.dll` 复制进安装目录(GNU 工具链下 tauri-bundler 默认漏打包)。
   文件里写死了构建机的绝对路径,项目移动后必须同步修改,否则 NSIS 打包报 `no files found`。
   > NSIS 宏环境里没有 `PROJECTDIR` 之类的变量,只能写死绝对路径;若路径含空格,`File "..."` 需加引号。

3. **`msvc` 缺包 / 其它:** GNU 工具链下编译需要 `mingw-w64`(含 `windres`、`ld`),见上。

## 🧪 测试服务器

项目附带 `test_server.py` —— 一个基于 paramiko 的模拟 SSH 服务器
(127.0.0.1:2222,账号 `test` / 密码 `test123`),内置迷你 Linux shell 和
虚拟 `/proc` 文件系统,可完整测试终端、SFTP、监控功能:

```bash
python test_server.py
```

### 端到端测试

`e2e_test.py` 通过 WebView2 CDP(端口 9222)驱动 UI。运行前需在
`src-tauri/tauri.conf.json` 的窗口配置中**临时**添加:

```json
"additionalBrowserArgs": "--remote-debugging-port=9222 --remote-allow-origins=*"
```

> ⚠️ 注意:环境变量 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` 对 Tauri 2 无效(创建
> WebView2 时会用空参数覆盖);新版本 WebView2 还必须加 `--remote-allow-origins=*`
> 否则 CDP WebSocket 握手返回 403。**测试完务必移除该参数**,避免调试端口打进生产包。

修改配置后 `npm run tauri dev` 会自动重编译重启,然后执行:

```bash
python e2e_test.py
```

## 🚨 故障排查:安装后没有界面

**现象:** 双击(或从开始菜单)运行安装好的 FinalShell-X,没有任何窗口出现,
进程可能在后台挂着,事件日志里也查不到崩溃。

**最常见根因:WebView2 依赖缺失或过旧。**

Tauri 2 应用依赖系统 **WebView2 Runtime** 渲染界面,而整个 WebView2 浏览器进程序列
依赖 **Microsoft Visual C++ 2022 Runtime**(`vcruntime140.dll` / `msvcp140.dll` 等)。
当这些组件缺失或版本过旧时,WebView2 环境创建会失败(`CreateCoreWebView2EnvironmentWithOptions`
返回 `0x800700AA / ERROR_BUSY`),窗口便无法显示。

**排查步骤:**

1. **检查 WebView2 Runtime 是否安装及版本** —— 注册表查询:
   ```powershell
   Get-ItemProperty "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" | Select pv
   ```
   没有输出或在 `C:\Program Files (x86)\Microsoft\EdgeWebView\Application` 无版本目录 → 需要安装 [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)。

2. **检查 VC++ 2022 Runtime 是否安装** —— 控制面板「应用」里搜「Microsoft Visual C++ 2022」;
   缺失则安装 [Visual C++ Redistributable](https://aka.ms/vs/17/release/vc_redist.x64.exe)。

3. **手工验证应用能否出窗** —— 直接运行安装目录的
   `%LOCALAPPDATA%\FinalShell-X\finalshell-x.exe`,若窗口出现说明应用本身没问题,
   只是依赖环境未就绪。

**排查小技巧:** 若开发环境(C# / Edge / 其它 WebView2 应用)能正常用,而本应用独独无界面,
优先怀疑**应用数据目录**(`%LOCALAPPDATA%\com.finalshell.clone\EBWebView`)被残留的 WebView2 浏览器进程
锁定(旧实例异常退出后未清干净)。删除该目录再启动通常可恢复(不影响已保存的服务器配置——
配置存于 `%APPDATA%\com.finalshell.clone\servers.json` / `settings.json`,与 WebView2 数据目录分开)。

## 📁 项目结构

```
finalshell-clone/
├── src/                         # React 前端
│   ├── components/              # Sidebar / TerminalPanel / SftpPanel / MonitorPanel
│   │   │                        #   / ConnectionDialog / EditorModal / QuickCommandPanel
│   │   │                        #   / SettingsModal / ContextMenu / TabBar / Sparkline
│   ├── store/app.ts             # zustand 全局状态(tabs / servers / settings)
│   ├── api.ts                   # Tauri invoke 封装 + 结构化错误
│   ├── events/terminalRouter.ts # 全局终端/SFTP 事件路由
│   ├── generated/               # 由 Rust 类型生成的 TS 定义
│   ├── themes.ts                # 10 套终端配色方案
│   ├── types.ts                 # 与 Rust 后端对应的类型定义
│   ├── main.tsx / App.tsx      # 入口与布局
│   └── styles.css               # 全局样式
├── src-tauri/                   # Rust 后端
│   ├── src/
│   │   ├── lib.rs               # 命令注册(app 入口)
│   │   ├── main.rs              # Windows 子系统入口
│   │   ├── ssh/
│   │   │   ├── manager.rs       # russh 会话管理 + 终端读循环 + 主机指纹校验
│   │   │   ├── proxy.rs         # SOCKS5 / HTTP CONNECT 代理
│   │   │   └── sftp.rs          # SFTP 文件操作(原子写入/原生递归删除)
│   │   ├── security/
│   │   │   ├── credentials.rs   # 系统凭据库封装
│   │   │   └── known_hosts.rs   # TOFU 主机密钥校验
│   │   ├── monitor.rs           # 跨平台监控采样与解析
│   │   ├── error.rs             # 结构化错误类型
│   │   └── store.rs             # 配置持久化(servers.json / settings.json)
│   ├── webview2-hook.nsh        # NSIS 钩子:打包 WebView2Loader.dll
│   ├── tauri.conf.json          # Tauri 配置
│   └── capabilities/default.json
├── scripts/
│   └── build-msvc.mjs           # MSVC 工具链打包脚本(build:msvc)
├── test_server.py               # 端到端测试用 SSH 服务器
└── e2e_test.py                  # CDP 驱动的 UI 自动化测试
```

## ⚠️ 已知限制

- 端口转发 / 海外加速尚未实现
- 未实现终端 cd 与 SFTP 面板目录自动同步(有 `sftp_pwd` 命令预留,可手动刷新)
- RDP 依赖系统客户端:Windows 用 `mstsc.exe`,Linux/macOS 需自行安装 FreeRDP(`xfreerdp`)
- 内置编辑器仅支持文本文件,上限 5 MB;更大的文件请下载后用专业工具编辑
- 监控依赖远程系统的内置命令,Linux/Windows/macOS 之外的系统不支持

## 🛡 安全设计(0.4.0 起)

- **主机密钥校验(TOFU)**:首次连接展示 SHA256 指纹,由用户确认后写入本应用的 `known_hosts`;
  之后指纹不一致会拒绝连接并提示新旧指纹对比,避免中间人攻击。
- **凭据存系统凭据库**:登录密码、私钥口令、代理密码保存在操作系统凭据管理中,
  不写入配置文件,也不会下发到前端。
- **内容安全策略已启用**:`tauri.conf.json` 配置了 CSP,插件权限收敛到实际使用的范围。
- **SFTP 递归删除不跟随符号链接**,且限制单次删除条目数;编辑器保存采用原子写入。
- 日志不记录任何凭据。
