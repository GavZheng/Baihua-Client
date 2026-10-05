//! The settings window: profile, avatar, password, requests, account and server
//! sections, plus the profile card and the methods that read and write app state.

use super::*;

/// Whether this target offers an operating system file dialog: phones hide the
/// picker button and keep the avatar directory list as their only local entry.
pub(crate) fn picker_supported() -> bool {
    cfg!(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux"
    ))
}

/// Side of the avatar block in the profile card (pixels), larger than in a row.
pub(crate) fn profile_avatar_side() -> usize {
    64
}

/// Expandable sections in the settings panel
#[derive(Clone, Copy, Default)]
pub(crate) struct Sections {
    pub(crate) profile: bool,
    pub(crate) password: bool,
    pub(crate) avatar: bool,
    pub(crate) requests: bool,
    pub(crate) account: bool,
    pub(crate) server: bool,
}

/// All text and data the settings panel needs to read and write
#[derive(Default)]
pub(crate) struct SettingsView {
    pub(crate) settings_title: String,
    pub(crate) language_title: String,
    pub(crate) appearance_title: String,
    pub(crate) show_uid: bool,
    pub(crate) show_uid_title: String,
    pub(crate) time_with_date: bool,
    pub(crate) time_title: String,
    pub(crate) quick_search: bool,
    pub(crate) quick_title: String,
    pub(crate) sound_enabled: bool,
    /// Phones: the explanation row that gates the system permission dialog.
    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub(crate) permission_prompt: bool,
    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub(crate) permission_title: String,
    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub(crate) permission_body: String,
    pub(crate) sound_title: String,
    pub(crate) avatar_title: String,
    pub(crate) avatar_choices: Vec<(String, PathBuf)>,
    pub(crate) avatar_directory: Option<String>,
    pub(crate) avatar_empty_hint: String,
    pub(crate) avatar_input_label: String,
    /// Text of the operating system file dialog button (hidden where unsupported)
    pub(crate) avatar_pick_title: String,
    /// Whether this target has a real file dialog, so the button is drawn at all
    pub(crate) avatar_picker_supported: bool,
    pub(crate) profile_title: String,
    pub(crate) profile_hint: String,
    pub(crate) nickname_label: String,
    pub(crate) phone_label: String,
    pub(crate) bio_label: String,
    pub(crate) password_title: String,
    pub(crate) old_label: String,
    pub(crate) new_label: String,
    pub(crate) repeat_label: String,
    pub(crate) requests_title: String,
    pub(crate) requests_empty: String,
    pub(crate) received_title: String,
    pub(crate) sent_title: String,
    pub(crate) accept_title: String,
    pub(crate) decline_title: String,
    pub(crate) cancel_title: String,
    pub(crate) account_title: String,
    pub(crate) delete_hint: String,
    pub(crate) server_title: String,
    pub(crate) server_hint: String,
    pub(crate) update_title: String,
    pub(crate) logout_title: String,
    pub(crate) confirm_title: String,
    pub(crate) save_title: String,
    pub(crate) current_language: String,
    pub(crate) language_codes: Vec<String>,
    pub(crate) current_appearance: String,
    pub(crate) appearance_names: Vec<String>,
    pub(crate) request_rows: Vec<RequestRow>,
    pub(crate) pending_count: usize,
    pub(crate) sections: Sections,
    /// Content of each text box in the panel, written back to the interface as-is after drawing
    pub(crate) fields: SettingsFields,
}

/// Content of each text box in the settings panel
#[derive(Default)]
pub(crate) struct SettingsFields {
    pub(crate) profile_nickname: String,
    pub(crate) profile_phone: String,
    pub(crate) profile_bio: String,
    pub(crate) password_old: String,
    pub(crate) password_new: String,
    pub(crate) password_repeat: String,
    pub(crate) avatar_url: String,
    pub(crate) delete_password: String,
    pub(crate) server_address: String,
}

/// The actions the settings panel hands back to the session layer
pub(crate) enum SettingsOutcome {
    Nothing,
    Language(String),
    Appearance(String),
    Toggles {
        show_uid: bool,
        time_with_date: bool,
        quick_search: bool,
        sound_enabled: bool,
    },
    /// Phones: the switch stops at the explanation row before the dialog.
    #[cfg(any(target_os = "android", target_os = "ios"))]
    OpenPermissionPrompt,
    #[cfg(any(target_os = "android", target_os = "ios"))]
    NotificationsConfirmed,
    #[cfg(any(target_os = "android", target_os = "ios"))]
    NotificationsCancelled,
    Sections(Sections),
    AvatarFile(PathBuf),
    AvatarUrl(String),
    OpenAvatarPicker,
    Profile {
        nickname: String,
        phone: String,
        bio: String,
    },
    Password {
        old: String,
        new: String,
        repeat: String,
    },
    Accept(String),
    Decline(String),
    Cancel(String),
    DeleteAccount(String),
    ServerAddress(String),
    CheckUpdate,
    Logout,
}

