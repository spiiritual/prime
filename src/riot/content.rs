use std::collections::HashMap;

use serde::Deserialize;
use thiserror::Error;

pub const WEAPONS_URL: &str = "https://valorant-api.com/v1/weapons";
pub const WEAPON_SKINS_URL: &str = "https://valorant-api.com/v1/weapons/skins";
pub const BUNDLES_URL: &str = "https://valorant-api.com/v1/bundles";
pub const CONTENT_TIERS_URL: &str = "https://valorant-api.com/v1/contenttiers";
pub const CURRENCIES_URL: &str = "https://valorant-api.com/v1/currencies";
pub const VERSION_URL: &str = "https://valorant-api.com/v1/version";

#[derive(Clone)]
pub struct ValorantContentApi {
    client: reqwest::Client,
}

impl ValorantContentApi {
    pub fn new() -> Result<Self, ContentError> {
        let client = reqwest::Client::builder()
            .user_agent("prime-valorant-manager/0.1")
            .build()?;

        Ok(Self { client })
    }

    pub async fn skin_catalog(&self) -> Result<SkinCatalog, ContentError> {
        let response: ApiResponse<Vec<WeaponSkin>> = self
            .client
            .get(WEAPON_SKINS_URL)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let tiers = self.content_tier_catalog().await.unwrap_or_default();

        Ok(SkinCatalog::from_skins_and_tiers(response.data, &tiers))
    }

    pub async fn weapon_catalog(&self) -> Result<WeaponCatalog, ContentError> {
        let response: ApiResponse<Vec<Weapon>> = self
            .client
            .get(WEAPONS_URL)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(WeaponCatalog::from_weapons(response.data))
    }

    pub async fn currency_catalog(&self) -> Result<CurrencyCatalog, ContentError> {
        let response: ApiResponse<Vec<Currency>> = self
            .client
            .get(CURRENCIES_URL)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(CurrencyCatalog::from_currencies(response.data))
    }

    pub async fn bundle_catalog(&self) -> Result<BundleCatalog, ContentError> {
        let response: ApiResponse<Vec<Bundle>> = self
            .client
            .get(BUNDLES_URL)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(BundleCatalog::from_bundles(response.data))
    }

    pub async fn content_tier_catalog(&self) -> Result<ContentTierCatalog, ContentError> {
        let response: ApiResponse<Vec<ContentTier>> = self
            .client
            .get(CONTENT_TIERS_URL)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(ContentTierCatalog::from_tiers(response.data))
    }

