use std::collections::HashMap;

use serde::Deserialize;

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
pub struct StorefrontResponse {
    pub featured_bundle: FeaturedBundle,
    pub skins_panel_layout: SkinsPanelLayout,
    #[serde(default)]
    pub bonus_store: Option<BonusStore>,
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
    pub duration_remaining_in_seconds: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct BundleItem {
    pub item: StoreItem,
    pub base_price: i64,
    #[serde(rename = "CurrencyID")]
    pub currency_id: String,
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
    pub discount_percent: i64,
    #[serde(default)]
    pub discount_costs: HashMap<String, i64>,
    pub is_seen: bool,
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
}
