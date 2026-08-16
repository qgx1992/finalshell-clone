use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ProxyConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ServerConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    /// "password" | "key"
    pub auth_type: String,
    /// 明文存储(第一版)。后续版本可换系统钥匙串加密。
    pub password: Option<String>,
    /// 私钥文件路径
    pub private_key: Option<String>,
    /// 私钥口令
    pub key_passphrase: Option<String>,
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
            username: String::new(),
            auth_type: "password".into(),
            password: None,
            private_key: None,
            key_passphrase: None,
            group: "默认分组".into(),
            remark: String::new(),
            use_proxy: false,
            proxy: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QuickCommand {
    pub name: String,
    pub command: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppSettings {
    pub theme: String,
    pub color_scheme: String,
    pub font_size: u32,
    pub quick_commands: Vec<QuickCommand>,
    pub last_servers: Vec<String>,
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

    pub fn save_servers(&self, servers: &[ServerConfig]) -> Result<(), String> {
        let json = serde_json::to_string_pretty(servers).map_err(|e| e.to_string())?;
        fs::write(self.servers_path(), json).map_err(|e| e.to_string())
    }

    pub fn load_settings(&self) -> AppSettings {
        match fs::read_to_string(self.settings_path()) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => AppSettings::default(),
        }
    }

    pub fn save_settings(&self, s: &AppSettings) -> Result<(), String> {
        let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
        fs::write(self.settings_path(), json).map_err(|e| e.to_string())
    }
}

// ---------- Tauri commands ----------

#[tauri::command]
pub fn list_servers(app: tauri::AppHandle) -> Vec<ServerConfig> {
    Store::new(&app).load_servers()
}

#[tauri::command]
pub fn save_server(app: tauri::AppHandle, server: ServerConfig) -> Result<Vec<ServerConfig>, String> {
    let store = Store::new(&app);
    let mut list = store.load_servers();
    if let Some(existing) = list.iter_mut().find(|s| s.id == server.id) {
        *existing = server;
    } else {
        list.push(server);
    }
    store.save_servers(&list)?;
    Ok(list)
}

#[tauri::command]
pub fn delete_server(app: tauri::AppHandle, id: String) -> Result<Vec<ServerConfig>, String> {
    let store = Store::new(&app);
    let list = store.load_servers();
    let filtered: Vec<ServerConfig> = list.into_iter().filter(|s| s.id != id).collect();
    store.save_servers(&filtered)?;
    Ok(filtered)
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
