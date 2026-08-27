use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

/// 服务器端 UDP 中继脚本（Python 3）的 base64 编码。
/// 源码见仓库 `server/udprelay.py`。
const UDP_RELAY_SCRIPT_B64: &str = "IyEvdXNyL2Jpbi9lbnYgcHl0aG9uMwoiIiJVRFAgcmVsYXkgZm9yIFNTSDJQcm94eS4KClJ1bnMgb24gdGhlIFNTSCBzZXJ2ZXIsIHN0YXJ0ZWQgYnkgdGhlIGNsaWVudCBvdmVyIGFuIFNTSCBleGVjIGNoYW5uZWwuCkNvbW11bmljYXRlcyBvdmVyIHN0ZGluL3N0ZG91dCB3aXRoIGxlbmd0aC1wcmVmaXhlZCBmcmFtZXMuCgpSZXF1ZXN0IGZyYW1lIChjbGllbnQgLT4gcmVsYXkpOgogICAgWzItYnl0ZSBiaWctZW5kaWFuIGxlbmd0aF1bNC1ieXRlIGRzdCBJUHY0XVsyLWJ5dGUgZHN0IHBvcnRdW1VEUCBwYXlsb2FkXQoKUmVzcG9uc2UgZnJhbWUgKHJlbGF5IC0+IGNsaWVudCk6CiAgICBbMi1ieXRlIGJpZy1lbmRpYW4gbGVuZ3RoXVs0LWJ5dGUgc3JjIElQdjRdWzItYnl0ZSBzcmMgcG9ydF1bVURQIHBheWxvYWRdCiIiIgppbXBvcnQgc29ja2V0CmltcG9ydCBzdHJ1Y3QKaW1wb3J0IHN5cwppbXBvcnQgc2VsZWN0Cgpzb2NrID0gc29ja2V0LnNvY2tldChzb2NrZXQuQUZfSU5FVCwgc29ja2V0LlNPQ0tfREdSQU0pCnNvY2suYmluZCgoIjAuMC4wLjAiLCAwKSkKCnN0ZGluID0gc3lzLnN0ZGluLmJ1ZmZlcgpzdGRvdXQgPSBzeXMuc3Rkb3V0LmJ1ZmZlcgoKCmRlZiByZWFkX2ZyYW1lKCk6CiAgICBoZHIgPSBzdGRpbi5yZWFkKDIpCiAgICBpZiBsZW4oaGRyKSA8IDI6CiAgICAgICAgcmV0dXJuIE5vbmUKICAgIChsZW5ndGgsKSA9IHN0cnVjdC51bnBhY2soIj5IIiwgaGRyKQogICAgZGF0YSA9IHN0ZGluLnJlYWQobGVuZ3RoKQogICAgaWYgbGVuKGRhdGEpIDwgbGVuZ3RoOgogICAgICAgIHJldHVybiBOb25lCiAgICByZXR1cm4gZGF0YQoKCndoaWxlIFRydWU6CiAgICByZWFkYWJsZSwgXywgXyA9IHNlbGVjdC5zZWxlY3QoW3N5cy5zdGRpbiwgc29ja10sIFtdLCBbXSkKICAgIGlmIHN5cy5zdGRpbiBpbiByZWFkYWJsZToKICAgICAgICBmcmFtZSA9IHJlYWRfZnJhbWUoKQogICAgICAgIGlmIGZyYW1lIGlzIE5vbmU6CiAgICAgICAgICAgIGJyZWFrCiAgICAgICAgaWYgbGVuKGZyYW1lKSA+PSA2OgogICAgICAgICAgICBpcCA9IHNvY2tldC5pbmV0X250b2EoZnJhbWVbMDo0XSkKICAgICAgICAgICAgcG9ydCA9IHN0cnVjdC51bnBhY2soIj5IIiwgZnJhbWVbNDo2XSlbMF0KICAgICAgICAgICAgc29jay5zZW5kdG8oZnJhbWVbNjpdLCAoaXAsIHBvcnQpKQogICAgaWYgc29jayBpbiByZWFkYWJsZToKICAgICAgICB0cnk6CiAgICAgICAgICAgIGRhdGEsIGFkZHIgPSBzb2NrLnJlY3Zmcm9tKDY1NTM1KQogICAgICAgICAgICBzcmMgPSBzb2NrZXQuaW5ldF9hdG9uKGFkZHJbMF0pICsgc3RydWN0LnBhY2soIj5IIiwgYWRkclsxXSkKICAgICAgICAgICAgc3Rkb3V0LndyaXRlKHN0cnVjdC5wYWNrKCI+SCIsIGxlbihzcmMpICsgbGVuKGRhdGEpKSArIHNyYyArIGRhdGEpCiAgICAgICAgICAgIHN0ZG91dC5mbHVzaCgpCiAgICAgICAgZXhjZXB0IEV4Y2VwdGlvbjoKICAgICAgICAgICAgcGFzcw==";

/// 生成在服务器上启动 UDP 中继的 shell 命令（无需在服务器上放文件，内嵌脚本）。
pub fn relay_command() -> String {
    format!(
        "python3 -u -c \"import base64;exec(base64.b64decode('{UDP_RELAY_SCRIPT_B64}'))\""
    )
}

