use std::path::Path;

use iced::widget::{checkbox, column, container, row, space};
use iced::{Color, Element, Length, Padding, Theme, alignment};

use crate::storage::EXPERIMENTS;
use crate::ui::components::{anchored_popover, spinner};
use crate::ui::data::live_match::{LIVE_MATCH_WEAPON_GROUPS, shown_weapons, weapon_name};
use crate::ui::shell::popover_style;
use crate::ui::theme::{self, Icon, button, text};
use crate::ui::{AppUpdateStatus, Message, PrimeApp};
use crate::updater::{BETA_CHANNEL, STABLE_CHANNEL};

pub(super) const ERROR_TEXT: Color = iced::color!(0xFF8A94);
const WEAPON_SLOT_HEIGHT: f32 = 36.0;
const WEAPON_PICKER_WIDTH: f32 = 452.0;
const OPEN_SLOT_BORDER: Color = iced::color!(0x3A4250);
const CURRENT_PILL_BORDER: Color = iced::color!(0xFF4F5E, 0x55 as f32 / 255.0);

/// A checkbox per experiment.
pub(super) fn experiment_controls(app: &PrimeApp) -> Element<'_, Message> {
    EXPERIMENTS
        .iter()
            .fold(column![].spacing(10), |controls, &(name, label)| {
            controls.push(
                checkbox(app.experiment(name))
                    .label(label)
                    .on_toggle(move |on| Message::ExperimentToggled(name, on))
                    .text_size(13),
            )
        })
        .into()
}

/// The design's plain secondary button.
pub(super) fn plain_button(label: &str) -> iced::widget::Button<'_, Message> {
    button(text(label).size(13).font(theme::SEMIBOLD_FONT)).padding([8, 14])
}

/// A button's label, or the spinner and `busy` while its work runs. Without the interface
/// refresh, `busy` alone.
pub(super) fn busy_label<'a>(
    app: &PrimeApp,
    busy: Option<&'a str>,
    idle: Element<'a, Message>,
) -> Element<'a, Message> {
    match busy {
        Some(label) => row![
            spinner(app.loading_frame, 14.0, theme::MUTED),
            text(label).size(13).font(theme::SEMIBOLD_FONT)
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center)
        .into(),
        None => idle,
    }
}

/// A slot per skin column, left to right as on the page, each opening a weapon picker.
pub(super) fn live_match_weapon_controls(app: &PrimeApp, slot_width: f32) -> Element<'_, Message> {
    let shown = shown_weapons(app.state.live_match_weapons.as_deref());
    let mut columns = shown
        .iter()
        .enumerate()
        .fold(row![].spacing(8), |columns, (column, weapon)| {
            let open = app.open_weapon_picker == Some(column);
            columns.push(anchored_popover(
                weapon_slot(column, weapon_name(weapon), open, slot_width),
                weapon_picker(column, &shown),
                open,
                WEAPON_SLOT_HEIGHT + 6.0,
                // The popover lines up with its anchor's right edge, so a negative inset of
                // the width difference lines it up with the slot's left edge instead.
                slot_width - WEAPON_PICKER_WIDTH,
            ))
        })
        .align_y(alignment::Vertical::Center)
        .width(Length::Fill);
    if app.state.live_match_weapons.is_some() {
        columns = columns.push(space().width(Length::Fill)).push(
            iced::widget::button(
                row![
                    theme::icon(Icon::RotateCcw, 13.0, theme::MUTED),
                    text("Reset to default")
                        .size(12)
                        .font(theme::MEDIUM_FONT)
                        .color(theme::MUTED),
                ]
                .spacing(6)
                .align_y(alignment::Vertical::Center),
            )
            .padding([8, 10])
            .style(|_, status| iced::widget::button::Style {
                background: hovered(status).then(|| theme::SURFACE.into()),
                border: iced::border::rounded(8),
                ..Default::default()
            })
            .on_press(Message::ResetLiveMatchWeapons),
        );
    }
    columns.into()
}

pub(super) fn hovered(status: iced::widget::button::Status) -> bool {
    matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    )
}

/// The column's number, weapon and a chevron, raised while its picker is open.
fn weapon_slot(
    column: usize,
    name: &'static str,
    open: bool,
    width: f32,
) -> Element<'static, Message> {
    iced::widget::button(
        row![
            text((column + 1).to_string())
                .size(11)
                .font(theme::MONO_SEMIBOLD_FONT)
                .line_height(theme::MONO_LINE_HEIGHT)
                .color(theme::FAINT),
            text(name)
                .size(13)
                .font(theme::SEMIBOLD_FONT)
                .width(Length::Fill),
            theme::icon(
                if open {
                    Icon::ChevronUp
                } else {
                    Icon::ChevronDown
                },
                14.0,
                if open { theme::TEXT } else { theme::MUTED },
            ),
        ]
        .spacing(10)
        .height(Length::Fill)
        .align_y(alignment::Vertical::Center),
    )
    .padding(Padding {
        top: 0.0,
        right: 10.0,
        bottom: 0.0,
        left: 12.0,
    })
    .width(width)
    .height(WEAPON_SLOT_HEIGHT)
    .style(move |_, status| iced::widget::button::Style {
        background: Some(if open || hovered(status) {
            theme::RAISED.into()
        } else {
            theme::SURFACE.into()
        }),
        text_color: theme::TEXT,
        border: iced::Border {
            color: if open { OPEN_SLOT_BORDER } else { theme::LINE },
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    })
    .on_press(Message::ToggleWeaponPicker(column))
    .into()
}

