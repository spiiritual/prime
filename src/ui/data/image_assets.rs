use super::*;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::futures::future::{join4, try_join4};
use iced::futures::stream::{self, StreamExt};

use crate::riot::content::{ContentError, MatchCatalog, ResolvedMap, WeaponContent};

use super::loadout::{BattlePassRewardDisplay, LoadoutSummary, SkinDisplay};
use super::shop::{AccessoryDisplay, BundleDisplay, BundleItem, BundleItemDisplay, StoreSummary};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct StoreMetadata {
    pub(in crate::ui) weapon_content: Arc<WeaponContent>,
    pub(in crate::ui) bundles: Arc<BundleCatalog>,
    pub(in crate::ui) currencies: Arc<CurrencyCatalog>,
    pub(in crate::ui) accessories: Arc<AccessoryCatalog>,
}

pub(in crate::ui) async fn fetch_store_metadata() -> Result<StoreMetadata, String> {
    let api = ValorantContentApi::shared()?;
    let (weapon_content, bundles, currencies, accessories) = try_join4(
        WEAPON_CONTENT.get_or_fetch(|| api.weapon_content()),
        BUNDLE_CATALOG.get_or_fetch(|| api.bundle_catalog()),
        CURRENCY_CATALOG.get_or_fetch(|| api.currency_catalog()),
        ACCESSORY_CATALOG.get_or_fetch(|| api.accessory_catalog()),
    )
    .await?;

    Ok(StoreMetadata {
        weapon_content,
        bundles,
        currencies,
        accessories,
    })
}

// Content catalogs only change with game patches, so Shop and Loadout share one download per
// session. Entries expire after an hour so a patch released while the app is open still shows up.
// Failed downloads are not cached.
const CONTENT_CATALOG_TTL: Duration = Duration::from_secs(60 * 60);

static WEAPON_CONTENT: CachedCatalog<WeaponContent> = CachedCatalog::new();
static BUNDLE_CATALOG: CachedCatalog<BundleCatalog> = CachedCatalog::new();
static CURRENCY_CATALOG: CachedCatalog<CurrencyCatalog> = CachedCatalog::new();
static ACCESSORY_CATALOG: CachedCatalog<AccessoryCatalog> = CachedCatalog::new();
static CONTRACT_CATALOG: CachedCatalog<ContractCatalog> = CachedCatalog::new();
static MATCH_CATALOG: CachedCatalog<MatchCatalog> = CachedCatalog::new();

/// Map, queue and agent names for a live match.
pub(in crate::ui) async fn fetch_match_catalog() -> Result<Arc<MatchCatalog>, String> {
    let api = ValorantContentApi::shared()?;
    MATCH_CATALOG.get_or_fetch(|| api.match_catalog()).await
}

/// Weapons and skins, for the skins players have equipped in a live match.
pub(in crate::ui) async fn fetch_weapon_content() -> Result<Arc<WeaponContent>, String> {
    let api = ValorantContentApi::shared()?;
    WEAPON_CONTENT.get_or_fetch(|| api.weapon_content()).await
}

pub(in crate::ui) struct CachedCatalog<T> {
    entry: tokio::sync::Mutex<Option<(Instant, Arc<T>)>>,
}

impl<T> CachedCatalog<T> {
    pub(in crate::ui) const fn new() -> Self {
        Self {
            entry: tokio::sync::Mutex::const_new(None),
        }
    }

    pub(in crate::ui) async fn get_or_fetch<Fut>(
        &self,
        fetch: impl FnOnce() -> Fut,
    ) -> Result<Arc<T>, String>
    where
        Fut: Future<Output = Result<T, ContentError>>,
    {
        // Held across the download, so a load that starts meanwhile waits for it instead of
        // downloading the same catalog again.
        let mut entry = self.entry.lock().await;
        if let Some(catalog) = fresh(&entry, Instant::now()) {
            return Ok(catalog);
        }

        let catalog = Arc::new(fetch().await.map_err(|error| error.to_string())?);
        *entry = Some((Instant::now(), Arc::clone(&catalog)));
        Ok(catalog)
    }

    #[cfg(test)]
    fn fresh_at(&self, now: Instant) -> Option<Arc<T>> {
        fresh(&*self.entry.try_lock().ok()?, now)
    }
}

