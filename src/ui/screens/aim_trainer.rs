//! The Aim Trainer tab: score, accuracy, best and health over a fixed-size arena, with the setup
//! and results as modals inside it.

use iced::widget::{canvas, column, container, row, space, stack};
use iced::{
    Border, Color, Element, Length, Point, Rectangle, Renderer, Shadow, Size, Theme, Vector,
    alignment, mouse,
};
use time::{OffsetDateTime, UtcOffset};

use crate::aim_trainer::{self, AimRun};
use crate::ui::aim::{AimMessage, AimPhase, AimTrainerTab};
use crate::ui::theme::{self, Icon, text};
use crate::ui::{Message, PrimeApp};

const HEALTH_BAR_WIDTH: f32 = 260.0;

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    column![hud(app), arena(app)].spacing(20).into()
}

fn hud(app: &PrimeApp) -> Element<'_, Message> {
    let game = app.aim.phase.game();
    let score = game.map_or(0, |game| game.score());
    let accuracy = game.and_then(|game| game.results().accuracy());
    let health = game.map_or(100, |game| game.health());
    let best = app.state.aim_trainer.best.map(|run| run.score);

    let health_bar = column![
        row![
            text("Health")
                .size(11)
                .font(theme::SEMIBOLD_FONT)
                .color(theme::MUTED),
            space().width(Length::Fill),
            text(format!("{health}%"))
                .size(11)
                .font(theme::MONO_FONT)
                .line_height(theme::MONO_LINE_HEIGHT)
                .color(theme::MUTED),
        ],
        container(
            container(space())
                .width(HEALTH_BAR_WIDTH * health as f32 / 100.0)
                .height(8)
                .style(|_| filled(theme::ACCENT, 4.0)),
        )
        .width(HEALTH_BAR_WIDTH)
        .height(8)
        .style(|_| filled(theme::RAISED, 4.0)),
    ]
    .spacing(6)
    .width(HEALTH_BAR_WIDTH);

    row![
        stat("Score", score.to_string(), theme::TEXT),
        stat("Accuracy", percent(accuracy), theme::TEXT),
        stat(
            "Best",
            best.map_or("—".into(), |best| best.to_string()),
            theme::MUTED
        ),
        space().width(Length::Fill),
        health_bar,
    ]
    .spacing(32)
    .align_y(alignment::Vertical::Bottom)
    .into()
}

fn stat<'a>(label: &'a str, value: String, color: Color) -> Element<'a, Message> {
    column![
        text(label)
            .size(11)
            .font(theme::SEMIBOLD_FONT)
            .color(theme::MUTED),
        text(value)
            .size(22)
            .font(theme::DISPLAY_FONT)
            .line_height(theme::DISPLAY_LINE_HEIGHT)
            .color(color),
    ]
    .spacing(2)
    .into()
}

