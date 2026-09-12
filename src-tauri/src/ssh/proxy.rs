//! SSH 代理支持:SOCKS5 与 HTTP CONNECT。
//!
//! 返回一个已经与目标主机建立 TCP 连接的流,交给 `russh::client::connect_stream`
//! 完成 SSH 握手。这样代理只影响传输层,SSH 逻辑无需感知。

use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::store::ProxyConfig;

/// 代理类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProxyKind {
    Socks5,
    HttpConnect,
}

impl ProxyKind {
    /// 从配置字符串解析,默认 SOCKS5
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "http" | "https" | "http_connect" | "http-connect" => Self::HttpConnect,
            _ => Self::Socks5,
        }
    }
}

/// 通过代理建立到 `host:port` 的 TCP 连接
pub async fn connect_through_proxy(
    host: &str,
    port: u16,
    proxy: &ProxyConfig,
) -> Result<TcpStream, String> {
    let proxy_addr = format!("{}:{}", proxy.host, proxy.port);
    let kind = ProxyKind::parse(proxy.kind.as_deref().unwrap_or("socks5"));
    let target = format!("{host}:{port}");

    // 先探测代理端口是否可达,便于给出与"目标不可达"区分的错误
    tokio::time::timeout(
        Duration::from_secs(10),
        TcpStream::connect(&proxy_addr),
    )
    .await
    .map_err(|_| format!("连接代理 {proxy_addr} 超时"))?
    .map_err(|e| format!("连接代理 {proxy_addr} 失败: {e}"))?;

    match kind {
        ProxyKind::Socks5 => connect_socks5(host, port, proxy, &proxy_addr).await,
        ProxyKind::HttpConnect => connect_http_connect(host, port, proxy, &proxy_addr, &target).await,
    }
}

async fn connect_socks5(
    host: &str,
    port: u16,
    proxy: &ProxyConfig,
    proxy_addr: &str,
) -> Result<TcpStream, String> {
    use tokio_socks::tcp::Socks5Stream;

    let stream = match (
        proxy.username.as_deref().filter(|u| !u.is_empty()),
        proxy.password.as_deref(),
    ) {
        (Some(user), Some(pass)) => {
            Socks5Stream::connect_with_password(proxy_addr, (host, port), user, pass).await
        }
        _ => Socks5Stream::connect(proxy_addr, (host, port)).await,
    }
    .map_err(|e| format!("SOCKS5 代理连接失败: {e}"))?;

    Ok(stream.into_inner())
}

async fn connect_http_connect(
    _host: &str,
    _port: u16,
    proxy: &ProxyConfig,
    proxy_addr: &str,
    target: &str,
) -> Result<TcpStream, String> {
    let mut stream = tokio::time::timeout(
        Duration::from_secs(10),
        TcpStream::connect(proxy_addr),
    )
    .await
    .map_err(|_| format!("连接代理 {proxy_addr} 超时"))?
    .map_err(|e| format!("连接代理 {proxy_addr} 失败: {e}"))?;

    let mut request = format!(
        "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\nProxy-Connection: Keep-Alive\r\n"
    );
    if let (Some(user), Some(pass)) = (
        proxy.username.as_deref().filter(|u| !u.is_empty()),
        proxy.password.as_deref(),
    ) {
        let token = B64.encode(format!("{user}:{pass}"));
        request.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
    }
    request.push_str("\r\n");

    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| format!("发送 CONNECT 请求失败: {e}"))?;

    // 读取响应头,直到 \r\n\r\n
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 256];
    loop {
        let n = tokio::time::timeout(Duration::from_secs(15), stream.read(&mut chunk))
            .await
            .map_err(|_| "代理响应超时".to_string())?
            .map_err(|e| format!("读取代理响应失败: {e}"))?;
        if n == 0 {
            return Err("代理在建立隧道前关闭了连接".into());
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() > 16 * 1024 {
            return Err("代理响应头过大".into());
        }
    }

    let head = String::from_utf8_lossy(&buf);
    let status_line = head.lines().next().unwrap_or("");
    let code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .unwrap_or(0);
    if code != 200 {
        return Err(format!("代理拒绝建立隧道: {status_line}"));
    }

    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_kind_parses_known_values() {
        assert_eq!(ProxyKind::parse("socks5"), ProxyKind::Socks5);
        assert_eq!(ProxyKind::parse("SOCKS5"), ProxyKind::Socks5);
        assert_eq!(ProxyKind::parse("http"), ProxyKind::HttpConnect);
        assert_eq!(ProxyKind::parse("HTTP_CONNECT"), ProxyKind::HttpConnect);
        assert_eq!(ProxyKind::parse(""), ProxyKind::Socks5);
    }
}
