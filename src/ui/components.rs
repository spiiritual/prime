use std::path::PathBuf;

use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer, widget::Tree,
};
use iced::widget::image::Handle;
use iced::widget::{Column, Row, button, container, image, row, space, text};
use iced::{
    Color, ContentFit, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector,
    alignment,
};

use super::data::{CurrencyBalanceDisplay, StoreSummary};
use super::{ImageViewerRequest, ImageViewerSource, Message, image_viewer_enabled};

// Keeps popovers in the overlay layer so controls are not clipped by their parent card.
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
        None => container(text("No image").size(13))
            .width(Length::Fill)
            .height(height)
            .style(iced::widget::container::rounded_box)
            .into(),
    }
}

pub(super) fn asset_background_image<'a>(
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
                .content_fit(ContentFit::Cover),
            path,
            height,
            title,
            high_res,
        ),
        None => container(text("No image").size(13))
            .width(Length::Fill)
            .height(height)
            .style(iced::widget::container::rounded_box)
            .into(),
    }
}

fn preview_image_button<'a>(
    image: impl Into<Element<'a, Message>>,
    path: &PathBuf,
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
                path.clone(),
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

pub(super) fn loading_line(label: &'static str, frame: usize) -> Element<'static, Message> {
    row![loading_indicator(frame), text(label).size(15)]
        .spacing(10)
        .align_y(alignment::Vertical::Center)
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
        let label = if summary.currency_balance_error.is_some() {
            "Currency balances unavailable"
        } else {
            "No currency balances returned"
        };
        text(label).size(14).into()
    } else {
        currency_balance_row(&summary.currency_balances)
    }
}

fn currency_balance_row<'a>(balances: &'a [CurrencyBalanceDisplay]) -> Element<'a, Message> {
    let mut row = iced::widget::Row::new().spacing(10);

    for balance in balances {
        row = row.push(currency_balance_chip(balance));
    }

    row.into()
}

fn currency_balance_chip(balance: &CurrencyBalanceDisplay) -> Element<'_, Message> {
    container(text(balance.label()).size(16))
        .padding([6, 10])
        .style(iced::widget::container::bordered_box)
        .into()
}

#[cfg(test)]
mod tests {
    use super::high_res_image_source;

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
}
