use serde::{Deserialize, Serialize};
use ts_rs::TS;
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

use crate::security::credentials::{self, SecretCache, SecretKind};

#[derive(Serialize, Deserialize, Clone, Debug, Default, TS)]
#[ts(export, export_to = "../../src/generated/")]
pub struct ProxyConfig {
    pub host: String,
    pub port: u16,
    /// "socks5" | "http"
    #[serde(default = "default_proxy_kind")]
    pub kind: Option<String>,
    pub username: Option<String>,
    /// 明文密码只用于旧配置迁移;迁移后置空,真实值在系统凭据库
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// 凭据库中是否存有代理密码
    #[serde(default)]
    pub has_password: bool,
}

fn default_proxy_kind() -> Option<String> {
    Some("socks5".into())
}

fn default_conn_type() -> String {
    "ssh".into()
}

fn default_rdp_resolution() -> String {
    "fullscreen".into()
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, TS)]
#[ts(export, export_to = "../../src/generated/")]
pub struct ServerConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    /// "ssh" | "rdp"
    #[serde(default = "default_conn_type")]
    pub conn_type: String,
    /// RDP 分辨率: "fullscreen" | "auto" | "WxH" (如 "1920x1080"),仅 rdp 使用
    #[serde(default = "default_rdp_resolution")]
    pub rdp_resolution: String,
    /// RDP 是否共享本地剪贴板(默认开),仅 rdp 使用
    #[serde(default = "default_true")]
    pub rdp_clipboard: bool,
    pub username: String,
    /// "password" | "key"
    pub auth_type: String,
    /// 私钥文件路径(非敏感)
    pub private_key: Option<String>,
    /// 旧版本的明文登录密码,仅用于迁移;迁移后为 None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// 旧版本的明文私钥口令,仅用于迁移;迁移后为 None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_passphrase: Option<String>,
    /// 凭据库中是否存有登录密码
    #[serde(default)]
    pub has_password: bool,
    /// 凭据库中是否存有私钥口令
    #[serde(default)]
    pub has_key_passphrase: bool,
    pub group: String,
    pub remark: String,
    #[serde(default)]
    pub use_proxy: bool,
    #[serde(default)]
    pub proxy: Option<ProxyConfig>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            host: String::new(),
            port: 22,
            conn_type: "ssh".into(),
            rdp_resolution: "fullscreen".into(),
            rdp_clipboard: true,
            username: String::new(),
            auth_type: "password".into(),
            private_key: None,
            password: None,
            key_passphrase: None,
            has_password: false,
            has_key_passphrase: false,
            group: "默认分组".into(),
            remark: String::new(),
            use_proxy: false,
            proxy: None,
        }
    }
}

/// 前端提交的凭据变更意图。
/// 用显式枚举而不是空字符串,避免"想清空"和"没改过"被混为一谈。
#[derive(Serialize, Deserialize, Clone, Debug, Default, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
#[ts(export, export_to = "../../src/generated/")]
pub enum SecretInput {
    /// 保持凭据库中的现有值不变
    #[default]
    Keep,
    /// 删除已保存的凭据
    Clear,
    /// 写入新凭据
    Set { value: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, TS)]
#[ts(export, export_to = "../../src/generated/")]
pub struct SecretsInput {
    #[serde(default)]
    pub password: SecretInput,
    #[serde(default)]
    pub key_passphrase: SecretInput,
    #[serde(default)]
    pub proxy_password: SecretInput,
}

#[derive(Serialize, Deserialize, Clone, Debug, TS)]
#[ts(export, export_to = "../../src/generated/")]
pub struct QuickCommand {
    pub name: String,
    pub command: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, TS)]
#[ts(export, export_to = "../../src/generated/")]
pub struct AppSettings {
    pub theme: String,
    pub color_scheme: String,
    pub font_size: u32,
    pub quick_commands: Vec<QuickCommand>,
    pub last_servers: Vec<String>,
    /// 终端回滚缓冲行数,控制后台标签的内存占用
    #[serde(default = "default_scrollback")]
    pub scrollback: u32,
}

