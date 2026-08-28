use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use crate::socks5::splice;
use crate::ssh::SshClient;

/// 启动一个本地 HTTP 代理（支持 CONNECT 隧道与普通 HTTP 转发），
/// 每个连接经 SSH `direct-tcpip` 转发到目标。
pub fn start_http_proxy(ssh: Arc<SshClient>, bind: SocketAddr) -> JoinHandle<()> {
    tokio::spawn(async move {
        let listener = match TcpListener::bind(bind).await {
            Ok(l) => l,
            Err(e) => {
                log::error!("failed to bind HTTP proxy on {bind}: {e}");
                return;
            }
        };
        loop {
            let Ok((socket, _)) = listener.accept().await else { continue };
            let ssh = ssh.clone();
            tokio::spawn(async move {
                if let Err(e) = serve_http(ssh, socket).await {
                    log::debug!("http proxy connection ended: {e}");
                }
            });
        }
    })
}

async fn serve_http(ssh: Arc<SshClient>, mut socket: TcpStream) -> anyhow::Result<()> {
    // 读取请求头（直到 \r\n\r\n）
    let (head, rest) = read_head(&mut socket).await?;
    let (method, target, host) = parse_head(&head)?;

    let (dst_host, dst_port) = parse_host_port(&target, &host, method == "CONNECT")
        .context("no target host")?;

    let channel = ssh
        .open_tcpip(&dst_host, dst_port)
        .await
        .context("open_tcpip")?;
    let mut stream = channel.into_stream();

    if method == "CONNECT" {
        socket
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        if !rest.is_empty() {
            stream.write_all(&rest).await?;
        }
        splice(&mut socket, &mut stream).await;
    } else {
        // 普通 HTTP：把请求行改写为 origin-form，其余头部原样保留后转发
        let path = origin_path(&target);
        let first_end = find_subsequence(&head, b"\r\n").context("malformed request head")?;
        let mut forward = Vec::with_capacity(head.len() + rest.len() + 16);
        forward.extend_from_slice(format!("{method} {path} HTTP/1.1\r\n").as_bytes());
        forward.extend_from_slice(&head[first_end + 2..]);
        forward.extend_from_slice(&rest);
        stream.write_all(&forward).await?;
        splice(&mut socket, &mut stream).await;
    }
    Ok(())
}

async fn read_head(socket: &mut TcpStream) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = socket.read(&mut tmp).await?;
        if n == 0 {
            anyhow::bail!("connection closed before request head");
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
            let head = buf[..pos + 4].to_vec();
            let rest = buf[pos + 4..].to_vec();
            return Ok((head, rest));
        }
        if buf.len() > 65536 {
            anyhow::bail!("request head too large");
        }
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 返回 (method, target, host_header)
fn parse_head(head: &[u8]) -> anyhow::Result<(String, String, String)> {
    let text = std::str::from_utf8(head).context("non-utf8 request head")?;
    let mut lines = text.lines();
    let req_line = lines.next().context("empty request")?;
    let mut parts = req_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    let mut host = String::new();
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if lower.strip_prefix("host:").is_some() {
            host = line["host:".len()..].trim().to_string();
            break;
        }
    }
    Ok((method, target, host))
}

fn parse_host_port(target: &str, host_header: &str, is_connect: bool) -> Option<(String, u16)> {
    let hostport = if is_connect {
        target.to_string()
    } else if let Some(rest) = target.split_once("://") {
        rest.1.split('/').next().unwrap_or("").to_string()
    } else {
        host_header.to_string()
    };
    if let Some(pos) = hostport.rfind(':') {
        if let Ok(port) = hostport[pos + 1..].parse::<u16>() {
            let host = &hostport[..pos];
            if !host.is_empty() {
                return Some((host.to_string(), port));
            }
        }
    }
    if hostport.is_empty() {
        None
    } else {
        Some((hostport, 80))
    }
}

fn origin_path(target: &str) -> String {
    if let Some(rest) = target.split_once("://") {
        if let Some(pos) = rest.1.find('/') {
            rest.1[pos..].to_string()
        } else {
            "/".to_string()
        }
    } else {
        target.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_connect_target() {
        assert_eq!(
            parse_host_port("example.com:443", "", true),
            Some(("example.com".into(), 443))
        );
    }

    #[test]
    fn parses_absolute_http_target() {
        assert_eq!(
            parse_host_port("http://example.com:8080/a/b", "", false),
            Some(("example.com".into(), 8080))
        );
    }

    #[test]
    fn parses_host_header_default_port() {
        assert_eq!(
            parse_host_port("/path", "example.com", false),
            Some(("example.com".into(), 80))
        );
    }

    #[test]
    fn rewrites_origin_path() {
        assert_eq!(origin_path("http://example.com/a/b?c=1"), "/a/b?c=1");
        assert_eq!(origin_path("/plain"), "/plain");
    }

    #[test]
    fn finds_header_end() {
        let buf = b"GET / HTTP/1.1\r\nHost: x\r\n\r\nbody";
        assert_eq!(find_subsequence(buf, b"\r\n\r\n"), Some(23));
    }
}