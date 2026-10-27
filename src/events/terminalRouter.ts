import { listen } from "@tauri-apps/api/event";

/**
 * 全局事件路由。
 *
 * 背景:每个标签各自 listen("term-data") 时,后端每发一次输出,
 * N 个标签的回调都会被调用后再按 sessionId 过滤,标签越多放大越严重。
 *
 * 这里把事件监听收敛为"整个应用只注册一次",内部再按 sessionId 分发,
 * 使回调次数与"实际匹配的会话数"成正比,而不是与"打开的标签数"成正比。
 */

type TermDataHandler = (b64: string, stream: string) => void;
type TermExitHandler = (status: string, reason: string) => void;
type SftpProgressHandler = (payload: [number, string, string, string, number, number]) => void;

const termDataHandlers = new Map<number, Set<TermDataHandler>>();
const termExitHandlers = new Map<number, Set<TermExitHandler>>();

let started = false;
let startPromise: Promise<void> | null = null;

/** 启动全局监听(幂等;多次调用只注册一次) */
export function ensureEventRouter(): Promise<void> {
  if (started) return Promise.resolve();
  if (startPromise) return startPromise;

  startPromise = (async () => {
    await listen<[number, string, string]>("term-data", (e) => {
      const [sid, b64, stream] = e.payload;
      const handlers = termDataHandlers.get(sid);
      if (!handlers) return;
      for (const h of handlers) {
        try {
          h(b64, stream);
        } catch {
          // 单个标签的渲染异常不应影响其他标签
        }
      }
    });

    await listen<[number, string, string]>("term-exit", (e) => {
      const [sid, status, reason] = e.payload;
      const handlers = termExitHandlers.get(sid);
      if (!handlers) return;
      for (const h of handlers) {
        try {
          h(status, reason);
        } catch {
          // 忽略单个订阅者的异常
        }
      }
    });

    started = true;
    startPromise = null;
  })();

  return startPromise;
}

/** 订阅某个会话的终端输出,返回取消订阅函数 */
export function onTermData(sessionId: number, handler: TermDataHandler): () => void {
  void ensureEventRouter();
  let set = termDataHandlers.get(sessionId);
  if (!set) {
    set = new Set();
    termDataHandlers.set(sessionId, set);
  }
  set.add(handler);
  return () => {
    const current = termDataHandlers.get(sessionId);
    if (!current) return;
    current.delete(handler);
    if (current.size === 0) termDataHandlers.delete(sessionId);
  };
}

/** 订阅某个会话的退出事件,返回取消订阅函数 */
export function onTermExit(sessionId: number, handler: TermExitHandler): () => void {
  void ensureEventRouter();
  let set = termExitHandlers.get(sessionId);
  if (!set) {
    set = new Set();
    termExitHandlers.set(sessionId, set);
  }
  set.add(handler);
  return () => {
    const current = termExitHandlers.get(sessionId);
    if (!current) return;
    current.delete(handler);
    if (current.size === 0) termExitHandlers.delete(sessionId);
  };
}

/** SFTP 进度事件同样收敛为单一监听 */
const sftpHandlers = new Map<number, Set<SftpProgressHandler>>();
let sftpStarted = false;
let sftpStartPromise: Promise<void> | null = null;

export function ensureSftpRouter(): Promise<void> {
  if (sftpStarted) return Promise.resolve();
  if (sftpStartPromise) return sftpStartPromise;
  sftpStartPromise = (async () => {
    await listen<[number, string, string, string, number, number]>("sftp-progress", (e) => {
      const [sid] = e.payload;
      const handlers = sftpHandlers.get(sid);
      if (!handlers) return;
      for (const h of handlers) {
        try {
          h(e.payload);
        } catch {
          // 忽略单个订阅者的异常
        }
      }
    });
    sftpStarted = true;
    sftpStartPromise = null;
  })();
  return sftpStartPromise;
}

export function onSftpProgress(sessionId: number, handler: SftpProgressHandler): () => void {
  void ensureSftpRouter();
  let set = sftpHandlers.get(sessionId);
  if (!set) {
    set = new Set();
    sftpHandlers.set(sessionId, set);
  }
  set.add(handler);
  return () => {
    const current = sftpHandlers.get(sessionId);
    if (!current) return;
    current.delete(handler);
    if (current.size === 0) sftpHandlers.delete(sessionId);
  };
}

/**
 * 打包(生成压缩包)进度。
 *
 * 独立一条事件通道,和传输进度分开:
 * 打包是「一个任务 + 总条目数」,不会像上传下载那样并发多条,
 * 复用 sftp-progress 会让两者互相覆盖。
 */
/** [sessionId, 百分比, 已处理, 总量, 阶段, 单位("bytes" | "files")] */
export type ArchiveProgressHandler = (
  payload: [number, number, number, number, string, string]
) => void;

const archiveHandlers = new Map<number, Set<ArchiveProgressHandler>>();
let archiveStarted = false;
let archiveStartPromise: Promise<void> | null = null;

export function ensureArchiveRouter(): Promise<void> {
  if (archiveStarted) return Promise.resolve();
  if (archiveStartPromise) return archiveStartPromise;
  archiveStartPromise = (async () => {
    await listen<[number, number, number, number, string, string]>("sftp-archive-progress", (e) => {
      const [sid] = e.payload;
      const handlers = archiveHandlers.get(sid);
      if (!handlers) return;
      for (const h of handlers) {
        try {
          h(e.payload);
        } catch {
          // 忽略单个订阅者的异常
        }
      }
    });
    archiveStarted = true;
    archiveStartPromise = null;
  })();
  return archiveStartPromise;
}

export function onArchiveProgress(sessionId: number, handler: ArchiveProgressHandler): () => void {
  void ensureArchiveRouter();
  let set = archiveHandlers.get(sessionId);
  if (!set) {
    set = new Set();
    archiveHandlers.set(sessionId, set);
  }
  set.add(handler);
  return () => {
    const current = archiveHandlers.get(sessionId);
    if (!current) return;
    current.delete(handler);
    if (current.size === 0) archiveHandlers.delete(sessionId);
  };
}
