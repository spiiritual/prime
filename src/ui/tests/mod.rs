use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tempfile::tempdir;

use super::aim::{AimMessage, AimPhase};
use super::app::{
    LaunchPreflightDecision, apply_account_detail_results, cancel_unavailable_launch_state,
    launch_preflight_decision,
};
use super::data::account_details::{
    AccountActivity, AccountAvailability, AccountAvailabilityRefresh, Busy, RefreshedApiContext,
};
use super::data::game_settings::{
    AppliedGameSettingsResult, RestoredGameSettingsResult, SavedGameSettingsResult,
};
use super::data::launch_flow::CapturedAccountDraft;
use super::data::launch_flow::tests::account_with_backup;
use super::data::launch_flow::{LaunchAccountResult, load_accounts};
use super::data::live_match::{
    LiveMatch, LiveMatchError, LiveMatchResult, MatchPhase, shown_weapons,
};
use super::data::loadout::tests::{battle_pass_display, battle_pass_with_chapters, loaded_loadout};
use super::data::loadout::{BattlePassProgressDisplay, LoadoutResult, LoadoutSummary};
use super::data::session::ApiIdentity;
use super::data::session::tests::redirect_url_for_subject;
use super::data::shop::tests::{
    bundle_item_json, featured_bundle_json, priced_bundle_json, summary_with_bundles,
};
use super::data::shop::{BundleItem, StoreSummary};
use super::{Dialog, QuitAction, StatusLaunchFailure, Tab, UnavailableLaunchWarning};
use super::{
    Message, PendingSettingsChange, PendingSettingsCheck, PresetNamePrompt, PresetNameTarget,
    PrimeApp, SettingsChange, Status, StatusKind, countdown_timer_interval,
    masked_account_export_payload, status_bar_visible, status_spinner_active, status_time_left,
    status_visible_at,
};
use crate::account::{
    AccountId, AccountPenaltyStatus, AccountProfile, AuthSession, CompetitiveRank,
    LauncherSessionBackup, Shard, ValorantRegion,
};
use crate::game_settings::{
    GameSettingsProfileMetadata, GameSettingsProfilePurpose, GameSettingsProfileSummary,
};
use crate::riot::chat_proxy::{ChatProxy, PresenceStatus};
use crate::riot::launcher_session::CapturedLauncherSession;
use crate::storage::{AccountRepository, StoredState};

mod accounts;
mod aim_trainer;
mod app_lifecycle;
mod chat_proxy;
mod launch;
mod live_match;
mod loadout;
mod settings_profiles;
mod shell;
mod shop;
mod tab_loading;

#[cfg(not(feature = "image-viewer-testing"))]
#[test]
fn image_viewer_is_disabled_without_testing_feature() {
    assert!(!crate::ui::image_viewer_enabled());
}

#[cfg(feature = "image-viewer-testing")]
#[test]
fn image_viewer_can_be_enabled_for_testing_builds() {
    assert!(crate::ui::image_viewer_enabled());
}

fn test_app(repo_dir: &Path) -> PrimeApp {
    let (mut app, _) = PrimeApp::boot();
    app.repo = AccountRepository::new(repo_dir.join("accounts.json"));
    app.state = StoredState::default();
    app.accounts_loaded = true;
    app.pending_account = None;
    app.show_add_account_prompt = false;
    app.launcher_capture_in_progress = false;
    app.launcher_capture_kind = None;
    app.status = Status::default();
    app
}

fn captured_account_draft(
    backup_root: &Path,
    puuid: &str,
    game_name: &str,
    tag_line: &str,
    shard: Shard,
) -> CapturedAccountDraft {
    let account_id = AccountId::new();
    let data_dir = backup_root.join(account_id.to_string()).join("Data");
    fs::create_dir_all(&data_dir).expect("backup data dir");
    fs::write(data_dir.join("RiotGamesPrivateSettings.yaml"), "settings")
        .expect("private settings");
    CapturedAccountDraft {
        account_id,
        backup: LauncherSessionBackup {
            data_dir,
            captured_at_unix: 100,
            puuid: puuid.to_string(),
            unknown: Default::default(),
        },
        puuid: puuid.to_string(),
        game_name: Some(game_name.to_string()),
        tag_line: Some(tag_line.to_string()),
        shard,
        session: Some(AuthSession::new(
            "access",
            Some("id".to_string()),
            Some("entitlement".to_string()),
            "Bearer",
            Some(3600),
            100,
        )),
    }
}

