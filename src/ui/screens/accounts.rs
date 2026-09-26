use std::fmt;

use iced::widget::{button, column, container, pick_list, row, space, text, text_input, tooltip};
use iced::{Color, Element, Length, Padding, Theme, alignment};
use time::{OffsetDateTime, UtcOffset};

use crate::account::{AccountId, AccountProfile, CompetitiveRank, Shard};
use crate::game_settings::{GameSettingsProfileMetadata, GameSettingsProfilePurpose};

use crate::ui::components::{anchored_popover, compact_loading_indicator};
use crate::ui::data::account_details::AccountAvailability;
use crate::ui::data::shop::format_whole_number;
use crate::ui::{LauncherCaptureKind, Message, PrimeApp};

const ACCOUNT_MENU_WIDTH: f32 = 190.0;
const ACCOUNT_MENU_TOP_OFFSET: f32 = 48.0;
const ACCOUNT_MENU_RIGHT_INSET: f32 = 14.0;
const ACCOUNT_TITLE_TEXT_SIZE: u32 = 22;
const ACCOUNT_BADGE_HEIGHT: f32 = 22.0;
const ACCOUNT_BADGE_PADDING: [u16; 2] = [0, 8];
const ACCOUNT_AVAILABILITY_DOT_SIZE: f32 = 12.0;
const ACCOUNT_AVAILABILITY_DOT_TOP_PADDING: f32 = 2.0;
const ACCOUNT_PENALTY_BADGE_SIZE: f32 = 20.0;

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

    let mut controls = row![
        add_account_button(app),
        add_current_account_button(app),
        button("Import account").on_press_maybe(
            (!app.import_account_in_progress && !app.launcher_capture_in_progress)
                .then_some(Message::OpenImportAccount)
        )
    ]
    .spacing(10);

    if let Some(capture) = &app.login_capture {
        // Enabled once Riot Client has reopened and the capture is waiting for a sign-in.
        controls = controls.push(
            button("Cancel login capture")
                .style(iced::widget::button::danger)
                .on_press_maybe(
                    capture
                        .wait
                        .is_some()
                        .then_some(Message::CancelLoginCapture),
                ),
        );
    }

    let mut content = column![].spacing(12).width(Length::Fill);

    // First, so it is in view when a capture finishes.
    if let Some(draft) = &app.pending_account {
        content = content.push(
            container(
                column![
                    text("Confirm captured account").size(22),
                    text(format!(
                        "Riot ID: {} | PUUID: {}",
                        draft
                            .riot_id()
                            .unwrap_or_else(|| "not captured".to_string()),
                        draft.puuid
                    )),
                    row![
                        text_input("Display name", &app.new_display_name)
                            .on_input(Message::NewDisplayNameChanged)
                            .on_submit(Message::ConfirmCapturedAccount)
                            .width(Length::Fill),
                        text_input("Riot username (optional)", &app.new_username)
                            .on_input(Message::NewUsernameChanged)
                            .on_submit(Message::ConfirmCapturedAccount)
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

    content = content.push(controls);
    if app.settings_cloning {
        content = content.push(settings_profiles_section(app));
    }

    content.push(account_cards).into()
}

/// A saved settings profile as the picker shows it.
#[derive(Clone, Debug, PartialEq)]
struct SettingsProfileChoice {
    profile: GameSettingsProfileMetadata,
    label: String,
}

impl SettingsProfileChoice {
    fn new(profile: &GameSettingsProfileMetadata) -> Self {
        Self {
            label: settings_profile_label(profile),
            profile: profile.clone(),
        }
    }
}

impl fmt::Display for SettingsProfileChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

fn settings_profiles_section(app: &PrimeApp) -> Element<'_, Message> {
    let mut profiles = column![
        text("Settings profiles").size(18),
        text(
            "Saved VALORANT settings profiles can be applied to any account with API access. \
             Applying one first backs up that account's current settings, listed here as a backup."
        )
        .size(13)
    ]
    .spacing(8)
    .width(Length::Fill);

    if app.settings_profiles.is_empty() {
        profiles = profiles.push(text("No saved settings profiles yet").size(13));
    } else {
        let profile_to_apply = app.settings_profile_to_apply();
        let settings_busy =
            app.settings_saving_account.is_some() || app.settings_applying_account.is_some();

        profiles = profiles.push(
            row![
                text("Profile to apply").size(13).width(Length::Fill),
                pick_list(
                    app.settings_profiles
                        .iter()
                        .map(SettingsProfileChoice::new)
                        .collect::<Vec<_>>(),
                    profile_to_apply.map(SettingsProfileChoice::new),
                    |choice| Message::GameSettingsProfileSelected(choice.profile)
                )
                .width(380)
            ]
            .spacing(10)
            .align_y(alignment::Vertical::Center),
        );

        for profile in &app.settings_profiles {
            let chosen = profile_to_apply.is_some_and(|chosen| chosen.id == profile.id);
            profiles = profiles.push(settings_profile_card(profile, chosen, !settings_busy));
        }
    }

    container(profiles)
        .padding(14)
        .width(Length::Fill)
        .style(iced::widget::container::bordered_box)
        .into()
}

fn settings_profile_card(
    profile: &GameSettingsProfileMetadata,
    chosen: bool,
    can_delete: bool,
) -> Element<'_, Message> {
    let kind = match profile.purpose {
        GameSettingsProfilePurpose::Profile => "Saved profile",
        GameSettingsProfilePurpose::Backup => "Backup made by Apply",
    };
    let version = profile
        .settings_version
        .map(|version| format!("Version {version}"))
        .unwrap_or_else(|| "Version unknown".to_string());
    let details = format!(
        "{kind} | {} | Source: {} | Saved: {}",
        version,
        profile.source_display_name,
        last_refreshed_label(Some(profile.captured_at_unix))
    );
    let select = if chosen {
        button("Selected")
    } else {
        button("Select").on_press(Message::GameSettingsProfileSelected(profile.clone()))
    };

    container(
        column![
            row![
                text(&profile.name).size(15).width(Length::Fill),
                select,
                button("Delete")
                    .style(iced::widget::button::danger)
                    .on_press_maybe(
                        can_delete
                            .then(|| Message::RequestDeleteSettingsProfile(profile.id.clone()))
                    )
            ]
            .spacing(10)
            .align_y(alignment::Vertical::Center),
            text(details).size(13),
            text(profile.summary.categories_label()).size(13),
            text(format!("Includes: {}", profile.summary.examples_label())).size(13)
        ]
        .spacing(6),
    )
    .padding(10)
    .width(Length::Fill)
    .style(iced::widget::container::bordered_box)
    .into()
}

fn add_account_button(app: &PrimeApp) -> Element<'static, Message> {
    let is_capturing_new_account = app.launcher_capture_kind == Some(LauncherCaptureKind::New);
    let content: Element<_> = if is_capturing_new_account {
        row![
            compact_loading_indicator(app.loading_frame),
            text("Capturing login...")
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center)
        .into()
    } else {
        text("Add new account").into()
    };

    let button: Element<'static, Message> = button(content)
        .on_press_maybe(
            (!app.launcher_capture_in_progress && !app.launch_in_progress())
                .then_some(Message::AddAccount),
        )
        .into();
    let tip =
        container(text("Adds an account by opening Riot Client for a login capture").size(13))
            .padding([6, 8])
            .style(iced::widget::container::bordered_box);

    tooltip(button, tip, tooltip::Position::Bottom).into()
}

fn add_current_account_button(app: &PrimeApp) -> Element<'static, Message> {
    let is_capturing_current_account =
        app.launcher_capture_kind == Some(LauncherCaptureKind::Current);
    let content: Element<_> = if is_capturing_current_account {
        row![
            compact_loading_indicator(app.loading_frame),
            text("Capturing current...")
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center)
        .into()
    } else {
        text("Add current account").into()
    };

    let button: Element<'static, Message> = button(content)
        .on_press_maybe(
            (!app.launcher_capture_in_progress && !app.launch_in_progress())
                .then_some(Message::AddCurrentAccount),
        )
        .into();
    let tip = container(text("Adds the Riot account currently logged into Riot Client").size(13))
        .padding([6, 8])
        .style(iced::widget::container::bordered_box);

    tooltip(button, tip, tooltip::Position::Bottom).into()
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
    let is_checking_launch = app.launch_preflight_account == Some(account.id);
    let launch_in_progress =
        app.launching_account.is_some() || app.launch_preflight_account.is_some();
    let selected_label = if is_selected {
        "Selected"
    } else {
        "Not selected"
    };
    let detail_badges = row![
        text(riot_tag).size(15),
        level_badge(app, account),
        rank_badge(app, account)
    ]
    .spacing(8)
    .align_y(alignment::Vertical::Center);

    let header = row![
        column![account_status_row(app, account), detail_badges]
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
        last_refreshed_row(app, account),
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
                is_checking_launch,
                launch_in_progress || app.launcher_capture_in_progress,
                account.has_launcher_session(),
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
        account_menu(app, account),
        is_account_menu_open,
        ACCOUNT_MENU_TOP_OFFSET,
        ACCOUNT_MENU_RIGHT_INSET,
    )
}

