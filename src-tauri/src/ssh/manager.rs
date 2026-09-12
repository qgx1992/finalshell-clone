use base64::{engine::general_purpose::STANDARD as B64, Engine};
use russh::client::{self, Handle};
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg, PublicKey};
use russh::ChannelMsg;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;
use tracing::info;

use crate::error::AppError;
use crate::security::credentials::{SecretCache, SecretKind};
use crate::security::known_hosts::{
    known_hosts_path, HostKeyObservation, HostKeyStatus, HostKeyVerifier, TrustDecision,
};
use crate::store::{ServerConfig, Store};

// ---------- 事件名 ----------
pub const EV_TERM_DATA: &str = "term-data";
pub const EV_TERM_EXIT: &str = "term-exit";

/// russh 客户端 Handler:把主机密钥校验委托给 TOFU 校验器
pub struct SshHandler {
    verifier: HostKeyVerifier,
}

impl SshHandler {
    pub fn new(verifier: HostKeyVerifier) -> Self {
        Self { verifier }
    }
}

impl client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        self.verifier.verify(server_public_key).await
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

/// 监控静态信息缓存:主机名/CPU 核数几乎不变,只在首次采样时获取一次
#[derive(Default)]
pub struct MonitorCache {
    pub hostname: Option<String>,
    pub cores: Option<u32>,
    /// 远程系统类型探测结果,避免在不支持的平台反复执行 /proc 命令
    pub platform: Option<RemotePlatform>,
}

