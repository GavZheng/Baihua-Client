//! The message area: the title row, the message rows with avatars, timestamps and
//! search highlighting, the empty-state logo and the methods that draw it all.

use super::*;

/// The side length (in pixels) of the avatar block in the message area. The TUI uses half-characters to make 32×32; here we use pixel blocks of the same size.
pub(crate) fn avatar_side_pixels() -> usize {
    32
}

/// A laid-out message row (compute data first, then draw, to avoid simultaneously borrowing self in the interface closure).
/// Cloneable: tests need to feed the same rows repeatedly into the draw function (the real interface also computes a fresh one every frame).
#[derive(Clone)]
pub(crate) struct MessageRow {
    /// The primary key of the message itself: when a search match occurs, the interface uses it to scroll this row into view
    pub(crate) message_id: String,
    /// The sender's user ID (use this to look up when clicking their avatar for the profile; display names may duplicate or change, ID will not)
    pub(crate) sender_id: String,
    pub(crate) sender: String,
    pub(crate) content: String,
    pub(crate) time_text: String,
    pub(crate) is_own: bool,
    /// Bubble fill: the sender's own body colour from the theme (own or other)
    pub(crate) bubble_color: Color32,
    /// The highlight background color behind the text (only on search match; the two slots in the theme are inherently "background colors")
    pub(crate) content_highlight: Option<Color32>,
    pub(crate) texture: Option<egui::TextureHandle>,
}

/// Title row data: the back button, the room title and (group rooms only) the "..." and
/// magnifier buttons. The buttons lay out first, so a long name cannot push them out.
#[derive(Clone, Copy)]
pub(crate) struct TitleRowView<'a> {
    /// Whether this frame draws the narrow single-layer conversation (only then the "X" back
    /// button exists at all)
    pub(crate) narrow: bool,
    /// The room name shown at the left of the row (already includes the typing suffix)
    pub(crate) title: &'a str,
    /// Whether the room on screen is a group: both right-hand buttons follow this one rule
    pub(crate) group_room: bool,
    /// Hover text of the "X" back button
    pub(crate) back_hint: &'a str,
    /// Hover text of the "..." group settings button
    pub(crate) group_settings_hint: &'a str,
    /// Hover text of the magnifier button
    pub(crate) search_hint: &'a str,
}

pub(crate) fn draw_title_row(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    view: &TitleRowView<'_>,
) -> (bool, bool, bool, Rect) {
    let TitleRowView {
        narrow,
        title,
        group_room,
        back_hint,
        group_settings_hint,
        search_hint,
    } = *view;
    let mut close_selected_room = false;
    let mut toggle_group_settings = false;
    let mut toggle_search_panel = false;
    let mut group_settings_button_rect = Rect::NOTHING;
    ui.horizontal(|ui| {
        // The narrow conversation layer is a full-window page: the "X" in the top-left corner
        // walks back to the room list and cancels the selection.
        if narrow
            && icon_button(ui, skin, icons, IconName::Back, None)
                .on_hover_text(back_hint)
                .clicked()
        {
            close_selected_room = true;
        }
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            if group_room {
                let toggle = icon_button(ui, skin, icons, IconName::More, None)
                    .on_hover_text(group_settings_hint);
                group_settings_button_rect = toggle.rect;
                if toggle.clicked() {
                    toggle_group_settings = true;
                }
                if draw_search_button(ui, skin, icons, search_hint.to_owned()).clicked() {
                    toggle_search_panel = true;
                }
            }
            ui.add(egui::Label::new(RichText::new(title).color(skin.message_border)).truncate());
        });
    });
    (
        close_selected_room,
        toggle_group_settings,
        toggle_search_panel,
        group_settings_button_rect,
    )
}

/// Message list plus the bottom input area, returning (input result, clicked avatar id,
/// scrolled to top). The input panel reserves its height first, pinned to one row.
pub(crate) fn draw_conversation<R>(
    ui: &mut Ui,
    skin: &Skin,
    message_content: (Vec<MessageRow>, String, Option<TextureHandle>),
    stick_to_bottom: bool,
    scroll_to_message: Option<String>,
    input_on_top: bool,
    draw_input_area: impl FnOnce(&mut Ui) -> R,
) -> (R, Option<String>, bool) {
    let (rows, empty_hint, background_logo) = message_content;
    let had_rows = !rows.is_empty();
    let mut clicked_avatar: Option<String> = None;
    // The input area takes only the height it needs. On touch platforms it docks to the
    // top while the box owns the focus, because iOS never shrinks for the keyboard.
    let panel = if input_on_top {
        egui::Panel::top(input_area_id())
    } else {
        egui::Panel::bottom(input_area_id())
    };
    // `exact_size` (min == max), not `default_size`: an egui panel is grow-only, so a
    // long draft would grow it one line per frame and eat the message area to zero.
    let input_area = panel
        .resizable(false)
        .exact_size(input_initial_height(ui))
        .frame(
            Frame::new()
                .fill(skin.app_background)
                .inner_margin(Margin::symmetric(0, 4)),
        )
        .show(ui, |ui| {
            let layout = if input_on_top {
                egui::Layout::top_down(Align::Min)
            } else {
                egui::Layout::bottom_up(Align::Min)
            };
            ui.with_layout(layout, draw_input_area).inner
        })
        .inner;
    // With no group chat open the logo sits centered in the message region, painted
    // before the list so the rows always cover it.
    if let Some(handle) = background_logo {
        let region = ui.available_rect_before_wrap();
        let side = (region.width().min(region.height()) * 0.45).clamp(120.0, 320.0);
        let rect = Rect::from_center_size(region.center(), Vec2::splat(side));
        let tint = if had_rows {
            Color32::from_white_alpha(48)
        } else {
            Color32::WHITE
        };
        ui.painter().image(
            handle.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            tint,
        );
    }
    // Horizontal auto-shrink is OFF on purpose: with it on the width follows the
    // content, so one over-wide frame starts a feedback loop past the panel border.
    let scroll_output = ScrollArea::vertical()
        .stick_to_bottom(stick_to_bottom)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            draw_visible_rows(
                ui,
                rows,
                scroll_to_message.as_deref(),
                |ui, row, scroll| draw_message_row(ui, skin, row, scroll),
                row_height_scope(),
            )
            .into_iter()
            .for_each(|user_id| clicked_avatar = Some(user_id));
            if !had_rows {
                ui.colored_label(skin.hint_text, empty_hint);
            }
        });
    // Touch-the-top judgment uses the scroll region's own output: content height, visible height, and this frame's final offset
    let reached_top = scrolled_to_top(
        scroll_output.content_size.y,
        scroll_output.inner_rect.height(),
        scroll_output.state.offset.y,
    );
    (input_area, clicked_avatar, reached_top)
}

/// Draw the rows inside a `ScrollArea`, laying out only the visible ones plus one screen
/// of slack; a skipped row replays its remembered height so the scroll bar never jumps.
pub(crate) fn draw_visible_rows(
    ui: &mut Ui,
    rows: Vec<MessageRow>,
    scroll_to_message: Option<&str>,
    draw_row: impl FnMut(&mut Ui, MessageRow, bool) -> Option<String>,
    height_scope: &'static str,
) -> Vec<String> {
    let viewport = ui.clip_rect();
    let slack = viewport.height();
    let mut clicked: Vec<String> = Vec::new();
    let mut draw_row = draw_row;
    for row in rows {
        let row_top = ui.cursor().min.y;
        let remembered = remembered_height(ui, height_scope, &row.message_id);
        // Only a row that was measured on an earlier frame and now sits outside
        // the viewport (plus its slack) is skipped; anything else is laid out.
        let skip_reserving = remembered.filter(|height| {
            !row_needs_layout(
                row_top,
                row_top + height,
                viewport,
                slack,
                scroll_to_message == Some(row.message_id.as_str()),
            )
        });
        if let Some(height) = skip_reserving {
            // Reserve the height without extending the content sideways: the clip
            // rectangle spans the whole screen and would widen it on every skip.
            ui.advance_cursor_after_rect(Rect::from_min_size(
                Pos2::new(ui.cursor().min.x, row_top),
                Vec2::new(0.0, height),
            ));
            continue;
        }
        let before = ui.min_rect().bottom();
        if let Some(user_id) = draw_row(
            ui,
            row.clone(),
            scroll_to_message == Some(row.message_id.as_str()),
        ) {
            clicked.push(user_id);
        }
        let measured = ui.min_rect().bottom() - before;
        remember_row_height(ui, height_scope, &row.message_id, measured);
    }
    clicked
}

/// The height-cache scope of the conversation's own rows.
pub(crate) fn row_height_scope() -> &'static str {
    "conversation"
}

/// The height-cache scope of the search panel's blocks: the same rows inside a
/// bordered block are taller, so they must not share the conversation's cache.
pub(crate) fn search_height_scope() -> &'static str {
    "search-panel"
}

/// Whether a row spanning `row_top..row_bottom` must be laid out this frame; pure, so
/// the rule is testable without a window. A jump target is always laid out.
pub(crate) fn row_needs_layout(
    row_top: f32,
    row_bottom: f32,
    viewport: Rect,
    slack: f32,
    scroll_target: bool,
) -> bool {
    scroll_target || row_bottom >= viewport.top() - slack && row_top <= viewport.bottom() + slack
}

/// The id a row's remembered height is stored under; `height_scope` keeps the
/// conversation and the search panel from reserving each other's heights.
fn row_height_id(height_scope: &str, message_id: &str) -> Id {
    Id::new(("message-row-height", height_scope, message_id))
}

/// The height a row had the last time it was laid out, or None if it never was.
fn remembered_height(ui: &Ui, height_scope: &str, message_id: &str) -> Option<f32> {
    ui.ctx()
        .data(|data| data.get_temp::<f32>(row_height_id(height_scope, message_id)))
}

