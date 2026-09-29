use iced::widget::image::Handle;
use iced::widget::text::Wrapping;
use iced::widget::{Column, column, container, image, opaque, row, scrollable, space, stack};
use iced::{Color, ContentFit, Element, Length, Padding, Theme, alignment};
use std::path::PathBuf;

use crate::account::AccountProfile;
use crate::game_settings::{GameSettingsProfileMetadata, GameSettingsProfilePurpose};

use super::components::{
    anchored_popover, balances_unavailable, compact_loading_indicator, currency_balance_display,
    loading_indicator, wallet_skeleton,
};
use super::theme::{self, button, text};
use super::{
    AccountExportOutput, CapturedAccountDraft, ImageViewerImage, LoadoutTab, LoginCapture,
    LoginCaptureTarget, MAIN_PANEL_SCROLLABLE_ID, Message, PendingSettingsChange, PresetNamePrompt,
    PresetNameTarget, PrimeApp, SettingsChange, Tab, UnavailableLaunchWarning, screens,
};
use super::{StatusKind, status_bar_visible, status_spinner_active};

/// Without its one-pixel border line.
const SIDEBAR_WIDTH: f32 = 231.0;
const ACCOUNT_SWITCHER_WIDTH: f32 = SIDEBAR_WIDTH - 32.0;
const ACCOUNT_SWITCHER_MENU_TOP_OFFSET: f32 = 60.0;
const ACCOUNT_SWITCHER_MENU_WIDTH: f32 = 280.0;
const POPOVER_BORDER: Color = iced::color!(0x2E3542);
const MENU_ITEM_SELECTED: Color = iced::color!(0x262C38);
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
                add_account_prompt_overlay(self.capture_prompt_valorant_running)
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else if let Some(capture) = &self.login_capture {
            stack![content, login_capture_overlay(self, capture)]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if let Some(draft) = &self.pending_account {
            stack![content, captured_account_overlay(self, draft)]
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

        // Over the dialogs, so errors from a dialog's action stay readable.
        let content: Element<_> = if status_bar_visible(self) {
            stack![
                content,
                // Bottom right, lined up with the right edge of the page content.
                container(self.status_toast())
                    .padding(Padding {
                        bottom: 24.0,
                        right: 36.0,
                        ..Padding::ZERO
                    })
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(alignment::Horizontal::Right)
                    .align_y(alignment::Vertical::Bottom)
            ]
            .into()
        } else {
            content
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
            text("prime")
                .size(20)
                .font(theme::DISPLAY_FONT)
                .line_height(theme::DISPLAY_LINE_HEIGHT)
        ]
        .spacing(6)
        .padding([0, 8])
        // The design's height; the 28px logo overhangs it by a pixel each way.
        .height(26)
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
            .line_height(theme::MONO_LINE_HEIGHT)
            .color(theme::FAINT);

        let sidebar = container(
            column![
                brand,
                self.account_switcher(),
                nav,
                space().height(Length::Fill),
                version
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
            menu = menu.push(account_switcher_menu_item(
                account,
                is_selected,
                self.rank_icon(account),
            ));
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
        if screens::fills_page(self, active_tab) {
            // The same inset as the scrolling page, so both centre on the same space.
            let body = container(body).padding(Padding {
                top: 0.0,
                right: 18.0,
                bottom: 28.0,
                left: 0.0,
            });
            return column![self.main_header(), body]
                .spacing(self.header_gap())
                .into();
        }
        let scroll_body = container(body)
            .padding(Padding {
                top: 0.0,
                right: 18.0,
                // Room to scroll the last content above the status toast.
                bottom: if status_visible { 90.0 } else { 28.0 },
                left: 0.0,
            })
            .width(Length::Fill);

        let page: Element<_> = scrollable(scroll_body)
            .id(MAIN_PANEL_SCROLLABLE_ID)
            .on_scroll(move |viewport| Message::MainPanelScrolled {
                tab: active_tab,
                offset: viewport.absolute_offset(),
            })
            .height(Length::Fill)
            .into();
        let page = match screens::side_nav(self, active_tab) {
            Some(nav) => row![nav, page].spacing(40).into(),
            None => page,
        };

        column![self.main_header(), page]
            .spacing(self.header_gap())
            .into()
    }

    /// The gap under the page title, which the design varies by screen.
    fn header_gap(&self) -> f32 {
        match self.active_tab {
            Tab::Accounts => 20.0,
            Tab::Shop if screens::fills_page(self, Tab::Shop) => 20.0,
            Tab::Shop => 26.0,
            Tab::Loadout if self.active_loadout_tab == LoadoutTab::BattlePass => 26.0,
            Tab::Loadout => 20.0,
            Tab::Settings => 32.0,
        }
    }

    fn main_header(&self) -> Element<'_, Message> {
        let title = text(self.active_tab.to_string())
            .size(28)
            .font(theme::DISPLAY_FONT)
            .line_height(theme::DISPLAY_LINE_HEIGHT);

        let wallet = match &self.store_summary {
            _ if self.active_tab != Tab::Shop => None,
            Some(summary) => Some(currency_balance_display(summary)),
            None if self.store_request.is_some() => Some(wallet_skeleton()),
            None if self.store_error.is_some() => Some(balances_unavailable()),
            None => None,
        };
        let header: Element<_> = match wallet {
            Some(wallet) => row![container(title).width(Length::Fill), wallet]
                .spacing(12)
                .align_y(alignment::Vertical::Center)
                .into(),
            None => title.into(),
        };

        container(header)
            .padding(Padding::ZERO.right(18))
            .width(Length::Fill)
            .into()
    }

    fn status_toast(&self) -> Element<'_, Message> {
        let kind = self.status.kind;
        let lead = if status_spinner_active(self) {
            loading_indicator(self.loading_frame)
        } else {
            match kind {
                StatusKind::Error => theme::icon(theme::Icon::TriangleAlert, 15.0, theme::ACCENT),
                StatusKind::Success => theme::icon(theme::Icon::CircleCheck, 15.0, theme::OK),
                StatusKind::Info | StatusKind::Progress => {
                    theme::icon(theme::Icon::Info, 15.0, theme::MUTED)
                }
            }
        };

        // Errors stay until closed, so only they get a close button.
        let close = (kind == StatusKind::Error).then(|| {
            button(theme::hover_icon(
                theme::Icon::X,
                14.0,
                theme::MUTED,
                theme::TEXT,
            ))
            // A 20px target around the 14px icon.
            .padding(3)
            .style(|_, _| iced::widget::button::Style::default())
            .on_press(Message::DismissStatus)
        });

        container(
            row![lead, text(&self.status.text).size(12)]
                .push(close)
                .spacing(10)
                .align_y(alignment::Vertical::Center),
        )
        .padding([10, 14])
        .max_width(STATUS_TOAST_MAX_WIDTH)
        .style(move |theme| status_toast_style(theme, kind))
        .into()
    }

    /// The icon for the account's rank, or the Unranked one once it's known to have none.
    fn rank_icon(&self, account: &AccountProfile) -> Option<&PathBuf> {
        let tier = match &account.competitive_rank {
            Some(rank) => rank.tier,
            None if self.unranked_accounts.contains(&account.id) => 0,
            None => return None,
        };
        self.rank_icons.get(&tier)
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
                    text(tab.to_string())
                        .size(14)
                        .font(font)
                        .line_height(iced::widget::text::LineHeight::Absolute(17.0.into()))
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

fn account_switcher_menu_item<'a>(
    account: &'a AccountProfile,
    is_selected: bool,
    rank_icon: Option<&PathBuf>,
) -> Element<'a, Message> {
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
    if let Some(path) = rank_icon {
        content = content.push(image(Handle::from_path(path)).width(18).height(18));
    }
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

/// The design's toast: a lighter shadow than popovers, and a green or red border for a finished
/// action or an error.
fn status_toast_style(theme: &Theme, kind: StatusKind) -> iced::widget::container::Style {
    let mut style = popover_style(theme);
    style.shadow = iced::Shadow {
        color: Color::from_rgba8(0, 0, 0, 0.4),
        offset: iced::Vector::new(0.0, 8.0),
        blur_radius: 24.0,
    };
    let tint = match kind {
        StatusKind::Success => Some(theme::OK),
        StatusKind::Error => Some(theme::ACCENT),
        StatusKind::Info | StatusKind::Progress => None,
    };
    if let Some(tint) = tint {
        style.border.color = Color {
            a: 0x55 as f32 / 255.0,
            ..tint
        };
    }
    style
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
    let background = if is_selected {
        Some(MENU_ITEM_SELECTED.into())
    } else if matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    ) {
        Some(theme::LINE.into())
    } else {
        None
    };

    iced::widget::button::Style {
        background,
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

const DIALOG_FOOTER: Color = iced::color!(0x0F1217);
const DIALOG_SCRIM: Color = iced::color!(0x05060A, 0.8);

/// The shared dialog frame: an optional coloured kicker, a title and description, an optional
/// body and a footer of actions, right-aligned, centred over a dark scrim.
fn dialog<'a>(
    width: f32,
    kicker: Option<(String, Color)>,
    title: String,
    description: Option<String>,
    body: Option<Column<'a, Message>>,
    actions: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut top = column![].spacing(6).width(Length::Fill);
    if let Some((kicker, color)) = kicker {
        top = top.push(text(kicker).size(10).font(theme::BOLD_FONT).color(color));
    }
    top = top.push(
        text(title)
            .size(21)
            .font(theme::DISPLAY_FONT)
            .line_height(theme::DISPLAY_LINE_HEIGHT),
    );
    if let Some(description) = description {
        top = top.push(text(description).size(13).color(theme::MUTED));
    }

    let mut modal = column![container(top).padding(Padding {
        top: 24.0,
        right: 24.0,
        bottom: 16.0,
        left: 24.0,
    })];

    if let Some(body) = body {
        modal = modal.push(
            container(body.spacing(12).width(Length::Fill)).padding(Padding {
                top: 4.0,
                right: 24.0,
                bottom: 20.0,
                left: 24.0,
            }),
        );
    }

    let footer = container(
        row![space().width(Length::Fill)]
            .extend(actions)
            .spacing(8)
            .align_y(alignment::Vertical::Center),
    )
    .padding([14, 24])
    .width(Length::Fill)
    .style(|_| {
        filled(DIALOG_FOOTER).border(iced::Border {
            radius: iced::border::bottom(15),
            ..Default::default()
        })
    });
    modal = modal.push(horizontal_rule()).push(footer);

    let modal = container(modal)
        .width(width)
        .style(|_| iced::widget::container::Style {
            background: Some(theme::SURFACE.into()),
            text_color: Some(theme::TEXT),
            border: iced::Border {
                color: theme::LINE,
                width: 1.0,
                radius: 16.0.into(),
            },
            shadow: iced::Shadow {
                color: Color::from_rgba8(0, 0, 0, 0.5),
                offset: iced::Vector::new(0.0, 24.0),
                blur_radius: 60.0,
            },
            ..Default::default()
        });

    opaque(
        container(modal)
            .center(Length::Fill)
            .padding(14)
            .style(|_| filled(DIALOG_SCRIM)),
    )
}

fn horizontal_rule() -> Element<'static, Message> {
    container(space())
        .width(Length::Fill)
        .height(1)
        .style(|_| filled(theme::LINE))
        .into()
}

