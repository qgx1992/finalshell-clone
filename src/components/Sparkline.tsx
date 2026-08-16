import { useEffect, useRef } from "react";

interface Props {
  data: number[];
  max?: number;
  color: string;
  height?: number;
  label?: string;
  unit?: string;
  /** 自定义当前值格式化(如字节 -> KB/s),默认 toFixed(1)+unit */
  formatValue?: (v: number) => string;
}

export default function Sparkline({ data, max = 100, color, height = 48, label, unit = "%", formatValue }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth;
    const h = height;
    canvas.width = w * dpr;
    canvas.height = h * dpr;
    const ctx = canvas.getContext("2d")!;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, w, h);

    // 网格
    ctx.strokeStyle = "rgba(128,128,128,0.15)";
    ctx.lineWidth = 1;
    for (let i = 0; i <= 4; i++) {
      const y = (h / 4) * i;
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(w, y);
      ctx.stroke();
    }

    if (data.length < 2) return;
    const n = data.length;
    const step = w / Math.max(n - 1, 1);
    const norm = (v: number) => h - 2 - Math.min(Math.max(v, 0), max) * ((h - 6) / max);

    // 渐变填充
    const grad = ctx.createLinearGradient(0, 0, 0, h);
    grad.addColorStop(0, color + "66");
    grad.addColorStop(1, color + "00");
    ctx.beginPath();
    ctx.moveTo(0, h - 2);
    for (let i = 0; i < n; i++) {
      ctx.lineTo(i * step, norm(data[i]));
    }
    ctx.lineTo((n - 1) * step, h - 2);
    ctx.closePath();
    ctx.fillStyle = grad;
    ctx.fill();

    // 折线
    ctx.beginPath();
    for (let i = 0; i < n; i++) {
      const x = i * step;
      const y = norm(data[i]);
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.8;
    ctx.stroke();

    // 最新值
    const last = data[n - 1];
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc((n - 1) * step, norm(last), 2.5, 0, Math.PI * 2);
    ctx.fill();
  }, [data, max, color, height]);

  return (
    <div className="sparkline">
      {label && (
        <div className="sparkline-label">
          <span>{label}</span>
          <span style={{ color }}>
            {data.length
              ? formatValue
                ? formatValue(data[data.length - 1])
                : `${data[data.length - 1].toFixed(1)}${unit}`
              : "-"}
          </span>
        </div>
      )}
      <canvas ref={canvasRef} style={{ width: "100%", height }} />
    </div>
  );
}
