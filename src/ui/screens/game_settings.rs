//! The Accounts tab's Game settings sub-tab: saved presets and each account's original settings.

use std::fmt;

use iced::widget::{Space, button, canvas, column, container, pick_list, row, text};
use iced::{
    Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, alignment, border, mouse,
};

use crate::account::AccountId;
use crate::game_settings::{
    CrosshairLines, CrosshairSummary, GameSettingsProfileMetadata, GameSettingsProfilePurpose,
    GameSettingsProfileSummary, Rgba, Setting,
};
use crate::ui::components::compact_loading_indicator;
use crate::ui::{Message, PrimeApp};

use super::accounts::last_refreshed_label;

const DETAIL_LABEL_WIDTH: f32 = 96.0;
/// How many keybinds a collapsed preset card shows; the rest are counted.
const SHORT_KEYBIND_LIMIT: usize = 3;
const CROSSHAIR_PREVIEW_SIZE: f32 = 88.0;
/// Screen pixels per VALORANT crosshair unit, before shrinking a large crosshair to fit.
const CROSSHAIR_PREVIEW_SCALE: f32 = 2.0;
const SWATCH_SIZE: f32 = 12.0;
const MUTED_TEXT: Color = Color::from_rgb(0.56, 0.59, 0.64);
const BRIGHT_TEXT: Color = Color::from_rgb(0.86, 0.88, 0.91);

pub(super) fn tab(app: &PrimeApp) -> Element<'_, Message> {
    let mut sections = column![presets_section(app)]
        .spacing(12)
        .width(Length::Fill);

    if let Some(restore) = restore_section(app) {
        sections = sections.push(restore);
    }

    sections.into()
}

/// An account as a picker lists it.
#[derive(Clone, Debug, PartialEq)]
struct AccountChoice {
    id: AccountId,
    label: String,
}

impl fmt::Display for AccountChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// Accounts whose VALORANT settings Prime can read and write.
fn settings_account_choices(app: &PrimeApp) -> Vec<AccountChoice> {
    app.state
        .accounts
        .iter()
        .filter(|account| account.has_launcher_session() || account.session.is_some())
        .map(|account| AccountChoice {
            id: account.id,
            label: account.display_name.clone(),
        })
        .collect()
}

fn settings_busy(app: &PrimeApp) -> bool {
    app.settings_work_in_progress()
}

fn presets_section(app: &PrimeApp) -> Element<'_, Message> {
    let busy = settings_busy(app);
    let accounts = settings_account_choices(app);
    // Picking while other settings work runs does nothing; the handler checks.
    let save_from = pick_list(accounts.clone(), None::<AccountChoice>, |choice| {
        Message::RequestSavePreset(choice.id)
    })
    .placeholder("Save preset from...")
    .width(220);

    let mut header = row![
        column![
            text("Presets").size(18),
            text("Saved VALORANT settings you can apply to any of your accounts.")
                .size(13)
                .color(MUTED_TEXT)
        ]
        .spacing(4)
        .width(Length::Fill)
    ]
    .spacing(10)
    .align_y(alignment::Vertical::Center);

    if let Some(account_id) = app.settings_saving_account {
        header = header.push(progress_label(app, account_id, "Saving"));
    }

    let mut presets = column![header.push(save_from)]
        .spacing(12)
        .width(Length::Fill);

    let saved = app
        .settings_profiles
        .iter()
        .filter(|profile| profile.purpose == GameSettingsProfilePurpose::Profile)
        .collect::<Vec<_>>();

    if saved.is_empty() {
        presets = presets.push(
            text("No presets yet. Pick an account above to save its settings as one.")
                .size(13)
                .color(MUTED_TEXT),
        );
    }

    for profile in saved {
        let expanded = app.expanded_presets.contains(&profile.id);
        presets = presets.push(preset_card(profile, &accounts, busy, expanded));
    }

    container(presets)
        .padding(14)
        .width(Length::Fill)
        .style(iced::widget::container::bordered_box)
        .into()
}

