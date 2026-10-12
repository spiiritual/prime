use tempfile::tempdir;

use crate::account::{
    AccountPenaltyDuration, AccountPenaltyStatus, AccountProfile, CompetitiveRank, Shard,
};

use super::*;

#[test]
fn accounts_live_in_roaming_app_data() {
    let expected = Path::new(&std::env::var_os("APPDATA").expect("APPDATA"))
        .join("spiiritual")
        .join("prime")
        .join("config")
        .join("accounts.json");

    assert_eq!(AccountRepository::default_path(), expected);
}

#[test]
fn older_save_snapshot_never_overwrites_a_newer_one() {
    let dir = tempdir().expect("temp dir");
    let repo = AccountRepository::new(dir.path().join("accounts.json"));
    let mut older = StoredState::default();
    older.push_account(AccountProfile::new("Older", Shard::Na).expect("account"));
    let mut newer = older.clone();
    newer.push_account(AccountProfile::new("Newer", Shard::Na).expect("account"));
    let older_snapshot = repo.snapshot(&older);
    let newer_snapshot = repo.snapshot(&newer);

    repo.save_snapshot(newer_snapshot).expect("save newer");
    repo.save_snapshot(older_snapshot).expect("skip older");

    assert_eq!(repo.load().expect("load"), newer);
}

#[test]
fn saving_unchanged_state_leaves_the_file_alone() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("accounts.json");
    let repo = AccountRepository::new(&path);
    let mut state = StoredState::default();
    state.push_account(AccountProfile::new("Main", Shard::Na).expect("account"));
    repo.save(&state).expect("first save");
    let written = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    fs::File::options()
        .write(true)
        .open(&path)
        .and_then(|file| file.set_modified(written))
        .expect("age file");
    let modified = || {
        fs::metadata(&path)
            .and_then(|m| m.modified())
            .expect("mtime")
    };

    repo.save(&state).expect("unchanged save");
    assert_eq!(modified(), written);

    state.push_account(AccountProfile::new("Alt", Shard::Na).expect("account"));
    repo.save(&state).expect("changed save");
    assert_ne!(modified(), written);
    assert_eq!(repo.load().expect("load"), state);
}

#[test]
fn concurrent_saves_do_not_fail() {
    let dir = tempdir().expect("temp dir");
    let repo = AccountRepository::new(dir.path().join("accounts.json"));
    let snapshots = (0..16)
        .map(|index| {
            let mut state = StoredState::default();
            for account in 0..=index {
                state.push_account(
                    AccountProfile::new(format!("Account {account}"), Shard::Na).expect("account"),
                );
            }
            repo.snapshot(&state)
        })
        .collect::<Vec<_>>();
    let newest = snapshots.last().expect("snapshot").state.clone();

    std::thread::scope(|scope| {
        for snapshot in snapshots {
            let repo = repo.clone();
            scope.spawn(move || repo.save_snapshot(snapshot).expect("save"));
        }
    });

    assert_eq!(repo.load().expect("load"), newest);
}

#[test]
fn missing_file_loads_default_state() {
    let dir = tempdir().expect("temp dir");
    let repo = AccountRepository::new(dir.path().join("missing.json"));

    let state = repo.load().expect("default state");

    assert_eq!(state, StoredState::default());
}

#[test]
fn round_trips_accounts() {
    let dir = tempdir().expect("temp dir");
    let repo = AccountRepository::new(dir.path().join("accounts.json"));
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    account.account_level = Some(123);
    let mut state = StoredState::default();
    let id = account.id;
    state.push_account(account);

    repo.save(&state).expect("save");
    let loaded = repo.load().expect("load");

    assert_eq!(loaded.accounts.len(), 1);
    assert_eq!(loaded.selected_account, Some(id));
    assert_eq!(loaded.accounts[0].display_name, "Main");
    assert_eq!(loaded.accounts[0].account_level, Some(123));
}

