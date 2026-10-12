use super::*;

#[test]
fn launch_preflight_decision_allows_available_and_unknown_but_warns_unavailable() {
    assert_eq!(
        launch_preflight_decision(&AccountAvailability::Available),
        LaunchPreflightDecision::Launch
    );
    assert_eq!(
        launch_preflight_decision(&AccountAvailability::Unknown {
            reason: "activity check failed".to_string()
        }),
        LaunchPreflightDecision::LaunchInconclusive
    );
    assert_eq!(
        launch_preflight_decision(&AccountAvailability::Unavailable(Busy::InMatch)),
        LaunchPreflightDecision::WarnUnavailable
    );
}

#[test]
fn cancel_unavailable_launch_clears_launch_state() {
    let account_id = AccountId::new();
    let mut warning = Some(UnavailableLaunchWarning {
        account_id,
        display_name: "Main".to_string(),
        reason: "in match".to_string(),
    });
    let mut preflight = Some(account_id);
    let mut launching = Some(account_id);
    let mut progress_checking = true;

    cancel_unavailable_launch_state(
        &mut warning,
        &mut preflight,
        &mut launching,
        &mut progress_checking,
    );

    assert_eq!(warning, None);
    assert_eq!(preflight, None);
    assert_eq!(launching, None);
    assert!(!progress_checking);
}

#[test]
fn account_detail_update_keeps_rank_and_level_when_penalty_fails() {
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    let update = apply_account_detail_results(
        &mut account,
        Ok(Some(CompetitiveRank::new(15, "Platinum 1", 42))),
        Ok(123),
        Err("penalty endpoint unavailable".to_string()),
    );

    assert_eq!(
        update,
        crate::ui::app::AccountDetailUpdate {
            updated: true,
            partial: true
        }
    );
    assert_eq!(
        account
            .competitive_rank
            .as_ref()
            .map(CompetitiveRank::label),
        Some("Platinum 1 - 42 RR".to_string())
    );
    assert_eq!(account.account_level, Some(123));
    assert_eq!(account.penalty_status, AccountPenaltyStatus::Unchecked);
}

#[test]
fn account_export_payload_display_is_partially_masked() {
    let payload = format!("{}{}{}", "a".repeat(18), "b".repeat(30), "c".repeat(18));
    let masked = masked_account_export_payload(&payload);

    assert!(masked.starts_with(&"a".repeat(18)));
    assert!(masked.ends_with(&"c".repeat(18)));
    assert!(masked.contains(&"*".repeat(24)));
    assert!(!masked.contains(&"b".repeat(30)));
}

fn app_with_puuid_account(dir: &Path, puuid: &str) -> PrimeApp {
    let mut app = test_app(dir);
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    account.puuid = Some(puuid.to_string());
    let account_id = account.id;
    app.state.push_account(account);
    app.state.select_account(account_id);
    app
}

#[test]
fn token_import_rejects_a_token_for_another_riot_account() {
    let dir = tempdir().expect("temp dir");
    let mut app = app_with_puuid_account(dir.path(), "puuid-a");
    app.redirect_input = redirect_url_for_subject("puuid-b");

    let _ = app.update(Message::ImportRedirect);

    assert!(
        app.status
            .text
            .starts_with("Could not import redirect token"),
        "{}",
        app.status.text
    );
    assert_eq!(app.state.accounts[0].session, None);
}

#[test]
fn token_import_rejects_a_token_whose_account_cannot_be_read() {
    let dir = tempdir().expect("temp dir");
    let mut app = app_with_puuid_account(dir.path(), "puuid-a");
    app.redirect_input = "https://playvalorant.com/opt_in#access_token=opaque".to_string();

    let _ = app.update(Message::ImportRedirect);

    assert!(
        app.status
            .text
            .starts_with("Could not import redirect token"),
        "{}",
        app.status.text
    );
    assert_eq!(app.state.accounts[0].session, None);
}

#[test]
fn token_import_accepts_the_accounts_own_token() {
    let dir = tempdir().expect("temp dir");
    let mut app = app_with_puuid_account(dir.path(), "puuid-a");
    app.redirect_input = redirect_url_for_subject("puuid-a");

    let _ = app.update(Message::ImportRedirect);

    assert!(
        app.state.accounts[0].session.is_some(),
        "{}",
        app.status.text
    );
    assert!(
        app.status.text.contains(&app.state.accounts[0].summary()),
        "{}",
        app.status.text
    );
}

