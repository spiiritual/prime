//! A local stand-in for Riot's client config service. Riot Client asks it where chat is, and it
//! answers with this PC, so chat goes through Prime's relay.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// Resolves to 127.0.0.1, and Deceive publishes a certificate for it, so Riot Client accepts the
/// relay's TLS. See the trust note in `mod.rs`.
pub const LOCALHOST_DOMAIN: &str = "deceive-localhost.molenzwiebel.xyz";
const CLIENT_CONFIG_BASE_URL: &str = "https://clientconfig.rpg.riotgames.com";
const GEO_PAS_URL: &str = "https://riot-geo.pas.si.riotgames.com/pas/v1/service/chat";
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
/// The most a request line and headers may take; a real one is under 4 KB.
const MAX_REQUEST_HEAD: u64 = 64 * 1024;
/// Windows reports a connection reset before it was accepted as an accept error. The listener
/// still works, so the loop waits briefly and goes on.
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ChatEndpoint {
    pub(super) host: String,
    pub(super) port: u16,
}

/// Where the relay connects for Riot Client. The player's own server, from their affinity, replaces
/// any other; the default server only fills in until it arrives.
#[derive(Clone, Debug, Default)]
pub(super) struct SharedEndpoint(Arc<Mutex<Option<(ChatEndpoint, bool)>>>);

impl SharedEndpoint {
    pub(super) fn get(&self) -> Option<ChatEndpoint> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|(endpoint, _)| endpoint.clone())
    }

    pub(super) fn offer(&self, endpoint: ChatEndpoint, from_affinity: bool) {
        let mut slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if from_affinity
            || slot
                .as_ref()
                .is_none_or(|(_, held_from_affinity)| !held_from_affinity)
        {
            *slot = Some((endpoint, from_affinity));
        }
    }
}

/// Requests carry Riot Client's tokens, so redirects are followed only to Riot's own hosts, and
/// at most 10, like reqwest's default.
pub(super) fn http_client() -> Result<reqwest::Client, String> {
    let redirects = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() < 10 && attempt.url().host_str().is_some_and(is_riot_host) {
            attempt.follow()
        } else {
            attempt.stop()
        }
    });
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(concat!("prime/", env!("CARGO_PKG_VERSION")))
        .redirect(redirects)
        .build()
        .map_err(|error| crate::http_error::format_reqwest_error(&error))
}

fn is_riot_host(host: &str) -> bool {
    host == "riotgames.com" || host.ends_with(".riotgames.com")
}

pub(super) async fn serve(
    listener: TcpListener,
    http: reqwest::Client,
    chat_port: u16,
    endpoint: SharedEndpoint,
) {
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(_) => {
                tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
                continue;
            }
        };
        let http = http.clone();
        let endpoint = endpoint.clone();
        tokio::spawn(async move { handle(stream, &http, chat_port, &endpoint).await });
    }
}

struct ConfigRequest {
    path: String,
    headers: HashMap<String, String>,
}

/// A GET's path and headers, or `None` for anything else, a head that never ends, or one over
/// `MAX_REQUEST_HEAD`.
async fn read_request(reader: impl AsyncBufRead + Unpin) -> Option<ConfigRequest> {
    let mut reader = reader.take(MAX_REQUEST_HEAD);
    let mut line = String::new();
    reader.read_line(&mut line).await.ok()?;

    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next()?, parts.next()?);
    // Riot Client asks for paths only; anything else isn't Riot Client.
    if !method.eq_ignore_ascii_case("GET") || !target.starts_with('/') {
        return None;
    }
    let path = target.to_string();

    let mut headers = HashMap::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await.ok()? == 0 {
            return None;
        }
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    Some(ConfigRequest { path, headers })
}

async fn handle(
    stream: TcpStream,
    http: &reqwest::Client,
    chat_port: u16,
    endpoint: &SharedEndpoint,
) {
    let (reader, mut writer) = stream.into_split();
    let Some(request) = read_request(BufReader::new(reader)).await else {
        respond(&mut writer, 400, "Bad Request", "{}").await;
        return;
    };

    let mut outbound = http.get(format!("{CLIENT_CONFIG_BASE_URL}{}", request.path));
    for (header, name) in [
        ("user-agent", "User-Agent"),
        ("authorization", "Authorization"),
        ("x-riot-entitlements-jwt", "X-Riot-Entitlements-JWT"),
    ] {
        if let Some(value) = request.headers.get(header) {
            outbound = outbound.header(name, value);
        }
    }

    let Ok(response) = outbound.send().await else {
        respond(&mut writer, 502, "Bad Gateway", "{}").await;
        return;
    };
    let status = response.status();
    let Ok(body) = response.text().await else {
        respond(&mut writer, 502, "Bad Gateway", "{}").await;
        return;
    };

    let body = if status.is_success() {
        let affinity = match request.headers.get("authorization") {
            Some(authorization) if body.contains("chat.affinities") => {
                resolve_player_affinity(http, authorization).await
            }
            _ => None,
        };
        let rewritten = rewrite_client_config(&body, chat_port, affinity.as_deref());
        if let Some(chat) = rewritten.chat_endpoint {
            endpoint.offer(chat, rewritten.from_affinity);
        }
        rewritten.body
    } else {
        body
    };

    respond(
        &mut writer,
        status.as_u16(),
        status.canonical_reason().unwrap_or("Unknown"),
        &body,
    )
    .await;
}

