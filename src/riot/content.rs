use std::collections::HashMap;

use serde::{Deserialize, de::DeserializeOwned};
use thiserror::Error;

const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
const USER_AGENT_VALUE: &str = concat!("prime/", env!("CARGO_PKG_VERSION"));

pub const WEAPONS_URL: &str = "https://valorant-api.com/v1/weapons";
pub const WEAPON_SKINS_URL: &str = "https://valorant-api.com/v1/weapons/skins";
pub const BUNDLES_URL: &str = "https://valorant-api.com/v1/bundles";
pub const CONTENT_TIERS_URL: &str = "https://valorant-api.com/v1/contenttiers";
pub const CURRENCIES_URL: &str = "https://valorant-api.com/v1/currencies";
pub const BUDDIES_URL: &str = "https://valorant-api.com/v1/buddies";
pub const SPRAYS_URL: &str = "https://valorant-api.com/v1/sprays";
pub const PLAYER_CARDS_URL: &str = "https://valorant-api.com/v1/playercards";
pub const PLAYER_TITLES_URL: &str = "https://valorant-api.com/v1/playertitles";
pub const CONTRACTS_URL: &str = "https://valorant-api.com/v1/contracts";
pub const VERSION_URL: &str = "https://valorant-api.com/v1/version";

#[derive(Clone)]
pub struct ValorantContentApi {
    client: reqwest::Client,
}

