use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tempfile::tempdir;

use super::UnavailableLaunchWarning;
use super::app::{
    LaunchPreflightDecision, apply_account_detail_results, cancel_unavailable_launch_state,
    launch_preflight_decision,
};
use super::data::account_details::{
    AccountActivity, AccountActivityProbe, AccountAvailability, AccountAvailabilityRefresh,
    RefreshedApiContext, classify_account_activity, competitive_rank_from_mmr,
    penalty_status_from_response, rank_name_for_competitive_tier,
};
use super::data::cache_account_api_context;
use super::data::game_settings::{
    AppliedGameSettingsResult, RestoredGameSettingsResult, SavedGameSettingsResult,
};
use super::data::launch_flow::CapturedAccountDraft;
use super::data::launch_flow::{
    LaunchAccountResult, is_pending_launcher_capture_error, load_accounts, require_launcher_session,
};
use super::data::loadout::{
    BattlePassProgressDisplay, LoadoutResult, LoadoutSummary, battle_pass_progress_from_responses,
    combine_loadout_sections, weapon_category, weapon_order,
};
use super::data::non_empty_path;
use super::data::session::{
    ApiIdentity, api_identity, needs_player_info, needs_player_info_after_geo,
};
use super::data::shop::{
    StoreAccessoryDisplay, StoreBundleDisplay, StoreOfferDisplay, StoreSummary, format_whole_number,
};
use super::{
    Message, PendingSettingsChange, PendingSettingsCheck, PresetNamePrompt, PresetNameTarget,
    PrimeApp, SettingsChange, countdown_timer_active, loading_status_active,
    masked_account_export_payload, status_bar_visible, status_message_is_error,
    status_spinner_active, status_visible_at,
};
use crate::account::{
    AccountId, AccountPenalty, AccountPenaltyDuration, AccountPenaltyStatus, AccountProfile,
    AuthSession, CompetitiveRank, LauncherSessionBackup, Shard, ValorantRegion,
};
use crate::game_settings::{
    GameSettingsProfileMetadata, GameSettingsProfilePurpose, GameSettingsProfileSummary,
};
use crate::riot::content::{
    AccessoryCatalog, Buddy, BuddyLevel, BundleCatalog, ContractCatalog, ContractChapter,
    ContractContent, ContractLevel, ContractReward, Currency, CurrencyCatalog, SkinCatalog,
    ValorantContract, WeaponCatalog,
};
use crate::riot::launcher_session::{CapturedLauncherSession, LauncherSessionError};
use crate::riot::models::{
    ContractsResponse, GameContentResponse, PlayerLoadoutResponse, PlayerMmrResponse,
    PlayerPenaltiesResponse, StorefrontResponse, WalletResponse,
};
use crate::storage::{AccountRepository, StoredState};

#[cfg(not(feature = "image-viewer-testing"))]
#[test]
fn image_viewer_is_disabled_without_testing_feature() {
    assert!(!super::image_viewer_enabled());
}

#[cfg(feature = "image-viewer-testing")]
#[test]
fn image_viewer_can_be_enabled_for_testing_builds() {
    assert!(super::image_viewer_enabled());
}

fn test_app(repo_dir: &Path) -> PrimeApp {
    let (mut app, _) = PrimeApp::boot();
    app.repo = AccountRepository::new(repo_dir.join("accounts.json"));
    app.state = StoredState::default();
    app.accounts_loaded = true;
    app.pending_account = None;
    app.show_add_account_prompt = false;
    app.launcher_capture_in_progress = false;
    app.launcher_capture_kind = None;
    app.status.clear();
    app
}

fn captured_account_draft(
    backup_root: &Path,
    puuid: &str,
    game_name: &str,
    tag_line: &str,
    shard: Shard,
) -> CapturedAccountDraft {
    let account_id = AccountId::new();
    let data_dir = backup_root.join(account_id.to_string()).join("Data");
    fs::create_dir_all(&data_dir).expect("backup data dir");
    fs::write(data_dir.join("RiotGamesPrivateSettings.yaml"), "settings")
        .expect("private settings");
    CapturedAccountDraft {
        account_id,
        backup: LauncherSessionBackup {
            data_dir,
            captured_at_unix: 100,
            puuid: puuid.to_string(),
        },
        puuid: puuid.to_string(),
        game_name: Some(game_name.to_string()),
        tag_line: Some(tag_line.to_string()),
        shard,
        session: Some(AuthSession::new(
            "access",
            Some("id".to_string()),
            Some("entitlement".to_string()),
            "Bearer",
            Some(3600),
            100,
        )),
    }
}

#[test]
fn account_activity_classification_uses_priority_order() {
    assert_eq!(
        classify_account_activity(
            AccountActivityProbe::Present,
            AccountActivityProbe::Present,
            AccountActivityProbe::Present,
        ),
        AccountActivity::InMatch
    );
    assert_eq!(
        classify_account_activity(
            AccountActivityProbe::NotFound,
            AccountActivityProbe::Present,
            AccountActivityProbe::Present,
        ),
        AccountActivity::AgentSelect
    );
    assert_eq!(
        classify_account_activity(
            AccountActivityProbe::NotFound,
            AccountActivityProbe::NotFound,
            AccountActivityProbe::Present,
        ),
        AccountActivity::InLobby
    );
}

#[test]
fn account_activity_classification_treats_all_missing_as_available() {
    assert_eq!(
        classify_account_activity(
            AccountActivityProbe::NotFound,
            AccountActivityProbe::NotFound,
            AccountActivityProbe::NotFound,
        ),
        AccountActivity::Available
    );
}

#[test]
fn account_activity_classification_treats_errors_as_unknown() {
    assert_eq!(
        classify_account_activity(
            AccountActivityProbe::Failed("activity check failed".to_string()),
            AccountActivityProbe::NotFound,
            AccountActivityProbe::NotFound,
        ),
        AccountActivity::Unknown("activity check failed".to_string())
    );
    assert_eq!(
        AccountAvailability::from(AccountActivity::Unknown(
            "activity check failed".to_string()
        ))
        .label(),
        "Unknown (activity check failed)"
    );
}

#[test]
fn launch_preflight_decision_allows_available_and_unknown_but_warns_unavailable() {
    assert_eq!(
        launch_preflight_decision(&AccountAvailability::Available),
        LaunchPreflightDecision::Launch
    );
    assert_eq!(
        launch_preflight_decision(&AccountAvailability::Unknown {
            reason: "activity check failed".to_string()
        }),
        LaunchPreflightDecision::LaunchInconclusive
    );
    assert_eq!(
        launch_preflight_decision(&AccountAvailability::Unavailable {
            reason: "in match".to_string()
        }),
        LaunchPreflightDecision::WarnUnavailable
    );
}

#[test]
fn cancel_unavailable_launch_clears_launch_state() {
    let account_id = AccountId::new();
    let mut warning = Some(UnavailableLaunchWarning {
        account_id,
        display_name: "Main".to_string(),
        reason: "in match".to_string(),
    });
    let mut preflight = Some(account_id);
    let mut launching = Some(account_id);
    let mut progress_checking = true;

    cancel_unavailable_launch_state(
        &mut warning,
        &mut preflight,
        &mut launching,
        &mut progress_checking,
    );

    assert_eq!(warning, None);
    assert_eq!(preflight, None);
    assert_eq!(launching, None);
    assert!(!progress_checking);
}

#[test]
fn store_summary_counts_night_market() {
    let response: StorefrontResponse = serde_json::from_value(serde_json::json!({
        "FeaturedBundle": {
            "Bundle": {
                "ID": "bundle",
                "DataAssetID": "asset",
                "CurrencyID": "vp",
                "Items": [{
                    "Item": {
                        "ItemTypeID": "skin-type",
                        "ItemID": "a",
                        "Amount": 1
                    },
                    "BasePrice": 1775,
                    "CurrencyID": "vp",
                    "DiscountPercent": 20,
                    "DiscountedPrice": 1420,
                    "IsPromoItem": false
                }],
                "DurationRemainingInSeconds": 10
            },
            "Bundles": [],
            "BundleRemainingDurationInSeconds": 20
        },
        "SkinsPanelLayout": {
            "SingleItemOffers": ["a", "b"],
            "SingleItemStoreOffers": [{
                "OfferID": "a",
                "IsDirectPurchase": true,
                "StartDate": "2026-05-25T00:00:00Z",
                "Cost": {"vp": 1775},
                "Rewards": [{
                    "ItemTypeID": "skin-type",
                    "ItemID": "a",
                    "Quantity": 1
                }]
            }],
            "SingleItemOffersRemainingDurationInSeconds": 30
        },
        "BonusStore": {
            "BonusStoreOffers": [{
                "BonusOfferID": "bonus",
                "Offer": {
                    "OfferID": "offer",
                    "IsDirectPurchase": true,
                    "StartDate": "2026-05-25T00:00:00Z",
                    "Cost": {"vp": 1775},
                    "Rewards": [{
                        "ItemTypeID": "skin-type",
                        "ItemID": "a",
                        "Quantity": 1
                    }]
                },
                "DiscountPercent": 10,
                "DiscountCosts": {"vp": 1200},
                "IsSeen": false
            }],
            "BonusStoreRemainingDurationInSeconds": 40
        }
    }))
    .expect("response");

    let catalog = SkinCatalog::from_skins(vec![crate::riot::content::WeaponSkin {
        uuid: "skin-a".to_string(),
        display_name: "Prime Vandal".to_string(),
        display_icon: None,
        content_tier_uuid: None,
        levels: vec![crate::riot::content::WeaponSkinLevel {
            uuid: "a".to_string(),
            display_name: "Prime Vandal Level 1".to_string(),
            display_icon: None,
        }],
        chromas: vec![],
    }]);
    let currencies = CurrencyCatalog::from_currencies(vec![crate::riot::content::Currency {
        uuid: "vp".to_string(),
        display_name: "VALORANT Points".to_string(),
        display_icon: None,
    }]);
    let bundles = BundleCatalog::from_bundles(vec![crate::riot::content::Bundle {
        uuid: "asset".to_string(),
        display_name: "Give Back Bundle".to_string(),
        display_icon: Some("bundle-icon".to_string()),
        display_icon2: None,
        vertical_promo_image: None,
    }]);
    let summary = StoreSummary::from_response(response, &catalog, &bundles, &currencies);

    assert_eq!(
        summary
            .featured_bundles
            .iter()
            .map(StoreBundleDisplay::label)
            .collect::<Vec<_>>(),
        ["Give Back Bundle (1,420 VP), 1 item"]
    );
    assert_eq!(
        summary
            .daily_offers
            .iter()
            .map(StoreOfferDisplay::label)
            .collect::<Vec<_>>(),
        ["Prime Vandal Level 1 (1,775 VP)", "b"]
    );
    assert_eq!(summary.daily_remaining_seconds, 30);
    assert_eq!(summary.bundle_remaining_seconds, 20);
    assert_eq!(summary.night_market_remaining_seconds, Some(40));
    assert_eq!(
        summary
            .night_market_offers
            .iter()
            .map(StoreOfferDisplay::label)
            .collect::<Vec<_>>(),
        ["Prime Vandal Level 1 (1,775 VP -> 1,200 VP), 10% off"]
    );
}

#[test]
fn store_summary_orders_currency_balances() {
    let response: StorefrontResponse = serde_json::from_value(serde_json::json!({
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
            "SingleItemOffersRemainingDurationInSeconds": 30
        }
    }))
    .expect("response");
    let wallet: WalletResponse = serde_json::from_value(serde_json::json!({
        "Balances": {
            "85ca954a-41f2-ce94-9b45-8ca3dd39a00d": 9000,
            "85ad13f7-3d1b-5128-9eb2-7cd8ee0b5741": 1250,
            "e59aa87c-4cbf-517a-5983-6e81511be9b7": 40
        }
    }))
    .expect("wallet");

    let summary = StoreSummary::from_response_with_wallet(
        response,
        Some(wallet),
        &SkinCatalog::default(),
        &BundleCatalog::default(),
        &CurrencyCatalog::default(),
    );

    assert_eq!(
        summary
            .currency_balances
            .iter()
            .map(|balance| balance.label())
            .collect::<Vec<_>>(),
        ["1,250 VP", "40 Radianite", "9,000 Kingdom Credits"]
    );
}

#[test]
fn store_summary_includes_accessory_store_offers() {
    let response: StorefrontResponse = serde_json::from_value(serde_json::json!({
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
            "SingleItemOffersRemainingDurationInSeconds": 30
        },
        "AccessoryStore": {
            "AccessoryStoreOffers": [{
                "ContractID": "contract",
                "Offer": {
                    "OfferID": "offer",
                    "IsDirectPurchase": true,
                    "StartDate": "2026-05-25T00:00:00Z",
                    "Cost": {"kc": 2500},
                    "Rewards": [{
                        "ItemTypeID": "dd3bf334-87f3-40bd-b043-682a57a8dc3a",
                        "ItemID": "buddy-level",
                        "Quantity": 1
                    }]
                }
            }],
            "AccessoryStoreRemainingDurationInSeconds": 50,
            "StorefrontID": "storefront"
        }
    }))
    .expect("response");
    let accessories = AccessoryCatalog::from_parts(
        vec![Buddy {
            uuid: "buddy".to_string(),
            display_name: "Penguin Buddy".to_string(),
            display_icon: None,
            levels: vec![BuddyLevel {
                uuid: "buddy-level".to_string(),
                display_name: "Penguin Buddy Level 1".to_string(),
                display_icon: Some("buddy-icon".to_string()),
            }],
        }],
        vec![],
        vec![],
        vec![],
    );

    let summary = StoreSummary::from_response_with_accessories(
        response,
        &SkinCatalog::default(),
        &BundleCatalog::default(),
        &CurrencyCatalog::default(),
        &accessories,
    );

    assert_eq!(summary.accessory_remaining_seconds, Some(50));
    assert_eq!(
        summary
            .accessory_offers
            .iter()
            .map(StoreAccessoryDisplay::label)
            .collect::<Vec<_>>(),
        ["Penguin Buddy Level 1 (2,500 Kingdom Credits)"]
    );
}

fn featured_bundle_json(id: &str, remaining_seconds: i64) -> serde_json::Value {
    serde_json::json!({
        "ID": id,
        "DataAssetID": id,
        "CurrencyID": "vp",
        "Items": [],
        "DurationRemainingInSeconds": remaining_seconds
    })
}

fn summary_with_bundles(
    bundles: Vec<serde_json::Value>,
    top_level_seconds: i64,
    loaded_at: iced::time::Instant,
) -> StoreSummary {
    let response: StorefrontResponse = serde_json::from_value(serde_json::json!({
        "FeaturedBundle": {
            "Bundle": bundles[0].clone(),
            "Bundles": bundles,
            "BundleRemainingDurationInSeconds": top_level_seconds
        },
        "SkinsPanelLayout": {
            "SingleItemOffers": [],
            "SingleItemStoreOffers": [],
            "SingleItemOffersRemainingDurationInSeconds": 86_400
        }
    }))
    .expect("response");

    StoreSummary::from_response_at(
        response,
        None,
        &SkinCatalog::default(),
        &BundleCatalog::default(),
        &CurrencyCatalog::default(),
        &AccessoryCatalog::default(),
        loaded_at,
    )
}

#[test]
fn each_featured_bundle_counts_down_on_its_own() {
    let loaded_at = iced::time::Instant::now();
    let summary = summary_with_bundles(
        vec![
            featured_bundle_json("short", 3_600),
            featured_bundle_json("long", 86_400),
        ],
        86_400,
        loaded_at,
    );
    let later = loaded_at + Duration::from_secs(600);

    let remaining = summary
        .featured_bundles
        .iter()
        .map(|bundle| summary.featured_bundle_remaining_seconds_at(bundle, later))
        .collect::<Vec<_>>();

    assert_eq!(remaining, vec![3_000, 85_800]);
}

#[test]
fn a_featured_bundle_without_its_own_time_uses_the_shared_one() {
    let loaded_at = iced::time::Instant::now();
    let summary = summary_with_bundles(vec![featured_bundle_json("only", 0)], 7_200, loaded_at);

    assert_eq!(
        summary.featured_bundle_remaining_seconds_at(&summary.featured_bundles[0], loaded_at),
        7_200
    );
}

#[test]
fn the_shop_expires_when_any_featured_bundle_does() {
    let loaded_at = iced::time::Instant::now();
    let summary = summary_with_bundles(
        vec![
            featured_bundle_json("short", 60),
            featured_bundle_json("long", 86_400),
        ],
        86_400,
        loaded_at,
    );

    assert!(!summary.is_expired_at(loaded_at + Duration::from_secs(30)));
    assert!(summary.is_expired_at(loaded_at + Duration::from_secs(61)));
}

