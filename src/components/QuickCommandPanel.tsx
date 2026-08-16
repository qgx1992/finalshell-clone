import { useState } from "react";
import { writeTerminal } from "../api";
import { useApp } from "../store/app";

interface Props {
  sessionId: number | null;
}

export default function QuickCommandPanel({ sessionId }: Props) {
  const settings = useApp((s) => s.settings);
  const updateSettings = useApp((s) => s.updateSettings);
  const [hint, setHint] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [newName, setNewName] = useState("");
  const [newCmd, setNewCmd] = useState("");
  const [cmdSeq, setCmdSeq] = useState(0);
  const [lastCmd, setLastCmd] = useState<string | null>(null);

  // 点击后在终端窗口执行该命令(写入会话,由 shell 真正运行并把输出显示在终端里)
  const run = (cmd: string) => {
    if (sessionId == null) {
      setHint("请先连接服务器");
      return;
    }
    const b64 = btoa(unescape(encodeURIComponent(cmd + "\n")));
    writeTerminal(sessionId, b64).catch(() => setHint("命令写入失败"));
    setHint(null);
    setLastCmd(cmd);
    setCmdSeq((n) => n + 1);
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

      {lastCmd && (
        <div className="qcp-hint" key={cmdSeq}>
          已发送到终端：{lastCmd}
        </div>
      )}
      {hint && <div className="qcp-hint qcp-hint-err">{hint}</div>}
    </div>
  );
}
