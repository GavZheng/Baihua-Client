use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur during API communication
#[derive(Debug, Error)]
pub enum ConnectorError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("API error: {error_code} - {message}")]
    Api { error_code: String, message: String },
    #[error("Missing authentication token")]
    MissingToken,
    #[error("Invalid server response: {0}")]
    InvalidResponse(String),
}

impl ConnectorError {
    /// Whether an error is of the "server unreachable" family (the connection could not be established or timed out), kept apart from business errors the server answered deliberately.
    /// Unreachability repeats with every two-second poll, so the interface should warn once on the transition and keep marking it in the top status bar,
    /// which is why callers must classify by kind rather than by error text (the text is localized, string matching would lie).
    pub fn is_unreachable(&self) -> bool {
        match self {
            ConnectorError::Http(error) => error.is_connect() || error.is_timeout(),
            _ => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, ConnectorError>;

impl ConnectorError {
    /// Whether the failure is "cannot reach the server" at transport level (connection refused, DNS failure, timeout, reset).
    /// Distinct from business errors (the server answered with an error code): the former repeats on every poll,
    /// so the interface warns once at the transition and lets the status bar carry the state instead of popping a box each time.
    pub fn is_connection_failure(&self) -> bool {
        match self {
            ConnectorError::Http(error) => {
                error.is_connect() || error.is_timeout() || error.is_request()
            }
            _ => false,
        }
    }
}

/// Server API version. Every wire difference that follows the server version (response success codes, endpoint paths, event names,
/// payload shapes, error strings, heartbeat and reconnect timing) is decided here through the match methods of this enum.
/// Supporting a new server version means adding one variant here and one arm in each match method below;
/// business layers such as app.rs never learn the concrete wire format.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ApiVersion {
    /// 0.1.3: the response wrapper field is error_code, success code OK
    V0_1_3,
    /// 0.1.4: the wrapper field became code, success code SUCCESS (private chats must be opened through a chat request)
    V0_1_4,
    /// Unrecognized version: behave like the closest known compatible version and ask the user to double-check during the probe
    Unknown,
}

impl ApiVersion {
    /// Parse the version from the server_version string the greet response returns (matched on the first three major.minor.patch segments, a leading 'v' tolerated).
    /// When the minor or patch number is unknown the closest newer known version's behavior applies.
    pub fn from_version_string(version_text: &str) -> Self {
        let normalized = version_text.trim().trim_start_matches('v').to_string();
        let mut segments = normalized.split('.');
        match (segments.next(), segments.next(), segments.next()) {
            (Some("0"), Some("1"), Some("3")) => ApiVersion::V0_1_3,
            (Some("0"), Some("1"), Some(_)) => ApiVersion::V0_1_4,
            _ => ApiVersion::Unknown,
        }
    }

    /// The set of code values ApiResponse treats as success on this version (a tuple slice, so no if-stacks scatter)
    pub fn success_codes(self) -> &'static [&'static str] {
        match self {
            ApiVersion::V0_1_3 => &["OK"],
            ApiVersion::V0_1_4 | ApiVersion::Unknown => &["SUCCESS", "OK"],
        }
    }

    /// The endpoint path for "fetch the whole registered user directory" on this version.
    /// Since 0.1.4 the route table reads `/user/list` as the username `/list` under `/user/{user}` (always a 404),
    /// so the full directory goes through `/user/search?username=` (the server matches everything with `ILIKE '%%'`) plus paging.
    pub fn user_directory_endpoint(self) -> &'static str {
        match self {
            ApiVersion::V0_1_3 => "/api/v1/user/list",
            ApiVersion::V0_1_4 | ApiVersion::Unknown => "/api/v1/user/search",
        }
    }

    /// Whether fetching the user directory needs client-side page aggregation on this version.
    /// `/user/search` caps a page at 50 rows, so the full directory must loop until count; `/user/list` returns everything at once.
    pub fn user_directory_requires_paging(self) -> bool {
        match self {
            ApiVersion::V0_1_3 => false,
            ApiVersion::V0_1_4 | ApiVersion::Unknown => true,
        }
    }
}

/// Standard API response wrapper (field code on 0.1.4, compatible with error_code on 0.1.3)
#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    #[allow(dead_code)]
    response_id: String,
    #[serde(alias = "error_code")]
    code: String,
    message: String,
    data: Option<T>,
}

/// Greet response (raw JSON, not wrapped in standard format)
#[derive(Debug, Deserialize, Clone)]
pub struct GreetData {
    pub server_version: String,
    pub api_version: String,
    pub message: String,
}

/// Health check response
#[derive(Debug, Deserialize, Clone)]
pub struct HealthData {
    pub status: String,
}

/// User registration request
#[derive(Debug, Serialize, Clone)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
}

/// User login request
#[derive(Debug, Serialize, Clone)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// User info returned from API
#[derive(Debug, Deserialize, Clone)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub email: String,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub phone_number: Option<String>,
    /// Bio text, served by the profile endpoint since 0.1.4; treated as missing on older versions
    #[serde(default)]
    pub bio: Option<String>,
    /// Avatar address (an uploaded avatar stores a /static/avatars/... relative path; a custom profile may hold a full URL)
    #[serde(default)]
    pub avatar: Option<String>,
    pub created_at: String,
    pub is_active: bool,
}

/// Public view of another user's profile (the data.user of GET /api/v1/user/{user}).
/// The server deliberately hides email and phone, so this is modeled apart instead of reusing UserInfo.
#[derive(Debug, Deserialize, Clone)]
pub struct PublicProfile {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
}

/// Profile update payload (PATCH /api/v1/user/me). The server treats "field absent = keep current, explicit null = clear,
/// non-empty string = validate then write" as three states, which is why this uses Option<Option<String>>
/// and lets the outer default stay out of the JSON, so untouched fields are never cleared by accident.
#[derive(Debug, Serialize, Default, Clone)]
pub struct ProfileUpdatePayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_number: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bio: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<Option<String>>,
}

/// Password change payload (PATCH /api/v1/user/me/password).
/// Both passwords go through the same client-side transform login uses; the server bcrypt-checks the transformed values.
#[derive(Debug, Serialize, Clone)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

/// Account deletion payload (DELETE /api/v1/user/me): the server re-checks the password before deleting
#[derive(Debug, Serialize, Clone)]
pub struct DeleteAccountRequest {
    pub password: String,
}

/// User data in registration response
#[derive(Debug, Deserialize, Clone)]
pub struct UserData {
    pub user: UserInfo,
}

/// Login response with token
#[derive(Debug, Deserialize, Clone)]
pub struct LoginData {
    pub token: String,
    pub user: UserInfo,
}

/// Group chat creation request (since 0.1.4 private chats are not created directly; an accepted chat request makes the server open the room)
#[derive(Debug, Serialize, Clone)]
pub struct CreateRoomRequest {
    pub is_group: bool,
    pub name: String,
    pub usernames: Vec<String>,
}

impl CreateRoomRequest {
    pub fn group(name: String, usernames: Vec<String>) -> Self {
        Self {
            is_group: true,
            name,
            usernames,
        }
    }
}

/// Room information
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct RoomInfo {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// The room founder. `rooms.created_by` is `ON DELETE SET NULL`: after the founder deletes the account this comes back null.
    /// Deserializing it as String makes the **entire** room list fail to decode (every room appears broken),
    /// so null collapses to an empty string and the display side falls back to "unknown".
    #[serde(default, deserialize_with = "null_to_empty_string")]
    pub created_by: String,
    pub created_at: String,
    pub is_group: bool,
    /// Whether this room is end-to-end encrypted (established with the chat request's encryption flag)
    #[serde(default)]
    pub is_encrypted: bool,
    pub members: Vec<String>,
}

/// Detailed room info
#[derive(Debug, Deserialize, Clone)]
pub struct RoomDetail {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Same as `RoomInfo::created_by`: null after the account is deleted, which must not void the whole room detail.
    #[serde(default, deserialize_with = "null_to_empty_string")]
    pub created_by: String,
    pub created_at: String,
    pub is_group: bool,
    /// Whether this room is end-to-end encrypted
    #[serde(default)]
    pub is_encrypted: bool,
    pub member_count: usize,
    pub members: Vec<RoomMember>,
}

