//! The standalone message search panel and the painted magnifier button that
//! opens it (group rooms only).

use super::*;

/// Everything the standalone search panel reads and writes for one frame (the window closure
/// never touches `self`, so everything is taken out first and written back afterwards).
pub(crate) struct SearchPanelView {
    /// Text currently in the keyword box (read into the box, written back as the draft)
    pub(crate) keyword: String,
    /// Window title
    pub(crate) title: String,
    /// Label of the confirm button beside the keyword box; the iOS soft keyboard
    /// never delivers Enter, so the button is the phone's guaranteed commit path.
    pub(crate) confirm_title: String,
    /// Placeholder shown while the box is empty
    pub(crate) placeholder: String,
    /// Whether quick search is on (decides if the list follows every keystroke and every message)
    pub(crate) quick_search: bool,
    /// Row under the box when quick search is off: results only move on Enter
    pub(crate) enter_hint: String,
    /// Shown when there is nothing to list (no keyword, no Enter yet, or no match at all)
    pub(crate) empty_hint: String,
    /// Hover text of a result block: clicking it jumps the message area
    pub(crate) jump_hint: String,
    /// Whether this frame should put the keyboard caret into the keyword box (true on the frame
    /// the panel was just opened; taken from the app exactly like the message box's focus flag)
    pub(crate) request_focus: bool,
    /// The matched messages, already built as rows - same layout as in the conversation
    pub(crate) rows: Vec<MessageRow>,
    /// What the user did in the panel this frame (the caller drives the session layer with it)
    pub(crate) outcome: SearchPanelOutcome,
}

/// The user action the standalone search panel took this frame.
#[derive(Debug, PartialEq)]
pub(crate) enum SearchPanelOutcome {
    /// Plain frame: nothing was submitted and no result block was clicked
    Nothing,
    /// Enter was pressed while the keyword box had focus: commit a search for this text
    Submitted(String),
    /// A result block was clicked: scroll the message area to this message
    JumpToMessage(String),
}

/// The magnifier button beside the group settings button: the embedded search
/// image, sized by the shared icon-button metrics so both buttons match exactly.
pub(crate) fn draw_search_button(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    hint: String,
) -> Response {
    icon_button(ui, skin, icons, IconName::Search, None)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hint)
}

/// Panel content: keyword box (Enter commits), the quick-search-off hint, and the
/// scrollable result list. The first submission/click of the frame wins.
pub(crate) fn draw_panel_body(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    view: &mut SearchPanelView,
) {
    // One row: right-to-left pins the confirm button at the row's right end (drawn
    // even while the keyword is empty, visible but inert) and lets the box eat the rest.
    let can_commit = !view.keyword.trim().is_empty();
    let (input, button_commit) = ui
        .horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let clicked = icon_button_enabled(
                    ui,
                    skin,
                    icons,
                    IconName::Confirm,
                    Some(view.confirm_title.clone()),
                    can_commit,
                )
                .clicked();
                let keyword_input = ui.add(
                    TextEdit::singleline(&mut view.keyword)
                        .hint_text(view.placeholder.clone())
                        .id(Id::new(search_keyword_id()))
                        .desired_width(ui.available_width()),
                );
                (keyword_input, clicked)
            })
            .inner
        })
        .inner;
    // Opening the panel puts the caret in the box, so Enter works without a click.
    if view.request_focus {
        input.request_focus();
    }
    // The box surrenders focus on Enter itself, so commit on "lost focus this frame
    // plus Enter queued" and never re-grab focus: that re-summons the phone keyboard.
    let keyboard_commit =
        input.lost_focus() && ui.input_mut(|state| state.consume_key(Modifiers::NONE, Key::Enter));
    // iOS never fires Key::Enter and keeps the focus after a soft Return, so commit
    // on the remembered press too; `consume_enter` eats the "\n" for everyone else.
    let soft_commit = !keyboard_commit && input.has_focus() && consume_enter(ui.ctx());
    // (The button was already added on the keyword box's row above; phones may
    // never deliver Enter at all (see `enter_activated`), and desktops lose nothing.)
    let has_keyword = !view.keyword.trim().is_empty();
    if (keyboard_commit || soft_commit || button_commit) && has_keyword {
        view.outcome = SearchPanelOutcome::Submitted(view.keyword.clone());
    }
    if !view.quick_search {
        ui.colored_label(skin.hint_text, view.enter_hint.clone());
    }
    ui.separator();
    // Rows are taken out of the view because the scroll closure writes the outcome through it.
    let rows = std::mem::take(&mut view.rows);
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if rows.is_empty() {
                ui.colored_label(skin.hint_text, view.empty_hint.clone());
            }
            // The conversation's virtualization: only the visible blocks (plus one
            // screen of slack) are built, the rest keep their remembered height.
            let jumped = draw_visible_rows(
                ui,
                rows,
                None,
                |ui, row, _scroll| draw_result_block(ui, skin, row, view.jump_hint.clone()),
                search_height_scope(),
            );
            if let Some(message_id) = jumped.into_iter().next()
                && matches!(view.outcome, SearchPanelOutcome::Nothing)
            {
                view.outcome = SearchPanelOutcome::JumpToMessage(message_id);
            }
        });
}

