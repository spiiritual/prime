use iced::widget::text::Wrapping;
use iced::widget::{Column, Row, column, container, row, space, stack};
use iced::{Color, Element, Length, Padding, alignment};

use crate::ui::components::{
    asset_image, card_style, empty_state, faded_asset_image, high_res_image_source, mono, outlined,
    radial_glow, skeleton, unavailable_state,
};
use crate::ui::data::loadout::{
    BattlePassProgressDisplay, BattlePassRewardDisplay, LoadoutGunDisplay, LoadoutSummary,
    REWARDS_PER_ROW,
};
use crate::ui::data::shop::{RarityTier, format_time_left};
use crate::ui::theme::{self, Icon, button, text};
use crate::ui::{LoadoutTab, Message, PrimeApp};

use super::shop::tier_color;

const LOADOUT_CATEGORIES: [&str; 8] = [
    "Sidearms",
    "SMGs",
    "Shotguns",
    "Rifles",
    "Sniper Rifles",
    "Heavy",
    "Melee",
    "Other",
];
const CATEGORY_LABEL_WIDTH: f32 = 96.0;
const TILE_WIDTH: f32 = 136.0;
const TILE_HEIGHT: f32 = 92.0;
const TILE_IMAGE_HEIGHT: f32 = 40.0;
/// A skin's glow and outline, as fractions of its tier colour's full strength.
const TILE_GLOW_ALPHA: f32 = 0x30 as f32 / 255.0;
const TILE_BORDER_ALPHA: f32 = 0x55 as f32 / 255.0;
/// A default skin's art, set back from the skins that were bought.
const DEFAULT_SKIN_OPACITY: f32 = 0.55;
const PASS_CARD_HEIGHT: f32 = 107.0;
const PASS_GLOW_ALPHA: f32 = 0x26 as f32 / 255.0;
const PASS_BAR_END: Color = iced::color!(0xF5955B);
const REWARD_ART_HEIGHT: f32 = 150.0;
const REWARD_ART_BACKGROUND: Color = iced::color!(0x0F1217);
/// The design's 20% green over the art background, made opaque so bright art can't wash it out.
const EARNED_BADGE: Color = iced::color!(0x1B372A);
const FEATURED_REWARD_BACKGROUND: Color = iced::color!(0x2A2214);
const FEATURED_REWARD_BORDER: Color = iced::color!(0xE8BE55, 0x77 as f32 / 255.0);

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    if fills_page(app) {
        return page_state(app);
    }

    let loaded = app
        .loadout_summary
        .as_ref()
        .and_then(|summary| match app.active_loadout_tab {
            LoadoutTab::Skins => summary.loadout_error.is_none().then(|| skins(summary)),
            LoadoutTab::BattlePass => summary
                .battle_pass
                .as_ref()
                .map(|battle_pass| battle_pass_page(battle_pass, app.now)),
        });
    if let Some(page) = loaded {
        return page;
    }

    if app.loadout_request.is_some() {
        return match app.active_loadout_tab {
            LoadoutTab::Skins => skins_loading(),
            LoadoutTab::BattlePass => battle_pass_loading(),
        };
    }

    super::account_view_waiting(app, "loadout").unwrap_or_else(|| space().into())
}

/// Whether the open sub-tab shows a centred state instead of content: its part failed to load, or
/// the account has no battle pass progress. The loadout and battle pass load together, so a load
/// that failed outright fails both.
pub(super) fn fills_page(app: &PrimeApp) -> bool {
    if app.loadout_request.is_some() {
        return false;
    }
    if app.loadout_error.is_some() {
        return true;
    }

    app.loadout_summary
        .as_ref()
        .is_some_and(|summary| match app.active_loadout_tab {
            LoadoutTab::Skins => summary.loadout_error.is_some(),
            LoadoutTab::BattlePass => summary.battle_pass.is_none(),
        })
}

