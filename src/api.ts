import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  FileEntry,
  MonitorSnapshot,
  ServerConfig,
} from "./types";

// ---------- 配置存储 ----------
export const listServers = () => invoke<ServerConfig[]>("list_servers");
export const saveServer = (server: ServerConfig) =>
  invoke<ServerConfig[]>("save_server", { server });
export const deleteServer = (id: string) =>
  invoke<ServerConfig[]>("delete_server", { id });
export const getSettings = () => invoke<AppSettings>("get_settings");
export const saveSettings = (settings: AppSettings) =>
  invoke<AppSettings>("save_settings", { settings });

// ---------- SSH 会话 ----------
export const connectServer = (server: ServerConfig) =>
  invoke<number>("connect_server", { server });
export const disconnect = (id: number) => invoke<void>("disconnect", { id });
export const writeTerminal = (id: number, dataB64: string) =>
  invoke<void>("write_terminal", { id, dataB64 });
export const resizeTerminal = (id: number, cols: number, rows: number) =>
  invoke<void>("resize_terminal", { id, cols, rows });
export const execCommand = (id: number, command: string) =>
  invoke<string>("exec_command", { id, command });
export const listSessions = () => invoke<unknown[]>("list_sessions");
export const closeSessionChannel = (id: number) =>
  invoke<void>("close_session_channel", { id });

// ---------- SFTP ----------
export const listDir = (id: number, path: string) =>
  invoke<FileEntry[]>("list_dir", { id, path });
export const sftpMkdir = (id: number, path: string) =>
  invoke<void>("sftp_mkdir", { id, path });
export const sftpDelete = (id: number, path: string) =>
  invoke<void>("sftp_delete", { id, path });
export const sftpRename = (id: number, oldPath: string, newPath: string) =>
  invoke<void>("sftp_rename", { id, oldPath, newPath });
export const sftpUpload = (id: number, localPath: string, remotePath: string) =>
  invoke<void>("sftp_upload", { id, localPath, remotePath });
export const sftpDownload = (id: number, remotePath: string, localPath: string) =>
  invoke<void>("sftp_download", { id, remotePath, localPath });
export const sftpReadFile = (id: number, remotePath: string) =>
  invoke<string>("sftp_read_file", { id, remotePath });
export const sftpWriteFile = (id: number, remotePath: string, contentB64: string) =>
  invoke<void>("sftp_write_file", { id, remotePath, contentB64 });
export const sftpPwd = (id: number) => invoke<string>("sftp_pwd", { id });

// ---------- 监控 ----------
export const monitorPoll = (id: number) =>
  invoke<MonitorSnapshot>("monitor_poll", { id });
