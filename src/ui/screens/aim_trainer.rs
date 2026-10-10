use iced::Element;

use crate::ui::{Message, PrimeApp};

pub(super) fn tab(_app: &PrimeApp) -> Element<'_, Message> {
    iced::widget::Space::new().into()
}
