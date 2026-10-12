use super::*;

#[test]
fn a_bundles_details_open_for_that_bundle_and_close_with_escape_or_a_tab_change() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let loaded_at = iced::time::Instant::now();
    app.store_summary = Some(summary_with_bundles(
        vec![
            featured_bundle_json("first", 3_600),
            featured_bundle_json("second", 7_200),
        ],
        7_200,
        loaded_at,
    ));

    let _ = app.update(Message::ShowBundleDetails("second".to_string()));
    assert_eq!(
        app.open_bundle_details()
            .map(|bundle| bundle.store_id.as_str()),
        Some("second")
    );

    let _ = app.update(Message::EscapePressed);
    assert!(app.open_bundle_details().is_none());

    let _ = app.update(Message::ShowBundleDetails("first".to_string()));
    let _ = app.update(Message::TabSelected(crate::ui::Tab::Loadout));
    assert!(app.open_bundle_details().is_none());

    // A bundle the shop no longer has shows nothing.
    let _ = app.update(Message::ShowBundleDetails("gone".to_string()));
    assert!(app.open_bundle_details().is_none());
}

#[test]
fn a_bundles_item_art_loads_when_its_details_open_and_a_shop_reset_closes_them() {
    let dir = tempdir().expect("temp dir");
    let mut app = test_app(dir.path());
    let loaded_at = iced::time::Instant::now();
    let mut bundle = priced_bundle_json(
        Some(2_175),
        Some(2_175),
        serde_json::json!([bundle_item_json(2_175, 0, 2_175)]),
    );
    bundle["DurationRemainingInSeconds"] = serde_json::json!(60);
    app.store_summary = Some(summary_with_bundles(vec![bundle], 60, loaded_at));

    let task = app.update(Message::ShowBundleDetails("bundle".to_string()));
    assert_eq!(task.units(), 1);
    // Its art shows as grey blocks until it arrives.
    assert_eq!(app.bundle_art_loading.as_deref(), Some("bundle"));

    let mut items = app.open_bundle_details().expect("open").items.clone();
    let art = dir.path().join("art.png");
    if let BundleItem::Skin(skin) = &mut items[0].item {
        skin.cached_icon = Some(art.clone());
    }
    let _ = app.update(Message::BundleItemArtLoaded("bundle".to_string(), items));
    assert_eq!(app.bundle_art_loading, None);
    assert_eq!(
        app.open_bundle_details().expect("open").items[0].cached_icon(),
        Some(&art)
    );
    // Once the art is cached, opening again fetches nothing.
    let _ = app.update(Message::CloseBundleDetails);
    assert_eq!(
        app.update(Message::ShowBundleDetails("bundle".to_string()))
            .units(),
        0
    );

    let _ = app.update(Message::ShopTimerTick(loaded_at + Duration::from_secs(61)));
    assert_eq!(app.bundle_details, None);
}