/// One result block: the message row inside a clickable rectangle, returning the
/// message id on a click. Avatar clicks are swallowed here: a click means "jump".
pub(crate) fn draw_result_block(
    ui: &mut Ui,
    skin: &Skin,
    row: MessageRow,
    jump_hint: String,
) -> Option<String> {
    let message_id = row.message_id.clone();
    let block_id = ui.make_persistent_id(("search-result-block", message_id.clone()));
    let block = Frame::new()
        .fill(skin.app_background)
        .stroke(Stroke::new(1.0, skin.message_border))
        .corner_radius(6.0)
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            draw_message_row(ui, skin, row, false);
        });
    let clicked = ui
        .interact(block.response.rect, block_id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(jump_hint)
        .clicked();
    clicked.then_some(message_id)
}

impl BaihuaApp {
    /// Read everything the panel draws this frame in one go, since the window
    /// closure never touches `self`; the match ids come from the session layer.
    pub(crate) fn search_panel_view(&mut self, context: &Context) -> SearchPanelView {
        let match_ids = self.client.panel_match_ids(&self.search_panel_keyword);
        let rows = self.search_panel_rows(context, &match_ids);
        SearchPanelView {
            confirm_title: self.text("button_confirm"),
            keyword: self.search_panel_keyword.clone(),
            title: self.text("search_panel_title"),
            placeholder: self.text("search_panel_placeholder"),
            quick_search: self.client.quick_search,
            enter_hint: self.text("search_panel_enter_hint"),
            empty_hint: self.text("search_panel_empty"),
            jump_hint: self.text("search_panel_jump_hint"),
            request_focus: std::mem::take(&mut self.focus_search_panel_input),
            rows,
            outcome: SearchPanelOutcome::Nothing,
        }
    }

    /// The search panel: a floating overlay window with the keyword box on top and
    /// the result blocks below. Clicking a block asks for a one-frame scroll.
    pub(crate) fn draw_search_panel(&mut self, context: &Context) {
        if !self.search_panel_open {
            return;
        }
        let skin = self.skin.clone();
        let icons = self.frame_icons(context);
        let mut view = self.search_panel_view(context);
        let mut keep_open = true;
        Window::new(view.title.clone())
            .id(Id::new(search_window_id()))
            .open(&mut keep_open)
            // Resizable and collapsible, like the other floating layers.
            .resizable(true)
            .collapsible(true)
            .default_pos([220.0, 140.0])
            .default_size([search_panel_width(), search_panel_height()])
            .frame(overlay_window_frame(&skin))
            .show(context, |ui| {
                draw_panel_body(ui, &skin, &icons, &mut view);
            });
        self.search_panel_open = keep_open;
        // The draft is written back before the outcome is applied, so a commit reads
        // this frame's text and the typed value still survives to the next one.
        self.search_panel_keyword = view.keyword.clone();
        match view.outcome {
            SearchPanelOutcome::Nothing => {}
            SearchPanelOutcome::Submitted(keyword) => {
                // Enter commits a search; with quick search on the panel recomputes per frame
                // anyway, so a commit there just refreshes the stored pair.
                self.client.run_panel_search(&keyword);
            }
            SearchPanelOutcome::JumpToMessage(message_id) => {
                // The conversation consumes this like a search-match step: the
                // stick-to-bottom lifts and the row scrolls into the middle.
                self.client.pending_scroll_message_id = Some(message_id);
                context.request_repaint();
            }
        }
    }
}

#[cfg(test)]
mod search_panel_tests {
    use super::{
        SearchPanelOutcome, SearchPanelView, Skin, draw_panel_body, draw_search_button,
        search_panel_height, search_panel_width,
    };
    use crate::app::icons::{IconName, icon_button, test_icons};
    use crate::app::test_support::frame;
    use baihua_core::config::Palette;
    use egui::{CentralPanel, Color32, Context, Event, Key, Modifiers, RawInput, Rect, TextureId};

    fn raw_input(events: Vec<Event>) -> RawInput {
        frame(800.0, 600.0, events)
    }

