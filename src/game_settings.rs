use std::cmp::Reverse;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;
use time::OffsetDateTime;

use crate::account::AccountId;
use crate::storage::UnknownFields;

mod summary;

pub use summary::{
    CrosshairCenterDot, CrosshairLines, CrosshairOutline, CrosshairSummary,
    GameSettingsProfileSummary, Keybind, Rgba, Setting,
};

pub const VALORANT_PLAYER_SETTINGS_TYPE: &str = "Ares.PlayerSettings";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameSettingsProfilePurpose {
    Profile,
    Backup,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameSettingsProfile {
    pub id: String,
    pub name: String,
    pub purpose: GameSettingsProfilePurpose,
    pub source_account_id: AccountId,
    pub source_display_name: String,
    pub source_puuid: String,
    pub captured_at_unix: i64,
    pub preference_base_url: String,
    pub settings_version: Option<i64>,
    pub preference: ValorantSettingsDocument,
    #[serde(flatten)]
    pub unknown: UnknownFields,
}

impl GameSettingsProfile {
    pub fn metadata(&self) -> Result<GameSettingsProfileMetadata, GameSettingsError> {
        let summary = self
            .preference
            .settings_payload()
            .map(|payload| Box::new(payload.summary()))?;

        Ok(GameSettingsProfileMetadata {
            id: self.id.clone(),
            name: self.name.clone(),
            purpose: self.purpose.clone(),
            source_account_id: self.source_account_id,
            source_display_name: self.source_display_name.clone(),
            source_puuid: self.source_puuid.clone(),
            captured_at_unix: self.captured_at_unix,
            settings_version: self.settings_version,
            summary,
        })
    }
}

/// A saved profile's details without its settings, as the Game settings tab lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct GameSettingsProfileMetadata {
    pub id: String,
    pub name: String,
    pub purpose: GameSettingsProfilePurpose,
    pub source_account_id: AccountId,
    pub source_display_name: String,
    pub source_puuid: String,
    pub captured_at_unix: i64,
    pub settings_version: Option<i64>,
    /// Boxed, since it is much larger than the rest and travels in UI messages.
    pub summary: Box<GameSettingsProfileSummary>,
}

impl fmt::Display for GameSettingsProfileMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValorantSettingsDocument {
    pub raw: Value,
}

impl ValorantSettingsDocument {
    pub fn new(raw: Value) -> Self {
        Self { raw }
    }

    pub fn settings_payload(&self) -> Result<ValorantSettingsPayload, GameSettingsError> {
        decode_settings_payload(&self.raw)
    }

    pub fn replace_settings_payload(
        &mut self,
        payload: &ValorantSettingsPayload,
    ) -> Result<(), GameSettingsError> {
        replace_settings_payload(&mut self.raw, payload)
    }

    pub fn save_body(&self, preference_type: &str) -> Result<Value, GameSettingsError> {
        let payload = self.settings_payload()?;

        Ok(serde_json::json!({
            "type": preference_type,
            "data": encode_settings_payload(&payload)?,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValorantSettingsPayload {
    #[serde(
        default,
        rename = "roamingSetttingsVersion",
        skip_serializing_if = "Option::is_none"
    )]
    pub roaming_settings_version: Option<i64>,
    #[serde(
        default,
        rename = "floatSettings",
        skip_serializing_if = "Option::is_none"
    )]
    pub float_settings: Option<Vec<Value>>,
    #[serde(
        default,
        rename = "intSettings",
        skip_serializing_if = "Option::is_none"
    )]
    pub int_settings: Option<Vec<Value>>,
    #[serde(
        default,
        rename = "boolSettings",
        skip_serializing_if = "Option::is_none"
    )]
    pub bool_settings: Option<Vec<Value>>,
    #[serde(
        default,
        rename = "stringSettings",
        skip_serializing_if = "Option::is_none"
    )]
    pub string_settings: Option<Vec<Value>>,
    #[serde(
        default,
        rename = "actionMappings",
        skip_serializing_if = "Option::is_none"
    )]
    pub action_mappings: Option<Vec<Value>>,
    #[serde(
        default,
        rename = "axisMappings",
        skip_serializing_if = "Option::is_none"
    )]
    pub axis_mappings: Option<Vec<Value>>,
    #[serde(
        default,
        rename = "settingsProfiles",
        skip_serializing_if = "Option::is_none"
    )]
    pub settings_profiles: Option<Vec<Value>>,
    #[serde(
        default,
        rename = "settingsProfileData",
        skip_serializing_if = "Option::is_none"
    )]
    pub settings_profile_data: Option<Vec<Value>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl ValorantSettingsPayload {
    pub fn summary(&self) -> GameSettingsProfileSummary {
        summary::summarize(self)
    }
}

