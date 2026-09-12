import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { CanvasAddon } from "@xterm/addon-canvas";
import { onTermData, onTermExit } from "../events/terminalRouter";
import { ApiError, connectServer, disconnect, resizeTerminal, writeTerminal } from "../api";
import { useApp } from "../store/app";
import type { HostKeyObservation, ServerConfig } from "../types";
import { DEFAULT_SCHEME, type TermScheme } from "../themes";
import ContextMenu, { type CtxMenuState } from "./ContextMenu";
import HostKeyDialog from "./HostKeyDialog";

// ---------- 高效 base64 解码 ----------
// 查表 + 每 4 字符一组解码,比 Uint8Array.from(atob(), charCodeAt) 快;
// 终端高吞吐输出时减少主线程解码开销。
// 注意:组内 acc 最多 24 bits,避免 JS 左移(<<)按 int32 截断导致位流错乱。
const B64_TABLE = new Int8Array(128).fill(-1);
for (let i = 0; i < 26; i++) {
  B64_TABLE[65 + i] = i; // A-Z
  B64_TABLE[97 + i] = 26 + i; // a-z
}
for (let i = 0; i < 10; i++) {
  B64_TABLE[48 + i] = 52 + i; // 0-9
}
B64_TABLE[43] = 62; // +
B64_TABLE[47] = 63; // /

function decodeBase64(input: string): Uint8Array {
  const len = input.length;
  // 宽松估算输出容量(实际长度由 subarray 截断),兼容无 padding 的短输入
  const out = new Uint8Array((len * 3) >> 2);
  let o = 0;
  let i = 0;
  while (i < len) {
    // 每组最多 4 个有效 base64 字符(24 bits → 3 字节),acc 不跨组累积
    let acc = 0;
    let n = 0;
    while (i < len && n < 4) {
      const code = input.charCodeAt(i++);
      if (code === 61) break; // "=" padding 结束
      const v = code < 128 ? B64_TABLE[code] : -1;
      if (v < 0) continue; // 跳过空白/非法字符
      acc = (acc << 6) | v;
      n++;
    }
    // n 个字符 → n*6 bits,从中提取完整字节(高到低)
    let remaining = n * 6;
    while (remaining >= 8) {
      remaining -= 8;
      out[o++] = (acc >> remaining) & 0xff;
    }
  }
  return out.subarray(0, o);
}

interface Props {
  tabKey: string;
  server: ServerConfig;
  scheme: TermScheme;
  fontSize: number;
  /** 终端回滚缓冲行数,后台标签越多越应调小 */
  scrollback: number;
  /** 是否为活动标签;切回活动时重新 fit(隐藏期间尺寸为 0) */
  active?: boolean;
}