/// All content the profile card needs to display (computed first, only read during drawing)
pub(crate) struct ProfileCardView {
    pub(crate) username: String,
    pub(crate) nickname: Option<String>,
    pub(crate) id: String,
    pub(crate) email: String,
    pub(crate) phone: String,
    pub(crate) bio: String,
    pub(crate) avatar: String,
    pub(crate) presence: String,
    pub(crate) texture_identity: String,
    pub(crate) bytes: Option<Vec<u8>>,
    /// Field labels in display order: nickname, UID, presence, email, phone, bio, avatar URL.
    /// The first one is only drawn when a nickname exists; the remaining six always get a row.
    pub(crate) labels: Vec<String>,
}

/// A row of a private chat request
pub(crate) struct RequestRow {
    pub(crate) id: String,
    pub(crate) is_sent: bool,
    pub(crate) peer: String,
    pub(crate) status: String,
    pub(crate) message: String,
    pub(crate) actionable: bool,
    pub(crate) cancellable: bool,
}

pub(crate) fn draw_settings_form(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    view: &mut SettingsView,
) -> SettingsOutcome {
    let mut outcome = SettingsOutcome::Nothing;
    ui.colored_label(skin.selected_text, view.settings_title.clone());
    ui.separator();
    ui.label(view.language_title.clone());
    ui.horizontal_wrapped(|ui| {
        for code in &view.language_codes {
            let selected = code == &view.current_language;
            if draw_switch(ui, skin, selected, code.clone()).clicked() {
                outcome = SettingsOutcome::Language(code.clone());
            }
        }
    });
    ui.label(view.appearance_title.clone());
    ui.horizontal_wrapped(|ui| {
        for name in &view.appearance_names {
            let selected = name == &view.current_appearance;
            if draw_switch(ui, skin, selected, name.clone()).clicked() {
                outcome = SettingsOutcome::Appearance(name.clone());
            }
        }
    });
    ui.separator();
    let mut changed = false;
    changed |= ui
        .checkbox(&mut view.show_uid, view.show_uid_title.clone())
        .changed();
    changed |= ui
        .checkbox(&mut view.time_with_date, view.time_title.clone())
        .changed();
    changed |= ui
        .checkbox(&mut view.quick_search, view.quick_title.clone())
        .changed();
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let sound_before = view.sound_enabled;
    changed |= ui
        .checkbox(&mut view.sound_enabled, view.sound_title.clone())
        .changed();
    // Phones: flipping the switch on stops at the explanation row first, so
    // the system dialog always rides a user gesture, never the message path.
    #[cfg(any(target_os = "android", target_os = "ios"))]
    if view.sound_enabled && !sound_before {
        view.sound_enabled = false;
        changed = false;
        outcome = SettingsOutcome::OpenPermissionPrompt;
    }
    if changed {
        outcome = SettingsOutcome::Toggles {
            show_uid: view.show_uid,
            time_with_date: view.time_with_date,
            quick_search: view.quick_search,
            sound_enabled: view.sound_enabled,
        };
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    if view.permission_prompt {
        ui.group(|ui| {
            ui.label(view.permission_title.clone());
            ui.label(view.permission_body.clone());
            ui.horizontal(|ui| {
                if icon_button(
                    ui,
                    skin,
                    icons,
                    IconName::Confirm,
                    Some(view.confirm_title.clone()),
                )
                .clicked()
                {
                    outcome = SettingsOutcome::NotificationsConfirmed;
                }
                if icon_button(
                    ui,
                    skin,
                    icons,
                    IconName::Cancel,
                    Some(view.cancel_title.clone()),
                )
                .clicked()
                {
                    outcome = SettingsOutcome::NotificationsCancelled;
                }
            });
        });
    }
    ui.separator();
    draw_avatar_group(ui, skin, icons, view, &mut outcome);
    draw_profile_group(ui, skin, icons, view, &mut outcome);
    draw_password_group(ui, skin, icons, view, &mut outcome);
    draw_requests_group(ui, skin, icons, view, &mut outcome);
    draw_account_group(ui, skin, icons, view, &mut outcome);
    ui.separator();
    if icon_button(
        ui,
        skin,
        icons,
        IconName::Proceed,
        Some(view.update_title.clone()),
    )
    .clicked()
    {
        outcome = SettingsOutcome::CheckUpdate;
    }
    if icon_button(
        ui,
        skin,
        icons,
        IconName::Exit,
        Some(view.logout_title.clone()),
    )
    .clicked()
    {
        outcome = SettingsOutcome::Logout;
    }
    outcome
}

pub(crate) fn draw_avatar_group(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    view: &mut SettingsView,
    outcome: &mut SettingsOutcome,
) {
    // The open state must follow the same path as every other section and be handed
    // back whole; returning on "no click this frame" made the section look dead.
    toggle_section(
        ui,
        skin,
        &mut view.sections,
        |sections| &mut sections.avatar,
        view.avatar_title.clone(),
        outcome,
    );
    if !view.sections.avatar {
        return;
    }
    // The project button style (bordered, one text line plus padding): a bare
    // `ui.button` rendered too small next to the avatar rows below it.
    if view.avatar_picker_supported
        && draw_switch(ui, skin, false, view.avatar_pick_title.clone()).clicked()
    {
        *outcome = SettingsOutcome::OpenAvatarPicker;
    }
    if view.avatar_choices.is_empty() {
        ui.colored_label(skin.hint_text, view.avatar_empty_hint.clone());
        if let Some(directory) = &view.avatar_directory {
            ui.colored_label(skin.hint_text, directory.clone());
        }
    } else {
        ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
            for (name, path) in &view.avatar_choices {
                if draw_switch(ui, skin, false, name.clone()).clicked() {
                    *outcome = SettingsOutcome::AvatarFile(path.clone());
                }
            }
        });
    }
    ui.horizontal(|ui| {
        ui.label(view.avatar_input_label.clone());
        ui.text_edit_singleline(&mut view.fields.avatar_url);
        if icon_button(
            ui,
            skin,
            icons,
            IconName::Confirm,
            Some(view.confirm_title.clone()),
        )
        .clicked()
        {
            *outcome = SettingsOutcome::AvatarUrl(view.fields.avatar_url.clone());
        }
    });
}

