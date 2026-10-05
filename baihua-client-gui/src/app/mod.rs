//! The application object: session lifecycle, per-frame logic, shortcuts, notices
//! and the routing that picks which panels are drawn this frame.

pub(crate) use crate::appearance::{AvatarTextures, Skin, contrasting_text, is_light_background};
pub(crate) use crate::client::{Client, Notice, NoticeKind, UpdateStage};
pub(crate) use crate::client_actions::UiIntent;
pub(crate) use baihua_core::api::{MessageInfo, PollingEvent, RoomDetail};
pub(crate) use baihua_core::config;
pub(crate) use egui::style::ScrollAnimation;
pub(crate) use egui::{
    Align, Align2, Area, Atom, Atoms, Button, Color32, Context, CornerRadius, Event, Frame, Galley,
    Id, Key, KeyboardShortcut, Margin, Modal, Modifiers, NumExt, Order, Pos2, Rect, Response,
    RichText, ScrollArea, Sense, Shape, Stroke, StrokeKind, TextEdit, TextStyle, TextWrapMode,
    TextureHandle, Ui, Vec2, Window,
};
pub(crate) use std::path::PathBuf;
pub(crate) use std::time::{Duration, Instant};

pub struct BaihuaApp {
    /// Session layer: connections, data, and all server-side actions
    client: Client,
    /// Color set derived from the theme
    skin: Skin,
    /// Avatar texture cache
    avatars: AvatarTextures,
    /// Whether the settings panel is expanded (entry is at the bottom-left of the group display area, left of the input box)
    settings_open: bool,
    /// Whether the group settings sidebar is slid out; the panel animates both ways
    /// on this single flag, so nothing else remembers an animation state.
    group_settings_open: bool,
    /// Draft in the group settings sidebar's "add member" box
    group_settings_member_name: String,
    /// Whether the message search panel is showing (opened by the magnifier button).
    search_panel_open: bool,
    /// Draft in the search panel's keyword box; with quick search off it is
    /// compared against the Enter-committed keyword.
    search_panel_keyword: String,
    /// The centered page displayed when not logged in
    auth_page: Option<AuthPage>,
    /// Profile card
    profile_card_open: bool,
    /// Which command is selected in the completion popup (arrow keys move it, a
    /// prefix change resets it to the first).
    completion_selection: usize,
    /// Whether the create group or private chat window is open, and which one; the
    /// entry is the plus button beside the room list title.
    creation_page: Option<CreationPage>,
    sections: Sections,
    login_name: String,
    login_password: String,
    register_name: String,
    register_email: String,
    register_password: String,
    server_address: String,
    profile_nickname: String,
    profile_phone: String,
    profile_bio: String,
    password_old: String,
    password_new: String,
    password_repeat: String,
    avatar_url: String,
    delete_password: String,
    group_name: String,
    group_members: String,
    private_target: String,
    /// Hand keyboard focus back to the message input box next frame
    focus_message_input: bool,
    /// Whether to focus the search panel keyword box next frame.
    focus_search_panel_input: bool,
    /// Touch platforms only: the message box focus latched from the last frame. While
    /// set, the input docks to the top so the soft keyboard cannot cover the draft.
    message_box_focused_last_frame: bool,
    /// Android only: the activity handle, used to query the real display density
    /// for the screen-scale fallback below.
    #[cfg(target_os = "android")]
    android_application: Option<winit::platform::android::activity::AndroidApp>,
    /// Phones: the notification-permission explanation is waiting for an answer.
    #[cfg(any(target_os = "android", target_os = "ios"))]
    notification_prompt_open: bool,
}

