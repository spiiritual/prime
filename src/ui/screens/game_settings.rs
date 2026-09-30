//! The Accounts tab's Settings profiles sub-tab: saved presets on the left, the selected one's
//! Apply panel on the right, and each account's own settings that an apply put aside.

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{Space, canvas, column, container, row, scrollable, space, stack};
use iced::{
    Color, Element, Length, Padding, Point, Rectangle, Renderer, Size, Theme, alignment, border,
    mouse,
};
use time::{OffsetDateTime, UtcOffset};

use crate::account::{AccountId, AccountProfile};
use crate::game_settings::{
    CrosshairLines, CrosshairSummary, GameSettingsProfileMetadata, GameSettingsProfilePurpose,
    GameSettingsProfileSummary, Rgba,
};
use crate::ui::components::{
    account_avatar, anchored_popover, card_style, compact_loading_indicator, mono, outlined,
};
use crate::ui::shell::popover_style;
use crate::ui::theme::{self, Icon, button, text};
use crate::ui::{Message, PrimeApp, SettingsChange};

use super::accounts::{menu_item, now_unix, relative_time_label};

/// How many keybinds a card shows at a glance; the rest are counted.
const SHORT_KEYBIND_LIMIT: usize = 3;
const CROSSHAIR_PREVIEW_SIZE: f32 = 56.0;
/// Screen pixels per VALORANT crosshair unit, before shrinking a large crosshair to fit.
const CROSSHAIR_PREVIEW_SCALE: f32 = 2.0;
const GLANCE_GAP: f32 = 16.0;
/// Below this luminance (0 to 255) a crosshair is drawn on grey instead of the dark tile.
const DARK_CROSSHAIR_LUMINANCE: f32 = 80.0;
const FACT_ICON_SIZE: f32 = 14.0;
const FACT_GAP: f32 = 8.0;
/// The full settings list's column shares: keybinds get more, since keys can be long.
const KEYBINDS_COLUMN_SHARE: u16 = 4;
const SETTINGS_COLUMN_SHARE: u16 = 3;
const SETTING_ROW_HEIGHT: f32 = 19.0;
const SWATCH_SIZE: f32 = 10.0;
const APPLY_PANEL_WIDTH: f32 = 340.0;
const PRESET_MENU_WIDTH: f32 = 180.0;
/// The menu opens just under the card's ⋯ button.
const PRESET_MENU_TOP_OFFSET: f32 = 46.0;
const PRESET_MENU_RIGHT_INSET: f32 = 10.0;
const PANEL_GAP: f32 = 16.0;
/// How long a save reads as "2 days ago" before it reads as a date.
const RELATIVE_SAVE_SECONDS: i64 = 7 * 86_400;
const DANGER_TEXT: Color = iced::color!(0xFF8A94);

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    let saved = saved_presets(app);
    let selected = selected_preset(app);

    let mut list = column![].spacing(10).width(Length::Fill);
    if let Some(account_id) = app.settings_saving_account {
        list = list.push(saving_card(app, account_id));
    }
    for profile in &saved {
        let is_selected = selected.is_some_and(|selected| selected.id == profile.id);
        list = list.push(profile_card(app, profile, is_selected));
    }
    let restore = restore_section(app);

    // Put-aside settings scroll with the list, so their Restore buttons can't be cut off.
    let left: Element<'_, Message> =
        if saved.is_empty() && app.settings_saving_account.is_none() && restore.is_none() {
            empty_profiles()
        } else {
            if let Some(restore) = restore {
                list = list.push(restore);
            }
            // The gap before the panel is inside the scrollable, so its scrollbar floats in the gap
            // instead of over the cards.
            let gap = if selected.is_some() { PANEL_GAP } else { 0.0 };
            scrollable(container(list).padding(Padding::ZERO.right(gap)))
                .direction(Direction::Vertical(
                    Scrollbar::new()
                        .width(4)
                        .scroller_width(4)
                        .margin((PANEL_GAP - 4.0) / 2.0),
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

    let mut body = row![left].height(Length::Fill);
    if let Some(profile) = selected {
        body = body.push(apply_panel(app, profile));
    }
    body.into()
}

fn saved_presets(app: &PrimeApp) -> Vec<&GameSettingsProfileMetadata> {
    app.settings_profiles
        .iter()
        .filter(|profile| profile.purpose == GameSettingsProfilePurpose::Profile)
        .collect()
}

/// The preset the Apply panel is for: the one last picked, or the first while none is.
pub(in crate::ui) fn selected_preset(app: &PrimeApp) -> Option<&GameSettingsProfileMetadata> {
    let saved = saved_presets(app);
    app.selected_preset
        .as_ref()
        .and_then(|id| saved.iter().find(|profile| &profile.id == id))
        .or(saved.first())
        .copied()
}

/// Whether Prime can read and write this account's VALORANT settings.
fn has_settings_access(account: &AccountProfile) -> bool {
    account.has_launcher_session() || account.session.is_some()
}

fn settings_busy(app: &PrimeApp) -> bool {
    app.settings_work_in_progress()
}

fn account(app: &PrimeApp, account_id: AccountId) -> Option<&AccountProfile> {
    app.state
        .accounts
        .iter()
        .find(|account| account.id == account_id)
}

/// Whether the preset was saved from this account. An account removed and added again gets a new
/// ID, so its PUUID counts too.
fn is_source(account: &AccountProfile, profile: &GameSettingsProfileMetadata) -> bool {
    account.id == profile.source_account_id
        || account.puuid.as_deref() == Some(profile.source_puuid.as_str())
}

/// A saved time as the design writes it: "2 days ago" within a week, then "Sep 2".
fn saved_label(then_unix: i64, now_unix: i64) -> String {
    let seconds = now_unix.saturating_sub(then_unix);
    match seconds / 86_400 {
        _ if seconds >= RELATIVE_SAVE_SECONDS => {}
        0 => return relative_time_label(then_unix, now_unix),
        1 => return "1 day ago".to_string(),
        days => return format!("{days} days ago"),
    }
    let Ok(saved_at) = OffsetDateTime::from_unix_timestamp(then_unix) else {
        return "a while ago".to_string();
    };
    let saved_at = saved_at.to_offset(UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC));
    let month = saved_at.month().to_string();

    format!("{} {}", &month[..3], saved_at.day())
}

fn profile_card<'a>(
    app: &'a PrimeApp,
    profile: &'a GameSettingsProfileMetadata,
    selected: bool,
) -> Element<'a, Message> {
    let expanded = app.expanded_presets.contains(&profile.id);
    let source = app
        .state
        .accounts
        .iter()
        .find(|account| is_source(account, profile));
    let source_name = source.map_or(profile.source_display_name.as_str(), |account| {
        account.display_name.as_str()
    });

    let mut top = row![
        column![
            text(&profile.name)
                .size(14)
                .font(theme::SEMIBOLD_FONT)
                .color(theme::TEXT),
            row![
                account_avatar(
                    source_name,
                    source.and_then(|account| app.player_card_art_path(account)),
                    14.0,
                    4.0
                ),
                text(format!(
                    "From {source_name} · saved {}",
                    saved_label(profile.captured_at_unix, now_unix())
                ))
                .size(12)
                .color(theme::MUTED)
            ]
            .spacing(6)
            .align_y(alignment::Vertical::Center)
        ]
        .spacing(2)
        .width(Length::Fill)
    ]
    .spacing(10)
    .align_y(alignment::Vertical::Center);
    if let Some(version) = profile.settings_version {
        top = top.push(mono(format!("v{version}"), 11).color(theme::FAINT));
    }
    let menu_open = app.open_preset_menu.as_ref() == Some(&profile.id);
    top = top.push(
        iced::widget::button(
            container(theme::icon(Icon::Ellipsis, 16.0, theme::MUTED))
                .center_x(28)
                .center_y(28),
        )
        .padding(0)
        .style(move |_, status| iced::widget::button::Style {
            background: (menu_open
                || matches!(
                    status,
                    iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
                ))
            .then(|| theme::RAISED.into()),
            border: border::rounded(7),
            ..Default::default()
        })
        .on_press(Message::TogglePresetMenu(profile.id.clone())),
    );

    // At a glance: the crosshair and the settings people compare presets by. The full list
    // opens below them.
    let summary = &profile.summary;
    let mut card = column![
        top,
        row![
            crosshair_tile(summary.crosshair.as_ref()),
            glance_facts(&profile.id, summary, expanded)
        ]
        .spacing(GLANCE_GAP)
    ]
    .spacing(12);
    if expanded {
        card = card.push(
            container(all_settings(summary))
                .padding(Padding::ZERO.left(CROSSHAIR_PREVIEW_SIZE + GLANCE_GAP)),
        );
    }

    let card = iced::widget::button(card)
        .padding([12, 16])
        .width(Length::Fill)
        .style(move |_, _| {
            let style = if selected {
                container::Style::default()
                    .background(Color {
                        a: 0x0D as f32 / 255.0,
                        ..theme::ACCENT
                    })
                    .border(iced::Border {
                        color: Color {
                            a: 0x66 as f32 / 255.0,
                            ..theme::ACCENT
                        },
                        width: 1.0,
                        radius: 12.0.into(),
                    })
            } else {
                card_style(12.0)
            };
            iced::widget::button::Style {
                background: style.background,
                border: style.border,
                text_color: theme::TEXT,
                ..Default::default()
            }
        })
        .on_press(Message::SelectPreset(profile.id.clone()));

    anchored_popover(
        card,
        preset_menu(app, profile),
        menu_open,
        PRESET_MENU_TOP_OFFSET,
        PRESET_MENU_RIGHT_INSET,
    )
}