/// Room member info
#[derive(Debug, Deserialize, Clone)]
pub struct RoomMember {
    pub user_id: String,
    pub username: String,
    #[serde(default)]
    pub nickname: Option<String>,
    pub role: String,
    pub joined_at: String,
}

/// Add members request
#[derive(Debug, Serialize, Clone)]
pub struct AddMembersRequest {
    pub usernames: Vec<String>,
}

/// Add members response
#[derive(Debug, Deserialize, Clone)]
pub struct AddMembersData {
    pub added: Vec<AddedMember>,
    pub added_count: usize,
}

/// Added member info
#[derive(Debug, Deserialize, Clone)]
pub struct AddedMember {
    pub user_id: String,
    pub username: String,
    pub joined_at: String,
}

/// Members list response
#[derive(Debug, Deserialize, Clone)]
pub struct MembersData {
    pub members: Vec<RoomMember>,
    pub count: usize,
}

/// Remove member response
#[derive(Debug, Deserialize, Clone)]
pub struct RemoveMemberData {
    pub room_id: String,
    #[serde(default)]
    pub removed_user_id: Option<String>,
    #[serde(default)]
    pub left_user_id: Option<String>,
    #[serde(default)]
    pub room_deleted: bool,
}

/// Message info
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct MessageInfo {
    pub id: String,
    pub room_id: String,
    /// The sender. `messages.sender_id` is also `ON DELETE SET NULL`: null once that person's account is gone.
    /// One unreadable message would void the whole page of history, so this also collapses to an empty string and the display shows "unknown user".
    #[serde(default, deserialize_with = "null_to_empty_string")]
    pub sender_id: String,
    /// In encrypted rooms this field is null (the ciphertext lives in encrypted_content); accepted as an empty string.
    /// An empty content is only the marker "this history message has no readable body"; each interface picks the placeholder text from its own language table
    /// (key `message_encrypted_history_unavailable`): the core layer never hard-codes a hint in one language.
    #[serde(default, deserialize_with = "null_to_empty_string")]
    pub content: String,
    pub created_at: String,
}

/// Server-null fields (null after the referenced user is deleted, null body in encrypted rooms) all arrive as empty strings:
/// both a missing key and an explicit null must be caught or the whole batch fails.
fn null_to_empty_string<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = <Option<String>>::deserialize(deserializer)?;
    Ok(value.unwrap_or_default())
}

/// Messages response with pagination
#[derive(Debug, Deserialize, Clone)]
pub struct MessagesData {
    pub messages: Vec<MessageInfo>,
    pub has_more: bool,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// A user search result entry (the directory listing and keyword search share the same public fields)
#[derive(Debug, Deserialize, Clone)]
pub struct UserSearchResult {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
}

/// One page of `/user/search`: besides the entries it carries the server-reported total hit count so the aggregation loop knows where to stop
#[derive(Debug, Deserialize, Clone)]
struct UserSearchPage {
    users: Vec<UserSearchResult>,
    #[serde(default)]
    count: i64,
}

impl From<UserInfo> for UserSearchResult {
    /// Downgrade a full user object to public entries: email and phone are never shown to outsiders and are always dropped
    fn from(user: UserInfo) -> Self {
        Self {
            id: user.id,
            username: user.username,
            nickname: user.nickname,
            bio: user.bio,
            avatar: user.avatar,
        }
    }
}

/// The counterparty in a chat request (the sender in the received list, the receiver in the sent list)
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct RoomRequestPeer {
    pub user_id: String,
    pub username: String,
    #[serde(default)]
    pub nickname: Option<String>,
}

/// A chat request entry (shared by the pending and sent lists)
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct RoomRequestInfo {
    pub id: String,
    pub message: String,
    pub is_encrypted: bool,
    pub created_at: String,
    #[serde(default)]
    pub sender: Option<RoomRequestPeer>,
    #[serde(default)]
    pub receiver: Option<RoomRequestPeer>,
    #[serde(default)]
    pub status: Option<String>,
}

/// Chat request creation payload
#[derive(Debug, Serialize, Clone)]
pub struct CreateRoomRequestPayload {
    pub receiver_id: String,
    pub is_encrypted: bool,
    pub message: String,
}

/// Result of a chat request state change (shared by create, accept, decline and cancel)
#[derive(Debug, Deserialize, Clone)]
pub struct RoomRequestStatusResult {
    pub request_id: String,
    pub status: String,
}

/// Result of accepting a chat request, including the private room the server created
#[derive(Debug, Deserialize, Clone)]
pub struct AcceptedRoomRequest {
    pub request_id: String,
    pub status: String,
    pub room: RoomInfo,
}

/// Android-only: the built-in (Mozilla) root certificate set converted to the
/// request library's certificate type. Entries that fail to parse are dropped --
/// the list is a stable, vendor-maintained set, so a drop would be a build
/// anomaly, not a runtime concern; verification keeps every usable anchor.
#[cfg(target_os = "android")]
fn android_trusted_root_certificates() -> Vec<reqwest::Certificate> {
    webpki_root_certs::TLS_SERVER_ROOT_CERTS
        .iter()
        .filter_map(|root| reqwest::Certificate::from_der(root.as_ref()).ok())
        .collect()
}

/// Centralized network communication component for Baihua Server
#[derive(Debug, Clone)]
pub struct Connector {
    client: Client,
    base_url: String,
    token: Option<String>,
    /// The server API version learned by probing; decides success codes and other wire differences; defaults to the newest known version
    version: ApiVersion,
    /// The raw version string the server greeted with (not normalized, shown to users as "server version");
    /// an empty string when no probe has succeeded
    server_version_text: String,
}

impl Connector {
    /// Create a new Connector with the given base URL
    pub fn new(base_url: &str) -> Self {
        let builder = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            // A dead or firewalled server must never strand a caller for the
            // full request timeout: TCP connect gives up after three seconds
            // (the startup black screen the developers saw was a 30-second
            // connect timeout blocking the first frame).
            .connect_timeout(std::time::Duration::from_secs(3));
        // See the dependency note in `baihua-core/Cargo.toml`: on Android the
        // platform trust store is not reachable from this packaging path, so
        // verification runs against the compiled-in root set instead.
        #[cfg(target_os = "android")]
        let builder = builder.tls_certs_only(android_trusted_root_certificates());
        let client = builder.build().expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            token: None,
            version: ApiVersion::V0_1_4,
            server_version_text: String::new(),
        }
    }

    /// Adopt a probe run earlier on another (cloned) connector: store the wire
    /// version and the raw greeting text without touching the network. The
    /// graphical client probes in a background startup thread and hands the
    /// verdict back to the authoritative connector on the UI thread.
    pub fn adopt_probe_result(&mut self, version: ApiVersion, server_version_text: &str) {
        self.version = version;
        self.server_version_text = server_version_text.to_string();
    }

    /// Set the JWT authentication token
    pub fn set_token(&mut self, token: &str) {
        self.token = Some(token.to_string());
    }

    /// The server API version currently in effect
    pub fn version(&self) -> ApiVersion {
        self.version
    }

    /// Probe the server version: call greet, read server_version (falling back to api_version), parse and record it.
    /// Call it after a successful login, an automatic login at startup, or a server address switch so later wire decisions match the real version.
    /// Returns (probed version, raw version string); the interface shows the raw string with its "unrecognized version" warning
    pub fn probe_version(&mut self) -> Result<(ApiVersion, String)> {
        let greet = self.greet()?;
        let raw = if !greet.server_version.is_empty() {
            greet.server_version
        } else {
            greet.api_version
        };
        let detected = ApiVersion::from_version_string(&raw);
        self.version = detected;
        self.server_version_text = raw.clone();
        Ok((detected, raw))
    }

