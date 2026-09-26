use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

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
            .map(|payload| payload.summary())?;

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameSettingsProfileMetadata {
    pub id: String,
    pub name: String,
    pub purpose: GameSettingsProfilePurpose,
    pub source_account_id: AccountId,
    pub source_display_name: String,
    pub source_puuid: String,
    pub captured_at_unix: i64,
    pub settings_version: Option<i64>,
    pub summary: GameSettingsProfileSummary,
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
        GameSettingsProfileSummary {
            sensitivity_count: count_named_settings(&self.float_settings, |key| {
                key.contains("sensitivity")
            }) + count_named_settings(&self.int_settings, |key| {
                key.contains("sensitivity")
            }) + count_named_settings(&self.bool_settings, |key| {
                key.contains("sensitivity")
            }) + count_named_settings(&self.string_settings, |key| {
                key.contains("sensitivity")
            }),
            crosshair_count: count_named_settings(&self.float_settings, |key| {
                key.contains("crosshair")
            }) + count_named_settings(&self.int_settings, |key| {
                key.contains("crosshair")
            }) + count_named_settings(&self.bool_settings, |key| {
                key.contains("crosshair")
            }) + count_named_settings(&self.string_settings, |key| {
                key.contains("crosshair")
            }) + self.settings_profile_data.as_ref().map_or(0, Vec::len),
            keybind_count: self.action_mappings.as_ref().map_or(0, Vec::len)
                + self.axis_mappings.as_ref().map_or(0, Vec::len),
            minimap_count: count_named_settings(&self.float_settings, |key| {
                key.contains("minimap")
            }) + count_named_settings(&self.int_settings, |key| {
                key.contains("minimap")
            }) + count_named_settings(&self.bool_settings, |key| {
                key.contains("minimap")
            }) + count_named_settings(&self.string_settings, |key| {
                key.contains("minimap")
            }),
            gameplay_interface_count: count_gameplay_interface_settings(self),
            examples: setting_examples(self, 6),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameSettingsProfileSummary {
    pub sensitivity_count: usize,
    pub crosshair_count: usize,
    pub keybind_count: usize,
    pub minimap_count: usize,
    pub gameplay_interface_count: usize,
    pub examples: Vec<String>,
}

impl GameSettingsProfileSummary {
    pub fn categories_label(&self) -> String {
        let mut parts = Vec::new();

        if self.sensitivity_count > 0 {
            parts.push(format!("Sensitivity {}", self.sensitivity_count));
        }
        if self.crosshair_count > 0 {
            parts.push(format!("Crosshair {}", self.crosshair_count));
        }
        if self.keybind_count > 0 {
            parts.push(format!("Keybinds {}", self.keybind_count));
        }
        if self.minimap_count > 0 {
            parts.push(format!("Minimap {}", self.minimap_count));
        }
        if self.gameplay_interface_count > 0 {
            parts.push(format!("Gameplay {}", self.gameplay_interface_count));
        }

        if parts.is_empty() {
            "No recognized settings".to_string()
        } else {
            parts.join(" | ")
        }
    }

    pub fn examples_label(&self) -> String {
        if self.examples.is_empty() {
            "No recognized setting names".to_string()
        } else {
            self.examples.join(", ")
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsCategories {
    pub sensitivity: bool,
    pub crosshair: bool,
    pub keybinds: bool,
    pub minimap: bool,
    pub gameplay_interface: bool,
}

impl SettingsCategories {
    pub fn all_gameplay() -> Self {
        Self {
            sensitivity: true,
            crosshair: true,
            keybinds: true,
            minimap: true,
            gameplay_interface: true,
        }
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

        write_synced(&tmp, &contents)?;
        fs::rename(&tmp, &path)?;

        Ok(path)
    }

    pub fn load(&self, id: &str) -> Result<GameSettingsProfile, GameSettingsError> {
        let contents = fs::read_to_string(self.profile_path(id))?;
        serde_json::from_str(&contents).map_err(GameSettingsError::Json)
    }

    pub fn latest_profile(&self) -> Result<GameSettingsProfile, GameSettingsError> {
        self.profile_metadata()?
            .into_iter()
            .max_by_key(|profile| profile.captured_at_unix)
            .ok_or(GameSettingsError::NoSavedProfile)
            .and_then(|metadata| self.load(&metadata.id))
    }

    pub fn profile_metadata(&self) -> Result<Vec<GameSettingsProfileMetadata>, GameSettingsError> {
        let mut profiles = self.metadata()?;
        profiles.retain(|profile| profile.purpose == GameSettingsProfilePurpose::Profile);
        profiles.sort_by_key(|profile| Reverse(profile.captured_at_unix));
        Ok(profiles)
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

            let contents = fs::read_to_string(path)?;
            let profile: GameSettingsProfile =
                serde_json::from_str(&contents).map_err(GameSettingsError::Json)?;
            profiles.push(profile.metadata()?);
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

pub fn merge_settings_payload(
    source: &ValorantSettingsPayload,
    target: &mut ValorantSettingsPayload,
    categories: SettingsCategories,
) {
    merge_named_settings(
        &source.float_settings,
        &mut target.float_settings,
        categories,
    );
    merge_named_settings(&source.int_settings, &mut target.int_settings, categories);
    merge_named_settings(&source.bool_settings, &mut target.bool_settings, categories);
    merge_named_settings(
        &source.string_settings,
        &mut target.string_settings,
        categories,
    );

    if categories.keybinds {
        merge_whole_array(&source.action_mappings, &mut target.action_mappings);
        merge_whole_array(&source.axis_mappings, &mut target.axis_mappings);
    }

    if categories.gameplay_interface {
        merge_whole_array(&source.settings_profiles, &mut target.settings_profiles);
        merge_whole_array(
            &source.settings_profile_data,
            &mut target.settings_profile_data,
        );
    }
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

fn count_named_settings(settings: &Option<Vec<Value>>, matches: impl Fn(&str) -> bool) -> usize {
    settings
        .as_ref()
        .map(|settings| {
            settings
                .iter()
                .filter_map(setting_entry_key)
                .map(str::to_ascii_lowercase)
                .filter(|key| matches(key))
                .count()
        })
        .unwrap_or(0)
}

fn count_gameplay_interface_settings(payload: &ValorantSettingsPayload) -> usize {
    let named_count = [
        &payload.float_settings,
        &payload.int_settings,
        &payload.bool_settings,
        &payload.string_settings,
    ]
    .into_iter()
    .map(|settings| {
        settings
            .as_ref()
            .map(|settings| {
                settings
                    .iter()
                    .filter_map(setting_entry_key)
                    .filter(|key| {
                        let key = key.to_ascii_lowercase();
                        setting_key_matches_categories(&key, SettingsCategories::all_gameplay())
                            && !key.contains("sensitivity")
                            && !key.contains("crosshair")
                            && !key.contains("minimap")
                    })
                    .count()
            })
            .unwrap_or(0)
    })
    .sum::<usize>();

    named_count + payload.settings_profiles.as_ref().map_or(0, Vec::len)
}

fn setting_examples(payload: &ValorantSettingsPayload, limit: usize) -> Vec<String> {
    let mut examples = Vec::new();
    let mut seen = HashSet::new();

    for settings in [
        &payload.float_settings,
        &payload.int_settings,
        &payload.bool_settings,
        &payload.string_settings,
        &payload.action_mappings,
        &payload.axis_mappings,
    ] {
        let Some(settings) = settings else {
            continue;
        };

        for key in settings.iter().filter_map(setting_entry_key) {
            if examples.len() >= limit {
                return examples;
            }

            if seen.insert(key.to_string()) {
                examples.push(key.to_string());
            }
        }
    }

    examples
}

fn merge_named_settings(
    source: &Option<Vec<Value>>,
    target: &mut Option<Vec<Value>>,
    categories: SettingsCategories,
) {
    let Some(source) = source else {
        return;
    };

    let source_entries = source
        .iter()
        .filter_map(|entry| {
            let key = setting_entry_key(entry)?;
            setting_key_matches_categories(key, categories).then_some((key.to_string(), entry))
        })
        .collect::<Vec<_>>();

    if source_entries.is_empty() {
        return;
    }

    let replacements = source_entries
        .iter()
        .map(|(key, entry)| (key.clone(), (*entry).clone()))
        .collect::<HashMap<_, _>>();
    let mut consumed = HashSet::new();
    let target_entries = target.get_or_insert_with(Vec::new);

    for entry in target_entries.iter_mut() {
        if let Some(key) = setting_entry_key(entry).map(str::to_string)
            && let Some(replacement) = replacements.get(&key)
        {
            *entry = replacement.clone();
            consumed.insert(key);
        }
    }

    for (key, entry) in source_entries {
        if !consumed.contains(&key) {
            target_entries.push(entry.clone());
        }
    }
}

fn merge_whole_array(source: &Option<Vec<Value>>, target: &mut Option<Vec<Value>>) {
    if let Some(source) = source {
        *target = Some(source.clone());
    }
}

fn setting_entry_key(entry: &Value) -> Option<&str> {
    let Value::Object(map) = entry else {
        return None;
    };

    [
        "settingEnum",
        "SettingEnum",
        "settingName",
        "SettingName",
        "name",
        "Name",
        "key",
        "Key",
        "actionName",
        "ActionName",
        "axisName",
        "AxisName",
    ]
    .into_iter()
    .find_map(|field| map.get(field)?.as_str())
    .filter(|key| !key.trim().is_empty())
}

fn setting_key_matches_categories(key: &str, categories: SettingsCategories) -> bool {
    let key = key.to_ascii_lowercase();

    if excluded_setting_key(&key) {
        return false;
    }

    if categories.sensitivity && key.contains("sensitivity") {
        return true;
    }

    if categories.crosshair && key.contains("crosshair") {
        return true;
    }

    if categories.minimap && key.contains("minimap") {
        return true;
    }

    categories.gameplay_interface
}

fn excluded_setting_key(key: &str) -> bool {
    const EXCLUDED_PARTS: &[&str] = &[
        "eula",
        "legal",
        "seen",
        "firsttime",
        "first_time",
        "onboarding",
        "tutorial",
        "audio",
        "volume",
        "sound",
        "voice",
        "microphone",
        "speaker",
        "device",
    ];

    EXCLUDED_PARTS.iter().any(|part| key.contains(part))
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

fn write_synced(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(contents)?;
    file.sync_all()
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
    #[error("no settings profile is available")]
    NoSavedProfile,
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

        let message = document.settings_payload().expect_err("invalid").to_string();

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

    #[test]
    fn summarizes_profile_categories_and_examples() {
        let payload = payload(serde_json::json!({
            "floatSettings": [{"settingEnum": "MouseSensitivity", "value": 0.32}],
            "boolSettings": [{"settingEnum": "MinimapRotates", "value": true}],
            "stringSettings": [{"settingEnum": "SavedCrosshairProfileData", "value": "crosshair"}],
            "actionMappings": [{"actionName": "Jump", "key": "Space"}],
            "settingsProfiles": [{"profile": "default"}]
        }));

        let summary = payload.summary();

        assert_eq!(summary.sensitivity_count, 1);
        assert_eq!(summary.crosshair_count, 1);
        assert_eq!(summary.keybind_count, 1);
        assert_eq!(summary.minimap_count, 1);
        assert_eq!(summary.gameplay_interface_count, 1);
        assert!(summary.examples.contains(&"MouseSensitivity".to_string()));
    }

    #[test]
    fn merge_replaces_selected_categories_and_preserves_excluded_target_settings() {
        let source = payload(serde_json::json!({
            "floatSettings": [
                {"settingEnum": "MouseSensitivity", "value": 0.32},
                {"settingEnum": "MasterVolume", "value": 1.0}
            ],
            "boolSettings": [
                {"settingEnum": "MinimapRotates", "value": true},
                {"settingEnum": "HasSeenIntro", "value": true}
            ],
            "stringSettings": [
                {"settingEnum": "SavedCrosshairProfileData", "value": "source-crosshair"}
            ],
            "actionMappings": [{"actionName": "Jump", "key": "Space"}]
        }));
        let mut target = payload(serde_json::json!({
            "floatSettings": [
                {"settingEnum": "MasterVolume", "value": 0.2},
                {"settingEnum": "MouseSensitivity", "value": 0.8}
            ],
            "boolSettings": [
                {"settingEnum": "HasSeenIntro", "value": false},
                {"settingEnum": "MinimapRotates", "value": false}
            ],
            "stringSettings": [
                {"settingEnum": "SavedCrosshairProfileData", "value": "target-crosshair"}
            ],
            "actionMappings": [{"actionName": "Jump", "key": "WheelDown"}],
            "unknownTargetField": true
        }));

        merge_settings_payload(&source, &mut target, SettingsCategories::all_gameplay());

        assert_eq!(
            target.float_settings.as_ref().expect("float")[0]["value"],
            serde_json::json!(0.2)
        );
        assert_eq!(
            target.float_settings.as_ref().expect("float")[1]["value"],
            serde_json::json!(0.32)
        );
        assert_eq!(
            target.bool_settings.as_ref().expect("bool")[0]["value"],
            serde_json::json!(false)
        );
        assert_eq!(
            target.bool_settings.as_ref().expect("bool")[1]["value"],
            serde_json::json!(true)
        );
        assert_eq!(
            target.string_settings.as_ref().expect("string")[0]["value"],
            serde_json::json!("source-crosshair")
        );
        assert_eq!(
            target.action_mappings.as_ref().expect("actions")[0]["key"],
            serde_json::json!("Space")
        );
        assert_eq!(target.extra["unknownTargetField"], serde_json::json!(true));
    }
}
