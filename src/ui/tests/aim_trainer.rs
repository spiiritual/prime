use super::*;

fn aim(message: AimMessage) -> Message {
    Message::Aim(message)
}

/// The Aim Trainer tab, laid out on a 1920px monitor, with 0.34 typed in.
fn aim_app(dir: &Path) -> PrimeApp {
    let mut app = test_app(dir);
    let _ = app.update(Message::TabSelected(Tab::AimTrainer));
    let _ = app.update(aim(AimMessage::MonitorMeasured(Some(1920.0))));
    let _ = app.update(aim(AimMessage::ArenaResized(iced::Rectangle::new(
        iced::Point::ORIGIN,
        iced::Size::new(976.0, 664.0),
    ))));
    let _ = app.update(aim(AimMessage::SensitivityChanged("0.34".into())));
    // Hard's pace, which the timings below were written for.
    app.state.aim_trainer.difficulty = crate::aim_trainer::Difficulty::Hard;
    app
}

#[test]
fn stable_runs_use_the_saved_aim_modes() {
    use crate::aim_trainer::SensitivityMode;

    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    app.update_channel = Some(crate::updater::STABLE_CHANNEL.to_string());
    let _ = app.update(aim(AimMessage::SensitivityModeChanged(
        SensitivityMode::Windows,
    )));
    let _ = app.update(aim(AimMessage::Start));
    let AimPhase::Playing(game) = &app.aim.phase else {
        panic!("playing");
    };
    assert_eq!(
        (game.sensitivity_mode(), game.mode()),
        (SensitivityMode::Windows, app.state.aim_trainer.view_mode)
    );
}

/// Sends frames `ms` apart in 100 ms steps, the way `window::frames` would.
fn aim_frames(app: &mut PrimeApp, ms: u64) {
    let mut at = app.aim.last_frame.unwrap_or_else(iced::time::Instant::now);
    let _ = app.update(aim(AimMessage::Frame {
        now: at,
        input: Default::default(),
    }));
    for _ in 0..ms / 100 {
        at += Duration::from_millis(100);
        let _ = app.update(aim(AimMessage::Frame {
            now: at,
            input: Default::default(),
        }));
    }
}

/// A frame with one raw click, after `moved` counts, and no time passing.
fn aim_click(app: &mut PrimeApp, moved: (i32, i32)) {
    let _ = app.update(aim(AimMessage::Frame {
        now: app.aim.last_frame.unwrap_or_else(iced::time::Instant::now),
        input: crate::raw_mouse::Input {
            clicks: vec![moved],
            moved: (0, 0),
        },
    }));
}

/// Moves the crosshair onto the newest dot in whole counts and clicks it.
fn aim_hit_newest(app: &mut PrimeApp) {
    let AimPhase::Playing(game) = &app.aim.phase else {
        panic!("not playing");
    };
    let (yaw, pitch) = game.dots().last().expect("a dot").angles();
    let camera = game.camera();
    let per_count = game.degrees_per_count().expect("aims with counts");
    let dx = ((yaw - camera.yaw) / per_count).round() as i32;
    let dy = ((pitch - camera.pitch) / per_count).round() as i32;
    aim_click(app, (dx, dy));
}

#[test]
fn aim_trainer_opens_with_the_saved_sensitivity_and_dpi() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.state.aim_trainer.sensitivity = Some(0.4);
    app.state.aim_trainer.dpi = Some(1600);

    let _ = app.update(Message::TabSelected(Tab::AimTrainer));

    assert_eq!(app.aim.sensitivity_input, "0.4");
    assert_eq!(app.aim.dpi_input, "1600");
}

