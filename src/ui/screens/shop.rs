use iced::widget::text::Wrapping;
use iced::widget::{Row, column, container, row, space, stack};
use iced::{Color, Element, Length, Padding, alignment};

use crate::ui::components::{
    asset_background_image, asset_image, card_style, currency_dot, high_res_image_source, mono,
    outlined, radial_glow, skeleton, unavailable_state,
};
use crate::ui::data::shop::{
    AccessoryKind, OfferPrice, RarityTier, StoreAccessoryDisplay, StoreBundleDisplay,
    StoreOfferDisplay, StoreSummary, format_countdown, format_time_left, format_whole_number,
};
use crate::ui::theme::{self, Icon, button, text};
use crate::ui::{Message, PrimeApp};

const BUNDLE_HEIGHT: f32 = 176.0;
/// With two featured bundles the design gives the second this width and the first the rest.
const OFFER_ART_HEIGHT: f32 = 78.0;
const ACCESSORY_THUMB_SIZE: f32 = 72.0;
const NIGHT_MARKET_BADGE: Color = iced::color!(0xC79BFF);
const NIGHT_MARKET_BADGE_TEXT: Color = iced::color!(0x150B22);
/// The shade under a bundle's art that keeps its name readable.
const BUNDLE_SCRIM: Color = theme::BG;
/// An offer's glow, as a fraction of its tier colour's full strength.
const OFFER_GLOW_ALPHA: f32 = 0x30 as f32 / 255.0;
const ACCESSORY_GLOW_ALPHA: f32 = 0x28 as f32 / 255.0;

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    if fills_page(app) {
        return load_error(app);
    }

    if let Some(summary) = &app.store_summary {
        return shop(summary, app.now);
    }

    if app.store_request.is_some() {
        return loading();
    }

    super::account_view_waiting(app, "shop").unwrap_or_else(|| space().into())
}

/// The load error fills the page, centred, so it can't sit in the scrolling area.
pub(super) fn fills_page(app: &PrimeApp) -> bool {
    app.store_error.is_some() && app.store_request.is_none()
}

fn shop(summary: &StoreSummary, now: iced::time::Instant) -> Element<'_, Message> {
    let mut page = column![
        section(
            "Featured bundles",
            // Each bundle shows its own time left.
            None,
            bundle_row(summary, now),
        ),
        section(
            "Daily offers",
            Some(countdown(summary.daily_remaining_seconds_at(now))),
            offer_row(&summary.daily_offers, false),
        ),
    ]
    .spacing(26)
    .width(Length::Fill);

    if !summary.accessory_offers.is_empty() {
        page = page.push(section(
            "Accessory Store",
            summary
                .accessory_remaining_seconds
                .map(|_| countdown(summary.accessory_remaining_seconds_at(now))),
            accessory_row(&summary.accessory_offers),
        ));
    }

    page = page.push(if summary.night_market_offers.is_empty() {
        night_market_closed()
    } else {
        section(
            "Night Market",
            Some(countdown(summary.night_market_remaining_seconds_at(now))),
            offer_row(&summary.night_market_offers, true),
        )
    });

    page.into()
}

/// A section's title, its countdown (or a placeholder for it) on the right, and its cards.
fn section<'a>(
    title: &'a str,
    right: Option<Element<'a, Message>>,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let head = row![
        text(title).size(15).font(theme::SEMIBOLD_FONT),
        space().width(Length::Fill),
    ]
    .push(right)
    .align_y(alignment::Vertical::Center);

    column![head, body].spacing(10).into()
}

fn countdown<'a>(seconds: i64) -> Element<'a, Message> {
    row![
        theme::icon(Icon::Timer, 13.0, theme::FAINT),
        mono(format_countdown(seconds), 12).color(theme::MUTED),
    ]
    .spacing(6)
    .align_y(alignment::Vertical::Center)
    .into()
}

fn bundle_row(summary: &StoreSummary, now: iced::time::Instant) -> Element<'_, Message> {
    if summary.featured_bundles.is_empty() {
        return text("No featured bundles right now")
            .size(13)
            .color(theme::MUTED)
            .into();
    }

    Row::with_children(
        summary
            .featured_bundles
            .iter()
            .map(|bundle| bundle_card(summary, bundle, now)),
    )
    .spacing(12)
    .height(BUNDLE_HEIGHT)
    .into()
}

