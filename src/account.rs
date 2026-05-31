use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccountId(Uuid);

impl AccountId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for AccountId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shard {
    #[default]
    Na,
    Eu,
    Ap,
    Kr,
    Pbe,
}

impl Shard {
    pub const ALL: [Shard; 5] = [Shard::Na, Shard::Eu, Shard::Ap, Shard::Kr, Shard::Pbe];

    pub fn as_str(self) -> &'static str {
        match self {
            Shard::Na => "na",
            Shard::Eu => "eu",
            Shard::Ap => "ap",
            Shard::Kr => "kr",
            Shard::Pbe => "pbe",
        }
    }

    pub fn from_live_affinity(value: &str) -> Option<Self> {
        ValorantRegion::from_live_affinity(value)
            .map(ValorantRegion::shard)
            .or_else(|| {
                value
                    .trim()
                    .eq_ignore_ascii_case("pbe")
                    .then_some(Shard::Pbe)
            })
    }
}

impl fmt::Display for Shard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Shard {
    type Err = AccountValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "na" => Ok(Shard::Na),
            "eu" => Ok(Shard::Eu),
            "ap" => Ok(Shard::Ap),
            "kr" => Ok(Shard::Kr),
            "pbe" => Ok(Shard::Pbe),
            other => Err(AccountValidationError::UnknownShard(other.to_string())),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValorantRegion {
    Na,
    Latam,
    Br,
    Eu,
    Ap,
    Kr,
}

impl ValorantRegion {
    pub fn as_str(self) -> &'static str {
        match self {
            ValorantRegion::Na => "na",
            ValorantRegion::Latam => "latam",
            ValorantRegion::Br => "br",
            ValorantRegion::Eu => "eu",
            ValorantRegion::Ap => "ap",
            ValorantRegion::Kr => "kr",
        }
    }

    pub fn shard(self) -> Shard {
        match self {
            ValorantRegion::Na | ValorantRegion::Latam | ValorantRegion::Br => Shard::Na,
            ValorantRegion::Eu => Shard::Eu,
            ValorantRegion::Ap => Shard::Ap,
            ValorantRegion::Kr => Shard::Kr,
        }
    }

    pub fn from_live_affinity(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "na" => Some(ValorantRegion::Na),
            "latam" => Some(ValorantRegion::Latam),
            "br" => Some(ValorantRegion::Br),
            "eu" => Some(ValorantRegion::Eu),
            "ap" => Some(ValorantRegion::Ap),
            "kr" => Some(ValorantRegion::Kr),
            _ => None,
        }
    }
}

impl fmt::Display for ValorantRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthSession {
    pub access_token: String,
    pub id_token: Option<String>,
    pub entitlements_token: Option<String>,
    pub token_type: String,
    pub expires_at_unix: Option<i64>,
}

impl fmt::Debug for AuthSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthSession")
            .field("access_token", &"<redacted>")
            .field("id_token", &self.id_token.as_ref().map(|_| "<redacted>"))
            .field(
                "entitlements_token",
                &self.entitlements_token.as_ref().map(|_| "<redacted>"),
            )
            .field("token_type", &self.token_type)
            .field("expires_at_unix", &self.expires_at_unix)
            .finish()
    }
}

impl AuthSession {
    pub fn new(
        access_token: impl Into<String>,
        id_token: Option<String>,
        entitlements_token: Option<String>,
        token_type: impl Into<String>,
        expires_in_seconds: Option<i64>,
        now_unix: i64,
    ) -> Self {
        let expires_at_unix = expires_in_seconds.map(|seconds| now_unix + seconds);

        Self {
            access_token: access_token.into(),
            id_token,
            entitlements_token,
            token_type: token_type.into(),
            expires_at_unix,
        }
    }

    pub fn is_expired_at(&self, now_unix: i64) -> bool {
        self.expires_at_unix
            .is_some_and(|expires_at| expires_at <= now_unix)
    }

