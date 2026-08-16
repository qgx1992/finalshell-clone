import { useState } from "react";
import { execCommand } from "../api";
import { useApp } from "../store/app";

interface Props {
  sessionId: number | null;
  serverName: string;
}

export default function QuickCommandPanel({ sessionId, serverName }: Props) {
  const settings = useApp((s) => s.settings);
  const updateSettings = useApp((s) => s.updateSettings);
  const [output, setOutput] = useState<{ cmd: string; text: string; err?: string } | null>(null);
  const [running, setRunning] = useState(false);
  const [adding, setAdding] = useState(false);
  const [newName, setNewName] = useState("");
  const [newCmd, setNewCmd] = useState("");

  const run = async (cmd: string) => {
    if (sessionId == null) {
      setOutput({ cmd, text: "", err: "请先连接服务器" });
      return;
    }
    setRunning(true);
    try {
      const text = await execCommand(sessionId, cmd);
      setOutput({ cmd, text });
    } catch (e) {
      setOutput({ cmd, text: "", err: String(e) });
    } finally {
      setRunning(false);
    }
  };

  const addCommand = async () => {
    if (!newName.trim() || !newCmd.trim()) return;
    await updateSettings({
      quick_commands: [...settings.quick_commands, { name: newName.trim(), command: newCmd.trim() }],
    });
    setNewName("");
    setNewCmd("");
    setAdding(false);
  };

  const removeCommand = async (idx: number) => {
    await updateSettings({
      quick_commands: settings.quick_commands.filter((_, i) => i !== idx),
    });
  };

  return (
    <div className="qcp">
      <div className="qcp-header">
        <span>⚡ 快捷命令</span>
        <button onClick={() => setAdding((a) => !a)} title="新增命令">
          ＋
        </button>
      </div>

      {adding && (
        <div className="qcp-add">
          <input
            placeholder="命令名称"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
          />
          <input
            placeholder="命令内容"
            value={newCmd}
            onChange={(e) => setNewCmd(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && addCommand()}
          />
          <button onClick={addCommand}>添加</button>
        </div>
      )}

      <div className="qcp-list">
        {settings.quick_commands.map((qc, i) => (
          <div key={i} className="qcp-item" title={qc.command}>
            <span className="qcp-run" onClick={() => run(qc.command)}>
              ▶ {qc.name}
            </span>
            <span className="qcp-del" onClick={() => removeCommand(i)}>
              ×
            </span>
          </div>
        ))}
        {settings.quick_commands.length === 0 && (
          <div className="qcp-empty">点击 ＋ 添加常用命令</div>
        )}
      </div>

      {output && (
        <div className="qcp-output">
          <div className="qcp-output-head">
            <span>
              {running ? "⏳ " : "📄 "}
              {output.cmd} @ {serverName}
            </span>
            <button onClick={() => setOutput(null)}>×</button>
          </div>
          {output.err ? (
            <pre className="qcp-output-err">{output.err}</pre>
          ) : (
            <pre className="qcp-output-text">{output.text || "(无输出)"}</pre>
          )}
        </div>
      )}
    </div>
  );
}
