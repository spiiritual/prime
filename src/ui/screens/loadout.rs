use iced::widget::text::Wrapping;
use iced::widget::{Column, Row, column, container, row, space, stack};
use iced::{Color, Element, Length, Padding, alignment};

use crate::ui::components::{
    card_style, empty_state, faded_asset_image, high_res_image_source, mono, outlined, radial_glow,
    skeleton, unavailable_state,
};
use crate::ui::data::loadout::{
    BattlePassChapterDisplay, BattlePassProgressDisplay, BattlePassRewardDisplay,
    LoadoutGunDisplay, LoadoutSummary,
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
const PASS_BAR_HEIGHT: f32 = 108.0;
const PASS_GLOW_ALPHA: f32 = 0x26 as f32 / 255.0;
const PASS_BAR_END: Color = iced::color!(0xF5955B);
/// A chapter rail pip for the tier being worked on, and an unreached epilogue tier's.
const CURRENT_PIP_ALPHA: f32 = 0x55 as f32 / 255.0;
const EPILOGUE_PIP: Color = iced::color!(0x1E232C);
const SELECTED_CHAPTER_ALPHA: f32 = 0x14 as f32 / 255.0;
const SELECTED_CHAPTER_BORDER_ALPHA: f32 = 0x66 as f32 / 255.0;
const REWARD_INFO_HEIGHT: f32 = 81.0;
const REWARD_ART_BACKGROUND: Color = iced::color!(0x0F1217);
/// Premium rewards without the premium pass.
const LOCKED_ART_OPACITY: f32 = 0.35;
const AHEAD_LOCKED_OPACITY: f32 = 0.5;
/// The design's 20% green over the art background, made opaque so bright art can't wash it out.
const EARNED_BADGE: Color = iced::color!(0x1B372A);
const FREE_TAG_BACKGROUND: Color = iced::color!(0x4CC974, 0x1F as f32 / 255.0);
const FREE_TAG_BORDER: Color = iced::color!(0x4CC974, 0x55 as f32 / 255.0);
const LOCKED_TAG_BACKGROUND: Color = iced::color!(0x0B0D11, 0xCC as f32 / 255.0);
const SKINS_AHEAD: usize = 3;

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    if shows_state(app) {
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

/// Whether the open sub-tab fills the page instead of scrolling: Battle Pass always does, sized to
/// the window, and Skins does while it shows a state.
pub(super) fn fills_page(app: &PrimeApp) -> bool {
    app.active_loadout_tab == LoadoutTab::BattlePass || shows_state(app)
}

/// Whether the open sub-tab shows a centred state instead of content: its part failed to load, or
/// the account has no battle pass progress. The loadout and battle pass load together, so a load
/// that failed outright fails both.
fn shows_state(app: &PrimeApp) -> bool {
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

/// The pass's progress on top, the open chapter's rewards filling the page, and the weapon skins
/// still ahead below.
fn battle_pass_page(
    battle_pass: &BattlePassProgressDisplay,
    now: iced::time::Instant,
) -> Element<'_, Message> {
    let chapter: Element<_> = match battle_pass.selected_chapter() {
        Some(chapter) => chapter_section(battle_pass, chapter),
        None => container(
            text("Riot's reward list for this pass didn't load.")
                .size(13)
                .color(theme::MUTED),
        )
        .center(Length::Fill)
        .into(),
    };

    let mut page = column![pass_bar(battle_pass, now), chapter]
        .spacing(22)
        .height(Length::Fill);
    let skins: Vec<_> = battle_pass.skins_ahead().take(SKINS_AHEAD).collect();
    if !skins.is_empty() {
        page = page.push(skins_ahead(battle_pass, skins));
    }
    page.into()
}

/// The tier reached, big, beside the pass's name, its numbers and the chapter rail.
fn pass_bar(
    battle_pass: &BattlePassProgressDisplay,
    now: iced::time::Instant,
) -> Element<'_, Message> {
    let label = |content: String, color: Color| {
        text(content)
            .size(10)
            .font(theme::SEMIBOLD_FONT)
            .color(color)
    };
    let (tier_kind, tier, out_of) = battle_pass.tier_display();
    let tier_block = column![
        label(tier_kind.to_string(), theme::FAINT),
        text(tier.to_string())
            .size(40)
            .font(theme::DISPLAY_FONT)
            .line_height(1.0),
    ]
    .push(out_of.map(|total| label(format!("OF {total}"), theme::FAINT)))
    .spacing(4)
    .width(84)
    .align_x(alignment::Horizontal::Center);

    let mut stats = Vec::new();
    if let Some(completion) = battle_pass.completion_label() {
        stats.push(completion.to_uppercase());
    }
    if let Some(seconds) = battle_pass.remaining_seconds_at(now) {
        stats.push(format!("{} LEFT", format_time_left(seconds).to_uppercase()));
    }
    let mut stats_row = Row::new().spacing(8);
    for stat in stats {
        stats_row = stats_row
            .push(label(stat, theme::MUTED))
            .push(label("·".to_string(), theme::FAINT));
    }
    let pass = battle_pass.pass_label().to_uppercase();
    stats_row = stats_row.push(label(
        pass,
        if battle_pass.paid_pass_owned {
            theme::GOLD
        } else {
            theme::MUTED
        },
    ));

    let details = column![
        row![
            text(battle_pass.title().to_uppercase())
                .size(10)
                .font(theme::BOLD_FONT)
                .color(theme::GOLD)
                .width(Length::Fill),
            stats_row,
        ],
        chapter_rail(battle_pass),
    ]
    .spacing(10);

    container(stack![
        radial_glow(
            theme::GOLD,
            PASS_GLOW_ALPHA,
            (0.9, 0.0),
            (1.2, 2.0),
            [11.0; 4]
        ),
        // The bar's 1px border is outside this.
        row![
            tier_block,
            container(space())
                .width(1)
                .height(76)
                .style(|_| container::Style::default().background(theme::LINE)),
            details,
        ]
        .spacing(24)
        .padding([15, 19])
        .height(Length::Fill)
        .align_y(alignment::Vertical::Center),
    ])
    // The glow is opaque, so it sits inside the border rather than over it.
    .padding(1)
    .width(Length::Fill)
    .height(PASS_BAR_HEIGHT)
    .style(|_| outlined(12.0))
    .into()
}

/// A button per chapter, with a pip per tier: filled once reached, faint for the tier being
/// worked on.
fn chapter_rail(battle_pass: &BattlePassProgressDisplay) -> Element<'_, Message> {
    let next_tier = battle_pass.next_tier();
    Row::with_children(
        battle_pass
            .chapters
            .iter()
            .enumerate()
            .map(|(index, chapter)| {
                let selected = index == battle_pass.selected_chapter;
                let started = chapter.first_tier <= battle_pass.level_reached + 1;
                let pips =
                    Row::with_children((chapter.first_tier..=chapter.last_tier).map(|tier| {
                        let fill: iced::Background = if tier <= battle_pass.level_reached {
                            pass_gradient().into()
                        } else if Some(tier) == next_tier {
                            Color {
                                a: CURRENT_PIP_ALPHA,
                                ..theme::GOLD
                            }
                            .into()
                        } else if chapter.is_epilogue {
                            EPILOGUE_PIP.into()
                        } else {
                            theme::MENU_HOVER.into()
                        };
                        container(space())
                            .width(Length::Fill)
                            .height(4)
                            .style(move |_| {
                                container::Style::default()
                                    .background(fill)
                                    .border(iced::border::rounded(2))
                            })
                            .into()
                    }))
                    .spacing(2);

                button(
                    column![
                        mono(chapter.short_name(), 10)
                            .font(theme::MONO_SEMIBOLD_FONT)
                            .color(if selected {
                                theme::GOLD
                            } else if started {
                                theme::MUTED
                            } else {
                                theme::FAINT
                            }),
                        pips,
                    ]
                    .spacing(5),
                )
                .width(Length::Fill)
                .padding([5, 7])
                .style(move |_, status| {
                    let hovered = matches!(status, iced::widget::button::Status::Hovered);
                    let (background, border) = if selected {
                        (
                            Color {
                                a: SELECTED_CHAPTER_ALPHA,
                                ..theme::GOLD
                            },
                            Color {
                                a: SELECTED_CHAPTER_BORDER_ALPHA,
                                ..theme::GOLD
                            },
                        )
                    } else if hovered {
                        (theme::RAISED, Color::TRANSPARENT)
                    } else {
                        (Color::TRANSPARENT, Color::TRANSPARENT)
                    };
                    iced::widget::button::Style {
                        background: Some(background.into()),
                        border: iced::Border {
                            color: border,
                            width: 1.0,
                            radius: 7.0.into(),
                        },
                        ..iced::widget::button::Style::default()
                    }
                })
                .on_press_maybe((!selected).then_some(Message::BattlePassChapterSelected(index)))
                .into()
            }),
    )
    .spacing(6)
    .into()
}

