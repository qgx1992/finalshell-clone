import { useState } from "react";
import { open as dialogOpen } from "@tauri-apps/plugin-dialog";
import { useApp } from "../store/app";
import type { ServerConfig } from "../types";

interface Props {
  initial: ServerConfig | null;
  onClose: () => void;
}

const GROUPS = ["默认分组", "生产环境", "测试环境", "开发环境", "云服务器"];

export default function ConnectionDialog({ initial, onClose }: Props) {
  const upsertServer = useApp((s) => s.upsertServer);
  const [form, setForm] = useState<ServerConfig>(
    initial ?? {
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
    }
  );
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

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <span>{initial ? "编辑服务器" : "新建连接"}</span>
          <button className="modal-close" onClick={onClose}>×</button>
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
