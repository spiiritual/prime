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
    let valorant = BASE64_STANDARD.encode(valorant_presence(millis));
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
         <valorant><st>chat</st><s.t>{millis}</s.t><s.p>valorant</s.p><s.r>PC</s.r><p>{valorant}</p><pty/></valorant>\
         </games><show>chat</show><platform>riot</platform></presence>\
         <message from='{from}' stamp='{stamp}' id='prime-{millis}' type='chat'>\
         <body>Prime is active. Friends see you {seen_as}.</body></message>"
    )
}

/// VALORANT's nested presence, as Deceive sends it since VALORANT dropped the flat one. VALORANT
/// logs the fields it reads and doesn't find, such as the loop and party states.
fn valorant_presence(millis: i128) -> String {
    let no_party = "00000000-0000-0000-0000-000000000000";
    serde_json::json!({
        "isValid": true,
        "isIdle": false,
        "queueId": "competitive",
        "provisioningFlow": "Invalid",
        "partyId": no_party,
        "partySize": 1,
        "maxPartySize": 5,
        "partyOwnerMatchScoreAllyTeam": 0,
        "partyOwnerMatchScoreEnemyTeam": 0,
        "premierPresenceData": {
            "rosterId": "",
            "rosterName": "Prime is active.",
            "rosterTag": "Prime Active!",
            "rosterType": "VCT",
            "division": 0,
            "score": 0,
            "plating": 0,
            "showAura": false,
            "showTag": true,
            "showPlating": false
        },
        "matchPresenceData": {
            "sessionLoopState": "MENUS",
            "provisioningFlow": "Invalid",
            "matchMap": "",
            "queueId": "competitive"
        },
        "partyPresenceData": {
            "partyId": no_party,
            "isPartyOwner": true,
            "partyState": "DEFAULT",
            "partyAccessibility": "CLOSED",
            "partyLFM": false,
            "partyClientVersion": "unknown",
            "partyVersion": millis,
            "partySize": 1,
            "queueEntryTime": "0001.01.01-00.00.00",
            "isPartyCrossPlayEnabled": false,
            "isPlayerCrossPlayEnabled": false,
            "partyPrecisePlatformTypes": 1,
            "customGameName": "Prime Active!",
            "customGameTeam": "",
            "maxPartySize": 5,
            "tournamentId": "",
            "rosterId": "",
            "partyOwnerSessionLoopState": "MENUS",
            "partyOwnerMatchMap": "",
            "partyOwnerProvisioningFlow": "Invalid",
            "partyOwnerMatchScoreAllyTeam": 0,
            "partyOwnerMatchScoreEnemyTeam": 0
        },
        "playerPresenceData": {
            "playerCardId": "893deca1-4123-9c1f-2985-aa9de74cb512",
            "playerTitleId": "e3ca05a4-4e44-9afe-3791-7d96ca8f71fa",
            "accountLevel": 999,
            "competitiveTier": 0,
            "leaderboardPosition": 0
        }
    })
    .to_string()
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

    #[test]
    fn the_friend_has_valorants_nested_presence() {
        let text = announcement(PresenceStatus::Online);
        let valorant = &text[text.find("<s.p>valorant</s.p>").expect("valorant")..];
        let p =
            &valorant[valorant.find("<p>").expect("<p>") + 3..valorant.find("</p>").expect("</p>")];
        let json: serde_json::Value =
            serde_json::from_slice(&BASE64_STANDARD.decode(p).expect("base64")).expect("json");

        // Without these, VALORANT logs an unhandled loop state and party state for the friend.
        assert_eq!(json["matchPresenceData"]["sessionLoopState"], "MENUS");
        assert_eq!(json["partyPresenceData"]["partyState"], "DEFAULT");
        assert!(json["playerPresenceData"].is_object(), "{json}");
        assert!(json["premierPresenceData"].is_object(), "{json}");
    }
}
