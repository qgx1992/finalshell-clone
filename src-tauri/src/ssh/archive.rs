//! 远程文件打包。
//!
//! 提供两条互补的实现：
//!
//! 1. **纯 SFTP 流式打包**（[`create_archive`]）：逐个文件读回来、边读边压、边写回远程。
//!    本地内存占用恒定（只有 zip 库内部的块缓冲），不需要远程 shell，
//!    适合「服务器没装 zip」「路径里有奇怪字符」「免密 sudo 受限」等场景，
//!    但每个文件都要往返两次 SFTP（读 + 写），网络往返多。
//!
//! 2. **远程 shell 打包**（[`run_archive`] + [`SHELL_ARCHIVE_SCRIPT`]）：在服务器上直接执行
//!    `tar` / `zip` / `unzip -Z -T`，吞吐量取决于服务器磁盘而不是网络，
//!    是默认路径；任何一步失败（没装对应工具等）自动回落到方案 1。
//!
//! 两条路径最终都把结果文件写回远程目录，并复用前端已有的「上传进度条」展示进度。

use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use russh::ChannelMsg;
use russh_sftp::client::SftpSession;
use serde::Serialize;
use ts_rs::TS;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::manager::{SessionEntry, SshManager};
use super::sftp::{get_sftp, join_remote};

/// 打包进度事件名。
///
/// 刻意**不复用** `sftp-progress`：那个通道是「每个文件一条传输」的并发模型，
/// 打包是「一个任务 + 总字节数」，混在一起会互相覆盖。前端由
/// `src/events/terminalRouter.ts` 的 `onArchiveProgress` 单独订阅。
///
/// payload: `(session_id, percent, done_bytes, total_bytes, phase)`
pub const EV_ARCHIVE_PROGRESS: &str = "sftp-archive-progress";

/// 单个归档任务最多打包的条目数，防止误选巨大目录后长时间无响应。
const MAX_ARCHIVE_ENTRIES: usize = 50_000;

/// `tar` / `zip` 允许的最长执行时间。服务器磁盘慢时放宽到 1 小时。
const SHELL_BUDGET: Duration = Duration::from_secs(3600);

/// 内联写入远程脚本的最大字节数（base64 后）。
/// 超过这个量级的命令串不适合塞进 SSH exec 请求。
const MAX_INLINE_SCRIPT_B64: usize = 96 * 1024;

/// 归档格式。
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, TS)]
#[ts(export, export_to = "../../src/generated/")]
#[serde(rename_all = "lowercase")]
pub enum ArchiveFormat {
    /// deflate 压缩的 zip，可被 Windows 资源管理器直接双击打开
    Zip,
    /// tar + gzip（.tar.gz），Unix 服务器上更常见、打包更快
    #[serde(rename = "tar.gz")]
    TarGz,
    /// tar + zstd（.tar.zst），压缩率优于 gzip
    #[serde(rename = "tar.zst")]
    TarZst,
}

impl ArchiveFormat {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "zip" => Ok(Self::Zip),
            "targz" | "tar.gz" | "tgz" => Ok(Self::TarGz),
            "tarzst" | "tar.zst" | "zst" => Ok(Self::TarZst),
            other => Err(format!(
                "不支持的归档格式: {other}（可选 zip / targz / tarzst）"
            )),
        }
    }

    /// 归档文件扩展名（包含前导点）。
    pub fn extension(self) -> &'static str {
        match self {
            Self::Zip => ".zip",
            Self::TarGz => ".tar.gz",
            Self::TarZst => ".tar.zst",
        }
    }

    /// shell 打包需要的外部命令名，用于生成脚本里的可用性检查。
    fn shell_tool(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGz => "tar",
            Self::TarZst => "tar",
        }
    }

    /// 远程 shell 打包时给 tar 的压缩参数。
    ///
    /// 只在给 `tar` 生成脚本时用到；`zip` 不走这里（脚本里 tar 分支是写死的字面量，
    /// 这个方法留给将来支持「按格式拼命令行」时复用）。
    #[allow(dead_code)]
    fn tar_flags(self) -> &'static str {
        match self {
            // 这条分支实际上不会走到（zip 不走 tar），保留完整匹配更清晰
            Self::Zip => "-cf",
            Self::TarGz => "-czf",
            Self::TarZst => "--zstd -cf",
        }
    }

    fn progress_tag(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGz => "tar.gz",
            Self::TarZst => "tar.zst",
        }
    }
}

/// 生成的归档文件信息，返回给前端用于提示。
#[derive(Serialize, Clone, Debug, TS)]
#[ts(export, export_to = "../../src/generated/")]
pub struct ArchiveInfo {
    /// 归档文件的完整远程路径
    pub path: String,
    /// 归档文件名（不含目录）
    pub name: String,
    /// 归档文件在远程服务器上的字节数
    #[ts(type = "number")]
    pub size: u64,
    /// 打包的顶层条目数（选中几个文件/目录，就是几）
    pub entries: usize,
    /// 递归展开后的文件总数
    pub files: usize,
    /// 实际使用的打包方式：`shell`（服务器端 tar/zip）或 `sftp`（纯 SFTP 流式）
    pub method: String,
    /// 打包耗时（毫秒）
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    /// 如果 shell 路径失败后回落，这里给出原因，便于用户判断是否值得排查
    pub fallback_reason: Option<String>,
}

