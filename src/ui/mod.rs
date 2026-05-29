mod app;
mod components;
mod data;
mod screens;
mod shell;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::time::Duration;

use iced::widget::operation::AbsoluteOffset;
use iced::{Size, Subscription, Theme, window};

use crate::account::{AccountId, Shard};
use crate::image_cache::ImageCache;
use crate::launch::LaunchTargetProcess;
use crate::storage::{AccountRepository, StoredState};
use crate::updater::AvailableUpdate;

use data::{
    AccountRanksResult, CapturedAccountDraft, LoadoutResult, LoadoutSummary,
    RefreshedProfileIdentity, StoreSummary, StorefrontResult,
};

const LOADING_TICK_INTERVAL: Duration = Duration::from_millis(120);
const LAUNCH_PROGRESS_CHECK_INTERVAL: Duration = Duration::from_secs(1);
const MAIN_PANEL_SCROLLABLE_ID: &str = "main-panel-scrollable";

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

    if app.store_summary.is_some()
        || app
            .loadout_summary
            .as_ref()
            .is_some_and(LoadoutSummary::battle_pass_timer_active)
    {
        subscriptions
            .push(iced::time::every(data::SHOP_RESET_CHECK_INTERVAL).map(Message::ShopTimerTick));
    }

    if loading_indicator_active(app) {
        subscriptions.push(iced::time::every(LOADING_TICK_INTERVAL).map(|_| Message::LoadingTick));
    }

    if app.launching_account.is_some() {
        subscriptions.push(
            iced::time::every(LAUNCH_PROGRESS_CHECK_INTERVAL).map(|_| Message::LaunchProgressTick),
        );
    }

    Subscription::batch(subscriptions)
}

fn loading_indicator_active(app: &PrimeApp) -> bool {
    app.store_loading
        || app.loadout_loading
        || app.account_ranks_loading
        || app.launcher_capture_in_progress
        || app.launching_account.is_some()
        || app.app_update_status.is_busy()
        || loading_status_active(&app.status)
}

