//! Shared layout metrics, widget identifiers and the pure decision rules several
//! panels depend on (narrow layout, sidebar visibility, scroll bookkeeping).

use super::*;

/// Focus identifier for the message input box: the input box itself and the check "is focus on it" share this single source
pub(crate) fn message_input_id() -> &'static str {
    "message-input"
}

/// Identifier of the input area panel at the bottom of the message area; it
/// reserves its slot before the messages are laid out.
pub(crate) fn input_area_id() -> &'static str {
    "conversation-input"
}

/// The text on the plus button to the right of the room list title
pub(crate) fn create_button_text() -> &'static str {
    "+"
}

/// Button text for the settings entry (the gear to the right of the room title)
pub(crate) fn settings_button_text() -> &'static str {
    "\u{2699}"
}

/// Width range of the room list panel. The lower bound matters: egui panels
/// remember their width, so a squeezed window would keep it forever.
pub(crate) fn room_panel_range() -> std::ops::RangeInclusive<f32> {
    160.0..=360.0
}

/// Settings window identifier (window position and collapsed state are remembered by it)
pub(crate) fn settings_window_id() -> &'static str {
    "settings-window"
}

/// Text of the button that opens the group settings sidebar: three ordinary
/// periods, because the interface must not use emoji or typographic characters.
pub(crate) fn sidebar_button_text() -> &'static str {
    "..."
}

/// Identifier of the group settings sidebar panel: it is the panel's memory key,
/// holding the dragged width and driving the slide animation.
pub(crate) fn sidebar_panel_id() -> &'static str {
    "group-settings"
}

/// Width the sidebar opens at (points): enough for "username (role, presence)"
/// plus its removal button, narrow enough to leave the message area usable.
pub(crate) fn sidebar_panel_width() -> f32 {
    260.0
}

/// Identifier of the search panel window, which remembers its position and
/// collapsed state like the settings and creation windows.
pub(crate) fn search_window_id() -> &'static str {
    "search-panel-window"
}

/// Stable id of the search panel keyword box (focus is requested on it by id).
pub(crate) fn search_keyword_id() -> &'static str {
    "search-panel-keyword-input"
}

/// Width the search panel opens at (points): enough for a full message row the
/// way the conversation shows it. The window stays resizable.
pub(crate) fn search_panel_width() -> f32 {
    380.0
}

/// Height the search panel opens at (points); only the starting size, the person resizes from here.
pub(crate) fn search_panel_height() -> f32 {
    320.0
}

/// Space a button keeps above and below its content (points). Buttons are sized per
/// call site: the shared style also moves panels and rows, which misaligned the areas.
pub(crate) fn button_inner_padding() -> f32 {
    8.0
}

/// Height of every button (points): one line of body text plus the button padding,
/// which is how tall the plus button beside the room list title is.
pub(crate) fn button_line_height(ui: &Ui) -> f32 {
    ui.text_style_height(&TextStyle::Body) + 2.0 * button_inner_padding()
}

/// Width and height of a square icon-only button, equal to any text button's height.
pub(crate) fn button_minimum_size(ui: &Ui) -> Vec2 {
    Vec2::splat(button_line_height(ui))
}

/// Height of one room list button (points): every row takes exactly this, so the
/// list is a stack of equal-size buttons instead of text-sized labels.
pub(crate) fn room_row_height() -> f32 {
    48.0
}

/// Corner radius of the room list, group settings and message area backgrounds.
pub(crate) fn panel_corner() -> f32 {
    12.0
}

/// Blank gap around those backgrounds (points): without it the rounded corners
/// would sit under the window edge and read as square again.
pub(crate) fn panel_inset() -> i8 {
    10
}

/// Points of system furniture above the app's own top bar this frame: the measured
/// status-bar overlap on Android, the safe-area inset on iOS, zero on desktops.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn mobile_top_inset(context: &Context) -> f32 {
    #[cfg(target_os = "android")]
    {
        crate::android_platform::overlap_points(context)
    }
    #[cfg(target_os = "ios")]
    {
        (context.content_rect().top() - context.viewport_rect().top()).max(0.0)
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = context;
        0.0
    }
}

