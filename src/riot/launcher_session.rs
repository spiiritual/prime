use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;
use time::OffsetDateTime;

use crate::account::{AccountId, LauncherSessionBackup};

const PRIVATE_SETTINGS_FILE: &str = "RiotGamesPrivateSettings.yaml";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LauncherCookie {
    pub name: String,
    pub value: String,
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
    let source_data_dir = default_data_dirs()
        .into_iter()
        .find(|dir| dir.join(PRIVATE_SETTINGS_FILE).exists())
        .ok_or(LauncherSessionError::PrivateSettingsNotFound)?;

    capture_launcher_session_from_data_dir(account_id, source_data_dir, backup_root)
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
    let cookies = parse_private_settings_cookies(&settings);

    if cookie_value(&cookies, "ssid").is_none() {
        return Err(LauncherSessionError::MissingSsid);
    }

    let puuid = cookie_value(&cookies, "sub").ok_or(LauncherSessionError::MissingSub)?;
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
            puuid,
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

pub fn parse_private_settings_cookies(contents: &str) -> Vec<LauncherCookie> {
    let mut cookies = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_value: Option<String> = None;

    for line in contents.lines() {
        let trimmed = line.trim();
        let starts_new_entry = trimmed.starts_with("- ");

        if starts_new_entry {
            push_cookie(&mut cookies, &mut current_name, &mut current_value);
        }

        let normalized = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        let Some((key, value)) = normalized.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = unquote_yaml_scalar(value.trim());

        match key {
            "name" => {
                if current_name.is_some() && current_value.is_some() {
                    push_cookie(&mut cookies, &mut current_name, &mut current_value);
                }

                current_name = Some(value);
            }
            "value" => current_value = Some(value),
            _ => {}
        }
    }

    push_cookie(&mut cookies, &mut current_name, &mut current_value);
    cookies
}

pub fn cookie_value(cookies: &[LauncherCookie], name: &str) -> Option<String> {
    cookies
        .iter()
        .find(|cookie| cookie.name.eq_ignore_ascii_case(name))
        .map(|cookie| cookie.value.clone())
}

pub fn default_data_dirs() -> Vec<PathBuf> {
    let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") else {
        return Vec::new();
    };

    let local_app_data = PathBuf::from(local_app_data);

    vec![
        local_app_data.join("Riot Games").join("Beta").join("Data"),
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

fn push_cookie(
    cookies: &mut Vec<LauncherCookie>,
    current_name: &mut Option<String>,
    current_value: &mut Option<String>,
) {
    if let (Some(name), Some(value)) = (current_name.take(), current_value.take()) {
        cookies.push(LauncherCookie { name, value });
    }
}

fn unquote_yaml_scalar(value: &str) -> String {
    let value = value.trim();

    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

fn replace_dir_contents(
    source_dir: impl AsRef<Path>,
    target_dir: impl AsRef<Path>,
) -> Result<(), LauncherSessionError> {
    let source_dir = source_dir.as_ref();
    let target_dir = target_dir.as_ref();

    if !source_dir.exists() {
        return Err(LauncherSessionError::SourceDataMissing(
            source_dir.to_path_buf(),
        ));
    }

    clear_dir(target_dir)?;
    copy_dir_contents(source_dir, target_dir)?;
    Ok(())
}

fn clear_dir(path: &Path) -> Result<(), LauncherSessionError> {
    if !path.exists() {
        fs::create_dir_all(path)?;
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }

    Ok(())
}

fn copy_dir_contents(source_dir: &Path, target_dir: &Path) -> Result<(), LauncherSessionError> {
    fs::create_dir_all(target_dir)?;

    for entry in fs::read_dir(source_dir)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target_dir.join(entry.file_name());

        if source_path.is_dir() {
            copy_dir_contents(&source_path, &target_path)?;
        } else {
            fs::copy(source_path, target_path)?;
        }
    }

    Ok(())
}

#[derive(Debug, Error)]
pub enum LauncherSessionError {
    #[error("Riot private settings file was not found in the default Riot Client data folders")]
    PrivateSettingsNotFound,
    #[error("failed to read Riot private settings file at {path}: {source}")]
    ReadPrivateSettings { path: PathBuf, source: io::Error },
    #[error("the Riot Client login did not include an ssid cookie; login with Remember Me enabled")]
    MissingSsid,
    #[error("the Riot Client login did not include a sub cookie")]
    MissingSub,
    #[error("launcher session backup does not exist at {0}")]
    BackupMissing(PathBuf),
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
        let cookies = parse_private_settings_cookies(sample_private_settings());

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
    fn captures_data_folder_backup_and_puuid() {
        let account_id = AccountId::new();
        let source = tempdir().expect("source");
        let backup_root = tempdir().expect("backup");
        fs::write(
            source.path().join(PRIVATE_SETTINGS_FILE),
            sample_private_settings(),
        )
        .expect("settings");
        fs::create_dir(source.path().join("Config")).expect("nested dir");
        fs::write(source.path().join("Config").join("state.bin"), "state").expect("nested file");

        let captured =
            capture_launcher_session_from_data_dir(account_id, source.path(), backup_root.path())
                .expect("capture");

        assert_eq!(captured.account_id, account_id);
        assert_eq!(captured.backup.puuid, "puuid-value");
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
}
