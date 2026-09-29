use std::f32::consts::FRAC_PI_2;

use iced::gradient::Linear;
use iced::widget::image::Handle;
use iced::widget::text::Wrapping;
use iced::widget::{Column, checkbox, column, container, image, row, space, stack, tooltip};
use iced::{Color, ContentFit, Element, Font, Length, Padding, Radians, Theme, alignment};
use time::{OffsetDateTime, UtcOffset};

use crate::account::{AccountId, AccountProfile, CompetitiveRank};

use crate::ui::components::{
    account_avatar, anchored_popover, card_style, compact_loading_indicator, mono, skeleton,
};
use crate::ui::data::account_details::AccountAvailability;
use crate::ui::shell::popover_style;
use crate::ui::theme::{self, Icon, button, text};
use crate::ui::{AccountsTab, LauncherCaptureKind, Message, PrimeApp};

const HERO_HEIGHT: f32 = 208.0;
const HERO_BASE: Color = iced::color!(0x141821);
const HERO_SCRIM: Color = iced::color!(0x0E1016);
/// Riot's wide player card art is 452x128. Fitting it to the banner's height instead of its width
/// stretches it about 1.6x rather than 2.2x, which keeps it noticeably sharper.
const WIDE_ART_WIDTH: f32 = (HERO_HEIGHT - 2.0) * 452.0 / 128.0;
const TAB_STRIP_HEIGHT: f32 = 36.0;
const RANK_COLUMN_WIDTH: f32 = 130.0;
const LEVEL_COLUMN_WIDTH: f32 = 64.0;
const STATUS_COLUMN_WIDTH: f32 = 190.0;
const ACTIONS_COLUMN_WIDTH: f32 = 150.0;
const ROW_AVATAR_SIZE: f32 = 36.0;
const PRESENCE_DOT_SIZE: f32 = 10.0;
const ACCOUNT_MENU_WIDTH: f32 = 232.0;
/// Where the menu opens under its row, and how far in from the row's right edge.
const ACCOUNT_MENU_TOP_OFFSET: f32 = 44.0;
const ACCOUNT_MENU_RIGHT_INSET: f32 = 8.0;
const MENU_ITEM_HOVER: Color = iced::color!(0x262C38);
const DANGER_TEXT: Color = iced::color!(0xFF8A94);
const CAPTURE_TEXT: Color = iced::color!(0x231A05);
const SEARCH_WIDTH: f32 = 220.0;
const EMPTY_OPTION_WIDTH: f32 = 250.0;
/// All three first-account options take the tallest one's height, the three-line Add account.
const EMPTY_OPTION_HEIGHT: f32 = 158.0;
const DISPLAY_MEDIUM_FONT: Font = Font {
    weight: iced::font::Weight::Medium,
    ..theme::DISPLAY_FONT
};

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    if app.state.accounts.is_empty() {
        return first_account(app);
    }

    let content = if app.settings_cloning && app.active_accounts_tab == AccountsTab::GameSettings {
        super::game_settings::tab(app)
    } else {
        account_list(app)
    };

    let mut page = Column::new().spacing(20).width(Length::Fill);
    if let Some(account) = app.state.selected_account() {
        page = page.push(hero(app, account));
    }
    page.push(tab_strip(app)).push(content).into()
}

/// With no accounts, the page is the centred first-account prompt and has no title.
pub(super) fn fills_page(app: &PrimeApp) -> bool {
    app.state.accounts.is_empty()
}

/// Import, Add current and Add account, beside the page title.
pub(super) fn header_actions(app: &PrimeApp) -> Element<'_, Message> {
    let capture_idle = !app.launcher_capture_in_progress && !app.launch_in_progress();
    let capturing = |kind| app.launcher_capture_kind == Some(kind);

    row![
        header_button(
            Icon::Download,
            "Import",
            None,
            (!app.import_account_in_progress && capture_idle).then_some(Message::OpenImportAccount),
            false,
        ),
        header_button(
            Icon::LogIn,
            "Add current",
            capturing(LauncherCaptureKind::Current).then_some(app.loading_frame),
            capture_idle.then_some(Message::AddCurrentAccount),
            false,
        ),
        header_button(
            Icon::Plus,
            "Add account",
            capturing(LauncherCaptureKind::New).then_some(app.loading_frame),
            capture_idle.then_some(Message::AddAccount),
            true,
        ),
    ]
    .spacing(8)
    .into()
}

/// A header button; `capturing` swaps its icon for a spinner while its capture runs.
fn header_button(
    icon: Icon,
    label: &'static str,
    capturing: Option<usize>,
    on_press: Option<Message>,
    primary: bool,
) -> Element<'static, Message> {
    let color = match (primary, on_press.is_some()) {
        (true, true) => theme::BG,
        (true, false) => Color {
            a: 0.4,
            ..theme::BG
        },
        (false, true) => theme::TEXT,
        (false, false) => theme::FAINT,
    };
    let lead = match capturing {
        Some(frame) => compact_loading_indicator(frame),
        None => theme::icon(icon, 15.0, color),
    };

    button(
        row![lead, text(label).size(13).font(theme::SEMIBOLD_FONT)]
            .spacing(8)
            .align_y(alignment::Vertical::Center),
    )
    .padding([9, 14])
    .style(if primary {
        theme::primary_button_style
    } else {
        theme::button_style
    })
    .on_press_maybe(on_press)
    .into()
}

