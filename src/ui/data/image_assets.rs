use super::*;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use iced::futures::future::{try_join4, try_join5};

use crate::riot::content::ContentError;

use super::loadout::{BattlePassRewardDisplay, LoadoutSummary, SkinDisplay, WeaponDisplay};
use super::shop::{AccessoryDisplay, BundleDisplay, StoreSummary};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct StoreMetadata {
    pub(in crate::ui) skins: Arc<SkinCatalog>,
    pub(in crate::ui) bundles: Arc<BundleCatalog>,
    pub(in crate::ui) currencies: Arc<CurrencyCatalog>,
    pub(in crate::ui) accessories: Arc<AccessoryCatalog>,
}

pub(in crate::ui) async fn fetch_store_metadata() -> Result<StoreMetadata, String> {
    let api = ValorantContentApi::new().map_err(|error| error.to_string())?;
    let (skins, bundles, currencies, accessories) = try_join4(
        SKIN_CATALOG.get_or_fetch(|| api.skin_catalog()),
        BUNDLE_CATALOG.get_or_fetch(|| api.bundle_catalog()),
        CURRENCY_CATALOG.get_or_fetch(|| api.currency_catalog()),
        ACCESSORY_CATALOG.get_or_fetch(|| api.accessory_catalog()),
    )
    .await?;

    Ok(StoreMetadata {
        skins,
        bundles,
        currencies,
        accessories,
    })
}

// Content catalogs only change with game patches, so Shop and Loadout share one download per
// session. Entries expire after an hour so a patch released while the app is open still shows up.
// Failed downloads are not cached.
const CONTENT_CATALOG_TTL: Duration = Duration::from_secs(60 * 60);

static SKIN_CATALOG: CachedCatalog<SkinCatalog> = CachedCatalog::new();
static WEAPON_CATALOG: CachedCatalog<WeaponCatalog> = CachedCatalog::new();
static BUNDLE_CATALOG: CachedCatalog<BundleCatalog> = CachedCatalog::new();
static CURRENCY_CATALOG: CachedCatalog<CurrencyCatalog> = CachedCatalog::new();
static ACCESSORY_CATALOG: CachedCatalog<AccessoryCatalog> = CachedCatalog::new();
static CONTRACT_CATALOG: CachedCatalog<ContractCatalog> = CachedCatalog::new();

pub(in crate::ui) struct CachedCatalog<T> {
    entry: Mutex<Option<(Instant, Arc<T>)>>,
}

impl<T> CachedCatalog<T> {
    pub(in crate::ui) const fn new() -> Self {
        Self {
            entry: Mutex::new(None),
        }
    }

    pub(in crate::ui) async fn get_or_fetch<Fut>(
        &self,
        fetch: impl FnOnce() -> Fut,
    ) -> Result<Arc<T>, String>
    where
        Fut: Future<Output = Result<T, ContentError>>,
    {
        if let Some(catalog) = self.fresh_at(Instant::now()) {
            return Ok(catalog);
        }

        let catalog = Arc::new(fetch().await.map_err(|error| error.to_string())?);
        *self.lock() = Some((Instant::now(), Arc::clone(&catalog)));
        Ok(catalog)
    }

    pub(in crate::ui) fn fresh_at(&self, now: Instant) -> Option<Arc<T>> {
        self.lock()
            .as_ref()
            .filter(|(fetched_at, _)| {
                now.saturating_duration_since(*fetched_at) < CONTENT_CATALOG_TTL
            })
            .map(|(_, catalog)| Arc::clone(catalog))
    }