/// Remember how tall a row was, for the frames that skip laying it out.
fn remember_row_height(ui: &Ui, height_scope: &str, message_id: &str, height: f32) {
    ui.ctx()
        .data_mut(|data| data.insert_temp(row_height_id(height_scope, message_id), height));
}

/// Corner radius of a message bubble (points).
fn bubble_corner() -> f32 {
    12.0
}

/// Space between the bubble border and the message body (points).
fn bubble_padding() -> i8 {
    8
}

/// How far the bubble's triangle reaches toward the avatar (points); the row
/// reserves exactly this much blank space beside the bubble.
fn bubble_tail_width() -> f32 {
    8.0
}

/// Half of that triangle's base (points): keeping the base short makes it read as
/// a tail rather than a wedge.
fn bubble_tail_height() -> f32 {
    8.0
}

/// Where the tail sits along the bubble's edge, as a fraction of its height: the
/// middle, so the triangle and the rounded rectangle share one axis.
fn bubble_tail_center() -> f32 {
    0.5
}

/// Body colour inside a bubble: black on a light bubble, white on a dark one; a
/// search match keeps the colour that contrasts with its highlight background.
fn bubble_text_color(bubble: Color32, highlight: Option<Color32>) -> Color32 {
    match highlight {
        Some(background) => contrasting_text(background),
        None if is_light_background(bubble) => Color32::BLACK,
        None => Color32::WHITE,
    }
}

/// The triangle that points the bubble at its avatar: base on the bubble's avatar
/// side, tip in the reserved strip; `points_right` is true for own messages.
fn bubble_tail_shape(bubble: Rect, points_right: bool, fill: Color32) -> Shape {
    let half = bubble_tail_height();
    let center_y = bubble.min.y + bubble.height() * bubble_tail_center();
    let (base_x, tip_x) = if points_right {
        (bubble.max.x - 1.0, bubble.max.x + bubble_tail_width())
    } else {
        (bubble.min.x + 1.0, bubble.min.x - bubble_tail_width())
    };
    Shape::convex_polygon(
        vec![
            Pos2::new(tip_x, center_y),
            Pos2::new(base_x, center_y - half),
            Pos2::new(base_x, center_y + half),
        ],
        fill,
        Stroke::NONE,
    )
}

/// The message body wrapped in a bubble: a rounded rectangle filled with the
/// sender's own body colour, then the tail painted in the same colour on top.
fn draw_message_bubble(
    ui: &mut Ui,
    content: &str,
    bubble: Color32,
    highlight: Option<Color32>,
    points_right: bool,
) {
    let body_color = bubble_text_color(bubble, highlight);
    let framed = Frame::new()
        .fill(bubble)
        .corner_radius(bubble_corner())
        .inner_margin(Margin::same(bubble_padding()))
        .show(ui, |ui| {
            ui.add(content_label(ui, content, body_color, highlight));
        });
    let rect = framed.response.rect;
    if ui.is_rect_visible(rect) {
        ui.painter()
            .add(bubble_tail_shape(rect, points_right, bubble));
    }
}

/// One message row: own messages right-aligned, others left, which needs a
/// right-to-left layout inside every sub-block. Only the avatar is clickable.
pub(crate) fn draw_message_row(
    ui: &mut Ui,
    skin: &Skin,
    row: MessageRow,
    scroll_to_this_row: bool,
) -> Option<String> {
    // Destructure once: the nested layout closures move individual fields, so
    // borrowing the whole row anywhere would collide with those moves.
    let MessageRow {
        message_id: _message_id,
        sender_id,
        sender,
        content,
        time_text,
        is_own,
        bubble_color,
        content_highlight,
        texture,
    } = row;
    let row_layout = if is_own {
        egui::Layout::right_to_left(Align::Min)
    } else {
        egui::Layout::left_to_right(Align::Min)
    };
    let text_direction = if is_own {
        egui::Layout::right_to_left(Align::Min)
    } else {
        egui::Layout::left_to_right(Align::Min)
    };
    // The entire row fills the available width; only then is there "rightmost" to stick to when laid out right-to-left
    let row_width = ui.available_width();
    let mut avatar_clicked = false;
    let row_area = ui.allocate_ui_with_layout(Vec2::new(row_width, 0.0), row_layout, |ui| {
        let avatar = match texture {
            Some(handle) => ui.add(
                egui::Image::from_texture(&handle)
                    .fit_to_exact_size(Vec2::splat(avatar_side_pixels() as f32))
                    .sense(Sense::click()),
            ),
            None => {
                draw_placeholder(ui, &sender, avatar_side_pixels() as f32).interact(Sense::click())
            }
        };
        // Hand cursor makes it visible the avatar is clickable
        avatar_clicked = avatar
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked();
        // The strip between the avatar and the text block is where the bubble's
        // triangle lives, so the tail can never overlap either of them.
        ui.add_space(bubble_tail_width());
        ui.with_layout(text_direction, |ui| {
            ui.vertical(|ui| {
                draw_sender_line(ui, skin, &sender, &time_text, is_own);
                ui.with_layout(text_direction, |ui| {
                    draw_message_bubble(ui, &content, bubble_color, content_highlight, is_own);
                });
            });
        });
    });
    // When the search switches to this message, scroll it to the middle of the view (the positioning request is only valid this frame)
    if std::env::var("BAIHUA_LAYOUT_DEBUG").is_ok() {
        eprintln!(
            "row_width={row_width} row_rect={:?} scroll_rect={:?}",
            row_area.response.rect,
            ui.max_rect()
        );
    }
    if scroll_to_this_row {
        ui.scroll_to_rect(row_area.response.rect, Some(Align::Center));
    }
    if avatar_clicked {
        Some(sender_id)
    } else {
        None
    }
}

/// The sender line: display name (plus the uid suffix when asked for) and timestamp
/// inside a locked maximum width; a half that cannot show its "..." is dropped.
fn draw_sender_line(ui: &mut Ui, skin: &Skin, sender: &str, time_text: &str, is_own: bool) {
    let name_color = if is_own {
        skin.own_username_text
    } else {
        skin.other_username_text
    };
    let row_layout = if is_own {
        egui::Layout::right_to_left(Align::Min)
    } else {
        egui::Layout::left_to_right(Align::Min)
    };
    ui.with_layout(row_layout, |ui| {
        // The line's cap, then each half's share of it.
        let line_width = ui.available_width() * sender_line_fraction();
        let name_width = single_line_width(ui, sender);
        let time_width = single_line_width(ui, time_text);
        let ellipsis_width = single_line_width(ui, ellipsis_run_text());
        let (name_share, time_share) = sender_line_shares(
            line_width,
            name_width,
            time_width,
            ui.spacing().item_spacing.x,
            ellipsis_width,
        );
        // Both halves use the same elided label so both honour their width; `None`
        // means there was no room for even an ellipsis, so that half is not drawn.
        let alignment = ui.layout().horizontal_placement();
        if let Some(share) = name_share {
            ui.add(sender_line_label(ui, sender, name_color, share, alignment));
        }
        if let Some(share) = time_share {
            ui.add(sender_line_label(
                ui,
                time_text,
                skin.time_text,
                share,
                alignment,
            ));
        }
    });
}

/// The widths the two sender-line halves get, or `None` for one that must not be drawn:
/// both fit, the wider shrinks, both are halved and elided, then the line stays blank.
fn sender_line_shares(
    line_width: f32,
    name_natural_width: f32,
    time_natural_width: f32,
    gap: f32,
    ellipsis_width: f32,
) -> (Option<f32>, Option<f32>) {
    if line_width <= 0.0 || line_width < ellipsis_width {
        return (None, None);
    }
    if name_natural_width + gap + time_natural_width <= line_width {
        return (Some(name_natural_width), Some(time_natural_width));
    }
    // Step 2: the wider half takes whatever the narrower one leaves.
    let name_is_wider = name_natural_width > time_natural_width;
    let (wider_share, narrower_share) = if name_is_wider {
        (line_width - time_natural_width - gap, time_natural_width)
    } else {
        (line_width - name_natural_width - gap, name_natural_width)
    };
    if wider_share >= ellipsis_width {
        return if name_is_wider {
            (Some(wider_share), Some(narrower_share))
        } else {
            (Some(narrower_share), Some(wider_share))
        };
    }
    // Step 3: split the cap in half and elide both.
    let half_share = (line_width - gap) / 2.0;
    if half_share >= ellipsis_width {
        return (Some(half_share), Some(half_share));
    }
    // Step 4: no room even for one ellipsis per half: draw no text at all.
    (None, None)
}

/// The "..." an elided run shows; measuring this exact text tells the sender line
/// whether a half has room to be elided at all.
fn ellipsis_run_text() -> &'static str {
    "\u{2026}"
}

/// How much of a row's width the sender line may use: a fraction rather than all that
/// is left, or a long "name (uid)" collides with whatever sits past the row.
fn sender_line_fraction() -> f32 {
    0.75
}

/// One half of the sender line laid out ahead of time as one elided row of at most
/// `maximum_width`; pre-laying pins the truncation and the alignment at once.
fn sender_line_label(
    ui: &Ui,
    text: &str,
    text_color: Color32,
    maximum_width: f32,
    alignment: Align,
) -> egui::Label {
    let job = egui::WidgetText::from(RichText::new(text).color(text_color)).into_layout_job(
        ui.style(),
        egui::FontSelection::Default,
        Align::Center,
    );
    let mut job = std::sync::Arc::unwrap_or_clone(job);
    job.wrap.max_width = maximum_width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.halign = alignment;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    egui::Label::new(galley)
}

/// Width a single-line run of body text needs on this frame's font table, so the
/// sender line can reserve room for the timestamp before eliding the name.
fn single_line_width(ui: &Ui, text: &str) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
    );
    galley.size().x
}

