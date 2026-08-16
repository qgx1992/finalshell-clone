import { Suspense, lazy, useEffect, useMemo, useState } from "react";
import { useApp } from "./store/app";
import Sidebar from "./components/Sidebar";
import TabBar from "./components/TabBar";
import TerminalPanel from "./components/TerminalPanel";
import SftpPanel from "./components/SftpPanel";
import MonitorPanel from "./components/MonitorPanel";
import ConnectionDialog from "./components/ConnectionDialog";
import QuickCommandPanel from "./components/QuickCommandPanel";
import SettingsModal from "./components/SettingsModal";
import { TERM_SCHEMES } from "./themes";
import type { ServerConfig } from "./types";

// EditorModal 含 CodeMirror 全家桶,懒加载避免拖慢首屏
const EditorModal = lazy(() => import("./components/EditorModal"));

export default function App() {
  const init = useApp((s) => s.init);
  const tabs = useApp((s) => s.tabs);
  const activeTabKey = useApp((s) => s.activeTabKey);
  const closeTab = useApp((s) => s.closeTab);
  const settings = useApp((s) => s.settings);

  const [dialog, setDialog] = useState<{ mode: "new" } | { mode: "edit"; server: ServerConfig } | null>(null);
  const [showMonitor, setShowMonitor] = useState(true);
  const [showSettings, setShowSettings] = useState(false);
  const [editingFile, setEditingFile] = useState<{ sessionId: number; path: string } | null>(null);
  const [sftpCollapsed, setSftpCollapsed] = useState(false);
  const [qcpOpen, setQcpOpen] = useState(false);

  useEffect(() => {
    init().catch(console.error);
  }, [init]);

  const activeTab = useMemo(
    () => tabs.find((t) => t.key === activeTabKey) ?? null,
    [tabs, activeTabKey]
  );

  const scheme = useMemo(
    () => TERM_SCHEMES.find((t) => t.name === settings.color_scheme) ?? TERM_SCHEMES[0],
    [settings.color_scheme]
  );

  const onCloseTab = (key: string) => closeTab(key);

  return (
    <div
      className={`app ${settings.theme === "light" ? "theme-light" : "theme-dark"}`}
      onContextMenu={(e) => e.preventDefault()}
    >
      <header className="app-header">
        <div className="logo">
          <span className="logo-mark">▮▮</span>
          <span className="logo-text">FinalShell Clone</span>
        </div>
        <div className="header-actions">
          <button className="btn btn-primary" onClick={() => setDialog({ mode: "new" })}>
            ➕ 新建连接
          </button>
          <button className="btn" onClick={() => setQcpOpen((v) => !v)} title="快捷命令">
            ⚡ 命令
          </button>
          <button className="btn" onClick={() => setShowMonitor((v) => !v)} title="监控面板">
            📊 监控
          </button>
          <button className="btn" onClick={() => setShowSettings(true)} title="设置">
            ⚙️
          </button>
        </div>
      </header>

      <div className="app-body">
        <aside className="app-sidebar">
          <Sidebar
            onNewServer={() => setDialog({ mode: "new" })}
            onEditServer={(s) => setDialog({ mode: "edit", server: s })}
          />
          {qcpOpen && (
            <div className="sidebar-qcp">
              <QuickCommandPanel
                sessionId={activeTab?.sessionId ?? null}
                serverName={activeTab?.serverName ?? ""}
              />
            </div>
          )}
        </aside>

        <main className="app-main">
          <TabBar onCloseTab={onCloseTab} />
          {tabs.length > 0 ? (
            <div className="workspace">
              {/* 所有标签的终端常驻挂载,仅切换可见性 —— 避免切标签断开重连 */}
              {tabs.map((t) => {
                const isActive = t.key === activeTabKey;
                return (
                  <div key={t.key} className={`tab-workspace ${isActive ? "active" : ""}`}>
                    <div className="terminal-area">
                      <TerminalPanel
                        tabKey={t.key}
                        server={t.server}
                        scheme={scheme}
                        fontSize={settings.font_size}
                        active={isActive}
                      />
                      {t.status === "closed" && (
                        <div className="terminal-reconnect">
                          <span>连接已断开</span>
                          <button className="btn btn-primary" onClick={() => closeTab(t.key)}>
                            关闭标签
                          </button>
                        </div>
                      )}
                    </div>
                    {t.sessionId != null && (
                      <SftpPanel
                        sessionId={t.sessionId}
                        collapsed={sftpCollapsed}
                        onToggle={() => setSftpCollapsed((v) => !v)}
                        onEditFile={(path) =>
                          setEditingFile({ sessionId: t.sessionId!, path })
                        }
                      />
                    )}
                  </div>
                );
              })}
            </div>
          ) : (
            <div className="welcome">
              <div className="welcome-mark">▮▮</div>
              <h1>FinalShell Clone</h1>
              <p>SSH 终端 · SFTP 文件管理 · 服务器监控 一体化工具</p>
              <button className="btn btn-primary" onClick={() => setDialog({ mode: "new" })}>
                ➕ 新建连接
              </button>
            </div>
          )}
        </main>

        {showMonitor && activeTab?.sessionId != null && (
          <aside className="app-monitor">
            <MonitorPanel
              sessionId={activeTab.sessionId}
              serverName={activeTab.serverName}
              onClose={() => setShowMonitor(false)}
            />
          </aside>
        )}
      </div>

      <footer className="app-statusbar">
        <span>
          {activeTab
            ? `${activeTab.serverName} · ${activeTab.host} · ${activeTab.status}`
            : "未连接"}
        </span>
        <span className="status-spacer" />
        <span>{tabs.length} 个会话</span>
        <span>v0.1.0</span>
      </footer>

      {dialog && (
        <ConnectionDialog
          initial={dialog.mode === "edit" ? dialog.server : null}
          onClose={() => setDialog(null)}
        />
      )}
      {showSettings && <SettingsModal onClose={() => setShowSettings(false)} />}
      {editingFile && (
        <Suspense fallback={null}>
          <EditorModal
            sessionId={editingFile.sessionId}
            path={editingFile.path}
            onClose={() => setEditingFile(null)}
          />
        </Suspense>
      )}
    </div>
  );
}
