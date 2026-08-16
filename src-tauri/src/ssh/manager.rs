use base64::{engine::general_purpose::STANDARD as B64, Engine};
use russh::client::{self, Handle};
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh::ChannelMsg;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;

use crate::store::{ServerConfig, Store};

// ---------- 事件名 ----------
pub const EV_TERM_DATA: &str = "term-data";
pub const EV_TERM_EXIT: &str = "term-exit";

/// russh 客户端 Handler:直接信任服务器密钥(第一版)
#[derive(Clone, Default)]
pub struct SshHandler;

impl client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub enum SessionStatus {
    Connected,
    Closed,
}

#[derive(Serialize, Clone)]
pub struct SessionInfo {
    pub id: u64,
    pub server_id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub status: String,
    pub connected_at: u64,
}

pub struct SessionEntry {
    pub id: u64,
    pub server: ServerConfig,
    pub handle: Handle<SshHandler>,
    /// 终端读半(读循环独占,等待数据)
    pub term_reader: Arc<Mutex<russh::ChannelReadHalf>>,
    /// 终端写半(发送输入/调整尺寸,全部 &self 无锁)
    pub term_writer: Arc<russh::ChannelWriteHalf<russh::client::Msg>>,
    pub sftp: Mutex<Option<Arc<russh_sftp::client::SftpSession>>>,
    pub status: tokio::sync::Mutex<SessionStatus>,
    pub connected_at: u64,
    /// 监控速率计算用的上次采样
    pub last_cpu: Mutex<Option<(u64, u64)>>,   // (idle, total)
    pub last_net: Mutex<Option<(u64, u64)>>,   // (rx, tx)
    pub last_net_time: Mutex<Option<u64>>,
}

#[derive(Default)]
pub struct SshManager {
    pub sessions: Arc<Mutex<HashMap<u64, Arc<SessionEntry>>>>,
    pub next_id: AtomicU64,
}

impl SshManager {
    pub fn new() -> Self {
        Self::default()
    }

    async fn get_entry(&self, id: u64) -> Option<Arc<SessionEntry>> {
        self.sessions.lock().await.get(&id).cloned()
    }
}

// ---------- 辅助:执行远程命令并收集全部输出 ----------
/// 打开一条新的 exec 通道
async fn open_exec_channel(
    entry: &SessionEntry,
) -> Result<russh::Channel<russh::client::Msg>, String> {
    entry
        .handle
        .channel_open_session()
        .await
        .map_err(|e| format!("打开命令通道失败: {e}"))
}

/// 在已有通道上执行一次命令并收集输出(不负责通道复用)
async fn exec_on_channel(
    channel: &mut russh::Channel<russh::client::Msg>,
    command: &str,
    timeout: std::time::Duration,
) -> Result<String, String> {
    channel
        .exec(true, command)
        .await
        .map_err(|e| format!("执行命令失败: {e}"))?;

    let mut output = Vec::new();
    let mut stderr = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let msg = tokio::time::timeout(remaining, channel.wait())
            .await
            .map_err(|_| {
                let _ = channel.close();
                format!("命令执行超时: {command}")
            })?;
        match msg {
            Some(ChannelMsg::Data { data }) => output.extend_from_slice(&data),
            Some(ChannelMsg::ExtendedData { data, .. }) => stderr.extend_from_slice(&data),
            Some(ChannelMsg::ExitStatus { .. }) | Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) => {
                break;
            }
            Some(_) => {}
            None => break,
        }
    }
    let _ = channel.close().await;

    let mut text = String::from_utf8_lossy(&output).into_owned();
    if !stderr.is_empty() {
        let err = String::from_utf8_lossy(&stderr).into_owned();
        if !err.trim().is_empty() {
            text.push_str("\n[stderr] ");
            text.push_str(&err);
        }
    }
    Ok(text)
}

/// 执行远程命令:每次打开独立通道(SSH 协议中一个 channel 只能 exec 一次,无法复用)
pub async fn run_command(
    entry: &SessionEntry,
    command: &str,
    timeout: std::time::Duration,
) -> Result<String, String> {
    let mut channel = open_exec_channel(entry).await?;
    let result = exec_on_channel(&mut channel, command, timeout).await;
    let _ = channel.close().await;
    result
}

