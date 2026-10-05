//! The conversation input area: the draft text box, the slash-command
//! completion popup, and the in-input `#` search key handling.

use super::*;

/// The completion candidates as (fill text, display name, description): a `/` prefix
/// filters the command table, `/language ` and `/appearance ` list the config entries.
pub(crate) fn command_candidates(
    draft: &str,
    commands: &[(&'static str, String)],
) -> Vec<(String, String, String)> {
    if let Some(filter_text) = draft.strip_prefix("/language ") {
        return config::Language::available_codes()
            .into_iter()
            .filter(|code| code.starts_with(filter_text))
            .map(|code| (format!("/language {code}"), code.clone(), String::new()))
            .collect();
    }
    if let Some(filter_text) = draft.strip_prefix("/appearance ") {
        return config::Palette::available_names()
            .into_iter()
            .filter(|name| name.starts_with(filter_text))
            .map(|name| (format!("/appearance {name}"), name.clone(), String::new()))
            .collect();
    }
    let Some(prefix) = baihua_core::commands::pending_command_prefix(draft) else {
        return Vec::new();
    };
    commands
        .iter()
        .filter(|(name, _)| name.starts_with(prefix))
        .map(|(name, description)| (format!("/{name}"), format!("/{name}"), description.clone()))
        .collect()
}

/// The completion popup: one row per candidate with its description, the selected one
/// highlighted. It floats above the box, so opening it never moves the message area.
pub(crate) fn draw_completions(
    context: &Context,
    anchor: Rect,
    skin: &Skin,
    candidates: &[(String, String, String)],
    selected: usize,
    selection_moved: bool,
) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }
    let mut picked = None;
    // Stick above the input box; when there's not enough room above, stick below the input box edge, at least it won't run off the screen
    let above = anchor.top() - completion_height() - completion_gap();
    let position = Pos2::new(
        anchor.left(),
        if above >= 0.0 {
            above
        } else {
            anchor.bottom() + completion_gap()
        },
    );
    egui::Area::new(Id::new(completion_area_id()))
        .order(egui::Order::Foreground)
        .fixed_pos(position)
        .show(context, |ui| {
            ui.set_width(anchor.width());
            Frame::new()
                .fill(skin.app_background)
                .stroke(Stroke::new(1.0, skin.command_border))
                .corner_radius(4.0)
                .inner_margin(Margin::same(4))
                .show(ui, |ui| {
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(completion_height())
                        .show(ui, |ui| {
                            // The rectangle of the selected row: the scroll region uses it to pull the selected item back into view
                            let mut selected_row: Option<Rect> = None;
                            for (index, (insert_text, label, description)) in
                                candidates.iter().enumerate()
                            {
                                let is_selected = index == selected;
                                let name_color =
                                    selectable_color(skin, is_selected, skin.selected_text);
                                let row = ui.horizontal(|ui| {
                                    let row_button = Button::selectable(
                                        is_selected,
                                        egui::RichText::new(label.clone()).color(name_color),
                                    )
                                    .min_size(button_minimum_size(ui));
                                    if ui.add(row_button).clicked() {
                                        picked = Some(insert_text.clone());
                                    }
                                    if !description.is_empty() {
                                        ui.label(
                                            egui::RichText::new(description.clone())
                                                .color(skin.hint_text)
                                                .small(),
                                        );
                                    }
                                });
                                if is_selected {
                                    selected_row = Some(row.response.rect);
                                }
                            }
                            // An arrow-key move scrolls without animation, which
                            // would otherwise lag behind the key repeat.
                            if selection_moved && let Some(row) = selected_row {
                                ui.scroll_to_rect_animation(
                                    row,
                                    Some(Align::Center),
                                    ScrollAnimation::none(),
                                );
                            }
                        });
                });
        });
    picked
}

/// Whether Enter was pressed this frame, before any widget claims it. One rule covers
/// every platform now that `ios_platform` re-emits the soft Return as `Key::Enter`.
pub(crate) fn enter_activated(context: &Context) -> bool {
    context.input(|state| {
        state.events.iter().any(|event| {
            matches!(event,
                Event::Key { key: Key::Enter, pressed: true, modifiers, .. } if !modifiers.shift)
        })
    })
}

/// Claim this frame's Enter for exactly one widget; the completion popup consumes
/// it before the send check runs, so later checks can never see it.
pub(crate) fn consume_enter(context: &Context) -> bool {
    context.input_mut(|state| state.consume_key(Modifiers::NONE, Key::Enter))
}

