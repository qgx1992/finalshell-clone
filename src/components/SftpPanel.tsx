import { useCallback, useEffect, useRef, useState } from "react";
import { open as dialogOpen, save as dialogSave } from "@tauri-apps/plugin-dialog";
import {
  listDir,
  sftpDelete,
  sftpDownload,
  sftpMkdir,
  sftpPwd,
  sftpRename,
  sftpUpload,
} from "../api";
import type { FileEntry } from "../types";
import { onSftpProgress } from "../events/terminalRouter";
import ContextMenu, { type CtxMenuState } from "./ContextMenu";

interface Props {
  sessionId: number;
  collapsed: boolean;
  onToggle: () => void;
  onEditFile?: (path: string) => void;
}

function fmtSize(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(1)} ${units[i]}`;
}

function fmtTime(secs: number): string {
  if (!secs) return "-";
  const d = new Date(secs * 1000);
  return d.toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

interface Transfer {
  kind: string;
  name: string;
  written: number;
  total: number;
}

export default function SftpPanel({ sessionId, collapsed, onToggle, onEditFile }: Props) {
  const [cwd, setCwd] = useState("");
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [transfers, setTransfers] = useState<Transfer[]>([]);
  const [history, setHistory] = useState<string[]>([]);
  const [pathInput, setPathInput] = useState("");
  const [editingPath, setEditingPath] = useState(false);
  const [menu, setMenu] = useState<CtxMenuState | null>(null);
  const mounted = useRef(true);

  const load = useCallback(
    async (path: string) => {
      if (!mounted.current) return;
      setLoading(true);
      setError(null);
      try {
        const entries = await listDir(sessionId, path);
        if (mounted.current) {
          setFiles(entries);
          setCwd(path);
          setPathInput(path);
          setSelected(null);
        }
      } catch (e) {
        if (mounted.current) setError(String(e));
      } finally {
        if (mounted.current) setLoading(false);
      }
    },
    [sessionId]
  );

  // 初始加载
  useEffect(() => {
    mounted.current = true;
    sftpPwd(sessionId)
      .then((p) => load(p))
      .catch(() => load("/"));
    return () => {
      mounted.current = false;
    };
  }, [sessionId, load]);

  // 传输进度:通过全局路由订阅,只接收本会话的事件
  useEffect(() => {
    const unsubscribe = onSftpProgress(sessionId, (payload) => {
      const [, kind, src, , written, total] = payload;
      const name = src.split(/[\\/]/).pop() ?? src;
      setTransfers((prev) => {
        const others = prev.filter((t) => t.name !== name || t.kind !== kind);
        if (written >= total && total > 0) return others; // 完成移除
        return [...others, { kind, name, written, total }];
      });
    });
    return unsubscribe;
  }, [sessionId]);

  const enterDir = (fe: FileEntry) => {
    if (!fe.is_dir) return;
    setHistory((h) => [...h, cwd]);
    load(fe.path);
  };

  const goUp = () => {
    if (!cwd || cwd === "/") return;
    const parent = cwd.endsWith("/") ? cwd.slice(0, -1) : cwd;
    const idx = parent.lastIndexOf("/");
    const up = idx <= 0 ? "/" : parent.slice(0, idx);
    setHistory((h) => [...h, cwd]);
    load(up);
  };

  const goBack = () => {
    const h = [...history];
    const prev = h.pop();
    if (prev != null) {
      setHistory(h);
      load(prev);
    }
  };

  const doUpload = async () => {
    const paths = await dialogOpen({
      multiple: true,
      title: "选择要上传的文件",
    });
    if (!paths) return;
    const list = Array.isArray(paths) ? paths : [paths];
    for (const lp of list) {
      const name = lp.split(/[\\/]/).pop() ?? "file";
      const target = cwd.endsWith("/") ? `${cwd}${name}` : `${cwd}/${name}`;
      try {
        await sftpUpload(sessionId, lp, target);
        await load(cwd);
      } catch (e) {
        setError(String(e));
      }
    }
  };

  const doDownload = async (fe: FileEntry) => {
    if (fe.is_dir) return;
    const dest = await dialogSave({
      title: "保存到本地",
      defaultPath: fe.name,
    });
    if (!dest) return;
    try {
      await sftpDownload(sessionId, fe.path, dest);
    } catch (e) {
      setError(String(e));
    }
  };

  const doMkdir = async () => {
    const name = prompt("输入新目录名称:");
    if (!name) return;
    const target = cwd.endsWith("/") ? `${cwd}${name}` : `${cwd}/${name}`;
    try {
      await sftpMkdir(sessionId, target);
      await load(cwd);
    } catch (e) {
      setError(String(e));
    }
  };

  const doDelete = async (fe: FileEntry) => {
    if (!confirm(`确定删除 ${fe.is_dir ? "目录" : "文件"} "${fe.name}"?\n目录将递归删除!`)) return;
    try {
      await sftpDelete(sessionId, fe.path);
      await load(cwd);
    } catch (e) {
      setError(String(e));
    }
  };

  const doRename = async (fe: FileEntry) => {
    const name = prompt("新名称:", fe.name);
    if (!name || name === fe.name) return;
    const parent = fe.path.slice(0, fe.path.lastIndexOf("/") + 1);
    try {
      await sftpRename(sessionId, fe.path, `${parent}${name}`);
      await load(cwd);
    } catch (e) {
      setError(String(e));
    }
  };

  const onPathCommit = () => {
    setEditingPath(false);
    load(pathInput || "/");
  };

  const showRowMenu = (e: React.MouseEvent, fe: FileEntry) => {
    e.preventDefault();
    e.stopPropagation();
    setSelected(fe.path);
    setMenu({
      x: e.clientX,
      y: e.clientY,
      items: fe.is_dir
        ? [
            { icon: "📂", label: "进入目录", onClick: () => enterDir(fe) },
            { divider: true },
            { icon: "✏", label: "重命名", onClick: () => doRename(fe) },
            {
              icon: "🗑",
              label: "删除",
              danger: true,
              onClick: () => doDelete(fe),
            },
          ]
        : [
            { icon: "⬇", label: "下载", onClick: () => doDownload(fe) },
            { icon: "📝", label: "编辑", onClick: () => onEditFile && onEditFile(fe.path) },
            { divider: true },
            { icon: "✏", label: "重命名", onClick: () => doRename(fe) },
            {
              icon: "🗑",
              label: "删除",
              danger: true,
              onClick: () => doDelete(fe),
            },
          ],
    });
  };

  const showBlankMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { icon: "⬆", label: "上传文件", onClick: () => doUpload() },
        { icon: "📁", label: "新建目录", onClick: () => doMkdir() },
        { divider: true },
        { icon: "⟳", label: "刷新", onClick: () => load(cwd) },
      ],
    });
  };

  const transferTotal = transfers.reduce((a, t) => a + (t.total || 0), 0);
  const transferDone = transfers.reduce((a, t) => a + t.written, 0);

  if (collapsed) {
    return (
      <div className="sftp-collapsed" onClick={onToggle}>
        📂 文件管理
        {transferTotal > 0 && (
          <span className="sftp-mini-progress">
            {Math.round((transferDone / transferTotal) * 100)}%
          </span>
        )}
      </div>
    );
  }

  return (
    <div className="sftp-panel">
      <div className="sftp-toolbar">
        <span className="sftp-title" onClick={onToggle} title="折叠">
          📂 文件管理
        </span>
        <button title="返回上一级" onClick={goUp}>⬆</button>
        <button title="后退" onClick={goBack} disabled={history.length === 0}>←</button>
        <button title="刷新" onClick={() => load(cwd)} disabled={loading}>⟳</button>
        {editingPath ? (
          <input
            className="sftp-path-input"
            value={pathInput}
            autoFocus
            onChange={(e) => setPathInput(e.target.value)}
            onBlur={onPathCommit}
            onKeyDown={(e) => e.key === "Enter" && onPathCommit()}
          />
        ) : (
          <span className="sftp-path" onClick={() => setEditingPath(true)} title={cwd}>
            {cwd}
          </span>
        )}
        <span className="sftp-spacer" />
        <button title="上传" onClick={doUpload}>⬆ 上传</button>
        <button title="新建目录" onClick={doMkdir}>📁 新建</button>
        {selected && (
          <>
            <button title="下载选中" onClick={() => {
              const fe = files.find((f) => f.path === selected);
              if (fe) doDownload(fe);
            }}>⬇ 下载</button>
            <button
              title="删除选中"
              onClick={() => {
                const fe = files.find((f) => f.path === selected);
                if (fe) doDelete(fe);
              }}
            >
              🗑 删除
            </button>
            <button
              title="重命名选中"
              onClick={() => {
                const fe = files.find((f) => f.path === selected);
                if (fe) doRename(fe);
              }}
            >
              ✏ 重命名
            </button>
          </>
        )}
      </div>

      {error && (
        <div className="sftp-error">
          {error}
          <button onClick={() => setError(null)}>×</button>
        </div>
      )}

      <div className="sftp-table-wrap" onContextMenu={showBlankMenu}>
        <table className="sftp-table">
          <thead>
            <tr>
              <th style={{ width: 40 }}>类型</th>
              <th>名称</th>
              <th style={{ width: 110 }}>大小</th>
              <th style={{ width: 170 }}>修改时间</th>
              <th style={{ width: 100 }}>权限</th>
            </tr>
          </thead>
          <tbody>
            {cwd && cwd !== "/" && (
              <tr className="sftp-row sftp-up" onDoubleClick={goUp} onClick={() => setSelected(null)}>
                <td>📁</td>
                <td>..</td>
                <td />
                <td />
                <td />
              </tr>
            )}
            {files.map((fe) => (
              <tr
                key={fe.path}
                className={`sftp-row ${selected === fe.path ? "sftp-selected" : ""}`}
                onClick={() => setSelected(fe.path)}
                onDoubleClick={() => (fe.is_dir ? enterDir(fe) : onEditFile ? onEditFile(fe.path) : doDownload(fe))}
                onContextMenu={(e) => showRowMenu(e, fe)}
              >
                <td>{fe.is_dir ? "📁" : "📄"}</td>
                <td className="sftp-name" title={fe.path}>
                  {fe.name}
                  {fe.is_symlink && <span className="sftp-link"> →</span>}
                </td>
                <td>{fe.is_dir ? "-" : fmtSize(fe.size)}</td>
                <td>{fmtTime(fe.modified)}</td>
                <td>{fe.permissions}</td>
              </tr>
            ))}
            {!loading && files.length === 0 && (
              <tr>
                <td colSpan={5} className="sftp-empty">
                  空目录
                </td>
              </tr>
            )}
          </tbody>
        </table>
        {loading && <div className="sftp-loading">加载中…</div>}
      </div>

      {transfers.length > 0 && (
        <div className="sftp-transfers">
          {transfers.map((t) => (
            <div className="transfer-item" key={`${t.kind}-${t.name}`}>
              <span>
                {t.kind === "upload" ? "⬆" : "⬇"} {t.name}
              </span>
              <div className="transfer-bar">
                <div
                  className="transfer-fill"
                  style={{ width: `${t.total ? (t.written / t.total) * 100 : 0}%` }}
                />
              </div>
              <span className="transfer-pct">
                {t.total ? `${Math.round((t.written / t.total) * 100)}%` : "…"}
              </span>
            </div>
          ))}
        </div>
      )}
      <ContextMenu menu={menu} onClose={() => setMenu(null)} />
    </div>
  );
}
