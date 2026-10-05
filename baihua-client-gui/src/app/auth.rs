//! The centered login/registration page shown while nobody is signed in.

use super::*;

/// Which page the login/registration page is currently on
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AuthPage {
    Login,
    Register,
}

/// Text needed for the login/registration page
pub(crate) struct AuthView {
    pub(crate) page: AuthPage,
    pub(crate) login_title: String,
    pub(crate) register_title: String,
    pub(crate) username_label: String,
    pub(crate) email_label: String,
    pub(crate) password_label: String,
    pub(crate) server_label: String,
    pub(crate) server_hint: String,
    pub(crate) confirm_title: String,
    pub(crate) server_address: String,
}

pub(crate) enum AuthOutcome {
    Nothing,
    SwitchTo(AuthPage),
    SignIn,
    SignUp,
    ServerAddress(String),
}

/// Drafts for each input box on the login/registration page (the interface closure doesn't touch self; drafts are passed in and out in full before and after drawing)
pub(crate) struct AuthDrafts {
    pub(crate) login_name: String,
    pub(crate) login_password: String,
    pub(crate) register_name: String,
    pub(crate) register_email: String,
    pub(crate) register_password: String,
    pub(crate) server_address: String,
}

pub(crate) fn draw_auth_form(
    ui: &mut Ui,
    skin: &Skin,
    icons: &IconFrame,
    view: &AuthView,
    drafts: &mut AuthDrafts,
) -> AuthOutcome {
    // Borrow by field once, and below when drawing each input box you can write the name directly
    let AuthDrafts {
        login_name,
        login_password,
        register_name,
        register_email,
        register_password,
        server_address,
    } = drafts;
    let mut outcome = AuthOutcome::Nothing;
    ui.horizontal(|ui| {
        if icon_switch(
            ui,
            skin,
            icons,
            IconName::Proceed,
            view.page == AuthPage::Login,
            view.login_title.clone(),
        )
        .clicked()
        {
            outcome = AuthOutcome::SwitchTo(AuthPage::Login);
        }
        if icon_switch(
            ui,
            skin,
            icons,
            IconName::Register,
            view.page == AuthPage::Register,
            view.register_title.clone(),
        )
        .clicked()
        {
            outcome = AuthOutcome::SwitchTo(AuthPage::Register);
        }
    });
    ui.add_space(8.0);
    match view.page {
        AuthPage::Login => {
            ui.label(view.username_label.clone());
            ui.add(TextEdit::singleline(login_name).desired_width(f32::INFINITY));
            ui.label(view.password_label.clone());
            ui.add(
                TextEdit::singleline(login_password)
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
            if icon_button(
                ui,
                skin,
                icons,
                IconName::Proceed,
                Some(view.login_title.clone()),
            )
            .clicked()
            {
                outcome = AuthOutcome::SignIn;
            }
        }
        AuthPage::Register => {
            ui.label(view.username_label.clone());
            ui.add(TextEdit::singleline(register_name).desired_width(f32::INFINITY));
            ui.label(view.email_label.clone());
            ui.add(TextEdit::singleline(register_email).desired_width(f32::INFINITY));
            ui.label(view.password_label.clone());
            ui.add(
                TextEdit::singleline(register_password)
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
            if icon_button(
                ui,
                skin,
                icons,
                IconName::Register,
                Some(view.register_title.clone()),
            )
            .clicked()
            {
                outcome = AuthOutcome::SignUp;
            }
        }
    }
    ui.separator();
    ui.label(view.server_label.clone());
    // The address field sits beside the confirm button on a desktop-wide form; on
    // a phone the two stack so nothing can stretch the modal past the screen.
    if auth_row_inline(ui.max_rect().width()) {
        let field_width = ui.max_rect().width() * 0.7;
        ui.horizontal(|ui| {
            ui.add_sized(
                [field_width, ui.spacing().interact_size.y],
                TextEdit::singleline(server_address),
            );
            if icon_button(
                ui,
                skin,
                icons,
                IconName::Confirm,
                Some(view.confirm_title.clone()),
            )
            .clicked()
            {
                outcome = AuthOutcome::ServerAddress(server_address.clone());
            }
        });
    } else {
        ui.add(TextEdit::singleline(server_address).desired_width(f32::INFINITY));
        if icon_button(
            ui,
            skin,
            icons,
            IconName::Confirm,
            Some(view.confirm_title.clone()),
        )
        .clicked()
        {
            outcome = AuthOutcome::ServerAddress(server_address.clone());
        }
    }
    ui.label(view.server_hint.clone());
    outcome
}

impl BaihuaApp {
    pub(crate) fn auth_view(&self, page: AuthPage) -> AuthView {
        AuthView {
            page,
            login_title: self.text("option_login"),
            register_title: self.text("option_register"),
            username_label: self.text("label_username"),
            email_label: self.text("label_email"),
            password_label: self.text("label_password"),
            server_label: self.text("option_server_address"),
            server_hint: self.text("hint_server_address"),
            confirm_title: self.text("button_confirm"),
            server_address: if self.server_address.is_empty() {
                self.client.connector.base_url().to_string()
            } else {
                self.server_address.clone()
            },
        }
    }

    /// Draw the centered sign-in modal and return its frame rectangle (None when no
    /// page shows); width and capped height follow the screen, so phones fit it.
    pub(crate) fn draw_auth_page(&mut self, context: &Context) -> Option<Rect> {
        let page = self.auth_page?;
        if self.client.is_signed_in() {
            self.auth_page = None;
            return None;
        }
        let skin = self.skin.clone();
        let icons = self.frame_icons(context);
        let view = self.auth_view(page);
        let mut drafts = AuthDrafts {
            login_name: self.login_name.clone(),
            login_password: self.login_password.clone(),
            register_name: self.register_name.clone(),
            register_email: self.register_email.clone(),
            register_password: self.register_password.clone(),
            server_address: view.server_address.clone(),
        };
        let mut outcome = AuthOutcome::Nothing;
        // `content_rect` (not the raw viewport) keeps the form out of notches and
        // rounded corners on platforms that report safe-area insets.
        let screen = context.content_rect().size();
        let response = Modal::new(Id::new("auth-page"))
            .backdrop_color(Color32::from_black_alpha(140))
            .frame(
                Frame::new()
                    .fill(skin.app_background)
                    .stroke(Stroke::new(1.0, skin.overlay_border))
                    .corner_radius(8.0)
                    .inner_margin(Margin::same(16)),
            )
            .show(context, |ui| {
                let form_width = auth_form_width(screen.x);
                ui.set_width(form_width);
                ui.set_max_height(auth_form_max_height(screen.y));
                ScrollArea::vertical().show(ui, |ui| {
                    // Pin the scroll content too: without this the infinite-width
                    // text edits stretch the area and the modal overflows the phone.
                    ui.set_width(form_width);
                    outcome = draw_auth_form(ui, &skin, &icons, &view, &mut drafts);
                });
            });
        self.store_auth_drafts(drafts);
        let frame_rect = response.response.rect;
        match outcome {
            AuthOutcome::Nothing => {}
            AuthOutcome::SwitchTo(page) => self.auth_page = Some(page),
            AuthOutcome::SignIn => {
                let (name, password) = (self.login_name.clone(), self.login_password.clone());
                if self.client.sign_in(&name, &password) {
                    self.login_password = String::new();
                    self.auth_page = None;
                    self.focus_message_input = true;
                }
            }
            AuthOutcome::SignUp => {
                let (name, email, password) = (
                    self.register_name.clone(),
                    self.register_email.clone(),
                    self.register_password.clone(),
                );
                if self.client.sign_up(&name, &email, &password) {
                    self.register_password = String::new();
                    self.login_name = name;
                    self.auth_page = Some(AuthPage::Login);
                }
            }
            AuthOutcome::ServerAddress(address) => self.client.apply_server_address(&address),
        }
        Some(frame_rect)
    }

    /// Write this frame's auth drafts back to the app fields in full: a missed
    /// field reverts to its old value next frame (its box "cannot type").
    pub(crate) fn store_auth_drafts(&mut self, drafts: AuthDrafts) {
        self.login_name = drafts.login_name;
        self.login_password = drafts.login_password;
        self.register_name = drafts.register_name;
        self.register_email = drafts.register_email;
        self.register_password = drafts.register_password;
        self.server_address = drafts.server_address;
    }

    // ==================== Notifications and Profile Card ====================
}

#[cfg(test)]
pub(crate) mod auth_draft_tests {
    use super::{AuthDrafts, AuthPage, BaihuaApp, Context, Sections};
    use crate::appearance::{AvatarTextures, Skin};
    use crate::client::Client;
    use baihua_core::config::Palette;

    /// Offline test shell: `Client::default()` (no network, no threads), shared
    /// by the draft write-back and status bar tests.
    pub(crate) fn test_app() -> BaihuaApp {
        BaihuaApp {
            client: Client::default(),
            skin: Skin::from(&Palette::built_in()),
            avatars: AvatarTextures::default(),
            settings_open: false,
            group_settings_open: false,
            group_settings_member_name: String::new(),
            search_panel_open: false,
            search_panel_keyword: String::new(),
            auth_page: Some(AuthPage::Register),
            profile_card_open: false,
            completion_selection: 0,
            creation_page: None,
            sections: Sections::default(),
            #[cfg(target_os = "android")]
            android_application: None,
            #[cfg(any(target_os = "android", target_os = "ios"))]
            notification_prompt_open: false,
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
            focus_message_input: false,
            focus_search_panel_input: false,
            message_box_focused_last_frame: false,
        }
    }

    /// The sign-in modal must fit inside a phone screen (the reported "window
    /// longer than the screen"); the rectangle is what `draw_auth_page` reports.
    #[test]
    fn auth_fits_phone() {
        let context = Context::default();
        let mut app = test_app();
        let mut frame = Option::None;
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::new(390.0, 844.0),
            )),
            focused: true,
            ..Default::default()
        };
        context
            .run_ui(raw_input, |ctx| {
                frame = app.draw_auth_page(ctx);
            })
            .drop_without_applying_deltas();
        let frame = frame.expect("the registration page was open, so the modal must draw");
        assert!(
            frame.width() <= 390.0 && frame.height() <= 844.0,
            "the auth modal measured {:?} on a 390x844 screen",
            frame.size()
        );
    }

    /// Every draft field must be written back: a missed one made the registration
    /// password box drop each typed character on the next frame.
    #[test]
    fn drafts_write_back() {
        let mut app = test_app();
        app.store_auth_drafts(AuthDrafts {
            login_name: "login name".to_string(),
            login_password: "login password".to_string(),
            register_name: "register name".to_string(),
            register_email: "mail@example.com".to_string(),
            register_password: "register password".to_string(),
            server_address: "http://localhost:2424".to_string(),
        });
        assert_eq!(app.login_name, "login name");
        assert_eq!(app.login_password, "login password");
        assert_eq!(app.register_name, "register name");
        assert_eq!(app.register_email, "mail@example.com");
        assert_eq!(
            app.register_password, "register password",
            "the register password must land back in the interface field or the box can never be typed into"
        );
        assert_eq!(app.server_address, "http://localhost:2424");
    }
}
