use super::*;

fn app_with_chat_through_prime(dir: &Path) -> PrimeApp {
    let mut app = test_app(dir);
    app.state.minimize_on_close = false;
    app.chat_proxy = Some(ChatProxy::detached(PresenceStatus::Online, true));
    app
}

#[test]
fn closing_asks_before_dropping_riot_client_chat() {
    let dir = tempdir().expect("temp dir");
    let mut app = app_with_chat_through_prime(dir.path());

    let task = app.update(Message::CloseRequested(iced::window::Id::unique()));

    assert_eq!(task.units(), 0, "doesn't quit yet");
    assert_eq!(app.confirm_quit, Some(QuitAction::Exit));
    assert_eq!(app.open_dialog(), Some(Dialog::Quit));

    let _ = app.update(Message::EscapePressed);
    assert_eq!(app.confirm_quit, None);
}

#[test]
fn closing_quits_at_once_when_chat_doesnt_go_through_prime() {
    let dir = tempdir().expect("temp dir");
    let mut app = app_with_chat_through_prime(dir.path());
    app.chat_proxy = Some(ChatProxy::detached(PresenceStatus::Online, false));

    let task = app.update(Message::CloseRequested(iced::window::Id::unique()));

    assert!(task.units() > 0);
    assert_eq!(app.confirm_quit, None);
}

#[test]
fn quitting_from_the_tray_brings_the_window_back_to_ask() {
    let dir = tempdir().expect("temp dir");
    let mut app = app_with_chat_through_prime(dir.path());
    app.window_minimized = true;

    let task = app.update(Message::Tray(crate::ui::tray::TrayAction::Quit));

    assert!(task.units() > 0, "shows the window");
    assert!(!app.window_minimized);
    assert_eq!(app.confirm_quit, Some(QuitAction::Exit));

    let task = app.update(Message::ConfirmQuit);
    assert!(task.units() > 0, "quits");
    assert_eq!(app.confirm_quit, None);
}

#[test]
fn an_update_asks_before_restarting_drops_riot_client_chat() {
    let dir = tempdir().expect("temp dir");
    let mut app = app_with_chat_through_prime(dir.path());
    app.app_update_status =
        crate::ui::AppUpdateStatus::Available(crate::updater::sample_update("9.9.9"));

    let task = app.update(Message::DownloadAppUpdate);

    assert_eq!(task.units(), 0);
    assert_eq!(app.confirm_quit, Some(QuitAction::InstallUpdate));
    assert_eq!(app.open_dialog(), Some(Dialog::Quit));

    let task = app.update(Message::ConfirmQuit);
    assert!(task.units() > 0, "downloads");
    assert_eq!(app.confirm_quit, None);
    assert!(matches!(
        app.app_update_status,
        crate::ui::AppUpdateStatus::Downloading(_)
    ));
}

#[test]
fn an_update_does_not_download_while_a_launch_runs() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.app_update_status =
        crate::ui::AppUpdateStatus::Available(crate::updater::sample_update("9.9.9"));
    app.launching_account = Some(AccountId::new());

    let task = app.update(Message::DownloadAppUpdate);

    assert_eq!(task.units(), 0);
    assert!(matches!(
        app.app_update_status,
        crate::ui::AppUpdateStatus::Available(_)
    ));
    assert!(
        (app.status.kind == StatusKind::Error),
        "{}",
        app.status.text
    );
}

#[test]
fn bringing_the_window_back_checks_for_updates_unless_one_was_dismissed() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.app_update_status = crate::ui::AppUpdateStatus::UpToDate;
    app.app_update_checked_at = None;

    let _ = app.update(Message::Tray(crate::ui::tray::TrayAction::Open));
    assert!(matches!(
        app.app_update_status,
        crate::ui::AppUpdateStatus::Checking
    ));

    app.app_update_status =
        crate::ui::AppUpdateStatus::Dismissed(crate::updater::sample_update("9.9.9"));
    app.app_update_checked_at = None;
    let _ = app.update(Message::Tray(crate::ui::tray::TrayAction::Open));
    assert!(matches!(
        app.app_update_status,
        crate::ui::AppUpdateStatus::Dismissed(_)
    ));
}