/// The selected account over its player card's wide art, with Launch.
fn hero<'a>(app: &'a PrimeApp, account: &'a AccountProfile) -> Element<'a, Message> {
    let login_captured = account.has_launcher_session();

    let mut name = row![
        text(&account.display_name)
            .size(40)
            .font(theme::DISPLAY_FONT)
            .line_height(1.0)
            .wrapping(Wrapping::None)
    ]
    .spacing(10)
    .align_y(alignment::Vertical::Bottom);
    if let Some(tag) = tag_label(account) {
        name = name.push(
            text(tag)
                .size(22)
                .font(DISPLAY_MEDIUM_FONT)
                .line_height(1.2)
                .color(theme::FAINT)
                .wrapping(Wrapping::None),
        );
    }

    let rank_loading = app.account_ranks_loading.contains(&account.id);
    let rank: Element<_> = match (account.competitive_rank.as_ref(), app.rank_icon(account)) {
        (None, _) if rank_loading => {
            row![skeleton(18, 18.0, 9.0, 1.0), skeleton(70, 14.0, 4.0, 1.0)]
                .spacing(6)
                .align_y(alignment::Vertical::Center)
                .into()
        }
        (Some(rank), icon) => row![rank_icon(icon, 18.0)]
            .push(meta_value(&rank.rank_name).color(rank_color(rank)))
            .spacing(6)
            .align_y(alignment::Vertical::Center)
            .into(),
        (None, icon) => row![rank_icon(icon, 18.0)]
            .push(meta_value(missing_rank_label(app, account.id)).color(theme::MUTED))
            .spacing(6)
            .align_y(alignment::Vertical::Center)
            .into(),
    };
    let level: Element<_> = match account.account_level.filter(|level| *level > 0) {
        Some(level) => mono(level.to_string(), 14)
            .font(theme::MONO_SEMIBOLD_FONT)
            .into(),
        None if rank_loading => skeleton(28, 14.0, 4.0, 1.0),
        None => mono("—", 14)
            .font(theme::MONO_SEMIBOLD_FONT)
            .color(theme::MUTED)
            .into(),
    };
    let saved: Element<_> = if app.profile_identity_refreshing.contains(&account.id) {
        meta_value("Refreshing…").color(theme::MUTED).into()
    } else {
        match launcher_session_captured_at_unix(account) {
            Some(captured_at) => meta_value(relative_time_label(captured_at, now_unix())).into(),
            None => meta_value("Never").color(theme::MUTED).into(),
        }
    };

    let identity = column![
        name,
        row![
            meta("RANK", rank),
            meta("LEVEL", level),
            meta("REGION", meta_value(region_label(account)).into()),
            meta("LOGIN SAVED", saved),
        ]
        .spacing(20)
        .align_y(alignment::Vertical::Center),
    ]
    .spacing(14)
    .width(Length::Fill);

    let (note_icon, note_color, note) = if login_captured {
        (Icon::ShieldCheck, theme::OK, "Launcher session captured")
    } else {
        (
            Icon::ShieldOff,
            theme::GOLD,
            "Capture this account's login to launch",
        )
    };
    let launch = column![
        launch_button(app, account),
        row![
            theme::icon(note_icon, 13.0, note_color),
            text(note).size(12).color(if login_captured {
                theme::MUTED
            } else {
                theme::GOLD
            })
        ]
        .spacing(6)
        .align_y(alignment::Vertical::Center),
    ]
    .spacing(10)
    .align_x(alignment::Horizontal::Right);

    let content = row![identity, launch]
        .spacing(20)
        .align_y(alignment::Vertical::Center);
    // The design fades the art a little less where the login still needs capturing.
    let scrim_end = if login_captured { 0.4 } else { 0.5 };

    let mut layers = stack![
        container(space())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style::default()
                .background(HERO_BASE)
                .border(iced::border::rounded(14)))
    ];
    if let Some(path) = app.player_card_wide_art_path(account) {
        // Pinned to the right, with its left edge faded into the banner. The art is rounded like the
        // banner, inside its border; its left corners sit under the opaque start of the fade.
        let fade = Linear::new(Radians(FRAC_PI_2))
            .add_stop(0.0, HERO_BASE)
            .add_stop(
                0.4,
                Color {
                    a: 0.0,
                    ..HERO_BASE
                },
            );
        layers = layers.push(
            container(
                stack![
                    image(Handle::from_path(path.clone()))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .content_fit(ContentFit::Cover)
                        .opacity(0.9)
                        .border_radius(13),
                    container(space())
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .style(move |_| container::Style::default().background(fade)),
                ]
                .width(WIDE_ART_WIDTH)
                .height(Length::Fill),
            )
            // Inside the banner's 1px border, so the art doesn't bleed around it.
            .padding(1)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Right),
        );
    }
    layers
        .push(
            container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| hero_scrim_style(scrim_end)),
        )
        .push(container(content).padding(28).center_y(Length::Fill))
        .width(Length::Fill)
        .height(HERO_HEIGHT)
        .into()
}

/// A dark scrim from the left, where the name sits, easing off to the right; and the card's
/// border, drawn here because content draws over a container's border.
fn hero_scrim_style(end_alpha: f32) -> container::Style {
    let gradient = Linear::new(Radians(FRAC_PI_2))
        .add_stop(
            0.0,
            Color {
                a: 0.98,
                ..HERO_SCRIM
            },
        )
        .add_stop(
            0.45,
            Color {
                a: 0.9,
                ..HERO_SCRIM
            },
        )
        .add_stop(
            1.0,
            Color {
                a: end_alpha,
                ..HERO_SCRIM
            },
        );

    container::Style::default()
        .background(gradient)
        .border(iced::Border {
            color: theme::LINE,
            width: 1.0,
            radius: 14.0.into(),
        })
}

