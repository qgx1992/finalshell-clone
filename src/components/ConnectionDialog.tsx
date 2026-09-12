import { useState } from "react";
import { open as dialogOpen, confirm as dialogConfirm } from "@tauri-apps/plugin-dialog";
import { useApp } from "../store/app";
import type { SecretsInput, ServerConfig } from "../types";

interface Props {
  initial: ServerConfig | null;
  onClose: () => void;
}

const GROUPS = ["默认分组", "生产环境", "测试环境", "开发环境", "云服务器"];

const RDP_RESOLUTIONS = [
  { value: "fullscreen", label: "全屏" },
  { value: "auto", label: "自适应窗口" },
  { value: "1280x720", label: "1280 x 720" },
  { value: "1366x768", label: "1366 x 768" },
  { value: "1600x900", label: "1600 x 900" },
  { value: "1920x1080", label: "1920 x 1080" },
  { value: "2560x1440", label: "2560 x 1440" },
];

const PROXY_KINDS = [
  { value: "socks5", label: "SOCKS5" },
  { value: "http", label: "HTTP CONNECT" },
];

/** 凭据操作:keep 保持原值,set 覆盖,clear 删除 */
type SecretAction = "keep" | "set" | "clear";

const emptyForm = (): ServerConfig => ({
  id: "",
  name: "",
  host: "",
  port: 22,
  conn_type: "ssh",
  rdp_resolution: "fullscreen",
  rdp_clipboard: true,
  username: "root",
  auth_type: "password",
  password: "",
  private_key: null,
  key_passphrase: null,
  has_password: false,
  has_key_passphrase: false,
  group: "默认分组",
  remark: "",
  use_proxy: false,
  proxy: {
    host: "",
    port: 1080,
    kind: "socks5",
    username: "",
    password: "",
    has_password: false,
  },
});

