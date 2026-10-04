//! The Riot Client's local API on this PC, read only for a live match. No Riot server reports
//! the score; only the chat presence the Riot Client shares with friends has it. Its Player
//! Account lookup also names players who hide their name in game.

use std::path::PathBuf;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::Deserialize;

const LOCAL_TIMEOUT: Duration = Duration::from_secs(5);

/// The team scores of a match in progress, from the selected account's side.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MatchScore {
    pub ally: i64,
    pub enemy: i64,
}

impl MatchScore {
    /// The round being played now.
    pub fn round(self) -> i64 {
        self.ally + self.enemy + 1
    }
}

/// Why a match has no score to show.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScoreUnavailable {
    RiotClientNotRunning,
    NotFound,
    Failed(String),
}

impl ScoreUnavailable {
    pub fn reason(&self) -> String {
        match self {
            Self::RiotClientNotRunning => {
                "The score comes from the Riot Client on this PC, which isn't running.".to_string()
            }
            Self::NotFound => "The Riot Client on this PC isn't sharing this match's score. It \
                               does when this account, or a friend of it, is signed in here."
                .to_string(),
            Self::Failed(error) => format!("The Riot Client on this PC didn't answer: {error}"),
        }
    }
}

/// The port and password from the Riot Client's lockfile. Read for each request, never saved.
#[derive(Clone, Eq, PartialEq)]
pub struct LocalApiAuth {
    pub port: u16,
    pub password: String,
}

impl std::fmt::Debug for LocalApiAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalApiAuth")
            .field("port", &self.port)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// Reads `name:pid:port:password:protocol`.
pub fn parse_lockfile(contents: &str) -> Option<LocalApiAuth> {
    let parts: Vec<&str> = contents.trim().split(':').collect();
    let [_, _, port, password, _] = parts.as_slice() else {
        return None;
    };

    Some(LocalApiAuth {
        port: port.parse().ok()?,
        password: (*password).to_string(),
    })
}

fn lockfile_path() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("Riot Games")
            .join("Riot Client")
            .join("Config")
            .join("lockfile"),
    )
}

fn read_lockfile() -> Option<LocalApiAuth> {
    lockfile_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|contents| parse_lockfile(&contents))
}

/// Sends `request` (built from a client and the base URL) with the lockfile's Basic auth and
/// returns the response body.
async fn local_request(
    auth: &LocalApiAuth,
    request: impl FnOnce(&reqwest::Client, &str) -> reqwest::RequestBuilder,
) -> Result<String, String> {
    let failed = |error: reqwest::Error| crate::http_error::format_reqwest_error(&error);
    // The Riot Client serves its local API with a self-signed certificate, and this client only
    // ever talks to 127.0.0.1. One client for every request, so Live Match polls reuse the
    // connection.
    static CLIENT: std::sync::LazyLock<Result<reqwest::Client, String>> =
        std::sync::LazyLock::new(|| {
            reqwest::Client::builder()
                .timeout(LOCAL_TIMEOUT)
                .tls_danger_accept_invalid_certs(true)
                // Certificates aren't checked, so the password must only ever reach this PC.
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|error| crate::http_error::format_reqwest_error(&error))
        });
    let client = CLIENT.as_ref().map_err(Clone::clone)?;
    request(client, &format!("https://127.0.0.1:{}", auth.port))
        .basic_auth("riot", Some(&auth.password))
        .send()
        .await
        .map_err(failed)?
        .error_for_status()
        .map_err(failed)?
        .text()
        .await
        .map_err(failed)
}

/// The score of the match `puuid` is playing, from the local Riot Client's chat presences.
pub async fn match_score(puuid: &str) -> Result<MatchScore, ScoreUnavailable> {
    let auth = read_lockfile().ok_or(ScoreUnavailable::RiotClientNotRunning)?;
    let body = local_request(&auth, |client, base| {
        client.get(format!("{base}/chat/v4/presences"))
    })
    .await
    .map_err(ScoreUnavailable::Failed)?;

    presence_score(&body, puuid)
}

