//! The redesign's colours, fonts and icons. The colour names match the variables in the Pencil
//! design so a token there maps to one constant here.

use iced::theme::Palette;
use iced::theme::palette::{Extended, Pair};
use iced::widget::button::{Status, Style};
use iced::widget::svg;
use iced::{Border, Color, Element, Font, Theme, color, font};

use super::Message;

pub(super) const BG: Color = color!(0x0B0D11);
pub(super) const SURFACE: Color = color!(0x12151B);
pub(super) const RAISED: Color = color!(0x191D25);
pub(super) const LINE: Color = color!(0x242A34);
pub(super) const TEXT: Color = color!(0xECEEF2);
pub(super) const MUTED: Color = color!(0x8B93A1);
pub(super) const FAINT: Color = color!(0x5B6270);
pub(super) const ACCENT: Color = color!(0xFF4F5E);
pub(super) const ACCENT_SOFT: Color = color!(0xFF4F5E, 0.12);
pub(super) const OK: Color = color!(0x4CC974);
pub(super) const GOLD: Color = color!(0xE8BE55);

pub(super) const FONTS: [&[u8]; 3] = [
    include_bytes!("../../assets/fonts/Inter.ttf"),
    include_bytes!("../../assets/fonts/SpaceGrotesk.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono.ttf"),
];

pub(super) const BODY_FONT: Font = Font::with_name("Inter");
pub(super) const DISPLAY_FONT: Font = Font {
    weight: font::Weight::Bold,
    ..Font::with_name("Space Grotesk")
};
pub(super) const MONO_FONT: Font = Font::with_name("JetBrains Mono");

pub(super) fn theme() -> Theme {
    let palette = Palette {
        background: BG,
        text: TEXT,
        primary: ACCENT,
        success: OK,
        warning: GOLD,
        danger: ACCENT,
    };

    Theme::custom_with_fn("Prime", palette, |palette| {
        let mut extended = Extended::generate(palette);
        // Built-in boxes fill with `weakest` and draw their border with `weak`.
        extended.background.weakest = Pair::new(SURFACE, TEXT);
        extended.background.weaker = Pair::new(RAISED, TEXT);
        extended.background.weak = Pair::new(LINE, TEXT);
        extended
    })
}

/// Iced's `button`, but a plain dark button by default. Iced's own default is the accent colour,
/// which the design saves for the one main action on a screen. A `.style(...)` still overrides it.
pub(super) fn button<'a>(
    content: impl Into<Element<'a, Message>>,
) -> iced::widget::Button<'a, Message> {
    iced::widget::button(content).style(button_style)
}

/// The design's plain button: raised, with a thin border.
pub(super) fn button_style(_: &Theme, status: Status) -> Style {
    let style = Style {
        background: Some(RAISED.into()),
        text_color: TEXT,
        border: Border {
            color: LINE,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Style::default()
    };

    match status {
        Status::Active => style,
        Status::Hovered | Status::Pressed => Style {
            background: Some(LINE.into()),
            ..style
        },
        Status::Disabled => Style {
            text_color: FAINT,
            ..style
        },
    }
}

/// One of a set of choices, such as a sidebar tab: flat until hovered, filled when selected.
/// Ignores `Disabled`, because the selected choice is usually the one that can't be pressed.
pub(super) fn choice_style(_: &Theme, status: Status, selected: bool) -> Style {
    let (background, text_color) = if selected {
        (Some(LINE.into()), TEXT)
    } else if matches!(status, Status::Hovered | Status::Pressed) {
        (Some(RAISED.into()), TEXT)
    } else {
        (None, MUTED)
    };

    Style {
        background,
        text_color,
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..Style::default()
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Icon {
    Users,
    ShoppingBag,
    Swords,
    Settings,
}

impl Icon {
    fn svg(self) -> &'static [u8] {
        match self {
            Icon::Users => include_bytes!("../../assets/icons/users.svg"),
            Icon::ShoppingBag => include_bytes!("../../assets/icons/shopping-bag.svg"),
            Icon::Swords => include_bytes!("../../assets/icons/swords.svg"),
            Icon::Settings => include_bytes!("../../assets/icons/settings.svg"),
        }
    }
}

/// A Lucide icon drawn in one colour.
pub(super) fn icon<'a>(icon: Icon, size: f32, color: Color) -> Element<'a, Message> {
    svg(svg::Handle::from_memory(icon.svg()))
        .width(size)
        .height(size)
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}
