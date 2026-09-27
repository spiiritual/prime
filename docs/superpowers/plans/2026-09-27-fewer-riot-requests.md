# Fewer Riot Requests Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Open the settings Apply/Restore dialog instantly and cut a typical Apply from 7 Riot requests to 2.

**Architecture:** One shared `reqwest` client for all Riot calls. Identity comes from the access
token's subject rather than `userinfo`, and the region is saved on the account after the first
Riot Geo lookup. The settings dialog opens at once, reuses the background poll's activity result
when it's under 90 seconds old, and otherwise checks match and party in parallel while the dialog
is open, saving any session the check obtains.

**Tech Stack:** Rust 2024, Iced 0.14, reqwest, serde, `iced::futures::join!`.

**Spec:** `docs/superpowers/specs/2026-09-27-fewer-riot-requests-design.md`

**Deviations from the spec (agreed in chat):**
- No automatic 404 retry. Region changes are rare, so the account's existing Refresh action
  forgets the saved region, and the next request looks it up again through Riot Geo.
- The shared client is a process-wide `RiotApi::shared()`, not a field on `PrimeApp` threaded
  through every loader.
- Freshness uses the poll's existing `account_availability_loaded_at`, not a per-account time.

Task 5 updates the spec to match.

## Global Constraints

- Follow AGENTS.md: edit with the Edit and Write tools only (no scripts or `sed`); use the LSP
  before grep; run each new test and watch it fail before writing the code.
- Before using or saving a session for an account, check it belongs to that account's PUUID
  (`AccountProfile::check_puuid`).
- UI tests assert on state and `task.units()` and never run tasks; build apps with `test_app` or
  `settings_app`.
- Keep `cargo clippy --all-targets` clean with and without `--features settings-cloning`.
- Run `cargo test` with and without `--features settings-cloning` before each commit.
- `rustfmt --edition 2024 <files you changed>` before committing. Don't run `cargo fmt` on the
  whole crate: `account_details.rs`, `launch_flow.rs` and `shop.rs` have unrelated pre-existing
  format differences.
- Fresh activity result: under 90 seconds old.

## Review Focus

- An `accounts.json` written by the current release (no `region` field) must still load. Task 3
  Step 1 tests this.
- A token whose subject is another Riot account must never be used or saved. Task 3 Step 1 tests
  `api_identity` with a mismatched subject.
- A token with no readable subject (an imported redirect token without JWT claims) must still work
  by falling back to `userinfo`. Task 3 Step 1 tests `api_identity` with no subject.
- Confirming while the check is still running must start the change, and the late check result
  must not reopen or alter anything except the cached availability and session. Task 4 Step 1
  tests both.
- A check that fails (network down) must leave the dialog usable with a note, not stuck in
  "Checking". Task 4 Step 1 tests an `Unknown` result.

---

### Task 0: Commit the expanded preset view first

The working tree holds the finished "Show all settings" work, uncommitted. Commit it on its own so
this plan's commits stay separate. `AGENTS.md` is also modified; leave it unstaged here, since
Task 5 edits it.

- [ ] **Step 1: Check the tests pass**

Run: `cargo test --features settings-cloning` and `cargo test`
Expected: all pass.

- [ ] **Step 2: Commit**

```bash
git add src/game_settings.rs src/game_settings/summary.rs src/ui/app.rs src/ui/mod.rs src/ui/screens/game_settings.rs src/ui/tests.rs
git commit -m "Show all of a preset's settings on its card"
```

---

### Task 1: One shared HTTP client

**Files:**
- Modify: `src/riot/client.rs` (next to `RiotApi::new`)
- Modify: every `RiotApi::new()` in `src/ui` except `src/ui/tests.rs`: `src/ui/app.rs`,
  `src/ui/data/account_details.rs` (3), `src/ui/data/game_settings.rs` (3),
  `src/ui/data/launch_flow.rs`, `src/ui/data/loadout.rs`, `src/ui/data/shop.rs`

**Interfaces:**
- Produces: `RiotApi::shared() -> Result<RiotApi, RiotApiError>`, a clone of one process-wide
  client. Later tasks call `RiotApi::shared()` wherever they need a client.

- [ ] **Step 1: Read how `RiotApi::new` builds the client and what it returns**

Use the LSP (`goToDefinition` on `RiotApi::new` in `src/ui/data/shop.rs:844`). Note its error
type. The code below assumes `Result<Self, RiotApiError>` and that `RiotApiError::Http` wraps
`reqwest::Error`; adjust the error mapping if `new` differs.

- [ ] **Step 2: Add `shared`**