/// The blank row that keeps the app's own top bar below the system status bar on
/// phones; the per-frame measurement means an inset window draws nothing.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn draw_status_spacer(ui: &mut Ui, skin: &Skin) {
    let inset_points = mobile_top_inset(ui.ctx());
    if inset_points <= 0.0 {
        return;
    }
    egui::Panel::top("mobile-status-bar-spacer")
        .resizable(false)
        .exact_size(inset_points)
        .frame(
            Frame::new()
                .fill(skin.app_background)
                .inner_margin(Margin::ZERO),
        )
        .show(ui, |_ui| {});
}

/// Window width (points) below which the narrow layout draws one area at a time,
/// because the room list and a usable message area stop fitting side by side.
pub(crate) fn narrow_threshold() -> f32 {
    720.0
}

/// Whether a window this wide must draw one layer at a time (room list,
/// conversation or sidebar) instead of the side-by-side layout.
pub(crate) fn using_narrow_layout(window_width: f32) -> bool {
    window_width < narrow_threshold()
}

/// Whether the current window must draw one layer at a time (viewport width,
/// shared by all three call sites).
pub(crate) fn window_is_narrow(context: &Context) -> bool {
    using_narrow_layout(context.viewport_rect().width())
}

/// Text of the narrow-layout back button: a plain capital "X", the counterpart of
/// the "..." button at the other end of the same title row.
pub(crate) fn back_button_text() -> &'static str {
    "X"
}

/// Which whole-window layer the narrow layout shows this frame; each back
/// button walks one layer down and each upper layer covers the one below.
pub(crate) enum NarrowLayer {
    /// Only the room list — also the state right after login, because no room is picked by default
    RoomList,
    /// Only the conversation (message area plus input), the room list is hidden
    Conversation,
    /// Only the group settings sidebar, covering the conversation
    GroupSettings,
}

/// Narrow-layout layer routing: no selection is the room list, a selection
/// opens the conversation, the sidebar flag covers it (never without a room).
pub(crate) fn resolve_narrow_layer(room_selected: bool, group_settings_open: bool) -> NarrowLayer {
    if !room_selected {
        return NarrowLayer::RoomList;
    }
    if group_settings_open {
        return NarrowLayer::GroupSettings;
    }
    NarrowLayer::Conversation
}

/// Default size of the settings window (points)
pub(crate) fn settings_width() -> f32 {
    460.0
}

pub(crate) fn settings_height() -> f32 {
    560.0
}

/// How far the settings window is from the top-left corner of the screen by default (points)
pub(crate) fn settings_offset() -> f32 {
    140.0
}

/// Width of the centered sign-in form for one screen width: the comfortable reading
/// width on desktops, shrunk on phones so frame margins and a gutter still fit.
pub(crate) fn auth_form_width(screen_width_points: f32) -> f32 {
    let comfortable_width: f32 = 420.0;
    let frame_margins: f32 = 16.0 * 2.0 + 24.0;
    (comfortable_width)
        .min(screen_width_points - frame_margins)
        .max(200.0)
}

/// Whether the server address field and its confirm button sit side by side: they
/// do on a desktop-wide form and stack vertically once it narrows to phone size.
pub(crate) fn auth_row_inline(form_width_points: f32) -> bool {
    let side_by_side_minimum = 360.0;
    form_width_points >= side_by_side_minimum
}

/// Maximum height of the centered sign-in form: whatever does not fit becomes a
/// scrollable area, so the modal never grows past the screen or the keyboard.
pub(crate) fn auth_form_max_height(screen_height_points: f32) -> f32 {
    let frame_margins: f32 = 16.0 * 2.0 + 48.0;
    (screen_height_points - frame_margins).max(200.0)
}

/// The shared frame of every overlay window: theme background and stroke plus a
/// small corner radius, instead of each window drawing its own right angles.
pub(crate) fn overlay_window_frame(skin: &Skin) -> Frame {
    Frame::new()
        .fill(skin.app_background)
        .stroke(Stroke::new(1.0, skin.overlay_border))
        .corner_radius(overlay_radius())
        .inner_margin(Margin::same(10))
}