fn bundle_card<'a>(
    summary: &StoreSummary,
    bundle: &'a StoreBundleDisplay,
    now: iced::time::Instant,
) -> Element<'a, Message> {
    let remaining = summary.featured_bundle_remaining_seconds_at(bundle, now);
    let mut kicker = vec![bundle.item_count_label()];
    if bundle.discount_percent > 0 {
        kicker.push(format!("{}% off", bundle.discount_percent));
    }

    let mut info = row![
        column![
            text(kicker.join(" · ").to_uppercase())
                .size(10)
                .font(theme::BOLD_FONT)
                .color(Color::from_rgba8(255, 255, 255, 0.7)),
            // Clipped, so a long name stops short of the price instead of running under it.
            container(
                text(&bundle.bundle.display_name)
                    .size(24)
                    .font(theme::DISPLAY_FONT)
                    .line_height(theme::DISPLAY_LINE_HEIGHT)
                    .wrapping(Wrapping::None),
            )
            .width(Length::Fill)
            .clip(true),
        ]
        .spacing(2)
        .width(Length::Fill),
    ]
    .spacing(12)
    .align_y(alignment::Vertical::Bottom);

    let pill = |lead: Element<'a, Message>, value: String| {
        container(
            row![lead, mono(value, 13).font(theme::MONO_BOLD_FONT)]
                .spacing(6)
                .align_y(alignment::Vertical::Center),
        )
        .padding([7, 11])
        .style(|_| {
            container::Style::default()
                .background(Color {
                    a: 0.7,
                    ..theme::BG
                })
                .border(iced::border::rounded(8))
        })
    };
    let mut badges = row![pill(
        theme::icon(Icon::Timer, 12.0, theme::TEXT),
        format_time_left(remaining),
    )]
    .spacing(8)
    .align_y(alignment::Vertical::Center);
    if let Some(price) = &bundle.price {
        badges = badges.push(pill(
            currency_dot(&price.currency, 8.0),
            format_whole_number(price.amount),
        ));
    }
    info = info.push(badges);

    let scrim = container(space())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| {
            container::Style::default()
                .background(
                    iced::gradient::Linear::new(std::f32::consts::PI)
                        .add_stop(0.15, Color::TRANSPARENT)
                        .add_stop(
                            0.7,
                            Color {
                                a: 0.8,
                                ..BUNDLE_SCRIM
                            },
                        )
                        .add_stop(
                            1.0,
                            Color {
                                a: 0.97,
                                ..BUNDLE_SCRIM
                            },
                        ),
                )
                .border(iced::border::rounded(11))
        });

    container(stack![
        asset_background_image(
            bundle.bundle.cached_icon.as_ref(),
            BUNDLE_HEIGHT - 2.0,
            11.0,
            &bundle.bundle.display_name,
            high_res_image_source(
                "viewer-bundles",
                &bundle.bundle.uuid,
                bundle.bundle.display_icon.as_deref(),
                bundle.bundle.viewer_icon.as_deref(),
            ),
        ),
        scrim,
        container(info)
            .padding(17)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(alignment::Vertical::Bottom),
    ])
    // The border is drawn under the content, so the art sits inside it to leave it showing.
    .padding(1)
    .width(Length::Fill)
    .height(BUNDLE_HEIGHT)
    .clip(true)
    .style(|_| card_style(12.0).background(iced::color!(0x161A22)))
    .into()
}

fn offer_row(offers: &[StoreOfferDisplay], night_market: bool) -> Element<'_, Message> {
    if offers.is_empty() {
        return text("No offers right now")
            .size(13)
            .color(theme::MUTED)
            .into();
    }

    Row::with_children(offers.iter().map(|offer| offer_card(offer, night_market)))
        .spacing(10)
        .into()
}

