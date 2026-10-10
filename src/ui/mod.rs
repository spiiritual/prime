mod aim;
mod app;
mod components;
mod data;
mod screens;
mod shell;
#[cfg(test)]
mod tests;
mod theme;
mod tray;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use iced::widget::operation::AbsoluteOffset;
use iced::{Size, Subscription, Theme, window};

use crate::account::AccountId;
use crate::image_cache::{CacheUsage, ImageCache};
use crate::riot::chat_proxy::{ChatProxy, PresenceStatus};
use crate::storage::{AccountRepository, StoredState};
use crate::updater::{AvailableUpdate, UpdateCheckOutcome};
use aim::{AimMessage, AimTrainerTab};

use crate::game_settings::GameSettingsProfileMetadata;
use data::account_details::{
    AccountActivityCheck, AccountAvailability, AccountAvailabilityRefresh, AccountRanksResult,
    RefreshedApiContext, RefreshedProfileIdentity,
};
use data::game_settings::{
    AppliedGameSettingsResult, RestoredGameSettingsResult, SavedGameSettingsResult,
};
use data::image_assets::PlayerCardArt;
use data::launch_flow::CapturedAccountDraft;
use data::launch_flow::{
    LaunchAccountResult, LoadedAccounts, LocalGame, SHOP_RESET_CHECK_INTERVAL,
};
use data::live_match::{LiveMatch, LiveMatchError, LiveMatchResult};
use data::loadout::{LoadoutResult, LoadoutSummary};
use data::shop::{StoreSummary, StorefrontResult};

const LOADING_TICK_INTERVAL: Duration = Duration::from_millis(120);
const LAUNCH_PROGRESS_CHECK_INTERVAL: Duration = Duration::from_secs(1);
const ACCOUNT_AVAILABILITY_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const LOCAL_GAME_CHECK_INTERVAL: Duration = Duration::from_secs(5);
/// While minimized with "minimize on close" on, a slow poll keeps every account's session in use
/// so its refresh token doesn't sit idle. Access tokens last an hour, so each poll re-signs in.
const BACKGROUND_SESSION_REFRESH_INTERVAL: Duration = Duration::from_secs(30 * 60);
/// Opening the Accounts tab reloads every account's details only when they are older than this.
const ACCOUNTS_TAB_RELOAD_AFTER: Duration = Duration::from_secs(60);
const STATUS_FLASH_DURATION: Duration = Duration::from_secs(4);
/// How often the app checks whether a toast has expired. The timer bar redraws itself each frame,
/// and the app follows every frame only while the toast sinks out.
const STATUS_FLASH_TICK_INTERVAL: Duration = Duration::from_millis(500);
/// The battle pass's time left shows whole minutes, so it needs no tick every second.
const BATTLE_PASS_TIMER_INTERVAL: Duration = Duration::from_secs(10);
const CLIENT_VERSION_RETRY_INTERVAL: Duration = Duration::from_secs(30);
const MAIN_PANEL_SCROLLABLE_ID: &str = "main-panel-scrollable";

fn image_viewer_enabled() -> bool {
    cfg!(feature = "image-viewer-testing")
}

pub fn run() -> iced::Result {
    let application = theme::FONTS.into_iter().fold(
        iced::application(PrimeApp::boot, PrimeApp::update, PrimeApp::view),
        |application, font| application.font(font),
    );

    application
        .default_font(theme::BODY_FONT)
        .title(app_title)
        .theme(app_theme)
        .subscription(app_subscription)
        .window(window::Settings {
            size: Size::new(1280.0, 840.0),
            min_size: Some(Size::new(1280.0, 840.0)),
            max_size: Some(Size::new(1280.0, 840.0)),
            resizable: false,
            exit_on_close_request: false,
            icon: window_icon(),
            ..window::Settings::default()
        })
        .run()
}

/// Decoded here because iced's own loader needs its `image` feature, which builds every image
/// format, and Prime only shows PNGs.
fn window_icon() -> Option<window::Icon> {
    let icon = ::image::load_from_memory(include_bytes!("../../assets/icon.png"))
        .ok()?
        .into_rgba8();
    let (width, height) = icon.dimensions();
    window::icon::from_rgba(icon.into_raw(), width, height).ok()
}

fn app_title(_: &PrimeApp) -> String {
    "prime".to_string()
}

fn app_theme(_: &PrimeApp) -> Theme {
    static THEME: std::sync::LazyLock<Theme> = std::sync::LazyLock::new(theme::theme);
    THEME.clone()
}

