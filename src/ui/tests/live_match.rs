use super::*;

fn live_match_app(dir: &Path) -> (PrimeApp, AccountProfile, AccountProfile) {
    let (mut app, main, alt) = two_account_app(dir);
    app.image_cache = crate::image_cache::ImageCache::new(dir.join("cache"));
    app.state.accounts[0].puuid = Some("puuid-main".to_string());
    app.state.accounts[1].puuid = Some("puuid-alt".to_string());
    (app, main, alt)
}

fn live_snapshot(account_id: AccountId) -> LiveMatch {
    LiveMatch {
        account_id,
        match_id: "match".to_string(),
        phase: MatchPhase::InProgress,
        map: Some("Ascent".to_string()),
        map_art: None,
        mode: Some("Competitive".to_string()),
        server: Some("Ashburn".to_string()),
        queue_id: Some("competitive".to_string()),
        score: Ok(crate::riot::local_client::MatchScore { ally: 5, enemy: 2 }),
        allies: Vec::new(),
        enemies: Vec::new(),
        weapons: shown_weapons(None),
        loadouts_loaded: true,
        hidden_names_error: None,
    }
}

fn live_result(
    account_id: AccountId,
    activity: AccountActivity,
    refreshed: Option<RefreshedApiContext>,
) -> LiveMatchResult {
    LiveMatchResult {
        account_id,
        live: (activity == AccountActivity::InMatch).then(|| live_snapshot(account_id)),
        activity,
        refreshed,
    }
}

fn refreshed_context(account_id: AccountId, puuid: &str) -> RefreshedApiContext {
    RefreshedApiContext {
        account_id,
        session: AuthSession::new("fresh-access", None, None, "Bearer", Some(3600), 100),
        launcher_session: None,
        identity: ApiIdentity {
            puuid: puuid.to_string(),
            game_name: None,
            tag_line: None,
            shard: Shard::Na,
            region: None,
        },
    }
}

fn live_tick() -> Message {
    Message::AccountAvailabilityTimerTick(iced::time::Instant::now())
}

#[test]
fn opening_live_match_loads_the_selected_account() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, _) = live_match_app(dir.path());

    let task = app.update(Message::TabSelected(crate::ui::Tab::LiveMatch));

    assert_eq!(app.live_match_request.map(|r| r.account_id), Some(main.id));
    assert!(task.units() >= 1);
}

#[test]
fn live_match_polls_only_while_open_and_visible() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = live_match_app(dir.path());
    app.state.minimize_on_close = false;
    app.active_tab = crate::ui::Tab::LiveMatch;

    let task = app.update(live_tick());
    assert!(app.live_match_request.is_some());
    assert_eq!(task.units(), 1);
    // Only the selected account, not every account's availability.
    assert!(!app.account_availability_loading);

    app.live_match_request = None;
    app.window_minimized = true;
    assert_eq!(app.update(live_tick()).units(), 0);
    assert_eq!(app.live_match_request, None);

    app.window_minimized = false;
    app.active_tab = crate::ui::Tab::Shop;
    assert_eq!(app.update(live_tick()).units(), 0);
    assert_eq!(app.live_match_request, None);
}

#[test]
fn live_match_and_availability_polls_never_overlap() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = live_match_app(dir.path());
    app.active_tab = crate::ui::Tab::LiveMatch;
    app.account_availability_loading = true;

    assert_eq!(app.update(live_tick()).units(), 0);
    assert_eq!(app.live_match_request, None);

    app.account_availability_loading = false;
    let _ = app.update(live_tick());
    assert!(app.live_match_request.is_some());

    app.active_tab = crate::ui::Tab::Accounts;
    assert_eq!(app.update(live_tick()).units(), 0);
    assert!(!app.account_availability_loading);
}

#[test]
fn a_live_match_opened_during_an_availability_poll_loads_when_it_ends() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = live_match_app(dir.path());
    app.account_availability_loading = true;
    let _ = app.update(Message::TabSelected(crate::ui::Tab::LiveMatch));
    assert_eq!(app.live_match_request, None);

    let _ = app.update(Message::AccountAvailabilitiesLoaded(
        AccountAvailabilityRefresh::default(),
    ));

    assert!(app.live_match_request.is_some());
}

#[test]
fn a_sign_in_failure_stops_live_match_polling() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = live_match_app(dir.path());
    let _ = app.update(Message::TabSelected(crate::ui::Tab::LiveMatch));
    let request = app.live_match_request.expect("request").id;

    let _ = app.update(Message::LiveMatchLoaded(
        request,
        Err(LiveMatchError::SignIn("signed out".to_string())),
    ));

    assert_eq!(app.update(live_tick()).units(), 0);
    assert_eq!(app.live_match_request, None);

    let task = app.update(Message::RetryLiveMatch);
    assert_eq!(task.units(), 1);
    assert!(app.live_match_request.is_some());
}

