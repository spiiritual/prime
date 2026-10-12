use super::session::reauth_launcher_backup;
use super::*;

#[derive(Clone, Debug)]
pub(in crate::ui) struct LoadedAccounts {
    pub(in crate::ui) state: StoredState,
    /// Accounts whose launcher session was captured by an older Riot Client and was removed.
    pub(in crate::ui) removed_legacy_sessions: Vec<String>,
    pub(in crate::ui) legacy_cleanup_error: Option<String>,
}

/// Loads saved accounts and removes launcher sessions captured by older Riot Client versions,
/// which can no longer be restored or reauthenticated.
pub(in crate::ui) fn load_accounts(repo: &AccountRepository) -> Result<LoadedAccounts, String> {
    let mut state = repo.load().map_err(|error| error.to_string())?;
    let removed_slots = match remove_legacy_launcher_backups(repo.launcher_backups_dir()) {
        Ok(removed_slots) => removed_slots,
        Err(error) => {
            return Ok(LoadedAccounts {
                state,
                removed_legacy_sessions: Vec::new(),
                legacy_cleanup_error: Some(error.to_string()),
            });
        }
    };

    let mut removed_legacy_sessions = Vec::new();
    for account in &mut state.accounts {
        if account.launcher_session.as_ref().is_some_and(|backup| {
            removed_slots
                .iter()
                .any(|slot| backup.data_dir.starts_with(slot))
        }) {
            account.launcher_session = None;
            removed_legacy_sessions.push(account.summary());
        }
    }

    let mut legacy_cleanup_error = if removed_legacy_sessions.is_empty() {
        None
    } else {
        repo.save(&state).err().map(|error| error.to_string())
    };

    let referenced_data_dirs = state
        .accounts
        .iter()
        .filter_map(|account| account.launcher_session.as_ref())
        .map(|backup| backup.data_dir.clone())
        .collect::<Vec<_>>();
    if let Err(error) =
        remove_unreferenced_launcher_backups(repo.launcher_backups_dir(), &referenced_data_dirs)
    {
        legacy_cleanup_error.get_or_insert(error.to_string());
    }

    Ok(LoadedAccounts {
        state,
        removed_legacy_sessions,
        legacy_cleanup_error,
    })
}

