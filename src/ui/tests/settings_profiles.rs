use super::*;

fn settings_profile_metadata(
    name: &str,
    purpose: GameSettingsProfilePurpose,
    captured_at_unix: i64,
) -> GameSettingsProfileMetadata {
    GameSettingsProfileMetadata {
        id: format!("{captured_at_unix}-profile-id"),
        name: name.to_string(),
        purpose,
        source_account_id: AccountId::new(),
        source_display_name: "Main".to_string(),
        source_puuid: "Main-puuid".to_string(),
        captured_at_unix,
        settings_version: Some(15),
        summary: Box::new(GameSettingsProfileSummary {
            sensitivity: Some(0.4),
            ..GameSettingsProfileSummary::default()
        }),
    }
}

fn settings_saved(account_id: AccountId, profile: GameSettingsProfileMetadata) -> Message {
    Message::AccountSettingsSaved(Ok(SavedGameSettingsResult {
        account_id,
        session: AuthSession::new("fresh", None, None, "Bearer", Some(3600), 100),
        launcher_session: None,
        identity: settings_api_identity(),
        profile,
    }))
}

fn settings_app(dir: &Path) -> (PrimeApp, AccountId) {
    let mut app = test_app(dir);
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    let account_id = account.id;
    app.state.push_account(account);
    (app, account_id)
}

#[test]
fn settings_profiles_have_their_own_accounts_sub_tab() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = settings_app(dir.path());

    let _ = app.update(Message::AccountsTabSelected(
        crate::ui::AccountsTab::GameSettings,
    ));
    assert_eq!(
        app.active_accounts_tab,
        crate::ui::AccountsTab::GameSettings
    );

    // A finished capture brings the Accounts sub-tab back so its confirmation is in view.
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );
    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft)));
    assert_eq!(app.active_accounts_tab, crate::ui::AccountsTab::Accounts);
}

#[test]
fn adding_an_account_saves_its_settings_when_asked() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );

    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft.clone())));
    let _ = app.update(Message::SaveSettingsOnAddToggled(true));
    let task = app.update(Message::ConfirmCapturedAccount);

    assert_eq!(app.state.accounts.len(), 1);
    assert_eq!(app.settings_saving_account, Some(draft.account_id));
    assert!(task.units() > 0);
    assert!(app.status.text.starts_with("Added "));
    assert!(
        app.status
            .text
            .ends_with("Saving its VALORANT settings as a preset\u{2026}")
    );
}

#[test]
fn an_account_added_while_its_capture_waits_saves_no_preset_and_keeps_the_error() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );
    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft)));
    let _ = app.update(Message::SaveSettingsOnAddToggled(true));
    // The same Riot account arrives another way, such as an import, before the draft is saved.
    let mut existing = AccountProfile::new("Main", Shard::Na).expect("account");
    existing
        .apply_riot_identity("puuid-a", "Player", "NA1")
        .expect("identity");
    app.state.push_account(existing);

    let _ = app.update(Message::ConfirmCapturedAccount);

    assert_eq!(app.state.accounts.len(), 1);
    assert_eq!(app.settings_saving_account, None);
    assert_eq!(app.status.kind, StatusKind::Error);
    assert!(
        app.status.text.starts_with("Duplicate account"),
        "{}",
        app.status.text
    );
}

#[test]
fn adding_an_account_leaves_its_settings_unless_asked() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );

    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft)));
    let _ = app.update(Message::SaveSettingsOnAddToggled(false));
    let _ = app.update(Message::ConfirmCapturedAccount);

    assert_eq!(app.state.accounts.len(), 1);
    assert_eq!(app.settings_saving_account, None);
}

#[test]
fn confirming_without_a_draft_saves_no_settings() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    app.state.select_account(account_id);
    app.save_settings_on_add = true;

    let _ = app.update(Message::ConfirmCapturedAccount);

    assert_eq!(app.settings_saving_account, None);
}

fn profile_ids(app: &PrimeApp) -> Vec<String> {
    app.settings_profiles
        .iter()
        .map(|profile| profile.id.clone())
        .collect()
}