#[test]
fn a_live_match_reply_for_an_old_request_only_keeps_its_session() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, _) = live_match_app(dir.path());
    let _ = app.update(Message::TabSelected(crate::ui::Tab::LiveMatch));
    let first = app.live_match_request.expect("request").id;
    app.live_match_request = None;
    app.live_match_in_flight = false;
    let _ = app.update(live_tick());
    let latest = app.live_match_request.expect("latest request");

    let task = app.update(Message::LiveMatchLoaded(
        first,
        Ok(live_result(
            main.id,
            AccountActivity::InMatch,
            Some(refreshed_context(main.id, "puuid-main")),
        )),
    ));

    assert_eq!(app.live_match_request, Some(latest));
    assert_eq!(app.live_match, None);
    assert_eq!(
        app.state.accounts[0]
            .session
            .as_ref()
            .map(|session| session.access_token.as_str()),
        Some("fresh-access")
    );
    assert_eq!(task.units(), 1, "saves the session");
}

#[test]
fn a_live_match_reply_updates_the_accounts_availability() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, _) = live_match_app(dir.path());
    let _ = app.update(Message::TabSelected(crate::ui::Tab::LiveMatch));
    let request = app.live_match_request.expect("request").id;

    let _ = app.update(Message::LiveMatchLoaded(
        request,
        Ok(live_result(main.id, AccountActivity::InMatch, None)),
    ));

    assert_eq!(
        app.account_availability.get(&main.id),
        Some(&AccountAvailability::Unavailable(Busy::InMatch))
    );
    assert!(app.fresh_availability(main.id).is_some());
    assert_eq!(app.live_match, Some(live_snapshot(main.id)));
    assert_eq!(app.live_match_request, None);
}

#[test]
fn a_live_match_loaded_for_columns_changed_since_loads_again() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, _) = live_match_app(dir.path());
    let _ = app.update(Message::TabSelected(crate::ui::Tab::LiveMatch));
    let request = app.live_match_request.expect("request").id;
    // Picked in Settings while the load ran.
    app.state.live_match_weapons = Some(
        ["1baa85b4-4c70-1284-64bb-6481dfc3bb4e"]
            .into_iter()
            .chain(shown_weapons(None).iter().skip(1).map(String::as_str))
            .map(str::to_string)
            .collect(),
    );

    let _ = app.update(Message::LiveMatchLoaded(
        request,
        Ok(live_result(main.id, AccountActivity::InMatch, None)),
    ));

    assert!(app.live_match.is_some(), "the loaded match shows meanwhile");
    assert!(app.live_match_request.is_some(), "and loads again at once");
}

#[test]
fn a_live_match_session_for_another_puuid_is_not_saved() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, _) = live_match_app(dir.path());
    let _ = app.update(Message::TabSelected(crate::ui::Tab::LiveMatch));
    let request = app.live_match_request.expect("request").id;

    let task = app.update(Message::LiveMatchLoaded(
        request,
        Ok(live_result(
            main.id,
            AccountActivity::Available,
            Some(refreshed_context(main.id, "puuid-alt")),
        )),
    ));

    assert_eq!(app.state.accounts[0].session, None);
    assert_eq!(task.units(), 0);
}

#[test]
fn switching_accounts_clears_the_live_match() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, alt) = live_match_app(dir.path());
    let _ = app.update(Message::TabSelected(crate::ui::Tab::LiveMatch));
    let request = app.live_match_request.expect("request").id;
    let _ = app.update(Message::LiveMatchLoaded(
        request,
        Ok(live_result(main.id, AccountActivity::InMatch, None)),
    ));
    app.live_match_error = Some(LiveMatchError::Request("old".to_string()));

    let _ = app.update(Message::SelectAccount(alt.id));

    assert_eq!(app.live_match, None);
    assert_eq!(app.live_match_error, None);
    assert_eq!(app.live_match_request.map(|r| r.account_id), Some(alt.id));
}

#[test]
fn hidden_details_stay_hidden_until_toggled() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    assert!(!app.show_hidden_details);

    let _ = app.update(Message::HiddenDetailsToggled);
    assert!(app.show_hidden_details);

    let _ = app.update(Message::HiddenDetailsToggled);
    assert!(!app.show_hidden_details);
}