/// The dialog's plain secondary button, such as Cancel.
fn dialog_button(label: &str, on_press: Option<Message>) -> Element<'_, Message> {
    button(text(label).size(13).font(theme::BOLD_FONT))
        .padding([9, 16])
        .on_press_maybe(on_press)
        .into()
}

/// The dialog's main action: an icon and label on a solid fill.
fn dialog_action<'a>(
    icon: theme::Icon,
    label: &'a str,
    style: fn(&Theme, iced::widget::button::Status) -> iced::widget::button::Style,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    button(theme::icon_label(icon, label, theme::BG))
        .padding([9, 16])
        .style(style)
        .on_press_maybe(on_press)
        .into()
}

/// A tinted callout with an icon, for warnings inside a dialog.
fn dialog_note<'a>(
    icon: theme::Icon,
    color: Color,
    message: impl iced::widget::text::IntoFragment<'a>,
) -> Element<'a, Message> {
    container(
        row![
            theme::icon(icon, 14.0, color),
            text(message).size(12).width(Length::Fill)
        ]
        .spacing(10),
    )
    .padding([10, 12])
    .width(Length::Fill)
    .style(move |_| {
        filled(Color { a: 0.08, ..color }).border(iced::Border {
            color: Color { a: 0.27, ..color },
            width: 1.0,
            radius: 8.0.into(),
        })
    })
    .into()
}

