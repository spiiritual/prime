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
/// Deceive's author can replace the certificate before it expires, so a cached copy this old is
/// fetched again. It's still used, while in date, if that download fails.
const REFRESH_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);

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

/// The cached certificate while it's in date and under 30 days old; otherwise a fresh download,
/// saved over it. Riot Client refuses an expired certificate, so an out-of-date one is never used.
async fn load_identity_from(
    path: &Path,
    download: impl Future<Output = Result<Vec<u8>, String>>,
) -> Result<native_tls::Identity, ChatProxyError> {
    let cached = fs::read(path).ok().and_then(|bytes| identity(&bytes));
    let recent = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age < REFRESH_AFTER);
    if let Some(cached) = &cached
        && recent
    {
        return Ok(cached.clone());
    }

    let downloaded = download.await.and_then(|bytes| {
        identity(&bytes)
            .map(|fresh| (fresh, bytes))
            .ok_or_else(|| "the download isn't a usable certificate, or it has expired".to_string())
    });
    match downloaded {
        Ok((fresh, bytes)) => {
            // A failed save is ignored; the next launch downloads it again.
            let _ = path
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| crate::image_cache::write_cache_file(path, &bytes));
            Ok(fresh)
        }
        Err(error) => cached.ok_or(ChatProxyError::Certificate(error)),
    }
}

/// The certificate in `bytes`, if it reads and every certificate in it is in date now.
fn identity(bytes: &[u8]) -> Option<native_tls::Identity> {
    let store = schannel::cert_store::PfxImportOptions::new()
        .password("")
        .no_persist_key(true)
        .import(bytes)
        .ok()?;
    let mut certs = store.certs().peekable();
    certs.peek()?;
    if !certs.all(|cert| cert.is_time_valid().unwrap_or(false)) {
        return None;
    }
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

    // Throwaway self-signed certificates with an empty password, like Deceive's. Nothing trusts
    // them; they aren't secrets. The second expired in 2021.
    const TEST_PFX: &[u8] = include_bytes!("../../../tests/fixtures/chat-proxy-test.pfx");
    const EXPIRED_PFX: &[u8] = include_bytes!("../../../tests/fixtures/chat-proxy-expired.pfx");

    #[tokio::test]
    async fn an_in_date_cached_certificate_is_used_without_downloading() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("cert.pfx");
        fs::write(&path, TEST_PFX).expect("write");

        let loaded = load_identity_from(&path, async { panic!("downloaded") }).await;

        assert!(loaded.is_ok(), "{:?}", loaded.err());
    }

    #[tokio::test]
    async fn an_expired_cached_certificate_is_downloaded_again() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("cert.pfx");
        fs::write(&path, EXPIRED_PFX).expect("write");

        let loaded = load_identity_from(&path, async { Ok(TEST_PFX.to_vec()) }).await;

        assert!(loaded.is_ok(), "{:?}", loaded.err());
        assert_eq!(fs::read(&path).expect("read"), TEST_PFX);
    }

    #[tokio::test]
    async fn a_month_old_certificate_is_kept_when_the_refresh_fails() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("cert.pfx");
        fs::write(&path, TEST_PFX).expect("write");
        let month_ago = std::time::SystemTime::now() - REFRESH_AFTER - Duration::from_secs(60);
        fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(month_ago))
            .expect("age the file");
        let tried = std::cell::Cell::new(false);

        let loaded = load_identity_from(&path, async {
            tried.set(true);
            Err("offline".to_string())
        })
        .await;

        assert!(tried.get(), "a month-old copy is refreshed");
        assert!(loaded.is_ok(), "{:?}", loaded.err());
    }

    #[tokio::test]
    async fn an_expired_download_is_refused() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("cert.pfx");

        let loaded = load_identity_from(&path, async { Ok(EXPIRED_PFX.to_vec()) }).await;

        assert!(loaded.is_err());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn a_usable_download_is_used_even_when_it_cant_be_cached() {
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