fn meta<'a>(key: &'static str, value: Element<'a, Message>) -> Element<'a, Message> {
    column![
        text(key)
            .size(10)
            .font(theme::SEMIBOLD_FONT)
            .color(theme::FAINT),
        value
    ]
    .spacing(3)
    .into()
}

fn meta_value<'a>(value: impl iced::widget::text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(value)
        .size(14)
        .font(theme::SEMIBOLD_FONT)
        .wrapping(Wrapping::None)
}

/// Launch, or Capture login when there's no login to launch with.
fn launch_button<'a>(app: &'a PrimeApp, account: &'a AccountProfile) -> Element<'a, Message> {
    let busy_label = if app.launching_account == Some(account.id) {
        Some("Launching…")
    } else if app.launch_preflight_account == Some(account.id) {
        Some("Checking…")
    } else {
        None
    };
    let blocked = app.launch_in_progress() || app.launcher_capture_in_progress;

    if !account.has_launcher_session() {
        return button(
            row![
                theme::icon(Icon::LogIn, 18.0, CAPTURE_TEXT),
                text("Capture login").size(15).font(theme::BOLD_FONT)
            ]
            .spacing(10)
            .align_y(alignment::Vertical::Center),
        )
        .padding([16, 26])
        .style(|_, status| cta_style(theme::GOLD, CAPTURE_TEXT, status, false))
        .on_press_maybe((!blocked).then_some(Message::RequestLauncherSessionLogin(account.id)))
        .into();
    }

    let busy = busy_label.is_some();
    let lead = if busy {
        compact_loading_indicator(app.loading_frame)
    } else {
        let alpha = if blocked { 0.4 } else { 1.0 };
        theme::icon(
            Icon::Play,
            18.0,
            Color {
                a: alpha,
                ..theme::BG
            },
        )
    };

    button(
        row![
            lead,
            text(busy_label.unwrap_or("Launch VALORANT"))
                .size(15)
                .font(theme::BOLD_FONT)
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center),
    )
    .padding([16, 26])
    .style(move |_, status| cta_style(theme::ACCENT, theme::BG, status, busy))
    .on_press_maybe((!blocked).then_some(Message::LaunchAccount(account.id)))
    .into()
}

/// The hero's big solid button, with the accent's glow under Launch.
fn cta_style(
    color: Color,
    text_color: Color,
    status: iced::widget::button::Status,
    busy: bool,
) -> iced::widget::button::Style {
    use iced::widget::button::Status;

    let alpha = match status {
        _ if busy => 0.85,
        Status::Active => 1.0,
        Status::Hovered | Status::Pressed => 0.9,
        Status::Disabled => 0.4,
    };
    let shadow = if color == theme::ACCENT && !matches!(status, Status::Disabled) || busy {
        iced::Shadow {
            color: Color { a: 0.25, ..color },
            offset: iced::Vector::new(0.0, 8.0),
            blur_radius: 24.0,
        }
    } else {
        iced::Shadow::default()
    };

    iced::widget::button::Style {
        background: Some(Color { a: alpha, ..color }.into()),
        text_color: Color {
            a: alpha,
            ..text_color
        },
        border: iced::border::rounded(10),
        shadow,
        ..Default::default()
    }
}

/// Accounts and, with settings cloning, Settings profiles, with the list's filter at the right.
fn tab_strip(app: &PrimeApp) -> Element<'_, Message> {
    let mut tabs = row![strip_tab(
        "Accounts",
        app.state.accounts.len(),
        !app.settings_cloning || app.active_accounts_tab == AccountsTab::Accounts,
        Message::AccountsTabSelected(AccountsTab::Accounts),
    )]
    .spacing(22)
    .align_y(alignment::Vertical::Bottom);

    let game_settings_open =
        app.settings_cloning && app.active_accounts_tab == AccountsTab::GameSettings;
    if app.settings_cloning {
        let profiles = app
            .settings_profiles
            .iter()
            .filter(|profile| {
                profile.purpose == crate::game_settings::GameSettingsProfilePurpose::Profile
            })
            .count();
        tabs = tabs.push(strip_tab(
            "Settings profiles",
            profiles,
            game_settings_open,
            Message::AccountsTabSelected(AccountsTab::GameSettings),
        ));
    }

    let trailing = if game_settings_open {
        save_settings_button(app)
    } else {
        account_filter(app)
    };
    let tabs = tabs
        .push(space().width(Length::Fill))
        .push(container(trailing).padding(Padding::ZERO.bottom(8)));

    stack![
        column![
            space().height(Length::Fill),
            container(space())
                .width(Length::Fill)
                .height(1)
                .style(|_| container::Style::default().background(theme::LINE))
        ],
        tabs.height(TAB_STRIP_HEIGHT)
    ]
    .width(Length::Fill)
    .height(TAB_STRIP_HEIGHT)
    .into()
}

