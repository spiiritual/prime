use super::*;
use super::image_assets::{cache_store_images, fetch_store_metadata};
use super::loadout::SkinDisplay;
use super::session::{ApiIdentity, resolve_credentials};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct StorefrontResult {
    pub(in crate::ui) account_id: AccountId,
    pub(in crate::ui) summary: StoreSummary,
    pub(in crate::ui) session: AuthSession,
    pub(in crate::ui) launcher_session: Option<LauncherSessionBackup>,
    pub(in crate::ui) identity: ApiIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct StoreSummary {
    pub(in crate::ui) currency_balances: Vec<CurrencyBalanceDisplay>,
    pub(in crate::ui) currency_balance_error: Option<String>,
    pub(in crate::ui) featured_bundles: Vec<StoreBundleDisplay>,
    pub(in crate::ui) daily_offers: Vec<StoreOfferDisplay>,
    pub(in crate::ui) daily_remaining_seconds: i64,
    pub(in crate::ui) bundle_remaining_seconds: i64,
    pub(in crate::ui) night_market_remaining_seconds: Option<i64>,
    pub(in crate::ui) loaded_at: iced::time::Instant,
    pub(in crate::ui) night_market_offers: Vec<StoreOfferDisplay>,
    pub(in crate::ui) accessory_remaining_seconds: Option<i64>,
    pub(in crate::ui) accessory_offers: Vec<StoreAccessoryDisplay>,
}

impl StoreSummary {
    #[cfg(test)]
    pub(in crate::ui) fn from_response(
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
    pub(in crate::ui) fn from_response_with_wallet(
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

    pub(in crate::ui) fn from_response_with_accessories(
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

    pub(in crate::ui) fn from_response_with_wallet_and_accessories(
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

    pub(in crate::ui) fn from_response_at(
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

    pub(in crate::ui) fn daily_remaining_seconds_at(&self, now: iced::time::Instant) -> i64 {
        remaining_seconds_at(self.daily_remaining_seconds, self.loaded_at, now)
    }

    pub(in crate::ui) fn bundle_remaining_seconds_at(&self, now: iced::time::Instant) -> i64 {
        remaining_seconds_at(self.bundle_remaining_seconds, self.loaded_at, now)
    }

    pub(in crate::ui) fn night_market_remaining_seconds_at(&self, now: iced::time::Instant) -> i64 {
        self.night_market_remaining_seconds
            .map(|seconds| remaining_seconds_at(seconds, self.loaded_at, now))
            .unwrap_or(0)
    }

    pub(in crate::ui) fn accessory_remaining_seconds_at(&self, now: iced::time::Instant) -> i64 {
        self.accessory_remaining_seconds
            .map(|seconds| remaining_seconds_at(seconds, self.loaded_at, now))
            .unwrap_or(0)
    }

    pub(in crate::ui) fn is_expired_at(&self, now: iced::time::Instant) -> bool {
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

pub(in crate::ui) fn remaining_seconds_at(
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
pub(in crate::ui) struct StoreOfferDisplay {
    pub(in crate::ui) skin: SkinDisplay,
    pub(in crate::ui) price: Option<OfferPrice>,
    pub(in crate::ui) original_price: Option<OfferPrice>,
    pub(in crate::ui) discount_percent: i64,
}

impl StoreOfferDisplay {
    #[cfg(test)]
    pub(in crate::ui) fn label(&self) -> String {
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
pub(in crate::ui) struct StoreAccessoryDisplay {
    pub(in crate::ui) accessory: AccessoryDisplay,
    pub(in crate::ui) price: Option<OfferPrice>,
}

impl StoreAccessoryDisplay {
    #[cfg(test)]
    pub(in crate::ui) fn label(&self) -> String {
        let mut label = self.accessory.display_name.clone();

        if let Some(price) = &self.price {
            label.push_str(&format!(" ({})", price.label()));
        }

        label
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct StoreBundleDisplay {
    pub(in crate::ui) bundle: BundleDisplay,
    pub(in crate::ui) price: Option<OfferPrice>,
    pub(in crate::ui) item_count: i64,
    pub(in crate::ui) rarity: Option<String>,
}

impl StoreBundleDisplay {
    pub(in crate::ui) fn item_count_label(&self) -> String {
        match self.item_count {
            1 => "1 item".to_string(),
            count => format!("{count} items"),
        }
    }

    #[cfg(test)]
    pub(in crate::ui) fn label(&self) -> String {
        let mut label = self.bundle.display_name.clone();

        if let Some(price) = &self.price {
            label.push_str(&format!(" ({})", price.label()));
        }

        label.push_str(&format!(", {}", self.item_count_label()));
        label
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct OfferPrice {
    pub(in crate::ui) amount: i64,
    pub(in crate::ui) currency: CurrencyDisplay,
}

impl OfferPrice {
    pub(in crate::ui) fn label(&self) -> String {
        format!("{} {}", self.amount, self.currency.display_name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct CurrencyBalanceDisplay {
    pub(in crate::ui) amount: i64,
    pub(in crate::ui) currency: CurrencyDisplay,
}

impl CurrencyBalanceDisplay {
    pub(in crate::ui) fn label(&self) -> String {
        format!(
            "{} {}",
            format_whole_number(self.amount),
            self.currency.display_name
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct CurrencyDisplay {
    pub(in crate::ui) uuid: String,
    pub(in crate::ui) display_name: String,
    pub(in crate::ui) display_icon: Option<String>,
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
pub(in crate::ui) const RADIANITE_POINTS_UUID: &str = "e59aa87c-4cbf-517a-5983-6e81511be9b7";
const KINGDOM_CREDITS_UUID: &str = "85ca954a-41f2-ce94-9b45-8ca3dd39a00d";

pub(in crate::ui) fn shop_currency_name(display_name: &str) -> String {
    known_shop_currency_name(display_name)
        .unwrap_or(display_name)
        .to_string()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct BundleDisplay {
    pub(in crate::ui) uuid: String,
    pub(in crate::ui) display_name: String,
    pub(in crate::ui) display_icon: Option<String>,
    pub(in crate::ui) viewer_icon: Option<String>,
    pub(in crate::ui) cached_icon: Option<PathBuf>,
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
pub(in crate::ui) struct AccessoryDisplay {
    pub(in crate::ui) uuid: String,
    pub(in crate::ui) display_name: String,
    pub(in crate::ui) display_icon: Option<String>,
    pub(in crate::ui) viewer_icon: Option<String>,
    pub(in crate::ui) cached_icon: Option<PathBuf>,
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

pub(in crate::ui) fn store_offer_display(
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

pub(in crate::ui) fn bonus_store_offer_display(
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

pub(in crate::ui) fn accessory_store_offer_display(
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

pub(in crate::ui) fn store_bundle_display(
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

pub(in crate::ui) fn bundle_price(
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

pub(in crate::ui) fn summed_bundle_item_price(
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

pub(in crate::ui) fn strongest_bundle_rarity(
    bundle: &StoreBundle,
    skins: &SkinCatalog,
) -> Option<String> {
    bundle
        .items
        .iter()
        .filter_map(|item| skins.resolve(&item.item.item_id).rarity)
        .max_by_key(|rarity| rarity_rank(rarity))
}

pub(in crate::ui) fn rarity_rank(rarity: &str) -> usize {
    RarityTier::from_name(rarity).map_or(0, RarityTier::rank)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum RarityTier {
    Select,
    Deluxe,
    Premium,
    Ultra,
    Exclusive,
}

impl RarityTier {
    pub(in crate::ui) fn from_name(rarity: &str) -> Option<Self> {
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

    pub(in crate::ui) fn rank(self) -> usize {
        match self {
            Self::Select => 1,
            Self::Deluxe => 2,
            Self::Premium => 3,
            Self::Ultra => 4,
            Self::Exclusive => 5,
        }
    }
}

pub(in crate::ui) fn offer_price(
    costs: &std::collections::HashMap<String, i64>,
    currencies: &CurrencyCatalog,
) -> Option<OfferPrice> {
    let (currency_id, amount) = costs.iter().min_by(|left, right| left.0.cmp(right.0))?;

    Some(OfferPrice {
        amount: *amount,
        currency: currency_display_for_id(currency_id, currencies),
    })
}

pub(in crate::ui) fn currency_balances_from_wallet(
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

pub(in crate::ui) fn currency_display_for_id(
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

pub(in crate::ui) fn format_whole_number(amount: i64) -> String {
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

pub(in crate::ui) fn format_duration(seconds: i64) -> String {
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

#[cfg(test)]
mod tests {
    use super::format_duration;

    #[test]
    fn format_duration_includes_ticking_seconds() {
        assert_eq!(format_duration(3_661), "1h 1m 1s");
        assert_eq!(format_duration(61), "1m 1s");
        assert_eq!(format_duration(5), "5s");
    }
}

pub(in crate::ui) async fn fetch_storefront(
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
        launcher_session: resolved.launcher_session,
        identity: resolved.identity,
    })
}
