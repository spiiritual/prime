use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::account::{
    AccountId, AccountProfile, AuthSession, CompetitiveRank, LauncherSessionBackup, Shard,
};

const STORAGE_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredState {
    pub version: u32,
    pub accounts: Vec<AccountProfile>,
    pub selected_account: Option<AccountId>,
    pub riot_client_path: Option<PathBuf>,
}

impl Default for StoredState {
    fn default() -> Self {
        Self {
            version: STORAGE_VERSION,
            accounts: Vec::new(),
            selected_account: None,
            riot_client_path: None,
        }
    }
}

impl StoredState {
    pub fn selected_account(&self) -> Option<&AccountProfile> {
        let selected = self.selected_account?;
        self.accounts.iter().find(|account| account.id == selected)
    }

    pub fn selected_account_mut(&mut self) -> Option<&mut AccountProfile> {
        let selected = self.selected_account?;
        self.accounts
            .iter_mut()
            .find(|account| account.id == selected)
    }

    pub fn select_account(&mut self, id: AccountId) -> bool {
        if !self.account_exists(id) {
            return false;
        }

        self.selected_account = Some(id);
        true
    }

    pub fn push_account(&mut self, account: AccountProfile) {
        if self.selected_account.is_none() {
            self.selected_account = Some(account.id);
        }

        self.accounts.push(account);
    }

    pub fn remove_account(&mut self, id: AccountId) {
        self.accounts.retain(|account| account.id != id);

        if self.selected_account == Some(id) {
            self.selected_account = self.accounts.first().map(|account| account.id);
        }
    }

    fn account_exists(&self, id: AccountId) -> bool {
        self.accounts.iter().any(|account| account.id == id)
    }
}

#[derive(Clone, Debug)]
pub struct AccountRepository {
    path: PathBuf,
}

impl AccountRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_path() -> PathBuf {
        ProjectDirs::from("dev", "spiiritual", "prime")
            .map(|dirs| dirs.config_dir().join("accounts.json"))
            .unwrap_or_else(|| PathBuf::from("accounts.json"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn launcher_backups_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(|parent| parent.join("launcher-backups"))
            .unwrap_or_else(|| PathBuf::from("launcher-backups"))
    }

    pub fn load(&self) -> Result<StoredState, StorageError> {
        if !self.path.exists() {
            return Ok(StoredState::default());
        }

        let contents = fs::read_to_string(&self.path)?;
        let contents = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
        let state: StoredState = match serde_json::from_str(contents) {
            Ok(state) => state,
            Err(error) => {
                let state = migrate_legacy_state(contents).map_err(|_| error)?;
                self.validate_state(&state)?;
                self.save(&state)?;
                return Ok(state);
            }
        };

        self.validate_state(&state)?;

        Ok(state)
    }

    pub fn save(&self, state: &StoredState) -> Result<(), StorageError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let pretty = serde_json::to_string_pretty(state)?;
        let tmp = self.path.with_extension("json.tmp");

        write_synced(&tmp, pretty.as_bytes())?;
        replace_file_atomically(&tmp, &self.path)?;

        Ok(())
    }

    fn validate_state(&self, state: &StoredState) -> Result<(), StorageError> {
        if state.version != STORAGE_VERSION {
            return Err(StorageError::UnsupportedVersion(state.version));
        }

        if let Some(selected) = state.selected_account
            && !state.accounts.iter().any(|account| account.id == selected)
        {
            return Err(StorageError::InvalidSelectedAccount(selected));
        }

        Ok(())
    }
}