    fn key_event(key: Key) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    /// A view with no rows and nothing committed: enough for the keyword-box key tests.
    fn empty_view(request_focus: bool) -> SearchPanelView {
        SearchPanelView {
            confirm_title: "confirm".to_string(),
            keyword: String::from("birch"),
            title: String::from("search"),
            placeholder: String::from("keyword"),
            quick_search: false,
            enter_hint: String::from("press Enter to search"),
            empty_hint: String::from("no matching messages"),
            jump_hint: String::from("click to jump to this message"),
            request_focus,
            rows: Vec::new(),
            outcome: SearchPanelOutcome::Nothing,
        }
    }

    /// Run the panel contents for one frame with the given events.
    fn run_one_frame(
        context: &Context,
        skin: &Skin,
        view: &mut SearchPanelView,
        events: Vec<Event>,
    ) {
        context
            .run_ui(raw_input(events), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    let icons = test_icons(ui.ctx());
                    draw_panel_body(ui, skin, &icons, view);
                });
            })
            .drop_without_applying_deltas();
    }

    /// The images painted this frame: (where, tint, which texture). egui paints an
    /// unrotated image as a rectangle that carries the texture as its fill brush.
    fn painted_images(context: &Context) -> Vec<(Rect, Color32, TextureId)> {
        context.graphics_mut(|graphics| {
            let mut images: Vec<(Rect, Color32, TextureId)> = Vec::new();
            if let Some(list) = graphics.get(egui::LayerId::background()) {
                for entry in list.all_entries() {
                    if let egui::Shape::Rect(shape) = &entry.shape
                        && shape.brush.is_some()
                    {
                        images.push((shape.rect, shape.fill, shape.fill_texture_id()));
                    }
                }
            }
            images
        })
    }

    /// Both icon-only buttons of the title row: equal, square, and each one really
    /// draws its image tinted with the theme colour.
    #[test]
    fn icon_buttons_equal() {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        skin.apply_to(&context);
        let mut pair = (Rect::NOTHING, Rect::NOTHING);
        let mut icon_textures: Vec<TextureId> = Vec::new();
        let mut painted: Vec<(Rect, Color32, TextureId)> = Vec::new();
        context
            .run_ui(raw_input(Vec::new()), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    let icons = test_icons(ui.ctx());
                    icon_textures = icons.iter().map(|(_, handle)| handle.id()).collect();
                    ui.horizontal(|ui| {
                        let settings = icon_button(ui, &skin, &icons, IconName::Settings, None);
                        let magnifier = draw_search_button(ui, &skin, &icons, String::new());
                        pair = (settings.rect, magnifier.rect);
                    });
                });
                painted = painted_images(ctx);
            })
            .drop_without_applying_deltas();
        let (settings_rect, magnifier_rect) = pair;

        for (name, rect) in [("settings", settings_rect), ("magnifier", magnifier_rect)] {
            assert!(
                rect.width() > 0.0 && rect.height() > 0.0,
                "the {name} button box must exist: {rect:?}"
            );
            assert!(
                (rect.width() - rect.height()).abs() < 0.5,
                "the {name} button box stays square, got {rect:?}"
            );
            assert!(
                painted
                    .iter()
                    .any(|(image, tint, texture)| rect.contains(image.center())
                        && *tint == skin.icon_color
                        && icon_textures.contains(texture)),
                "the {name} button must paint a theme-tinted icon image inside {rect:?}, got {painted:?}"
            );
        }
        assert!(
            (settings_rect.height() - magnifier_rect.height()).abs() < 0.5,
            "both icon buttons must share one height, got {} vs {}",
            settings_rect.height(),
            magnifier_rect.height()
        );
    }

    /// The panel opens with a positive starting size (the window itself stays resizable).
    #[test]
    fn panel_starting_size() {
        assert!(search_panel_width() > 0.0);
        assert!(search_panel_height() > 0.0);
    }

    /// Enter commits the typed keyword only while the caret is in the box, and the
    /// commit frame must not hand focus back or the phone keyboard pops up again.
    #[test]
    fn enter_commit_rule() {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        skin.apply_to(&context);

        let mut view = empty_view(false);
        run_one_frame(&context, &skin, &mut view, vec![key_event(Key::Enter)]);
        assert!(
            matches!(view.outcome, SearchPanelOutcome::Nothing),
            "with the caret outside the keyword box, Enter must not commit a search"
        );

        let mut view = empty_view(true);
        // Frame one hands the caret to the box, frame two presses Enter in it.
        run_one_frame(&context, &skin, &mut view, Vec::new());
        view.request_focus = false;
        run_one_frame(&context, &skin, &mut view, vec![key_event(Key::Enter)]);
        assert!(
            matches!(view.outcome, SearchPanelOutcome::Submitted(ref keyword) if keyword == "birch"),
            "with quick search off, Enter must commit the current keyword (settled: {:?})",
            view.outcome
        );
        let keyword_id = egui::Id::new(crate::app::search_keyword_id());
        assert!(
            !context.memory(|memory| memory.has_focus(keyword_id)),
            "the commit frame must not hand focus back to the keyword box"
        );
    }

    /// The confirm button must share the keyword box's row (the reported stacking)
    /// and carry the translated title, never the raw `button_confirm` key name.
    #[test]
    fn confirm_button() {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        skin.apply_to(&context);
        let mut view = empty_view(false);
        let mut keyword_rect = Rect::NOTHING;
        let mut button_rect = Rect::NOTHING;
        let icons = test_icons(&context);
        context
            .run_ui(raw_input(Vec::new()), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    draw_panel_body(ui, &skin, &icons, &mut view);
                });
                let (found_keyword, found_button) = ctx.graphics_mut(|graphics| {
                    let mut keyword = Rect::NOTHING;
                    let mut button = Rect::NOTHING;
                    if let Some(list) = graphics.get(egui::LayerId::background()) {
                        for entry in list.all_entries() {
                            if let egui::Shape::Text(text_shape) = &entry.shape {
                                let rect =
                                    Rect::from_min_size(text_shape.pos, text_shape.galley.size());
                                if text_shape.galley.text().starts_with("birch") {
                                    keyword = rect;
                                } else if text_shape.galley.text() == "confirm" {
                                    button = rect;
                                }
                            }
                        }
                    }
                    (keyword, button)
                });
                keyword_rect = found_keyword;
                button_rect = found_button;
            })
            .drop_without_applying_deltas();
        assert!(
            keyword_rect.is_positive() && button_rect.is_positive(),
            "the keyword text {keyword_rect:?} and the button title {button_rect:?} must both be painted",
        );
        assert!(
            keyword_rect.min.y < button_rect.max.y && button_rect.min.y < keyword_rect.max.y,
            "the keyword box and the confirm button must sit on one row: {keyword_rect:?} versus {button_rect:?}",
        );
        assert!(
            button_rect.min.x > keyword_rect.min.x + 50.0,
            "the confirm button must sit at the row's right end, not under the box",
        );

        let mut app = crate::app::auth::auth_draft_tests::test_app();
        app.client.language = baihua_core::config::Language::load("zh-CN")
            .expect("the repository's language files must load");
        let view = app.search_panel_view(&context);
        assert_eq!(
            view.confirm_title,
            app.client.text("button_confirm"),
            "the panel must bind the confirm title to the `button_confirm` key",
        );
        assert_ne!(
            view.confirm_title, "confirm",
            "a missing key must not leak into the panel as its own name",
        );
    }

    /// An empty keyword must not make the confirm button vanish: it stays drawn on
    /// the row, faded away from the theme icon colour, and commits nothing.
    #[test]
    fn confirm_stays_empty() {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        skin.apply_to(&context);
        let mut view = empty_view(false);
        view.keyword.clear();
        let icons = test_icons(&context);
        let icon_textures: Vec<TextureId> = icons.iter().map(|(_, handle)| handle.id()).collect();
        let mut painted: Vec<(Rect, Color32, TextureId)> = Vec::new();
        let mut title_rect = Rect::NOTHING;
        context
            .run_ui(raw_input(Vec::new()), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    draw_panel_body(ui, &skin, &icons, &mut view);
                });
                let (images, title) = ctx.graphics_mut(|graphics| {
                    let mut images: Vec<(Rect, Color32, TextureId)> = Vec::new();
                    let mut title = Rect::NOTHING;
                    if let Some(list) = graphics.get(egui::LayerId::background()) {
                        for entry in list.all_entries() {
                            match &entry.shape {
                                egui::Shape::Rect(shape) if shape.brush.is_some() => {
                                    images.push((shape.rect, shape.fill, shape.fill_texture_id()));
                                }
                                egui::Shape::Text(text_shape)
                                    if text_shape.galley.text() == "confirm" =>
                                {
                                    title = Rect::from_min_size(
                                        text_shape.pos,
                                        text_shape.galley.size(),
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                    (images, title)
                });
                painted = images;
                title_rect = title;
            })
            .drop_without_applying_deltas();
        assert!(
            title_rect.is_positive(),
            "the confirm button must stay painted while the keyword box is empty"
        );
        assert!(
            painted.iter().any(|(rect, tint, texture)| {
                icon_textures.contains(texture)
                    && *tint != skin.icon_color
                    && rect.min.y < title_rect.max.y
                    && title_rect.min.y < rect.max.y
            }),
            "the inert confirm icon must be painted faded, got {painted:?}"
        );
        assert!(
            matches!(view.outcome, SearchPanelOutcome::Nothing),
            "an empty keyword box must commit nothing"
        );
    }
}
