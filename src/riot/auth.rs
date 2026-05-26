use std::collections::HashMap;

use thiserror::Error;
use time::OffsetDateTime;
use url::Url;

use crate::account::AuthSession;

pub const COOKIE_REAUTH_URL: &str = "https://auth.riotgames.com/authorize?redirect_uri=https%3A%2F%2Fplayvalorant.com%2Fopt_in&client_id=play-valorant-web-prod&response_type=token%20id_token&nonce=1&scope=account%20openid";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedirectTokens {
    pub access_token: String,
    pub id_token: Option<String>,
    pub token_type: String,
    pub expires_in_seconds: Option<i64>,
    pub scope: Option<String>,
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

    #[test]
    fn rejects_redirect_without_access_token() {
        let err = parse_redirect_tokens("https://playvalorant.com/opt_in#id_token=id")
            .expect_err("missing token");

        assert!(matches!(err, AuthParseError::MissingAccessToken));
    }
}