/// Rename and Delete for one preset, from its card's ⋯ button.
fn preset_menu(app: &PrimeApp, profile: &GameSettingsProfileMetadata) -> Element<'static, Message> {
    let menu = column![
        menu_item(
            Icon::Pencil,
            "Rename",
            false,
            Some(Message::RequestRenamePreset(profile.id.clone())),
        ),
        menu_item(
            Icon::Trash,
            "Delete preset",
            true,
            (!settings_busy(app))
                .then(|| Message::RequestDeleteSettingsProfile(profile.id.clone())),
        ),
    ]
    .spacing(1)
    .width(Length::Fill);

    // Opaque, so clicks on its gaps don't reach the card under it.
    iced::widget::opaque(
        container(menu)
            .padding(6)
            .width(PRESET_MENU_WIDTH)
            .style(popover_style),
    )
}

fn saving_card(app: &PrimeApp, account_id: AccountId) -> Element<'_, Message> {
    let name = account(app, account_id).map_or("", |account| account.display_name.as_str());

    container(progress(app, format!("Saving {name}'s settings…"), 13))
        .padding(16)
        .width(Length::Fill)
        .style(|_| card_style(12.0))
        .into()
}

/// The selected preset and every account it can go to, each with its own Apply.
fn apply_panel<'a>(
    app: &'a PrimeApp,
    profile: &'a GameSettingsProfileMetadata,
) -> Element<'a, Message> {
    let busy = settings_busy(app);
    let head = column![
        text(format!("Apply “{}”", profile.name))
            .size(14)
            .font(theme::SEMIBOLD_FONT)
            .color(theme::TEXT),
        text("Overwrites these accounts' in-game settings.")
            .size(12)
            .color(theme::MUTED)
    ]
    .spacing(3);

    let rows = column(
        app.state
            .accounts
            .iter()
            .map(|account| apply_row(app, profile, account, busy)),
    )
    .width(Length::Fill);

    // Inset by the border, since the dividers draw over it.
    container(column![
        container(head).padding([13, 15]).width(Length::Fill),
        divider(),
        scrollable(rows)
            .direction(Direction::Vertical(
                Scrollbar::new().width(4).scroller_width(4).margin(2),
            ))
            .height(Length::Fill),
    ])
    .padding(1)
    .width(APPLY_PANEL_WIDTH)
    .height(Length::Fill)
    .style(|_| card_style(12.0))
    .into()
}

