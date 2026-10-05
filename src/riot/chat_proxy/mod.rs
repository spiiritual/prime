use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use x509_parser::prelude::{FromDer, X509Certificate};

mod presence;

pub use presence::PresenceStatus;
use presence::rewrite_presence_content;

pub const LOCALHOST_DOMAIN: &str = "deceive-localhost.molenzwiebel.xyz";
pub const PROXY_CERT_URL: &str = "https://mln.cx/deceive/localhost.pfx";
pub const CLIENT_CONFIG_BASE_URL: &str = "https://clientconfig.rpg.riotgames.com";
pub const GEO_PAS_URL: &str = "https://riot-geo.pas.si.riotgames.com/pas/v1/service/chat";
pub const CERT_MIN_VALID_DAYS: i64 = 20;

const CERT_HTTP_TIMEOUT: StdDuration = StdDuration::from_secs(20);
const CONFIG_BODY_LIMIT: usize = 512 * 1024;
const CHAT_PUMP_BUFFER: usize = 16 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatEndpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RewrittenClientConfig {
    pub body: String,
    pub chat_endpoint: Option<ChatEndpoint>,
}

pub fn client_config_url_arg(port: u16) -> String {
    format!("--client-config-url=\"http://127.0.0.1:{port}\"")
}

pub fn rewrite_client_config(
    body: &str,
    chat_port: u16,
    affinity: Option<&str>,
) -> RewrittenClientConfig {
    let passthrough = || RewrittenClientConfig {
        body: body.to_string(),
        chat_endpoint: None,
    };
    let mut value: serde_json::Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => return passthrough(),
    };
    let Some(object) = value.as_object_mut() else {
        return passthrough();
    };

    let original_host = object
        .get("chat.host")
        .and_then(|host| host.as_str())
        .map(str::to_string);
    let original_port = object
        .get("chat.port")
        .and_then(|port| port.as_u64())
        .and_then(|port| u16::try_from(port).ok());
    let mut original_affinities = HashMap::new();
    if let Some(affinities) = object
        .get("chat.affinities")
        .and_then(|value| value.as_object())
    {
        for (region, host) in affinities {
            if let Some(host) = host.as_str() {
                original_affinities.insert(region.clone(), host.to_string());
            }
        }
    }

    if object.contains_key("chat.host") {
        object.insert(
            "chat.host".to_string(),
            serde_json::Value::String(LOCALHOST_DOMAIN.to_string()),
        );
    }
    if object.contains_key("chat.port") {
        object.insert(
            "chat.port".to_string(),
            serde_json::Value::Number(chat_port.into()),
        );
    }
    if let Some(affinities) = object
        .get_mut("chat.affinities")
        .and_then(|value| value.as_object_mut())
    {
        for (_, host) in affinities.iter_mut() {
            *host = serde_json::Value::String(LOCALHOST_DOMAIN.to_string());
        }
    }

    let host = affinity
        .and_then(|region| original_affinities.get(region))
        .cloned()
        .or(original_host);
    let chat_endpoint = match (host, original_port) {
        (Some(host), Some(port)) if !host.trim().is_empty() && port != 0 => {
            Some(ChatEndpoint { host, port })
        }
        _ => None,
    };

    RewrittenClientConfig {
        body: serde_json::to_string(&value).unwrap_or_else(|_| body.to_string()),
        chat_endpoint,
    }
}

pub fn affinity_from_pas_jwt(jwt: &str) -> Option<String> {
    let payload = jwt.trim().split('.').nth(1)?;
    let decoded = BASE64_URL.decode(payload.trim_end_matches('=')).ok()?;
    let payload: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    payload
        .get("affinity")
        .and_then(|affinity| affinity.as_str())
        .map(str::trim)
        .filter(|affinity| !affinity.is_empty())
        .map(str::to_string)
}

pub async fn resolve_player_affinity(authorization: &str) -> Option<String> {
    let client = proxy_http_client().ok()?;
    let body = client
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

pub fn addresses_resolve_to_loopback(addresses: &[SocketAddr]) -> bool {
    addresses
        .iter()
        .any(|address| address.ip() == IpAddr::V4(Ipv4Addr::LOCALHOST))
}

pub async fn ensure_localhost_resolution() -> Result<(), ChatProxyError> {
    let addresses = tokio::net::lookup_host((LOCALHOST_DOMAIN, 443))
        .await
        .map_err(|error| ChatProxyError::Dns(error.to_string()))?;
    let addresses: Vec<SocketAddr> = addresses.collect();

    if addresses_resolve_to_loopback(&addresses) {
        Ok(())
    } else {
        Err(ChatProxyError::LocalhostUnresolvable)
    }
}

pub fn default_proxy_cert_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|dir| PathBuf::from(dir).join(r"spiiritual\prime\config\localhostCert.pfx"))
        .unwrap_or_else(|| PathBuf::from("localhostCert.pfx"))
}