export default function ConnectionDialog({ initial, onClose }: Props) {
  const upsertServer = useApp((s) => s.upsertServer);
  const [form, setForm] = useState<ServerConfig>(initial ?? emptyForm());
  // 凭据操作:已有凭据默认保持;输入新值后变成 set;点击清除变成 clear
  const [passwordAction, setPasswordAction] = useState<SecretAction>("keep");
  const [keyPassAction, setKeyPassAction] = useState<SecretAction>("keep");
  const [proxyPassAction, setProxyPassAction] = useState<SecretAction>("keep");

  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const set = <K extends keyof ServerConfig>(k: K, v: ServerConfig[K]) =>
    setForm((f) => ({ ...f, [k]: v }));

  const setProxy = <K extends keyof NonNullable<ServerConfig["proxy"]>>(
    k: K,
    v: NonNullable<ServerConfig["proxy"]>[K]
  ) =>
    setForm((f) => ({
      ...f,
      proxy: { ...(f.proxy ?? { host: "", port: 1080, kind: "socks5" }), [k]: v },
    }));

  // 初始快照:表单与快照不一致视为"有未保存修改"
  const snapshot = useState(() => JSON.stringify(initial ?? emptyForm()))[0];
  const dirty =
    JSON.stringify(form) !== snapshot ||
    passwordAction !== "keep" ||
    keyPassAction !== "keep" ||
    proxyPassAction !== "keep";

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

  const buildSecrets = (): SecretsInput => {
    const secrets: SecretsInput = {};
    if (passwordAction === "clear") secrets.password = { action: "clear" };
    else if (passwordAction === "set" && form.password)
      secrets.password = { action: "set", value: form.password };
    else secrets.password = { action: "keep" };

    if (keyPassAction === "clear") secrets.key_passphrase = { action: "clear" };
    else if (keyPassAction === "set" && form.key_passphrase)
      secrets.key_passphrase = { action: "set", value: form.key_passphrase };
    else secrets.key_passphrase = { action: "keep" };

    if (proxyPassAction === "clear") secrets.proxy_password = { action: "clear" };
    else if (proxyPassAction === "set" && form.proxy?.password)
      secrets.proxy_password = { action: "set", value: form.proxy.password };
    else secrets.proxy_password = { action: "keep" };

    return secrets;
  };

  const save = async () => {
    if (!form.name.trim()) return setErr("请输入连接名称");
    if (!form.host.trim()) return setErr("请输入主机地址");
    if (!form.username.trim()) return setErr("请输入用户名");
    if (form.conn_type === "ssh") {
      if (form.auth_type === "password") {
        const alreadySaved = initial?.has_password;
        if (!alreadySaved && passwordAction !== "set")
          return setErr("请输入密码");
      }
      if (form.auth_type === "key" && !form.private_key)
        return setErr("请选择私钥文件");
    }
    if (form.use_proxy && !form.proxy?.host?.trim()) {
      return setErr("请输入代理服务器地址");
    }

    setSaving(true);
    setErr(null);
    try {
      // 提交的配置不包含明文凭据,凭据通过 secrets 单独下发
      const payload: ServerConfig = {
        ...form,
        id: form.id || crypto.randomUUID(),
        password: undefined,
        key_passphrase: undefined,
        private_key: form.private_key || null,
        proxy: form.proxy
          ? { ...form.proxy, password: undefined }
          : null,
      };
      await upsertServer(payload, buildSecrets());
      onClose();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
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

  const isRdp = form.conn_type === "rdp";

  // 选的固定分辨率超过本机屏幕时,mstsc 只能显示部分画面(出现滚动条),提前提醒
  const resTooLarge = (() => {
    if (!isRdp) return false;
    const m = /^(\d+)x(\d+)$/.exec(form.rdp_resolution || "");
    if (!m) return false;
    return Number(m[1]) > window.screen.availWidth || Number(m[2]) > window.screen.availHeight;
  })();

  return (
    <div className="modal-backdrop" onClick={() => void tryClose(true)}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <span>{initial ? "编辑服务器" : "新建连接"}</span>
          <button className="modal-close" onClick={() => void tryClose(false)}>×</button>
        </div>
        <div className="modal-body">
          <div className="form-grid">
            <label>
              连接类型
              <select
                value={form.conn_type}
                onChange={(e) =>
                  setForm((f) => ({
                    ...f,
                    conn_type: e.target.value as "ssh" | "rdp",
                    port: e.target.value === "rdp" ? 3389 : 22,
                  }))
                }
              >
                <option value="ssh">SSH (Linux/Unix)</option>
                <option value="rdp">远程 Windows (RDP)</option>
              </select>
            </label>
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
                placeholder={isRdp ? "3389" : "22"}
                onChange={(e) =>
                  set("port", parseInt(e.target.value) || (isRdp ? 3389 : 22))
                }
              />
            </label>
            <label>
              用户名 *
              <input
                value={form.username}
                placeholder={isRdp ? "Administrator" : "root"}
                onChange={(e) => set("username", e.target.value)}
              />
            </label>

            {isRdp ? (
              <>
                <label>
                  分辨率
                  <select
                    value={RDP_RESOLUTIONS.some((r) => r.value === form.rdp_resolution)
                      ? form.rdp_resolution
                      : "fullscreen"}
                    onChange={(e) => set("rdp_resolution", e.target.value)}
                  >
                    {RDP_RESOLUTIONS.map((r) => (
                      <option key={r.value} value={r.value}>
                        {r.label}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="form-full form-check">
                  <input
                    type="checkbox"
                    checked={form.rdp_clipboard ?? true}
                    onChange={(e) => set("rdp_clipboard", e.target.checked)}
                  />
                  共享剪贴板(本地与远程互相复制粘贴)
                </label>
                <label className="form-full">
                  备注
                  <textarea
                    rows={2}
                    value={form.remark}
                    placeholder="备注信息"
                    onChange={(e) => set("remark", e.target.value)}
                  />
                </label>
                <div className="form-warn form-full">
                  💡 远程 Windows 将调用系统自带远程桌面(mstsc)打开,密码在远程桌面登录框输入。
                  {resTooLarge && (
                    <>
                      <br />
                      ⚠️ 所选分辨率大于本机屏幕({window.screen.availWidth}×
                      {window.screen.availHeight}),窗口会出现滚动条;建议选「全屏」或更小的分辨率。
                    </>
                  )}
                </div>
              </>
            ) : (
              <>
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
                    密码 {!initial?.has_password && "*"}
                    <input
                      type="password"
                      value={form.password ?? ""}
                      placeholder={
                        passwordAction === "clear"
                          ? "已标记清除,保存后将删除已存密码"
                          : initial?.has_password
                            ? "已安全保存,留空保持不变"
                            : "输入服务器登录密码"
                      }
                      disabled={passwordAction === "clear"}
                      onChange={(e) => {
                        set("password", e.target.value);
                        setPasswordAction(e.target.value ? "set" : "keep");
                      }}
                    />
                    {initial?.has_password && (
                      <div className="secret-actions">
                        {passwordAction === "clear" ? (
                          <button
                            type="button"
                            className="btn-link"
                            onClick={() => {
                              setPasswordAction("keep");
                              set("password", "");
                            }}
                          >
                            取消清除
                          </button>
                        ) : (
                          <button
                            type="button"
                            className="btn-link btn-danger-link"
                            onClick={() => {
                              setPasswordAction("clear");
                              set("password", "");
                            }}
                          >
                            清除已保存密码
                          </button>
                        )}
                      </div>
                    )}
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
                        placeholder={
                          keyPassAction === "clear"
                            ? "已标记清除"
                            : initial?.has_key_passphrase
                              ? "已安全保存,留空保持不变"
                              : "如私钥有口令则填写"
                        }
                        disabled={keyPassAction === "clear"}
                        onChange={(e) => {
                          set("key_passphrase", e.target.value);
                          setKeyPassAction(e.target.value ? "set" : "keep");
                        }}
                      />
                      {initial?.has_key_passphrase && (
                        <div className="secret-actions">
                          {keyPassAction === "clear" ? (
                            <button
                              type="button"
                              className="btn-link"
                              onClick={() => {
                                setKeyPassAction("keep");
                                set("key_passphrase", "");
                              }}
                            >
                              取消清除
                            </button>
                          ) : (
                            <button
                              type="button"
                              className="btn-link btn-danger-link"
                              onClick={() => {
                                setKeyPassAction("clear");
                                set("key_passphrase", "");
                              }}
                            >
                              清除已保存口令
                            </button>
                          )}
                        </div>
                      )}
                    </label>
                  </>
                )}

                <label className="form-full form-check">
                  <input
                    type="checkbox"
                    checked={form.use_proxy}
                    onChange={(e) => set("use_proxy", e.target.checked)}
                  />
                  通过代理连接(SOCKS5 / HTTP CONNECT)
                </label>

                {form.use_proxy && (
                  <>
                    <label>
                      代理类型
                      <select
                        value={form.proxy?.kind || "socks5"}
                        onChange={(e) => setProxy("kind", e.target.value)}
                      >
                        {PROXY_KINDS.map((k) => (
                          <option key={k.value} value={k.value}>
                            {k.label}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label>
                      代理端口
                      <input
                        type="number"
                        value={form.proxy?.port ?? 1080}
                        onChange={(e) => setProxy("port", parseInt(e.target.value) || 1080)}
                      />
                    </label>
                    <label className="form-full">
                      代理地址 *
                      <input
                        value={form.proxy?.host ?? ""}
                        placeholder="127.0.0.1"
                        onChange={(e) => setProxy("host", e.target.value)}
                      />
                    </label>
                    <label>
                      代理用户名
                      <input
                        value={form.proxy?.username ?? ""}
                        placeholder="可选"
                        onChange={(e) => setProxy("username", e.target.value)}
                      />
                    </label>
                    <label>
                      代理密码
                      <input
                        type="password"
                        value={form.proxy?.password ?? ""}
                        placeholder={
                          proxyPassAction === "clear"
                            ? "已标记清除"
                            : initial?.proxy?.has_password
                              ? "已安全保存,留空保持不变"
                              : "可选"
                        }
                        disabled={proxyPassAction === "clear"}
                        onChange={(e) => {
                          setProxy("password", e.target.value);
                          setProxyPassAction(e.target.value ? "set" : "keep");
                        }}
                      />
                      {initial?.proxy?.has_password && (
                        <div className="secret-actions">
                          {proxyPassAction === "clear" ? (
                            <button
                              type="button"
                              className="btn-link"
                              onClick={() => {
                                setProxyPassAction("keep");
                                setProxy("password", "");
                              }}
                            >
                              取消清除
                            </button>
                          ) : (
                            <button
                              type="button"
                              className="btn-link btn-danger-link"
                              onClick={() => {
                                setProxyPassAction("clear");
                                setProxy("password", "");
                              }}
                            >
                              清除已保存密码
                            </button>
                          )}
                        </div>
                      )}
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
              </>
            )}
          </div>
          {err && <div className="form-error">{err}</div>}
          {!isRdp && (
            <div className="form-warn">
              🔐 密码与私钥口令保存在操作系统凭据库中,不再写入配置文件,也不会下发到界面。
              首次连接服务器时会要求确认主机指纹。
            </div>
          )}
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