#[derive(Clone, Debug)]
pub struct GameSettingsProfileRepository {
    dir: PathBuf,
}

impl GameSettingsProfileRepository {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn save(&self, profile: &GameSettingsProfile) -> Result<PathBuf, GameSettingsError> {
        fs::create_dir_all(&self.dir)?;

        let path = self.profile_path(&profile.id);
        let tmp = path.with_extension("json.tmp");
        let contents = serde_json::to_vec_pretty(profile)?;

        crate::storage::write_synced(&tmp, &contents)?;
        fs::rename(&tmp, &path)?;

        Ok(path)
    }

    pub fn load(&self, id: &str) -> Result<GameSettingsProfile, GameSettingsError> {
        let contents = fs::read_to_string(self.profile_path(id))?;
        serde_json::from_str(&contents).map_err(GameSettingsError::Json)
    }

    #[cfg(test)]
    pub fn profile_metadata(&self) -> Result<Vec<GameSettingsProfileMetadata>, GameSettingsError> {
        let mut profiles = self.saved_metadata()?;
        profiles.retain(|profile| profile.purpose == GameSettingsProfilePurpose::Profile);
        Ok(profiles)
    }

    /// Every saved settings file, including the backups Apply makes, newest first.
    pub fn saved_metadata(&self) -> Result<Vec<GameSettingsProfileMetadata>, GameSettingsError> {
        let mut profiles = self.metadata()?;
        profiles.sort_by_key(|profile| Reverse(profile.captured_at_unix));
        Ok(profiles)
    }

    /// Renames a saved profile, keeping its settings.
    pub fn rename(
        &self,
        id: &str,
        name: &str,
    ) -> Result<GameSettingsProfileMetadata, GameSettingsError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(GameSettingsError::EmptyName);
        }

        let mut profile = self.load(id)?;
        profile.name = name.to_string();
        self.save(&profile)?;
        profile.metadata()
    }

    /// The settings an account had before a preset was first applied to it: its oldest backup.
    pub fn original_settings(
        &self,
        account_id: AccountId,
    ) -> Result<Option<GameSettingsProfileMetadata>, GameSettingsError> {
        Ok(self.backups_for(account_id)?.pop())
    }

    /// Every backup of an account's settings, newest first.
    pub fn backups_for(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<GameSettingsProfileMetadata>, GameSettingsError> {
        let mut backups = self.saved_metadata()?;
        backups.retain(|profile| {
            profile.purpose == GameSettingsProfilePurpose::Backup
                && profile.source_account_id == account_id
        });
        Ok(backups)
    }

    pub fn delete(&self, id: &str) -> Result<(), GameSettingsError> {
        fs::remove_file(self.profile_path(id))?;
        Ok(())
    }

    fn metadata(&self) -> Result<Vec<GameSettingsProfileMetadata>, GameSettingsError> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }

        let mut profiles = Vec::new();

        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }

            // A single unreadable or outdated profile file should not hide every other profile.
            let Some(metadata) = fs::read_to_string(&path)
                .ok()
                .and_then(|contents| serde_json::from_str::<GameSettingsProfile>(&contents).ok())
                .and_then(|profile| profile.metadata().ok())
            else {
                continue;
            };
            profiles.push(metadata);
        }

        Ok(profiles)
    }

    fn profile_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{}.json", safe_profile_id(id)))
    }
}