#[test]
fn store_summary_keeps_distinct_featured_bundle_entries_with_shared_asset() {
    let response: StorefrontResponse = serde_json::from_value(serde_json::json!({
        "FeaturedBundle": {
            "Bundle": {
                "ID": "bundle-a",
                "DataAssetID": "asset-a",
                "CurrencyID": "vp",
                "Items": [{
                    "Item": {
                        "ItemTypeID": "skin-type",
                        "ItemID": "skin-a",
                        "Amount": 99
                    },
                    "BasePrice": 0,
                    "CurrencyID": "vp",
                    "DiscountPercent": 0,
                    "DiscountedPrice": 0,
                    "IsPromoItem": false
                }],
                "DurationRemainingInSeconds": 10
            },
            "Bundles": [
                {
                    "ID": "bundle-a",
                    "DataAssetID": "asset-a",
                    "CurrencyID": "vp",
                    "Items": [{
                        "Item": {
                            "ItemTypeID": "skin-type",
                            "ItemID": "skin-a",
                            "Amount": 1
                        },
                        "BasePrice": 0,
                        "CurrencyID": "vp",
                        "DiscountPercent": 0,
                        "DiscountedPrice": 0,
                        "IsPromoItem": false
                    }],
                    "DurationRemainingInSeconds": 10
                },
                {
                    "ID": "bundle-b",
                    "DataAssetID": "asset-a",
                    "CurrencyID": "vp",
                    "Items": [{
                        "Item": {
                            "ItemTypeID": "skin-type",
                            "ItemID": "skin-b",
                            "Amount": 2
                        },
                        "BasePrice": 0,
                        "CurrencyID": "vp",
                        "DiscountPercent": 0,
                        "DiscountedPrice": 0,
                        "IsPromoItem": false
                    }, {
                        "Item": {
                            "ItemTypeID": "buddy-type",
                            "ItemID": "buddy-b",
                            "Amount": 1
                        },
                        "BasePrice": 0,
                        "CurrencyID": "vp",
                        "DiscountPercent": 0,
                        "DiscountedPrice": 0,
                        "IsPromoItem": false
                    }],
                    "DurationRemainingInSeconds": 10
                }
            ],
            "BundleRemainingDurationInSeconds": 20
        },
        "SkinsPanelLayout": {
            "SingleItemOffers": [],
            "SingleItemStoreOffers": [],
            "SingleItemOffersRemainingDurationInSeconds": 30
        }
    }))
    .expect("response");
    let bundles = BundleCatalog::from_bundles(vec![crate::riot::content::Bundle {
        uuid: "asset-a".to_string(),
        display_name: "Shared Test Bundle".to_string(),
        display_icon: None,
        display_icon2: None,
        vertical_promo_image: None,
    }]);

    let summary = StoreSummary::from_response(
        response,
        &SkinCatalog::default(),
        &bundles,
        &CurrencyCatalog::default(),
    );

    assert_eq!(summary.featured_bundles.len(), 2);
    assert!(
        summary
            .featured_bundles
            .iter()
            .all(|bundle| bundle.bundle.display_name == "Shared Test Bundle")
    );
    assert_eq!(
        summary
            .featured_bundles
            .iter()
            .map(StoreBundleDisplay::item_count_label)
            .collect::<Vec<_>>(),
        ["1 item", "2 items"]
    );
}

#[test]
fn store_summary_expires_at_earliest_shop_section_reset() {
    let loaded_at = iced::time::Instant::now();
    let summary = StoreSummary {
        currency_balances: vec![],
        currency_balance_error: None,
        featured_bundles: vec![],
        daily_offers: vec![],
        daily_remaining_seconds: 30,
        bundle_remaining_seconds: 20,
        night_market_remaining_seconds: None,
        loaded_at,
        night_market_offers: vec![],
        accessory_remaining_seconds: None,
        accessory_offers: vec![],
    };

    assert!(!summary.is_expired_at(loaded_at + Duration::from_secs(19)));
    assert!(summary.is_expired_at(loaded_at + Duration::from_secs(20)));
}

#[test]
fn store_summary_expires_when_a_tick_skips_past_the_reset() {
    let loaded_at = iced::time::Instant::now();
    let summary = StoreSummary {
        currency_balances: vec![],
        currency_balance_error: None,
        featured_bundles: vec![],
        daily_offers: vec![],
        daily_remaining_seconds: 30,
        bundle_remaining_seconds: 20,
        night_market_remaining_seconds: Some(10),
        loaded_at,
        night_market_offers: vec![],
        accessory_remaining_seconds: None,
        accessory_offers: vec![],
    };
    let late = loaded_at + Duration::from_secs(45);

    assert!(summary.is_expired_at(late));
    assert_eq!(summary.bundle_remaining_seconds_at(late), 0);
    assert_eq!(summary.night_market_remaining_seconds_at(late), 0);
}

#[test]
fn store_summary_ignores_sections_the_server_reports_as_zero() {
    let loaded_at = iced::time::Instant::now();
    let summary = StoreSummary {
        currency_balances: vec![],
        currency_balance_error: None,
        featured_bundles: vec![],
        daily_offers: vec![],
        daily_remaining_seconds: 30,
        bundle_remaining_seconds: 0,
        night_market_remaining_seconds: Some(0),
        loaded_at,
        night_market_offers: vec![],
        accessory_remaining_seconds: Some(-5),
        accessory_offers: vec![],
    };

    assert!(!summary.is_expired_at(loaded_at));
    assert!(!summary.is_expired_at(loaded_at + Duration::from_secs(29)));
    assert!(summary.is_expired_at(loaded_at + Duration::from_secs(30)));
    assert_eq!(summary.accessory_remaining_seconds_at(loaded_at), 0);
}

#[test]
fn format_whole_number_groups_thousands() {
    assert_eq!(format_whole_number(0), "0");
    assert_eq!(format_whole_number(1000), "1,000");
    assert_eq!(format_whole_number(-1250000), "-1,250,000");
}

#[test]
fn competitive_rank_from_mmr_uses_latest_competitive_season() {
    let response: PlayerMmrResponse = serde_json::from_value(serde_json::json!({
        "Version": 1,
        "Subject": "puuid",
        "QueueSkills": {
            "competitive": {
                "SeasonalInfoBySeasonID": {
                    "old-season": {
                        "SeasonID": "old-season",
                        "CompetitiveTier": 12,
                        "RankedRating": 80,
                        "NumberOfGames": 9,
                        "GamesNeededForRating": 0
                    },
                    "current-season": {
                        "SeasonID": "current-season",
                        "CompetitiveTier": 15,
                        "RankedRating": 42,
                        "NumberOfGames": 12,
                        "GamesNeededForRating": 0
                    }
                }
            }
        },
        "LatestCompetitiveUpdate": {
            "SeasonID": "current-season",
            "TierAfterUpdate": 15,
            "RankedRatingAfterUpdate": 42
        }
    }))
    .expect("mmr");

    let rank = competitive_rank_from_mmr(&response).expect("rank");

    assert_eq!(rank.rank_name, "Platinum 1");
    assert_eq!(rank.ranked_rating, 42);
    assert_eq!(rank.label(), "Platinum 1 - 42 RR");
}

#[test]
fn competitive_rank_names_known_tiers() {
    assert_eq!(rank_name_for_competitive_tier(0), "Unrated");
    assert_eq!(rank_name_for_competitive_tier(21), "Ascendant 1");
    assert_eq!(rank_name_for_competitive_tier(27), "Radiant");
}

#[test]
fn penalty_status_detects_empty_active_expired_and_missing_expiry() {
    let now = time::OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();

    assert_eq!(
        penalty_status_from_response(&penalty_response(std::iter::empty()), now),
        AccountPenaltyStatus::NotPenalized
    );
    assert_eq!(
        penalty_status_from_response(&penalty_response([Some("2027-02-01T00:00:00Z")]), now),
        AccountPenaltyStatus::penalized_for(
            Some("Queue Dodge".to_string()),
            AccountPenaltyDuration::new(Some(expiry_timestamp("2027-02-01T00:00:00Z")), Some(1))
        )
    );
    assert_eq!(
        penalty_status_from_response(&penalty_response([Some("2027-01-01T00:00:00Z")]), now),
        AccountPenaltyStatus::NotPenalized
    );
    assert_eq!(
        penalty_status_from_response(&penalty_response([None]), now),
        AccountPenaltyStatus::penalized_for(
            Some("Queue Dodge".to_string()),
            AccountPenaltyDuration::new(None, Some(1))
        )
    );
}

#[test]
fn penalty_status_treats_invalid_expiry_as_active() {
    let now = time::OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();

    assert_eq!(
        penalty_status_from_response(&penalty_response([Some("not-a-date")]), now),
        AccountPenaltyStatus::penalized_for(
            Some("Queue Dodge".to_string()),
            AccountPenaltyDuration::new(None, Some(1))
        )
    );
}

#[test]
fn penalty_status_includes_all_active_penalties() {
    let now = time::OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();

    assert_eq!(
        penalty_status_from_response(
            &penalty_response([Some("2027-02-01T00:00:00Z"), Some("2027-03-01T00:00:00Z")]),
            now
        ),
        AccountPenaltyStatus::penalized_many(vec![
            AccountPenalty::new(
                Some("Queue Dodge".to_string()),
                AccountPenaltyDuration::new(
                    Some(expiry_timestamp("2027-02-01T00:00:00Z")),
                    Some(1)
                )
            ),
            AccountPenalty::new(
                Some("Queue Dodge".to_string()),
                AccountPenaltyDuration::new(
                    Some(expiry_timestamp("2027-03-01T00:00:00Z")),
                    Some(1)
                )
            )
        ])
    );
}

#[test]
fn penalty_status_uses_premier_effect_in_display_name() {
    let now = time::OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
    let response: PlayerPenaltiesResponse = serde_json::from_value(serde_json::json!({
        "Subject": "puuid",
        "Penalties": [{
            "ID": "penalty-id",
            "IssuingGameStartUnixMillis": 1_800_000_000_000i64,
            "IssuingMatchID": "match-id",
            "Expiry": "2027-02-01T00:00:00Z",
            "GamesRemaining": 0,
            "ApplyToAllPlatforms": true,
            "ApplyToPlatforms": ["PC"],
            "ApplyToPlatformGroups": ["riot"],
            "InfractionID": "infraction-id",
            "Origin": "automated",
            "ForgivenessIneligible": false,
            "IsAutomatedDetection": true,
            "PenaltyInfo": null,
            "DelayedPenaltyEffect": null,
            "GameBanEffect": null,
            "QueueDelayEffect": null,
            "QueueRestrictionEffect": null,
            "RankedRatingPenaltyEffect": null,
            "RiotRestrictionEffect": null,
            "RMSNotifyEffect": null,
            "WarningEffect": null,
            "XPMultiplierEffect": null,
            "PremierRestrictionEffect": {
                "RestrictionType": "DISQUALIFIED",
                "SeasonID": "season-id",
                "Source": "COMMS_RESTRICTION"
            }
        }],
        "Infractions": [{
            "ID": "infraction-id",
            "Name": "comms",
            "RatingName": "comms"
        }],
        "Version": 1
    }))
    .expect("penalty response");

    assert_eq!(
        penalty_status_from_response(&response, now),
        AccountPenaltyStatus::penalized_for(
            Some("Premier Disqualification".to_string()),
            AccountPenaltyDuration::new(Some(expiry_timestamp("2027-02-01T00:00:00Z")), None)
        )
    );
}

#[test]
fn penalty_status_humanizes_unknown_premier_restriction_type() {
    let now = time::OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
    let response: PlayerPenaltiesResponse = serde_json::from_value(serde_json::json!({
        "Subject": "puuid",
        "Penalties": [{
            "ID": "penalty-id",
            "IssuingGameStartUnixMillis": 1_800_000_000_000i64,
            "IssuingMatchID": "match-id",
            "Expiry": "2027-02-01T00:00:00Z",
            "GamesRemaining": 0,
            "ApplyToAllPlatforms": true,
            "ApplyToPlatforms": ["PC"],
            "ApplyToPlatformGroups": ["riot"],
            "InfractionID": "infraction-id",
            "Origin": "automated",
            "ForgivenessIneligible": false,
            "IsAutomatedDetection": true,
            "PenaltyInfo": null,
            "DelayedPenaltyEffect": null,
            "GameBanEffect": null,
            "QueueDelayEffect": null,
            "QueueRestrictionEffect": null,
            "RankedRatingPenaltyEffect": null,
            "RiotRestrictionEffect": null,
            "RMSNotifyEffect": null,
            "WarningEffect": null,
            "XPMultiplierEffect": null,
            "PremierRestrictionEffect": {
                "RestrictionType": "MATCHMAKING_LOCK",
                "SeasonID": "season-id",
                "Source": "source"
            }
        }],
        "Infractions": [{
            "ID": "infraction-id",
            "Name": "unknown",
            "RatingName": "unknown"
        }],
        "Version": 1
    }))
    .expect("penalty response");

    assert_eq!(
        penalty_status_from_response(&response, now),
        AccountPenaltyStatus::penalized_for(
            Some("Premier Matchmaking Lock".to_string()),
            AccountPenaltyDuration::new(Some(expiry_timestamp("2027-02-01T00:00:00Z")), None)
        )
    );
}

fn expiry_timestamp(value: &str) -> i64 {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .unwrap()
        .unix_timestamp()
}

#[test]
fn account_detail_update_keeps_rank_and_level_when_penalty_fails() {
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    let update = apply_account_detail_results(
        &mut account,
        Ok(Some(CompetitiveRank::new(15, "Platinum 1", 42))),
        Ok(123),
        Err("penalty endpoint unavailable".to_string()),
    );

    assert_eq!(
        update,
        super::app::AccountDetailUpdate {
            updated: true,
            partial: true
        }
    );
    assert_eq!(
        account
            .competitive_rank
            .as_ref()
            .map(CompetitiveRank::label),
        Some("Platinum 1 - 42 RR".to_string())
    );
    assert_eq!(account.account_level, Some(123));
    assert_eq!(account.penalty_status, AccountPenaltyStatus::Unchecked);
}

fn penalty_response(
    expiries: impl IntoIterator<Item = Option<&'static str>>,
) -> PlayerPenaltiesResponse {
    let penalties = expiries
        .into_iter()
        .enumerate()
        .map(|(index, expiry)| {
            serde_json::json!({
                "ID": format!("penalty-{index}"),
                "IssuingGameStartUnixMillis": 1_800_000_000_000i64,
                "IssuingMatchID": "match-id",
                "Expiry": expiry,
                "GamesRemaining": 1,
                "ApplyToAllPlatforms": true,
                "ApplyToPlatforms": ["PC"],
                "ApplyToPlatformGroups": ["riot"],
                "InfractionID": "infraction-id",
                "Origin": "automated",
                "ForgivenessIneligible": false,
                "IsAutomatedDetection": true,
                "PenaltyInfo": null,
                "DelayedPenaltyEffect": null,
                "GameBanEffect": null,
                "QueueDelayEffect": null,
                "QueueRestrictionEffect": null,
                "RankedRatingPenaltyEffect": null,
                "RiotRestrictionEffect": null,
                "RMSNotifyEffect": null,
                "WarningEffect": null,
                "XPMultiplierEffect": null,
                "PremierRestrictionEffect": null
            })
        })
        .collect::<Vec<_>>();

    serde_json::from_value(serde_json::json!({
        "Subject": "puuid",
        "Penalties": penalties,
        "Infractions": [{
            "ID": "infraction-id",
            "Name": "queue dodge",
            "RatingName": "Queue Dodge"
        }],
        "Version": 1
    }))
    .expect("penalty response")
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
        skins: vec![],
    }]);

    let summary = LoadoutSummary::from_response(response, &catalog, &weapons, None);

    assert_eq!(
        summary.gun_skins[0].label(),
        "Vandal: Prime Vandal - Level 3"
    );
    assert_eq!(summary.gun_skins[0].weapon.category, "Rifles");
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
fn status_bar_keeps_only_error_like_messages_on_screen() {
    let changed_at = iced::time::Instant::now();
    let later = changed_at + Duration::from_secs(10);
    let visible_later = |status: &str| status_visible_at(status, changed_at, later);

    assert!(!visible_later("Loaded 2 account profile(s)"));
    assert!(!visible_later("Loading shop"));
    assert!(!visible_later("Saved settings"));

    assert!(visible_later("Failed to load accounts: disk error"));
    assert!(visible_later(
        "Could not import redirect token: invalid URL"
    ));
    assert!(visible_later(
        "Store loaded, but profile update failed: missing profile"
    ));
    assert!(visible_later("Select an account before opening the shop"));
    assert!(visible_later("display name cannot be empty"));
}

#[test]
fn status_bar_briefly_shows_successful_actions() {
    let changed_at = iced::time::Instant::now();

    assert!(status_visible_at("Saved settings", changed_at, changed_at));
    assert!(status_visible_at(
        "Saved settings",
        changed_at,
        changed_at + Duration::from_secs(3)
    ));
    assert!(!status_visible_at(
        "Saved settings",
        changed_at,
        changed_at + Duration::from_secs(4)
    ));
    assert!(!status_visible_at("", changed_at, changed_at));
}