fn app_subscription(app: &PrimeApp) -> Subscription<Message> {
    let mut subscriptions = vec![
        iced::window::resize_events().map(|(_, size)| Message::WindowResized(size)),
        iced::keyboard::listen().filter_map(escape_key_message),
        window::close_requests().map(Message::CloseRequested),
        tray::actions().map(Message::Tray),
    ];

    if let Some(interval) = countdown_timer_interval(app) {
        subscriptions.push(iced::time::every(interval).map(Message::ShopTimerTick));
    }

    let animating = appearing(app) && !app.window_minimized;
    if animating {
        subscriptions.push(window::frames().map(Message::AnimationFrame));
    }

    // While something settles in, its frames already keep `now` current.
    if status_flash_active(app) && !animating {
        subscriptions.push(if !app.window_minimized && status_sinking(app) {
            window::frames().map(Message::StatusTimerTick)
        } else {
            iced::time::every(STATUS_FLASH_TICK_INTERVAL).map(Message::StatusTimerTick)
        });
    }

    // Nobody sees the spinner while minimized, and a stuck progress status would keep it ticking.
    if loading_indicator_active(app) && !app.window_minimized {
        subscriptions.push(iced::time::every(LOADING_TICK_INTERVAL).map(|_| Message::LoadingTick));
    }

    // Once Riot Client shows, the button stops waiting for it.
    if app.launching_account.is_some() && !app.launch_client_open {
        subscriptions.push(
            iced::time::every(LAUNCH_PROGRESS_CHECK_INTERVAL).map(|_| Message::LaunchProgressTick),
        );
    }

    // Availability polling makes requests for every account, so it pauses while nobody can see it,
    // unless the user chose to keep Prime running in the background, where it polls slowly. Live
    // Match polls only the selected account on the same timer.
    if !app.state.accounts.is_empty() {
        let interval = if background_refresh_active(app) {
            Some(BACKGROUND_SESSION_REFRESH_INTERVAL)
        } else if matches!(app.active_tab, Tab::Accounts | Tab::LiveMatch) && !app.window_minimized
        {
            Some(ACCOUNT_AVAILABILITY_REFRESH_INTERVAL)
        } else {
            None
        };
        if let Some(interval) = interval {
            subscriptions
                .push(iced::time::every(interval).map(Message::AccountAvailabilityTimerTick));
        }
    }

    // Only a launch through the chat proxy can have its status changed, so only then is the game
    // watched. Nobody sees the control while minimized.
    if app.chat_proxy.is_some() && !app.window_minimized {
        subscriptions
            .push(iced::time::every(LOCAL_GAME_CHECK_INTERVAL).map(|_| Message::LocalGameTick));
    }

    if let Some(aim) = aim::subscription(app) {
        subscriptions.push(aim);
    }

    Subscription::batch(subscriptions)
}

fn background_refresh_active(app: &PrimeApp) -> bool {
    app.state.minimize_on_close && app.window_minimized
}

/// Whether a spinner is on screen. Loads that show skeletons or nothing, such as Shop, Loadout and
/// account details, don't count, since each tick rebuilds the whole window.
fn loading_indicator_active(app: &PrimeApp) -> bool {
    !app.profile_identity_refreshing.is_empty()
        || app.launcher_capture_in_progress
        || app.launch_preflight_account.is_some()
        || app.launching_account.is_some()
        || app.settings_saving_account.is_some()
        || app.settings_applying_account.is_some()
        || app.settings_check.is_some()
        || image_viewer_enabled()
            && app
                .image_viewer
                .as_ref()
                .is_some_and(|image| image.high_res_loading)
        || status_spinner_active(app) && status_bar_visible(app)
}

/// How often the countdown on screen ticks, or `None` when none shows. A reset reached meanwhile
/// is caught when the tab opens.
fn countdown_timer_interval(app: &PrimeApp) -> Option<Duration> {
    if app.window_minimized {
        return None;
    }
    match app.active_tab {
        Tab::Shop if app.store_summary.is_some() => Some(SHOP_RESET_CHECK_INTERVAL),
        Tab::Loadout
            if app.active_loadout_tab == LoadoutTab::BattlePass
                && app
                    .loadout_summary
                    .as_ref()
                    .is_some_and(LoadoutSummary::battle_pass_timer_active) =>
        {
            Some(BATTLE_PASS_TIMER_INTERVAL)
        }
        _ => None,
    }
}

fn escape_key_message(event: iced::keyboard::Event) -> Option<Message> {
    match event {
        iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            ..
        } => Some(Message::EscapePressed),
        _ => None,
    }
}

/// What a status message reports, which sets its toast's look and how long it stays.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum StatusKind {
    #[default]
    Info,
    /// Work that is still running; the toast shows a spinner.
    Progress,
    /// A finished action; the toast is green.
    Success,
    /// Something finished but needs attention, such as a partial load; the toast is gold.
    Warning,
    /// Stays on screen until something the user does replaces it.
    Error,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Status {
    kind: StatusKind,
    text: String,
}