    /// A lightweight "is the server reachable" probe: request greet with a short timeout;
    /// any HTTP response counts as reachable (business codes are each endpoint's own concern, not this one).
    /// The status bar's connection mark is maintained by a dedicated probe thread on this verdict — it does not depend on the sign-in state or business requests,
    /// so signing out, a server restart, or an occasional business timeout never skew the connection state.
    pub fn probe_reachable(&self) -> bool {
        let url = format!("{}/greet", self.base_url);
        self.client
            .get(&url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .map(|response| response.status().as_u16() != 0)
            .unwrap_or(false)
    }

    /// The raw server version string from the last successful probe; empty when no probe succeeded (the interface shows or hides it accordingly)
    pub fn server_version_text(&self) -> &str {
        &self.server_version_text
    }

    /// Forget the recorded server version (called when the address changes so the interface stops showing the old server's version)
    pub fn clear_server_version(&mut self) {
        self.server_version_text.clear();
    }

    /// Get the current base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Set the base URL (used when user changes server address)
    pub fn set_base_url(&mut self, url: &str) {
        self.base_url = url.trim_end_matches('/').to_string();
    }

    /// Build request headers with optional auth
    fn headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(token) = &self.token {
            let auth_value = HeaderValue::from_str(&authorization_value(token))
                .map_err(|e| ConnectorError::InvalidResponse(format!("Invalid token: {}", e)))?;
            headers.insert(AUTHORIZATION, auth_value);
        }
        Ok(headers)
    }

    /// Parse API response, handling error codes (the accepted success values follow ApiVersion and are decided here)
    fn parse_response<T: for<'de> Deserialize<'de>>(
        &self,
        response: reqwest::blocking::Response,
    ) -> Result<T> {
        let _status = response.status();
        let api_response: ApiResponse<T> = response.json()?;

        if self
            .version
            .success_codes()
            .contains(&api_response.code.as_str())
        {
            api_response.data.ok_or_else(|| {
                ConnectorError::InvalidResponse("Missing data in successful response".to_string())
            })
        } else {
            Err(ConnectorError::Api {
                error_code: api_response.code,
                message: api_response.message,
            })
        }
    }

    /// Parse greet response (raw JSON, not wrapped)
    fn parse_greet(response: reqwest::blocking::Response) -> Result<GreetData> {
        let greet: GreetData = response.json()?;
        Ok(greet)
    }

    /// GET /greet - Verify Baihua server and get version info
    pub fn greet(&self) -> Result<GreetData> {
        let url = format!("{}/greet", self.base_url);
        let response = self.client.get(&url).send()?;
        Self::parse_greet(response)
    }

    /// POST /api/v1/user/register - Register new user
    pub fn register(&self, req: RegisterRequest) -> Result<UserData> {
        let url = format!("{}/api/v1/user/register", self.base_url);
        let response = self.client.post(&url).json(&req).send()?;
        self.parse_response(response)
    }

    /// POST /api/v1/user/login - Login and get JWT token
    pub fn login(&self, req: LoginRequest) -> Result<LoginData> {
        let url = format!("{}/api/v1/user/login", self.base_url);
        let response = self.client.post(&url).json(&req).send()?;
        self.parse_response(response)
    }

    /// POST /api/v1/chat/rooms - Create chat room (private or group)
    pub fn create_room(&self, req: CreateRoomRequest) -> Result<RoomInfo> {
        let url = format!("{}/api/v1/chat/rooms", self.base_url);
        let headers = self.headers()?;
        let response = self.client.post(&url).headers(headers).json(&req).send()?;
        self.parse_response(response)
    }

    /// GET /api/v1/chat/rooms - List user's rooms with last message preview
    pub fn list_rooms(&self) -> Result<Vec<RoomInfo>> {
        let url = format!("{}/api/v1/chat/rooms", self.base_url);
        let headers = self.headers()?;
        let response = self.client.get(&url).headers(headers).send()?;

        #[derive(Deserialize)]
        struct RoomsWrapper {
            rooms: Vec<RoomInfo>,
        }
        let wrapper: RoomsWrapper = self.parse_response(response)?;
        Ok(wrapper.rooms)
    }

    /// GET /api/v1/chat/rooms/{room_id} - Get room detail with member info
    pub fn get_room(&self, room_id: &str) -> Result<RoomDetail> {
        let url = format!("{}/api/v1/chat/rooms/{}", self.base_url, room_id);
        let headers = self.headers()?;
        let response = self.client.get(&url).headers(headers).send()?;
        self.parse_response(response)
    }

    /// POST /api/v1/chat/rooms/{room_id}/members - Add members (group, admin only)
    pub fn add_members(&self, room_id: &str, usernames: &[String]) -> Result<AddMembersData> {
        let url = format!("{}/api/v1/chat/rooms/{}/members", self.base_url, room_id);
        let headers = self.headers()?;
        let req = AddMembersRequest {
            usernames: usernames.to_vec(),
        };
        let response = self.client.post(&url).headers(headers).json(&req).send()?;
        self.parse_response(response)
    }

    /// GET /api/v1/chat/rooms/{room_id}/members - List members with roles
    pub fn list_members(&self, room_id: &str) -> Result<MembersData> {
        let url = format!("{}/api/v1/chat/rooms/{}/members", self.base_url, room_id);
        let headers = self.headers()?;
        let response = self.client.get(&url).headers(headers).send()?;
        self.parse_response(response)
    }

    /// DELETE /api/v1/chat/rooms/{room_id}/members/{user_id} - Remove member / leave
    pub fn remove_member(&self, room_id: &str, user_id: &str) -> Result<RemoveMemberData> {
        let url = format!(
            "{}/api/v1/chat/rooms/{}/members/{}",
            self.base_url, room_id, user_id
        );
        let headers = self.headers()?;
        let response = self.client.delete(&url).headers(headers).send()?;
        self.parse_response(response)
    }

    /// GET /api/v1/chat/rooms/{room_id}/messages - Get messages with cursor pagination
    pub fn get_messages(
        &self,
        room_id: &str,
        limit: u32,
        before: Option<&str>,
    ) -> Result<MessagesData> {
        let mut url = format!(
            "{}/api/v1/chat/rooms/{}/messages?limit={}",
            self.base_url, room_id, limit
        );
        if let Some(cursor) = before {
            url.push_str(&format!("&before={}", encode_query_component(cursor)));
        }
        let headers = self.headers()?;
        let response = self.client.get(&url).headers(headers).send()?;
        self.parse_response(response)
    }

    /// GET /api/v1/user/search?username= - fuzzy search active users by username
    pub fn search_users(&self, username: &str) -> Result<Vec<UserSearchResult>> {
        Ok(self.search_users_page(username, 0, 50)?.users)
    }

    /// Fetch one page of user search results for a keyword (the server caps pages at 50 and silently truncates beyond)
    fn search_users_page(&self, username: &str, offset: u32, limit: u32) -> Result<UserSearchPage> {
        let url = format!(
            "{}/api/v1/user/search?username={}&limit={}&offset={}",
            self.base_url,
            encode_query_component(username),
            limit,
            offset
        );
        let headers = self.headers()?;
        let response = self.client.get(&url).headers(headers).send()?;
        self.parse_response(response)
    }

    /// Fetch the directory of all registered users. The path and the paging are decided by ApiVersion:
    /// 0.1.4 has no `/user/list` (the route would read "list" as a username under `/user/{user}`),
    /// so the only option is aggregating `/user/search` pages with an empty keyword, matching every active user via `ILIKE '%%'`.
    pub fn list_all_users(&self) -> Result<Vec<UserSearchResult>> {
        let endpoint = self.version.user_directory_endpoint();
        let url = format!("{}{}", self.base_url, endpoint);
        let headers = self.headers()?;
        if !self.version.user_directory_requires_paging() {
            let response = self.client.get(&url).headers(headers).send()?;

            #[derive(Deserialize)]
            struct UsersWrapper {
                users: Vec<UserInfo>,
            }
            let wrapper: UsersWrapper = self.parse_response(response)?;
            return Ok(wrapper
                .users
                .into_iter()
                .map(UserSearchResult::from)
                .collect());
        }
        // Page aggregation: stop at count, take full pages; when the server ignores offset, an empty page ends the loop
        let mut directory: Vec<UserSearchResult> = Vec::new();
        let mut offset: u32 = 0;
        loop {
            let page = self.search_users_page("", offset, 50)?;
            if page.users.is_empty() {
                break;
            }
            let page_size = page.users.len() as u32;
            directory.extend(page.users);
            offset += page_size;
            if directory.len() as i64 >= page.count || offset >= 2000 {
                break;
            }
        }
        Ok(directory)
    }