fn running_game_note() -> Element<'static, Message> {
    dialog_note(
        theme::Icon::TriangleAlert,
        theme::GOLD,
        "VALORANT is running. Continuing will close it, including any match in progress.",
    )
}

/// One line of what a dialog is about to do, with its icon in a small tile.
fn dialog_point<'a>(icon: theme::Icon, label: &'a str) -> Element<'a, Message> {
    row![
        container(theme::icon(icon, 14.0, theme::MUTED))
            .center(28)
            .style(|_| filled(theme::RAISED).border(iced::border::rounded(8))),
        text(label).size(13).width(Length::Fill)
    ]
    .spacing(10)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// A labelled input with an optional hint underneath.
fn dialog_field<'a>(
    label: &'a str,
    input: impl Into<Element<'a, Message>>,
    hint: Option<&'a str>,
) -> Element<'a, Message> {
    column![
        text(label)
            .size(12)
            .font(theme::SEMIBOLD_FONT)
            .color(theme::MUTED),
        input.into()
    ]
    .push(hint.map(|hint| text(hint).size(11).color(theme::FAINT)))
    .spacing(6)
    .into()
}

/// The account a dialog acts on: avatar, name and one line of detail.
fn identity_card<'a>(
    avatar_name: &str,
    name: String,
    detail: String,
    detail_font: iced::Font,
) -> Element<'a, Message> {
    container(
        row![
            account_avatar(avatar_name, 36.0, 9.0),
            column![
                text(name).size(14).font(theme::SEMIBOLD_FONT),
                text(detail).size(12).font(detail_font).color(theme::MUTED)
            ]
            .spacing(2)
            .width(Length::Fill)
        ]
        .spacing(12)
        .align_y(alignment::Vertical::Center),
    )
    .padding(12)
    .width(Length::Fill)
    .style(|_| filled(theme::RAISED).border(iced::border::rounded(10)))
    .into()
}