#[test]
fn changing_the_status_restarts_its_display_time() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.status_changed_at = iced::time::Instant::now() - Duration::from_secs(60);
    app.now = iced::time::Instant::now();
    assert!(!status_bar_visible(&app));

    let _ = app.update(Message::SaveSettings);

    assert_eq!(app.status, "Saved settings");
    assert!(status_bar_visible(&app));
}

#[test]
fn repeating_an_action_shows_its_status_again() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let _ = app.update(Message::SaveSettings);
    app.status_changed_at = iced::time::Instant::now() - Duration::from_secs(60);
    app.now = iced::time::Instant::now();
    assert!(!status_bar_visible(&app));

    let _ = app.update(Message::SaveSettings);

    assert_eq!(app.status, "Saved settings");
    assert!(status_bar_visible(&app));
}

#[test]
fn loading_status_detection_still_tracks_hidden_progress_messages() {
    assert!(loading_status_active("Loading shop"));
    assert!(loading_status_active("Refreshing Riot client version"));
    assert!(!loading_status_active(
        "Failed to load accounts: disk error"
    ));
}

#[test]
fn status_spinner_shows_only_beside_progress() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.account_availability_loading = true;

    app.status = "Loaded account details for 2 account(s)".to_string();
    assert!(!status_spinner_active(&app));

    app.status = "Could not load shop: offline".to_string();
    assert!(!status_spinner_active(&app));

    app.status = "Loading shop".to_string();
    assert!(status_spinner_active(&app));

    app.launcher_capture_in_progress = true;
    app.status = "Sign in to Riot Client and tick Stay signed in".to_string();
    assert!(status_spinner_active(&app));

    app.status = "Signed in as a different Riot account".to_string();
    assert!(!status_spinner_active(&app));
}

#[test]
fn account_export_payload_display_is_partially_masked() {
    let payload = format!("{}{}{}", "a".repeat(18), "b".repeat(30), "c".repeat(18));
    let masked = masked_account_export_payload(&payload);

    assert!(masked.starts_with(&"a".repeat(18)));
    assert!(masked.ends_with(&"c".repeat(18)));
    assert!(masked.contains(&"*".repeat(24)));
    assert!(!masked.contains(&"b".repeat(30)));
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
    assert_eq!(progress.tier_label(), "Tier 2 of 4");
    assert_eq!(
        progress.next_tier_label(),
        "2,500 / 3,000 XP toward next tier"
    );
    assert_eq!(
        progress.progress_percent_label().as_deref(),
        Some("50% complete")
    );
    assert!(progress.remaining_seconds.is_some());
}

#[test]
fn battle_pass_percent_is_not_rounded_up_to_complete() {
    let progress = BattlePassProgressDisplay {
        total_progression_earned: 995,
        total_progression_required: Some(1_000),
        ..battle_pass_display()
    };

    assert_eq!(
        progress.progress_percent_label().as_deref(),
        Some("99% complete")
    );
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

    assert_eq!(progress.tier_label(), "Tier 4 of 4 + Epilogue 1 of 2");
    assert_eq!(
        progress.progress_percent_label().as_deref(),
        Some("100% complete")
    );
    assert_eq!(
        progress.next_tier_label(),
        "1,000 / 5,000 XP toward next tier"
    );
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
fn battle_pass_progress_separates_free_unearned_and_locked_paid_rewards() {
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
                                highlighted: false,
                            }),
                            xp: Some(0),
                        },
                        ContractLevel {
                            reward: Some(ContractReward {
                                kind: "EquippableSkinLevel".to_string(),
                                uuid: "paid-tier-two".to_string(),
                                amount: 1,
                                highlighted: true,
                            }),
                            xp: Some(2_000),
                        },
                    ],
                    free_rewards: Some(vec![ContractReward {
                        kind: "Title".to_string(),
                        uuid: "free-title".to_string(),
                        amount: 1,
                        highlighted: false,
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
                        highlighted: false,
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

    assert_eq!(progress.earned_rewards.len(), 1);
    assert_eq!(progress.earned_rewards[0].name, "free-title");
    assert_eq!(progress.earned_rewards[0].track.label(), "Free");
    assert_eq!(progress.unearned_rewards.len(), 1);
    assert_eq!(progress.unearned_rewards[0].name, "future-free-title");
    assert_eq!(progress.locked_paid_rewards.len(), 2);
    assert!(
        progress
            .locked_paid_rewards
            .iter()
            .all(|reward| reward.track.label() == "Paid")
    );
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
                levels: vec![ContractLevel {
                    reward: Some(ContractReward {
                        kind: "Currency".to_string(),
                        uuid: radianite_uuid.to_string(),
                        amount: 1,
                        highlighted: false,
                    }),
                    xp: Some(2_000),
                }],
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

    let progress = battle_pass_progress_from_responses(
        &contracts,
        &catalog,
        Some(&content),
        &SkinCatalog::default(),
        &AccessoryCatalog::default(),
        &currencies,
    )
    .expect("battle pass progress");

    assert_eq!(progress.earned_rewards.len(), 1);
    assert_eq!(progress.earned_rewards[0].name, "10 Radianite");
    assert_eq!(progress.earned_rewards[0].amount_label(), None);
}

#[test]
fn non_empty_path_trims_input() {
    assert_eq!(
        non_empty_path(r"  C:\Riot Games\Riot Client\RiotClientServices.exe  "),
        Some(PathBuf::from(
            r"C:\Riot Games\Riot Client\RiotClientServices.exe"
        ))
    );
    assert_eq!(non_empty_path("   "), None);
}

#[test]
fn require_launcher_session_rejects_missing_backup() {
    let err = require_launcher_session(None).expect_err("missing backup");

    assert!(err.contains("captured launcher session"));
}

#[test]
fn require_launcher_session_rejects_missing_backup_folder() {
    let err = require_launcher_session(Some(LauncherSessionBackup {
        data_dir: PathBuf::from("missing-launcher-backup"),
        captured_at_unix: 100,
        puuid: "puuid".to_string(),
    }))
    .expect_err("missing backup folder");

    assert!(err.contains("backup folder is missing"));
}

#[test]
fn require_launcher_session_rejects_missing_private_settings_file() {
    let dir = tempdir().expect("temp dir");
    let err = require_launcher_session(Some(LauncherSessionBackup {
        data_dir: dir.path().to_path_buf(),
        captured_at_unix: 100,
        puuid: "puuid".to_string(),
    }))
    .expect_err("missing private settings");

    assert!(err.contains("missing Riot private settings"));
}

#[test]
fn require_launcher_session_accepts_ready_backup() {
    let dir = tempdir().expect("temp dir");
    fs::write(dir.path().join("RiotGamesPrivateSettings.yaml"), "settings")
        .expect("private settings");
    let backup = LauncherSessionBackup {
        data_dir: dir.path().to_path_buf(),
        captured_at_unix: 100,
        puuid: "puuid".to_string(),
    };

    let accepted = require_launcher_session(Some(backup)).expect("ready backup");

    assert_eq!(accepted.puuid, "puuid");
}

fn staged_recapture(backup_root: &Path, puuid: &str, settings: &str) -> CapturedLauncherSession {
    let staging_id = AccountId::new();
    let data_dir = backup_root.join(staging_id.to_string()).join("Data");
    fs::create_dir_all(&data_dir).expect("staging data dir");
    fs::write(data_dir.join("RiotGamesPrivateSettings.yaml"), settings).expect("settings");
    CapturedLauncherSession {
        account_id: staging_id,
        backup: LauncherSessionBackup {
            data_dir,
            captured_at_unix: 200,
            puuid: puuid.to_string(),
        },
    }
}

/// Puts the app in a login capture that has opened Riot Client and is waiting for a sign-in.
fn waiting_login_capture(
    app: &mut PrimeApp,
    target: super::LoginCaptureTarget,
) -> iced::task::Handle {
    let (_, handle) = iced::Task::<Message>::none().abortable();
    app.launcher_capture_in_progress = true;
    app.launcher_capture_kind = Some(target.kind());
    app.login_capture = Some(super::LoginCapture {
        target,
        wait: Some(handle.clone()),
    });
    handle
}

/// Puts the app in a login capture that is still closing and reopening Riot Client.
fn preparing_login_capture(app: &mut PrimeApp, target: super::LoginCaptureTarget) {
    app.launcher_capture_in_progress = true;
    app.launcher_capture_kind = Some(target.kind());
    app.login_capture = Some(super::LoginCapture { target, wait: None });
}

#[test]
fn cancelling_a_login_capture_stops_waiting_and_removes_its_slot() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let partial = staged_recapture(&backup_root, "puuid", "partial");
    let slot = backup_root.join(partial.account_id.to_string());
    let handle = waiting_login_capture(
        &mut app,
        super::LoginCaptureTarget::NewAccount(partial.account_id),
    );

    let _ = app.update(Message::CancelLoginCapture);

    assert!(handle.is_aborted());
    assert!(!app.launcher_capture_in_progress);
    assert_eq!(app.launcher_capture_kind, None);
    assert!(app.login_capture.is_none());
    assert!(!slot.exists());
    assert!(
        app.status.starts_with("Canceled login capture"),
        "{}",
        app.status
    );
}

#[test]
fn a_capture_cannot_be_cancelled_while_riot_client_reopens() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    preparing_login_capture(
        &mut app,
        super::LoginCaptureTarget::NewAccount(AccountId::new()),
    );

    let _ = app.update(Message::CancelLoginCapture);

    assert!(app.launcher_capture_in_progress);
    assert!(app.login_capture.is_some());
}

#[test]
fn a_capture_result_after_cancelling_is_discarded() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );
    let slot = app
        .repo
        .launcher_backups_dir()
        .join(draft.account_id.to_string());

    let _ = app.update(Message::AccountCaptureFinished(Ok(draft)));

    assert_eq!(app.pending_account, None);
    assert!(!slot.exists());
}

#[test]
fn a_capture_saves_the_signed_in_accounts_login_before_waiting() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let previous = account_with_backup(&backup_root, "Previous", "settings");
    let mut refreshed = previous.launcher_session.clone().expect("backup");
    refreshed.captured_at_unix = 500;
    app.state.push_account(previous.clone());
    let target = super::LoginCaptureTarget::NewAccount(AccountId::new());
    preparing_login_capture(&mut app, target);

    let task = app.update(Message::LoginCapturePrepared {
        target,
        result: Ok(Ok(Some((previous.id, refreshed.clone())))),
    });

    assert_eq!(
        app.state.accounts[0].launcher_session.as_ref(),
        Some(&refreshed)
    );
    assert!(
        app.login_capture
            .as_ref()
            .is_some_and(|capture| capture.wait.is_some())
    );
    assert!(task.units() > 0);
}

#[test]
fn a_capture_warns_when_the_signed_in_login_could_not_be_saved() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let target = super::LoginCaptureTarget::NewAccount(AccountId::new());
    preparing_login_capture(&mut app, target);

    let _ = app.update(Message::LoginCapturePrepared {
        target,
        result: Ok(Err("disk full".to_string())),
    });

    assert!(app.launcher_capture_in_progress);
    assert!(app.status.contains("disk full"), "{}", app.status);
    assert!(status_message_is_error(&app.status), "{}", app.status);
}

#[test]
fn a_capture_that_cannot_open_riot_client_ends_with_the_error() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let target = super::LoginCaptureTarget::NewAccount(AccountId::new());
    preparing_login_capture(&mut app, target);

    let _ = app.update(Message::LoginCapturePrepared {
        target,
        result: Err("Riot Client was not found".to_string()),
    });

    assert!(!app.launcher_capture_in_progress);
    assert_eq!(app.launcher_capture_kind, None);
    assert!(app.login_capture.is_none());
    assert_eq!(
        app.status,
        "Could not add account: Riot Client was not found"
    );
}

#[test]
fn starting_another_capture_discards_the_pending_draft_backup() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let draft = captured_account_draft(&backup_root, "puuid", "Main", "NA1", Shard::Na);
    let draft_slot = backup_root.join(draft.account_id.to_string());
    assert!(draft_slot.exists());
    app.pending_account = Some(draft);

    let _ = app.update(Message::AddCurrentAccount);

    assert!(!draft_slot.exists());
}

#[test]
fn loading_accounts_removes_unreferenced_backup_slots() {
    let dir = tempdir().expect("temp dir");
    let repo = AccountRepository::new(dir.path().join("accounts.json"));
    let backup_root = repo.launcher_backups_dir();
    let account = account_with_backup(
        &backup_root,
        "Current",
        "psl:
    authorization:
        riot-client:
            refresh_token: \"refresh-value\"
",
    );
    let orphaned_slot = backup_root.join(AccountId::new().to_string());
    fs::create_dir_all(orphaned_slot.join("Data")).expect("orphaned slot");
    let mut state = StoredState::default();
    state.push_account(account.clone());
    repo.save(&state).expect("save state");

    let loaded = load_accounts(&repo).expect("load accounts");

    assert!(!orphaned_slot.exists());
    assert!(backup_root.join(account.id.to_string()).exists());
    assert_eq!(
        loaded.state.accounts[0].launcher_session,
        account.launcher_session
    );
}

#[test]
fn login_capture_is_refused_while_an_account_is_launching() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());
    app.launching_account = Some(account.id);

    for message in [
        Message::AddAccount,
        Message::AddCurrentAccount,
        Message::ConfirmAddAccountCapture,
        Message::RequestLauncherSessionLogin(account.id),
        Message::StartLauncherSessionLogin(account.id),
    ] {
        let _ = app.update(message);

        assert!(!app.launcher_capture_in_progress);
        assert!(!app.show_add_account_prompt);
        assert_eq!(app.confirm_recapture_account, None);
        assert!(app.status.contains("launching"), "{}", app.status);
    }
}

#[test]
fn launch_is_refused_while_a_login_capture_runs() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());
    app.launcher_capture_in_progress = true;

    let _ = app.update(Message::LaunchAccount(account.id));

    assert_eq!(app.launching_account, None);
    assert_eq!(app.launch_preflight_account, None);
    assert!(app.status.contains("login capture"), "{}", app.status);
}

#[test]
fn launch_without_a_captured_login_is_refused_before_switching() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let selected = account_with_backup(&backup_root, "Main", "settings");
    let uncaptured = AccountProfile::new("Alt", Shard::Na).expect("alt");
    app.state.push_account(selected.clone());
    app.state.push_account(uncaptured.clone());
    app.state.select_account(selected.id);
    app.store_summary = Some(empty_store_summary());

    let task = app.update(Message::LaunchAccount(uncaptured.id));

    assert_eq!(task.units(), 0);
    assert_eq!(app.launch_preflight_account, None);
    assert_eq!(app.state.selected_account, Some(selected.id));
    assert!(app.store_summary.is_some());
    assert!(
        app.status.starts_with("Could not launch Alt"),
        "{}",
        app.status
    );
    assert!(app.status.contains("Re-capture login"), "{}", app.status);
}

fn finished_launch(previous_account_backup: Option<(AccountId, LauncherSessionBackup)>) -> Message {
    Message::LaunchFinished(Ok(LaunchAccountResult {
        target: crate::launch::LaunchTargetProcess::Valorant,
        previous_account_backup,
        previous_account_sync_warning: None,
        synced_backup: None,
        sync_warning: None,
    }))
}

#[test]
fn finished_launch_keeps_the_previous_account_login() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let previous = account_with_backup(&backup_root, "Previous", "settings");
    let mut refreshed = previous.launcher_session.clone().expect("backup");
    refreshed.captured_at_unix = 500;
    app.state.push_account(previous.clone());

    let _ = app.update(finished_launch(Some((previous.id, refreshed.clone()))));

    assert_eq!(
        app.state.accounts[0].launcher_session.as_ref(),
        Some(&refreshed)
    );
}

#[test]
fn finished_launch_ignores_a_previous_login_for_a_replaced_slot() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let previous = account_with_backup(&backup_root, "Previous", "settings");
    let current = previous.launcher_session.clone().expect("backup");
    let stale = LauncherSessionBackup {
        data_dir: backup_root.join("replaced").join("Data"),
        captured_at_unix: 500,
        puuid: current.puuid.clone(),
    };
    app.state.push_account(previous.clone());

    let _ = app.update(finished_launch(Some((previous.id, stale))));

    assert_eq!(
        app.state.accounts[0].launcher_session.as_ref(),
        Some(&current)
    );
}

#[test]
fn a_finished_launch_check_leaves_a_dialog_opened_meanwhile() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());
    app.launch_preflight_account = Some(account.id);
    let _ = app.update(Message::OpenImportAccount);
    let _ = app.update(Message::ImportAccountInputChanged("export".to_string()));

    let _ = app.update(Message::LaunchPreflightChecked(
        super::data::account_details::AccountActivityCheck {
            account_id: account.id,
            availability: AccountAvailability::Available,
        },
        false,
    ));

    assert_eq!(app.launching_account, Some(account.id));
    assert!(app.show_import_account_prompt);
    assert_eq!(app.import_account_input, "export");
}