    /// GET /api/v1/user/{user} - read any user's public profile (username or UID both work, same shape back)
    pub fn get_user_profile(&self, user_key: &str) -> Result<PublicProfile> {
        let url = format!(
            "{}/api/v1/user/{}",
            self.base_url,
            encode_query_component(user_key)
        );
        let headers = self.headers()?;
        let response = self.client.get(&url).headers(headers).send()?;

        #[derive(Deserialize)]
        struct ProfileWrapper {
            user: PublicProfile,
        }
        Ok(self.parse_response::<ProfileWrapper>(response)?.user)
    }

    /// PATCH /api/v1/user/me - update one's own profile; on success the persisted full user comes back
    pub fn update_profile(&self, payload: &ProfileUpdatePayload) -> Result<UserInfo> {
        let url = format!("{}/api/v1/user/me", self.base_url);
        let headers = self.headers()?;
        let response = self
            .client
            .patch(&url)
            .headers(headers)
            .json(payload)
            .send()?;

        #[derive(Deserialize)]
        struct UserWrapper {
            user: UserInfo,
        }
        Ok(self.parse_response::<UserWrapper>(response)?.user)
    }

    /// PATCH /api/v1/user/me/password - change the password.
    /// The server bumps token_version, instantly invalidating every JWT it issued before (including this client's session),
    /// so after a success the client must drop its stored session and demand a new login.
    pub fn change_password(&self, old_password: &str, new_password: &str) -> Result<String> {
        let url = format!("{}/api/v1/user/me/password", self.base_url);
        let headers = self.headers()?;
        let payload = ChangePasswordRequest {
            old_password: old_password.to_string(),
            new_password: new_password.to_string(),
        };
        let response = self
            .client
            .patch(&url)
            .headers(headers)
            .json(&payload)
            .send()?;
        self.parse_message_response(response)
    }

    /// DELETE /api/v1/user/me - delete the account (the server re-checks the password then hard-deletes; memberships and chat requests cascade)
    pub fn delete_account(&self, password: &str) -> Result<String> {
        let url = format!("{}/api/v1/user/me", self.base_url);
        let headers = self.headers()?;
        let payload = DeleteAccountRequest {
            password: password.to_string(),
        };
        let response = self
            .client
            .delete(&url)
            .headers(headers)
            .json(&payload)
            .send()?;
        self.parse_message_response(response)
    }

    /// POST /api/v1/user/me/avatar - upload an avatar image (multipart field name file).
    /// The server accepts only JPEG/PNG/GIF/WebP and on success points the avatar field at /static/avatars/{filename}
    pub fn upload_avatar(
        &self,
        file_name: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<UserInfo> {
        let url = format!("{}/api/v1/user/me/avatar", self.base_url);
        // The blocking client's multipart Part only exposes headers(), so the image type travels in a CONTENT_TYPE header
        let mut part_headers = HeaderMap::new();
        part_headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str(content_type).map_err(|e| {
                ConnectorError::InvalidResponse(format!("Invalid content type: {e}"))
            })?,
        );
        let part = reqwest::blocking::multipart::Part::bytes(bytes)
            .file_name(file_name.to_string())
            .headers(part_headers);
        let form = reqwest::blocking::multipart::Form::new().part("file", part);
        let mut headers = self.headers()?;
        // reqwest generates the multipart boundary, which must be handed back for the server to parse instead of a fixed JSON header
        headers.remove(CONTENT_TYPE);
        let response = self
            .client
            .post(&url)
            .headers(headers)
            .multipart(form)
            .send()?;

        #[derive(Deserialize)]
        struct UserWrapper {
            user: UserInfo,
        }
        Ok(self.parse_response::<UserWrapper>(response)?.user)
    }

    /// Fetch a public static resource such as an avatar (the server skips token checks here; the image path is a plain GET).
    /// A server-written avatar value may be a `/static/avatars/...` relative path, so it is completed against base_url
    pub fn fetch_static_resource(&self, resource_path: &str) -> Result<Vec<u8>> {
        let url = if resource_path.starts_with("http://") || resource_path.starts_with("https://") {
            resource_path.to_string()
        } else {
            format!("{}{}", self.base_url, resource_path)
        };
        let response = self.client.get(&url).send()?;
        let status = response.status();
        if !status.is_success() {
            return Err(ConnectorError::InvalidResponse(format!(
                "failed to fetch the static resource: HTTP {status}"
            )));
        }
        Ok(response.bytes()?.to_vec())
    }

    /// Parse responses whose data is always null (password change, account deletion), returning the server's message on success
    fn parse_message_response(&self, response: reqwest::blocking::Response) -> Result<String> {
        let api_response: ApiResponse<serde_json::Value> = response.json()?;
        if self
            .version
            .success_codes()
            .contains(&api_response.code.as_str())
        {
            Ok(api_response.message)
        } else {
            Err(ConnectorError::Api {
                error_code: api_response.code,
                message: api_response.message,
            })
        }
    }

    /// POST /api/v1/chat/rooms/requests - send a chat request (the only way to open a private chat on 0.1.4)
    pub fn create_room_request(
        &self,
        receiver_id: &str,
        message: &str,
        is_encrypted: bool,
    ) -> Result<RoomRequestStatusResult> {
        let url = format!("{}/api/v1/chat/rooms/requests", self.base_url);
        let headers = self.headers()?;
        let payload = CreateRoomRequestPayload {
            receiver_id: receiver_id.to_string(),
            is_encrypted,
            message: message.to_string(),
        };
        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&payload)
            .send()?;
        self.parse_response(response)
    }

    /// GET /api/v1/chat/rooms/requests/pending - the chat requests awaiting the current user
    pub fn list_pending_requests(&self) -> Result<Vec<RoomRequestInfo>> {
        let url = format!("{}/api/v1/chat/rooms/requests/pending", self.base_url);
        let headers = self.headers()?;
        let response = self.client.get(&url).headers(headers).send()?;

        #[derive(Deserialize)]
        struct RequestsWrapper {
            requests: Vec<RoomRequestInfo>,
        }
        let wrapper: RequestsWrapper = self.parse_response(response)?;
        Ok(wrapper.requests)
    }

    /// GET /api/v1/chat/rooms/requests/sent - the chat requests the current user has sent
    pub fn list_sent_requests(&self) -> Result<Vec<RoomRequestInfo>> {
        let url = format!("{}/api/v1/chat/rooms/requests/sent", self.base_url);
        let headers = self.headers()?;
        let response = self.client.get(&url).headers(headers).send()?;

        #[derive(Deserialize)]
        struct RequestsWrapper {
            requests: Vec<RoomRequestInfo>,
        }
        let wrapper: RequestsWrapper = self.parse_response(response)?;
        Ok(wrapper.requests)
    }

    /// POST /api/v1/chat/rooms/requests/{request_id}/accept - accept a chat request; the server then creates the private room
    pub fn accept_room_request(&self, request_id: &str) -> Result<AcceptedRoomRequest> {
        let url = format!(
            "{}/api/v1/chat/rooms/requests/{}/accept",
            self.base_url, request_id
        );
        let headers = self.headers()?;
        let response = self.client.post(&url).headers(headers).send()?;
        self.parse_response(response)
    }

    /// POST /api/v1/chat/rooms/requests/{request_id}/decline - decline a chat request
    pub fn decline_room_request(&self, request_id: &str) -> Result<RoomRequestStatusResult> {
        let url = format!(
            "{}/api/v1/chat/rooms/requests/{}/decline",
            self.base_url, request_id
        );
        let headers = self.headers()?;
        let response = self.client.post(&url).headers(headers).send()?;
        self.parse_response(response)
    }

    /// POST /api/v1/chat/rooms/requests/{request_id}/cancel - withdraw a chat request one sent
    pub fn cancel_room_request(&self, request_id: &str) -> Result<RoomRequestStatusResult> {
        let url = format!(
            "{}/api/v1/chat/rooms/requests/{}/cancel",
            self.base_url, request_id
        );
        let headers = self.headers()?;
        let response = self.client.post(&url).headers(headers).send()?;
        self.parse_response(response)
    }
}