async fn respond(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    status: u16,
    reason: &str,
    body: &str,
) {
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = writer.write_all(head.as_bytes()).await;
    let _ = writer.write_all(body.as_bytes()).await;
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RewrittenClientConfig {
    body: String,
    /// The real chat server, for the relay.
    chat_endpoint: Option<ChatEndpoint>,
    /// Whether `chat_endpoint` is the player's own server, from their affinity.
    from_affinity: bool,
}

fn rewrite_client_config(
    body: &str,
    chat_port: u16,
    affinity: Option<&str>,
) -> RewrittenClientConfig {
    let passthrough = || RewrittenClientConfig {
        body: body.to_string(),
        chat_endpoint: None,
        from_affinity: false,
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(body) else {
        return passthrough();
    };
    let Some(object) = value.as_object_mut() else {
        return passthrough();
    };

    let default_host = object
        .get("chat.host")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let port = object
        .get("chat.port")
        .and_then(serde_json::Value::as_u64)
        .and_then(|port| u16::try_from(port).ok());
    let affinities_enabled = object
        .get("chat.affinity.enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let affinity_host = affinity
        .filter(|_| affinities_enabled)
        .and_then(|region| object.get("chat.affinities")?.get(region)?.as_str())
        .filter(|host| !host.trim().is_empty())
        .map(str::to_string);

    if object.contains_key("chat.host") {
        object.insert("chat.host".to_string(), LOCALHOST_DOMAIN.into());
    }
    if object.contains_key("chat.port") {
        object.insert("chat.port".to_string(), chat_port.into());
    }
    if let Some(affinities) = object
        .get_mut("chat.affinities")
        .and_then(serde_json::Value::as_object_mut)
    {
        for host in affinities.values_mut() {
            *host = LOCALHOST_DOMAIN.into();
        }
    }

    let from_affinity = affinity_host.is_some();
    let chat_endpoint = match (affinity_host.or(default_host), port) {
        (Some(host), Some(port)) if !host.trim().is_empty() && port != 0 => {
            Some(ChatEndpoint { host, port })
        }
        _ => None,
    };
    let from_affinity = from_affinity && chat_endpoint.is_some();

    RewrittenClientConfig {
        body: serde_json::to_string(&value).unwrap_or_else(|_| body.to_string()),
        chat_endpoint,
        from_affinity,
    }
}

async fn resolve_player_affinity(http: &reqwest::Client, authorization: &str) -> Option<String> {
    let body = http
        .get(GEO_PAS_URL)
        .header(reqwest::header::AUTHORIZATION, authorization)
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    affinity_from_pas_jwt(body.trim())
}

fn affinity_from_pas_jwt(jwt: &str) -> Option<String> {
    let payload = jwt.trim().split('.').nth(1)?;
    let decoded = BASE64_URL.decode(payload.trim_end_matches('=')).ok()?;
    let payload: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    payload
        .get("affinity")?
        .as_str()
        .map(str::trim)
        .filter(|affinity| !affinity.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAT_PORT: u16 = 54321;

    fn client_config(affinity_enabled: bool) -> String {
        serde_json::json!({
            "chat.host": "chat-eu1.example.net",
            "chat.port": 5223,
            "chat.affinities": {
                "eu": "chat-eu1.example.net",
                "na": "chat-na1.example.net"
            },
            "chat.affinity.enabled": affinity_enabled,
            "unrelated": "kept"
        })
        .to_string()
    }

    fn endpoint(host: &str) -> ChatEndpoint {
        ChatEndpoint {
            host: host.to_string(),
            port: 5223,
        }
    }

    #[test]
    fn rewrites_every_chat_host_to_the_proxy() {
        let rewritten = rewrite_client_config(&client_config(true), CHAT_PORT, Some("na"));

        assert!(rewritten.body.contains(LOCALHOST_DOMAIN));
        assert!(!rewritten.body.contains("chat-eu1.example.net"));
        assert!(!rewritten.body.contains("chat-na1.example.net"));
        assert!(rewritten.body.contains("\"unrelated\":\"kept\""));
        assert!(rewritten.body.contains(&CHAT_PORT.to_string()));
        assert_eq!(
            rewritten.chat_endpoint,
            Some(endpoint("chat-na1.example.net"))
        );
        assert!(rewritten.from_affinity);
    }

    #[test]
    fn falls_back_to_the_default_host_without_an_affinity() {
        let rewritten = rewrite_client_config(&client_config(true), CHAT_PORT, None);

        assert_eq!(
            rewritten.chat_endpoint,
            Some(endpoint("chat-eu1.example.net"))
        );
        assert!(!rewritten.from_affinity);
    }

    #[test]
    fn ignores_affinities_riot_turned_off() {
        let rewritten = rewrite_client_config(&client_config(false), CHAT_PORT, Some("na"));

        assert_eq!(
            rewritten.chat_endpoint,
            Some(endpoint("chat-eu1.example.net"))
        );
        assert!(!rewritten.from_affinity);
    }

    #[test]
    fn an_empty_affinity_host_falls_back_to_the_default() {
        let config = serde_json::json!({
            "chat.host": "chat-eu1.example.net",
            "chat.port": 5223,
            "chat.affinities": { "na": "" }
        })
        .to_string();

        let rewritten = rewrite_client_config(&config, CHAT_PORT, Some("na"));

        assert_eq!(
            rewritten.chat_endpoint,
            Some(endpoint("chat-eu1.example.net"))
        );
        assert!(!rewritten.from_affinity);
    }

    #[test]
    fn only_riot_hosts_get_redirects() {
        assert!(is_riot_host("riotgames.com"));
        assert!(is_riot_host("clientconfig.rpg.riotgames.com"));
        assert!(!is_riot_host("evil-riotgames.com"));
        assert!(!is_riot_host("riotgames.com.evil.net"));
        assert!(!is_riot_host("example.com"));
    }

    #[test]
    fn passes_through_non_json_client_config() {
        let rewritten = rewrite_client_config("internal error", CHAT_PORT, None);

        assert_eq!(rewritten.body, "internal error");
        assert_eq!(rewritten.chat_endpoint, None);
    }

    #[test]
    fn the_players_own_chat_server_wins_over_the_default() {
        let shared = SharedEndpoint::default();

        shared.offer(endpoint("default-1"), false);
        assert_eq!(shared.get(), Some(endpoint("default-1")));

        shared.offer(endpoint("affinity"), true);
        shared.offer(endpoint("default-2"), false);
        assert_eq!(shared.get(), Some(endpoint("affinity")));

        shared.offer(endpoint("affinity-2"), true);
        assert_eq!(shared.get(), Some(endpoint("affinity-2")));
    }

    #[test]
    fn reads_affinity_from_a_pas_jwt() {
        let jwt = "header.eyJhZmZpbml0eSI6ImV1In0.signature";

        assert_eq!(affinity_from_pas_jwt(jwt).as_deref(), Some("eu"));
        assert_eq!(affinity_from_pas_jwt("not-a-jwt"), None);
        assert_eq!(affinity_from_pas_jwt("a.b.c"), None);
    }

    #[tokio::test]
    async fn reads_a_get_request_head() {
        let head = b"GET /api/v1/config/player?os=windows HTTP/1.1\r\n\
            Authorization: Bearer abc\r\nX-Riot-Entitlements-JWT: ent\r\n\r\n";

        let request = read_request(&head[..]).await.expect("request");

        assert_eq!(request.path, "/api/v1/config/player?os=windows");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer abc")
        );
        assert_eq!(
            request
                .headers
                .get("x-riot-entitlements-jwt")
                .map(String::as_str),
            Some("ent")
        );
    }

    #[tokio::test]
    async fn refuses_other_requests() {
        assert!(
            read_request(&b"POST /api HTTP/1.1\r\n\r\n"[..])
                .await
                .is_none()
        );
        assert!(
            read_request(&b"GET http://evil.example/api HTTP/1.1\r\n\r\n"[..])
                .await
                .is_none()
        );
        assert!(
            read_request(&b"GET /api HTTP/1.1\r\nHost: x\r\n"[..])
                .await
                .is_none()
        );

        let mut endless = b"GET /api HTTP/1.1\r\nX-Long: ".to_vec();
        endless.extend(std::iter::repeat_n(b'a', MAX_REQUEST_HEAD as usize));
        assert!(read_request(&endless[..]).await.is_none());
    }
}