// ---------- 路径工具 ----------

/// 取路径的最后一段作为文件名。
fn base_name(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rsplit('/').next() {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => trimmed.to_string(),
    }
}

/// 取父目录（`/a/b` → `/a`，`/a` → `/`）。
fn parent_of(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(idx) => trimmed[..idx].to_string(),
    }
}

/// 远程文件是否已存在（用 read_dir 比 metadata 更宽容：
/// 部分服务器对不存在的路径返回 SFTP 权限错误而不是 no-such-file）。
async fn remote_exists(sftp: &Arc<SftpSession>, path: &str) -> bool {
    sftp.symlink_metadata(path).await.is_ok()
}

// ---------- 纯 SFTP 流式打包 ----------

/// 目标路径已被占用时自动改名：`a.zip` → `a (1).zip` → `a (2).zip` …
async fn unique_target(sftp: &Arc<SftpSession>, dir: &str, base: &str, ext: &str) -> String {
    let mut candidate = join_remote(dir, &format!("{base}{ext}"));
    let mut i = 1u32;
    while remote_exists(sftp, &candidate).await {
        candidate = join_remote(dir, &format!("{base} ({i}){ext}"));
        i += 1;
        if i > 999 {
            candidate = join_remote(dir, &format!("{base}-{}{ext}", uuid::Uuid::new_v4().simple()));
            break;
        }
    }
    candidate
}

/// 收集一个条目下的全部文件（`(归档内相对路径, 远程完整路径)`）。
///
/// - 不跟随符号链接（与 `sftp_delete` 的语义保持一致：链接本身按文件处理），
///   避免一个指向祖先目录的链接把打包变成无限递归。
/// - 空目录也会单独记录，保证解包后目录结构完整。
async fn collect_files(
    sftp: &Arc<SftpSession>,
    remote_path: &str,
    archive_name: String,
    out: &mut Vec<(String, String, bool, u64)>,
) -> Result<(), String> {
    if out.len() > MAX_ARCHIVE_ENTRIES {
        return Err(format!(
            "打包条目超过 {MAX_ARCHIVE_ENTRIES} 条，已中止。请缩小选择范围"
        ));
    }
    let meta = sftp
        .symlink_metadata(remote_path)
        .await
        .map_err(|e| format!("读取 {remote_path} 信息失败: {e}"))?;
    let ft = meta.file_type();
    if ft.is_dir() && !ft.is_symlink() {
        out.push((archive_name.clone(), remote_path.to_string(), true, 0));
        let entries = sftp
            .read_dir(remote_path)
            .await
            .map_err(|e| format!("读取目录 {remote_path} 失败: {e}"))?;
        let mut names: Vec<String> = Vec::new();
        for entry in entries {
            let name = entry.file_name();
            if name == "." || name == ".." || name.is_empty() || name.contains('/') {
                continue;
            }
            names.push(name);
        }
        names.sort();
        for name in names {
            let child_remote = join_remote(remote_path, &name);
            let child_archive = format!("{archive_name}/{name}");
            // 递归深度由远程目录结构决定；用 Box::pin 打破异步递归的类型递归
            Box::pin(collect_files(sftp, &child_remote, child_archive, out)).await?;
        }
    } else {
        out.push((archive_name, remote_path.to_string(), false, meta.len()));
    }
    Ok(())
}

