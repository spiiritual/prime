use super::*;

use super::launch_flow::{
    region_from_player_affinities, resolve_session_region, resolve_session_shard,
};

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
    let ResolvedSession {
        mut session,
        launcher_session,
        identity,
        region,
    } = resolve_session(api, account).await?;

    let entitlements_token = entitlement_token(api, &session).await?;
    if session
        .entitlements_token
        .as_ref()
        .is_none_or(|token| token.trim().is_empty())
    {
        session.entitlements_token = Some(entitlements_token.clone());
    }

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
        launcher_session,
        identity,
    })
}

/// An account's API session with its PUUID-checked identity and region, without the
/// entitlements token that only game API requests need.
pub(in crate::ui) async fn resolve_session(
    api: &RiotApi,
    account: &AccountProfile,
) -> Result<ResolvedSession, String> {
    let api_session = active_api_session(api, account).await?;
    let session = api_session.session;
    let token_subject = crate::riot::auth::jwt_subject(&session.access_token);
    let player_info_requested =
        needs_player_info(token_subject.is_some(), account.region, &session);
    let mut player_info = if player_info_requested {
        api.player_info(&session.access_token).await.ok()
    } else {
        None
    };

    let mut identity = api_identity(
        account,
        player_info.as_ref(),
        token_subject.as_deref(),
        account.shard,
    )?;
    let mut looked_up_region = match account.region {
        Some(_) => None,
        None => resolve_session_region(api, &session, player_info.as_ref())
            .await
            .ok(),
    };
    if needs_player_info_after_geo(account.region, player_info_requested, looked_up_region) {
        player_info = api.player_info(&session.access_token).await.ok();
        if let Some(info) = &player_info {
            // Its `sub` goes through the same PUUID check.
            identity = api_identity(account, Some(info), token_subject.as_deref(), account.shard)?;
            looked_up_region = region_from_player_affinities(info);
        }
    }
    let region = account.region.or(looked_up_region);
    // Only a region looked up now is saved, so a request that started before Refresh cleared
    // the saved region can't write the old one back.
    identity.region = looked_up_region;
    identity.shard = match region {
        Some(region) => region.shard(),
        None => resolve_session_shard(api, &session, player_info.as_ref(), account.shard).await,
    };

    Ok(ResolvedSession {
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

/// Whether to ask userinfo for the region after all: none is saved, the lookup (Riot Geo) found
/// none, and userinfo hasn't been asked yet.
pub(in crate::ui) fn needs_player_info_after_geo(
    saved_region: Option<ValorantRegion>,
    player_info_requested: bool,
    looked_up_region: Option<ValorantRegion>,
) -> bool {
    saved_region.is_none() && !player_info_requested && looked_up_region.is_none()
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
pub(in crate::ui) struct ResolvedSession {
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

/// Whether the account has a login Prime could sign in with: an unexpired token of its own, or
/// a complete captured launcher session.
pub(in crate::ui) fn has_saved_login(account: &AccountProfile) -> bool {
    account.session.as_ref().is_some_and(|session| {
        !session.is_expired()
            && crate::riot::auth::jwt_subject(&session.access_token)
                .is_none_or(|subject| account.check_puuid(&subject).is_ok())
    }) || account
        .launcher_session
        .as_ref()
        .is_some_and(|backup| backup.is_ready())
}

pub(in crate::ui) async fn active_api_session(
    api: &RiotApi,
    account: &AccountProfile,
) -> Result<ApiSession, String> {
    if let Some(session) = riot_client_session(account).await {
        return Ok(session);
    }

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
    if let Some(session) = riot_client_session(account).await {
        return Ok(session);
    }
    if let Some(backup) = &account.launcher_session {
        return launcher_api_session(api, backup).await;
    }

    active_api_session(api, account).await
}

/// The Riot Client's own session when it's signed in on this PC as this account, so Prime doesn't
/// refresh a second session alongside the game's. Riot asking for Riot Mobile verification
/// before ranked lined up with Prime polling through a second session.
async fn riot_client_session(account: &AccountProfile) -> Option<ApiSession> {
    // Tests never reach the Riot Client running on this PC.
    if cfg!(test) {
        return None;
    }
    let puuid = account.puuid.as_deref().or_else(|| {
        account
            .launcher_session
            .as_ref()
            .map(|backup| backup.puuid.as_str())
    })?;
    let session = crate::riot::local_client::signed_in_session(puuid).await?;

    Some(ApiSession {
        session,
        launcher_session: None,
    })
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
    if let Some(error) = rejected_refresh_token(&refresh_token) {
        return Err(error);
    }
    let reauth = match api.refresh_token_reauth(&refresh_token).await {
        Ok(reauth) => reauth,
        // Already says what happened and what to do.
        Err(error @ crate::riot::client::RiotApiError::RefreshTokenRejected(_)) => {
            let error = error.to_string();
            remember_rejected_refresh_token(&refresh_token, &error);
            return Err(error);
        }
        Err(error) => {
            return Err(format!(
                "launcher session reauth failed: {error}. Recapture the Riot Client session or import a fresh redirect token."
            ));
        }
    };

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

/// Refresh tokens Riot rejected, by hash, with the error each got. Riot never takes a rejected
/// one again (signing out revokes it), so polls stop sending it; a re-capture saves a new one.
// ponytail: in memory, so each start of Prime sends a dead login once more; save it if that matters.
static REJECTED_REFRESH_TOKENS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<u64, String>>,
> = std::sync::LazyLock::new(Default::default);

fn refresh_token_key(refresh_token: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    refresh_token.hash(&mut hasher);
    hasher.finish()
}

fn rejected_refresh_token(refresh_token: &str) -> Option<String> {
    REJECTED_REFRESH_TOKENS
        .lock()
        .ok()?
        .get(&refresh_token_key(refresh_token))
        .cloned()
}

fn remember_rejected_refresh_token(refresh_token: &str, error: &str) {
    if let Ok(mut rejected) = REJECTED_REFRESH_TOKENS.lock() {
        rejected.insert(refresh_token_key(refresh_token), error.to_string());
    }
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

#[cfg(test)]
pub(in crate::ui) mod tests {
    use super::*;
    use crate::account::{AccountProfile, AuthSession, Shard, ValorantRegion};

    pub(in crate::ui) fn redirect_url_for_subject(subject: &str) -> String {
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;

        let claims = URL_SAFE_NO_PAD.encode(format!(r#"{{"sub":"{subject}"}}"#));
        format!(
            "https://playvalorant.com/opt_in#access_token=header.{claims}.signature&expires_in=3600"
        )
    }

    fn player_info(puuid: &str) -> crate::riot::models::PlayerInfoResponse {
        serde_json::from_value(serde_json::json!({
            "country": "usa",
            "sub": puuid,
            "acct": {"game_name": "Player", "tag_line": "NA1"},
        }))
        .expect("player info")
    }

    #[test]
    fn api_identity_refuses_a_session_for_another_riot_account() {
        let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
        account.puuid = Some("puuid-a".to_string());

        let error = api_identity(&account, Some(&player_info("puuid-b")), None, Shard::Na)
            .expect_err("another account");

        assert!(error.contains("puuid-b"), "{error}");
    }

    #[test]
    fn api_identity_uses_the_signed_in_riot_account() {
        let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
        account.puuid = Some("puuid-a".to_string());

        let identity = api_identity(&account, Some(&player_info("puuid-a")), None, Shard::Eu)
            .expect("identity");

        assert_eq!(identity.puuid, "puuid-a");
        assert_eq!(identity.game_name.as_deref(), Some("Player"));
        assert_eq!(identity.shard, Shard::Eu);
    }

    #[test]
    fn api_identity_falls_back_to_the_saved_puuid_without_player_info() {
        let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
        account.puuid = Some("puuid-a".to_string());

        let identity = api_identity(&account, None, None, Shard::Na).expect("identity");

        assert_eq!(identity.puuid, "puuid-a");
        assert_eq!(identity.game_name, None);
    }

    #[test]
    fn api_identity_takes_the_puuid_from_the_token_subject() {
        // No saved PUUID and no launcher backup, so only the subject can supply it.
        let account = AccountProfile::new("Main", Shard::Na).expect("account");

        let identity = api_identity(&account, None, Some("puuid-a"), Shard::Na).expect("identity");

        assert_eq!(identity.puuid, "puuid-a");
        assert_eq!(identity.game_name, None);
    }

    #[test]
    fn userinfo_is_skipped_when_the_token_names_the_account_and_a_region_is_saved() {
        let session = AuthSession::new("access", None, None, "Bearer", Some(3600), 100);

        assert!(!needs_player_info(true, Some(ValorantRegion::Na), &session));
    }

    #[test]
    fn userinfo_is_skipped_when_riot_geo_can_find_the_region() {
        let session = AuthSession::new(
            "access",
            Some("id-token".to_string()),
            None,
            "Bearer",
            Some(3600),
            100,
        );

        assert!(!needs_player_info(true, None, &session));
    }

    #[test]
    fn userinfo_finds_the_region_when_there_is_no_id_token() {
        let session = AuthSession::new(
            "access",
            Some("  ".to_string()),
            None,
            "Bearer",
            Some(3600),
            100,
        );

        assert!(needs_player_info(true, None, &session));
        assert!(needs_player_info(false, Some(ValorantRegion::Na), &session));
    }

    #[test]
    fn userinfo_finds_the_region_when_riot_geo_fails() {
        assert!(needs_player_info_after_geo(None, false, None));
    }

    #[test]
    fn userinfo_is_not_asked_again_after_riot_geo_fails() {
        assert!(!needs_player_info_after_geo(None, true, None));
    }

    #[test]
    fn userinfo_is_skipped_when_riot_geo_finds_the_region_or_one_is_saved() {
        assert!(!needs_player_info_after_geo(
            None,
            false,
            Some(ValorantRegion::Eu)
        ));
        assert!(!needs_player_info_after_geo(
            Some(ValorantRegion::Na),
            false,
            None
        ));
    }

    #[test]
    fn api_identity_refuses_a_token_for_another_riot_account() {
        let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
        account.puuid = Some("puuid-a".to_string());

        assert!(api_identity(&account, None, Some("puuid-b"), Shard::Na).is_err());
    }

    #[test]
    fn api_identity_without_a_token_subject_uses_the_saved_puuid() {
        let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
        account.puuid = Some("puuid-a".to_string());

        let identity = api_identity(&account, None, None, Shard::Na).expect("identity");

        assert_eq!(identity.puuid, "puuid-a");
    }

    #[test]
    fn a_stored_token_for_another_riot_account_is_not_used() {
        let tokens = crate::riot::auth::parse_redirect_tokens(&redirect_url_for_subject("puuid-b"))
            .expect("tokens");
        let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
        account.puuid = Some("puuid-a".to_string());
        account.session = Some(tokens.into_session());
        let api = crate::riot::client::RiotApi::new().expect("api");

        let result = iced::futures::executor::block_on(super::active_api_session(&api, &account));

        assert!(result.is_err(), "the other account's token was used");
    }

    #[test]
    fn remembers_a_rejected_refresh_token() {
        assert_eq!(
            super::rejected_refresh_token("refresh-token-test-rejected"),
            None
        );

        super::remember_rejected_refresh_token("refresh-token-test-rejected", "signed out");

        assert_eq!(
            super::rejected_refresh_token("refresh-token-test-rejected").as_deref(),
            Some("signed out")
        );
        assert_eq!(
            super::rejected_refresh_token("refresh-token-test-other"),
            None
        );
    }
}
