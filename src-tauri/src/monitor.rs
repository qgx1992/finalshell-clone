use serde::Serialize;
use ts_rs::TS;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;

use crate::ssh::manager::{run_command, RemotePlatform, SessionEntry, SshManager};

#[derive(Serialize, Clone, Default, TS)]
#[ts(export, export_to = "../../src/generated/")]
pub struct MonitorSnapshot {
    pub cpu_percent: f64,
    pub cpu_cores: u32,
    #[ts(type = "number")]
    pub mem_total: u64,   // bytes
    #[ts(type = "number")]
    pub mem_used: u64,
    pub mem_percent: f64,
    #[ts(type = "number")]
    pub swap_total: u64,
    #[ts(type = "number")]
    pub swap_used: u64,
    #[ts(type = "number")]
    pub disk_total: u64,
    #[ts(type = "number")]
    pub disk_used: u64,
    pub disk_percent: f64,
    #[ts(type = "number")]
    pub net_rx: u64,
    #[ts(type = "number")]
    pub net_tx: u64,
    pub net_rx_speed: f64, // bytes/s
    pub net_tx_speed: f64,
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    #[ts(type = "number")]
    pub uptime_secs: u64,
    pub hostname: String,
}

/// 判断远程系统类型。只用一条尽量通用的命令,失败时判定为不支持。
const PLATFORM_DETECT_CMD: &str = "uname -s 2>/dev/null || ver";

/// Linux:仅首次执行,采集几乎不变的静态信息(缓存复用)
const LINUX_INIT_CMD: &str = "echo '---HOST---'; hostname 2>/dev/null || uname -n; \
    echo '---CORES---'; nproc 2>/dev/null || grep -c ^processor /proc/cpuinfo";

/// Windows 静态信息
const WINDOWS_INIT_CMD: &str = "powershell -NoProfile -Command \"\
    Write-Output '---HOST---'; Write-Output $env:COMPUTERNAME; \
    Write-Output '---CORES---'; Write-Output $env:NUMBER_OF_PROCESSORS\"";

/// macOS 静态信息
const MACOS_INIT_CMD: &str = "echo '---HOST---'; hostname; \
    echo '---CORES---'; sysctl -n hw.ncpu";

/// Linux 每轮采样:CPU/内存/网络/磁盘/负载/运行时间
const LINUX_POLL_CMD: &str = "echo '---CPU---'; cat /proc/stat | head -1; \
    echo '---MEM---'; cat /proc/meminfo; \
    echo '---NET---'; cat /proc/net/dev; \
    echo '---DISK---'; df -Pk 2>/dev/null; \
    echo '---LOAD---'; cat /proc/loadavg; \
    echo '---UPTIME---'; cat /proc/uptime";

/// Windows 每轮采样:输出与 Linux 相同的分段结构,CPU 直接给瞬时占用率
const WINDOWS_POLL_CMD: &str = "powershell -NoProfile -Command \"\
    Write-Output '---CPUWIN---'; \
    Write-Output ((Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor | Where-Object {$_.Name -eq '_Total'}).PercentProcessorTime); \
    Write-Output '---MEM---'; \
    Write-Output ('MemTotal: ' + [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory/1024)); \
    Write-Output ('MemAvailable: ' + [math]::Round((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory)); \
    Write-Output 'SwapTotal: 0'; Write-Output 'SwapFree: 0'; \
    Write-Output '---NET---'; \
    Get-NetAdapterStatistics | ForEach-Object { Write-Output ($_.Name + ': ' + $_.ReceivedBytes + ' 0 0 0 0 0 0 0 ' + $_.SentBytes) }; \
    Write-Output '---DISK---'; Write-Output 'Filesystem 1024-blocks Used Available Capacity Mounted'; \
    Get-CimInstance Win32_LogicalDisk -Filter \"DriveType=3\" | ForEach-Object { $t=$_.Size/1024; $f=$_.FreeSpace/1024; $u=$t-$f; $d=$_.DeviceID.Replace(':', ''); Write-Output ($d + ' ' + [math]::Round($t) + ' ' + [math]::Round($u) + ' ' + [math]::Round($f) + ' 0% /' + $d) }; \
    Write-Output '---LOAD---'; Write-Output '0 0 0'; \
    Write-Output '---UPTIME---'; Write-Output ([math]::Round(((Get-Date) - (Get-CimInstance Win32_OperatingSystem).LastBootUpTime).TotalSeconds))\"";

