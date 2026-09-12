use base64::{engine::general_purpose::STANDARD as B64, Engine};
use russh_sftp::client::SftpSession;
use serde::Serialize;
use ts_rs::TS;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

use super::manager::{SessionEntry, SshManager};

pub const EV_SFTP_PROGRESS: &str = "sftp-progress";

#[derive(Serialize, Clone, TS)]
#[ts(export, export_to = "../../src/generated/")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number")]
    pub modified: u64,
    pub permissions: String,
}

/// 惰性获取(或创建)SFTP 会话
pub async fn get_sftp(entry: &SessionEntry) -> Result<Arc<SftpSession>, String> {
    let mut slot = entry.sftp.lock().await;
    if let Some(s) = slot.as_ref() {
        return Ok(s.clone());
    }
    // 打开 sftp 子系统通道
    let channel = entry
        .handle
        .channel_open_session()
        .await
        .map_err(|e| format!("打开 SFTP 通道失败: {e}"))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("请求 SFTP 子系统失败: {e}"))?;
    let stream = channel.into_stream();
    let sftp = SftpSession::new(stream)
        .await
        .map_err(|e| format!("SFTP 初始化失败: {e}"))?;
    let arc = Arc::new(sftp);
    *slot = Some(arc.clone());
    Ok(arc)
}

pub fn to_file_entry(parent: &str, name: &str, meta: &russh_sftp::protocol::FileAttributes) -> FileEntry {
    let ft = meta.file_type();
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mode = meta
        .permissions()
        .to_string();
    let path = if parent.ends_with('/') || parent.is_empty() {
        format!("{parent}{name}")
    } else {
        format!("{parent}/{name}")
    };
    FileEntry {
        name: name.to_string(),
        path,
        is_dir: ft.is_dir(),
        is_symlink: ft.is_symlink(),
        size: meta.len(),
        modified,
        permissions: mode,
    }
}

// ---------- Tauri commands ----------

/// 列出远程目录
#[tauri::command]
pub async fn list_dir(
    state: State<'_, SshManager>,
    id: u64,
    path: String,
) -> Result<Vec<FileEntry>, String> {
    let entry = state
        .sessions
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("会话 {id} 不存在"))?;
    let sftp = get_sftp(&entry).await?;
    let read_dir = sftp
        .read_dir(&path)
        .await
        .map_err(|e| format!("读取目录失败: {e}"))?;

    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for de in read_dir {
        let name = de.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let meta = de.metadata();
        let fe = to_file_entry(&path, &name, &meta);
        if fe.is_dir {
            dirs.push(fe);
        } else {
            files.push(fe);
        }
    }
    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    files.sort_by(|a, b| a.name.cmp(&b.name));
    dirs.extend(files);
    Ok(dirs)
}

/// 创建目录
#[tauri::command]
pub async fn sftp_mkdir(
    state: State<'_, SshManager>,
    id: u64,
    path: String,
) -> Result<(), String> {
    let entry = state
        .sessions
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("会话 {id} 不存在"))?;
    let sftp = get_sftp(&entry).await?;
    sftp.create_dir(&path)
        .await
        .map_err(|e| format!("创建目录失败: {e}"))
}

/// 删除(目录递归删除)
#[tauri::command]
pub async fn sftp_delete(
    state: State<'_, SshManager>,
    id: u64,
    path: String,
) -> Result<(), String> {
    let entry = state
        .sessions
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("会话 {id} 不存在"))?;
    let sftp = get_sftp(&entry).await?;
    // 用 symlink_metadata 判断类型:符号链接本身按文件删除,绝不跟随,
    // 避免 rm -rf 语义下把链接指向的目录内容一并删掉。
    let meta = sftp
        .symlink_metadata(&path)
        .await
        .map_err(|e| format!("获取元数据失败: {e}"))?;
    if meta.file_type().is_dir() {
        delete_dir_recursive(&sftp, &path).await?;
    } else {
        sftp.remove_file(&path)
            .await
            .map_err(|e| format!("删除文件失败: {e}"))?;
    }
    Ok(())
}