fn default_scrollback() -> u32 {
    5000
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            color_scheme: "Monokai Remastered".into(),
            font_size: 14,
            quick_commands: vec![
                QuickCommand {
                    name: "查看系统信息".into(),
                    command: "uname -a && cat /etc/os-release | head -3".into(),
                },
                QuickCommand {
                    name: "查看磁盘使用".into(),
                    command: "df -h".into(),
                },
                QuickCommand {
                    name: "查看内存占用 TOP".into(),
                    command: "top -bn1 | head -25".into(),
                },
                QuickCommand {
                    name: "查看端口监听".into(),
                    command: "ss -tlnp".into(),
                },
                QuickCommand {
                    name: "查看当前目录大文件".into(),
                    command: "du -sh * | sort -rh | head -20".into(),
                },
            ],
            last_servers: Vec::new(),
            scrollback: default_scrollback(),
        }
    }
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(app: &tauri::AppHandle) -> Self {
        let dir = app.path().app_config_dir().unwrap_or_else(|_| {
            std::env::temp_dir().join("finalshell-clone-config")
        });
        let _ = fs::create_dir_all(&dir);
        Self { dir }
    }

    pub fn config_dir(&self) -> &PathBuf {
        &self.dir
    }

    fn servers_path(&self) -> PathBuf {
        self.dir.join("servers.json")
    }

    fn settings_path(&self) -> PathBuf {
        self.dir.join("settings.json")
    }

    pub fn load_servers(&self) -> Vec<ServerConfig> {
        match fs::read_to_string(self.servers_path()) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    /// 原子写入:先写临时文件再替换,避免崩溃/断电产生半个配置
    pub fn save_servers(&self, servers: &[ServerConfig]) -> Result<(), String> {
        let json = serde_json::to_string_pretty(servers).map_err(|e| e.to_string())?;
        atomic_write(&self.servers_path(), &json)
    }

    pub fn load_settings(&self) -> AppSettings {
        match fs::read_to_string(self.settings_path()) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => AppSettings::default(),
        }
    }

    pub fn save_settings(&self, s: &AppSettings) -> Result<(), String> {
        let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
        atomic_write(&self.settings_path(), &json)
    }

    /// 把旧版本明文凭据迁移到系统凭据库。
    ///
    /// - 迁移前保留一次性备份 `servers.json.bak`
    /// - 迁移成功后清空 JSON 里的明文字段
    /// - 单条失败不回滚已迁移部分,下次启动会重试剩余条目
    pub fn migrate_plaintext_secrets(&self) -> Result<usize, String> {
        let mut servers = self.load_servers();
        let mut migrated = 0usize;
        let mut backup_needed = false;

        for server in servers.iter_mut() {
            if let Some(password) = server.password.clone().filter(|v| !v.is_empty()) {
                credentials::set_secret(&server.id, SecretKind::Password, &password)
                    .map_err(|e| e.to_string())?;
                server.password = None;
                server.has_password = true;
                migrated += 1;
                backup_needed = true;
            }
            if let Some(pass) = server.key_passphrase.clone().filter(|v| !v.is_empty()) {
                credentials::set_secret(&server.id, SecretKind::KeyPassphrase, &pass)
                    .map_err(|e| e.to_string())?;
                server.key_passphrase = None;
                server.has_key_passphrase = true;
                migrated += 1;
                backup_needed = true;
            }
            if let Some(proxy) = server.proxy.as_mut() {
                if let Some(pass) = proxy.password.clone().filter(|v| !v.is_empty()) {
                    credentials::set_secret(&server.id, SecretKind::ProxyPassword, &pass)
                        .map_err(|e| e.to_string())?;
                    proxy.password = None;
                    proxy.has_password = true;
                    migrated += 1;
                    backup_needed = true;
                }
            }
        }

        if backup_needed {
            let original = self.servers_path();
            if original.exists() {
                let backup = self.dir.join("servers.json.bak");
                if !backup.exists() {
                    let _ = fs::copy(&original, &backup);
                }
            }
            self.save_servers(&servers)?;
        }
        Ok(migrated)
    }
}

/// 原子写入实现:同目录临时文件 + 替换
fn atomic_write(path: &PathBuf, content: &str) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, content).map_err(|e| e.to_string())?;
    // Windows 上 rename 不会覆盖已存在文件,先尝试直接替换
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(_) => {
            let _ = fs::remove_file(path);
            fs::rename(&tmp, path).map_err(|e| e.to_string())
        }
    }
}

/// 对外返回的配置:不含任何明文凭据,只带"是否已保存"标志
fn sanitize(mut server: ServerConfig) -> ServerConfig {
    server.password = None;
    server.key_passphrase = None;
    if let Some(proxy) = server.proxy.as_mut() {
        proxy.password = None;
    }
    server
}

// ---------- Tauri commands ----------

#[tauri::command]
pub fn list_servers(app: tauri::AppHandle) -> Vec<ServerConfig> {
    Store::new(&app)
        .load_servers()
        .into_iter()
        .map(sanitize)
        .collect()
}