pub(in crate::ui) async fn launch_account(
    riot_client_path: Option<PathBuf>,
    chat_proxy_port: Option<u16>,
    backup: Option<LauncherSessionBackup>,
    saved_sessions: Vec<(AccountId, LauncherSessionBackup)>,
) -> Result<LaunchAccountResult, String> {
    let backup = require_launcher_session(backup)?;

    let previous_sync = prepare_account_launch(
        riot_client_path,
        chat_proxy_port,
        backup.clone(),
        saved_sessions,
    )
    .await?;
    wait_for_valorant_window(VALORANT_OPEN_TIMEOUT, VALORANT_OPEN_POLL_INTERVAL).await?;
    let sync = sync_launcher_session_after_launch(backup).await;

    Ok(LaunchAccountResult {
        previous_account_backup: previous_sync.as_ref().ok().cloned().flatten(),
        previous_account_sync_warning: previous_sync.err(),
        synced_backup: sync.as_ref().ok().cloned(),
        sync_warning: sync.err(),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct LaunchAccountResult {
    /// The account that was signed in before switching, with its backup refreshed from the live login.
    pub(in crate::ui) previous_account_backup: Option<(AccountId, LauncherSessionBackup)>,
    pub(in crate::ui) previous_account_sync_warning: Option<String>,
    pub(in crate::ui) synced_backup: Option<LauncherSessionBackup>,
    pub(in crate::ui) sync_warning: Option<String>,
}

/// The account that was signed in before a switch, with its backup refreshed from the live login,
/// or why that login could not be saved.
pub(in crate::ui) type PreviousAccountSync =
    Result<Option<(AccountId, LauncherSessionBackup)>, String>;

async fn prepare_account_launch(
    riot_client_path: Option<PathBuf>,
    chat_proxy_port: Option<u16>,
    backup: LauncherSessionBackup,
    saved_sessions: Vec<(AccountId, LauncherSessionBackup)>,
) -> Result<PreviousAccountSync, String> {
    tokio::task::spawn_blocking(move || {
        close_riot_processes().map_err(|error| error.to_string())?;
        // Save the outgoing login first; Riot may have rotated its refresh token since capture.
        let previous_sync =
            sync_signed_in_launcher_session(&saved_sessions).map_err(|error| error.to_string());
        apply_launcher_session_backup(&backup).map_err(|error| error.to_string())?;
        launch_valorant(riot_client_path.as_deref(), chat_proxy_port)
            .map_err(|error| error.to_string())?;
        Ok(previous_sync)
    })
    .await
    .map_err(|error| format!("failed to join VALORANT launch preparation task: {error}"))?
}

async fn sync_launcher_session_after_launch(
    backup: LauncherSessionBackup,
) -> Result<LauncherSessionBackup, String> {
    tokio::task::spawn_blocking(move || sync_current_launcher_session_backup(&backup))
        .await
        .map_err(|error| format!("failed to join launcher session backup sync task: {error}"))?
        .map_err(|error| error.to_string())
}

pub(in crate::ui) fn require_launcher_session(
    backup: Option<LauncherSessionBackup>,
) -> Result<LauncherSessionBackup, String> {
    let Some(backup) = backup else {
        return Err(
            "selected account does not have a captured launcher session; start login capture first"
                .to_string(),
        );
    };

    if !backup.is_ready() {
        return Err(
            "selected account launcher session is incomplete, missing Riot private settings, or its backup folder is missing; re-capture selected login"
                .to_string(),
        );
    }

    Ok(backup)
}

pub(in crate::ui) const LOGIN_CAPTURE_TIMEOUT: Duration = Duration::from_secs(600);
pub(in crate::ui) const LOGIN_CAPTURE_POLL_INTERVAL: Duration = Duration::from_secs(2);
pub(in crate::ui) const VALORANT_OPEN_TIMEOUT: Duration = Duration::from_secs(300);
pub(in crate::ui) const VALORANT_OPEN_POLL_INTERVAL: Duration = Duration::from_secs(1);
pub(in crate::ui) const SHOP_RESET_CHECK_INTERVAL: Duration = Duration::from_secs(1);

pub(in crate::ui) async fn wait_for_valorant_window(
    timeout: Duration,
    poll_interval: Duration,
) -> Result<(), String> {
    let started = std::time::Instant::now();

    while started.elapsed() < timeout {
        if check_valorant_window().await? {
            return Ok(());
        }

        tokio::time::sleep(poll_interval).await;
    }

    Err(
        "sent VALORANT launch request to Riot Client, but a visible VALORANT window was not detected"
            .to_string(),
    )
}

async fn check_valorant_window() -> Result<bool, String> {
    tokio::task::spawn_blocking(valorant_window_is_visible)
        .await
        .map_err(|error| format!("failed to join VALORANT window check task: {error}"))?
        .map_err(|error| error.to_string())
}

/// Whether a VALORANT process is running. A failed check counts as not running.
pub(in crate::ui) async fn valorant_is_running() -> bool {
    tokio::task::spawn_blocking(crate::launch::valorant_process_is_running)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or(false)
}

pub(in crate::ui) async fn start_chat_proxy(
    status: crate::riot::chat_proxy::PresenceStatus,
) -> Result<crate::riot::chat_proxy::ChatProxy, String> {
    crate::riot::chat_proxy::ChatProxy::start(status)
        .await
        .map_err(|error| error.to_string())
}

pub(in crate::ui) async fn check_riot_client_window_visible() -> Result<bool, String> {
    tokio::task::spawn_blocking(riot_client_window_is_visible)
        .await
        .map_err(|error| format!("failed to join Riot Client window check task: {error}"))?
        .map_err(|error| error.to_string())
}

/// Closes VALORANT and Riot Client, saves the signed-in account's live login to its backup, clears
/// the live login and reopens Riot Client for a sign-in. Riot may have rotated the signed-in
/// account's refresh token since its last sync, so it is saved before the live login is cleared.
pub(in crate::ui) async fn prepare_login_capture(
    riot_client_path: Option<PathBuf>,
    saved_sessions: Vec<(AccountId, LauncherSessionBackup)>,
) -> Result<PreviousAccountSync, String> {
    tokio::task::spawn_blocking(move || {
        close_riot_processes().map_err(|error| error.to_string())?;
        let previous_sync =
            sync_signed_in_launcher_session(&saved_sessions).map_err(|error| error.to_string());
        clear_existing_launcher_data_dirs().map_err(|error| error.to_string())?;
        launch_riot_login_capture(riot_client_path.as_deref())
            .map_err(|error| error.to_string())?;
        Ok(previous_sync)
    })
    .await
    .map_err(|error| format!("failed to join login capture preparation task: {error}"))?
}

async fn wait_for_login_capture(
    account_id: AccountId,
    backup_root: PathBuf,
) -> Result<CapturedLauncherSession, String> {
    wait_for_launcher_session_capture(
        account_id,
        backup_root,
        LOGIN_CAPTURE_TIMEOUT,
        LOGIN_CAPTURE_POLL_INTERVAL,
    )
    .await
}

/// Waits for the re-capture sign-in after `prepare_login_capture` and resolves whose login it is.
pub(in crate::ui) async fn finish_verified_launcher_session_login(
    account_id: AccountId,
    backup_root: PathBuf,
) -> Result<CapturedLauncherSession, String> {
    let mut captured = wait_for_login_capture(account_id, backup_root.clone()).await?;
    match resolve_captured_launcher_identity(&captured.backup, Shard::default()).await {
        Ok(identity) => {
            captured.backup.puuid = identity.puuid;
            Ok(captured)
        }
        Err(error) => {
            let _ = remove_launcher_session_backup(backup_root, account_id);
            Err(format!(
                "captured remembered login, but Prime could not resolve the account identity: {error}. Try again, or import a fresh redirect token from Settings."
            ))
        }
    }
}

pub(in crate::ui) async fn wait_for_launcher_session_capture(
    account_id: AccountId,
    backup_root: PathBuf,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<CapturedLauncherSession, String> {
    let started = std::time::Instant::now();

    while started.elapsed() < timeout {
        match capture_current_launcher_session(account_id, &backup_root) {
            Ok(captured) => return Ok(captured),
            Err(error) if is_pending_launcher_capture_error(&error) => {
                tokio::time::sleep(poll_interval).await;
            }
            Err(error) => return Err(error.to_string()),
        }
    }

    Err(
        "timed out waiting for the Riot Client login; sign in with \u{201c}Stay signed in\u{201d} ticked"
            .to_string(),
    )
}

/// Riot Client has kept no login yet. Signing in without "Stay signed in" leaves either no
/// settings file or one without a refresh token, and both wait for the user to sign in again.
pub(in crate::ui) fn is_pending_launcher_capture_error(error: &LauncherSessionError) -> bool {
    matches!(
        error,
        LauncherSessionError::PrivateSettingsNotFound | LauncherSessionError::MissingRefreshToken
    )
}

pub(in crate::ui) async fn resolve_session_shard(
    api: &RiotApi,
    session: &AuthSession,
    player_info: Option<&PlayerInfoResponse>,
    fallback: Shard,
) -> Shard {
    if let Some(shard) = player_info.and_then(shard_from_player_affinities) {
        return shard;
    }

    let Some(id_token) = session.id_token.as_ref().filter(|token| !token.is_empty()) else {
        return fallback;
    };

    api.riot_geo(&session.access_token, id_token)
        .await
        .ok()
        .and_then(|geo| Shard::from_live_affinity(&geo.affinities.live))
        .unwrap_or(fallback)
}

pub(in crate::ui) fn shard_from_player_affinities(
    player_info: &PlayerInfoResponse,
) -> Option<Shard> {
    ["live", "pp", "pvp"]
        .into_iter()
        .filter_map(|key| player_info.affinity.get(key))
        .find_map(|value| Shard::from_live_affinity(value))
}

pub(in crate::ui) async fn resolve_session_region(
    api: &RiotApi,
    session: &AuthSession,
    player_info: Option<&PlayerInfoResponse>,
) -> Result<ValorantRegion, String> {
    if let Some(region) = player_info.and_then(region_from_player_affinities) {
        return Ok(region);
    }

    let id_token = session
        .id_token
        .as_ref()
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| {
            "Riot session did not include an ID token for region resolution".to_string()
        })?;

    let geo = api
        .riot_geo(&session.access_token, id_token)
        .await
        .map_err(|error| error.to_string())?;

    ValorantRegion::from_live_affinity(&geo.affinities.live)
        .ok_or_else(|| "Riot Geo did not return a supported VALORANT region".to_string())
}

pub(in crate::ui) fn region_from_player_affinities(
    player_info: &PlayerInfoResponse,
) -> Option<ValorantRegion> {
    ["live", "pp", "pvp"]
        .into_iter()
        .filter_map(|key| player_info.affinity.get(key))
        .find_map(|value| ValorantRegion::from_live_affinity(value))
}

/// Waits for the new account's sign-in after `prepare_login_capture`, then closes Riot Client and
/// resolves the account's identity.
pub(in crate::ui) async fn finish_account_capture(
    account_id: AccountId,
    backup_root: PathBuf,
) -> Result<CapturedAccountDraft, String> {
    let captured = wait_for_login_capture(account_id, backup_root.clone()).await?;
    if let Err(error) = close_riot_client_after_capture().await {
        let _ = remove_launcher_session_backup(backup_root, account_id);
        return Err(error);
    }

    match enrich_captured_account(captured).await {
        Ok(draft) => Ok(draft),
        Err(error) => {
            let _ = remove_launcher_session_backup(backup_root, account_id);
            Err(error)
        }
    }
}

pub(in crate::ui) async fn start_current_account_capture(
    account_id: AccountId,
    backup_root: PathBuf,
) -> Result<CapturedAccountDraft, String> {
    let backup_root_for_capture = backup_root.clone();
    let captured = tokio::task::spawn_blocking(move || {
        capture_current_launcher_session(account_id, &backup_root_for_capture)
    })
    .await
    .map_err(|error| format!("failed to join current launcher session capture task: {error}"))?
    .map_err(current_account_capture_error)?;

    match enrich_captured_account(captured).await {
        Ok(draft) => Ok(draft),
        Err(error) => {
            let _ = remove_launcher_session_backup(backup_root, account_id);
            Err(error)
        }
    }
}

fn current_account_capture_error(error: LauncherSessionError) -> String {
    match error {
        LauncherSessionError::PrivateSettingsNotFound
        | LauncherSessionError::MissingRefreshToken => {
            "Riot Client is not signed in with \u{201c}Stay signed in\u{201d} ticked".to_string()
        }
        error => error.to_string(),
    }
}

async fn close_riot_client_after_capture() -> Result<(), String> {
    tokio::task::spawn_blocking(close_riot_client_processes)
        .await
        .map_err(|error| format!("failed to join Riot Client close task: {error}"))?
        .map_err(|error| error.to_string())
}

pub(in crate::ui) async fn enrich_captured_account(
    captured: CapturedLauncherSession,
) -> Result<CapturedAccountDraft, String> {
    let mut draft = CapturedAccountDraft::new(captured.account_id, captured.backup);
    enrich_captured_account_identity(&mut draft)
        .await
        .map_err(|error| {
            format!(
                "captured remembered login, but Prime could not resolve the account identity: {error}. Try again, or import a fresh redirect token from Settings."
            )
        })?;
    Ok(draft)
}

pub(in crate::ui) async fn enrich_captured_account_identity(
    draft: &mut CapturedAccountDraft,
) -> Result<(), String> {
    let identity = resolve_captured_launcher_identity(&draft.backup, draft.shard).await?;

    draft.puuid = identity.puuid.clone();
    draft.backup.puuid = identity.puuid;
    draft.game_name = Some(identity.game_name);
    draft.tag_line = Some(identity.tag_line);
    draft.shard = identity.shard;
    draft.session = Some(identity.session);
    Ok(())
}

async fn resolve_captured_launcher_identity(
    backup: &LauncherSessionBackup,
    fallback_shard: Shard,
) -> Result<CapturedLauncherIdentity, String> {
    let api = RiotApi::shared().map_err(|error| error.to_string())?;
    let mut session = reauth_launcher_backup(&api, backup).await?.session;
    let player_info = api
        .player_info(&session.access_token)
        .await
        .map_err(|error| error.to_string())?;
    let puuid = player_info.sub.trim().to_string();

    if puuid.is_empty() {
        return Err("Riot player info did not include a PUUID".to_string());
    }

    let shard = resolve_session_shard(&api, &session, Some(&player_info), fallback_shard).await;

    if session
        .entitlements_token
        .as_ref()
        .is_none_or(|token| token.trim().is_empty())
        && let Ok(entitlement) = api.entitlement(&session.access_token).await
    {
        session.entitlements_token = Some(entitlement.entitlements_token);
    }

    Ok(CapturedLauncherIdentity {
        session,
        puuid,
        game_name: player_info.acct.game_name,
        tag_line: player_info.acct.tag_line,
        shard,
    })
}

struct CapturedLauncherIdentity {
    session: AuthSession,
    puuid: String,
    game_name: String,
    tag_line: String,
    shard: Shard,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct CapturedAccountDraft {
    pub(in crate::ui) account_id: AccountId,
    pub(in crate::ui) backup: LauncherSessionBackup,
    pub(in crate::ui) puuid: String,
    pub(in crate::ui) game_name: Option<String>,
    pub(in crate::ui) tag_line: Option<String>,
    pub(in crate::ui) shard: Shard,
    pub(in crate::ui) session: Option<AuthSession>,
}

impl CapturedAccountDraft {
    pub(in crate::ui) fn new(account_id: AccountId, backup: LauncherSessionBackup) -> Self {
        let puuid = backup.puuid.clone();

        Self {
            account_id,
            backup,
            puuid,
            game_name: None,
            tag_line: None,
            shard: Shard::default(),
            session: None,
        }
    }

    pub(in crate::ui) fn riot_id(&self) -> Option<String> {
        match (&self.game_name, &self.tag_line) {
            (Some(game_name), Some(tag_line)) if !game_name.is_empty() && !tag_line.is_empty() => {
                Some(format!("{game_name}#{tag_line}"))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
pub(in crate::ui) mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::tempdir;

    use crate::account::{AccountId, AccountProfile, LauncherSessionBackup, Shard};
    use crate::riot::launcher_session::LauncherSessionError;
    use crate::storage::{AccountRepository, StoredState};

    #[test]
    fn require_launcher_session_rejects_missing_backup() {
        let err = require_launcher_session(None).expect_err("missing backup");

        assert!(err.contains("captured launcher session"));
    }

    #[test]
    fn require_launcher_session_rejects_missing_backup_folder() {
        let err = require_launcher_session(Some(LauncherSessionBackup {
            data_dir: PathBuf::from("missing-launcher-backup"),
            captured_at_unix: 100,
            puuid: "puuid".to_string(),
            unknown: Default::default(),
        }))
        .expect_err("missing backup folder");

        assert!(err.contains("backup folder is missing"));
    }

    #[test]
    fn require_launcher_session_rejects_missing_private_settings_file() {
        let dir = tempdir().expect("temp dir");
        let err = require_launcher_session(Some(LauncherSessionBackup {
            data_dir: dir.path().to_path_buf(),
            captured_at_unix: 100,
            puuid: "puuid".to_string(),
            unknown: Default::default(),
        }))
        .expect_err("missing private settings");

        assert!(err.contains("missing Riot private settings"));
    }

    #[test]
    fn require_launcher_session_accepts_ready_backup() {
        let dir = tempdir().expect("temp dir");
        fs::write(dir.path().join("RiotGamesPrivateSettings.yaml"), "settings")
            .expect("private settings");
        let backup = LauncherSessionBackup {
            data_dir: dir.path().to_path_buf(),
            captured_at_unix: 100,
            puuid: "puuid".to_string(),
            unknown: Default::default(),
        };

        let accepted = require_launcher_session(Some(backup)).expect("ready backup");

        assert_eq!(accepted.puuid, "puuid");
    }

    #[test]
    fn loading_accounts_removes_unreferenced_backup_slots() {
        let dir = tempdir().expect("temp dir");
        let repo = AccountRepository::new(dir.path().join("accounts.json"));
        let backup_root = repo.launcher_backups_dir();
        let account = account_with_backup(
            &backup_root,
            "Current",
            "psl:
        authorization:
            riot-client:
                refresh_token: \"refresh-value\"
    ",
        );
        let orphaned_slot = backup_root.join(AccountId::new().to_string());
        fs::create_dir_all(orphaned_slot.join("Data")).expect("orphaned slot");
        let mut state = StoredState::default();
        state.push_account(account.clone());
        repo.save(&state).expect("save state");

        let loaded = load_accounts(&repo).expect("load accounts");

        assert!(!orphaned_slot.exists());
        assert!(backup_root.join(account.id.to_string()).exists());
        assert_eq!(
            loaded.state.accounts[0].launcher_session,
            account.launcher_session
        );
    }

    #[test]
    fn loading_accounts_removes_legacy_launcher_sessions() {
        let dir = tempdir().expect("temp dir");
        let repo = AccountRepository::new(dir.path().join("accounts.json"));
        let backup_root = repo.launcher_backups_dir();
        let legacy = account_with_backup(
            &backup_root,
            "Legacy",
            "riot-login:
      persist:
        session:
          cookies:
            - name: \"ssid\"
              value: \"ssid-value\"
    ",
        );
        let current = account_with_backup(
            &backup_root,
            "Current",
            "psl:
        authorization:
            riot-client:
                refresh_token: \"refresh-value\"
    ",
        );
        let legacy_slot = backup_root.join(legacy.id.to_string());
        let orphaned_legacy_slot = backup_root.join(AccountId::new().to_string());
        fs::create_dir_all(orphaned_legacy_slot.join("Data")).expect("orphaned slot");
        fs::write(
            orphaned_legacy_slot
                .join("Data")
                .join("RiotGamesPrivateSettings.yaml"),
            "riot-login:
      persist:
        session:
          cookies:
            - name: \"ssid\"
              value: \"old\"
    ",
        )
        .expect("orphaned settings");
        let mut state = StoredState::default();
        state.push_account(legacy.clone());
        state.push_account(current.clone());
        repo.save(&state).expect("save state");

        let loaded = load_accounts(&repo).expect("load accounts");

        assert_eq!(loaded.removed_legacy_sessions, vec![legacy.summary()]);
        assert_eq!(loaded.legacy_cleanup_error, None);
        assert!(!legacy_slot.exists());
        assert!(!orphaned_legacy_slot.exists());
        let find = |state: &StoredState, id| {
            state
                .accounts
                .iter()
                .find(|account| account.id == id)
                .cloned()
                .expect("account")
        };
        assert_eq!(find(&loaded.state, legacy.id).launcher_session, None);
        assert_eq!(
            find(&loaded.state, current.id).launcher_session,
            current.launcher_session
        );
        let saved = repo.load().expect("reload saved state");
        assert_eq!(find(&saved, legacy.id).launcher_session, None);
    }

    #[test]
    fn a_login_capture_waits_while_riot_client_kept_no_login() {
        assert!(is_pending_launcher_capture_error(
            &LauncherSessionError::PrivateSettingsNotFound
        ));
        assert!(is_pending_launcher_capture_error(
            &LauncherSessionError::MissingRefreshToken
        ));
        assert!(!is_pending_launcher_capture_error(
            &LauncherSessionError::DifferentAccountSignedIn
        ));
    }

    pub(in crate::ui) fn account_with_backup(
        backup_root: &Path,
        name: &str,
        settings: &str,
    ) -> AccountProfile {
        let mut account = AccountProfile::new(name, Shard::Na).expect("account");
        let data_dir = backup_root.join(account.id.to_string()).join("Data");
        fs::create_dir_all(&data_dir).expect("backup data dir");
        fs::write(data_dir.join("RiotGamesPrivateSettings.yaml"), settings).expect("settings");
        account
            .attach_launcher_session(LauncherSessionBackup {
                data_dir,
                captured_at_unix: 100,
                puuid: format!("{name}-puuid"),
                unknown: Default::default(),
            })
            .expect("attach backup");
        account
    }
}
