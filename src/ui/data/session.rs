use super::*;

use super::launch_flow::{resolve_session_region, resolve_session_shard};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ApiIdentity {
    pub(in crate::ui) puuid: String,
    pub(in crate::ui) game_name: Option<String>,
    pub(in crate::ui) tag_line: Option<String>,
    pub(in crate::ui) shard: Shard,
    pub(in crate::ui) region: Option<ValorantRegion>,
}

pub(in crate::ui) async fn resolve_credentials(
    api: &RiotApi,
    account: &AccountProfile,
    client_version: String,
) -> Result<ResolvedApiCredentials, String> {
    let api_session = active_api_session(api, account).await?;
    let mut session = api_session.session;
    let token_subject = crate::riot::auth::jwt_subject(&session.access_token);
    let player_info = if needs_player_info(token_subject.is_some(), account.region, &session) {
        api.player_info(&session.access_token).await.ok()
    } else {
        None
    };

    let entitlements_token = entitlement_token(api, &session).await?;
    if session
        .entitlements_token
        .as_ref()
        .is_none_or(|token| token.trim().is_empty())
    {
        session.entitlements_token = Some(entitlements_token.clone());
    }

    let mut identity = api_identity(
        account,
        player_info.as_ref(),
        token_subject.as_deref(),
        account.shard,
    )?;
    let looked_up_region = match account.region {
        Some(_) => None,
        None => resolve_session_region(api, &session, player_info.as_ref())
            .await
            .ok(),
    };
    let region = account.region.or(looked_up_region);
    // Only a region looked up now is saved, so a request that started before Refresh cleared
    // the saved region can't write the old one back.
    identity.region = looked_up_region;
    identity.shard = match region {
        Some(region) => region.shard(),
        None => resolve_session_shard(api, &session, player_info.as_ref(), account.shard).await,
    };

    Ok(ResolvedApiCredentials {
        credentials: ApiCredentials {
            access_token: session.access_token.clone(),
            entitlements_token,
            client_version,
            shard: identity.shard,
            puuid: identity.puuid.clone(),
        },
        region,
        session,
        launcher_session: api_session.launcher_session,
        identity,
    })
}

/// Whether to ask userinfo: for the PUUID when the token doesn't name its account, or for the
/// region when none is saved and Riot Geo can't be asked because there is no ID token.
pub(in crate::ui) fn needs_player_info(
    token_has_subject: bool,
    saved_region: Option<ValorantRegion>,
    session: &AuthSession,
) -> bool {
    let has_id_token = session
        .id_token
        .as_ref()
        .is_some_and(|token| !token.trim().is_empty());
    !token_has_subject || (saved_region.is_none() && !has_id_token)
}

/// The Riot account an API session acts as. Fails when the session belongs to a different Riot
/// account than the profile, so nothing is read or written under the wrong profile.
pub(in crate::ui) fn api_identity(
    account: &AccountProfile,
    player_info: Option<&PlayerInfoResponse>,
    token_subject: Option<&str>,
    shard: Shard,
) -> Result<ApiIdentity, String> {
    let puuid = player_info
        .map(|info| info.sub.trim().to_string())
        .or_else(|| token_subject.map(|subject| subject.trim().to_string()))
        .or_else(|| account.puuid.clone())
        .or_else(|| {
            account
                .launcher_session
                .as_ref()
                .map(|backup| backup.puuid.clone())
        })
        .filter(|puuid| !puuid.trim().is_empty())
        .ok_or_else(|| "selected account does not have a Riot PUUID".to_string())?;
    account
        .check_puuid(&puuid)
        .map_err(|error| error.to_string())?;

    Ok(ApiIdentity {
        puuid,
        game_name: player_info.map(|info| info.acct.game_name.clone()),
        tag_line: player_info.map(|info| info.acct.tag_line.clone()),
        shard,
        region: None,
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
    // A stored token issued for another Riot account is skipped rather than used as this one.
    if let Some(session) = &account.session
        && !session.is_expired()
        && crate::riot::auth::jwt_subject(&session.access_token)
            .is_none_or(|subject| account.check_puuid(&subject).is_ok())
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

    reauth_launcher_backup(api, backup).await
}

/// Exchanges a captured backup's remembered Riot Client refresh token for an API session.
pub(in crate::ui::data) async fn reauth_launcher_backup(
    api: &RiotApi,
    backup: &LauncherSessionBackup,
) -> Result<ApiSession, String> {
    let refresh_token = read_backup_refresh_token(backup).map_err(|error| error.to_string())?;
    let reauth = api
        .refresh_token_reauth(&refresh_token)
        .await
        .map_err(|error| match error {
            // Already says what happened and what to do.
            crate::riot::client::RiotApiError::RefreshTokenRejected(_) => error.to_string(),
            error => format!(
                "launcher session reauth failed: {error}. Recapture the Riot Client session or import a fresh redirect token."
            ),
        })?;

    if let Some(rotated) = reauth
        .refresh_token
        .as_deref()
        .filter(|rotated| *rotated != refresh_token)
    {
        persist_refreshed_refresh_token(backup, rotated).map_err(|error| {
            format!("launcher session reauth succeeded, but Prime could not save the refreshed Riot Client login: {error}")
        })?;
    }

    Ok(ApiSession {
        session: reauth.tokens.into_session(),
        // Still the same sign-in, so it keeps its capture time.
        launcher_session: Some(backup.clone()),
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
