use std::borrow::Cow;
use std::path::{Path, PathBuf};

use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer, widget::Tree,
};
use iced::widget::image::Handle;
use iced::widget::{Column, Row, column, container, image, responsive, row, space};
use iced::{
    Color, ContentFit, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector,
    alignment,
};

use super::data::shop::{
    CurrencyBalanceDisplay, CurrencyDisplay, StoreSummary, format_whole_number,
};
use super::theme::{self, button, text};
use super::{ImageViewerRequest, ImageViewerSource, Message, image_viewer_enabled};

const RADIANITE_COLOR: Color = iced::color!(0x54D1C2);

// Keeps popovers in the overlay layer so controls are not clipped by their parent card. A left
// click outside both the popover and its anchor publishes `Message::DismissPopovers`; clicks on the
// anchor are left to its own toggle.
pub(super) fn anchored_popover<'a>(
    base: impl Into<Element<'a, Message>>,
    popover: impl Into<Element<'a, Message>>,
    is_open: bool,
    top_offset: f32,
    right_inset: f32,
) -> Element<'a, Message> {
    Element::new(AnchoredPopover {
        base: base.into(),
        popover: popover.into(),
        is_open,
        top_offset,
        right_inset,
    })
}

struct AnchoredPopover<'a> {
    base: Element<'a, Message>,
    popover: Element<'a, Message>,
    is_open: bool,
    top_offset: f32,
    right_inset: f32,
}

impl Widget<Message, Theme, Renderer> for AnchoredPopover<'_> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.base), Tree::new(&self.popover)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.base.as_widget(), self.popover.as_widget()]);
    }

    fn size(&self) -> Size<Length> {
        self.base.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.base.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.base
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.base
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.base.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.base.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.base.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let mut children = tree.children.iter_mut();

        let base_overlay = self.base.as_widget_mut().overlay(
            children.next().unwrap(),
            layout,
            renderer,
            viewport,
            translation,
        );

        let popover_overlay = self.is_open.then(|| {
            overlay::Element::new(Box::new(AnchoredOverlay {
                anchor: layout.bounds() + translation,
                popover: &mut self.popover,
                tree: children.next().unwrap(),
                top_offset: self.top_offset,
                right_inset: self.right_inset,
            }))
        });

        if base_overlay.is_some() || popover_overlay.is_some() {
            Some(
                overlay::Group::with_children(
                    base_overlay.into_iter().chain(popover_overlay).collect(),
                )
                .overlay(),
            )
        } else {
            None
        }
    }
}

struct AnchoredOverlay<'a, 'b> {
    anchor: Rectangle,
    popover: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    top_offset: f32,
    right_inset: f32,
}

impl overlay::Overlay<Message, Theme, Renderer> for AnchoredOverlay<'_, '_> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let viewport = Rectangle::with_size(bounds);
        let popover = self.popover.as_widget_mut().layout(
            self.tree,
            renderer,
            &layout::Limits::new(Size::ZERO, viewport.size()),
        );
        let popover_size = popover.size();

        let max_x = viewport.x + viewport.width - popover_size.width;
        let x = (self.anchor.x + self.anchor.width - popover_size.width - self.right_inset)
            .clamp(viewport.x, max_x.max(viewport.x));

        let desired_y = self.anchor.y + self.top_offset;
        let max_y = viewport.y + viewport.height - popover_size.height;
        let y = if desired_y > max_y {
            (self.anchor.y + self.anchor.height - popover_size.height)
                .clamp(viewport.y, max_y.max(viewport.y))
        } else {
            desired_y
        };

        layout::Node::with_children(popover_size, vec![popover]).move_to(Point::new(x, y))
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.popover.as_widget_mut().operate(
            self.tree,
            layout.children().next().unwrap(),
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let viewport = Rectangle::with_size(Size::INFINITE);

        self.popover.as_widget_mut().update(
            self.tree,
            event,
            layout.children().next().unwrap(),
            cursor,
            renderer,
            clipboard,
            shell,
            &viewport,
        );

        // Not captured, so the click still reaches whatever is under it.
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) && !cursor.is_over(layout.bounds())
            && !cursor.is_over(self.anchor)
        {
            shell.publish(Message::DismissPopovers);
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let viewport = Rectangle::with_size(Size::INFINITE);

        self.popover.as_widget().mouse_interaction(
            self.tree,
            layout.children().next().unwrap(),
            cursor,
            &viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let viewport = Rectangle::with_size(Size::INFINITE);

        self.popover.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            layout.children().next().unwrap(),
            cursor,
            &viewport,
        );
    }

    fn overlay<'a>(
        &'a mut self,
        layout: Layout<'a>,
        renderer: &Renderer,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        let viewport = Rectangle::with_size(Size::INFINITE);

        self.popover.as_widget_mut().overlay(
            self.tree,
            layout.children().next().unwrap(),
            renderer,
            &viewport,
            Vector::ZERO,
        )
    }

    fn index(&self) -> f32 {
        10.0
    }
}

