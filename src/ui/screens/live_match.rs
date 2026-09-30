use std::f32::consts::FRAC_PI_2;

use iced::gradient::Linear;
use iced::widget::image::Handle;
use iced::widget::text::Wrapping;
use iced::widget::{Column, Row, Stack, column, container, image, row, space, stack, tooltip};
use iced::{Color, ContentFit, Element, Length, Padding, Radians, Theme, alignment};

use crate::ui::components::{
    account_avatar, card_style, empty_state, radial_glow, skeleton, unavailable_state,
};
use crate::ui::data::account_details::{AccountAvailability, Busy};
use crate::ui::data::live_match::{
    LiveMatch, LiveMatchError, LivePlayer, MatchPhase, RankState, ShownIdentity, SkinCell,
    agent_line, match_meta, match_title, shown_identity, team_average_rank,
};
use crate::ui::data::shop::RarityTier;
use crate::ui::theme::{self, Icon, button, text};
use crate::ui::{Message, PrimeApp, Tab};

use super::shop::tier_color;

const ROW_HEIGHT: f32 = 54.0;
const IDENTITY_WIDTH: f32 = 302.0;
const RANK_WIDTH: f32 = 140.0;
const SKIN_WIDTH: f32 = 112.0;
const SKIN_HEIGHT: f32 = 40.0;
const MATCH_BAR_HEIGHT: f32 = 65.0;
const WEAPON_HEADERS: [&str; 4] = ["VANDAL", "PHANTOM", "SHERIFF", "OPERATOR"];
/// A skin's glow and outline, as on Loadout's tiles.
const SKIN_GLOW_ALPHA: f32 = 0x30 as f32 / 255.0;
const SKIN_BORDER_ALPHA: f32 = 0x55 as f32 / 255.0;
const SELF_ROW_BORDER: Color = iced::color!(0xFF4F5E, 0x55 as f32 / 255.0);

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    if let Some(waiting) = super::account_view_waiting(app, "live match") {
        return waiting;
    }
    let name = app
        .state
        .selected_account()
        .map(|account| account.display_name.as_str())
        .unwrap_or_default();

    if let Some(LiveMatchError::SignIn(error)) = &app.live_match_error {
        return unavailable_state(
            Icon::LogIn,
            "Can't sign in to this account",
            format!(
                "Prime couldn't sign in to {name} to check its match: {error}. Re-capture its \
                 login on the Accounts tab."
            ),
            vec![
                button(theme::icon_label(
                    Icon::ArrowRight,
                    "Go to Accounts",
                    theme::TEXT,
                ))
                .padding([9, 16])
                .on_press(Message::TabSelected(Tab::Accounts))
                .into(),
            ],
            None,
        );
    }
    if let Some(live) = &app.live_match {
        return match_page(app, live);
    }
    if let Some(LiveMatchError::Request(error)) = &app.live_match_error {
        return unavailable_state(
            Icon::Swords,
            "Live match didn't load",
            format!("Riot's match data for {name} didn't load. Prime tries again every minute."),
            vec![
                button(theme::icon_label(Icon::RefreshCw, "Try again", theme::TEXT))
                    .padding([9, 16])
                    .on_press(Message::RetryLiveMatch)
                    .into(),
            ],
            Some(error),
        );
    }
    let availability = app
        .state
        .selected_account()
        .and_then(|account| app.account_availability.get(&account.id));
    match availability {
        _ if app.live_match_request.is_some() => loading(),
        Some(AccountAvailability::Unavailable(Busy::InLobby)) => empty_state(
            Icon::Swords,
            "In the lobby",
            "This account is in the lobby. The match shows here once agent select starts, and \
             this page checks every minute while it's open.",
        ),
        Some(AccountAvailability::Available) => empty_state(
            Icon::Swords,
            "Not in a match",
            "This account isn't in a match or agent select. This page checks every minute while \
             it's open.",
        ),
        // Not checked yet: the first check starts once other sign-ins finish.
        _ => loading(),
    }
}