// ---------- Tauri commands ----------

/// 建立 SSH 连接,返回会话 id
#[tauri::command]
pub async fn connect_server(
    app: AppHandle,
    state: State<'_, SshManager>,
    server: ServerConfig,
) -> Result<u64, String> {
    let id = state.next_id.fetch_add(1, Ordering::SeqCst);
    let addr = format!("{}:{}", server.host, server.port);
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let config = client::Config::default();
    let mut handle = client::connect(Arc::new(config), &*addr, SshHandler)
        .await
        .map_err(|e| format!("连接 {addr} 失败: {e}"))?;

    // 认证
    let auth_ok = match server.auth_type.as_str() {
        "key" => {
            let key_path = server
                .private_key
                .clone()
                .ok_or_else(|| "未指定私钥路径".to_string())?;
            let pass = server.key_passphrase.clone();
            let key = load_secret_key(&key_path, pass.as_deref())
                .map_err(|e| format!("加载私钥失败: {e}"))?;
            let kh = PrivateKeyWithHashAlg::new(Arc::new(key), None);
            handle
                .authenticate_publickey(&server.username, kh)
                .await
                .map_err(|e| format!("密钥认证失败: {e}"))?
        }
        _ => {
            let pass = server
                .password
                .clone()
                .ok_or_else(|| "未设置密码".to_string())?;
            handle
                .authenticate_password(&server.username, &pass)
                .await
                .map_err(|e| format!("密码认证失败: {e}"))?
        }
    };
    if !matches!(auth_ok, russh::client::AuthResult::Success) {
        return Err("认证失败:用户名或密码/密钥不正确".into());
    }

    // 打开终端通道
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| format!("打开会话通道失败: {e}"))?;
    channel
        .request_pty(true, "xterm-256color", 120, 32, 0, 0, &[])
        .await
        .map_err(|e| format!("请求 PTY 失败: {e}"))?;
    channel
        .request_shell(true)
        .await
        .map_err(|e| format!("启动 shell 失败: {e}"))?;
    // 拆分读写半:读循环与写入互不阻塞,消除输入卡顿
    let (reader, writer) = channel.split();

    let entry = Arc::new(SessionEntry {
        id,
        server: server.clone(),
        handle,
        term_reader: Arc::new(Mutex::new(reader)),
        term_writer: Arc::new(writer),
        sftp: Mutex::new(None),
        status: tokio::sync::Mutex::new(SessionStatus::Connected),
        connected_at: started,
        last_cpu: Mutex::new(None),
        last_net: Mutex::new(None),
        last_net_time: Mutex::new(None),
    });

    // 存进 manager
    state.sessions.lock().await.insert(id, entry.clone());

    // 记录最近连接
    let store = Store::new(&app);
    let mut settings = store.load_settings();
    settings.last_servers.retain(|s| s != &server.id);
    settings.last_servers.insert(0, server.id.clone());
    if settings.last_servers.len() > 20 {
        settings.last_servers.truncate(20);
    }
    let _ = store.save_settings(&settings);

    // 广播连接事件
    let _ = app.emit(EV_TERM_EXIT, (id, "connected".to_string(), String::new()));

    // 启动终端读循环:数据批量合并后发送(最多 16ms 或 32KB 一次),大幅降低 IPC 事件数
    let app2 = app.clone();
    let entry2 = entry.clone();
    let sessions = state.sessions.clone();
    tokio::spawn(async move {
        let result: Result<(), String> = async {
            const FLUSH_BYTES: usize = 32 * 1024;
            let mut pending: Vec<u8> = Vec::new();
            loop {
                // 用 16ms 超时轮询:既有数据时等待,空闲时充当 flush 定时器
                let (timed_out, msg) = {
                    let mut reader = entry2.term_reader.lock().await;
                    match tokio::time::timeout(
                        std::time::Duration::from_millis(16),
                        reader.wait(),
                    )
                    .await
                    {
                        Ok(m) => (false, m),
                        Err(_) => (true, None),
                    }
                };
                match msg {
                    Some(ChannelMsg::Data { data }) => pending.extend_from_slice(&data),
                    Some(ChannelMsg::ExtendedData { data, .. }) => pending.extend_from_slice(&data),
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) => break,
                    Some(_) => {}
                    None => {
                        if !timed_out {
                            break; // 通道已关闭
                        }
                    }
                }
                if !pending.is_empty() && (pending.len() >= FLUSH_BYTES || timed_out) {
                    let _ = app2.emit(EV_TERM_DATA, (id, B64.encode(&pending), "out".to_string()));
                    pending.clear();
                }
            }
            // 退出前补发剩余数据
            if !pending.is_empty() {
                let _ = app2.emit(EV_TERM_DATA, (id, B64.encode(&pending), "out".to_string()));
            }
            Ok(())
        }
        .await;
        // 更新状态并通知前端
        if let Some(e) = sessions.lock().await.get(&id) {
            *e.status.lock().await = SessionStatus::Closed;
        }
        let reason = match result {
            Ok(_) => "连接已关闭".to_string(),
            Err(e) => e,
        };
        let _ = app2.emit(EV_TERM_EXIT, (id, "closed".to_string(), reason));
    });

    Ok(id)
}

