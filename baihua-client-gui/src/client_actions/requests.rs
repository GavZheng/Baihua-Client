//! Private-chat requests: receive, accept, decline, cancel, announce.

use super::*;

impl Client {
    // ==================== Private Chat Requests ====================

    /// Align received requests with poll results: entries already processed by this side stay in history
    pub(crate) fn apply_requests(&mut self, polled: Vec<RoomRequestInfo>) {
        // First-seen pending request: pop a desktop notification (same as terminal version; repeated polls don't pop repeatedly)
        let fresh_senders: Vec<String> = polled
            .iter()
            .filter(|request| {
                !self
                    .pending_requests
                    .iter()
                    .any(|known| known.id == request.id)
            })
            .map(|request| match &request.sender {
                Some(sender) => sender.username.clone(),
                None => self.text("unknown_user"),
            })
            .collect();
        let kept: Vec<RoomRequestInfo> = self
            .pending_requests
            .iter()
            .filter(|request| !is_pending_request(request))
            .filter(|kept| !polled.iter().any(|request| request.id == kept.id))
            .cloned()
            .collect();
        self.pending_requests = polled.into_iter().chain(kept).collect();
        for sender_name in fresh_senders {
            let title = self.text("notification_new_request");
            let body = self
                .text("notification_request_received")
                .replace("{sender}", &sender_name);
            self.notify_in_app(&title, &body);
            crate::desktop_notice::send(self.sound_enabled, &title, &body);
        }
    }

    /// Private chat request list: received ones come first
    pub fn request_entries(&self) -> Vec<(bool, RoomRequestInfo)> {
        self.pending_requests
            .iter()
            .cloned()
            .map(|request| (false, request))
            .chain(
                self.sent_requests
                    .iter()
                    .cloned()
                    .map(|request| (true, request)),
            )
            .collect()
    }

    /// Number of pending invitations (used for number badge in settings)
    pub fn pending_count(&self) -> usize {
        self.pending_requests
            .iter()
            .filter(|request| is_pending_request(request))
            .count()
            + self
                .sent_requests
                .iter()
                .filter(|request| request.status.as_deref() == Some("pending"))
                .count()
    }

    /// Accept a received invitation
    pub fn accept_request(&mut self, request_id: &str) {
        match self.connector.accept_room_request(request_id) {
            Ok(accepted) => {
                self.mark_request_handled(request_id, "accepted");
                self.notify(self.text("notification_request_accepted"));
                self.load_rooms_now();
                self.restart_websocket();
                let _ = accepted;
            }
            Err(error) => {
                self.notify_error(format!("{}: {error}", self.text("error_accept_failed")))
            }
        }
    }

    /// Reject a received invitation
    pub fn decline_request(&mut self, request_id: &str) {
        match self.connector.decline_room_request(request_id) {
            Ok(_status) => {
                self.mark_request_handled(request_id, "declined");
                self.notify(self.text("notification_request_declined"));
            }
            Err(error) => {
                self.notify_error(format!("{}: {error}", self.text("error_decline_failed")))
            }
        }
    }

    /// Withdraw an invitation you sent
    pub fn cancel_sent_request(&mut self, request_id: &str) {
        match self.connector.cancel_room_request(request_id) {
            Ok(_status) => {
                if let Some(request) = self
                    .sent_requests
                    .iter_mut()
                    .find(|request| request.id == request_id)
                {
                    request.status = Some("cancelled".to_string());
                }
                self.notify(self.text("notification_request_cancelled"));
            }
            Err(error) => {
                self.notify_error(format!("{}: {error}", self.text("error_cancel_failed")))
            }
        }
    }

    pub(crate) fn mark_request_handled(&mut self, request_id: &str, status: &str) {
        if let Some(request) = self
            .pending_requests
            .iter_mut()
            .find(|request| request.id == request_id)
        {
            request.status = Some(status.to_string());
        }
    }

    /// Announce the invitations the other side declined by comparing last round's
    /// records; walking this round's list re-announced old declines after a login.
    pub(crate) fn announce_declined(&mut self, latest: &[RoomRequestInfo]) {
        let mut notices: Vec<String> = Vec::new();
        for previous in &self.sent_requests {
            if previous.status.as_deref() != Some("pending") {
                continue;
            }
            let Some(current) = latest.iter().find(|request| request.id == previous.id) else {
                continue;
            };
            if current.status.as_deref() != Some("declined") {
                continue;
            }
            let receiver_name = current
                .receiver
                .as_ref()
                .map(|receiver| receiver.username.clone())
                .unwrap_or_else(|| self.text("unknown_user"));
            notices.push(
                self.text("request_declined_notice")
                    .replace("{user}", &receiver_name),
            );
        }
        for notice in notices {
            self.notify(notice);
        }
    }

    /// Status text for request entries (unknown status displayed as-is, never guessed as "withdrawn")
    pub fn request_status_label(&self, status: &str) -> String {
        match status {
            "pending" => self.text("request_status_pending"),
            "accepted" => self.text("request_status_accepted"),
            "declined" => self.text("request_status_declined"),
            "expired" => self.text("request_status_expired"),
            "cancelled" => self.text("request_status_cancelled"),
            other => other.to_string(),
        }
    }
}
