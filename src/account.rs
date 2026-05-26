use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
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
pub enum Region {
    #[default]
    Na,
    Latam,
    Br,
    Eu,
    Ap,
    Kr,
    Pbe,
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

    pub fn from_live_region(region: Region) -> Self {
        match region {
            Region::Na | Region::Latam | Region::Br => Shard::Na,
            Region::Eu => Shard::Eu,
            Region::Ap => Shard::Ap,
            Region::Kr => Shard::Kr,
            Region::Pbe => Shard::Pbe,
        }
    }

    pub fn from_live_affinity(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "na" | "latam" | "br" => Some(Shard::Na),
            "eu" => Some(Shard::Eu),
            "ap" => Some(Shard::Ap),
            "kr" => Some(Shard::Kr),
            "pbe" => Some(Shard::Pbe),
            _ => None,
        }
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

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
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
pub struct AccountProfile {
    pub id: AccountId,
    pub display_name: String,
    pub username: Option<String>,
    pub puuid: Option<String>,
    pub game_name: Option<String>,
    pub tag_line: Option<String>,
    pub region: Region,
    pub shard: Shard,
    pub session: Option<AuthSession>,
    #[serde(default)]
    pub launcher_session: Option<LauncherSessionBackup>,
    #[serde(default)]
    pub notes: String,
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
            region: Region::default(),
            shard,
            session: None,
            launcher_session: None,
            notes: String::new(),
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

    pub fn has_api_session(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| !session.access_token.is_empty() && !session.is_expired())
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
    fn maps_regions_to_documented_live_shards() {
        assert_eq!(Shard::from_live_region(Region::Latam), Shard::Na);
        assert_eq!(Shard::from_live_region(Region::Br), Shard::Na);
        assert_eq!(Shard::from_live_region(Region::Eu), Shard::Eu);
        assert_eq!(Shard::from_live_region(Region::Ap), Shard::Ap);
        assert_eq!(Shard::from_live_region(Region::Kr), Shard::Kr);
    }

    #[test]
    fn maps_live_affinity_to_shard() {
        assert_eq!(Shard::from_live_affinity("latam"), Some(Shard::Na));
        assert_eq!(Shard::from_live_affinity("BR"), Some(Shard::Na));
        assert_eq!(Shard::from_live_affinity("eu"), Some(Shard::Eu));
        assert_eq!(Shard::from_live_affinity("unknown"), None);
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
}