export default function TerminalPanel({
  tabKey,
  server,
  scheme,
  fontSize,
  scrollback,
  active,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionIdRef = useRef<number | null>(null);
  const disposedRef = useRef(false);
  const [menu, setMenu] = useState<CtxMenuState | null>(null);
  // 待用户确认的主机指纹(首次连接或指纹变更)
  const [hostKeyPrompt, setHostKeyPrompt] = useState<{
    observation: HostKeyObservation;
    changed: boolean;
  } | null>(null);
  // 待用户确认后重试的连接参数:用户确认时复用同一份凭据
  const pendingRetryRef = useRef<(() => void) | null>(null);
  // 供标签页右键"重新连接"触发的连接函数(首个 effect 中定义)
  const doConnectRef = useRef<() => void>(() => {});

  const pasteFromClipboard = async () => {
    try {
      const text = await navigator.clipboard.readText();
      const id = sessionIdRef.current;
      if (id != null && text) {
        const b64 = btoa(unescape(encodeURIComponent(text)));
        writeTerminal(id, b64).catch(() => {});
      }
    } catch {
      const ta = document.createElement("textarea");
      document.body.appendChild(ta);
      ta.focus();
      document.execCommand("paste");
      const text = ta.value;
      document.body.removeChild(ta);
      const id = sessionIdRef.current;
      if (id != null && text) {
        const b64 = btoa(unescape(encodeURIComponent(text)));
        writeTerminal(id, b64).catch(() => {});
      }
    }
  };

  const setSession = useApp((s) => s.setSession);
  const clearSession = useApp((s) => s.clearSession);
  const setTabStatus = useApp((s) => s.setTabStatus);
  const canConnect = useApp((s) => s.canConnect);

  useEffect(() => {
    disposedRef.current = false;
    const container = containerRef.current!;
    const term = new Terminal({
      fontFamily: '"Cascadia Code", Consolas, "Courier New", monospace',
      fontSize,
      cursorBlink: true,
      scrollback,
      convertEol: false,
      theme: scheme.theme,
      allowProposedApi: true,
    });
    termRef.current = term;

    const fit = new FitAddon();
    fitRef.current = fit;
    term.loadAddon(fit);
    // Canvas renderer:大输出(日志/编译/cat)时远优于默认 DOM renderer
    term.loadAddon(new CanvasAddon());
    term.loadAddon(new WebLinksAddon());
    term.open(container);
    fit.fit();

    const handleResize = () => {
      if (termRef.current && sessionIdRef.current != null) {
        const dims = fit.proposeDimensions();
        if (dims) {
          resizeTerminal(sessionIdRef.current, dims.cols, dims.rows).catch(() => {});
        }
      }
      try {
        fit.fit();
      } catch {
        /* ignore */
      }
    };

    const handleTermCtx = (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const hasSel = !!term.getSelection();
      setMenu({
        x: e.clientX,
        y: e.clientY,
        items: [
          {
            icon: "📋",
            label: "复制",
            disabled: !hasSel,
            onClick: () => {
              const sel = term.getSelection();
              if (sel) navigator.clipboard.writeText(sel).catch(() => {});
            },
          },
          { icon: "📥", label: "粘贴", onClick: () => pasteFromClipboard() },
          { divider: true },
          { icon: "🧹", label: "清屏", onClick: () => term.clear() },
          { icon: "🔄", label: "重置终端", onClick: () => term.reset() },
        ],
      });
    };
    const ro = new ResizeObserver(() => handleResize());
    ro.observe(container);
    container.addEventListener("contextmenu", handleTermCtx);

    // 键盘输入 -> 后端(合并小批量发送,减少每击键一次的 IPC 往返)
    let inputBuf = "";
    let inputTimer: number | null = null;
    const flushInput = () => {
      inputTimer = null;
      if (!inputBuf) return;
      const b64 = btoa(unescape(encodeURIComponent(inputBuf)));
      inputBuf = "";
      const id = sessionIdRef.current;
      if (id == null) return;
      writeTerminal(id, b64).catch((e) => {
        term.writeln("\r\n\x1b[31m[写入失败] " + String(e) + "\x1b[0m");
      });
    };
    const dataDispose = term.onData((data) => {
      inputBuf += data;
      if (inputTimer == null) {
        inputTimer = window.setTimeout(flushInput, 12);
      }
    });

    // 事件订阅句柄:会话建立后注册到全局路由,断开/卸载时注销
    let unsubData: (() => void) | null = null;
    let unsubExit: (() => void) | null = null;

    // 事件订阅:交给全局路由按 sessionId 分发,避免每个标签都接收全部输出
    const decoder = new TextDecoder();
    const unsubscribeSession = () => {
      unsubData?.();
      unsubExit?.();
      unsubData = null;
      unsubExit = null;
    };
    const subscribeSession = (sid: number) => {
      unsubscribeSession();
      unsubData = onTermData(sid, (b64) => {
        if (disposedRef.current) return;
        try {
          const bytes = decodeBase64(b64);
          term.write(decoder.decode(bytes, { stream: true }));
        } catch {
          /* ignore */
        }
      });
      unsubExit = onTermExit(sid, (status, reason) => {
        if (disposedRef.current) return;
        if (status === "closed") {
          // 会话已失效:清空组件引用与全局状态,避免残留 sessionId
          sessionIdRef.current = null;
          unsubscribeSession();
          clearSession(tabKey);
          term.writeln("\r\n\x1b[90m[连接已断开] " + reason + "\x1b[0m");
        }
      });
    };

    // 连接逻辑:首次挂载与标签页右键"重新连接"共用
    const doConnect = (trust?: {
      decision: "trust_new" | "trust_changed";
      fingerprint: string;
    }) => {
      if (disposedRef.current) return;
      const hadSession = sessionIdRef.current != null; // 已断开过 → 属重连场景
      setTabStatus(tabKey, "connecting");
      connectServer(server, {
        hostKeyDecision: trust?.decision,
        hostKeyFingerprint: trust?.fingerprint,
      })
        .then((id) => {
          if (disposedRef.current) {
            return; // 组件已卸载
          }
          sessionIdRef.current = id;
          subscribeSession(id);
          setSession(tabKey, id);
          setTabStatus(tabKey, "connected");
          setHostKeyPrompt(null);
          pendingRetryRef.current = null;
          if (hadSession) {
            term.writeln("\r\n\x1b[32m[已连接]\x1b[0m");
          }
        })
        .catch((e) => {
          if (disposedRef.current) return;
          const err = e instanceof ApiError ? e : null;
          const body = err?.body;
          // 主机指纹待确认/已变更:弹窗让用户决定,不当作连接失败
          if (
            body &&
            (body.code === "host_key_unknown" || body.code === "host_key_changed")
          ) {
            const changed = body.code === "host_key_changed";
            const obs = body.observation;
            setHostKeyPrompt({ observation: obs, changed });
            pendingRetryRef.current = () =>
              doConnect({
                decision: changed ? "trust_changed" : "trust_new",
                fingerprint: obs.fingerprint,
              });
            setTabStatus(tabKey, "closed");
            return;
          }
          setTabStatus(tabKey, "closed", String(e));
          term.writeln("\r\n\x1b[31m[连接失败] " + String(e) + "\x1b[0m");
        });
    };
    doConnectRef.current = doConnect;
    doConnect();

    return () => {
      disposedRef.current = true;
      container.removeEventListener("contextmenu", handleTermCtx);
      ro.disconnect();
      dataDispose.dispose();
      // 卸载前把缓冲的输入发出去
      if (inputTimer != null) clearTimeout(inputTimer);
      flushInput();
      unsubscribeSession();
      const id = sessionIdRef.current;
      if (id != null) {
        disconnect(id).catch(() => {});
      }
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
      sessionIdRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tabKey]);

  // 主题/字号变化
  useEffect(() => {
    if (termRef.current) termRef.current.options.theme = scheme.theme;
  }, [scheme]);

  useEffect(() => {
    if (termRef.current) {
      termRef.current.options.fontSize = fontSize;
    }
  }, [fontSize]);

  // 标签页右键"重新连接":序号每次 +1,变化即重连(首次为 undefined 不触发)
  const reconnectSeq = useApp((s) => s.reconnectSeq[tabKey]);
  useEffect(() => {
    if (!reconnectSeq) return;
    // 防重入:已连接或连接中不重复发起
    if (!canConnect(tabKey)) return;
    doConnectRef.current();
  }, [reconnectSeq, tabKey, canConnect]);

  // 标签从隐藏切回活动:display:none 期间尺寸为 0,重新 fit 并同步远程窗口尺寸
  useEffect(() => {
    if (!active) return;
    const raf = requestAnimationFrame(() => {
      const container = containerRef.current;
      const fit = fitRef.current;
      if (!container || !fit || container.offsetParent == null) return;
      try {
        fit.fit();
      } catch {
        /* ignore */
      }
      const id = sessionIdRef.current;
      if (id != null) {
        const dims = fit.proposeDimensions();
        if (dims) resizeTerminal(id, dims.cols, dims.rows).catch(() => {});
      }
    });
    return () => cancelAnimationFrame(raf);
  }, [active]);

  return (
    <div
      ref={containerRef}
      className="terminal-host"
      style={{
        width: "100%",
        height: "100%",
        background: scheme.theme.background,
        padding: "4px 0 0 4px",
      }}
      onContextMenu={(e) => {
        e.preventDefault();
      }}
    >
      <ContextMenu menu={menu} onClose={() => setMenu(null)} />
      {hostKeyPrompt && (
        <HostKeyDialog
          observation={hostKeyPrompt.observation}
          changed={hostKeyPrompt.changed}
          onConfirm={() => {
            const retry = pendingRetryRef.current;
            setHostKeyPrompt(null);
            retry?.();
          }}
          onCancel={() => {
            setHostKeyPrompt(null);
            pendingRetryRef.current = null;
          }}
        />
      )}
    </div>
  );
}

export { DEFAULT_SCHEME };
