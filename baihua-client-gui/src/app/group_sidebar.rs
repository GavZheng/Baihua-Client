//! The group settings sidebar: room information, member roster, add and remove
//! members, as a collapsible right panel (wide layout) or a full layer (narrow).

use super::*;

pub(crate) struct GroupSettingsView {
    /// Whether the sidebar should be slid out this frame
    pub(crate) open: bool,
    /// Panel title, drawn at the top of the sidebar
    pub(crate) group_title: String,
    /// Title of the "add member" entry
    pub(crate) add_member_title: String,
    /// Title of the "remove member" entry
    pub(crate) remove_member_title: String,
    /// Title of the do-not-disturb switch
    pub(crate) mute_title: String,
    /// Title of the group information block
    pub(crate) information_title: String,
    /// Title above the member list
    pub(crate) members_title: String,
    /// Shown in place of the member list when there are no rows to draw
    pub(crate) members_empty_hint: String,
    /// Explains why the removal buttons are disabled for a non-admin
    pub(crate) readonly_hint: String,
    /// Placeholder for the "add member" input box
    pub(crate) add_member_hint: String,
    /// Text of the confirm button
    pub(crate) confirm_title: String,
    /// Text of the "leave group chat" button
    pub(crate) leave_title: String,
    /// Whether the current room has do-not-disturb enabled (drives the switch label)
    pub(crate) muted: bool,
    /// Member rows of the current room: (user ID, display text)
    pub(crate) member_rows: Vec<(String, String)>,
    /// Draft in the "add member" input box
    pub(crate) member_name: String,
    /// Whether the signed-in user may remove members (owner/admin)
    pub(crate) allow_removal: bool,
    /// Summary lines of the current room, formatted exactly like the header of /info
    pub(crate) summary_lines: Vec<String>,
    /// What the sidebar asks the session layer to do
    pub(crate) outcome: GroupSettingsOutcome,
}

/// The actions the group settings sidebar hands back to the session layer.
pub(crate) enum GroupSettingsOutcome {
    Nothing,
    AddMember(String),
    RemoveMember(String),
    ToggleMute,
    LeaveGroup,
}

/// The group settings sidebar: a right panel that shifts the message area on the
/// wide layout, a whole layer on the narrow one, reporting the rectangle on screen.
pub(crate) fn draw_sidebar(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    view: &mut GroupSettingsView,
) -> Rect {
    let panel = egui::Panel::right(sidebar_panel_id())
        .resizable(false)
        .default_size(sidebar_panel_width())
        .frame(sidebar_frame(skin));
    // A local copy on purpose: the app's own state is never written back from the panel, so the
    // sidebar's open state has exactly one owner (`resolve_sidebar_open`).
    let mut open = view.open;
    // Capture the area the panel may occupy before it reserves its space: while it
    // slides the reported rectangle is already shifted, so only this part is real.
    let parent_area = ui.max_rect();
    let response = panel.show_collapsible(ui, &mut open, |ui| {
        draw_sidebar_body(ui, skin, icons, view);
    });
    let _ = open;
    response.map_or(Rect::NOTHING, |response| {
        response.response.rect.intersect(parent_area)
    })
}

/// The sidebar's contents. Split out from the panel plumbing (identifier,
/// (identifier, size range, slide animation) stays readable on its own.
pub(crate) fn draw_sidebar_body(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    view: &mut GroupSettingsView,
) {
    ui.colored_label(skin.message_border, view.group_title.clone());
    ui.separator();
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            draw_sidebar_members(ui, skin, icons, view);
            ui.separator();
            draw_sidebar_add(ui, skin, icons, view);
            ui.separator();
            draw_sidebar_detail(ui, skin, view);
            ui.separator();
            if icon_button(
                ui,
                skin,
                icons,
                IconName::Exit,
                Some(view.leave_title.clone()),
            )
            .clicked()
            {
                view.outcome = GroupSettingsOutcome::LeaveGroup;
            }
        });
}

