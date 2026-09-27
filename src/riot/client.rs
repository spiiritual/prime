use reqwest::StatusCode;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, de::DeserializeOwned};
use thiserror::Error;

use crate::account::{Shard, ValorantRegion};

const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
const USER_AGENT_VALUE: &str = concat!("prime/", env!("CARGO_PKG_VERSION"));
const RIOT_TOKEN_URL: &str = "https://auth.riotgames.com/token";
const RIOT_CLIENT_USER_AGENT: &str =
    "RiotClient/139.0.8.4969 rso-auth (Windows;10;;Professional, x64)";

use super::auth::RedirectTokens;
use super::endpoints::{
    CLIENT_PLATFORM, ENTITLEMENTS_URL, HEADER_CLIENT_PLATFORM, HEADER_CLIENT_VERSION,
    HEADER_ENTITLEMENTS, PLAYER_INFO_URL, RIOT_GEO_URL, account_xp_url, content_url, contracts_url,
    current_game_player_url, party_player_url, player_loadout_url, player_mmr_url,
    player_penalties_url, player_preference_get_url, player_preference_save_url,
    pregame_player_url, storefront_url, wallet_url,
};
use super::models::{
    AccountXpResponse, ContractsResponse, EntitlementResponse, GameContentResponse,
    PlayerInfoResponse, PlayerLoadoutResponse, PlayerMmrResponse, PlayerPenaltiesResponse,
    RiotGeoResponse, StorefrontResponse, WalletResponse,
};

#[derive(Clone, Eq, PartialEq)]
pub struct ApiCredentials {
    pub access_token: String,
    pub entitlements_token: String,
    pub client_version: String,
    pub shard: Shard,
    pub puuid: String,
}

impl std::fmt::Debug for ApiCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiCredentials")
            .field("access_token", &"<redacted>")
            .field("entitlements_token", &"<redacted>")
            .field("client_version", &self.client_version)
            .field("shard", &self.shard)
            .field("puuid", &self.puuid)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerActivityEndpointPresence {
    Present,
    Missing,
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
}

#[derive(Clone, Eq, PartialEq)]
pub struct RefreshTokenReauth {
    pub tokens: RedirectTokens,
    /// A replacement refresh token, when Riot rotates it during the exchange.
    pub refresh_token: Option<String>,
}

