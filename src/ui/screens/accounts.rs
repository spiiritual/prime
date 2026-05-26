use iced::widget::{
    button, column, container, opaque, pick_list, row, space, stack, text, text_input,
};
use iced::{Color, Element, Length, Theme, alignment};

use crate::account::{AccountId, AccountProfile, Shard};

use super::super::components::anchored_popover;
use super::super::{Message, PrimeApp};

const ACCOUNT_MENU_WIDTH: f32 = 180.0;
const ACCOUNT_MENU_TOP_OFFSET: f32 = 48.0;
const ACCOUNT_MENU_RIGHT_INSET: f32 = 14.0;

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    let mut account_cards = column![].spacing(12).width(Length::Fill);

    if app.state.accounts.is_empty() {
        account_cards = account_cards.push(
            container(text("No account profiles yet"))
                .padding(16)
                .width(Length::Fill)
                .style(iced::widget::container::bordered_box),
        );
    }

    for account in &app.state.accounts {
        account_cards = account_cards.push(account_card(app, account));
    }

    let mut content = column![
        row![button("Add account").on_press(Message::AddAccount)].spacing(10),
        text("Add account opens Riot Client, waits for a remembered login, then asks you to confirm the profile details."),
        account_cards
    ]
    .spacing(12)
    .width(Length::Fill);

    if let Some(draft) = &app.pending_account {
        content = content.push(
            container(
                column![
                    text("Confirm captured account").size(22),
                    text(format!("PUUID: {}", draft.puuid)),
                    row![
                        text_input("Display name", &app.new_display_name)
                            .on_input(Message::NewDisplayNameChanged)
                            .width(Length::Fill),
                        text_input("Riot username", &app.new_username)
                            .on_input(Message::NewUsernameChanged)
                            .width(Length::Fill),
                        pick_list(
                            Shard::ALL.as_slice(),
                            Some(app.new_shard),
                            Message::NewShardSelected
                        )
                    ]
                    .spacing(10),
                    row![
                        button("Save account").on_press(Message::ConfirmCapturedAccount),
                        button("Cancel").on_press(Message::CancelCapturedAccount)
                    ]
                    .spacing(10)
                ]
                .spacing(10),
            )
            .padding(16)
            .style(iced::widget::container::bordered_box),
        );
    }

    content.into()
}

fn account_card<'a>(app: &'a PrimeApp, account: &'a AccountProfile) -> Element<'a, Message> {
    let riot_tag = account
        .riot_id()
        .unwrap_or_else(|| "Riot tag not captured".to_string());
    let session_label = if account.has_launcher_session() {
        "Launcher session captured"
    } else {
        "Launcher session not captured"
    };
    let is_selected = app.state.selected_account == Some(account.id);
    let is_account_menu_open = app.open_account_menu == Some(account.id);
    let selected_label = if is_selected {
        "Selected"
    } else {
        "Not selected"
    };

    let header = row![
        column![
            text(&account.display_name).size(22),
            text(riot_tag).size(15)
        ]
        .spacing(4)
        .width(Length::Fill),
        button(text("..."))
            .style(move |theme, status| {
                account_menu_button_style(theme, status, is_account_menu_open)
            })
            .on_press(Message::ToggleAccountMenu(account.id))
    ]
    .spacing(10)
    .align_y(alignment::Vertical::Top);

    let body = column![
        header,
        text(format!(
            "Riot shard: {} | {} | {}",
            account.shard, session_label, selected_label
        ))
        .size(13),
    ]
    .spacing(10)
    .width(Length::Fill);

    let body = body.push(
        row![
            button("Select")
                .style(move |theme, status| {
                    select_account_button_style(theme, status, is_selected)
                })
                .on_press_maybe((!is_selected).then_some(Message::SelectAccount(account.id))),
            space().width(Length::Fill),
            button("Launch VALORANT").on_press(Message::LaunchAccount(account.id))
        ]
        .spacing(10)
        .width(Length::Fill),
    );

    let base = container(body)
        .padding(14)
        .width(Length::Fill)
        .style(move |theme| account_card_style(theme, is_selected));

    let mut card = stack![base].width(Length::Fill);

    if app.confirm_delete_account == Some(account.id) {
        card = card.push(delete_account_prompt_overlay(account));
    }

    anchored_popover(
        card,
        account_menu(account.id),
        is_account_menu_open,
        ACCOUNT_MENU_TOP_OFFSET,
        ACCOUNT_MENU_RIGHT_INSET,
    )
}

fn account_menu(account_id: AccountId) -> Element<'static, Message> {
    container(
        column![
            button("Re-capture login")
                .width(Length::Fill)
                .on_press(Message::StartLauncherSessionLogin(account_id)),
            button("Refresh profile")
                .width(Length::Fill)
                .on_press(Message::RefreshProfileIdentity(account_id)),
            button("Delete account")
                .width(Length::Fill)
                .style(iced::widget::button::danger)
                .on_press(Message::RequestDeleteAccount(account_id))
        ]
        .spacing(8),
    )
    .padding(8)
    .width(ACCOUNT_MENU_WIDTH)
    .style(iced::widget::container::bordered_box)
    .into()
}

fn delete_account_prompt_overlay(account: &AccountProfile) -> Element<'_, Message> {
    let prompt = container(
        column![
            column![
                text(format!("Delete {}?", account.display_name)).size(16),
                text("This removes the local profile and captured launcher session reference.")
                    .size(13)
            ]
            .spacing(3)
            .width(Length::Fill),
            row![
                space().width(Length::Fill),
                button("Cancel").on_press(Message::CancelDeleteAccount),
                button("Delete")
                    .style(iced::widget::button::danger)
                    .on_press(Message::ConfirmDeleteAccount(account.id))
            ]
            .spacing(10)
        ]
        .spacing(10),
    )
    .padding(10)
    .width(520)
    .style(delete_prompt_style);

    opaque(
        container(prompt)
            .padding(14)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
            .style(delete_prompt_scrim_style),
    )
}

fn account_card_style(theme: &Theme, selected: bool) -> iced::widget::container::Style {
    let mut style = iced::widget::container::bordered_box(theme);

    if selected {
        style.background = Some(Color::from_rgba8(30, 48, 67, 0.55).into());
        style.border.color = Color::from_rgb8(95, 176, 224);
    }

    style
}

fn select_account_button_style(
    theme: &Theme,
    status: iced::widget::button::Status,
    selected: bool,
) -> iced::widget::button::Style {
    if !selected {
        return iced::widget::button::primary(theme, status);
    }

    let mut style = iced::widget::button::secondary(theme, iced::widget::button::Status::Disabled);
    style.background = Some(Color::from_rgb8(68, 72, 78).into());
    style.text_color = Color::from_rgb8(180, 184, 190);
    style
}

fn account_menu_button_style(
    theme: &Theme,
    status: iced::widget::button::Status,
    is_open: bool,
) -> iced::widget::button::Style {
    if !is_open {
        return iced::widget::button::primary(theme, status);
    }

    iced::widget::button::secondary(theme, status)
}

fn delete_prompt_style(theme: &Theme) -> iced::widget::container::Style {
    let mut style = iced::widget::container::bordered_box(theme);
    style.background = Some(Color::from_rgba8(70, 28, 32, 0.55).into());
    style.border.color = Color::from_rgb8(214, 92, 92);
    style
}

fn delete_prompt_scrim_style(_: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(Color::from_rgba8(8, 10, 14, 0.68).into()),
        ..Default::default()
    }
}