/// macOS 每轮采样:CPU 用 ps 汇总瞬时占用,其余尽量对齐 Linux 结构
const MACOS_POLL_CMD: &str = "echo '---CPUWIN---'; ps -A -o %cpu | awk 'NR>1{s+=$1} END{print s}'; \
    echo '---MEM---'; vm_stat | awk -v psz=$(sysctl -n hw.pagesize) '/Pages free/{f=$3} /Pages active/{a=$3} /Pages inactive/{i=$3} /Pages wired/{w=$4} END{printf \"MemTotal: %d\\nMemAvailable: %d\\nSwapTotal: 0\\nSwapFree: 0\\n\", (f+a+i+w)*psz/1024, f*psz/1024}'; \
    echo '---NET---'; netstat -ib 2>/dev/null | awk 'NR>1 && $1!=\"lo0\"{rx[$1]=$7; tx[$1]=$10} END{for(i in rx) print i \": \" rx[i] \" 0 0 0 0 0 0 0 \" tx[i]}'; \
    echo '---DISK---'; df -Pk 2>/dev/null; \
    echo '---LOAD---'; sysctl -n vm.loadavg | tr -d '{}'; \
    echo '---UPTIME---'; echo $(( $(date +%s) - $(sysctl -n kern.boottime | sed 's/.*sec = \\([0-9]*\\).*/\\1/') ))";

fn parse_kb(s: &str) -> u64 {
    s.trim().parse::<u64>().unwrap_or(0)
}

/// 根据 uname/ver 输出判定远程平台
fn detect_platform(output: &str) -> RemotePlatform {
    let text = output.to_ascii_lowercase();
    if text.contains("linux") {
        RemotePlatform::Linux
    } else if text.contains("darwin") {
        RemotePlatform::MacOs
    } else if text.contains("windows") || text.contains("microsoft") {
        RemotePlatform::Windows
    } else {
        RemotePlatform::Unsupported
    }
}

fn init_cmd(platform: RemotePlatform) -> Option<&'static str> {
    match platform {
        RemotePlatform::Linux => Some(LINUX_INIT_CMD),
        RemotePlatform::Windows => Some(WINDOWS_INIT_CMD),
        RemotePlatform::MacOs => Some(MACOS_INIT_CMD),
        RemotePlatform::Unsupported => None,
    }
}

fn poll_cmd(platform: RemotePlatform) -> Option<&'static str> {
    match platform {
        RemotePlatform::Linux => Some(LINUX_POLL_CMD),
        RemotePlatform::Windows => Some(WINDOWS_POLL_CMD),
        RemotePlatform::MacOs => Some(MACOS_POLL_CMD),
        RemotePlatform::Unsupported => None,
    }
}