#[test]
fn closing_the_import_prompt_forgets_the_pasted_export() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());
    let _ = app.update(Message::OpenImportAccount);
    let _ = app.update(Message::ImportAccountInputChanged("export".to_string()));

    let _ = app.update(Message::RequestDeleteAccount(account.id));
    let _ = app.update(Message::CancelDeleteAccount);
    let _ = app.update(Message::OpenImportAccount);

    assert_eq!(app.import_account_input, "");
}

#[test]
fn launch_warns_before_closing_a_running_game() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());
    app.launch_preflight_account = Some(account.id);

    let _ = app.update(Message::LaunchPreflightChecked(
        super::data::account_details::AccountActivityCheck {
            account_id: account.id,
            availability: AccountAvailability::Available,
        },
        true,
    ));

    let warning = app.unavailable_launch_warning.as_ref().expect("warning");
    assert!(
        warning.reason.contains("already running"),
        "{}",
        warning.reason
    );
    assert_eq!(app.launching_account, None);
}

#[test]
fn a_dialog_opening_closes_the_popovers() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());
    app.launch_preflight_account = Some(account.id);
    app.account_switcher_open = true;
    app.open_account_menu = Some(account.id);

    let _ = app.update(Message::LaunchPreflightChecked(
        super::data::account_details::AccountActivityCheck {
            account_id: account.id,
            availability: AccountAvailability::Available,
        },
        true,
    ));

    assert!(app.unavailable_launch_warning.is_some());
    assert!(!app.account_switcher_open);
    assert_eq!(app.open_account_menu, None);
}

#[test]
fn clicking_outside_a_popover_closes_it() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());
    app.account_switcher_open = true;
    app.open_account_menu = Some(account.id);

    let _ = app.update(Message::DismissPopovers);

    assert!(!app.account_switcher_open);
    assert_eq!(app.open_account_menu, None);
}

#[test]
fn launch_starts_when_no_game_is_running() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    let backup_root = app.repo.launcher_backups_dir();
    account = {
        let with_backup = account_with_backup(&backup_root, "Main", "settings");
        AccountProfile {
            id: account.id,
            ..with_backup
        }
    };
    app.state.push_account(account.clone());
    app.launch_preflight_account = Some(account.id);

    let _ = app.update(Message::LaunchPreflightChecked(
        super::data::account_details::AccountActivityCheck {
            account_id: account.id,
            availability: AccountAvailability::Available,
        },
        false,
    ));

    assert!(app.unavailable_launch_warning.is_none());
    assert_eq!(app.launching_account, Some(account.id));
}

#[test]
fn recapture_asks_for_confirmation_before_starting() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());

    let _ = app.update(Message::RequestLauncherSessionLogin(account.id));

    assert_eq!(app.confirm_recapture_account, Some(account.id));
    assert!(!app.launcher_capture_in_progress);

    let _ = app.update(Message::CancelLauncherSessionLogin);

    assert_eq!(app.confirm_recapture_account, None);
    assert!(!app.launcher_capture_in_progress);
}

#[test]
fn add_account_prompt_warns_when_valorant_is_running() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());

    let task = app.update(Message::AddAccount);

    assert!(app.show_add_account_prompt);
    assert!(
        task.units() > 0,
        "opening the prompt checks for a running game"
    );

    let _ = app.update(Message::CapturePromptGameChecked(true));
    assert!(app.capture_prompt_valorant_running);

    let _ = app.update(Message::CancelAddAccountCapture);
    assert!(!app.capture_prompt_valorant_running);
}

#[test]
fn recapture_prompt_warns_when_valorant_is_running() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    app.state.push_account(account.clone());

    let task = app.update(Message::RequestLauncherSessionLogin(account.id));
    assert!(
        task.units() > 0,
        "opening the prompt checks for a running game"
    );

    let _ = app.update(Message::CapturePromptGameChecked(true));
    assert!(app.capture_prompt_valorant_running);

    let _ = app.update(Message::CancelLauncherSessionLogin);
    assert!(!app.capture_prompt_valorant_running);
}

#[test]
fn a_late_game_check_does_not_warn_once_the_prompt_closed() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());

    let _ = app.update(Message::AddAccount);
    let _ = app.update(Message::CancelAddAccountCapture);
    let _ = app.update(Message::CapturePromptGameChecked(true));

    assert!(!app.capture_prompt_valorant_running);
}

#[test]
fn recapture_as_a_different_account_keeps_the_existing_backup() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let account = account_with_backup(&backup_root, "Main", "original");
    let original_backup = account.launcher_session.clone();
    app.state.push_account(account.clone());
    let captured = staged_recapture(&backup_root, "someone-else", "other login");
    let staging_slot = backup_root.join(captured.account_id.to_string());
    waiting_login_capture(
        &mut app,
        super::LoginCaptureTarget::Existing {
            account_id: account.id,
            staging_id: captured.account_id,
        },
    );

    let _ = app.update(Message::LauncherSessionLoginStarted(
        account.id,
        Ok(captured),
    ));

    let saved = &app.state.accounts[0];
    assert_eq!(saved.launcher_session, original_backup);
    assert_eq!(
        fs::read_to_string(
            original_backup
                .expect("backup")
                .data_dir
                .join("RiotGamesPrivateSettings.yaml")
        )
        .expect("original settings"),
        "original"
    );
    assert!(!staging_slot.exists());
    assert!(
        app.status.contains("different Riot account"),
        "{}",
        app.status
    );
}

#[test]
fn a_recapture_as_another_account_stays_on_screen_and_brings_prime_forward() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let account = account_with_backup(&backup_root, "Main", "original");
    app.state.push_account(account.clone());
    let captured = staged_recapture(&backup_root, "someone-else", "other login");
    waiting_login_capture(
        &mut app,
        super::LoginCaptureTarget::Existing {
            account_id: account.id,
            staging_id: captured.account_id,
        },
    );

    let task = app.update(Message::LauncherSessionLoginStarted(
        account.id,
        Ok(captured),
    ));

    assert!(status_message_is_error(&app.status), "{}", app.status);
    assert!(task.units() > 0);
}

#[test]
fn a_launch_that_could_not_save_the_previous_login_stays_on_screen() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());

    let _ = app.update(Message::LaunchFinished(Ok(LaunchAccountResult {
        target: crate::launch::LaunchTargetProcess::Valorant,
        previous_account_backup: None,
        previous_account_sync_warning: Some("file locked".to_string()),
        synced_backup: None,
        sync_warning: None,
    })));

    assert!(app.status.contains("could not be saved"), "{}", app.status);
    assert!(status_message_is_error(&app.status), "{}", app.status);
}

#[test]
fn recapture_as_the_same_account_replaces_the_backup() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let account = account_with_backup(&backup_root, "Main", "original");
    app.state.push_account(account.clone());
    let captured = staged_recapture(&backup_root, "Main-puuid", "fresh login");
    let staging_slot = backup_root.join(captured.account_id.to_string());
    waiting_login_capture(
        &mut app,
        super::LoginCaptureTarget::Existing {
            account_id: account.id,
            staging_id: captured.account_id,
        },
    );

    let _ = app.update(Message::LauncherSessionLoginStarted(
        account.id,
        Ok(captured),
    ));

    let backup = app.state.accounts[0]
        .launcher_session
        .clone()
        .expect("backup");
    assert_eq!(
        backup.data_dir,
        backup_root.join(account.id.to_string()).join("Data")
    );
    assert_eq!(backup.captured_at_unix, 200);
    assert_eq!(
        fs::read_to_string(backup.data_dir.join("RiotGamesPrivateSettings.yaml"))
            .expect("fresh settings"),
        "fresh login"
    );
    assert!(!staging_slot.exists());
}

#[test]
fn accounts_are_not_saved_before_a_successful_load() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.accounts_loaded = false;

    assert!(app.state_to_save().is_err());

    let _ = app.update(Message::Loaded(Err("accounts.json is invalid".to_string())));

    assert!(app.state_to_save().is_err());
    assert!(!app.accounts_loaded);
}

#[test]
fn accounts_are_saved_after_a_successful_load() {
    let dir = tempdir().expect("temp dir");
    let repo = AccountRepository::new(dir.path().join("accounts.json"));
    let mut app = test_app(dir.path());
    app.accounts_loaded = false;

    let _ = app.update(Message::Loaded(load_accounts(&repo)));

    assert!(app.accounts_loaded);
    assert!(app.state_to_save().is_ok());
}

fn account_with_backup(backup_root: &Path, name: &str, settings: &str) -> AccountProfile {
    let mut account = AccountProfile::new(name, Shard::Na).expect("account");
    let data_dir = backup_root.join(account.id.to_string()).join("Data");
    fs::create_dir_all(&data_dir).expect("backup data dir");
    fs::write(data_dir.join("RiotGamesPrivateSettings.yaml"), settings).expect("settings");
    account
        .attach_launcher_session(LauncherSessionBackup {
            data_dir,
            captured_at_unix: 100,
            puuid: format!("{name}-puuid"),
        })
        .expect("attach backup");
    account
}

#[test]
fn loading_accounts_removes_legacy_launcher_sessions() {
    let dir = tempdir().expect("temp dir");
    let repo = AccountRepository::new(dir.path().join("accounts.json"));
    let backup_root = repo.launcher_backups_dir();
    let legacy = account_with_backup(
        &backup_root,
        "Legacy",
        "riot-login:
  persist:
    session:
      cookies:
        - name: \"ssid\"
          value: \"ssid-value\"
",
    );
    let current = account_with_backup(
        &backup_root,
        "Current",
        "psl:
    authorization:
        riot-client:
            refresh_token: \"refresh-value\"
",
    );
    let legacy_slot = backup_root.join(legacy.id.to_string());
    let orphaned_legacy_slot = backup_root.join(AccountId::new().to_string());
    fs::create_dir_all(orphaned_legacy_slot.join("Data")).expect("orphaned slot");
    fs::write(
        orphaned_legacy_slot
            .join("Data")
            .join("RiotGamesPrivateSettings.yaml"),
        "riot-login:
  persist:
    session:
      cookies:
        - name: \"ssid\"
          value: \"old\"
",
    )
    .expect("orphaned settings");
    let mut state = StoredState::default();
    state.push_account(legacy.clone());
    state.push_account(current.clone());
    repo.save(&state).expect("save state");

    let loaded = load_accounts(&repo).expect("load accounts");

    assert_eq!(loaded.removed_legacy_sessions, vec![legacy.summary()]);
    assert_eq!(loaded.legacy_cleanup_error, None);
    assert!(!legacy_slot.exists());
    assert!(!orphaned_legacy_slot.exists());
    let find = |state: &StoredState, id| {
        state
            .accounts
            .iter()
            .find(|account| account.id == id)
            .cloned()
            .expect("account")
    };
    assert_eq!(find(&loaded.state, legacy.id).launcher_session, None);
    assert_eq!(
        find(&loaded.state, current.id).launcher_session,
        current.launcher_session
    );
    let saved = repo.load().expect("reload saved state");
    assert_eq!(find(&saved, legacy.id).launcher_session, None);
}

#[test]
fn only_missing_private_settings_is_pending_login_capture() {
    assert!(is_pending_launcher_capture_error(
        &LauncherSessionError::PrivateSettingsNotFound
    ));
    assert!(!is_pending_launcher_capture_error(
        &LauncherSessionError::MissingRefreshToken
    ));
}

#[test]
fn add_current_account_starts_capture_without_login_prompt() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());

    let _ = app.update(Message::AddCurrentAccount);

    assert!(app.launcher_capture_in_progress);
    assert_eq!(
        app.launcher_capture_kind,
        Some(super::LauncherCaptureKind::Current)
    );
    assert!(!app.show_add_account_prompt);
    assert_eq!(app.status, "Capturing the Riot account currently signed in");
}

#[test]
fn current_account_capture_success_populates_confirmation_fields() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.launcher_capture_in_progress = true;
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );

    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft.clone())));

    assert!(!app.launcher_capture_in_progress);
    assert_eq!(app.launcher_capture_kind, None);
    assert_eq!(app.pending_account, Some(draft));
    assert_eq!(app.new_display_name, "Player");
    assert_eq!(
        app.status,
        "Captured current Riot account. Confirm the account details to save it."
    );
}

#[test]
fn current_account_capture_failure_clears_capture_state() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.launcher_capture_in_progress = true;

    let _ = app.update(Message::CurrentAccountCaptureFinished(Err(
        "Riot Client is not signed in with Stay signed in enabled".to_string(),
    )));

    assert!(!app.launcher_capture_in_progress);
    assert_eq!(app.launcher_capture_kind, None);
    assert_eq!(app.pending_account, None);
    assert_eq!(
        app.status,
        "Could not add current account: Riot Client is not signed in with Stay signed in enabled"
    );
}

#[test]
fn confirming_new_current_account_saves_and_selects_profile() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );

    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft.clone())));
    let _ = app.update(Message::ConfirmCapturedAccount);
    app.repo.save(&app.state).expect("persist state");
    let saved = app.repo.load().expect("load saved state");

    assert_eq!(app.state.accounts.len(), 1);
    assert_eq!(app.state.selected_account, Some(draft.account_id));
    assert_eq!(app.pending_account, None);
    assert_eq!(saved.accounts.len(), 1);
    assert_eq!(saved.selected_account, Some(draft.account_id));
    assert_eq!(saved.accounts[0].puuid.as_deref(), Some("puuid-a"));
}

fn empty_store_summary() -> StoreSummary {
    StoreSummary {
        currency_balances: vec![],
        currency_balance_error: None,
        featured_bundles: vec![],
        daily_offers: vec![],
        daily_remaining_seconds: 86_400,
        bundle_remaining_seconds: 86_400,
        night_market_remaining_seconds: None,
        loaded_at: iced::time::Instant::now(),
        night_market_offers: vec![],
        accessory_remaining_seconds: None,
        accessory_offers: vec![],
    }
}

#[test]
fn adding_an_account_clears_the_previous_accounts_shop_and_loadout() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.store_summary = Some(empty_store_summary());
    app.loadout_summary = Some(LoadoutSummary::without_loadout("old".to_string(), None));
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );

    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft)));
    let _ = app.update(Message::ConfirmCapturedAccount);

    assert_eq!(app.store_summary, None);
    assert_eq!(app.loadout_summary, None);
}

#[test]
fn duplicate_current_account_capture_updates_existing_profile_without_confirmation() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.client_version_input = "release-10.10-shipping-1-1234567".to_string();
    let backup_root = app.repo.launcher_backups_dir();
    let mut existing = AccountProfile::new("Main", Shard::Na).expect("account");
    existing
        .apply_riot_identity("puuid-a", "OldName", "OLD")
        .expect("identity");
    let existing_id = existing.id;
    let existing_data = backup_root.join(existing_id.to_string()).join("Data");
    fs::create_dir_all(&existing_data).expect("existing data");
    fs::write(existing_data.join("old.txt"), "old").expect("old backup");
    existing.launcher_session = Some(LauncherSessionBackup {
        data_dir: existing_data.clone(),
        captured_at_unix: 50,
        puuid: "puuid-a".to_string(),
    });
    app.state.push_account(existing);
    let draft = captured_account_draft(&backup_root, "puuid-a", "Player", "NA1", Shard::Eu);
    let draft_id = draft.account_id;
    fs::write(
        draft.backup.data_dir.join("RiotGamesPrivateSettings.yaml"),
        "new-settings",
    )
    .expect("new settings");

    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft)));

    assert_eq!(app.state.accounts.len(), 1);
    assert_eq!(app.state.selected_account, Some(existing_id));
    assert_eq!(app.pending_account, None);
    assert_eq!(
        app.state.accounts[0].riot_id().as_deref(),
        Some("Player#NA1")
    );
    assert_eq!(app.state.accounts[0].shard, Shard::Eu);
    assert!(app.state.accounts[0].session.is_some());
    assert_eq!(
        app.state.accounts[0]
            .launcher_session
            .as_ref()
            .map(|backup| backup.data_dir.as_path()),
        Some(existing_data.as_path())
    );
    assert!(!backup_root.join(draft_id.to_string()).exists());
    assert!(!existing_data.join("old.txt").exists());
    assert_eq!(
        fs::read_to_string(existing_data.join("RiotGamesPrivateSettings.yaml")).expect("settings"),
        "new-settings"
    );
    assert!(
        app.status
            .starts_with("Duplicate account: Prime did not add a new profile because this Riot account is already in Prime; updated and selected Main")
    );
    assert!(!app.status.starts_with("Loading account details"));
    assert!(status_message_is_error(&app.status));
}

#[test]
fn a_captured_account_opens_the_accounts_tab_to_confirm_it() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.active_tab = super::Tab::Shop;
    app.tab_scroll_offsets.set(
        super::Tab::Accounts,
        iced::widget::operation::AbsoluteOffset { x: 0.0, y: 400.0 },
    );
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );

    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft)));

    assert_eq!(app.active_tab, super::Tab::Accounts);
    assert_eq!(app.tab_scroll_offsets.get(super::Tab::Accounts).y, 0.0);
}

#[test]
fn adding_the_current_account_says_it_discarded_the_unsaved_one() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.pending_account = Some(captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    ));

    let _ = app.update(Message::AddCurrentAccount);

    assert_eq!(app.pending_account, None);
    assert!(app.status.contains("Discarded"), "{}", app.status);
}

