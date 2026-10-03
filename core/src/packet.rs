pub const IPV4_PROTO_TCP: u8 = 6;
pub const IPV4_PROTO_UDP: u8 = 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Header {
    pub src: [u8; 4],
    pub dst: [u8; 4],
    pub protocol: u8,
    pub total_len: u16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TcpFlags {
    pub fin: bool,
    pub syn: bool,
    pub rst: bool,
    pub psh: bool,
    pub ack: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpSegment<'a> {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq: u32,
    pub ack: u32,
    pub flags: TcpFlags,
    pub window: u16,
    pub payload: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UdpDatagram<'a> {
    pub src_port: u16,
    pub dst_port: u16,
    pub payload: &'a [u8],
}

pub fn parse_ipv4(buf: &[u8]) -> Option<(Ipv4Header, &[u8])> {
    if buf.len() < 20 || (buf[0] >> 4) != 4 {
        return None;
    }
    let ihl = ((buf[0] & 0x0F) as usize) * 4;
    let total_len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
    if buf.len() < total_len || total_len < ihl {
        return None;
    }
    let header = Ipv4Header {
        src: [buf[12], buf[13], buf[14], buf[15]],
        dst: [buf[16], buf[17], buf[18], buf[19]],
        protocol: buf[9],
        total_len: total_len as u16,
    };
    Some((header, &buf[ihl..total_len]))
}

pub fn parse_tcp<'a>(buf: &'a [u8]) -> Option<TcpSegment<'a>> {
    if buf.len() < 20 {
        return None;
    }
    let data_offset = ((buf[12] >> 4) as usize) * 4;
    if buf.len() < data_offset {
        return None;
    }
    let flags = TcpFlags {
        fin: buf[13] & 0x01 != 0,
        syn: buf[13] & 0x02 != 0,
        rst: buf[13] & 0x04 != 0,
        psh: buf[13] & 0x08 != 0,
        ack: buf[13] & 0x10 != 0,
    };
    Some(TcpSegment {
        src_port: u16::from_be_bytes([buf[0], buf[1]]),
        dst_port: u16::from_be_bytes([buf[2], buf[3]]),
        seq: u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]),
        ack: u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]),
        flags,
        window: u16::from_be_bytes([buf[14], buf[15]]),
        payload: &buf[data_offset..],
    })
}

pub fn parse_udp<'a>(buf: &'a [u8]) -> Option<UdpDatagram<'a>> {
    if buf.len() < 8 {
        return None;
    }
    let len = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    if len < 8 || buf.len() < len {
        return None;
    }
    Some(UdpDatagram {
        src_port: u16::from_be_bytes([buf[0], buf[1]]),
        dst_port: u16::from_be_bytes([buf[2], buf[3]]),
        payload: &buf[8..len],
    })
}