#[test]
fn switching_update_channel_checks_that_channel_unless_busy() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.app_update_status = crate::ui::AppUpdateStatus::UpToDate;

    let task = app.update(Message::SwitchUpdateChannel(crate::updater::BETA_CHANNEL));
    assert!(task.units() > 0, "checks");
    assert!(matches!(
        app.app_update_status,
        crate::ui::AppUpdateStatus::Checking
    ));

    let task = app.update(Message::SwitchUpdateChannel(crate::updater::BETA_CHANNEL));
    assert_eq!(task.units(), 0, "one check at a time");
}

#[test]
fn bringing_the_window_back_soon_after_a_check_does_not_check_again() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.app_update_status = crate::ui::AppUpdateStatus::UpToDate;
    // Startup just checked.
    assert!(app.app_update_checked_at.is_some());

    let _ = app.update(Message::Tray(crate::ui::tray::TrayAction::Open));

    assert!(matches!(
        app.app_update_status,
        crate::ui::AppUpdateStatus::UpToDate
    ));
}

fn downloading_update_app(dir: &Path) -> PrimeApp {
    let mut app = test_app(dir);
    app.app_update_status =
        crate::ui::AppUpdateStatus::Downloading(crate::updater::sample_update("9.9.9"));
    app
}

#[test]
fn a_launch_does_not_start_while_an_update_downloads() {
    let dir = tempdir().expect("temp dir");
    let mut app = downloading_update_app(dir.path());
    let account = account_with_backup(&app.repo.launcher_backups_dir(), "Main", "settings");
    app.state.push_account(account.clone());

    let task = app.update(Message::LaunchAccount(account.id));

    assert_eq!(task.units(), 0);
    assert_eq!(app.launch_preflight_account, None);
    assert!(app.status.text.contains("update"), "{}", app.status.text);
}

#[test]
fn a_login_capture_does_not_start_while_an_update_downloads() {
    let dir = tempdir().expect("temp dir");
    let mut app = downloading_update_app(dir.path());

    let _ = app.update(Message::AddAccount);

    assert!(!app.show_add_account_prompt);
    assert!(app.status.text.contains("update"), "{}", app.status.text);
}

#[test]
fn an_import_does_not_start_while_an_update_downloads() {
    let dir = tempdir().expect("temp dir");
    let mut app = downloading_update_app(dir.path());
    app.import_account_input = "export".to_string();

    let _ = app.update(Message::ConfirmImportAccount);

    assert!(!app.import_account_in_progress);
    assert!(app.status.text.contains("update"), "{}", app.status.text);
}

#[test]
fn development_builds_do_not_report_update_checks_as_failed() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());

    let _ = app.update(Message::AppUpdateChecked {
        user_requested: true,
        result: Ok(crate::updater::UpdateCheckOutcome::NotInstalled),
    });

    assert!(!app.status.text.contains("failed"), "{}", app.status.text);
    assert!(
        app.status.text.contains("not an installed build"),
        "{}",
        app.status.text
    );
}

#[test]
fn a_manual_client_version_refresh_replaces_the_field() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.client_version_input = "x".to_string();
    app.status = Status::progress("Refreshing Riot client version");

    let _ = app.update(Message::ClientVersionLoaded {
        user_requested: true,
        result: Ok("release-2".to_string()),
    });

    assert_eq!(app.client_version_input, "release-2");
    // The field shows the version, so no toast repeats it.
    assert_eq!(app.status, Status::default());
}

#[test]
fn a_finished_load_leaves_launch_progress_on_screen() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account) = accounts_tab_app(dir.path());
    app.launching_account = Some(account.id);
    app.status = Status::progress("Launching Main");

    let _ = app.update(Message::ClientVersionLoaded {
        user_requested: true,
        result: Ok("release-2".to_string()),
    });

    assert_eq!(app.status, Status::progress("Launching Main"));
}