fn strip_tab(
    label: &'static str,
    count: usize,
    selected: bool,
    on_press: Message,
) -> Element<'static, Message> {
    let (color, font, count_color, count_background) = if selected {
        (
            theme::TEXT,
            theme::SEMIBOLD_FONT,
            theme::TEXT,
            theme::RAISED,
        )
    } else {
        (
            theme::MUTED,
            theme::MEDIUM_FONT,
            theme::FAINT,
            theme::SURFACE,
        )
    };
    let label = row![
        text(label).size(14).font(font).color(color),
        container(mono(count.to_string(), 11).color(count_color))
            .padding([1, 6])
            .style(move |_| {
                container::Style::default()
                    .background(count_background)
                    .border(iced::border::rounded(5))
            })
    ]
    .spacing(7)
    .align_y(alignment::Vertical::Center);
    let underline = container(space())
        .width(Length::Fill)
        .height(2)
        .style(move |_| {
            container::Style::default().background(if selected {
                theme::ACCENT
            } else {
                Color::TRANSPARENT
            })
        });

    iced::widget::button(
        column![container(label).padding([0, 2]), underline]
            .spacing(9)
            .width(Length::Shrink),
    )
    .padding(0)
    .style(|_, _| iced::widget::button::Style::default())
    .on_press_maybe((!selected).then_some(on_press))
    .into()
}

fn account_filter(app: &PrimeApp) -> Element<'_, Message> {
    container(
        row![
            theme::icon(Icon::Search, 14.0, theme::FAINT),
            theme::text_input("Filter accounts", &app.account_filter)
                .on_input(Message::AccountFilterChanged)
                .padding(0)
                .style(|theme, status| {
                    let mut style = theme::field_style(Color::TRANSPARENT, false)(theme, status);
                    style.border.width = 0.0;
                    style
                })
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center),
    )
    .padding([6, 12])
    .width(SEARCH_WIDTH)
    .style(|_| card_style(8.0))
    .into()
}

/// Saves the selected account's VALORANT settings as a new profile.
fn save_settings_button(app: &PrimeApp) -> Element<'_, Message> {
    let Some(account) = app.state.selected_account() else {
        return space().into();
    };
    let enabled = !app.settings_work_in_progress();
    let color = if enabled { theme::TEXT } else { theme::FAINT };

    button(
        row![
            theme::icon(Icon::Save, 14.0, color),
            text(format!("Save from {}", account.display_name))
                .size(12)
                .font(theme::SEMIBOLD_FONT)
        ]
        .spacing(7)
        .align_y(alignment::Vertical::Center),
    )
    .padding([6, 12])
    .on_press_maybe(enabled.then_some(Message::RequestSavePreset(account.id)))
    .into()
}

fn account_list(app: &PrimeApp) -> Element<'_, Message> {
    let filter = app.account_filter.trim().to_lowercase();
    let accounts: Vec<&AccountProfile> = app
        .state
        .accounts
        .iter()
        .filter(|account| filter.is_empty() || matches_filter(account, &filter))
        .collect();

    if accounts.is_empty() {
        return container(
            text(format!("No accounts match “{}”", app.account_filter.trim()))
                .size(13)
                .color(theme::MUTED),
        )
        .padding([22, 18])
        .width(Length::Fill)
        .style(|_| card_style(12.0))
        .into();
    }

    let last = accounts.len() - 1;
    let mut rows = Column::new().width(Length::Fill);
    for (index, account) in accounts.into_iter().enumerate() {
        if index > 0 {
            rows = rows.push(
                container(space())
                    .width(Length::Fill)
                    .height(1)
                    .style(|_| container::Style::default().background(theme::LINE)),
            );
        }
        rows = rows.push(account_row(app, account, index == 0, index == last));
    }

    // Inset by the border, since the rows draw over it.
    container(rows)
        .padding(1)
        .width(Length::Fill)
        .style(|_| card_style(12.0))
        .into()
}

fn matches_filter(account: &AccountProfile, filter: &str) -> bool {
    account.display_name.to_lowercase().contains(filter)
        || account
            .riot_id()
            .is_some_and(|riot_id| riot_id.to_lowercase().contains(filter))
}

