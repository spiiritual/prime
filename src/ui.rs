use std::path::PathBuf;
use std::time::Duration;

use iced::widget::image::Handle;
use iced::widget::{
    button, column, container, grid, image, pick_list, row, scrollable, stack, text, text_input,
};
use iced::{Color, ContentFit, Element, Length, Padding, Subscription, Task, Theme, alignment};

use crate::account::LauncherSessionBackup;
use crate::account::{AccountId, AccountProfile, AuthSession, Shard};
use crate::image_cache::ImageCache;
use crate::launch::{
    LaunchConfig, close_riot_processes, launch_riot_login_capture, launch_valorant,
};
use crate::riot::auth::parse_redirect_tokens;
use crate::riot::client::{ApiCredentials, RiotApi};
use crate::riot::content::{
    BundleCatalog, CurrencyCatalog, ResolvedBundle, ResolvedCurrency, ResolvedSkin, ResolvedWeapon,
    SkinCatalog, ValorantContentApi, WeaponCatalog,
};
use crate::riot::launcher_session::{
    CapturedLauncherSession, LauncherSessionError, apply_launcher_session_backup,
    capture_current_launcher_session, clear_existing_launcher_data_dirs, launcher_cookie_header,
    read_backup_cookies,
};
use crate::riot::models::{
    BonusStoreOffer, PlayerInfoResponse, PlayerLoadoutResponse, StoreBundle, StoreOffer,
    StorefrontResponse,
};
use crate::storage::{AccountRepository, StoredState};

pub fn run() -> iced::Result {
    iced::application(PrimeApp::boot, PrimeApp::update, PrimeApp::view)
        .title(app_title)
        .theme(app_theme)
        .subscription(app_subscription)
        .window_size((1280.0, 840.0))
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
        iced::time::every(SHOP_RESET_CHECK_INTERVAL).map(Message::ShopTimerTick)
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
    pending_account: Option<CapturedAccountDraft>,
    store_summary: Option<StoreSummary>,
    loadout_summary: Option<LoadoutSummary>,
    store_loading: bool,
    loadout_loading: bool,
    image_cache_size_bytes: u64,
    now: iced::time::Instant,
}