fn loading_status_active(status: &str) -> bool {
    status.starts_with("Loading ")
        || status.starts_with("Refreshing ")
        || status.starts_with("Opening Riot Client")
        || status.starts_with("Clearing ")
        || status.starts_with("Launching ")
        || status.starts_with("Exporting ")
        || status.starts_with("Importing ")
        || status.starts_with("Checking for Prime updates")
        || status.starts_with("Downloading Prime ")
        || status.starts_with("Preparing to restart")
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
        "Update check failed",
        "Update failed",
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
    image_viewer: Option<ImageViewerImage>,
    state: StoredState,
    active_tab: Tab,
    active_loadout_tab: LoadoutTab,
    tab_scroll_offsets: TabScrollOffsets,
    new_display_name: String,
    new_username: String,
    new_shard: Shard,
    redirect_input: String,
    client_version_input: String,
    riot_client_path_input: String,
    status: String,
    account_switcher_open: bool,
    open_account_menu: Option<AccountId>,
    show_add_account_prompt: bool,
    show_import_account_prompt: bool,
    import_account_input: String,
    import_account_in_progress: bool,
    exported_account: Option<AccountExportOutput>,
    confirm_delete_account: Option<AccountId>,
    pending_account: Option<CapturedAccountDraft>,
    store_summary: Option<StoreSummary>,
    loadout_summary: Option<LoadoutSummary>,
    store_loading: bool,
    loadout_loading: bool,
    store_loading_account: Option<AccountId>,
    loadout_loading_account: Option<AccountId>,
    account_ranks_loading: bool,
    launcher_capture_in_progress: bool,
    launching_account: Option<AccountId>,
    launch_progress_checking: bool,
    app_update_status: AppUpdateStatus,
    image_cache_size_bytes: u64,
    loading_frame: usize,
    now: iced::time::Instant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ImageViewerImage {
    path: PathBuf,
    title: String,
}

impl ImageViewerImage {
    fn new(path: PathBuf, title: impl Into<String>) -> Self {
        let title = title.into();

        Self {
            path,
            title: if title.trim().is_empty() {
                "Image".to_string()
            } else {
                title
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AccountExportOutput {
    account_id: AccountId,
    display_name: String,
    payload: String,
    masked_payload: String,
}

impl AccountExportOutput {
    fn new(account_id: AccountId, display_name: String, payload: String) -> Self {
        let masked_payload = masked_account_export_payload(&payload);

        Self {
            account_id,
            display_name,
            payload,
            masked_payload,
        }
    }
}

fn masked_account_export_payload(payload: &str) -> String {
    const VISIBLE_CHARS: usize = 18;
    const MASK_CHARS: usize = 24;

    let payload = payload.trim();
    let char_count = payload.chars().count();

    if char_count <= VISIBLE_CHARS * 2 {
        return "*".repeat(char_count);
    }

    let prefix = payload.chars().take(VISIBLE_CHARS).collect::<String>();
    let suffix = payload
        .chars()
        .skip(char_count - VISIBLE_CHARS)
        .collect::<String>();

    format!("{prefix}...{}...{suffix}", "*".repeat(MASK_CHARS))
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AppUpdateStatus {
    Checking,
    UpToDate,
    Available(AvailableUpdate),
    Dismissed(AvailableUpdate),
    Downloading(AvailableUpdate),
    Installing,
    Failed(String),
}

impl AppUpdateStatus {
    fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Downloading(_) | Self::Installing
        )
    }

    fn prompt_update(&self) -> Option<&AvailableUpdate> {
        match self {
            Self::Available(update) => Some(update),
            _ => None,
        }
    }

    fn pending_update(&self) -> Option<&AvailableUpdate> {
        match self {
            Self::Available(update) | Self::Dismissed(update) => Some(update),
            _ => None,
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Checking => "Checking for Prime updates".to_string(),
            Self::UpToDate => {
                format!("Prime is up to date ({})", crate::updater::CURRENT_VERSION)
            }
            Self::Available(update) | Self::Dismissed(update) => format!(
                "Prime {} is available (installed: {})",
                update.latest_version, update.current_version
            ),
            Self::Downloading(update) => format!("Downloading Prime {}", update.latest_version),
            Self::Installing => "Preparing to restart and install the update".to_string(),
            Self::Failed(error) => format!("Update check failed: {error}"),
        }
    }
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

#[derive(Clone, Copy, Debug, Default)]
struct TabScrollOffsets {
    accounts: AbsoluteOffset,
    shop: AbsoluteOffset,
    loadout: AbsoluteOffset,
    settings: AbsoluteOffset,
}

impl TabScrollOffsets {
    fn get(self, tab: Tab) -> AbsoluteOffset {
        match tab {
            Tab::Accounts => self.accounts,
            Tab::Shop => self.shop,
            Tab::Loadout => self.loadout,
            Tab::Settings => self.settings,
        }
    }

    fn set(&mut self, tab: Tab, offset: AbsoluteOffset) {
        match tab {
            Tab::Accounts => self.accounts = offset,
            Tab::Shop => self.shop = offset,
            Tab::Loadout => self.loadout = offset,
            Tab::Settings => self.settings = offset,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoadoutTab {
    Skins,
    BattlePass,
}

impl std::fmt::Display for LoadoutTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadoutTab::Skins => f.write_str("Skins"),
            LoadoutTab::BattlePass => f.write_str("Battle Pass"),
        }
    }
}

#[derive(Clone, Debug)]
enum Message {
    Loaded(Result<StoredState, String>),
    Saved(Result<(), String>),
    TabSelected(Tab),
    LoadoutTabSelected(LoadoutTab),
    MainPanelScrolled {
        tab: Tab,
        offset: AbsoluteOffset,
    },
    ToggleAccountSwitcher,
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
    RequestExportAccount(AccountId),
    AccountExportPrepared(Result<AccountExportOutput, String>),
    CopyAccountExport,
    CloseAccountExport,
    OpenImportAccount,
    ImportAccountInputChanged(String),
    CancelImportAccount,
    ConfirmImportAccount,
    AccountImported(Result<crate::account_transfer::ImportedAccount, String>),
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
    StorefrontLoaded(AccountId, Result<StorefrontResult, String>),
    ShopTimerTick(iced::time::Instant),
    LoadingTick,
    LoadoutLoaded(AccountId, Result<LoadoutResult, String>),
    OpenImageViewer(ImageViewerImage),
    CloseImageViewer,
    RiotClientPathChanged(String),
    SaveSettings,
    ImageCacheSizeLoaded(Result<u64, String>),
    ClearImageCache,
    ImageCacheCleared(Result<(), String>),
    LaunchAccount(AccountId),
    LaunchProgressTick,
    LaunchProgressChecked(Result<bool, String>),
    LaunchFinished(Result<LaunchTargetProcess, String>),
    CheckForAppUpdate,
    AppUpdateChecked {
        user_requested: bool,
        result: Result<Option<AvailableUpdate>, String>,
    },
    DismissAppUpdate,
    DownloadAppUpdate,
    AppUpdatePrepared(Result<(), String>),
}
