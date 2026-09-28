# Fewer Riot requests for settings Apply and Restore

Date: 2026-09-27
Status: approved

## Why

Clicking an account in a preset's "Apply to..." list takes seconds before the confirmation dialog
opens, and an Apply can send up to 12 Riot requests with two refresh-token sign-ins. The Accounts
tab already polls every account's activity every 60 seconds, so most of that work repeats
something Prime learned under a minute earlier. Fewer requests also lowers the risk of Riot rate
limiting.

## Goals

- The Apply and Restore confirmation dialog opens immediately when a fresh background result
  exists, and otherwise after a visible check of about a second.
- A typical Apply (Accounts tab open, saved token valid) sends only the settings read and save:
  2 requests.
- The worst case (token expired, region unknown) sends one sign-in, not two.
- No feature loses correctness: sessions are still checked against the account's PUUID before use,
  and refreshing an account corrects a stale saved region.

## Non-goals

- Changing what the Accounts tab shows (in match, agent select, in lobby, available).
- Changing Launch's warnings. Launch benefits from the parallel requests and the shared client, but
  its behaviour stays the same.
- Skipping `userinfo` where it supplies more than identity: profile refresh (Riot ID) and account
  capture keep calling it.

## Current request chain

Game check before the dialog (`check_account_in_game` → `check_account_availability`):

1. `POST auth.riotgames.com/token` when the saved access token has expired.
2. `GET auth.riotgames.com/userinfo`, always.
3. `POST entitlements.auth.riotgames.com/api/token/v1` when the session has no entitlements token.
4. `PUT riot-geo.pas.si.riotgames.com/pas/v1/product/valorant` when userinfo has no region.
5. to 7. core-game, then pregame, then party, one after another; an idle account needs all three.

Any session the check obtains is thrown away (`fetch_account_availability` drops it).

Apply after confirming (`resolve_settings_context`): sign-in again when expired, `userinfo`, Riot
Geo when needed, then `getPreference` and `savePreference`. Restore is the same.

Best case 7 requests; worst case 12 with two sign-ins.

## Design

### 1. Reuse the background activity result

- A result counts only if the background poll's last run (`account_availability_loaded_at`) is
  recent and the result isn't "couldn't check".
- A result is fresh when it is under 90 seconds old (the poll runs every 60 seconds).
- Requesting Apply or Restore checks first, then opens the confirmation dialog with its warning
  complete:
  - While the checks run, the control that was pressed shows a loading state ("Checking
    <account>..." on the preset card for Apply, on the account's Restore row for Restore), and other
    settings actions wait. Escape, or anything else that closes the account dialogs, cancels it;
    a late result then opens nothing.
  - VALORANT running on this PC is checked locally every time (no request).
  - Fresh result: the dialog's warning comes from it and no Riot request is sent, so the dialog
    opens near-instantly, after the local check.
  - Stale or missing result: the Riot check runs, about a second, and the dialog opens with its
    result. A result for another account or an earlier request opens nothing.
  - The check's refreshed session is saved before the dialog opens, so the Apply or Restore
    reuses it. This avoids two sign-ins with the same saved login at once, which a Confirm during
    the check would otherwise start.
- `settings_preflight` goes away. `settings_check` holds the change and the request ID of its
  check; the pending change in the dialog carries `warning` and `check_failed`.

### 2. Keep sessions the check obtains

- The settings game check returns the `RefreshedApiContext` it obtained, and the app saves it with
  `cache_refreshed_api_context`, as the background poll already does.
- Apply and Restore then usually find a valid saved access token and skip the sign-in.

### 3. Skip userinfo for identity and save the region

- `resolve_credentials` and `resolve_settings_context` stop calling `userinfo`. Identity comes from
  the access token's `sub` claim (`jwt_subject`), checked against the account's PUUID with
  `check_puuid` as `active_api_session` already does. A token without a readable subject falls back
  to `userinfo`.
- `AccountProfile` gains `region: Option<ValorantRegion>`, saved in `accounts.json`
  (`#[serde(default)]`, so existing files load). `ValorantRegion` gains serde support.
- Region resolution: use the saved region; with none, call Riot Geo and save the result through
  `ApiIdentity` and `cache_account_api_context`, as the shard is saved today.
- The shard comes from the region, as it does after a Geo lookup today.
- Recovery from a stale region: the account's Refresh action forgets the saved region, and the
  next request looks it up again through Riot Geo. No automatic retry: region changes are rare
  (agreed in chat).
- Until the account is refreshed, activity requests read a 404 as "not in the game", and other
  regional requests return 404 errors. The accepted risk: a wrong region makes the account look
  available, so the settings warning could be missed until the account is refreshed.
- AGENTS.md's Riot API note says the region is saved per account after the first Riot Geo lookup,
  and that refreshing an account forgets it.

### 4. Fewer and parallel activity requests

- The settings check sends core-game and party at the same time: 2 requests, one wait. In a match
  or in a party means in the game. Agent select is covered on the assumption that the player is still
  in a party then (not verified).
- The background poll and Launch keep all three requests, sent at the same time, because the
  Accounts tab and Launch's warning name the exact state. The existing precedence (match, then agent
  select, then lobby) stays.

### 5. One shared HTTP client

- `RiotApi::shared()` creates one client on first use and hands out clones, in place of the
  eleven `RiotApi::new()` calls in `src/ui`. `reqwest::Client` is reference-counted, so clones
  share one connection pool. If creating it fails, every feature that needs Riot reports the same
  error, as it does now when `RiotApi::new()` fails.

## New request chain

Game check before the dialog: none when the background result is fresh. Otherwise a sign-in and
entitlements only when the token has expired, Riot Geo only when no region is saved, then core-game
and party in parallel.

Apply after confirming: a sign-in only when the token has expired (rare after the check or poll),
Riot Geo only when no region is saved, then `getPreference` and `savePreference`.

| Situation | Before | After |
|---|---|---|
| Accounts tab open, token valid | 7 | 2 |
| Accounts tab not open recently, token valid | 7 | 4 |
| Token expired, region unknown | 12, two sign-ins | 7, one sign-in |

Background poll per account per minute: 4 to 7 requests before, 3 after in most minutes.

## Error handling

- A failed settings check leaves the dialog without a warning and with a muted note that Prime
  couldn't check, so Apply stays possible, matching Launch's "inconclusive" case.
- A session whose `sub` doesn't match the account is never used or saved (existing rule).
- Riot Geo failing with no saved region fails the request as today.

## Testing

Tests follow `src/ui/tests.rs` conventions: assert on state and `task.units()`, never run tasks.

- Fresh result: requesting Apply starts no Riot check; the dialog opens with the stored warning
  after the local check.
- Stale result: requesting shows the loading state and no dialog; the check result opens the
  dialog with its warning; a result for another account or an earlier request opens nothing;
  other settings work waits; Escape cancels the check.
- The check's refreshed session is saved to the account, even after the check was canceled.
- Identity from the token's subject: a matching subject is accepted without `userinfo`; a
  mismatched subject is refused.
- Region: saved after the first lookup; a saved region is reused; `accounts.json` without the field
  still loads.
- Activity classification with parallel results keeps today's precedence.
