use std::collections::BTreeMap;

use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::time::Instant;
use smoltcp::wire::{IpCidr, Ipv4Address, Ipv4Cidr};

use crate::tun::TunDevice;

pub struct TunConfig {
    pub address: Ipv4Address,
    pub netmask: Ipv4Cidr,
    pub dns: Ipv4Address,
    pub mtu: usize,
}

impl Default for TunConfig {
    fn default() -> Self {
        Self {
            address: Ipv4Address::new(10, 0, 0, 2),
            netmask: Ipv4Cidr::new(Ipv4Address::new(10, 0, 0, 0), 24),
            dns: Ipv4Address::new(10, 0, 0, 1),
            mtu: 1500,
        }
    }
}

pub struct Tun2Socks<R> {
    device: TunDevice<R>,
    iface: Interface,
    sockets: SocketSet<'static>,
    #[allow(dead_code)]
    tcp_handles: BTreeMap<SocketHandle, tokio::task::JoinHandle<()>>,
}

impl<R: std::io::Read + std::io::Write> Tun2Socks<R> {
    pub fn new(mut device: TunDevice<R>, cfg: TunConfig) -> Self {
        let config = Config::new(smoltcp::wire::HardwareAddress::Ip);
        let mut iface = Interface::new(config, &mut device, Instant::ZERO);
        iface.update_ip_addrs(|addrs| {
            let _ = addrs.push(IpCidr::Ipv4(Ipv4Cidr::new(
                cfg.address,
                cfg.netmask.prefix_len(),
            )));
        });
        iface
            .routes_mut()
            .add_default_ipv4_route(Ipv4Address::UNSPECIFIED)
            .unwrap();
        Self {
            device,
            iface,
            sockets: SocketSet::new(vec![]),
            tcp_handles: BTreeMap::new(),
        }
    }

    pub fn poll(&mut self, now: Instant) {
        let device = &mut self.device;
        self.iface.poll(now, device, &mut self.sockets);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ip_is_10_0_0_2() {
        let cfg = TunConfig::default();
        assert_eq!(cfg.address, Ipv4Address::new(10, 0, 0, 2));
        assert_eq!(cfg.dns, Ipv4Address::new(10, 0, 0, 1));
    }
}
