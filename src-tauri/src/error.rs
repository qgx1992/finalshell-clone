//! 统一错误类型。
//!
//! 前端需要区分"主机指纹待确认"和普通连接失败,才能弹出正确的确认框。
//! 因此错误不是裸字符串,而是带 code 的结构化对象。

use serde::Serialize;
use ts_rs::TS;

use crate::security::known_hosts::HostKeyObservation;

// 该枚举是前后端错误契约的一部分:部分变体当前未在 Rust 侧构造,
// 但前端会依据 code 处理,因此不因未使用而删除。
#[allow(dead_code)]
#[derive(Debug, Serialize, TS)]
#[serde(tag = "code", rename_all = "snake_case")]
#[ts(export, export_to = "../../src/generated/")]
pub enum AppError {
    /// 首次连接,需要用户确认指纹
    HostKeyUnknown { observation: HostKeyObservation },
    /// 指纹与本机记录不一致,可能是中间人攻击
    HostKeyChanged { observation: HostKeyObservation },
    /// 会话不存在
    SessionNotFound {
        #[ts(type = "number")]
        id: u64,
    },
    /// 认证失败
    AuthFailed { message: String },
    /// 配置或凭据问题
    Config { message: String },
    /// 网络/代理问题
    Network { message: String },
    /// 操作超时
    Timeout { message: String },
    /// 其它错误
    Other { message: String },
}

impl AppError {
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config { message: message.into() }
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::Network { message: message.into() }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::HostKeyUnknown { observation } => write!(
                f,
                "首次连接 {}:{},需要确认服务器指纹",
                observation.host, observation.port
            ),
            AppError::HostKeyChanged { observation } => write!(
                f,
                "服务器 {}:{} 的主机密钥已变更,可能存在中间人攻击",
                observation.host, observation.port
            ),
            AppError::SessionNotFound { id } => write!(f, "会话 {id} 不存在"),
            AppError::AuthFailed { message } => write!(f, "认证失败: {message}"),
            AppError::Config { message } => write!(f, "配置错误: {message}"),
            AppError::Network { message } => write!(f, "网络错误: {message}"),
            AppError::Timeout { message } => write!(f, "超时: {message}"),
            AppError::Other { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<String> for AppError {
    fn from(value: String) -> Self {
        AppError::Other { message: value }
    }
}

impl From<&str> for AppError {
    fn from(value: &str) -> Self {
        AppError::Other { message: value.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_serializes_with_code() {
        let err = AppError::SessionNotFound { id: 7 };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "session_not_found");
        assert_eq!(json["id"], 7);
    }

    #[test]
    fn displays_human_readable_message() {
        let err = AppError::Config { message: "缺少密码".into() };
        assert!(err.to_string().contains("缺少密码"));
    }
}
