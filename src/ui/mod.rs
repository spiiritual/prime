mod app;
mod components;
mod data;
mod screens;
mod shell;
#[cfg(test)]
mod tests;

use std::time::Duration;

use iced::{Size, Subscription, Theme, window};

use crate::account::{AccountId, Shard};
use crate::image_cache::ImageCache;
use crate::storage::{AccountRepository, StoredState};

use data::{
    AccountRanksResult, CapturedAccountDraft, LoadoutResult, LoadoutSummary,
    RefreshedProfileIdentity, StoreSummary, StorefrontResult,
};

const LOADING_TICK_INTERVAL: Duration = Duration::from_millis(120);

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
    let mut subscriptions = Vec::new();

    if app.store_summary.is_some() {
        subscriptions
            .push(iced::time::every(data::SHOP_RESET_CHECK_INTERVAL).map(Message::ShopTimerTick));
    }

    if loading_indicator_active(app) {
        subscriptions.push(iced::time::every(LOADING_TICK_INTERVAL).map(|_| Message::LoadingTick));
    }

    Subscription::batch(subscriptions)
}

fn loading_indicator_active(app: &PrimeApp) -> bool {
    app.store_loading
        || app.loadout_loading
        || app.account_ranks_loading
        || app.launching_account.is_some()
        || loading_status_active(&app.status)
}

fn loading_status_active(status: &str) -> bool {
    status.starts_with("Loading ")
        || status.starts_with("Refreshing ")
        || status.starts_with("Opening Riot Client")
        || status.starts_with("Clearing ")
        || status.starts_with("Launching ")
}

fn status_bar_visible(status: &str) -> bool {
    status_message_is_error(status)
}

fn status_message_is_error(status: &str) -> bool {
    const ERROR_PREFIXES: &[&str] = &[
        "Failed ",
        "Could not ",
        "Launch failed",
        "Profile refresh failed",
        "Store check failed",
        "Loadout check failed",
        "Rank refresh failed",
        "Captured account rejected",
        "Captured identity rejected",
        "Profile identity rejected",
        "Launcher session rejected",
        "No captured account",
        "Select an account before",
        "Account profile no longer exists",
        "display name cannot be empty",
        "unknown Valorant shard",
    ];

    ERROR_PREFIXES
        .iter()
        .any(|prefix| status.starts_with(prefix))
        || status.contains(" failed")
        || status.contains(" rejected")
        || status.contains(" no longer exists")
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
    show_add_account_prompt: bool,
    confirm_delete_account: Option<AccountId>,
    pending_account: Option<CapturedAccountDraft>,
    store_summary: Option<StoreSummary>,
    loadout_summary: Option<LoadoutSummary>,
    store_loading: bool,
    loadout_loading: bool,
    account_ranks_loading: bool,
    launching_account: Option<AccountId>,
    image_cache_size_bytes: u64,
    loading_frame: usize,
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
    ConfirmAddAccountCapture,
    CancelAddAccountCapture,
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
    AccountRanksLoaded(AccountRanksResult),
    StorefrontLoaded(Result<StorefrontResult, String>),
    ShopTimerTick(iced::time::Instant),
    LoadingTick,
    LoadoutLoaded(Result<LoadoutResult, String>),
    RiotClientPathChanged(String),
    SaveSettings,
    ImageCacheSizeLoaded(Result<u64, String>),
    ClearImageCache,
    ImageCacheCleared(Result<(), String>),
    LaunchAccount(AccountId),
    LaunchFinished(Result<(), String>),
}