fn account_row<'a>(
    app: &'a PrimeApp,
    account: &'a AccountProfile,
    first: bool,
    last: bool,
) -> Element<'a, Message> {
    let is_selected = app.state.selected_account == Some(account.id);
    let is_menu_open = app.open_account_menu == Some(account.id);
    let availability = account_availability(app, account);

    let mut name = row![
        text(&account.display_name)
            .size(14)
            .font(theme::SEMIBOLD_FONT)
            .wrapping(Wrapping::None)
    ]
    .spacing(6)
    .align_y(alignment::Vertical::Center);
    if let Some(tag) = tag_label(account) {
        name = name.push(
            text(tag)
                .size(14)
                .color(theme::FAINT)
                .wrapping(Wrapping::None),
        );
    }
    if is_selected {
        name = name.push(
            container(
                text("Selected")
                    .size(10)
                    .font(theme::BOLD_FONT)
                    .color(theme::ACCENT),
            )
            .padding([2, 6])
            .style(|_| {
                container::Style::default()
                    .background(Color {
                        a: 0x1F as f32 / 255.0,
                        ..theme::ACCENT
                    })
                    .border(iced::border::rounded(4))
            }),
        );
    }

    let identity = row![
        avatar_with_presence(app, account, &availability),
        column![
            name,
            text(format!(
                "{} · {}",
                presence_label(&availability),
                region_label(account)
            ))
            .size(12)
            .color(theme::MUTED)
            .wrapping(Wrapping::None)
        ]
        .spacing(3)
        .clip(true)
    ]
    .spacing(12)
    .align_y(alignment::Vertical::Center)
    .width(Length::Fill);

    let content = row![
        identity,
        container(rank_cell(app, account)).width(RANK_COLUMN_WIDTH),
        container(level_cell(app, account)).width(LEVEL_COLUMN_WIDTH),
        container(status_cell(app, account))
            .width(STATUS_COLUMN_WIDTH)
            .clip(true),
        container(row_actions(app, account, is_selected, is_menu_open))
            .width(ACTIONS_COLUMN_WIDTH)
            .align_x(alignment::Horizontal::Right),
    ]
    .spacing(16)
    .align_y(alignment::Vertical::Center);

    // The inner corners of the list's 12px border.
    let radius = iced::border::Radius {
        top_left: if first { 11.0 } else { 0.0 },
        top_right: if first { 11.0 } else { 0.0 },
        bottom_right: if last { 11.0 } else { 0.0 },
        bottom_left: if last { 11.0 } else { 0.0 },
    };
    let row = container(content)
        // 64px a row, counting the divider under it as the design does.
        .padding(Padding {
            top: 14.0,
            right: 18.0,
            bottom: if last { 14.0 } else { 13.0 },
            left: 18.0,
        })
        .width(Length::Fill)
        .style(move |_| {
            let style = container::Style::default().border(iced::Border {
                radius,
                ..Default::default()
            });
            if is_selected {
                // The accent at 5% over the list's surface.
                style.background(Color {
                    a: 0x0D as f32 / 255.0,
                    ..theme::ACCENT
                })
            } else {
                style
            }
        });

    anchored_popover(
        row,
        account_menu(app, account),
        is_menu_open,
        ACCOUNT_MENU_TOP_OFFSET,
        ACCOUNT_MENU_RIGHT_INSET,
    )
}

fn avatar_with_presence<'a>(
    app: &'a PrimeApp,
    account: &'a AccountProfile,
    availability: &AccountAvailability,
) -> Element<'a, Message> {
    let color = presence_color(availability);
    let dot = container(space())
        .width(PRESENCE_DOT_SIZE)
        .height(PRESENCE_DOT_SIZE)
        .style(move |_| {
            container::Style::default()
                .background(color)
                .border(iced::Border {
                    color: theme::SURFACE,
                    width: 2.0,
                    radius: (PRESENCE_DOT_SIZE / 2.0).into(),
                })
        });

    stack![
        account_avatar(
            &account.display_name,
            app.player_card_art_path(account),
            ROW_AVATAR_SIZE,
            9.0
        ),
        container(dot)
            .width(ROW_AVATAR_SIZE)
            .height(ROW_AVATAR_SIZE)
            .align_x(alignment::Horizontal::Right)
            .align_y(alignment::Vertical::Bottom)
    ]
    .into()
}

fn rank_cell<'a>(app: &'a PrimeApp, account: &'a AccountProfile) -> Element<'a, Message> {
    let icon = app.rank_icon(account);
    let (label, color) = match &account.competitive_rank {
        Some(rank) => (rank.rank_name.as_str(), rank_color(rank)),
        None if app.account_ranks_loading.contains(&account.id) => {
            return row![skeleton(20, 20.0, 10.0, 1.0), skeleton(70, 10.0, 4.0, 1.0)]
                .spacing(8)
                .align_y(alignment::Vertical::Center)
                .into();
        }
        None => (missing_rank_label(app, account.id), theme::MUTED),
    };

    row![rank_icon(icon, 22.0)]
        .push(
            text(label)
                .size(13)
                .font(theme::SEMIBOLD_FONT)
                .color(color)
                .wrapping(Wrapping::None),
        )
        .spacing(8)
        .align_y(alignment::Vertical::Center)
        .into()
}

fn rank_icon(path: Option<&std::path::PathBuf>, size: f32) -> Option<Element<'static, Message>> {
    path.map(|path| {
        image(Handle::from_path(path.clone()))
            .width(size)
            .height(size)
            .into()
    })
}

fn level_cell(app: &PrimeApp, account: &AccountProfile) -> Element<'static, Message> {
    match account.account_level.filter(|level| *level > 0) {
        Some(level) => mono(format!("Lv {level}"), 12).color(theme::MUTED),
        None if app.account_ranks_loading.contains(&account.id) => {
            mono("Lv ···", 12).color(theme::FAINT)
        }
        None => mono("Lv —", 12).color(theme::MUTED),
    }
    .into()
}

fn status_cell<'a>(app: &'a PrimeApp, account: &'a AccountProfile) -> Element<'a, Message> {
    let line = |lead: Element<'a, Message>, label: String, color: Color| -> Element<'a, Message> {
        row![
            lead,
            text(label).size(12).color(color).wrapping(Wrapping::None)
        ]
        .spacing(6)
        .align_y(alignment::Vertical::Center)
        .into()
    };

    if app.profile_identity_refreshing.contains(&account.id) {
        return line(
            compact_loading_indicator(app.loading_frame),
            "Refreshing…".to_string(),
            theme::MUTED,
        );
    }
    if let Some(penalty) = account.penalty_status.tooltip_label() {
        let short = penalty.lines().collect::<Vec<_>>().join(" · ");
        let tip = container(text(penalty).size(12))
            .padding([6, 8])
            .style(|_| popover_style(&Theme::Dark));
        return tooltip(
            line(
                theme::icon(Icon::OctagonAlert, 14.0, theme::ACCENT),
                short,
                DANGER_TEXT,
            ),
            tip,
            tooltip::Position::Top,
        )
        .into();
    }
    match launcher_session_captured_at_unix(account) {
        Some(captured_at) => line(
            theme::icon(Icon::ShieldCheck, 14.0, theme::OK),
            format!("Session · {}", relative_time_label(captured_at, now_unix())),
            theme::MUTED,
        ),
        None => line(
            theme::icon(Icon::ShieldOff, 14.0, theme::GOLD),
            "Login not captured".to_string(),
            theme::GOLD,
        ),
    }
}

