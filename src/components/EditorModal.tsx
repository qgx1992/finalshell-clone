import { useEffect, useRef, useState } from "react";
import { EditorView, basicSetup } from "codemirror";
import { oneDark } from "@codemirror/theme-one-dark";
import { EditorState } from "@codemirror/state";
import { LanguageDescription } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { languages } from "@codemirror/language-data";
import { sftpReadFile, sftpWriteFile } from "../api";

interface Props {
  sessionId: number;
  path: string;
  onClose: () => void;
}

function langFor(path: string): Extension {
  const name = path.split("/").pop() ?? path;
  const desc = LanguageDescription.matchFilename(languages, name);
  if (desc) {
    try {
      const support = desc.support;
      if (support) return support;
    } catch {
      /* fallthrough */
    }
  }
  return [];
}

export default function EditorModal({ sessionId, path, onClose }: Props) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    let cancelled = false;
    sftpReadFile(sessionId, path)
      .then((b64) => {
        if (cancelled) return;
        const text = decodeURIComponent(escape(atob(b64)));
        const state = EditorState.create({
          doc: text,
          extensions: [
            basicSetup,
            oneDark,
            EditorView.lineWrapping,
            EditorView.updateListener.of((u) => {
              if (u.docChanged) setSaved(false);
            }),
            langFor(path),
          ],
        });
        const view = new EditorView({ state, parent: wrapRef.current! });
        viewRef.current = view;
        setLoading(false);
      })
      .catch((e) => {
        if (!cancelled) {
          setError(String(e));
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
      viewRef.current?.destroy();
      viewRef.current = null;
    };
  }, [sessionId, path]);

  const doSave = async () => {
    const text = viewRef.current?.state.doc.toString() ?? "";
    setSaving(true);
    setError(null);
    try {
      const b64 = btoa(unescape(encodeURIComponent(text)));
      await sftpWriteFile(sessionId, path, b64);
      setSaved(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal editor-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <span className="editor-path">📝 {path}</span>
          <div className="editor-actions">
            {saved && <span className="editor-saved">✓ 已保存</span>}
            <button className="btn btn-primary" onClick={doSave} disabled={saving || loading}>
              {saving ? "保存中…" : "保存 (Ctrl+S)"}
            </button>
            <button className="modal-close" onClick={onClose}>×</button>
          </div>
        </div>
        <div className="editor-body">
          {loading && <div className="editor-loading">加载中…</div>}
          {error && <div className="editor-error">{error}</div>}
          <div ref={wrapRef} className="editor-wrap" />
        </div>
        <div className="modal-footer">
          <span className="editor-hint">远程文件直接编辑,保存即写回服务器。</span>
        </div>
      </div>
    </div>
  );
}
