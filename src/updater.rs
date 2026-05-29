use std::env;

use thiserror::Error;
use velopack::sources::AutoSource;
use velopack::{UpdateCheck, UpdateInfo, UpdateManager, UpdateOptions};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_UPDATE_SOURCE: &str = "https://github.com/spiiritual/prime";
pub const UPDATE_SOURCE_ENV: &str = "PRIME_UPDATE_SOURCE";
pub const UPDATE_CHANNEL_ENV: &str = "PRIME_UPDATE_CHANNEL";

#[derive(Clone, Debug)]
pub struct AvailableUpdate {
    pub current_version: String,
    pub latest_version: String,
    pub changelog: Option<String>,
    pub package: ReleasePackage,
    update: Box<UpdateInfo>,
}

impl AvailableUpdate {
    fn update_info(&self) -> &UpdateInfo {
        &self.update
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleasePackage {
    pub package_id: Option<String>,
    pub file_name: String,
    pub size_bytes: u64,
    pub update_strategy: UpdateStrategy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateStrategy {
    Full,
    Delta {
        package_count: usize,
        size_bytes: u64,
    },
}

pub async fn check_for_update() -> Result<Option<AvailableUpdate>, UpdateError> {
    tokio::task::spawn_blocking(check_for_update_blocking)
        .await?
        .map_err(UpdateError::from)
}

pub async fn download_and_prepare_update(update: AvailableUpdate) -> Result<(), UpdateError> {
    tokio::task::spawn_blocking(move || download_and_prepare_update_blocking(update))
        .await?
        .map_err(UpdateError::from)
}

fn check_for_update_blocking() -> Result<Option<AvailableUpdate>, UpdateError> {
    let manager = update_manager()?;

    match manager.check_for_updates()? {
        UpdateCheck::UpdateAvailable(update) => Ok(Some(available_update_from_info(
            manager.get_current_version_as_string(),
            update,
        ))),
        UpdateCheck::RemoteIsEmpty | UpdateCheck::NoUpdateAvailable => Ok(None),
    }
}

fn download_and_prepare_update_blocking(update: AvailableUpdate) -> Result<(), UpdateError> {
    let manager = update_manager()?;
    let update_info = update.update_info();

    manager.download_updates(update_info, None)?;
    manager.wait_exit_then_apply_updates(update_info, false, true, Vec::<String>::new())?;

    Ok(())
}

fn update_manager() -> Result<UpdateManager, UpdateError> {
    let source = AutoSource::new(&update_source());
    let options = UpdateOptions {
        ExplicitChannel: update_channel(),
        ..UpdateOptions::default()
    };

    UpdateManager::new(source, Some(options), None).map_err(UpdateError::from)
}

fn update_source() -> String {
    env::var(UPDATE_SOURCE_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_UPDATE_SOURCE.to_string())
}

fn update_channel() -> Option<String> {
    env::var(UPDATE_CHANNEL_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn available_update_from_info(current_version: String, update: Box<UpdateInfo>) -> AvailableUpdate {
    let target = &update.TargetFullRelease;
    let latest_version = target.Version.clone();
    let changelog = trimmed_text(&target.NotesMarkdown);
    let delta_size_bytes = update
        .DeltasToTarget
        .iter()
        .map(|delta| delta.Size)
        .sum::<u64>();
    let update_strategy = if update.DeltasToTarget.is_empty() {
        UpdateStrategy::Full
    } else {
        UpdateStrategy::Delta {
            package_count: update.DeltasToTarget.len(),
            size_bytes: delta_size_bytes,
        }
    };

    AvailableUpdate {
        current_version,
        latest_version,
        changelog,
        package: ReleasePackage {
            package_id: trimmed_text(&target.PackageId),
            file_name: target.FileName.clone(),
            size_bytes: target.Size,
            update_strategy,
        },
        update,
    }
}

fn trimmed_text(value: &str) -> Option<String> {
    let value = value.trim();

    (!value.is_empty()).then(|| value.to_string())
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("Velopack update error: {0}")]
    Velopack(#[from] velopack::Error),
    #[error("update task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use velopack::VelopackAsset;

    #[test]
    fn update_summary_uses_release_notes_markdown() {
        let update = available_update_from_info(
            "0.1.2".to_string(),
            Box::new(full_update(asset(
                "prime",
                "0.1.3",
                "prime-0.1.3-full.nupkg",
                128,
                "  ## Changes\n\n- Velopack  ",
            ))),
        );

        assert_eq!(update.latest_version, "0.1.3");
        assert_eq!(
            update.changelog.as_deref(),
            Some("## Changes\n\n- Velopack")
        );
        assert_eq!(update.package.package_id.as_deref(), Some("prime"));
        assert_eq!(update.package.update_strategy, UpdateStrategy::Full);
    }

    #[test]
    fn update_summary_reports_delta_strategy() {
        let update = available_update_from_info(
            "0.1.2".to_string(),
            Box::new(delta_update(
                asset("prime", "0.1.4", "prime-0.1.4-full.nupkg", 1_024, ""),
                asset("prime", "0.1.2", "prime-0.1.2-full.nupkg", 900, ""),
                vec![
                    asset("prime", "0.1.3", "prime-0.1.3-delta.nupkg", 80, ""),
                    asset("prime", "0.1.4", "prime-0.1.4-delta.nupkg", 90, ""),
                ],
            )),
        );

        assert_eq!(
            update.package.update_strategy,
            UpdateStrategy::Delta {
                package_count: 2,
                size_bytes: 170
            }
        );
        assert_eq!(update.changelog, None);
    }

    fn full_update(target: VelopackAsset) -> UpdateInfo {
        UpdateInfo {
            TargetFullRelease: target,
            BaseRelease: None,
            DeltasToTarget: Vec::new(),
            IsDowngrade: false,
        }
    }

    fn delta_update(
        target: VelopackAsset,
        base: VelopackAsset,
        deltas: Vec<VelopackAsset>,
    ) -> UpdateInfo {
        UpdateInfo {
            TargetFullRelease: target,
            BaseRelease: Some(base),
            DeltasToTarget: deltas,
            IsDowngrade: false,
        }
    }

    fn asset(
        package_id: &str,
        version: &str,
        file_name: &str,
        size: u64,
        notes_markdown: &str,
    ) -> VelopackAsset {
        VelopackAsset {
            PackageId: package_id.to_string(),
            Version: version.to_string(),
            Type: if file_name.contains("delta") {
                "Delta".to_string()
            } else {
                "Full".to_string()
            },
            FileName: file_name.to_string(),
            SHA1: "sha1".to_string(),
            SHA256: "sha256".to_string(),
            Size: size,
            NotesMarkdown: notes_markdown.to_string(),
            NotesHtml: String::new(),
        }
    }
}