/// Select, or Capture login for an account without one, then the menu button.
fn row_actions<'a>(
    app: &'a PrimeApp,
    account: &'a AccountProfile,
    is_selected: bool,
    is_menu_open: bool,
) -> Element<'a, Message> {
    let capture_idle = !app.launcher_capture_in_progress && !app.launch_in_progress();
    // The selected account's own Capture login is the hero's.
    let primary = if !account.has_launcher_session() {
        (!is_selected).then_some((
            "Capture login",
            capture_idle.then_some(Message::RequestLauncherSessionLogin(account.id)),
        ))
    } else if !is_selected {
        Some((
            "Select",
            (!app.launch_in_progress()).then_some(Message::SelectAccount(account.id)),
        ))
    } else {
        None
    };

    let more = iced::widget::button(
        container(theme::icon(Icon::Ellipsis, 16.0, theme::MUTED))
            .center_x(30)
            .center_y(30),
    )
    .padding(0)
    .style(move |_, status| {
        let active = is_menu_open
            || matches!(
                status,
                iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
            );
        iced::widget::button::Style {
            background: active.then(|| theme::RAISED.into()),
            border: iced::border::rounded(7),
            ..Default::default()
        }
    })
    .on_press(Message::ToggleAccountMenu(account.id));

    let mut actions = row![].spacing(6).align_y(alignment::Vertical::Center);
    if let Some((label, on_press)) = primary {
        actions = actions.push(
            button(text(label).size(12).font(theme::SEMIBOLD_FONT))
                .padding([6, 12])
                .style(|theme, status| {
                    let mut style = theme::button_style(theme, status);
                    style.border.radius = 7.0.into();
                    style
                })
                .on_press_maybe(on_press),
        );
    }
    actions.push(more).into()
}

fn account_menu(app: &PrimeApp, account: &AccountProfile) -> Element<'static, Message> {
    let account_id = account.id;
    let capture_idle = !app.launcher_capture_in_progress && !app.launch_in_progress();

    let mut menu = column![].spacing(1).width(Length::Fill);
    // Without a login, the row's own button is Capture login, so selecting lives here.
    if !account.has_launcher_session() && app.state.selected_account != Some(account_id) {
        menu = menu.push(menu_item(
            Icon::Check,
            "Select",
            false,
            (!app.launch_in_progress()).then_some(Message::SelectAccount(account_id)),
        ));
    }
    menu = menu.extend([
        menu_item(
            Icon::LogIn,
            "Re-capture login",
            false,
            capture_idle.then_some(Message::RequestLauncherSessionLogin(account_id)),
        ),
        menu_item(
            Icon::RefreshCw,
            "Refresh profile",
            false,
            (!app.profile_identity_refreshing.contains(&account_id))
                .then_some(Message::RefreshProfileIdentity(account_id)),
        ),
    ]);

    if app.settings_cloning {
        let settings_idle = !app.settings_work_in_progress();
        menu = menu
            .push(menu_divider())
            .push(menu_item(
                Icon::Save,
                "Save VALORANT settings",
                false,
                settings_idle.then_some(Message::RequestSavePreset(account_id)),
            ))
            .push(menu_item(
                Icon::SlidersHorizontal,
                "Apply settings profile…",
                false,
                Some(Message::AccountsTabSelected(AccountsTab::GameSettings)),
            ));
    }

    menu = menu
        .push(menu_divider())
        .push(menu_item(
            Icon::Upload,
            "Export account",
            false,
            Some(Message::RequestExportAccount(account_id)),
        ))
        .push(menu_item(
            Icon::Trash,
            "Delete account",
            true,
            Some(Message::RequestDeleteAccount(account_id)),
        ));

    // Opaque, so clicks on its gaps don't reach the rows under it.
    iced::widget::opaque(
        container(menu)
            .padding(6)
            .width(ACCOUNT_MENU_WIDTH)
            .style(popover_style),
    )
}

fn menu_item(
    icon: Icon,
    label: &'static str,
    danger: bool,
    on_press: Option<Message>,
) -> Element<'static, Message> {
    let (icon_color, label_color) = match (on_press.is_some(), danger) {
        (false, _) => (theme::FAINT, theme::FAINT),
        (true, true) => (theme::ACCENT, DANGER_TEXT),
        (true, false) => (theme::MUTED, theme::TEXT),
    };

    iced::widget::button(
        row![
            theme::icon(icon, 15.0, icon_color),
            text(label).size(13).color(label_color)
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center),
    )
    .padding([8, 10])
    .width(Length::Fill)
    .style(|_, status| iced::widget::button::Style {
        background: matches!(
            status,
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
        )
        .then(|| MENU_ITEM_HOVER.into()),
        border: iced::border::rounded(6),
        ..Default::default()
    })
    .on_press_maybe(on_press)
    .into()
}

fn menu_divider() -> Element<'static, Message> {
    container(space())
        .width(Length::Fill)
        .height(1)
        .style(|_| container::Style::default().background(theme::LINE))
        .into()
}

