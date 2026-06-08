use super::*;

use super::launch_flow::{resolve_session_region, resolve_session_shard};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ApiIdentity {
    pub(in crate::ui) puuid: String,
    pub(in crate::ui) game_name: Option<String>,
    pub(in crate::ui) tag_line: Option<String>,
    pub(in crate::ui) shard: Shard,
}

pub(in crate::ui) async fn resolve_credentials(
    api: &RiotApi,
    account: &AccountProfile,
    client_version: String,
) -> Result<ResolvedApiCredentials, String> {
    let api_session = active_api_session(api, account).await?;
    let mut session = api_session.session;
    let player_info = api.player_info(&session.access_token).await.ok();

    let entitlements_token = entitlement_token(api, &session).await?;
    if session
        .entitlements_token
        .as_ref()
        .is_none_or(|token| token.trim().is_empty())
    {
        session.entitlements_token = Some(entitlements_token.clone());
    }

    let puuid = player_info
        .as_ref()
        .map(|info| info.sub.clone())
        .or_else(|| {
            account
                .puuid
                .clone()
                .filter(|puuid| !puuid.trim().is_empty())
        })
        .or_else(|| {
            account
                .launcher_session
                .as_ref()
                .map(|backup| backup.puuid.clone())
                .filter(|puuid| !puuid.trim().is_empty())
        })
        .ok_or_else(|| "selected account does not have a Riot PUUID".to_string())?;
    let region = resolve_session_region(api, &session, player_info.as_ref())
        .await
        .ok();
    let shard = match region {
        Some(region) => region.shard(),
        None => resolve_session_shard(api, &session, player_info.as_ref(), account.shard).await,
    };
    let identity = match player_info {
        Some(info) => ApiIdentity {
            puuid: puuid.clone(),
            game_name: Some(info.acct.game_name),
            tag_line: Some(info.acct.tag_line),
            shard,
        },
        None => ApiIdentity {
            puuid: puuid.clone(),
            game_name: None,
            tag_line: None,
            shard,
        },
    };

    Ok(ResolvedApiCredentials {
        credentials: ApiCredentials {
            access_token: session.access_token.clone(),
            entitlements_token,
            client_version,
            shard,
            puuid,
        },
        region,
        session,
        launcher_session: api_session.launcher_session,
        identity,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ResolvedApiCredentials {
    pub(in crate::ui) credentials: ApiCredentials,
    pub(in crate::ui) region: Option<ValorantRegion>,
    pub(in crate::ui) session: AuthSession,
    pub(in crate::ui) launcher_session: Option<LauncherSessionBackup>,
    pub(in crate::ui) identity: ApiIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ApiSession {
    pub(in crate::ui) session: AuthSession,
    pub(in crate::ui) launcher_session: Option<LauncherSessionBackup>,
}

pub(in crate::ui) async fn active_api_session(
    api: &RiotApi,
    account: &AccountProfile,
) -> Result<ApiSession, String> {
    if let Some(session) = &account.session
        && !session.is_expired()
    {
        return Ok(ApiSession {
            session: session.clone(),
            launcher_session: None,
        });
    }

    let Some(backup) = &account.launcher_session else {
        return Err(
            "selected account needs an imported Riot token or a captured launcher session"
                .to_string(),
        );
    };

    launcher_api_session(api, backup).await
}

pub(in crate::ui) async fn refreshed_api_session(
    api: &RiotApi,
    account: &AccountProfile,
) -> Result<ApiSession, String> {
    if let Some(backup) = &account.launcher_session {
        return launcher_api_session(api, backup).await;
    }

    active_api_session(api, account).await
}

async fn launcher_api_session(
    api: &RiotApi,
    backup: &LauncherSessionBackup,
) -> Result<ApiSession, String> {
    if !backup.is_ready() {
        return Err(
            "selected account launcher session is incomplete, missing Riot private settings, or its backup folder is missing; re-capture selected login"
                .to_string(),
        );
    }

    let cookies = read_backup_cookies(backup).map_err(|error| error.to_string())?;
    let cookie_header = launcher_cookie_header(&cookies).map_err(|error| error.to_string())?;
    let reauth = api.launcher_reauth(&cookie_header).await.map_err(|error| {
            format!(
                "launcher session reauth failed: {error}. Recapture the Riot Client session or import a fresh redirect token."
            )
        })?;

    persist_launcher_reauth_session(backup, reauth)
}

pub(in crate::ui::data) fn persist_launcher_reauth_session(
    backup: &LauncherSessionBackup,
    reauth: LauncherReauth,
) -> Result<ApiSession, String> {
    persist_refreshed_launcher_cookies(backup, &reauth.refreshed_cookies).map_err(|error| {
        format!("launcher session reauth succeeded, but Prime could not save refreshed launcher cookies: {error}")
    })?;

    Ok(ApiSession {
        session: reauth.tokens.into_session(),
        launcher_session: Some(LauncherSessionBackup {
            data_dir: backup.data_dir.clone(),
            captured_at_unix: OffsetDateTime::now_utc().unix_timestamp(),
            puuid: backup.puuid.clone(),
        }),
    })
}

pub(in crate::ui) async fn entitlement_token(
    api: &RiotApi,
    session: &AuthSession,
) -> Result<String, String> {
    if let Some(token) = &session.entitlements_token
        && !token.trim().is_empty()
    {
        return Ok(token.clone());
    }

    api.entitlement(&session.access_token)
        .await
        .map(|response| response.entitlements_token)
        .map_err(|error| error.to_string())
}
