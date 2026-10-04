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
    update: Box<UpdateInfo>,
}

#[derive(Clone, Debug)]
pub enum UpdateCheckOutcome {
    UpToDate,
    Available(AvailableUpdate),
    /// Running outside a Velopack install (for example `cargo run`), where updates cannot apply.
    NotInstalled,
}

pub async fn check_for_update() -> Result<UpdateCheckOutcome, UpdateError> {
    tokio::task::spawn_blocking(check_for_update_blocking).await?
}

pub async fn download_and_prepare_update(update: AvailableUpdate) -> Result<(), UpdateError> {
    tokio::task::spawn_blocking(move || download_and_prepare_update_blocking(update)).await?
}

fn check_for_update_blocking() -> Result<UpdateCheckOutcome, UpdateError> {
    let manager = match update_manager() {
        Ok(manager) => manager,
        Err(UpdateError::Velopack(velopack::Error::NotInstalled(_))) => {
            return Ok(UpdateCheckOutcome::NotInstalled);
        }
        Err(error) => return Err(error),
    };

    match manager.check_for_updates()? {
        UpdateCheck::UpdateAvailable(update) => Ok(UpdateCheckOutcome::Available(
            available_update_from_info(manager.get_current_version_as_string(), update),
        )),
        UpdateCheck::RemoteIsEmpty | UpdateCheck::NoUpdateAvailable => {
            Ok(UpdateCheckOutcome::UpToDate)
        }
    }
}

fn download_and_prepare_update_blocking(update: AvailableUpdate) -> Result<(), UpdateError> {
    let manager = update_manager()?;
    let update_info: &UpdateInfo = &update.update;

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

    AvailableUpdate {
        current_version,
        latest_version,
        changelog,
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

/// An update to the given version, for tests elsewhere in the crate.
#[cfg(test)]
pub(crate) fn sample_update(latest_version: &str) -> AvailableUpdate {
    available_update_from_info(
        CURRENT_VERSION.to_string(),
        Box::new(UpdateInfo {
            TargetFullRelease: velopack::VelopackAsset {
                PackageId: "prime".to_string(),
                Version: latest_version.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use velopack::VelopackAsset;

    #[test]
    fn uninstalled_builds_report_not_installed_instead_of_an_error() {
        let outcome = check_for_update_blocking().expect("check");

        assert!(matches!(outcome, UpdateCheckOutcome::NotInstalled));
    }

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
    }

    #[test]
    fn blank_release_notes_give_no_changelog() {
        let update = available_update_from_info(
            "0.1.2".to_string(),
            Box::new(full_update(asset(
                "prime",
                "0.1.3",
                "prime-0.1.3-full.nupkg",
                128,
                "  ",
            ))),
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
