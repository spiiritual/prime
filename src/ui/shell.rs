use iced::widget::image::Handle;
use iced::widget::text::Wrapping;
use iced::widget::{
    column, container, image, opaque, row, scrollable, space, stack, text, text_input,
};
use iced::{Color, ContentFit, Element, Length, Padding, Theme, alignment};

use crate::account::AccountProfile;
use crate::game_settings::{GameSettingsProfileMetadata, GameSettingsProfilePurpose};

use super::components::{anchored_popover, currency_balance_display, loading_indicator};
use super::theme::{self, button};
use super::{
    AccountExportOutput, ImageViewerImage, MAIN_PANEL_SCROLLABLE_ID, Message,
    PendingSettingsChange, PresetNamePrompt, PresetNameTarget, PrimeApp, SettingsChange, Tab,
    UnavailableLaunchWarning, screens,
};
use super::{status_bar_visible, status_message_is_error, status_spinner_active};

/// Without its one-pixel border line.
const SIDEBAR_WIDTH: f32 = 231.0;
const ACCOUNT_SWITCHER_WIDTH: f32 = SIDEBAR_WIDTH - 32.0;
const ACCOUNT_SWITCHER_MENU_TOP_OFFSET: f32 = 60.0;
const ACCOUNT_SWITCHER_MENU_WIDTH: f32 = 280.0;
const POPOVER_BORDER: Color = iced::color!(0x2E3542);
const STATUS_TOAST_MAX_WIDTH: f32 = 640.0;
const UPDATE_CHANGELOG_MAX_HEIGHT: f32 = 260.0;

impl PrimeApp {
    pub(super) fn view(&self) -> Element<'_, Message> {
        let content = row![
            self.sidebar(),
            container(self.main_panel())
                .padding(Padding {
                    top: 28.0,
                    right: 18.0,
                    bottom: 0.0,
                    left: 36.0,
                })
                .width(Length::Fill)
                .height(Length::Fill)
        ]
        .height(Length::Fill);

        let pending_delete_account = self.confirm_delete_account.and_then(|account_id| {
            self.state
                .accounts
                .iter()
                .find(|account| account.id == account_id)
        });

        let pending_recapture_account = self.confirm_recapture_account.and_then(|account_id| {
            self.state
                .accounts
                .iter()
                .find(|account| account.id == account_id)
        });

        let pending_settings_delete =
            self.confirm_delete_settings_profile
                .as_ref()
                .and_then(|profile_id| {
                    self.settings_profiles
                        .iter()
                        .find(|profile| &profile.id == profile_id)
                });