/// Percent-encode a query parameter keeping only the RFC 3986 unreserved characters
fn encode_query_component(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

impl Default for Connector {
    fn default() -> Self {
        Self::new("http://localhost:2424")
    }
}

// ======================= WebSocket wire protocol seam (inbound) =======================
// This section is the only translation layer between server WebSocket push messages and the client domain event PollingEvent.
// Wire facts (event type strings, field names, error texts) live only here; the business layer (app.rs) sees domain events.
// When a future version renames or reshapes anything, extend the ApiVersion matches here; app.rs stays unaware.

/// Domain events the background threads (polling / WebSocket) send to the main loop. Variant names are decoupled from wire type strings.
#[derive(Debug, Clone)]
pub enum PollingEvent {
    /// The refreshed room list
    RoomsUpdated(Vec<RoomInfo>),
    /// The refreshed list of pending chat requests (received from others)
    PendingRequestsUpdated(Vec<RoomRequestInfo>),
    /// The list of chat requests one sent (shown on the private-chat management page and used for withdrawal)
    SentRequestsUpdated(Vec<RoomRequestInfo>),
    /// The server confirmed a message one sent
    MessageSent(MessageInfo),
    /// A new message from another member (pushed in real time)
    IncomingMessage(MessageInfo),
    /// An encryption-session invitation from the peer
    EncryptInvitation(EncryptHandshakeData),
    /// The peer's acceptance of an encryption-session invitation
    EncryptAccepted(EncryptHandshakeData),
    /// Both sides ready, the encryption session activates (room id)
    EncryptSessionReady(String),
    /// An encrypted message arrived (ciphertext)
    EncryptedMessage(EncryptedMessageInfo),
    /// The server confirmed an encrypted message one sent (message id receipt)
    EncryptedMessageSent(String),
    /// The encryption session ended (room id, reason)
    EncryptSessionEnded((String, String)),
    /// A member is typing (room id, user id, username). The server only ever sends frames with typing true,
    /// never a "stopped typing" signal, so receivers decay the indicator on a local timeout window
    MemberTyping((String, String, String)),
    /// A member's online state flipped (user id, username, online). The server broadcasts only on connection-count 0-to-1 flips
    /// and sends no baseline roster when a connection opens, so clients can only accumulate the flips
    PresenceChanged((String, String, bool)),
    /// A state change of this client's own WebSocket (given as a text key). It is not a server error
    /// and must stay apart from Error: the Error branch matches localized text against the server's English error strings,
    /// a match that can never happen, so every transient connection blip would pop an error box before self-healing (the "mystery error that fixed itself" symptom).
    /// The keys error_ws_disconnected_reconnect / error_ws_connect_failed are self-healing blips: debug log only;
    /// error_ws_send_failed means one message genuinely did not leave and the user must be told.
    WebSocketState(String),
    /// The server reachability flipped (true means reachable again). Polling runs every two seconds, so one outage retriggers endlessly;
    /// handled as a plain Error that would stack identical popups, so only transitions are reported and the interface warns once,
    /// after which the status-bar connection mark carries the state.
    ReachabilityChanged(bool),
    /// An avatar was fetched (user id, image bytes; None means no avatar or a failed fetch).
    /// The profile endpoint only gives a path; a background thread pulls the bytes, the interface decodes and caches on demand, and the render path never touches the network.
    /// A failed fetch is still reported once so the interface marks "already tried" and stops re-requesting for the same user.
    AvatarLoaded((String, Option<Vec<u8>>)),
    /// The desktop avatar file picker finished: Some is a chosen image path, None a
    /// cancel. It rides the same channel as the network threads so the dialog never blocks a frame.
    AvatarFileChosen(Option<std::path::PathBuf>),
    /// The full registered-user directory arrived (used by /profile autocompletion). The pull starts only the moment the user types
    /// "/profile ", runs exactly once, and the main thread caches the result; the render path never issues requests.
    RegisteredUsersUpdated(Vec<UserSearchResult>),
    /// The release feed offered a newer package for this platform. The interface prompts the
    /// user first and downloads only after the answer; nothing is fetched without consent.
    UpdateAvailable(crate::update::ReleasePackage),
    /// The feed has nothing newer than the running version (the version that was checked).
    UpdateUpToDate(String),
    /// A newer package was downloaded and verified (new version, local package path). The
    /// interface installs it right away and closes itself for the detached installer.
    UpdateReady((String, std::path::PathBuf)),
    /// The WebSocket is ready: the server completed room subscriptions, handshakes and messages can be sent safely
    WebSocketConnected,
    /// The exit cleanup finished in the background and the application may quit safely now
    QuitCleanupFinished,
    /// A polling or connection error
    Error(String),
}

/// Encryption handshake data (shared by invitation and acceptance; peer is the counterparty)
#[derive(Debug, Clone)]
pub struct EncryptHandshakeData {
    pub room_id: String,
    pub peer_id: String,
    pub public_key: String,
    pub identity_key: String,
    pub signature: String,
}

/// Encrypted message payload (ciphertext, no plaintext before decryption)
#[derive(Debug, Clone)]
pub struct EncryptedMessageInfo {
    pub id: String,
    pub room_id: String,
    pub sender_id: String,
    pub ciphertext: String,
    pub created_at: String,
}

/// Parse a server-pushed WebSocket text message into a domain event; unrecognized content yields None
pub fn parse_websocket_event(text: &str, tr: &dyn Fn(&str) -> String) -> Option<PollingEvent> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let event_type = value.get("type")?.as_str()?;
    let data = value.get("data")?;
    match event_type {
        "message_sent" => parse_message_info(data).map(PollingEvent::MessageSent),
        "new_message" => parse_message_info(data).map(PollingEvent::IncomingMessage),
        "encrypt_invitation" => {
            parse_handshake_data(data, "inviter_id").map(PollingEvent::EncryptInvitation)
        }
        "encrypt_accept_response" => {
            parse_handshake_data(data, "acceptor_id").map(PollingEvent::EncryptAccepted)
        }
        "encrypt_session_ready" => Some(PollingEvent::EncryptSessionReady(
            data.get("room_id")?.as_str()?.to_string(),
        )),
        "new_encrypted_message" => Some(PollingEvent::EncryptedMessage(EncryptedMessageInfo {
            id: data.get("id")?.as_str()?.to_string(),
            room_id: data.get("room_id")?.as_str()?.to_string(),
            sender_id: data.get("sender_id")?.as_str()?.to_string(),
            ciphertext: data.get("ciphertext")?.as_str()?.to_string(),
            created_at: data.get("created_at")?.as_str()?.to_string(),
        })),
        "encrypted_message_sent" => Some(PollingEvent::EncryptedMessageSent(
            data.get("id")?.as_str()?.to_string(),
        )),
        "encrypt_session_ended" => Some(PollingEvent::EncryptSessionEnded((
            data.get("room_id")?.as_str()?.to_string(),
            data.get("reason")
                .and_then(|reason| reason.as_str())
                .unwrap_or("unknown")
                .to_string(),
        ))),
        "encrypt_partner_disconnected" => {
            Some(PollingEvent::Error(tr("warning_partner_disconnected")))
        }
        // Connection receipt: only after it does the server deliver later broadcasts to this connection
        "connected" => Some(PollingEvent::WebSocketConnected),
        // Member online flips: the server broadcasts once per 0-to-1 / 1-to-0 connection-count change
        "user_online" => parse_presence(data, true),
        "user_offline" => parse_presence(data, false),
        // Typing state: the server only broadcasts typing-true frames; receivers decay locally
        "typing" => Some(PollingEvent::MemberTyping((
            data.get("room_id")?.as_str()?.to_string(),
            data.get("user_id")?.as_str()?.to_string(),
            data.get("username")?.as_str()?.to_string(),
        ))),
        "encrypt_session_expired" => Some(PollingEvent::Error(tr("warning_session_expired"))),
        "error" => {
            let server_message = data
                .get("message")
                .and_then(|message| message.as_str())
                .map(|message| message.to_string());
            Some(PollingEvent::Error(
                server_message.unwrap_or_else(|| tr("error_server_unknown")),
            ))
        }
        _ => None,
    }
}

