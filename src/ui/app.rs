use iced::widget::operation;
use iced::{Task, window};
use std::path::Path;

use crate::account::{
    AccountId, AccountPenaltyStatus, AccountProfile, AuthSession, CompetitiveRank,
    LauncherSessionBackup,
};
use crate::account_transfer::{export_account, import_account};
use crate::image_cache::{CacheUsage, ImageCache};
use crate::launch::{LaunchConfig, LaunchTargetProcess};
use crate::riot::auth::{RedirectTokens, parse_redirect_tokens};
use crate::riot::launcher_session::{
    CapturedLauncherSession, adopt_launcher_session_backup, remove_launcher_session_backup,
};
use crate::secret_clipboard::copy_secret_text;
use crate::storage::{AccountRepository, StoredState};
use crate::updater::{UpdateCheckOutcome, check_for_update, download_and_prepare_update};

use super::data::account_details::{
    AccountActivityCheck, AccountAvailability, AccountRankResult, RefreshedApiContext,
    check_settings_activity, fetch_account_availabilities, fetch_account_availability,
    fetch_account_ranks, fetch_profile_identity,
};
use super::data::game_settings::{
    apply_game_settings_profile, delete_game_settings_profile, load_game_settings_profiles,
    rename_game_settings_profile, restore_original_game_settings, save_game_settings_profile,
};
use super::data::image_assets::{
    cache_player_card_art, cache_rank_icons, fetch_current_client_version,
};
use super::data::launch_flow::{
    PreviousAccountSync, check_riot_client_window_visible, finish_account_capture,
    finish_verified_launcher_session_login, launch_account, load_accounts, prepare_login_capture,
    start_current_account_capture, valorant_is_running,
};
use super::data::live_match::{LiveMatchError, fetch_live_match, pick_shown_weapon, shown_weapons};
use super::data::loadout::fetch_loadout;
use super::data::shop::fetch_storefront;
use super::data::{cache_account_api_context, typed_riot_client_path};
use super::{
    AccountsTab, AppUpdateStatus, ImageViewerImage, ImageViewerSource, LoadoutTab, LoginCapture,
    LoginCaptureTarget, MAIN_PANEL_SCROLLABLE_ID, Message, PendingSettingsChange,
    PendingSettingsCheck, PresetNamePrompt, PresetNameTarget, PrimeApp, SettingsChange,
    SettingsSection, Status, StatusKind, Tab, TabScrollOffsets, ViewRequest,
    background_refresh_active, screens,
};

impl PrimeApp {
    pub(super) fn boot() -> (Self, Task<Message>) {
        let repo = AccountRepository::new(AccountRepository::default_path());
        let image_cache = ImageCache::new(ImageCache::default_path());
        let load_repo = repo.clone();
        let profile_dir = repo.settings_profiles_dir();
        let cache_for_size = image_cache.clone();
        let image_cache_for_ranks = image_cache.clone();

        (
            Self {
                repo,
                image_cache,
                rank_icons: Default::default(),
                player_card_art: Default::default(),
                account_filter: String::new(),
                image_viewer: None,
                dialog_opened: None,
                bundle_details: None,
                state: StoredState::default(),
                accounts_loaded: false,
                active_tab: Tab::Accounts,
                active_accounts_tab: AccountsTab::Accounts,
                active_loadout_tab: LoadoutTab::Skins,
                tab_scroll_offsets: TabScrollOffsets::default(),
                new_display_name: String::new(),
                redirect_input: String::new(),
                settings_section: SettingsSection::RiotClient,
                token_import_open: false,
                client_version_input: String::new(),
                riot_client_path_input: String::new(),
                status: Status::progress("Loading accounts"),
                account_switcher_open: false,
                open_account_menu: None,
                show_add_account_prompt: false,
                show_import_account_prompt: false,
                import_account_input: String::new(),
                import_account_in_progress: false,
                exported_account: None,
                confirm_delete_account: None,
                confirm_recapture_account: None,
                capture_prompt_valorant_running: false,
                pending_account: None,
                store_summary: None,
                loadout_summary: None,
                store_request: None,
                loadout_request: None,
                store_error: None,
                loadout_error: None,
                live_match: None,
                live_match_request: None,
                live_match_in_flight: false,
                live_match_error: None,
                show_hidden_details: false,
                next_request_id: 0,
                profile_identity_refreshing: Default::default(),
                account_ranks_loading: Default::default(),
                unranked_accounts: Default::default(),
                rank_errors: Default::default(),
                account_details_loaded_at: None,
                account_availability: Default::default(),
                account_availability_loading: false,
                account_availability_loaded_at: None,
                account_availability_checked_at: Default::default(),
                save_settings_on_add: false,
                settings_profiles: Vec::new(),
                expanded_presets: std::collections::HashSet::new(),
                open_preset_menu: None,
                open_weapon_picker: None,
                selected_preset: None,
                preset_name_prompt: None,
                settings_saving_account: None,
                settings_applying_account: None,
                settings_check: None,
                confirm_settings_change: None,
                confirm_delete_settings_profile: None,
                launcher_capture_in_progress: false,
                launcher_capture_kind: None,
                login_capture: None,
                launch_preflight_account: None,
                unavailable_launch_warning: None,
                launching_account: None,
                launch_progress_checking: false,
                launch_client_open: false,
                window_minimized: false,
                status_changed_at: iced::time::Instant::now(),
                toast_appeared_at: iced::time::Instant::now(),
                dialog_closed_at: None,
                app_update_status: AppUpdateStatus::Checking,
                image_cache_usage: CacheUsage::default(),
                image_cache_clearing: false,
                loading_frame: 0,
                now: iced::time::Instant::now(),
            },
            Task::batch([
                Task::perform(async move { load_accounts(&load_repo) }, Message::Loaded),
                Task::perform(
                    load_game_settings_profiles(profile_dir),
                    Message::GameSettingsProfilesLoaded,
                ),
                fetch_client_version_task(false),
                cache_rank_icons_task(&image_cache_for_ranks),
                Task::perform(check_for_update(), |result| Message::AppUpdateChecked {
                    user_requested: false,
                    result: result.map_err(|error| error.to_string()),
                }),
                Task::perform(
                    async move {
                        cache_for_size
                            .remove_unused()
                            .map_err(|error| error.to_string())
                    },
                    Message::ImageCacheSizeLoaded,
                ),
            ]),
        )
    }

    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.handle_message(message);

        // Popovers are drawn above everything, including a dialog's scrim, so they close when a
        // dialog opens.
        if self.dialog_open() {
            self.close_popovers();
        }

        // A dialog that opens, or replaces another, starts its entrance; one that closes leaves
        // its backdrop to fade out.
        let dialog = self.open_dialog();
        if dialog != self.dialog_opened.map(|(open, _)| open) {
            let now = iced::time::Instant::now();
            self.dialog_closed_at = dialog.is_none().then_some(now);
            self.dialog_opened = dialog.map(|dialog| (dialog, now));
        }