#[test]
fn refreshing_a_profile_forgets_its_saved_region() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    account.puuid = Some("puuid-a".to_string());
    account.region = Some(ValorantRegion::Na);
    let account_id = account.id;
    app.state.push_account(account);

    let _ = app.update(Message::ProfileIdentityLoaded(
        account_id,
        Ok(crate::ui::data::account_details::RefreshedProfileIdentity {
            account_id,
            session: AuthSession::new("access", None, None, "Bearer", Some(3600), 100),
            launcher_session: None,
            puuid: "puuid-a".to_string(),
            game_name: "Main".to_string(),
            tag_line: "NA1".to_string(),
        }),
    ));

    assert_eq!(app.state.accounts[0].region, None);
}

#[test]
fn selecting_an_account_refreshes_it_without_a_full_reload() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let main = AccountProfile::new("Main", Shard::Na).expect("main");
    let alt = AccountProfile::new("Alt", Shard::Na).expect("alt");
    app.state.push_account(main);
    app.state.push_account(alt.clone());
    app.client_version_input = "release-1".to_string();
    // A shop-reset toast for the account left behind would otherwise stay.
    app.status = Status::progress("Shop reset reached; loading updated shop");

    let _ = app.update(Message::SelectAccount(alt.id));

    assert!(!app.account_ranks_loading.is_empty());
    assert!(app.account_availability_loading);
    // The page shows the selected account, so no toast says so.
    assert_eq!(app.status, Status::default());
}

#[test]
fn refresh_profile_result_is_not_replaced_by_a_details_reload() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account) = accounts_tab_app(dir.path());

    let _ = app.update(Message::ProfileIdentityLoaded(
        account.id,
        Ok(crate::ui::data::account_details::RefreshedProfileIdentity {
            account_id: account.id,
            session: AuthSession::new("access", None, None, "Bearer", Some(3600), 100),
            launcher_session: None,
            puuid: "puuid-a".to_string(),
            game_name: "Player".to_string(),
            tag_line: "NA1".to_string(),
        }),
    ));

    assert!(
        app.status.text.starts_with("Refreshed "),
        "{}",
        app.status.text
    );
}

#[test]
fn profile_refreshes_are_tracked_per_account() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, alt) = two_account_app(dir.path());

    let _ = app.update(Message::RefreshProfileIdentity(main.id));
    let _ = app.update(Message::RefreshProfileIdentity(alt.id));
    let _ = app.update(Message::ProfileIdentityLoaded(
        main.id,
        Err("offline".to_string()),
    ));

    assert!(!app.profile_identity_refreshing.contains(&main.id));
    assert!(app.profile_identity_refreshing.contains(&alt.id));
}

#[test]
fn a_profile_refresh_is_not_started_twice() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, _) = two_account_app(dir.path());
    let _ = app.update(Message::RefreshProfileIdentity(main.id));

    let task = app.update(Message::RefreshProfileIdentity(main.id));

    assert_eq!(task.units(), 0);
    assert!(app.profile_identity_refreshing.contains(&main.id));
}

#[test]
fn import_result_is_not_replaced_by_a_details_reload() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = accounts_tab_app(dir.path());
    let imported = AccountProfile::new("Alt", Shard::Na).expect("alt");

    let _ = app.update(Message::AccountImported(Ok(
        crate::account_transfer::ImportedAccount {
            original_id: AccountId::new(),
            account: imported,
            id_changed: true,
            imported_launcher_file_count: 1,
        },
    )));

    assert!(
        app.status.text.ends_with("with a new local ID"),
        "{}",
        app.status.text
    );
}

#[test]
fn background_account_details_leave_the_status_alone() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = accounts_tab_app(dir.path());
    app.status = Status::success("Added Main");

    let _ = app.update(Message::AccountRanksLoaded {
        result: Default::default(),
        announce: false,
    });

    assert_eq!(app.status, Status::success("Added Main"));
}

#[test]
fn opening_accounts_during_a_launch_keeps_the_launch_progress() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account) = accounts_tab_app(dir.path());
    app.active_tab = crate::ui::Tab::Settings;
    app.launching_account = Some(account.id);
    app.status = Status::info("Riot Client is open; waiting for VALORANT");

    let _ = app.update(Message::TabSelected(crate::ui::Tab::Accounts));

    assert_eq!(
        app.status,
        Status::info("Riot Client is open; waiting for VALORANT")
    );
}

