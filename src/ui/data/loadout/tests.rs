use super::*;

use crate::riot::content::{
    AccessoryCatalog, ContractCatalog, ContractChapter, ContractContent, ContractLevel,
    ContractReward, Currency, CurrencyCatalog, PlayerCard, SkinCatalog, ValorantContract,
    WeaponCatalog,
};
use crate::riot::models::{ContractsResponse, GameContentResponse, PlayerLoadoutResponse};

use crate::riot::content::Flex;

#[test]
fn totem_rewards_resolve_to_flex_items() {
    let accessories =
        AccessoryCatalog::from_parts(vec![], vec![], vec![], vec![]).with_flex(vec![Flex {
            uuid: "flex-uuid".to_string(),
            display_name: "ORA Flex".to_string(),
            display_icon: Some("flex-icon".to_string()),
        }]);
    let reward = ResolvedContractReward {
        kind: "Totem".to_string(),
        uuid: "FLEX-UUID".to_string(),
        amount: 1,
    };

    let resolved = resolve_battle_pass_reward(
        &reward,
        &SkinCatalog::default(),
        &accessories,
        &CurrencyCatalog::default(),
    );

    assert_eq!(resolved.name, "ORA Flex");
    assert_eq!(resolved.kind, "Flex");
    assert_eq!(resolved.display_icon.as_deref(), Some("flex-icon"));
}

