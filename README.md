# Prime Valorant Manager

Rust + Iced desktop account manager for VALORANT.

Current scope:

- Manage local account profiles with region/shard metadata.
- Launch VALORANT through `RiotClientServices.exe --launch-product=valorant --launch-patchline=live`.
- Capture and restore per-account Riot Client launcher sessions from the local Riot Client `Data` folder after a remembered login.
- Import Riot web redirect tokens for API access.
- Query the unofficial player store and player loadout endpoints when a valid token, entitlement token, PUUID, shard, and client version are available.

Notes:

- Riot's direct username/password auth is intentionally isolated because the currently documented flow is prone to captcha and anti-bot breakage.
- Store and loadout requests use the undocumented client endpoints described by <https://valapidocs.techchrism.me/>.
- Launcher switching follows the same broad approach as Assist: preserve Riot Client remembered-login data per account, restore the selected account's data, then launch VALORANT through Riot Client.
- This app should not store Riot passwords. The current token import flow stores only session tokens in the local profile JSON and redacts them from debug output.

Run:

```powershell
cargo run
```

Test:

```powershell
cargo test
```
