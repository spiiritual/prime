//! What a saved settings profile holds, read from its VALORANT settings for display.
//!
//! VALORANT only stores settings that differ from the game's defaults, so a missing value means
//! the default.

use serde_json::Value;

use super::ValorantSettingsPayload;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GameSettingsProfileSummary {
    pub sensitivity: Option<f64>,
    /// The aim-down-sights multiplier.
    pub ads_multiplier: Option<f64>,
    /// The scoped multiplier.
    pub scoped_multiplier: Option<f64>,
    /// The crosshair in use, or `None` for the default one.
    pub crosshair: Option<CrosshairSummary>,
    pub crosshair_profile_count: usize,
    /// Every changed keybind.
    pub keybinds: Vec<Keybind>,
    pub minimap: Vec<String>,
    /// Audio and voice settings.
    pub audio_settings: Vec<Setting>,
    /// Everything else the rows above don't describe.
    pub other_settings: Vec<Setting>,
}

/// One setting as the expanded preset card lists it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Setting {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CrosshairSummary {
    pub name: Option<String>,
    pub color: Rgba,
    pub outline: Option<CrosshairOutline>,
    pub center_dot: Option<CrosshairCenterDot>,
    pub inner_lines: Option<CrosshairLines>,
    pub outer_lines: Option<CrosshairLines>,
}