fn account_status_row<'a>(app: &'a PrimeApp, account: &'a AccountProfile) -> Element<'a, Message> {
    let mut status = row![
        text(&account.display_name).size(ACCOUNT_TITLE_TEXT_SIZE),
        account_availability_indicator(app, account)
    ]
    .spacing(8)
    .align_y(alignment::Vertical::Center);

    if let Some(indicator) = penalty_indicator(account) {
        status = status.push(indicator);
    }

    status.into()
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
    } else if app.account_ranks_loading.contains(&account.id) {
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
    is_checking_launch: bool,
    launch_blocked: bool,
    login_captured: bool,
) -> Element<'static, Message> {
    let content: Element<_> = if is_launching {
        row![compact_loading_indicator(loading_frame), text("Opening...")]
            .spacing(8)
            .align_y(alignment::Vertical::Center)
            .into()
    } else if is_checking_launch {
        row![
            compact_loading_indicator(loading_frame),
            text("Checking...")
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center)
        .into()
    } else {
        text("Launch VALORANT").into()
    };

    let button = button(content).on_press_maybe(
        (!launch_blocked && login_captured).then_some(Message::LaunchAccount(account_id)),
    );

    if login_captured {
        return button.into();
    }

    let tip =
        container(text("Capture this account's login first: ... > Re-capture login").size(13))
            .padding([6, 8])
            .style(iced::widget::container::bordered_box);

    tooltip(button, tip, tooltip::Position::Top).into()
}

