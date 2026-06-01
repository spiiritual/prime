use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc2822;

use crate::account::{AccountId, LauncherSessionBackup};

const PRIVATE_SETTINGS_FILE: &str = "RiotGamesPrivateSettings.yaml";

mod backup_files;
mod cookie_parsing;
mod private_settings;

use backup_files::{clear_dir, replace_dir_contents};
#[cfg(test)]
use cookie_parsing::parse_set_cookie_headers_at;
use cookie_parsing::unquote_yaml_scalar;
pub use cookie_parsing::{cookie_value, parse_private_settings_cookies, parse_set_cookie_headers};
use private_settings::update_private_settings_cookie_values;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LauncherCookie {
    pub name: String,
    pub value: String,
    pub metadata: LauncherCookieMetadata,
}

impl LauncherCookie {
    fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            metadata: LauncherCookieMetadata::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LauncherCookieMetadata {
    pub domain: Option<String>,
    pub path: Option<String>,
    pub persistent: Option<bool>,
    pub expires_at_unix: Option<i64>,
    pub host_only: Option<bool>,
    pub http_only: Option<bool>,
    pub secure_only: Option<bool>,
}

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
    data_dirs.into_iter().find(|dir| {
        let settings_path = dir.join(PRIVATE_SETTINGS_FILE);
        let Ok(settings) = fs::read_to_string(settings_path) else {
            return false;
        };
        let Ok(cookies) = parse_private_settings_cookies(&settings) else {
            return false;
        };

        cookie_value(&cookies, "ssid").is_some()
    })
}

pub fn capture_launcher_session_from_data_dir(
    account_id: AccountId,
    source_data_dir: impl AsRef<Path>,
    backup_root: impl AsRef<Path>,
) -> Result<CapturedLauncherSession, LauncherSessionError> {
    let source_data_dir = source_data_dir.as_ref();
    let settings_path = source_data_dir.join(PRIVATE_SETTINGS_FILE);
    let settings = fs::read_to_string(&settings_path).map_err(|source| {
        LauncherSessionError::ReadPrivateSettings {
            path: settings_path,
            source,
        }
    })?;
    let cookies = parse_private_settings_cookies(&settings)?;

    if cookie_value(&cookies, "ssid").is_none() {
        return Err(LauncherSessionError::MissingSsid);
    }

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
            // Riot Client no longer persists the account PUUID in this file; the UI resolves it
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
    let settings_path = source_data_dir.join(PRIVATE_SETTINGS_FILE);
    let settings = fs::read_to_string(&settings_path).map_err(|source| {
        LauncherSessionError::ReadPrivateSettings {
            path: settings_path,
            source,
        }
    })?;
    let cookies = parse_private_settings_cookies(&settings)?;

    if cookie_value(&cookies, "ssid").is_none() {
        return Err(LauncherSessionError::MissingSsid);
    }

    replace_dir_contents(source_data_dir, &backup.data_dir)?;

    Ok(LauncherSessionBackup {
        data_dir: backup.data_dir.clone(),
        captured_at_unix: OffsetDateTime::now_utc().unix_timestamp(),
        puuid: backup.puuid.clone(),
    })
}

pub fn read_backup_cookies(
    backup: &LauncherSessionBackup,
) -> Result<Vec<LauncherCookie>, LauncherSessionError> {
    if !backup.data_dir.exists() {
        return Err(LauncherSessionError::BackupMissing(backup.data_dir.clone()));
    }

    let settings_path = backup.data_dir.join(PRIVATE_SETTINGS_FILE);
    if !settings_path.is_file() {
        return Err(LauncherSessionError::BackupPrivateSettingsMissing(
            settings_path,
        ));
    }

    let settings = fs::read_to_string(&settings_path).map_err(|source| {
        LauncherSessionError::ReadPrivateSettings {
            path: settings_path,
            source,
        }
    })?;

    parse_private_settings_cookies(&settings)
}