/// Draw the message box and return (focused, send, draft changed, box rectangle). A
/// multiline `TextEdit` grows to its content, so the slot geometry here is explicit.
pub(crate) fn draw_message_input(
    ui: &mut Ui,
    draft: &mut String,
    placeholder: String,
    request_focus: bool,
    text_color: Color32,
    box_width: f32,
) -> (bool, bool, bool, Rect) {
    // The visible box: the width the caller left free and one input row tall, both
    // axes pinned because a multiline `TextEdit` sizes itself to its content.
    let box_size = Vec2::new(box_width, message_input_height());
    // Focus is read before the widget exists so the frame can be drawn with the
    // right stroke; the id is the persistent one the editor will use.
    let editor_id = ui.make_persistent_id(message_input_id());
    let editor_is_focused = ui.memory(|memory| memory.has_focus(editor_id));
    let box_look = message_input_look(ui, editor_is_focused);
    let text_margin = input_text_margin();
    let (box_rect, response) = ui
        .allocate_ui_with_layout(box_size, egui::Layout::top_down(Align::Min), |ui| {
            let slot = ui.max_rect();
            // The border belongs to the reserved slot, not to the editor.
            ui.painter().rect(
                slot,
                box_look.1,
                box_look.0,
                Stroke::new(box_look.2, box_look.3),
                StrokeKind::Middle,
            );
            let text_area = Rect::from_min_max(
                Pos2::new(
                    slot.left() + text_margin.leftf(),
                    slot.top() + text_margin.topf(),
                ),
                Pos2::new(
                    slot.right() - text_margin.rightf(),
                    slot.bottom() - text_margin.bottomf(),
                ),
            );
            // The empty frame still carries the text margin, which insets the glyphs
            // and the caret; without it the draft starts left of its own clip.
            let editor = TextEdit::multiline(draft)
                .text_color(text_color)
                .hint_text(placeholder)
                .id_source(message_input_id())
                .return_key(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter))
                .desired_rows(2)
                .desired_width(text_area.width() + text_margin.sum().x)
                .min_size(Vec2::ZERO)
                .frame(Frame::NONE.inner_margin(text_margin));
            // The editor sits in a scroll area sized to the text rectangle, so a taller
            // draft scrolls instead of growing the box; re-setting `max_rect` would freeze it.
            let editor_response = ScrollArea::vertical()
                .id_salt(input_scroll_id())
                .auto_shrink([false, false])
                // egui's default `min_scrolled_height` would grow this viewport
                // past the box, so the draft would never overflow and never scroll.
                .min_scrolled_height(text_area.height())
                .show(ui, |ui| {
                    // Clip to the text rectangle only; the editor's own frame
                    // margin puts its glyphs exactly inside that rectangle.
                    ui.set_clip_rect(text_area);
                    ui.add(editor)
                })
                .inner;
            (slot, editor_response)
        })
        .inner;
    if request_focus {
        response.request_focus();
        move_caret_to_end(ui.ctx(), response.id, draft);
    }
    // The shared helper decides whether Enter sends: it reads the key event's own
    // modifiers, so Shift+Enter stays a newline (see `enter_activated`).
    let enter_without_shift = enter_activated(ui.ctx());
    (
        response.has_focus(),
        response.has_focus() && enter_without_shift,
        response.changed(),
        box_rect,
    )
}

/// The box's own look (fill, corner radius, stroke width, stroke color), the values
/// egui gives a `TextEdit`. A tuple, because a `Frame` would size itself to content.
fn message_input_look(ui: &Ui, focused: bool) -> (Color32, CornerRadius, f32, Color32) {
    let inactive = &ui.visuals().widgets.inactive;
    let (stroke_width, stroke_color) = if focused {
        (
            ui.visuals().selection.stroke.width,
            ui.visuals().selection.stroke.color,
        )
    } else {
        (inactive.bg_stroke.width, inactive.bg_stroke.color)
    };
    (
        ui.visuals().text_edit_bg_color(),
        inactive.corner_radius,
        stroke_width,
        stroke_color,
    )
}

/// Space between the box border and the text inside it: the margin egui's own
/// `TextEdit` frame uses, so the caret and glyphs sit where they always did.
fn input_text_margin() -> Margin {
    Margin::symmetric(4, 2)
}

/// Stable id of the scroll viewport inside the message box (its scroll offset is
/// remembered across frames, so it needs one identity of its own).
pub(crate) fn input_scroll_id() -> &'static str {
    "message-input-scroll"
}

/// Move the cursor in the input box to the end of the text (useful for completing a command or after sending a message)
pub(crate) fn move_caret_to_end(context: &Context, id: Id, draft: &str) {
    let Some(mut state) = egui::widgets::text_edit::TextEditState::load(context, id) else {
        return;
    };
    let end = egui::text::CCursor::new(draft.chars().count());
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(end)));
    state.store(context, id);
}

/// All text and data the message input area needs to read and write (the input area closure doesn't touch self, so take it out first and write it back)
pub(crate) struct MessageInputView {
    /// Draft in the input box
    pub(crate) draft: String,
    /// Placeholder hint for the input box
    pub(crate) placeholder: String,
    /// Whether this frame should hand keyboard focus to the input box (only true for the frame just clicked in or just after sending a message)
    pub(crate) request_focus: bool,
    /// Which item is currently selected in the completion tooltip (up/down arrow switches)
    pub(crate) selected_command: usize,
    /// Label of the touch-platform send button (soft keyboards may never deliver
    /// Enter at all, so the button is the guaranteed path)
    pub(crate) send_title: String,
}

/// The input area's result this frame
pub(crate) struct MessageInputOutcome {
    /// Whether to send the draft (normal Enter with focus in the input box)
    pub(crate) send: bool,
    /// Whether the input box content changed this frame (search and fast search use this to decide whether to rescan)
    pub(crate) draft_changed: bool,
    /// In search mode, up/down arrow keys were pressed: Some(true) is the previous match, Some(false) is the next
    pub(crate) search_step: Option<bool>,
    /// Which complete text to fill into the input box (Enter to fill the selected completion, or click a row);
    /// command name completion is `/commandname`, and `/language` / `/appearance` parameter completion is `/commandname parameter`
    pub(crate) complete_command: Option<String>,
    /// Which rectangle the input box falls into this frame (the completion popup uses this to stick above the input box)
    pub(crate) input_rect: Rect,
    /// Does the message box own the keyboard focus this frame (mobile moves the
    /// whole input area to the top while it does, see `draw_conversation`)
    pub(crate) focused: bool,
}

