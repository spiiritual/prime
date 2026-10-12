use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;
use time::OffsetDateTime;

use crate::account::{AccountId, LauncherSessionBackup};

pub(crate) const PRIVATE_SETTINGS_FILE: &str = "RiotGamesPrivateSettings.yaml";

mod backup_files;
mod private_settings;

use backup_files::{clear_dir, replace_dir_contents};
use private_settings::{
    private_settings_is_legacy_ssid_login, update_private_settings_refresh_token,
};
pub use private_settings::{private_settings_refresh_token, private_settings_signed_in_puuid};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedLauncherSession {
    pub account_id: AccountId,
    pub backup: LauncherSessionBackup,
}

pub fn capture_current_launcher_session(
    account_id: AccountId,
    backup_root: impl AsRef<Path>,
) -> Result<CapturedLauncherSession, LauncherSessionError> {
    let source_data_dir = ready_launcher_data_dir(default_data_dirs())
        .ok_or(LauncherSessionError::PrivateSettingsNotFound)?;

    capture_launcher_session_from_data_dir(account_id, source_data_dir, backup_root)
}

pub fn ready_launcher_data_dir(data_dirs: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    data_dirs
        .into_iter()
        .find(|dir| read_data_dir_refresh_token(dir).is_ok())
}

pub fn capture_launcher_session_from_data_dir(
    account_id: AccountId,
    source_data_dir: impl AsRef<Path>,
    backup_root: impl AsRef<Path>,
) -> Result<CapturedLauncherSession, LauncherSessionError> {
    let source_data_dir = source_data_dir.as_ref();
    read_data_dir_refresh_token(source_data_dir)?;

    let backup_data_dir = backup_root
        .as_ref()
        .join(account_id.to_string())
        .join("Data");

    replace_dir_contents(source_data_dir, &backup_data_dir)?;

    Ok(CapturedLauncherSession {
        account_id,
        backup: LauncherSessionBackup {
            data_dir: backup_data_dir,
            captured_at_unix: OffsetDateTime::now_utc().unix_timestamp(),
            // Riot Client does not persist the account PUUID in this file; the UI resolves it
            // through launcher reauth before saving the backup to an account profile.
            puuid: String::new(),
            unknown: Default::default(),
        },
    })
}

pub fn apply_launcher_session_backup(
    backup: &LauncherSessionBackup,
) -> Result<PathBuf, LauncherSessionError> {
    let target_data_dir = default_restore_data_dir();
    apply_launcher_session_backup_to_dir(backup, &target_data_dir)?;
    Ok(target_data_dir)
}

pub fn apply_launcher_session_backup_to_dir(
    backup: &LauncherSessionBackup,
    target_data_dir: impl AsRef<Path>,
) -> Result<(), LauncherSessionError> {
    if !backup.data_dir.exists() {
        return Err(LauncherSessionError::BackupMissing(backup.data_dir.clone()));
    }

    replace_dir_contents(&backup.data_dir, target_data_dir.as_ref())
}

pub fn sync_current_launcher_session_backup(
    backup: &LauncherSessionBackup,
) -> Result<LauncherSessionBackup, LauncherSessionError> {
    let source_data_dir = ready_launcher_data_dir(default_data_dirs())
        .ok_or(LauncherSessionError::PrivateSettingsNotFound)?;

    sync_launcher_session_backup_from_data_dir(backup, source_data_dir)
}

pub fn sync_launcher_session_backup_from_data_dir(
    backup: &LauncherSessionBackup,
    source_data_dir: impl AsRef<Path>,
) -> Result<LauncherSessionBackup, LauncherSessionError> {
    if !backup.data_dir.exists() {
        return Err(LauncherSessionError::BackupMissing(backup.data_dir.clone()));
    }

    let source_data_dir = source_data_dir.as_ref();
    let settings = read_data_dir_private_settings(source_data_dir)?;
    private_settings_refresh_token(&settings).ok_or(LauncherSessionError::MissingRefreshToken)?;

    // Only save the live login back when it belongs to this account. If Riot rejected the restored
    // login and someone signed in to another account, that login must not land in this backup.
    let signed_in_this_account = private_settings_signed_in_puuid(&settings)
        .is_some_and(|puuid| puuid.eq_ignore_ascii_case(&backup.puuid));
    if !signed_in_this_account {
        return Err(LauncherSessionError::DifferentAccountSignedIn);
    }

    replace_dir_contents(source_data_dir, &backup.data_dir)?;

    Ok(LauncherSessionBackup {
        data_dir: backup.data_dir.clone(),
        captured_at_unix: OffsetDateTime::now_utc().unix_timestamp(),
        puuid: backup.puuid.clone(),
        unknown: Default::default(),
    })
}

