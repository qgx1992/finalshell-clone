use serde::Serialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;

use crate::ssh::manager::{run_command, SessionEntry, SshManager};

#[derive(Serialize, Clone, Default)]
pub struct MonitorSnapshot {
    pub cpu_percent: f64,
    pub cpu_cores: u32,
    pub mem_total: u64,   // bytes
    pub mem_used: u64,
    pub mem_percent: f64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub disk_total: u64,
    pub disk_used: u64,
    pub disk_percent: f64,
    pub net_rx: u64,
    pub net_tx: u64,
    pub net_rx_speed: f64, // bytes/s
    pub net_tx_speed: f64,
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub uptime_secs: u64,
    pub hostname: String,
}

const MONITOR_CMD: &str = "echo '---CPU---'; cat /proc/stat | head -1; \
    echo '---MEM---'; cat /proc/meminfo; \
    echo '---NET---'; cat /proc/net/dev; \
    echo '---DISK---'; df -Pk 2>/dev/null; \
    echo '---LOAD---'; cat /proc/loadavg; \
    echo '---UPTIME---'; cat /proc/uptime; \
    echo '---CORES---'; nproc 2>/dev/null || grep -c ^processor /proc/cpuinfo; \
    echo '---HOST---'; hostname 2>/dev/null || uname -n";

fn parse_kb(s: &str) -> u64 {
    s.trim().parse::<u64>().unwrap_or(0)
}

/// 采集一次监控快照(需要连续两次采样计算 CPU/网络速率)
#[tauri::command]
pub async fn monitor_poll(
    state: State<'_, SshManager>,
    id: u64,
) -> Result<MonitorSnapshot, String> {
    let entry = state
        .sessions
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("会话 {id} 不存在"))?;

    let output = run_command(&entry, MONITOR_CMD, Duration::from_secs(15)).await?;
    parse_monitor(&entry, &output).await
}