#[test]
fn loadout_summary_resolves_skin_names() {
    let response: PlayerLoadoutResponse = serde_json::from_value(serde_json::json!({
        "Subject": "puuid",
        "Version": 1,
        "Guns": [{
            "ID": "weapon",
            "SkinID": "skin-a",
            "SkinLevelID": "level-a",
            "ChromaID": "chroma-a",
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
    }))
    .expect("loadout");
    let catalog = SkinCatalog::from_skins(vec![crate::riot::content::WeaponSkin {
        uuid: "skin-a".to_string(),
        display_name: "Prime Vandal".to_string(),
        display_icon: None,
        content_tier_uuid: None,
        levels: vec![
            crate::riot::content::WeaponSkinLevel {
                uuid: "base-level".to_string(),
                display_name: "Prime Vandal".to_string(),
                display_icon: None,
            },
            crate::riot::content::WeaponSkinLevel {
                uuid: "level-a".to_string(),
                display_name: "Prime Vandal Level 3".to_string(),
                display_icon: None,
            },
        ],
        chromas: vec![],
    }]);
    let weapons = WeaponCatalog::from_weapons(vec![crate::riot::content::Weapon {
        uuid: "weapon".to_string(),
        display_name: "Vandal".to_string(),
        display_icon: None,
        category: Some("EEquippableCategory::Rifle".to_string()),
        default_skin_uuid: None,
        skins: vec![],
    }]);

    let summary = LoadoutSummary::from_response(response, &catalog, &weapons, None);

    assert_eq!(
        summary.gun_skins[0].label(),
        "Vandal: Prime Vandal - Level 3"
    );
    assert_eq!(summary.gun_skins[0].weapon.category, "Rifles");
    // A skin with no tier, like a battle pass skin, is still one the account got.
    assert!(!summary.gun_skins[0].default_skin);
}

#[test]
fn a_weapons_own_default_skin_is_marked_default() {
    let weapons = WeaponCatalog::from_weapons(vec![crate::riot::content::Weapon {
        uuid: "weapon".to_string(),
        display_name: "Vandal".to_string(),
        display_icon: None,
        category: None,
        default_skin_uuid: Some("STANDARD-SKIN".to_string()),
        skins: vec![],
    }]);

    let summary = LoadoutSummary::from_response(
        single_gun_loadout("standard-skin", "level", "chroma"),
        &SkinCatalog::default(),
        &weapons,
        None,
    );

    assert!(summary.gun_skins[0].default_skin);
}

fn single_gun_loadout(skin_id: &str, level_id: &str, chroma_id: &str) -> PlayerLoadoutResponse {
    serde_json::from_value(serde_json::json!({
        "Subject": "puuid",
        "Version": 1,
        "Guns": [{
            "ID": "weapon",
            "SkinID": skin_id,
            "SkinLevelID": level_id,
            "ChromaID": chroma_id,
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
    }))
    .expect("loadout")
}

fn weapon_catalog(name: &str) -> WeaponCatalog {
    WeaponCatalog::from_weapons(vec![crate::riot::content::Weapon {
        uuid: "weapon".to_string(),
        display_name: name.to_string(),
        display_icon: None,
        category: Some("EEquippableCategory::Rifle".to_string()),
        default_skin_uuid: None,
        skins: vec![],
    }])
}

fn skin_level(uuid: &str, name: &str) -> crate::riot::content::WeaponSkinLevel {
    crate::riot::content::WeaponSkinLevel {
        uuid: uuid.to_string(),
        display_name: name.to_string(),
        display_icon: None,
    }
}

fn skin_chroma(uuid: &str, name: &str) -> crate::riot::content::WeaponSkinChroma {
    crate::riot::content::WeaponSkinChroma {
        uuid: uuid.to_string(),
        display_name: name.to_string(),
        display_icon: None,
        full_render: None,
    }
}

#[test]
fn loadout_label_names_the_equipped_variant() {
    let catalog = SkinCatalog::from_skins(vec![crate::riot::content::WeaponSkin {
        uuid: "skin".to_string(),
        display_name: "Prime Vandal".to_string(),
        display_icon: None,
        content_tier_uuid: None,
        levels: vec![
            skin_level("level-1", "Prime Vandal"),
            skin_level("level-4", "Prime Vandal Level 4"),
        ],
        chromas: vec![
            skin_chroma("chroma-base", "Prime Vandal"),
            skin_chroma(
                "chroma-orange",
                "Prime Vandal Level 4\r\n(Variant 1 Orange)",
            ),
        ],
    }]);

    let summary = LoadoutSummary::from_response(
        single_gun_loadout("skin", "level-4", "chroma-orange"),
        &catalog,
        &weapon_catalog("Vandal"),
        None,
    );

    assert_eq!(
        summary.gun_skins[0].label(),
        "Vandal: Prime Vandal - Level 4 - Orange"
    );
}

#[test]
fn loadout_labels_of_single_level_skins_have_no_level() {
    let catalog = SkinCatalog::from_skins(vec![
        crate::riot::content::WeaponSkin {
            uuid: "velocity".to_string(),
            display_name: "Velocity Shorty".to_string(),
            display_icon: None,
            content_tier_uuid: None,
            levels: vec![skin_level("velocity-level", "Velocity Shorty")],
            chromas: vec![skin_chroma("velocity-chroma", "Velocity Shorty")],
        },
        crate::riot::content::WeaponSkin {
            uuid: "standard".to_string(),
            display_name: "Standard Bandit".to_string(),
            display_icon: None,
            content_tier_uuid: None,
            levels: vec![skin_level("standard-level", "Bandit")],
            chromas: vec![skin_chroma("standard-chroma", "Bandit")],
        },
    ]);

    let velocity = LoadoutSummary::from_response(
        single_gun_loadout("velocity", "velocity-level", "velocity-chroma"),
        &catalog,
        &weapon_catalog("Shorty"),
        None,
    );
    let standard = LoadoutSummary::from_response(
        single_gun_loadout("standard", "standard-level", "standard-chroma"),
        &catalog,
        &weapon_catalog("Bandit"),
        None,
    );

    assert_eq!(velocity.gun_skins[0].label(), "Shorty: Velocity Shorty");
    assert_eq!(standard.gun_skins[0].label(), "Bandit: Standard Bandit");
}

#[test]
fn loadout_weapon_categories_come_from_the_catalog() {
    assert_eq!(
        weapon_category(Some("EEquippableCategory::Sidearm")),
        "Sidearms"
    );
    assert_eq!(
        weapon_category(Some("EEquippableCategory::Sniper")),
        "Sniper Rifles"
    );
    assert_eq!(
        weapon_category(Some("EEquippableCategory::Unknown")),
        "Other"
    );
    assert_eq!(weapon_category(None), "Other");
    assert!(weapon_order("Bandit") < weapon_order("Stinger"));
    assert!(weapon_order("Outlaw") < weapon_order("Operator"));
}

#[test]
fn loadout_summary_prefers_current_chroma_render() {
    let response: PlayerLoadoutResponse = serde_json::from_value(serde_json::json!({
        "Subject": "puuid",
        "Version": 1,
        "Guns": [{
            "ID": "weapon",
            "SkinID": "skin-a",
            "SkinLevelID": "level-a",
            "ChromaID": "chroma-a",
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
    }))
    .expect("loadout");
    let catalog = SkinCatalog::from_skins(vec![crate::riot::content::WeaponSkin {
        uuid: "skin-a".to_string(),
        display_name: "Prime Vandal".to_string(),
        display_icon: Some("skin-icon".to_string()),
        content_tier_uuid: None,
        levels: vec![
            crate::riot::content::WeaponSkinLevel {
                uuid: "base-level".to_string(),
                display_name: "Prime Vandal".to_string(),
                display_icon: None,
            },
            crate::riot::content::WeaponSkinLevel {
                uuid: "level-a".to_string(),
                display_name: "Prime Vandal Level 4".to_string(),
                display_icon: None,
            },
        ],
        chromas: vec![crate::riot::content::WeaponSkinChroma {
            uuid: "chroma-a".to_string(),
            display_name: "Prime Vandal Blue".to_string(),
            display_icon: Some("chroma-display-icon".to_string()),
            full_render: Some("chroma-render".to_string()),
        }],
    }]);
    let weapons = WeaponCatalog::from_weapons(vec![crate::riot::content::Weapon {
        uuid: "weapon".to_string(),
        display_name: "Vandal".to_string(),
        display_icon: Some("weapon-icon".to_string()),
        category: Some("EEquippableCategory::Rifle".to_string()),
        default_skin_uuid: None,
        skins: vec![],
    }]);

    let summary = LoadoutSummary::from_response(response, &catalog, &weapons, None);

    assert_eq!(summary.gun_skins[0].skin.uuid, "chroma-a");
    assert_eq!(
        summary.gun_skins[0].skin.display_icon.as_deref(),
        Some("chroma-render")
    );
    assert_eq!(
        summary.gun_skins[0].skin_detail_label(),
        "Prime Vandal - Level 4"
    );
}

#[test]
fn loadout_summary_prefers_account_xp_level() {
    let response: PlayerLoadoutResponse = serde_json::from_value(serde_json::json!({
        "Subject": "puuid",
        "Version": 1,
        "Guns": [],
        "Sprays": [],
        "Identity": {
            "PlayerCardID": "card",
            "PlayerTitleID": "title",
            "AccountLevel": 0,
            "PreferredLevelBorderID": "border",
            "HideAccountLevel": false
        },
        "Incognito": false
    }))
    .expect("loadout");

    let summary = LoadoutSummary::from_response(
        response,
        &SkinCatalog::default(),
        &WeaponCatalog::default(),
        Some(88),
    );

    assert_eq!(summary.account_level, Some(88));
}

#[test]
fn loadout_summary_reports_no_level_instead_of_zero() {
    let response: PlayerLoadoutResponse = serde_json::from_value(serde_json::json!({
        "Subject": "puuid",
        "Version": 1,
        "Guns": [],
        "Identity": {
            "PlayerCardID": "card",
            "PlayerTitleID": "title",
            "AccountLevel": 0,
            "PreferredLevelBorderID": "border",
            "HideAccountLevel": true
        },
        "Incognito": false
    }))
    .expect("loadout");

    let summary = LoadoutSummary::from_response(
        response,
        &SkinCatalog::default(),
        &WeaponCatalog::default(),
        Some(0),
    );

    assert_eq!(summary.account_level, None);
    assert_eq!(
        LoadoutSummary::without_loadout("down".to_string(), Some(0)).account_level,
        None
    );
}

#[test]
fn battle_pass_progress_uses_story_contract_and_active_act() {
    let contracts: ContractsResponse = serde_json::from_value(serde_json::json!({
        "Version": 1,
        "Subject": "puuid",
        "Contracts": [{
            "ContractDefinitionID": "battle-pass",
            "ContractProgression": {
                "TotalProgressionEarned": 4_500,
                "TotalProgressionEarnedVersion": 1,
                "HighestRewardedLevel": {}
            },
            "ProgressionLevelReached": 2,
            "ProgressionTowardsNextLevel": 2_500,
            "ProgressionCompleted": false
        }],
        "ActiveSpecialContract": ""
    }))
    .expect("contracts");
    let catalog = ContractCatalog::from_contracts(vec![ValorantContract {
        uuid: Some("battle-pass".to_string()),
        display_name: Some("Season 2026 // Act III".to_string()),
        free_reward_schedule_uuid: Some("free-schedule".to_string()),
        content: Some(ContractContent {
            relation_type: Some("Season".to_string()),
            relation_uuid: Some("act".to_string()),
            premium_reward_schedule_uuid: Some("premium-schedule".to_string()),
            chapters: vec![ContractChapter {
                is_epilogue: false,
                levels: vec![
                    ContractLevel {
                        reward: None,
                        xp: Some(0),
                    },
                    ContractLevel {
                        reward: None,
                        xp: Some(2_000),
                    },
                    ContractLevel {
                        reward: None,
                        xp: Some(3_000),
                    },
                    ContractLevel {
                        reward: None,
                        xp: Some(4_000),
                    },
                ],
                free_rewards: None,
            }],
        }),
    }]);
    let content: GameContentResponse = serde_json::from_value(serde_json::json!({
        "DisabledIDs": [],
        "Seasons": [{
            "ID": "act",
            "Name": "Act 3",
            "Type": "act",
            "StartTime": "2026-05-01T00:00:00Z",
            "EndTime": "2099-06-24T13:00:00Z",
            "IsActive": true
        }],
        "Events": []
    }))
    .expect("content");

    let progress = battle_pass_progress_from_responses(
        &contracts,
        &catalog,
        Some(&content),
        &SkinCatalog::default(),
        &AccessoryCatalog::default(),
        &CurrencyCatalog::default(),
    )
    .expect("battle pass progress");

    assert_eq!(progress.title(), "Act 3 Battle Pass");
    assert_eq!(progress.tier_display(), ("TIER", 2, Some(4)));
    assert_eq!(progress.next_tier_label(), "2,500 / 3,000 XP");
    assert!((progress.progress_fraction() - 0.5).abs() < f32::EPSILON);
    assert!(progress.remaining_seconds.is_some());
}

#[test]
fn epilogue_tiers_are_counted_apart_from_the_main_pass() {
    let contracts: ContractsResponse = serde_json::from_value(serde_json::json!({
        "Version": 1,
        "Subject": "puuid",
        "Contracts": [{
            "ContractDefinitionID": "battle-pass",
            "ContractProgression": {
                "TotalProgressionEarned": 15_000,
                "TotalProgressionEarnedVersion": 1,
                "HighestRewardedLevel": {}
            },
            "ProgressionLevelReached": 5,
            "ProgressionTowardsNextLevel": 1_000,
            "ProgressionCompleted": false
        }],
        "ActiveSpecialContract": ""
    }))
    .expect("contracts");
    let level = |xp| ContractLevel {
        reward: None,
        xp: Some(xp),
    };
    let catalog = ContractCatalog::from_contracts(vec![ValorantContract {
        uuid: Some("battle-pass".to_string()),
        display_name: Some("Battle Pass".to_string()),
        free_reward_schedule_uuid: None,
        content: Some(ContractContent {
            relation_type: Some("Season".to_string()),
            relation_uuid: Some("act".to_string()),
            premium_reward_schedule_uuid: None,
            chapters: vec![
                ContractChapter {
                    is_epilogue: false,
                    levels: vec![level(0), level(2_000), level(3_000), level(4_000)],
                    free_rewards: None,
                },
                ContractChapter {
                    is_epilogue: true,
                    levels: vec![level(5_000), level(5_000)],
                    free_rewards: None,
                },
            ],
        }),
    }]);

    let progress = battle_pass_progress_from_responses(
        &contracts,
        &catalog,
        None,
        &SkinCatalog::default(),
        &AccessoryCatalog::default(),
        &CurrencyCatalog::default(),
    )
    .expect("battle pass progress");

    assert_eq!(progress.tier_display(), ("EPILOGUE", 1, Some(2)));
    let epilogue = &progress.chapters[1];
    assert_eq!(epilogue.name(), "Epilogue 1");
    assert_eq!(epilogue.short_name(), "EP 1");
    assert_eq!(progress.chapter_tiers_label(epilogue), "Epilogue 1–2");
    assert_eq!(progress.selected_chapter, 1);
    assert!((progress.progress_fraction() - 1.0).abs() < f32::EPSILON);
    assert_eq!(progress.next_tier_label(), "1,000 / 5,000 XP");
}

#[test]
fn an_older_battle_pass_does_not_borrow_the_current_acts_name_or_countdown() {
    let contracts: ContractsResponse = serde_json::from_value(serde_json::json!({
        "Version": 1,
        "Subject": "puuid",
        "Contracts": [{
            "ContractDefinitionID": "old-battle-pass",
            "ContractProgression": {
                "TotalProgressionEarned": 2_000,
                "TotalProgressionEarnedVersion": 1,
                "HighestRewardedLevel": {}
            },
            "ProgressionLevelReached": 1,
            "ProgressionTowardsNextLevel": 0,
            "ProgressionCompleted": false
        }],
        "ActiveSpecialContract": ""
    }))
    .expect("contracts");
    let catalog = ContractCatalog::from_contracts(vec![ValorantContract {
        uuid: Some("old-battle-pass".to_string()),
        display_name: Some("Season 2025 // Act VI".to_string()),
        free_reward_schedule_uuid: None,
        content: Some(ContractContent {
            relation_type: Some("Season".to_string()),
            relation_uuid: Some("old-act".to_string()),
            premium_reward_schedule_uuid: None,
            chapters: vec![ContractChapter {
                is_epilogue: false,
                levels: vec![
                    ContractLevel {
                        reward: None,
                        xp: Some(2_000),
                    },
                    ContractLevel {
                        reward: None,
                        xp: Some(3_000),
                    },
                ],
                free_rewards: None,
            }],
        }),
    }]);
    let content: GameContentResponse = serde_json::from_value(serde_json::json!({
        "DisabledIDs": [],
        "Seasons": [{
            "ID": "act",
            "Name": "Act 3",
            "Type": "act",
            "StartTime": "2026-05-01T00:00:00Z",
            "EndTime": "2099-06-24T13:00:00Z",
            "IsActive": true
        }],
        "Events": []
    }))
    .expect("content");

    let progress = battle_pass_progress_from_responses(
        &contracts,
        &catalog,
        Some(&content),
        &SkinCatalog::default(),
        &AccessoryCatalog::default(),
        &CurrencyCatalog::default(),
    )
    .expect("battle pass progress");

    assert_eq!(progress.title(), "Season 2025 // Act VI");
    assert_eq!(progress.remaining_seconds, None);
}

#[test]
fn battle_pass_rewards_group_by_chapter_and_premium_ones_need_the_premium_pass() {
    let contracts: ContractsResponse = serde_json::from_value(serde_json::json!({
        "Version": 1,
        "Subject": "puuid",
        "Contracts": [{
            "ContractDefinitionID": "battle-pass",
            "ContractProgression": {
                "TotalProgressionEarned": 2_000,
                "TotalProgressionEarnedVersion": 1,
                "HighestRewardedLevel": {
                    "free-schedule": { "Amount": 1, "Version": 1 }
                }
            },
            "ProgressionLevelReached": 2,
            "ProgressionTowardsNextLevel": 0,
            "ProgressionCompleted": false
        }],
        "ActiveSpecialContract": ""
    }))
    .expect("contracts");
    let catalog = ContractCatalog::from_contracts(vec![ValorantContract {
        uuid: Some("battle-pass".to_string()),
        display_name: Some("Season 2026 // Act III".to_string()),
        free_reward_schedule_uuid: Some("free-schedule".to_string()),
        content: Some(ContractContent {
            relation_type: Some("Season".to_string()),
            relation_uuid: Some("act".to_string()),
            premium_reward_schedule_uuid: Some("premium-schedule".to_string()),
            chapters: vec![
                ContractChapter {
                    is_epilogue: false,
                    levels: vec![
                        ContractLevel {
                            reward: Some(ContractReward {
                                kind: "EquippableSkinLevel".to_string(),
                                uuid: "paid-tier-one".to_string(),
                                amount: 1,
                            }),
                            xp: Some(0),
                        },
                        ContractLevel {
                            reward: Some(ContractReward {
                                kind: "EquippableSkinLevel".to_string(),
                                uuid: "paid-tier-two".to_string(),
                                amount: 1,
                            }),
                            xp: Some(2_000),
                        },
                    ],
                    free_rewards: Some(vec![ContractReward {
                        kind: "Title".to_string(),
                        uuid: "free-title".to_string(),
                        amount: 1,
                    }]),
                },
                ContractChapter {
                    is_epilogue: false,
                    levels: vec![ContractLevel {
                        reward: None,
                        xp: Some(3_000),
                    }],
                    free_rewards: Some(vec![ContractReward {
                        kind: "Title".to_string(),
                        uuid: "future-free-title".to_string(),
                        amount: 1,
                    }]),
                },
            ],
        }),
    }]);
    let content: GameContentResponse = serde_json::from_value(serde_json::json!({
        "DisabledIDs": [],
        "Seasons": [{
            "ID": "act",
            "Name": "Act 3",
            "Type": "act",
            "StartTime": "2026-05-01T00:00:00Z",
            "EndTime": "2099-06-24T13:00:00Z",
            "IsActive": true
        }],
        "Events": []
    }))
    .expect("content");

    let progress = battle_pass_progress_from_responses(
        &contracts,
        &catalog,
        Some(&content),
        &SkinCatalog::default(),
        &AccessoryCatalog::default(),
        &CurrencyCatalog::default(),
    )
    .expect("battle pass progress");

    let names = |chapter: &BattlePassChapterDisplay| -> Vec<String> {
        chapter
            .rewards
            .iter()
            .map(|reward| reward.name.clone())
            .collect()
    };
    assert_eq!(progress.chapters.len(), 2);
    // Each tier's premium reward, then the chapter's free ones.
    assert_eq!(
        names(&progress.chapters[0]),
        ["paid-tier-one", "paid-tier-two", "free-title"]
    );
    assert_eq!(names(&progress.chapters[1]), ["future-free-title"]);
    assert_eq!(
        progress.chapter_tiers_label(&progress.chapters[0]),
        "Tiers 1–2"
    );

    let earned: Vec<_> = progress.chapters[0]
        .rewards
        .iter()
        .map(|reward| progress.is_earned(reward))
        .collect();
    assert_eq!(earned, [false, false, true]);
    assert!(progress.is_locked(&progress.chapters[0].rewards[0]));
    assert!(!progress.is_earned(&progress.chapters[1].rewards[0]));
    // Tier 3 is next, so its chapter opens first.
    assert_eq!(progress.next_tier(), Some(3));
    assert_eq!(progress.selected_chapter, 1);
    assert_eq!(progress.pass_label(), "Free");
}

#[test]
fn battle_pass_currency_rewards_show_amount_in_name() {
    let contracts: ContractsResponse = serde_json::from_value(serde_json::json!({
        "Version": 1,
        "Subject": "puuid",
        "Contracts": [{
            "ContractDefinitionID": "battle-pass",
            "ContractProgression": {
                "TotalProgressionEarned": 2_000,
                "TotalProgressionEarnedVersion": 1,
                "HighestRewardedLevel": {
                    "premium-schedule": { "Amount": 1, "Version": 1 }
                }
            },
            "ProgressionLevelReached": 1,
            "ProgressionTowardsNextLevel": 0,
            "ProgressionCompleted": false
        }],
        "ActiveSpecialContract": ""
    }))
    .expect("contracts");
    let radianite_uuid = "e59aa87c-4cbf-517a-5983-6e81511be9b7";
    let catalog = ContractCatalog::from_contracts(vec![ValorantContract {
        uuid: Some("battle-pass".to_string()),
        display_name: Some("Season 2026 // Act III".to_string()),
        free_reward_schedule_uuid: Some("free-schedule".to_string()),
        content: Some(ContractContent {
            relation_type: Some("Season".to_string()),
            relation_uuid: Some("act".to_string()),
            premium_reward_schedule_uuid: Some("premium-schedule".to_string()),
            chapters: vec![ContractChapter {
                is_epilogue: false,
                levels: vec![
                    ContractLevel {
                        reward: Some(ContractReward {
                            kind: "Currency".to_string(),
                            uuid: radianite_uuid.to_string(),
                            amount: 1,
                        }),
                        xp: Some(2_000),
                    },
                    ContractLevel {
                        reward: Some(ContractReward {
                            kind: "PlayerCard".to_string(),
                            uuid: "card".to_string(),
                            amount: 1,
                        }),
                        xp: Some(3_000),
                    },
                ],
                free_rewards: None,
            }],
        }),
    }]);
    let content: GameContentResponse = serde_json::from_value(serde_json::json!({
        "DisabledIDs": [],
        "Seasons": [{
            "ID": "act",
            "Name": "Act 3",
            "Type": "act",
            "StartTime": "2026-05-01T00:00:00Z",
            "EndTime": "2099-06-24T13:00:00Z",
            "IsActive": true
        }],
        "Events": []
    }))
    .expect("content");
    let currencies = CurrencyCatalog::from_currencies(vec![Currency {
        uuid: radianite_uuid.to_string(),
        display_name: "Radianite Points".to_string(),
        display_icon: None,
    }]);

    let accessories = AccessoryCatalog::from_parts(
        vec![],
        vec![],
        vec![PlayerCard {
            uuid: "card".to_string(),
            display_name: "Gourmand Card".to_string(),
            display_icon: Some("card-icon".to_string()),
            small_art: Some("card-small".to_string()),
            large_art: Some("card-large".to_string()),
        }],
        vec![],
    );

    let progress = battle_pass_progress_from_responses(
        &contracts,
        &catalog,
        Some(&content),
        &SkinCatalog::default(),
        &accessories,
        &currencies,
    )
    .expect("battle pass progress");

    // Player cards use their wide banner, which fills a reward tile; the viewer opens the tall art.
    let card = &progress.chapters[0].rewards[1];
    assert_eq!(card.name, "Gourmand Card");
    assert_eq!(
        card.display_icon.as_deref(),
        Some("https://media.valorant-api.com/playercards/card/wideart.png")
    );
    assert_eq!(card.viewer_icon.as_deref(), Some("card-large"));

    let reward = &progress.chapters[0].rewards[0];
    assert_eq!(reward.name, "10 Radianite");
    assert!(progress.is_earned(reward));
    assert_eq!(reward.amount_label(), None);
    assert_eq!(progress.reward_tier_label(reward), "Tier 1");
}

/// Ten chapters of 5 tiers, each tier with a spray but tiers 23, 33, 43 and 48, which have weapon
/// skins. Tier 12 is reached.
pub(in crate::ui) fn battle_pass_with_chapters() -> BattlePassProgressDisplay {
    let reward = |tier: i64| BattlePassRewardDisplay {
        tier,
        is_epilogue: false,
        free: false,
        uuid: format!("reward-{tier}"),
        name: format!("Reward {tier}"),
        kind: if [23, 33, 43, 48].contains(&tier) {
            "Weapon skin"
        } else {
            "Spray"
        }
        .to_string(),
        amount: 1,
        display_icon: Some(format!("https://example.test/{tier}.png")),
        viewer_icon: None,
        cached_icon: None,
    };
    BattlePassProgressDisplay {
        level_reached: 12,
        chapters: (0..10)
            .map(|index| BattlePassChapterDisplay {
                number: index + 1,
                is_epilogue: false,
                first_tier: index * 5 + 1,
                last_tier: index * 5 + 5,
                rewards: (index * 5 + 1..=index * 5 + 5).map(reward).collect(),
                art_loading: false,
            })
            .collect(),
        selected_chapter: 2,
        ..battle_pass_display()
    }
}

#[test]
fn only_the_battle_pass_rewards_on_screen_get_their_art_downloaded() {
    let mut battle_pass = battle_pass_with_chapters();

    let shown: Vec<i64> = battle_pass
        .shown_rewards_mut()
        .map(|reward| reward.tier)
        .collect();

    // The open chapter, then the next 3 weapon skins not reached yet, which show below it.
    assert_eq!(shown, [11, 12, 13, 14, 15, 23, 33, 43]);
}

#[test]
fn a_failed_battle_pass_does_not_hide_the_loadout() {
    let summary = combine_loadout_sections(Ok(loaded_loadout()), Err("500".to_string()), None)
        .expect("loadout");

    assert_eq!(summary.loadout_error, None);
    assert_eq!(summary.battle_pass_error.as_deref(), Some("500"));
}

#[test]
fn a_failed_loadout_does_not_hide_the_battle_pass() {
    let summary = combine_loadout_sections(
        Err("weapon content unavailable".to_string()),
        Ok(Some(battle_pass_display())),
        Some(12),
    )
    .expect("battle pass");

    assert_eq!(
        summary.loadout_error.as_deref(),
        Some("weapon content unavailable")
    );
    assert!(summary.battle_pass.is_some());
    assert_eq!(summary.account_level, Some(12));
}

#[test]
fn when_loadout_and_battle_pass_both_fail_both_errors_are_reported() {
    let error = combine_loadout_sections(
        Err("loadout 404".to_string()),
        Err("contracts 500".to_string()),
        None,
    )
    .expect_err("both failed");

    assert!(error.contains("loadout 404"), "{error}");
    assert!(error.contains("contracts 500"), "{error}");
}

#[test]
fn a_failed_loadout_with_no_battle_pass_progress_is_only_a_loadout_error() {
    let summary = combine_loadout_sections(Err("loadout 500".to_string()), Ok(None), None)
        .expect("no progress isn't a failure");

    assert_eq!(summary.loadout_error.as_deref(), Some("loadout 500"));
    assert_eq!(summary.battle_pass, None);
    assert_eq!(summary.battle_pass_error, None);
}

pub(in crate::ui) fn loaded_loadout() -> LoadoutSummary {
    LoadoutSummary {
        account_level: None,
        player_card_id: None,
        gun_skins: Vec::new(),
        loadout_error: None,
        battle_pass: None,
        battle_pass_error: None,
    }
}

pub(in crate::ui) fn battle_pass_display() -> BattlePassProgressDisplay {
    BattlePassProgressDisplay {
        name: "Pass".to_string(),
        season_name: None,
        level_reached: 1,
        total_levels: Some(50),
        epilogue_levels: 0,
        progression_towards_next_level: 0,
        next_level_progress_required: None,
        total_progression_earned: 0,
        total_progression_required: None,
        completed: false,
        remaining_seconds: None,
        chapters: Vec::new(),
        selected_chapter: 0,
        paid_pass_owned: false,
        loaded_at: iced::time::Instant::now(),
    }
}