fn apply_row<'a>(
    app: &'a PrimeApp,
    profile: &'a GameSettingsProfileMetadata,
    account: &'a AccountProfile,
    busy: bool,
) -> Element<'a, Message> {
    let available = has_settings_access(account);
    // Matched on the account alone, so the check stays visible while another preset is shown.
    let checking = app.settings_check.as_ref().is_some_and(|check| {
        matches!(check.change, SettingsChange::Apply { account_id, .. } if account_id == account.id)
    });

    let trailing: Element<'a, Message> = if app.settings_applying_account == Some(account.id) {
        progress(app, "Applying…".to_string(), 11)
    } else if checking {
        progress(app, "Checking…".to_string(), 11)
    } else if !available {
        text("No API session")
            .size(11)
            .color(Color {
                a: 0.5,
                ..theme::GOLD
            })
            .into()
    } else {
        let mut trailing = row![].spacing(10).align_y(alignment::Vertical::Center);
        if is_source(account, profile) {
            trailing = trailing.push(text("source").size(11).color(theme::FAINT));
        }
        trailing
            .push(small_button(
                "Apply",
                theme::TEXT,
                (!busy).then(|| Message::RequestApplyPreset {
                    profile_id: profile.id.clone(),
                    account_id: account.id,
                }),
            ))
            .into()
    };

    let avatar = account_avatar(
        &account.display_name,
        app.player_card_art_path(account),
        24.0,
        6.0,
    );
    // The design fades an account without API access to half opacity.
    let avatar: Element<'a, Message> = if available {
        avatar
    } else {
        stack![
            avatar,
            container(space()).width(24).height(24).style(|_| {
                container::Style::default()
                    .background(Color {
                        a: 0.5,
                        ..theme::SURFACE
                    })
                    .border(border::rounded(6))
            })
        ]
        .into()
    };
    let name_color = if available {
        theme::TEXT
    } else {
        Color {
            a: 0.5,
            ..theme::TEXT
        }
    };

    container(
        row![
            avatar,
            text(&account.display_name)
                .size(13)
                .font(theme::MEDIUM_FONT)
                .color(name_color)
                .width(Length::Fill),
            trailing
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center),
    )
    .padding([10, 15])
    .height(44)
    .align_y(alignment::Vertical::Center)
    .width(Length::Fill)
    .into()
}