impl PrimeApp {
    fn boot() -> (Self, Task<Message>) {
        let repo = AccountRepository::new(AccountRepository::default_path());
        let image_cache = ImageCache::new(ImageCache::default_path());
        let load_repo = repo.clone();
        let cache_for_size = image_cache.clone();

        (
            Self {
                repo,
                image_cache,
                state: StoredState::default(),
                active_tab: Tab::Accounts,
                new_display_name: String::new(),
                new_username: String::new(),
                new_shard: Shard::Na,
                redirect_input: String::new(),
                client_version_input: String::new(),
                riot_client_path_input: String::new(),
                status: "Loading accounts".to_string(),
                pending_account: None,
                store_summary: None,
                loadout_summary: None,
                store_loading: false,
                loadout_loading: false,
                image_cache_size_bytes: 0,
                now: iced::time::Instant::now(),
            },
            Task::batch([
                Task::perform(
                    async move { load_repo.load().map_err(|error| error.to_string()) },
                    Message::Loaded,
                ),
                Task::perform(fetch_current_client_version(), Message::ClientVersionLoaded),
                Task::perform(
                    async move {
                        cache_for_size
                            .size_bytes()
                            .map_err(|error| error.to_string())
                    },
                    Message::ImageCacheSizeLoaded,
                ),
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
                self.load_active_tab()
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
                Task::batch([self.save_task(), self.load_active_tab()])
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
                let account_id = AccountId::new();
                let config = LaunchConfig {
                    riot_client_path: self.state.riot_client_path.clone(),
                    ..LaunchConfig::default()
                };
                let backup_root = self.repo.launcher_backups_dir();
                self.pending_account = None;
                self.new_display_name.clear();
                self.new_username.clear();
                self.status =
                    "Opening Riot Client. Log in with Remember Me enabled to add the account."
                        .to_string();

                Task::perform(
                    async move { start_account_capture(account_id, backup_root, config).await },
                    Message::AccountCaptureFinished,
                )
            }
            Message::AccountCaptureFinished(result) => {
                match result {
                    Ok(draft) => {
                        self.new_display_name = draft
                            .game_name
                            .clone()
                            .unwrap_or_else(|| "New account".to_string());
                        self.new_username = draft.riot_id().unwrap_or_else(|| draft.puuid.clone());
                        self.new_shard = draft.shard;
                        self.status = draft.identity_warning.clone().unwrap_or_else(|| {
                            "Captured login. Confirm the account details to save it.".to_string()
                        });
                        self.pending_account = Some(draft);
                    }
                    Err(error) => {
                        self.status = format!("Could not add account: {error}");
                    }
                }

                Task::none()
            }
            Message::ConfirmCapturedAccount => {
                let Some(draft) = self.pending_account.clone() else {
                    self.status = "No captured account is waiting to be saved".to_string();
                    return Task::none();
                };

                match AccountProfile::new(
                    self.new_display_name.clone(),
                    Some(self.new_username.clone()),
                    self.new_shard,
                ) {
                    Ok(mut account) => {
                        account.id = draft.account_id;
                        account.shard = self.new_shard;
                        account.session = draft.session;

                        if let Err(error) = account.attach_launcher_session(draft.backup) {
                            self.status = format!("Captured account rejected: {error}");
                            return Task::none();
                        }

                        if let (Some(game_name), Some(tag_line)) = (draft.game_name, draft.tag_line)
                            && let Err(error) =
                                account.apply_riot_identity(draft.puuid, game_name, tag_line)
                        {
                            self.status = format!("Captured identity rejected: {error}");
                            return Task::none();
                        }

                        self.status = format!("Added {}", account.summary());
                        self.state.push_account(account);
                        self.state.selected_account = Some(draft.account_id);
                        self.pending_account = None;
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
            Message::CancelCapturedAccount => {
                self.pending_account = None;
                self.new_display_name.clear();
                self.new_username.clear();
                self.status = "Discarded captured account draft".to_string();
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
                let Some(account_id) = self.state.selected_account else {
                    self.status =
                        "Select an account before starting launcher session capture".to_string();
                    return Task::none();
                };

                let config = LaunchConfig {
                    riot_client_path: self.state.riot_client_path.clone(),
                    ..LaunchConfig::default()
                };
                let backup_root = self.repo.launcher_backups_dir();
                self.status =
                    "Opening Riot Client and waiting for remembered login capture".to_string();

                Task::perform(
                    async move { start_launcher_session_login(account_id, backup_root, config).await },
                    Message::LauncherSessionLoginStarted,
                )
            }
            Message::LauncherSessionLoginStarted(result) => {
                match result {
                    Ok(captured) => {
                        return self.store_captured_launcher_session(captured);
                    }
                    Err(error) => {
                        self.status = format!("Could not complete launcher session login: {error}");
                    }
                }

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
            Message::StorefrontLoaded(result) => {
                self.store_loading = false;
                self.now = iced::time::Instant::now();

                match result {
                    Ok(result) => {
                        if let Err(error) = cache_account_api_context(
                            &mut self.state,
                            result.account_id,
                            result.session,
                            result.identity,
                        ) {
                            self.status =
                                format!("Store loaded, but profile update failed: {error}");
                            return Task::none();
                        }

                        let bundle_count = result.summary.featured_bundles.len();
                        let daily_count = result.summary.daily_offers.len();
                        let night_market_count = result.summary.night_market_offers.len();

                        self.status = format!(
                            "Loaded {} featured bundle(s), {} daily offer(s), and {} night market offer(s)",
                            bundle_count, daily_count, night_market_count
                        );
                        if self.state.selected_account == Some(result.account_id) {
                            self.store_summary = Some(result.summary);
                        }

                        return Task::batch([self.save_task(), self.image_cache_size_task()]);
                    }
                    Err(error) => {
                        self.status = format!("Store check failed: {error}");
                    }
                }

                Task::none()
            }
            Message::ShopTimerTick(now) => {
                self.now = now;

                if self.store_loading {
                    return Task::none();
                }

                if self
                    .store_summary
                    .as_ref()
                    .is_some_and(|summary| summary.is_expired_at(now))
                {
                    self.store_summary = None;
                    self.status = "Shop reset reached; loading updated shop".to_string();
                    return self.fetch_storefront_task();
                }

                Task::none()
            }
            Message::LoadoutLoaded(result) => {
                self.loadout_loading = false;

                match result {
                    Ok(result) => {
                        if let Err(error) = cache_account_api_context(
                            &mut self.state,
                            result.account_id,
                            result.session,
                            result.identity,
                        ) {
                            self.status =
                                format!("Loadout loaded, but profile update failed: {error}");
                            return Task::none();
                        }

                        let gun_count = result.summary.gun_skins.len();

                        self.status = format!("Loaded loadout with {} gun skin(s)", gun_count);
                        if self.state.selected_account == Some(result.account_id) {
                            self.loadout_summary = Some(result.summary);
                        }

                        return Task::batch([self.save_task(), self.image_cache_size_task()]);
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
            Message::ImageCacheSizeLoaded(result) => {
                match result {
                    Ok(size) => {
                        self.image_cache_size_bytes = size;
                    }
                    Err(error) => {
                        self.status = format!("Could not read image cache size: {error}");
                    }
                }

                Task::none()
            }
            Message::ClearImageCache => {
                let cache = self.image_cache.clone();
                self.status = "Clearing image cache".to_string();
                Task::perform(
                    async move { cache.clear().map_err(|error| error.to_string()) },
                    Message::ImageCacheCleared,
                )
            }
            Message::ImageCacheCleared(result) => {
                match result {
                    Ok(()) => {
                        self.image_cache_size_bytes = 0;
                        self.status = "Cleared image cache".to_string();
                    }
                    Err(error) => {
                        self.status = format!("Could not clear image cache: {error}");
                    }
                }

                Task::none()
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
                .padding(22)
                .width(Length::Fill)
                .height(Length::Fill)
        ]
        .height(Length::Fill);

        container(content)
            .padding(Padding::ZERO.right(14))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let mut accounts = column![text("Prime").size(26), text("Profiles").size(16)].spacing(8);

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
            text("Navigate").size(16),
            self.tab_button(Tab::Accounts),
            self.tab_button(Tab::Shop),
            self.tab_button(Tab::Loadout),
            self.tab_button(Tab::Settings),
        ]
        .spacing(8);

        container(scrollable(column![accounts, tabs].spacing(16)))
            .padding(16)
            .width(280)
            .height(Length::Fill)
            .style(iced::widget::container::dark)
            .into()
    }

    fn main_panel(&self) -> Element<'_, Message> {
        let body = match self.active_tab {
            Tab::Accounts => self.accounts_tab(),
            Tab::Shop => self.shop_tab(),
            Tab::Loadout => self.loadout_tab(),
            Tab::Settings => self.settings_tab(),
        };
        let scroll_body = container(body)
            .padding(Padding::ZERO.right(18))
            .width(Length::Fill);

        column![
            container(text(self.active_tab.to_string()).size(30))
                .padding(14)
                .width(Length::Fill)
                .style(iced::widget::container::bordered_box),
            container(scrollable(scroll_body))
                .padding(16)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(iced::widget::container::rounded_box),
            container(text(&self.status))
                .padding(10)
                .width(Length::Fill)
                .style(iced::widget::container::bordered_box)
        ]
        .spacing(12)
        .into()
    }

    fn tab_button(&self, tab: Tab) -> Element<'_, Message> {
        let label = if self.active_tab == tab {
            format!("[{}]", tab)
        } else {
            tab.to_string()
        };

        button(text(label))
            .width(Length::Fill)
            .on_press(Message::TabSelected(tab))
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

        let mut content = column![
            text(format!("Selected: {selected}")),
            text(launcher_session),
            row![
                button("Add account").on_press(Message::AddAccount),
                button("Remove selected").on_press(Message::DeleteSelected),
                button("Re-capture selected login").on_press(Message::StartLauncherSessionLogin),
                button("Refresh profile").on_press(Message::RefreshProfileIdentity),
                button("Launch VALORANT").on_press(Message::LaunchSelected)
            ]
            .spacing(10),
            text("Add account opens Riot Client, waits for a remembered login, then asks you to confirm the profile details.")
        ]
        .spacing(12);

        if let Some(draft) = &self.pending_account {
            content = content.push(
                container(
                    column![
                        text("Confirm captured account").size(22),
                        text(format!("PUUID: {}", draft.puuid)),
                        row![
                            text_input("Display name", &self.new_display_name)
                                .on_input(Message::NewDisplayNameChanged)
                                .width(Length::Fill),
                            text_input("Riot username", &self.new_username)
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
                            button("Save account").on_press(Message::ConfirmCapturedAccount),
                            button("Cancel").on_press(Message::CancelCapturedAccount)
                        ]
                        .spacing(10)
                    ]
                    .spacing(10),
                )
                .padding(16)
                .style(iced::widget::container::bordered_box),
            );
        }

        content.into()
    }

    fn token_import_controls(&self) -> Element<'_, Message> {
        column![
            text("Advanced API token import"),
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
        let loading_label = if self.store_loading {
            "Loading shop..."
        } else {
            "Shop loads automatically and updates when the reset timer expires."
        };
        let mut content = column![text(loading_label)].spacing(12).width(Length::Fill);

        if let Some(summary) = &self.store_summary {
            content = content
                .push(text(format!(
                    "Featured bundles expire in {}",
                    format_duration(summary.bundle_remaining_seconds_at(self.now))
                )))
                .push(bundle_row(&summary.featured_bundles))
                .push(text(format!(
                    "Daily offers reset in {}",
                    format_duration(summary.daily_remaining_seconds_at(self.now))
                )))
                .push(offer_row(&summary.daily_offers));

            if !summary.night_market_offers.is_empty() {
                content = content
                    .push(text(format!(
                        "Night Market expires in {}",
                        format_duration(summary.night_market_remaining_seconds_at(self.now))
                    )))
                    .push(offer_row(&summary.night_market_offers));
            }
        }

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn loadout_tab(&self) -> Element<'_, Message> {
        let loading_label = if self.loadout_loading {
            "Loading loadout..."
        } else {
            "Loadout loads automatically for the selected account."
        };
        let mut content = column![text(loading_label)].spacing(12).width(Length::Fill);

        if let Some(summary) = &self.loadout_summary {
            for category in [
                "Sidearms",
                "SMGs",
                "Shotguns",
                "Rifles",
                "Sniper Rifles",
                "Heavy",
                "Melee",
                "Other",
            ] {
                if let Some(section) = loadout_section(
                    category,
                    summary
                        .gun_skins
                        .iter()
                        .filter(|gun| weapon_category(&gun.weapon.display_name) == category),
                ) {
                    content = content.push(section);
                }
            }
        }

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn settings_tab(&self) -> Element<'_, Message> {
        column![
            text(format!("Profile storage: {}", self.repo.path().display())),
            text_input(
                r"C:\Riot Games\Riot Client\RiotClientServices.exe",
                &self.riot_client_path_input
            )
            .on_input(Message::RiotClientPathChanged),
            button("Save settings").on_press(Message::SaveSettings),
            text(format!(
                "Image cache: {}",
                format_bytes(self.image_cache_size_bytes)
            )),
            text(format!(
                "Image cache folder: {}",
                self.image_cache.path().display()
            )),
            button("Delete image cache").on_press(Message::ClearImageCache),
            self.token_import_controls()
        ]
        .spacing(12)
        .into()
    }

    fn load_active_tab(&mut self) -> Task<Message> {
        match self.active_tab {
            Tab::Shop if self.store_summary.is_none() && !self.store_loading => {
                self.fetch_storefront_task()
            }
            Tab::Loadout if self.loadout_summary.is_none() && !self.loadout_loading => {
                self.fetch_loadout_task()
            }
            _ => Task::none(),
        }
    }

    fn fetch_storefront_task(&mut self) -> Task<Message> {
        let Some(account) = self.state.selected_account().cloned() else {
            self.status = "Select an account before opening the shop".to_string();
            return Task::none();
        };

        self.store_loading = true;
        self.status = "Loading shop".to_string();
        let image_cache = self.image_cache.clone();
        Task::perform(
            fetch_storefront(account, self.client_version_input.clone(), image_cache),
            Message::StorefrontLoaded,
        )
    }

    fn fetch_loadout_task(&mut self) -> Task<Message> {
        let Some(account) = self.state.selected_account().cloned() else {
            self.status = "Select an account before opening loadout".to_string();
            return Task::none();
        };

        self.loadout_loading = true;
        self.status = "Loading loadout".to_string();
        let image_cache = self.image_cache.clone();
        Task::perform(
            fetch_loadout(account, self.client_version_input.clone(), image_cache),
            Message::LoadoutLoaded,
        )
    }

    fn save_task(&self) -> Task<Message> {
        let repo = self.repo.clone();
        let state = self.state.clone();

        Task::perform(
            async move { repo.save(&state).map_err(|error| error.to_string()) },
            Message::Saved,
        )
    }

    fn image_cache_size_task(&self) -> Task<Message> {
        let cache = self.image_cache.clone();

        Task::perform(
            async move { cache.size_bytes().map_err(|error| error.to_string()) },
            Message::ImageCacheSizeLoaded,
        )
    }

    fn store_captured_launcher_session(
        &mut self,
        captured: CapturedLauncherSession,
    ) -> Task<Message> {
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

            self.status =
                format!("Captured launcher session for selected account ({captured_puuid})");
            return self.save_task();
        }

        self.status =
            "Captured launcher session, but the selected profile no longer exists".to_string();
        Task::none()
    }
}

fn offer_row<'a>(offers: &'a [StoreOfferDisplay]) -> Element<'a, Message> {
    if offers.is_empty() {
        return text("No offers available").into();
    }

    let mut cards = iced::widget::Row::new().spacing(10).width(Length::Fill);

    for offer in offers {
        cards = cards.push(store_offer_card(offer));
    }

    cards.into()
}

fn bundle_row<'a>(bundles: &'a [StoreBundleDisplay]) -> Element<'a, Message> {
    if bundles.is_empty() {
        return text("No featured bundles available").into();
    }

    let mut cards = iced::widget::Row::new().spacing(12).width(Length::Fill);

    for bundle in bundles {
        cards = cards.push(store_bundle_card(bundle));
    }

    cards.into()
}

fn store_bundle_card(bundle: &StoreBundleDisplay) -> Element<'_, Message> {
    let price = bundle
        .price
        .as_ref()
        .map(OfferPrice::label)
        .unwrap_or_else(|| "Price unavailable".to_string());
    let rarity_for_style = bundle.rarity.clone();
    let details = column![
        text(&bundle.bundle.display_name).size(20),
        text(price).size(16),
        text(bundle.item_count_label()).size(14),
    ]
    .spacing(5)
    .width(Length::Fill);
    let overlay = container(
        container(details)
            .padding([10, 12])
            .width(Length::Fill)
            .style(bundle_text_scrim_style),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_y(alignment::Vertical::Bottom);

    container(
        stack![
            asset_background_image(bundle.bundle.cached_icon.as_ref(), 214.0),
            overlay
        ]
        .width(Length::Fill)
        .height(214.0)
        .clip(true),
    )
    .width(Length::Fill)
    .height(214.0)
    .clip(true)
    .style(move |theme| rarity_card_style(theme, rarity_for_style.as_deref()))
    .into()
}

fn store_offer_card(offer: &StoreOfferDisplay) -> Element<'_, Message> {
    let price = offer
        .price
        .as_ref()
        .map(OfferPrice::label)
        .unwrap_or_else(|| "Price unavailable".to_string());
    let rarity_for_style = offer.skin.rarity.clone();
    let mut details = iced::widget::Column::new()
        .spacing(6)
        .push(asset_image(offer.skin.cached_icon.as_ref(), 118.0))
        .push(text(&offer.skin.display_name).size(16))
        .push(text(price).size(14));

