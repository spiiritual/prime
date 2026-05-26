use crate::account::Shard;

pub const ENTITLEMENTS_URL: &str = "https://entitlements.auth.riotgames.com/api/token/v1";
pub const PLAYER_INFO_URL: &str = "https://auth.riotgames.com/userinfo";
pub const RIOT_GEO_URL: &str = "https://riot-geo.pas.si.riotgames.com/pas/v1/product/valorant";

pub const HEADER_CLIENT_PLATFORM: &str = "X-Riot-ClientPlatform";
pub const HEADER_CLIENT_VERSION: &str = "X-Riot-ClientVersion";
pub const HEADER_ENTITLEMENTS: &str = "X-Riot-Entitlements-JWT";

pub const CLIENT_PLATFORM: &str = "ew0KCSJwbGF0Zm9ybVR5cGUiOiAiUEMiLA0KCSJwbGF0Zm9ybU9TIjogIldpbmRvd3MiLA0KCSJwbGF0Zm9ybU9TVmVyc2lvbiI6ICIxMC4wLjE5MDQyLjEuMjU2LjY0Yml0IiwNCgkicGxhdGZvcm1DaGlwc2V0IjogIlVua25vd24iDQp9";

pub fn pd_base_url(shard: Shard) -> String {
    format!("https://pd.{}.a.pvp.net", shard.as_str())
}

pub fn storefront_url(shard: Shard, puuid: &str) -> String {
    format!("{}/store/v3/storefront/{puuid}", pd_base_url(shard))
}

pub fn player_loadout_url(shard: Shard, puuid: &str) -> String {
    format!(
        "{}/personalization/v2/players/{puuid}/playerloadout",
        pd_base_url(shard)
    )
}

pub fn account_xp_url(shard: Shard, puuid: &str) -> String {
    format!("{}/account-xp/v1/players/{puuid}", pd_base_url(shard))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_current_storefront_url() {
        assert_eq!(
            storefront_url(Shard::Na, "puuid"),
            "https://pd.na.a.pvp.net/store/v3/storefront/puuid"
        );
    }

    #[test]
    fn builds_documented_loadout_url() {
        assert_eq!(
            player_loadout_url(Shard::Eu, "puuid"),
            "https://pd.eu.a.pvp.net/personalization/v2/players/puuid/playerloadout"
        );
    }

    #[test]
    fn builds_documented_account_xp_url() {
        assert_eq!(
            account_xp_url(Shard::Ap, "puuid"),
            "https://pd.ap.a.pvp.net/account-xp/v1/players/puuid"
        );
    }
}