#[test]
fn adding_a_new_account_says_it_discarded_the_unsaved_one() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.pending_account = Some(captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    ));

    let _ = app.update(Message::ConfirmAddAccountCapture);

    assert_eq!(app.pending_account, None);
    assert!(app.status.contains("Discarded"), "{}", app.status);
}

#[test]
fn canceling_captured_current_account_removes_temporary_backup() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );
    let backup_slot = app
        .repo
        .launcher_backups_dir()
        .join(draft.account_id.to_string());

    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft)));
    assert!(backup_slot.exists());

    let _ = app.update(Message::CancelCapturedAccount);

    assert_eq!(app.pending_account, None);
    assert!(!backup_slot.exists());
}

fn redirect_url_for_subject(subject: &str) -> String {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let claims = URL_SAFE_NO_PAD.encode(format!(r#"{{"sub":"{subject}"}}"#));
    format!(
        "https://playvalorant.com/opt_in#access_token=header.{claims}.signature&expires_in=3600"
    )
}

fn app_with_puuid_account(dir: &Path, puuid: &str) -> PrimeApp {
    let mut app = test_app(dir);
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    account.puuid = Some(puuid.to_string());
    let account_id = account.id;
    app.state.push_account(account);
    app.state.select_account(account_id);
    app
}

#[test]
fn token_import_rejects_a_token_for_another_riot_account() {
    let dir = tempdir().expect("temp dir");
    let mut app = app_with_puuid_account(dir.path(), "puuid-a");
    app.redirect_input = redirect_url_for_subject("puuid-b");

    let _ = app.update(Message::ImportRedirect);

    assert!(
        app.status.starts_with("Could not import redirect token"),
        "{}",
        app.status
    );
    assert_eq!(app.state.accounts[0].session, None);
}

#[test]
fn token_import_rejects_a_token_whose_account_cannot_be_read() {
    let dir = tempdir().expect("temp dir");
    let mut app = app_with_puuid_account(dir.path(), "puuid-a");
    app.redirect_input = "https://playvalorant.com/opt_in#access_token=opaque".to_string();

    let _ = app.update(Message::ImportRedirect);

    assert!(
        app.status.starts_with("Could not import redirect token"),
        "{}",
        app.status
    );
    assert_eq!(app.state.accounts[0].session, None);
}

#[test]
fn token_import_accepts_the_accounts_own_token() {
    let dir = tempdir().expect("temp dir");
    let mut app = app_with_puuid_account(dir.path(), "puuid-a");
    app.redirect_input = redirect_url_for_subject("puuid-a");

    let _ = app.update(Message::ImportRedirect);

    assert!(app.state.accounts[0].session.is_some(), "{}", app.status);
    assert!(
        app.status.contains(&app.state.accounts[0].summary()),
        "{}",
        app.status
    );
}

#[test]
fn cache_account_api_context_leaves_the_account_alone_for_another_riot_account() {
    let mut state = StoredState::default();
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    account.puuid = Some("puuid-a".to_string());
    let account_id = account.id;
    state.push_account(account);

    let result = cache_account_api_context(
        &mut state,
        account_id,
        AuthSession::new("access", None, None, "Bearer", Some(3600), 100),
        None,
        ApiIdentity {
            puuid: "puuid-b".to_string(),
            game_name: None,
            tag_line: None,
            shard: Shard::Eu,
            region: None,
        },
    );

    assert!(result.is_err());
    assert_eq!(state.accounts[0].session, None);
    assert_eq!(state.accounts[0].shard, Shard::Na);
    assert_eq!(state.accounts[0].puuid.as_deref(), Some("puuid-a"));
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

    let identity =
        api_identity(&account, Some(&player_info("puuid-a")), None, Shard::Eu).expect("identity");

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
fn caching_an_api_context_saves_the_region() {
    let mut state = StoredState::default();
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    account.puuid = Some("puuid-a".to_string());
    let account_id = account.id;
    state.push_account(account);

    cache_account_api_context(
        &mut state,
        account_id,
        AuthSession::new("access", None, None, "Bearer", Some(3600), 100),
        None,
        ApiIdentity {
            puuid: "puuid-a".to_string(),
            game_name: None,
            tag_line: None,
            shard: Shard::Na,
            region: Some(ValorantRegion::Br),
        },
    )
    .expect("cached");

    assert_eq!(state.accounts[0].region, Some(ValorantRegion::Br));
}

#[test]
fn refreshing_a_profile_forgets_its_saved_region() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    account.puuid = Some("puuid-a".to_string());
    account.region = Some(ValorantRegion::Na);
    let account_id = account.id;
    app.state.push_account(account);

    let _ = app.update(Message::ProfileIdentityLoaded(
        account_id,
        Ok(super::data::account_details::RefreshedProfileIdentity {
            account_id,
            session: AuthSession::new("access", None, None, "Bearer", Some(3600), 100),
            launcher_session: None,
            puuid: "puuid-a".to_string(),
            game_name: "Main".to_string(),
            tag_line: "NA1".to_string(),
        }),
    ));

    assert_eq!(app.state.accounts[0].region, None);
}

#[test]
fn a_stored_token_for_another_riot_account_is_not_used() {
    let tokens = crate::riot::auth::parse_redirect_tokens(&redirect_url_for_subject("puuid-b"))
        .expect("tokens");
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    account.puuid = Some("puuid-a".to_string());
    account.session = Some(tokens.into_session());
    let api = crate::riot::client::RiotApi::new().expect("api");

    let result =
        iced::futures::executor::block_on(super::data::session::active_api_session(&api, &account));

    assert!(result.is_err(), "the other account's token was used");
}

#[test]
fn cache_account_api_context_updates_matching_account() {
    let mut state = StoredState::default();
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    let account_id = account.id;
    state.push_account(account);
    let session = AuthSession::new(
        "access",
        None,
        Some("entitlement".to_string()),
        "Bearer",
        Some(3600),
        100,
    );

    cache_account_api_context(
        &mut state,
        account_id,
        session.clone(),
        None,
        ApiIdentity {
            puuid: "puuid".to_string(),
            game_name: Some("Player".to_string()),
            tag_line: Some("NA1".to_string()),
            shard: Shard::Eu,
            region: None,
        },
    )
    .expect("cache api context");

    assert_eq!(state.accounts[0].session, Some(session));
    assert_eq!(state.accounts[0].puuid.as_deref(), Some("puuid"));
    assert_eq!(state.accounts[0].riot_id().as_deref(), Some("Player#NA1"));
    assert_eq!(state.accounts[0].shard, Shard::Eu);
}

#[test]
fn cache_account_api_context_updates_refreshed_launcher_session() {
    let mut state = StoredState::default();
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    let account_id = account.id;
    account.launcher_session = Some(LauncherSessionBackup {
        data_dir: "old-backup".into(),
        captured_at_unix: 100,
        puuid: "puuid".to_string(),
    });
    state.push_account(account);
    let session = AuthSession::new("access", None, None, "Bearer", Some(3600), 100);
    let launcher_session = LauncherSessionBackup {
        data_dir: "new-backup".into(),
        captured_at_unix: 200,
        puuid: "puuid".to_string(),
    };

    cache_account_api_context(
        &mut state,
        account_id,
        session,
        Some(launcher_session.clone()),
        ApiIdentity {
            puuid: "puuid".to_string(),
            game_name: None,
            tag_line: None,
            shard: Shard::Na,
            region: None,
        },
    )
    .expect("cache api context");

    assert_eq!(state.accounts[0].launcher_session, Some(launcher_session));
}

fn availability_refresh(
    account_id: AccountId,
    launcher_session: Option<LauncherSessionBackup>,
) -> Message {
    Message::AccountAvailabilitiesLoaded(AccountAvailabilityRefresh {
        accounts: vec![],
        refreshed_sessions: vec![RefreshedApiContext {
            account_id,
            session: AuthSession::new("fresh", None, None, "Bearer", Some(3600), 100),
            launcher_session,
            identity: ApiIdentity {
                puuid: "Main-puuid".to_string(),
                game_name: None,
                tag_line: None,
                shard: Shard::Na,
                region: None,
            },
        }],
    })
}

#[test]
fn selecting_an_account_refreshes_it_without_a_full_reload() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let main = AccountProfile::new("Main", Shard::Na).expect("main");
    let alt = AccountProfile::new("Alt", Shard::Na).expect("alt");
    app.state.push_account(main);
    app.state.push_account(alt.clone());
    app.client_version_input = "release-1".to_string();

    let _ = app.update(Message::SelectAccount(alt.id));

    assert!(!app.account_ranks_loading.is_empty());
    assert!(app.account_availability_loading);
    assert_eq!(app.status, format!("Selected {}", alt.summary()));
}

fn accounts_tab_app(dir: &Path) -> (PrimeApp, AccountProfile) {
    let mut app = test_app(dir);
    let mut account = AccountProfile::new("Main", Shard::Na).expect("account");
    account.puuid = Some("puuid-a".to_string());
    app.state.push_account(account.clone());
    app.state.select_account(account.id);
    app.client_version_input = "release-1".to_string();
    app.active_tab = super::Tab::Accounts;
    (app, account)
}

#[test]
fn refresh_profile_result_is_not_replaced_by_a_details_reload() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account) = accounts_tab_app(dir.path());

    let _ = app.update(Message::ProfileIdentityLoaded(
        account.id,
        Ok(super::data::account_details::RefreshedProfileIdentity {
            account_id: account.id,
            session: AuthSession::new("access", None, None, "Bearer", Some(3600), 100),
            launcher_session: None,
            puuid: "puuid-a".to_string(),
            game_name: "Player".to_string(),
            tag_line: "NA1".to_string(),
        }),
    ));

    assert!(app.status.starts_with("Refreshed "), "{}", app.status);
}

#[test]
fn profile_refreshes_are_tracked_per_account() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, alt) = two_account_app(dir.path());

    let _ = app.update(Message::RefreshProfileIdentity(main.id));
    let _ = app.update(Message::RefreshProfileIdentity(alt.id));
    let _ = app.update(Message::ProfileIdentityLoaded(
        main.id,
        Err("offline".to_string()),
    ));

    assert!(!app.profile_identity_refreshing.contains(&main.id));
    assert!(app.profile_identity_refreshing.contains(&alt.id));
}

#[test]
fn a_profile_refresh_is_not_started_twice() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, _) = two_account_app(dir.path());
    let _ = app.update(Message::RefreshProfileIdentity(main.id));

    let task = app.update(Message::RefreshProfileIdentity(main.id));

    assert_eq!(task.units(), 0);
    assert!(app.profile_identity_refreshing.contains(&main.id));
}

#[test]
fn import_result_is_not_replaced_by_a_details_reload() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = accounts_tab_app(dir.path());
    let imported = AccountProfile::new("Alt", Shard::Na).expect("alt");

    let _ = app.update(Message::AccountImported(Ok(
        crate::account_transfer::ImportedAccount {
            original_id: AccountId::new(),
            account: imported,
            id_changed: true,
            imported_launcher_file_count: 1,
        },
    )));

    assert!(
        app.status.ends_with("with a new local ID"),
        "{}",
        app.status
    );
}

#[test]
fn background_account_details_leave_the_status_alone() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = accounts_tab_app(dir.path());
    app.status = "Added Main".to_string();

    let _ = app.update(Message::AccountRanksLoaded {
        result: Default::default(),
        announce: false,
    });

    assert_eq!(app.status, "Added Main");
}

#[test]
fn opening_accounts_during_a_launch_keeps_the_launch_progress() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account) = accounts_tab_app(dir.path());
    app.active_tab = super::Tab::Settings;
    app.launching_account = Some(account.id);
    app.status = "Riot Client is open; waiting for VALORANT".to_string();

    let _ = app.update(Message::TabSelected(super::Tab::Accounts));

    assert_eq!(app.status, "Riot Client is open; waiting for VALORANT");
}

#[test]
fn reopening_accounts_soon_after_a_load_does_not_refetch() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = accounts_tab_app(dir.path());
    app.active_tab = super::Tab::Settings;

    let _ = app.update(Message::TabSelected(super::Tab::Accounts));
    assert!(!app.account_ranks_loading.is_empty());
    let _ = app.update(Message::AccountRanksLoaded {
        result: Default::default(),
        announce: true,
    });
    let _ = app.update(Message::AccountAvailabilitiesLoaded(Default::default()));
    let _ = app.update(Message::TabSelected(super::Tab::Settings));
    let _ = app.update(Message::TabSelected(super::Tab::Accounts));

    assert!(app.account_ranks_loading.is_empty());
    assert!(!app.account_availability_loading);
}

fn two_account_app(dir: &Path) -> (PrimeApp, AccountProfile, AccountProfile) {
    let mut app = test_app(dir);
    let main = AccountProfile::new("Main", Shard::Na).expect("main");
    let alt = AccountProfile::new("Alt", Shard::Na).expect("alt");
    app.state.push_account(main.clone());
    app.state.push_account(alt.clone());
    app.state.select_account(main.id);
    app.client_version_input = "release-1".to_string();
    (app, main, alt)
}

#[test]
fn a_shop_reply_for_an_account_switched_away_from_is_not_reported() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, alt) = two_account_app(dir.path());
    let _ = app.update(Message::TabSelected(super::Tab::Shop));
    let request = app.store_request.expect("shop request").id;
    let _ = app.update(Message::TabSelected(super::Tab::Settings));
    let _ = app.update(Message::SelectAccount(alt.id));

    let _ = app.update(Message::StorefrontLoaded(request, Err("boom".to_string())));

    assert!(!app.status.contains("Store check failed"), "{}", app.status);
    assert_eq!(app.store_request, None);
}

#[test]
fn an_old_shop_reply_does_not_end_the_newer_load_for_the_same_account() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, alt) = two_account_app(dir.path());
    let _ = app.update(Message::TabSelected(super::Tab::Shop));
    let first = app.store_request.expect("first request").id;
    let _ = app.update(Message::SelectAccount(alt.id));
    let _ = app.update(Message::SelectAccount(main.id));
    let latest = app.store_request.expect("latest request");

    let _ = app.update(Message::StorefrontLoaded(first, Err("boom".to_string())));

    assert_eq!(app.store_request, Some(latest));
    assert!(!app.status.contains("Store check failed"), "{}", app.status);
}

#[test]
fn an_old_loadout_reply_does_not_end_the_newer_load_for_the_same_account() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, alt) = two_account_app(dir.path());
    let _ = app.update(Message::TabSelected(super::Tab::Loadout));
    let first = app.loadout_request.expect("first request").id;
    let _ = app.update(Message::SelectAccount(alt.id));
    let _ = app.update(Message::SelectAccount(main.id));
    let latest = app.loadout_request.expect("latest request");

    let _ = app.update(Message::LoadoutLoaded(first, Err("boom".to_string())));

    assert_eq!(app.loadout_request, Some(latest));
    assert!(
        !app.status.contains("Loadout check failed"),
        "{}",
        app.status
    );
}

#[test]
fn clearing_the_image_cache_drops_shop_and_loadout_that_point_into_it() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.store_summary = Some(empty_store_summary());
    app.loadout_summary = Some(LoadoutSummary::without_loadout("old".to_string(), None));

    let _ = app.update(Message::ImageCacheCleared(Ok(())));

    assert_eq!(app.store_summary, None);
    assert_eq!(app.loadout_summary, None);
}

#[test]
fn the_image_cache_is_not_cleared_twice_at_once() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.image_cache = crate::image_cache::ImageCache::new(dir.path().join("images"));

    let first = app.update(Message::ClearImageCache);
    let second = app.update(Message::ClearImageCache);
    let _ = app.update(Message::ImageCacheCleared(Ok(())));
    let after = app.update(Message::ClearImageCache);

    assert_eq!(first.units(), 1);
    assert_eq!(second.units(), 0);
    assert_eq!(after.units(), 1);
}

#[test]
fn an_update_does_not_download_while_a_launch_runs() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.app_update_status =
        super::AppUpdateStatus::Available(crate::updater::sample_update("9.9.9"));
    app.launching_account = Some(AccountId::new());

    let task = app.update(Message::DownloadAppUpdate);

    assert_eq!(task.units(), 0);
    assert!(matches!(
        app.app_update_status,
        super::AppUpdateStatus::Available(_)
    ));
    assert!(status_message_is_error(&app.status), "{}", app.status);
}

fn downloading_update_app(dir: &Path) -> PrimeApp {
    let mut app = test_app(dir);
    app.app_update_status =
        super::AppUpdateStatus::Downloading(crate::updater::sample_update("9.9.9"));
    app
}

#[test]
fn a_launch_does_not_start_while_an_update_downloads() {
    let dir = tempdir().expect("temp dir");
    let mut app = downloading_update_app(dir.path());
    let account = account_with_backup(&app.repo.launcher_backups_dir(), "Main", "settings");
    app.state.push_account(account.clone());

    let task = app.update(Message::LaunchAccount(account.id));

    assert_eq!(task.units(), 0);
    assert_eq!(app.launch_preflight_account, None);
    assert!(app.status.contains("update"), "{}", app.status);
}