/// Every weapon by category. The column's own weapon is highlighted, and weapons in other
/// columns show that column's number, since picking one swaps the two.
fn weapon_picker(column: usize, shown: &[String]) -> Element<'static, Message> {
    let groups = LIVE_MATCH_WEAPON_GROUPS.into_iter().fold(
        column![].spacing(10),
        |groups, (category, weapons)| {
            let pills = weapons.iter().fold(
                row![].spacing(2).align_y(alignment::Vertical::Center),
                |pills, &(id, name)| {
                    let at = shown.iter().position(|shown| shown == id);
                    pills.push(weapon_pill(column, id, name, at))
                },
            );
            groups.push(
                row![
                    text(category.to_uppercase())
                        .size(9)
                        .font(theme::SEMIBOLD_FONT)
                        .color(theme::FAINT)
                        .width(64),
                    pills,
                ]
                .spacing(8)
                .align_y(alignment::Vertical::Center),
            )
        },
    );
    let hint = row![
        theme::icon(Icon::ArrowLeftRight, 12.0, theme::FAINT),
        text("Numbered weapons are in another column. Picking one swaps the two.")
            .size(11)
            .color(theme::FAINT),
    ]
    .spacing(6)
    .align_y(alignment::Vertical::Center);

    // Opaque, so clicks on its gaps don't reach the page under it.
    iced::widget::opaque(
        container(
            column![
                groups,
                container(space())
                    .width(Length::Fill)
                    .height(1)
                    .style(|_| container::Style::default().background(theme::LINE)),
                hint,
            ]
            .spacing(10),
        )
        .padding(Padding {
            top: 12.0,
            right: 12.0,
            bottom: 10.0,
            left: 12.0,
        })
        .width(WEAPON_PICKER_WIDTH)
        .style(popover_style),
    )
}

/// `at` is the column the weapon is already in, if any.
fn weapon_pill(
    column: usize,
    id: &'static str,
    name: &'static str,
    at: Option<usize>,
) -> Element<'static, Message> {
    let current = at == Some(column);
    let label = text(name).size(12).font(if current {
        theme::SEMIBOLD_FONT
    } else {
        theme::MEDIUM_FONT
    });
    let content: Element<_> = match at {
        Some(other) if other != column => row![
            label.color(theme::MUTED),
            container(
                text((other + 1).to_string())
                    .size(9)
                    .font(theme::MONO_SEMIBOLD_FONT)
                    .line_height(theme::MONO_LINE_HEIGHT)
                    .color(theme::MUTED),
            )
            .center(14)
            .style(|_| {
                container::Style::default()
                    .background(theme::LINE)
                    .border(iced::border::rounded(4))
            }),
        ]
        .spacing(5)
        .align_y(alignment::Vertical::Center)
        .into(),
        _ => label.into(),
    };

    iced::widget::button(content)
        .padding([5, 8])
        .style(move |_, status| {
            let background = if current {
                Some(theme::ACCENT_SOFT.into())
            } else {
                hovered(status).then(|| theme::MENU_HOVER.into())
            };
            iced::widget::button::Style {
                background,
                text_color: theme::TEXT,
                border: iced::Border {
                    color: if current {
                        CURRENT_PILL_BORDER
                    } else {
                        Color::TRANSPARENT
                    },
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            }
        })
        .on_press(Message::LiveMatchWeaponPicked { column, weapon: id })
        .into()
}

pub(super) const LOGS_HELP: &str = "Prime's, VALORANT's and Riot Client's logs in one zip, \
                                     tokens taken out. Send it if VALORANT crashes or Prime \
                                     misbehaves.";

pub(super) fn export_logs_button(app: &PrimeApp) -> Element<'_, Message> {
    button(busy_label(
        app,
        app.logs_exporting.then_some("Exporting\u{2026}"),
        theme::icon_label(Icon::Download, "Export logs", theme::TEXT),
    ))
    .padding([8, 14])
    .on_press_maybe((!app.logs_exporting).then_some(Message::ExportLogs))
    .into()
}

/// The path with %APPDATA% or %LOCALAPPDATA% in place of the user's own folders.
pub(super) fn short_path(path: &Path) -> String {
    for variable in ["LOCALAPPDATA", "APPDATA"] {
        if let Some(root) = std::env::var_os(variable)
            && let Ok(rest) = path.strip_prefix(&root)
        {
            return format!("%{variable}%\\{}", rest.display());
        }
    }

    path.display().to_string()
}