impl std::fmt::Debug for RefreshTokenReauth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshTokenReauth")
            .field("tokens", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

impl RiotApi {
    pub fn new() -> Result<Self, RiotApiError> {
        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent(USER_AGENT_VALUE)
            .build()?;

        Ok(Self { client })
    }

    /// Exchanges the remembered Riot Client refresh token for fresh access and ID tokens.
    pub async fn refresh_token_reauth(
        &self,
        refresh_token: &str,
    ) -> Result<RefreshTokenReauth, RiotApiError> {
        let response = self
            .client
            .post(RIOT_TOKEN_URL)
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(USER_AGENT, RIOT_CLIENT_USER_AGENT)
            .body(refresh_token_request_body(refresh_token))
            .send()
            .await?;

        if matches!(
            response.status(),
            StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED
        ) {
            let body = response.text().await.unwrap_or_default();
            return Err(RiotApiError::RefreshTokenRejected(
                refresh_token_error_code(&body),
            ));
        }

        let body = response.error_for_status()?.text().await?;
        parse_refresh_token_response(&body)
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

    pub async fn riot_geo(
        &self,
        access_token: &str,
        id_token: &str,
    ) -> Result<RiotGeoResponse, RiotApiError> {
        self.client
            .put(RIOT_GEO_URL)
            .bearer_auth(access_token)
            .json(&serde_json::json!({ "id_token": id_token }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(RiotApiError::Http)
    }

    pub async fn get_player_preference(
        &self,
        base_url: &str,
        access_token: &str,
        preference_type: &str,
    ) -> Result<serde_json::Value, RiotApiError> {
        if access_token.trim().is_empty() {
            return Err(RiotApiError::MissingField("access token"));
        }

        self.client
            .get(player_preference_get_url(base_url, preference_type))
            .header(ACCEPT, "application/json")
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(RiotApiError::Http)
    }

    pub async fn save_player_preference(
        &self,
        base_url: &str,
        access_token: &str,
        body: &serde_json::Value,
    ) -> Result<(), RiotApiError> {
        if access_token.trim().is_empty() {
            return Err(RiotApiError::MissingField("access token"));
        }

        self.client
            .put(player_preference_save_url(base_url))
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .bearer_auth(access_token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    pub async fn storefront(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<StorefrontResponse, RiotApiError> {
        self.post_valorant_json(
            storefront_url(credentials.shard, &credentials.puuid),
            credentials,
        )
        .await
    }

    pub async fn wallet(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<WalletResponse, RiotApiError> {
        self.get_valorant_json(
            wallet_url(credentials.shard, &credentials.puuid),
            credentials,
        )
        .await
    }

    pub async fn player_loadout(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<PlayerLoadoutResponse, RiotApiError> {
        self.get_valorant_json(
            player_loadout_url(credentials.shard, &credentials.puuid),
            credentials,
        )
        .await
    }

    pub async fn account_xp(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<AccountXpResponse, RiotApiError> {
        self.get_valorant_json(
            account_xp_url(credentials.shard, &credentials.puuid),
            credentials,
        )
        .await
    }

    pub async fn player_mmr(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<PlayerMmrResponse, RiotApiError> {
        self.get_valorant_json(
            player_mmr_url(credentials.shard, &credentials.puuid),
            credentials,
        )
        .await
    }

    pub async fn player_penalties(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<PlayerPenaltiesResponse, RiotApiError> {
        self.get_valorant_json(player_penalties_url(credentials.shard), credentials)
            .await
    }

    pub async fn contracts(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<ContractsResponse, RiotApiError> {
        self.get_valorant_json(
            contracts_url(credentials.shard, &credentials.puuid),
            credentials,
        )
        .await
    }

    pub async fn game_content(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<GameContentResponse, RiotApiError> {
        self.get_valorant_json(content_url(credentials.shard), credentials)
            .await
    }

    async fn get_valorant_json<T>(
        &self,
        url: String,
        credentials: &ApiCredentials,
    ) -> Result<T, RiotApiError>
    where
        T: DeserializeOwned,
    {
        credentials.validate()?;

        self.client
            .get(url)
            .headers(valorant_headers(credentials)?)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(RiotApiError::Http)
    }

    async fn post_valorant_json<T>(
        &self,
        url: String,
        credentials: &ApiCredentials,
    ) -> Result<T, RiotApiError>
    where
        T: DeserializeOwned,
    {
        credentials.validate()?;

        self.client
            .post(url)
            .headers(valorant_headers(credentials)?)
            .json(&serde_json::json!({}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(RiotApiError::Http)
    }

    pub async fn current_game_player(
        &self,
        credentials: &ApiCredentials,
        region: ValorantRegion,
    ) -> Result<PlayerActivityEndpointPresence, RiotApiError> {
        self.player_activity_presence(
            current_game_player_url(region, credentials.shard, &credentials.puuid),
            credentials,
        )
        .await
    }

    pub async fn pregame_player(
        &self,
        credentials: &ApiCredentials,
        region: ValorantRegion,
    ) -> Result<PlayerActivityEndpointPresence, RiotApiError> {
        self.player_activity_presence(
            pregame_player_url(region, credentials.shard, &credentials.puuid),
            credentials,
        )
        .await
    }

    pub async fn party_player(
        &self,
        credentials: &ApiCredentials,
        region: ValorantRegion,
    ) -> Result<PlayerActivityEndpointPresence, RiotApiError> {
        self.player_activity_presence(
            party_player_url(region, credentials.shard, &credentials.puuid),
            credentials,
        )
        .await
    }

    async fn player_activity_presence(
        &self,
        url: String,
        credentials: &ApiCredentials,
    ) -> Result<PlayerActivityEndpointPresence, RiotApiError> {
        let response = self
            .client
            .get(url)
            .headers(valorant_headers(credentials)?)
            .send()
            .await?;
        let status = response.status();

        if status == StatusCode::NOT_FOUND {
            return Ok(PlayerActivityEndpointPresence::Missing);
        }

        response.error_for_status()?;
        Ok(PlayerActivityEndpointPresence::Present)
    }
}

fn refresh_token_request_body(refresh_token: &str) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "refresh_token")
        .append_pair("refresh_token", refresh_token)
        .append_pair("client_id", "riot-client")
        .finish()
}

#[derive(Deserialize)]
struct RiotTokenResponse {
    access_token: Option<String>,
    id_token: Option<String>,
    token_type: Option<String>,
    expires_in: Option<i64>,
    scope: Option<String>,
    refresh_token: Option<String>,
}

/// Riot's OAuth error code from a rejected token request, such as `invalid_grant`.
fn refresh_token_error_code(body: &str) -> String {
    #[derive(Deserialize)]
    struct TokenError {
        error: Option<String>,
    }

    serde_json::from_str::<TokenError>(body)
        .ok()
        .and_then(|response| response.error)
        .map(|code| code.trim().to_string())
        .filter(|code| !code.is_empty())
        .unwrap_or_else(|| "no reason given".to_string())
}

fn parse_refresh_token_response(body: &str) -> Result<RefreshTokenReauth, RiotApiError> {
    let response: RiotTokenResponse = serde_json::from_str(body)?;
    let access_token = response
        .access_token
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| RiotApiError::RefreshTokenRejected(refresh_token_error_code(body)))?;
    let non_empty = |value: Option<String>| value.filter(|value| !value.trim().is_empty());

    Ok(RefreshTokenReauth {
        tokens: RedirectTokens {
            access_token,
            id_token: non_empty(response.id_token),
            token_type: non_empty(response.token_type).unwrap_or_else(|| "Bearer".to_string()),
            expires_in_seconds: response.expires_in,
            scope: non_empty(response.scope),
        },
        refresh_token: non_empty(response.refresh_token),
    })
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
    /// Riot refused the saved login's refresh token, with its OAuth error code. Riot does this
    /// when the account is signed out somewhere else, its password changes or the login expires.
    #[error(
        "Riot signed out this account's saved login ({0}). This happens when the account is \
         signed out somewhere else, its password changes or the login expires; re-capture this \
         account's login"
    )]
    RefreshTokenRejected(String),
    #[error("Riot token response was not valid JSON: {0}")]
    AuthResponseJson(#[from] serde_json::Error),
    #[error(
        "Riot API HTTP error: {}",
        crate::http_error::format_reqwest_error(.0)
    )]
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

    #[test]
    fn parses_refresh_token_response() {
        let reauth = parse_refresh_token_response(
            r#"{
                "access_token": "access",
                "expires_in": 3600,
                "id_token": "id",
                "prm_hints": {},
                "refresh_token": "rotated-refresh",
                "scope": "openid account",
                "token_type": "Bearer"
            }"#,
        )
        .expect("tokens");

        assert_eq!(reauth.tokens.access_token, "access");
        assert_eq!(reauth.tokens.id_token.as_deref(), Some("id"));
        assert_eq!(reauth.tokens.token_type, "Bearer");
        assert_eq!(reauth.tokens.expires_in_seconds, Some(3600));
        assert_eq!(reauth.tokens.scope.as_deref(), Some("openid account"));
        assert_eq!(reauth.refresh_token.as_deref(), Some("rotated-refresh"));
    }

    #[test]
    fn rejects_refresh_token_response_without_access_token() {
        let err = parse_refresh_token_response(r#"{"error":"invalid_grant"}"#)
            .expect_err("missing access token");

        assert!(matches!(err, RiotApiError::RefreshTokenRejected(code) if code == "invalid_grant"));
    }

    #[test]
    fn refresh_token_error_code_reads_riots_reason() {
        assert_eq!(
            refresh_token_error_code(r#"{"error":"invalid_grant","error_description":""}"#),
            "invalid_grant"
        );
        assert_eq!(refresh_token_error_code("not json"), "no reason given");
        assert_eq!(
            refresh_token_error_code(r#"{"error":""}"#),
            "no reason given"
        );
    }

    #[test]
    fn refresh_token_request_body_is_form_encoded() {
        assert_eq!(
            refresh_token_request_body("a+b/c="),
            "grant_type=refresh_token&refresh_token=a%2Bb%2Fc%3D&client_id=riot-client"
        );
    }
}
