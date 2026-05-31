use reqwest::StatusCode;
use reqwest::header::{ACCEPT, AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, COOKIE, USER_AGENT};
use serde::Deserialize;
use thiserror::Error;

use crate::account::{Shard, ValorantRegion};

const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
const USER_AGENT_VALUE: &str = concat!("prime/", env!("CARGO_PKG_VERSION"));
const RIOT_CLIENT_AUTHORIZATION_URL: &str = "https://auth.riotgames.com/api/v1/authorization";
const RIOT_CLIENT_REAUTH_USER_AGENT: &str =
    "RiotGamesApi/24.3.0.3124 rso-auth (Windows;10;;Home, x64) riot_client/0";

use super::auth::{AuthParseError, RedirectTokens, parse_redirect_tokens};
use super::endpoints::{
    CLIENT_PLATFORM, ENTITLEMENTS_URL, HEADER_CLIENT_PLATFORM, HEADER_CLIENT_VERSION,
    HEADER_ENTITLEMENTS, PLAYER_INFO_URL, RIOT_GEO_URL, account_xp_url, content_url, contracts_url,
    current_game_player_url, party_player_url, player_loadout_url, player_mmr_url,
    player_penalties_url, pregame_player_url, storefront_url, wallet_url,
};
use super::models::{
    AccountXpResponse, ContractsResponse, EntitlementResponse, GameContentResponse,
    PlayerInfoResponse, PlayerLoadoutResponse, PlayerMmrResponse, PlayerPenaltiesResponse,
    RiotGeoResponse, StorefrontResponse, WalletResponse,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiCredentials {
    pub access_token: String,
    pub entitlements_token: String,
    pub client_version: String,
    pub shard: Shard,
    pub puuid: String,
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

impl RiotApi {
    pub fn new() -> Result<Self, RiotApiError> {
        let client = reqwest::Client::builder()
            .cookie_store(true)
            .timeout(HTTP_TIMEOUT)
            .user_agent(USER_AGENT_VALUE)
            .build()?;

        Ok(Self { client })
    }

    pub async fn launcher_reauth(
        &self,
        cookie_header: &str,
    ) -> Result<RedirectTokens, RiotApiError> {
        let response = self
            .client
            .post(RIOT_CLIENT_AUTHORIZATION_URL)
            .header(ACCEPT, "application/json")
            .header(CACHE_CONTROL, "no-cache")
            .header(CONTENT_TYPE, "application/json")
            .header(COOKIE, cookie_header)
            .header(USER_AGENT, RIOT_CLIENT_REAUTH_USER_AGENT)
            .json(&serde_json::json!({
                "acr_values": "",
                "claims": "",
                "client_id": "riot-client",
                "code_challenge": "",
                "code_challenge_method": "",
                "nonce": "1",
                "redirect_uri": "http://localhost/redirect",
                "response_type": "token id_token",
                "scope": "openid lol lol_region link ban account offline_access",
            }))
            .send()
            .await?
            .error_for_status()?;
        let body = response.text().await?;

        parse_riot_client_authorization_tokens(&body)
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

    pub async fn storefront(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<StorefrontResponse, RiotApiError> {
        credentials.validate()?;

        self.client
            .post(storefront_url(credentials.shard, &credentials.puuid))
            .headers(valorant_headers(credentials)?)
            .json(&serde_json::json!({}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(RiotApiError::Http)
    }

    pub async fn wallet(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<WalletResponse, RiotApiError> {
        credentials.validate()?;

        self.client
            .get(wallet_url(credentials.shard, &credentials.puuid))
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

    pub async fn account_xp(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<AccountXpResponse, RiotApiError> {
        credentials.validate()?;

        self.client
            .get(account_xp_url(credentials.shard, &credentials.puuid))
            .headers(valorant_headers(credentials)?)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(RiotApiError::Http)
    }

    pub async fn player_mmr(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<PlayerMmrResponse, RiotApiError> {
        credentials.validate()?;

        self.client
            .get(player_mmr_url(credentials.shard, &credentials.puuid))
            .headers(valorant_headers(credentials)?)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(RiotApiError::Http)
    }

    pub async fn player_penalties(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<PlayerPenaltiesResponse, RiotApiError> {
        credentials.validate()?;

        self.client
            .get(player_penalties_url(credentials.shard))
            .headers(valorant_headers(credentials)?)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(RiotApiError::Http)
    }

    pub async fn contracts(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<ContractsResponse, RiotApiError> {
        credentials.validate()?;

        self.client
            .get(contracts_url(credentials.shard, &credentials.puuid))
            .headers(valorant_headers(credentials)?)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(RiotApiError::Http)
    }

    pub async fn game_content(
        &self,
        credentials: &ApiCredentials,
    ) -> Result<GameContentResponse, RiotApiError> {
        credentials.validate()?;

        self.client
            .get(content_url(credentials.shard))
            .headers(valorant_headers(credentials)?)
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

#[derive(Debug, Deserialize)]
struct RiotClientAuthorizationResponse {
    response: Option<RiotClientAuthorizationResult>,
}

#[derive(Debug, Deserialize)]
struct RiotClientAuthorizationResult {
    parameters: Option<RiotClientAuthorizationParameters>,
}

#[derive(Debug, Deserialize)]
struct RiotClientAuthorizationParameters {
    uri: Option<String>,
}

fn parse_riot_client_authorization_tokens(body: &str) -> Result<RedirectTokens, RiotApiError> {
    let response: RiotClientAuthorizationResponse = serde_json::from_str(body)?;
    let redirect_uri = response
        .response
        .and_then(|response| response.parameters)
        .and_then(|parameters| parameters.uri)
        .filter(|uri| !uri.trim().is_empty())
        .ok_or(RiotApiError::RiotClientReauthRejected)?;

    parse_redirect_tokens(&redirect_uri).map_err(RiotApiError::AuthParse)
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
    #[error("captured Riot Client cookies were not accepted; recapture the Riot Client session")]
    RiotClientReauthRejected,
    #[error("Riot Client authorization response was not valid JSON: {0}")]
    AuthResponseJson(#[from] serde_json::Error),
    #[error("Riot authorization redirect did not contain Riot tokens: {0}")]
    AuthParse(#[from] AuthParseError),
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
    fn parses_riot_client_authorization_response_uri() {
        let tokens = parse_riot_client_authorization_tokens(
            r#"{
                "type": "response",
                "response": {
                    "mode": "fragment",
                    "parameters": {
                        "uri": "http://localhost/redirect#access_token=access&id_token=id&expires_in=3600&token_type=Bearer&scope=openid%20account"
                    }
                }
            }"#,
        )
        .expect("tokens");

        assert_eq!(tokens.access_token, "access");
        assert_eq!(tokens.id_token.as_deref(), Some("id"));
        assert_eq!(tokens.expires_in_seconds, Some(3600));
        assert_eq!(tokens.scope.as_deref(), Some("openid account"));
    }

    #[test]
    fn rejects_riot_client_authorization_response_without_uri() {
        let err =
            parse_riot_client_authorization_tokens(r#"{"type":"auth"}"#).expect_err("missing uri");

        assert!(matches!(err, RiotApiError::RiotClientReauthRejected));
    }
}
