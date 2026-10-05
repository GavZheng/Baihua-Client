//! Shared off-screen scaffolding for the interface tests: one raw input frame, a
//! click event pair and the painted position of a text.

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

/// One off-screen frame on a `width` by `height` screen carrying `events`.
pub(crate) fn frame(width: f32, height: f32, events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height))),
        focused: true,
        events,
        ..Default::default()
    }
}

/// A press and a release at one point, which is what egui counts as a click.
pub(crate) fn click_at(point: Pos2) -> Vec<Event> {
    vec![
        Event::PointerButton {
            pos: point,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
        Event::PointerButton {
            pos: point,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    ]
}

/// The painted position of `needle`, panicking when the frame never drew it.
pub(crate) fn position_of(texts: &[(String, Pos2)], needle: &str) -> Pos2 {
    texts
        .iter()
        .find(|(text, _)| text == needle)
        .map(|(_, position)| *position)
        .unwrap_or_else(|| panic!("this frame did not draw {needle:?}; it drew {texts:?}"))
}