/// A saved account's card: its Riot ID, then rank, level and when its login was captured.
fn account_identity_card(account: &AccountProfile) -> Element<'_, Message> {
    let mut detail = Vec::new();
    if let Some(rank) = &account.competitive_rank {
        detail.push(rank.rank_name.clone());
    }
    if let Some(level) = account.account_level.filter(|level| *level > 0) {
        detail.push(format!("Level {level}"));
    }
    detail.push(screens::captured_on_label(account));

    identity_card(
        &account.display_name,
        account
            .riot_id()
            .unwrap_or_else(|| account.display_name.clone()),
        detail.join(" · "),
        theme::BODY_FONT,
    )
}

fn add_account_prompt_overlay(valorant_running: bool) -> Element<'static, Message> {
    let mut body = column![
        dialog_point(
            theme::Icon::Power,
            "Riot Client and VALORANT will be closed"
        ),
        dialog_point(
            theme::Icon::Eraser,
            "The current remembered login is cleared (other saved accounts are untouched)"
        ),
        dialog_point(
            theme::Icon::MousePointerClick,
            "Sign in and tick \u{201c}Stay signed in\u{201d}"
        ),
    ];

    if valorant_running {
        body = body.push(running_game_note());
    }

    dialog(
        520.0,
        Some(("STEP 1 OF 3".to_string(), theme::ACCENT)),
        "Add a Riot account".to_string(),
        Some("Prime needs a clean Riot Client to capture a new login.".to_string()),
        Some(body),
        vec![
            dialog_button("Cancel", Some(Message::CancelAddAccountCapture)),
            dialog_action(
                theme::Icon::ArrowRight,
                "Continue",
                theme::primary_button_style,
                Some(Message::ConfirmAddAccountCapture),
            ),
        ],
    )
}