fn arena(app: &PrimeApp) -> Element<'_, Message> {
    let mut layers = stack![
        canvas(Arena { tab: &app.aim })
            .width(Length::Fill)
            .height(Length::Fill)
    ];
    match &app.aim.phase {
        AimPhase::Playing(_) => {
            layers = layers.push(
                container(
                    text("Esc to stop")
                        .size(11)
                        .font(theme::MONO_FONT)
                        .line_height(theme::MONO_LINE_HEIGHT)
                        .color(theme::FAINT),
                )
                .padding(16),
            );
        }
        AimPhase::Ready => layers = layers.push(modal(setup_card(app, false))),
        AimPhase::Paused(_) => layers = layers.push(modal(setup_card(app, true))),
        AimPhase::Over {
            run,
            previous_best,
            new_best,
            ..
        } => layers = layers.push(modal(results_card(run, previous_best.as_ref(), *new_best))),
    }

    container(layers)
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .style(|_| container::Style {
            background: Some(theme::SURFACE.into()),
            border: Border {
                color: theme::LINE,
                width: 1.0,
                radius: 12.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// A card centred over the dimmed arena.
fn modal(card: Element<'_, Message>) -> Element<'_, Message> {
    container(card)
        .center(Length::Fill)
        .style(|_| container::Style {
            background: Some(
                Color {
                    a: 0.6,
                    ..theme::BG
                }
                .into(),
            ),
            ..container::Style::default()
        })
        .into()
}

fn card_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(theme::SURFACE.into()),
        border: Border {
            color: theme::LINE,
            width: 1.0,
            radius: 14.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.6),
            offset: Vector::new(0.0, 16.0),
            blur_radius: 48.0,
        },
        ..container::Style::default()
    }
}

fn setup_card(app: &PrimeApp, paused: bool) -> Element<'_, Message> {
    let tab = &app.aim;
    let sensitivity = tab.sensitivity();
    let dpi = aim_trainer::parse_dpi(&tab.dpi_input);

    let mut sensitivity_input =
        theme::text_input("e.g. 0.35", &tab.sensitivity_input).font(theme::MONO_FONT);
    let mut dpi_input = theme::text_input("800", &tab.dpi_input).font(theme::MONO_FONT);
    // A run keeps the sensitivity it started with, so the fields are read-only while paused.
    if !paused {
        sensitivity_input =
            sensitivity_input.on_input(|value| Message::Aim(AimMessage::SensitivityChanged(value)));
        dpi_input = dpi_input.on_input(|value| Message::Aim(AimMessage::DpiChanged(value)));
    }

    let (edpi, cm) = match (sensitivity, dpi) {
        (Some(sensitivity), Some(dpi)) => (
            format!("{}", (sensitivity * f64::from(dpi)).round()),
            format!("{:.1} cm", aim_trainer::cm_per_360(sensitivity, dpi)),
        ),
        _ => ("—".to_string(), "—".to_string()),
    };

    let start_label = if paused { "Resume" } else { "Start" };
    let start = theme::button(theme::icon_label(Icon::Play, start_label, theme::BG))
        .style(theme::danger_button_style)
        .padding([9, 16])
        .on_press_maybe(tab.can_start().then_some(Message::Aim(AimMessage::Start)));

    let mut card = column![
        column![
            text(if paused { "Paused" } else { "Ready" })
                .size(20)
                .font(theme::DISPLAY_FONT)
                .line_height(theme::DISPLAY_LINE_HEIGHT),
            text(
                "Pop the dots before they grow. Your crosshair moves at your VALORANT sensitivity."
            )
            .size(13)
            .color(theme::MUTED),
        ]
        .spacing(4),
        row![
            field(
                "VALORANT sensitivity",
                sensitivity_input.into(),
                Length::Fill
            ),
            field("Mouse DPI", dpi_input.into(), Length::Fixed(130.0)),
        ]
        .spacing(12),
        row![
            tile("eDPI", edpi),
            tile("cm / 360°", cm),
            tile("FOV", format!("{}°", aim_trainer::HORIZONTAL_FOV_DEGREES)),
        ]
        .spacing(8),
    ]
    .spacing(18);

    if let Some(best) = &app.state.aim_trainer.best {
        card = card.push(
            container(
                row![
                    theme::icon(Icon::Trophy, 15.0, theme::GOLD),
                    text(format!("Best {}", best.score))
                        .size(13)
                        .font(theme::SEMIBOLD_FONT),
                    text(best_detail(best)).size(12).color(theme::MUTED),
                ]
                .spacing(10)
                .align_y(alignment::Vertical::Center),
            )
            .width(Length::Fill)
            .padding([12, 14])
            .style(|_| container::Style {
                border: Border {
                    color: theme::LINE,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..container::Style::default()
            }),
        );
    }

    card = card.push(
        row![
            row![
                theme::icon(Icon::Mouse, 13.0, theme::FAINT),
                text("Raw input · Esc to stop").size(11).color(theme::FAINT),
            ]
            .spacing(6)
            .align_y(alignment::Vertical::Center),
            space().width(Length::Fill),
            start,
        ]
        .align_y(alignment::Vertical::Center),
    );

    container(card)
        .width(460)
        .padding(24)
        .style(card_style)
        .into()
}

fn results_card<'a>(
    run: &AimRun,
    previous_best: Option<&AimRun>,
    new_best: bool,
) -> Element<'a, Message> {
    let mut top = row![
        text("Game over")
            .size(20)
            .font(theme::DISPLAY_FONT)
            .line_height(theme::DISPLAY_LINE_HEIGHT),
        space().width(Length::Fill),
    ]
    .align_y(alignment::Vertical::Center);
    if new_best {
        top = top.push(
            container(
                row![
                    theme::icon(Icon::Trophy, 13.0, theme::GOLD),
                    text("New best")
                        .size(12)
                        .font(theme::SEMIBOLD_FONT)
                        .color(theme::GOLD),
                ]
                .spacing(6)
                .align_y(alignment::Vertical::Center),
            )
            .padding([4, 9])
            .style(|_| {
                filled(
                    Color {
                        a: 0.12,
                        ..theme::GOLD
                    },
                    6.0,
                )
            }),
        );
    }

    let compared = match previous_best {
        Some(previous) if new_best => format!("dots · previous best {}", previous.score),
        Some(best) => format!("dots · best {}", best.score),
        None => "dots".to_string(),
    };

    let note = format!(
        "At {} sens · {} DPI ({:.1} cm/360°)",
        run.sensitivity,
        run.dpi,
        aim_trainer::cm_per_360(run.sensitivity, run.dpi)
    );

    let actions = row![
        space().width(Length::Fill),
        theme::button(text("Done").size(13).font(theme::BOLD_FONT))
            .padding([9, 14])
            .on_press(Message::Aim(AimMessage::Done)),
        theme::button(theme::icon_label(Icon::RotateCcw, "Play again", theme::BG))
            .style(theme::danger_button_style)
            .padding([9, 14])
            .on_press(Message::Aim(AimMessage::Start)),
    ]
    .spacing(8);

    container(
        column![
            top,
            row![
                text(run.score.to_string())
                    .size(56)
                    .font(theme::DISPLAY_FONT)
                    .line_height(1.0),
                text(compared).size(13).color(theme::MUTED),
            ]
            .spacing(10)
            .align_y(alignment::Vertical::Bottom),
            row![
                tile("Hits", run.score.to_string()),
                tile("Misses", run.misses.to_string()),
                tile("Burst", run.bursts.to_string()),
            ]
            .spacing(8),
            row![
                tile("Accuracy", percent(run.accuracy())),
                tile("Avg reaction", reaction(run.avg_reaction_ms)),
                tile("Time", clock(run.duration_ms)),
            ]
            .spacing(8),
            text(note).size(11).color(theme::FAINT),
            actions,
        ]
        .spacing(18),
    )
    .width(440)
    .padding(24)
    .style(card_style)
    .into()
}