impl BaihuaApp {
    /// Establish a session and start the background threads.
    pub fn new(context: &Context) -> Self {
        let mut client = Client::start();
        client.open_event_channel();
        // Background threads wake the frame loop through this callback the moment an
        // event lands, which lets `run_logic` drop the fast fixed repaint beat.
        {
            let wake_context = context.clone();
            client.set_event_waker(std::sync::Arc::new(move || wake_context.request_repaint()));
        }
        // Startup networking runs on a background thread and lands in `run_logic`:
        // the old synchronous probe made the first frame wait on a dead server.
        client.begin_startup();
        client.watch_reachability();
        let skin = Skin::from(&client.palette);
        skin.apply_to(context);
        // Install the Chinese fallback font once: egui's built-in fonts carry no
        // Han glyphs, so without it every Chinese string would be a box.
        crate::appearance::install_chinese_font(context);
        Self {
            client,
            skin,
            avatars: AvatarTextures::default(),
            settings_open: false,
            group_settings_open: false,
            group_settings_member_name: String::new(),
            search_panel_open: false,
            search_panel_keyword: String::new(),
            auth_page: Some(AuthPage::Login),
            profile_card_open: false,
            completion_selection: 0,
            creation_page: None,
            sections: Sections::default(),
            login_name: String::new(),
            login_password: String::new(),
            register_name: String::new(),
            register_email: String::new(),
            register_password: String::new(),
            server_address: String::new(),
            profile_nickname: String::new(),
            profile_phone: String::new(),
            profile_bio: String::new(),
            password_old: String::new(),
            password_new: String::new(),
            password_repeat: String::new(),
            avatar_url: String::new(),
            delete_password: String::new(),
            group_name: String::new(),
            group_members: String::new(),
            private_target: String::new(),
            // The caret goes to the message box only once auto-login closed the page.
            focus_message_input: false,
            focus_search_panel_input: false,
            message_box_focused_last_frame: false,
            #[cfg(target_os = "android")]
            android_application: None,
            #[cfg(any(target_os = "android", target_os = "ios"))]
            notification_prompt_open: false,
        }
    }

    /// Android only: store the activity handle (called once from `android_main`).
    #[cfg(target_os = "android")]
    pub fn attach_activity(
        &mut self,
        android_application: winit::platform::android::activity::AndroidApp,
    ) {
        self.android_application = Some(android_application);
    }

    /// Android screen-scale fallback: when winit reports the silent default scale,
    /// set egui's pixels per point from the real display density. Idempotent.
    #[cfg(target_os = "android")]
    pub(crate) fn apply_screen_scale(&self, context: &Context) {
        let Some(android_application) = &self.android_application else {
            return;
        };
        let Some(density) = android_application
            .config()
            .density()
            .filter(|density| *density > 0)
        else {
            return;
        };
        let display_scale = density as f32 / 160.0;
        let reported_scale = context.native_pixels_per_point().unwrap_or(0.0);
        let window_system_is_silent =
            reported_scale <= 0.0 || ((reported_scale - 1.0).abs() < f32::EPSILON);
        if window_system_is_silent && (display_scale - reported_scale).abs() > f32::EPSILON {
            context.set_pixels_per_point(display_scale);
            config::debug_log(&format!(
                "Android: applied display density fallback, pixels per point {display_scale}"
            ));
        }
    }

    pub(crate) fn text(&self, key: &str) -> String {
        self.client.text(key)
    }

    /// The theme may have just been switched: recalculate colors and write back to egui visuals
    pub(crate) fn refresh_skin(&mut self, context: &Context) {
        self.skin = Skin::from(&self.client.palette);
        self.skin.apply_to(context);
    }

    pub(crate) fn apply_intent(&mut self, intent: UiIntent) {
        match intent {
            UiIntent::Quit => self.client.quit_requested = true,
            UiIntent::ShowOwnProfile => self.profile_card_open = true,
            UiIntent::OpenSettings => self.settings_open = true,
            UiIntent::OpenSignIn(username) => {
                if let Some(username) = username {
                    self.login_name = username;
                }
                self.auth_page = Some(AuthPage::Login);
            }
            UiIntent::OpenSignUp(username) => {
                if let Some(username) = username {
                    self.register_name = username;
                }
                self.auth_page = Some(AuthPage::Register);
            }
            UiIntent::Nothing => {}
        }
    }

    // ==================== Per-frame Logic (Don't Draw Interface) ====================

