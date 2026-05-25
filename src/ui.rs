use std::path::PathBuf;

use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input};
use iced::{Element, Length, Task, Theme};

use crate::account::LauncherSessionBackup;
use crate::account::{AccountId, AccountProfile, AuthSession, Shard};
use crate::launch::{
    LaunchConfig, close_riot_processes, launch_riot_login_capture, launch_valorant,
};
use crate::riot::auth::parse_redirect_tokens;
use crate::riot::client::{ApiCredentials, RiotApi};
use crate::riot::content::{
    CurrencyCatalog, ResolvedCurrency, ResolvedSkin, ResolvedWeapon, SkinCatalog,
    ValorantContentApi, WeaponCatalog,
};
use crate::riot::launcher_session::{
    CapturedLauncherSession, apply_launcher_session_backup, capture_current_launcher_session,
    clear_existing_launcher_data_dirs, launcher_cookie_header, read_backup_cookies,
};
use crate::riot::models::{BonusStoreOffer, PlayerLoadoutResponse, StoreOffer, StorefrontResponse};
use crate::storage::{AccountRepository, StoredState};

pub fn run() -> iced::Result {
    iced::application(PrimeApp::boot, PrimeApp::update, PrimeApp::view)
        .title(app_title)
        .theme(app_theme)
        .window_size((1100.0, 720.0))
        .run()
}

fn app_title(_: &PrimeApp) -> String {
    "Prime Valorant Manager".to_string()
}

fn app_theme(_: &PrimeApp) -> Theme {
    Theme::Dark
}

#[derive(Clone, Debug)]
struct PrimeApp {
    repo: AccountRepository,
    state: StoredState,
    active_tab: Tab,
    new_display_name: String,
    new_username: String,
    new_shard: Shard,
    redirect_input: String,
    client_version_input: String,
    riot_client_path_input: String,
    status: String,
    store_summary: Option<StoreSummary>,
    loadout_summary: Option<LoadoutSummary>,
}

