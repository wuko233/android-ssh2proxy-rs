use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use fast_socks5::server::{transfer, Socks5ServerProtocol};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::ssh::SshClient;

pub async fn splice<A, B>(a: &mut A, b: &mut B)
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let (mut ar, mut aw) = tokio::io::split(a);
    let (mut br, mut bw) = tokio::io::split(b);
    let ab = tokio::io::copy(&mut ar, &mut bw);
    let ba = tokio::io::copy(&mut br, &mut aw);
    let _ = tokio::join!(ab, ba);
}

pub fn start_socks5_server(ssh: Arc<SshClient>, bind: SocketAddr) -> JoinHandle<()> {
    tokio::spawn(async move {
        let listener = match TcpListener::bind(bind).await {
            Ok(l) => l,
            Err(e) => {
                log::error!("failed to bind SOCKS5 listener on {bind}: {e}");
                return;
            }
        };
        loop {
            let Ok((socket, _)) = listener.accept().await else { continue };
            let ssh = ssh.clone();
            tokio::spawn(async move {
                let _ = serve(ssh, socket).await;
            });
        }
    })
}

async fn serve(ssh: Arc<SshClient>, socket: tokio::net::TcpStream) -> anyhow::Result<()> {
    let proto = Socks5ServerProtocol::accept_no_auth(socket).await?;
    let (proto, cmd, target) = proto.read_command().await?;
    if !matches!(cmd, fast_socks5::Socks5Command::TCPConnect) {
        proto
            .reply_error(&fast_socks5::ReplyError::CommandNotSupported)
            .await?;
        anyhow::bail!("non-TCP command");
    }
    let (host, port) = target.into_string_and_port();
    let channel = ssh.open_tcpip(&host, port).await.context("open_tcpip")?;
    let stream = channel.into_stream();
    let inner = proto
        .reply_success(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await?;
    transfer(inner, stream).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn splice_relays_bytes_both_ways() {
        let (a1, a2) = tokio::io::duplex(1024);
        let (b1, b2) = tokio::io::duplex(1024);
        let (mut a1, mut b1) = (a1, b1);
        let task = tokio::spawn(async move {
            let mut a2 = a2;
            let mut b2 = b2;
            splice(&mut a2, &mut b2).await;
        });
        a1.write_all(b"hello").await.unwrap();
        let mut buf = [0u8; 5];
        b1.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
        b1.write_all(b"world").await.unwrap();
        let mut buf2 = [0u8; 5];
        a1.read_exact(&mut buf2).await.unwrap();
        assert_eq!(&buf2, b"world");
        drop(a1);
        drop(b1);
        let _ = task.await;
    }
}