/// Corner radius of the overlay windows (points), tighter than a window's own.
pub(crate) fn overlay_radius() -> f32 {
    6.0
}

/// Seconds one notice popup takes to glide in from the right or out to the right.
pub(crate) fn notice_glide_seconds() -> f32 {
    0.25
}

/// Extra distance (points) a gliding notice popup travels past its own width, so
/// it starts and ends fully clear of the screen edge.
pub(crate) fn notice_glide_margin() -> f32 {
    24.0
}

/// Diameter (points) of the red dot drawn before an unseen notice popup title.
pub(crate) fn notice_dot_side() -> f32 {
    8.0
}

/// Vertical gap (points) kept between two stacked notice popups.
pub(crate) fn notice_stack_gap() -> f32 {
    8.0
}

/// Height (points) of the notice stack's resting anchor below the top bar.
pub(crate) fn notice_stack_first() -> f32 {
    34.0
}

/// Distance (points) a resting notice popup keeps from the right screen edge.
pub(crate) fn notice_edge_inset() -> f32 {
    12.0
}

/// Side of the notice window's close control (points): smaller than a full icon
/// button so the corner overlay does not bury the first notice line.
pub(crate) fn notice_close_side() -> f32 {
    22.0
}

/// Default width of the room list panel, kept separate so the squeeze recovery
/// can be tested on its own.
pub(crate) fn room_panel_size() -> f32 {
    220.0
}

/// The frame the room list wears, shared by the wide layout's resizable left
/// panel and the narrow layout's full-width central panel.
pub(crate) fn room_panel_frame(skin: &Skin) -> Frame {
    panel_background(skin, skin.room_border, true, false).inner_margin(Margin::same(8))
}

/// The message area while the sidebar shares its right seam: both facing edges
/// stay square and flush, the way the room list already meets it on the left.
pub(crate) fn chat_area_frame(skin: &Skin, border: Color32, sidebar_open: bool) -> Frame {
    panel_background(skin, border, false, !sidebar_open).inner_margin(Margin::same(10))
}

/// The frame the group settings sidebar wears as a top-layer right panel: rounded
/// and inset toward the window edge, square toward the chat area it sits beside.
pub(crate) fn sidebar_frame(skin: &Skin) -> Frame {
    panel_background(skin, skin.room_border, false, true).inner_margin(Margin::same(8))
}

/// A background that owns the whole window (the narrow layout draws one area at a
/// time), so all four corners are rounded.
pub(crate) fn window_layer_frame(skin: &Skin, border: Color32) -> Frame {
    panel_background(skin, border, true, true).inner_margin(Margin::same(10))
}

/// The shared part of every panel background: fill, border and the corner set plus
/// gap that belong to the same pair of flags.
fn panel_background(skin: &Skin, border: Color32, open_left: bool, open_right: bool) -> Frame {
    let (radius, gap) = panel_shape(open_left, open_right);
    Frame::new()
        .fill(skin.app_background)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(radius)
        .outer_margin(gap)
}

/// Shape of a panel background: sides facing a window edge get radius and inset,
/// sides facing another panel stay square and flush, so the two areas meet exactly.
pub(crate) fn panel_shape(open_left: bool, open_right: bool) -> (CornerRadius, Margin) {
    let radius = panel_corner() as u8;
    let inset = panel_inset();
    let left = if open_left { radius } else { 0 };
    let right = if open_right { radius } else { 0 };
    (
        CornerRadius {
            nw: left,
            ne: right,
            sw: left,
            se: right,
        },
        Margin {
            left: if open_left { inset } else { 0 },
            right: if open_right { inset } else { 0 },
            top: inset,
            bottom: inset,
        },
    )
}

pub(crate) fn room_panel(skin: &Skin) -> egui::Panel {
    egui::Panel::left("room-list")
        .resizable(true)
        .size_range(room_panel_range())
        .default_size(room_panel_size())
        .frame(room_panel_frame(skin))
}

/// Height of the message input box (points), used both to draw it and to reserve
/// its space so the two can never disagree.
pub(crate) fn message_input_height() -> f32 {
    48.0
}