#[test]
fn a_saved_preset_is_listed_first() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let older = settings_profile_metadata("Alt settings", GameSettingsProfilePurpose::Profile, 100);
    app.settings_profiles = vec![older.clone()];
    app.settings_saving_account = Some(account_id);
    let saved =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);

    let _ = app.update(settings_saved(account_id, saved.clone()));

    assert_eq!(app.settings_saving_account, None);
    assert_eq!(profile_ids(&app), [saved.id, older.id]);
    assert_eq!(app.status, Status::success("Saved preset Main settings"));
}

#[test]
fn saved_settings_profile_is_listed_when_the_account_update_fails() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let saved =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);

    let _ = app.update(settings_saved(AccountId::new(), saved.clone()));

    assert_eq!(profile_ids(&app), [saved.id]);
    assert!(
        app.status.text.contains("account update failed"),
        "{}",
        app.status.text
    );
}

#[test]
fn saving_a_preset_asks_for_its_name_first() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());

    let task = app.update(Message::RequestSavePreset(account_id));

    assert_eq!(task.units(), 0);
    assert_eq!(app.settings_saving_account, None);
    assert_eq!(
        app.preset_name_prompt,
        Some(PresetNamePrompt {
            target: PresetNameTarget::New(account_id),
            name: "Main settings".to_string(),
        })
    );

    let _ = app.update(Message::PresetNameChanged("   ".to_string()));
    let task = app.update(Message::ConfirmPresetName);

    assert_eq!(task.units(), 0);
    assert!(app.preset_name_prompt.is_some());

    let _ = app.update(Message::PresetNameChanged(" Aim duels ".to_string()));
    let task = app.update(Message::ConfirmPresetName);

    assert!(task.units() > 0);
    assert_eq!(app.preset_name_prompt, None);
    assert_eq!(app.settings_saving_account, Some(account_id));
    assert!(app.status.text.contains("Aim duels"), "{}", app.status.text);
}

#[test]
fn the_name_dialog_stays_open_when_the_save_cannot_start() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestSavePreset(account_id));
    app.settings_applying_account = Some(AccountId::new());

    let task = app.update(Message::ConfirmPresetName);

    assert_eq!(task.units(), 0);
    assert!(app.preset_name_prompt.is_some());
    assert_eq!(app.settings_saving_account, None);
}

#[test]
fn renaming_a_preset_starts_from_its_name() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = settings_app(dir.path());
    let preset =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    app.settings_profiles = vec![preset.clone()];

    let _ = app.update(Message::RequestRenamePreset(preset.id.clone()));

    assert_eq!(
        app.preset_name_prompt,
        Some(PresetNamePrompt {
            target: PresetNameTarget::Rename(preset.id.clone()),
            name: "Main settings".to_string(),
        })
    );

    let _ = app.update(Message::PresetNameChanged("Old crosshair".to_string()));
    let task = app.update(Message::ConfirmPresetName);

    assert!(task.units() > 0);
    assert_eq!(app.preset_name_prompt, None);

    let renamed = GameSettingsProfileMetadata {
        name: "Old crosshair".to_string(),
        ..preset
    };
    let _ = app.update(Message::PresetRenamed(Ok(renamed.clone())));

    assert_eq!(app.settings_profiles, [renamed]);
    assert_eq!(
        app.status,
        Status::success("Renamed preset to Old crosshair")
    );
}

#[test]
fn a_presets_menu_opens_from_its_card_and_closes_with_escape_or_a_choice() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = settings_app(dir.path());
    let preset =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    app.settings_profiles = vec![preset.clone()];

    let _ = app.update(Message::TogglePresetMenu(preset.id.clone()));
    assert_eq!(app.open_preset_menu.as_ref(), Some(&preset.id));

    let _ = app.update(Message::EscapePressed);
    assert_eq!(app.open_preset_menu, None);

    let _ = app.update(Message::TogglePresetMenu(preset.id.clone()));
    let _ = app.update(Message::RequestRenamePreset(preset.id.clone()));
    assert_eq!(app.open_preset_menu, None);
    assert!(app.preset_name_prompt.is_some());
}

