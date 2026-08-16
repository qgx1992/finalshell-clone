import { useState } from "react";
import { useApp } from "../store/app";
import ContextMenu, { type CtxMenuState } from "./ContextMenu";

interface Props {
  onCloseTab: (key: string) => void;
}

const STATUS_ICON: Record<string, string> = {
  connecting: "⏳",
  connected: "🟢",
  closed: "🔴",
};

export default function TabBar({ onCloseTab }: Props) {
  const tabs = useApp((s) => s.tabs);
  const activeTabKey = useApp((s) => s.activeTabKey);
  const setActiveTab = useApp((s) => s.setActiveTab);
  const closeTab = useApp((s) => s.closeTab);
  const disconnectTab = useApp((s) => s.disconnectTab);
  const requestReconnect = useApp((s) => s.requestReconnect);
  const [menu, setMenu] = useState<CtxMenuState | null>(null);

  const closeOthers = (key: string) => {
    for (const t of tabs) {
      if (t.key !== key) onCloseTab(t.key);
    }
  };

  return (
    <div className="tabbar">
      {tabs.map((t) => (
        <div
          key={t.key}
          className={`tab ${activeTabKey === t.key ? "tab-active" : ""}`}
          onClick={() => setActiveTab(t.key)}
          onContextMenu={(e) => {
            e.preventDefault();
            e.stopPropagation();
            setMenu({
              x: e.clientX,
              y: e.clientY,
              items: [
                // 连接中显示"连接中…"禁用;已连接 → 断开;断开 → 重新连接
                t.status === "connecting"
                  ? { icon: "⏳", label: "连接中…", disabled: true }
                  : t.status === "connected"
                    ? { icon: "🔌", label: "断开连接", onClick: () => disconnectTab(t.key) }
                    : { icon: "🔄", label: "重新连接", onClick: () => requestReconnect(t.key) },
                { divider: true },
                { icon: "⏹", label: "关闭", onClick: () => onCloseTab(t.key) },
                { divider: true },
                { icon: "❎", label: "关闭其他", onClick: () => closeOthers(t.key) },
                { icon: "🚫", label: "全部关闭", onClick: () => tabs.forEach((x) => onCloseTab(x.key)) },
              ],
            });
          }}
          title={`${t.host} · ${t.status}`}
        >
          <span className="tab-icon">{STATUS_ICON[t.status] ?? "🖥"}</span>
          <span className="tab-title">{t.serverName}</span>
          <button
            className="tab-close"
            onClick={(e) => {
              e.stopPropagation();
              onCloseTab(t.key);
            }}
          >
            ×
          </button>
        </div>
      ))}
      {tabs.length === 0 && <div className="tabbar-empty">欢迎使用 FinalShell Clone</div>}
      <ContextMenu menu={menu} onClose={() => setMenu(null)} />
    </div>
  );
}