mod app;
mod components;
mod data;
mod screens;
mod shell;
#[cfg(test)]
mod tests;

use iced::{Size, Subscription, Theme, window};

use crate::account::{AccountId, Shard};
use crate::image_cache::ImageCache;
use crate::storage::{AccountRepository, StoredState};

use data::{
    CapturedAccountDraft, LoadoutResult, LoadoutSummary, RefreshedProfileIdentity, StoreSummary,
    StorefrontResult,
};

pub fn run() -> iced::Result {
    iced::application(PrimeApp::boot, PrimeApp::update, PrimeApp::view)
        .title(app_title)
        .theme(app_theme)
        .subscription(app_subscription)
        .window(window::Settings {
            size: Size::new(1280.0, 840.0),
            min_size: Some(Size::new(1280.0, 840.0)),
            max_size: Some(Size::new(1280.0, 840.0)),
            resizable: false,
            ..window::Settings::default()
        })
        .run()
}

fn app_title(_: &PrimeApp) -> String {
    "prime".to_string()
}

fn app_theme(_: &PrimeApp) -> Theme {
    Theme::Dark
}

fn app_subscription(app: &PrimeApp) -> Subscription<Message> {
    if app.store_summary.is_some() {
        iced::time::every(data::SHOP_RESET_CHECK_INTERVAL).map(Message::ShopTimerTick)
    } else {
        Subscription::none()
    }
}

#[derive(Clone, Debug)]
struct PrimeApp {
    repo: AccountRepository,
    image_cache: ImageCache,
    state: StoredState,
    active_tab: Tab,
    new_display_name: String,
    new_username: String,
    new_shard: Shard,
    redirect_input: String,
    client_version_input: String,
    riot_client_path_input: String,
    status: String,
    open_account_menu: Option<AccountId>,
    confirm_delete_account: Option<AccountId>,
    pending_account: Option<CapturedAccountDraft>,
    store_summary: Option<StoreSummary>,
    loadout_summary: Option<LoadoutSummary>,
    store_loading: bool,
    loadout_loading: bool,
    image_cache_size_bytes: u64,
    now: iced::time::Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Tab {
    Accounts,
    Shop,
    Loadout,
    Settings,
}

impl std::fmt::Display for Tab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tab::Accounts => f.write_str("Accounts"),
            Tab::Shop => f.write_str("Shop"),
            Tab::Loadout => f.write_str("Loadout"),
            Tab::Settings => f.write_str("Settings"),
        }
    }
}

#[derive(Clone, Debug)]
enum Message {
    Loaded(Result<StoredState, String>),
    Saved(Result<(), String>),
    TabSelected(Tab),
    SelectAccount(AccountId),
    NewDisplayNameChanged(String),
    NewUsernameChanged(String),
    NewShardSelected(Shard),
    AddAccount,
    AccountCaptureFinished(Result<CapturedAccountDraft, String>),
    ConfirmCapturedAccount,
    CancelCapturedAccount,
    ToggleAccountMenu(AccountId),
    RequestDeleteAccount(AccountId),
    CancelDeleteAccount,
    ConfirmDeleteAccount(AccountId),
    RedirectChanged(String),
    ClientVersionChanged(String),
    RefreshClientVersion,
    ClientVersionLoaded(Result<String, String>),
    ImportRedirect,
    StartLauncherSessionLogin(AccountId),
    LauncherSessionLoginStarted(
        Result<crate::riot::launcher_session::CapturedLauncherSession, String>,
    ),
    RefreshProfileIdentity(AccountId),
    ProfileIdentityLoaded(Result<RefreshedProfileIdentity, String>),
    StorefrontLoaded(Result<StorefrontResult, String>),
    ShopTimerTick(iced::time::Instant),
    LoadoutLoaded(Result<LoadoutResult, String>),
    RiotClientPathChanged(String),
    SaveSettings,
    ImageCacheSizeLoaded(Result<u64, String>),
    ClearImageCache,
    ImageCacheCleared(Result<(), String>),
    LaunchAccount(AccountId),
    LaunchFinished(Result<(), String>),
}
