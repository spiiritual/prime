mod accounts;
mod game_settings;
mod live_match;
mod loadout;
mod settings;
mod shop;

use iced::Element;

use super::theme::text;
use super::{Message, PrimeApp, Tab};

#[cfg(test)]
pub(super) use accounts::missing_rank_label;
#[cfg(test)]
pub(super) use game_settings::selected_preset;
pub(super) use accounts::{captured_on_label, save_settings_on_add_checkbox};
pub(super) use game_settings::original_settings;
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

/// Import, Add current and Add account, beside the Accounts title.
pub(super) fn accounts_header_actions(app: &PrimeApp) -> Element<'_, Message> {
    accounts::header_actions(app)
}

/// Loadout's Skins and Battle Pass switch, which sits beside the page title.
pub(super) fn loadout_sub_tabs(app: &PrimeApp) -> Element<'_, Message> {
    loadout::sub_tabs(app)
}

/// Whether hidden players stay hidden, beside the Live Match title.
pub(super) fn live_match_streamer_toggle(app: &PrimeApp) -> Element<'_, Message> {
    live_match::streamer_toggle(app)
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
