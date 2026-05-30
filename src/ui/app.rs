use iced::widget::operation;
use iced::{Task, clipboard, window};

use crate::account::{AccountId, AccountProfile, Shard};
use crate::account_transfer::{export_account, import_account};
use crate::image_cache::ImageCache;
use crate::launch::{LaunchConfig, LaunchTargetProcess};
use crate::riot::auth::parse_redirect_tokens;
use crate::riot::launcher_session::{CapturedLauncherSession, remove_launcher_session_backup};
use crate::storage::{AccountRepository, StoredState};
use crate::updater::{check_for_update, download_and_prepare_update};

use super::data::{
    cache_account_api_context, check_riot_client_window_visible, fetch_account_ranks,
    fetch_current_client_version, fetch_loadout, fetch_profile_identity, fetch_storefront,
    launch_account, non_empty_path, start_account_capture, start_launcher_session_login,
};
use super::{
    AppUpdateStatus, ImageViewerImage, ImageViewerSource, LoadoutTab, MAIN_PANEL_SCROLLABLE_ID,
    Message, PrimeApp, Tab, TabScrollOffsets,
};

impl PrimeApp {
    pub(super) fn boot() -> (Self, Task<Message>) {
        let repo = AccountRepository::new(AccountRepository::default_path());
        let image_cache = ImageCache::new(ImageCache::default_path());
        let load_repo = repo.clone();
        let cache_for_size = image_cache.clone();

        (
            Self {
                repo,
                image_cache,
                image_viewer: None,
                state: StoredState::default(),
                active_tab: Tab::Accounts,
                active_loadout_tab: LoadoutTab::Skins,
                tab_scroll_offsets: TabScrollOffsets::default(),
                new_display_name: String::new(),
                new_username: String::new(),
                new_shard: Shard::Na,
                redirect_input: String::new(),
                client_version_input: String::new(),
                riot_client_path_input: String::new(),
                status: "Loading accounts".to_string(),
                account_switcher_open: false,
                open_account_menu: None,
                show_add_account_prompt: false,
                show_import_account_prompt: false,
                import_account_input: String::new(),
                import_account_in_progress: false,
                exported_account: None,
                confirm_delete_account: None,
                pending_account: None,
                store_summary: None,
                loadout_summary: None,
                store_loading: false,
                loadout_loading: false,
                store_loading_account: None,
                loadout_loading_account: None,
                account_ranks_loading: false,
                launcher_capture_in_progress: false,
                launching_account: None,
                launch_progress_checking: false,
                app_update_status: AppUpdateStatus::Checking,
                image_cache_size_bytes: 0,
                loading_frame: 0,
                now: iced::time::Instant::now(),
            },
            Task::batch([
                Task::perform(
                    async move { load_repo.load().map_err(|error| error.to_string()) },
                    Message::Loaded,
                ),
                Task::perform(fetch_current_client_version(), Message::ClientVersionLoaded),
                Task::perform(check_for_update(), |result| Message::AppUpdateChecked {
                    user_requested: false,
                    result: result.map_err(|error| error.to_string()),
                }),
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

    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
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

                self.load_active_tab()
            }
            Message::Saved(result) => {
                if let Err(error) = result {
                    self.status = format!("Failed to save accounts: {error}");
                }

                Task::none()
            }
            Message::TabSelected(tab) => {
                self.active_tab = tab;
                self.image_viewer = None;
                self.account_switcher_open = false;
                self.open_account_menu = None;
                self.show_add_account_prompt = false;
                self.show_import_account_prompt = false;
                self.exported_account = None;
                self.confirm_delete_account = None;
                Task::batch([
                    self.load_active_tab(),
                    self.restore_active_tab_scroll_task(),
                ])
            }
            Message::LoadoutTabSelected(tab) => {
                self.active_loadout_tab = tab;
                Task::none()
            }
            Message::MainPanelScrolled { tab, offset } => {
                if self.active_tab == tab {
                    self.tab_scroll_offsets.set(tab, offset);
                }

                Task::none()
            }
            Message::ToggleAccountSwitcher => {
                if self.state.accounts.is_empty() {
                    self.account_switcher_open = false;
                } else {
                    self.account_switcher_open = !self.account_switcher_open;
                    self.open_account_menu = None;
                    self.show_add_account_prompt = false;
                    self.show_import_account_prompt = false;
                    self.exported_account = None;
                    self.confirm_delete_account = None;
                }

                Task::none()
            }
            Message::SelectAccount(id) => {
                if !self.state.select_account(id) {
                    self.account_switcher_open = false;
                    self.status = "Account profile no longer exists".to_string();
                    return Task::none();
                }

                self.account_switcher_open = false;
                self.image_viewer = None;
                self.open_account_menu = None;
                self.show_add_account_prompt = false;
                self.show_import_account_prompt = false;
                self.exported_account = None;
                self.confirm_delete_account = None;
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
                self.show_add_account_prompt = true;
                self.show_import_account_prompt = false;
                self.exported_account = None;
                self.account_switcher_open = false;
                self.open_account_menu = None;
                self.confirm_delete_account = None;
                self.status =
                    "Before Riot Client opens, confirm that you will tick Stay signed in."
                        .to_string();

                Task::none()
            }
            Message::ConfirmAddAccountCapture => {
                if self.launcher_capture_in_progress {
                    self.status = "Launcher login capture is already in progress".to_string();
                    return Task::none();
                }

                let account_id = AccountId::new();
                let config = LaunchConfig {
                    riot_client_path: self.state.riot_client_path.clone(),
                    ..LaunchConfig::default()
                };
                let backup_root = self.repo.launcher_backups_dir();
                self.show_add_account_prompt = false;
                self.show_import_account_prompt = false;
                self.exported_account = None;
                self.pending_account = None;
                self.account_switcher_open = false;
                self.open_account_menu = None;
                self.confirm_delete_account = None;
                self.new_display_name.clear();
                self.new_username.clear();
                self.status =
                    "Opening Riot Client. Log in with Remember Me enabled to add the account."
                        .to_string();
                self.launcher_capture_in_progress = true;

                Task::perform(
                    async move { start_account_capture(account_id, backup_root, config).await },
                    Message::AccountCaptureFinished,
                )
            }
            Message::CancelAddAccountCapture => {
                self.show_add_account_prompt = false;
                self.status = "Canceled account capture".to_string();
                Task::none()
            }
            Message::AccountCaptureFinished(result) => {
                self.launcher_capture_in_progress = false;

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
                        alert_and_focus_latest_window()
                    }
                    Err(error) => {
                        self.status = format!("Could not add account: {error}");
                        Task::none()
                    }
                }
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
                        self.state.select_account(draft.account_id);
                        self.pending_account = None;
                        self.new_display_name.clear();
                        self.new_username.clear();
                        return Task::batch([self.save_task(), self.load_active_tab()]);
                    }
                    Err(error) => {
                        self.status = error.to_string();
                    }
                }