/// 保存服务器配置,并按 `secrets` 的意图增删系统凭据库中的凭据
#[tauri::command]
pub fn save_server(
    app: tauri::AppHandle,
    cache: tauri::State<'_, SecretCache>,
    mut server: ServerConfig,
    #[allow(non_snake_case)] secrets: Option<SecretsInput>,
) -> Result<Vec<ServerConfig>, String> {
    let store = Store::new(&app);
    let secrets = secrets.unwrap_or_default();

    // 凭据落库(先于配置写入,避免配置指向不存在的凭据)
    apply_secret(&server.id, SecretKind::Password, &secrets.password)?;
    apply_secret(
        &server.id,
        SecretKind::KeyPassphrase,
        &secrets.key_passphrase,
    )?;
    apply_secret(
        &server.id,
        SecretKind::ProxyPassword,
        &secrets.proxy_password,
    )?;
    if !matches!(secrets.password, SecretInput::Keep) {
        cache.invalidate(&server.id);
    }
    if !matches!(secrets.key_passphrase, SecretInput::Keep) {
        cache.invalidate(&server.id);
    }
    if !matches!(secrets.proxy_password, SecretInput::Keep) {
        cache.invalidate(&server.id);
    }

    // 依据凭据库真实状态回填标志位
    server.password = None;
    server.key_passphrase = None;
    server.has_password = credentials::get_secret(&server.id, SecretKind::Password)
        .ok()
        .flatten()
        .is_some();
    server.has_key_passphrase = credentials::get_secret(&server.id, SecretKind::KeyPassphrase)
        .ok()
        .flatten()
        .is_some();
    if let Some(proxy) = server.proxy.as_mut() {
        proxy.password = None;
        proxy.has_password = credentials::get_secret(&server.id, SecretKind::ProxyPassword)
            .ok()
            .flatten()
            .is_some();
    }

    let mut list = store.load_servers();
    if let Some(existing) = list.iter_mut().find(|s| s.id == server.id) {
        *existing = server;
    } else {
        list.push(server);
    }
    store.save_servers(&list)?;
    Ok(list.into_iter().map(sanitize).collect())
}

fn apply_secret(
    server_id: &str,
    kind: SecretKind,
    input: &SecretInput,
) -> Result<(), String> {
    match input {
        SecretInput::Keep => Ok(()),
        SecretInput::Clear => {
            credentials::delete_secret(server_id, kind).map_err(|e| e.to_string())
        }
        SecretInput::Set { value } => {
            if value.is_empty() {
                credentials::delete_secret(server_id, kind).map_err(|e| e.to_string())
            } else {
                credentials::set_secret(server_id, kind, value).map_err(|e| e.to_string())
            }
        }
    }
}

#[tauri::command]
pub fn delete_server(
    app: tauri::AppHandle,
    cache: tauri::State<'_, SecretCache>,
    id: String,
) -> Result<Vec<ServerConfig>, String> {
    let store = Store::new(&app);
    let list = store.load_servers();
    let filtered: Vec<ServerConfig> = list.into_iter().filter(|s| s.id != id).collect();
    store.save_servers(&filtered)?;
    // 配置删除后清理对应凭据,避免凭据库残留
    credentials::delete_all(&id);
    cache.invalidate(&id);
    Ok(filtered.into_iter().map(sanitize).collect())
}

#[tauri::command]
pub fn get_settings(app: tauri::AppHandle) -> AppSettings {
    Store::new(&app).load_settings()
}

#[tauri::command]
pub fn save_settings(app: tauri::AppHandle, settings: AppSettings) -> Result<AppSettings, String> {
    let store = Store::new(&app);
    store.save_settings(&settings)?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_removes_all_plaintext_secrets() {
        let mut cfg = ServerConfig::default();
        cfg.password = Some("p".into());
        cfg.key_passphrase = Some("k".into());
        cfg.proxy = Some(ProxyConfig {
            host: "127.0.0.1".into(),
            port: 1080,
            kind: Some("socks5".into()),
            username: Some("u".into()),
            password: Some("pp".into()),
            has_password: true,
        });
        let clean = sanitize(cfg);
        assert!(clean.password.is_none());
        assert!(clean.key_passphrase.is_none());
        assert!(clean.proxy.unwrap().password.is_none());
    }

    #[test]
    fn secret_input_defaults_to_keep() {
        assert!(matches!(
            SecretsInput::default().password,
            SecretInput::Keep
        ));
    }

    #[test]
    fn settings_default_scrollback_is_bounded() {
        assert_eq!(AppSettings::default().scrollback, 5000);
    }
}