    pub async fn client_version(&self) -> Result<String, ContentError> {
        let response: ApiResponse<ValorantVersion> = self
            .client
            .get(VERSION_URL)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(response.data.riot_client_version)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WeaponCatalog {
    by_uuid: HashMap<String, ResolvedWeapon>,
}

impl WeaponCatalog {
    pub fn from_weapons(weapons: Vec<Weapon>) -> Self {
        let by_uuid = weapons
            .into_iter()
            .map(|weapon| {
                (
                    normalize_uuid(&weapon.uuid),
                    ResolvedWeapon {
                        uuid: weapon.uuid,
                        display_name: weapon.display_name,
                        display_icon: weapon.display_icon,
                    },
                )
            })
            .collect();

        Self { by_uuid }
    }

    pub fn resolve(&self, uuid: &str) -> ResolvedWeapon {
        self.by_uuid
            .get(&normalize_uuid(uuid))
            .cloned()
            .unwrap_or_else(|| ResolvedWeapon::unknown(uuid))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedWeapon {
    pub uuid: String,
    pub display_name: String,
    pub display_icon: Option<String>,
}

impl ResolvedWeapon {
    pub fn unknown(uuid: &str) -> Self {
        Self {
            uuid: uuid.to_string(),
            display_name: uuid.to_string(),
            display_icon: None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SkinCatalog {
    by_uuid: HashMap<String, ResolvedSkin>,
}

impl SkinCatalog {
    pub fn from_skins(skins: Vec<WeaponSkin>) -> Self {
        Self::from_skins_and_tiers(skins, &ContentTierCatalog::default())
    }

    pub fn from_skins_and_tiers(skins: Vec<WeaponSkin>, tiers: &ContentTierCatalog) -> Self {
        let mut by_uuid = HashMap::new();

        for skin in skins {
            let rarity = skin
                .content_tier_uuid
                .as_ref()
                .and_then(|uuid| tiers.resolve_name(uuid));
            let skin_info = ResolvedSkin {
                uuid: skin.uuid.clone(),
                display_name: skin.display_name.clone(),
                display_icon: skin.display_icon.clone(),
                rarity: rarity.clone(),
            };
            by_uuid.insert(normalize_uuid(&skin.uuid), skin_info.clone());

            for level in skin.levels {
                by_uuid.insert(
                    normalize_uuid(&level.uuid),
                    ResolvedSkin {
                        uuid: level.uuid,
                        display_name: display_name_with_fallback(
                            &level.display_name,
                            &skin.display_name,
                        ),
                        display_icon: level.display_icon.or_else(|| skin.display_icon.clone()),
                        rarity: rarity.clone(),
                    },
                );
            }

            for chroma in skin.chromas {
                by_uuid.insert(
                    normalize_uuid(&chroma.uuid),
                    ResolvedSkin {
                        uuid: chroma.uuid,
                        display_name: display_name_with_fallback(
                            &chroma.display_name,
                            &skin.display_name,
                        ),
                        display_icon: chroma
                            .display_icon
                            .or(chroma.full_render)
                            .or_else(|| skin_info.display_icon.clone()),
                        rarity: rarity.clone(),
                    },
                );
            }
        }

        Self { by_uuid }
    }

    pub fn resolve(&self, uuid: &str) -> ResolvedSkin {
        self.by_uuid
            .get(&normalize_uuid(uuid))
            .cloned()
            .unwrap_or_else(|| ResolvedSkin::unknown(uuid))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedSkin {
    pub uuid: String,
    pub display_name: String,
    pub display_icon: Option<String>,
    pub rarity: Option<String>,
}

impl ResolvedSkin {
    pub fn unknown(uuid: &str) -> Self {
        Self {
            uuid: uuid.to_string(),
            display_name: uuid.to_string(),
            display_icon: None,
            rarity: None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContentTierCatalog {
    by_uuid: HashMap<String, String>,
}

impl ContentTierCatalog {
    pub fn from_tiers(tiers: Vec<ContentTier>) -> Self {
        let by_uuid = tiers
            .into_iter()
            .map(|tier| (normalize_uuid(&tier.uuid), tier.display_name))
            .collect();

        Self { by_uuid }
    }

    pub fn resolve_name(&self, uuid: &str) -> Option<String> {
        self.by_uuid.get(&normalize_uuid(uuid)).cloned()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CurrencyCatalog {
    by_uuid: HashMap<String, ResolvedCurrency>,
}

impl CurrencyCatalog {
    pub fn from_currencies(currencies: Vec<Currency>) -> Self {
        let by_uuid = currencies
            .into_iter()
            .map(|currency| {
                (
                    normalize_uuid(&currency.uuid),
                    ResolvedCurrency {
                        uuid: currency.uuid,
                        display_name: currency.display_name,
                        display_icon: currency.display_icon,
                    },
                )
            })
            .collect();

        Self { by_uuid }
    }

    pub fn resolve(&self, uuid: &str) -> ResolvedCurrency {
        self.by_uuid
            .get(&normalize_uuid(uuid))
            .cloned()
            .unwrap_or_else(|| ResolvedCurrency::unknown(uuid))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedCurrency {
    pub uuid: String,
    pub display_name: String,
    pub display_icon: Option<String>,
}

impl ResolvedCurrency {
    pub fn unknown(uuid: &str) -> Self {
        Self {
            uuid: uuid.to_string(),
            display_name: uuid.to_string(),
            display_icon: None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BundleCatalog {
    by_uuid: HashMap<String, ResolvedBundle>,
}

impl BundleCatalog {
    pub fn from_bundles(bundles: Vec<Bundle>) -> Self {
        let by_uuid = bundles
            .into_iter()
            .map(|bundle| {
                (
                    normalize_uuid(&bundle.uuid),
                    ResolvedBundle {
                        uuid: bundle.uuid,
                        display_name: bundle.display_name,
                        display_icon: bundle
                            .vertical_promo_image
                            .or(bundle.display_icon2)
                            .or(bundle.display_icon),
                    },
                )
            })
            .collect();

        Self { by_uuid }
    }

    pub fn resolve(&self, uuid: &str) -> ResolvedBundle {
        self.by_uuid
            .get(&normalize_uuid(uuid))
            .cloned()
            .unwrap_or_else(|| ResolvedBundle::unknown(uuid))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedBundle {
    pub uuid: String,
    pub display_name: String,
    pub display_icon: Option<String>,
}

impl ResolvedBundle {
    pub fn unknown(uuid: &str) -> Self {
        Self {
            uuid: uuid.to_string(),
            display_name: uuid.to_string(),
            display_icon: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct ApiResponse<T> {
    data: T,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Weapon {
    pub uuid: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "displayIcon")]
    pub display_icon: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct WeaponSkin {
    pub uuid: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "displayIcon")]
    pub display_icon: Option<String>,
    #[serde(rename = "contentTierUuid")]
    pub content_tier_uuid: Option<String>,
    #[serde(default)]
    pub levels: Vec<WeaponSkinLevel>,
    #[serde(default)]
    pub chromas: Vec<WeaponSkinChroma>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct WeaponSkinLevel {
    pub uuid: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "displayIcon")]
    pub display_icon: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct WeaponSkinChroma {
    pub uuid: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "displayIcon")]
    pub display_icon: Option<String>,
    #[serde(rename = "fullRender")]
    pub full_render: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ValorantVersion {
    #[serde(rename = "riotClientVersion")]
    pub riot_client_version: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Currency {
    pub uuid: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "displayIcon")]
    pub display_icon: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ContentTier {
    pub uuid: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Bundle {
    pub uuid: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "displayIcon")]
    pub display_icon: Option<String>,
    #[serde(rename = "displayIcon2")]
    pub display_icon2: Option<String>,
    #[serde(rename = "verticalPromoImage")]
    pub vertical_promo_image: Option<String>,
}

fn normalize_uuid(uuid: &str) -> String {
    uuid.trim().to_ascii_lowercase()
}

fn display_name_with_fallback(name: &str, fallback: &str) -> String {
    let trimmed = name.trim();

    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("standard") {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Debug, Error)]
pub enum ContentError {
    #[error("Valorant content API HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_skin_level_and_chroma_ids_to_display_names() {
        let catalog = SkinCatalog::from_skins(vec![WeaponSkin {
            uuid: "skin-uuid".to_string(),
            display_name: "Prime Vandal".to_string(),
            display_icon: Some("skin-icon".to_string()),
            content_tier_uuid: Some("premium".to_string()),
            levels: vec![WeaponSkinLevel {
                uuid: "level-uuid".to_string(),
                display_name: "Prime Vandal Level 4".to_string(),
                display_icon: None,
            }],
            chromas: vec![WeaponSkinChroma {
                uuid: "chroma-uuid".to_string(),
                display_name: "Prime Vandal Orange".to_string(),
                display_icon: None,
                full_render: Some("render".to_string()),
            }],
        }]);

        assert_eq!(catalog.resolve("SKIN-UUID").display_name, "Prime Vandal");
        assert_eq!(
            catalog.resolve("level-uuid").display_name,
            "Prime Vandal Level 4"
        );
        assert_eq!(
            catalog.resolve("chroma-uuid").display_name,
            "Prime Vandal Orange"
        );
        assert_eq!(
            catalog.resolve("chroma-uuid").display_icon.as_deref(),
            Some("render")
        );
        assert_eq!(catalog.resolve("skin-uuid").rarity.as_deref(), None);
    }

    #[test]
    fn resolves_skin_rarity_names() {
        let tiers = ContentTierCatalog::from_tiers(vec![ContentTier {
            uuid: "premium".to_string(),
            display_name: "Premium Edition".to_string(),
        }]);
        let catalog = SkinCatalog::from_skins_and_tiers(
            vec![WeaponSkin {
                uuid: "skin-uuid".to_string(),
                display_name: "Prime Vandal".to_string(),
                display_icon: None,
                content_tier_uuid: Some("premium".to_string()),
                levels: vec![],
                chromas: vec![],
            }],
            &tiers,
        );

        assert_eq!(
            catalog.resolve("skin-uuid").rarity.as_deref(),
            Some("Premium Edition")
        );
    }

    #[test]
    fn resolves_weapon_ids_to_display_names() {
        let catalog = WeaponCatalog::from_weapons(vec![Weapon {
            uuid: "weapon-uuid".to_string(),
            display_name: "Vandal".to_string(),
            display_icon: Some("weapon-icon".to_string()),
        }]);

        assert_eq!(catalog.resolve("WEAPON-UUID").display_name, "Vandal");
        assert_eq!(
            catalog.resolve("weapon-uuid").display_icon.as_deref(),
            Some("weapon-icon")
        );
        assert_eq!(catalog.resolve("missing").display_name, "missing");
    }

    #[test]
    fn unknown_skin_uses_uuid_as_display_name() {
        let catalog = SkinCatalog::default();

        assert_eq!(catalog.resolve("missing").display_name, "missing");
    }

    #[test]
    fn resolves_currency_ids_to_display_names() {
        let catalog = CurrencyCatalog::from_currencies(vec![Currency {
            uuid: "vp-uuid".to_string(),
            display_name: "VALORANT Points".to_string(),
            display_icon: Some("vp-icon".to_string()),
        }]);

        assert_eq!(catalog.resolve("VP-UUID").display_name, "VALORANT Points");
        assert_eq!(
            catalog.resolve("vp-uuid").display_icon.as_deref(),
            Some("vp-icon")
        );
        assert_eq!(catalog.resolve("missing").display_name, "missing");
    }

    #[test]
    fn resolves_bundle_ids_to_display_names_and_promo_images() {
        let catalog = BundleCatalog::from_bundles(vec![Bundle {
            uuid: "bundle-uuid".to_string(),
            display_name: "Give Back Bundle".to_string(),
            display_icon: Some("display-icon".to_string()),
            display_icon2: Some("display-icon-2".to_string()),
            vertical_promo_image: Some("vertical-promo".to_string()),
        }]);

        let bundle = catalog.resolve("BUNDLE-UUID");

        assert_eq!(bundle.display_name, "Give Back Bundle");
        assert_eq!(bundle.display_icon.as_deref(), Some("vertical-promo"));
        assert_eq!(catalog.resolve("missing").display_name, "missing");
    }

    #[test]
    fn deserializes_riot_client_version() {
        let response: ApiResponse<ValorantVersion> = serde_json::from_value(serde_json::json!({
            "status": 200,
            "data": {
                "riotClientVersion": "release-12.09-shipping-25-4697179"
            }
        }))
        .expect("version response");

        assert_eq!(
            response.data.riot_client_version,
            "release-12.09-shipping-25-4697179"
        );
    }
}