/// 远程主机可用的监控采集方式
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemotePlatform {
    Linux,
    Windows,
    MacOs,
    Unsupported,
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
    /// 监控静态信息缓存(hostname/cores 仅采集一次)
    pub monitor_cache: Mutex<MonitorCache>,
    /// 读循环是否已经结束(用于幂等清理)
    pub reader_done: std::sync::atomic::AtomicBool,
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

    pub async fn get_entry(&self, id: u64) -> Option<Arc<SessionEntry>> {
        self.sessions.lock().await.get(&id).cloned()
    }

    /// 移除会话并返回它,保证清理逻辑只执行一次
    pub async fn take_entry(&self, id: u64) -> Option<Arc<SessionEntry>> {
        self.sessions.lock().await.remove(&id)
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

/// 解析认证所需的凭据(优先当前输入,其次系统凭据库)
fn resolve_credential(
    cache: &SecretCache,
    server: &ServerConfig,
    kind: SecretKind,
    inline: Option<String>,
) -> Result<Option<String>, AppError> {
    if let Some(value) = inline.filter(|v| !v.is_empty()) {
        return Ok(Some(value));
    }
    cache
        .get(&server.id, kind)
        .map_err(|e| AppError::config(e.to_string()))
}

/// 建立 TCP 连接:直连或经代理
async fn open_transport(
    server: &ServerConfig,
    cache: &SecretCache,
) -> Result<tokio::net::TcpStream, AppError> {
    if !server.use_proxy {
        let addr = format!("{}:{}", server.host, server.port);
        return tokio::time::timeout(
            std::time::Duration::from_secs(20),
            tokio::net::TcpStream::connect(&addr),
        )
        .await
        .map_err(|_| AppError::Timeout {
            message: format!("连接 {addr} 超时"),
        })?
        .map_err(|e| AppError::network(format!("连接 {addr} 失败: {e}")));
    }

    let Some(mut proxy) = server.proxy.clone() else {
        return Err(AppError::config("已启用代理但未配置代理服务器"));
    };
    // 代理密码从系统凭据库补齐
    if proxy.password.is_none() {
        proxy.password = cache
            .get(&server.id, SecretKind::ProxyPassword)
            .map_err(|e| AppError::config(e.to_string()))?;
    }
    crate::ssh::proxy::connect_through_proxy(&server.host, server.port, &proxy)
        .await
        .map_err(AppError::network)
}

// ---------- Tauri commands ----------

/// 建立 SSH 连接,返回会话 id
#[tauri::command]
pub async fn connect_server(
    app: AppHandle,
    state: State<'_, SshManager>,
    cache: State<'_, SecretCache>,
    server: ServerConfig,
    host_key_decision: Option<String>,
    host_key_fingerprint: Option<String>,
    inline_password: Option<String>,
    inline_key_passphrase: Option<String>,
) -> Result<u64, AppError> {
    let store = Store::new(&app);
    let decision = match (host_key_decision.as_deref(), host_key_fingerprint) {
        (Some("trust_new"), Some(fp)) => Some(TrustDecision::TrustNew { fingerprint: fp }),
        (Some("trust_changed"), Some(fp)) => Some(TrustDecision::TrustChanged { fingerprint: fp }),
        _ => None,
    };

    let verifier = HostKeyVerifier::new(
        known_hosts_path(store.config_dir()),
        server.host.clone(),
        server.port,
        decision,
    );
    let observation = verifier.observation();
    let handler = SshHandler::new(verifier);

    // 认证凭据解析(不写日志)
    let auth_secret = match server.auth_type.as_str() {
        "key" => resolve_credential(
            &cache,
            &server,
            SecretKind::KeyPassphrase,
            inline_key_passphrase.clone(),
        )?,
        _ => resolve_credential(
            &cache,
            &server,
            SecretKind::Password,
            inline_password.clone(),
        )?,
    };

    let stream = open_transport(&server, &cache).await?;
    let config = client::Config::default();
    let connect_result = client::connect_stream(Arc::new(config), stream, handler).await;

    let mut handle = match connect_result {
        Ok(handle) => handle,
        Err(e) => {
            // 握手失败:如果是主机密钥未确认/变更,转换成结构化错误让前端弹窗
            if let Some(obs) = observation.lock().await.clone() {
                return match obs.status {
                    HostKeyStatus::Unknown => Err(AppError::HostKeyUnknown { observation: obs }),
                    HostKeyStatus::Changed => Err(AppError::HostKeyChanged { observation: obs }),
                    HostKeyStatus::Trusted => Err(AppError::network(format!("SSH 握手失败: {e}"))),
                };
            }
            return Err(AppError::network(format!(
                "连接 {}:{} 失败: {e}",
                server.host, server.port
            )));
        }
    };

    // 认证
    let auth_ok = match server.auth_type.as_str() {
        "key" => {
            let key_path = server
                .private_key
                .clone()
                .filter(|p| !p.is_empty())
                .ok_or_else(|| AppError::config("未指定私钥路径"))?;
            let key = load_secret_key(&key_path, auth_secret.as_deref())
                .map_err(|e| AppError::config(format!("加载私钥失败: {e}")))?;
            let kh = PrivateKeyWithHashAlg::new(Arc::new(key), None);
            handle
                .authenticate_publickey(&server.username, kh)
                .await
                .map_err(|e| AppError::AuthFailed {
                    message: e.to_string(),
                })?
        }
        _ => {
            let pass = auth_secret.ok_or_else(|| AppError::config("未设置密码"))?;
            handle
                .authenticate_password(&server.username, &pass)
                .await
                .map_err(|e| AppError::AuthFailed {
                    message: e.to_string(),
                })?
        }
    };
    if !matches!(auth_ok, russh::client::AuthResult::Success) {
        return Err(AppError::AuthFailed {
            message: "用户名或密码/密钥不正确".into(),
        });
    }

    // 打开终端通道
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| AppError::network(format!("打开会话通道失败: {e}")))?;
    channel
        .request_pty(true, "xterm-256color", 120, 32, 0, 0, &[])
        .await
        .map_err(|e| AppError::network(format!("请求 PTY 失败: {e}")))?;
    channel
        .request_shell(true)
        .await
        .map_err(|e| AppError::network(format!("启动 shell 失败: {e}")))?;
    // 拆分读写半:读循环与写入互不阻塞,消除输入卡顿
    let (reader, writer) = channel.split();

    let id = state.next_id.fetch_add(1, Ordering::SeqCst);
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

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
        monitor_cache: Mutex::new(MonitorCache::default()),
        reader_done: std::sync::atomic::AtomicBool::new(false),
    });

    state.sessions.lock().await.insert(id, entry.clone());
    info!(session_id = id, host = %server.host, port = server.port, "SSH connected");

    // 记录最近连接
    let mut settings = store.load_settings();
    settings.last_servers.retain(|s| s != &server.id);
    settings.last_servers.insert(0, server.id.clone());
    if settings.last_servers.len() > 20 {
        settings.last_servers.truncate(20);
    }
    let _ = store.save_settings(&settings);

    // 广播连接事件
    let _ = app.emit(EV_TERM_EXIT, (id, "connected".to_string(), String::new()));

    // 启动终端读循环:首个数据到达前无限等待,收到数据后用 16ms 窗口合并
    let app2 = app.clone();
    let entry2 = entry.clone();
    let sessions = state.sessions.clone();
    tokio::spawn(async move {
        const FLUSH_BYTES: usize = 32 * 1024;
        let mut pending: Vec<u8> = Vec::new();
        let mut first_byte_seen = false;

        loop {
            let msg = if first_byte_seen {
                let mut reader = entry2.term_reader.lock().await;
                match tokio::time::timeout(std::time::Duration::from_millis(16), reader.wait()).await
                {
                    Ok(msg) => (true, msg),
                    Err(_) => (false, None),
                }
            } else {
                let mut reader = entry2.term_reader.lock().await;
                (true, reader.wait().await)
            };
            let (got_message, msg) = msg;

            match msg {
                Some(ChannelMsg::Data { data }) => {
                    first_byte_seen = true;
                    pending.extend_from_slice(&data);
                }
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    first_byte_seen = true;
                    pending.extend_from_slice(&data);
                }
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) => break,
                Some(_) => {}
                None => {
                    if got_message {
                        break;
                    }
                }
            }

            if !pending.is_empty() && (pending.len() >= FLUSH_BYTES || !got_message) {
                let _ = app2.emit(EV_TERM_DATA, (id, B64.encode(&pending), "out".to_string()));
                pending.clear();
                first_byte_seen = false;
            }
        }
        // 退出前补发剩余数据
        if !pending.is_empty() {
            let _ = app2.emit(EV_TERM_DATA, (id, B64.encode(&pending), "out".to_string()));
        }

        // 读循环结束后清理会话,避免断线会话残留在内存里
        let taken = sessions.lock().await.remove(&id);
        if let Some(entry) = taken {
            entry
                .reader_done
                .store(true, std::sync::atomic::Ordering::SeqCst);
            *entry.status.lock().await = SessionStatus::Closed;
            let _ = entry.term_writer.close().await;
        }
        let _ = app2.emit(
            EV_TERM_EXIT,
            (id, "closed".to_string(), "连接已关闭".to_string()),
        );
        info!(session_id = id, "SSH session closed");
    });

    // 内存凭据用后即清
    if let Some(mut value) = inline_password {
        crate::security::credentials::zeroize_string(&mut value);
    }
    if let Some(mut value) = inline_key_passphrase {
        crate::security::credentials::zeroize_string(&mut value);
    }

    Ok(id)
}

