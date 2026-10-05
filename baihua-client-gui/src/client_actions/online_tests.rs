/// End-to-end online check: requires this machine running a server, so skipped by default.
/// How to run: cargo test -p baihua-client-gui -- --ignored --nocapture
use super::Client;
use baihua_core::config;
use std::thread::sleep;
use std::time::Duration;

fn server_address() -> String {
    std::env::var("BAIHUA_TEST_SERVER")
        .unwrap_or_else(|_| config::preference_string("server_address", "http://localhost:8080"))
}

/// Build an account used only for this test and log in; return the client with address and session filled in
fn signed_in_client(name: &str, password: &str) -> Client {
    let address = server_address();
    let mut client = Client::default();
    client.connector.set_base_url(&address);
    let email = format!("{name}@example.com");
    assert!(
        client.sign_up(name, &email, password),
        "registering {name} must succeed (duplicate names may register again with this run's own password)"
    );
    assert!(
        client.sign_in(name, password),
        "signing in {name} must succeed"
    );
    client
}

fn pump(client: &mut Client, seconds: f32) {
    let deadline = std::time::Instant::now() + Duration::from_secs_f32(seconds);
    while std::time::Instant::now() < deadline {
        while let Some(event) = client.next_event() {
            client.apply_event(event);
        }
        sleep(Duration::from_millis(100));
    }
}

/// Auto-login only has a token and a user id, so `prepare_session` must fetch the
/// profile by id to fill the top-bar name that a login response would carry.
#[test]
#[ignore = "needs a Baihua server running on this machine"]
fn auto_login_name() {
    let stamp = chrono::Local::now().format("%H%M%S%.3f").to_string();
    let name = format!("guilogin{}", stamp.replace('.', ""));
    let password = "gui-auto-login-pass".to_string();
    let mut signed_in = signed_in_client(&name, &password);
    pump(&mut signed_in, 1.0);

    let token = signed_in
        .websocket_token
        .clone()
        .expect("a token must remain after signing in");
    let user_id = signed_in
        .current_user_id
        .clone()
        .expect("a user id must remain after signing in");

    // The auto-login state: only token and user ID, no username from the login response
    let mut restored = Client::default();
    restored.connector.set_base_url(&server_address());
    restored.connector.set_token(&token);
    restored.current_user_id = Some(user_id);
    assert!(
        restored.current_username.is_empty(),
        "automatic login starts without a name"
    );
    restored.prepare_session(&token);
    assert_eq!(
        restored.current_username, name,
        "after automatic login the username must come back with the user id"
    );
    let (_connection_label, _mark, user_text, _right_text) = restored.status_bar_texts();
    assert!(
        user_text.contains(&name),
        "the status bar's current-user segment must show the freshly restored name, got {user_text:?}"
    );
}

#[test]
#[ignore = "needs a Baihua server running on this machine"]
fn group_round_trip() {
    let stamp = chrono::Local::now().format("%H%M%S%.3f").to_string();
    let name_a = format!("gui{}a", stamp.replace('.', ""));
    let name_b = format!("gui{}b", stamp.replace('.', ""));
    let password = "gui-e2e-pass".to_string();
    let marker = format!("graphical integration message {stamp}");

    let mut sender = signed_in_client(&name_a, &password);
    let mut receiver = signed_in_client(&name_b, &password);
    pump(&mut sender, 1.0);
    pump(&mut receiver, 1.0);

    // A builds a room and brings B in
    sender.create_group("graphical integration group", &name_b);
    pump(&mut sender, 3.0);
    sender.load_rooms_now();
    let rooms = sender.room_entries();
    assert!(
        rooms
            .iter()
            .any(|room| room.title == "graphical integration group"),
        "after A builds the room its own list must contain it, got {:?}",
        rooms
            .iter()
            .map(|room| room.title.clone())
            .collect::<Vec<String>>()
    );
    let index = rooms
        .iter()
        .position(|room| room.title == "graphical integration group")
        .expect("existence asserted just above");
    sender.open_room(index);
    pump(&mut sender, 1.0);

    // A sends a message: goes up via WebSocket
    sender.send_message(&marker);
    pump(&mut sender, 2.0);

    // B pulls history from the server, verifies the message really reached the server and has the sender name
    receiver.load_rooms_now();
    pump(&mut receiver, 2.0);
    let receiver_rooms = receiver.room_entries();
    let position = receiver_rooms
        .iter()
        .position(|room| room.title == "graphical integration group")
        .expect("B should be in the group as well");
    receiver.open_room(position);
    let contents: Vec<String> = receiver
        .messages
        .iter()
        .map(|message| message.content.clone())
        .collect();
    assert!(
        contents.iter().any(|text| text == &marker),
        "the history B pulls must contain that message, got {contents:?}"
    );
    let posted = receiver
        .messages
        .iter()
        .find(|message| message.content == marker)
        .expect("existence asserted just above");
    assert_eq!(
        receiver.sender_display_name(&posted.sender_id),
        name_a,
        "the sender name must resolve through the member table"
    );

    // Cleanup: both accounts logged out, no residue left on the dev server
    sender.delete_account(&password);
    receiver.delete_account(&password);
    assert!(
        !sender.is_signed_in() && !receiver.is_signed_in(),
        "the local session must be void after account deletion"
    );
}
