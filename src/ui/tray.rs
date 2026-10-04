//! The notification-area icon Prime shows while closing its window has hidden it.

use std::cell::RefCell;

use iced::Subscription;
use iced::futures::{SinkExt, Stream};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

const OPEN_ID: &str = "open";
const QUIT_ID: &str = "quit";
/// The app icon `build.rs` embeds in the executable.
const APP_ICON_RESOURCE: u16 = 1;

thread_local! {
    // A tray icon belongs to the thread running the window's message loop, which runs `update`.
    static TRAY: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TrayAction {
    Open,
    Quit,
}

pub(super) fn show() -> Result<(), String> {
    TRAY.with_borrow_mut(|tray| {
        if tray.is_none() {
            *tray = Some(build().map_err(|error| format!("Could not add the tray icon: {error}"))?);
        }
        Ok(())
    })
}

/// Removes the icon; also needed before quitting, or Windows leaves a dead icon behind.
pub(super) fn remove() {
    TRAY.with_borrow_mut(|tray| *tray = None);
}

fn build() -> Result<TrayIcon, Box<dyn std::error::Error>> {
    let menu = Menu::with_items(&[
        &MenuItem::with_id(OPEN_ID, "Open Prime", true, None),
        &MenuItem::with_id(QUIT_ID, "Quit", true, None),
    ])?;

    Ok(TrayIconBuilder::new()
        .with_icon(tray_icon::Icon::from_resource(APP_ICON_RESOURCE, None)?)
        .with_tooltip("prime")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()?)
}

/// Left clicks on the icon and picks from its menu.
pub(super) fn actions() -> Subscription<TrayAction> {
    Subscription::run(action_stream)
}

fn action_stream() -> impl Stream<Item = TrayAction> {
    iced::stream::channel(4, async |mut output| {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let click_sender = sender.clone();
        // Starting Prime again while it runs opens this window, the same as the tray's Open.
        let relaunch_sender = sender.clone();
        std::thread::spawn(move || {
            while crate::single_instance::wait_for_show_request() {
                if relaunch_sender.send(TrayAction::Open).is_err() {
                    break;
                }
            }
        });
        TrayIconEvent::set_event_handler(Some(move |event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = click_sender.send(TrayAction::Open);
            }
        }));
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let action = match event.id.as_ref() {
                OPEN_ID => TrayAction::Open,
                QUIT_ID => TrayAction::Quit,
                _ => return,
            };
            let _ = sender.send(action);
        }));

        while let Some(action) = receiver.recv().await {
            if output.send(action).await.is_err() {
                break;
            }
        }
    })
}
