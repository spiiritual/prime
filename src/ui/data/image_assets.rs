use super::*;

use super::loadout::{BattlePassRewardDisplay, LoadoutSummary, SkinDisplay, WeaponDisplay};
use super::shop::{AccessoryDisplay, BundleDisplay, StoreSummary};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct StoreMetadata {
    pub(in crate::ui) skins: SkinCatalog,
    pub(in crate::ui) bundles: BundleCatalog,
    pub(in crate::ui) currencies: CurrencyCatalog,
    pub(in crate::ui) accessories: AccessoryCatalog,
}

pub(in crate::ui) async fn fetch_store_metadata() -> Result<StoreMetadata, String> {
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
    pub(in crate::ui) skins: SkinCatalog,
    pub(in crate::ui) weapons: WeaponCatalog,
    pub(in crate::ui) contracts: ContractCatalog,
    pub(in crate::ui) accessories: AccessoryCatalog,
    pub(in crate::ui) currencies: CurrencyCatalog,
}

pub(in crate::ui) async fn fetch_loadout_metadata() -> Result<LoadoutMetadata, String> {
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

pub(in crate::ui) async fn fetch_current_client_version() -> Result<String, String> {
    ValorantContentApi::new()
        .map_err(|error| error.to_string())?
        .client_version()
        .await
        .map_err(|error| error.to_string())
}