    pub(crate) fn run_logic(&mut self, context: &Context) {
        // The one-shot startup verdict: fold it in when it lands (never before).
        if let Some(outcome) = self.client.take_startup_outcome() {
            let signed_in = self.client.apply_startup(outcome);
            if signed_in {
                // The auto-login session won: close the login page the first
                // frames showed and hand the caret to the message box.
                self.auth_page = None;
                self.focus_message_input = true;
            } else {
                self.client.notify_signed_out();
            }
        }
        self.client.tick();
        while let Some(event) = self.client.next_event() {
            let picked_avatar = matches!(event, PollingEvent::AvatarFileChosen(Some(_)));
            self.client.apply_event(event);
            if picked_avatar {
                // Same follow-up the settings row does: the face the cache holds is
                // the old file, so drop it and let the next frame re-decode.
                let own_id = self.client.current_user_id.clone().unwrap_or_default();
                self.avatars.forget(&own_id);
            }
        }
        if self.client.is_signed_in() {
            self.client.report_typing();
        }
        if self.client.quit_requested {
            context.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Events, input and panel animations wake the loop themselves; only work
        // that is genuinely timed needs a beat, and only while something is pending.
        let beat = if self.timed_work_pending() {
            active_interval()
        } else {
            idle_interval()
        };
        context.request_repaint_after(beat);
    }

    /// Whether anything on screen waits for the clock rather than for an event: a
    /// visible notice, a fading typing indicator or a handshake to retry.
    fn timed_work_pending(&self) -> bool {
        !self.client.notices.is_empty()
            || !self.client.typing_members.is_empty()
            || self
                .client
                .crypto
                .sessions
                .values()
                .any(|session| session.phase != crate::client::EncryptionPhase::Active)
    }

    /// Global shortcut: Esc collapses the floating layers and the Android back
    /// gesture walks one step further; Enter and the arrows belong to the input.
    pub(crate) fn handle_shortcuts(&mut self, context: &Context) {
        let escape = context.input(|state| state.key_pressed(Key::Escape));
        let back = context.input(|state| state.key_pressed(Key::BrowserBack));
        if !escape && !back {
            return;
        }
        if self.creation_page.is_some() {
            self.creation_page = None;
        } else if self.profile_card_open {
            self.profile_card_open = false;
        } else if self.settings_open {
            self.settings_open = false;
        } else if self.group_settings_open {
            self.group_settings_open = false;
        } else if self.search_panel_open {
            // The search panel joins the same overlay chain, last.
            self.search_panel_open = false;
        } else if back && window_is_narrow(context) && self.client.selected_room_index.is_some() {
            // Edge-swipe back on the conversation layer: "return to the parent
            // page" means dropping the room selection, i.e. the room list.
            self.client.close_room_selection();
        }
    }

    // ==================== Top Bar ====================

    pub(crate) fn draw_status_bar(&mut self, ui: &mut Ui) {
        let skin = self.skin.clone();
        let (connection_label, mark, user_text, right_text) = self.client.status_bar_texts();
        let mark_color = if self.client.connection_ready == Some(true) {
            skin.own_username_text
        } else {
            skin.notice_error_border
        };
        egui::Panel::top("status-bar")
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(skin.app_background)
                    .inner_margin(Margin::same(6)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(connection_label);
                    ui.colored_label(mark_color, mark);
                    // The status texts carry their own separators; drawing an extra
                    // one would paint a full-height vertical bar.
                    if !user_text.is_empty() {
                        ui.label(user_text);
                    }
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.colored_label(skin.hint_text, right_text);
                    });
                });
            });
    }

    // ==================== Left: Room List and Settings Switch ====================

    pub(crate) fn draw_notices(&mut self, context: &Context) {
        if self.client.notices.is_empty() {
            return;
        }
        let skin = self.skin.clone();
        let icons = self.frame_icons(context);
        let close_hint = self.text("hint_close_notices");
        let kinds = self.client_notice_kinds_in_order();
        let mut outcomes: Vec<(NoticeKind, (bool, bool, bool))> = Vec::new();
        let mut stack_top = notice_stack_first();
        for kind in kinds {
            let title = self.text(kind.title_key());
            let lines: Vec<&Notice> = self
                .client
                .notices
                .iter()
                .filter(|notice| notice.kind == kind)
                .collect();
            let drawn = draw_notice_window(
                context,
                &skin,
                &icons,
                kind,
                (&title, &close_hint),
                &lines,
                stack_top,
            );
            stack_top += drawn.0.height() + notice_stack_gap();
            outcomes.push((kind, drawn.1));
        }
        for (kind, (closed, viewed, tapped)) in outcomes {
            for notice in self.client.notices.iter_mut() {
                if notice.kind != kind {
                    continue;
                }
                if viewed {
                    notice.read = true;
                }
                if tapped {
                    notice.pinned = true;
                }
                if closed && notice.closing_at.is_none() {
                    notice.closing_at = Some(Instant::now());
                }
            }
        }
        // A glide shorter than the active beat would sample two frames only: while
        // any line is still moving, ask for the next frame at once and stay smooth.
        let glide = Duration::from_secs_f32(notice_glide_seconds());
        let gliding = self
            .client
            .notices
            .iter()
            .any(|notice| match notice.closing_at {
                Some(started) => started.elapsed() < glide,
                None => notice.shown_at.elapsed() < glide,
            });
        if gliding {
            context.request_repaint();
        }
    }

    /// The popup kinds on screen, ordered by the arrival of their first line:
    /// older popups stay on top and a new kind lands below them.
    fn client_notice_kinds_in_order(&self) -> Vec<NoticeKind> {
        let mut kinds: Vec<NoticeKind> = Vec::new();
        for notice in self.client.notices.iter() {
            if !kinds.contains(&notice.kind) {
                kinds.push(notice.kind);
            }
        }
        kinds
    }
    /// The update prompt: a newer package was offered and nothing fetched yet, so ask
    /// first; a yes starts the download and a finished package closes this client.
    pub(crate) fn draw_update_prompt(&mut self, context: &Context) -> Option<Rect> {
        let Some((package, UpdateStage::AwaitingAnswer)) = self.client.pending_update.clone()
        else {
            return None;
        };
        let skin = self.skin.clone();
        let icons = self.frame_icons(context);
        let title = self
            .text("update_available_title")
            .replace("{version}", &package.version);
        let body = self
            .text("update_available_body")
            .replace("{version}", &package.version)
            .replace("{package}", &package.file_name);
        let mut accept = false;
        let mut decline = false;
        let response = Modal::new(Id::new("update-prompt"))
            .backdrop_color(Color32::from_black_alpha(140))
            .frame(
                Frame::new()
                    .fill(skin.app_background)
                    .stroke(Stroke::new(1.0, skin.overlay_border))
                    .corner_radius(8.0)
                    .inner_margin(Margin::same(16)),
            )
            .show(context, |ui| {
                ui.label(RichText::new(title).color(skin.own_username_text));
                ui.label(RichText::new(body).color(skin.message_text));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if icon_button(
                        ui,
                        &skin,
                        &icons,
                        IconName::Proceed,
                        Some(self.text("option_update_client")),
                    )
                    .clicked()
                    {
                        accept = true;
                    }
                    if icon_button(
                        ui,
                        &skin,
                        &icons,
                        IconName::Cancel,
                        Some(self.text("button_later")),
                    )
                    .clicked()
                    {
                        decline = true;
                    }
                });
            });
        if accept {
            self.client.start_download();
        } else if decline {
            self.client.pending_update = None;
        }
        Some(response.response.rect)
    }
}

