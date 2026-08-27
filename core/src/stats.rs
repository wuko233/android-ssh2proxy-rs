use std::sync::atomic::{AtomicU64, Ordering};

/// 连接期间的流量统计（字节）。
#[derive(Default)]
pub struct TrafficStats {
    /// 上行：App -> 隧道（从 tun 读到的字节）
    up: AtomicU64,
    /// 下行：隧道 -> App（写回 tun 的字节）
    down: AtomicU64,
}

impl TrafficStats {
    pub fn add_up(&self, n: u64) {
        self.up.fetch_add(n, Ordering::Relaxed);
    }

    pub fn add_down(&self, n: u64) {
        self.down.fetch_add(n, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> (u64, u64) {
        (
            self.up.load(Ordering::Relaxed),
            self.down.load(Ordering::Relaxed),
        )
    }
}