#[test]
fn a_login_capture_does_not_start_while_an_update_downloads() {
    let dir = tempdir().expect("temp dir");
    let mut app = downloading_update_app(dir.path());

    let _ = app.update(Message::AddAccount);

    assert!(!app.show_add_account_prompt);
    assert!(app.status.contains("update"), "{}", app.status);
}

#[test]
fn an_import_does_not_start_while_an_update_downloads() {
    let dir = tempdir().expect("temp dir");
    let mut app = downloading_update_app(dir.path());
    app.import_account_input = "export".to_string();

    let _ = app.update(Message::ConfirmImportAccount);

    assert!(!app.import_account_in_progress);
    assert!(app.status.contains("update"), "{}", app.status);
}

#[test]
fn a_failed_shop_load_is_kept_for_the_retry_panel() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    let _ = app.update(Message::TabSelected(super::Tab::Shop));
    let request = app.store_request.expect("shop request").id;

    let _ = app.update(Message::StorefrontLoaded(request, Err("boom".to_string())));

    assert_eq!(app.store_error.as_deref(), Some("boom"));
}

#[test]
fn trying_the_shop_again_starts_a_new_load() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    app.active_tab = super::Tab::Shop;
    app.store_error = Some("boom".to_string());

    let _ = app.update(Message::RetryShop);

    assert!(app.store_request.is_some());
    assert_eq!(app.store_error, None);
}

#[test]
fn trying_the_loadout_again_starts_a_new_load() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    app.active_tab = super::Tab::Loadout;
    app.loadout_summary = Some(LoadoutSummary {
        battle_pass_error: Some("500".to_string()),
        ..loaded_loadout()
    });

    let _ = app.update(Message::RetryLoadout);

    assert!(app.loadout_request.is_some());
}

#[test]
fn the_shop_waits_for_the_client_version() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    app.client_version_input.clear();

    let _ = app.update(Message::TabSelected(super::Tab::Shop));
    assert_eq!(app.store_request, None);

    let _ = app.update(Message::ClientVersionLoaded {
        user_requested: false,
        result: Ok("release-2".to_string()),
    });
    assert!(app.store_request.is_some());
}

#[test]
fn opening_the_shop_without_accounts_shows_no_error() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.client_version_input = "release-1".to_string();

    let _ = app.update(Message::TabSelected(super::Tab::Shop));

    assert!(!status_message_is_error(&app.status), "{}", app.status);
}

#[test]
fn launching_the_selected_account_keeps_its_shop() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = account_with_backup(&app.repo.launcher_backups_dir(), "Main", "settings");
    app.state.push_account(account.clone());
    app.state.select_account(account.id);
    app.store_summary = Some(empty_store_summary());
    app.launch_preflight_account = Some(account.id);

    let _ = app.update(Message::LaunchPreflightChecked(
        super::data::account_details::AccountActivityCheck {
            account_id: account.id,
            availability: AccountAvailability::Available,
        },
        false,
    ));

    assert_eq!(app.launching_account, Some(account.id));
    assert!(app.store_summary.is_some());
}

#[test]
fn launching_another_account_reloads_the_open_tab_for_it() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let main = account_with_backup(&backup_root, "Main", "settings");
    let alt = account_with_backup(&backup_root, "Alt", "settings");
    app.state.push_account(main.clone());
    app.state.push_account(alt.clone());
    app.state.select_account(main.id);
    app.client_version_input = "release-1".to_string();
    app.active_tab = super::Tab::Shop;
    app.store_summary = Some(empty_store_summary());
    app.launch_preflight_account = Some(alt.id);

    let _ = app.update(Message::LaunchPreflightChecked(
        super::data::account_details::AccountActivityCheck {
            account_id: alt.id,
            availability: AccountAvailability::Available,
        },
        false,
    ));

    assert_eq!(app.state.selected_account, Some(alt.id));
    assert!(
        app.store_request
            .is_some_and(|request| request.account_id == alt.id)
    );
    assert!(app.status.starts_with("Launching"), "{}", app.status);
}

fn loaded_loadout() -> LoadoutSummary {
    LoadoutSummary {
        account_level: None,
        gun_skins: Vec::new(),
        loadout_error: None,
        battle_pass: None,
        battle_pass_error: None,
    }
}

fn battle_pass_display() -> BattlePassProgressDisplay {
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
        earned_rewards: Vec::new(),
        unearned_rewards: Vec::new(),
        locked_paid_rewards: Vec::new(),
        loaded_at: iced::time::Instant::now(),
    }
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
        Ok(battle_pass_display()),
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
fn a_loadout_whose_battle_pass_failed_says_so() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account) = accounts_tab_app(dir.path());
    app.loadout_request = Some(super::ViewRequest {
        id: 1,
        account_id: account.id,
    });
    let summary = LoadoutSummary {
        battle_pass_error: Some("Riot returned 500".to_string()),
        ..loaded_loadout()
    };

    let _ = app.update(Message::LoadoutLoaded(
        1,
        Ok(LoadoutResult {
            account_id: account.id,
            summary,
            session: AuthSession::new("access", None, None, "Bearer", Some(3600), 100),
            launcher_session: None,
            identity: ApiIdentity {
                puuid: "puuid-a".to_string(),
                game_name: None,
                tag_line: None,
                shard: Shard::Na,
                region: None,
            },
        }),
    ));

    assert!(app.status.contains("battle pass failed"), "{}", app.status);
    assert!(app.status.contains("Riot returned 500"), "{}", app.status);
    assert!(status_message_is_error(&app.status), "{}", app.status);
}

#[test]
fn development_builds_do_not_report_update_checks_as_failed() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());

    let _ = app.update(Message::AppUpdateChecked {
        user_requested: true,
        result: Ok(crate::updater::UpdateCheckOutcome::NotInstalled),
    });

    assert!(!app.status.contains("failed"), "{}", app.status);
    assert!(
        app.status.contains("not an installed build"),
        "{}",
        app.status
    );
}

#[test]
fn a_manual_client_version_refresh_replaces_the_field() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.client_version_input = "x".to_string();

    let _ = app.update(Message::ClientVersionLoaded {
        user_requested: true,
        result: Ok("release-2".to_string()),
    });

    assert_eq!(app.client_version_input, "release-2");
    assert!(app.status.contains("release-2"), "{}", app.status);
}

#[test]
fn a_failed_manual_client_version_refresh_is_reported() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let _ = app.update(Message::RefreshClientVersion);

    let _ = app.update(Message::ClientVersionLoaded {
        user_requested: true,
        result: Err("offline".to_string()),
    });

    assert!(
        app.status
            .starts_with("Could not fetch Riot client version"),
        "{}",
        app.status
    );
    assert!(!loading_status_active(&app.status));
}

#[test]
fn the_startup_client_version_does_not_replace_a_startup_error() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.status = "Failed to load accounts: disk error".to_string();

    let _ = app.update(Message::ClientVersionLoaded {
        user_requested: false,
        result: Ok("release-1".to_string()),
    });

    assert_eq!(app.client_version_input, "release-1");
    assert_eq!(app.status, "Failed to load accounts: disk error");
}

#[test]
fn a_failed_startup_client_version_fetch_is_shown_and_retried() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.status = "Loaded 1 account profile(s)".to_string();

    let task = app.update(Message::ClientVersionLoaded {
        user_requested: false,
        result: Err("offline".to_string()),
    });

    assert!(
        app.status
            .starts_with("Could not fetch Riot client version"),
        "{}",
        app.status
    );
    assert!(task.units() > 0, "a retry is scheduled");
}

#[test]
fn a_background_update_check_leaves_the_status_alone() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.status = "Failed to load accounts: disk error".to_string();

    let _ = app.update(Message::AppUpdateChecked {
        user_requested: false,
        result: Ok(crate::updater::UpdateCheckOutcome::Available(
            crate::updater::sample_update("9.9.9"),
        )),
    });

    assert_eq!(app.status, "Failed to load accounts: disk error");
    assert!(app.app_update_status.prompt_update().is_some());
}

#[test]
fn failed_update_install_is_not_labelled_as_a_failed_check() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());

    let _ = app.update(Message::AppUpdatePrepared(Err("disk full".to_string())));

    assert_eq!(app.app_update_status.label(), "Update failed: disk full");
}

#[test]
fn availability_polling_resumes_when_the_window_is_restored() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.state
        .push_account(AccountProfile::new("Main", Shard::Na).expect("account"));
    app.client_version_input = "release-1".to_string();

    let _ = app.update(Message::WindowResized(iced::Size::new(0.0, 0.0)));
    assert!(app.window_minimized);
    assert!(!app.account_availability_loading);

    let _ = app.update(Message::WindowResized(iced::Size::new(900.0, 600.0)));
    assert!(!app.window_minimized);
    assert!(app.account_availability_loading);
}

#[test]
fn minimized_polling_only_runs_when_minimize_on_close_is_chosen() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.state
        .push_account(AccountProfile::new("Main", Shard::Na).expect("account"));
    app.client_version_input = "release-1".to_string();
    app.active_tab = super::Tab::Shop;
    app.window_minimized = true;
    let tick = || Message::AccountAvailabilityTimerTick(iced::time::Instant::now());
    assert!(app.state.minimize_on_close, "on by default");

    let _ = app.update(Message::MinimizeOnCloseToggled(false));

    assert_eq!(app.update(tick()).units(), 0);
    assert!(!app.account_availability_loading);

    let _ = app.update(Message::MinimizeOnCloseToggled(true));
    assert!(app.state.minimize_on_close);
    let _ = app.update(tick());
    assert!(app.account_availability_loading);
}

#[test]
fn minimize_on_close_is_not_changed_before_accounts_load() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.accounts_loaded = false;
    let _ = app.update(Message::MinimizeOnCloseToggled(false));
    assert!(
        app.state.minimize_on_close,
        "ignored until accounts.json loads"
    );
}

#[test]
fn availability_checks_keep_the_sessions_they_refresh() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = account_with_backup(&app.repo.launcher_backups_dir(), "Main", "settings");
    let backup = account.launcher_session.clone();
    app.state.push_account(account.clone());

    let _ = app.update(availability_refresh(account.id, backup));

    let session = app.state.accounts[0].session.as_ref().expect("session");
    assert_eq!(session.access_token, "fresh");
}

#[test]
fn availability_checks_drop_sessions_for_a_replaced_login() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = account_with_backup(&app.repo.launcher_backups_dir(), "Main", "settings");
    let current = account.launcher_session.clone();
    app.state.push_account(account.clone());
    let stale = LauncherSessionBackup {
        data_dir: dir.path().join("old-slot"),
        captured_at_unix: 50,
        puuid: "Main-puuid".to_string(),
    };

    let _ = app.update(availability_refresh(account.id, Some(stale)));

    assert!(app.state.accounts[0].session.is_none());
    assert_eq!(app.state.accounts[0].launcher_session, current);
}

#[test]
fn cache_account_api_context_rejects_missing_account() {
    let mut state = StoredState::default();
    let session = AuthSession::new("access", None, None, "Bearer", Some(3600), 100);

    let err = cache_account_api_context(
        &mut state,
        AccountId::new(),
        session,
        None,
        ApiIdentity {
            puuid: "puuid".to_string(),
            game_name: None,
            tag_line: None,
            shard: Shard::Na,
            region: None,
        },
    )
    .expect_err("missing account");

    assert!(err.contains("profile no longer exists"));
}

fn settings_profile_metadata(
    name: &str,
    purpose: GameSettingsProfilePurpose,
    captured_at_unix: i64,
) -> GameSettingsProfileMetadata {
    GameSettingsProfileMetadata {
        id: format!("{captured_at_unix}-profile-id"),
        name: name.to_string(),
        purpose,
        source_account_id: AccountId::new(),
        source_display_name: "Main".to_string(),
        source_puuid: "Main-puuid".to_string(),
        captured_at_unix,
        settings_version: Some(15),
        summary: Box::new(GameSettingsProfileSummary {
            sensitivity: Some(0.4),
            ..GameSettingsProfileSummary::default()
        }),
    }
}

fn settings_api_identity() -> ApiIdentity {
    ApiIdentity {
        puuid: "Main-puuid".to_string(),
        game_name: None,
        tag_line: None,
        shard: Shard::Na,
        region: None,
    }
}

fn settings_saved(account_id: AccountId, profile: GameSettingsProfileMetadata) -> Message {
    Message::AccountSettingsSaved(Ok(SavedGameSettingsResult {
        account_id,
        session: AuthSession::new("fresh", None, None, "Bearer", Some(3600), 100),
        launcher_session: None,
        identity: settings_api_identity(),
        profile,
    }))
}

fn settings_app(dir: &Path) -> (PrimeApp, AccountId) {
    let mut app = test_app(dir);
    app.settings_cloning = true;
    let account = AccountProfile::new("Main", Shard::Na).expect("account");
    let account_id = account.id;
    app.state.push_account(account);
    (app, account_id)
}

#[cfg(not(feature = "settings-cloning"))]
#[test]
fn settings_cloning_is_disabled_without_its_feature() {
    let dir = tempdir().expect("temp dir");

    assert!(!super::settings_cloning_enabled());
    assert!(!test_app(dir.path()).settings_cloning);
}

#[test]
fn settings_cloning_does_nothing_while_disabled() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    app.settings_cloning = false;
    let profile =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    app.settings_profiles = vec![profile.clone()];

    let tasks = [
        app.update(Message::RequestSavePreset(account_id)),
        app.update(Message::SaveSettingsPreset {
            account_id,
            name: "Main settings".to_string(),
        }),
        app.update(Message::RequestRenamePreset(profile.id.clone())),
        app.update(Message::RequestApplyPreset {
            profile_id: profile.id.clone(),
            account_id,
        }),
        app.update(Message::RequestRestoreSettings(account_id)),
        app.update(Message::RequestDeleteSettingsProfile(profile.id)),
    ];

    assert!(tasks.iter().all(|task| task.units() == 0));
    assert_eq!(app.settings_saving_account, None);
    assert_eq!(app.preset_name_prompt, None);
    assert_eq!(app.settings_check, None);
    assert_eq!(app.confirm_settings_change, None);
    assert_eq!(app.confirm_delete_settings_profile, None);
}

#[test]
fn settings_profiles_have_their_own_accounts_sub_tab() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = settings_app(dir.path());

    let _ = app.update(Message::AccountsTabSelected(
        super::AccountsTab::GameSettings,
    ));
    assert_eq!(app.active_accounts_tab, super::AccountsTab::GameSettings);

    // A finished capture brings the Accounts sub-tab back so its confirmation is in view.
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );
    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft)));
    assert_eq!(app.active_accounts_tab, super::AccountsTab::Accounts);
}

#[test]
fn adding_an_account_saves_its_settings_when_asked() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.settings_cloning = true;
    let draft = captured_account_draft(
        &app.repo.launcher_backups_dir(),
        "puuid-a",
        "Player",
        "NA1",
        Shard::Na,
    );

    let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft.clone())));
    let _ = app.update(Message::SaveSettingsOnAddToggled(true));
    let task = app.update(Message::ConfirmCapturedAccount);

    assert_eq!(app.state.accounts.len(), 1);
    assert_eq!(app.settings_saving_account, Some(draft.account_id));
    assert!(task.units() > 0);
    assert!(app.status.starts_with("Added "));
    assert!(
        app.status
            .ends_with("Saving its VALORANT settings as a preset")
    );
}

#[test]
fn adding_an_account_leaves_its_settings_unless_asked() {
    for (cloning, checked) in [(true, false), (false, true)] {
        let dir = tempdir().expect("temp dir");
        let mut app = test_app(dir.path());
        app.settings_cloning = cloning;
        let draft = captured_account_draft(
            &app.repo.launcher_backups_dir(),
            "puuid-a",
            "Player",
            "NA1",
            Shard::Na,
        );

        let _ = app.update(Message::CurrentAccountCaptureFinished(Ok(draft)));
        let _ = app.update(Message::SaveSettingsOnAddToggled(checked));
        let _ = app.update(Message::ConfirmCapturedAccount);

        assert_eq!(app.state.accounts.len(), 1);
        assert_eq!(app.settings_saving_account, None);
    }
}

#[test]
fn confirming_without_a_draft_saves_no_settings() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    app.state.select_account(account_id);
    app.save_settings_on_add = true;

    let _ = app.update(Message::ConfirmCapturedAccount);

    assert_eq!(app.settings_saving_account, None);
}

fn profile_ids(app: &PrimeApp) -> Vec<String> {
    app.settings_profiles
        .iter()
        .map(|profile| profile.id.clone())
        .collect()
}

#[test]
fn a_saved_preset_is_listed_first() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let older = settings_profile_metadata("Alt settings", GameSettingsProfilePurpose::Profile, 100);
    app.settings_profiles = vec![older.clone()];
    app.settings_saving_account = Some(account_id);
    let saved =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);

    let _ = app.update(settings_saved(account_id, saved.clone()));

    assert_eq!(app.settings_saving_account, None);
    assert_eq!(profile_ids(&app), [saved.id, older.id]);
    assert_eq!(app.status, "Saved preset Main settings");
}

