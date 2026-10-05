//! Group rooms: information, roster, kick/add/remove, mute, leaving.

use super::*;

impl Client {
    // ==================== Group Chat and Members ====================

    /// Create a group chat
    pub fn create_group(&mut self, name: &str, members: &str) {
        let usernames: Vec<String> = members
            .split([',', '，']) // ASCII comma and the fullwidth comma people type with a Chinese keyboard
            .map(|entry| entry.trim().to_string())
            .filter(|entry| !entry.is_empty())
            .collect();
        let request = CreateRoomRequest::group(name.to_string(), usernames);
        match self.connector.create_room(request) {
            Ok(room) => {
                self.notify(self.text("group_created"));
                self.closed_room_ids.remove(&room.id);
                self.load_rooms_now();
                self.restart_websocket();
            }
            Err(error) => self.notify_error(format!(
                "{}: {error}",
                self.text("error_group_create_failed")
            )),
        }
    }

    /// Initiate private chat: the server requires a previously accepted request first, so what's sent here is an invitation
    pub fn create_private_chat(&mut self, username: &str) {
        let target = match self.connector.get_user_profile(username) {
            Ok(profile) => profile,
            Err(error) => {
                self.notify_error(format!("{}: {error}", self.text("error_user_not_found")));
                return;
            }
        };
        let message = self.text("private_request_message");
        match self
            .connector
            .create_room_request(&target.id, &message, true)
        {
            Ok(_request) => self.notify(self.text("notification_request_sent")),
            Err(error) => {
                self.notify_error(format!("{}: {error}", self.text("error_request_failed")))
            }
        }
    }

    /// The `/info` text lines: one blocking detail read, allowed because the
    /// command is a single user action, then formatting by `information_lines`.
    pub fn room_information(&self) -> Vec<String> {
        let Some(room_id) = self.current_room_id() else {
            return vec![self.text("error_no_room_selected")];
        };
        let detail = match self.connector.get_room(&room_id) {
            Ok(detail) => detail,
            Err(error) => return vec![format!("{}: {error}", self.text("error_room_info_failed"))],
        };
        let members = self.connector.list_members(&room_id).ok();
        let roster = members.map(|data| data.members).unwrap_or_default();
        self.information_lines(&detail, &roster)
    }

    /// Format a room information block from data already in hand: the summary lines
    /// plus one line per member, exactly what the sidebar and `/info` share.
    pub fn information_lines(&self, detail: &RoomDetail, roster: &[RoomMember]) -> Vec<String> {
        let mut lines = self.summary_lines(detail, roster);
        lines.extend(self.member_lines(roster));
        lines
    }

    /// The information header: name, creator, time, encryption, member and online
    /// counts. The roster in hand is authoritative for both counts.
    pub fn summary_lines(&self, detail: &RoomDetail, roster: &[RoomMember]) -> Vec<String> {
        let mut lines: Vec<String> = vec![
            format!(
                "{}: {}",
                self.text("room_info_title"),
                detail
                    .name
                    .clone()
                    .unwrap_or_else(|| self.text("private_chat_fallback"))
            ),
            format!(
                "{}: {}",
                self.text("room_info_creator"),
                if detail.created_by.is_empty() {
                    self.text("unknown_user")
                } else {
                    self.sender_display_name(&detail.created_by)
                }
            ),
            format!(
                "{}: {}",
                self.text("room_info_created_at"),
                detail.created_at
            ),
            format!(
                "{}: {}",
                self.text("room_info_encrypted"),
                if detail.is_encrypted {
                    self.text("yes")
                } else {
                    self.text("no")
                }
            ),
        ];
        lines.push(format!(
            "{}: {}",
            self.text("room_info_members"),
            roster.len().max(detail.member_count)
        ));
        lines.push(format!(
            "{}: {}",
            self.text("room_info_online"),
            self.online_member_count(roster)
        ));
        lines
    }

    /// One indented line per member with role and presence, used by `/info` only;
    /// the sidebar draws the same roster through `member_rows` instead.
    pub(crate) fn member_lines(&self, roster: &[RoomMember]) -> Vec<String> {
        roster
            .iter()
            .map(|member| format!("  {}", self.member_row_text(member)))
            .collect()
    }

    /// How many of the given members the client currently believes are online.
    /// Presence comes from the server's 0↔1 broadcast (see `presence_by_user`); an unknown user counts as offline.
    pub fn online_member_count(&self, roster: &[RoomMember]) -> usize {
        roster
            .iter()
            .filter(|member| {
                self.presence_by_user
                    .get(&member.user_id)
                    .copied()
                    .unwrap_or(false)
            })
            .count()
    }

