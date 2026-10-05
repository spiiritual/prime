# Invisible Status Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** While VALORANT runs on this PC for the selected account, a sidebar control lets the user
show as Online, Mobile or Invisible to friends, applied right away and remembered for the next
launch.

**Architecture:** Every Prime launch starts a chat proxy first, the way Deceive does. A local HTTP
proxy for Riot's client config points Riot Client's chat at `deceive-localhost.molenzwiebel.xyz`,
which resolves to 127.0.0.1. A local TLS relay there forwards chat to Riot's real server and
rewrites the presence Riot Client sends. A `tokio::sync::watch` channel carries the chosen status
into each open connection, which re-sends its last presence when the status changes. The UI keeps
the running proxy as `PrimeApp::chat_proxy`. Every 5 seconds it checks whether VALORANT is running
here and which PUUID the local Riot Client is signed in as. It shows the control only when Riot
Client's chat goes through Prime's proxy for the selected account.

**Tech Stack:** Rust 2024, Iced 0.14, tokio (net, io-util, macros, rt, sync, time), native-tls and
tokio-native-tls (the chat TLS server and client), reqwest (client config, certificate download,
local Riot Client API), serde.

**Spec:** There is no separate spec document. The decisions below were agreed in chat on
2026-10-04, and the design is in `~/.pencil/documents/2517e1ae-923d-4a91-a3cb-f6528b4323a8/pencil-new.pen`:
`Accounts — In a Match · Invisible` (q10Rjx), `Shop — In a Match · Status Menu` (DBrZG),
`Shop — In a Match · Invisible` (c3nSQH), the tight-gap variant (Xq33U) and
`Dialog — Can't Go Invisible` (npug6).

**Decisions (agreed in chat):**
- The statuses are **Online**, **Mobile** and **Invisible**. ("Appear offline" was renamed
  Invisible.)
- The control lives at the bottom of the sidebar, between the Live Match indicator and the version
  label. It shows **only** while VALORANT is running on this PC, Riot Client here is signed in as
  the selected account, and that client's chat goes through Prime's proxy. At most one VALORANT
  runs at a time. Launches outside Prime don't need handling beyond hiding the control.
- The control shows the status name alone, with no subtitle.
- The menu opens upward, 4px above the control. It has the three options, each with a description,
  a check on the current one, an 8px gap above the divider, and a full-width note with no icon:
  "Applies right away. Your next launch starts the same way."
- A change applies immediately. The choice is saved and the next launch starts with it.
- If the proxy can't start and the saved status is Invisible, the launch stops at the "Can't go
  invisible" dialog (Cancel / Launch online). If the saved status is Online, the launch goes ahead
  without the proxy and a warning toast says the status can't be changed this session.
- The review findings on the unwired proxy are fixed while wiring it (Tasks 1–6).

- The Live Match indicator and the status control sit 8px apart, as in the tight-gap variant
  (Xq33U), not at the sidebar's usual 28.

## Global Constraints

- Follow AGENTS.md. UI tests assert on state and `task.units()` and never run a task. Build apps
  with `test_app(tempdir)`.
- No test may start a real proxy, read the real lockfile, or reach the network. Tests use
  `ChatProxy::detached` and the pure functions.
- Proxy listeners bind `127.0.0.1` only.
- Never log, store or `Debug`-print tokens. The local entitlements response is parsed only for
  `subject` and then dropped. The lockfile is read per request and never saved.
- Dependencies, exactly: `native-tls = "0.2.18"`, `tokio-native-tls = "0.3.1"`,
  `tokio = { version = "1.53.2", features = ["io-util", "macros", "net", "rt", "sync", "time"] }`.
  Remove `p12` and `x509-parser`. Keep `[dependencies]` in alphabetical order. Let Cargo update the
  lockfile.
- Format with `rustfmt --edition 2024 <files you changed>`. Don't run `cargo fmt` on the whole
  crate: `src/single_instance.rs` has an unrelated pre-existing format difference.
- Run `cargo test` and `cargo clippy --all-targets` before each commit. Tasks 1–5 add functions that
  only tests use until Task 6 wires them, so clippy may report `dead_code` for those, and nothing
  else, until then. Task 6 and every later task must end with zero warnings.
- Copy UI text verbatim from this plan, including the curly apostrophes in "Can’t" where shown.
- Design measurements for the control and menu are in Task 10. Colours come from `theme`.

## Review Focus

- **A presence stanza cut across two socket reads, or a multi-byte character cut at a read
  boundary.** Expected: the real presence never reaches Riot, and no byte outside a presence
  changes. Tests: Task 2 Step 1 and Task 4 Step 1.
- **Changing status mid-match while Riot Client sends nothing.** Expected: friends see the new
  status within seconds. Test: Task 4 Step 1 checks that the last presence is re-sent.
- **Riot's chat server drops the connection.** Expected: Riot Client sees the close and reconnects,
  rather than holding a dead connection. Test: Task 4 Step 1.
- **Another account signed in to Riot Client, or another account selected in Prime.** Expected: the
  control hides, so a status is never applied to the wrong account. Test: Task 9 Step 1.
- **The proxy fails to start while the saved status is Invisible.** Expected: Prime never launches
  online without asking. Test: Task 8 Step 1.

---

### Task 0: Commit the unwired proxy as it stands

The working tree holds the unwired proxy (`src/riot/chat_proxy.rs`, its `Cargo.toml`, `Cargo.lock`,
`src/launch.rs` and `src/riot/mod.rs` changes). Commit it on its own so the review fixes show as
diffs.

- [ ] **Step 1: Commit**

```bash
git add Cargo.toml Cargo.lock src/launch.rs src/riot/mod.rs src/riot/chat_proxy.rs
git commit -m "Add an unwired Deceive-style chat proxy"
```

---

### Task 1: Dependencies and the presence module

Turns `chat_proxy.rs` into a directory and moves presence rewriting into `presence.rs`, where
`PresenceStatus` gets the agreed names, a serde form and `Online` as the default. Online now passes
presence through untouched ("Friends see you as usual"), which removes the do-not-disturb special
case.

**Files:**
- Modify: `Cargo.toml`
- Move: `src/riot/chat_proxy.rs` → `src/riot/chat_proxy/mod.rs` (`git mv`)
- Create: `src/riot/chat_proxy/presence.rs`

**Interfaces:**
- Produces:
  - `pub enum PresenceStatus { Online, Mobile, Invisible }`, with
    `Clone, Copy, Debug, Default (= Online), Eq, PartialEq, Serialize, Deserialize` and serde
    `rename_all = "snake_case"`.
  - `PresenceStatus::ALL: [PresenceStatus; 3]`, in that order.
  - `PresenceStatus::is_online(&self) -> bool`.
  - `pub(super) fn rewrite_presence(stanza: &str, status: PresenceStatus) -> String`, for one
    complete `<presence>` element.
  - `pub(super) fn is_own_presence(stanza: &str) -> bool`, true when the open tag has no `to=`
    attribute.
  - `pub(super) fn tag_open_end(bytes: &[u8], start: usize) -> Option<usize>`, now byte-based for
    Task 2.
  - Interim only: `pub(super) fn rewrite_presence_content(content: &str, status: PresenceStatus) -> String`.
    The old pump uses it until Task 4 deletes both.

- [ ] **Step 1: Update `Cargo.toml`**

In `[dependencies]`, remove `p12 = "0.6"` and `x509-parser = "0.18"`, and set these lines (keep
alphabetical order):

```toml
native-tls = "0.2.18"
tokio = { version = "1.53.2", features = ["io-util", "macros", "net", "rt", "sync", "time"] }
tokio-native-tls = "0.3.1"
```

- [ ] **Step 2: Move the module into a directory**

```bash
mkdir src/riot/chat_proxy
git mv src/riot/chat_proxy.rs src/riot/chat_proxy/mod.rs
```

- [ ] **Step 3: Write the failing tests in `src/riot/chat_proxy/presence.rs`**

Create the file with only the tests, so they fail to compile:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn presence_stanza() -> String {
        "<presence from='user@example/RC' id='b-1'><games>\
        <league_of_legends><st>chat</st><s.t>1</s.t><p>e30=</p><m>abc</m></league_of_legends>\
        <valorant><st>chat</st><p>e30=</p></valorant>\
        <keystone><st>chat</st></keystone></games>\
        <show>chat</show><status>In Lobby</status></presence>"
            .to_string()
    }

    #[test]
    fn invisible_presence_strips_game_blocks_and_status() {
        let rewritten = rewrite_presence(&presence_stanza(), PresenceStatus::Invisible);

        assert!(rewritten.contains("<show>offline</show>"));
        assert!(rewritten.contains("from='user@example/RC'"));
        assert!(!rewritten.contains("league_of_legends"));
        assert!(!rewritten.contains("valorant"));
        assert!(!rewritten.contains("keystone"));
        assert!(!rewritten.contains("<status>"));
    }

    #[test]
    fn mobile_presence_keeps_league_without_party_details() {
        let rewritten = rewrite_presence(&presence_stanza(), PresenceStatus::Mobile);

        assert!(rewritten.contains("<show>mobile</show>"));
        assert!(rewritten.contains("league_of_legends"));
        assert!(!rewritten.contains("valorant"));
        assert!(!rewritten.contains("<p>"));
        assert!(!rewritten.contains("<m>"));
    }

    #[test]
    fn online_presence_passes_through_untouched() {
        let away = "<presence from='user@example/RC'><games>\
            <league_of_legends><st>dnd</st></league_of_legends></games>\
            <show>away</show></presence>";

        assert_eq!(
            rewrite_presence(&presence_stanza(), PresenceStatus::Online),
            presence_stanza()
        );
        assert_eq!(rewrite_presence(away, PresenceStatus::Online), away);
    }

    #[test]
    fn self_closing_presence_is_left_alone() {
        let stanza = "<presence type='unavailable'/>";

        assert_eq!(rewrite_presence(stanza, PresenceStatus::Invisible), stanza);
    }

    #[test]
    fn lobby_presence_is_not_the_accounts_own() {
        assert!(is_own_presence("<presence from='a@b/RC' id='1'><show>chat</show></presence>"));
        assert!(!is_own_presence(
            "<presence to='room@lobby.example' from='a@b/RC'><show>chat</show></presence>"
        ));
        assert!(!is_own_presence(
            "<presence\n  to='room@lobby.example'><show>chat</show></presence>"
        ));
    }

    #[test]
    fn statuses_save_by_name_and_default_to_online() {
        assert_eq!(PresenceStatus::default(), PresenceStatus::Online);
        assert_eq!(
            serde_json::to_string(&PresenceStatus::Invisible).expect("serialize"),
            "\"invisible\""
        );
        assert_eq!(
            serde_json::from_str::<PresenceStatus>("\"mobile\"").expect("deserialize"),
            PresenceStatus::Mobile
        );
    }

    #[test]
    fn non_presence_traffic_passes_through() {
        let stanza = "<message from='a@b'><body>hi</body></message>";

        assert_eq!(
            rewrite_presence_content(stanza, PresenceStatus::Invisible),
            stanza
        );
    }

    #[test]
    fn finds_element_text() {
        assert_eq!(
            element_text("<show>chat</show>", "show").as_deref(),
            Some("chat")
        );
        assert_eq!(element_text("<a/>", "a").as_deref(), Some(""));
    }
}
```

In `mod.rs`, add `mod presence;` and `pub use presence::PresenceStatus;` after the `use` block.
Delete `mod.rs`'s own `PresenceStatus` enum and impl, and the presence tests that moved:
`offline_presence_strips_game_blocks_and_status`, `mobile_presence_keeps_league_without_party_details`,
`online_presence_passes_through`, `online_presence_keeps_do_not_disturb`,
`lobby_chat_is_dropped_unless_enabled`, `non_presence_traffic_passes_through`, `finds_element_text`
and the `presence_stanza` helper.

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test --lib riot::chat_proxy::presence`
Expected: compile errors such as "cannot find function `rewrite_presence`".

- [ ] **Step 5: Write `presence.rs` above the tests**

Move these functions out of `mod.rs` into `presence.rs` without changing them: `find_tag_open`,
`block_range`, `transform_block`, `element_text`, `remove_xml_elements` and
`replace_xml_element_text`. Delete `rewrite_presence_stanza` and the old `rewrite_presence_content`
from `mod.rs`. Then add:

```rust
//! Rewrites the presence Riot Client sends to its chat server, so friends see the status picked in
//! Prime.

use serde::{Deserialize, Serialize};

/// What friends see.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceStatus {
    /// Riot Client's own presence, untouched.
    #[default]
    Online,
    Mobile,
    Invisible,
}

impl PresenceStatus {
    pub const ALL: [Self; 3] = [Self::Online, Self::Mobile, Self::Invisible];

    pub fn is_online(&self) -> bool {
        *self == Self::Online
    }

    /// The XMPP `show` value friends receive.
    fn show(self) -> &'static str {
        match self {
            Self::Online => "chat",
            Self::Mobile => "mobile",
            Self::Invisible => "offline",
        }
    }
}

const PRESENCE_CLOSE: &str = "</presence>";

/// `stanza`, one complete `<presence>` element, as `status` shows it. Online and anything that
/// can't be read pass through unchanged.
pub(super) fn rewrite_presence(stanza: &str, status: PresenceStatus) -> String {
    if status.is_online() {
        return stanza.to_string();
    }
    rewrite_hidden_presence(stanza, status).unwrap_or_else(|| stanza.to_string())
}

fn rewrite_hidden_presence(stanza: &str, status: PresenceStatus) -> Option<String> {
    let open_end = tag_open_end(stanza.as_bytes(), 0)?;
    let open_tag = &stanza[..=open_end];
    if open_tag.ends_with("/>") {
        return Some(stanza.to_string());
    }

    let inner_end = stanza.rfind(PRESENCE_CLOSE)?;
    let mut inner = stanza[open_end + 1..inner_end].to_string();

    inner = replace_xml_element_text(inner, "show", status.show());
    transform_block(&mut inner, "league_of_legends", |block| {
        Some(replace_xml_element_text(block, "st", status.show()))
    });
    inner = remove_xml_elements(inner, "status");
    for game in [
        "bacon",
        "lion",
        "keystone",
        "riot_client",
        "teamfighttactics",
        "valorant",
    ] {
        inner = remove_xml_elements(inner, game);
    }

    if status == PresenceStatus::Mobile {
        transform_block(&mut inner, "league_of_legends", |block| {
            let block = remove_xml_elements(block, "p");
            Some(remove_xml_elements(block, "m"))
        });
    } else {
        inner = remove_xml_elements(inner, "league_of_legends");
    }

    Some(format!("{open_tag}{inner}{PRESENCE_CLOSE}"))
}

/// Whether `stanza` is the account's own presence. A presence for a party or lobby chat room
/// carries a `to` attribute.
pub(super) fn is_own_presence(stanza: &str) -> bool {
    let Some(open_end) = tag_open_end(stanza.as_bytes(), 0) else {
        return false;
    };
    !stanza[..open_end]
        .split_ascii_whitespace()
        .any(|attribute| attribute.starts_with("to="))
}

/// Interim: rewrites every complete presence in one socket read. Task 4 replaces the pump that
/// uses it and deletes it.
pub(super) fn rewrite_presence_content(content: &str, status: PresenceStatus) -> String {
    let mut output = String::with_capacity(content.len());
    let mut rest = content;

    while let Some(start) = rest.find("<presence") {
        output.push_str(&rest[..start]);
        let stanza = &rest[start..];
        let end = tag_open_end(stanza.as_bytes(), 0).and_then(|open_end| {
            if stanza.as_bytes()[open_end - 1] == b'/' {
                Some(open_end + 1)
            } else {
                stanza
                    .find(PRESENCE_CLOSE)
                    .map(|close| close + PRESENCE_CLOSE.len())
            }
        });
        let Some(end) = end else {
            output.push_str(stanza);
            return output;
        };
        output.push_str(&rewrite_presence(&stanza[..end], status));
        rest = &stanza[end..];
    }

    output.push_str(rest);
    output
}

/// The index of the `>` that ends the tag opened at `start`, skipping any inside quoted attribute
/// values.
pub(super) fn tag_open_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut quote = None;
    for (index, &byte) in bytes.iter().enumerate().skip(start) {
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None if byte == b'\'' || byte == b'"' => quote = Some(byte),
            None if byte == b'>' => return Some(index),
            None => {}
        }
    }
    None
}
```

The moved helpers called `tag_open_end(text, start)` with a `&str`. Change those calls to
`tag_open_end(text.as_bytes(), start)`.

In `mod.rs`:
- `ChatProxyControl::new` and its test now take `PresenceStatus::Invisible` where they took
  `PresenceStatus::Offline`.
- Delete the `connect_to_muc` field.
- In `pump_client_to_server`, call `rewrite_presence_content(&text, status)` (imported from
  `presence`).
- Rename the test `proxy_control_reads_live_status`'s expectations from `Offline` to `Invisible`.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --lib riot::chat_proxy`
Expected: PASS.

- [ ] **Step 7: Format, lint, commit**

```bash
rustfmt --edition 2024 src/riot/chat_proxy/mod.rs src/riot/chat_proxy/presence.rs
cargo clippy --all-targets
cargo test
git add Cargo.toml Cargo.lock src/riot/chat_proxy
git commit -m "Name the chat statuses Online, Mobile and Invisible, and pass Online through untouched"
```

---

### Task 2: Split presence stanzas out of the byte stream

Fixes review finding 3. Socket reads cut stanzas anywhere, so this keeps unfinished presence bytes
until the stanza is complete, and passes every other byte through exactly as received.

**Files:**
- Modify: `src/riot/chat_proxy/presence.rs`

**Interfaces:**
- Produces:
  - `pub(super) enum Piece { Raw(Vec<u8>), Presence(String) }`, with `Debug, Eq, PartialEq`.
  - `pub(super) enum SplitError { TooLarge, NotUtf8 }`, with `Debug, Eq, PartialEq`.
  - `#[derive(Debug, Default)] pub(super) struct StanzaSplitter`, whose
    `push(&mut self, bytes: &[u8]) -> Result<Vec<Piece>, SplitError>` returns pieces in stream
    order.
  - `pub(super) const MAX_PENDING: usize = 1 << 20;`

- [ ] **Step 1: Write the failing tests** (in `presence.rs`'s `tests`)

```rust
    fn raw_bytes(pieces: &[Piece]) -> Vec<u8> {
        pieces
            .iter()
            .flat_map(|piece| match piece {
                Piece::Raw(bytes) => bytes.clone(),
                Piece::Presence(_) => Vec::new(),
            })
            .collect()
    }

    #[test]
    fn passes_other_traffic_through_unchanged() {
        let mut splitter = StanzaSplitter::default();
        let traffic = b"<iq type='get' id='1'><ping/></iq><message><body>hi</body></message>";

        let pieces = splitter.push(traffic).expect("split");

        assert_eq!(raw_bytes(&pieces), traffic);
        assert!(pieces.iter().all(|piece| matches!(piece, Piece::Raw(_))));
    }

    #[test]
    fn holds_a_presence_until_it_is_complete() {
        let mut splitter = StanzaSplitter::default();

        let first = splitter
            .push(b"<iq/><presence><show>chat</show><games><valorant><p>e30")
            .expect("split");
        let second = splitter
            .push(b"=</p></valorant></games></presence><iq/>")
            .expect("split");

        assert_eq!(first, vec![Piece::Raw(b"<iq/>".to_vec())]);
        assert_eq!(
            second,
            vec![
                Piece::Presence(
                    "<presence><show>chat</show><games><valorant><p>e30=</p></valorant></games></presence>"
                        .to_string()
                ),
                Piece::Raw(b"<iq/>".to_vec()),
            ]
        );
    }

    #[test]
    fn holds_a_tag_name_cut_off_at_the_end_of_a_read() {
        let mut splitter = StanzaSplitter::default();

        let first = splitter.push(b"<iq/><pres").expect("split");
        let second = splitter
            .push(b"ence><show>chat</show></presence>")
            .expect("split");

        assert_eq!(first, vec![Piece::Raw(b"<iq/>".to_vec())]);
        assert_eq!(
            second,
            vec![Piece::Presence(
                "<presence><show>chat</show></presence>".to_string()
            )]
        );
    }

    #[test]
    fn keeps_multibyte_characters_cut_by_a_read_intact() {
        let mut splitter = StanzaSplitter::default();
        let message = "<message><body>héllo 🙂</body></message>".as_bytes();
        let cut = message
            .iter()
            .position(|&byte| byte == 0xC3)
            .expect("é starts with 0xC3")
            + 1;

        let mut pieces = splitter.push(&message[..cut]).expect("split");
        pieces.extend(splitter.push(&message[cut..]).expect("split"));

        assert_eq!(raw_bytes(&pieces), message);
    }

    #[test]
    fn a_self_closing_presence_is_complete() {
        let mut splitter = StanzaSplitter::default();

        let pieces = splitter
            .push(b"<presence type='unavailable'/>")
            .expect("split");

        assert_eq!(
            pieces,
            vec![Piece::Presence("<presence type='unavailable'/>".to_string())]
        );
    }

    #[test]
    fn longer_tag_names_are_not_presence() {
        let mut splitter = StanzaSplitter::default();
        let traffic = b"<presences>x</presences>";

        assert_eq!(raw_bytes(&splitter.push(traffic).expect("split")), traffic);
    }

    #[test]
    fn quoted_angle_brackets_do_not_end_the_tag() {
        let mut splitter = StanzaSplitter::default();
        let stanza = "<presence id='a>b'><show>chat</show></presence>";

        assert_eq!(
            splitter.push(stanza.as_bytes()).expect("split"),
            vec![Piece::Presence(stanza.to_string())]
        );
    }

    #[test]
    fn an_oversized_presence_is_refused() {
        let mut splitter = StanzaSplitter::default();
        let mut huge = b"<presence>".to_vec();
        huge.extend(std::iter::repeat_n(b'a', MAX_PENDING));

        assert_eq!(splitter.push(&huge), Err(SplitError::TooLarge));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib riot::chat_proxy::presence`
Expected: compile errors, "cannot find type `StanzaSplitter`".

- [ ] **Step 3: Implement**

Add to `presence.rs`, above the tests:

```rust
const PRESENCE_OPEN: &[u8] = b"<presence";

/// The most one stanza may hold while it waits for its end. A real presence is a few KB; past
/// this, the connection is dropped and Riot Client reconnects.
pub(super) const MAX_PENDING: usize = 1 << 20;

/// A run of the chat stream: bytes to forward as they are, or one complete presence to rewrite.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum Piece {
    Raw(Vec<u8>),
    Presence(String),
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum SplitError {
    TooLarge,
    NotUtf8,
}

/// Cuts the bytes Riot Client sends into presences and everything else. Socket reads end anywhere,
/// even inside a tag name or a multi-byte character, so an unfinished presence waits for the rest
/// of it. Other bytes go out as soon as they can't be the start of a presence.
#[derive(Debug, Default)]
pub(super) struct StanzaSplitter {
    pending: Vec<u8>,
}

impl StanzaSplitter {
    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<Vec<Piece>, SplitError> {
        self.pending.extend_from_slice(bytes);
        let mut pieces = Vec::new();

        loop {
            let Some(start) = find_presence_start(&self.pending) else {
                let flush = self.pending.len() - partial_open_suffix(&self.pending);
                if flush > 0 {
                    pieces.push(Piece::Raw(self.pending.drain(..flush).collect()));
                }
                break;
            };
            if start > 0 {
                pieces.push(Piece::Raw(self.pending.drain(..start).collect()));
            }
            let Some(end) = presence_end(&self.pending) else {
                break;
            };
            let stanza: Vec<u8> = self.pending.drain(..end).collect();
            pieces.push(Piece::Presence(
                String::from_utf8(stanza).map_err(|_| SplitError::NotUtf8)?,
            ));
        }

        if self.pending.len() > MAX_PENDING {
            return Err(SplitError::TooLarge);
        }
        Ok(pieces)
    }
}

/// Where the first `<presence` tag starts. One that ends the buffer counts, since the next read
/// may still turn it into a longer tag name, which is then skipped.
fn find_presence_start(bytes: &[u8]) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = find(&bytes[from..], PRESENCE_OPEN) {
        let start = from + found;
        match bytes.get(start + PRESENCE_OPEN.len()) {
            None => return Some(start),
            Some(&next) if next == b'>' || next == b'/' || next.is_ascii_whitespace() => {
                return Some(start);
            }
            Some(_) => from = start + 1,
        }
    }
    None
}

/// The end, exclusive, of the presence that starts `bytes`, once it has all arrived.
fn presence_end(bytes: &[u8]) -> Option<usize> {
    let open_end = tag_open_end(bytes, 0)?;
    if bytes[open_end - 1] == b'/' {
        return Some(open_end + 1);
    }
    find(&bytes[open_end..], PRESENCE_CLOSE.as_bytes())
        .map(|close| open_end + close + PRESENCE_CLOSE.len())
}

/// How many bytes at the end of `bytes` could be the start of `<presence`.
fn partial_open_suffix(bytes: &[u8]) -> usize {
    (1..PRESENCE_OPEN.len())
        .rev()
        .find(|&len| bytes.ends_with(&PRESENCE_OPEN[..len]))
        .unwrap_or(0)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib riot::chat_proxy::presence`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
rustfmt --edition 2024 src/riot/chat_proxy/presence.rs
cargo clippy --all-targets
cargo test
git add src/riot/chat_proxy/presence.rs
git commit -m "Hold unfinished presence stanzas until they arrive whole, and pass other bytes through as sent"
```

---

### Task 3: The client config proxy

Fixes review findings 6 and 9, and the config half of 7.
- Finding 6: the player's own chat server, found through its affinity, replaces the default
  server, and `chat.affinity.enabled: false` is respected.
- Finding 9: only origin-form paths are accepted. The request head is size-limited before it is
  read. A failed body read answers 502 instead of an empty 200. Every successful body is rewritten,
  whatever its size.
- Finding 7: the accept loop survives accept errors, and one HTTP client is shared.

**Files:**
- Create: `src/riot/chat_proxy/config.rs`
- Modify: `src/riot/chat_proxy/mod.rs` (delete what moves)

**Interfaces:**
- Consumes: `presence` (nothing), `crate::http_error::format_reqwest_error`.
- Produces:
  - `#[derive(Clone, Debug, Eq, PartialEq)] pub(super) struct ChatEndpoint { pub(super) host: String, pub(super) port: u16 }`.
  - `#[derive(Clone, Debug, Default)] pub(super) struct SharedEndpoint`, with
    `get(&self) -> Option<ChatEndpoint>` and
    `offer(&self, endpoint: ChatEndpoint, from_affinity: bool)`.
  - `pub(super) fn http_client() -> Result<reqwest::Client, String>`.
  - `pub(super) async fn serve(listener: TcpListener, http: reqwest::Client, chat_port: u16, endpoint: SharedEndpoint)`,
    which runs until it is aborted.
  - `pub(super) const LOCALHOST_DOMAIN: &str`, moved here from `mod.rs`. `mod.rs` re-exports it as
    `pub use config::LOCALHOST_DOMAIN;`.

- [ ] **Step 1: Write the failing tests** (create `config.rs` with only these)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const CHAT_PORT: u16 = 54321;

    fn client_config(affinity_enabled: bool) -> String {
        serde_json::json!({
            "chat.host": "chat-eu1.example.net",
            "chat.port": 5223,
            "chat.affinities": {
                "eu": "chat-eu1.example.net",
                "na": "chat-na1.example.net"
            },
            "chat.affinity.enabled": affinity_enabled,
            "unrelated": "kept"
        })
        .to_string()
    }

    fn endpoint(host: &str) -> ChatEndpoint {
        ChatEndpoint {
            host: host.to_string(),
            port: 5223,
        }
    }

    #[test]
    fn rewrites_every_chat_host_to_the_proxy() {
        let rewritten = rewrite_client_config(&client_config(true), CHAT_PORT, Some("na"));

        assert!(rewritten.body.contains(LOCALHOST_DOMAIN));
        assert!(!rewritten.body.contains("chat-eu1.example.net"));
        assert!(!rewritten.body.contains("chat-na1.example.net"));
        assert!(rewritten.body.contains("\"unrelated\":\"kept\""));
        assert!(rewritten.body.contains(&CHAT_PORT.to_string()));
        assert_eq!(rewritten.chat_endpoint, Some(endpoint("chat-na1.example.net")));
        assert!(rewritten.from_affinity);
    }

    #[test]
    fn falls_back_to_the_default_host_without_an_affinity() {
        let rewritten = rewrite_client_config(&client_config(true), CHAT_PORT, None);

        assert_eq!(rewritten.chat_endpoint, Some(endpoint("chat-eu1.example.net")));
        assert!(!rewritten.from_affinity);
    }

    #[test]
    fn ignores_affinities_riot_turned_off() {
        let rewritten = rewrite_client_config(&client_config(false), CHAT_PORT, Some("na"));

        assert_eq!(rewritten.chat_endpoint, Some(endpoint("chat-eu1.example.net")));
        assert!(!rewritten.from_affinity);
    }

    #[test]
    fn passes_through_non_json_client_config() {
        let rewritten = rewrite_client_config("internal error", CHAT_PORT, None);

        assert_eq!(rewritten.body, "internal error");
        assert_eq!(rewritten.chat_endpoint, None);
    }

    #[test]
    fn the_players_own_chat_server_wins_over_the_default() {
        let shared = SharedEndpoint::default();

        shared.offer(endpoint("default-1"), false);
        assert_eq!(shared.get(), Some(endpoint("default-1")));

        shared.offer(endpoint("affinity"), true);
        shared.offer(endpoint("default-2"), false);
        assert_eq!(shared.get(), Some(endpoint("affinity")));

        shared.offer(endpoint("affinity-2"), true);
        assert_eq!(shared.get(), Some(endpoint("affinity-2")));
    }

    #[test]
    fn reads_affinity_from_a_pas_jwt() {
        let jwt = "header.eyJhZmZpbml0eSI6ImV1In0.signature";

        assert_eq!(affinity_from_pas_jwt(jwt).as_deref(), Some("eu"));
        assert_eq!(affinity_from_pas_jwt("not-a-jwt"), None);
        assert_eq!(affinity_from_pas_jwt("a.b.c"), None);
    }

    #[tokio::test]
    async fn reads_a_get_request_head() {
        let head = b"GET /api/v1/config/player?os=windows HTTP/1.1\r\n\
            Authorization: Bearer abc\r\nX-Riot-Entitlements-JWT: ent\r\n\r\n";

        let request = read_request(&head[..]).await.expect("request");

        assert_eq!(request.path, "/api/v1/config/player?os=windows");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer abc")
        );
        assert_eq!(
            request.headers.get("x-riot-entitlements-jwt").map(String::as_str),
            Some("ent")
        );
    }

    #[tokio::test]
    async fn refuses_other_requests() {
        assert!(read_request(&b"POST /api HTTP/1.1\r\n\r\n"[..]).await.is_none());
        assert!(
            read_request(&b"GET http://evil.example/api HTTP/1.1\r\n\r\n"[..])
                .await
                .is_none()
        );
        assert!(read_request(&b"GET /api HTTP/1.1\r\nHost: x\r\n"[..]).await.is_none());

        let mut endless = b"GET /api HTTP/1.1\r\nX-Long: ".to_vec();
        endless.extend(std::iter::repeat_n(b'a', MAX_REQUEST_HEAD as usize));
        assert!(read_request(&endless[..]).await.is_none());
    }
}
```

Add `mod config;` to `mod.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib riot::chat_proxy::config`
Expected: compile errors, "cannot find function `rewrite_client_config` in this scope".

- [ ] **Step 3: Implement `config.rs`** (above the tests)

```rust
//! A local stand-in for Riot's client config service. Riot Client asks it where chat is, and it
//! answers with this PC, so chat goes through Prime's relay.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// Resolves to 127.0.0.1, and Deceive publishes a certificate for it, so Riot Client accepts the
/// relay's TLS. See the trust note in `mod.rs`.
pub const LOCALHOST_DOMAIN: &str = "deceive-localhost.molenzwiebel.xyz";
const CLIENT_CONFIG_BASE_URL: &str = "https://clientconfig.rpg.riotgames.com";
const GEO_PAS_URL: &str = "https://riot-geo.pas.si.riotgames.com/pas/v1/service/chat";
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
/// The most a request line and headers may take; a real one is under 4 KB.
const MAX_REQUEST_HEAD: u64 = 64 * 1024;
/// Windows reports a connection reset before it was accepted as an accept error. The listener
/// still works, so the loop waits briefly and goes on.
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ChatEndpoint {
    pub(super) host: String,
    pub(super) port: u16,
}

