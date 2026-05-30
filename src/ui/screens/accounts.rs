use iced::widget::{button, column, container, pick_list, row, space, text, text_input};
use iced::{Color, Element, Length, Theme, alignment};
use time::{OffsetDateTime, UtcOffset};

use crate::account::{AccountId, AccountProfile, CompetitiveRank, Shard};

use super::super::components::{anchored_popover, compact_loading_indicator};
use super::super::data::format_whole_number;
use super::super::{Message, PrimeApp};

const ACCOUNT_MENU_WIDTH: f32 = 190.0;
const ACCOUNT_MENU_TOP_OFFSET: f32 = 48.0;
const ACCOUNT_MENU_RIGHT_INSET: f32 = 14.0;
const ACCOUNT_BADGE_HEIGHT: f32 = 22.0;
const ACCOUNT_BADGE_PADDING: [u16; 2] = [0, 8];

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    let mut account_cards = column![].spacing(12).width(Length::Fill);

    if app.state.accounts.is_empty() {
        account_cards = account_cards.push(
            container(text("No account profiles yet"))
                .padding(16)
                .width(Length::Fill)
                .style(iced::widget::container::bordered_box),
        );
    }

    for account in &app.state.accounts {
        account_cards = account_cards.push(account_card(app, account));
    }

    let controls = row![
        add_account_button(app),
        button("Import account").on_press_maybe(
            (!app.import_account_in_progress).then_some(Message::OpenImportAccount)
        )
    ]
    .spacing(10);

    let mut content = column![controls].spacing(12).width(Length::Fill);

    content = content.push(account_cards);

    if let Some(draft) = &app.pending_account {
        content = content.push(
            container(
                column![
                    text("Confirm captured account").size(22),
                    text(format!("PUUID: {}", draft.puuid)),
                    row![
                        text_input("Display name", &app.new_display_name)
                            .on_input(Message::NewDisplayNameChanged)
                            .width(Length::Fill),
                        text_input("Riot username", &app.new_username)
                            .on_input(Message::NewUsernameChanged)
                            .width(Length::Fill),
                        pick_list(
                            Shard::ALL.as_slice(),
                            Some(app.new_shard),
                            Message::NewShardSelected
                        )
                    ]
                    .spacing(10),
                    row![
                        button("Save account").on_press(Message::ConfirmCapturedAccount),
                        button("Cancel").on_press(Message::CancelCapturedAccount)
                    ]
                    .spacing(10)
                ]
                .spacing(10),
            )
            .padding(16)
            .style(iced::widget::container::bordered_box),
        );
    }

    content.into()
}

fn add_account_button(app: &PrimeApp) -> Element<'static, Message> {
    let content: Element<_> = if app.launcher_capture_in_progress {
        row![
            compact_loading_indicator(app.loading_frame),
            text("Capturing login...")
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center)
        .into()
    } else {
        text("Add account").into()
    };

    button(content)
        .on_press_maybe((!app.launcher_capture_in_progress).then_some(Message::AddAccount))
        .into()
}

fn account_card<'a>(app: &'a PrimeApp, account: &'a AccountProfile) -> Element<'a, Message> {
    let riot_tag = account
        .riot_id()
        .unwrap_or_else(|| "Riot tag not captured".to_string());
    let session_label = if account.has_launcher_session() {
        "Launcher session captured"
    } else {
        "Launcher session not captured"
    };
    let is_selected = app.state.selected_account == Some(account.id);
    let is_account_menu_open = app.open_account_menu == Some(account.id);
    let is_launching = app.launching_account == Some(account.id);
    let launch_in_progress = app.launching_account.is_some();
    let selected_label = if is_selected {
        "Selected"
    } else {
        "Not selected"
    };
    let header = row![
        column![
            text(&account.display_name).size(22),
            row![
                text(riot_tag).size(15),
                level_badge(app, account),
                rank_badge(app, account)
            ]
            .spacing(8)
            .align_y(alignment::Vertical::Center)
        ]
        .spacing(4)
        .width(Length::Fill),
        button(text("..."))
            .style(move |theme, status| {
                account_menu_button_style(theme, status, is_account_menu_open)
            })
            .on_press(Message::ToggleAccountMenu(account.id))
    ]
    .spacing(10)
    .align_y(alignment::Vertical::Top);

    let body = column![
        header,
        text(format!(
            "Riot shard: {} | {} | {}",
            account.shard, session_label, selected_label
        ))
        .size(13),
        text(format!(
            "Last refreshed: {}",
            last_refreshed_label(account.last_refreshed_at_unix)
        ))
        .size(13),
    ]
    .spacing(10)
    .width(Length::Fill);

    let body = body.push(
        row![
            button("Select")
                .style(move |theme, status| {
                    select_account_button_style(theme, status, is_selected)
                })
                .on_press_maybe((!is_selected).then_some(Message::SelectAccount(account.id))),
            space().width(Length::Fill),
            launch_button(
                account.id,
                app.loading_frame,
                is_launching,
                launch_in_progress
            )
        ]
        .spacing(10)
        .width(Length::Fill),
    );

    let card = container(body)
        .padding(14)
        .width(Length::Fill)
        .style(move |theme| account_card_style(theme, is_selected));

    anchored_popover(
        card,
        account_menu(account.id),
        is_account_menu_open,
        ACCOUNT_MENU_TOP_OFFSET,
        ACCOUNT_MENU_RIGHT_INSET,
    )
}