fn preset_card<'a>(
    profile: &'a GameSettingsProfileMetadata,
    accounts: &[AccountChoice],
    busy: bool,
    expanded: bool,
) -> Element<'a, Message> {
    let profile_id = profile.id.clone();
    let apply_to = pick_list(accounts.to_vec(), None::<AccountChoice>, move |choice| {
        Message::RequestApplyPreset {
            profile_id: profile_id.clone(),
            account_id: choice.id,
        }
    })
    .placeholder("Apply to...")
    .width(170);

    let header = row![
        column![
            text(&profile.name).size(17),
            text(format!(
                "From {} · Saved {}",
                profile.source_display_name,
                last_refreshed_label(Some(profile.captured_at_unix))
            ))
            .size(12)
            .color(MUTED_TEXT)
        ]
        .spacing(2)
        .width(Length::Fill),
        apply_to,
        button("Rename").on_press(Message::RequestRenamePreset(profile.id.clone())),
        button("Delete")
            .style(iced::widget::button::danger)
            .on_press_maybe(
                (!busy).then(|| Message::RequestDeleteSettingsProfile(profile.id.clone()))
            )
    ]
    .spacing(10)
    .align_y(alignment::Vertical::Center);

    let summary = &profile.summary;
    let body = row![
        crosshair_tile(summary.crosshair.as_ref()),
        settings_details(&profile.id, summary, expanded)
    ]
    .spacing(16)
    // Keeps the crosshair beside the summary rows when the full list opens below them.
    .align_y(if expanded {
        alignment::Vertical::Top
    } else {
        alignment::Vertical::Center
    });

    container(column![header, divider(), body].spacing(12))
        .padding(14)
        .width(Length::Fill)
        .style(preset_card_style)
        .into()
}

/// The preset's settings as labelled rows, and when expanded, every setting it copies. Missing
/// values are VALORANT's defaults.
fn settings_details<'a>(
    profile_id: &str,
    summary: &'a GameSettingsProfileSummary,
    expanded: bool,
) -> Element<'a, Message> {
    let mut details = column![
        detail_row("Sensitivity", sensitivity_value(summary)),
        detail_row("Crosshair", crosshair_value(summary)),
        detail_row("Keybinds", keybinds_value(summary)),
        detail_row(
            "Minimap",
            if summary.minimap.is_empty() {
                default_value()
            } else {
                text(summary.minimap.join(" · "))
                    .size(13)
                    .color(BRIGHT_TEXT)
                    .into()
            }
        ),
    ]
    .spacing(8)
    .width(Length::Fill);

    let listed =
        summary.keybinds.len() + summary.audio_settings.len() + summary.other_settings.len();
    if listed == 0 {
        return details.into();
    }

    let toggle_label = if expanded {
        "Show less".to_string()
    } else {
        format!("Show all settings ({listed})")
    };
    details = details.push(
        button(text(toggle_label).size(13))
            .style(iced::widget::button::text)
            .padding([2, 0])
            .on_press(Message::TogglePresetSettings(profile_id.to_string())),
    );

    if expanded {
        details = details.push(all_settings(summary));
    }

    details.into()
}

/// Every keybind and every other setting a preset copies, as labelled groups.
fn all_settings(summary: &GameSettingsProfileSummary) -> Element<'_, Message> {
    let groups = [
        (
            "All keybinds",
            summary
                .keybinds
                .iter()
                .map(|keybind| (keybind.action.as_str(), keybind.key.as_str()))
                .collect::<Vec<_>>(),
        ),
        ("Audio", setting_pairs(&summary.audio_settings)),
        ("Other", setting_pairs(&summary.other_settings)),
    ];

    let mut list = column![].spacing(12).width(Length::Fill);
    for (title, settings) in groups {
        if settings.is_empty() {
            continue;
        }

        let mut group = column![text(title.to_uppercase()).size(11).color(MUTED_TEXT)]
            .spacing(4)
            .width(Length::Fill);
        for (label, value) in settings {
            group = group.push(
                row![
                    text(label).size(13).width(Length::Fill),
                    text(value).size(13).color(BRIGHT_TEXT)
                ]
                .spacing(12),
            );
        }
        list = list.push(group);
    }

    list.into()
}

