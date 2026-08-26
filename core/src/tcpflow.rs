use crate::packet::{TcpFlags, TcpSegment};

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

pub struct Flow {
    pub key: FlowKey,
    pub state: FlowState,
    client_isn: u32,
    our_isn: u32,
    our_seq: u32,
    our_acked: u32,
    next_client_seq: u32,
    window: u16,
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
                } else if seg.flags.syn {
                    out.push(FlowAction::SendToClient(self.synack()));
                }
            }
            FlowState::Established => {
                self.window = seg.window;
                self.our_acked = self.our_acked.max(seg.ack);
                let mut should_ack = false;
                if !seg.payload.is_empty() {
                    should_ack = true;
                    // 重传幂等：只转发新数据
                    if seg.seq == self.next_client_seq {
                        self.next_client_seq = self.next_client_seq.wrapping_add(seg.payload.len() as u32);
                        out.push(FlowAction::SendUpstream(seg.payload.to_vec()));
                    }
                }
                if seg.flags.fin {
                    should_ack = true;
                    // FIN 消耗 1 个序号
                    self.next_client_seq = self.next_client_seq.wrapping_add(1);
                    self.state = FlowState::Closing;
                    out.push(FlowAction::SendToClient(self.take_fin()));
                    out.push(FlowAction::CloseUpstream);
                }
                if should_ack {
                    out.push(FlowAction::SendToClient(self.ack_only()));
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
        let n = data.len() as u32;
        let pkt = self.data_packet(&data);
        self.our_seq = self.our_seq.wrapping_add(n);
        vec![FlowAction::SendToClient(pkt)]
    }

    pub fn handle_upstream_eof(&mut self) -> Vec<FlowAction> {
        match self.state {
            FlowState::Established => {
                self.state = FlowState::Closing;
                vec![FlowAction::SendToClient(self.take_fin()), FlowAction::Done]
            }
            _ => vec![FlowAction::Done],
        }
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

    fn data_packet(&self, data: &[u8]) -> Vec<u8> {
        let iph = self.iph();
        crate::packet::build_tcp_packet(
            &iph,
            self.key.dst_port,
            self.key.src_port,
            self.our_seq,
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

    fn seg(seq: u32, ack: u32, flags: TcpFlags, payload: &[u8]) -> TcpSegment {
        TcpSegment { src_port: 40000, dst_port: 443, seq, ack, flags, window: 65535, payload }
    }

    #[test]
    fn syn_generates_synack() {
        let key = FlowKey { src_ip: [10,0,0,2], src_port: 40000, dst_ip: [1,2,3,4], dst_port: 443 };
        let mut flow = Flow::new(key, 1000, 5000);
        let actions = flow.handle_packet(&seg(1000, 0, TcpFlags { syn: true, ..Default::default() }, &[]));
        assert!(actions.iter().any(|a| matches!(a, FlowAction::SendToClient(_))));
        assert_eq!(flow.state, FlowState::SynAckSent);
    }

    #[test]
    fn retransmitted_syn_does_not_duplicate_upstream() {
        let key = FlowKey { src_ip: [10,0,0,2], src_port: 40000, dst_ip: [1,2,3,4], dst_port: 443 };
        let mut flow = Flow::new(key, 1000, 5000);
        let a = flow.handle_packet(&seg(1000, 0, TcpFlags { syn: true, ..Default::default() }, &[]));
        let upstream = a.iter().filter(|x| matches!(x, FlowAction::SendUpstream(_))).count();
        assert_eq!(upstream, 0);
        // established
        let ack = TcpFlags { ack: true, ..Default::default() };
        flow.handle_packet(&seg(1001, 5001, ack, &[]));
        assert_eq!(flow.state, FlowState::Established);
        // client data
        let d = flow.handle_packet(&seg(1001, 5001, TcpFlags { ack: true, psh: true, ..Default::default() }, b"GET"));
        assert_eq!(d.iter().filter(|x| matches!(x, FlowAction::SendUpstream(_))).count(), 1);
        // retransmit same seq -> no new upstream
        let r = flow.handle_packet(&seg(1001, 5001, TcpFlags { ack: true, ..Default::default() }, b"GET"));
        assert_eq!(r.iter().filter(|x| matches!(x, FlowAction::SendUpstream(_))).count(), 0);
    }

    #[test]
    fn upstream_data_sends_to_client_and_fin_closes() {
        let key = FlowKey { src_ip: [10,0,0,2], src_port: 40000, dst_ip: [1,2,3,4], dst_port: 443 };
        let mut flow = Flow::new(key, 1000, 5000);
        flow.handle_packet(&seg(1000, 0, TcpFlags { syn: true, ..Default::default() }, &[]));
        flow.handle_packet(&seg(1001, 5001, TcpFlags { ack: true, ..Default::default() }, &[]));
        let a = flow.handle_upstream(b"HTTP".to_vec());
        assert!(a.iter().any(|x| matches!(x, FlowAction::SendToClient(_))));
        let e = flow.handle_upstream_eof();
        assert!(e.iter().any(|x| matches!(x, FlowAction::SendToClient(_))));
        assert_eq!(flow.state, FlowState::Closing);
    }

    #[test]
    fn upstream_data_uses_seq_after_syn_and_reversed_addr() {
        let key = FlowKey { src_ip: [10,0,0,2], src_port: 40000, dst_ip: [1,2,3,4], dst_port: 443 };
        let mut flow = Flow::new(key, 1000, 5000);
        flow.handle_packet(&seg(1000, 0, TcpFlags { syn: true, ..Default::default() }, &[]));
        flow.handle_packet(&seg(1001, 5001, TcpFlags { ack: true, ..Default::default() }, &[]));
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
    fn client_fin_triggers_our_fin() {
        let key = FlowKey { src_ip: [10,0,0,2], src_port: 40000, dst_ip: [1,2,3,4], dst_port: 443 };
        let mut flow = Flow::new(key, 1000, 5000);
        flow.handle_packet(&seg(1000, 0, TcpFlags { syn: true, ..Default::default() }, &[]));
        flow.handle_packet(&seg(1001, 5001, TcpFlags { ack: true, ..Default::default() }, &[]));
        let actions = flow.handle_packet(&seg(1001, 5001, TcpFlags { ack: true, fin: true, ..Default::default() }, &[]));
        let fins = actions.iter().filter(|a| {
            if let FlowAction::SendToClient(p) = a {
                let (_, rest) = crate::packet::parse_ipv4(p).unwrap();
                crate::packet::parse_tcp(rest).unwrap().flags.fin
            } else {
                false
            }
        }).count();
        assert_eq!(fins, 1);
        assert!(actions.iter().any(|a| matches!(a, FlowAction::CloseUpstream)));
        assert_eq!(flow.state, FlowState::Closing);
    }
}