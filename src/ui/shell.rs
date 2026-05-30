use iced::widget::image::Handle;
use iced::widget::{
    button, column, container, image, opaque, row, scrollable, space, stack, text, text_input,
};
use iced::{Color, ContentFit, Element, Length, Padding, Theme, alignment};

use crate::account::AccountProfile;

use super::components::{anchored_popover, currency_balance_display, loading_indicator};
use super::{
    AccountExportOutput, ImageViewerImage, MAIN_PANEL_SCROLLABLE_ID, Message, PrimeApp, Tab,
    UnavailableLaunchWarning, screens,
};
use super::{loading_indicator_active, status_bar_visible};

const SIDEBAR_WIDTH: f32 = 210.0;
const SIDEBAR_PADDING: u16 = 16;
const ACCOUNT_SWITCHER_MENU_TOP_OFFSET: f32 = 62.0;
const ACCOUNT_SWITCHER_MENU_WIDTH: f32 = SIDEBAR_WIDTH - (SIDEBAR_PADDING as f32 * 2.0);
const UPDATE_CHANGELOG_MAX_HEIGHT: f32 = 260.0;

impl PrimeApp {
    pub(super) fn view(&self) -> Element<'_, Message> {
        let content = row![
            self.sidebar(),
            container(self.main_panel())
                .padding(22)
                .width(Length::Fill)
                .height(Length::Fill)
        ]
        .height(Length::Fill);

        let content = container(content)
            .padding(Padding::ZERO.right(14))
            .width(Length::Fill)
            .height(Length::Fill);

        let pending_delete_account = self.confirm_delete_account.and_then(|account_id| {
            self.state
                .accounts
                .iter()
                .find(|account| account.id == account_id)
        });

        let content: Element<_> = if self.show_add_account_prompt {
            stack![content, add_account_prompt_overlay()]
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
        } else if let Some(warning) = &self.unavailable_launch_warning {
            stack![content, unavailable_launch_prompt_overlay(warning)]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if let Some(update) = self.app_update_status.prompt_update() {
            stack![content, app_update_prompt_overlay(update)]
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
        let accounts = column![text("Prime").size(26), self.account_switcher()].spacing(8);

        let tabs = column![
            text("Navigate").size(16),
            self.tab_button(Tab::Accounts),
            self.tab_button(Tab::Shop),
            self.tab_button(Tab::Loadout),
            self.tab_button(Tab::Settings),
        ]
        .spacing(8);

        container(scrollable(column![accounts, tabs].spacing(16)))
            .padding(SIDEBAR_PADDING)
            .width(SIDEBAR_WIDTH)
            .height(Length::Fill)
            .style(iced::widget::container::dark)
            .into()
    }

    fn account_switcher(&self) -> Element<'_, Message> {
        anchored_popover(
            self.account_badge(),
            self.account_switcher_menu(),
            self.account_switcher_open,
            ACCOUNT_SWITCHER_MENU_TOP_OFFSET,
            0.0,
        )
    }

    fn account_badge(&self) -> Element<'_, Message> {
        let account = self.state.selected_account();
        let is_open = self.account_switcher_open;
        let display_name = account
            .map(|account| account.display_name.clone())
            .unwrap_or_else(|| "No profile".to_string());
        let detail = account
            .map(account_detail_label)
            .unwrap_or_else(|| "Add or select an account".to_string());

        let content = container(
            row![
                column![
                    text(display_name).size(15).width(Length::Fill),
                    text(detail).size(12).width(Length::Fill)
                ]
                .spacing(2)
                .width(Length::Fill),
                text(if self.account_switcher_open { "^" } else { "v" }).size(13)
            ]
            .spacing(8)
            .align_y(alignment::Vertical::Center),
        )
        .padding([9, 10])
        .width(Length::Fill);

        button(content)
            .padding(0)
            .width(Length::Fill)
            .style(move |theme, status| account_badge_button_style(theme, status, is_open))
            .on_press_maybe(
                (!self.state.accounts.is_empty()).then_some(Message::ToggleAccountSwitcher),
            )
            .into()
    }

    fn account_switcher_menu(&self) -> Element<'_, Message> {
        let mut accounts = column![].spacing(6).width(Length::Fill);

        if self.state.accounts.is_empty() {
            accounts = accounts.push(text("No profiles yet").size(13));
        }

        for account in &self.state.accounts {
            let is_selected = self.state.selected_account == Some(account.id);
            accounts = accounts.push(account_switcher_menu_item(account, is_selected));
        }

        container(accounts)
            .padding(8)
            .width(ACCOUNT_SWITCHER_MENU_WIDTH)
            .style(iced::widget::container::bordered_box)
            .into()
    }

    fn main_panel(&self) -> Element<'_, Message> {
        let active_tab = self.active_tab;
        let body = screens::tab(self, self.active_tab);
        let scroll_body = container(body)
            .padding(Padding::ZERO.right(18))
            .width(Length::Fill);

        let mut panel = column![
            self.main_header(),
            container(
                scrollable(scroll_body)
                    .id(MAIN_PANEL_SCROLLABLE_ID)
                    .on_scroll(move |viewport| Message::MainPanelScrolled {
                        tab: active_tab,
                        offset: viewport.absolute_offset(),
                    })
            )
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(iced::widget::container::rounded_box)
        ]
        .spacing(12);

        if status_bar_visible(&self.status) {
            panel = panel.push(self.status_bar());
        }

        panel.into()
    }

    fn main_header(&self) -> Element<'_, Message> {
        let title = text(self.active_tab.to_string()).size(30);

        let header: Element<_> = if self.active_tab == Tab::Shop {
            row![
                container(title).width(Length::Fill),
                self.shop_header_currency()
            ]
            .spacing(12)
            .align_y(alignment::Vertical::Center)
            .into()
        } else {
            title.into()
        };

        container(header)
            .padding(14)
            .width(Length::Fill)
            .style(iced::widget::container::bordered_box)
            .into()
    }

    fn shop_header_currency(&self) -> Element<'_, Message> {
        if let Some(summary) = &self.store_summary {
            currency_balance_display(summary)
        } else {
            text("").into()
        }
    }

    fn status_bar(&self) -> Element<'_, Message> {
        let status: Element<_> = if loading_indicator_active(self) {
            row![
                loading_indicator(self.loading_frame),
                text(&self.status).width(Length::Fill)
            ]
            .spacing(10)
            .align_y(alignment::Vertical::Center)
            .into()
        } else {
            text(&self.status).into()
        };

        container(status)
            .padding(10)
            .width(Length::Fill)
            .style(iced::widget::container::bordered_box)
            .into()
    }

    fn tab_button(&self, tab: Tab) -> Element<'_, Message> {
        let is_selected = self.active_tab == tab;
        let label = if is_selected {
            format!("[{}]", tab)
        } else {
            tab.to_string()
        };

        button(text(label))
            .width(Length::Fill)
            .style(move |theme, status| sidebar_tab_button_style(theme, status, is_selected))
            .on_press_maybe((!is_selected).then_some(Message::TabSelected(tab)))
            .into()
    }
}