impl Status {
    fn new(kind: StatusKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }

    fn info(text: impl Into<String>) -> Self {
        Self::new(StatusKind::Info, text)
    }

    fn progress(text: impl Into<String>) -> Self {
        Self::new(StatusKind::Progress, text)
    }

    fn success(text: impl Into<String>) -> Self {
        Self::new(StatusKind::Success, text)
    }

    fn warning(text: impl Into<String>) -> Self {
        Self::new(StatusKind::Warning, text)
    }

    fn error(text: impl Into<String>) -> Self {
        Self::new(StatusKind::Error, text)
    }
}

/// The status bar's spinner marks a progress message, not unrelated work in the background.
fn status_spinner_active(app: &PrimeApp) -> bool {
    app.status.kind == StatusKind::Progress
        || app.status.kind != StatusKind::Error
            && (app.launching_account.is_some() || app.launcher_capture_in_progress)
}

/// Errors stay on screen; other updates show briefly so actions still get feedback without
/// cluttering the window, and launch or login capture progress stays visible while it runs.
fn status_bar_visible(app: &PrimeApp) -> bool {
    status_visible_at(&app.status, app.status_changed_at, app.now)
        || (!app.status.text.trim().is_empty()
            && (app.launching_account.is_some() || app.launcher_capture_in_progress))
}

fn status_visible_at(
    status: &Status,
    changed_at: iced::time::Instant,
    now: iced::time::Instant,
) -> bool {
    !status.text.trim().is_empty()
        && (status.kind == StatusKind::Error
            || now.saturating_duration_since(changed_at) < STATUS_FLASH_DURATION)
}

/// How much of a closing-by-itself toast's time is left, from 1 down to 0, or `None` for a
/// toast that stays: an error, or progress that is still running.
fn status_time_left(app: &PrimeApp) -> Option<f32> {
    if !matches!(
        app.status.kind,
        StatusKind::Info | StatusKind::Success | StatusKind::Warning
    ) || status_spinner_active(app)
        || !status_flash_active(app)
    {
        return None;
    }
    let elapsed = app.now.saturating_duration_since(app.status_changed_at);
    Some((1.0 - elapsed.as_secs_f32() / STATUS_FLASH_DURATION.as_secs_f32()).clamp(0.0, 1.0))
}

/// When a toast that closes by itself starts sinking out, or `None` for a toast that stays.
fn status_sink_starts_at(app: &PrimeApp) -> Option<iced::time::Instant> {
    status_time_left(app)?;
    Some(app.status_changed_at + STATUS_FLASH_DURATION - APPEAR_DURATION)
}

fn status_sinking(app: &PrimeApp) -> bool {
    status_sink_starts_at(app).is_some_and(|starts_at| app.now >= starts_at)
}

fn status_flash_active(app: &PrimeApp) -> bool {
    app.status.kind != StatusKind::Error
        && status_visible_at(&app.status, app.status_changed_at, app.now)
}