        let content: Element<_> = if self.show_add_account_prompt {
            stack![
                content,
                add_account_prompt_overlay(
                    self.capture_prompt_valorant_running,
                    self.pending_account.is_some()
                )
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else if self.show_import_account_prompt {
            stack![content, import_account_prompt_overlay(self)]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if let Some(export) = &self.exported_account {
            stack![content, export_account_prompt_overlay(export)]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if let Some(account) = pending_delete_account {
            stack![content, delete_account_prompt_overlay(account)]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if let Some(account) = pending_recapture_account {
            stack![
                content,
                recapture_prompt_overlay(account, self.capture_prompt_valorant_running)
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else if let Some(prompt) = self
            .confirm_settings_change
            .as_ref()
            .and_then(|pending| settings_change_prompt_overlay(self, pending))
        {
            stack![content, prompt]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if let Some(profile) = pending_settings_delete {
            stack![content, delete_settings_profile_prompt_overlay(profile)]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if let Some(prompt) = &self.preset_name_prompt {
            stack![content, preset_name_prompt_overlay(self, prompt)]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if let Some(warning) = &self.unavailable_launch_warning {
            stack![content, unavailable_launch_prompt_overlay(warning)]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if let Some(update) = self.app_update_status.prompt_update() {
            stack![
                content,
                app_update_prompt_overlay(update, self.work_blocking_update())
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            content.into()
        };

        if super::image_viewer_enabled()
            && let Some(image) = &self.image_viewer
        {
            stack![content, image_viewer_overlay(image, self.loading_frame)]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            content
        }
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let brand = row![
            theme::sized_icon(theme::Icon::Logo, 30.0, 28.0, theme::TEXT),
            text("prime").size(20).font(theme::DISPLAY_FONT)
        ]
        .spacing(6)
        .padding([0, 8])
        .align_y(alignment::Vertical::Center);

        let nav = column![
            self.tab_button(Tab::Accounts),
            self.tab_button(Tab::Shop),
            self.tab_button(Tab::Loadout),
            self.tab_button(Tab::Settings),
        ]
        .spacing(2);

        let version = text(format!("v{}", env!("CARGO_PKG_VERSION")))
            .size(11)
            .font(theme::MONO_FONT)
            .color(theme::FAINT);

        let sidebar = container(
            column![
                brand,
                self.account_switcher(),
                nav,
                space().height(Length::Fill),
                container(version).padding([0, 8])
            ]
            .spacing(28),
        )
        .padding([24, 16])
        .width(SIDEBAR_WIDTH)
        .height(Length::Fill)
        .style(|_| filled(theme::SURFACE));

        row![sidebar, rule(theme::LINE)].into()
    }

    fn account_switcher(&self) -> Element<'_, Message> {
        anchored_popover(
            self.account_badge(),
            self.account_switcher_menu(),
            self.account_switcher_open,
            ACCOUNT_SWITCHER_MENU_TOP_OFFSET,
            // Negative, so the wider menu lines up with the badge's left edge.
            ACCOUNT_SWITCHER_WIDTH - ACCOUNT_SWITCHER_MENU_WIDTH,
        )
    }

    fn account_badge(&self) -> Element<'_, Message> {
        let account = self.state.selected_account();
        let is_open = self.account_switcher_open;
        let display_name = account
            .map(|account| account.display_name.as_str())
            .unwrap_or("No profile");
        let avatar = match account {
            Some(account) => account_avatar(&account.display_name, 34.0, 8.0),
            None => container(theme::icon(theme::Icon::Users, 17.0, theme::MUTED))
                .center_x(34)
                .center_y(34)
                .style(|_| filled(theme::LINE).border(iced::border::rounded(8)))
                .into(),
        };
        let detail = account
            .map(account_detail_label)
            .unwrap_or_else(|| "Add or select an account".to_string());

        let content = row![
            avatar,
            column![
                text(display_name)
                    .size(13)
                    .font(theme::SEMIBOLD_FONT)
                    .wrapping(Wrapping::None),
                text(detail)
                    .size(11)
                    .color(theme::MUTED)
                    .wrapping(Wrapping::None)
            ]
            .spacing(2)
            .width(Length::Fill)
            .clip(true),
            theme::icon(theme::Icon::ChevronsUpDown, 16.0, theme::MUTED)
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center);

        button(content)
            .padding(10)
            .width(ACCOUNT_SWITCHER_WIDTH)
            .style(move |theme, status| account_badge_button_style(theme, status, is_open))
            .on_press_maybe(
                (!self.state.accounts.is_empty()).then_some(Message::ToggleAccountSwitcher),
            )
            .into()
    }

    fn account_switcher_menu(&self) -> Element<'_, Message> {
        let mut menu = column![
            container(
                text("SWITCH ACCOUNT")
                    .size(10)
                    .font(theme::BOLD_FONT)
                    .color(theme::FAINT)
            )
            .padding(Padding {
                top: 6.0,
                right: 10.0,
                bottom: 8.0,
                left: 10.0,
            })
        ]
        .spacing(1)
        .width(Length::Fill);

        for account in &self.state.accounts {
            let is_selected = self.state.selected_account == Some(account.id);
            menu = menu.push(account_switcher_menu_item(account, is_selected));
        }

        menu = menu
            .push(
                container(space())
                    .width(Length::Fill)
                    .height(1)
                    .style(|_| filled(theme::LINE)),
            )
            .push(menu_action(
                theme::Icon::Plus,
                "Add account",
                Message::AddAccount,
            ))
            .push(menu_action(
                theme::Icon::Settings2,
                "Manage accounts",
                Message::TabSelected(Tab::Accounts),
            ));

        // Opaque, so clicks on its gaps don't reach the page it overhangs.
        opaque(
            container(menu)
                .padding(6)
                .width(ACCOUNT_SWITCHER_MENU_WIDTH)
                .style(popover_style),
        )
    }

    fn main_panel(&self) -> Element<'_, Message> {
        let active_tab = self.active_tab;
        let status_visible = status_bar_visible(self);
        let body = screens::tab(self, self.active_tab);
        let scroll_body = container(body)
            .padding(Padding {
                top: 0.0,
                right: 18.0,
                // Room to scroll the last content above the status toast.
                bottom: if status_visible { 90.0 } else { 28.0 },
                left: 0.0,
            })
            .width(Length::Fill);

        let panel = column![
            self.main_header(),
            scrollable(scroll_body)
                .id(MAIN_PANEL_SCROLLABLE_ID)
                .on_scroll(move |viewport| Message::MainPanelScrolled {
                    tab: active_tab,
                    offset: viewport.absolute_offset(),
                })
                .height(Length::Fill)
        ]
        .spacing(22);

        if status_visible {
            stack![
                panel,
                container(self.status_toast())
                    .padding(Padding::ZERO.bottom(24))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_y(alignment::Vertical::Bottom)
            ]
            .into()
        } else {
            panel.into()
        }
    }

    fn main_header(&self) -> Element<'_, Message> {
        let title = text(self.active_tab.to_string())
            .size(28)
            .font(theme::DISPLAY_FONT);

        let header: Element<_> = match (&self.store_summary, self.active_tab) {
            (Some(summary), Tab::Shop) => row![
                container(title).width(Length::Fill),
                currency_balance_display(summary)
            ]
            .spacing(12)
            .align_y(alignment::Vertical::Center)
            .into(),
            _ => title.into(),
        };

        container(header)
            .padding(Padding::ZERO.right(18))
            .width(Length::Fill)
            .into()
    }

    fn status_toast(&self) -> Element<'_, Message> {
        let lead = if status_spinner_active(self) {
            loading_indicator(self.loading_frame)
        } else if status_message_is_error(&self.status) {
            theme::icon(theme::Icon::TriangleAlert, 15.0, theme::ACCENT)
        } else {
            theme::icon(theme::Icon::Info, 15.0, theme::MUTED)
        };

        container(
            row![lead, text(&self.status).size(12)]
                .spacing(10)
                .align_y(alignment::Vertical::Center),
        )
        .padding([10, 14])
        .max_width(STATUS_TOAST_MAX_WIDTH)
        .style(popover_style)
        .into()
    }

    fn tab_button(&self, tab: Tab) -> Element<'_, Message> {
        let is_selected = self.active_tab == tab;
        let icon = match tab {
            Tab::Accounts => theme::Icon::Users,
            Tab::Shop => theme::Icon::ShoppingBag,
            Tab::Loadout => theme::Icon::Swords,
            Tab::Settings => theme::Icon::Settings,
        };
        let (color, font) = if is_selected {
            (theme::TEXT, theme::SEMIBOLD_FONT)
        } else {
            (theme::MUTED, theme::MEDIUM_FONT)
        };
        let indicator = container(space()).width(3).height(16).style(move |_| {
            let style = filled(if is_selected {
                theme::ACCENT
            } else {
                Color::TRANSPARENT
            });
            style.border(iced::border::rounded(2))
        });

        button(
            row![
                indicator,
                row![
                    theme::icon(icon, 17.0, color),
                    text(tab.to_string()).size(14).font(font)
                ]
                .spacing(12)
                .align_y(alignment::Vertical::Center)
            ]
            .spacing(9)
            .align_y(alignment::Vertical::Center),
        )
        .padding(Padding {
            top: 9.0,
            right: 12.0,
            bottom: 9.0,
            left: 0.0,
        })
        .width(Length::Fill)
        .style(move |theme, status| theme::choice_style(theme, status, is_selected))
        .on_press_maybe((!is_selected).then_some(Message::TabSelected(tab)))
        .into()
    }
}

fn account_switcher_menu_item(account: &AccountProfile, is_selected: bool) -> Element<'_, Message> {
    let tag = account
        .tag_line
        .as_deref()
        .map(|tag_line| format!("#{tag_line}"));
    let (session, session_color) = if account.has_launcher_session() {
        ("Session captured", theme::MUTED)
    } else {
        ("Login not captured", theme::GOLD)
    };

    let mut name = row![
        text(&account.display_name)
            .size(13)
            .font(theme::SEMIBOLD_FONT)
            .color(theme::TEXT)
            .wrapping(Wrapping::None)
    ]
    .spacing(5);
    if let Some(tag) = tag {
        name = name.push(
            text(tag)
                .size(13)
                .color(theme::FAINT)
                .wrapping(Wrapping::None),
        );
    }

    let mut content = row![
        account_avatar(&account.display_name, 28.0, 7.0),
        column![name, text(session).size(11).color(session_color)]
            .spacing(1)
            .width(Length::Fill)
            .clip(true)
    ]
    .spacing(10)
    .align_y(alignment::Vertical::Center);
    if is_selected {
        content = content.push(theme::icon(theme::Icon::Check, 15.0, theme::ACCENT));
    }

    button(content)
        .padding([7, 10])
        .width(Length::Fill)
        .style(move |_, status| menu_item_style(status, is_selected))
        .on_press_maybe((!is_selected).then_some(Message::SelectAccount(account.id)))
        .into()
}

fn menu_action(
    icon: theme::Icon,
    label: &'static str,
    message: Message,
) -> Element<'static, Message> {
    button(
        row![
            theme::icon(icon, 15.0, theme::MUTED),
            text(label).size(13).color(theme::TEXT)
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center),
    )
    .padding([8, 10])
    .width(Length::Fill)
    .style(|_, status| menu_item_style(status, false))
    .on_press(message)
    .into()
}

/// A square with the account's initials, standing in for its player card.
fn account_avatar(display_name: &str, size: f32, radius: f32) -> Element<'static, Message> {
    let initials: String = display_name
        .chars()
        .filter(|character| !character.is_whitespace())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();

    container(
        text(initials)
            .size(size * 0.36)
            .font(theme::SEMIBOLD_FONT)
            .color(theme::TEXT),
    )
    .center_x(size)
    .center_y(size)
    .style(move |_| filled(theme::LINE).border(iced::border::rounded(radius)))
    .into()
}

/// The Riot tag and rank under an account's name, falling back to its shard.
fn account_detail_label(account: &AccountProfile) -> String {
    let tag = account
        .tag_line
        .as_deref()
        .map(|tag_line| format!("#{tag_line}"));
    let rank = account
        .competitive_rank
        .as_ref()
        .map(|rank| rank.rank_name.clone());

    match (tag, rank) {
        (Some(tag), Some(rank)) => format!("{tag} · {rank}"),
        (Some(tag), None) => tag,
        (None, Some(rank)) => rank,
        (None, None) => account.shard.to_string(),
    }
}

fn filled(color: Color) -> iced::widget::container::Style {
    iced::widget::container::Style::default().background(color)
}

fn rule(color: Color) -> Element<'static, Message> {
    container(space())
        .width(1)
        .height(Length::Fill)
        .style(move |_| filled(color))
        .into()
}