    if offer.discount_percent > 0 {
        details = details.push(text(format!("{}% off", offer.discount_percent)).size(13));
    }

    container(details)
        .padding(10)
        .width(Length::Fill)
        .style(move |theme| rarity_card_style(theme, rarity_for_style.as_deref()))
        .into()
}

fn loadout_section<'a>(
    category: &'static str,
    guns: impl IntoIterator<Item = &'a LoadoutGunDisplay>,
) -> Option<Element<'a, Message>> {
    let mut cards = grid::Grid::new()
        .spacing(12)
        .fluid(220)
        .height(grid::aspect_ratio(220, 252));
    let mut count = 0;

    for gun in guns {
        cards = cards.push(loadout_card(gun));
        count += 1;
    }

    (count > 0).then(|| column![text(category).size(20), cards].spacing(8).into())
}

fn loadout_card(gun: &LoadoutGunDisplay) -> Element<'_, Message> {
    container(
        column![
            asset_image(gun.skin.cached_icon.as_ref(), 164.0),
            text(&gun.weapon.display_name).size(15).width(Length::Fill),
            text(&gun.skin.display_name).size(14).width(Length::Fill)
        ]
        .spacing(6),
    )
    .padding(10)
    .width(Length::Fill)
    .style(iced::widget::container::bordered_box)
    .into()
}