#[derive(Clone, Debug)]
struct PrimeApp {
    repo: AccountRepository,
    image_cache: ImageCache,
    /// Cached rank icons by competitive tier, loaded once at startup.
    rank_icons: HashMap<i64, PathBuf>,
    /// Cached player card art by card ID. An entry is added when its download starts, so each card
    /// is requested once; art that failed stays empty and avatars show initials.
    player_card_art: HashMap<String, PlayerCardArt>,
    /// What the Accounts list is filtered by.
    account_filter: String,
    image_viewer: Option<ImageViewerImage>,
    /// The dialog on screen and when it opened, for its entrance.
    dialog_opened: Option<(Dialog, iced::time::Instant)>,
    /// The store ID of the featured bundle whose details are open.
    bundle_details: Option<String>,
    state: StoredState,
    /// False until accounts.json loads successfully; saving is refused until then so an empty
    /// in-memory state can never overwrite the user's saved accounts.
    accounts_loaded: bool,
    active_tab: Tab,
    active_accounts_tab: AccountsTab,
    active_loadout_tab: LoadoutTab,
    tab_scroll_offsets: TabScrollOffsets,
    /// The Aim Trainer tab's fields and run.
    aim: AimTrainerTab,
    new_display_name: String,
    redirect_input: String,
    /// The Settings section last picked from its menu.
    settings_section: SettingsSection,
    token_import_open: bool,
    client_version_input: String,
    riot_client_path_input: String,
    status: Status,
    account_switcher_open: bool,
    status_menu_open: bool,
    open_account_menu: Option<AccountId>,
    show_add_account_prompt: bool,
    show_import_account_prompt: bool,
    import_account_input: String,
    import_account_in_progress: bool,
    exported_account: Option<AccountExportOutput>,
    confirm_delete_account: Option<AccountId>,
    confirm_recapture_account: Option<AccountId>,
    /// Set while the add or re-capture prompt is open and VALORANT was found running, since
    /// continuing closes the game.
    capture_prompt_valorant_running: bool,
    pending_account: Option<CapturedAccountDraft>,
    store_summary: Option<StoreSummary>,
    loadout_summary: Option<LoadoutSummary>,
    /// The Shop load whose reply is shown; replies to other requests only cache their session.
    store_request: Option<ViewRequest>,
    loadout_request: Option<ViewRequest>,
    /// Why the selected account's last Shop or Loadout load failed, shown with Try again.
    store_error: Option<String>,
    loadout_error: Option<String>,
    /// The selected account's last live match: agent select or a match in progress.
    live_match: Option<LiveMatch>,
    live_match_request: Option<ViewRequest>,
    /// Whether a Live Match load is signing in, even one an account switch left behind.
    live_match_in_flight: bool,
    live_match_error: Option<LiveMatchError>,
    /// Whether Live Match shows the names and levels players hide. Off at every start.
    show_hidden_details: bool,
    next_request_id: u64,
    /// Accounts whose Riot profile refresh is running.
    profile_identity_refreshing: HashSet<AccountId>,
    /// Accounts whose rank, level and penalties are loading.
    account_ranks_loading: HashSet<AccountId>,
    /// Accounts whose rank loaded and turned out to be none.
    unranked_accounts: HashSet<AccountId>,
    /// Why each account's last rank lookup failed, shown on hover over "Rank unavailable".
    rank_errors: HashMap<AccountId, String>,
    /// When the last all-account details and availability loads started, so reopening the
    /// Accounts tab doesn't refetch everything each time.
    account_details_loaded_at: Option<iced::time::Instant>,
    account_availability: HashMap<AccountId, AccountAvailability>,
    account_availability_loading: bool,
    account_availability_loaded_at: Option<iced::time::Instant>,
    /// When each account's entry in `account_availability` arrived, so Apply and Restore only
    /// trust recent results.
    account_availability_checked_at: HashMap<AccountId, iced::time::Instant>,
    /// Whether a newly added account's VALORANT settings are saved as a settings profile.
    save_settings_on_add: bool,
    settings_profiles: Vec<GameSettingsProfileMetadata>,
    /// Presets whose cards list all their settings.
    expanded_presets: HashSet<String>,
    /// The preset whose Rename and Delete menu is open.
    open_preset_menu: Option<String>,
    /// The Live Match column whose weapon picker is open in Settings.
    open_weapon_picker: Option<usize>,
    /// The preset the Apply panel is for; the first one when unset or deleted.
    selected_preset: Option<String>,
    /// The name dialog for saving or renaming a preset.
    preset_name_prompt: Option<PresetNamePrompt>,
    settings_saving_account: Option<AccountId>,
    settings_applying_account: Option<AccountId>,
    /// The Apply or Restore whose game check runs before its dialog opens.
    settings_check: Option<PendingSettingsCheck>,
    confirm_settings_change: Option<PendingSettingsChange>,
    confirm_delete_settings_profile: Option<String>,
    launcher_capture_in_progress: bool,
    launcher_capture_kind: Option<LauncherCaptureKind>,
    /// The running add or re-capture that reopened Riot Client for a sign-in.
    login_capture: Option<LoginCapture>,
    launch_preflight_account: Option<AccountId>,
    unavailable_launch_warning: Option<UnavailableLaunchWarning>,
    launching_account: Option<AccountId>,
    launch_progress_checking: bool,
    /// Riot Client's window is up and the launch waits for VALORANT's.
    launch_client_open: bool,
    /// The chat proxy the last launch went through. Dropping it stops the proxy.
    chat_proxy: Option<LaunchedChatProxy>,
    /// The last check of what runs on this PC, made while a chat proxy runs.
    local_game: Option<LocalGame>,
    local_game_checking: bool,
    status_launch_failure: Option<StatusLaunchFailure>,
    /// A quit waiting on the user, because it would drop Riot Client's chat.
    confirm_quit: Option<QuitAction>,
    window_minimized: bool,
    status_changed_at: iced::time::Instant,
    /// When the toast last appeared. A toast already on screen changes text without rising again.
    toast_appeared_at: iced::time::Instant,
    /// When the last dialog closed, while its backdrop fades out.
    dialog_closed_at: Option<iced::time::Instant>,
    app_update_status: AppUpdateStatus,
    image_cache_usage: CacheUsage,
    image_cache_clearing: bool,
    loading_frame: usize,
    now: iced::time::Instant,
}