/// Where the relay connects for Riot Client. The player's own server, from their affinity, replaces
/// any other; the default server only fills in until it arrives.
#[derive(Clone, Debug, Default)]
pub(super) struct SharedEndpoint(Arc<Mutex<Option<(ChatEndpoint, bool)>>>);

impl SharedEndpoint {
    pub(super) fn get(&self) -> Option<ChatEndpoint> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|(endpoint, _)| endpoint.clone())
    }

    pub(super) fn offer(&self, endpoint: ChatEndpoint, from_affinity: bool) {
        let mut slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if from_affinity || slot.as_ref().is_none_or(|(_, held_from_affinity)| !held_from_affinity)
        {
            *slot = Some((endpoint, from_affinity));
        }
    }
}

pub(super) fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(concat!("prime/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| crate::http_error::format_reqwest_error(&error))
}

pub(super) async fn serve(
    listener: TcpListener,
    http: reqwest::Client,
    chat_port: u16,
    endpoint: SharedEndpoint,
) {
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(_) => {
                tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
                continue;
            }
        };
        let http = http.clone();
        let endpoint = endpoint.clone();
        tokio::spawn(async move { handle(stream, &http, chat_port, &endpoint).await });
    }
}

struct ConfigRequest {
    path: String,
    headers: HashMap<String, String>,
}

/// A GET's path and headers, or `None` for anything else, a head that never ends, or one over
/// `MAX_REQUEST_HEAD`.
async fn read_request(reader: impl AsyncBufRead + Unpin) -> Option<ConfigRequest> {
    let mut reader = reader.take(MAX_REQUEST_HEAD);
    let mut line = String::new();
    reader.read_line(&mut line).await.ok()?;

    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next()?, parts.next()?);
    // Riot Client asks for paths only; anything else isn't Riot Client.
    if !method.eq_ignore_ascii_case("GET") || !target.starts_with('/') {
        return None;
    }
    let path = target.to_string();

    let mut headers = HashMap::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await.ok()? == 0 {
            return None;
        }
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    Some(ConfigRequest { path, headers })
}

async fn handle(stream: TcpStream, http: &reqwest::Client, chat_port: u16, endpoint: &SharedEndpoint) {
    let (reader, mut writer) = stream.into_split();
    let Some(request) = read_request(BufReader::new(reader)).await else {
        respond(&mut writer, 400, "Bad Request", "{}").await;
        return;
    };

    let mut outbound = http.get(format!("{CLIENT_CONFIG_BASE_URL}{}", request.path));
    for (header, name) in [
        ("user-agent", "User-Agent"),
        ("authorization", "Authorization"),
        ("x-riot-entitlements-jwt", "X-Riot-Entitlements-JWT"),
    ] {
        if let Some(value) = request.headers.get(header) {
            outbound = outbound.header(name, value);
        }
    }

    let Ok(response) = outbound.send().await else {
        respond(&mut writer, 502, "Bad Gateway", "{}").await;
        return;
    };
    let status = response.status();
    let Ok(body) = response.text().await else {
        respond(&mut writer, 502, "Bad Gateway", "{}").await;
        return;
    };

    let body = if status.is_success() {
        let affinity = match request.headers.get("authorization") {
            Some(authorization) if body.contains("chat.affinities") => {
                resolve_player_affinity(http, authorization).await
            }
            _ => None,
        };
        let rewritten = rewrite_client_config(&body, chat_port, affinity.as_deref());
        if let Some(chat) = rewritten.chat_endpoint {
            endpoint.offer(chat, rewritten.from_affinity);
        }
        rewritten.body
    } else {
        body
    };

    respond(
        &mut writer,
        status.as_u16(),
        status.canonical_reason().unwrap_or("Unknown"),
        &body,
    )
    .await;
}

async fn respond(writer: &mut tokio::net::tcp::OwnedWriteHalf, status: u16, reason: &str, body: &str) {
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = writer.write_all(head.as_bytes()).await;
    let _ = writer.write_all(body.as_bytes()).await;
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RewrittenClientConfig {
    body: String,
    /// The real chat server, for the relay.
    chat_endpoint: Option<ChatEndpoint>,
    /// Whether `chat_endpoint` is the player's own server, from their affinity.
    from_affinity: bool,
}

fn rewrite_client_config(body: &str, chat_port: u16, affinity: Option<&str>) -> RewrittenClientConfig {
    let passthrough = || RewrittenClientConfig {
        body: body.to_string(),
        chat_endpoint: None,
        from_affinity: false,
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(body) else {
        return passthrough();
    };
    let Some(object) = value.as_object_mut() else {
        return passthrough();
    };

    let default_host = object
        .get("chat.host")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let port = object
        .get("chat.port")
        .and_then(serde_json::Value::as_u64)
        .and_then(|port| u16::try_from(port).ok());
    let affinities_enabled = object
        .get("chat.affinity.enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let affinity_host = affinity
        .filter(|_| affinities_enabled)
        .and_then(|region| object.get("chat.affinities")?.get(region)?.as_str())
        .map(str::to_string);

    if object.contains_key("chat.host") {
        object.insert("chat.host".to_string(), LOCALHOST_DOMAIN.into());
    }
    if object.contains_key("chat.port") {
        object.insert("chat.port".to_string(), chat_port.into());
    }
    if let Some(affinities) = object
        .get_mut("chat.affinities")
        .and_then(serde_json::Value::as_object_mut)
    {
        for host in affinities.values_mut() {
            *host = LOCALHOST_DOMAIN.into();
        }
    }

    let from_affinity = affinity_host.is_some();
    let chat_endpoint = match (affinity_host.or(default_host), port) {
        (Some(host), Some(port)) if !host.trim().is_empty() && port != 0 => {
            Some(ChatEndpoint { host, port })
        }
        _ => None,
    };
    let from_affinity = from_affinity && chat_endpoint.is_some();

    RewrittenClientConfig {
        body: serde_json::to_string(&value).unwrap_or_else(|_| body.to_string()),
        chat_endpoint,
        from_affinity,
    }
}

async fn resolve_player_affinity(http: &reqwest::Client, authorization: &str) -> Option<String> {
    let body = http
        .get(GEO_PAS_URL)
        .header(reqwest::header::AUTHORIZATION, authorization)
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    affinity_from_pas_jwt(body.trim())
}

fn affinity_from_pas_jwt(jwt: &str) -> Option<String> {
    let payload = jwt.trim().split('.').nth(1)?;
    let decoded = BASE64_URL.decode(payload.trim_end_matches('=')).ok()?;
    let payload: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    payload
        .get("affinity")?
        .as_str()
        .map(str::trim)
        .filter(|affinity| !affinity.is_empty())
        .map(str::to_string)
}
```

In `mod.rs`, delete everything this replaces:
- `LOCALHOST_DOMAIN`, `CLIENT_CONFIG_BASE_URL`, `GEO_PAS_URL` and `CONFIG_BODY_LIMIT`.
- `ChatEndpoint`, `RewrittenClientConfig`, `rewrite_client_config`, `affinity_from_pas_jwt` and
  `resolve_player_affinity`.
- `ConfigProxyEndpoint`, `RunningConfigProxy`, `start_config_proxy`, `handle_config_connection` and
  `respond_with_body`.
- Their tests: `client_config`, `rewrites_client_config_host_port_and_affinities`,
  `falls_back_to_chat_host_without_an_affinity_match`, `passes_through_non_json_client_config` and
  `reads_affinity_from_a_pas_jwt`.

Change `run_chat_proxy` and `proxy_chat_connection` to take `config::ChatEndpoint`. Task 4 replaces
both. Add `pub use config::LOCALHOST_DOMAIN;`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib riot::chat_proxy`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
rustfmt --edition 2024 src/riot/chat_proxy/mod.rs src/riot/chat_proxy/config.rs
cargo clippy --all-targets
cargo test
git add src/riot/chat_proxy
git commit -m "Prefer the player's own chat server and harden the client config proxy"
```

---

### Task 4: The chat relay

Fixes review finding 5 (a dead server connection now closes Riot Client's side too) and the delay
on mid-session changes (each connection re-sends its last presence when the status changes).
Accept errors no longer stop the relay (finding 7). Also counts live connections so the UI knows
chat is going through Prime.

**Files:**
- Create: `src/riot/chat_proxy/relay.rs`
- Modify: `src/riot/chat_proxy/mod.rs` (delete the old relay and the interim rewrite)
- Modify: `src/riot/chat_proxy/presence.rs` (delete `rewrite_presence_content` and its test)

**Interfaces:**
- Consumes: `config::{ChatEndpoint, SharedEndpoint}`,
  `presence::{Piece, PresenceStatus, StanzaSplitter, is_own_presence, rewrite_presence}`.
- Produces:
  - `pub(super) async fn serve(listener: TcpListener, acceptor: tokio_native_tls::TlsAcceptor, connector: tokio_native_tls::TlsConnector, endpoint: SharedEndpoint, status: watch::Receiver<PresenceStatus>, connections: Arc<AtomicUsize>)`,
    which runs until it is aborted.
  - `async fn relay_streams<C, S>(client: C, server: S, status: watch::Receiver<PresenceStatus>)`,
    private and tested directly.

- [ ] **Step 1: Write the failing tests** (create `relay.rs` with only these)

```rust
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, duplex};

    use super::*;

    async fn read_until(reader: &mut (impl AsyncRead + Unpin), end: &[u8]) -> String {
        let mut out = Vec::new();
        let mut byte = [0u8; 1];
        while !out.ends_with(end) {
            reader.read_exact(&mut byte).await.expect("read");
            out.push(byte[0]);
        }
        String::from_utf8(out).expect("utf-8")
    }

    const PRESENCE: &[u8] = b"<presence><show>chat</show>\
        <games><valorant><st>chat</st><p>e30=</p></valorant></games></presence>";

    #[tokio::test]
    async fn rewrites_presence_and_resends_it_when_the_status_changes() {
        let (mut riot_client, from_client) = duplex(64 * 1024);
        let (to_server, mut chat_server) = duplex(64 * 1024);
        let (status, status_rx) = watch::channel(PresenceStatus::Invisible);
        let forward = tokio::spawn(forward_client(from_client, to_server, status_rx));

        riot_client
            .write_all(b"<message><body>hi</body></message>")
            .await
            .expect("write");
        riot_client.write_all(PRESENCE).await.expect("write");
        let first = read_until(&mut chat_server, b"</presence>").await;
        assert!(first.starts_with("<message><body>hi</body></message>"), "{first}");
        assert!(first.contains("<show>offline</show>"), "{first}");
        assert!(!first.contains("valorant"), "{first}");

        status.send_replace(PresenceStatus::Online);
        let second = read_until(&mut chat_server, b"</presence>").await;
        assert_eq!(second.as_bytes(), PRESENCE);

        drop(riot_client);
        forward.await.expect("forward ends");
    }

    #[tokio::test]
    async fn sends_nothing_of_a_presence_until_it_is_whole() {
        let (mut riot_client, from_client) = duplex(64 * 1024);
        let (to_server, mut chat_server) = duplex(64 * 1024);
        let (_status, status_rx) = watch::channel(PresenceStatus::Invisible);
        tokio::spawn(forward_client(from_client, to_server, status_rx));

        riot_client.write_all(&PRESENCE[..40]).await.expect("write");
        let mut byte = [0u8; 1];
        let early =
            tokio::time::timeout(Duration::from_millis(100), chat_server.read(&mut byte)).await;
        assert!(early.is_err(), "nothing reaches the server yet");

        riot_client.write_all(&PRESENCE[40..]).await.expect("write");
        let sent = read_until(&mut chat_server, b"</presence>").await;
        assert!(sent.contains("<show>offline</show>"), "{sent}");
    }

    #[tokio::test]
    async fn stopping_the_proxy_ends_the_connection() {
        let (_riot_client, from_client) = duplex(1024);
        let (to_server, _chat_server) = duplex(1024);
        let (status, status_rx) = watch::channel(PresenceStatus::Online);
        let forward = tokio::spawn(forward_client(from_client, to_server, status_rx));

        drop(status);

        forward.await.expect("forward ends");
    }

    #[tokio::test]
    async fn a_closed_server_connection_closes_riot_clients() {
        let (mut riot_client, client_side) = duplex(1024);
        let (server_side, chat_server) = duplex(1024);
        let (_status, status_rx) = watch::channel(PresenceStatus::Online);
        let relay = tokio::spawn(relay_streams(client_side, server_side, status_rx));

        drop(chat_server);
        relay.await.expect("relay ends");

        let mut byte = [0u8; 1];
        assert_eq!(riot_client.read(&mut byte).await.expect("read"), 0);
    }
}
```

Add `mod relay;` to `mod.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib riot::chat_proxy::relay`
Expected: compile errors, "cannot find function `forward_client`".

- [ ] **Step 3: Implement `relay.rs`** (above the tests)

```rust
//! Carries Riot Client's chat to Riot's server over TLS on both sides, rewriting the presence it
//! sends.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use super::config::SharedEndpoint;
use super::presence::{Piece, PresenceStatus, StanzaSplitter, is_own_presence, rewrite_presence};

const READ_BUFFER: usize = 16 * 1024;
/// Windows reports a connection reset before it was accepted as an accept error. The listener
/// still works, so the loop waits briefly and goes on.
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(250);

pub(super) async fn serve(
    listener: TcpListener,
    acceptor: tokio_native_tls::TlsAcceptor,
    connector: tokio_native_tls::TlsConnector,
    endpoint: SharedEndpoint,
    status: watch::Receiver<PresenceStatus>,
    connections: Arc<AtomicUsize>,
) {
    loop {
        let incoming = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(_) => {
                tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
                continue;
            }
        };
        let (acceptor, connector, endpoint, status, connections) = (
            acceptor.clone(),
            connector.clone(),
            endpoint.clone(),
            status.clone(),
            connections.clone(),
        );
        tokio::spawn(async move {
            // Riot Client asks the config proxy where chat is before it connects here.
            let Some(target) = endpoint.get() else {
                return;
            };
            let Ok(client) = acceptor.accept(incoming).await else {
                return;
            };
            let Ok(upstream) = TcpStream::connect((target.host.as_str(), target.port)).await else {
                return;
            };
            let Ok(server) = connector.connect(&target.host, upstream).await else {
                return;
            };
            let _open = OpenConnection::new(connections);
            relay_streams(client, server, status).await;
        });
    }
}

