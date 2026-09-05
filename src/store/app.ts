import { create } from "zustand";
import {
  deleteServer as apiDeleteServer,
  disconnect as apiDisconnect,
  getSettings,
  listServers,
  saveServer as apiSaveServer,
  saveSettings as apiSaveSettings,
} from "../api";
import type { AppSettings, ServerConfig } from "../types";

/** 旧版本保存的服务器没有 conn_type / rdp_resolution 字段,统一补默认值 */
function normalizeServers(servers: ServerConfig[]): ServerConfig[] {
  return servers.map((s) => ({
    ...s,
    conn_type: s.conn_type ?? "ssh",
    rdp_resolution: s.rdp_resolution || "fullscreen",
  }));
}

export interface Tab {
  key: string; // 唯一 key
  serverId: string;
  sessionId: number | null;
  server: ServerConfig; // 连接时快照,即使删除配置也不影响已打开的会话
  serverName: string;
  host: string;
  status: "connecting" | "connected" | "closed";
  error?: string;
}

interface AppState {
  servers: ServerConfig[];
  settings: AppSettings;
  tabs: Tab[];
  activeTabKey: string | null;
  sessionToTab: Record<number, string>; // sessionId -> tabKey
  /** 重连信号:tabKey -> 递增序号(每请求一次 +1,确保重复请求也能触发 effect) */
  reconnectSeq: Record<string, number>;

  init: () => Promise<void>;
  refreshServers: () => Promise<void>;
  upsertServer: (s: ServerConfig) => Promise<void>;
  removeServer: (id: string) => Promise<void>;
  updateSettings: (patch: Partial<AppSettings>) => Promise<void>;

  openTab: (server: ServerConfig) => string;
  closeTab: (key: string) => void;
  setActiveTab: (key: string) => void;
  setSession: (key: string, sessionId: number) => void;
  setTabStatus: (key: string, status: Tab["status"], error?: string) => void;
  /** 断开标签的连接(清空 sessionId,状态置 closed) */
  disconnectTab: (key: string) => Promise<void>;
  /** 请求重连(由 TerminalPanel 监听信号实际执行连接) */
  requestReconnect: (key: string) => void;
}

let tabSeq = 0;

export const useApp = create<AppState>((set, get) => ({
  servers: [],
  settings: {
    theme: "dark",
    color_scheme: "Monokai Remastered",
    font_size: 14,
    quick_commands: [],
    last_servers: [],
  },
  tabs: [],
  activeTabKey: null,
  sessionToTab: {},
  reconnectSeq: {},

  init: async () => {
    const [servers, settings] = await Promise.all([listServers(), getSettings()]);
    set({ servers: normalizeServers(servers), settings });
  },

  refreshServers: async () => {
    set({ servers: normalizeServers(await listServers()) });
  },

  upsertServer: async (s) => {
    await apiSaveServer(s);
    await get().refreshServers();
  },

  removeServer: async (id) => {
    await apiDeleteServer(id);
    await get().refreshServers();
  },

  updateSettings: async (patch) => {
    const next = { ...get().settings, ...patch };
    set({ settings: next });
    await apiSaveSettings(next);
  },

  openTab: (server) => {
    const key = `${server.id}-${Date.now()}-${tabSeq++}`;
    const tab: Tab = {
      key,
      serverId: server.id,
      sessionId: null,
      server,
      serverName: server.name,
      host: server.host,
      status: "connecting",
    };
    set((s) => ({ tabs: [...s.tabs, tab], activeTabKey: key }));
    return key;
  },

  closeTab: (key) => {
    const { tabs, activeTabKey, sessionToTab } = get();
    const tab = tabs.find((t) => t.key === key);
    if (!tab) return;
    const idx = tabs.findIndex((t) => t.key === key);
    const nextTabs = tabs.filter((t) => t.key !== key);
    // 切换活动标签
    let nextActive = activeTabKey;
    if (activeTabKey === key) {
      const neighbor = nextTabs[Math.min(idx, nextTabs.length - 1)];
      nextActive = neighbor ? neighbor.key : null;
    }
    const st = { ...sessionToTab };
    if (tab.sessionId != null) delete st[tab.sessionId];
    set({ tabs: nextTabs, activeTabKey: nextActive, sessionToTab: st });
  },

  setActiveTab: (key) => set({ activeTabKey: key }),

  setSession: (key, sessionId) => {
    const { sessionToTab } = get();
    set((s) => ({
      tabs: s.tabs.map((t) => (t.key === key ? { ...t, sessionId } : t)),
      sessionToTab: { ...sessionToTab, [sessionId]: key },
    }));
  },

  setTabStatus: (key, status, error) => {
    set((s) => ({
      tabs: s.tabs.map((t) =>
        t.key === key ? { ...t, status, error: error ?? t.error } : t
      ),
    }));
  },

  disconnectTab: async (key) => {
    const { tabs, sessionToTab } = get();
    const tab = tabs.find((t) => t.key === key);
    if (!tab || tab.sessionId == null) return;
    await apiDisconnect(tab.sessionId);
    const st = { ...sessionToTab };
    delete st[tab.sessionId];
    set((s) => ({
      tabs: s.tabs.map((t) =>
        t.key === key
          ? { ...t, sessionId: null, status: "closed" as const }
          : t
      ),
      sessionToTab: st,
    }));
  },

  requestReconnect: (key) => {
    set((s) => ({
      reconnectSeq: {
        ...s.reconnectSeq,
        [key]: (s.reconnectSeq[key] ?? 0) + 1,
      },
    }));
  },
}));