/// Skins and Battle Pass, beside the page title.
pub(in crate::ui) fn sub_tabs(app: &PrimeApp) -> Element<'_, Message> {
    let tab_button = |tab: LoadoutTab| {
        let selected = app.active_loadout_tab == tab;
        button(text(tab.to_string()).size(13).font(if selected {
            theme::SEMIBOLD_FONT
        } else {
            theme::MEDIUM_FONT
        }))
        .padding([7, 16])
        .style(move |theme, status| {
            let mut style = theme::choice_style(theme, status, selected);
            style.border.radius = 7.0.into();
            style
        })
        .on_press_maybe((!selected).then_some(Message::LoadoutTabSelected(tab)))
    };

    container(
        row![
            tab_button(LoadoutTab::Skins),
            tab_button(LoadoutTab::BattlePass)
        ]
        .spacing(2),
    )
    .padding(3)
    .style(|_| card_style(9.0))
    .into()
}

fn page_state(app: &PrimeApp) -> Element<'_, Message> {
    let summary = app.loadout_summary.as_ref();
    let try_again = || {
        vec![
            button(theme::icon_label(Icon::RefreshCw, "Try again", theme::TEXT))
                .padding([9, 16])
                .on_press(Message::RetryLoadout)
                .into(),
        ]
    };

    match app.active_loadout_tab {
        LoadoutTab::Skins => {
            let error = app
                .loadout_error
                .as_deref()
                .or_else(|| summary.and_then(|summary| summary.loadout_error.as_deref()))
                .unwrap_or_default();
            unavailable_state(
                Icon::Shirt,
                "Loadout unavailable",
                format!(
                    "Riot's player loadout didn't load ({error}). Your shop and account data are \
                     unaffected."
                ),
                try_again(),
                None,
            )
        }
        LoadoutTab::BattlePass => {
            let error = app
                .loadout_error
                .as_deref()
                .or_else(|| summary.and_then(|summary| summary.battle_pass_error.as_deref()));
            match error {
                Some(error) => unavailable_state(
                    Icon::Ticket,
                    "Battle pass unavailable",
                    format!(
                        "Riot's battle pass progress didn't load ({error}). Your shop and account \
                         data are unaffected."
                    ),
                    try_again(),
                    None,
                ),
                None => empty_state(
                    Icon::Ticket,
                    "No battle pass progress yet",
                    "This account hasn't played a match this act, so Riot has no contract \
                     progress to report. Play a match and it'll show up here.",
                ),
            }
        }
    }
}

/// Each category's equipped skins, in the in-game collection order.
fn skins(summary: &LoadoutSummary) -> Element<'_, Message> {
    Column::with_children(LOADOUT_CATEGORIES.into_iter().filter_map(|category| {
        let guns: Vec<_> = summary
            .gun_skins
            .iter()
            .filter(|gun| gun.weapon.category == category)
            .collect();
        (!guns.is_empty()).then(|| {
            let count = match guns.len() {
                1 => "1 weapon".to_string(),
                count => format!("{count} weapons"),
            };
            category_row(
                column![
                    text(category).size(13).font(theme::SEMIBOLD_FONT),
                    text(count).size(11).color(theme::FAINT),
                ]
                .spacing(2)
                .into(),
                guns.into_iter().map(skin_tile).collect(),
            )
        })
    }))
    .spacing(12)
    .into()
}

/// A category's name beside its tiles, which wrap when the page is narrow.
fn category_row<'a>(
    label: Element<'a, Message>,
    tiles: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![
        container(label).width(CATEGORY_LABEL_WIDTH),
        Row::with_children(tiles)
            .spacing(8)
            .width(Length::Fill)
            .wrap()
            .vertical_spacing(8),
    ]
    .spacing(14)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// A skin over a glow of its tier colour, or a plain, dimmed tile for a default skin.
