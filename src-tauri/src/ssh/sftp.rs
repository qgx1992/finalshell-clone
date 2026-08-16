use base64::{engine::general_purpose::STANDARD as B64, Engine};
use russh_sftp::client::SftpSession;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

use super::manager::{run_command, SessionEntry, SshManager};

pub const EV_SFTP_PROGRESS: &str = "sftp-progress";

#[derive(Serialize, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
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
    // 用 SFTP 元数据判断类型
    let meta = sftp
        .metadata(&path)
        .await
        .map_err(|e| format!("获取元数据失败: {e}"))?;
    if meta.file_type().is_dir() {
        // 递归删除目录:shell 方式(可靠且快),路径经引号转义
        let escaped = path.replace('\'', "'\\''");
        run_command(
            &entry,
            &format!("rm -rf -- '{escaped}'"),
            Duration::from_secs(30),
        )
        .await?;
    } else {
        sftp.remove_file(&path)
            .await
            .map_err(|e| format!("删除文件失败: {e}"))?;
    }
    Ok(())
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
    let data = sftp
        .read(&remote_path)
        .await
        .map_err(|e| format!("读取文件失败: {e}"))?;
    Ok(B64.encode(&data))
}

/// 写入远程文件内容(内置编辑器用)
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
    // 用 create(带 TRUNCATE)整体覆盖,避免残留旧内容
    let mut file = sftp
        .create(&remote_path)
        .await
        .map_err(|e| format!("打开远程文件失败: {e}"))?;
    tokio::io::AsyncWriteExt::write_all(&mut file, &data)
        .await
        .map_err(|e| format!("写入文件失败: {e}"))?;
    file.close()
        .await
        .map_err(|e| format!("关闭文件失败: {e}"))?;
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