fn popover_style(_: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(theme::RAISED.into()),
        text_color: Some(theme::TEXT),
        border: iced::Border {
            color: POPOVER_BORDER,
            width: 1.0,
            radius: 10.0.into(),
        },
        shadow: iced::Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.6),
            offset: iced::Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
        ..Default::default()
    }
}

fn menu_item_style(
    status: iced::widget::button::Status,
    is_selected: bool,
) -> iced::widget::button::Style {
    let highlighted = is_selected
        || matches!(
            status,
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
        );

    iced::widget::button::Style {
        background: highlighted.then_some(theme::LINE.into()),
        text_color: theme::TEXT,
        border: iced::border::rounded(7),
        ..Default::default()
    }
}

fn account_badge_button_style(
    theme: &Theme,
    status: iced::widget::button::Status,
    is_open: bool,
) -> iced::widget::button::Style {
    let status = if is_open {
        iced::widget::button::Status::Hovered
    } else {
        status
    };

    let mut style = theme::button_style(theme, status);
    style.border.radius = 10.0.into();
    style
}

fn add_account_prompt_overlay(
    valorant_running: bool,
    discards_pending_account: bool,
) -> Element<'static, Message> {
    let mut details = column![
        text("Add Riot account").size(20),
        text(
            "Prime will close Riot Client and VALORANT, clear any stale remembered launcher data, and open the Riot login screen."
        )
        .size(14),
        text(
            "On the Riot login screen, tick \"Stay signed in\" before you sign in. After Riot Client remembers the login, Prime will capture the launcher session and ask you to confirm the profile details."
        )
        .size(14)
    ]
    .spacing(8)
    .width(Length::Fill);

    if discards_pending_account {
        details = details.push(
            text("The captured account waiting for confirmation will be discarded.").size(14),
        );
    }

    if valorant_running {
        details = details.push(running_game_warning());
    }

    let prompt = container(
        column![
            details,
            row![
                space().width(Length::Fill),
                button("Cancel").on_press(Message::CancelAddAccountCapture),
                button("Continue").on_press(Message::ConfirmAddAccountCapture)
            ]
            .spacing(10)
        ]
        .spacing(18),
    )
    .padding(24)
    .width(720)
    .style(add_account_prompt_style);

    opaque(
        container(prompt)
            .padding(14)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
            .style(add_account_prompt_scrim_style),
    )
}