pub fn persist_refreshed_launcher_cookies(
    backup: &LauncherSessionBackup,
    refreshed_cookies: &[LauncherCookie],
) -> Result<(), LauncherSessionError> {
    if !backup.data_dir.exists() {
        return Err(LauncherSessionError::BackupMissing(backup.data_dir.clone()));
    }

    let settings_path = backup.data_dir.join(PRIVATE_SETTINGS_FILE);
    if !settings_path.is_file() {
        return Err(LauncherSessionError::BackupPrivateSettingsMissing(
            settings_path,
        ));
    }

    let settings = fs::read_to_string(&settings_path).map_err(|source| {
        LauncherSessionError::ReadPrivateSettings {
            path: settings_path.clone(),
            source,
        }
    })?;
    let updated = update_private_settings_cookie_values(&settings, refreshed_cookies)?;
    let cookies = parse_private_settings_cookies(&updated)?;
    launcher_cookie_header(&cookies)?;

    if updated != settings {
        fs::write(&settings_path, updated).map_err(|source| {
            LauncherSessionError::WritePrivateSettings {
                path: settings_path,
                source,
            }
        })?;
    }

    Ok(())
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

pub fn launcher_cookie_header(cookies: &[LauncherCookie]) -> Result<String, LauncherSessionError> {
    let usable = cookies
        .iter()
        .filter(|cookie| !cookie.name.trim().is_empty() && !cookie.value.trim().is_empty())
        .map(|cookie| format!("{}={}", cookie.name.trim(), cookie.value.trim()))
        .collect::<Vec<_>>();

    if !usable.iter().any(|cookie| cookie.starts_with("ssid=")) {
        return Err(LauncherSessionError::MissingSsid);
    }

    Ok(usable.join("; "))
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

#[derive(Debug, Error)]
pub enum LauncherSessionError {
    #[error("Riot private settings file was not found in the default Riot Client data folders")]
    PrivateSettingsNotFound,
    #[error("failed to read Riot private settings file at {path}: {source}")]
    ReadPrivateSettings { path: PathBuf, source: io::Error },
    #[error("failed to write Riot private settings file at {path}: {source}")]
    WritePrivateSettings { path: PathBuf, source: io::Error },
    #[error("failed to parse Riot private settings YAML at line {line}: {reason}")]
    PrivateSettingsFormat { line: usize, reason: String },
    #[error("the Riot Client login did not include an ssid cookie; login with Remember Me enabled")]
    MissingSsid,
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

    fn sample_private_settings() -> &'static str {
        r#"
riot-login:
  persist:
    session:
      cookies:
        - domain: "auth.riotgames.com"
          name: "ssid"
          value: "ssid-value"
        - domain: "auth.riotgames.com"
          name: "sub"
          value: "puuid-value"
rso-authenticator:
  tdid:
    name: "tdid"
    value: "tdid-value"
"#
    }

    #[test]
    fn parses_required_launcher_cookies_from_private_settings_yaml() {
        let cookies = parse_private_settings_cookies(sample_private_settings()).expect("cookies");

        assert_eq!(
            cookie_value(&cookies, "ssid").as_deref(),
            Some("ssid-value")
        );
        assert_eq!(
            cookie_value(&cookies, "sub").as_deref(),
            Some("puuid-value")
        );
    }

    #[test]
    fn builds_launcher_cookie_header() {
        let cookies = parse_private_settings_cookies(sample_private_settings()).expect("cookies");

        let header = launcher_cookie_header(&cookies).expect("cookie header");

        assert!(header.contains("ssid=ssid-value"));
        assert!(header.contains("sub=puuid-value"));
    }

    #[test]
    fn parses_set_cookie_headers_as_name_value_pairs() {
        let cookies = parse_set_cookie_headers_at(
            [
                "ssid=new-ssid; Path=/; HttpOnly; Secure; Max-Age=60",
                "empty=; Max-Age=0",
                "__cf_bm=cf-value; Expires=Tue, 30 Jun 2026 19:47:35 GMT; Domain=riotgames.com; Path=/",
                "malformed",
                "=missing-name",
                "ssid=latest-ssid; Secure",
            ],
            1_000,
        );

        assert_eq!(
            cookies,
            vec![
                LauncherCookie {
                    name: "ssid".to_string(),
                    value: "new-ssid".to_string(),
                    metadata: LauncherCookieMetadata {
                        path: Some("/".to_string()),
                        persistent: Some(true),
                        expires_at_unix: Some(1_060),
                        http_only: Some(true),
                        secure_only: Some(true),
                        ..LauncherCookieMetadata::default()
                    }
                },
                LauncherCookie {
                    name: "empty".to_string(),
                    value: String::new(),
                    metadata: LauncherCookieMetadata {
                        persistent: Some(true),
                        expires_at_unix: Some(1_000),
                        ..LauncherCookieMetadata::default()
                    }
                },
                LauncherCookie {
                    name: "__cf_bm".to_string(),
                    value: "cf-value".to_string(),
                    metadata: LauncherCookieMetadata {
                        domain: Some("riotgames.com".to_string()),
                        path: Some("/".to_string()),
                        persistent: Some(true),
                        expires_at_unix: Some(1_782_848_855),
                        host_only: Some(false),
                        ..LauncherCookieMetadata::default()
                    }
                },
                LauncherCookie {
                    name: "ssid".to_string(),
                    value: "latest-ssid".to_string(),
                    metadata: LauncherCookieMetadata {
                        secure_only: Some(true),
                        ..LauncherCookieMetadata::default()
                    }
                }
            ]
        );
    }

    #[test]
    fn updates_private_settings_cookie_values_without_rewriting_unrelated_yaml() {
        let original = concat!(
            "riot-login:\n",
            "  persist:\n",
            "    session:\n",
            "      cookies:\n",
            "        - domain: \"auth.riotgames.com\"\n",
            "          name: \"ssid\"\n",
            "          scope: \"rso\"\n",
            "          value: \"old-ssid\"\n",
            "        - domain: \"auth.riotgames.com\"\n",
            "          name: \"sub\"\n",
            "          value: \"old-sub\"\n",
            "        - domain: \"auth.riotgames.com\"\n",
            "          name: \"clid\"\n",
            "          value: \"old-clid\"\n",
            "rso-authenticator:\n",
            "  tdid:\n",
            "    name: \"ssid\"\n",
            "    value: \"not-a-cookie-list\"\n",
        );
        let refreshed = vec![
            LauncherCookie::new("ssid", "first-ssid"),
            LauncherCookie::new("sub", ""),
            LauncherCookie::new("clid", "new-clid"),
            LauncherCookie::new("missing", "ignored"),
            LauncherCookie::new("ssid", "latest-ssid"),
        ];

        let updated =
            update_private_settings_cookie_values(original, &refreshed).expect("update cookies");

        assert_eq!(
            updated,
            concat!(
                "riot-login:\n",
                "  persist:\n",
                "    session:\n",
                "      cookies:\n",
                "        - domain: \"auth.riotgames.com\"\n",
                "          name: \"ssid\"\n",
                "          scope: \"rso\"\n",
                "          value: \"latest-ssid\"\n",
                "        - domain: \"auth.riotgames.com\"\n",
                "          name: \"sub\"\n",
                "          value: \"old-sub\"\n",
                "        - domain: \"auth.riotgames.com\"\n",
                "          name: \"clid\"\n",
                "          value: \"new-clid\"\n",
                "rso-authenticator:\n",
                "  tdid:\n",
                "    name: \"ssid\"\n",
                "    value: \"not-a-cookie-list\"\n",
            )
        );
    }

    #[test]
    fn updates_cookie_metadata_from_set_cookie_headers() {
        let original = concat!(
            "riot-login:\n",
            "  persist:\n",
            "    session:\n",
            "      cookies:\n",
            "        - domain: \"riotgames.com\"\n",
            "          hostOnly: false\n",
            "          httpOnly: true\n",
            "          name: \"ssid\"\n",
            "          path: \"/\"\n",
            "          persistent: true\n",
            "          secureOnly: true\n",
            "          value: \"old-ssid\"\n",
            "          expiryTime: 100\n",
            "        - domain: \"auth.riotgames.com\"\n",
            "          hostOnly: true\n",
            "          httpOnly: true\n",
            "          name: \"clid\"\n",
            "          path: \"/\"\n",
            "          persistent: true\n",
            "          secureOnly: true\n",
            "          value: \"old-clid\"\n",
        );
        let refreshed = parse_set_cookie_headers_at(
            [
                "ssid=new-ssid; Path=/; Max-Age=2592000; HttpOnly; Secure",
                "clid=new-clid; Path=/; Max-Age=2592000; HttpOnly; Secure",
            ],
            1_000,
        );

        let updated =
            update_private_settings_cookie_values(original, &refreshed).expect("update cookies");

        assert!(updated.contains("value: \"new-ssid\""));
        assert!(updated.contains("value: \"new-clid\""));
        assert!(updated.contains("expiryTime: 2593000"));
        assert!(updated.contains("persistent: true"));
        assert!(updated.contains("httpOnly: true"));
        assert!(updated.contains("secureOnly: true"));
    }

    #[test]
    fn persisting_refreshed_cookies_keeps_missing_ssid_invalid() {
        let dir = tempdir().expect("backup dir");
        fs::write(
            dir.path().join(PRIVATE_SETTINGS_FILE),
            r#"
riot-login:
  persist:
    session:
      cookies:
        - name: "clid"
          value: "old-clid"
"#,
        )
        .expect("settings");
        let backup = LauncherSessionBackup {
            data_dir: dir.path().to_path_buf(),
            captured_at_unix: 100,
            puuid: "puuid-value".to_string(),
        };

        let err =
            persist_refreshed_launcher_cookies(&backup, &[LauncherCookie::new("clid", "new-clid")])
                .expect_err("missing ssid");

        assert!(matches!(err, LauncherSessionError::MissingSsid));
    }

    #[test]
    fn persisting_refreshed_cookies_keeps_settings_when_riot_returned_none() {
        let dir = tempdir().expect("backup dir");
        fs::write(
            dir.path().join(PRIVATE_SETTINGS_FILE),
            sample_private_settings(),
        )
        .expect("settings");
        let backup = LauncherSessionBackup {
            data_dir: dir.path().to_path_buf(),
            captured_at_unix: 100,
            puuid: "puuid-value".to_string(),
        };

        persist_refreshed_launcher_cookies(&backup, &[]).expect("persist unchanged cookies");

        assert_eq!(
            fs::read_to_string(dir.path().join(PRIVATE_SETTINGS_FILE)).expect("settings"),
            sample_private_settings()
        );
    }

    #[test]
    fn updates_cookie_entries_indented_like_riot_client_yaml() {
        let original = concat!(
            "riot-login:\n",
            "    session:\n",
            "        cookies:\n",
            "        -   domain: \"auth.riotgames.com\"\n",
            "            name: \"ssid\"\n",
            "            value: \"old-ssid\"\n",
            "        -   domain: \"auth.riotgames.com\"\n",
            "            name: \"asid\"\n",
            "            value: \"old-asid\"\n",
            "    other: true\n",
        );

        let updated = update_private_settings_cookie_values(
            original,
            &[LauncherCookie::new("ssid", "new-ssid")],
        )
        .expect("update cookies");

        assert!(updated.contains("value: \"new-ssid\""));
        assert!(updated.contains("value: \"old-asid\""));
    }

    #[test]
    fn read_backup_cookies_rejects_missing_backup_folder() {
        let backup = LauncherSessionBackup {
            data_dir: PathBuf::from("missing-launcher-backup"),
            captured_at_unix: 100,
            puuid: "puuid-value".to_string(),
        };

        let err = read_backup_cookies(&backup).expect_err("missing backup");

        assert!(
            matches!(err, LauncherSessionError::BackupMissing(path) if path == backup.data_dir)
        );
    }

    #[test]
    fn read_backup_cookies_rejects_missing_private_settings_file() {
        let dir = tempdir().expect("backup dir");
        let backup = LauncherSessionBackup {
            data_dir: dir.path().to_path_buf(),
            captured_at_unix: 100,
            puuid: "puuid-value".to_string(),
        };

        let err = read_backup_cookies(&backup).expect_err("missing private settings");

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
            r#"
riot-login:
  persist:
    session:
      cookies:
        - name: "ssid"
          value: "ssid-value"
"#,
        )
        .expect("settings");
        fs::create_dir(source.path().join("Config")).expect("nested dir");
        fs::write(source.path().join("Config").join("state.bin"), "state").expect("nested file");

        let captured =
            capture_launcher_session_from_data_dir(account_id, source.path(), backup_root.path())
                .expect("capture");

        assert_eq!(captured.account_id, account_id);
        assert!(captured.backup.puuid.is_empty());
        assert!(
            captured
                .backup
                .data_dir
                .join(PRIVATE_SETTINGS_FILE)
                .exists()
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
    fn ready_launcher_data_dir_requires_remembered_ssid() {
        let first = tempdir().expect("first");
        let second = tempdir().expect("second");
        fs::write(
            first.path().join(PRIVATE_SETTINGS_FILE),
            r#"
riot-login:
  persist:
    session:
      cookies:
        - name: "tdid"
          value: "tdid-value"
"#,
        )
        .expect("first settings");
        fs::write(
            second.path().join(PRIVATE_SETTINGS_FILE),
            r#"
riot-login:
  persist:
    session:
      cookies:
        - name: "ssid"
          value: "ssid-value"
"#,
        )
        .expect("second settings");

        let ready = ready_launcher_data_dir(vec![
            first.path().to_path_buf(),
            second.path().to_path_buf(),
        ])
        .expect("ready data dir");

        assert_eq!(ready, second.path());
    }

    #[test]
    fn rejects_capture_without_remembered_ssid() {
        let account_id = AccountId::new();
        let source = tempdir().expect("source");
        let backup_root = tempdir().expect("backup");
        fs::write(
            source.path().join(PRIVATE_SETTINGS_FILE),
            r#"
riot-login:
  persist:
    session:
      cookies:
        - name: "tdid"
          value: "tdid-value"
"#,
        )
        .expect("settings");

        let err =
            capture_launcher_session_from_data_dir(account_id, source.path(), backup_root.path())
                .expect_err("missing ssid");

        assert!(matches!(err, LauncherSessionError::MissingSsid));
    }

    #[test]
    fn applies_backup_by_replacing_target_data_folder() {
        let backup_source = tempdir().expect("backup source");
        let target = tempdir().expect("target");
        let backup = LauncherSessionBackup {
            data_dir: backup_source.path().to_path_buf(),
            captured_at_unix: 100,
            puuid: "puuid".to_string(),
        };
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
            r#"
riot-login:
  persist:
    session:
      cookies:
        - name: "ssid"
          value: "live-ssid"
        - name: "sub"
          value: "live-puuid"
"#,
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
        assert!(
            fs::read_to_string(backup_dir.path().join(PRIVATE_SETTINGS_FILE))
                .expect("settings")
                .contains("live-ssid")
        );
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
}
