use super::*;

fn loadout_result_with_card(account_id: AccountId, puuid: &str, card: &str) -> LoadoutResult {
    LoadoutResult {
        account_id,
        summary: LoadoutSummary {
            player_card_id: Some(card.to_string()),
            ..loaded_loadout()
        },
        session: AuthSession::new("access", None, None, "Bearer", Some(3600), 100),
        launcher_session: None,
        identity: ApiIdentity {
            puuid: puuid.to_string(),
            game_name: None,
            tag_line: None,
            shard: Shard::Na,
            region: None,
        },
    }
}

fn player_card_of(app: &PrimeApp, account_id: AccountId) -> Option<&str> {
    app.state
        .accounts
        .iter()
        .find(|account| account.id == account_id)
        .and_then(|account| account.player_card_id.as_deref())
}

#[test]
fn a_loaded_loadout_saves_the_player_card_of_its_own_account_only() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, alt) = two_account_app(dir.path());
    app.image_cache = crate::image_cache::ImageCache::new(dir.path().join("cache"));
    app.state.accounts[0].puuid = Some("puuid-main".to_string());
    app.state.accounts[1].puuid = Some("puuid-alt".to_string());
    app.loadout_request = Some(crate::ui::ViewRequest {
        id: 1,
        account_id: main.id,
    });

    let task = app.update(Message::LoadoutLoaded(
        1,
        Ok(loadout_result_with_card(main.id, "puuid-main", "card-main")),
    ));

    assert_eq!(player_card_of(&app, main.id), Some("card-main"));
    assert_eq!(player_card_of(&app, alt.id), None);
    // The art is requested once, and saving the profile is too.
    assert!(app.player_card_art.contains_key("card-main"));
    assert!(task.units() >= 2);

    // A late reply whose session turns out to be another Riot account's changes nothing.
    let _ = app.update(Message::LoadoutLoaded(
        2,
        Ok(loadout_result_with_card(alt.id, "puuid-main", "card-main")),
    ));

    assert_eq!(player_card_of(&app, alt.id), None);
}

#[test]
fn an_avatar_uses_its_player_card_art_once_cached_and_initials_otherwise() {
    let dir = tempdir().expect("temp dir");
    let (mut app, main, _) = two_account_app(dir.path());
    let art = dir.path().join("card.png");
    let account = |app: &PrimeApp| app.state.accounts[0].clone();

    assert_eq!(app.player_card_art_path(&account(&app)), None);

    app.state.accounts[0].player_card_id = Some("card-main".to_string());
    assert_eq!(
        app.player_card_art_path(&account(&app)),
        None,
        "not cached yet"
    );

    let _ = app.update(Message::PlayerCardArtLoaded(
        "card-main".to_string(),
        crate::ui::data::image_assets::PlayerCardArt {
            small: Some(art.clone()),
            wide: None,
        },
    ));
    assert_eq!(app.player_card_art_path(&account(&app)), Some(&art));
    assert_eq!(app.state.accounts[0].id, main.id);

    // Art that failed to download leaves the initials.
    let _ = app.update(Message::PlayerCardArtLoaded(
        "card-main".to_string(),
        Default::default(),
    ));
    assert_eq!(app.player_card_art_path(&account(&app)), None);
}

#[test]
fn opening_a_battle_pass_chapter_downloads_its_art_once() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    app.image_cache = crate::image_cache::ImageCache::new(dir.path().join("images"));
    app.loadout_summary = Some(LoadoutSummary {
        battle_pass: Some(battle_pass_with_chapters()),
        ..loaded_loadout()
    });
    let battle_pass = |app: &PrimeApp| {
        app.loadout_summary
            .as_ref()
            .and_then(|summary| summary.battle_pass.clone())
            .expect("battle pass")
    };

    let task = app.update(Message::BattlePassChapterSelected(5));
    assert_eq!(battle_pass(&app).selected_chapter, 5);
    assert!(battle_pass(&app).chapters[5].art_loading);
    assert!(task.units() > 0);

    // Opening it again while its art downloads doesn't download it twice.
    let _ = app.update(Message::BattlePassChapterSelected(2));
    let task = app.update(Message::BattlePassChapterSelected(5));
    assert_eq!(task.units(), 0);

    // A chapter past the end changes nothing.
    let task = app.update(Message::BattlePassChapterSelected(10));
    assert_eq!(battle_pass(&app).selected_chapter, 5);
    assert_eq!(task.units(), 0);

    let mut loaded = battle_pass(&app).chapters[5].rewards.clone();
    for reward in &mut loaded {
        reward.cached_icon = Some(PathBuf::from(format!("{}.png", reward.uuid)));
    }
    // Art for a different chapter's rewards isn't applied.
    let _ = app.update(Message::BattlePassChapterArtLoaded(4, loaded.clone()));
    assert!(
        battle_pass(&app).chapters[4]
            .rewards
            .iter()
            .all(|reward| reward.cached_icon.is_none())
    );

    let _ = app.update(Message::BattlePassChapterArtLoaded(5, loaded));
    let chapter = &battle_pass(&app).chapters[5];
    assert!(!chapter.art_loading);
    assert!(
        chapter
            .rewards
            .iter()
            .all(|reward| reward.cached_icon.is_some())
    );

    // Back to a chapter whose art is already here: no download.
    let _ = app.update(Message::BattlePassChapterSelected(2));
    let task = app.update(Message::BattlePassChapterSelected(5));
    assert_eq!(task.units(), 0);
}

#[test]
fn the_battle_pass_and_a_failed_loadout_sub_tab_fill_the_page() {
    let dir = tempdir().expect("temp dir");
    let (mut app, _, _) = two_account_app(dir.path());
    // No battle pass progress this act: the loadout loaded and the battle pass is simply absent.
    app.loadout_summary = Some(loaded_loadout());

    app.active_loadout_tab = crate::ui::LoadoutTab::Skins;
    assert!(!crate::ui::screens::fills_page(
        &app,
        crate::ui::Tab::Loadout
    ));
    app.active_loadout_tab = crate::ui::LoadoutTab::BattlePass;
    assert!(crate::ui::screens::fills_page(
        &app,
        crate::ui::Tab::Loadout
    ));

    app.loadout_summary = None;
    app.loadout_error = Some("boom".to_string());
    app.active_loadout_tab = crate::ui::LoadoutTab::Skins;
    assert!(crate::ui::screens::fills_page(
        &app,
        crate::ui::Tab::Loadout
    ));

    let _ = app.update(Message::RetryLoadout);

    assert!(!crate::ui::screens::fills_page(
        &app,
        crate::ui::Tab::Loadout
    ));
    // The battle pass is sized to the window even while it loads.
    app.active_loadout_tab = crate::ui::LoadoutTab::BattlePass;
    assert!(crate::ui::screens::fills_page(
        &app,
        crate::ui::Tab::Loadout
    ));
}

#[test]
fn a_loadout_whose_battle_pass_failed_says_so() {
    let dir = tempdir().expect("temp dir");
    let (mut app, account) = accounts_tab_app(dir.path());
    app.loadout_request = Some(crate::ui::ViewRequest {
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

    assert!(
        app.status.text.contains("battle pass failed"),
        "{}",
        app.status.text
    );
    assert!(
        app.status.text.contains("Riot returned 500"),
        "{}",
        app.status.text
    );
    assert!(
        (app.status.kind == StatusKind::Error),
        "{}",
        app.status.text
    );
}
