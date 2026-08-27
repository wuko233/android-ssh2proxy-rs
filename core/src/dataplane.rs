use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot, Notify};

use crate::dns;
use crate::packet::{self, Ipv4Header};
use crate::socks5::Socks5Dialer;
use crate::tcpflow::{Flow, FlowAction, FlowKey, FlowState};

const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

enum UpstreamMsg {
    Data(Vec<u8>),
    Eof,
    ConnectFailed,
}

struct FlowHandle {
    flow: Flow,
    c2r_tx: mpsc::UnboundedSender<Vec<u8>>,
    start_tx: Option<oneshot::Sender<()>>,
    last_activity: Instant,
}

pub struct DataPlane<T> {
    tun: T,
    socks: Socks5Dialer,
    dns_server: String,
    flows: HashMap<FlowKey, FlowHandle>,
    r2c_tx: mpsc::UnboundedSender<(FlowKey, UpstreamMsg)>,
    r2c_rx: mpsc::UnboundedReceiver<(FlowKey, UpstreamMsg)>,
    dns_tx: mpsc::UnboundedSender<Vec<u8>>,
    dns_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    stop: Arc<Notify>,
}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> DataPlane<T> {
    pub fn new(tun: T, socks: Socks5Dialer, dns_server: String) -> Self {
        let (r2c_tx, r2c_rx) = mpsc::unbounded_channel();
        let (dns_tx, dns_rx) = mpsc::unbounded_channel();
        Self {
            tun,
            socks,
            dns_server,
            flows: HashMap::new(),
            r2c_tx,
            r2c_rx,
            dns_tx,
            dns_rx,
            stop: Arc::new(Notify::new()),
        }
    }

    pub fn stop_handle(&self) -> Arc<Notify> {
        self.stop.clone()
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut buf = vec![0u8; 65536];
        loop {
            tokio::select! {
                r = self.tun.read(&mut buf) => {
                    let n = r?;
                    if n == 0 { continue; }
                    self.handle_tun_packet(&buf[..n]).await;
                    self.sweep_idle();
                }
                msg = self.r2c_rx.recv() => {
                    match msg {
                        Some((key, msg)) => self.handle_upstream(key, msg).await,
                        None => break,
                    }
                }
                pkt = self.dns_rx.recv() => {
                    match pkt {
                        Some(pkt) => { let _ = self.tun.write_all(&pkt).await; }
                        None => break,
                    }
                }
                _ = self.stop.notified() => {
                    log::info!("dataplane stop requested");
                    break;
                }
            }
        }
        Ok(())
    }

    async fn handle_tun_packet(&mut self, pkt: &[u8]) {
        let Some((iph, rest)) = packet::parse_ipv4(pkt) else { return };
        match iph.protocol {
            packet::IPV4_PROTO_TCP => self.handle_tcp(&iph, rest).await,
            packet::IPV4_PROTO_UDP => self.handle_udp(&iph, rest).await,
            _ => {}
        }
    }

    async fn handle_tcp(&mut self, iph: &Ipv4Header, seg: &[u8]) {
        let Some(tcp) = packet::parse_tcp(seg) else { return };
        let key = FlowKey { src_ip: iph.src, src_port: tcp.src_port, dst_ip: iph.dst, dst_port: tcp.dst_port };
        if !self.flows.contains_key(&key) {
            if !tcp.flags.syn {
                return;
            }
            log::debug!("new tcp flow {} -> {}:{}", ip_to_str(&key.src_ip), ip_to_str(&key.dst_ip), key.dst_port);
            let h = self.spawn_flow(key, tcp.seq);
            self.flows.insert(key, h);
        }
        let Some(h) = self.flows.get_mut(&key) else { return };
        h.last_activity = Instant::now();
        let actions = h.flow.handle_packet(&tcp);
        if h.flow.state == FlowState::Established {
            if let Some(tx) = h.start_tx.take() {
                let _ = tx.send(());
            }
        }
        self.apply(key, actions).await;
    }

    fn spawn_flow(&self, key: FlowKey, client_isn: u32) -> FlowHandle {
        let flow = Flow::new(key, client_isn, rand_isn());
        let (c2r_tx, mut c2r_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (start_tx, start_rx) = oneshot::channel();
        let r2c_tx = self.r2c_tx.clone();
        let socks = self.socks.clone();
        let dst_ip = key.dst_ip;
        let dst_port = key.dst_port;
        tokio::spawn(async move {
            let _ = start_rx.await;
            let mut s = match socks.connect(&ip_to_str(&dst_ip), dst_port).await {
                Ok(s) => s,
                Err(e) => { log::warn!("dial {}:{} failed: {e}", ip_to_str(&dst_ip), dst_port); let _ = r2c_tx.send((key, UpstreamMsg::ConnectFailed)); return; }
            };
            let mut b = [0u8; 32768];
            loop {
                tokio::select! {
                    data = c2r_rx.recv() => {
                        match data {
                            Some(d) if d.is_empty() => { let _ = s.shutdown().await; }
                            Some(d) => { if s.write_all(&d).await.is_err() { break; } }
                            None => { let _ = s.shutdown().await; break; }
                        }
                    }
                    r = s.read(&mut b) => {
                        match r {
                            Ok(0) | Err(_) => { let _ = r2c_tx.send((key, UpstreamMsg::Eof)); break; }
                            Ok(n) => { let _ = r2c_tx.send((key, UpstreamMsg::Data(b[..n].to_vec()))); }
                        }
                    }
                }
            }
        });
        FlowHandle { flow, c2r_tx, start_tx: Some(start_tx), last_activity: Instant::now() }
    }

    async fn handle_udp(&mut self, iph: &Ipv4Header, seg: &[u8]) {
        let Some(udp) = packet::parse_udp(seg) else { return };
        if udp.dst_port != 53 { return; }
        log::debug!("dns query from {:?}:{} ({} bytes)", iph.src, udp.src_port, udp.payload.len());
        let socks = self.socks.clone();
        let dns_server = self.dns_server.clone();
        let query = udp.payload.to_vec();
        let resp_iph = Ipv4Header { src: iph.dst, dst: iph.src, protocol: packet::IPV4_PROTO_UDP, total_len: 0 };
        let src_port = udp.dst_port;
        let dst_port = udp.src_port;
        let dns_tx = self.dns_tx.clone();
        tokio::spawn(async move {
            let answer = tokio::time::timeout(
                Duration::from_secs(10),
                dns::resolve(|h, p| async move { socks.connect(&h, p).await }, &dns_server, &query),
            )
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default();
            log::debug!("dns resolved {} bytes", answer.len());
            let pkt = packet::build_udp_packet(&resp_iph, src_port, dst_port, &answer);
            let _ = dns_tx.send(pkt);
        });
    }

    async fn handle_upstream(&mut self, key: FlowKey, msg: UpstreamMsg) {
        let Some(h) = self.flows.get_mut(&key) else { return };
        h.last_activity = Instant::now();
        let actions = match msg {
            UpstreamMsg::Data(d) => h.flow.handle_upstream(d),
            UpstreamMsg::Eof => h.flow.handle_upstream_eof(),
            UpstreamMsg::ConnectFailed => h.flow.handle_connect_failed(),
        };
        self.apply(key, actions).await;
    }

    fn sweep_idle(&mut self) {
        let now = Instant::now();
        self.flows.retain(|_, h| now.duration_since(h.last_activity) < IDLE_TIMEOUT);
    }

    async fn apply(&mut self, key: FlowKey, actions: Vec<FlowAction>) {
        let mut done = false;
        for a in actions {
            match a {
                FlowAction::SendToClient(pkt) => { let _ = self.tun.write_all(&pkt).await; }
                FlowAction::SendUpstream(d) => {
                    if let Some(h) = self.flows.get(&key) { let _ = h.c2r_tx.send(d); }
                }
                FlowAction::CloseUpstream => {
                    if let Some(h) = self.flows.get(&key) { let _ = h.c2r_tx.send(vec![]); }
                }
                FlowAction::Done => { done = true; }
            }
        }
        if done {
            self.flows.remove(&key);
        } else {
            self.flows.retain(|_, h| h.flow.state != FlowState::Closed);
        }
    }
}

static ISN: AtomicU32 = AtomicU32::new(0x5a5a_0000);
fn rand_isn() -> u32 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    ISN.fetch_add(1, Ordering::Relaxed).wrapping_add(t)
}
fn ip_to_str(ip: &[u8; 4]) -> String { format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{self, TcpFlags};

    #[tokio::test]
    async fn syn_triggers_synack() {
        let (tun_end, mut app_end) = tokio::io::duplex(65536);
        let socks = Socks5Dialer { addr: "127.0.0.1:1080".parse().unwrap() };
        let mut dp = DataPlane::new(tun_end, socks, "1.1.1.1".to_string());
        let handle = tokio::spawn(async move { dp.run().await });

        let iph = Ipv4Header { src: [10, 0, 0, 2], dst: [1, 2, 3, 4], protocol: 6, total_len: 0 };
        let syn = packet::build_tcp_packet(
            &iph,
            40000,
            443,
            1000,
            0,
            TcpFlags { syn: true, ..Default::default() },
            65535,
            &[],
        );
        app_end.write_all(&syn).await.unwrap();

        let mut buf = vec![0u8; 4096];
        let n = tokio::time::timeout(std::time::Duration::from_secs(2), app_end.read(&mut buf))
            .await
            .expect("timed out waiting for SYN-ACK")
            .expect("app_end closed");
        assert!(n > 0);

        let (parsed, rest) = packet::parse_ipv4(&buf[..n]).unwrap();
        assert_eq!(parsed.src, [1, 2, 3, 4]);
        assert_eq!(parsed.dst, [10, 0, 0, 2]);
        assert_eq!(parsed.protocol, 6);
        let tcp = packet::parse_tcp(rest).unwrap();
        assert!(tcp.flags.syn && tcp.flags.ack);
        assert_eq!(tcp.src_port, 443);
        assert_eq!(tcp.dst_port, 40000);

        handle.abort();
    }

    #[tokio::test]
    async fn non_syn_packet_does_not_create_flow() {
        let (tun_end, mut app_end) = tokio::io::duplex(65536);
        let socks = Socks5Dialer { addr: "127.0.0.1:1080".parse().unwrap() };
        let mut dp = DataPlane::new(tun_end, socks, "1.1.1.1".to_string());
        let handle = tokio::spawn(async move { dp.run().await });

        let iph = Ipv4Header { src: [10, 0, 0, 2], dst: [1, 2, 3, 4], protocol: 6, total_len: 0 };
        let ack = packet::build_tcp_packet(
            &iph,
            40000,
            443,
            1001,
            5001,
            TcpFlags { ack: true, ..Default::default() },
            65535,
            &[],
        );
        app_end.write_all(&ack).await.unwrap();

        let mut buf = vec![0u8; 4096];
        let read = tokio::time::timeout(std::time::Duration::from_millis(200), app_end.read(&mut buf))
            .await;
        assert!(read.is_err(), "expected no response to non-SYN packet");

        handle.abort();
    }
}