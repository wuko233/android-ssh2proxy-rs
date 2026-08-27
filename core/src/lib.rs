pub mod dns;
pub mod dataplane;
pub mod packet;
pub mod probe;
pub mod tcpflow;
pub mod socks5;
pub mod ssh;
pub mod state;
pub mod udp;
pub use state::{Auth, ProxyConfig, ProxyState, StateEvent};

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::{mpsc, Notify};

use crate::socks5::start_socks5_server;
use crate::ssh::SshClient;
use crate::udp::UdpRelayManager;

/// Poll interval for the disconnect monitor. Kept short so a dropped session is
/// detected promptly without busy-looping.
const DISCONNECT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Exponential backoff: 1s doubling, capped at 30s.
pub fn backoff_delay(attempt: u32) -> Duration {
    let secs = 1u64.checked_shl(attempt.min(5)).unwrap_or(32).min(30);
    Duration::from_secs(secs)
}

pub struct Proxy {
    config: ProxyConfig,
    ssh: Option<Arc<SshClient>>,
    socks_handle: Option<tokio::task::JoinHandle<()>>,
    udp: Option<Arc<UdpRelayManager>>,
    state: ProxyState,
    tx: mpsc::UnboundedSender<StateEvent>,
    stop: Arc<Notify>,
}

impl Proxy {
    pub fn new(
        config: ProxyConfig,
        tx: mpsc::UnboundedSender<StateEvent>,
        udp: Option<Arc<UdpRelayManager>>,
    ) -> Self {
        Self {
            config,
            ssh: None,
            socks_handle: None,
            udp,
            state: ProxyState::Disconnected,
            tx,
            stop: Arc::new(Notify::new()),
        }
    }

    /// Returns a handle used to stop a running reconnect loop.
    pub fn stop_handle(&self) -> Arc<Notify> {
        self.stop.clone()
    }

    pub async fn connect(&mut self) -> Result<()> {
        self.set_state(ProxyState::Connecting);
        let ssh = match SshClient::connect(&self.config).await {
            Ok(ssh) => Arc::new(ssh),
            Err(e) => {
                self.set_state(ProxyState::Error);
                return Err(e);
            }
        };
        let handle = start_socks5_server(ssh.clone(), "127.0.0.1:1080".parse()?);
        self.ssh = Some(ssh);
        self.socks_handle = Some(handle);
        self.setup_udp().await;
        self.set_state(ProxyState::Connected);
        Ok(())
    }

    pub async fn disconnect(&mut self) {
        self.stop.notify_one();
        self.teardown().await;
        self.set_state(ProxyState::Disconnected);
    }

    /// 在当前 SSH 会话上（重新）建立 UDP 中继；失败则记录并保持 TCP/DNS 可用。
    async fn setup_udp(&self) {
        let Some(mgr) = &self.udp else { return };
        let Some(ssh) = &self.ssh else { return };
        match ssh.open_session_exec(&crate::udp::relay_command()).await {
            Ok(stream) => {
                mgr.replace(stream);
                log::info!("udp relay (re)established");
            }
            Err(e) => log::warn!("udp relay setup failed (UDP disabled): {e}"),
        }
    }

    /// Drops the current SSH session and SOCKS5 server without changing state.
    async fn teardown(&mut self) {
        if let Some(ssh) = self.ssh.take() {
            ssh.disconnect().await;
        }
        if let Some(h) = self.socks_handle.take() {
            h.abort();
        }
    }

    /// Blocking reconnect loop: monitors the live session and, once it drops,
    /// re-establishes it with `backoff_delay` between attempts. Returns when an
    /// explicit `disconnect()` (or `stop_handle().notify_one()`) is issued.
    pub async fn run_reconnect_loop(&mut self) -> Result<()> {
        loop {
            if self.ssh.is_none() && !self.reconnect_with_backoff().await {
                self.disconnect().await;
                return Ok(());
            }

            // Session is up. Watch for a disconnect.
            let ssh = self.ssh.clone().expect("connected session");
            let (drop_tx, mut drop_rx) = mpsc::unbounded_channel();
            tokio::spawn(monitor_disconnect(ssh, drop_tx));

            tokio::select! {
                _ = self.stop.notified() => {
                    self.disconnect().await;
                    return Ok(());
                }
                _ = drop_rx.recv() => {
                    log::warn!("ssh session dropped; reconnecting");
                    self.teardown().await;
                    self.set_state(ProxyState::Reconnecting);
                }
            }
        }
    }

    /// Retries `connect()` forever, backing off between attempts. Returns true
    /// once connected, or false if a stop was requested while retrying.
    async fn reconnect_with_backoff(&mut self) -> bool {
        let mut attempt = 0u32;
        loop {
            match self.connect().await {
                Ok(()) => return true,
                Err(e) => {
                    self.set_state(ProxyState::Reconnecting);
                    log::warn!("ssh connect failed (attempt {attempt}): {e:#}");
                    tokio::select! {
                        _ = self.stop.notified() => return false,
                        _ = tokio::time::sleep(backoff_delay(attempt)) => {}
                    }
                    attempt += 1;
                }
            }
        }
    }

    pub fn state(&self) -> ProxyState {
        self.state
    }

    fn set_state(&mut self, s: ProxyState) {
        self.state = s;
        let _ = self.tx.send(StateEvent::StateChanged(s));
    }
}

/// Polls the session and signals `drop_tx` once it has closed.
async fn monitor_disconnect(ssh: Arc<SshClient>, drop_tx: mpsc::UnboundedSender<()>) {
    loop {
        if ssh.is_closed().await {
            let _ = drop_tx.send(());
            return;
        }
        tokio::time::sleep(DISCONNECT_POLL_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ProxyConfig, ProxyState};

    #[test]
    fn proxy_starts_disconnected() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let p = Proxy::new(ProxyConfig::default(), tx, None);
        assert_eq!(p.state(), ProxyState::Disconnected);
    }

    #[tokio::test]
    async fn disconnect_without_connect_is_safe() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut p = Proxy::new(ProxyConfig::default(), tx, None);
        p.disconnect().await;
        assert_eq!(p.state(), ProxyState::Disconnected);
    }

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(backoff_delay(0).as_secs(), 1);
        assert_eq!(backoff_delay(1).as_secs(), 2);
        assert_eq!(backoff_delay(2).as_secs(), 4);
        assert_eq!(backoff_delay(3).as_secs(), 8);
        assert_eq!(backoff_delay(4).as_secs(), 16);
        assert_eq!(backoff_delay(5).as_secs(), 30);
        assert_eq!(backoff_delay(6).as_secs(), 30);
        assert_eq!(backoff_delay(10).as_secs(), 30);
        assert_eq!(backoff_delay(u32::MAX).as_secs(), 30);
    }
}