/// The member roster: each row carries its own removal button, acting on the member's
/// user id (display text is not unique).
pub(crate) fn draw_sidebar_members(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    view: &mut GroupSettingsView,
) {
    ui.label(view.members_title.clone());
    if view.member_rows.is_empty() {
        ui.colored_label(skin.hint_text, view.members_empty_hint.clone());
    }
    let mut removal_requested: Option<String> = None;
    for (user_id, label) in &view.member_rows {
        ui.horizontal(|ui| {
            ui.label(label.clone());
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                // The button stays visible but inert for a non-admin: hiding it outright would
                // leave the person wondering whether the feature exists at all (see the hint below).
                let removal = icon_button_sized(
                    ui,
                    skin,
                    icons,
                    IconName::RemoveMember,
                    Some(view.remove_member_title.clone()),
                    button_minimum_size(ui),
                    view.allow_removal,
                );
                if removal.clicked() {
                    removal_requested = Some(user_id.clone());
                }
            });
        });
    }
    if let Some(user_id) = removal_requested {
        view.outcome = GroupSettingsOutcome::RemoveMember(user_id);
    }
    if !view.allow_removal {
        ui.colored_label(skin.hint_text, view.readonly_hint.clone());
    }
}

/// The "add member" row: a name box with its confirm button at the row's right end.
pub(crate) fn draw_sidebar_add(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    view: &mut GroupSettingsView,
) {
    ui.label(view.add_member_title.clone());
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            // The box and the button share one row: the button is pinned at the row's
            // right end, visible but inert while the name box is still empty.
            let can_add = !view.member_name.trim().is_empty();
            let confirmed = icon_button_enabled(
                ui,
                skin,
                icons,
                IconName::AddMember,
                Some(view.confirm_title.clone()),
                can_add,
            )
            .clicked();
            ui.add(
                TextEdit::singleline(&mut view.member_name)
                    .hint_text(view.add_member_hint.clone())
                    .desired_width(ui.available_width()),
            );
            if confirmed {
                let username = view.member_name.trim().to_string();
                if !username.is_empty() {
                    view.outcome = GroupSettingsOutcome::AddMember(username);
                }
            }
        });
    });
}

/// Do-not-disturb switch (the same toggle `/mute` uses with no argument) plus the
/// information block (`summary_lines`, in step with `/info` by construction).
pub(crate) fn draw_sidebar_detail(ui: &mut Ui, skin: &Skin, view: &mut GroupSettingsView) {
    let mut muted = view.muted;
    // The state itself carries the label: a checkbox with its own text needs no second label and no
    // hand-picked color, and its box takes the theme's background and border from `Skin::apply_to`.
    if ui.checkbox(&mut muted, view.mute_title.clone()).changed() {
        view.outcome = GroupSettingsOutcome::ToggleMute;
    }
    ui.label(view.information_title.clone());
    if view.summary_lines.is_empty() {
        ui.colored_label(skin.hint_text, view.members_empty_hint.clone());
    }
    for line in &view.summary_lines {
        ui.label(line.clone());
    }
}

impl BaihuaApp {
    /// One per-frame snapshot of everything the sidebar draws (the closure must not
    /// touch `self`); outcomes and the add-member draft are written back afterwards.
    pub(crate) fn group_settings_view(&self) -> GroupSettingsView {
        let room_id = self.client.current_room_id();
        // Online/member counts come from the cached roster; the summary lines below reuse the /info
        // formatting so the sidebar and the command never disagree about the same room.
        let summary_lines = match self.client.current_room_detail.as_ref() {
            Some(detail) => {
                let roster: Vec<baihua_core::api::RoomMember> = detail.members.clone();
                self.client.summary_lines(detail, &roster)
            }
            None => Vec::new(),
        };
        GroupSettingsView {
            open: self.group_settings_open,
            // The sidebar title reuses the "..." button's key, since both describe
            // the same panel and the old dedicated key never existed.
            group_title: self.text("group_settings_button"),
            add_member_title: self.text("group_settings_add_member"),
            remove_member_title: self.text("group_settings_remove_member"),
            mute_title: self.text("group_settings_mute"),
            information_title: self.text("group_settings_information"),
            members_title: self.text("group_settings_members"),
            members_empty_hint: self.text("group_settings_members_empty"),
            readonly_hint: self.text("group_settings_readonly_hint"),
            add_member_hint: self.text("add_member_hint"),
            confirm_title: self.text("button_confirm"),
            leave_title: self.text("command_quit_group"),
            muted: room_id
                .as_deref()
                .is_some_and(|room_id| self.client.muted_room_ids.contains(room_id)),
            member_rows: self.client.member_rows(),
            member_name: self.group_settings_member_name.clone(),
            allow_removal: self.client.allows_removal(),
            summary_lines,
            outcome: GroupSettingsOutcome::Nothing,
        }
    }