#[test]
fn minimize_on_close_defaults_on_and_only_off_is_saved() {
    let dir = tempdir().expect("temp dir");
    let repo = AccountRepository::new(dir.path().join("accounts.json"));
    let mut state = StoredState::default();
    assert!(state.minimize_on_close);

    repo.save(&state).expect("save");
    let saved = fs::read_to_string(repo.path()).expect("read");
    assert!(!saved.contains("minimize_on_close"));
    assert!(repo.load().expect("load").minimize_on_close);

    state.minimize_on_close = false;
    repo.save(&state).expect("save");
    assert!(!repo.load().expect("load").minimize_on_close);
}

#[test]
fn the_chat_proxy_defaults_on_and_only_off_is_saved() {
    let dir = tempdir().expect("temp dir");
    let repo = AccountRepository::new(dir.path().join("accounts.json"));
    let mut state = StoredState::default();
    assert!(state.chat_proxy);

    repo.save(&state).expect("save");
    let saved = fs::read_to_string(repo.path()).expect("read");
    assert!(!saved.contains("chat_proxy"), "{saved}");
    assert!(repo.load().expect("load").chat_proxy);

    state.chat_proxy = false;
    repo.save(&state).expect("save");
    assert!(!repo.load().expect("load").chat_proxy);
}

#[test]
fn presence_status_defaults_to_online_and_only_others_are_saved() {
    let dir = tempdir().expect("temp dir");
    let repo = AccountRepository::new(dir.path().join("accounts.json"));
    let mut state = StoredState::default();
    assert_eq!(
        state.presence_status,
        crate::riot::chat_proxy::PresenceStatus::Online
    );

    repo.save(&state).expect("save");
    let saved = fs::read_to_string(repo.path()).expect("read");
    assert!(!saved.contains("presence_status"), "{saved}");

    state.presence_status = crate::riot::chat_proxy::PresenceStatus::Invisible;
    repo.save(&state).expect("save");
    let saved = fs::read_to_string(repo.path()).expect("read");
    assert!(saved.contains("invisible"), "{saved}");
    assert_eq!(
        repo.load().expect("load").presence_status,
        crate::riot::chat_proxy::PresenceStatus::Invisible
    );
}

#[test]
fn aim_trainer_state_is_only_saved_once_set() {
    let dir = tempdir().expect("temp dir");
    let repo = AccountRepository::new(dir.path().join("accounts.json"));
    let mut state = StoredState::default();

    repo.save(&state).expect("save");
    let saved = fs::read_to_string(repo.path()).expect("read");
    assert!(!saved.contains("aim_trainer"), "{saved}");

    state.aim_trainer.sensitivity = Some(0.34);
    state.aim_trainer.dpi = Some(1600);
    state.aim_trainer.best = Some(crate::aim_trainer::AimRun {
        score: 64,
        misses: 6,
        bursts: 4,
        avg_reaction_ms: Some(412),
        duration_ms: 72_000,
        sensitivity: Some(0.34),
        dpi: Some(1600),
        finished_at_unix: 1_791_331_200,
        unknown: Default::default(),
    });
    repo.save(&state).expect("save");
    assert_eq!(repo.load().expect("load").aim_trainer, state.aim_trainer);
}

#[test]
fn legacy_fields_load_and_are_dropped_on_save() {
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    let raw = serde_json::json!({
        "version": 1,
        "accounts": [{
            "id": account.id,
            "display_name": "Main",
            "username": "player",
            "puuid": null,
            "game_name": null,
            "tag_line": null,
            "shard": "na",
            "session": null,
            "launcher_session": null,
            "competitive_rank": {
                "tier": 15,
                "rank_name": "Gold 1",
                "ranked_rating": 42,
                "season_id": "season-a"
            },
            "account_level": null,
            "last_refreshed_at_unix": 1_800_000_000
        }],
        "selected_account": account.id,
        "riot_client_path": null
    });
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("accounts.json");
    fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).expect("write");
    let repo = AccountRepository::new(path);

    let loaded = repo.load().expect("load");
    repo.save(&loaded).expect("save");
    let saved = fs::read_to_string(repo.path()).expect("saved json");

    assert_eq!(
        loaded.accounts[0]
            .competitive_rank
            .as_ref()
            .map(CompetitiveRank::label)
            .as_deref(),
        Some("Gold 1 - 42 RR")
    );
    for key in ["username", "season_id", "last_refreshed_at_unix"] {
        assert!(!saved.contains(key), "{key} in {saved}");
    }
}

