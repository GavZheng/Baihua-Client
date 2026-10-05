//! The create-group and create-private-chat windows and their form handling.

use super::*;

/// Which form the create window currently has open
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreationPage {
    /// Create group chat: group name + members (comma-separated)
    Group,
    /// Create private chat: the other person's username
    Private,
}

/// Text and data for the two creation windows (same shape: labeled rows plus a
/// create button; they differ only in title, first label, and the extra members row).
pub(crate) struct CreationView {
    /// Which form to draw
    pub(crate) page: CreationPage,
    /// Window title
    pub(crate) title: String,
    /// The label on the first row (group name / other person's username)
    pub(crate) first_label: String,
    /// The content in the first-row input box
    pub(crate) first_value: String,
    /// The label for the members row (only for creating group chat)
    pub(crate) members_label: String,
    /// Placeholder hint for the member input box (only for creating group chat)
    pub(crate) members_placeholder: String,
    /// Content in the member input box (only for creating group chat)
    pub(crate) members_value: String,
    /// Text on the create button
    pub(crate) confirm: String,
}

/// Draw the create group/private chat window. Returns `(is the window still open, did this frame click the create button)`:
/// Close the window via the close button in the title bar's top-right corner; submit via the create button; both are handled separately by the caller.
pub(crate) fn draw_creation_window(
    context: &Context,
    skin: &Skin,
    icons: &IconFrame,
    view: &mut CreationView,
) -> (bool, bool) {
    let mut keep_open = true;
    let mut submitted = false;
    Window::new(view.title.clone())
        .id(Id::new("creation-window"))
        .open(&mut keep_open)
        // Unified with other floating layers: small border radius, collapsible (the triangle in the title bar)
        .resizable(true)
        .collapsible(true)
        .default_pos([160.0, 120.0])
        .frame(overlay_window_frame(skin))
        .show(context, |ui| {
            submitted = draw_creation_form(ui, skin, icons, view);
        });
    (keep_open, submitted)
}

/// Draw the form content in the create window: several rows of "label + input box", plus a create button.
/// Return whether the create button was clicked this frame.
pub(crate) fn draw_creation_form(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    view: &mut CreationView,
) -> bool {
    draw_labeled_input(ui, &view.first_label, &mut view.first_value, "");
    if view.page == CreationPage::Group {
        draw_labeled_input(
            ui,
            &view.members_label,
            &mut view.members_value,
            &view.members_placeholder,
        );
    }
    icon_button(
        ui,
        skin,
        icons,
        IconName::Confirm,
        Some(view.confirm.clone()),
    )
    .clicked()
}

/// Width of a "one label + one input box" row in the creation form (points)
pub(crate) fn creation_input_width() -> f32 {
    200.0
}

/// The "+" menu beside the room list title: opens the create-group/create-private
/// picker and reports this frame's choice (None when nothing was picked).
pub(crate) fn draw_creation_menu(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    create_group_title: &str,
    create_private_title: &str,
) -> Option<CreationPage> {
    // The plus itself stays text: no single image means "create either kind". Its
    // size is set here because the shared style no longer enlarges buttons.
    let button = egui::Button::new(create_button_text()).min_size(button_minimum_size(ui));
    egui::containers::menu::MenuButton::from_button(button)
        .ui(ui, |ui| {
            let mut picked = None;
            if icon_button(
                ui,
                skin,
                icons,
                IconName::CreateGroup,
                Some(create_group_title.to_string()),
            )
            .clicked()
            {
                picked = Some(CreationPage::Group);
            }
            if icon_button(
                ui,
                skin,
                icons,
                IconName::CreatePrivateChat,
                Some(create_private_title.to_string()),
            )
            .clicked()
            {
                picked = Some(CreationPage::Private);
            }
            picked
        })
        .1
        .and_then(|inner_response| inner_response.inner)
}

impl BaihuaApp {
    /// Draw the create group/private chat window. Entry is in the plus button to the right of the room list title.
    pub(crate) fn draw_creation_panel(&mut self, context: &Context) {
        let Some(page) = self.creation_page else {
            return;
        };
        let skin = self.skin.clone();
        let icons = self.frame_icons(context);
        let mut view = self.creation_view(page);
        let (keep_open, submitted) = draw_creation_window(context, &skin, &icons, &mut view);
        self.apply_creation_view(page, view);
        if submitted && !self.submit_creation(page) {
            // Input is invalid: the window stays, the draft stays, fix and click again
            return;
        }
        if submitted || !keep_open {
            self.creation_page = None;
        }
    }

    /// All text and data the create window needs to read and write (the window closure doesn't touch self, so take it out first and write it back)
    pub(crate) fn creation_view(&self, page: CreationPage) -> CreationView {
        match page {
            CreationPage::Group => CreationView {
                page,
                title: self.text("create_group_title"),
                first_label: self.text("group_name_label"),
                first_value: self.group_name.clone(),
                members_label: self.text("create_group_members_label"),
                members_placeholder: members_placeholder(),
                members_value: self.group_members.clone(),
                confirm: self.text("button_confirm"),
            },
            CreationPage::Private => CreationView {
                page,
                title: self.text("create_private_title"),
                first_label: self.text("private_target_label"),
                first_value: self.private_target.clone(),
                members_label: String::new(),
                members_placeholder: String::new(),
                members_value: String::new(),
                confirm: self.text("button_confirm"),
            },
        }
    }

