//! The behavior half of the session layer: everything the interface asks the
//! server to do, one file per domain extending `impl Client`.

pub(crate) use crate::client::{
    Client, EncryptionPhase, EncryptionSession, NoticeKind, UpdateStage,
};
pub(crate) use baihua_core::api::UserInfo;
pub(crate) use baihua_core::config;
pub(crate) use baihua_core::{
    api::{
        CreateRoomRequest, EncryptHandshakeData, EncryptedMessageInfo, LoginRequest, MessageInfo,
        PollingEvent, ProfileUpdatePayload, RegisterRequest, RoomDetail, RoomInfo, RoomMember,
        RoomRequestInfo, WsCommand, outbound_ws_payload,
    },
    chat_cache::ChatCache,
    commands::allowed_signed_out,
    crypto, paths,
    update::{ReleaseChannel, UpdateCheck, check_for_update, download_package},
};
pub(crate) use std::collections::HashSet;
pub(crate) use std::fs;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::sync::Arc;
pub(crate) use std::sync::atomic::AtomicBool;
pub(crate) use std::time::{Duration, Instant};

/// What the interface must do after a command ran; the session layer never
/// touches overlays or focus itself.
#[derive(Debug, Clone, PartialEq)]
pub enum UiIntent {
    /// Nothing needs to be done (the command already expresses the result via notifications/data updates)
    Nothing,
    /// Exit the program
    Quit,
    /// Open your own profile card
    ShowOwnProfile,
    /// Open the settings panel, which holds language, appearance and server address
    OpenSettings,
    /// Open the login page; pre-fill with username when provided (`/login username`)
    OpenSignIn(Option<String>),
    /// Open the registration page; pre-fill with username when provided (`/register username`)
    OpenSignUp(Option<String>),
}

/// Whether a group member's role is admin or owner. The server sends role strings
/// in inconsistent case, so this one protocol fact folds it (unlike usernames).
pub(crate) fn is_admin_role(role: &str) -> bool {
    let lowered = role.to_lowercase();
    lowered == "owner" || lowered == "admin"
}

/// The `/kick` batch parameter only recognizes the exact lowercase `all`: any
/// other spelling is a username, so a typo can never clear the whole room.
pub(crate) fn is_kick_all_argument(argument: &str) -> bool {
    argument == "all"
}

/// Username in command parameters: empty argument means "not given", represented by None
pub(crate) fn some_username(argument: String) -> Option<String> {
    let trimmed = argument.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Parallel thread count for fetching avatars: machine parallelism, max 4, min 1
pub(crate) fn fetch_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get().min(4))
        .unwrap_or(1)
}

/// Whether a private chat request is still pending (the server's pending list doesn't give a status field)
pub(crate) fn is_pending_request(request: &RoomRequestInfo) -> bool {
    request
        .status
        .as_deref()
        .is_none_or(|status| status == "pending")
}

/// Form values to the server's three states: leave empty means don't rewrite this item
pub(crate) fn profile_field_value(text: &str) -> Option<Option<String>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(Some(trimmed.to_string()))
    }
}

/// Local image file to (file name, content type, bytes); the format whitelist
/// matches the upload endpoint.
pub(crate) fn read_local_image(path: &Path) -> Option<(String, String, Vec<u8>)> {
    let content_type = image_content_type(path)?;
    if fs::metadata(path)
        .ok()
        .is_some_and(|m| m.len() > 8 * 1024 * 1024)
    {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let file_name = path.file_name()?.to_str()?.to_string();
    Some((file_name, content_type.to_string(), bytes))
}

pub(crate) fn image_content_type(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

pub(crate) fn avatar_files() -> Vec<(String, PathBuf)> {
    let Some(directory) = paths::avatar_source_directory() else {
        return Vec::new();
    };
    let _ = fs::create_dir_all(&directory);
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut files: Vec<(String, PathBuf)> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?.to_string();
            image_content_type(&path).map(|_| (name, path))
        })
        .collect();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

pub(crate) mod account;
pub(crate) mod commands;
pub(crate) mod encryption;
pub(crate) mod events;
pub(crate) mod groups;
pub(crate) mod messages;
pub(crate) mod profile;
pub(crate) mod requests;
pub(crate) mod rooms;
pub(crate) mod search;

#[cfg(test)]
mod online_tests;
#[cfg(test)]
mod tests;
