use std::path::Path;

use iced::advanced::widget::operation::{Operation, Outcome};
use iced::advanced::widget::{Id, operate};
use iced::widget::operation::{AbsoluteOffset, scroll_to};
use iced::widget::{checkbox, column, container, row, space, stack};
use iced::{Color, Element, Length, Padding, Rectangle, Task, Theme, Vector, alignment};

use crate::storage::{AIM_MODES_EXPERIMENT, INTERFACE_REFRESH_EXPERIMENT, SETTINGS_PAGES_EXPERIMENT};
use crate::ui::components::{anchored_popover, card_style, spinner};
use crate::ui::data::live_match::{LIVE_MATCH_WEAPON_GROUPS, shown_weapons, weapon_name};
use crate::ui::data::{format_bytes, typed_riot_client_path};
use crate::ui::shell::popover_style;
use crate::ui::theme::{self, Icon, button, text};
use crate::ui::{AppUpdateStatus, MAIN_PANEL_SCROLLABLE_ID, Message, PrimeApp, SettingsSection};
use crate::updater::{BETA_CHANNEL, STABLE_CHANNEL};

const SECTION_NAV_WIDTH: f32 = 170.0;
const KEY_WIDTH: f32 = 110.0;
pub(super) const ERROR_TEXT: Color = iced::color!(0xFF8A94);
const WEAPON_SLOT_WIDTH: f32 = 148.0;
const WEAPON_SLOT_HEIGHT: f32 = 36.0;
const WEAPON_PICKER_WIDTH: f32 = 452.0;
const OPEN_SLOT_BORDER: Color = iced::color!(0x3A4250);
const CURRENT_PILL_BORDER: Color = iced::color!(0xFF4F5E, 0x55 as f32 / 255.0);

/// Sits beside the scrolling page, so it stays in view.
pub(super) fn section_nav(app: &PrimeApp) -> Element<'_, Message> {
    SettingsSection::ALL
        .into_iter()
        .filter(|section| *section != SettingsSection::Experiments || app.experiments_shown())
        .fold(column![].spacing(2), |nav, section| {
            nav.push(section_nav_item(section, app.settings_section == section))
        })
        .width(SECTION_NAV_WIDTH)
        .into()
}

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    column![
        section(
            SettingsSection::RiotClient,
            "Prime restores each account's remembered login into Riot Client's Data folder, \
             then launches VALORANT through RiotClientServices.exe. Routing its chat through \
             Prime lets you appear online, on mobile or invisible; a change applies from the \
             next launch.",
            column![
                riot_client_path_controls(app),
                checkbox(app.state.chat_proxy)
                    .label("Route chat through Prime to choose how friends see you")
                    .on_toggle(Message::ChatProxyToggled)
                    .text_size(13),
            ]
            .spacing(12)
            .into(),
        ),
        section(
            SettingsSection::SystemTray,
            "Keeps Prime running when you close it, so it refreshes every account's sign-in in \
             the background. Click the tray icon to open it again, or right-click it to quit.",
            checkbox(app.state.minimize_on_close)
                .label("Keep Prime in the system tray when closed")
                .on_toggle(Message::MinimizeOnCloseToggled)
                .text_size(13)
                .into(),
        ),
        section(
            SettingsSection::LiveMatch,
            "The weapons whose skins show beside each player, left to right. Click a column to \
             change it.",
            live_match_weapon_controls(app, WEAPON_SLOT_WIDTH),
        ),
        section(
            SettingsSection::Storage,
            "Profiles hold session tokens only, never passwords.",
            storage_controls(app),
        ),
        section(
            SettingsSection::Updates,
            "Prime checks for new releases on startup and when its window is opened again.",
            column![update_controls(app), channel_controls(app)]
                .spacing(12)
                .into(),
        ),
        if app.experiments_shown() {
            section(
                SettingsSection::Experiments,
                "Beta features still being worked on, on until you switch them off. Each can \
                 change or go away.",
                experiment_controls(app),
            )
        } else {
            space().into()
        },
        section(
            SettingsSection::Advanced,
            "Import a Riot web redirect token for API access when a captured launcher session \
             isn't available.",
            token_import_controls(app),
        ),
    ]
    .width(Length::Fill)
    .into()
}