/// How long the frame loop may sleep while timed work is pending: fast enough
/// that an expiring notice, a fading indicator or a retry never visibly lags.
fn active_interval() -> Duration {
    Duration::from_millis(120)
}

/// How long the frame loop may sleep with nothing pending. It is only a safety
/// net, so it may be slow and an idle window stops burning layout passes.
fn idle_interval() -> Duration {
    Duration::from_secs(1)
}

impl eframe::App for BaihuaApp {
    /// Inject the input egui cannot see: the iOS soft keyboard hands Return over as
    /// `insertText:@"\n"`, which `ios_platform` remembers and this hook replays.
    fn raw_input_hook(&mut self, _context: &Context, raw_input: &mut egui::RawInput) {
        #[cfg(target_os = "ios")]
        if crate::ios_platform::take_return_key() {
            raw_input.events.push(Event::Key {
                key: Key::Enter,
                physical_key: Some(Key::Enter),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            });
        }
        // Desktops and Android need no injected input.
        #[cfg(not(target_os = "ios"))]
        let _ = raw_input;
    }

    /// Run the frame logic first: background events, notices, handshakes, exit.
    fn logic(&mut self, context: &Context, _frame: &mut eframe::Frame) {
        self.run_logic(context);
        // Android: keep the point scale honest before anything is drawn this frame.
        #[cfg(target_os = "android")]
        self.apply_screen_scale(context);
        // Android: the status-bar spacer follows the real window geometry, so
        // rotation and keyboard transitions re-measure exactly once.
        #[cfg(target_os = "android")]
        if let Some(android_application) = &self.android_application {
            crate::android_platform::remeasure_viewport(android_application, context);
        }
        // iOS: the Return-key swizzle (retries until winit's view class exists)
        // and the one-time notification setup.
        #[cfg(target_os = "ios")]
        crate::ios_platform::install();
    }