fn account_availability_indicator<'a>(
    app: &'a PrimeApp,
    account: &'a AccountProfile,
) -> Element<'a, Message> {
    let availability = app
        .account_availability
        .get(&account.id)
        .cloned()
        .unwrap_or_else(|| {
            if app.account_availability_loading {
                AccountAvailability::checking()
            } else {
                AccountAvailability::not_checked()
            }
        });
    let color = account_availability_color(&availability);
    let label = availability.label();
    let dot = container(space())
        .width(ACCOUNT_AVAILABILITY_DOT_SIZE)
        .height(ACCOUNT_AVAILABILITY_DOT_SIZE)
        .style(move |_| account_availability_dot_style(color));
    let dot = container(dot)
        .height(ACCOUNT_TITLE_TEXT_SIZE as f32)
        .padding(Padding::default().top(ACCOUNT_AVAILABILITY_DOT_TOP_PADDING))
        .align_y(alignment::Vertical::Center);
    let tip = container(text(label).size(13))
        .padding([6, 8])
        .style(iced::widget::container::bordered_box);

    tooltip(dot, tip, tooltip::Position::Right).into()
}

fn rank_badge<'a>(app: &PrimeApp, account: &'a AccountProfile) -> Element<'a, Message> {
    if let Some(rank) = &account.competitive_rank {
        let color = rank_color(rank);

        container(rank_badge_label(&rank.rank_name, rank.ranked_rating, color))
            .height(ACCOUNT_BADGE_HEIGHT)
            .clip(true)
            .style(move |theme| rank_badge_style(theme, Some(color)))
            .into()
    } else if app.account_ranks_loading.contains(&account.id) {
        loading_rank_badge(app.loading_frame)
    } else {
        neutral_rank_badge(missing_rank_label(app, account.id))
    }
}

/// What the rank badge says without a rank: the account has none, or it couldn't be loaded.
pub(in crate::ui) fn missing_rank_label(app: &PrimeApp, account_id: AccountId) -> &'static str {
    if app.unranked_accounts.contains(&account_id) {
        "Unranked"
    } else {
        "Rank unavailable"
    }
}

fn penalty_indicator(account: &AccountProfile) -> Option<Element<'static, Message>> {
    let label = account.penalty_status.tooltip_label()?;
    let badge = container(text("!").size(13))
        .width(ACCOUNT_PENALTY_BADGE_SIZE)
        .height(ACCOUNT_PENALTY_BADGE_SIZE)
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Center)
        .style(penalty_badge_style);
    let badge = container(badge)
        .height(ACCOUNT_TITLE_TEXT_SIZE as f32)
        .align_y(alignment::Vertical::Center);
    let tip = container(text(label).size(13))
        .padding([6, 8])
        .style(iced::widget::container::bordered_box);

    Some(tooltip(badge, tip, tooltip::Position::Right).into())
}

