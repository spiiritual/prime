use super::*;

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

pub(in crate::ui) async fn cache_store_images(
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

pub(in crate::ui) async fn cache_loadout_images(
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

pub(in crate::ui) async fn cache_skin_icon(
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

pub(in crate::ui) async fn cache_weapon_icon(
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

pub(in crate::ui) async fn cache_accessory_icon(
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

pub(in crate::ui) async fn cache_bundle_icon(
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

pub(in crate::ui) async fn cache_battle_pass_reward_icon(
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
