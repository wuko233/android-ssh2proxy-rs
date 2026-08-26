pub mod dns;
pub mod packet;
pub mod socks5;
pub mod ssh;
pub mod state;
pub mod tun;
pub mod tun2socks;
pub use state::{Auth, ProxyConfig, ProxyState, StateEvent};

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;

use crate::socks5::start_socks5_server;
use crate::ssh::SshClient;

pub struct Proxy {
    config: ProxyConfig,
    ssh: Option<Arc<SshClient>>,
    socks_handle: Option<tokio::task::JoinHandle<()>>,
    state: ProxyState,
    tx: mpsc::UnboundedSender<StateEvent>,
}

impl Proxy {
    pub fn new(config: ProxyConfig, tx: mpsc::UnboundedSender<StateEvent>) -> Self {
        Self {
            config,
            ssh: None,
            socks_handle: None,
            state: ProxyState::Disconnected,
            tx,
        }
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
        self.set_state(ProxyState::Connected);
        Ok(())
    }

    pub async fn disconnect(&mut self) {
        if let Some(ssh) = self.ssh.take() {
            ssh.disconnect().await;
        }
        if let Some(h) = self.socks_handle.take() {
            h.abort();
        }
        self.set_state(ProxyState::Disconnected);
    }

    pub fn state(&self) -> ProxyState {
        self.state
    }

    fn set_state(&mut self, s: ProxyState) {
        self.state = s;
        let _ = self.tx.send(StateEvent::StateChanged(s));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ProxyConfig, ProxyState};

    #[test]
    fn proxy_starts_disconnected() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let p = Proxy::new(ProxyConfig::default(), tx);
        assert_eq!(p.state(), ProxyState::Disconnected);
    }

    #[tokio::test]
    async fn disconnect_without_connect_is_safe() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut p = Proxy::new(ProxyConfig::default(), tx);
        p.disconnect().await;
        assert_eq!(p.state(), ProxyState::Disconnected);
    }
}