/// The input area, laid out bottom-up: input row (settings gear beside it) first so it
/// sticks to the bottom, then the typing hint and completion popup above it.
pub(crate) fn draw_input_area(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    commands: &[(&'static str, String)],
    view: &mut MessageInputView,
) -> MessageInputOutcome {
    let mut outcome = MessageInputOutcome {
        send: false,
        draft_changed: false,
        search_step: None,
        complete_command: None,
        input_rect: Rect::NOTHING,
        focused: false,
    };
    // Command and parameter completion reacts to the draft; Enter fills the selected
    // candidate (and only executes directly when the input already equals it).
    let candidates = command_candidates(&view.draft, commands);
    // Whether the completion selection was changed with up/down arrow keys this frame: if changed, the completion list must scroll along
    let mut completion_selection_moved = false;
    if !candidates.is_empty() {
        view.selected_command = view.selected_command.min(candidates.len() - 1);
        if ui.input_mut(|state| state.consume_key(Modifiers::NONE, Key::ArrowUp)) {
            view.selected_command = view.selected_command.saturating_sub(1);
            completion_selection_moved = true;
        }
        if ui.input_mut(|state| state.consume_key(Modifiers::NONE, Key::ArrowDown)) {
            view.selected_command = (view.selected_command + 1).min(candidates.len() - 1);
            completion_selection_moved = true;
        }
        // When the content in the input box is already a candidate (command name fully typed, or the parameter is exactly some selectable value),
        // Enter is "execute"; otherwise Enter fills the selected candidate into the input box first.
        let typed_is_exact_candidate = candidates
            .iter()
            .any(|(insert_text, _, _)| insert_text == &view.draft);
        if !typed_is_exact_candidate && consume_enter(ui.ctx()) {
            outcome.complete_command = Some(candidates[view.selected_command].0.clone());
        }
    } else if view.draft.trim_start().starts_with('#') {
        // In `#` search mode the arrows switch matches instead of moving the caret:
        // consume the keys here so the text box never sees them.
        let backwards = ui.input_mut(|state| state.consume_key(Modifiers::NONE, Key::ArrowUp));
        let forwards = ui.input_mut(|state| state.consume_key(Modifiers::NONE, Key::ArrowDown));
        if backwards {
            outcome.search_step = Some(true);
        } else if forwards {
            outcome.search_step = Some(false);
        }
    }
    // The input row is drawn inside a fixed-height container first: a bottom_up
    // panel given an unsized inner block overflows downward and the panel grows each frame.
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), message_input_height()),
        egui::Layout::left_to_right(Align::Center),
        |ui| {
            // The send button's slot is reserved up front: letting the box take the
            // whole row pushed it past the panel and out of the message area.
            let row_width = ui.available_width();
            let box_width = (row_width - send_button_width() - ui.spacing().item_spacing.x)
                .at_least(minimum_input_width());
            let input = draw_message_input(
                ui,
                &mut view.draft,
                view.placeholder.clone(),
                view.request_focus,
                skin.input_text,
                box_width,
            );
            outcome.send = input.1;
            outcome.draft_changed = input.2;
            outcome.input_rect = input.3;
            outcome.focused = input.0;
            // The send button is drawn every frame and never gated on focus (a gated
            // click dies with it); an empty draft only leaves it visible but inert.
            let can_send = !view.draft.trim().is_empty();
            if icon_button_sized(
                ui,
                skin,
                icons,
                IconName::Proceed,
                Some(view.send_title.clone()),
                Vec2::new(send_button_width(), message_input_height()),
                can_send,
            )
            .clicked()
            {
                outcome.send = true;
            }
        },
    );
    // When the input changes, the selection is pulled back to the first item (same as terminal version: when the prefix changes, selection starts from the beginning)
    if outcome.draft_changed {
        view.selected_command = 0;
    }
    // The completion tooltip is drawn as a floating layer (does not occupy layout): it does not push the message area, clicking one item fills it into the input box
    if !candidates.is_empty()
        && let Some(picked) = draw_completions(
            ui.ctx(),
            outcome.input_rect,
            skin,
            &candidates,
            view.selected_command,
            completion_selection_moved,
        )
    {
        outcome.complete_command = Some(picked);
    }
    outcome
}

#[cfg(test)]
mod input_box_tests {
    use super::draw_message_input;
    use crate::app::test_support::{click_at, frame};
    use egui::{CentralPanel, Color32, Context, Event, Key, Modifiers, Pos2, RawInput, TextEdit};

    fn raw_input(events: Vec<Event>) -> RawInput {
        frame(800.0, 600.0, events)
    }

    fn key_event(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// Draw a decoy text box before the message box in one frame; returns
    /// (focus in message box, send?, message box center) measured, not hardcoded.
    fn run_one_frame(
        context: &Context,
        draft: &mut String,
        events: Vec<Event>,
        request_focus: bool,
    ) -> (bool, bool, Pos2) {
        let mut draft_copy = draft.clone();
        let mut input_has_focus = false;
        let mut send_requested = false;
        let mut input_center = Pos2::ZERO;
        context
            .run_ui(raw_input(events), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    ui.add(TextEdit::singleline(&mut draft_copy).hint_text("username"));
                    let body_top = ui.cursor().min;
                    let input = draw_message_input(
                        ui,
                        &mut draft_copy,
                        "type a message".to_string(),
                        request_focus,
                        Color32::WHITE,
                        ui.available_width(),
                    );
                    input_has_focus = input.0;
                    send_requested = input.1;
                    input_center = Pos2::new(ui.max_rect().center().x, body_top.y + 20.0);
                });
            })
            .drop_without_applying_deltas();
        *draft = draft_copy;
        (input_has_focus, send_requested, input_center)
    }

    /// After requesting focus, the input box acknowledges it received focus itself
    #[test]
    fn focus_puts_caret() {
        let context = Context::default();
        let mut draft = String::new();
        let (input_has_focus, send, _center) =
            run_one_frame(&context, &mut draft, Vec::new(), true);
        assert!(
            input_has_focus,
            "the frame that requests focus must show the box focused"
        );
        assert!(!send, "focusing alone must not send");
    }

    /// Mouse clicking the input box must receive focus; the GUI can't rely on keyboard alone
    #[test]
    fn click_focuses_box() {
        let context = Context::default();
        let mut draft = String::new();
        let (_focus, _send, center) = run_one_frame(&context, &mut draft, Vec::new(), false);
        let (input_has_focus, send, _center) =
            run_one_frame(&context, &mut draft, click_at(center), false);
        assert!(
            input_has_focus,
            "a click landing on the input box at {center:?} must focus it"
        );
        assert!(!send, "a plain click must not send");
    }

    /// This is a root-cause regression for "the input box is unusable": in a normal frame without requesting focus,
    /// the message input box must never steal focus back, otherwise the login, registration, and settings forms the user clicked on would all be untypeable
    #[test]
    fn focus_not_stolen() {
        let context = Context::default();
        let mut draft = String::new();
        let (_focus, _send, center) = run_one_frame(&context, &mut draft, Vec::new(), false);
        // Click the input box above (simulating a user clicking the username box on the login page)
        let other_point = Pos2::new(center.x, 12.0);
        run_one_frame(&context, &mut draft, click_at(other_point), false);
        let (input_has_focus, _send, _center) =
            run_one_frame(&context, &mut draft, Vec::new(), false);
        assert!(
            !input_has_focus,
            "with focus on another box the message input must not claim focus"
        );
    }

    /// After getting focus, typed characters go into the draft
    #[test]
    fn typed_in_draft() {
        let context = Context::default();
        let mut draft = String::new();
        run_one_frame(&context, &mut draft, Vec::new(), true);
        let (input_has_focus, send, _center) = run_one_frame(
            &context,
            &mut draft,
            vec![Event::Text("hi".to_string())],
            false,
        );
        assert!(
            input_has_focus,
            "after typing the focus must still be in the box"
        );
        assert!(!send, "typing must not send");
        assert!(
            draft.ends_with("hi"),
            "typed characters must stay in the draft, got {draft:?}"
        );
    }

    /// Enter sends and never inserts a newline, Shift+Enter inserts a newline
    /// without sending, and Enter with the caret elsewhere sends nothing.
    #[test]
    fn enter_rule() {
        let context = Context::default();
        let mut draft = String::new();
        run_one_frame(&context, &mut draft, Vec::new(), true);
        let (input_has_focus, send, _center) = run_one_frame(
            &context,
            &mut draft,
            vec![key_event(Key::Enter, Modifiers::NONE)],
            false,
        );
        assert!(
            input_has_focus,
            "after Enter the focus must stay in the box"
        );
        assert!(send, "Enter with the box focused must report send");
        assert!(
            !draft.contains('\n'),
            "Enter must not stuff a newline into the draft, got {draft:?}"
        );

        let mut draft = String::from("abc");
        run_one_frame(&context, &mut draft, Vec::new(), true);
        let (_focus, send, _center) = run_one_frame(
            &context,
            &mut draft,
            vec![key_event(Key::Enter, Modifiers::SHIFT)],
            false,
        );
        assert!(!send, "Shift+Enter is a newline and must not send");

        // Enter in another box must never send a chat message by accident, so this
        // half starts from a fresh context whose box was never focused.
        let unfocused = Context::default();
        let mut draft = String::from("abc");
        let (_focus, send, _center) = run_one_frame(
            &unfocused,
            &mut draft,
            vec![key_event(Key::Enter, Modifiers::NONE)],
            false,
        );
        assert!(!send, "without focus there must be no send");
    }
}