    /// `/kick`: remove a member (or `all`) from the current group, after a group
    /// check, one roster fetch, an admin check and exact username matching.
    pub fn execute_kick(&mut self, target: &str) {
        let Some(room_id) = self.current_room_id() else {
            self.notify_error(self.text("error_no_room_selected"));
            return;
        };
        if !self.room_is_group(&room_id) {
            self.notify_error(self.text("error_not_group"));
            return;
        }
        let detail = match self.connector.get_room(&room_id) {
            Ok(detail) => detail,
            Err(error) => {
                self.notify_error(format!(
                    "{}: {error}",
                    self.text("error_get_members_failed")
                ));
                return;
            }
        };
        let is_admin = self.current_user_id.as_deref().is_some_and(|current| {
            detail
                .members
                .iter()
                .any(|member| member.user_id == current && is_admin_role(&member.role))
        });
        if !is_admin {
            self.notify_error(self.text("error_kick_requires_admin"));
            return;
        }
        if is_kick_all_argument(target) {
            self.kick_all_members(&room_id);
            return;
        }
        let Some(member) = detail
            .members
            .iter()
            .find(|member| member.username == target)
        else {
            self.notify_error(
                self.text("error_user_not_found_in_group")
                    .replace("{target}", target),
            );
            return;
        };
        let member_user_id = member.user_id.clone();
        match self.connector.remove_member(&room_id, &member_user_id) {
            Ok(_result) => {
                // kicking yourself out also gets recorded in the "active exit" set, so it's not misjudged as "removed from the group"
                if self.current_user_id.as_deref() == Some(member_user_id.as_str()) {
                    self.left_room_ids.insert(room_id);
                }
                self.notify(self.text("removed_member").replace("{target}", target));
                self.load_rooms_now();
            }
            Err(error) => self.notify_error(format!("{}: {error}", self.text("error_kick_failed"))),
        }
    }

    /// `/kick all`: remove everyone else, then leave. Single removal failures do not
    /// stop the batch, and leaving is recorded so the event is not read as a kick.
    pub(crate) fn kick_all_members(&mut self, room_id: &str) {
        let member_ids: Vec<String> = self
            .rooms
            .iter()
            .find(|room| room.id == room_id)
            .map(|room| room.members.clone())
            .unwrap_or_default();
        let own_id = self.current_user_id.clone().unwrap_or_default();
        let mut other_ids: Vec<String> = member_ids
            .iter()
            .filter(|member_id| *member_id != &own_id)
            .cloned()
            .collect();
        other_ids.reverse();
        for member_id in &other_ids {
            let _ = self.connector.remove_member(room_id, member_id);
        }
        self.left_room_ids.insert(room_id.to_string());
        let _ = self.connector.remove_member(room_id, &own_id);
        self.load_rooms_now();
        self.notify(self.text("removed_all_members"));
    }

    // ==================== Group Settings Sidebar ====================

    /// Fetch and cache the current room detail once. Blocking, so only for user
    /// actions; a failure keeps the old cache and reports like any other read.
    pub fn refresh_room_detail(&mut self) {
        let Some(room_id) = self.current_room_id() else {
            self.current_room_detail = None;
            return;
        };
        match self.connector.get_room(&room_id) {
            Ok(detail) => self.current_room_detail = Some(detail),
            Err(error) => self.notify_error(format!(
                "{}: {error}",
                self.text("error_get_room_info_failed")
            )),
        }
    }

    /// Drop the cached room detail: the room selection changed, so the old member table no longer applies.
    pub fn forget_room_detail(&mut self) {
        self.current_room_detail = None;
    }

    /// The cached member table as (user id, "username (role, presence)") rows for
    /// the sidebar; removal acts on the id because names can change.
    pub fn member_rows(&self) -> Vec<(String, String)> {
        let Some(detail) = self.current_room_detail.as_ref() else {
            return Vec::new();
        };
        detail
            .members
            .iter()
            .map(|member| (member.user_id.clone(), self.member_row_text(member)))
            .collect()
    }

    /// Display text for one member row: "username (role, online/offline)", with the presence suffix
    /// left off for yourself (the same shape the `/info` member list uses).
    pub(crate) fn member_row_text(&self, member: &RoomMember) -> String {
        let is_self = self.current_user_id.as_deref() == Some(member.user_id.as_str());
        let presence = if is_self {
            String::new()
        } else if self
            .presence_by_user
            .get(&member.user_id)
            .copied()
            .unwrap_or(false)
        {
            format!(", {}", self.text("status_online"))
        } else {
            format!(", {}", self.text("status_offline"))
        };
        format!("{} ({}{})", member.username, member.role, presence)
    }

