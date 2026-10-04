mod accounts;
mod game_settings;
mod live_match;
mod loadout;
mod settings;
mod shop;

use iced::Element;

use super::theme::text;
use super::{Message, PrimeApp, Tab};

pub(super) use accounts::header_actions as accounts_header_actions;
#[cfg(test)]
pub(super) use accounts::missing_rank_label;
pub(super) use accounts::{captured_on_label, save_settings_on_add_checkbox};
pub(super) use game_settings::original_settings;
#[cfg(test)]
pub(super) use game_settings::selected_preset;
pub(super) use live_match::hidden_details_toggle as live_match_hidden_details_toggle;
pub(super) use loadout::sub_tabs as loadout_sub_tabs;
pub(super) use settings::scroll_to_settings_section;
pub(super) use shop::bundle_details;

pub(super) fn tab(app: &PrimeApp, tab: Tab) -> Element<'_, Message> {
    match tab {
        Tab::Accounts => accounts::tab(app),
        Tab::Shop => shop::tab(app),
        Tab::Loadout => loadout::tab(app),
        Tab::Settings => settings::tab(app),
        Tab::LiveMatch => live_match::tab(app),
    }
}

/// Whether the tab shows a state that fills the page, centred, instead of scrolling content.
pub(super) fn fills_page(app: &PrimeApp, tab: Tab) -> bool {
    match tab {
        Tab::Shop => shop::fills_page(app),
        Tab::Loadout => loadout::fills_page(app),
        Tab::Accounts => accounts::fills_page(app),
        Tab::Settings => false,
        Tab::LiveMatch => live_match::fills_page(app),
    }
}

/// A tab's menu that stays beside its scrolling page; only Settings has one.
pub(super) fn side_nav(app: &PrimeApp, tab: Tab) -> Option<Element<'_, Message>> {
    (tab == Tab::Settings).then(|| settings::section_nav(app))
}

/// What Shop or Loadout shows instead of content it can't load yet: no account to load, or the
/// Riot client version still missing.
fn account_view_waiting(app: &PrimeApp, what: &str) -> Option<Element<'static, Message>> {
    let line = if app.state.accounts.is_empty() {
        format!("Add an account on the Accounts tab to see its {what}.")
    } else if app.state.selected_account().is_none() {
        format!("Select an account to see its {what}.")
    } else if app.client_version_input.trim().is_empty() {
        "Waiting for the Riot client version. Prime keeps retrying; you can also refresh it in Settings."
            .to_string()
    } else {
        return None;
    };

    Some(text(line).into())
}