pub(super) fn asset_image<'a>(
    path: Option<&'a PathBuf>,
    height: f32,
    title: impl Into<String>,
    high_res: Option<ImageViewerSource>,
) -> Element<'a, Message> {
    let title = title.into();

    match path {
        Some(path) => preview_image_button(
            image(Handle::from_path(path.clone()))
                .width(Length::Fill)
                .height(height)
                .content_fit(ContentFit::Contain),
            path,
            height,
            title,
            high_res,
        ),
        None => no_image_placeholder(height),
    }
}

pub(super) fn asset_background_image<'a>(
    path: Option<&'a PathBuf>,
    height: f32,
    radius: f32,
    title: impl Into<String>,
    high_res: Option<ImageViewerSource>,
) -> Element<'a, Message> {
    let title = title.into();

    match path {
        Some(path) => preview_image_button(
            image(Handle::from_path(path.clone()))
                .width(Length::Fill)
                .height(height)
                .border_radius(radius)
                .content_fit(ContentFit::Cover),
            path,
            height,
            title,
            high_res,
        ),
        None => no_image_placeholder(height),
    }
}

fn no_image_placeholder<'a>(height: f32) -> Element<'a, Message> {
    container(text("No image").size(13))
        .width(Length::Fill)
        .height(height)
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Center)
        .style(iced::widget::container::rounded_box)
        .into()
}

fn preview_image_button<'a>(
    image: impl Into<Element<'a, Message>>,
    path: &Path,
    height: f32,
    title: String,
    high_res: Option<ImageViewerSource>,
) -> Element<'a, Message> {
    let image = image.into();

    if image_viewer_enabled() {
        button(image)
            .padding(0)
            .width(Length::Fill)
            .height(height)
            .style(preview_image_button_style)
            .on_press(Message::OpenImageViewer(ImageViewerRequest::new(
                path.to_path_buf(),
                title,
                high_res,
            )))
            .into()
    } else {
        container(image).width(Length::Fill).height(height).into()
    }
}

pub(super) fn high_res_image_source(
    namespace: &str,
    id: &str,
    thumbnail_url: Option<&str>,
    viewer_url: Option<&str>,
) -> Option<ImageViewerSource> {
    let viewer_url = viewer_url
        .map(str::trim)
        .filter(|viewer_url| !viewer_url.is_empty())?;

    if thumbnail_url
        .map(str::trim)
        .is_some_and(|thumbnail_url| thumbnail_url == viewer_url)
    {
        return None;
    }

    Some(ImageViewerSource::new(namespace, id, viewer_url))
}

fn preview_image_button_style(
    _: &Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let mut style = iced::widget::button::Style {
        text_color: Color::WHITE,
        ..Default::default()
    };

    if matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    ) {
        style.background = Some(Color::from_rgba8(255, 255, 255, 0.05).into());
    }

    style
}

/// Why a load failed, with Try again, in place of what it would have shown. `retry` is `None`
/// while a new load is already running.
pub(super) fn load_error_panel(
    title: &'static str,
    error: &str,
    retry: Option<Message>,
) -> Element<'static, Message> {
    container(
        column![
            text(title).size(18),
            text(error.to_string()).size(14),
            button("Try again").on_press_maybe(retry)
        ]
        .spacing(10),
    )
    .padding(16)
    .width(Length::Fill)
    .style(container::bordered_box)
    .into()
}