#[test]
fn save_drops_runtime_penalty_status() {
    let dir = tempdir().expect("temp dir");
    let repo = AccountRepository::new(dir.path().join("accounts.json"));
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    account.penalty_status = AccountPenaltyStatus::penalized_for(
        Some("Premier comms".to_string()),
        AccountPenaltyDuration::default(),
    );
    let mut state = StoredState::default();
    state.push_account(account);

    repo.save(&state).expect("save");
    let saved = fs::read_to_string(repo.path()).expect("saved json");

    assert!(!saved.contains("penalty_status"));
    assert!(!saved.contains("Premier comms"));
}

#[test]
fn load_ignores_cached_penalty_status() {
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    let raw = serde_json::json!({
        "version": 1,
        "accounts": [{
            "id": account.id,
            "display_name": "Main",
            "username": "player",
            "puuid": null,
            "game_name": null,
            "tag_line": null,
            "shard": "na",
            "session": null,
            "launcher_session": null,
            "competitive_rank": null,
            "penalty_status": {
                "status": "penalized",
                "penalties": [{
                    "rating_name": "Premier comms",
                    "duration": {
                        "ends_at_unix": null,
                        "games_remaining": null
                    }
                }]
            },
            "account_level": null
        }],
        "selected_account": account.id,
        "riot_client_path": null
    });
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("accounts.json");
    fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).expect("write");
    let repo = AccountRepository::new(path);

    let loaded = repo.load().expect("load");

    assert_eq!(
        loaded.accounts[0].penalty_status,
        AccountPenaltyStatus::Unchecked
    );
}

#[test]
fn load_accepts_utf8_bom() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("accounts.json");
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    let mut state = StoredState::default();
    state.push_account(account);
    let json = serde_json::to_string_pretty(&state).expect("state json");
    fs::write(&path, format!("\u{feff}{json}")).expect("write bom json");
    let repo = AccountRepository::new(path);

    let loaded = repo.load().expect("load");

    assert_eq!(loaded.accounts.len(), 1);
    assert_eq!(loaded.accounts[0].display_name, "Main");
}

#[test]
fn keeps_fields_a_newer_build_wrote() {
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    let mut raw = serde_json::to_value(StoredState {
        selected_account: Some(account.id),
        accounts: vec![account],
        ..StoredState::default()
    })
    .expect("state json");
    raw["beta_setting"] = serde_json::json!({ "token": "secret" });
    raw["accounts"][0]["beta_account_field"] = serde_json::json!(7);
    raw["aim_trainer"] = serde_json::json!({ "beta_mode": "4d" });
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("accounts.json");
    fs::write(&path, raw.to_string()).expect("write");
    let repo = AccountRepository::new(path.clone());

    let state = repo.load().expect("a newer build's file loads");
    repo.save(&state).expect("save");

    let saved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("saved json");
    assert_eq!(saved, raw, "written back as they were");
    let debug = format!("{state:?}");
    assert!(debug.contains("beta_setting") && !debug.contains("secret"));
}

#[test]
fn rejects_selected_account_when_profile_is_missing() {
    let first = AccountProfile::new("First", Shard::Na).expect("first");
    let missing = AccountProfile::new("Missing", Shard::Na).expect("missing");
    let raw = serde_json::json!({
        "version": 1,
        "accounts": [first],
        "selected_account": missing.id,
        "riot_client_path": null
    });
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("accounts.json");
    fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).expect("write");
    let repo = AccountRepository::new(path);

    let error = repo.load().expect_err("invalid selected account");

    assert!(matches!(
        error,
        StorageError::InvalidSelectedAccount(id) if id == missing.id
    ));
}

#[test]
fn rejects_unsupported_storage_version() {
    let raw = serde_json::json!({
        "version": 0,
        "accounts": [],
        "selected_account": null,
        "riot_client_path": null
    });
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("accounts.json");
    fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).expect("write");
    let repo = AccountRepository::new(path);

    let error = repo.load().expect_err("unsupported version");

    assert!(matches!(error, StorageError::UnsupportedVersion(0)));
}