pub(crate) fn draw_profile_group(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    view: &mut SettingsView,
    outcome: &mut SettingsOutcome,
) {
    toggle_section(
        ui,
        skin,
        &mut view.sections,
        |sections| &mut sections.profile,
        view.profile_title.clone(),
        outcome,
    );
    if !view.sections.profile {
        return;
    }
    ui.label(view.nickname_label.clone());
    ui.text_edit_singleline(&mut view.fields.profile_nickname);
    ui.label(view.phone_label.clone());
    ui.text_edit_singleline(&mut view.fields.profile_phone);
    ui.label(view.bio_label.clone());
    ui.text_edit_singleline(&mut view.fields.profile_bio);
    if icon_button(
        ui,
        skin,
        icons,
        IconName::Confirm,
        Some(view.save_title.clone()),
    )
    .clicked()
    {
        *outcome = SettingsOutcome::Profile {
            nickname: view.fields.profile_nickname.clone(),
            phone: view.fields.profile_phone.clone(),
            bio: view.fields.profile_bio.clone(),
        };
    }
    ui.colored_label(skin.hint_text, view.profile_hint.clone());
}

pub(crate) fn draw_password_group(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    view: &mut SettingsView,
    outcome: &mut SettingsOutcome,
) {
    toggle_section(
        ui,
        skin,
        &mut view.sections,
        |sections| &mut sections.password,
        view.password_title.clone(),
        outcome,
    );
    if !view.sections.password {
        return;
    }
    ui.horizontal(|ui| {
        ui.label(view.old_label.clone());
        ui.add(TextEdit::singleline(&mut view.fields.password_old).password(true));
    });
    ui.horizontal(|ui| {
        ui.label(view.new_label.clone());
        ui.add(TextEdit::singleline(&mut view.fields.password_new).password(true));
    });
    ui.horizontal(|ui| {
        ui.label(view.repeat_label.clone());
        ui.add(TextEdit::singleline(&mut view.fields.password_repeat).password(true));
    });
    if icon_button(
        ui,
        skin,
        icons,
        IconName::Confirm,
        Some(view.confirm_title.clone()),
    )
    .clicked()
    {
        *outcome = SettingsOutcome::Password {
            old: view.fields.password_old.clone(),
            new: view.fields.password_new.clone(),
            repeat: view.fields.password_repeat.clone(),
        };
    }
}