/// Shown instead of the list while no preset is saved.
fn empty_profiles() -> Element<'static, Message> {
    container(
        column![
            container(theme::icon(Icon::SlidersHorizontal, 20.0, theme::MUTED))
                .center_x(44)
                .center_y(44)
                .style(|_| card_style(12.0)),
            text("No saved settings profiles")
                .size(16)
                .font(theme::SEMIBOLD_FONT)
                .color(theme::TEXT),
            text(
                "Save crosshair, keybinds, mouse and video settings from one account, then apply \
                 them to any other account with API access."
            )
            .size(13)
            .line_height(1.5)
            .color(theme::MUTED)
            .width(420)
            .align_x(alignment::Horizontal::Center),
        ]
        .spacing(10)
        .align_x(alignment::Horizontal::Center),
    )
    .center(Length::Fill)
    .style(|_| outlined(12.0))
    .into()
}

/// Accounts with their own settings put aside by an apply, each with Restore and Discard. Hidden
/// when there are none.
fn restore_section(app: &PrimeApp) -> Option<Element<'_, Message>> {
    let originals = original_settings(app);
    if originals.is_empty() {
        return None;
    }

    let busy = settings_busy(app);
    let mut rows = column![].width(Length::Fill);
    for original in originals {
        let account = account(app, original.source_account_id);
        let name = account.map_or(original.source_display_name.as_str(), |account| {
            account.display_name.as_str()
        });

        let trailing: Element<'_, Message> =
            if app.settings_applying_account == Some(original.source_account_id) {
                progress(app, "Restoring…".to_string(), 11)
            } else if app.settings_check.as_ref().is_some_and(|check| {
                check.change == SettingsChange::Restore(original.source_account_id)
            }) {
                progress(app, "Checking…".to_string(), 11)
            } else {
                row![
                    small_button(
                        "Restore",
                        theme::TEXT,
                        (!busy && account.is_some())
                            .then_some(Message::RequestRestoreSettings(original.source_account_id)),
                    ),
                    small_button(
                        "Discard",
                        DANGER_TEXT,
                        (!busy).then(|| Message::RequestDeleteSettingsProfile(original.id.clone())),
                    ),
                ]
                .spacing(6)
                .into()
            };

        rows = rows.push(
            container(
                row![
                    account_avatar(
                        name,
                        account.and_then(|account| app.player_card_art_path(account)),
                        24.0,
                        6.0
                    ),
                    column![
                        text(format!("{name}'s own settings"))
                            .size(13)
                            .font(theme::MEDIUM_FONT)
                            .color(theme::TEXT),
                        text(format!(
                            "Put aside {}",
                            saved_label(original.captured_at_unix, now_unix())
                        ))
                        .size(11)
                        .color(theme::FAINT)
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    trailing
                ]
                .spacing(10)
                .align_y(alignment::Vertical::Center),
            )
            .padding([10, 16])
            .width(Length::Fill),
        );
    }

    let head = column![
        text("Own settings put aside")
            .size(14)
            .font(theme::SEMIBOLD_FONT)
            .color(theme::TEXT),
        text(
            "Applying a preset puts the account's own settings aside first. Restore puts them back."
        )
        .size(12)
        .color(theme::MUTED)
    ]
    .spacing(3);

    Some(
        container(column![
            container(head).padding([14, 16]).width(Length::Fill),
            divider(),
            rows
        ])
        .padding(1)
        .width(Length::Fill)
        .style(|_| card_style(12.0))
        .into(),
    )
}

fn small_button(
    label: &'static str,
    color: Color,
    on_press: Option<Message>,
) -> Element<'static, Message> {
    let color = if on_press.is_some() {
        color
    } else {
        theme::FAINT
    };
    button(text(label).size(12).font(theme::SEMIBOLD_FONT).color(color))
        .padding([4, 10])
        .style(|theme, status| {
            let mut style = theme::button_style(theme, status);
            style.border.radius = 7.0.into();
            style
        })
        .on_press_maybe(on_press)
        .into()
}

fn progress(app: &PrimeApp, label: String, size: u32) -> Element<'static, Message> {
    row![
        compact_loading_indicator(app.loading_frame),
        text(label).size(size).color(theme::MUTED)
    ]
    .spacing(6)
    .align_y(alignment::Vertical::Center)
    .into()
}

fn divider() -> Element<'static, Message> {
    container(space())
        .width(Length::Fill)
        .height(1)
        .style(|_| container::Style::default().background(theme::LINE))
        .into()
}