fn skin_tile(gun: &LoadoutGunDisplay) -> Element<'_, Message> {
    let tier = gun.skin.rarity.as_deref().and_then(RarityTier::from_name);
    let art = high_res_image_source(
        "viewer-skins",
        &gun.skin.uuid,
        gun.skin.display_icon.as_deref(),
        gun.skin.viewer_icon.as_deref(),
    );
    let details = column![
        faded_asset_image(
            gun.skin.cached_icon.as_ref(),
            TILE_IMAGE_HEIGHT,
            gun.skin_detail_label(),
            art,
            if gun.default_skin {
                DEFAULT_SKIN_OPACITY
            } else {
                1.0
            },
        ),
        one_line(
            text(skin_name(gun))
                .size(11)
                .font(theme::SEMIBOLD_FONT)
                .color(if gun.default_skin {
                    theme::MUTED
                } else {
                    theme::TEXT
                }),
        ),
        one_line(
            text(gun.weapon.display_name.to_uppercase())
                .size(9)
                .font(theme::SEMIBOLD_FONT)
                .color(theme::FAINT),
        ),
    ]
    .spacing(4)
    // The tile's 1px border is outside this.
    .padding([9, 11]);

    let (content, border): (Element<_>, _) = match tier.map(tier_color) {
        Some(color) => (
            stack![
                radial_glow(color, TILE_GLOW_ALPHA, (0.5, 0.35), (1.3, 1.2), [9.0; 4]),
                details,
            ]
            .into(),
            Color {
                a: TILE_BORDER_ALPHA,
                ..color
            },
        ),
        None => (details.into(), theme::LINE),
    };

    container(content)
        // The glow is opaque, so it sits inside the border rather than over it.
        .padding(1)
        .width(TILE_WIDTH)
        .height(TILE_HEIGHT)
        .clip(true)
        .style(move |_| {
            container::Style::default()
                .background(theme::SURFACE)
                .border(iced::Border {
                    color: border,
                    width: 1.0,
                    radius: 10.0.into(),
                })
        })
        .into()
}

/// The skin's name without its weapon's, which the tile shows below it: "Prime", not "Prime
/// Classic". Default skins read "Standard".
fn skin_name(gun: &LoadoutGunDisplay) -> &str {
    gun.skin_name
        .strip_suffix(gun.weapon.display_name.as_str())
        .map(str::trim_end)
        .filter(|name| !name.is_empty())
        .unwrap_or(&gun.skin_name)
}

fn battle_pass_page(
    battle_pass: &BattlePassProgressDisplay,
    now: iced::time::Instant,
) -> Element<'_, Message> {
    let earned = &battle_pass.earned_rewards;
    let earned_note = match earned.len() {
        1 => "1 reward".to_string(),
        count if count > REWARDS_PER_ROW => format!("{count} rewards · showing latest"),
        count => format!("{count} rewards"),
    };
    let latest_earned = &earned[earned.len().saturating_sub(REWARDS_PER_ROW)..];
    let up_next = &battle_pass.unearned_rewards;
    let locked = &battle_pass.locked_paid_rewards;

    let mut page = column![
        pass_card(battle_pass, now),
        reward_section(
            "Earned",
            earned_note,
            latest_earned,
            true,
            "Nothing earned yet"
        ),
        reward_section(
            "Up next",
            format!("{} remaining", up_next.len()),
            first_row(up_next),
            false,
            "Every reward is earned",
        ),
    ]
    .spacing(26)
    .width(Length::Fill);

    if !locked.is_empty() {
        page = page.push(reward_section(
            "Premium pass",
            format!("{} locked", locked.len()),
            first_row(locked),
            false,
            "",
        ));
    }

    let shown = || {
        latest_earned
            .iter()
            .chain(first_row(up_next))
            .chain(first_row(locked))
    };
    if shown().any(|reward| reward.highlighted) {
        page = page.push(
            row![
                container(space())
                    .width(10)
                    .height(10)
                    .style(|_| featured_reward_style(3.0)),
                text("Gold-outlined rewards are Riot's featured reward for that tier.")
                    .size(12)
                    .color(theme::FAINT),
            ]
            .spacing(8)
            .align_y(alignment::Vertical::Center),
        );
    }

    page.into()
}

fn first_row(rewards: &[BattlePassRewardDisplay]) -> &[BattlePassRewardDisplay] {
    &rewards[..rewards.len().min(REWARDS_PER_ROW)]
}

