//! End-to-end encryption handshake: invite, accept, session-ready, encrypted traffic.

use super::*;

impl Client {
    // ==================== End-to-End Encryption Handshake ====================

    /// Initiate handshake: generate ephemeral key, sign public key and send invitation
    pub fn initiate_encryption(&mut self, room_id: &str, pending_content: Option<String>) {
        let ephemeral_secret = crypto::generate_ephemeral_secret();
        let public_key = crypto::encode_x25519_public(&ephemeral_secret);
        let identity_key = crypto::encode_identity_public(&self.crypto.identity_key);
        let Ok(signature) = crypto::sign_public_key(&self.crypto.identity_key, &public_key) else {
            self.notify_error(self.text("notification_encryption_failed_signature"));
            return;
        };
        self.send_payload(outbound_ws_payload(
            self.connector.version(),
            WsCommand::EncryptRequest {
                room_id,
                public_key: &public_key,
                identity_key: &identity_key,
                signature: &signature,
            },
        ));
        self.crypto.sessions.insert(
            room_id.to_string(),
            EncryptionSession {
                phase: EncryptionPhase::AwaitingAcceptance,
                ephemeral_secret: Some(ephemeral_secret),
                own_public_key: public_key,
                shared_key: None,
                pending_content,
                initiated_at: Instant::now(),
            },
        );
    }

    /// Received invitation: accept and reply with your public key
    pub(crate) fn handle_invitation(&mut self, handshake: EncryptHandshakeData) {
        if Some(&handshake.peer_id) == self.current_user_id.as_ref() {
            return;
        }
        if !crypto::verify_handshake_signature(
            &handshake.identity_key,
            &handshake.public_key,
            &handshake.signature,
        ) {
            self.notify_error(self.text("notification_invitation_failed_signature"));
            return;
        }
        let existing = self
            .crypto
            .sessions
            .get(&handshake.room_id)
            .map(|session| session.phase);
        if existing == Some(EncryptionPhase::Active) {
            return;
        }
        let ephemeral_secret = crypto::generate_ephemeral_secret();
        let public_key = crypto::encode_x25519_public(&ephemeral_secret);
        let identity_key = crypto::encode_identity_public(&self.crypto.identity_key);
        let Ok(signature) = crypto::sign_public_key(&self.crypto.identity_key, &public_key) else {
            self.notify_error(self.text("notification_encryption_failed_signature"));
            return;
        };
        let shared_key = match crypto::derive_shared_key(ephemeral_secret, &handshake.public_key) {
            Ok(key) => key,
            Err(error) => {
                self.notify_error(format!("{}: {error}", self.text("error_key_derivation")));
                return;
            }
        };
        self.send_payload(outbound_ws_payload(
            self.connector.version(),
            WsCommand::EncryptAccept {
                room_id: &handshake.room_id,
                public_key: &public_key,
                identity_key: &identity_key,
                signature: &signature,
            },
        ));
        self.send_payload(outbound_ws_payload(
            self.connector.version(),
            WsCommand::EncryptReady {
                room_id: &handshake.room_id,
            },
        ));
        self.crypto.sessions.insert(
            handshake.room_id.clone(),
            EncryptionSession {
                phase: EncryptionPhase::AwaitingSessionReady,
                ephemeral_secret: None,
                own_public_key: public_key,
                shared_key: Some(shared_key),
                pending_content: None,
                initiated_at: Instant::now(),
            },
        );
    }

    /// Received the other side's acceptance: compute the shared key and reply ready
    pub(crate) fn handle_accepted(&mut self, handshake: EncryptHandshakeData) {
        if Some(&handshake.peer_id) == self.current_user_id.as_ref() {
            return;
        }
        let waiting = self
            .crypto
            .sessions
            .get(&handshake.room_id)
            .is_some_and(|session| session.phase == EncryptionPhase::AwaitingAcceptance);
        if !waiting {
            return;
        }
        if !crypto::verify_handshake_signature(
            &handshake.identity_key,
            &handshake.public_key,
            &handshake.signature,
        ) {
            self.crypto.sessions.remove(&handshake.room_id);
            self.notify_error(self.text("notification_accept_failed_signature"));
            return;
        }
        let shared_key = {
            let Some(session) = self.crypto.sessions.get_mut(&handshake.room_id) else {
                return;
            };
            let Some(ephemeral_secret) = session.ephemeral_secret.take() else {
                return;
            };
            match crypto::derive_shared_key(ephemeral_secret, &handshake.public_key) {
                Ok(key) => key,
                Err(error) => {
                    self.notify_error(format!("{}: {error}", self.text("error_key_derivation")));
                    return;
                }
            }
        };
        self.send_payload(outbound_ws_payload(
            self.connector.version(),
            WsCommand::EncryptReady {
                room_id: &handshake.room_id,
            },
        ));
        if let Some(session) = self.crypto.sessions.get_mut(&handshake.room_id) {
            session.phase = EncryptionPhase::AwaitingSessionReady;
            session.shared_key = Some(shared_key);
            session.initiated_at = Instant::now();
        }
    }

