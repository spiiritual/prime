use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;
use time::OffsetDateTime;

use crate::account::{AccountId, LauncherSessionBackup};

const PRIVATE_SETTINGS_FILE: &str = "RiotGamesPrivateSettings.yaml";

mod backup_files;
mod private_settings;

use backup_files::{clear_dir, replace_dir_contents};
pub use private_settings::private_settings_refresh_token;
use private_settings::{
    private_settings_is_legacy_ssid_login, update_private_settings_refresh_token,
};

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
    read_data_dir_refresh_token(source_data_dir)?;
    replace_dir_contents(source_data_dir, &backup.data_dir)?;

    Ok(LauncherSessionBackup {
        data_dir: backup.data_dir.clone(),
        captured_at_unix: OffsetDateTime::now_utc().unix_timestamp(),
        puuid: backup.puuid.clone(),
    })
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
    let settings_path = data_dir.join(PRIVATE_SETTINGS_FILE);
    let settings = fs::read_to_string(&settings_path).map_err(|source| {
        LauncherSessionError::ReadPrivateSettings {
            path: settings_path,
            source,
        }
    })?;

    private_settings_refresh_token(&settings).ok_or(LauncherSessionError::MissingRefreshToken)
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
            clear_launcher_data_dir(&data_dir)?;
            cleared += 1;
        }
    }

    Ok(cleared)
}

