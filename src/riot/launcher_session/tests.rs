use tempfile::tempdir;

use super::*;

fn fake_id_token(puuid: &str) -> String {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let encode = |value: serde_json::Value| URL_SAFE_NO_PAD.encode(value.to_string());
    format!(
        "{}.{}.signature",
        encode(serde_json::json!({"alg": "RS256", "typ": "id_token+jwt"})),
        encode(serde_json::json!({"sub": puuid, "acct": {"game_name": "Main"}}))
    )
}

fn remembered_login_settings(refresh_token: &str) -> String {
    remembered_login_settings_for("account-puuid", refresh_token)
}

fn remembered_login_settings_for(puuid: &str, refresh_token: &str) -> String {
    format!(
        concat!(
            "psl:\n",
            "    authorization:\n",
            "        riot-client:\n",
            "            claims: []\n",
            "            id_token: \"{}\"\n",
            "            is_dpop_bound: false\n",
            "            refresh_token: \"{}\"\n",
            "            refresh_token_write_count: 1\n",
            "            scopes:\n",
            "            - \"openid\"\n",
            "            - \"account\"\n",
            "riot-login:\n",
            "    persist: null\n",
            "rso-authenticator:\n",
            "    tdid:\n",
            "        name: \"tdid\"\n",
            "        value: \"tdid-value\"\n",
        ),
        fake_id_token(puuid),
        refresh_token
    )
}

fn legacy_ssid_settings() -> &'static str {
    r#"
riot-login:
  persist:
session:
  cookies:
    - domain: "auth.riotgames.com"
      name: "ssid"
      value: "ssid-value"
rso-authenticator:
  tdid:
name: "tdid"
value: "tdid-value"
"#
}

fn backup_at(data_dir: &Path) -> LauncherSessionBackup {
    LauncherSessionBackup {
        data_dir: data_dir.to_path_buf(),
        captured_at_unix: 100,
        puuid: "puuid-value".to_string(),
        unknown: Default::default(),
    }
}

#[test]
fn reads_refresh_token_from_psl_riot_client_authorization() {
    assert_eq!(
        private_settings_refresh_token(&remembered_login_settings("refresh-value")).as_deref(),
        Some("refresh-value")
    );
}

#[test]
fn refresh_token_requires_psl_riot_client_path() {
    let settings = concat!(
        "psl:\n",
        "    authorization:\n",
        "        other-client:\n",
        "            refresh_token: \"other\"\n",
        "refresh_token: \"top-level\"\n",
    );

    assert_eq!(private_settings_refresh_token(settings), None);
    assert_eq!(private_settings_refresh_token(legacy_ssid_settings()), None);
}

#[test]
fn refresh_token_supports_crlf_line_endings() {
    let settings = remembered_login_settings("refresh-value").replace('\n', "\r\n");

    assert_eq!(
        private_settings_refresh_token(&settings).as_deref(),
        Some("refresh-value")
    );
}

#[test]
fn updates_refresh_token_without_rewriting_unrelated_yaml() {
    let original = remembered_login_settings("old-refresh").replace('\n', "\r\n");

    let updated =
        update_private_settings_refresh_token(&original, "new-refresh").expect("update token");

    assert_eq!(
        updated,
        remembered_login_settings("new-refresh").replace('\n', "\r\n")
    );
}

#[test]
fn persists_refreshed_refresh_token_to_backup() {
    let dir = tempdir().expect("backup dir");
    fs::write(
        dir.path().join(PRIVATE_SETTINGS_FILE),
        remembered_login_settings("old-refresh"),
    )
    .expect("settings");
    let backup = backup_at(dir.path());

    persist_refreshed_refresh_token(&backup, "new-refresh").expect("persist token");

    assert_eq!(
        read_backup_refresh_token(&backup).expect("refresh token"),
        "new-refresh"
    );
}

#[test]
fn read_backup_refresh_token_rejects_legacy_ssid_backup() {
    let dir = tempdir().expect("backup dir");
    fs::write(
        dir.path().join(PRIVATE_SETTINGS_FILE),
        legacy_ssid_settings(),
    )
    .expect("settings");

    let err = read_backup_refresh_token(&backup_at(dir.path())).expect_err("legacy backup");

    assert!(matches!(err, LauncherSessionError::MissingRefreshToken));
}

