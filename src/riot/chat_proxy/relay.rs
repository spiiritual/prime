//! Carries Riot Client's chat to Riot's server over TLS on both sides, rewriting the presence it
//! sends.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use super::config::SharedEndpoint;
use super::presence::{Piece, PresenceStatus, StanzaSplitter, is_own_presence, rewrite_presence};

const READ_BUFFER: usize = 16 * 1024;
/// Windows reports a connection reset before it was accepted as an accept error. The listener
/// still works, so the loop waits briefly and goes on.
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(250);

pub(super) async fn serve(
    listener: TcpListener,
    acceptor: tokio_native_tls::TlsAcceptor,
    connector: tokio_native_tls::TlsConnector,
    endpoint: SharedEndpoint,
    status: watch::Receiver<PresenceStatus>,
    connections: Arc<AtomicUsize>,
) {
    loop {
        let incoming = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(_) => {
                tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
                continue;
            }
        };
        let (acceptor, connector, endpoint, status, connections) = (
            acceptor.clone(),
            connector.clone(),
            endpoint.clone(),
            status.clone(),
            connections.clone(),
        );
        tokio::spawn(async move {
            // Riot Client asks the config proxy where chat is before it connects here.
            let Some(target) = endpoint.get() else {
                return;
            };
            let Ok(client) = acceptor.accept(incoming).await else {
                return;
            };
            let Ok(upstream) = TcpStream::connect((target.host.as_str(), target.port)).await else {
                return;
            };
            let Ok(server) = connector.connect(&target.host, upstream).await else {
                return;
            };
            let _open = OpenConnection::new(connections);
            relay_streams(client, server, status).await;
        });
    }
}

/// Counts a relayed connection for as long as it lives.
struct OpenConnection(Arc<AtomicUsize>);

impl OpenConnection {
    fn new(connections: Arc<AtomicUsize>) -> Self {
        connections.fetch_add(1, Ordering::SeqCst);
        Self(connections)
    }
}

impl Drop for OpenConnection {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Relays until either side closes. Returning drops both streams, which closes the other side
/// too, so Riot Client reconnects instead of waiting on a dead connection.
async fn relay_streams<C, S>(client: C, server: S, status: watch::Receiver<PresenceStatus>)
where
    C: AsyncRead + AsyncWrite + Unpin,
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (client_read, mut client_write) = tokio::io::split(client);
    let (mut server_read, server_write) = tokio::io::split(server);

    tokio::select! {
        () = forward_client(client_read, server_write, status) => {}
        _ = tokio::io::copy(&mut server_read, &mut client_write) => {}
    }
}