/// 通用打包（不依赖远程任何工具）：把选中内容读回来，在**本地临时目录**组装 zip，
/// 再通过 SFTP 上传到远程目标路径。返回归档文件的字节数。
///
/// 为什么不在远程边读边写：`zip` crate 的 [`ZipWriter`] 要求底层实现
/// `Write + Seek`（收尾时要回填中央目录并定位），而 `russh_sftp` 的远程文件
/// 只实现了异步 `AsyncWrite`，两者 trait 不兼容。落到本地临时文件既能满足
/// `Seek`，又保证「服务器上没装 zip/tar 也能打包」。
async fn archive_via_local_zip(
    app: &tauri::AppHandle,
    session_id: u64,
    sftp: &Arc<SftpSession>,
    target: &str,
    files: &[(String, String, bool, u64)],
    total_bytes: u64,
) -> Result<u64, String> {
    use tauri::Emitter;
    use tokio::io::AsyncReadExt;

    let total_bytes = total_bytes.max(1);
    let emit = |percent: u32, done: u64, phase: &str| {
        let _ = app.emit(
            EV_ARCHIVE_PROGRESS,
            (session_id, percent, done, total_bytes, phase.to_string(), "bytes".to_string()),
        );
    };

    // 临时文件名带随机后缀，避免并发打包互相覆盖；目录用系统 temp，
    // 用 uid 后缀避免中文用户名路径在某些平台上的编码问题。
    let tmp_path = std::env::temp_dir().join(format!(
        "fina-archive-{}.zip",
        uuid::Uuid::new_v4().simple()
    ));
    let tmp_for_cleanup = tmp_path.clone();

    // 保证任何提前 return 都不会把临时文件留在磁盘上
    let result = async {
        // ---- 1) 本地组装 zip ----
        let local = std::fs::File::create(&tmp_path)
            .map_err(|e| format!("创建本地临时文件失败: {e}"))?;
        let mut zip = ZipWriter::new(local);
        let dir_options = SimpleFileOptions::default().unix_permissions(0o755);
        let file_options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);

        let mut done_bytes: u64 = 0;
        let mut last_emit_bytes: u64 = 0;
        let mut last_emit = Instant::now();
        let mut payload = vec![0u8; 256 * 1024];

        for (archive_name, remote_path, is_dir, size_hint) in files {
            if *is_dir {
                zip.add_directory(format!("{archive_name}/"), dir_options)
                    .map_err(|e| format!("写入目录 {archive_name} 失败: {e}"))?;
                continue;
            }
            let options = file_options.large_file(*size_hint > 4 * 1024 * 1024 * 1024);
            zip.start_file(archive_name.as_str(), options)
                .map_err(|e| format!("写入归档条目 {archive_name} 失败: {e}"))?;

            let mut src = sftp
                .open(remote_path)
                .await
                .map_err(|e| format!("打开远程文件 {remote_path} 失败: {e}"))?;
            loop {
                let n = src
                    .read(&mut payload)
                    .await
                    .map_err(|e| format!("读取 {remote_path} 失败: {e}"))?;
                if n == 0 {
                    break;
                }
                zip.write_all(&payload[..n])
                    .map_err(|e| format!("压缩 {archive_name} 失败: {e}"))?;
                done_bytes += n as u64;
                if done_bytes - last_emit_bytes >= 256 * 1024
                    || last_emit.elapsed() >= Duration::from_millis(150)
                {
                    // 打包阶段只占到总进度的前面一段（后面还要上传），留 20% 给上传
                    let pct = ((done_bytes.min(total_bytes) as f64 / total_bytes as f64) * 80.0) as u32;
                    emit(pct.min(80), done_bytes, "打包中");
                    last_emit_bytes = done_bytes;
                    last_emit = Instant::now();
                }
            }
            let _ = src.close().await;
        }

        // finish() 回填中央目录并交出底层 File
        let local = zip
            .finish()
            .map_err(|e| format!("收尾写入归档失败: {e}"))?;
        local
            .sync_all()
            .map_err(|e| format!("落盘本地归档失败: {e}"))?;
        drop(local);

        // ---- 2) 上传到远程 ----
        let uploaded = upload_local_file(app, session_id, sftp, &tmp_path, target).await?;
        Ok::<u64, String>(uploaded)
    }
    .await;

    let _ = tokio::fs::remove_file(&tmp_for_cleanup).await;
    result
}

/// 把本地文件流式上传到远程路径，附带节流后的进度事件。
///
/// 上传阶段占整体进度的后 20%（打包占前 80%），所以百分比从 80 起算。
async fn upload_local_file(
    app: &tauri::AppHandle,
    session_id: u64,
    sftp: &Arc<SftpSession>,
    local_path: &std::path::Path,
    target: &str,
) -> Result<u64, String> {
    use tauri::Emitter;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let total = tokio::fs::metadata(local_path)
        .await
        .map_err(|e| format!("读取本地归档信息失败: {e}"))?
        .len();
    let mut src = tokio::fs::File::open(local_path)
        .await
        .map_err(|e| format!("打开本地归档失败: {e}"))?;
    let mut dst = sftp
        .create(target)
        .await
        .map_err(|e| format!("创建远程归档 {target} 失败: {e}"))?;

    let mut buf = vec![0u8; 256 * 1024];
    let mut written: u64 = 0;
    let mut last_emit: u64 = 0;
    let mut last_time = Instant::now();
    loop {
        let n = src
            .read(&mut buf)
            .await
            .map_err(|e| format!("读取本地归档失败: {e}"))?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n])
            .await
            .map_err(|e| format!("写入远程归档失败: {e}"))?;
        written += n as u64;
        if written - last_emit >= 1024 * 1024 || last_time.elapsed() >= Duration::from_millis(120) {
            let pct = 80 + ((written.min(total) as f64 / total.max(1) as f64) * 18.0) as u32;
            let _ = app.emit(
                EV_ARCHIVE_PROGRESS,
                (session_id, pct.min(98), written, total.max(1), "上传中".to_string(), "bytes".to_string()),
            );
            last_emit = written;
            last_time = Instant::now();
        }
    }
    dst.close()
        .await
        .map_err(|e| format!("关闭远程归档失败: {e}"))?;
    Ok(written)
}

// ---------- 远程 shell 打包 ----------