    /// The wide-layout sidebar on the window's own layer: drawn next to the room
    /// list before the central area reserves its space, so both share one bottom.
    pub(crate) fn draw_sidebar_top(&mut self, ui: &mut Ui, view: &mut GroupSettingsView) -> Rect {
        let skin = self.skin.clone();
        let icons = self.frame_icons(ui.ctx());
        draw_sidebar(ui, &skin, &icons, view)
    }

    /// Apply what the sidebar asked for. Called after drawing, so every server call happens on a
    /// user-initiated moment, never inside the render closure.
    pub(crate) fn apply_sidebar(&mut self, outcome: GroupSettingsOutcome) {
        match outcome {
            GroupSettingsOutcome::Nothing => {}
            GroupSettingsOutcome::AddMember(username) => {
                self.client.add_member(&username);
                // The member table changed and the sidebar is showing it: clear the box so the next
                // name can be typed straight away.
                self.group_settings_member_name.clear();
            }
            GroupSettingsOutcome::RemoveMember(user_id) => {
                self.client.remove_member_by_id(&user_id);
                self.group_settings_member_name.clear();
            }
            GroupSettingsOutcome::ToggleMute => self.client.apply_mute_command(""),
            GroupSettingsOutcome::LeaveGroup => {
                self.client.leave_current_room();
                // The room is gone; there is nothing left for the sidebar to describe.
                self.group_settings_open = false;
            }
        }
    }

    // ==================== Standalone Message Search Panel ====================

    /// The narrow layout's sidebar layer: the same contents as the wide sidebar, given
    /// the whole window (minus the status bar), closed by an "X" back button.
    pub(crate) fn draw_sidebar_layer(&mut self, ui: &mut Ui) {
        let skin = self.skin.clone();
        let icons = self.frame_icons(ui.ctx());
        let back_hint = self.text("narrow_back_hint");
        let mut view = self.group_settings_view();
        let mut back_requested = false;
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(skin.app_background)
                    .corner_radius(panel_corner())
                    .outer_margin(Margin::same(panel_inset()))
                    .inner_margin(Margin::same(10)),
            )
            .show(ui, |ui| {
                if icon_button(ui, &skin, &icons, IconName::Back, None)
                    .on_hover_text(back_hint.clone())
                    .clicked()
                {
                    back_requested = true;
                }
                ui.separator();
                draw_sidebar_body(ui, &skin, &icons, &mut view);
            });
        if back_requested {
            self.group_settings_open = false;
        }
        self.group_settings_member_name = view.member_name.clone();
        self.apply_sidebar(view.outcome);
    }

    // ==================== Settings Panel ====================
}

#[cfg(test)]
mod group_settings_tests {
    use super::{
        GroupSettingsOutcome, GroupSettingsView, TitleRowView, draw_sidebar_body, draw_title_row,
        resolve_sidebar_open, sidebar_blank_click, sidebar_button_text, sidebar_panel_id,
        sidebar_panel_width,
    };
    use crate::app::icons::test_icons;
    use crate::app::test_support::{click_at as click, frame};
    use crate::appearance::Skin;
    use baihua_core::api::{RoomDetail, RoomMember};
    use baihua_core::config::Palette;
    use egui::{
        CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2,
    };