fn field<'a>(label: &'a str, input: Element<'a, Message>, width: Length) -> Element<'a, Message> {
    column![
        text(label)
            .size(12)
            .font(theme::SEMIBOLD_FONT)
            .color(theme::MUTED),
        input
    ]
    .spacing(6)
    .width(width)
    .into()
}

fn tile<'a>(label: &'a str, value: String) -> Element<'a, Message> {
    container(
        column![
            text(label)
                .size(11)
                .font(theme::SEMIBOLD_FONT)
                .color(theme::MUTED),
            text(value)
                .size(14)
                .font(theme::MONO_SEMIBOLD_FONT)
                .line_height(theme::MONO_LINE_HEIGHT),
        ]
        .spacing(4),
    )
    .width(Length::Fill)
    .padding([10, 12])
    .style(|_| filled(theme::RAISED, 8.0))
    .into()
}

fn filled(color: Color, radius: f32) -> container::Style {
    container::Style {
        background: Some(color.into()),
        border: Border {
            radius: radius.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

fn percent(accuracy: Option<f64>) -> String {
    accuracy.map_or("—".to_string(), |accuracy| {
        format!("{:.0}%", accuracy * 100.0)
    })
}

fn reaction(ms: Option<u32>) -> String {
    ms.map_or("—".to_string(), |ms| format!("{ms} ms"))
}

fn clock(ms: u64) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn best_detail(best: &AimRun) -> String {
    let date = OffsetDateTime::from_unix_timestamp(best.finished_at_unix)
        .map(|at| at.to_offset(UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC)))
        .map(|at| format!("{} {}", &at.month().to_string()[..3], at.day()))
        .unwrap_or_default();
    format!(
        "{} accuracy · {} · {date}",
        percent(best.accuracy()),
        reaction(best.avg_reaction_ms)
    )
}

/// Draws the dots and the crosshair, hides the cursor during a run, and reports the arena's size,
/// which sets how much of VALORANT's view fits.
struct Arena<'a> {
    tab: &'a AimTrainerTab,
}

impl canvas::Program<Message> for Arena<'_> {
    type State = Option<Size>;

    fn update(
        &self,
        state: &mut Self::State,
        _event: &iced::Event,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let size = bounds.size();
        (*state != Some(size)).then(|| {
            *state = Some(size);
            canvas::Action::publish(Message::Aim(AimMessage::ArenaResized(size)))
        })
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let Some(game) = self.tab.phase.game() else {
            return vec![frame.into_geometry()];
        };
        let view = game.view();
        let playing = matches!(self.tab.phase, AimPhase::Playing(_));
        // Behind a modal the run's last dots stay, faded.
        let color = if playing {
            theme::ACCENT
        } else {
            Color {
                a: 0.25,
                ..theme::ACCENT
            }
        };
        for (dot, diameter) in game.dots() {
            let (x, y) = view.to_px(dot.yaw, dot.pitch);
            let radius = view.radius_px(dot.yaw, dot.pitch, diameter);
            frame.fill(
                &canvas::Path::circle(Point::new(x as f32, y as f32), radius as f32),
                color,
            );
        }
        if playing {
            let (x, y) = view.to_px(game.aim().0, game.aim().1);
            draw_crosshair(&mut frame, Point::new(x as f32, y as f32));
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if matches!(self.tab.phase, AimPhase::Playing(_)) {
            mouse::Interaction::Hidden
        } else {
            mouse::Interaction::default()
        }
    }
}

/// VALORANT's default look: four white 6×2 arms 4px out and a centre dot, each with a black
/// 1px outline so it shows over a dot.
fn draw_crosshair(frame: &mut canvas::Frame, center: Point) {
    let parts = [
        (-10.0, -1.0, 6.0, 2.0),
        (4.0, -1.0, 6.0, 2.0),
        (-1.0, -10.0, 2.0, 6.0),
        (-1.0, 4.0, 2.0, 6.0),
        (-1.0, -1.0, 2.0, 2.0),
    ];
    for (color, grow) in [(Color::BLACK, 1.0), (Color::WHITE, 0.0)] {
        for (x, y, width, height) in parts {
            frame.fill_rectangle(
                Point::new(center.x + x - grow, center.y + y - grow),
                Size::new(width + grow * 2.0, height + grow * 2.0),
                color,
            );
        }
    }
}