#[test]
fn show_all_settings_opens_and_closes_one_preset() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = settings_app(dir.path());
    let main = settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    let alt = settings_profile_metadata("Alt settings", GameSettingsProfilePurpose::Profile, 300);
    app.settings_profiles = vec![main.clone(), alt.clone()];

    let task = app.update(Message::TogglePresetSettings(main.id.clone()));

    assert_eq!(task.units(), 0);
    assert!(app.expanded_presets.contains(&main.id));
    assert!(!app.expanded_presets.contains(&alt.id));

    let _ = app.update(Message::TogglePresetSettings(main.id));

    assert!(app.expanded_presets.is_empty());
}

#[test]
fn the_apply_panel_follows_the_picked_preset_and_falls_back_to_the_first() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = settings_app(dir.path());
    let backup = settings_profile_metadata("Backup", GameSettingsProfilePurpose::Backup, 100);
    let main = settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    let alt = settings_profile_metadata("Alt settings", GameSettingsProfilePurpose::Profile, 300);
    app.settings_profiles = vec![backup, main.clone(), alt.clone()];
    let selected =
        |app: &PrimeApp| crate::ui::screens::selected_preset(app).map(|profile| profile.id.clone());

    // A backup is never a preset to apply.
    assert_eq!(selected(&app), Some(main.id.clone()));

    let task = app.update(Message::SelectPreset(alt.id.clone()));

    assert_eq!(task.units(), 0);
    assert_eq!(selected(&app), Some(alt.id.clone()));

    app.settings_profiles.retain(|profile| profile.id != alt.id);

    assert_eq!(selected(&app), Some(main.id));
}

#[test]
fn settings_profiles_fill_the_page() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = settings_app(dir.path());

    assert!(!crate::ui::screens::fills_page(
        &app,
        crate::ui::Tab::Accounts
    ));

    let _ = app.update(Message::AccountsTabSelected(
        crate::ui::AccountsTab::GameSettings,
    ));

    assert!(crate::ui::screens::fills_page(
        &app,
        crate::ui::Tab::Accounts
    ));
}

#[test]
fn an_error_toast_closes_with_its_button_or_escape_once_dialogs_are_closed() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let error = Status::error("Could not load shop: request timed out");

    app.status = error.clone();
    let _ = app.update(Message::DismissStatus);
    assert!(!status_bar_visible(&app));

    app.status = error.clone();
    app.account_switcher_open = true;
    let _ = app.update(Message::EscapePressed);
    assert!(!app.account_switcher_open);
    assert_eq!(app.status, error);

    let _ = app.update(Message::EscapePressed);
    assert!(!status_bar_visible(&app));

    // Other kinds fade by themselves, so Escape leaves them alone.
    app.status = Status::success("Saved settings");
    app.status_changed_at = iced::time::Instant::now();
    let _ = app.update(Message::EscapePressed);
    assert_eq!(app.status, Status::success("Saved settings"));
}

#[test]
fn escape_closes_the_preset_dialogs() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());

    let _ = app.update(Message::RequestSavePreset(account_id));
    let _ = app.update(Message::EscapePressed);
    assert_eq!(app.preset_name_prompt, None);

    let _ = app.update(Message::RequestRestoreSettings(account_id));
    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Available,
        false,
    ));
    assert!(app.confirm_settings_change.is_some());
    let _ = app.update(Message::EscapePressed);
    assert_eq!(app.confirm_settings_change, None);
}

fn settings_checked(
    request_id: u64,
    account_id: AccountId,
    availability: AccountAvailability,
    valorant_running: bool,
) -> Message {
    Message::SettingsPreflightChecked(
        request_id,
        account_id,
        Some((
            crate::ui::data::account_details::AccountActivityCheck {
                account_id,
                availability,
            },
            None,
        )),
        valorant_running,
    )
}

fn with_fresh_availability(
    app: &mut PrimeApp,
    account_id: AccountId,
    availability: AccountAvailability,
) {
    app.account_availability_checked_at
        .insert(account_id, iced::time::Instant::now());
    app.account_availability.insert(account_id, availability);
}

fn pending_warning(app: &PrimeApp) -> Option<&str> {
    app.confirm_settings_change
        .as_ref()
        .and_then(|pending| pending.warning.as_deref())
}