/// Whether the page shows a centred state instead of a match.
pub(super) fn fills_page(app: &PrimeApp) -> bool {
    app.live_match.is_none() || matches!(app.live_match_error, Some(LiveMatchError::SignIn(_)))
}

/// Whether players who hide their name stay hidden, beside the page title.
pub(super) fn streamer_toggle(app: &PrimeApp) -> Element<'_, Message> {
    let on = app.respect_streamer_mode;
    let knob = container(space()).width(14).height(14).style(move |_| {
        container::Style::default()
            .background(if on { Color::WHITE } else { theme::MUTED })
            .border(iced::border::rounded(7))
    });
    let switch = container(knob)
        .padding(2)
        .width(32)
        .height(18)
        .align_x(if on {
            alignment::Horizontal::Right
        } else {
            alignment::Horizontal::Left
        })
        .style(move |_| {
            container::Style::default()
                .background(if on { theme::ACCENT } else { theme::LINE })
                .border(iced::border::rounded(9))
        });

    button(
        row![
            theme::icon(
                if on { Icon::EyeOff } else { Icon::Eye },
                15.0,
                theme::MUTED
            ),
            text("Respect streamer mode")
                .size(13)
                .font(theme::MEDIUM_FONT)
                .width(Length::Fill),
            switch,
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center),
    )
    .padding(Padding {
        top: 7.0,
        right: 8.0,
        bottom: 7.0,
        left: 12.0,
    })
    .width(236)
    .style(|theme: &Theme, status| {
        let mut style = theme::button_style(theme, status);
        style.border.radius = 9.0.into();
        style
    })
    .on_press(Message::StreamerModeToggled)
    .into()
}

fn match_page<'a>(app: &'a PrimeApp, live: &'a LiveMatch) -> Element<'a, Message> {
    let mut teams = Column::new()
        .push(team(app, &live.allies, true))
        .spacing(16);
    teams = match live.phase {
        MatchPhase::InProgress => teams.push(team(app, &live.enemies, false)),
        MatchPhase::AgentSelect => teams.push(
            container(
                text("The enemy team shows once the match starts.")
                    .size(12)
                    .color(theme::MUTED),
            )
            .padding([14, 18])
            .width(Length::Fill)
            .style(|_| card_style(10.0)),
        ),
    };

    column![match_bar(live), teams].spacing(24).into()
}

fn match_bar(live: &LiveMatch) -> Element<'_, Message> {
    let info = column![
        text(match_title(live))
            .size(16)
            .font(theme::DISPLAY_SEMIBOLD_FONT)
            .line_height(theme::DISPLAY_LINE_HEIGHT)
            .color(theme::TEXT)
            .wrapping(Wrapping::None),
        text(match_meta(live)).size(12).color(theme::MUTED),
    ]
    .spacing(2)
    .width(Length::Fill);

    let score: Option<Element<_>> = match (live.phase, &live.score) {
        (MatchPhase::AgentSelect, _) => None,
        (MatchPhase::InProgress, Ok(score)) => {
            let big = |value: i64, color: Color| {
                text(value.to_string())
                    .size(26)
                    .font(theme::DISPLAY_FONT)
                    .line_height(theme::DISPLAY_LINE_HEIGHT)
                    .color(color)
            };
            let label = |label: &'static str, color: Color| {
                text(label).size(10).font(theme::SEMIBOLD_FONT).color(color)
            };
            Some(
                row![
                    label("YOUR TEAM", theme::OK),
                    big(score.ally, theme::TEXT),
                    text(":")
                        .size(20)
                        .font(theme::DISPLAY_FONT)
                        .color(theme::FAINT),
                    big(score.enemy, theme::MUTED),
                    label("ENEMY", theme::ACCENT),
                ]
                .spacing(14)
                .align_y(alignment::Vertical::Center)
                .into(),
            )
        }
        (MatchPhase::InProgress, Err(reason)) => Some(with_tip(
            text("Score unavailable")
                .size(12)
                .color(theme::FAINT)
                .into(),
            reason.reason(),
        )),
    };

    let content = row![info]
        .push(score)
        .spacing(16)
        .align_y(alignment::Vertical::Center);

    let mut layers = Stack::new().width(Length::Fill).height(MATCH_BAR_HEIGHT);
    if let Some(path) = &live.map_art {
        layers = layers.push(
            image(Handle::from_path(path.clone()))
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(ContentFit::Cover)
                .border_radius(12),
        );
    }
    let shade = |alpha: u8| Color {
        a: alpha as f32 / 255.0,
        ..theme::SURFACE
    };
    let gradient = Linear::new(Radians(FRAC_PI_2))
        .add_stop(0.0, shade(0xFA))
        .add_stop(0.45, shade(0xC0))
        .add_stop(0.75, shade(0xB0))
        .add_stop(1.0, shade(0xE6));
    layers
        .push(
            container(content)
                .padding([14, 18])
                .width(Length::Fill)
                .height(Length::Fill)
                .center_y(Length::Fill)
                // Drawn over the art, with the bar's border, since content draws over a
                // container's border.
                .style(move |_| {
                    container::Style::default()
                        .background(gradient)
                        .border(iced::Border {
                            color: theme::LINE,
                            width: 1.0,
                            radius: 12.0.into(),
                        })
                }),
        )
        .into()
}

