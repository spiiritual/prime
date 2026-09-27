use time::OffsetDateTime;

use super::launch_flow::resolve_session_region;
use super::session::{ApiIdentity, active_api_session, api_identity};
use super::*;
use crate::game_settings::{
    GameSettingsProfile, GameSettingsProfileMetadata, GameSettingsProfilePurpose,
    GameSettingsProfileRepository, VALORANT_PLAYER_SETTINGS_TYPE, ValorantSettingsDocument,
    apply_preset, new_profile_id,
};
use crate::riot::endpoints::player_preferences_base_url_for_region;

#[derive(Clone, Debug)]
pub(in crate::ui) struct SavedGameSettingsResult {
    pub(in crate::ui) account_id: AccountId,
    pub(in crate::ui) session: AuthSession,
    pub(in crate::ui) launcher_session: Option<LauncherSessionBackup>,
    pub(in crate::ui) identity: ApiIdentity,
    pub(in crate::ui) profile: GameSettingsProfileMetadata,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct AppliedGameSettingsResult {
    pub(in crate::ui) account_id: AccountId,
    pub(in crate::ui) session: AuthSession,
    pub(in crate::ui) launcher_session: Option<LauncherSessionBackup>,
    pub(in crate::ui) identity: ApiIdentity,
    pub(in crate::ui) source_profile: GameSettingsProfileMetadata,
    /// The account's own settings, put aside by this apply; `None` when an earlier apply had
    /// already put them aside.
    pub(in crate::ui) backup_profile: Option<GameSettingsProfileMetadata>,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct RestoredGameSettingsResult {
    pub(in crate::ui) account_id: AccountId,
    pub(in crate::ui) session: AuthSession,
    pub(in crate::ui) launcher_session: Option<LauncherSessionBackup>,
    pub(in crate::ui) identity: ApiIdentity,
}

pub(in crate::ui) async fn load_game_settings_profiles(
    profile_dir: PathBuf,
) -> Result<Vec<GameSettingsProfileMetadata>, String> {
    GameSettingsProfileRepository::new(profile_dir)
        .saved_metadata()
        .map_err(|error| error.to_string())
}

pub(in crate::ui) async fn delete_game_settings_profile(
    profile_dir: PathBuf,
    profile_id: String,
) -> Result<(), String> {
    GameSettingsProfileRepository::new(profile_dir)
        .delete(&profile_id)
        .map_err(|error| error.to_string())
}

pub(in crate::ui) async fn rename_game_settings_profile(
    profile_dir: PathBuf,
    profile_id: String,
    name: String,
) -> Result<GameSettingsProfileMetadata, String> {
    GameSettingsProfileRepository::new(profile_dir)
        .rename(&profile_id, &name)
        .map_err(|error| error.to_string())
}

pub(in crate::ui) async fn save_game_settings_profile(
    account: AccountProfile,
    profile_dir: PathBuf,
    name: String,
) -> Result<SavedGameSettingsResult, String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let context = resolve_settings_context(&api, &account).await?;
    let document = fetch_settings_document(&api, &context).await?;
    let settings_version = document
        .settings_payload()
        .map_err(|error| error.to_string())?
        .roaming_settings_version;
    let profile = GameSettingsProfile {
        id: new_profile_id(account.id, GameSettingsProfilePurpose::Profile),
        name,
        purpose: GameSettingsProfilePurpose::Profile,
        source_account_id: account.id,
        source_display_name: account.display_name.clone(),
        source_puuid: context.identity.puuid.clone(),
        captured_at_unix: OffsetDateTime::now_utc().unix_timestamp(),
        preference_base_url: context.preference_base_url.clone(),
        settings_version,
        preference: document,
    };
    let repository = GameSettingsProfileRepository::new(profile_dir);
    repository
        .save(&profile)
        .map_err(|error| error.to_string())?;
    let metadata = profile.metadata().map_err(|error| error.to_string())?;

    Ok(SavedGameSettingsResult {
        account_id: account.id,
        session: context.session,
        launcher_session: context.launcher_session,
        identity: context.identity,
        profile: metadata,
    })
}

pub(in crate::ui) async fn apply_game_settings_profile(
    account: AccountProfile,
    profile_dir: PathBuf,
    profile_id: String,
) -> Result<AppliedGameSettingsResult, String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let repository = GameSettingsProfileRepository::new(profile_dir);
    let source_profile = repository
        .load(&profile_id)
        .map_err(|error| error.to_string())?;
    // Checked before signing in, so an unreadable preset fails without touching the account.
    source_profile
        .preference
        .settings_payload()
        .map_err(|error| error.to_string())?;
    let context = resolve_settings_context(&api, &account).await?;
    let mut target_document = fetch_settings_document(&api, &context).await?;

