use std::path::PathBuf;
use std::time::Duration;

use time::format_description::well_known::Rfc3339;
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};

use crate::account::{
    AccountId, AccountPenalty, AccountPenaltyDuration, AccountPenaltyStatus, AccountProfile,
    AuthSession, CompetitiveRank, LauncherSessionBackup, Shard, ValorantRegion,
};
use crate::image_cache::ImageCache;
use crate::launch::{
    LaunchConfig, LaunchTargetProcess, close_riot_client_processes, close_riot_processes,
    launch_riot_login_capture, launch_target_window_is_visible, launch_valorant,
    riot_client_window_is_visible,
};
use crate::riot::client::LauncherReauth;
use crate::riot::client::{ApiCredentials, PlayerActivityEndpointPresence, RiotApi};
use crate::riot::content::{
    AccessoryCatalog, BundleCatalog, ContractCatalog, CurrencyCatalog, ResolvedAccessory,
    ResolvedBundle, ResolvedContract, ResolvedContractReward, ResolvedCurrency, ResolvedSkin,
    ResolvedWeapon, SkinCatalog, ValorantContentApi, WeaponCatalog,
};
use crate::riot::launcher_session::{
    CapturedLauncherSession, LauncherSessionError, apply_launcher_session_backup,
    capture_current_launcher_session, clear_existing_launcher_data_dirs, launcher_cookie_header,
    persist_refreshed_launcher_cookies, read_backup_cookies, remove_launcher_session_backup,
    sync_current_launcher_session_backup,
};
use crate::riot::models::{
    AccessoryStoreOffer, BonusStoreOffer, ContractsResponse, GameContentResponse,
    GameContentSeason, MmrSeasonInfo, PlayerContract, PlayerInfoResponse, PlayerLoadoutResponse,
    PlayerMmrResponse, PlayerPenaltiesResponse, StoreBundle, StoreOffer, StorefrontResponse,
    WalletResponse,
};
use crate::storage::StoredState;

use self::session::ApiIdentity;

pub(super) mod account_details;
pub(super) mod game_settings;
pub(super) mod image_assets;
pub(super) mod launch_flow;
pub(super) mod loadout;
pub(super) mod session;
pub(super) mod shop;

fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(super) fn non_empty_path(input: &str) -> Option<PathBuf> {
    let trimmed = input.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

pub(super) fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes_f = bytes as f64;

    if bytes_f >= GB {
        format!("{:.1} GB", bytes_f / GB)
    } else if bytes_f >= MB {
        format!("{:.1} MB", bytes_f / MB)
    } else if bytes_f >= KB {
        format!("{:.1} KB", bytes_f / KB)
    } else {
        format!("{bytes} B")
    }
}

pub(super) fn cache_account_api_context(
    state: &mut StoredState,
    account_id: AccountId,
    session: AuthSession,
    launcher_session: Option<LauncherSessionBackup>,
    identity: ApiIdentity,
) -> Result<(), String> {
    let Some(account) = state
        .accounts
        .iter_mut()
        .find(|account| account.id == account_id)
    else {
        return Err("selected profile no longer exists".to_string());
    };

    account.shard = identity.shard;
    account.session = Some(session);
    if let Some(launcher_session) = launcher_session {
        account.launcher_session = Some(launcher_session);
    }

    match (identity.game_name, identity.tag_line) {
        (Some(game_name), Some(tag_line)) => account
            .apply_riot_identity(identity.puuid, game_name, tag_line)
            .map_err(|error| error.to_string()),
        _ => {
            account.puuid = Some(identity.puuid);
            Ok(())
        }
    }
}