/// The body of one message laid out ahead of time with `break_anywhere` on, at most
/// `maximum_width` wide: a plain label only wraps at spaces and overflowed the row.
fn build_message_galley(
    ui: &Ui,
    content: &str,
    content_color: Color32,
    content_highlight: Option<Color32>,
    maximum_width: f32,
    alignment: Align,
) -> std::sync::Arc<Galley> {
    let mut text = egui::RichText::new(content).color(content_color);
    if let Some(highlight) = content_highlight {
        text = text.background_color(highlight);
    }
    let mut job = std::sync::Arc::unwrap_or_clone(egui::WidgetText::from(text).into_layout_job(
        ui.style(),
        egui::FontSelection::Default,
        Align::Center,
    ));
    job.wrap.max_width = maximum_width;
    job.wrap.break_anywhere = true;
    job.halign = alignment;
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

/// The body of one message as a label, cached only inside egui's `Fonts` layout cache:
/// a galley bakes the atlas glyph uv, so a private cache across an atlas rebuild paints garbage.
pub(crate) fn content_label(
    ui: &Ui,
    content: &str,
    content_color: Color32,
    content_highlight: Option<Color32>,
) -> egui::Label {
    let maximum_width = ui.available_width();
    let alignment = ui.layout().horizontal_placement();
    let galley = build_message_galley(
        ui,
        content,
        content_color,
        content_highlight,
        maximum_width,
        alignment,
    );
    egui::Label::new(galley)
}

/// Draw an "avatar placeholder": use this to take the place when this person has no avatar (or the avatar hasn't been fetched yet).
/// Return the response of the whole block; the caller can use it to turn it into a clickable entry (click the message avatar to view the profile).
pub(crate) fn draw_placeholder(ui: &mut Ui, name: &str, side: f32) -> Response {
    let initial = name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    let color = placeholder_color(name);
    // Deterministic box: allocate the exact square and paint it by hand, since a
    // frame sized from its label placed the glyph outside the box in a right-to-left row.
    let (response, painter) = ui.allocate_painter(Vec2::splat(side), Sense::hover());
    let rect = response.rect;
    painter.rect_filled(rect, 4.0, color);
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(initial, font_id, ui.visuals().text_color());
    painter.galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
    response
}

/// Placeholder hint for the member input box: comma-separated usernames
pub(crate) fn members_placeholder() -> String {
    "user1,user2".to_string()
}

/// Sender label for a message row; with "show sender uid" on the id is appended for display
/// only (name-based lookups like `/kick` are unaffected).
pub(crate) fn sender_label(name: &str, user_id: &str, show_uid: bool) -> String {
    if show_uid && !name.is_empty() {
        format!("{name} ({user_id})")
    } else {
        name.to_string()
    }
}

/// "someone is typing" title suffix (templates `typing_one`/`typing_multiple` from the
/// language table); None when nobody is typing.
pub(crate) fn typing_text(
    one_template: &str,
    multiple_template: &str,
    names: &[String],
) -> Option<String> {
    match names.len() {
        0 => None,
        1 => Some(one_template.replace("{username}", &names[0])),
        _ => Some(multiple_template.replace("{names}", &names.join(", "))),
    }
}

/// Test-visible entry: verify the time format changes with the switch
#[cfg(test)]
pub fn format_time_for_test(created_at: &str, with_date: bool) -> String {
    format_message_time(created_at, with_date)
}

/// Time text: whether to include the date is decided by the user toggle
pub(crate) fn format_message_time(created_at: &str, with_date: bool) -> String {
    let parsed = chrono::DateTime::parse_from_rfc3339(created_at)
        .ok()
        .map(|value| value.with_timezone(&chrono::Local));
    match parsed {
        Some(local) if with_date => local.format("%Y-%m-%d %H:%M:%S").to_string(),
        Some(local) => local.format("%H:%M:%S").to_string(),
        None => created_at.to_string(),
    }
}

/// Placeholder background color: the same person sees the same color each time
pub(crate) fn placeholder_color(name: &str) -> Color32 {
    let mut hash: u64 = 1469598103934665603;
    for byte in name.as_bytes() {
        hash = (hash ^ *byte as u64).wrapping_mul(1099511628211);
    }
    let red = (((hash >> 32) as u8) >> 1).max(48);
    let green = (((hash >> 40) as u8) >> 1).max(48);
    let blue = (((hash >> 48) as u8) >> 1).max(48);
    Color32::from_rgb(red, green, blue)
}

impl BaihuaApp {
    /// First compute what each message needs to draw (including textures); the drawing phase only reads local data
    pub(crate) fn message_rows(&mut self, context: &Context) -> Vec<MessageRow> {
        let (matches, current_match) = self.client.search_matches();
        let skin = self.skin.clone();
        // Encrypted private history arrives with an empty body (only the session holds keys):
        // paint the localized placeholder; search skips it since raw content stays empty.
        let encrypted_history_placeholder = self.text("message_encrypted_history_unavailable");
        self.client
            .messages
            .clone()
            .into_iter()
            .map(|message| {
                // Search match: if the theme gives the "match fragment background color", lay it behind the text as intended,
                // swap the text color to the one that contrasts with this background, to avoid light-on-light being unreadable
                let content_highlight = if matches.iter().any(|id| id == &message.id) {
                    Some(if current_match.as_deref() == Some(message.id.as_str()) {
                        skin.search_current_match_background
                    } else {
                        skin.search_match_background
                    })
                } else {
                    None
                };
                self.message_row_data(
                    context,
                    &message,
                    content_highlight,
                    &encrypted_history_placeholder,
                )
            })
            .collect()
    }

    /// Drawing data for one message (label, time, avatar texture, body colors). Shared by
    /// the conversation and the search panel so a match looks identical in both.
    pub(crate) fn message_row_data(
        &mut self,
        context: &Context,
        message: &MessageInfo,
        content_highlight: Option<Color32>,
        encrypted_history_placeholder: &str,
    ) -> MessageRow {
        let own_id = self.client.current_user_id.clone().unwrap_or_default();
        let is_own = message.sender_id == own_id;
        let sender = self.client.sender_display_name(&message.sender_id);
        let sender = sender_label(&sender, &message.sender_id, self.client.show_uid);
        let skin = self.skin.clone();
        let bytes = self
            .client
            .avatar_images
            .get(&message.sender_id)
            .and_then(|cached| cached.as_ref())
            .cloned();
        let texture = self.avatars.texture(
            context,
            &message.sender_id,
            bytes.as_deref(),
            avatar_side_pixels(),
        );
        let bubble_color = if is_own {
            skin.own_username_text
        } else {
            skin.message_text
        };
        let content: String = if message.content.is_empty() {
            encrypted_history_placeholder.to_string()
        } else {
            message.content.clone()
        };
        MessageRow {
            message_id: message.id.clone(),
            sender_id: message.sender_id.clone(),
            time_text: format_message_time(&message.created_at, self.client.time_with_date),
            sender,
            content,
            is_own,
            bubble_color,
            content_highlight,
            texture,
        }
    }

    /// Search panel result rows in conversation order, built with the same row data.
    /// No highlight: the panel lists matches without touching the conversation's own marking.
    pub(crate) fn search_panel_rows(
        &mut self,
        context: &Context,
        match_ids: &[String],
    ) -> Vec<MessageRow> {
        let encrypted_history_placeholder = self.text("message_encrypted_history_unavailable");
        let matched: Vec<MessageInfo> = self
            .client
            .messages
            .iter()
            .filter(|message| match_ids.iter().any(|id| id == &message.id))
            .cloned()
            .collect();
        matched
            .into_iter()
            .map(|message| {
                self.message_row_data(context, &message, None, &encrypted_history_placeholder)
            })
            .collect()
    }

    pub(crate) fn draw_central(
        &mut self,
        ui: &mut Ui,
        context: &Context,
        sidebar_rect: Rect,
        group_settings: &mut GroupSettingsView,
    ) {
        let skin = self.skin.clone();
        // Every button image this frame needs, decoded once before the closures run.
        let icons = self.frame_icons(context);
        let title = self.chat_title();
        let border = self.input_border_color();
        let placeholder = self.text("message_input_placeholder");
        let empty_hint = self.text("messages_empty");
        let mut complete_command: Option<String> = None;
        // Whether the message area touched the top this frame (the session layer uses this to automatically pull earlier messages from the server)
        let mut reached_top = false;
        // The message to scroll to for the search match (the session layer gives this once per frame)
        let scroll_to_message = self.client.pending_scroll_message_id.clone();
        let commands: Vec<(&'static str, String)> = crate::command_entries()
            .into_iter()
            .map(|(name, key)| (name, self.text(key)))
            .collect();
        let rows = self.message_rows(context);
        let mut input_view = MessageInputView {
            draft: self.client.draft.clone(),
            placeholder,
            // Fetch all at once: only the frame that "just needs to hand the cursor to the message box" requests focus,
            // Otherwise requesting focus every frame would steal focus from other input boxes the user just clicked
            request_focus: std::mem::take(&mut self.focus_message_input),
            selected_command: self.completion_selection,
            send_title: self.text("hint_send"),
        };
        let mut send_from_input_box = false;
        let mut draft_changed = false;
        // Whether the message box owns keyboard focus this frame (written by the
        // input-area closure, latched onto the app after the panel is drawn).
        let mut message_box_focused = false;
        let mut clicked_avatar: Option<String> = None;
        // In search mode, press up/down arrow keys: jot down in the frame, then apply to the session layer after the closure
        let mut search_step: Option<bool> = None;
        let stick_to_bottom = self.client.pending_scroll_message_id.is_none();
        // Both title-row buttons exist only for group rooms; read once here so the drawing
        // closure never touches self.
        let current_is_group = self
            .client
            .current_room_id()
            .is_some_and(|room_id| self.client.room_is_group(&room_id));
        // Whether the cached roster describes the room on screen (it guards *staying* open,
        // never opening; see `resolve_sidebar_open`).
        let member_roster_ready = detail_matches_room(
            self.client.current_room_detail.as_ref(),
            self.client.current_room_id().as_deref(),
        );
        let group_settings_title = self.text("group_settings_button");
        let mut toggle_group_settings = false;
        let search_panel_hint = self.text("search_panel_button");
        let mut toggle_search_panel = false;
        // Narrow layout: the sidebar is its own whole-window layer, never a panel
        // here, and the logo only replaces the message region with no group open.
        let background_logo = if current_is_group {
            None
        } else {
            self.avatars.embedded_texture(
                context,
                "baihua-embedded-logo",
                crate::logo_bytes(),
                crate::logo_texture_side(),
            )
        };
        // Touch platforms lift the input dock to the top while the box is focused;
        // desktops keep the bottom dock forever.
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let input_on_top = self.message_box_focused_last_frame;
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let input_on_top = false;
        let narrow = window_is_narrow(context);
        let narrow_back_hint = self.text("narrow_back_hint");
        let mut close_selected_room = false;
        // Where the sidebar landed this frame and whether the "..." button was hit: the two rectangles the
        // click-outside rule below must not treat as "blank space" (see `GroupSettingsView`).
        let group_settings_rect = sidebar_rect;
        let mut group_settings_button_rect = Rect::NOTHING;

        // Wide layout: facing edges stay square (room list left, sidebar right);
        // alone on the window (narrow layout) all four corners are rounded.
        let area_frame = if narrow {
            window_layer_frame(&skin, border)
        } else {
            chat_area_frame(&skin, border, sidebar_rect.is_positive())
        };
        egui::CentralPanel::default()
            .frame(area_frame)
            .show(ui, |ui| {
                // The whole title row (back button, room name, both right-hand buttons)
                // is one testable function: `draw_title_row`.
                let title_row_view = TitleRowView {
                    narrow,
                    title: &title,
                    group_room: current_is_group,
                    back_hint: &narrow_back_hint,
                    group_settings_hint: &group_settings_title,
                    search_hint: &search_panel_hint,
                };
                let (back_pressed, settings_pressed, search_pressed, settings_rect) =
                    draw_title_row(ui, &skin, &icons, &title_row_view);
                close_selected_room = back_pressed;
                toggle_group_settings = settings_pressed;
                toggle_search_panel = search_pressed;
                group_settings_button_rect = settings_rect;
                ui.separator();
                let (_, avatar, top) = draw_conversation(
                    ui,
                    &skin,
                    (rows, empty_hint, background_logo),
                    stick_to_bottom,
                    scroll_to_message.clone(),
                    input_on_top,
                    |ui| {
                        let outcome =
                            draw_input_area(ui, &skin, &icons, &commands, &mut input_view);
                        send_from_input_box = outcome.send;
                        draft_changed = outcome.draft_changed;
                        search_step = outcome.search_step;
                        complete_command = outcome.complete_command;
                        message_box_focused = outcome.focused;
                    },
                );
                clicked_avatar = avatar;
                reached_top = top;
            });

        // One decision point for the sidebar flag; opening fetches the roster once
        // here (a user action, not the render path) and a blank click closes it.
        let blank_click = !narrow
            && self.group_settings_open
            && group_settings_button_rect != Rect::NOTHING
            && sidebar_blank_click(context, group_settings_rect, group_settings_button_rect);
        let was_open = self.group_settings_open;
        self.group_settings_open = resolve_sidebar_open(
            current_is_group,
            member_roster_ready,
            was_open,
            toggle_group_settings,
            blank_click,
            group_settings.open,
        );
        if !was_open && self.group_settings_open {
            // Opening the sidebar is a user-initiated moment, so pulling the member table once here is
            // allowed (the render path itself never issues a request). Closing it needs no read.
            self.client.refresh_room_detail();
        }
        if toggle_search_panel {
            // The magnifier toggles the panel; the window's own close button and Esc dismiss it too.
            self.search_panel_open = !self.search_panel_open;
            if self.search_panel_open {
                // Just opened: hand the caret to the keyword box so Enter can commit right away.
                self.focus_search_panel_input = true;
            }
        }
        if close_selected_room {
            // "X" in the narrow layout: drop the selection, which also settles the
            // sidebar flag (no room means no group room).
            self.client.close_room_selection();
        }
        // The draft is written back before the outcome is applied: removing the member clears the box, and
        // the cleared value must be the one that survives to the next frame.
        self.group_settings_member_name = group_settings.member_name.clone();
        self.apply_sidebar(std::mem::replace(
            &mut group_settings.outcome,
            GroupSettingsOutcome::Nothing,
        ));
        self.client.draft = input_view.draft;
        self.message_box_focused_last_frame = message_box_focused;
        // The selected item in the completion list must be saved across frames: `input_view` is rebuilt each frame based on the current field,
        // if not written back to the interface field, whichever item the up/down arrows selected would be lost by next frame (looks like no response)
        self.completion_selection = input_view.selected_command;
        // In search mode, press up/down arrow keys: switch matches, do not move the cursor in the input box (keys already consumed by the input area)
        if let Some(backwards) = search_step {
            self.client.navigate_search(backwards);
        }
        // Input box content changed: hand it to the session layer to process with the same rules as the terminal version
        // (clear old results when not in search mode; rescan on-the-fly when fast search is on)
        if draft_changed {
            self.client.handle_draft_changed();
        }
        if send_from_input_box {
            let appearance_before = self.client.appearance_name.clone();
            let intent = self.client.submit_draft();
            self.apply_intent(intent);
            self.focus_message_input = true;
            // `/appearance <appearance name>` switches the theme directly at the session layer: after switching, the egui visuals must also be rewritten,
            // otherwise the window appearance would stay on the old theme (the settings panel switch goes through `apply_settings`, which has already refreshed).
            if self.client.appearance_name != appearance_before {
                self.refresh_skin(context);
            }
        }
        if let Some(user_id) = clicked_avatar {
            // Clicking the avatar is a "user-initiated one-time action", following the same path as /profile:
            // Fetching the profile once synchronously here is allowed (the render path itself doesn't send requests)
            self.client.show_profile_of(&user_id);
            self.profile_card_open = true;
        }
        // Pull earlier history on touch-top, guarded like the terminal version: older
        // messages exist, no pending scroll, and the list belongs to the current room.
        let list_matches_room = self.client.messages.last().is_some_and(|message| {
            Some(message.room_id.as_str()) == self.client.current_room_id().as_deref()
        });
        let auto_load_older = reached_top
            && self.client.has_more_older
            && self.client.older_cursor.is_some()
            && self.client.pending_scroll_message_id.is_none()
            && list_matches_room;
        if auto_load_older {
            // First remember the very top message now: after earlier history is inserted, scroll it back into view,
            // both not disturbing the position being read and moving the scroll position away from the top, to avoid repeated requests from a single touch-to-top
            let anchor_message_id = self
                .client
                .messages
                .first()
                .map(|message| message.id.clone());
            self.client.load_older_messages(auto_load_older);
            self.client.pending_scroll_message_id = anchor_message_id;
        }
        // Clear the scroll request only when this frame actually consumed it,
        // so a request written after drawing survives to the next frame.
        if scroll_consumed(
            scroll_to_message.as_deref(),
            self.client.pending_scroll_message_id.as_deref(),
        ) {
            self.client.pending_scroll_message_id = None;
        }
        // Completing (Enter fills the selected item, or clicking one item) just fills the command into the input box,
        // Same as the terminal version: whether to actually run the command is decided by the user pressing Enter once more
        if let Some(insert_text) = complete_command {
            self.client.draft = insert_text;
            self.completion_selection = 0;
            self.focus_message_input = true;
        }
    }

    pub(crate) fn chat_title(&self) -> String {
        if self.client.in_search_mode() {
            return match &self.client.search_result {
                None => self.text("search_mode_title"),
                Some((_keyword, matches, _index)) if matches.is_empty() => {
                    self.text("search_mode_no_match")
                }
                Some((_keyword, matches, index)) => {
                    let position = index + 1;
                    let total = matches.len();
                    // Display "the keyword that was already searched", not what's being typed in the input box:
                    // the latter changes with every keystroke; pairing it with old keyword match counts would make people think they're searching for a new word
                    let keyword = match self.client.searched_keyword() {
                        Some(keyword) => keyword.to_string(),
                        None => self.client.search_keyword(),
                    };
                    format!(
                        "{}: {keyword} {position}/{total}",
                        self.text("search_mode_title")
                    )
                }
            };
        }
        let entries = self.client.room_entries();
        let room_title = match self
            .client
            .selected_room_index
            .and_then(|index| entries.get(index))
        {
            Some(entry) => entry.title.clone(),
            None => self.text("chat_history"),
        };
        // Input status follows the group chat name (same location and format as the terminal version):
        // "group name · someone is typing…"; doesn't occupy a message row or interrupt reading
        match self.typing_text() {
            Some(typing) => format!("{room_title} {typing}"),
            None => room_title,
        }
    }

    /// "· someone is typing…" (multiple people separated by commas); return None when no one is typing.
    /// The member list is deduplicated by the session layer (same-name members kept only once); this just selects the text based on the count.
    pub(crate) fn typing_text(&self) -> Option<String> {
        typing_text(
            &self.text("typing_one"),
            &self.text("typing_multiple"),
            &self.client.typing_names(),
        )
    }

    pub(crate) fn input_border_color(&self) -> Color32 {
        if self.client.in_search_mode() {
            self.skin.search_border
        } else if self.client.draft.trim_start().starts_with('/') {
            self.skin.command_border
        } else {
            self.skin.input_border
        }
    }

    // ==================== Group Settings Sidebar ====================
}

#[cfg(test)]
mod sidebar_overlap_tests {
    use super::{MessageRow, draw_conversation};
    use crate::app::group_sidebar::{GroupSettingsOutcome, GroupSettingsView, draw_sidebar};
    use crate::app::icons::test_icons;
    use crate::appearance::Skin;
    use baihua_core::config::Palette;
    use egui::{CentralPanel, Color32, Context, Frame, Margin, Pos2, RawInput, Rect, Vec2};

    /// A short window like a phone turned sideways: the wide layout survives but
    /// the message area is squeezed by the room panel and the open sidebar.
    fn raw_input() -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 400.0))),
            focused: true,
            predicted_dt: 0.1,
            ..Default::default()
        }
    }

    fn own_row(content: &str) -> MessageRow {
        MessageRow {
            message_id: "message-1".to_string(),
            sender_id: "me".to_string(),
            sender: "myself".to_string(),
            content: content.to_string(),
            time_text: "12:00".to_string(),
            is_own: true,
            bubble_color: Color32::WHITE,
            content_highlight: None,
            texture: None,
        }
    }

    fn sidebar_view() -> GroupSettingsView {
        GroupSettingsView {
            open: true,
            group_title: "group settings".to_string(),
            add_member_title: "add member".to_string(),
            remove_member_title: "remove member".to_string(),
            mute_title: "mute this chat".to_string(),
            information_title: "group information".to_string(),
            members_title: "members".to_string(),
            members_empty_hint: "no member list yet".to_string(),
            readonly_hint: "only the owner or an admin may remove members".to_string(),
            add_member_hint: "username to add".to_string(),
            confirm_title: "confirm".to_string(),
            leave_title: "leave the selected group chat".to_string(),
            muted: false,
            member_rows: vec![("user-self".to_string(), "alice (owner)".to_string())],
            member_name: String::new(),
            allow_removal: true,
            summary_lines: vec!["group name: team one".to_string()],
            outcome: GroupSettingsOutcome::Nothing,
        }
    }

    /// With the sidebar open on a landscape window, a body that cannot be split at
    /// spaces used to paint through the panel; no row may cross its left border now.
    #[test]
    fn rows_clear_sidebar() {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let mut view = sidebar_view();
        let mut sidebar_rect = Rect::NOTHING;
        let mut texts: Vec<(String, f32, f32)> = Vec::new();
        for _frame in 0..24 {
            let rows = vec![own_row(&format!("{} {}", "a".repeat(400), "plain tail"))];
            context
                .run_ui(raw_input(), |ctx| {
                    CentralPanel::default()
                        .frame(
                            Frame::new()
                                .fill(skin.app_background)
                                .inner_margin(Margin::same(10)),
                        )
                        .show(ctx, |ui| {
                            let icons = test_icons(ui.ctx());
                            sidebar_rect = draw_sidebar(ui, &skin, &icons, &mut view);
                            let _ = draw_conversation(
                                ui,
                                &skin,
                                (rows.clone(), "no messages".to_string(), None),
                                true,
                                None,
                                false,
                                |_ui| {},
                            );
                        });
                    texts = ctx.graphics_mut(|graphics| {
                        let mut found: Vec<(String, f32, f32)> = Vec::new();
                        if let Some(list) = graphics.get(egui::LayerId::background()) {
                            for entry in list.all_entries() {
                                if let egui::Shape::Text(text) = &entry.shape {
                                    // `visual_bounds` covers the offsets a right-aligned
                                    // galley carries; `pos + size` measures the wrong edge.
                                    let bounds = entry.shape.visual_bounding_rect();
                                    found.push((
                                        text.galley.text().chars().take(8).collect(),
                                        bounds.min.x,
                                        bounds.max.x,
                                    ));
                                }
                            }
                        }
                        found
                    });
                })
                .drop_without_applying_deltas();
        }
        assert!(
            sidebar_rect.is_finite(),
            "the test frame must report the settled sidebar rectangle"
        );
        // Only the message row's own pieces (the sidebar paints its own texts
        // inside its own rectangle, which is not what this guards).
        let own_row_texts: Vec<&(String, f32, f32)> = texts
            .iter()
            .filter(|(text, _, _)| {
                text.starts_with("aaa")
                    || text == "myself"
                    || text == "plain ta"
                    || text == "12:00"
                    || text == "M"
            })
            .collect();
        assert!(
            own_row_texts.len() >= 3,
            "the message row must have painted its parts: {texts:?}"
        );
        for (text, _left, right) in own_row_texts {
            assert!(
                *right <= sidebar_rect.left(),
                "message row text {text:?} ends at {right}, past the sidebar border at {}",
                sidebar_rect.left()
            );
        }
    }

    /// A long room must keep every row inside the panel, and opening the sidebar
    /// afterwards must squeeze the rows left (the reported width feedback loop).
    #[test]
    fn panel_squeezes() {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let rows: Vec<super::MessageRow> = (0..200)
            .map(|index| {
                let mut row = own_row(&format!(
                    "message {index} padded so the row wraps over a couple of lines {}",
                    "filler".repeat(6)
                ));
                row.message_id = format!("message-{index}");
                row
            })
            .collect();
        let mut view = sidebar_view();
        view.open = false;
        let mut sidebar_rect = Rect::NOTHING;
        let mut widest_right = 0.0f32;
        let mut leftmost_left = f32::INFINITY;
        for _ in 0..12 {
            let (right, left, rect) = draw_long_room_frame(&context, &skin, &rows, &mut view);
            sidebar_rect = rect;
            widest_right = right;
            leftmost_left = left;
        }
        assert!(
            widest_right <= 890.0 && leftmost_left >= 10.0,
            "a settled long room must stay inside the panel edges 10..890, measured {leftmost_left}..{widest_right}"
        );
        view.open = true;
        for _ in 0..24 {
            let (right, _left, rect) = draw_long_room_frame(&context, &skin, &rows, &mut view);
            sidebar_rect = rect;
            widest_right = right;
        }
        assert!(
            sidebar_rect.is_finite(),
            "the sidebar must have settled at a real rectangle"
        );
        assert!(
            widest_right <= sidebar_rect.left(),
            "with the sidebar open no message may reach past its left border {}: measured right edge {widest_right}",
            sidebar_rect.left()
        );
    }

    /// One off-screen frame of the long room: the widest and leftmost body-text edges
    /// plus the sidebar rectangle.
    fn draw_long_room_frame(
        context: &Context,
        skin: &Skin,
        rows: &[super::MessageRow],
        view: &mut GroupSettingsView,
    ) -> (f32, f32, Rect) {
        use std::cell::RefCell;
        let rows = rows.to_vec();
        let measurements = RefCell::new((0.0f32, f32::INFINITY, Rect::NOTHING));
        context
            .run_ui(raw_input(), |ctx| {
                CentralPanel::default()
                    .frame(
                        Frame::new()
                            .fill(skin.app_background)
                            .inner_margin(Margin::same(10)),
                    )
                    .show(ctx, |ui| {
                        let icons = test_icons(ui.ctx());
                        let sidebar_rect = draw_sidebar(ui, skin, &icons, view);
                        let _ = draw_conversation(
                            ui,
                            skin,
                            (rows.clone(), "no messages".to_string(), None),
                            true,
                            None,
                            false,
                            |_ui| {},
                        );
                        measurements.borrow_mut().2 = sidebar_rect;
                    });
                let (widest, leftmost) = ctx.graphics_mut(|graphics| {
                    let mut widest = 0.0f32;
                    let mut leftmost = f32::INFINITY;
                    if let Some(list) = graphics.get(egui::LayerId::background()) {
                        for entry in list.all_entries() {
                            if let egui::Shape::Text(text) = &entry.shape
                                && text.galley.text().starts_with("message ")
                            {
                                let bounds = entry.shape.visual_bounding_rect();
                                widest = widest.max(bounds.max.x);
                                leftmost = leftmost.min(bounds.min.x);
                            }
                        }
                    }
                    (widest, leftmost)
                });
                let mut guard = measurements.borrow_mut();
                guard.0 = widest;
                guard.1 = leftmost;
            })
            .drop_without_applying_deltas();
        let (widest, leftmost, sidebar_rect) = *measurements.borrow();
        (widest, leftmost, sidebar_rect)
    }
}