pub fn certificate_is_fresh(pfx_bytes: &[u8]) -> bool {
    pfx_certificate_not_after(pfx_bytes).is_some_and(|not_after| {
        not_after > time::OffsetDateTime::now_utc() + time::Duration::days(CERT_MIN_VALID_DAYS)
    })
}

fn pfx_certificate_not_after(pfx_bytes: &[u8]) -> Option<time::OffsetDateTime> {
    let pfx = p12::PFX::parse(pfx_bytes).ok()?;
    let certs = pfx.cert_x509_bags("").ok()?;
    let der = certs.first()?;
    let (_, certificate) = X509Certificate::from_der(der).ok()?;
    Some(certificate.validity().not_after.to_datetime())
}

pub async fn ensure_proxy_certificate(cache_path: &Path) -> Result<Vec<u8>, ChatProxyError> {
    if let Ok(cached) = std::fs::read(cache_path)
        && certificate_is_fresh(&cached)
    {
        return Ok(cached);
    }

    let client = proxy_http_client()?;
    let bytes = client
        .get(PROXY_CERT_URL)
        .send()
        .await
        .map_err(|error| {
            ChatProxyError::CertDownload(crate::http_error::format_reqwest_error(&error))
        })?
        .error_for_status()
        .map_err(|error| {
            ChatProxyError::CertDownload(crate::http_error::format_reqwest_error(&error))
        })?
        .bytes()
        .await
        .map_err(|error| {
            ChatProxyError::CertDownload(crate::http_error::format_reqwest_error(&error))
        })?
        .to_vec();

    if !certificate_is_fresh(&bytes) {
        return Err(ChatProxyError::CertInvalid(
            "the downloaded chat proxy certificate is missing or expires too soon".to_string(),
        ));
    }

    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::image_cache::write_cache_file(cache_path, &bytes)?;

    Ok(bytes)
}

fn proxy_http_client() -> Result<reqwest::Client, ChatProxyError> {
    reqwest::Client::builder()
        .timeout(CERT_HTTP_TIMEOUT)
        .user_agent(concat!("prime/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| ChatProxyError::Http(crate::http_error::format_reqwest_error(&error)))
}

#[derive(Clone, Debug, Default)]
pub struct ConfigProxyEndpoint {
    slot: Arc<Mutex<Option<ChatEndpoint>>>,
}

impl ConfigProxyEndpoint {
    pub fn get(&self) -> Option<ChatEndpoint> {
        self.slot.lock().ok().and_then(|slot| slot.clone())
    }

    fn set_once(&self, endpoint: ChatEndpoint) {
        if let Ok(mut slot) = self.slot.lock()
            && slot.is_none()
        {
            *slot = Some(endpoint);
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChatProxyControl {
    status: Arc<Mutex<PresenceStatus>>,
    enabled: Arc<Mutex<bool>>,
}

impl ChatProxyControl {
    pub fn new(status: PresenceStatus) -> Self {
        Self {
            status: Arc::new(Mutex::new(status)),
            enabled: Arc::new(Mutex::new(true)),
        }
    }

    pub fn current(&self) -> (bool, PresenceStatus) {
        let enabled = self.enabled.lock().ok().map(|flag| *flag).unwrap_or(true);
        let status = self
            .status
            .lock()
            .ok()
            .map(|status| *status)
            .unwrap_or_default();
        (enabled, status)
    }

    pub fn set_status(&self, status: PresenceStatus) {
        if let Ok(mut current) = self.status.lock() {
            *current = status;
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        if let Ok(mut current) = self.enabled.lock() {
            *current = enabled;
        }
    }
}

pub struct RunningConfigProxy {
    pub port: u16,
    pub endpoint: ConfigProxyEndpoint,
}

pub async fn bind_loopback_listener() -> Result<(TcpListener, u16), ChatProxyError> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    Ok((listener, port))
}

pub async fn start_config_proxy(chat_port: u16) -> Result<RunningConfigProxy, ChatProxyError> {
    let (listener, port) = bind_loopback_listener().await?;
    let endpoint = ConfigProxyEndpoint::default();
    let accept_endpoint = endpoint.clone();

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let connection_endpoint = accept_endpoint.clone();

            tokio::spawn(async move {
                handle_config_connection(stream, chat_port, connection_endpoint).await;
            });
        }
    });

    Ok(RunningConfigProxy { port, endpoint })
}

async fn handle_config_connection(
    stream: TcpStream,
    chat_port: u16,
    endpoint: ConfigProxyEndpoint,
) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).await.unwrap_or(0) == 0 {
        return;
    }

    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return;
    };
    if !method.eq_ignore_ascii_case("get") {
        return;
    }

    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        let Ok(read) = reader.read_line(&mut line).await else {
            return;
        };
        if read == 0 || line.trim().is_empty() {
            break;
        }
        if headers.len() > 128 || line.len() > 8192 {
            return;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    let mut path = target
        .strip_prefix(CLIENT_CONFIG_BASE_URL)
        .unwrap_or(target);
    if let Some(after_scheme) = path.split("://").nth(1) {
        path = after_scheme
            .find('/')
            .map(|index| &after_scheme[index..])
            .unwrap_or("/");
    }

    let client = match proxy_http_client() {
        Ok(client) => client,
        Err(_) => return,
    };
    let mut outbound = client.get(format!("{CLIENT_CONFIG_BASE_URL}{path}"));
    if let Some(user_agent) = headers.get("user-agent") {
        outbound = outbound.header(reqwest::header::USER_AGENT, user_agent.clone());
    }
    if let Some(authorization) = headers.get("authorization") {
        outbound = outbound.header(reqwest::header::AUTHORIZATION, authorization.clone());
    }
    if let Some(entitlements) = headers.get("x-riot-entitlements-jwt") {
        outbound = outbound.header("X-Riot-Entitlements-JWT", entitlements.clone());
    }

    let response = match outbound.send().await {
        Ok(response) => response,
        Err(_) => {
            respond_with_body(&mut writer, 502, "Bad Gateway", "{}").await;
            return;
        }
    };
    let status = response.status();
    let mut body = response.text().await.unwrap_or_default();

    if status.is_success() && body.len() <= CONFIG_BODY_LIMIT {
        let affinity = match headers.get("authorization") {
            Some(authorization) if !authorization.trim().is_empty() => {
                resolve_player_affinity(authorization).await
            }
            _ => None,
        };
        let rewritten = rewrite_client_config(&body, chat_port, affinity.as_deref());

        if let Some(chat_endpoint) = rewritten.chat_endpoint {
            endpoint.set_once(chat_endpoint);
        }
        body = rewritten.body;
    }

    respond_with_body(
        &mut writer,
        status.as_u16(),
        status.canonical_reason().unwrap_or("Unknown"),
        &body,
    )
    .await;
}

