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
    let open_tag = &stanza[..open_end];
    find_tag_open(open_tag, "presence", 0) == Some(0)
        && !open_tag
            .split_ascii_whitespace()
            .any(|attribute| attribute.starts_with("to="))
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

fn find_tag_open(haystack: &str, tag: &str, from: usize) -> Option<usize> {
    let pattern = format!("<{tag}");
    let mut search = from.min(haystack.len());

    while let Some(found) = haystack[search..].find(&pattern) {
        let start = search + found;
        let after = start + pattern.len();
        let boundary = haystack[after..]
            .chars()
            .next()
            .is_none_or(|next| next == '>' || next == '/' || next.is_whitespace());

        if boundary {
            return Some(start);
        }
        search = after;
    }

    None
}

fn block_range(text: &str, tag: &str) -> Option<(usize, usize)> {
    let start = find_tag_open(text, tag, 0)?;
    let open_end = tag_open_end(text.as_bytes(), start)?;

    if text.as_bytes()[open_end - 1] == b'/' {
        return Some((start, open_end));
    }

    let close_pattern = format!("</{tag}");
    let relative = text[open_end..].find(&close_pattern)?;
    let close_start = open_end + relative;
    let close_end = tag_open_end(text.as_bytes(), close_start)?;

    Some((start, close_end))
}

fn transform_block(text: &mut String, tag: &str, transform: impl FnOnce(String) -> Option<String>) {
    let Some((start, end)) = block_range(text, tag) else {
        return;
    };

    match transform(text[start..=end].to_string()) {
        Some(replacement) => text.replace_range(start..=end, &replacement),
        None => text.replace_range(start..=end, ""),
    }
}

#[cfg(test)]
fn element_text(haystack: &str, tag: &str) -> Option<String> {
    let start = find_tag_open(haystack, tag, 0)?;
    let open_end = tag_open_end(haystack.as_bytes(), start)?;

    if haystack.as_bytes()[open_end - 1] == b'/' {
        return Some(String::new());
    }

    let content_start = open_end + 1;
    let close = format!("</{tag}>");
    let relative = haystack[content_start..].find(&close)?;

    Some(haystack[content_start..content_start + relative].to_string())
}

fn remove_xml_elements(mut text: String, tag: &str) -> String {
    let mut from = 0;

    while let Some((start, end)) =
        block_range(&text[from..], tag).map(|(start, end)| (start + from, end + from))
    {
        text.replace_range(start..=end, "");
        from = start;
    }

    text
}

fn replace_xml_element_text(mut text: String, tag: &str, replacement: &str) -> String {
    let close = format!("</{tag}>");
    let mut from = 0;

    while let Some(start) = find_tag_open(&text, tag, from) {
        let Some(open_end) = tag_open_end(text.as_bytes(), start) else {
            break;
        };

        if text.as_bytes()[open_end - 1] == b'/' {
            from = open_end + 1;
            continue;
        }

        let content_start = open_end + 1;
        let Some(relative) = text[content_start..].find(&close) else {
            break;
        };
        let content_end = content_start + relative;

        text.replace_range(content_start..content_end, replacement);
        from = content_end + replacement.len() + close.len();
    }

    text
}

/// The most one stanza may hold while it waits for its end. A presence is a few KB and a roster
/// with hundreds of friends a few hundred; past this, the connection is dropped and Riot Client
/// reconnects.
pub(super) const MAX_PENDING: usize = 4 << 20;

/// `<stream:stream>` stays open for the whole connection, and Riot's server opens a new one after
/// sign-in without closing the first.
const STREAM_TAG: &[u8] = b"stream:stream";

/// A run of the chat stream: bytes between stanzas, such as the stream header and whitespace
/// keep-alives, or one complete stanza.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum Piece {
    Raw(Vec<u8>),
    Stanza(String),
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum SplitError {
    TooLarge,
    NotUtf8,
}

/// Cuts a chat stream into whole stanzas and the bytes between them. Socket reads end anywhere,
/// even inside a tag name or a multi-byte character, so an unfinished stanza waits for the rest
/// of it. Everything handed out ends between two stanzas, so a stanza Prime writes after it can't
/// land inside one.
#[derive(Debug, Default)]
pub(super) struct StanzaSplitter {
    pending: Vec<u8>,
    /// How much of `pending` has been looked at.
    scanned: usize,
    /// Open elements, not counting `<stream:stream>`. A stanza is an element opened at 0.
    depth: usize,
    tag_start: Option<usize>,
    quote: Option<u8>,
    stanza_start: Option<usize>,
}