/// Parse an online-state flip broadcast (user_online / user_offline).
/// user_id is required; username is optional and falls back to an empty string — a single optional field must never discard the whole state change.
fn parse_presence(data: &serde_json::Value, is_online: bool) -> Option<PollingEvent> {
    Some(PollingEvent::PresenceChanged((
        data.get("user_id")?.as_str()?.to_string(),
        data.get("username")
            .and_then(|username| username.as_str())
            .unwrap_or_default()
            .to_string(),
        is_online,
    )))
}

/// Parse encryption handshake data from JSON; peer_id_field names the field carrying the peer user id
fn parse_handshake_data(
    data: &serde_json::Value,
    peer_id_field: &str,
) -> Option<EncryptHandshakeData> {
    Some(EncryptHandshakeData {
        room_id: data.get("room_id")?.as_str()?.to_string(),
        peer_id: data.get(peer_id_field)?.as_str()?.to_string(),
        public_key: data.get("public_key")?.as_str()?.to_string(),
        identity_key: data.get("identity_key")?.as_str()?.to_string(),
        signature: data.get("signature")?.as_str()?.to_string(),
    })
}

/// Parse a message object from JSON. content is optional (null for encrypted-room history)
/// and is accepted as an empty string exactly like the HTTP DTO (the interface fills an empty body with a localized placeholder); a missing field must never drop the whole message
fn parse_message_info(data: &serde_json::Value) -> Option<MessageInfo> {
    Some(MessageInfo {
        id: data.get("id")?.as_str()?.to_string(),
        room_id: data.get("room_id")?.as_str()?.to_string(),
        sender_id: data.get("sender_id")?.as_str()?.to_string(),
        content: data
            .get("content")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
            .unwrap_or_default(),
        created_at: data.get("created_at")?.as_str()?.to_string(),
    })
}

// ======================= WebSocket wire protocol seam (outbound) =======================
// All client-to-server WebSocket frames (message sends, every handshake stage) are built here, in one place.
// The domain command WsCommand stays decoupled from the wire "type" and field layout; type names match on ApiVersion,
// so a future rename only touches outbound_type and outbound_ws_payload in this section.

/// Client-to-server upstream commands (domain semantics). Fields borrow by reference to avoid ownership tangles with callers.
pub enum WsCommand<'a> {
    SendMessage {
        room_id: &'a str,
        content: &'a str,
    },
    /// Typing report. The server's typing branch reads room_id from the **top level** of the frame (not from data),
    /// so this command's outbound frame carries no data wrapper; see the note at outbound_ws_payload
    SendTyping {
        room_id: &'a str,
    },
    EncryptMessage {
        room_id: &'a str,
        ciphertext: &'a str,
    },
    EncryptRequest {
        room_id: &'a str,
        public_key: &'a str,
        identity_key: &'a str,
        signature: &'a str,
    },
    EncryptAccept {
        room_id: &'a str,
        public_key: &'a str,
        identity_key: &'a str,
        signature: &'a str,
    },
    EncryptReady {
        room_id: &'a str,
    },
    EncryptLeave {
        room_id: &'a str,
    },
}

/// The logical class of an upstream command, used to look up the wire type string per version
enum OutboundKind {
    SendMessage,
    SendTyping,
    EncryptMessage,
    EncryptRequest,
    EncryptAccept,
    EncryptReady,
    EncryptLeave,
}

impl ApiVersion {
    /// The wire "type" string a logical command maps to on this version. The known 0.1.3/0.1.4/unrecognized cases currently agree,
    /// but are listed explicitly so a future per-version rename only touches this spot.
    fn outbound_type(self, kind: OutboundKind) -> &'static str {
        match kind {
            OutboundKind::SendMessage => "send_message",
            OutboundKind::SendTyping => "typing",
            OutboundKind::EncryptMessage => "encrypt_message",
            OutboundKind::EncryptRequest => "encrypt_request",
            OutboundKind::EncryptAccept => "encrypt_accept",
            OutboundKind::EncryptReady => "encrypt_ready",
            OutboundKind::EncryptLeave => "encrypt_leave",
        }
    }
}

/// Serialize a domain upstream command into the server wire frame (type names and data field layouts live in the seam)
pub fn outbound_ws_payload(version: ApiVersion, command: WsCommand) -> serde_json::Value {
    match command {
        WsCommand::SendMessage { room_id, content } => serde_json::json!({
            "type": version.outbound_type(OutboundKind::SendMessage),
            "data": { "room_id": room_id, "content": content },
        }),
        // typing is the only upstream command without a data wrapper: the server's handle_incoming typing branch
        // reads room_id from the top level of the frame; wrapping it in data would be rejected as a missing field
        WsCommand::SendTyping { room_id } => serde_json::json!({
            "type": version.outbound_type(OutboundKind::SendTyping),
            "room_id": room_id,
        }),
        WsCommand::EncryptMessage {
            room_id,
            ciphertext,
        } => serde_json::json!({
            "type": version.outbound_type(OutboundKind::EncryptMessage),
            "data": { "room_id": room_id, "ciphertext": ciphertext },
        }),
        WsCommand::EncryptRequest {
            room_id,
            public_key,
            identity_key,
            signature,
        } => serde_json::json!({
            "type": version.outbound_type(OutboundKind::EncryptRequest),
            "data": { "room_id": room_id, "public_key": public_key, "identity_key": identity_key, "signature": signature },
        }),
        WsCommand::EncryptAccept {
            room_id,
            public_key,
            identity_key,
            signature,
        } => serde_json::json!({
            "type": version.outbound_type(OutboundKind::EncryptAccept),
            "data": { "room_id": room_id, "public_key": public_key, "identity_key": identity_key, "signature": signature },
        }),
        WsCommand::EncryptReady { room_id } => serde_json::json!({
            "type": version.outbound_type(OutboundKind::EncryptReady),
            "data": { "room_id": room_id },
        }),
        WsCommand::EncryptLeave { room_id } => serde_json::json!({
            "type": version.outbound_type(OutboundKind::EncryptLeave),
            "data": { "room_id": room_id },
        }),
    }
}

// ==================== WebSocket connection and keep-alive wire constants (per version) ====================
// HTTP-to-WebSocket address mapping, the /websocket path, the Bearer header format, the application heartbeat frame,
// authentication-failure markers, and the heartbeat / subscription-refresh / handshake-retry timings — all here, matched on ApiVersion.
impl ApiVersion {
    /// Derive the WebSocket address from the HTTP base_url (scheme replacement plus a fixed path).
    /// The path `/websocket` is wire contract; a future change touches only this line.
    pub fn websocket_url(self, base_url: &str) -> String {
        let secured = base_url
            .replace("https://", "wss://")
            .replace("http://", "ws://");
        format!("{secured}/websocket")
    }

    /// The application-level two-way heartbeat frame. The server silently ignores "pong" frames (no reply), keeping traffic flowing in the client-to-server
    /// direction so middleboxes never declare the connection dead from one-way silence.
    pub fn heartbeat_frame(self) -> String {
        serde_json::json!({ "type": "pong" }).to_string()
    }