#[cfg(test)]
mod message_input_area_tests {
    use super::{
        MessageInputOutcome, MessageInputView, completion_gap, completion_height, draw_input_area,
    };
    use crate::app::icons::test_icons;
    use crate::app::test_support::{click_at, frame, position_of};
    use crate::appearance::Skin;
    use baihua_core::config::Palette;
    use egui::{
        CentralPanel, Context, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2,
    };

    fn raw_input(events: Vec<Event>) -> RawInput {
        frame(900.0, 600.0, events)
    }

    /// The data for drawing the input area: the draft is provided by the parameter, the rest uses fixed Chinese text,
    /// so that assertions can find hit points directly by text
    fn input_view(draft: &str) -> MessageInputView {
        MessageInputView {
            draft: draft.to_string(),
            placeholder: "type a message".to_string(),
            request_focus: false,
            selected_command: 0,
            send_title: "send".to_string(),
        }
    }

    /// Press a certain key (without modifier keys)
    fn key_event(key: Key) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    /// Command table: descriptions are in Chinese, used directly as lookup targets in assertions
    fn test_commands() -> Vec<(&'static str, String)> {
        vec![
            ("list_users", "list every registered user".to_string()),
            ("login", "open the login view".to_string()),
            ("logout", "sign out".to_string()),
            ("kick", "remove a group member".to_string()),
        ]
    }

    /// Draw the input area for one frame, return (the text and positions drawn this frame, the input area result, whether arrow keys are still in the event queue)
    fn run_one_frame(
        context: &Context,
        view: &mut MessageInputView,
        events: Vec<Event>,
    ) -> (Vec<(String, Pos2)>, MessageInputOutcome, bool) {
        run_command_frame(context, view, events, test_commands())
    }