fn rarity_card_style(theme: &Theme, rarity: Option<&str>) -> iced::widget::container::Style {
    let mut style = iced::widget::container::bordered_box(theme);

    if let Some((background, border)) = rarity_colors(rarity) {
        style.background = Some(background.into());
        style.border.color = border;
    }

    style
}

fn bundle_text_scrim_style(_: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(Color::from_rgba8(8, 10, 14, 0.78).into()),
        text_color: Some(Color::WHITE),
        ..Default::default()
    }
}

fn rarity_colors(rarity: Option<&str>) -> Option<(Color, Color)> {
    let rarity = rarity?.to_ascii_lowercase();

    if rarity.contains("exclusive") {
        Some((
            Color::from_rgba8(86, 42, 42, 0.72),
            Color::from_rgb8(214, 92, 92),
        ))
    } else if rarity.contains("ultra") {
        Some((
            Color::from_rgba8(78, 58, 32, 0.72),
            Color::from_rgb8(218, 154, 72),
        ))
    } else if rarity.contains("premium") {
        Some((
            Color::from_rgba8(58, 48, 82, 0.72),
            Color::from_rgb8(166, 132, 224),
        ))
    } else if rarity.contains("deluxe") {
        Some((
            Color::from_rgba8(34, 55, 82, 0.72),
            Color::from_rgb8(91, 157, 218),
        ))
    } else if rarity.contains("select") {
        Some((
            Color::from_rgba8(32, 68, 55, 0.72),
            Color::from_rgb8(86, 184, 139),
        ))
    } else {
        None
    }
}

fn asset_image(path: Option<&PathBuf>, height: f32) -> Element<'_, Message> {
    match path {
        Some(path) => image(Handle::from_path(path.clone()))
            .width(Length::Fill)
            .height(height)
            .content_fit(ContentFit::Contain)
            .into(),
        None => container(text("No image").size(13))
            .width(Length::Fill)
            .height(height)
            .style(iced::widget::container::rounded_box)
            .into(),
    }
}

fn asset_background_image(path: Option<&PathBuf>, height: f32) -> Element<'_, Message> {
    match path {
        Some(path) => image(Handle::from_path(path.clone()))
            .width(Length::Fill)
            .height(height)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(text("No image").size(13))
            .width(Length::Fill)
            .height(height)
            .style(iced::widget::container::rounded_box)
            .into(),
    }
}