/// Forwards what Riot Client sends, with each presence rewritten to the current status. When the
/// status changes, the account's last presence is sent again with the new one, so friends see it
/// without waiting for Riot Client to send another. Ends when Riot Client closes, a write fails,
/// or the proxy stops (the status sender drops).
async fn forward_client<R, W>(
    mut reader: R,
    mut writer: W,
    mut status: watch::Receiver<PresenceStatus>,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut splitter = StanzaSplitter::default();
    let mut own_presence: Option<String> = None;
    let mut buffer = vec![0u8; READ_BUFFER];

    loop {
        tokio::select! {
            read = reader.read(&mut buffer) => {
                let read = match read {
                    Ok(0) | Err(_) => return,
                    Ok(read) => read,
                };
                let Ok(pieces) = splitter.push(&buffer[..read]) else {
                    return;
                };
                for piece in pieces {
                    let bytes = match piece {
                        Piece::Raw(bytes) => bytes,
                        // A party or lobby room presence goes through unchanged.
                        Piece::Presence(stanza) if !is_own_presence(&stanza) => {
                            stanza.into_bytes()
                        }
                        Piece::Presence(stanza) => {
                            let rewritten = rewrite_presence(&stanza, *status.borrow());
                            own_presence = Some(stanza);
                            rewritten.into_bytes()
                        }
                    };
                    if writer.write_all(&bytes).await.is_err() {
                        return;
                    }
                }
                if writer.flush().await.is_err() {
                    return;
                }
            }
            changed = status.changed() => {
                if changed.is_err() {
                    return;
                }
                let current = *status.borrow_and_update();
                // ponytail: a re-sent presence can land in the middle of a non-presence stanza
                // if the status changes between two reads of a split `<message>`/`<iq>`. Riot
                // then closes the connection and Riot Client reconnects with the current status.
                // Upgrade path: element-depth tracking in `StanzaSplitter`.
                if let Some(stanza) = &own_presence {
                    let rewritten = rewrite_presence(stanza, current);
                    if writer.write_all(rewritten.as_bytes()).await.is_err()
                        || writer.flush().await.is_err()
                    {
                        return;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, duplex};

    use super::*;

    async fn read_until(reader: &mut (impl AsyncRead + Unpin), end: &[u8]) -> String {
        let mut out = Vec::new();
        let mut byte = [0u8; 1];
        while !out.ends_with(end) {
            reader.read_exact(&mut byte).await.expect("read");
            out.push(byte[0]);
        }
        String::from_utf8(out).expect("utf-8")
    }

    const PRESENCE: &[u8] = b"<presence><show>chat</show>\
        <games><valorant><st>chat</st><p>e30=</p></valorant></games></presence>";

    #[tokio::test]
    async fn rewrites_presence_and_resends_it_when_the_status_changes() {
        let (mut riot_client, from_client) = duplex(64 * 1024);
        let (to_server, mut chat_server) = duplex(64 * 1024);
        let (status, status_rx) = watch::channel(PresenceStatus::Invisible);
        let forward = tokio::spawn(forward_client(from_client, to_server, status_rx));

        riot_client
            .write_all(b"<message><body>hi</body></message>")
            .await
            .expect("write");
        riot_client.write_all(PRESENCE).await.expect("write");
        let first = read_until(&mut chat_server, b"</presence>").await;
        assert!(
            first.starts_with("<message><body>hi</body></message>"),
            "{first}"
        );
        assert!(first.contains("<show>offline</show>"), "{first}");
        assert!(!first.contains("valorant"), "{first}");

        status.send_replace(PresenceStatus::Online);
        let second = read_until(&mut chat_server, b"</presence>").await;
        assert_eq!(second.as_bytes(), PRESENCE);

        drop(riot_client);
        forward.await.expect("forward ends");
    }

    #[tokio::test]
    async fn room_presences_pass_through_while_own_presence_is_rewritten() {
        const ROOM: &[u8] = b"<presence to='room@x'><show>chat</show>\
            <games><valorant><st>chat</st><p>e30=</p></valorant></games></presence>";
        let (mut riot_client, from_client) = duplex(64 * 1024);
        let (to_server, mut chat_server) = duplex(64 * 1024);
        let (_status, status_rx) = watch::channel(PresenceStatus::Invisible);
        tokio::spawn(forward_client(from_client, to_server, status_rx));

        riot_client.write_all(ROOM).await.expect("write");
        let room = read_until(&mut chat_server, b"</presence>").await;
        assert_eq!(room.as_bytes(), ROOM);

        riot_client.write_all(PRESENCE).await.expect("write");
        let own = read_until(&mut chat_server, b"</presence>").await;
        assert!(own.contains("<show>offline</show>"), "{own}");
        assert!(!own.contains("valorant"), "{own}");
    }

    #[tokio::test]
    async fn sends_nothing_of_a_presence_until_it_is_whole() {
        let (mut riot_client, from_client) = duplex(64 * 1024);
        let (to_server, mut chat_server) = duplex(64 * 1024);
        let (_status, status_rx) = watch::channel(PresenceStatus::Invisible);
        tokio::spawn(forward_client(from_client, to_server, status_rx));

        riot_client.write_all(&PRESENCE[..40]).await.expect("write");
        let mut byte = [0u8; 1];
        let early =
            tokio::time::timeout(Duration::from_millis(100), chat_server.read(&mut byte)).await;
        assert!(early.is_err(), "nothing reaches the server yet");

        riot_client.write_all(&PRESENCE[40..]).await.expect("write");
        let sent = read_until(&mut chat_server, b"</presence>").await;
        assert!(sent.contains("<show>offline</show>"), "{sent}");
    }

    #[tokio::test]
    async fn stopping_the_proxy_ends_the_connection() {
        let (_riot_client, from_client) = duplex(1024);
        let (to_server, _chat_server) = duplex(1024);
        let (status, status_rx) = watch::channel(PresenceStatus::Online);
        let forward = tokio::spawn(forward_client(from_client, to_server, status_rx));

        drop(status);

        forward.await.expect("forward ends");
    }

    #[tokio::test]
    async fn a_closed_server_connection_closes_riot_clients() {
        let (mut riot_client, client_side) = duplex(1024);
        let (server_side, chat_server) = duplex(1024);
        let (_status, status_rx) = watch::channel(PresenceStatus::Online);
        let relay = tokio::spawn(relay_streams(client_side, server_side, status_rx));

        drop(chat_server);
        relay.await.expect("relay ends");

        let mut byte = [0u8; 1];
        assert_eq!(riot_client.read(&mut byte).await.expect("read"), 0);
    }
}
