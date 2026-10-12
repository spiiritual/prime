use std::env;

use thiserror::Error;

use crate::account::non_empty_string;
use velopack::locator::{self, LocationContext};
use velopack::sources::{AutoSource, GithubSource};
use velopack::{UpdateCheck, UpdateInfo, UpdateManager, UpdateOptions};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_UPDATE_SOURCE: &str = "https://github.com/spiiritual/prime";
pub const UPDATE_SOURCE_ENV: &str = "PRIME_UPDATE_SOURCE";
pub const UPDATE_CHANNEL_ENV: &str = "PRIME_UPDATE_CHANNEL";
/// Velopack's default channel, which stable releases use.
pub const STABLE_CHANNEL: &str = "win";
/// Beta releases. Stable releases are packaged for it too, unless beta is already ahead.
pub const BETA_CHANNEL: &str = "beta";

#[derive(Clone, Debug)]
pub struct AvailableUpdate {
    pub current_version: String,
    pub latest_version: String,
    pub changelog: Option<String>,
    update: Box<UpdateInfo>,
    /// The channel this update moves to, when it switches channels.
    switch_to: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub enum UpdateCheckOutcome {
    UpToDate,
    Available(AvailableUpdate),
    /// A channel switch found no release on that channel, so nothing changes.
    ChannelEmpty(&'static str),
    /// Running outside a Velopack install (for example `cargo run`), where updates cannot apply.
    NotInstalled,
}

/// The channel update checks follow: `PRIME_UPDATE_CHANNEL`, or the one this installed copy
/// came from. `None` outside an install, such as `cargo run`, unless the variable is set.
pub fn current_channel() -> Option<String> {
    update_channel().or_else(|| {
        locator::auto_locate_app_manifest(LocationContext::FromCurrentExe)
            .ok()
            .map(|locator| locator.get_manifest_channel())
    })
}

impl AvailableUpdate {
    /// The channel installing this update moves to, when it switches channels.
    pub fn switch_to(&self) -> Option<&'static str> {
        self.switch_to
    }
}

/// Checks this copy's channel, or `switch_to`'s. Switching offers that channel's latest release
/// even when it's older, so leaving beta installs the latest stable version.
pub async fn check_for_update(
    switch_to: Option<&'static str>,
) -> Result<UpdateCheckOutcome, UpdateError> {
    tokio::task::spawn_blocking(move || check_for_update_blocking(switch_to)).await?
}

pub async fn download_and_prepare_update(update: AvailableUpdate) -> Result<(), UpdateError> {
    tokio::task::spawn_blocking(move || download_and_prepare_update_blocking(update)).await?
}

fn check_for_update_blocking(
    switch_to: Option<&'static str>,
) -> Result<UpdateCheckOutcome, UpdateError> {
    let manager = match update_manager(switch_to) {
        Ok(manager) => manager,
        Err(UpdateError::Velopack(velopack::Error::NotInstalled(_))) => {
            return Ok(UpdateCheckOutcome::NotInstalled);
        }
        Err(error) => return Err(error),
    };

    match manager.check_for_updates()? {
        UpdateCheck::UpdateAvailable(update) => Ok(UpdateCheckOutcome::Available(
            available_update_from_info(manager.get_current_version_as_string(), update, switch_to),
        )),
        UpdateCheck::RemoteIsEmpty => Ok(match switch_to {
            Some(channel) => UpdateCheckOutcome::ChannelEmpty(channel),
            None => UpdateCheckOutcome::UpToDate,
        }),
        UpdateCheck::NoUpdateAvailable => Ok(UpdateCheckOutcome::UpToDate),
    }
}

fn download_and_prepare_update_blocking(update: AvailableUpdate) -> Result<(), UpdateError> {
    let manager = update_manager(update.switch_to)?;
    let update_info: &UpdateInfo = &update.update;

    manager.download_updates(update_info, None)?;
    manager.wait_exit_then_apply_updates(update_info, false, true, Vec::<String>::new())?;

    Ok(())
}

fn update_manager(switch_to: Option<&'static str>) -> Result<UpdateManager, UpdateError> {
    let source = update_source();
    let explicit = switch_to.map(str::to_string).or_else(update_channel);
    let beta = explicit.clone().or_else(current_channel).as_deref() == Some(BETA_CHANNEL);
    let options = UpdateOptions {
        // Only a switch may go back a version: leaving beta installs the older stable release.
        AllowVersionDowngrade: switch_to.is_some(),
        ExplicitChannel: explicit,
        ..UpdateOptions::default()
    };

    // Betas are GitHub pre-releases, and Velopack reads only the 10 newest releases. Stable reads
    // stable releases only, so betas never crowd it out. Nearly every release carries the beta
    // feed, so beta always finds one.
    if source == DEFAULT_UPDATE_SOURCE {
        UpdateManager::new(GithubSource::new(&source, None, beta), Some(options), None)
    } else {
        UpdateManager::new(AutoSource::new(&source), Some(options), None)
    }
    .map_err(UpdateError::from)
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

fn available_update_from_info(
    current_version: String,
    update: Box<UpdateInfo>,
    switch_to: Option<&'static str>,
) -> AvailableUpdate {
    let target = &update.TargetFullRelease;
    let latest_version = target.Version.clone();
    let changelog = non_empty_string(target.NotesMarkdown.clone());

    AvailableUpdate {
        current_version,
        latest_version,
        changelog,
        update,
        switch_to,
    }
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
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use velopack::VelopackAsset;

    #[test]
    fn uninstalled_builds_report_not_installed_instead_of_an_error() {
        let outcome = check_for_update_blocking(None).expect("check");

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
            None,
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
            None,
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
