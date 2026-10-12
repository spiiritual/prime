use std::collections::HashMap;

use serde::{Deserialize, Deserializer};

#[derive(Clone, Deserialize, PartialEq)]
pub struct EntitlementResponse {
    pub entitlements_token: String,
}

impl std::fmt::Debug for EntitlementResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntitlementResponse")
            .field("entitlements_token", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct PlayerInfoResponse {
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
    #[serde(default)]
    pub seasons: Vec<GameContentSeason>,
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
    // Riot sends null here for an account that has never played competitive.
    #[serde(
        default,
        rename = "SeasonalInfoBySeasonID",
        deserialize_with = "null_as_default"
    )]
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
pub struct PlayerPenaltiesResponse {
    pub subject: String,
    pub penalties: Vec<PlayerPenalty>,
    pub infractions: Vec<PlayerInfraction>,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct PlayerPenalty {
    #[serde(rename = "ID")]
    pub id: String,
    pub expiry: Option<String>,
    pub games_remaining: i64,
    #[serde(rename = "InfractionID")]
    pub infraction_id: String,
    pub forgiveness_ineligible: bool,
    pub is_automated_detection: bool,
    pub penalty_info: Option<serde_json::Value>,
    pub premier_restriction_effect: Option<PremierRestrictionEffect>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct PremierRestrictionEffect {
    pub restriction_type: String,
    #[serde(rename = "SeasonID")]
    pub season_id: String,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct PlayerInfraction {
    #[serde(rename = "ID")]
    pub id: String,
    pub name: String,
    pub rating_name: String,
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
    pub offer: StoreOffer,
    #[serde(deserialize_with = "deserialize_discount_percent")]
    pub discount_percent: i64,
    #[serde(default)]
    pub discount_costs: HashMap<String, i64>,
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

fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
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
    pub identity: LoadoutIdentity,
    pub incognito: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct LoadoutGun {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "SkinID")]
    pub skin_id: String,
    #[serde(rename = "SkinLevelID")]
    pub skin_level_id: String,
    #[serde(rename = "ChromaID")]
    pub chroma_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct LoadoutIdentity {
    #[serde(rename = "PlayerCardID")]
    pub player_card_id: String,
    #[serde(rename = "AccountLevel")]
    pub account_level: i64,
    #[serde(rename = "PreferredLevelBorderID")]
    pub preferred_level_border_id: String,
    #[serde(rename = "HideAccountLevel")]
    pub hide_account_level: bool,
}

/// Current game or pre-game player: which match the player is in.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct GamePlayerResponse {
    #[serde(rename = "MatchID")]
    pub match_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CoreGameMatchResponse {
    #[serde(rename = "MatchID")]
    pub match_id: String,
    #[serde(rename = "MapID", default)]
    pub map_id: String,
    #[serde(rename = "ProvisioningFlow", default)]
    pub provisioning_flow: String,
    #[serde(rename = "GamePodID", default)]
    pub game_pod_id: String,
    #[serde(rename = "Players", default)]
    pub players: Vec<CoreGamePlayer>,
    /// Null in some matches, such as custom games.
    #[serde(rename = "MatchmakingData", default)]
    pub matchmaking_data: Option<MatchmakingData>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct MatchmakingData {
    #[serde(rename = "QueueID", default)]
    pub queue_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CoreGamePlayer {
    #[serde(rename = "Subject")]
    pub subject: String,
    #[serde(rename = "TeamID", default)]
    pub team_id: String,
    #[serde(rename = "CharacterID", default)]
    pub character_id: String,
    #[serde(rename = "PlayerIdentity", default)]
    pub player_identity: MatchPlayerIdentity,
    #[serde(rename = "IsCoach", default)]
    pub is_coach: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct MatchPlayerIdentity {
    #[serde(rename = "PlayerCardID", default)]
    pub player_card_id: String,
    #[serde(rename = "AccountLevel", default)]
    pub account_level: i64,
    /// The player hides their name in game (streamer mode).
    #[serde(rename = "Incognito", default)]
    pub incognito: bool,
    #[serde(rename = "HideAccountLevel", default)]
    pub hide_account_level: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct PregameMatchResponse {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "MapID", default)]
    pub map_id: String,
    #[serde(rename = "QueueID", default)]
    pub queue_id: String,
    #[serde(rename = "GamePodID", default)]
    pub game_pod_id: String,
    #[serde(rename = "ProvisioningFlowID", default)]
    pub provisioning_flow_id: String,
    #[serde(rename = "AllyTeam", default)]
    pub ally_team: Option<PregameTeam>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct PregameTeam {
    #[serde(rename = "Players", default)]
    pub players: Vec<PregamePlayer>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct PregamePlayer {
    #[serde(rename = "Subject")]
    pub subject: String,
    #[serde(rename = "CharacterID", default)]
    pub character_id: String,
    /// "", "selected" or "locked".
    #[serde(rename = "CharacterSelectionState", default)]
    pub character_selection_state: String,
    #[serde(rename = "PlayerIdentity", default)]
    pub player_identity: MatchPlayerIdentity,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CoreGameLoadoutsResponse {
    #[serde(rename = "Loadouts", default)]
    pub loadouts: Vec<CoreGameLoadout>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CoreGameLoadout {
    #[serde(rename = "Loadout")]
    pub loadout: MatchLoadout,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct PregameLoadoutsResponse {
    #[serde(rename = "Loadouts", default)]
    pub loadouts: Vec<MatchLoadout>,
    #[serde(rename = "LoadoutsValid", default)]
    pub loadouts_valid: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct MatchLoadout {
    #[serde(rename = "Subject")]
    pub subject: String,
    #[serde(rename = "Items", default)]
    pub items: HashMap<String, MatchLoadoutItem>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct MatchLoadoutItem {
    #[serde(rename = "Sockets", default)]
    pub sockets: HashMap<String, MatchLoadoutSocket>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct MatchLoadoutSocket {
    #[serde(rename = "Item")]
    pub item: MatchLoadoutSocketItem,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct MatchLoadoutSocketItem {
    #[serde(rename = "ID")]
    pub id: String,
}

impl MatchLoadout {
    /// The item in a weapon's socket, such as its skin. Riot's IDs are compared without case.
    pub fn socket_item(&self, weapon_id: &str, socket_id: &str) -> Option<&str> {
        let (_, item) = self
            .items
            .iter()
            .find(|(id, _)| id.eq_ignore_ascii_case(weapon_id))?;
        item.sockets
            .iter()
            .find(|(id, _)| id.eq_ignore_ascii_case(socket_id))
            .map(|(_, socket)| socket.item.id.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct NameServiceEntry {
    #[serde(rename = "Subject")]
    pub subject: String,
    #[serde(rename = "GameName", default)]
    pub game_name: String,
    #[serde(rename = "TagLine", default)]
    pub tag_line: String,
}

#[cfg(test)]
mod tests;
