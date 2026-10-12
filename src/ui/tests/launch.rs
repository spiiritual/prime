use super::*;

fn staged_recapture(backup_root: &Path, puuid: &str, settings: &str) -> CapturedLauncherSession {
    let staging_id = AccountId::new();
    let data_dir = backup_root.join(staging_id.to_string()).join("Data");
    fs::create_dir_all(&data_dir).expect("staging data dir");
    fs::write(data_dir.join("RiotGamesPrivateSettings.yaml"), settings).expect("settings");
    CapturedLauncherSession {
        account_id: staging_id,
        backup: LauncherSessionBackup {
            data_dir,
            captured_at_unix: 200,
            puuid: puuid.to_string(),
            unknown: Default::default(),
        },
    }
}

#[test]
fn cancelling_a_login_capture_stops_waiting_and_removes_its_slot() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let partial = staged_recapture(&backup_root, "puuid", "partial");
    let slot = backup_root.join(partial.account_id.to_string());
    let handle = waiting_login_capture(
        &mut app,
        crate::ui::LoginCaptureTarget::NewAccount(partial.account_id),
    );

    let _ = app.update(Message::CancelLoginCapture);

    assert!(handle.is_aborted());
    assert!(!app.launcher_capture_in_progress);
    assert_eq!(app.launcher_capture_kind, None);
    assert!(app.login_capture.is_none());
    assert!(!slot.exists());
    assert!(
        app.status.text.starts_with("Canceled login capture"),
        "{}",
        app.status.text
    );
}

#[test]
fn a_capture_cannot_be_cancelled_while_riot_client_reopens() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    preparing_login_capture(
        &mut app,
        crate::ui::LoginCaptureTarget::NewAccount(AccountId::new()),
    );

    let _ = app.update(Message::CancelLoginCapture);

    assert!(app.launcher_capture_in_progress);
    assert!(app.login_capture.is_some());
}

#[test]
fn a_capture_result_after_cancelling_is_discarded() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );
    let slot = app
        .repo
        .launcher_backups_dir()
        .join(draft.account_id.to_string());

    let _ = app.update(Message::AccountCaptureFinished(Ok(draft)));

    assert_eq!(app.pending_account, None);
    assert!(!slot.exists());
}

#[test]
fn a_capture_saves_the_signed_in_accounts_login_before_waiting() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let previous = account_with_backup(&backup_root, "Previous", "settings");
    let mut refreshed = previous.launcher_session.clone().expect("backup");
    refreshed.captured_at_unix = 500;
    app.state.push_account(previous.clone());
    let target = crate::ui::LoginCaptureTarget::NewAccount(AccountId::new());
    preparing_login_capture(&mut app, target);

    let task = app.update(Message::LoginCapturePrepared {
        target,
        result: Ok(Ok(Some((previous.id, refreshed.clone())))),
    });

    assert_eq!(
        app.state.accounts[0].launcher_session.as_ref(),
        Some(&refreshed)
    );
    assert!(
        app.login_capture
            .as_ref()
            .is_some_and(|capture| capture.wait.is_some())
    );
    assert!(task.units() > 0);
}

#[test]
fn a_capture_warns_when_the_signed_in_login_could_not_be_saved() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let target = crate::ui::LoginCaptureTarget::NewAccount(AccountId::new());
    preparing_login_capture(&mut app, target);

    let _ = app.update(Message::LoginCapturePrepared {
        target,
        result: Ok(Err("disk full".to_string())),
    });

    assert!(app.launcher_capture_in_progress);
    assert!(app.status.text.contains("disk full"), "{}", app.status.text);
    assert!(
        (app.status.kind == StatusKind::Error),
        "{}",
        app.status.text
    );
}

#[test]
fn a_capture_that_cannot_open_riot_client_ends_with_the_error() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let target = crate::ui::LoginCaptureTarget::NewAccount(AccountId::new());
    preparing_login_capture(&mut app, target);

    let _ = app.update(Message::LoginCapturePrepared {
        target,
        result: Err("Riot Client was not found".to_string()),
    });

    assert!(!app.launcher_capture_in_progress);
    assert_eq!(app.launcher_capture_kind, None);
    assert!(app.login_capture.is_none());
    assert_eq!(
        app.status.text,
        "Could not add account: Riot Client was not found"
    );
}