/// 采集一次监控快照
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

    // 1. 平台探测(仅首次;结果缓存)
    let platform = {
        let cache = entry.monitor_cache.lock().await;
        cache.platform
    };
    let platform = match platform {
        Some(p) => p,
        None => {
            let detected = run_command(&entry, PLATFORM_DETECT_CMD, Duration::from_secs(10))
                .await
                .map(|out| detect_platform(&out))
                .unwrap_or(RemotePlatform::Unsupported);
            entry.monitor_cache.lock().await.platform = Some(detected);
            detected
        }
    };

    let Some(poll_cmd) = poll_cmd(platform) else {
        return Err("当前远程系统不支持内置监控(仅支持 Linux / Windows / macOS)".into());
    };

    // 2. 静态信息(主机名/CPU 核数)仅首次采集并缓存
    {
        let cache = entry.monitor_cache.lock().await;
        if cache.hostname.is_none() {
            drop(cache);
            if let Some(cmd) = init_cmd(platform) {
                if let Ok(init_out) = run_command(&entry, cmd, Duration::from_secs(15)).await {
                    let mut cache = entry.monitor_cache.lock().await;
                    let mut section = "";
                    for line in init_out.lines() {
                        let line = line.trim();
                        if line.starts_with("---") && line.ends_with("---") {
                            section = &line[3..line.len() - 3];
                            continue;
                        }
                        match section {
                            "HOST" => {
                                if !line.is_empty() {
                                    cache.hostname = Some(line.to_string());
                                }
                            }
                            "CORES" => {
                                if let Ok(c) = line.parse::<u32>() {
                                    cache.cores = Some(c.max(1));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // 3. 动态指标
    let output = run_command(&entry, poll_cmd, Duration::from_secs(15)).await?;
    let mut snap = parse_monitor(&entry, &output).await?;
    let cache = entry.monitor_cache.lock().await;
    snap.hostname = cache.hostname.clone().unwrap_or_default();
    snap.cpu_cores = cache.cores.unwrap_or(1);
    Ok(snap)
}

async fn parse_monitor(entry: &SessionEntry, output: &str) -> Result<MonitorSnapshot, String> {
    let mut snap = MonitorSnapshot::default();
    let mut cpu_line: Option<(u64, u64)> = None; // (idle, total)
    let mut cpu_direct: Option<f64> = None; // Windows/macOS 直接给出的占用率
    let mut mem_kb = (0u64, 0u64); // total, available
    let mut swap_kb = (0u64, 0u64);
    let mut net_rx: u64 = 0;
    let mut net_tx: u64 = 0;
    let mut disk_used_kb: u64 = 0;
    let mut disk_total_kb: u64 = 0;
    let mut section = "";
    let mut load_vals = [0f64; 3];
    let mut uptime: u64 = 0;

    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("---") && line.ends_with("---") {
            section = &line[3..line.len() - 3];
            continue;
        }
        match section {
            "CPU" => {
                if let Some(rest) = line.strip_prefix("cpu ") {
                    let vals: Vec<u64> =
                        rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                    if vals.len() >= 8 {
                        let idle = vals[3] + vals[4]; // idle + iowait
                        let total: u64 = vals.iter().sum();
                        cpu_line = Some((idle, total));
                    }
                }
            }
            "CPUWIN" => {
                if let Ok(v) = line.parse::<f64>() {
                    cpu_direct = Some(v.clamp(0.0, 100.0));
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
                if let Some(idx) = line.find(':') {
                    let iface = &line[..idx];
                    if iface == "lo" || iface == "lo0" {
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
                let vals: Vec<f64> =
                    line.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                if vals.len() >= 3 {
                    load_vals = [vals[0], vals[1], vals[2]];
                }
            }
            "UPTIME" => {
                if let Some(secs) = line
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse::<f64>().ok())
                {
                    uptime = secs as u64;
                }
            }
            _ => {}
        }
    }

    // CPU:优先使用直接占用率(Windows/macOS),否则用两次采样差值(Linux)
    if let Some(v) = cpu_direct {
        snap.cpu_percent = v;
    } else if let Some((idle, total)) = cpu_line {
        let mut last = entry.last_cpu.lock().await;
        if let Some((l_idle, l_total)) = *last {
            let d_idle = idle.saturating_sub(l_idle);
            let d_total = total.saturating_sub(l_total);
            if d_total > 0 {
                snap.cpu_percent =
                    ((d_total - d_idle) as f64 / d_total as f64 * 100.0).clamp(0.0, 100.0);
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

    snap.load1 = load_vals[0];
    snap.load5 = load_vals[1];
    snap.load15 = load_vals[2];
    snap.uptime_secs = uptime;
    snap.net_rx = net_rx;
    snap.net_tx = net_tx;

    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_linux_windows_macos() {
        assert_eq!(detect_platform("Linux"), RemotePlatform::Linux);
        assert_eq!(detect_platform("Darwin"), RemotePlatform::MacOs);
        assert_eq!(
            detect_platform("Microsoft Windows [Version 10.0.22631.3155]"),
            RemotePlatform::Windows
        );
        assert_eq!(detect_platform("SomeBSD"), RemotePlatform::Unsupported);
    }

    #[test]
    fn unsupported_platform_has_no_commands() {
        assert!(poll_cmd(RemotePlatform::Unsupported).is_none());
        assert!(init_cmd(RemotePlatform::Unsupported).is_none());
    }
}