impl CrosshairSummary {
    /// VALORANT's default crosshair, for presets that never changed it.
    pub fn default_crosshair() -> Self {
        Self {
            name: None,
            color: Rgba::WHITE,
            outline: Some(CrosshairOutline {
                thickness: 1.0,
                opacity: 0.5,
            }),
            center_dot: None,
            inner_lines: Some(CrosshairLines {
                length: 6.0,
                vertical_length: 6.0,
                thickness: 2.0,
                offset: 3.0,
                opacity: 0.8,
            }),
            outer_lines: Some(CrosshairLines {
                length: 2.0,
                vertical_length: 2.0,
                thickness: 2.0,
                offset: 10.0,
                opacity: 0.35,
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrosshairOutline {
    pub thickness: f32,
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrosshairCenterDot {
    pub size: f32,
    pub opacity: f32,
}

/// One set of crosshair lines, in VALORANT's pixel units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrosshairLines {
    pub length: f32,
    pub vertical_length: f32,
    pub thickness: f32,
    pub offset: f32,
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// The colour's name in VALORANT's picker, or else its hex code.
    pub fn label(self) -> String {
        const NAMED: [(Rgba, &str); 8] = [
            (Rgba::rgb(255, 255, 255), "White"),
            (Rgba::rgb(0, 255, 0), "Green"),
            (Rgba::rgb(127, 255, 0), "Yellow green"),
            (Rgba::rgb(223, 255, 0), "Green yellow"),
            (Rgba::rgb(255, 255, 0), "Yellow"),
            (Rgba::rgb(0, 255, 255), "Cyan"),
            (Rgba::rgb(255, 0, 255), "Pink"),
            (Rgba::rgb(255, 0, 0), "Red"),
        ];

        NAMED
            .iter()
            .find(|(color, _)| (color.r, color.g, color.b) == (self.r, self.g, self.b))
            .map(|(_, name)| (*name).to_string())
            .unwrap_or_else(|| format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b))
    }

    fn from_json(value: &Value) -> Option<Self> {
        let channel = |name: &str| {
            value
                .get(name)
                .and_then(Value::as_u64)
                .and_then(|channel| u8::try_from(channel).ok())
        };

        Some(Self {
            r: channel("r")?,
            g: channel("g")?,
            b: channel("b")?,
            a: channel("a").unwrap_or(255),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Keybind {
    pub action: String,
    pub key: String,
}

pub(super) fn summarize(payload: &ValorantSettingsPayload) -> GameSettingsProfileSummary {
    let (crosshair, crosshair_profile_count) = saved_crosshair(payload);
    let (audio_settings, other_settings) = listed_settings(payload);

    GameSettingsProfileSummary {
        sensitivity: number_setting(payload, "MouseSensitivity"),
        ads_multiplier: number_setting(payload, "MouseSensitivityADS"),
        scoped_multiplier: number_setting(payload, "MouseSensitivityZoomed"),
        crosshair,
        crosshair_profile_count,
        keybinds: keybinds(payload),
        minimap: minimap(payload),
        audio_settings,
        other_settings,
    }
}

/// Every named setting, from the float, int, bool and string lists.
fn named_settings(payload: &ValorantSettingsPayload) -> impl Iterator<Item = (&str, &Value)> {
    [
        &payload.float_settings,
        &payload.int_settings,
        &payload.bool_settings,
        &payload.string_settings,
    ]
    .into_iter()
    .flatten()
    .flatten()
    .filter_map(|entry| {
        Some((
            setting_name(entry.get("settingEnum")?.as_str()?),
            entry.get("value")?,
        ))
    })
}

/// `EAresFloatSettingName::MouseSensitivity` becomes `MouseSensitivity`.
fn setting_name(key: &str) -> &str {
    key.rsplit("::").next().unwrap_or(key)
}

fn setting<'a>(payload: &'a ValorantSettingsPayload, name: &str) -> Option<&'a Value> {
    named_settings(payload)
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
}

fn number_setting(payload: &ValorantSettingsPayload, name: &str) -> Option<f64> {
    // Stored as 32-bit floats, so 0.34 comes back as 0.3400000035762787.
    setting(payload, name)?
        .as_f64()
        .map(|value| (value * 1000.0).round() / 1000.0)
}

fn bool_setting(payload: &ValorantSettingsPayload, name: &str) -> Option<bool> {
    setting(payload, name)?.as_bool()
}

/// The crosshair profile in use and how many are saved, from `SavedCrosshairProfileData`.
fn saved_crosshair(payload: &ValorantSettingsPayload) -> (Option<CrosshairSummary>, usize) {
    let Some(data) = setting(payload, "SavedCrosshairProfileData")
        .and_then(Value::as_str)
        .and_then(|data| serde_json::from_str::<Value>(data).ok())
    else {
        return (None, 0);
    };
    let profiles = data
        .get("profiles")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let current = data
        .get("currentProfile")
        .and_then(Value::as_u64)
        .and_then(|index| profiles.get(usize::try_from(index).ok()?));

    (current.and_then(crosshair_profile), profiles.len())
}

fn crosshair_profile(profile: &Value) -> Option<CrosshairSummary> {
    let primary = profile.get("primary")?;
    let flag = |value: &Value, name: &str| value.get(name).and_then(Value::as_bool);
    let number = |value: &Value, name: &str| {
        value
            .get(name)
            .and_then(Value::as_f64)
            .map(|number| number as f32)
    };
    let color_field = if flag(primary, "bUseCustomColor") == Some(true) {
        "colorCustom"
    } else {
        "color"
    };
    let lines = |name: &str| {
        let lines = primary.get(name)?;
        if flag(lines, "bShowLines") == Some(false) {
            return None;
        }

        let length = number(lines, "lineLength")?;
        Some(CrosshairLines {
            length,
            vertical_length: if flag(lines, "bAllowVertScaling") == Some(true) {
                number(lines, "lineLengthVertical").unwrap_or(length)
            } else {
                length
            },
            thickness: number(lines, "lineThickness")?,
            offset: number(lines, "lineOffset").unwrap_or(0.0),
            opacity: number(lines, "opacity").unwrap_or(1.0),
        })
    };

    Some(CrosshairSummary {
        name: profile
            .get("profileName")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string),
        color: primary
            .get(color_field)
            .and_then(Rgba::from_json)
            .unwrap_or(Rgba::WHITE),
        outline: (flag(primary, "bHasOutline") == Some(true))
            .then(|| CrosshairOutline {
                thickness: number(primary, "outlineThickness").unwrap_or(1.0),
                opacity: number(primary, "outlineOpacity").unwrap_or(0.5),
            })
            .filter(|outline| outline.opacity > 0.0),
        center_dot: (flag(primary, "bDisplayCenterDot") == Some(true)).then(|| {
            CrosshairCenterDot {
                size: number(primary, "centerDotSize").unwrap_or(2.0),
                opacity: number(primary, "centerDotOpacity").unwrap_or(1.0),
            }
        }),
        inner_lines: lines("innerLines"),
        outer_lines: lines("outerLines"),
    })
}

fn keybinds(payload: &ValorantSettingsPayload) -> Vec<Keybind> {
    payload
        .action_mappings
        .iter()
        .flatten()
        .filter_map(|mapping| {
            let action = mapping.get("name")?.as_str()?;
            let key = mapping.get("key")?.as_str()?;
            let modifiers = ["ctrl", "shift", "alt", "cmd"]
                .into_iter()
                .filter(|modifier| mapping.get(*modifier).and_then(Value::as_bool) == Some(true))
                .map(|modifier| match modifier {
                    "ctrl" => "Ctrl",
                    "shift" => "Shift",
                    "alt" => "Alt",
                    _ => "Cmd",
                });
            let key = modifiers
                .chain(std::iter::once(key_label(key).as_str()))
                .collect::<Vec<_>>()
                .join(" + ");

            Some(Keybind {
                action: action_label(action),
                key,
            })
        })
        .collect()
}

/// Riot's input name for a key, as the settings screen shows it.
fn key_label(key: &str) -> String {
    let label = match key {
        "None" | "" => "Unbound",
        "LeftMouseButton" => "Mouse 1",
        "RightMouseButton" => "Mouse 2",
        "MiddleMouseButton" => "Mouse 3",
        "ThumbMouseButton" => "Mouse 4",
        "ThumbMouseButton2" => "Mouse 5",
        "MouseScrollUp" => "Scroll up",
        "MouseScrollDown" => "Scroll down",
        "SpaceBar" => "Space",
        "LeftShift" => "Left Shift",
        "RightShift" => "Right Shift",
        "LeftControl" => "Left Ctrl",
        "RightControl" => "Right Ctrl",
        "LeftAlt" => "Left Alt",
        "RightAlt" => "Right Alt",
        "Zero" => "0",
        "One" => "1",
        "Two" => "2",
        "Three" => "3",
        "Four" => "4",
        "Five" => "5",
        "Six" => "6",
        "Seven" => "7",
        "Eight" => "8",
        "Nine" => "9",
        _ => return split_words(key),
    };

    label.to_string()
}

/// Riot's action name, as the settings screen shows it.
fn action_label(action: &str) -> String {
    let label = match action {
        "Activate_GrenadeAbility" => "Ability (C)",
        "Activate_Ability1" => "Ability (Q)",
        "Activate_Ability2" => "Ability (E)",
        "Activate_Ultimate" => "Ultimate (X)",
        "ToggleMouseCursor" => "Toggle mouse cursor",
        _ => {
            let words = split_words(action.trim_start_matches("Activate_"));
            let mut chars = words.chars();
            return match chars.next() {
                Some(first) => first
                    .to_uppercase()
                    .chain(chars.flat_map(char::to_lowercase))
                    .collect(),
                None => words,
            };
        }
    };

    label.to_string()
}

/// `EquipPrimaryWeapon` and `Equip_Primary` both become `Equip Primary Weapon`-style words.
fn split_words(name: &str) -> String {
    let mut words = String::new();
    let mut previous: Option<char> = None;

    for ch in name.chars() {
        if ch == '_' {
            words.push(' ');
        } else {
            if ch.is_ascii_uppercase()
                && previous.is_some_and(|previous| previous.is_ascii_lowercase())
            {
                words.push(' ');
            }
            words.push(ch);
        }
        previous = Some(ch);
    }

    words.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn minimap(payload: &ValorantSettingsPayload) -> Vec<String> {
    let mut minimap = Vec::new();

    match (
        bool_setting(payload, "MinimapRotates"),
        bool_setting(payload, "MinimapFixedRotation"),
    ) {
        (Some(true), _) => minimap.push("Rotates".to_string()),
        (Some(false), _) | (None, Some(true)) => minimap.push("Fixed rotation".to_string()),
        _ => {}
    }

    match bool_setting(payload, "MinimapTranslates") {
        Some(true) => minimap.push("Keeps player centered".to_string()),
        Some(false) => minimap.push("Player not centered".to_string()),
        None => {}
    }

    if let Some(size) = number_setting(payload, "MinimapSize") {
        minimap.push(format!("Size {size}"));
    }
    if let Some(zoom) = number_setting(payload, "MinimapZoom") {
        minimap.push(format!("Zoom {zoom}"));
    }

    minimap
}

/// The settings the summary's own rows don't describe, as audio and everything else, each sorted
/// by label. Tips and pop-ups the player has already seen are copied too, but aren't listed.
fn listed_settings(payload: &ValorantSettingsPayload) -> (Vec<Setting>, Vec<Setting>) {
    let mut audio = Vec::new();
    let mut other = Vec::new();

    for (entries, from_float_list) in [
        (&payload.float_settings, true),
        (&payload.int_settings, false),
        (&payload.bool_settings, false),
        (&payload.string_settings, false),
    ] {
        for entry in entries.iter().flatten() {
            let (Some(name), Some(value)) = (
                entry
                    .get("settingEnum")
                    .and_then(Value::as_str)
                    .map(setting_name),
                entry.get("value"),
            ) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if described_by_summary(&lower) || already_seen(name) {
                continue;
            }

            let setting = Setting {
                label: setting_label(name),
                value: setting_value(&lower, value, from_float_list),
            };
            if is_audio(&lower) {
                audio.push(setting);
            } else {
                other.push(setting);
            }
        }
    }

    audio.sort_by(|a, b| a.label.cmp(&b.label));
    other.sort_by(|a, b| a.label.cmp(&b.label));
    (audio, other)
}

fn described_by_summary(lower_name: &str) -> bool {
    ["sensitivity", "crosshair", "minimap"]
        .iter()
        .any(|part| lower_name.contains(part))
}

fn already_seen(name: &str) -> bool {
    [
        "HasSeen",
        "HasEver",
        "HasAccepted",
        "LastSeen",
        "LastAccepted",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
        || name.ends_with("ModuleComplete")
}

fn is_audio(lower_name: &str) -> bool {
    [
        "volume",
        "voice",
        "music",
        "sound",
        "audio",
        "hrtf",
        "pushtotalk",
        "microphone",
    ]
    .iter()
    .any(|part| lower_name.contains(part))
}

/// `TeamPushToTalkKey` becomes `Team push to talk key`; acronyms like `HRTF` stay capitalised.
fn setting_label(name: &str) -> String {
    split_words(name)
        .split(' ')
        .enumerate()
        .map(|(index, word)| {
            let acronym = word.len() > 1 && word.chars().all(|ch| ch.is_ascii_uppercase());
            if index == 0 || acronym {
                word.to_string()
            } else {
                word.to_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn setting_value(lower_name: &str, value: &Value, from_float_list: bool) -> String {
    match value {
        Value::Bool(true) => "On".to_string(),
        Value::Bool(false) => "Off".to_string(),
        Value::Number(number) => {
            let number = number.as_f64().unwrap_or_default();
            if lower_name.contains("volume") {
                // Float volumes run from 0 to 1; integer ones are already percentages.
                let percent = if from_float_list {
                    number * 100.0
                } else {
                    number
                };
                format!("{}%", percent.round())
            } else {
                // Stored as 32-bit floats, so 0.34 comes back as 0.3400000035762787.
                ((number * 1000.0).round() / 1000.0).to_string()
            }
        }
        Value::String(text) if lower_name.ends_with("key") => key_label(text),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(value: Value) -> ValorantSettingsPayload {
        serde_json::from_value(value).expect("payload")
    }

    fn crosshair_data(current: usize) -> String {
        serde_json::json!({
            "currentProfile": current,
            "profiles": [
                {"profileName": "Dot", "primary": {"color": {"r": 255, "g": 255, "b": 255, "a": 255}}},
                {
                    "profileName": "Main",
                    "primary": {
                        "color": {"r": 0, "g": 255, "b": 255, "a": 255},
                        "colorCustom": {"r": 138, "g": 206, "b": 0, "a": 255},
                        "bUseCustomColor": false,
                        "bHasOutline": true,
                        "outlineThickness": 1,
                        "outlineOpacity": 0.5,
                        "bDisplayCenterDot": false,
                        "innerLines": {
                            "lineThickness": 2, "lineLength": 4, "lineLengthVertical": 3,
                            "bAllowVertScaling": true, "lineOffset": 1, "opacity": 1,
                            "bShowLines": true
                        },
                        "outerLines": {
                            "lineThickness": 2, "lineLength": 2, "lineOffset": 10,
                            "opacity": 0.35, "bShowLines": false
                        }
                    }
                }
            ]
        })
        .to_string()
    }

    #[test]
    fn reads_sensitivity_rounded_from_32_bit_floats() {
        let summary = summarize(&payload(serde_json::json!({
            "floatSettings": [
                {"settingEnum": "EAresFloatSettingName::MouseSensitivity", "value": 0.3400000035762787},
                {"settingEnum": "EAresFloatSettingName::MouseSensitivityADS", "value": 0.6000000238418579},
                {"settingEnum": "EAresFloatSettingName::MouseSensitivityZoomed", "value": 0.800000011920929}
            ]
        })));

        assert_eq!(summary.sensitivity, Some(0.34));
        assert_eq!(summary.ads_multiplier, Some(0.6));
        assert_eq!(summary.scoped_multiplier, Some(0.8));
    }

    #[test]
    fn missing_settings_mean_the_defaults() {
        let summary = summarize(&payload(serde_json::json!({})));

        assert_eq!(summary.sensitivity, None);
        assert_eq!(summary.crosshair, None);
        assert_eq!(summary.crosshair_profile_count, 0);
        assert!(summary.keybinds.is_empty());
        assert!(summary.minimap.is_empty());
        assert!(summary.audio_settings.is_empty());
        assert!(summary.other_settings.is_empty());
    }

    #[test]
    fn reads_the_crosshair_in_use() {
        let summary = summarize(&payload(serde_json::json!({
            "stringSettings": [
                {"settingEnum": "EAresStringSettingName::SavedCrosshairProfileData", "value": crosshair_data(1)}
            ]
        })));

        assert_eq!(summary.crosshair_profile_count, 2);
        assert_eq!(
            summary.crosshair,
            Some(CrosshairSummary {
                name: Some("Main".to_string()),
                color: Rgba::rgb(0, 255, 255),
                outline: Some(CrosshairOutline {
                    thickness: 1.0,
                    opacity: 0.5,
                }),
                center_dot: None,
                inner_lines: Some(CrosshairLines {
                    length: 4.0,
                    vertical_length: 3.0,
                    thickness: 2.0,
                    offset: 1.0,
                    opacity: 1.0,
                }),
                outer_lines: None,
            })
        );
    }

    #[test]
    fn a_custom_crosshair_colour_is_used_when_turned_on() {
        let data =
            crosshair_data(1).replace("\"bUseCustomColor\":false", "\"bUseCustomColor\":true");
        let summary = summarize(&payload(serde_json::json!({
            "stringSettings": [
                {"settingEnum": "EAresStringSettingName::SavedCrosshairProfileData", "value": data}
            ]
        })));

        assert_eq!(
            summary.crosshair.expect("crosshair").color,
            Rgba::rgb(138, 206, 0)
        );
    }

    #[test]
    fn names_valorants_crosshair_colours() {
        assert_eq!(Rgba::rgb(0, 255, 255).label(), "Cyan");
        assert_eq!(Rgba::rgb(138, 206, 0).label(), "#8ACE00");
    }

    #[test]
    fn keybinds_read_like_the_settings_screen() {
        let summary = summarize(&payload(serde_json::json!({
            "actionMappings": [
                {"name": "Activate_GrenadeAbility", "key": "ThumbMouseButton", "ctrl": false, "shift": false, "alt": false},
                {"name": "Ping", "key": "MiddleMouseButton"},
                {"name": "EquipPrimaryWeapon", "key": "One", "shift": true},
                {"name": "ToggleMouseCursor", "key": "None"}
            ]
        })));

        assert_eq!(
            summary.keybinds,
            [
                Keybind {
                    action: "Ability (C)".to_string(),
                    key: "Mouse 4".to_string(),
                },
                Keybind {
                    action: "Ping".to_string(),
                    key: "Mouse 3".to_string(),
                },
                Keybind {
                    action: "Equip primary weapon".to_string(),
                    key: "Shift + 1".to_string(),
                },
                Keybind {
                    action: "Toggle mouse cursor".to_string(),
                    key: "Unbound".to_string(),
                },
            ]
        );
    }

    #[test]
    fn reads_the_minimap_options() {
        let summary = summarize(&payload(serde_json::json!({
            "boolSettings": [
                {"settingEnum": "EAresBoolSettingName::MinimapFixedRotation", "value": true},
                {"settingEnum": "EAresBoolSettingName::MinimapRotates", "value": false},
                {"settingEnum": "EAresBoolSettingName::MinimapTranslates", "value": false}
            ]
        })));

        assert_eq!(summary.minimap, ["Fixed rotation", "Player not centered"]);
    }

    fn setting(label: &str, value: &str) -> Setting {
        Setting {
            label: label.to_string(),
            value: value.to_string(),
        }
    }

    #[test]
    fn lists_audio_and_other_settings_readably() {
        let summary = summarize(&payload(serde_json::json!({
            "floatSettings": [
                // Stored as a 32-bit float.
                {"settingEnum": "EAresFloatSettingName::OverallVolume", "value": 0.824999988079071},
                {"settingEnum": "EAresFloatSettingName::MouseSensitivity", "value": 0.4}
            ],
            "intSettings": [
                {"settingEnum": "EAresIntSettingName::MicVolume", "value": 68},
                {"settingEnum": "EAresIntSettingName::PlayerPerfShowFrameRate", "value": 3}
            ],
            "boolSettings": [
                {"settingEnum": "EAresBoolSettingName::ShowCorpses", "value": false},
                {"settingEnum": "EAresBoolSettingName::EnableHRTF", "value": true},
                {"settingEnum": "EAresBoolSettingName::MinimapRotates", "value": false}
            ],
            "stringSettings": [
                {"settingEnum": "EAresStringSettingName::TeamPushToTalkKey", "value": "ThumbMouseButton2"},
                {"settingEnum": "EAresStringSettingName::PlayerPerfPresetType", "value": "Performance Detailed"}
            ]
        })));

        assert_eq!(
            summary.audio_settings,
            [
                setting("Enable HRTF", "On"),
                setting("Mic volume", "68%"),
                setting("Overall volume", "82%"),
                setting("Team push to talk key", "Mouse 5"),
            ]
        );
        assert_eq!(
            summary.other_settings,
            [
                setting("Player perf preset type", "Performance Detailed"),
                setting("Player perf show frame rate", "3"),
                setting("Show corpses", "Off"),
            ]
        );
    }

    #[test]
    fn leaves_out_tips_and_pop_ups_already_seen() {
        let summary = summarize(&payload(serde_json::json!({
            "boolSettings": [
                {"settingEnum": "EAresBoolSettingName::HasSeenSettingsTutorial", "value": true},
                {"settingEnum": "EAresBoolSettingName::HasAcceptedCodeOfConduct", "value": true},
                {"settingEnum": "EAresBoolSettingName::HasEverStartedAMatch", "value": true},
                {"settingEnum": "EAresBoolSettingName::ContextAwareModuleComplete", "value": true}
            ],
            "intSettings": [
                {"settingEnum": "EAresIntSettingName::LastAcceptedCodeOfConductVersion", "value": 1}
            ],
            "stringSettings": [
                {"settingEnum": "EAresStringSettingName::LastSeenAdHocPopup", "value": "ep9act1adhoc"}
            ]
        })));

        assert_eq!(summary.audio_settings, []);
        assert_eq!(summary.other_settings, []);
    }
}
