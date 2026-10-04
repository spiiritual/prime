use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize, de::IgnoredAny};
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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

    /// Counts a token as expired a minute early, so one that runs out between this check and
    /// the request it's for isn't sent and rejected.
    pub fn is_expired_at(&self, now_unix: i64) -> bool {
        const MARGIN_SECONDS: i64 = 60;
        self.expires_at_unix
            .is_some_and(|expires_at| expires_at <= now_unix + MARGIN_SECONDS)
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
    #[serde(rename = "season_id", default, skip_serializing)]
    #[serde(deserialize_with = "discard_legacy_field")]
    // Compatibility: older files saved the rank's season, which nothing read.
    pub legacy_season_id: (),
}

impl CompetitiveRank {
    pub fn new(tier: i64, rank_name: impl Into<String>, ranked_rating: i64) -> Self {
        Self {
            tier,
            rank_name: rank_name.into(),
            ranked_rating,
            legacy_season_id: (),
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
    #[cfg(test)]
    pub fn penalized_for(rating_name: Option<String>, duration: AccountPenaltyDuration) -> Self {
        Self::Penalized {
            penalties: vec![AccountPenalty::new(rating_name, duration)],
        }
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
    pub puuid: Option<String>,
    pub game_name: Option<String>,
    pub tag_line: Option<String>,
    pub shard: Shard,
    pub session: Option<AuthSession>,
    pub launcher_session: Option<LauncherSessionBackup>,
    pub competitive_rank: Option<CompetitiveRank>,
    #[serde(default)]
    #[serde(skip_serializing)]
    #[serde(deserialize_with = "discard_cached_penalty_status")]
    pub penalty_status: AccountPenaltyStatus,
    pub account_level: Option<i64>,
    #[serde(default)]
    pub region: Option<ValorantRegion>,
    /// The equipped player card, saved whenever the account's loadout loads, for its avatar.
    #[serde(default)]
    pub player_card_id: Option<String>,
    #[serde(rename = "last_refreshed_at_unix", default, skip_serializing)]
    #[serde(deserialize_with = "discard_legacy_field")]
    // Compatibility: older profiles saved a refresh time; "Login saved" uses the capture time.
    pub legacy_last_refreshed_at_unix: (),
    #[serde(rename = "username", default, skip_serializing)]
    #[serde(deserialize_with = "discard_legacy_field")]
    // Compatibility: older profiles saved an optional Riot sign-in name; it is read and dropped.
    pub legacy_username: (),
}

/// Reads and drops a field older files may still contain.
fn discard_legacy_field<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: Deserializer<'de>,
{
    IgnoredAny::deserialize(deserializer)?;
    Ok(())
}

fn discard_cached_penalty_status<'de, D>(deserializer: D) -> Result<AccountPenaltyStatus, D::Error>
where
    D: Deserializer<'de>,
{
    IgnoredAny::deserialize(deserializer)?;
    Ok(AccountPenaltyStatus::Unchecked)
}

impl AccountProfile {
    pub fn new(
        display_name: impl Into<String>,
        shard: Shard,
    ) -> Result<Self, AccountValidationError> {
        let display_name = display_name.into().trim().to_string();

        if display_name.is_empty() {
            return Err(AccountValidationError::EmptyDisplayName);
        }

        Ok(Self {
            id: AccountId::new(),
            display_name,
            puuid: None,
            game_name: None,
            tag_line: None,
            shard,
            session: None,
            launcher_session: None,
            competitive_rank: None,
            penalty_status: AccountPenaltyStatus::default(),
            account_level: None,
            region: None,
            player_card_id: None,
            legacy_last_refreshed_at_unix: (),
            legacy_username: (),
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

    /// Checks that a Riot login belongs to this profile. A profile without a known PUUID yet
    /// accepts any account.
    pub fn check_puuid(&self, puuid: &str) -> Result<(), AccountSessionError> {
        let puuid = puuid.trim();

        match self
            .puuid
            .as_ref()
            .filter(|existing| !existing.trim().is_empty())
        {
            Some(existing) if !existing.eq_ignore_ascii_case(puuid) => {
                Err(AccountSessionError::PuuidMismatch {
                    expected: existing.clone(),
                    actual: puuid.to_string(),
                })
            }
            _ => Ok(()),
        }
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

        self.check_puuid(normalized_puuid)?;

        self.puuid = Some(normalized_puuid.to_string());
        self.game_name = non_empty_string(game_name.into());
        self.tag_line = non_empty_string(tag_line.into());

        Ok(())
    }
}

pub(crate) fn non_empty_string(value: String) -> Option<String> {
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
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum AccountSessionError {
    #[error("captured launcher session did not include a PUUID")]
    MissingCapturedPuuid,
    #[error("this Riot login belongs to PUUID `{actual}`, but this profile is `{expected}`")]
    PuuidMismatch { expected: String, actual: String },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn a_token_counts_as_expired_a_minute_before_it_runs_out() {
        let session = AuthSession::new("token", None, None, "Bearer", Some(3600), 1_000);

        assert!(!session.is_expired_at(1_000 + 3600 - 61));
        assert!(session.is_expired_at(1_000 + 3600 - 60));
    }

    #[test]
    fn rejects_empty_display_name() {
        let err = AccountProfile::new("  ", Shard::Na).unwrap_err();

        assert_eq!(err, AccountValidationError::EmptyDisplayName);
    }

    #[test]
    fn a_profile_saved_without_a_region_still_loads() {
        let mut value =
            serde_json::to_value(AccountProfile::new("Main", Shard::Na).expect("account"))
                .expect("serialize");
        value.as_object_mut().expect("object").remove("region");

        let profile: AccountProfile = serde_json::from_value(value).expect("loads");

        assert_eq!(profile.region, None);
    }

    #[test]
    fn a_saved_region_round_trips() {
        let mut profile = AccountProfile::new("Main", Shard::Na).expect("account");
        profile.region = Some(ValorantRegion::Latam);

        let json = serde_json::to_string(&profile).expect("serialize");
        let loaded: AccountProfile = serde_json::from_str(&json).expect("loads");

        assert_eq!(loaded.region, Some(ValorantRegion::Latam));
        assert!(json.contains("\"region\":\"latam\""), "{json}");
    }

    #[test]
    fn a_profile_saved_with_a_username_still_loads_and_drops_it() {
        let mut value =
            serde_json::to_value(AccountProfile::new("Main", Shard::Na).expect("account"))
                .expect("serialize");
        value
            .as_object_mut()
            .expect("object")
            .insert("username".to_string(), serde_json::json!("player"));

        let profile: AccountProfile = serde_json::from_value(value).expect("loads");
        let json = serde_json::to_string(&profile).expect("serialize");

        assert!(!json.contains("username"), "{json}");
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
        let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
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
        let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
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
        let mut account = AccountProfile::new("Main", Shard::Na).expect("account");

        account
            .apply_riot_identity("puuid-a", "Player", "NA1")
            .expect("identity");

        assert_eq!(account.puuid.as_deref(), Some("puuid-a"));
        assert_eq!(account.riot_id().as_deref(), Some("Player#NA1"));
    }

    #[test]
    fn puuid_check_accepts_the_same_account_and_rejects_another() {
        let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
        assert!(account.check_puuid("anything").is_ok());

        account.puuid = Some("puuid-a".to_string());

        assert!(account.check_puuid("PUUID-A").is_ok());
        assert!(matches!(
            account.check_puuid("puuid-b"),
            Err(AccountSessionError::PuuidMismatch { .. })
        ));
    }

    #[test]
    fn apply_riot_identity_rejects_wrong_puuid() {
        let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
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
        let rank = CompetitiveRank::new(15, "Gold 1", 42);

        assert_eq!(rank.label(), "Gold 1 - 42 RR");
    }

    #[test]
    fn new_account_has_unchecked_penalty_status() {
        let account = AccountProfile::new("Main", Shard::Na).expect("account");

        assert_eq!(account.penalty_status, AccountPenaltyStatus::Unchecked);
    }

    #[test]
    fn penalty_status_tooltip_uses_rating_name_when_available() {
        assert_eq!(
            AccountPenaltyStatus::penalized_for(Some("AFK".to_string()), Default::default())
                .tooltip_label(),
            Some("Penalized: AFK".to_string())
        );
        assert_eq!(
            AccountPenaltyStatus::penalized_for(Some("  ".to_string()), Default::default())
                .tooltip_label(),
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
            AccountPenaltyStatus::Penalized {
                penalties: vec![
                AccountPenalty::new(
                    Some("comms".to_string()),
                    AccountPenaltyDuration::new(Some(1_800_003_600), None),
                ),
                AccountPenalty::new(
                    Some("AFK".to_string()),
                    AccountPenaltyDuration::new(Some(1_800_007_200), Some(1)),
                )
            ]}
            .tooltip_label_at(now),
            Some(
                "Penalized: comms - Ends in 1h 0m 0s\nPenalized: AFK - Ends in 2h 0m 0s (1 game remaining)"
                    .to_string()
            )
        );
    }
}