fn team<'a>(app: &'a PrimeApp, players: &'a [LivePlayer], allies: bool) -> Element<'a, Message> {
    let (label, swatch) = if allies {
        ("YOUR TEAM", theme::OK)
    } else {
        ("ENEMY TEAM", theme::ACCENT)
    };
    let header_text = |label: &'static str, width: f32| {
        text(label)
            .size(9)
            .font(theme::SEMIBOLD_FONT)
            .color(theme::FAINT)
            .width(width)
    };
    let team_label = row![
        container(space())
            .width(8)
            .height(8)
            .style(move |_| container::Style::default()
                .background(swatch)
                .border(iced::border::rounded(2))),
        text(label).size(11).font(theme::SEMIBOLD_FONT),
    ]
    .push(
        team_average_rank(players)
            .map(|average| text(format!("Avg {average}")).size(11).color(theme::FAINT)),
    )
    .spacing(8)
    .align_y(alignment::Vertical::Center)
    .width(IDENTITY_WIDTH);
    let header = Row::with_children(
        [team_label.into(), header_text("RANK", RANK_WIDTH).into()]
            .into_iter()
            .chain(
                WEAPON_HEADERS
                    .into_iter()
                    .map(|weapon| header_text(weapon, SKIN_WIDTH).into()),
            ),
    )
    .spacing(12)
    .align_y(alignment::Vertical::Center)
    .padding(Padding {
        top: 0.0,
        right: 12.0,
        bottom: 2.0,
        left: 14.0,
    });

    Column::with_children(
        std::iter::once(header.into())
            .chain(players.iter().map(|player| player_row(app, player, swatch))),
    )
    .spacing(4)
    .into()
}