fn add_account_prompt_style(theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::bordered_box(theme)
}

fn add_account_prompt_scrim_style(_: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(Color::from_rgba8(8, 10, 14, 0.68).into()),
        ..Default::default()
    }
}

fn import_account_prompt_overlay(app: &PrimeApp) -> Element<'_, Message> {
    let import_ready =
        !app.import_account_in_progress && !app.import_account_input.trim().is_empty();
    let mut import_input =
        text_input("Paste account export", &app.import_account_input).width(Length::Fill);

    if !app.import_account_in_progress {
        import_input = import_input.on_input(Message::ImportAccountInputChanged);

        if import_ready {
            import_input = import_input.on_submit(Message::ConfirmImportAccount);
        }
    }

    let prompt = container(
        column![
            column![
                text("Import account").size(20),
                text("Paste an account export from another Prime install.").size(14)
            ]
            .spacing(8)
            .width(Length::Fill),
            import_input,
            row![
                space().width(Length::Fill),
                button("Cancel").on_press_maybe(
                    (!app.import_account_in_progress).then_some(Message::CancelImportAccount)
                ),
                button(if app.import_account_in_progress {
                    "Importing..."
                } else {
                    "Import"
                })
                .on_press_maybe(import_ready.then_some(Message::ConfirmImportAccount))
            ]
            .spacing(10)
        ]
        .spacing(18),
    )
    .padding(24)
    .width(720)
    .style(add_account_prompt_style);

    opaque(
        container(prompt)
            .padding(14)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
            .style(add_account_prompt_scrim_style),
    )
}