    /// Write the input in the window back to the draft: close the window and reopen and it's still what was last filled in, making it easy to retry after editing
    pub(crate) fn apply_creation_view(&mut self, page: CreationPage, view: CreationView) {
        match page {
            CreationPage::Group => {
                self.group_name = view.first_value;
                self.group_members = view.members_value;
            }
            CreationPage::Private => self.private_target = view.first_value,
        }
    }

    /// Clicked the create button: empty input errors on the spot and keeps the window open; only with content does it actually send.
    /// Return whether it was submitted (if submitted, collapse the window).
    pub(crate) fn submit_creation(&mut self, page: CreationPage) -> bool {
        match page {
            CreationPage::Group => {
                if self.group_name.trim().is_empty() {
                    self.client
                        .notify_error(self.text("error_group_name_empty"));
                    return false;
                }
                let (name, members) = (self.group_name.clone(), self.group_members.clone());
                self.client.create_group(&name, &members);
                true
            }
            CreationPage::Private => {
                if self.private_target.trim().is_empty() {
                    self.client.notify_error(self.text("error_username_empty"));
                    return false;
                }
                let target = self.private_target.clone();
                self.client.create_private_chat(&target);
                true
            }
        }
    }
}

#[cfg(test)]
mod creation_window_tests {
    use super::{CreationPage, CreationView, draw_creation_form};
    use crate::app::icons::test_icons;
    use crate::app::test_support::{click_at, frame, position_of};
    use crate::appearance::Skin;
    use baihua_core::config::Palette;
    use egui::{CentralPanel, Context, Event, Pos2, RawInput, Vec2};

    fn raw_input(events: Vec<Event>) -> RawInput {
        frame(640.0, 480.0, events)
    }

    /// Create group chat form: two rows of input boxes plus a create button
    fn group_view() -> CreationView {
        CreationView {
            page: CreationPage::Group,
            title: "create group chat".to_string(),
            first_label: "group name".to_string(),
            first_value: String::new(),
            members_label: "members (comma separated)".to_string(),
            members_placeholder: "user1,user2".to_string(),
            members_value: String::new(),
            confirm: "confirm".to_string(),
        }
    }

    /// Create private chat form: only one row of input box plus a create button
    fn private_view() -> CreationView {
        CreationView {
            page: CreationPage::Private,
            title: "create private chat".to_string(),
            first_label: "the other user's name".to_string(),
            first_value: String::new(),
            members_label: String::new(),
            members_placeholder: String::new(),
            members_value: String::new(),
            confirm: "confirm".to_string(),
        }
    }

    /// Draw a create form for one frame, return (the text and positions drawn this frame, whether the create button was clicked)
    fn run_one_frame(
        context: &Context,
        view: &mut CreationView,
        events: Vec<Event>,
    ) -> (Vec<(String, Pos2)>, bool) {
        let mut submitted = false;
        let mut painted: Vec<(String, Pos2)> = Vec::new();
        context
            .run_ui(raw_input(events), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    let skin = Skin::from(&Palette::built_in());
                    let icons = test_icons(ui.ctx());
                    submitted = draw_creation_form(ui, &skin, &icons, view);
                });
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
        (painted, submitted)
    }

    fn drawn_texts(texts: &[(String, Pos2)]) -> Vec<String> {
        texts.iter().map(|(text, _)| text.clone()).collect()
    }

    /// The create-group form paints the group name and members rows plus a create
    /// button; the create-private form paints only the target row and no members.
    #[test]
    fn creation_forms() {
        let context = Context::default();
        let mut view = group_view();
        let (texts, _submitted) = run_one_frame(&context, &mut view, Vec::new());
        let drawn = drawn_texts(&texts);
        for expected in ["group name", "members (comma separated)", "confirm"] {
            assert!(
                drawn.iter().any(|text| text == expected),
                "the create-group form must paint {expected:?}, drew {drawn:?}"
            );
        }

        let mut view = private_view();
        let (texts, _submitted) = run_one_frame(&context, &mut view, Vec::new());
        let drawn = drawn_texts(&texts);
        for expected in ["the other user's name", "confirm"] {
            assert!(
                drawn.iter().any(|text| text == expected),
                "the create-private form must paint {expected:?}, drew {drawn:?}"
            );
        }
        assert!(
            !drawn.iter().any(|text| text == "members (comma separated)"),
            "the create-private form must not draw the members row, drew {drawn:?}"
        );
    }

    /// Clicking the create button must report "it was submitted this frame"
    #[test]
    fn create_button_click() {
        let context = Context::default();
        let mut view = group_view();
        let (texts, _submitted) = run_one_frame(&context, &mut view, Vec::new());
        let button = position_of(&texts, "confirm");
        let (_texts, submitted) =
            run_one_frame(&context, &mut view, click_at(button + Vec2::new(4.0, 8.0)));
        assert!(
            submitted,
            "clicking the create button must report submission"
        );
    }

    /// The input box can type: click into the first input row, then type, and the content must go into the draft
    #[test]
    fn typing_first_input() {
        let context = Context::default();
        let mut view = group_view();
        let (texts, _submitted) = run_one_frame(&context, &mut view, Vec::new());
        let label = position_of(&texts, "group name");
        // The input box is right after the label: shift right from the label's left edge, the hit point is still inside the input box
        let input_point = Pos2::new(label.x + 150.0, label.y + 8.0);
        run_one_frame(&context, &mut view, click_at(input_point));
        run_one_frame(
            &context,
            &mut view,
            vec![Event::Text("graphical integration group".to_string())],
        );
        assert_eq!(
            view.first_value, "graphical integration group",
            "typed characters must stay in the group name, got {:?}",
            view.first_value
        );
    }
}