/// A page that couldn't load, centred in the space the page would fill: an icon, what failed,
/// what to do, the actions, and the raw error for reference.
pub(super) fn unavailable_state<'a>(
    icon: theme::Icon,
    title: &'a str,
    body: String,
    actions: Vec<Element<'a, Message>>,
    detail: &'a str,
) -> Element<'a, Message> {
    let badge = container(theme::icon(icon, 24.0, theme::ACCENT))
        .width(52)
        .height(52)
        .center_x(52)
        .center_y(52)
        .style(|_| {
            container::Style::default()
                .background(Color {
                    a: 0x1F as f32 / 255.0,
                    ..theme::ACCENT
                })
                .border(iced::border::rounded(14))
        });
    let detail = container(
        text(detail)
            .size(11)
            .font(theme::MONO_FONT)
            .line_height(theme::MONO_LINE_HEIGHT)
            .color(theme::FAINT),
    )
    .padding([8, 12])
    .max_width(560)
    .style(|_| {
        container::Style::default()
            .background(theme::SURFACE)
            .border(iced::Border {
                color: theme::LINE,
                width: 1.0,
                radius: 8.0.into(),
            })
    });

    container(
        column![
            badge,
            text(title)
                .size(20)
                .font(theme::DISPLAY_FONT)
                .line_height(theme::DISPLAY_LINE_HEIGHT),
            text(body)
                .size(13)
                .line_height(1.5)
                .color(theme::MUTED)
                .width(440)
                .align_x(alignment::Horizontal::Center),
            Row::with_children(actions).spacing(8),
            detail,
        ]
        .spacing(14)
        .align_x(alignment::Horizontal::Center),
    )
    .center(Length::Fill)
    .into()
}

/// A placeholder block while content loads.
pub(super) fn skeleton<'a>(
    width: impl Into<Length>,
    height: f32,
    radius: f32,
    opacity: f32,
) -> Element<'a, Message> {
    container(space())
        .width(width)
        .height(height)
        .style(move |_| {
            container::Style::default()
                .background(Color {
                    a: opacity,
                    ..theme::RAISED
                })
                .border(iced::border::rounded(radius))
        })
        .into()
}

/// A soft radial glow of `color` over an opaque `base`, filling its space, drawn as SVG because
/// Iced's own gradients are linear only. `center_y` and `scale` are fractions of the box, as the
/// design sets them; `radii` round the corners clockwise from the top left.
pub(super) fn radial_glow<'a>(
    base: Color,
    color: Color,
    alpha: f32,
    center_y: f32,
    scale: (f32, f32),
    radii: [f32; 4],
) -> Element<'a, Message> {
    let hex = |color: Color| {
        let [red, green, blue, _] = color.into_rgba8();
        format!("#{red:02x}{green:02x}{blue:02x}")
    };
    let (base, color) = (hex(base), hex(color));
    let (scale_x, scale_y) = scale;
    let [top_left, top_right, bottom_right, bottom_left] = radii;

    // Iced rasterizes an SVG at its own aspect ratio, so the SVG takes the box's size. It also
    // blends the rasterized pixels as if they weren't premultiplied, which fades anything
    // translucent twice, so the glow is painted over its opaque base instead of left see-through.
    iced::widget::responsive(move |size| {
        let (width, height) = (size.width.max(1.0), size.height.max(1.0));
        let shape = format!(
            "M{top_left} 0H{}A{top_right} {top_right} 0 0 1 {width} {top_right}V{}             A{bottom_right} {bottom_right} 0 0 1 {} {height}H{bottom_left}             A{bottom_left} {bottom_left} 0 0 1 0 {}V{top_left}A{top_left} {top_left} 0 0 1 {top_left} 0Z",
            width - top_right,
            height - bottom_right,
            width - bottom_right,
            height - bottom_left,
        );
        let svg_source = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}">
<defs><radialGradient id="g" cx="0.5" cy="{center_y}" r="0.5"
gradientTransform="translate(0.5 {center_y}) scale({scale_x} {scale_y}) translate(-0.5 -{center_y})">
<stop offset="0" stop-color="{color}" stop-opacity="{alpha}"/>
<stop offset="1" stop-color="{color}" stop-opacity="0"/>
</radialGradient></defs><path d="{shape}" fill="{base}"/><path d="{shape}" fill="url(#g)"/></svg>"##
        );

        iced::widget::svg(iced::widget::svg::Handle::from_memory(svg_source.into_bytes()))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    })
    .into()
}

