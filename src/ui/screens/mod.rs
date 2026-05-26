mod accounts;
mod loadout;
mod settings;
mod shop;

use iced::Element;

use super::{Message, PrimeApp, Tab};

pub(super) fn tab(app: &PrimeApp, tab: Tab) -> Element<'_, Message> {
    match tab {
        Tab::Accounts => accounts::tab(app),
        Tab::Shop => shop::tab(app),
        Tab::Loadout => loadout::tab(app),
        Tab::Settings => settings::tab(app),
    }
}
