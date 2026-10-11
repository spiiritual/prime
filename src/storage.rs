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
#[serde(deny_unknown_fields)]
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
    /// The Aim Trainer's sensitivity, DPI and best run. Left out until one is set.
    #[serde(default, skip_serializing_if = "AimTrainerState::is_default")]
    pub aim_trainer: AimTrainerState,
}

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
mod tests {
    use tempfile::tempdir;

    use crate::account::{
        AccountPenaltyDuration, AccountPenaltyStatus, AccountProfile, CompetitiveRank, Shard,
    };

    use super::*;

    #[test]
    fn accounts_live_in_roaming_app_data() {
        let expected = Path::new(&std::env::var_os("APPDATA").expect("APPDATA"))
            .join("spiiritual")
            .join("prime")
            .join("config")
            .join("accounts.json");

        assert_eq!(AccountRepository::default_path(), expected);
    }

    #[test]
    fn older_save_snapshot_never_overwrites_a_newer_one() {
        let dir = tempdir().expect("temp dir");
        let repo = AccountRepository::new(dir.path().join("accounts.json"));
        let mut older = StoredState::default();
        older.push_account(AccountProfile::new("Older", Shard::Na).expect("account"));
        let mut newer = older.clone();
        newer.push_account(AccountProfile::new("Newer", Shard::Na).expect("account"));
        let older_snapshot = repo.snapshot(&older);
        let newer_snapshot = repo.snapshot(&newer);

        repo.save_snapshot(newer_snapshot).expect("save newer");
        repo.save_snapshot(older_snapshot).expect("skip older");

        assert_eq!(repo.load().expect("load"), newer);
    }