/// Counts a relayed connection for as long as it lives.
struct OpenConnection(Arc<AtomicUsize>);

impl OpenConnection {
    fn new(connections: Arc<AtomicUsize>) -> Self {
        connections.fetch_add(1, Ordering::SeqCst);
        Self(connections)
    }
}

impl Drop for OpenConnection {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Relays until either side closes. Returning drops both streams, which closes the other side
/// too, so Riot Client reconnects instead of waiting on a dead connection.
async fn relay_streams<C, S>(client: C, server: S, status: watch::Receiver<PresenceStatus>)
where
    C: AsyncRead + AsyncWrite + Unpin,
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (client_read, mut client_write) = tokio::io::split(client);
    let (mut server_read, server_write) = tokio::io::split(server);

    tokio::select! {
        () = forward_client(client_read, server_write, status) => {}
        _ = tokio::io::copy(&mut server_read, &mut client_write) => {}
    }
}

/// Forwards what Riot Client sends, with each presence rewritten to the current status. When the
/// status changes, the account's last presence is sent again with the new one, so friends see it
/// without waiting for Riot Client to send another. Ends when Riot Client closes, a write fails,
/// or the proxy stops (the status sender drops).
async fn forward_client<R, W>(mut reader: R, mut writer: W, mut status: watch::Receiver<PresenceStatus>)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut splitter = StanzaSplitter::default();
    let mut own_presence: Option<String> = None;
    let mut buffer = vec![0u8; READ_BUFFER];

    loop {
        tokio::select! {
            read = reader.read(&mut buffer) => {
                let read = match read {
                    Ok(0) | Err(_) => return,
                    Ok(read) => read,
                };
                let Ok(pieces) = splitter.push(&buffer[..read]) else {
                    return;
                };
                for piece in pieces {
                    let bytes = match piece {
                        Piece::Raw(bytes) => bytes,
                        Piece::Presence(stanza) => {
                            let current = *status.borrow();
                            let rewritten = rewrite_presence(&stanza, current);
                            if is_own_presence(&stanza) {
                                own_presence = Some(stanza);
                            }
                            rewritten.into_bytes()
                        }
                    };
                    if writer.write_all(&bytes).await.is_err() {
                        return;
                    }
                }
                if writer.flush().await.is_err() {
                    return;
                }
            }
            changed = status.changed() => {
                if changed.is_err() {
                    return;
                }
                let current = *status.borrow_and_update();
                if let Some(stanza) = &own_presence {
                    let rewritten = rewrite_presence(stanza, current);
                    if writer.write_all(rewritten.as_bytes()).await.is_err()
                        || writer.flush().await.is_err()
                    {
                        return;
                    }
                }
            }
        }
    }
}
```

In `mod.rs`, delete `CHAT_PUMP_BUFFER`, `ChatProxyControl`, `run_chat_proxy`,
`proxy_chat_connection`, `pump_raw`, `pump_client_to_server` and the test
`proxy_control_reads_live_status`. In `presence.rs`, delete `rewrite_presence_content` and the test
`non_presence_traffic_passes_through`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib riot::chat_proxy`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
rustfmt --edition 2024 src/riot/chat_proxy/mod.rs src/riot/chat_proxy/relay.rs src/riot/chat_proxy/presence.rs
cargo clippy --all-targets
cargo test
git add src/riot/chat_proxy
git commit -m "Relay chat with whole presence stanzas, re-send presence on a status change, and close both sides together"
```

---

### Task 5: The certificate and the loopback check

Fixes findings 4 and 8.
- Finding 4: the domain must resolve to loopback addresses only, and to at least one, and the check
  is actually called (Task 6).
- Finding 8: the `p12` and `x509-parser` expiry check, which can't read AES-encrypted PFX files, is
  replaced by asking native-tls whether it can load the file. A cached copy is refreshed weekly and
  kept when a refresh fails. Riot Client checks the certificate's dates itself.

**Files:**
- Create: `src/riot/chat_proxy/certificate.rs`
- Modify: `src/riot/chat_proxy/mod.rs` (delete what moves)

**Interfaces:**
- Consumes: `config::LOCALHOST_DOMAIN`, `crate::image_cache::write_cache_file`,
  `crate::http_error::format_reqwest_error`.
- Produces:
  - `pub(super) async fn check_localhost_domain() -> Result<(), ChatProxyError>`.
  - `pub(super) async fn load_identity(path: &Path) -> Result<native_tls::Identity, ChatProxyError>`.
  - `pub(super) fn default_path() -> PathBuf`, which is
    `%LOCALAPPDATA%\spiiritual\prime\cache\chat-proxy-localhost.pfx`.
  - `ChatProxyError` moves here and is re-exported from `mod.rs` as
    `pub use certificate::ChatProxyError;`.

- [ ] **Step 1: Write the failing tests** (create `certificate.rs` with only these)

```rust
#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tempfile::tempdir;

    use super::*;

    fn address(text: &str) -> SocketAddr {
        text.parse().expect("address")
    }

    #[test]
    fn the_domain_must_resolve_to_this_pc_alone() {
        assert!(all_loopback(&[address("127.0.0.1:443")]));
        assert!(all_loopback(&[address("127.0.0.1:443"), address("[::1]:443")]));
        assert!(!all_loopback(&[address("127.0.0.1:443"), address("8.8.8.8:443")]));
        assert!(!all_loopback(&[address("8.8.8.8:443")]));
        assert!(!all_loopback(&[]));
    }

    #[tokio::test]
    async fn without_a_usable_certificate_the_download_error_is_reported() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("cert.pfx");
        fs::write(&path, b"not a pfx").expect("write");

        let error = load_identity_from(&path, async { Err("offline".to_string()) })
            .await
            .expect_err("no certificate");

        assert!(error.to_string().contains("offline"), "{error}");
    }

    #[tokio::test]
    async fn an_unusable_download_is_not_cached() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("cert.pfx");

        let error = load_identity_from(&path, async { Ok(b"not a pfx".to_vec()) })
            .await
            .expect_err("unusable");

        assert!(matches!(error, ChatProxyError::Certificate(_)), "{error}");
        assert!(!path.exists());
    }
}
```

Add `mod certificate;` to `mod.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib riot::chat_proxy::certificate`
Expected: compile errors, "cannot find function `all_loopback`".

- [ ] **Step 3: Implement `certificate.rs`** (above the tests)

```rust
//! The TLS certificate Riot Client sees when it connects to the relay, and the check that its
//! domain resolves to this PC.

use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use directories::ProjectDirs;
use thiserror::Error;

use super::config::LOCALHOST_DOMAIN;

const CERTIFICATE_URL: &str = "https://mln.cx/deceive/localhost.pfx";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(20);
/// Riot Client checks the certificate's dates, so a cached copy is only fetched again weekly, in
/// case Deceive's author renewed it.
const REFRESH_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Error)]
pub enum ChatProxyError {
    #[error("{} doesn't point at this PC, so Riot Client's chat can't go through Prime", LOCALHOST_DOMAIN)]
    NotLoopback,
    #[error("Couldn't look up {}: {0}", LOCALHOST_DOMAIN)]
    Dns(String),
    #[error("Couldn't get the chat certificate: {0}")]
    Certificate(String),
    #[error("Couldn't start the chat proxy: {0}")]
    Io(#[from] io::Error),
    #[error("Couldn't set up the chat proxy's TLS: {0}")]
    Tls(String),
}

pub(super) fn default_path() -> PathBuf {
    ProjectDirs::from("dev", "spiiritual", "prime")
        .map(|dirs| dirs.cache_dir().join("chat-proxy-localhost.pfx"))
        .unwrap_or_else(|| PathBuf::from("chat-proxy-localhost.pfx"))
}

/// Riot Client resolves the name itself, so this can't stop it connecting elsewhere later. It
/// does stop a launch through a resolver that already sends the name off this PC, which would
/// hand Riot Client's chat login to whoever answers.
pub(super) async fn check_localhost_domain() -> Result<(), ChatProxyError> {
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((LOCALHOST_DOMAIN, 443))
        .await
        .map_err(|error| ChatProxyError::Dns(error.to_string()))?
        .collect();
    if all_loopback(&addresses) {
        Ok(())
    } else {
        Err(ChatProxyError::NotLoopback)
    }
}

fn all_loopback(addresses: &[SocketAddr]) -> bool {
    !addresses.is_empty() && addresses.iter().all(|address| address.ip().is_loopback())
}

pub(super) async fn load_identity(path: &Path) -> Result<native_tls::Identity, ChatProxyError> {
    load_identity_from(path, download()).await
}

/// The cached certificate while it's under a week old; otherwise a fresh download, saved over it.
/// A failed download falls back to the cached copy, however old.
async fn load_identity_from(
    path: &Path,
    download: impl Future<Output = Result<Vec<u8>, String>>,
) -> Result<native_tls::Identity, ChatProxyError> {
    let cached = fs::read(path).ok().and_then(|bytes| identity(&bytes));
    let age = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok());
    if let Some(identity) = &cached
        && age.is_some_and(|age| age < REFRESH_AFTER)
    {
        return Ok(identity.clone());
    }