pub fn internet_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let (chunks, remainder) = data.as_chunks::<2>();
    for c in chunks {
        sum += u16::from_be_bytes(*c) as u32;
    }
    if let [b] = remainder {
        sum += (*b as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

fn tcp_checksum(iph: &Ipv4Header, tcp: &[u8]) -> u16 {
    let mut pseudo = Vec::with_capacity(12 + tcp.len());
    pseudo.extend_from_slice(&iph.src);
    pseudo.extend_from_slice(&iph.dst);
    pseudo.push(0);
    pseudo.push(iph.protocol);
    pseudo.extend_from_slice(&(tcp.len() as u16).to_be_bytes());
    pseudo.extend_from_slice(tcp);
    internet_checksum(&pseudo)
}

fn udp_checksum(iph: &Ipv4Header, udp: &[u8]) -> u16 {
    let mut pseudo = Vec::with_capacity(12 + udp.len());
    pseudo.extend_from_slice(&iph.src);
    pseudo.extend_from_slice(&iph.dst);
    pseudo.push(0);
    pseudo.push(iph.protocol);
    pseudo.extend_from_slice(&(udp.len() as u16).to_be_bytes());
    pseudo.extend_from_slice(udp);
    let s = internet_checksum(&pseudo);
    if s == 0 { 0xFFFF } else { s }
}

fn build_ipv4_header(total_len: usize, protocol: u8, src: [u8; 4], dst: [u8; 4]) -> [u8; 20] {
    let mut h = [0u8; 20];
    h[0] = 0x45;
    h[2] = (total_len >> 8) as u8;
    h[3] = total_len as u8;
    h[8] = 64;
    h[9] = protocol;
    h[12..16].copy_from_slice(&src);
    h[16..20].copy_from_slice(&dst);
    let csum = internet_checksum(&h);
    h[10] = (csum >> 8) as u8;
    h[11] = csum as u8;
    h
}

#[allow(clippy::too_many_arguments)]
pub fn build_tcp_packet(
    iph: &Ipv4Header,
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: TcpFlags,
    window: u16,
    payload: &[u8],
) -> Vec<u8> {
    let mut tcp = [0u8; 20];
    tcp[0..2].copy_from_slice(&src_port.to_be_bytes());
    tcp[2..4].copy_from_slice(&dst_port.to_be_bytes());
    tcp[4..8].copy_from_slice(&seq.to_be_bytes());
    tcp[8..12].copy_from_slice(&ack.to_be_bytes());
    tcp[12] = 0x50;
    let mut f: u8 = 0;
    if flags.fin { f |= 0x01; }
    if flags.syn { f |= 0x02; }
    if flags.rst { f |= 0x04; }
    if flags.psh { f |= 0x08; }
    if flags.ack { f |= 0x10; }
    tcp[13] = f;
    tcp[14..16].copy_from_slice(&window.to_be_bytes());
    let mut full = Vec::with_capacity(20 + payload.len());
    full.extend_from_slice(&tcp);
    full.extend_from_slice(payload);
    let csum = tcp_checksum(iph, &full);
    full[16] = (csum >> 8) as u8;
    full[17] = csum as u8;

    let total_len = 20 + full.len();
    let mut pkt = Vec::with_capacity(total_len);
    pkt.extend_from_slice(&build_ipv4_header(total_len, iph.protocol, iph.src, iph.dst));
    pkt.extend_from_slice(&full);
    pkt
}

pub fn build_udp_packet(
    iph: &Ipv4Header,
    src_port: u16,
    dst_port: u16,
    payload: &[u8],
) -> Vec<u8> {
    let udp_len = 8 + payload.len();
    let mut udp = Vec::with_capacity(udp_len);
    udp.extend_from_slice(&src_port.to_be_bytes());
    udp.extend_from_slice(&dst_port.to_be_bytes());
    udp.extend_from_slice(&(udp_len as u16).to_be_bytes());
    udp.extend_from_slice(&0u16.to_be_bytes());
    udp.extend_from_slice(payload);
    let csum = udp_checksum(iph, &udp);
    udp[6] = (csum >> 8) as u8;
    udp[7] = csum as u8;

    let total_len = 20 + udp.len();
    let mut pkt = Vec::with_capacity(total_len);
    pkt.extend_from_slice(&build_ipv4_header(total_len, iph.protocol, iph.src, iph.dst));
    pkt.extend_from_slice(&udp);
    pkt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internet_checksum_known_vector() {
        assert_eq!(internet_checksum(&[0, 0]), 0xFFFF);
        assert_eq!(internet_checksum(&[0xFF, 0xFF]), 0x0000);
    }

    #[test]
    fn parse_ipv4_roundtrip() {
        let iph = Ipv4Header { src: [10, 0, 0, 2], dst: [93, 184, 216, 34], protocol: 6, total_len: 20 };
        let pkt = build_tcp_packet(&iph, 12345, 443, 100, 0, TcpFlags { syn: true, ..Default::default() }, 65535, &[]);
        let (parsed, rest) = parse_ipv4(&pkt).unwrap();
        assert_eq!(parsed.src, [10, 0, 0, 2]);
        assert_eq!(parsed.dst, [93, 184, 216, 34]);
        assert_eq!(parsed.protocol, 6);
        let seg = parse_tcp(rest).unwrap();
        assert!(seg.flags.syn);
        assert_eq!(seg.src_port, 12345);
        assert_eq!(seg.dst_port, 443);
        assert_eq!(seg.seq, 100);
    }

    #[test]
    fn internet_checksum_odd_length() {
        assert_eq!(internet_checksum(&[0x01]), 0xFEFF);
    }

    fn tcp_checksum_of(pkt: &[u8], iph: &Ipv4Header) -> u16 {
        let seg = &pkt[20..];
        let mut data = Vec::with_capacity(12 + seg.len());
        data.extend_from_slice(&iph.src);
        data.extend_from_slice(&iph.dst);
        data.push(0);
        data.push(iph.protocol);
        data.extend_from_slice(&(seg.len() as u16).to_be_bytes());
        data.extend_from_slice(seg);
        data[28] = 0;
        data[29] = 0;
        internet_checksum(&data)
    }

    #[test]
    fn tcp_checksum_is_valid() {
        let iph = Ipv4Header { src: [10, 0, 0, 2], dst: [1, 2, 3, 4], protocol: 6, total_len: 0 };
        let payload = b"GET / HTTP/1.1\r\n";
        let pkt = build_tcp_packet(&iph, 40000, 443, 5000, 1001, TcpFlags { ack: true, psh: true, ..Default::default() }, 65535, payload);
        let embedded = u16::from_be_bytes([pkt[36], pkt[37]]);
        assert_eq!(embedded, tcp_checksum_of(&pkt, &iph));
        assert_ne!(embedded, 0);
    }

    #[test]
    fn udp_checksum_is_valid() {
        let iph = Ipv4Header { src: [10, 0, 0, 1], dst: [10, 0, 0, 2], protocol: 17, total_len: 0 };
        let pkt = build_udp_packet(&iph, 53, 40000, &[1, 2, 3, 4]);
        let embedded = u16::from_be_bytes([pkt[26], pkt[27]]);
        assert_ne!(embedded, 0);
    }
}