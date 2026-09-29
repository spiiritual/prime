use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use directories::ProjectDirs;
use thiserror::Error;
use url::Url;

const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// Images nobody has shown for this long are deleted at startup. A file's modified time records its
/// last use, because Windows often doesn't keep access times.
const UNUSED_IMAGE_LIFETIME: Duration = Duration::from_secs(60 * 24 * 60 * 60);
const USER_AGENT_VALUE: &str = concat!("prime/", env!("CARGO_PKG_VERSION"));

#[derive(Clone)]
pub struct ImageCache {
    root: PathBuf,
    client: reqwest::Client,
}

impl ImageCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent(USER_AGENT_VALUE)
            .build()
            .expect("image cache HTTP client configuration should be valid");

        Self {
            root: root.into(),
            client,
        }
    }

    pub fn default_path() -> PathBuf {
        ProjectDirs::from("dev", "spiiritual", "prime")
            .map(|dirs| dirs.cache_dir().join("images"))
            .unwrap_or_else(|| PathBuf::from("image-cache"))
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn usage(&self) -> Result<CacheUsage, ImageCacheError> {
        let mut usage = CacheUsage::default();
        add_dir_usage(&self.root, None, &mut usage)?;
        Ok(usage)
    }

    /// Deletes images unused for 60 days and returns the usage of what's left.
    pub fn remove_unused(&self) -> Result<CacheUsage, ImageCacheError> {
        let cutoff = SystemTime::now()
            .checked_sub(UNUSED_IMAGE_LIFETIME)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let mut usage = CacheUsage::default();
        add_dir_usage(&self.root, Some(cutoff), &mut usage)?;
        Ok(usage)
    }

    pub fn clear(&self) -> Result<(), ImageCacheError> {
        if self.root.exists() {
            fs::remove_dir_all(&self.root)?;
        }

        Ok(())
    }

    pub async fn cache_url(
        &self,
        namespace: &str,
        id: &str,
        url: &str,
    ) -> Result<PathBuf, ImageCacheError> {
        let path = self.asset_path(namespace, id, url);

        if path.exists() {
            // Only delays expiry, so a failure doesn't matter.
            let _ = fs::File::options()
                .write(true)
                .open(&path)
                .and_then(|file| file.set_modified(SystemTime::now()));
            return Ok(path);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let bytes = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        write_cache_file(&path, &bytes)?;

        Ok(path)
    }

    fn asset_path(&self, namespace: &str, id: &str, url: &str) -> PathBuf {
        self.root
            .join(sanitize_path_component(namespace))
            .join(format!(
                "{}-{:016x}.{}",
                sanitize_path_component(id),
                url_fingerprint(url),
                image_extension(url)
            ))
    }
}

impl std::fmt::Debug for ImageCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageCache")
            .field("root", &self.root)
            .finish()
    }
}

impl PartialEq for ImageCache {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
    }
}

impl Eq for ImageCache {}

/// How much the cache holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheUsage {
    pub bytes: u64,
    pub files: u64,
}

/// Adds up the files under `path`, first deleting any last used before `remove_before`.
fn add_dir_usage(
    path: &Path,
    remove_before: Option<SystemTime>,
    usage: &mut CacheUsage,
) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;

        if metadata.is_dir() {
            add_dir_usage(&entry.path(), remove_before, usage)?;
        } else if remove_before
            .is_some_and(|cutoff| metadata.modified().is_ok_and(|used| used < cutoff))
            && fs::remove_file(entry.path()).is_ok()
        {
            continue;
        } else {
            usage.bytes += metadata.len();
            usage.files += 1;
        }
    }

    Ok(())
}

fn sanitize_path_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '_',
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "asset".to_string()
    } else {
        sanitized
    }
}

fn image_extension(raw_url: &str) -> String {
    Url::parse(raw_url)
        .ok()
        .and_then(|url| {
            Path::new(url.path())
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.to_ascii_lowercase())
        })
        .filter(|extension| matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp"))
        .unwrap_or_else(|| "png".to_string())
}

fn url_fingerprint(value: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    value
        .as_bytes()
        .iter()
        .fold(FNV_OFFSET_BASIS, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
        })
}