fn level_badge(app: &PrimeApp, account: &AccountProfile) -> Element<'static, Message> {
    if let Some(level) = account.account_level {
        container(text(format!("Level {}", format_whole_number(level))).size(13))
            .height(ACCOUNT_BADGE_HEIGHT)
            .padding(ACCOUNT_BADGE_PADDING)
            .align_y(alignment::Vertical::Center)
            .clip(true)
            .style(level_badge_style)
            .into()
    } else if app.account_ranks_loading {
        loading_level_badge(app.loading_frame)
    } else {
        neutral_rank_badge("Level unavailable")
    }
}

fn loading_level_badge(frame: usize) -> Element<'static, Message> {
    container(
        row![compact_loading_indicator(frame), text("Level").size(13)]
            .spacing(6)
            .align_y(alignment::Vertical::Center),
    )
    .height(ACCOUNT_BADGE_HEIGHT)
    .padding(ACCOUNT_BADGE_PADDING)
    .align_y(alignment::Vertical::Center)
    .clip(true)
    .style(|theme| rank_badge_style(theme, None))
    .into()
}

fn launch_button(
    account_id: AccountId,
    loading_frame: usize,
    is_launching: bool,
    launch_in_progress: bool,
) -> Element<'static, Message> {
    let content: Element<_> = if is_launching {
        row![compact_loading_indicator(loading_frame), text("Opening...")]
            .spacing(8)
            .align_y(alignment::Vertical::Center)
            .into()
    } else {
        text("Launch VALORANT").into()
    };

    button(content)
        .on_press_maybe((!launch_in_progress).then_some(Message::LaunchAccount(account_id)))
        .into()
}

fn rank_badge<'a>(app: &PrimeApp, account: &'a AccountProfile) -> Element<'a, Message> {
    if let Some(rank) = &account.competitive_rank {
        let color = rank_color(rank);

        container(rank_badge_label(&rank.rank_name, rank.ranked_rating, color))
            .height(ACCOUNT_BADGE_HEIGHT)
            .clip(true)
            .style(move |theme| rank_badge_style(theme, Some(color)))
            .into()
    } else if app.account_ranks_loading {
        loading_rank_badge(app.loading_frame)
    } else {
        neutral_rank_badge("Unavailable")
    }
}

fn rank_badge_label<'a>(
    rank_name: &'a str,
    ranked_rating: i64,
    accent: Color,
) -> Element<'a, Message> {
    row![
        container(text(rank_name).size(13))
            .height(ACCOUNT_BADGE_HEIGHT)
            .padding(ACCOUNT_BADGE_PADDING)
            .align_y(alignment::Vertical::Center),
        rank_badge_divider(accent),
        container(text(format!("{} RR", ranked_rating)).size(13))
            .height(ACCOUNT_BADGE_HEIGHT)
            .padding(ACCOUNT_BADGE_PADDING)
            .align_y(alignment::Vertical::Center)
    ]
    .height(ACCOUNT_BADGE_HEIGHT)
    .align_y(alignment::Vertical::Center)
    .into()
}

fn rank_badge_divider(accent: Color) -> Element<'static, Message> {
    container(space())
        .width(1.0)
        .height(ACCOUNT_BADGE_HEIGHT)
        .style(move |_| rank_badge_divider_style(accent))
        .into()
}