#[cfg(test)]
mod message_alignment_tests {
    use super::{
        MessageRow, bubble_padding, build_message_galley, draw_message_row, row_needs_layout,
        sender_line_fraction, sender_line_shares,
    };
    use crate::appearance::Skin;
    use baihua_core::config::Palette;
    use egui::{Align, CentralPanel, Color32, Context, Pos2, RawInput, Rect, Vec2};

    /// The virtualization rule itself: a row is laid out when it is the jump
    /// target, or when it overlaps the viewport extended by `slack`.
    #[test]
    fn row_layout_rule() {
        let viewport = Rect::from_min_max(Pos2::new(0.0, 100.0), Pos2::new(100.0, 200.0));
        assert!(
            row_needs_layout(110.0, 140.0, viewport, 100.0, false),
            "a row inside the viewport must be laid out"
        );
        assert!(
            !row_needs_layout(900.0, 930.0, viewport, 100.0, false),
            "a row far below the viewport plus its slack must be skipped"
        );
        assert!(
            !row_needs_layout(-50.0, -20.0, viewport, 100.0, false),
            "a row far above the viewport minus its slack must be skipped"
        );
        assert!(
            row_needs_layout(900.0, 930.0, viewport, 100.0, true),
            "the row a search jump names must be laid out wherever it is"
        );
        assert!(
            row_needs_layout(250.0, 280.0, viewport, 100.0, false),
            "a row inside the slack band below the viewport must be laid out"
        );
    }

