import { useState } from "react";
import { TERM_SCHEMES } from "../themes";
import { useApp } from "../store/app";
import type { ServerConfig } from "../types";

interface Props {
  onClose: () => void;
}

export default function SettingsModal({ onClose }: Props) {
  const settings = useApp((s) => s.settings);
  const updateSettings = useApp((s) => s.updateSettings);

  const [theme, setTheme] = useState(settings.theme);
  const [scheme, setScheme] = useState(settings.color_scheme);
  const [fontSize, setFontSize] = useState(settings.font_size);

  const apply = () => {
    updateSettings({ theme, color_scheme: scheme, font_size: fontSize });
    onClose();
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
