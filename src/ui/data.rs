use std::path::PathBuf;
use std::time::Duration;

use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};

use crate::account::{
    AccountId, AccountProfile, AuthSession, CompetitiveRank, LauncherSessionBackup, Shard,
};
use crate::image_cache::ImageCache;
use crate::launch::{
    LaunchConfig, LaunchTargetProcess, close_riot_client_processes, close_riot_processes,
    launch_riot_login_capture, launch_target_window_is_visible, launch_valorant,
    riot_client_window_is_visible,
};
use crate::riot::client::{ApiCredentials, RiotApi};
use crate::riot::content::{
    AccessoryCatalog, BundleCatalog, ContractCatalog, CurrencyCatalog, ResolvedAccessory,
    ResolvedBundle, ResolvedContract, ResolvedContractReward, ResolvedCurrency, ResolvedSkin,
    ResolvedWeapon, SkinCatalog, ValorantContentApi, WeaponCatalog,
};
use crate::riot::launcher_session::{
    CapturedLauncherSession, LauncherSessionError, apply_launcher_session_backup,
    capture_current_launcher_session, clear_existing_launcher_data_dirs, launcher_cookie_header,
    read_backup_cookies, remove_launcher_session_backup,
};
use crate::riot::models::{
    AccessoryStoreOffer, BonusStoreOffer, ContractsResponse, GameContentResponse,
    GameContentSeason, MmrSeasonInfo, PlayerContract, PlayerInfoResponse, PlayerLoadoutResponse,
    PlayerMmrResponse, StoreBundle, StoreOffer, StorefrontResponse, WalletResponse,
};
use crate::storage::StoredState;

pub(super) async fn launch_account(
    config: LaunchConfig,
    backup: Option<LauncherSessionBackup>,
) -> Result<LaunchTargetProcess, String> {
    let backup = require_launcher_session(backup)?;

    prepare_account_launch(config, backup).await?;
    wait_for_launch_target_window(VALORANT_OPEN_TIMEOUT, VALORANT_OPEN_POLL_INTERVAL).await
}

async fn prepare_account_launch(
    config: LaunchConfig,
    backup: LauncherSessionBackup,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        close_riot_processes().map_err(|error| error.to_string())?;
        apply_launcher_session_backup(&backup).map_err(|error| error.to_string())?;
        launch_valorant(&config).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("failed to join VALORANT launch preparation task: {error}"))?
}