/// 原生递归删除目录。
///
/// 与旧的 `rm -rf` 方案相比:
/// - 不依赖远程 shell 与路径引号转义
/// - 使用 symlink_metadata,不跟随符号链接
/// - 限制最大删除条目数,避免误选巨大目录时长时间阻塞
async fn delete_dir_recursive(
    sftp: &Arc<SftpSession>,
    path: &str,
) -> Result<(), String> {
    const MAX_ENTRIES: usize = 100_000;
    let mut stack = vec![path.to_string()];
    let mut removed = 0usize;

    while let Some(dir) = stack.pop() {
        let entries = sftp
            .read_dir(&dir)
            .await
            .map_err(|e| format!("读取目录 {dir} 失败: {e}"))?;
        let mut children: Vec<String> = Vec::new();
        let mut subdirs: Vec<String> = Vec::new();
        for entry in entries {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let child = join_remote(&dir, &name);
            removed += 1;
            if removed > MAX_ENTRIES {
                return Err(format!("目录条目超过 {MAX_ENTRIES} 条,已中止删除"));
            }
            let ft = entry.file_type();
            if ft.is_dir() && !ft.is_symlink() {
                subdirs.push(child);
            } else {
                children.push(child);
            }
        }
        for child in children {
            sftp.remove_file(&child)
                .await
                .map_err(|e| format!("删除文件 {child} 失败: {e}"))?;
        }
        // 先删除子目录内容,再删除自身
        for sub in subdirs {
            stack.push(sub);
        }
        sftp.remove_dir(&dir)
            .await
            .map_err(|e| format!("删除目录 {dir} 失败: {e}"))?;
    }
    Ok(())
}

/// 拼接远程路径,避免出现双斜杠
fn join_remote(parent: &str, name: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{name}")
    } else {
        format!("{parent}/{name}")
    }
}

/// 重命名/移动
#[tauri::command]
pub async fn sftp_rename(
    state: State<'_, SshManager>,
    id: u64,
    old_path: String,
    new_path: String,
) -> Result<(), String> {
    let entry = state
        .sessions
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("会话 {id} 不存在"))?;
    let sftp = get_sftp(&entry).await?;
    sftp.rename(&old_path, &new_path)
        .await
        .map_err(|e| format!("重命名失败: {e}"))
}

/// 上传文件(本地 -> 远程)
#[tauri::command]
pub async fn sftp_upload(
    app: AppHandle,
    state: State<'_, SshManager>,
    id: u64,
    local_path: String,
    remote_path: String,
) -> Result<(), String> {
    let entry = state
        .sessions
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("会话 {id} 不存在"))?;
    let sftp = get_sftp(&entry).await?;

    let local = PathBuf::from(&local_path);
    let total = tokio::fs::metadata(&local)
        .await
        .map_err(|e| format!("读取本地文件失败: {e}"))?
        .len();
    let mut src = tokio::fs::File::open(&local)
        .await
        .map_err(|e| format!("打开本地文件失败: {e}"))?;
    let mut dst = sftp
        .create(&remote_path)
        .await
        .map_err(|e| format!("创建远程文件失败: {e}"))?;

    let mut buf = vec![0u8; 256 * 1024];
    let mut written: u64 = 0;
    // 进度事件节流:每 1MB 或每 100ms 才发一次,避免大文件产生海量 IPC 事件
    let mut last_emit: u64 = 0;
    let mut last_time = std::time::Instant::now();
    loop {
        let n = tokio::io::AsyncReadExt::read(&mut src, &mut buf)
            .await
            .map_err(|e| format!("读取本地文件失败: {e}"))?;
        if n == 0 {
            break;
        }
        tokio::io::AsyncWriteExt::write_all(&mut dst, &buf[..n])
            .await
            .map_err(|e| format!("写入远程文件失败: {e}"))?;
        written += n as u64;
        if written - last_emit >= 1024 * 1024 || last_time.elapsed() >= Duration::from_millis(100)
        {
            let _ = app.emit(
                EV_SFTP_PROGRESS,
                (id, "upload", &local_path, &remote_path, written, total),
            );
            last_emit = written;
            last_time = std::time::Instant::now();
        }
    }
    // 结束时补发一次,保证 100% 状态可见
    let _ = app.emit(
        EV_SFTP_PROGRESS,
        (id, "upload", &local_path, &remote_path, written, total),
    );
    let _ = dst.close().await;
    Ok(())
}