/// A checkbox per experiment.
pub(super) fn experiment_controls(app: &PrimeApp) -> Element<'_, Message> {
    [
        (
            AIM_MODES_EXPERIMENT,
            "Aim Trainer: 3D view and Windows sensitivity",
        ),
        (
            INTERFACE_REFRESH_EXPERIMENT,
            "Interface refresh: new spinner, sign-in dialog and placeholders",
        ),
        (SETTINGS_PAGES_EXPERIMENT, "Settings: one page per section"),
    ]
    .into_iter()
    .fold(column![].spacing(10), |controls, (name, label)| {
        controls.push(
            checkbox(app.experiment(name))
                .label(label)
                .on_toggle(move |on| Message::ExperimentToggled(name, on))
                .text_size(13),
        )
    })
    .into()
}

fn section_id(section: SettingsSection) -> Id {
    Id::new(match section {
        SettingsSection::RiotClient => "settings-riot-client",
        SettingsSection::SystemTray => "settings-system-tray",
        SettingsSection::LiveMatch => "settings-live-match",
        SettingsSection::Storage => "settings-storage",
        SettingsSection::Updates => "settings-updates",
        SettingsSection::Experiments => "settings-experiments",
        SettingsSection::Advanced => "settings-advanced",
    })
}

/// Scrolls the main panel so the section's top is at the top of the view.
pub(in crate::ui) fn scroll_to_settings_section(section: SettingsSection) -> Task<Message> {
    operate(FindSectionOffset {
        section: section_id(section),
        content_top: None,
        section_top: None,
    })
    .then(|y| {
        scroll_to(
            MAIN_PANEL_SCROLLABLE_ID,
            AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        )
    })
}

/// Finds how far down the main panel's content a section starts. Layout bounds ignore the
/// current scroll, so the difference is the offset to scroll to.
struct FindSectionOffset {
    section: Id,
    content_top: Option<f32>,
    section_top: Option<f32>,
}

impl Operation<f32> for FindSectionOffset {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<f32>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        _bounds: Rectangle,
        content_bounds: Rectangle,
        _translation: Vector,
        _state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&Id::new(MAIN_PANEL_SCROLLABLE_ID)) {
            self.content_top = Some(content_bounds.y);
        }
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&self.section) {
            self.section_top = Some(bounds.y);
        }
    }

    fn finish(&self) -> Outcome<f32> {
        match (self.content_top, self.section_top) {
            (Some(content), Some(section)) => Outcome::Some(section - content),
            _ => Outcome::None,
        }
    }
}

fn section_nav_item(section: SettingsSection, selected: bool) -> Element<'static, Message> {
    let (color, font) = if selected {
        (theme::TEXT, theme::SEMIBOLD_FONT)
    } else {
        (theme::MUTED, theme::MEDIUM_FONT)
    };

    button(text(section.to_string()).size(13).font(font).color(color))
        .padding([7, 10])
        .width(Length::Fill)
        .style(move |_, status| {
            let background = if selected {
                Some(theme::SURFACE.into())
            } else if hovered(status) {
                Some(
                    Color {
                        a: 0.5,
                        ..theme::SURFACE
                    }
                    .into(),
                )
            } else {
                None
            };

            iced::widget::button::Style {
                background,
                border: iced::border::rounded(6),
                ..Default::default()
            }
        })
        .on_press(Message::SettingsSectionSelected(section))
        .into()
}

/// A titled section with a rule under it, except the last.
fn section<'a>(
    section: SettingsSection,
    description: &'a str,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    let body = container(
        column![
            column![
                text(section.to_string())
                    .size(15)
                    .font(theme::SEMIBOLD_FONT),
                text(description).size(13).color(theme::MUTED)
            ]
            .spacing(3),
            content
        ]
        .spacing(14),
    )
    .padding([20, 0])
    .width(Length::Fill)
    .id(section_id(section));

    if section == SettingsSection::Advanced {
        return body.into();
    }

    column![
        body,
        container(space())
            .width(Length::Fill)
            .height(1)
            .style(|_| container::Style::default().background(theme::LINE))
    ]
    .into()
}

fn field_label(label: &str) -> Element<'_, Message> {
    text(label)
        .size(12)
        .font(theme::SEMIBOLD_FONT)
        .color(theme::MUTED)
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
        Some(label) if !app.refreshed() => text(label).size(13).font(theme::SEMIBOLD_FONT).into(),
        Some(label) => row![
            spinner(true, app.loading_frame, 14.0, theme::MUTED),
            text(label).size(13).font(theme::SEMIBOLD_FONT)
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center)
        .into(),
        None => idle,
    }
}