In `src/riot/client.rs`, inside `impl RiotApi`, after `new`:

```rust
    /// One client for the whole app, so requests to the same Riot host reuse connections.
    /// `reqwest::Client` is reference-counted, so each clone shares the same pool.
    pub fn shared() -> Result<Self, RiotApiError> {
        static SHARED: std::sync::LazyLock<Result<RiotApi, String>> =
            std::sync::LazyLock::new(|| RiotApi::new().map_err(|error| error.to_string()));

        SHARED
            .as_ref()
            .cloned()
            .map_err(|error| RiotApiError::ClientSetup(error.clone()))
    }
```

and a variant on `RiotApiError`:

```rust
    #[error("could not set up the Riot HTTP client: {0}")]
    ClientSetup(String),
```

- [ ] **Step 3: Replace the call sites**

In each file listed above, change `RiotApi::new()` to `RiotApi::shared()` (Edit tool, one per
site). `crate::riot::client::RiotApi::new()` in `src/ui/app.rs` becomes
`crate::riot::client::RiotApi::shared()`. Leave `src/ui/tests.rs` alone.

- [ ] **Step 4: Build, test, lint**

Run: `cargo test --features settings-cloning`, `cargo test`, `cargo clippy --all-targets --features settings-cloning`, `cargo clippy --all-targets`
Expected: all pass, no warnings. (No new test: one shared value behind `LazyLock`, and every
existing loader test path still compiles against it.)

- [ ] **Step 5: Commit**

```bash
git add src/riot/client.rs src/ui/app.rs src/ui/data
git commit -m "Share one Riot HTTP client across the app"
```

---

### Task 2: Parallel activity requests, and an in-game-only check

**Files:**
- Modify: `src/ui/data/account_details.rs:437-503` (`check_account_availability`,
  `fetch_resolved_account_activity`)
- Test: `src/ui/data/account_details.rs` has no request-level tests; classification is covered by
  `classify_account_activity` tests in `src/ui/tests.rs`

**Interfaces:**
- Produces: `pub(in crate::ui) async fn check_settings_activity(account: AccountProfile, client_version: String) -> (AccountActivityCheck, Option<RefreshedApiContext>)`,
  which asks only core-game and party, in parallel. Task 4 calls it.
- `check_account_availability` gains a `detail: ActivityDetail` parameter.

- [ ] **Step 1: Add the detail level and run the requests in parallel**

Replace `fetch_resolved_account_activity` with:

```rust
/// How much of the account's activity a check needs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum ActivityDetail {
    /// Match, agent select or lobby, for the Accounts tab and Launch's warning.
    Full,
    /// Only whether the account is in the game: match or party. One request fewer.
    InGame,
}

async fn fetch_resolved_account_activity(
    api: &RiotApi,
    credentials: &ApiCredentials,
    region: ValorantRegion,
    detail: ActivityDetail,
) -> AccountActivity {
    let pregame = async {
        match detail {
            ActivityDetail::Full => {
                account_activity_probe(api.pregame_player(credentials, region).await)
            }
            // Agent select is covered by the party request: players stay in their party.
            ActivityDetail::InGame => AccountActivityProbe::NotFound,
        }
    };
    let (current_game, pregame, party) = iced::futures::join!(
        async { account_activity_probe(api.current_game_player(credentials, region).await) },
        pregame,
        async { account_activity_probe(api.party_player(credentials, region).await) },
    );

    classify_account_activity(current_game, pregame, party)
}
```

`classify_account_activity` already applies match, then agent select, then lobby precedence, so
the Accounts tab shows the same states as before.

- [ ] **Step 2: Thread the detail through `check_account_availability`**

Add `detail: ActivityDetail` as its last parameter and pass it to
`fetch_resolved_account_activity(api, &resolved.credentials, region, detail)`. Update its two
callers, `fetch_account_availabilities` and `fetch_account_availability`, to pass
`ActivityDetail::Full`.

- [ ] **Step 3: Add the settings check**

Below `fetch_account_availability`:

```rust
/// Whether the account is in VALORANT, for the settings Apply and Restore warning, with any
/// session the check obtained so the caller can save it.
pub(in crate::ui) async fn check_settings_activity(
    account: AccountProfile,
    client_version: String,
) -> (AccountActivityCheck, Option<RefreshedApiContext>) {
    match RiotApi::shared() {
        Ok(api) => {
            check_account_availability(&api, account, client_version, ActivityDetail::InGame)
                .await
        }
        Err(_) => (
            AccountActivityCheck {
                account_id: account.id,
                availability: AccountAvailability::activity_check_failed(),
            },
            None,
        ),
    }
}
```