    /// A view with a couple of members, as the sidebar would get it from the session layer
    fn view() -> GroupSettingsView {
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
            member_rows: vec![
                ("user-self".to_string(), "alice (owner)".to_string()),
                ("user-other".to_string(), "bob (member, online)".to_string()),
            ],
            member_name: String::new(),
            allow_removal: true,
            summary_lines: vec!["group name: team one".to_string()],
            outcome: GroupSettingsOutcome::Nothing,
        }
    }

    fn room_member(user_id: &str, username: &str, role: &str) -> RoomMember {
        RoomMember {
            user_id: user_id.to_string(),
            username: username.to_string(),
            nickname: None,
            role: role.to_string(),
            joined_at: "2026-09-06T00:00:00+00:00".to_string(),
        }
    }

    fn room_detail(members: Vec<RoomMember>) -> RoomDetail {
        let member_count = members.len();
        RoomDetail {
            id: "room-one".to_string(),
            name: Some("team one".to_string()),
            created_by: "user-self".to_string(),
            created_at: "2026-09-06T00:00:00+00:00".to_string(),
            is_group: true,
            is_encrypted: false,
            member_count,
            members,
        }
    }

    fn click_at(point: Pos2) -> RawInput {
        frame(900.0, 600.0, click(point))
    }

    /// The switch drawn in the title row is a text button reading "...", not an image or an emoji,
    /// and the panel identifier has to stay distinct from every other panel's.
    #[test]
    fn entry_point_button() {
        assert_eq!(sidebar_button_text(), "...");
        assert_eq!(sidebar_panel_id(), "group-settings");
        assert_ne!(
            sidebar_panel_id(),
            "room-list",
            "the sidebar's panel identifier must not collide with the room list's or they would overwrite each other's remembered width"
        );
    }

    /// The fixed sidebar width is wide enough for a member row plus its removal
    /// button and narrow enough to leave the message area usable.
    #[test]
    fn sidebar_width_fixed() {
        let width = sidebar_panel_width();
        assert!(
            width > 0.0,
            "the sidebar must have a real width, got {width}"
        );
        // The narrowest wide-layout window is the narrow threshold; beside the
        // sidebar sits the room list at its minimum, and the rest is the message area.
        let window = super::narrow_threshold();
        let message_area = window - width - *super::room_panel_range().start();
        assert!(
            message_area > window / 3.0,
            "the fixed sidebar width {width} leaves only {message_area} points of a {window}-point window to the message area"
        );
    }

    /// Room list and sidebar sit on the window's own layer side by side, so their
    /// painted borders must end on one bottom line; `bottom_edges_align` guards it.
    #[test]
    fn bottom_edges_align() {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let mut view = view();
        let mut panel_borders: Vec<(f32, f32, f32)> = Vec::new();
        context
            .run_ui(frame(900.0, 600.0, Vec::new()), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    let icons = test_icons(ui.ctx());
                    let _ = super::room_panel(&skin).show(ui, |_ui| {});
                    let _ = super::draw_sidebar(ui, &skin, &icons, &mut view);
                });
                panel_borders = ctx.graphics_mut(|graphics| {
                    let mut found: Vec<(f32, f32, f32)> = Vec::new();
                    if let Some(list) = graphics.get(egui::LayerId::background()) {
                        for entry in list.all_entries() {
                            if let egui::Shape::Rect(rect) = &entry.shape
                                && rect.stroke.width == 1.0
                                && rect.stroke.color == skin.room_border
                            {
                                found.push((
                                    rect.rect.width(),
                                    rect.rect.height(),
                                    rect.rect.bottom(),
                                ));
                            }
                        }
                    }
                    found
                });
            })
            .drop_without_applying_deltas();
        let tall: Vec<f32> = panel_borders
            .iter()
            .filter(|(width, height, _bottom)| *width > 40.0 && *height > 200.0)
            .map(|(_, _, bottom)| *bottom)
            .collect();
        assert!(
            tall.len() >= 2,
            "both panel borders must be painted, found {panel_borders:?}"
        );
        let first = tall[0];
        for bottom in &tall {
            assert!(
                (bottom - first).abs() < 1.5,
                "panel bottom edges disagree: {tall:?} (points apart)"
            );
        }
    }

    /// The name box and its confirm button share one row, and an empty name box
    /// hands back no outcome at all (the retired usage notice is what this guards).
    #[test]
    fn add_row_one_line() {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let mut view = view();
        view.member_name.clear();
        let mut hint_rect = Rect::NOTHING;
        let mut title_rect = Rect::NOTHING;
        context
            .run_ui(frame(900.0, 600.0, Vec::new()), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    let icons = test_icons(ui.ctx());
                    draw_sidebar_body(ui, &skin, &icons, &mut view);
                });
                let (hint, title) = ctx.graphics_mut(|graphics| {
                    let mut hint = Rect::NOTHING;
                    let mut title = Rect::NOTHING;
                    if let Some(list) = graphics.get(egui::LayerId::background()) {
                        for entry in list.all_entries() {
                            if let egui::Shape::Text(text_shape) = &entry.shape {
                                let rect =
                                    Rect::from_min_size(text_shape.pos, text_shape.galley.size());
                                if text_shape.galley.text().contains("username to add") {
                                    hint = rect;
                                } else if text_shape.galley.text() == "confirm" {
                                    title = rect;
                                }
                            }
                        }
                    }
                    (hint, title)
                });
                hint_rect = hint;
                title_rect = title;
            })
            .drop_without_applying_deltas();
        assert!(
            hint_rect.is_positive() && title_rect.is_positive(),
            "the name box hint {hint_rect:?} and the confirm title {title_rect:?} must both be painted"
        );
        assert!(
            hint_rect.min.y < title_rect.max.y && title_rect.min.y < hint_rect.max.y,
            "the name box and the confirm button must sit on one row: {hint_rect:?} versus {title_rect:?}"
        );
        assert!(
            matches!(view.outcome, GroupSettingsOutcome::Nothing),
            "an empty name box must hand back no outcome"
        );
    }

    /// A roster that is missing or belongs to another room closes an already-open
    /// sidebar; it never blocks opening.
    #[test]
    fn stale_member_table() {
        let detail = room_detail(vec![room_member("user-other", "bob", "member")]);
        assert!(
            super::detail_matches_room(Some(&detail), Some("room-one")),
            "the member table of the same room must be usable"
        );
        assert!(
            !super::detail_matches_room(Some(&detail), Some("room-two")),
            "a member table from another room must not serve the current room's sidebar"
        );
        assert!(
            !super::detail_matches_room(Some(&detail), None),
            "with no room selected it must be unusable too"
        );
        assert!(
            !super::detail_matches_room(None, Some("room-one")),
            "without a fetched member table (a failed fetch included) it must be unusable"
        );
    }

    /// The open state has five independent inputs whose priority is asserted here:
    /// getting it wrong slides the panel back out or leaves it on a private chat.
    #[test]
    fn open_state_priority() {
        // 1. A private chat (or no room at all) always keeps it in, no matter what the other inputs say
        for (roster_ready, was_open, toggle, blank, panel_open) in [
            (true, true, true, false, true),
            (true, true, false, false, true),
            (true, false, true, false, true),
            (true, true, false, true, true),
        ] {
            assert!(
                !resolve_sidebar_open(false, roster_ready, was_open, toggle, blank, panel_open),
                "in a private chat (or with no room) the sidebar must stay closed: roster_ready={roster_ready} was_open={was_open} toggle={toggle} blank={blank} panel_open={panel_open}"
            );
        }

        // 2. The "..." button flips the state it came in with (not the panel's own report),
        //    whether or not the member table is already in hand
        assert!(resolve_sidebar_open(true, true, false, true, false, false));
        assert!(resolve_sidebar_open(true, false, false, true, false, false));
        assert!(!resolve_sidebar_open(true, true, true, true, false, true));

        // 3. A stale or missing member table only puts away a sidebar that was already out
        assert!(!resolve_sidebar_open(true, false, true, false, false, true));

        // 4. A blank click puts it away
        assert!(!resolve_sidebar_open(true, true, true, false, true, true));

        // 5. Otherwise the app's own state passes through unchanged (the panel is not resizable,
        // so it has no report of its own any more)
        assert!(resolve_sidebar_open(true, true, true, false, false, true));
        assert!(
            !resolve_sidebar_open(true, true, true, false, false, false),
            "discarding the panel's own reported collapsed state would make it slide back out right after a drag-close"
        );
    }

    /// A click outside both the sidebar and its switch counts as blank space, a
    /// click on either of them does not, and a swipe across blank space is a drag.
    #[test]
    fn blank_click_rule() {
        let context = Context::default();
        let sidebar = Rect::from_min_max(Pos2::new(600.0, 100.0), Pos2::new(880.0, 500.0));
        let toggle_button = Rect::from_min_max(Pos2::new(700.0, 60.0), Pos2::new(730.0, 84.0));
        let verdict = |input: RawInput| {
            let mut blank = false;
            context
                .run_ui(input, |ctx| {
                    blank = sidebar_blank_click(ctx, sidebar, toggle_button);
                })
                .drop_without_applying_deltas();
            blank
        };
        assert!(
            verdict(click_at(Pos2::new(320.0, 300.0))),
            "a click outside the sidebar and its switch must count as blank space"
        );
        assert!(
            !verdict(click_at(Pos2::new(700.0, 300.0))),
            "a click inside the sidebar must not close it"
        );
        assert!(
            !verdict(click_at(Pos2::new(715.0, 72.0))),
            "the switch's own click belongs to the button, not to blank space"
        );
        // Press, travel far, release: a drag, which must not read as a click.
        let release = |point: Pos2| Event::PointerButton {
            pos: point,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        };
        let swipe = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 600.0))),
            focused: true,
            events: vec![
                Event::PointerButton {
                    pos: Pos2::new(120.0, 300.0),
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                Event::PointerMoved(Pos2::new(320.0, 300.0)),
                Event::PointerMoved(Pos2::new(520.0, 300.0)),
                release(Pos2::new(560.0, 300.0)),
            ],
            ..Default::default()
        };
        assert!(
            !verdict(swipe),
            "a swipe across the message area must not put the sidebar away"
        );
    }

    /// Every member row carries the user ID removal acts on: the display text holds the nickname and
    /// presence, so it must never be what the removal request is keyed by.
    #[test]
    fn removal_by_user_id() {
        let mut client = crate::client::Client::default();
        client.current_user_id = Some("user-self".to_string());
        client.rooms = vec![baihua_core::api::RoomInfo {
            id: "room-one".to_string(),
            name: Some("team one".to_string()),
            created_by: "user-self".to_string(),
            created_at: String::new(),
            is_group: true,
            is_encrypted: false,
            members: vec!["user-self".to_string(), "user-other".to_string()],
        }];
        client.selected_room_index = Some(0);
        client.current_room_detail = Some(room_detail(vec![
            room_member("user-self", "alice", "owner"),
            room_member("user-other", "bob", "member"),
        ]));
        client
            .presence_by_user
            .insert("user-other".to_string(), true);

        let rows = client.member_rows();
        assert_eq!(rows.len(), 2, "every member deserves a row");
        assert_eq!(rows[0].0, "user-self");
        assert!(
            rows[0].1.starts_with("alice") && rows[0].1.contains("owner"),
            "row text must carry username and role, got {:?}",
            rows[0].1
        );
        assert!(
            rows[1].1.contains("bob") && rows[1].1.contains(&client.text("status_online")),
            "an online member row must say so, got {:?}",
            rows[1].1
        );
        assert!(
            client.allows_removal(),
            "the owner and admins must be allowed to remove members"
        );

        // A plain member must not be allowed to remove anybody
        client.current_room_detail = Some(room_detail(vec![
            room_member("user-self", "alice", "member"),
            room_member("user-other", "bob", "owner"),
        ]));
        assert!(
            !client.allows_removal(),
            "an ordinary member must not gain removal rights"
        );
    }

    /// Switching rooms drops the cached member table, so the sidebar never shows the previous room's members.
    #[test]
    fn room_switch_forgets() {
        let mut client = crate::client::Client::default();
        client.connector.set_base_url("http://127.0.0.1:1");
        client.current_user_id = Some("user-self".to_string());
        client.rooms = vec![
            baihua_core::api::RoomInfo {
                id: "room-one".to_string(),
                name: Some("team one".to_string()),
                created_by: "user-self".to_string(),
                created_at: String::new(),
                is_group: true,
                is_encrypted: false,
                members: vec!["user-self".to_string()],
            },
            baihua_core::api::RoomInfo {
                id: "room-two".to_string(),
                name: Some("team two".to_string()),
                created_by: "user-self".to_string(),
                created_at: String::new(),
                is_group: true,
                is_encrypted: false,
                members: vec!["user-self".to_string()],
            },
        ];
        client.selected_room_index = Some(0);
        client.current_room_detail = Some(room_detail(vec![room_member(
            "user-other",
            "bob",
            "member",
        )]));
        client.open_room(1);
        assert!(
            client.current_room_detail.is_none(),
            "switching rooms must drop the previous member table or the sidebar shows the wrong people"
        );
    }

    /// Draw the sidebar for a few frames and return the last reported rectangle;
    /// one frame would only measure the animation, not where the panel settles.
    fn settled_rect(context: &Context, skin: &Skin, open: bool) -> Rect {
        let mut state = view();
        state.open = open;
        let mut rect = Rect::NOTHING;
        for _ in 0..40 {
            context
                .run_ui(
                    RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 600.0))),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let icons = test_icons(ui.ctx());
                            rect = super::draw_sidebar(ui, skin, &icons, &mut state);
                        });
                    },
                )
                .drop_without_applying_deltas();
        }
        rect
    }

    /// The panel must report its area while out and none once settled closed: a
    /// phantom rectangle breaks the blank-click rule, a missing one inverts it.
    #[test]
    fn panel_rect_while_out() {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let open_rect = settled_rect(&context, &skin, true);
        assert!(
            open_rect.width() > 0.0 && open_rect.height() > 0.0,
            "an open sidebar must report its footprint (blank clicks judge by it), got {open_rect:?}"
        );
        assert!(
            open_rect.width() >= sidebar_panel_width() - 1.0,
            "an open sidebar must be as wide as its fixed width, got {}",
            open_rect.width()
        );
        let closed_rect = settled_rect(&context, &skin, false);
        assert!(
            closed_rect.width() <= 0.0 && closed_rect.height() <= 0.0,
            "a fully closed sidebar must report no footprint, got {closed_rect:?}"
        );
    }

    /// Draw one title row and return (the "..." rect, row height) — the ground truth
    /// for the overlap regression: the row must stay one button tall.
    fn title_row_with_name(group_room: bool, room_name: &str) -> (Rect, f32) {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let mut button_rect = Rect::NOTHING;
        let mut row_growth = 0.0;
        context
            .run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 600.0))),
                    ..Default::default()
                },
                |ctx| {
                    CentralPanel::default().show(ctx, |ui| {
                        let before = ui.cursor().min.y;
                        let title_row_view = TitleRowView {
                            narrow: false,
                            title: room_name,
                            group_room,
                            back_hint: "back",
                            group_settings_hint: "settings",
                            search_hint: "search",
                        };
                        let icons = test_icons(ui.ctx());
                        let (_, _, _, rect) = draw_title_row(ui, &skin, &icons, &title_row_view);
                        button_rect = rect;
                        row_growth = ui.cursor().min.y - before;
                    });
                },
            )
            .drop_without_applying_deltas();
        (button_rect, row_growth)
    }

    /// A room name with no break opportunity must still leave the title row one
    /// button tall, and a private chat draws no button at all.
    #[test]
    fn title_row_buttons() {
        let long_name = "baihua-development-team-room-with-a-name-long-enough-to-wrap-forever";
        let (button_rect, row_growth) = title_row_with_name(true, long_name);
        assert!(
            button_rect.is_finite() && button_rect.width() > 0.0,
            "a group room must show its \"...\" button, got {button_rect:?}"
        );
        assert!(
            row_growth <= button_rect.height() * 2.0,
            "the title row must stay one button tall, grew {row_growth} against a {}-point button",
            button_rect.height()
        );

        let (button_rect, row_growth) = title_row_with_name(false, "alice");
        assert_eq!(
            button_rect,
            Rect::NOTHING,
            "a private chat must not draw the \"...\" button at all"
        );
        assert!(
            row_growth > 0.0 && row_growth < 100.0,
            "the title row still occupies exactly one row, got {row_growth}"
        );
    }
}
