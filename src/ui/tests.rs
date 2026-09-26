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
use super::data::launch_flow::CapturedAccountDraft;
use super::data::launch_flow::{
    LaunchAccountResult, is_pending_launcher_capture_error, load_accounts, require_launcher_session,
};
use super::data::loadout::{
    LoadoutSummary, battle_pass_progress_from_responses, weapon_category, weapon_order,
};
use super::data::non_empty_path;
use super::data::session::ApiIdentity;
use super::data::shop::{
    StoreAccessoryDisplay, StoreBundleDisplay, StoreOfferDisplay, StoreSummary, format_whole_number,
};
use super::{
    Message, PrimeApp, loading_status_active, masked_account_export_payload, status_bar_visible,
};
use crate::account::{
    AccountId, AccountPenalty, AccountPenaltyDuration, AccountPenaltyStatus, AccountProfile,
    AuthSession, CompetitiveRank, LauncherSessionBackup, Shard,
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
        ["Give Back Bundle (1420 VP), 1 item"]
    );
    assert_eq!(
        summary
            .daily_offers
            .iter()
            .map(StoreOfferDisplay::label)
            .collect::<Vec<_>>(),
        ["Prime Vandal Level 1 (1775 VP)", "b"]
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
        ["Prime Vandal Level 1 (1775 VP -> 1200 VP), 10% off"]
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
        ["Penguin Buddy Level 1 (2500 Kingdom Credits)"]
    );
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
    let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
    let update = apply_account_detail_results(
        &mut account,
        Ok(Some(CompetitiveRank::new(
            15,
            "Platinum 1",
            42,
            Some("season".to_string()),
        ))),
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
        levels: vec![crate::riot::content::WeaponSkinLevel {
            uuid: "level-a".to_string(),
            display_name: "Prime Vandal Level 3".to_string(),
            display_icon: None,
        }],
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
fn status_bar_only_shows_error_like_messages() {
    assert!(!status_bar_visible("Loaded 2 account profile(s)"));
    assert!(!status_bar_visible("Loading shop"));
    assert!(!status_bar_visible("Saved settings"));

    assert!(status_bar_visible("Failed to load accounts: disk error"));
    assert!(status_bar_visible(
        "Could not import redirect token: invalid URL"
    ));
    assert!(status_bar_visible(
        "Store loaded, but profile update failed: missing profile"
    ));
    assert!(status_bar_visible(
        "Select an account before opening the shop"
    ));
    assert!(status_bar_visible("display name cannot be empty"));
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
        levels: vec![crate::riot::content::WeaponSkinLevel {
            uuid: "level-a".to_string(),
            display_name: "Prime Vandal Level 4".to_string(),
            display_icon: None,
        }],
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
    let account = AccountProfile::new("Main", None, Shard::Na).expect("account");
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
    let account = AccountProfile::new("Main", None, Shard::Na).expect("account");
    app.state.push_account(account.clone());
    app.launcher_capture_in_progress = true;

    let _ = app.update(Message::LaunchAccount(account.id));

    assert_eq!(app.launching_account, None);
    assert_eq!(app.launch_preflight_account, None);
    assert!(app.status.contains("login capture"), "{}", app.status);
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
fn launch_warns_before_closing_a_running_game() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let account = AccountProfile::new("Main", None, Shard::Na).expect("account");
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
fn launch_starts_when_no_game_is_running() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
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
    let account = AccountProfile::new("Main", None, Shard::Na).expect("account");
    app.state.push_account(account.clone());

    let _ = app.update(Message::RequestLauncherSessionLogin(account.id));

    assert_eq!(app.confirm_recapture_account, Some(account.id));
    assert!(!app.launcher_capture_in_progress);

    let _ = app.update(Message::CancelLauncherSessionLogin);

    assert_eq!(app.confirm_recapture_account, None);
    assert!(!app.launcher_capture_in_progress);
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
fn recapture_as_the_same_account_replaces_the_backup() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let backup_root = app.repo.launcher_backups_dir();
    let account = account_with_backup(&backup_root, "Main", "original");
    app.state.push_account(account.clone());
    let captured = staged_recapture(&backup_root, "Main-puuid", "fresh login");
    let staging_slot = backup_root.join(captured.account_id.to_string());

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
    let mut account = AccountProfile::new(name, None, Shard::Na).expect("account");
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
        Some(super::LauncherCaptureKind::CurrentAccount)
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
    assert_eq!(app.new_username, "Player#NA1");
    assert_eq!(app.new_shard, Shard::Na);
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

#[test]
fn duplicate_current_account_capture_updates_existing_profile_without_confirmation() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.client_version_input = "release-10.10-shipping-1-1234567".to_string();
    let backup_root = app.repo.launcher_backups_dir();
    let mut existing = AccountProfile::new("Main", None, Shard::Na).expect("account");
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
    assert_eq!(
        app.state.accounts[0].username.as_deref(),
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
    assert!(status_bar_visible(&app.status));
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

#[test]
fn cache_account_api_context_updates_matching_account() {
    let mut state = StoredState::default();
    let account = AccountProfile::new("Main", None, Shard::Na).expect("account");
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
    let mut account = AccountProfile::new("Main", None, Shard::Na).expect("account");
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
            },
        }],
    })
}

#[test]
fn selecting_an_account_refreshes_it_without_a_full_reload() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let main = AccountProfile::new("Main", None, Shard::Na).expect("main");
    let alt = AccountProfile::new("Alt", None, Shard::Na).expect("alt");
    app.state.push_account(main);
    app.state.push_account(alt.clone());
    app.client_version_input = "release-1".to_string();

    let _ = app.update(Message::SelectAccount(alt.id));

    assert!(app.account_ranks_loading);
    assert!(app.account_availability_loading);
    assert_eq!(app.status, format!("Selected {}", alt.summary()));
}

#[test]
fn availability_polling_resumes_when_the_window_is_restored() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.state
        .push_account(AccountProfile::new("Main", None, Shard::Na).expect("account"));
    app.client_version_input = "release-1".to_string();

    let _ = app.update(Message::WindowResized(iced::Size::new(0.0, 0.0)));
    assert!(app.window_minimized);
    assert!(!app.account_availability_loading);

    let _ = app.update(Message::WindowResized(iced::Size::new(900.0, 600.0)));
    assert!(!app.window_minimized);
    assert!(app.account_availability_loading);
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
        },
    )
    .expect_err("missing account");

    assert!(err.contains("profile no longer exists"));
}