    fn lock(&self) -> MutexGuard<'_, Option<(Instant, Arc<T>)>> {
        self.entry.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

// Image downloads never fail a tab: an item whose art cannot be fetched keeps `cached_icon: None`
// and the view shows a placeholder for it.

pub(in crate::ui) async fn cache_store_images(
    summary: &mut StoreSummary,
    image_cache: &ImageCache,
) {
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

    for offer in &mut summary.accessory_offers {
        cache_accessory_icon(&mut offer.accessory, image_cache).await;
    }
}

pub(in crate::ui) async fn cache_loadout_images(
    summary: &mut LoadoutSummary,
    image_cache: &ImageCache,
) {
    for gun in &mut summary.gun_skins {
        cache_weapon_icon(&mut gun.weapon, image_cache).await;
        cache_skin_icon(&mut gun.skin, image_cache).await;
    }

    if let Some(battle_pass) = &mut summary.battle_pass {
        for reward in battle_pass
            .earned_rewards
            .iter_mut()
            .chain(battle_pass.unearned_rewards.iter_mut())
            .chain(battle_pass.locked_paid_rewards.iter_mut())
        {
            cache_battle_pass_reward_icon(reward, image_cache).await;
        }
    }
}

async fn cached_icon(
    image_cache: &ImageCache,
    namespace: &str,
    id: &str,
    url: Option<&String>,
) -> Option<PathBuf> {
    image_cache.cache_url(namespace, id, url?).await.ok()
}

pub(in crate::ui) async fn cache_skin_icon(skin: &mut SkinDisplay, image_cache: &ImageCache) {
    skin.cached_icon =
        cached_icon(image_cache, "skins", &skin.uuid, skin.display_icon.as_ref()).await;
}

pub(in crate::ui) async fn cache_weapon_icon(weapon: &mut WeaponDisplay, image_cache: &ImageCache) {
    weapon.cached_icon = cached_icon(
        image_cache,
        "weapons",
        &weapon.uuid,
        weapon.display_icon.as_ref(),
    )
    .await;
}

pub(in crate::ui) async fn cache_accessory_icon(
    accessory: &mut AccessoryDisplay,
    image_cache: &ImageCache,
) {
    accessory.cached_icon = cached_icon(
        image_cache,
        "accessories",
        &accessory.uuid,
        accessory.display_icon.as_ref(),
    )
    .await;
}

pub(in crate::ui) async fn cache_bundle_icon(bundle: &mut BundleDisplay, image_cache: &ImageCache) {
    bundle.cached_icon = cached_icon(
        image_cache,
        "bundles",
        &bundle.uuid,
        bundle.display_icon.as_ref(),
    )
    .await;
}

pub(in crate::ui) async fn cache_battle_pass_reward_icon(
    reward: &mut BattlePassRewardDisplay,
    image_cache: &ImageCache,
) {
    reward.cached_icon = cached_icon(
        image_cache,
        "battle-pass",
        &reward.uuid,
        reward.display_icon.as_ref(),
    )
    .await;
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct LoadoutMetadata {
    pub(in crate::ui) skins: Arc<SkinCatalog>,
    pub(in crate::ui) weapons: Arc<WeaponCatalog>,
    pub(in crate::ui) contracts: Arc<ContractCatalog>,
    pub(in crate::ui) accessories: Arc<AccessoryCatalog>,
    pub(in crate::ui) currencies: Arc<CurrencyCatalog>,
}

pub(in crate::ui) async fn fetch_loadout_metadata() -> Result<LoadoutMetadata, String> {
    let api = ValorantContentApi::new().map_err(|error| error.to_string())?;
    let (skins, weapons, contracts, accessories, currencies) = try_join5(
        SKIN_CATALOG.get_or_fetch(|| api.skin_catalog()),
        WEAPON_CATALOG.get_or_fetch(|| api.weapon_catalog()),
        CONTRACT_CATALOG.get_or_fetch(|| api.contract_catalog()),
        ACCESSORY_CATALOG.get_or_fetch(|| api.accessory_catalog()),
        CURRENCY_CATALOG.get_or_fetch(|| api.currency_catalog()),
    )
    .await?;

    Ok(LoadoutMetadata {
        skins,
        weapons,
        contracts,
        accessories,
        currencies,
    })
}

pub(in crate::ui) async fn fetch_current_client_version() -> Result<String, String> {
    ValorantContentApi::new()
        .map_err(|error| error.to_string())?
        .client_version()
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use iced::futures::executor::block_on;

    use super::*;

    fn failed_fetch() -> ContentError {
        let error = reqwest::Client::new()
            .get("not a url")
            .build()
            .expect_err("invalid URL");
        ContentError::Http(error)
    }

    #[test]
    fn content_catalog_is_downloaded_once_and_reused() {
        let cache = CachedCatalog::<u32>::new();
        let fetches = Cell::new(0);
        let fetch = || {
            fetches.set(fetches.get() + 1);
            async { Ok(7) }
        };

        assert_eq!(*block_on(cache.get_or_fetch(fetch)).expect("first"), 7);
        assert_eq!(*block_on(cache.get_or_fetch(fetch)).expect("second"), 7);
        assert_eq!(fetches.get(), 1);
    }

    #[test]
    fn failed_content_download_is_retried_next_time() {
        let cache = CachedCatalog::<u32>::new();

        block_on(cache.get_or_fetch(|| async { Err(failed_fetch()) })).expect_err("failure");
        let catalog = block_on(cache.get_or_fetch(|| async { Ok(3) })).expect("retry");

        assert_eq!(*catalog, 3);
    }

    #[test]
    fn content_catalog_expires_after_its_ttl() {
        let cache = CachedCatalog::<u32>::new();
        block_on(cache.get_or_fetch(|| async { Ok(1) })).expect("fetch");
        let now = Instant::now();

        assert!(cache.fresh_at(now).is_some());
        assert!(cache.fresh_at(now + CONTENT_CATALOG_TTL).is_none());
    }
}