fn fresh<T>(entry: &Option<(Instant, Arc<T>)>, now: Instant) -> Option<Arc<T>> {
    entry
        .as_ref()
        .filter(|(fetched_at, _)| now.saturating_duration_since(*fetched_at) < CONTENT_CATALOG_TTL)
        .map(|(_, catalog)| Arc::clone(catalog))
}

// Image downloads never fail a tab: an item whose art cannot be fetched keeps `cached_icon: None`
// and the view shows a placeholder for it.

/// How many images download at once on a first load.
const ICON_DOWNLOADS_AT_ONCE: usize = 8;

pub(in crate::ui) async fn cache_store_images(
    summary: &mut StoreSummary,
    image_cache: &ImageCache,
) {
    let mut downloads: Vec<IconDownload<'_>> = Vec::new();

    for bundle in &mut summary.featured_bundles {
        downloads.push(Box::pin(cache_bundle_icon(&mut bundle.bundle, image_cache)));
    }

    for offer in summary
        .daily_offers
        .iter_mut()
        .chain(summary.night_market_offers.iter_mut())
    {
        downloads.push(Box::pin(cache_skin_icon(&mut offer.skin, image_cache)));
    }

    for offer in &mut summary.accessory_offers {
        downloads.push(Box::pin(cache_accessory_icon(
            &mut offer.accessory,
            image_cache,
        )));
    }

    download_icons(downloads).await;
}

/// A bundle's item art, fetched when its details open so the shop doesn't wait on every
/// bundle's items.
pub(in crate::ui) async fn cache_bundle_item_images(
    mut items: Vec<BundleItemDisplay>,
    image_cache: ImageCache,
) -> Vec<BundleItemDisplay> {
    let downloads = items
        .iter_mut()
        .map(|item| -> IconDownload<'_> {
            match &mut item.item {
                BundleItem::Skin(skin) => Box::pin(cache_skin_icon(skin, &image_cache)),
                BundleItem::Accessory { accessory, .. } => {
                    Box::pin(cache_accessory_icon(accessory, &image_cache))
                }
            }
        })
        .collect();
    download_icons(downloads).await;
    items
}

pub(in crate::ui) async fn cache_loadout_images(
    summary: &mut LoadoutSummary,
    image_cache: &ImageCache,
) {
    let mut downloads: Vec<IconDownload<'_>> = Vec::new();

    for gun in &mut summary.gun_skins {
        downloads.push(Box::pin(cache_skin_icon(&mut gun.skin, image_cache)));
    }

    if let Some(battle_pass) = &mut summary.battle_pass {
        for reward in battle_pass.shown_rewards_mut() {
            downloads.push(Box::pin(cache_battle_pass_reward_icon(reward, image_cache)));
        }
    }

    download_icons(downloads).await;
}

/// A battle pass chapter's reward art, fetched when the chapter opens.
pub(in crate::ui) async fn cache_battle_pass_reward_icons(
    mut rewards: Vec<BattlePassRewardDisplay>,
    image_cache: ImageCache,
) -> Vec<BattlePassRewardDisplay> {
    let downloads = rewards
        .iter_mut()
        .map(|reward| -> IconDownload<'_> {
            Box::pin(cache_battle_pass_reward_icon(reward, &image_cache))
        })
        .collect();
    download_icons(downloads).await;
    rewards
}

type IconDownload<'a> = std::pin::Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

async fn download_icons(downloads: Vec<IconDownload<'_>>) {
    stream::iter(downloads)
        .buffer_unordered(ICON_DOWNLOADS_AT_ONCE)
        .collect::<Vec<()>>()
        .await;
}

/// Accessory and battle pass art is 512px square but shows at 150px or less, so it's kept at
/// twice that at most, which still looks sharp at 200% display scaling.
const SMALL_ART_MAX_SIDE: u32 = 256;

async fn cached_icon(
    image_cache: &ImageCache,
    namespace: &str,
    id: &str,
    url: Option<&String>,
) -> Option<PathBuf> {
    image_cache.cache_url(namespace, id, url?, None).await.ok()
}

async fn cached_small_art(
    image_cache: &ImageCache,
    namespace: &str,
    id: &str,
    url: Option<&String>,
) -> Option<PathBuf> {
    image_cache
        .cache_url(namespace, id, url?, Some(SMALL_ART_MAX_SIDE))
        .await
        .ok()
}

pub(in crate::ui) async fn cache_skin_icon(skin: &mut SkinDisplay, image_cache: &ImageCache) {
    skin.cached_icon =
        cached_icon(image_cache, "skins", &skin.uuid, skin.display_icon.as_ref()).await;
}