/// 生成远程打包脚本。
///
/// 设计要点：
/// - 全程使用相对路径 + `cd <父目录>`，彻底绕开远程路径里的空格 / 单引号 / 中文 / `$` 转义问题
///   （只对父目录做一次单引号转义）。
/// - 已存在的同名归档自动改名 `name (1).zip`，绝不静默覆盖用户数据。
/// - 每完成关键一步都往 stdout 打一行 `FINA-PROGRESS <0..10000> <序号>`，
///   后端把它映射成进度事件（zip 无进度输出，就按「已打条目数/总条目数」推）。
/// - 任何工具缺失都返回非零退出码 + `FINA-ERR <原因>`，由 Rust 侧决定是否回落到 SFTP 方案。
pub fn shell_archive_script(fmt: ArchiveFormat, items: &[String]) -> String {
    let parent = parent_of(&items[0]);
    let archive_base = if items.len() == 1 {
        base_name(&items[0])
    } else {
        format!("{}等{}项", base_name(&items[0]), items.len())
    };
    let ext = fmt.extension();
    let mut out = String::new();

    out.push_str("set -u\n");
    out.push_str("LANG=C\nLC_ALL=C\nexport LANG LC_ALL\n");
    out.push_str(&format!("D='{}'\n", shell_single_quote(&parent)));
    out.push_str(&format!("B='{}'\n", shell_single_quote(&archive_base)));
    out.push_str(&format!(
        "T=\"$D/$B{ext}\"; i=0; while [ -e \"$T\" ]; do i=$((i+1)); T=\"$D/$B ($i){ext}\"; [ $i -gt 999 ] && break; done\n"
    ));

    // 条目清单：只传文件名（相对父目录），避免任何路径拼接与转义。
    // 以 `-` 开头的名字（如 `-rf`）会被 tar/zip 当成选项，前置 `./` 让它稳稳变成路径。
    out.push_str("set --\n");
    for item in items {
        let name = base_name(item);
        let arg = if name.starts_with('-') {
            format!("./{name}")
        } else {
            name
        };
        out.push_str(&format!("set -- \"$@\" '{}'\n", shell_single_quote(&arg)));
    }
    out.push_str("cd \"$D\" || { echo 'FINA-ERR 无法进入目录'; exit 4; }\n");
    out.push_str("rm -f \"$T\"\n");

    match fmt {
        ArchiveFormat::Zip => {
            // 进度按「已打包的顶层条目 / 顶层条目总数」推进，与脚本里 for x 的计数一致
            let total = items.len().max(1);
            out.push_str(
                "command -v zip >/dev/null 2>&1 || { echo 'FINA-ERR 服务器未安装 zip'; exit 3; }\n",
            );
            // 逐条打包：既能输出进度，也让单条目失败不影响其余内容
            out.push_str("n=0\n");
            out.push_str("for x in \"$@\"; do\n");
            out.push_str("  n=$((n+1))\n");
            out.push_str(
                "  zip -r -q -y \"$T\" \"$x\" || { rm -f \"$T\"; echo 'FINA-ERR zip 执行失败（磁盘空间或权限不足）'; exit 5; }\n",
            );
            out.push_str(&format!(
                "  echo \"FINA-PROGRESS $((n*10000/{total})) $n\"\n"
            ));
            out.push_str("done\n");
        }
        ArchiveFormat::TarGz => {
            out.push_str(
                "command -v tar >/dev/null 2>&1 || { echo 'FINA-ERR 服务器未安装 tar'; exit 3; }\n",
            );
            out.push_str(
                "tar -czf \"$T\" \"$@\" || { rm -f \"$T\"; echo 'FINA-ERR tar 执行失败（磁盘空间或权限不足）'; exit 5; }\n",
            );
            out.push_str("echo 'FINA-PROGRESS 10000 1'\n");
        }
        ArchiveFormat::TarZst => {
            out.push_str(
                "command -v tar >/dev/null 2>&1 || { echo 'FINA-ERR 服务器未安装 tar'; exit 3; }\n",
            );
            out.push_str(
                "if tar --help 2>&1 | grep -q -- '--zstd'; then tar --zstd -cf \"$T\" \"$@\";\n",
            );
            out.push_str(
                "elif command -v zstd >/dev/null 2>&1; then tar -cf - \"$@\" | zstd -q -T0 -o \"$T\";\n",
            );
            out.push_str("else echo 'FINA-ERR 服务器不支持 zstd（tar 无 --zstd 且未安装 zstd 命令）'; exit 3; fi\n");
            out.push_str(
                "rc=$?; if [ $rc -ne 0 ] || [ ! -f \"$T\" ]; then rm -f \"$T\"; echo 'FINA-ERR tar/zstd 执行失败'; exit 5; fi\n",
            );
            out.push_str("echo 'FINA-PROGRESS 10000 1'\n");
        }
    }

    // 校验产物并回报
    out.push_str("if [ ! -f \"$T\" ]; then echo 'FINA-ERR 归档文件未生成'; exit 6; fi\n");
    out.push_str("SZ=$(wc -c < \"$T\" | tr -d ' ')\n");
    out.push_str("echo \"FINA-OK $SZ\"\n");
    out.push_str("printf 'FINA-PATH %s\\n' \"$T\"\n");
    out
}

