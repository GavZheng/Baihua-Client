//! Slash-command execution and the self-update flows it triggers.

use super::*;

impl Client {
    // ==================== Commands ====================

    /// Execute a command (without leading slash), return what the interface needs to do
    pub fn execute_command(&mut self, line: &str) -> UiIntent {
        let mut parts = line.splitn(2, ' ');
        let name = parts.next().unwrap_or_default().trim();
        let argument = parts.next().unwrap_or_default().trim().to_string();
        if !self.is_signed_in() && !allowed_signed_out(name) {
            self.notify_error(self.text("error_not_logged_in_graphical"));
            return UiIntent::Nothing;
        }
        match name {
            "" => UiIntent::Nothing,
            "quit" | "exit" => {
                self.quit_requested = true;
                UiIntent::Quit
            }
            "info" => {
                let lines = self.room_information();
                for line in lines {
                    self.notify(line);
                }
                UiIntent::Nothing
            }
            "profile" => {
                self.show_profile_of(&argument);
                UiIntent::ShowOwnProfile
            }
            "list_users" => {
                self.ensure_users_loaded();
                let rows = self.registered_user_rows();
                if rows.is_empty() {
                    self.notify(self.text("users_empty"));
                }
                for row in rows {
                    self.notify(row);
                }
                UiIntent::Nothing
            }
            "search_users" => {
                match self.connector.search_users(&argument) {
                    Ok(users) => {
                        for user in users {
                            self.notify(format!("{} - {}", user.username, user.id));
                        }
                    }
                    Err(error) => self.notify_error(format!(
                        "{}: {error}",
                        self.text("error_user_search_failed")
                    )),
                }
                UiIntent::Nothing
            }
            "kick" => {
                self.execute_kick(&argument);
                UiIntent::Nothing
            }
            "leave" | "quit_group" => {
                self.leave_current_room();
                UiIntent::Nothing
            }
            "mute" => {
                self.apply_mute_command(&argument);
                UiIntent::Nothing
            }
            "add_member" => {
                self.add_member(&argument);
                UiIntent::Nothing
            }
            "logout" => {
                self.sign_out();
                UiIntent::OpenSignIn(None)
            }
            // Flow control: the interface opens the sign-in form itself, since no
            // password may travel through a command line.
            "register" => UiIntent::OpenSignUp(some_username(argument)),
            "update" => {
                // The feed answer arrives as an event; the prompt it opens does the rest
                self.start_update_check(env!("CARGO_PKG_VERSION"));
                UiIntent::Nothing
            }
            "language" => {
                if !argument.is_empty() {
                    // Same as terminal version: the parameter must be a language code that actually exists under config/languages,
                    // and matched with case-sensitive exact matching (zh-cn is not zh-CN), no guessing, no case folding.
                    if !config::Language::available_codes()
                        .iter()
                        .any(|code| code == &argument)
                    {
                        self.notify_error(self.text("error_no_languages"));
                        return UiIntent::Nothing;
                    }
                    self.switch_language(&argument);
                    self.save_preferences();
                    self.notify(self.text("language_switched").replace("{lang}", &argument));
                    return UiIntent::Nothing;
                }
                // without arguments: languages are listed one by one, taking the person to the settings panel to choose
                UiIntent::OpenSettings
            }
            "appearance" => {
                if !argument.is_empty() {
                    // Same as terminal version: the parameter must be an appearance name that actually exists under config/themes,
                    // and matched with case-sensitive exact matching, to avoid silently applying an incorrect name as a built-in default color.
                    if !config::Palette::available_names()
                        .iter()
                        .any(|name| name == &argument)
                    {
                        self.notify_error(
                            self.text("error_appearance_not_found")
                                .replace("{name}", &argument),
                        );
                        return UiIntent::Nothing;
                    }
                    self.switch_appearance(&argument);
                    self.notify(
                        self.text("appearance_switched")
                            .replace("{name}", &argument),
                    );
                    return UiIntent::Nothing;
                }
                UiIntent::OpenSettings
            }
            "server_address" => {
                if !argument.is_empty() {
                    self.apply_server_address(&argument);
                    return UiIntent::Nothing;
                }
                UiIntent::OpenSettings
            }
            other => {
                self.notify_error(self.text("error_unknown_command").replace("{name}", other));
                UiIntent::Nothing
            }
        }
    }

    // ==================== Update ====================

    /// Ask the release feed for a newer package: the answer becomes a prompt, and only
    /// a yes starts the download.
    pub fn start_update_check(&mut self, current_version: &str) {
        if self
            .update_check_running
            .as_ref()
            .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
        {
            return;
        }
        let Some(sender) = self.events.clone() else {
            return;
        };
        let flag = Arc::new(AtomicBool::new(true));
        self.update_check_running = Some(flag);
        let checked_version = current_version.to_string();
        let current_version = checked_version.clone();
        std::thread::spawn(move || {
            let event = match check_for_update(&current_version, ReleaseChannel::Graphical) {
                UpdateCheck::Available(package) => Some(PollingEvent::UpdateAvailable(package)),
                UpdateCheck::UpToDate { .. } => Some(PollingEvent::UpdateUpToDate(checked_version)),
                // A missing package or feed is technical and untranslated, so it only
                // reaches the debug log instead of popping raw library text in front.
                UpdateCheck::Unavailable(reason) => {
                    config::debug_log(&format!("update check failed: {reason}"));
                    None
                }
            };
            if let Some(event) = event {
                sender.send(event);
            }
        });
    }

    /// Download the package the prompt offered. A blocking fetch would freeze the frame
    /// loop for the whole transfer, so it runs on a background thread and reports back.
    pub fn start_download(&mut self) {
        let Some((package, UpdateStage::AwaitingAnswer)) = self.pending_update.clone() else {
            return;
        };
        let Some(sender) = self.events.clone() else {
            return;
        };
        self.pending_update = Some((package.clone(), UpdateStage::Downloading));
        self.notify(
            self.text("update_downloading")
                .replace("{version}", &package.version),
        );
        let download_failed_wording = self.text("error_update_download_failed");
        std::thread::spawn(move || {
            let event = match download_package(&package) {
                Ok(package_path) => {
                    PollingEvent::UpdateReady((package.version.clone(), package_path))
                }
                Err(error) => PollingEvent::Error(format!("{download_failed_wording}: {error}")),
            };
            sender.send(event);
        });
    }

    /// Hand the downloaded package to a detached installer that waits for this process,
    /// then close: nothing is replaced while this process still holds the file open.
    pub fn start_update_install(&mut self) -> bool {
        let Some((package, UpdateStage::Package(package_path))) = self.pending_update.clone()
        else {
            self.notify(self.text("update_not_ready"));
            return false;
        };
        let Some(prefix) = baihua_core::installer::current_prefix()
            .or_else(baihua_core::installer::default_prefix)
        else {
            self.notify_error(self.text("error_update_prefix_unavailable"));
            return false;
        };
        let handoff = baihua_core::installer::PendingInstall {
            package_path: package_path.to_string_lossy().to_string(),
            version: package.version.clone(),
            prefix: prefix.to_string_lossy().to_string(),
            wait_for_process: Some(std::process::id()),
        };
        match baihua_core::installer::spawn_detached_installer(&handoff) {
            Ok(()) => {
                self.pending_update = None;
                self.quit_requested = true;
                true
            }
            Err(error) => {
                self.notify_error(format!(
                    "{}: {error}",
                    self.text("error_update_start_failed")
                ));
                false
            }
        }
    }
}