#[test]
fn saved_settings_profile_is_listed_when_the_account_update_fails() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let saved =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);

    let _ = app.update(settings_saved(AccountId::new(), saved.clone()));

    assert_eq!(profile_ids(&app), [saved.id]);
    assert!(
        app.status.contains("account update failed"),
        "{}",
        app.status
    );
}

#[test]
fn saving_a_preset_asks_for_its_name_first() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());

    let task = app.update(Message::RequestSavePreset(account_id));

    assert_eq!(task.units(), 0);
    assert_eq!(app.settings_saving_account, None);
    assert_eq!(
        app.preset_name_prompt,
        Some(PresetNamePrompt {
            target: PresetNameTarget::New(account_id),
            name: "Main settings".to_string(),
        })
    );

    let _ = app.update(Message::PresetNameChanged("   ".to_string()));
    let task = app.update(Message::ConfirmPresetName);

    assert_eq!(task.units(), 0);
    assert!(app.preset_name_prompt.is_some());

    let _ = app.update(Message::PresetNameChanged(" Aim duels ".to_string()));
    let task = app.update(Message::ConfirmPresetName);

    assert!(task.units() > 0);
    assert_eq!(app.preset_name_prompt, None);
    assert_eq!(app.settings_saving_account, Some(account_id));
    assert!(app.status.contains("Aim duels"), "{}", app.status);
}

#[test]
fn the_name_dialog_stays_open_when_the_save_cannot_start() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestSavePreset(account_id));
    app.settings_applying_account = Some(AccountId::new());

    let task = app.update(Message::ConfirmPresetName);

    assert_eq!(task.units(), 0);
    assert!(app.preset_name_prompt.is_some());
    assert_eq!(app.settings_saving_account, None);
}

#[test]
fn renaming_a_preset_starts_from_its_name() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = settings_app(dir.path());
    let preset =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    app.settings_profiles = vec![preset.clone()];

    let _ = app.update(Message::RequestRenamePreset(preset.id.clone()));

    assert_eq!(
        app.preset_name_prompt,
        Some(PresetNamePrompt {
            target: PresetNameTarget::Rename(preset.id.clone()),
            name: "Main settings".to_string(),
        })
    );

    let _ = app.update(Message::PresetNameChanged("Old crosshair".to_string()));
    let task = app.update(Message::ConfirmPresetName);

    assert!(task.units() > 0);
    assert_eq!(app.preset_name_prompt, None);

    let renamed = GameSettingsProfileMetadata {
        name: "Old crosshair".to_string(),
        ..preset
    };
    let _ = app.update(Message::PresetRenamed(Ok(renamed.clone())));

    assert_eq!(app.settings_profiles, [renamed]);
    assert_eq!(app.status, "Renamed preset to Old crosshair");
}

#[test]
fn show_all_settings_opens_and_closes_one_preset() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = settings_app(dir.path());
    let main = settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    let alt = settings_profile_metadata("Alt settings", GameSettingsProfilePurpose::Profile, 300);
    app.settings_profiles = vec![main.clone(), alt.clone()];

    let task = app.update(Message::TogglePresetSettings(main.id.clone()));

    assert_eq!(task.units(), 0);
    assert!(app.expanded_presets.contains(&main.id));
    assert!(!app.expanded_presets.contains(&alt.id));

    let _ = app.update(Message::TogglePresetSettings(main.id));

    assert!(app.expanded_presets.is_empty());
}

#[test]
fn escape_closes_the_preset_dialogs() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());

    let _ = app.update(Message::RequestSavePreset(account_id));
    let _ = app.update(Message::EscapePressed);
    assert_eq!(app.preset_name_prompt, None);

    let _ = app.update(Message::RequestRestoreSettings(account_id));
    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Available,
        false,
    ));
    assert!(app.confirm_settings_change.is_some());
    let _ = app.update(Message::EscapePressed);
    assert_eq!(app.confirm_settings_change, None);
}

fn settings_checked(
    request_id: u64,
    account_id: AccountId,
    availability: AccountAvailability,
    valorant_running: bool,
) -> Message {
    Message::SettingsPreflightChecked(
        request_id,
        account_id,
        Some((
            super::data::account_details::AccountActivityCheck {
                account_id,
                availability,
            },
            None,
        )),
        valorant_running,
    )
}

fn with_fresh_availability(
    app: &mut PrimeApp,
    account_id: AccountId,
    availability: AccountAvailability,
) {
    app.account_availability_checked_at
        .insert(account_id, iced::time::Instant::now());
    app.account_availability.insert(account_id, availability);
}

fn pending_warning(app: &PrimeApp) -> Option<&str> {
    app.confirm_settings_change
        .as_ref()
        .and_then(|pending| pending.warning.as_deref())
}

/// Delivers the local-only result sent when the poll's result was fresh, so no Riot check ran.
fn settings_checked_locally(app: &mut PrimeApp, account_id: AccountId, valorant_running: bool) {
    let _ = app.update(Message::SettingsPreflightChecked(
        app.next_request_id,
        account_id,
        None,
        valorant_running,
    ));
}

#[test]
fn a_fresh_result_opens_the_dialog_without_checking() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let preset =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    app.settings_profiles = vec![preset.clone()];
    with_fresh_availability(&mut app, account_id, AccountAvailability::Available);
    let change = SettingsChange::Apply {
        account_id,
        profile_id: preset.id.clone(),
    };

    let _ = app.update(Message::RequestApplyPreset {
        profile_id: preset.id,
        account_id,
    });

    // Only the local VALORANT check runs; the dialog waits for it.
    assert_eq!(app.confirm_settings_change, None);
    assert_eq!(
        app.settings_check,
        Some(PendingSettingsCheck {
            request_id: app.next_request_id,
            change: change.clone(),
        })
    );

    settings_checked_locally(&mut app, account_id, false);

    assert_eq!(app.settings_check, None);
    assert_eq!(
        app.confirm_settings_change,
        Some(PendingSettingsChange {
            change,
            warning: None,
            check_failed: false,
        })
    );
}

#[test]
fn a_fresh_result_in_a_match_warns_at_once() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    with_fresh_availability(
        &mut app,
        account_id,
        AccountAvailability::Unavailable {
            reason: "in match".to_string(),
        },
    );

    let _ = app.update(Message::RequestRestoreSettings(account_id));
    settings_checked_locally(&mut app, account_id, false);

    let warning = pending_warning(&app).expect("warning");
    assert!(warning.contains("in match"), "{warning}");
}

#[test]
fn an_old_result_checks_first_with_a_loading_state() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());

    let task = app.update(Message::RequestRestoreSettings(account_id));

    assert!(task.units() > 0);
    assert_eq!(app.confirm_settings_change, None);
    assert_eq!(
        app.settings_check,
        Some(PendingSettingsCheck {
            request_id: app.next_request_id,
            change: SettingsChange::Restore(account_id),
        })
    );
    assert!(app.settings_work_in_progress());
    assert!(super::loading_indicator_active(&app));

    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Unavailable {
            reason: "in lobby".to_string(),
        },
        false,
    ));

    assert_eq!(app.settings_check, None);
    assert!(!app.settings_work_in_progress());
    let pending = app.confirm_settings_change.as_ref().expect("open");
    assert!(!pending.check_failed);
    assert!(pending_warning(&app).expect("warning").contains("in lobby"));
    // The check only looks for the game, so it doesn't relabel the Accounts tab.
    assert_eq!(app.account_availability.get(&account_id), None);
}

#[test]
fn a_check_finding_a_match_warns() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));

    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Unavailable {
            reason: "in match".to_string(),
        },
        false,
    ));

    let warning = pending_warning(&app).expect("warning");
    assert!(warning.contains("in match"), "{warning}");
}

#[test]
fn a_cached_unknown_result_starts_a_real_check() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    with_fresh_availability(
        &mut app,
        account_id,
        AccountAvailability::activity_check_failed(),
    );

    let task = app.update(Message::RequestRestoreSettings(account_id));

    assert!(task.units() > 0);
    assert_eq!(app.confirm_settings_change, None);
    assert!(app.settings_check.is_some());

    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Available,
        false,
    ));

    // The check's own result wins over the cached failure.
    let pending = app.confirm_settings_change.as_ref().expect("open");
    assert!(!pending.check_failed);
    assert_eq!(pending.warning, None);
}

#[test]
fn a_poll_over_90_seconds_old_starts_a_real_check() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let in_match = AccountAvailability::Unavailable {
        reason: "in match".to_string(),
    };
    with_fresh_availability(&mut app, account_id, in_match.clone());
    app.account_availability_checked_at.insert(
        account_id,
        iced::time::Instant::now() - std::time::Duration::from_secs(91),
    );

    let task = app.update(Message::RequestRestoreSettings(account_id));

    assert!(task.units() > 0);
    assert_eq!(app.confirm_settings_change, None);
    assert!(app.settings_check.is_some());

    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Available,
        false,
    ));

    // The warning comes from the check, and the Accounts tab keeps the poll's result.
    assert!(app.confirm_settings_change.is_some());
    assert_eq!(pending_warning(&app), None);
    assert_eq!(app.account_availability.get(&account_id), Some(&in_match));
}

#[test]
fn a_poll_that_has_only_started_does_not_make_an_old_result_fresh() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    app.client_version_input = "release-1".to_string();
    // Left over from a poll long ago, before the user spent a while on another tab.
    app.account_availability
        .insert(account_id, AccountAvailability::Available);
    app.active_tab = super::Tab::Shop;

    let task = app.update(Message::TabSelected(super::Tab::Accounts));

    assert!(task.units() > 0);
    assert!(app.account_availability_loading);
    assert_eq!(app.fresh_availability(account_id), None);
}

#[test]
fn a_poll_result_arriving_makes_it_fresh() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());

    let _ = app.update(Message::AccountAvailabilitiesLoaded(
        AccountAvailabilityRefresh {
            accounts: vec![super::data::account_details::AccountActivityCheck {
                account_id,
                availability: AccountAvailability::Available,
            }],
            refreshed_sessions: vec![],
        },
    ));

    assert_eq!(
        app.fresh_availability(account_id),
        Some(&AccountAvailability::Available)
    );
}

#[test]
fn a_launch_check_result_makes_it_fresh() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    app.launch_preflight_account = Some(account_id);
    let in_lobby = AccountAvailability::Unavailable {
        reason: "in lobby".to_string(),
    };

    let _ = app.update(Message::LaunchPreflightChecked(
        super::data::account_details::AccountActivityCheck {
            account_id,
            availability: in_lobby.clone(),
        },
        false,
    ));

    assert_eq!(app.fresh_availability(account_id), Some(&in_lobby));
}

#[test]
fn a_poll_tick_during_a_settings_check_starts_no_poll() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    app.client_version_input = "release-1".to_string();
    app.active_tab = super::Tab::Accounts;
    app.settings_check = Some(PendingSettingsCheck {
        request_id: 1,
        change: SettingsChange::Restore(account_id),
    });

    let task = app.update(Message::AccountAvailabilityTimerTick(
        iced::time::Instant::now(),
    ));

    assert_eq!(task.units(), 0);
    assert!(!app.account_availability_loading);
}

#[test]
fn a_poll_tick_while_settings_are_applied_starts_no_poll() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    app.client_version_input = "release-1".to_string();
    app.active_tab = super::Tab::Accounts;
    app.settings_applying_account = Some(account_id);

    let task = app.update(Message::AccountAvailabilityTimerTick(
        iced::time::Instant::now(),
    ));

    assert_eq!(task.units(), 0);
    assert!(!app.account_availability_loading);
}

#[test]
fn a_poll_tick_while_a_preset_is_saved_starts_no_poll() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    app.client_version_input = "release-1".to_string();
    app.active_tab = super::Tab::Accounts;
    app.settings_saving_account = Some(account_id);

    let task = app.update(Message::AccountAvailabilityTimerTick(
        iced::time::Instant::now(),
    ));

    assert_eq!(task.units(), 0);
    assert!(!app.account_availability_loading);
}

#[test]
fn a_failed_check_opens_the_dialog_with_a_note() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));

    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::activity_check_failed(),
        false,
    ));

    let pending = app.confirm_settings_change.as_ref().expect("open");
    assert!(pending.check_failed);
    assert_eq!(pending.warning, None);
}

#[test]
fn applying_warns_when_valorant_is_running() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    with_fresh_availability(&mut app, account_id, AccountAvailability::Available);
    let _ = app.update(Message::RequestRestoreSettings(account_id));

    settings_checked_locally(&mut app, account_id, true);

    let warning = pending_warning(&app).expect("warning");
    assert!(warning.contains("VALORANT is running"), "{warning}");
}

#[test]
fn a_check_for_another_account_changes_nothing() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));
    let checking = app.settings_check.clone();
    let other_account = AccountId::new();

    let _ = app.update(settings_checked(
        app.next_request_id,
        other_account,
        AccountAvailability::Available,
        true,
    ));

    assert_eq!(app.confirm_settings_change, None);
    assert_eq!(app.settings_check, checking);
    assert_eq!(app.account_availability.get(&other_account), None);
}

#[test]
fn a_check_from_an_earlier_request_changes_nothing() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));
    let first_request = app.next_request_id;
    let _ = app.update(Message::EscapePressed);
    with_fresh_availability(&mut app, account_id, AccountAvailability::Available);
    let _ = app.update(Message::RequestRestoreSettings(account_id));
    let checking = app.settings_check.clone();

    let _ = app.update(settings_checked(
        first_request,
        account_id,
        AccountAvailability::Unavailable {
            reason: "in lobby".to_string(),
        },
        true,
    ));

    assert_eq!(app.confirm_settings_change, None);
    assert_eq!(app.settings_check, checking);
    assert_eq!(
        app.account_availability.get(&account_id),
        Some(&AccountAvailability::Available)
    );
}

fn check_with_session(request_id: u64, account_id: AccountId, session: &AuthSession) -> Message {
    Message::SettingsPreflightChecked(
        request_id,
        account_id,
        Some((
            super::data::account_details::AccountActivityCheck {
                account_id,
                availability: AccountAvailability::Available,
            },
            Some(super::data::account_details::RefreshedApiContext {
                account_id,
                session: session.clone(),
                launcher_session: None,
                identity: settings_api_identity(),
            }),
        )),
        false,
    )
}

fn saved_session(app: &PrimeApp, account_id: AccountId) -> Option<&AuthSession> {
    app.state
        .accounts
        .iter()
        .find(|account| account.id == account_id)
        .and_then(|account| account.session.as_ref())
}

#[test]
fn escape_while_checking_cancels_but_keeps_the_checks_session() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));
    let request_id = app.next_request_id;

    let _ = app.update(Message::EscapePressed);

    assert_eq!(app.settings_check, None);
    assert!(!app.settings_work_in_progress());

    let session = AuthSession::new("fresh", None, None, "Bearer", Some(3600), 100);
    let task = app.update(check_with_session(request_id, account_id, &session));

    assert!(task.units() > 0, "saves accounts.json");
    assert_eq!(saved_session(&app, account_id), Some(&session));
    assert_eq!(app.confirm_settings_change, None);
}

#[test]
fn the_checks_refreshed_session_is_saved_before_the_dialog_opens() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));
    let session = AuthSession::new("fresh", None, None, "Bearer", Some(3600), 100);

    let task = app.update(check_with_session(
        app.next_request_id,
        account_id,
        &session,
    ));

    assert!(task.units() > 0, "saves accounts.json");
    assert_eq!(saved_session(&app, account_id), Some(&session));
    assert!(app.confirm_settings_change.is_some());
}

#[test]
fn other_settings_work_waits_for_the_check() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let preset =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    app.settings_profiles = vec![preset.clone()];
    let _ = app.update(Message::RequestRestoreSettings(account_id));
    let checking = app.settings_check.clone();

    let tasks = [
        app.update(Message::RequestSavePreset(account_id)),
        app.update(Message::RequestApplyPreset {
            profile_id: preset.id,
            account_id,
        }),
        app.update(Message::ConfirmSettingsChange),
    ];

    assert!(tasks.iter().all(|task| task.units() == 0));
    assert_eq!(app.preset_name_prompt, None);
    assert_eq!(app.settings_check, checking);
    assert_eq!(app.settings_applying_account, None);
}

#[test]
fn a_check_for_a_deleted_account_opens_nothing() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let _ = app.update(Message::RequestRestoreSettings(account_id));
    app.state.remove_account(account_id);

    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Available,
        false,
    ));

    assert_eq!(app.settings_check, None);
    assert_eq!(app.confirm_settings_change, None);
}

#[test]
fn confirming_apply_starts_it() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let preset =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    app.settings_profiles = vec![preset.clone()];
    let _ = app.update(Message::RequestApplyPreset {
        profile_id: preset.id,
        account_id,
    });
    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Available,
        false,
    ));

    let task = app.update(Message::ConfirmSettingsChange);

    assert!(task.units() > 0);
    assert_eq!(app.confirm_settings_change, None);
    assert_eq!(app.settings_applying_account, Some(account_id));
    assert!(app.status.contains("Main settings"), "{}", app.status);
}