fn pass_gradient() -> iced::gradient::Linear {
    iced::gradient::Linear::new(std::f32::consts::FRAC_PI_2)
        .add_stop(0.0, theme::GOLD)
        .add_stop(1.0, PASS_BAR_END)
}

/// The chapter's name and tiers, buttons to the chapters either side, and its rewards: each
/// premium tier's, then the free ones past a divider.
fn chapter_section<'a>(
    battle_pass: &'a BattlePassProgressDisplay,
    chapter: &'a BattlePassChapterDisplay,
) -> Element<'a, Message> {
    let tiers = battle_pass.chapter_tiers_label(chapter);
    let note = if battle_pass.paid_pass_owned {
        let earned = chapter
            .rewards
            .iter()
            .filter(|reward| battle_pass.is_earned(reward))
            .count();
        format!("{tiers} · {earned} of {} earned", chapter.rewards.len())
    } else {
        let free: Vec<_> = chapter
            .rewards
            .iter()
            .filter(|reward| reward.free)
            .collect();
        match free.first() {
            Some(first) => format!(
                "{tiers} · free {} at {}",
                if free.len() == 1 { "reward" } else { "rewards" },
                battle_pass.reward_tier_label(first).to_lowercase()
            ),
            None => tiers,
        }
    };

    let index = battle_pass.selected_chapter;
    let neighbour = |index: Option<usize>, icon: Icon, before: bool| {
        index
            .and_then(|index| Some((index, battle_pass.chapters.get(index)?)))
            .map(|(index, chapter)| {
                let name = text(chapter.name())
                    .size(12)
                    .font(theme::MEDIUM_FONT)
                    .color(theme::MUTED);
                let icon = theme::icon(icon, 13.0, theme::MUTED);
                let content = if before {
                    row![icon, name]
                } else {
                    row![name, icon]
                };
                button(content.spacing(6).align_y(alignment::Vertical::Center))
                    .padding([6, 10])
                    .style(|theme, status| {
                        let mut style = theme::button_style(theme, status);
                        if matches!(status, iced::widget::button::Status::Active) {
                            style.background = Some(theme::SURFACE.into());
                        }
                        style.border.radius = 7.0.into();
                        style
                    })
                    .on_press(Message::BattlePassChapterSelected(index))
            })
    };

    let head = row![
        row![
            text(chapter.name())
                .size(16)
                .font(theme::DISPLAY_SEMIBOLD_FONT),
            text(note).size(12).color(theme::FAINT),
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center)
        .width(Length::Fill),
        row![]
            .push(neighbour(index.checked_sub(1), Icon::ChevronLeft, true))
            .push(neighbour(index.checked_add(1), Icon::ChevronRight, false))
            .spacing(6),
    ]
    .align_y(alignment::Vertical::Center);

    let (free, premium): (Vec<_>, Vec<_>) = chapter.rewards.iter().partition(|reward| reward.free);
    let mut cards = Row::with_children(
        premium
            .into_iter()
            .map(|reward| reward_card(battle_pass, reward)),
    )
    .spacing(10)
    .height(Length::Fill);
    if !free.is_empty() {
        cards = cards.push(
            container(space())
                .width(1)
                .height(Length::Fill)
                .style(|_| container::Style::default().background(theme::LINE)),
        );
        cards = cards.extend(
            free.into_iter()
                .map(|reward| reward_card(battle_pass, reward)),
        );
    }

    column![head, cards].spacing(12).height(Length::Fill).into()
}