/// The page with no accounts yet: the three ways to add one.
fn first_account(app: &PrimeApp) -> Element<'_, Message> {
    let capture_idle = !app.launcher_capture_in_progress && !app.launch_in_progress();

    let intro = column![
        text("Add your first account")
            .size(30)
            .font(theme::DISPLAY_FONT)
            .line_height(theme::DISPLAY_LINE_HEIGHT),
        text(
            "prime saves each account's remembered Riot Client login so you can switch without \
             retyping a password."
        )
        .size(13)
        .line_height(1.5)
        .color(theme::MUTED)
        .width(460)
        .align_x(alignment::Horizontal::Center),
    ]
    .spacing(8)
    .align_x(alignment::Horizontal::Center);

    let options = row![
        first_account_option(
            Icon::Plus,
            "Add account",
            "Sign in fresh in Riot Client. prime captures the login once you tick “Stay signed in”.",
            true,
            capture_idle.then_some(Message::AddAccount),
        ),
        first_account_option(
            Icon::LogIn,
            "Add current account",
            "Use whoever is already signed in to Riot Client right now.",
            false,
            capture_idle.then_some(Message::AddCurrentAccount),
        ),
        first_account_option(
            Icon::Download,
            "Import account",
            "Paste an export from another prime install.",
            false,
            (!app.import_account_in_progress && capture_idle)
                .then_some(Message::OpenImportAccount),
        ),
    ]
    .spacing(12);

    let note = row![
        theme::icon(Icon::Lock, 12.0, theme::FAINT),
        text("prime never stores Riot passwords.")
            .size(12)
            .color(theme::FAINT)
    ]
    .spacing(6)
    .align_y(alignment::Vertical::Center);

    container(
        column![intro, options, note]
            .spacing(28)
            .align_x(alignment::Horizontal::Center),
    )
    .center(Length::Fill)
    .into()
}

fn first_account_option(
    icon: Icon,
    title: &'static str,
    body: &'static str,
    primary: bool,
    on_press: Option<Message>,
) -> Element<'static, Message> {
    let (badge, icon_color) = if primary {
        (theme::ACCENT, theme::BG)
    } else {
        (theme::RAISED, theme::TEXT)
    };

    iced::widget::button(
        column![
            container(theme::icon(icon, 17.0, icon_color))
                .center_x(34)
                .center_y(34)
                .style(move |_| {
                    container::Style::default()
                        .background(badge)
                        .border(iced::border::rounded(9))
                }),
            text(title)
                .size(14)
                .font(theme::SEMIBOLD_FONT)
                .color(theme::TEXT),
            text(body).size(12).line_height(1.45).color(theme::MUTED),
        ]
        .spacing(10),
    )
    .padding(18)
    .width(EMPTY_OPTION_WIDTH)
    .height(EMPTY_OPTION_HEIGHT)
    .style(move |_, status| {
        use iced::widget::button::Status;

        let hovered = matches!(status, Status::Hovered | Status::Pressed);
        let (background, border) = if primary {
            (
                Color {
                    a: 0x0F as f32 / 255.0,
                    ..theme::ACCENT
                },
                Color {
                    a: if hovered { 0.8 } else { 0x66 as f32 / 255.0 },
                    ..theme::ACCENT
                },
            )
        } else {
            (
                theme::SURFACE,
                if hovered { theme::FAINT } else { theme::LINE },
            )
        };

        iced::widget::button::Style {
            background: Some(background.into()),
            border: iced::Border {
                color: border,
                width: 1.0,
                radius: 12.0.into(),
            },
            ..Default::default()
        }
    })
    .on_press_maybe(on_press)
    .into()
}

pub(in crate::ui) fn save_settings_on_add_checkbox(app: &PrimeApp) -> Element<'_, Message> {
    if !app.settings_cloning {
        return space().into();
    }

    checkbox(app.save_settings_on_add)
        .label("Save this account's VALORANT settings as a preset")
        .on_toggle(Message::SaveSettingsOnAddToggled)
        .text_size(13)
        .into()
}

/// What the Riot client can tell about the account right now, or that it's still checking.
fn account_availability(app: &PrimeApp, account: &AccountProfile) -> AccountAvailability {
    app.account_availability
        .get(&account.id)
        .cloned()
        .unwrap_or_else(|| {
            if app.account_availability_loading {
                AccountAvailability::checking()
            } else {
                AccountAvailability::not_checked()
            }
        })
}

fn presence_color(availability: &AccountAvailability) -> Color {
    match availability {
        AccountAvailability::Available => theme::OK,
        AccountAvailability::Unavailable { .. } => theme::GOLD,
        AccountAvailability::Unknown { .. } => theme::FAINT,
    }
}

/// The line under an account's name: free to play, busy and why, or not known.
fn presence_label(availability: &AccountAvailability) -> String {
    match availability {
        AccountAvailability::Available => "Available".to_string(),
        AccountAvailability::Unavailable { reason } => {
            let mut chars = reason.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_else(|| "Busy".to_string())
        }
        _ if *availability == AccountAvailability::checking() => {
            "Checking availability…".to_string()
        }
        _ if *availability == AccountAvailability::not_checked() => "Not checked".to_string(),
        AccountAvailability::Unknown { .. } => "Status unknown".to_string(),
    }
}

fn tag_label(account: &AccountProfile) -> Option<String> {
    account
        .tag_line
        .as_deref()
        .filter(|tag| !tag.is_empty())
        .map(|tag| format!("#{tag}"))
}

