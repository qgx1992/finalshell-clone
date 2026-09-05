// ---------- 与 Rust 后端对应的类型 ----------

export interface ProxyConfig {
  host: string;
  port: number;
  username?: string | null;
  password?: string | null;
}

export interface ServerConfig {
  id: string;
  name: string;
  host: string;
  port: number;
  /** "ssh" | "rdp"(远程 Windows) */
  conn_type: "ssh" | "rdp";
  username: string;
  auth_type: "password" | "key";
  password?: string | null;
  private_key?: string | null;
  key_passphrase?: string | null;
  group: string;
  remark: string;
  use_proxy?: boolean;
  proxy?: ProxyConfig | null;
}

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
