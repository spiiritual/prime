use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct RefreshedProfileIdentity {
    pub(in crate::ui) account_id: AccountId,
    pub(in crate::ui) session: AuthSession,
    pub(in crate::ui) launcher_session: Option<LauncherSessionBackup>,
    pub(in crate::ui) puuid: String,
    pub(in crate::ui) game_name: String,
    pub(in crate::ui) tag_line: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct AccountRanksResult {
    pub(in crate::ui) ranks: Vec<AccountRankResult>,
    pub(in crate::ui) failures: Vec<AccountRankFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct AccountRankResult {
    pub(in crate::ui) account_id: AccountId,
    pub(in crate::ui) rank: Result<Option<CompetitiveRank>, String>,
    pub(in crate::ui) account_level: Result<i64, String>,
    pub(in crate::ui) penalty_status: Result<AccountPenaltyStatus, String>,
    pub(in crate::ui) session: AuthSession,
    pub(in crate::ui) launcher_session: Option<LauncherSessionBackup>,
    pub(in crate::ui) identity: ApiIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct AccountRankFailure {
    pub(in crate::ui) account_id: AccountId,
    pub(in crate::ui) error: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct AccountAvailabilityRefresh {
    pub(in crate::ui) accounts: Vec<AccountActivityCheck>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct AccountActivityCheck {
    pub(in crate::ui) account_id: AccountId,
    pub(in crate::ui) availability: AccountAvailability,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum AccountActivity {
    InMatch,
    AgentSelect,
    InLobby,
    Available,
    Unknown(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum AccountAvailability {
    Available,
    Unavailable { reason: String },
    Unknown { reason: String },
}

impl AccountAvailability {
    pub(in crate::ui) fn not_checked() -> Self {
        Self::Unknown {
            reason: "not checked".to_string(),
        }
    }

    pub(in crate::ui) fn checking() -> Self {
        Self::Unknown {
            reason: "checking activity".to_string(),
        }
    }

    pub(in crate::ui) fn activity_check_failed() -> Self {
        Self::Unknown {
            reason: ACCOUNT_ACTIVITY_FAILED_REASON.to_string(),
        }
    }

    pub(in crate::ui) fn label(&self) -> String {
        match self {
            Self::Available => "Available".to_string(),
            Self::Unavailable { reason } => format!("Unavailable ({reason})"),
            Self::Unknown { reason } => format!("Unknown ({reason})"),
        }
    }

    pub(in crate::ui) fn unavailable_reason(&self) -> Option<&str> {
        match self {
            Self::Unavailable { reason } => Some(reason),
            _ => None,
        }
    }
}

impl From<AccountActivity> for AccountAvailability {
    fn from(activity: AccountActivity) -> Self {
        match activity {
            AccountActivity::InMatch => AccountAvailability::Unavailable {
                reason: "in match".to_string(),
            },
            AccountActivity::AgentSelect => AccountAvailability::Unavailable {
                reason: "agent select".to_string(),
            },
            AccountActivity::InLobby => AccountAvailability::Unavailable {
                reason: "in lobby".to_string(),
            },
            AccountActivity::Available => AccountAvailability::Available,
            AccountActivity::Unknown(reason) => AccountAvailability::Unknown { reason },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) enum AccountActivityProbe {
    Present,
    NotFound,
    Failed(String),
}

pub(in crate::ui) const ACCOUNT_ACTIVITY_FAILED_REASON: &str = "activity check failed";

pub(in crate::ui) fn competitive_rank_from_mmr(
    response: &PlayerMmrResponse,
) -> Option<CompetitiveRank> {
    let competitive = response.queue_skills.competitive.as_ref()?;
    let seasons = &competitive.seasonal_info_by_season_id;

    if let Some((season_id, season)) =
        response
            .latest_competitive_update
            .as_ref()
            .and_then(|update| {
                seasons
                    .get(&update.season_id)
                    .map(|season| (update.season_id.as_str(), season))
            })
    {
        return Some(competitive_rank_from_season(season_id, season));
    }

    if let Some(update) = response
        .latest_competitive_update
        .as_ref()
        .filter(|update| update.tier_after_update > 0 || update.ranked_rating_after_update > 0)
    {
        return Some(CompetitiveRank::new(
            update.tier_after_update,
            rank_name_for_competitive_tier(update.tier_after_update),
            update.ranked_rating_after_update,
            non_empty_string(update.season_id.clone()),
        ));
    }

    seasons
        .iter()
        .find(|(_, season)| season_has_rank_data(season))
        .map(|(season_id, season)| competitive_rank_from_season(season_id, season))
}

pub(in crate::ui) fn penalty_status_from_response(
    response: &PlayerPenaltiesResponse,
    now: OffsetDateTime,
) -> AccountPenaltyStatus {
    let mut active_penalties = Vec::new();

    for penalty in &response.penalties {
        let expiry = penalty.expiry.as_deref().and_then(parse_penalty_expiry);

        if !penalty_is_active(penalty.expiry.as_deref(), expiry, now) {
            continue;
        }

        active_penalties.push(ActivePenaltySummary {
            penalty: AccountPenalty::new(
                penalty_display_name(response, penalty),
                AccountPenaltyDuration::new(
                    expiry.map(|expiry| expiry.unix_timestamp()),
                    Some(penalty.games_remaining),
                ),
            ),
            sort_key: penalty_duration_sort_key(expiry, penalty.games_remaining),
        });
    }

    if active_penalties.is_empty() {
        AccountPenaltyStatus::NotPenalized
    } else {
        active_penalties.sort_by_key(|penalty| penalty.sort_key);
        AccountPenaltyStatus::penalized_many(
            active_penalties
                .into_iter()
                .map(|summary| summary.penalty)
                .collect(),
        )
    }
}

fn penalty_display_name(
    response: &PlayerPenaltiesResponse,
    penalty: &crate::riot::models::PlayerPenalty,
) -> Option<String> {
    let rating_name = response
        .infractions
        .iter()
        .find(|infraction| infraction.id == penalty.infraction_id)
        .and_then(|infraction| non_empty_string(infraction.rating_name.clone()));

    if penalty.premier_restriction_effect.is_some() {
        return Some(premier_penalty_display_name(penalty));
    }

    rating_name
}

fn premier_penalty_display_name(penalty: &crate::riot::models::PlayerPenalty) -> String {
    let Some(effect) = penalty.premier_restriction_effect.as_ref() else {
        return "Premier Restriction".to_string();
    };

    match effect.restriction_type.trim().to_ascii_uppercase().as_str() {
        "DISQUALIFIED" => "Premier Disqualification".to_string(),
        "RESTRICTION" | "RESTRICTED" => "Premier Restriction".to_string(),
        restriction_type => {
            let restriction_type = restriction_type
                .split('_')
                .filter(|part| !part.is_empty())
                .map(title_case_ascii)
                .collect::<Vec<_>>()
                .join(" ");

            if restriction_type.is_empty() {
                "Premier Restriction".to_string()
            } else {
                format!("Premier {restriction_type}")
            }
        }
    }
}

fn title_case_ascii(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };

    format!(
        "{}{}",
        first.to_ascii_uppercase(),
        chars.as_str().to_ascii_lowercase()
    )
}

struct ActivePenaltySummary {
    penalty: AccountPenalty,
    sort_key: (i8, i64),
}

fn penalty_duration_sort_key(expiry: Option<OffsetDateTime>, games_remaining: i64) -> (i8, i64) {
    if let Some(expiry) = expiry {
        (0, expiry.unix_timestamp())
    } else if games_remaining > 0 {
        (1, games_remaining)
    } else {
        (2, 0)
    }
}

fn penalty_is_active(
    raw_expiry: Option<&str>,
    expiry: Option<OffsetDateTime>,
    now: OffsetDateTime,
) -> bool {
    match (raw_expiry, expiry) {
        (_, Some(expiry)) => expiry > now,
        (Some(_), None) | (None, None) => true,
    }
}

fn parse_penalty_expiry(expiry: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(expiry.trim(), &Rfc3339).ok()
}

fn competitive_rank_from_season(season_id: &str, season: &MmrSeasonInfo) -> CompetitiveRank {
    CompetitiveRank::new(
        season.competitive_tier,
        rank_name_for_competitive_tier(season.competitive_tier),
        season.ranked_rating,
        non_empty_string(if season.season_id.is_empty() {
            season_id.to_string()
        } else {
            season.season_id.clone()
        }),
    )
}

fn season_has_rank_data(season: &MmrSeasonInfo) -> bool {
    season.competitive_tier > 0
        || season.ranked_rating > 0
        || season.number_of_games > 0
        || season.games_needed_for_rating > 0
}

pub(in crate::ui) fn rank_name_for_competitive_tier(tier: i64) -> String {
    const COMPETITIVE_RANK_NAMES: &[(i64, &str)] = &[
        (3, "Iron 1"),
        (4, "Iron 2"),
        (5, "Iron 3"),
        (6, "Bronze 1"),
        (7, "Bronze 2"),
        (8, "Bronze 3"),
        (9, "Silver 1"),
        (10, "Silver 2"),
        (11, "Silver 3"),
        (12, "Gold 1"),
        (13, "Gold 2"),
        (14, "Gold 3"),
        (15, "Platinum 1"),
        (16, "Platinum 2"),
        (17, "Platinum 3"),
        (18, "Diamond 1"),
        (19, "Diamond 2"),
        (20, "Diamond 3"),
        (21, "Ascendant 1"),
        (22, "Ascendant 2"),
        (23, "Ascendant 3"),
        (24, "Immortal 1"),
        (25, "Immortal 2"),
        (26, "Immortal 3"),
        (27, "Radiant"),
    ];

    COMPETITIVE_RANK_NAMES
        .iter()
        .find_map(|(rank_tier, name)| (*rank_tier == tier).then_some((*name).to_string()))
        .unwrap_or_else(|| {
            if tier > 27 {
                format!("Tier {tier}")
            } else {
                "Unrated".to_string()
            }
        })
}

pub(in crate::ui) async fn fetch_account_ranks(
    accounts: Vec<AccountProfile>,
    client_version: String,
) -> AccountRanksResult {
    let api = match RiotApi::new() {
        Ok(api) => api,
        Err(error) => {
            return AccountRanksResult {
                ranks: Vec::new(),
                failures: accounts
                    .into_iter()
                    .map(|account| AccountRankFailure {
                        account_id: account.id,
                        error: error.to_string(),
                    })
                    .collect(),
            };
        }
    };

    let mut result = AccountRanksResult::default();

    for account in accounts {
        let account_id = account.id;

        match fetch_account_rank(&api, account, client_version.clone()).await {
            Ok(rank) => result.ranks.push(rank),
            Err(error) => result
                .failures
                .push(AccountRankFailure { account_id, error }),
        }
    }

    result
}

pub(in crate::ui) async fn fetch_account_availabilities(
    accounts: Vec<AccountProfile>,
    client_version: String,
) -> AccountAvailabilityRefresh {
    let api = match RiotApi::new() {
        Ok(api) => api,
        Err(_) => {
            return AccountAvailabilityRefresh {
                accounts: accounts
                    .into_iter()
                    .map(|account| AccountActivityCheck {
                        account_id: account.id,
                        availability: AccountAvailability::activity_check_failed(),
                    })
                    .collect(),
            };
        }
    };

    let mut result = AccountAvailabilityRefresh::default();

    for account in accounts {
        result
            .accounts
            .push(fetch_account_availability(&api, account, client_version.clone()).await);
    }

    result
}

pub(in crate::ui) async fn fetch_account_availability(
    api: &RiotApi,
    account: AccountProfile,
    client_version: String,
) -> AccountActivityCheck {
    let account_id = account.id;
    let availability = match resolve_credentials(api, &account, client_version).await {
        Ok(resolved) => match resolved.region {
            Some(region) => fetch_resolved_account_activity(api, &resolved.credentials, region)
                .await
                .into(),
            None => AccountAvailability::activity_check_failed(),
        },
        Err(_) => AccountAvailability::activity_check_failed(),
    };

    AccountActivityCheck {
        account_id,
        availability,
    }
}

async fn fetch_resolved_account_activity(
    api: &RiotApi,
    credentials: &ApiCredentials,
    region: ValorantRegion,
) -> AccountActivity {
    let current_game = account_activity_probe(api.current_game_player(credentials, region).await);
    if current_game != AccountActivityProbe::NotFound {
        return classify_account_activity(
            current_game,
            AccountActivityProbe::NotFound,
            AccountActivityProbe::NotFound,
        );
    }

    let pregame = account_activity_probe(api.pregame_player(credentials, region).await);
    if pregame != AccountActivityProbe::NotFound {
        return classify_account_activity(
            AccountActivityProbe::NotFound,
            pregame,
            AccountActivityProbe::NotFound,
        );
    }

    let party = account_activity_probe(api.party_player(credentials, region).await);
    classify_account_activity(
        AccountActivityProbe::NotFound,
        AccountActivityProbe::NotFound,
        party,
    )
}

fn account_activity_probe(
    result: Result<PlayerActivityEndpointPresence, crate::riot::client::RiotApiError>,
) -> AccountActivityProbe {
    match result {
        Ok(PlayerActivityEndpointPresence::Present) => AccountActivityProbe::Present,
        Ok(PlayerActivityEndpointPresence::Missing) => AccountActivityProbe::NotFound,
        Err(_) => AccountActivityProbe::Failed(ACCOUNT_ACTIVITY_FAILED_REASON.to_string()),
    }
}

pub(in crate::ui) fn classify_account_activity(
    current_game: AccountActivityProbe,
    pregame: AccountActivityProbe,
    party: AccountActivityProbe,
) -> AccountActivity {
    match current_game {
        AccountActivityProbe::Present => AccountActivity::InMatch,
        AccountActivityProbe::Failed(error) => AccountActivity::Unknown(error),
        AccountActivityProbe::NotFound => match pregame {
            AccountActivityProbe::Present => AccountActivity::AgentSelect,
            AccountActivityProbe::Failed(error) => AccountActivity::Unknown(error),
            AccountActivityProbe::NotFound => match party {
                AccountActivityProbe::Present => AccountActivity::InLobby,
                AccountActivityProbe::Failed(error) => AccountActivity::Unknown(error),
                AccountActivityProbe::NotFound => AccountActivity::Available,
            },
        },
    }
}

async fn fetch_account_rank(
    api: &RiotApi,
    account: AccountProfile,
    client_version: String,
) -> Result<AccountRankResult, String> {
    let account_id = account.id;
    let resolved = resolve_credentials(api, &account, client_version).await?;
    let rank = api
        .player_mmr(&resolved.credentials)
        .await
        .map(|response| competitive_rank_from_mmr(&response))
        .map_err(|error| error.to_string());
    let account_level = api
        .account_xp(&resolved.credentials)
        .await
        .map(|response| response.progress.level)
        .map_err(|error| error.to_string());
    let penalty_status = api
        .player_penalties(&resolved.credentials)
        .await
        .map(|response| penalty_status_from_response(&response, OffsetDateTime::now_utc()))
        .map_err(|error| error.to_string());

    if let (Err(rank_error), Err(level_error), Err(penalty_error)) =
        (&rank, &account_level, &penalty_status)
    {
        return Err(format!(
            "rank unavailable: {rank_error}; level unavailable: {level_error}; penalty status unavailable: {penalty_error}"
        ));
    }

    Ok(AccountRankResult {
        account_id,
        rank,
        account_level,
        penalty_status,
        session: resolved.session,
        launcher_session: resolved.launcher_session,
        identity: resolved.identity,
    })
}

pub(in crate::ui) async fn fetch_profile_identity(
    account: AccountProfile,
) -> Result<RefreshedProfileIdentity, String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let api_session = refreshed_api_session(&api, &account).await?;
    let session = api_session.session;
    let player_info = api
        .player_info(&session.access_token)
        .await
        .map_err(|error| error.to_string())?;

    Ok(RefreshedProfileIdentity {
        account_id: account.id,
        session,
        launcher_session: api_session.launcher_session,
        puuid: player_info.sub,
        game_name: player_info.acct.game_name,
        tag_line: player_info.acct.tag_line,
    })
}