pub(crate) fn draw_requests_group(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    view: &mut SettingsView,
    outcome: &mut SettingsOutcome,
) {
    let title = format!("{} ({})", view.requests_title, view.pending_count);
    toggle_section(
        ui,
        skin,
        &mut view.sections,
        |sections| &mut sections.requests,
        title,
        outcome,
    );
    if !view.sections.requests {
        return;
    }
    if view.request_rows.is_empty() {
        ui.colored_label(skin.hint_text, view.requests_empty.clone());
        return;
    }
    ui.colored_label(skin.selected_text, view.received_title.clone());
    for row in view.request_rows.iter().filter(|row| !row.is_sent) {
        ui.group(|ui| {
            ui.label(format!("{}  {}", row.peer, row.status));
            ui.colored_label(skin.hint_text, row.message.clone());
            if row.actionable {
                ui.horizontal(|ui| {
                    if icon_button(
                        ui,
                        skin,
                        icons,
                        IconName::Confirm,
                        Some(view.accept_title.clone()),
                    )
                    .clicked()
                    {
                        *outcome = SettingsOutcome::Accept(row.id.clone());
                    }
                    if icon_button(
                        ui,
                        skin,
                        icons,
                        IconName::Cancel,
                        Some(view.decline_title.clone()),
                    )
                    .clicked()
                    {
                        *outcome = SettingsOutcome::Decline(row.id.clone());
                    }
                });
            }
        });
    }
    ui.colored_label(skin.selected_text, view.sent_title.clone());
    for row in view.request_rows.iter().filter(|row| row.is_sent) {
        ui.group(|ui| {
            ui.label(format!("{}  {}", row.peer, row.status));
            ui.colored_label(skin.hint_text, row.message.clone());
            if row.cancellable
                && icon_button(
                    ui,
                    skin,
                    icons,
                    IconName::Cancel,
                    Some(view.cancel_title.clone()),
                )
                .clicked()
            {
                *outcome = SettingsOutcome::Cancel(row.id.clone());
            }
        });
    }
}

pub(crate) fn draw_account_group(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    view: &mut SettingsView,
    outcome: &mut SettingsOutcome,
) {
    toggle_section(
        ui,
        skin,
        &mut view.sections,
        |sections| &mut sections.account,
        view.account_title.clone(),
        outcome,
    );
    if view.sections.account {
        ui.colored_label(skin.notice_error_border, view.delete_hint.clone());
        ui.horizontal(|ui| {
            // One row: label, password box, and the confirm button pinned at the row's
            // right end, visible but inert until a password is typed.
            ui.label(view.old_label.clone());
            let can_delete = !view.fields.delete_password.trim().is_empty();
            let button_side = button_minimum_size(ui);
            let box_width =
                (ui.available_width() - button_side.x - 2.0 * ui.spacing().item_spacing.x)
                    .at_least(minimum_input_width());
            ui.add(
                TextEdit::singleline(&mut view.fields.delete_password)
                    .password(true)
                    .desired_width(box_width),
            );
            if icon_button_sized(
                ui,
                skin,
                icons,
                IconName::DeleteAccount,
                Some(view.confirm_title.clone()),
                button_side,
                can_delete,
            )
            .clicked()
            {
                *outcome = SettingsOutcome::DeleteAccount(view.fields.delete_password.clone());
            }
        });
    }
    ui.separator();
    toggle_section(
        ui,
        skin,
        &mut view.sections,
        |sections| &mut sections.server,
        view.server_title.clone(),
        outcome,
    );
    if view.sections.server {
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut view.fields.server_address);
            if icon_button(
                ui,
                skin,
                icons,
                IconName::Confirm,
                Some(view.confirm_title.clone()),
            )
            .clicked()
            {
                *outcome = SettingsOutcome::ServerAddress(view.fields.server_address.clone());
            }
        });
        ui.colored_label(skin.hint_text, view.server_hint.clone());
    }
}

/// Expandable section title: a click toggles it and writes the whole new `Sections`
/// state back through the outcome, since a local toggle made the clicks look dead.
pub(crate) fn toggle_section(
    ui: &mut Ui,
    skin: &Skin,
    sections: &mut Sections,
    pick: impl Fn(&mut Sections) -> &mut bool,
    title: String,
    outcome: &mut SettingsOutcome,
) {
    let selected = *pick(sections);
    if draw_switch(ui, skin, selected, title).clicked() {
        let open = pick(sections);
        *open = !*open;
        *outcome = SettingsOutcome::Sections(*sections);
    }
}

impl BaihuaApp {
    /// Settings window: a standalone floating layer, no longer occupying the message display area (entry is the gear to the right of the room title).
    /// The window has slide-in/slide-out and collapse; when content is too tall, the inside of the window scrolls.
    pub(crate) fn draw_settings_window(&mut self, context: &Context) {
        let skin = self.skin.clone();
        let title = self.text("settings_title");
        let mut open = self.settings_open;
        Window::new(title)
            .id(Id::new(settings_window_id()))
            .open(&mut open)
            .collapsible(true)
            .resizable(true)
            .default_pos([settings_offset(), settings_offset()])
            .default_size([settings_width(), settings_height()])
            .max_height(settings_height())
            .scroll(true)
            .frame(overlay_window_frame(&skin))
            .show(context, |ui| {
                let icons = self.frame_icons(context);
                let mut view = self.state_view();
                let outcome = draw_settings_form(ui, &skin, &icons, &mut view);
                let fields = view.fields;
                self.apply_fields(fields);
                self.apply_settings(outcome, ui.ctx());
            });
        self.settings_open = open;
    }