/// 断开连接
#[tauri::command]
pub async fn disconnect(app: AppHandle, state: State<'_, SshManager>, id: u64) -> Result<(), String> {
    if let Some(entry) = state.sessions.lock().await.remove(&id) {
        {
            let _ = entry.term_writer.close().await;
        }
        *entry.status.lock().await = SessionStatus::Closed;
        let _ = app.emit(EV_TERM_EXIT, (id, "closed".to_string(), "用户主动断开".to_string()));
    }
    Ok(())
}

/// 向终端写入数据(前端键盘输入)
#[tauri::command]
pub async fn write_terminal(
    state: State<'_, SshManager>,
    id: u64,
    data_b64: String,
) -> Result<(), String> {
    let entry = state
        .get_entry(id)
        .await
        .ok_or_else(|| format!("会话 {id} 不存在"))?;
    let bytes = B64
        .decode(&data_b64)
        .map_err(|e| format!("数据解码失败: {e}"))?;
    entry
        .term_writer
        .data_bytes(bytes)
        .await
        .map_err(|e| format!("写入失败: {e}"))
}

/// 调整终端尺寸
#[tauri::command]
pub async fn resize_terminal(
    state: State<'_, SshManager>,
    id: u64,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let entry = state
        .get_entry(id)
        .await
        .ok_or_else(|| format!("会话 {id} 不存在"))?;
    entry
        .term_writer
        .window_change(cols, rows, 0, 0)
        .await
        .map_err(|e| format!("调整尺寸失败: {e}"))
}

/// 执行一条远程命令(监控/快捷命令),返回文本输出
#[tauri::command]
pub async fn exec_command(
    state: State<'_, SshManager>,
    id: u64,
    command: String,
) -> Result<String, String> {
    let entry = state
        .get_entry(id)
        .await
        .ok_or_else(|| format!("会话 {id} 不存在"))?;
    run_command(&entry, &command, std::time::Duration::from_secs(20)).await
}

/// 列出所有活动会话
#[tauri::command]
pub async fn list_sessions(state: State<'_, SshManager>) -> Result<Vec<SessionInfo>, String> {
    let sessions = state.sessions.lock().await;
    let mut out = Vec::new();
    for e in sessions.values() {
        out.push(SessionInfo {
            id: e.id,
            server_id: e.server.id.clone(),
            name: e.server.name.clone(),
            host: e.server.host.clone(),
            port: e.server.port,
            username: e.server.username.clone(),
            status: format!("{:?}", *e.status.lock().await),
            connected_at: e.connected_at,
        });
    }
    Ok(out)
}

/// 发送 SSH 断开包(用于窗口关闭时的清理)
#[tauri::command]
pub async fn close_session_channel(
    state: State<'_, SshManager>,
    id: u64,
) -> Result<(), String> {
    if let Some(entry) = state.sessions.lock().await.remove(&id) {
        let _ = entry.term_writer.close().await;
    }
    Ok(())
}
