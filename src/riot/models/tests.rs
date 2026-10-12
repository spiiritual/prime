use super::*;

#[test]
fn deserializes_v3_player_loadout_without_sprays() {
    let json = serde_json::json!({
        "Subject": "puuid",
        "Version": 4002,
        "Guns": [{
            "ID": "vandal",
            "SkinID": "skin",
            "SkinLevelID": "skin-level",
            "ChromaID": "chroma",
            "Attachments": []
        }],
        "ActiveExpressions": [{"AssetID": "spray", "TypeID": "spray-type"}],
        "DynamicOptions": {},
        "Identity": {
            "PlayerCardID": "card",
            "PlayerTitleID": "title",
            "AccountLevel": 0,
            "PreferredLevelBorderID": "border",
            "HideAccountLevel": true
        },
        "Incognito": false
    });

    let loadout: PlayerLoadoutResponse = serde_json::from_value(json).expect("v3 player loadout");

    assert_eq!(loadout.guns[0].skin_id, "skin");
}

#[test]
fn deserializes_storefront_offer_ids() {
    let json = serde_json::json!({
        "FeaturedBundle": {
            "Bundle": {
                "ID": "bundle",
                "DataAssetID": "asset",
                "CurrencyID": "vp",
                "Items": [],
                "DurationRemainingInSeconds": 10
            },
            "Bundles": [],
            "BundleRemainingDurationInSeconds": 20
        },
        "SkinsPanelLayout": {
            "SingleItemOffers": ["offer-a"],
            "SingleItemStoreOffers": [{
                "OfferID": "offer-a",
                "IsDirectPurchase": true,
                "StartDate": "2026-05-25T00:00:00Z",
                "Cost": {"vp": 1775},
                "Rewards": [{
                    "ItemTypeID": "skin-type",
                    "ItemID": "skin",
                    "Quantity": 1
                }]
            }],
            "SingleItemOffersRemainingDurationInSeconds": 86400
        }
    });

    let storefront: StorefrontResponse = serde_json::from_value(json).expect("storefront response");

    assert_eq!(
        storefront.skins_panel_layout.single_item_offers,
        ["offer-a"]
    );
    assert_eq!(
        storefront.skins_panel_layout.single_item_store_offers[0].cost["vp"],
        1775
    );
}

#[test]
fn deserializes_wallet_balances() {
    let json = serde_json::json!({
        "Balances": {
            "85ad13f7-3d1b-5128-9eb2-7cd8ee0b5741": 1250,
            "e59aa87c-4cbf-517a-5983-6e81511be9b7": 40
        }
    });

    let wallet: WalletResponse = serde_json::from_value(json).expect("wallet response");

    assert_eq!(
        wallet.balances["85ad13f7-3d1b-5128-9eb2-7cd8ee0b5741"],
        1250
    );
    assert_eq!(wallet.balances["e59aa87c-4cbf-517a-5983-6e81511be9b7"], 40);
}

#[test]
fn deserializes_game_content_active_act() {
    let json = serde_json::json!({
        "DisabledIDs": [],
        "Seasons": [{
            "ID": "episode",
            "Name": "Episode 11",
            "Type": "episode",
            "StartTime": "2026-01-01T00:00:00Z",
            "EndTime": "2026-12-31T00:00:00Z",
            "IsActive": true
        }, {
            "ID": "act",
            "Name": "Act 3",
            "Type": "act",
            "StartTime": "2026-05-01T00:00:00Z",
            "EndTime": "2026-06-24T13:00:00Z",
            "IsActive": true
        }],
        "Events": []
    });

    let content: GameContentResponse = serde_json::from_value(json).expect("content");

    assert_eq!(
        content.active_act().map(|act| act.name.as_str()),
        Some("Act 3")
    );
    assert_eq!(
        content.active_act().map(|act| act.end_time.as_str()),
        Some("2026-06-24T13:00:00Z")
    );
}

#[test]
fn deserializes_contract_progress() {
    let json = serde_json::json!({
        "Version": 1,
        "Subject": "puuid",
        "Contracts": [{
            "ContractDefinitionID": "battle-pass",
            "ContractProgression": {
                "TotalProgressionEarned": 123456,
                "TotalProgressionEarnedVersion": 7,
                "HighestRewardedLevel": {
                    "0": {"Amount": 10, "Version": 7}
                }
            },
            "ProgressionLevelReached": 12,
            "ProgressionTowardsNextLevel": 3456,
            "ProgressionCompleted": false
        }],
        "ActiveSpecialContract": "special"
    });

    let contracts: ContractsResponse = serde_json::from_value(json).expect("contracts");

    assert_eq!(contracts.subject, "puuid");
    assert_eq!(contracts.contracts[0].contract_definition_id, "battle-pass");
    assert_eq!(
        contracts.contracts[0]
            .contract_progression
            .total_progression_earned,
        123456
    );
    assert_eq!(contracts.contracts[0].progression_level_reached, 12);
}

