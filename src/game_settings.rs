use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;
use time::OffsetDateTime;

use crate::account::AccountId;

pub const VALORANT_PLAYER_SETTINGS_TYPE: &str = "Ares.PlayerSettings";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameSettingsSnapshotPurpose {
    Saved,
    Backup,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameSettingsSnapshot {
    pub id: String,
    pub purpose: GameSettingsSnapshotPurpose,
    pub source_account_id: AccountId,
    pub source_display_name: String,
    pub source_puuid: String,
    pub captured_at_unix: i64,
    pub preference_base_url: String,
    pub settings_version: Option<i64>,
    pub preference: ValorantSettingsDocument,
}

impl GameSettingsSnapshot {
    pub fn metadata(&self) -> GameSettingsSnapshotMetadata {
        GameSettingsSnapshotMetadata {
            id: self.id.clone(),
            purpose: self.purpose.clone(),
            source_account_id: self.source_account_id,
            source_display_name: self.source_display_name.clone(),
            source_puuid: self.source_puuid.clone(),
            captured_at_unix: self.captured_at_unix,
            settings_version: self.settings_version,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameSettingsSnapshotMetadata {
    pub id: String,
    pub purpose: GameSettingsSnapshotPurpose,
    pub source_account_id: AccountId,
    pub source_display_name: String,
    pub source_puuid: String,
    pub captured_at_unix: i64,
    pub settings_version: Option<i64>,
}

impl fmt::Display for GameSettingsSnapshotMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({})",
            self.source_display_name, self.captured_at_unix
        )
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

    pub fn save_body(&self, preference_type: &str) -> Value {
        match &self.raw {
            Value::Object(map) if has_data_field(map) => self.raw.clone(),
            _ => serde_json::json!({
                "type": preference_type,
                "data": self.raw,
            }),
        }
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
pub struct GameSettingsSnapshotRepository {
    dir: PathBuf,
}

impl GameSettingsSnapshotRepository {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn save(&self, snapshot: &GameSettingsSnapshot) -> Result<PathBuf, GameSettingsError> {
        fs::create_dir_all(&self.dir)?;

        let path = self.snapshot_path(&snapshot.id);
        let tmp = path.with_extension("json.tmp");
        let contents = serde_json::to_vec_pretty(snapshot)?;

        write_synced(&tmp, &contents)?;
        fs::rename(&tmp, &path)?;

        Ok(path)
    }

    pub fn load(&self, id: &str) -> Result<GameSettingsSnapshot, GameSettingsError> {
        let contents = fs::read_to_string(self.snapshot_path(id))?;
        serde_json::from_str(&contents).map_err(GameSettingsError::Json)
    }

    pub fn latest_saved(&self) -> Result<GameSettingsSnapshot, GameSettingsError> {
        self.saved_metadata()?
            .into_iter()
            .max_by_key(|snapshot| snapshot.captured_at_unix)
            .ok_or(GameSettingsError::NoSavedSnapshot)
            .and_then(|metadata| self.load(&metadata.id))
    }

    pub fn saved_metadata(&self) -> Result<Vec<GameSettingsSnapshotMetadata>, GameSettingsError> {
        let mut snapshots = self.metadata()?;
        snapshots.retain(|snapshot| snapshot.purpose == GameSettingsSnapshotPurpose::Saved);
        snapshots.sort_by_key(|snapshot| Reverse(snapshot.captured_at_unix));
        Ok(snapshots)
    }

    fn metadata(&self) -> Result<Vec<GameSettingsSnapshotMetadata>, GameSettingsError> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }

        let mut snapshots = Vec::new();

        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }

            let contents = fs::read_to_string(path)?;
            let snapshot: GameSettingsSnapshot =
                serde_json::from_str(&contents).map_err(GameSettingsError::Json)?;
            snapshots.push(snapshot.metadata());
        }

        Ok(snapshots)
    }

    fn snapshot_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{}.json", safe_snapshot_id(id)))
    }
}

pub fn new_snapshot_id(account_id: AccountId, purpose: GameSettingsSnapshotPurpose) -> String {
    let purpose = match purpose {
        GameSettingsSnapshotPurpose::Saved => "saved",
        GameSettingsSnapshotPurpose::Backup => "backup",
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
        Value::String(data) => {
            let value: Value =
                serde_json::from_str(data).map_err(GameSettingsError::SettingsJson)?;
            serde_json::from_value(value).map_err(GameSettingsError::SettingsJson)
        }
        _ => Err(GameSettingsError::InvalidSettingsPayload),
    }
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
                    Value::String(
                        serde_json::to_string(&encoded).map_err(GameSettingsError::SettingsJson)?,
                    ),
                );
            } else {
                map.insert("data".to_string(), encoded);
            }
        }
        Value::Object(map) if map.get("Data").is_some() => {
            if map.get("Data").is_some_and(Value::is_string) {
                map.insert(
                    "Data".to_string(),
                    Value::String(
                        serde_json::to_string(&encoded).map_err(GameSettingsError::SettingsJson)?,
                    ),
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

fn has_data_field(map: &Map<String, Value>) -> bool {
    map.contains_key("data") || map.contains_key("Data")
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

fn safe_snapshot_id(value: &str) -> String {
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
    #[error("settings payload version is unsupported")]
    SettingsJson(serde_json::Error),
    #[error("settings snapshot JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("settings snapshot I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("no saved settings snapshot is available")]
    NoSavedSnapshot,
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
