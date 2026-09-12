import type { HostKeyObservation } from "../types";

interface Props {
  observation: HostKeyObservation;
  /** 指纹变更时风险更高,确认按钮文案不同 */
  changed: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * 主机指纹确认弹窗。
 *
 * - 首次连接:展示指纹,由用户确认后写入 known_hosts
 * - 指纹变更:高亮警告,展示新旧指纹对比(不提供"永久忽略"选项)
 */
export default function HostKeyDialog({
  observation,
  changed,
  onConfirm,
  onCancel,
}: Props) {
  const title = changed ? "⚠️ 服务器指纹已变更" : "🔐 首次连接,请确认服务器指纹";

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal hostkey-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <span>{title}</span>
          <button className="modal-close" onClick={onCancel}>×</button>
        </div>
        <div className="modal-body">
          {changed ? (
            <div className="form-error">
              服务器 <b>{observation.host}:{observation.port}</b> 的主机密钥与之前记录不一致。
              这可能是服务器重装/更换密钥,也可能是中间人攻击。
              请通过可信渠道核实新指纹后再继续。
            </div>
          ) : (
            <p className="hostkey-intro">
              这是本机第一次连接 <b>{observation.host}:{observation.port}</b>。
              请确认下面的指纹与服务器管理员提供的一致。
            </p>
          )}

          <div className="hostkey-block">
            <div className="hostkey-label">密钥算法</div>
            <div className="hostkey-value">{observation.algorithm}</div>
          </div>

          {changed && observation.previous_fingerprint && (
            <div className="hostkey-block hostkey-old">
              <div className="hostkey-label">原记录指纹</div>
              <div className="hostkey-value">{observation.previous_fingerprint}</div>
            </div>
          )}

          <div className={"hostkey-block" + (changed ? " hostkey-new" : "")}>
            <div className="hostkey-label">
              {changed ? "当前服务器指纹" : "服务器指纹(SHA256)"}
            </div>
            <div className="hostkey-value hostkey-fingerprint">
              {observation.fingerprint}
            </div>
          </div>

          {changed && (
            <div className="form-warn">
              仅在你确认服务器确实更换了密钥时才继续。确认后旧指纹记录会被替换。
            </div>
          )}
        </div>
        <div className="modal-footer">
          <button className="btn" onClick={onCancel}>取消</button>
          <button
            className={changed ? "btn btn-danger" : "btn btn-primary"}
            onClick={onConfirm}
          >
            {changed ? "接受新指纹并连接" : "信任并连接"}
          </button>
        </div>
      </div>
    </div>
  );
}
