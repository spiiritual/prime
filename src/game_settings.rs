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
#[serde(deny_unknown_fields)]
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
    let value: Value = serde_json::from_str(&json).map_err(GameSettingsError::SettingsJson)?;

    serde_json::from_value(value).map_err(GameSettingsError::SettingsJson)
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

    match raw {
        Value::Object(map) if map.get("data").is_some() => {
            if map.get("data").is_some_and(Value::is_string) {
                map.insert(
                    "data".to_string(),
                    Value::String(encode_settings_payload(payload)?),
                );
            } else {
                map.insert("data".to_string(), encoded);
            }
        }
        Value::Object(map) if map.get("Data").is_some() => {
            if map.get("Data").is_some_and(Value::is_string) {
                map.insert(
                    "Data".to_string(),
                    Value::String(encode_settings_payload(payload)?),
                );
            } else {
                map.insert("Data".to_string(), encoded);
            }
        }
        _ => {
            *raw = encoded;
        }
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
mod tests {
    use super::*;

    fn payload(value: Value) -> ValorantSettingsPayload {
        serde_json::from_value(value).expect("payload")
    }

    #[test]
    fn decodes_payload_from_raw_data_object() {
        let document = ValorantSettingsDocument::new(serde_json::json!({
            "type": VALORANT_PLAYER_SETTINGS_TYPE,
            "data": {
                "roamingSetttingsVersion": 15,
                "floatSettings": [{"settingEnum": "MouseSensitivity", "value": 0.4}]
            }
        }));

        let payload = document.settings_payload().expect("settings");

        assert_eq!(payload.roaming_settings_version, Some(15));
        assert_eq!(payload.float_settings.expect("float").len(), 1);
    }

    #[test]
    fn decodes_payload_from_base64_raw_deflate_data() {
        let payload = payload(serde_json::json!({
            "roamingSetttingsVersion": 15,
            "floatSettings": [{"settingEnum": "MouseSensitivity", "value": 0.4}]
        }));
        let data = encode_settings_payload(&payload).expect("encode");
        let document = ValorantSettingsDocument::new(serde_json::json!({
            "type": VALORANT_PLAYER_SETTINGS_TYPE,
            "data": data
        }));

        let decoded = document.settings_payload().expect("settings");

        assert_eq!(decoded.roaming_settings_version, Some(15));
        assert_eq!(decoded.float_settings.expect("float").len(), 1);
    }

    #[test]
    fn settings_json_error_explains_the_parse_failure() {
        let document = ValorantSettingsDocument::new(serde_json::json!({
            "type": VALORANT_PLAYER_SETTINGS_TYPE,
            "data": {"floatSettings": "not a list"}
        }));

        let message = document
            .settings_payload()
            .expect_err("invalid")
            .to_string();

        assert!(message.starts_with("VALORANT settings data could not be read: "));
        assert!(message.contains("invalid type"), "{message}");
    }

    #[test]
    fn save_body_uses_verified_player_preferences_shape() {
        let payload = payload(serde_json::json!({
            "roamingSetttingsVersion": 15,
            "boolSettings": [{"settingEnum": "MinimapRotates", "value": true}]
        }));
        let data = encode_settings_payload(&payload).expect("encode");
        let document = ValorantSettingsDocument::new(serde_json::json!({
            "type": VALORANT_PLAYER_SETTINGS_TYPE,
            "data": data,
            "modified": 123
        }));

        let body = document
            .save_body(VALORANT_PLAYER_SETTINGS_TYPE)
            .expect("body");

        assert_eq!(
            body["type"],
            serde_json::json!(VALORANT_PLAYER_SETTINGS_TYPE)
        );
        assert!(body["data"].as_str().is_some());
        assert!(body.get("modified").is_none());
    }

    #[test]
    fn replace_payload_keeps_base64_raw_deflate_envelope_writable() {
        let original = payload(serde_json::json!({
            "roamingSetttingsVersion": 15,
            "floatSettings": [{"settingEnum": "MouseSensitivity", "value": 0.4}]
        }));
        let replacement = payload(serde_json::json!({
            "roamingSetttingsVersion": 16,
            "floatSettings": [{"settingEnum": "MouseSensitivity", "value": 0.7}]
        }));
        let data = encode_settings_payload(&original).expect("encode");
        let mut document = ValorantSettingsDocument::new(serde_json::json!({
            "type": VALORANT_PLAYER_SETTINGS_TYPE,
            "data": data
        }));

        document
            .replace_settings_payload(&replacement)
            .expect("replace");
        let body = document
            .save_body(VALORANT_PLAYER_SETTINGS_TYPE)
            .expect("body");
        let saved = ValorantSettingsDocument::new(body)
            .settings_payload()
            .expect("settings");

        assert_eq!(saved.roaming_settings_version, Some(16));
        assert_eq!(
            saved.float_settings.as_ref().expect("float")[0]["value"],
            serde_json::json!(0.7)
        );
    }

    fn encoded_document(payload: &ValorantSettingsPayload) -> ValorantSettingsDocument {
        ValorantSettingsDocument::new(serde_json::json!({
            "type": VALORANT_PLAYER_SETTINGS_TYPE,
            "data": encode_settings_payload(payload).expect("encode")
        }))
    }

    // Riot leaves settings at their default out of the payload, so these two are shaped like
    // real accounts: each lists only what its player changed.
    fn preset_account() -> ValorantSettingsPayload {
        payload(serde_json::json!({
            "roamingSetttingsVersion": 15,
            "floatSettings": [
                {"settingEnum": "EAresFloatSettingName::MouseSensitivity", "value": 0.3},
                {"settingEnum": "EAresFloatSettingName::OverallVolume", "value": 0.5}
            ],
            "boolSettings": [
                {"settingEnum": "EAresBoolSettingName::PushToTalkEnabled", "value": true}
            ],
            "stringSettings": [
                {"settingEnum": "EAresStringSettingName::TeamPushToTalkKey", "value": "ThumbMouseButton2"}
            ],
            "actionMappings": [{"name": "Ping", "key": "MiddleMouseButton"}]
        }))
    }

    fn target_account() -> ValorantSettingsPayload {
        payload(serde_json::json!({
            "roamingSetttingsVersion": 15,
            "floatSettings": [
                {"settingEnum": "EAresFloatSettingName::MouseSensitivity", "value": 0.8},
                {"settingEnum": "EAresFloatSettingName::VoiceOverVolume", "value": 0.2}
            ],
            "intSettings": [
                {"settingEnum": "EAresIntSettingName::ColorBlindMode", "value": 1}
            ],
            "boolSettings": [
                {"settingEnum": "EAresBoolSettingName::ShowNewPlayerTips", "value": true}
            ],
            "actionMappings": [
                {"name": "Ping", "key": "MiddleMouseButton"},
                {"name": "ShowScoreboard", "key": "None"}
            ]
        }))
    }

    #[test]
    fn applying_a_preset_copies_exactly_its_settings() {
        let preset = encoded_document(&preset_account());
        let mut target = encoded_document(&target_account());

        apply_preset(&preset, &mut target).expect("apply");

        assert_eq!(
            target.settings_payload().expect("settings"),
            preset_account()
        );
        assert!(target.raw["data"].is_string(), "{:?}", target.raw);
    }

    #[test]
    fn applying_the_original_after_a_preset_gives_back_the_original() {
        let original = encoded_document(&target_account());
        let preset = encoded_document(&preset_account());
        let mut account = original.clone();

        apply_preset(&preset, &mut account).expect("apply preset");
        apply_preset(&original, &mut account).expect("restore original");

        assert_eq!(
            account.settings_payload().expect("settings"),
            target_account()
        );
    }

    #[test]
    fn profile_listing_skips_unreadable_profile_files() {
        let dir = tempfile::tempdir().expect("profile dir");
        let repository = GameSettingsProfileRepository::new(dir.path());
        let profile = GameSettingsProfile {
            id: new_profile_id(AccountId::new(), GameSettingsProfilePurpose::Profile),
            name: "Main".to_string(),
            purpose: GameSettingsProfilePurpose::Profile,
            source_account_id: AccountId::new(),
            source_display_name: "Main".to_string(),
            source_puuid: "puuid".to_string(),
            captured_at_unix: 100,
            preference_base_url: "https://player-preferences-usw2.pp.sgp.pvp.net".to_string(),
            settings_version: Some(15),
            preference: ValorantSettingsDocument::new(serde_json::json!({
                "type": VALORANT_PLAYER_SETTINGS_TYPE,
                "data": {"roamingSetttingsVersion": 15}
            })),
        };
        repository.save(&profile).expect("save profile");
        fs::write(dir.path().join("corrupt.json"), "{ not json").expect("corrupt file");

        let profiles = repository.profile_metadata().expect("profile listing");

        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.id.as_str())
                .collect::<Vec<_>>(),
            [profile.id.as_str()]
        );
    }

    fn saved_profile(
        purpose: GameSettingsProfilePurpose,
        captured_at_unix: i64,
    ) -> GameSettingsProfile {
        GameSettingsProfile {
            id: format!(
                "{captured_at_unix}-{}",
                new_profile_id(AccountId::new(), purpose.clone())
            ),
            name: "Main".to_string(),
            purpose,
            source_account_id: AccountId::new(),
            source_display_name: "Main".to_string(),
            source_puuid: "puuid".to_string(),
            captured_at_unix,
            preference_base_url: "https://player-preferences-usw2.pp.sgp.pvp.net".to_string(),
            settings_version: Some(15),
            preference: ValorantSettingsDocument::new(serde_json::json!({
                "type": VALORANT_PLAYER_SETTINGS_TYPE,
                "data": {"roamingSetttingsVersion": 15}
            })),
        }
    }

    #[test]
    fn saved_listing_includes_backups_newest_first() {
        let dir = tempfile::tempdir().expect("profile dir");
        let repository = GameSettingsProfileRepository::new(dir.path());
        let profile = saved_profile(GameSettingsProfilePurpose::Profile, 100);
        let backup = saved_profile(GameSettingsProfilePurpose::Backup, 200);
        repository.save(&profile).expect("save profile");
        repository.save(&backup).expect("save backup");

        let saved = repository.saved_metadata().expect("saved listing");

        assert_eq!(
            saved
                .iter()
                .map(|saved| saved.id.as_str())
                .collect::<Vec<_>>(),
            [backup.id.as_str(), profile.id.as_str()]
        );
        assert_eq!(
            repository
                .profile_metadata()
                .expect("profile listing")
                .iter()
                .map(|saved| saved.id.as_str())
                .collect::<Vec<_>>(),
            [profile.id.as_str()]
        );
    }

    #[test]
    fn rename_changes_only_the_name() {
        let dir = tempfile::tempdir().expect("profile dir");
        let repository = GameSettingsProfileRepository::new(dir.path());
        let profile = saved_profile(GameSettingsProfilePurpose::Profile, 100);
        repository.save(&profile).expect("save profile");

        let renamed = repository
            .rename(&profile.id, "  Old crosshair  ")
            .expect("rename");

        assert_eq!(renamed.name, "Old crosshair");
        let loaded = repository.load(&profile.id).expect("load");
        assert_eq!(
            loaded,
            GameSettingsProfile {
                name: "Old crosshair".to_string(),
                ..profile
            }
        );
    }

    #[test]
    fn rename_refuses_an_empty_name() {
        let dir = tempfile::tempdir().expect("profile dir");
        let repository = GameSettingsProfileRepository::new(dir.path());
        let profile = saved_profile(GameSettingsProfilePurpose::Profile, 100);
        repository.save(&profile).expect("save profile");

        assert!(matches!(
            repository.rename(&profile.id, "   "),
            Err(GameSettingsError::EmptyName)
        ));
        assert_eq!(repository.load(&profile.id).expect("load").name, "Main");
    }

    #[test]
    fn original_settings_are_the_accounts_oldest_backup() {
        let dir = tempfile::tempdir().expect("profile dir");
        let repository = GameSettingsProfileRepository::new(dir.path());
        let account_id = AccountId::new();
        let for_account = |purpose, captured_at_unix| GameSettingsProfile {
            source_account_id: account_id,
            ..saved_profile(purpose, captured_at_unix)
        };
        let original = for_account(GameSettingsProfilePurpose::Backup, 100);
        let later_backup = for_account(GameSettingsProfilePurpose::Backup, 200);
        let preset = for_account(GameSettingsProfilePurpose::Profile, 50);
        let other_account = saved_profile(GameSettingsProfilePurpose::Backup, 10);
        for profile in [&original, &later_backup, &preset, &other_account] {
            repository.save(profile).expect("save");
        }

        assert_eq!(
            repository
                .original_settings(account_id)
                .expect("original")
                .map(|profile| profile.id),
            Some(original.id.clone())
        );
        assert_eq!(
            repository
                .backups_for(account_id)
                .expect("backups")
                .iter()
                .map(|profile| profile.id.as_str())
                .collect::<Vec<_>>(),
            [later_backup.id.as_str(), original.id.as_str()]
        );
        assert_eq!(
            repository
                .original_settings(AccountId::new())
                .expect("no original"),
            None
        );
    }

    #[test]
    fn delete_removes_a_saved_profile() {
        let dir = tempfile::tempdir().expect("profile dir");
        let repository = GameSettingsProfileRepository::new(dir.path());
        let profile = saved_profile(GameSettingsProfilePurpose::Profile, 100);
        let kept = saved_profile(GameSettingsProfilePurpose::Backup, 200);
        repository.save(&profile).expect("save profile");
        repository.save(&kept).expect("save backup");

        repository.delete(&profile.id).expect("delete");

        assert!(repository.load(&profile.id).is_err());
        assert_eq!(
            repository
                .saved_metadata()
                .expect("saved listing")
                .iter()
                .map(|saved| saved.id.as_str())
                .collect::<Vec<_>>(),
            [kept.id.as_str()]
        );
    }
}