/// Saves the live Riot Client login back to whichever saved account it belongs to, so switching away
/// from it does not lose a refresh token Riot rotated since the last capture. Returns the account
/// whose backup was updated, or `None` when the live login does not belong to a saved account.
pub fn sync_signed_in_launcher_session(
    saved_sessions: &[(AccountId, LauncherSessionBackup)],
) -> Result<Option<(AccountId, LauncherSessionBackup)>, LauncherSessionError> {
    let Some(source_data_dir) = ready_launcher_data_dir(default_data_dirs()) else {
        return Ok(None);
    };

    sync_signed_in_launcher_session_from_data_dir(saved_sessions, source_data_dir)
}

pub fn sync_signed_in_launcher_session_from_data_dir(
    saved_sessions: &[(AccountId, LauncherSessionBackup)],
    source_data_dir: impl AsRef<Path>,
) -> Result<Option<(AccountId, LauncherSessionBackup)>, LauncherSessionError> {
    let source_data_dir = source_data_dir.as_ref();
    let settings = read_data_dir_private_settings(source_data_dir)?;
    let Some(puuid) = private_settings_signed_in_puuid(&settings) else {
        return Ok(None);
    };
    let Some((account_id, backup)) = saved_sessions
        .iter()
        .find(|(_, backup)| backup.puuid.eq_ignore_ascii_case(&puuid))
    else {
        return Ok(None);
    };

    let synced = sync_launcher_session_backup_from_data_dir(backup, source_data_dir)?;
    Ok(Some((*account_id, synced)))
}

pub fn read_backup_refresh_token(
    backup: &LauncherSessionBackup,
) -> Result<String, LauncherSessionError> {
    let settings = read_backup_private_settings(backup)?;

    private_settings_refresh_token(&settings).ok_or(LauncherSessionError::MissingRefreshToken)
}

pub fn persist_refreshed_refresh_token(
    backup: &LauncherSessionBackup,
    refresh_token: &str,
) -> Result<(), LauncherSessionError> {
    let settings = read_backup_private_settings(backup)?;
    let updated = update_private_settings_refresh_token(&settings, refresh_token)
        .ok_or(LauncherSessionError::MissingRefreshToken)?;

    if updated != settings {
        let settings_path = backup.data_dir.join(PRIVATE_SETTINGS_FILE);
        fs::write(&settings_path, updated).map_err(|source| {
            LauncherSessionError::WritePrivateSettings {
                path: settings_path,
                source,
            }
        })?;
    }

    Ok(())
}

fn read_backup_private_settings(
    backup: &LauncherSessionBackup,
) -> Result<String, LauncherSessionError> {
    if !backup.data_dir.exists() {
        return Err(LauncherSessionError::BackupMissing(backup.data_dir.clone()));
    }

    let settings_path = backup.data_dir.join(PRIVATE_SETTINGS_FILE);
    if !settings_path.is_file() {
        return Err(LauncherSessionError::BackupPrivateSettingsMissing(
            settings_path,
        ));
    }

    fs::read_to_string(&settings_path).map_err(|source| LauncherSessionError::ReadPrivateSettings {
        path: settings_path,
        source,
    })
}

fn read_data_dir_refresh_token(data_dir: &Path) -> Result<String, LauncherSessionError> {
    let settings = read_data_dir_private_settings(data_dir)?;

    private_settings_refresh_token(&settings).ok_or(LauncherSessionError::MissingRefreshToken)
}

fn read_data_dir_private_settings(data_dir: &Path) -> Result<String, LauncherSessionError> {
    let settings_path = data_dir.join(PRIVATE_SETTINGS_FILE);
    fs::read_to_string(&settings_path).map_err(|source| LauncherSessionError::ReadPrivateSettings {
        path: settings_path,
        source,
    })
}

/// Removes captured backup slots saved by older Riot Client versions, which stored the remembered
/// login as an `ssid` cookie instead of a refresh token. Those sessions can no longer be restored
/// or reauthenticated. Slots in any other unrecognized format are kept, so a future Riot Client
/// format change cannot silently delete every backup. Returns the removed slot directories.
pub fn remove_legacy_launcher_backups(
    backup_root: impl AsRef<Path>,
) -> Result<Vec<PathBuf>, LauncherSessionError> {
    let backup_root = backup_root.as_ref();
    if !backup_root.exists() {
        return Ok(Vec::new());
    }

    let mut removed = Vec::new();
    for entry in fs::read_dir(backup_root)? {
        let slot_dir = entry?.path();
        let settings_path = slot_dir.join("Data").join(PRIVATE_SETTINGS_FILE);
        let Ok(settings) = fs::read_to_string(&settings_path) else {
            continue;
        };

        if private_settings_is_legacy_ssid_login(&settings) {
            fs::remove_dir_all(&slot_dir)?;
            removed.push(slot_dir);
        }
    }

    Ok(removed)
}

