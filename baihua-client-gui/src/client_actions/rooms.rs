//! Room list state: opening, selecting, paging history, hiding.

use super::*;

impl Client {
    /// Currently selected room ID
    pub fn current_room_id(&self) -> Option<String> {
        let index = self.selected_room_index?;
        self.rooms.get(index).map(|room| room.id.clone())
    }

    /// Select a room (called when clicking a room entry in the interface)
    pub fn open_room(&mut self, index: usize) {
        if index >= self.rooms.len() {
            return;
        }
        // Reopening a room you're already viewing also clears unread: the people are here, the red dot shouldn't stay
        let opened_id = self.rooms[index].id.clone();
        self.unread_counts.remove(&opened_id);
        if self.selected_room_index == Some(index) {
            return;
        }
        let previous_room = self
            .selected_room_index
            .and_then(|index| self.rooms.get(index).map(|room| room.id.clone()));
        if let Some(previous_room) = previous_room {
            self.cache_messages(&previous_room);
        }
        self.selected_room_index = Some(index);
        self.unread_counts.remove(&self.rooms[index].id);
        self.search_result = None;
        // The panel's stored matches name messages of the room just left: dropping them with the
        // room keeps a stale block from being clicked into a message that is not on screen.
        self.panel_search_result = None;
        // The cached member table belongs to the room that was just left: the group settings sidebar
        // must not keep showing it (it refetches when reopened on the new room).
        self.forget_room_detail();
        let room_id = self.rooms[index].id.clone();
        self.load_room_messages(&room_id);
    }

    /// A click on a room-list row: open that room, or close the selection when the
    /// row is already the open one (the narrow layout's way back to the list).
    pub fn toggle_room(&mut self, index: usize) {
        if self.selected_room_index == Some(index) {
            self.close_room_selection();
            return;
        }
        self.open_room(index);
    }

    /// Drop the room selection (back to "no room open"): cache the messages first,
    /// forget the cached roster too, clear search state.
    pub fn close_room_selection(&mut self) {
        if let Some(room_id) = self.current_room_id() {
            self.cache_messages(&room_id);
        }
        self.selected_room_index = None;
        self.messages.clear();
        self.older_cursor = None;
        self.has_more_older = false;
        self.pending_scroll_message_id = None;
        self.search_result = None;
        self.panel_search_result = None;
        self.forget_room_detail();
    }

    /// Load room messages: first fill from local cache, then fetch the first page from the server and merge by ID
    pub(crate) fn load_room_messages(&mut self, room_id: &str) {
        // What this side already holds: in an encrypted chat the decrypted plaintext,
        // where the server returns an empty body, so a reload merges by message id.
        let same_room = self
            .messages
            .first()
            .is_some_and(|message| message.room_id == room_id);
        if !same_room {
            self.messages.clear();
            self.older_cursor = None;
            self.has_more_older = false;
            let encrypted = self.room_is_encrypted(room_id);
            if !encrypted
                && let Some(cache) = self.chat_cache.as_ref()
                && let Some(cached) = cache.load_room(room_id)
            {
                self.messages = cached.messages;
                self.older_cursor = cached.older_cursor;
                self.has_more_older = cached.has_more;
            }
        }
        match self.connector.get_messages(room_id, 50, None) {
            Ok(page) => {
                self.merge_messages(page.messages);
                self.older_cursor = page.next_cursor.clone();
                self.has_more_older = page.has_more;
            }
            Err(error) if error.is_connection_failure() => {
                config::debug_log(&format!(
                    "Server unreachable while pulling messages, keeping cached content: {error}"
                ));
            }
            Err(error) => {
                self.notify_error(format!("{}: {error}", self.text("error_load_history")))
            }
        }
        self.messages_reloaded_at = Instant::now();
        self.refresh_sender_names();
    }

