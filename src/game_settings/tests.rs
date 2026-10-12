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
        unknown: Default::default(),
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
        unknown: Default::default(),
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
