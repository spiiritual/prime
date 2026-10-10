# prime

Windows desktop account manager for VALORANT, written in Rust (edition 2024) with Iced 0.14. The Cargo
package and the app are both named `prime`.

## Commands

```powershell
cargo run                                  # run the app
cargo test                                 # run the tests
cargo clippy --all-targets                 # lint; keep it clean
gh workflow run release.yml -f bump=patch   # release from GitHub Actions (see below)
.\scripts\release.ps1 -UseGeneratedNotes -Publish   # the same release from this PC
```

GitHub Actions: `ci.yml` runs clippy (warnings denied) and the tests on pushes to master and on PRs.
`release.yml` (run by hand; inputs `bump`, `version`, `notes`) downloads the latest release so Velopack
can build a delta, then runs `scripts/release.ps1 -Publish`, which pushes the release commit and tag to
master. Pull afterwards. Dependabot opens one grouped Cargo PR weekly and Actions bumps monthly.

## What the app does

Accounts tab:

- Keeps account profiles (display name, Riot ID, PUUID, region and shard) in `accounts.json`.
- Adds accounts by capturing Riot Client's remembered login. "Add new account" clears the live login,
  waits for the sign-in, then asks for a display name. "Add current account" captures whoever is signed in
  now. "Re-capture login" replaces a saved account's captured login.
- Launches VALORANT for the selected account: closes Riot Client and VALORANT, restores that account's
  saved Riot Client `Data`, then runs `RiotClientServices.exe --launch-product=valorant --launch-patchline=live`.
  Launch needs a captured login, and warns first when the game is running or the account is in a match,
  agent select or a lobby.
- Shows each account's rank, level and penalties, and refreshes its PUUID, Riot ID and shard on request.
- Exports and imports an account, with its captured login, as text. Exports are copied to the clipboard
  without entering Windows clipboard history.
- Settings profiles sub-tab: save an account's VALORANT settings as a named preset, apply a preset to
  any account and restore that account's own settings afterwards. Adding an account can also save its
  settings as a preset.

Shop tab: featured bundles (each with its own countdown), daily offers, Night Market and accessories, with
art, rarity colours and discounts. The wallet balance shows in the header.

Loadout tab: equipped gun skins in the in-game collection order, and a Battle Pass sub-tab sized to
the window: the tier reached, the pass's completion and a rail of chapters on top, the open chapter's
rewards (the current one first; others' art loads when opened), and the next weapon skins ahead.

Aim Trainer tab: WAIUA-style red dots that grow until they burst (health 100, a burst costs 15, a
miss 5). The crosshair turns `0.07° × sens` per raw mouse count, read with Windows Raw Input
(`src/raw_mouse.rs`) because iced drops winit's raw device events, so Windows pointer speed and
acceleration don't matter. Dots sit where they would at VALORANT's 103° FOV on this monitor. During
a run the cursor is hidden and held on one pixel; Esc, leaving the tab or game over end the run,
focus loss pauses it. Sensitivity, DPI and one best run for the whole app are saved in
`accounts.json` (`aim_trainer`).

Live Match: while the selected account is in agent select or a match, a sidebar indicator shows the
map and score and opens the Live Match page (it has no nav item and no Launch button). The page
shows the map, mode, round, server and both teams with names, agents, levels, ranks and the skins
of the 4 weapons picked per column in Settings (Vandal, Phantom, Sheriff and Operator by default).
It polls on the one-minute availability timer only while open and the window is visible, and
each poll is also the account's availability check. Players who
hide their name in game show as hidden, since Riot's name service returns no name for them while
the match runs; the user's own saved accounts are never hidden and fall back to their saved Riot ID.
A "Show hidden details" switch beside the page title (off at every start) shows the levels players
hide (`HideAccountLevel`) and the names of players in streamer mode (`Incognito`). The names come
from the Riot Client running on this PC, signed in as any account; they're looked up right away and
on each poll, and without the Riot Client they show as unavailable, with the reason on hover.

Invisible status: every launch goes through a chat proxy (`src/riot/chat_proxy/`), the way Deceive
does. While VALORANT runs on this PC, Riot Client here is signed in as the selected account and its
chat goes through that proxy, Prime shows a sidebar control above the version label with the status
friends see: Online, Mobile or Invisible. A change applies at once and is saved (`presence_status` in
`accounts.json`, written only when not Online), so the next launch starts with it. If the proxy
can't start and the saved status is Invisible or Mobile, the launch asks before going online; with Online it
launches anyway and warns that the status can't be changed this session. Prime checks the game every
5 seconds while a proxy runs and stops the proxy when Riot Client closes. Quitting Prime drops Riot
Client's chat until Riot Client restarts, so while chat goes through the proxy, quitting (from the
tray too) and restarting for an update ask first.

Settings tab: Riot Client path, "keep in the system tray when closed" (on by default; while minimized or in the tray it
polls every 30 minutes so sessions keep refreshing; the tray menu quits), Live Match's skin columns, client version
(fetched at startup), image cache size and clearing, app updates, and a Riot redirect-token import as an
advanced fallback for API access.

One instance: starting Prime while it runs, even from the tray, brings the running window forward
and the new start exits (`single_instance.rs`). Debug builds use their own name, so `cargo run`
works beside an installed Prime.

Updates: Velopack checks the GitHub releases of `spiiritual/prime`. `PRIME_UPDATE_SOURCE` and
`PRIME_UPDATE_CHANNEL` override the source and channel.

## Feature flags

Off by default, so release builds leave it out.

- `image-viewer-testing`: click an image to open it full size.

## Code layout

Dependencies run one way: `src/riot` → `src/ui/data` → `src/ui/app.rs` → views.

