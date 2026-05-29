use iced::widget::{button, column, container, grid, progress_bar, row, text};
use iced::{Color, Element, Length, Theme, border};

use super::super::components::{asset_image, loading_line};
use super::super::data::{
    BattlePassProgressDisplay, BattlePassRewardDisplay, LoadoutGunDisplay, format_duration,
    weapon_category,
};
use super::super::{LoadoutTab, Message, PrimeApp};

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
const LOADOUT_CARD_WIDTH: u32 = 220;
const LOADOUT_CARD_HEIGHT: u32 = 264;
const LOADOUT_IMAGE_HEIGHT: f32 = 148.0;
const BATTLE_PASS_REWARD_CARD_WIDTH: u32 = 174;
const BATTLE_PASS_REWARD_CARD_HEIGHT: u32 = 214;
const BATTLE_PASS_REWARD_IMAGE_HEIGHT: f32 = 92.0;

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    container(
        column![loadout_tabs(app), active_loadout_tab(app)]
            .spacing(14)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn loadout_tabs(app: &PrimeApp) -> Element<'_, Message> {
    row![
        loadout_tab_button(app, LoadoutTab::Skins),
        loadout_tab_button(app, LoadoutTab::BattlePass),
    ]
    .spacing(8)
    .width(Length::Fill)
    .into()
}

fn loadout_tab_button(app: &PrimeApp, tab: LoadoutTab) -> Element<'_, Message> {
    let is_selected = app.active_loadout_tab == tab;
    let label = if is_selected {
        format!("[{}]", tab)
    } else {
        tab.to_string()
    };

    button(text(label).size(14))
        .padding([12, 16])
        .width(Length::Fill)
        .height(46)
        .style(move |theme, status| loadout_tab_button_style(theme, status, is_selected))
        .on_press_maybe((!is_selected).then_some(Message::LoadoutTabSelected(tab)))
        .into()
}

fn active_loadout_tab(app: &PrimeApp) -> Element<'_, Message> {
    match app.active_loadout_tab {
        LoadoutTab::Skins => skins_tab(app),
        LoadoutTab::BattlePass => battle_pass_tab(app),
    }
}

fn skins_tab(app: &PrimeApp) -> Element<'_, Message> {
    let mut content = column![].spacing(12).width(Length::Fill);

    if app.loadout_loading {
        content = content.push(loading_line("Loading loadout...", app.loading_frame));
    }

    if let Some(summary) = &app.loadout_summary {
        for category in LOADOUT_CATEGORIES {
            if let Some(section) = loadout_section(
                category,
                summary
                    .gun_skins
                    .iter()
                    .filter(|gun| weapon_category(&gun.weapon.display_name) == category),
            ) {
                content = content.push(section);
            }
        }
    }

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn battle_pass_tab(app: &PrimeApp) -> Element<'_, Message> {
    let mut content = column![].spacing(12).width(Length::Fill);

    if app.loadout_loading {
        content = content.push(loading_line("Loading battle pass...", app.loading_frame));
    }

    if let Some(summary) = &app.loadout_summary {
        if let Some(battle_pass) = &summary.battle_pass {
            content = content.push(battle_pass_panel(battle_pass, app.now));
        } else if let Some(error) = &summary.battle_pass_error {
            content = content.push(text(format!("Battle pass progress unavailable: {error}")));
        } else if !app.loadout_loading {
            content = content.push(text("No battle pass progress loaded"));
        }
    }

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn loadout_tab_button_style(
    theme: &Theme,
    status: iced::widget::button::Status,
    is_selected: bool,
) -> iced::widget::button::Style {
    if !is_selected {
        let mut style = iced::widget::button::primary(theme, status);
        style.border.radius = border::radius(4);
        style.border.width = 1.0;
        return style;
    }

    let mut style = iced::widget::button::secondary(theme, iced::widget::button::Status::Disabled);
    style.background = Some(Color::from_rgb8(68, 72, 78).into());
    style.text_color = Color::from_rgb8(180, 184, 190);
    style.border.radius = border::radius(4);
    style.border.width = 1.0;
    style.border.color = Color::from_rgb8(96, 102, 112);
    style
}

fn loadout_section<'a>(
    category: &'static str,
    guns: impl IntoIterator<Item = &'a LoadoutGunDisplay>,
) -> Option<Element<'a, Message>> {
    let mut cards = grid::Grid::new()
        .spacing(12)
        .fluid(LOADOUT_CARD_WIDTH)
        .height(grid::aspect_ratio(LOADOUT_CARD_WIDTH, LOADOUT_CARD_HEIGHT));
    let mut count = 0;

    for gun in guns {
        cards = cards.push(loadout_card(gun));
        count += 1;
    }

    (count > 0).then(|| column![text(category).size(20), cards].spacing(8).into())
}

