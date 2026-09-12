//! 服务器主机密钥校验(TOFU:首次信任,后续严格校验)。
//!
//! 复用 OpenSSH 的 known_hosts 文本格式,文件放在应用配置目录下,
//! 与系统 ~/.ssh/known_hosts 隔离,避免污染用户的命令行 SSH 配置。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh::keys::known_hosts::{check_known_hosts_path, learn_known_hosts_path};
use russh::keys::{HashAlg, PublicKey};
use serde::Serialize;
use ts_rs::TS;
use tokio::sync::Mutex;

/// 主机密钥校验状态,返回给前端用于展示确认弹窗
#[derive(Clone, Debug, Serialize, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../src/generated/")]
pub enum HostKeyStatus {
    /// 首次连接,等待用户确认
    Unknown,
    /// 与本机记录不一致,可能是中间人攻击
    Changed,
    /// 已信任
    Trusted,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(export, export_to = "../../src/generated/")]
pub struct HostKeyObservation {
    pub status: HostKeyStatus,
    pub host: String,
    pub port: u16,
    /// SHA256 指纹,形如 SHA256:xxxx
    pub fingerprint: String,
    /// 密钥算法,如 ssh-ed25519
    pub algorithm: String,
    /// 已有的旧指纹(仅 status = changed 时存在)
    pub previous_fingerprint: Option<String>,
}

/// 用户在本轮连接中给出的信任意图
#[derive(Clone, Debug, PartialEq)]
pub enum TrustDecision {
    /// 首次信任:指纹必须与观察到的完全一致才写入
    TrustNew { fingerprint: String },
    /// 明确接受一次指纹变更(高风险操作)
    TrustChanged { fingerprint: String },
}

/// 主机密钥校验器:每个 SSH 连接持有一个实例
pub struct HostKeyVerifier {
    path: PathBuf,
    host: String,
    port: u16,
    decision: Option<TrustDecision>,
    observation: Arc<Mutex<Option<HostKeyObservation>>>,
}

impl HostKeyVerifier {
    pub fn new(path: PathBuf, host: String, port: u16, decision: Option<TrustDecision>) -> Self {
        Self {
            path,
            host,
            port,
            decision,
            observation: Arc::new(Mutex::new(None)),
        }
    }

    pub fn observation(&self) -> Arc<Mutex<Option<HostKeyObservation>>> {
        self.observation.clone()
    }

    /// 计算展示用指纹(SHA256)
    fn fingerprint_of(key: &PublicKey) -> String {
        key.fingerprint(HashAlg::Sha256).to_string()
    }

    fn record_observation(
        &self,
        status: HostKeyStatus,
        key: &PublicKey,
        previous_fingerprint: Option<String>,
    ) {
        let observation = HostKeyObservation {
            status,
            host: self.host.clone(),
            port: self.port,
            fingerprint: Self::fingerprint_of(key),
            algorithm: key.algorithm().to_string(),
            previous_fingerprint,
        };
        if let Ok(mut slot) = self.observation.try_lock() {
            *slot = Some(observation);
        }
    }

    /// 读取本机已记录的第一条主机密钥指纹,用于变更告警展示旧指纹
    fn recorded_fingerprint(&self) -> Option<String> {
        recorded_keys(&self.path, &self.host, self.port)
            .first()
            .map(Self::fingerprint_of)
    }

    pub async fn verify(&mut self, key: &PublicKey) -> Result<bool, russh::Error> {
        match check_known_hosts_path(&self.host, self.port, key, &self.path) {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(russh::keys::Error::KeyChanged { .. }) => {
                let previous = self.recorded_fingerprint();
                self.record_observation(HostKeyStatus::Changed, key, previous);
                return Err(russh::Error::UnknownKey);
            }
            Err(_) => {}
        }

        let current = Self::fingerprint_of(key);
        match self.decision.clone() {
            Some(TrustDecision::TrustNew { fingerprint })
                if fingerprint == current && self.recorded_fingerprint().is_none() =>
            {
                self.learn(key)?;
                Ok(true)
            }
            Some(TrustDecision::TrustChanged { fingerprint }) if fingerprint == current => {
                self.remove_host_entries()?;
                self.learn(key)?;
                Ok(true)
            }
            _ => {
                let previous = self.recorded_fingerprint();
                let status = if previous.is_some() {
                    HostKeyStatus::Changed
                } else {
                    HostKeyStatus::Unknown
                };
                self.record_observation(status, key, previous);
                Err(russh::Error::UnknownKey)
            }
        }
    }

    fn learn(&self, key: &PublicKey) -> Result<(), russh::Error> {
        learn_known_hosts_path(&self.host, self.port, key, &self.path)
            .map_err(|_| russh::Error::UnknownKey)
    }

    /// 指纹变更时,先移除本机对旧密钥的记录,再写入新密钥
    fn remove_host_entries(&self) -> Result<(), russh::Error> {
        let Ok(content) = std::fs::read_to_string(&self.path) else {
            return Ok(());
        };
        let target_plain = self.host.clone();
        let target_port = format!("[{}]:{}", self.host, self.port);

        let kept: Vec<&str> = content
            .lines()
            .filter(|line| {
                let Some(hosts) = line.split_whitespace().next() else {
                    return true;
                };
                if hosts.starts_with('|') {
                    return true;
                }
                !hosts
                    .split(',')
                    .any(|h| h == target_plain || h == target_port)
            })
            .collect();

        let mut text = kept.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        std::fs::write(&self.path, text).map_err(|_| russh::Error::UnknownKey)
    }
}

/// 读取 known_hosts 中与指定主机匹配的公钥。
///
/// russh 0.62 只导出 check/learn,未导出查询函数,因此这里直接解析标准 known_hosts 文本。
/// 应用自身写入的条目均为明文主机名;哈希主机名条目交由 russh 在校验阶段处理。
pub fn recorded_keys(path: &Path, host: &str, port: u16) -> Vec<PublicKey> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let host_port = if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    };
    let mut out = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(hosts), Some(_algo), Some(key_b64)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if hosts.starts_with('|') {
            continue;
        }
        if hosts.split(',').any(|h| h == host_port) {
            if let Ok(key) = russh::keys::parse_public_key_base64(key_b64) {
                out.push(key);
            }
        }
    }
    out
}

/// known_hosts 文件路径
pub fn known_hosts_path(config_dir: &Path) -> PathBuf {
    config_dir.join("known_hosts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_hosts_path_lives_in_config_dir() {
        let p = known_hosts_path(Path::new("C:/cfg"));
        assert!(p.ends_with("known_hosts"));
    }
}