- [ ] **Step 4: Build, test, lint**

Run: `cargo test --features settings-cloning`, `cargo test`, both clippy commands.
Expected: all pass. `check_settings_activity` is unused until Task 4; if clippy flags it as dead
code, add `#[allow(dead_code)]` on it for this commit only and remove the allow in Task 4.

- [ ] **Step 5: Commit**

```bash
git add src/ui/data/account_details.rs
git commit -m "Send activity requests in parallel and add an in-game-only check"
```

---

### Task 3: Identity from the token, and a saved region

**Files:**
- Modify: `src/account.rs:89-137` (`ValorantRegion` serde), `src/account.rs:408-467`
  (`AccountProfile.region`, `new`)
- Modify: `src/ui/data/session.rs:5-83` (`ApiIdentity.region`, `resolve_credentials`,
  `api_identity`)
- Modify: `src/ui/data.rs:96-131` (`cache_account_api_context` saves the region)
- Modify: `src/ui/data/game_settings.rs:242-275` (`resolve_settings_context` uses
  `resolve_credentials`)
- Modify: `src/ui/app.rs:954-996` (`ProfileIdentityLoaded` forgets the saved region)
- Modify: `src/ui/tests.rs`: the 9 `ApiIdentity { .. }` literals gain `region: None`; the
  `AccountProfile { .. }` literal at about line 2409 gains `region: None`; the 3 `api_identity(..)`
  calls gain the new argument
- Test: `src/account.rs` tests, `src/ui/tests.rs`

**Interfaces:**
- Produces: `AccountProfile.region: Option<ValorantRegion>`; `ApiIdentity.region: Option<ValorantRegion>`;
  `api_identity(account: &AccountProfile, player_info: Option<&PlayerInfoResponse>, token_subject: Option<&str>, shard: Shard) -> Result<ApiIdentity, String>`.

- [ ] **Step 1: Write the failing tests**

In `src/account.rs`'s test module (find an existing `#[cfg(test)] mod tests` with the LSP
`documentSymbol`; add one if there is none):

```rust
    #[test]
    fn a_profile_saved_without_a_region_still_loads() {
        let mut value = serde_json::to_value(
            AccountProfile::new("Main", None, Shard::Na).expect("account"),
        )
        .expect("serialize");
        value.as_object_mut().expect("object").remove("region");

        let profile: AccountProfile = serde_json::from_value(value).expect("loads");

        assert_eq!(profile.region, None);
    }

    #[test]
    fn a_saved_region_round_trips() {
        let mut profile = AccountProfile::new("Main", None, Shard::Na).expect("account");
        profile.region = Some(ValorantRegion::Latam);

        let json = serde_json::to_string(&profile).expect("serialize");
        let loaded: AccountProfile = serde_json::from_str(&json).expect("loads");

        assert_eq!(loaded.region, Some(ValorantRegion::Latam));
        assert!(json.contains("\"region\":\"latam\""), "{json}");
    }
```

In `src/ui/tests.rs`, next to the existing `api_identity` tests (about line 3140):

```rust
#[test]
fn api_identity_takes_the_puuid_from_the_token_subject() {
    let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
    account.puuid = Some("puuid-a".to_string());

    let identity = api_identity(&account, None, Some("puuid-a"), Shard::Na).expect("identity");

    assert_eq!(identity.puuid, "puuid-a");
    assert_eq!(identity.game_name, None);
}

#[test]
fn api_identity_refuses_a_token_for_another_riot_account() {
    let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
    account.puuid = Some("puuid-a".to_string());

    assert!(api_identity(&account, None, Some("puuid-b"), Shard::Na).is_err());
}

#[test]
fn api_identity_without_a_token_subject_uses_the_saved_puuid() {
    let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
    account.puuid = Some("puuid-a".to_string());

    let identity = api_identity(&account, None, None, Shard::Na).expect("identity");

    assert_eq!(identity.puuid, "puuid-a");
}

#[test]
fn caching_an_api_context_saves_the_region() {
    let mut state = StoredState::default();
    let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
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
fn refreshing_a_profile_forgets_its_saved_region() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
    account.puuid = Some("puuid-a".to_string());
    account.region = Some(ValorantRegion::Na);
    let account_id = account.id;
    app.state.push_account(account);

    let _ = app.update(Message::ProfileIdentityLoaded(
        account_id,
        Ok(super::data::account_details::RefreshedProfileIdentity {
            account_id,
            session: AuthSession::new("access", None, None, "Bearer", Some(3600), 100),
            launcher_session: None,
            puuid: "puuid-a".to_string(),
            game_name: "Main".to_string(),
            tag_line: "NA1".to_string(),
        }),
    ));

    assert_eq!(app.state.accounts[0].region, None);
}
```