impl ValorantContentApi {
    pub fn new() -> Result<Self, ContentError> {
        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent(USER_AGENT_VALUE)
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
        let tiers = self.content_tier_catalog().await?;

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

    pub async fn accessory_catalog(&self) -> Result<AccessoryCatalog, ContentError> {
        let buddies = self.content_data::<Vec<Buddy>>(BUDDIES_URL).await?;
        let sprays = self.content_data::<Vec<Spray>>(SPRAYS_URL).await?;
        let player_cards = self
            .content_data::<Vec<PlayerCard>>(PLAYER_CARDS_URL)
            .await?;
        let player_titles = self
            .content_data::<Vec<PlayerTitle>>(PLAYER_TITLES_URL)
            .await?;

        Ok(AccessoryCatalog::from_parts(
            buddies,
            sprays,
            player_cards,
            player_titles,
        ))
    }

    pub async fn contract_catalog(&self) -> Result<ContractCatalog, ContentError> {
        let contracts = self
            .content_data::<Vec<ValorantContract>>(CONTRACTS_URL)
            .await?;

        Ok(ContractCatalog::from_contracts(contracts))
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

    async fn content_data<T>(&self, url: &str) -> Result<T, ContentError>
    where
        T: DeserializeOwned,
    {
        let response: ApiResponse<T> = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(response.data)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContractCatalog {
    by_uuid: HashMap<String, ResolvedContract>,
    season_contracts: HashMap<String, String>,
}

impl ContractCatalog {
    pub fn from_contracts(contracts: Vec<ValorantContract>) -> Self {
        let mut by_uuid = HashMap::new();
        let mut season_contracts = HashMap::new();

        for contract in contracts {
            let uuid = contract.uuid.unwrap_or_default();
            let content = contract.content.unwrap_or_default();
            let free_reward_schedule_uuid = contract.free_reward_schedule_uuid;
            let premium_reward_schedule_uuid = content.premium_reward_schedule_uuid.clone();
            let relation_type = content.relation_type.unwrap_or_default();
            let relation_uuid = content.relation_uuid.unwrap_or_default();
            let mut level_xp = Vec::new();
            let mut reward_levels = Vec::new();
            let mut tier = 0_i64;

            for (chapter_index, chapter) in content.chapters.into_iter().enumerate() {
                let chapter_number = i64::try_from(chapter_index + 1).unwrap_or(i64::MAX);
                let mut chapter_level_indexes = Vec::new();

                for (level_index, level) in chapter.levels.into_iter().enumerate() {
                    tier = tier.saturating_add(1);
                    level_xp.push(level.xp.unwrap_or(0));

                    chapter_level_indexes.push(reward_levels.len());
                    reward_levels.push(ResolvedContractRewardLevel {
                        tier,
                        chapter: chapter_number,
                        level_in_chapter: i64::try_from(level_index + 1).unwrap_or(i64::MAX),
                        is_epilogue: chapter.is_epilogue,
                        premium_reward: level.reward.map(ResolvedContractReward::from),
                        free_rewards: Vec::new(),
                    });
                }

                if let (Some(last_level), Some(free_rewards)) =
                    (chapter_level_indexes.last().copied(), chapter.free_rewards)
                {
                    reward_levels[last_level].free_rewards = free_rewards
                        .into_iter()
                        .map(ResolvedContractReward::from)
                        .collect();
                }
            }

            if relation_type.eq_ignore_ascii_case("season") && !relation_uuid.trim().is_empty() {
                season_contracts.insert(normalize_uuid(&relation_uuid), normalize_uuid(&uuid));
            }

            by_uuid.insert(
                normalize_uuid(&uuid),
                ResolvedContract {
                    uuid,
                    display_name: contract.display_name.unwrap_or_default(),
                    relation_type,
                    relation_uuid,
                    level_xp,
                    reward_levels,
                    free_reward_schedule_uuid,
                    premium_reward_schedule_uuid,
                },
            );
        }

        Self {
            by_uuid,
            season_contracts,
        }
    }

    pub fn resolve(&self, uuid: &str) -> Option<&ResolvedContract> {
        self.by_uuid.get(&normalize_uuid(uuid))
    }

    pub fn resolve_active_season(&self, season_uuid: &str) -> Option<&ResolvedContract> {
        self.season_contracts
            .get(&normalize_uuid(season_uuid))
            .and_then(|contract_uuid| self.by_uuid.get(contract_uuid))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedContract {
    pub uuid: String,
    pub display_name: String,
    pub relation_type: String,
    pub relation_uuid: String,
    pub level_xp: Vec<i64>,
    pub reward_levels: Vec<ResolvedContractRewardLevel>,
    pub free_reward_schedule_uuid: Option<String>,
    pub premium_reward_schedule_uuid: Option<String>,
}

impl ResolvedContract {
    pub fn is_season_contract(&self) -> bool {
        self.relation_type.eq_ignore_ascii_case("season")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedContractRewardLevel {
    pub tier: i64,
    pub chapter: i64,
    pub level_in_chapter: i64,
    pub is_epilogue: bool,
    pub premium_reward: Option<ResolvedContractReward>,
    pub free_rewards: Vec<ResolvedContractReward>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedContractReward {
    pub kind: String,
    pub uuid: String,
    pub amount: i64,
    pub highlighted: bool,
}

impl From<ContractReward> for ResolvedContractReward {
    fn from(reward: ContractReward) -> Self {
        Self {
            kind: reward.kind,
            uuid: reward.uuid,
            amount: reward.amount,
            highlighted: reward.highlighted,
        }
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
                level_label: None,
            };
            by_uuid.insert(normalize_uuid(&skin.uuid), skin_info.clone());

            for (level_index, level) in skin.levels.into_iter().enumerate() {
                let display_name =
                    display_name_with_fallback(&level.display_name, &skin.display_name);

                by_uuid.insert(
                    normalize_uuid(&level.uuid),
                    ResolvedSkin {
                        uuid: level.uuid,
                        level_label: Some(skin_level_label(
                            &display_name,
                            &skin.display_name,
                            level_index + 1,
                        )),
                        display_name,
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
                            .full_render
                            .or(chroma.display_icon)
                            .or_else(|| skin_info.display_icon.clone()),
                        rarity: rarity.clone(),
                        level_label: None,
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
    pub level_label: Option<String>,
}

impl ResolvedSkin {
    pub fn unknown(uuid: &str) -> Self {
        Self {
            uuid: uuid.to_string(),
            display_name: uuid.to_string(),
            display_icon: None,
            rarity: None,
            level_label: None,
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AccessoryCatalog {
    by_uuid: HashMap<String, ResolvedAccessory>,
}

impl AccessoryCatalog {
    pub fn from_parts(
        buddies: Vec<Buddy>,
        sprays: Vec<Spray>,
        player_cards: Vec<PlayerCard>,
        player_titles: Vec<PlayerTitle>,
    ) -> Self {
        let mut by_uuid = HashMap::new();

        for buddy in buddies {
            let buddy_icon = buddy.display_icon.clone();
            by_uuid.insert(
                normalize_uuid(&buddy.uuid),
                ResolvedAccessory {
                    uuid: buddy.uuid.clone(),
                    display_name: buddy.display_name.clone(),
                    display_icon: buddy_icon.clone(),
                },
            );

            for level in buddy.levels {
                by_uuid.insert(
                    normalize_uuid(&level.uuid),
                    ResolvedAccessory {
                        uuid: level.uuid,
                        display_name: display_name_with_fallback(
                            &level.display_name,
                            &buddy.display_name,
                        ),
                        display_icon: level.display_icon.or_else(|| buddy_icon.clone()),
                    },
                );
            }
        }

        for spray in sprays {
            by_uuid.insert(
                normalize_uuid(&spray.uuid),
                ResolvedAccessory {
                    uuid: spray.uuid,
                    display_name: spray.display_name,
                    display_icon: spray
                        .full_transparent_icon
                        .or(spray.full_icon)
                        .or(spray.display_icon),
                },
            );
        }

        for card in player_cards {
            by_uuid.insert(
                normalize_uuid(&card.uuid),
                ResolvedAccessory {
                    uuid: card.uuid,
                    display_name: card.display_name,
                    display_icon: card.display_icon.or(card.small_art).or(card.large_art),
                },
            );
        }

        for title in player_titles {
            let fallback = title.title_text.as_deref().unwrap_or(&title.uuid);
            let display_name = title.display_name.as_deref().unwrap_or_default();
            by_uuid.insert(
                normalize_uuid(&title.uuid),
                ResolvedAccessory {
                    uuid: title.uuid.clone(),
                    display_name: display_name_with_fallback(display_name, fallback),
                    display_icon: None,
                },
            );
        }

        Self { by_uuid }
    }

    pub fn resolve(&self, uuid: &str) -> ResolvedAccessory {
        self.by_uuid
            .get(&normalize_uuid(uuid))
            .cloned()
            .unwrap_or_else(|| ResolvedAccessory::unknown(uuid))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedAccessory {
    pub uuid: String,
    pub display_name: String,
    pub display_icon: Option<String>,
}

impl ResolvedAccessory {
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

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ValorantContract {
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(default, rename = "freeRewardScheduleUuid")]
    pub free_reward_schedule_uuid: Option<String>,
    #[serde(default)]
    pub content: Option<ContractContent>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ContractContent {
    #[serde(default, rename = "relationType")]
    pub relation_type: Option<String>,
    #[serde(default, rename = "relationUuid")]
    pub relation_uuid: Option<String>,
    #[serde(default)]
    pub chapters: Vec<ContractChapter>,
    #[serde(default, rename = "premiumRewardScheduleUuid")]
    pub premium_reward_schedule_uuid: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ContractChapter {
    #[serde(default, rename = "isEpilogue")]
    pub is_epilogue: bool,
    #[serde(default)]
    pub levels: Vec<ContractLevel>,
    #[serde(default, rename = "freeRewards")]
    pub free_rewards: Option<Vec<ContractReward>>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ContractLevel {
    #[serde(default)]
    pub reward: Option<ContractReward>,
    #[serde(default)]
    pub xp: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ContractReward {
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub amount: i64,
    #[serde(default, rename = "isHighlighted")]
    pub highlighted: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Buddy {
    pub uuid: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "displayIcon")]
    pub display_icon: Option<String>,
    #[serde(default)]
    pub levels: Vec<BuddyLevel>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct BuddyLevel {
    pub uuid: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "displayIcon")]
    pub display_icon: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Spray {
    pub uuid: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "displayIcon")]
    pub display_icon: Option<String>,
    #[serde(rename = "fullIcon")]
    pub full_icon: Option<String>,
    #[serde(rename = "fullTransparentIcon")]
    pub full_transparent_icon: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct PlayerCard {
    pub uuid: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "displayIcon")]
    pub display_icon: Option<String>,
    #[serde(rename = "smallArt")]
    pub small_art: Option<String>,
    #[serde(rename = "largeArt")]
    pub large_art: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct PlayerTitle {
    pub uuid: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "titleText")]
    pub title_text: Option<String>,
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

fn skin_level_label(level_name: &str, skin_name: &str, level_number: usize) -> String {
    let level_name = level_name.trim();
    let skin_name = skin_name.trim();

    if let Some(rest) = strip_prefix_ignore_ascii_case(level_name, skin_name) {
        let rest = rest
            .trim_start_matches(|ch: char| ch.is_whitespace() || ch == '-' || ch == ':')
            .trim();

        if !rest.is_empty() {
            return rest.to_string();
        }
    } else if !level_name.is_empty() {
        return level_name.to_string();
    }

    format!("Level {level_number}")
}

fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let value_prefix = value.get(..prefix.len())?;

    value_prefix
        .eq_ignore_ascii_case(prefix)
        .then_some(&value[prefix.len()..])
}

#[derive(Debug, Error)]
pub enum ContentError {
    #[error(
        "Valorant content API HTTP error: {}",
        crate::http_error::format_reqwest_error(.0)
    )]
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
            catalog.resolve("level-uuid").level_label.as_deref(),
            Some("Level 4")
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
    fn prefers_chroma_full_render_over_display_icon() {
        let catalog = SkinCatalog::from_skins(vec![WeaponSkin {
            uuid: "skin-uuid".to_string(),
            display_name: "Standard Vandal".to_string(),
            display_icon: Some("skin-icon".to_string()),
            content_tier_uuid: None,
            levels: vec![],
            chromas: vec![WeaponSkinChroma {
                uuid: "chroma-uuid".to_string(),
                display_name: "Standard Vandal".to_string(),
                display_icon: Some("display-icon".to_string()),
                full_render: Some("full-render".to_string()),
            }],
        }]);

        assert_eq!(
            catalog.resolve("chroma-uuid").display_icon.as_deref(),
            Some("full-render")
        );
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
    fn resolves_active_season_contracts() {
        let catalog = ContractCatalog::from_contracts(vec![ValorantContract {
            uuid: Some("contract-uuid".to_string()),
            display_name: Some("Season 2026 // Act III".to_string()),
            free_reward_schedule_uuid: Some("free-schedule".to_string()),
            content: Some(ContractContent {
                relation_type: Some("Season".to_string()),
                relation_uuid: Some("act-uuid".to_string()),
                premium_reward_schedule_uuid: Some("premium-schedule".to_string()),
                chapters: vec![ContractChapter {
                    is_epilogue: false,
                    levels: vec![
                        ContractLevel {
                            reward: None,
                            xp: Some(0),
                        },
                        ContractLevel {
                            reward: None,
                            xp: Some(2_000),
                        },
                        ContractLevel {
                            reward: None,
                            xp: Some(2_750),
                        },
                    ],
                    free_rewards: None,
                }],
            }),
        }]);

        let contract = catalog.resolve_active_season("ACT-UUID").expect("contract");

        assert_eq!(contract.uuid, "contract-uuid");
        assert_eq!(contract.display_name, "Season 2026 // Act III");
        assert_eq!(contract.level_xp, [0, 2_000, 2_750]);
    }

    #[test]
    fn resolves_accessory_ids_to_display_names_and_icons() {
        let catalog = AccessoryCatalog::from_parts(
            vec![Buddy {
                uuid: "buddy-uuid".to_string(),
                display_name: "Penguin Buddy".to_string(),
                display_icon: Some("buddy-icon".to_string()),
                levels: vec![BuddyLevel {
                    uuid: "buddy-level-uuid".to_string(),
                    display_name: "Penguin Buddy Level 1".to_string(),
                    display_icon: None,
                }],
            }],
            vec![Spray {
                uuid: "spray-uuid".to_string(),
                display_name: "Penguin Spray".to_string(),
                display_icon: None,
                full_icon: Some("spray-icon".to_string()),
                full_transparent_icon: None,
            }],
            vec![PlayerCard {
                uuid: "card-uuid".to_string(),
                display_name: "Penguin Card".to_string(),
                display_icon: Some("card-icon".to_string()),
                small_art: None,
                large_art: None,
            }],
            vec![PlayerTitle {
                uuid: "title-uuid".to_string(),
                display_name: Some("Penguin".to_string()),
                title_text: Some("Penguin".to_string()),
            }],
        );

        let buddy = catalog.resolve("BUDDY-LEVEL-UUID");
        let spray = catalog.resolve("spray-uuid");
        let card = catalog.resolve("card-uuid");
        let title = catalog.resolve("title-uuid");
        let unknown = catalog.resolve("missing");

        assert_eq!(buddy.display_name, "Penguin Buddy Level 1");
        assert_eq!(buddy.display_icon.as_deref(), Some("buddy-icon"));
        assert_eq!(spray.display_icon.as_deref(), Some("spray-icon"));
        assert_eq!(card.display_name, "Penguin Card");
        assert_eq!(title.display_name, "Penguin");
        assert_eq!(unknown.display_name, "missing");
    }

    #[test]
    fn tolerates_nullable_player_title_names() {
        let response: ApiResponse<Vec<PlayerTitle>> = serde_json::from_value(serde_json::json!({
            "status": 200,
            "data": [{
                "uuid": "title-uuid",
                "displayName": null,
                "titleText": null
            }],
        }))
        .expect("player titles response");

        let catalog = AccessoryCatalog::from_parts(vec![], vec![], vec![], response.data);
        let title = catalog.resolve("title-uuid");

        assert_eq!(title.display_name, "title-uuid");
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
