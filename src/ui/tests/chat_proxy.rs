use super::*;

fn launchable_account(app: &PrimeApp, name: &str) -> AccountProfile {
    let mut account = account_with_backup(&app.repo.launcher_backups_dir(), name, "settings");
    account.puuid = Some(format!("{name}-puuid"));
    account
}

fn proxy_failure_for(account: &AccountProfile) -> Message {
    Message::ChatProxyStarted(
        account.id,
        Err("Couldn't get the chat certificate: offline".to_string()),
    )
}

#[test]
fn a_failed_chat_proxy_asks_before_launching_online_unless_online() {
    for status in [PresenceStatus::Invisible, PresenceStatus::Mobile] {
        let dir = tempdir().expect("temp dir");
        let mut app = test_app(dir.path());
        let account = launchable_account(&app, "Main");
        app.state.push_account(account.clone());
        app.state.presence_status = status;
        app.launching_account = Some(account.id);

        let task = app.update(proxy_failure_for(&account));

        assert_eq!(task.units(), 0, "nothing launches with {status:?}");
        assert_eq!(app.launching_account, None);
        let failure = app.status_launch_failure.as_ref().expect("dialog");
        assert_eq!(failure.account_id, account.id);
        assert_eq!(failure.status, status);
        assert!(failure.error.contains("certificate"), "{}", failure.error);
        assert_eq!(app.open_dialog(), Some(Dialog::StatusLaunchFailed));
    }
}

#[test]
fn a_failed_chat_proxy_still_launches_when_online() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.launching_account = Some(account.id);

    let task = app.update(proxy_failure_for(&account));

    assert!(task.units() > 0, "launches without the proxy");
    assert_eq!(app.launching_account, Some(account.id));
    assert!(app.status_launch_failure.is_none());
    assert!(app.chat_proxy.is_none());
    assert_eq!(app.status.kind, StatusKind::Warning);
}

#[test]
fn launching_online_without_a_proxy_drops_the_previous_one() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.launching_account = Some(account.id);
    app.chat_proxy = Some(ChatProxy::detached(PresenceStatus::Online, true));

    let _ = app.update(proxy_failure_for(&account));

    assert!(app.chat_proxy.is_none());
}

#[test]
fn cancelling_after_an_invisible_failure_keeps_the_running_proxy() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.state.presence_status = PresenceStatus::Invisible;
    app.launching_account = Some(account.id);
    app.chat_proxy = Some(ChatProxy::detached(PresenceStatus::Online, true));

    let _ = app.update(proxy_failure_for(&account));
    let _ = app.update(Message::CancelStatusLaunch);

    assert!(app.chat_proxy.is_some(), "Riot Client is still on it");
}

#[test]
fn a_started_chat_proxy_is_kept() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.launching_account = Some(account.id);
    // The old Riot Client window, seen while the proxy started.
    app.launch_client_open = true;

    let task = app.update(Message::ChatProxyStarted(
        account.id,
        Ok(ChatProxy::detached(PresenceStatus::Online, false)),
    ));

    assert!(task.units() > 0, "launches through the proxy");
    assert!(!app.launch_client_open);
    assert!(app.chat_proxy.is_some());
}

#[test]
fn a_chat_proxy_for_a_cancelled_launch_is_dropped() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());

    let task = app.update(Message::ChatProxyStarted(
        account.id,
        Ok(ChatProxy::detached(PresenceStatus::Online, false)),
    ));

    assert_eq!(task.units(), 0);
    assert!(app.chat_proxy.is_none());
}

#[test]
fn with_the_chat_proxy_off_a_launch_skips_it_and_drops_the_last_one() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.accounts_loaded = true;
    app.chat_proxy = Some(ChatProxy::detached(PresenceStatus::Online, true));

    let _ = app.update(Message::ChatProxyToggled(false));
    assert!(
        app.chat_proxy.is_some(),
        "a running proxy keeps carrying chat"
    );

    app.unavailable_launch_warning = Some(UnavailableLaunchWarning {
        account_id: account.id,
        display_name: account.display_name.clone(),
        reason: "In a match.".to_string(),
    });
    let task = app.update(Message::LaunchAnyway(account.id));

    assert!(task.units() > 0, "launches");
    assert_eq!(app.launching_account, Some(account.id));
    assert!(app.chat_proxy.is_none());
    assert_ne!(
        app.status.kind,
        StatusKind::Warning,
        "Online needs no warning"
    );
}

#[test]
fn with_the_chat_proxy_off_a_saved_invisible_status_warns_it_does_not_apply() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.state.chat_proxy = false;
    app.state.presence_status = PresenceStatus::Invisible;
    app.unavailable_launch_warning = Some(UnavailableLaunchWarning {
        account_id: account.id,
        display_name: account.display_name.clone(),
        reason: "In a match.".to_string(),
    });

    let task = app.update(Message::LaunchAnyway(account.id));

    assert!(task.units() > 0, "launches anyway");
    assert_eq!(app.status.kind, StatusKind::Warning);
    assert!(app.status.text.contains("online"), "{}", app.status.text);
}

#[test]
fn launch_online_launches_the_account_whose_proxy_failed() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.status_launch_failure = Some(StatusLaunchFailure {
        account_id: account.id,
        display_name: account.display_name.clone(),
        status: PresenceStatus::Invisible,
        error: "offline".to_string(),
    });

    let task = app.update(Message::LaunchOnline(account.id));

    assert!(task.units() > 0);
    assert_eq!(app.launching_account, Some(account.id));
    assert!(app.status_launch_failure.is_none());
}