    /// Take a snapshot of the fields the settings panel needs to read (avoid borrowing the entire self in the interface closure)
    /// Text box content the settings panel needs to read and write (the panel closure doesn't touch self, so take it out first and write it back)
    pub(crate) fn settings_fields(&self) -> SettingsFields {
        SettingsFields {
            profile_nickname: if self.profile_nickname.is_empty() {
                self.client.profile_nickname_draft.clone()
            } else {
                self.profile_nickname.clone()
            },
            profile_phone: self.profile_phone.clone(),
            profile_bio: if self.profile_bio.is_empty() {
                self.client.profile_bio_draft.clone()
            } else {
                self.profile_bio.clone()
            },
            password_old: self.password_old.clone(),
            password_new: self.password_new.clone(),
            password_repeat: self.password_repeat.clone(),
            avatar_url: self.avatar_url.clone(),
            delete_password: self.delete_password.clone(),
            server_address: if self.server_address.is_empty() {
                self.client.connector.base_url().to_string()
            } else {
                self.server_address.clone()
            },
        }
    }

    pub(crate) fn state_view(&self) -> SettingsView {
        SettingsView {
            settings_title: self.text("settings_title"),
            language_title: self.text("option_language"),
            appearance_title: self.text("option_appearance"),
            show_uid: self.client.show_uid,
            show_uid_title: self.text("option_show_uid"),
            time_with_date: self.client.time_with_date,
            time_title: self.text("option_time_format"),
            quick_search: self.client.quick_search,
            quick_title: self.text("option_quick_search"),
            sound_enabled: self.client.sound_enabled,
            sound_title: self.text("option_sound_enabled"),
            #[cfg(any(target_os = "android", target_os = "ios"))]
            permission_prompt: self.notification_prompt_open,
            #[cfg(any(target_os = "android", target_os = "ios"))]
            permission_title: self.text("dialog_permission_title"),
            #[cfg(any(target_os = "android", target_os = "ios"))]
            permission_body: self.text("dialog_permission_body"),
            avatar_title: self.text("option_change_avatar"),
            avatar_choices: Client::avatar_choices(),
            avatar_directory: Client::avatar_directory().map(|path| path.display().to_string()),
            avatar_empty_hint: self.text("hint_avatar_directory_empty"),
            avatar_input_label: self.text("avatar_input_label"),
            avatar_pick_title: self.text("avatar_pick_button"),
            avatar_picker_supported: picker_supported(),
            profile_title: self.text("option_edit_profile"),
            profile_hint: self.text("form_profile_hint"),
            nickname_label: self.text("profile_nickname_label"),
            phone_label: self.text("profile_phone_label"),
            bio_label: self.text("profile_bio_label"),
            password_title: self.text("option_change_password"),
            old_label: self.text("password_old_label"),
            new_label: self.text("password_new_label"),
            repeat_label: self.text("password_repeat_label"),
            requests_title: self.text("option_pending_requests"),
            requests_empty: self.text("no_pending_requests"),
            received_title: self.text("request_section_received"),
            sent_title: self.text("request_section_sent"),
            accept_title: self.text("button_accept"),
            decline_title: self.text("button_decline"),
            cancel_title: self.text("button_cancel"),
            account_title: self.text("option_delete_account"),
            delete_hint: self.text("form_delete_hint"),
            server_title: self.text("option_server_address"),
            server_hint: self.text("hint_server_address"),
            update_title: self.text("option_update_client"),
            logout_title: self.text("option_logout"),
            confirm_title: self.text("button_confirm"),
            save_title: self.text("button_save"),
            current_language: config::preference_string("language", "zh-CN"),
            language_codes: config::Language::available_codes(),
            current_appearance: self.client.appearance_name.clone(),
            appearance_names: config::Palette::available_names(),
            request_rows: self.request_rows(),
            pending_count: self.client.pending_count(),
            sections: self.sections,
            fields: self.settings_fields(),
        }
    }

    /// Convert a private chat request entry into interface data (including "whether it can still be operated")
    pub(crate) fn request_rows(&self) -> Vec<RequestRow> {
        self.client
            .request_entries()
            .into_iter()
            .map(|(is_sent, request)| {
                let peer = if is_sent {
                    request.receiver
                } else {
                    request.sender
                };
                let status = request
                    .status
                    .as_ref()
                    .map(|status| self.client.request_status_label(status))
                    .unwrap_or_default();
                let actionable = match request.status.as_deref() {
                    None => !is_sent,
                    Some("pending") => true,
                    _ => false,
                };
                RequestRow {
                    id: request.id,
                    is_sent,
                    peer: peer
                        .map(|peer| peer.username)
                        .unwrap_or_else(|| self.text("unknown_user")),
                    status,
                    message: request.message,
                    actionable,
                    cancellable: is_sent && request.status.as_deref() == Some("pending"),
                }
            })
            .collect()
    }