fn applied(
    account_id: AccountId,
    source_profile: GameSettingsProfileMetadata,
    backup_profile: Option<GameSettingsProfileMetadata>,
) -> Message {
    Message::SavedSettingsApplied(Ok(AppliedGameSettingsResult {
        account_id,
        session: AuthSession::new("fresh", None, None, "Bearer", Some(3600), 100),
        launcher_session: None,
        identity: settings_api_identity(),
        source_profile,
        backup_profile,
    }))
}

#[test]
fn applied_status_points_to_restore_only_when_it_saved_the_accounts_settings() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let preset =
        settings_profile_metadata("Alt settings", GameSettingsProfilePurpose::Profile, 200);
    let backup = settings_profile_metadata(
        "Main original settings",
        GameSettingsProfilePurpose::Backup,
        300,
    );

    app.settings_applying_account = Some(account_id);
    let task = app.update(applied(account_id, preset.clone(), Some(backup)));

    assert!(task.units() > 0, "reloads the list to show the new backup");
    assert_eq!(app.settings_applying_account, None);
    assert_eq!(
        app.status,
        "Applied preset Alt settings. Its own settings were saved; restore them from Game settings"
    );

    let _ = app.update(applied(account_id, preset, None));

    assert_eq!(app.status, "Applied preset Alt settings");
}

#[test]
fn restoring_asks_first_then_starts() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());

    let task = app.update(Message::RequestRestoreSettings(account_id));
    assert!(task.units() > 0);
    let _ = app.update(settings_checked(
        app.next_request_id,
        account_id,
        AccountAvailability::Available,
        false,
    ));

    assert_eq!(
        app.confirm_settings_change,
        Some(PendingSettingsChange {
            change: SettingsChange::Restore(account_id),
            warning: None,
            check_failed: false,
        })
    );
    assert_eq!(app.settings_applying_account, None);

    let task = app.update(Message::ConfirmSettingsChange);

    assert!(task.units() > 0);
    assert_eq!(app.confirm_settings_change, None);
    assert_eq!(app.settings_applying_account, Some(account_id));

    let task = app.update(Message::SettingsRestored(Ok(RestoredGameSettingsResult {
        account_id,
        session: AuthSession::new("fresh", None, None, "Bearer", Some(3600), 100),
        launcher_session: None,
        identity: settings_api_identity(),
    })));

    assert!(
        task.units() > 0,
        "reloads the list without the restored backup"
    );
    assert_eq!(app.settings_applying_account, None);
    assert_eq!(app.status, "Restored original settings");
}

#[test]
fn restore_waits_for_other_settings_work() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    app.settings_saving_account = Some(account_id);

    let task = app.update(Message::RequestRestoreSettings(account_id));

    assert_eq!(task.units(), 0);
    assert_eq!(app.settings_check, None);
    assert_eq!(app.confirm_settings_change, None);
}

#[test]
fn each_accounts_original_settings_are_its_oldest_backup() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account_id) = settings_app(dir.path());
    let backup_of = |account_id, captured_at_unix| GameSettingsProfileMetadata {
        source_account_id: account_id,
        ..settings_profile_metadata(
            "Main original settings",
            GameSettingsProfilePurpose::Backup,
            captured_at_unix,
        )
    };
    let other_account = AccountId::new();
    let newer = backup_of(account_id, 300);
    let original = backup_of(account_id, 100);
    let other = backup_of(other_account, 200);
    let preset = GameSettingsProfileMetadata {
        source_account_id: account_id,
        ..settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 50)
    };
    app.settings_profiles = vec![newer, other.clone(), original.clone(), preset];

    let originals = super::screens::original_settings(&app);

    assert_eq!(
        originals
            .iter()
            .map(|original| original.id.as_str())
            .collect::<Vec<_>>(),
        [original.id.as_str(), other.id.as_str()]
    );
}

#[test]
fn deleting_a_preset_asks_first() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _) = settings_app(dir.path());
    let deleted =
        settings_profile_metadata("Main settings", GameSettingsProfilePurpose::Profile, 200);
    let kept = settings_profile_metadata("Alt settings", GameSettingsProfilePurpose::Profile, 100);
    app.settings_profiles = vec![deleted.clone(), kept.clone()];

    let task = app.update(Message::RequestDeleteSettingsProfile(deleted.id.clone()));

    assert_eq!(task.units(), 0);
    assert_eq!(
        app.confirm_delete_settings_profile,
        Some(deleted.id.clone())
    );

    let task = app.update(Message::ConfirmDeleteSettingsProfile);

    assert!(task.units() > 0);
    assert_eq!(app.confirm_delete_settings_profile, None);

    let _ = app.update(Message::SettingsProfileDeleted(deleted.id.clone(), Ok(())));

    assert_eq!(profile_ids(&app), std::slice::from_ref(&kept.id));
    assert_eq!(app.status, "Deleted Main settings");
}

fn seconds_ago(seconds: u64) -> iced::time::Instant {
    iced::time::Instant::now()
        .checked_sub(Duration::from_secs(seconds))
        .expect("earlier instant")
}

fn loadout_with_battle_pass(
    remaining_seconds: i64,
    loaded_at: iced::time::Instant,
) -> LoadoutSummary {
    LoadoutSummary {
        battle_pass: Some(BattlePassProgressDisplay {
            remaining_seconds: Some(remaining_seconds),
            loaded_at,
            ..battle_pass_display()
        }),
        ..loaded_loadout()
    }
}

#[test]
fn countdown_timer_runs_only_where_a_countdown_shows() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.store_summary = Some(empty_store_summary());
    app.loadout_summary = Some(loadout_with_battle_pass(3_600, iced::time::Instant::now()));

    app.active_tab = super::Tab::Accounts;
    assert!(!countdown_timer_active(&app));

    app.active_tab = super::Tab::Shop;
    assert!(countdown_timer_active(&app));

    app.window_minimized = true;
    assert!(!countdown_timer_active(&app));
    app.window_minimized = false;

    app.active_tab = super::Tab::Loadout;
    app.active_loadout_tab = super::LoadoutTab::Skins;
    assert!(!countdown_timer_active(&app));

    app.active_loadout_tab = super::LoadoutTab::BattlePass;
    assert!(countdown_timer_active(&app));
}

#[test]
fn opening_the_shop_after_its_reset_reloads_it() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    app.store_summary = Some(StoreSummary {
        daily_remaining_seconds: 10,
        loaded_at: seconds_ago(20),
        ..empty_store_summary()
    });

    let _ = app.update(Message::TabSelected(super::Tab::Shop));

    assert!(app.store_request.is_some());
    assert!(app.store_summary.is_none());
}

#[test]
fn an_ended_battle_pass_reloads_the_loadout() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    app.active_tab = super::Tab::Loadout;
    app.active_loadout_tab = super::LoadoutTab::BattlePass;
    app.loadout_summary = Some(loadout_with_battle_pass(10, seconds_ago(20)));

    let _ = app.update(Message::ShopTimerTick(iced::time::Instant::now()));

    assert!(app.loadout_request.is_some());
}

#[test]
fn a_battle_pass_that_had_already_ended_does_not_keep_reloading() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    app.active_tab = super::Tab::Loadout;
    app.active_loadout_tab = super::LoadoutTab::BattlePass;
    app.loadout_summary = Some(loadout_with_battle_pass(0, seconds_ago(20)));

    let _ = app.update(Message::ShopTimerTick(iced::time::Instant::now()));

    assert!(app.loadout_request.is_none());
    assert!(!countdown_timer_active(&app));
}

fn priced_bundle_json(
    base_cost: Option<i64>,
    discounted_cost: Option<i64>,
    items: serde_json::Value,
) -> serde_json::Value {
    let mut bundle = featured_bundle_json("bundle", 3_600);
    bundle["Items"] = items;
    if let Some(cost) = base_cost {
        bundle["TotalBaseCost"] = serde_json::json!({ "vp": cost });
    }
    if let Some(cost) = discounted_cost {
        bundle["TotalDiscountedCost"] = serde_json::json!({ "vp": cost });
    }
    bundle
}

fn bundle_item_json(
    base_price: i64,
    discount_percent: i64,
    discounted_price: i64,
) -> serde_json::Value {
    serde_json::json!({
        "Item": { "ItemTypeID": "skin", "ItemID": "item", "Amount": 1 },
        "BasePrice": base_price,
        "CurrencyID": "vp",
        "DiscountPercent": discount_percent,
        "DiscountedPrice": discounted_price,
        "IsPromoItem": false
    })
}

#[test]
fn offer_prices_use_thousands_separators() {
    let summary = summary_with_bundles(
        vec![priced_bundle_json(
            Some(7_100),
            Some(7_100),
            serde_json::json!([]),
        )],
        3_600,
        iced::time::Instant::now(),
    );

    let price = summary.featured_bundles[0].price.as_ref().expect("price");

    assert!(price.label().starts_with("7,100 "), "{}", price.label());
}

#[test]
fn a_discounted_bundle_shows_its_original_price_and_discount() {
    let summary = summary_with_bundles(
        vec![priced_bundle_json(
            Some(8_000),
            Some(6_000),
            serde_json::json!([]),
        )],
        3_600,
        iced::time::Instant::now(),
    );
    let bundle = &summary.featured_bundles[0];

    assert_eq!(bundle.price.as_ref().map(|price| price.amount), Some(6_000));
    assert_eq!(
        bundle.original_price.as_ref().map(|price| price.amount),
        Some(8_000)
    );
    assert_eq!(bundle.discount_percent, 25);
}

#[test]
fn a_bundle_at_full_price_shows_no_discount() {
    let summary = summary_with_bundles(
        vec![priced_bundle_json(
            Some(8_000),
            Some(8_000),
            serde_json::json!([]),
        )],
        3_600,
        iced::time::Instant::now(),
    );
    let bundle = &summary.featured_bundles[0];

    assert_eq!(bundle.original_price, None);
    assert_eq!(bundle.discount_percent, 0);
}

#[test]
fn a_free_bundle_item_is_not_counted_at_full_price() {
    let summary = summary_with_bundles(
        vec![priced_bundle_json(
            None,
            None,
            serde_json::json!([
                bundle_item_json(1_000, 100, 0),
                bundle_item_json(500, 0, 500)
            ]),
        )],
        3_600,
        iced::time::Instant::now(),
    );

    assert_eq!(
        summary.featured_bundles[0]
            .price
            .as_ref()
            .map(|price| price.amount),
        Some(500)
    );
}

#[test]
fn rarity_ranks_match_the_game() {
    use super::data::shop::rarity_rank;

    let ranks = [
        "Select Edition",
        "Deluxe Edition",
        "Premium Edition",
        "Exclusive Edition",
        "Ultra Edition",
    ]
    .map(rarity_rank);

    assert!(
        ranks.is_sorted_by(|lower, higher| lower < higher),
        "{ranks:?}"
    );
}

#[test]
fn rarity_colors_match_the_game() {
    use super::data::shop::RarityTier;

    assert_eq!(RarityTier::Select.highlight_rgb(), [0x5a, 0x9f, 0xe2]);
    assert_eq!(RarityTier::Deluxe.highlight_rgb(), [0x00, 0x95, 0x87]);
    assert_eq!(RarityTier::Premium.highlight_rgb(), [0xd1, 0x54, 0x8d]);
    assert_eq!(RarityTier::Exclusive.highlight_rgb(), [0xf5, 0x95, 0x5b]);
    assert_eq!(RarityTier::Ultra.highlight_rgb(), [0xfa, 0xd6, 0x63]);
}

#[test]
fn a_quoted_riot_client_path_is_saved_without_quotes() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let client = dir.path().join("RiotClientServices.exe");
    fs::write(&client, "exe").expect("client");
    app.riot_client_path_input = format!("\"{}\"", client.display());

    let _ = app.update(Message::SaveSettings);

    assert_eq!(app.state.riot_client_path, Some(client));
    assert_eq!(app.status, "Saved settings");
}

#[test]
fn a_riot_client_path_that_does_not_exist_is_not_saved() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.riot_client_path_input = dir.path().join("missing.exe").display().to_string();

    let task = app.update(Message::SaveSettings);

    assert_eq!(task.units(), 0);
    assert_eq!(app.state.riot_client_path, None);
    assert!(status_message_is_error(&app.status), "{}", app.status);
}

#[test]
fn an_empty_riot_client_path_means_find_it_automatically() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.state.riot_client_path = Some(dir.path().join("old.exe"));
    app.riot_client_path_input = "  ".to_string();

    let _ = app.update(Message::SaveSettings);

    assert_eq!(app.state.riot_client_path, None);
}

#[test]
fn an_edited_riot_client_path_shows_as_unsaved_until_saved() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let client = dir.path().join("RiotClientServices.exe");
    fs::write(&client, "exe").expect("client");
    assert!(!app.riot_client_path_unsaved());

    let _ = app.update(Message::RiotClientPathChanged(client.display().to_string()));
    assert!(app.riot_client_path_unsaved());

    let _ = app.update(Message::SaveSettings);
    assert!(!app.riot_client_path_unsaved());
}

#[test]
fn a_failed_update_download_can_be_tried_again() {
    let dir = tempdir().expect("temp dir");
    let mut app = downloading_update_app(dir.path());

    let _ = app.update(Message::AppUpdatePrepared(Err("disk full".to_string())));

    assert_eq!(
        app.app_update_status
            .pending_update()
            .map(|update| update.latest_version.as_str()),
        Some("9.9.9")
    );
    assert_eq!(app.app_update_status.label(), "Update failed: disk full");
}

#[test]
fn opening_settings_refreshes_the_image_cache_size() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.image_cache = crate::image_cache::ImageCache::new(dir.path().join("images"));

    let task = app.update(Message::TabSelected(super::Tab::Settings));

    // Restoring the tab's scroll position is one task; the size refresh is another.
    assert_eq!(task.units(), 2);
}

fn rank_result(account_id: AccountId, rank: Result<Option<CompetitiveRank>, String>) -> Message {
    Message::AccountRanksLoaded {
        result: super::data::account_details::AccountRanksResult {
            ranks: vec![super::data::account_details::AccountRankResult {
                account_id,
                rank,
                account_level: Ok(20),
                penalty_status: Ok(AccountPenaltyStatus::default()),
                session: AuthSession::new("fresh", None, None, "Bearer", Some(3600), 100),
                launcher_session: None,
                identity: ApiIdentity {
                    puuid: "Main-puuid".to_string(),
                    game_name: None,
                    tag_line: None,
                    shard: Shard::Na,
                    region: None,
                },
            }],
            failures: vec![],
        },
        announce: false,
    }
}

#[test]
fn only_the_accounts_being_loaded_show_loading() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, alt) = two_account_app(dir.path());

    let _ = app.update(Message::SelectAccount(alt.id));

    assert!(app.account_ranks_loading.contains(&alt.id));
    assert!(!app.account_ranks_loading.contains(&main.id));
}

#[test]
fn an_account_with_no_rank_reads_unranked_and_a_failed_one_unavailable() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, alt) = two_account_app(dir.path());

    let _ = app.update(rank_result(main.id, Ok(None)));
    let _ = app.update(rank_result(alt.id, Err("offline".to_string())));

    assert_eq!(
        super::screens::missing_rank_label(&app, main.id),
        "Unranked"
    );
    assert_eq!(
        super::screens::missing_rank_label(&app, alt.id),
        "Rank unavailable"
    );
}

fn key_press(key: iced::keyboard::Key) -> iced::keyboard::Event {
    iced::keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::default(),
        text: None,
        repeat: false,
    }
}

#[test]
fn only_the_escape_key_asks_to_close() {
    let escape = key_press(iced::keyboard::Key::Named(
        iced::keyboard::key::Named::Escape,
    ));
    let enter = key_press(iced::keyboard::Key::Named(
        iced::keyboard::key::Named::Enter,
    ));

    assert!(matches!(
        super::escape_key_message(escape),
        Some(Message::EscapePressed)
    ));
    assert!(super::escape_key_message(enter).is_none());
}

#[test]
fn escape_closes_the_open_dialog() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let _ = app.update(Message::OpenImportAccount);

    let _ = app.update(Message::EscapePressed);

    assert!(!app.show_import_account_prompt);
}

#[test]
fn escape_closes_an_open_account_menu() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, _) = two_account_app(dir.path());
    app.open_account_menu = Some(main.id);

    let _ = app.update(Message::EscapePressed);

    assert_eq!(app.open_account_menu, None);
}

#[test]
fn escape_does_not_close_an_import_that_is_running() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let _ = app.update(Message::OpenImportAccount);
    app.import_account_in_progress = true;

    let _ = app.update(Message::EscapePressed);

    assert!(app.show_import_account_prompt);
}

#[test]
fn window_icon_decodes() {
    assert!(
        iced::window::icon::from_file_data(include_bytes!("../../assets/icon.png"), None).is_ok()
    );
}