/// Riot IDs for `puuids` as `(puuid, game name, tag line)`, from the local Riot Client's Player
/// Account lookup. Unlike VALORANT's name service it ignores streamer mode, so it names players
/// who hide their name in a match. Needs the Riot Client running on this PC, signed in as any
/// account; the error says why there are no names.
pub async fn player_account_names(
    puuids: &[String],
) -> Result<Vec<(String, String, String)>, String> {
    let auth = read_lockfile().ok_or_else(|| {
        "Hidden names come from the Riot Client on this PC, which isn't running.".to_string()
    })?;
    let body = local_request(&auth, |client, base| {
        client
            .post(format!(
                "{base}/player-account/lookup/v2/namesets-for-puuids"
            ))
            .json(&serde_json::json!({ "puuids": puuids }))
    })
    .await
    .map_err(|error| format!("The Riot Client on this PC didn't answer: {error}"))?;
    Ok(namesets(&body))
}

#[derive(Deserialize)]
struct Namesets {
    #[serde(default)]
    namesets: Vec<Nameset>,
}

// Riot may send `null` for any of these, which a plain `String` with `default` rejects.
#[derive(Deserialize)]
struct Nameset {
    #[serde(default)]
    puuid: Option<String>,
    #[serde(default)]
    alias: Option<Alias>,
}

#[derive(Deserialize)]
struct Alias {
    #[serde(rename = "gameName", default)]
    game_name: Option<String>,
    #[serde(rename = "tagLine", default)]
    tag_line: Option<String>,
}

/// The named entries of a namesets response; entries without a PUUID or game name are left out.
pub fn namesets(body: &str) -> Vec<(String, String, String)> {
    serde_json::from_str::<Namesets>(body)
        .map(|response| response.namesets)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|nameset| {
            let alias = nameset.alias?;
            let game_name = alias.game_name.filter(|name| !name.trim().is_empty())?;
            Some((
                nameset.puuid?,
                game_name,
                alias.tag_line.unwrap_or_default(),
            ))
        })
        .collect()
}

#[derive(Deserialize)]
struct Presences {
    #[serde(default)]
    presences: Vec<Presence>,
}

#[derive(Deserialize)]
struct Presence {
    #[serde(default)]
    puuid: String,
    #[serde(default)]
    product: String,
    #[serde(default)]
    private: Option<String>,
}

