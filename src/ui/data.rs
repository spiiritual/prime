use std::path::PathBuf;
use std::time::Duration;

use crate::account::{AccountId, AccountProfile, AuthSession, LauncherSessionBackup, Shard};
use crate::image_cache::ImageCache;
use crate::launch::{
    LaunchConfig, close_riot_processes, launch_riot_login_capture, launch_valorant,
};
use crate::riot::client::{ApiCredentials, RiotApi};
use crate::riot::content::{
    BundleCatalog, CurrencyCatalog, ResolvedBundle, ResolvedCurrency, ResolvedSkin, ResolvedWeapon,
    SkinCatalog, ValorantContentApi, WeaponCatalog,
};
use crate::riot::launcher_session::{
    CapturedLauncherSession, LauncherSessionError, apply_launcher_session_backup,
    capture_current_launcher_session, clear_existing_launcher_data_dirs, launcher_cookie_header,
    read_backup_cookies,
};
use crate::riot::models::{
    BonusStoreOffer, PlayerInfoResponse, PlayerLoadoutResponse, StoreBundle, StoreOffer,
    StorefrontResponse,
};
use crate::storage::StoredState;

pub(super) async fn launch_account(
    config: LaunchConfig,
    backup: Option<LauncherSessionBackup>,
) -> Result<(), String> {
    let backup = require_launcher_session(backup)?;

    close_riot_processes().map_err(|error| error.to_string())?;
    apply_launcher_session_backup(&backup).map_err(|error| error.to_string())?;

    launch_valorant(&config).map_err(|error| error.to_string())
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
pub(super) const SHOP_RESET_CHECK_INTERVAL: Duration = Duration::from_secs(1);

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
    let captured = start_launcher_session_login(account_id, backup_root, config).await?;
    Ok(enrich_captured_account(captured).await)
}

pub(super) async fn enrich_captured_account(
    captured: CapturedLauncherSession,
) -> CapturedAccountDraft {
    let mut draft = CapturedAccountDraft::new(captured.account_id, captured.backup);
    let Err(error) = enrich_captured_account_identity(&mut draft).await else {
        return draft;
    };

    draft.identity_warning = Some(format!(
        "Captured login, but Riot identity lookup failed: {error}. Confirm the account details manually."
    ));
    draft
}

pub(super) async fn enrich_captured_account_identity(
    draft: &mut CapturedAccountDraft,
) -> Result<(), String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let cookies = read_backup_cookies(&draft.backup).map_err(|error| error.to_string())?;
    let cookie_header = launcher_cookie_header(&cookies).map_err(|error| error.to_string())?;
    let mut session = api
        .cookie_reauth(&cookie_header)
        .await
        .map(|tokens| tokens.into_session())
        .map_err(|error| error.to_string())?;
    let player_info = api
        .player_info(&session.access_token)
        .await
        .map_err(|error| error.to_string())?;

    draft.puuid = player_info.sub.clone();
    draft.game_name = Some(player_info.acct.game_name.clone());
    draft.tag_line = Some(player_info.acct.tag_line.clone());
    draft.shard = resolve_session_shard(&api, &session, Some(&player_info), draft.shard).await;

    if session
        .entitlements_token
        .as_ref()
        .is_none_or(|token| token.trim().is_empty())
        && let Ok(entitlement) = api.entitlement(&session.access_token).await
    {
        session.entitlements_token = Some(entitlement.entitlements_token);
    }

    draft.session = Some(session);
    Ok(())
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CapturedAccountDraft {
    pub(super) account_id: AccountId,
    pub(super) backup: LauncherSessionBackup,
    pub(super) puuid: String,
    pub(super) game_name: Option<String>,
    pub(super) tag_line: Option<String>,
    pub(super) shard: Shard,
    pub(super) session: Option<AuthSession>,
    pub(super) identity_warning: Option<String>,
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
            identity_warning: None,
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
    pub(super) featured_bundles: Vec<StoreBundleDisplay>,
    pub(super) daily_offers: Vec<StoreOfferDisplay>,
    pub(super) daily_remaining_seconds: i64,
    pub(super) bundle_remaining_seconds: i64,
    pub(super) night_market_remaining_seconds: Option<i64>,
    pub(super) loaded_at: iced::time::Instant,
    pub(super) night_market_offers: Vec<StoreOfferDisplay>,
}