/// A reward's art filling the card, with its name and tier below. Earned rewards carry a check,
/// free ones a FREE tag, and premium ones without the pass a PREMIUM tag over dimmed art. The tier
/// being worked on is outlined and shows the XP towards it.
fn reward_card<'a>(
    battle_pass: &'a BattlePassProgressDisplay,
    reward: &'a BattlePassRewardDisplay,
) -> Element<'a, Message> {
    let earned = battle_pass.is_earned(reward);
    let locked = battle_pass.is_locked(reward);
    let current = !reward.free && battle_pass.next_tier() == Some(reward.tier);

    // Titles have no picture, so their text stands in for one.
    let picture = if reward.cached_icon.is_none() && reward.kind == "Title" {
        container(
            text(reward.name.to_uppercase())
                .size(22)
                .font(theme::DISPLAY_FONT)
                .line_height(1.1)
                .color(theme::GOLD)
                .align_x(alignment::Horizontal::Center),
        )
        .center(Length::Fill)
        .into()
    } else {
        faded_asset_image(
            reward.cached_icon.as_ref(),
            Length::Fill,
            &reward.name,
            high_res_image_source(
                "viewer-battle-pass",
                &reward.uuid,
                reward.display_icon.as_deref(),
                reward.viewer_icon.as_deref(),
            ),
            if locked { LOCKED_ART_OPACITY } else { 1.0 },
        )
    };

    let badge: Option<Element<_>> = if earned {
        Some(
            container(theme::icon(Icon::Check, 11.0, theme::OK))
                .center_x(20)
                .center_y(20)
                .style(|_| {
                    container::Style::default()
                        .background(EARNED_BADGE)
                        .border(iced::border::rounded(10))
                })
                .into(),
        )
    } else if reward.free {
        Some(tag("FREE", theme::OK, FREE_TAG_BACKGROUND, FREE_TAG_BORDER))
    } else if locked {
        Some(tag(
            "PREMIUM",
            theme::MUTED,
            LOCKED_TAG_BACKGROUND,
            theme::LINE,
        ))
    } else {
        None
    };

    let mut art = stack![
        container(picture)
            .padding(14)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| {
                container::Style::default()
                    .background(REWARD_ART_BACKGROUND)
                    .border(iced::border::rounded(iced::border::radius(9).bottom(0)))
            }),
    ];
    if let Some(badge) = badge {
        art = art.push(container(badge).padding(8));
    }

    let mut info = column![
        text(&reward.name)
            .size(12)
            .font(theme::SEMIBOLD_FONT)
            .color(if locked { theme::MUTED } else { theme::TEXT }),
        mono(battle_pass.reward_tier_label(reward), 10).color(theme::FAINT),
    ]
    .spacing(3);
    if current {
        info = info.push(
            column![
                tier_bar(battle_pass.next_tier_fraction()),
                mono(battle_pass.next_tier_label(), 10).font(theme::MONO_SEMIBOLD_FONT),
            ]
            .spacing(5)
            .padding(Padding::default().top(7)),
        );
    }

    container(column![
        art,
        container(info)
            .padding([9, 11])
            .width(Length::Fill)
            .height(REWARD_INFO_HEIGHT)
            .clip(true),
    ])
    // The art is opaque, so it sits inside the border rather than over it.
    .padding(1)
    .width(Length::Fill)
    .height(Length::Fill)
    .clip(true)
    .style(move |_| {
        let style = card_style(10.0);
        if current {
            style.border(iced::Border {
                color: theme::ACCENT,
                width: 1.0,
                radius: 10.0.into(),
            })
        } else {
            style
        }
    })
    .into()
}

