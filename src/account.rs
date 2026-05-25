use std::fmt;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