    /// The signature strings of a server handshake authentication failure (HTTP 401/403 or expired-token texts), making the client drop its session and return to the login page.
    /// Matched lowercase (callers lowercase first, or this table stays all-lowercase fragments; the numeric marker is judged separately).
    fn auth_failure_markers(self) -> &'static [&'static str] {
        &[
            "401",
            "403",
            "unauthorized",
            "forbidden",
            "invalid token",
            "token expired",
        ]
    }

    /// Decide whether a WebSocket connection error text is a server authentication failure (the local session should be cleared)
    pub fn is_auth_failure(self, error_text: &str) -> bool {
        let lowered = error_text.to_lowercase();
        self.auth_failure_markers()
            .iter()
            .any(|marker| lowered.contains(marker))
    }

    /// Application heartbeat send interval (below the server's 30-second protocol Ping, keeping traffic two-way)
    pub fn application_heartbeat_interval(self) -> std::time::Duration {
        std::time::Duration::from_secs(20)
    }

    /// Interval for rebuilding the WebSocket to refresh room subscriptions (the server only snapshots subscriptions at connect time)
    pub fn subscription_refresh_interval(self) -> std::time::Duration {
        std::time::Duration::from_secs(60)
    }

    /// Retry interval for an encryption handshake that never activated
    pub fn handshake_resend_interval(self) -> std::time::Duration {
        std::time::Duration::from_secs(5)
    }

    /// Minimum interval between typing reports. The server rate-limits inbound frames to 30 per 30 seconds, and typing has only the
    /// "is typing" frame, so reports are throttled to 1.5 seconds: below the receiver's 2-second decay window (continuous typing keeps the indicator steady)
    /// and with ample quota left for ordinary messages.
    pub fn typing_send_interval(self) -> std::time::Duration {
        std::time::Duration::from_millis(1500)
    }

    /// The receiver's local decay window for deciding "a member stopped typing" (agreement: typing counts within the last 2 seconds)
    pub fn typing_display_window(self) -> std::time::Duration {
        std::time::Duration::from_secs(2)
    }

    /// Map the server's "session ended" reason (wire string) onto a localized text key, kept central so versions can adjust it
    pub fn session_end_reason_key(self, reason: &str) -> &'static str {
        match reason {
            "user_left" => "notification_partner_ended_session",
            "partner_timeout" => "notification_session_timeout_ended",
            _ => "notification_session_ended",
        }
    }

    /// Whether a "room not found / not a member" error from leaving is expected (silently ignored); the string table is central per version
    pub fn is_ignorable_room_removal_error(self, error_text: &str) -> bool {
        let lowered = error_text.to_lowercase();
        ["not found", "not exist", "not a member", "member"]
            .iter()
            .any(|keyword| lowered.contains(keyword))
    }
}

/// Build the Authorization header value for HTTP/WebSocket requests (the Bearer scheme, wire contract)
pub fn authorization_value(token: &str) -> String {
    format!("Bearer {token}")
}

/// The internal sentinel for a client-side WebSocket authentication failure (invented here, not a server wire string). Sender and matcher both take it from
/// this one place so the two literals can never drift apart.
pub fn websocket_auth_sentinel() -> &'static str {
    "WS_AUTH_FAILED"
}

/// Domain classification of server runtime error texts. The substrings are wire contract, judged centrally per version;
/// app.rs decides behavior from the class alone instead of scattering contains literals.
pub enum ServerSignal {
    /// Hit a stale active encryption session on the server; send encrypt_leave to trigger cleanup
    StuckEncryptedSession,
    /// The peer being offline made the handshake refuse; clear the local session stuck waiting for acceptance
    PartnerOfflineHandshakeRejected,
    /// No active encryption session (/quit batch cleanup may leave rooms that never had one; expected) — ignore silently
    NoActiveEncryptedSession,
    /// Not a member of the room (private-chat exit already sent encrypt_leave first; expected) — ignore silently
    NotRoomMember,
    /// Any other real error; show it to the user
    Displayable,
}

