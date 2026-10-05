//! Draft submission, sending, typing reports, and the local message cache.

use super::*;

impl Client {
    // ==================== Sending and Input State ====================

    /// Send the content in the input box: starting with # is search, starting with / is a command, everything else is a message
    pub fn submit_draft(&mut self) -> UiIntent {
        let draft = self.draft.clone();
        let trimmed = draft.trim().to_string();
        if trimmed.is_empty() {
            return UiIntent::Nothing;
        }
        if let Some(rest) = trimmed.strip_prefix('/') {
            let intent = self.execute_command(rest);
            self.draft.clear();
            return intent;
        }
        if let Some(keyword) = trimmed.strip_prefix('#') {
            self.run_search(keyword);
            return UiIntent::Nothing;
        }
        self.send_message(&trimmed);
        self.draft.clear();
        UiIntent::Nothing
    }

    /// Send a normal message; if the room is encrypted and the session isn't ready, first initiate a handshake then resend
    pub fn send_message(&mut self, content: &str) {
        let Some(room_id) = self.current_room_id() else {
            self.notify_error(self.text("logged_out_hint_graphical"));
            return;
        };
        if !self.is_signed_in() {
            self.notify_error(self.text("error_not_logged_in_graphical"));
            return;
        }
        if !self.room_is_encrypted(&room_id) {
            self.send_payload(outbound_ws_payload(
                self.connector.version(),
                WsCommand::SendMessage {
                    room_id: &room_id,
                    content,
                },
            ));
            self.draft.clear();
            return;
        }
        match self
            .crypto
            .sessions
            .get(&room_id)
            .map(|session| session.phase)
        {
            Some(EncryptionPhase::Active) => self.send_encrypted(&room_id, content),
            Some(_) => {
                if let Some(session) = self.crypto.sessions.get_mut(&room_id) {
                    session.pending_content = Some(content.to_string());
                }
            }
            None => {
                self.initiate_encryption(&room_id, Some(content.to_string()));
            }
        }
    }

    /// Encrypt with the session key and send
    pub(crate) fn send_encrypted(&mut self, room_id: &str, content: &str) {
        let key = match self
            .crypto
            .sessions
            .get(room_id)
            .and_then(|session| session.shared_key)
        {
            Some(key) => key,
            None => return,
        };
        match crypto::encrypt_message(&key, content) {
            Ok(ciphertext) => self.send_payload(outbound_ws_payload(
                self.connector.version(),
                WsCommand::EncryptMessage {
                    room_id,
                    ciphertext: &ciphertext,
                },
            )),
            Err(error) => self.notify_error(format!(
                "{}: {error}",
                self.text("error_encrypt_send_failed")
            )),
        }
    }

    /// Input status reporting: throttle at the interval given by the seam; don't report if the draft is empty
    pub fn report_typing(&mut self) {
        let Some(room_id) = self.current_room_id() else {
            return;
        };
        if self.draft.trim().is_empty() {
            return;
        }
        if self.last_typing_frame_sent_at.is_some_and(|sent_at| {
            sent_at.elapsed() < self.connector.version().typing_send_interval()
        }) {
            return;
        }
        self.last_typing_frame_sent_at = Some(Instant::now());
        self.send_payload(outbound_ws_payload(
            self.connector.version(),
            WsCommand::SendTyping { room_id: &room_id },
        ));
    }

    /// Names of members currently typing in the current room
    pub fn typing_names(&self) -> Vec<String> {
        let room_id = self.current_room_id();
        // Members with the same name (old records left after multi-account or reconnection) are displayed only once:
        // without deduplication the title would first show two identical names, and after one expires it would go back to one
        let mut names: Vec<String> = Vec::new();
        for (_room, name, _seen) in self
            .typing_members
            .iter()
            .filter(|(typing_room, _name, _seen)| Some(typing_room.as_str()) == room_id.as_deref())
        {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
    }

    // ==================== Message Cache ====================

    pub(crate) fn ensure_chat_cache(&mut self) {
        if self.chat_cache.is_some() {
            return;
        }
        if let Some(user_id) = self.current_user_id.clone() {
            self.chat_cache = ChatCache::open(&user_id);
        }
    }

    /// Messages arriving one by one only set a flag; write the whole room to disk once the batch window is full
    pub(crate) fn mark_messages_dirty(&mut self) {
        if self.cache_pending_flush_since.is_none() {
            self.cache_pending_flush_since = Some(Instant::now());
        }
    }

    pub(crate) fn flush_cache_if_due(&mut self) {
        let Some(since) = self.cache_pending_flush_since else {
            return;
        };
        if since.elapsed() < Duration::from_secs(5) {
            return;
        }
        if let Some(room_id) = self.current_room_id() {
            self.cache_messages(&room_id);
        }
        self.cache_pending_flush_since = None;
    }

    /// Write the current room's messages into cache (encrypted rooms are never persisted)
    pub fn cache_messages(&mut self, room_id: &str) {
        let Some(room_index) = self.rooms.iter().position(|room| room.id == room_id) else {
            return;
        };
        if self.rooms[room_index].is_encrypted || self.messages.is_empty() {
            return;
        }
        let loaded_room_matches = self
            .messages
            .last()
            .is_some_and(|message| message.room_id == room_id);
        if !loaded_room_matches {
            return;
        }
        self.ensure_chat_cache();
        if let Some(cache) = self.chat_cache.as_ref() {
            cache.store_room(
                room_id,
                &self.messages,
                self.older_cursor.as_deref(),
                self.has_more_older,
            );
        }
    }
}