    match download.await {
        Ok(bytes) => match identity(&bytes) {
            Some(fresh) => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                crate::image_cache::write_cache_file(path, &bytes)?;
                Ok(fresh)
            }
            None => cached.ok_or_else(|| {
                ChatProxyError::Certificate("the download isn't a usable certificate".to_string())
            }),
        },
        Err(error) => cached.ok_or(ChatProxyError::Certificate(error)),
    }
}

fn identity(bytes: &[u8]) -> Option<native_tls::Identity> {
    native_tls::Identity::from_pkcs12(bytes, "").ok()
}

async fn download() -> Result<Vec<u8>, String> {
    let failed = |error: reqwest::Error| crate::http_error::format_reqwest_error(&error);
    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .user_agent(concat!("prime/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(failed)?;
    Ok(client
        .get(CERTIFICATE_URL)
        .send()
        .await
        .map_err(failed)?
        .error_for_status()
        .map_err(failed)?
        .bytes()
        .await
        .map_err(failed)?
        .to_vec())
}
```

In `mod.rs`, delete `PROXY_CERT_URL`, `CERT_MIN_VALID_DAYS`, `CERT_HTTP_TIMEOUT`,
`addresses_resolve_to_loopback`, `ensure_localhost_resolution`, `default_proxy_cert_path`,
`certificate_is_fresh`, `pfx_certificate_not_after`, `ensure_proxy_certificate`,
`proxy_http_client`, the old `ChatProxyError`, and the tests `loopback_check_matches_only_this_pc`
and `rejects_garbage_proxy_certificates`. Add `pub use certificate::ChatProxyError;`. Remove the
`p12`, `x509_parser`, `time` and `directories` imports from `mod.rs` if nothing there uses them.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib riot::chat_proxy`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
rustfmt --edition 2024 src/riot/chat_proxy/mod.rs src/riot/chat_proxy/certificate.rs
cargo clippy --all-targets
cargo test
git add src/riot/chat_proxy
git commit -m "Check the chat domain resolves only to this PC, and cache the certificate by age"
```

---

### Task 6: The `ChatProxy` handle and the launch argument

Puts the pieces behind one handle that the UI can hold. Dropping the last clone stops both
listeners and ends open connections (the rest of finding 7). Also fixes finding 2: no quotes in
`--client-config-url`. Login capture never goes through the proxy.

**Files:**
- Modify: `src/riot/chat_proxy/mod.rs` (whole file replaced)
- Modify: `src/launch.rs`

**Interfaces:**
- Consumes: Tasks 1–5.
- Produces (public, from `crate::riot::chat_proxy`):
  - `#[derive(Clone)] pub struct ChatProxy`, with a manual `Debug`.
  - `pub async fn start(status: PresenceStatus) -> Result<ChatProxy, ChatProxyError>` (on
    `ChatProxy`).
  - `pub fn config_port(&self) -> u16`.
  - `pub fn status(&self) -> PresenceStatus`.
  - `pub fn set_status(&self, status: PresenceStatus)`.
  - `pub fn connected(&self) -> bool`.
  - `#[cfg(test)] pub fn detached(status: PresenceStatus, connected: bool) -> ChatProxy`, for UI
    tests.
  - Re-exports `PresenceStatus`, `ChatProxyError` and `LOCALHOST_DOMAIN`.
  - In `launch.rs`, `build_launch_plan` adds `--client-config-url=http://127.0.0.1:{port}` when
    `client_config_url_port` is set. `build_launcher_login_capture_plan` ignores it.

- [ ] **Step 1: Write the failing tests**

Replace the `tests` module at the bottom of `mod.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_change_reaches_open_connections() {
        let proxy = ChatProxy::detached(PresenceStatus::Online, true);
        let mut connection = proxy.0.status.subscribe();

        proxy.set_status(PresenceStatus::Mobile);

        assert!(connection.has_changed().expect("proxy running"));
        assert_eq!(*connection.borrow_and_update(), PresenceStatus::Mobile);
        assert_eq!(proxy.status(), PresenceStatus::Mobile);
    }

    #[test]
    fn the_last_handle_dropped_ends_open_connections() {
        let proxy = ChatProxy::detached(PresenceStatus::Online, true);
        let connection = proxy.0.status.subscribe();
        let copy = proxy.clone();

        drop(proxy);
        assert!(connection.has_changed().is_ok(), "a clone keeps it running");
        drop(copy);
        assert!(connection.has_changed().is_err());
    }

    #[test]
    fn connected_follows_open_connections() {
        assert!(ChatProxy::detached(PresenceStatus::Online, true).connected());
        assert!(!ChatProxy::detached(PresenceStatus::Online, false).connected());
    }
}
```

In `src/launch.rs`, change `proxied_launch_adds_client_config_url`'s expected last argument to
`"--client-config-url=http://127.0.0.1:51234".to_string()`. Replace
`login_capture_includes_client_config_url_when_proxied` with:

```rust
    #[test]
    fn login_capture_never_goes_through_the_chat_proxy() {
        let config = LaunchConfig {
            riot_client_path: Some(PathBuf::from(
                r"C:\Riot Games\Riot Client\RiotClientServices.exe",
            )),
            client_config_url_port: Some(51234),
            ..LaunchConfig::default()
        };

        let plan = build_launcher_login_capture_plan(&config).expect("launch plan");

        assert_eq!(
            plan.args,
            vec![
                "--launch-product=valorant".to_string(),
                "--allow-multiple-clients".to_string(),
            ]
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib riot::chat_proxy launch`
Expected: compile errors in `chat_proxy` ("cannot find function `detached`"), and the launch tests
fail on the quoted argument.

- [ ] **Step 3: Write `mod.rs`** (everything above the tests)

```rust
//! Routes Riot Client's chat through Prime so friends see the account as online, on mobile or
//! invisible, the way Deceive (github.com/molenzwiebel/Deceive) does.
//!
//! A launch through Prime passes `--client-config-url` to Riot Client, pointing it at `config`.
//! That proxy fetches Riot's real client config and changes the chat host to
//! `deceive-localhost.molenzwiebel.xyz`, which resolves to 127.0.0.1, so Riot Client connects to
//! `relay`. The relay forwards chat to Riot's server and rewrites the presence Riot Client sends.
//!
//! Trust: Riot Client only connects over TLS to a certificate for that domain. Deceive's author
//! publishes one, private key included, and owns the domain, so this rests on the domain keeping
//! its 127.0.0.1 answer. Prime checks it resolves only to loopback before each launch, binds
//! only to 127.0.0.1, and never sends the certificate anywhere.

mod certificate;
mod config;
mod presence;
mod relay;

use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::AbortHandle;

pub use certificate::ChatProxyError;
pub use config::LOCALHOST_DOMAIN;
pub use presence::PresenceStatus;

/// A running chat proxy. Clones share it; dropping the last one stops it and ends its
/// connections, after which Riot Client's chat stays down until Riot Client restarts.
#[derive(Clone)]
pub struct ChatProxy(Arc<Running>);

struct Running {
    config_port: u16,
    status: watch::Sender<PresenceStatus>,
    connections: Arc<AtomicUsize>,
    tasks: Vec<AbortHandle>,
}

impl Drop for Running {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl ChatProxy {
    pub async fn start(status: PresenceStatus) -> Result<Self, ChatProxyError> {
        certificate::check_localhost_domain().await?;
        let identity = certificate::load_identity(&certificate::default_path()).await?;
        let tls = |error: native_tls::Error| ChatProxyError::Tls(error.to_string());
        let acceptor =
            tokio_native_tls::TlsAcceptor::from(native_tls::TlsAcceptor::new(identity).map_err(tls)?);
        let connector =
            tokio_native_tls::TlsConnector::from(native_tls::TlsConnector::new().map_err(tls)?);
        let http = config::http_client().map_err(ChatProxyError::Tls)?;

        let chat_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let chat_port = chat_listener.local_addr()?.port();
        let config_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let config_port = config_listener.local_addr()?.port();

        let endpoint = config::SharedEndpoint::default();
        let (status_sender, status_receiver) = watch::channel(status);
        let connections = Arc::new(AtomicUsize::new(0));
        let tasks = vec![
            tokio::spawn(config::serve(config_listener, http, chat_port, endpoint.clone()))
                .abort_handle(),
            tokio::spawn(relay::serve(
                chat_listener,
                acceptor,
                connector,
                endpoint,
                status_receiver,
                connections.clone(),
            ))
            .abort_handle(),
        ];

        Ok(Self(Arc::new(Running {
            config_port,
            status: status_sender,
            connections,
            tasks,
        })))
    }

    /// A proxy that listens on nothing, for tests.
    #[cfg(test)]
    pub fn detached(status: PresenceStatus, connected: bool) -> Self {
        Self(Arc::new(Running {
            config_port: 0,
            status: watch::Sender::new(status),
            connections: Arc::new(AtomicUsize::new(usize::from(connected))),
            tasks: Vec::new(),
        }))
    }

    /// The port Riot Client's `--client-config-url` points at.
    pub fn config_port(&self) -> u16 {
        self.0.config_port
    }

    pub fn status(&self) -> PresenceStatus {
        *self.0.status.borrow()
    }

    /// Applies at once: each open connection sends its last presence again with `status`.
    pub fn set_status(&self, status: PresenceStatus) {
        self.0.status.send_replace(status);
    }

    /// Whether Riot Client's chat goes through this proxy now.
    pub fn connected(&self) -> bool {
        self.0.connections.load(Ordering::SeqCst) > 0
    }
}

impl std::fmt::Debug for ChatProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatProxy")
            .field("config_port", &self.config_port())
            .field("status", &self.status())
            .field("connected", &self.connected())
            .finish()
    }
}
```

`#[cfg(test)]` items are only compiled into this crate's own test build, which includes
`src/ui/tests.rs`. That is enough, since nothing outside the crate uses `detached`.

In `src/launch.rs`, replace `LaunchConfig::extra_args` with a free function, and use it only in
`build_launch_plan`:

```rust
/// Riot Client fetches its config from the chat proxy on `port` instead of Riot.
fn client_config_url_arg(port: u16) -> String {
    format!("--client-config-url=http://127.0.0.1:{port}")
}
```

In `build_launch_plan`, build `args` as before and then:

```rust
    let mut args = vec![
        format!("--launch-product={}", config.product),
        format!("--launch-patchline={}", config.patchline),
    ];
    args.extend(config.client_config_url_port.map(client_config_url_arg));
```

Restore `build_launcher_login_capture_plan`'s `args` to the two fixed arguments it had before Task 0.
Delete the `impl LaunchConfig { fn extra_args … }` block. Change the `client_config_url_port` doc
comment to: `/// The chat proxy's config port, so friends see the status picked in Prime. Launches only;
login capture ignores it.`

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib riot::chat_proxy launch`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit** (zero clippy warnings from here on)

```bash
rustfmt --edition 2024 src/riot/chat_proxy/mod.rs src/launch.rs
cargo clippy --all-targets
cargo test
git add src/riot/chat_proxy/mod.rs src/launch.rs
git commit -m "Run the chat proxy behind one handle, and pass Riot Client an unquoted config URL"
```

---

### Task 7: The saved status and the signed-in PUUID

**Files:**
- Modify: `src/storage.rs`
- Modify: `src/riot/local_client.rs`

**Interfaces:**
- Produces:
  - `StoredState::presence_status: PresenceStatus`. It defaults to `Online`, which isn't written.
  - `crate::riot::local_client::signed_in_puuid() -> impl Future<Output = Option<String>>`.
  - `crate::riot::local_client::riot_client_running() -> bool`.
  - `crate::riot::local_client::entitlements_subject(body: &str) -> Option<String>`.

- [ ] **Step 1: Write the failing tests**

In `src/storage.rs`'s tests, next to `minimize_on_close_defaults_on_and_only_off_is_saved`:

```rust
    #[test]
    fn presence_status_defaults_to_online_and_only_others_are_saved() {
        let dir = tempdir().expect("temp dir");
        let repo = AccountRepository::new(dir.path().join("accounts.json"));
        let mut state = StoredState::default();
        assert_eq!(state.presence_status, PresenceStatus::Online);

        repo.save(&state).expect("save");
        let saved = fs::read_to_string(repo.path()).expect("read");
        assert!(!saved.contains("presence_status"), "{saved}");

        state.presence_status = PresenceStatus::Invisible;
        repo.save(&state).expect("save");
        let saved = fs::read_to_string(repo.path()).expect("read");
        assert!(saved.contains("invisible"), "{saved}");
        assert_eq!(
            repo.load().expect("load").presence_status,
            PresenceStatus::Invisible
        );
    }
```

Match the neighbouring test's imports (`tempdir` and `fs`). If it uses other helper names, follow
them.

In `src/riot/local_client.rs`'s tests:

```rust
    #[test]
    fn reads_the_signed_in_puuid_from_the_entitlements_token() {
        let body = r#"{"accessToken":"a","entitlements":[],"issuer":"i",
            "subject":" 1f6c9a3e-puuid ","token":"t"}"#;

        assert_eq!(
            entitlements_subject(body).as_deref(),
            Some("1f6c9a3e-puuid")
        );
        assert_eq!(entitlements_subject(r#"{"subject":""}"#), None);
        assert_eq!(entitlements_subject(r#"{"subject":null}"#), None);
        assert_eq!(entitlements_subject("not json"), None);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib storage local_client`
Expected: compile errors, "no field `presence_status`" and "cannot find function
`entitlements_subject`".

- [ ] **Step 3: Implement**

`src/storage.rs`: add `use crate::riot::chat_proxy::PresenceStatus;`. Add the field after
`live_match_weapons`:

```rust
    /// What friends see while VALORANT runs through Prime's chat proxy, and what the next launch
    /// starts with. Online, the default, isn't written.
    #[serde(default, skip_serializing_if = "PresenceStatus::is_online")]
    pub presence_status: PresenceStatus,
```

Add `presence_status: PresenceStatus::default(),` to `StoredState::default()`.

`src/riot/local_client.rs`: change the module doc's first line to
`//! The Riot Client's local API on this PC: the live match score, the names players hide, and who
//! is signed in.`. Then add, after `player_account_names`:

```rust
/// Whether the Riot Client is running on this PC, judged by its lockfile.
pub fn riot_client_running() -> bool {
    read_lockfile().is_some()
}

/// The PUUID the Riot Client on this PC is signed in as, or `None` when it isn't running, isn't
/// signed in, or didn't answer. Read from its entitlements token, of which only the subject is
/// kept.
pub async fn signed_in_puuid() -> Option<String> {
    let auth = read_lockfile()?;
    let body = local_request(&auth, |client, base| {
        client.get(format!("{base}/entitlements/v1/token"))
    })
    .await
    .ok()?;
    entitlements_subject(&body)
}

pub fn entitlements_subject(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Entitlements {
        #[serde(default)]
        subject: Option<String>,
    }

    serde_json::from_str::<Entitlements>(body)
        .ok()?
        .subject
        .map(|subject| subject.trim().to_string())
        .filter(|subject| !subject.is_empty())
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib storage local_client`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
rustfmt --edition 2024 src/storage.rs src/riot/local_client.rs
cargo clippy --all-targets
cargo test
git add src/storage.rs src/riot/local_client.rs
git commit -m "Save the chosen chat status, and read who the local Riot Client is signed in as"
```

---

### Task 8: Launch through the chat proxy

Each launch first starts a proxy with the saved status, then launches through it. A failure while
Invisible opens the "Can't go invisible" dialog. A failure while Online launches anyway, with a
warning.

**Files:**
- Modify: `src/ui/mod.rs` (state, messages, dialog)
- Modify: `src/ui/app.rs` (handlers, `start_account_launch`, `open_dialog`, `escape_message`,
  `boot`)
- Modify: `src/ui/data/launch_flow.rs` (`start_chat_proxy`)
- Modify: `src/ui/shell.rs` (dialog view)
- Test: `src/ui/tests.rs`

**Interfaces:**
- Consumes: `ChatProxy`, `PresenceStatus` (Task 6), `StoredState::presence_status` (Task 7).
- Produces:
  - `#[derive(Clone, Debug)] struct LaunchedChatProxy { account_id: AccountId, proxy: ChatProxy }`.
  - `#[derive(Clone, Debug, Eq, PartialEq)] struct InvisibleLaunchFailure { account_id: AccountId, display_name: String, error: String }`.
  - `PrimeApp` fields `chat_proxy: Option<LaunchedChatProxy>` and
    `invisible_launch_failure: Option<InvisibleLaunchFailure>`.
  - `Dialog::InvisibleLaunchFailed`.
  - `Message::ChatProxyStarted(AccountId, Result<ChatProxy, String>)`.
  - `Message::LaunchOnline(AccountId)`.
  - `Message::CancelInvisibleLaunch`.
  - `PrimeApp::run_account_launch(&mut self, account: AccountProfile, config_port: Option<u16>) -> Task<Message>`.

- [ ] **Step 1: Write the failing tests** (in `src/ui/tests.rs`)

Add `use crate::riot::chat_proxy::{ChatProxy, PresenceStatus};`, and
`super::{InvisibleLaunchFailure, LaunchedChatProxy, Dialog}` to the `use super::{...}` list. Then:

```rust
fn launchable_account(app: &PrimeApp, name: &str) -> AccountProfile {
    let mut account = account_with_backup(&app.repo.launcher_backups_dir(), name, "settings");
    account.puuid = Some(format!("{name}-puuid"));
    account
}

fn proxy_failure_for(account: &AccountProfile) -> Message {
    Message::ChatProxyStarted(
        account.id,
        Err("Couldn't get the chat certificate: offline".to_string()),
    )
}

#[test]
fn a_failed_chat_proxy_asks_before_launching_online_when_invisible() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.state.presence_status = PresenceStatus::Invisible;
    app.launching_account = Some(account.id);

    let task = app.update(proxy_failure_for(&account));

    assert_eq!(task.units(), 0, "nothing launches");
    assert_eq!(app.launching_account, None);
    let failure = app.invisible_launch_failure.as_ref().expect("dialog");
    assert_eq!(failure.account_id, account.id);
    assert!(failure.error.contains("certificate"), "{}", failure.error);
    assert_eq!(app.open_dialog(), Some(Dialog::InvisibleLaunchFailed));
}

#[test]
fn a_failed_chat_proxy_still_launches_when_online() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.launching_account = Some(account.id);

    let task = app.update(proxy_failure_for(&account));

    assert!(task.units() > 0, "launches without the proxy");
    assert_eq!(app.launching_account, Some(account.id));
    assert!(app.invisible_launch_failure.is_none());
    assert!(app.chat_proxy.is_none());
    assert_eq!(app.status.kind, StatusKind::Warning);
}

#[test]
fn a_started_chat_proxy_is_kept_for_the_launched_account() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.launching_account = Some(account.id);

    let task = app.update(Message::ChatProxyStarted(
        account.id,
        Ok(ChatProxy::detached(PresenceStatus::Online, false)),
    ));

    assert!(task.units() > 0, "launches through the proxy");
    assert_eq!(
        app.chat_proxy.as_ref().map(|launched| launched.account_id),
        Some(account.id)
    );
}

#[test]
fn a_chat_proxy_for_a_cancelled_launch_is_dropped() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());

    let task = app.update(Message::ChatProxyStarted(
        account.id,
        Ok(ChatProxy::detached(PresenceStatus::Online, false)),
    ));

    assert_eq!(task.units(), 0);
    assert!(app.chat_proxy.is_none());
}

#[test]
fn launch_online_launches_the_account_whose_proxy_failed() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.invisible_launch_failure = Some(InvisibleLaunchFailure {
        account_id: account.id,
        display_name: account.display_name.clone(),
        error: "offline".to_string(),
    });

    let task = app.update(Message::LaunchOnline(account.id));

    assert!(task.units() > 0);
    assert_eq!(app.launching_account, Some(account.id));
    assert!(app.invisible_launch_failure.is_none());
}

#[test]
fn escape_cancels_the_invisible_launch_dialog() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.invisible_launch_failure = Some(InvisibleLaunchFailure {
        account_id: account.id,
        display_name: account.display_name.clone(),
        error: "offline".to_string(),
    });

    let _ = app.update(Message::EscapePressed);

    assert!(app.invisible_launch_failure.is_none());
    assert_eq!(app.launching_account, None);
}

#[test]
fn a_failed_launch_stops_its_chat_proxy() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.launching_account = Some(account.id);
    app.chat_proxy = Some(LaunchedChatProxy {
        account_id: account.id,
        proxy: ChatProxy::detached(PresenceStatus::Online, false),
    });

    let _ = app.update(Message::LaunchFinished(Err("boom".to_string())));

    assert!(app.chat_proxy.is_none());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib ui::tests::a_failed_chat_proxy`
Expected: compile errors, "no variant `ChatProxyStarted`".

- [ ] **Step 3: Implement**

`src/ui/mod.rs`:
- Import `crate::riot::chat_proxy::ChatProxy`.
- Add the two structs from Interfaces, near `UnavailableLaunchWarning`, with doc comments:
  - `LaunchedChatProxy`: `/// The chat proxy a launch went through, and for which account. Dropping it stops the proxy.`
  - `InvisibleLaunchFailure`: `/// A launch stopped because the chat proxy couldn't start while the saved status is Invisible.`
- Add the `PrimeApp` fields after `launch_client_open`:

```rust
    /// The chat proxy the last launch went through. Dropping it stops the proxy.
    chat_proxy: Option<LaunchedChatProxy>,
    invisible_launch_failure: Option<InvisibleLaunchFailure>,
```

- Add `InvisibleLaunchFailed` to `enum Dialog`, after `UnavailableLaunch`.
- Add after `LaunchFinished` in `enum Message`:

```rust
    /// The chat proxy for a launch of this account started, or why it couldn't.
    ChatProxyStarted(AccountId, Result<ChatProxy, String>),
    /// Launches without the chat proxy, from the "Can't go invisible" dialog.
    LaunchOnline(AccountId),
    CancelInvisibleLaunch,
```

`src/ui/data/launch_flow.rs`, after `valorant_is_running`:

```rust
pub(in crate::ui) async fn start_chat_proxy(
    status: crate::riot::chat_proxy::PresenceStatus,
) -> Result<crate::riot::chat_proxy::ChatProxy, String> {
    crate::riot::chat_proxy::ChatProxy::start(status)
        .await
        .map_err(|error| error.to_string())
}
```

`src/ui/app.rs`:
- In `boot`'s struct literal, after `launch_client_open: false,`, add `chat_proxy: None,` and
  `invisible_launch_failure: None,`.
- In `open_dialog`, after the `UnavailableLaunch` entry:
  `(self.invisible_launch_failure.is_some(), Dialog::InvisibleLaunchFailed),`.
- In `escape_message`, after the `unavailable_launch_warning` branch:

```rust
        } else if self.invisible_launch_failure.is_some() {
            Message::CancelInvisibleLaunch
```

- Replace `start_account_launch` with:

```rust
    fn start_account_launch(&mut self, account: AccountProfile) -> Task<Message> {
        let id = account.id;
        let selection_changed = self.state.selected_account != Some(id);
        self.state.select_account(id);
        // Dialogs were closed when the launch was asked for; any open now were opened since.
        self.unavailable_launch_warning = None;
        self.clear_progress_status();
        self.launching_account = Some(id);
        self.launch_progress_checking = false;
        self.launch_client_open = false;
        // The launch restarts Riot Client, so the last proxy has nothing left to carry.
        self.chat_proxy = None;

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
            Task::perform(start_chat_proxy(self.state.presence_status), move |result| {
                Message::ChatProxyStarted(id, result)
            }),
            reload,
        ])
    }

    /// Restores the account's login and launches VALORANT, through the chat proxy on
    /// `config_port` when there is one.
    fn run_account_launch(
        &mut self,
        account: AccountProfile,
        config_port: Option<u16>,
    ) -> Task<Message> {
        let config = LaunchConfig {
            riot_client_path: self.state.riot_client_path.clone(),
            client_config_url_port: config_port,
            ..LaunchConfig::default()
        };
        let backup = account.launcher_session.clone();
        let saved_sessions = self.saved_launcher_sessions();

        Task::perform(
            async move { launch_account(config, backup, saved_sessions).await },
            Message::LaunchFinished,
        )
    }
```

- Add the handlers next to `LaunchAnyway`:

```rust
            Message::ChatProxyStarted(account_id, result) => {
                // Cancelled or replaced while the proxy started; dropping it stops it.
                if self.launching_account != Some(account_id) {
                    return Task::none();
                }
                let Some(account) = self.account_by_id(account_id).cloned() else {
                    self.launching_account = None;
                    self.set_status(Status::error("Account profile no longer exists"));
                    return Task::none();
                };

                match result {
                    Ok(proxy) => {
                        let port = proxy.config_port();
                        self.chat_proxy = Some(super::LaunchedChatProxy { account_id, proxy });
                        self.run_account_launch(account, Some(port))
                    }
                    Err(error) if self.state.presence_status == PresenceStatus::Invisible => {
                        self.launching_account = None;
                        self.launch_progress_checking = false;
                        self.clear_progress_status();
                        self.close_popovers();
                        self.invisible_launch_failure = Some(super::InvisibleLaunchFailure {
                            account_id,
                            display_name: account.display_name,
                            error,
                        });
                        Task::none()
                    }
                    Err(error) => {
                        // Online looks the same without the proxy; only changing it needs one.
                        let launch = self.run_account_launch(account, None);
                        self.set_status(Status::warning(format!(
                            "Your status can't be changed this session. {error}"
                        )));
                        launch
                    }
                }
            }
            Message::LaunchOnline(account_id) => {
                if self.launching_account.is_some() || self.launch_preflight_account.is_some() {
                    return Task::none();
                }
                let Some(failure) = self.invisible_launch_failure.take() else {
                    return Task::none();
                };
                if failure.account_id != account_id {
                    self.invisible_launch_failure = Some(failure);
                    return Task::none();
                }
                let Some(account) = self.account_by_id(account_id).cloned() else {
                    self.set_status(Status::error("Account profile no longer exists"));
                    return Task::none();
                };

                self.launching_account = Some(account_id);
                self.launch_progress_checking = false;
                self.launch_client_open = false;
                self.run_account_launch(account, None)
            }
            Message::CancelInvisibleLaunch => {
                self.invisible_launch_failure = None;
                Task::none()
            }
```

`account_by_id` is a private method in `shell.rs` (line 163). Make it `pub(super)` so `app.rs` can
call it.

- In `LaunchFinished`'s `Err(error)` arm, add `self.chat_proxy = None;` before `set_status`.
- Import `crate::riot::chat_proxy::PresenceStatus` and `start_chat_proxy` (from `launch_flow`) in
  `app.rs`.

`src/ui/shell.rs`: in the dialog `match`, after `Dialog::UnavailableLaunch`:

```rust
                Dialog::InvisibleLaunchFailed => invisible_launch_failed_prompt_overlay(
                    self.invisible_launch_failure.as_ref()?,
                ),
```

and next to `unavailable_launch_prompt_overlay`:

```rust
fn invisible_launch_failed_prompt_overlay(
    failure: &InvisibleLaunchFailure,
) -> Element<'_, Message> {
    dialog(
        480.0,
        Some(("CAN\u{2019}T GO INVISIBLE".to_string(), theme::GOLD)),
        format!("Launch {} online?", failure.display_name),
        Some(
            "Prime couldn\u{2019}t start the chat proxy that makes you invisible, so friends \
             would see you online."
                .to_string(),
        ),
        Some(column![dialog_note(
            theme::Icon::TriangleAlert,
            theme::GOLD,
            failure.error.as_str(),
        )]),
        vec![
            dialog_button("Cancel", Some(Message::CancelInvisibleLaunch)),
            dialog_action(
                theme::Icon::Play,
                "Launch online",
                theme::danger_button_style,
                Some(Message::LaunchOnline(failure.account_id)),
            ),
        ],
    )
}
```

Import `super::InvisibleLaunchFailure` in `shell.rs` where `UnavailableLaunchWarning` is imported.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib ui::tests`
Expected: PASS. That includes the existing launch tests, such as
`launch_starts_when_no_game_is_running`, which still see `launching_account` set.

- [ ] **Step 5: Format, lint, commit**

```bash
rustfmt --edition 2024 src/ui/mod.rs src/ui/app.rs src/ui/data/launch_flow.rs src/ui/shell.rs src/ui/tests.rs
cargo clippy --all-targets
cargo test
git add src/ui
git commit -m "Launch through the chat proxy, and ask before launching online when invisible fails"
```

---

### Task 9: Watch for the game on this PC

While a proxy runs, Prime checks every 5 seconds whether VALORANT is running and who the local Riot
Client is signed in as. `status_control_visible` combines that with the selected account. Riot
Client closing stops the proxy.

**Files:**
- Modify: `src/ui/data/launch_flow.rs` (`LocalGame`, `check_local_game`)
- Modify: `src/ui/mod.rs` (fields, messages, subscription)
- Modify: `src/ui/app.rs` (handlers, `status_control_visible`, `boot`)
- Test: `src/ui/tests.rs`

**Interfaces:**
- Consumes: `local_client::{signed_in_puuid, riot_client_running}` (Task 7), `chat_proxy` (Task 8).
- Produces:
  - `#[derive(Clone, Debug, Default, Eq, PartialEq)] pub(in crate::ui) struct LocalGame { pub(in crate::ui) valorant_running: bool, pub(in crate::ui) signed_in_puuid: Option<String>, pub(in crate::ui) riot_client_running: bool }`.
  - `pub(in crate::ui) async fn check_local_game() -> LocalGame`.
  - `PrimeApp` fields `local_game: Option<LocalGame>` and `local_game_checking: bool`.
  - `Message::LocalGameTick` and `Message::LocalGameChecked(LocalGame)`.
  - `pub(super) fn status_control_visible(&self) -> bool` on `PrimeApp`.

- [ ] **Step 1: Write the failing tests**

```rust
fn in_game_here(puuid: &str) -> LocalGame {
    LocalGame {
        valorant_running: true,
        signed_in_puuid: Some(puuid.to_string()),
        riot_client_running: true,
    }
}

/// An app whose selected account plays here through Prime's connected chat proxy.
fn app_in_game(dir: &Path) -> (PrimeApp, AccountProfile) {
    let mut app = test_app(dir);
    let account = launchable_account(&app, "Main");
    app.state.push_account(account.clone());
    app.chat_proxy = Some(LaunchedChatProxy {
        account_id: account.id,
        proxy: ChatProxy::detached(PresenceStatus::Online, true),
    });
    app.local_game = Some(in_game_here("Main-puuid"));
    (app, account)
}

#[test]
fn the_status_control_shows_while_the_selected_account_plays_here() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_in_game(dir.path());
    assert!(app.status_control_visible());

    app.local_game = Some(in_game_here("MAIN-PUUID"));
    assert!(app.status_control_visible(), "PUUIDs compare without case");
}

#[test]
fn the_status_control_hides_unless_every_condition_holds() {
    let cases: [(&str, fn(&mut PrimeApp)); 6] = [
        ("VALORANT isn't running here", |app| {
            app.local_game.as_mut().expect("game").valorant_running = false;
        }),
        ("Riot Client is signed in as someone else", |app| {
            app.local_game.as_mut().expect("game").signed_in_puuid = Some("other".to_string());
        }),
        ("Riot Client didn't say who is signed in", |app| {
            app.local_game.as_mut().expect("game").signed_in_puuid = None;
        }),
        ("chat isn't going through Prime", |app| {
            let account_id = app.chat_proxy.as_ref().expect("proxy").account_id;
            app.chat_proxy = Some(LaunchedChatProxy {
                account_id,
                proxy: ChatProxy::detached(PresenceStatus::Online, false),
            });
        }),
        ("Prime launched another account", |app| {
            app.chat_proxy.as_mut().expect("proxy").account_id = AccountId::new();
        }),
        ("Prime didn't launch the game", |app| app.chat_proxy = None),
    ];

    for (why, change) in cases {
        let dir = tempdir().expect("temp dir");
        let (mut app, _) = app_in_game(dir.path());
        change(&mut app);
        assert!(!app.status_control_visible(), "{why}");
    }
}

#[test]
fn the_status_control_hides_when_another_account_is_selected() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_in_game(dir.path());
    let alt = launchable_account(&app, "Alt");
    app.state.push_account(alt.clone());

    let _ = app.update(Message::SelectAccount(alt.id));

    assert!(!app.status_control_visible());
}

#[test]
fn the_game_is_only_checked_while_a_proxy_runs() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_in_game(dir.path());

    assert_eq!(app.update(Message::LocalGameTick).units(), 1);
    assert_eq!(app.update(Message::LocalGameTick).units(), 0, "one check at a time");

    app.local_game_checking = false;
    app.chat_proxy = None;
    assert_eq!(app.update(Message::LocalGameTick).units(), 0);
}

#[test]
fn closing_riot_client_stops_the_chat_proxy() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_in_game(dir.path());
    app.local_game_checking = true;

    let _ = app.update(Message::LocalGameChecked(LocalGame::default()));

    assert!(app.chat_proxy.is_none());
    assert!(app.local_game.is_none());
    assert!(!app.local_game_checking);
}

#[test]
fn riot_client_starting_during_a_launch_keeps_the_proxy() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account) = app_in_game(dir.path());
    app.launching_account = Some(account.id);

    let _ = app.update(Message::LocalGameChecked(LocalGame::default()));

    assert!(app.chat_proxy.is_some());
}
```

Import `super::data::launch_flow::LocalGame` in the tests.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib ui::tests::the_status_control`
Expected: compile errors, "cannot find type `LocalGame`".

- [ ] **Step 3: Implement**

`src/ui/data/launch_flow.rs`:

```rust
/// What runs on this PC: whether VALORANT is up and who the Riot Client here is signed in as.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct LocalGame {
    pub(in crate::ui) valorant_running: bool,
    pub(in crate::ui) signed_in_puuid: Option<String>,
    pub(in crate::ui) riot_client_running: bool,
}

pub(in crate::ui) async fn check_local_game() -> LocalGame {
    let valorant_running = valorant_is_running().await;
    let signed_in_puuid = if valorant_running {
        crate::riot::local_client::signed_in_puuid().await
    } else {
        None
    };
    LocalGame {
        valorant_running,
        signed_in_puuid,
        riot_client_running: crate::riot::local_client::riot_client_running(),
    }
}
```

`src/ui/mod.rs`:
- `const LOCAL_GAME_CHECK_INTERVAL: Duration = Duration::from_secs(5);`
- Fields after `chat_proxy`:

```rust
    /// The last check of what runs on this PC, made while a chat proxy runs.
    local_game: Option<LocalGame>,
    local_game_checking: bool,
```

- Messages after `CancelInvisibleLaunch`:

```rust
    LocalGameTick,
    LocalGameChecked(LocalGame),
```

- In `app_subscription`, before `Subscription::batch`:

```rust
    // Only a launch through the chat proxy can have its status changed, so only then is the game
    // watched. Nobody sees the control while minimized.
    if app.chat_proxy.is_some() && !app.window_minimized {
        subscriptions
            .push(iced::time::every(LOCAL_GAME_CHECK_INTERVAL).map(|_| Message::LocalGameTick));
    }
```

`src/ui/app.rs`:
- In `boot`, add `local_game: None,` and `local_game_checking: false,`.
- Add the method next to `launch_in_progress`:

```rust
    /// Whether the status control shows: VALORANT runs on this PC, Riot Client here is signed in
    /// as the selected account, and its chat goes through the proxy Prime launched that account
    /// with. Otherwise a change would reach nobody, or the wrong account.
    pub(super) fn status_control_visible(&self) -> bool {
        let (Some(account), Some(launched), Some(game)) = (
            self.state.selected_account(),
            self.chat_proxy.as_ref(),
            self.local_game.as_ref(),
        ) else {
            return false;
        };

        launched.account_id == account.id
            && launched.proxy.connected()
            && game.valorant_running
            && account
                .puuid
                .as_deref()
                .zip(game.signed_in_puuid.as_deref())
                .is_some_and(|(saved, signed_in)| saved.eq_ignore_ascii_case(signed_in))
    }
```

- The handlers:

```rust
            Message::LocalGameTick => {
                if self.chat_proxy.is_none() || self.local_game_checking {
                    return Task::none();
                }
                self.local_game_checking = true;
                Task::perform(check_local_game(), Message::LocalGameChecked)
            }
            Message::LocalGameChecked(game) => {
                self.local_game_checking = false;
                // Riot Client closed, so nothing will connect through this proxy again. During a
                // launch it may not have started yet.
                if !game.riot_client_running && self.launching_account.is_none() {
                    self.chat_proxy = None;
                    self.local_game = None;
                } else {
                    self.local_game = Some(game);
                }
                Task::none()
            }
```

- In `LaunchFinished`'s `Ok(result) if result.target == LaunchTargetProcess::Valorant` arm, the
  control should appear without waiting a full interval. Batch `Task::done(Message::LocalGameTick)`
  into the task it returns. For example, wrap the final `if saved_backup { … } else { … }` as
  `let save = if saved_backup { self.save_task() } else { Task::none() };` followed by
  `Task::batch([save, Task::done(Message::LocalGameTick)])`.
- In `start_account_launch`, next to `self.chat_proxy = None;`, add `self.local_game = None;`.
- Import `check_local_game` and `LocalGame` from `launch_flow`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib ui::tests`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
rustfmt --edition 2024 src/ui/mod.rs src/ui/app.rs src/ui/data/launch_flow.rs src/ui/tests.rs
cargo clippy --all-targets
cargo test
git add src/ui
git commit -m "Watch for VALORANT on this PC while a chat proxy runs, and stop it when Riot Client closes"
```

---

### Task 10: The sidebar status control and menu

**Files:**
- Create: `assets/icons/smartphone.svg`
- Modify: `src/ui/theme.rs` (`Icon::Smartphone`)
- Modify: `src/ui/components.rs` (`anchored_popover_above`)
- Modify: `src/ui/shell.rs` (the control, the menu, the sidebar)
- Modify: `src/ui/mod.rs` (field, messages)
- Modify: `src/ui/app.rs` (handlers, popovers, Escape, `boot`)
- Test: `src/ui/tests.rs`

**Interfaces:**
- Consumes: `status_control_visible` (Task 9), `PresenceStatus::ALL`, `ChatProxy::set_status`.
- Produces:
  - `PrimeApp::status_menu_open: bool`.
  - `Message::ToggleStatusMenu` and `Message::PresenceStatusPicked(PresenceStatus)`.
  - `pub(super) fn anchored_popover_above(base, popover, is_open: bool, gap: f32, right_inset: f32) -> Element<Message>`.

**Design (from DBrZG, Xq33U and q10Rjx):**
- **Control:** full sidebar width (200). Padding 10 × 12, spacing 10, radius 10. Closed: fill `BG`,
  border `LINE`. Open or hovered: fill `RAISED`, border `#3A4250`. It holds a 14 × 14 mark, the
  label (13, semibold, `TEXT`) and `ChevronsUpDown` (15, `MUTED`).
- **Marks:**
  - Online: a 9 × 9 `OK` dot.
  - Mobile: the `Smartphone` icon, 14, `MUTED`.
  - Invisible: a 10 × 10 `MUTED` ring with a 3px border and a transparent middle.
- **Menu:** 280 wide, `popover_style`, padding 6, rows 1 apart. It opens 4px above the control,
  with its left edge on the control's.
- **Menu items:** padding 8 × 10, spacing 10, aligned to the top, and `menu_item_style`
  (selected = `MENU_HOVER`). Each holds a mark column 15 × 18 centred, then the title (13; semibold
  when selected) over the description (12, `MUTED`, wraps), 2 apart, then `Check` (15, `TEXT`) on
  the selected item, or 15px of space.
- **After the items:** 8px of space, then a 1px `LINE` divider, then the note, padding 8 × 10,
  12px, `FAINT`, line height 1.4, wrapping across the full width.
- **Copy:**
  - Online: "Friends see you as usual."
  - Mobile: "Friends see you on Riot Mobile, not in game."
  - Invisible: "Friends see you offline. You can still message them."
  - Note: "Applies right away. Your next launch starts the same way."
- **Sidebar order:** … `space(Fill)`, then the live match button and the status control grouped
  with `MATCH_STATUS_GAP` (8), then the version label.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn the_status_menu_opens_only_with_the_control() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_in_game(dir.path());

    let _ = app.update(Message::ToggleStatusMenu);
    assert!(app.status_menu_open);
    let _ = app.update(Message::ToggleStatusMenu);
    assert!(!app.status_menu_open);

    app.chat_proxy = None;
    let _ = app.update(Message::ToggleStatusMenu);
    assert!(!app.status_menu_open);
}

#[test]
fn picking_a_status_saves_it_and_applies_it_now() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_in_game(dir.path());
    app.status_menu_open = true;

    let task = app.update(Message::PresenceStatusPicked(PresenceStatus::Invisible));

    assert!(task.units() > 0, "saves accounts.json");
    assert_eq!(app.state.presence_status, PresenceStatus::Invisible);
    assert_eq!(
        app.chat_proxy.as_ref().expect("proxy").proxy.status(),
        PresenceStatus::Invisible
    );
    assert!(!app.status_menu_open);
}

#[test]
fn picking_the_current_status_saves_nothing() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_in_game(dir.path());

    let task = app.update(Message::PresenceStatusPicked(PresenceStatus::Online));

    assert_eq!(task.units(), 0);
}

#[test]
fn escape_and_outside_clicks_close_the_status_menu() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_in_game(dir.path());

    app.status_menu_open = true;
    let _ = app.update(Message::EscapePressed);
    assert!(!app.status_menu_open);

    app.status_menu_open = true;
    let _ = app.update(Message::DismissPopovers);
    assert!(!app.status_menu_open);
}

#[test]
fn the_status_menu_closes_when_the_control_hides() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = app_in_game(dir.path());
    app.status_menu_open = true;

    let _ = app.update(Message::LocalGameChecked(LocalGame {
        riot_client_running: true,
        ..LocalGame::default()
    }));

    assert!(!app.status_menu_open);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib ui::tests::the_status_menu`
Expected: compile errors, "no variant `ToggleStatusMenu`".

- [ ] **Step 3: Implement the state and messages**

`src/ui/mod.rs`:
- Add the field `status_menu_open: bool,` after `account_switcher_open`.
- Add the messages after `LocalGameChecked`:

```rust
    ToggleStatusMenu,
    PresenceStatusPicked(crate::riot::chat_proxy::PresenceStatus),
```

`src/ui/app.rs`:
- In `boot`, add `status_menu_open: false,`.
- In `close_popovers` and `close_account_surfaces`, add `self.status_menu_open = false;`.
- In `escape_message`, add `|| self.status_menu_open` to the popover condition.
- At the end of the `LocalGameChecked` handler, before `Task::none()`:

```rust
                if !self.status_control_visible() {
                    self.status_menu_open = false;
                }
```

- The handlers:

```rust
            Message::ToggleStatusMenu => {
                let open = !self.status_menu_open && self.status_control_visible();
                self.close_popovers();
                self.status_menu_open = open;
                Task::none()
            }
            Message::PresenceStatusPicked(status) => {
                self.status_menu_open = false;
                if self.state.presence_status == status {
                    return Task::none();
                }
                self.state.presence_status = status;
                if let Some(launched) = &self.chat_proxy {
                    launched.proxy.set_status(status);
                }
                self.save_task()
            }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib ui::tests`
Expected: PASS.

- [ ] **Step 5: Add the icon and the upward popover**

`assets/icons/smartphone.svg`:

```svg
<!-- @license lucide-static v1.48.0 - ISC -->
<svg
  class="lucide lucide-smartphone"
  xmlns="http://www.w3.org/2000/svg"
  width="24"
  height="24"
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <rect width="14" height="20" x="5" y="2" rx="2" ry="2" />
  <path d="M12 18h.01" />
</svg>
```

`src/ui/theme.rs`: add `Smartphone,` to `enum Icon` in alphabetical position, and
`Icon::Smartphone => include_bytes!("../../assets/icons/smartphone.svg"),` to the match.

`src/ui/components.rs`:
- Add `above: bool` to `AnchoredPopover` and `AnchoredOverlay`. `anchored_popover` sets it to
  `false`. Pass it through in `overlay()`.
- Add:

```rust
/// Like `anchored_popover`, but the popover opens `gap` above its anchor, for controls near the
/// bottom of the window.
pub(super) fn anchored_popover_above<'a>(
    base: impl Into<Element<'a, Message>>,
    popover: impl Into<Element<'a, Message>>,
    is_open: bool,
    gap: f32,
    right_inset: f32,
) -> Element<'a, Message> {
    Element::new(AnchoredPopover {
        base: base.into(),
        popover: popover.into(),
        is_open,
        top_offset: gap,
        right_inset,
        above: true,
    })
}
```

- In `AnchoredOverlay::layout`, compute `y` as:

```rust
        let y = if self.above {
            (self.anchor.y - popover_size.height - self.top_offset).max(viewport.y)
        } else {
            let desired_y = self.anchor.y + self.top_offset;
            let max_y = viewport.y + viewport.height - popover_size.height;
            if desired_y > max_y {
                (self.anchor.y + self.anchor.height - popover_size.height)
                    .clamp(viewport.y, max_y.max(viewport.y))
            } else {
                desired_y
            }
        };
