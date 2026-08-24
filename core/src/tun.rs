use std::io::{Read, Write};
use std::marker::PhantomData;

use smoltcp::phy::{Device, DeviceCapabilities, Medium};
use smoltcp::time::Instant;

/// 把任意可读写字节流的 fd（Android 上为 VpnService 提供的 tun fd）
/// 包装成 smoltcp 的物理设备。TUN 是 L3 设备，读写裸 IP 包。
pub struct TunDevice<R> {
    inner: R,
    mtu: usize,
}

impl<R> TunDevice<R> {
    pub fn new(inner: R) -> Self {
        Self { inner, mtu: 1500 }
    }
    pub fn with_mtu(inner: R, mtu: usize) -> Self {
        Self { inner, mtu }
    }
}

pub struct RxToken<'a, R> {
    buf: [u8; 65536],
    len: usize,
    _marker: PhantomData<&'a mut R>,
}

pub struct TxToken<'a, R> {
    dev: &'a mut R,
}

impl<R: Read + Write> Device for TunDevice<R> {
    type RxToken<'a> = RxToken<'a, R> where R: 'a;
    type TxToken<'a> = TxToken<'a, R> where R: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let mut buf = [0u8; 65536];
        match self.inner.read(&mut buf) {
            Ok(0) | Err(_) => None,
            Ok(len) => Some((
                RxToken { buf, len, _marker: PhantomData },
                TxToken { dev: &mut self.inner },
            )),
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(TxToken { dev: &mut self.inner })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ip;
        caps.max_transmission_unit = self.mtu;
        caps
    }
}

impl<R> smoltcp::phy::RxToken for RxToken<'_, R> {
    fn consume<R2, F>(self, f: F) -> R2
    where
        F: FnOnce(&[u8]) -> R2,
    {
        f(&self.buf[..self.len])
    }
}

impl<R: Write> smoltcp::phy::TxToken for TxToken<'_, R> {
    fn consume<R2, F>(self, len: usize, f: F) -> R2
    where
        F: FnOnce(&mut [u8]) -> R2,
    {
        let mut buf = [0u8; 65536];
        let r = f(&mut buf[..len]);
        let _ = self.dev.write(&buf[..len]);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RingBuf {
        data: Vec<u8>,
        pos: usize,
    }
    impl RingBuf {
        fn new(cap: usize) -> Self {
            Self { data: vec![0; cap], pos: 0 }
        }
    }
    impl std::io::Read for RingBuf {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len().min(self.pos);
            buf[..n].copy_from_slice(&self.data[..n]);
            self.data.copy_within(n..self.pos, 0);
            self.pos -= n;
            Ok(n)
        }
    }
    impl std::io::Write for RingBuf {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let n = buf.len().min(self.data.len() - self.pos);
            self.data[self.pos..self.pos + n].copy_from_slice(&buf[..n]);
            self.pos += n;
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn capabilities_are_ipv4_mtu_1500() {
        let dev = TunDevice::new(RingBuf::new(2048));
        let caps = dev.capabilities();
        assert_eq!(caps.max_transmission_unit, 1500);
        assert_eq!(caps.medium, Medium::Ip);
    }
}