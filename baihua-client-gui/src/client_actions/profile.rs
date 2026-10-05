//! Profile, password, avatar, and user-directory actions.

use super::*;

impl Client {
    /// When opening the profile form, fetch your own profile: the form shows the current nickname and bio from the server.
    /// phone numbers have no read-only API to fetch back (the server only gives them in login/registration/change-profile responses), so keep empty
    pub fn prepare_profile_form(&mut self) {
        let Some(user_id) = self.current_user_id.clone() else {
            return;
        };
        if let Ok(profile) = self.connector.get_user_profile(&user_id) {
            self.profile_nickname_draft = profile.nickname.unwrap_or_default();
            self.profile_bio_draft = profile.bio.unwrap_or_default();
        }
    }

    /// Save profile: only write non-empty items into the request body; the server's three states are expressed by ProfileUpdatePayload
    pub fn update_profile(&mut self, nickname: &str, phone: &str, bio: &str) {
        let payload = ProfileUpdatePayload {
            nickname: profile_field_value(nickname),
            phone_number: profile_field_value(phone),
            bio: profile_field_value(bio),
            avatar: None,
        };
        if payload.nickname.is_none() && payload.phone_number.is_none() && payload.bio.is_none() {
            self.notify_error(self.text("profile_nothing_to_update"));
            return;
        }
        match self.connector.update_profile(&payload) {
            Ok(user) => {
                self.remember_own_profile(&user);
                // The saved contact pair just changed, so keep the on-disk session
                // in sync: a process kill must not roll it back.
                self.persist_session();
                self.notify(self.text("profile_saved"));
            }
            Err(error) => self.notify_error(format!(
                "{}: {error}",
                self.text("error_profile_update_failed")
            )),
        }
    }

    /// Change password: the server bumps token_version, and the local session must be voided on success
    pub fn change_password(&mut self, old_password: &str, new_password: &str, repeated: &str) {
        if new_password != repeated {
            self.notify_error(self.text("error_password_mismatch"));
            return;
        }
        match self.connector.change_password(
            &crypto::encrypt_login_password(old_password),
            &crypto::encrypt_login_password(new_password),
        ) {
            Ok(_result) => {
                self.sign_out();
                self.notify_error(self.text("notification_password_changed"));
            }
            Err(error) => self.notify_error(format!(
                "{}: {error}",
                self.text("error_password_change_failed")
            )),
        }
    }

    /// Delete account: clear all local residue on success
    pub fn delete_account(&mut self, password: &str) {
        match self
            .connector
            .delete_account(&crypto::encrypt_login_password(password))
        {
            Ok(_result) => {
                self.sign_out();
                if let Some(cache) = self.chat_cache.as_ref() {
                    cache.clear_all();
                }
                self.chat_cache = None;
                self.avatar_images.clear();
                self.notify(self.text("account_deleted"));
            }
            Err(error) => self.notify_error(format!(
                "{}: {error}",
                self.text("error_account_delete_failed")
            )),
        }
    }

    /// Directory for user-supplied avatars (same location as terminal version)
    pub fn avatar_directory() -> Option<PathBuf> {
        paths::avatar_source_directory()
    }

    /// Image filenames available in the avatar directory, sorted by name
    pub fn avatar_choices() -> Vec<(String, PathBuf)> {
        self::avatar_files()
    }

    /// Change avatar using a local image file
    pub fn apply_local_avatar(&mut self, path: &Path) {
        let Some((file_name, content_type, bytes)) = read_local_image(path) else {
            self.notify_error(self.text("error_avatar_local_file_unusable"));
            return;
        };
        match self
            .connector
            .upload_avatar(&file_name, &content_type, bytes)
        {
            Ok(user) => self.finish_avatar_change(&user),
            Err(error) => self.notify_error(format!(
                "{}: {error}",
                self.text("error_avatar_update_failed")
            )),
        }
    }

    /// Open the operating system's file picker on a worker thread; the answer
    /// lands as `PollingEvent::AvatarFileChosen`, so no frame ever waits on a dialog.
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    pub fn pick_avatar_file(&mut self) {
        let Some(sender) = self.events.clone() else {
            return;
        };
        std::thread::spawn(move || {
            let choice = rfd::FileDialog::new()
                .add_filter("image", &["png", "jpg", "jpeg", "gif", "webp"])
                .pick_file();
            sender.send(PollingEvent::AvatarFileChosen(choice));
        });
    }

    /// Phones have no desktop file dialog: the button is hidden there, and this
    /// stub only exists so the shared call site compiles on every target.
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    pub fn pick_avatar_file(&mut self) {}

