//! The Aim Trainer tab's state and how `PrimeApp` handles its messages. The game is
//! `crate::aim_trainer`; raw mouse input and the cursor clip are `crate::raw_mouse`.

use std::time::{SystemTime, UNIX_EPOCH};

use iced::{Point, Rectangle, Subscription, Task, window};

use super::Status;
use super::{Message, PrimeApp, Tab};
use crate::aim_trainer::{
    self, AimRun, AimTrainerState, Difficulty, Game, SensitivityMode, View, ViewMode,
};
use crate::raw_mouse;
use crate::storage::AIM_MODES_EXPERIMENT;

#[derive(Clone, Debug)]
pub(super) enum AimMessage {
    SensitivityChanged(String),
    DpiChanged(String),
    SensitivityModeChanged(SensitivityMode),
    ViewModeChanged(ViewMode),
    DifficultyChanged(Difficulty),
    FullscreenToggled(bool),
    MonitorMeasured(Option<f32>),
    /// The arena's bounds in the window.
    ArenaResized(Rectangle),
    /// Start from Ready, Resume from Paused, Play again from Game over.
    Start,
    /// Esc during a run: end it and show its results.
    Stop,
    /// Game over's Done: back to Ready.
    Done,
    /// A shot with the Windows cursor aiming a 2D view, at this point in the arena.
    ClickedAt(Point),
    /// With the mouse input read since the last frame.
    Frame {
        now: iced::time::Instant,
        input: raw_mouse::Input,
    },
    FocusLost,
    /// The raw input reader couldn't start.
    RawInputUnavailable,
}

#[derive(Clone, Debug, Default)]
pub(super) struct AimTrainerTab {
    pub(super) phase: AimPhase,
    pub(super) sensitivity_input: String,
    pub(super) dpi_input: String,
    pub(super) monitor_width: Option<f32>,
    pub(super) arena: Option<Rectangle>,
    pub(super) last_frame: Option<iced::time::Instant>,
    /// The window went fullscreen for this run.
    pub(super) fullscreen: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) enum AimPhase {
    #[default]
    Ready,
    Playing(Box<Game>),
    /// Prime lost focus mid-run: the clock is stopped and the cursor released.
    Paused(Box<Game>),
    Over {
        game: Box<Game>,
        run: AimRun,
        previous_best: Option<AimRun>,
        new_best: bool,
    },
}

impl AimPhase {
    pub(super) fn game(&self) -> Option<&Game> {
        match self {
            Self::Ready => None,
            Self::Playing(game) | Self::Paused(game) | Self::Over { game, .. } => Some(game),
        }
    }

    pub(super) fn is_running(&self) -> bool {
        matches!(self, Self::Playing(_) | Self::Paused(_))
    }
}

impl AimTrainerTab {
    pub(super) fn sensitivity(&self) -> Option<f64> {
        aim_trainer::parse_sensitivity(&self.sensitivity_input)
    }

    pub(super) fn view(&self) -> Option<View> {
        let arena = self.arena?.size();
        let monitor = self
            .monitor_width
            .unwrap_or(aim_trainer::FALLBACK_MONITOR_WIDTH);
        View::new(monitor, arena.width, arena.height)
    }

    pub(super) fn can_start(&self, sensitivity_mode: SensitivityMode) -> bool {
        !matches!(self.phase, AimPhase::Playing(_))
            && (sensitivity_mode == SensitivityMode::Windows || self.sensitivity().is_some())
            && self.view().is_some()
    }
}