    /// Write the panel's edited text back to the interface state, so a redraw can
    /// never lose what was typed.
    pub(crate) fn apply_fields(&mut self, fields: SettingsFields) {
        self.profile_nickname = fields.profile_nickname;
        self.profile_phone = fields.profile_phone;
        self.profile_bio = fields.profile_bio;
        self.password_old = fields.password_old;
        self.password_new = fields.password_new;
        self.password_repeat = fields.password_repeat;
        self.avatar_url = fields.avatar_url;
        self.delete_password = fields.delete_password;
        self.server_address = fields.server_address;
    }

    /// Apply the actions the settings panel hands back to the session layer
    pub(crate) fn apply_settings(&mut self, outcome: SettingsOutcome, context: &Context) {
        match outcome {
            SettingsOutcome::Nothing => {}
            SettingsOutcome::Language(code) => {
                self.client.switch_language(&code);
                self.refresh_skin(context);
            }
            SettingsOutcome::Appearance(name) => {
                self.client.switch_appearance(&name);
                self.refresh_skin(context);
            }
            #[cfg(any(target_os = "android", target_os = "ios"))]
            SettingsOutcome::OpenPermissionPrompt => self.notification_prompt_open = true,
            #[cfg(any(target_os = "android", target_os = "ios"))]
            SettingsOutcome::NotificationsConfirmed => {
                self.notification_prompt_open = false;
                self.client.sound_enabled = true;
                self.client.save_preferences();
                #[cfg(target_os = "android")]
                crate::android_platform::grant_permission();
                #[cfg(target_os = "ios")]
                crate::ios_platform::grant_permission();
            }
            #[cfg(any(target_os = "android", target_os = "ios"))]
            SettingsOutcome::NotificationsCancelled => self.notification_prompt_open = false,
            SettingsOutcome::Toggles {
                show_uid,
                time_with_date,
                quick_search,
                sound_enabled,
            } => {
                self.client.show_uid = show_uid;
                self.client.time_with_date = time_with_date;
                self.client.quick_search = quick_search;
                self.client.sound_enabled = sound_enabled;
                self.client.save_preferences();
            }
            SettingsOutcome::Sections(sections) => {
                let opened_profile = sections.profile && !self.sections.profile;
                self.sections = sections;
                if opened_profile {
                    self.client.prepare_profile_form();
                }
            }
            SettingsOutcome::AvatarFile(path) => {
                self.client.apply_local_avatar(&path);
                let own_id = self.client.current_user_id.clone().unwrap_or_default();
                self.avatars.forget(&own_id);
            }
            SettingsOutcome::OpenAvatarPicker => self.client.pick_avatar_file(),
            SettingsOutcome::AvatarUrl(url) => {
                self.client.apply_avatar_url(&url);
                let own_id = self.client.current_user_id.clone().unwrap_or_default();
                self.avatars.forget(&own_id);
            }
            SettingsOutcome::Profile {
                nickname,
                phone,
                bio,
            } => {
                self.profile_nickname = nickname.clone();
                self.profile_phone = phone.clone();
                self.profile_bio = bio.clone();
                self.client.update_profile(&nickname, &phone, &bio);
            }
            SettingsOutcome::Password { old, new, repeat } => {
                self.client.change_password(&old, &new, &repeat);
                self.password_old = String::new();
                self.password_new = String::new();
                self.password_repeat = String::new();
                if !self.client.is_signed_in() {
                    self.auth_page = Some(AuthPage::Login);
                    self.settings_open = false;
                }
            }
            SettingsOutcome::Accept(id) => self.client.accept_request(&id),
            SettingsOutcome::Decline(id) => self.client.decline_request(&id),
            SettingsOutcome::Cancel(id) => self.client.cancel_sent_request(&id),
            SettingsOutcome::DeleteAccount(password) => {
                self.client.delete_account(&password);
                self.delete_password = String::new();
                self.auth_page = Some(AuthPage::Login);
                self.settings_open = false;
            }
            SettingsOutcome::ServerAddress(address) => {
                self.server_address = address.clone();
                self.client.apply_server_address(&address);
            }
            SettingsOutcome::CheckUpdate => {
                let version = env!("CARGO_PKG_VERSION").to_string();
                self.client.start_update_check(&version);
            }
            SettingsOutcome::Logout => {
                self.client.sign_out();
                self.auth_page = Some(AuthPage::Login);
                self.settings_open = false;
            }
        }
    }

    // ==================== Login / Registration Page ====================