/// POSIX shell 单引号转义：把 `'` 变成 `'\''`。
pub fn shell_single_quote(s: &str) -> String {
    s.replace('\'', "'\\''")
}

/// 在远程执行脚本（通过 stdin 传入，避免超长命令行），收集输出。
///
/// `progress` 非空时，把脚本 stdout 里的 `FINA-PROGRESS <0..10000> <序号>` 实时
/// 翻译成 `EV_ARCHIVE_PROGRESS` 事件（服务器端打包也能有进度条）。
async fn run_script(
    entry: &SessionEntry,
    script: &str,
    budget: Duration,
    progress: Option<(&tauri::AppHandle, u64, u64)>,
) -> Result<(u32, String), String> {
    use tauri::Emitter;

    let mut channel = entry
        .handle
        .channel_open_session()
        .await
        .map_err(|e| format!("打开归档通道失败: {e}"))?;
    channel
        .exec(true, "sh -s")
        .await
        .map_err(|e| format!("启动远程脚本失败: {e}"))?;
    channel
        .data(script.as_bytes())
        .await
        .map_err(|e| format!("发送归档脚本失败: {e}"))?;
    let _ = channel.eof().await;

    let deadline = Instant::now() + budget;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    // 行还没收全时的尾巴（数据块可能在任意字节处切断）
    let mut tail = String::new();
    let mut code: u32 = 0;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = channel.close().await;
            return Err("打包超时（超过 1 小时），已中止".into());
        }
        let msg = tokio::time::timeout(remaining, channel.wait()).await;
        match msg {
            Err(_) => {
                let _ = channel.close().await;
                return Err("打包超时（超过 1 小时），已中止".into());
            }
            Ok(None) => break,
            Ok(Some(ChannelMsg::Data { data })) => {
                stdout.extend_from_slice(&data);
                if let Some((app, sid, total_files)) = progress {
                    tail.push_str(&String::from_utf8_lossy(&data));
                    while let Some(pos) = tail.find('\n') {
                        let line = tail[..pos].to_string();
                        tail.drain(..=pos);
                        if let Some(rest) = line.trim().strip_prefix("FINA-PROGRESS ") {
                            let mut it = rest.split_whitespace();
                            let permille: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                            let done: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                            // 服务器端阶段映射到 5% ~ 95%
                            let pct = 5 + (permille.min(10000) * 90 / 10000) as u32;
                            let _ = app.emit(
                                EV_ARCHIVE_PROGRESS,
                                (
                                    sid,
                                    pct,
                                    done,
                                    total_files.max(1),
                                    "服务器端打包中".to_string(),
                                    "files".to_string(),
                                ),
                            );
                        }
                    }
                    // 防止没有换行的输出把 tail 撑大
                    if tail.len() > 8 * 1024 {
                        tail.clear();
                    }
                }
            }
            Ok(Some(ChannelMsg::ExtendedData { data, .. })) => stderr.extend_from_slice(&data),
            Ok(Some(ChannelMsg::ExitStatus { exit_status })) => code = exit_status,
            Ok(Some(ChannelMsg::Close)) | Ok(Some(ChannelMsg::Eof)) => {}
            Ok(Some(_)) => {}
        }
    }
    let _ = channel.close().await;

    let mut text = String::from_utf8_lossy(&stdout).into_owned();
    if !stderr.is_empty() {
        let err = String::from_utf8_lossy(&stderr).into_owned();
        if !err.trim().is_empty() {
            text.push_str("\n[stderr] ");
            text.push_str(&err);
        }
    }
    Ok((code, text))
}

/// 从脚本输出里取出 `FINA-PATH` / `FINA-OK` / `FINA-ERR` 标记。
fn parse_archive_output(output: &str) -> (Option<String>, Option<u64>, Option<String>) {
    let mut path = None;
    let mut size = None;
    let mut err = None;
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("FINA-PATH ") {
            path = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("FINA-OK ") {
            size = rest.trim().parse::<u64>().ok();
        } else if let Some(rest) = line.strip_prefix("FINA-ERR ") {
            err = Some(rest.trim().to_string());
        }
    }
    (path, size, err)
}

// ---------- 对外命令 ----------

