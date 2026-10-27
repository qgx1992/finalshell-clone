# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 的格式,
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### 新增
- **SFTP 右键一键打包**:文件列表右键菜单可对文件 / 目录生成 `ZIP` 或 `TAR.GZ`。
  优先在**服务器侧**用 `zip` / `tar` 压缩(文件不出机房,速度最快),服务器缺工具、
  条目过多或所选不在同一目录时自动回落到本地 SFTP 流式打包,并通过 `fallback_reason`
  说明原因。打包过程有独立进度条(服务器端按条目、本地按字节),完成后自动刷新目录并选中产物。
- **文件多选**:`Ctrl/⌘+点击` 增减选中、`Shift+点击` 从锚点连选,工具栏显示「已选 N 项」,
  右键菜单文案随之变成「生成 ZIP 压缩包(N 项)」;右键某行会先把选择收敛到该行,
  避免误打包其他条目。

### 修复
- 远程打包脚本对**以 `-` 开头**的文件名(如 `-rf`)前置 `./`,避免被 `tar` / `zip`
  当成命令行选项。
- zip 分支的进度分母改为**顶层条目数**(原来是递归展开后的文件数),否则一次选中多个
  目录时进度条永远走不到 100%。
- 归档相关的单元测试断言与实现对齐:`FINA-PROGRESS` 百分比按
  `5 + permille*90/10000` 推导(3333 → 34),不再写成不成立的 `5 + 2999`;
  `base_name` 的根目录边界改为钉住"不 panic、不产生含 `/` 的文件名"这条实际约束。

### 依赖
- `zip` 2.6.1 → **9.1.2**。2.6.1 受 3 条公告影响(`RUSTSEC-2026-0367` 解压时保留
  SUID/SGID、`RUSTSEC-2026-0368` 解压符号链接逃逸、`RUSTSEC-2026-0369` 通过符号链接
  施加权限,其中一条 CVSS 9.1)。本程序只用 `ZipWriter` **写**压缩包、不调用
  `extract()`,但仍按"能升就升"直接跨大版本升级;唯一 API 变化是
  `ZipEntry::name()` 返回 `Result<Cow<str>, ZipError>`,单测里补了一次 `unwrap()`。
- `crypto-bigint` 0.7.5 → **0.7.10**、`ctutils` 0.4.2 → **0.4.6**(`RUSTSEC-2026-0387`、
  `RUSTSEC-2026-0385` 均已修复)。随之 `cargo audit` 的漏洞数从 8 条降到 2 条。
- 新增 `src-tauri/.cargo/audit.toml`,把剩下 2 条**上游无修复版本且本程序不可达**的
  公告显式忽略,并在文件里逐条写明不可达理由(`rsa` 的 Marvin Attack 只影响私钥解密
  的计时侧信道,本程序只用 RSA 做 SSH 客户端签名;`crypto-bigint` 0.5.x 遗留项同理)。
  新增公告不在忽略列表内,CI 仍会报警。
- `vite` 6.4.3 → **6.4.5**(patch 升级)。6.4.3 及其以下受 5 条公告影响
  (`GHSA-9jrq-w75r-8gcw`、`GHSA-rq7h-c2jc-7f22`、`GHSA-698v-4r3x-3vcv`、
  `GHSA-r825-w8mm-5pxw`、`GHSA-587h-7wmj-h5pm`,vulnerable `<=6.4.3`),
  6.4.5 已修正,`npm audit --audit-level=high` 归零。未跨到 vite 7(会连带
  `@vitejs/plugin-react` 大版本升级),构建产物文件名逐字节未变。

### 工程化
- 新增端到端冒烟测试 `e2e_test.py`(WebView2 CDP 驱动真实窗口),支持 `--full`
  跑需要 SSH 测试机的完整流程;CI 新增 `e2e` job 自动执行冒烟。
- CI 新增**版本号一致性校验**(`npm run check:versions`),覆盖 `package.json` /
  `Cargo.toml` / `tauri.conf.json`。
- `src/generated/` 补齐 `ArchiveFormat.ts` / `ArchiveInfo.ts`(ts-rs),与 Rust 类型同步。
- `.gitignore` 补上 `*.zip` / `*.tar.gz` / `*.tar.zst` / `*.tmp` / `screenshot-*.png`
  等本地打包与调试产物;清理了 `src-tauri/target`、`src-tauri/target-msvc` 中的
  构建垃圾(约 34.6 GB)与 `__pycache__`。
- `docs/DEV.md` 新增 **§3.4 无 MSVC 的机器用 MinGW(GNU)工具链出包**:
  记录 `--target x86_64-pc-windows-gnu` + MinGW 置顶 `PATH` + `CARGO_TARGET_DIR`
  指向无空格目录这三条硬约束,以及 NSIS 工具链的下载/缓存规则
  (手工预置缓存会被 bundler 判定不完整并重建,网络不通时应设 `HTTPS_PROXY`);
  另补 §4.1「构建完报路径」的校验与静默安装/卸载自检脚本、§5.1 构建产物落位表。