fn setting_pairs(settings: &[Setting]) -> Vec<(&str, &str)> {
    settings
        .iter()
        .map(|setting| (setting.label.as_str(), setting.value.as_str()))
        .collect()
}

fn detail_row<'a>(label: &'a str, value: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(label.to_uppercase())
            .size(11)
            .color(MUTED_TEXT)
            .width(DETAIL_LABEL_WIDTH),
        value
    ]
    .spacing(8)
    .align_y(alignment::Vertical::Center)
    .into()
}

fn default_value() -> Element<'static, Message> {
    text("Default").size(13).color(MUTED_TEXT).into()
}

fn sensitivity_value(summary: &GameSettingsProfileSummary) -> Element<'static, Message> {
    let parts = [
        summary
            .sensitivity
            .map(|value| chip(None, format_number(value))),
        summary
            .ads_multiplier
            .map(|value| chip(Some("ADS"), format!("{}×", format_number(value)))),
        summary
            .scoped_multiplier
            .map(|value| chip(Some("Scoped"), format!("{}×", format_number(value)))),
    ];

    if parts.iter().all(Option::is_none) {
        return default_value();
    }

    row(parts.into_iter().flatten())
        .spacing(6)
        .align_y(alignment::Vertical::Center)
        .into()
}

fn crosshair_value(summary: &GameSettingsProfileSummary) -> Element<'_, Message> {
    let Some(crosshair) = &summary.crosshair else {
        return default_value();
    };

    let mut value = row![
        color_swatch(crosshair.color),
        text(crosshair.color.label()).size(13).color(BRIGHT_TEXT)
    ]
    .spacing(6)
    .align_y(alignment::Vertical::Center);

    if let Some(name) = &crosshair.name {
        value = value
            .push(separator())
            .push(text(name).size(13).color(BRIGHT_TEXT));
    }

    if summary.crosshair_profile_count > 1 {
        value = value.push(separator()).push(
            text(format!(
                "{} saved crosshairs",
                summary.crosshair_profile_count
            ))
            .size(13)
            .color(MUTED_TEXT),
        );
    }

    value.into()
}

fn keybinds_value(summary: &GameSettingsProfileSummary) -> Element<'_, Message> {
    if summary.keybinds.is_empty() {
        return default_value();
    }

    let mut value = row![].spacing(12).align_y(alignment::Vertical::Center);

    for keybind in summary.keybinds.iter().take(SHORT_KEYBIND_LIMIT) {
        value = value.push(
            row![
                keycap(&keybind.key),
                text(&keybind.action).size(13).color(BRIGHT_TEXT)
            ]
            .spacing(6)
            .align_y(alignment::Vertical::Center),
        );
    }

    let hidden = summary.keybinds.len().saturating_sub(SHORT_KEYBIND_LIMIT);
    if hidden > 0 {
        value = value.push(text(format!("+{hidden} more")).size(13).color(MUTED_TEXT));
    }

    value.into()
}

/// A value with an optional small label, such as `ADS 0.6×`.
fn chip(label: Option<&'static str>, value: String) -> Element<'static, Message> {
    let mut content = row![].spacing(5).align_y(alignment::Vertical::Center);
    if let Some(label) = label {
        content = content.push(text(label).size(11).color(MUTED_TEXT));
    }
    content = content.push(text(value).size(13).color(BRIGHT_TEXT));

    container(content).padding([2, 8]).style(chip_style).into()
}

fn keycap(key: &str) -> Element<'_, Message> {
    container(text(key).size(12).color(BRIGHT_TEXT))
        .padding([1, 7])
        .style(keycap_style)
        .into()
}

fn color_swatch(color: Rgba) -> Element<'static, Message> {
    container(Space::new())
        .width(SWATCH_SIZE)
        .height(SWATCH_SIZE)
        .style(move |_| iced::widget::container::Style {
            background: Some(Color::from_rgb8(color.r, color.g, color.b).into()),
            border: border::rounded(3)
                .width(1)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.35)),
            ..Default::default()
        })
        .into()
}

fn separator() -> Element<'static, Message> {
    text("·").size(13).color(MUTED_TEXT).into()
}

