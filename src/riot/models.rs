use std::collections::HashMap;

use serde::{Deserialize, Deserializer};

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct EntitlementResponse {
    pub entitlements_token: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct PlayerInfoResponse {
    pub country: String,
    pub sub: String,
    pub acct: RiotAccount,
    #[serde(default)]
    pub affinity: HashMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RiotAccount {
    pub game_name: String,
    pub tag_line: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RiotGeoResponse {
    pub token: String,
    pub affinities: RiotGeoAffinities,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RiotGeoAffinities {
    pub pbe: String,
    pub live: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct AccountXpResponse {
    pub version: i64,
    pub subject: String,
    pub progress: AccountXpProgress,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct AccountXpProgress {
    pub level: i64,
    #[serde(rename = "XP")]
    pub xp: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct GameContentResponse {
    #[serde(default, rename = "DisabledIDs")]
    pub disabled_ids: Vec<serde_json::Value>,
    #[serde(default)]
    pub seasons: Vec<GameContentSeason>,
    #[serde(default)]
    pub events: Vec<GameContentEvent>,
}

impl GameContentResponse {
    pub fn active_act(&self) -> Option<&GameContentSeason> {
        self.seasons
            .iter()
            .find(|season| season.is_active && season.season_type.eq_ignore_ascii_case("act"))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct GameContentSeason {
    #[serde(rename = "ID")]
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub season_type: String,
    pub start_time: String,
    pub end_time: String,
    pub is_active: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct GameContentEvent {
    #[serde(rename = "ID")]
    pub id: String,
    pub name: String,
    pub start_time: String,
    pub end_time: String,
    pub is_active: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct PlayerMmrResponse {
    pub version: i64,
    pub subject: String,
    pub queue_skills: MmrQueueSkills,
    #[serde(default)]
    pub latest_competitive_update: Option<CompetitiveUpdate>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct MmrQueueSkills {
    #[serde(default, rename = "competitive")]
    pub competitive: Option<MmrQueueSkill>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct MmrQueueSkill {
    #[serde(default, rename = "SeasonalInfoBySeasonID")]
    pub seasonal_info_by_season_id: HashMap<String, MmrSeasonInfo>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct MmrSeasonInfo {
    #[serde(default, rename = "SeasonID")]
    pub season_id: String,
    #[serde(default)]
    pub competitive_tier: i64,
    #[serde(default)]
    pub ranked_rating: i64,
    #[serde(default)]
    pub number_of_games: i64,
    #[serde(default)]
    pub games_needed_for_rating: i64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct CompetitiveUpdate {
    #[serde(default, rename = "SeasonID")]
    pub season_id: String,
    #[serde(default)]
    pub tier_after_update: i64,
    #[serde(default)]
    pub ranked_rating_after_update: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ContractsResponse {
    pub version: i64,
    pub subject: String,
    #[serde(default)]
    pub contracts: Vec<PlayerContract>,
    #[serde(default)]
    pub active_special_contract: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct PlayerContract {
    #[serde(default, rename = "ContractDefinitionID")]
    pub contract_definition_id: String,
    #[serde(default)]
    pub contract_progression: ContractProgression,
    #[serde(default)]
    pub progression_level_reached: i64,
    #[serde(default)]
    pub progression_towards_next_level: i64,
    #[serde(default)]
    pub progression_completed: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ContractProgression {
    #[serde(default)]
    pub total_progression_earned: i64,
    #[serde(default)]
    pub total_progression_earned_version: i64,
    #[serde(default)]
    pub highest_rewarded_level: HashMap<String, ContractRewardedLevel>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ContractRewardedLevel {
    #[serde(default)]
    pub amount: i64,
    #[serde(default)]
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct StorefrontResponse {
    pub featured_bundle: FeaturedBundle,
    pub skins_panel_layout: SkinsPanelLayout,
    #[serde(default)]
    pub bonus_store: Option<BonusStore>,
    #[serde(default)]
    pub accessory_store: Option<AccessoryStore>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct WalletResponse {
    #[serde(default)]
    pub balances: HashMap<String, i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct FeaturedBundle {
    pub bundle: StoreBundle,
    #[serde(default)]
    pub bundles: Vec<StoreBundle>,
    pub bundle_remaining_duration_in_seconds: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct StoreBundle {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "DataAssetID")]
    pub data_asset_id: String,
    #[serde(rename = "CurrencyID")]
    pub currency_id: String,
    #[serde(default)]
    pub items: Vec<BundleItem>,
    #[serde(default)]
    pub total_base_cost: Option<HashMap<String, i64>>,
    #[serde(default)]
    pub total_discounted_cost: Option<HashMap<String, i64>>,
    #[serde(default)]
    pub duration_remaining_in_seconds: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct BundleItem {
    pub item: StoreItem,
    pub base_price: i64,
    #[serde(rename = "CurrencyID")]
    pub currency_id: String,
    #[serde(deserialize_with = "deserialize_discount_percent")]
    pub discount_percent: i64,
    pub discounted_price: i64,
    pub is_promo_item: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct StoreItem {
    #[serde(rename = "ItemTypeID")]
    pub item_type_id: String,
    #[serde(rename = "ItemID")]
    pub item_id: String,
    pub amount: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct SkinsPanelLayout {
    #[serde(default)]
    pub single_item_offers: Vec<String>,
    #[serde(default)]
    pub single_item_store_offers: Vec<StoreOffer>,
    pub single_item_offers_remaining_duration_in_seconds: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct StoreOffer {
    #[serde(rename = "OfferID")]
    pub offer_id: String,
    pub is_direct_purchase: bool,
    pub start_date: String,
    #[serde(default)]
    pub cost: HashMap<String, i64>,
    #[serde(default)]
    pub rewards: Vec<Reward>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Reward {
    #[serde(rename = "ItemTypeID")]
    pub item_type_id: String,
    #[serde(rename = "ItemID")]
    pub item_id: String,
    pub quantity: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct BonusStore {
    #[serde(default)]
    pub bonus_store_offers: Vec<BonusStoreOffer>,
    pub bonus_store_remaining_duration_in_seconds: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct BonusStoreOffer {
    #[serde(rename = "BonusOfferID")]
    pub bonus_offer_id: String,
    pub offer: StoreOffer,
    #[serde(deserialize_with = "deserialize_discount_percent")]
    pub discount_percent: i64,
    #[serde(default)]
    pub discount_costs: HashMap<String, i64>,
    pub is_seen: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct AccessoryStore {
    #[serde(default)]
    pub accessory_store_offers: Vec<AccessoryStoreOffer>,
    #[serde(default)]
    pub accessory_store_remaining_duration_in_seconds: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct AccessoryStoreOffer {
    pub offer: StoreOffer,
}

fn deserialize_discount_percent<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Number::deserialize(deserializer)?;

    if let Some(percent) = value.as_i64() {
        return Ok(percent);
    }

    if let Some(percent) = value.as_u64() {
        return i64::try_from(percent)
            .map_err(|_| serde::de::Error::custom("discount percent is too large"));
    }

    let Some(percent) = value.as_f64() else {
        return Err(serde::de::Error::custom("discount percent is not a number"));
    };

    if !percent.is_finite() {
        return Err(serde::de::Error::custom("discount percent is not finite"));
    }

    let normalized = if percent.abs() <= 1.0 {
        percent * 100.0
    } else {
        percent
    };

    Ok(normalized.round() as i64)
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct PlayerLoadoutResponse {
    pub subject: String,
    pub version: i64,
    #[serde(default)]
    pub guns: Vec<LoadoutGun>,
    #[serde(default)]
    pub sprays: Vec<LoadoutSpray>,
    pub identity: LoadoutIdentity,
    pub incognito: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct LoadoutGun {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "CharmInstanceID")]
    pub charm_instance_id: Option<String>,
    #[serde(rename = "CharmID")]
    pub charm_id: Option<String>,
    #[serde(rename = "CharmLevelID")]
    pub charm_level_id: Option<String>,
    #[serde(rename = "SkinID")]
    pub skin_id: String,
    #[serde(rename = "SkinLevelID")]
    pub skin_level_id: String,
    #[serde(rename = "ChromaID")]
    pub chroma_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct LoadoutSpray {
    #[serde(rename = "EquipSlotID")]
    pub equip_slot_id: String,
    #[serde(rename = "SprayID")]
    pub spray_id: String,
    #[serde(rename = "SprayLevelID")]
    pub spray_level_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct LoadoutIdentity {
    #[serde(rename = "PlayerCardID")]
    pub player_card_id: String,
    #[serde(rename = "PlayerTitleID")]
    pub player_title_id: String,
    #[serde(rename = "AccountLevel")]
    pub account_level: i64,
    #[serde(rename = "PreferredLevelBorderID")]
    pub preferred_level_border_id: String,
    #[serde(rename = "HideAccountLevel")]
    pub hide_account_level: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_storefront_offer_ids() {
        let json = serde_json::json!({
            "FeaturedBundle": {
                "Bundle": {
                    "ID": "bundle",
                    "DataAssetID": "asset",
                    "CurrencyID": "vp",
                    "Items": [],
                    "DurationRemainingInSeconds": 10
                },
                "Bundles": [],
                "BundleRemainingDurationInSeconds": 20
            },
            "SkinsPanelLayout": {
                "SingleItemOffers": ["offer-a"],
                "SingleItemStoreOffers": [{
                    "OfferID": "offer-a",
                    "IsDirectPurchase": true,
                    "StartDate": "2026-05-25T00:00:00Z",
                    "Cost": {"vp": 1775},
                    "Rewards": [{
                        "ItemTypeID": "skin-type",
                        "ItemID": "skin",
                        "Quantity": 1
                    }]
                }],
                "SingleItemOffersRemainingDurationInSeconds": 86400
            }
        });

        let storefront: StorefrontResponse =
            serde_json::from_value(json).expect("storefront response");

        assert_eq!(
            storefront.skins_panel_layout.single_item_offers,
            ["offer-a"]
        );
        assert_eq!(
            storefront.skins_panel_layout.single_item_store_offers[0].cost["vp"],
            1775
        );
    }

    #[test]
    fn deserializes_wallet_balances() {
        let json = serde_json::json!({
            "Balances": {
                "85ad13f7-3d1b-5128-9eb2-7cd8ee0b5741": 1250,
                "e59aa87c-4cbf-517a-5983-6e81511be9b7": 40
            }
        });

        let wallet: WalletResponse = serde_json::from_value(json).expect("wallet response");

        assert_eq!(
            wallet.balances["85ad13f7-3d1b-5128-9eb2-7cd8ee0b5741"],
            1250
        );
        assert_eq!(wallet.balances["e59aa87c-4cbf-517a-5983-6e81511be9b7"], 40);
    }

    #[test]
    fn deserializes_game_content_active_act() {
        let json = serde_json::json!({
            "DisabledIDs": [],
            "Seasons": [{
                "ID": "episode",
                "Name": "Episode 11",
                "Type": "episode",
                "StartTime": "2026-01-01T00:00:00Z",
                "EndTime": "2026-12-31T00:00:00Z",
                "IsActive": true
            }, {
                "ID": "act",
                "Name": "Act 3",
                "Type": "act",
                "StartTime": "2026-05-01T00:00:00Z",
                "EndTime": "2026-06-24T13:00:00Z",
                "IsActive": true
            }],
            "Events": []
        });

        let content: GameContentResponse = serde_json::from_value(json).expect("content");

        assert_eq!(
            content.active_act().map(|act| act.name.as_str()),
            Some("Act 3")
        );
        assert_eq!(
            content.active_act().map(|act| act.end_time.as_str()),
            Some("2026-06-24T13:00:00Z")
        );
    }

    #[test]
    fn deserializes_contract_progress() {
        let json = serde_json::json!({
            "Version": 1,
            "Subject": "puuid",
            "Contracts": [{
                "ContractDefinitionID": "battle-pass",
                "ContractProgression": {
                    "TotalProgressionEarned": 123456,
                    "TotalProgressionEarnedVersion": 7,
                    "HighestRewardedLevel": {
                        "0": {"Amount": 10, "Version": 7}
                    }
                },
                "ProgressionLevelReached": 12,
                "ProgressionTowardsNextLevel": 3456,
                "ProgressionCompleted": false
            }],
            "ActiveSpecialContract": "special"
        });

        let contracts: ContractsResponse = serde_json::from_value(json).expect("contracts");

        assert_eq!(contracts.subject, "puuid");
        assert_eq!(contracts.contracts[0].contract_definition_id, "battle-pass");
        assert_eq!(
            contracts.contracts[0]
                .contract_progression
                .total_progression_earned,
            123456
        );
        assert_eq!(contracts.contracts[0].progression_level_reached, 12);
    }

    #[test]
    fn deserializes_accessory_storefront_offers() {
        let json = serde_json::json!({
            "FeaturedBundle": {
                "Bundle": {
                    "ID": "bundle",
                    "DataAssetID": "asset",
                    "CurrencyID": "vp",
                    "Items": [],
                    "DurationRemainingInSeconds": 10
                },
                "Bundles": [],
                "BundleRemainingDurationInSeconds": 20
            },
            "SkinsPanelLayout": {
                "SingleItemOffers": [],
                "SingleItemStoreOffers": [],
                "SingleItemOffersRemainingDurationInSeconds": 86400
            },
            "AccessoryStore": {
                "AccessoryStoreOffers": [{
                    "ContractID": "contract",
                    "Offer": {
                        "OfferID": "accessory-offer",
                        "IsDirectPurchase": true,
                        "StartDate": "2026-05-25T00:00:00Z",
                        "Cost": {"kc": 2500},
                        "Rewards": [{
                            "ItemTypeID": "dd3bf334-87f3-40bd-b043-682a57a8dc3a",
                            "ItemID": "buddy",
                            "Quantity": 1
                        }]
                    }
                }],
                "AccessoryStoreRemainingDurationInSeconds": 604800,
                "StorefrontID": "storefront"
            }
        });

        let storefront: StorefrontResponse =
            serde_json::from_value(json).expect("storefront response");
        let accessory_store = storefront.accessory_store.expect("accessory store");

        assert_eq!(accessory_store.accessory_store_offers.len(), 1);
        assert_eq!(
            accessory_store.accessory_store_offers[0].offer.offer_id,
            "accessory-offer"
        );
        assert_eq!(
            accessory_store.accessory_store_remaining_duration_in_seconds,
            604800
        );
    }

    #[test]
    fn deserializes_fractional_storefront_discounts() {
        let json = serde_json::json!({
            "FeaturedBundle": {
                "Bundle": {
                    "ID": "bundle",
                    "DataAssetID": "asset",
                    "CurrencyID": "vp",
                    "Items": [{
                        "Item": {
                            "ItemTypeID": "skin-type",
                            "ItemID": "skin",
                            "Amount": 1
                        },
                        "BasePrice": 2175,
                        "CurrencyID": "vp",
                        "DiscountPercent": 0.4,
                        "DiscountedPrice": 870,
                        "IsPromoItem": false
                    }],
                    "DurationRemainingInSeconds": 10
                },
                "Bundles": [],
                "BundleRemainingDurationInSeconds": 20
            },
            "SkinsPanelLayout": {
                "SingleItemOffers": [],
                "SingleItemStoreOffers": [],
                "SingleItemOffersRemainingDurationInSeconds": 86400
            },
            "BonusStore": {
                "BonusStoreOffers": [{
                    "BonusOfferID": "bonus",
                    "Offer": {
                        "OfferID": "offer",
                        "IsDirectPurchase": true,
                        "StartDate": "2026-05-25T00:00:00Z",
                        "Cost": {"vp": 1775},
                        "Rewards": []
                    },
                    "DiscountPercent": 0.348,
                    "DiscountCosts": {"vp": 1158},
                    "IsSeen": false
                }],
                "BonusStoreRemainingDurationInSeconds": 40
            }
        });

        let storefront: StorefrontResponse =
            serde_json::from_value(json).expect("storefront response");

        assert_eq!(
            storefront.featured_bundle.bundle.items[0].discount_percent,
            40
        );
        assert_eq!(
            storefront.bonus_store.unwrap().bonus_store_offers[0].discount_percent,
            35
        );
    }

    #[test]
    fn deserializes_player_loadout_acronym_fields() {
        let json = serde_json::json!({
            "Subject": "puuid",
            "Version": 1,
            "Guns": [{
                "ID": "weapon",
                "SkinID": "skin",
                "SkinLevelID": "level",
                "ChromaID": "chroma",
                "Attachments": []
            }],
            "Sprays": [],
            "Identity": {
                "PlayerCardID": "card",
                "PlayerTitleID": "title",
                "AccountLevel": 42,
                "PreferredLevelBorderID": "border",
                "HideAccountLevel": false
            },
            "Incognito": false
        });

        let loadout: PlayerLoadoutResponse = serde_json::from_value(json).expect("loadout");

        assert_eq!(loadout.subject, "puuid");
        assert_eq!(loadout.guns[0].skin_id, "skin");
        assert_eq!(loadout.identity.account_level, 42);
    }

    #[test]
    fn deserializes_account_xp_progress() {
        let json = serde_json::json!({
            "Version": 1,
            "Subject": "puuid",
            "Progress": {
                "Level": 123,
                "XP": 456
            },
            "History": []
        });

        let xp: AccountXpResponse = serde_json::from_value(json).expect("account xp");

        assert_eq!(xp.subject, "puuid");
        assert_eq!(xp.progress.level, 123);
        assert_eq!(xp.progress.xp, 456);
    }

    #[test]
    fn deserializes_player_mmr_ranked_rating() {
        let json = serde_json::json!({
            "Version": 1,
            "Subject": "puuid",
            "QueueSkills": {
                "competitive": {
                    "SeasonalInfoBySeasonID": {
                        "season-a": {
                            "SeasonID": "season-a",
                            "CompetitiveTier": 15,
                            "RankedRating": 42,
                            "NumberOfGames": 12,
                            "GamesNeededForRating": 0
                        }
                    }
                }
            },
            "LatestCompetitiveUpdate": {
                "SeasonID": "season-a",
                "TierAfterUpdate": 15,
                "RankedRatingAfterUpdate": 42
            }
        });

        let mmr: PlayerMmrResponse = serde_json::from_value(json).expect("mmr response");
        let competitive = mmr.queue_skills.competitive.expect("competitive");
        let season = &competitive.seasonal_info_by_season_id["season-a"];

        assert_eq!(season.competitive_tier, 15);
        assert_eq!(season.ranked_rating, 42);
        assert_eq!(
            mmr.latest_competitive_update
                .as_ref()
                .map(|update| update.season_id.as_str()),
            Some("season-a")
        );
    }
}