/// Delivers the local-only result sent when the poll's result was fresh, so no Riot check ran.
fn settings_checked_locally(app: &mut PrimeApp, account_id: AccountId, valorant_running: bool) {
    let _ = app.update(Message::SettingsPreflightChecked(
        app.next_request_id,
        account_id,
        None,
        valorant_running,
    ));
}

#[test]
fn a_fresh_result_opens_the_dialog_without_checking() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let preset =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    app.settings_profiles = vec![preset.clone()];
    with_fresh_availability(&mut app, account_id, AccountAvailability::Available);
    let change = SettingsChange::Apply {
        account_id,
        profile_id: preset.id.clone(),
    };

    let _ = app.update(Message::RequestApplyPreset {
        profile_id: preset.id,
        account_id,
    });

    // Only the local VALORANT check runs; the dialog waits for it.
    assert_eq!(app.confirm_settings_change, None);
    assert_eq!(
        app.settings_check,
        Some(PendingSettingsCheck {
            request_id: app.next_request_id,
            change: change.clone(),
        })
    );

    settings_checked_locally(&mut app, account_id, false);

    assert_eq!(app.settings_check, None);
    assert_eq!(
        app.confirm_settings_change,
        Some(PendingSettingsChange {
            change,
            warning: None,
            check_failed: false,
        })
    );
}

#[test]
fn a_fresh_result_in_a_match_warns_at_once() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    with_fresh_availability(
        &mut app,
        account_id,
        AccountAvailability::Unavailable(Busy::InMatch),
    );

    let _ = app.update(Message::RequestRestoreSettings(account_id));
    settings_checked_locally(&mut app, account_id, false);

    let warning = pending_warning(&app).expect("warning");
    assert!(warning.contains("in match"), "{warning}");
}

#[test]
fn an_old_result_checks_first_with_a_loading_state() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());

    let task = app.update(Message::RequestRestoreSettings(account_id));

    assert!(task.units() > 0);
    assert_eq!(app.confirm_settings_change, None);
    assert_eq!(
        app.settings_check,
        Some(PendingSettingsCheck {
            request_id: app.next_request_id,
            change: SettingsChange::Restore(account_id),
        })
    );
    assert!(app.settings_work_in_progress());
    assert!(crate::ui::loading_indicator_active(&app));

    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Unavailable(Busy::InLobby),
        false,
    ));

    assert_eq!(app.settings_check, None);
    assert!(!app.settings_work_in_progress());
    let pending = app.confirm_settings_change.as_ref().expect("open");
    assert!(!pending.check_failed);
    assert!(pending_warning(&app).expect("warning").contains("in lobby"));
    // The check only looks for the game, so it doesn't relabel the Accounts tab.
    assert_eq!(app.account_availability.get(&account_id), None);
}

#[test]
fn a_check_finding_a_match_warns() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));

    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Unavailable(Busy::InMatch),
        false,
    ));

    let warning = pending_warning(&app).expect("warning");
    assert!(warning.contains("in match"), "{warning}");
}

#[test]
fn a_cached_unknown_result_starts_a_real_check() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    with_fresh_availability(
        &mut app,
        account_id,
        AccountAvailability::activity_check_failed(),
    );

    let task = app.update(Message::RequestRestoreSettings(account_id));

    assert!(task.units() > 0);
    assert_eq!(app.confirm_settings_change, None);
    assert!(app.settings_check.is_some());

    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Available,
        false,
    ));

    // The check's own result wins over the cached failure.
    let pending = app.confirm_settings_change.as_ref().expect("open");
    assert!(!pending.check_failed);
    assert_eq!(pending.warning, None);
}

#[test]
fn a_poll_over_90_seconds_old_starts_a_real_check() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let in_match = AccountAvailability::Unavailable(Busy::InMatch);
    with_fresh_availability(&mut app, account_id, in_match.clone());
    app.account_availability_checked_at.insert(
        account_id,
        iced::time::Instant::now() - std::time::Duration::from_secs(91),
    );

    let task = app.update(Message::RequestRestoreSettings(account_id));

    assert!(task.units() > 0);
    assert_eq!(app.confirm_settings_change, None);
    assert!(app.settings_check.is_some());

    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Available,
        false,
    ));

    // The warning comes from the check, and the Accounts tab keeps the poll's result.
    assert!(app.confirm_settings_change.is_some());
    assert_eq!(pending_warning(&app), None);
    assert_eq!(app.account_availability.get(&account_id), Some(&in_match));
}