    /// Test window size: leave enough blank space on both sides so you can measure "which side it's sticking to"
    fn screen_width() -> f32 {
        800.0
    }

    /// A narrow phone-like window: a portrait phone is roughly 390 points wide.
    fn phone_width() -> f32 {
        390.0
    }

    fn raw_input() -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(screen_width(), 600.0),
            )),
            ..Default::default()
        }
    }

    fn phone_raw_input() -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(phone_width(), 600.0),
            )),
            ..Default::default()
        }
    }

    /// The same measurement as `painted_texts` but on the phone-sized window.
    fn texts_on_phone(row: MessageRow) -> Vec<(String, f32, f32)> {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let mut row_slot = Some(row);
        let mut collected: Vec<(String, f32, f32)> = Vec::new();
        context
            .run_ui(phone_raw_input(), |ctx| {
                if let Some(row) = row_slot.take() {
                    CentralPanel::default().show(ctx, |ui| {
                        draw_message_row(ui, &skin, row, false);
                    });
                }
                collected = ctx.graphics_mut(|graphics| {
                    let mut texts: Vec<(String, f32, f32)> = Vec::new();
                    if let Some(list) = graphics.get(egui::LayerId::background()) {
                        for entry in list.all_entries() {
                            if let egui::Shape::Text(text) = &entry.shape {
                                let bounds = entry.shape.visual_bounding_rect();
                                texts.push((
                                    text.galley.text().to_string(),
                                    bounds.min.x,
                                    bounds.max.x,
                                ));
                            }
                        }
                    }
                    texts
                });
            })
            .drop_without_applying_deltas();
        collected
    }

    /// Draw a message row and take back the painted span of every text, measured as
    /// visual bounds because a right-aligned galley extends left of `pos`.
    fn painted_texts(row: MessageRow) -> Vec<(String, f32, f32)> {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let mut row_slot = Some(row);
        let mut collected: Vec<(String, f32, f32)> = Vec::new();
        context
            .run_ui(raw_input(), |ctx| {
                if let Some(row) = row_slot.take() {
                    CentralPanel::default().show(ctx, |ui| {
                        draw_message_row(ui, &skin, row, false);
                    });
                }
                collected = ctx.graphics_mut(|graphics| {
                    let mut texts: Vec<(String, f32, f32)> = Vec::new();
                    if let Some(list) = graphics.get(egui::LayerId::background()) {
                        for entry in list.all_entries() {
                            if let egui::Shape::Text(text) = &entry.shape {
                                let bounds = entry.shape.visual_bounding_rect();
                                texts.push((
                                    text.galley.text().to_string(),
                                    bounds.min.x,
                                    bounds.max.x,
                                ));
                            }
                        }
                    }
                    texts
                });
            })
            .drop_without_applying_deltas();
        collected
    }

    /// What one row painted: the rounded rectangles (bubble bodies) together with
    /// their radius and fill, and the filled triangles (the tails).
    type BubblePaint = (Vec<(Rect, u8, Color32)>, Vec<(Rect, Color32)>);

    /// Draw one row and return the rounded rectangles and the filled triangles it
    /// painted: the bubble body and its tail.
    fn painted_bubble(row: MessageRow) -> BubblePaint {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let mut row_slot = Some(row);
        let mut rects: Vec<(Rect, u8, Color32)> = Vec::new();
        let mut tails: Vec<(Rect, Color32)> = Vec::new();
        context
            .run_ui(raw_input(), |ctx| {
                if let Some(row) = row_slot.take() {
                    CentralPanel::default().show(ctx, |ui| {
                        draw_message_row(ui, &skin, row, false);
                    });
                }
                let collected = ctx.graphics_mut(|graphics| {
                    let mut shapes: Vec<(Rect, u8, Color32, bool)> = Vec::new();
                    if let Some(list) = graphics.get(egui::LayerId::background()) {
                        for entry in list.all_entries() {
                            match &entry.shape {
                                egui::Shape::Rect(shape) => shapes.push((
                                    shape.rect,
                                    shape.corner_radius.nw,
                                    shape.fill,
                                    false,
                                )),
                                egui::Shape::Path(shape) if shape.fill != Color32::TRANSPARENT => {
                                    let bounds = entry.shape.visual_bounding_rect();
                                    shapes.push((bounds, 0, shape.fill, true));
                                }
                                _ => {}
                            }
                        }
                    }
                    shapes
                });
                rects = collected
                    .iter()
                    .filter(|(_, _, _, tail)| !*tail)
                    .map(|(rect, radius, color, _)| (*rect, *radius, *color))
                    .collect();
                tails = collected
                    .iter()
                    .filter(|(_, _, _, tail)| *tail)
                    .map(|(rect, _, color, _)| (*rect, *color))
                    .collect();
            })
            .drop_without_applying_deltas();
        (rects, tails)
    }

    fn span_of(texts: &[(String, f32, f32)], needle: &str) -> (f32, f32) {
        texts
            .iter()
            .find(|(text, _, _)| text == needle)
            .map(|(_, left, right)| (*left, *right))
            .unwrap_or_else(|| panic!("no painted text {needle:?} in {texts:?}"))
    }

    fn row(content: &str, sender: &str, is_own: bool) -> MessageRow {
        MessageRow {
            message_id: "message-1".to_string(),
            sender_id: if is_own {
                "me".to_string()
            } else {
                "other".to_string()
            },
            sender: sender.to_string(),
            content: content.to_string(),
            time_text: "12:00".to_string(),
            is_own,
            bubble_color: Color32::WHITE,
            content_highlight: None,
            texture: None,
        }
    }

    /// The uv rectangles of the glyphs of one galley text, in paint order.
    fn glyph_uves(context: &Context, wanted: &str) -> Vec<(u16, u16)> {
        context.graphics(|list| {
            let list = list
                .get(egui::LayerId::background())
                .expect("the frame must paint something");
            for entry in list.all_entries() {
                if let egui::Shape::Text(shape) = &entry.shape
                    && shape.galley.text() == wanted
                {
                    return shape
                        .galley
                        .rows
                        .iter()
                        .flat_map(|placed| placed.glyphs.iter())
                        .map(|glyph| (glyph.uv_rect.min[0], glyph.uv_rect.min[1]))
                        .collect();
                }
            }
            Vec::new()
        })
    }

    /// A message body must never be painted from a galley shaped against an older font
    /// table: `add_font` lands next frame, and the poisoned uv would stay forever.
    #[test]
    fn late_font_repaints() {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let body = "中文消息正文";
        let row = row(body, "myself", true);
        let mut painted: Vec<(u16, u16)> = Vec::new();
        let mut fresh: Vec<(u16, u16)> = Vec::new();
        for pass in 0..4 {
            let mut slot = Some(vec![row.clone()]);
            context
                .run_ui(raw_input(), |ctx| {
                    if pass == 0 {
                        // The font table changes now and only takes effect next frame:
                        // this frame's galley is shaped against the old table.
                        crate::appearance::install_chinese_font(ctx);
                    }
                    CentralPanel::default().show(ctx, |ui| {
                        let rows = slot.take().unwrap_or_default();
                        let _ = draw_message_row(ui, &skin, rows[0].clone(), false);
                        if pass == 3 {
                            let galley = build_message_galley(
                                ui,
                                body,
                                Color32::BLACK,
                                None,
                                ui.available_width(),
                                Align::Min,
                            );
                            fresh = galley
                                .rows
                                .iter()
                                .flat_map(|placed| placed.glyphs.iter())
                                .map(|glyph| (glyph.uv_rect.min[0], glyph.uv_rect.min[1]))
                                .collect();
                        }
                    });
                    painted = glyph_uves(ctx, body);
                })
                .drop_without_applying_deltas();
        }
        assert!(
            !painted.is_empty() && painted.len() == fresh.len(),
            "the body must be painted and comparable, got {} painted and {} fresh",
            painted.len(),
            fresh.len()
        );
        assert_eq!(
            painted, fresh,
            "the painted message body must come from the current font table"
        );
    }

    /// Own messages hug the right edge and foreign ones the left, and the sender
    /// and content of an own row must share one right edge, not merely one side.
    #[test]
    fn message_alignment() {
        let texts = painted_texts(row("this is a message from me", "myself", true));
        let (content_left, _) = span_of(&texts, "this is a message from me");
        assert!(
            content_left > screen_width() / 2.0,
            "in my own message the content must start on the right half, start was {content_left}"
        );
        let (_, name_right) = span_of(&texts, "myself");
        let (_, content_right) = span_of(&texts, "this is a message from me");
        // The body now lives inside a bubble: its own text stops one bubble padding
        // short of the edge, while the bubble itself ends exactly on the name edge.
        assert!(
            (name_right - content_right - bubble_padding() as f32).abs() < 2.0,
            "sender (ends {name_right}) and the bubble body must share the right edge, content ends {content_right}"
        );
        let (rects, tails) = painted_bubble(row("this is a message from me", "myself", true));
        let bubble = rects
            .iter()
            .find(|(_, radius, color)| *radius > 0 && *color == Color32::WHITE)
            .unwrap_or_else(|| panic!("a rounded white bubble must be painted, got {rects:?}"));
        assert!(
            (bubble.0.max.x - name_right).abs() < 2.0,
            "the bubble (ends {}) must end on the sender line's right edge ({name_right})",
            bubble.0.max.x
        );
        assert_eq!(
            bubble.2,
            Color32::WHITE,
            "the bubble must be filled with the row's own body colour"
        );
        assert!(
            tails.iter().any(|(rect, color)| *color == bubble.2
                && rect.min.x >= bubble.0.max.x - 2.0
                && (rect.center().y - bubble.0.center().y).abs() < 1.5),
            "the tail must point right at the avatar, on the bubble's own axis, got {tails:?} against {:?}",
            bubble.0
        );

        let texts = painted_texts(row("a message from bob", "bob", false));
        let (content_left, _) = span_of(&texts, "a message from bob");
        assert!(
            content_left < screen_width() / 2.0,
            "in a foreign message the content must start on the left half, start was {content_left}"
        );
        // The other person's bubble hugs the left edge and points left at its avatar.
        let (rects, tails) = painted_bubble(row("a message from bob", "bob", false));
        let bubble = rects
            .iter()
            .find(|(_, radius, color)| *radius > 0 && *color == Color32::WHITE)
            .unwrap_or_else(|| panic!("a rounded white bubble must be painted, got {rects:?}"));
        assert!(
            bubble.0.min.x < screen_width() / 2.0,
            "the foreign bubble must sit on the left half, got {:?}",
            bubble.0
        );
        assert!(
            tails.iter().any(|(rect, _)| {
                rect.max.x <= bubble.0.min.x + 2.0
                    && (rect.center().y - bubble.0.center().y).abs() < 1.5
            }),
            "the tail must point left at the avatar, on the bubble's own axis, got {tails:?} for bubble {:?}",
            bubble.0
        );
    }

    /// A long unbreakable run must NOT stay one huge line: it has to break at
    /// the row width (the overflow was what painted over the group sidebar).
    #[test]
    fn long_run_wraps() {
        let texts = painted_texts(row(&"x".repeat(400), "myself", true));
        let (content_left, content_right) = span_of(&texts, &"x".repeat(400));
        assert!(
            content_right - content_left <= screen_width(),
            "the wrapped content ({}..{content_right}) must not be wider than the row",
            content_left
        );
    }

    /// With the uid suffix and the date on a sender line is wider than a portrait
    /// phone: the name must truncate inside it, in an own row and in a foreign one.
    #[test]
    fn long_sender_name() {
        let long_sender = "a_very_long_username_indeed_here (12345678-1234-1234-1234-123456789012)";
        for is_own in [true, false] {
            let texts = texts_on_phone(row("hello", long_sender, is_own));
            for (text, left, right) in &texts {
                assert!(
                    *left >= -1.0 && *right <= phone_width() + 1.0,
                    "text {text:?} of an own={is_own} row sticks out of the phone screen: {left}..{right}"
                );
            }
            // The painted name must be narrower than the same text on one
            // unbroken line, otherwise nothing was elided.
            let (name_left, name_right) = span_of(&texts, long_sender);
            assert!(
                name_right - name_left < single_line_width(long_sender),
                "the long name must be truncated below its full single-line width"
            );
        }
    }

    /// Width this text would need on one unbroken line (the value the elided
    /// name has to stay below).
    fn single_line_width(text: &str) -> f32 {
        let context = Context::default();
        let mut width = 0.0;
        context
            .run_ui(phone_raw_input(), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    width = ui
                        .painter()
                        .layout_no_wrap(
                            text.to_owned(),
                            egui::TextStyle::Body.resolve(ui.style()),
                            Color32::WHITE,
                        )
                        .size()
                        .x;
                });
            })
            .drop_without_applying_deltas();
        width
    }

    /// The timestamp must survive inside the phone width next to a truncated name and
    /// shrink below its natural width when the row is tight.
    #[test]
    fn timestamp_survives() {
        let long_sender = "a_very_long_username_indeed_here (12345678-1234-1234-1234-123456789012)";
        let texts = texts_on_phone(row("hello", long_sender, true));
        let cap = phone_width() * sender_line_fraction();
        let (time_left, time_right) = span_of(&texts, "12:00");
        assert!(
            time_left >= 0.0 && time_right <= phone_width(),
            "the timestamp ({time_left}..{time_right}) must stay inside the phone width"
        );
        assert!(
            time_right - time_left <= cap,
            "the timestamp ({time_left}..{time_right}) must fit the sender line cap too"
        );
        assert!(
            time_right - time_left < single_line_width("12:00"),
            "the timestamp must be squeezed below its natural width when the row is tight"
        );
    }

    /// An over-long "name (uid)" must be elided inside `sender_line_fraction` of the
    /// row: an own row lays out right-to-left, so a free block grows left out of it.
    #[test]
    fn sender_line_capped() {
        let long_sender = "a_very_long_username_indeed_here (12345678-1234-1234-1234-123456789012)";
        let texts = texts_on_phone(row("hello", long_sender, true));
        let (name_left, name_right) = span_of(&texts, long_sender);
        // The whole line (name plus the timestamp beside it) must fit inside the
        // cap, which is a fraction of the row's width.
        let (time_left, _time_right) = span_of(&texts, "12:00");
        let line_left = name_left.min(time_left);
        let line_right = name_right.max(_time_right);
        let cap = phone_width() * sender_line_fraction();
        assert!(
            line_right - line_left <= cap + 1.0,
            "the sender line ({}..{line_right}) must stay inside its {cap}-point cap",
            line_left
        );
        // It must also be visibly shorter than the untruncated text (i.e. it
        // really elided rather than silently being allowed to be full width).
        assert!(
            name_right - name_left < single_line_width(long_sender),
            "the long name must be elided inside the cap, not kept at full size"
        );
    }

    /// A short sender line must NOT be squeezed: the cap is a maximum, and a
    /// short "bob 12:00" keeps its natural size and position.
    #[test]
    fn short_line_untouched() {
        let texts = texts_on_phone(row("hello", "bob", true));
        let (name_left, name_right) = span_of(&texts, "bob");
        assert!(
            (name_right - name_left - single_line_width("bob")).abs() < 1.0,
            "a short name must keep its natural width, got {}..{name_right}",
            name_left
        );
    }

    /// The four ordered rules that split the sender line: both fit, the wider gives
    /// way, both are halved and elided, then nothing is drawn at all.
    #[test]
    fn sender_line_halves() {
        assert_eq!(
            sender_line_shares(300.0, 100.0, 120.0, 8.0, 9.0),
            (Some(100.0), Some(120.0)),
            "when both halves fit, both keep their natural widths"
        );
        assert_eq!(
            sender_line_shares(300.0, 400.0, 100.0, 8.0, 9.0),
            (Some(192.0), Some(100.0)),
            "a wide name shrinks to what the short timestamp leaves"
        );
        assert_eq!(
            sender_line_shares(300.0, 100.0, 400.0, 8.0, 9.0),
            (Some(100.0), Some(192.0)),
            "a wide timestamp shrinks to what the short name leaves (the reported bug)"
        );
        assert_eq!(
            sender_line_shares(200.0, 400.0, 400.0, 8.0, 9.0),
            (Some(96.0), Some(96.0)),
            "two oversized halves each get half the cap and are elided"
        );
        assert_eq!(
            sender_line_shares(10.0, 400.0, 400.0, 8.0, 9.0),
            (None, None),
            "a cap that cannot hold half an ellipsis must draw no text at all"
        );
        assert_eq!(
            sender_line_shares(5.0, 400.0, 400.0, 8.0, 9.0),
            (None, None),
            "a cap narrower than one ellipsis must draw no text at all"
        );
    }
}

