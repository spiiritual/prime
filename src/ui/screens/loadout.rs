use iced::widget::{button, column, container, grid, row, text};
use iced::{Color, Element, Length, Theme, border};

use super::super::components::{asset_image, loading_line};
use super::super::data::{LoadoutGunDisplay, weapon_category};
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
        LoadoutTab::BattlePass => container(column![])
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
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
            asset_image(gun.skin.cached_icon.as_ref(), LOADOUT_IMAGE_HEIGHT),
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