    /// Same as `run_one_frame` except the command table is provided by the parameter:
    /// Testing "when the completion list doesn't fit on one screen it scrolls by itself" needs a sufficiently long command table
    fn run_command_frame(
        context: &Context,
        view: &mut MessageInputView,
        events: Vec<Event>,
        commands: Vec<(&'static str, String)>,
    ) -> (Vec<(String, Pos2)>, MessageInputOutcome, bool) {
        // First warm up a frame: the completion tooltip is a floating layer (`Area` + scroll area), the first frame only does size probing,
        // The text inside hasn't landed on the layer yet; in the real interface it's always drawn continuously, so here also warm up one frame first
        draw_command_frame(context, view, Vec::new(), commands.clone());
        draw_command_frame(context, view, events, commands)
    }

    /// Draw the input area for one frame and collect the text drawn this frame (command table provided by parameter)
    fn draw_command_frame(
        context: &Context,
        view: &mut MessageInputView,
        events: Vec<Event>,
        commands: Vec<(&'static str, String)>,
    ) -> (Vec<(String, Pos2)>, MessageInputOutcome, bool) {
        let skin = Skin::from(&Palette::built_in());
        let mut outcome = MessageInputOutcome {
            send: false,
            draft_changed: false,
            search_step: None,
            complete_command: None,
            input_rect: Rect::NOTHING,
            focused: false,
        };
        let mut arrow_survived = false;
        let mut painted: Vec<(String, Pos2)> = Vec::new();
        context
            .run_ui(raw_input(events), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    // The input area in the real interface is laid out from bottom to top; here lay it out the same way,
                    // so you can accurately measure "which row is at the very bottom"
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                        let icons = test_icons(ui.ctx());
                        outcome = draw_input_area(ui, &skin, &icons, &commands, view);
                    });
                });
                // After the input area is drawn, are the arrow keys still in this frame's event queue?
                // (if they stay, it means the input box layer still sees them and the cursor would move)
                arrow_survived = ctx.input(|state| {
                    state.events.iter().any(|event| {
                        matches!(
                            event,
                            Event::Key {
                                key: Key::ArrowUp | Key::ArrowDown,
                                pressed: true,
                                ..
                            }
                        )
                    })
                });
                painted = ctx.graphics_mut(|graphics| {
                    let mut texts: Vec<(String, Pos2)> = Vec::new();
                    // Text in the input area is on the background layer; the completion tooltip is a floating layer (`egui::Area`),
                    // painted on its own `Order::Foreground` layer, need to read it separately
                    for layer in [
                        egui::LayerId::background(),
                        egui::LayerId::new(
                            egui::Order::Foreground,
                            egui::Id::new(super::completion_area_id()),
                        ),
                    ] {
                        if let Some(list) = graphics.get(layer) {
                            for entry in list.all_entries() {
                                if let egui::Shape::Text(text_shape) = &entry.shape {
                                    texts.push((
                                        text_shape.galley.text().to_string(),
                                        text_shape.pos,
                                    ));
                                }
                            }
                        }
                    }
                    texts
                });
            })
            .drop_without_applying_deltas();
        (painted, outcome, arrow_survived)
    }

    /// In search mode the up and down arrows switch matches and are consumed
    /// before the input box; outside search mode they stay with the box.
    #[test]
    fn arrow_key_owner() {
        let context = Context::default();
        let mut view = input_view("#keyword");
        let (_texts, outcome, arrow_survived) =
            run_one_frame(&context, &mut view, vec![key_event(Key::ArrowUp)]);
        assert_eq!(
            outcome.search_step,
            Some(true),
            "in search mode the up arrow must report stepping to the previous match"
        );
        assert!(
            !arrow_survived,
            "the up arrow must be consumed by the input area, not left to move a caret"
        );

        let mut view = input_view("plain message");
        let (_texts, outcome, arrow_survived) =
            run_one_frame(&context, &mut view, vec![key_event(Key::ArrowUp)]);
        assert_eq!(
            outcome.search_step, None,
            "plain input must not capture the up and down arrow keys"
        );
        assert!(
            arrow_survived,
            "in plain input the arrow must be left for the box to move its caret"
        );
    }

    /// The arrows move the completion selection and are consumed before the box,
    /// and the selection survives across frames (written back from the view).
    #[test]
    fn completion_arrows() {
        let context = Context::default();
        let mut view = input_view("/l");
        let (_texts, _outcome, arrow_survived) =
            run_one_frame(&context, &mut view, vec![key_event(Key::ArrowDown)]);
        assert_eq!(
            view.selected_command, 1,
            "one down arrow must select the second candidate"
        );
        assert!(
            !arrow_survived,
            "while the list is open the arrow must be consumed, not left for a caret move"
        );
        run_one_frame(&context, &mut view, vec![key_event(Key::ArrowUp)]);
        run_one_frame(&context, &mut view, vec![key_event(Key::ArrowUp)]);
        assert_eq!(
            view.selected_command, 0,
            "the up arrow stops at the first entry instead of going out of bounds"
        );

        // The interface loop: stored selection, a new view, one frame, write back.
        let mut stored_selection = 0;
        for events in [vec![key_event(Key::ArrowDown)], Vec::new()] {
            let mut view = input_view("/l");
            view.selected_command = stored_selection;
            run_one_frame(&context, &mut view, events);
            stored_selection = view.selected_command;
        }
        assert_eq!(
            stored_selection, 1,
            "after one down arrow the next frame's list must rest on the second entry"
        );
    }

    /// The arrow keys must scroll the completion popup so the selected row stays
    /// visible; before the fix the highlight could leave the fixed-height box.
    #[test]
    fn completion_scrolls() {
        let context = Context::default();
        let commands = long_command_list();
        let mut view = input_view("/l");
        for _ in 0..10 {
            run_command_frame(
                &context,
                &mut view,
                vec![key_event(Key::ArrowDown)],
                commands.clone(),
            );
        }
        assert_eq!(
            view.selected_command, 10,
            "ten presses must land on the eleventh entry"
        );
        // egui applies a scroll target on the next frame, so draw one before measuring.
        let (texts, outcome, _arrow) =
            run_command_frame(&context, &mut view, Vec::new(), commands.clone());
        let completion_top = outcome.input_rect.top() - completion_height() - completion_gap();
        let completion_bottom = completion_top + completion_height();
        let selected_position = position_of(&texts, "/l10");
        assert!(
            selected_position.y > completion_top && selected_position.y < completion_bottom,
            "the selected row must stay inside the popup band {completion_top}..{completion_bottom}, got {selected_position:?}"
        );
        // Text that scrolled out of the band is simply not drawn, so both "absent"
        // and "above the top edge" count as having scrolled out.
        let first_position = texts
            .iter()
            .find(|(text, _)| text == "/l0")
            .map(|(_, position)| *position);
        assert!(
            first_position.is_none_or(|position| position.y < completion_top),
            "after scrolling the first row must sit above the popup top {completion_top}, got {first_position:?}"
        );
    }

    /// Enter fills the selected entry into the box while the command name is
    /// incomplete, and sends normally once the name is a complete command.
    #[test]
    fn completion_enter() {
        let context = Context::default();
        let mut view = input_view("/k");
        view.request_focus = true;
        let (_texts, outcome, _arrow) =
            run_one_frame(&context, &mut view, vec![key_event(Key::Enter)]);
        assert_eq!(
            outcome.complete_command,
            Some("/kick".to_string()),
            "Enter must fill the selected /kick into the input box"
        );
        assert!(
            !outcome.send,
            "the completion Enter must not send the draft at the same time"
        );

        let mut view = input_view("/info");
        view.request_focus = true;
        let (_texts, outcome, _arrow) =
            run_one_frame(&context, &mut view, vec![key_event(Key::Enter)]);
        assert_eq!(
            outcome.complete_command, None,
            "a complete command name has nothing left to complete"
        );
        assert!(outcome.send, "a complete command name sends on Enter");
    }

    /// The list shows only the commands matching the prefix, each on its own row in
    /// table order with its description, and closes once an argument is typed.
    #[test]
    fn completion_list() {
        let context = Context::default();
        let mut view = input_view("/l");
        let (texts, _outcome, _arrow) = run_one_frame(&context, &mut view, Vec::new());
        let drawn: Vec<String> = texts.iter().map(|(text, _)| text.clone()).collect();
        for expected in ["/list_users", "/login", "/logout"] {
            assert!(
                drawn.iter().any(|text| text == expected),
                "typing /l must suggest {expected:?}; drew {drawn:?}"
            );
        }
        assert!(
            !drawn.iter().any(|text| text == "/kick"),
            "a non-matching command must not appear; drew {drawn:?}"
        );
        for expected in [
            "list every registered user",
            "open the login view",
            "sign out",
        ] {
            assert!(
                drawn.iter().any(|text| text == expected),
                "every row must carry its description {expected:?}; drew {drawn:?}"
            );
        }
        let entries: Vec<Pos2> = ["/login", "/logout", "/list_users"]
            .iter()
            .map(|name| position_of(&texts, name))
            .collect();
        for (index, first) in entries.iter().enumerate() {
            for second in entries.iter().skip(index + 1) {
                assert!(
                    (first.y - second.y).abs() > 1.0,
                    "each entry must sit on its own row; these two share one: {first:?} and {second:?}"
                );
            }
        }
        let mut sorted: Vec<(f32, &str)> = ["/login", "/logout", "/list_users"]
            .iter()
            .map(|name| (position_of(&texts, name).y, *name))
            .collect();
        sorted.sort_by(|left, right| {
            left.0
                .partial_cmp(&right.0)
                .expect("coordinates are never NaN")
        });
        assert_eq!(
            sorted.iter().map(|(_, name)| *name).collect::<Vec<&str>>(),
            vec!["/list_users", "/login", "/logout"],
            "the list must follow the command table from top to bottom; got {sorted:?}"
        );

        // Once an argument is being typed the list must stop covering the input.
        let mut view = input_view("/kick somebody");
        let (texts, _outcome, _arrow) = run_one_frame(&context, &mut view, Vec::new());
        let drawn: Vec<String> = texts.iter().map(|(text, _)| text.clone()).collect();
        assert!(
            !drawn.iter().any(|text| text == "/kick"),
            "commands must not be listed while an argument is typed; drew {drawn:?}"
        );
    }

    /// Clicking one item in the completion list: should report "complete into the input box" (not execute the command directly)
    #[test]
    fn completion_click() {
        let context = Context::default();
        let mut view = input_view("/log");
        let (texts, _outcome, _arrow) = run_one_frame(&context, &mut view, Vec::new());
        let entry = position_of(&texts, "/logout");
        let (_texts, outcome, _arrow) =
            run_one_frame(&context, &mut view, click_at(entry + Vec2::new(4.0, 8.0)));
        assert_eq!(
            outcome.complete_command,
            Some("/logout".to_string()),
            "Clicking /logout in the completion list should \"complete into the input box\", not execute directly"
        );
    }

    /// `/language ` and `/appearance ` list the config entries, keep filtering by a
    /// case-sensitive prefix, and Enter fills the whole line. Skipped without config.
    #[test]
    fn argument_completion() {
        let context = Context::default();
        let languages = baihua_core::config::Language::available_codes();
        let appearances = baihua_core::config::Palette::available_names();
        if languages.is_empty() || appearances.is_empty() {
            return;
        }
        for (draft, expected) in [
            ("/language ", languages.clone()),
            ("/appearance ", appearances.clone()),
        ] {
            let mut view = input_view(draft);
            let (texts, _outcome, _arrow) = run_one_frame(&context, &mut view, Vec::new());
            let drawn: Vec<String> = texts.iter().map(|(text, _)| text.clone()).collect();
            for candidate in &expected {
                assert!(
                    drawn.iter().any(|text| text == candidate),
                    "{draft} must list {candidate:?}; drew {drawn:?}"
                );
            }
        }

        let Some(sample) = languages.first() else {
            return;
        };
        let Some(first_character) = sample.chars().next() else {
            return;
        };
        let mut view = input_view(&format!("/language {first_character}"));
        let (texts, _outcome, _arrow) = run_one_frame(&context, &mut view, Vec::new());
        let drawn: Vec<String> = texts.iter().map(|(text, _)| text.clone()).collect();
        assert!(
            drawn.iter().any(|text| text == sample),
            "a verbatim prefix must match the language code {sample:?}; drew {drawn:?}"
        );
        let mut upper_view = input_view(&format!("/language {}", first_character.to_uppercase()));
        upper_view.selected_command = 0;
        let (upper_texts, _, _) = run_one_frame(&context, &mut upper_view, Vec::new());
        let upper_drawn: Vec<String> = upper_texts.iter().map(|(text, _)| text.clone()).collect();
        assert!(
            !upper_drawn.iter().any(|text| text == sample),
            "an uppercase prefix must not match {sample:?} (case sensitive); drew {upper_drawn:?}"
        );

        let mut view = input_view("/language ");
        view.request_focus = true;
        let (_texts, outcome, _arrow) =
            run_one_frame(&context, &mut view, vec![key_event(Key::Enter)]);
        assert_eq!(
            outcome.complete_command,
            Some(format!("/language {sample}")),
            "Enter must fill the whole `/language <code>` line into the box"
        );
    }

    /// Selected completion rows paint `selection_background`, so their text takes the
    /// contrasting color (both were yellow in the default theme).
    #[test]
    fn completion_contrast() {
        fn brightness(color: egui::Color32) -> u32 {
            (color.r() as u32 * 299 + color.g() as u32 * 587 + color.b() as u32 * 114) / 1000
        }
        let skin = Skin::from(&Palette::built_in());
        let selected_color = super::selectable_color(&skin, true, skin.selected_text);
        assert!(
            brightness(selected_color).abs_diff(brightness(skin.selection_background)) >= 128,
            "the selected item text and highlight background must be distinguishable: text {selected_color:?}, background {:?}",
            skin.selection_background
        );
        assert_eq!(
            super::selectable_color(&skin, false, skin.selected_text),
            skin.selected_text,
            "unselected rows keep the caller's color untouched"
        );
    }

    /// A sufficiently long command table: 12 commands with the same prefix, one screen doesn't fit (the popup's max height is `completion_height()`),
    /// used to verify "the completion list scrolls by itself when pressing up/down arrow keys"
    fn long_command_list() -> Vec<(&'static str, String)> {
        vec![
            ("l0", "entry 0".to_string()),
            ("l1", "entry 1".to_string()),
            ("l2", "entry 2".to_string()),
            ("l3", "entry 3".to_string()),
            ("l4", "entry 4".to_string()),
            ("l5", "entry 5".to_string()),
            ("l6", "entry 6".to_string()),
            ("l7", "entry 7".to_string()),
            ("l8", "entry 8".to_string()),
            ("l9", "entry 9".to_string()),
            ("l10", "entry 10".to_string()),
            ("l11", "entry 11".to_string()),
        ]
    }

    /// The send button must survive the click that presses it and stay drawn while
    /// the box is unfocused, since the row reserves its slot in every state.
    #[test]
    fn send_button_click() {
        let context = Context::default();
        let mut view = input_view("hello there");
        // Unfocused from the start: the button must still be painted.
        let (texts, outcome, _) = run_one_frame(&context, &mut view, Vec::new());
        assert!(!outcome.focused, "this case starts without focus");
        let button_position = texts
            .iter()
            .find(|(text, _)| text == "send")
            .map(|(_, position)| *position);
        assert!(
            button_position.is_some(),
            "the send button must be painted even while the box is unfocused"
        );
        // Press and release in SEPARATE frames (a real click): the button has
        // to still exist on release for `clicked()` to fire.
        let button_center = Pos2::new(
            button_position.unwrap().x + 20.0,
            button_position.unwrap().y + 5.0,
        );
        let (texts_after_press, _outcome_after_press, _) = run_one_frame(
            &context,
            &mut view,
            vec![Event::PointerButton {
                pos: button_center,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }],
        );
        assert!(
            texts_after_press.iter().any(|(text, _)| text == "send"),
            "pressing the button must not make it vanish mid-click"
        );
        let (_texts, outcome, _) = run_one_frame(
            &context,
            &mut view,
            vec![Event::PointerButton {
                pos: button_center,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        );
        assert!(
            outcome.send,
            "a press+release on the send button must ask for a send"
        );
    }

    /// An empty draft keeps the send button drawn (the row must not jump) yet the
    /// button stays inert: a full press+release on it asks for no send.
    #[test]
    fn inert_send_button() {
        let context = Context::default();
        let mut view = input_view("   ");
        let (texts, outcome, _) = run_one_frame(&context, &mut view, Vec::new());
        assert!(!outcome.send, "a frame without any click must not send");
        let button_position = texts
            .iter()
            .find(|(text, _)| text == "send")
            .map(|(_, position)| *position)
            .expect("the send button must stay drawn while the draft is empty");
        let center = Pos2::new(button_position.x + 20.0, button_position.y + 5.0);
        let pointer = |pressed: bool| Event::PointerButton {
            pos: center,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        run_one_frame(&context, &mut view, vec![pointer(true)]);
        let (_texts, outcome, _) = run_one_frame(&context, &mut view, vec![pointer(false)]);
        assert!(
            !outcome.send,
            "a click on the inert send button must not send"
        );
    }

    /// A draft taller than the box must scroll with the wheel: the default
    /// `min_scrolled_height` grew the viewport past the box, so nothing overflowed.
    #[test]
    fn draft_scrolls() {
        let context = Context::default();
        let mut view = input_view("alpha line\nbravo line\ncharlie line\ndelta line\necho line");
        let (texts, _outcome, _) = run_one_frame(&context, &mut view, Vec::new());
        let first_line = texts
            .iter()
            .find(|(text, _)| text.starts_with("alpha line"))
            .map(|(_, position)| position.y)
            .expect("the first draft line must be painted");
        let wheel = Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -30.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        };
        let mut moved = None;
        for _frame in 0..6 {
            let (texts, _outcome, _) = run_one_frame(
                &context,
                &mut view,
                vec![
                    // Hover over the draft itself: the wheel only scrolls the
                    // area the pointer is inside.
                    Event::PointerMoved(Pos2::new(300.0, first_line + 5.0)),
                    wheel.clone(),
                ],
            );
            if let Some(position) = texts
                .iter()
                .find(|(text, _)| text.starts_with("alpha line"))
                .map(|(_, position)| position.y)
            {
                moved = Some(position);
            }
        }
        let scrolled_top = moved.expect("the draft must stay painted while scrolling");
        assert!(
            scrolled_top < first_line,
            "the wheel must move the draft up: {first_line} -> {scrolled_top}"
        );
    }

    /// The draft origin must never sit left of its own clip, or the first strokes
    /// and the zero-position caret are cut away (the reported left-side clipping).
    #[test]
    fn draft_not_clipped() {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let commands: Vec<(&'static str, String)> = Vec::new();
        let mut view = input_view("line one\nline two\nline three\nline four");
        view.request_focus = true;
        let mut draft_clip = Rect::NOTHING;
        let mut draft_origin = Pos2::ZERO;
        for _frame in 0..6 {
            context
                .run_ui(raw_input(Vec::new()), |ctx| {
                    CentralPanel::default().show(ctx, |ui| {
                        draw_input_area(ui, &skin, &test_icons(ui.ctx()), &commands, &mut view);
                    });
                    let (found_clip, found_origin) = ctx.graphics_mut(|graphics| {
                        let mut clip = Rect::NOTHING;
                        let mut origin = Pos2::ZERO;
                        if let Some(list) = graphics.get(egui::LayerId::background()) {
                            for entry in list.all_entries() {
                                if let egui::Shape::Text(text_shape) = &entry.shape
                                    && text_shape.galley.text().starts_with("line one")
                                {
                                    clip = entry.clip_rect;
                                    origin = text_shape.pos;
                                }
                            }
                        }
                        (clip, origin)
                    });
                    draft_clip = found_clip;
                    draft_origin = found_origin;
                })
                .drop_without_applying_deltas();
        }
        assert!(draft_clip.is_positive(), "the draft must be painted");
        assert!(
            draft_origin.x + 0.5 >= draft_clip.min.x,
            "the draft origin {draft_origin:?} must not sit left of its clip {draft_clip:?}",
        );
    }
}

#[cfg(test)]
mod input_area_growth_tests {
    use super::{MessageInputView, draw_conversation, draw_input_area, message_input_height};
    use crate::app::icons::test_icons;
    use crate::appearance::Skin;
    use baihua_core::config::Palette;
    use egui::{CentralPanel, Context, Pos2, RawInput, Rect, Ui, Vec2};

    fn raw_input() -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 600.0))),
            ..Default::default()
        }
    }

    fn input_view() -> MessageInputView {
        MessageInputView {
            draft: String::new(),
            placeholder: "type a message".to_string(),
            request_focus: false,
            selected_command: 0,
            send_title: "send".to_string(),
        }
    }

    /// Draw a conversation area for one frame, return the height the input area panel got this frame
    fn area_height_frame(context: &Context, view: &mut MessageInputView) -> f32 {
        let skin = Skin::from(&Palette::built_in());
        let commands: Vec<(&'static str, String)> =
            vec![("info", "view group chat information".to_string())];
        let mut height = 0.0;
        context
            .run_ui(raw_input(), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    let _ = draw_conversation(
                        ui,
                        &skin,
                        (Vec::new(), "no messages yet".to_string(), None),
                        true,
                        None,
                        false,
                        |ui: &mut Ui| {
                            // The input area is drawn inside the panel content; its available height is whatever the panel remembered
                            height = ui.max_rect().height();
                            draw_input_area(ui, &skin, &test_icons(ui.ctx()), &commands, view)
                        },
                    );
                });
            })
            .drop_without_applying_deltas();
        height
    }

    /// The input panel height must be stable from the second frame on and a long
    /// draft must not grow it: an egui panel is grow-only, so growth never ends.
    #[test]
    fn area_height_stable() {
        let context = Context::default();
        let mut view = input_view();
        let heights: Vec<f32> = (0..5)
            .map(|_| area_height_frame(&context, &mut view))
            .collect();
        for (index, height) in heights.iter().enumerate().skip(1) {
            assert_eq!(
                *height, heights[1],
                "the input area height must be stable from the second frame, frame {index} is {height} (all: {heights:?})"
            );
        }
        assert!(
            heights[1] < 200.0,
            "the input area must not keep growing taller, got {} (all: {heights:?})",
            heights[1]
        );

        let mut view = input_view();
        view.draft = "w".repeat(300);
        let heights: Vec<f32> = (0..12)
            .map(|_| area_height_frame(&context, &mut view))
            .collect();
        assert_eq!(
            heights[0],
            heights[heights.len() - 1],
            "a long draft must not change the input area height (all: {heights:?})"
        );
    }

    /// The visible box stays one input row tall whatever the draft, and the painted
    /// border must be the rectangle the caller was told about, clipping the draft.
    #[test]
    fn input_box_geometry() {
        let context = Context::default();
        let expected = area_height_frame(&context, &mut input_view());
        for draft in [String::new(), "hi".to_string(), "w".repeat(300)] {
            let mut view = input_view();
            view.draft = draft.clone();
            let height = area_height_frame(&context, &mut view);
            assert!(
                (height - expected).abs() < 1.0,
                "the input box must stay {expected} tall for a {}-char draft, got {height}",
                draft.len()
            );
        }

        for draft in [
            String::new(),
            "hi".to_string(),
            "line one\nline two\nline three\nline four".to_string(),
            "w".repeat(400),
        ] {
            let context = Context::default();
            let skin = Skin::from(&Palette::built_in());
            let commands: Vec<(&'static str, String)> = Vec::new();
            let mut view = input_view();
            view.draft = draft.clone();
            view.request_focus = true;
            let mut reported = Rect::NOTHING;
            let mut border = Rect::NOTHING;
            let mut draft_clip = Rect::NOTHING;
            for _frame in 0..6 {
                context
                    .run_ui(raw_input(), |ctx| {
                        CentralPanel::default().show(ctx, |ui| {
                            let outcome = draw_input_area(
                                ui,
                                &skin,
                                &test_icons(ui.ctx()),
                                &commands,
                                &mut view,
                            );
                            reported = outcome.input_rect;
                        });
                        let (border_rect, draft_rect) = ctx.graphics_mut(|graphics| {
                            let mut border = Rect::NOTHING;
                            let mut draft_clip = Rect::NOTHING;
                            if let Some(list) = graphics.get(egui::LayerId::background()) {
                                for entry in list.all_entries() {
                                    if let egui::Shape::Rect(rect_shape) = &entry.shape
                                        && (rect_shape.rect.height() - message_input_height()).abs()
                                            < 1.0
                                        && rect_shape.rect.width() > 500.0
                                    {
                                        border = rect_shape.rect;
                                    } else if let egui::Shape::Text(text) = &entry.shape
                                        && text.galley.text().starts_with("line one")
                                    {
                                        draft_clip = entry.clip_rect;
                                    }
                                }
                            }
                            (border, draft_clip)
                        });
                        border = border_rect;
                        draft_clip = draft_rect;
                    })
                    .drop_without_applying_deltas();
            }
            assert!(
                (border.height() - message_input_height()).abs() < 1.0,
                "a {}-char draft must not change the painted border height: {}",
                draft.len(),
                border.height()
            );
            assert!(
                (border.size() - reported.size()).length() < 1.0
                    && (border.min - reported.min).length() < 1.0,
                "the painted border {border:?} must be the box the caller was told about {reported:?}"
            );
            if !draft_clip.is_positive() {
                continue; // this draft has no "line one" text to check
            }
            assert!(
                reported.contains(draft_clip.min) && reported.contains(draft_clip.max),
                "the draft must be clipped inside the box {reported:?}, clip was {draft_clip:?}"
            );
        }
    }
}
