import { useMemo, useState } from "react";
import { connectRdp } from "../api";
import { useApp } from "../store/app";
import type { ServerConfig } from "../types";
import ContextMenu, { type CtxMenuState } from "./ContextMenu";

interface Props {
  onNewServer: () => void;
  onEditServer: (s: ServerConfig) => void;
}

export default function Sidebar({ onNewServer, onEditServer }: Props) {
  const servers = useApp((s) => s.servers);
  const settings = useApp((s) => s.settings);
  const openTab = useApp((s) => s.openTab);
  const removeServer = useApp((s) => s.removeServer);
  const setActiveTab = useApp((s) => s.setActiveTab);
  const updateSettings = useApp((s) => s.updateSettings);
  const tabs = useApp((s) => s.tabs);

  const [filter, setFilter] = useState("");
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set());
  const [menu, setMenu] = useState<CtxMenuState | null>(null);

  const copyToClipboard = (text: string) => {
    const ta = document.createElement("textarea");
    ta.value = text;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    document.body.removeChild(ta);
  };

  const groups = useMemo(() => {
    const map = new Map<string, ServerConfig[]>();
    for (const s of servers) {
      const g = s.group || "默认分组";
      if (!map.has(g)) map.set(g, []);
      map.get(g)!.push(s);
    }
    return [...map.entries()].sort((a, b) => a[0].localeCompare(b[0], "zh"));
  }, [servers]);

  const filteredGroups = useMemo(() => {
    if (!filter) return groups;
    return groups
      .map(([g, list]) => [
        g,
        list.filter(
          (s) =>
            s.name.toLowerCase().includes(filter.toLowerCase()) ||
            s.host.toLowerCase().includes(filter.toLowerCase()) ||
            s.username.toLowerCase().includes(filter.toLowerCase())
        ),
      ] as [string, ServerConfig[]])
      .filter(([, list]) => list.length > 0);
  }, [groups, filter]);

  const toggleGroup = (g: string) => {
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(g)) next.delete(g);
      else next.add(g);
      return next;
    });
  };

  const connect = (s: ServerConfig) => {
    if (s.conn_type === "rdp") {
      // 远程 Windows:拉起系统远程桌面,不建 SSH 标签
      connectRdp(s).catch((e) => alert(`拉起远程桌面失败: ${e}`));
      updateSettings({
        last_servers: [s.id, ...(settings.last_servers ?? []).filter((id) => id !== s.id)].slice(0, 8),
      });
      return;
    }
    const key = openTab(s);
    setActiveTab(key);
  };

  const recentServers = useMemo(() => {
    const map = new Map(servers.map((s) => [s.id, s]));
    return settings.last_servers
      .map((id) => map.get(id))
      .filter((s): s is ServerConfig => !!s)
      .slice(0, 8);
  }, [servers, settings.last_servers]);

  return (
    <div className="sidebar" onContextMenu={(e) => e.preventDefault()}>
      <div className="sidebar-search">
        <input
          placeholder="🔍 搜索服务器…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
      </div>

      <div className="sidebar-section">
        <div className="sidebar-section-title" onClick={onNewServer}>
          ➕ 新建连接
        </div>
      </div>

      {recentServers.length > 0 && (
        <div className="sidebar-section">
          <div className="sidebar-section-title">🕘 最近连接</div>
          {recentServers.map((s) => (
            <div
              key={s.id}
              className="sidebar-server sidebar-recent"
              onClick={() => connect(s)}
              title={`${s.username}@${s.host}:${s.port}`}
            >
              <span className="sv-icon">🖥</span>
              <span className="sv-name">{s.name}</span>
            </div>
          ))}
        </div>
      )}

      <div className="sidebar-section sidebar-grow">
        <div className="sidebar-section-title">
          <span>🗂 我的服务器</span>
          <span className="sv-count">{servers.length}</span>
        </div>
        {filteredGroups.length === 0 && (
          <div className="sidebar-empty">
            {servers.length === 0 ? "还没有服务器,点击上方新建连接" : "无匹配结果"}
          </div>
        )}
        {filteredGroups.map(([group, list]) => (
          <div key={group}>
            <div className="sidebar-group" onClick={() => toggleGroup(group)}>
              <span className={`sg-arrow ${collapsedGroups.has(group) ? "" : "sg-open"}`}>▸</span>
              <span className="sg-name">{group}</span>
              <span className="sv-count">{list.length}</span>
            </div>
            {!collapsedGroups.has(group) &&
              list.map((s) => {
                const isActive = tabs.some(
                  (t) => t.serverId === s.id && (t.status === "connected" || t.status === "connecting")
                );
                return (
                  <div
                    key={s.id}
                    className={`sidebar-server ${isActive ? "sv-active" : ""}`}
                    onClick={() => connect(s)}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      setMenu({
                        x: e.clientX,
                        y: e.clientY,
                        items: [
                          { icon: "⚡", label: "连接", onClick: () => connect(s) },
                          { icon: "✏", label: "编辑", onClick: () => onEditServer(s) },
                          {
                            icon: "🔗",
                            label: "复制连接",
                            onClick: () => copyToClipboard(`${s.username}@${s.host}:${s.port}`),
                          },
                          { divider: true },
                          {
                            icon: "🗑",
                            label: "删除",
                            danger: true,
                            onClick: () => {
                              if (confirm(`确定删除服务器 "${s.name}"?`)) {
                                removeServer(s.id);
                              }
                            },
                          },
                        ],
                      });
                    }}
                    title={`${s.username}@${s.host}:${s.port}${s.conn_type === "rdp" ? " (RDP)" : ""}`}
                  >
                    <span className="sv-icon">
                      {isActive ? "🟢" : s.conn_type === "rdp" ? "🖥️" : "🖥"}
                    </span>
                    <span className="sv-name">
                      {s.name}
                      {s.conn_type === "rdp" && (
                        <span className="sv-badge" title="远程 Windows (RDP)">RDP</span>
                      )}
                    </span>
                    <span className="sv-host">{s.host}</span>
                  </div>
                );
              })}
          </div>
        ))}
      </div>

      <ContextMenu menu={menu} onClose={() => setMenu(null)} />
    </div>
  );
}