```

- [ ] **Step 6: Build the control and menu in `src/ui/shell.rs`**

Constants next to `POPOVER_BORDER`:

```rust
const STATUS_MENU_WIDTH: f32 = 280.0;
const STATUS_MENU_GAP: f32 = 4.0;
const OPEN_CONTROL_BORDER: Color = iced::color!(0x3A4250);
/// Between the Live Match indicator and the status control, tighter than the sidebar's 28 so
/// they read as one group.
const MATCH_STATUS_GAP: f32 = 8.0;
```

In `sidebar()`, replace `self.live_match_button(),` with:

```rust
                column![self.live_match_button(), self.status_control()]
                    .spacing(MATCH_STATUS_GAP),
```

The sidebar column's `spacing(28)` then puts 28px above the group when only one of its items
shows. Check in the app that an empty group (neither item showing) leaves no extra gap above the
version label. If it does, build the group only when at least one item is `Some`.

Methods on `PrimeApp` in `shell.rs`:

```rust
    /// What friends see, and the menu to change it, while the selected account plays here.
    fn status_control(&self) -> Option<Element<'_, Message>> {
        if !self.status_control_visible() {
            return None;
        }
        let status = self.state.presence_status;
        let is_open = self.status_menu_open;

        let control = button(
            row![
                container(presence_mark(status)).center_x(14).center_y(14),
                text(presence_label(status))
                    .size(13)
                    .font(theme::SEMIBOLD_FONT)
                    .width(Length::Fill),
                theme::icon(theme::Icon::ChevronsUpDown, 15.0, theme::MUTED),
            ]
            .spacing(10)
            .align_y(alignment::Vertical::Center),
        )
        .padding([10, 12])
        .width(Length::Fill)
        .style(move |_, button_status| status_control_style(button_status, is_open))
        .on_press(Message::ToggleStatusMenu);

        Some(anchored_popover_above(
            control,
            self.status_menu(),
            is_open,
            STATUS_MENU_GAP,
            // Negative, so the wider menu lines up with the control's left edge.
            ACCOUNT_SWITCHER_WIDTH - STATUS_MENU_WIDTH,
        ))
    }

    fn status_menu(&self) -> Element<'_, Message> {
        let current = self.state.presence_status;
        let mut menu = column![].spacing(1).width(Length::Fill);
        for status in PresenceStatus::ALL {
            menu = menu.push(status_menu_item(status, status == current));
        }
        menu = menu
            .push(
                container(
                    container(space())
                        .width(Length::Fill)
                        .height(1)
                        .style(|_| filled(theme::LINE)),
                )
                .padding(Padding {
                    top: 8.0,
                    ..Padding::ZERO
                }),
            )
            .push(
                container(
                    text("Applies right away. Your next launch starts the same way.")
                        .size(12)
                        .color(theme::FAINT)
                        .line_height(iced::widget::text::LineHeight::Relative(1.4)),
                )
                .padding([8, 10])
                .width(Length::Fill),
            );

        // Opaque, so clicks on its gaps don't reach the page it overhangs.
        opaque(
            container(menu)
                .padding(6)
                .width(STATUS_MENU_WIDTH)
                .style(popover_style),
        )
    }