fn riot_client_path_controls(app: &PrimeApp) -> Element<'_, Message> {
    let typed = typed_riot_client_path(&app.riot_client_path_input);
    // ponytail: one stat per redraw; cache it in state if the Settings tab ever animates.
    let missing = typed.as_deref().is_some_and(|path| !path.is_file());
    let can_save = app.riot_client_path_unsaved() && !missing;

    let mut input = iced::widget::text_input(
        r"C:\Riot Games\Riot Client\RiotClientServices.exe",
        &app.riot_client_path_input,
    )
    .font(theme::MONO_FONT)
    .line_height(theme::MONO_LINE_HEIGHT)
    .size(12)
    .padding(Padding {
        top: 10.0,
        right: 36.0,
        bottom: 10.0,
        left: 12.0,
    })
    .style(theme::field_style(theme::SURFACE, missing))
    .on_input(Message::RiotClientPathChanged);
    if can_save {
        input = input.on_submit(Message::SaveSettings);
    }

    let status_icon: Element<_> = match (&typed, missing) {
        (None, _) => space().into(),
        (Some(_), true) => theme::icon(Icon::CircleX, 15.0, theme::ACCENT),
        (Some(_), false) => theme::icon(Icon::CircleCheck, 15.0, theme::OK),
    };
    let field = stack![
        input,
        container(status_icon)
            .padding(Padding::ZERO.right(12))
            .align_right(Length::Fill)
            .center_y(Length::Fill)
    ]
    .width(Length::Fill);

    let note = if missing {
        text("No file at this path. Riot Client is usually in C:\\Riot Games\\Riot Client.")
            .size(12)
            .color(ERROR_TEXT)
    } else {
        text("Leave empty to find it automatically.")
            .size(11)
            .color(theme::FAINT)
    };

    column![
        field_label("RiotClientServices.exe path"),
        row![
            field,
            plain_button("Browse").on_press(Message::BrowseRiotClientPath),
            button(text("Save").size(13).font(theme::SEMIBOLD_FONT))
                .padding([8, 14])
                .style(theme::primary_button_style)
                .on_press_maybe(can_save.then_some(Message::SaveSettings))
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center),
        note
    ]
    .spacing(6)
    .into()
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

fn storage_controls(app: &PrimeApp) -> Element<'_, Message> {
    let usage = app.image_cache_usage;
    let images = if usage.files == 1 { "image" } else { "images" };

    let cache = container(
        row![
            text("Image cache").size(13).font(theme::SEMIBOLD_FONT),
            space().width(Length::Fill),
            // A made-up zero would show until the cache is measured.
            text(if app.image_cache_measured {
                format!("{} · {} {images}", format_bytes(usage.bytes), usage.files)
            } else {
                "\u{2014}".to_string()
            })
            .size(12)
            .font(theme::MONO_FONT)
            .line_height(theme::MONO_LINE_HEIGHT)
            .color(theme::MUTED),
            button(busy_label(
                app,
                app.image_cache_clearing.then_some("Clearing\u{2026}"),
                text("Clear cache")
                    .size(13)
                    .font(theme::SEMIBOLD_FONT)
                    .into(),
            ))
            .padding([8, 14])
            .style(clear_cache_style)
            .on_press_maybe((!app.image_cache_clearing).then_some(Message::ClearImageCache))
        ]
        .spacing(14)
        .align_y(alignment::Vertical::Center),
    )
    .padding(14)
    .width(Length::Fill)
    .style(|_| card_style(10.0));

    let logs = container(
        row![
            column![
                text("Logs").size(13).font(theme::SEMIBOLD_FONT),
                text(LOGS_HELP).size(12).color(theme::MUTED),
            ]
            .spacing(3)
            .width(Length::Fill),
            export_logs_button(app),
        ]
        .spacing(14)
        .align_y(alignment::Vertical::Center),
    )
    .padding(14)
    .width(Length::Fill)
    .style(|_| card_style(10.0));

    column![
        column![
            path_row("Profiles", app.repo.path()),
            path_row("Image cache", app.image_cache.path())
        ]
        .spacing(10),
        cache,
        logs
    ]
    .spacing(14)
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

