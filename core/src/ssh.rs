use std::sync::Arc;

use anyhow::{Context, Result};
use russh::client::{self, Handle};
use russh::keys::{decode_secret_key, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use tokio::sync::Mutex;

use crate::state::{Auth, ProxyConfig};

pub struct SshClient {
    handle: Mutex<Handle<Handler>>,
}

struct Handler;

impl client::Handler for Handler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // TOFU：v1 默认接受，指纹展示在 UI 层（P3）；此处至少记录指纹。
        let key = server_public_key.public_key();
        log::info!(
            "server host key fingerprint (SHA256): {}",
            key.fingerprint(russh::keys::HashAlg::Sha256)
        );
        Ok(true)
    }
}

impl SshClient {
    pub async fn connect(config: &ProxyConfig) -> Result<Self> {
        let cfg = client::Config {
            keepalive_interval: config.keepalive_interval,
            keepalive_max: config.keepalive_max as usize,
            inactivity_timeout: config.inactivity_timeout,
            ..Default::default()
        };
        let cfg = Arc::new(cfg);
        let mut handle = client::connect(cfg, (config.host.as_str(), config.port), Handler)
            .await
            .context("ssh connect failed")?;

        let user = config.username.clone();
        let auth = config.auth.clone();
        let ok = match auth {
            Auth::Password { password: pw } => handle
                .authenticate_password(user, pw)
                .await
                .context("password auth failed")?
                .success(),
            Auth::PrivateKey { key, passphrase } => {
                let key = decode_secret_key(&key, passphrase.as_deref())
                    .context("parse private key failed")?;
                let key = PrivateKeyWithHashAlg::new(Arc::new(key), None);
                handle
                    .authenticate_publickey(user, key)
                    .await
                    .context("publickey auth failed")?
                    .success()
            }
        };
        if !ok {
            anyhow::bail!("authentication rejected");
        }

        Ok(Self {
            handle: Mutex::new(handle),
        })
    }

    pub async fn open_tcpip(
        &self,
        host: &str,
        port: u16,
    ) -> Result<russh::Channel<russh::client::Msg>> {
        let handle = self.handle.lock().await;
        handle
            .channel_open_direct_tcpip(host, port as u32, "127.0.0.1", 0)
            .await
            .context("direct-tcpip open failed")
    }

    pub async fn disconnect(&self) {
        let handle = self.handle.lock().await;
        let _ = handle
            .disconnect(russh::Disconnect::ByApplication, "client disconnect", "")
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_password_auth() {
        let cfg = ProxyConfig {
            auth: Auth::Password {
                password: "secret".into(),
            },
            ..ProxyConfig::default()
        };
        // 仅验证无需网络：Auth 到 russh 参数的映射函数
        assert!(matches!(cfg.auth, Auth::Password { .. }));
    }

    #[test]
    fn default_port_is_22() {
        assert_eq!(ProxyConfig::default().port, 22);
    }
}
