//! The TLS certificate Riot Client sees when it connects to the relay, and the check that its
//! domain resolves to this PC.

use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use thiserror::Error;

use super::config::LOCALHOST_DOMAIN;

const CERTIFICATE_URL: &str = "https://mln.cx/deceive/localhost.pfx";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(20);
/// Riot Client checks the certificate's dates, so a cached copy is only fetched again weekly, in
/// case Deceive's author renewed it.
const REFRESH_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Error)]
pub enum ChatProxyError {
    #[error(
        "Riot Client's chat can't go through Prime: {} doesn't point at this PC",
        LOCALHOST_DOMAIN
    )]
    NotLoopback,
    #[error("Couldn't look up {domain}: {0}", domain = LOCALHOST_DOMAIN)]
    Dns(String),
    #[error("Couldn't get the chat certificate: {0}")]
    Certificate(String),
    #[error("Couldn't start the chat proxy: {0}")]
    Io(#[from] io::Error),
    #[error("Couldn't set up the chat proxy's TLS: {0}")]
    Tls(String),
}

pub(super) fn default_path() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(|dir| PathBuf::from(dir).join(r"spiiritual\prime\cache\chat-proxy-localhost.pfx"))
        .unwrap_or_else(|| PathBuf::from("chat-proxy-localhost.pfx"))
}

/// Riot Client resolves the name itself, so this can't stop it connecting elsewhere later. It
/// does stop a launch through a resolver that already sends the name off this PC, which would
/// hand Riot Client's chat login to whoever answers.
pub(super) async fn check_localhost_domain() -> Result<(), ChatProxyError> {
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((LOCALHOST_DOMAIN, 443))
        .await
        .map_err(|error| ChatProxyError::Dns(error.to_string()))?
        .collect();
    if all_loopback(&addresses) {
        Ok(())
    } else {
        Err(ChatProxyError::NotLoopback)
    }
}

fn all_loopback(addresses: &[SocketAddr]) -> bool {
    !addresses.is_empty() && addresses.iter().all(|address| address.ip().is_loopback())
}

pub(super) async fn load_identity(path: &Path) -> Result<native_tls::Identity, ChatProxyError> {
    load_identity_from(path, download()).await
}

/// The cached certificate while it's under a week old; otherwise a fresh download, saved over it.
/// A failed download falls back to the cached copy, however old.
async fn load_identity_from(
    path: &Path,
    download: impl Future<Output = Result<Vec<u8>, String>>,
) -> Result<native_tls::Identity, ChatProxyError> {
    let cached = fs::read(path).ok().and_then(|bytes| identity(&bytes));
    let age = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok());
    if let Some(identity) = &cached
        && age.is_some_and(|age| age < REFRESH_AFTER)
    {
        return Ok(identity.clone());
    }

    match download.await {
        Ok(bytes) => match identity(&bytes) {
            Some(fresh) => {
                // A failed save is ignored; the next launch downloads it again.
                let _ = path
                    .parent()
                    .map_or(Ok(()), fs::create_dir_all)
                    .and_then(|()| crate::image_cache::write_cache_file(path, &bytes));
                Ok(fresh)
            }
            None => cached.ok_or_else(|| {
                ChatProxyError::Certificate("the download isn't a usable certificate".to_string())
            }),
        },
        Err(error) => cached.ok_or(ChatProxyError::Certificate(error)),
    }
}

fn identity(bytes: &[u8]) -> Option<native_tls::Identity> {
    native_tls::Identity::from_pkcs12(bytes, "").ok()
}

async fn download() -> Result<Vec<u8>, String> {
    let failed = |error: reqwest::Error| crate::http_error::format_reqwest_error(&error);
    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .user_agent(concat!("prime/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(failed)?;
    Ok(client
        .get(CERTIFICATE_URL)
        .send()
        .await
        .map_err(failed)?
        .error_for_status()
        .map_err(failed)?
        .bytes()
        .await
        .map_err(failed)?
        .to_vec())
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tempfile::tempdir;

    use super::*;

    fn address(text: &str) -> SocketAddr {
        text.parse().expect("address")
    }

    #[test]
    fn the_domain_must_resolve_to_this_pc_alone() {
        assert!(all_loopback(&[address("127.0.0.1:443")]));
        assert!(all_loopback(&[
            address("127.0.0.1:443"),
            address("[::1]:443")
        ]));
        assert!(!all_loopback(&[
            address("127.0.0.1:443"),
            address("8.8.8.8:443")
        ]));
        assert!(!all_loopback(&[address("8.8.8.8:443")]));
        assert!(!all_loopback(&[]));
    }

    #[tokio::test]
    async fn without_a_usable_certificate_the_download_error_is_reported() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("cert.pfx");
        fs::write(&path, b"not a pfx").expect("write");

        let error = load_identity_from(&path, async { Err("offline".to_string()) })
            .await
            .err()
            .expect("no certificate");

        assert!(error.to_string().contains("offline"), "{error}");
    }

    #[tokio::test]
    async fn a_usable_download_is_used_even_when_it_cant_be_cached() {
        // A throwaway self-signed certificate with an empty password, like Deceive's. Nothing trusts it; it isn't a secret.
        const TEST_PFX: &[u8] = include_bytes!("../../../tests/fixtures/chat-proxy-test.pfx");
        let dir = tempdir().expect("temp dir");
        let not_a_dir = dir.path().join("file");
        fs::write(&not_a_dir, b"").expect("write");
        let path = not_a_dir.join("cert.pfx");

        let loaded = load_identity_from(&path, async { Ok(TEST_PFX.to_vec()) }).await;

        assert!(loaded.is_ok(), "{:?}", loaded.err());
    }

    #[tokio::test]
    async fn an_unusable_download_is_not_cached() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("cert.pfx");

        let error = load_identity_from(&path, async { Ok(b"not a pfx".to_vec()) })
            .await
            .err()
            .expect("unusable");

        assert!(matches!(error, ChatProxyError::Certificate(_)), "{error}");
        assert!(!path.exists());
    }
}
