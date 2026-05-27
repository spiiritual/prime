use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::account::{AccountId, AccountProfile, AccountSessionError, LauncherSessionBackup};

const EXPORT_VERSION: u32 = 1;
const BASE64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const PRIVATE_SETTINGS_FILE: &str = "RiotGamesPrivateSettings.yaml";
const MAX_ENCODED_EXPORT_BYTES: usize = 64 * 1024 * 1024;
const MAX_LAUNCHER_FILE_COUNT: usize = 1024;
const MAX_LAUNCHER_FILE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedAccount {
    pub account: AccountProfile,
    pub original_id: AccountId,
    pub id_changed: bool,
    pub imported_launcher_file_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct AccountExportPackage {
    version: u32,
    account: AccountProfile,
    #[serde(default)]
    launcher_files: Vec<ExportedLauncherFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct ExportedLauncherFile {
    path: String,
    contents: String,
}

pub fn export_account(account: &AccountProfile) -> Result<String, AccountTransferError> {
    let mut account = account.clone();
    let launcher_files = if let Some(backup) = &account.launcher_session {
        if !backup.is_ready() {
            return Err(AccountTransferError::IncompleteLauncherSession(
                backup.data_dir.clone(),
            ));
        }

        collect_exported_files(&backup.data_dir)?
    } else {
        Vec::new()
    };

    if let Some(backup) = &mut account.launcher_session {
        backup.data_dir = PathBuf::from("Data");
    }

    let package = AccountExportPackage {
        version: EXPORT_VERSION,
        account,
        launcher_files,
    };
    let json = serde_json::to_vec(&package)?;

    Ok(base64_encode(&json))
}

pub fn import_account(
    encoded: &str,
    backup_root: impl AsRef<Path>,
    existing_ids: &[AccountId],
) -> Result<ImportedAccount, AccountTransferError> {
    if encoded.trim().is_empty() {
        return Err(AccountTransferError::EmptyInput);
    }

    let encoded_byte_count = encoded
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .count();
    if encoded_byte_count > MAX_ENCODED_EXPORT_BYTES {
        return Err(AccountTransferError::ExportTooLarge {
            actual_bytes: encoded_byte_count,
            max_bytes: MAX_ENCODED_EXPORT_BYTES,
        });
    }

    let json = base64_decode(encoded)?;
    let package: AccountExportPackage = serde_json::from_slice(&json)?;

    if package.version != EXPORT_VERSION {
        return Err(AccountTransferError::UnsupportedVersion(package.version));
    }

    validate_launcher_file_count(package.launcher_files.len())?;

    let backup_root = backup_root.as_ref();
    let mut account = package.account;
    let original_id = account.id;
    let launcher_backup = validate_imported_launcher_session(&account, &package.launcher_files)?;
    let (account_id, id_changed) = unique_import_id(account.id, existing_ids, backup_root);
    account.id = account_id;

    let imported_launcher_file_count = package.launcher_files.len();
    if package.launcher_files.is_empty() {
        account.launcher_session = None;
    } else {
        let account_backup_dir = backup_slot_dir(backup_root, account.id);
        let data_dir = backup_data_dir(backup_root, account.id);
        let staging_dir = staging_backup_slot_dir(backup_root, account.id);
        let staging_data_dir = staging_dir.join("Data");

        remove_dir_if_exists(&staging_dir)?;

        if let Err(error) = write_exported_files(&package.launcher_files, &staging_data_dir) {
            let _ = remove_dir_if_exists(&staging_dir);
            return Err(error);
        }

        let mut backup =
            launcher_backup.ok_or(AccountTransferError::MissingLauncherSessionMetadata)?;
        backup.data_dir = staging_data_dir.clone();

        if !backup.is_ready() {
            let _ = remove_dir_if_exists(&staging_dir);
            return Err(AccountTransferError::IncompleteImportedLauncherSession(
                staging_data_dir,
            ));
        }

        if account_backup_dir.exists() {
            let _ = remove_dir_if_exists(&staging_dir);
            return Err(AccountTransferError::BackupSlotAlreadyExists(
                account_backup_dir,
            ));
        }

        fs::rename(&staging_dir, &account_backup_dir)?;

        backup.data_dir = data_dir;
        if let Err(error) = account.attach_launcher_session(backup) {
            let _ = remove_dir_if_exists(&account_backup_dir);
            return Err(AccountTransferError::InvalidLauncherSession(error));
        }
    }

    Ok(ImportedAccount {
        account,
        original_id,
        id_changed,
        imported_launcher_file_count,
    })
}

fn validate_launcher_file_count(file_count: usize) -> Result<(), AccountTransferError> {
    if file_count > MAX_LAUNCHER_FILE_COUNT {
        return Err(AccountTransferError::TooManyLauncherFiles {
            actual: file_count,
            max: MAX_LAUNCHER_FILE_COUNT,
        });
    }

    Ok(())
}

fn validate_imported_launcher_session(
    account: &AccountProfile,
    launcher_files: &[ExportedLauncherFile],
) -> Result<Option<LauncherSessionBackup>, AccountTransferError> {
    if launcher_files.is_empty() {
        return Ok(None);
    }

    let backup = account
        .launcher_session
        .clone()
        .ok_or(AccountTransferError::MissingLauncherSessionMetadata)?;
    let captured_puuid = backup.puuid.trim();

    if captured_puuid.is_empty() {
        return Err(AccountTransferError::InvalidLauncherSession(
            AccountSessionError::MissingCapturedPuuid,
        ));
    }

    if let Some(existing_puuid) = account
        .puuid
        .as_ref()
        .filter(|puuid| !puuid.trim().is_empty())
        && !existing_puuid.eq_ignore_ascii_case(captured_puuid)
    {
        return Err(AccountTransferError::InvalidLauncherSession(
            AccountSessionError::PuuidMismatch {
                expected: existing_puuid.clone(),
                actual: captured_puuid.to_string(),
            },
        ));
    }

    if !launcher_files
        .iter()
        .any(|file| file.path == PRIVATE_SETTINGS_FILE)
    {
        return Err(AccountTransferError::IncompleteImportedLauncherSession(
            PathBuf::from(PRIVATE_SETTINGS_FILE),
        ));
    }

    Ok(Some(backup))
}

fn collect_exported_files(root: &Path) -> Result<Vec<ExportedLauncherFile>, AccountTransferError> {
    let mut files = Vec::new();
    collect_exported_files_from_dir(root, root, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn collect_exported_files_from_dir(
    root: &Path,
    dir: &Path,
    files: &mut Vec<ExportedLauncherFile>,
) -> Result<(), AccountTransferError> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            collect_exported_files_from_dir(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| AccountTransferError::UnsupportedLauncherPath(path.clone()))?;
            let relative = relative_path_string(relative)?;
            let contents = fs::read(&path)?;

            files.push(ExportedLauncherFile {
                path: relative,
                contents: base64_encode(&contents),
            });
        }
    }

    Ok(())
}

fn write_exported_files(
    files: &[ExportedLauncherFile],
    data_dir: &Path,
) -> Result<(), AccountTransferError> {
    let mut seen = HashSet::new();
    let mut decoded_files = Vec::with_capacity(files.len());
    let mut decoded_byte_count = 0_usize;

    for file in files {
        if !seen.insert(file.path.clone()) {
            return Err(AccountTransferError::DuplicateLauncherPath(
                file.path.clone(),
            ));
        }

        let target = export_file_target(data_dir, &file.path)?;
        let contents = base64_decode(&file.contents)?;
        decoded_byte_count = decoded_byte_count.checked_add(contents.len()).ok_or(
            AccountTransferError::LauncherFilesTooLarge {
                actual_bytes: usize::MAX,
                max_bytes: MAX_LAUNCHER_FILE_BYTES,
            },
        )?;

        if decoded_byte_count > MAX_LAUNCHER_FILE_BYTES {
            return Err(AccountTransferError::LauncherFilesTooLarge {
                actual_bytes: decoded_byte_count,
                max_bytes: MAX_LAUNCHER_FILE_BYTES,
            });
        }

        decoded_files.push((target, contents));
    }

    for (target, contents) in decoded_files {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(target, contents)?;
    }

    Ok(())
}

fn relative_path_string(path: &Path) -> Result<String, AccountTransferError> {
    let mut parts = Vec::new();

    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(
                part.to_str()
                    .ok_or_else(|| {
                        AccountTransferError::UnsupportedLauncherPath(path.to_path_buf())
                    })?
                    .to_string(),
            ),
            _ => {
                return Err(AccountTransferError::UnsupportedLauncherPath(
                    path.to_path_buf(),
                ));
            }
        }
    }

    if parts.is_empty() {
        return Err(AccountTransferError::UnsupportedLauncherPath(
            path.to_path_buf(),
        ));
    }

    Ok(parts.join("/"))
}