/// Puts the app in a login capture that has opened Riot Client and is waiting for a sign-in.
fn waiting_login_capture(
    app: &mut PrimeApp,
    target: crate::ui::LoginCaptureTarget,
) -> iced::task::Handle {
    let (_, handle) = iced::Task::<Message>::none().abortable();
    app.launcher_capture_in_progress = true;
    app.launcher_capture_kind = Some(target.kind());
    app.login_capture = Some(crate::ui::LoginCapture {
        wait: Some(handle.clone()),
        ..crate::ui::LoginCapture::new(target)
    });
    handle
}

/// Puts the app in a login capture that is still closing and reopening Riot Client.
fn preparing_login_capture(app: &mut PrimeApp, target: crate::ui::LoginCaptureTarget) {
    app.launcher_capture_in_progress = true;
    app.launcher_capture_kind = Some(target.kind());
    app.login_capture = Some(crate::ui::LoginCapture::new(target));
}

fn empty_store_summary() -> StoreSummary {
    StoreSummary {
        currency_balances: vec![],
        currency_balance_error: None,
        featured_bundles: vec![],
        daily_offers: vec![],
        daily_remaining_seconds: 86_400,
        bundle_remaining_seconds: 86_400,
        night_market_remaining_seconds: None,
        loaded_at: iced::time::Instant::now(),
        night_market_offers: vec![],
        accessory_remaining_seconds: None,
        accessory_offers: vec![],
    }
}

fn availability_refresh(
    account_id: AccountId,
    launcher_session: Option<LauncherSessionBackup>,
) -> Message {
    Message::AccountAvailabilitiesLoaded(AccountAvailabilityRefresh {
        accounts: vec![],
        refreshed_sessions: vec![RefreshedApiContext {
            account_id,
            session: AuthSession::new("fresh", None, None, "Bearer", Some(3600), 100),
            launcher_session,
            identity: ApiIdentity {
                puuid: "Main-puuid".to_string(),
                game_name: None,
                tag_line: None,
                shard: Shard::Na,
                region: None,
            },
        }],
    })
}

fn accounts_tab_app(dir: &Path) -> (PrimeApp, AccountProfile) {
    let mut app = test_app(dir);
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    account.puuid = Some("puuid-a".to_string());
    app.state.push_account(account.clone());
    app.state.select_account(account.id);
    app.client_version_input = "release-1".to_string();
    app.active_tab = crate::ui::Tab::Accounts;
    (app, account)
}

fn two_account_app(dir: &Path) -> (PrimeApp, AccountProfile, AccountProfile) {
    let mut app = test_app(dir);
    let main = AccountProfile::new("Main", Shard::Na).expect("main");
    let alt = AccountProfile::new("Alt", Shard::Na).expect("alt");
    app.state.push_account(main.clone());
    app.state.push_account(alt.clone());
    app.state.select_account(main.id);
    app.client_version_input = "release-1".to_string();
    (app, main, alt)
}

fn settings_api_identity() -> ApiIdentity {
    ApiIdentity {
        puuid: "Main-puuid".to_string(),
        game_name: None,
        tag_line: None,
        shard: Shard::Na,
        region: None,
    }
}

fn applied(
    account_id: AccountId,
    source_profile: GameSettingsProfileMetadata,
    backup_profile: Option<GameSettingsProfileMetadata>,
) -> Message {
    Message::SavedSettingsApplied(Ok(AppliedGameSettingsResult {
        account_id,
        session: AuthSession::new("fresh", None, None, "Bearer", Some(3600), 100),
        launcher_session: None,
        identity: settings_api_identity(),
        source_profile,
        backup_profile,
    }))
}

#[test]
fn window_icon_decodes() {
    assert!(crate::ui::window_icon().is_some());
}