pub(super) fn loading_line(label: &'static str, frame: usize) -> Element<'static, Message> {
    row![loading_indicator(frame), text(label).size(15)]
        .spacing(10)
        .align_y(alignment::Vertical::Center)
        .into()
}

/// A one-line item name that shrinks to fit the width its card really has, and is clipped to the
/// card if it still doesn't fit at the smallest size.
pub(super) fn compact_item_name<'a>(
    name: impl Into<Cow<'a, str>>,
    base_size: u32,
    height: f32,
) -> Element<'a, Message> {
    let name: Cow<'a, str> = name.into();

    container(responsive(move |size| {
        text(name.clone())
            .size(compact_item_name_size(&name, base_size, size.width))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Left)
            .align_y(alignment::Vertical::Center)
            .wrapping(iced::widget::text::Wrapping::None)
            .into()
    }))
    .width(Length::Fill)
    .height(height)
    .clip(true)
    .into()
}

fn compact_item_name_size(name: &str, base_size: u32, available_width: f32) -> u32 {
    let text_units: f32 = name.chars().map(compact_item_name_char_width).sum();
    let estimated_width = text_units * base_size as f32;

    if estimated_width <= available_width {
        return base_size;
    }

    let minimum_size = base_size.saturating_sub(5).max(8);
    ((available_width / text_units).floor() as u32).clamp(minimum_size, base_size)
}

fn compact_item_name_char_width(character: char) -> f32 {
    match character {
        ' ' => 0.32,
        'i' | 'l' | 'I' | '1' | '!' | '\'' | '.' | ',' | ':' | ';' | '|' => 0.3,
        'm' | 'w' | 'M' | 'W' => 0.85,
        'A'..='Z' => 0.64,
        '0'..='9' => 0.52,
        '-' | '/' | '\\' => 0.42,
        _ => 0.54,
    }
}

/// One button of a row of sub-tabs, such as Loadout's Skins and Battle Pass.
pub(super) fn sub_tab_button(
    label: String,
    is_selected: bool,
    on_press: Message,
) -> Element<'static, Message> {
    button(text(label).size(14))
        .padding([12, 16])
        .width(Length::Fill)
        .height(46)
        .style(move |theme, status| theme::choice_style(theme, status, is_selected))
        .on_press_maybe((!is_selected).then_some(on_press))
        .into()
}

pub(super) fn loading_indicator(frame: usize) -> Element<'static, Message> {
    loading_orbit(frame, 5.0, 7.0, 2)
}

pub(super) fn compact_loading_indicator(frame: usize) -> Element<'static, Message> {
    loading_orbit(frame, 3.0, 5.0, 1)
}

fn loading_orbit(
    frame: usize,
    dot_size: f32,
    slot_size: f32,
    spacing: u32,
) -> Element<'static, Message> {
    let active = frame % 8;
    let mut grid = Column::new().spacing(spacing);

    for row_index in 0..3 {
        let mut row = Row::new().spacing(spacing);

        for column_index in 0..3 {
            let cell: Element<_> = if let Some(index) = orbit_index(row_index, column_index) {
                let intensity = loading_intensity(active, index, 8);
                let size = dot_size * (0.45 + 0.55 * intensity);
                container(space())
                    .width(size)
                    .height(size)
                    .style(move |_| loading_shape_style(intensity, size / 2.0))
                    .into()
            } else {
                space().width(0.0).height(0.0).into()
            };

            row = row.push(
                container(cell)
                    .width(slot_size)
                    .height(slot_size)
                    .align_x(alignment::Horizontal::Center)
                    .align_y(alignment::Vertical::Center),
            );
        }

        grid = grid.push(row);
    }

    grid.into()
}

fn orbit_index(row: usize, column: usize) -> Option<usize> {
    match (row, column) {
        (0, 1) => Some(0),
        (0, 2) => Some(1),
        (1, 2) => Some(2),
        (2, 2) => Some(3),
        (2, 1) => Some(4),
        (2, 0) => Some(5),
        (1, 0) => Some(6),
        (0, 0) => Some(7),
        _ => None,
    }
}

fn loading_intensity(active: usize, index: usize, count: usize) -> f32 {
    let distance = active.abs_diff(index);
    let wrapped_distance = distance.min(count - distance);

    match wrapped_distance {
        0 => 1.0,
        1 => 0.62,
        2 => 0.34,
        _ => 0.18,
    }
}

fn loading_shape_style(intensity: f32, radius: f32) -> iced::widget::container::Style {
    let alpha = 0.18 + 0.74 * intensity;
    let mut style = iced::widget::container::Style {
        background: Some(Color::from_rgba8(255, 255, 255, alpha).into()),
        ..Default::default()
    };
    style.border.radius = iced::border::radius(radius);
    style
}