pub fn clear_launcher_data_dir(data_dir: impl AsRef<Path>) -> Result<(), LauncherSessionError> {
    clear_dir(data_dir.as_ref())
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
    #[error("Riot private settings file was not found in the default Riot Client data folders")]
    PrivateSettingsNotFound,
    #[error("failed to read Riot private settings file at {path}: {source}")]
    ReadPrivateSettings { path: PathBuf, source: io::Error },
    #[error("failed to write Riot private settings file at {path}: {source}")]
    WritePrivateSettings { path: PathBuf, source: io::Error },
    #[error("Riot Client did not save a remembered login; log in with Stay signed in enabled")]
    MissingRefreshToken,
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
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn remembered_login_settings(refresh_token: &str) -> String {
        format!(
            concat!(
                "psl:\n",
                "    authorization:\n",
                "        riot-client:\n",
                "            claims: []\n",
                "            id_token: \"id-token\"\n",
                "            is_dpop_bound: false\n",
                "            refresh_token: \"{}\"\n",
                "            refresh_token_write_count: 1\n",
                "            scopes:\n",
                "            - \"openid\"\n",
                "            - \"account\"\n",
                "riot-login:\n",
                "    persist: null\n",
                "rso-authenticator:\n",
                "    tdid:\n",
                "        name: \"tdid\"\n",
                "        value: \"tdid-value\"\n",
            ),
            refresh_token
        )
    }

    fn legacy_ssid_settings() -> &'static str {
        r#"
riot-login:
  persist:
    session:
      cookies:
        - domain: "auth.riotgames.com"
          name: "ssid"
          value: "ssid-value"
rso-authenticator:
  tdid:
    name: "tdid"
    value: "tdid-value"
"#
    }

    fn backup_at(data_dir: &Path) -> LauncherSessionBackup {
        LauncherSessionBackup {
            data_dir: data_dir.to_path_buf(),
            captured_at_unix: 100,
            puuid: "puuid-value".to_string(),
        }
    }

    #[test]
    fn reads_refresh_token_from_psl_riot_client_authorization() {
        assert_eq!(
            private_settings_refresh_token(&remembered_login_settings("refresh-value")).as_deref(),
            Some("refresh-value")
        );
    }

    #[test]
    fn refresh_token_requires_psl_riot_client_path() {
        let settings = concat!(
            "psl:\n",
            "    authorization:\n",
            "        other-client:\n",
            "            refresh_token: \"other\"\n",
            "refresh_token: \"top-level\"\n",
        );

        assert_eq!(private_settings_refresh_token(settings), None);
        assert_eq!(private_settings_refresh_token(legacy_ssid_settings()), None);
    }

    #[test]
    fn refresh_token_supports_crlf_line_endings() {
        let settings = remembered_login_settings("refresh-value").replace('\n', "\r\n");

        assert_eq!(
            private_settings_refresh_token(&settings).as_deref(),
            Some("refresh-value")
        );
    }

    #[test]
    fn updates_refresh_token_without_rewriting_unrelated_yaml() {
        let original = remembered_login_settings("old-refresh").replace('\n', "\r\n");

        let updated =
            update_private_settings_refresh_token(&original, "new-refresh").expect("update token");

        assert_eq!(
            updated,
            remembered_login_settings("new-refresh").replace('\n', "\r\n")
        );
    }

    #[test]
    fn persists_refreshed_refresh_token_to_backup() {
        let dir = tempdir().expect("backup dir");
        fs::write(
            dir.path().join(PRIVATE_SETTINGS_FILE),
            remembered_login_settings("old-refresh"),
        )
        .expect("settings");
        let backup = backup_at(dir.path());

        persist_refreshed_refresh_token(&backup, "new-refresh").expect("persist token");

        assert_eq!(
            read_backup_refresh_token(&backup).expect("refresh token"),
            "new-refresh"
        );
    }

    #[test]
    fn read_backup_refresh_token_rejects_legacy_ssid_backup() {
        let dir = tempdir().expect("backup dir");
        fs::write(
            dir.path().join(PRIVATE_SETTINGS_FILE),
            legacy_ssid_settings(),
        )
        .expect("settings");

        let err = read_backup_refresh_token(&backup_at(dir.path())).expect_err("legacy backup");

        assert!(matches!(err, LauncherSessionError::MissingRefreshToken));
    }

    #[test]
    fn read_backup_refresh_token_rejects_missing_backup_folder() {
        let backup = backup_at(Path::new("missing-launcher-backup"));

        let err = read_backup_refresh_token(&backup).expect_err("missing backup");

        assert!(
            matches!(err, LauncherSessionError::BackupMissing(path) if path == backup.data_dir)
        );
    }

    #[test]
    fn read_backup_refresh_token_rejects_missing_private_settings_file() {
        let dir = tempdir().expect("backup dir");
        let backup = backup_at(dir.path());

        let err = read_backup_refresh_token(&backup).expect_err("missing private settings");

        assert!(
            matches!(err, LauncherSessionError::BackupPrivateSettingsMissing(path) if path == backup.data_dir.join(PRIVATE_SETTINGS_FILE))
        );
    }

    #[test]
    fn clears_launcher_data_dir_without_removing_directory() {
        let data_dir = tempdir().expect("data dir");
        fs::write(data_dir.path().join("old.txt"), "old").expect("old file");
        fs::create_dir(data_dir.path().join("nested")).expect("nested dir");
        fs::write(data_dir.path().join("nested").join("old.txt"), "old").expect("nested file");

        clear_launcher_data_dir(data_dir.path()).expect("clear");

        assert!(data_dir.path().exists());
        assert_eq!(fs::read_dir(data_dir.path()).expect("read dir").count(), 0);
    }

    #[test]
    fn captures_remembered_login_data_folder_backup() {
        let account_id = AccountId::new();
        let source = tempdir().expect("source");
        let backup_root = tempdir().expect("backup");
        fs::write(
            source.path().join(PRIVATE_SETTINGS_FILE),
            remembered_login_settings("refresh-value"),
        )
        .expect("settings");
        fs::create_dir(source.path().join("Config")).expect("nested dir");
        fs::write(source.path().join("Config").join("state.bin"), "state").expect("nested file");

        let captured =
            capture_launcher_session_from_data_dir(account_id, source.path(), backup_root.path())
                .expect("capture");

        assert_eq!(captured.account_id, account_id);
        assert!(captured.backup.puuid.is_empty());
        assert_eq!(
            read_backup_refresh_token(&captured.backup).expect("refresh token"),
            "refresh-value"
        );
        assert!(
            captured
                .backup
                .data_dir
                .join("Config")
                .join("state.bin")
                .exists()
        );
    }

    #[test]
    fn ready_launcher_data_dir_requires_remembered_refresh_token() {
        let legacy = tempdir().expect("legacy");
        let signed_out = tempdir().expect("signed out");
        let remembered = tempdir().expect("remembered");
        fs::write(
            legacy.path().join(PRIVATE_SETTINGS_FILE),
            legacy_ssid_settings(),
        )
        .expect("legacy settings");
        fs::write(
            signed_out.path().join(PRIVATE_SETTINGS_FILE),
            "riot-login:\n    persist: null\n",
        )
        .expect("signed out settings");
        fs::write(
            remembered.path().join(PRIVATE_SETTINGS_FILE),
            remembered_login_settings("refresh-value"),
        )
        .expect("remembered settings");

        let ready = ready_launcher_data_dir(vec![
            legacy.path().to_path_buf(),
            signed_out.path().to_path_buf(),
            remembered.path().to_path_buf(),
        ])
        .expect("ready data dir");

        assert_eq!(ready, remembered.path());
    }

    #[test]
    fn rejects_capture_without_remembered_refresh_token() {
        let account_id = AccountId::new();
        let source = tempdir().expect("source");
        let backup_root = tempdir().expect("backup");
        fs::write(
            source.path().join(PRIVATE_SETTINGS_FILE),
            legacy_ssid_settings(),
        )
        .expect("settings");

        let err =
            capture_launcher_session_from_data_dir(account_id, source.path(), backup_root.path())
                .expect_err("missing refresh token");

        assert!(matches!(err, LauncherSessionError::MissingRefreshToken));
        assert!(!backup_root.path().join(account_id.to_string()).exists());
    }

    #[test]
    fn applies_backup_by_replacing_target_data_folder() {
        let backup_source = tempdir().expect("backup source");
        let target = tempdir().expect("target");
        let backup = backup_at(backup_source.path());
        fs::write(backup_source.path().join(PRIVATE_SETTINGS_FILE), "new").expect("backup file");
        fs::write(target.path().join("old.txt"), "old").expect("old file");

        apply_launcher_session_backup_to_dir(&backup, target.path()).expect("apply");

        assert!(!target.path().join("old.txt").exists());
        assert_eq!(
            fs::read_to_string(target.path().join(PRIVATE_SETTINGS_FILE)).expect("new file"),
            "new"
        );
    }

    #[test]
    fn syncs_live_launcher_data_back_to_backup() {
        let source = tempdir().expect("source");
        let backup_dir = tempdir().expect("backup");
        fs::write(
            source.path().join(PRIVATE_SETTINGS_FILE),
            remembered_login_settings("live-refresh"),
        )
        .expect("settings");
        fs::create_dir(source.path().join("Config")).expect("source nested dir");
        fs::write(source.path().join("Config").join("state.bin"), "live")
            .expect("source nested file");
        fs::write(backup_dir.path().join(PRIVATE_SETTINGS_FILE), "old").expect("old settings");
        fs::write(backup_dir.path().join("old.txt"), "old").expect("old file");
        let backup = LauncherSessionBackup {
            data_dir: backup_dir.path().to_path_buf(),
            captured_at_unix: 100,
            puuid: "account-puuid".to_string(),
        };

        let synced = sync_launcher_session_backup_from_data_dir(&backup, source.path())
            .expect("sync backup");

        assert_eq!(synced.data_dir, backup.data_dir);
        assert_eq!(synced.puuid, "account-puuid");
        assert!(synced.captured_at_unix >= backup.captured_at_unix);
        assert!(!backup_dir.path().join("old.txt").exists());
        assert_eq!(
            fs::read_to_string(backup_dir.path().join("Config").join("state.bin"))
                .expect("nested file"),
            "live"
        );
        assert_eq!(
            read_backup_refresh_token(&synced).expect("refresh token"),
            "live-refresh"
        );
    }

    #[test]
    fn removes_legacy_launcher_backups_only() {
        let backup_root = tempdir().expect("backup root");
        let legacy_slot = backup_root.path().join(AccountId::new().to_string());
        let current_slot = backup_root.path().join(AccountId::new().to_string());
        let unrelated_slot = backup_root.path().join("unrelated");
        let unknown_format_slot = backup_root.path().join(AccountId::new().to_string());
        fs::create_dir_all(unknown_format_slot.join("Data")).expect("unknown format slot");
        fs::write(
            unknown_format_slot.join("Data").join(PRIVATE_SETTINGS_FILE),
            "future-login:
    session: \"opaque\"
",
        )
        .expect("unknown format settings");
        fs::create_dir_all(legacy_slot.join("Data")).expect("legacy slot");
        fs::create_dir_all(current_slot.join("Data")).expect("current slot");
        fs::create_dir_all(&unrelated_slot).expect("unrelated slot");
        fs::write(
            legacy_slot.join("Data").join(PRIVATE_SETTINGS_FILE),
            legacy_ssid_settings(),
        )
        .expect("legacy settings");
        fs::write(
            current_slot.join("Data").join(PRIVATE_SETTINGS_FILE),
            remembered_login_settings("refresh-value"),
        )
        .expect("current settings");

        let removed = remove_legacy_launcher_backups(backup_root.path()).expect("remove legacy");

        assert_eq!(removed, vec![legacy_slot.clone()]);
        assert!(!legacy_slot.exists());
        assert!(current_slot.exists());
        assert!(unrelated_slot.exists());
        assert!(unknown_format_slot.exists());
    }

    #[test]
    fn removes_backup_slots_no_account_references() {
        let backup_root = tempdir().expect("backup root");
        let referenced_slot = backup_root.path().join(AccountId::new().to_string());
        let orphaned_slot = backup_root.path().join(AccountId::new().to_string());
        let unrelated_dir = backup_root.path().join("keep-me");
        for dir in [&referenced_slot, &orphaned_slot, &unrelated_dir] {
            fs::create_dir_all(dir.join("Data")).expect("slot");
        }

        let removed = remove_unreferenced_launcher_backups(
            backup_root.path(),
            &[referenced_slot.join("Data")],
        )
        .expect("remove unreferenced");

        assert_eq!(removed, vec![orphaned_slot.clone()]);
        assert!(referenced_slot.exists());
        assert!(!orphaned_slot.exists());
        assert!(unrelated_dir.exists());
    }

    #[test]
    fn removing_legacy_backups_ignores_missing_backup_root() {
        let removed = remove_legacy_launcher_backups(Path::new("missing-launcher-backups"))
            .expect("missing root");

        assert!(removed.is_empty());
    }

    #[test]
    fn removes_captured_launcher_backup_slot() {
        let backup_root = tempdir().expect("backup root");
        let account_id = AccountId::new();
        let slot = backup_root.path().join(account_id.to_string());
        fs::create_dir_all(slot.join("Data")).expect("slot");
        fs::write(slot.join("Data").join(PRIVATE_SETTINGS_FILE), "settings").expect("settings");

        remove_launcher_session_backup(backup_root.path(), account_id).expect("remove backup");

        assert!(!slot.exists());
    }

    #[test]
    fn adopts_captured_launcher_backup_into_existing_slot() {
        let backup_root = tempdir().expect("backup root");
        let source_id = AccountId::new();
        let target_id = AccountId::new();
        let source_data = backup_root.path().join(source_id.to_string()).join("Data");
        let target_slot = backup_root.path().join(target_id.to_string());
        let target_data = target_slot.join("Data");
        fs::create_dir_all(&source_data).expect("source data");
        fs::create_dir_all(&target_data).expect("target data");
        fs::write(source_data.join(PRIVATE_SETTINGS_FILE), "new").expect("source settings");
        fs::write(target_data.join("old.txt"), "old").expect("old target file");
        let backup = backup_at(&source_data);

        let adopted =
            adopt_launcher_session_backup(backup_root.path(), source_id, target_id, backup)
                .expect("adopt backup");

        assert_eq!(adopted.data_dir, target_data);
        assert_eq!(adopted.puuid, "puuid-value");
        assert!(!backup_root.path().join(source_id.to_string()).exists());
        assert_eq!(
            fs::read_to_string(adopted.data_dir.join(PRIVATE_SETTINGS_FILE)).expect("settings"),
            "new"
        );
        assert!(!adopted.data_dir.join("old.txt").exists());
    }
}