/// The preset's settings, one line each behind an icon, and a link to every setting it copies.
/// Missing values are VALORANT's defaults.
fn glance_facts<'a>(
    profile_id: &str,
    summary: &'a GameSettingsProfileSummary,
    expanded: bool,
) -> Element<'a, Message> {
    let mut facts = column![
        fact(Icon::Mouse, 17.0, sensitivity_value(summary)),
        fact(Icon::Crosshair, 15.0, crosshair_value(summary)),
        // Beside the first line of keybinds, which are as tall as their key chips.
        fact(Icon::Keyboard, SETTING_ROW_HEIGHT, keybinds_value(summary)),
        fact(Icon::Map, 15.0, minimap_value(summary)),
    ]
    .spacing(7)
    .width(Length::Fill);

    let listed =
        summary.keybinds.len() + summary.audio_settings.len() + summary.other_settings.len();
    if listed == 0 {
        return facts.into();
    }

    let (label, chevron) = if expanded {
        ("Hide settings".to_string(), Icon::ChevronUp)
    } else {
        (format!("Show all settings ({listed})"), Icon::ChevronDown)
    };
    facts = facts.push(
        container(
            iced::widget::button(
                row![
                    text(label).size(12).font(theme::MEDIUM_FONT),
                    theme::icon(chevron, 12.0, theme::MUTED)
                ]
                .spacing(4)
                .align_y(alignment::Vertical::Center),
            )
            .padding(Padding::ZERO.top(1))
            .style(|_, status| iced::widget::button::Style {
                text_color: if matches!(
                    status,
                    iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
                ) {
                    theme::TEXT
                } else {
                    theme::MUTED
                },
                ..Default::default()
            })
            .on_press(Message::TogglePresetSettings(profile_id.to_string())),
        )
        .padding(Padding::ZERO.left(FACT_ICON_SIZE + FACT_GAP)),
    );

    facts.into()
}

/// One line of the glance: an icon centred on the value's first line of `line_height`.
fn fact<'a>(icon: Icon, line_height: f32, value: Element<'a, Message>) -> Element<'a, Message> {
    row![
        container(theme::icon(icon, FACT_ICON_SIZE, theme::FAINT))
            .height(line_height)
            .align_y(alignment::Vertical::Center),
        value
    ]
    .spacing(FACT_GAP)
    .into()
}

/// Every keybind and every other setting a preset copies, in three columns.
fn all_settings(summary: &GameSettingsProfileSummary) -> Element<'_, Message> {
    let mut columns = row![].spacing(20);

    if !summary.keybinds.is_empty() {
        let rows = summary
            .keybinds
            .iter()
            .map(|keybind| setting_row(&keybind.action, key_chip(&keybind.key)));
        columns = columns.push(
            settings_group("All keybinds", rows).width(Length::FillPortion(KEYBINDS_COLUMN_SHARE)),
        );
    }
    for (title, settings) in [
        ("Audio", &summary.audio_settings),
        ("Other", &summary.other_settings),
    ] {
        if settings.is_empty() {
            continue;
        }
        let rows = settings.iter().map(|setting| {
            setting_row(
                &setting.label,
                mono(&setting.value, 11).color(theme::TEXT).into(),
            )
        });
        columns = columns
            .push(settings_group(title, rows).width(Length::FillPortion(SETTINGS_COLUMN_SHARE)));
    }

    container(columns)
        .padding([10, 12])
        .width(Length::Fill)
        .style(|_| {
            container::Style::default()
                .background(theme::BG)
                .border(border::rounded(8).width(1).color(theme::LINE))
        })
        .into()
}

fn settings_group<'a>(
    title: &'a str,
    rows: impl Iterator<Item = Element<'a, Message>>,
) -> iced::widget::Column<'a, Message> {
    column![
        text(title)
            .size(11)
            .font(theme::SEMIBOLD_FONT)
            .color(theme::MUTED)
    ]
    .extend(rows)
    .spacing(5)
}

fn setting_row<'a>(label: &'a str, value: Element<'a, Message>) -> Element<'a, Message> {
    // The empty strut keeps rows at least a key chip tall; a wrapped label or value grows them.
    row![
        Space::new().height(SETTING_ROW_HEIGHT),
        row![
            text(label).size(12).color(theme::MUTED).width(Length::Fill),
            value
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center)
    ]
    .align_y(alignment::Vertical::Center)
    .into()
}

fn default_value() -> Element<'static, Message> {
    text("Default").size(12).color(theme::MUTED).into()
}

/// The pieces of a line, with a dot between each.
fn dotted<'a>(pieces: impl IntoIterator<Item = Element<'a, Message>>) -> Element<'a, Message> {
    let mut line = row![]
        .spacing(FACT_GAP)
        .align_y(alignment::Vertical::Center);
    for (index, piece) in pieces.into_iter().enumerate() {
        if index > 0 {
            line = line.push(text("·").size(12).color(theme::FAINT));
        }
        line = line.push(piece);
    }
    // Wraps onto another line rather than squeezing the pieces when the card is narrow.
    line.wrap().vertical_spacing(4).into()
}