async fn respond_with_body(
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

pub async fn run_chat_proxy(
    listener: TcpListener,
    pfx_bytes: Vec<u8>,
    endpoint: ChatEndpoint,
    control: ChatProxyControl,
) -> Result<(), ChatProxyError> {
    let identity = native_tls::Identity::from_pkcs12(&pfx_bytes, "")
        .map_err(|error| ChatProxyError::Tls(error.to_string()))?;
    let acceptor = native_tls::TlsAcceptor::new(identity)
        .map_err(|error| ChatProxyError::Tls(error.to_string()))?;
    let acceptor = tokio_native_tls::TlsAcceptor::from(acceptor);
    let connector =
        native_tls::TlsConnector::new().map_err(|error| ChatProxyError::Tls(error.to_string()))?;
    let connector = tokio_native_tls::TlsConnector::from(connector);

    loop {
        let (incoming, _) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let connector = connector.clone();
        let endpoint = endpoint.clone();
        let control = control.clone();

        tokio::spawn(async move {
            proxy_chat_connection(incoming, &acceptor, &connector, &endpoint, &control).await;
        });
    }
}

async fn proxy_chat_connection(
    incoming: TcpStream,
    acceptor: &tokio_native_tls::TlsAcceptor,
    connector: &tokio_native_tls::TlsConnector,
    endpoint: &ChatEndpoint,
    control: &ChatProxyControl,
) {
    let Ok(tls_incoming) = acceptor.accept(incoming).await else {
        return;
    };
    let Ok(outgoing) = TcpStream::connect((endpoint.host.as_str(), endpoint.port)).await else {
        return;
    };
    let Ok(tls_outgoing) = connector.connect(endpoint.host.as_str(), outgoing).await else {
        return;
    };

    let (mut incoming_read, mut incoming_write) = tokio::io::split(tls_incoming);
    let (mut outgoing_read, mut outgoing_write) = tokio::io::split(tls_outgoing);

    let server_to_client =
        tokio::spawn(async move { pump_raw(&mut outgoing_read, &mut incoming_write).await });
    pump_client_to_server(&mut incoming_read, &mut outgoing_write, control).await;
    server_to_client.abort();
}

async fn pump_raw<R, W>(reader: &mut R, writer: &mut W)
where
    R: AsyncReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let mut buffer = vec![0u8; CHAT_PUMP_BUFFER];

    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                if writer.write_all(&buffer[..read]).await.is_err() {
                    break;
                }
            }
        }
    }
}

