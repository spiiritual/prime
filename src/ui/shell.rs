use iced::widget::{button, column, container, opaque, row, scrollable, space, stack, text};
use iced::{Color, Element, Length, Padding, Theme, alignment};

use super::components::{currency_balance_display, loading_indicator};
use super::{Message, PrimeApp, Tab, screens};
use super::{loading_indicator_active, status_bar_visible};

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

        if self.show_add_account_prompt {
            stack![content, add_account_prompt_overlay()]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            content.into()
        }
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let mut accounts = column![text("Prime").size(26), text("Profiles").size(16)].spacing(8);

        if self.state.accounts.is_empty() {
            accounts = accounts.push(text("No profiles yet"));
        }

        for account in &self.state.accounts {
            let prefix = if self.state.selected_account == Some(account.id) {
                ">"
            } else {
                " "
            };
            let session = match (account.has_api_session(), account.has_launcher_session()) {
                (true, true) => "api + launcher",
                (true, false) => "api",
                (false, true) => "launcher",
                (false, false) => "no session",
            };
            let label = format!("{prefix} {} [{session}]", account.summary());
            accounts = accounts.push(
                button(text(label))
                    .width(Length::Fill)
                    .on_press(Message::SelectAccount(account.id)),
            );
        }

        let tabs = column![
            text("Navigate").size(16),
            self.tab_button(Tab::Accounts),
            self.tab_button(Tab::Shop),
            self.tab_button(Tab::Loadout),
            self.tab_button(Tab::Settings),
        ]
        .spacing(8);

        container(scrollable(column![accounts, tabs].spacing(16)))
            .padding(16)
            .width(280)
            .height(Length::Fill)
            .style(iced::widget::container::dark)
            .into()
    }

    fn main_panel(&self) -> Element<'_, Message> {
        let body = screens::tab(self, self.active_tab);
        let scroll_body = container(body)
            .padding(Padding::ZERO.right(18))
            .width(Length::Fill);

        let mut panel = column![
            self.main_header(),
            container(scrollable(scroll_body))
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
        let label = if self.active_tab == tab {
            format!("[{}]", tab)
        } else {
            tab.to_string()
        };

        button(text(label))
            .width(Length::Fill)
            .on_press(Message::TabSelected(tab))
            .into()
    }
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