/// The score in `puuid`'s VALORANT presence. Riot has sent the scores both at the top of the
/// presence and nested under `partyPresenceData` or `matchPresenceData`.
pub fn presence_score(body: &str, puuid: &str) -> Result<MatchScore, ScoreUnavailable> {
    let presences: Presences = serde_json::from_str(body)
        .map_err(|error| ScoreUnavailable::Failed(format!("unexpected presences: {error}")))?;
    let private = presences
        .presences
        .into_iter()
        .find(|presence| {
            presence.puuid.eq_ignore_ascii_case(puuid)
                && presence.product.eq_ignore_ascii_case("valorant")
        })
        .and_then(|presence| presence.private)
        .ok_or(ScoreUnavailable::NotFound)?;
    let decoded = BASE64_STANDARD
        .decode(private.trim())
        .map_err(|error| ScoreUnavailable::Failed(format!("unexpected presence: {error}")))?;
    let private: serde_json::Value = serde_json::from_slice(&decoded)
        .map_err(|error| ScoreUnavailable::Failed(format!("unexpected presence: {error}")))?;

    [
        Some(&private),
        private.get("partyPresenceData"),
        private.get("matchPresenceData"),
    ]
    .into_iter()
    .flatten()
    .find_map(|data| {
        Some(MatchScore {
            ally: data.get("partyOwnerMatchScoreAllyTeam")?.as_i64()?,
            enemy: data.get("partyOwnerMatchScoreEnemyTeam")?.as_i64()?,
        })
    })
    .ok_or(ScoreUnavailable::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_lockfile() {
        assert_eq!(
            parse_lockfile("Riot Client:1234:51234:secret:https\n"),
            Some(LocalApiAuth {
                port: 51234,
                password: "secret".to_string(),
            })
        );
        assert_eq!(parse_lockfile("Riot Client:1234:51234"), None);
        assert_eq!(parse_lockfile("Riot Client:1234:port:secret:https"), None);
    }

    #[test]
    fn lockfile_debug_hides_the_password() {
        let auth = parse_lockfile("Riot Client:1:2:secret:https").expect("lockfile");

        assert!(!format!("{auth:?}").contains("secret"));
    }

    fn presences(puuid: &str, private: &serde_json::Value) -> String {
        serde_json::json!({
            "presences": [{
                "puuid": puuid,
                "product": "valorant",
                "private": BASE64_STANDARD.encode(private.to_string()),
            }]
        })
        .to_string()
    }

    #[test]
    fn reads_a_flat_or_nested_score_for_the_account() {
        let flat = serde_json::json!({
            "partyOwnerMatchScoreAllyTeam": 5,
            "partyOwnerMatchScoreEnemyTeam": 2
        });
        let nested = serde_json::json!({ "partyPresenceData": flat.clone() });
        let expected = Ok(MatchScore { ally: 5, enemy: 2 });

        assert_eq!(presence_score(&presences("self", &flat), "SELF"), expected);
        assert_eq!(
            presence_score(&presences("self", &nested), "self"),
            expected
        );
        assert_eq!(expected.map(MatchScore::round), Ok(8));
    }

    #[test]
    fn a_missing_presence_or_score_is_not_found() {
        let flat = serde_json::json!({
            "partyOwnerMatchScoreAllyTeam": 5,
            "partyOwnerMatchScoreEnemyTeam": 2
        });

        assert_eq!(
            presence_score(&presences("friend", &flat), "self"),
            Err(ScoreUnavailable::NotFound)
        );
        assert_eq!(
            presence_score(&presences("self", &serde_json::json!({})), "self"),
            Err(ScoreUnavailable::NotFound)
        );
    }

    #[test]
    fn reads_named_namesets() {
        // The v2 shape the Riot Client returned in a live match, plus entries without a name.
        let consoles = serde_json::json!({
            "switchNameset": {"nickname": ""},
            "xboxNameset": {"classicGamertag": "", "modernGamertag": "", "modernSuffix": ""},
            "playstationNameset": {"onlineId": ""}
        });
        let mut streamer = serde_json::json!({
            "puuid": "streamer",
            "providerId": "",
            "error": "",
            "alias": {"gameName": "Streamer", "tagLine": "TTV"}
        });
        streamer
            .as_object_mut()
            .expect("object")
            .extend(consoles.as_object().expect("object").clone());
        let body = serde_json::json!({
            "namesets": [
                streamer,
                {"puuid": "console", "error": "", "alias": {"gameName": null, "tagLine": null}},
                {"puuid": null, "alias": {"gameName": "Nobody", "tagLine": "0000"}},
                {"puuid": "blank", "alias": {"gameName": " ", "tagLine": ""}},
                {"puuid": "missing", "error": "not found", "alias": null}
            ]
        })
        .to_string();

        assert_eq!(
            namesets(&body),
            [(
                "streamer".to_string(),
                "Streamer".to_string(),
                "TTV".to_string()
            )]
        );
        assert!(namesets("not json").is_empty());
    }

    #[test]
    fn an_invalid_presence_fails() {
        let body = serde_json::json!({
            "presences": [{"puuid": "self", "product": "valorant", "private": "not base64!"}]
        })
        .to_string();

        assert!(matches!(
            presence_score(&body, "self"),
            Err(ScoreUnavailable::Failed(_))
        ));
    }
}
