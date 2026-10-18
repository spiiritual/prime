use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::account::{AccountId, AccountProfile};
use crate::aim_trainer::AimTrainerState;
use crate::riot::chat_proxy::PresenceStatus;

const STORAGE_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredState {
    pub version: u32,
    pub accounts: Vec<AccountProfile>,
    pub selected_account: Option<AccountId>,
    pub riot_client_path: Option<PathBuf>,
    /// Closing the window minimizes Prime so it keeps refreshing sessions in the background.
    /// On unless the user turns it off in Settings; only the off choice is written.
    #[serde(
        default = "minimize_on_close_default",
        skip_serializing_if = "Clone::clone"
    )]
    pub minimize_on_close: bool,
    /// The weapon IDs whose skins Live Match shows, in column order. `None` is the default set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_match_weapons: Option<Vec<String>>,
    /// What friends see while VALORANT runs through Prime's chat proxy, and what the next launch
    /// starts with. Online, the default, isn't written.
    #[serde(default, skip_serializing_if = "PresenceStatus::is_online")]
    pub presence_status: PresenceStatus,
    /// Whether launches go through the chat proxy, which the status control needs. On, the
    /// default, isn't written.
    #[serde(default = "chat_proxy_default", skip_serializing_if = "Clone::clone")]
    pub chat_proxy: bool,
    /// The Aim Trainer's settings and best runs. Left out until one is set.
    #[serde(default, skip_serializing_if = "AimTrainerState::is_default")]
    pub aim_trainer: AimTrainerState,
    /// The experiments a beta tester switched off in Settings, by name; beta starts with every
    /// one on. A name this build doesn't know is kept and ignored.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub disabled_experiments: BTreeSet<String>,
    #[serde(flatten)]
    pub unknown: UnknownFields,
}

/// Fields a newer Prime wrote that this build doesn't know, such as a beta's settings. They're
/// kept and written back as they were, so going back a version loses nothing. `Debug` shows only
/// their names, since a newer build may keep tokens in them.
#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnknownFields(serde_json::Map<String, serde_json::Value>);

impl std::fmt::Debug for UnknownFields {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.0.keys()).finish()
    }
}

/// The experiments in this build, as `(name, Settings label)`. Beta starts with each on and shows
/// a switch for it; empty while nothing is being tried.
pub const EXPERIMENTS: &[(&str, &str)] = &[];

fn minimize_on_close_default() -> bool {
    true
}

fn chat_proxy_default() -> bool {
    true
}

impl Default for StoredState {
    fn default() -> Self {
        Self {
            version: STORAGE_VERSION,
            accounts: Vec::new(),
            selected_account: None,
            riot_client_path: None,
            minimize_on_close: minimize_on_close_default(),
            live_match_weapons: None,
            presence_status: PresenceStatus::default(),
            chat_proxy: chat_proxy_default(),
            aim_trainer: AimTrainerState::default(),
            disabled_experiments: BTreeSet::new(),
            unknown: UnknownFields::default(),
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
    saves: Arc<SaveSequence>,
}

/// Orders saves by when their state was captured, so concurrent save tasks cannot interleave
/// their writes or leave an older state on disk.
#[derive(Debug, Default)]
struct SaveSequence {
    next_generation: AtomicU64,
    last_written_generation: Mutex<u64>,
}

/// Accounts state captured for saving, tagged with the order in which it was captured.
#[derive(Clone, Debug)]
pub struct SaveSnapshot {
    generation: u64,
    pub state: StoredState,
}

impl AccountRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            saves: Arc::default(),
        }
    }

    pub fn default_path() -> PathBuf {
        std::env::var_os("APPDATA")
            .map(|dir| PathBuf::from(dir).join(r"spiiritual\prime\config\accounts.json"))
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

    pub fn settings_profiles_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(|parent| parent.join("settings-profiles"))
            .unwrap_or_else(|| PathBuf::from("settings-profiles"))
    }

    pub fn load(&self) -> Result<StoredState, StorageError> {
        if !self.path.exists() {
            return Ok(StoredState::default());
        }

        let contents = fs::read_to_string(&self.path)?;
        let contents = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
        let state: StoredState = serde_json::from_str(contents)?;

        self.validate_state(&state)?;

        Ok(state)
    }

    pub fn save(&self, state: &StoredState) -> Result<(), StorageError> {
        self.save_snapshot(self.snapshot(state))
    }

    /// Captures `state` for a later [`save_snapshot`](Self::save_snapshot) call.
    pub fn snapshot(&self, state: &StoredState) -> SaveSnapshot {
        SaveSnapshot {
            generation: self.saves.next_generation.fetch_add(1, Ordering::SeqCst) + 1,
            state: state.clone(),
        }
    }

    /// Writes a snapshot unless a newer one has already been written.
    pub fn save_snapshot(&self, snapshot: SaveSnapshot) -> Result<(), StorageError> {
        let mut last_written = self
            .saves
            .last_written_generation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);

        if snapshot.generation <= *last_written {
            return Ok(());
        }

        self.write_state(&snapshot.state)?;
        *last_written = snapshot.generation;
        Ok(())
    }

    fn write_state(&self, state: &StoredState) -> Result<(), StorageError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let pretty = serde_json::to_string_pretty(state)?;
        // Most saves change nothing, and reading the file back is far cheaper than a synced write.
        if fs::read(&self.path).is_ok_and(|saved| saved == pretty.as_bytes()) {
            return Ok(());
        }
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

pub(crate) fn write_synced(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(contents)?;
    file.sync_all()
}

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
mod tests;