    pub fn is_expired(&self) -> bool {
        self.is_expired_at(OffsetDateTime::now_utc().unix_timestamp())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LauncherSessionBackup {
    pub data_dir: PathBuf,
    pub captured_at_unix: i64,
    pub puuid: String,
}

impl LauncherSessionBackup {
    pub fn private_settings_path(&self) -> PathBuf {
        self.data_dir.join("RiotGamesPrivateSettings.yaml")
    }

    pub fn is_ready(&self) -> bool {
        !self.puuid.trim().is_empty() && self.private_settings_path().is_file()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompetitiveRank {
    pub tier: i64,
    pub rank_name: String,
    pub ranked_rating: i64,
    pub season_id: Option<String>,
}

impl CompetitiveRank {
    pub fn new(
        tier: i64,
        rank_name: impl Into<String>,
        ranked_rating: i64,
        season_id: Option<String>,
    ) -> Self {
        Self {
            tier,
            rank_name: rank_name.into(),
            ranked_rating,
            season_id,
        }
    }

    pub fn label(&self) -> String {
        format!("{} - {} RR", self.rank_name, self.ranked_rating)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountPenaltyDuration {
    pub ends_at_unix: Option<i64>,
    pub games_remaining: Option<i64>,
}

impl AccountPenaltyDuration {
    pub fn new(ends_at_unix: Option<i64>, games_remaining: Option<i64>) -> Self {
        Self {
            ends_at_unix,
            games_remaining: games_remaining.filter(|games| *games > 0),
        }
    }

    pub fn unknown() -> Self {
        Self::default()
    }

    fn label_at(&self, now: OffsetDateTime) -> Option<String> {
        let time_label = self.ends_at_unix.map(|ends_at_unix| {
            let seconds = ends_at_unix.saturating_sub(now.unix_timestamp());

            if seconds <= 0 {
                "Ends soon".to_string()
            } else {
                format!("Ends in {}", format_penalty_duration(seconds))
            }
        });
        match (time_label, self.games_remaining) {
            (Some(time_label), Some(1)) => Some(format!("{time_label} (1 game remaining)")),
            (Some(time_label), Some(games)) => {
                Some(format!("{time_label} ({games} games remaining)"))
            }
            (Some(time_label), None) => Some(time_label),
            (None, Some(1)) => Some("Ends after 1 game".to_string()),
            (None, Some(games)) => Some(format!("Ends after {games} games")),
            (None, None) => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountPenalty {
    pub rating_name: Option<String>,
    pub duration: AccountPenaltyDuration,
}

impl AccountPenalty {
    pub fn new(rating_name: Option<String>, duration: AccountPenaltyDuration) -> Self {
        Self {
            rating_name: rating_name.and_then(non_empty_string),
            duration,
        }
    }

    fn tooltip_label_at(&self, now: OffsetDateTime) -> String {
        let base = match self.rating_name.as_ref() {
            Some(rating_name) => format!("Penalized: {rating_name}"),
            None => "Penalized".to_string(),
        };

        penalty_tooltip_label(base, &self.duration, now)
    }

    fn tooltip_summary_at(&self, now: OffsetDateTime) -> String {
        let base = match self.rating_name.as_ref() {
            Some(rating_name) => format!("Penalized: {rating_name}"),
            None => "Penalized".to_string(),
        };

        match self.duration.label_at(now) {
            Some(duration_label) => format!("{base} - {duration_label}"),
            None => base,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "status", deny_unknown_fields)]
pub enum AccountPenaltyStatus {
    #[default]
    Unchecked,
    NotPenalized,
    Penalized {
        penalties: Vec<AccountPenalty>,
    },
}

impl AccountPenaltyStatus {
    pub fn penalized(rating_name: Option<String>) -> Self {
        Self::penalized_for(rating_name, AccountPenaltyDuration::unknown())
    }

    pub fn penalized_for(rating_name: Option<String>, duration: AccountPenaltyDuration) -> Self {
        Self::penalized_many(vec![AccountPenalty::new(rating_name, duration)])
    }

    pub fn penalized_many(penalties: Vec<AccountPenalty>) -> Self {
        let penalties = penalties
            .into_iter()
            .map(|penalty| AccountPenalty::new(penalty.rating_name, penalty.duration))
            .collect::<Vec<_>>();

        Self::Penalized {
            penalties: if penalties.is_empty() {
                vec![AccountPenalty::new(None, AccountPenaltyDuration::unknown())]
            } else {
                penalties
            },
        }
    }

    pub fn is_penalized(&self) -> bool {
        matches!(self, Self::Penalized { .. })
    }

    pub fn tooltip_label(&self) -> Option<String> {
        self.tooltip_label_at(OffsetDateTime::now_utc())
    }

    pub fn tooltip_label_at(&self, now: OffsetDateTime) -> Option<String> {
        match self {
            Self::Penalized { penalties } if penalties.len() == 1 => penalties
                .first()
                .map(|penalty| penalty.tooltip_label_at(now)),
            Self::Penalized { penalties } => Some(
                penalties
                    .iter()
                    .map(|penalty| penalty.tooltip_summary_at(now))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            Self::Unchecked | Self::NotPenalized => None,
        }
    }
}

impl<'de> Deserialize<'de> for AccountPenaltyStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case", tag = "status", deny_unknown_fields)]
        enum RawAccountPenaltyStatus {
            Unchecked,
            NotPenalized,
            Penalized {
                #[serde(default)]
                penalties: Vec<AccountPenalty>,
                rating_name: Option<String>,
                #[serde(default)]
                duration: AccountPenaltyDuration,
            },
        }

        match RawAccountPenaltyStatus::deserialize(deserializer)? {
            RawAccountPenaltyStatus::Unchecked => Ok(Self::Unchecked),
            RawAccountPenaltyStatus::NotPenalized => Ok(Self::NotPenalized),
            RawAccountPenaltyStatus::Penalized {
                penalties,
                rating_name,
                duration,
            } => {
                if penalties.is_empty() {
                    Ok(Self::penalized_for(rating_name, duration))
                } else {
                    Ok(Self::penalized_many(penalties))
                }
            }
        }
    }
}

fn penalty_tooltip_label(
    base: String,
    duration: &AccountPenaltyDuration,
    now: OffsetDateTime,
) -> String {
    match duration.label_at(now) {
        Some(duration_label) => format!("{base}\n{duration_label}"),
        None => base,
    }
}

fn format_penalty_duration(seconds: i64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;

    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountProfile {
    pub id: AccountId,
    pub display_name: String,
    pub username: Option<String>,
    pub puuid: Option<String>,
    pub game_name: Option<String>,
    pub tag_line: Option<String>,
    pub shard: Shard,
    pub session: Option<AuthSession>,
    pub launcher_session: Option<LauncherSessionBackup>,
    pub competitive_rank: Option<CompetitiveRank>,
    #[serde(default)]
    pub penalty_status: AccountPenaltyStatus,
    pub account_level: Option<i64>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub last_refreshed_at_unix: Option<i64>,
}

impl AccountProfile {
    pub fn new(
        display_name: impl Into<String>,
        username: Option<String>,
        shard: Shard,
    ) -> Result<Self, AccountValidationError> {
        let display_name = display_name.into().trim().to_string();

        if display_name.is_empty() {
            return Err(AccountValidationError::EmptyDisplayName);
        }

        Ok(Self {
            id: AccountId::new(),
            display_name,
            username: username.and_then(non_empty_string),
            puuid: None,
            game_name: None,
            tag_line: None,
            shard,
            session: None,
            launcher_session: None,
            competitive_rank: None,
            penalty_status: AccountPenaltyStatus::default(),
            account_level: None,
            last_refreshed_at_unix: None,
        })
    }

    pub fn riot_id(&self) -> Option<String> {
        match (&self.game_name, &self.tag_line) {
            (Some(game_name), Some(tag_line)) if !game_name.is_empty() && !tag_line.is_empty() => {
                Some(format!("{game_name}#{tag_line}"))
            }
            _ => None,
        }
    }

    pub fn summary(&self) -> String {
        if let Some(riot_id) = self.riot_id() {
            format!("{} ({riot_id}, {})", self.display_name, self.shard)
        } else if let Some(username) = &self.username {
            format!("{} ({username}, {})", self.display_name, self.shard)
        } else {
            format!("{} ({})", self.display_name, self.shard)
        }
    }

    pub fn has_launcher_session(&self) -> bool {
        self.launcher_session
            .as_ref()
            .is_some_and(LauncherSessionBackup::is_ready)
    }

    pub fn attach_launcher_session(
        &mut self,
        backup: LauncherSessionBackup,
    ) -> Result<(), AccountSessionError> {
        let captured_puuid = backup.puuid.trim();

        if captured_puuid.is_empty() {
            return Err(AccountSessionError::MissingCapturedPuuid);
        }

        if let Some(existing_puuid) = self.puuid.as_ref().filter(|puuid| !puuid.trim().is_empty())
            && !existing_puuid.eq_ignore_ascii_case(captured_puuid)
        {
            return Err(AccountSessionError::PuuidMismatch {
                expected: existing_puuid.clone(),
                actual: captured_puuid.to_string(),
            });
        }

        self.puuid = Some(captured_puuid.to_string());
        self.launcher_session = Some(backup);

        Ok(())
    }

    pub fn apply_riot_identity(
        &mut self,
        puuid: impl Into<String>,
        game_name: impl Into<String>,
        tag_line: impl Into<String>,
    ) -> Result<(), AccountSessionError> {
        let puuid = puuid.into();
        let normalized_puuid = puuid.trim();

        if normalized_puuid.is_empty() {
            return Err(AccountSessionError::MissingCapturedPuuid);
        }

        if let Some(existing_puuid) = self.puuid.as_ref().filter(|puuid| !puuid.trim().is_empty())
            && !existing_puuid.eq_ignore_ascii_case(normalized_puuid)
        {
            return Err(AccountSessionError::PuuidMismatch {
                expected: existing_puuid.clone(),
                actual: normalized_puuid.to_string(),
            });
        }

        self.puuid = Some(normalized_puuid.to_string());
        self.game_name = non_empty_string(game_name.into());
        self.tag_line = non_empty_string(tag_line.into());

        Ok(())
    }
}

fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum AccountValidationError {
    #[error("display name cannot be empty")]
    EmptyDisplayName,
    #[error("unknown Valorant shard `{0}`")]
    UnknownShard(String),
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum AccountSessionError {
    #[error("captured launcher session did not include a PUUID")]
    MissingCapturedPuuid,
    #[error(
        "captured launcher session belongs to PUUID `{actual}`, but this profile is `{expected}`"
    )]
    PuuidMismatch { expected: String, actual: String },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rejects_empty_display_name() {
        let err = AccountProfile::new("  ", None, Shard::Na).unwrap_err();

        assert_eq!(err, AccountValidationError::EmptyDisplayName);
    }

    #[test]
    fn normalizes_optional_username() {
        let account = AccountProfile::new("Main", Some(" player ".to_string()), Shard::Eu)
            .expect("valid account");

        assert_eq!(account.username.as_deref(), Some("player"));
    }

    #[test]
    fn maps_live_affinity_to_shard() {
        assert_eq!(Shard::from_live_affinity("latam"), Some(Shard::Na));
        assert_eq!(Shard::from_live_affinity("BR"), Some(Shard::Na));
        assert_eq!(Shard::from_live_affinity("eu"), Some(Shard::Eu));
        assert_eq!(Shard::from_live_affinity("unknown"), None);
    }

    #[test]
    fn maps_live_affinity_to_region() {
        assert_eq!(
            ValorantRegion::from_live_affinity("latam"),
            Some(ValorantRegion::Latam)
        );
        assert_eq!(
            ValorantRegion::from_live_affinity("BR"),
            Some(ValorantRegion::Br)
        );
        assert_eq!(ValorantRegion::from_live_affinity("pbe"), None);
    }

    #[test]
    fn redacts_auth_session_debug() {
        let session = AuthSession::new(
            "secret-access-token",
            Some("secret-id-token".to_string()),
            Some("secret-entitlement-token".to_string()),
            "Bearer",
            Some(3600),
            100,
        );

        let debug = format!("{session:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret-access-token"));
        assert!(!debug.contains("secret-id-token"));
        assert!(!debug.contains("secret-entitlement-token"));
    }

    #[test]
    fn launcher_session_backup_requires_private_settings_file() {
        let dir = tempdir().expect("backup dir");
        let backup = LauncherSessionBackup {
            data_dir: dir.path().to_path_buf(),
            captured_at_unix: 100,
            puuid: "puuid-a".to_string(),
        };

        assert!(!backup.is_ready());

        fs::write(backup.private_settings_path(), "settings").expect("private settings");

        assert!(backup.is_ready());
    }

    #[test]
    fn attach_launcher_session_sets_missing_puuid() {
        let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
        let backup = LauncherSessionBackup {
            data_dir: PathBuf::from("backup"),
            captured_at_unix: 100,
            puuid: "puuid-a".to_string(),
        };

        account
            .attach_launcher_session(backup)
            .expect("attach launcher session");

        assert_eq!(account.puuid.as_deref(), Some("puuid-a"));
        assert!(account.launcher_session.is_some());
    }

    #[test]
    fn attach_launcher_session_rejects_wrong_puuid() {
        let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
        account.puuid = Some("puuid-a".to_string());
        let backup = LauncherSessionBackup {
            data_dir: PathBuf::from("backup"),
            captured_at_unix: 100,
            puuid: "puuid-b".to_string(),
        };

        let err = account
            .attach_launcher_session(backup)
            .expect_err("mismatched puuid");

        assert_eq!(
            err,
            AccountSessionError::PuuidMismatch {
                expected: "puuid-a".to_string(),
                actual: "puuid-b".to_string()
            }
        );
        assert!(account.launcher_session.is_none());
    }

    #[test]
    fn apply_riot_identity_sets_riot_id() {
        let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");

        account
            .apply_riot_identity("puuid-a", "Player", "NA1")
            .expect("identity");

        assert_eq!(account.puuid.as_deref(), Some("puuid-a"));
        assert_eq!(account.riot_id().as_deref(), Some("Player#NA1"));
    }

    #[test]
    fn apply_riot_identity_rejects_wrong_puuid() {
        let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
        account.puuid = Some("puuid-a".to_string());

        let err = account
            .apply_riot_identity("puuid-b", "Player", "NA1")
            .expect_err("mismatch");

        assert!(matches!(err, AccountSessionError::PuuidMismatch { .. }));
        assert_eq!(account.game_name, None);
        assert_eq!(account.tag_line, None);
    }

    #[test]
    fn competitive_rank_formats_rank_and_rr() {
        let rank = CompetitiveRank::new(15, "Gold 1", 42, Some("season".to_string()));

        assert_eq!(rank.label(), "Gold 1 - 42 RR");
    }

    #[test]
    fn new_account_has_unchecked_penalty_status() {
        let account = AccountProfile::new("Main", None, Shard::Na).expect("account");

        assert_eq!(account.penalty_status, AccountPenaltyStatus::Unchecked);
        assert!(!account.penalty_status.is_penalized());
    }

    #[test]
    fn penalty_status_tooltip_uses_rating_name_when_available() {
        assert_eq!(
            AccountPenaltyStatus::penalized(Some("AFK".to_string())).tooltip_label(),
            Some("Penalized: AFK".to_string())
        );
        assert_eq!(
            AccountPenaltyStatus::penalized(Some("  ".to_string())).tooltip_label(),
            Some("Penalized".to_string())
        );
    }

    #[test]
    fn penalty_status_tooltip_includes_duration_when_available() {
        let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();

        assert_eq!(
            AccountPenaltyStatus::penalized_for(
                Some("AFK".to_string()),
                AccountPenaltyDuration::new(Some(1_800_003_661), Some(2)),
            )
            .tooltip_label_at(now),
            Some("Penalized: AFK\nEnds in 1h 1m 1s (2 games remaining)".to_string())
        );
        assert_eq!(
            AccountPenaltyStatus::penalized_for(None, AccountPenaltyDuration::new(None, Some(1)),)
                .tooltip_label_at(now),
            Some("Penalized\nEnds after 1 game".to_string())
        );
    }

    #[test]
    fn penalty_status_tooltip_lists_multiple_penalties() {
        let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();

        assert_eq!(
            AccountPenaltyStatus::penalized_many(vec![
                AccountPenalty::new(
                    Some("comms".to_string()),
                    AccountPenaltyDuration::new(Some(1_800_003_600), None),
                ),
                AccountPenalty::new(
                    Some("AFK".to_string()),
                    AccountPenaltyDuration::new(Some(1_800_007_200), Some(1)),
                )
            ])
            .tooltip_label_at(now),
            Some(
                "Penalized: comms - Ends in 1h 0m 0s\nPenalized: AFK - Ends in 2h 0m 0s (1 game remaining)"
                    .to_string()
            )
        );
    }

    #[test]
    fn penalty_status_loads_without_duration() {
        let status: AccountPenaltyStatus = serde_json::from_value(serde_json::json!({
            "status": "penalized",
            "rating_name": "comms"
        }))
        .expect("legacy penalty status");

        assert_eq!(
            status,
            AccountPenaltyStatus::penalized(Some("comms".to_string()))
        );
    }

    #[test]
    fn penalty_status_loads_current_penalty_list() {
        let status: AccountPenaltyStatus = serde_json::from_value(serde_json::json!({
            "status": "penalized",
            "penalties": [
                {
                    "rating_name": "comms",
                    "duration": {
                        "ends_at_unix": 1_800_003_600i64,
                        "games_remaining": null
                    }
                },
                {
                    "rating_name": "AFK",
                    "duration": {
                        "ends_at_unix": null,
                        "games_remaining": 1
                    }
                }
            ]
        }))
        .expect("current penalty status");

        assert_eq!(
            status,
            AccountPenaltyStatus::penalized_many(vec![
                AccountPenalty::new(
                    Some("comms".to_string()),
                    AccountPenaltyDuration::new(Some(1_800_003_600), None)
                ),
                AccountPenalty::new(
                    Some("AFK".to_string()),
                    AccountPenaltyDuration::new(None, Some(1))
                )
            ])
        );
    }
}