/// 断开连接
#[tauri::command]
pub async fn disconnect(app: AppHandle, state: State<'_, SshManager>, id: u64) -> Result<(), String> {
    if let Some(entry) = state.take_entry(id).await {
        entry
            .reader_done
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = entry.term_writer.close().await;
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
    if let Some(entry) = state.take_entry(id).await {
        let _ = entry.term_writer.close().await;
    }
    Ok(())
}

/// 查询一台服务器在本机记录的指纹(用于设置页展示/清理)
#[tauri::command]
pub fn trusted_host_fingerprint(
    app: AppHandle,
    host: String,
    port: u16,
) -> Option<HostKeyObservation> {
    let store = Store::new(&app);
    let path = known_hosts_path(store.config_dir());
    let keys = crate::security::known_hosts::recorded_keys(&path, &host, port);
    let key = keys.first()?;
    Some(HostKeyObservation {
        status: HostKeyStatus::Trusted,
        host,
        port,
        fingerprint: key.fingerprint(russh::keys::HashAlg::Sha256).to_string(),
        algorithm: key.algorithm().to_string(),
        previous_fingerprint: None,
    })
}

/// 清除某台主机的信任记录
#[tauri::command]
pub fn forget_host_key(app: AppHandle, host: String, port: u16) -> Result<(), String> {
    let store = Store::new(&app);
    let path = known_hosts_path(store.config_dir());
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let target_plain = host.clone();
    let target_port = format!("[{host}]:{port}");
    let kept: Vec<&str> = content
        .lines()
        .filter(|line| {
            let Some(hosts) = line.split_whitespace().next() else {
                return true;
            };
            if hosts.starts_with('|') {
                return true;
            }
            !hosts
                .split(',')
                .any(|h| h == target_plain || h == target_port)
        })
        .collect();
    let mut text = kept.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    std::fs::write(&path, text).map_err(|e| e.to_string())
}
