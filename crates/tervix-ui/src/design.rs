//! Shared presentation tokens.
use iced::{
    Border, Color, Shadow, Theme, Vector,
    widget::{button, container},
};

pub(super) const TEXT: Color = Color::from_rgb(238.0 / 255.0, 244.0 / 255.0, 252.0 / 255.0);
pub(super) const MUTED: Color = Color::from_rgb(190.0 / 255.0, 205.0 / 255.0, 224.0 / 255.0);
pub(super) const TEAL: Color = Color::from_rgb(83.0 / 255.0, 215.0 / 255.0, 184.0 / 255.0);
pub(super) const VIOLET: Color = Color::from_rgb(190.0 / 255.0, 174.0 / 255.0, 1.0);
pub(super) const BLUE: Color = Color::from_rgb(150.0 / 255.0, 195.0 / 255.0, 1.0);
pub(super) const CANVAS: Color = Color::from_rgb(38.0 / 255.0, 51.0 / 255.0, 71.0 / 255.0);
pub(super) const PANE: Color = Color::from_rgb(46.0 / 255.0, 61.0 / 255.0, 82.0 / 255.0);
pub(super) const SURFACE: Color = Color::from_rgb(55.0 / 255.0, 72.0 / 255.0, 94.0 / 255.0);
pub(super) const BORDER: Color = Color::from_rgb(111.0 / 255.0, 135.0 / 255.0, 165.0 / 255.0);
pub(super) const DANGER: Color = Color::from_rgb(1.0, 155.0 / 255.0, 171.0 / 255.0);
pub(super) const SCRIM: Color = Color::from_rgba(30.0 / 255.0, 42.0 / 255.0, 61.0 / 255.0, 0.62);
// Keep in sync with the adapter's NamedColor::Background, not ANSI black.
pub(super) const TERMINAL_BACKGROUND: [u8; 3] = [40, 54, 74];

pub(super) fn surface(_: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE.into()),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..Default::default()
    }
}

pub(super) fn control(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = button::secondary(theme, status);
    style.background = Some(
        Color::from_rgba8(
            111,
            140,
            192,
            if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                0.28
            } else {
                0.12
            },
        )
        .into(),
    );
    style.text_color = TEXT;
    style.border = Border {
        color: BORDER,
        width: 1.0,
        radius: 8.0.into(),
    };
    style
}

pub(super) fn shadow() -> Shadow {
    Shadow {
        color: Color::from_rgba8(0, 0, 0, 0.24),
        offset: Vector::new(0.0, 4.0),
        blur_radius: 14.0,
    }
}

pub(super) fn danger(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = control(theme, status);
    style.background = Some(Color::from_rgb8(114, 49, 67).into());
    style.border.color = DANGER;
    if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        style.background = Some(Color::from_rgb8(135, 58, 77).into());
    }
    style
}

pub(super) fn selected(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = control(theme, status);
    style.border.color = TEAL;
    style
}

pub(super) fn picker(
    _: &Theme,
    status: iced::widget::pick_list::Status,
) -> iced::widget::pick_list::Style {
    iced::widget::pick_list::Style {
        text_color: TEXT,
        placeholder_color: MUTED,
        handle_color: TEAL,
        background: SURFACE.into(),
        border: Border {
            color: if matches!(status, iced::widget::pick_list::Status::Active) {
                BORDER
            } else {
                TEAL
            },
            width: 1.0,
            radius: 7.0.into(),
        },
    }
}
