import { useEffect, useRef, useState } from "react";
import { monitorPoll } from "../api";
import type { MonitorSnapshot } from "../types";
import Sparkline from "./Sparkline";

interface Props {
  sessionId: number;
  serverName: string;
  onClose: () => void;
}

const MAX_POINTS = 120;

function fmtBytes(n: number): string {
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

function fmtUptime(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}天${h}小时`;
  if (h > 0) return `${h}小时${m}分`;
  return `${m}分钟`;
}

function Gauge({ percent, color, label, detail }: { percent: number; color: string; label: string; detail: string }) {
  const r = 34;
  const circ = 2 * Math.PI * r;
  const pct = Math.min(Math.max(percent, 0), 100);
  return (
    <div className="gauge">
      <svg width="84" height="84" viewBox="0 0 84 84">
        <circle cx="42" cy="42" r={r} fill="none" stroke="rgba(128,128,128,0.18)" strokeWidth="8" />
        <circle
          cx="42"
          cy="42"
          r={r}
          fill="none"
          stroke={color}
          strokeWidth="8"
          strokeLinecap="round"
          strokeDasharray={circ}
          strokeDashoffset={circ * (1 - pct / 100)}
          transform="rotate(-90 42 42)"
          style={{ transition: "stroke-dashoffset 0.6s ease" }}
        />
        <text x="42" y="40" textAnchor="middle" className="gauge-val" dominantBaseline="central">
          {pct.toFixed(0)}%
        </text>
        <text x="42" y="60" textAnchor="middle" className="gauge-sub" dominantBaseline="central">
          {label}
        </text>
      </svg>
      <div className="gauge-detail">{detail}</div>
    </div>
  );
}

export default function MonitorPanel({ sessionId, serverName, onClose }: Props) {
  const [snap, setSnap] = useState<MonitorSnapshot | null>(null);
  const [cpuHist, setCpuHist] = useState<number[]>([]);
  const [memHist, setMemHist] = useState<number[]>([]);
  const [rxHist, setRxHist] = useState<number[]>([]);
  const [txHist, setTxHist] = useState<number[]>([]);
  const [error, setError] = useState<string | null>(null);
  const histRef = useRef({ cpu: [] as number[], mem: [] as number[], rx: [] as number[], tx: [] as number[] });
  const [netMax, setNetMax] = useState(1024 * 1024);

  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setInterval> | null = null;

    const tick = async () => {
      try {
        const s = await monitorPoll(sessionId);
        if (stopped) return;
        setSnap(s);
        const h = histRef.current;
        const push = (arr: number[], v: number) => {
          arr.push(v);
          if (arr.length > MAX_POINTS) arr.shift();
          return [...arr];
        };
        setCpuHist(push(h.cpu, s.cpu_percent));
        setMemHist(push(h.mem, s.mem_percent));
        const maxRate = Math.max(s.net_rx_speed, s.net_tx_speed, 64 * 1024);
        setNetMax((prev) => Math.max(prev * 0.9, maxRate * 1.3));
        setRxHist(push(h.rx, s.net_rx_speed));
        setTxHist(push(h.tx, s.net_tx_speed));
        setError(null);
      } catch (e) {
        if (!stopped) setError(String(e));
      }
    };

    tick();
    timer = setInterval(tick, 2000);
    return () => {
      stopped = true;
      if (timer) clearInterval(timer);
    };
  }, [sessionId]);

  return (
    <div className="monitor-panel">
      <div className="monitor-header">
        <span className="monitor-title">📊 {serverName} · 实时监控</span>
        <button onClick={onClose} title="关闭监控面板">×</button>
      </div>

      {error && <div className="monitor-error">{error}</div>}

      {snap ? (
        <>
          <div className="monitor-info">
            <div>主机: <b>{snap.hostname || "-"}</b></div>
            <div>CPU: <b>{snap.cpu_cores} 核</b></div>
            <div>负载: <b>{snap.load1.toFixed(2)} / {snap.load5.toFixed(2)} / {snap.load15.toFixed(2)}</b></div>
            <div>运行: <b>{fmtUptime(snap.uptime_secs)}</b></div>
          </div>

          <div className="monitor-gauges">
            <Gauge percent={snap.cpu_percent} color="#61afef" label="CPU" detail={`${snap.cpu_cores} 核`} />
            <Gauge
              percent={snap.mem_percent}
              color="#98c379"
              label="内存"
              detail={`${fmtBytes(snap.mem_used)} / ${fmtBytes(snap.mem_total)}`}
            />
            <Gauge
              percent={snap.disk_percent}
              color="#e5c07b"
              label="磁盘"
              detail={`${fmtBytes(snap.disk_used)} / ${fmtBytes(snap.disk_total)}`}
            />
          </div>

          {snap.swap_total > 0 && (
            <div className="monitor-swap">
              交换分区: {fmtBytes(snap.swap_used)} / {fmtBytes(snap.swap_total)}
            </div>
          )}

          <Sparkline data={cpuHist} color="#61afef" label="CPU 使用率" />
          <Sparkline data={memHist} color="#98c379" label="内存使用率" />
          <Sparkline
            data={rxHist}
            color="#56b6c2"
            label="网络下载"
            max={netMax}
            formatValue={(v) => `${fmtBytes(v)}/s`}
          />
          <Sparkline
            data={txHist}
            color="#c678dd"
            label="网络上传"
            max={netMax}
            formatValue={(v) => `${fmtBytes(v)}/s`}
          />
          <div className="monitor-net-total">
            累计 ↓ {fmtBytes(snap.net_rx)} ↑ {fmtBytes(snap.net_tx)}
          </div>
        </>
      ) : (
        <div className="monitor-loading">等待数据…</div>
      )}
    </div>
  );
}