pub(super) fn currency_balance_display(summary: &StoreSummary) -> Element<'_, Message> {
    if summary.currency_balances.is_empty() {
        balances_unavailable()
    } else {
        currency_balance_row(&summary.currency_balances)
    }
}

/// The wallet's place in the header when there are no balances to show.
pub(super) fn balances_unavailable<'a>() -> Element<'a, Message> {
    container(text("Balances unavailable").size(12).color(theme::FAINT))
        .padding([8, 12])
        .style(|_| {
            container::Style::default().border(iced::Border {
                color: theme::LINE,
                width: 1.0,
                radius: 10.0.into(),
            })
        })
        .into()
}

/// Skeleton pills in the wallet's place while the shop loads.
pub(super) fn wallet_skeleton<'a>() -> Element<'a, Message> {
    row![
        skeleton(92, 34.0, 8.0, 1.0),
        skeleton(64, 34.0, 8.0, 1.0),
        skeleton(84, 34.0, 8.0, 1.0),
    ]
    .spacing(8)
    .into()
}

fn currency_balance_row<'a>(balances: &'a [CurrencyBalanceDisplay]) -> Element<'a, Message> {
    let mut row = iced::widget::Row::new().spacing(4);

    for balance in balances {
        row = row.push(currency_balance_chip(balance));
    }

    container(row)
        .padding(4)
        .style(|theme| {
            let mut style = iced::widget::container::bordered_box(theme);
            style.border.radius = 10.0.into();
            style
        })
        .into()
}

/// A currency's short name and colour. The wallet already names its currencies VP, Radianite and
/// Kingdom Credits.
fn currency_style(currency: &CurrencyDisplay) -> (&str, Color) {
    match currency.display_name.as_str() {
        "VP" => ("VP", theme::ACCENT),
        "Radianite" => ("RAD", RADIANITE_COLOR),
        "Kingdom Credits" => ("KC", theme::GOLD),
        other => (other, theme::MUTED),
    }
}

/// The small round currency marker in front of an amount.
pub(super) fn currency_dot<'a>(currency: &CurrencyDisplay, size: f32) -> Element<'a, Message> {
    let (_, color) = currency_style(currency);

    container(space())
        .width(size)
        .height(size)
        .style(move |_| {
            iced::widget::container::Style::default()
                .background(color)
                .border(iced::border::rounded(size / 2.0))
        })
        .into()
}

fn currency_balance_chip(balance: &CurrencyBalanceDisplay) -> Element<'_, Message> {
    let (short_name, _) = currency_style(&balance.currency);

    container(
        row![
            currency_dot(&balance.currency, 8.0),
            text(format_whole_number(balance.amount))
                .size(13)
                .font(theme::MONO_SEMIBOLD_FONT)
                .line_height(theme::MONO_LINE_HEIGHT),
            text(short_name)
                .size(11)
                .font(theme::SEMIBOLD_FONT)
                .color(theme::FAINT)
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center),
    )
    .padding([6, 12])
    .into()
}

#[cfg(test)]
mod tests {
    use super::{compact_item_name_size, high_res_image_source};

    #[test]
    fn high_res_image_source_ignores_missing_or_duplicate_urls() {
        assert!(high_res_image_source("viewer", "id", Some("thumb"), None).is_none());
        assert!(high_res_image_source("viewer", "id", Some("same"), Some("same")).is_none());
        assert!(high_res_image_source("viewer", "id", Some("same"), Some(" same ")).is_none());
    }

    #[test]
    fn high_res_image_source_keeps_distinct_viewer_url() {
        let source =
            high_res_image_source("viewer", "id", Some("thumb"), Some("full")).expect("source");

        assert_eq!(source.namespace, "viewer");
        assert_eq!(source.id, "id");
        assert_eq!(source.url, "full");
    }

    #[test]
    fn compact_item_name_size_shrinks_to_available_width() {
        assert_eq!(compact_item_name_size("Short Name", 14, 156.0), 14);
        assert_eq!(
            compact_item_name_size("Radiant Crisis 001 Baseball Bat", 16, 212.0),
            14
        );
        assert_eq!(
            compact_item_name_size("Radiant Crisis 001 Baseball Bat", 14, 156.0),
            10
        );
        assert_eq!(
            compact_item_name_size("A very long reward or shop item name", 14, 156.0),
            9
        );
    }
}
