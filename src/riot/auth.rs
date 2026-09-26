use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use thiserror::Error;
use time::OffsetDateTime;
use url::Url;

use crate::account::AuthSession;

#[derive(Clone, Eq, PartialEq)]
pub struct RedirectTokens {
    pub access_token: String,
    pub id_token: Option<String>,
    pub token_type: String,
    pub expires_in_seconds: Option<i64>,
    pub scope: Option<String>,
}

impl std::fmt::Debug for RedirectTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedirectTokens")
            .field("access_token", &"<redacted>")
            .field("id_token", &self.id_token.as_ref().map(|_| "<redacted>"))
            .field("token_type", &self.token_type)
            .field("expires_in_seconds", &self.expires_in_seconds)
            .field("scope", &self.scope)
            .finish()
    }
}

impl RedirectTokens {
    pub fn into_session(self) -> AuthSession {
        AuthSession::new(
            self.access_token,
            self.id_token,
            None,
            self.token_type,
            self.expires_in_seconds,
            OffsetDateTime::now_utc().unix_timestamp(),
        )
    }

    /// The Riot account (PUUID) the tokens were issued for, read locally from the JWT claims.
    pub fn subject(&self) -> Option<String> {
        jwt_subject(&self.access_token).or_else(|| self.id_token.as_deref().and_then(jwt_subject))
    }
}

/// Reads the `sub` claim of a JWT without verifying it. Riot tokens use the PUUID as the subject.
pub fn jwt_subject(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let claims = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&claims).ok()?;
    let subject = claims.get("sub")?.as_str()?.trim();

    (!subject.is_empty()).then(|| subject.to_string())
}

pub fn parse_redirect_tokens(redirect_url: &str) -> Result<RedirectTokens, AuthParseError> {
    let url = Url::parse(redirect_url).map_err(AuthParseError::InvalidUrl)?;
    let values = parse_pairs(url.fragment().unwrap_or_default())
        .into_iter()
        .chain(parse_pairs(url.query().unwrap_or_default()))
        .collect::<HashMap<_, _>>();

    let access_token = values
        .get("access_token")
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or(AuthParseError::MissingAccessToken)?;

    let expires_in_seconds = values
        .get("expires_in")
        .map(|value| value.parse::<i64>())
        .transpose()
        .map_err(|_| AuthParseError::InvalidExpiresIn)?;

    Ok(RedirectTokens {
        access_token,
        id_token: values
            .get("id_token")
            .filter(|value| !value.is_empty())
            .cloned(),
        token_type: values
            .get("token_type")
            .filter(|value| !value.is_empty())
            .cloned()
            .unwrap_or_else(|| "Bearer".to_string()),
        expires_in_seconds,
        scope: values
            .get("scope")
            .filter(|value| !value.is_empty())
            .cloned(),
    })
}

fn parse_pairs(input: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(input.as_bytes())
        .into_owned()
        .collect()
}

#[derive(Debug, Error)]
pub enum AuthParseError {
    #[error("invalid redirect URL: {0}")]
    InvalidUrl(url::ParseError),
    #[error("redirect URL did not include an access_token")]
    MissingAccessToken,
    #[error("redirect URL expires_in value was not an integer")]
    InvalidExpiresIn,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tokens_from_redirect_fragment() {
        let tokens = parse_redirect_tokens(
            "https://playvalorant.com/opt_in#access_token=abc&id_token=id&expires_in=3600&token_type=Bearer&scope=account%20openid",
        )
        .expect("tokens");

        assert_eq!(tokens.access_token, "abc");
        assert_eq!(tokens.id_token.as_deref(), Some("id"));
        assert_eq!(tokens.expires_in_seconds, Some(3600));
        assert_eq!(tokens.scope.as_deref(), Some("account openid"));
    }

    fn jwt_with_subject(subject: &str) -> String {
        let claims = URL_SAFE_NO_PAD.encode(format!(r#"{{"sub":"{subject}"}}"#));
        format!("header.{claims}.signature")
    }

    #[test]
    fn reads_the_riot_account_from_a_token() {
        assert_eq!(
            jwt_subject(&jwt_with_subject("puuid-a")).as_deref(),
            Some("puuid-a")
        );
        assert_eq!(jwt_subject("not-a-jwt"), None);
    }

    #[test]
    fn redirect_tokens_name_their_riot_account() {
        let access_token = jwt_with_subject("puuid-a");
        let tokens = parse_redirect_tokens(&format!(
            "https://playvalorant.com/opt_in#access_token={access_token}"
        ))
        .expect("tokens");

        assert_eq!(tokens.subject().as_deref(), Some("puuid-a"));
    }

    #[test]
    fn redirect_tokens_fall_back_to_the_id_token_subject() {
        let id_token = jwt_with_subject("puuid-b");
        let tokens = parse_redirect_tokens(&format!(
            "https://playvalorant.com/opt_in#access_token=opaque&id_token={id_token}"
        ))
        .expect("tokens");

        assert_eq!(tokens.subject().as_deref(), Some("puuid-b"));
    }

    #[test]
    fn rejects_redirect_without_access_token() {
        let err = parse_redirect_tokens("https://playvalorant.com/opt_in#id_token=id")
            .expect_err("missing token");

        assert!(matches!(err, AuthParseError::MissingAccessToken));
    }
}