    #[test]
    fn saving_unchanged_state_leaves_the_file_alone() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("accounts.json");
        let repo = AccountRepository::new(&path);
        let mut state = StoredState::default();
        state.push_account(AccountProfile::new("Main", Shard::Na).expect("account"));
        repo.save(&state).expect("first save");
        let written = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(written))
            .expect("age file");
        let modified = || {
            fs::metadata(&path)
                .and_then(|m| m.modified())
                .expect("mtime")
        };

        repo.save(&state).expect("unchanged save");
        assert_eq!(modified(), written);

        state.push_account(AccountProfile::new("Alt", Shard::Na).expect("account"));
        repo.save(&state).expect("changed save");
        assert_ne!(modified(), written);
        assert_eq!(repo.load().expect("load"), state);
    }

    #[test]
    fn concurrent_saves_do_not_fail() {
        let dir = tempdir().expect("temp dir");
        let repo = AccountRepository::new(dir.path().join("accounts.json"));
        let snapshots = (0..16)
            .map(|index| {
                let mut state = StoredState::default();
                for account in 0..=index {
                    state.push_account(
                        AccountProfile::new(format!("Account {account}"), Shard::Na)
                            .expect("account"),
                    );
                }
                repo.snapshot(&state)
            })
            .collect::<Vec<_>>();
        let newest = snapshots.last().expect("snapshot").state.clone();

        std::thread::scope(|scope| {
            for snapshot in snapshots {
                let repo = repo.clone();
                scope.spawn(move || repo.save_snapshot(snapshot).expect("save"));
            }
        });

        assert_eq!(repo.load().expect("load"), newest);
    }

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
        let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
        account.account_level = Some(123);
        let mut state = StoredState::default();
        let id = account.id;
        state.push_account(account);

        repo.save(&state).expect("save");
        let loaded = repo.load().expect("load");

        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.selected_account, Some(id));
        assert_eq!(loaded.accounts[0].display_name, "Main");
        assert_eq!(loaded.accounts[0].account_level, Some(123));
    }

    #[test]
    fn minimize_on_close_defaults_on_and_only_off_is_saved() {
        let dir = tempdir().expect("temp dir");
        let repo = AccountRepository::new(dir.path().join("accounts.json"));
        let mut state = StoredState::default();
        assert!(state.minimize_on_close);

        repo.save(&state).expect("save");
        let saved = fs::read_to_string(repo.path()).expect("read");
        assert!(!saved.contains("minimize_on_close"));
        assert!(repo.load().expect("load").minimize_on_close);

        state.minimize_on_close = false;
        repo.save(&state).expect("save");
        assert!(!repo.load().expect("load").minimize_on_close);
    }

    #[test]
    fn the_chat_proxy_defaults_on_and_only_off_is_saved() {
        let dir = tempdir().expect("temp dir");
        let repo = AccountRepository::new(dir.path().join("accounts.json"));
        let mut state = StoredState::default();
        assert!(state.chat_proxy);

        repo.save(&state).expect("save");
        let saved = fs::read_to_string(repo.path()).expect("read");
        assert!(!saved.contains("chat_proxy"), "{saved}");
        assert!(repo.load().expect("load").chat_proxy);

        state.chat_proxy = false;
        repo.save(&state).expect("save");
        assert!(!repo.load().expect("load").chat_proxy);
    }

    #[test]
    fn presence_status_defaults_to_online_and_only_others_are_saved() {
        let dir = tempdir().expect("temp dir");
        let repo = AccountRepository::new(dir.path().join("accounts.json"));
        let mut state = StoredState::default();
        assert_eq!(
            state.presence_status,
            crate::riot::chat_proxy::PresenceStatus::Online
        );

        repo.save(&state).expect("save");
        let saved = fs::read_to_string(repo.path()).expect("read");
        assert!(!saved.contains("presence_status"), "{saved}");

        state.presence_status = crate::riot::chat_proxy::PresenceStatus::Invisible;
        repo.save(&state).expect("save");
        let saved = fs::read_to_string(repo.path()).expect("read");
        assert!(saved.contains("invisible"), "{saved}");
        assert_eq!(
            repo.load().expect("load").presence_status,
            crate::riot::chat_proxy::PresenceStatus::Invisible
        );
    }

    #[test]
    fn aim_trainer_state_is_only_saved_once_set() {
        let dir = tempdir().expect("temp dir");
        let repo = AccountRepository::new(dir.path().join("accounts.json"));
        let mut state = StoredState::default();

        repo.save(&state).expect("save");
        let saved = fs::read_to_string(repo.path()).expect("read");
        assert!(!saved.contains("aim_trainer"), "{saved}");

        state.aim_trainer.sensitivity = Some(0.34);
        state.aim_trainer.dpi = Some(1600);
        state.aim_trainer.best = Some(crate::aim_trainer::AimRun {
            score: 64,
            misses: 6,
            bursts: 4,
            avg_reaction_ms: Some(412),
            duration_ms: 72_000,
            sensitivity: 0.34,
            dpi: 1600,
            finished_at_unix: 1_791_331_200,
        });
        repo.save(&state).expect("save");
        assert_eq!(repo.load().expect("load").aim_trainer, state.aim_trainer);
    }

    #[test]
    fn legacy_fields_load_and_are_dropped_on_save() {
        let account = AccountProfile::new("Main", Shard::Na).expect("account");
        let raw = serde_json::json!({
            "version": 1,
            "accounts": [{
                "id": account.id,
                "display_name": "Main",
                "username": "player",
                "puuid": null,
                "game_name": null,
                "tag_line": null,
                "shard": "na",
                "session": null,
                "launcher_session": null,
                "competitive_rank": {
                    "tier": 15,
                    "rank_name": "Gold 1",
                    "ranked_rating": 42,
                    "season_id": "season-a"
                },
                "account_level": null,
                "last_refreshed_at_unix": 1_800_000_000
            }],
            "selected_account": account.id,
            "riot_client_path": null
        });
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("accounts.json");
        fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).expect("write");
        let repo = AccountRepository::new(path);

        let loaded = repo.load().expect("load");
        repo.save(&loaded).expect("save");
        let saved = fs::read_to_string(repo.path()).expect("saved json");

        assert_eq!(
            loaded.accounts[0]
                .competitive_rank
                .as_ref()
                .map(CompetitiveRank::label)
                .as_deref(),
            Some("Gold 1 - 42 RR")
        );
        for key in ["username", "season_id", "last_refreshed_at_unix"] {
            assert!(!saved.contains(key), "{key} in {saved}");
        }
    }

    #[test]
    fn save_drops_runtime_penalty_status() {
        let dir = tempdir().expect("temp dir");
        let repo = AccountRepository::new(dir.path().join("accounts.json"));
        let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
        account.penalty_status = AccountPenaltyStatus::penalized_for(
            Some("Premier comms".to_string()),
            AccountPenaltyDuration::default(),
        );
        let mut state = StoredState::default();
        state.push_account(account);

        repo.save(&state).expect("save");
        let saved = fs::read_to_string(repo.path()).expect("saved json");

        assert!(!saved.contains("penalty_status"));
        assert!(!saved.contains("Premier comms"));
    }

    #[test]
    fn load_ignores_cached_penalty_status() {
        let account = AccountProfile::new("Main", Shard::Na).expect("account");
        let raw = serde_json::json!({
            "version": 1,
            "accounts": [{
                "id": account.id,
                "display_name": "Main",
                "username": "player",
                "puuid": null,
                "game_name": null,
                "tag_line": null,
                "shard": "na",
                "session": null,
                "launcher_session": null,
                "competitive_rank": null,
                "penalty_status": {
                    "status": "penalized",
                    "penalties": [{
                        "rating_name": "Premier comms",
                        "duration": {
                            "ends_at_unix": null,
                            "games_remaining": null
                        }
                    }]
                },
                "account_level": null
            }],
            "selected_account": account.id,
            "riot_client_path": null
        });
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("accounts.json");
        fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).expect("write");
        let repo = AccountRepository::new(path);

        let loaded = repo.load().expect("load");

        assert_eq!(
            loaded.accounts[0].penalty_status,
            AccountPenaltyStatus::Unchecked
        );
    }

    #[test]
    fn load_accepts_utf8_bom() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("accounts.json");
        let account = AccountProfile::new("Main", Shard::Na).expect("account");
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
    fn rejects_legacy_accounts_with_region_and_notes() {
        let account = AccountProfile::new("Main", Shard::Na).expect("account");
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
        let repo = AccountRepository::new(path);

        let error = repo.load().expect_err("legacy state is rejected");

        match error {
            StorageError::Json(error) => {
                let message = error.to_string();
                assert!(message.contains("unknown field"), "{message}");
            }
            other => panic!("expected JSON schema error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_selected_account_when_profile_is_missing() {
        let first = AccountProfile::new("First", Shard::Na).expect("first");
        let missing = AccountProfile::new("Missing", Shard::Na).expect("missing");
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