/// Shown while a capture waits for a sign-in in Riot Client.
fn login_capture_overlay<'a>(app: &'a PrimeApp, capture: &'a LoginCapture) -> Element<'a, Message> {
    let riot_client_open = capture.wait.is_some();
    let existing = match capture.target {
        LoginCaptureTarget::NewAccount(_) => None,
        LoginCaptureTarget::Existing { account_id, .. } => app
            .state
            .accounts
            .iter()
            .find(|account| account.id == account_id),
    };

    let close_step = if riot_client_open {
        capture_step(
            CaptureStep::Done,
            "Close Riot Client & VALORANT",
            "Done",
            app,
        )
    } else {
        capture_step(
            CaptureStep::Active,
            "Close Riot Client & VALORANT",
            "Closing and clearing the current login\u{2026}",
            app,
        )
    };
    let sign_in_state = if riot_client_open {
        CaptureStep::Active
    } else {
        CaptureStep::Pending
    };
    let sign_in_detail = if riot_client_open {
        "Waiting for remembered login\u{2026}"
    } else {
        "Riot Client opens next"
    };

    let mut steps = column![
        close_step,
        capture_step(
            sign_in_state,
            "Sign in and tick \u{201c}Stay signed in\u{201d}",
            sign_in_detail,
            app
        )
    ];
    if existing.is_none() {
        steps = steps.push(capture_step(
            CaptureStep::Pending,
            "Name the account",
            "Choose how it shows in Prime",
            app,
        ));
    }

    let (kicker, title) = match existing {
        Some(account) => (None, format!("Sign in as {}", account.display_name)),
        None => (
            Some(("STEP 2 OF 3".to_string(), theme::ACCENT)),
            "Sign in to Riot Client".to_string(),
        ),
    };

    let body = column![
        steps,
        dialog_note(
            theme::Icon::Info,
            theme::MUTED,
            "Without \u{201c}Stay signed in\u{201d}, Riot Client won't remember the login and \
             capture can't finish.",
        )
    ];

    dialog(
        520.0,
        kicker,
        title,
        Some(
            "Sign in with the account in Riot Client. Prime never sees your password.".to_string(),
        ),
        Some(body),
        vec![
            // Cancelling only works once Riot Client is open and the wait has started.
            dialog_button(
                "Cancel",
                riot_client_open.then_some(Message::CancelLoginCapture),
            ),
            container(
                row![
                    compact_loading_indicator(app.loading_frame),
                    text("Waiting for login")
                        .size(13)
                        .font(theme::BOLD_FONT)
                        .color(theme::MUTED)
                ]
                .spacing(8)
                .align_y(alignment::Vertical::Center),
            )
            .padding([9, 16])
            .style(|_| filled(theme::RAISED).border(iced::border::rounded(8)))
            .into(),
        ],
    )
}

#[derive(Clone, Copy, PartialEq)]
enum CaptureStep {
    Done,
    Active,
    Pending,
}

fn capture_step<'a>(
    state: CaptureStep,
    label: &'a str,
    detail: &'a str,
    app: &PrimeApp,
) -> Element<'a, Message> {
    let badge: Element<_> = match state {
        CaptureStep::Done => container(theme::icon(theme::Icon::Check, 13.0, theme::OK))
            .center(24)
            .style(|_| {
                filled(Color {
                    a: 0.15,
                    ..theme::OK
                })
                .border(iced::border::rounded(12))
            })
            .into(),
        CaptureStep::Active => container(compact_loading_indicator(app.loading_frame))
            .center(24)
            .style(|_| {
                filled(theme::ACCENT_SOFT).border(iced::Border {
                    color: theme::ACCENT,
                    width: 1.0,
                    radius: 12.0.into(),
                })
            })
            .into(),
        CaptureStep::Pending => container(space())
            .center(24)
            .style(|_| filled(theme::RAISED).border(iced::border::rounded(12)))
            .into(),
    };
    let (label_color, detail_color) = match state {
        CaptureStep::Done => (theme::TEXT, theme::FAINT),
        CaptureStep::Active => (theme::TEXT, theme::ACCENT),
        CaptureStep::Pending => (theme::MUTED, theme::FAINT),
    };

    row![
        badge,
        column![
            text(label)
                .size(13)
                .font(theme::SEMIBOLD_FONT)
                .color(label_color),
            text(detail).size(12).color(detail_color)
        ]
        .spacing(2)
    ]
    .spacing(12)
    .padding([10, 0])
    .align_y(alignment::Vertical::Center)
    .into()
}

