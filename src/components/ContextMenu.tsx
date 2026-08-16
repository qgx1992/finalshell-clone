import { useEffect, useRef, useState } from "react";

export interface MenuItem {
  label?: string;
  icon?: string;
  danger?: boolean;
  disabled?: boolean;
  onClick?: () => void;
  divider?: boolean;
}

export interface CtxMenuState {
  x: number;
  y: number;
  items: MenuItem[];
}

interface Props {
  menu: CtxMenuState | null;
  onClose: () => void;
}

/** 通用右键菜单(自动贴边 + 点击外部关闭 + Esc 关闭) */
export default function ContextMenu({ menu, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x: 0, y: 0 });

  useEffect(() => {
    if (!menu) return;
    const el = ref.current;
    if (!el) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    const { innerWidth, innerHeight } = window;
    let x = menu.x;
    let y = menu.y;
    if (x + w > innerWidth - 8) x = Math.max(4, innerWidth - w - 8);
    if (y + h > innerHeight - 8) y = Math.max(4, innerHeight - h - 8);
    setPos({ x, y });
  }, [menu]);

  useEffect(() => {
    if (!menu) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [menu, onClose]);

  if (!menu) return null;
  return (
    <>
      <div className="ctx-backdrop" onClick={onClose} onContextMenu={(e) => { e.preventDefault(); onClose(); }} />
      <div
        ref={ref}
        className="context-menu"
        style={{ left: pos.x, top: pos.y }}
        onClick={(e) => e.stopPropagation()}
      >
        {menu.items.map((it, i) =>
          it.divider ? (
            <div className="ctx-divider" key={i} />
          ) : (
            <div
              key={i}
              className={`ctx-item ${it.danger ? "ctx-danger" : ""} ${it.disabled ? "ctx-disabled" : ""}`}
              onClick={() => {
                onClose();
                if (!it.disabled && it.onClick) it.onClick();
              }}
            >
              {it.icon && <span className="ctx-icon">{it.icon}</span>}
              {it.label}
            </div>
          )
        )}
      </div>
    </>
  );
}