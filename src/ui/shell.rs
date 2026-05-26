use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Length, Padding};

use super::{Message, PrimeApp, Tab, screens};

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

        container(content)
            .padding(Padding::ZERO.right(14))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
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

        column![
            container(text(self.active_tab.to_string()).size(30))
                .padding(14)
                .width(Length::Fill)
                .style(iced::widget::container::bordered_box),
            container(scrollable(scroll_body))
                .padding(16)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(iced::widget::container::rounded_box),
            container(text(&self.status))
                .padding(10)
                .width(Length::Fill)
                .style(iced::widget::container::bordered_box)
        ]
        .spacing(12)
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