fn divider() -> Element<'static, Message> {
    container(Space::new())
        .width(Length::Fill)
        .height(1)
        .style(|_| iced::widget::container::Style {
            background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.08).into()),
            ..Default::default()
        })
        .into()
}

/// `0.34`, `0.6` and `1` rather than `0.340` or `1.0`.
fn format_number(value: f64) -> String {
    let formatted = format!("{value:.3}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// The crosshair drawn on a grey tile, or VALORANT's default one when the preset has none.
fn crosshair_tile(crosshair: Option<&CrosshairSummary>) -> Element<'static, Message> {
    let crosshair = crosshair
        .cloned()
        .unwrap_or_else(CrosshairSummary::default_crosshair);

    container(
        canvas(CrosshairPreview { crosshair })
            .width(CROSSHAIR_PREVIEW_SIZE)
            .height(CROSSHAIR_PREVIEW_SIZE),
    )
    .clip(true)
    .style(crosshair_tile_style)
    .into()
}

/// Draws a crosshair onto the preview tile.
struct CrosshairPreview {
    crosshair: CrosshairSummary,
}

impl canvas::Program<Message> for CrosshairPreview {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        for (rect, color) in crosshair_rects(&self.crosshair, bounds.width.min(bounds.height)) {
            frame.fill_rectangle(
                Point::new(rect.x, rect.y),
                Size::new(rect.width, rect.height),
                color,
            );
        }

        vec![frame.into_geometry()]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Rect {
    fn grown(self, by: f32) -> Self {
        Self {
            x: self.x - by,
            y: self.y - by,
            width: self.width + by * 2.0,
            height: self.height + by * 2.0,
        }
    }

    /// Snapped to whole pixels so thin lines stay crisp.
    fn snapped(self) -> Self {
        Self {
            x: self.x.round(),
            y: self.y.round(),
            width: self.width.round().max(1.0),
            height: self.height.round().max(1.0),
        }
    }
}

/// The crosshair's pieces, outlines first, centred in a square of `size` pixels.
fn crosshair_rects(crosshair: &CrosshairSummary, size: f32) -> Vec<(Rect, Color)> {
    let center = size / 2.0;
    let extent = [crosshair.inner_lines, crosshair.outer_lines]
        .into_iter()
        .flatten()
        .map(|lines| lines.offset + lines.length.max(lines.vertical_length))
        .chain(crosshair.center_dot.map(|dot| dot.size / 2.0))
        .fold(0.0_f32, f32::max);
    // Large crosshairs shrink to fit, leaving room for the outline.
    let scale = if extent > 0.0 {
        CROSSHAIR_PREVIEW_SCALE.min((center - 4.0) / extent)
    } else {
        CROSSHAIR_PREVIEW_SCALE
    };
    let color = |opacity: f32| {
        Color::from_rgba8(
            crosshair.color.r,
            crosshair.color.g,
            crosshair.color.b,
            f32::from(crosshair.color.a) / 255.0 * opacity.clamp(0.0, 1.0),
        )
    };

    let mut pieces = Vec::new();
    for lines in [crosshair.inner_lines, crosshair.outer_lines]
        .into_iter()
        .flatten()
    {
        pieces.extend(
            line_rects(lines, center, scale)
                .into_iter()
                .map(|rect| (rect, color(lines.opacity))),
        );
    }
    if let Some(dot) = crosshair.center_dot {
        let side = dot.size * scale;
        pieces.push((
            Rect {
                x: center - side / 2.0,
                y: center - side / 2.0,
                width: side,
                height: side,
            },
            color(dot.opacity),
        ));
    }

    let mut rects = Vec::new();
    if let Some(outline) = crosshair.outline {
        let outline_color = Color::from_rgba(0.0, 0.0, 0.0, outline.opacity.clamp(0.0, 1.0));
        rects.extend(pieces.iter().map(|(rect, _)| {
            (
                rect.grown(outline.thickness * scale).snapped(),
                outline_color,
            )
        }));
    }
    rects.extend(
        pieces
            .into_iter()
            .map(|(rect, color)| (rect.snapped(), color)),
    );

    rects
}

/// The four arms of one set of lines: right, left, down and up.
fn line_rects(lines: CrosshairLines, center: f32, scale: f32) -> Vec<Rect> {
    let thickness = lines.thickness * scale;
    let offset = lines.offset * scale;
    let length = lines.length * scale;
    let vertical_length = lines.vertical_length * scale;
    let mut rects = Vec::new();

    if length > 0.0 && thickness > 0.0 {
        let y = center - thickness / 2.0;
        rects.push(Rect {
            x: center + offset,
            y,
            width: length,
            height: thickness,
        });
        rects.push(Rect {
            x: center - offset - length,
            y,
            width: length,
            height: thickness,
        });
    }
    if vertical_length > 0.0 && thickness > 0.0 {
        let x = center - thickness / 2.0;
        rects.push(Rect {
            x,
            y: center + offset,
            width: thickness,
            height: vertical_length,
        });
        rects.push(Rect {
            x,
            y: center - offset - vertical_length,
            width: thickness,
            height: vertical_length,
        });
    }

    rects
}

/// Accounts with their own settings put aside by an apply, each with Restore and Discard. Hidden
/// when there are none.
fn restore_section(app: &PrimeApp) -> Option<Element<'_, Message>> {
    let originals = original_settings(app);
    if originals.is_empty() {
        return None;
    }

    let busy = settings_busy(app);
    let mut rows = column![
        text("Restore").size(18),
        text(
            "Applying a preset puts the account's own settings aside first. Restore puts them back."
        )
        .size(13)
        .color(MUTED_TEXT)
    ]
    .spacing(10)
    .width(Length::Fill);

    for original in originals {
        let account = app
            .state
            .accounts
            .iter()
            .find(|account| account.id == original.source_account_id);
        let name = account.map_or(original.source_display_name.as_str(), |account| {
            account.display_name.as_str()
        });

        let mut line = row![
            column![
                text(format!("{name}'s own settings")).size(15),
                text(format!(
                    "Put aside {}",
                    last_refreshed_label(Some(original.captured_at_unix))
                ))
                .size(12)
                .color(MUTED_TEXT)
            ]
            .spacing(2)
            .width(Length::Fill)
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center);

        if app.settings_applying_account == Some(original.source_account_id) {
            line = line.push(progress_label(
                app,
                original.source_account_id,
                "Working on",
            ));
        }

        line = line
            .push(
                button("Restore").on_press_maybe(
                    (!busy && account.is_some())
                        .then_some(Message::RequestRestoreSettings(original.source_account_id)),
                ),
            )
            .push(
                button("Discard")
                    .style(iced::widget::button::danger)
                    .on_press_maybe(
                        (!busy).then(|| Message::RequestDeleteSettingsProfile(original.id.clone())),
                    ),
            );

        rows = rows.push(
            container(line)
                .padding(10)
                .width(Length::Fill)
                .style(preset_card_style),
        );
    }

    Some(
        container(rows)
            .padding(14)
            .width(Length::Fill)
            .style(iced::widget::container::bordered_box)
            .into(),
    )
}

/// Each account's original settings: its oldest backup.
pub(in crate::ui) fn original_settings(app: &PrimeApp) -> Vec<&GameSettingsProfileMetadata> {
    let mut originals: Vec<&GameSettingsProfileMetadata> = Vec::new();

    for backup in app
        .settings_profiles
        .iter()
        .filter(|profile| profile.purpose == GameSettingsProfilePurpose::Backup)
    {
        match originals
            .iter_mut()
            .find(|original| original.source_account_id == backup.source_account_id)
        {
            Some(original) if backup.captured_at_unix < original.captured_at_unix => {
                *original = backup;
            }
            Some(_) => {}
            None => originals.push(backup),
        }
    }

    originals
}

fn progress_label(app: &PrimeApp, account_id: AccountId, verb: &str) -> Element<'static, Message> {
    let name = app
        .state
        .accounts
        .iter()
        .find(|account| account.id == account_id)
        .map(|account| account.display_name.clone())
        .unwrap_or_default();

    row![
        compact_loading_indicator(app.loading_frame),
        text(format!("{verb} {name}...")).size(13)
    ]
    .spacing(6)
    .align_y(alignment::Vertical::Center)
    .into()
}

