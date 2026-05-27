use iced::widget::{button, column, row, text, text_input};
use iced::{Element, Length};

use super::super::data::format_bytes;
use super::super::{Message, PrimeApp};

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    column![
        text(format!("Profile storage: {}", app.repo.path().display())),
        text_input(
            r"C:\Riot Games\Riot Client\RiotClientServices.exe",
            &app.riot_client_path_input
        )
        .on_input(Message::RiotClientPathChanged),
        button("Save settings").on_press(Message::SaveSettings),
        text(format!(
            "Image cache: {}",
            format_bytes(app.image_cache_size_bytes)
        )),
        text(format!(
            "Image cache folder: {}",
            app.image_cache.path().display()
        )),
        button("Delete image cache").on_press(Message::ClearImageCache),
        app_update_controls(app),
        token_import_controls(app)
    ]
    .spacing(12)
    .into()
}

fn app_update_controls(app: &PrimeApp) -> Element<'_, Message> {
    let mut check_button = button("Check for updates");

    if !app.app_update_status.is_busy() {
        check_button = check_button.on_press(Message::CheckForAppUpdate);
    }

    let mut controls = row![check_button].spacing(10);

    if app.app_update_status.pending_update().is_some() {
        controls = controls.push(button("Download update").on_press(Message::DownloadAppUpdate));
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
    column![
        text("Advanced API token import"),
        text_input(
            "Paste https://playvalorant.com/opt_in#access_token=...",
            &app.redirect_input
        )
        .on_input(Message::RedirectChanged),
        row![
            text_input(
                "Client version, for example release-10.00-shipping-...",
                &app.client_version_input
            )
            .on_input(Message::ClientVersionChanged)
            .width(Length::Fill),
            button("Refresh version").on_press(Message::RefreshClientVersion),
            button("Import token").on_press(Message::ImportRedirect)
        ]
        .spacing(10)
    ]
    .spacing(12)
    .into()
}
