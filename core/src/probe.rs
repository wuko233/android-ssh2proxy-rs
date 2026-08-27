use std::time::Duration;

use serde::Serialize;

use crate::dns;
use crate::socks5::Socks5Dialer;

#[derive(Debug, Clone, Serialize)]
pub struct ProbeResult {
    pub domain: String,
    pub socks_tcp_ok: bool,
    pub dns_ok: bool,
    pub resolved_ips: Vec<String>,
    pub tcp_connect_ok: bool,
    pub error: Option<String>,
}

pub fn build_a_query(domain: &str) -> Vec<u8> {
    let mut q = Vec::with_capacity(17 + domain.len());
    q.extend_from_slice(&[0x12, 0x34]); // id
    q.extend_from_slice(&[0x01, 0x00]); // flags: RD
    q.extend_from_slice(&[0x00, 0x01]); // qdcount = 1
    q.extend_from_slice(&[0x00, 0x00]); // ancount
    q.extend_from_slice(&[0x00, 0x00]); // nscount
    q.extend_from_slice(&[0x00, 0x00]); // arcount
    for label in domain.split('.') {
        let b = label.as_bytes();
        q.push(b.len() as u8);
        q.extend_from_slice(b);
    }
    q.push(0); // name terminator
    q.extend_from_slice(&[0x00, 0x01]); // qtype A
    q.extend_from_slice(&[0x00, 0x01]); // qclass IN
    q
}

pub fn parse_a_records(resp: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    if resp.len() < 12 {
        return out;
    }
    let qdcount = u16::from_be_bytes([resp[4], resp[5]]) as usize;
    let ancount = u16::from_be_bytes([resp[6], resp[7]]) as usize;
    let mut pos = 12usize;

    let skip_name = |pos: &mut usize, resp: &[u8]| -> bool {
        loop {
            if *pos >= resp.len() {
                return false;
            }
            let len = resp[*pos];
            if len == 0 {
                *pos += 1;
                return true;
            }
            if len & 0xC0 == 0xC0 {
                *pos += 2;
                return true;
            }
            *pos += 1 + len as usize;
        }
    };

    for _ in 0..qdcount {
        if !skip_name(&mut pos, resp) {
            return out;
        }
        pos += 4; // qtype + qclass
    }
    for _ in 0..ancount {
        if !skip_name(&mut pos, resp) {
            break;
        }
        if pos + 10 > resp.len() {
            break;
        }
        let atype = u16::from_be_bytes([resp[pos], resp[pos + 1]]);
        let rdlength = u16::from_be_bytes([resp[pos + 8], resp[pos + 9]]) as usize;
        let rdata = pos + 10;
        if atype == 1 && rdlength == 4 && rdata + 4 <= resp.len() {
            out.push(format!(
                "{}.{}.{}.{}",
                resp[rdata], resp[rdata + 1], resp[rdata + 2], resp[rdata + 3]
            ));
        }
        pos = rdata + rdlength;
    }
    out
}

pub async fn connectivity_test(dialer: &Socks5Dialer, dns_server: &str, domain: &str) -> ProbeResult {
    let mut result = ProbeResult {
        domain: domain.to_string(),
        socks_tcp_ok: false,
        dns_ok: false,
        resolved_ips: Vec::new(),
        tcp_connect_ok: false,
        error: None,
    };

    // Stage 1: plain TCP connect to the DNS server's port 53 via SOCKS5/SSH.
    // This exercises SSH -> direct-tcpip -> remote egress, without any DNS protocol.
    match tokio::time::timeout(Duration::from_secs(10), dialer.connect(dns_server, 53)).await {
        Ok(Ok(_)) => {
            result.socks_tcp_ok = true;
            log::info!("probe: socks5 tcp connect to {dns_server}:53 ok");
        }
        Ok(Err(e)) => {
            result.error = Some(format!("socks5 connect to {dns_server}:53 failed: {e}"));
            log::warn!("probe: {}", result.error.as_deref().unwrap_or_default());
            return result;
        }
        Err(_) => {
            result.error = Some(format!("socks5 connect to {dns_server}:53 timed out"));
            log::warn!("probe: {}", result.error.as_deref().unwrap_or_default());
            return result;
        }
    }

    // Stage 2: DNS-over-TCP resolution of `domain` through the tunnel.
    let query = build_a_query(domain);
    match tokio::time::timeout(
        Duration::from_secs(10),
        dns::resolve(|h, p| async move { dialer.connect(&h, p).await }, dns_server, &query),
    )
    .await
    {
        Ok(Ok(answer)) => {
            result.dns_ok = !answer.is_empty();
            result.resolved_ips = parse_a_records(&answer);
            log::info!("probe: dns resolve {} -> {:?}", domain, result.resolved_ips);
        }
        Ok(Err(e)) => {
            result.error = Some(format!("dns resolve: {e}"));
            log::warn!("probe: {}", result.error.as_deref().unwrap_or_default());
            return result;
        }
        Err(_) => {
            result.error = Some("dns resolve timed out".to_string());
            log::warn!("probe: {}", result.error.as_deref().unwrap_or_default());
            return result;
        }
    }

    // Stage 3: TCP connect to the first resolved IP on port 443.
    if let Some(ip) = result.resolved_ips.first() {
        match tokio::time::timeout(Duration::from_secs(10), dialer.connect(ip, 443)).await {
            Ok(Ok(_)) => result.tcp_connect_ok = true,
            Ok(Err(e)) => result.error = Some(format!("tcp connect {ip}:443: {e}")),
            Err(_) => result.error = Some(format!("tcp connect {ip}:443 timed out")),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_query_for_domain() {
        let q = build_a_query("example.com");
        // qdcount == 1
        assert_eq!(&q[4..6], &[0, 1]);
        // question: 7 "example" 3 "com" 0
        assert_eq!(&q[12..13], &[7]);
        assert_eq!(&q[13..20], b"example");
        assert_eq!(&q[20..21], &[3]);
        assert_eq!(&q[21..24], b"com");
        assert_eq!(q[24], 0);
    }

    #[test]
    fn parses_single_a_record() {
        // header: qd=1 an=1
        let mut resp = vec![0u8; 12];
        resp[4] = 0;
        resp[5] = 1;
        resp[6] = 0;
        resp[7] = 1;
        // question name "x" (1 'x' 0) + qtype/qclass
        resp.extend_from_slice(&[1, b'x', 0, 0, 1, 0, 1]);
        // answer name pointer (0xc00c) + type A + class IN + ttl + rdlen 4 + ip
        resp.extend_from_slice(&[0xc0, 0x0c, 0x00, 0x01, 0x00, 0x01, 0, 0, 0, 0, 0, 4, 93, 184, 216, 34]);
        let ips = parse_a_records(&resp);
        assert_eq!(ips, vec!["93.184.216.34".to_string()]);
    }
}