use iced::widget::{column, container, grid, text};
use iced::{Element, Length};

use super::super::components::{asset_image, loading_line};
use super::super::data::{LoadoutGunDisplay, weapon_category};
use super::super::{Message, PrimeApp};

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