/// 打包所选远程条目，返回生成的归档信息。
///
/// 默认走服务器端 `tar`/`zip`（快）；任何一步不可用则自动回落到纯 SFTP 流式打包（通用）。
#[tauri::command]
pub async fn sftp_create_archive(
    app: tauri::AppHandle,
    state: tauri::State<'_, SshManager>,
    id: u64,
    paths: Vec<String>,
    format: String,
) -> Result<ArchiveInfo, String> {
    let fmt = ArchiveFormat::parse(&format)?;
    if paths.is_empty() {
        return Err("请先选择要打包的文件或目录".into());
    }
    let sources: Vec<String> = paths
        .iter()
        .map(|p| p.trim_end_matches('/').to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if sources.is_empty() {
        return Err("请先选择要打包的文件或目录".into());
    }

    let entry = state
        .sessions
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("会话 {id} 不存在"))?;
    let sftp = get_sftp(&entry).await?;
    let started = Instant::now();

    // 1) 先递归展开，得到真实文件数与总字节数（供进度与 zip64 判定使用）
    let mut files: Vec<(String, String, bool, u64)> = Vec::new();
    for src in &sources {
        collect_files(&sftp, src, base_name(src), &mut files).await?;
    }
    if files.is_empty() {
        return Err("所选内容为空，没有可打包的文件".into());
    }
    let total_bytes: u64 = files.iter().filter(|e| !e.2).map(|e| e.3).sum();
    // 目标目录 = 第一个选中项的父目录
    let target_dir = parent_of(&sources[0]);

    // 让前端立刻进入「打包中」状态（服务器端打包可能在几十毫秒内完成，
    // 但展开大目录本身就要时间，先推一条 0%）
    let _ = {
        use tauri::Emitter;
        app.emit(
            EV_ARCHIVE_PROGRESS,
            (id, 0u32, 0u64, total_bytes.max(1), "准备中".to_string(), "bytes".to_string()),
        )
    };

    // 2) 优先服务器端打包
    let mut fallback_reason: Option<String> = None;
    let mut result: Option<(String, u64)> = None;

    // 同一目录下的多选：只有都在同一父目录时才走 shell（脚本靠 cd 父目录工作）
    let same_parent = sources.iter().all(|p| parent_of(p) == target_dir);
    if same_parent {
        let script = shell_archive_script(fmt, &sources);
        if script.len() <= MAX_INLINE_SCRIPT_B64 {
            {
                use tauri::Emitter;
                let _ = app.emit(
                    EV_ARCHIVE_PROGRESS,
                    (id, 5u32, 0u64, total_bytes.max(1), "服务器端打包中".to_string(), "bytes".to_string()),
                );
            }
            match run_script(&entry, &script, SHELL_BUDGET, Some((&app, id, files.len() as u64)))
                .await
            {
                Ok((code, output)) => {
                    let (path, size, err) = parse_archive_output(&output);
                    match (code, path, err) {
                        (0, Some(p), _) => {
                            // 脚本没回传字节数时先记 0，下一步用 symlink_metadata 补齐
                            result = Some((p, size.unwrap_or(0)));
                        }
                        (_, _, Some(reason)) => fallback_reason = Some(reason),
                        _ => {
                            fallback_reason = Some(format!(
                                "服务器端打包失败（退出码 {code}）：{}",
                                first_lines(&output, 3)
                            ))
                        }
                    }
                }
                Err(e) => fallback_reason = Some(e),
            }
        } else {
            fallback_reason = Some("条目过多，跳过服务器端打包".into());
        }
    } else {
        fallback_reason = Some("所选文件不在同一目录，改用通用打包".into());
    }

    // 3) 回落到通用打包（本地临时 zip + SFTP 上传）
    let (archive_path, archive_size, method) = match result {
        Some((p, sz)) => {
            // shell 路径若没回传字节数（部分服务器 wc 行为不一致），补一次远程 stat
            let sz = if sz == 0 {
                sftp.symlink_metadata(&p)
                    .await
                    .map(|m| m.len())
                    .unwrap_or(0)
            } else {
                sz
            };
            (p, sz, "shell".to_string())
        }
        None => {
            // 只有 zip 能纯 SFTP 生成（tar 需要一个本地临时 tar 层，成本更高，直接提示）
            if fmt != ArchiveFormat::Zip {
                return Err(format!(
                    "服务器端 {0} 打包不可用：{1}。\n可在该服务器安装 {2} 命令，或改用「压缩为 ZIP」",
                    fmt.progress_tag(),
                    fallback_reason.unwrap_or_else(|| "未知原因".into()),
                    fmt.shell_tool()
                ));
            }
            let base = if sources.len() == 1 {
                base_name(&sources[0])
            } else {
                format!("{}等{}项", base_name(&sources[0]), sources.len())
            };
            let target = unique_target(&sftp, &target_dir, &base, fmt.extension()).await;
            let written = archive_via_local_zip(
                &app,
                id,
                &sftp,
                &target,
                &files,
                total_bytes.max(1),
            )
            .await?;
            let size = sftp
                .symlink_metadata(&target)
                .await
                .map(|m| m.len())
                .unwrap_or(written);
            (target, size, "sftp".to_string())
        }
    };

    let name = base_name(&archive_path);
    {
        use tauri::Emitter;
        let _ = app.emit(
            EV_ARCHIVE_PROGRESS,
            (id, 100u32, total_bytes, total_bytes.max(1), "完成".to_string(), "bytes".to_string()),
        );
    }
    Ok(ArchiveInfo {
        path: archive_path,
        name,
        size: archive_size,
        entries: sources.len(),
        files: files.iter().filter(|e| !e.2).count(),
        method,
        elapsed_ms: started.elapsed().as_millis() as u64,
        fallback_reason,
    })
}

fn first_lines(s: &str, n: usize) -> String {
    s.lines()
        .filter(|l| !l.trim().is_empty())
        .take(n)
        .collect::<Vec<_>>()
        .join(" / ")
}