/// A small label in a box over a card's art.
fn tag<'a>(label: &'a str, color: Color, background: Color, border: Color) -> Element<'a, Message> {
    container(text(label).size(9).font(theme::BOLD_FONT).color(color))
        .padding([3, 7])
        .style(move |_| {
            container::Style::default()
                .background(background)
                .border(iced::Border {
                    color: border,
                    width: 1.0,
                    radius: 5.0.into(),
                })
        })
        .into()
}

/// How far into the tier being worked on.
fn tier_bar<'a>(fraction: f32) -> Element<'a, Message> {
    // Thousandths of the bar, so the filled part and the rest share the width.
    let filled = (fraction * 1000.0).round() as u16;
    let mut bar = Row::new().height(4);
    if filled > 0 {
        bar = bar.push(
            container(space())
                .width(Length::FillPortion(filled))
                .height(4)
                .style(|_| {
                    container::Style::default()
                        .background(theme::ACCENT)
                        .border(iced::border::rounded(2))
                }),
        );
    }
    if filled < 1000 {
        bar = bar.push(space().width(Length::FillPortion(1000 - filled)));
    }
    container(bar)
        .width(Length::Fill)
        .style(|_| {
            container::Style::default()
                .background(theme::RAISED)
                .border(iced::border::rounded(2))
        })
        .into()
}

