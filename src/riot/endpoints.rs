use crate::account::{Shard, ValorantRegion};

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

pub fn shared_base_url(shard: Shard) -> String {
    format!("https://shared.{}.a.pvp.net", shard.as_str())
}

pub fn glz_base_url(region: ValorantRegion, shard: Shard) -> String {
    format!(
        "https://glz-{}-1.{}.a.pvp.net",
        region.as_str(),
        shard.as_str()
    )
}

pub fn content_url(shard: Shard) -> String {
    format!("{}/content-service/v3/content", shared_base_url(shard))
}

pub fn storefront_url(shard: Shard, puuid: &str) -> String {
    format!("{}/store/v3/storefront/{puuid}", pd_base_url(shard))
}

pub fn wallet_url(shard: Shard, puuid: &str) -> String {
    format!("{}/store/v1/wallet/{puuid}", pd_base_url(shard))
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

pub fn player_mmr_url(shard: Shard, puuid: &str) -> String {
    format!("{}/mmr/v1/players/{puuid}", pd_base_url(shard))
}

pub fn contracts_url(shard: Shard, puuid: &str) -> String {
    format!("{}/contracts/v1/contracts/{puuid}", pd_base_url(shard))
}

pub fn current_game_player_url(region: ValorantRegion, shard: Shard, puuid: &str) -> String {
    format!(
        "{}/core-game/v1/players/{puuid}",
        glz_base_url(region, shard)
    )
}

pub fn pregame_player_url(region: ValorantRegion, shard: Shard, puuid: &str) -> String {
    format!("{}/pregame/v1/players/{puuid}", glz_base_url(region, shard))
}

pub fn party_player_url(region: ValorantRegion, shard: Shard, puuid: &str) -> String {
    format!("{}/parties/v1/players/{puuid}", glz_base_url(region, shard))
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
    fn builds_documented_wallet_url() {
        assert_eq!(
            wallet_url(Shard::Na, "puuid"),
            "https://pd.na.a.pvp.net/store/v1/wallet/puuid"
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

    #[test]
    fn builds_documented_player_mmr_url() {
        assert_eq!(
            player_mmr_url(Shard::Na, "puuid"),
            "https://pd.na.a.pvp.net/mmr/v1/players/puuid"
        );
    }

    #[test]
    fn builds_documented_content_url() {
        assert_eq!(
            content_url(Shard::Na),
            "https://shared.na.a.pvp.net/content-service/v3/content"
        );
    }

    #[test]
    fn builds_documented_contracts_url() {
        assert_eq!(
            contracts_url(Shard::Eu, "puuid"),
            "https://pd.eu.a.pvp.net/contracts/v1/contracts/puuid"
        );
    }

    #[test]
    fn builds_glz_activity_urls() {
        assert_eq!(
            current_game_player_url(ValorantRegion::Latam, Shard::Na, "puuid"),
            "https://glz-latam-1.na.a.pvp.net/core-game/v1/players/puuid"
        );
        assert_eq!(
            pregame_player_url(ValorantRegion::Eu, Shard::Eu, "puuid"),
            "https://glz-eu-1.eu.a.pvp.net/pregame/v1/players/puuid"
        );
        assert_eq!(
            party_player_url(ValorantRegion::Ap, Shard::Ap, "puuid"),
            "https://glz-ap-1.ap.a.pvp.net/parties/v1/players/puuid"
        );
    }
}