/// 下载文件(远程 -> 本地)
#[tauri::command]
pub async fn sftp_download(
    app: AppHandle,
    state: State<'_, SshManager>,
    id: u64,
    remote_path: String,
    local_path: String,
) -> Result<(), String> {
    let entry = state
        .sessions
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("会话 {id} 不存在"))?;
    let sftp = get_sftp(&entry).await?;

    let mut src = sftp
        .open(&remote_path)
        .await
        .map_err(|e| format!("打开远程文件失败: {e}"))?;
    let total = src.metadata().await.map(|m| m.len()).unwrap_or(0);
    let mut dst = tokio::fs::File::create(&local_path)
        .await
        .map_err(|e| format!("创建本地文件失败: {e}"))?;

    let mut buf = vec![0u8; 256 * 1024];
    let mut written: u64 = 0;
    // 进度事件节流:每 1MB 或每 100ms 才发一次
    let mut last_emit: u64 = 0;
    let mut last_time = std::time::Instant::now();
    loop {
        let n = tokio::io::AsyncReadExt::read(&mut src, &mut buf)
            .await
            .map_err(|e| format!("读取远程文件失败: {e}"))?;
        if n == 0 {
            break;
        }
        tokio::io::AsyncWriteExt::write_all(&mut dst, &buf[..n])
            .await
            .map_err(|e| format!("写入本地文件失败: {e}"))?;
        written += n as u64;
        if written - last_emit >= 1024 * 1024 || last_time.elapsed() >= Duration::from_millis(100)
        {
            let _ = app.emit(
                EV_SFTP_PROGRESS,
                (id, "download", &remote_path, &local_path, written, total),
            );
            last_emit = written;
            last_time = std::time::Instant::now();
        }
    }
    // 结束时补发一次,保证 100% 状态可见
    let _ = app.emit(
        EV_SFTP_PROGRESS,
        (id, "download", &remote_path, &local_path, written, total),
    );
    let _ = tokio::io::AsyncWriteExt::flush(&mut dst).await;
    Ok(())
}

/// 读取远程文件内容(内置编辑器用)
#[tauri::command]
pub async fn sftp_read_file(
    state: State<'_, SshManager>,
    id: u64,
    remote_path: String,
) -> Result<String, String> {
    let entry = state
        .sessions
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("会话 {id} 不存在"))?;
    let sftp = get_sftp(&entry).await?;

    // 内置编辑器是文本编辑器:超过限制的文件直接拒绝,避免前端内存暴涨
    const MAX_EDIT_BYTES: u64 = 5 * 1024 * 1024;
    let meta = sftp
        .metadata(&remote_path)
        .await
        .map_err(|e| format!("读取文件信息失败: {e}"))?;
    if meta.len() > MAX_EDIT_BYTES {
        return Err(format!(
            "文件大小 {:.1} MB 超过编辑器上限 5 MB,请下载到本地后用专业工具编辑",
            meta.len() as f64 / (1024.0 * 1024.0)
        ));
    }

    let data = sftp
        .read(&remote_path)
        .await
        .map_err(|e| format!("读取文件失败: {e}"))?;

    // 含 NUL 字节判定为二进制文件,编辑器打开会损坏内容
    if data.iter().take(8192).any(|b| *b == 0) {
        return Err("该文件看起来是二进制文件,无法用文本编辑器打开".into());
    }

    Ok(B64.encode(&data))
}

/// 写入远程文件内容(内置编辑器用)。
///
/// 采用"临时文件 + 重命名"的原子写入:写入中途失败时原文件保持完整,
/// 不会出现被截断成半个文件的情况。
#[tauri::command]
pub async fn sftp_write_file(
    state: State<'_, SshManager>,
    id: u64,
    remote_path: String,
    content_b64: String,
) -> Result<(), String> {
    let entry = state
        .sessions
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("会话 {id} 不存在"))?;
    let sftp = get_sftp(&entry).await?;
    let data = B64
        .decode(&content_b64)
        .map_err(|e| format!("内容解码失败: {e}"))?;

    // 临时文件放在同目录,保证 rename 不跨文件系统
    let tmp_path = format!("{remote_path}.fina-tmp-{}", uuid::Uuid::new_v4().simple());

    let mut file = match sftp.create(&tmp_path).await {
        Ok(f) => f,
        Err(e) => return Err(format!("创建临时文件失败: {e}")),
    };
    if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut file, &data).await {
        let _ = file.close().await;
        let _ = sftp.remove_file(&tmp_path).await;
        return Err(format!("写入临时文件失败: {e}"));
    }
    if let Err(e) = file.close().await {
        let _ = sftp.remove_file(&tmp_path).await;
        return Err(format!("关闭临时文件失败: {e}"));
    }

    // 覆盖前先删除目标(部分 SFTP 服务器 rename 不覆盖已存在文件)
    if sftp.metadata(&remote_path).await.is_ok() {
        sftp.remove_file(&remote_path)
            .await
            .map_err(|e| format!("替换原文件失败: {e}"))?;
    }
    if let Err(e) = sftp.rename(&tmp_path, &remote_path).await {
        let _ = sftp.remove_file(&tmp_path).await;
        return Err(format!("重命名临时文件失败: {e}"));
    }
    Ok(())
}

/// 获取当前目录(用于终端 cd 同步 SFTP 面板)
#[tauri::command]
pub async fn sftp_pwd(state: State<'_, SshManager>, id: u64) -> Result<String, String> {
    let entry = state
        .sessions
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("会话 {id} 不存在"))?;
    let sftp = get_sftp(&entry).await?;
    sftp.canonicalize(".")
        .await
        .map_err(|e| format!("获取当前目录失败: {e}"))
}
