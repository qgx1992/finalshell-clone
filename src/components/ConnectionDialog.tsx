import { useState } from "react";
import { open as dialogOpen, confirm as dialogConfirm } from "@tauri-apps/plugin-dialog";
import { useApp } from "../store/app";
import type { ServerConfig } from "../types";

interface Props {
  initial: ServerConfig | null;
  onClose: () => void;
}

const GROUPS = ["默认分组", "生产环境", "测试环境", "开发环境", "云服务器"];

const emptyForm = (): ServerConfig => ({
  id: "",
  name: "",
  host: "",
  port: 22,
  username: "root",
  auth_type: "password",
  password: "",
  private_key: null,
  key_passphrase: null,
  group: "默认分组",
  remark: "",
  use_proxy: false,
  proxy: null,
});

export default function ConnectionDialog({ initial, onClose }: Props) {
  const upsertServer = useApp((s) => s.upsertServer);
  const [form, setForm] = useState<ServerConfig>(initial ?? emptyForm());
  // 快照对比时用同一函数归一化(null/空串统一成空串),避免两侧格式不一致导致永远 dirty
  const norm = (s: ServerConfig) =>
    JSON.stringify({
      ...s,
      password: s.password || "",
      private_key: s.private_key || "",
      key_passphrase: s.key_passphrase || "",
    });
  // 初始快照:表单与快照不一致视为"有未保存修改"
  const [snapshot] = useState(() => norm(initial ?? emptyForm()));
  const dirty = norm(form) !== snapshot;
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const set = <K extends keyof ServerConfig>(k: K, v: ServerConfig[K]) =>
    setForm((f) => ({ ...f, [k]: v }));

  const pickKey = async () => {
    const p = await dialogOpen({
      title: "选择私钥文件",
      // Windows 文件对话框按扩展名过滤时会隐藏无扩展名的文件(如 id_rsa、id_ed25519),
      // 所以默认提供"所有文件",让 OpenSSH 私钥也能显示出来
      filters: [
        { name: "所有文件", extensions: ["*"] },
        { name: "私钥", extensions: ["pem", "key", "ppk"] },
      ],
    });
    if (p && typeof p === "string") set("private_key", p);
  };

  const save = async () => {
    if (!form.name.trim()) return setErr("请输入连接名称");
    if (!form.host.trim()) return setErr("请输入主机地址");
    if (!form.username.trim()) return setErr("请输入用户名");
    if (form.auth_type === "password" && !form.password) return setErr("请输入密码");
    if (form.auth_type === "key" && !form.private_key) return setErr("请选择私钥文件");
    setSaving(true);
    setErr(null);
    try {
      await upsertServer({
        ...form,
        id: form.id || crypto.randomUUID(),
        password: form.password || null,
        private_key: form.private_key || null,
        key_passphrase: form.key_passphrase || null,
      });
      onClose();
    } catch (e) {
      setErr(String(e));
      setSaving(false);
    }
  };

  // 统一的关闭入口:编辑模式下点遮罩不直接关闭;有未保存修改时弹系统确认框,确认后才关
  const isEditing = !!initial;

  const tryClose = async (fromBackdrop: boolean) => {
    if (fromBackdrop && isEditing) return; // 编辑服务器时,遮罩点击一律不关
    if (dirty) {
      const ok = await dialogConfirm("有未保存的修改，确定要关闭吗？", {
        title: "未保存的修改",
        kind: "warning",
      });
      if (!ok) return;
    }
    onClose();
  };

  return (
    <div
      className="modal-backdrop"
      onClick={() => void tryClose(true)}
    >
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <span>{initial ? "编辑服务器" : "新建连接"}</span>
          <button className="modal-close" onClick={() => void tryClose(false)}>×</button>
        </div>
        <div className="modal-body">
          <div className="form-grid">
            <label>
              名称 *
              <input
                value={form.name}
                placeholder="例如:生产服务器-01"
                onChange={(e) => set("name", e.target.value)}
                autoFocus
              />
            </label>
            <label>
              分组
              <input
                list="group-list"
                value={form.group}
                onChange={(e) => set("group", e.target.value)}
              />
              <datalist id="group-list">
                {GROUPS.map((g) => (
                  <option key={g} value={g} />
                ))}
              </datalist>
            </label>
            <label>
              主机地址 *
              <input
                value={form.host}
                placeholder="IP 或域名"
                onChange={(e) => set("host", e.target.value)}
              />
            </label>
            <label>
              端口
              <input
                type="number"
                value={form.port}
                onChange={(e) => set("port", parseInt(e.target.value) || 22)}
              />
            </label>
            <label>
              用户名 *
              <input value={form.username} onChange={(e) => set("username", e.target.value)} />
            </label>
            <label>
              认证方式
              <select
                value={form.auth_type}
                onChange={(e) => set("auth_type", e.target.value as "password" | "key")}
              >
                <option value="password">密码</option>
                <option value="key">私钥</option>
              </select>
            </label>
            {form.auth_type === "password" ? (
              <label className="form-full">
                密码 *
                <input
                  type="password"
                  value={form.password ?? ""}
                  placeholder="服务器登录密码(明文存储,请注意安全)"
                  onChange={(e) => set("password", e.target.value)}
                />
              </label>
            ) : (
              <>
                <label className="form-full">
                  私钥文件 *
                  <div className="key-picker">
                    <input
                      readOnly
                      value={form.private_key ?? ""}
                      placeholder="点击右侧选择私钥文件"
                      onClick={pickKey}
                    />
                    <button onClick={pickKey}>浏览…</button>
                  </div>
                </label>
                <label className="form-full">
                  私钥口令
                  <input
                    type="password"
                    value={form.key_passphrase ?? ""}
                    placeholder="如私钥有口令则填写"
                    onChange={(e) => set("key_passphrase", e.target.value)}
                  />
                </label>
              </>
            )}
            <label className="form-full">
              备注
              <textarea
                rows={2}
                value={form.remark}
                placeholder="备注信息"
                onChange={(e) => set("remark", e.target.value)}
              />
            </label>
            <label className="form-full form-check">
              <input
                type="checkbox"
                checked={form.use_proxy}
                onChange={(e) => set("use_proxy", e.target.checked)}
              />
              使用 SSH 代理(第一版暂未实现,保留配置)
            </label>
          </div>
          {err && <div className="form-error">{err}</div>}
          <div className="form-warn">
            ⚠️ 密码将以明文保存在本地配置文件中,请勿在公共电脑上保存敏感服务器密码。
          </div>
        </div>
        <div className="modal-footer">
          <button className="btn" onClick={onClose}>取消</button>
          <button className="btn btn-primary" onClick={save} disabled={saving}>
            {saving ? "保存中…" : "保存"}
          </button>
        </div>
      </div>
    </div>
  );
}
