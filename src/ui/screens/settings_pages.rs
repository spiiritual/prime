//! Settings as one page per section, behind the settings-pages beta experiment.

use std::path::Path;

use iced::widget::{column, container, row, space, stack, toggler};
use iced::{Color, Element, Length, Padding, alignment};

use super::settings::{
    ERROR_TEXT, busy_label, channel_controls, clear_cache_style, experiment_controls, hovered,
    live_match_weapon_controls, short_path, update_controls,
};
use crate::riot::chat_proxy::PresenceStatus;
use crate::ui::components::card_style;
use crate::ui::data::{format_bytes, typed_riot_client_path};
use crate::ui::shell::{presence_label, presence_mark};
use crate::ui::theme::{self, Icon, button, text};
use crate::ui::{AppUpdateStatus, Message, PrimeApp, SettingsPage};

const SECTION_NAV_WIDTH: f32 = 190.0;
const WEAPON_SLOT_WIDTH: f32 = 132.0;

/// Sits beside the page and switches which page shows.
pub(super) fn section_nav(app: &PrimeApp) -> Element<'_, Message> {
    SettingsPage::ALL
        .into_iter()
        .fold(column![].spacing(2), |nav, page| {
            nav.push(section_nav_item(page, app.settings_page == page))
        })
        .width(SECTION_NAV_WIDTH)
        .into()
}

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    let section = app.settings_page;
    let (description, rows): (&str, Vec<Element<'_, Message>>) = match section {
        SettingsPage::General => (
            "How Prime starts, closes and finds Riot Client.",
            vec![
                setting_row(
                    "Keep Prime in the system tray when closed",
                    "Prime keeps every account's sign-in fresh in the background. Right-click \
                     the tray icon to quit.",
                    switch(app.state.minimize_on_close, Message::MinimizeOnCloseToggled),
                ),
                stacked_row(
                    "Riot Client location",
                    "Leave empty and Prime finds it. Saved as soon as the file exists.",
                    riot_client_path_controls(app),
                ),
            ],
        ),
        SettingsPage::Status => (
            "Choose how friends see you while you play.",
            vec![
                setting_row(
                    "Route chat through Prime",
                    "Needed to appear Mobile or Invisible. Applies from the next launch.",
                    switch(app.state.chat_proxy, Message::ChatProxyToggled),
                ),
                setting_row(
                    "Status friends see",
                    if app.state.chat_proxy {
                        "Also in the sidebar, above the version."
                    } else {
                        "Turn on routing chat through Prime to choose."
                    },
                    status_choices(app),
                ),
            ],
        ),
        SettingsPage::LiveMatch => (
            "What the Live Match page shows for each player.",
            vec![stacked_row(
                "Skin columns",
                "The weapons whose skins show beside each player, left to right. Click one to \
                 change it.",
                live_match_weapon_controls(app, WEAPON_SLOT_WIDTH),
            )],
        ),
        SettingsPage::Storage => (
            "Where Prime keeps its data. Profiles hold session tokens only, never passwords.",
            storage_rows(app),
        ),
        SettingsPage::About => (
            "Versions of Prime and VALORANT.",
            vec![
                update_row(app),
                container(channel_controls(app)).padding([16, 0]).into(),
                client_version_row(app),
            ],
        ),
        SettingsPage::Experiments => (
            "Beta features still being worked on, on until you switch them off. Each can \
             change or go away.",
            vec![container(experiment_controls(app)).padding([16, 0]).into()],
        ),
        SettingsPage::Advanced => (
            "Fallbacks for when a captured login isn't enough.",
            vec![stacked_row(
                "API token import",
                "Import a Riot web redirect token when a captured launcher session isn't \
                 available.",
                token_import_controls(app),
            )],
        ),
    };

    let head = column![
        text(section.to_string())
            .size(18)
            .font(theme::SEMIBOLD_FONT),
        text(description).size(13).color(theme::MUTED)
    ]
    .spacing(4)
    .padding(Padding::ZERO.bottom(8));

    let count = rows.len();
    rows.into_iter()
        .enumerate()
        .fold(column![head], |page, (index, row)| {
            let page = page.push(row);
            if index + 1 < count {
                page.push(rule())
            } else {
                page
            }
        })
        .width(Length::Fill)
        .into()
}

fn section_icon(section: SettingsPage) -> Icon {
    match section {
        SettingsPage::General => Icon::Settings2,
        SettingsPage::Status => Icon::EyeOff,
        SettingsPage::LiveMatch => Icon::Swords,
        SettingsPage::Storage => Icon::HardDrive,
        SettingsPage::About => Icon::Info,
        SettingsPage::Experiments => Icon::Sparkles,
        SettingsPage::Advanced => Icon::Wrench,
    }
}

