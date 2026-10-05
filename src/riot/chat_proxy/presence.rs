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
        assert!(is_own_presence(
            "<presence from='a@b/RC' id='1'><show>chat</show></presence>"
        ));
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