fn player_row<'a>(
    app: &'a PrimeApp,
    player: &'a LivePlayer,
    team_color: Color,
) -> Element<'a, Message> {
    let own = player.is_self
        || app.state.accounts.iter().any(|account| {
            account
                .puuid
                .as_deref()
                .is_some_and(|puuid| puuid.eq_ignore_ascii_case(&player.puuid))
        });
    let respect = app.respect_streamer_mode;
    let identity = shown_identity(player, respect, own);

    let avatar: Element<_> = match identity {
        ShownIdentity::Hidden => container(theme::icon(Icon::EyeOff, 16.0, theme::FAINT))
            .center_x(34)
            .center_y(34)
            .style(|_| {
                container::Style::default()
                    .background(theme::RAISED)
                    .border(iced::border::rounded(8))
            })
            .into(),
        ShownIdentity::Shown { name, .. } => account_avatar(
            name.map_or("", |(name, _)| name),
            player
                .card_id
                .as_ref()
                .and_then(|card| app.player_card_art.get(card)?.small.as_ref()),
            34.0,
            8.0,
        ),
    };

    let mut name_row = Row::new().spacing(4).align_y(alignment::Vertical::Center);
    name_row = match identity {
        ShownIdentity::Hidden => name_row.push(
            text("Player hidden")
                .size(13)
                .font(theme::SEMIBOLD_FONT)
                .color(theme::MUTED),
        ),
        ShownIdentity::Shown {
            name: Some((name, tag)),
            ..
        } => name_row
            .push(
                text(name)
                    .size(13)
                    .font(theme::SEMIBOLD_FONT)
                    .wrapping(Wrapping::None),
            )
            .push(
                text(format!("#{tag}"))
                    .size(13)
                    .color(theme::FAINT)
                    .wrapping(Wrapping::None),
            ),
        ShownIdentity::Shown { name: None, .. } => name_row.push(
            text("Name unavailable")
                .size(13)
                .font(theme::SEMIBOLD_FONT)
                .color(theme::MUTED),
        ),
    };
    if player.is_self {
        name_row = name_row.push(chip(
            text("YOU")
                .size(9)
                .font(theme::BOLD_FONT)
                .color(theme::ACCENT)
                .into(),
            false,
        ));
    }
    if matches!(identity, ShownIdentity::Shown { streamer: true, .. }) {
        name_row = name_row.push(chip(
            row![
                theme::icon(Icon::EyeOff, 10.0, theme::MUTED),
                text("STREAMER")
                    .size(9)
                    .font(theme::SEMIBOLD_FONT)
                    .color(theme::MUTED),
            ]
            .spacing(3)
            .align_y(alignment::Vertical::Center)
            .into(),
            true,
        ));
    }

    let identity = row![
        avatar,
        column![
            name_row,
            text(agent_line(player, respect, own))
                .size(11)
                .color(theme::MUTED)
                .wrapping(Wrapping::None),
        ]
        .spacing(2),
    ]
    .spacing(10)
    .align_y(alignment::Vertical::Center)
    .width(IDENTITY_WIDTH)
    .clip(true);

    let cells = Row::with_children(
        [identity.into(), rank_cell(app, &player.rank)]
            .into_iter()
            .chain(player.skins.iter().map(skin_cell)),
    )
    .spacing(12)
    .align_y(alignment::Vertical::Center);

    let (background, border) = if player.is_self {
        (theme::RAISED, SELF_ROW_BORDER)
    } else {
        (theme::SURFACE, theme::LINE)
    };
    let row = container(cells)
        .padding(Padding {
            top: 7.0,
            right: 12.0,
            bottom: 7.0,
            left: 14.0,
        })
        .width(Length::Fill)
        .height(ROW_HEIGHT)
        .center_y(ROW_HEIGHT)
        .style(move |_| {
            container::Style::default()
                .background(background)
                .border(iced::Border {
                    color: border,
                    width: 1.0,
                    radius: 10.0.into(),
                })
        });

    stack![row, team_bar(team_color)].into()
}

/// The 3px strip at a row's left edge, cut to the row's rounded corners.
fn team_bar<'a>(color: Color) -> Element<'a, Message> {
    let [red, green, blue, _] = color.into_rgba8();
    // The left 3px of a 10px-radius rounded rectangle 54px tall.
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="3" height="54"><path d="M0 10A10 10 0 0 1 3 2.86V51.14A10 10 0 0 1 0 44Z" fill="#{red:02x}{green:02x}{blue:02x}"/></svg>"##
    );
    iced::widget::svg(iced::widget::svg::Handle::from_memory(svg.into_bytes()))
        .width(3)
        .height(ROW_HEIGHT)
        .into()
}

fn chip<'a>(content: Element<'a, Message>, outlined: bool) -> Element<'a, Message> {
    container(content)
        .padding([1, 5])
        .style(move |_| {
            if outlined {
                card_style(4.0).background(theme::RAISED)
            } else {
                container::Style::default()
                    .background(Color {
                        a: 0x1F as f32 / 255.0,
                        ..theme::ACCENT
                    })
                    .border(iced::border::rounded(4))
            }
        })
        .into()
}