    /// Then draw the interface; the panel order determines the positioning.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        // Mobile: a blank row of the measured inset keeps the app's own top bar
        // below the system status bar.
        #[cfg(any(target_os = "android", target_os = "ios"))]
        draw_status_spacer(ui, &self.skin);
        self.draw_status_bar(ui);
        if window_is_narrow(&context) {
            // A narrow window draws exactly one layer at a time.
            match resolve_narrow_layer(
                self.client.selected_room_index.is_some(),
                self.group_settings_open,
            ) {
                NarrowLayer::RoomList => self.draw_room_panel(ui),
                NarrowLayer::Conversation => {
                    let mut group_settings = self.group_settings_view();
                    self.draw_central(ui, &context, Rect::NOTHING, &mut group_settings);
                }
                NarrowLayer::GroupSettings => self.draw_sidebar_layer(ui),
            }
        } else {
            // Wide layout: the sidebar joins the room list on the window's own
            // layer, so its background bottom edge meets the room list's exactly.
            self.draw_room_panel(ui);
            let mut group_settings = self.group_settings_view();
            let sidebar_rect = self.draw_sidebar_top(ui, &mut group_settings);
            self.draw_central(ui, &context, sidebar_rect, &mut group_settings);
        }
        self.handle_shortcuts(&context);
        self.draw_settings_window(&context);
        // The search panel belongs to group rooms only, like the magnifier button,
        // and is checked per frame because it floats above every layout layer.
        let current_room_is_group = self
            .client
            .current_room_id()
            .is_some_and(|room_id| self.client.room_is_group(&room_id));
        if !current_room_is_group {
            self.search_panel_open = false;
        }
        self.draw_search_panel(&context);
        self.draw_update_prompt(&context);
        self.draw_notices(&context);
        self.draw_profile_card(&context);
        self.draw_creation_panel(&context);
        self.draw_auth_page(&context);
    }

    /// On exit: flush the message cache and persist the session. The signature
    /// differs per operating system because iOS builds with the wgpu renderer.
    #[cfg(target_os = "ios")]
    fn on_exit(&mut self) {
        self.client.flush_before_exit();
        self.client.persist_session();
    }

    #[cfg(not(target_os = "ios"))]
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.client.flush_before_exit();
        self.client.persist_session();
    }

    /// eframe calls this when the operating system suspends the app; on Android it
    /// is the last moment before a recents-swipe kill, where `on_exit` never runs.
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.client.flush_before_exit();
        self.client.persist_session();
    }
}

/// Distance (points) a notice popup has travelled past its resting spot: the entry
/// glide starts one card-width plus margin off the right screen edge and settles at
/// zero, the exit glide runs the same path back out.
pub(crate) fn notice_glide_distance(lines: &[&Notice], width: f32) -> f32 {
    let glide = notice_glide_seconds().max(f32::EPSILON);
    let travel = width + notice_glide_margin();
    if lines.iter().all(|line| line.closing_at.is_some()) {
        let started = lines
            .iter()
            .filter_map(|line| line.closing_at)
            .min()
            .expect("a closing popup carries at least one line");
        let fraction = (started.elapsed().as_secs_f32() / glide).clamp(0.0, 1.0);
        travel * emath::easing::cubic_in(fraction)
    } else {
        // The earliest live arrival drives the entry: a merged line keeps the
        // older timestamp, so gaining text never replays the glide-in.
        let shown = lines
            .iter()
            .filter(|line| line.closing_at.is_none())
            .map(|line| line.shown_at)
            .min()
            .expect("an open popup carries at least one live line");
        let fraction = (shown.elapsed().as_secs_f32() / glide).clamp(0.0, 1.0);
        travel * (1.0 - emath::easing::cubic_out(fraction))
    }
}