impl StanzaSplitter {
    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<Vec<Piece>, SplitError> {
        self.pending.extend_from_slice(bytes);
        let mut pieces = Vec::new();
        let mut emitted = 0;

        while self.scanned < self.pending.len() {
            let index = self.scanned;
            let byte = self.pending[index];
            self.scanned += 1;
            let Some(tag_start) = self.tag_start else {
                if byte == b'<' {
                    self.tag_start = Some(index);
                }
                continue;
            };
            // A `>` inside a quoted attribute value doesn't end the tag.
            match self.quote {
                Some(open) => {
                    if byte == open {
                        self.quote = None;
                    }
                    continue;
                }
                None if byte == b'\'' || byte == b'"' => {
                    self.quote = Some(byte);
                    continue;
                }
                None if byte != b'>' => continue,
                None => self.tag_start = None,
            }

            let tag = &self.pending[tag_start..=index];
            let closing = tag[1] == b'/';
            if tag[if closing { 2 } else { 1 }..].starts_with(STREAM_TAG) {
                continue;
            }
            let stanza = if closing {
                self.depth = self.depth.saturating_sub(1);
                if self.depth == 0 {
                    self.stanza_start.take()
                } else {
                    None
                }
            } else if tag[1] == b'?' || tag[1] == b'!' {
                None
            } else if tag[tag.len() - 2] == b'/' {
                (self.depth == 0).then_some(tag_start)
            } else {
                if self.depth == 0 {
                    self.stanza_start = Some(tag_start);
                }
                self.depth += 1;
                None
            };

            if let Some(start) = stanza {
                if start > emitted {
                    pieces.push(Piece::Raw(self.pending[emitted..start].to_vec()));
                }
                let text = std::str::from_utf8(&self.pending[start..=index])
                    .map_err(|_| SplitError::NotUtf8)?;
                pieces.push(Piece::Stanza(text.to_string()));
                emitted = index + 1;
            }
        }

        let held = self
            .stanza_start
            .or(self.tag_start)
            .unwrap_or(self.pending.len());
        if held > emitted {
            pieces.push(Piece::Raw(self.pending[emitted..held].to_vec()));
            emitted = held;
        }
        self.pending.drain(..emitted);
        self.scanned -= emitted;
        self.tag_start = self.tag_start.map(|start| start - emitted);
        self.stanza_start = self.stanza_start.map(|start| start - emitted);

        if self.pending.len() > MAX_PENDING {
            return Err(SplitError::TooLarge);
        }
        Ok(pieces)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(splitter: &mut StanzaSplitter, bytes: &[u8]) -> Vec<Piece> {
        splitter.push(bytes).expect("split")
    }

    fn stanza(text: &str) -> Piece {
        Piece::Stanza(text.to_string())
    }

    #[test]
    fn cuts_the_stream_into_whole_stanzas() {
        let mut splitter = StanzaSplitter::default();

        let pieces = split(
            &mut splitter,
            b"<?xml version='1.0'?><stream:stream to='x'>\
              <iq type='get' id='1'><ping/></iq> <presence/><message><body>a > b</body></message>",
        );

        assert_eq!(
            pieces,
            vec![
                Piece::Raw(b"<?xml version='1.0'?><stream:stream to='x'>".to_vec()),
                stanza("<iq type='get' id='1'><ping/></iq>"),
                Piece::Raw(b" ".to_vec()),
                stanza("<presence/>"),
                stanza("<message><body>a > b</body></message>"),
            ]
        );
    }

    #[test]
    fn a_second_stream_header_after_sign_in_keeps_stanzas_at_the_top() {
        let mut splitter = StanzaSplitter::default();
        split(&mut splitter, b"<stream:stream><success/>");

        let pieces = split(&mut splitter, b"<stream:stream id='2'><iq/>");

        assert_eq!(
            pieces,
            vec![
                Piece::Raw(b"<stream:stream id='2'>".to_vec()),
                stanza("<iq/>")
            ]
        );
    }

    #[test]
    fn holds_a_stanza_until_it_is_complete() {
        let mut splitter = StanzaSplitter::default();

        let first = split(
            &mut splitter,
            b"<iq/><presence><show>chat</show><games><p>e30",
        );
        let second = split(&mut splitter, b"=</p></games></presence> ");

        assert_eq!(first, vec![stanza("<iq/>")]);
        assert_eq!(
            second,
            vec![
                stanza("<presence><show>chat</show><games><p>e30=</p></games></presence>"),
                Piece::Raw(b" ".to_vec()),
            ]
        );
    }

    #[test]
    fn holds_a_tag_cut_off_at_the_end_of_a_read() {
        let mut splitter = StanzaSplitter::default();

        assert_eq!(
            split(&mut splitter, b" <pres"),
            vec![Piece::Raw(b" ".to_vec())]
        );
        assert_eq!(
            split(&mut splitter, b"ence id='a>b'/>"),
            vec![stanza("<presence id='a>b'/>")]
        );
    }

    #[test]
    fn keeps_a_multibyte_character_cut_by_a_read_intact() {
        let mut splitter = StanzaSplitter::default();
        let message = "<message><body>héllo 🙂</body></message>";
        let cut = message.find('é').expect("é") + 1;

        let mut pieces = split(&mut splitter, &message.as_bytes()[..cut]);
        pieces.extend(split(&mut splitter, &message.as_bytes()[cut..]));

        assert_eq!(pieces, vec![stanza(message)]);
    }

    #[test]
    fn an_oversized_stanza_is_refused() {
        let mut splitter = StanzaSplitter::default();
        let mut huge = b"<presence>".to_vec();
        huge.extend(std::iter::repeat_n(b'a', MAX_PENDING));

        assert_eq!(splitter.push(&huge), Err(SplitError::TooLarge));
    }

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
        assert!(is_own_presence(
            "<presence from='a@b/RC' id='1'><show>chat</show></presence>"
        ));
        assert!(!is_own_presence(
            "<presence to='room@lobby.example' from='a@b/RC'><show>chat</show></presence>"
        ));
        assert!(!is_own_presence(
            "<presence\n  to='room@lobby.example'><show>chat</show></presence>"
        ));
        assert!(!is_own_presence("<presences/>"));
        assert!(!is_own_presence("<message><body>hi</body></message>"));
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
    fn finds_element_text() {
        assert_eq!(
            element_text("<show>chat</show>", "show").as_deref(),
            Some("chat")
        );
        assert_eq!(element_text("<a/>", "a").as_deref(), Some(""));
    }
}