#[test]
fn starting_another_capture_discards_the_pending_draft_backup() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let draft = captured_account_draft(&backup_root, "puuid", "Main", "NA1", Shard::Na);
    let draft_slot = backup_root.join(draft.account_id.to_string());
    assert!(draft_slot.exists());
    app.pending_account = Some(draft);

    let _ = app.update(Message::AddCurrentAccount);

    assert!(!draft_slot.exists());
}

#[test]
fn login_capture_is_refused_while_an_account_is_launching() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());
    app.launching_account = Some(account.id);

    for message in [
        Message::AddAccount,
        Message::AddCurrentAccount,
        Message::ConfirmAddAccountCapture,
        Message::RequestLauncherSessionLogin(account.id),
        Message::StartLauncherSessionLogin(account.id),
    ] {
        let _ = app.update(message);

        assert!(!app.launcher_capture_in_progress);
        assert!(!app.show_add_account_prompt);
        assert_eq!(app.confirm_recapture_account, None);
        assert!(app.status.text.contains("launching"), "{}", app.status.text);
    }
}

#[test]
fn launch_is_refused_while_a_login_capture_runs() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());
    app.launcher_capture_in_progress = true;

    let _ = app.update(Message::LaunchAccount(account.id));

    assert_eq!(app.launching_account, None);
    assert_eq!(app.launch_preflight_account, None);
    assert!(
        app.status.text.contains("login capture"),
        "{}",
        app.status.text
    );
}

#[test]
fn launch_without_a_captured_login_is_refused_before_switching() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let selected = account_with_backup(&backup_root, "Main", "settings");
    let uncaptured = AccountProfile::new("Alt", Shard::Na).expect("alt");
    app.state.push_account(selected.clone());
    app.state.push_account(uncaptured.clone());
    app.state.select_account(selected.id);
    app.store_summary = Some(empty_store_summary());

    let task = app.update(Message::LaunchAccount(uncaptured.id));

    assert_eq!(task.units(), 0);
    assert_eq!(app.launch_preflight_account, None);
    assert_eq!(app.state.selected_account, Some(selected.id));
    assert!(app.store_summary.is_some());
    assert!(
        app.status.text.starts_with("Could not launch Alt"),
        "{}",
        app.status.text
    );
    assert!(
        app.status.text.contains("Re-capture login"),
        "{}",
        app.status.text
    );
}

fn finished_launch(previous_account_backup: Option<(AccountId, LauncherSessionBackup)>) -> Message {
    Message::LaunchFinished(Ok(LaunchAccountResult {
        previous_account_backup,
        previous_account_sync_warning: None,
        synced_backup: None,
        sync_warning: None,
    }))
}

#[test]
fn finished_launch_keeps_the_previous_account_login() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let previous = account_with_backup(&backup_root, "Previous", "settings");
    let mut refreshed = previous.launcher_session.clone().expect("backup");
    refreshed.captured_at_unix = 500;
    app.state.push_account(previous.clone());

    let _ = app.update(finished_launch(Some((previous.id, refreshed.clone()))));

    assert_eq!(
        app.state.accounts[0].launcher_session.as_ref(),
        Some(&refreshed)
    );
}

#[test]
fn finished_launch_ignores_a_previous_login_for_a_replaced_slot() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let previous = account_with_backup(&backup_root, "Previous", "settings");
    let current = previous.launcher_session.clone().expect("backup");
    let stale = LauncherSessionBackup {
        data_dir: backup_root.join("replaced").join("Data"),
        captured_at_unix: 500,
        puuid: current.puuid.clone(),
        unknown: Default::default(),
    };
    app.state.push_account(previous.clone());

    let _ = app.update(finished_launch(Some((previous.id, stale))));

    assert_eq!(
        app.state.accounts[0].launcher_session.as_ref(),
        Some(&current)
    );
}

#[test]
fn a_finished_launch_check_leaves_a_dialog_opened_meanwhile() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());
    app.launch_preflight_account = Some(account.id);
    let _ = app.update(Message::OpenImportAccount);
    let _ = app.update(Message::ImportAccountInputChanged("export".to_string()));

    let _ = app.update(Message::LaunchPreflightChecked(
        crate::ui::data::account_details::AccountActivityCheck {
            account_id: account.id,
            availability: AccountAvailability::Available,
        },
        false,
    ));

    assert_eq!(app.launching_account, Some(account.id));
    assert!(app.show_import_account_prompt);
    assert_eq!(app.import_account_input, "export");
}