async fn pump_client_to_server<R, W>(reader: &mut R, writer: &mut W, control: &ChatProxyControl)
where
    R: AsyncReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let mut buffer = vec![0u8; CHAT_PUMP_BUFFER];

    loop {
        let read = match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let chunk = &buffer[..read];
        let text = String::from_utf8_lossy(chunk);
        let (enabled, status) = control.current();

        if enabled && text.contains("<presence") {
            let rewritten = rewrite_presence_content(&text, status);

            if writer.write_all(rewritten.as_bytes()).await.is_err() {
                break;
            }
        } else if writer.write_all(chunk).await.is_err() {
            break;
        }
    }
}

#[derive(Debug, Error)]
pub enum ChatProxyError {
    #[error(
        "deceive-localhost.molenzwiebel.xyz does not resolve to this PC (127.0.0.1); add a hosts entry or change DNS before masking presence"
    )]
    LocalhostUnresolvable,
    #[error("chat proxy DNS lookup failed: {0}")]
    Dns(String),
    #[error("chat proxy I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("chat proxy HTTP error: {0}")]
    Http(String),
    #[error("chat proxy certificate download failed: {0}")]
    CertDownload(String),
    #[error("chat proxy certificate is invalid: {0}")]
    CertInvalid(String),
    #[error("chat proxy TLS error: {0}")]
    Tls(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAT_PORT: u16 = 54321;

    fn client_config() -> String {
        serde_json::json!({
            "chat.host": "chat-eu1.example.net",
            "chat.port": 5223,
            "chat.affinities": {
                "eu": "chat-eu1.example.net",
                "na": "chat-na1.example.net"
            },
            "chat.affinity.enabled": true,
            "unrelated": "kept"
        })
        .to_string()
    }

    #[test]
    fn rewrites_client_config_host_port_and_affinities() {
        let rewritten = rewrite_client_config(&client_config(), CHAT_PORT, Some("eu"));

        assert!(rewritten.body.contains(LOCALHOST_DOMAIN));
        assert!(!rewritten.body.contains("chat-eu1.example.net"));
        assert!(!rewritten.body.contains("chat-na1.example.net"));
        assert!(rewritten.body.contains("\"unrelated\":\"kept\""));
        assert_eq!(
            rewritten.chat_endpoint,
            Some(ChatEndpoint {
                host: "chat-eu1.example.net".to_string(),
                port: 5223,
            })
        );
    }

    #[test]
    fn falls_back_to_chat_host_without_an_affinity_match() {
        let rewritten = rewrite_client_config(&client_config(), CHAT_PORT, None);

        assert_eq!(
            rewritten.chat_endpoint,
            Some(ChatEndpoint {
                host: "chat-eu1.example.net".to_string(),
                port: 5223,
            })
        );
    }

    #[test]
    fn passes_through_non_json_client_config() {
        let rewritten = rewrite_client_config("internal error", CHAT_PORT, None);

        assert_eq!(rewritten.body, "internal error");
        assert_eq!(rewritten.chat_endpoint, None);
    }

    #[test]
    fn reads_affinity_from_a_pas_jwt() {
        let jwt = "header.eyJhZmZpbml0eSI6ImV1In0.signature";

        assert_eq!(affinity_from_pas_jwt(jwt).as_deref(), Some("eu"));
        assert_eq!(affinity_from_pas_jwt("not-a-jwt"), None);
        assert_eq!(affinity_from_pas_jwt("a.b.c"), None);
    }

    #[test]
    fn loopback_check_matches_only_this_pc() {
        let loopback: SocketAddr = "127.0.0.1:443".parse().expect("loopback");
        let remote: SocketAddr = "8.8.8.8:443".parse().expect("remote");

        assert!(addresses_resolve_to_loopback(&[loopback]));
        assert!(!addresses_resolve_to_loopback(&[remote]));
        assert!(!addresses_resolve_to_loopback(&[]));
    }

    #[test]
    fn rejects_garbage_proxy_certificates() {
        assert!(!certificate_is_fresh(b"not a pfx"));
        assert!(!certificate_is_fresh(&[]));
    }

    #[test]
    fn client_config_url_quotes_the_loopback_address() {
        assert_eq!(
            client_config_url_arg(1234),
            "--client-config-url=\"http://127.0.0.1:1234\"".to_string()
        );
    }

    #[test]
    fn proxy_control_reads_live_status() {
        let control = ChatProxyControl::new(PresenceStatus::Invisible);

        assert_eq!(control.current(), (true, PresenceStatus::Invisible));

        control.set_status(PresenceStatus::Mobile);
        control.set_enabled(false);

        assert_eq!(control.current(), (false, PresenceStatus::Mobile));
    }
}