#[test]
fn read_backup_refresh_token_rejects_missing_backup_folder() {
    let backup = backup_at(Path::new("missing-launcher-backup"));

    let err = read_backup_refresh_token(&backup).expect_err("missing backup");

    assert!(matches!(err, LauncherSessionError::BackupMissing(path) if path == backup.data_dir));
}

#[test]
fn read_backup_refresh_token_rejects_missing_private_settings_file() {
    let dir = tempdir().expect("backup dir");
    let backup = backup_at(dir.path());

    let err = read_backup_refresh_token(&backup).expect_err("missing private settings");

    assert!(
        matches!(err, LauncherSessionError::BackupPrivateSettingsMissing(path) if path == backup.data_dir.join(PRIVATE_SETTINGS_FILE))
    );
}

#[test]
fn clears_launcher_data_dir_without_removing_directory() {
    let data_dir = tempdir().expect("data dir");
    fs::write(data_dir.path().join("old.txt"), "old").expect("old file");
    fs::create_dir(data_dir.path().join("nested")).expect("nested dir");
    fs::write(data_dir.path().join("nested").join("old.txt"), "old").expect("nested file");

    clear_dir(data_dir.path()).expect("clear");

    assert!(data_dir.path().exists());
    assert_eq!(fs::read_dir(data_dir.path()).expect("read dir").count(), 0);
}

#[test]
fn captures_remembered_login_data_folder_backup() {
    let account_id = AccountId::new();
    let source = tempdir().expect("source");
    let backup_root = tempdir().expect("backup");
    fs::write(
        source.path().join(PRIVATE_SETTINGS_FILE),
        remembered_login_settings("refresh-value"),
    )
    .expect("settings");
    fs::create_dir(source.path().join("Config")).expect("nested dir");
    fs::write(source.path().join("Config").join("state.bin"), "state").expect("nested file");

    let captured =
        capture_launcher_session_from_data_dir(account_id, source.path(), backup_root.path())
            .expect("capture");

    assert_eq!(captured.account_id, account_id);
    assert!(captured.backup.puuid.is_empty());
    assert_eq!(
        read_backup_refresh_token(&captured.backup).expect("refresh token"),
        "refresh-value"
    );
    assert!(
        captured
            .backup
            .data_dir
            .join("Config")
            .join("state.bin")
            .exists()
    );
}

#[test]
fn ready_launcher_data_dir_requires_remembered_refresh_token() {
    let legacy = tempdir().expect("legacy");
    let signed_out = tempdir().expect("signed out");
    let remembered = tempdir().expect("remembered");
    fs::write(
        legacy.path().join(PRIVATE_SETTINGS_FILE),
        legacy_ssid_settings(),
    )
    .expect("legacy settings");
    fs::write(
        signed_out.path().join(PRIVATE_SETTINGS_FILE),
        "riot-login:\n    persist: null\n",
    )
    .expect("signed out settings");
    fs::write(
        remembered.path().join(PRIVATE_SETTINGS_FILE),
        remembered_login_settings("refresh-value"),
    )
    .expect("remembered settings");

    let ready = ready_launcher_data_dir(vec![
        legacy.path().to_path_buf(),
        signed_out.path().to_path_buf(),
        remembered.path().to_path_buf(),
    ])
    .expect("ready data dir");

    assert_eq!(ready, remembered.path());
}

#[test]
fn rejects_capture_without_remembered_refresh_token() {
    let account_id = AccountId::new();
    let source = tempdir().expect("source");
    let backup_root = tempdir().expect("backup");
    fs::write(
        source.path().join(PRIVATE_SETTINGS_FILE),
        legacy_ssid_settings(),
    )
    .expect("settings");

    let err = capture_launcher_session_from_data_dir(account_id, source.path(), backup_root.path())
        .expect_err("missing refresh token");

    assert!(matches!(err, LauncherSessionError::MissingRefreshToken));
    assert!(!backup_root.path().join(account_id.to_string()).exists());
}