// Temporary 0.1.x migration for pre-hard-cut account JSON that still contains
// `region` and `notes`. Canonical storage remains `StoredState`; delete this in
// the next product version after users have had one release to re-save profiles.
// Tracking: user-requested follow-up removal in the next version.
fn migrate_legacy_state(contents: &str) -> Result<StoredState, serde_json::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct LegacyStoredState {
        version: u32,
        #[serde(default)]
        accounts: Vec<LegacyAccountProfile>,
        selected_account: Option<AccountId>,
        riot_client_path: Option<PathBuf>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct LegacyAccountProfile {
        id: AccountId,
        display_name: String,
        username: Option<String>,
        puuid: Option<String>,
        game_name: Option<String>,
        tag_line: Option<String>,
        #[serde(default)]
        region: Option<LegacyRegion>,
        shard: Shard,
        session: Option<AuthSession>,
        #[serde(default)]
        launcher_session: Option<LauncherSessionBackup>,
        #[serde(default)]
        competitive_rank: Option<CompetitiveRank>,
        #[serde(default)]
        account_level: Option<i64>,
        #[serde(default)]
        last_refreshed_at_unix: Option<i64>,
        #[serde(default)]
        notes: String,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum LegacyRegion {
        Na,
        Latam,
        Br,
        Eu,
        Ap,
        Kr,
        Pbe,
    }

    let legacy: LegacyStoredState = serde_json::from_str(contents)?;

    Ok(StoredState {
        version: legacy.version,
        accounts: legacy
            .accounts
            .into_iter()
            .map(|account| {
                let _ = (account.region, account.notes);

                AccountProfile {
                    id: account.id,
                    display_name: account.display_name,
                    username: account.username,
                    puuid: account.puuid,
                    game_name: account.game_name,
                    tag_line: account.tag_line,
                    shard: account.shard,
                    session: account.session,
                    launcher_session: account.launcher_session,
                    competitive_rank: account.competitive_rank,
                    account_level: account.account_level,
                    last_refreshed_at_unix: account.last_refreshed_at_unix,
                }
            })
            .collect(),
        selected_account: legacy.selected_account,
        riot_client_path: legacy.riot_client_path,
    })
}

fn write_synced(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(contents)?;
    file.sync_all()
}

#[cfg(windows)]
fn replace_file_atomically(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };

    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file_atomically(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("storage I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("storage JSON/schema error: {0}; delete accounts.json and re-add accounts")]
    Json(#[from] serde_json::Error),
    #[error("unsupported storage version {0}; delete accounts.json and re-add accounts")]
    UnsupportedVersion(u32),
    #[error(
        "selected account {0} does not exist in storage; delete accounts.json and re-add accounts"
    )]
    InvalidSelectedAccount(AccountId),
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::account::{AccountProfile, Shard};

    use super::*;

    #[test]
    fn missing_file_loads_default_state() {
        let dir = tempdir().expect("temp dir");
        let repo = AccountRepository::new(dir.path().join("missing.json"));

        let state = repo.load().expect("default state");

        assert_eq!(state, StoredState::default());
    }

    #[test]
    fn round_trips_accounts() {
        let dir = tempdir().expect("temp dir");
        let repo = AccountRepository::new(dir.path().join("accounts.json"));
        let mut account =
            AccountProfile::new("Main", Some("player".to_string()), Shard::Na).expect("account");
        account.account_level = Some(123);
        account.last_refreshed_at_unix = Some(1_800_000_000);
        let mut state = StoredState::default();
        let id = account.id;
        state.push_account(account);

        repo.save(&state).expect("save");
        let loaded = repo.load().expect("load");

        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.selected_account, Some(id));
        assert_eq!(loaded.accounts[0].display_name, "Main");
        assert_eq!(loaded.accounts[0].account_level, Some(123));
        assert_eq!(
            loaded.accounts[0].last_refreshed_at_unix,
            Some(1_800_000_000)
        );
    }

    #[test]
    fn load_accepts_utf8_bom() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("accounts.json");
        let account = AccountProfile::new("Main", None, Shard::Na).expect("account");
        let mut state = StoredState::default();
        state.push_account(account);
        let json = serde_json::to_string_pretty(&state).expect("state json");
        fs::write(&path, format!("\u{feff}{json}")).expect("write bom json");
        let repo = AccountRepository::new(path);

        let loaded = repo.load().expect("load");

        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].display_name, "Main");
    }

    #[test]
    fn migrates_legacy_accounts_with_region_and_notes() {
        let account =
            AccountProfile::new("Main", Some("player".to_string()), Shard::Na).expect("account");
        let raw = serde_json::json!({
            "version": 1,
            "accounts": [{
                "id": account.id,
                "display_name": "Main",
                "username": "player",
                "puuid": "puuid-a",
                "game_name": "Player",
                "tag_line": "NA1",
                "region": "latam",
                "shard": "na",
                "session": null,
                "launcher_session": null,
                "competitive_rank": {
                    "tier": 15,
                    "rank_name": "Gold 1",
                    "ranked_rating": 42
                },
                "account_level": 123,
                "notes": "old local notes"
            }],
            "selected_account": account.id,
            "riot_client_path": null
        });
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("accounts.json");
        fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).expect("write");
        let repo = AccountRepository::new(path.clone());

        let loaded = repo.load().expect("migrated state");
        let migrated_json = fs::read_to_string(path).expect("migrated json");

        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.selected_account, Some(account.id));
        assert_eq!(loaded.accounts[0].display_name, "Main");
        assert_eq!(loaded.accounts[0].riot_id().as_deref(), Some("Player#NA1"));
        assert_eq!(loaded.accounts[0].last_refreshed_at_unix, None);
        assert_eq!(
            loaded.accounts[0]
                .competitive_rank
                .as_ref()
                .unwrap()
                .season_id,
            None
        );
        assert!(!migrated_json.contains("\"region\""));
        assert!(!migrated_json.contains("\"notes\""));
    }

    #[test]
    fn rejects_selected_account_when_profile_is_missing() {
        let first = AccountProfile::new("First", None, Shard::Na).expect("first");
        let missing = AccountProfile::new("Missing", None, Shard::Na).expect("missing");
        let raw = serde_json::json!({
            "version": 1,
            "accounts": [first],
            "selected_account": missing.id,
            "riot_client_path": null
        });
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("accounts.json");
        fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).expect("write");
        let repo = AccountRepository::new(path);

        let error = repo.load().expect_err("invalid selected account");

        assert!(matches!(
            error,
            StorageError::InvalidSelectedAccount(id) if id == missing.id
        ));
    }

    #[test]
    fn rejects_unsupported_storage_version() {
        let raw = serde_json::json!({
            "version": 0,
            "accounts": [],
            "selected_account": null,
            "riot_client_path": null
        });
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("accounts.json");
        fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).expect("write");
        let repo = AccountRepository::new(path);

        let error = repo.load().expect_err("unsupported version");

        assert!(matches!(error, StorageError::UnsupportedVersion(0)));
    }
}