pub(super) fn require_launcher_session(
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

pub(super) const LOGIN_CAPTURE_TIMEOUT: Duration = Duration::from_secs(600);
pub(super) const LOGIN_CAPTURE_POLL_INTERVAL: Duration = Duration::from_secs(2);
pub(super) const VALORANT_OPEN_TIMEOUT: Duration = Duration::from_secs(300);
pub(super) const VALORANT_OPEN_POLL_INTERVAL: Duration = Duration::from_secs(1);
pub(super) const SHOP_RESET_CHECK_INTERVAL: Duration = Duration::from_secs(1);

pub(super) async fn wait_for_launch_target_window(
    timeout: Duration,
    poll_interval: Duration,
) -> Result<LaunchTargetProcess, String> {
    let started = std::time::Instant::now();

    while started.elapsed() < timeout {
        if let Some(target) = check_launch_target_window().await? {
            return Ok(target);
        }

        tokio::time::sleep(poll_interval).await;
    }

    Err(
        "sent VALORANT launch request to Riot Client, but a visible VALORANT window was not detected"
            .to_string(),
    )
}

async fn check_launch_target_window() -> Result<Option<LaunchTargetProcess>, String> {
    tokio::task::spawn_blocking(launch_target_window_is_visible)
        .await
        .map_err(|error| format!("failed to join VALORANT window check task: {error}"))?
        .map_err(|error| error.to_string())
}

pub(super) async fn check_riot_client_window_visible() -> Result<bool, String> {
    tokio::task::spawn_blocking(riot_client_window_is_visible)
        .await
        .map_err(|error| format!("failed to join Riot Client window check task: {error}"))?
        .map_err(|error| error.to_string())
}

pub(super) async fn start_launcher_session_login(
    account_id: AccountId,
    backup_root: PathBuf,
    config: LaunchConfig,
) -> Result<CapturedLauncherSession, String> {
    close_riot_processes().map_err(|error| error.to_string())?;
    clear_existing_launcher_data_dirs().map_err(|error| error.to_string())?;
    launch_riot_login_capture(&config).map_err(|error| error.to_string())?;
    wait_for_launcher_session_capture(
        account_id,
        backup_root,
        LOGIN_CAPTURE_TIMEOUT,
        LOGIN_CAPTURE_POLL_INTERVAL,
    )
    .await
}

pub(super) async fn start_verified_launcher_session_login(
    account_id: AccountId,
    backup_root: PathBuf,
    config: LaunchConfig,
) -> Result<CapturedLauncherSession, String> {
    let mut captured =
        start_launcher_session_login(account_id, backup_root.clone(), config).await?;
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

pub(super) async fn wait_for_launcher_session_capture(
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
        "timed out waiting for Riot Client remembered login; make sure Remember Me is enabled"
            .to_string(),
    )
}

pub(super) fn is_pending_launcher_capture_error(error: &LauncherSessionError) -> bool {
    matches!(error, LauncherSessionError::PrivateSettingsNotFound)
}

pub(super) async fn resolve_session_shard(
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

pub(super) fn shard_from_player_affinities(player_info: &PlayerInfoResponse) -> Option<Shard> {
    ["live", "pp", "pvp"]
        .into_iter()
        .filter_map(|key| player_info.affinity.get(key))
        .find_map(|value| Shard::from_live_affinity(value))
}

pub(super) async fn start_account_capture(
    account_id: AccountId,
    backup_root: PathBuf,
    config: LaunchConfig,
) -> Result<CapturedAccountDraft, String> {
    let captured = start_launcher_session_login(account_id, backup_root.clone(), config).await?;
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

async fn close_riot_client_after_capture() -> Result<(), String> {
    tokio::task::spawn_blocking(close_riot_client_processes)
        .await
        .map_err(|error| format!("failed to join Riot Client close task: {error}"))?
        .map_err(|error| error.to_string())
}

pub(super) async fn enrich_captured_account(
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

pub(super) async fn enrich_captured_account_identity(
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
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let cookies = read_backup_cookies(backup).map_err(|error| error.to_string())?;
    let cookie_header = launcher_cookie_header(&cookies).map_err(|error| error.to_string())?;
    let mut session = api
        .launcher_reauth(&cookie_header)
        .await
        .map(|tokens| tokens.into_session())
        .map_err(|error| error.to_string())?;
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
pub(super) struct RefreshedProfileIdentity {
    pub(super) account_id: AccountId,
    pub(super) session: AuthSession,
    pub(super) puuid: String,
    pub(super) game_name: String,
    pub(super) tag_line: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StorefrontResult {
    pub(super) account_id: AccountId,
    pub(super) summary: StoreSummary,
    pub(super) session: AuthSession,
    pub(super) identity: ApiIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LoadoutResult {
    pub(super) account_id: AccountId,
    pub(super) summary: LoadoutSummary,
    pub(super) session: AuthSession,
    pub(super) identity: ApiIdentity,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct AccountRanksResult {
    pub(super) ranks: Vec<AccountRankResult>,
    pub(super) failures: Vec<AccountRankFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AccountRankResult {
    pub(super) account_id: AccountId,
    pub(super) rank: Result<Option<CompetitiveRank>, String>,
    pub(super) account_level: Result<i64, String>,
    pub(super) session: AuthSession,
    pub(super) identity: ApiIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AccountRankFailure {
    pub(super) account_id: AccountId,
    pub(super) error: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CapturedAccountDraft {
    pub(super) account_id: AccountId,
    pub(super) backup: LauncherSessionBackup,
    pub(super) puuid: String,
    pub(super) game_name: Option<String>,
    pub(super) tag_line: Option<String>,
    pub(super) shard: Shard,
    pub(super) session: Option<AuthSession>,
}

impl CapturedAccountDraft {
    pub(super) fn new(account_id: AccountId, backup: LauncherSessionBackup) -> Self {
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

    pub(super) fn riot_id(&self) -> Option<String> {
        match (&self.game_name, &self.tag_line) {
            (Some(game_name), Some(tag_line)) if !game_name.is_empty() && !tag_line.is_empty() => {
                Some(format!("{game_name}#{tag_line}"))
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ApiIdentity {
    pub(super) puuid: String,
    pub(super) game_name: Option<String>,
    pub(super) tag_line: Option<String>,
    pub(super) shard: Shard,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StoreSummary {
    pub(super) currency_balances: Vec<CurrencyBalanceDisplay>,
    pub(super) currency_balance_error: Option<String>,
    pub(super) featured_bundles: Vec<StoreBundleDisplay>,
    pub(super) daily_offers: Vec<StoreOfferDisplay>,
    pub(super) daily_remaining_seconds: i64,
    pub(super) bundle_remaining_seconds: i64,
    pub(super) night_market_remaining_seconds: Option<i64>,
    pub(super) loaded_at: iced::time::Instant,
    pub(super) night_market_offers: Vec<StoreOfferDisplay>,
    pub(super) accessory_remaining_seconds: Option<i64>,
    pub(super) accessory_offers: Vec<StoreAccessoryDisplay>,
}

impl StoreSummary {
    #[cfg(test)]
    pub(super) fn from_response(
        response: StorefrontResponse,
        skins: &SkinCatalog,
        bundles: &BundleCatalog,
        currencies: &CurrencyCatalog,
    ) -> Self {
        Self::from_response_with_wallet_and_accessories(
            response,
            None,
            skins,
            bundles,
            currencies,
            &AccessoryCatalog::default(),
        )
    }

    #[cfg(test)]
    pub(super) fn from_response_with_wallet(
        response: StorefrontResponse,
        wallet: Option<WalletResponse>,
        skins: &SkinCatalog,
        bundles: &BundleCatalog,
        currencies: &CurrencyCatalog,
    ) -> Self {
        Self::from_response_with_wallet_and_accessories(
            response,
            wallet,
            skins,
            bundles,
            currencies,
            &AccessoryCatalog::default(),
        )
    }

    pub(super) fn from_response_with_accessories(
        response: StorefrontResponse,
        skins: &SkinCatalog,
        bundles: &BundleCatalog,
        currencies: &CurrencyCatalog,
        accessories: &AccessoryCatalog,
    ) -> Self {
        Self::from_response_with_wallet_and_accessories(
            response,
            None,
            skins,
            bundles,
            currencies,
            accessories,
        )
    }

    pub(super) fn from_response_with_wallet_and_accessories(
        response: StorefrontResponse,
        wallet: Option<WalletResponse>,
        skins: &SkinCatalog,
        bundles: &BundleCatalog,
        currencies: &CurrencyCatalog,
        accessories: &AccessoryCatalog,
    ) -> Self {
        Self::from_response_at(
            response,
            wallet,
            skins,
            bundles,
            currencies,
            accessories,
            iced::time::Instant::now(),
        )
    }

    pub(super) fn from_response_at(
        response: StorefrontResponse,
        wallet: Option<WalletResponse>,
        skins: &SkinCatalog,
        bundles: &BundleCatalog,
        currencies: &CurrencyCatalog,
        accessories: &AccessoryCatalog,
        loaded_at: iced::time::Instant,
    ) -> Self {
        let featured_bundles = if response.featured_bundle.bundles.is_empty() {
            std::iter::once(&response.featured_bundle.bundle)
                .map(|bundle| store_bundle_display(bundle, skins, bundles, currencies))
                .collect()
        } else {
            response
                .featured_bundle
                .bundles
                .iter()
                .map(|bundle| store_bundle_display(bundle, skins, bundles, currencies))
                .collect()
        };
        let night_market_remaining_seconds = response
            .bonus_store
            .as_ref()
            .map(|store| store.bonus_store_remaining_duration_in_seconds);
        let night_market_offers = response
            .bonus_store
            .as_ref()
            .map(|store| {
                store
                    .bonus_store_offers
                    .iter()
                    .map(|offer| bonus_store_offer_display(offer, skins, currencies))
                    .collect()
            })
            .unwrap_or_default();
        let daily_offers = response
            .skins_panel_layout
            .single_item_offers
            .iter()
            .map(|offer_id| {
                let matching_offer = response
                    .skins_panel_layout
                    .single_item_store_offers
                    .iter()
                    .find(|offer| offer.offer_id == *offer_id);

                store_offer_display(offer_id, matching_offer, 0, skins, currencies)
            })
            .collect();
        let accessory_remaining_seconds = response
            .accessory_store
            .as_ref()
            .map(|store| store.accessory_store_remaining_duration_in_seconds);
        let accessory_offers = response
            .accessory_store
            .as_ref()
            .map(|store| {
                store
                    .accessory_store_offers
                    .iter()
                    .map(|offer| accessory_store_offer_display(offer, accessories, currencies))
                    .collect()
            })
            .unwrap_or_default();

        Self {
            currency_balances: wallet
                .as_ref()
                .map(|wallet| currency_balances_from_wallet(wallet, currencies))
                .unwrap_or_default(),
            currency_balance_error: None,
            featured_bundles,
            daily_offers,
            daily_remaining_seconds: response
                .skins_panel_layout
                .single_item_offers_remaining_duration_in_seconds,
            bundle_remaining_seconds: response
                .featured_bundle
                .bundle_remaining_duration_in_seconds,
            night_market_remaining_seconds,
            loaded_at,
            night_market_offers,
            accessory_remaining_seconds,
            accessory_offers,
        }
    }

    pub(super) fn daily_remaining_seconds_at(&self, now: iced::time::Instant) -> i64 {
        remaining_seconds_at(self.daily_remaining_seconds, self.loaded_at, now)
    }

    pub(super) fn bundle_remaining_seconds_at(&self, now: iced::time::Instant) -> i64 {
        remaining_seconds_at(self.bundle_remaining_seconds, self.loaded_at, now)
    }

    pub(super) fn night_market_remaining_seconds_at(&self, now: iced::time::Instant) -> i64 {
        self.night_market_remaining_seconds
            .map(|seconds| remaining_seconds_at(seconds, self.loaded_at, now))
            .unwrap_or(0)
    }

    pub(super) fn accessory_remaining_seconds_at(&self, now: iced::time::Instant) -> i64 {
        self.accessory_remaining_seconds
            .map(|seconds| remaining_seconds_at(seconds, self.loaded_at, now))
            .unwrap_or(0)
    }

    pub(super) fn is_expired_at(&self, now: iced::time::Instant) -> bool {
        let section_expired =
            self.daily_remaining_seconds_at(now) == 0 || self.bundle_remaining_seconds_at(now) == 0;
        let night_market_expired = self
            .night_market_remaining_seconds
            .is_some_and(|_| self.night_market_remaining_seconds_at(now) == 0);
        let accessory_expired = self
            .accessory_remaining_seconds
            .is_some_and(|_| self.accessory_remaining_seconds_at(now) == 0);

        section_expired || night_market_expired || accessory_expired
    }
}

pub(super) fn remaining_seconds_at(
    original_seconds: i64,
    loaded_at: iced::time::Instant,
    now: iced::time::Instant,
) -> i64 {
    let elapsed_seconds = now
        .checked_duration_since(loaded_at)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0);

    original_seconds.saturating_sub(elapsed_seconds)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StoreOfferDisplay {
    pub(super) skin: SkinDisplay,
    pub(super) price: Option<OfferPrice>,
    pub(super) original_price: Option<OfferPrice>,
    pub(super) discount_percent: i64,
}

impl StoreOfferDisplay {
    #[cfg(test)]
    pub(super) fn label(&self) -> String {
        let mut label = self.skin.display_name.clone();

        if let Some(price) = &self.price {
            if let Some(original_price) = &self.original_price {
                label.push_str(&format!(
                    " ({} -> {})",
                    original_price.label(),
                    price.label()
                ));
            } else {
                label.push_str(&format!(" ({})", price.label()));
            }
        }

        if self.discount_percent > 0 {
            label.push_str(&format!(", {}% off", self.discount_percent));
        }

        label
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StoreAccessoryDisplay {
    pub(super) accessory: AccessoryDisplay,
    pub(super) price: Option<OfferPrice>,
}

impl StoreAccessoryDisplay {
    #[cfg(test)]
    pub(super) fn label(&self) -> String {
        let mut label = self.accessory.display_name.clone();

        if let Some(price) = &self.price {
            label.push_str(&format!(" ({})", price.label()));
        }

        label
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StoreBundleDisplay {
    pub(super) bundle: BundleDisplay,
    pub(super) price: Option<OfferPrice>,
    pub(super) item_count: i64,
    pub(super) rarity: Option<String>,
}

impl StoreBundleDisplay {
    pub(super) fn item_count_label(&self) -> String {
        match self.item_count {
            1 => "1 item".to_string(),
            count => format!("{count} items"),
        }
    }

    #[cfg(test)]
    pub(super) fn label(&self) -> String {
        let mut label = self.bundle.display_name.clone();

        if let Some(price) = &self.price {
            label.push_str(&format!(" ({})", price.label()));
        }

        label.push_str(&format!(", {}", self.item_count_label()));
        label
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct OfferPrice {
    pub(super) amount: i64,
    pub(super) currency: CurrencyDisplay,
}

impl OfferPrice {
    pub(super) fn label(&self) -> String {
        format!("{} {}", self.amount, self.currency.display_name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CurrencyBalanceDisplay {
    pub(super) amount: i64,
    pub(super) currency: CurrencyDisplay,
}

impl CurrencyBalanceDisplay {
    pub(super) fn label(&self) -> String {
        format!(
            "{} {}",
            format_whole_number(self.amount),
            self.currency.display_name
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CurrencyDisplay {
    pub(super) uuid: String,
    pub(super) display_name: String,
    pub(super) display_icon: Option<String>,
}

impl From<ResolvedCurrency> for CurrencyDisplay {
    fn from(currency: ResolvedCurrency) -> Self {
        Self {
            uuid: currency.uuid,
            display_name: shop_currency_name(&currency.display_name),
            display_icon: currency.display_icon,
        }
    }
}

const VALORANT_POINTS_UUID: &str = "85ad13f7-3d1b-5128-9eb2-7cd8ee0b5741";
const RADIANITE_POINTS_UUID: &str = "e59aa87c-4cbf-517a-5983-6e81511be9b7";
const KINGDOM_CREDITS_UUID: &str = "85ca954a-41f2-ce94-9b45-8ca3dd39a00d";

pub(super) fn shop_currency_name(display_name: &str) -> String {
    known_shop_currency_name(display_name)
        .unwrap_or(display_name)
        .to_string()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BundleDisplay {
    pub(super) uuid: String,
    pub(super) display_name: String,
    pub(super) display_icon: Option<String>,
    pub(super) viewer_icon: Option<String>,
    pub(super) cached_icon: Option<PathBuf>,
}

impl From<ResolvedBundle> for BundleDisplay {
    fn from(bundle: ResolvedBundle) -> Self {
        Self {
            uuid: bundle.uuid,
            display_name: bundle.display_name,
            display_icon: bundle.display_icon,
            viewer_icon: bundle.viewer_icon,
            cached_icon: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AccessoryDisplay {
    pub(super) uuid: String,
    pub(super) display_name: String,
    pub(super) display_icon: Option<String>,
    pub(super) viewer_icon: Option<String>,
    pub(super) cached_icon: Option<PathBuf>,
}

impl From<ResolvedAccessory> for AccessoryDisplay {
    fn from(accessory: ResolvedAccessory) -> Self {
        Self {
            uuid: accessory.uuid,
            display_name: accessory.display_name,
            display_icon: accessory.display_icon,
            viewer_icon: accessory.viewer_icon,
            cached_icon: None,
        }
    }
}

pub(super) fn store_offer_display(
    offer_id: &str,
    offer: Option<&StoreOffer>,
    discount_percent: i64,
    skins: &SkinCatalog,
    currencies: &CurrencyCatalog,
) -> StoreOfferDisplay {
    let direct = skins.resolve(offer_id);
    let skin = if direct.display_name != offer_id {
        SkinDisplay::from(direct)
    } else {
        offer
            .and_then(|offer| offer.rewards.first())
            .map(|reward| SkinDisplay::from(skins.resolve(&reward.item_id)))
            .unwrap_or_else(|| SkinDisplay::from(direct))
    };
    let price = offer.and_then(|offer| offer_price(&offer.cost, currencies));

    StoreOfferDisplay {
        skin,
        price,
        original_price: None,
        discount_percent,
    }
}

pub(super) fn bonus_store_offer_display(
    offer: &BonusStoreOffer,
    skins: &SkinCatalog,
    currencies: &CurrencyCatalog,
) -> StoreOfferDisplay {
    let skin = offer
        .offer
        .rewards
        .first()
        .map(|reward| SkinDisplay::from(skins.resolve(&reward.item_id)))
        .unwrap_or_else(|| SkinDisplay::from(skins.resolve(&offer.offer.offer_id)));
    let discounted_price = offer_price(&offer.discount_costs, currencies);
    let base_price = offer_price(&offer.offer.cost, currencies);
    let price = discounted_price.clone().or_else(|| base_price.clone());
    let original_price = base_price.filter(|base_price| {
        discounted_price
            .as_ref()
            .is_some_and(|discounted_price| discounted_price != base_price)
    });

    StoreOfferDisplay {
        skin,
        price,
        original_price,
        discount_percent: offer.discount_percent,
    }
}

pub(super) fn accessory_store_offer_display(
    offer: &AccessoryStoreOffer,
    accessories: &AccessoryCatalog,
    currencies: &CurrencyCatalog,
) -> StoreAccessoryDisplay {
    let accessory = offer
        .offer
        .rewards
        .first()
        .map(|reward| AccessoryDisplay::from(accessories.resolve(&reward.item_id)))
        .unwrap_or_else(|| AccessoryDisplay::from(accessories.resolve(&offer.offer.offer_id)));
    let price = offer_price(&offer.offer.cost, currencies);

    StoreAccessoryDisplay { accessory, price }
}

pub(super) fn store_bundle_display(
    bundle: &StoreBundle,
    skins: &SkinCatalog,
    bundles: &BundleCatalog,
    currencies: &CurrencyCatalog,
) -> StoreBundleDisplay {
    let direct = bundles.resolve(&bundle.data_asset_id);
    let resolved = if direct.display_name != bundle.data_asset_id {
        direct
    } else {
        bundles.resolve(&bundle.id)
    };
    let rarity = strongest_bundle_rarity(bundle, skins);
    let item_count = bundle.items.len() as i64;

    StoreBundleDisplay {
        bundle: BundleDisplay::from(resolved),
        price: bundle_price(bundle, currencies),
        item_count,
        rarity,
    }
}

pub(super) fn bundle_price(
    bundle: &StoreBundle,
    currencies: &CurrencyCatalog,
) -> Option<OfferPrice> {
    bundle
        .total_discounted_cost
        .as_ref()
        .and_then(|costs| offer_price(costs, currencies))
        .or_else(|| {
            bundle
                .total_base_cost
                .as_ref()
                .and_then(|costs| offer_price(costs, currencies))
        })
        .or_else(|| summed_bundle_item_price(bundle, currencies))
}

pub(super) fn summed_bundle_item_price(
    bundle: &StoreBundle,
    currencies: &CurrencyCatalog,
) -> Option<OfferPrice> {
    let currency_id = bundle
        .currency_id
        .trim()
        .is_empty()
        .then(|| bundle.items.first().map(|item| item.currency_id.as_str()))
        .flatten()
        .unwrap_or(bundle.currency_id.as_str());

    if currency_id.trim().is_empty() || bundle.items.is_empty() {
        return None;
    }

    let amount = bundle
        .items
        .iter()
        .filter(|item| item.currency_id.eq_ignore_ascii_case(currency_id))
        .map(|item| {
            if item.discounted_price > 0 {
                item.discounted_price
            } else {
                item.base_price
            }
        })
        .sum();

    Some(OfferPrice {
        amount,
        currency: currency_display_for_id(currency_id, currencies),
    })
}

pub(super) fn strongest_bundle_rarity(bundle: &StoreBundle, skins: &SkinCatalog) -> Option<String> {
    bundle
        .items
        .iter()
        .filter_map(|item| skins.resolve(&item.item.item_id).rarity)
        .max_by_key(|rarity| rarity_rank(rarity))
}

pub(super) fn rarity_rank(rarity: &str) -> usize {
    RarityTier::from_name(rarity).map_or(0, RarityTier::rank)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RarityTier {
    Select,
    Deluxe,
    Premium,
    Ultra,
    Exclusive,
}

impl RarityTier {
    pub(super) fn from_name(rarity: &str) -> Option<Self> {
        let rarity = rarity.to_ascii_lowercase();

        if rarity.contains("exclusive") {
            Some(Self::Exclusive)
        } else if rarity.contains("ultra") {
            Some(Self::Ultra)
        } else if rarity.contains("premium") {
            Some(Self::Premium)
        } else if rarity.contains("deluxe") {
            Some(Self::Deluxe)
        } else if rarity.contains("select") {
            Some(Self::Select)
        } else {
            None
        }
    }

    pub(super) fn rank(self) -> usize {
        match self {
            Self::Select => 1,
            Self::Deluxe => 2,
            Self::Premium => 3,
            Self::Ultra => 4,
            Self::Exclusive => 5,
        }
    }
}

pub(super) fn offer_price(
    costs: &std::collections::HashMap<String, i64>,
    currencies: &CurrencyCatalog,
) -> Option<OfferPrice> {
    let (currency_id, amount) = costs.iter().min_by(|left, right| left.0.cmp(right.0))?;

    Some(OfferPrice {
        amount: *amount,
        currency: currency_display_for_id(currency_id, currencies),
    })
}

pub(super) fn currency_balances_from_wallet(
    wallet: &WalletResponse,
    currencies: &CurrencyCatalog,
) -> Vec<CurrencyBalanceDisplay> {
    let mut balances = wallet
        .balances
        .iter()
        .filter_map(|(currency_id, amount)| {
            let currency = currency_display_for_id(currency_id, currencies);

            shop_currency_rank(&currency.uuid, &currency.display_name).map(|_| {
                CurrencyBalanceDisplay {
                    amount: *amount,
                    currency,
                }
            })
        })
        .collect::<Vec<_>>();

    balances.sort_by(|left, right| {
        shop_currency_rank(&left.currency.uuid, &left.currency.display_name)
            .unwrap_or(usize::MAX)
            .cmp(
                &shop_currency_rank(&right.currency.uuid, &right.currency.display_name)
                    .unwrap_or(usize::MAX),
            )
            .then_with(|| left.currency.display_name.cmp(&right.currency.display_name))
    });
    balances
}

pub(super) fn currency_display_for_id(
    currency_id: &str,
    currencies: &CurrencyCatalog,
) -> CurrencyDisplay {
    let resolved = currencies.resolve(currency_id);
    let fallback_name = known_shop_currency_name(currency_id);
    let display_name = if resolved.display_name.eq_ignore_ascii_case(currency_id) {
        fallback_name
            .map(str::to_string)
            .unwrap_or_else(|| shop_currency_name(&resolved.display_name))
    } else {
        shop_currency_name(&resolved.display_name)
    };

    CurrencyDisplay {
        uuid: resolved.uuid,
        display_name,
        display_icon: resolved.display_icon,
    }
}

fn known_shop_currency_name(value: &str) -> Option<&'static str> {
    let value = value.trim();

    if value.eq_ignore_ascii_case(VALORANT_POINTS_UUID)
        || value.eq_ignore_ascii_case("vp")
        || value.eq_ignore_ascii_case("valorant points")
        || value.eq_ignore_ascii_case("valorant point")
    {
        Some("VP")
    } else if value.eq_ignore_ascii_case(RADIANITE_POINTS_UUID)
        || value.eq_ignore_ascii_case("radianite")
        || value.eq_ignore_ascii_case("radianite points")
        || value.eq_ignore_ascii_case("radianite point")
    {
        Some("Radianite")
    } else if value.eq_ignore_ascii_case(KINGDOM_CREDITS_UUID)
        || value.eq_ignore_ascii_case("kingdom credits")
        || value.eq_ignore_ascii_case("kingdom credit")
        || value.eq_ignore_ascii_case("kc")
    {
        Some("Kingdom Credits")
    } else {
        None
    }
}

fn shop_currency_rank(uuid: &str, display_name: &str) -> Option<usize> {
    let name = known_shop_currency_name(uuid).or_else(|| known_shop_currency_name(display_name))?;

    match name {
        "VP" => Some(0),
        "Radianite" => Some(1),
        "Kingdom Credits" => Some(2),
        _ => None,
    }
}

pub(super) fn format_whole_number(amount: i64) -> String {
    let negative = amount.is_negative();
    let magnitude = if negative {
        -(amount as i128)
    } else {
        amount as i128
    };
    let digits = magnitude.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3 + 1);

    for (index, digit) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(digit);
    }

    let mut formatted = formatted.chars().rev().collect::<String>();
    if negative {
        formatted.insert(0, '-');
    }
    formatted
}

pub(super) fn format_duration(seconds: i64) -> String {
    if seconds <= 0 {
        return "soon".to_string();
    }

    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;

    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

pub(super) fn competitive_rank_from_mmr(response: &PlayerMmrResponse) -> Option<CompetitiveRank> {
    let competitive = response.queue_skills.competitive.as_ref()?;
    let seasons = &competitive.seasonal_info_by_season_id;

    if let Some((season_id, season)) =
        response
            .latest_competitive_update
            .as_ref()
            .and_then(|update| {
                seasons
                    .get(&update.season_id)
                    .map(|season| (update.season_id.as_str(), season))
            })
    {
        return Some(competitive_rank_from_season(season_id, season));
    }

    if let Some(update) = response
        .latest_competitive_update
        .as_ref()
        .filter(|update| update.tier_after_update > 0 || update.ranked_rating_after_update > 0)
    {
        return Some(CompetitiveRank::new(
            update.tier_after_update,
            rank_name_for_competitive_tier(update.tier_after_update),
            update.ranked_rating_after_update,
            non_empty_string(update.season_id.clone()),
        ));
    }

    seasons
        .iter()
        .find(|(_, season)| season_has_rank_data(season))
        .map(|(season_id, season)| competitive_rank_from_season(season_id, season))
}

fn competitive_rank_from_season(season_id: &str, season: &MmrSeasonInfo) -> CompetitiveRank {
    CompetitiveRank::new(
        season.competitive_tier,
        rank_name_for_competitive_tier(season.competitive_tier),
        season.ranked_rating,
        non_empty_string(if season.season_id.is_empty() {
            season_id.to_string()
        } else {
            season.season_id.clone()
        }),
    )
}

fn season_has_rank_data(season: &MmrSeasonInfo) -> bool {
    season.competitive_tier > 0
        || season.ranked_rating > 0
        || season.number_of_games > 0
        || season.games_needed_for_rating > 0
}

pub(super) fn rank_name_for_competitive_tier(tier: i64) -> String {
    const COMPETITIVE_RANK_NAMES: &[(i64, &str)] = &[
        (3, "Iron 1"),
        (4, "Iron 2"),
        (5, "Iron 3"),
        (6, "Bronze 1"),
        (7, "Bronze 2"),
        (8, "Bronze 3"),
        (9, "Silver 1"),
        (10, "Silver 2"),
        (11, "Silver 3"),
        (12, "Gold 1"),
        (13, "Gold 2"),
        (14, "Gold 3"),
        (15, "Platinum 1"),
        (16, "Platinum 2"),
        (17, "Platinum 3"),
        (18, "Diamond 1"),
        (19, "Diamond 2"),
        (20, "Diamond 3"),
        (21, "Ascendant 1"),
        (22, "Ascendant 2"),
        (23, "Ascendant 3"),
        (24, "Immortal 1"),
        (25, "Immortal 2"),
        (26, "Immortal 3"),
        (27, "Radiant"),
    ];

    COMPETITIVE_RANK_NAMES
        .iter()
        .find_map(|(rank_tier, name)| (*rank_tier == tier).then_some((*name).to_string()))
        .unwrap_or_else(|| {
            if tier > 27 {
                format!("Tier {tier}")
            } else {
                "Unrated".to_string()
            }
        })
}

fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LoadoutSummary {
    pub(super) account_level: i64,
    pub(super) gun_skins: Vec<LoadoutGunDisplay>,
    pub(super) battle_pass: Option<BattlePassProgressDisplay>,
    pub(super) battle_pass_error: Option<String>,
}

impl LoadoutSummary {
    pub(super) fn from_response(
        response: PlayerLoadoutResponse,
        skins: &SkinCatalog,
        weapons: &WeaponCatalog,
        account_level: Option<i64>,
    ) -> Self {
        let mut gun_skins = response
            .guns
            .into_iter()
            .map(|gun| {
                let weapon = WeaponDisplay::from(weapons.resolve(&gun.id));
                let base_skin = skins.resolve(&gun.skin_id);
                let skin_level = loadout_skin_level_label(
                    &skins.resolve(&gun.skin_level_id),
                    &base_skin.display_name,
                    &gun.skin_level_id,
                );
                let skin = SkinDisplay::from(resolve_current_skin(
                    skins,
                    &gun.skin_id,
                    &gun.skin_level_id,
                    &gun.chroma_id,
                ));

                LoadoutGunDisplay {
                    weapon,
                    skin,
                    skin_name: base_skin.display_name,
                    skin_level,
                }
            })
            .collect::<Vec<_>>();
        gun_skins.sort_by_key(|gun| weapon_order(&gun.weapon.display_name));

        Self {
            account_level: account_level.unwrap_or(response.identity.account_level),
            gun_skins,
            battle_pass: None,
            battle_pass_error: None,
        }
    }

    pub(super) fn battle_pass_timer_active(&self) -> bool {
        self.battle_pass
            .as_ref()
            .is_some_and(|battle_pass| battle_pass.remaining_seconds.is_some())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BattlePassProgressDisplay {
    pub(super) name: String,
    pub(super) season_name: Option<String>,
    pub(super) level_reached: i64,
    pub(super) total_levels: Option<i64>,
    pub(super) progression_towards_next_level: i64,
    pub(super) next_level_progress_required: Option<i64>,
    pub(super) total_progression_earned: i64,
    pub(super) total_progression_required: Option<i64>,
    pub(super) completed: bool,
    pub(super) remaining_seconds: Option<i64>,
    pub(super) earned_rewards: Vec<BattlePassRewardDisplay>,
    pub(super) unearned_rewards: Vec<BattlePassRewardDisplay>,
    pub(super) locked_paid_rewards: Vec<BattlePassRewardDisplay>,
    pub(super) loaded_at: iced::time::Instant,
}

impl BattlePassProgressDisplay {
    pub(super) fn title(&self) -> String {
        self.season_name
            .as_ref()
            .filter(|name| !name.trim().is_empty())
            .map(|season| format!("{season} Battle Pass"))
            .unwrap_or_else(|| self.name.clone())
    }

    pub(super) fn tier_label(&self) -> String {
        match self.total_levels {
            Some(total_levels) if total_levels > 0 => {
                format!(
                    "Tier {} of {}",
                    self.level_reached.clamp(0, total_levels),
                    total_levels
                )
            }
            _ => format!("Tier {}", self.level_reached.max(0)),
        }
    }

    pub(super) fn next_tier_label(&self) -> String {
        if self.completed {
            return "Complete".to_string();
        }

        match self.next_level_progress_required {
            Some(required) if required > 0 => format!(
                "{} / {} XP toward next tier",
                format_whole_number(self.progression_towards_next_level.max(0)),
                format_whole_number(required)
            ),
            _ => format!(
                "{} XP toward next tier",
                format_whole_number(self.progression_towards_next_level.max(0))
            ),
        }
    }

    pub(super) fn total_progress_label(&self) -> Option<String> {
        self.total_progression_required
            .filter(|required| *required > 0)
            .map(|required| {
                format!(
                    "{} / {} XP total",
                    format_whole_number(self.total_progression_earned.max(0)),
                    format_whole_number(required)
                )
            })
    }

    pub(super) fn progress_percent_label(&self) -> Option<String> {
        self.total_progression_required
            .filter(|required| *required > 0)
            .map(|required| {
                let percent =
                    (self.total_progression_earned.max(0) as f64 / required as f64) * 100.0;
                format!("{:.0}% complete", percent.clamp(0.0, 100.0))
            })
    }

    pub(super) fn progress_fraction(&self) -> f32 {
        if self.completed {
            return 1.0;
        }

        self.total_progression_required
            .filter(|required| *required > 0)
            .map(|required| {
                (self.total_progression_earned.max(0) as f32 / required as f32).clamp(0.0, 1.0)
            })
            .unwrap_or(0.0)
    }

    pub(super) fn remaining_seconds_at(&self, now: iced::time::Instant) -> Option<i64> {
        self.remaining_seconds
            .map(|seconds| remaining_seconds_at(seconds, self.loaded_at, now))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BattlePassRewardTrack {
    Free,
    Paid,
}

impl BattlePassRewardTrack {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Paid => "Paid",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BattlePassRewardDisplay {
    pub(super) tier: i64,
    pub(super) chapter: i64,
    pub(super) level_in_chapter: i64,
    pub(super) is_epilogue: bool,
    pub(super) track: BattlePassRewardTrack,
    pub(super) uuid: String,
    pub(super) name: String,
    pub(super) kind: String,
    pub(super) amount: i64,
    pub(super) highlighted: bool,
    pub(super) display_icon: Option<String>,
    pub(super) viewer_icon: Option<String>,
    pub(super) cached_icon: Option<PathBuf>,
}

impl BattlePassRewardDisplay {
    pub(super) fn location_label(&self) -> String {
        if self.is_epilogue {
            format!("Epilogue tier {}", self.tier.max(0))
        } else {
            format!(
                "Tier {} (Ch {} L{})",
                self.tier.max(0),
                self.chapter.max(0),
                self.level_in_chapter.max(0)
            )
        }
    }

    pub(super) fn amount_label(&self) -> Option<String> {
        (self.amount > 1).then(|| format!("x{}", format_whole_number(self.amount)))
    }
}

pub(super) fn battle_pass_progress_from_responses(
    contracts: &ContractsResponse,
    contract_catalog: &ContractCatalog,
    content: Option<&GameContentResponse>,
    skins: &SkinCatalog,
    accessories: &AccessoryCatalog,
    currencies: &CurrencyCatalog,
) -> Option<BattlePassProgressDisplay> {
    battle_pass_progress_from_responses_at(
        contracts,
        contract_catalog,
        content,
        skins,
        accessories,
        currencies,
        OffsetDateTime::now_utc(),
        iced::time::Instant::now(),
    )
}

fn battle_pass_progress_from_responses_at(
    contracts: &ContractsResponse,
    contract_catalog: &ContractCatalog,
    content: Option<&GameContentResponse>,
    skins: &SkinCatalog,
    accessories: &AccessoryCatalog,
    currencies: &CurrencyCatalog,
    now_utc: OffsetDateTime,
    loaded_at: iced::time::Instant,
) -> Option<BattlePassProgressDisplay> {
    let active_act = content.and_then(GameContentResponse::active_act);
    let (definition, contract) =
        find_battle_pass_contract(contracts, contract_catalog, active_act)?;
    let progression_deltas = definition.level_xp.as_slice();
    let total_levels = Some(i64::try_from(progression_deltas.len()).unwrap_or(0));
    let total_progression_required = Some(progression_deltas.iter().copied().sum::<i64>());
    let next_level_index = usize::try_from(contract.progression_level_reached.max(0)).ok();
    let next_level_progress_required =
        next_level_index.and_then(|index| progression_deltas.get(index).copied());
    let completed = contract.progression_completed
        || total_levels
            .is_some_and(|levels| levels > 0 && contract.progression_level_reached >= levels);
    let remaining_seconds =
        active_act.and_then(|act| remaining_seconds_until_utc_at(&act.end_time, now_utc));
    let paid_pass_owned = battle_pass_paid_pass_owned(definition, contract);
    let (earned_rewards, unearned_rewards, locked_paid_rewards) = battle_pass_reward_groups(
        definition,
        contract,
        paid_pass_owned,
        skins,
        accessories,
        currencies,
    );

    Some(BattlePassProgressDisplay {
        name: non_empty_string(definition.display_name.clone())
            .unwrap_or_else(|| "Battle Pass".to_string()),
        season_name: active_act.and_then(|act| non_empty_string(act.name.clone())),
        level_reached: contract.progression_level_reached,
        total_levels,
        progression_towards_next_level: contract.progression_towards_next_level,
        next_level_progress_required,
        total_progression_earned: contract.contract_progression.total_progression_earned,
        total_progression_required,
        completed,
        remaining_seconds,
        earned_rewards,
        unearned_rewards,
        locked_paid_rewards,
        loaded_at,
    })
}

fn battle_pass_paid_pass_owned(definition: &ResolvedContract, contract: &PlayerContract) -> bool {
    let Some(schedule_id) = definition
        .premium_reward_schedule_uuid
        .as_ref()
        .filter(|schedule_id| !schedule_id.trim().is_empty())
    else {
        return true;
    };

    contract
        .contract_progression
        .highest_rewarded_level
        .iter()
        .any(|(reward_schedule_id, level)| {
            ids_match(reward_schedule_id, schedule_id) && level.amount > 0
        })
}

fn battle_pass_reward_groups(
    definition: &ResolvedContract,
    contract: &PlayerContract,
    paid_pass_owned: bool,
    skins: &SkinCatalog,
    accessories: &AccessoryCatalog,
    currencies: &CurrencyCatalog,
) -> (
    Vec<BattlePassRewardDisplay>,
    Vec<BattlePassRewardDisplay>,
    Vec<BattlePassRewardDisplay>,
) {
    let mut earned_rewards = Vec::new();
    let mut unearned_rewards = Vec::new();
    let mut locked_paid_rewards = Vec::new();
    let level_reached = contract.progression_level_reached.max(0);

    for level in &definition.reward_levels {
        if let Some(reward) = &level.premium_reward {
            let display = battle_pass_reward_display(
                reward,
                level,
                BattlePassRewardTrack::Paid,
                skins,
                accessories,
                currencies,
            );

            if !paid_pass_owned {
                locked_paid_rewards.push(display);
            } else if level.tier <= level_reached {
                earned_rewards.push(display);
            } else {
                unearned_rewards.push(display);
            }
        }

        for reward in &level.free_rewards {
            let display = battle_pass_reward_display(
                reward,
                level,
                BattlePassRewardTrack::Free,
                skins,
                accessories,
                currencies,
            );

            if level.tier <= level_reached {
                earned_rewards.push(display);
            } else {
                unearned_rewards.push(display);
            }
        }
    }

    (earned_rewards, unearned_rewards, locked_paid_rewards)
}

fn battle_pass_reward_display(
    reward: &ResolvedContractReward,
    level: &crate::riot::content::ResolvedContractRewardLevel,
    track: BattlePassRewardTrack,
    skins: &SkinCatalog,
    accessories: &AccessoryCatalog,
    currencies: &CurrencyCatalog,
) -> BattlePassRewardDisplay {
    let resolved = resolve_battle_pass_reward(reward, skins, accessories, currencies);

    BattlePassRewardDisplay {
        tier: level.tier,
        chapter: level.chapter,
        level_in_chapter: level.level_in_chapter,
        is_epilogue: level.is_epilogue,
        track,
        uuid: reward.uuid.clone(),
        name: resolved.name,
        kind: resolved.kind,
        amount: reward.amount,
        highlighted: reward.highlighted,
        display_icon: resolved.display_icon,
        viewer_icon: resolved.viewer_icon,
        cached_icon: None,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedBattlePassReward {
    name: String,
    kind: String,
    display_icon: Option<String>,
    viewer_icon: Option<String>,
}

fn resolve_battle_pass_reward(
    reward: &ResolvedContractReward,
    skins: &SkinCatalog,
    accessories: &AccessoryCatalog,
    currencies: &CurrencyCatalog,
) -> ResolvedBattlePassReward {
    match reward.kind.as_str() {
        "EquippableSkinLevel" | "EquippableSkinChroma" | "Skin" => {
            let skin = skins.resolve(&reward.uuid);
            ResolvedBattlePassReward {
                name: skin.display_name,
                kind: "Weapon skin".to_string(),
                display_icon: skin.display_icon,
                viewer_icon: skin.viewer_icon,
            }
        }
        "EquippableCharmLevel" | "Buddy" | "BuddyLevel" => {
            let accessory = accessories.resolve(&reward.uuid);
            ResolvedBattlePassReward {
                name: accessory.display_name,
                kind: "Gun buddy".to_string(),
                display_icon: accessory.display_icon,
                viewer_icon: accessory.viewer_icon,
            }
        }
        "Spray" => {
            let accessory = accessories.resolve(&reward.uuid);
            ResolvedBattlePassReward {
                name: accessory.display_name,
                kind: "Spray".to_string(),
                display_icon: accessory.display_icon,
                viewer_icon: accessory.viewer_icon,
            }
        }
        "PlayerCard" => {
            let accessory = accessories.resolve(&reward.uuid);
            ResolvedBattlePassReward {
                name: accessory.display_name,
                kind: "Player card".to_string(),
                display_icon: accessory.display_icon,
                viewer_icon: accessory.viewer_icon,
            }
        }
        "Title" => {
            let accessory = accessories.resolve(&reward.uuid);
            ResolvedBattlePassReward {
                name: accessory.display_name,
                kind: "Title".to_string(),
                display_icon: accessory.display_icon,
                viewer_icon: accessory.viewer_icon,
            }
        }
        "Currency" => {
            let currency = currencies.resolve(&reward.uuid);
            ResolvedBattlePassReward {
                name: currency.display_name,
                kind: "Currency".to_string(),
                display_icon: currency.display_icon,
                viewer_icon: currency.viewer_icon,
            }
        }
        kind => ResolvedBattlePassReward {
            name: reward.uuid.clone(),
            kind: if kind.trim().is_empty() {
                "Reward".to_string()
            } else {
                kind.to_string()
            },
            display_icon: None,
            viewer_icon: None,
        },
    }
}

fn find_battle_pass_contract<'a>(
    contracts: &'a ContractsResponse,
    contract_catalog: &'a ContractCatalog,
    active_act: Option<&GameContentSeason>,
) -> Option<(&'a ResolvedContract, &'a PlayerContract)> {
    if let Some((definition, contract)) = active_act
        .and_then(|act| contract_catalog.resolve_active_season(&act.id))
        .filter(|definition| !definition.level_xp.is_empty())
        .and_then(|definition| {
            contracts
                .contracts
                .iter()
                .find(|contract| ids_match(&contract.contract_definition_id, &definition.uuid))
                .map(|contract| (definition, contract))
        })
    {
        return Some((definition, contract));
    }

    contracts
        .contracts
        .iter()
        .filter_map(|contract| {
            contract_catalog
                .resolve(&contract.contract_definition_id)
                .filter(|definition| {
                    definition.is_season_contract() && !definition.level_xp.is_empty()
                })
                .map(|definition| (definition, contract))
        })
        .max_by_key(|(_, contract)| {
            (
                !contract.progression_completed,
                contract.progression_level_reached,
                contract.contract_progression.total_progression_earned,
            )
        })
}

fn ids_match(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn remaining_seconds_until_utc_at(end_time: &str, now: OffsetDateTime) -> Option<i64> {
    let end = parse_utc_timestamp(end_time)?;

    Some(
        end.unix_timestamp()
            .saturating_sub(now.unix_timestamp())
            .max(0),
    )
}

fn parse_utc_timestamp(value: &str) -> Option<OffsetDateTime> {
    let trimmed = value.trim();
    let timestamp = trimmed
        .strip_suffix('Z')
        .or_else(|| trimmed.strip_suffix("+00:00"))
        .unwrap_or(trimmed);
    let (date, time) = timestamp.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i32>().ok()?;
    let month = date_parts.next()?.parse::<u8>().ok()?;
    let day = date_parts.next()?.parse::<u8>().ok()?;

    if date_parts.next().is_some() {
        return None;
    }

    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u8>().ok()?;
    let minute = time_parts.next()?.parse::<u8>().ok()?;
    let second_part = time_parts.next()?;

    if time_parts.next().is_some() {
        return None;
    }

    let second = second_part
        .split_once('.')
        .map(|(seconds, _)| seconds)
        .unwrap_or(second_part)
        .parse::<u8>()
        .ok()?;
    let date = Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()?;
    let time = Time::from_hms(hour, minute, second).ok()?;

    Some(PrimitiveDateTime::new(date, time).assume_utc())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LoadoutGunDisplay {
    pub(super) weapon: WeaponDisplay,
    pub(super) skin: SkinDisplay,
    pub(super) skin_name: String,
    pub(super) skin_level: Option<String>,
}

impl LoadoutGunDisplay {
    pub(super) fn skin_detail_label(&self) -> String {
        match self
            .skin_level
            .as_ref()
            .map(|level| level.trim())
            .filter(|level| !level.is_empty())
        {
            Some(level) => format!("{} - {}", self.skin_name, level),
            None => self.skin_name.clone(),
        }
    }

    #[cfg(test)]
    pub(super) fn label(&self) -> String {
        format!("{}: {}", self.weapon.display_name, self.skin_detail_label())
    }
}

fn loadout_skin_level_label(
    level: &ResolvedSkin,
    skin_name: &str,
    level_id: &str,
) -> Option<String> {
    level.level_label.clone().or_else(|| {
        let display_name = level.display_name.trim();

        (!display_name.is_empty()
            && !display_name.eq_ignore_ascii_case(level_id)
            && !display_name.eq_ignore_ascii_case(skin_name))
        .then(|| display_name.to_string())
    })
}

pub(super) fn weapon_order(name: &str) -> (usize, String) {
    const WEAPON_ORDER: &[&str] = &[
        "Classic", "Shorty", "Frenzy", "Ghost", "Sheriff", "Bandit", "Stinger", "Spectre", "Bucky",
        "Judge", "Bulldog", "Guardian", "Phantom", "Vandal", "Marshal", "Outlaw", "Operator",
        "Ares", "Odin", "Melee",
    ];
    let index = WEAPON_ORDER
        .iter()
        .position(|weapon| *weapon == name)
        .unwrap_or(99);

    (index, name.to_string())
}

pub(super) fn weapon_category(name: &str) -> &'static str {
    match name {
        "Classic" | "Shorty" | "Frenzy" | "Ghost" | "Sheriff" | "Bandit" => "Sidearms",
        "Stinger" | "Spectre" => "SMGs",
        "Bucky" | "Judge" => "Shotguns",
        "Bulldog" | "Guardian" | "Phantom" | "Vandal" => "Rifles",
        "Marshal" | "Outlaw" | "Operator" => "Sniper Rifles",
        "Ares" | "Odin" => "Heavy",
        "Melee" => "Melee",
        _ => "Other",
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct WeaponDisplay {
    pub(super) uuid: String,
    pub(super) display_name: String,
    pub(super) display_icon: Option<String>,
    pub(super) viewer_icon: Option<String>,
    pub(super) cached_icon: Option<PathBuf>,
}

impl From<ResolvedWeapon> for WeaponDisplay {
    fn from(weapon: ResolvedWeapon) -> Self {
        Self {
            uuid: weapon.uuid,
            display_name: weapon.display_name,
            display_icon: weapon.display_icon,
            viewer_icon: weapon.viewer_icon,
            cached_icon: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SkinDisplay {
    pub(super) uuid: String,
    pub(super) display_name: String,
    pub(super) display_icon: Option<String>,
    pub(super) viewer_icon: Option<String>,
    pub(super) rarity: Option<String>,
    pub(super) cached_icon: Option<PathBuf>,
}

impl From<ResolvedSkin> for SkinDisplay {
    fn from(skin: ResolvedSkin) -> Self {
        Self {
            uuid: skin.uuid,
            display_name: skin.display_name,
            display_icon: skin.display_icon,
            viewer_icon: skin.viewer_icon,
            rarity: skin.rarity,
            cached_icon: None,
        }
    }
}

pub(super) async fn fetch_storefront(
    account: AccountProfile,
    client_version: String,
    image_cache: ImageCache,
) -> Result<StorefrontResult, String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let resolved = resolve_credentials(&api, &account, client_version).await?;
    let metadata = fetch_store_metadata().await?;
    let mut summary = api
        .storefront(&resolved.credentials)
        .await
        .map(|response| {
            StoreSummary::from_response_with_accessories(
                response,
                &metadata.skins,
                &metadata.bundles,
                &metadata.currencies,
                &metadata.accessories,
            )
        })
        .map_err(|error| error.to_string())?;
    match api.wallet(&resolved.credentials).await {
        Ok(wallet) => {
            summary.currency_balances =
                currency_balances_from_wallet(&wallet, &metadata.currencies);
            summary.currency_balance_error = None;
        }
        Err(error) => {
            summary.currency_balance_error = Some(error.to_string());
        }
    }
    cache_store_images(&mut summary, &image_cache).await?;

    Ok(StorefrontResult {
        account_id: account.id,
        summary,
        session: resolved.session,
        identity: resolved.identity,
    })
}

pub(super) async fn fetch_loadout(
    account: AccountProfile,
    client_version: String,
    image_cache: ImageCache,
) -> Result<LoadoutResult, String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let resolved = resolve_credentials(&api, &account, client_version).await?;
    let metadata = fetch_loadout_metadata().await?;
    let account_level = api
        .account_xp(&resolved.credentials)
        .await
        .ok()
        .map(|xp| xp.progress.level);
    let battle_pass = fetch_battle_pass_progress(&api, &resolved.credentials, &metadata).await;
    let mut summary = api
        .player_loadout(&resolved.credentials)
        .await
        .map(|response| {
            LoadoutSummary::from_response(
                response,
                &metadata.skins,
                &metadata.weapons,
                account_level,
            )
        })
        .map_err(|error| error.to_string())?;
    match battle_pass {
        Ok(progress) => {
            summary.battle_pass = Some(progress);
            summary.battle_pass_error = None;
        }
        Err(error) => {
            summary.battle_pass = None;
            summary.battle_pass_error = Some(error);
        }
    }
    cache_loadout_images(&mut summary, &image_cache).await?;

    Ok(LoadoutResult {
        account_id: account.id,
        summary,
        session: resolved.session,
        identity: resolved.identity,
    })
}

async fn fetch_battle_pass_progress(
    api: &RiotApi,
    credentials: &ApiCredentials,
    metadata: &LoadoutMetadata,
) -> Result<BattlePassProgressDisplay, String> {
    let contracts = api
        .contracts(credentials)
        .await
        .map_err(|error| error.to_string())?;
    let content = api
        .game_content(credentials)
        .await
        .map_err(|error| error.to_string())?;

    battle_pass_progress_from_responses(
        &contracts,
        &metadata.contracts,
        Some(&content),
        &metadata.skins,
        &metadata.accessories,
        &metadata.currencies,
    )
    .ok_or_else(|| "No active battle pass progress found".to_string())
}

pub(super) async fn fetch_account_ranks(
    accounts: Vec<AccountProfile>,
    client_version: String,
) -> AccountRanksResult {
    let api = match RiotApi::new() {
        Ok(api) => api,
        Err(error) => {
            return AccountRanksResult {
                ranks: Vec::new(),
                failures: accounts
                    .into_iter()
                    .map(|account| AccountRankFailure {
                        account_id: account.id,
                        error: error.to_string(),
                    })
                    .collect(),
            };
        }
    };

    let mut result = AccountRanksResult::default();

    for account in accounts {
        let account_id = account.id;

        match fetch_account_rank(&api, account, client_version.clone()).await {
            Ok(rank) => result.ranks.push(rank),
            Err(error) => result
                .failures
                .push(AccountRankFailure { account_id, error }),
        }
    }

    result
}

async fn fetch_account_rank(
    api: &RiotApi,
    account: AccountProfile,
    client_version: String,
) -> Result<AccountRankResult, String> {
    let account_id = account.id;
    let resolved = resolve_credentials(api, &account, client_version).await?;
    let rank = api
        .player_mmr(&resolved.credentials)
        .await
        .map(|response| competitive_rank_from_mmr(&response))
        .map_err(|error| error.to_string());
    let account_level = api
        .account_xp(&resolved.credentials)
        .await
        .map(|response| response.progress.level)
        .map_err(|error| error.to_string());

    if let (Err(rank_error), Err(level_error)) = (&rank, &account_level) {
        return Err(format!(
            "rank unavailable: {rank_error}; level unavailable: {level_error}"
        ));
    }

    Ok(AccountRankResult {
        account_id,
        rank,
        account_level,
        session: resolved.session,
        identity: resolved.identity,
    })
}

pub(super) async fn fetch_profile_identity(
    account: AccountProfile,
) -> Result<RefreshedProfileIdentity, String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let session = refreshed_api_session(&api, &account).await?;
    let player_info = api
        .player_info(&session.access_token)
        .await
        .map_err(|error| error.to_string())?;

    Ok(RefreshedProfileIdentity {
        account_id: account.id,
        session,
        puuid: player_info.sub,
        game_name: player_info.acct.game_name,
        tag_line: player_info.acct.tag_line,
    })
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct StoreMetadata {
    pub(super) skins: SkinCatalog,
    pub(super) bundles: BundleCatalog,
    pub(super) currencies: CurrencyCatalog,
    pub(super) accessories: AccessoryCatalog,
}

pub(super) async fn fetch_store_metadata() -> Result<StoreMetadata, String> {
    let api = ValorantContentApi::new().map_err(|error| error.to_string())?;

    Ok(StoreMetadata {
        skins: api
            .skin_catalog()
            .await
            .map_err(|error| error.to_string())?,
        bundles: api
            .bundle_catalog()
            .await
            .map_err(|error| error.to_string())?,
        currencies: api
            .currency_catalog()
            .await
            .map_err(|error| error.to_string())?,
        accessories: api
            .accessory_catalog()
            .await
            .map_err(|error| error.to_string())?,
    })
}

pub(super) async fn cache_store_images(
    summary: &mut StoreSummary,
    image_cache: &ImageCache,
) -> Result<(), String> {
    for bundle in &mut summary.featured_bundles {
        cache_bundle_icon(&mut bundle.bundle, image_cache).await?;
    }

    for offer in summary
        .daily_offers
        .iter_mut()
        .chain(summary.night_market_offers.iter_mut())
    {
        cache_skin_icon(&mut offer.skin, image_cache).await?;
    }

    for offer in &mut summary.accessory_offers {
        cache_accessory_icon(&mut offer.accessory, image_cache).await?;
    }

    Ok(())
}

pub(super) async fn cache_loadout_images(
    summary: &mut LoadoutSummary,
    image_cache: &ImageCache,
) -> Result<(), String> {
    for gun in &mut summary.gun_skins {
        cache_weapon_icon(&mut gun.weapon, image_cache).await?;
        cache_skin_icon(&mut gun.skin, image_cache).await?;
    }

    if let Some(battle_pass) = &mut summary.battle_pass {
        for reward in battle_pass
            .earned_rewards
            .iter_mut()
            .chain(battle_pass.unearned_rewards.iter_mut())
            .chain(battle_pass.locked_paid_rewards.iter_mut())
        {
            cache_battle_pass_reward_icon(reward, image_cache).await?;
        }
    }

    Ok(())
}

pub(super) async fn cache_skin_icon(
    skin: &mut SkinDisplay,
    image_cache: &ImageCache,
) -> Result<(), String> {
    let Some(url) = skin.display_icon.as_ref() else {
        return Ok(());
    };

    skin.cached_icon = Some(
        image_cache
            .cache_url("skins", &skin.uuid, url)
            .await
            .map_err(|error| error.to_string())?,
    );
    Ok(())
}

pub(super) async fn cache_weapon_icon(
    weapon: &mut WeaponDisplay,
    image_cache: &ImageCache,
) -> Result<(), String> {
    let Some(url) = weapon.display_icon.as_ref() else {
        return Ok(());
    };

    weapon.cached_icon = Some(
        image_cache
            .cache_url("weapons", &weapon.uuid, url)
            .await
            .map_err(|error| error.to_string())?,
    );
    Ok(())
}

pub(super) async fn cache_accessory_icon(
    accessory: &mut AccessoryDisplay,
    image_cache: &ImageCache,
) -> Result<(), String> {
    let Some(url) = accessory.display_icon.as_ref() else {
        return Ok(());
    };

    accessory.cached_icon = Some(
        image_cache
            .cache_url("accessories", &accessory.uuid, url)
            .await
            .map_err(|error| error.to_string())?,
    );
    Ok(())
}

pub(super) async fn cache_bundle_icon(
    bundle: &mut BundleDisplay,
    image_cache: &ImageCache,
) -> Result<(), String> {
    let Some(url) = bundle.display_icon.as_ref() else {
        return Ok(());
    };

    bundle.cached_icon = Some(
        image_cache
            .cache_url("bundles", &bundle.uuid, url)
            .await
            .map_err(|error| error.to_string())?,
    );
    Ok(())
}

pub(super) async fn cache_battle_pass_reward_icon(
    reward: &mut BattlePassRewardDisplay,
    image_cache: &ImageCache,
) -> Result<(), String> {
    let Some(url) = reward.display_icon.as_ref() else {
        return Ok(());
    };

    reward.cached_icon = Some(
        image_cache
            .cache_url("battle-pass", &reward.uuid, url)
            .await
            .map_err(|error| error.to_string())?,
    );
    Ok(())
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct LoadoutMetadata {
    pub(super) skins: SkinCatalog,
    pub(super) weapons: WeaponCatalog,
    pub(super) contracts: ContractCatalog,
    pub(super) accessories: AccessoryCatalog,
    pub(super) currencies: CurrencyCatalog,
}

pub(super) async fn fetch_loadout_metadata() -> Result<LoadoutMetadata, String> {
    let api = ValorantContentApi::new().map_err(|error| error.to_string())?;

    Ok(LoadoutMetadata {
        skins: api
            .skin_catalog()
            .await
            .map_err(|error| error.to_string())?,
        weapons: api
            .weapon_catalog()
            .await
            .map_err(|error| error.to_string())?,
        contracts: api
            .contract_catalog()
            .await
            .map_err(|error| error.to_string())?,
        accessories: api
            .accessory_catalog()
            .await
            .map_err(|error| error.to_string())?,
        currencies: api
            .currency_catalog()
            .await
            .map_err(|error| error.to_string())?,
    })
}

pub(super) async fn fetch_current_client_version() -> Result<String, String> {
    ValorantContentApi::new()
        .map_err(|error| error.to_string())?
        .client_version()
        .await
        .map_err(|error| error.to_string())
}

pub(super) fn resolve_current_skin(
    catalog: &SkinCatalog,
    skin_id: &str,
    skin_level_id: &str,
    chroma_id: &str,
) -> ResolvedSkin {
    let mut fallback = None;

    for id in [chroma_id, skin_level_id, skin_id] {
        let skin = catalog.resolve(id);

        if skin.display_name == id {
            continue;
        }

        if skin.display_icon.is_some() {
            return skin;
        }

        fallback.get_or_insert(skin);
    }

    fallback.unwrap_or_else(|| catalog.resolve(skin_id))
}

pub(super) async fn resolve_credentials(
    api: &RiotApi,
    account: &AccountProfile,
    client_version: String,
) -> Result<ResolvedApiCredentials, String> {
    let mut session = active_api_session(api, account).await?;
    let player_info = api.player_info(&session.access_token).await.ok();

    let entitlements_token = entitlement_token(api, &session).await?;
    if session
        .entitlements_token
        .as_ref()
        .is_none_or(|token| token.trim().is_empty())
    {
        session.entitlements_token = Some(entitlements_token.clone());
    }

    let puuid = player_info
        .as_ref()
        .map(|info| info.sub.clone())
        .or_else(|| {
            account
                .puuid
                .clone()
                .filter(|puuid| !puuid.trim().is_empty())
        })
        .or_else(|| {
            account
                .launcher_session
                .as_ref()
                .map(|backup| backup.puuid.clone())
                .filter(|puuid| !puuid.trim().is_empty())
        })
        .ok_or_else(|| "selected account does not have a Riot PUUID".to_string())?;
    let shard = resolve_session_shard(api, &session, player_info.as_ref(), account.shard).await;
    let identity = match player_info {
        Some(info) => ApiIdentity {
            puuid: puuid.clone(),
            game_name: Some(info.acct.game_name),
            tag_line: Some(info.acct.tag_line),
            shard,
        },
        None => ApiIdentity {
            puuid: puuid.clone(),
            game_name: None,
            tag_line: None,
            shard,
        },
    };

    Ok(ResolvedApiCredentials {
        credentials: ApiCredentials {
            access_token: session.access_token.clone(),
            entitlements_token,
            client_version,
            shard,
            puuid,
        },
        session,
        identity,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ResolvedApiCredentials {
    pub(super) credentials: ApiCredentials,
    pub(super) session: AuthSession,
    pub(super) identity: ApiIdentity,
}

pub(super) async fn active_api_session(
    api: &RiotApi,
    account: &AccountProfile,
) -> Result<AuthSession, String> {
    if let Some(session) = &account.session
        && !session.is_expired()
    {
        return Ok(session.clone());
    }

    let Some(backup) = &account.launcher_session else {
        return Err(
            "selected account needs an imported Riot token or a captured launcher session"
                .to_string(),
        );
    };

    launcher_api_session(api, backup).await
}

pub(super) async fn refreshed_api_session(
    api: &RiotApi,
    account: &AccountProfile,
) -> Result<AuthSession, String> {
    if let Some(backup) = &account.launcher_session {
        return launcher_api_session(api, backup).await;
    }

    active_api_session(api, account).await
}

async fn launcher_api_session(
    api: &RiotApi,
    backup: &LauncherSessionBackup,
) -> Result<AuthSession, String> {
    if !backup.is_ready() {
        return Err(
            "selected account launcher session is incomplete, missing Riot private settings, or its backup folder is missing; re-capture selected login"
                .to_string(),
        );
    }

    let cookies = read_backup_cookies(backup).map_err(|error| error.to_string())?;
    let cookie_header = launcher_cookie_header(&cookies).map_err(|error| error.to_string())?;
    api.launcher_reauth(&cookie_header)
        .await
        .map(|tokens| tokens.into_session())
        .map_err(|error| {
            format!(
                "launcher session reauth failed: {error}. Recapture the Riot Client session or import a fresh redirect token."
            )
        })
}

pub(super) async fn entitlement_token(
    api: &RiotApi,
    session: &AuthSession,
) -> Result<String, String> {
    if let Some(token) = &session.entitlements_token
        && !token.trim().is_empty()
    {
        return Ok(token.clone());
    }

    api.entitlement(&session.access_token)
        .await
        .map(|response| response.entitlements_token)
        .map_err(|error| error.to_string())
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
    account.mark_refreshed_now();

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