#[test]
fn escape_cancels_the_status_launch_dialog() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.status_launch_failure = Some(StatusLaunchFailure {
        account_id: account.id,
        display_name: account.display_name.clone(),
        status: PresenceStatus::Invisible,
        error: "offline".to_string(),
    });

    let _ = app.update(Message::EscapePressed);

    assert!(app.status_launch_failure.is_none());
    assert_eq!(app.launching_account, None);
}

#[test]
fn a_failed_launch_stops_its_chat_proxy() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.launching_account = Some(account.id);
    app.chat_proxy = Some(ChatProxy::detached(PresenceStatus::Online, false));

    let _ = app.update(Message::LaunchFinished(Err("boom".to_string())));

    assert!(app.chat_proxy.is_none());
}

/// An app whose selected account was launched through Prime's connected chat proxy.
fn app_with_proxy(dir: &Path) -> (PrimeApp, AccountProfile) {
    let mut app = test_app(dir);
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.chat_proxy = Some(ChatProxy::detached(PresenceStatus::Online, true));
    (app, account)
}

#[test]
fn the_status_control_shows_before_a_launch_while_chat_goes_through_prime() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.accounts_loaded = false;
    assert!(
        !app.status_control_visible(),
        "hidden until accounts.json loads"
    );
    let _ = app.update(Message::PresenceStatusPicked(PresenceStatus::Invisible));
    assert_eq!(app.state.presence_status, PresenceStatus::Online);

    app.accounts_loaded = true;
    assert!(app.status_control_visible());
    let task = app.update(Message::PresenceStatusPicked(PresenceStatus::Invisible));
    assert_eq!(app.state.presence_status, PresenceStatus::Invisible);
    assert_eq!(task.units(), 1, "the pick is saved for the next launch");

    app.status_menu_open = true;
    let _ = app.update(Message::ChatProxyToggled(false));
    assert!(!app.status_control_visible());
    assert!(!app.status_menu_open);
}

#[test]
fn the_status_control_stays_while_a_proxy_runs_after_routing_is_turned_off() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_with_proxy(dir.path());
    app.state.chat_proxy = false;
    assert!(app.status_control_visible());
}

#[test]
fn the_game_is_only_checked_while_a_proxy_runs() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_with_proxy(dir.path());

    assert_eq!(app.update(Message::RiotClientTick).units(), 1);
    assert_eq!(
        app.update(Message::RiotClientTick).units(),
        0,
        "one check at a time"
    );

    app.riot_client_checking = false;
    app.chat_proxy = None;
    assert_eq!(app.update(Message::RiotClientTick).units(), 0);
}

#[test]
fn closing_riot_client_stops_the_chat_proxy() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_with_proxy(dir.path());
    app.riot_client_checking = true;

    let _ = app.update(Message::RiotClientChecked(false));

    assert!(app.chat_proxy.is_none());
    assert!(!app.riot_client_checking);
}

#[test]
fn riot_client_starting_during_a_launch_keeps_the_proxy() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account) = app_with_proxy(dir.path());
    app.launching_account = Some(account.id);

    let _ = app.update(Message::RiotClientChecked(false));

    assert!(app.chat_proxy.is_some());
}

#[test]
fn the_status_menu_opens_only_with_the_control() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_with_proxy(dir.path());

    let _ = app.update(Message::ToggleStatusMenu);
    assert!(app.status_menu_open);
    let _ = app.update(Message::ToggleStatusMenu);
    assert!(!app.status_menu_open);

    app.chat_proxy = None;
    app.state.chat_proxy = false;
    let _ = app.update(Message::ToggleStatusMenu);
    assert!(!app.status_menu_open);
}

#[test]
fn picking_a_status_saves_it_and_applies_it_now() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_with_proxy(dir.path());
    app.status_menu_open = true;

    let task = app.update(Message::PresenceStatusPicked(PresenceStatus::Invisible));

    assert!(task.units() > 0, "saves accounts.json");
    assert_eq!(app.state.presence_status, PresenceStatus::Invisible);
    assert_eq!(
        app.chat_proxy.as_ref().expect("proxy").status(),
        PresenceStatus::Invisible
    );
    assert!(!app.status_menu_open);
}

#[test]
fn picking_the_current_status_saves_nothing() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_with_proxy(dir.path());

    let task = app.update(Message::PresenceStatusPicked(PresenceStatus::Online));

    assert_eq!(task.units(), 0);
}

#[test]
fn escape_and_outside_clicks_close_the_status_menu() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_with_proxy(dir.path());

    app.status_menu_open = true;
    let _ = app.update(Message::EscapePressed);
    assert!(!app.status_menu_open);

    app.status_menu_open = true;
    let _ = app.update(Message::DismissPopovers);
    assert!(!app.status_menu_open);
}

#[test]
fn the_status_menu_closes_when_the_control_hides() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_with_proxy(dir.path());
    app.state.chat_proxy = false;
    app.status_menu_open = true;

    // Riot Client closed, so the proxy goes, and with routing off the control goes with it.
    let _ = app.update(Message::RiotClientChecked(false));

    assert!(!app.status_menu_open);
}