/// Width the send button reserves beside the message box (points), taken before the
/// text box is laid out or the row overflows and the box border leaves the panel.
pub(crate) fn send_button_width() -> f32 {
    104.0
}

/// Narrowest the message box may be squeezed to (points), since the button slot is
/// subtracted from the row width; the floor matches egui's smallest text edit.
pub(crate) fn minimum_input_width() -> f32 {
    24.0
}

/// How tall the input completion tooltip occupies (points): commands are listed in full; if it exceeds this height, scroll inside
pub(crate) fn completion_height() -> f32 {
    132.0
}

/// Gap left between the completion tooltip and the input box (points)
pub(crate) fn completion_gap() -> f32 {
    4.0
}

/// The identifier for the completion popup floating layer (it's a floating layer that doesn't take layout space; scroll position is remembered by this identifier)
pub(crate) fn completion_area_id() -> &'static str {
    "command-completions"
}

/// Margin (points) for "the message area reached the top": an offset at or below
/// it counts, because a strict zero misses the scroll animation's rounding.
pub(crate) fn scroll_top_threshold() -> f32 {
    1.0
}

/// Whether the message area reached the top and may pull earlier messages. The
/// content must really overflow, or a short session would re-request in a loop.
pub(crate) fn scrolled_to_top(content_height: f32, viewport_height: f32, offset_y: f32) -> bool {
    content_height > viewport_height + scroll_top_threshold() && offset_y <= scroll_top_threshold()
}

/// Whether the cached member table describes the room now on screen (the
/// sidebar draws its roster from this cache; missing counts as no).
pub(crate) fn detail_matches_room(detail: Option<&RoomDetail>, room_id: Option<&str>) -> bool {
    match (detail, room_id) {
        (Some(detail), Some(room_id)) => detail.id == room_id,
        _ => false,
    }
}

/// Whether the sidebar stays open after this frame, first match wins: not a group
/// room, the toggle press, a stale roster, a blank click, then the panel state.
pub(crate) fn resolve_sidebar_open(
    room_is_group: bool,
    member_roster_ready: bool,
    was_open: bool,
    toggle_pressed: bool,
    blank_click: bool,
    panel_open: bool,
) -> bool {
    if !room_is_group {
        return false;
    }
    if toggle_pressed {
        return !was_open;
    }
    if was_open && !member_roster_ready {
        return false;
    }
    if blank_click {
        return false;
    }
    panel_open
}

/// Whether this frame's click landed on blank space, neither on the sidebar nor on
/// its toggle; a swipe never counts, and a sliding panel reports only its visible part.
pub(crate) fn sidebar_blank_click(
    context: &Context,
    sidebar_rect: Rect,
    toggle_button_rect: Rect,
) -> bool {
    context.input(|state| {
        state.pointer.primary_clicked()
            && state.pointer.interact_pos().is_some_and(|position| {
                !sidebar_rect.contains(position) && !toggle_button_rect.contains(position)
            })
    })
}

/// Whether the scroll-to-message request may be cleared, which is only true once a
/// frame really handed it to the scroll region (search switches are written later).
pub(crate) fn scroll_consumed(
    consumed_this_frame: Option<&str>,
    pending_after_this_frame: Option<&str>,
) -> bool {
    consumed_this_frame.is_some() && consumed_this_frame == pending_after_this_frame
}

/// Top and bottom padding of the input area panel, matching the panel frame's
/// `Margin::symmetric(0, 4)` plus a little for the separator line.
pub(crate) fn input_padding() -> f32 {
    8.0 + 1.0
}

/// Initial height of the input area panel before it remembers its own: one input
/// row plus padding. Too large leaves a gap, too small self-corrects next frame.
pub(crate) fn input_initial_height(ui: &Ui) -> f32 {
    let _ = ui;
    message_input_height() + input_padding()
}

#[cfg(test)]
mod message_scroll_tests {
    use super::{scroll_consumed, scrolled_to_top};