#[test]
fn closing_the_import_prompt_forgets_the_pasted_export() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());
    let _ = app.update(Message::OpenImportAccount);
    let _ = app.update(Message::ImportAccountInputChanged("export".to_string()));

    let _ = app.update(Message::RequestDeleteAccount(account.id));
    let _ = app.update(Message::CancelDeleteAccount);
    let _ = app.update(Message::OpenImportAccount);

    assert_eq!(app.import_account_input, "");
}

#[test]
fn launch_warns_before_closing_a_running_game() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());
    app.launch_preflight_account = Some(account.id);

    let _ = app.update(Message::LaunchPreflightChecked(
        crate::ui::data::account_details::AccountActivityCheck {
            account_id: account.id,
            availability: AccountAvailability::Available,
        },
        true,
    ));

    let warning = app.unavailable_launch_warning.as_ref().expect("warning");
    assert!(
        warning.reason.contains("already running"),
        "{}",
        warning.reason
    );
    assert_eq!(app.launching_account, None);
}

#[test]
fn a_dialog_opening_closes_the_popovers() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());
    app.launch_preflight_account = Some(account.id);
    app.account_switcher_open = true;
    app.open_account_menu = Some(account.id);

    let _ = app.update(Message::LaunchPreflightChecked(
        crate::ui::data::account_details::AccountActivityCheck {
            account_id: account.id,
            availability: AccountAvailability::Available,
        },
        true,
    ));

    assert!(app.unavailable_launch_warning.is_some());
    assert!(!app.account_switcher_open);
    assert_eq!(app.open_account_menu, None);
}

#[test]
fn clicking_outside_a_popover_closes_it() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());
    app.account_switcher_open = true;
    app.open_account_menu = Some(account.id);

    let _ = app.update(Message::DismissPopovers);

    assert!(!app.account_switcher_open);
    assert_eq!(app.open_account_menu, None);
}

#[test]
fn launch_starts_when_no_game_is_running() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    let backup_root = app.repo.launcher_backups_dir();
    account = {
        let with_backup = account_with_backup(&backup_root, "Main", "settings");
        AccountProfile {
            id: account.id,
            ..with_backup
        }
    };
    app.state.push_account(account.clone());
    app.launch_preflight_account = Some(account.id);

    let _ = app.update(Message::LaunchPreflightChecked(
        crate::ui::data::account_details::AccountActivityCheck {
            account_id: account.id,
            availability: AccountAvailability::Available,
        },
        false,
    ));

    assert!(app.unavailable_launch_warning.is_none());
    assert_eq!(app.launching_account, Some(account.id));
}

#[test]
fn recapture_asks_for_confirmation_before_starting() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());

    let _ = app.update(Message::RequestLauncherSessionLogin(account.id));

    assert_eq!(app.confirm_recapture_account, Some(account.id));
    assert!(!app.launcher_capture_in_progress);

    let _ = app.update(Message::CancelLauncherSessionLogin);

    assert_eq!(app.confirm_recapture_account, None);
    assert!(!app.launcher_capture_in_progress);
}

#[test]
fn add_account_prompt_warns_when_valorant_is_running() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());

    let task = app.update(Message::AddAccount);

    assert!(app.show_add_account_prompt);
    assert!(
        task.units() > 0,
        "opening the prompt checks for a running game"
    );

    let _ = app.update(Message::CapturePromptGameChecked(true));
    assert!(app.capture_prompt_valorant_running);

    let _ = app.update(Message::CancelAddAccountCapture);
    assert!(!app.capture_prompt_valorant_running);
}

#[test]
fn recapture_prompt_warns_when_valorant_is_running() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());

    let task = app.update(Message::RequestLauncherSessionLogin(account.id));
    assert!(
        task.units() > 0,
        "opening the prompt checks for a running game"
    );

    let _ = app.update(Message::CapturePromptGameChecked(true));
    assert!(app.capture_prompt_valorant_running);

    let _ = app.update(Message::CancelLauncherSessionLogin);
    assert!(!app.capture_prompt_valorant_running);
}

#[test]
fn a_late_game_check_does_not_warn_once_the_prompt_closed() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());

    let _ = app.update(Message::AddAccount);
    let _ = app.update(Message::CancelAddAccountCapture);
    let _ = app.update(Message::CapturePromptGameChecked(true));

    assert!(!app.capture_prompt_valorant_running);
}