    /// Server confirms both sides are ready: activate the session and resend messages that piled up during the handshake
    pub(crate) fn handle_session_ready(&mut self, room_id: String) {
        let pending = match self.crypto.sessions.get_mut(&room_id) {
            Some(session) => {
                session.phase = EncryptionPhase::Active;
                session.initiated_at = Instant::now();
                session.pending_content.take()
            }
            None => return,
        };
        self.notify(self.text("notification_encryption_ready"));
        if let Some(content) = pending {
            self.send_encrypted(&room_id, &content);
        }
    }

    /// Received ciphertext: decrypt and fall into the view as a normal message
    pub(crate) fn decrypt_message(&mut self, incoming: EncryptedMessageInfo) {
        let key = match self
            .crypto
            .sessions
            .get(&incoming.room_id)
            .and_then(|session| session.shared_key)
        {
            Some(key) => key,
            None => {
                self.notify_error(self.text("notification_message_undecryptable"));
                return;
            }
        };
        let Ok(plaintext) = crypto::decrypt_message(&key, &incoming.ciphertext) else {
            self.notify_error(self.text("notification_message_undecryptable"));
            return;
        };
        let is_own = Some(&incoming.sender_id) == self.current_user_id.as_ref();
        let room_id = incoming.room_id.clone();
        let sender_name = self.sender_display_name(&incoming.sender_id);
        let preview = plaintext.clone();
        self.absorb_message(MessageInfo {
            id: incoming.id,
            room_id: incoming.room_id,
            sender_id: incoming.sender_id,
            content: plaintext,
            created_at: incoming.created_at,
        });
        // Encrypted messages also pop desktop notifications (same as terminal version: the body takes the just-decrypted plaintext)
        if !is_own && !self.muted_room_ids.contains(&room_id) {
            let title = self.text("notification_new_message");
            let body = format!("{sender_name}: {preview}");
            self.notify_in_app(&title, &body);
            crate::desktop_notice::send(self.sound_enabled, &title, &body);
        }
    }

    /// Resend a stalled handshake at intervals (invitation, then ready notice),
    /// reusing the same ephemeral key; the payload is sent after the borrow ends.
    pub(crate) fn resend_handshakes(&mut self) {
        let interval = self.connector.version().handshake_resend_interval();
        let stalled: Vec<String> = self
            .crypto
            .sessions
            .iter()
            .filter(|(_room_id, session)| session.phase != EncryptionPhase::Active)
            .filter(|(_room_id, session)| session.initiated_at.elapsed() >= interval)
            .map(|(room_id, _session)| room_id.clone())
            .collect();
        let identity_key = crypto::encode_identity_public(&self.crypto.identity_key);
        for room_id in stalled {
            let payload = match self.crypto.sessions.get_mut(&room_id) {
                None => continue,
                Some(session) => match session.phase {
                    EncryptionPhase::AwaitingAcceptance => {
                        let Ok(signature) = crypto::sign_public_key(
                            &self.crypto.identity_key,
                            &session.own_public_key,
                        ) else {
                            continue;
                        };
                        session.initiated_at = Instant::now();
                        Some(outbound_ws_payload(
                            self.connector.version(),
                            WsCommand::EncryptRequest {
                                room_id: &room_id,
                                public_key: &session.own_public_key,
                                identity_key: &identity_key,
                                signature: &signature,
                            },
                        ))
                    }
                    EncryptionPhase::AwaitingSessionReady => {
                        session.initiated_at = Instant::now();
                        Some(outbound_ws_payload(
                            self.connector.version(),
                            WsCommand::EncryptReady { room_id: &room_id },
                        ))
                    }
                    EncryptionPhase::Active => None,
                },
            };
            if let Some(payload) = payload {
                self.send_payload(payload);
            }
        }
    }
}