```

Free functions in `shell.rs`:

```rust
fn presence_label(status: PresenceStatus) -> &'static str {
    match status {
        PresenceStatus::Online => "Online",
        PresenceStatus::Mobile => "Mobile",
        PresenceStatus::Invisible => "Invisible",
    }
}

fn presence_description(status: PresenceStatus) -> &'static str {
    match status {
        PresenceStatus::Online => "Friends see you as usual.",
        PresenceStatus::Mobile => "Friends see you on Riot Mobile, not in game.",
        PresenceStatus::Invisible => "Friends see you offline. You can still message them.",
    }
}

fn presence_mark(status: PresenceStatus) -> Element<'static, Message> {
    match status {
        PresenceStatus::Online => container(space())
            .width(9)
            .height(9)
            .style(|_| filled(theme::OK).border(iced::border::rounded(4.5)))
            .into(),
        PresenceStatus::Mobile => theme::icon(theme::Icon::Smartphone, 14.0, theme::MUTED),
        PresenceStatus::Invisible => container(space())
            .width(10)
            .height(10)
            .style(|_| iced::widget::container::Style {
                border: iced::Border {
                    color: theme::MUTED,
                    width: 3.0,
                    radius: 5.0.into(),
                },
                ..Default::default()
            })
            .into(),
    }
}

