use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;
use url::Url;

use crate::account::AuthSession;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AuthCookiesBody {
    pub client_id: &'static str,
    pub nonce: &'static str,
    pub redirect_uri: &'static str,
    pub response_type: &'static str,
    pub scope: &'static str,
}

impl Default for AuthCookiesBody {
    fn default() -> Self {
        Self {
            client_id: "play-valorant-web-prod",
            nonce: "1",
            redirect_uri: "https://playvalorant.com/opt_in",
            response_type: "token id_token",
            scope: "account openid",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AuthRequestBody {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub language: &'static str,
    pub remember: bool,
    pub riot_identity: RiotIdentity,
}

impl AuthRequestBody {
    pub fn password(
        username: impl Into<String>,
        password: impl Into<String>,
        captcha: impl Into<String>,
        remember: bool,
    ) -> Self {
        Self {
            kind: "auth",
            language: "en_US",
            remember,
            riot_identity: RiotIdentity {
                captcha: captcha.into(),
                username: username.into(),
                password: password.into(),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RiotIdentity {
    pub captcha: String,
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MfaBody {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub multifactor: MfaCode,
}

impl MfaBody {
    pub fn new(otp: impl Into<String>, remember_device: bool) -> Self {
        Self {
            kind: "multifactor",
            multifactor: MfaCode {
                otp: otp.into(),
                remember_device,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MfaCode {
    pub otp: String,
    #[serde(rename = "rememberDevice")]
    pub remember_device: bool,
}

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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct AuthSuccess {
    pub login_token: String,
    pub redirect_url: String,
    pub is_console_link_session: bool,
    pub auth_method: String,
    pub puuid: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "type")]
pub enum AuthRequestResponse {
    #[serde(rename = "success")]
    Success {
        success: AuthSuccess,
        country: String,
        platform: String,
    },
    #[serde(rename = "multifactor")]
    Multifactor {
        multifactor: MultifactorChallenge,
        country: String,
        platform: String,
        error: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct MultifactorChallenge {
    pub method: String,
    #[serde(default)]
    pub methods: Vec<String>,
    pub email: Option<String>,
    pub mode: String,
    pub auth_method: String,
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

    #[test]
    fn builds_documented_auth_cookie_body() {
        let body = serde_json::to_value(AuthCookiesBody::default()).expect("json");

        assert_eq!(body["client_id"], "play-valorant-web-prod");
        assert_eq!(body["redirect_uri"], "https://playvalorant.com/opt_in");
        assert_eq!(body["response_type"], "token id_token");
    }

    #[test]
    fn serializes_mfa_body_with_expected_field_names() {
        let body = serde_json::to_value(MfaBody::new("123456", true)).expect("json");

        assert_eq!(body["type"], "multifactor");
        assert_eq!(body["multifactor"]["otp"], "123456");
        assert_eq!(body["multifactor"]["rememberDevice"], true);
    }
}