fn section_nav_item(section: SettingsPage, selected: bool) -> Element<'static, Message> {
    let (color, icon_color, font) = if selected {
        (theme::TEXT, theme::TEXT, theme::SEMIBOLD_FONT)
    } else {
        (theme::MUTED, theme::FAINT, theme::MEDIUM_FONT)
    };

    button(
        row![
            theme::icon(section_icon(section), 15.0, icon_color),
            text(section.to_string()).size(13).font(font).color(color)
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center),
    )
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
    .on_press(Message::SettingsPageSelected(section))
    .into()
}

fn rule() -> Element<'static, Message> {
    container(space())
        .width(Length::Fill)
        .height(1)
        .style(|_| container::Style::default().background(theme::LINE))
        .into()
}

fn setting_info<'a>(label: &'a str, help: &'a str) -> iced::widget::Column<'a, Message> {
    column![
        text(label).size(13).font(theme::SEMIBOLD_FONT),
        text(help).size(12).color(theme::MUTED)
    ]
    .spacing(3)
}

/// A setting's name and help on the left, its control on the right.
fn setting_row<'a>(
    label: &'a str,
    help: &'a str,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    row![setting_info(label, help).width(Length::Fill), control]
        .spacing(24)
        .padding([16, 0])
        .align_y(alignment::Vertical::Center)
        .into()
}

/// A setting whose control is too wide to sit beside its name.
fn stacked_row<'a>(
    label: &'a str,
    help: &'a str,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    column![setting_info(label, help), control]
        .spacing(12)
        .padding([16, 0])
        .width(Length::Fill)
        .into()
}

fn switch<'a>(on: bool, message: fn(bool) -> Message) -> Element<'a, Message> {
    toggler(on)
        .size(20)
        .on_toggle(message)
        .style(|_, status| {
            let on = match status {
                toggler::Status::Active { is_toggled }
                | toggler::Status::Hovered { is_toggled }
                | toggler::Status::Disabled { is_toggled } => is_toggled,
            };
            toggler::Style {
                background: if on { theme::ACCENT } else { theme::LINE }.into(),
                background_border_width: 0.0,
                background_border_color: Color::TRANSPARENT,
                foreground: if on { Color::WHITE } else { theme::MUTED }.into(),
                foreground_border_width: 0.0,
                foreground_border_color: Color::TRANSPARENT,
                text_color: None,
                border_radius: None,
                padding_ratio: 0.1,
            }
        })
        .into()
}

/// The same choice as the sidebar's status menu, usable only while chat goes through Prime.
fn status_choices(app: &PrimeApp) -> Element<'_, Message> {
    let enabled = app.state.chat_proxy;
    let current = app.state.presence_status;
    let choices = PresenceStatus::ALL
        .into_iter()
        .fold(row![].spacing(2), |choices, status| {
            let selected = status == current;
            let label = text(presence_label(status))
                .size(12)
                .font(if selected {
                    theme::SEMIBOLD_FONT
                } else {
                    theme::MEDIUM_FONT
                })
                .color(if selected && enabled {
                    theme::TEXT
                } else {
                    theme::MUTED
                });
            choices.push(
                iced::widget::button(
                    row![presence_mark(status), label]
                        .spacing(6)
                        .align_y(alignment::Vertical::Center),
                )
                .padding([6, 12])
                .style(move |_, button_status| iced::widget::button::Style {
                    background: (selected || (enabled && hovered(button_status)))
                        .then(|| theme::RAISED.into()),
                    border: iced::border::rounded(6),
                    ..Default::default()
                })
                .on_press_maybe(enabled.then_some(Message::PresenceStatusPicked(status))),
            )
        });

    container(choices)
        .padding(3)
        .style(|_| card_style(8.0))
        .into()
}

fn open_folder_button(path: &Path) -> iced::widget::Button<'static, Message> {
    button(theme::icon_label(Icon::FolderOpen, "Open", theme::TEXT))
        .padding([8, 14])
        .on_press(Message::OpenInExplorer(path.to_path_buf()))
}

fn mono_detail(value: String) -> iced::widget::Text<'static> {
    text(value)
        .size(12)
        .font(theme::MONO_FONT)
        .line_height(theme::MONO_LINE_HEIGHT)
        .color(theme::MUTED)
}

fn riot_client_path_controls(app: &PrimeApp) -> Element<'_, Message> {
    let typed = typed_riot_client_path(&app.riot_client_path_input);
    // ponytail: one stat per redraw; cache it in state if the Settings tab ever animates.
    let missing = typed.as_deref().is_some_and(|path| !path.is_file());

    let input = iced::widget::text_input(
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

    let controls = row![
        field,
        button(theme::icon_label(Icon::FolderOpen, "Browse", theme::TEXT))
            .padding([8, 14])
            .on_press(Message::BrowseRiotClientPath)
    ]
    .spacing(10)
    .align_y(alignment::Vertical::Center);

    if !missing {
        return controls.into();
    }
    column![
        controls,
        text(
            "No file at this path, so it isn't saved. Riot Client is usually in \
             C:\\Riot Games\\Riot Client."
        )
        .size(12)
        .color(ERROR_TEXT)
    ]
    .spacing(6)
    .into()
}