        task
    }

    fn dialog_open(&self) -> bool {
        self.open_dialog().is_some()
            || (super::image_viewer_enabled() && self.image_viewer.is_some())
    }

    /// The dialog the shell shows, when one is open. Only one shows at a time; earlier ones win.
    pub(super) fn open_dialog(&self) -> Option<super::Dialog> {
        use super::Dialog;
        [
            (self.show_add_account_prompt, Dialog::AddAccount),
            (self.login_capture.is_some(), Dialog::LoginCapture),
            (self.pending_account.is_some(), Dialog::CapturedAccount),
            (self.show_import_account_prompt, Dialog::ImportAccount),
            (self.exported_account.is_some(), Dialog::ExportAccount),
            (self.confirm_delete_account.is_some(), Dialog::DeleteAccount),
            (self.confirm_recapture_account.is_some(), Dialog::Recapture),
            (
                self.confirm_settings_change.is_some(),
                Dialog::SettingsChange,
            ),
            (
                self.confirm_delete_settings_profile.is_some(),
                Dialog::DeleteSettingsProfile,
            ),
            (self.preset_name_prompt.is_some(), Dialog::PresetName),
            (
                self.unavailable_launch_warning.is_some(),
                Dialog::UnavailableLaunch,
            ),
            (
                self.app_update_status.prompt_update().is_some(),
                Dialog::AppUpdate,
            ),
            (self.open_bundle_details().is_some(), Dialog::BundleDetails),
        ]
        .into_iter()
        .find_map(|(open, dialog)| open.then_some(dialog))
    }

    /// The featured bundle whose details are open, while the shop still has it.
    pub(super) fn open_bundle_details(&self) -> Option<&super::data::shop::StoreBundleDisplay> {
        let store_id = self.bundle_details.as_ref()?;
        self.store_summary
            .as_ref()?
            .featured_bundles
            .iter()
            .find(|bundle| &bundle.store_id == store_id)
    }

    /// What Escape does: cancel the dialog on top, in the order the view stacks them, or else close
    /// an open popover.
    fn escape_message(&self) -> Option<Message> {
        let message = if super::image_viewer_enabled() && self.image_viewer.is_some() {
            Message::CloseImageViewer
        } else if self.show_add_account_prompt {
            Message::CancelAddAccountCapture
        } else if let Some(capture) = &self.login_capture {
            // Like the dialog's Cancel, only once Riot Client is open and waiting for a sign-in.
            capture.wait.as_ref()?;
            Message::CancelLoginCapture
        } else if self.pending_account.is_some() {
            // Cancelling throws the captured login away, so it takes the button, not a stray key.
            return None;
        } else if self.show_import_account_prompt {
            Message::CancelImportAccount
        } else if self.exported_account.is_some() {
            Message::CloseAccountExport
        } else if self.confirm_delete_account.is_some() {
            Message::CancelDeleteAccount
        } else if self.confirm_recapture_account.is_some() {
            Message::CancelLauncherSessionLogin
        } else if self.confirm_settings_change.is_some() {
            Message::CancelSettingsChange
        } else if self.confirm_delete_settings_profile.is_some() {
            Message::CancelDeleteSettingsProfile
        } else if self.preset_name_prompt.is_some() {
            Message::CancelPresetName
        } else if self.unavailable_launch_warning.is_some() {
            Message::CancelUnavailableLaunch
        } else if self.app_update_status.prompt_update().is_some() {
            Message::DismissAppUpdate
        } else if self.open_bundle_details().is_some() {
            Message::CloseBundleDetails
        } else if self.account_switcher_open
            || self.open_account_menu.is_some()
            || self.open_preset_menu.is_some()
            || self.open_weapon_picker.is_some()
        {
            Message::DismissPopovers
        } else if self.settings_check.is_some() {
            Message::CancelSettingsChange
        } else if self.status.kind == StatusKind::Error && super::status_bar_visible(self) {
            Message::DismissStatus
        } else {
            return None;
        };

        Some(message)
    }

    fn close_popovers(&mut self) {
        self.account_switcher_open = false;
        self.open_account_menu = None;
        self.open_preset_menu = None;
        self.open_weapon_picker = None;
    }

    /// Shows a status message. Setting the same text again restarts its display time, so a
    /// repeated action still gets visible feedback.
    fn set_status(&mut self, status: Status) {
        self.now = iced::time::Instant::now();
        if !super::status_bar_visible(self) {
            self.toast_appeared_at = self.now;
        }
        self.status = status;
        self.status_changed_at = self.now;
    }

    /// Ends a progress toast whose result shows on screen by itself. Anything else, such as an
    /// error, stays.
    fn clear_progress_status(&mut self) {
        if self.status.kind == StatusKind::Progress && !self.progress_pinned() {
            self.status = Status::default();
        }
    }

    /// Shows a status from background work the user didn't ask for, unless an error is on screen;
    /// errors stay until something the user does replaces them.
    fn set_background_status(&mut self, status: Status) {
        if self.status.kind != StatusKind::Error {
            self.set_status(status);
        }
    }

    /// Shows Shop and Loadout load progress and results, unless launch or login capture progress
    /// is pinned in the status bar; the tab itself shows its loading line and errors.
    fn set_view_status(&mut self, status: Status) {
        if !self.progress_pinned() {
            self.set_status(status);
        }
    }

    fn handle_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loaded(result) => {
                match result {
                    Ok(loaded) => {
                        self.riot_client_path_input = loaded
                            .state
                            .riot_client_path
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_default();
                        self.state = loaded.state;
                        self.accounts_loaded = true;
                        if let Some(error) = loaded.legacy_cleanup_error {
                            self.set_status(Status::error(format!(
                                "Could not remove old Riot Client sessions: {error}"
                            )));
                        } else if !loaded.removed_legacy_sessions.is_empty() {
                            self.set_status(Status::warning(format!(
                                "Removed outdated Riot Client sessions for {}; re-capture their login",
                                loaded.removed_legacy_sessions.join(", ")
                            )));
                        } else {
                            self.clear_progress_status();
                        }
                    }
                    Err(error) => {
                        self.set_status(Status::error(format!(
                            "Failed to load accounts: {error}. Changes will not be saved until accounts.json loads."
                        )));
                    }
                }

                Task::batch([self.load_active_tab(), self.player_card_art_task()])
            }
            Message::Saved(result) => {
                if let Err(error) = result {
                    self.set_status(Status::error(format!("Failed to save accounts: {error}")));
                }

                Task::none()
            }
            Message::TabSelected(tab) => {
                self.active_tab = tab;
                // Countdowns don't tick on other tabs, so bring them up to date.
                self.now = iced::time::Instant::now();
                self.image_viewer = None;
                self.bundle_details = None;
                self.open_weapon_picker = None;
                self.close_account_surfaces();
                self.unavailable_launch_warning = None;
                Task::batch([
                    self.load_active_tab(),
                    self.restore_active_tab_scroll_task(),
                ])
            }
            Message::AccountsTabSelected(tab) => {
                self.active_accounts_tab = tab;
                self.close_account_surfaces();
                Task::none()
            }
            Message::LoadoutTabSelected(tab) => {
                self.active_loadout_tab = tab;
                self.now = iced::time::Instant::now();
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
                    self.close_account_action_surfaces();
                }

                Task::none()
            }
            Message::DismissStatus => {
                self.status = Status::default();
                Task::none()
            }
            Message::DismissPopovers => {
                self.close_popovers();
                Task::none()
            }
            Message::SelectAccount(id) => {
                if !self.state.select_account(id) {
                    self.account_switcher_open = false;
                    self.set_status(Status::error("Account profile no longer exists"));
                    return Task::none();
                }

                self.image_viewer = None;
                self.close_account_surfaces();
                self.unavailable_launch_warning = None;
                self.clear_selected_account_views();
                // Ends a "loading updated shop" toast for the account just left.
                self.clear_progress_status();
                Task::batch([self.save_task(), self.load_account_tab(id)])
            }
            Message::NewDisplayNameChanged(value) => {
                self.new_display_name = value;
                Task::none()
            }
            Message::SaveSettingsOnAddToggled(save) => {
                self.save_settings_on_add = save;
                Task::none()
            }
            Message::AddAccount => {
                if self.login_capture_blocked() {
                    return Task::none();
                }

                self.close_account_surfaces();
                self.show_add_account_prompt = true;
                self.set_status(Status::info(
                    "Before Riot Client opens, confirm that you will tick Stay signed in.",
                ));

                check_capture_prompt_game_task()
            }
            Message::AddCurrentAccount => {
                if self.login_capture_blocked() {
                    return Task::none();
                }

                let account_id = AccountId::new();
                let backup_root = self.repo.launcher_backups_dir();
                let discarded = self.discard_pending_account();
                self.close_account_surfaces();
                self.new_display_name.clear();
                self.set_status(Status::progress(if discarded {
                    "Discarded the unsaved captured account; capturing the Riot account currently signed in"
                } else {
                    "Capturing the Riot account currently signed in"
                }));
                self.launcher_capture_in_progress = true;
                self.launcher_capture_kind = Some(super::LauncherCaptureKind::Current);

                Task::perform(
                    async move { start_current_account_capture(account_id, backup_root).await },
                    Message::CurrentAccountCaptureFinished,
                )
            }
            Message::ConfirmAddAccountCapture => {
                if self.login_capture_blocked() {
                    return Task::none();
                }

                let discarded = self.discard_pending_account();
                self.close_account_surfaces();
                self.new_display_name.clear();
                self.set_status(Status::progress(format!(
                    "{}Opening Riot Client. When it appears, sign in normally with \"Stay signed in\" ticked.",
                    if discarded {
                        "Discarded the unsaved captured account. "
                    } else {
                        ""
                    }
                )));
                self.start_login_capture(LoginCaptureTarget::NewAccount(AccountId::new()))
            }
            Message::LoginCapturePrepared { target, result } => {
                self.handle_login_capture_prepared(target, result)
            }
            Message::CancelLoginCapture => self.cancel_login_capture(),
            Message::CapturePromptGameChecked(valorant_running) => {
                self.capture_prompt_valorant_running = valorant_running
                    && (self.show_add_account_prompt || self.confirm_recapture_account.is_some());
                Task::none()
            }
            Message::CancelAddAccountCapture => {
                self.show_add_account_prompt = false;
                self.capture_prompt_valorant_running = false;
                Task::none()
            }
            Message::AccountCaptureFinished(result) => {
                let slot = result.as_ref().ok().map(|draft| draft.account_id);
                if !self.finish_login_capture(slot) {
                    return Task::none();
                }

                match result {
                    Ok(draft) => {
                        self.new_display_name = draft
                            .game_name
                            .clone()
                            .unwrap_or_else(|| "New account".to_string());
                        // The confirm dialog opens, so the capture's progress toast ends.
                        self.clear_progress_status();
                        self.pending_account = Some(draft);
                        Task::batch([
                            self.show_accounts_tab_top(),
                            alert_and_focus_latest_window(),
                        ])
                    }
                    Err(error) => {
                        self.set_status(Status::error(format!("Could not add account: {error}")));
                        Task::none()
                    }
                }
            }
            Message::CurrentAccountCaptureFinished(result) => {
                self.launcher_capture_in_progress = false;
                self.launcher_capture_kind = None;

                match result {
                    Ok(draft) => {
                        if let Some(existing_id) =
                            self.state
                                .accounts
                                .iter()
                                .find(|account| {
                                    account.puuid.as_ref().is_some_and(|puuid| {
                                        puuid.eq_ignore_ascii_case(&draft.puuid)
                                    })
                                })
                                .map(|account| account.id)
                        {
                            return Task::batch([
                                self.update_existing_captured_account(existing_id, draft),
                                alert_and_focus_latest_window(),
                            ]);
                        }

                        self.new_display_name = draft
                            .game_name
                            .clone()
                            .unwrap_or_else(|| "New account".to_string());
                        self.clear_progress_status();
                        self.pending_account = Some(draft);
                        Task::batch([
                            self.show_accounts_tab_top(),
                            alert_and_focus_latest_window(),
                        ])
                    }
                    Err(error) => {
                        self.set_status(Status::error(format!(
                            "Could not add current account: {error}"
                        )));
                        Task::none()
                    }
                }
            }
            Message::ConfirmCapturedAccount => self.confirm_captured_account(),
            Message::CancelCapturedAccount => {
                let Some(draft) = self.pending_account.as_ref() else {
                    self.set_status(Status::error(
                        "No captured account is waiting to be discarded",
                    ));
                    return Task::none();
                };

                if let Err(error) = remove_launcher_session_backup(
                    self.repo.launcher_backups_dir(),
                    draft.account_id,
                ) {
                    self.set_status(Status::error(format!(
                        "Could not discard captured account session: {error}"
                    )));
                    return Task::none();
                }

                self.pending_account = None;
                self.close_account_surfaces();
                self.new_display_name.clear();
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
                    self.set_status(Status::error("Account profile no longer exists"));
                    return Task::none();
                };

                let account_id = account.id;
                let display_name = account.display_name.clone();
                let summary = account.summary();
                self.close_account_surfaces();
                self.set_status(Status::progress(format!("Exporting {summary}")));

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
                        self.set_status(Status::success(format!(
                            "Prepared account export for {}",
                            export.display_name
                        )));
                        self.exported_account = Some(export);
                    }
                    Err(error) => {
                        self.set_status(Status::error(format!(
                            "Could not export account: {error}"
                        )));
                    }
                }

                Task::none()
            }
            Message::CopyAccountExport => {
                let Some(export) = &self.exported_account else {
                    self.set_status(Status::error(
                        "Could not export account: no export is ready",
                    ));
                    return Task::none();
                };

                let display_name = export.display_name.clone();
                let payload = export.payload.clone();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || copy_secret_text(&payload))
                            .await
                            .map_err(|error| error.to_string())?
                            .map_err(|error| error.to_string())
                    },
                    move |result| Message::AccountExportCopied(display_name.clone(), result),
                )
            }
            Message::AccountExportCopied(display_name, result) => {
                self.set_status(match result {
                    Ok(()) => Status::success(format!(
                        "Copied account export for {display_name}; it is kept out of clipboard history"
                    )),
                    Err(error) => Status::error(format!("Could not copy account export: {error}")),
                });
                Task::none()
            }
            Message::CloseAccountExport => {
                self.exported_account = None;
                Task::none()
            }
            Message::OpenImportAccount => {
                self.close_account_surfaces();
                self.show_import_account_prompt = true;
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
                    self.set_status(Status::progress("Importing account"));
                    return Task::none();
                }

                self.show_import_account_prompt = false;
                self.import_account_input.clear();
                Task::none()
            }
            Message::ConfirmImportAccount => {
                if self.import_account_in_progress || self.update_blocks_new_work() {
                    return Task::none();
                }

                if self.import_account_input.trim().is_empty() {
                    self.set_status(Status::error(
                        "Could not import account: paste an account export first",
                    ));
                    return Task::none();
                }

                let input = self.import_account_input.clone();
                let backup_root = self.repo.launcher_backups_dir();
                let existing_accounts = self.state.accounts.clone();

                self.import_account_in_progress = true;
                self.set_status(Status::progress("Importing account"));

                Task::perform(
                    async move {
                        import_account(&input, backup_root, &existing_accounts)
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
                            self.set_status(Status::error(
                                "Could not import account: imported account ID already exists",
                            ));
                            return Task::none();
                        }

                        self.state.push_account(imported.account);
                        self.account_availability.remove(&account_id);
                        self.state.select_account(account_id);
                        self.show_import_account_prompt = false;
                        self.import_account_input.clear();
                        self.clear_selected_account_views();
                        self.set_status(Status::success(format!("Imported {summary}{id_note}")));
                        return Task::batch([self.save_task(), self.load_account_tab(account_id)]);
                    }
                    Err(error) => {
                        self.set_status(Status::error(format!(
                            "Could not import account: {error}"
                        )));
                    }
                }

                Task::none()
            }
            Message::RequestDeleteAccount(id) => {
                if self.state.accounts.iter().any(|account| account.id == id) {
                    self.close_account_surfaces();
                    self.confirm_delete_account = Some(id);
                } else {
                    self.close_account_surfaces();
                    self.set_status(Status::error("Account profile no longer exists"));
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
                    self.set_status(Status::error("Account profile no longer exists"));
                    return Task::none();
                };

                if let Err(error) =
                    remove_launcher_session_backup(self.repo.launcher_backups_dir(), id)
                {
                    self.open_account_menu = None;
                    self.set_status(Status::error(format!(
                        "Could not delete captured launcher session for {}: {error}",
                        account.summary()
                    )));
                    return Task::none();
                }

                let was_selected = self.state.selected_account == Some(id);
                self.state.remove_account(id);
                self.account_availability.remove(&id);
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
                    self.clear_selected_account_views();
                }

                self.set_status(Status::success(format!("Deleted {}", account.summary())));
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
                self.set_status(Status::progress("Refreshing Riot client version"));
                fetch_client_version_task(true)
            }
            Message::RetryClientVersion => {
                if self.client_version_input.trim().is_empty() {
                    fetch_client_version_task(false)
                } else {
                    Task::none()
                }
            }
            Message::ClientVersionLoaded {
                user_requested,
                result,
            } => match result {
                Ok(version) => {
                    // The automatic lookup only fills an empty field; a manual refresh replaces it.
                    if user_requested || self.client_version_input.trim().is_empty() {
                        self.client_version_input = version.clone();
                    }
                    // The field shows the version.
                    if user_requested {
                        self.clear_progress_status();
                    }

                    self.load_active_tab()
                }
                Err(error) => {
                    let status =
                        Status::error(format!("Could not fetch Riot client version: {error}"));

                    if user_requested {
                        self.set_status(status);
                        return Task::none();
                    }

                    // Rank, level, availability, Shop and Loadout all need the version, so keep
                    // trying in the background.
                    self.set_background_status(status);
                    Task::perform(
                        async { tokio::time::sleep(super::CLIENT_VERSION_RETRY_INTERVAL).await },
                        |()| Message::RetryClientVersion,
                    )
                }
            },
            Message::ImportRedirect => {
                let Some(account) = self.state.selected_account_mut() else {
                    self.account_switcher_open = false;
                    self.set_status(Status::error("Select an account before importing a token"));
                    return Task::none();
                };

                match parse_redirect_tokens(&self.redirect_input)
                    .map_err(|error| error.to_string())
                    .and_then(|tokens| redirect_session_for_account(account, tokens))
                {
                    Ok(session) => {
                        let account_id = account.id;
                        let summary = account.summary();
                        account.session = Some(session);
                        self.redirect_input.clear();
                        self.set_status(Status::success(format!(
                            "Imported Riot redirect token for {summary}"
                        )));
                        Task::batch([self.save_task(), self.load_account_tab(account_id)])
                    }
                    Err(error) => {
                        self.set_status(Status::error(format!(
                            "Could not import redirect token: {error}"
                        )));
                        Task::none()
                    }
                }
            }
            Message::RequestLauncherSessionLogin(account_id) => {
                if self.login_capture_blocked() {
                    return Task::none();
                }

                self.close_account_surfaces();

                if self
                    .state
                    .accounts
                    .iter()
                    .any(|account| account.id == account_id)
                {
                    self.confirm_recapture_account = Some(account_id);
                    check_capture_prompt_game_task()
                } else {
                    self.set_status(Status::error("Account profile no longer exists"));
                    Task::none()
                }
            }
            Message::CancelLauncherSessionLogin => {
                self.confirm_recapture_account = None;
                self.capture_prompt_valorant_running = false;
                Task::none()
            }
            Message::StartLauncherSessionLogin(account_id) => {
                self.confirm_recapture_account = None;
                self.capture_prompt_valorant_running = false;

                if self.login_capture_blocked() {
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
                    self.set_status(Status::error("Account profile no longer exists"));
                    return Task::none();
                };

                let summary = account.summary();
                self.close_account_surfaces();
                self.set_status(Status::progress(format!(
                    "Opening Riot Client and waiting for remembered login capture for {summary}"
                )));
                // Capture into a separate slot so the account's working backup is only replaced
                // after the login is confirmed to belong to this account.
                self.start_login_capture(LoginCaptureTarget::Existing {
                    account_id,
                    staging_id: AccountId::new(),
                })
            }
            Message::LauncherSessionLoginStarted(account_id, result) => {
                let slot = result.as_ref().ok().map(|captured| captured.account_id);
                if !self.finish_login_capture(slot) {
                    return Task::none();
                }

                // The user is signing in to Riot Client, so bring Prime back with the result.
                let stored = match result {
                    Ok(captured) => self.store_captured_launcher_session(account_id, captured),
                    Err(error) => {
                        self.set_status(Status::error(format!(
                            "Could not complete launcher session login: {error}"
                        )));
                        Task::none()
                    }
                };

                Task::batch([stored, alert_and_focus_latest_window()])
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
                    self.set_status(Status::error("Account profile no longer exists"));
                    return Task::none();
                };

                let summary = account.summary();
                self.close_account_surfaces();
                self.set_status(Status::progress(format!(
                    "Refreshing Riot profile identity for {summary}"
                )));
                if !self.profile_identity_refreshing.insert(account_id) {
                    return Task::none();
                }

                Task::perform(fetch_profile_identity(account), move |result| {
                    Message::ProfileIdentityLoaded(account_id, result)
                })
            }
            Message::ProfileIdentityLoaded(account_id, result) => {
                self.profile_identity_refreshing.remove(&account_id);

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
                                self.set_status(Status::error(format!(
                                    "Profile identity rejected: {error}"
                                )));
                                return Task::none();
                            }

                            account.session = Some(identity.session);
                            // Refresh re-reads who the account is; its region is looked up again
                            // through Riot Geo on the next request, in case it moved.
                            account.region = None;
                            if let Some(launcher_session) = identity.launcher_session {
                                account.launcher_session = Some(launcher_session);
                            }
                            let summary = account.summary();
                            self.set_status(Status::success(format!("Refreshed {summary}")));
                            return Task::batch([
                                self.save_task(),
                                self.load_account_tab(identity.account_id),
                            ]);
                        }

                        self.set_status(Status::error(
                            "Refreshed profile identity, but the selected profile no longer exists",
                        ));
                    }
                    Err(error) => {
                        self.set_status(Status::error(format!("Profile refresh failed: {error}")));
                    }
                }

                Task::none()
            }
            Message::AccountRanksLoaded { result, announce } => {
                self.account_ranks_loading.clear();

                let mut updated = 0usize;
                let mut partial = 0usize;
                let mut context_failures = 0usize;

                for rank in result.ranks {
                    let AccountRankResult {
                        account_id,
                        rank,
                        account_level,
                        penalty_status,
                        session,
                        launcher_session,
                        identity,
                    } = rank;

                    if cache_account_api_context(
                        &mut self.state,
                        account_id,
                        session,
                        launcher_session,
                        identity,
                    )
                    .is_err()
                    {
                        context_failures += 1;
                        continue;
                    }

                    if matches!(rank, Ok(None)) {
                        self.unranked_accounts.insert(account_id);
                    } else {
                        self.unranked_accounts.remove(&account_id);
                    }
                    match &rank {
                        Err(error) => {
                            self.rank_errors.insert(account_id, error.clone());
                        }
                        Ok(_) => {
                            self.rank_errors.remove(&account_id);
                        }
                    }

                    if let Some(account) = self
                        .state
                        .accounts
                        .iter_mut()
                        .find(|account| account.id == account_id)
                    {
                        let update = apply_account_detail_results(
                            account,
                            rank,
                            account_level,
                            penalty_status,
                        );

                        if update.updated {
                            updated += 1;
                        }

                        if update.partial {
                            partial += 1;
                        }
                    }
                }

                let failed = result.failures.len() + context_failures;
                // Details that loaded in full show on screen, so only a problem gets a toast.
                let status = match (updated, failed, partial) {
                    (_, 0, 0) => None,
                    (0, failed, _) => Some(Status::error(format!(
                        "Account detail refresh failed for {failed} account(s)"
                    ))),
                    (updated, 0, partial) => Some(Status::warning(format!(
                        "Loaded account details for {updated} account(s); {partial} partial"
                    ))),
                    (updated, failed, 0) => Some(Status::warning(format!(
                        "Loaded account details for {updated} account(s); {failed} unavailable"
                    ))),
                    (updated, failed, partial) => Some(Status::warning(format!(
                        "Loaded account details for {updated} account(s); {failed} unavailable, {partial} partial"
                    ))),
                };
                if announce && !self.progress_pinned() {
                    match status {
                        Some(status) => self.set_status(status),
                        None => self.clear_progress_status(),
                    }
                }

                if updated > 0 {
                    self.save_task()
                } else {
                    Task::none()
                }
            }
            Message::AccountAvailabilityTimerTick(now) => {
                self.now = now;

                if self.active_tab == Tab::LiveMatch && !background_refresh_active(self) {
                    return self.poll_live_match();
                }
                let polling = self.active_tab == Tab::Accounts || background_refresh_active(self);
                if !polling || self.availability_poll_blocked() {
                    return Task::none();
                }

                self.fetch_account_availabilities_task()
            }
            Message::AnimationFrame(now) => {
                self.now = now;
                Task::none()
            }
            Message::StatusTimerTick(now) => {
                self.now = now;
                Task::none()
            }
            Message::WindowResized(size) => {
                // Windows reports a minimized window as resized to zero.
                let minimized = size.width <= 0.0 || size.height <= 0.0;
                if self.window_minimized && !minimized {
                    return self.window_shown();
                }
                let just_minimized = minimized && !self.window_minimized;
                self.window_minimized = minimized;

                if just_minimized {
                    trim_memory()
                } else {
                    Task::none()
                }
            }
            Message::CloseRequested(id) => {
                if !self.state.minimize_on_close {
                    return iced::exit();
                }

                // Hidden, the window leaves the taskbar and lives on as the tray icon.
                match super::tray::show() {
                    Ok(()) => {
                        // Hiding sends no resize, so nothing else marks the window as out of sight.
                        self.window_minimized = true;
                        window::set_mode(id, window::Mode::Hidden).chain(trim_memory())
                    }
                    Err(error) => {
                        self.set_status(Status::error(format!(
                            "{error}; minimized to the taskbar instead"
                        )));
                        window::minimize(id, true)
                    }
                }
            }
            Message::Tray(super::tray::TrayAction::Open) => {
                super::tray::remove();
                let shown = self.window_shown();
                Task::batch([
                    window::latest().then(|id| {
                        id.map_or_else(Task::none, |id| {
                            Task::batch([
                                window::set_mode(id, window::Mode::Windowed),
                                window::minimize(id, false),
                                window::gain_focus(id),
                            ])
                        })
                    }),
                    shown,
                ])
            }
            Message::Tray(super::tray::TrayAction::Quit) => {
                super::tray::remove();
                iced::exit()
            }
            Message::MinimizeOnCloseToggled(enabled) => {
                // Loading accounts.json would undo a change made before it arrives.
                if !self.accounts_loaded {
                    return Task::none();
                }
                self.state.minimize_on_close = enabled;
                self.save_task()
            }
            Message::LiveMatchWeaponPicked { column, weapon } => {
                // Loading accounts.json would undo a change made before it arrives.
                if !self.accounts_loaded {
                    return Task::none();
                }
                self.open_weapon_picker = None;
                self.state.live_match_weapons =
                    pick_shown_weapon(self.state.live_match_weapons.as_deref(), column, weapon);
                self.save_task()
            }
            Message::ToggleWeaponPicker(column) => {
                self.open_weapon_picker =
                    (self.open_weapon_picker != Some(column)).then_some(column);
                Task::none()
            }
            Message::ResetLiveMatchWeapons => {
                if !self.accounts_loaded {
                    return Task::none();
                }
                self.open_weapon_picker = None;
                self.state.live_match_weapons = None;
                self.save_task()
            }
            Message::AccountAvailabilitiesLoaded(result) => {
                self.account_availability_loading = false;

                let arrived_at = iced::time::Instant::now();
                for account in result.accounts {
                    if self
                        .state
                        .accounts
                        .iter()
                        .any(|profile| profile.id == account.account_id)
                    {
                        self.account_availability
                            .insert(account.account_id, account.availability);
                        self.account_availability_checked_at
                            .insert(account.account_id, arrived_at);
                    }
                }

                let mut cached_session = false;
                for refreshed in result.refreshed_sessions {
                    cached_session |= self.cache_refreshed_api_context(refreshed);
                }

                // Live Match, opened or shown again while this poll ran, waited for it rather than
                // signing in at the same time.
                let live_match = if self.active_tab == Tab::LiveMatch {
                    self.poll_live_match()
                } else {
                    Task::none()
                };

                if cached_session {
                    Task::batch([self.save_task(), live_match])
                } else {
                    live_match
                }
            }
            Message::GameSettingsProfilesLoaded(result) => {
                match result {
                    Ok(profiles) => self.settings_profiles = profiles,
                    Err(error) => {
                        self.set_status(Status::error(format!(
                            "Could not load settings profiles: {error}"
                        )));
                    }
                }

                Task::none()
            }
            Message::RequestSavePreset(account_id) => {
                if self.settings_work_in_progress() {
                    return Task::none();
                }

                let Some(account) = self
                    .state
                    .accounts
                    .iter()
                    .find(|account| account.id == account_id)
                else {
                    self.set_status(Status::error("Account profile no longer exists"));
                    return Task::none();
                };

                let name = default_preset_name(&account.display_name);
                self.close_account_surfaces();
                self.preset_name_prompt = Some(PresetNamePrompt {
                    target: PresetNameTarget::New(account_id),
                    name,
                });
                Task::none()
            }
            Message::TogglePresetMenu(profile_id) => {
                self.open_preset_menu = match self.open_preset_menu.take() {
                    Some(open) if open == profile_id => None,
                    _ => Some(profile_id),
                };
                Task::none()
            }
            Message::TogglePresetSettings(profile_id) => {
                self.open_preset_menu = None;
                if !self.expanded_presets.remove(&profile_id) {
                    self.expanded_presets.insert(profile_id);
                }
                Task::none()
            }
            Message::SelectPreset(profile_id) => {
                self.open_preset_menu = None;
                self.selected_preset = Some(profile_id);
                Task::none()
            }
            Message::RequestRenamePreset(profile_id) => {
                self.open_preset_menu = None;
                let Some(name) = self
                    .settings_profiles
                    .iter()
                    .find(|profile| profile.id == profile_id)
                    .map(|profile| profile.name.clone())
                else {
                    self.set_status(Status::error("Settings preset no longer exists"));
                    return Task::none();
                };

                self.close_account_surfaces();
                self.preset_name_prompt = Some(PresetNamePrompt {
                    target: PresetNameTarget::Rename(profile_id),
                    name,
                });
                Task::none()
            }
            Message::PresetNameChanged(name) => {
                if let Some(prompt) = &mut self.preset_name_prompt {
                    prompt.name = name;
                }
                Task::none()
            }
            Message::CancelPresetName => {
                self.preset_name_prompt = None;
                Task::none()
            }
            Message::ConfirmPresetName => {
                let Some(prompt) = self.preset_name_prompt.clone() else {
                    return Task::none();
                };
                let name = prompt.name.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }

                match prompt.target {
                    PresetNameTarget::New(account_id) => {
                        let task =
                            self.handle_message(Message::SaveSettingsPreset { account_id, name });
                        // Stays open when the save could not start, so the name isn't lost.
                        if self.settings_saving_account == Some(account_id) {
                            self.preset_name_prompt = None;
                        }
                        task
                    }
                    PresetNameTarget::Rename(profile_id) => {
                        self.preset_name_prompt = None;
                        Task::perform(
                            rename_game_settings_profile(
                                self.repo.settings_profiles_dir(),
                                profile_id,
                                name,
                            ),
                            Message::PresetRenamed,
                        )
                    }
                }
            }
            Message::PresetRenamed(result) => match result {
                Ok(renamed) => {
                    if let Some(profile) = self
                        .settings_profiles
                        .iter_mut()
                        .find(|profile| profile.id == renamed.id)
                    {
                        *profile = renamed.clone();
                    }
                    self.set_status(Status::success(format!(
                        "Renamed preset to {}",
                        renamed.name
                    )));
                    Task::none()
                }
                Err(error) => {
                    self.set_status(Status::error(format!("Could not rename preset: {error}")));
                    self.load_settings_profiles_task()
                }
            },
            Message::SaveSettingsPreset { account_id, name } => {
                if self.settings_work_in_progress() || self.update_blocks_new_work() {
                    return Task::none();
                }

                let Some(account) = self
                    .state
                    .accounts
                    .iter()
                    .find(|account| account.id == account_id)
                    .cloned()
                else {
                    self.open_account_menu = None;
                    self.account_switcher_open = false;
                    self.set_status(Status::error("Account profile no longer exists"));
                    return Task::none();
                };

                let summary = account.summary();
                let profile_dir = self.repo.settings_profiles_dir();
                self.close_account_surfaces();
                self.settings_saving_account = Some(account_id);
                self.set_status(Status::progress(format!(
                    "Saving {summary}'s settings as {name}"
                )));

                Task::perform(
                    save_game_settings_profile(account, profile_dir, name),
                    Message::AccountSettingsSaved,
                )
            }
            Message::AccountSettingsSaved(result) => {
                let result_account_id = result.as_ref().ok().map(|result| result.account_id);

                if result_account_id.is_none() || self.settings_saving_account == result_account_id
                {
                    self.settings_saving_account = None;
                }

                match result {
                    Ok(result) => {
                        // The profile file is saved either way, so list it before the account
                        // update that can still fail.
                        self.settings_profiles
                            .retain(|profile| profile.id != result.profile.id);
                        self.settings_profiles.insert(0, result.profile.clone());

                        if let Err(error) = cache_account_api_context(
                            &mut self.state,
                            result.account_id,
                            result.session,
                            result.launcher_session,
                            result.identity,
                        ) {
                            self.set_status(Status::error(format!(
                                "Saved preset {}, but account update failed: {error}",
                                result.profile.name
                            )));
                            return Task::none();
                        }

                        self.set_status(Status::success(format!(
                            "Saved preset {}",
                            result.profile.name
                        )));
                        Task::batch([self.save_task(), self.load_settings_profiles_task()])
                    }
                    Err(error) => {
                        self.set_status(Status::error(format!(
                            "Could not save account settings: {error}"
                        )));
                        Task::none()
                    }
                }
            }
            Message::EscapePressed => match self.escape_message() {
                Some(message) => self.handle_message(message),
                None => Task::none(),
            },
            Message::RequestDeleteSettingsProfile(profile_id) => {
                self.open_preset_menu = None;
                if self.settings_work_in_progress() {
                    return Task::none();
                }

                self.close_account_surfaces();
                if self
                    .settings_profiles
                    .iter()
                    .any(|profile| profile.id == profile_id)
                {
                    self.confirm_delete_settings_profile = Some(profile_id);
                } else {
                    self.set_status(Status::error("Settings profile no longer exists"));
                }

                Task::none()
            }
            Message::CancelDeleteSettingsProfile => {
                self.confirm_delete_settings_profile = None;
                Task::none()
            }
            Message::ConfirmDeleteSettingsProfile => {
                let Some(profile_id) = self.confirm_delete_settings_profile.take() else {
                    return Task::none();
                };
                let profile_dir = self.repo.settings_profiles_dir();

                Task::perform(
                    delete_game_settings_profile(profile_dir, profile_id.clone()),
                    move |result| Message::SettingsProfileDeleted(profile_id.clone(), result),
                )
            }
            Message::SettingsProfileDeleted(profile_id, result) => {
                let name = self
                    .settings_profiles
                    .iter()
                    .find(|profile| profile.id == profile_id)
                    .map(|profile| profile.name.clone())
                    .unwrap_or_else(|| "settings profile".to_string());

                match result {
                    Ok(()) => {
                        self.settings_profiles
                            .retain(|profile| profile.id != profile_id);
                        self.set_status(Status::success(format!("Deleted {name}")));
                        Task::none()
                    }
                    Err(error) => {
                        self.set_status(Status::error(format!(
                            "Could not delete settings profile {name}: {error}"
                        )));
                        self.load_settings_profiles_task()
                    }
                }
            }
            Message::RequestApplyPreset {
                profile_id,
                account_id,
            } => {
                if !self
                    .settings_profiles
                    .iter()
                    .any(|profile| profile.id == profile_id)
                {
                    self.set_status(Status::error("Settings preset no longer exists"));
                    return Task::none();
                }

                self.open_settings_change(SettingsChange::Apply {
                    account_id,
                    profile_id,
                })
            }
            Message::SavedSettingsApplied(result) => {
                let result_account_id = result.as_ref().ok().map(|result| result.account_id);

                if result_account_id.is_none()
                    || self.settings_applying_account == result_account_id
                {
                    self.settings_applying_account = None;
                }

                match result {
                    Ok(result) => {
                        if let Err(error) = cache_account_api_context(
                            &mut self.state,
                            result.account_id,
                            result.session,
                            result.launcher_session,
                            result.identity,
                        ) {
                            self.set_status(Status::error(format!(
                                "Applied saved settings, but profile update failed: {error}"
                            )));
                            return self.load_settings_profiles_task();
                        }

                        let put_aside = if result.backup_profile.is_some() {
                            ". Its own settings were saved; restore them from Game settings"
                        } else {
                            ""
                        };
                        self.set_status(Status::success(format!(
                            "Applied preset {}{put_aside}",
                            result.source_profile.name
                        )));
                        Task::batch([self.save_task(), self.load_settings_profiles_task()])
                    }
                    Err(error) => {
                        self.set_status(Status::error(format!(
                            "Could not apply account settings: {error}"
                        )));
                        // Apply may have saved its backup before failing.
                        self.load_settings_profiles_task()
                    }
                }
            }
            Message::RequestRestoreSettings(account_id) => {
                self.open_settings_change(SettingsChange::Restore(account_id))
            }
            Message::SettingsPreflightChecked(request_id, account_id, check, valorant_running) => {
                let mut task = Task::none();
                // The check only looks for the game, so its result stays out of the Accounts
                // tab's availability: it would show agent select as a lobby.
                let checked = check.map(|(check, refreshed)| {
                    // Goes through the same PUUID check as the background poll's sessions.
                    if let Some(refreshed) = refreshed
                        && self.cache_refreshed_api_context(refreshed)
                    {
                        task = self.save_task();
                    }
                    check.availability
                });

                // A canceled or superseded check opens nothing.
                let Some(pending) = self.settings_check.take_if(|pending| {
                    pending.request_id == request_id && pending.change.account_id() == account_id
                }) else {
                    return task;
                };
                let Some(display_name) = self
                    .state
                    .accounts
                    .iter()
                    .find(|account| account.id == account_id)
                    .map(|account| account.display_name.clone())
                else {
                    return task;
                };
                // Without a check, the poll's result was fresh.
                let availability = checked
                    .or_else(|| self.account_availability.get(&account_id).cloned())
                    .unwrap_or_else(AccountAvailability::activity_check_failed);
                self.confirm_settings_change = Some(PendingSettingsChange {
                    change: pending.change,
                    warning: settings_change_warning(
                        &display_name,
                        &availability,
                        valorant_running,
                    ),
                    check_failed: matches!(availability, AccountAvailability::Unknown { .. }),
                });

                task
            }
            Message::CancelSettingsChange => {
                self.settings_check = None;
                self.confirm_settings_change = None;
                Task::none()
            }
            Message::ConfirmSettingsChange => {
                let Some(pending) = self.confirm_settings_change.take() else {
                    return Task::none();
                };
                if self.settings_work_in_progress() || self.update_blocks_new_work() {
                    return Task::none();
                }

                let Some(account) = self
                    .state
                    .accounts
                    .iter()
                    .find(|account| account.id == pending.change.account_id())
                    .cloned()
                else {
                    self.set_status(Status::error("Account profile no longer exists"));
                    return Task::none();
                };

                let summary = account.summary();
                let profile_dir = self.repo.settings_profiles_dir();
                match pending.change {
                    SettingsChange::Apply { profile_id, .. } => {
                        let Some(profile_name) = self
                            .settings_profiles
                            .iter()
                            .find(|profile| profile.id == profile_id)
                            .map(|profile| profile.name.clone())
                        else {
                            self.set_status(Status::error(
                                "Could not apply account settings: that settings profile no longer exists",
                            ));
                            return Task::none();
                        };

                        self.settings_applying_account = Some(account.id);
                        self.set_status(Status::progress(format!(
                            "Applying {profile_name} to {summary}"
                        )));
                        Task::perform(
                            apply_game_settings_profile(account, profile_dir, profile_id),
                            Message::SavedSettingsApplied,
                        )
                    }
                    SettingsChange::Restore(_) => {
                        self.settings_applying_account = Some(account.id);
                        self.set_status(Status::progress(format!(
                            "Restoring {summary}'s original settings"
                        )));
                        Task::perform(
                            restore_original_game_settings(account, profile_dir),
                            Message::SettingsRestored,
                        )
                    }
                }
            }
            Message::SettingsRestored(result) => {
                let result_account_id = result.as_ref().ok().map(|result| result.account_id);

                if result_account_id.is_none()
                    || self.settings_applying_account == result_account_id
                {
                    self.settings_applying_account = None;
                }

                match result {
                    Ok(result) => {
                        if let Err(error) = cache_account_api_context(
                            &mut self.state,
                            result.account_id,
                            result.session,
                            result.launcher_session,
                            result.identity,
                        ) {
                            self.set_status(Status::error(format!(
                                "Restored original settings, but profile update failed: {error}"
                            )));
                            return self.load_settings_profiles_task();
                        }

                        self.set_status(Status::success("Restored original settings"));
                        Task::batch([self.save_task(), self.load_settings_profiles_task()])
                    }
                    Err(error) => {
                        self.set_status(Status::error(format!(
                            "Could not restore original settings: {error}"
                        )));
                        self.load_settings_profiles_task()
                    }
                }
            }
            Message::StorefrontLoaded(request_id, result) => {
                let is_current_request = self
                    .store_request
                    .is_some_and(|request| request.id == request_id);

                if is_current_request {
                    self.store_request = None;
                }
                self.now = iced::time::Instant::now();

                match result {
                    Ok(result) => {
                        if !is_current_request {
                            if cache_account_api_context(
                                &mut self.state,
                                result.account_id,
                                result.session,
                                result.launcher_session,
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
                            result.launcher_session,
                            result.identity,
                        ) {
                            let error = format!("Store loaded, but profile update failed: {error}");
                            self.store_error = Some(error.clone());
                            self.set_view_status(Status::error(error));
                            return Task::none();
                        }

                        let bundle_count = result.summary.featured_bundles.len();
                        // The shop shows what loaded; only missing balances get a toast.
                        if result.summary.currency_balance_error.is_some() {
                            self.set_view_status(Status::warning(format!(
                                "Loaded {} featured bundle(s), {} daily offer(s), and {} night \
                                 market offer(s), but currency balances were unavailable",
                                bundle_count,
                                result.summary.daily_offers.len(),
                                result.summary.night_market_offers.len()
                            )));
                        } else if !self.progress_pinned() {
                            // Ends a "loading updated shop" toast from a shop reset.
                            self.clear_progress_status();
                        }
                        if self.state.selected_account == Some(result.account_id) {
                            self.store_summary = Some(result.summary);
                        }

                        return Task::batch([self.save_task(), self.image_cache_size_task()]);
                    }
                    Err(error) => {
                        if is_current_request {
                            self.set_view_status(Status::error(format!(
                                "Store check failed: {error}"
                            )));
                            self.store_error = Some(error);
                        }
                    }
                }

                Task::none()
            }
            Message::RetryShop => {
                if self.selected_account_is_store_loading() {
                    return Task::none();
                }

                self.fetch_storefront_task()
            }
            Message::RetryLoadout => {
                if self.selected_account_is_loadout_loading() {
                    return Task::none();
                }

                self.fetch_loadout_task()
            }
            Message::RetryLiveMatch => {
                self.live_match_error = None;
                self.poll_live_match()
            }
            Message::HiddenDetailsToggled => {
                self.show_hidden_details = !self.show_hidden_details;
                // Hidden players' names load now instead of on the next poll.
                if self.show_hidden_details {
                    self.poll_live_match()
                } else {
                    Task::none()
                }
            }
            Message::LiveMatchLoaded(request_id, result) => {
                self.handle_live_match_loaded(request_id, result)
            }
            Message::ShopTimerTick(now) => {
                self.now = now;

                if self.store_request.is_none()
                    && self
                        .store_summary
                        .as_ref()
                        .is_some_and(|summary| summary.is_expired_at(now))
                {
                    self.store_summary = None;
                    // Otherwise it would open again by itself once the shop reloads.
                    self.bundle_details = None;
                    let task = self.fetch_storefront_task();
                    self.set_view_status(Status::progress(
                        "Shop reset reached; loading updated shop",
                    ));
                    return task;
                }

                if self.loadout_request.is_none()
                    && self
                        .loadout_summary
                        .as_ref()
                        .is_some_and(|summary| summary.battle_pass_ended_at(now))
                {
                    self.loadout_summary = None;
                    let task = self.fetch_loadout_task();
                    self.set_view_status(Status::progress(
                        "Battle pass act ended; loading the new one",
                    ));
                    return task;
                }

                Task::none()
            }
            Message::LoadingTick => {
                if super::loading_indicator_active(self) {
                    self.loading_frame = self.loading_frame.wrapping_add(1);
                }

                Task::none()
            }
            Message::LoadoutLoaded(request_id, result) => {
                let is_current_request = self
                    .loadout_request
                    .is_some_and(|request| request.id == request_id);

                if is_current_request {
                    self.loadout_request = None;
                }
                self.now = iced::time::Instant::now();

                match result {
                    Ok(result) => {
                        if !is_current_request {
                            if cache_account_api_context(
                                &mut self.state,
                                result.account_id,
                                result.session,
                                result.launcher_session,
                                result.identity,
                            )
                            .is_ok()
                            {
                                self.save_player_card(
                                    result.account_id,
                                    result.summary.player_card_id,
                                );
                                return Task::batch([
                                    self.save_task(),
                                    self.player_card_art_task(),
                                ]);
                            }

                            return Task::none();
                        }

                        if let Err(error) = cache_account_api_context(
                            &mut self.state,
                            result.account_id,
                            result.session,
                            result.launcher_session,
                            result.identity,
                        ) {
                            let error =
                                format!("Loadout loaded, but profile update failed: {error}");
                            self.loadout_error = Some(error.clone());
                            self.set_view_status(Status::error(error));
                            return Task::none();
                        }
                        // Only once the session checked out as this account's own.
                        self.save_player_card(
                            result.account_id,
                            result.summary.player_card_id.clone(),
                        );

                        let gun_count = result.summary.gun_skins.len();
                        // The tab shows what loaded; only a failed half gets a toast.
                        match (
                            &result.summary.loadout_error,
                            &result.summary.battle_pass_error,
                        ) {
                            (Some(error), _) => self.set_view_status(Status::error(format!(
                                "Loaded battle pass progress; loadout failed: {error}"
                            ))),
                            (None, Some(error)) => self.set_view_status(Status::error(format!(
                                "Loaded loadout with {gun_count} gun skin(s); battle pass failed: {error}"
                            ))),
                            (None, None) if !self.progress_pinned() => self.clear_progress_status(),
                            (None, None) => {}
                        }
                        if let Some(level) = result.summary.account_level
                            && let Some(account) = self
                                .state
                                .accounts
                                .iter_mut()
                                .find(|account| account.id == result.account_id)
                        {
                            account.account_level = Some(level);
                        }

                        if self.state.selected_account == Some(result.account_id) {
                            self.loadout_summary = Some(result.summary);
                        }

                        return Task::batch([
                            self.save_task(),
                            self.image_cache_size_task(),
                            self.player_card_art_task(),
                        ]);
                    }
                    Err(error) => {
                        if is_current_request {
                            self.set_view_status(Status::error(format!(
                                "Loadout check failed: {error}"
                            )));
                            self.loadout_error = Some(error);
                        }
                    }
                }

                Task::none()
            }
            Message::OpenImageViewer(image) => self.open_image_viewer(image),
            Message::ImageViewerImageLoaded(source, result) => {
                self.handle_image_viewer_image_loaded(source, result)
            }
            Message::CloseImageViewer => self.close_image_viewer(),
            Message::ShowBundleDetails(store_id) => {
                self.bundle_details = Some(store_id.clone());
                let Some(bundle) = self.open_bundle_details() else {
                    return Task::none();
                };
                if bundle.items.iter().all(|item| item.cached_icon().is_some()) {
                    return Task::none();
                }
                Task::perform(
                    super::data::image_assets::cache_bundle_item_images(
                        bundle.items.clone(),
                        self.image_cache.clone(),
                    ),
                    move |items| Message::BundleItemArtLoaded(store_id.clone(), items),
                )
            }
            Message::BundleItemArtLoaded(store_id, items) => {
                if let Some(bundle) = self.store_summary.as_mut().and_then(|summary| {
                    summary
                        .featured_bundles
                        .iter_mut()
                        .find(|bundle| bundle.store_id == store_id)
                }) && bundle.items.len() == items.len()
                {
                    bundle.items = items;
                }
                Task::none()
            }
            Message::CloseBundleDetails => {
                self.bundle_details = None;
                Task::none()
            }
            Message::RiotClientPathChanged(value) => {
                self.riot_client_path_input = value;
                Task::none()
            }
            Message::BrowseRiotClientPath => {
                let start_dir = typed_riot_client_path(&self.riot_client_path_input)
                    .and_then(|path| path.parent().map(Path::to_path_buf))
                    .filter(|dir| dir.is_dir());

                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            crate::file_dialog::pick_exe(
                                "Find RiotClientServices.exe",
                                start_dir.as_deref(),
                            )
                        })
                        .await
                        .ok()
                        .flatten()
                    },
                    Message::RiotClientPathPicked,
                )
            }
            Message::RiotClientPathPicked(path) => {
                // Still needs Save, like a typed path.
                if let Some(path) = path {
                    self.riot_client_path_input = path.display().to_string();
                }
                Task::none()
            }
            Message::SettingsSectionSelected(section) => {
                self.settings_section = section;
                screens::scroll_to_settings_section(section)
            }
            Message::ToggleTokenImport => {
                self.token_import_open = !self.token_import_open;
                Task::none()
            }
            Message::OpenInExplorer(path) => Task::perform(
                async move { open_in_explorer(&path).map_err(|error| error.to_string()) },
                Message::ExplorerOpened,
            ),
            Message::ExplorerOpened(result) => {
                if let Err(error) = result {
                    self.set_status(Status::error(format!("Could not open Explorer: {error}")));
                }
                Task::none()
            }
            Message::SaveSettings => {
                let path = typed_riot_client_path(&self.riot_client_path_input);

                if let Some(path) = &path
                    && !path.is_file()
                {
                    self.set_status(Status::error(format!(
                        "Could not save settings: there is no Riot Client at {}",
                        path.display()
                    )));
                    return Task::none();
                }

                self.riot_client_path_input = path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default();
                self.state.riot_client_path = path;
                self.set_status(Status::success("Saved settings"));
                self.save_task()
            }
            Message::ImageCacheSizeLoaded(result) => self.handle_image_cache_size_loaded(result),
            Message::RankIconsLoaded(result) => {
                // Without icons the switcher just leaves them out, so a failure isn't reported.
                if let Ok(icons) = result {
                    self.rank_icons = icons;
                }
                Task::none()
            }
            Message::PlayerCardArtLoaded(card_id, art) => {
                self.player_card_art.insert(card_id, art);
                Task::none()
            }
            Message::AccountFilterChanged(filter) => {
                self.account_filter = filter;
                Task::none()
            }
            Message::ClearImageCache => self.clear_image_cache(),
            Message::ImageCacheCleared(result) => self.handle_image_cache_cleared(result),
            Message::LaunchAccount(id) => {
                if self.launching_account.is_some() || self.launch_preflight_account.is_some() {
                    return Task::none();
                }

                if self.update_blocks_new_work() {
                    return Task::none();
                }

                if self.launcher_capture_in_progress {
                    self.set_status(Status::error(
                        "Wait for the login capture to finish before launching VALORANT",
                    ));
                    return Task::none();
                }

                let Some(account) = self
                    .state
                    .accounts
                    .iter()
                    .find(|account| account.id == id)
                    .cloned()
                else {
                    self.set_status(Status::error("Account profile no longer exists"));
                    return Task::none();
                };

                let summary = account.summary();

                // Riot Client would otherwise start on whichever account it last remembered.
                if !account.has_launcher_session() {
                    self.set_status(Status::error(format!(
                        "Could not launch {summary}: capture its login first (... > Re-capture login)"
                    )));
                    return Task::none();
                }

                self.close_account_surfaces();
                self.unavailable_launch_warning = None;
                self.launch_preflight_account = Some(id);
                self.clear_progress_status();

                Task::perform(
                    check_account_in_game(account, self.client_version_input.clone()),
                    |(check, valorant_running)| {
                        Message::LaunchPreflightChecked(check, valorant_running)
                    },
                )
            }
            Message::LaunchPreflightChecked(check, valorant_running) => {
                if self.launch_preflight_account != Some(check.account_id) {
                    return Task::none();
                }

                self.launch_preflight_account = None;
                self.account_availability
                    .insert(check.account_id, check.availability.clone());
                self.account_availability_checked_at
                    .insert(check.account_id, iced::time::Instant::now());

                let Some(account) = self
                    .state
                    .accounts
                    .iter()
                    .find(|account| account.id == check.account_id)
                    .cloned()
                else {
                    self.set_status(Status::error("Account profile no longer exists"));
                    return Task::none();
                };

                let decision = launch_preflight_decision(&check.availability);
                let mut warnings = Vec::new();
                if valorant_running {
                    warnings.push(format!(
                        "VALORANT is already running. Launching {} will close it, including any match in progress.",
                        account.display_name
                    ));
                }
                if decision == LaunchPreflightDecision::WarnUnavailable
                    && let Some(reason) = check.availability.unavailable_reason()
                {
                    warnings.push(format!(
                        "This account appears unavailable ({reason}). Launching may interrupt that active VALORANT session."
                    ));
                }

                if !warnings.is_empty() {
                    self.unavailable_launch_warning = Some(super::UnavailableLaunchWarning {
                        account_id: account.id,
                        display_name: account.display_name,
                        reason: warnings.join(" "),
                    });
                    // The warning dialog asks instead.
                    self.clear_progress_status();
                    return Task::none();
                }

                match decision {
                    LaunchPreflightDecision::WarnUnavailable => {
                        self.set_status(Status::warning(
                            "Account appears unavailable; confirm launch to continue",
                        ));
                        Task::none()
                    }
                    LaunchPreflightDecision::Launch => self.start_account_launch(account),
                    LaunchPreflightDecision::LaunchInconclusive => {
                        let summary = account.summary();
                        let launch = self.start_account_launch(account);
                        // The button can't say the check failed, so this stays a toast.
                        self.set_status(Status::warning(format!(
                            "Couldn't check whether {summary} is in a match; launching anyway"
                        )));
                        launch
                    }
                }
            }
            Message::CancelUnavailableLaunch => {
                cancel_unavailable_launch_state(
                    &mut self.unavailable_launch_warning,
                    &mut self.launch_preflight_account,
                    &mut self.launching_account,
                    &mut self.launch_progress_checking,
                );
                self.clear_progress_status();
                Task::none()
            }
            Message::LaunchAnyway(id) => {
                if self.launching_account.is_some() || self.launch_preflight_account.is_some() {
                    return Task::none();
                }

                if self.launcher_capture_in_progress {
                    self.set_status(Status::error(
                        "Wait for the login capture to finish before launching VALORANT",
                    ));
                    return Task::none();
                }

                let Some(warning) = self.unavailable_launch_warning.take() else {
                    return Task::none();
                };

                if warning.account_id != id {
                    self.unavailable_launch_warning = Some(warning);
                    return Task::none();
                }

                let Some(account) = self
                    .state
                    .accounts
                    .iter()
                    .find(|account| account.id == id)
                    .cloned()
                else {
                    self.set_status(Status::error("Account profile no longer exists"));
                    return Task::none();
                };

                self.start_account_launch(account)
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
                    self.launch_client_open = true;
                }

                Task::none()
            }
            Message::LaunchFinished(result) => match result {
                Ok(result) if result.target == LaunchTargetProcess::Valorant => {
                    let launched_account = self.launching_account.take();
                    self.launch_progress_checking = false;
                    let mut saved_backup =
                        self.store_previous_account_backup(result.previous_account_backup);

                    if let (Some(account_id), Some(backup)) =
                        (launched_account, result.synced_backup)
                        && let Some(account) = self
                            .state
                            .accounts
                            .iter_mut()
                            .find(|account| account.id == account_id)
                    {
                        account.launcher_session = Some(backup);
                        saved_backup = true;
                    }

                    match (result.sync_warning, result.previous_account_sync_warning) {
                        (Some(warning), _) => self.set_status(Status::error(format!(
                            "Could not sync launcher session after VALORANT window detected: {warning}"
                        ))),
                        (None, Some(warning)) => self.set_status(Status::error(format!(
                            "VALORANT window detected, but the previous account's login could not be saved: {warning}"
                        ))),
                        (None, None) => self.clear_progress_status(),
                    }

                    if saved_backup {
                        self.save_task()
                    } else {
                        Task::none()
                    }
                }
                Ok(result) => {
                    self.launching_account = None;
                    self.launch_progress_checking = false;
                    if self.store_previous_account_backup(result.previous_account_backup) {
                        self.save_task()
                    } else {
                        Task::none()
                    }
                }
                Err(error) => {
                    self.launching_account = None;
                    self.launch_progress_checking = false;
                    self.set_status(Status::error(format!("Launch failed: {error}")));
                    Task::none()
                }
            },
            Message::CheckForAppUpdate => self.check_for_app_update(),
            Message::AppUpdateChecked {
                user_requested,
                result,
            } => self.handle_app_update_checked(user_requested, result),
            Message::DismissAppUpdate => self.dismiss_app_update(),
            Message::ShowAppUpdate => {
                if let AppUpdateStatus::Dismissed(update)
                | AppUpdateStatus::InstallFailed {
                    update: Some(update),
                    ..
                } = &self.app_update_status
                {
                    self.app_update_status = AppUpdateStatus::Available(update.clone());
                }
                Task::none()
            }
            Message::DownloadAppUpdate => self.download_app_update(),
            Message::AppUpdatePrepared(result) => self.handle_app_update_prepared(result),
        }
    }

    fn check_for_app_update(&mut self) -> Task<Message> {
        if self.app_update_status.is_busy() {
            return Task::none();
        }

        self.app_update_status = AppUpdateStatus::Checking;
        self.set_status(Status::progress("Checking for Prime updates"));
        Task::perform(check_for_update(), |result| Message::AppUpdateChecked {
            user_requested: true,
            result: result.map_err(|error| error.to_string()),
        })
    }

    fn handle_app_update_checked(
        &mut self,
        user_requested: bool,
        result: Result<UpdateCheckOutcome, String>,
    ) -> Task<Message> {
        match result {
            Ok(UpdateCheckOutcome::Available(update)) => {
                // A background check opens the update prompt, which says the same thing.
                if user_requested {
                    self.set_status(Status::info(format!(
                        "Prime {} is available; download it when ready",
                        update.latest_version
                    )));
                }
                self.app_update_status = AppUpdateStatus::Available(update);
            }
            Ok(UpdateCheckOutcome::NotInstalled) => {
                self.app_update_status = AppUpdateStatus::NotInstalled;

                if user_requested {
                    self.set_status(Status::info(self.app_update_status.label()));
                }
            }
            Ok(UpdateCheckOutcome::UpToDate) => {
                self.app_update_status = AppUpdateStatus::UpToDate;

                if user_requested {
                    self.set_status(Status::success(format!(
                        "Prime is up to date ({})",
                        crate::updater::CURRENT_VERSION
                    )));
                }
            }
            Err(error) => {
                self.app_update_status = AppUpdateStatus::CheckFailed(error.clone());

                if user_requested {
                    self.set_status(Status::error(format!("Update check failed: {error}")));
                }
            }
        }

        Task::none()
    }

    fn dismiss_app_update(&mut self) -> Task<Message> {
        if let Some(update) = self.app_update_status.prompt_update().cloned() {
            self.app_update_status = AppUpdateStatus::Dismissed(update);
        }

        Task::none()
    }

    fn download_app_update(&mut self) -> Task<Message> {
        let Some(update) = self.app_update_status.pending_update().cloned() else {
            self.set_status(Status::info("No Prime update is available to download"));
            return Task::none();
        };

        // Prime exits to install the update, which would cut this work off.
        if let Some(work) = self.work_blocking_update() {
            self.set_status(Status::error(format!(
                "Could not start the update: wait for {work}"
            )));
            return Task::none();
        }

        self.set_status(Status::progress(format!(
            "Downloading Prime {}",
            update.latest_version
        )));
        self.app_update_status = AppUpdateStatus::Downloading(update.clone());
        Task::perform(download_and_prepare_update(update), |result| {
            Message::AppUpdatePrepared(result.map_err(|error| error.to_string()))
        })
    }

    fn handle_app_update_prepared(&mut self, result: Result<(), String>) -> Task<Message> {
        match result {
            Ok(()) => {
                self.app_update_status = AppUpdateStatus::Installing;
                self.set_status(Status::progress(
                    "Preparing to restart and install the update",
                ));
                iced::exit()
            }
            Err(error) => {
                let update = match &self.app_update_status {
                    AppUpdateStatus::Downloading(update) => Some(update.clone()),
                    _ => None,
                };
                self.set_status(Status::error(format!("Update failed: {error}")));
                self.app_update_status = AppUpdateStatus::InstallFailed { update, error };
                Task::none()
            }
        }
    }

    fn open_image_viewer(&mut self, image: super::ImageViewerRequest) -> Task<Message> {
        if !super::image_viewer_enabled() {
            self.image_viewer = None;
            return Task::none();
        }

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

    fn handle_image_viewer_image_loaded(
        &mut self,
        source: ImageViewerSource,
        result: Result<std::path::PathBuf, String>,
    ) -> Task<Message> {
        if !super::image_viewer_enabled() {
            return Task::none();
        }

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
                self.set_status(Status::error(format!("Could not load full image: {error}")));
            }
        }

        Task::none()
    }

    fn close_image_viewer(&mut self) -> Task<Message> {
        self.image_viewer = None;
        Task::none()
    }

    fn handle_image_cache_size_loaded(
        &mut self,
        result: Result<CacheUsage, String>,
    ) -> Task<Message> {
        match result {
            Ok(usage) => {
                self.image_cache_usage = usage;
            }
            Err(error) => {
                self.set_status(Status::error(format!(
                    "Could not read image cache size: {error}"
                )));
            }
        }

        Task::none()
    }

    fn clear_image_cache(&mut self) -> Task<Message> {
        if self.image_cache_clearing {
            return Task::none();
        }

        let cache = self.image_cache.clone();
        self.image_cache_clearing = true;
        self.set_status(Status::progress("Clearing image cache"));
        Task::perform(
            async move { cache.clear().map_err(|error| error.to_string()) },
            Message::ImageCacheCleared,
        )
    }

    fn handle_image_cache_cleared(&mut self, result: Result<(), String>) -> Task<Message> {
        self.image_cache_clearing = false;
        // Shop and Loadout art points at cached files, some of which are now gone; they reload
        // with fresh downloads when opened.
        self.clear_selected_account_views();

        match result {
            Ok(()) => {
                self.image_cache_usage = CacheUsage::default();
                self.set_status(Status::success("Cleared image cache"));
                // The rank icon files were deleted with the rest.
                self.rank_icons.clear();
                self.player_card_art.clear();
                return Task::batch([
                    cache_rank_icons_task(&self.image_cache),
                    self.player_card_art_task(),
                ]);
            }
            Err(error) => {
                self.set_status(Status::error(format!(
                    "Could not clear image cache: {error}"
                )));
            }
        }

        Task::none()
    }

    fn close_account_action_surfaces(&mut self) {
        self.open_account_menu = None;
        self.open_preset_menu = None;
        self.show_add_account_prompt = false;
        self.show_import_account_prompt = false;
        // The pasted export holds the account's login, so it isn't kept once the prompt closes.
        self.import_account_input.clear();
        self.exported_account = None;
        self.confirm_delete_account = None;
        self.confirm_recapture_account = None;
        self.settings_check = None;
        self.confirm_settings_change = None;
        self.confirm_delete_settings_profile = None;
        self.capture_prompt_valorant_running = false;
    }

    /// Whether a preset is being saved, a preset applied or original settings restored, or the
    /// check before one of those runs.
    pub(super) fn settings_work_in_progress(&self) -> bool {
        self.settings_saving_account.is_some()
            || self.settings_applying_account.is_some()
            || self.settings_check.is_some()
    }

    /// Starts the checks behind an Apply or Restore; its confirmation opens when they finish, with
    /// the pressed control showing a loading state meanwhile. A background activity result under
    /// 90 seconds old skips the Riot check; otherwise `check_settings_activity` runs, and its
    /// refreshed session is saved before the dialog can start the change, so the two never sign
    /// in at once. Whether VALORANT is running on this PC is checked every time, locally.
    fn open_settings_change(&mut self, change: SettingsChange) -> Task<Message> {
        if self.settings_work_in_progress() || self.update_blocks_new_work() {
            return Task::none();
        }

        let Some(account) = self
            .state
            .accounts
            .iter()
            .find(|account| account.id == change.account_id())
            .cloned()
        else {
            self.set_status(Status::error("Account profile no longer exists"));
            return Task::none();
        };

        let fresh = self.fresh_availability(account.id).is_some();
        self.close_account_surfaces();
        self.next_request_id += 1;
        let request_id = self.next_request_id;
        self.settings_check = Some(PendingSettingsCheck { request_id, change });

        let account_id = account.id;
        let client_version = self.client_version_input.clone();
        Task::perform(
            async move {
                let check = if fresh {
                    None
                } else {
                    Some(check_settings_activity(account, client_version).await)
                };
                (check, valorant_is_running().await)
            },
            move |(check, valorant_running)| {
                Message::SettingsPreflightChecked(request_id, account_id, check, valorant_running)
            },
        )
    }

    /// The background poll's (or Launch check's) result for this account, when it arrived
    /// recently enough to trust.
    pub(super) fn fresh_availability(&self, account_id: AccountId) -> Option<&AccountAvailability> {
        const FRESH_FOR: std::time::Duration = std::time::Duration::from_secs(90);

        self.account_availability_checked_at
            .get(&account_id)
            .filter(|checked_at| checked_at.elapsed() < FRESH_FOR)?;
        self.account_availability
            .get(&account_id)
            .filter(|availability| !matches!(availability, AccountAvailability::Unknown { .. }))
    }

    fn close_account_surfaces(&mut self) {
        self.account_switcher_open = false;
        self.close_account_action_surfaces();
    }

    /// Clears Shop and Loadout, and stops treating loads already in flight as current; their
    /// replies only cache the session they obtained.
    fn clear_selected_account_views(&mut self) {
        self.store_summary = None;
        self.bundle_details = None;
        self.loadout_summary = None;
        self.store_request = None;
        self.loadout_request = None;
        self.store_error = None;
        self.loadout_error = None;
        self.live_match = None;
        self.live_match_request = None;
        self.live_match_error = None;
    }

    /// Loads the active tab after one account changed (selected, added, imported, re-captured or
    /// refreshed). On the Accounts tab only that account is refreshed, quietly, so the message
    /// about the change stays on screen; the periodic availability check covers the rest.
    fn load_account_tab(&mut self, account_id: AccountId) -> Task<Message> {
        if self.active_tab != Tab::Accounts {
            return self.load_active_tab();
        }

        let Some(account) = self
            .state
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .cloned()
        else {
            return Task::none();
        };

        Task::batch([
            if !self.account_ranks_loading.is_empty() {
                Task::none()
            } else {
                self.fetch_account_ranks_task_for(vec![account.clone()], false)
            },
            if self.availability_poll_blocked() {
                Task::none()
            } else {
                self.fetch_account_availabilities_task_for(vec![account])
            },
        ])
    }

    fn load_active_tab(&mut self) -> Task<Message> {
        match self.active_tab {
            Tab::Accounts => {
                let now = iced::time::Instant::now();
                let fresh = |loaded_at: Option<iced::time::Instant>| {
                    loaded_at.is_some_and(|loaded_at| {
                        now.saturating_duration_since(loaded_at) < super::ACCOUNTS_TAB_RELOAD_AFTER
                    })
                };
                let reload_details =
                    self.account_ranks_loading.is_empty() && !fresh(self.account_details_loaded_at);
                let reload_availability = !self.availability_poll_blocked()
                    && !fresh(self.account_availability_loaded_at);

                Task::batch([
                    if reload_details {
                        self.fetch_account_ranks_task()
                    } else {
                        Task::none()
                    },
                    if reload_availability {
                        self.fetch_account_availabilities_task()
                    } else {
                        Task::none()
                    },
                ])
            }
            // Countdowns only tick on their own tab, so a reset reached elsewhere reloads here.
            Tab::Shop
                if !self.selected_account_is_store_loading()
                    && self.store_summary.as_ref().is_none_or(|summary| {
                        summary.is_expired_at(iced::time::Instant::now())
                    }) =>
            {
                self.store_summary = None;
                self.fetch_storefront_task()
            }
            // Every Shop and Loadout load can add images, so the size is read again here.
            Tab::Settings => self.image_cache_size_task(),
            // Opening the page checks at once, and tries again after a sign-in failure.
            Tab::LiveMatch => {
                self.live_match_error = None;
                self.poll_live_match()
            }
            Tab::Loadout
                if !self.selected_account_is_loadout_loading()
                    && self.loadout_summary.as_ref().is_none_or(|summary| {
                        summary.battle_pass_ended_at(iced::time::Instant::now())
                    }) =>
            {
                self.loadout_summary = None;
                self.fetch_loadout_task()
            }
            _ => Task::none(),
        }
    }

    /// The window is back on screen after being minimized or hidden: catch up the clock and the
    /// availability poll that paused.
    fn window_shown(&mut self) -> Task<Message> {
        if !self.window_minimized {
            return Task::none();
        }
        self.window_minimized = false;
        self.now = iced::time::Instant::now();

        if self.active_tab == Tab::Accounts && !self.availability_poll_blocked() {
            return self.fetch_account_availabilities_task();
        }
        if self.active_tab == Tab::LiveMatch {
            return self.poll_live_match();
        }

        Task::none()
    }

    /// Whether an availability poll must not start: one is running, or a Launch check or settings
    /// work (check, save or apply), or a Live Match load is signing in with the same refresh
    /// token.
    fn availability_poll_blocked(&self) -> bool {
        self.account_availability_loading
            || self.live_match_in_flight
            || self.launch_preflight_account.is_some()
            || self.settings_work_in_progress()
    }

    fn fetch_account_availabilities_task(&mut self) -> Task<Message> {
        let task = self.fetch_account_availabilities_task_for(self.state.accounts.clone());

        if task.units() > 0 {
            self.account_availability_loaded_at = Some(iced::time::Instant::now());
        }

        task
    }

    fn fetch_account_availabilities_task_for(
        &mut self,
        accounts: Vec<AccountProfile>,
    ) -> Task<Message> {
        if accounts.is_empty() {
            return Task::none();
        }

        if self.client_version_input.trim().is_empty() {
            return Task::none();
        }

        self.account_availability_loading = true;
        let client_version = self.client_version_input.clone();

        Task::perform(
            fetch_account_availabilities(accounts, client_version),
            Message::AccountAvailabilitiesLoaded,
        )
    }

    fn fetch_account_ranks_task(&mut self) -> Task<Message> {
        let task = self.fetch_account_ranks_task_for(self.state.accounts.clone(), true);

        if task.units() > 0 {
            self.account_details_loaded_at = Some(iced::time::Instant::now());
        }

        task
    }

    /// Starts a details load. `announce` shows its progress and result in the status bar, unless
    /// launch or login capture progress is pinned there.
    fn fetch_account_ranks_task_for(
        &mut self,
        accounts: Vec<AccountProfile>,
        announce: bool,
    ) -> Task<Message> {
        if accounts.is_empty() {
            return Task::none();
        }

        if self.client_version_input.trim().is_empty() {
            return Task::none();
        }

        let announce = announce && !self.progress_pinned();
        self.account_ranks_loading = accounts.iter().map(|account| account.id).collect();
        let client_version = self.client_version_input.clone();

        Task::perform(
            fetch_account_ranks(accounts, client_version),
            move |result| Message::AccountRanksLoaded { result, announce },
        )
    }

    fn fetch_storefront_task(&mut self) -> Task<Message> {
        // Without an account or the client version, the tab explains what it is waiting for; the
        // version arriving loads the open tab.
        let Some(account) = self.state.selected_account().cloned() else {
            return Task::none();
        };
        if self.client_version_input.trim().is_empty() {
            return Task::none();
        }

        let request = self.next_view_request(account.id);
        self.store_request = Some(request);
        self.store_error = None;
        let image_cache = self.image_cache.clone();
        Task::perform(
            fetch_storefront(account, self.client_version_input.clone(), image_cache),
            move |result| Message::StorefrontLoaded(request.id, result),
        )
    }

    fn fetch_loadout_task(&mut self) -> Task<Message> {
        let Some(account) = self.state.selected_account().cloned() else {
            return Task::none();
        };
        if self.client_version_input.trim().is_empty() {
            return Task::none();
        }

        let request = self.next_view_request(account.id);
        self.loadout_request = Some(request);
        self.loadout_error = None;
        let image_cache = self.image_cache.clone();
        Task::perform(
            fetch_loadout(account, self.client_version_input.clone(), image_cache),
            move |result| Message::LoadoutLoaded(request.id, result),
        )
    }

    /// Loads the selected account's live match, unless another request is signing in with the
    /// same refresh token or its login failed; a sign-in failure waits for Try again or for the
    /// page to open again.
    fn poll_live_match(&mut self) -> Task<Message> {
        if self.window_minimized
            || self.availability_poll_blocked()
            || matches!(self.live_match_error, Some(LiveMatchError::SignIn(_)))
        {
            return Task::none();
        }
        let Some(account) = self.state.selected_account().cloned() else {
            return Task::none();
        };
        if self.client_version_input.trim().is_empty() {
            return Task::none();
        }

        let request = self.next_view_request(account.id);
        self.live_match_request = Some(request);
        self.live_match_in_flight = true;
        let previous = self
            .live_match
            .clone()
            .filter(|live| live.account_id == account.id);
        let show_hidden = self.show_hidden_details;
        Task::perform(
            fetch_live_match(
                account,
                self.client_version_input.clone(),
                self.image_cache.clone(),
                previous,
                shown_weapons(self.state.live_match_weapons.as_deref()),
                show_hidden,
            ),
            move |result| Message::LiveMatchLoaded(request.id, result),
        )
    }

    fn handle_live_match_loaded(
        &mut self,
        request_id: u64,
        result: Result<super::LiveMatchResult, LiveMatchError>,
    ) -> Task<Message> {
        let is_current_request = self
            .live_match_request
            .is_some_and(|request| request.id == request_id);
        // With no current request, this was the one signing in.
        let left_behind = self.live_match_request.is_none();
        if is_current_request || left_behind {
            self.live_match_request = None;
            self.live_match_in_flight = false;
        }
        // A reply left behind by an account switch frees the sign-in, so the page's own load
        // can start.
        let follow_up = if left_behind && self.active_tab == Tab::LiveMatch {
            self.poll_live_match()
        } else {
            Task::none()
        };

        let result = match result {
            Ok(result) => result,
            Err(error) => {
                if is_current_request {
                    // A match already on screen stays; the toast says it's out of date.
                    if self.live_match.is_some() && matches!(error, LiveMatchError::Request(_)) {
                        self.set_view_status(Status::error(format!(
                            "Live match didn't update: {}",
                            error.message()
                        )));
                    }
                    self.live_match_error = Some(error);
                }
                return follow_up;
            }
        };

        let cached_session = result
            .refreshed
            .is_some_and(|refreshed| self.cache_refreshed_api_context(refreshed));
        let save = if cached_session {
            self.save_task()
        } else {
            Task::none()
        };
        if !is_current_request {
            return Task::batch([save, follow_up]);
        }

        self.account_availability
            .insert(result.account_id, result.activity.into());
        self.account_availability_checked_at
            .insert(result.account_id, iced::time::Instant::now());
        self.live_match = result.live;
        self.live_match_error = None;
        // Columns changed in Settings while this load ran load again, rather than a minute later.
        let stale_columns = self.active_tab == Tab::LiveMatch
            && self.live_match.as_ref().is_some_and(|live| {
                live.weapons != shown_weapons(self.state.live_match_weapons.as_deref())
            });
        let reload = if stale_columns {
            self.poll_live_match()
        } else {
            Task::none()
        };
        Task::batch([save, self.player_card_art_task(), reload])
    }

    /// Remembers the player card an account's loadout reported, for its avatar.
    fn save_player_card(&mut self, account_id: AccountId, card_id: Option<String>) {
        if let Some(card_id) = card_id
            && let Some(account) = self
                .state
                .accounts
                .iter_mut()
                .find(|account| account.id == account_id)
        {
            account.player_card_id = Some(card_id);
        }
    }

    /// Caches the art of every account's and live match player's card that isn't cached or on
    /// its way yet.
    fn player_card_art_task(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        let live_players = self
            .live_match
            .iter()
            .flat_map(|live| live.allies.iter().chain(&live.enemies));
        let card_ids = self
            .state
            .accounts
            .iter()
            .map(|account| &account.player_card_id)
            .chain(live_players.map(|player| &player.card_id));
        for card_id in card_ids {
            let Some(card_id) = card_id else {
                continue;
            };
            if self.player_card_art.contains_key(card_id) {
                continue;
            }
            self.player_card_art
                .insert(card_id.clone(), Default::default());
            let card_id = card_id.clone();
            tasks.push(Task::perform(
                cache_player_card_art(self.image_cache.clone(), card_id.clone()),
                move |art| Message::PlayerCardArtLoaded(card_id.clone(), art),
            ));
        }
        Task::batch(tasks)
    }

    fn next_view_request(&mut self, account_id: AccountId) -> ViewRequest {
        self.next_request_id += 1;
        ViewRequest {
            id: self.next_request_id,
            account_id,
        }
    }

    /// Keeps an API session obtained in the background, unless the account's launcher login was
    /// replaced or removed while the request ran. Returns whether anything changed.
    fn cache_refreshed_api_context(&mut self, refreshed: RefreshedApiContext) -> bool {
        let still_current = self
            .state
            .accounts
            .iter()
            .find(|account| account.id == refreshed.account_id)
            .is_some_and(|account| match &refreshed.launcher_session {
                Some(backup) => account
                    .launcher_session
                    .as_ref()
                    .is_some_and(|current| current.data_dir == backup.data_dir),
                None => true,
            });

        still_current
            && cache_account_api_context(
                &mut self.state,
                refreshed.account_id,
                refreshed.session,
                refreshed.launcher_session,
                refreshed.identity,
            )
            .is_ok()
    }

    /// Records the refreshed backup of the account that was signed in before a switch. Returns
    /// whether anything changed and needs saving.
    fn store_previous_account_backup(
        &mut self,
        previous: Option<(AccountId, LauncherSessionBackup)>,
    ) -> bool {
        let Some((account_id, backup)) = previous else {
            return false;
        };
        let Some(account) = self
            .state
            .accounts
            .iter_mut()
            .find(|account| account.id == account_id)
        else {
            return false;
        };

        // Only accept it if the account still points at that slot; it may have been re-captured or
        // removed while the launch ran.
        if account
            .launcher_session
            .as_ref()
            .is_none_or(|current| current.data_dir != backup.data_dir)
        {
            return false;
        }

        account.launcher_session = Some(backup);
        true
    }

    fn start_account_launch(&mut self, account: AccountProfile) -> Task<Message> {
        let id = account.id;
        let config = LaunchConfig {
            riot_client_path: self.state.riot_client_path.clone(),
            ..LaunchConfig::default()
        };
        let backup = account.launcher_session.clone();
        let saved_sessions = self.saved_launcher_sessions();

        let selection_changed = self.state.selected_account != Some(id);
        self.state.select_account(id);
        // Dialogs were closed when the launch was asked for; any open now were opened since.
        self.unavailable_launch_warning = None;
        self.clear_progress_status();
        self.launching_account = Some(id);
        self.launch_progress_checking = false;
        self.launch_client_open = false;

        // Shop and Loadout show the selected account, so they reload when launching switched it.
        let reload = if selection_changed {
            self.clear_selected_account_views();
            match self.active_tab {
                Tab::Shop | Tab::Loadout | Tab::LiveMatch => self.load_active_tab(),
                Tab::Accounts | Tab::Settings => Task::none(),
            }
        } else {
            Task::none()
        };

        Task::batch([
            self.save_task(),
            Task::perform(
                async move { launch_account(config, backup, saved_sessions).await },
                Message::LaunchFinished,
            ),
            reload,
        ])
    }

    fn selected_account_is_store_loading(&self) -> bool {
        self.store_request
            .is_some_and(|request| Some(request.account_id) == self.state.selected_account)
    }

    fn selected_account_is_loadout_loading(&self) -> bool {
        self.loadout_request
            .is_some_and(|request| Some(request.account_id) == self.state.selected_account)
    }

    pub(super) fn state_to_save(&self) -> Result<StoredState, String> {
        if !self.accounts_loaded {
            return Err(format!(
                "accounts were not loaded from {}, so Prime did not save changes to avoid overwriting them",
                self.repo.path().display()
            ));
        }

        Ok(self.state.clone())
    }

    fn save_task(&self) -> Task<Message> {
        let repo = self.repo.clone();
        let snapshot = match self.state_to_save() {
            Ok(state) => repo.snapshot(&state),
            Err(error) => return Task::done(Message::Saved(Err(error))),
        };

        Task::perform(
            async move {
                repo.save_snapshot(snapshot)
                    .map_err(|error| error.to_string())
            },
            Message::Saved,
        )
    }

    fn image_cache_size_task(&self) -> Task<Message> {
        let cache = self.image_cache.clone();

        Task::perform(
            async move { cache.usage().map_err(|error| error.to_string()) },
            Message::ImageCacheSizeLoaded,
        )
    }

    pub(super) fn riot_client_path_unsaved(&self) -> bool {
        typed_riot_client_path(&self.riot_client_path_input) != self.state.riot_client_path
    }

    fn load_settings_profiles_task(&self) -> Task<Message> {
        let profile_dir = self.repo.settings_profiles_dir();

        Task::perform(
            load_game_settings_profiles(profile_dir),
            Message::GameSettingsProfilesLoaded,
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

    fn confirm_captured_account(&mut self) -> Task<Message> {
        let Some(draft) = self.pending_account.clone() else {
            self.set_status(Status::error("No captured account is waiting to be saved"));
            return Task::none();
        };

        if let Some(existing_id) = self
            .state
            .accounts
            .iter()
            .find(|account| {
                account
                    .puuid
                    .as_ref()
                    .is_some_and(|puuid| puuid.eq_ignore_ascii_case(&draft.puuid))
            })
            .map(|account| account.id)
        {
            return self.update_existing_captured_account(existing_id, draft);
        }

        match AccountProfile::new(self.new_display_name.clone(), draft.shard) {
            Ok(mut account) => {
                account.id = draft.account_id;
                account.session = draft.session;

                if let Err(error) = account.attach_launcher_session(draft.backup) {
                    self.set_status(Status::error(format!("Captured account rejected: {error}")));
                    return Task::none();
                }

                if let (Some(game_name), Some(tag_line)) = (draft.game_name, draft.tag_line)
                    && let Err(error) =
                        account.apply_riot_identity(draft.puuid, game_name, tag_line)
                {
                    self.set_status(Status::error(format!(
                        "Captured identity rejected: {error}"
                    )));
                    return Task::none();
                }

                let added = format!("Added {}", account.summary());
                self.set_status(Status::success(added.clone()));
                self.state.push_account(account);
                self.account_availability.remove(&draft.account_id);
                self.state.select_account(draft.account_id);
                self.clear_selected_account_views();
                self.pending_account = None;
                self.new_display_name.clear();
                let saved =
                    Task::batch([self.save_task(), self.load_account_tab(draft.account_id)]);
                // Only a new account; a duplicate updates the existing one and adds nothing.
                if self.save_settings_on_add {
                    return Task::batch([
                        saved,
                        self.save_added_account_settings(draft.account_id, &added),
                    ]);
                }
                return saved;
            }
            Err(error) => {
                self.set_status(Status::error(error.to_string()));
            }
        }

        Task::none()
    }

    /// Saves a just-added account's VALORANT settings as a profile, when that was asked for.
    fn save_added_account_settings(&mut self, account_id: AccountId, added: &str) -> Task<Message> {
        let name = self
            .state
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(|account| default_preset_name(&account.display_name))
            .unwrap_or_default();
        let task = self.handle_message(Message::SaveSettingsPreset { account_id, name });

        if self.settings_saving_account == Some(account_id) {
            self.set_status(Status::progress(format!(
                "{added}. Saving its VALORANT settings as a preset"
            )));
        } else {
            self.set_status(Status::error(format!(
                "{added}, but its VALORANT settings could not be saved while other settings work runs"
            )));
        }

        task
    }

    fn update_existing_captured_account(
        &mut self,
        account_id: AccountId,
        draft: super::data::launch_flow::CapturedAccountDraft,
    ) -> Task<Message> {
        let backup = match adopt_launcher_session_backup(
            self.repo.launcher_backups_dir(),
            draft.account_id,
            account_id,
            draft.backup,
        ) {
            Ok(backup) => backup,
            Err(error) => {
                self.set_status(Status::error(format!("Captured account rejected: {error}")));
                return Task::none();
            }
        };

        let Some(account) = self
            .state
            .accounts
            .iter_mut()
            .find(|account| account.id == account_id)
        else {
            self.set_status(match remove_launcher_session_backup(
                self.repo.launcher_backups_dir(),
                draft.account_id,
            ) {
                Ok(()) => Status::error("Captured account, but the profile no longer exists"),
                Err(error) => Status::error(format!(
                    "Captured account, but the profile no longer exists and cleanup failed: {error}"
                )),
            });
            return Task::none();
        };

        account.shard = draft.shard;
        account.session = draft.session;

        if let Err(error) = account.attach_launcher_session(backup) {
            self.set_status(Status::error(format!("Captured account rejected: {error}")));
            return Task::none();
        }

        if let (Some(game_name), Some(tag_line)) = (draft.game_name, draft.tag_line)
            && let Err(error) = account.apply_riot_identity(draft.puuid, game_name, tag_line)
        {
            self.set_status(Status::error(format!(
                "Captured identity rejected: {error}"
            )));
            return Task::none();
        }

        let summary = account.summary();
        self.account_availability.remove(&account_id);
        self.state.select_account(account_id);
        self.pending_account = None;
        self.new_display_name.clear();
        self.clear_selected_account_views();
        self.set_status(Status::error(format!(
            "Duplicate account: Prime did not add a new profile because this Riot account is already in Prime; updated and selected {summary}"
        )));
        Task::batch([self.save_task(), self.load_account_tab(account_id)])
    }

    fn restore_active_tab_scroll_task(&self) -> Task<Message> {
        operation::scroll_to(
            MAIN_PANEL_SCROLLABLE_ID,
            self.tab_scroll_offsets.get(self.active_tab),
        )
    }

    pub(super) fn launch_in_progress(&self) -> bool {
        self.launching_account.is_some() || self.launch_preflight_account.is_some()
    }

    /// Launch and login capture progress stays in the status bar while it runs, so background
    /// results don't replace it.
    fn progress_pinned(&self) -> bool {
        self.launch_in_progress() || self.launcher_capture_in_progress
    }

    /// Login capture and launching both rewrite Riot Client's live login data, so only one may run.
    /// Returns true (and explains why in the status) when a capture cannot start right now.
    fn login_capture_blocked(&mut self) -> bool {
        if self.update_blocks_new_work() {
            return true;
        }

        if self.launcher_capture_in_progress {
            self.set_status(Status::error(
                "Launcher login capture is already in progress",
            ));
            return true;
        }

        if self.launch_in_progress() {
            self.set_status(Status::error(
                "Wait for VALORANT to finish launching before capturing a login",
            ));
            return true;
        }

        false
    }

    /// Work that would be cut off if Prime exited to install an update, as a "wait for" phrase.
    pub(super) fn work_blocking_update(&self) -> Option<&'static str> {
        if self.launch_in_progress() {
            Some("VALORANT to finish launching")
        } else if self.launcher_capture_in_progress {
            Some("the login capture to finish")
        } else if self.settings_saving_account.is_some() {
            Some("the settings profile to finish saving")
        } else if self.settings_applying_account.is_some() {
            Some("the settings profile to finish applying")
        } else if self.import_account_in_progress {
            Some("the account import to finish")
        } else {
            None
        }
    }

    /// Prime exits to install a downloaded update, so work that exit would cut off doesn't start
    /// while one downloads. Returns true (and says why in the status) in that case.
    fn update_blocks_new_work(&mut self) -> bool {
        if matches!(
            self.app_update_status,
            AppUpdateStatus::Downloading(_) | AppUpdateStatus::Installing
        ) {
            self.set_status(Status::error(
                "Wait for the Prime update to finish; Prime restarts to install it",
            ));
            return true;
        }

        false
    }

    /// Every saved account's launcher backup, so the account signed in to Riot Client can be
    /// found and its live login saved before switching away from it.
    fn saved_launcher_sessions(&self) -> Vec<(AccountId, LauncherSessionBackup)> {
        self.state
            .accounts
            .iter()
            .filter_map(|account| {
                account
                    .launcher_session
                    .clone()
                    .map(|backup| (account.id, backup))
            })
            .collect()
    }

    fn start_login_capture(&mut self, target: LoginCaptureTarget) -> Task<Message> {
        let config = LaunchConfig {
            riot_client_path: self.state.riot_client_path.clone(),
            ..LaunchConfig::default()
        };
        let saved_sessions = self.saved_launcher_sessions();
        self.launcher_capture_in_progress = true;
        self.launcher_capture_kind = Some(target.kind());
        self.login_capture = Some(LoginCapture { target, wait: None });

        Task::perform(
            prepare_login_capture(config, saved_sessions),
            move |result| Message::LoginCapturePrepared { target, result },
        )
    }

    fn handle_login_capture_prepared(
        &mut self,
        target: LoginCaptureTarget,
        result: Result<PreviousAccountSync, String>,
    ) -> Task<Message> {
        let save = match &result {
            Ok(Ok(previous)) if self.store_previous_account_backup(previous.clone()) => {
                self.save_task()
            }
            _ => Task::none(),
        };

        if self.login_capture.as_ref().map(|capture| capture.target) != Some(target) {
            return save;
        }

        let previous_sync_warning = match result {
            Ok(previous_sync) => previous_sync.err(),
            Err(error) => {
                self.end_login_capture();
                self.set_status(match target {
                    LoginCaptureTarget::NewAccount(_) => {
                        Status::error(format!("Could not add account: {error}"))
                    }
                    LoginCaptureTarget::Existing { .. } => Status::error(format!(
                        "Could not complete launcher session login: {error}"
                    )),
                });
                return save;
            }
        };

        if let Some(warning) = previous_sync_warning {
            self.set_status(Status::error(format!(
                "Could not save the previously signed-in account's login: {warning}. Sign in to Riot Client with \"Stay signed in\" ticked to continue."
            )));
        }

        let backup_root = self.repo.launcher_backups_dir();
        let (wait, handle) = match target {
            LoginCaptureTarget::NewAccount(account_id) => Task::perform(
                finish_account_capture(account_id, backup_root),
                Message::AccountCaptureFinished,
            ),
            LoginCaptureTarget::Existing {
                account_id,
                staging_id,
            } => Task::perform(
                finish_verified_launcher_session_login(staging_id, backup_root),
                move |result| Message::LauncherSessionLoginStarted(account_id, result),
            ),
        }
        .abortable();

        if let Some(capture) = &mut self.login_capture {
            capture.wait = Some(handle);
        }

        Task::batch([save, wait])
    }

    fn cancel_login_capture(&mut self) -> Task<Message> {
        // Until the wait starts, Riot Client is still being closed and reopened, and cancelling
        // would race that work.
        let Some(handle) = self
            .login_capture
            .as_ref()
            .and_then(|capture| capture.wait.clone())
        else {
            return Task::none();
        };
        let Some(capture) = self.login_capture.take() else {
            return Task::none();
        };

        handle.abort();
        self.end_login_capture();
        // The slot may already hold the new sign-in, which is a live login.
        self.set_status(
            match remove_launcher_session_backup(
                self.repo.launcher_backups_dir(),
                capture.target.slot_id(),
            ) {
                Ok(()) => Status::info("Canceled login capture. Riot Client was left open and signed out; launching an account from Prime signs it back in."),
                Err(error) => Status::error(format!(
                    "Canceled login capture, but could not remove its partial login backup: {error}"
                )),
            },
        );
        Task::none()
    }

    /// Ends the running add or re-capture when its result arrives. Returns false for a result
    /// from a capture that was cancelled, removing any login it copied.
    fn finish_login_capture(&mut self, slot: Option<AccountId>) -> bool {
        let current_slot = self
            .login_capture
            .as_ref()
            .map(|capture| capture.target.slot_id());

        if current_slot.is_none() || slot.is_some_and(|slot| Some(slot) != current_slot) {
            if let Some(slot) = slot {
                let _ = remove_launcher_session_backup(self.repo.launcher_backups_dir(), slot);
            }
            return false;
        }

        self.end_login_capture();
        true
    }

    fn end_login_capture(&mut self) {
        self.launcher_capture_in_progress = false;
        self.launcher_capture_kind = None;
        self.login_capture = None;
    }

    /// Drops an unsaved captured account and its backup folder, which holds a live login.
    /// Returns whether there was one.
    fn discard_pending_account(&mut self) -> bool {
        let Some(draft) = self.pending_account.take() else {
            return false;
        };

        let _ = remove_launcher_session_backup(self.repo.launcher_backups_dir(), draft.account_id);
        true
    }

    /// Shows the top of the Accounts tab, where a captured account waits for confirmation.
    fn show_accounts_tab_top(&mut self) -> Task<Message> {
        let top = operation::AbsoluteOffset { x: 0.0, y: 0.0 };
        self.active_tab = Tab::Accounts;
        self.active_accounts_tab = AccountsTab::Accounts;
        self.tab_scroll_offsets.set(Tab::Accounts, top);
        operation::scroll_to(MAIN_PANEL_SCROLLABLE_ID, top)
    }

    fn store_captured_launcher_session(
        &mut self,
        account_id: AccountId,
        captured: CapturedLauncherSession,
    ) -> Task<Message> {
        let backup_root = self.repo.launcher_backups_dir();

        if let Some(account) = self
            .state
            .accounts
            .iter_mut()
            .find(|account| account.id == account_id)
        {
            let captured_puuid = captured.backup.puuid.clone();
            let summary = account.summary();

            if account
                .puuid
                .as_ref()
                .is_some_and(|puuid| !puuid.eq_ignore_ascii_case(&captured_puuid))
            {
                let _ = remove_launcher_session_backup(&backup_root, captured.account_id);
                self.set_status(Status::error(format!(
                    "Signed in as a different Riot account; {summary} was not changed. Re-capture and sign in to {summary}."
                )));
                return Task::none();
            }

            let backup = match adopt_launcher_session_backup(
                &backup_root,
                captured.account_id,
                account_id,
                captured.backup,
            ) {
                Ok(backup) => backup,
                Err(error) => {
                    let _ = remove_launcher_session_backup(&backup_root, captured.account_id);
                    self.set_status(Status::error(format!(
                        "Could not save the captured login for {summary}: {error}"
                    )));
                    return Task::none();
                }
            };

            if let Err(error) = account.attach_launcher_session(backup) {
                self.set_status(Status::error(format!("Launcher session rejected: {error}")));
                return Task::none();
            }

            self.set_status(Status::success(format!(
                "Captured launcher session for {summary} ({captured_puuid})"
            )));
            return Task::batch([self.save_task(), self.load_account_tab(account_id)]);
        }

        self.set_status(match remove_launcher_session_backup(
            self.repo.launcher_backups_dir(),
            captured.account_id,
        ) {
            Ok(()) => Status::error("Captured launcher session, but the profile no longer exists"),
            Err(error) => Status::error(format!(
                "Captured launcher session, but the profile no longer exists and cleanup failed: {error}"
            )),
        });
        Task::none()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AccountDetailUpdate {
    pub(super) updated: bool,
    pub(super) partial: bool,
}

pub(super) fn apply_account_detail_results(
    account: &mut AccountProfile,
    rank: Result<Option<CompetitiveRank>, String>,
    account_level: Result<i64, String>,
    penalty_status: Result<AccountPenaltyStatus, String>,
) -> AccountDetailUpdate {
    let partial = rank.is_err() || account_level.is_err() || penalty_status.is_err();
    let mut updated = false;

    if let Ok(competitive_rank) = rank {
        account.competitive_rank = competitive_rank;
        updated = true;
    }

    if let Ok(account_level) = account_level {
        account.account_level = Some(account_level);
        updated = true;
    }

    if let Ok(penalty_status) = penalty_status {
        account.penalty_status = penalty_status;
        updated = true;
    }

    AccountDetailUpdate { updated, partial }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LaunchPreflightDecision {
    Launch,
    LaunchInconclusive,
    WarnUnavailable,
}

pub(super) fn launch_preflight_decision(
    availability: &AccountAvailability,
) -> LaunchPreflightDecision {
    match availability {
        AccountAvailability::Available => LaunchPreflightDecision::Launch,
        AccountAvailability::Unavailable(_) => LaunchPreflightDecision::WarnUnavailable,
        AccountAvailability::Unknown { .. } => LaunchPreflightDecision::LaunchInconclusive,
    }
}

pub(super) fn cancel_unavailable_launch_state(
    unavailable_launch_warning: &mut Option<super::UnavailableLaunchWarning>,
    launch_preflight_account: &mut Option<AccountId>,
    launching_account: &mut Option<AccountId>,
    launch_progress_checking: &mut bool,
) {
    *unavailable_launch_warning = None;
    *launch_preflight_account = None;
    *launching_account = None;
    *launch_progress_checking = false;
}

/// The account's activity, and whether VALORANT is running on this PC.
async fn check_account_in_game(
    account: AccountProfile,
    client_version: String,
) -> (AccountActivityCheck, bool) {
    let account_id = account.id;
    let check = match crate::riot::client::RiotApi::shared() {
        Ok(api) => fetch_account_availability(&api, account, client_version).await,
        Err(_) => AccountActivityCheck {
            account_id,
            availability: AccountAvailability::activity_check_failed(),
        },
    };

    (check, valorant_is_running().await)
}

/// Why an Apply or Restore might not stick: a running game keeps the settings it loaded and can
/// save them over the change.
pub(super) fn settings_change_warning(
    display_name: &str,
    availability: &AccountAvailability,
    valorant_running: bool,
) -> Option<String> {
    let mut warnings = Vec::new();
    if let Some(reason) = availability.unavailable_reason() {
        warnings.push(format!(
            "{display_name} appears to be in VALORANT right now ({reason}), maybe on another PC."
        ));
    }
    if valorant_running {
        warnings.push("VALORANT is running on this PC.".to_string());
    }
    if warnings.is_empty() {
        return None;
    }

    warnings.push(format!(
        "If {display_name} is signed in to the game, it won't see the change and may save its \
         old settings over it. Close VALORANT first."
    ));
    Some(warnings.join(" "))
}

fn fetch_client_version_task(user_requested: bool) -> Task<Message> {
    Task::perform(fetch_current_client_version(), move |result| {
        Message::ClientVersionLoaded {
            user_requested,
            result,
        }
    })
}

fn cache_rank_icons_task(image_cache: &ImageCache) -> Task<Message> {
    Task::perform(
        cache_rank_icons(image_cache.clone()),
        Message::RankIconsLoaded,
    )
}

fn check_capture_prompt_game_task() -> Task<Message> {
    Task::perform(valorant_is_running(), Message::CapturePromptGameChecked)
}

/// Lets Windows page out what the window no longer touches once it is out of sight, mostly GPU
/// driver memory. Nothing stops: the background poll pages back in only what it uses.
fn trim_memory() -> Task<Message> {
    Task::future(async {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
        // SAFETY: the current-process pseudo handle is always valid; usize::MAX for both sizes
        // asks Windows to trim the working set, and a failure only leaves memory as it was.
        unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) };
    })
    .discard()
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

/// Turns imported redirect tokens into a session for this profile, refusing tokens issued for a
/// different Riot account so API calls never act on another account under this profile's name.
fn redirect_session_for_account(
    account: &AccountProfile,
    tokens: RedirectTokens,
) -> Result<AuthSession, String> {
    match tokens.subject() {
        Some(subject) => account
            .check_puuid(&subject)
            .map_err(|error| error.to_string())?,
        None if account.puuid.is_some() => {
            return Err(
                "Prime could not read which Riot account this token belongs to".to_string(),
            );
        }
        None => {}
    }

    Ok(tokens.into_session())
}

/// The name a new preset starts with.
fn default_preset_name(display_name: &str) -> String {
    format!("{display_name} settings")
}

/// Opens Explorer at a folder, or at a file's folder with the file selected. A path that doesn't
/// exist yet (a cleared cache, no accounts saved) opens its nearest existing folder, because
/// Explorer would otherwise open Documents without an error.
fn open_in_explorer(path: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;

    // Explorer splits unquoted arguments on commas, so always quote the path.
    let mut explorer = std::process::Command::new("explorer");
    if path.is_file() {
        explorer.raw_arg(format!("/select,\"{}\"", path.display()));
    } else {
        let folder = path
            .ancestors()
            .find(|folder| folder.is_dir())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "the folder doesn't exist")
            })?;
        explorer.raw_arg(format!("\"{}\"", folder.display()));
    }
    explorer.spawn().map(|_| ())
}