pub(super) fn update_controls(app: &PrimeApp) -> Element<'_, Message> {
    let current = crate::updater::CURRENT_VERSION;

    if let Some(update) = app.app_update_status.pending_update() {
        let blocking_work = app.work_blocking_update();
        let detail = match (&app.app_update_status, blocking_work) {
            (AppUpdateStatus::InstallFailed { error, .. }, _) => {
                format!("You have {current} · the last try failed: {error}")
            }
            (_, Some(work)) => format!("You have {current} · wait for {work} to finish first"),
            (_, None) => format!("You have {current}"),
        };

        let mut card = row![
            theme::icon(Icon::Sparkles, 18.0, theme::OK),
            column![
                text(format!("Prime {} is available", update.latest_version))
                    .size(13)
                    .font(theme::SEMIBOLD_FONT),
                text(detail).size(12).color(theme::MUTED)
            ]
            .spacing(2)
            .width(Length::Fill)
        ]
        .spacing(14)
        .align_y(alignment::Vertical::Center);

        if update.changelog.is_some() {
            card = card.push(
                plain_button("Changelog")
                    .padding([8, 12])
                    .on_press(Message::ShowAppUpdate),
            );
        }

        card = card.push(
            button(theme::icon_label(
                Icon::Download,
                "Download & restart",
                theme::BG,
            ))
            .padding([8, 14])
            .style(theme::success_button_style)
            .on_press_maybe(
                blocking_work
                    .is_none()
                    .then_some(Message::DownloadAppUpdate),
            ),
        );

        return container(card)
            .padding(14)
            .width(Length::Fill)
            .style(|_| {
                container::Style::default()
                    .background(Color {
                        a: 0.06,
                        ..theme::OK
                    })
                    .border(iced::Border {
                        color: Color {
                            a: 0.33,
                            ..theme::OK
                        },
                        width: 1.0,
                        radius: 10.0.into(),
                    })
            })
            .into();
    }

    let status = match &app.app_update_status {
        AppUpdateStatus::Checking => "checking for updates…".to_string(),
        AppUpdateStatus::UpToDate => "you're up to date".to_string(),
        AppUpdateStatus::Downloading(update) => {
            format!("downloading {}…", update.latest_version)
        }
        AppUpdateStatus::Installing => "restarting to install the update…".to_string(),
        AppUpdateStatus::NotInstalled => {
            "not an installed copy, so updates don't apply".to_string()
        }
        AppUpdateStatus::CheckFailed(error) => format!("update check failed: {error}"),
        AppUpdateStatus::InstallFailed { error, .. } => format!("update failed: {error}"),
        // Shown as the card above.
        AppUpdateStatus::Available(_) | AppUpdateStatus::Dismissed(_) => String::new(),
    };

    row![
        text(format!("Version {current} — {status}"))
            .size(13)
            .width(Length::Fill),
        button(busy_label(
            app,
            matches!(app.app_update_status, AppUpdateStatus::Checking)
                .then_some("Checking\u{2026}"),
            text("Check for updates")
                .size(13)
                .font(theme::SEMIBOLD_FONT)
                .into(),
        ))
        .padding([8, 14])
        .on_press_maybe((!app.app_update_status.is_busy()).then_some(Message::CheckForAppUpdate))
    ]
    .spacing(12)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// Beta gets new versions first. Joining or leaving checks the other channel; its latest release
/// shows as an update to install, older or not. Outside an install, such as `cargo run`, it shows
/// stable and can't switch.
pub(super) fn channel_controls(app: &PrimeApp) -> Element<'_, Message> {
    let beta = app.update_channel.as_deref() == Some(BETA_CHANNEL);
    let switching = app
        .app_update_status
        .pending_update()
        .and_then(|update| update.switch_to());
    let label = match switching {
        Some(BETA_CHANNEL) => "Download & restart above to finish joining beta.",
        Some(_) => "Download & restart above to finish going back to stable.",
        None if beta => "You get beta versions, which have new features first and may have bugs.",
        None => "Get beta versions to try new features first. They may have bugs.",
    };
    let (action, switch_to) = if beta {
        ("Go back to stable", STABLE_CHANNEL)
    } else {
        ("Join beta", BETA_CHANNEL)
    };

    row![
        text(label).size(13).color(theme::MUTED).width(Length::Fill),
        plain_button(action).on_press_maybe(
            (app.update_channel.is_some()
                && switching.is_none()
                && !app.app_update_status.is_busy())
            .then_some(Message::SwitchUpdateChannel(switch_to))
        )
    ]
    .spacing(12)
    .align_y(alignment::Vertical::Center)
    .into()
}

pub(super) fn clear_cache_style(
    theme: &Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let mut style = theme::button_style(theme, status);
    style.border.color = Color {
        a: 0.33,
        ..theme::ACCENT
    };
    if !matches!(status, iced::widget::button::Status::Disabled) {
        style.text_color = ERROR_TEXT;
    }
    style
}