Check `RefreshedProfileIdentity`'s field types with the LSP `hover` before running. If
`game_name` and `tag_line` are `Option<String>`, wrap the values in `Some`. Import
`ValorantRegion`, `StoredState` and `cache_account_api_context` in `src/ui/tests.rs` if they
aren't imported already.

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test --features settings-cloning --lib`
Expected: compile errors for `region` and the new `api_identity` argument. Add only the fields
(`pub region: Option<ValorantRegion>` with `#[serde(default)]` on `AccountProfile`,
`region: None` in `AccountProfile::new`, `pub(in crate::ui) region: Option<ValorantRegion>` on
`ApiIdentity`) and the unused `token_subject: Option<&str>` parameter so it compiles, add
`region: None` to the test literals, then run again.
Expected: `a_saved_region_round_trips` fails (no serde on `ValorantRegion`, so it won't compile
yet; add `Serialize, Deserialize` and `#[serde(rename_all = "lowercase")]` to `ValorantRegion`
as part of this step), `api_identity_takes_the_puuid_from_the_token_subject` and
`api_identity_refuses_a_token_for_another_riot_account` fail,
`caching_an_api_context_saves_the_region` fails, `refreshing_a_profile_forgets_its_saved_region`
fails.

- [ ] **Step 3: Implement**

`api_identity` in `src/ui/data/session.rs`: take the PUUID from `userinfo`, then the token's
subject, then the saved values:

```rust
pub(in crate::ui) fn api_identity(
    account: &AccountProfile,
    player_info: Option<&PlayerInfoResponse>,
    token_subject: Option<&str>,
    shard: Shard,
) -> Result<ApiIdentity, String> {
    let puuid = player_info
        .map(|info| info.sub.trim().to_string())
        .or_else(|| token_subject.map(|subject| subject.trim().to_string()))
        .or_else(|| account.puuid.clone())
        .or_else(|| {
            account
                .launcher_session
                .as_ref()
                .map(|backup| backup.puuid.clone())
        })
        .filter(|puuid| !puuid.trim().is_empty())
        .ok_or_else(|| "selected account does not have a Riot PUUID".to_string())?;
    account
        .check_puuid(&puuid)
        .map_err(|error| error.to_string())?;

    Ok(ApiIdentity {
        puuid,
        game_name: player_info.map(|info| info.acct.game_name.clone()),
        tag_line: player_info.map(|info| info.acct.tag_line.clone()),
        shard,
        region: None,
    })
}
```

`resolve_credentials`: call `userinfo` only when the token has no subject, and use the saved
region before Riot Geo:

```rust
pub(in crate::ui) async fn resolve_credentials(
    api: &RiotApi,
    account: &AccountProfile,
    client_version: String,
) -> Result<ResolvedApiCredentials, String> {
    let api_session = active_api_session(api, account).await?;
    let mut session = api_session.session;
    let token_subject = crate::riot::auth::jwt_subject(&session.access_token);
    // The token already names its account; userinfo is only needed when it doesn't.
    let player_info = match token_subject {
        Some(_) => None,
        None => api.player_info(&session.access_token).await.ok(),
    };

    let entitlements_token = entitlement_token(api, &session).await?;
    if session
        .entitlements_token
        .as_ref()
        .is_none_or(|token| token.trim().is_empty())
    {
        session.entitlements_token = Some(entitlements_token.clone());
    }

    let mut identity = api_identity(
        account,
        player_info.as_ref(),
        token_subject.as_deref(),
        account.shard,
    )?;
    let region = match account.region {
        Some(region) => Some(region),
        None => resolve_session_region(api, &session, player_info.as_ref())
            .await
            .ok(),
    };
    identity.region = region;
    identity.shard = match region {
        Some(region) => region.shard(),
        None => resolve_session_shard(api, &session, player_info.as_ref(), account.shard).await,
    };

    Ok(ResolvedApiCredentials {
        credentials: ApiCredentials {
            access_token: session.access_token.clone(),
            entitlements_token,
            client_version,
            shard: identity.shard,
            puuid: identity.puuid.clone(),
        },
        region,
        session,
        launcher_session: api_session.launcher_session,
        identity,
    })
}
```

`cache_account_api_context` in `src/ui/data.rs`, after `account.shard = identity.shard;`:

```rust
    if identity.region.is_some() {
        account.region = identity.region;
    }
```

`resolve_settings_context` in `src/ui/data/game_settings.rs`: reuse `resolve_credentials`
instead of its own `userinfo` and Geo calls. It needs a client version, which
`resolve_credentials` only forwards into `ApiCredentials`; player preferences don't use it, so
pass an empty string:

```rust
async fn resolve_settings_context(
    api: &RiotApi,
    account: &AccountProfile,
) -> Result<SettingsContext, String> {
    // Player preferences don't send the client version.
    let resolved = resolve_credentials(api, account, String::new())
        .await
        .map_err(|error| {
            if error.contains("needs an imported Riot token or a captured launcher session") {
                "Account needs a captured launcher session or imported Riot token".to_string()
            } else {
                error
            }
        })?;
    let region = resolved
        .region
        .ok_or_else(|| "Could not resolve Riot player preferences region".to_string())?;

    Ok(SettingsContext {
        session: resolved.session,
        launcher_session: resolved.launcher_session,
        identity: resolved.identity,
        preference_base_url: player_preferences_base_url_for_region(region).to_string(),
    })
}
```

Remove the imports this leaves unused (`active_api_session`, `api_identity`,
`resolve_session_region` in `game_settings.rs`); the compiler lists them.

`ProfileIdentityLoaded` in `src/ui/app.rs`, after `account.session = Some(identity.session);`:

```rust
                            // Refresh re-reads who the account is; its region is looked up again
                            // through Riot Geo on the next request, in case it moved.
                            account.region = None;
```

The three existing `api_identity(..)` calls in `src/ui/tests.rs` gain `None` as the third
argument.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test --features settings-cloning`, `cargo test`, both clippy commands.
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/account.rs src/ui/data.rs src/ui/data/session.rs src/ui/data/game_settings.rs src/ui/app.rs src/ui/tests.rs
git commit -m "Take identity from the token and save each account's region"
```

---

### Task 4: Open the settings dialog at once

**Files:**
- Modify: `src/ui/mod.rs` (`PendingSettingsChange`, remove `settings_preflight`, the
  `SettingsPreflightChecked` message, the loading indicator line)
- Modify: `src/ui/app.rs` (`start_settings_preflight` → `open_settings_change`,
  `SettingsPreflightChecked` handler, `settings_work_in_progress`, `settings_change_warning`)
- Modify: `src/ui/shell.rs` (`settings_change_prompt_overlay`)
- Test: `src/ui/tests.rs` (the settings dialog tests from `settings_checked` down to
  `confirming_apply_starts_it`, plus `restoring_asks_first_then_starts`,
  `restore_waits_for_other_settings_work`, `settings_cloning_does_nothing_while_disabled` and
  `escape_closes_the_preset_dialogs`)

**Interfaces:**
- Consumes: `check_settings_activity` (Task 2); `RefreshedApiContext` and
  `PrimeApp::cache_refreshed_api_context(&mut self, RefreshedApiContext) -> bool` (existing).
- Produces: `PendingSettingsChange { change, warning: Option<String>, checking: bool }`;
  `Message::SettingsPreflightChecked(AccountId, Option<(AccountActivityCheck, Option<RefreshedApiContext>)>, bool)`.

- [ ] **Step 1: Rewrite the tests**

Replace the `settings_checked` helper and the tests that use it with:

```rust
fn settings_checked(
    account_id: AccountId,
    availability: AccountAvailability,
    valorant_running: bool,
) -> Message {
    Message::SettingsPreflightChecked(
        account_id,
        Some((
            super::data::account_details::AccountActivityCheck {
                account_id,
                availability,
            },
            None,
        )),
        valorant_running,
    )
}

fn with_fresh_availability(app: &mut PrimeApp, account_id: AccountId, availability: AccountAvailability) {
    app.account_availability_loaded_at = Some(iced::time::Instant::now());
    app.account_availability.insert(account_id, availability);
}

fn pending_warning(app: &PrimeApp) -> Option<&str> {
    app.confirm_settings_change
        .as_ref()
        .and_then(|pending| pending.warning.as_deref())
}

#[test]
fn a_fresh_result_opens_the_dialog_without_checking() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let preset =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    app.settings_profiles = vec![preset.clone()];
    with_fresh_availability(&mut app, account_id, AccountAvailability::Available);

    let _ = app.update(Message::RequestApplyPreset {
        profile_id: preset.id.clone(),
        account_id,
    });

    assert_eq!(
        app.confirm_settings_change,
        Some(PendingSettingsChange {
            change: SettingsChange::Apply {
                account_id,
                profile_id: preset.id,
            },
            warning: None,
            checking: false,
        })
    );
}

#[test]
fn a_fresh_result_in_a_match_warns_at_once() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    with_fresh_availability(
        &mut app,
        account_id,
        AccountAvailability::Unavailable {
            reason: "in match".to_string(),
        },
    );

    let _ = app.update(Message::RequestRestoreSettings(account_id));

    let warning = pending_warning(&app).expect("warning");
    assert!(warning.contains("in match"), "{warning}");
}

#[test]
fn an_old_result_opens_the_dialog_while_checking() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());

    let task = app.update(Message::RequestRestoreSettings(account_id));

    assert!(task.units() > 0);
    assert_eq!(
        app.confirm_settings_change,
        Some(PendingSettingsChange {
            change: SettingsChange::Restore(account_id),
            warning: None,
            checking: true,
        })
    );

    let _ = app.update(settings_checked(
        account_id,
        AccountAvailability::Unavailable {
            reason: "in lobby".to_string(),
        },
        false,
    ));

    let pending = app.confirm_settings_change.as_ref().expect("still open");
    assert!(!pending.checking);
    assert!(pending_warning(&app).expect("warning").contains("in lobby"));
}

#[test]
fn a_failed_check_leaves_a_note_and_stops_checking() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));

    let _ = app.update(settings_checked(
        account_id,
        AccountAvailability::activity_check_failed(),
        false,
    ));

    assert!(!app.confirm_settings_change.as_ref().expect("open").checking);
    assert!(pending_warning(&app).expect("note").contains("couldn't check"));
}

#[test]
fn applying_warns_when_valorant_is_running() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    with_fresh_availability(&mut app, account_id, AccountAvailability::Available);
    let _ = app.update(Message::RequestRestoreSettings(account_id));

    let _ = app.update(Message::SettingsPreflightChecked(account_id, None, true));

    let warning = pending_warning(&app).expect("warning");
    assert!(warning.contains("VALORANT is running"), "{warning}");
}

#[test]
fn a_check_for_another_account_changes_nothing() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));

    let _ = app.update(settings_checked(
        AccountId::new(),
        AccountAvailability::Available,
        true,
    ));

    assert!(app.confirm_settings_change.as_ref().expect("open").checking);
    assert_eq!(pending_warning(&app), None);
}

#[test]
fn confirming_while_checking_starts_the_change_and_the_late_result_is_only_cached() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));

    let task = app.update(Message::ConfirmSettingsChange);

    assert!(task.units() > 0);
    assert_eq!(app.settings_applying_account, Some(account_id));

    let _ = app.update(settings_checked(account_id, AccountAvailability::Available, false));

    assert_eq!(app.confirm_settings_change, None);
    assert_eq!(
        app.account_availability.get(&account_id),
        Some(&AccountAvailability::Available)
    );
}

#[test]
fn the_checks_refreshed_session_is_saved() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));
    let session = AuthSession::new("fresh", None, None, "Bearer", Some(3600), 100);

    let task = app.update(Message::SettingsPreflightChecked(
        account_id,
        Some((
            super::data::account_details::AccountActivityCheck {
                account_id,
                availability: AccountAvailability::Available,
            },
            Some(super::data::account_details::RefreshedApiContext {
                account_id,
                session: session.clone(),
                launcher_session: None,
                identity: settings_api_identity(),
            }),
        )),
        false,
    ));

    assert!(task.units() > 0, "saves accounts.json");
    let account = app
        .state
        .accounts
        .iter()
        .find(|account| account.id == account_id)
        .expect("account");
    assert_eq!(account.session.as_ref(), Some(&session));
}

#[test]
fn confirming_apply_starts_it() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let preset =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    app.settings_profiles = vec![preset.clone()];
    let _ = app.update(Message::RequestApplyPreset {
        profile_id: preset.id,
        account_id,
    });

    let task = app.update(Message::ConfirmSettingsChange);

    assert!(task.units() > 0);
    assert_eq!(app.confirm_settings_change, None);
    assert_eq!(app.settings_applying_account, Some(account_id));
    assert!(app.status.contains("Main settings"), "{}", app.status);
}
```

Delete `other_settings_work_waits_for_the_game_check`: the dialog is modal now, so nothing else
can start while it's open. In `restoring_asks_first_then_starts`, replace the preflight
assertions with:

```rust
    let task = app.update(Message::RequestRestoreSettings(account_id));

    assert!(task.units() > 0);
    assert_eq!(
        app.confirm_settings_change,
        Some(PendingSettingsChange {
            change: SettingsChange::Restore(account_id),
            warning: None,
            checking: true,
        })
    );
    assert_eq!(app.settings_applying_account, None);

    let task = app.update(Message::ConfirmSettingsChange);
```

In `restore_waits_for_other_settings_work` and `settings_cloning_does_nothing_while_disabled`,
replace `app.settings_preflight` with `app.confirm_settings_change`. In
`escape_closes_the_preset_dialogs`, drop the `settings_checked` line after
`RequestRestoreSettings` (the dialog is open straight away).

Check `settings_api_identity()`'s PUUID matches the account `settings_app` creates (LSP
`goToDefinition`); `the_checks_refreshed_session_is_saved` relies on it.

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test --features settings-cloning --lib`
Expected: compile errors for `checking` and the new message shape. Add `checking: bool` to
`PendingSettingsChange` and change `SettingsPreflightChecked` to
`SettingsPreflightChecked(AccountId, Option<(AccountActivityCheck, Option<RefreshedApiContext>)>, bool)`
in `src/ui/mod.rs`, with a handler that does nothing (`=> Task::none()`) and `checking: false`
where the current code builds `PendingSettingsChange`. Run again.
Expected: the new tests fail on their assertions (for example
`an_old_result_opens_the_dialog_while_checking` finds no pending change).

- [ ] **Step 3: Implement**

`src/ui/mod.rs`:
- `PendingSettingsChange` gains `/// Whether the game check is still running.` `checking: bool`.
- Remove the `settings_preflight` field and its doc comment, and the
  `|| app.settings_preflight.is_some()` line in `loading_indicator_active`.
- Import `RefreshedApiContext` from `data::account_details` for the message.

`src/ui/app.rs`:
- Remove `settings_preflight: None,` from the constructor.
- `settings_work_in_progress` drops `|| self.settings_preflight.is_some()` and its doc comment
  mentions only saving and applying.
- Replace `start_settings_preflight` with:

```rust
    /// Opens the confirmation for an Apply or Restore straight away. A background activity
    /// result under 90 seconds old supplies the warning; otherwise the dialog shows that it's
    /// checking while `check_settings_activity` runs. Whether VALORANT is running on this PC is
    /// checked every time, locally.
    fn open_settings_change(&mut self, change: SettingsChange) -> Task<Message> {
        if !self.settings_cloning
            || self.settings_work_in_progress()
            || self.update_blocks_new_work()
        {
            return Task::none();
        }

        let Some(account) = self
            .state
            .accounts
            .iter()
            .find(|account| account.id == change.account_id())
            .cloned()
        else {
            self.set_status("Account profile no longer exists");
            return Task::none();
        };

        let fresh = self.fresh_availability(account.id).cloned();
        let warning = fresh
            .as_ref()
            .and_then(|availability| {
                settings_change_warning(&account.display_name, availability, false)
            });
        self.close_account_surfaces();
        self.confirm_settings_change = Some(PendingSettingsChange {
            change,
            warning,
            checking: fresh.is_none(),
        });

        let account_id = account.id;
        let client_version = self.client_version_input.clone();
        Task::perform(
            async move {
                let check = match fresh {
                    Some(_) => None,
                    None => Some(check_settings_activity(account, client_version).await),
                };
                (check, valorant_is_running().await)
            },
            move |(check, valorant_running)| {
                Message::SettingsPreflightChecked(account_id, check, valorant_running)
            },
        )
    }

    /// The background poll's result for this account, when it's recent enough to trust.
    fn fresh_availability(&self, account_id: AccountId) -> Option<&AccountAvailability> {
        const FRESH_FOR: std::time::Duration = std::time::Duration::from_secs(90);

        self.account_availability_loaded_at
            .filter(|loaded_at| loaded_at.elapsed() < FRESH_FOR)?;
        self.account_availability
            .get(&account_id)
            .filter(|availability| !matches!(availability, AccountAvailability::Unknown { .. }))
    }
```

Update the two callers (`RequestApplyPreset`, `RequestRestoreSettings`) from
`start_settings_preflight` to `open_settings_change`.

- Replace the `SettingsPreflightChecked` handler:

```rust
            Message::SettingsPreflightChecked(account_id, check, valorant_running) => {
                let mut task = Task::none();
                if let Some((check, refreshed)) = check {
                    self.account_availability
                        .insert(check.account_id, check.availability);
                    if let Some(refreshed) = refreshed
                        && self.cache_refreshed_api_context(refreshed)
                    {
                        task = self.save_task();
                    }
                }

                let Some(display_name) = self
                    .state
                    .accounts
                    .iter()
                    .find(|account| account.id == account_id)
                    .map(|account| account.display_name.clone())
                else {
                    return task;
                };
                let availability = self
                    .account_availability
                    .get(&account_id)
                    .cloned()
                    .unwrap_or_else(AccountAvailability::activity_check_failed);
                if let Some(pending) = self
                    .confirm_settings_change
                    .as_mut()
                    .filter(|pending| pending.change.account_id() == account_id)
                {
                    pending.warning =
                        settings_change_warning(&display_name, &availability, valorant_running);
                    pending.checking = false;
                }

                task
            }
```

- `settings_change_warning`: add, before `if valorant_running`:

```rust
    if matches!(availability, AccountAvailability::Unknown { .. }) {
        warnings.push(format!(
            "Prime couldn't check whether {display_name} is in VALORANT."
        ));
    }
```

- Import `check_settings_activity` from `super::data::account_details` where `app.rs` imports
  `fetch_account_availability`, and remove Task 2's temporary `#[allow(dead_code)]` if you added
  it.

`src/ui/shell.rs`, in `settings_change_prompt_overlay`, replace the `let (details, action) = match &pending.warning` block with:

```rust
    let mut details = details;
    if pending.checking {
        details.push_str(&format!("\n\nChecking whether {name} is in VALORANT…"));
    }
    let action = match &pending.warning {
        Some(warning) => {
            details.push_str(&format!("\n\n{warning}"));
            format!("{action} anyway")
        }
        None => action.to_string(),
    };
```

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test --features settings-cloning`, `cargo test`, both clippy commands.
Expected: all pass.

- [ ] **Step 5: Break it on purpose once**

Change `fresh_availability`'s `FRESH_FOR` to `from_secs(0)` and run
`cargo test --features settings-cloning --lib ui::tests`. Expected:
`a_fresh_result_opens_the_dialog_without_checking` and `a_fresh_result_in_a_match_warns_at_once`
fail. Put `90` back.

- [ ] **Step 6: Commit**

```bash
git add src/ui/mod.rs src/ui/app.rs src/ui/shell.rs src/ui/tests.rs src/ui/data/account_details.rs
git commit -m "Open the settings dialog at once and reuse recent activity checks"
```

---

### Task 5: Docs

**Files:**
- Modify: `AGENTS.md` (the Riot Geo note)
- Modify: `docs/superpowers/specs/2026-09-27-fewer-riot-requests-design.md` (region recovery)

- [ ] **Step 1: Update the AGENTS.md note**

Replace:

```markdown
- Resolve the shard through Riot Geo when an ID token is available. A stale shard gives storefront 404s.
```

with:

```markdown
- Each account's region is saved after the first Riot Geo lookup and reused; the shard comes from
  it. Refreshing an account forgets the region so the next request looks it up again. A stale
  region gives storefront 404s.
- Identity comes from the access token's subject; `userinfo` is only called by account refresh
  and capture, or when a token has no subject.
```

- [ ] **Step 2: Update the spec**

In the spec's "3. Skip userinfo for identity and save the region" section, replace the
"Recovery from a stale region" bullet with:

```markdown
- Recovery from a stale region: the account's Refresh action forgets the saved region, and the
  next request looks it up again through Riot Geo. No automatic retry: region changes are rare
  (agreed in chat).
```

In section "5. One shared HTTP client", replace the two bullets with:

```markdown
- `RiotApi::shared()` creates one client on first use and hands out clones, in place of the
  eleven `RiotApi::new()` calls in `src/ui`. `reqwest::Client` is reference-counted, so clones
  share one connection pool. If creating it fails, every feature that needs Riot reports the same
  error, as it does now when `RiotApi::new()` fails.
```

In section "1. Reuse the background activity result", replace the first bullet with:

```markdown
- A result counts only if the background poll's last run (`account_availability_loaded_at`) is
  recent and the result isn't "couldn't check".
```

In "Testing", delete the "404 recovery" line, and change the header's `Status:` line to
`Status: approved`.

- [ ] **Step 3: Commit**

AGENTS.md also holds the earlier "How to work here" note, which the user asked to keep out of
commits. Commit only the spec, and leave the AGENTS.md changes uncommitted for the user to take.

```bash
git add docs/superpowers/specs/2026-09-27-fewer-riot-requests-design.md docs/superpowers/plans/2026-09-27-fewer-riot-requests.md
git commit -m "Update the fewer-requests spec for the agreed simplifications"
```