/// The pass's name and tier, its numbers on the right, and a bar for the whole pass.
fn pass_card(
    battle_pass: &BattlePassProgressDisplay,
    now: iced::time::Instant,
) -> Element<'_, Message> {
    let metric = |label: &'static str, value: String| {
        column![
            text(label)
                .size(10)
                .font(theme::SEMIBOLD_FONT)
                .color(theme::FAINT),
            mono(value, 13).font(theme::MONO_SEMIBOLD_FONT),
        ]
        .spacing(3)
    };
    let time_left = battle_pass
        .remaining_seconds_at(now)
        .map_or_else(|| "—".to_string(), format_time_left);

    let top = row![
        column![
            text(battle_pass.title().to_uppercase())
                .size(10)
                .font(theme::BOLD_FONT)
                .color(theme::GOLD),
            text(battle_pass.tier_label())
                .size(24)
                .font(theme::DISPLAY_FONT)
                .line_height(theme::DISPLAY_LINE_HEIGHT),
        ]
        .spacing(2)
        .width(Length::Fill),
        row![
            metric("NEXT TIER", battle_pass.next_tier_label()),
            metric("TIME LEFT", time_left),
            metric("PASS", battle_pass.pass_label().to_string()),
        ]
        .spacing(28),
    ]
    .spacing(20)
    .align_y(alignment::Vertical::Bottom);

    // Thousandths of the bar, so the filled part and the rest share the width.
    let filled = (battle_pass.progress_fraction() * 1000.0).round() as u16;
    let mut bar = Row::new().height(8);
    if filled > 0 {
        bar = bar.push(
            container(space())
                .width(Length::FillPortion(filled))
                .height(8)
                .style(|_| {
                    container::Style::default()
                        .background(
                            iced::gradient::Linear::new(std::f32::consts::FRAC_PI_2)
                                .add_stop(0.0, theme::GOLD)
                                .add_stop(1.0, PASS_BAR_END),
                        )
                        .border(iced::border::rounded(4))
                }),
        );
    }
    if filled < 1000 {
        bar = bar.push(space().width(Length::FillPortion(1000 - filled)));
    }
    let bar = container(bar).width(Length::Fill).style(|_| {
        container::Style::default()
            .background(theme::RAISED)
            .border(iced::border::rounded(4))
    });

    container(
        stack![
            radial_glow(
                theme::GOLD,
                PASS_GLOW_ALPHA,
                (0.9, 0.0),
                (1.2, 2.0),
                [11.0; 4]
            ),
            // The card's 1px border is outside this.
            column![top, bar].spacing(14).padding(19),
        ]
        .width(Length::Fill)
        .height(PASS_CARD_HEIGHT - 2.0),
    )
    // The glow is opaque, so it sits inside the border rather than over it.
    .padding(1)
    .width(Length::Fill)
    .style(|_| outlined(12.0))
    .into()
}

/// A heading with a count beside it, over a row of reward cards.
fn reward_section<'a>(
    title: &'a str,
    note: String,
    rewards: &'a [BattlePassRewardDisplay],
    earned: bool,
    empty: &'a str,
) -> Element<'a, Message> {
    let body: Element<_> = if rewards.is_empty() {
        text(empty).size(13).color(theme::MUTED).into()
    } else {
        // Empty slots keep a short row's cards the same width as a full row's.
        Row::with_children(
            rewards
                .iter()
                .map(|reward| reward_card(reward, earned))
                .chain(
                    (rewards.len()..REWARDS_PER_ROW).map(|_| space().width(Length::Fill).into()),
                ),
        )
        .spacing(10)
        .into()
    };

    column![
        row![
            text(title).size(15).font(theme::SEMIBOLD_FONT),
            text(note).size(12).color(theme::FAINT),
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center),
        body,
    ]
    .spacing(10)
    .into()
}