fn export_account_prompt_overlay(export: &AccountExportOutput) -> Element<'_, Message> {
    let token_row = row![
        text_input("Account export", &export.masked_payload)
            .width(Length::Fill)
            .size(13),
        button("Copy").on_press(Message::CopyAccountExport)
    ]
    .spacing(10)
    .align_y(alignment::Vertical::Center);

    let prompt = container(
        column![
            column![
                text(format!("Export {}", export.display_name)).size(20),
                text(
                    "Only paste this into your own Prime install. Copying keeps it out of Windows clipboard history."
                )
                .size(14),
                text(format!(
                    "Warning: anyone with this export can sign in to {} without its password.",
                    export.display_name
                ))
                .size(14)
                .width(Length::Fill)
                .color(theme::ACCENT)
            ]
            .spacing(8)
            .width(Length::Fill),
            token_row,
            row![space().width(Length::Fill), button("Close").on_press(Message::CloseAccountExport)]
                .spacing(10)
        ]
        .spacing(18),
    )
    .padding(24)
    .width(760)
    .style(add_account_prompt_style);

    opaque(
        container(prompt)
            .padding(14)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
            .style(add_account_prompt_scrim_style),
    )
}

fn delete_account_prompt_overlay(account: &AccountProfile) -> Element<'_, Message> {
    let prompt = container(
        column![
            column![
                text(format!("Delete {}?", account.display_name)).size(20),
                text("This removes the local profile and captured launcher session data.").size(14)
            ]
            .spacing(8)
            .width(Length::Fill),
            row![
                space().width(Length::Fill),
                button("Cancel").on_press(Message::CancelDeleteAccount),
                button("Delete")
                    .style(iced::widget::button::danger)
                    .on_press(Message::ConfirmDeleteAccount(account.id))
            ]
            .spacing(10)
        ]
        .spacing(18),
    )
    .padding(24)
    .width(560)
    .style(add_account_prompt_style);

    opaque(
        container(prompt)
            .padding(14)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
            .style(add_account_prompt_scrim_style),
    )
}