/// The account's saved region, or its shard before the first lookup.
fn region_label(account: &AccountProfile) -> String {
    account
        .region
        .map(|region| region.as_str().to_uppercase())
        .unwrap_or_else(|| account.shard.to_string().to_uppercase())
}

/// What the rank badge says without a rank: the account has none, or it couldn't be loaded.
pub(in crate::ui) fn missing_rank_label(app: &PrimeApp, account_id: AccountId) -> &'static str {
    if app.unranked_accounts.contains(&account_id) {
        "Unranked"
    } else {
        "Rank unavailable"
    }
}

fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// How long ago something happened, as the design writes it: "12 min ago", "2 h ago".
fn relative_time_label(then_unix: i64, now_unix: i64) -> String {
    let seconds = now_unix.saturating_sub(then_unix).max(0);
    match seconds {
        0..60 => "just now".to_string(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86_400 => format!("{} h ago", seconds / 3600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

pub(super) fn last_refreshed_label(timestamp: Option<i64>) -> String {
    let Some(timestamp) = timestamp else {
        return "Never".to_string();
    };
    let Ok(refreshed_at) = OffsetDateTime::from_unix_timestamp(timestamp) else {
        return "Unknown".to_string();
    };
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);

    format_refreshed_at(refreshed_at, offset)
}

/// The short form dialogs use, such as "captured Sep 27".
pub(in crate::ui) fn captured_on_label(account: &AccountProfile) -> String {
    let captured_at = launcher_session_captured_at_unix(account)
        .and_then(|timestamp| OffsetDateTime::from_unix_timestamp(timestamp).ok());
    let Some(captured_at) = captured_at else {
        return "login not captured".to_string();
    };
    let captured_at =
        captured_at.to_offset(UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC));
    let month = captured_at.month().to_string();

    format!("captured {} {}", &month[..3], captured_at.day())
}

fn launcher_session_captured_at_unix(account: &AccountProfile) -> Option<i64> {
    account
        .launcher_session
        .as_ref()
        .map(|backup| backup.captured_at_unix)
}

fn format_refreshed_at(refreshed_at: OffsetDateTime, offset: UtcOffset) -> String {
    let refreshed_at = refreshed_at.to_offset(offset);
    let hour = refreshed_at.hour();
    let hour_12 = match hour % 12 {
        0 => 12,
        value => value,
    };
    let period = if hour < 12 { "AM" } else { "PM" };

    format!(
        "{:04}-{:02}-{:02} {}:{:02} {}",
        refreshed_at.year(),
        u8::from(refreshed_at.month()),
        refreshed_at.day(),
        hour_12,
        refreshed_at.minute(),
        period
    )
}

fn rank_color(rank: &CompetitiveRank) -> Color {
    match rank.tier {
        3..=5 => Color::from_rgb8(145, 151, 158),
        6..=8 => Color::from_rgb8(190, 124, 74),
        9..=11 => Color::from_rgb8(188, 198, 205),
        12..=14 => Color::from_rgb8(235, 190, 82),
        15..=17 => Color::from_rgb8(82, 204, 194),
        18..=20 => Color::from_rgb8(181, 134, 236),
        21..=23 => Color::from_rgb8(84, 209, 125),
        24..=26 => Color::from_rgb8(224, 87, 92),
        27 => Color::from_rgb8(255, 219, 108),
        _ => Color::from_rgb8(160, 166, 176),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::Shard;

    #[test]
    fn formats_last_refreshed_time() {
        assert_eq!(last_refreshed_label(None), "Never");
        assert!(!last_refreshed_label(Some(1_800_000_000)).contains("UTC"));
        assert_eq!(
            format_refreshed_at(
                OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap(),
                UtcOffset::from_hms(-5, 0, 0).unwrap(),
            ),
            "2027-01-15 3:00 AM"
        );
        assert_eq!(
            format_refreshed_at(
                OffsetDateTime::from_unix_timestamp(1_800_032_400).unwrap(),
                UtcOffset::UTC,
            ),
            "2027-01-15 5:00 PM"
        );
    }

    #[test]
    fn relative_times_read_as_the_design_writes_them() {
        assert_eq!(relative_time_label(1_000, 1_030), "just now");
        assert_eq!(relative_time_label(1_000, 1_000 + 12 * 60), "12 min ago");
        assert_eq!(relative_time_label(1_000, 1_000 + 2 * 3600 + 5), "2 h ago");
        assert_eq!(relative_time_label(1_000, 1_000 + 3 * 86_400), "3 d ago");
        // A clock that went backwards isn't "in the future".
        assert_eq!(relative_time_label(2_000, 1_000), "just now");
    }

    #[test]
    fn presence_names_why_an_account_is_busy() {
        assert_eq!(
            presence_label(&AccountAvailability::Unavailable {
                reason: "in match".to_string()
            }),
            "In match"
        );
        assert_eq!(
            presence_label(&AccountAvailability::checking()),
            "Checking availability…"
        );
        assert_eq!(presence_label(&AccountAvailability::Available), "Available");
    }

    #[test]
    fn last_refreshed_uses_launcher_capture_time() {
        let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
        account.launcher_session = Some(crate::account::LauncherSessionBackup {
            data_dir: std::path::PathBuf::from("backup"),
            captured_at_unix: 100,
            puuid: "puuid".to_string(),
        });

        assert_eq!(launcher_session_captured_at_unix(&account), Some(100));
    }
}