/// Removes backup slots that no account profile points at, such as captures abandoned before they
/// were saved. Only folders named like Prime account IDs are touched. Returns the removed slots.
pub fn remove_unreferenced_launcher_backups(
    backup_root: impl AsRef<Path>,
    referenced_data_dirs: &[PathBuf],
) -> Result<Vec<PathBuf>, LauncherSessionError> {
    let backup_root = backup_root.as_ref();
    if !backup_root.exists() {
        return Ok(Vec::new());
    }

    let mut removed = Vec::new();
    for entry in fs::read_dir(backup_root)? {
        let slot_dir = entry?.path();
        let is_account_slot = slot_dir.is_dir()
            && slot_dir
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| uuid::Uuid::parse_str(name).is_ok());
        let is_referenced = referenced_data_dirs
            .iter()
            .any(|data_dir| data_dir.starts_with(&slot_dir));

        if is_account_slot && !is_referenced {
            fs::remove_dir_all(&slot_dir)?;
            removed.push(slot_dir);
        }
    }

    Ok(removed)
}

pub fn clear_existing_launcher_data_dirs() -> Result<usize, LauncherSessionError> {
    let mut cleared = 0;

    for data_dir in default_data_dirs() {
        if data_dir.exists() {
            clear_dir(&data_dir)?;
            cleared += 1;
        }
    }

    Ok(cleared)
}

pub fn default_data_dirs() -> Vec<PathBuf> {
    let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") else {
        return Vec::new();
    };

    let local_app_data = PathBuf::from(local_app_data);

    vec![
        local_app_data
            .join("Riot Games")
            .join("Riot Client")
            .join("Data"),
    ]
}

fn default_restore_data_dir() -> PathBuf {
    default_data_dirs()
        .into_iter()
        .find(|dir| dir.exists())
        .unwrap_or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join("Riot Games")
                .join("Riot Client")
                .join("Data")
        })
}

pub fn remove_launcher_session_backup(
    backup_root: impl AsRef<Path>,
    account_id: AccountId,
) -> Result<(), LauncherSessionError> {
    let backup_slot_dir = backup_root.as_ref().join(account_id.to_string());

    if backup_slot_dir.exists() {
        fs::remove_dir_all(backup_slot_dir)?;
    }

    Ok(())
}

pub fn adopt_launcher_session_backup(
    backup_root: impl AsRef<Path>,
    source_account_id: AccountId,
    target_account_id: AccountId,
    mut backup: LauncherSessionBackup,
) -> Result<LauncherSessionBackup, LauncherSessionError> {
    if source_account_id == target_account_id {
        return Ok(backup);
    }

    let backup_root = backup_root.as_ref();
    let target_data_dir = backup_root.join(target_account_id.to_string()).join("Data");

    replace_dir_contents(&backup.data_dir, &target_data_dir)?;
    remove_launcher_session_backup(backup_root, source_account_id)?;

    backup.data_dir = target_data_dir;
    Ok(backup)
}

#[derive(Debug, Error)]
pub enum LauncherSessionError {
    #[error("Riot Client has no remembered login; sign in with \u{201c}Stay signed in\u{201d} ticked")]
    PrivateSettingsNotFound,
    #[error("failed to read Riot private settings file at {path}: {source}")]
    ReadPrivateSettings { path: PathBuf, source: io::Error },
    #[error("failed to write Riot private settings file at {path}: {source}")]
    WritePrivateSettings { path: PathBuf, source: io::Error },
    #[error("Riot Client has no remembered login; sign in with \u{201c}Stay signed in\u{201d} ticked")]
    MissingRefreshToken,
    #[error(
        "Riot Client is signed in to a different account, or the account could not be confirmed; the saved login was left unchanged"
    )]
    DifferentAccountSignedIn,
    #[error(
        "captured launcher session backup does not exist at {0}; re-capture this account's login"
    )]
    BackupMissing(PathBuf),
    #[error(
        "captured launcher session backup is missing Riot private settings at {0}; re-capture this account's login"
    )]
    BackupPrivateSettingsMissing(PathBuf),
    #[error("source Riot Client data folder does not exist at {0}")]
    SourceDataMissing(PathBuf),
    #[error("launcher session filesystem error: {0}")]
    Io(#[from] io::Error),
}

#[cfg(test)]
mod tests;