#[derive(Debug, Error)]
pub enum ImageCacheError {
    #[error("image cache I/O error: {0}")]
    Io(#[from] io::Error),
    #[error(
        "image cache HTTP error: {}",
        crate::http_error::format_reqwest_error(.0)
    )]
    Http(#[from] reqwest::Error),
}

/// Writes through a temporary file so an interrupted write never leaves a truncated image that
/// later loads would treat as already cached.
fn write_cache_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    // Images download at the same time, so each write gets its own partial file.
    static NEXT_PARTIAL: AtomicU64 = AtomicU64::new(0);

    let mut partial = path.as_os_str().to_owned();
    partial.push(format!(
        ".{}-{}.partial",
        std::process::id(),
        NEXT_PARTIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let partial = PathBuf::from(partial);

    fs::write(&partial, bytes)?;
    if let Err(error) = fs::rename(&partial, path) {
        let _ = fs::remove_file(&partial);
        // Another download of the same image may have finished first.
        if path.exists() {
            return Ok(());
        }
        return Err(error);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn cache_file_write_leaves_only_the_finished_image() {
        let dir = tempdir().expect("cache dir");
        let path = dir.path().join("skin.png");
        fs::write(&path, [9]).expect("stale file");

        write_cache_file(&path, &[1, 2, 3]).expect("write");

        assert_eq!(fs::read(&path).expect("image"), [1, 2, 3]);
        assert_eq!(fs::read_dir(dir.path()).expect("cache dir").count(), 1);
    }

    #[test]
    fn cache_file_writes_do_not_share_a_partial_file() {
        let dir = tempdir().expect("cache dir");
        let path = dir.path().join("skin.png");
        // Another download of the same image holding the plain partial name.
        fs::create_dir(dir.path().join("skin.png.partial")).expect("other writer");

        write_cache_file(&path, &[1, 2, 3]).expect("write");

        assert_eq!(fs::read(&path).expect("image"), [1, 2, 3]);
    }

    #[test]
    fn remove_unused_deletes_only_images_unused_for_60_days() {
        let dir = tempdir().expect("cache dir");
        let nested = dir.path().join("skins");
        fs::create_dir(&nested).expect("nested dir");
        let old = nested.join("old.png");
        let fresh = nested.join("fresh.png");
        fs::write(&old, [1, 2, 3]).expect("old file");
        fs::write(&fresh, [4, 5]).expect("fresh file");
        fs::File::options()
            .write(true)
            .open(&old)
            .and_then(|file| {
                file.set_modified(SystemTime::now() - Duration::from_secs(61 * 24 * 60 * 60))
            })
            .expect("age old file");

        let usage = ImageCache::new(dir.path()).remove_unused().expect("remove");

        assert_eq!(usage, CacheUsage { bytes: 2, files: 1 });
        assert!(!old.exists());
        assert!(fresh.exists());
    }

    #[test]
    fn usage_counts_nested_files() {
        let dir = tempdir().expect("cache dir");
        let nested = dir.path().join("skins");
        fs::create_dir(&nested).expect("nested dir");
        fs::write(dir.path().join("root.bin"), [1, 2, 3]).expect("root file");
        fs::write(nested.join("skin.bin"), [4, 5]).expect("nested file");

        let cache = ImageCache::new(dir.path());

        assert_eq!(
            cache.usage().expect("usage"),
            CacheUsage { bytes: 5, files: 2 }
        );
    }

    #[test]
    fn asset_path_uses_stable_safe_names() {
        let cache = ImageCache::new("cache");
        let path = cache.asset_path("skins", "abc/123", "https://example.com/render.PNG?x=1");

        assert_eq!(
            path.parent(),
            Some(PathBuf::from("cache").join("skins").as_path())
        );
        assert_eq!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("png")
        );
        assert!(
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("abc_123-"))
        );
    }

    #[test]
    fn asset_path_changes_when_url_changes() {
        let cache = ImageCache::new("cache");

        assert_ne!(
            cache.asset_path("skins", "skin-id", "https://example.com/displayicon.png"),
            cache.asset_path("skins", "skin-id", "https://example.com/fullrender.png")
        );
    }
}