#[test]
fn a_failed_manual_client_version_refresh_is_reported() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let _ = app.update(Message::RefreshClientVersion);

    let _ = app.update(Message::ClientVersionLoaded {
        user_requested: true,
        result: Err("offline".to_string()),
    });

    assert!(
        app.status
            .text
            .starts_with("Could not fetch Riot client version"),
        "{}",
        app.status.text
    );
    assert!(!(app.status.kind == StatusKind::Progress));
}

#[test]
fn the_startup_client_version_does_not_replace_a_startup_error() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.status = Status::error("Failed to load accounts: disk error");

    let _ = app.update(Message::ClientVersionLoaded {
        user_requested: false,
        result: Ok("release-1".to_string()),
    });

    assert_eq!(app.client_version_input, "release-1");
    assert_eq!(
        app.status,
        Status::error("Failed to load accounts: disk error")
    );
}

#[test]
fn a_failed_startup_client_version_fetch_is_shown_and_retried() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.status = Status::info("Loaded 1 account profile(s)");

    let task = app.update(Message::ClientVersionLoaded {
        user_requested: false,
        result: Err("offline".to_string()),
    });

    assert!(
        app.status
            .text
            .starts_with("Could not fetch Riot client version"),
        "{}",
        app.status.text
    );
    assert!(task.units() > 0, "a retry is scheduled");
}

#[test]
fn a_background_update_check_leaves_the_status_alone() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.status = Status::error("Failed to load accounts: disk error");

    let _ = app.update(Message::AppUpdateChecked {
        user_requested: false,
        result: Ok(crate::updater::UpdateCheckOutcome::Available(
            crate::updater::sample_update("9.9.9"),
        )),
    });

    assert_eq!(
        app.status,
        Status::error("Failed to load accounts: disk error")
    );
    assert!(app.app_update_status.prompt_update().is_some());
}

#[test]
fn failed_update_install_is_not_labelled_as_a_failed_check() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());

    let _ = app.update(Message::AppUpdatePrepared(Err("disk full".to_string())));

    assert_eq!(app.app_update_status.label(), "Update failed: disk full");
}

#[test]
fn availability_polling_resumes_when_the_window_is_restored() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.state
        .push_account(AccountProfile::new("Main", Shard::Na).expect("account"));
    app.client_version_input = "release-1".to_string();

    // Minimizing trims memory once; a repeat zero-size resize doesn't trim again.
    let task = app.update(Message::WindowResized(iced::Size::new(0.0, 0.0)));
    assert_eq!(task.units(), 1);
    assert!(app.window_minimized);
    assert!(!app.account_availability_loading);
    let task = app.update(Message::WindowResized(iced::Size::new(0.0, 0.0)));
    assert_eq!(task.units(), 0);

    let _ = app.update(Message::WindowResized(iced::Size::new(900.0, 600.0)));
    assert!(!app.window_minimized);
    assert!(app.account_availability_loading);
}

#[test]
fn minimized_polling_only_runs_when_minimize_on_close_is_chosen() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.state
        .push_account(AccountProfile::new("Main", Shard::Na).expect("account"));
    app.client_version_input = "release-1".to_string();
    app.active_tab = crate::ui::Tab::Shop;
    app.window_minimized = true;
    let tick = || Message::AccountAvailabilityTimerTick(iced::time::Instant::now());
    assert!(app.state.minimize_on_close, "on by default");

    let _ = app.update(Message::MinimizeOnCloseToggled(false));

    assert_eq!(app.update(tick()).units(), 0);
    assert!(!app.account_availability_loading);

    let _ = app.update(Message::MinimizeOnCloseToggled(true));
    assert!(app.state.minimize_on_close);
    let _ = app.update(tick());
    assert!(app.account_availability_loading);
}