impl PrimeApp {
    pub(super) fn update_aim(&mut self, message: AimMessage) -> Task<Message> {
        match message {
            AimMessage::SensitivityChanged(input) => {
                if self.aim.phase.is_running() {
                    return Task::none();
                }
                let value = aim_trainer::parse_sensitivity(&input);
                self.aim.sensitivity_input = input;
                match value {
                    Some(value) if self.state.aim_trainer.sensitivity != Some(value) => {
                        self.state.aim_trainer.sensitivity = Some(value);
                        self.save_task()
                    }
                    _ => Task::none(),
                }
            }
            AimMessage::DpiChanged(input) => {
                if self.aim.phase.is_running() {
                    return Task::none();
                }
                // The default isn't written.
                let dpi = aim_trainer::parse_dpi(&input)
                    .map(|dpi| (dpi != aim_trainer::DEFAULT_DPI).then_some(dpi));
                self.aim.dpi_input = input;
                match dpi {
                    Some(dpi) if self.state.aim_trainer.dpi != dpi => {
                        self.state.aim_trainer.dpi = dpi;
                        self.save_task()
                    }
                    _ => Task::none(),
                }
            }
            AimMessage::SensitivityModeChanged(mode) => {
                self.set_aim_setting(|settings| &mut settings.sensitivity_mode, mode)
            }
            AimMessage::ViewModeChanged(mode) => {
                self.set_aim_setting(|settings| &mut settings.view_mode, mode)
            }
            AimMessage::DifficultyChanged(difficulty) => {
                self.set_aim_setting(|settings| &mut settings.difficulty, difficulty)
            }
            AimMessage::FullscreenToggled(on) => {
                self.set_aim_setting(|settings| &mut settings.fullscreen, on)
            }
            AimMessage::MonitorMeasured(width) => {
                self.aim.monitor_width = width;
                self.refresh_aim_view();
                Task::none()
            }
            AimMessage::ArenaResized(bounds) => {
                self.aim.arena = Some(bounds);
                self.refresh_aim_view();
                // Going fullscreen moves the arena under the cursor.
                if let AimPhase::Playing(game) = &mut self.aim.phase {
                    grab_cursor(game, bounds);
                }
                Task::none()
            }
            AimMessage::Start => self.start_aim_run(),
            AimMessage::Stop => self.end_aim_run(),
            AimMessage::Done => {
                if matches!(self.aim.phase, AimPhase::Over { .. }) {
                    self.aim.phase = AimPhase::Ready;
                }
                Task::none()
            }
            AimMessage::ClickedAt(point) => {
                let AimPhase::Playing(game) = &mut self.aim.phase else {
                    return Task::none();
                };
                game.click_at(f64::from(point.x), f64::from(point.y));
                if game.is_over() {
                    self.end_aim_run()
                } else {
                    Task::none()
                }
            }
            AimMessage::Frame { now, input } => {
                let AimPhase::Playing(game) = &mut self.aim.phase else {
                    return Task::none();
                };
                // Each click shoots where the crosshair was when its button went down.
                for (dx, dy) in input.clicks {
                    game.move_aim(dx, dy);
                    game.click();
                }
                game.move_aim(input.moved.0, input.moved.1);
                let elapsed = self.aim.last_frame.map_or(0, |last| {
                    u64::try_from(now.saturating_duration_since(last).as_millis())
                        .unwrap_or(u64::MAX)
                });
                // The part of a millisecond left over counts toward the next frame, or a 144 Hz
                // frame of 6.9 ms would count as 6 and the run would play 13% slow.
                self.aim.last_frame = Some(match self.aim.last_frame {
                    Some(last) => last + std::time::Duration::from_millis(elapsed),
                    None => now,
                });
                game.advance(elapsed);
                if game.is_over() {
                    self.end_aim_run()
                } else {
                    Task::none()
                }
            }
            AimMessage::FocusLost => self.pause_aim_run(),
            AimMessage::RawInputUnavailable => {
                let task = self.end_aim_run();
                self.set_status(Status::error(
                    "Raw mouse input isn't available, so the Aim Trainer can't read your mouse.",
                ));
                task
            }
        }
    }