fn format_duration(seconds: i64) -> String {
    if seconds <= 0 {
        return "soon".to_string();
    }

    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;

    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
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
    AccountCaptureFinished(Result<CapturedAccountDraft, String>),
    ConfirmCapturedAccount,
    CancelCapturedAccount,
    DeleteSelected,
    RedirectChanged(String),
    ClientVersionChanged(String),
    RefreshClientVersion,
    ClientVersionLoaded(Result<String, String>),
    ImportRedirect,
    StartLauncherSessionLogin,
    LauncherSessionLoginStarted(Result<CapturedLauncherSession, String>),
    RefreshProfileIdentity,
    ProfileIdentityLoaded(Result<RefreshedProfileIdentity, String>),
    StorefrontLoaded(Result<StorefrontResult, String>),
    ShopTimerTick(iced::time::Instant),
    LoadoutLoaded(Result<LoadoutResult, String>),
    RiotClientPathChanged(String),
    SaveSettings,
    ImageCacheSizeLoaded(Result<u64, String>),
    ClearImageCache,
    ImageCacheCleared(Result<(), String>),
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

const LOGIN_CAPTURE_TIMEOUT: Duration = Duration::from_secs(600);
const LOGIN_CAPTURE_POLL_INTERVAL: Duration = Duration::from_secs(2);
const SHOP_RESET_CHECK_INTERVAL: Duration = Duration::from_secs(1);

async fn start_launcher_session_login(
    account_id: AccountId,
    backup_root: PathBuf,
    config: LaunchConfig,
) -> Result<CapturedLauncherSession, String> {
    close_riot_processes().map_err(|error| error.to_string())?;
    clear_existing_launcher_data_dirs().map_err(|error| error.to_string())?;
    launch_riot_login_capture(&config).map_err(|error| error.to_string())?;
    wait_for_launcher_session_capture(
        account_id,
        backup_root,
        LOGIN_CAPTURE_TIMEOUT,
        LOGIN_CAPTURE_POLL_INTERVAL,
    )
    .await
}

async fn wait_for_launcher_session_capture(
    account_id: AccountId,
    backup_root: PathBuf,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<CapturedLauncherSession, String> {
    let started = std::time::Instant::now();

    while started.elapsed() < timeout {
        match capture_current_launcher_session(account_id, &backup_root) {
            Ok(captured) => return Ok(captured),
            Err(error) if is_pending_launcher_capture_error(&error) => {
                tokio::time::sleep(poll_interval).await;
            }
            Err(error) => return Err(error.to_string()),
        }
    }

    Err(
        "timed out waiting for Riot Client remembered login; make sure Remember Me is enabled"
            .to_string(),
    )
}

fn is_pending_launcher_capture_error(error: &LauncherSessionError) -> bool {
    matches!(error, LauncherSessionError::PrivateSettingsNotFound)
}

async fn resolve_session_shard(
    api: &RiotApi,
    session: &AuthSession,
    player_info: Option<&PlayerInfoResponse>,
    fallback: Shard,
) -> Shard {
    if let Some(shard) = player_info.and_then(shard_from_player_affinities) {
        return shard;
    }

    let Some(id_token) = session.id_token.as_ref().filter(|token| !token.is_empty()) else {
        return fallback;
    };

    api.riot_geo(&session.access_token, id_token)
        .await
        .ok()
        .and_then(|geo| Shard::from_live_affinity(&geo.affinities.live))
        .unwrap_or(fallback)
}

fn shard_from_player_affinities(player_info: &PlayerInfoResponse) -> Option<Shard> {
    ["live", "pp", "pvp"]
        .into_iter()
        .filter_map(|key| player_info.affinity.get(key))
        .find_map(|value| Shard::from_live_affinity(value))
}

async fn start_account_capture(
    account_id: AccountId,
    backup_root: PathBuf,
    config: LaunchConfig,
) -> Result<CapturedAccountDraft, String> {
    let captured = start_launcher_session_login(account_id, backup_root, config).await?;
    Ok(enrich_captured_account(captured).await)
}

async fn enrich_captured_account(captured: CapturedLauncherSession) -> CapturedAccountDraft {
    let mut draft = CapturedAccountDraft::new(captured.account_id, captured.backup);
    let Err(error) = enrich_captured_account_identity(&mut draft).await else {
        return draft;
    };

    draft.identity_warning = Some(format!(
        "Captured login, but Riot identity lookup failed: {error}. Confirm the account details manually."
    ));
    draft
}

async fn enrich_captured_account_identity(draft: &mut CapturedAccountDraft) -> Result<(), String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let cookies = read_backup_cookies(&draft.backup).map_err(|error| error.to_string())?;
    let cookie_header = launcher_cookie_header(&cookies).map_err(|error| error.to_string())?;
    let mut session = api
        .cookie_reauth(&cookie_header)
        .await
        .map(|tokens| tokens.into_session())
        .map_err(|error| error.to_string())?;
    let player_info = api
        .player_info(&session.access_token)
        .await
        .map_err(|error| error.to_string())?;

    draft.puuid = player_info.sub.clone();
    draft.game_name = Some(player_info.acct.game_name.clone());
    draft.tag_line = Some(player_info.acct.tag_line.clone());
    draft.shard = resolve_session_shard(&api, &session, Some(&player_info), draft.shard).await;

    if session
        .entitlements_token
        .as_ref()
        .is_none_or(|token| token.trim().is_empty())
        && let Ok(entitlement) = api.entitlement(&session.access_token).await
    {
        session.entitlements_token = Some(entitlement.entitlements_token);
    }

    draft.session = Some(session);
    Ok(())
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
    identity: ApiIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LoadoutResult {
    account_id: AccountId,
    summary: LoadoutSummary,
    session: AuthSession,
    identity: ApiIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CapturedAccountDraft {
    account_id: AccountId,
    backup: LauncherSessionBackup,
    puuid: String,
    game_name: Option<String>,
    tag_line: Option<String>,
    shard: Shard,
    session: Option<AuthSession>,
    identity_warning: Option<String>,
}

impl CapturedAccountDraft {
    fn new(account_id: AccountId, backup: LauncherSessionBackup) -> Self {
        let puuid = backup.puuid.clone();

        Self {
            account_id,
            backup,
            puuid,
            game_name: None,
            tag_line: None,
            shard: Shard::default(),
            session: None,
            identity_warning: None,
        }
    }

    fn riot_id(&self) -> Option<String> {
        match (&self.game_name, &self.tag_line) {
            (Some(game_name), Some(tag_line)) if !game_name.is_empty() && !tag_line.is_empty() => {
                Some(format!("{game_name}#{tag_line}"))
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ApiIdentity {
    puuid: String,
    game_name: Option<String>,
    tag_line: Option<String>,
    shard: Shard,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StoreSummary {
    featured_bundles: Vec<StoreBundleDisplay>,
    daily_offers: Vec<StoreOfferDisplay>,
    daily_remaining_seconds: i64,
    bundle_remaining_seconds: i64,
    night_market_remaining_seconds: Option<i64>,
    loaded_at: iced::time::Instant,
    night_market_offers: Vec<StoreOfferDisplay>,
}

impl StoreSummary {
    fn from_response(
        response: StorefrontResponse,
        skins: &SkinCatalog,
        bundles: &BundleCatalog,
        currencies: &CurrencyCatalog,
    ) -> Self {
        Self::from_response_at(
            response,
            skins,
            bundles,
            currencies,
            iced::time::Instant::now(),
        )
    }

    fn from_response_at(
        response: StorefrontResponse,
        skins: &SkinCatalog,
        bundles: &BundleCatalog,
        currencies: &CurrencyCatalog,
        loaded_at: iced::time::Instant,
    ) -> Self {
        let featured_bundles = if response.featured_bundle.bundles.is_empty() {
            std::iter::once(&response.featured_bundle.bundle)
                .map(|bundle| store_bundle_display(bundle, skins, bundles, currencies))
                .collect()
        } else {
            response
                .featured_bundle
                .bundles
                .iter()
                .map(|bundle| store_bundle_display(bundle, skins, bundles, currencies))
                .collect()
        };
        let night_market_remaining_seconds = response
            .bonus_store
            .as_ref()
            .map(|store| store.bonus_store_remaining_duration_in_seconds);
        let night_market_offers = response
            .bonus_store
            .as_ref()
            .map(|store| {
                store
                    .bonus_store_offers
                    .iter()
                    .map(|offer| bonus_store_offer_display(offer, skins, currencies))
                    .collect()
            })
            .unwrap_or_default();
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

        Self {
            featured_bundles,
            daily_offers,
            daily_remaining_seconds: response
                .skins_panel_layout
                .single_item_offers_remaining_duration_in_seconds,
            bundle_remaining_seconds: response
                .featured_bundle
                .bundle_remaining_duration_in_seconds,
            night_market_remaining_seconds,
            loaded_at,
            night_market_offers,
        }
    }

    fn daily_remaining_seconds_at(&self, now: iced::time::Instant) -> i64 {
        remaining_seconds_at(self.daily_remaining_seconds, self.loaded_at, now)
    }

    fn bundle_remaining_seconds_at(&self, now: iced::time::Instant) -> i64 {
        remaining_seconds_at(self.bundle_remaining_seconds, self.loaded_at, now)
    }

    fn night_market_remaining_seconds_at(&self, now: iced::time::Instant) -> i64 {
        self.night_market_remaining_seconds
            .map(|seconds| remaining_seconds_at(seconds, self.loaded_at, now))
            .unwrap_or(0)
    }

    fn is_expired_at(&self, now: iced::time::Instant) -> bool {
        let section_expired =
            self.daily_remaining_seconds_at(now) == 0 || self.bundle_remaining_seconds_at(now) == 0;
        let night_market_expired = self
            .night_market_remaining_seconds
            .is_some_and(|_| self.night_market_remaining_seconds_at(now) == 0);

        section_expired || night_market_expired
    }
}

fn remaining_seconds_at(
    original_seconds: i64,
    loaded_at: iced::time::Instant,
    now: iced::time::Instant,
) -> i64 {
    let elapsed_seconds = now
        .checked_duration_since(loaded_at)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0);

    original_seconds.saturating_sub(elapsed_seconds)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StoreOfferDisplay {
    skin: SkinDisplay,
    price: Option<OfferPrice>,
    discount_percent: i64,
}

impl StoreOfferDisplay {
    #[cfg(test)]
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
struct StoreBundleDisplay {
    bundle: BundleDisplay,
    price: Option<OfferPrice>,
    item_count: i64,
    rarity: Option<String>,
}

impl StoreBundleDisplay {
    fn item_count_label(&self) -> String {
        match self.item_count {
            1 => "1 item".to_string(),
            count => format!("{count} items"),
        }
    }

    #[cfg(test)]
    fn label(&self) -> String {
        let mut label = self.bundle.display_name.clone();

        if let Some(price) = &self.price {
            label.push_str(&format!(" ({})", price.label()));
        }

        label.push_str(&format!(", {}", self.item_count_label()));
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct BundleDisplay {
    uuid: String,
    display_name: String,
    display_icon: Option<String>,
    cached_icon: Option<PathBuf>,
}

impl From<ResolvedBundle> for BundleDisplay {
    fn from(bundle: ResolvedBundle) -> Self {
        Self {
            uuid: bundle.uuid,
            display_name: bundle.display_name,
            display_icon: bundle.display_icon,
            cached_icon: None,
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

fn store_bundle_display(
    bundle: &StoreBundle,
    skins: &SkinCatalog,
    bundles: &BundleCatalog,
    currencies: &CurrencyCatalog,
) -> StoreBundleDisplay {
    let direct = bundles.resolve(&bundle.data_asset_id);
    let resolved = if direct.display_name != bundle.data_asset_id {
        direct
    } else {
        bundles.resolve(&bundle.id)
    };
    let rarity = strongest_bundle_rarity(bundle, skins);
    let item_count = bundle
        .items
        .iter()
        .map(|item| item.item.amount.max(1))
        .sum::<i64>();

    StoreBundleDisplay {
        bundle: BundleDisplay::from(resolved),
        price: bundle_price(bundle, currencies),
        item_count,
        rarity,
    }
}

fn bundle_price(bundle: &StoreBundle, currencies: &CurrencyCatalog) -> Option<OfferPrice> {
    bundle
        .total_discounted_cost
        .as_ref()
        .and_then(|costs| offer_price(costs, currencies))
        .or_else(|| {
            bundle
                .total_base_cost
                .as_ref()
                .and_then(|costs| offer_price(costs, currencies))
        })
        .or_else(|| summed_bundle_item_price(bundle, currencies))
}

fn summed_bundle_item_price(
    bundle: &StoreBundle,
    currencies: &CurrencyCatalog,
) -> Option<OfferPrice> {
    let currency_id = bundle
        .currency_id
        .trim()
        .is_empty()
        .then(|| bundle.items.first().map(|item| item.currency_id.as_str()))
        .flatten()
        .unwrap_or(bundle.currency_id.as_str());

    if currency_id.trim().is_empty() || bundle.items.is_empty() {
        return None;
    }

    let amount = bundle
        .items
        .iter()
        .filter(|item| item.currency_id.eq_ignore_ascii_case(currency_id))
        .map(|item| {
            if item.discounted_price > 0 {
                item.discounted_price
            } else {
                item.base_price
            }
        })
        .sum();

    Some(OfferPrice {
        amount,
        currency: CurrencyDisplay::from(currencies.resolve(currency_id)),
    })
}

fn strongest_bundle_rarity(bundle: &StoreBundle, skins: &SkinCatalog) -> Option<String> {
    bundle
        .items
        .iter()
        .filter_map(|item| skins.resolve(&item.item.item_id).rarity)
        .max_by_key(|rarity| rarity_rank(rarity))
}

fn rarity_rank(rarity: &str) -> usize {
    let rarity = rarity.to_ascii_lowercase();

    if rarity.contains("exclusive") {
        5
    } else if rarity.contains("ultra") {
        4
    } else if rarity.contains("premium") {
        3
    } else if rarity.contains("deluxe") {
        2
    } else if rarity.contains("select") {
        1
    } else {
        0
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
        account_level: Option<i64>,
    ) -> Self {
        let mut gun_skins = response
            .guns
            .into_iter()
            .map(|gun| {
                let weapon = WeaponDisplay::from(weapons.resolve(&gun.id));
                let skin = SkinDisplay::from(resolve_current_skin(
                    skins,
                    &gun.skin_id,
                    &gun.skin_level_id,
                    &gun.chroma_id,
                ));

                LoadoutGunDisplay { weapon, skin }
            })
            .collect::<Vec<_>>();
        gun_skins.sort_by_key(|gun| weapon_order(&gun.weapon.display_name));

        Self {
            account_level: account_level.unwrap_or(response.identity.account_level),
            gun_skins,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LoadoutGunDisplay {
    weapon: WeaponDisplay,
    skin: SkinDisplay,
}

impl LoadoutGunDisplay {
    #[cfg(test)]
    fn label(&self) -> String {
        format!("{}: {}", self.weapon.display_name, self.skin.display_name)
    }
}

fn weapon_order(name: &str) -> (usize, String) {
    let index = match name {
        "Classic" => 0,
        "Shorty" => 1,
        "Frenzy" => 2,
        "Ghost" => 3,
        "Sheriff" => 4,
        "Bandit" => 5,
        "Stinger" => 6,
        "Spectre" => 7,
        "Bucky" => 8,
        "Judge" => 9,
        "Bulldog" => 10,
        "Guardian" => 11,
        "Phantom" => 12,
        "Vandal" => 13,
        "Marshal" => 14,
        "Outlaw" => 15,
        "Operator" => 16,
        "Ares" => 17,
        "Odin" => 18,
        "Melee" => 19,
        _ => 99,
    };

    (index, name.to_string())
}

fn weapon_category(name: &str) -> &'static str {
    match name {
        "Classic" | "Shorty" | "Frenzy" | "Ghost" | "Sheriff" | "Bandit" => "Sidearms",
        "Stinger" | "Spectre" => "SMGs",
        "Bucky" | "Judge" => "Shotguns",
        "Bulldog" | "Guardian" | "Phantom" | "Vandal" => "Rifles",
        "Marshal" | "Outlaw" | "Operator" => "Sniper Rifles",
        "Ares" | "Odin" => "Heavy",
        "Melee" => "Melee",
        _ => "Other",
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WeaponDisplay {
    uuid: String,
    display_name: String,
    display_icon: Option<String>,
    cached_icon: Option<PathBuf>,
}

impl From<ResolvedWeapon> for WeaponDisplay {
    fn from(weapon: ResolvedWeapon) -> Self {
        Self {
            uuid: weapon.uuid,
            display_name: weapon.display_name,
            display_icon: weapon.display_icon,
            cached_icon: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SkinDisplay {
    uuid: String,
    display_name: String,
    display_icon: Option<String>,
    rarity: Option<String>,
    cached_icon: Option<PathBuf>,
}

impl From<ResolvedSkin> for SkinDisplay {
    fn from(skin: ResolvedSkin) -> Self {
        Self {
            uuid: skin.uuid,
            display_name: skin.display_name,
            display_icon: skin.display_icon,
            rarity: skin.rarity,
            cached_icon: None,
        }
    }
}

async fn fetch_storefront(
    account: AccountProfile,
    client_version: String,
    image_cache: ImageCache,
) -> Result<StorefrontResult, String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let resolved = resolve_credentials(&api, &account, client_version).await?;
    let metadata = fetch_store_metadata().await;
    let mut summary = api
        .storefront(&resolved.credentials)
        .await
        .map(|response| {
            StoreSummary::from_response(
                response,
                &metadata.skins,
                &metadata.bundles,
                &metadata.currencies,
            )
        })
        .map_err(|error| error.to_string())?;
    cache_store_images(&mut summary, &image_cache).await;

    Ok(StorefrontResult {
        account_id: account.id,
        summary,
        session: resolved.session,
        identity: resolved.identity,
    })
}

async fn fetch_loadout(
    account: AccountProfile,
    client_version: String,
    image_cache: ImageCache,
) -> Result<LoadoutResult, String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let resolved = resolve_credentials(&api, &account, client_version).await?;
    let metadata = fetch_loadout_metadata().await;
    let account_level = api
        .account_xp(&resolved.credentials)
        .await
        .ok()
        .map(|xp| xp.progress.level);
    let mut summary = api
        .player_loadout(&resolved.credentials)
        .await
        .map(|response| {
            LoadoutSummary::from_response(
                response,
                &metadata.skins,
                &metadata.weapons,
                account_level,
            )
        })
        .map_err(|error| error.to_string())?;
    cache_loadout_images(&mut summary, &image_cache).await;

    Ok(LoadoutResult {
        account_id: account.id,
        summary,
        session: resolved.session,
        identity: resolved.identity,
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
    bundles: BundleCatalog,
    currencies: CurrencyCatalog,
}

async fn fetch_store_metadata() -> StoreMetadata {
    match ValorantContentApi::new() {
        Ok(api) => StoreMetadata {
            skins: api.skin_catalog().await.unwrap_or_default(),
            bundles: api.bundle_catalog().await.unwrap_or_default(),
            currencies: api.currency_catalog().await.unwrap_or_default(),
        },
        Err(_) => StoreMetadata::default(),
    }
}

async fn cache_store_images(summary: &mut StoreSummary, image_cache: &ImageCache) {
    for bundle in &mut summary.featured_bundles {
        cache_bundle_icon(&mut bundle.bundle, image_cache).await;
    }

    for offer in summary
        .daily_offers
        .iter_mut()
        .chain(summary.night_market_offers.iter_mut())
    {
        cache_skin_icon(&mut offer.skin, image_cache).await;
    }
}

async fn cache_loadout_images(summary: &mut LoadoutSummary, image_cache: &ImageCache) {
    for gun in &mut summary.gun_skins {
        cache_weapon_icon(&mut gun.weapon, image_cache).await;
        cache_skin_icon(&mut gun.skin, image_cache).await;
    }
}

async fn cache_skin_icon(skin: &mut SkinDisplay, image_cache: &ImageCache) {
    let Some(url) = skin.display_icon.as_ref() else {
        return;
    };

    skin.cached_icon = image_cache.cache_url("skins", &skin.uuid, url).await.ok();
}

async fn cache_weapon_icon(weapon: &mut WeaponDisplay, image_cache: &ImageCache) {
    let Some(url) = weapon.display_icon.as_ref() else {
        return;
    };

    weapon.cached_icon = image_cache
        .cache_url("weapons", &weapon.uuid, url)
        .await
        .ok();
}

async fn cache_bundle_icon(bundle: &mut BundleDisplay, image_cache: &ImageCache) {
    let Some(url) = bundle.display_icon.as_ref() else {
        return;
    };

    bundle.cached_icon = image_cache
        .cache_url("bundles", &bundle.uuid, url)
        .await
        .ok();
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

fn resolve_current_skin(
    catalog: &SkinCatalog,
    skin_id: &str,
    skin_level_id: &str,
    chroma_id: &str,
) -> ResolvedSkin {
    let mut fallback = None;

    for id in [chroma_id, skin_level_id, skin_id] {
        let skin = catalog.resolve(id);

        if skin.display_name == id {
            continue;
        }

        if skin.display_icon.is_some() {
            return skin;
        }

        fallback.get_or_insert(skin);
    }

    fallback.unwrap_or_else(|| catalog.resolve(skin_id))
}

async fn resolve_credentials(
    api: &RiotApi,
    account: &AccountProfile,
    client_version: String,
) -> Result<ResolvedApiCredentials, String> {
    let mut session = active_api_session(api, account).await?;
    let player_info = api.player_info(&session.access_token).await.ok();

    let entitlements_token = entitlement_token(api, &session).await?;
    if session
        .entitlements_token
        .as_ref()
        .is_none_or(|token| token.trim().is_empty())
    {
        session.entitlements_token = Some(entitlements_token.clone());
    }

    let puuid = player_info
        .as_ref()
        .map(|info| info.sub.clone())
        .or_else(|| {
            account
                .puuid
                .clone()
                .filter(|puuid| !puuid.trim().is_empty())
        })
        .or_else(|| {
            account
                .launcher_session
                .as_ref()
                .map(|backup| backup.puuid.clone())
                .filter(|puuid| !puuid.trim().is_empty())
        })
        .ok_or_else(|| "selected account does not have a Riot PUUID".to_string())?;
    let shard = resolve_session_shard(api, &session, player_info.as_ref(), account.shard).await;
    let identity = match player_info {
        Some(info) => ApiIdentity {
            puuid: puuid.clone(),
            game_name: Some(info.acct.game_name),
            tag_line: Some(info.acct.tag_line),
            shard,
        },
        None => ApiIdentity {
            puuid: puuid.clone(),
            game_name: None,
            tag_line: None,
            shard,
        },
    };

    Ok(ResolvedApiCredentials {
        credentials: ApiCredentials {
            access_token: session.access_token.clone(),
            entitlements_token,
            client_version,
            shard,
            puuid,
        },
        session,
        identity,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedApiCredentials {
    credentials: ApiCredentials,
    session: AuthSession,
    identity: ApiIdentity,
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

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes_f = bytes as f64;

    if bytes_f >= GB {
        format!("{:.1} GB", bytes_f / GB)
    } else if bytes_f >= MB {
        format!("{:.1} MB", bytes_f / MB)
    } else if bytes_f >= KB {
        format!("{:.1} KB", bytes_f / KB)
    } else {
        format!("{bytes} B")
    }
}

fn cache_account_api_context(
    state: &mut StoredState,
    account_id: AccountId,
    session: AuthSession,
    identity: ApiIdentity,
) -> Result<(), String> {
    let Some(account) = state
        .accounts
        .iter_mut()
        .find(|account| account.id == account_id)
    else {
        return Err("selected profile no longer exists".to_string());
    };

    account.shard = identity.shard;
    account.session = Some(session);

    match (identity.game_name, identity.tag_line) {
        (Some(game_name), Some(tag_line)) => account
            .apply_riot_identity(identity.puuid, game_name, tag_line)
            .map_err(|error| error.to_string()),
        _ => {
            account.puuid = Some(identity.puuid);
            Ok(())
        }
    }
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
                    "Items": [{
                        "Item": {
                            "ItemTypeID": "skin-type",
                            "ItemID": "a",
                            "Amount": 1
                        },
                        "BasePrice": 1775,
                        "CurrencyID": "vp",
                        "DiscountPercent": 20,
                        "DiscountedPrice": 1420,
                        "IsPromoItem": false
                    }],
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
            content_tier_uuid: None,
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
        let bundles = BundleCatalog::from_bundles(vec![crate::riot::content::Bundle {
            uuid: "asset".to_string(),
            display_name: "Give Back Bundle".to_string(),
            display_icon: Some("bundle-icon".to_string()),
            display_icon2: None,
            vertical_promo_image: None,
        }]);
        let summary = StoreSummary::from_response(response, &catalog, &bundles, &currencies);

        assert_eq!(
            summary
                .featured_bundles
                .iter()
                .map(StoreBundleDisplay::label)
                .collect::<Vec<_>>(),
            ["Give Back Bundle (1420 VP), 1 item"]
        );
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
        assert_eq!(summary.night_market_remaining_seconds, Some(40));
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
    fn store_summary_keeps_distinct_featured_bundle_entries_with_shared_asset() {
        let response: StorefrontResponse = serde_json::from_value(serde_json::json!({
            "FeaturedBundle": {
                "Bundle": {
                    "ID": "bundle-a",
                    "DataAssetID": "asset-a",
                    "CurrencyID": "vp",
                    "Items": [{
                        "Item": {
                            "ItemTypeID": "skin-type",
                            "ItemID": "skin-a",
                            "Amount": 99
                        },
                        "BasePrice": 0,
                        "CurrencyID": "vp",
                        "DiscountPercent": 0,
                        "DiscountedPrice": 0,
                        "IsPromoItem": false
                    }],
                    "DurationRemainingInSeconds": 10
                },
                "Bundles": [
                    {
                        "ID": "bundle-a",
                        "DataAssetID": "asset-a",
                        "CurrencyID": "vp",
                        "Items": [{
                            "Item": {
                                "ItemTypeID": "skin-type",
                                "ItemID": "skin-a",
                                "Amount": 1
                            },
                            "BasePrice": 0,
                            "CurrencyID": "vp",
                            "DiscountPercent": 0,
                            "DiscountedPrice": 0,
                            "IsPromoItem": false
                        }],
                        "DurationRemainingInSeconds": 10
                    },
                    {
                        "ID": "bundle-b",
                        "DataAssetID": "asset-a",
                        "CurrencyID": "vp",
                        "Items": [{
                            "Item": {
                                "ItemTypeID": "skin-type",
                                "ItemID": "skin-b",
                                "Amount": 2
                            },
                            "BasePrice": 0,
                            "CurrencyID": "vp",
                            "DiscountPercent": 0,
                            "DiscountedPrice": 0,
                            "IsPromoItem": false
                        }],
                        "DurationRemainingInSeconds": 10
                    }
                ],
                "BundleRemainingDurationInSeconds": 20
            },
            "SkinsPanelLayout": {
                "SingleItemOffers": [],
                "SingleItemStoreOffers": [],
                "SingleItemOffersRemainingDurationInSeconds": 30
            }
        }))
        .expect("response");
        let bundles = BundleCatalog::from_bundles(vec![crate::riot::content::Bundle {
            uuid: "asset-a".to_string(),
            display_name: "Shared Test Bundle".to_string(),
            display_icon: None,
            display_icon2: None,
            vertical_promo_image: None,
        }]);

        let summary = StoreSummary::from_response(
            response,
            &SkinCatalog::default(),
            &bundles,
            &CurrencyCatalog::default(),
        );

        assert_eq!(summary.featured_bundles.len(), 2);
        assert!(
            summary
                .featured_bundles
                .iter()
                .all(|bundle| bundle.bundle.display_name == "Shared Test Bundle")
        );
        assert_eq!(
            summary
                .featured_bundles
                .iter()
                .map(StoreBundleDisplay::item_count_label)
                .collect::<Vec<_>>(),
            ["1 item", "2 items"]
        );
    }

    #[test]
    fn store_summary_expires_at_earliest_shop_section_reset() {
        let loaded_at = iced::time::Instant::now();
        let summary = StoreSummary {
            featured_bundles: vec![],
            daily_offers: vec![],
            daily_remaining_seconds: 30,
            bundle_remaining_seconds: 20,
            night_market_remaining_seconds: None,
            loaded_at,
            night_market_offers: vec![],
        };

        assert!(!summary.is_expired_at(loaded_at + Duration::from_secs(19)));
        assert!(summary.is_expired_at(loaded_at + Duration::from_secs(20)));
    }

    #[test]
    fn format_duration_includes_ticking_seconds() {
        assert_eq!(format_duration(3_661), "1h 1m 1s");
        assert_eq!(format_duration(61), "1m 1s");
        assert_eq!(format_duration(5), "5s");
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
            content_tier_uuid: None,
            levels: vec![],
            chromas: vec![],
        }]);
        let weapons = WeaponCatalog::from_weapons(vec![crate::riot::content::Weapon {
            uuid: "weapon".to_string(),
            display_name: "Vandal".to_string(),
            display_icon: None,
        }]);

        let summary = LoadoutSummary::from_response(response, &catalog, &weapons, None);

        assert_eq!(summary.gun_skins[0].label(), "Vandal: Prime Vandal");
    }

    #[test]
    fn loadout_weapon_categories_include_newer_weapons() {
        assert_eq!(weapon_category("Bandit"), "Sidearms");
        assert_eq!(weapon_category("Outlaw"), "Sniper Rifles");
        assert!(weapon_order("Bandit") < weapon_order("Stinger"));
        assert!(weapon_order("Outlaw") < weapon_order("Operator"));
    }

    #[test]
    fn loadout_summary_prefers_current_chroma_render() {
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
            display_icon: Some("skin-icon".to_string()),
            content_tier_uuid: None,
            levels: vec![crate::riot::content::WeaponSkinLevel {
                uuid: "level-a".to_string(),
                display_name: "Prime Vandal Level 4".to_string(),
                display_icon: None,
            }],
            chromas: vec![crate::riot::content::WeaponSkinChroma {
                uuid: "chroma-a".to_string(),
                display_name: "Prime Vandal Blue".to_string(),
                display_icon: None,
                full_render: Some("chroma-render".to_string()),
            }],
        }]);
        let weapons = WeaponCatalog::from_weapons(vec![crate::riot::content::Weapon {
            uuid: "weapon".to_string(),
            display_name: "Vandal".to_string(),
            display_icon: Some("weapon-icon".to_string()),
        }]);

        let summary = LoadoutSummary::from_response(response, &catalog, &weapons, None);

        assert_eq!(summary.gun_skins[0].skin.uuid, "chroma-a");
        assert_eq!(
            summary.gun_skins[0].skin.display_icon.as_deref(),
            Some("chroma-render")
        );
    }

    #[test]
    fn loadout_summary_prefers_account_xp_level() {
        let response: PlayerLoadoutResponse = serde_json::from_value(serde_json::json!({
            "Subject": "puuid",
            "Version": 1,
            "Guns": [],
            "Sprays": [],
            "Identity": {
                "PlayerCardID": "card",
                "PlayerTitleID": "title",
                "AccountLevel": 0,
                "PreferredLevelBorderID": "border",
                "HideAccountLevel": false
            },
            "Incognito": false
        }))
        .expect("loadout");

        let summary = LoadoutSummary::from_response(
            response,
            &SkinCatalog::default(),
            &WeaponCatalog::default(),
            Some(88),
        );

        assert_eq!(summary.account_level, 88);
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
    fn only_missing_private_settings_is_pending_login_capture() {
        assert!(is_pending_launcher_capture_error(
            &LauncherSessionError::PrivateSettingsNotFound
        ));
        assert!(!is_pending_launcher_capture_error(
            &LauncherSessionError::MissingSsid
        ));
    }

    #[test]
    fn cache_account_api_context_updates_matching_account() {
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

        cache_account_api_context(
            &mut state,
            account_id,
            session.clone(),
            ApiIdentity {
                puuid: "puuid".to_string(),
                game_name: Some("Player".to_string()),
                tag_line: Some("NA1".to_string()),
                shard: Shard::Eu,
            },
        )
        .expect("cache api context");

        assert_eq!(state.accounts[0].session, Some(session));
        assert_eq!(state.accounts[0].puuid.as_deref(), Some("puuid"));
        assert_eq!(state.accounts[0].riot_id().as_deref(), Some("Player#NA1"));
        assert_eq!(state.accounts[0].shard, Shard::Eu);
    }

    #[test]
    fn cache_account_api_context_rejects_missing_account() {
        let mut state = StoredState::default();
        let session = AuthSession::new("access", None, None, "Bearer", Some(3600), 100);

        let err = cache_account_api_context(
            &mut state,
            AccountId::new(),
            session,
            ApiIdentity {
                puuid: "puuid".to_string(),
                game_name: None,
                tag_line: None,
                shard: Shard::Na,
            },
        )
        .expect_err("missing account");

        assert!(err.contains("profile no longer exists"));
    }
}