pub fn new_profile_id(account_id: AccountId, purpose: GameSettingsProfilePurpose) -> String {
    let purpose = match purpose {
        GameSettingsProfilePurpose::Profile => "profile",
        GameSettingsProfilePurpose::Backup => "backup",
    };
    format!(
        "{}-{}-{account_id}",
        OffsetDateTime::now_utc().unix_timestamp(),
        purpose
    )
}

/// Makes the target's settings an exact copy of the preset's. Riot leaves settings at their
/// default out of the payload, so a setting the preset doesn't list goes back to its default
/// rather than keeping the target's value.
pub fn apply_preset(
    preset: &ValorantSettingsDocument,
    target: &mut ValorantSettingsDocument,
) -> Result<(), GameSettingsError> {
    target.replace_settings_payload(&preset.settings_payload()?)
}

fn decode_settings_payload(raw: &Value) -> Result<ValorantSettingsPayload, GameSettingsError> {
    match data_value(raw)? {
        Value::Object(_) => serde_json::from_value(data_value(raw)?.clone())
            .map_err(GameSettingsError::SettingsJson),
        Value::String(data) => decode_base64_deflate_settings_payload(data),
        _ => Err(GameSettingsError::InvalidSettingsPayload),
    }
}

fn decode_base64_deflate_settings_payload(
    data: &str,
) -> Result<ValorantSettingsPayload, GameSettingsError> {
    let compressed = BASE64_STANDARD.decode(data.trim())?;
    let mut decoder = DeflateDecoder::new(compressed.as_slice());
    let mut json = String::new();
    decoder.read_to_string(&mut json)?;

    serde_json::from_str(&json).map_err(GameSettingsError::SettingsJson)
}

fn encode_settings_payload(payload: &ValorantSettingsPayload) -> Result<String, GameSettingsError> {
    let json = serde_json::to_vec(payload).map_err(GameSettingsError::SettingsJson)?;
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&json)?;
    let compressed = encoder.finish()?;

    Ok(BASE64_STANDARD.encode(compressed))
}

fn replace_settings_payload(
    raw: &mut Value,
    payload: &ValorantSettingsPayload,
) -> Result<(), GameSettingsError> {
    let encoded = serde_json::to_value(payload).map_err(GameSettingsError::SettingsJson)?;
    let key = match raw {
        Value::Object(map) => ["data", "Data"]
            .into_iter()
            .find(|key| map.contains_key(*key)),
        _ => None,
    };

    match key {
        Some(key) if raw[key].is_string() => {
            raw[key] = Value::String(encode_settings_payload(payload)?);
        }
        Some(key) => raw[key] = encoded,
        None => *raw = encoded,
    }

    Ok(())
}

fn data_value(raw: &Value) -> Result<&Value, GameSettingsError> {
    match raw {
        Value::Object(map) => Ok(map.get("data").or_else(|| map.get("Data")).unwrap_or(raw)),
        _ => Err(GameSettingsError::InvalidSettingsPayload),
    }
}

fn safe_profile_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '_',
        })
        .collect()
}

#[derive(Debug, Error)]
pub enum GameSettingsError {
    #[error("Riot did not return VALORANT settings for this account")]
    InvalidSettingsPayload,
    #[error("settings payload data is not valid base64")]
    Base64(#[from] base64::DecodeError),
    #[error("VALORANT settings data could not be read: {0}")]
    SettingsJson(serde_json::Error),
    #[error("settings profile JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("settings profile I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("a preset name can't be empty")]
    EmptyName,
}

#[cfg(test)]
mod tests;