#[test]
fn live_match_weapons_are_saved_from_settings() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let ghost = "1baa85b4-4c70-1284-64bb-6481dfc3bb4e";

    let _ = app.update(Message::ToggleWeaponPicker(2));
    assert_eq!(app.open_weapon_picker, Some(2));
    let task = app.update(Message::LiveMatchWeaponPicked {
        column: 2,
        weapon: ghost,
    });

    assert!(task.units() > 0, "saves accounts.json");
    assert_eq!(app.open_weapon_picker, None, "picking closes the picker");
    let weapons = app.state.live_match_weapons.as_deref().expect("saved");
    assert_eq!(weapons[2], ghost);

    let _ = app.update(Message::ToggleWeaponPicker(1));
    let _ = app.update(Message::ToggleWeaponPicker(1));
    assert_eq!(app.open_weapon_picker, None, "its slot closes it again");
    let _ = app.update(Message::ToggleWeaponPicker(1));
    let _ = app.update(Message::EscapePressed);
    assert_eq!(app.open_weapon_picker, None, "Escape closes it");

    let _ = app.update(Message::ResetLiveMatchWeapons);
    assert_eq!(app.state.live_match_weapons, None);
}

#[test]
fn minimize_on_close_is_not_changed_before_accounts_load() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.accounts_loaded = false;
    let _ = app.update(Message::MinimizeOnCloseToggled(false));
    assert!(
        app.state.minimize_on_close,
        "ignored until accounts.json loads"
    );
}

#[test]
fn availability_checks_keep_the_sessions_they_refresh() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = account_with_backup(&app.repo.launcher_backups_dir(), "Main", "settings");
    let backup = account.launcher_session.clone();
    app.state.push_account(account.clone());

    let _ = app.update(availability_refresh(account.id, backup));

    let session = app.state.accounts[0].session.as_ref().expect("session");
    assert_eq!(session.access_token, "fresh");
}

#[test]
fn a_details_load_that_fails_still_keeps_the_session_it_signed_in_with() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = account_with_backup(&app.repo.launcher_backups_dir(), "Main", "settings");
    let Message::AccountAvailabilitiesLoaded(refresh) =
        availability_refresh(account.id, account.launcher_session.clone())
    else {
        unreachable!()
    };
    app.state.push_account(account.clone());

    let task = app.update(Message::AccountRanksLoaded {
        result: crate::ui::data::account_details::AccountRanksResult {
            failures: vec![crate::ui::data::account_details::AccountRankFailure {
                account_id: account.id,
                error: "rank unavailable".to_string(),
            }],
            refreshed_sessions: refresh.refreshed_sessions,
            ..Default::default()
        },
        announce: false,
    });

    let session = app.state.accounts[0].session.as_ref().expect("session");
    assert_eq!(session.access_token, "fresh");
    assert!(task.units() > 0, "saved");
}

#[test]
fn availability_checks_drop_sessions_for_a_replaced_login() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = account_with_backup(&app.repo.launcher_backups_dir(), "Main", "settings");
    let current = account.launcher_session.clone();
    app.state.push_account(account.clone());
    let stale = LauncherSessionBackup {
        data_dir: dir.path().join("old-slot"),
        captured_at_unix: 50,
        puuid: "Main-puuid".to_string(),
        unknown: Default::default(),
    };

    let _ = app.update(availability_refresh(account.id, Some(stale)));

    assert!(app.state.accounts[0].session.is_none());
    assert_eq!(app.state.accounts[0].launcher_session, current);
}

#[test]
fn a_quoted_riot_client_path_is_saved_without_quotes() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let client = dir.path().join("RiotClientServices.exe");
    fs::write(&client, "exe").expect("client");
    app.riot_client_path_input = format!("\"{}\"", client.display());

    let _ = app.update(Message::SaveSettings);

    assert_eq!(app.state.riot_client_path, Some(client));
    assert_eq!(app.status, Status::success("Saved settings"));
}