fn loadout_card(gun: &LoadoutGunDisplay) -> Element<'_, Message> {
    let skin_label = gun.skin_detail_label();

    container(
        column![
            asset_image(
                gun.skin.cached_icon.as_ref(),
                LOADOUT_IMAGE_HEIGHT,
                skin_label.clone()
            ),
            text(&gun.weapon.display_name).size(15).width(Length::Fill),
            text(skin_label).size(12).width(Length::Fill)
        ]
        .spacing(6),
    )
    .padding(10)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(iced::widget::container::bordered_box)
    .into()
}

fn battle_pass_panel(
    battle_pass: &BattlePassProgressDisplay,
    now: iced::time::Instant,
) -> Element<'_, Message> {
    let remaining = battle_pass
        .remaining_seconds_at(now)
        .map(format_duration)
        .unwrap_or_else(|| "unavailable".to_string());
    let mut details = column![
        text(battle_pass.title()).size(22),
        progress_bar(0.0..=1.0, battle_pass.progress_fraction()),
        row![
            battle_pass_metric("Progress", battle_pass.tier_label()),
            battle_pass_metric("Next tier", battle_pass.next_tier_label()),
            battle_pass_metric("Time left", remaining),
        ]
        .spacing(18)
        .width(Length::Fill),
    ]
    .spacing(12)
    .width(Length::Fill);

    if let Some(total_progress) = battle_pass.total_progress_label() {
        details = details.push(text(total_progress).size(14));
    }

    if let Some(percent) = battle_pass.progress_percent_label() {
        details = details.push(text(percent).size(14));
    }

    details = details
        .push(battle_pass_reward_section(
            "Earned rewards",
            &battle_pass.earned_rewards,
            "No earned rewards yet",
        ))
        .push(battle_pass_reward_section(
            "Unearned rewards",
            &battle_pass.unearned_rewards,
            "No unearned rewards",
        ));

    if !battle_pass.locked_paid_rewards.is_empty() {
        details = details.push(battle_pass_reward_section(
            "Locked paid pass rewards",
            &battle_pass.locked_paid_rewards,
            "No locked paid rewards",
        ));
    }

    container(details)
        .padding(14)
        .width(Length::Fill)
        .style(iced::widget::container::bordered_box)
        .into()
}

fn battle_pass_metric(label: &'static str, value: String) -> Element<'static, Message> {
    column![text(label).size(12), text(value).size(16)]
        .spacing(4)
        .width(Length::FillPortion(1))
        .into()
}

fn battle_pass_reward_section<'a>(
    title: &'static str,
    rewards: &'a [BattlePassRewardDisplay],
    empty_label: &'static str,
) -> Element<'a, Message> {
    let mut section = column![text(title).size(18)].spacing(8).width(Length::Fill);

    if rewards.is_empty() {
        section = section.push(text(empty_label).size(13));
    } else {
        let mut cards = grid::Grid::new()
            .spacing(10)
            .fluid(BATTLE_PASS_REWARD_CARD_WIDTH)
            .height(grid::aspect_ratio(
                BATTLE_PASS_REWARD_CARD_WIDTH,
                BATTLE_PASS_REWARD_CARD_HEIGHT,
            ));

        for reward in rewards {
            cards = cards.push(battle_pass_reward_card(reward));
        }

        section = section.push(cards);
    }

    section.into()
}

fn battle_pass_reward_card(reward: &BattlePassRewardDisplay) -> Element<'_, Message> {
    let amount = reward
        .amount_label()
        .map(|amount| format!(" {amount}"))
        .unwrap_or_default();
    let meta = format!(
        "{} | {}{}",
        reward.location_label(),
        reward.track.label(),
        amount
    );
    let highlighted = reward.highlighted;

    container(
        column![
            asset_image(
                reward.cached_icon.as_ref(),
                BATTLE_PASS_REWARD_IMAGE_HEIGHT,
                &reward.name
            ),
            text(&reward.name).size(14).width(Length::Fill),
            text(&reward.kind).size(12).width(Length::Fill),
            text(meta).size(12).width(Length::Fill)
        ]
        .spacing(5),
    )
    .padding(9)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |theme| battle_pass_reward_card_style(theme, highlighted))
    .into()
}

fn battle_pass_reward_card_style(
    theme: &Theme,
    highlighted: bool,
) -> iced::widget::container::Style {
    let mut style = iced::widget::container::bordered_box(theme);

    if highlighted {
        style.background = Some(Color::from_rgba8(78, 58, 32, 0.72).into());
        style.border.color = Color::from_rgb8(218, 154, 72);
    }

    style
}
