use super::*;

use iced::futures::stream::{self, StreamExt};

use crate::riot::client::RiotApiError;
use crate::riot::content::{MatchCatalog, WeaponContent};
use crate::riot::local_client::{MatchScore, ScoreUnavailable, match_score, player_account_names};
use crate::riot::models::{CoreGameMatchResponse, MatchLoadout, PregameMatchResponse};

use super::account_details::{
    AccountActivity, RefreshedApiContext, competitive_rank_from_mmr,
    rank_name_for_competitive_tier, refreshed_api_context,
};
use super::image_assets::{
    cache_map_art, cache_skin_icon, fetch_match_catalog, fetch_weapon_content,
};
use super::loadout::{SkinDisplay, resolve_current_skin};
use super::session::{has_saved_login, resolve_credentials};

/// The weapons whose skins the page shows, in its column order: Vandal, Phantom, Sheriff,
/// Operator.
pub(in crate::ui) const SHOWN_WEAPONS: [&str; 4] = [
    "9c82e19d-4575-0200-1a81-3eacf00cf872",
    "ee8e8d15-496b-07ac-e5f6-8fae5d4c7b1a",
    "e336c6b8-418d-9340-d77f-7a9e4cfe0702",
    "a03b24d3-4319-996d-0f8c-94bbfba1dfc7",
];
const SKIN_SOCKET: &str = "bcef87d6-209b-46c6-8b19-fbe40bd95abc";
const SKIN_LEVEL_SOCKET: &str = "e7c63390-eda7-46e0-bb7a-a6abdacd2433";
const CHROMA_SOCKET: &str = "3ad1b2b2-acdb-4524-852f-954a76ddae0a";
/// How many players' ranks load at once.
const RANKS_AT_ONCE: usize = 3;
const RANK_NOT_LOADED: &str = "not loaded yet";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum LiveMatchError {
    /// The account has no login to sign in with; polling stops until the user acts.
    SignIn(String),
    /// A Riot request failed; the next poll tries again.
    Request(String),
}