#[test]
fn a_riot_client_path_that_does_not_exist_is_not_saved() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.riot_client_path_input = dir.path().join("missing.exe").display().to_string();

    let task = app.update(Message::SaveSettings);

    assert_eq!(task.units(), 0);
    assert_eq!(app.state.riot_client_path, None);
    assert!(
        (app.status.kind == StatusKind::Error),
        "{}",
        app.status.text
    );
}

#[test]
fn an_empty_riot_client_path_means_find_it_automatically() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.state.riot_client_path = Some(dir.path().join("old.exe"));
    app.riot_client_path_input = "  ".to_string();

    let _ = app.update(Message::SaveSettings);

    assert_eq!(app.state.riot_client_path, None);
}

#[test]
fn a_typed_riot_client_path_saves_once_the_file_exists() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let client = dir.path().join("RiotClientServices.exe");
    fs::write(&client, "exe").expect("client");

    let half_typed = client.display().to_string();
    let half_typed = &half_typed[..half_typed.len() - 4];
    let task = app.update(Message::RiotClientPathChanged(half_typed.to_string()));
    assert_eq!(task.units(), 0);
    assert!(app.riot_client_path_unsaved());

    let _ = app.update(Message::RiotClientPathChanged(client.display().to_string()));
    assert_eq!(app.state.riot_client_path, Some(client));

    let _ = app.update(Message::RiotClientPathChanged(String::new()));
    assert_eq!(app.state.riot_client_path, None);
}

#[test]
fn a_failed_update_download_can_be_tried_again() {
    let dir = tempdir().expect("temp dir");
    let mut app = downloading_update_app(dir.path());

    let _ = app.update(Message::AppUpdatePrepared(Err("disk full".to_string())));

    assert_eq!(
        app.app_update_status
            .pending_update()
            .map(|update| update.latest_version.as_str()),
        Some("9.9.9")
    );
    assert_eq!(app.app_update_status.label(), "Update failed: disk full");
}

#[test]
fn opening_settings_refreshes_the_image_cache_size() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.image_cache = crate::image_cache::ImageCache::new(dir.path().join("images"));

    let task = app.update(Message::TabSelected(crate::ui::Tab::Settings));

    // Restoring the tab's scroll position is one task; the size refresh is another.
    assert_eq!(task.units(), 2);
}

#[test]
fn fetch_latest_waits_for_its_lookup_and_the_shop_fills_the_page_without_a_version() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    app.client_version_input.clear();
    assert!(crate::ui::screens::fills_page(&app, crate::ui::Tab::Shop));

    assert_eq!(app.update(Message::RefreshClientVersion).units(), 1);
    assert!(app.client_version_fetching);
    assert_eq!(app.update(Message::RefreshClientVersion).units(), 0);

    let _ = app.update(Message::ClientVersionLoaded {
        user_requested: true,
        result: Err("offline".to_string()),
    });
    assert!(!app.client_version_fetching);
}

#[test]
fn settings_nav_switches_pages() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    assert_eq!(app.settings_page, crate::ui::SettingsPage::General);

    let task = app.update(Message::SettingsPageSelected(
        crate::ui::SettingsPage::Storage,
    ));
    assert_eq!(app.settings_page, crate::ui::SettingsPage::Storage);
    assert_eq!(task.units(), 0);
}

#[test]
fn export_logs_runs_once_at_a_time_and_names_no_account() {
    let dir = tempdir().expect("temp dir");
    let (mut app, first, _) = two_account_app(dir.path());

    assert_eq!(app.update(Message::ExportLogs).units(), 1);
    assert!(app.logs_exporting);
    assert_eq!(app.update(Message::ExportLogs).units(), 0);
    let info = app.system_info();
    assert!(info.contains("Accounts: 2"));
    assert!(!info.contains(&first.display_name));

    let task = app.update(Message::LogsExported(Err("disk full".to_string())));
    assert_eq!(task.units(), 0);
    assert!(!app.logs_exporting);
    assert_eq!(app.status.kind, StatusKind::Error);
}