/// Caches each skin's icon, several at a time.
pub(in crate::ui) async fn cache_skin_icons<'a>(
    skins: impl IntoIterator<Item = &'a mut SkinDisplay>,
    image_cache: &'a ImageCache,
) {
    let downloads = skins
        .into_iter()
        .map(|skin| -> IconDownload<'a> { Box::pin(cache_skin_icon(skin, image_cache)) })
        .collect();
    download_icons(downloads).await;
}

pub(in crate::ui) async fn cache_map_art(
    map: &ResolvedMap,
    image_cache: &ImageCache,
) -> Option<PathBuf> {
    cached_icon(image_cache, "maps", &map.uuid, map.list_view_icon.as_ref()).await
}

pub(in crate::ui) async fn cache_accessory_icon(
    accessory: &mut AccessoryDisplay,
    image_cache: &ImageCache,
) {
    accessory.cached_icon = cached_small_art(
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

/// Battle pass tiles show weapon skins and player card banners up to about 300px wide, so their
/// art (about 500px) is kept whole for 200% display scaling.
const BATTLE_PASS_ART_MAX_SIDE: u32 = 640;

pub(in crate::ui) async fn cache_battle_pass_reward_icon(
    reward: &mut BattlePassRewardDisplay,
    image_cache: &ImageCache,
) {
    let Some(url) = reward.display_icon.as_ref() else {
        reward.cached_icon = None;
        return;
    };
    reward.cached_icon = image_cache
        .cache_url(
            "battle-pass",
            &reward.uuid,
            url,
            Some(BATTLE_PASS_ART_MAX_SIDE),
        )
        .await
        .ok();
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct LoadoutMetadata {
    pub(in crate::ui) weapon_content: Arc<WeaponContent>,
    pub(in crate::ui) contracts: Arc<ContractCatalog>,
    pub(in crate::ui) accessories: Arc<AccessoryCatalog>,
    pub(in crate::ui) currencies: Arc<CurrencyCatalog>,
}

/// The catalogs each Loadout section needs. The loadout uses only the weapon content and the
/// battle pass uses all four, so a failed download only affects the section that needs it.
pub(in crate::ui) struct LoadoutCatalogs {
    pub(in crate::ui) weapon_content: Result<Arc<WeaponContent>, String>,
    pub(in crate::ui) battle_pass: Result<LoadoutMetadata, String>,
}

pub(in crate::ui) async fn fetch_loadout_metadata() -> LoadoutCatalogs {
    let api = match ValorantContentApi::shared() {
        Ok(api) => api,
        Err(error) => {
            return LoadoutCatalogs {
                weapon_content: Err(error.clone()),
                battle_pass: Err(error),
            };
        }
    };
    let (weapon_content, contracts, accessories, currencies) = join4(
        WEAPON_CONTENT.get_or_fetch(|| api.weapon_content()),
        CONTRACT_CATALOG.get_or_fetch(|| api.contract_catalog()),
        ACCESSORY_CATALOG.get_or_fetch(|| api.accessory_catalog()),
        CURRENCY_CATALOG.get_or_fetch(|| api.currency_catalog()),
    )
    .await;
    let battle_pass = weapon_content.clone().and_then(|weapon_content| {
        Ok(LoadoutMetadata {
            weapon_content,
            contracts: contracts?,
            accessories: accessories?,
            currencies: currencies?,
        })
    });

    LoadoutCatalogs {
        weapon_content,
        battle_pass,
    }
}

/// Downloads every rank's icon into the image cache, keyed by tier. Icons that fail are left out.
pub(in crate::ui) async fn cache_rank_icons(
    image_cache: ImageCache,
) -> Result<HashMap<i64, PathBuf>, String> {
    let urls = ValorantContentApi::shared()?
        .rank_icon_urls()
        .await
        .map_err(|error| error.to_string())?;
    let image_cache = &image_cache;

    Ok(stream::iter(urls)
        .map(|(tier, url)| async move {
            let path = cached_icon(image_cache, "ranks", &tier.to_string(), Some(&url)).await;
            path.map(|path| (tier, path))
        })
        .buffer_unordered(ICON_DOWNLOADS_AT_ONCE)
        .filter_map(std::future::ready)
        .collect()
        .await)
}

/// A player card's art in the image cache: the square small art for avatars and the wide art for
/// the Accounts banner. Either is `None` when it could not be downloaded.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct PlayerCardArt {
    pub(in crate::ui) small: Option<PathBuf>,
    pub(in crate::ui) wide: Option<PathBuf>,
}

/// Caches a player card's art. valorant-api.com serves each card's `smallArt` and `wideArt` at
/// fixed media URLs, so this skips downloading the whole player card catalog, and a card already
/// cached needs no request at all.
pub(in crate::ui) async fn cache_player_card_art(
    image_cache: ImageCache,
    card_id: String,
) -> PlayerCardArt {
    // The ID comes from Riot; anything that isn't a UUID has no art to fetch.
    if card_id.is_empty() || !card_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return PlayerCardArt::default();
    }
    let url =
        |kind: &str| format!("https://media.valorant-api.com/playercards/{card_id}/{kind}.png");
    let (small_url, wide_url) = (url("smallart"), url("wideart"));
    let (small, wide) = iced::futures::join!(
        cached_icon(&image_cache, "playercards", &card_id, Some(&small_url)),
        cached_icon(&image_cache, "playercards-wide", &card_id, Some(&wide_url)),
    );

    PlayerCardArt {
        small,
        wide: wide.and_then(|path| blurred_copy(&path)),
    }
}

/// The wide art is 452x128 and the banner shows it upscaled, so a slight blur makes the softness
/// look intentional. The blurred copy is saved next to the original and made once.
fn blurred_copy(path: &std::path::Path) -> Option<PathBuf> {
    let blurred = path.with_extension("blur.png");
    if blurred.exists() {
        // Keeps a copy in use from expiring, as `ImageCache::cache_url` does for downloads.
        let _ = std::fs::File::options()
            .write(true)
            .open(&blurred)
            .and_then(|file| file.set_modified(std::time::SystemTime::now()));
    } else {
        let mut png = std::io::Cursor::new(Vec::new());
        image::open(path)
            .ok()?
            .blur(1.2)
            .write_to(&mut png, image::ImageFormat::Png)
            .ok()?;
        // Written atomically, so an interrupted write can't leave a broken copy behind.
        crate::image_cache::write_cache_file(&blurred, png.get_ref()).ok()?;
    }
    Some(blurred)
}

pub(in crate::ui) async fn fetch_current_client_version() -> Result<String, String> {
    ValorantContentApi::shared()?
        .client_version()
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use iced::futures::executor::block_on;

    use super::*;

    #[test]
    fn wide_art_is_blurred_once_into_a_sibling_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let art = dir.path().join("card.png");
        image::RgbaImage::new(8, 4).save(&art).expect("save art");

        let blurred = blurred_copy(&art).expect("blurred copy");

        assert_eq!(blurred, dir.path().join("card.blur.png"));
        assert_eq!(
            image::image_dimensions(&blurred).expect("read blurred"),
            (8, 4)
        );
        assert_eq!(blurred_copy(&art), Some(blurred));
    }

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

    /// Pending once, so every download that has started is in flight at the same time.
    struct YieldOnce(bool);

    impl Future for YieldOnce {
        type Output = ();

        fn poll(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<()> {
            if self.0 {
                return std::task::Poll::Ready(());
            }
            self.0 = true;
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    }

    #[test]
    fn loads_that_start_together_share_one_download() {
        let cache = CachedCatalog::<u32>::new();
        let fetches = Cell::new(0);
        let fetch = || {
            fetches.set(fetches.get() + 1);
            async {
                YieldOnce(false).await;
                Ok(5)
            }
        };

        let (first, second) = block_on(iced::futures::future::join(
            cache.get_or_fetch(fetch),
            cache.get_or_fetch(fetch),
        ));

        assert_eq!((*first.expect("first"), *second.expect("second")), (5, 5));
        assert_eq!(fetches.get(), 1);
    }

    #[test]
    fn icon_downloads_run_at_the_same_time() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let in_flight = AtomicUsize::new(0);
        let most_in_flight = AtomicUsize::new(0);
        let downloads = (0..4)
            .map(|_| {
                Box::pin(async {
                    let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                    most_in_flight.fetch_max(now, Ordering::SeqCst);
                    YieldOnce(false).await;
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                }) as IconDownload<'_>
            })
            .collect();

        block_on(download_icons(downloads));

        assert_eq!(most_in_flight.load(Ordering::SeqCst), 4);
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