fn account_switcher_menu_item(account: &AccountProfile, is_selected: bool) -> Element<'_, Message> {
    let prefix = if is_selected { "> " } else { "" };
    let display_name = format!("{prefix}{}", account.display_name);

    let content = column![
        text(display_name).size(14).width(Length::Fill),
        text(account_detail_label(account))
            .size(12)
            .width(Length::Fill)
    ]
    .spacing(1)
    .width(Length::Fill);

    button(content)
        .padding([7, 8])
        .width(Length::Fill)
        .style(move |theme, status| account_switcher_item_style(theme, status, is_selected))
        .on_press_maybe((!is_selected).then_some(Message::SelectAccount(account.id)))
        .into()
}

fn account_detail_label(account: &AccountProfile) -> String {
    account
        .riot_id()
        .or_else(|| account.username.clone())
        .map(|identity| format!("{identity} | {}", account.shard))
        .unwrap_or_else(|| account.shard.to_string())
}

fn account_badge_button_style(
    theme: &Theme,
    status: iced::widget::button::Status,
    is_open: bool,
) -> iced::widget::button::Style {
    if is_open {
        iced::widget::button::secondary(theme, status)
    } else {
        iced::widget::button::primary(theme, status)
    }
}

fn sidebar_tab_button_style(
    theme: &Theme,
    status: iced::widget::button::Status,
    is_selected: bool,
) -> iced::widget::button::Style {
    if !is_selected {
        return iced::widget::button::primary(theme, status);
    }

    let mut style = iced::widget::button::secondary(theme, iced::widget::button::Status::Disabled);
    style.background = Some(Color::from_rgb8(68, 72, 78).into());
    style.text_color = Color::from_rgb8(180, 184, 190);
    style
}

fn account_switcher_item_style(
    theme: &Theme,
    status: iced::widget::button::Status,
    is_selected: bool,
) -> iced::widget::button::Style {
    if !is_selected {
        return iced::widget::button::primary(theme, status);
    }

    let mut style = iced::widget::button::secondary(theme, iced::widget::button::Status::Disabled);
    style.background = Some(Color::from_rgb8(68, 72, 78).into());
    style.text_color = Color::from_rgb8(180, 184, 190);
    style
}

fn add_account_prompt_overlay() -> Element<'static, Message> {
    let prompt = container(
        column![
            column![
                text("Add Riot account").size(20),
                text(
                    "Prime will close Riot Client, clear any stale remembered launcher data, and open the Riot login screen."
                )
                .size(14),
                text(
                    "On the Riot login screen, tick \"Stay signed in\" before you sign in. After Riot Client remembers the login, Prime will capture the launcher session and ask you to confirm the profile details."
                )
                .size(14)
            ]
            .spacing(8)
            .width(Length::Fill),
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
                text("Keep this export private; it includes captured Riot Client session data.")
                    .size(14),
                text(
                    "Warning: this is like sharing the account password and grants full access to the account."
                )
                .size(14)
                .width(Length::Fill)
                .color(Color::from_rgb8(255, 112, 112))
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

fn unavailable_launch_prompt_overlay(warning: &UnavailableLaunchWarning) -> Element<'_, Message> {
    let prompt = container(
        column![
            column![
                text(format!("Launch {} anyway?", warning.display_name)).size(20),
                text(format!(
                    "This account appears unavailable ({}). Launching may interrupt that active VALORANT session.",
                    warning.reason
                ))
                .size(14)
                .width(Length::Fill)
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

fn app_update_prompt_overlay(update: &crate::updater::AvailableUpdate) -> Element<'_, Message> {
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

    let prompt = container(
        column![
            details,
            row![
                space().width(Length::Fill),
                button("Later").on_press(Message::DismissAppUpdate),
                button("Download and restart").on_press(Message::DownloadAppUpdate)
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
        text(error)
            .size(13)
            .color(Color::from_rgb8(255, 112, 112))
            .into()
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
