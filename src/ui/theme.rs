//! The redesign's colours, fonts and icons. The colour names match the variables in the Pencil
//! design so a token there maps to one constant here.

use iced::theme::Palette;
use iced::theme::palette::{Extended, Pair};
use iced::widget::button::{Status, Style};
use iced::widget::text::{IntoFragment, LineHeight};
use iced::widget::{row, svg};
use iced::{Border, Color, Element, Font, Theme, alignment, color, font};

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

/// One static file per weight. cosmic-text only picks a face whose weight matches exactly, and
/// registers a variable font at its default weight alone, so a variable font's other weights
/// silently fall back to a system font.
pub(super) const FONTS: [&[u8]; 10] = [
    include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../../assets/fonts/SpaceGrotesk-Medium.ttf"),
    include_bytes!("../../assets/fonts/SpaceGrotesk-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/SpaceGrotesk-Bold.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf"),
];

pub(super) const BODY_FONT: Font = Font::with_name("Inter");
pub(super) const MEDIUM_FONT: Font = Font {
    weight: font::Weight::Medium,
    ..BODY_FONT
};
pub(super) const SEMIBOLD_FONT: Font = Font {
    weight: font::Weight::Semibold,
    ..BODY_FONT
};
pub(super) const BOLD_FONT: Font = Font {
    weight: font::Weight::Bold,
    ..BODY_FONT
};
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

/// Each font's own line height, as the design lays text out. Iced's default of 1.3 makes every
/// line of Inter a pixel or two taller than the design.
pub(super) const BODY_LINE_HEIGHT: LineHeight = LineHeight::Relative(1.21);
pub(super) const DISPLAY_LINE_HEIGHT: LineHeight = LineHeight::Relative(1.28);
pub(super) const MONO_LINE_HEIGHT: LineHeight = LineHeight::Relative(1.32);

/// Iced's `text` with Inter's line height. Set `DISPLAY_LINE_HEIGHT` or `MONO_LINE_HEIGHT` along
/// with those fonts.
pub(super) fn text<'a>(content: impl IntoFragment<'a>) -> iced::widget::Text<'a> {
    iced::widget::text(content).line_height(BODY_LINE_HEIGHT)
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

/// The one main action in a dialog or header: light fill, dark text.
pub(super) fn primary_button_style(_: &Theme, status: Status) -> Style {
    solid_button_style(TEXT, status)
}

/// A destructive main action.
pub(super) fn danger_button_style(_: &Theme, status: Status) -> Style {
    solid_button_style(ACCENT, status)
}

/// A main action that installs or confirms something good, such as an update.
pub(super) fn success_button_style(_: &Theme, status: Status) -> Style {
    solid_button_style(OK, status)
}

fn solid_button_style(color: Color, status: Status) -> Style {
    let alpha = match status {
        Status::Active => 1.0,
        Status::Hovered | Status::Pressed => 0.85,
        Status::Disabled => 0.4,
    };

    Style {
        background: Some(Color { a: alpha, ..color }.into()),
        text_color: Color { a: alpha, ..BG },
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..Style::default()
    }
}

/// A button label with a leading icon, in the button's text colour.
pub(super) fn icon_label<'a>(
    icon_kind: Icon,
    label: &'a str,
    color: Color,
) -> Element<'a, Message> {
    row![
        icon(icon_kind, 14.0, color),
        text(label).size(13).font(BOLD_FONT)
    ]
    .spacing(7)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// Iced's `text_input` in the design's field style.
pub(super) fn text_input<'a>(
    placeholder: &str,
    value: &str,
) -> iced::widget::TextInput<'a, Message> {
    iced::widget::text_input(placeholder, value)
        .padding([10, 12])
        .size(13)
        .line_height(BODY_LINE_HEIGHT)
        .style(input_style)
}

fn input_style(theme: &Theme, status: iced::widget::text_input::Status) -> InputStyle {
    field_style(BG, false)(theme, status)
}

type InputStyle = iced::widget::text_input::Style;

/// The design's field on a given background; an invalid field keeps an accent border.
pub(super) fn field_style(
    background: Color,
    invalid: bool,
) -> impl Fn(&Theme, iced::widget::text_input::Status) -> InputStyle {
    move |_, status| {
        use iced::widget::text_input::Status;

        let border_color = match status {
            _ if invalid => ACCENT,
            Status::Focused { .. } => ACCENT,
            Status::Hovered => FAINT,
            Status::Active | Status::Disabled => LINE,
        };

        InputStyle {
            background: background.into(),
            border: Border {
                color: border_color,
                width: 1.0,
                radius: 8.0.into(),
            },
            icon: MUTED,
            placeholder: FAINT,
            value: if matches!(status, Status::Disabled) {
                MUTED
            } else {
                TEXT
            },
            selection: Color { a: 0.35, ..ACCENT },
        }
    }
}