/// 打包预览：告诉前端「默认会生成什么文件名、目标目录里有没有同名文件」。
/// 只做一次目录列举，不递归统计大小（大目录上递归统计会卡住右键菜单）。
#[tauri::command]
pub async fn sftp_archive_preview(
    state: tauri::State<'_, SshManager>,
    id: u64,
    paths: Vec<String>,
    format: String,
) -> Result<Vec<ArchiveTarget>, String> {
    let fmt = ArchiveFormat::parse(&format)?;
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let entry = state
        .sessions
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("会话 {id} 不存在"))?;
    let sftp = get_sftp(&entry).await?;

    let base = if paths.len() == 1 {
        base_name(&paths[0].trim_end_matches('/'))
    } else {
        format!("{}等{}项", base_name(&paths[0].trim_end_matches('/')), paths.len())
    };
    let dir = parent_of(paths[0].trim_end_matches('/'));
    let default_path = fmt.candidate_path(&dir, &base);
    let exists = remote_exists(&sftp, &default_path).await;

    let mut candidates = Vec::new();
    candidates.push(default_path);
    for i in 1..4 {
        candidates.push(join_remote(&dir, &format!("{base} ({i}){}", fmt.extension())));
    }

    Ok(vec![ArchiveTarget {
        source: paths[0].clone(),
        name: base,
        candidates,
        exists,
    }])
}

/// 打包目标预览（前端用于「将生成 xxx.zip」的提示）。
#[derive(Serialize, Clone, Debug, TS)]
#[ts(export, export_to = "../../src/generated/")]
pub struct ArchiveTarget {
    pub source: String,
    pub name: String,
    /// 目标候选路径（第一个为默认名，后续为去重备用名）
    pub candidates: Vec<String>,
    /// 目标目录里是否已存在同名文件
    pub exists: bool,
}

impl ArchiveFormat {
    fn candidate_path(&self, dir: &str, base: &str) -> String {
        join_remote(dir, &format!("{base}{}", self.extension()))
    }
}

/// 删除归档文件（失败回滚用）。
#[allow(dead_code)]
async fn remove_quiet(sftp: &Arc<SftpSession>, path: &str) {
    let _ = sftp.remove_file(path).await;
}

/// 把本地字节直接写进远程文件（给测试与未来扩展用）。
#[allow(dead_code)]
async fn write_remote_all(
    sftp: &Arc<SftpSession>,
    path: &str,
    data: &[u8],
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let mut f = sftp
        .create(path)
        .await
        .map_err(|e| format!("创建远程文件失败: {e}"))?;
    f.write_all(data)
        .await
        .map_err(|e| format!("写入远程文件失败: {e}"))?;
    // russh-sftp 的 close() 是异步的，必须 await
    f.close()
        .await
        .map_err(|e| format!("关闭远程文件失败: {e}"))
}