fn preset_card_style(theme: &Theme) -> iced::widget::container::Style {
    let mut style = iced::widget::container::bordered_box(theme);
    style.background = Some(Color::from_rgba8(255, 255, 255, 0.025).into());
    style.border = border::rounded(6)
        .width(1)
        .color(Color::from_rgba8(255, 255, 255, 0.10));
    style
}

fn crosshair_tile_style(_: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        // A mid grey, like a wall in game, so light and dark crosshairs both show.
        background: Some(Color::from_rgb8(92, 98, 106).into()),
        border: border::rounded(6)
            .width(1)
            .color(Color::from_rgba8(255, 255, 255, 0.12)),
        ..Default::default()
    }
}

fn chip_style(_: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(Color::from_rgba8(255, 255, 255, 0.06).into()),
        border: border::rounded(4)
            .width(1)
            .color(Color::from_rgba8(255, 255, 255, 0.12)),
        ..Default::default()
    }
}

fn keycap_style(_: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(Color::from_rgb8(48, 52, 59).into()),
        border: border::rounded(4)
            .width(1)
            .color(Color::from_rgb8(96, 102, 112)),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_settings::{CrosshairCenterDot, CrosshairOutline};

    fn plus(outline: Option<CrosshairOutline>) -> CrosshairSummary {
        CrosshairSummary {
            name: None,
            color: Rgba::WHITE,
            outline,
            center_dot: None,
            inner_lines: Some(CrosshairLines {
                length: 4.0,
                vertical_length: 3.0,
                thickness: 2.0,
                offset: 1.0,
                opacity: 1.0,
            }),
            outer_lines: None,
        }
    }

    #[test]
    fn formats_numbers_without_trailing_zeros() {
        assert_eq!(format_number(0.34), "0.34");
        assert_eq!(format_number(0.6), "0.6");
        assert_eq!(format_number(1.0), "1");
    }

    #[test]
    fn draws_the_four_arms_around_the_centre() {
        let rects = crosshair_rects(&plus(None), 88.0)
            .into_iter()
            .map(|(rect, _)| rect)
            .collect::<Vec<_>>();

        assert_eq!(
            rects,
            [
                // Right and left: 4 long, 2 thick, 1 from the centre, at 2x.
                Rect {
                    x: 46.0,
                    y: 42.0,
                    width: 8.0,
                    height: 4.0,
                },
                Rect {
                    x: 34.0,
                    y: 42.0,
                    width: 8.0,
                    height: 4.0,
                },
                // Down and up use the vertical length.
                Rect {
                    x: 42.0,
                    y: 46.0,
                    width: 4.0,
                    height: 6.0,
                },
                Rect {
                    x: 42.0,
                    y: 36.0,
                    width: 4.0,
                    height: 6.0,
                },
            ]
        );
    }

    #[test]
    fn draws_outlines_under_the_lines() {
        let rects = crosshair_rects(
            &plus(Some(CrosshairOutline {
                thickness: 1.0,
                opacity: 0.5,
            })),
            88.0,
        );

        assert_eq!(rects.len(), 8);
        assert_eq!(rects[0].1, Color::from_rgba(0.0, 0.0, 0.0, 0.5));
        assert_eq!(
            rects[0].0,
            Rect {
                x: 44.0,
                y: 40.0,
                width: 12.0,
                height: 8.0,
            }
        );
        assert_eq!(rects[4].1, Color::WHITE);
    }

    #[test]
    fn a_large_crosshair_shrinks_to_fit() {
        let mut crosshair = plus(None);
        crosshair.inner_lines = Some(CrosshairLines {
            length: 20.0,
            vertical_length: 20.0,
            thickness: 2.0,
            offset: 20.0,
            opacity: 1.0,
        });
        crosshair.center_dot = Some(CrosshairCenterDot {
            size: 4.0,
            opacity: 1.0,
        });

        for (rect, _) in crosshair_rects(&crosshair, 88.0) {
            assert!(rect.x >= 0.0 && rect.y >= 0.0, "{rect:?}");
            assert!(
                rect.x + rect.width <= 88.0 && rect.y + rect.height <= 88.0,
                "{rect:?}"
            );
        }
    }
}