#[test]
fn a_poll_that_has_only_started_does_not_make_an_old_result_fresh() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    app.client_version_input = "release-1".to_string();
    // Left over from a poll long ago, before the user spent a while on another tab.
    app.account_availability
        .insert(account_id, AccountAvailability::Available);
    app.active_tab = crate::ui::Tab::Shop;

    let task = app.update(Message::TabSelected(crate::ui::Tab::Accounts));

    assert!(task.units() > 0);
    assert!(app.account_availability_loading);
    assert_eq!(app.fresh_availability(account_id), None);
}

#[test]
fn a_poll_result_arriving_makes_it_fresh() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());

    let _ = app.update(Message::AccountAvailabilitiesLoaded(
        AccountAvailabilityRefresh {
            accounts: vec![crate::ui::data::account_details::AccountActivityCheck {
                account_id,
                availability: AccountAvailability::Available,
            }],
            refreshed_sessions: vec![],
        },
    ));

    assert_eq!(
        app.fresh_availability(account_id),
        Some(&AccountAvailability::Available)
    );
}

#[test]
fn a_launch_check_result_makes_it_fresh() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    app.launch_preflight_account = Some(account_id);
    let in_lobby = AccountAvailability::Unavailable(Busy::InLobby);

    let _ = app.update(Message::LaunchPreflightChecked(
        crate::ui::data::account_details::AccountActivityCheck {
            account_id,
            availability: in_lobby.clone(),
        },
        false,
    ));

    assert_eq!(app.fresh_availability(account_id), Some(&in_lobby));
}

#[test]
fn a_poll_tick_during_a_settings_check_starts_no_poll() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    app.client_version_input = "release-1".to_string();
    app.active_tab = crate::ui::Tab::Accounts;
    app.settings_check = Some(PendingSettingsCheck {
        request_id: 1,
        change: SettingsChange::Restore(account_id),
    });

    let task = app.update(Message::AccountAvailabilityTimerTick(
        iced::time::Instant::now(),
    ));

    assert_eq!(task.units(), 0);
    assert!(!app.account_availability_loading);
}

#[test]
fn a_poll_tick_while_settings_are_applied_starts_no_poll() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    app.client_version_input = "release-1".to_string();
    app.active_tab = crate::ui::Tab::Accounts;
    app.settings_applying_account = Some(account_id);

    let task = app.update(Message::AccountAvailabilityTimerTick(
        iced::time::Instant::now(),
    ));

    assert_eq!(task.units(), 0);
    assert!(!app.account_availability_loading);
}

#[test]
fn a_poll_tick_while_a_preset_is_saved_starts_no_poll() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    app.client_version_input = "release-1".to_string();
    app.active_tab = crate::ui::Tab::Accounts;
    app.settings_saving_account = Some(account_id);

    let task = app.update(Message::AccountAvailabilityTimerTick(
        iced::time::Instant::now(),
    ));

    assert_eq!(task.units(), 0);
    assert!(!app.account_availability_loading);
}

#[test]
fn a_failed_check_opens_the_dialog_with_a_note() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));

    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::activity_check_failed(),
        false,
    ));

    let pending = app.confirm_settings_change.as_ref().expect("open");
    assert!(pending.check_failed);
    assert_eq!(pending.warning, None);
}

#[test]
fn applying_warns_when_valorant_is_running() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    with_fresh_availability(&mut app, account_id, AccountAvailability::Available);
    let _ = app.update(Message::RequestRestoreSettings(account_id));

    settings_checked_locally(&mut app, account_id, true);

    let warning = pending_warning(&app).expect("warning");
    assert!(warning.contains("VALORANT is running"), "{warning}");
}

#[test]
fn a_check_for_another_account_changes_nothing() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));
    let checking = app.settings_check.clone();
    let other_account = AccountId::new();

    let _ = app.update(settings_checked(
        app.next_request_id,
        other_account,
        AccountAvailability::Available,
        true,
    ));

    assert_eq!(app.confirm_settings_change, None);
    assert_eq!(app.settings_check, checking);
    assert_eq!(app.account_availability.get(&other_account), None);
}

