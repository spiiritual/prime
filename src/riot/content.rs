use std::collections::HashMap;

use serde::Deserialize;
use thiserror::Error;

pub const WEAPON_SKINS_URL: &str = "https://valorant-api.com/v1/weapons/skins";
pub const VERSION_URL: &str = "https://valorant-api.com/v1/version";

#[derive(Clone)]
pub struct ValorantContentApi {
    client: reqwest::Client,
}

impl ValorantContentApi {
    pub fn new() -> Result<Self, ContentError> {
        let client = reqwest::Client::builder()
            .user_agent("prime-valorant-manager/0.1")
            .build()?;

        Ok(Self { client })
    }

    pub async fn skin_catalog(&self) -> Result<SkinCatalog, ContentError> {
        let response: ApiResponse<Vec<WeaponSkin>> = self
            .client
            .get(WEAPON_SKINS_URL)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(SkinCatalog::from_skins(response.data))
    }

    pub async fn client_version(&self) -> Result<String, ContentError> {
        let response: ApiResponse<ValorantVersion> = self
            .client
            .get(VERSION_URL)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(response.data.riot_client_version)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SkinCatalog {
    by_uuid: HashMap<String, ResolvedSkin>,
}

impl SkinCatalog {
    pub fn from_skins(skins: Vec<WeaponSkin>) -> Self {
        let mut by_uuid = HashMap::new();

        for skin in skins {
            let skin_info = ResolvedSkin {
                uuid: skin.uuid.clone(),
                display_name: skin.display_name.clone(),
                display_icon: skin.display_icon.clone(),
            };
            by_uuid.insert(normalize_uuid(&skin.uuid), skin_info.clone());

            for level in skin.levels {
                by_uuid.insert(
                    normalize_uuid(&level.uuid),
                    ResolvedSkin {
                        uuid: level.uuid,
                        display_name: display_name_with_fallback(
                            &level.display_name,
                            &skin.display_name,
                        ),
                        display_icon: level.display_icon.or_else(|| skin.display_icon.clone()),
                    },
                );
            }

            for chroma in skin.chromas {
                by_uuid.insert(
                    normalize_uuid(&chroma.uuid),
                    ResolvedSkin {
                        uuid: chroma.uuid,
                        display_name: display_name_with_fallback(
                            &chroma.display_name,
                            &skin.display_name,
                        ),
                        display_icon: chroma
                            .display_icon
                            .or(chroma.full_render)
                            .or_else(|| skin_info.display_icon.clone()),
                    },
                );
            }
        }

        Self { by_uuid }
    }

    pub fn resolve(&self, uuid: &str) -> ResolvedSkin {
        self.by_uuid
            .get(&normalize_uuid(uuid))
            .cloned()
            .unwrap_or_else(|| ResolvedSkin::unknown(uuid))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedSkin {
    pub uuid: String,
    pub display_name: String,
    pub display_icon: Option<String>,
}

impl ResolvedSkin {
    pub fn unknown(uuid: &str) -> Self {
        Self {
            uuid: uuid.to_string(),
            display_name: uuid.to_string(),
            display_icon: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct ApiResponse<T> {
    data: T,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct WeaponSkin {
    pub uuid: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "displayIcon")]
    pub display_icon: Option<String>,
    #[serde(default)]
    pub levels: Vec<WeaponSkinLevel>,
    #[serde(default)]
    pub chromas: Vec<WeaponSkinChroma>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct WeaponSkinLevel {
    pub uuid: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "displayIcon")]
    pub display_icon: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct WeaponSkinChroma {
    pub uuid: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "displayIcon")]
    pub display_icon: Option<String>,
    #[serde(rename = "fullRender")]
    pub full_render: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ValorantVersion {
    #[serde(rename = "riotClientVersion")]
    pub riot_client_version: String,
}

fn normalize_uuid(uuid: &str) -> String {
    uuid.trim().to_ascii_lowercase()
}

fn display_name_with_fallback(name: &str, fallback: &str) -> String {
    let trimmed = name.trim();

    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("standard") {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Debug, Error)]
pub enum ContentError {
    #[error("Valorant content API HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_skin_level_and_chroma_ids_to_display_names() {
        let catalog = SkinCatalog::from_skins(vec![WeaponSkin {
            uuid: "skin-uuid".to_string(),
            display_name: "Prime Vandal".to_string(),
            display_icon: Some("skin-icon".to_string()),
            levels: vec![WeaponSkinLevel {
                uuid: "level-uuid".to_string(),
                display_name: "Prime Vandal Level 4".to_string(),
                display_icon: None,
            }],
            chromas: vec![WeaponSkinChroma {
                uuid: "chroma-uuid".to_string(),
                display_name: "Prime Vandal Orange".to_string(),
                display_icon: None,
                full_render: Some("render".to_string()),
            }],
        }]);

        assert_eq!(catalog.resolve("SKIN-UUID").display_name, "Prime Vandal");
        assert_eq!(
            catalog.resolve("level-uuid").display_name,
            "Prime Vandal Level 4"
        );
        assert_eq!(
            catalog.resolve("chroma-uuid").display_name,
            "Prime Vandal Orange"
        );
        assert_eq!(
            catalog.resolve("chroma-uuid").display_icon.as_deref(),
            Some("render")
        );
    }

    #[test]
    fn unknown_skin_uses_uuid_as_display_name() {
        let catalog = SkinCatalog::default();

        assert_eq!(catalog.resolve("missing").display_name, "missing");
    }

    #[test]
    fn deserializes_riot_client_version() {
        let response: ApiResponse<ValorantVersion> = serde_json::from_value(serde_json::json!({
            "status": 200,
            "data": {
                "riotClientVersion": "release-12.09-shipping-25-4697179"
            }
        }))
        .expect("version response");

        assert_eq!(
            response.data.riot_client_version,
            "release-12.09-shipping-25-4697179"
        );
    }
}
