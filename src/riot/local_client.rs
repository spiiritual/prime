//! The Riot Client's local API on this PC, read only for a live match's score. No Riot server
//! reports the score; only the chat presence the Riot Client shares with friends has it.

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

/// The score of the match `puuid` is playing, from the local Riot Client's chat presences.
pub async fn match_score(puuid: &str) -> Result<MatchScore, ScoreUnavailable> {
    let auth = lockfile_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|contents| parse_lockfile(&contents))
        .ok_or(ScoreUnavailable::RiotClientNotRunning)?;
    let failed = |error: reqwest::Error| {
        ScoreUnavailable::Failed(crate::http_error::format_reqwest_error(&error))
    };
    // The Riot Client serves its local API with a self-signed certificate, and this client only
    // ever talks to 127.0.0.1.
    let client = reqwest::Client::builder()
        .timeout(LOCAL_TIMEOUT)
        .tls_danger_accept_invalid_certs(true)
        .build()
        .map_err(failed)?;
    let body = client
        .get(format!("https://127.0.0.1:{}/chat/v4/presences", auth.port))
        .basic_auth("riot", Some(&auth.password))
        .send()
        .await
        .map_err(failed)?
        .error_for_status()
        .map_err(failed)?
        .text()
        .await
        .map_err(failed)?;

    presence_score(&body, puuid)
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
