# Prime Valorant Manager

Rust + Iced desktop account manager for VALORANT.

Current scope:

- Manage local account profiles with region/shard metadata.
- Launch VALORANT through `RiotClientServices.exe --launch-product=valorant --launch-patchline=live`.
- Import Riot web redirect tokens for API access.
- Query the unofficial player store and player loadout endpoints when a valid token, entitlement token, PUUID, shard, and client version are available.

Notes:

- Riot's direct username/password auth is intentionally isolated because the currently documented flow is prone to captcha and anti-bot breakage.
- Store and loadout requests use the undocumented client endpoints described by <https://valapidocs.techchrism.me/>.
- This app should not store Riot passwords. The current token import flow stores only session tokens in the local profile JSON and redacts them from debug output.

Run:

```powershell
cargo run
```

Test:

```powershell
cargo test
```
