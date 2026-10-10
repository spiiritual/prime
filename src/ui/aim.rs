//! The Aim Trainer tab's state and how `PrimeApp` handles its messages. The game is
//! `crate::aim_trainer`; raw mouse input and the cursor clip are `crate::raw_mouse`.

// The view (Task 5) is what constructs and reads the rest; drop this when it lands.
#![allow(dead_code)]

use std::time::{SystemTime, UNIX_EPOCH};

use iced::{Size, Subscription, Task, mouse, window};

use super::{Message, PrimeApp, Tab};
use crate::aim_trainer::{self, AimRun, Game, View};
use crate::raw_mouse;

#[derive(Clone, Debug)]
pub(super) enum AimMessage {
    SensitivityChanged(String),
    DpiChanged(String),
    MonitorMeasured(Option<f32>),
    ArenaResized(Size),
    /// Start from Ready, Resume from Paused, Play again from Game over.
    Start,
    /// Esc during a run: end it and show its results.
    Stop,
    /// Game over's Done: back to Ready.
    Done,
    MouseMoved(i32, i32),
    Clicked,
    Frame(iced::time::Instant),
    FocusLost,
}

#[derive(Clone, Debug, Default)]
pub(super) struct AimTrainerTab {
    pub(super) phase: AimPhase,
    pub(super) sensitivity_input: String,
    pub(super) dpi_input: String,
    pub(super) monitor_width: Option<f32>,
    pub(super) arena: Option<Size>,
    pub(super) last_frame: Option<iced::time::Instant>,
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
        let arena = self.arena?;
        let monitor = self
            .monitor_width
            .unwrap_or(aim_trainer::FALLBACK_MONITOR_WIDTH);
        View::new(monitor, arena.width, arena.height)
    }

    pub(super) fn can_start(&self) -> bool {
        !matches!(self.phase, AimPhase::Playing(_))
            && self.sensitivity().is_some()
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
            AimMessage::MonitorMeasured(width) => {
                self.aim.monitor_width = width;
                self.refresh_aim_view();
                Task::none()
            }
            AimMessage::ArenaResized(size) => {
                self.aim.arena = Some(size);
                self.refresh_aim_view();
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
            AimMessage::MouseMoved(dx, dy) => {
                if let AimPhase::Playing(game) = &mut self.aim.phase {
                    game.move_aim(dx, dy);
                }
                Task::none()
            }
            AimMessage::Clicked => {
                let AimPhase::Playing(game) = &mut self.aim.phase else {
                    return Task::none();
                };
                game.click();
                if game.is_over() {
                    self.end_aim_run()
                } else {
                    Task::none()
                }
            }
            AimMessage::Frame(now) => {
                let AimPhase::Playing(game) = &mut self.aim.phase else {
                    return Task::none();
                };
                let elapsed = self.aim.last_frame.map_or(0, |last| {
                    u64::try_from(now.saturating_duration_since(last).as_millis())
                        .unwrap_or(u64::MAX)
                });
                self.aim.last_frame = Some(now);
                game.advance(elapsed);
                if game.is_over() {
                    self.end_aim_run()
                } else {
                    Task::none()
                }
            }
            AimMessage::FocusLost => self.pause_aim_run(),
        }
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
        window::latest().then(|id| {
            id.map_or_else(Task::none, |id| {
                window::monitor_size(id).map(|size| {
                    Message::Aim(AimMessage::MonitorMeasured(size.map(|size| size.width)))
                })
            })
        })
    }

    fn refresh_aim_view(&mut self) {
        let Some(view) = self.aim.view() else {
            return;
        };
        if let AimPhase::Playing(game) | AimPhase::Paused(game) = &mut self.aim.phase {
            game.set_view(view);
        }
    }

    fn start_aim_run(&mut self) -> Task<Message> {
        if !self.aim.can_start() {
            return Task::none();
        }
        let (Some(view), Some(sensitivity)) = (self.aim.view(), self.aim.sensitivity()) else {
            return Task::none();
        };
        self.aim.phase = match std::mem::take(&mut self.aim.phase) {
            AimPhase::Paused(game) => AimPhase::Playing(game),
            _ => AimPhase::Playing(Box::new(Game::new(sensitivity, view, seed()))),
        };
        self.aim.last_frame = None;
        Task::future(async { raw_mouse::hold_cursor() }).discard()
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
            self.state.aim_trainer.dpi(),
            unix_now(),
        );
        let previous_best = self.state.aim_trainer.best;
        let new_best = run.score > previous_best.map_or(0, |best| best.score);
        let save = if new_best {
            self.state.aim_trainer.best = Some(run);
            self.save_task()
        } else {
            Task::none()
        };
        self.aim.last_frame = None;
        self.aim.phase = AimPhase::Over {
            game,
            run,
            previous_best,
            new_best,
        };
        Task::batch([release_cursor_task(), save])
    }

    /// Focus loss or closing the window: stop the clock and give the cursor back.
    pub(super) fn pause_aim_run(&mut self) -> Task<Message> {
        if !matches!(self.aim.phase, AimPhase::Playing(_)) {
            return Task::none();
        }
        if let AimPhase::Playing(game) = std::mem::take(&mut self.aim.phase) {
            self.aim.phase = AimPhase::Paused(game);
        }
        self.aim.last_frame = None;
        release_cursor_task()
    }
}

/// Raw mouse counts, frames and clicks, only while a run plays on screen.
pub(super) fn subscription(app: &PrimeApp) -> Option<Subscription<Message>> {
    let playing = matches!(app.aim.phase, AimPhase::Playing(_));
    (app.active_tab == Tab::AimTrainer && playing).then(|| {
        Subscription::batch([
            Subscription::run(raw_mouse::movements)
                .map(|(dx, dy)| Message::Aim(AimMessage::MouseMoved(dx, dy))),
            window::frames().map(|now| Message::Aim(AimMessage::Frame(now))),
            iced::event::listen_with(run_event),
        ])
    })
}

fn run_event(
    event: iced::Event,
    _status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
            Some(Message::Aim(AimMessage::Clicked))
        }
        iced::Event::Window(window::Event::Unfocused) => Some(Message::Aim(AimMessage::FocusLost)),
        _ => None,
    }
}

fn release_cursor_task() -> Task<Message> {
    Task::future(async { raw_mouse::release_cursor() }).discard()
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
