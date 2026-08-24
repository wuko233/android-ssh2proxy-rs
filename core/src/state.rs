use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Auth {
    Password(String),
    PrivateKey { key: String, passphrase: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxyConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: Auth,
    pub keepalive_interval: Option<Duration>,
    pub keepalive_max: u32,
    pub inactivity_timeout: Option<Duration>,
    pub dns_server: String, // 远程解析器 host，默认 "8.8.8.8"
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 22,
            username: "root".into(),
            auth: Auth::Password(String::new()),
            keepalive_interval: Some(Duration::from_secs(15)),
            keepalive_max: 3,
            inactivity_timeout: Some(Duration::from_secs(30)),
            dns_server: "8.8.8.8".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateEvent {
    StateChanged(ProxyState),
    Error(String),
    Log(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sane_values() {
        let c = ProxyConfig::default();
        assert_eq!(c.port, 22);
        assert_eq!(c.keepalive_max, 3);
        assert_eq!(c.dns_server, "8.8.8.8");
    }

    #[test]
    fn state_initial_is_disconnected() {
        let s = ProxyState::Disconnected;
        assert_eq!(s, ProxyState::Disconnected);
    }
}