fn rank_cell<'a>(app: &'a PrimeApp, rank: &'a RankState) -> Element<'a, Message> {
    let (tier, label, color) = match rank {
        RankState::Ranked(rank) => (Some(rank.tier), rank.rank_name.as_str(), theme::TEXT),
        RankState::Unranked => (Some(0), "Unranked", theme::FAINT),
        RankState::Unavailable(_) => (None, "Rank unavailable", theme::FAINT),
    };
    let icon = tier
        .and_then(|tier| app.rank_icons.get(&tier))
        .map(|path| image(Handle::from_path(path.clone())).width(24).height(24));
    let cell = row![]
        .push(icon)
        .push(
            text(label)
                .size(12)
                .font(theme::SEMIBOLD_FONT)
                .color(color)
                .wrapping(Wrapping::None),
        )
        .spacing(8)
        .align_y(alignment::Vertical::Center)
        .width(RANK_WIDTH)
        .into();
    match rank {
        RankState::Unavailable(error) => with_tip(cell, error.clone()),
        _ => cell,
    }
}

fn skin_cell(cell: &SkinCell) -> Element<'_, Message> {
    let label = |label: &'static str| {
        container(
            text(label)
                .size(10)
                .font(theme::SEMIBOLD_FONT)
                .color(theme::FAINT),
        )
        .center(Length::Fill)
    };
    let (content, border): (Element<_>, _) = match cell {
        SkinCell::Standard => (label("Standard").into(), theme::LINE),
        SkinCell::Unavailable => (label("Unavailable").into(), theme::LINE),
        SkinCell::Skin(skin) => {
            let art: Element<_> = match &skin.cached_icon {
                Some(path) => container(
                    image(Handle::from_path(path.clone()))
                        .width(96)
                        .height(32)
                        .content_fit(ContentFit::Contain),
                )
                .center(Length::Fill)
                .into(),
                None => container(
                    text(skin.display_name.as_str())
                        .size(10)
                        .color(theme::MUTED)
                        .wrapping(Wrapping::None),
                )
                .padding([0, 8])
                .center(Length::Fill)
                .clip(true)
                .into(),
            };
            match skin
                .rarity
                .as_deref()
                .and_then(RarityTier::from_name)
                .map(tier_color)
            {
                Some(color) => (
                    stack![
                        radial_glow(color, SKIN_GLOW_ALPHA, (0.5, 0.35), (1.3, 1.2), [6.0; 4]),
                        art
                    ]
                    .into(),
                    Color {
                        a: SKIN_BORDER_ALPHA,
                        ..color
                    },
                ),
                None => (art, theme::LINE),
            }
        }
    };
    let name = match cell {
        SkinCell::Skin(skin) => Some(skin.display_name.clone()),
        _ => None,
    };
    let tile = container(content)
        // The glow is opaque, so it sits inside the border rather than over it.
        .padding(1)
        .width(SKIN_WIDTH)
        .height(SKIN_HEIGHT)
        .clip(true)
        .style(move |_| {
            container::Style::default().border(iced::Border {
                color: border,
                width: 1.0,
                radius: 7.0.into(),
            })
        })
        .into();
    match name {
        Some(name) => with_tip(tile, name),
        None => tile,
    }
}

fn with_tip(content: Element<'_, Message>, tip: String) -> Element<'_, Message> {
    let tip = container(text(tip).size(12))
        .padding([6, 8])
        .max_width(420)
        .style(|_| card_style(8.0).background(theme::RAISED));
    tooltip(content, tip, tooltip::Position::Top).into()
}

/// The match's shape in placeholders while the first check runs.
fn loading<'a>() -> Element<'a, Message> {
    Column::with_children(
        std::iter::once(skeleton(Length::Fill, MATCH_BAR_HEIGHT, 12.0, 1.0)).chain(
            (0..10)
                .map(|index| skeleton(Length::Fill, ROW_HEIGHT, 10.0, 1.0 - index as f32 * 0.07)),
        ),
    )
    .spacing(8)
    .into()
}