/// A reward's art, with a check once earned, then its name and tier. Riot's featured reward for a
/// tier is outlined in gold.
fn reward_card(reward: &BattlePassRewardDisplay, earned: bool) -> Element<'_, Message> {
    let featured = reward.highlighted;
    // Titles have no picture, so their text stands in for one.
    let picture = if reward.cached_icon.is_none() && reward.kind == "Title" {
        container(
            text(reward.name.to_uppercase())
                .size(13)
                .font(theme::DISPLAY_FONT)
                .line_height(1.1)
                .color(theme::GOLD)
                .align_x(alignment::Horizontal::Center),
        )
        .center(Length::Fill)
        .into()
    } else {
        asset_image(
            reward.cached_icon.as_ref(),
            REWARD_ART_HEIGHT - 21.0,
            &reward.name,
            high_res_image_source(
                "viewer-battle-pass",
                &reward.uuid,
                reward.display_icon.as_deref(),
                reward.viewer_icon.as_deref(),
            ),
        )
    };
    let mut art = stack![
        container(picture)
            .padding(Padding {
                top: 9.0,
                right: 9.0,
                bottom: 10.0,
                left: 9.0,
            })
            .width(Length::Fill)
            .height(REWARD_ART_HEIGHT - 1.0)
            .style(|_| {
                container::Style::default()
                    .background(REWARD_ART_BACKGROUND)
                    .border(iced::border::rounded(iced::border::radius(9).bottom(0)))
            }),
    ];
    if earned {
        art = art.push(
            container(
                container(theme::icon(Icon::Check, 11.0, theme::OK))
                    .center_x(20)
                    .center_y(20)
                    .style(|_| {
                        container::Style::default()
                            .background(EARNED_BADGE)
                            .border(iced::border::rounded(10))
                    }),
            )
            .padding(7),
        );
    }
    let mut tier = reward.location_label();
    if let Some(amount) = reward.amount_label() {
        tier = format!("{tier} · {amount}");
    }

    container(column![
        art,
        column![
            one_line(text(&reward.name).size(11).font(theme::SEMIBOLD_FONT)),
            mono(tier, 10).color(if featured { theme::GOLD } else { theme::FAINT }),
        ]
        .spacing(2)
        .padding(Padding {
            top: 7.0,
            right: 9.0,
            bottom: 7.0,
            left: 9.0,
        }),
    ])
    // The art is opaque, so it sits inside the border rather than over it.
    .padding(1)
    .width(Length::Fill)
    .clip(true)
    .style(move |_| {
        if featured {
            featured_reward_style(10.0)
        } else {
            card_style(10.0)
        }
    })
    .into()
}

fn featured_reward_style(radius: f32) -> container::Style {
    container::Style::default()
        .background(FEATURED_REWARD_BACKGROUND)
        .border(iced::Border {
            color: FEATURED_REWARD_BORDER,
            width: 1.0,
            radius: radius.into(),
        })
}

/// The skins page's shape in placeholders, fading down the page, while it loads.
fn skins_loading<'a>() -> Element<'a, Message> {
    Column::with_children(
        [6, 2, 2, 4, 3, 2, 1]
            .into_iter()
            .enumerate()
            .map(|(index, count)| {
                let opacity = 1.0 - index as f32 * 0.1;
                category_row(
                    column![skeleton(70, 12.0, 4.0, 1.0), skeleton(48, 10.0, 4.0, 1.0)]
                        .spacing(6)
                        .into(),
                    (0..count)
                        .map(|_| skeleton(TILE_WIDTH, 86.0, 10.0, opacity))
                        .collect(),
                )
            }),
    )
    .spacing(12)
    .into()
}

/// The battle pass's shape in placeholders while it loads.
fn battle_pass_loading<'a>() -> Element<'a, Message> {
    let section = |opacity: f32| {
        column![
            skeleton(120, 14.0, 4.0, 1.0),
            Row::with_children((0..REWARDS_PER_ROW).map(|_| skeleton(
                Length::Fill,
                140.0,
                10.0,
                opacity
            )))
            .spacing(10),
        ]
        .spacing(10)
    };

    column![
        skeleton(Length::Fill, 128.0, 12.0, 1.0),
        section(1.0),
        section(0.7)
    ]
    .spacing(26)
    .into()
}

/// A line of text that is clipped at its box instead of wrapping.
fn one_line(content: iced::widget::Text<'_>) -> Element<'_, Message> {
    container(content.wrapping(Wrapping::None))
        .width(Length::Fill)
        .clip(true)
        .into()
}