    pub(crate) fn profile_card_view(&self) -> Option<ProfileCardView> {
        let profile = self.client.profile_view.clone()?;
        let is_self = Some(&profile.id) == self.client.current_user_id.as_ref();
        let bytes = self
            .client
            .avatar_images
            .get(&profile.id)
            .and_then(|cached| cached.as_ref())
            .cloned();
        let (email, phone) = match (self.client.own_contact.clone(), is_self) {
            (Some(contact), true) => contact,
            _ => (String::new(), String::new()),
        };
        let none = self.text("profile_none");
        Some(ProfileCardView {
            username: profile.username.clone(),
            nickname: profile.nickname.filter(|value| !value.is_empty()),
            id: profile.id.clone(),
            email: if email.is_empty() {
                none.clone()
            } else {
                email
            },
            phone: if phone.is_empty() {
                none.clone()
            } else {
                phone
            },
            bio: profile
                .bio
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| none.clone()),
            avatar: match profile.avatar.as_deref() {
                None => none.clone(),
                Some(path) if path.starts_with('/') => self.text("profile_avatar_local"),
                Some(path) => path.to_string(),
            },
            presence: match self.client.presence_by_user.get(&profile.id) {
                Some(true) => self.text("status_online"),
                Some(false) => self.text("status_offline"),
                None => none,
            },
            texture_identity: profile.id,
            bytes,
            // Same order as the terminal version's profile card, so the same language keys describe the same rows.
            labels: vec![
                self.text("profile_nickname_label"),
                self.text("profile_uid"),
                self.text("presence_state"),
                self.text("profile_email_label"),
                self.text("profile_phone_label"),
                self.text("profile_bio_label"),
                self.text("profile_avatar_url"),
            ],
        })
    }

    pub(crate) fn draw_profile_card(&mut self, context: &Context) {
        if !self.profile_card_open {
            return;
        }
        let Some(view) = self.profile_card_view() else {
            return;
        };
        let skin = self.skin.clone();
        let texture = self.avatars.texture(
            context,
            &view.texture_identity,
            view.bytes.as_deref(),
            profile_avatar_side(),
        );
        let mut open = self.profile_card_open;
        Window::new(view.username.clone())
            .id(Id::new("profile-card"))
            .open(&mut open)
            .collapsible(true)
            .default_pos([120.0, 80.0])
            .frame(overlay_window_frame(&skin))
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    match texture {
                        Some(handle) => {
                            ui.add(
                                egui::Image::from_texture(&handle)
                                    .fit_to_exact_size(Vec2::splat(profile_avatar_side() as f32)),
                            );
                        }
                        None => {
                            draw_placeholder(ui, &view.username, profile_avatar_side() as f32);
                        }
                    }
                    ui.vertical(|ui| {
                        let labels = &view.labels;
                        if let Some(nickname) = view.nickname {
                            ui.colored_label(
                                skin.selected_text,
                                format!("{}: {nickname}", labels[0]),
                            );
                        }
                        ui.label(format!("{}: {}", labels[1], view.id));
                        ui.label(format!("{}: {}", labels[2], view.presence));
                        ui.label(format!("{}: {}", labels[3], view.email));
                        ui.label(format!("{}: {}", labels[4], view.phone));
                        ui.label(format!("{}: {}", labels[5], view.bio));
                        ui.label(format!("{}: {}", labels[6], view.avatar));
                    });
                });
            });
        self.profile_card_open = open;
    }

    // ==================== Create Group/Private Chat Window ====================
}

#[cfg(test)]
mod settings_section_tests {
    use super::{
        Sections, SettingsOutcome, SettingsView, draw_avatar_group, draw_switch, toggle_section,
    };
    use crate::app::icons::test_icons;
    use crate::app::test_support::{click_at, frame};
    use crate::appearance::Skin;
    use baihua_core::config::Palette;
    use egui::{CentralPanel, Context, Event, Pos2, RawInput, Vec2};

    fn raw_input(events: Vec<Event>) -> RawInput {
        frame(600.0, 400.0, events)
    }