fn sensitivity_value(summary: &GameSettingsProfileSummary) -> Element<'static, Message> {
    let multiplier = |label: &'static str, value: f64| -> Element<'static, Message> {
        row![
            text(label).size(12).color(theme::MUTED),
            mono(format!("{}×", format_number(value)), 12).color(theme::TEXT)
        ]
        .spacing(FACT_GAP)
        .align_y(alignment::Vertical::Center)
        .into()
    };
    let parts = [
        summary.sensitivity.map(|value| {
            mono(format_number(value), 13)
                .font(theme::MONO_SEMIBOLD_FONT)
                .color(theme::TEXT)
                .into()
        }),
        summary.ads_multiplier.map(|value| multiplier("ADS", value)),
        summary
            .scoped_multiplier
            .map(|value| multiplier("Scoped", value)),
    ];

    if parts.iter().all(Option::is_none) {
        return default_value();
    }
    dotted(parts.into_iter().flatten())
}

fn crosshair_value(summary: &GameSettingsProfileSummary) -> Element<'_, Message> {
    let Some(crosshair) = &summary.crosshair else {
        return default_value();
    };

    let mut pieces: Vec<Element<'_, Message>> = vec![
        row![
            color_swatch(crosshair.color),
            text(crosshair.color.label())
                .size(12)
                .font(theme::MEDIUM_FONT)
                .color(theme::TEXT)
        ]
        .spacing(FACT_GAP)
        .align_y(alignment::Vertical::Center)
        .into(),
    ];
    if let Some(name) = &crosshair.name {
        pieces.push(text(name).size(12).color(theme::MUTED).into());
    }
    if summary.crosshair_profile_count > 1 {
        pieces.push(
            text(format!(
                "{} saved crosshairs",
                summary.crosshair_profile_count
            ))
            .size(12)
            .color(theme::FAINT)
            .into(),
        );
    }

    dotted(pieces)
}

fn keybinds_value(summary: &GameSettingsProfileSummary) -> Element<'_, Message> {
    if summary.keybinds.is_empty() {
        return default_value();
    }

    let mut value = row![].spacing(14).align_y(alignment::Vertical::Center);
    for keybind in summary.keybinds.iter().take(SHORT_KEYBIND_LIMIT) {
        value = value.push(
            row![
                key_chip(&keybind.key),
                text("→").size(11).color(theme::FAINT),
                text(&keybind.action).size(12).color(theme::TEXT)
            ]
            .spacing(6)
            .align_y(alignment::Vertical::Center),
        );
    }

    let hidden = summary.keybinds.len().saturating_sub(SHORT_KEYBIND_LIMIT);
    if hidden > 0 {
        value = value.push(text(format!("+{hidden} more")).size(12).color(theme::FAINT));
    }

    // Wraps whole keybinds onto the next line when the card is narrow.
    value.wrap().vertical_spacing(4).into()
}

fn minimap_value(summary: &GameSettingsProfileSummary) -> Element<'_, Message> {
    if summary.minimap.is_empty() {
        return default_value();
    }
    dotted(
        summary
            .minimap
            .iter()
            .map(|value| text(value).size(12).color(theme::TEXT).into()),
    )
}

/// A key as a chip, or an outline when the action has no key.
fn key_chip(key: &str) -> Element<'_, Message> {
    let unbound = key == "Unbound";
    container(mono(key, 11).color(if unbound { theme::FAINT } else { theme::TEXT }))
        .padding([2, 6])
        .style(move |_| {
            if unbound {
                container::Style::default().border(border::rounded(5).width(1).color(theme::LINE))
            } else {
                container::Style::default()
                    .background(theme::RAISED)
                    .border(border::rounded(5))
            }
        })
        .into()
}

fn color_swatch(color: Rgba) -> Element<'static, Message> {
    container(Space::new())
        .width(SWATCH_SIZE)
        .height(SWATCH_SIZE)
        .style(move |_| iced::widget::container::Style {
            background: Some(Color::from_rgb8(color.r, color.g, color.b).into()),
            border: border::rounded(3).width(1).color(Color::from_rgba8(
                255,
                255,
                255,
                0x26 as f32 / 255.0,
            )),
            ..Default::default()
        })
        .into()
}

/// `0.34`, `0.6` and `1` rather than `0.340` or `1.0`.
fn format_number(value: f64) -> String {
    let formatted = format!("{value:.3}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// The crosshair drawn on a dark tile, or VALORANT's default one when the preset has none.
fn crosshair_tile(crosshair: Option<&CrosshairSummary>) -> Element<'static, Message> {
    let crosshair = crosshair
        .cloned()
        .unwrap_or_else(CrosshairSummary::default_crosshair);
    // A dark crosshair would vanish on the dark tile, so it gets a mid grey, like a wall in game.
    let Rgba { r, g, b, .. } = crosshair.color;
    let luminance = 0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b);
    let background = if luminance < DARK_CROSSHAIR_LUMINANCE {
        Color::from_rgb8(92, 98, 106)
    } else {
        theme::BG
    };

    container(
        canvas(CrosshairPreview { crosshair })
            .width(CROSSHAIR_PREVIEW_SIZE)
            .height(CROSSHAIR_PREVIEW_SIZE),
    )
    .clip(true)
    .style(move |_| {
        container::Style::default()
            .background(background)
            .border(border::rounded(8).width(1).color(theme::LINE))
    })
    .into()
}

