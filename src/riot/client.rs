use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, COOKIE, LOCATION};
use thiserror::Error;

use crate::account::Shard;

use super::auth::{AuthParseError, COOKIE_REAUTH_URL, RedirectTokens, parse_redirect_tokens};
use super::endpoints::{
    CLIENT_PLATFORM, ENTITLEMENTS_URL, HEADER_CLIENT_PLATFORM, HEADER_CLIENT_VERSION,
    HEADER_ENTITLEMENTS, PLAYER_INFO_URL, player_loadout_url, storefront_url,
};
use super::models::{
    EntitlementResponse, PlayerInfoResponse, PlayerLoadoutResponse, StorefrontResponse,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiCredentials {
    pub access_token: String,
    pub entitlements_token: String,
    pub client_version: String,
    pub shard: Shard,
    pub puuid: String,
}

impl ApiCredentials {
    pub fn validate(&self) -> Result<(), RiotApiError> {
        if self.access_token.trim().is_empty() {
            return Err(RiotApiError::MissingField("access token"));
        }

        if self.entitlements_token.trim().is_empty() {
            return Err(RiotApiError::MissingField("entitlements token"));
        }

        if self.client_version.trim().is_empty() {
            return Err(RiotApiError::MissingField("client version"));
        }

        if self.puuid.trim().is_empty() {
            return Err(RiotApiError::MissingField("PUUID"));
        }

        Ok(())
    }
}

#[derive(Clone)]
pub struct RiotApi {
    client: reqwest::Client,
    no_redirect_client: reqwest::Client,
}

impl RiotApi {
    pub fn new() -> Result<Self, RiotApiError> {
        let client = reqwest::Client::builder()
            .cookie_store(true)
            .user_agent("prime-valorant-manager/0.1")
            .build()?;
        let no_redirect_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("prime-valorant-manager/0.1")
            .build()?;

        Ok(Self {
            client,
            no_redirect_client,
        })
    }

    pub async fn cookie_reauth(&self, cookie_header: &str) -> Result<RedirectTokens, RiotApiError> {
        let response = self
            .no_redirect_client
            .get(COOKIE_REAUTH_URL)
            .header(COOKIE, cookie_header)
            .send()
            .await?;
        let location = response
            .headers()
            .get(LOCATION)
            .ok_or(RiotApiError::MissingRedirectLocation)?
            .to_str()
            .map_err(|_| RiotApiError::InvalidRedirectLocation)?
            .to_string();

        parse_redirect_tokens(&location).map_err(RiotApiError::AuthParse)
    }

    pub async fn entitlement(
        &self,
        access_token: &str,
    ) -> Result<EntitlementResponse, RiotApiError> {
        self.client
            .post(ENTITLEMENTS_URL)
            .header(CONTENT_TYPE, "application/json")
            .bearer_auth(access_token)
            .json(&serde_json::json!({}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(RiotApiError::Http)
    }

    pub async fn player_info(
        &self,
        access_token: &str,
    ) -> Result<PlayerInfoResponse, RiotApiError> {
        self.client
            .get(PLAYER_INFO_URL)
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(RiotApiError::Http)
    }

    pub async fn storefront(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<StorefrontResponse, RiotApiError> {
        credentials.validate()?;

        self.client
            .get(storefront_url(credentials.shard, &credentials.puuid))
            .headers(valorant_headers(credentials)?)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(RiotApiError::Http)
    }

    pub async fn player_loadout(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<PlayerLoadoutResponse, RiotApiError> {
        credentials.validate()?;

        self.client
            .get(player_loadout_url(credentials.shard, &credentials.puuid))
            .headers(valorant_headers(credentials)?)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(RiotApiError::Http)
    }
}

pub fn valorant_headers(
    credentials: &ApiCredentials,
) -> Result<reqwest::header::HeaderMap, RiotApiError> {
    credentials.validate()?;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        format!("Bearer {}", credentials.access_token).parse()?,
    );
    headers.insert(HEADER_CLIENT_PLATFORM, CLIENT_PLATFORM.parse()?);
    headers.insert(HEADER_CLIENT_VERSION, credentials.client_version.parse()?);
    headers.insert(HEADER_ENTITLEMENTS, credentials.entitlements_token.parse()?);

    Ok(headers)
}

#[derive(Debug, Error)]
pub enum RiotApiError {
    #[error("missing required Riot API field: {0}")]
    MissingField(&'static str),
    #[error("invalid header value: {0}")]
    Header(#[from] reqwest::header::InvalidHeaderValue),
    #[error("cookie reauth response did not include a redirect location")]
    MissingRedirectLocation,
    #[error("cookie reauth redirect location header was not valid UTF-8")]
    InvalidRedirectLocation,
    #[error("cookie reauth redirect did not contain Riot tokens: {0}")]
    AuthParse(#[from] AuthParseError),
    #[error("Riot API HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credentials() -> ApiCredentials {
        ApiCredentials {
            access_token: "access".to_string(),
            entitlements_token: "entitlement".to_string(),
            client_version: "release-10.00-shipping-1-123456".to_string(),
            shard: Shard::Na,
            puuid: "puuid".to_string(),
        }
    }

    #[test]
    fn validation_rejects_missing_client_version() {
        let mut credentials = credentials();
        credentials.client_version.clear();

        let err = credentials.validate().expect_err("missing version");

        assert!(matches!(err, RiotApiError::MissingField("client version")));
    }

    #[test]
    fn valorant_headers_include_required_client_headers() {
        let headers = valorant_headers(&credentials()).expect("headers");

        assert_eq!(headers[HEADER_CLIENT_PLATFORM], CLIENT_PLATFORM);
        assert_eq!(
            headers[HEADER_CLIENT_VERSION],
            "release-10.00-shipping-1-123456"
        );
        assert_eq!(headers[HEADER_ENTITLEMENTS], "entitlement");
        assert_eq!(headers[AUTHORIZATION], "Bearer access");
    }
}
