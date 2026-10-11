//! The in-game sign that Prime carries chat: a made-up friend, "Prime Active!", added to the
//! friend list the way Deceive adds "Deceive Active!", who messages the status friends see.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

use super::presence::{PresenceStatus, tag_open_end};

/// No Riot account has this PUUID; Riot's chat server never hears of it.
const FRIEND_PUUID: &str = "5e0c1b8e-2f7a-4c3d-9a61-7b2d4e8f0a19";
const FRIEND_NAME: &str = "&#9;Prime Active!";
const ROSTER_OPEN: &str = "<query xmlns='jabber:iq:riotgames:roster'>";
const ROSTER_EMPTY: &str = "<query xmlns='jabber:iq:riotgames:roster'/>";

fn friend_jid() -> String {
    format!("{FRIEND_PUUID}@eu1.pvp.net")
}

fn roster_item() -> String {
    format!(
        "<item jid='{jid}' name='{FRIEND_NAME}' subscription='both' puuid='{FRIEND_PUUID}'>\
         <group priority='9999'>Prime</group><state>online</state>\
         <id name='{FRIEND_NAME}' tagline='...'/><lol name='{FRIEND_NAME}'/>\
         <platforms><riot name='{FRIEND_NAME}' tagline='...'/></platforms></item>",
        jid = friend_jid()
    )
}

/// `stanza` from Riot's chat server with the friend added, if it's the roster Riot Client asked
/// for. Each fetch gets it, so a roster fetched again keeps it. A roster push, which only
/// changes one friend, doesn't.
pub(super) fn add_to_roster(stanza: &str) -> Option<String> {
    let open_tag = &stanza[..tag_open_end(stanza.as_bytes(), 0)?];
    if !open_tag.starts_with("<iq") || !open_tag.contains("type='result'") {
        return None;
    }
    if let Some(at) = stanza.find(ROSTER_OPEN) {
        let at = at + ROSTER_OPEN.len();
        return Some(format!(
            "{}{}{}",
            &stanza[..at],
            roster_item(),
            &stanza[at..]
        ));
    }
    let at = stanza.find(ROSTER_EMPTY)?;
    Some(format!(
        "{}{ROSTER_OPEN}{}</query>{}",
        &stanza[..at],
        roster_item(),
        &stanza[at + ROSTER_EMPTY.len()..]
    ))
}

/// Whether Riot Client sent `stanza` to the friend, or about it: a message, a removal or a
/// block. Riot's server mustn't see those, as it would learn the friend was made up.
pub(super) fn is_for_friend(stanza: &str) -> bool {
    stanza.contains(FRIEND_PUUID)
}

/// The friend's presence, online in VALORANT, then a message saying how friends see the account.
pub(super) fn announcement(status: PresenceStatus) -> String {
    let now = time::OffsetDateTime::now_utc();
    let millis = now.unix_timestamp_nanos() / 1_000_000;
    let valorant = BASE64_STANDARD.encode(
        "{\"isValid\":true,\"partyId\":\"00000000-0000-0000-0000-000000000000\",\
         \"partyClientVersion\":\"unknown\",\"accountLevel\":1000}",
    );
    let stamp = format!(
        "{}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.millisecond()
    );
    let seen_as = match status {
        PresenceStatus::Online => "online",
        PresenceStatus::Mobile => "on mobile",
        PresenceStatus::Invisible => "offline (invisible)",
    };
    let from = format!("{}/RC-Prime", friend_jid());
    format!(
        "<presence from='{from}' id='prime-{millis}'><games>\
         <keystone><st>chat</st><s.t>{millis}</s.t><s.p>keystone</s.p></keystone>\
         <valorant><st>chat</st><s.t>{millis}</s.t><s.p>valorant</s.p><p>{valorant}</p></valorant>\
         </games><show>chat</show><platform>riot</platform></presence>\
         <message from='{from}' stamp='{stamp}' id='prime-{millis}' type='chat'>\
         <body>Prime is active. Friends see you {seen_as}.</body></message>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_the_friend_to_a_roster_with_or_without_friends() {
        let roster =
            "<iq type='result'><query xmlns='jabber:iq:riotgames:roster'><item/></query></iq>";
        let added = add_to_roster(roster).expect("roster");
        assert!(added.contains("roster'><item jid='5e0c1b8e"), "{added}");
        assert!(added.ends_with("<item/></query></iq>"), "{added}");
        assert!(is_for_friend(&added));

        let empty = "<iq type='result'><query xmlns='jabber:iq:riotgames:roster'/></iq>";
        let added = add_to_roster(empty).expect("roster");
        assert!(added.contains("roster'><item jid='5e0c1b8e"), "{added}");
        assert!(added.ends_with("</item></query></iq>"), "{added}");

        let push = "<iq type='set'><query xmlns='jabber:iq:riotgames:roster'><item/></query></iq>";
        assert!(add_to_roster(push).is_none());
        assert!(add_to_roster("<message><body>hi</body></message>").is_none());
    }

    #[test]
    fn the_announcement_names_the_status() {
        let text = announcement(PresenceStatus::Invisible);
        assert!(text.starts_with("<presence from='5e0c1b8e"), "{text}");
        assert!(
            text.contains("you offline (invisible).</body></message>"),
            "{text}"
        );
    }
}