impl ApiVersion {
    /// Classify a server error text into a domain signal (all known versions share the texts today; central for a future per-version fork)
    pub fn classify_server_error(self, error_text: &str) -> ServerSignal {
        let lowered = error_text.to_lowercase();
        if lowered.contains("already has an active encrypted session") {
            ServerSignal::StuckEncryptedSession
        } else if lowered.contains("both users must be online") {
            ServerSignal::PartnerOfflineHandshakeRejected
        } else if lowered.contains("no active encrypted session") {
            ServerSignal::NoActiveEncryptedSession
        } else if lowered.contains("not a member of this room") {
            ServerSignal::NotRoomMember
        } else {
            ServerSignal::Displayable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use tungstenite::Message as WebSocketMessage;
    use tungstenite::client::IntoClientRequest;

    /// After an account is deleted the server returns rooms.created_by and messages.sender_id as null
    /// (both foreign keys are `ON DELETE SET NULL`). Decoding these fields as String,
    /// one failure would sink the whole room list or a whole history page, so the seam must catch them.
    #[test]
    fn null_user_references_still_decode_the_whole_payload() {
        let rooms: Vec<RoomInfo> = serde_json::from_str(
            r#"[{"id":"room-1","name":null,"created_by":null,"created_at":"2026-09-05T00:00:00Z","is_group":true,"is_encrypted":false,"member_count":2,"role":"member","members":["user-a"]},{"id":"room-2","name":"team chat","created_at":"2026-09-05T00:00:00Z","is_group":true,"is_encrypted":false,"member_count":1,"role":"member","members":["user-a"]}]"#,
        )
        .expect("neither a null nor a missing created_by may break the room list decode");
        assert_eq!(rooms.len(), 2);
        assert_eq!(rooms[0].created_by, "");
        assert_eq!(rooms[1].created_by, "");

        let history: MessagesData = serde_json::from_str(
            r#"{"messages":[{"id":"msg-1","room_id":"room-1","sender_id":null,"content":"hello","created_at":"2026-09-05T00:00:00Z"},{"id":"msg-2","room_id":"room-1","sender_id":"user-a","content":null,"created_at":"2026-09-05T00:01:00Z"},{"id":"msg-3","room_id":"room-1","sender_id":"user-a","created_at":"2026-09-05T00:02:00Z"}],"has_more":false,"next_cursor":null}"#,
        )
        .expect("null sender_id and null content must not break the whole message page decode");
        assert_eq!(history.messages.len(), 3);
        assert_eq!(history.messages[0].sender_id, "");
        assert_eq!(history.messages[0].content, "hello");
        // In encrypted rooms the server gives null body (the ciphertext lives in encrypted_content): empty content is this seam's marker,
        // not user-facing text; the interfaces fill in the placeholder from their own language tables
        assert_eq!(history.messages[1].content, "");
        assert_eq!(history.messages[2].content, "");
    }

    #[test]
    fn test_connector_creation() {
        let connector = Connector::new("http://localhost:2424");
        assert_eq!(connector.base_url(), "http://localhost:2424");
        assert!(connector.token.is_none());
    }

    #[test]
    fn test_connector_with_token() {
        let mut connector = Connector::new("http://localhost:2424");
        connector.set_token("test-token");
        assert_eq!(connector.token, Some("test-token".to_string()));
    }

    #[test]
    fn test_create_room_request_group() {
        let req = CreateRoomRequest::group("Team".to_string(), vec!["bob".to_string()]);
        assert!(req.is_group);
        assert_eq!(req.name, "Team");
        assert_eq!(req.usernames, vec!["bob"]);
    }

    #[test]
    fn test_encode_query_component() {
        assert_eq!(encode_query_component("alice_01"), "alice_01");
        assert_eq!(encode_query_component("a b/c"), "a%20b%2Fc");
        assert_eq!(encode_query_component("héllo"), "h%C3%A9llo");
    }

    /// For diagnostics: two WebSocket connections run a full encryption handshake, printing the event kind at every step,
    /// to locate where encrypt_invitation / accept_response / session_ready breaks
    #[test]
    #[ignore]
    fn live_test_encrypted_handshake_flow() {
        use base64::Engine;
        use base64::engine::general_purpose::STANDARD as BASE64;

        // -- preparation: register two accounts and open an encrypted private room (reusing the request flow) --
        let mut connector = Connector::new("http://localhost:2424");
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let name_a = format!("hs_alice_{suffix}");
        let name_b = format!("hs_bob_{suffix}");
        let encrypted_password = crate::crypto::encrypt_login_password("pass1234");
        for username in [&name_a, &name_b] {
            connector
                .register(RegisterRequest {
                    username: username.clone(),
                    email: format!("{username}@example.com"),
                    password: encrypted_password.clone(),
                })
                .expect("register failed");
        }
        let login_a = connector
            .login(LoginRequest {
                username: name_a.clone(),
                password: encrypted_password.clone(),
            })
            .expect("login a failed");
        connector.set_token(&login_a.token);
        let results = connector.search_users(&name_b).expect("search failed");
        let partner_id = results[0].id.clone();
        connector
            .create_room_request(&partner_id, "handshake diagnosis", true)
            .expect("create request failed");
        let login_b = connector
            .login(LoginRequest {
                username: name_b.clone(),
                password: encrypted_password.clone(),
            })
            .expect("login b failed");
        connector.set_token(&login_b.token);
        let pending = connector.list_pending_requests().expect("pending failed");
        let request_id = pending[0].id.clone();
        let accepted = connector
            .accept_room_request(&request_id)
            .expect("accept failed");
        let room_id = accepted.room.id.clone();
        println!("ROOM: {room_id}");

        // -- both connections established --
        let build_request = |token: &str| {
            let mut request = "ws://localhost:2424/websocket"
                .to_string()
                .into_client_request()
                .expect("ws request");
            request.headers_mut().insert(
                "Authorization",
                HeaderValue::from_str(&format!("Bearer {token}")).expect("header"),
            );
            request
        };
        let (mut socket_a, _) =
            tungstenite::connect(build_request(&login_a.token)).expect("connect a");
        let (mut socket_b, _) =
            tungstenite::connect(build_request(&login_b.token)).expect("connect b");
        if let tungstenite::stream::MaybeTlsStream::Plain(stream) = socket_a.get_ref() {
            let _ = stream.set_nonblocking(true);
        }
        if let tungstenite::stream::MaybeTlsStream::Plain(stream) = socket_b.get_ref() {
            let _ = stream.set_nonblocking(true);
        }

        // Collect the sequence of event kinds each side receives within the time limit
        fn drain(
            socket: &mut tungstenite::WebSocket<
                tungstenite::stream::MaybeTlsStream<std::net::TcpStream>,
            >,
            milliseconds: u64,
        ) -> Vec<String> {
            let deadline = std::time::Instant::now() + Duration::from_millis(milliseconds);
            let mut event_types = Vec::new();
            while std::time::Instant::now() < deadline {
                if let Ok(WebSocketMessage::Text(text)) = socket.read()
                    && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
                {
                    event_types.push(value["type"].as_str().unwrap_or("?").to_string());
                }
                thread::sleep(Duration::from_millis(10));
            }
            event_types
        }
        let write_json = |socket: &mut tungstenite::WebSocket<
            tungstenite::stream::MaybeTlsStream<std::net::TcpStream>,
        >,
                          value: serde_json::Value| {
            socket
                .write(WebSocketMessage::text(value.to_string()))
                .expect("ws write");
            let _ = socket.flush();
        };

        println!(
            "STEP1 connected: A={:?} B={:?}",
            drain(&mut socket_a, 600),
            drain(&mut socket_b, 600)
        );

        // A sends encrypt_request (placeholder keys; only verifying server routing)
        write_json(
            &mut socket_a,
            serde_json::json!({
                "type": "encrypt_request",
                "data": { "room_id": room_id, "public_key": "AAA=", "identity_key": "BBB=", "signature": "CCC=" }
            }),
        );
        println!(
            "STEP2 after request_A: A={:?} B={:?}",
            drain(&mut socket_a, 900),
            drain(&mut socket_b, 900)
        );

        // B answers accept + ready
        write_json(
            &mut socket_b,
            serde_json::json!({
                "type": "encrypt_accept",
                "data": { "room_id": room_id, "public_key": "DDD=", "identity_key": "EEE=", "signature": "FFF=" }
            }),
        );
        write_json(
            &mut socket_b,
            serde_json::json!({
                "type": "encrypt_ready",
                "data": { "room_id": room_id }
            }),
        );
        println!(
            "STEP3 after accept_B+ready_B: A={:?} B={:?}",
            drain(&mut socket_a, 900),
            drain(&mut socket_b, 900)
        );

        // A answers ready -- both ready should trigger session_ready
        write_json(
            &mut socket_a,
            serde_json::json!({
                "type": "encrypt_ready",
                "data": { "room_id": room_id }
            }),
        );
        println!(
            "STEP4 after ready_A: A={:?} B={:?}",
            drain(&mut socket_a, 900),
            drain(&mut socket_b, 900)
        );

        // A sends an encrypted message (placeholder ciphertext) -- both sides should receive new_encrypted_message
        write_json(
            &mut socket_a,
            serde_json::json!({
                "type": "encrypt_message",
                "data": { "room_id": room_id, "ciphertext": BASE64.encode(b"payload") }
            }),
        );
        println!(
            "STEP5 after message_A: A={:?} B={:?}",
            drain(&mut socket_a, 900),
            drain(&mut socket_b, 900)
        );
    }

    #[test]
    #[ignore]
    fn live_test_private_chat_flow() {
        // The 0.1.4 flow: search the user, send a chat request, the peer accepts, the room is created
        let mut connector = Connector::new("http://localhost:2424");
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let initiator_name = format!("live_initiator_{suffix}");
        let partner_name = format!("live_partner_{suffix}");
        let encrypted_password = crate::crypto::encrypt_login_password("pass1234");
        connector
            .register(RegisterRequest {
                username: initiator_name.clone(),
                email: format!("{initiator_name}@example.com"),
                password: encrypted_password.clone(),
            })
            .expect("register initiator failed");
        connector
            .register(RegisterRequest {
                username: partner_name.clone(),
                email: format!("{partner_name}@example.com"),
                password: encrypted_password.clone(),
            })
            .expect("register partner failed");

        // The initiator signs in (the encrypted password travels back, proving deterministic encryption can log in)
        let initiator_login = connector
            .login(LoginRequest {
                username: initiator_name.clone(),
                password: encrypted_password.clone(),
            })
            .expect("initiator login with encrypted password failed");

        // Search for the peer to obtain the user_id
        connector.set_token(&initiator_login.token);
        let search_results = connector
            .search_users(&partner_name)
            .expect("search failed");
        let partner_id = search_results
            .iter()
            .find(|user| user.username == partner_name)
            .expect("partner not found in search")
            .id
            .clone();

        // Send the chat request
        connector
            .create_room_request(&partner_id, "open a private chat", false)
            .expect("create room request failed");

        // The peer signs in, reads the pending request and accepts it
        let partner_login = connector
            .login(LoginRequest {
                username: partner_name.clone(),
                password: encrypted_password.clone(),
            })
            .expect("partner login failed");
        connector.set_token(&partner_login.token);
        let pending = connector
            .list_pending_requests()
            .expect("list pending failed");
        let request_id = pending
            .iter()
            .find(|request| {
                request
                    .sender
                    .as_ref()
                    .is_some_and(|sender| sender.username == initiator_name)
            })
            .expect("pending request not found")
            .id
            .clone();
        let accepted = connector
            .accept_room_request(&request_id)
            .expect("accept failed");
        assert!(!accepted.room.is_group);

        // Both sides' room lists must now show the room
        let partner_rooms = connector.list_rooms().expect("partner list rooms failed");
        assert_eq!(partner_rooms.len(), 1);
        connector.set_token(&initiator_login.token);
        let initiator_rooms = connector.list_rooms().expect("initiator list rooms failed");
        assert_eq!(initiator_rooms.len(), 1);
        println!("FLOW_OK room: {:?}", initiator_rooms[0].id);
    }
}