    /// Change avatar using a network URL
    pub fn apply_avatar_url(&mut self, url: &str) {
        let trimmed = url.trim();
        if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            self.notify_error(self.text("error_avatar_url_invalid"));
            return;
        }
        let payload = ProfileUpdatePayload {
            nickname: None,
            phone_number: None,
            bio: None,
            avatar: Some(Some(trimmed.to_string())),
        };
        match self.connector.update_profile(&payload) {
            Ok(user) => self.finish_avatar_change(&user),
            Err(error) => self.notify_error(format!(
                "{}: {error}",
                self.text("error_avatar_update_failed")
            )),
        }
    }

    pub(crate) fn finish_avatar_change(&mut self, user: &UserInfo) {
        config::drop_cached_avatar(&user.id);
        self.remember_own_profile(user);
        self.notify(self.text("avatar_updated"));
    }

    // ==================== Profile Card and User Directory ====================

    /// View someone's profile; leave empty to view your own
    pub fn show_profile_of(&mut self, key: &str) {
        let lookup = if key.trim().is_empty() {
            self.current_user_id.clone()
        } else {
            Some(key.trim().to_string())
        };
        let Some(lookup) = lookup else {
            self.notify_error(self.text("error_no_user_reference"));
            return;
        };
        match self.connector.get_user_profile(&lookup) {
            Ok(profile) => {
                self.request_avatars(std::slice::from_ref(&profile.id));
                self.profile_view = Some(profile);
            }
            Err(error) => self.notify_error(format!(
                "{}: {error}",
                self.text("error_profile_fetch_failed")
            )),
        }
    }

    /// Fetch the registered user directory (send one request only when needed)
    pub fn ensure_users_loaded(&mut self) {
        if self.registered_users.is_some() {
            return;
        }
        let Some(sender) = self.events.clone() else {
            return;
        };
        let connector = self.connector.clone();
        std::thread::spawn(move || match connector.list_all_users() {
            Ok(users) => {
                sender.send(PollingEvent::RegisteredUsersUpdated(users));
            }
            Err(error) => config::debug_log(&format!(
                "Fetching the registered user directory failed: {error}"
            )),
        });
    }

    /// All registered users (for auto-completion and /list_users)
    pub fn registered_user_rows(&self) -> Vec<String> {
        self.registered_users
            .as_ref()
            .map(|users| {
                users
                    .iter()
                    .map(|user| format!("{} - {}", user.username, user.id))
                    .collect()
            })
            .unwrap_or_default()
    }

    // ==================== Avatar Fetch Bytes ====================

    /// Fetch avatar bytes for several users: read from disk first, only dispatch a background thread on miss; never connect to the network during rendering
    pub fn request_avatars(&mut self, user_ids: &[String]) {
        let Some(sender) = self.events.clone() else {
            return;
        };
        let mut missing: Vec<String> = Vec::new();
        for user_id in user_ids {
            if user_id.is_empty() || self.avatar_images.contains_key(user_id) {
                continue;
            }
            if let Some(bytes) = config::load_cached_avatar(user_id) {
                self.avatar_images.insert(user_id.clone(), Some(bytes));
                continue;
            }
            self.avatar_images.insert(user_id.clone(), None);
            missing.push(user_id.clone());
        }
        if missing.is_empty() {
            return;
        }
        // Fetching avatars is one network request per user; serial waiting would be very slow; split into segments by machine parallelism and fetch simultaneously,
        // one thread per segment, fetch one and hand it back to the main thread immediately via the event channel (the render thread only receives events, never connects to the network)
        let chunk_size = missing.len().div_ceil(fetch_worker_count()).max(1);
        for chunk in missing.chunks(chunk_size) {
            let connector = self.connector.clone();
            let sender_for_thread = sender.clone();
            let chunk: Vec<String> = chunk.to_vec();
            std::thread::spawn(move || {
                for user_id in chunk {
                    let bytes = connector
                        .get_user_profile(&user_id)
                        .ok()
                        .and_then(|profile| profile.avatar)
                        .and_then(|path| connector.fetch_static_resource(&path).ok());
                    if let Some(bytes) = &bytes {
                        config::store_cached_avatar(&user_id, bytes);
                    }
                    sender_for_thread.send(PollingEvent::AvatarLoaded((user_id, bytes)));
                }
            });
        }
    }

    /// People who have appeared in the current room (avatars need bytes fetched for them)
    pub(crate) fn refresh_room_avatars(&mut self) {
        let ids: Vec<String> = self
            .messages
            .iter()
            .map(|message| message.sender_id.clone())
            .filter(|id| !id.is_empty())
            .collect();
        self.request_avatars(&ids);
    }

    /// Username mapping completion: pull room members once, avoid looking up people per message
    pub(crate) fn refresh_sender_names(&mut self) {
        let Some(room_id) = self.current_room_id() else {
            return;
        };
        if let Ok(detail) = self.connector.get_room(&room_id) {
            for member in detail.members {
                self.sender_names
                    .insert(member.user_id, member.nickname.unwrap_or(member.username));
            }
        }
        let ids: Vec<String> = self
            .messages
            .iter()
            .map(|message| message.sender_id.clone())
            .collect();
        self.request_avatars(&ids);
    }

    /// Sender display name: look up table → fall back to ID → fall back to "unknown user" for empty ID
    pub fn sender_display_name(&self, sender_id: &str) -> String {
        if sender_id.is_empty() {
            return self.text("unknown_user");
        }
        self.sender_names
            .get(sender_id)
            .cloned()
            .unwrap_or_else(|| sender_id.to_string())
    }
}