#[test]
fn deserializes_accessory_storefront_offers() {
    let json = serde_json::json!({
        "FeaturedBundle": {
            "Bundle": {
                "ID": "bundle",
                "DataAssetID": "asset",
                "CurrencyID": "vp",
                "Items": [],
                "DurationRemainingInSeconds": 10
            },
            "Bundles": [],
            "BundleRemainingDurationInSeconds": 20
        },
        "SkinsPanelLayout": {
            "SingleItemOffers": [],
            "SingleItemStoreOffers": [],
            "SingleItemOffersRemainingDurationInSeconds": 86400
        },
        "AccessoryStore": {
            "AccessoryStoreOffers": [{
                "ContractID": "contract",
                "Offer": {
                    "OfferID": "accessory-offer",
                    "IsDirectPurchase": true,
                    "StartDate": "2026-05-25T00:00:00Z",
                    "Cost": {"kc": 2500},
                    "Rewards": [{
                        "ItemTypeID": "dd3bf334-87f3-40bd-b043-682a57a8dc3a",
                        "ItemID": "buddy",
                        "Quantity": 1
                    }]
                }
            }],
            "AccessoryStoreRemainingDurationInSeconds": 604800,
            "StorefrontID": "storefront"
        }
    });

    let storefront: StorefrontResponse = serde_json::from_value(json).expect("storefront response");
    let accessory_store = storefront.accessory_store.expect("accessory store");

    assert_eq!(accessory_store.accessory_store_offers.len(), 1);
    assert_eq!(
        accessory_store.accessory_store_offers[0].offer.offer_id,
        "accessory-offer"
    );
    assert_eq!(
        accessory_store.accessory_store_remaining_duration_in_seconds,
        604800
    );
}

#[test]
fn deserializes_fractional_storefront_discounts() {
    let json = serde_json::json!({
        "FeaturedBundle": {
            "Bundle": {
                "ID": "bundle",
                "DataAssetID": "asset",
                "CurrencyID": "vp",
                "Items": [{
                    "Item": {
                        "ItemTypeID": "skin-type",
                        "ItemID": "skin",
                        "Amount": 1
                    },
                    "BasePrice": 2175,
                    "CurrencyID": "vp",
                    "DiscountPercent": 0.4,
                    "DiscountedPrice": 870,
                    "IsPromoItem": false
                }],
                "DurationRemainingInSeconds": 10
            },
            "Bundles": [],
            "BundleRemainingDurationInSeconds": 20
        },
        "SkinsPanelLayout": {
            "SingleItemOffers": [],
            "SingleItemStoreOffers": [],
            "SingleItemOffersRemainingDurationInSeconds": 86400
        },
        "BonusStore": {
            "BonusStoreOffers": [{
                "BonusOfferID": "bonus",
                "Offer": {
                    "OfferID": "offer",
                    "IsDirectPurchase": true,
                    "StartDate": "2026-05-25T00:00:00Z",
                    "Cost": {"vp": 1775},
                    "Rewards": []
                },
                "DiscountPercent": 0.348,
                "DiscountCosts": {"vp": 1158},
                "IsSeen": false
            }],
            "BonusStoreRemainingDurationInSeconds": 40
        }
    });

    let storefront: StorefrontResponse = serde_json::from_value(json).expect("storefront response");

    assert_eq!(
        storefront.featured_bundle.bundle.items[0].discount_percent,
        40
    );
    assert_eq!(
        storefront.bonus_store.unwrap().bonus_store_offers[0].discount_percent,
        35
    );
}

#[test]
fn deserializes_player_loadout_acronym_fields() {
    let json = serde_json::json!({
        "Subject": "puuid",
        "Version": 1,
        "Guns": [{
            "ID": "weapon",
            "SkinID": "skin",
            "SkinLevelID": "level",
            "ChromaID": "chroma",
            "Attachments": []
        }],
        "Sprays": [],
        "Identity": {
            "PlayerCardID": "card",
            "PlayerTitleID": "title",
            "AccountLevel": 42,
            "PreferredLevelBorderID": "border",
            "HideAccountLevel": false
        },
        "Incognito": false
    });

    let loadout: PlayerLoadoutResponse = serde_json::from_value(json).expect("loadout");

    assert_eq!(loadout.subject, "puuid");
    assert_eq!(loadout.guns[0].skin_id, "skin");
    assert_eq!(loadout.identity.account_level, 42);
}