    /// Page to earlier messages. An `automatic` (touch-top) failure stops retrying
    /// so the render path cannot re-request every frame; a manual retry keeps going.
    pub fn load_older_messages(&mut self, automatic: bool) {
        let (Some(room_id), Some(cursor)) = (self.current_room_id(), self.older_cursor.clone())
        else {
            return;
        };
        match self
            .connector
            .get_messages(&room_id, 50, Some(cursor.as_str()))
        {
            Ok(page) => {
                self.merge_messages(page.messages);
                self.older_cursor = page.next_cursor.clone();
                self.has_more_older = page.has_more;
                self.mark_messages_dirty();
            }
            Err(error) if error.is_connection_failure() => {
                if automatic {
                    self.older_cursor = None;
                    self.has_more_older = false;
                }
            }
            Err(error) => {
                self.notify_error(format!("{}: {error}", self.text("error_load_history")));
                if automatic {
                    self.older_cursor = None;
                    self.has_more_older = false;
                }
            }
        }
    }

    /// Merge messages: deduplicate by ID, sort by time and ID ascending, only add never subtract
    pub(crate) fn merge_messages(&mut self, incoming: Vec<MessageInfo>) {
        let known: std::collections::HashSet<String> = self
            .messages
            .iter()
            .map(|message| message.id.clone())
            .collect();
        for message in incoming {
            if !known.contains(&message.id) {
                self.sender_names
                    .insert(message.sender_id.clone(), message.sender_id.clone());
                self.messages.push(message);
            }
        }
        self.messages.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
    }

    /// Server receipt or locally sent messages fall into the view
    pub(crate) fn absorb_message(&mut self, message: MessageInfo) {
        if Some(message.room_id.clone()) != self.current_room_id() {
            return;
        }
        if self.messages.iter().any(|stored| stored.id == message.id) {
            return;
        }
        self.messages.push(message);
        self.mark_messages_dirty();
    }

    /// Whether the room is encrypted
    pub fn room_is_encrypted(&self, room_id: &str) -> bool {
        self.rooms
            .iter()
            .find(|room| room.id == room_id)
            .is_some_and(|room| room.is_encrypted)
    }

    /// Whether this room is a group chat (only group chats can add members or view the member list)
    pub fn room_is_group(&self, room_id: &str) -> bool {
        self.rooms
            .iter()
            .find(|room| room.id == room_id)
            .is_some_and(|room| room.is_group)
    }

    pub(crate) fn room_was_encrypted(&self, room_id: &str) -> bool {
        self.room_is_encrypted(room_id)
    }

    /// Room display entry: name, encrypted status, unread count, do-not-disturb
    pub fn room_entries(&self) -> Vec<crate::RoomEntry> {
        self.rooms
            .iter()
            .filter(|room| !self.closed_room_ids.contains(&room.id))
            .map(|room| crate::RoomEntry {
                id: room.id.clone(),
                member_count: room.members.len(),
                title: room
                    .name
                    .clone()
                    .unwrap_or_else(|| self.text("private_chat_fallback")),
                encrypted: room.is_encrypted,
                unread: self.unread_counts.get(&room.id).copied().unwrap_or(0),
                muted: self.muted_room_ids.contains(&room.id),
            })
            .collect()
    }

    /// Hide a room locally (encrypted chat ended, left group): drop from the list,
    /// selection, and messages if it was the open one — never jump to another room.
    pub fn close_local_room(&mut self, room_id: &str) {
        self.closed_room_ids.insert(room_id.to_string());
        self.crypto.sessions.remove(room_id);
        if let Some(cache) = self.chat_cache.as_ref() {
            cache.forget_room(room_id);
        }
        let was_selected = self.current_room_id().as_deref() == Some(room_id);
        self.rooms.retain(|room| room.id != room_id);
        if was_selected {
            self.selected_room_index = None;
            self.messages.clear();
            self.older_cursor = None;
            self.has_more_older = false;
            // The cached member table described the room that just went away: forget it, or the
            // group settings sidebar could open on some other room holding this stale table.
            self.forget_room_detail();
        }
    }

    /// Immediately pull the room list once
    pub fn load_rooms_now(&mut self) {
        match self.connector.list_rooms() {
            Ok(rooms) => self.apply_room_snapshot(rooms),
            Err(error) if error.is_connection_failure() => {}
            Err(error) => self.notify_error(format!("{}: {error}", self.text("error_poll_rooms"))),
        }
    }
}