impl StoreSummary {
    pub(super) fn from_response(
        response: StorefrontResponse,
        skins: &SkinCatalog,
        bundles: &BundleCatalog,
        currencies: &CurrencyCatalog,
    ) -> Self {
        Self::from_response_at(
            response,
            skins,
            bundles,
            currencies,
            iced::time::Instant::now(),
        )
    }

    pub(super) fn from_response_at(
        response: StorefrontResponse,
        skins: &SkinCatalog,
        bundles: &BundleCatalog,
        currencies: &CurrencyCatalog,
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

        Self {
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

    pub(super) fn is_expired_at(&self, now: iced::time::Instant) -> bool {
        let section_expired =
            self.daily_remaining_seconds_at(now) == 0 || self.bundle_remaining_seconds_at(now) == 0;
        let night_market_expired = self
            .night_market_remaining_seconds
            .is_some_and(|_| self.night_market_remaining_seconds_at(now) == 0);

        section_expired || night_market_expired
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

pub(super) fn shop_currency_name(display_name: &str) -> String {
    if display_name.eq_ignore_ascii_case("VALORANT Points") {
        "VP".to_string()
    } else {
        display_name.to_string()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BundleDisplay {
    pub(super) uuid: String,
    pub(super) display_name: String,
    pub(super) display_icon: Option<String>,
    pub(super) cached_icon: Option<PathBuf>,
}

impl From<ResolvedBundle> for BundleDisplay {
    fn from(bundle: ResolvedBundle) -> Self {
        Self {
            uuid: bundle.uuid,
            display_name: bundle.display_name,
            display_icon: bundle.display_icon,
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
        currency: CurrencyDisplay::from(currencies.resolve(currency_id)),
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
    let rarity = rarity.to_ascii_lowercase();

    if rarity.contains("exclusive") {
        5
    } else if rarity.contains("ultra") {
        4
    } else if rarity.contains("premium") {
        3
    } else if rarity.contains("deluxe") {
        2
    } else if rarity.contains("select") {
        1
    } else {
        0
    }
}

pub(super) fn offer_price(
    costs: &std::collections::HashMap<String, i64>,
    currencies: &CurrencyCatalog,
) -> Option<OfferPrice> {
    let (currency_id, amount) = costs.iter().min_by(|left, right| left.0.cmp(right.0))?;

    Some(OfferPrice {
        amount: *amount,
        currency: CurrencyDisplay::from(currencies.resolve(currency_id)),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LoadoutSummary {
    pub(super) account_level: i64,
    pub(super) gun_skins: Vec<LoadoutGunDisplay>,
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
                let skin = SkinDisplay::from(resolve_current_skin(
                    skins,
                    &gun.skin_id,
                    &gun.skin_level_id,
                    &gun.chroma_id,
                ));

                LoadoutGunDisplay { weapon, skin }
            })
            .collect::<Vec<_>>();
        gun_skins.sort_by_key(|gun| weapon_order(&gun.weapon.display_name));

        Self {
            account_level: account_level.unwrap_or(response.identity.account_level),
            gun_skins,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LoadoutGunDisplay {
    pub(super) weapon: WeaponDisplay,
    pub(super) skin: SkinDisplay,
}

impl LoadoutGunDisplay {
    #[cfg(test)]
    pub(super) fn label(&self) -> String {
        format!("{}: {}", self.weapon.display_name, self.skin.display_name)
    }
}

pub(super) fn weapon_order(name: &str) -> (usize, String) {
    let index = match name {
        "Classic" => 0,
        "Shorty" => 1,
        "Frenzy" => 2,
        "Ghost" => 3,
        "Sheriff" => 4,
        "Bandit" => 5,
        "Stinger" => 6,
        "Spectre" => 7,
        "Bucky" => 8,
        "Judge" => 9,
        "Bulldog" => 10,
        "Guardian" => 11,
        "Phantom" => 12,
        "Vandal" => 13,
        "Marshal" => 14,
        "Outlaw" => 15,
        "Operator" => 16,
        "Ares" => 17,
        "Odin" => 18,
        "Melee" => 19,
        _ => 99,
    };

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
    pub(super) cached_icon: Option<PathBuf>,
}

impl From<ResolvedWeapon> for WeaponDisplay {
    fn from(weapon: ResolvedWeapon) -> Self {
        Self {
            uuid: weapon.uuid,
            display_name: weapon.display_name,
            display_icon: weapon.display_icon,
            cached_icon: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SkinDisplay {
    pub(super) uuid: String,
    pub(super) display_name: String,
    pub(super) display_icon: Option<String>,
    pub(super) rarity: Option<String>,
    pub(super) cached_icon: Option<PathBuf>,
}

impl From<ResolvedSkin> for SkinDisplay {
    fn from(skin: ResolvedSkin) -> Self {
        Self {
            uuid: skin.uuid,
            display_name: skin.display_name,
            display_icon: skin.display_icon,
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
    let metadata = fetch_store_metadata().await;
    let mut summary = api
        .storefront(&resolved.credentials)
        .await
        .map(|response| {
            StoreSummary::from_response(
                response,
                &metadata.skins,
                &metadata.bundles,
                &metadata.currencies,
            )
        })
        .map_err(|error| error.to_string())?;
    cache_store_images(&mut summary, &image_cache).await;

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
    let metadata = fetch_loadout_metadata().await;
    let account_level = api
        .account_xp(&resolved.credentials)
        .await
        .ok()
        .map(|xp| xp.progress.level);
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
    cache_loadout_images(&mut summary, &image_cache).await;

    Ok(LoadoutResult {
        account_id: account.id,
        summary,
        session: resolved.session,
        identity: resolved.identity,
    })
}

pub(super) async fn fetch_profile_identity(
    account: AccountProfile,
) -> Result<RefreshedProfileIdentity, String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let session = active_api_session(&api, &account).await?;
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
}

pub(super) async fn fetch_store_metadata() -> StoreMetadata {
    match ValorantContentApi::new() {
        Ok(api) => StoreMetadata {
            skins: api.skin_catalog().await.unwrap_or_default(),
            bundles: api.bundle_catalog().await.unwrap_or_default(),
            currencies: api.currency_catalog().await.unwrap_or_default(),
        },
        Err(_) => StoreMetadata::default(),
    }
}

pub(super) async fn cache_store_images(summary: &mut StoreSummary, image_cache: &ImageCache) {
    for bundle in &mut summary.featured_bundles {
        cache_bundle_icon(&mut bundle.bundle, image_cache).await;
    }

    for offer in summary
        .daily_offers
        .iter_mut()
        .chain(summary.night_market_offers.iter_mut())
    {
        cache_skin_icon(&mut offer.skin, image_cache).await;
    }
}

pub(super) async fn cache_loadout_images(summary: &mut LoadoutSummary, image_cache: &ImageCache) {
    for gun in &mut summary.gun_skins {
        cache_weapon_icon(&mut gun.weapon, image_cache).await;
        cache_skin_icon(&mut gun.skin, image_cache).await;
    }
}

pub(super) async fn cache_skin_icon(skin: &mut SkinDisplay, image_cache: &ImageCache) {
    let Some(url) = skin.display_icon.as_ref() else {
        return;
    };

    skin.cached_icon = image_cache.cache_url("skins", &skin.uuid, url).await.ok();
}

pub(super) async fn cache_weapon_icon(weapon: &mut WeaponDisplay, image_cache: &ImageCache) {
    let Some(url) = weapon.display_icon.as_ref() else {
        return;
    };

    weapon.cached_icon = image_cache
        .cache_url("weapons", &weapon.uuid, url)
        .await
        .ok();
}

pub(super) async fn cache_bundle_icon(bundle: &mut BundleDisplay, image_cache: &ImageCache) {
    let Some(url) = bundle.display_icon.as_ref() else {
        return;
    };

    bundle.cached_icon = image_cache
        .cache_url("bundles", &bundle.uuid, url)
        .await
        .ok();
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct LoadoutMetadata {
    pub(super) skins: SkinCatalog,
    pub(super) weapons: WeaponCatalog,
}

pub(super) async fn fetch_loadout_metadata() -> LoadoutMetadata {
    match ValorantContentApi::new() {
        Ok(api) => LoadoutMetadata {
            skins: api.skin_catalog().await.unwrap_or_default(),
            weapons: api.weapon_catalog().await.unwrap_or_default(),
        },
        Err(_) => LoadoutMetadata::default(),
    }
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

    if !backup.is_ready() {
        return Err(
            "selected account launcher session is incomplete, missing Riot private settings, or its backup folder is missing; re-capture selected login"
                .to_string(),
        );
    }

    let cookies = read_backup_cookies(backup).map_err(|error| error.to_string())?;
    let cookie_header = launcher_cookie_header(&cookies).map_err(|error| error.to_string())?;
    api.cookie_reauth(&cookie_header)
        .await
        .map(|tokens| tokens.into_session())
        .map_err(|error| {
            format!(
                "launcher session reauth failed; recapture the Riot Client session or import a fresh redirect token: {error}"
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
