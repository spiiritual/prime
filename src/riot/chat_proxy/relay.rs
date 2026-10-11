//! Carries Riot Client's chat to Riot's server over TLS on both sides, rewriting the presence it
//! sends.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::{Instant, sleep_until};

use super::config::SharedEndpoint;
use super::indicator;
use super::presence::{Piece, PresenceStatus, StanzaSplitter, is_own_presence, rewrite_presence};

const READ_BUFFER: usize = 16 * 1024;
/// Windows reports a connection reset before it was accepted as an accept error. The listener
/// still works, so the loop waits briefly and goes on.
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(250);
/// Riot Client needs a moment with the roster before a message from the added friend shows.
const ANNOUNCE_DELAY: Duration = Duration::from_secs(5);

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
    let (client_read, client_write) = tokio::io::split(client);
    let (server_read, server_write) = tokio::io::split(server);

    tokio::select! {
        () = forward_client(client_read, server_write, status.clone()) => {}
        () = forward_server(server_read, client_write, status) => {}
    }
}

/// Forwards what Riot's server sends, adding Prime's friend to each roster. A moment after the
/// first, and on each status change after, the friend comes online and says how friends see the
/// account; a roster fetched again gets that again too, since it resets the friend's presence.
async fn forward_server<R, W>(
    mut reader: R,
    mut writer: W,
    mut status: watch::Receiver<PresenceStatus>,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut splitter = StanzaSplitter::default();
    let mut buffer = vec![0u8; READ_BUFFER];
    let mut friend_added = false;
    let mut announce_at: Option<Instant> = None;

    loop {
        let bytes = tokio::select! {
            read = reader.read(&mut buffer) => {
                let read = match read {
                    Ok(0) | Err(_) => return,
                    Ok(read) => read,
                };
                let Ok(pieces) = splitter.push(&buffer[..read]) else {
                    return;
                };
                let mut bytes = Vec::with_capacity(read);
                for piece in pieces {
                    match piece {
                        Piece::Raw(raw) => bytes.extend(raw),
                        Piece::Stanza(stanza) => match indicator::add_to_roster(&stanza) {
                            Some(with_friend) => {
                                friend_added = true;
                                announce_at.get_or_insert(Instant::now() + ANNOUNCE_DELAY);
                                bytes.extend(with_friend.into_bytes());
                            }
                            None => bytes.extend(stanza.into_bytes()),
                        },
                    }
                }
                bytes
            }
            () = sleep_until(announce_at.unwrap_or_else(Instant::now)), if announce_at.is_some() => {
                announce_at = None;
                indicator::announcement(*status.borrow()).into_bytes()
            }
            changed = status.changed() => {
                if changed.is_err() {
                    return;
                }
                let current = *status.borrow_and_update();
                if !friend_added || announce_at.is_some() {
                    continue;
                }
                // The splitter only hands out whole stanzas, so this lands between two.
                indicator::announcement(current).into_bytes()
            }
        };
        if bytes.is_empty() {
            continue;
        }
        if writer.write_all(&bytes).await.is_err() || writer.flush().await.is_err() {
            return;
        }
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
                        Piece::Stanza(stanza) if indicator::is_for_friend(&stanza) => continue,
                        // A party or lobby room presence, and anything else, goes through as is.
                        Piece::Stanza(stanza) if !is_own_presence(&stanza) => stanza.into_bytes(),
                        Piece::Stanza(stanza) => {
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
                // The splitter only hands out whole stanzas, so this lands between two.
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

    #[tokio::test(start_paused = true)]
    async fn adds_prime_to_the_friend_list_and_announces_each_status() {
        let (mut chat_server, from_server) = duplex(64 * 1024);
        let (to_client, mut riot_client) = duplex(64 * 1024);
        let (status, status_rx) = watch::channel(PresenceStatus::Invisible);
        tokio::spawn(forward_server(from_server, to_client, status_rx));

        chat_server
            .write_all(b"<iq type='result'><query xmlns='jabber:iq:riotgames:roster'/></iq>")
            .await
            .expect("write");
        let roster = read_until(&mut riot_client, b"</iq>").await;
        assert!(roster.contains("Prime Active!"), "{roster}");

        let first = read_until(&mut riot_client, b"</message>").await;
        assert!(first.contains("you offline (invisible)."), "{first}");

        status.send_replace(PresenceStatus::Mobile);
        let second = read_until(&mut riot_client, b"</message>").await;
        assert!(second.contains("you on mobile."), "{second}");
    }

    #[tokio::test(start_paused = true)]
    async fn the_announcement_goes_out_whole_while_a_stanza_is_half_received() {
        let (mut chat_server, from_server) = duplex(64 * 1024);
        let (to_client, mut riot_client) = duplex(64 * 1024);
        let (_status, status_rx) = watch::channel(PresenceStatus::Online);
        tokio::spawn(forward_server(from_server, to_client, status_rx));

        chat_server
            .write_all(b"<iq type='result'><query xmlns='jabber:iq:riotgames:roster'/></iq>")
            .await
            .expect("write");
        read_until(&mut riot_client, b"</iq>").await;
        chat_server
            .write_all(b"<message><body>half")
            .await
            .expect("write");
        tokio::time::sleep(ANNOUNCE_DELAY * 2).await;
        chat_server
            .write_all(b"</body></message>")
            .await
            .expect("write");

        // The half that arrived was held back, so the announcement doesn't land inside it.
        let announced = read_until(&mut riot_client, b"</message>").await;
        assert!(announced.starts_with("<presence"), "{announced}");
        assert!(announced.contains("you online."), "{announced}");
        let next = read_until(&mut riot_client, b"</message>").await;
        assert_eq!(next, "<message><body>half</body></message>");
    }

    #[tokio::test]
    async fn what_riot_client_sends_the_friend_never_reaches_riot() {
        let (mut riot_client, from_client) = duplex(64 * 1024);
        let (to_server, mut chat_server) = duplex(64 * 1024);
        let (_status, status_rx) = watch::channel(PresenceStatus::Online);
        tokio::spawn(forward_client(from_client, to_server, status_rx));

        riot_client
            .write_all(
                b"<message to='5e0c1b8e-2f7a-4c3d-9a61-7b2d4e8f0a19@eu1.pvp.net'>                  <body>hi</body></message><iq id='2'/>",
            )
            .await
            .expect("write");

        assert_eq!(read_until(&mut chat_server, b"/>").await, "<iq id='2'/>");
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
