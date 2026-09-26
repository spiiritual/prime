use iced::widget::{column, container, rich_text, span, stack, text};
use iced::{Color, Element, Length, Theme, alignment};

use crate::ui::components::{
    asset_background_image, asset_image, compact_item_name, high_res_image_source,
    load_error_panel, loading_line,
};
use crate::ui::data::shop::{
    OfferPrice, RarityTier, StoreAccessoryDisplay, StoreBundleDisplay, StoreOfferDisplay,
    StoreSummary, format_duration,
};
use crate::ui::{Message, PrimeApp};

const SHOP_ITEM_NAME_HEIGHT: f32 = 20.0;
const SHOP_BUNDLE_NAME_HEIGHT: f32 = 24.0;

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    let mut content = column![].spacing(12).width(Length::Fill);

    if app.store_request.is_some() {
        content = content.push(loading_line("Loading shop...", app.loading_frame));
    } else if let Some(error) = &app.store_error {
        content = content.push(load_error_panel(
            "Could not load the shop",
            error,
            Some(Message::RetryShop),
        ));
    } else if app.store_summary.is_none()
        && let Some(waiting) = super::account_view_waiting(app, "shop")
    {
        content = content.push(waiting);
    }

    if let Some(summary) = &app.store_summary {
        content = content
            .push(text("Featured bundles"))
            .push(bundle_row(summary, app.now))
            .push(text(format!(
                "Daily offers reset in {}",
                format_duration(summary.daily_remaining_seconds_at(app.now))
            )))
            .push(offer_row(&summary.daily_offers));

        if !summary.night_market_offers.is_empty() {
            content = content
                .push(text(format!(
                    "Night Market expires in {}",
                    format_duration(summary.night_market_remaining_seconds_at(app.now))
                )))
                .push(offer_row(&summary.night_market_offers));
        }

        if summary.accessory_remaining_seconds.is_some() || !summary.accessory_offers.is_empty() {
            content = content
                .push(text(format!(
                    "Accessories reset in {}",
                    format_duration(summary.accessory_remaining_seconds_at(app.now))
                )))
                .push(accessory_row(&summary.accessory_offers));
        }
    }

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn offer_row<'a>(offers: &'a [StoreOfferDisplay]) -> Element<'a, Message> {
    if offers.is_empty() {
        return text("No offers available").into();
    }

    let mut cards = iced::widget::Row::new().spacing(10).width(Length::Fill);

    for offer in offers {
        cards = cards.push(store_offer_card(offer));
    }

    cards.into()
}

fn accessory_row<'a>(offers: &'a [StoreAccessoryDisplay]) -> Element<'a, Message> {
    if offers.is_empty() {
        return text("No accessories available").into();
    }

    let mut cards = iced::widget::Row::new().spacing(10).width(Length::Fill);

    for offer in offers {
        cards = cards.push(store_accessory_card(offer));
    }

    cards.into()
}

fn bundle_row(summary: &StoreSummary, now: iced::time::Instant) -> Element<'_, Message> {
    if summary.featured_bundles.is_empty() {
        return text("No featured bundles available").into();
    }

    let mut cards = iced::widget::Row::new().spacing(12).width(Length::Fill);

    for bundle in &summary.featured_bundles {
        cards = cards.push(store_bundle_card(
            bundle,
            summary.featured_bundle_remaining_seconds_at(bundle, now),
        ));
    }

    cards.into()
}

fn store_bundle_card(bundle: &StoreBundleDisplay, remaining_seconds: i64) -> Element<'_, Message> {
    let rarity_for_style = bundle.rarity.clone();
    let mut meta = bundle.item_count_label();
    if bundle.discount_percent > 0 {
        meta.push_str(&format!(" | {}% off", bundle.discount_percent));
    }
    let details = column![
        compact_item_name(&bundle.bundle.display_name, 20, SHOP_BUNDLE_NAME_HEIGHT),
        price_line(bundle.price.as_ref(), bundle.original_price.as_ref(), 16),
        text(format!(
            "{meta} | Expires in {}",
            format_duration(remaining_seconds)
        ))
        .size(14),
    ]
    .spacing(5)
    .width(Length::Fill);
    let overlay = container(
        container(details)
            .padding([10, 12])
            .width(Length::Fill)
            .style(bundle_text_scrim_style),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_y(alignment::Vertical::Bottom);

    container(
        stack![
            asset_background_image(
                bundle.bundle.cached_icon.as_ref(),
                214.0,
                &bundle.bundle.display_name,
                high_res_image_source(
                    "viewer-bundles",
                    &bundle.bundle.uuid,
                    bundle.bundle.display_icon.as_deref(),
                    bundle.bundle.viewer_icon.as_deref(),
                )
            ),
            overlay
        ]
        .width(Length::Fill)
        .height(214.0)
        .clip(true),
    )
    .width(Length::Fill)
    .height(214.0)
    .clip(true)
    .style(move |theme| rarity_card_style(theme, rarity_for_style.as_deref()))
    .into()
}