#[test]
fn a_check_from_an_earlier_request_changes_nothing() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));
    let first_request = app.next_request_id;
    let _ = app.update(Message::EscapePressed);
    with_fresh_availability(&mut app, account_id, AccountAvailability::Available);
    let _ = app.update(Message::RequestRestoreSettings(account_id));
    let checking = app.settings_check.clone();

    let _ = app.update(settings_checked(
        first_request,
        account_id,
        AccountAvailability::Unavailable(Busy::InLobby),
        true,
    ));

    assert_eq!(app.confirm_settings_change, None);
    assert_eq!(app.settings_check, checking);
    assert_eq!(
        app.account_availability.get(&account_id),
        Some(&AccountAvailability::Available)
    );
}

fn check_with_session(request_id: u64, account_id: AccountId, session: &AuthSession) -> Message {
    Message::SettingsPreflightChecked(
        request_id,
        account_id,
        Some((
            crate::ui::data::account_details::AccountActivityCheck {
                account_id,
                availability: AccountAvailability::Available,
            },
            Some(crate::ui::data::account_details::RefreshedApiContext {
                account_id,
                session: session.clone(),
                launcher_session: None,
                identity: settings_api_identity(),
            }),
        )),
        false,
    )
}

fn saved_session(app: &PrimeApp, account_id: AccountId) -> Option<&AuthSession> {
    app.state
        .accounts
        .iter()
        .find(|account| account.id == account_id)
        .and_then(|account| account.session.as_ref())
}

#[test]
fn escape_while_checking_cancels_but_keeps_the_checks_session() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));
    let request_id = app.next_request_id;

    let _ = app.update(Message::EscapePressed);

    assert_eq!(app.settings_check, None);
    assert!(!app.settings_work_in_progress());

    let session = AuthSession::new("fresh", None, None, "Bearer", Some(3600), 100);
    let task = app.update(check_with_session(request_id, account_id, &session));

    assert!(task.units() > 0, "saves accounts.json");
    assert_eq!(saved_session(&app, account_id), Some(&session));
    assert_eq!(app.confirm_settings_change, None);
}

#[test]
fn the_checks_refreshed_session_is_saved_before_the_dialog_opens() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));
    let session = AuthSession::new("fresh", None, None, "Bearer", Some(3600), 100);

    let task = app.update(check_with_session(
        app.next_request_id,
        account_id,
        &session,
    ));

    assert!(task.units() > 0, "saves accounts.json");
    assert_eq!(saved_session(&app, account_id), Some(&session));
    assert!(app.confirm_settings_change.is_some());
}

#[test]
fn other_settings_work_waits_for_the_check() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let preset =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    app.settings_profiles = vec![preset.clone()];
    let _ = app.update(Message::RequestRestoreSettings(account_id));
    let checking = app.settings_check.clone();

    let tasks = [
        app.update(Message::RequestSavePreset(account_id)),
        app.update(Message::RequestApplyPreset {
            profile_id: preset.id,
            account_id,
        }),
        app.update(Message::ConfirmSettingsChange),
    ];

    assert!(tasks.iter().all(|task| task.units() == 0));
    assert_eq!(app.preset_name_prompt, None);
    assert_eq!(app.settings_check, checking);
    assert_eq!(app.settings_applying_account, None);
}

#[test]
fn a_check_for_a_deleted_account_opens_nothing() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));
    app.state.remove_account(account_id);

    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Available,
        false,
    ));

    assert_eq!(app.settings_check, None);
    assert_eq!(app.confirm_settings_change, None);
}

#[test]
fn confirming_apply_starts_it() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let preset =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    app.settings_profiles = vec![preset.clone()];
    let _ = app.update(Message::RequestApplyPreset {
        profile_id: preset.id,
        account_id,
    });
    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Available,
        false,
    ));

    let task = app.update(Message::ConfirmSettingsChange);

    assert!(task.units() > 0);
    assert_eq!(app.confirm_settings_change, None);
    assert_eq!(app.settings_applying_account, Some(account_id));
    assert!(
        app.status.text.contains("Main settings"),
        "{}",
        app.status.text
    );
}