fn neutral_rank_badge(label: impl Into<String>) -> Element<'static, Message> {
    container(text(label.into()).size(13))
        .height(ACCOUNT_BADGE_HEIGHT)
        .padding(ACCOUNT_BADGE_PADDING)
        .align_y(alignment::Vertical::Center)
        .clip(true)
        .style(|theme| rank_badge_style(theme, None))
        .into()
}

fn loading_rank_badge(frame: usize) -> Element<'static, Message> {
    container(
        row![compact_loading_indicator(frame), text("Loading").size(13)]
            .spacing(6)
            .align_y(alignment::Vertical::Center),
    )
    .height(ACCOUNT_BADGE_HEIGHT)
    .padding(ACCOUNT_BADGE_PADDING)
    .align_y(alignment::Vertical::Center)
    .clip(true)
    .style(|theme| rank_badge_style(theme, None))
    .into()
}

fn account_menu(account_id: AccountId) -> Element<'static, Message> {
    container(
        column![
            button("Re-capture login")
                .width(Length::Fill)
                .on_press(Message::StartLauncherSessionLogin(account_id)),
            button("Refresh profile")
                .width(Length::Fill)
                .on_press(Message::RefreshProfileIdentity(account_id)),
            button("Export account")
                .width(Length::Fill)
                .on_press(Message::RequestExportAccount(account_id)),
            button("Delete account")
                .width(Length::Fill)
                .style(iced::widget::button::danger)
                .on_press(Message::RequestDeleteAccount(account_id))
        ]
        .spacing(8),
    )
    .padding(8)
    .width(ACCOUNT_MENU_WIDTH)
    .style(iced::widget::container::bordered_box)
    .into()
}

fn last_refreshed_label(timestamp: Option<i64>) -> String {
    let Some(timestamp) = timestamp else {
        return "Never".to_string();
    };
    let Ok(refreshed_at) = OffsetDateTime::from_unix_timestamp(timestamp) else {
        return "Unknown".to_string();
    };
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);

    format_refreshed_at(refreshed_at, offset)
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

fn account_card_style(theme: &Theme, selected: bool) -> iced::widget::container::Style {
    let mut style = iced::widget::container::bordered_box(theme);

    if selected {
        style.background = Some(Color::from_rgba8(30, 48, 67, 0.55).into());
        style.border.color = Color::from_rgb8(95, 176, 224);
    }

    style
}

fn rank_badge_style(theme: &Theme, accent: Option<Color>) -> iced::widget::container::Style {
    let mut style = iced::widget::container::bordered_box(theme);

    match accent {
        Some(color) => {
            style.background = Some(Color::from_rgba(color.r, color.g, color.b, 0.12).into());
            style.border.color = Color::from_rgba(color.r, color.g, color.b, 0.62);
            style.text_color = Some(Color::from_rgb8(218, 222, 230));
        }
        None => {
            style.background = Some(Color::from_rgba8(48, 52, 59, 0.64).into());
            style.border.color = Color::from_rgb8(88, 94, 105);
            style.text_color = Some(Color::from_rgb8(170, 176, 188));
        }
    }

    style
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

fn level_badge_style(theme: &Theme) -> iced::widget::container::Style {
    let mut style = iced::widget::container::bordered_box(theme);
    let accent = Color::from_rgb8(95, 176, 224);

    style.background = Some(Color::from_rgba(accent.r, accent.g, accent.b, 0.10).into());
    style.border.color = Color::from_rgba(accent.r, accent.g, accent.b, 0.58);
    style.text_color = Some(Color::from_rgb8(218, 226, 234));
    style
}

fn rank_badge_divider_style(accent: Color) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(Color::from_rgba(accent.r, accent.g, accent.b, 0.62).into()),
        ..Default::default()
    }
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

fn select_account_button_style(
    theme: &Theme,
    status: iced::widget::button::Status,
    selected: bool,
) -> iced::widget::button::Style {
    if !selected {
        return iced::widget::button::primary(theme, status);
    }

    let mut style = iced::widget::button::secondary(theme, iced::widget::button::Status::Disabled);
    style.background = Some(Color::from_rgb8(68, 72, 78).into());
    style.text_color = Color::from_rgb8(180, 184, 190);
    style
}

fn account_menu_button_style(
    theme: &Theme,
    status: iced::widget::button::Status,
    is_open: bool,
) -> iced::widget::button::Style {
    if !is_open {
        return iced::widget::button::primary(theme, status);
    }

    iced::widget::button::secondary(theme, status)
}