/// One of a set of choices, such as a sidebar tab: flat until hovered, filled when selected.
/// Ignores `Disabled`, because the selected choice is usually the one that can't be pressed.
pub(super) fn choice_style(_: &Theme, status: Status, selected: bool) -> Style {
    let (background, text_color) = if selected {
        (Some(RAISED.into()), TEXT)
    } else if matches!(status, Status::Hovered | Status::Pressed) {
        (Some(Color { a: 0.5, ..RAISED }.into()), TEXT)
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
    /// Prime's own mark, not a Lucide icon.
    Logo,
    ArrowRight,
    Check,
    ChevronDown,
    ChevronRight,
    ChevronsUpDown,
    CircleCheck,
    CircleX,
    Download,
    Eraser,
    FolderOpen,
    Info,
    MousePointerClick,
    Play,
    Plus,
    Power,
    RefreshCw,
    Settings,
    Settings2,
    ShoppingBag,
    Sparkles,
    Swords,
    Trash,
    TriangleAlert,
    Users,
    X,
}

impl Icon {
    fn svg(self) -> &'static [u8] {
        match self {
            Icon::Logo => include_bytes!("../../assets/logo.svg"),
            Icon::ArrowRight => include_bytes!("../../assets/icons/arrow-right.svg"),
            Icon::Check => include_bytes!("../../assets/icons/check.svg"),
            Icon::ChevronDown => include_bytes!("../../assets/icons/chevron-down.svg"),
            Icon::ChevronRight => include_bytes!("../../assets/icons/chevron-right.svg"),
            Icon::CircleCheck => include_bytes!("../../assets/icons/circle-check.svg"),
            Icon::CircleX => include_bytes!("../../assets/icons/circle-x.svg"),
            Icon::ChevronsUpDown => include_bytes!("../../assets/icons/chevrons-up-down.svg"),
            Icon::Download => include_bytes!("../../assets/icons/download.svg"),
            Icon::Eraser => include_bytes!("../../assets/icons/eraser.svg"),
            Icon::FolderOpen => include_bytes!("../../assets/icons/folder-open.svg"),
            Icon::Info => include_bytes!("../../assets/icons/info.svg"),
            Icon::MousePointerClick => {
                include_bytes!("../../assets/icons/mouse-pointer-click.svg")
            }
            Icon::Play => include_bytes!("../../assets/icons/play.svg"),
            Icon::Plus => include_bytes!("../../assets/icons/plus.svg"),
            Icon::Power => include_bytes!("../../assets/icons/power.svg"),
            Icon::RefreshCw => include_bytes!("../../assets/icons/refresh-cw.svg"),
            Icon::Settings => include_bytes!("../../assets/icons/settings.svg"),
            Icon::Settings2 => include_bytes!("../../assets/icons/settings-2.svg"),
            Icon::ShoppingBag => include_bytes!("../../assets/icons/shopping-bag.svg"),
            Icon::Sparkles => include_bytes!("../../assets/icons/sparkles.svg"),
            Icon::Swords => include_bytes!("../../assets/icons/swords.svg"),
            Icon::Trash => include_bytes!("../../assets/icons/trash-2.svg"),
            Icon::TriangleAlert => include_bytes!("../../assets/icons/triangle-alert.svg"),
            Icon::Users => include_bytes!("../../assets/icons/users.svg"),
            Icon::X => include_bytes!("../../assets/icons/x.svg"),
        }
    }
}

/// A Lucide icon drawn in one colour.
pub(super) fn icon<'a>(icon: Icon, size: f32, color: Color) -> Element<'a, Message> {
    sized_icon(icon, size, size, color)
}

pub(super) fn sized_icon<'a>(
    icon: Icon,
    width: f32,
    height: f32,
    color: Color,
) -> Element<'a, Message> {
    svg(svg::Handle::from_memory(icon.svg()))
        .width(width)
        .height(height)
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

#[cfg(test)]
mod tests {
    use iced::advanced::graphics::text::cosmic_text::{
        Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight, fontdb,
    };
    use std::sync::Arc;

    use super::*;

    #[test]
    fn every_font_resolves_to_its_own_face_not_a_system_fallback() {
        // System fonts included, as in the app, so a miss falls back to Segoe UI and fails.
        let mut system =
            FontSystem::new_with_fonts(FONTS.map(|bytes| fontdb::Source::Binary(Arc::new(bytes))));

        for font in [
            BODY_FONT,
            MEDIUM_FONT,
            SEMIBOLD_FONT,
            BOLD_FONT,
            DISPLAY_FONT,
            MONO_FONT,
        ] {
            let iced::font::Family::Name(family) = font.family else {
                panic!("{font:?} has no family name");
            };
            let weight = Weight(match font.weight {
                font::Weight::Medium => 500,
                font::Weight::Semibold => 600,
                font::Weight::Bold => 700,
                _ => 400,
            });
            let attrs = Attrs::new().family(Family::Name(family)).weight(weight);
            let mut buffer = Buffer::new(&mut system, Metrics::new(20.0, 24.0));
            buffer.set_text(&mut system, "Good", &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut system, false);
            let glyph_font = buffer.layout_runs().next().expect("a line").glyphs[0].font_id;
            let face = system.db().face(glyph_font).expect("the face");

            assert_eq!(face.families[0].0, family, "{font:?}");
            assert_eq!(face.weight, weight, "{font:?}");
        }
    }
}