- `src/riot/`: Riot HTTP client, endpoints, response models, the public content API (`content.rs`),
  redirect-token parsing (`auth.rs`), launcher session capture and restore (`launcher_session/`), and
  the chat proxy for the invisible status (`chat_proxy/`).
- `src/ui/data/`: async loaders and view models for Shop, Loadout, Battle Pass, account details, the
  launch flow, images and settings profiles.
- `src/ui/app.rs`: how `PrimeApp` handles each `Message`. `src/ui/mod.rs` holds the state, `Message` and
  the subscription.
- Views: `src/ui/screens/*` (one per tab), `src/ui/shell.rs` (header, status bar, dialogs) and
  `src/ui/components.rs` (shared widgets).
- Outside the UI: `account.rs`, `storage.rs`, `launch.rs` (Riot Client processes), `account_transfer.rs`,
  `game_settings.rs`, `image_cache.rs`, `updater.rs`, `secret_clipboard.rs`, `aim_trainer.rs` and
  `raw_mouse.rs`.

Local data: `%APPDATA%\spiiritual\prime\config\` holds `accounts.json`, `launcher-backups\` and
`settings-profiles\`. Downloaded images go in `%LOCALAPPDATA%\spiiritual\prime\cache\images\`.
The chat proxy's certificate is cached in `%LOCALAPPDATA%\spiiritual\prime\cache\chat-proxy-localhost.pfx`.
`accounts.json` rejects unknown fields, so a build older than a setting it holds can't load it.
Settings left at their default aren't written, which keeps that rare.

## Rules

- Never store Riot passwords. There is no username/password sign-in, because Riot's direct auth flow
  breaks on captcha and anti-bot checks. Only session tokens are stored, and `Debug` output redacts them.
- Before using or saving a session for an account, check that it belongs to that account's PUUID.
- Launch belongs on the Accounts tab only. Don't add a launch button to every tab header.
- Shop and Loadout load when their tab opens. Loadout has no refresh button or account level indicator.
- Keep direct dependencies current with crates.io when touching dependency metadata, then let Cargo
  update the lockfile.
- The chat proxy trusts `deceive-localhost.molenzwiebel.xyz` to resolve to 127.0.0.1 and uses the
  certificate Deceive publishes, private key included. Check the name resolves only to loopback
  before each launch, bind proxy listeners to 127.0.0.1 only, and never send that certificate
  anywhere.

## Riot API notes

- Store, wallet, loadout, MMR, penalties, contracts, match, pregame and party requests use the
  undocumented client endpoints described at <https://valapidocs.techchrism.me/>.
- Launcher switching follows the same approach as Assist: keep Riot Client's remembered-login data per
  account and restore it before launching.
- Requests re-authenticate from the captured launcher session when possible. Refreshed sessions and
  entitlement tokens are saved back to the account.
- Signing the account out anywhere revokes its saved refresh token (`invalid_grant`), and only a
  re-capture fixes it. Signing in or launching the game elsewhere does not revoke it.
- Each account's region is saved after the first Riot Geo lookup and reused; the shard comes from
  it. Refreshing an account forgets the region so the next request looks it up again. A stale
  region gives storefront 404s.
- Identity comes from the access token's subject; `userinfo` is only called by account refresh
  and capture, when a token has no subject, or when no region is saved and there is no ID token for
  Riot Geo.
- The client version comes from the public Valorant version endpoint. Shop and Loadout need it.
- Skin, bundle, currency and weapon names come from the public content API at valorant-api.com.
- Featured bundles are told apart by store bundle ID, even when two resolve to the same content bundle.
- Account level comes from the account XP endpoint when the loadout reports zero.
- Newer weapons need explicit categories: Bandit is a sidearm and Outlaw is a sniper rifle.
- Live Match uses the core-game and pre-game player, match and loadout endpoints, one name service
  batch per match, and one MMR request per player per match (3 at a time). Names are kept in
  memory only. Map, queue and agent names come from valorant-api.com.
- No Riot server reports a match's score or round. They come only from the local Riot Client's chat
  presence (`https://127.0.0.1:{port}/chat/v4/presences`, port and password from its lockfile,
  read per request and never saved). Without it the page says the score is unavailable.
- The server label is the city in the match's `GamePodID`, such as "Ashburn".
- During a match the name service returns blank names for players with `Incognito` (streamer
  mode; `HideAccountLevel` is a separate setting). The local Riot Client's
  `POST /player-account/lookup/v2/namesets-for-puuids` (`{"puuids": [...]}`, v1 is gone) ignores
  streamer mode, so with "Show hidden details" on Live Match names them from it.
- The chat proxy passes `--client-config-url=http://127.0.0.1:{port}` (no quotes) and rewrites
  `chat.host`, `chat.port` and `chat.affinities` in Riot's client config. The real server is the
  player's affinity host from `riot-geo.pas.si.riotgames.com/pas/v1/service/chat`, unless
  `chat.affinity.enabled` is false.
- Who the local Riot Client is signed in as comes from its `GET /entitlements/v1/token` subject.
  Only the subject is kept.

## Tests

- UI behaviour tests live in `src/ui/tests.rs`. Other modules have their own unit tests.
- `app.update` returns lazy `Task`s. Tests assert on state and `task.units()` and never run a task.
- Build apps with `test_app(tempdir)`, and point `app.image_cache` at a tempdir when a test could touch
  it, so tests never touch the real `accounts.json`, Riot Client data or image cache.

## Trying the app by hand

`cargo run` uses the real `accounts.json` and the real Riot Client data. Ask before doing any of these:

- Launch VALORANT, Add new account, Add current account and Re-capture login close Riot Client and
  VALORANT and replace the live login.
- Apply settings profile writes VALORANT settings to a Riot account.
- Delete account, Delete image cache and Download and restart can't be undone.
