mod accounts;
mod aim_trainer;
mod game_settings;
mod live_match;
mod loadout;
mod settings;
mod settings_pages;
mod shop;

use iced::Element;
use iced::widget::row;

use crate::storage::SETTINGS_PAGES_EXPERIMENT;

use super::components::{empty_state, spinner, unavailable_state};
use super::theme::{self, Icon, button, text};
use super::{Message, PrimeApp, Tab};

pub(super) use accounts::header_actions as accounts_header_actions;
#[cfg(test)]
pub(super) use accounts::missing_rank_label;
pub(super) use accounts::{captured_on_label, save_settings_on_add_checkbox};
pub(super) use aim_trainer::fullscreen as aim_trainer_fullscreen;
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
        Tab::AimTrainer => aim_trainer::tab(app),
        Tab::Settings if app.experiment(SETTINGS_PAGES_EXPERIMENT) => settings_pages::tab(app),
        Tab::Settings => settings::tab(app),
        Tab::LiveMatch => live_match::tab(app),
    }
}

/// Whether the tab fills the page, without scrolling: a centred state, or a page sized to the window.
pub(super) fn fills_page(app: &PrimeApp, tab: Tab) -> bool {
    match tab {
        Tab::Shop => {
            shop::fills_page(app)
                || (app.store_summary.is_none()
                    && app.store_request.is_none()
                    && account_view_blocked(app))
        }
        Tab::Loadout => {
            loadout::fills_page(app)
                || (app.loadout_summary.is_none()
                    && app.loadout_request.is_none()
                    && account_view_blocked(app))
        }
        Tab::Accounts => accounts::fills_page(app),
        Tab::AimTrainer => true,
        Tab::Settings => false,
        Tab::LiveMatch => live_match::fills_page(app),
    }
}

/// A tab's menu that stays beside its scrolling page; only Settings has one.
pub(super) fn side_nav(app: &PrimeApp, tab: Tab) -> Option<Element<'_, Message>> {
    (tab == Tab::Settings).then(|| {
        if app.experiment(SETTINGS_PAGES_EXPERIMENT) {
            settings_pages::section_nav(app)
        } else {
            settings::section_nav(app)
        }
    })
}

/// What Shop or Loadout shows instead of content it can't load yet: no account to load, or the
/// Riot client version still missing.
fn account_view_waiting<'a>(app: &PrimeApp, what: &str) -> Option<Element<'a, Message>> {
    if !app.refreshed() {
        return account_view_waiting_line(app, what);
    }
    if app.state.accounts.is_empty() {
        return Some(empty_state(
            Icon::Users,
            "No accounts yet",
            format!("Add an account on the Accounts tab to see its {what}."),
        ));
    }
    if app.state.selected_account().is_none() {
        return Some(empty_state(
            Icon::Users,
            "No account selected",
            format!("Select an account to see its {what}."),
        ));
    }
    if !app.client_version_input.trim().is_empty() {
        return None;
    }

    // Riot only answers requests that name the current VALORANT version.
    let fetching = app.client_version_fetching;
    let retry = button(
        row![
            if fetching {
                spinner(true, app.loading_frame, 14.0, theme::MUTED)
            } else {
                theme::icon(Icon::RefreshCw, 14.0, theme::TEXT)
            },
            text(if fetching {
                "Trying\u{2026}"
            } else {
                "Try again"
            })
            .size(13)
            .font(theme::SEMIBOLD_FONT)
        ]
        .spacing(8)
        .align_y(iced::alignment::Vertical::Center),
    )
    .padding([9, 16])
    .on_press_maybe((!fetching).then_some(Message::RefreshClientVersion));
    Some(unavailable_state(
        Icon::PlugZap,
        if what == "shop" {
            "Shop can't load yet"
        } else {
            "Loadout can't load yet"
        },
        format!(
            "Prime couldn't find out VALORANT's current version, which Riot needs before it \
             sends the {what}. It tries again every 30 seconds."
        ),
        vec![retry.into()],
        None,
    ))
}

/// Stable's `account_view_waiting`, without the interface refresh: one line of text.
fn account_view_waiting_line(app: &PrimeApp, what: &str) -> Option<Element<'static, Message>> {
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

/// Shop and Loadout can't load: `account_view_waiting` shows a centred state instead, unless
/// they already have something to show.
fn account_view_blocked(app: &PrimeApp) -> bool {
    app.refreshed()
        && (app.state.selected_account().is_none() || app.client_version_input.trim().is_empty())
}
