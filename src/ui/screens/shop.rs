use iced::widget::{column, container, rich_text, span, stack, text};
use iced::{Color, Element, Length, Theme, alignment};

use super::super::components::{
    asset_background_image, asset_image, compact_item_name, high_res_image_source, loading_line,
};
use super::super::data::{
    OfferPrice, RarityTier, StoreAccessoryDisplay, StoreBundleDisplay, StoreOfferDisplay,
    format_duration,
};
use super::super::{Message, PrimeApp};

const SHOP_ITEM_NAME_HEIGHT: f32 = 20.0;
const SHOP_ITEM_NAME_WIDTH: f32 = 212.0;
const SHOP_BUNDLE_NAME_HEIGHT: f32 = 24.0;
const SHOP_BUNDLE_NAME_WIDTH: f32 = 260.0;

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    let mut content = column![].spacing(12).width(Length::Fill);

    if app.store_loading {
        content = content.push(loading_line("Loading shop...", app.loading_frame));
    }

    if let Some(summary) = &app.store_summary {
        content = content
            .push(text(format!(
                "Featured bundles expire in {}",
                format_duration(summary.bundle_remaining_seconds_at(app.now))
            )))
            .push(bundle_row(&summary.featured_bundles))
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

fn bundle_row<'a>(bundles: &'a [StoreBundleDisplay]) -> Element<'a, Message> {
    if bundles.is_empty() {
        return text("No featured bundles available").into();
    }

    let mut cards = iced::widget::Row::new().spacing(12).width(Length::Fill);

    for bundle in bundles {
        cards = cards.push(store_bundle_card(bundle));
    }

    cards.into()
}

fn store_bundle_card(bundle: &StoreBundleDisplay) -> Element<'_, Message> {
    let price = bundle
        .price
        .as_ref()
        .map(OfferPrice::label)
        .unwrap_or_else(|| "Price unavailable".to_string());
    let rarity_for_style = bundle.rarity.clone();
    let details = column![
        compact_item_name(
            &bundle.bundle.display_name,
            20,
            SHOP_BUNDLE_NAME_HEIGHT,
            SHOP_BUNDLE_NAME_WIDTH
        ),
        text(price).size(16),
        text(bundle.item_count_label()).size(14),
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
        compact_item_name(
            &offer.accessory.display_name,
            16,
            SHOP_ITEM_NAME_HEIGHT,
            SHOP_ITEM_NAME_WIDTH
        ),
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
            SHOP_ITEM_NAME_WIDTH,
        ))
        .push(offer_price_line(offer));

    if offer.discount_percent > 0 {
        details = details.push(text(format!("{}% off", offer.discount_percent)).size(13));
    }

    container(details)
        .padding(10)
        .width(Length::Fill)
        .style(move |theme| rarity_card_style(theme, rarity_for_style.as_deref()))
        .into()
}

fn offer_price_line(offer: &StoreOfferDisplay) -> Element<'_, Message> {
    let Some(price) = &offer.price else {
        return text("Price unavailable").size(14).into();
    };

    if let Some(original_price) = &offer.original_price
        && original_price != price
    {
        return rich_text::<(), Message, Theme, iced::Renderer>([
            span(original_price.label())
                .strikethrough(true)
                .color(Color::from_rgb8(158, 164, 176)),
            span(" "),
            span(price.label()).color(Color::WHITE),
        ])
        .size(14)
        .into();
    }

    text(price.label()).size(14).into()
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

fn rarity_colors(rarity: Option<&str>) -> Option<(Color, Color)> {
    match RarityTier::from_name(rarity?)? {
        RarityTier::Exclusive => Some((
            Color::from_rgba8(86, 42, 42, 0.72),
            Color::from_rgb8(214, 92, 92),
        )),
        RarityTier::Ultra => Some((
            Color::from_rgba8(78, 58, 32, 0.72),
            Color::from_rgb8(218, 154, 72),
        )),
        RarityTier::Premium => Some((
            Color::from_rgba8(58, 48, 82, 0.72),
            Color::from_rgb8(166, 132, 224),
        )),
        RarityTier::Deluxe => Some((
            Color::from_rgba8(34, 55, 82, 0.72),
            Color::from_rgb8(91, 157, 218),
        )),
        RarityTier::Select => Some((
            Color::from_rgba8(32, 68, 55, 0.72),
            Color::from_rgb8(86, 184, 139),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::data::format_duration;

    #[test]
    fn format_duration_includes_ticking_seconds() {
        assert_eq!(format_duration(3_661), "1h 1m 1s");
        assert_eq!(format_duration(61), "1m 1s");
        assert_eq!(format_duration(5), "5s");
    }
}