/// Asks for a display name once a capture has found the account's login.
fn captured_account_overlay<'a>(
    app: &'a PrimeApp,
    draft: &'a CapturedAccountDraft,
) -> Element<'a, Message> {
    let riot_id = draft
        .riot_id()
        .unwrap_or_else(|| "Riot ID not captured".to_string());
    let avatar_name = draft.game_name.as_deref().unwrap_or(&riot_id);
    let puuid = short_puuid(&draft.puuid);

    let mut body = column![
        identity_card(
            avatar_name,
            riot_id.clone(),
            format!("PUUID {puuid} \u{b7} {} shard", draft.shard),
            theme::MONO_FONT,
        ),
        dialog_field(
            "Display name",
            theme::text_input("Display name", &app.new_display_name)
                .on_input(Message::NewDisplayNameChanged)
                .on_submit(Message::ConfirmCapturedAccount),
            Some("Shown in Prime only."),
        )
    ];
    body = body.push(screens::save_settings_on_add_checkbox(app));

    dialog(
        520.0,
        None,
        "Login captured".to_string(),
        Some(
            "Riot Client remembered the login. Give the account a name you'll recognise."
                .to_string(),
        ),
        Some(body),
        vec![
            dialog_button("Cancel", Some(Message::CancelCapturedAccount)),
            dialog_action(
                theme::Icon::Check,
                "Save account",
                theme::primary_button_style,
                (!app.new_display_name.trim().is_empty())
                    .then_some(Message::ConfirmCapturedAccount),
            ),
        ],
    )
}

/// The first and last four characters, which is enough to tell PUUIDs apart at a glance.
fn short_puuid(puuid: &str) -> String {
    let characters: Vec<char> = puuid.chars().collect();
    if characters.len() <= 10 {
        return puuid.to_string();
    }

    let start: String = characters[..4].iter().collect();
    let end: String = characters[characters.len() - 4..].iter().collect();
    format!("{start}\u{2026}{end}")
}

fn import_account_prompt_overlay(app: &PrimeApp) -> Element<'_, Message> {
    let import_ready =
        !app.import_account_in_progress && !app.import_account_input.trim().is_empty();
    let mut import_input = theme::text_input("Paste account export", &app.import_account_input)
        .font(theme::MONO_FONT)
        .line_height(theme::MONO_LINE_HEIGHT)
        .size(12)
        // The design's 96px box with the export on its first line.
        .padding(Padding {
            top: 10.0,
            right: 12.0,
            bottom: 68.0,
            left: 12.0,
        });

    if !app.import_account_in_progress {
        import_input = import_input.on_input(Message::ImportAccountInputChanged);

        if import_ready {
            import_input = import_input.on_submit(Message::ConfirmImportAccount);
        }
    }

    dialog(
        520.0,
        None,
        "Import account".to_string(),
        Some("Paste an account export from another Prime install.".to_string()),
        Some(column![dialog_field("Account export", import_input, None)]),
        vec![
            dialog_button(
                "Cancel",
                (!app.import_account_in_progress).then_some(Message::CancelImportAccount),
            ),
            dialog_action(
                theme::Icon::Download,
                if app.import_account_in_progress {
                    "Importing\u{2026}"
                } else {
                    "Import"
                },
                theme::primary_button_style,
                import_ready.then_some(Message::ConfirmImportAccount),
            ),
        ],
    )
}