#[cfg(test)]
mod conversation_layout_tests {
    use super::{MessageRow, draw_conversation};
    use crate::appearance::Skin;
    use baihua_core::config::Palette;
    use egui::{CentralPanel, Color32, Context, Pos2, RawInput, Rect, Ui, Vec2};

    fn raw_input() -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 600.0))),
            focused: true,
            // egui animates a `scroll_to_rect` request and only advances it when the
            // frame reports elapsed time, so without this a jump never arrives.
            predicted_dt: 0.1,
            ..Default::default()
        }
    }

    fn no_rows() -> Vec<MessageRow> {
        Vec::new()
    }

    /// One frame: draw the conversation area and report the y of the empty hint
    /// (message region) and of the input-area marker.
    fn marker_ys(input_on_top: bool) -> (f32, f32) {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let mut ys = (0.0, 0.0);
        context
            .run_ui(raw_input(), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    let _ = draw_conversation(
                        ui,
                        &skin,
                        (no_rows(), "no messages yet".to_string(), None),
                        true,
                        None,
                        input_on_top,
                        |ui: &mut Ui| {
                            ys.1 = ui.label("input marker").rect.center().y;
                        },
                    );
                    ys.0 = 0.0;
                });
                ys.0 = ctx
                    .graphics_mut(|graphics| {
                        let list = graphics.get(egui::LayerId::background())?;
                        for entry in list.all_entries() {
                            if let egui::Shape::Text(text) = &entry.shape
                                && text.galley.text() == "no messages yet"
                            {
                                return Some(entry.shape.visual_bounding_rect().center().y);
                            }
                        }
                        None
                    })
                    .unwrap_or_default();
            })
            .drop_without_applying_deltas();
        ys
    }

    /// With no messages the empty-state hint is painted at all.
    #[test]
    fn empty_hint_painted() {
        let (hint_y, _) = marker_ys(false);
        assert!(hint_y > 0.0, "the empty hint must have been painted");
    }

    /// The input dock stays below the messages by default and moves above them on a
    /// touch platform, where the soft keyboard would otherwise cover the draft.
    #[test]
    fn input_dock_side() {
        let (hint_y, input_y) = marker_ys(false);
        assert!(
            input_y > hint_y,
            "bottom-docked input ({input_y}) must sit below the message area ({hint_y})"
        );
        let (hint_y, input_y) = marker_ys(true);
        assert!(
            input_y < hint_y,
            "top-docked input ({input_y}) must sit above the message area ({hint_y})"
        );
    }

    fn numbered_rows(count: usize) -> Vec<MessageRow> {
        (0..count)
            .map(|index| MessageRow {
                message_id: format!("message-{index}"),
                sender_id: "other".to_string(),
                sender: format!("sender-{index}"),
                content: format!("body {index}"),
                time_text: "12:00".to_string(),
                is_own: false,
                bubble_color: Color32::WHITE,
                content_highlight: None,
                texture: None,
            })
            .collect()
    }

    /// Draw a room of `count` rows for a batch of frames and report which sender
    /// labels actually reached the screen.
    fn painted_senders(count: usize, jump_to: Option<&str>) -> Vec<usize> {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let rows = numbered_rows(count);
        let mut painted: Vec<usize> = Vec::new();
        for _frame in 0..12 {
            let jump = jump_to.map(|text| text.to_string());
            context
                .run_ui(raw_input(), |ctx| {
                    CentralPanel::default().show(ctx, |ui| {
                        let _ = draw_conversation(
                            ui,
                            &skin,
                            (rows.clone(), "none".to_string(), None),
                            false,
                            jump.clone(),
                            false,
                            |_ui: &mut Ui| {},
                        );
                    });
                    painted = ctx.graphics_mut(|graphics| {
                        let mut found: Vec<usize> = Vec::new();
                        if let Some(list) = graphics.get(egui::LayerId::background()) {
                            for entry in list.all_entries() {
                                if let egui::Shape::Text(text) = &entry.shape {
                                    let label = text.galley.text();
                                    if let Some(number) = label.strip_prefix("sender-")
                                        && let Ok(number) = number.parse::<usize>()
                                    {
                                        found.push(number);
                                    }
                                }
                            }
                        }
                        found
                    });
                })
                .drop_without_applying_deltas();
        }
        painted.sort_unstable();
        painted
    }

    /// A long room must not lay every row out on every frame, and skipping must
    /// not lose their space; a room that fits on screen is laid out completely.
    #[test]
    fn row_virtualization() {
        let painted = painted_senders(5, None);
        assert_eq!(
            painted,
            vec![0, 1, 2, 3, 4],
            "every visible row must be drawn"
        );

        let painted = painted_senders(200, Some("message-150"));
        assert!(
            painted.contains(&150),
            "the jumped-to row must be laid out: {painted:?}"
        );
        assert!(
            !painted.contains(&0),
            "row 0 must be skipped once the view is down at row 150: {painted:?}"
        );
        let highest = painted.iter().copied().max().unwrap_or(usize::MAX);
        let lowest = painted.iter().copied().min().unwrap_or(0);
        assert!(
            highest - lowest < 60,
            "only a windowful of rows around the jump may be laid out, got {lowest}..={highest}"
        );
    }
}