/// A change to one account's VALORANT settings.
#[derive(Clone, Debug, Eq, PartialEq)]
enum SettingsChange {
    /// Replaces the account's settings with a saved preset's.
    Apply {
        account_id: AccountId,
        profile_id: String,
    },
    /// Puts back the settings the account had before its first preset.
    Restore(AccountId),
}

impl SettingsChange {
    fn account_id(&self) -> AccountId {
        match self {
            Self::Apply { account_id, .. } | Self::Restore(account_id) => *account_id,
        }
    }
}

/// A settings change whose game check is running; its dialog opens when the check is done.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingSettingsCheck {
    /// Tells this check's result apart from one started by an earlier, canceled request.
    request_id: u64,
    change: SettingsChange,
}

/// A settings change waiting for confirmation, with a warning when the game could undo it.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingSettingsChange {
    change: SettingsChange,
    warning: Option<String>,
    /// Whether the game check failed, so the dialog notes that Prime couldn't check.
    check_failed: bool,
}

/// What the preset name dialog is for.
#[derive(Clone, Debug, Eq, PartialEq)]
enum PresetNameTarget {
    /// Saving this account's current settings as a new preset.
    New(AccountId),
    /// Renaming the saved preset with this ID.
    Rename(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PresetNamePrompt {
    target: PresetNameTarget,
    name: String,
}

/// A Shop or Loadout load for one account. Each load gets a new ID, so a reply to an earlier
/// load for the same account is not mistaken for the latest one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ViewRequest {
    id: u64,
    account_id: AccountId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum LauncherCaptureKind {
    New,
    Current,
    Existing,
}

/// A login capture that closes Riot Client, clears its live login and waits for a new sign-in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoginCaptureTarget {
    /// Adding an account; the login is captured into the new account's slot.
    NewAccount(AccountId),
    /// Re-capturing an account; the login is staged in its own slot until it is verified.
    Existing {
        account_id: AccountId,
        staging_id: AccountId,
    },
}

impl LoginCaptureTarget {
    fn kind(self) -> LauncherCaptureKind {
        match self {
            Self::NewAccount(_) => LauncherCaptureKind::New,
            Self::Existing { .. } => LauncherCaptureKind::Existing,
        }
    }