fn export_account_prompt_overlay(export: &AccountExportOutput) -> Element<'_, Message> {
    let export_field = container(
        row![
            text(&export.masked_payload)
                .size(12)
                .font(theme::MONO_FONT)
                .line_height(theme::MONO_LINE_HEIGHT)
                .color(theme::MUTED)
                .width(Length::Fill)
                .wrapping(Wrapping::None),
            button(text("Copy").size(11).font(theme::SEMIBOLD_FONT))
                .padding([4, 10])
                .on_press(Message::CopyAccountExport)
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center),
    )
    .padding(Padding {
        top: 6.0,
        right: 6.0,
        bottom: 6.0,
        left: 12.0,
    })
    .clip(true)
    .style(|_| {
        filled(theme::BG).border(iced::Border {
            color: theme::LINE,
            width: 1.0,
            radius: 8.0.into(),
        })
    });

    dialog(
        520.0,
        None,
        format!("Export {}", export.display_name),
        Some(
            "Only paste this into your own Prime install. Copying keeps it out of Windows \
             clipboard history."
                .to_string(),
        ),
        Some(column![
            export_field,
            dialog_note(
                theme::Icon::TriangleAlert,
                theme::ACCENT,
                format!(
                    "Anyone with this export can sign in to {} without its password.",
                    export.display_name
                ),
            )
        ]),
        vec![dialog_button("Close", Some(Message::CloseAccountExport))],
    )
}

fn delete_account_prompt_overlay(account: &AccountProfile) -> Element<'_, Message> {
    dialog(
        440.0,
        None,
        format!("Delete {}?", account.display_name),
        Some(
            "This removes the local profile and its captured launcher session. Your Riot account \
             itself isn't affected."
                .to_string(),
        ),
        Some(column![account_identity_card(account)]),
        vec![
            dialog_button("Cancel", Some(Message::CancelDeleteAccount)),
            dialog_action(
                theme::Icon::Trash,
                "Delete account",
                theme::danger_button_style,
                Some(Message::ConfirmDeleteAccount(account.id)),
            ),
        ],
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

    let (title, details, action) = match &pending.change {
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

    let mut body = column![];
    if let Some(warning) = &pending.warning {
        body = body.push(dialog_note(
            theme::Icon::TriangleAlert,
            theme::GOLD,
            warning.as_str(),
        ));
    }
    if pending.check_failed {
        body = body.push(dialog_note(
            theme::Icon::Info,
            theme::MUTED,
            format!("Prime couldn't check whether {name} is in VALORANT."),
        ));
    }
    let action = if pending.warning.is_some() {
        format!("{action} anyway")
    } else {
        action.to_string()
    };

    Some(dialog(
        480.0,
        None,
        title,
        Some(details),
        (pending.warning.is_some() || pending.check_failed).then_some(body),
        vec![
            dialog_button("Cancel", Some(Message::CancelSettingsChange)),
            button(text(action).size(13).font(theme::BOLD_FONT))
                .padding([9, 16])
                .style(theme::primary_button_style)
                .on_press(Message::ConfirmSettingsChange)
                .into(),
        ],
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

    dialog(
        440.0,
        None,
        title,
        Some(details.to_string()),
        None,
        vec![
            dialog_button("Cancel", Some(Message::CancelDeleteSettingsProfile)),
            dialog_action(
                theme::Icon::Trash,
                action,
                theme::danger_button_style,
                Some(Message::ConfirmDeleteSettingsProfile),
            ),
        ],
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
    let mut input =
        theme::text_input("Preset name", &prompt.name).on_input(Message::PresetNameChanged);
    if ready {
        input = input.on_submit(Message::ConfirmPresetName);
    }

    dialog(
        480.0,
        None,
        title,
        Some(details),
        Some(column![dialog_field("Preset name", input, None)]),
        vec![
            dialog_button("Cancel", Some(Message::CancelPresetName)),
            dialog_action(
                theme::Icon::Check,
                action,
                theme::primary_button_style,
                ready.then_some(Message::ConfirmPresetName),
            ),
        ],
    )
}

fn recapture_prompt_overlay(
    account: &AccountProfile,
    valorant_running: bool,
) -> Element<'_, Message> {
    let mut body = column![account_identity_card(account)];
    if valorant_running {
        body = body.push(running_game_note());
    }

    dialog(
        460.0,
        None,
        format!("Re-capture login for {}?", account.display_name),
        Some(format!(
            "Prime will close Riot Client, clear its current login and wait for you to sign in \
             as {} again. Tick \u{201c}Stay signed in\u{201d}. If you sign in to a different \
             Riot account, the saved login is left unchanged.",
            account.display_name
        )),
        Some(body),
        vec![
            dialog_button("Cancel", Some(Message::CancelLauncherSessionLogin)),
            dialog_action(
                theme::Icon::ArrowRight,
                "Continue",
                theme::primary_button_style,
                Some(Message::StartLauncherSessionLogin(account.id)),
            ),
        ],
    )
}

fn unavailable_launch_prompt_overlay(warning: &UnavailableLaunchWarning) -> Element<'_, Message> {
    dialog(
        480.0,
        None,
        format!("Launch {} anyway?", warning.display_name),
        Some("Switching accounts restarts Riot Client and VALORANT.".to_string()),
        Some(column![dialog_note(
            theme::Icon::Swords,
            theme::ACCENT,
            warning.reason.as_str(),
        )]),
        vec![
            dialog_button("Cancel", Some(Message::CancelUnavailableLaunch)),
            dialog_action(
                theme::Icon::Play,
                "Launch anyway",
                theme::danger_button_style,
                Some(Message::LaunchAnyway(warning.account_id)),
            ),
        ],
    )
}

fn app_update_prompt_overlay<'a>(
    update: &'a crate::updater::AvailableUpdate,
    blocking_work: Option<&'static str>,
) -> Element<'a, Message> {
    let mut body = column![];
    if let Some(changelog) = update.changelog.as_deref() {
        body = body.push(app_update_changelog(changelog));
    }
    if let Some(work) = blocking_work {
        body = body.push(dialog_note(
            theme::Icon::Info,
            theme::GOLD,
            format!("Prime restarts to install the update. Wait for {work} first."),
        ));
    }

    dialog(
        500.0,
        Some(("UPDATE".to_string(), theme::OK)),
        format!("Prime {} is available", update.latest_version),
        Some(format!("You're running {}.", update.current_version)),
        (update.changelog.is_some() || blocking_work.is_some()).then_some(body),
        vec![
            dialog_button("Later", Some(Message::DismissAppUpdate)),
            dialog_action(
                theme::Icon::Download,
                "Download & restart",
                theme::success_button_style,
                blocking_work
                    .is_none()
                    .then_some(Message::DownloadAppUpdate),
            ),
        ],
    )
}