#[cfg(test)]
mod avatar_click_tests {
    use super::{MessageRow, draw_message_row};
    use crate::app::test_support::{click_at, frame, position_of};
    use crate::appearance::Skin;
    use baihua_core::config::Palette;
    use egui::{CentralPanel, Color32, Context, Event, Pos2, RawInput, Vec2};

    fn raw_input(events: Vec<Event>) -> RawInput {
        frame(800.0, 600.0, events)
    }

    /// A message from someone else: when there's no avatar, what's drawn is an avatar placeholder block, and the letter on the placeholder is the first character of the sender's name.
    fn other_message() -> MessageRow {
        MessageRow {
            message_id: "message-1".to_string(),
            sender_id: "user-2".to_string(),
            sender: "someone".to_string(),
            content: "this is a message from someone else".to_string(),
            time_text: "12:01".to_string(),
            is_own: false,
            bubble_color: Color32::WHITE,
            content_highlight: None,
            texture: None,
        }
    }

    /// A message from yourself: the avatar is on the far right
    fn own_message() -> MessageRow {
        MessageRow {
            message_id: "message-1".to_string(),
            sender_id: "me".to_string(),
            sender: "myself".to_string(),
            content: "this is a message from me".to_string(),
            time_text: "12:00".to_string(),
            is_own: true,
            bubble_color: Color32::WHITE,
            content_highlight: None,
            texture: None,
        }
    }