/// A skin offer: its art over a glow of its tier colour, a bar in that colour, then its name,
/// tier and price. Night Market offers show their discount and original price instead of the tier.
fn offer_card(offer: &StoreOfferDisplay, night_market: bool) -> Element<'_, Message> {
    let tier = offer.skin.rarity.as_deref().and_then(RarityTier::from_name);
    let tier_color = tier.map_or(theme::LINE, tier_color);

    let mut art = stack![
        radial_glow(
            tier_color,
            OFFER_GLOW_ALPHA,
            (0.5, 0.6),
            (1.3, 1.6),
            [9.0, 9.0, 0.0, 0.0],
        ),
        container(asset_image(
            offer.skin.cached_icon.as_ref(),
            OFFER_ART_HEIGHT - 25.0,
            &offer.skin.display_name,
            high_res_image_source(
                "viewer-skins",
                &offer.skin.uuid,
                offer.skin.display_icon.as_deref(),
                offer.skin.viewer_icon.as_deref(),
            ),
        ))
        .padding([12, 14]),
    ]
    .width(Length::Fill)
    .height(OFFER_ART_HEIGHT - 1.0);

    if night_market && offer.discount_percent > 0 {
        art = art.push(
            container(
                container(
                    mono(format!("-{}%", offer.discount_percent), 10)
                        .font(theme::MONO_BOLD_FONT)
                        .color(NIGHT_MARKET_BADGE_TEXT),
                )
                .padding([2, 6])
                .style(|_| {
                    container::Style::default()
                        .background(NIGHT_MARKET_BADGE)
                        .border(iced::border::rounded(4))
                }),
            )
            .padding(8),
        );
    }

    let mut bottom = row![].align_y(alignment::Vertical::Center);
    if night_market {
        bottom = bottom.push(space().width(Length::Fill));
        if let Some(original) = &offer.original_price {
            bottom = bottom.push(
                container(mono(format_whole_number(original.amount), 11).color(theme::FAINT))
                    .padding(Padding::ZERO.right(5)),
            );
        }
    } else {
        bottom = bottom.push(
            text(tier.map_or("", RarityTier::label))
                .size(11)
                .font(theme::SEMIBOLD_FONT)
                .color(tier_color)
                .width(Length::Fill),
        );
    }
    bottom = bottom.push(price(offer.price.as_ref()));

    let bar = container(space())
        .width(Length::Fill)
        .height(2)
        .style(move |_| container::Style::default().background(tier_color));

    container(column![
        art,
        bar,
        column![item_name(&offer.skin.display_name), bottom]
            .spacing(4)
            .padding(Padding {
                top: 9.0,
                right: 11.0,
                bottom: 8.0,
                left: 11.0,
            }),
    ])
    // The art is opaque, so it sits inside the border rather than over it.
    .padding(1)
    .width(Length::Fill)
    .clip(true)
    .style(|_| card_style(10.0))
    .into()
}

fn accessory_row(offers: &[StoreAccessoryDisplay]) -> Element<'_, Message> {
    Row::with_children(offers.iter().map(accessory_card))
        .spacing(10)
        .into()
}

/// An accessory: a thumbnail over a gold glow, then its name, what it is and its price. Titles
/// have no picture, so their text stands in for one.
fn accessory_card(offer: &StoreAccessoryDisplay) -> Element<'_, Message> {
    let picture: Element<_> =
        if offer.accessory.cached_icon.is_none() && offer.kind == Some(AccessoryKind::Title) {
            text(offer.accessory.display_name.to_uppercase())
                .size(11)
                .font(theme::DISPLAY_FONT)
                .line_height(1.1)
                .color(theme::GOLD)
                .align_x(alignment::Horizontal::Center)
                .into()
        } else {
            asset_image(
                offer.accessory.cached_icon.as_ref(),
                ACCESSORY_THUMB_SIZE - 16.0,
                &offer.accessory.display_name,
                high_res_image_source(
                    "viewer-accessories",
                    &offer.accessory.uuid,
                    offer.accessory.display_icon.as_deref(),
                    offer.accessory.viewer_icon.as_deref(),
                ),
            )
        };
    let thumb = stack![
        radial_glow(
            theme::GOLD,
            ACCESSORY_GLOW_ALPHA,
            (0.5, 0.5),
            (1.4, 1.4),
            [8.0; 4],
        ),
        container(picture).padding(8).center(Length::Fill),
    ]
    .width(ACCESSORY_THUMB_SIZE)
    .height(ACCESSORY_THUMB_SIZE);

    let mut info = column![item_name(&offer.accessory.display_name)]
        .spacing(4)
        .width(Length::Fill);
    if let Some(kind) = offer.kind {
        info = info.push(text(kind.label()).size(11).color(theme::MUTED));
    }
    info = info.push(container(price(offer.price.as_ref())).padding(Padding::ZERO.top(4)));

    container(
        row![thumb, info]
            .spacing(14)
            .align_y(alignment::Vertical::Center),
    )
    .padding(Padding {
        top: 8.0,
        right: 14.0,
        bottom: 8.0,
        left: 8.0,
    })
    .width(Length::Fill)
    .style(|_| card_style(10.0))
    .into()
}

