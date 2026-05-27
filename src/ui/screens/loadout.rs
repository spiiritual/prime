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
        .fluid(220)
        .height(grid::aspect_ratio(220, 252));
    let mut count = 0;

    for gun in guns {
        cards = cards.push(loadout_card(gun));
        count += 1;
    }

    (count > 0).then(|| column![text(category).size(20), cards].spacing(8).into())
}

fn loadout_card(gun: &LoadoutGunDisplay) -> Element<'_, Message> {
    let skin_label = loadout_skin_label(&gun.skin.display_name);

    container(
        column![
            asset_image(gun.skin.cached_icon.as_ref(), 164.0),
            text(&gun.weapon.display_name).size(15).width(Length::Fill),
            text(skin_label).size(12).width(Length::Fill)
        ]
        .spacing(6),
    )
    .padding(10)
    .width(Length::Fill)
    .style(iced::widget::container::bordered_box)
    .into()
}

fn loadout_skin_label(name: &str) -> String {
    let mut label = name.trim();

    while let Some(without_close) = label.strip_suffix(')') {
        let Some(open_index) = without_close.rfind('(') else {
            break;
        };
        let parenthetical = without_close[open_index + 1..].trim();

        if !parenthetical
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("variant"))
        {
            break;
        }

        label = without_close[..open_index].trim_end();
    }

    label.to_string()
}

#[cfg(test)]
mod tests {
    use super::loadout_skin_label;

    #[test]
    fn loadout_skin_label_removes_variant_suffix_but_keeps_level() {
        assert_eq!(
            loadout_skin_label("Prime Vandal Level 4 (Variant 1 Blue)"),
            "Prime Vandal Level 4"
        );
        assert_eq!(
            loadout_skin_label("Prime Vandal Level 4"),
            "Prime Vandal Level 4"
        );
        assert_eq!(
            loadout_skin_label("Prime Vandal (Upgraded)"),
            "Prime Vandal (Upgraded)"
        );
    }
}