    /// Draw a message row for one frame, return (the text and positions drawn this frame, the user ID reported when clicking the avatar)
    fn run_one_frame(
        context: &Context,
        row: MessageRow,
        events: Vec<Event>,
    ) -> (Vec<(String, Pos2)>, Option<String>) {
        let skin = Skin::from(&Palette::built_in());
        let mut row_slot = Some(row);
        let mut clicked_sender: Option<String> = None;
        let mut painted: Vec<(String, Pos2)> = Vec::new();
        context
            .run_ui(raw_input(events), |ctx| {
                if let Some(row) = row_slot.take() {
                    CentralPanel::default().show(ctx, |ui| {
                        clicked_sender = draw_message_row(ui, &skin, row, false);
                    });
                }
                painted = ctx.graphics_mut(|graphics| {
                    let mut texts: Vec<(String, Pos2)> = Vec::new();
                    if let Some(list) = graphics.get(egui::LayerId::background()) {
                        for entry in list.all_entries() {
                            if let egui::Shape::Text(text_shape) = &entry.shape {
                                texts.push((text_shape.galley.text().to_string(), text_shape.pos));
                            }
                        }
                    }
                    texts
                });
            })
            .drop_without_applying_deltas();
        (painted, clicked_sender)
    }

    /// Only the avatar is clickable and reports the sender's id, own rows included;
    /// a click on the body text reports nothing so the text stays selectable.
    #[test]
    fn avatar_click() {
        let context = Context::default();
        let (texts, clicked) = run_one_frame(&context, other_message(), Vec::new());
        assert!(
            clicked.is_none(),
            "with no click at all no user id may be reported"
        );
        let avatar = position_of(&texts, "S");
        let (_texts, clicked) = run_one_frame(
            &context,
            other_message(),
            click_at(avatar + Vec2::new(2.0, 8.0)),
        );
        assert_eq!(
            clicked.as_deref(),
            Some("user-2"),
            "clicking another member's avatar must report their user id"
        );

        let (texts, _clicked) = run_one_frame(&context, own_message(), Vec::new());
        let avatar = position_of(&texts, "M");
        let (_texts, clicked) = run_one_frame(
            &context,
            own_message(),
            click_at(avatar + Vec2::new(2.0, 8.0)),
        );
        assert_eq!(
            clicked.as_deref(),
            Some("me"),
            "clicking your own avatar must report your own user id"
        );

        let (texts, _clicked) = run_one_frame(&context, other_message(), Vec::new());
        let content = position_of(&texts, "this is a message from someone else");
        let (_texts, clicked) = run_one_frame(
            &context,
            other_message(),
            click_at(content + Vec2::new(4.0, 8.0)),
        );
        assert!(
            clicked.is_none(),
            "a click on the body text must not report a user id, got {clicked:?}"
        );
    }
}

#[cfg(test)]
mod sender_label_tests {
    use super::{sender_label, typing_text};

    /// Regression for this round's feedback that "showing sender UID didn't work": when the switch is on, append the user ID after the name
    #[test]
    fn show_uid_suffix() {
        assert_eq!(sender_label("alice", "u-1", false), "alice");
        assert_eq!(sender_label("alice", "u-1", true), "alice (u-1)");
        // When the name is unavailable (e.g., a speaker who has been deregistered), only keep the ID without an empty pair of parentheses
        assert_eq!(sender_label("", "u-1", true), "");
    }

    /// Regression for feedback "the same person might be shown twice as typing" (this display side):
    /// The input status on the title selects the text based on the count; the member list has already been deduplicated
    #[test]
    fn typing_names_text() {
        let one = "· {username} is typing...";
        let many = "· {names} are typing...";
        assert_eq!(typing_text(one, many, &[]), None);
        assert_eq!(
            typing_text(one, many, &["alice".to_string()]),
            Some("· alice is typing...".to_string())
        );
        assert_eq!(
            typing_text(one, many, &["alice".to_string(), "bob".to_string()]),
            Some("· alice, bob are typing...".to_string())
        );
    }
}

#[cfg(test)]
mod logo_tests {
    use super::*;
    use crate::appearance::AvatarTextures;
    use baihua_core::config::Palette;
    use egui::{CentralPanel, Event, RawInput};

    fn raw_input() -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 600.0))),
            focused: true,
            events: vec![Event::PointerMoved(Pos2::new(450.0, 300.0))],
            ..Default::default()
        }
    }

    fn painted_logo_rects(context: &Context, background_logo: Option<TextureHandle>) -> Vec<Rect> {
        let expected_texture = background_logo.as_ref().map(|handle| handle.id());
        let skin = Skin::from(&Palette::built_in());
        let mut rects: Vec<Rect> = Vec::new();
        context
            .run_ui(raw_input(), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    let _ = draw_conversation(
                        ui,
                        &skin,
                        (
                            Vec::new(),
                            "no messages yet".to_string(),
                            background_logo.clone(),
                        ),
                        false,
                        None,
                        false,
                        |_ui: &mut Ui| {},
                    );
                });
                rects = ctx.graphics_mut(|graphics| {
                    let mut found: Vec<Rect> = Vec::new();
                    if let Some(list) = graphics.get(egui::LayerId::background()) {
                        for entry in list.all_entries() {
                            if let egui::Shape::Mesh(mesh) = &entry.shape
                                && Some(mesh.texture_id) == expected_texture
                            {
                                found.push(entry.shape.visual_bounding_rect());
                            }
                        }
                    }
                    found
                });
            })
            .drop_without_applying_deltas();
        rects
    }

    #[test]
    fn logo_decodes() {
        let decoded =
            image::load_from_memory(crate::logo_bytes()).expect("the embedded logo must decode");
        assert_eq!(decoded.width(), decoded.height(), "the logo must be square");
    }

    /// The embedded logo is painted centered in the message area while no group
    /// chat is open, and not at all when the caller withholds the texture.
    #[test]
    fn logo_centered() {
        let context = Context::default();
        let mut avatars = AvatarTextures::default();
        let logo = avatars
            .embedded_texture(
                &context,
                "baihua-embedded-logo",
                crate::logo_bytes(),
                crate::logo_texture_side(),
            )
            .expect("the logo texture must load");
        let rects = painted_logo_rects(&context, Some(logo));
        assert_eq!(rects.len(), 1, "exactly one logo image must be painted");
        let center = rects[0].center();
        // The message region of a 900x600 central panel: the logo sits near its center.
        assert!((center.x - 450.0).abs() < 60.0, "logo center x: {center:?}");
        assert!(
            (center.y - 300.0).abs() < 120.0,
            "logo center y: {center:?}"
        );
        assert!(
            painted_logo_rects(&context, None).is_empty(),
            "with no texture handed over nothing may be painted"
        );
    }
}
