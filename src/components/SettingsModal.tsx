import { useState } from "react";
import { TERM_SCHEMES } from "../themes";
import { useApp } from "../store/app";
import { forgetHostKey, trustedHostFingerprint } from "../api";
import type { HostKeyObservation } from "../types";

interface Props {
  onClose: () => void;
}

export default function SettingsModal({ onClose }: Props) {
  const settings = useApp((s) => s.settings);
  const servers = useApp((s) => s.servers);
  const updateSettings = useApp((s) => s.updateSettings);

  const [theme, setTheme] = useState(settings.theme);
  const [scheme, setScheme] = useState(settings.color_scheme);
  const [fontSize, setFontSize] = useState(settings.font_size);
  const [scrollback, setScrollback] = useState(settings.scrollback ?? 5000);

  // 主机信任管理:按需查询,避免打开设置时批量访问文件
  const [trusted, setTrusted] = useState<Record<string, HostKeyObservation | null>>({});
  const [checking, setChecking] = useState(false);

  const sshServers = servers.filter((s) => s.conn_type !== "rdp");

  const apply = () => {
    updateSettings({
      theme,
      color_scheme: scheme,
      font_size: fontSize,
      scrollback,
    });
    onClose();
  };

  const loadTrusted = async () => {
    setChecking(true);
    const entries = await Promise.all(
      sshServers.map(async (s) => {
        try {
          const obs = await trustedHostFingerprint(s.host, s.port);
          return [s.id, obs] as const;
        } catch {
          return [s.id, null] as const;
        }
      })
    );
    setTrusted(Object.fromEntries(entries));
    setChecking(false);
  };

  const forget = async (host: string, port: number, id: string) => {
    await forgetHostKey(host, port);
    setTrusted((prev) => ({ ...prev, [id]: null }));
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal settings-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <span>⚙️ 设置</span>
          <button className="modal-close" onClick={onClose}>×</button>
        </div>
        <div className="modal-body">
          <label className="form-full">
            界面主题
            <select value={theme} onChange={(e) => setTheme(e.target.value as "dark" | "light")}>
              <option value="dark">深色</option>
              <option value="light">浅色</option>
            </select>
          </label>
          <label className="form-full">
            终端配色方案 ({TERM_SCHEMES.length} 套)
            <select value={scheme} onChange={(e) => setScheme(e.target.value)}>
              {TERM_SCHEMES.map((t) => (
                <option key={t.name} value={t.name}>
                  {t.name}
                </option>
              ))}
            </select>
          </label>
          <label className="form-full">
            终端字号
            <div className="font-size-row">
              <input
                type="range"
                min={10}
                max={24}
                value={fontSize}
                onChange={(e) => setFontSize(parseInt(e.target.value))}
              />
              <span>{fontSize}px</span>
            </div>
          </label>
          <label className="form-full">
            终端回滚行数
            <div className="font-size-row">
              <input
                type="range"
                min={1000}
                max={50000}
                step={1000}
                value={scrollback}
                onChange={(e) => setScrollback(parseInt(e.target.value))}
              />
              <span>{scrollback} 行</span>
            </div>
          </label>
          <div className="form-hint">
            回滚行数越大越占内存。多个标签同时开启时,建议 3000~8000 行。
          </div>

          <div className="settings-section">
            <div className="settings-section-head">
              <span>🔐 主机信任(known_hosts)</span>
              <button className="btn-link" onClick={() => void loadTrusted()}>
                {checking ? "查询中…" : "查看已信任主机"}
              </button>
            </div>
            {Object.keys(trusted).length > 0 && (
              <div className="trusted-list">
                {sshServers.map((s) => {
                  const obs = trusted[s.id];
                  return (
                    <div className="trusted-row" key={s.id}>
                      <div className="trusted-info">
                        <div className="trusted-host">{s.name} ({s.host}:{s.port})</div>
                        <div className="trusted-fp">
                          {obs ? obs.fingerprint : "未记录(首次连接时确认)"}
                        </div>
                      </div>
                      {obs && (
                        <button
                          className="btn-link btn-danger-link"
                          onClick={() => void forget(s.host, s.port, s.id)}
                        >
                          清除信任
                        </button>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </div>

          <div className="scheme-preview">
            {TERM_SCHEMES.filter((t) => t.name === scheme).map((t) => (
              <div
                key={t.name}
                className="scheme-swatch"
                style={{
                  background: t.theme.background as string,
                  color: t.theme.foreground as string,
                }}
              >
                <span>user@server:~$ </span>
                <span style={{ color: t.theme.brightGreen as string }}>ls -la</span>
                <br />
                <span style={{ color: t.theme.brightBlue as string }}>drwxr-xr-x</span> apps{" "}
                <span style={{ color: t.theme.brightRed as string }}>conf.d</span> main.py
              </div>
            ))}
          </div>
        </div>
        <div className="modal-footer">
          <button className="btn" onClick={onClose}>取消</button>
          <button className="btn btn-primary" onClick={apply}>应用</button>
        </div>
      </div>
    </div>
  );
}
