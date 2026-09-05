# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 的格式,
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [0.3.4] - 2026-09-05

### 新增
- **RDP 默认共享剪贴板**:`.rdp` 显式写入 `redirectclipboard:i:1`,本地与远程 Windows 可互相复制粘贴;表单里新增「共享剪贴板」开关(默认勾选,可关),旧配置自动补为开启。
- **自动勾选 Windows 资源确认弹窗里的「剪贴板」**:Windows 11 近期版本在每次 RDP 连接前弹的「远程桌面连接安全警告」里,剪贴板/打印机默认**未勾选**且不受 `.rdp` 与 `LocalDevices` 信任记录影响 —— 直接点「连接」等于拒绝共享剪贴板,这是剪贴板一直不通的真正原因。现在开启共享剪贴板时,应用会在自己拉起的那个 mstsc 进程的确认弹窗上自动把「剪贴板」勾上(不代替用户点「连接」、不动打印机/智能卡/不再询问等其它选项);支持中/英/日/韩/俄/德/法/西/葡/意/瑞/波兰/荷兰语文案,识别不到时静默放弃(用户仍可手动勾)。

### 修复
- **RDP 固定分辨率窗口贴顶 / 一拖就缩回小窗**:
  - `.rdp` 新增 `dynamic resolution:i:0` 与 `smart sizing:i:0` —— 此前该项默认为 1,mstsc 会在窗口尺寸变化时重新协商会话分辨率,所以拖动后分辨率就被改成小窗尺寸。
  - 新增 `winposstr:s:0,1,...` 将窗口显式指定为**普通窗口(ShowCmd=1)并居中**于主屏工作区;此前未指定时 mstsc 会沿用 Default.rdp 的最大化状态,表现为贴顶,而从最大化往下拖会被 Windows 还原成很小的窗口。
  - 拉起 mstsc 时同步传入 `/w:<宽> /h:<高>` 命令行开关作双保险。
  - 选到比本机屏幕还大的分辨率时,窗口贴工作区左上角(保证标题栏可拖),并在表单里提前提示会出现滚动条。

### 其他
- `rdp.rs` 抽出 `Display` 枚丙 + `parse_resolution`,并补上 6 个单元测试(全屏/自适应/固定分辨率居中/超大分辨率靠边/非法值回退/命令行开关)。

## [0.3.2] - 2026-09-05

### 修复
- **RDP 分辨率选择无效(选择固定分辨率仍全屏)**:`.rdp` 中 `screen mode id` 取值写反 —— `i:1` 才是窗口模式、`i:2` 是全屏。现修正为:全屏→`i:2`;固定分辨率/自适应→`i:1` + 宽高/smart sizing,固定分辨率真正按所选尺寸窗口化。

## [0.3.1] - 2026-09-05

### 新增
- **RDP 分辨率选择**:远程 Windows 服务器新增「分辨率」选项(全屏 / 自适应窗口 / 1280x720 / 1366x768 / 1600x900 / 1920x1080 / 2560x1440),写入 `.rdp` 的 `screen mode` / `desktopwidth` / `desktopheight`;旧数据默认全屏,行为不变。

## [0.3.0] - 2026-09-05

### 新增
- **项目改名 FinalShell-X**:产品名、窗口标题、logo、欢迎页、安装包名统一为 FinalShell-X(Rust 包名同步为 `finalshell-x`)。
- **连接远程 Windows**:服务器表单新增「连接类型」(SSH / 远程 Windows RDP);RDP 服务器点击连接时生成临时 `.rdp` 文件并调用系统自带 `mstsc.exe` 拉起远程桌面(自动填入主机、端口 3389、用户名),侧边栏带 RDP 徽标;旧数据自动兼容(默认 SSH)。
- **关闭窗口确认**:点击窗口 × / Alt+F4 时弹「确认退出」系统对话框,确认后才退出,防止误关丢失会话。

## [0.2.0] - 2026-09-05

### 新增
- **顶栏连接 IP 显示**:当前活动标签页的 IP / 主机名显示在顶栏中央,状态圆点(连接中黄点闪烁 / 已连接绿点),带一键复制按钮(点击后短暂显示 ✓ 已复制)。
- **快捷命令在终端执行**:点击快捷命令后直接写入当前 SSH 会话(`writeTerminal`)在**终端窗口执行并回显输出**,不再在侧边栏面板里显示(移除 `qcp-output` 输出展示)。
- **`build:msvc` 打包脚本**(`scripts/build-msvc.mjs`):固化 MSVC 工具链打包命令,自动设置 `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc` 与 `CARGO_TARGET_DIR=src-tauri/target-msvc`,规避项目路径含空格时 GNU `windres` 的资源编译失败。

### 变更
- 快捷命令面板(`QuickCommandPanel`)不再依赖 `execCommand` 后端调用与前端 `serverName` prop;未连接时点击给出「请先连接服务器」提示。
- 构建产物目录 `src-tauri/target-msvc/` 加入 `.gitignore` 忽略。

### 修复
- **编辑服务器弹窗误触关闭**:点击遮罩不再直接关闭弹窗 —— 编辑模式下点遮罩一律不关闭;任何关闭操作(右上角 ×)若存在未保存修改,先弹系统确认框「有未保存的修改,确定要关闭吗?」,确认后才关闭(此前因 dirty 判断两侧归一化不一致,弹窗会被永远判为「有修改」,一并修复)。
- **私钥文件选择器看不到无扩展名文件**(如 `id_rsa`):Windows 文件对话框按扩展名过滤时会隐藏无后缀文件,现默认提供「所有文件」选项。

---

## [0.1.0] - 2026-08

初始版本(FinalShell-X 复刻):

- SSH 终端:多标签、密码 / 私钥认证、xterm.js 渲染、标签页保活(切换不重连)、终端尺寸自适应。
- SFTP 文件管理:同屏底部面板、增删改、拖拽上传 / 下载(进度条)、双击远程文件直接编辑(CodeMirror,保存写回)。
- 服务器监控:CPU / 内存 / 磁盘 / 网络实时折线图、负载、运行时间、主机名,2 秒采样,无需服务端插件。
- 效率工具:快捷命令面板、10 套终端配色、深 / 浅主题、字号调节、最近连接、服务器分组。
- 打包:NSIS 安装包,WebView2 依赖自检与故障排查文档。

[0.3.4]: https://github.com/qgx1992/finalshell-clone/compare/v0.3.2...v0.3.4
[0.3.2]: https://github.com/qgx1992/finalshell-clone/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/qgx1992/finalshell-clone/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/qgx1992/finalshell-clone/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/qgx1992/finalshell-clone/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/qgx1992/finalshell-clone/releases/tag/v0.1.0
