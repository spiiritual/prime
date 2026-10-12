use super::*;

#[test]
fn a_shop_reply_for_an_account_switched_away_from_is_not_reported() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, alt) = two_account_app(dir.path());
    let _ = app.update(Message::TabSelected(crate::ui::Tab::Shop));
    let request = app.store_request.expect("shop request").id;
    let _ = app.update(Message::TabSelected(crate::ui::Tab::Settings));
    let _ = app.update(Message::SelectAccount(alt.id));

    let _ = app.update(Message::StorefrontLoaded(request, Err("boom".to_string())));

    assert!(
        !app.status.text.contains("Store check failed"),
        "{}",
        app.status.text
    );
    assert_eq!(app.store_request, None);
}

#[test]
fn an_old_shop_reply_does_not_end_the_newer_load_for_the_same_account() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, alt) = two_account_app(dir.path());
    let _ = app.update(Message::TabSelected(crate::ui::Tab::Shop));
    let first = app.store_request.expect("first request").id;
    let _ = app.update(Message::SelectAccount(alt.id));
    let _ = app.update(Message::SelectAccount(main.id));
    let latest = app.store_request.expect("latest request");

    let _ = app.update(Message::StorefrontLoaded(first, Err("boom".to_string())));

    assert_eq!(app.store_request, Some(latest));
    assert!(
        !app.status.text.contains("Store check failed"),
        "{}",
        app.status.text
    );
}

#[test]
fn an_old_loadout_reply_does_not_end_the_newer_load_for_the_same_account() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, alt) = two_account_app(dir.path());
    let _ = app.update(Message::TabSelected(crate::ui::Tab::Loadout));
    let first = app.loadout_request.expect("first request").id;
    let _ = app.update(Message::SelectAccount(alt.id));
    let _ = app.update(Message::SelectAccount(main.id));
    let latest = app.loadout_request.expect("latest request");

    let _ = app.update(Message::LoadoutLoaded(first, Err("boom".to_string())));

    assert_eq!(app.loadout_request, Some(latest));
    assert!(
        !app.status.text.contains("Loadout check failed"),
        "{}",
        app.status.text
    );
}

#[test]
fn clearing_the_image_cache_drops_shop_and_loadout_that_point_into_it() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.store_summary = Some(empty_store_summary());
    app.loadout_summary = Some(LoadoutSummary::without_loadout("old".to_string(), None));

    let _ = app.update(Message::ImageCacheCleared(Ok(())));

    assert_eq!(app.store_summary, None);
    assert_eq!(app.loadout_summary, None);
}

#[test]
fn the_image_cache_is_not_cleared_twice_at_once() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.image_cache = crate::image_cache::ImageCache::new(dir.path().join("images"));

    let first = app.update(Message::ClearImageCache);
    let second = app.update(Message::ClearImageCache);
    let _ = app.update(Message::ImageCacheCleared(Ok(())));
    let after = app.update(Message::ClearImageCache);

    assert_eq!(first.units(), 1);
    assert_eq!(second.units(), 0);
    assert_eq!(after.units(), 1);
}

#[test]
fn a_failed_shop_load_is_kept_for_the_retry_panel() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    let _ = app.update(Message::TabSelected(crate::ui::Tab::Shop));
    let request = app.store_request.expect("shop request").id;

    let _ = app.update(Message::StorefrontLoaded(request, Err("boom".to_string())));

    assert_eq!(app.store_error.as_deref(), Some("boom"));
}

#[test]
fn only_a_failed_shop_with_no_retry_running_fills_the_page() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    app.store_error = Some("boom".to_string());

    assert!(crate::ui::screens::fills_page(&app, crate::ui::Tab::Shop));
    assert!(!crate::ui::screens::fills_page(
        &app,
        crate::ui::Tab::Loadout
    ));

    let _ = app.update(Message::RetryShop);

    assert!(!crate::ui::screens::fills_page(&app, crate::ui::Tab::Shop));
}

#[test]
fn trying_the_shop_again_starts_a_new_load() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    app.active_tab = crate::ui::Tab::Shop;
    app.store_error = Some("boom".to_string());

    let _ = app.update(Message::RetryShop);

    assert!(app.store_request.is_some());
    assert_eq!(app.store_error, None);
}

#[test]
fn trying_the_loadout_again_starts_a_new_load() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    app.active_tab = crate::ui::Tab::Loadout;
    app.loadout_summary = Some(LoadoutSummary {
        battle_pass_error: Some("500".to_string()),
        ..loaded_loadout()
    });

    let _ = app.update(Message::RetryLoadout);

    assert!(app.loadout_request.is_some());
}

#[test]
fn the_shop_waits_for_the_client_version() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    app.client_version_input.clear();

    let _ = app.update(Message::TabSelected(crate::ui::Tab::Shop));
    assert_eq!(app.store_request, None);

    let _ = app.update(Message::ClientVersionLoaded {
        user_requested: false,
        result: Ok("release-2".to_string()),
    });
    assert!(app.store_request.is_some());
}

#[test]
fn opening_the_shop_without_accounts_shows_no_error() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.client_version_input = "release-1".to_string();

    let _ = app.update(Message::TabSelected(crate::ui::Tab::Shop));

    assert!(
        !(app.status.kind == StatusKind::Error),
        "{}",
        app.status.text
    );
}

