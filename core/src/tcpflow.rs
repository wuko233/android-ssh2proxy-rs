use std::collections::VecDeque;

use crate::packet::{TcpFlags, TcpSegment};

/// pending 缓冲区上限（1MB），防止窗口为 0 时无限缓冲上游数据。
const MAX_PENDING_BYTES: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlowKey {
    pub src_ip: [u8; 4],
    pub src_port: u16,
    pub dst_ip: [u8; 4],
    pub dst_port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowState {
    SynReceived,
    SynAckSent,
    Established,
    Closing,
    Closed,
}

pub enum FlowAction {
    /// IP 包，写回 tun 给客户端
    SendToClient(Vec<u8>),
    /// 载荷字节，写往 SOCKS5
    SendUpstream(Vec<u8>),
    /// 通知 SOCKS5 侧 EOF（FIN）
    CloseUpstream,
    /// 流结束，可从流表移除
    Done,
}

/// 序号比较（处理 32 位回绕）：`seq_ge(a, b)` 为 a 在序号空间上 >= b。
fn seq_ge(a: u32, b: u32) -> bool {
    a.wrapping_sub(b) < 0x8000_0000
}

fn seq_ahead(a: u32, b: u32) -> bool {
    a != b && a.wrapping_sub(b) < 0x8000_0000
}

/// 已发送但未确认的段（用于重传）。
struct RtEntry {
    seq: u32,
    data: Vec<u8>,
}

pub struct Flow {
    pub key: FlowKey,
    pub state: FlowState,
    client_isn: u32,
    our_isn: u32,
    our_seq: u32,
    our_acked: u32,
    next_client_seq: u32,
    window: u16,
    retransmit_buf: VecDeque<RtEntry>,
    pending: VecDeque<Vec<u8>>,
    pending_bytes: usize,
    /// 上游已 EOF，等待未确认数据清空后再发 FIN。
    fin_pending: bool,
}

impl Flow {
    pub fn new(key: FlowKey, client_isn: u32, our_isn: u32) -> Self {
        Self {
            key,
            state: FlowState::SynReceived,
            client_isn,
            our_isn,
            our_seq: our_isn.wrapping_add(1),
            our_acked: our_isn,
            next_client_seq: client_isn.wrapping_add(1),
            window: 65535,
            retransmit_buf: VecDeque::new(),
            pending: VecDeque::new(),
            pending_bytes: 0,
            fin_pending: false,
        }
    }

    pub fn handle_packet(&mut self, seg: &TcpSegment) -> Vec<FlowAction> {
        let mut out = Vec::new();
        if seg.flags.rst {
            self.state = FlowState::Closed;
            out.push(FlowAction::CloseUpstream);
            out.push(FlowAction::Done);
            return out;
        }
        match self.state {
            FlowState::SynReceived => {
                if seg.flags.syn {
                    self.state = FlowState::SynAckSent;
                    out.push(FlowAction::SendToClient(self.synack()));
                }
            }
            FlowState::SynAckSent => {
                if seg.flags.ack {
                    self.state = FlowState::Established;
                    self.our_acked = self.our_isn.wrapping_add(1);
                } else if seg.flags.syn {
                    out.push(FlowAction::SendToClient(self.synack()));
                }
            }
            FlowState::Established => {
                self.window = seg.window;
                let advanced = seq_ahead(seg.ack, self.our_acked);
                if advanced {
                    self.our_acked = seg.ack;
                    self.drop_acked();
                }

                let mut should_ack = false;

                // 客户端 -> 上游：转发新数据（重传幂等）
                if !seg.payload.is_empty() {
                    should_ack = true;
                    if seg.seq == self.next_client_seq {
                        self.next_client_seq =
                            self.next_client_seq.wrapping_add(seg.payload.len() as u32);
                        out.push(FlowAction::SendUpstream(seg.payload.to_vec()));
                    }
                }

                // 客户端 FIN：ACK + 半关闭上游（我方 FIN 等数据清空后由上游 EOF 触发）
                if seg.flags.fin {
                    should_ack = true;
                    self.next_client_seq = self.next_client_seq.wrapping_add(1);
                    out.push(FlowAction::CloseUpstream);
                }

                if should_ack {
                    out.push(FlowAction::SendToClient(self.ack_only()));
                }

                if advanced {
                    // 窗口打开：冲刷 pending；若上游已关且数据清空，发 FIN 收尾
                    out.extend(self.flush_pending());
                    if self.fin_pending
                        && self.retransmit_buf.is_empty()
                        && self.pending.is_empty()
                    {
                        self.fin_pending = false;
                        self.state = FlowState::Closing;
                        out.push(FlowAction::SendToClient(self.take_fin()));
                        out.push(FlowAction::Done);
                    }
                } else if !self.retransmit_buf.is_empty() {
                    // 重复 ACK：重传最早未确认段
                    let e = self.retransmit_buf.front().unwrap();
                    let seq = e.seq;
                    let data = e.data.clone();
                    out.push(FlowAction::SendToClient(self.data_packet_with_seq(seq, &data)));
                }
            }
            FlowState::Closing | FlowState::Closed => {}
        }
        out
    }

    pub fn handle_upstream(&mut self, data: Vec<u8>) -> Vec<FlowAction> {
        if self.state != FlowState::Established {
            return vec![];
        }
        if self.pending_bytes + data.len() > MAX_PENDING_BYTES {
            log::warn!("flow pending buffer overflow, dropping {} bytes", data.len());
            return vec![];
        }
        self.pending_bytes += data.len();
        self.pending.push_back(data);
        self.flush_pending()
    }

    pub fn handle_upstream_eof(&mut self) -> Vec<FlowAction> {
        if self.state != FlowState::Established {
            return vec![FlowAction::Done];
        }
        let mut out = self.flush_pending();
        if self.retransmit_buf.is_empty() && self.pending.is_empty() {
            self.state = FlowState::Closing;
            out.push(FlowAction::SendToClient(self.take_fin()));
            out.push(FlowAction::Done);
        } else {
            // 还有未确认数据，FIN 等清空后再发
            self.fin_pending = true;
        }
        out
    }

    pub fn handle_connect_failed(&mut self) -> Vec<FlowAction> {
        self.state = FlowState::Closed;
        vec![FlowAction::SendToClient(self.rst_packet()), FlowAction::Done]
    }

    fn drop_acked(&mut self) {
        while let Some(e) = self.retransmit_buf.front() {
            let end = e.seq.wrapping_add(e.data.len() as u32);
            if seq_ge(self.our_acked, end) {
                self.retransmit_buf.pop_front();
            } else {
                break;
            }
        }
    }

    /// 在窗口允许的范围内，把 pending 中的上游数据发出去。
    fn flush_pending(&mut self) -> Vec<FlowAction> {
        let mut out = Vec::new();
        loop {
            let unacked = self.our_seq.wrapping_sub(self.our_acked);
            if unacked >= self.window as u32 {
                break;
            }
            let Some(data) = self.pending.pop_front() else { break };
            self.pending_bytes -= data.len();
            let seq = self.our_seq;
            out.push(FlowAction::SendToClient(self.data_packet_with_seq(seq, &data)));
            self.our_seq = self.our_seq.wrapping_add(data.len() as u32);
            self.retransmit_buf.push_back(RtEntry { seq, data });
        }
        out
    }

    fn iph(&self) -> crate::packet::Ipv4Header {
        crate::packet::Ipv4Header {
            src: self.key.dst_ip,
            dst: self.key.src_ip,
            protocol: crate::packet::IPV4_PROTO_TCP,
            total_len: 0,
        }
    }

    fn synack(&self) -> Vec<u8> {
        let iph = self.iph();
        crate::packet::build_tcp_packet(
            &iph,
            self.key.dst_port,
            self.key.src_port,
            self.our_isn,
            self.client_isn.wrapping_add(1),
            TcpFlags { syn: true, ack: true, ..Default::default() },
            self.window,
            &[],
        )
    }

    fn ack_only(&self) -> Vec<u8> {
        let iph = self.iph();
        crate::packet::build_tcp_packet(
            &iph,
            self.key.dst_port,
            self.key.src_port,
            self.our_seq,
            self.next_client_seq,
            TcpFlags { ack: true, ..Default::default() },
            self.window,
            &[],
        )
    }

    fn data_packet_with_seq(&self, seq: u32, data: &[u8]) -> Vec<u8> {
        let iph = self.iph();
        crate::packet::build_tcp_packet(
            &iph,
            self.key.dst_port,
            self.key.src_port,
            seq,
            self.next_client_seq,
            TcpFlags { ack: true, psh: true, ..Default::default() },
            self.window,
            data,
        )
    }

    fn fin_packet(&self) -> Vec<u8> {
        let iph = self.iph();
        crate::packet::build_tcp_packet(
            &iph,
            self.key.dst_port,
            self.key.src_port,
            self.our_seq,
            self.next_client_seq,
            TcpFlags { ack: true, fin: true, ..Default::default() },
            self.window,
            &[],
        )
    }

    fn rst_packet(&self) -> Vec<u8> {
        let iph = self.iph();
        crate::packet::build_tcp_packet(
            &iph,
            self.key.dst_port,
            self.key.src_port,
            self.our_seq,
            self.next_client_seq,
            TcpFlags { rst: true, ack: true, ..Default::default() },
            self.window,
            &[],
        )
    }

    fn take_fin(&mut self) -> Vec<u8> {
        let pkt = self.fin_packet();
        self.our_seq = self.our_seq.wrapping_add(1);
        pkt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{TcpFlags, TcpSegment};

    fn seg<'a>(seq: u32, ack: u32, flags: TcpFlags, payload: &'a [u8]) -> TcpSegment<'a> {
        TcpSegment { src_port: 40000, dst_port: 443, seq, ack, flags, window: 65535, payload }
    }

    fn key() -> FlowKey {
        FlowKey { src_ip: [10, 0, 0, 2], src_port: 40000, dst_ip: [1, 2, 3, 4], dst_port: 443 }
    }

    fn establish(flow: &mut Flow) {
        flow.handle_packet(&seg(1000, 0, TcpFlags { syn: true, ..Default::default() }, &[]));
        flow.handle_packet(&seg(1001, 5001, TcpFlags { ack: true, ..Default::default() }, &[]));
        assert_eq!(flow.state, FlowState::Established);
    }

    #[test]
    fn syn_generates_synack() {
        let mut flow = Flow::new(key(), 1000, 5000);
        let actions = flow.handle_packet(&seg(1000, 0, TcpFlags { syn: true, ..Default::default() }, &[]));
        assert!(actions.iter().any(|a| matches!(a, FlowAction::SendToClient(_))));
        assert_eq!(flow.state, FlowState::SynAckSent);
    }

    #[test]
    fn retransmitted_syn_does_not_duplicate_upstream() {
        let mut flow = Flow::new(key(), 1000, 5000);
        let a = flow.handle_packet(&seg(1000, 0, TcpFlags { syn: true, ..Default::default() }, &[]));
        assert_eq!(a.iter().filter(|x| matches!(x, FlowAction::SendUpstream(_))).count(), 0);
        establish(&mut flow);
        let d = flow.handle_packet(&seg(1001, 5001, TcpFlags { ack: true, psh: true, ..Default::default() }, b"GET"));
        assert_eq!(d.iter().filter(|x| matches!(x, FlowAction::SendUpstream(_))).count(), 1);
        let r = flow.handle_packet(&seg(1001, 5001, TcpFlags { ack: true, ..Default::default() }, b"GET"));
        assert_eq!(r.iter().filter(|x| matches!(x, FlowAction::SendUpstream(_))).count(), 0);
    }

    #[test]
    fn upstream_data_uses_seq_after_syn_and_reversed_addr() {
        let mut flow = Flow::new(key(), 1000, 5000);
        establish(&mut flow);
        let actions = flow.handle_upstream(b"X".to_vec());
        let pkt = match &actions[0] {
            FlowAction::SendToClient(p) => p,
            _ => panic!("expected SendToClient"),
        };
        let (iph, rest) = crate::packet::parse_ipv4(pkt).unwrap();
        assert_eq!(iph.src, [1, 2, 3, 4]);
        assert_eq!(iph.dst, [10, 0, 0, 2]);
        let tcp = crate::packet::parse_tcp(rest).unwrap();
        assert_eq!(tcp.seq, 5001);
        assert_eq!(tcp.ack, 1001);
    }

    #[test]
    fn dup_ack_triggers_retransmit() {
        let mut flow = Flow::new(key(), 1000, 5000);
        establish(&mut flow);
        let a = flow.handle_upstream(b"data".to_vec());
        assert_eq!(a.iter().filter(|x| matches!(x, FlowAction::SendToClient(_))).count(), 1);
        // 重复 ACK（ack 仍是 5001，未确认数据）
        let r = flow.handle_packet(&seg(1001, 5001, TcpFlags { ack: true, ..Default::default() }, &[]));
        let retransmit = r.iter().find(|x| matches!(x, FlowAction::SendToClient(_)));
        assert!(retransmit.is_some(), "expected retransmit on duplicate ack");
        if let Some(FlowAction::SendToClient(p)) = retransmit {
            let (_, rest) = crate::packet::parse_ipv4(p).unwrap();
            let tcp = crate::packet::parse_tcp(rest).unwrap();
            assert_eq!(tcp.seq, 5001, "retransmit must reuse original seq");
        }
    }

    #[test]
    fn ack_discards_retransmit_buffer() {
        let mut flow = Flow::new(key(), 1000, 5000);
        establish(&mut flow);
        flow.handle_upstream(b"data".to_vec());
        // 客户端 ACK 数据（ack=5005，覆盖 seq 5001..5005）
        let r = flow.handle_packet(&seg(1001, 5005, TcpFlags { ack: true, ..Default::default() }, &[]));
        assert_eq!(r.iter().filter(|x| matches!(x, FlowAction::SendToClient(_))).count(), 0,
            "after ack, no retransmit should be pending");
        // 再来一个重复 ACK，不应有重传
        let r2 = flow.handle_packet(&seg(1001, 5005, TcpFlags { ack: true, ..Default::default() }, &[]));
        assert_eq!(r2.iter().filter(|x| matches!(x, FlowAction::SendToClient(_))).count(), 0);
    }

    #[test]
    fn eof_defers_fin_until_acked() {
        let mut flow = Flow::new(key(), 1000, 5000);
        establish(&mut flow);
        flow.handle_upstream(b"data".to_vec());
        // 上游 EOF，数据未确认 -> 不发 FIN（fin_pending）
        let e = flow.handle_upstream_eof();
        assert_eq!(e.iter().filter(|x| matches!(x, FlowAction::Done)).count(), 0, "should not be done yet");
        assert_eq!(flow.state, FlowState::Established);
        // 客户端 ACK 数据 -> 现在发 FIN + Done
        let r = flow.handle_packet(&seg(1001, 5005, TcpFlags { ack: true, ..Default::default() }, &[]));
        assert!(r.iter().any(|x| matches!(x, FlowAction::Done)));
        assert_eq!(flow.state, FlowState::Closing);
    }

    #[test]
    fn eof_sends_fin_when_no_unacked_data() {
        let mut flow = Flow::new(key(), 1000, 5000);
        establish(&mut flow);
        let e = flow.handle_upstream_eof();
        assert!(e.iter().any(|x| matches!(x, FlowAction::SendToClient(_))));
        assert!(e.iter().any(|x| matches!(x, FlowAction::Done)));
        assert_eq!(flow.state, FlowState::Closing);
    }

    #[test]
    fn client_fin_half_closes_upstream() {
        let mut flow = Flow::new(key(), 1000, 5000);
        establish(&mut flow);
        let actions = flow.handle_packet(&seg(1001, 5001, TcpFlags { ack: true, fin: true, ..Default::default() }, &[]));
        assert!(actions.iter().any(|a| matches!(a, FlowAction::CloseUpstream)));
        assert!(!actions.iter().any(|a| matches!(a, FlowAction::Done)), "client FIN should not immediately close");
        assert_eq!(flow.state, FlowState::Established, "still established until upstream EOF");
    }

    #[test]
    fn connect_failed_emits_rst_and_done() {
        let mut flow = Flow::new(key(), 1000, 5000);
        let actions = flow.handle_connect_failed();
        assert!(actions.iter().any(|a| matches!(a, FlowAction::Done)));
        let rst = actions.iter().find(|a| matches!(a, FlowAction::SendToClient(_))).expect("expected RST SendToClient");
        if let FlowAction::SendToClient(p) = rst {
            let (_, rest) = crate::packet::parse_ipv4(p).unwrap();
            let tcp = crate::packet::parse_tcp(rest).unwrap();
            assert!(tcp.flags.rst && tcp.flags.ack);
            assert_eq!(tcp.seq, 5001);
            assert_eq!(tcp.ack, 1001);
        } else {
            panic!("expected SendToClient");
        }
        assert_eq!(flow.state, FlowState::Closed);
    }
}