fn store_accessory_card(offer: &StoreAccessoryDisplay) -> Element<'_, Message> {
    let price = offer
        .price
        .as_ref()
        .map(OfferPrice::label)
        .unwrap_or_else(|| "Price unavailable".to_string());
    let details = column![
        asset_image(
            offer.accessory.cached_icon.as_ref(),
            96.0,
            &offer.accessory.display_name,
            high_res_image_source(
                "viewer-accessories",
                &offer.accessory.uuid,
                offer.accessory.display_icon.as_deref(),
                offer.accessory.viewer_icon.as_deref(),
            )
        ),
        compact_item_name(&offer.accessory.display_name, 16, SHOP_ITEM_NAME_HEIGHT),
        text(price).size(14),
    ]
    .spacing(6)
    .width(Length::Fill);

    container(details)
        .padding(10)
        .width(Length::Fill)
        .style(iced::widget::container::bordered_box)
        .into()
}

fn store_offer_card(offer: &StoreOfferDisplay) -> Element<'_, Message> {
    let rarity_for_style = offer.skin.rarity.clone();
    let mut details = iced::widget::Column::new()
        .spacing(6)
        .width(Length::Fill)
        .push(asset_image(
            offer.skin.cached_icon.as_ref(),
            118.0,
            &offer.skin.display_name,
            high_res_image_source(
                "viewer-skins",
                &offer.skin.uuid,
                offer.skin.display_icon.as_deref(),
                offer.skin.viewer_icon.as_deref(),
            ),
        ))
        .push(compact_item_name(
            &offer.skin.display_name,
            16,
            SHOP_ITEM_NAME_HEIGHT,
        ))
        .push(price_line(
            offer.price.as_ref(),
            offer.original_price.as_ref(),
            14,
        ));

    if offer.discount_percent > 0 {
        details = details.push(text(format!("{}% off", offer.discount_percent)).size(13));
    }

    container(details)
        .padding(10)
        .width(Length::Fill)
        .style(move |theme| rarity_card_style(theme, rarity_for_style.as_deref()))
        .into()
}

/// The price, after the struck-through original price when it's discounted.
fn price_line(
    price: Option<&OfferPrice>,
    original_price: Option<&OfferPrice>,
    size: u32,
) -> Element<'static, Message> {
    let Some(price) = price else {
        return text("Price unavailable").size(size).into();
    };

    if let Some(original_price) = original_price
        && original_price != price
    {
        return rich_text::<(), Message, Theme, iced::Renderer>([
            span(original_price.label())
                .strikethrough(true)
                .color(Color::from_rgb8(158, 164, 176)),
            span(" "),
            span(price.label()).color(Color::WHITE),
        ])
        .size(size)
        .into();
    }

    text(price.label()).size(size).into()
}

fn rarity_card_style(theme: &Theme, rarity: Option<&str>) -> iced::widget::container::Style {
    let mut style = iced::widget::container::bordered_box(theme);

    if let Some((background, border)) = rarity_colors(rarity) {
        style.background = Some(background.into());
        style.border.color = border;
    }

    style
}

fn bundle_text_scrim_style(_: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(Color::from_rgba8(8, 10, 14, 0.78).into()),
        text_color: Some(Color::WHITE),
        ..Default::default()
    }
}

/// A dark tint of the tier's in-game color for the card, and the color itself for its border.
fn rarity_colors(rarity: Option<&str>) -> Option<(Color, Color)> {
    let [red, green, blue] = RarityTier::from_name(rarity?)?.highlight_rgb();
    let shade = |channel: u8| (f32::from(channel) * 0.36) as u8;

    Some((
        Color::from_rgba8(shade(red), shade(green), shade(blue), 0.72),
        Color::from_rgb8(red, green, blue),
    ))
}