/// UDP 数据报在 SSH 通道内的帧格式（与服务器端 relay 脚本一致）：
///
/// 请求（客户端 -> 服务器）：
///   [2 字节大端长度 L][4 字节目的 IPv4][2 字节目的端口][L-6 字节 UDP 载荷]
///
/// 响应（服务器 -> 客户端）：
///   [2 字节大端长度 L][4 字节源 IPv4][2 字节源端口][L-6 字节 UDP 载荷]
///
/// 响应中的「源地址」通常是当初请求的「目的地址」，客户端据此关联回原始 UDP 流。
pub struct UdpRelay {
    request_tx: mpsc::UnboundedSender<Vec<u8>>,
    pub response_rx: mpsc::UnboundedReceiver<([u8; 4], u16, Vec<u8>)>,
}

impl UdpRelay {
    /// `stream` 是连到服务器 relay 的 SSH 通道（AsyncRead + AsyncWrite，stdin/stdout）。
    /// 内部起两个任务：一个把请求帧写进通道，一个从通道读响应帧。
    pub fn new<S>(stream: S) -> Self
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let (request_tx, mut request_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (response_tx, response_rx) = mpsc::unbounded_channel::<([u8; 4], u16, Vec<u8>)>();

        let (mut read_half, mut write_half) = tokio::io::split(stream);

        tokio::spawn(async move {
            while let Some(frame) = request_rx.recv().await {
                if write_half.write_all(&frame).await.is_err() {
                    break;
                }
            }
        });

        tokio::spawn(async move {
            let mut len_buf = [0u8; 2];
            loop {
                if read_half.read_exact(&mut len_buf).await.is_err() {
                    break;
                }
                let len = u16::from_be_bytes(len_buf) as usize;
                if len < 6 {
                    break;
                }
                let mut body = vec![0u8; len];
                if read_half.read_exact(&mut body).await.is_err() {
                    break;
                }
                let ip = [body[0], body[1], body[2], body[3]];
                let port = u16::from_be_bytes([body[4], body[5]]);
                let payload = body[6..].to_vec();
                if response_tx.send((ip, port, payload)).is_err() {
                    break;
                }
            }
        });

        Self {
            request_tx,
            response_rx,
        }
    }

    /// 封装并发送一个 UDP 数据报到 `dst_ip:dst_port`。
    pub fn send(&self, dst_ip: [u8; 4], dst_port: u16, payload: &[u8]) {
        let len = 6 + payload.len();
        let mut frame = Vec::with_capacity(2 + len);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
        frame.extend_from_slice(&dst_ip);
        frame.extend_from_slice(&dst_port.to_be_bytes());
        frame.extend_from_slice(payload);
        let _ = self.request_tx.send(frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_layout_matches_protocol() {
        let dst_ip = [1, 2, 3, 4];
        let dst_port = 53u16;
        let payload = b"hello";
        let len = 6 + payload.len();
        let mut frame = Vec::new();
        frame.extend_from_slice(&(len as u16).to_be_bytes());
        frame.extend_from_slice(&dst_ip);
        frame.extend_from_slice(&dst_port.to_be_bytes());
        frame.extend_from_slice(payload);

        // 前 2 字节是长度，接下来是 IP 和端口
        assert_eq!(&frame[0..2], &(11u16).to_be_bytes());
        assert_eq!(&frame[2..6], &[1, 2, 3, 4]);
        assert_eq!(&frame[6..8], &53u16.to_be_bytes());
        assert_eq!(&frame[8..], b"hello");
    }

    #[test]
    fn udp_relay_roundtrip_over_duplex() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            // 用 duplex 模拟 SSH 通道：客户端写请求、读响应；另一侧模拟服务器。
            let (client_side, mut server_side) = tokio::io::duplex(65536);
            let mut relay = UdpRelay::new(client_side);

            // 模拟服务器：读请求帧，回一个响应帧
            tokio::spawn(async move {
                let mut len_buf = [0u8; 2];
                server_side.read_exact(&mut len_buf).await.unwrap();
                let len = u16::from_be_bytes(len_buf) as usize;
                let mut body = vec![0u8; len];
                server_side.read_exact(&mut body).await.unwrap();
                assert_eq!(&body[0..4], &[1, 2, 3, 4]);
                assert_eq!(&body[4..6], &53u16.to_be_bytes());
                assert_eq!(&body[6..], b"hello");

                let resp_payload = b"world";
                let resp_len = 6 + resp_payload.len();
                let mut resp = Vec::new();
                resp.extend_from_slice(&(resp_len as u16).to_be_bytes());
                resp.extend_from_slice(&[1, 2, 3, 4]);
                resp.extend_from_slice(&53u16.to_be_bytes());
                resp.extend_from_slice(resp_payload);
                server_side.write_all(&resp).await.unwrap();
            });

            relay.send([1, 2, 3, 4], 53, b"hello");
            let (ip, port, payload) = relay.response_rx.recv().await.unwrap();
            assert_eq!(ip, [1, 2, 3, 4]);
            assert_eq!(port, 53);
            assert_eq!(payload, b"world");
        });
    }
}