    /// The content must really exceed the visible height and the offset must touch
    /// the top; a short session sits at offset 0 forever and must not auto-page.
    #[test]
    fn top_needs_overflow() {
        // Long session scrolled to the top: touching top
        assert!(scrolled_to_top(2000.0, 400.0, 0.0));
        // Long session still stopped in the middle: not touching top
        assert!(!scrolled_to_top(2000.0, 400.0, 120.0));
        // Short session has no room to scroll at all: not touching top (otherwise it would auto-page every frame)
        assert!(!scrolled_to_top(120.0, 400.0, 0.0));
    }

    /// A scroll request is cleared only once a frame really handed it to the scroll
    /// region; a search match written after drawing has to wait for the next one.
    #[test]
    fn scroll_request_lives() {
        // This frame used a, and after drawing it's still a: already consumed, clear it
        assert!(scroll_consumed(Some("a"), Some("a")));
        // This frame has no target, after drawing new b was written (search match change): wait until next frame
        assert!(!scroll_consumed(None, Some("b")));
        // This frame used a, but after drawing it switched to b (arrow key pressed in succession): the new one must be kept
        assert!(!scroll_consumed(Some("a"), Some("b")));
    }
}

#[cfg(test)]
mod panel_size_tests {
    use super::{room_panel, room_panel_range, room_panel_size};
    use crate::appearance::Skin;
    use baihua_core::config::Palette;
    use egui::{CentralPanel, Context, Pos2, RawInput, Rect, Vec2};

    fn raw_input(width: f32) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 400.0))),
            ..Default::default()
        }
    }

    /// Draw a room list panel for one frame, return its width
    fn room_width_frame(context: &Context, screen_width: f32) -> f32 {
        let skin = Skin::from(&Palette::built_in());
        let mut expanded = true;
        let mut width = 0.0;
        context
            .run_ui(raw_input(screen_width), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    let response = room_panel(&skin).show_collapsible(ui, &mut expanded, |ui| {
                        ui.label("rooms");
                    });
                    if let Some(response) = response {
                        width = response.response.rect.width();
                    }
                });
            })
            .drop_without_applying_deltas();
        width
    }

    /// After the window is squeezed and widened again the panel must return to at
    /// least the minimum width (see `room_panel_range`).
    #[test]
    fn panel_recovers_width() {
        let range = room_panel_range();
        assert!(
            *range.start() > 0.0 && *range.end() > *range.start(),
            "the sidebar must offer a positive minimum width and a larger maximum, got {range:?}"
        );
        let context = Context::default();
        let squeezed = room_width_frame(&context, 200.0);
        let restored = room_width_frame(&context, 1200.0);
        let minimum = *room_panel_range().start();
        assert!(
            squeezed < room_panel_size(),
            "a narrow window must actually have squeezed it, width {squeezed}"
        );
        assert!(
            restored >= minimum,
            "after widening the window it must recover above the minimum {minimum}, width {restored}"
        );
    }
}

#[cfg(test)]
mod narrow_layout_tests {
    use super::{
        NarrowLayer, back_button_text, narrow_threshold, resolve_narrow_layer, using_narrow_layout,
    };

    /// The narrow threshold must sit above the window's minimum width or the
    /// single-layer mode could never be reached.
    #[test]
    fn narrow_switch() {
        let threshold = narrow_threshold();
        assert!(
            threshold > 640.0,
            "the window's minimum width must fall inside the narrow layout or it could never switch"
        );
        assert!(
            using_narrow_layout(threshold - 1.0),
            "below the threshold use the narrow layout"
        );
        assert!(
            !using_narrow_layout(threshold + 1.0),
            "above the threshold keep the side-by-side layout"
        );
    }

    /// One layer at a time: no selection shows the room list, a selection shows the
    /// conversation, the sidebar flag covers it, and the back button is a plain letter.
    #[test]
    fn narrow_layer_stack() {
        assert!(
            matches!(resolve_narrow_layer(false, false), NarrowLayer::RoomList),
            "with no room selected show only the session list (the default state after login)"
        );
        assert!(
            matches!(resolve_narrow_layer(false, true), NarrowLayer::RoomList),
            "with no room selected the sidebar must not cover the session list"
        );
        assert!(
            matches!(resolve_narrow_layer(true, false), NarrowLayer::Conversation),
            "once a room is selected the message area owns the whole window"
        );
        assert!(
            matches!(resolve_narrow_layer(true, true), NarrowLayer::GroupSettings),
            "on the narrow layout the sidebar is a fully covering layer"
        );
        assert_eq!(
            back_button_text(),
            "X",
            "the back button must be a plain capital letter, not an emoji"
        );
    }
}

