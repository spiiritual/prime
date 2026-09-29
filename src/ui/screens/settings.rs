use iced::widget::{checkbox, column, row, text, text_input};
use iced::{Element, Length, alignment};

use crate::ui::data::format_bytes;
use crate::ui::theme::{self, button};
use crate::ui::{Message, PrimeApp};

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    column![
        text(format!("Profile storage: {}", app.repo.path().display())),
        riot_client_path_controls(app),
        minimize_on_close_controls(app),
        client_version_controls(app),
        text(format!(
            "Image cache: {}",
            format_bytes(app.image_cache_size_bytes)
        )),
        text(format!(
            "Image cache folder: {}",
            app.image_cache.path().display()
        )),
        button("Delete image cache")
            .on_press_maybe((!app.image_cache_clearing).then_some(Message::ClearImageCache)),
        app_update_controls(app),
        token_import_controls(app)
    ]
    .spacing(12)
    .into()
}

fn riot_client_path_controls(app: &PrimeApp) -> Element<'_, Message> {
    let mut save = row![button("Save settings").on_press(Message::SaveSettings)]
        .spacing(10)
        .align_y(alignment::Vertical::Center);

    if app.riot_client_path_unsaved() {
        save = save.push(text("Unsaved changes").size(13));
    }

    column![
        text("Riot Client path"),
        text("Leave empty to find RiotClientServices.exe automatically.").size(13),
        text_input(
            r"C:\Riot Games\Riot Client\RiotClientServices.exe",
            &app.riot_client_path_input
        )
        .font(theme::MONO_FONT)
        .on_input(Message::RiotClientPathChanged)
        .on_submit(Message::SaveSettings),
        save
    ]
    .spacing(8)
    .into()
}

fn minimize_on_close_controls(app: &PrimeApp) -> Element<'_, Message> {
    column![
        checkbox(app.state.minimize_on_close)
            .label("Keep Prime in the system tray when closed")
            .on_toggle(Message::MinimizeOnCloseToggled),
        text(
            "Keeps Prime running so it refreshes every account's sign-in in the background. Click the tray icon to open it again, or right-click it to quit."
        )
        .size(13)
    ]
    .spacing(8)
    .into()
}

fn client_version_controls(app: &PrimeApp) -> Element<'_, Message> {
    column![
        text("Riot client version"),
        text("Prime fetches this when it starts. Shop and Loadout need it.").size(13),
        row![
            text_input(
                "For example release-10.00-shipping-...",
                &app.client_version_input
            )
            .on_input(Message::ClientVersionChanged)
            .width(Length::Fill),
            button("Refresh version").on_press(Message::RefreshClientVersion)
        ]
        .spacing(10)
    ]
    .spacing(8)
    .into()
}

fn app_update_controls(app: &PrimeApp) -> Element<'_, Message> {
    let mut check_button = button("Check for updates");

    if !app.app_update_status.is_busy() {
        check_button = check_button.on_press(Message::CheckForAppUpdate);
    }

    let mut controls = row![check_button].spacing(10);

    if app.app_update_status.pending_update().is_some() {
        controls = controls.push(
            button("Download update").on_press_maybe(
                app.work_blocking_update()
                    .is_none()
                    .then_some(Message::DownloadAppUpdate),
            ),
        );
    }

    column![
        text(format!(
            "Prime version: {}",
            crate::updater::CURRENT_VERSION
        )),
        text(app.app_update_status.label()),
        controls
    ]
    .spacing(8)
    .into()
}

fn token_import_controls(app: &PrimeApp) -> Element<'_, Message> {
    let target = match app.state.selected_account() {
        Some(account) => format!("Imports into {}", account.summary()),
        None => "Select an account first; the token is imported into the selected one.".to_string(),
    };

    column![
        text("Advanced API token import"),
        text(target).size(13),
        text("Riot sign-in redirect URL").size(13),
        row![
            text_input(
                "Paste https://playvalorant.com/opt_in#access_token=...",
                &app.redirect_input
            )
            .on_input(Message::RedirectChanged)
            .width(Length::Fill),
            button("Import token").on_press(Message::ImportRedirect)
        ]
        .spacing(10)
    ]
    .spacing(8)
    .into()
}