impl PrimeApp {
    fn boot() -> (Self, Task<Message>) {
        let repo = AccountRepository::new(AccountRepository::default_path());
        let load_repo = repo.clone();

        (
            Self {
                repo,
                state: StoredState::default(),
                active_tab: Tab::Accounts,
                new_display_name: String::new(),
                new_username: String::new(),
                new_shard: Shard::Na,
                redirect_input: String::new(),
                client_version_input: String::new(),
                riot_client_path_input: String::new(),
                status: "Loading accounts".to_string(),
                store_summary: None,
                loadout_summary: None,
            },
            Task::batch([
                Task::perform(
                    async move { load_repo.load().map_err(|error| error.to_string()) },
                    Message::Loaded,
                ),
                Task::perform(fetch_current_client_version(), Message::ClientVersionLoaded),
            ]),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loaded(result) => {
                match result {
                    Ok(state) => {
                        self.riot_client_path_input = state
                            .riot_client_path
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_default();
                        self.state = state;
                        self.status = format!(
                            "Loaded {} account profile(s) from {}",
                            self.state.accounts.len(),
                            self.repo.path().display()
                        );
                    }
                    Err(error) => {
                        self.status = format!("Failed to load accounts: {error}");
                    }
                }

                Task::none()
            }
            Message::Saved(result) => {
                if let Err(error) = result {
                    self.status = format!("Failed to save accounts: {error}");
                }

                Task::none()
            }
            Message::TabSelected(tab) => {
                self.active_tab = tab;
                Task::none()
            }
            Message::SelectAccount(id) => {
                self.state.selected_account = Some(id);
                self.store_summary = None;
                self.loadout_summary = None;
                self.status = self
                    .state
                    .selected_account()
                    .map(|account| format!("Selected {}", account.summary()))
                    .unwrap_or_else(|| "No account selected".to_string());
                self.save_task()
            }
            Message::NewDisplayNameChanged(value) => {
                self.new_display_name = value;
                Task::none()
            }
            Message::NewUsernameChanged(value) => {
                self.new_username = value;
                Task::none()
            }
            Message::NewShardSelected(shard) => {
                self.new_shard = shard;
                Task::none()
            }
            Message::AddAccount => {
                match AccountProfile::new(
                    self.new_display_name.clone(),
                    Some(self.new_username.clone()),
                    self.new_shard,
                ) {
                    Ok(account) => {
                        self.status = format!("Added {}", account.summary());
                        self.state.push_account(account);
                        self.new_display_name.clear();
                        self.new_username.clear();
                        return self.save_task();
                    }
                    Err(error) => {
                        self.status = error.to_string();
                    }
                }

                Task::none()
            }
            Message::DeleteSelected => {
                if let Some(id) = self.state.selected_account {
                    self.state.remove_account(id);
                    self.status = "Removed selected account profile".to_string();
                    return self.save_task();
                }

                self.status = "No account selected".to_string();
                Task::none()
            }
            Message::RedirectChanged(value) => {
                self.redirect_input = value;
                Task::none()
            }
            Message::ClientVersionChanged(value) => {
                self.client_version_input = value;
                Task::none()
            }
            Message::RefreshClientVersion => {
                self.status = "Refreshing Riot client version".to_string();
                Task::perform(fetch_current_client_version(), Message::ClientVersionLoaded)
            }
            Message::ClientVersionLoaded(result) => {
                match result {
                    Ok(version) => {
                        if self.client_version_input.trim().is_empty() {
                            self.client_version_input = version.clone();
                        }
                        self.status = format!("Current Riot client version: {version}");
                    }
                    Err(error) => {
                        if self.status == "Loading accounts" {
                            self.status = format!("Could not fetch Riot client version: {error}");
                        }
                    }
                }

                Task::none()
            }
            Message::ImportRedirect => {
                let Some(account) = self.state.selected_account_mut() else {
                    self.status = "Select an account before importing a token".to_string();
                    return Task::none();
                };

                match parse_redirect_tokens(&self.redirect_input) {
                    Ok(tokens) => {
                        account.session = Some(tokens.into_session());
                        self.redirect_input.clear();
                        self.status =
                            "Imported Riot redirect token for selected account".to_string();
                        self.save_task()
                    }
                    Err(error) => {
                        self.status = format!("Could not import redirect token: {error}");
                        Task::none()
                    }
                }
            }
            Message::StartLauncherSessionLogin => {
                if self.state.selected_account().is_none() {
                    self.status =
                        "Select an account before starting launcher session capture".to_string();
                    return Task::none();
                }

                let config = LaunchConfig {
                    riot_client_path: self.state.riot_client_path.clone(),
                    ..LaunchConfig::default()
                };
                self.status =
                    "Opening Riot Client for a fresh remembered login capture".to_string();

                Task::perform(
                    async move { start_launcher_session_login(config).await },
                    Message::LauncherSessionLoginStarted,
                )
            }
            Message::LauncherSessionLoginStarted(result) => {
                self.status = match result {
                    Ok(()) => "Riot Client opened. Log into the selected account with Remember Me enabled, then press Capture launcher session.".to_string(),
                    Err(error) => format!("Could not start launcher session login: {error}"),
                };

                Task::none()
            }
            Message::RefreshProfileIdentity => {
                let Some(account) = self.state.selected_account().cloned() else {
                    self.status =
                        "Select an account before refreshing profile identity".to_string();
                    return Task::none();
                };

                self.status = "Refreshing Riot profile identity".to_string();
                Task::perform(
                    fetch_profile_identity(account),
                    Message::ProfileIdentityLoaded,
                )
            }
            Message::ProfileIdentityLoaded(result) => {
                match result {
                    Ok(identity) => {
                        if let Some(account) = self
                            .state
                            .accounts
                            .iter_mut()
                            .find(|account| account.id == identity.account_id)
                        {
                            if let Err(error) = account.apply_riot_identity(
                                identity.puuid,
                                identity.game_name,
                                identity.tag_line,
                            ) {
                                self.status = format!("Profile identity rejected: {error}");
                                return Task::none();
                            }

                            account.session = Some(identity.session);
                            self.status = format!("Refreshed {}", account.summary());
                            return self.save_task();
                        }

                        self.status =
                            "Refreshed profile identity, but the selected profile no longer exists"
                                .to_string();
                    }
                    Err(error) => {
                        self.status = format!("Profile refresh failed: {error}");
                    }
                }

                Task::none()
            }
            Message::CaptureLauncherSession => {
                let Some(account_id) = self.state.selected_account else {
                    self.status =
                        "Select an account before capturing a launcher session".to_string();
                    return Task::none();
                };

                let backup_root = self.repo.launcher_backups_dir();
                self.status = "Capturing current Riot Client launcher session".to_string();

                Task::perform(
                    async move {
                        capture_current_launcher_session(account_id, backup_root)
                            .map_err(|error| error.to_string())
                    },
                    Message::LauncherSessionCaptured,
                )
            }
            Message::LauncherSessionCaptured(result) => {
                match result {
                    Ok(captured) => {
                        if let Some(account) = self
                            .state
                            .accounts
                            .iter_mut()
                            .find(|account| account.id == captured.account_id)
                        {
                            let captured_puuid = captured.backup.puuid.clone();

                            if let Err(error) = account.attach_launcher_session(captured.backup) {
                                self.status = format!("Launcher session rejected: {error}");
                                return Task::none();
                            }

                            self.status = format!(
                                "Captured launcher session for selected account ({captured_puuid})"
                            );
                            return self.save_task();
                        }

                        self.status =
                            "Captured launcher session, but the selected profile no longer exists"
                                .to_string();
                    }
                    Err(error) => {
                        self.status = format!("Launcher session capture failed: {error}");
                    }
                }

                Task::none()
            }
            Message::FetchStorefront => {
                let Some(account) = self.state.selected_account().cloned() else {
                    self.status = "Select an account before checking the shop".to_string();
                    return Task::none();
                };

                self.status = "Checking store".to_string();
                Task::perform(
                    fetch_storefront(account, self.client_version_input.clone()),
                    Message::StorefrontLoaded,
                )
            }
            Message::StorefrontLoaded(result) => {
                match result {
                    Ok(result) => {
                        cache_account_session(&mut self.state, result.account_id, result.session);
                        let daily_count = result.summary.daily_offers.len();
                        let night_market_count = result.summary.night_market_offers.len();

                        self.status = format!(
                            "Loaded {} daily offer(s) and {} night market offer(s)",
                            daily_count, night_market_count
                        );
                        if self.state.selected_account == Some(result.account_id) {
                            self.store_summary = Some(result.summary);
                        }

                        return self.save_task();
                    }
                    Err(error) => {
                        self.status = format!("Store check failed: {error}");
                    }
                }

                Task::none()
            }
            Message::FetchLoadout => {
                let Some(account) = self.state.selected_account().cloned() else {
                    self.status = "Select an account before checking loadout".to_string();
                    return Task::none();
                };

                self.status = "Checking loadout".to_string();
                Task::perform(
                    fetch_loadout(account, self.client_version_input.clone()),
                    Message::LoadoutLoaded,
                )
            }
            Message::LoadoutLoaded(result) => {
                match result {
                    Ok(result) => {
                        cache_account_session(&mut self.state, result.account_id, result.session);
                        let gun_count = result.summary.gun_skins.len();

                        self.status = format!("Loaded loadout with {} gun skin(s)", gun_count);
                        if self.state.selected_account == Some(result.account_id) {
                            self.loadout_summary = Some(result.summary);
                        }

                        return self.save_task();
                    }
                    Err(error) => {
                        self.status = format!("Loadout check failed: {error}");
                    }
                }

                Task::none()
            }
            Message::RiotClientPathChanged(value) => {
                self.riot_client_path_input = value;
                Task::none()
            }
            Message::SaveSettings => {
                self.state.riot_client_path = non_empty_path(&self.riot_client_path_input);
                self.status = "Saved settings".to_string();
                self.save_task()
            }
            Message::LaunchSelected => {
                let Some(account) = self.state.selected_account() else {
                    self.status = "Select an account before launching".to_string();
                    return Task::none();
                };

                let config = LaunchConfig {
                    riot_client_path: self.state.riot_client_path.clone(),
                    ..LaunchConfig::default()
                };
                let backup = account.launcher_session.clone();

                Task::perform(
                    async move { launch_account(config, backup).await },
                    Message::LaunchFinished,
                )
            }
            Message::LaunchFinished(result) => {
                self.status = match result {
                    Ok(()) => "Prepared selected launcher session and sent VALORANT launch request to Riot Client".to_string(),
                    Err(error) => format!("Launch failed: {error}"),
                };

                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let content = row![
            self.sidebar(),
            container(self.main_panel())
                .padding(18)
                .width(Length::Fill)
                .height(Length::Fill)
        ]
        .height(Length::Fill);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let mut accounts = column![text("Accounts").size(24)].spacing(8);

        if self.state.accounts.is_empty() {
            accounts = accounts.push(text("No profiles yet"));
        }

        for account in &self.state.accounts {
            let prefix = if self.state.selected_account == Some(account.id) {
                ">"
            } else {
                " "
            };
            let session = match (account.has_api_session(), account.has_launcher_session()) {
                (true, true) => "api + launcher",
                (true, false) => "api",
                (false, true) => "launcher",
                (false, false) => "no session",
            };
            let label = format!("{prefix} {} [{session}]", account.summary());
            accounts = accounts.push(
                button(text(label))
                    .width(Length::Fill)
                    .on_press(Message::SelectAccount(account.id)),
            );
        }

        let tabs = column![
            text(""),
            button("Accounts")
                .width(Length::Fill)
                .on_press(Message::TabSelected(Tab::Accounts)),
            button("Shop")
                .width(Length::Fill)
                .on_press(Message::TabSelected(Tab::Shop)),
            button("Loadout")
                .width(Length::Fill)
                .on_press(Message::TabSelected(Tab::Loadout)),
            button("Settings")
                .width(Length::Fill)
                .on_press(Message::TabSelected(Tab::Settings)),
        ]
        .spacing(8);

        container(scrollable(column![accounts, tabs].spacing(16)))
            .padding(16)
            .width(280)
            .height(Length::Fill)
            .into()
    }

    fn main_panel(&self) -> Element<'_, Message> {
        let body = match self.active_tab {
            Tab::Accounts => self.accounts_tab(),
            Tab::Shop => self.shop_tab(),
            Tab::Loadout => self.loadout_tab(),
            Tab::Settings => self.settings_tab(),
        };

        column![
            row![
                text(self.active_tab.to_string()).size(28),
                button("Launch VALORANT").on_press(Message::LaunchSelected)
            ]
            .spacing(16),
            text(""),
            body,
            text(""),
            text(&self.status)
        ]
        .spacing(14)
        .into()
    }

    fn accounts_tab(&self) -> Element<'_, Message> {
        let selected = self
            .state
            .selected_account()
            .map(|account| account.summary())
            .unwrap_or_else(|| "No account selected".to_string());
        let launcher_session = self
            .state
            .selected_account()
            .and_then(|account| account.launcher_session.as_ref())
            .map(|backup| {
                format!(
                    "Launcher session: captured for {} at {}",
                    backup.puuid, backup.captured_at_unix
                )
            })
            .unwrap_or_else(|| "Launcher session: not captured".to_string());

        column![
            text(format!("Selected: {selected}")),
            text(launcher_session),
            row![
                text_input("Display name", &self.new_display_name)
                    .on_input(Message::NewDisplayNameChanged)
                    .width(Length::Fill),
                text_input("Riot username (optional)", &self.new_username)
                    .on_input(Message::NewUsernameChanged)
                    .width(Length::Fill),
                pick_list(
                    Shard::ALL.as_slice(),
                    Some(self.new_shard),
                    Message::NewShardSelected
                )
            ]
            .spacing(10),
            row![
                button("Add account").on_press(Message::AddAccount),
                button("Remove selected").on_press(Message::DeleteSelected),
                button("Start login capture").on_press(Message::StartLauncherSessionLogin),
                button("Capture launcher session").on_press(Message::CaptureLauncherSession),
                button("Refresh profile").on_press(Message::RefreshProfileIdentity)
            ]
            .spacing(10),
            text("Start login capture clears stale Riot Client login data and opens Riot Client. Log in with Remember Me enabled, then capture the launcher session for this profile."),
            text(""),
            text("Riot web redirect token"),
            text_input(
                "Paste https://playvalorant.com/opt_in#access_token=...",
                &self.redirect_input
            )
            .on_input(Message::RedirectChanged),
            row![
                text_input(
                    "Client version, for example release-10.00-shipping-...",
                    &self.client_version_input
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

    fn shop_tab(&self) -> Element<'_, Message> {
        let mut content = column![
            text("Daily store offers require a selected profile with an imported Riot token, PUUID, shard, entitlement token, and current client version."),
            button("Check shop").on_press(Message::FetchStorefront)
        ]
        .spacing(12);

        if let Some(summary) = &self.store_summary {
            content = content
                .push(text(format!(
                    "Bundle expires in {} seconds",
                    summary.bundle_remaining_seconds
                )))
                .push(text(format!(
                    "Daily offers expire in {} seconds",
                    summary.daily_remaining_seconds
                )))
                .push(text(format!(
                    "Daily offers: {}",
                    summary
                        .daily_offers
                        .iter()
                        .map(StoreOfferDisplay::label)
                        .collect::<Vec<_>>()
                        .join(", ")
                )))
                .push(text(format!(
                    "Night market offers: {}",
                    summary
                        .night_market_offers
                        .iter()
                        .map(StoreOfferDisplay::label)
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
        }

        content.into()
    }

    fn loadout_tab(&self) -> Element<'_, Message> {
        let mut content = column![
            text("Loadout reads the selected account's equipped skins from the personalization endpoint."),
            button("Check loadout").on_press(Message::FetchLoadout)
        ]
        .spacing(12);

        if let Some(summary) = &self.loadout_summary {
            content = content
                .push(text(format!("Account level: {}", summary.account_level)))
                .push(text(format!(
                    "Equipped skins: {}",
                    summary
                        .gun_skins
                        .iter()
                        .map(LoadoutGunDisplay::label)
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
        }

        content.into()
    }

    fn settings_tab(&self) -> Element<'_, Message> {
        column![
            text(format!("Profile storage: {}", self.repo.path().display())),
            text_input(
                r"C:\Riot Games\Riot Client\RiotClientServices.exe",
                &self.riot_client_path_input
            )
            .on_input(Message::RiotClientPathChanged),
            button("Save settings").on_press(Message::SaveSettings)
        ]
        .spacing(12)
        .into()
    }

    fn save_task(&self) -> Task<Message> {
        let repo = self.repo.clone();
        let state = self.state.clone();

        Task::perform(
            async move { repo.save(&state).map_err(|error| error.to_string()) },
            Message::Saved,
        )
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
    DeleteSelected,
    RedirectChanged(String),
    ClientVersionChanged(String),
    RefreshClientVersion,
    ClientVersionLoaded(Result<String, String>),
    ImportRedirect,
    StartLauncherSessionLogin,
    LauncherSessionLoginStarted(Result<(), String>),
    RefreshProfileIdentity,
    ProfileIdentityLoaded(Result<RefreshedProfileIdentity, String>),
    CaptureLauncherSession,
    LauncherSessionCaptured(Result<CapturedLauncherSession, String>),
    FetchStorefront,
    StorefrontLoaded(Result<StorefrontResult, String>),
    FetchLoadout,
    LoadoutLoaded(Result<LoadoutResult, String>),
    RiotClientPathChanged(String),
    SaveSettings,
    LaunchSelected,
    LaunchFinished(Result<(), String>),
}

async fn launch_account(
    config: LaunchConfig,
    backup: Option<LauncherSessionBackup>,
) -> Result<(), String> {
    let backup = require_launcher_session(backup)?;

    close_riot_processes().map_err(|error| error.to_string())?;
    apply_launcher_session_backup(&backup).map_err(|error| error.to_string())?;

    launch_valorant(&config).map_err(|error| error.to_string())
}

fn require_launcher_session(
    backup: Option<LauncherSessionBackup>,
) -> Result<LauncherSessionBackup, String> {
    let Some(backup) = backup else {
        return Err(
            "selected account does not have a captured launcher session; start login capture first"
                .to_string(),
        );
    };

    if !backup.is_ready() {
        return Err(
            "selected account launcher session is incomplete or its backup folder is missing"
                .to_string(),
        );
    }

    Ok(backup)
}

async fn start_launcher_session_login(config: LaunchConfig) -> Result<(), String> {
    close_riot_processes().map_err(|error| error.to_string())?;
    clear_existing_launcher_data_dirs().map_err(|error| error.to_string())?;
    launch_riot_login_capture(&config).map_err(|error| error.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RefreshedProfileIdentity {
    account_id: AccountId,
    session: AuthSession,
    puuid: String,
    game_name: String,
    tag_line: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StorefrontResult {
    account_id: AccountId,
    summary: StoreSummary,
    session: AuthSession,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LoadoutResult {
    account_id: AccountId,
    summary: LoadoutSummary,
    session: AuthSession,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StoreSummary {
    daily_offers: Vec<StoreOfferDisplay>,
    daily_remaining_seconds: i64,
    bundle_remaining_seconds: i64,
    night_market_offers: Vec<StoreOfferDisplay>,
}

impl StoreSummary {
    fn from_response(
        response: StorefrontResponse,
        skins: &SkinCatalog,
        currencies: &CurrencyCatalog,
    ) -> Self {
        let daily_offers = response
            .skins_panel_layout
            .single_item_offers
            .iter()
            .map(|offer_id| {
                let matching_offer = response
                    .skins_panel_layout
                    .single_item_store_offers
                    .iter()
                    .find(|offer| offer.offer_id == *offer_id);

                store_offer_display(offer_id, matching_offer, 0, skins, currencies)
            })
            .collect();
        let night_market_offers = response
            .bonus_store
            .map(|store| {
                store
                    .bonus_store_offers
                    .iter()
                    .map(|offer| bonus_store_offer_display(offer, skins, currencies))
                    .collect()
            })
            .unwrap_or_default();

        Self {
            daily_offers,
            daily_remaining_seconds: response
                .skins_panel_layout
                .single_item_offers_remaining_duration_in_seconds,
            bundle_remaining_seconds: response
                .featured_bundle
                .bundle_remaining_duration_in_seconds,
            night_market_offers,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StoreOfferDisplay {
    skin: SkinDisplay,
    price: Option<OfferPrice>,
    discount_percent: i64,
}

impl StoreOfferDisplay {
    fn label(&self) -> String {
        let mut label = self.skin.display_name.clone();

        if let Some(price) = &self.price {
            label.push_str(&format!(" ({})", price.label()));
        }

        if self.discount_percent > 0 {
            label.push_str(&format!(", {}% off", self.discount_percent));
        }

        label
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OfferPrice {
    amount: i64,
    currency: CurrencyDisplay,
}

impl OfferPrice {
    fn label(&self) -> String {
        format!("{} {}", self.amount, self.currency.display_name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CurrencyDisplay {
    uuid: String,
    display_name: String,
    display_icon: Option<String>,
}

impl From<ResolvedCurrency> for CurrencyDisplay {
    fn from(currency: ResolvedCurrency) -> Self {
        Self {
            uuid: currency.uuid,
            display_name: currency.display_name,
            display_icon: currency.display_icon,
        }
    }
}

fn store_offer_display(
    offer_id: &str,
    offer: Option<&StoreOffer>,
    discount_percent: i64,
    skins: &SkinCatalog,
    currencies: &CurrencyCatalog,
) -> StoreOfferDisplay {
    let direct = skins.resolve(offer_id);
    let skin = if direct.display_name != offer_id {
        SkinDisplay::from(direct)
    } else {
        offer
            .and_then(|offer| offer.rewards.first())
            .map(|reward| SkinDisplay::from(skins.resolve(&reward.item_id)))
            .unwrap_or_else(|| SkinDisplay::from(direct))
    };
    let price = offer.and_then(|offer| offer_price(&offer.cost, currencies));

    StoreOfferDisplay {
        skin,
        price,
        discount_percent,
    }
}

fn bonus_store_offer_display(
    offer: &BonusStoreOffer,
    skins: &SkinCatalog,
    currencies: &CurrencyCatalog,
) -> StoreOfferDisplay {
    let skin = offer
        .offer
        .rewards
        .first()
        .map(|reward| SkinDisplay::from(skins.resolve(&reward.item_id)))
        .unwrap_or_else(|| SkinDisplay::from(skins.resolve(&offer.offer.offer_id)));
    let price = offer_price(&offer.discount_costs, currencies)
        .or_else(|| offer_price(&offer.offer.cost, currencies));

    StoreOfferDisplay {
        skin,
        price,
        discount_percent: offer.discount_percent,
    }
}

fn offer_price(
    costs: &std::collections::HashMap<String, i64>,
    currencies: &CurrencyCatalog,
) -> Option<OfferPrice> {
    let (currency_id, amount) = costs.iter().min_by(|left, right| left.0.cmp(right.0))?;

    Some(OfferPrice {
        amount: *amount,
        currency: CurrencyDisplay::from(currencies.resolve(currency_id)),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LoadoutSummary {
    account_level: i64,
    gun_skins: Vec<LoadoutGunDisplay>,
}

impl LoadoutSummary {
    fn from_response(
        response: PlayerLoadoutResponse,
        skins: &SkinCatalog,
        weapons: &WeaponCatalog,
    ) -> Self {
        Self {
            account_level: response.identity.account_level,
            gun_skins: response
                .guns
                .into_iter()
                .map(|gun| {
                    let weapon = WeaponDisplay::from(weapons.resolve(&gun.id));
                    let skin = SkinDisplay::from(resolve_first(
                        skins,
                        [&gun.skin_id, &gun.skin_level_id, &gun.chroma_id],
                    ));

                    LoadoutGunDisplay { weapon, skin }
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LoadoutGunDisplay {
    weapon: WeaponDisplay,
    skin: SkinDisplay,
}

impl LoadoutGunDisplay {
    fn label(&self) -> String {
        format!("{}: {}", self.weapon.display_name, self.skin.display_name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WeaponDisplay {
    uuid: String,
    display_name: String,
    display_icon: Option<String>,
}

impl From<ResolvedWeapon> for WeaponDisplay {
    fn from(weapon: ResolvedWeapon) -> Self {
        Self {
            uuid: weapon.uuid,
            display_name: weapon.display_name,
            display_icon: weapon.display_icon,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SkinDisplay {
    uuid: String,
    display_name: String,
    display_icon: Option<String>,
}

impl From<ResolvedSkin> for SkinDisplay {
    fn from(skin: ResolvedSkin) -> Self {
        Self {
            uuid: skin.uuid,
            display_name: skin.display_name,
            display_icon: skin.display_icon,
        }
    }
}

async fn fetch_storefront(
    account: AccountProfile,
    client_version: String,
) -> Result<StorefrontResult, String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let resolved = resolve_credentials(&api, &account, client_version).await?;
    let metadata = fetch_store_metadata().await;
    let summary = api
        .storefront(&resolved.credentials)
        .await
        .map(|response| {
            StoreSummary::from_response(response, &metadata.skins, &metadata.currencies)
        })
        .map_err(|error| error.to_string())?;

    Ok(StorefrontResult {
        account_id: account.id,
        summary,
        session: resolved.session,
    })
}

async fn fetch_loadout(
    account: AccountProfile,
    client_version: String,
) -> Result<LoadoutResult, String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let resolved = resolve_credentials(&api, &account, client_version).await?;
    let metadata = fetch_loadout_metadata().await;
    let summary = api
        .player_loadout(&resolved.credentials)
        .await
        .map(|response| LoadoutSummary::from_response(response, &metadata.skins, &metadata.weapons))
        .map_err(|error| error.to_string())?;

    Ok(LoadoutResult {
        account_id: account.id,
        summary,
        session: resolved.session,
    })
}

async fn fetch_profile_identity(
    account: AccountProfile,
) -> Result<RefreshedProfileIdentity, String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let session = active_api_session(&api, &account).await?;
    let player_info = api
        .player_info(&session.access_token)
        .await
        .map_err(|error| error.to_string())?;

    Ok(RefreshedProfileIdentity {
        account_id: account.id,
        session,
        puuid: player_info.sub,
        game_name: player_info.acct.game_name,
        tag_line: player_info.acct.tag_line,
    })
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct StoreMetadata {
    skins: SkinCatalog,
    currencies: CurrencyCatalog,
}

async fn fetch_store_metadata() -> StoreMetadata {
    match ValorantContentApi::new() {
        Ok(api) => StoreMetadata {
            skins: api.skin_catalog().await.unwrap_or_default(),
            currencies: api.currency_catalog().await.unwrap_or_default(),
        },
        Err(_) => StoreMetadata::default(),
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct LoadoutMetadata {
    skins: SkinCatalog,
    weapons: WeaponCatalog,
}

async fn fetch_loadout_metadata() -> LoadoutMetadata {
    match ValorantContentApi::new() {
        Ok(api) => LoadoutMetadata {
            skins: api.skin_catalog().await.unwrap_or_default(),
            weapons: api.weapon_catalog().await.unwrap_or_default(),
        },
        Err(_) => LoadoutMetadata::default(),
    }
}

async fn fetch_current_client_version() -> Result<String, String> {
    ValorantContentApi::new()
        .map_err(|error| error.to_string())?
        .client_version()
        .await
        .map_err(|error| error.to_string())
}

fn resolve_first<'a>(
    catalog: &SkinCatalog,
    ids: impl IntoIterator<Item = &'a String>,
) -> ResolvedSkin {
    let ids = ids.into_iter().collect::<Vec<_>>();

    for id in &ids {
        let raw = id.as_str();
        let skin = catalog.resolve(raw);

        if skin.display_name != raw {
            return skin;
        }
    }

    ids.first()
        .map(|id| catalog.resolve(id.as_str()))
        .unwrap_or_else(|| ResolvedSkin::unknown(""))
}

async fn resolve_credentials(
    api: &RiotApi,
    account: &AccountProfile,
    client_version: String,
) -> Result<ResolvedApiCredentials, String> {
    let mut session = active_api_session(api, account).await?;

    let entitlements_token = entitlement_token(api, &session).await?;
    if session
        .entitlements_token
        .as_ref()
        .is_none_or(|token| token.trim().is_empty())
    {
        session.entitlements_token = Some(entitlements_token.clone());
    }

    let puuid = match &account.puuid {
        Some(puuid) if !puuid.trim().is_empty() => puuid.clone(),
        _ if account.launcher_session.is_some() => account
            .launcher_session
            .as_ref()
            .map(|backup| backup.puuid.clone())
            .filter(|puuid| !puuid.trim().is_empty())
            .unwrap_or_default(),
        _ => {
            api.player_info(&session.access_token)
                .await
                .map_err(|error| error.to_string())?
                .sub
        }
    };

    Ok(ResolvedApiCredentials {
        credentials: ApiCredentials {
            access_token: session.access_token.clone(),
            entitlements_token,
            client_version,
            shard: account.shard,
            puuid,
        },
        session,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedApiCredentials {
    credentials: ApiCredentials,
    session: AuthSession,
}

async fn active_api_session(
    api: &RiotApi,
    account: &AccountProfile,
) -> Result<AuthSession, String> {
    if let Some(session) = &account.session
        && !session.is_expired()
    {
        return Ok(session.clone());
    }

    let Some(backup) = &account.launcher_session else {
        return Err(
            "selected account needs an imported Riot token or a captured launcher session"
                .to_string(),
        );
    };

    let cookies = read_backup_cookies(backup).map_err(|error| error.to_string())?;
    let cookie_header = launcher_cookie_header(&cookies).map_err(|error| error.to_string())?;
    api.cookie_reauth(&cookie_header)
        .await
        .map(|tokens| tokens.into_session())
        .map_err(|error| {
            format!(
                "launcher session reauth failed; recapture the Riot Client session or import a fresh redirect token: {error}"
            )
        })
}

async fn entitlement_token(api: &RiotApi, session: &AuthSession) -> Result<String, String> {
    if let Some(token) = &session.entitlements_token
        && !token.trim().is_empty()
    {
        return Ok(token.clone());
    }

    api.entitlement(&session.access_token)
        .await
        .map(|response| response.entitlements_token)
        .map_err(|error| error.to_string())
}

fn non_empty_path(input: &str) -> Option<PathBuf> {
    let trimmed = input.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn cache_account_session(
    state: &mut StoredState,
    account_id: AccountId,
    session: AuthSession,
) -> bool {
    let Some(account) = state
        .accounts
        .iter_mut()
        .find(|account| account.id == account_id)
    else {
        return false;
    };

    account.session = Some(session);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn store_summary_counts_night_market() {
        let response: StorefrontResponse = serde_json::from_value(serde_json::json!({
            "FeaturedBundle": {
                "Bundle": {
                    "ID": "bundle",
                    "DataAssetID": "asset",
                    "CurrencyID": "vp",
                    "Items": [],
                    "DurationRemainingInSeconds": 10
                },
                "Bundles": [],
                "BundleRemainingDurationInSeconds": 20
            },
            "SkinsPanelLayout": {
                "SingleItemOffers": ["a", "b"],
                "SingleItemStoreOffers": [{
                    "OfferID": "a",
                    "IsDirectPurchase": true,
                    "StartDate": "2026-05-25T00:00:00Z",
                    "Cost": {"vp": 1775},
                    "Rewards": [{
                        "ItemTypeID": "skin-type",
                        "ItemID": "a",
                        "Quantity": 1
                    }]
                }],
                "SingleItemOffersRemainingDurationInSeconds": 30
            },
            "BonusStore": {
                "BonusStoreOffers": [{
                    "BonusOfferID": "bonus",
                    "Offer": {
                        "OfferID": "offer",
                        "IsDirectPurchase": true,
                        "StartDate": "2026-05-25T00:00:00Z",
                        "Cost": {},
                        "Rewards": [{
                            "ItemTypeID": "skin-type",
                            "ItemID": "a",
                            "Quantity": 1
                        }]
                    },
                    "DiscountPercent": 10,
                    "DiscountCosts": {"vp": 1200},
                    "IsSeen": false
                }],
                "BonusStoreRemainingDurationInSeconds": 40
            }
        }))
        .expect("response");

        let catalog = SkinCatalog::from_skins(vec![crate::riot::content::WeaponSkin {
            uuid: "skin-a".to_string(),
            display_name: "Prime Vandal".to_string(),
            display_icon: None,
            levels: vec![crate::riot::content::WeaponSkinLevel {
                uuid: "a".to_string(),
                display_name: "Prime Vandal Level 1".to_string(),
                display_icon: None,
            }],
            chromas: vec![],
        }]);
        let currencies = CurrencyCatalog::from_currencies(vec![crate::riot::content::Currency {
            uuid: "vp".to_string(),
            display_name: "VP".to_string(),
            display_icon: None,
        }]);
        let summary = StoreSummary::from_response(response, &catalog, &currencies);

        assert_eq!(
            summary
                .daily_offers
                .iter()
                .map(StoreOfferDisplay::label)
                .collect::<Vec<_>>(),
            ["Prime Vandal Level 1 (1775 VP)", "b"]
        );
        assert_eq!(summary.daily_remaining_seconds, 30);
        assert_eq!(summary.bundle_remaining_seconds, 20);
        assert_eq!(
            summary
                .night_market_offers
                .iter()
                .map(StoreOfferDisplay::label)
                .collect::<Vec<_>>(),
            ["Prime Vandal Level 1 (1200 VP), 10% off"]
        );
    }

    #[test]
    fn loadout_summary_resolves_skin_names() {
        let response: PlayerLoadoutResponse = serde_json::from_value(serde_json::json!({
            "Subject": "puuid",
            "Version": 1,
            "Guns": [{
                "ID": "weapon",
                "SkinID": "skin-a",
                "SkinLevelID": "level-a",
                "ChromaID": "chroma-a",
                "Attachments": []
            }],
            "Sprays": [],
            "Identity": {
                "PlayerCardID": "card",
                "PlayerTitleID": "title",
                "AccountLevel": 42,
                "PreferredLevelBorderID": "border",
                "HideAccountLevel": false
            },
            "Incognito": false
        }))
        .expect("loadout");
        let catalog = SkinCatalog::from_skins(vec![crate::riot::content::WeaponSkin {
            uuid: "skin-a".to_string(),
            display_name: "Prime Vandal".to_string(),
            display_icon: None,
            levels: vec![],
            chromas: vec![],
        }]);
        let weapons = WeaponCatalog::from_weapons(vec![crate::riot::content::Weapon {
            uuid: "weapon".to_string(),
            display_name: "Vandal".to_string(),
            display_icon: None,
        }]);

        let summary = LoadoutSummary::from_response(response, &catalog, &weapons);

        assert_eq!(summary.gun_skins[0].label(), "Vandal: Prime Vandal");
    }

    #[test]
    fn non_empty_path_trims_input() {
        assert_eq!(
            non_empty_path(r"  C:\Riot Games\Riot Client\RiotClientServices.exe  "),
            Some(PathBuf::from(
                r"C:\Riot Games\Riot Client\RiotClientServices.exe"
            ))
        );
        assert_eq!(non_empty_path("   "), None);
    }

    #[test]
    fn require_launcher_session_rejects_missing_backup() {
        let err = require_launcher_session(None).expect_err("missing backup");

        assert!(err.contains("captured launcher session"));
    }

    #[test]
    fn require_launcher_session_rejects_missing_backup_folder() {
        let err = require_launcher_session(Some(LauncherSessionBackup {
            data_dir: PathBuf::from("missing-launcher-backup"),
            captured_at_unix: 100,
            puuid: "puuid".to_string(),
        }))
        .expect_err("missing backup folder");

        assert!(err.contains("backup folder is missing"));
    }

    #[test]
    fn require_launcher_session_accepts_ready_backup() {
        let dir = tempdir().expect("temp dir");
        let backup = LauncherSessionBackup {
            data_dir: dir.path().to_path_buf(),
            captured_at_unix: 100,
            puuid: "puuid".to_string(),
        };

        let accepted = require_launcher_session(Some(backup)).expect("ready backup");

        assert_eq!(accepted.puuid, "puuid");
    }

    #[test]
    fn cache_account_session_updates_matching_account() {
        let mut state = StoredState::default();
        let account = AccountProfile::new("Main", None, Shard::Na).expect("account");
        let account_id = account.id;
        state.push_account(account);
        let session = AuthSession::new(
            "access",
            None,
            Some("entitlement".to_string()),
            "Bearer",
            Some(3600),
            100,
        );

        assert!(cache_account_session(
            &mut state,
            account_id,
            session.clone()
        ));
        assert_eq!(state.accounts[0].session, Some(session));
    }

    #[test]
    fn cache_account_session_ignores_missing_account() {
        let mut state = StoredState::default();
        let session = AuthSession::new("access", None, None, "Bearer", Some(3600), 100);

        assert!(!cache_account_session(
            &mut state,
            AccountId::new(),
            session
        ));
    }
}
