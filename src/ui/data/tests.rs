use super::*;
use std::path::PathBuf;

use crate::account::{
    AccountId, AccountProfile, AuthSession, LauncherSessionBackup, Shard, ValorantRegion,
};
use crate::storage::StoredState;
use crate::ui::data::session::ApiIdentity;

#[test]
fn cache_account_api_context_leaves_the_account_alone_for_another_riot_account() {
    let mut state = StoredState::default();
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    account.puuid = Some("puuid-a".to_string());
    let account_id = account.id;
    state.push_account(account);

    let result = cache_account_api_context(
        &mut state,
        account_id,
        AuthSession::new("access", None, None, "Bearer", Some(3600), 100),
        None,
        ApiIdentity {
            puuid: "puuid-b".to_string(),
            game_name: None,
            tag_line: None,
            shard: Shard::Eu,
            region: None,
        },
    );

    assert!(result.is_err());
    assert_eq!(state.accounts[0].session, None);
    assert_eq!(state.accounts[0].shard, Shard::Na);
    assert_eq!(state.accounts[0].puuid.as_deref(), Some("puuid-a"));
}

#[test]
fn caching_an_api_context_saves_the_region() {
    let mut state = StoredState::default();
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    account.puuid = Some("puuid-a".to_string());
    let account_id = account.id;
    state.push_account(account);

    cache_account_api_context(
        &mut state,
        account_id,
        AuthSession::new("access", None, None, "Bearer", Some(3600), 100),
        None,
        ApiIdentity {
            puuid: "puuid-a".to_string(),
            game_name: None,
            tag_line: None,
            shard: Shard::Na,
            region: Some(ValorantRegion::Br),
        },
    )
    .expect("cached");

    assert_eq!(state.accounts[0].region, Some(ValorantRegion::Br));
}

#[test]
fn cache_account_api_context_updates_matching_account() {
    let mut state = StoredState::default();
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
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
        None,
        ApiIdentity {
            puuid: "puuid".to_string(),
            game_name: Some("Player".to_string()),
            tag_line: Some("NA1".to_string()),
            shard: Shard::Eu,
            region: None,
        },
    )
    .expect("cache api context");

    assert_eq!(state.accounts[0].session, Some(session));
    assert_eq!(state.accounts[0].puuid.as_deref(), Some("puuid"));
    assert_eq!(state.accounts[0].riot_id().as_deref(), Some("Player#NA1"));
    assert_eq!(state.accounts[0].shard, Shard::Eu);
}

#[test]
fn cache_account_api_context_updates_refreshed_launcher_session() {
    let mut state = StoredState::default();
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    let account_id = account.id;
    account.launcher_session = Some(LauncherSessionBackup {
        data_dir: "old-backup".into(),
        captured_at_unix: 100,
        puuid: "puuid".to_string(),
        unknown: Default::default(),
    });
    state.push_account(account);
    let session = AuthSession::new("access", None, None, "Bearer", Some(3600), 100);
    let launcher_session = LauncherSessionBackup {
        data_dir: "new-backup".into(),
        captured_at_unix: 200,
        puuid: "puuid".to_string(),
        unknown: Default::default(),
    };

    cache_account_api_context(
        &mut state,
        account_id,
        session,
        Some(launcher_session.clone()),
        ApiIdentity {
            puuid: "puuid".to_string(),
            game_name: None,
            tag_line: None,
            shard: Shard::Na,
            region: None,
        },
    )
    .expect("cache api context");

    assert_eq!(state.accounts[0].launcher_session, Some(launcher_session));
}

#[test]
fn cache_account_api_context_rejects_missing_account() {
    let mut state = StoredState::default();
    let session = AuthSession::new("access", None, None, "Bearer", Some(3600), 100);

    let err = cache_account_api_context(
        &mut state,
        AccountId::new(),
        session,
        None,
        ApiIdentity {
            puuid: "puuid".to_string(),
            game_name: None,
            tag_line: None,
            shard: Shard::Na,
            region: None,
        },
    )
    .expect_err("missing account");

    assert!(err.contains("profile no longer exists"));
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
