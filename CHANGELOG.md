# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 的格式,
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### 新增
- **顶栏连接 IP 显示**:当前活动标签页的 IP / 主机名显示在顶栏中央,状态圆点(连接中黄点闪烁 / 已连接绿点),带一键复制按钮(点击后短暂显示 ✓ 已复制)。
- **快捷命令在终端执行**:点击快捷命令后直接写入当前 SSH 会话(`writeTerminal`)在**终端窗口执行并回显输出**,不再在侧边栏面板里显示(移除 `qcp-output` 输出展示)。
- **`build:msvc` 打包脚本**(`scripts/build-msvc.mjs`):固化 MSVC 工具链打包命令,自动设置 `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc` 与 `CARGO_TARGET_DIR=src-tauri/target-msvc`,规避项目路径含空格时 GNU `windres` 的资源编译失败。

### 变更
- 快捷命令面板(`QuickCommandPanel`)不再依赖 `execCommand` 后端调用与前端 `serverName` prop;未连接时点击给出「请先连接服务器」提示。
- 构建产物目录 `src-tauri/target-msvc/` 加入 `.gitignore` 忽略。

---

## [0.1.0] - 2026-08

初始版本(FinalShell Clone 复刻):

- SSH 终端:多标签、密码 / 私钥认证、xterm.js 渲染、标签页保活(切换不重连)、终端尺寸自适应。
- SFTP 文件管理:同屏底部面板、增删改、拖拽上传 / 下载(进度条)、双击远程文件直接编辑(CodeMirror,保存写回)。
- 服务器监控:CPU / 内存 / 磁盘 / 网络实时折线图、负载、运行时间、主机名,2 秒采样,无需服务端插件。
- 效率工具:快捷命令面板、10 套终端配色、深 / 浅主题、字号调节、最近连接、服务器分组。
- 打包:NSIS 安装包,WebView2 依赖自检与故障排查文档。

[Unreleased]: https://github.com/qgx1992/finalshell-clone/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/qgx1992/finalshell-clone/releases/tag/v0.1.0