#[test]
fn deserializes_account_xp_progress() {
    let json = serde_json::json!({
        "Version": 1,
        "Subject": "puuid",
        "Progress": {
            "Level": 123,
            "XP": 456
        },
        "History": []
    });

    let xp: AccountXpResponse = serde_json::from_value(json).expect("account xp");

    assert_eq!(xp.subject, "puuid");
    assert_eq!(xp.progress.level, 123);
    assert_eq!(xp.progress.xp, 456);
}

#[test]
fn deserializes_player_mmr_ranked_rating() {
    let json = serde_json::json!({
        "Version": 1,
        "Subject": "puuid",
        "QueueSkills": {
            "competitive": {
                "SeasonalInfoBySeasonID": {
                    "season-a": {
                        "SeasonID": "season-a",
                        "CompetitiveTier": 15,
                        "RankedRating": 42,
                        "NumberOfGames": 12,
                        "GamesNeededForRating": 0
                    }
                }
            }
        },
        "LatestCompetitiveUpdate": {
            "SeasonID": "season-a",
            "TierAfterUpdate": 15,
            "RankedRatingAfterUpdate": 42
        }
    });

    let mmr: PlayerMmrResponse = serde_json::from_value(json).expect("mmr response");
    let competitive = mmr.queue_skills.competitive.expect("competitive");
    let season = &competitive.seasonal_info_by_season_id["season-a"];

    assert_eq!(season.competitive_tier, 15);
    assert_eq!(season.ranked_rating, 42);
    assert_eq!(
        mmr.latest_competitive_update
            .as_ref()
            .map(|update| update.season_id.as_str()),
        Some("season-a")
    );
}

#[test]
fn a_never_ranked_players_mmr_has_no_seasons() {
    let json = serde_json::json!({
        "Version": 1,
        "Subject": "puuid",
        "QueueSkills": {
            "competitive": { "SeasonalInfoBySeasonID": null }
        },
        "LatestCompetitiveUpdate": null
    });

    let mmr: PlayerMmrResponse = serde_json::from_value(json).expect("mmr response");

    assert!(
        mmr.queue_skills
            .competitive
            .expect("competitive")
            .seasonal_info_by_season_id
            .is_empty()
    );
}

#[test]
fn deserializes_player_penalties_response() {
    let json = serde_json::json!({
        "Subject": "puuid",
        "Penalties": [{
            "ID": "penalty-id",
            "IssuingGameStartUnixMillis": 1_800_000_000_000i64,
            "IssuingMatchID": "match-id",
            "Expiry": "2099-01-01T00:00:00Z",
            "GamesRemaining": 2,
            "ApplyToAllPlatforms": true,
            "ApplyToPlatforms": ["PC"],
            "ApplyToPlatformGroups": ["riot"],
            "InfractionID": "infraction-id",
            "Origin": "automated",
            "ForgivenessIneligible": false,
            "IsAutomatedDetection": true,
            "PenaltyInfo": {"kind": "test"},
            "DelayedPenaltyEffect": null,
            "GameBanEffect": null,
            "QueueDelayEffect": null,
            "QueueRestrictionEffect": {
                "QueueIDs": ["competitive"],
                "RecordID": "record-id"
            },
            "RankedRatingPenaltyEffect": null,
            "RiotRestrictionEffect": null,
            "RMSNotifyEffect": null,
            "WarningEffect": null,
            "XPMultiplierEffect": null,
            "PremierRestrictionEffect": {
                "RestrictionType": "premier",
                "SeasonID": "season-id",
                "Source": "source"
            }
        }],
        "Infractions": [{
            "ID": "infraction-id",
            "Name": "queue dodge",
            "RatingName": "Queue Dodge"
        }],
        "Version": 7
    });

    let penalties: PlayerPenaltiesResponse =
        serde_json::from_value(json).expect("penalties response");

    assert_eq!(penalties.subject, "puuid");
    assert_eq!(penalties.version, 7);
    assert_eq!(
        penalties.infractions[0].rating_name,
        "Queue Dodge".to_string()
    );
}

fn core_game_match_json(matchmaking_data: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "MatchID": "match",
        "Version": 1,
        "State": "IN_PROGRESS",
        "MapID": "/Game/Maps/Ascent/Ascent",
        "ModeID": "/Game/GameModes/Bomb/BombGameMode.BombGameMode_C",
        "ProvisioningFlow": "Matchmaking",
        "GamePodID": "aresriot.aws-rclusterprod-use1-1.na-gp-ashburn-1",
        "AllMUCName": "all",
        "TeamMUCName": "team",
        "TeamVoiceID": "voice",
        "IsReconnectable": true,
        "ConnectionDetails": {},
        "PostGameDetails": null,
        "Players": [{
            "Subject": "self",
            "TeamID": "Blue",
            "CharacterID": "agent",
            "PlayerIdentity": {
                "Subject": "self",
                "PlayerCardID": "card",
                "PlayerTitleID": "title",
                "AccountLevel": 214,
                "PreferredLevelBorderID": "border",
                "Incognito": true,
                "HideAccountLevel": false
            },
            "SeasonalBadgeInfo": {},
            "IsCoach": false,
            "IsAssociated": true
        }],
        "MatchmakingData": matchmaking_data
    })
}