#[test]
fn applied_status_points_to_restore_only_when_it_saved_the_accounts_settings() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let preset =
        settings_profile_metadata("Alt settings", GameSettingsProfilePurpose::Profile, 200);
    let backup = settings_profile_metadata(
        "Main original settings",
        GameSettingsProfilePurpose::Backup,
        300,
    );

    app.settings_applying_account = Some(account_id);
    let task = app.update(applied(account_id, preset.clone(), Some(backup)));

    assert!(task.units() > 0, "reloads the list to show the new backup");
    assert_eq!(app.settings_applying_account, None);
    assert_eq!(
        app.status.text,
        "Applied preset Alt settings. Its own settings were saved; restore them from Game settings"
    );

    let _ = app.update(applied(account_id, preset, None));

    assert_eq!(app.status, Status::success("Applied preset Alt settings"));
}

#[test]
fn restoring_asks_first_then_starts() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());

    let task = app.update(Message::RequestRestoreSettings(account_id));
    assert!(task.units() > 0);
    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Available,
        false,
    ));

    assert_eq!(
        app.confirm_settings_change,
        Some(PendingSettingsChange {
            change: SettingsChange::Restore(account_id),
            warning: None,
            check_failed: false,
        })
    );
    assert_eq!(app.settings_applying_account, None);

    let task = app.update(Message::ConfirmSettingsChange);

    assert!(task.units() > 0);
    assert_eq!(app.confirm_settings_change, None);
    assert_eq!(app.settings_applying_account, Some(account_id));

    let task = app.update(Message::SettingsRestored(Ok(RestoredGameSettingsResult {
        account_id,
        session: AuthSession::new("fresh", None, None, "Bearer", Some(3600), 100),
        launcher_session: None,
        identity: settings_api_identity(),
    })));

    assert!(
        task.units() > 0,
        "reloads the list without the restored backup"
    );
    assert_eq!(app.settings_applying_account, None);
    assert_eq!(app.status, Status::success("Restored original settings"));
}

#[test]
fn restore_waits_for_other_settings_work() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    app.settings_saving_account = Some(account_id);

    let task = app.update(Message::RequestRestoreSettings(account_id));

    assert_eq!(task.units(), 0);
    assert_eq!(app.settings_check, None);
    assert_eq!(app.confirm_settings_change, None);
}

#[test]
fn each_accounts_original_settings_are_its_oldest_backup() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let backup_of = |account_id, captured_at_unix| GameSettingsProfileMetadata {
        source_account_id: account_id,
        ..settings_profile_metadata(
            "Main original settings",
            GameSettingsProfilePurpose::Backup,
            captured_at_unix,
        )
    };
    let other_account = AccountId::new();
    let newer = backup_of(account_id, 300);
    let original = backup_of(account_id, 100);
    let other = backup_of(other_account, 200);
    let preset = GameSettingsProfileMetadata {
        source_account_id: account_id,
        ..settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 50)
    };
    app.settings_profiles = vec![newer, other.clone(), original.clone(), preset];

    let originals = crate::ui::screens::original_settings(&app);

    assert_eq!(
        originals
            .iter()
            .map(|original| original.id.as_str())
            .collect::<Vec<_>>(),
        [original.id.as_str(), other.id.as_str()]
    );
}

#[test]
fn deleting_a_preset_asks_first() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = settings_app(dir.path());
    let deleted =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    let kept = settings_profile_metadata("Alt settings", GameSettingsProfilePurpose::Profile, 100);
    app.settings_profiles = vec![deleted.clone(), kept.clone()];

    let task = app.update(Message::RequestDeleteSettingsProfile(deleted.id.clone()));

    assert_eq!(task.units(), 0);
    assert_eq!(
        app.confirm_delete_settings_profile,
        Some(deleted.id.clone())
    );

    let task = app.update(Message::ConfirmDeleteSettingsProfile);

    assert!(task.units() > 0);
    assert_eq!(app.confirm_delete_settings_profile, None);

    let _ = app.update(Message::SettingsProfileDeleted(deleted.id.clone(), Ok(())));

    assert_eq!(profile_ids(&app), std::slice::from_ref(&kept.id));
    assert_eq!(app.status, Status::success("Deleted Main settings"));
}