                Task::none()
            }
            Message::CancelCapturedAccount => {
                let Some(draft) = self.pending_account.as_ref() else {
                    self.status = "No captured account is waiting to be discarded".to_string();
                    return Task::none();
                };

                if let Err(error) = remove_launcher_session_backup(
                    self.repo.launcher_backups_dir(),
                    draft.account_id,
                ) {
                    self.status = format!("Could not discard captured account session: {error}");
                    return Task::none();
                }

                self.pending_account = None;
                self.account_switcher_open = false;
                self.open_account_menu = None;
                self.show_add_account_prompt = false;
                self.show_import_account_prompt = false;
                self.exported_account = None;
                self.confirm_delete_account = None;
                self.new_display_name.clear();
                self.new_username.clear();
                self.status = "Discarded captured account draft".to_string();
                Task::none()
            }
            Message::ToggleAccountMenu(id) => {
                self.confirm_delete_account = None;
                self.account_switcher_open = false;

                if self.state.accounts.iter().any(|account| account.id == id) {
                    self.open_account_menu = match self.open_account_menu {
                        Some(open_id) if open_id == id => None,
                        _ => Some(id),
                    };
                } else {
                    self.open_account_menu = None;
                }

                Task::none()
            }
            Message::RequestExportAccount(id) => {
                let Some(account) = self
                    .state
                    .accounts
                    .iter()
                    .find(|account| account.id == id)
                    .cloned()
                else {
                    self.open_account_menu = None;
                    self.account_switcher_open = false;
                    self.status = "Account profile no longer exists".to_string();
                    return Task::none();
                };

                let account_id = account.id;
                let display_name = account.display_name.clone();
                let summary = account.summary();
                self.account_switcher_open = false;
                self.open_account_menu = None;
                self.show_add_account_prompt = false;
                self.show_import_account_prompt = false;
                self.confirm_delete_account = None;
                self.exported_account = None;
                self.status = format!("Exporting {summary}");

                Task::perform(
                    async move {
                        export_account(&account)
                            .map(|payload| {
                                super::AccountExportOutput::new(account_id, display_name, payload)
                            })
                            .map_err(|error| error.to_string())
                    },
                    Message::AccountExportPrepared,
                )
            }
            Message::AccountExportPrepared(result) => {
                match result {
                    Ok(export) => {
                        self.status =
                            format!("Prepared account export for {}", export.display_name);
                        self.exported_account = Some(export);
                    }
                    Err(error) => {
                        self.status = format!("Could not export account: {error}");
                    }
                }

                Task::none()
            }
            Message::CopyAccountExport => {
                let Some(export) = &self.exported_account else {
                    self.status = "Could not export account: no export is ready".to_string();
                    return Task::none();
                };

                self.status = format!("Copied account export for {}", export.display_name);
                clipboard::write(export.payload.clone())
            }
            Message::CloseAccountExport => {
                self.exported_account = None;
                Task::none()
            }
            Message::OpenImportAccount => {
                self.show_import_account_prompt = true;
                self.show_add_account_prompt = false;
                self.exported_account = None;
                self.account_switcher_open = false;
                self.open_account_menu = None;
                self.confirm_delete_account = None;
                self.status = "Paste an account export to import it".to_string();
                Task::none()
            }
            Message::ImportAccountInputChanged(value) => {
                if self.import_account_in_progress {
                    return Task::none();
                }

                self.import_account_input = value;
                Task::none()
            }
            Message::CancelImportAccount => {
                if self.import_account_in_progress {
                    self.status = "Importing account".to_string();
                    return Task::none();
                }

                self.show_import_account_prompt = false;
                self.import_account_input.clear();
                self.status = "Canceled account import".to_string();
                Task::none()
            }
            Message::ConfirmImportAccount => {
                if self.import_account_in_progress {
                    return Task::none();
                }

                if self.import_account_input.trim().is_empty() {
                    self.status =
                        "Could not import account: paste an account export first".to_string();
                    return Task::none();
                }

                let input = self.import_account_input.clone();
                let backup_root = self.repo.launcher_backups_dir();
                let existing_ids = self
                    .state
                    .accounts
                    .iter()
                    .map(|account| account.id)
                    .collect::<Vec<_>>();

                self.import_account_in_progress = true;
                self.status = "Importing account".to_string();

                Task::perform(
                    async move {
                        import_account(&input, backup_root, &existing_ids)
                            .map_err(|error| error.to_string())
                    },
                    Message::AccountImported,
                )
            }
            Message::AccountImported(result) => {
                self.import_account_in_progress = false;

                match result {
                    Ok(imported) => {
                        let account_id = imported.account.id;
                        let summary = imported.account.summary();
                        let id_note = if imported.id_changed {
                            " with a new local ID"
                        } else {
                            ""
                        };

                        if self
                            .state
                            .accounts
                            .iter()
                            .any(|account| account.id == account_id)
                        {
                            self.status =
                                "Could not import account: imported account ID already exists"
                                    .to_string();
                            return Task::none();
                        }

                        self.state.push_account(imported.account);
                        self.state.select_account(account_id);
                        self.show_import_account_prompt = false;
                        self.import_account_input.clear();
                        self.store_summary = None;
                        self.loadout_summary = None;
                        self.status = format!("Imported {summary}{id_note}");
                        return Task::batch([self.save_task(), self.load_active_tab()]);
                    }
                    Err(error) => {
                        self.status = format!("Could not import account: {error}");
                    }
                }

                Task::none()
            }
            Message::RequestDeleteAccount(id) => {
                if self.state.accounts.iter().any(|account| account.id == id) {
                    self.account_switcher_open = false;
                    self.open_account_menu = None;
                    self.show_add_account_prompt = false;
                    self.show_import_account_prompt = false;
                    self.exported_account = None;
                    self.confirm_delete_account = Some(id);
                } else {
                    self.account_switcher_open = false;
                    self.open_account_menu = None;
                    self.show_add_account_prompt = false;
                    self.show_import_account_prompt = false;
                    self.exported_account = None;
                    self.confirm_delete_account = None;
                    self.status = "Account profile no longer exists".to_string();
                }

                Task::none()
            }
            Message::CancelDeleteAccount => {
                self.confirm_delete_account = None;
                Task::none()
            }
            Message::ConfirmDeleteAccount(id) => {
                let Some(account) = self
                    .state
                    .accounts
                    .iter()
                    .find(|account| account.id == id)
                    .cloned()
                else {
                    self.open_account_menu = None;
                    self.confirm_delete_account = None;
                    self.status = "Account profile no longer exists".to_string();
                    return Task::none();
                };

                if let Err(error) =
                    remove_launcher_session_backup(self.repo.launcher_backups_dir(), id)
                {
                    self.open_account_menu = None;
                    self.status = format!(
                        "Could not delete captured launcher session for {}: {error}",
                        account.summary()
                    );
                    return Task::none();
                }

                let was_selected = self.state.selected_account == Some(id);
                self.state.remove_account(id);
                self.account_switcher_open = false;
                self.open_account_menu = None;
                if self
                    .exported_account
                    .as_ref()
                    .is_some_and(|export| export.account_id == id)
                {
                    self.exported_account = None;
                }
                self.confirm_delete_account = None;

                if was_selected {
                    self.store_summary = None;
                    self.loadout_summary = None;
                }

                self.status = format!("Deleted {}", account.summary());
                self.save_task()
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

                self.load_active_tab()
            }
            Message::ImportRedirect => {
                let Some(account) = self.state.selected_account_mut() else {
                    self.account_switcher_open = false;
                    self.status = "Select an account before importing a token".to_string();
                    return Task::none();
                };

                match parse_redirect_tokens(&self.redirect_input) {
                    Ok(tokens) => {
                        account.session = Some(tokens.into_session());
                        self.redirect_input.clear();
                        self.status =
                            "Imported Riot redirect token for selected account".to_string();
                        Task::batch([self.save_task(), self.load_active_tab()])
                    }
                    Err(error) => {
                        self.status = format!("Could not import redirect token: {error}");
                        Task::none()
                    }
                }
            }
            Message::StartLauncherSessionLogin(account_id) => {
                if self.launcher_capture_in_progress {
                    self.status = "Launcher login capture is already in progress".to_string();
                    return Task::none();
                }

                let Some(account) = self
                    .state
                    .accounts
                    .iter()
                    .find(|account| account.id == account_id)
                else {
                    self.open_account_menu = None;
                    self.account_switcher_open = false;
                    self.status = "Account profile no longer exists".to_string();
                    return Task::none();
                };

                let config = LaunchConfig {
                    riot_client_path: self.state.riot_client_path.clone(),
                    ..LaunchConfig::default()
                };
                let backup_root = self.repo.launcher_backups_dir();
                let summary = account.summary();
                self.account_switcher_open = false;
                self.open_account_menu = None;
                self.show_import_account_prompt = false;
                self.exported_account = None;
                self.confirm_delete_account = None;
                self.status = format!(
                    "Opening Riot Client and waiting for remembered login capture for {summary}"
                );
                self.launcher_capture_in_progress = true;

                Task::perform(
                    async move { start_launcher_session_login(account_id, backup_root, config).await },
                    Message::LauncherSessionLoginStarted,
                )
            }
            Message::LauncherSessionLoginStarted(result) => {
                self.launcher_capture_in_progress = false;

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
            Message::RefreshProfileIdentity(account_id) => {
                let Some(account) = self
                    .state
                    .accounts
                    .iter()
                    .find(|account| account.id == account_id)
                    .cloned()
                else {
                    self.open_account_menu = None;
                    self.account_switcher_open = false;
                    self.status = "Account profile no longer exists".to_string();
                    return Task::none();
                };

                let summary = account.summary();
                self.account_switcher_open = false;
                self.open_account_menu = None;
                self.show_import_account_prompt = false;
                self.exported_account = None;
                self.confirm_delete_account = None;
                self.status = format!("Refreshing Riot profile identity for {summary}");
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
                            account.mark_refreshed_now();
                            self.status = format!("Refreshed {}", account.summary());
                            return Task::batch([self.save_task(), self.load_active_tab()]);
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
            Message::AccountRanksLoaded(result) => {
                self.account_ranks_loading = false;

                let mut updated = 0usize;
                let mut partial = 0usize;
                let mut context_failures = 0usize;

                for rank in result.ranks {
                    let account_id = rank.account_id;
                    let rank_partial = rank.rank.is_err() || rank.account_level.is_err();

                    if let Err(error) = cache_account_api_context(
                        &mut self.state,
                        account_id,
                        rank.session,
                        rank.identity,
                    ) {
                        context_failures += 1;
                        self.status =
                            format!("Account details loaded, but profile update failed: {error}");
                        continue;
                    }

                    if let Some(account) = self
                        .state
                        .accounts
                        .iter_mut()
                        .find(|account| account.id == account_id)
                    {
                        let mut account_updated = false;

                        if let Ok(competitive_rank) = rank.rank {
                            account.competitive_rank = competitive_rank;
                            account_updated = true;
                        }

                        if let Ok(account_level) = rank.account_level {
                            account.account_level = Some(account_level);
                            account_updated = true;
                        }

                        if account_updated {
                            updated += 1;
                        }

                        if rank_partial {
                            partial += 1;
                        }
                    }
                }

                let failed = result.failures.len() + context_failures;
                self.status = match (updated, failed, partial) {
                    (0, 0, 0) => "No account details to refresh".to_string(),
                    (0, failed, _) => {
                        format!("Account detail refresh failed for {failed} account(s)")
                    }
                    (updated, 0, 0) => format!("Loaded account details for {updated} account(s)"),
                    (updated, 0, partial) => {
                        format!(
                            "Loaded account details for {updated} account(s); {partial} partial"
                        )
                    }
                    (updated, failed, 0) => {
                        format!(
                            "Loaded account details for {updated} account(s); {failed} unavailable"
                        )
                    }
                    (updated, failed, partial) => {
                        format!(
                            "Loaded account details for {updated} account(s); {failed} unavailable, {partial} partial"
                        )
                    }
                };

                if updated > 0 {
                    self.save_task()
                } else {
                    Task::none()
                }
            }
            Message::StorefrontLoaded(account_id, result) => {
                let is_current_request = self.store_loading_account == Some(account_id);

                if is_current_request {
                    self.store_loading = false;
                    self.store_loading_account = None;
                }
                self.now = iced::time::Instant::now();

                match result {
                    Ok(result) => {
                        if !is_current_request {
                            if result.account_id != account_id {
                                return Task::none();
                            }

                            if cache_account_api_context(
                                &mut self.state,
                                result.account_id,
                                result.session,
                                result.identity,
                            )
                            .is_ok()
                            {
                                return self.save_task();
                            }

                            return Task::none();
                        }

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
                        let balance_status = if result.summary.currency_balance_error.is_some() {
                            ", but currency balances were unavailable"
                        } else {
                            ""
                        };

                        self.status = format!(
                            "Loaded {} featured bundle(s), {} daily offer(s), and {} night market offer(s){}",
                            bundle_count, daily_count, night_market_count, balance_status
                        );
                        if self.state.selected_account == Some(result.account_id) {
                            self.store_summary = Some(result.summary);
                        }

                        return Task::batch([self.save_task(), self.image_cache_size_task()]);
                    }
                    Err(error) => {
                        if is_current_request {
                            self.status = format!("Store check failed: {error}");
                        }
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
            Message::LoadingTick => {
                if super::loading_indicator_active(self) {
                    self.loading_frame = self.loading_frame.wrapping_add(1);
                }

                Task::none()
            }
            Message::LoadoutLoaded(account_id, result) => {
                let is_current_request = self.loadout_loading_account == Some(account_id);

                if is_current_request {
                    self.loadout_loading = false;
                    self.loadout_loading_account = None;
                }
                self.now = iced::time::Instant::now();

                match result {
                    Ok(result) => {
                        if !is_current_request {
                            if result.account_id != account_id {
                                return Task::none();
                            }

                            if cache_account_api_context(
                                &mut self.state,
                                result.account_id,
                                result.session,
                                result.identity,
                            )
                            .is_ok()
                            {
                                return self.save_task();
                            }

                            return Task::none();
                        }

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
                        let battle_pass_status = if result.summary.battle_pass.is_some() {
                            " and battle pass progress"
                        } else {
                            ""
                        };

                        self.status = format!(
                            "Loaded loadout with {} gun skin(s){}",
                            gun_count, battle_pass_status
                        );
                        if let Some(account) = self
                            .state
                            .accounts
                            .iter_mut()
                            .find(|account| account.id == result.account_id)
                        {
                            account.account_level = Some(result.summary.account_level);
                        }

                        if self.state.selected_account == Some(result.account_id) {
                            self.loadout_summary = Some(result.summary);
                        }

                        return Task::batch([self.save_task(), self.image_cache_size_task()]);
                    }
                    Err(error) => {
                        if is_current_request {
                            self.status = format!("Loadout check failed: {error}");
                        }
                    }
                }

                Task::none()
            }
            Message::OpenImageViewer(image) => {
                let high_res = image.high_res.clone();
                self.image_viewer = Some(ImageViewerImage::from_request(image));

                if let Some(source) = high_res {
                    if let Some(viewer) = &mut self.image_viewer {
                        viewer.high_res_loading = true;
                    }

                    return self.load_image_viewer_source_task(source);
                }

                Task::none()
            }
            Message::ImageViewerImageLoaded(source, result) => {
                let Some(viewer) = &mut self.image_viewer else {
                    return Task::none();
                };

                if viewer.high_res.as_ref() != Some(&source) {
                    return Task::none();
                }

                viewer.high_res_loading = false;

                match result {
                    Ok(path) => {
                        viewer.path = path;
                        viewer.high_res_error = None;
                        return self.image_cache_size_task();
                    }
                    Err(error) => {
                        viewer.high_res_error = Some("Full image unavailable".to_string());
                        self.status = format!("Could not load full image: {error}");
                    }
                }

                Task::none()
            }
            Message::CloseImageViewer => {
                self.image_viewer = None;
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
            Message::LaunchAccount(id) => {
                if self.launching_account.is_some() {
                    return Task::none();
                }

                let Some(account) = self
                    .state
                    .accounts
                    .iter()
                    .find(|account| account.id == id)
                    .cloned()
                else {
                    self.status = "Account profile no longer exists".to_string();
                    return Task::none();
                };

                let config = LaunchConfig {
                    riot_client_path: self.state.riot_client_path.clone(),
                    ..LaunchConfig::default()
                };
                let backup = account.launcher_session.clone();
                let summary = account.summary();

                self.state.select_account(id);
                self.account_switcher_open = false;
                self.open_account_menu = None;
                self.show_import_account_prompt = false;
                self.exported_account = None;
                self.confirm_delete_account = None;
                self.store_summary = None;
                self.loadout_summary = None;
                self.launching_account = Some(id);
                self.launch_progress_checking = false;
                self.status = format!("Launching {summary}");

                Task::batch([
                    self.save_task(),
                    Task::perform(
                        async move { launch_account(config, backup).await },
                        Message::LaunchFinished,
                    ),
                ])
            }
            Message::LaunchProgressTick => {
                if self.launching_account.is_none() || self.launch_progress_checking {
                    return Task::none();
                }

                self.launch_progress_checking = true;
                Task::perform(
                    check_riot_client_window_visible(),
                    Message::LaunchProgressChecked,
                )
            }
            Message::LaunchProgressChecked(result) => {
                self.launch_progress_checking = false;

                if self.launching_account.is_none() {
                    return Task::none();
                }

                if matches!(result, Ok(true)) {
                    self.status = "Riot Client is open; waiting for VALORANT".to_string();
                }

                Task::none()
            }
            Message::LaunchFinished(result) => match result {
                Ok(LaunchTargetProcess::Valorant) => {
                    self.launching_account = None;
                    self.launch_progress_checking = false;
                    self.status = "VALORANT window detected".to_string();
                    Task::none()
                }
                Err(error) => {
                    self.launching_account = None;
                    self.launch_progress_checking = false;
                    self.status = format!("Launch failed: {error}");
                    Task::none()
                }
            },
            Message::CheckForAppUpdate => {
                if self.app_update_status.is_busy() {
                    return Task::none();
                }

                self.app_update_status = AppUpdateStatus::Checking;
                self.status = "Checking for Prime updates".to_string();
                Task::perform(check_for_update(), |result| Message::AppUpdateChecked {
                    user_requested: true,
                    result: result.map_err(|error| error.to_string()),
                })
            }
            Message::AppUpdateChecked {
                user_requested,
                result,
            } => {
                match result {
                    Ok(Some(update)) => {
                        self.status = format!(
                            "Prime {} is available; download it when ready",
                            update.latest_version
                        );
                        self.app_update_status = AppUpdateStatus::Available(update);
                    }
                    Ok(None) => {
                        self.app_update_status = AppUpdateStatus::UpToDate;

                        if user_requested {
                            self.status = format!(
                                "Prime is up to date ({})",
                                crate::updater::CURRENT_VERSION
                            );
                        }
                    }
                    Err(error) => {
                        self.app_update_status = AppUpdateStatus::Failed(error.clone());

                        if user_requested {
                            self.status = format!("Update check failed: {error}");
                        }
                    }
                }

                Task::none()
            }
            Message::DismissAppUpdate => {
                if let Some(update) = self.app_update_status.prompt_update().cloned() {
                    self.app_update_status = AppUpdateStatus::Dismissed(update);
                    self.status = "Update postponed".to_string();
                }

                Task::none()
            }
            Message::DownloadAppUpdate => {
                let Some(update) = self.app_update_status.pending_update().cloned() else {
                    self.status = "No Prime update is available to download".to_string();
                    return Task::none();
                };

                self.status = format!("Downloading Prime {}", update.latest_version);
                self.app_update_status = AppUpdateStatus::Downloading(update.clone());
                Task::perform(download_and_prepare_update(update), |result| {
                    Message::AppUpdatePrepared(result.map_err(|error| error.to_string()))
                })
            }
            Message::AppUpdatePrepared(result) => match result {
                Ok(()) => {
                    self.app_update_status = AppUpdateStatus::Installing;
                    self.status = "Preparing to restart and install the update".to_string();
                    iced::exit()
                }
                Err(error) => {
                    self.app_update_status = AppUpdateStatus::Failed(error.clone());
                    self.status = format!("Update failed: {error}");
                    Task::none()
                }
            },
        }
    }

    fn load_active_tab(&mut self) -> Task<Message> {
        match self.active_tab {
            Tab::Accounts if !self.account_ranks_loading => self.fetch_account_ranks_task(),
            Tab::Shop
                if self.store_summary.is_none() && !self.selected_account_is_store_loading() =>
            {
                self.fetch_storefront_task()
            }
            Tab::Loadout
                if self.loadout_summary.is_none()
                    && !self.selected_account_is_loadout_loading() =>
            {
                self.fetch_loadout_task()
            }
            _ => Task::none(),
        }
    }

    fn fetch_account_ranks_task(&mut self) -> Task<Message> {
        if self.state.accounts.is_empty() {
            return Task::none();
        }

        if self.client_version_input.trim().is_empty() {
            return Task::none();
        }

        self.account_ranks_loading = true;
        self.status = "Loading account details".to_string();
        let accounts = self.state.accounts.clone();
        let client_version = self.client_version_input.clone();

        Task::perform(
            fetch_account_ranks(accounts, client_version),
            Message::AccountRanksLoaded,
        )
    }

    fn fetch_storefront_task(&mut self) -> Task<Message> {
        let Some(account) = self.state.selected_account().cloned() else {
            self.status = "Select an account before opening the shop".to_string();
            return Task::none();
        };

        self.store_loading = true;
        self.store_loading_account = Some(account.id);
        self.status = "Loading shop".to_string();
        let image_cache = self.image_cache.clone();
        let account_id = account.id;
        Task::perform(
            fetch_storefront(account, self.client_version_input.clone(), image_cache),
            move |result| Message::StorefrontLoaded(account_id, result),
        )
    }

    fn fetch_loadout_task(&mut self) -> Task<Message> {
        let Some(account) = self.state.selected_account().cloned() else {
            self.status = "Select an account before opening loadout".to_string();
            return Task::none();
        };

        self.loadout_loading = true;
        self.loadout_loading_account = Some(account.id);
        self.status = "Loading loadout".to_string();
        let image_cache = self.image_cache.clone();
        let account_id = account.id;
        Task::perform(
            fetch_loadout(account, self.client_version_input.clone(), image_cache),
            move |result| Message::LoadoutLoaded(account_id, result),
        )
    }

    fn selected_account_is_store_loading(&self) -> bool {
        self.store_loading_account
            .is_some_and(|account_id| Some(account_id) == self.state.selected_account)
    }

    fn selected_account_is_loadout_loading(&self) -> bool {
        self.loadout_loading_account
            .is_some_and(|account_id| Some(account_id) == self.state.selected_account)
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

    fn load_image_viewer_source_task(&self, source: ImageViewerSource) -> Task<Message> {
        let cache = self.image_cache.clone();
        let source_for_load = source.clone();

        Task::perform(
            async move {
                cache
                    .cache_url(
                        &source_for_load.namespace,
                        &source_for_load.id,
                        &source_for_load.url,
                    )
                    .await
                    .map_err(|error| error.to_string())
            },
            move |result| Message::ImageViewerImageLoaded(source, result),
        )
    }

    fn restore_active_tab_scroll_task(&self) -> Task<Message> {
        operation::scroll_to(
            MAIN_PANEL_SCROLLABLE_ID,
            self.tab_scroll_offsets.get(self.active_tab),
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
            let summary = account.summary();

            if let Err(error) = account.attach_launcher_session(captured.backup) {
                self.status = format!("Launcher session rejected: {error}");
                return Task::none();
            }

            self.status = format!("Captured launcher session for {summary} ({captured_puuid})");
            return Task::batch([self.save_task(), self.load_active_tab()]);
        }

        self.status = match remove_launcher_session_backup(
            self.repo.launcher_backups_dir(),
            captured.account_id,
        ) {
            Ok(()) => "Captured launcher session, but the profile no longer exists".to_string(),
            Err(error) => format!(
                "Captured launcher session, but the profile no longer exists and cleanup failed: {error}"
            ),
        };
        Task::none()
    }
}

fn alert_and_focus_latest_window() -> Task<Message> {
    window::latest().then(|id| {
        id.map_or_else(Task::none, |id| {
            Task::batch([
                window::request_user_attention(id, Some(window::UserAttention::Informational)),
                window::gain_focus(id),
            ])
        })
    })
}