fn export_file_target(
    data_dir: &Path,
    relative_path: &str,
) -> Result<PathBuf, AccountTransferError> {
    let path = Path::new(relative_path);
    let mut target = data_dir.to_path_buf();
    let mut has_parts = false;

    for component in path.components() {
        match component {
            Component::Normal(part) => {
                has_parts = true;
                target.push(part);
            }
            _ => {
                return Err(AccountTransferError::UnsupportedLauncherPath(
                    path.to_path_buf(),
                ));
            }
        }
    }

    if !has_parts {
        return Err(AccountTransferError::UnsupportedLauncherPath(
            path.to_path_buf(),
        ));
    }

    Ok(target)
}

fn unique_import_id(
    preferred: AccountId,
    existing_ids: &[AccountId],
    backup_root: &Path,
) -> (AccountId, bool) {
    if !existing_ids.contains(&preferred) && !backup_slot_exists(backup_root, preferred) {
        return (preferred, false);
    }

    loop {
        let candidate = AccountId::new();

        if !existing_ids.contains(&candidate) && !backup_slot_exists(backup_root, candidate) {
            return (candidate, true);
        }
    }
}

fn backup_slot_exists(backup_root: &Path, account_id: AccountId) -> bool {
    backup_slot_dir(backup_root, account_id).exists()
}