fn settings_change_prompt_overlay<'a>(
    app: &'a PrimeApp,
    pending: &'a PendingSettingsChange,
) -> Option<Element<'a, Message>> {
    let account = app
        .state
        .accounts
        .iter()
        .find(|account| account.id == pending.change.account_id())?;
    let name = &account.display_name;

    let (title, mut details, action) = match &pending.change {
        SettingsChange::Apply { profile_id, .. } => {
            let profile = app
                .settings_profiles
                .iter()
                .find(|profile| &profile.id == profile_id)?;
            let keeps_earlier_original = screens::original_settings(app)
                .iter()
                .any(|original| original.source_account_id == account.id);
            let undo = if keeps_earlier_original {
                format!(
                    "{name}'s own settings from before the first preset are still saved, so \
                     Restore will bring those back."
                )
            } else {
                format!("Prime saves {name}'s own settings first, so you can restore them later.")
            };

            (
                format!("Apply {} to {name}?", profile.name),
                format!(
                    "This replaces all of {name}'s VALORANT settings, including audio, with the \
                     preset's. {undo}"
                ),
                "Apply preset",
            )
        }
        SettingsChange::Restore(_) => (
            format!("Restore {name}'s own settings?"),
            format!(
                "This puts back all the VALORANT settings {name} had before a preset was applied."
            ),
            "Restore",
        ),
    };

    let action = match &pending.warning {
        Some(warning) => {
            details.push_str(&format!("\n\n{warning}"));
            format!("{action} anyway")
        }
        None => action.to_string(),
    };
    let note = pending
        .check_failed
        .then(|| format!("Prime couldn't check whether {name} is in VALORANT."));

    Some(confirmation_prompt_overlay(
        title,
        details,
        note,
        Message::CancelSettingsChange,
        button(text(action)).on_press(Message::ConfirmSettingsChange),
    ))
}

fn delete_settings_profile_prompt_overlay(
    profile: &GameSettingsProfileMetadata,
) -> Element<'_, Message> {
    let (title, details, action) = match profile.purpose {
        GameSettingsProfilePurpose::Profile => (
            format!("Delete {}?", profile.name),
            "This removes the preset from this PC. It doesn't change any account's VALORANT \
             settings.",
            "Delete",
        ),
        GameSettingsProfilePurpose::Backup => (
            format!("Discard {}'s own settings?", profile.source_display_name),
            "The account keeps the settings it has now, and you won't be able to restore the \
             ones it had before.",
            "Discard",
        ),
    };

    confirmation_prompt_overlay(
        title,
        details.to_string(),
        None,
        Message::CancelDeleteSettingsProfile,
        button(action)
            .style(iced::widget::button::danger)
            .on_press(Message::ConfirmDeleteSettingsProfile),
    )
}

