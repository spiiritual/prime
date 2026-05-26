use std::path::PathBuf;
use std::time::Duration;

use iced::widget::image::Handle;
use iced::widget::{
    button, column, container, image, pick_list, row, scrollable, text, text_input,
};
use iced::{ContentFit, Element, Length, Task, Theme};

use crate::account::LauncherSessionBackup;
use crate::account::{AccountId, AccountProfile, AuthSession, Shard};
use crate::image_cache::ImageCache;
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
    CapturedLauncherSession, LauncherSessionError, apply_launcher_session_backup,
    capture_current_launcher_session, clear_existing_launcher_data_dirs, launcher_cookie_header,
    read_backup_cookies,
};
use crate::riot::models::{
    BonusStoreOffer, BundleItem, PlayerInfoResponse, PlayerLoadoutResponse, StoreOffer,
    StorefrontResponse,
};
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
            Message::FetchStorefront => self.fetch_storefront_task(),
            Message::StorefrontLoaded(result) => {
                self.store_loading = false;

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

                        let daily_count = result.summary.daily_offers.len();
                        let night_market_count = result.summary.night_market_offers.len();

                        self.status = format!(
                            "Loaded {} daily offer(s) and {} night market offer(s)",
                            daily_count, night_market_count
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
            Message::FetchLoadout => self.fetch_loadout_task(),
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

        column![
            container(
                row![
                    text(self.active_tab.to_string()).size(30),
                    button("Launch VALORANT").on_press(Message::LaunchSelected)
                ]
                .spacing(16)
            )
            .padding(14)
            .width(Length::Fill)
            .style(iced::widget::container::bordered_box),
            container(scrollable(body))
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
                button("Refresh profile").on_press(Message::RefreshProfileIdentity)
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
            "Shop loads automatically for the selected account."
        };
        let mut content = column![
            text(loading_label),
            button("Refresh shop").on_press(Message::FetchStorefront)
        ]
        .spacing(12);

        if let Some(summary) = &self.store_summary {
            content = content
                .push(text(format!(
                    "Featured bundle expires in {}",
                    format_duration(summary.bundle_remaining_seconds)
                )))
                .push(offer_row(&summary.featured_bundle_items))
                .push(text(format!(
                    "Daily offers expire in {}",
                    format_duration(summary.daily_remaining_seconds)
                )))
                .push(offer_row(&summary.daily_offers));

            if !summary.night_market_offers.is_empty() {
                content = content
                    .push(text("Night Market"))
                    .push(offer_row(&summary.night_market_offers));
            }
        }

        content.into()
    }

    fn loadout_tab(&self) -> Element<'_, Message> {
        let loading_label = if self.loadout_loading {
            "Loading loadout..."
        } else {
            "Loadout loads automatically for the selected account."
        };
        let mut content = column![
            text(loading_label),
            button("Refresh loadout").on_press(Message::FetchLoadout)
        ]
        .spacing(12);

        if let Some(summary) = &self.loadout_summary {
            content = content.push(text(format!("Account level {}", summary.account_level)));

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

    let mut cards = iced::widget::Row::new().spacing(10);

    for offer in offers {
        cards = cards.push(store_offer_card(offer));
    }

    cards.into()
}

fn store_offer_card(offer: &StoreOfferDisplay) -> Element<'_, Message> {
    let price = offer
        .price
        .as_ref()
        .map(OfferPrice::label)
        .unwrap_or_else(|| "Price unavailable".to_string());
    let rarity = offer
        .skin
        .rarity
        .as_deref()
        .unwrap_or("Unknown rarity")
        .to_string();
    let discount = if offer.discount_percent > 0 {
        format!("{}% off", offer.discount_percent)
    } else {
        String::new()
    };

    container(
        column![
            asset_image(offer.skin.cached_icon.as_ref(), 118.0),
            text(&offer.skin.display_name).size(16),
            text(rarity).size(13),
            text(price).size(14),
            text(discount).size(13)
        ]
        .spacing(6),
    )
    .padding(10)
    .width(160)
    .style(iced::widget::container::bordered_box)
    .into()
}

fn loadout_section<'a>(
    category: &'static str,
    guns: impl IntoIterator<Item = &'a LoadoutGunDisplay>,
) -> Option<Element<'a, Message>> {
    let mut cards = iced::widget::Row::new().spacing(10);
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
            asset_image(gun.weapon.cached_icon.as_ref(), 56.0),
            text(&gun.weapon.display_name).size(15),
            asset_image(gun.skin.cached_icon.as_ref(), 86.0),
            text(&gun.skin.display_name).size(14)
        ]
        .spacing(6),
    )
    .padding(10)
    .width(150)
    .style(iced::widget::container::bordered_box)
    .into()
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

fn format_duration(seconds: i64) -> String {
    if seconds <= 0 {
        return "soon".to_string();
    }

    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;

    if hours >= 24 {
        format!("{}d {}h", hours / 24, hours % 24)
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
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
    FetchStorefront,
    StorefrontLoaded(Result<StorefrontResult, String>),
    FetchLoadout,
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
    featured_bundle_items: Vec<StoreOfferDisplay>,
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
        let featured_bundle_items = response
            .featured_bundle
            .bundle
            .items
            .iter()
            .map(|item| bundle_item_display(item, skins, currencies))
            .collect();
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
            featured_bundle_items,
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

fn bundle_item_display(
    item: &BundleItem,
    skins: &SkinCatalog,
    currencies: &CurrencyCatalog,
) -> StoreOfferDisplay {
    StoreOfferDisplay {
        skin: SkinDisplay::from(skins.resolve(&item.item.item_id)),
        price: Some(OfferPrice {
            amount: if item.discounted_price > 0 {
                item.discounted_price
            } else {
                item.base_price
            },
            currency: CurrencyDisplay::from(currencies.resolve(&item.currency_id)),
        }),
        discount_percent: item.discount_percent,
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
                let skin = SkinDisplay::from(resolve_first(
                    skins,
                    [&gun.skin_id, &gun.skin_level_id, &gun.chroma_id],
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
        "Stinger" => 5,
        "Spectre" => 6,
        "Bucky" => 7,
        "Judge" => 8,
        "Bulldog" => 9,
        "Guardian" => 10,
        "Phantom" => 11,
        "Vandal" => 12,
        "Marshal" => 13,
        "Operator" => 14,
        "Ares" => 15,
        "Odin" => 16,
        "Melee" => 17,
        _ => 99,
    };

    (index, name.to_string())
}

fn weapon_category(name: &str) -> &'static str {
    match name {
        "Classic" | "Shorty" | "Frenzy" | "Ghost" | "Sheriff" => "Sidearms",
        "Stinger" | "Spectre" => "SMGs",
        "Bucky" | "Judge" => "Shotguns",
        "Bulldog" | "Guardian" | "Phantom" | "Vandal" => "Rifles",
        "Marshal" | "Operator" => "Sniper Rifles",
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
            StoreSummary::from_response(response, &metadata.skins, &metadata.currencies)
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

async fn cache_store_images(summary: &mut StoreSummary, image_cache: &ImageCache) {
    for offer in summary
        .featured_bundle_items
        .iter_mut()
        .chain(summary.daily_offers.iter_mut())
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
        let summary = StoreSummary::from_response(response, &catalog, &currencies);

        assert_eq!(
            summary
                .featured_bundle_items
                .iter()
                .map(StoreOfferDisplay::label)
                .collect::<Vec<_>>(),
            ["Prime Vandal Level 1 (1420 VP), 20% off"]
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