## [0.4.0] - 2026-09-12

### 安全
- **主机密钥校验(TOFU)**:首次连接展示 SHA256 指纹由用户确认,后续连接严格比对;
  指纹变更时弹窗展示新旧指纹并要求显式确认,不再无条件信任服务器密钥。
  信任记录写入应用配置目录的 `known_hosts`(OpenSSH 格式),设置页可查看和清除。
- **凭据移出配置文件**:登录密码、私钥口令、代理密码改存操作系统凭据库
  (Windows 凭据管理器 / macOS Keychain / Linux Secret Service)。
  旧版本明文配置会在启动时自动迁移到凭据库并清空 JSON 字段,迁移前保留 `servers.json.bak`。
- **凭据不再下发前端**:`list_servers` 只返回"是否已保存密码"的布尔标志;
  编辑时通过 保持 / 更新 / 清除 三种意图提交,明文密码不经过前端状态与界面。
- **收紧 Tauri 权限与 CSP**:启用内容安全策略,插件权限由默认全集收敛为实际使用的 dialog 子集。

### 新增
- **SSH 代理支持**:`use_proxy` 不再是预留字段,支持 SOCKS5(含用户名密码)与 HTTP CONNECT;
  代理密码同样存入系统凭据库。
- **跨平台监控**:自动探测远程系统类型,Linux 用 `/proc`、Windows 用 PowerShell、
  macOS 用 `sysctl`/`vm_stat`;不支持的平台给出明确提示而非反复失败。
- **跨平台 RDP 抽象**:Windows 继续使用系统 `mstsc.exe`;Linux/macOS 支持 FreeRDP(`xfreerdp`);
  应用启动时清理遗留的临时 `.rdp` 文件。
- **终端回滚行数设置**:可在设置中调整(1000~50000,默认 5000),用于控制多标签内存占用。
- **结构化错误**:后端命令返回带 `code` 的错误对象,前端据此区分指纹确认、认证失败、网络错误等。
- **日志**:接入 `tracing`,按天写入应用日志目录,且不记录任何凭据。
- **TS 类型自动生成**:Rust 类型通过 `ts-rs` 导出到 `src/generated/`,CI 会校验其是否与后端同步。
- **CI 工作流**:Windows 下执行前端类型检查、后端编译、单元测试、类型一致性校验与前端构建。

### 变更
- **会话生命周期修复**:读循环结束时后端主动移除会话,前端同步清空 `sessionId`;
  断开重连不再残留旧会话引用。
- **终端事件全局路由**:`term-data` / `term-exit` / `sftp-progress` 由每个标签各注册一次
  改为全应用单一监听 + 按会话分发,消除多标签下的事件回调放大。
- **终端空闲优化**:读循环在无数据时不再每 16ms 唤醒一次,仅在收到数据后进入 16ms 合并窗口。
- **监控轮询**:由固定 2 秒 `setInterval` 改为串行轮询 + 指数退避(最长 30 秒),避免请求堆积。
- **SFTP 递归删除**:不再依赖远程 `rm -rf`,改用原生 SFTP 遍历,不跟随符号链接,
  并限制单次最多删除 10 万条,降低误删与命令注入风险。
- **SFTP 原子保存**:编辑器保存改为"临时文件 + 重命名",写入中断不会损坏原文件;
  同时拒绝超过 5 MB 或含 NUL 字节的二进制文件。
- 版本号统一提升至 0.4.0。

### 修复
- 修复断开连接后全局状态仍保留会话 ID、导致后续无法正确清理的问题。
- 修复 SFTP 与监控面板在连接已关闭时仍可能渲染的问题。
- 修复重连按钮可重复触发导致同一标签建立多个会话的问题。

## [0.3.5] - 2026-09-06

### 变更
- **全新应用图标**:蓝→青 45° 渐变圆角方卡 + 白色终端提示符(`>` 与光标下划线),笔画加粗、主体放大,16px 任务栏与浅色背景下依然清楚(旧图标线条过细、蓝绿两色不成体系,32px 以下就糊)。图标由 `scripts/gen-icon.py` 可重复生成,再经 `tauri icon` 展开全平台尺寸。
- **应用内 logo 同步**:顶栏与欢迎页的 `▮▮` 文字标志换成与应用图标同一设计的 SVG(`BrandMark` 组件,渐变 id 用 `useId` 保证多实例唯一)。

### 修复
- **状态栏版本号写死为 v0.1.0**:改为构建期从 `package.json` 注入(`__APP_VERSION__`),不会再与实际发版脉络不一致。

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

[0.3.5]: https://github.com/qgx1992/finalshell-clone/compare/v0.3.4...v0.3.5
[0.3.4]: https://github.com/qgx1992/finalshell-clone/compare/v0.3.2...v0.3.4
[0.3.2]: https://github.com/qgx1992/finalshell-clone/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/qgx1992/finalshell-clone/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/qgx1992/finalshell-clone/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/qgx1992/finalshell-clone/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/qgx1992/finalshell-clone/releases/tag/v0.1.0