#[test]
fn recapture_as_a_different_account_keeps_the_existing_backup() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let account = account_with_backup(&backup_root, "Main", "original");
    let original_backup = account.launcher_session.clone();
    app.state.push_account(account.clone());
    let captured = staged_recapture(&backup_root, "someone-else", "other login");
    let staging_slot = backup_root.join(captured.account_id.to_string());
    waiting_login_capture(
        &mut app,
        crate::ui::LoginCaptureTarget::Existing {
            account_id: account.id,
            staging_id: captured.account_id,
        },
    );

    let _ = app.update(Message::LauncherSessionLoginStarted(
        account.id,
        Ok(captured),
    ));

    let saved = &app.state.accounts[0];
    assert_eq!(saved.launcher_session, original_backup);
    assert_eq!(
        fs::read_to_string(
            original_backup
                .expect("backup")
                .data_dir
                .join("RiotGamesPrivateSettings.yaml")
        )
        .expect("original settings"),
        "original"
    );
    assert!(!staging_slot.exists());
    assert!(
        app.status.text.contains("different Riot account"),
        "{}",
        app.status.text
    );
}

#[test]
fn a_recapture_as_another_account_stays_on_screen_and_brings_prime_forward() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let account = account_with_backup(&backup_root, "Main", "original");
    app.state.push_account(account.clone());
    let captured = staged_recapture(&backup_root, "someone-else", "other login");
    waiting_login_capture(
        &mut app,
        crate::ui::LoginCaptureTarget::Existing {
            account_id: account.id,
            staging_id: captured.account_id,
        },
    );

    let task = app.update(Message::LauncherSessionLoginStarted(
        account.id,
        Ok(captured),
    ));

    assert!(
        (app.status.kind == StatusKind::Error),
        "{}",
        app.status.text
    );
    assert!(task.units() > 0);
}

#[test]
fn a_launch_that_could_not_save_the_previous_login_stays_on_screen() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());

    let _ = app.update(Message::LaunchFinished(Ok(LaunchAccountResult {
        previous_account_backup: None,
        previous_account_sync_warning: Some("file locked".to_string()),
        synced_backup: None,
        sync_warning: None,
    })));

    assert!(
        app.status.text.contains("could not be saved"),
        "{}",
        app.status.text
    );
    assert!(
        (app.status.kind == StatusKind::Error),
        "{}",
        app.status.text
    );
}

#[test]
fn recapture_as_the_same_account_replaces_the_backup() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let account = account_with_backup(&backup_root, "Main", "original");
    app.state.push_account(account.clone());
    let captured = staged_recapture(&backup_root, "Main-puuid", "fresh login");
    let staging_slot = backup_root.join(captured.account_id.to_string());
    waiting_login_capture(
        &mut app,
        crate::ui::LoginCaptureTarget::Existing {
            account_id: account.id,
            staging_id: captured.account_id,
        },
    );

    let _ = app.update(Message::LauncherSessionLoginStarted(
        account.id,
        Ok(captured),
    ));

    let backup = app.state.accounts[0]
        .launcher_session
        .clone()
        .expect("backup");
    assert_eq!(
        backup.data_dir,
        backup_root.join(account.id.to_string()).join("Data")
    );
    assert_eq!(backup.captured_at_unix, 200);
    assert_eq!(
        fs::read_to_string(backup.data_dir.join("RiotGamesPrivateSettings.yaml"))
            .expect("fresh settings"),
        "fresh login"
    );
    assert!(!staging_slot.exists());
}

#[test]
fn accounts_are_not_saved_before_a_successful_load() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.accounts_loaded = false;

    assert!(app.state_to_save().is_err());

    let _ = app.update(Message::Loaded(Err("accounts.json is invalid".to_string())));

    assert!(app.state_to_save().is_err());
    assert!(!app.accounts_loaded);
}

#[test]
fn accounts_are_saved_after_a_successful_load() {
    let dir = tempdir().expect("temp dir");
    let repo = AccountRepository::new(dir.path().join("accounts.json"));
    let mut app = test_app(dir.path());
    app.accounts_loaded = false;

    let _ = app.update(Message::Loaded(load_accounts(&repo)));

    assert!(app.accounts_loaded);
    assert!(app.state_to_save().is_ok());
}

