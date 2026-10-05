//! Routes Riot Client's chat through Prime so friends see the account as online, on mobile or
//! invisible, the way Deceive (github.com/molenzwiebel/Deceive) does.
//!
//! A launch through Prime passes `--client-config-url` to Riot Client, pointing it at `config`.
//! That proxy fetches Riot's real client config and changes the chat host to
//! `deceive-localhost.molenzwiebel.xyz`, which resolves to 127.0.0.1, so Riot Client connects to
//! `relay`. The relay forwards chat to Riot's server and rewrites the presence Riot Client sends.
//!
//! Trust: Riot Client only connects over TLS to a certificate for that domain. Deceive's author
//! publishes one, private key included, and owns the domain, so this rests on the domain keeping
//! its 127.0.0.1 answer. Prime checks it resolves only to loopback before each launch, binds
//! only to 127.0.0.1, and never sends the certificate anywhere.

mod certificate;
mod config;
mod presence;
mod relay;

use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::AbortHandle;

pub use certificate::ChatProxyError;
pub use config::LOCALHOST_DOMAIN;
pub use presence::PresenceStatus;

/// A running chat proxy. Clones share it; dropping the last one stops it and ends its
/// connections, after which Riot Client's chat stays down until Riot Client restarts.
#[derive(Clone)]
pub struct ChatProxy(Arc<Running>);

struct Running {
    config_port: u16,
    status: watch::Sender<PresenceStatus>,
    connections: Arc<AtomicUsize>,
    tasks: Vec<AbortHandle>,
}

impl Drop for Running {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl ChatProxy {
    pub async fn start(status: PresenceStatus) -> Result<Self, ChatProxyError> {
        certificate::check_localhost_domain().await?;
        let identity = certificate::load_identity(&certificate::default_path()).await?;
        let tls = |error: native_tls::Error| ChatProxyError::Tls(error.to_string());
        let acceptor = tokio_native_tls::TlsAcceptor::from(
            native_tls::TlsAcceptor::new(identity).map_err(tls)?,
        );
        let connector =
            tokio_native_tls::TlsConnector::from(native_tls::TlsConnector::new().map_err(tls)?);
        let http = config::http_client().map_err(ChatProxyError::Tls)?;

        let chat_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let chat_port = chat_listener.local_addr()?.port();
        let config_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let config_port = config_listener.local_addr()?.port();

        let endpoint = config::SharedEndpoint::default();
        let (status_sender, status_receiver) = watch::channel(status);
        let connections = Arc::new(AtomicUsize::new(0));
        let tasks = vec![
            tokio::spawn(config::serve(
                config_listener,
                http,
                chat_port,
                endpoint.clone(),
            ))
            .abort_handle(),
            tokio::spawn(relay::serve(
                chat_listener,
                acceptor,
                connector,
                endpoint,
                status_receiver,
                connections.clone(),
            ))
            .abort_handle(),
        ];

        Ok(Self(Arc::new(Running {
            config_port,
            status: status_sender,
            connections,
            tasks,
        })))
    }

    /// A proxy that listens on nothing, for tests.
    #[cfg(test)]
    pub fn detached(status: PresenceStatus, connected: bool) -> Self {
        Self(Arc::new(Running {
            config_port: 0,
            status: watch::Sender::new(status),
            connections: Arc::new(AtomicUsize::new(usize::from(connected))),
            tasks: Vec::new(),
        }))
    }

    /// The port Riot Client's `--client-config-url` points at.
    pub fn config_port(&self) -> u16 {
        self.0.config_port
    }

    pub fn status(&self) -> PresenceStatus {
        *self.0.status.borrow()
    }

    /// Applies at once: each open connection sends its last presence again with `status`.
    pub fn set_status(&self, status: PresenceStatus) {
        self.0.status.send_replace(status);
    }

    /// Whether Riot Client's chat goes through this proxy now.
    pub fn connected(&self) -> bool {
        self.0.connections.load(Ordering::SeqCst) > 0
    }
}

impl std::fmt::Debug for ChatProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatProxy")
            .field("config_port", &self.config_port())
            .field("status", &self.status())
            .field("connected", &self.connected())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_change_reaches_open_connections() {
        let proxy = ChatProxy::detached(PresenceStatus::Online, true);
        let mut connection = proxy.0.status.subscribe();

        proxy.set_status(PresenceStatus::Mobile);

        assert!(connection.has_changed().expect("proxy running"));
        assert_eq!(*connection.borrow_and_update(), PresenceStatus::Mobile);
        assert_eq!(proxy.status(), PresenceStatus::Mobile);
    }

    #[test]
    fn the_last_handle_dropped_ends_open_connections() {
        let proxy = ChatProxy::detached(PresenceStatus::Online, true);
        let connection = proxy.0.status.subscribe();
        let copy = proxy.clone();

        drop(proxy);
        assert!(connection.has_changed().is_ok(), "a clone keeps it running");
        drop(copy);
        assert!(connection.has_changed().is_err());
    }

    #[test]
    fn connected_follows_open_connections() {
        assert!(ChatProxy::detached(PresenceStatus::Online, true).connected());
        assert!(!ChatProxy::detached(PresenceStatus::Online, false).connected());
    }
}