#[test]
fn failed_copy_keeps_the_previous_backup_intact() {
    use std::os::windows::fs::OpenOptionsExt;

    let source = tempdir().expect("source");
    let target_root = tempdir().expect("target root");
    let target = target_root.path().join("Data");
    fs::create_dir_all(&target).expect("target");
    fs::write(target.join("old.txt"), "old").expect("old file");
    fs::write(source.path().join("a.txt"), "new").expect("new file");
    fs::write(source.path().join("locked.txt"), "locked").expect("locked file");
    let _lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(source.path().join("locked.txt"))
        .expect("lock source file");

    replace_dir_contents(source.path(), &target).expect_err("locked source file");

    assert_eq!(
        fs::read_to_string(target.join("old.txt")).expect("old file"),
        "old"
    );
    assert!(!target.join("a.txt").exists());
    assert_eq!(
        fs::read_dir(target_root.path())
            .expect("target root")
            .count(),
        1,
        "no leftover staging folders"
    );
}

#[test]
fn replacing_contents_leaves_no_staging_folders() {
    let source = tempdir().expect("source");
    let target_root = tempdir().expect("target root");
    let target = target_root.path().join("Data");
    fs::create_dir_all(&target).expect("target");
    fs::write(target.join("old.txt"), "old").expect("old file");
    fs::create_dir(source.path().join("nested")).expect("nested");
    fs::write(source.path().join("nested").join("a.txt"), "new").expect("new file");

    replace_dir_contents(source.path(), &target).expect("replace");

    assert!(!target.join("old.txt").exists());
    assert_eq!(
        fs::read_to_string(target.join("nested").join("a.txt")).expect("new file"),
        "new"
    );
    assert_eq!(
        fs::read_dir(target_root.path())
            .expect("target root")
            .count(),
        1
    );
}

#[test]
fn applies_backup_by_replacing_target_data_folder() {
    let backup_source = tempdir().expect("backup source");
    let target = tempdir().expect("target");
    let backup = backup_at(backup_source.path());
    fs::write(backup_source.path().join(PRIVATE_SETTINGS_FILE), "new").expect("backup file");
    fs::write(target.path().join("old.txt"), "old").expect("old file");

    apply_launcher_session_backup_to_dir(&backup, target.path()).expect("apply");

    assert!(!target.path().join("old.txt").exists());
    assert_eq!(
        fs::read_to_string(target.path().join(PRIVATE_SETTINGS_FILE)).expect("new file"),
        "new"
    );
}

#[test]
fn syncs_live_launcher_data_back_to_backup() {
    let source = tempdir().expect("source");
    let backup_dir = tempdir().expect("backup");
    fs::write(
        source.path().join(PRIVATE_SETTINGS_FILE),
        remembered_login_settings("live-refresh"),
    )
    .expect("settings");
    fs::create_dir(source.path().join("Config")).expect("source nested dir");
    fs::write(source.path().join("Config").join("state.bin"), "live").expect("source nested file");
    fs::write(backup_dir.path().join(PRIVATE_SETTINGS_FILE), "old").expect("old settings");
    fs::write(backup_dir.path().join("old.txt"), "old").expect("old file");
    let backup = LauncherSessionBackup {
        data_dir: backup_dir.path().to_path_buf(),
        captured_at_unix: 100,
        puuid: "account-puuid".to_string(),
        unknown: Default::default(),
    };

    let synced =
        sync_launcher_session_backup_from_data_dir(&backup, source.path()).expect("sync backup");

    assert_eq!(synced.data_dir, backup.data_dir);
    assert_eq!(synced.puuid, "account-puuid");
    assert!(synced.captured_at_unix >= backup.captured_at_unix);
    assert!(!backup_dir.path().join("old.txt").exists());
    assert_eq!(
        fs::read_to_string(backup_dir.path().join("Config").join("state.bin"))
            .expect("nested file"),
        "live"
    );
    assert_eq!(
        read_backup_refresh_token(&synced).expect("refresh token"),
        "live-refresh"
    );
}

#[test]
fn reads_signed_in_puuid_from_id_token() {
    assert_eq!(
        private_settings_signed_in_puuid(&remembered_login_settings_for("abc-123", "refresh"))
            .as_deref(),
        Some("abc-123")
    );
    assert_eq!(
        private_settings_signed_in_puuid(legacy_ssid_settings()),
        None
    );
}