#[test]
fn aim_sensitivity_is_saved_only_when_valid() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    assert_eq!(app.state.aim_trainer.sensitivity, Some(0.34));

    let task = app.update(aim(AimMessage::SensitivityChanged("0,35".into())));
    assert!(task.units() > 0, "saves accounts.json");
    assert_eq!(app.state.aim_trainer.sensitivity, Some(0.35));

    for bad in ["abc", "0", "-1", ""] {
        let task = app.update(aim(AimMessage::SensitivityChanged(bad.into())));
        assert_eq!(task.units(), 0, "{bad} isn't saved");
        assert_eq!(
            app.aim.sensitivity_input, bad,
            "the field keeps what was typed"
        );
        assert_eq!(app.state.aim_trainer.sensitivity, Some(0.35));
        assert!(
            !app.aim
                .can_start(crate::aim_trainer::SensitivityMode::Valorant),
            "{bad} can't start a run"
        );
        let _ = app.update(aim(AimMessage::Start));
        assert!(matches!(app.aim.phase, AimPhase::Ready));
    }
}

#[test]
fn aim_dpi_of_800_is_not_saved() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());

    let _ = app.update(aim(AimMessage::DpiChanged("1600".into())));
    assert_eq!(app.state.aim_trainer.dpi, Some(1600));
    let _ = app.update(aim(AimMessage::DpiChanged("800".into())));
    assert_eq!(app.state.aim_trainer.dpi, None);
    let _ = app.update(aim(AimMessage::DpiChanged("0".into())));
    assert_eq!(app.state.aim_trainer.dpi, None);
    assert_eq!(app.aim.dpi_input, "0");
}

#[test]
fn aim_run_starts_once_and_esc_ends_it_with_results() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());

    aim_click(&mut app, (0, 0));
    assert!(
        matches!(app.aim.phase, AimPhase::Ready),
        "clicks before Start do nothing"
    );

    let _ = app.update(aim(AimMessage::Start));
    assert!(matches!(app.aim.phase, AimPhase::Playing(_)));
    aim_frames(&mut app, 0);
    aim_hit_newest(&mut app);

    let _ = app.update(aim(AimMessage::Start));
    let AimPhase::Playing(game) = &app.aim.phase else {
        panic!("still playing");
    };
    assert_eq!(game.score(), 1, "a second Start doesn't restart the run");

    let _ = app.update(Message::EscapePressed);
    let AimPhase::Over { run, .. } = &app.aim.phase else {
        panic!("Esc shows the results");
    };
    assert_eq!(run.score, 1);

    let _ = app.update(aim(AimMessage::Done));
    assert!(matches!(app.aim.phase, AimPhase::Ready));
}

#[test]
fn aim_with_the_windows_cursor_in_2d_needs_no_sensitivity_and_shoots_where_clicked() {
    use crate::aim_trainer::{SensitivityMode, ViewMode};

    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    let _ = app.update(aim(AimMessage::SensitivityChanged(String::new())));
    assert!(!app.aim.can_start(SensitivityMode::Valorant));

    let task = app.update(aim(AimMessage::SensitivityModeChanged(
        SensitivityMode::Windows,
    )));
    assert!(task.units() > 0, "saved");
    let task = app.update(aim(AimMessage::ViewModeChanged(ViewMode::TwoD)));
    assert!(task.units() > 0, "saved");
    assert_eq!(
        (
            app.state.aim_trainer.sensitivity_mode,
            app.state.aim_trainer.view_mode
        ),
        (SensitivityMode::Windows, ViewMode::TwoD)
    );
    let _ = app.update(aim(AimMessage::Start));
    aim_frames(&mut app, 0);
    let AimPhase::Playing(game) = &app.aim.phase else {
        panic!("playing");
    };
    let dot = game.drawn_dots().last().expect("a dot");
    let (x, y) = (dot.x, dot.y);
    let _ = app.update(aim(AimMessage::ClickedAt(iced::Point::new(
        x as f32, y as f32,
    ))));
    // A raw-count click is ignored when the cursor aims itself.
    aim_click(&mut app, (0, 0));

    let _ = app.update(aim(AimMessage::Stop));
    let AimPhase::Over { run, .. } = &app.aim.phase else {
        panic!("over");
    };
    assert_eq!((run.score, run.misses), (1, 0));
    assert_eq!((run.sensitivity, run.dpi), (None, None));
}