    // Only the first apply puts the account's own settings aside, so Restore always goes back
    // to how the account was before any preset.
    let backup_metadata = if repository
        .original_settings(account.id)
        .map_err(|error| error.to_string())?
        .is_none()
    {
        let backup_profile = GameSettingsProfile {
            id: new_profile_id(account.id, GameSettingsProfilePurpose::Backup),
            name: format!("{} original settings", account.display_name),
            purpose: GameSettingsProfilePurpose::Backup,
            source_account_id: account.id,
            source_display_name: account.display_name.clone(),
            source_puuid: context.identity.puuid.clone(),
            captured_at_unix: OffsetDateTime::now_utc().unix_timestamp(),
            preference_base_url: context.preference_base_url.clone(),
            settings_version: target_document
                .settings_payload()
                .map_err(|error| error.to_string())?
                .roaming_settings_version,
            preference: target_document.clone(),
        };
        repository
            .save(&backup_profile)
            .map_err(|error| error.to_string())?;
        Some(
            backup_profile
                .metadata()
                .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };

    apply_preset(&source_profile.preference, &mut target_document)
        .map_err(|error| error.to_string())?;
    let body = target_document
        .save_body(VALORANT_PLAYER_SETTINGS_TYPE)
        .map_err(|error| error.to_string())?;
    api.save_player_preference(
        &context.preference_base_url,
        &context.session.access_token,
        &body,
    )
    .await
    .map_err(|error| format!("Riot rejected the settings save: {error}"))?;

    Ok(AppliedGameSettingsResult {
        account_id: account.id,
        session: context.session,
        launcher_session: context.launcher_session,
        identity: context.identity,
        source_profile: source_profile
            .metadata()
            .map_err(|error| error.to_string())?,
        backup_profile: backup_metadata,
    })
}

/// Writes an account's original settings back and then removes every backup of them.
pub(in crate::ui) async fn restore_original_game_settings(
    account: AccountProfile,
    profile_dir: PathBuf,
) -> Result<RestoredGameSettingsResult, String> {
    let api = RiotApi::new().map_err(|error| error.to_string())?;
    let repository = GameSettingsProfileRepository::new(profile_dir);
    let original = repository
        .original_settings(account.id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "this account has no original settings to restore".to_string())?;
    let original = repository
        .load(&original.id)
        .map_err(|error| error.to_string())?;
    // Checked before signing in, so unreadable original settings fail without touching the account.
    original
        .preference
        .settings_payload()
        .map_err(|error| error.to_string())?;
    let context = resolve_settings_context(&api, &account).await?;
    if !original
        .source_puuid
        .eq_ignore_ascii_case(&context.identity.puuid)
    {
        return Err("these original settings belong to a different Riot account".to_string());
    }

    let mut target_document = fetch_settings_document(&api, &context).await?;
    apply_preset(&original.preference, &mut target_document).map_err(|error| error.to_string())?;
    let body = target_document
        .save_body(VALORANT_PLAYER_SETTINGS_TYPE)
        .map_err(|error| error.to_string())?;
    api.save_player_preference(
        &context.preference_base_url,
        &context.session.access_token,
        &body,
    )
    .await
    .map_err(|error| format!("Riot rejected the settings save: {error}"))?;

    for backup in repository
        .backups_for(account.id)
        .map_err(|error| error.to_string())?
    {
        repository.delete(&backup.id).map_err(|error| {
            format!("restored the settings, but could not remove the saved copy: {error}")
        })?;
    }

    Ok(RestoredGameSettingsResult {
        account_id: account.id,
        session: context.session,
        launcher_session: context.launcher_session,
        identity: context.identity,
    })
}

#[derive(Clone, Debug)]
struct SettingsContext {
    session: AuthSession,
    launcher_session: Option<LauncherSessionBackup>,
    identity: ApiIdentity,
    preference_base_url: String,
}

async fn resolve_settings_context(
    api: &RiotApi,
    account: &AccountProfile,
) -> Result<SettingsContext, String> {
    let api_session = active_api_session(api, account).await.map_err(|error| {
        if error.contains("needs an imported Riot token or a captured launcher session") {
            "Account needs a captured launcher session or imported Riot token".to_string()
        } else {
            error
        }
    })?;
    let session = api_session.session;
    let player_info = api.player_info(&session.access_token).await.ok();
    let region = resolve_session_region(api, &session, player_info.as_ref())
        .await
        .map_err(|_| "Could not resolve Riot player preferences region".to_string())?;
    // Settings are read and written through this session, so it must be this account's.
    let identity = api_identity(account, player_info.as_ref(), region.shard())?;

    Ok(SettingsContext {
        session,
        launcher_session: api_session.launcher_session,
        identity,
        preference_base_url: player_preferences_base_url_for_region(region).to_string(),
    })
}

async fn fetch_settings_document(
    api: &RiotApi,
    context: &SettingsContext,
) -> Result<ValorantSettingsDocument, String> {
    let raw = api
        .get_player_preference(
            &context.preference_base_url,
            &context.session.access_token,
            VALORANT_PLAYER_SETTINGS_TYPE,
        )
        .await
        .map_err(|error| {
            format!("Riot did not return VALORANT settings for this account: {error}")
        })?;
    let document = ValorantSettingsDocument::new(raw);
    document
        .settings_payload()
        .map_err(|error| error.to_string())?;

    Ok(document)
}