#[cfg(test)]
mod auth_form_size_tests {
    use super::{auth_form_max_height, auth_form_width};

    /// Phones get a narrower login form with a visible gutter (rounded corners
    /// never clip it); desktops keep the comfortable reading width.
    #[test]
    fn auth_width_follows() {
        let on_phone = auth_form_width(390.0);
        assert!(
            (0.0..390.0).contains(&on_phone),
            "a 390-point phone screen must get a form narrower than the screen, got {on_phone}"
        );
        assert_eq!(
            auth_form_width(1200.0),
            420.0,
            "wide screens keep the comfortable form width"
        );
    }

    /// The form caps below the screen height (the rest scrolls), both on tall
    /// phones and on short windows (soft keyboard up).
    #[test]
    fn auth_height_fits() {
        assert!(auth_form_max_height(844.0) < 844.0);
        let with_keyboard_up = auth_form_max_height(380.0);
        assert!(
            (200.0..380.0).contains(&with_keyboard_up),
            "a short window still gets a usable (scrollable) form, got {with_keyboard_up}"
        );
    }
}

#[cfg(test)]
mod panel_shape_tests {
    use super::{
        chat_area_frame, panel_corner, panel_inset, panel_shape, room_panel_frame, sidebar_frame,
        window_layer_frame,
    };
    use crate::appearance::Skin;
    use baihua_core::config::Palette;
    use egui::{CornerRadius, Margin};

    /// Only sides facing another panel lose rounding *and* gap: the room list keeps
    /// its left pair, the message area its right pair, and the two meet flush.
    #[test]
    fn seam_corners_square() {
        let corner = panel_corner() as u8;
        let inset = panel_inset();
        let (room, room_gap) = panel_shape(true, false);
        assert_eq!((room.nw, room.sw), (corner, corner));
        assert_eq!((room.ne, room.se), (0, 0));
        assert_eq!((room_gap.left, room_gap.right), (inset, 0));
        let (message, message_gap) = panel_shape(false, true);
        assert_eq!((message.ne, message.se), (corner, corner));
        assert_eq!((message.nw, message.sw), (0, 0));
        assert_eq!((message_gap.left, message_gap.right), (0, inset));
        let (alone, alone_gap) = panel_shape(true, true);
        assert_eq!(alone, CornerRadius::same(corner));
        assert_eq!(alone_gap, Margin::same(inset));
        assert_eq!((room_gap.top, room_gap.bottom), (inset, inset));
    }

    /// The two frames carry those corner sets and the inset, so what is actually
    /// drawn follows the rule instead of merely being described by it.
    #[test]
    fn frames_carry_corners() {
        let skin = Skin::from(&Palette::built_in());
        let inset = panel_inset();
        let room = room_panel_frame(&skin);
        assert_eq!(room.outer_margin.left, panel_inset());
        assert_eq!(room.outer_margin.right, 0);
        let message = chat_area_frame(&skin, skin.message_border, false);
        assert_eq!(message.outer_margin.left, 0);
        assert_eq!(message.outer_margin.right, panel_inset());
        let alone = window_layer_frame(&skin, skin.message_border);
        assert_eq!(alone.outer_margin, Margin::same(panel_inset()));
        // All three panels share one bottom line on the top layer, and a visible
        // sidebar closes the chat area's right seam flush instead of inset.
        let sidebar = sidebar_frame(&skin);
        assert_eq!(
            (sidebar.outer_margin.left, sidebar.outer_margin.right),
            (0, inset)
        );
        assert_eq!(sidebar.outer_margin.bottom, room.outer_margin.bottom);
        let shared = chat_area_frame(&skin, skin.message_border, true);
        assert_eq!(shared.outer_margin.right, 0);
        assert_eq!(shared.outer_margin.bottom, room.outer_margin.bottom);
    }
}