fn app_update_changelog(changelog: &str) -> Element<'_, Message> {
    let changelog_scroll = scrollable(
        container(
            text(changelog)
                .size(13)
                .color(theme::MUTED)
                .width(Length::Fill)
                .wrapping(Wrapping::WordOrGlyph),
        )
        .padding(Padding::ZERO.right(12))
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Shrink);

    container(
        column![
            text("CHANGELOG")
                .size(10)
                .font(theme::BOLD_FONT)
                .color(theme::FAINT),
            container(changelog_scroll)
                .width(Length::Fill)
                .max_height(UPDATE_CHANGELOG_MAX_HEIGHT)
                .clip(true)
        ]
        .spacing(10),
    )
    .padding(14)
    .width(Length::Fill)
    .style(|_| {
        filled(theme::BG).border(iced::Border {
            color: theme::LINE,
            width: 1.0,
            radius: 10.0.into(),
        })
    })
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
        text(&image_to_view.title)
            .size(18)
            .font(theme::DISPLAY_FONT)
            .line_height(theme::DISPLAY_LINE_HEIGHT)
            .width(Length::Fill),
        status,
        button(theme::icon(theme::Icon::X, 16.0, theme::TEXT))
            .padding(8)
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
    .style(|_| {
        filled(theme::SURFACE).border(iced::Border {
            color: theme::LINE,
            width: 1.0,
            radius: 16.0.into(),
        })
    });

    opaque(
        container(prompt)
            .padding(28)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| filled(DIALOG_SCRIM)),
    )
}
