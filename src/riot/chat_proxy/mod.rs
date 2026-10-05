use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration as StdDuration;

use thiserror::Error;
use tokio::net::TcpListener;
use x509_parser::prelude::{FromDer, X509Certificate};

mod config;
mod presence;
mod relay;

pub use config::LOCALHOST_DOMAIN;
pub use presence::PresenceStatus;

pub const PROXY_CERT_URL: &str = "https://mln.cx/deceive/localhost.pfx";
pub const CERT_MIN_VALID_DAYS: i64 = 20;

const CERT_HTTP_TIMEOUT: StdDuration = StdDuration::from_secs(20);

pub fn client_config_url_arg(port: u16) -> String {
    format!("--client-config-url=\"http://127.0.0.1:{port}\"")
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

pub async fn bind_loopback_listener() -> Result<(TcpListener, u16), ChatProxyError> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    Ok((listener, port))
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
}