    /// Draw a section title for one frame and take back (the title hit point, the result handed to the interface this frame)
    fn run_one_frame(
        context: &Context,
        sections: &mut Sections,
        events: Vec<Event>,
    ) -> (Pos2, SettingsOutcome) {
        let mut outcome = SettingsOutcome::Nothing;
        let mut painted: Vec<(String, Pos2)> = Vec::new();
        context
            .run_ui(raw_input(events), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    let skin = Skin::from(&Palette::built_in());
                    toggle_section(
                        ui,
                        &skin,
                        sections,
                        |sections| &mut sections.avatar,
                        "change avatar".to_string(),
                        &mut outcome,
                    );
                });
                painted = ctx.graphics_mut(|graphics| {
                    let mut texts: Vec<(String, Pos2)> = Vec::new();
                    if let Some(list) = graphics.get(egui::LayerId::background()) {
                        for entry in list.all_entries() {
                            if let egui::Shape::Text(shape) = &entry.shape {
                                texts.push((shape.galley.text().to_string(), shape.pos));
                            }
                        }
                    }
                    texts
                });
            })
            .drop_without_applying_deltas();
        let title_position = painted
            .iter()
            .find(|(text, _)| text == "change avatar")
            .map(|(_, position)| *position)
            .expect("this frame must paint the section title");
        (title_position, outcome)
    }

    /// Every settings section header must really toggle, with the full state written
    /// back, from the avatar section to the server address one.
    #[test]
    fn section_toggle() {
        let context = Context::default();
        let mut sections = Sections::default();
        let (title, _outcome) = run_one_frame(&context, &mut sections, Vec::new());
        let (_title, outcome) = run_one_frame(
            &context,
            &mut sections,
            click_at(title + Vec2::new(4.0, 8.0)),
        );
        let reported = match outcome {
            SettingsOutcome::Sections(reported) => reported,
            _ => panic!(
                "Clicking a section header should return the full open/close state, not some other result"
            ),
        };
        assert!(
            reported.avatar,
            "after opening the avatar section, the state handed back must show it expanded"
        );
        assert!(
            sections.avatar,
            "This frame's state should also become expanded"
        );
    }

    /// Draw the avatar section for one frame and return every painted text position.
    fn paint_avatar(
        context: &Context,
        skin: &Skin,
        view: &mut SettingsView,
        outcome: &mut SettingsOutcome,
        events: Vec<Event>,
    ) -> Vec<(String, Pos2)> {
        let mut painted: Vec<(String, Pos2)> = Vec::new();
        context
            .run_ui(raw_input(events), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    let icons = test_icons(ui.ctx());
                    draw_avatar_group(ui, skin, &icons, view, outcome);
                });
                painted = ctx.graphics_mut(|graphics| {
                    let mut texts: Vec<(String, Pos2)> = Vec::new();
                    if let Some(list) = graphics.get(egui::LayerId::background()) {
                        for entry in list.all_entries() {
                            if let egui::Shape::Text(shape) = &entry.shape {
                                texts.push((shape.galley.text().to_string(), shape.pos));
                            }
                        }
                    }
                    texts
                });
            })
            .drop_without_applying_deltas();
        painted
    }

    /// With a real file dialog on this target the picker button paints and its
    /// click hands back `OpenAvatarPicker`; without one the button never appears.
    #[test]
    fn picker_button_row() {
        use super::SettingsFields;
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        let mut view = SettingsView {
            sections: Sections {
                avatar: true,
                ..Sections::default()
            },
            avatar_pick_title: "choose file".to_string(),
            avatar_picker_supported: true,
            fields: SettingsFields::default(),
            ..SettingsView::default()
        };
        let mut outcome = SettingsOutcome::Nothing;
        let painted = paint_avatar(&context, &skin, &mut view, &mut outcome, Vec::new());
        let position = painted
            .iter()
            .find(|(text, _)| text == "choose file")
            .map(|(_, position)| *position)
            .expect("the picker button must paint when this target has a file dialog");
        paint_avatar(
            &context,
            &skin,
            &mut view,
            &mut outcome,
            click_at(position + Vec2::new(6.0, 8.0)),
        );
        assert!(
            matches!(outcome, SettingsOutcome::OpenAvatarPicker),
            "the picker click must reach the session layer as OpenAvatarPicker"
        );
        view.avatar_picker_supported = false;
        let painted = paint_avatar(&context, &skin, &mut view, &mut outcome, Vec::new());
        assert!(
            !painted.iter().any(|(text, _)| text == "choose file"),
            "a target without a file dialog must not show the picker button"
        );
    }

    /// An unselected, unhovered switch row must still show the theme border (the
    /// `frame_when_inactive` fix in `draw_switch`).
    #[test]
    fn switch_border() {
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        // The border comes from egui's visuals, so the theme must be applied first.
        skin.apply_to(&context);
        let mut output = context.run_ui(raw_input(Vec::new()), |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                let _ = draw_switch(ui, &skin, false, "change avatar".to_string());
            });
        });
        // Clear the texture delta before the output drops: nothing else consumes it.
        output.textures_delta.clear();
        let painted_borders: Vec<egui::Color32> = output
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Rect(rect) if rect.stroke.width > 0.0 => Some(rect.stroke.color),
                _ => None,
            })
            .collect();
        assert!(
            painted_borders.contains(&skin.room_border),
            "an unselected switch must paint the theme-colored border (got {painted_borders:?}, expected to include {:?})",
            skin.room_border
        );
    }
}
