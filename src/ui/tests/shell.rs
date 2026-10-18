use super::*;

#[test]
fn status_bar_keeps_errors_and_running_progress_on_screen() {
    let changed_at = iced::time::Instant::now();
    let later = changed_at + Duration::from_secs(10);
    let visible_later = |status: Status| status_visible_at(&status, changed_at, later);

    assert!(!visible_later(Status::info("Loaded 2 account profile(s)")));
    assert!(visible_later(Status::progress("Loading shop")));
    assert!(!visible_later(Status::success("Saved settings")));
    assert!(!visible_later(Status::warning(
        "Loaded account details for 2 account(s); 1 partial"
    )));
    assert!(visible_later(Status::error(
        "Failed to load accounts: disk error"
    )));
    assert!(!visible_later(Status::error("")));
}

#[test]
fn status_bar_briefly_shows_successful_actions() {
    let changed_at = iced::time::Instant::now();

    assert!(status_visible_at(
        &Status::success("Saved settings"),
        changed_at,
        changed_at
    ));
    assert!(status_visible_at(
        &Status::success("Saved settings"),
        changed_at,
        changed_at + Duration::from_secs(3)
    ));
    assert!(!status_visible_at(
        &Status::success("Saved settings"),
        changed_at,
        changed_at + Duration::from_secs(4)
    ));
    assert!(!status_visible_at(
        &Status::info(""),
        changed_at,
        changed_at
    ));
}

#[test]
fn changing_the_status_restarts_its_display_time() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.status_changed_at = iced::time::Instant::now() - Duration::from_secs(60);
    app.now = iced::time::Instant::now();
    assert!(!status_bar_visible(&app));

    let _ = app.update(Message::SaveSettings);

    assert_eq!(app.status, Status::success("Saved settings"));
    assert!(status_bar_visible(&app));
}

#[test]
fn a_toast_that_closes_by_itself_counts_down_and_an_error_does_not() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let _ = app.update(Message::SaveSettings);
    app.now = app.status_changed_at;
    assert_eq!(status_time_left(&app), Some(1.0));

    app.now = app.status_changed_at + Duration::from_secs(2);
    assert_eq!(status_time_left(&app), Some(0.5));

    app.status = Status::error("Could not save settings");
    app.now = app.status_changed_at;
    assert_eq!(status_time_left(&app), None);
}

#[test]
fn repeating_an_action_shows_its_status_again() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let _ = app.update(Message::SaveSettings);
    app.status_changed_at = iced::time::Instant::now() - Duration::from_secs(60);
    app.now = iced::time::Instant::now();
    assert!(!status_bar_visible(&app));

    let _ = app.update(Message::SaveSettings);

    assert_eq!(app.status, Status::success("Saved settings"));
    assert!(status_bar_visible(&app));
}

#[test]
fn status_spinner_shows_only_beside_progress() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.account_availability_loading = true;

    app.status = Status::info("Loaded account details for 2 account(s)");
    assert!(!status_spinner_active(&app));

    app.status = Status::error("Could not load shop: offline");
    assert!(!status_spinner_active(&app));

    app.status = Status::progress("Loading shop");
    assert!(status_spinner_active(&app));

    app.launcher_capture_in_progress = true;
    app.status = Status::info("Sign in to Riot Client and tick Stay signed in");
    assert!(status_spinner_active(&app));

    app.status = Status::error("Signed in as a different Riot account");
    assert!(!status_spinner_active(&app));
}

fn key_press(key: iced::keyboard::Key) -> iced::keyboard::Event {
    iced::keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::default(),
        text: None,
        repeat: false,
    }
}

#[test]
fn only_the_escape_key_asks_to_close() {
    let escape = key_press(iced::keyboard::Key::Named(
        iced::keyboard::key::Named::Escape,
    ));
    let enter = key_press(iced::keyboard::Key::Named(
        iced::keyboard::key::Named::Enter,
    ));

    assert!(matches!(
        crate::ui::escape_key_message(escape),
        Some(Message::EscapePressed)
    ));
    assert!(crate::ui::escape_key_message(enter).is_none());
}

#[test]
fn escape_closes_the_open_dialog() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let _ = app.update(Message::OpenImportAccount);

    let _ = app.update(Message::EscapePressed);

    assert!(!app.show_import_account_prompt);
}

#[test]
fn a_dialog_starts_its_entrance_when_it_opens_or_replaces_another() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, _) = two_account_app(dir.path());

    let _ = app.update(Message::OpenImportAccount);
    let (dialog, opened) = app.dialog_opened.expect("import dialog");
    assert_eq!(dialog, crate::ui::Dialog::ImportAccount);

    // Typing in the open dialog doesn't restart it.
    let _ = app.update(Message::ImportAccountInputChanged("x".to_string()));
    assert_eq!(app.dialog_opened, Some((dialog, opened)));

    let _ = app.update(Message::CancelImportAccount);
    assert_eq!(app.dialog_opened, None);

    let _ = app.update(Message::RequestDeleteAccount(main.id));
    assert_eq!(
        app.dialog_opened.map(|(dialog, _)| dialog),
        Some(crate::ui::Dialog::DeleteAccount)
    );
}