#[test]
fn syncs_the_signed_in_account_before_switching() {
    let source = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    fs::write(
        source.path().join(PRIVATE_SETTINGS_FILE),
        remembered_login_settings_for("second-puuid", "rotated-refresh"),
    )
    .expect("settings");
    let first_dir = backups.path().join("first");
    let second_dir = backups.path().join("second");
    for (dir, puuid) in [(&first_dir, "first-puuid"), (&second_dir, "second-puuid")] {
        fs::create_dir(dir).expect("backup dir");
        fs::write(
            dir.join(PRIVATE_SETTINGS_FILE),
            remembered_login_settings_for(puuid, "old-refresh"),
        )
        .expect("backup settings");
    }
    let first_id = AccountId::new();
    let second_id = AccountId::new();
    let saved = [
        (
            first_id,
            LauncherSessionBackup {
                data_dir: first_dir.clone(),
                captured_at_unix: 100,
                puuid: "first-puuid".to_string(),
                unknown: Default::default(),
            },
        ),
        (
            second_id,
            LauncherSessionBackup {
                data_dir: second_dir.clone(),
                captured_at_unix: 100,
                puuid: "SECOND-PUUID".to_string(),
                unknown: Default::default(),
            },
        ),
    ];

    let (synced_id, synced) = sync_signed_in_launcher_session_from_data_dir(&saved, source.path())
        .expect("sync")
        .expect("signed-in account is saved");

    assert_eq!(synced_id, second_id);
    assert_eq!(synced.data_dir, second_dir);
    assert!(
        fs::read_to_string(second_dir.join(PRIVATE_SETTINGS_FILE))
            .expect("second settings")
            .contains("rotated-refresh")
    );
    assert!(
        fs::read_to_string(first_dir.join(PRIVATE_SETTINGS_FILE))
            .expect("first settings")
            .contains("old-refresh")
    );
}

#[test]
fn switching_leaves_backups_alone_when_the_live_login_is_not_saved() {
    let source = tempdir().expect("source");
    let backup_dir = tempdir().expect("backup");
    fs::write(
        source.path().join(PRIVATE_SETTINGS_FILE),
        remembered_login_settings_for("stranger-puuid", "live-refresh"),
    )
    .expect("settings");
    fs::write(backup_dir.path().join(PRIVATE_SETTINGS_FILE), "old").expect("old settings");
    let saved = [(AccountId::new(), backup_at(backup_dir.path()))];

    let synced =
        sync_signed_in_launcher_session_from_data_dir(&saved, source.path()).expect("sync");

    assert_eq!(synced, None);
    assert_eq!(
        fs::read_to_string(backup_dir.path().join(PRIVATE_SETTINGS_FILE)).expect("backup settings"),
        "old"
    );
}

#[test]
fn sync_refuses_when_a_different_account_is_signed_in() {
    let source = tempdir().expect("source");
    let backup_dir = tempdir().expect("backup");
    fs::write(
        source.path().join(PRIVATE_SETTINGS_FILE),
        remembered_login_settings_for("someone-else", "other-refresh"),
    )
    .expect("live settings");
    fs::write(backup_dir.path().join(PRIVATE_SETTINGS_FILE), "old").expect("old settings");
    let backup = LauncherSessionBackup {
        data_dir: backup_dir.path().to_path_buf(),
        captured_at_unix: 100,
        puuid: "account-puuid".to_string(),
        unknown: Default::default(),
    };

    let err = sync_launcher_session_backup_from_data_dir(&backup, source.path())
        .expect_err("different account");

    assert!(matches!(
        err,
        LauncherSessionError::DifferentAccountSignedIn
    ));
    assert_eq!(
        fs::read_to_string(backup_dir.path().join(PRIVATE_SETTINGS_FILE)).expect("settings"),
        "old"
    );
}

#[test]
fn sync_refuses_when_the_signed_in_account_is_unknown() {
    let source = tempdir().expect("source");
    let backup_dir = tempdir().expect("backup");
    fs::write(
        source.path().join(PRIVATE_SETTINGS_FILE),
        "psl:\n    authorization:\n        riot-client:\n            refresh_token: \"refresh\"\n",
    )
    .expect("live settings");
    fs::write(backup_dir.path().join(PRIVATE_SETTINGS_FILE), "old").expect("old settings");

    let err =
        sync_launcher_session_backup_from_data_dir(&backup_at(backup_dir.path()), source.path())
            .expect_err("unknown account");

    assert!(matches!(
        err,
        LauncherSessionError::DifferentAccountSignedIn
    ));
    assert_eq!(
        fs::read_to_string(backup_dir.path().join(PRIVATE_SETTINGS_FILE)).expect("settings"),
        "old"
    );
}