    /// A setup choice, which a run keeps until it ends.
    fn set_aim_setting<T: PartialEq>(
        &mut self,
        field: fn(&mut AimTrainerState) -> &mut T,
        value: T,
    ) -> Task<Message> {
        let setting = field(&mut self.state.aim_trainer);
        if self.aim.phase.is_running() || *setting == value {
            return Task::none();
        }
        *setting = value;
        self.save_task()
    }

    /// Opening the tab: the fields show the saved values, and the monitor is measured again in
    /// case the window moved.
    pub(super) fn open_aim_trainer(&mut self) -> Task<Message> {
        if !self.aim.phase.is_running() {
            self.aim.sensitivity_input = self
                .state
                .aim_trainer
                .sensitivity
                .map(|value| value.to_string())
                .unwrap_or_default();
            self.aim.dpi_input = self.state.aim_trainer.dpi().to_string();
        }
        measure_monitor()
    }

    fn refresh_aim_view(&mut self) {
        let Some(view) = self.aim.view() else {
            return;
        };
        if let AimPhase::Playing(game) | AimPhase::Paused(game) = &mut self.aim.phase {
            game.set_view(view);
        }
    }

    /// The Sensitivity and View a new run uses. Without the experiment they're the original
    /// trainer's, whatever is saved, so switching it back on restores the saved choices.
    pub(super) fn aim_modes(&self) -> (SensitivityMode, ViewMode) {
        let settings = &self.state.aim_trainer;
        if self.state.experiment(AIM_MODES_EXPERIMENT) {
            (settings.sensitivity_mode, settings.view_mode)
        } else {
            (SensitivityMode::Valorant, ViewMode::TwoD)
        }
    }

    fn start_aim_run(&mut self) -> Task<Message> {
        let (sensitivity_mode, view_mode) = self.aim_modes();
        let settings = &self.state.aim_trainer;
        if !self.aim.can_start(sensitivity_mode) {
            return Task::none();
        }
        let (Some(view), Some(arena)) = (self.aim.view(), self.aim.arena) else {
            return Task::none();
        };
        let sensitivity = match sensitivity_mode {
            SensitivityMode::Valorant => self.aim.sensitivity(),
            SensitivityMode::Windows => None,
        };
        let mut game = match std::mem::take(&mut self.aim.phase) {
            AimPhase::Paused(game) => game,
            _ => Box::new(Game::new(
                sensitivity,
                view_mode,
                settings.difficulty,
                view,
                seed(),
            )),
        };
        grab_cursor(&mut game, arena);
        self.aim.phase = AimPhase::Playing(game);
        self.aim.last_frame = None;
        self.aim.fullscreen = settings.fullscreen;
        if settings.fullscreen {
            Task::batch([set_fullscreen(true), measure_monitor()])
        } else {
            measure_monitor()
        }
    }

    /// The cursor back, and the window back to its size if the run went fullscreen.
    fn leave_aim_run(&mut self) -> Task<Message> {
        self.aim.last_frame = None;
        raw_mouse::release_cursor();
        if std::mem::take(&mut self.aim.fullscreen) {
            set_fullscreen(false)
        } else {
            Task::none()
        }
    }

    /// Esc, game over or leaving the tab: show the results and keep a new best.
    pub(super) fn end_aim_run(&mut self) -> Task<Message> {
        let game = match std::mem::take(&mut self.aim.phase) {
            AimPhase::Playing(game) | AimPhase::Paused(game) => game,
            other => {
                self.aim.phase = other;
                return Task::none();
            }
        };
        let run = AimRun::new(
            game.results(),
            game.sensitivity(),
            game.sensitivity().map(|_| self.state.aim_trainer.dpi()),
            unix_now(),
        );
        let previous_best = self.state.aim_trainer.best(game.difficulty());
        let new_best = run.score > previous_best.as_ref().map_or(0, |best| best.score);
        let save = if new_best {
            *self.state.aim_trainer.best_mut(game.difficulty()) = Some(run.clone());
            self.save_task()
        } else {
            Task::none()
        };
        self.aim.phase = AimPhase::Over {
            game,
            run,
            previous_best,
            new_best,
        };
        Task::batch([save, self.leave_aim_run()])
    }

