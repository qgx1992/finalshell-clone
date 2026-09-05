import { useId } from "react";

interface Props {
  /** 边长(px),默认 20 */
  size?: number;
  className?: string;
}

/**
 * 应用标志(与 src-tauri/icons 的应用图标同一套设计):
 * 蓝→青渐变圆角方卡 + 白色终端提示符。
 * 坐标按 48×48 画布给出,与 scripts/gen-icon.py 的比例保持一致。
 */
export default function BrandMark({ size = 20, className }: Props) {
  // 同一页面可能渲染多个实例,渐变 id 必须唯一
  const gid = useId();
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 48 48"
      role="img"
      aria-label="FinalShell-X"
    >
      <defs>
        <linearGradient id={gid} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#3c84ff" />
          <stop offset="100%" stopColor="#24cddc" />
        </linearGradient>
      </defs>
      <rect width="48" height="48" rx="10.8" fill={`url(#${gid})`} />
      <path
        d="M14.8 10.5 L27.2 21.1 L14.8 31.7"
        fill="none"
        stroke="#fff"
        strokeWidth="4.3"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <rect x="21.6" y="30.5" width="14.4" height="4.3" rx="2.15" fill="#fff" fillOpacity="0.92" />
    </svg>
  );
}