#[test]
fn opening_accounts_loads_details_and_availability_with_one_sign_in() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account) = accounts_tab_app(dir.path());
    app.active_tab = crate::ui::Tab::Settings;

    let task = app.update(Message::TabSelected(crate::ui::Tab::Accounts));

    // Restoring the scroll position, and one load for details and availability together.
    assert_eq!(task.units(), 2);
    assert!(!app.account_ranks_loading.is_empty());
    assert!(app.account_availability_loading);

    let _ = app.update(Message::AccountRanksLoaded {
        result: crate::ui::data::account_details::AccountRanksResult {
            availability: Some(vec![
                crate::ui::data::account_details::AccountActivityCheck {
                    account_id: account.id,
                    availability: AccountAvailability::Available,
                },
            ]),
            ..Default::default()
        },
        announce: true,
    });

    assert!(!app.account_availability_loading);
    assert_eq!(
        app.account_availability.get(&account.id),
        Some(&AccountAvailability::Available)
    );
}

#[test]
fn reopening_accounts_soon_after_a_load_does_not_refetch() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = accounts_tab_app(dir.path());
    app.active_tab = crate::ui::Tab::Settings;

    let _ = app.update(Message::TabSelected(crate::ui::Tab::Accounts));
    assert!(!app.account_ranks_loading.is_empty());
    let _ = app.update(Message::AccountRanksLoaded {
        result: Default::default(),
        announce: true,
    });
    let _ = app.update(Message::AccountAvailabilitiesLoaded(Default::default()));
    let _ = app.update(Message::TabSelected(crate::ui::Tab::Settings));
    let _ = app.update(Message::TabSelected(crate::ui::Tab::Accounts));

    assert!(app.account_ranks_loading.is_empty());
    assert!(!app.account_availability_loading);
}

fn rank_result(account_id: AccountId, rank: Result<Option<CompetitiveRank>, String>) -> Message {
    Message::AccountRanksLoaded {
        result: crate::ui::data::account_details::AccountRanksResult {
            ranks: vec![crate::ui::data::account_details::AccountRankResult {
                account_id,
                rank,
                account_level: Ok(20),
                penalty_status: Ok(AccountPenaltyStatus::default()),
                session: AuthSession::new("fresh", None, None, "Bearer", Some(3600), 100),
                launcher_session: None,
                identity: ApiIdentity {
                    puuid: "Main-puuid".to_string(),
                    game_name: None,
                    tag_line: None,
                    shard: Shard::Na,
                    region: None,
                },
            }],
            ..Default::default()
        },
        announce: false,
    }
}

#[test]
fn only_the_accounts_being_loaded_show_loading() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, alt) = two_account_app(dir.path());

    let _ = app.update(Message::SelectAccount(alt.id));

    assert!(app.account_ranks_loading.contains(&alt.id));
    assert!(!app.account_ranks_loading.contains(&main.id));
}

#[test]
fn an_account_with_no_rank_reads_unranked_and_a_failed_one_unavailable() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, alt) = two_account_app(dir.path());

    let _ = app.update(rank_result(main.id, Ok(None)));
    let _ = app.update(rank_result(alt.id, Err("offline".to_string())));

    assert_eq!(
        crate::ui::screens::missing_rank_label(&app, main.id),
        "Unranked"
    );
    assert_eq!(
        crate::ui::screens::missing_rank_label(&app, alt.id),
        "Rank unavailable"
    );
    // The failure's reason is kept for the label's tooltip, and cleared once the rank loads.
    assert_eq!(
        app.rank_errors.get(&alt.id).map(String::as_str),
        Some("offline")
    );
    assert!(!app.rank_errors.contains_key(&main.id));

    let _ = app.update(rank_result(alt.id, Ok(None)));
    assert!(!app.rank_errors.contains_key(&alt.id));
}

#[test]
fn export_account_runs_once_at_a_time() {
    let dir = tempdir().expect("temp dir");
    let (mut app, first, _) = two_account_app(dir.path());

    assert_eq!(
        app.update(Message::RequestExportAccount(first.id)).units(),
        1
    );
    assert!(app.account_export_in_progress);
    assert_eq!(
        app.update(Message::RequestExportAccount(first.id)).units(),
        0
    );

    let _ = app.update(Message::AccountExportPrepared(Err("failed".to_string())));
    assert!(!app.account_export_in_progress);
}