/// Draw one notice popup (all lines of one kind) as a card anchored at the given
/// height below the right screen edge, gliding in and out horizontally. A hover or
/// tap marks the card viewed (the red dot off; a tap also pins its close control),
/// and the corner control answers as close. Returns the card rectangle with the
/// per-card outcomes (closed, viewed, tapped).
pub(crate) fn draw_notice_window(
    context: &Context,
    skin: &Skin,
    icons: &IconFrame,
    kind: NoticeKind,
    texts: (&str, &str),
    lines: &[&Notice],
    stack_top: f32,
) -> (Rect, (bool, bool, bool)) {
    let (title, close_hint) = texts;
    let card_id = Id::new(("notice-card", kind));
    let pinned = lines.iter().any(|line| line.pinned);
    let unread = lines.iter().any(|line| !line.read);
    let border = if kind == NoticeKind::Error {
        skin.notice_error_border
    } else {
        skin.notice_hint_border
    };
    // The card size measured last frame sets the glide distance; egui runs an
    // invisible sizing pass on a brand-new area, so the first visible frame knows it.
    let size = context
        .data(|data| data.get_temp::<Vec2>(card_id))
        .unwrap_or_default();
    let offset = Vec2::new(
        notice_glide_distance(lines, size.x) - notice_edge_inset(),
        stack_top,
    );
    let inner = Area::new(card_id)
        .anchor(Align2::RIGHT_TOP, offset)
        // The glide lives outside the screen edge, so egui must not clamp the spot
        // back into view; the middle order keeps the foreground close control visible.
        .order(Order::Middle)
        .constrain(false)
        .show(context, |ui| {
            // Lines define the card width: extend instead of wrapping in the box,
            // and plain labels must not swallow the taps meant for the card body.
            ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
            ui.style_mut().interaction.selectable_labels = false;
            Frame::new()
                .fill(skin.app_background)
                .stroke(Stroke::new(if pinned { 2.0 } else { 1.0 }, border))
                .corner_radius(overlay_radius())
                .inner_margin(Margin::same(8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (slot, _) = ui.allocate_exact_size(
                            Vec2::splat(notice_dot_side() + 4.0),
                            Sense::hover(),
                        );
                        if unread {
                            ui.painter().circle_filled(
                                slot.center(),
                                notice_dot_side() / 2.0,
                                skin.notice_error_border,
                            );
                        }
                        ui.label(RichText::new(title).color(border).strong());
                    });
                    for line in lines {
                        ui.colored_label(border, &line.text);
                    }
                })
        });
    let window_rect = inner.inner.response.rect;
    context.data_mut(|data| data.insert_temp(card_id, window_rect.size()));
    // The control straddles the frame's top-left corner, as the old popup had it,
    // so it reads as the card's corner rather than as another notice line.
    let corner_rect = Rect::from_center_size(window_rect.min, Vec2::splat(notice_close_side()));
    let corner_hover = context.input(|input| {
        input
            .pointer
            .hover_pos()
            .is_some_and(|point| window_rect.union(corner_rect).contains(point))
    });
    let hovered = inner.response.hovered() || corner_hover;
    let tapped = inner.response.clicked();
    let mut closed = false;
    if hovered || pinned {
        Area::new(Id::new(("notice-close-corner", kind)))
            .order(Order::Foreground)
            .fixed_pos(corner_rect.min)
            .show(context, |ui| {
                if icon_button_rect(ui, skin, icons, IconName::Cancel, None, corner_rect)
                    .on_hover_text(close_hint.to_string())
                    .clicked()
                {
                    closed = true;
                }
            });
    }
    (window_rect, (closed, hovered || tapped, tapped))
}

