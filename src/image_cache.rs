use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use thiserror::Error;
use url::Url;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageCache {
    root: PathBuf,
}

impl ImageCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn default_path() -> PathBuf {
        ProjectDirs::from("dev", "prime", "prime")
            .map(|dirs| dirs.cache_dir().join("images"))
            .unwrap_or_else(|| PathBuf::from("image-cache"))
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn size_bytes(&self) -> Result<u64, ImageCacheError> {
        dir_size(&self.root).map_err(ImageCacheError::Io)
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
            return Ok(path);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let bytes = reqwest::Client::new()
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        fs::write(&path, bytes)?;

        Ok(path)
    }

    fn asset_path(&self, namespace: &str, id: &str, url: &str) -> PathBuf {
        self.root
            .join(sanitize_path_component(namespace))
            .join(format!(
                "{}.{}",
                sanitize_path_component(id),
                image_extension(url)
            ))
    }
}

fn dir_size(path: &Path) -> io::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }

    let mut size = 0;

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;

        if metadata.is_dir() {
            size += dir_size(&entry.path())?;
        } else {
            size += metadata.len();
        }
    }

    Ok(size)
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

#[derive(Debug, Error)]
pub enum ImageCacheError {
    #[error("image cache I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("image cache HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn size_bytes_counts_nested_files() {
        let dir = tempdir().expect("cache dir");
        let nested = dir.path().join("skins");
        fs::create_dir(&nested).expect("nested dir");
        fs::write(dir.path().join("root.bin"), [1, 2, 3]).expect("root file");
        fs::write(nested.join("skin.bin"), [4, 5]).expect("nested file");

        let cache = ImageCache::new(dir.path());

        assert_eq!(cache.size_bytes().expect("size"), 5);
    }

    #[test]
    fn asset_path_uses_stable_safe_names() {
        let cache = ImageCache::new("cache");

        assert_eq!(
            cache.asset_path("skins", "abc/123", "https://example.com/render.PNG?x=1"),
            PathBuf::from("cache").join("skins").join("abc_123.png")
        );
    }
}
