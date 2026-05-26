use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::account::{AccountId, AccountProfile};

const STORAGE_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredState {
    pub version: u32,
    #[serde(default)]
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
        ProjectDirs::from("dev", "prime", "prime")
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
        let mut state: StoredState = serde_json::from_str(&contents)?;
        state.version = STORAGE_VERSION;

        if state
            .selected_account
            .is_some_and(|id| !state.accounts.iter().any(|account| account.id == id))
        {
            state.selected_account = state.accounts.first().map(|account| account.id);
        }

        Ok(state)
    }

    pub fn save(&self, state: &StoredState) -> Result<(), StorageError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let pretty = serde_json::to_string_pretty(state)?;
        let tmp = self.path.with_extension("json.tmp");

        fs::write(&tmp, pretty)?;

        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }

        fs::rename(tmp, &self.path)?;

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("storage I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("storage JSON error: {0}")]
    Json(#[from] serde_json::Error),
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
        let account =
            AccountProfile::new("Main", Some("player".to_string()), Shard::Na).expect("account");
        let mut state = StoredState::default();
        let id = account.id;
        state.push_account(account);

        repo.save(&state).expect("save");
        let loaded = repo.load().expect("load");

        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.selected_account, Some(id));
        assert_eq!(loaded.accounts[0].display_name, "Main");
    }

    #[test]
    fn repairs_selected_account_when_profile_is_missing() {
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

        let state = repo.load().expect("load");

        assert_eq!(state.selected_account, Some(state.accounts[0].id));
    }
}