#[test]
fn removes_legacy_launcher_backups_only() {
    let backup_root = tempdir().expect("backup root");
    let legacy_slot = backup_root.path().join(AccountId::new().to_string());
    let current_slot = backup_root.path().join(AccountId::new().to_string());
    let unrelated_slot = backup_root.path().join("unrelated");
    let unknown_format_slot = backup_root.path().join(AccountId::new().to_string());
    fs::create_dir_all(unknown_format_slot.join("Data")).expect("unknown format slot");
    fs::write(
        unknown_format_slot.join("Data").join(PRIVATE_SETTINGS_FILE),
        "future-login:
session: \"opaque\"
",
    )
    .expect("unknown format settings");
    fs::create_dir_all(legacy_slot.join("Data")).expect("legacy slot");
    fs::create_dir_all(current_slot.join("Data")).expect("current slot");
    fs::create_dir_all(&unrelated_slot).expect("unrelated slot");
    fs::write(
        legacy_slot.join("Data").join(PRIVATE_SETTINGS_FILE),
        legacy_ssid_settings(),
    )
    .expect("legacy settings");
    fs::write(
        current_slot.join("Data").join(PRIVATE_SETTINGS_FILE),
        remembered_login_settings("refresh-value"),
    )
    .expect("current settings");

    let removed = remove_legacy_launcher_backups(backup_root.path()).expect("remove legacy");

    assert_eq!(removed, vec![legacy_slot.clone()]);
    assert!(!legacy_slot.exists());
    assert!(current_slot.exists());
    assert!(unrelated_slot.exists());
    assert!(unknown_format_slot.exists());
}

#[test]
fn removes_backup_slots_no_account_references() {
    let backup_root = tempdir().expect("backup root");
    let referenced_slot = backup_root.path().join(AccountId::new().to_string());
    let orphaned_slot = backup_root.path().join(AccountId::new().to_string());
    let unrelated_dir = backup_root.path().join("keep-me");
    for dir in [&referenced_slot, &orphaned_slot, &unrelated_dir] {
        fs::create_dir_all(dir.join("Data")).expect("slot");
    }

    let removed =
        remove_unreferenced_launcher_backups(backup_root.path(), &[referenced_slot.join("Data")])
            .expect("remove unreferenced");

    assert_eq!(removed, vec![orphaned_slot.clone()]);
    assert!(referenced_slot.exists());
    assert!(!orphaned_slot.exists());
    assert!(unrelated_dir.exists());
}

#[test]
fn removing_legacy_backups_ignores_missing_backup_root() {
    let removed = remove_legacy_launcher_backups(Path::new("missing-launcher-backups"))
        .expect("missing root");

    assert!(removed.is_empty());
}

#[test]
fn removes_captured_launcher_backup_slot() {
    let backup_root = tempdir().expect("backup root");
    let account_id = AccountId::new();
    let slot = backup_root.path().join(account_id.to_string());
    fs::create_dir_all(slot.join("Data")).expect("slot");
    fs::write(slot.join("Data").join(PRIVATE_SETTINGS_FILE), "settings").expect("settings");

    remove_launcher_session_backup(backup_root.path(), account_id).expect("remove backup");

    assert!(!slot.exists());
}

#[test]
fn adopts_captured_launcher_backup_into_existing_slot() {
    let backup_root = tempdir().expect("backup root");
    let source_id = AccountId::new();
    let target_id = AccountId::new();
    let source_data = backup_root.path().join(source_id.to_string()).join("Data");
    let target_slot = backup_root.path().join(target_id.to_string());
    let target_data = target_slot.join("Data");
    fs::create_dir_all(&source_data).expect("source data");
    fs::create_dir_all(&target_data).expect("target data");
    fs::write(source_data.join(PRIVATE_SETTINGS_FILE), "new").expect("source settings");
    fs::write(target_data.join("old.txt"), "old").expect("old target file");
    let backup = backup_at(&source_data);

    let adopted = adopt_launcher_session_backup(backup_root.path(), source_id, target_id, backup)
        .expect("adopt backup");

    assert_eq!(adopted.data_dir, target_data);
    assert_eq!(adopted.puuid, "puuid-value");
    assert!(!backup_root.path().join(source_id.to_string()).exists());
    assert_eq!(
        fs::read_to_string(adopted.data_dir.join(PRIVATE_SETTINGS_FILE)).expect("settings"),
        "new"
    );
    assert!(!adopted.data_dir.join("old.txt").exists());
}