#[test]
fn aim_with_the_windows_cursor_in_3d_turns_the_view_and_shoots_the_centre() {
    use crate::aim_trainer::SensitivityMode;

    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    let _ = app.update(aim(AimMessage::SensitivityModeChanged(
        SensitivityMode::Windows,
    )));
    let _ = app.update(aim(AimMessage::Start));
    aim_frames(&mut app, 0);
    aim_hit_newest(&mut app);

    let _ = app.update(aim(AimMessage::Stop));
    let AimPhase::Over { run, .. } = &app.aim.phase else {
        panic!("over");
    };
    assert_eq!((run.score, run.misses), (1, 0));
    assert_eq!(run.sensitivity, None);
}

#[test]
fn aim_fullscreen_lasts_only_while_a_run_plays() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    let _ = app.update(aim(AimMessage::FullscreenToggled(true)));
    assert!(app.state.aim_trainer.fullscreen);
    assert!(!app.aim_fullscreen());

    let _ = app.update(aim(AimMessage::Start));
    assert!(app.aim_fullscreen());
    // Settings stay as the run started.
    let _ = app.update(aim(AimMessage::FullscreenToggled(false)));
    assert!(app.state.aim_trainer.fullscreen);

    let task = app.update(aim(AimMessage::FocusLost));
    assert!(!app.aim_fullscreen());
    assert!(task.units() > 0, "the window leaves fullscreen");
    let _ = app.update(aim(AimMessage::Start));
    assert!(app.aim_fullscreen());
    let _ = app.update(aim(AimMessage::Stop));
    assert!(!app.aim_fullscreen());

    let _ = app.update(aim(AimMessage::Done));
    let _ = app.update(aim(AimMessage::FullscreenToggled(false)));
    let _ = app.update(aim(AimMessage::Start));
    assert!(!app.aim_fullscreen());
    let task = app.update(aim(AimMessage::FocusLost));
    assert_eq!(task.units(), 0, "a windowed run leaves the window alone");
}

#[test]
fn aim_best_run_is_replaced_only_by_a_higher_score() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    let best = |score| crate::aim_trainer::AimRun {
        score,
        misses: 0,
        bursts: 0,
        avg_reaction_ms: None,
        duration_ms: 0,
        sensitivity: Some(0.34),
        dpi: Some(800),
        finished_at_unix: 0,
        unknown: Default::default(),
    };
    app.state.aim_trainer.best = Some(best(1));

    let _ = app.update(aim(AimMessage::Start));
    aim_frames(&mut app, 0);
    aim_hit_newest(&mut app);
    aim_frames(&mut app, 600);
    aim_hit_newest(&mut app);
    let _ = app.update(aim(AimMessage::Stop));

    let AimPhase::Over {
        new_best,
        previous_best,
        ..
    } = &app.aim.phase
    else {
        panic!("over");
    };
    assert!(*new_best);
    assert_eq!(previous_best.as_ref().map(|run| run.score), Some(1));
    assert_eq!(
        app.state.aim_trainer.best.as_ref().map(|run| run.score),
        Some(2)
    );

    app.state.aim_trainer.best = Some(best(5));
    let _ = app.update(aim(AimMessage::Start));
    let _ = app.update(aim(AimMessage::Stop));
    let AimPhase::Over { new_best, .. } = &app.aim.phase else {
        panic!("over");
    };
    assert!(!*new_best);
    assert_eq!(
        app.state.aim_trainer.best.as_ref().map(|run| run.score),
        Some(5)
    );

    // Each difficulty keeps its own best.
    let _ = app.update(aim(AimMessage::DifficultyChanged(
        crate::aim_trainer::Difficulty::Easy,
    )));
    let _ = app.update(aim(AimMessage::Start));
    aim_frames(&mut app, 0);
    aim_hit_newest(&mut app);
    let _ = app.update(aim(AimMessage::Stop));
    let AimPhase::Over { new_best, .. } = &app.aim.phase else {
        panic!("over");
    };
    assert!(*new_best);
    assert_eq!(
        app.state.aim_trainer.best_easy.map(|run| run.score),
        Some(1)
    );
    assert_eq!(
        app.state.aim_trainer.best.as_ref().map(|run| run.score),
        Some(5)
    );
}