fn preset_name_prompt_overlay<'a>(
    app: &'a PrimeApp,
    prompt: &'a PresetNamePrompt,
) -> Element<'a, Message> {
    let (title, details, action) = match &prompt.target {
        PresetNameTarget::New(account_id) => {
            let account = app
                .state
                .accounts
                .iter()
                .find(|account| account.id == *account_id)
                .map_or("this account", |account| account.display_name.as_str());
            (
                "Save preset".to_string(),
                format!("Saves {account}'s current VALORANT settings under this name."),
                "Save",
            )
        }
        PresetNameTarget::Rename(_) => (
            "Rename preset".to_string(),
            "Only the name changes.".to_string(),
            "Rename",
        ),
    };
    let ready = !prompt.name.trim().is_empty();
    let mut input = text_input("Preset name", &prompt.name)
        .on_input(Message::PresetNameChanged)
        .width(Length::Fill);
    if ready {
        input = input.on_submit(Message::ConfirmPresetName);
    }

    let dialog = container(
        column![
            column![text(title).size(20), text(details).size(14)]
                .spacing(8)
                .width(Length::Fill),
            input,
            row![
                space().width(Length::Fill),
                button("Cancel").on_press(Message::CancelPresetName),
                button(action).on_press_maybe(ready.then_some(Message::ConfirmPresetName))
            ]
            .spacing(10)
        ]
        .spacing(18),
    )
    .padding(24)
    .width(560)
    .style(add_account_prompt_style);

    opaque(
        container(dialog)
            .padding(14)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
            .style(add_account_prompt_scrim_style),
    )
}

/// A confirmation dialog; `note` is a muted line under the details.
fn confirmation_prompt_overlay<'a>(
    title: String,
    details: String,
    note: Option<String>,
    cancel: Message,
    confirm: iced::widget::Button<'a, Message>,
) -> Element<'a, Message> {
    let prompt = container(
        column![
            column![text(title).size(20), text(details).size(14)]
                .push(note.map(|note| text(note).size(14).color(theme::MUTED)))
                .spacing(8)
                .width(Length::Fill),
            row![
                space().width(Length::Fill),
                button("Cancel").on_press(cancel),
                confirm
            ]
            .spacing(10)
        ]
        .spacing(18),
    )
    .padding(24)
    .width(560)
    .style(add_account_prompt_style);

    opaque(
        container(prompt)
            .padding(14)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
            .style(add_account_prompt_scrim_style),
    )
}

fn recapture_prompt_overlay(
    account: &AccountProfile,
    valorant_running: bool,
) -> Element<'_, Message> {
    let mut details = column![
        text(format!("Re-capture login for {}?", account.display_name)).size(20),
        text(
            "Prime will close Riot Client and VALORANT, clear the current remembered login, and open the Riot login screen."
        )
        .size(14),
        text(format!(
            "Tick \"Stay signed in\" and sign in to {}. If you sign in to a different Riot account, the saved login is left unchanged.",
            account.summary()
        ))
        .size(14)
    ]
    .spacing(8)
    .width(Length::Fill);

    if valorant_running {
        details = details.push(running_game_warning());
    }

    let prompt = container(
        column![
            details,
            row![
                space().width(Length::Fill),
                button("Cancel").on_press(Message::CancelLauncherSessionLogin),
                button("Continue").on_press(Message::StartLauncherSessionLogin(account.id))
            ]
            .spacing(10)
        ]
        .spacing(18),
    )
    .padding(24)
    .width(640)
    .style(add_account_prompt_style);

    opaque(
        container(prompt)
            .padding(14)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
            .style(add_account_prompt_scrim_style),
    )
}

fn running_game_warning() -> Element<'static, Message> {
    text("VALORANT is running. Continuing will close it, including any match in progress.")
        .size(14)
        .width(Length::Fill)
        .color(theme::ACCENT)
        .into()
}