    /// The backup slot the captured login is copied into.
    fn slot_id(self) -> AccountId {
        match self {
            Self::NewAccount(account_id) => account_id,
            Self::Existing { staging_id, .. } => staging_id,
        }
    }
}

#[derive(Clone, Debug)]
struct LoginCapture {
    target: LoginCaptureTarget,
    /// Set once Riot Client is open and the wait for a sign-in has started; only then can the
    /// capture be cancelled.
    wait: Option<iced::task::Handle>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UnavailableLaunchWarning {
    account_id: AccountId,
    display_name: String,
    reason: String,
}

/// The chat proxy a launch went through, and for which account. Dropping it stops the proxy.
#[derive(Clone, Debug)]
struct LaunchedChatProxy {
    account_id: AccountId,
    proxy: ChatProxy,
}

/// A launch stopped because the chat proxy couldn't start while the saved status isn't Online.
#[derive(Clone, Debug, Eq, PartialEq)]
struct StatusLaunchFailure {
    account_id: AccountId,
    display_name: String,
    status: PresenceStatus,
    error: String,
}

/// How Prime is about to quit: closing, or restarting to install an update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QuitAction {
    Exit,
    InstallUpdate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ImageViewerRequest {
    preview_path: PathBuf,
    title: String,
    high_res: Option<ImageViewerSource>,
}

impl ImageViewerRequest {
    fn new(
        preview_path: PathBuf,
        title: impl Into<String>,
        high_res: Option<ImageViewerSource>,
    ) -> Self {
        let title = title.into();

        Self {
            preview_path,
            title: if title.trim().is_empty() {
                "Image".to_string()
            } else {
                title
            },
            high_res,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ImageViewerSource {
    namespace: String,
    id: String,
    url: String,
}

impl ImageViewerSource {
    fn new(namespace: impl Into<String>, id: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            id: id.into(),
            url: url.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ImageViewerImage {
    path: PathBuf,
    title: String,
    high_res: Option<ImageViewerSource>,
    high_res_loading: bool,
    high_res_error: Option<String>,
}

impl ImageViewerImage {
    fn from_request(request: ImageViewerRequest) -> Self {
        Self {
            path: request.preview_path,
            title: request.title,
            high_res: request.high_res,
            high_res_loading: false,
            high_res_error: None,
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

#[derive(Clone, Debug)]
enum AppUpdateStatus {
    Checking,
    UpToDate,
    Available(AvailableUpdate),
    Dismissed(AvailableUpdate),
    Downloading(AvailableUpdate),
    Installing,
    NotInstalled,
    CheckFailed(String),
    /// A download or install that failed; the update is kept so it can be tried again.
    InstallFailed {
        update: Option<AvailableUpdate>,
        error: String,
    },
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
            Self::Available(update)
            | Self::Dismissed(update)
            | Self::InstallFailed {
                update: Some(update),
                ..
            } => Some(update),
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
            Self::NotInstalled => format!(
                "Prime {} is not an installed build; updates only apply to installed copies",
                crate::updater::CURRENT_VERSION
            ),
            Self::CheckFailed(error) => format!("Update check failed: {error}"),
            Self::InstallFailed { error, .. } => format!("Update failed: {error}"),
        }
    }
}

/// The dialog on screen, one at a time, in the order the shell picks them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Dialog {
    Quit,
    AddAccount,
    LoginCapture,
    CapturedAccount,
    ImportAccount,
    ExportAccount,
    DeleteAccount,
    Recapture,
    SettingsChange,
    DeleteSettingsProfile,
    PresetName,
    UnavailableLaunch,
    StatusLaunchFailed,
    AppUpdate,
    BundleDetails,
}

/// How long a dialog or toast takes to settle in.
const APPEAR_DURATION: Duration = Duration::from_millis(180);

/// How far an appearance that started at `since` has got, from 0 to 1, easing out.
fn appear_progress(since: iced::time::Instant, now: iced::time::Instant) -> f32 {
    ease_out(now.saturating_duration_since(since).as_secs_f32() / APPEAR_DURATION.as_secs_f32())
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// How strong a closed dialog's backdrop still is as it fades out, from 1 down to 0, or `None`
/// once it has gone or while a dialog is open.
fn closing_scrim(app: &PrimeApp) -> Option<f32> {
    let left = 1.0 - appear_progress(app.dialog_closed_at?, app.now);
    (left > 0.0).then_some(left)
}

/// How far the toast is into place: it rises in, and a toast that closes by itself sinks back out
/// as its time runs out.
fn toast_progress(app: &PrimeApp) -> f32 {
    let leaving = status_time_left(app).map_or(1.0, |left| {
        ease_out(left * STATUS_FLASH_DURATION.as_secs_f32() / APPEAR_DURATION.as_secs_f32())
    });
    appear_progress(app.toast_appeared_at, app.now).min(leaving)
}

/// Whether a dialog or toast is still settling in or a backdrop fading out, so frames keep coming.
fn appearing(app: &PrimeApp) -> bool {
    let settling =
        |since: iced::time::Instant| app.now.saturating_duration_since(since) < APPEAR_DURATION;
    app.dialog_opened.is_some_and(|(_, since)| settling(since))
        || closing_scrim(app).is_some()
        || (status_bar_visible(app) && settling(app.toast_appeared_at))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Tab {
    Accounts,
    Shop,
    Loadout,
    AimTrainer,
    Settings,
    /// Opened from the sidebar's live match indicator; it has no nav item.
    LiveMatch,
}

impl std::fmt::Display for Tab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tab::Accounts => f.write_str("Accounts"),
            Tab::Shop => f.write_str("Shop"),
            Tab::Loadout => f.write_str("Loadout"),
            Tab::AimTrainer => f.write_str("Aim Trainer"),
            Tab::Settings => f.write_str("Settings"),
            Tab::LiveMatch => f.write_str("Live Match"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct TabScrollOffsets {
    accounts: AbsoluteOffset,
    shop: AbsoluteOffset,
    loadout: AbsoluteOffset,
    aim_trainer: AbsoluteOffset,
    settings: AbsoluteOffset,
    live_match: AbsoluteOffset,
}

impl TabScrollOffsets {
    fn get(self, tab: Tab) -> AbsoluteOffset {
        match tab {
            Tab::Accounts => self.accounts,
            Tab::Shop => self.shop,
            Tab::Loadout => self.loadout,
            Tab::AimTrainer => self.aim_trainer,
            Tab::Settings => self.settings,
            Tab::LiveMatch => self.live_match,
        }
    }

    fn set(&mut self, tab: Tab, offset: AbsoluteOffset) {
        match tab {
            Tab::Accounts => self.accounts = offset,
            Tab::Shop => self.shop = offset,
            Tab::Loadout => self.loadout = offset,
            Tab::AimTrainer => self.aim_trainer = offset,
            Tab::Settings => self.settings = offset,
            Tab::LiveMatch => self.live_match = offset,
        }
    }
}

/// The Settings tab's sections, in page order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettingsSection {
    RiotClient,
    SystemTray,
    LiveMatch,
    Storage,
    Updates,
    Advanced,
}

impl SettingsSection {
    const ALL: [SettingsSection; 6] = [
        SettingsSection::RiotClient,
        SettingsSection::SystemTray,
        SettingsSection::LiveMatch,
        SettingsSection::Storage,
        SettingsSection::Updates,
        SettingsSection::Advanced,
    ];
}

impl std::fmt::Display for SettingsSection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SettingsSection::RiotClient => "Riot Client",
            SettingsSection::SystemTray => "System tray",
            SettingsSection::LiveMatch => "Live Match",
            SettingsSection::Storage => "Storage & cache",
            SettingsSection::Updates => "Updates",
            SettingsSection::Advanced => "Advanced",
        })
    }
}

/// The Accounts tab's sub-tabs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AccountsTab {
    Accounts,
    GameSettings,
}

impl std::fmt::Display for AccountsTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountsTab::Accounts => f.write_str("Accounts"),
            AccountsTab::GameSettings => f.write_str("Game settings"),
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
    Loaded(Result<LoadedAccounts, String>),
    Saved(Result<(), String>),
    TabSelected(Tab),
    Aim(AimMessage),
    AccountsTabSelected(AccountsTab),
    LoadoutTabSelected(LoadoutTab),
    MainPanelScrolled {
        tab: Tab,
        offset: AbsoluteOffset,
    },
    ToggleAccountSwitcher,
    /// A click outside the open account switcher or account menu.
    DismissPopovers,
    /// Closes the error toast.
    DismissStatus,
    /// Escape closes the topmost dialog or popover.
    EscapePressed,
    SelectAccount(AccountId),
    NewDisplayNameChanged(String),
    SaveSettingsOnAddToggled(bool),
    AddAccount,
    AddCurrentAccount,
    ConfirmAddAccountCapture,
    CancelAddAccountCapture,
    /// Riot Client was closed, the signed-in account's login saved, and Riot Client reopened for a
    /// sign-in. The outer error means Riot Client could not be reopened; the inner one that the
    /// previous login could not be saved.
    LoginCapturePrepared {
        target: LoginCaptureTarget,
        result: Result<data::launch_flow::PreviousAccountSync, String>,
    },
    CancelLoginCapture,
    /// Whether VALORANT was running when the add or re-capture prompt opened.
    CapturePromptGameChecked(bool),
    AccountCaptureFinished(Result<CapturedAccountDraft, String>),
    CurrentAccountCaptureFinished(Result<CapturedAccountDraft, String>),
    ConfirmCapturedAccount,
    CancelCapturedAccount,
    ToggleAccountMenu(AccountId),
    RequestExportAccount(AccountId),
    AccountExportPrepared(Result<AccountExportOutput, String>),
    CopyAccountExport,
    AccountExportCopied(String, Result<(), String>),
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
    ClientVersionLoaded {
        user_requested: bool,
        result: Result<String, String>,
    },
    /// Tries the automatic client version lookup again after it failed.
    RetryClientVersion,
    ImportRedirect,
    RequestLauncherSessionLogin(AccountId),
    CancelLauncherSessionLogin,
    StartLauncherSessionLogin(AccountId),
    /// Re-capture for the given account finished; the capture is staged in its own slot.
    LauncherSessionLoginStarted(
        AccountId,
        Result<crate::riot::launcher_session::CapturedLauncherSession, String>,
    ),
    RefreshProfileIdentity(AccountId),
    ProfileIdentityLoaded(AccountId, Result<RefreshedProfileIdentity, String>),
    /// Rank, level and penalty results. `announce` is false for follow-up refreshes of one
    /// account, whose results only update the cards.
    AccountRanksLoaded {
        result: AccountRanksResult,
        announce: bool,
    },
    AccountAvailabilityTimerTick(iced::time::Instant),
    WindowResized(iced::Size),
    /// The window's close button, Alt+F4 or the taskbar's Close window.
    CloseRequested(window::Id),
    Tray(tray::TrayAction),
    MinimizeOnCloseToggled(bool),
    /// Opens or closes the weapon picker under one of Settings' Live Match columns, from 0.
    ToggleWeaponPicker(usize),
    ResetLiveMatchWeapons,
    /// A weapon ID picked for one of Live Match's skin columns, counted from 0.
    LiveMatchWeaponPicked {
        column: usize,
        weapon: &'static str,
    },
    StatusTimerTick(iced::time::Instant),
    /// A frame while a dialog or toast settles in.
    AnimationFrame(iced::time::Instant),
    AccountAvailabilitiesLoaded(AccountAvailabilityRefresh),
    GameSettingsProfilesLoaded(Result<Vec<GameSettingsProfileMetadata>, String>),
    RequestDeleteSettingsProfile(String),
    CancelDeleteSettingsProfile,
    ConfirmDeleteSettingsProfile,
    SettingsProfileDeleted(String, Result<(), String>),
    /// Opens the name dialog for a new preset from this account's settings.
    RequestSavePreset(AccountId),
    RequestRenamePreset(String),
    /// Opens or closes the full list of a preset's settings.
    TogglePresetSettings(String),
    TogglePresetMenu(String),
    /// Shows this preset in the Apply panel.
    SelectPreset(String),
    PresetNameChanged(String),
    CancelPresetName,
    ConfirmPresetName,
    SaveSettingsPreset {
        account_id: AccountId,
        name: String,
    },
    AccountSettingsSaved(Result<SavedGameSettingsResult, String>),
    PresetRenamed(Result<GameSettingsProfileMetadata, String>),
    RequestApplyPreset {
        profile_id: String,
        account_id: AccountId,
    },
    SavedSettingsApplied(Result<AppliedGameSettingsResult, String>),
    RequestRestoreSettings(AccountId),
    /// The game check for the Apply or Restore dialog with this request ID (none when a recent
    /// result was used), with any session it refreshed, and whether VALORANT was running.
    SettingsPreflightChecked(
        u64,
        AccountId,
        Option<(AccountActivityCheck, Option<RefreshedApiContext>)>,
        bool,
    ),
    CancelSettingsChange,
    ConfirmSettingsChange,
    SettingsRestored(Result<RestoredGameSettingsResult, String>),
    /// The reply to the Shop load with this request ID.
    StorefrontLoaded(u64, Result<StorefrontResult, String>),
    RetryShop,
    ShopTimerTick(iced::time::Instant),
    LoadingTick,
    /// The reply to the Loadout load with this request ID.
    LoadoutLoaded(u64, Result<LoadoutResult, String>),
    /// Reloads the loadout and battle pass, which load together.
    RetryLoadout,
    /// The reply to the Live Match load with this request ID.
    LiveMatchLoaded(u64, Result<LiveMatchResult, LiveMatchError>),
    RetryLiveMatch,
    HiddenDetailsToggled,
    OpenImageViewer(ImageViewerRequest),
    ImageViewerImageLoaded(ImageViewerSource, Result<PathBuf, String>),
    CloseImageViewer,
    /// Opens a featured bundle's details, by its store ID.
    ShowBundleDetails(String),
    /// A bundle's item art finished caching, by the bundle's store ID.
    BundleItemArtLoaded(String, Vec<data::shop::BundleItemDisplay>),
    CloseBundleDetails,
    RiotClientPathChanged(String),
    BrowseRiotClientPath,
    RiotClientPathPicked(Option<PathBuf>),
    SaveSettings,
    SettingsSectionSelected(SettingsSection),
    ToggleTokenImport,
    OpenInExplorer(PathBuf),
    ExplorerOpened(Result<(), String>),
    ImageCacheSizeLoaded(Result<CacheUsage, String>),
    RankIconsLoaded(Result<HashMap<i64, PathBuf>, String>),
    PlayerCardArtLoaded(String, PlayerCardArt),
    AccountFilterChanged(String),
    ClearImageCache,
    ImageCacheCleared(Result<(), String>),
    LaunchAccount(AccountId),
    /// Preflight result for a launch, and whether VALORANT was already running.
    LaunchPreflightChecked(AccountActivityCheck, bool),
    CancelUnavailableLaunch,
    LaunchAnyway(AccountId),
    LaunchProgressTick,
    LaunchProgressChecked(Result<bool, String>),
    LaunchFinished(Result<LaunchAccountResult, String>),
    /// The chat proxy for a launch of this account started, or why it couldn't.
    ChatProxyStarted(AccountId, Result<ChatProxy, String>),
    /// Launches without the chat proxy, from the "Can't go invisible" dialog.
    LaunchOnline(AccountId),
    CancelStatusLaunch,
    /// Quits anyway, from the dialog that warns it drops Riot Client's chat.
    ConfirmQuit,
    CancelQuit,
    LocalGameTick,
    LocalGameChecked(LocalGame),
    ToggleStatusMenu,
    PresenceStatusPicked(crate::riot::chat_proxy::PresenceStatus),
    CheckForAppUpdate,
    AppUpdateChecked {
        user_requested: bool,
        result: Result<UpdateCheckOutcome, String>,
    },
    DismissAppUpdate,
    ShowAppUpdate,
    DownloadAppUpdate,
    AppUpdatePrepared(Result<(), String>),
}