#[test]
fn a_toast_on_screen_changes_text_without_rising_again() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let _ = app.update(Message::SaveSettings);
    let appeared = app.toast_appeared_at;

    std::thread::sleep(Duration::from_millis(2));
    let _ = app.update(Message::SaveSettings);
    assert_eq!(app.toast_appeared_at, appeared);

    // Once it has gone, the next one rises in.
    app.status_changed_at -= Duration::from_secs(60);
    app.now = iced::time::Instant::now();
    let _ = app.update(Message::SaveSettings);
    assert!(app.toast_appeared_at > appeared);
}

#[test]
fn an_entrance_eases_out_over_its_duration() {
    let start = iced::time::Instant::now();
    assert_eq!(crate::ui::appear_progress(start, start), 0.0);
    assert_eq!(
        crate::ui::appear_progress(start, start + Duration::from_secs(1)),
        1.0
    );
    let halfway = crate::ui::appear_progress(start, start + crate::ui::APPEAR_DURATION / 2);
    // Most of the motion happens early.
    assert!(halfway > 0.8 && halfway < 1.0, "{halfway}");
}

#[test]
fn a_closing_dialog_fades_its_backdrop_out() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let _ = app.update(Message::OpenImportAccount);
    assert_eq!(crate::ui::closing_scrim(&app), None);

    let _ = app.update(Message::CancelImportAccount);
    let closed = app.dialog_closed_at.expect("closing");
    assert!(crate::ui::appearing(&app));

    app.now = closed + crate::ui::APPEAR_DURATION / 2;
    let scrim = crate::ui::closing_scrim(&app).expect("still fading");
    assert!(scrim > 0.0 && scrim < 1.0, "{scrim}");

    app.now = closed + crate::ui::APPEAR_DURATION;
    assert_eq!(crate::ui::closing_scrim(&app), None);
    assert!(!crate::ui::appearing(&app));

    // Opening another dialog ends the fade at once.
    app.now = closed;
    let _ = app.update(Message::OpenImportAccount);
    assert_eq!(crate::ui::closing_scrim(&app), None);
}

#[test]
fn a_toast_that_closes_by_itself_sinks_out_at_the_end() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let _ = app.update(Message::SaveSettings);
    assert!(status_time_left(&app).is_some(), "{}", app.status.text);

    app.now = app.status_changed_at + Duration::from_secs(1);
    assert_eq!(crate::ui::toast_progress(&app), 1.0);

    app.now =
        app.status_changed_at + crate::ui::STATUS_FLASH_DURATION - crate::ui::APPEAR_DURATION / 2;
    let leaving = crate::ui::toast_progress(&app);
    assert!(leaving > 0.0 && leaving < 1.0, "{leaving}");
}

#[test]
fn escape_closes_an_open_account_menu() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, _) = two_account_app(dir.path());
    app.open_account_menu = Some(main.id);

    let _ = app.update(Message::EscapePressed);

    assert_eq!(app.open_account_menu, None);
}

#[test]
fn escape_does_not_close_an_import_that_is_running() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let _ = app.update(Message::OpenImportAccount);
    app.import_account_in_progress = true;

    let _ = app.update(Message::EscapePressed);

    assert!(app.show_import_account_prompt);
}

#[test]
fn escape_cancels_a_login_capture_only_once_it_is_waiting() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let target = crate::ui::LoginCaptureTarget::NewAccount(AccountId::new());
    preparing_login_capture(&mut app, target);
    let _ = app.update(Message::EscapePressed);
    assert!(app.login_capture.is_some());

    waiting_login_capture(&mut app, target);
    let _ = app.update(Message::EscapePressed);
    assert!(app.login_capture.is_none());
}

#[test]
fn the_spinner_ticks_only_while_one_is_on_screen() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());

    // Availability checks show no spinner.
    app.account_availability_loading = true;
    assert!(!crate::ui::loading_indicator_active(&app));

    app.status = Status::progress("Loading…");
    app.status_changed_at = app.now;
    assert!(crate::ui::loading_indicator_active(&app));

    // Progress stays until its result replaces it, and the result has no spinner.
    app.now = app.status_changed_at + Duration::from_secs(60);
    assert!(crate::ui::loading_indicator_active(&app));
    app.status = Status::success("Loaded");
    assert!(!crate::ui::loading_indicator_active(&app));
}

#[test]
fn progress_reads_as_unfinished_and_regions_read_in_capitals() {
    assert_eq!(
        Status::progress("Loading accounts").text,
        "Loading accounts\u{2026}"
    );
    assert_eq!(Status::progress("Loading\u{2026}").text, "Loading\u{2026}");
    assert_eq!(Status::progress("Sign in.").text, "Sign in.");

    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    assert_eq!(account.summary(), "Main (NA)");
}

#[test]
fn a_dialog_that_opens_is_its_own_feedback_without_a_toast() {
    let dir = tempdir().expect("temp dir");
    let (mut app, first, _) = two_account_app(dir.path());

    let _ = app.update(Message::AddAccount);
    assert!(app.show_add_account_prompt);
    assert!(!status_bar_visible(&app));
    let _ = app.update(Message::CancelAddAccountCapture);

    let _ = app.update(Message::RequestExportAccount(first.id));
    let _ = app.update(Message::AccountExportPrepared(Ok(
        crate::ui::AccountExportOutput::new(first.id, "Main".to_string(), "payload".to_string()),
    )));
    assert!(app.exported_account.is_some());
    assert!(!status_bar_visible(&app));
}