fn night_market_closed<'a>() -> Element<'a, Message> {
    container(
        row![
            theme::icon(Icon::Moon, 16.0, theme::FAINT),
            text("Night Market isn't running right now. It'll appear here automatically when it opens.")
                .size(13)
                .color(theme::MUTED),
        ]
        .spacing(12)
        .align_y(alignment::Vertical::Center),
    )
    .padding([14, 16])
    .width(Length::Fill)
    .style(|_| outlined(10.0))
    .into()
}

/// The shop's shape in placeholders, fading along each row, while it loads.
fn loading<'a>() -> Element<'a, Message> {
    let fading_row = |count: usize, height: f32| {
        Row::with_children(
            (0..count).map(|index| skeleton(Length::Fill, height, 10.0, 1.0 - index as f32 * 0.08)),
        )
        .spacing(10)
        .into()
    };
    let placeholder = |title, body| section(title, Some(skeleton(74, 14.0, 4.0, 1.0)), body);

    column![
        section("Featured bundles", None, fading_row(2, BUNDLE_HEIGHT)),
        placeholder("Daily offers", fading_row(4, 133.0)),
        placeholder("Accessory Store", fading_row(4, 88.0)),
        placeholder("Night Market", fading_row(6, 133.0)),
    ]
    .spacing(26)
    .into()
}

fn load_error(app: &PrimeApp) -> Element<'_, Message> {
    let error = app.store_error.as_deref().unwrap_or_default();
    let account = app.state.selected_account();
    let name = account.map_or("this account", |account| account.display_name.as_str());

    let mut actions = vec![
        button(theme::icon_label(Icon::RefreshCw, "Retry", theme::BG))
            .padding([9, 16])
            .style(theme::primary_button_style)
            .on_press(Message::RetryShop)
            .into(),
    ];
    if let Some(account) = account {
        actions.push(
            button(text("Re-capture login").size(13).font(theme::SEMIBOLD_FONT))
                .padding([9, 16])
                .on_press_maybe(
                    (!app.launcher_capture_in_progress && !app.launch_in_progress())
                        .then_some(Message::RequestLauncherSessionLogin(account.id)),
                )
                .into(),
        );
    }

    unavailable_state(
        Icon::PlugZap,
        "Couldn't load the shop",
        format!(
            "Prime couldn't get the shop for {name}. Retry signs in again from the captured \
             login; if Riot keeps refusing it, re-capture the login."
        ),
        actions,
        Some(error),
    )
}

fn item_name(name: &str) -> Element<'_, Message> {
    container(
        text(name)
            .size(12)
            .font(theme::SEMIBOLD_FONT)
            .wrapping(Wrapping::None),
    )
    .width(Length::Fill)
    .clip(true)
    .into()
}

/// An amount with its currency's dot, or a dash when Riot sent no price.
fn price(price: Option<&OfferPrice>) -> Element<'_, Message> {
    match price {
        Some(price) => row![
            currency_dot(&price.currency, 7.0),
            mono(format_whole_number(price.amount), 12).font(theme::MONO_SEMIBOLD_FONT),
        ]
        .spacing(5)
        .align_y(alignment::Vertical::Center)
        .into(),
        None => text("—").size(12).color(theme::FAINT).into(),
    }
}

/// A skin tier's colour in the game, which Shop and Loadout glow and outline skins with.
pub(super) fn tier_color(tier: RarityTier) -> Color {
    let [red, green, blue] = tier.highlight_rgb();
    Color::from_rgb8(red, green, blue)
}