#[test]
fn a_sign_in_without_stay_signed_in_is_noticed_and_the_capture_keeps_waiting() {
    let dir = tempdir().unwrap();
    let mut app = test_app(dir.path());
    let target = crate::ui::LoginCaptureTarget::NewAccount(AccountId::new());

    // Nothing is asked of Riot Client until it has been reopened.
    preparing_login_capture(&mut app, target);
    assert_eq!(app.update(Message::LoginCaptureSignInTick).units(), 0);

    waiting_login_capture(&mut app, target);
    assert_eq!(app.update(Message::LoginCaptureSignInTick).units(), 1);
    for _ in 0..crate::ui::LOGIN_CAPTURE_UNTICKED_CHECKS - 1 {
        let _ = app.update(Message::LoginCaptureSignInChecked(true));
    }
    assert!(!app.login_capture.as_ref().unwrap().unticked());
    let _ = app.update(Message::LoginCaptureSignInChecked(true));
    assert!(app.login_capture.as_ref().unwrap().unticked());
    assert!(app.login_capture.as_ref().unwrap().wait.is_some());

    // Signing out to try again goes back to waiting.
    let _ = app.update(Message::LoginCaptureSignInChecked(false));
    assert!(!app.login_capture.as_ref().unwrap().unticked());
}

#[test]
fn the_sign_in_dialog_hides_the_progress_toast_but_not_errors() {
    let dir = tempdir().unwrap();
    let mut app = test_app(dir.path());
    waiting_login_capture(
        &mut app,
        crate::ui::LoginCaptureTarget::NewAccount(AccountId::new()),
    );

    app.set_status(Status::progress("Opening Riot Client"));
    // Stable, without the interface refresh, keeps the toast over its dialog.
    assert!(status_bar_visible(&app));
    app.update_channel = Some(crate::updater::BETA_CHANNEL.to_string());
    assert!(!status_bar_visible(&app));
    app.set_status(Status::error("Could not save the previous login"));
    assert!(status_bar_visible(&app));
}

#[test]
fn add_current_account_starts_capture_without_login_prompt() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());

    let _ = app.update(Message::AddCurrentAccount);

    assert!(app.launcher_capture_in_progress);
    assert_eq!(
        app.launcher_capture_kind,
        Some(crate::ui::LauncherCaptureKind::Current)
    );
    assert!(!app.show_add_account_prompt);
    assert_eq!(
        app.status,
        Status::progress("Capturing the Riot account currently signed in")
    );
}

#[test]
fn current_account_capture_success_populates_confirmation_fields() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.launcher_capture_in_progress = true;
    app.status = Status::progress("Capturing the Riot account currently signed in");
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );

    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft.clone())));

    assert!(!app.launcher_capture_in_progress);
    assert_eq!(app.launcher_capture_kind, None);
    assert_eq!(app.pending_account, Some(draft));
    assert_eq!(app.new_display_name, "Player");
    // The confirm dialog opens, so the capture's toast ends.
    assert_eq!(app.status, Status::default());
}

#[test]
fn current_account_capture_failure_clears_capture_state() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.launcher_capture_in_progress = true;

    let _ = app.update(Message::CurrentAccountCaptureFinished(Err(
        "Riot Client is not signed in with Stay signed in enabled".to_string(),
    )));

    assert!(!app.launcher_capture_in_progress);
    assert_eq!(app.launcher_capture_kind, None);
    assert_eq!(app.pending_account, None);
    assert_eq!(
        app.status.text,
        "Could not add current account: Riot Client is not signed in with Stay signed in enabled"
    );
}

#[test]
fn confirming_new_current_account_saves_and_selects_profile() {
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
    let _ = app.update(Message::ConfirmCapturedAccount);
    app.repo.save(&app.state).expect("persist state");
    let saved = app.repo.load().expect("load saved state");

    assert_eq!(app.state.accounts.len(), 1);
    assert_eq!(app.state.selected_account, Some(draft.account_id));
    assert_eq!(app.pending_account, None);
    assert_eq!(saved.accounts.len(), 1);
    assert_eq!(saved.selected_account, Some(draft.account_id));
    assert_eq!(saved.accounts[0].puuid.as_deref(), Some("puuid-a"));
}