#[test]
fn deserializes_core_game_match_with_and_without_matchmaking_data() {
    let without: CoreGameMatchResponse =
        serde_json::from_value(core_game_match_json(serde_json::Value::Null))
            .expect("match without matchmaking data");
    assert_eq!(without.matchmaking_data, None);
    assert_eq!(without.map_id, "/Game/Maps/Ascent/Ascent");
    let player = &without.players[0];
    assert_eq!(player.team_id, "Blue");
    assert!(player.player_identity.incognito);
    assert_eq!(player.player_identity.account_level, 214);

    let with: CoreGameMatchResponse = serde_json::from_value(core_game_match_json(
        serde_json::json!({"QueueID": "competitive", "IsRanked": true}),
    ))
    .expect("match with matchmaking data");
    assert_eq!(
        with.matchmaking_data
            .and_then(|data| data.queue_id)
            .as_deref(),
        Some("competitive")
    );
}

#[test]
fn deserializes_pregame_match_ally_team() {
    let json = serde_json::json!({
        "ID": "match",
        "Version": 1,
        "Teams": [],
        "AllyTeam": {
            "TeamID": "Red",
            "Players": [{
                "Subject": "self",
                "CharacterID": "",
                "CharacterSelectionState": "",
                "PregamePlayerState": "joined",
                "CompetitiveTier": 0,
                "PlayerIdentity": {"PlayerCardID": "card", "AccountLevel": 3},
                "IsCaptain": false
            }]
        },
        "EnemyTeam": null,
        "MapID": "/Game/Maps/Bonsai/Bonsai",
        "GamePodID": "pod",
        "Mode": "/Game/GameModes/Bomb/BombGameMode.BombGameMode_C",
        "QueueID": "unrated",
        "ProvisioningFlowID": "Matchmaking",
        "IsRanked": false
    });

    let pregame: PregameMatchResponse = serde_json::from_value(json).expect("pregame");

    assert_eq!(pregame.queue_id, "unrated");
    let players = pregame.ally_team.expect("ally team").players;
    assert_eq!(players[0].subject, "self");
    assert_eq!(players[0].character_selection_state, "");
    assert!(!players[0].player_identity.incognito);
}

fn loadout_json() -> serde_json::Value {
    serde_json::json!({
        "Subject": "self",
        "Sprays": {},
        "Items": {
            "9C82E19D-4575-0200-1A81-3EACF00CF872": {
                "ID": "9c82e19d-4575-0200-1a81-3eacf00cf872",
                "TypeID": "weapon",
                "Sockets": {
                    "bcef87d6-209b-46c6-8b19-fbe40bd95abc": {
                        "ID": "bcef87d6-209b-46c6-8b19-fbe40bd95abc",
                        "Item": {"ID": "skin", "TypeID": "skin-type"}
                    }
                }
            }
        }
    })
}

#[test]
fn reads_match_loadouts_in_both_shapes() {
    let core: CoreGameLoadoutsResponse = serde_json::from_value(serde_json::json!({
        "Loadouts": [{"CharacterID": "agent", "Loadout": loadout_json()}]
    }))
    .expect("core-game loadouts");
    let pregame: PregameLoadoutsResponse = serde_json::from_value(serde_json::json!({
        "Loadouts": [loadout_json()],
        "LoadoutsValid": true
    }))
    .expect("pregame loadouts");

    assert!(pregame.loadouts_valid);
    for loadout in [&core.loadouts[0].loadout, &pregame.loadouts[0]] {
        assert_eq!(loadout.subject, "self");
        assert_eq!(
            loadout.socket_item(
                "9c82e19d-4575-0200-1a81-3eacf00cf872",
                "BCEF87D6-209B-46C6-8B19-FBE40BD95ABC"
            ),
            Some("skin")
        );
        assert_eq!(
            loadout.socket_item("other-weapon", "bcef87d6-209b-46c6-8b19-fbe40bd95abc"),
            None
        );
    }
}

#[test]
fn deserializes_name_service_entries() {
    let entries: Vec<NameServiceEntry> = serde_json::from_value(serde_json::json!([{
        "DisplayName": "",
        "Subject": "self",
        "GameName": "Player",
        "TagLine": "NA1"
    }]))
    .expect("names");

    assert_eq!(entries[0].game_name, "Player");
    assert_eq!(entries[0].tag_line, "NA1");
}