fn status_menu_item(status: PresenceStatus, is_selected: bool) -> Element<'static, Message> {
    let title = text(presence_label(status)).size(13).font(if is_selected {
        theme::SEMIBOLD_FONT
    } else {
        theme::BODY_FONT
    });
    let check: Element<'static, Message> = if is_selected {
        theme::icon(theme::Icon::Check, 15.0, theme::TEXT)
    } else {
        space().width(15).into()
    };

    button(
        row![
            container(presence_mark(status)).center_x(15).center_y(18),
            column![
                title,
                text(presence_description(status))
                    .size(12)
                    .color(theme::MUTED)
            ]
            .spacing(2)
            .width(Length::Fill),
            check,
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Top),
    )
    .padding([8, 10])
    .width(Length::Fill)
    .style(move |_, button_status| menu_item_style(button_status, is_selected))
    .on_press(Message::PresenceStatusPicked(status))
    .into()
}

fn status_control_style(
    status: iced::widget::button::Status,
    is_open: bool,
) -> iced::widget::button::Style {
    let raised = is_open
        || matches!(
            status,
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
        );
    iced::widget::button::Style {
        background: Some(if raised { theme::RAISED } else { theme::BG }.into()),
        text_color: theme::TEXT,
        border: iced::Border {
            color: if raised { OPEN_CONTROL_BORDER } else { theme::LINE },
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    }
}
```

Import `PresenceStatus` and `anchored_popover_above` in `shell.rs`. Use whichever `opaque`,
`space`, `filled` and `Padding` imports `account_switcher_menu` already uses.

- [ ] **Step 7: Check against the design**

Run `cargo run`, which uses the real `accounts.json` and Riot Client data. Don't launch VALORANT
without asking the user (AGENTS.md). To see the control without a game, temporarily make
`status_control_visible` return `true` in a debug build. Don't commit that change.
- Export DBrZG and Xq33U at scale 1 from Pencil.
- Screenshot Prime at native size.
- Compare the sidebar bottom pixel for pixel: control height 36, menu 4px above, divider gap,
  note wrapping.

Fix differences, revert the temporary change, then:

```bash
rustfmt --edition 2024 src/ui/mod.rs src/ui/app.rs src/ui/shell.rs src/ui/components.rs src/ui/theme.rs src/ui/tests.rs
cargo clippy --all-targets
cargo test
git add assets/icons/smartphone.svg src/ui
git commit -m "Add the sidebar status control: Online, Mobile or Invisible while playing here"
```

---

### Task 11: Document it and try it for real

**Files:**
- Modify: `AGENTS.md`

- [ ] **Step 1: Update `AGENTS.md`**

Under "What the app does", after the Live Match paragraph:

```markdown
Invisible status: every launch goes through a chat proxy (`src/riot/chat_proxy/`), the way Deceive
does. While VALORANT runs on this PC, Riot Client here is signed in as the selected account and its
chat goes through that proxy, a sidebar control above the version label shows the status friends
see: Online, Mobile or Invisible. A change applies at once and is saved (`presence_status` in
`accounts.json`, written only when not Online), so the next launch starts with it. If the proxy
can't start and the saved status is Invisible, the launch asks before going online; with Online it
launches anyway and warns that the status can't be changed this session. Prime checks the game every
5 seconds while a proxy runs and stops the proxy when Riot Client closes. Quitting Prime while
VALORANT runs drops Riot Client's chat until Riot Client restarts.
```

Under "Code layout", in the `src/riot/` line, add `the chat proxy for the invisible status
(chat_proxy/)` to the list.

Under "Local data", add `The chat proxy's certificate is cached in %LOCALAPPDATA%\spiiritual\prime\cache\chat-proxy-localhost.pfx.`

Under "Rules", add:

```markdown
- The chat proxy trusts `deceive-localhost.molenzwiebel.xyz` to resolve to 127.0.0.1 and uses the
  certificate Deceive publishes, private key included. Check the name resolves only to loopback
  before each launch, bind proxy listeners to 127.0.0.1 only, and never send that certificate
  anywhere.
```

Under "Riot API notes", add:

```markdown
- The chat proxy passes `--client-config-url=http://127.0.0.1:{port}` (no quotes) and rewrites
  `chat.host`, `chat.port` and `chat.affinities` in Riot's client config. The real server is the
  player's affinity host from `riot-geo.pas.si.riotgames.com/pas/v1/service/chat`, unless
  `chat.affinity.enabled` is false.
- Who the local Riot Client is signed in as comes from its `GET /entitlements/v1/token` subject.
  Only the subject is kept.
```

- [ ] **Step 2: Commit the docs**

```bash
git add AGENTS.md
git commit -m "Document the invisible status and the chat proxy's trust"
```

- [ ] **Step 3: Try it with a real game (ask the user first)**

Launching closes Riot Client and VALORANT and replaces the live login, so ask the user before each
launch. With their go-ahead, and a friend's client or second account to watch from:

1. With Online saved, launch from Prime. The control appears within about 5 seconds of the
   VALORANT window and shows Online. The friend sees you online and in game.
2. Pick Invisible. Within a few seconds the friend sees you offline. Send a message both ways. If
   messages don't get through, change Invisible's description in `shell.rs` and remove "You can
   still message them".
3. Pick Mobile. The friend sees you on mobile.
4. Start a match while Invisible and open Live Match. If the score shows as unavailable, add
   "Live Match's score is unavailable while Invisible" to the AGENTS.md paragraph. Fixing that is
   out of scope.
5. Sign out in Riot Client. The control hides.
6. Close Riot Client. The control hides. Run `netstat -ano | findstr LISTENING` before and after the
   next check: Prime's two 127.0.0.1 ports go.
7. Launch again with Invisible saved. The friend never sees you come online.
8. Run `nslookup deceive-localhost.molenzwiebel.xyz` and note what it returns, for the review.

Report each result to the user, with screenshots of the control and menu.
```