async fn parse_monitor(entry: &SessionEntry, output: &str) -> Result<MonitorSnapshot, String> {
    let mut snap = MonitorSnapshot::default();
    let mut cpu_line: Option<(u64, u64)> = None; // (idle, total)
    let mut mem_kb = (0u64, 0u64); // total, available
    let mut swap_kb = (0u64, 0u64);
    let mut net_rx: u64 = 0;
    let mut net_tx: u64 = 0;
    let mut disk_used_kb: u64 = 0;
    let mut disk_total_kb: u64 = 0;
    let mut section = "";
    let mut load_vals = [0f64; 3];
    let mut uptime: u64 = 0;
    let mut cores: u32 = 1;

    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("---") && line.ends_with("---") {
            section = &line[3..line.len() - 3];
            continue;
        }
        match section {
            "CPU" => {
                if let Some(rest) = line.strip_prefix("cpu ") {
                    let vals: Vec<u64> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                    if vals.len() >= 8 {
                        let idle = vals[3] + vals[4]; // idle + iowait
                        let total: u64 = vals.iter().sum();
                        cpu_line = Some((idle, total));
                    }
                }
            }
            "MEM" => {
                if let Some(v) = line.strip_prefix("MemTotal:") {
                    mem_kb.0 = parse_kb(v);
                } else if let Some(v) = line.strip_prefix("MemAvailable:") {
                    mem_kb.1 = parse_kb(v);
                } else if let Some(v) = line.strip_prefix("SwapTotal:") {
                    swap_kb.0 = parse_kb(v);
                } else if let Some(v) = line.strip_prefix("SwapFree:") {
                    swap_kb.1 = parse_kb(v);
                }
            }
            "NET" => {
                // 格式: eth0:  rx_bytes ... tx_bytes ...
                if let Some(idx) = line.find(':') {
                    let iface = &line[..idx];
                    if iface == "lo" {
                        continue;
                    }
                    let parts: Vec<&str> = line[idx + 1..].split_whitespace().collect();
                    if parts.len() >= 9 {
                        if let (Ok(rx), Ok(tx)) = (parts[0].parse::<u64>(), parts[8].parse::<u64>()) {
                            net_rx += rx;
                            net_tx += tx;
                        }
                    }
                }
            }
            "DISK" => {
                // df -Pk: Filesystem 1024-blocks Used Available Capacity Mounted on
                let mut it = line.split_whitespace();
                let fs = it.next().unwrap_or("");
                if fs == "Filesystem" || fs.is_empty() || fs == "none" {
                    continue;
                }
                let cols: Vec<&str> = line.split_whitespace().collect();
                if cols.len() < 6 {
                    continue;
                }
                let mount = cols[5];
                // 跳过虚拟文件系统
                if mount.starts_with("/proc")
                    || mount.starts_with("/sys")
                    || mount.starts_with("/dev")
                {
                    continue;
                }
                let total = cols[1].parse::<u64>().unwrap_or(0);
                let used = cols[2].parse::<u64>().unwrap_or(0);
                if total > 0 {
                    disk_used_kb += used.min(total);
                    disk_total_kb += total;
                }
            }
            "LOAD" => {
                let vals: Vec<f64> = line.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                if vals.len() >= 3 {
                    load_vals = [vals[0], vals[1], vals[2]];
                }
            }
            "UPTIME" => {
                if let Some(secs) = line.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()) {
                    uptime = secs as u64;
                }
            }
            "CORES" => {
                if let Ok(c) = line.trim().parse::<u32>() {
                    cores = c.max(1);
                }
            }
            "HOST" => {
                snap.hostname = line.trim().to_string();
            }
            _ => {}
        }
    }

    // CPU 使用率(两次采样差值)
    if let Some((idle, total)) = cpu_line {
        let mut last = entry.last_cpu.lock().await;
        if let Some((l_idle, l_total)) = *last {
            let d_idle = idle.saturating_sub(l_idle);
            let d_total = total.saturating_sub(l_total);
            if d_total > 0 {
                snap.cpu_percent = ((d_total - d_idle) as f64 / d_total as f64 * 100.0).clamp(0.0, 100.0);
            }
        }
        *last = Some((idle, total));
    }

    // 网络速率(差值 / 时间)
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    {
        let mut last = entry.last_net.lock().await;
        let mut last_t = entry.last_net_time.lock().await;
        if let (Some((l_rx, l_tx)), Some(lt)) = (*last, *last_t) {
            let dt_ms = now_ms.saturating_sub(lt).max(1);
            snap.net_rx_speed = net_rx.saturating_sub(l_rx) as f64 * 1000.0 / dt_ms as f64;
            snap.net_tx_speed = net_tx.saturating_sub(l_tx) as f64 * 1000.0 / dt_ms as f64;
        }
        *last = Some((net_rx, net_tx));
        *last_t = Some(now_ms);
    }

    // 内存
    let mem_total = mem_kb.0.saturating_mul(1024);
    let mem_avail = mem_kb.1.saturating_mul(1024);
    let mem_used = mem_total.saturating_sub(mem_avail);
    snap.mem_total = mem_total;
    snap.mem_used = mem_used;
    if mem_total > 0 {
        snap.mem_percent = mem_used as f64 / mem_total as f64 * 100.0;
    }
    let swap_total = swap_kb.0.saturating_mul(1024);
    snap.swap_total = swap_total;
    snap.swap_used = swap_total.saturating_sub(swap_kb.1.saturating_mul(1024));

    // 磁盘
    snap.disk_total = disk_total_kb.saturating_mul(1024);
    snap.disk_used = disk_used_kb.saturating_mul(1024);
    if disk_total_kb > 0 {
        snap.disk_percent = disk_used_kb as f64 / disk_total_kb as f64 * 100.0;
    }

    snap.cpu_cores = cores;
    snap.load1 = load_vals[0];
    snap.load5 = load_vals[1];
    snap.load15 = load_vals[2];
    snap.uptime_secs = uptime;
    snap.net_rx = net_rx;
    snap.net_tx = net_tx;

    Ok(snap)
}