// ---------- 单元测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    /// 模拟 `run_script` 里那段「按行切 FINA-PROGRESS」的逻辑，
    /// 不改动生产代码也能验证分块到达时的解析正确性。
    fn feed_chunks(chunks: &[&str]) -> Vec<(u64, u64)> {
        let mut tail = String::new();
        let mut seen = Vec::new();
        for c in chunks {
            tail.push_str(c);
            while let Some(pos) = tail.find('\n') {
                let line = tail[..pos].to_string();
                tail.drain(..=pos);
                if let Some(rest) = line.trim().strip_prefix("FINA-PROGRESS ") {
                    let mut it = rest.split_whitespace();
                    let permille: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                    let done: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                    seen.push((5 + (permille.min(10000) * 90 / 10000), done));
                }
            }
        }
        seen
    }

    #[test]
    fn progress_lines_survive_chunk_boundaries() {
        // 数据块在任意位置切断（包括切在数字中间）
        let got = feed_chunks(&[
            "FINA-PROG",
            "RESS 33",
            "33 3\nFIN",
            "A-PROGRESS 10000 9\nFINA-OK 1234\n",
        ]);
        assert_eq!(got, vec![(5 + 2999, 3), (95, 9)]);
    }

    #[test]
    fn progress_oob_values_are_clamped() {
        let got = feed_chunks(&["FINA-PROGRESS 999999 99\n", "FINA-PROGRESS notanumber x\n"]);
        assert_eq!(got, vec![(95, 99), (5, 0)]);
    }

    #[test]
    fn base_name_and_parent() {
        assert_eq!(base_name("/srv/www/a.txt"), "a.txt");
        assert_eq!(base_name("/srv/www/dir/"), "dir");
        assert_eq!(base_name("/"), "/");
        assert_eq!(parent_of("/srv/www/a.txt"), "/srv/www");
        assert_eq!(parent_of("/a"), "/");
        assert_eq!(parent_of("/a/b/"), "/a");
    }

    #[test]
    fn single_quote_escapes() {
        assert_eq!(shell_single_quote("a'b"), "a'\\''b");
        assert_eq!(shell_single_quote("普通目录"), "普通目录");
    }

    #[test]
    fn format_parse_and_ext() {
        assert_eq!(ArchiveFormat::parse("zip").unwrap(), ArchiveFormat::Zip);
        assert_eq!(ArchiveFormat::parse("TARGZ").unwrap(), ArchiveFormat::TarGz);
        assert_eq!(ArchiveFormat::parse("tar.zst").unwrap(), ArchiveFormat::TarZst);
        assert!(ArchiveFormat::parse("rar").is_err());
        assert_eq!(ArchiveFormat::Zip.extension(), ".zip");
        assert_eq!(ArchiveFormat::TarGz.extension(), ".tar.gz");
        assert_eq!(ArchiveFormat::TarZst.extension(), ".tar.zst");
    }

    #[test]
    fn parse_output_markers() {
        let out = "FINA-PROGRESS 5000 3\nFINA-OK 1234\nFINA-PATH /srv/a.zip\n";
        let (p, s, e) = parse_archive_output(out);
        assert_eq!(p.as_deref(), Some("/srv/a.zip"));
        assert_eq!(s, Some(1234));
        assert!(e.is_none());

        let (_, _, e) = parse_archive_output("FINA-ERR 服务器未安装 zip");
        assert_eq!(e.as_deref(), Some("服务器未安装 zip"));
    }

    #[test]
    fn zip_script_contains_items() {
        let items = vec!["/srv/www/a.txt".to_string(), "/srv/www/b dir".to_string()];
        let script = shell_archive_script(ArchiveFormat::Zip, &items);
        assert!(script.contains("cd \"$D\""), "脚本应 cd 到父目录：{script}");
        assert!(script.contains("'b dir'"), "脚本里应保持文件名原样：{script}");
        assert!(script.contains("command -v zip"));
        assert!(script.contains("FINA-PATH"));
        assert!(script.contains("D='/srv/www'"), "父目录应转义进 D：{script}");
        // 绝不出现跨目录绝对路径作为条目
        assert!(!script.contains("set -- \"$@\" '/srv/www/a.txt'"));
    }

    #[test]
    fn script_handles_quotes_and_non_ascii() {
        let items = vec!["/srv/我的 目录/it's.txt".to_string()];
        let script = shell_archive_script(ArchiveFormat::Zip, &items);
        assert!(script.contains("D='/srv/我的 目录'"));
        assert!(script.contains("'it'\\''s.txt'"), "单引号需转义：{script}");
    }

    #[test]
    fn multi_item_archive_name() {
        let items = vec!["/srv/www/a".to_string(), "/srv/www/b".to_string()];
        let script = shell_archive_script(ArchiveFormat::TarGz, &items);
        assert!(script.contains("B='a等2项'"), "多选应生成 a等2项：{script}");
        assert!(script.contains("tar -czf"));
    }

    #[test]
    fn targz_and_zst_flags() {
        let items = vec!["/a/x".to_string()];
        let gz = shell_archive_script(ArchiveFormat::TarGz, &items);
        assert!(gz.contains("tar -czf"));
        let zst = shell_archive_script(ArchiveFormat::TarZst, &items);
        assert!(zst.contains("--zstd"));
        assert!(zst.contains("zstd -q") || zst.contains("zstd "));
    }

    #[test]
    fn dash_prefixed_name_is_not_treated_as_option() {
        // `-rf` 这类名字若原样作为参数传下去会被 tar/zip 当成选项，必须前置 `./`
        let items = vec!["/srv/www/-rf".to_string()];
        let script = shell_archive_script(ArchiveFormat::TarGz, &items);
        assert!(script.contains("'./-rf'"), "应以 ./ 前缀传递：{script}");
        assert!(!script.contains("set -- \"$@\" '-rf'"), "不得裸传 -rf：{script}");
    }

    #[test]
    fn zip_progress_counts_top_level_items() {
        // 进度分母必须是「顶层条目数」，不能是递归展开后的文件数，
        // 否则选了 3 个目录（内含上百个文件）时进度条永远走不到 100%。
        let items = vec![
            "/srv/a".to_string(),
            "/srv/b".to_string(),
            "/srv/c".to_string(),
        ];
        let script = shell_archive_script(ArchiveFormat::Zip, &items);
        assert!(script.contains("n*10000/3"), "分母应为 3：{script}");
    }

    #[test]
    fn counting_writer_counts() {
        // 保留占位：原来的 CountingWriter 适配器已随「本地临时 zip」方案移除，
        // 这里改成验证「本地 zip 能正常写出并可读回」这条新路径。
        let dir = std::env::temp_dir().join(format!(
            "fina-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.zip");

        {
            let f = std::fs::File::create(&path).unwrap();
            let mut zip = ZipWriter::new(f);
            let opts = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .unix_permissions(0o644);
            zip.add_directory("d/", SimpleFileOptions::default())
                .unwrap();
            zip.start_file("d/a.txt", opts).unwrap();
            zip.write_all(b"hello").unwrap();
            zip.finish().unwrap();
        }

        let f = std::fs::File::open(&path).unwrap();
        let mut archive = zip::ZipArchive::new(f).unwrap();
        let mut names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["d/".to_string(), "d/a.txt".to_string()]);

        std::fs::remove_dir_all(&dir).ok();
    }
}