    /// Focus loss or closing the window: stop the clock and give the cursor back.
    pub(super) fn pause_aim_run(&mut self) -> Task<Message> {
        if !matches!(self.aim.phase, AimPhase::Playing(_)) {
            return Task::none();
        }
        if let AimPhase::Playing(game) = std::mem::take(&mut self.aim.phase) {
            self.aim.phase = AimPhase::Paused(game);
        }
        self.leave_aim_run()
    }

    /// Whether a run fills the window, without the sidebar.
    pub(super) fn aim_fullscreen(&self) -> bool {
        self.aim.fullscreen && matches!(self.aim.phase, AimPhase::Playing(_))
    }
}

/// Frames, focus loss and the raw mouse reader, only while a run plays on screen. The reader
/// also keeps the cursor held when aiming with it. The Windows cursor's clicks in 2D come from
/// the arena instead, which knows where they land.
pub(super) fn subscription(app: &PrimeApp) -> Option<Subscription<Message>> {
    if !matches!(app.aim.phase, AimPhase::Playing(_)) {
        return None;
    }
    (app.active_tab == Tab::AimTrainer).then(|| {
        Subscription::batch([
            Subscription::run(raw_mouse::reader)
                .map(|_| Message::Aim(AimMessage::RawInputUnavailable)),
            // Input is taken once a frame: a message for each count would rebuild the UI each time.
            window::frames().map(|now| {
                Message::Aim(AimMessage::Frame {
                    now,
                    input: raw_mouse::take_input(),
                })
            }),
            iced::event::listen_with(focus_event),
        ])
    })
}

fn focus_event(
    event: iced::Event,
    _status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Window(window::Event::Unfocused) => Some(Message::Aim(AimMessage::FocusLost)),
        _ => None,
    }
}

/// Takes the cursor for a run. Called here, not in a task, so it's always ordered before a
/// release. Raw counts aim from wherever the cursor is held; the Windows cursor aims a 2D view
/// inside the arena, and turns a 3D one from its centre.
fn grab_cursor(game: &mut Game, arena: Rectangle) {
    match (game.sensitivity_mode(), game.mode()) {
        (SensitivityMode::Valorant, _) => raw_mouse::hold_cursor(),
        (SensitivityMode::Windows, ViewMode::TwoD) => raw_mouse::confine_cursor(arena),
        (SensitivityMode::Windows, ViewMode::ThreeD) => {
            if let Some(scale) = raw_mouse::recentre_cursor(arena) {
                game.set_pixel_scale(scale);
            }
        }
    }
}

/// Fullscreen on the window's monitor, or back to Prime's fixed size. The size limits would
/// otherwise hold a fullscreen window at that size.
fn set_fullscreen(on: bool) -> Task<Message> {
    window::latest().then(move |id| {
        let Some(id) = id else {
            return Task::none();
        };
        if on {
            window::set_min_size(id, None)
                .chain(window::set_max_size(id, None))
                .chain(window::set_mode(id, window::Mode::Fullscreen))
        } else {
            window::set_mode(id, window::Mode::Windowed)
                .chain(window::set_min_size(id, Some(super::WINDOW_SIZE)))
                .chain(window::set_max_size(id, Some(super::WINDOW_SIZE)))
        }
    })
}

/// The monitor can change under the window, so it is measured on opening the tab and each start.
fn measure_monitor() -> Task<Message> {
    window::latest().then(|id| {
        id.map_or_else(Task::none, |id| {
            window::monitor_size(id)
                .map(|size| Message::Aim(AimMessage::MonitorMeasured(size.map(|size| size.width))))
        })
    })
}

fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |elapsed| elapsed.subsec_nanos().into())
        ^ unix_now().unsigned_abs()
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}