#[test]
fn launching_the_selected_account_keeps_its_shop() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = account_with_backup(&app.repo.launcher_backups_dir(), "Main", "settings");
    app.state.push_account(account.clone());
    app.state.select_account(account.id);
    app.store_summary = Some(empty_store_summary());
    app.launch_preflight_account = Some(account.id);

    let _ = app.update(Message::LaunchPreflightChecked(
        crate::ui::data::account_details::AccountActivityCheck {
            account_id: account.id,
            availability: AccountAvailability::Available,
        },
        false,
    ));

    assert_eq!(app.launching_account, Some(account.id));
    assert!(app.store_summary.is_some());
}

#[test]
fn launching_another_account_reloads_the_open_tab_for_it() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let main = account_with_backup(&backup_root, "Main", "settings");
    let alt = account_with_backup(&backup_root, "Alt", "settings");
    app.state.push_account(main.clone());
    app.state.push_account(alt.clone());
    app.state.select_account(main.id);
    app.client_version_input = "release-1".to_string();
    app.active_tab = crate::ui::Tab::Shop;
    app.store_summary = Some(empty_store_summary());
    app.launch_preflight_account = Some(alt.id);

    let _ = app.update(Message::LaunchPreflightChecked(
        crate::ui::data::account_details::AccountActivityCheck {
            account_id: alt.id,
            availability: AccountAvailability::Available,
        },
        false,
    ));

    assert_eq!(app.state.selected_account, Some(alt.id));
    assert!(
        app.store_request
            .is_some_and(|request| request.account_id == alt.id)
    );
    assert_eq!(app.launching_account, Some(alt.id));
    // The Launch button says it's launching, so no toast repeats it.
    assert!(app.status.text.is_empty(), "{}", app.status.text);
}

fn seconds_ago(seconds: u64) -> iced::time::Instant {
    iced::time::Instant::now()
        .checked_sub(Duration::from_secs(seconds))
        .expect("earlier instant")
}

fn loadout_with_battle_pass(
    remaining_seconds: i64,
    loaded_at: iced::time::Instant,
) -> LoadoutSummary {
    LoadoutSummary {
        battle_pass: Some(BattlePassProgressDisplay {
            remaining_seconds: Some(remaining_seconds),
            loaded_at,
            ..battle_pass_display()
        }),
        ..loaded_loadout()
    }
}

#[test]
fn countdown_timer_runs_only_where_a_countdown_shows() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.store_summary = Some(empty_store_summary());
    app.loadout_summary = Some(loadout_with_battle_pass(3_600, iced::time::Instant::now()));

    app.active_tab = crate::ui::Tab::Accounts;
    assert_eq!(countdown_timer_interval(&app), None);

    app.active_tab = crate::ui::Tab::Shop;
    assert!(countdown_timer_interval(&app).is_some());

    app.window_minimized = true;
    assert_eq!(countdown_timer_interval(&app), None);
    app.window_minimized = false;

    app.active_tab = crate::ui::Tab::Loadout;
    app.active_loadout_tab = crate::ui::LoadoutTab::Skins;
    assert_eq!(countdown_timer_interval(&app), None);

    app.active_loadout_tab = crate::ui::LoadoutTab::BattlePass;
    assert_eq!(
        countdown_timer_interval(&app),
        Some(crate::ui::BATTLE_PASS_TIMER_INTERVAL)
    );
}

#[test]
fn opening_the_shop_after_its_reset_reloads_it() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    app.store_summary = Some(StoreSummary {
        daily_remaining_seconds: 10,
        loaded_at: seconds_ago(20),
        ..empty_store_summary()
    });

    let _ = app.update(Message::TabSelected(crate::ui::Tab::Shop));

    assert!(app.store_request.is_some());
    assert!(app.store_summary.is_none());
}

#[test]
fn an_ended_battle_pass_reloads_the_loadout() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    app.active_tab = crate::ui::Tab::Loadout;
    app.active_loadout_tab = crate::ui::LoadoutTab::BattlePass;
    app.loadout_summary = Some(loadout_with_battle_pass(10, seconds_ago(20)));

    let _ = app.update(Message::ShopTimerTick(iced::time::Instant::now()));

    assert!(app.loadout_request.is_some());
}

#[test]
fn a_battle_pass_that_had_already_ended_does_not_keep_reloading() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    app.active_tab = crate::ui::Tab::Loadout;
    app.active_loadout_tab = crate::ui::LoadoutTab::BattlePass;
    app.loadout_summary = Some(loadout_with_battle_pass(0, seconds_ago(20)));

    let _ = app.update(Message::ShopTimerTick(iced::time::Instant::now()));

    assert!(app.loadout_request.is_none());
    assert_eq!(countdown_timer_interval(&app), None);
}

#[test]
fn switching_accounts_ends_the_progress_of_loads_it_drops() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, alt) = two_account_app(dir.path());
    app.set_status(Status::progress("Shop reset reached; loading updated shop"));

    let _ = app.update(Message::SelectAccount(alt.id));
    assert_ne!(app.status.kind, StatusKind::Progress);
}