/// The next few weapon skins in tiers not reached yet, wherever they are in the pass.
fn skins_ahead<'a>(
    battle_pass: &'a BattlePassProgressDisplay,
    skins: Vec<&'a BattlePassRewardDisplay>,
) -> Element<'a, Message> {
    let heading = if battle_pass.paid_pass_owned {
        "Weapon skins ahead"
    } else {
        "Weapon skins ahead · premium only"
    };
    let opacity = if battle_pass.paid_pass_owned {
        1.0
    } else {
        AHEAD_LOCKED_OPACITY
    };

    let cards = skins.into_iter().map(|skin| {
        let away = match skin.tier - battle_pass.level_reached.max(0) {
            1 => "1 tier away".to_string(),
            tiers => format!("{tiers} tiers away"),
        };
        container(
            row![
                container(faded_asset_image(
                    skin.cached_icon.as_ref(),
                    Length::Fill,
                    &skin.name,
                    high_res_image_source(
                        "viewer-battle-pass",
                        &skin.uuid,
                        skin.display_icon.as_deref(),
                        skin.viewer_icon.as_deref(),
                    ),
                    opacity,
                ))
                .padding(6)
                .width(80)
                .height(56)
                .style(|_| {
                    container::Style::default()
                        .background(REWARD_ART_BACKGROUND)
                        .border(iced::border::rounded(7))
                }),
                column![
                    one_line(text(&skin.name).size(12).font(theme::SEMIBOLD_FONT)),
                    mono(battle_pass.reward_tier_label(skin), 10).color(theme::FAINT),
                    text(away).size(11).color(theme::FAINT),
                ]
                .spacing(3),
            ]
            .spacing(12)
            .align_y(alignment::Vertical::Center),
        )
        .padding(8)
        .width(Length::Fill)
        .style(|_| card_style(10.0))
        .into()
    });

    column![
        row![
            text(heading).size(15).font(theme::SEMIBOLD_FONT),
            text("The pass's skins you haven't reached")
                .size(12)
                .color(theme::FAINT),
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center),
        Row::with_children(
            cards
                .chain(
                    // Empty slots keep a short row's cards the same width as a full row's.
                    std::iter::repeat_with(|| space().width(Length::Fill).into()),
                )
                .take(SKINS_AHEAD)
        )
        .spacing(10),
    ]
    .spacing(10)
    .into()
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
    let cards =
        Row::with_children((0..5).map(|_| skeleton(Length::Fill, Length::Fill, 10.0, 0.85)))
            .push(skeleton(1, Length::Fill, 0.0, 1.0))
            .push(skeleton(Length::Fill, Length::Fill, 10.0, 0.85))
            .spacing(10)
            .height(Length::Fill);

    column![
        skeleton(Length::Fill, PASS_BAR_HEIGHT, 12.0, 1.0),
        column![
            row![
                skeleton(120, 16.0, 4.0, 1.0),
                space().width(Length::Fill),
                skeleton(92, 28.0, 7.0, 1.0),
                skeleton(92, 28.0, 7.0, 1.0),
            ]
            .spacing(6)
            .align_y(alignment::Vertical::Center),
            cards,
        ]
        .spacing(12)
        .height(Length::Fill),
        column![
            skeleton(150, 15.0, 4.0, 0.7),
            Row::with_children((0..SKINS_AHEAD).map(|_| skeleton(Length::Fill, 74.0, 10.0, 0.6)))
                .spacing(10),
        ]
        .spacing(10),
    ]
    .spacing(22)
    .height(Length::Fill)
    .into()
}

/// A line of text that is clipped at its box instead of wrapping.
fn one_line(content: iced::widget::Text<'_>) -> Element<'_, Message> {
    container(content.wrapping(Wrapping::None))
        .width(Length::Fill)
        .clip(true)
        .into()
}