#[cfg(test)]
mod rendered_text_tests {
    use crate::app::auth::auth_draft_tests::test_app;
    use crate::app::test_support::frame;
    use baihua_core::api::MessageInfo;
    pub(crate) use egui::{CentralPanel, Context, Pos2, RawInput};

    fn raw_input() -> RawInput {
        frame(900.0, 600.0, Vec::new())
    }

    /// The top bar paints no separator of its own: the vertical bars live inside
    /// the status texts.
    #[test]
    fn status_bar_lines() {
        let context = Context::default();
        let mut app = test_app();
        app.client.current_username = "alice".to_string();
        let mut line_segments: Vec<(Pos2, Pos2)> = Vec::new();
        context
            .run_ui(raw_input(), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    app.draw_status_bar(ui);
                });
                line_segments = ctx.graphics_mut(|graphics| {
                    let mut lines: Vec<(Pos2, Pos2)> = Vec::new();
                    if let Some(list) = graphics.get(egui::LayerId::background()) {
                        for entry in list.all_entries() {
                            if let egui::Shape::LineSegment { points, .. } = &entry.shape {
                                lines.push((points[0], points[1]));
                            }
                        }
                    }
                    lines
                });
            })
            .drop_without_applying_deltas();
        for (start, end) in line_segments {
            let painted_height = (end.y - start.y).abs();
            assert!(
                painted_height <= 4.0,
                "the status bar painted its own {painted_height}-tall vertical line ({start:?} -> {end:?}): pipes belong in the texts"
            );
        }
    }

    /// A message with an empty body (encrypted history the server cannot read back)
    /// must be drawn with the localized placeholder, never as a blank row.
    #[test]
    fn body_placeholder() {
        let context = Context::default();
        let mut app = test_app();
        app.client.messages = vec![MessageInfo {
            id: "message-secret".to_string(),
            room_id: "room-secret".to_string(),
            sender_id: "user-other".to_string(),
            content: String::new(),
            created_at: "2026-09-06T00:00:00+00:00".to_string(),
        }];
        let placeholder = app.client.text("message_encrypted_history_unavailable");
        let rows = app.message_rows(&context);
        assert_eq!(rows.len(), 1);
        assert_ne!(
            rows[0].content, "",
            "an empty body must not paint as a bare blank row"
        );
        assert_eq!(rows[0].content, placeholder);
    }
    /// The prompt comes from the session state alone: no offer shows nothing, a
    /// waiting answer shows the modal, an agreed download never asks a second time.
    #[test]
    fn prompt_when_waiting() {
        use crate::client::UpdateStage;
        let context = Context::default();
        let mut app = test_app();
        // egui has no font table before the very first frame, so warm the context up
        context
            .run_ui(raw_input(), |_ui| {})
            .drop_without_applying_deltas();
        assert!(
            app.draw_update_prompt(&context).is_none(),
            "no offer, no modal"
        );
        let package = offered_package();
        app.client.pending_update = Some((package.clone(), UpdateStage::AwaitingAnswer));
        let shown = app
            .draw_update_prompt(&context)
            .expect("the answer must be on screen");
        assert!(shown.width() > 0.0 && shown.height() > 0.0);
        app.client.pending_update = Some((package, UpdateStage::Downloading));
        assert!(app.draw_update_prompt(&context).is_none(), "no second ask");
    }

    /// The notice card stays drawn but inert away from its corner, and the corner
    /// close control only answers once the pointer hovers the card itself.
    #[test]
    fn notice_close_button() {
        use crate::app::icons::test_icons;
        use crate::app::test_support::click_at;

        /// A glide then a press and release at the same point: egui hit-tests the
        /// previous frame's widgets, so hover must precede the click by one frame.
        fn hover_then_click(point: egui::Pos2) -> Vec<egui::Event> {
            let mut events = vec![egui::Event::PointerMoved(point)];
            events.extend(click_at(point));
            events
        }
        /// A hint line whose entry glide has already settled, so the card rect
        /// stays put between the hover frame and the click frame under test.
        fn notice_line(text: &str) -> crate::client::Notice {
            let now = std::time::Instant::now();
            crate::client::Notice {
                kind: crate::client::NoticeKind::Hint,
                text: text.to_string(),
                expires_at: now + std::time::Duration::from_secs(6),
                shown_at: now - std::time::Duration::from_millis(500),
                closing_at: None,
                read: false,
                pinned: false,
            }
        }

        /// Draw one frame of the card and hand back the last outcomes.
        fn draw_card(
            context: &Context,
            skin: &crate::appearance::Skin,
            icons: &crate::app::icons::IconFrame,
            lines: &[&crate::client::Notice],
            events: Vec<egui::Event>,
        ) -> (Rect, (bool, bool, bool)) {
            let mut seen = (Rect::NOTHING, (false, false, false));
            context
                .run_ui(frame(900.0, 600.0, events), |ctx| {
                    seen = crate::app::draw_notice_window(
                        ctx,
                        skin,
                        icons,
                        crate::client::NoticeKind::Hint,
                        ("title", "close"),
                        lines,
                        34.0,
                    );
                })
                .drop_without_applying_deltas();
            seen
        }
        use crate::appearance::Skin;
        use baihua_core::config::Palette;
        use egui::{Context, Rect, Vec2};
        let context = Context::default();
        // egui has no font table before the very first frame, so warm the context up
        context
            .run_ui(frame(900.0, 600.0, Vec::new()), |_ctx| {})
            .drop_without_applying_deltas();
        let skin = Skin::from(&Palette::built_in());
        let icons = test_icons(&context);
        let owned = [notice_line("first notice"), notice_line("second notice")];
        let lines: Vec<&crate::client::Notice> = owned.iter().collect();
        let mut window = Rect::NOTHING;
        let mut outcomes = (false, false, false);
        for _ in 0..2 {
            let drawn = draw_card(&context, &skin, &icons, &lines, Vec::new());
            window = drawn.0;
            outcomes = drawn.1;
        }
        assert!(window.is_positive(), "the popup must be on screen");
        assert!(!outcomes.0, "a frame without a click must not dismiss");
        let far = window.max - Vec2::splat(6.0);
        outcomes = draw_card(&context, &skin, &icons, &lines, hover_then_click(far)).1;
        assert!(!outcomes.0, "a click inside the popup body is not a close");
        assert!(outcomes.2, "a click inside the body taps the card");
        // The control straddles the frame corner, so a point just inside it is the hit.
        let corner = window.min + Vec2::splat(5.0);
        draw_card(
            &context,
            &skin,
            &icons,
            &lines,
            vec![egui::Event::PointerMoved(corner)],
        );
        outcomes = draw_card(&context, &skin, &icons, &lines, hover_then_click(corner)).1;
        assert!(
            outcomes.0,
            "the top-left corner close control must answer while hovered"
        );
    }

    /// The offer a macOS disk image release makes; the address is never fetched.
    fn offered_package() -> baihua_core::update::ReleasePackage {
        baihua_core::update::ReleasePackage {
            version: "9.9.9".to_string(),
            tag: "gui-v9.9.9".to_string(),
            file_name: "baihua-gui-9.9.9-aarch64-apple-darwin.dmg".to_string(),
            download_url: "https://example.invalid/package.dmg".to_string(),
            size_bytes: 1,
        }
    }
}
pub(crate) mod auth;
pub(crate) mod conversation;
pub(crate) mod creation;
pub(crate) mod group_sidebar;
pub(crate) mod icons;
pub(crate) mod layout;
pub(crate) mod message_input;
pub(crate) mod room_panel;
pub(crate) mod search;
pub(crate) mod settings;
#[cfg(test)]
pub(crate) mod test_support;
pub(crate) mod widgets;

pub(crate) use auth::*;
pub(crate) use conversation::*;
pub(crate) use creation::*;
pub(crate) use group_sidebar::*;
pub(crate) use icons::*;
pub(crate) use layout::*;
pub(crate) use message_input::*;
pub(crate) use search::*;
pub(crate) use settings::*;
pub(crate) use widgets::*;