/// Draws a crosshair onto the preview tile.
struct CrosshairPreview {
    crosshair: CrosshairSummary,
}

impl canvas::Program<Message> for CrosshairPreview {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        for (rect, color) in crosshair_rects(&self.crosshair, bounds.width.min(bounds.height)) {
            frame.fill_rectangle(
                Point::new(rect.x, rect.y),
                Size::new(rect.width, rect.height),
                color,
            );
        }

        vec![frame.into_geometry()]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Rect {
    fn grown(self, by: f32) -> Self {
        Self {
            x: self.x - by,
            y: self.y - by,
            width: self.width + by * 2.0,
            height: self.height + by * 2.0,
        }
    }

    /// Snapped to whole pixels so thin lines stay crisp.
    fn snapped(self) -> Self {
        Self {
            x: self.x.round(),
            y: self.y.round(),
            width: self.width.round().max(1.0),
            height: self.height.round().max(1.0),
        }
    }
}

/// The crosshair's pieces, outlines first, centred in a square of `size` pixels.
fn crosshair_rects(crosshair: &CrosshairSummary, size: f32) -> Vec<(Rect, Color)> {
    let center = size / 2.0;
    let extent = [crosshair.inner_lines, crosshair.outer_lines]
        .into_iter()
        .flatten()
        .map(|lines| lines.offset + lines.length.max(lines.vertical_length))
        .chain(crosshair.center_dot.map(|dot| dot.size / 2.0))
        .fold(0.0_f32, f32::max);
    // Large crosshairs shrink to fit, leaving room for the outline.
    let scale = if extent > 0.0 {
        CROSSHAIR_PREVIEW_SCALE.min((center - 4.0) / extent)
    } else {
        CROSSHAIR_PREVIEW_SCALE
    };
    let color = |opacity: f32| {
        Color::from_rgba8(
            crosshair.color.r,
            crosshair.color.g,
            crosshair.color.b,
            f32::from(crosshair.color.a) / 255.0 * opacity.clamp(0.0, 1.0),
        )
    };

    let mut pieces = Vec::new();
    for lines in [crosshair.inner_lines, crosshair.outer_lines]
        .into_iter()
        .flatten()
    {
        pieces.extend(
            line_rects(lines, center, scale)
                .into_iter()
                .map(|rect| (rect, color(lines.opacity))),
        );
    }
    if let Some(dot) = crosshair.center_dot {
        let side = dot.size * scale;
        pieces.push((
            Rect {
                x: center - side / 2.0,
                y: center - side / 2.0,
                width: side,
                height: side,
            },
            color(dot.opacity),
        ));
    }

    let mut rects = Vec::new();
    if let Some(outline) = crosshair.outline {
        let outline_color = Color::from_rgba(0.0, 0.0, 0.0, outline.opacity.clamp(0.0, 1.0));
        rects.extend(pieces.iter().map(|(rect, _)| {
            (
                rect.grown(outline.thickness * scale).snapped(),
                outline_color,
            )
        }));
    }
    rects.extend(
        pieces
            .into_iter()
            .map(|(rect, color)| (rect.snapped(), color)),
    );

    rects
}

/// The four arms of one set of lines: right, left, down and up.
fn line_rects(lines: CrosshairLines, center: f32, scale: f32) -> Vec<Rect> {
    let thickness = lines.thickness * scale;
    let offset = lines.offset * scale;
    let length = lines.length * scale;
    let vertical_length = lines.vertical_length * scale;
    let mut rects = Vec::new();

    if length > 0.0 && thickness > 0.0 {
        let y = center - thickness / 2.0;
        rects.push(Rect {
            x: center + offset,
            y,
            width: length,
            height: thickness,
        });
        rects.push(Rect {
            x: center - offset - length,
            y,
            width: length,
            height: thickness,
        });
    }
    if vertical_length > 0.0 && thickness > 0.0 {
        let x = center - thickness / 2.0;
        rects.push(Rect {
            x,
            y: center + offset,
            width: thickness,
            height: vertical_length,
        });
        rects.push(Rect {
            x,
            y: center - offset - vertical_length,
            width: thickness,
            height: vertical_length,
        });
    }

    rects
}

/// Each account's original settings: its oldest backup.
pub(in crate::ui) fn original_settings(app: &PrimeApp) -> Vec<&GameSettingsProfileMetadata> {
    let mut originals: Vec<&GameSettingsProfileMetadata> = Vec::new();

    for backup in app
        .settings_profiles
        .iter()
        .filter(|profile| profile.purpose == GameSettingsProfilePurpose::Backup)
    {
        match originals
            .iter_mut()
            .find(|original| original.source_account_id == backup.source_account_id)
        {
            Some(original) if backup.captured_at_unix < original.captured_at_unix => {
                *original = backup;
            }
            Some(_) => {}
            None => originals.push(backup),
        }
    }

    originals
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_settings::{CrosshairCenterDot, CrosshairOutline};

    fn plus(outline: Option<CrosshairOutline>) -> CrosshairSummary {
        CrosshairSummary {
            name: None,
            color: Rgba::WHITE,
            outline,
            center_dot: None,
            inner_lines: Some(CrosshairLines {
                length: 4.0,
                vertical_length: 3.0,
                thickness: 2.0,
                offset: 1.0,
                opacity: 1.0,
            }),
            outer_lines: None,
        }
    }

    #[test]
    fn saves_read_as_relative_times_for_a_week_then_as_dates() {
        let now = 1_800_000_000;
        assert_eq!(saved_label(now - 2 * 86_400, now), "2 days ago");
        assert_eq!(saved_label(now - 86_400, now), "1 day ago");
        assert_eq!(saved_label(now - 5 * 60, now), "5 min ago");
        let older = saved_label(now - 30 * 86_400, now);
        assert!(older.starts_with("Dec "), "{older}");
    }

    #[test]
    fn an_account_added_again_is_still_the_presets_source() {
        let mut account = AccountProfile::new("Main", crate::account::Shard::Na).expect("account");
        let mut profile = GameSettingsProfileMetadata {
            id: "preset".to_string(),
            name: "Main settings".to_string(),
            purpose: GameSettingsProfilePurpose::Profile,
            source_account_id: account.id,
            source_display_name: "Main".to_string(),
            source_puuid: "main-puuid".to_string(),
            captured_at_unix: 0,
            settings_version: None,
            summary: Box::default(),
        };
        assert!(is_source(&account, &profile));

        profile.source_account_id = AccountId::new();
        assert!(!is_source(&account, &profile));

        account.puuid = Some("main-puuid".to_string());
        assert!(is_source(&account, &profile));
    }

    #[test]
    fn formats_numbers_without_trailing_zeros() {
        assert_eq!(format_number(0.34), "0.34");
        assert_eq!(format_number(0.6), "0.6");
        assert_eq!(format_number(1.0), "1");
    }

    #[test]
    fn draws_the_four_arms_around_the_centre() {
        let rects = crosshair_rects(&plus(None), 88.0)
            .into_iter()
            .map(|(rect, _)| rect)
            .collect::<Vec<_>>();

        assert_eq!(
            rects,
            [
                // Right and left: 4 long, 2 thick, 1 from the centre, at 2x.
                Rect {
                    x: 46.0,
                    y: 42.0,
                    width: 8.0,
                    height: 4.0,
                },
                Rect {
                    x: 34.0,
                    y: 42.0,
                    width: 8.0,
                    height: 4.0,
                },
                // Down and up use the vertical length.
                Rect {
                    x: 42.0,
                    y: 46.0,
                    width: 4.0,
                    height: 6.0,
                },
                Rect {
                    x: 42.0,
                    y: 36.0,
                    width: 4.0,
                    height: 6.0,
                },
            ]
        );
    }

    #[test]
    fn draws_outlines_under_the_lines() {
        let rects = crosshair_rects(
            &plus(Some(CrosshairOutline {
                thickness: 1.0,
                opacity: 0.5,
            })),
            88.0,
        );

        assert_eq!(rects.len(), 8);
        assert_eq!(rects[0].1, Color::from_rgba(0.0, 0.0, 0.0, 0.5));
        assert_eq!(
            rects[0].0,
            Rect {
                x: 44.0,
                y: 40.0,
                width: 12.0,
                height: 8.0,
            }
        );
        assert_eq!(rects[4].1, Color::WHITE);
    }

    #[test]
    fn a_large_crosshair_shrinks_to_fit() {
        let mut crosshair = plus(None);
        crosshair.inner_lines = Some(CrosshairLines {
            length: 20.0,
            vertical_length: 20.0,
            thickness: 2.0,
            offset: 20.0,
            opacity: 1.0,
        });
        crosshair.center_dot = Some(CrosshairCenterDot {
            size: 4.0,
            opacity: 1.0,
        });

        for (rect, _) in crosshair_rects(&crosshair, 88.0) {
            assert!(rect.x >= 0.0 && rect.y >= 0.0, "{rect:?}");
            assert!(
                rect.x + rect.width <= 88.0 && rect.y + rect.height <= 88.0,
                "{rect:?}"
            );
        }
    }
}