fn last_refreshed_row<'a>(app: &'a PrimeApp, account: &'a AccountProfile) -> Element<'a, Message> {
    if app.profile_identity_refreshing.contains(&account.id) {
        return row![
            compact_loading_indicator(app.loading_frame),
            text("Last refreshed: Refreshing...").size(13)
        ]
        .spacing(6)
        .align_y(alignment::Vertical::Center)
        .into();
    }

    text(format!(
        "Last refreshed: {}",
        last_refreshed_label(launcher_session_captured_at_unix(account))
    ))
    .size(13)
    .into()
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

fn account_menu(app: &PrimeApp, account: &AccountProfile) -> Element<'static, Message> {
    let account_id = account.id;
    let settings_busy =
        app.settings_saving_account.is_some() || app.settings_applying_account.is_some();
    let account_has_api_access = account.has_launcher_session() || account.session.is_some();

    let mut actions = column![
        button("Re-capture login")
            .width(Length::Fill)
            .on_press_maybe(
                (!app.launcher_capture_in_progress && !app.launch_in_progress())
                    .then_some(Message::RequestLauncherSessionLogin(account_id))
            ),
        button("Refresh profile")
            .width(Length::Fill)
            .on_press_maybe(
                (!app.profile_identity_refreshing.contains(&account_id))
                    .then_some(Message::RefreshProfileIdentity(account_id))
            ),
    ]
    .spacing(8);

    if app.settings_cloning {
        actions = actions
            .push(
                button("Save VALORANT settings")
                    .width(Length::Fill)
                    .on_press_maybe(
                        (!settings_busy && account_has_api_access)
                            .then_some(Message::SaveAccountSettings(account_id)),
                    ),
            )
            .push(
                button("Apply settings profile")
                    .width(Length::Fill)
                    .on_press_maybe(
                        (!settings_busy
                            && account_has_api_access
                            && !app.settings_profiles.is_empty())
                        .then_some(Message::RequestApplySavedSettings(account_id)),
                    ),
            );
    }

    container(
        actions
            .push(
                button("Export account")
                    .width(Length::Fill)
                    .on_press(Message::RequestExportAccount(account_id)),
            )
            .push(
                button("Delete account")
                    .width(Length::Fill)
                    .style(iced::widget::button::danger)
                    .on_press(Message::RequestDeleteAccount(account_id)),
            ),
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

/// Names a saved settings profile with its save time, since repeated saves share a name.
pub(in crate::ui) fn settings_profile_label(profile: &GameSettingsProfileMetadata) -> String {
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);

    settings_profile_label_at(&profile.name, profile.captured_at_unix, offset)
}

fn settings_profile_label_at(name: &str, captured_at_unix: i64, offset: UtcOffset) -> String {
    match OffsetDateTime::from_unix_timestamp(captured_at_unix) {
        Ok(saved_at) => format!("{name} (saved {})", format_refreshed_at(saved_at, offset)),
        Err(_) => name.to_string(),
    }
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

fn account_availability_color(availability: &AccountAvailability) -> Color {
    match availability {
        AccountAvailability::Available => Color::from_rgb8(77, 201, 116),
        AccountAvailability::Unavailable { .. } => Color::from_rgb8(236, 89, 94),
        AccountAvailability::Unknown { .. } => Color::from_rgb8(122, 130, 142),
    }
}

fn account_availability_dot_style(color: Color) -> iced::widget::container::Style {
    let mut style = iced::widget::container::Style {
        background: Some(color.into()),
        ..Default::default()
    };
    style.border.radius = iced::border::radius(ACCOUNT_AVAILABILITY_DOT_SIZE / 2.0);
    style.border.width = 1.0;
    style.border.color = Color::from_rgba8(255, 255, 255, 0.35);
    style
}

fn penalty_badge_style(_: &Theme) -> iced::widget::container::Style {
    let mut style = iced::widget::container::Style {
        background: Some(Color::from_rgba8(224, 87, 92, 0.20).into()),
        text_color: Some(Color::from_rgb8(255, 224, 224)),
        ..Default::default()
    };
    style.border.radius = iced::border::radius(ACCOUNT_PENALTY_BADGE_SIZE / 2.0);
    style.border.width = 1.0;
    style.border.color = Color::from_rgb8(224, 87, 92);
    style
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

    #[test]
    fn settings_profile_label_includes_the_saved_time() {
        assert_eq!(
            settings_profile_label_at("Main settings", 1_800_032_400, UtcOffset::UTC),
            "Main settings (saved 2027-01-15 5:00 PM)"
        );
    }

    #[test]
    fn last_refreshed_uses_launcher_capture_time() {
        let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
        account.last_refreshed_at_unix = Some(50);
        account.launcher_session = Some(crate::account::LauncherSessionBackup {
            data_dir: std::path::PathBuf::from("backup"),
            captured_at_unix: 100,
            puuid: "puuid".to_string(),
        });

        assert_eq!(launcher_session_captured_at_unix(&account), Some(100));
    }
}
