use anyhow::Context;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::ssh::SshClient;

fn frame(msg: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(msg.len() + 2);
    out.extend_from_slice(&(msg.len() as u16).to_be_bytes());
    out.extend_from_slice(msg);
    out
}

fn unframe(buf: &[u8]) -> anyhow::Result<(Vec<u8>, &[u8])> {
    if buf.len() < 2 {
        anyhow::bail!("short dns tcp frame");
    }
    let len = u16::from_be_bytes([buf[0], buf[1]]) as usize;
    if buf.len() < 2 + len {
        anyhow::bail!("truncated dns tcp frame");
    }
    Ok((buf[2..2 + len].to_vec(), &buf[2 + len..]))
}

pub async fn resolve_over_tcpip(
    ssh: &SshClient,
    dns_server: &str,
    query: &[u8],
) -> anyhow::Result<Vec<u8>> {
    let mut stream = ssh
        .open_tcpip(dns_server, 53)
        .await
        .context("open dns tcpip")?
        .into_stream();
    stream.write_all(&frame(query)).await?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    let (payload, _) = unframe(&buf)?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_frame_roundtrip() {
        let q = vec![1u8, 2, 3, 4, 5];
        let framed = frame(&q);
        assert_eq!(&framed[..2], &[0, 5]);
        assert_eq!(&framed[2..], &q[..]);
        let (payload, rest) = unframe(&framed).unwrap();
        assert_eq!(payload, q);
        assert!(rest.is_empty());
    }
}