#[test]
fn showing_hidden_details_loads_hidden_names_now() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = live_match_app(dir.path());
    let _ = app.update(Message::TabSelected(crate::ui::Tab::LiveMatch));
    app.live_match_request = None;
    app.live_match_in_flight = false;

    assert_eq!(app.update(Message::HiddenDetailsToggled).units(), 1);
    assert!(app.live_match_request.is_some());

    app.live_match_request = None;
    app.live_match_in_flight = false;
    assert_eq!(app.update(Message::HiddenDetailsToggled).units(), 0);
}

#[test]
fn a_failed_update_keeps_the_last_match_on_screen() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, _) = live_match_app(dir.path());
    let _ = app.update(Message::TabSelected(crate::ui::Tab::LiveMatch));
    let request = app.live_match_request.expect("request").id;
    let _ = app.update(Message::LiveMatchLoaded(
        request,
        Ok(live_result(main.id, AccountActivity::InMatch, None)),
    ));
    let _ = app.update(live_tick());
    let request = app.live_match_request.expect("poll").id;

    let _ = app.update(Message::LiveMatchLoaded(
        request,
        Err(LiveMatchError::Request("timed out".to_string())),
    ));

    assert_eq!(app.live_match, Some(live_snapshot(main.id)));
    assert_eq!(app.status.kind, StatusKind::Error);
    assert!(app.status.text.contains("timed out"), "{}", app.status.text);
    // A request failure is tried again on the next poll.
    let _ = app.update(live_tick());
    assert!(app.live_match_request.is_some());
}

#[test]
fn the_live_match_indicator_shows_only_in_agent_select_or_a_match() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, alt) = live_match_app(dir.path());
    let set = |app: &mut PrimeApp, availability| {
        app.account_availability.insert(main.id, availability);
    };

    set(&mut app, AccountAvailability::Unavailable(Busy::InMatch));
    assert_eq!(
        app.live_match_indicator(),
        Some(("IN A MATCH", "View match".to_string()))
    );
    set(
        &mut app,
        AccountAvailability::Unavailable(Busy::AgentSelect),
    );
    assert_eq!(
        app.live_match_indicator(),
        Some(("AGENT SELECT", "View match".to_string()))
    );
    for availability in [
        AccountAvailability::Unavailable(Busy::InLobby),
        AccountAvailability::Available,
        AccountAvailability::not_checked(),
    ] {
        set(&mut app, availability);
        assert_eq!(app.live_match_indicator(), None);
    }

    set(&mut app, AccountAvailability::Unavailable(Busy::InMatch));
    app.live_match = Some(live_snapshot(main.id));
    assert_eq!(
        app.live_match_indicator(),
        Some(("IN A MATCH", "Ascent · 5 – 2".to_string()))
    );
    app.live_match.as_mut().expect("match").score =
        Err(crate::riot::local_client::ScoreUnavailable::NotFound);
    assert_eq!(
        app.live_match_indicator(),
        Some(("IN A MATCH", "Ascent".to_string()))
    );

    // Another account's match is left out, as is another account being in one.
    app.live_match = Some(live_snapshot(alt.id));
    assert_eq!(
        app.live_match_indicator(),
        Some(("IN A MATCH", "View match".to_string()))
    );
    app.account_availability
        .insert(alt.id, AccountAvailability::Unavailable(Busy::InMatch));
    app.state.selected_account = None;
    assert_eq!(app.live_match_indicator(), None);
}

#[test]
fn the_live_match_page_fills_the_page_for_states() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, _) = live_match_app(dir.path());
    let fills = |app: &PrimeApp| crate::ui::screens::fills_page(app, crate::ui::Tab::LiveMatch);

    assert!(fills(&app), "not in a match");
    app.live_match_error = Some(LiveMatchError::Request("boom".to_string()));
    assert!(fills(&app), "error");

    app.live_match = Some(live_snapshot(main.id));
    assert!(!fills(&app), "a match with an update error");
    app.live_match_error = Some(LiveMatchError::SignIn("signed out".to_string()));
    assert!(fills(&app), "sign-in error");
}

#[test]
fn a_live_match_load_left_by_an_account_switch_blocks_sign_ins_until_it_answers() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, alt) = live_match_app(dir.path());
    let _ = app.update(Message::TabSelected(crate::ui::Tab::LiveMatch));
    let first = app.live_match_request.expect("request").id;

    // The old account's load still signs in, so neither the new one nor an availability poll
    // starts yet.
    let _ = app.update(Message::SelectAccount(alt.id));
    assert_eq!(app.live_match_request, None);
    assert!(app.live_match_in_flight);

    let task = app.update(Message::LiveMatchLoaded(
        first,
        Err(LiveMatchError::Request("offline".to_string())),
    ));
    assert_eq!(task.units(), 1);
    assert!(app.live_match_request.is_some());
    assert_eq!(app.live_match_error, None);
}
