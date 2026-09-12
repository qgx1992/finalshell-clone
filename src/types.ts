// ---------- 与 Rust 后端对应的类型 ----------
// 注意:该文件由后端类型手工对齐;字段变更时必须同步 src-tauri/src/store.rs

export interface ProxyConfig {
  host: string;
  port: number;
  /** "socks5" | "http" */
  kind?: string | null;
  username?: string | null;
  /** 后端永不返回明文密码,该字段仅用于提交新密码 */
  password?: string | null;
  /** 凭据库中是否已保存代理密码 */
  has_password?: boolean;
}

export interface ServerConfig {
  id: string;
  name: string;
  host: string;
  port: number;
  /** "ssh" | "rdp"(远程 Windows) */
  conn_type: "ssh" | "rdp";
  /** RDP 分辨率: "fullscreen" | "auto" | "WxH"(如 "1920x1080"),仅 rdp 使用 */
  rdp_resolution?: string;
  /** RDP 是否共享本地剪贴板(默认 true),仅 rdp 使用 */
  rdp_clipboard?: boolean;
  username: string;
  auth_type: "password" | "key";
  /** 旧字段:仅用于提交新密码,列表接口不会返回明文 */
  password?: string | null;
  private_key?: string | null;
  /** 旧字段:仅用于提交新口令,列表接口不会返回明文 */
  key_passphrase?: string | null;
  /** 系统凭据库中是否已保存登录密码 */
  has_password?: boolean;
  /** 系统凭据库中是否已保存私钥口令 */
  has_key_passphrase?: boolean;
  group: string;
  remark: string;
  use_proxy?: boolean;
  proxy?: ProxyConfig | null;
}

/** 凭据变更意图:与后端 SecretInput 对应 */
export type SecretInput =
  | { action: "keep" }
  | { action: "clear" }
  | { action: "set"; value: string };

export interface SecretsInput {
  password?: SecretInput;
  key_passphrase?: SecretInput;
  proxy_password?: SecretInput;
}

export interface HostKeyObservation {
  status: "unknown" | "changed" | "trusted";
  host: string;
  port: number;
  fingerprint: string;
  algorithm: string;
  previous_fingerprint?: string | null;
}

/** 后端结构化错误(serde 内部标签 code) */
export type AppErrorBody =
  | { code: "host_key_unknown"; observation: HostKeyObservation }
  | { code: "host_key_changed"; observation: HostKeyObservation }
  | { code: "session_not_found"; id: number }
  | { code: "auth_failed"; message: string }
  | { code: "config"; message: string }
  | { code: "network"; message: string }
  | { code: "timeout"; message: string }
  | { code: "other"; message: string };

export interface QuickCommand {
  name: string;
  command: string;
}

export interface AppSettings {
  theme: "dark" | "light";
  color_scheme: string;
  font_size: number;
  quick_commands: QuickCommand[];
  last_servers: string[];
  /** 终端回滚缓冲行数 */
  scrollback: number;
}

export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  is_symlink: boolean;
  size: number;
  modified: number;
  permissions: string;
}

export interface MonitorSnapshot {
  cpu_percent: number;
  cpu_cores: number;
  mem_total: number;
  mem_used: number;
  mem_percent: number;
  swap_total: number;
  swap_used: number;
  disk_total: number;
  disk_used: number;
  disk_percent: number;
  net_rx: number;
  net_tx: number;
  net_rx_speed: number;
  net_tx_speed: number;
  load1: number;
  load5: number;
  load15: number;
  uptime_secs: number;
  hostname: string;
}

export interface SessionInfo {
  id: number;
  server_id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  status: string;
  connected_at: number;
}