    /// Whether the signed-in user is an admin or owner of the current room, decided
    /// from the cached member table so drawing costs no request.
    pub fn allows_removal(&self) -> bool {
        let Some(detail) = self.current_room_detail.as_ref() else {
            return false;
        };
        let Some(own_id) = self.current_user_id.as_deref() else {
            return false;
        };
        detail
            .members
            .iter()
            .any(|member| member.user_id == own_id && is_admin_role(&member.role))
    }

    /// Remove one member by **user id** (the sidebar lists the authoritative table;
    /// `/kick` keeps username matching). Leaving via removal is recorded like `/kick`.
    pub fn remove_member_by_id(&mut self, user_id: &str) {
        let Some(room_id) = self.current_room_id() else {
            self.notify_error(self.text("error_no_room_selected"));
            return;
        };
        if !self.room_is_group(&room_id) {
            self.notify_error(self.text("error_not_group"));
            return;
        }
        if !self.allows_removal() {
            self.notify_error(self.text("error_kick_requires_admin"));
            return;
        }
        let target = self
            .current_room_detail
            .as_ref()
            .and_then(|detail| {
                detail
                    .members
                    .iter()
                    .find(|member| member.user_id == user_id)
            })
            .map(|member| member.username.clone());
        let Some(target) = target else {
            self.notify_error(self.text("error_kick_target_unknown"));
            return;
        };
        match self.connector.remove_member(&room_id, user_id) {
            Ok(_result) => {
                if self.current_user_id.as_deref() == Some(user_id) {
                    self.left_room_ids.insert(room_id.clone());
                }
                self.notify(self.text("removed_member").replace("{target}", &target));
                self.load_rooms_now();
                // The member table just changed, and the sidebar is showing it: refresh the cache once,
                // so the removed row disappears immediately instead of on the next manual refresh.
                self.refresh_room_detail();
            }
            Err(error) => self.notify_error(format!("{}: {error}", self.text("error_kick_failed"))),
        }
    }

    /// Add a member to the current group chat (`/add_member username`): an empty name is a
    /// silent no-op; only "no room selected" and "not a group chat" still report anything.
    pub fn add_member(&mut self, username: &str) {
        let username = username.trim();
        if username.is_empty() {
            return;
        }
        let Some(room_id) = self.current_room_id() else {
            self.notify_error(self.text("error_no_room_selected"));
            return;
        };
        if !self.room_is_group(&room_id) {
            self.notify_error(self.text("error_not_group"));
            return;
        }
        match self
            .connector
            .add_members(&room_id, &[username.to_string()])
        {
            Ok(_result) => {
                self.notify(self.text("add_member_success"));
                self.load_rooms_now();
                // The member table just changed and the sidebar is showing it: refresh the cache once,
                // so the new row appears immediately.
                self.refresh_room_detail();
            }
            Err(error) => {
                self.notify_error(format!("{}: {error}", self.text("error_add_member_failed")))
            }
        }
    }

    /// `/mute`: without arguments toggles the current room's do-not-disturb; with arguments sets according to the parameter
    /// (`true`/`on`/`1` to enable, `false`/`off`/`0` to disable); both forms are recognized by the terminal version.
    pub fn apply_mute_command(&mut self, argument: &str) {
        let Some(room_id) = self.current_room_id() else {
            self.notify_error(self.text("error_no_room_selected"));
            return;
        };
        let wanted = match argument.trim().to_lowercase().as_str() {
            "" => !self.muted_room_ids.contains(&room_id),
            "true" | "on" | "1" => true,
            "false" | "off" | "0" => false,
            _ => {
                self.notify_error(self.text("error_mute_usage"));
                return;
            }
        };
        if wanted {
            self.muted_room_ids.insert(room_id);
        } else {
            self.muted_room_ids.remove(&room_id);
        }
        self.save_preferences();
        self.notify(self.text(if wanted {
            "mute_dnd_on"
        } else {
            "mute_dnd_off"
        }));
    }

    /// Exit the current group chat
    pub fn leave_current_room(&mut self) {
        let Some(index) = self.selected_room_index else {
            return;
        };
        let Some(room) = self.rooms.get(index).cloned() else {
            return;
        };
        match self
            .connector
            .remove_member(&room.id, &self.current_user_id.clone().unwrap_or_default())
        {
            Ok(_result) => {
                self.left_room_ids.insert(room.id.clone());
                self.close_local_room(&room.id);
                self.notify(self.text("room_left"));
                self.load_rooms_now();
            }
            Err(error) => {
                self.notify_error(format!("{}: {error}", self.text("error_leave_failed")))
            }
        }
    }
}