/// A labelled path with a button that shows it in Explorer.
fn path_row<'a>(label: &'a str, path: &Path) -> Element<'a, Message> {
    row![
        text(label).size(13).color(theme::MUTED).width(KEY_WIDTH),
        text(short_path(path))
            .size(12)
            .font(theme::MONO_FONT)
            .line_height(theme::MONO_LINE_HEIGHT)
            .width(Length::Fill),
        iced::widget::button(theme::icon(Icon::FolderOpen, 15.0, theme::FAINT))
            .padding(0)
            .style(|_, _| iced::widget::button::Style::default())
            .on_press(Message::OpenInExplorer(path.to_path_buf()))
    ]
    .spacing(12)
    .align_y(alignment::Vertical::Center)
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

fn token_import_controls(app: &PrimeApp) -> Element<'_, Message> {
    if !app.token_import_open {
        let mut header = row![
            theme::icon(Icon::ChevronRight, 15.0, theme::MUTED),
            text("API token import")
                .size(13)
                .font(theme::SEMIBOLD_FONT)
                .width(Length::Fill)
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center);

        if !app.client_version_input.is_empty() {
            header = header.push(
                text(format!("Client {}", app.client_version_input))
                    .size(11)
                    .font(theme::MONO_FONT)
                    .line_height(theme::MONO_LINE_HEIGHT)
                    .color(theme::FAINT),
            );
        }

        return iced::widget::button(header)
            .padding([10, 12])
            .width(Length::Fill)
            .style(|_, status| iced::widget::button::Style {
                background: hovered(status).then(|| theme::SURFACE.into()),
                text_color: theme::TEXT,
                border: iced::Border {
                    color: theme::LINE,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            })
            .on_press(Message::ToggleTokenImport)
            .into();
    }

    let header = iced::widget::button(
        row![
            theme::icon(Icon::ChevronDown, 15.0, theme::MUTED),
            text("API token import")
                .size(13)
                .font(theme::SEMIBOLD_FONT)
                .width(Length::Fill),
            container(
                text("Fallback")
                    .size(10)
                    .font(theme::BOLD_FONT)
                    .color(theme::GOLD)
            )
            .padding([2, 7])
            .style(|_| {
                container::Style::default()
                    .background(Color {
                        a: 0.13,
                        ..theme::GOLD
                    })
                    .border(iced::border::rounded(4))
            })
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center),
    )
    .padding(0)
    .style(|_, _| iced::widget::button::Style {
        text_color: theme::TEXT,
        ..Default::default()
    })
    .on_press(Message::ToggleTokenImport);

    let redirect = theme::text_input(
        "https://playvalorant.com/opt_in#access_token=…",
        &app.redirect_input,
    )
    .font(theme::MONO_FONT)
    .line_height(theme::MONO_LINE_HEIGHT)
    .size(12)
    .on_input(Message::RedirectChanged);

    let client_version = theme::text_input(
        "For example release-10.00-shipping-…",
        &app.client_version_input,
    )
    .font(theme::MONO_FONT)
    .line_height(theme::MONO_LINE_HEIGHT)
    .size(12)
    .on_input(Message::ClientVersionChanged);

    let account = app.state.selected_account();
    let note = match account {
        Some(account) => format!(
            "Imports into {}. Tokens are saved with the account and redacted from logs.",
            account.display_name
        ),
        None => "Select an account first; the token is imported into the selected one.".to_string(),
    };
    let can_import = account.is_some() && !app.redirect_input.trim().is_empty();

    container(
        column![
            header,
            column![field_label("Redirect URL"), redirect].spacing(6),
            row![
                column![field_label("Client version"), client_version]
                    .spacing(6)
                    .width(Length::Fill),
                button(busy_label(
                    app,
                    app.client_version_fetching.then_some("Fetching\u{2026}"),
                    theme::icon_label(Icon::RefreshCw, "Fetch latest", theme::TEXT),
                ))
                .padding([9, 12])
                .on_press_maybe(
                    (!app.client_version_fetching).then_some(Message::RefreshClientVersion)
                )
            ]
            .spacing(10)
            .align_y(alignment::Vertical::Bottom),
            row![
                text(note).size(12).color(theme::FAINT).width(Length::Fill),
                button(text("Import token").size(13).font(theme::SEMIBOLD_FONT))
                    .padding([8, 14])
                    .style(theme::primary_button_style)
                    .on_press_maybe(can_import.then_some(Message::ImportRedirect))
            ]
            .spacing(12)
            .align_y(alignment::Vertical::Center)
        ]
        .spacing(12),
    )
    .padding(14)
    .width(Length::Fill)
    .style(|_| card_style(10.0))
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
