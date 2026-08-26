use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

fn frame(msg: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(msg.len() + 2);
    out.extend_from_slice(&(msg.len() as u16).to_be_bytes());
    out.extend_from_slice(msg);
    out
}

#[allow(dead_code)]
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

pub async fn resolve<F, Fut, S>(
    connect: F,
    dns_server: &str,
    query: &[u8],
) -> anyhow::Result<Vec<u8>>
where
    F: FnOnce(String, u16) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<S>>,
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut stream = connect(dns_server.to_string(), 53).await?;
    stream.write_all(&frame(query)).await?;
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf) as usize;
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;
    Ok(body)
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

    #[test]
    fn unframe_rejects_truncated() {
        assert!(unframe(&[0, 10, 1, 2]).is_err());
    }

    #[test]
    fn unframe_returns_rest() {
        let buf = vec![0, 2, 9, 9, 0, 3, 1, 2, 3];
        let (payload, rest) = unframe(&buf).unwrap();
        assert_eq!(payload, vec![9, 9]);
        assert_eq!(rest, vec![0, 3, 1, 2, 3]);
    }
}