fn backup_slot_dir(backup_root: &Path, account_id: AccountId) -> PathBuf {
    backup_root.join(account_id.to_string())
}

fn backup_data_dir(backup_root: &Path, account_id: AccountId) -> PathBuf {
    backup_slot_dir(backup_root, account_id).join("Data")
}

fn staging_backup_slot_dir(backup_root: &Path, account_id: AccountId) -> PathBuf {
    backup_root.join(format!("{account_id}.importing"))
}

fn remove_dir_if_exists(path: &Path) -> Result<(), AccountTransferError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AccountTransferError::Io(error)),
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);

        output.push(BASE64_TABLE[(first >> 2) as usize] as char);
        output.push(BASE64_TABLE[(((first & 0b0000_0011) << 4) | (second >> 4)) as usize] as char);

        if chunk.len() > 1 {
            output.push(
                BASE64_TABLE[(((second & 0b0000_1111) << 2) | (third >> 6)) as usize] as char,
            );
        } else {
            output.push('=');
        }

        if chunk.len() > 2 {
            output.push(BASE64_TABLE[(third & 0b0011_1111) as usize] as char);
        } else {
            output.push('=');
        }
    }

    output
}

fn base64_decode(input: &str) -> Result<Vec<u8>, AccountTransferError> {
    let bytes = input
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();

    if bytes.len() % 4 != 0 {
        return Err(AccountTransferError::InvalidBase64);
    }

    let chunk_count = bytes.len() / 4;
    let mut output = Vec::with_capacity(chunk_count * 3);

    for (index, chunk) in bytes.chunks(4).enumerate() {
        let is_last = index + 1 == chunk_count;
        let padding = match (chunk[2], chunk[3]) {
            (b'=', b'=') => 2,
            (_, b'=') => 1,
            (b'=', _) => return Err(AccountTransferError::InvalidBase64),
            _ => 0,
        };

        if padding > 0 && !is_last {
            return Err(AccountTransferError::InvalidBase64);
        }

        if chunk[0] == b'=' || chunk[1] == b'=' {
            return Err(AccountTransferError::InvalidBase64);
        }

        let first = base64_value(chunk[0]).ok_or(AccountTransferError::InvalidBase64)?;
        let second = base64_value(chunk[1]).ok_or(AccountTransferError::InvalidBase64)?;
        let third = if padding == 2 {
            0
        } else {
            base64_value(chunk[2]).ok_or(AccountTransferError::InvalidBase64)?
        };
        let fourth = if padding > 0 {
            0
        } else {
            base64_value(chunk[3]).ok_or(AccountTransferError::InvalidBase64)?
        };

        output.push((first << 2) | (second >> 4));

        if padding < 2 {
            output.push((second << 4) | (third >> 2));
        }

        if padding == 0 {
            output.push((third << 6) | fourth);
        }
    }

    Ok(output)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[derive(Debug, Error)]
pub enum AccountTransferError {
    #[error("paste an account export before importing")]
    EmptyInput,
    #[error("account export is not valid base64")]
    InvalidBase64,
    #[error("account export JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("account export version {0} is not supported")]
    UnsupportedVersion(u32),
    #[error("account export is too large ({actual_bytes} bytes); maximum is {max_bytes} bytes")]
    ExportTooLarge {
        actual_bytes: usize,
        max_bytes: usize,
    },
    #[error("account export contains too many launcher files ({actual}); maximum is {max}")]
    TooManyLauncherFiles { actual: usize, max: usize },
    #[error(
        "account export launcher session files are too large ({actual_bytes} bytes); maximum is {max_bytes} bytes"
    )]
    LauncherFilesTooLarge {
        actual_bytes: usize,
        max_bytes: usize,
    },
    #[error("launcher session backup is incomplete at {0}; re-capture it before exporting")]
    IncompleteLauncherSession(PathBuf),
    #[error("account export launcher session is incomplete at {0}")]
    IncompleteImportedLauncherSession(PathBuf),
    #[error("launcher session file path is not supported: {0}")]
    UnsupportedLauncherPath(PathBuf),
    #[error("launcher session file appears more than once: {0}")]
    DuplicateLauncherPath(String),
    #[error("account export is missing launcher session metadata")]
    MissingLauncherSessionMetadata,
    #[error("account export launcher session metadata is invalid: {0}")]
    InvalidLauncherSession(AccountSessionError),
    #[error("launcher backup slot already exists at {0}")]
    BackupSlotAlreadyExists(PathBuf),
    #[error("account transfer I/O error: {0}")]
    Io(#[from] io::Error),
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use crate::account::{AuthSession, LauncherSessionBackup, Shard};

    use super::*;

    const PRIVATE_SETTINGS_FILE: &str = "RiotGamesPrivateSettings.yaml";

    #[test]
    fn base64_matches_standard_vectors() {
        for (raw, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(raw.as_bytes()), encoded);
            assert_eq!(base64_decode(encoded).expect("decode"), raw.as_bytes());
        }
    }

    #[test]
    fn base64_decode_accepts_whitespace() {
        assert_eq!(base64_decode(" Zm9v\r\nYmFy ").expect("decode"), b"foobar");
    }

    #[test]
    fn export_import_round_trips_account_and_launcher_files() {
        let backup_source = tempdir().expect("backup source");
        fs::write(
            backup_source.path().join(PRIVATE_SETTINGS_FILE),
            "private settings",
        )
        .expect("private settings");
        fs::create_dir(backup_source.path().join("Config")).expect("config dir");
        fs::write(
            backup_source.path().join("Config").join("state.bin"),
            [0_u8, 1, 2, 3],
        )
        .expect("state file");

        let mut account =
            AccountProfile::new("Main", Some("player".to_string()), Shard::Na).expect("account");
        account.puuid = Some("puuid".to_string());
        account.session = Some(AuthSession::new(
            "access",
            Some("id".to_string()),
            Some("entitlement".to_string()),
            "Bearer",
            Some(3600),
            100,
        ));
        account.launcher_session = Some(LauncherSessionBackup {
            data_dir: backup_source.path().to_path_buf(),
            captured_at_unix: 100,
            puuid: "puuid".to_string(),
        });

        let encoded = export_account(&account).expect("export");
        let import_root = tempdir().expect("import root");

        let imported = import_account(&encoded, import_root.path(), &[]).expect("import");

        assert_eq!(imported.original_id, account.id);
        assert!(!imported.id_changed);
        assert_eq!(imported.imported_launcher_file_count, 2);
        assert_eq!(imported.account.id, account.id);
        assert_eq!(imported.account.display_name, "Main");
        assert_eq!(imported.account.session, account.session);

        let imported_backup = imported
            .account
            .launcher_session
            .as_ref()
            .expect("launcher session");
        assert_eq!(
            imported_backup.data_dir,
            import_root.path().join(account.id.to_string()).join("Data")
        );
        assert_eq!(
            fs::read(imported_backup.data_dir.join("Config").join("state.bin"))
                .expect("state file"),
            [0, 1, 2, 3]
        );
        assert_eq!(
            fs::read_to_string(imported_backup.data_dir.join(PRIVATE_SETTINGS_FILE))
                .expect("settings"),
            "private settings"
        );
    }

    #[test]
    fn import_assigns_new_id_when_exported_id_exists() {
        let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
        let backup_source = tempdir().expect("backup source");
        fs::write(
            backup_source.path().join(PRIVATE_SETTINGS_FILE),
            "private settings",
        )
        .expect("private settings");
        account.launcher_session = Some(LauncherSessionBackup {
            data_dir: backup_source.path().to_path_buf(),
            captured_at_unix: 100,
            puuid: "puuid".to_string(),
        });

        let encoded = export_account(&account).expect("export");
        let import_root = tempdir().expect("import root");
        let imported = import_account(&encoded, import_root.path(), &[account.id]).expect("import");

        assert_ne!(imported.account.id, account.id);
        assert!(imported.id_changed);
        assert_eq!(
            imported
                .account
                .launcher_session
                .as_ref()
                .expect("launcher session")
                .data_dir,
            import_root
                .path()
                .join(imported.account.id.to_string())
                .join("Data")
        );
    }

    #[test]
    fn import_rejects_invalid_base64() {
        let import_root = tempdir().expect("import root");
        let err = import_account("not base64!", import_root.path(), &[]).expect_err("invalid");

        assert!(matches!(err, AccountTransferError::InvalidBase64));
    }

    #[test]
    fn import_rejects_launcher_path_traversal() {
        let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
        account.launcher_session = Some(LauncherSessionBackup {
            data_dir: PathBuf::from("Data"),
            captured_at_unix: 100,
            puuid: "puuid".to_string(),
        });
        let package = AccountExportPackage {
            version: EXPORT_VERSION,
            account,
            launcher_files: vec![
                ExportedLauncherFile {
                    path: PRIVATE_SETTINGS_FILE.to_string(),
                    contents: base64_encode(b"private settings"),
                },
                ExportedLauncherFile {
                    path: "../escape.txt".to_string(),
                    contents: base64_encode(b"nope"),
                },
            ],
        };
        let encoded =
            base64_encode(&serde_json::to_vec(&package).expect("package should serialize"));
        let import_root = tempdir().expect("import root");

        let err = import_account(&encoded, import_root.path(), &[]).expect_err("path traversal");

        assert!(matches!(
            err,
            AccountTransferError::UnsupportedLauncherPath(path)
                if path == PathBuf::from("../escape.txt")
        ));
    }

    #[test]
    fn import_rejects_launcher_files_without_metadata_before_writing() {
        let account = AccountProfile::new("Main", None, Shard::Na).expect("account");
        let account_id = account.id;
        let package = AccountExportPackage {
            version: EXPORT_VERSION,
            account,
            launcher_files: vec![ExportedLauncherFile {
                path: PRIVATE_SETTINGS_FILE.to_string(),
                contents: base64_encode(b"private settings"),
            }],
        };
        let encoded =
            base64_encode(&serde_json::to_vec(&package).expect("package should serialize"));
        let import_root = tempdir().expect("import root");

        let err = import_account(&encoded, import_root.path(), &[]).expect_err("missing metadata");

        assert!(matches!(
            err,
            AccountTransferError::MissingLauncherSessionMetadata
        ));
        assert!(!import_root.path().join(account_id.to_string()).exists());
        assert!(
            !import_root
                .path()
                .join(format!("{account_id}.importing"))
                .exists()
        );
    }

    #[test]
    fn import_rejects_launcher_files_missing_private_settings_before_writing() {
        let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
        let account_id = account.id;
        account.launcher_session = Some(LauncherSessionBackup {
            data_dir: PathBuf::from("Data"),
            captured_at_unix: 100,
            puuid: "puuid".to_string(),
        });
        let package = AccountExportPackage {
            version: EXPORT_VERSION,
            account,
            launcher_files: vec![ExportedLauncherFile {
                path: "Config/state.bin".to_string(),
                contents: base64_encode(b"state"),
            }],
        };
        let encoded =
            base64_encode(&serde_json::to_vec(&package).expect("package should serialize"));
        let import_root = tempdir().expect("import root");

        let err = import_account(&encoded, import_root.path(), &[])
            .expect_err("missing private settings");

        assert!(matches!(
            err,
            AccountTransferError::IncompleteImportedLauncherSession(path)
                if path == PathBuf::from(PRIVATE_SETTINGS_FILE)
        ));
        assert!(!import_root.path().join(account_id.to_string()).exists());
        assert!(
            !import_root
                .path()
                .join(format!("{account_id}.importing"))
                .exists()
        );
    }

    #[test]
    fn import_rejects_too_many_launcher_files() {
        let account = AccountProfile::new("Main", None, Shard::Na).expect("account");
        let package = AccountExportPackage {
            version: EXPORT_VERSION,
            account,
            launcher_files: (0..=MAX_LAUNCHER_FILE_COUNT)
                .map(|index| ExportedLauncherFile {
                    path: format!("file-{index}.bin"),
                    contents: base64_encode(b"x"),
                })
                .collect(),
        };
        let encoded =
            base64_encode(&serde_json::to_vec(&package).expect("package should serialize"));
        let import_root = tempdir().expect("import root");

        let err = import_account(&encoded, import_root.path(), &[]).expect_err("too many files");

        assert!(matches!(
            err,
            AccountTransferError::TooManyLauncherFiles {
                actual,
                max: MAX_LAUNCHER_FILE_COUNT,
            } if actual == MAX_LAUNCHER_FILE_COUNT + 1
        ));
    }

    #[test]
    fn export_rejects_incomplete_launcher_session() {
        let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
        let missing = tempdir().expect("missing");
        account.launcher_session = Some(LauncherSessionBackup {
            data_dir: missing.path().join("Data"),
            captured_at_unix: 100,
            puuid: "puuid".to_string(),
        });

        let err = export_account(&account).expect_err("incomplete launcher session");

        assert!(matches!(
            err,
            AccountTransferError::IncompleteLauncherSession(_)
        ));
    }
}