impl LiveMatchError {
    pub(in crate::ui) fn message(&self) -> &str {
        match self {
            Self::SignIn(error) | Self::Request(error) => error,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct LiveMatchResult {
    pub(in crate::ui) account_id: AccountId,
    pub(in crate::ui) activity: AccountActivity,
    /// `Some` in agent select or a match.
    pub(in crate::ui) live: Option<LiveMatch>,
    pub(in crate::ui) refreshed: Option<RefreshedApiContext>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum MatchPhase {
    AgentSelect,
    InProgress,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct LiveMatch {
    pub(in crate::ui) account_id: AccountId,
    pub(in crate::ui) match_id: String,
    pub(in crate::ui) phase: MatchPhase,
    pub(in crate::ui) map: Option<String>,
    pub(in crate::ui) map_art: Option<PathBuf>,
    pub(in crate::ui) mode: Option<String>,
    pub(in crate::ui) server: Option<String>,
    /// Kept from agent select, since some matches don't report their queue.
    pub(in crate::ui) queue_id: Option<String>,
    pub(in crate::ui) score: Result<MatchScore, ScoreUnavailable>,
    pub(in crate::ui) allies: Vec<LivePlayer>,
    pub(in crate::ui) enemies: Vec<LivePlayer>,
    /// Whether every player's skins have loaded, so they aren't asked for again.
    pub(in crate::ui) loadouts_loaded: bool,
    /// Why the last lookup of hidden players' names failed.
    pub(in crate::ui) hidden_names_error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct LivePlayer {
    pub(in crate::ui) puuid: String,
    pub(in crate::ui) is_self: bool,
    pub(in crate::ui) agent: Option<String>,
    pub(in crate::ui) agent_pick: AgentPick,
    /// `None` when Riot reports no level.
    pub(in crate::ui) level: Option<i64>,
    pub(in crate::ui) hides_level: bool,
    /// The player hides their name in game.
    pub(in crate::ui) incognito: bool,
    /// Game name and tag line, kept in memory only.
    pub(in crate::ui) name: Option<(String, String)>,
    pub(in crate::ui) card_id: Option<String>,
    pub(in crate::ui) rank: RankState,
    pub(in crate::ui) skins: [SkinCell; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum AgentPick {
    Locked,
    Picking,
    NotPicked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum RankState {
    Ranked(CompetitiveRank),
    Unranked,
    Unavailable(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum SkinCell {
    Standard,
    Skin(SkinDisplay),
    Unavailable,
}

pub(in crate::ui) async fn fetch_live_match(
    account: AccountProfile,
    client_version: String,
    image_cache: ImageCache,
    previous: Option<LiveMatch>,
    // Whether to name players who hide their name, from the Riot Client on this PC.
    show_hidden: bool,
) -> Result<LiveMatchResult, LiveMatchError> {
    let request = |error: RiotApiError| LiveMatchError::Request(error.to_string());
    let api = RiotApi::shared().map_err(request)?;
    if !has_saved_login(&account) {
        return Err(LiveMatchError::SignIn(
            "it has no captured login or saved Riot token".to_string(),
        ));
    }
    // A failed sign-in may be a network drop or a Riot outage, so the next poll tries again.
    let resolved = resolve_credentials(&api, &account, client_version)
        .await
        .map_err(LiveMatchError::Request)?;
    let region = resolved.region.ok_or_else(|| {
        LiveMatchError::Request("Riot didn't report this account's region".to_string())
    })?;
    let credentials = &resolved.credentials;
    let previous = previous.filter(|previous| previous.account_id == account.id);

    let (activity, live) = if let Some(player) = api
        .current_game_player(credentials, region)
        .await
        .map_err(request)?
    {
        let reusable = previous.filter(|previous| previous.match_id == player.match_id);
        let mut live = match reusable {
            // A match whose map didn't resolve is read again, in case the catalog failed.
            Some(previous)
                if previous.phase == MatchPhase::InProgress && previous.map.is_some() =>
            {
                previous
            }
            carried => {
                let response = api
                    .core_game_match(credentials, region, &player.match_id)
                    .await
                    .map_err(request)?;
                let catalog = fetch_match_catalog().await.ok();
                let mut live = live_match_from_core_game(
                    account.id,
                    &credentials.puuid,
                    &response,
                    catalog.as_deref(),
                    carried.as_ref(),
                );
                live.map_art = map_art(catalog.as_deref(), &response.map_id, &image_cache).await;
                live
            }
        };
        fill_missing(
            &api,
            credentials,
            region,
            &mut live,
            &image_cache,
            show_hidden,
        )
        .await;
        (AccountActivity::InMatch, Some(live))
    } else if let Some(player) = api
        .pregame_player(credentials, region)
        .await
        .map_err(request)?
    {
        // Picks change while agent select runs, so the match is read on every poll.
        let response = api
            .pregame_match(credentials, region, &player.match_id)
            .await
            .map_err(request)?;
        let catalog = fetch_match_catalog().await.ok();
        let previous = previous.filter(|previous| previous.match_id == player.match_id);
        let mut live = live_match_from_pregame(
            account.id,
            &credentials.puuid,
            &response,
            catalog.as_deref(),
            previous.as_ref(),
        );
        live.map_art = match previous.and_then(|previous| previous.map_art) {
            Some(path) => Some(path),
            None => map_art(catalog.as_deref(), &response.map_id, &image_cache).await,
        };
        fill_missing(
            &api,
            credentials,
            region,
            &mut live,
            &image_cache,
            show_hidden,
        )
        .await;
        (AccountActivity::AgentSelect, Some(live))
    } else if api
        .party_player(credentials, region)
        .await
        .map_err(request)?
        .is_some()
    {
        (AccountActivity::InLobby, None)
    } else {
        (AccountActivity::Available, None)
    };

    Ok(LiveMatchResult {
        account_id: account.id,
        activity,
        live,
        refreshed: refreshed_api_context(&account, resolved),
    })
}

async fn map_art(
    catalog: Option<&MatchCatalog>,
    map_id: &str,
    image_cache: &ImageCache,
) -> Option<PathBuf> {
    cache_map_art(catalog?.map(map_id)?, image_cache).await
}

/// Loads what the match doesn't have yet: skins, names, ranks and, in a match, the score.
/// Anything that fails stays missing and is asked for again on the next poll.
async fn fill_missing(
    api: &RiotApi,
    credentials: &ApiCredentials,
    region: ValorantRegion,
    live: &mut LiveMatch,
    image_cache: &ImageCache,
    show_hidden: bool,
) {
    let players = || live.allies.iter().chain(&live.enemies);
    // The name service gives no name for players who hide theirs while the match runs, so
    // they're left out rather than asked for on every poll.
    let unnamed: Vec<String> = players()
        .filter(|player| player.name.is_none() && !player.incognito)
        .map(|player| player.puuid.clone())
        .collect();
    let hidden: Vec<String> = players()
        .filter(|player| show_hidden && player.incognito && player.name.is_none())
        .map(|player| player.puuid.clone())
        .collect();
    let unranked: Vec<String> = players()
        .filter(|player| matches!(player.rank, RankState::Unavailable(_)))
        .map(|player| player.puuid.clone())
        .collect();
    let (match_id, phase, loadouts_loaded) =
        (live.match_id.clone(), live.phase, live.loadouts_loaded);

    let (loadouts, names, hidden_names, ranks, score) = iced::futures::join!(
        async {
            if loadouts_loaded {
                return None;
            }
            let weapon_content = fetch_weapon_content().await.ok()?;
            let loadouts = match phase {
                MatchPhase::InProgress => api
                    .core_game_loadouts(credentials, region, &match_id)
                    .await
                    .ok()
                    .map(|response| {
                        let loadouts = response.loadouts.into_iter().map(|loadout| loadout.loadout);
                        (loadouts.collect::<Vec<_>>(), true)
                    }),
                MatchPhase::AgentSelect => api
                    .pregame_loadouts(credentials, region, &match_id)
                    .await
                    .ok()
                    .map(|response| (response.loadouts, response.loadouts_valid)),
            }?;
            Some((loadouts, weapon_content))
        },
        async {
            if unnamed.is_empty() {
                return Vec::new();
            }
            api.player_names(credentials, &unnamed)
                .await
                .unwrap_or_default()
        },
        async {
            if hidden.is_empty() {
                return None;
            }
            Some(player_account_names(&hidden).await)
        },
        stream::iter(unranked)
            .map(|puuid| async move {
                let rank = match api.player_mmr(credentials, &puuid).await {
                    Ok(response) => match competitive_rank_from_mmr(&response) {
                        Some(rank) => RankState::Ranked(rank),
                        None => RankState::Unranked,
                    },
                    Err(error) => RankState::Unavailable(error.to_string()),
                };
                (puuid, rank)
            })
            .buffer_unordered(RANKS_AT_ONCE)
            .collect::<Vec<_>>(),
        async {
            match phase {
                MatchPhase::InProgress => Some(match_score(&credentials.puuid).await),
                MatchPhase::AgentSelect => None,
            }
        },
    );

    if let Some(((loadouts, valid), weapon_content)) = loadouts {
        for loadout in &loadouts {
            if let Some(player) = player_mut(live, &loadout.subject) {
                player.skins = skin_cells(loadout, &weapon_content);
                for cell in &mut player.skins {
                    if let SkinCell::Skin(skin) = cell {
                        cache_skin_icon(skin, image_cache).await;
                    }
                }
            }
        }
        live.loadouts_loaded = valid;
    }
    let names = names
        .into_iter()
        .map(|entry| (entry.subject, entry.game_name, entry.tag_line));
    let hidden_names = match hidden_names {
        Some(Ok(hidden_names)) => {
            live.hidden_names_error = None;
            hidden_names
        }
        Some(Err(error)) => {
            live.hidden_names_error = Some(error);
            Vec::new()
        }
        None => Vec::new(),
    };
    apply_names(live, names.chain(hidden_names));
    for (puuid, rank) in ranks {
        if let Some(player) = player_mut(live, &puuid) {
            player.rank = rank;
        }
    }
    if let Some(score) = score {
        live.score = score;
    }
}

/// Sets players' names from `(puuid, game name, tag line)`. Blank names, which the name service
/// gives players who hide theirs, never replace a name.
pub(in crate::ui) fn apply_names(
    live: &mut LiveMatch,
    names: impl IntoIterator<Item = (String, String, String)>,
) {
    for (puuid, game_name, tag_line) in names {
        if let Some(player) = player_mut(live, &puuid)
            && !game_name.trim().is_empty()
        {
            player.name = Some((game_name, tag_line));
        }
    }
}

fn player_mut<'a>(live: &'a mut LiveMatch, puuid: &str) -> Option<&'a mut LivePlayer> {
    live.allies
        .iter_mut()
        .chain(live.enemies.iter_mut())
        .find(|player| player.puuid.eq_ignore_ascii_case(puuid))
}

pub(in crate::ui) fn live_match_from_core_game(
    account_id: AccountId,
    self_puuid: &str,
    response: &CoreGameMatchResponse,
    catalog: Option<&MatchCatalog>,
    previous: Option<&LiveMatch>,
) -> LiveMatch {
    let team = response
        .players
        .iter()
        .find(|player| player.subject.eq_ignore_ascii_case(self_puuid))
        .map(|player| player.team_id.as_str());
    let mut allies = Vec::new();
    let mut enemies = Vec::new();
    for player in response.players.iter().filter(|player| !player.is_coach) {
        let live_player = live_player(
            &player.subject,
            self_puuid,
            &player.character_id,
            AgentPick::Locked,
            &player.player_identity,
            catalog,
        );
        if Some(player.team_id.as_str()) == team {
            allies.push(live_player);
        } else {
            enemies.push(live_player);
        }
    }
    let queue_id = response
        .matchmaking_data
        .as_ref()
        .and_then(|data| data.queue_id.clone())
        .filter(|queue| !queue.trim().is_empty())
        .or_else(|| previous.and_then(|previous| previous.queue_id.clone()))
        .or_else(|| custom_game_queue(&response.provisioning_flow));

    let mut live = LiveMatch {
        account_id,
        match_id: response.match_id.clone(),
        phase: MatchPhase::InProgress,
        map: map_name(catalog, &response.map_id),
        map_art: None,
        mode: mode_name(catalog, queue_id.as_deref()),
        server: server_label(&response.game_pod_id),
        queue_id,
        score: Err(ScoreUnavailable::NotFound),
        allies,
        enemies,
        loadouts_loaded: false,
        hidden_names_error: None,
    };
    carry_over(&mut live, previous);
    live
}

pub(in crate::ui) fn live_match_from_pregame(
    account_id: AccountId,
    self_puuid: &str,
    response: &PregameMatchResponse,
    catalog: Option<&MatchCatalog>,
    previous: Option<&LiveMatch>,
) -> LiveMatch {
    let allies = response
        .ally_team
        .iter()
        .flat_map(|team| &team.players)
        .map(|player| {
            let pick = match player
                .character_selection_state
                .to_ascii_lowercase()
                .as_str()
            {
                "locked" => AgentPick::Locked,
                "selected" => AgentPick::Picking,
                _ => AgentPick::NotPicked,
            };
            live_player(
                &player.subject,
                self_puuid,
                &player.character_id,
                pick,
                &player.player_identity,
                catalog,
            )
        })
        .collect();
    let queue_id = Some(response.queue_id.clone())
        .filter(|queue| !queue.trim().is_empty())
        .or_else(|| custom_game_queue(&response.provisioning_flow_id));

    let mut live = LiveMatch {
        account_id,
        match_id: response.id.clone(),
        phase: MatchPhase::AgentSelect,
        map: map_name(catalog, &response.map_id),
        map_art: None,
        mode: mode_name(catalog, queue_id.as_deref()),
        server: server_label(&response.game_pod_id),
        queue_id,
        score: Err(ScoreUnavailable::NotFound),
        allies,
        // Riot doesn't reveal the enemy team until the match starts.
        enemies: Vec::new(),
        loadouts_loaded: false,
        hidden_names_error: None,
    };
    carry_over(&mut live, previous);
    live
}

fn live_player(
    puuid: &str,
    self_puuid: &str,
    character_id: &str,
    agent_pick: AgentPick,
    identity: &crate::riot::models::MatchPlayerIdentity,
    catalog: Option<&MatchCatalog>,
) -> LivePlayer {
    LivePlayer {
        puuid: puuid.to_string(),
        is_self: puuid.eq_ignore_ascii_case(self_puuid),
        agent: catalog
            .and_then(|catalog| catalog.agent_name(character_id))
            .map(str::to_string),
        agent_pick,
        level: Some(identity.account_level).filter(|level| *level > 0),
        hides_level: identity.hide_account_level,
        incognito: identity.incognito,
        name: None,
        card_id: Some(identity.player_card_id.clone()).filter(|id| !id.trim().is_empty()),
        rank: RankState::Unavailable(RANK_NOT_LOADED.to_string()),
        skins: std::array::from_fn(|_| SkinCell::Unavailable),
    }
}

/// Keeps what an earlier poll of the same match already loaded: names, ranks and skins, and
/// whether the skins are complete while the phase is the same.
fn carry_over(live: &mut LiveMatch, previous: Option<&LiveMatch>) {
    let Some(previous) = previous.filter(|previous| previous.match_id == live.match_id) else {
        return;
    };
    let old_players: Vec<&LivePlayer> = previous.allies.iter().chain(&previous.enemies).collect();
    for player in live.allies.iter_mut().chain(live.enemies.iter_mut()) {
        let Some(old) = old_players
            .iter()
            .find(|old| old.puuid.eq_ignore_ascii_case(&player.puuid))
        else {
            continue;
        };
        player.name.clone_from(&old.name);
        player.rank = old.rank.clone();
        player.skins = old.skins.clone();
    }
    live.loadouts_loaded = previous.loadouts_loaded && previous.phase == live.phase;
}

fn custom_game_queue(provisioning_flow: &str) -> Option<String> {
    provisioning_flow
        .eq_ignore_ascii_case("CustomGame")
        .then(|| "custom".to_string())
}

fn map_name(catalog: Option<&MatchCatalog>, map_id: &str) -> Option<String> {
    Some(catalog?.map(map_id)?.display_name.clone())
}

fn mode_name(catalog: Option<&MatchCatalog>, queue_id: Option<&str>) -> Option<String> {
    Some(catalog?.queue_name(queue_id?)?.to_string())
}

/// The skins of the shown weapons. A weapon's own default skin is "Standard", told by its ID.
pub(in crate::ui) fn skin_cells(loadout: &MatchLoadout, content: &WeaponContent) -> [SkinCell; 4] {
    SHOWN_WEAPONS.map(|weapon_id| {
        let Some(skin_id) = loadout.socket_item(weapon_id, SKIN_SOCKET) else {
            return SkinCell::Unavailable;
        };
        let is_default = content
            .weapons
            .resolve(weapon_id)
            .default_skin_uuid
            .is_some_and(|default| default.eq_ignore_ascii_case(skin_id));
        if is_default {
            return SkinCell::Standard;
        }
        SkinCell::Skin(SkinDisplay::from(resolve_current_skin(
            &content.skins,
            skin_id,
            loadout
                .socket_item(weapon_id, SKIN_LEVEL_SOCKET)
                .unwrap_or_default(),
            loadout
                .socket_item(weapon_id, CHROMA_SOCKET)
                .unwrap_or_default(),
        )))
    })
}

/// The city in Riot's game server ID, such as "Ashburn" for
/// `aresriot.aws-rclusterprod-use1-1.na-gp-ashburn-1`.
pub(in crate::ui) fn server_label(game_pod_id: &str) -> Option<String> {
    let (_, city) = game_pod_id.rsplit_once("-gp-")?;
    let city = city
        .rsplit_once('-')
        .filter(|(_, number)| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
        .map_or(city, |(city, _)| city);
    let words: Vec<String> = city
        .split('-')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        })
        .collect();
    (!words.is_empty()
        && words
            .iter()
            .all(|word| word.chars().all(char::is_alphabetic)))
    .then(|| words.join(" "))
}

/// The average of a team's known ranks, such as "Diamond 2". Unranked players are left out.
pub(in crate::ui) fn team_average_rank(players: &[LivePlayer]) -> Option<String> {
    let tiers: Vec<i64> = players
        .iter()
        .filter_map(|player| match &player.rank {
            RankState::Ranked(rank) if rank.tier >= 3 => Some(rank.tier),
            _ => None,
        })
        .collect();
    if tiers.is_empty() {
        return None;
    }
    let average = (tiers.iter().sum::<i64>() as f64 / tiers.len() as f64).round() as i64;
    Some(rank_name_for_competitive_tier(average))
}

/// "Ascent · Competitive", or the map alone when the mode isn't known.
pub(in crate::ui) fn match_title(live: &LiveMatch) -> String {
    let map = live.map.as_deref().unwrap_or("Unknown map");
    match &live.mode {
        Some(mode) => format!("{map} · {mode}"),
        None => map.to_string(),
    }
}

/// "Round 8 · Ashburn" in a match, "Agent select · Ashburn" before it. Parts that aren't known
/// are left out.
pub(in crate::ui) fn match_meta(live: &LiveMatch) -> String {
    let stage = match live.phase {
        MatchPhase::AgentSelect => Some("Agent select".to_string()),
        MatchPhase::InProgress => live
            .score
            .as_ref()
            .ok()
            .map(|score| format!("Round {}", score.round())),
    };
    [stage, live.server.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The sidebar indicator's detail: "Ascent · 5 – 2", or the map alone without a score.
pub(in crate::ui) fn indicator_detail(live: &LiveMatch) -> String {
    let map = live.map.as_deref().unwrap_or("View match");
    match (&live.score, live.phase) {
        (Ok(score), MatchPhase::InProgress) => format!("{map} · {} – {}", score.ally, score.enemy),
        _ => map.to_string(),
    }
}

/// How a player's name shows under the "Show hidden details" setting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum ShownIdentity<'a> {
    /// The player hides their name and hidden details aren't shown.
    Hidden,
    Shown {
        /// `None` when the name didn't load.
        name: Option<(&'a str, &'a str)>,
        /// The player hides their name in game, but the user chose to see it.
        streamer: bool,
    },
}

/// `own` is whether the player is one of the user's saved accounts, which are never hidden.
/// `saved_name` is that account's saved Riot ID, shown when Riot gives no name, as the name
/// service doesn't for a player who hides theirs.
pub(in crate::ui) fn shown_identity<'a>(
    player: &'a LivePlayer,
    show_hidden: bool,
    own: bool,
    saved_name: Option<(&'a str, &'a str)>,
) -> ShownIdentity<'a> {
    let hides = player.incognito && !own;
    if hides && !show_hidden {
        return ShownIdentity::Hidden;
    }
    ShownIdentity::Shown {
        name: player
            .name
            .as_ref()
            .map(|(name, tag)| (name.as_str(), tag.as_str()))
            .or(saved_name.filter(|_| own)),
        streamer: hides,
    }
}

/// "Jett · Lv 214", with the level left out when unknown, or hidden by the player while hidden
/// details aren't shown. In agent select: "Jett (picking)" or "No agent yet".
pub(in crate::ui) fn agent_line(player: &LivePlayer, show_hidden: bool, own: bool) -> String {
    let agent = match (&player.agent, player.agent_pick) {
        (_, AgentPick::NotPicked) => "No agent yet".to_string(),
        (Some(agent), AgentPick::Picking) => format!("{agent} (picking)"),
        (Some(agent), AgentPick::Locked) => agent.clone(),
        (None, _) => "Unknown agent".to_string(),
    };
    let level_hidden = player.hides_level && !show_hidden && !own;
    match player.level.filter(|_| !level_hidden) {
        Some(level) => format!("{agent} · Lv {level}"),
        None => agent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::riot::content::{Agent, Map, Queue, Weapon, WeaponSkin};
    use crate::riot::models::{CoreGamePlayer, MatchPlayerIdentity, PregamePlayer, PregameTeam};

    const JETT: &str = "add6443a-41bd-e414-f6ad-e58d267f4e95";

    fn catalog() -> MatchCatalog {
        MatchCatalog::from_parts(
            vec![Map {
                uuid: "ascent".to_string(),
                display_name: "Ascent".to_string(),
                map_url: "/Game/Maps/Ascent/Ascent".to_string(),
                list_view_icon: None,
            }],
            vec![
                Queue {
                    queue_id: Some("competitive".to_string()),
                    display_name: "Competitive".to_string(),
                },
                Queue {
                    queue_id: Some("custom".to_string()),
                    display_name: "Custom Game".to_string(),
                },
            ],
            vec![Agent {
                uuid: JETT.to_string(),
                display_name: "Jett".to_string(),
            }],
        )
    }

    fn identity(level: i64, incognito: bool, hide_level: bool) -> MatchPlayerIdentity {
        MatchPlayerIdentity {
            player_card_id: "card".to_string(),
            account_level: level,
            incognito,
            hide_account_level: hide_level,
        }
    }

    fn core_player(subject: &str, team: &str, coach: bool) -> CoreGamePlayer {
        CoreGamePlayer {
            subject: subject.to_string(),
            team_id: team.to_string(),
            character_id: JETT.to_string(),
            player_identity: identity(214, subject == "enemy", false),
            is_coach: coach,
        }
    }

    fn core_game(queue: Option<&str>, flow: &str) -> CoreGameMatchResponse {
        CoreGameMatchResponse {
            match_id: "match".to_string(),
            map_id: "/Game/Maps/Ascent/Ascent".to_string(),
            provisioning_flow: flow.to_string(),
            game_pod_id: "aresriot.aws-rclusterprod-use1-1.na-gp-ashburn-1".to_string(),
            players: vec![
                core_player("ally", "Blue", false),
                core_player("SELF", "Blue", false),
                core_player("enemy", "Red", false),
                core_player("coach", "Red", true),
            ],
            matchmaking_data: queue.map(|queue| crate::riot::models::MatchmakingData {
                queue_id: Some(queue.to_string()),
            }),
        }
    }

    fn pregame() -> PregameMatchResponse {
        let player = |subject: &str, state: &str| PregamePlayer {
            subject: subject.to_string(),
            character_id: JETT.to_string(),
            character_selection_state: state.to_string(),
            player_identity: identity(10, false, false),
        };
        PregameMatchResponse {
            id: "match".to_string(),
            map_id: "/Game/Maps/Ascent/Ascent".to_string(),
            queue_id: "competitive".to_string(),
            game_pod_id: "pod".to_string(),
            provisioning_flow_id: "Matchmaking".to_string(),
            ally_team: Some(PregameTeam {
                players: vec![
                    player("self", "locked"),
                    player("ally", "selected"),
                    player("other", ""),
                ],
            }),
        }
    }

    fn in_match(previous: Option<&LiveMatch>) -> LiveMatch {
        live_match_from_core_game(
            AccountId::new(),
            "self",
            &core_game(Some("competitive"), "Matchmaking"),
            Some(&catalog()),
            previous,
        )
    }

    fn ranked(tier: i64) -> RankState {
        RankState::Ranked(CompetitiveRank::new(
            tier,
            rank_name_for_competitive_tier(tier),
            0,
        ))
    }

    #[test]
    fn teams_split_by_the_accounts_team_without_coaches() {
        let live = in_match(None);

        let allies: Vec<_> = live.allies.iter().map(|p| p.puuid.as_str()).collect();
        let enemies: Vec<_> = live.enemies.iter().map(|p| p.puuid.as_str()).collect();
        assert_eq!(allies, ["ally", "SELF"]);
        assert_eq!(enemies, ["enemy"]);
        assert!(live.allies[1].is_self);
        assert!(!live.allies[0].is_self);
        assert!(live.enemies[0].incognito);
        assert_eq!(live.allies[0].agent.as_deref(), Some("Jett"));
        assert_eq!(match_title(&live), "Ascent · Competitive");
        assert_eq!(live.server.as_deref(), Some("Ashburn"));
    }

    #[test]
    fn the_mode_falls_back_to_agent_selects_queue_then_custom_games() {
        let catalog = catalog();
        let from_pregame =
            live_match_from_pregame(AccountId::new(), "self", &pregame(), Some(&catalog), None);
        let remembered = live_match_from_core_game(
            AccountId::new(),
            "self",
            &core_game(None, "Matchmaking"),
            Some(&catalog),
            Some(&from_pregame),
        );
        let custom = live_match_from_core_game(
            AccountId::new(),
            "self",
            &core_game(None, "CustomGame"),
            Some(&catalog),
            None,
        );
        let unknown = live_match_from_core_game(
            AccountId::new(),
            "self",
            &core_game(None, "Matchmaking"),
            Some(&catalog),
            None,
        );

        assert_eq!(remembered.mode.as_deref(), Some("Competitive"));
        assert_eq!(custom.mode.as_deref(), Some("Custom Game"));
        assert_eq!(unknown.mode, None);
        assert_eq!(match_title(&unknown), "Ascent");
    }

    #[test]
    fn names_ranks_and_skins_carry_over_within_the_same_match() {
        let mut agent_select =
            live_match_from_pregame(AccountId::new(), "self", &pregame(), Some(&catalog()), None);
        agent_select.allies[1].name = Some(("Ally".to_string(), "NA1".to_string()));
        agent_select.allies[1].rank = ranked(19);
        agent_select.allies[1].skins[0] = SkinCell::Standard;
        agent_select.loadouts_loaded = true;

        let live = in_match(Some(&agent_select));

        let ally = &live.allies[0];
        assert_eq!(ally.name, Some(("Ally".to_string(), "NA1".to_string())));
        assert_eq!(ally.rank, ranked(19));
        assert_eq!(ally.skins[0], SkinCell::Standard);
        // The match's loadouts include the enemies, so they still load.
        assert!(!live.loadouts_loaded);
        assert!(matches!(live.enemies[0].rank, RankState::Unavailable(_)));

        let mut other_match = agent_select.clone();
        other_match.match_id = "other".to_string();
        assert_eq!(in_match(Some(&other_match)).allies[0].name, None);
    }

    #[test]
    fn agent_select_shows_picks_and_no_enemies() {
        let live =
            live_match_from_pregame(AccountId::new(), "self", &pregame(), Some(&catalog()), None);

        assert!(live.enemies.is_empty());
        assert_eq!(match_meta(&live), "Agent select");
        let lines: Vec<_> = live
            .allies
            .iter()
            .map(|player| agent_line(player, false, false))
            .collect();
        assert_eq!(
            lines,
            [
                "Jett · Lv 10",
                "Jett (picking) · Lv 10",
                "No agent yet · Lv 10"
            ]
        );
    }

    #[test]
    fn hidden_names_and_levels_show_only_when_asked_for() {
        let mut player = in_match(None).enemies.remove(0);
        player.name = Some(("Streamer".to_string(), "TTV".to_string()));
        player.hides_level = true;

        assert_eq!(
            shown_identity(&player, false, false, None),
            ShownIdentity::Hidden
        );
        assert_eq!(agent_line(&player, false, false), "Jett");
        assert_eq!(
            shown_identity(&player, true, false, None),
            ShownIdentity::Shown {
                name: Some(("Streamer", "TTV")),
                streamer: true,
            }
        );
        assert_eq!(agent_line(&player, true, false), "Jett · Lv 214");
        // The user's own saved accounts are never hidden.
        assert_eq!(
            shown_identity(&player, false, true, None),
            ShownIdentity::Shown {
                name: Some(("Streamer", "TTV")),
                streamer: false,
            }
        );
        assert_eq!(agent_line(&player, false, true), "Jett · Lv 214");
    }

    #[test]
    fn an_own_account_without_a_name_from_riot_shows_its_saved_riot_id() {
        let player = in_match(None).enemies.remove(0);
        assert!(player.incognito && player.name.is_none());

        assert_eq!(
            shown_identity(&player, false, true, Some(("Alt", "0001"))),
            ShownIdentity::Shown {
                name: Some(("Alt", "0001")),
                streamer: false,
            }
        );
        // Another player's saved name is never used.
        assert_eq!(
            shown_identity(&player, true, false, Some(("Alt", "0001"))),
            ShownIdentity::Shown {
                name: None,
                streamer: true,
            }
        );
    }

    #[test]
    fn hidden_names_fill_blank_name_service_entries() {
        let mut live = in_match(None);
        let enemy = live.enemies[0].puuid.clone();
        let names = [
            (enemy.clone(), String::new(), String::new()),
            (
                enemy.to_uppercase(),
                "Streamer".to_string(),
                "TTV".to_string(),
            ),
            (enemy, " ".to_string(), String::new()),
        ];

        apply_names(&mut live, names);

        assert_eq!(
            live.enemies[0].name,
            Some(("Streamer".to_string(), "TTV".to_string()))
        );
    }

    #[test]
    fn standard_skins_are_told_by_the_weapons_default_skin_id() {
        let vandal = SHOWN_WEAPONS[0];
        let content = WeaponContent::from_weapons_and_tiers(
            vec![Weapon {
                uuid: vandal.to_string(),
                display_name: "Vandal".to_string(),
                display_icon: None,
                category: None,
                default_skin_uuid: Some("default-skin".to_string()),
                skins: vec![WeaponSkin {
                    uuid: "prime-skin".to_string(),
                    display_name: "Prime Vandal".to_string(),
                    display_icon: Some("icon".to_string()),
                    content_tier_uuid: None,
                    levels: vec![],
                    chromas: vec![],
                }],
            }],
            &Default::default(),
        );
        let loadout = |skin: &str| -> MatchLoadout {
            serde_json::from_value(serde_json::json!({
                "Subject": "self",
                "Items": {
                    vandal: {"Sockets": {SKIN_SOCKET: {"Item": {"ID": skin}}}}
                }
            }))
            .expect("loadout")
        };

        let standard = skin_cells(&loadout("DEFAULT-SKIN"), &content);
        let prime = skin_cells(&loadout("prime-skin"), &content);

        assert_eq!(standard[0], SkinCell::Standard);
        assert!(matches!(&prime[0], SkinCell::Skin(skin) if skin.display_name == "Prime Vandal"));
        assert_eq!(prime[1], SkinCell::Unavailable);
    }

    #[test]
    fn team_average_leaves_out_unranked_players() {
        let mut players = in_match(None).allies;
        players[0].rank = ranked(18);
        players[1].rank = ranked(21);
        assert_eq!(team_average_rank(&players).as_deref(), Some("Diamond 3"));

        players[0].rank = RankState::Unranked;
        players[1].rank = RankState::Unavailable("error".to_string());
        assert_eq!(team_average_rank(&players), None);
    }

    #[test]
    fn server_label_is_the_city_in_the_game_server_id() {
        assert_eq!(
            server_label("aresriot.aws-rclusterprod-use1-1.na-gp-ashburn-1").as_deref(),
            Some("Ashburn")
        );
        assert_eq!(
            server_label("aresriot.aws-rclusterprod-sae1-1.br-gp-sao-paulo-1").as_deref(),
            Some("Sao Paulo")
        );
        assert_eq!(server_label("pod"), None);
        assert_eq!(server_label(""), None);
    }

    #[test]
    fn match_meta_and_indicator_show_the_score_when_known() {
        let mut live = in_match(None);
        assert_eq!(match_meta(&live), "Ashburn");
        assert_eq!(indicator_detail(&live), "Ascent");

        live.score = Ok(MatchScore { ally: 5, enemy: 2 });
        assert_eq!(match_meta(&live), "Round 8 · Ashburn");
        assert_eq!(indicator_detail(&live), "Ascent · 5 – 2");
    }
}