#[test]
fn adding_an_account_clears_the_previous_accounts_shop_and_loadout() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.store_summary = Some(empty_store_summary());
    app.loadout_summary = Some(LoadoutSummary::without_loadout("old".to_string(), None));
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );

    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft)));
    let _ = app.update(Message::ConfirmCapturedAccount);

    assert_eq!(app.store_summary, None);
    assert_eq!(app.loadout_summary, None);
}

#[test]
fn duplicate_current_account_capture_updates_existing_profile_without_confirmation() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.client_version_input = "release-10.10-shipping-1-1234567".to_string();
    let backup_root = app.repo.launcher_backups_dir();
    let mut existing = AccountProfile::new("Main", Shard::Na).expect("account");
    existing
        .apply_riot_identity("puuid-a", "OldName", "OLD")
        .expect("identity");
    let existing_id = existing.id;
    let existing_data = backup_root.join(existing_id.to_string()).join("Data");
    fs::create_dir_all(&existing_data).expect("existing data");
    fs::write(existing_data.join("old.txt"), "old").expect("old backup");
    existing.launcher_session = Some(LauncherSessionBackup {
        data_dir: existing_data.clone(),
        captured_at_unix: 50,
        puuid: "puuid-a".to_string(),
        unknown: Default::default(),
    });
    app.state.push_account(existing);
    let draft = captured_account_draft(&backup_root, "puuid-a", "Player", "NA1", Shard::Eu);
    let draft_id = draft.account_id;
    fs::write(
        draft.backup.data_dir.join("RiotGamesPrivateSettings.yaml"),
        "new-settings",
    )
    .expect("new settings");

    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft)));

    assert_eq!(app.state.accounts.len(), 1);
    assert_eq!(app.state.selected_account, Some(existing_id));
    assert_eq!(app.pending_account, None);
    assert_eq!(
        app.state.accounts[0].riot_id().as_deref(),
        Some("Player#NA1")
    );
    assert_eq!(app.state.accounts[0].shard, Shard::Eu);
    assert!(app.state.accounts[0].session.is_some());
    assert_eq!(
        app.state.accounts[0]
            .launcher_session
            .as_ref()
            .map(|backup| backup.data_dir.as_path()),
        Some(existing_data.as_path())
    );
    assert!(!backup_root.join(draft_id.to_string()).exists());
    assert!(!existing_data.join("old.txt").exists());
    assert_eq!(
        fs::read_to_string(existing_data.join("RiotGamesPrivateSettings.yaml")).expect("settings"),
        "new-settings"
    );
    assert!(
        app.status.text
            .starts_with("Duplicate account: Prime did not add a new profile because this Riot account is already in Prime; updated and selected Main")
    );
    assert!(!app.status.text.starts_with("Loading account details"));
    assert!((app.status.kind == StatusKind::Error));
}

#[test]
fn a_captured_account_opens_the_accounts_tab_to_confirm_it() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.active_tab = crate::ui::Tab::Shop;
    app.tab_scroll_offsets.set(
        crate::ui::Tab::Accounts,
        iced::widget::operation::AbsoluteOffset { x: 0.0, y: 400.0 },
    );
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );

    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft)));

    assert_eq!(app.active_tab, crate::ui::Tab::Accounts);
    assert_eq!(app.tab_scroll_offsets.get(crate::ui::Tab::Accounts).y, 0.0);
}

#[test]
fn adding_the_current_account_says_it_discarded_the_unsaved_one() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.pending_account = Some(captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    ));

    let _ = app.update(Message::AddCurrentAccount);

    assert_eq!(app.pending_account, None);
    assert!(app.status.text.contains("Discarded"), "{}", app.status.text);
}

#[test]
fn adding_a_new_account_says_it_discarded_the_unsaved_one() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.pending_account = Some(captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    ));

    let _ = app.update(Message::ConfirmAddAccountCapture);

    assert_eq!(app.pending_account, None);
    assert!(app.status.text.contains("Discarded"), "{}", app.status.text);
}

#[test]
fn canceling_captured_current_account_removes_temporary_backup() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );
    let backup_slot = app
        .repo
        .launcher_backups_dir()
        .join(draft.account_id.to_string());

    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft)));
    assert!(backup_slot.exists());

    let _ = app.update(Message::CancelCapturedAccount);

    assert_eq!(app.pending_account, None);
    assert!(!backup_slot.exists());
}
