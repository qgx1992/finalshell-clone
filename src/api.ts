import { invoke } from "@tauri-apps/api/core";
import type {
  AppErrorBody,
  AppSettings,
  FileEntry,
  HostKeyObservation,
  MonitorSnapshot,
  SecretsInput,
  ServerConfig,
} from "./types";

/** 所有后端 invoke 共用的错误类型,保留 code 以便区分指纹确认等场景 */
export class ApiError extends Error {
  readonly code: string;
  readonly body: AppErrorBody;

  constructor(body: AppErrorBody) {
    super(describe(body));
    this.name = "ApiError";
    this.body = body;
    this.code = body.code;
  }
}

function describe(body: AppErrorBody): string {
  switch (body.code) {
    case "host_key_unknown":
      return `首次连接 ${body.observation.host}:${body.observation.port},需要确认服务器指纹`;
    case "host_key_changed":
      return `服务器 ${body.observation.host}:${body.observation.port} 的主机密钥已变更`;
    case "session_not_found":
      return `会话 ${body.id} 不存在`;
    case "auth_failed":
      return `认证失败: ${body.message}`;
    case "config":
      return `配置错误: ${body.message}`;
    case "network":
      return `网络错误: ${body.message}`;
    case "timeout":
      return `超时: ${body.message}`;
    default:
      return body.message;
  }
}

/** 把 Tauri 抛出的任意错误归一化为 ApiError */
export function toApiError(e: unknown): ApiError {
  if (e instanceof ApiError) return e;
  if (e && typeof e === "object" && "code" in e) {
    return new ApiError(e as AppErrorBody);
  }
  const message = e instanceof Error ? e.message : String(e);
  // 后端旧命令仍返回字符串错误
  try {
    const parsed = JSON.parse(message) as AppErrorBody;
    if (parsed && typeof parsed === "object" && "code" in parsed) {
      return new ApiError(parsed);
    }
  } catch {
    // 非 JSON,按普通错误处理
  }
  return new ApiError({ code: "other", message });
}

/** 统一 invoke 包装:失败时抛 ApiError,成功时返回强类型结果 */
async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toApiError(e);
  }
}

// ---------- 配置存储 ----------
export const listServers = () => call<ServerConfig[]>("list_servers");
export const saveServer = (server: ServerConfig, secrets?: SecretsInput) =>
  call<ServerConfig[]>("save_server", { server, secrets: secrets ?? null });
export const deleteServer = (id: string) => call<ServerConfig[]>("delete_server", { id });
export const getSettings = () => call<AppSettings>("get_settings");
export const saveSettings = (settings: AppSettings) =>
  call<AppSettings>("save_settings", { settings });

// ---------- SSH 会话 ----------
export interface ConnectOptions {
  hostKeyDecision?: "trust_new" | "trust_changed";
  hostKeyFingerprint?: string;
  /** 仅本次连接使用的临时凭据,不会写入磁盘 */
  inlinePassword?: string;
  inlineKeyPassphrase?: string;
}

export const connectServer = (server: ServerConfig, options: ConnectOptions = {}) =>
  call<number>("connect_server", {
    server,
    hostKeyDecision: options.hostKeyDecision ?? null,
    hostKeyFingerprint: options.hostKeyFingerprint ?? null,
    inlinePassword: options.inlinePassword ?? null,
    inlineKeyPassphrase: options.inlineKeyPassphrase ?? null,
  });
export const disconnect = (id: number) => call<void>("disconnect", { id });
export const writeTerminal = (id: number, dataB64: string) =>
  call<void>("write_terminal", { id, dataB64 });
export const resizeTerminal = (id: number, cols: number, rows: number) =>
  call<void>("resize_terminal", { id, cols, rows });
export const execCommand = (id: number, command: string) =>
  call<string>("exec_command", { id, command });
export const listSessions = () => call<unknown[]>("list_sessions");

// ---------- 主机信任 ----------
export const trustedHostFingerprint = (host: string, port: number) =>
  call<HostKeyObservation | null>("trusted_host_fingerprint", { host, port });
export const forgetHostKey = (host: string, port: number) =>
  call<void>("forget_host_key", { host, port });

// ---------- 远程 Windows (RDP) ----------
export const connectRdp = (server: ServerConfig) =>
  call<void>("connect_rdp", { server });
export const closeSessionChannel = (id: number) =>
  call<void>("close_session_channel", { id });

// ---------- SFTP ----------
export const listDir = (id: number, path: string) =>
  call<FileEntry[]>("list_dir", { id, path });
export const sftpMkdir = (id: number, path: string) =>
  call<void>("sftp_mkdir", { id, path });
export const sftpDelete = (id: number, path: string) =>
  call<void>("sftp_delete", { id, path });
export const sftpRename = (id: number, oldPath: string, newPath: string) =>
  call<void>("sftp_rename", { id, oldPath, newPath });
export const sftpUpload = (id: number, localPath: string, remotePath: string) =>
  call<void>("sftp_upload", { id, localPath, remotePath });
export const sftpDownload = (id: number, remotePath: string, localPath: string) =>
  call<void>("sftp_download", { id, remotePath, localPath });
export const sftpReadFile = (id: number, remotePath: string) =>
  call<string>("sftp_read_file", { id, remotePath });
export const sftpWriteFile = (id: number, remotePath: string, contentB64: string) =>
  call<void>("sftp_write_file", { id, remotePath, contentB64 });
export const sftpPwd = (id: number) => call<string>("sftp_pwd", { id });

// ---------- 监控 ----------
export const monitorPoll = (id: number) =>
  call<MonitorSnapshot>("monitor_poll", { id });