#[test]
fn aim_run_pauses_on_focus_loss_and_resumes() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    let _ = app.update(aim(AimMessage::Start));
    aim_frames(&mut app, 0);
    aim_hit_newest(&mut app);

    let _ = app.update(aim(AimMessage::FocusLost));
    assert!(matches!(app.aim.phase, AimPhase::Paused(_)));
    let _ = app.update(aim(AimMessage::SensitivityChanged("2".into())));
    assert_eq!(
        app.state.aim_trainer.sensitivity,
        Some(0.34),
        "read-only while paused"
    );

    let _ = app.update(aim(AimMessage::Start));
    let AimPhase::Playing(game) = &app.aim.phase else {
        panic!("resumed");
    };
    assert_eq!(game.score(), 1, "resuming keeps the run");
}

#[test]
fn aim_run_ends_when_leaving_the_tab_and_pauses_when_closing() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    let _ = app.update(aim(AimMessage::Start));
    let _ = app.update(Message::TabSelected(Tab::Settings));
    assert!(matches!(app.aim.phase, AimPhase::Over { .. }));

    let _ = app.update(Message::TabSelected(Tab::AimTrainer));
    let _ = app.update(aim(AimMessage::Start));
    let _ = app.update(Message::CloseRequested(iced::window::Id::unique()));
    assert!(
        matches!(app.aim.phase, AimPhase::Paused(_)),
        "closing releases the cursor first"
    );
}

#[test]
fn aim_frames_keep_the_part_of_a_millisecond_they_leave_over() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    let _ = app.update(aim(AimMessage::Start));
    let mut at = iced::time::Instant::now();
    // 144 Hz for a second.
    for _ in 0..=144 {
        let _ = app.update(aim(AimMessage::Frame {
            now: at,
            input: Default::default(),
        }));
        at += Duration::from_micros(6_944);
    }
    let AimPhase::Playing(game) = &app.aim.phase else {
        panic!("playing");
    };
    assert_eq!(game.results().duration_ms, 999);
}

#[test]
fn aim_arena_of_zero_size_is_ignored() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let _ = app.update(Message::TabSelected(Tab::AimTrainer));
    let _ = app.update(aim(AimMessage::SensitivityChanged("0.34".into())));
    let _ = app.update(aim(AimMessage::ArenaResized(iced::Rectangle::default())));
    assert!(
        !app.aim
            .can_start(crate::aim_trainer::SensitivityMode::Valorant),
        "no arena to play in yet"
    );

    let mut app = aim_app(dir.path());
    let _ = app.update(aim(AimMessage::Start));
    let before = app.aim.phase.game().expect("game").view();
    let _ = app.update(aim(AimMessage::ArenaResized(iced::Rectangle::default())));
    assert_eq!(app.aim.phase.game().expect("game").view(), before);
}

#[test]
fn aim_run_ends_when_a_login_capture_jumps_to_accounts() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    let _ = app.update(aim(AimMessage::Start));

    // The capture-complete messages need a live login capture, so call the shared jump directly.
    let _ = app.show_accounts_tab_top();

    assert_eq!(app.active_tab, Tab::Accounts);
    assert!(matches!(app.aim.phase, AimPhase::Over { .. }));
}

#[test]
fn aim_run_ends_with_an_error_when_raw_input_is_unavailable() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    let _ = app.update(aim(AimMessage::Start));
    let _ = app.update(aim(AimMessage::RawInputUnavailable));
    assert!(matches!(app.aim.phase, AimPhase::Over { .. }));
    assert_eq!(app.status.kind, StatusKind::Error);
    assert!(app.status.text.contains("Raw mouse input"));
}

#[test]
fn aim_esc_closes_a_dialog_before_stopping_a_paused_run() {
    let dir = tempdir().expect("temp dir");
    let mut app = aim_app(dir.path());
    let _ = app.update(aim(AimMessage::Start));
    let _ = app.update(aim(AimMessage::FocusLost));
    app.confirm_quit = Some(QuitAction::Exit);

    let _ = app.update(Message::EscapePressed);
    assert!(app.confirm_quit.is_none(), "the dialog closes first");
    assert!(matches!(app.aim.phase, AimPhase::Paused(_)));

    let _ = app.update(Message::EscapePressed);
    assert!(matches!(app.aim.phase, AimPhase::Over { .. }));
}
