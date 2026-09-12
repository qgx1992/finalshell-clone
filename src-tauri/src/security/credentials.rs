//! 凭据管理:密码 / 私钥口令 / 代理密码存入操作系统凭据库。
//!
//! 设计要点:
//! - 前端永远拿不到明文凭据。`list_servers` 返回的配置只带 `has_password` 之类的布尔位。
//! - 连接时前端只传服务器 ID,后端自己从凭据库读取。
//! - 旧版本明文配置会在启动时迁移:写入凭据库后清空 JSON 字段,并保留一次性备份。
//! - 凭据库不可用(少见)时降级为 DPAPI/配置文件加密不可用,此时明确报错而不是静默明文回退。

use std::collections::HashMap;
use std::sync::Mutex;

use keyring::Entry;
use serde::{Deserialize, Serialize};

const SERVICE: &str = "com.finalshell.clone";

/// 服务器上需要托管的凭据种类
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    /// SSH 登录密码
    Password,
    /// 私钥口令
    KeyPassphrase,
    /// 代理密码
    ProxyPassword,
}

impl SecretKind {
    fn suffix(self) -> &'static str {
        match self {
            SecretKind::Password => "password",
            SecretKind::KeyPassphrase => "key-passphrase",
            SecretKind::ProxyPassword => "proxy-password",
        }
    }
}

/// 凭据库不可用时的统一错误
#[derive(Debug)]
pub struct CredentialError(pub String);

impl std::fmt::Display for CredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CredentialError {}

fn entry(server_id: &str, kind: SecretKind) -> Result<Entry, CredentialError> {
    let user = format!("{server_id}:{}", kind.suffix());
    Entry::new(SERVICE, &user).map_err(|e| CredentialError(format!("凭据库初始化失败: {e}")))
}

/// 写入或覆盖一个凭据
pub fn set_secret(server_id: &str, kind: SecretKind, value: &str) -> Result<(), CredentialError> {
    entry(server_id, kind)?
        .set_password(value)
        .map_err(|e| CredentialError(format!("写入凭据失败: {e}")))
}

/// 读取凭据;不存在返回 None
pub fn get_secret(server_id: &str, kind: SecretKind) -> Result<Option<String>, CredentialError> {
    match entry(server_id, kind)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(CredentialError(format!("读取凭据失败: {e}"))),
    }
}

/// 删除凭据(不存在视为成功)
pub fn delete_secret(server_id: &str, kind: SecretKind) -> Result<(), CredentialError> {
    match entry(server_id, kind)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(CredentialError(format!("删除凭据失败: {e}"))),
    }
}

/// 删除一台服务器的全部凭据(删除服务器配置时调用)
pub fn delete_all(server_id: &str) {
    for kind in [
        SecretKind::Password,
        SecretKind::KeyPassphrase,
        SecretKind::ProxyPassword,
    ] {
        let _ = delete_secret(server_id, kind);
    }
}

/// 内存缓存:避免每次连接都访问系统凭据库(可能弹授权框)
#[derive(Default)]
pub struct SecretCache {
    inner: Mutex<HashMap<(String, SecretKind), Option<String>>>,
}

impl SecretCache {
    pub fn get(
        &self,
        server_id: &str,
        kind: SecretKind,
    ) -> Result<Option<String>, CredentialError> {
        let key = (server_id.to_string(), kind);
        if let Some(cached) = self.inner.lock().unwrap().get(&key) {
            return Ok(cached.clone());
        }
        let value = get_secret(server_id, kind)?;
        self.inner.lock().unwrap().insert(key, value.clone());
        Ok(value)
    }

    pub fn invalidate(&self, server_id: &str) {
        let mut guard = self.inner.lock().unwrap();
        guard.retain(|(id, _), _| id != server_id);
    }
}

/// 安全地清空字符串内容(尽力而为,避免明文长期驻留)
pub fn zeroize_string(value: &mut String) {
    unsafe {
        for byte in value.as_bytes_mut() {
            std::ptr::write_volatile(byte, 0);
        }
    }
    value.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_kinds_have_distinct_suffixes() {
        assert_ne!(
            SecretKind::Password.suffix(),
            SecretKind::KeyPassphrase.suffix()
        );
        assert_ne!(
            SecretKind::Password.suffix(),
            SecretKind::ProxyPassword.suffix()
        );
    }

    /// 真实访问系统凭据库的集成测试。
    /// 默认忽略(避免在 CI/他人机器上写入真实凭据),需要时执行:
    /// `cargo test keyring_roundtrip -- --ignored`
    #[test]
    #[ignore]
    fn keyring_roundtrip() {
        let id = format!("__test__{}", uuid::Uuid::new_v4());
        set_secret(&id, SecretKind::Password, "s3cret").expect("set");
        assert_eq!(
            get_secret(&id, SecretKind::Password).unwrap().as_deref(),
            Some("s3cret")
        );
        delete_secret(&id, SecretKind::Password).expect("delete");
        assert_eq!(get_secret(&id, SecretKind::Password).unwrap(), None);
        delete_all(&id);
    }

    #[test]
    fn zeroize_clears_string() {
        let mut s = String::from("secret");
        zeroize_string(&mut s);
        assert!(s.is_empty());
    }
}