fn unavailable_launch_prompt_overlay(warning: &UnavailableLaunchWarning) -> Element<'_, Message> {
    let prompt = container(
        column![
            column![
                text(format!("Launch {} anyway?", warning.display_name)).size(20),
                text(&warning.reason).size(14).width(Length::Fill)
            ]
            .spacing(8)
            .width(Length::Fill),
            row![
                space().width(Length::Fill),
                button("Cancel").on_press(Message::CancelUnavailableLaunch),
                button("Launch anyway").on_press(Message::LaunchAnyway(warning.account_id))
            ]
            .spacing(10)
        ]
        .spacing(18),
    )
    .padding(24)
    .width(620)
    .style(add_account_prompt_style);

    opaque(
        container(prompt)
            .padding(14)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
            .style(add_account_prompt_scrim_style),
    )
}

fn app_update_prompt_overlay<'a>(
    update: &'a crate::updater::AvailableUpdate,
    blocking_work: Option<&'static str>,
) -> Element<'a, Message> {
    let mut details = column![
        text(format!("Prime {} is available", update.latest_version)).size(20),
        text(format!(
            "You are running Prime {}. Would you like to update to {}?",
            update.current_version, update.latest_version
        ))
        .size(14)
    ]
    .spacing(8)
    .width(Length::Fill);

    if let Some(changelog) = update.changelog.as_deref() {
        details = details.push(app_update_changelog(changelog));
    }

    if let Some(work) = blocking_work {
        details = details.push(
            text(format!(
                "Prime restarts to install the update. Wait for {work} first."
            ))
            .size(14),
        );
    }

    let prompt = container(
        column![
            details,
            row![
                space().width(Length::Fill),
                button("Later").on_press(Message::DismissAppUpdate),
                button("Download and restart").on_press_maybe(
                    blocking_work
                        .is_none()
                        .then_some(Message::DownloadAppUpdate)
                )
            ]
            .spacing(10)
        ]
        .spacing(18),
    )
    .padding(24)
    .width(720)
    .style(add_account_prompt_style);

    opaque(
        container(prompt)
            .padding(14)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
            .style(add_account_prompt_scrim_style),
    )
}

fn app_update_changelog(changelog: &str) -> Element<'_, Message> {
    let changelog_text = text(changelog)
        .size(13)
        .width(Length::Fill)
        .wrapping(iced::widget::text::Wrapping::WordOrGlyph);

    let changelog_scroll = scrollable(
        container(changelog_text)
            .padding(Padding::ZERO.right(12))
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Shrink);

    column![
        text("Changelog").size(15),
        container(changelog_scroll)
            .width(Length::Fill)
            .max_height(UPDATE_CHANGELOG_MAX_HEIGHT)
            .clip(true)
    ]
    .spacing(6)
    .width(Length::Fill)
    .into()
}

fn image_viewer_overlay(
    image_to_view: &ImageViewerImage,
    loading_frame: usize,
) -> Element<'_, Message> {
    let status: Element<_> = if image_to_view.high_res_loading {
        row![
            loading_indicator(loading_frame),
            text("Loading full image").size(13)
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center)
        .into()
    } else if let Some(error) = &image_to_view.high_res_error {
        text(error).size(13).color(theme::ACCENT).into()
    } else {
        text("").into()
    };

    let header = row![
        text(&image_to_view.title).size(18).width(Length::Fill),
        status,
        button(text("x").size(18))
            .padding([6, 12])
            .on_press(Message::CloseImageViewer)
    ]
    .spacing(12)
    .align_y(alignment::Vertical::Center);

    let viewer = image::viewer(Handle::from_path(image_to_view.path.clone()))
        .width(Length::Fill)
        .height(Length::Fill)
        .content_fit(ContentFit::Contain)
        .min_scale(0.5)
        .max_scale(12.0)
        .scale_step(0.12);

    let prompt = container(
        column![header, viewer]
            .spacing(12)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .padding(14)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(image_viewer_panel_style);

    opaque(
        container(prompt)
            .padding(28)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(add_account_prompt_scrim_style),
    )
}

fn image_viewer_panel_style(theme: &Theme) -> iced::widget::container::Style {
    let mut style = iced::widget::container::bordered_box(theme);
    style.background = Some(Color::from_rgba8(10, 12, 16, 0.96).into());
    style
}