fn storage_rows(app: &PrimeApp) -> Vec<Element<'_, Message>> {
    let usage = app.image_cache_usage;
    let images = if usage.files == 1 { "image" } else { "images" };
    let data_folder = app.repo.path().parent().unwrap_or(app.repo.path());

    vec![
        setting_row(
            "Data folder",
            "Accounts, captured logins and settings profiles.",
            row![
                mono_detail(short_path(data_folder)),
                open_folder_button(data_folder)
            ]
            .spacing(12)
            .align_y(alignment::Vertical::Center)
            .into(),
        ),
        setting_row(
            "Image cache",
            "Skin and bundle art. Clearing it only means images download again.",
            row![
                // A made-up zero would show until the cache is measured.
                mono_detail(if app.image_cache_measured {
                    format!("{} · {} {images}", format_bytes(usage.bytes), usage.files)
                } else {
                    "\u{2014}".to_string()
                }),
                open_folder_button(app.image_cache.path()),
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
            .spacing(10)
            .align_y(alignment::Vertical::Center)
            .into(),
        ),
    ]
}

fn update_row(app: &PrimeApp) -> Element<'_, Message> {
    let current = crate::updater::CURRENT_VERSION;

    // The same card as the single page.
    if app.app_update_status.pending_update().is_some() {
        return container(update_controls(app)).padding([16, 0]).into();
    }

    let status = match &app.app_update_status {
        AppUpdateStatus::Checking => "Checking for updates…".to_string(),
        AppUpdateStatus::UpToDate => "You're up to date".to_string(),
        AppUpdateStatus::Downloading(update) => {
            format!("Downloading {}…", update.latest_version)
        }
        AppUpdateStatus::Installing => "Restarting to install the update…".to_string(),
        AppUpdateStatus::NotInstalled => {
            "Not an installed copy, so updates don't apply".to_string()
        }
        AppUpdateStatus::CheckFailed(error) => format!("Update check failed: {error}"),
        AppUpdateStatus::InstallFailed { error, .. } => format!("Update failed: {error}"),
        // Shown as the card above.
        AppUpdateStatus::Available(_) | AppUpdateStatus::Dismissed(_) => String::new(),
    };

    row![
        column![
            text(format!("Prime {current}"))
                .size(13)
                .font(theme::SEMIBOLD_FONT),
            text(status).size(12).color(theme::MUTED)
        ]
        .spacing(3)
        .width(Length::Fill),
        button(busy_label(
            app,
            matches!(app.app_update_status, AppUpdateStatus::Checking)
                .then_some("Checking\u{2026}"),
            theme::icon_label(Icon::RefreshCw, "Check for updates", theme::TEXT),
        ))
        .padding([8, 14])
        .on_press_maybe((!app.app_update_status.is_busy()).then_some(Message::CheckForAppUpdate))
    ]
    .spacing(24)
    .padding([16, 0])
    .align_y(alignment::Vertical::Center)
    .into()
}

/// Editable, so a version can still be typed in when fetching it fails.
fn client_version_row(app: &PrimeApp) -> Element<'_, Message> {
    let client_version = theme::text_input("release-…", &app.client_version_input)
        .font(theme::MONO_FONT)
        .line_height(theme::MONO_LINE_HEIGHT)
        .size(12)
        .width(280)
        .on_input(Message::ClientVersionChanged);

    setting_row(
        "VALORANT client version",
        "Fetched at startup. Shop and Loadout need it.",
        row![
            client_version,
            button(busy_label(
                app,
                app.client_version_fetching.then_some("Fetching\u{2026}"),
                theme::icon_label(Icon::RefreshCw, "Fetch latest", theme::TEXT),
            ))
            .padding([8, 14])
            .on_press_maybe(
                (!app.client_version_fetching).then_some(Message::RefreshClientVersion)
            )
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center)
        .into(),
    )
}

fn token_import_controls(app: &PrimeApp) -> Element<'_, Message> {
    let redirect = theme::text_input(
        "https://playvalorant.com/opt_in#access_token=…",
        &app.redirect_input,
    )
    .font(theme::MONO_FONT)
    .line_height(theme::MONO_LINE_HEIGHT)
    .size(12)
    .on_input(Message::RedirectChanged);

    let account = app.state.selected_account();
    let note = match account {
        Some(account) => format!(
            "Imports into {}. Tokens are saved with the account and redacted from logs.",
            account.display_name
        ),
        None => "Select an account first; the token is imported into the selected one.".to_string(),
    };
    let can_import = account.is_some() && !app.redirect_input.trim().is_empty();

    column![
        column![
            text("Redirect URL")
                .size(12)
                .font(theme::SEMIBOLD_FONT)
                .color(theme::MUTED),
            redirect
        ]
        .spacing(6),
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
    .spacing(12)
    .into()
}
