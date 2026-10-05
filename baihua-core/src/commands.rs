//! Chat command table.
//!
//! Both the TUI and GUI share the same table: command names and their language key names are all here,
//! input completion, command panel, and "unknown command" prompts all read from it, so both sides won't diverge by writing separate versions.
//!
//! This module is "shared code": it only deals with command names and language key names, not recognizing any interface type
//! (no egui/epaint/ratatui), so anyone can use it and it's easy to test separately.

/// Built-in chat command table, entries are (command name, language key of the description); the order is the display order in the interfaces.
///
/// To add a command: append an entry here, then add a branch in each interface's command-execution match.
/// `exit` and `quit` are two spellings of the same action; both are listed so completion offers them.
pub fn chat_commands() -> Vec<(&'static str, &'static str)> {
    vec![
        ("quit", "command_quit"),
        ("exit", "command_quit"),
        ("quit_group", "command_quit_group"),
        ("kick", "command_kick"),
        ("info", "command_info"),
        ("list_users", "command_list_users"),
        ("search_users", "command_search_users"),
        ("profile", "command_profile"),
        ("language", "command_language"),
        ("appearance", "command_appearance"),
        ("update", "command_update"),
        ("logout", "command_logout"),
        ("server_address", "command_server_address"),
        ("add_member", "command_add_member"),
        ("mute", "command_mute"),
        ("login", "command_login"),
        ("register", "command_register"),
    ]
}

/// Filter the completable entries by the command-name prefix typed so far.
/// An empty prefix (only a slash typed) returns the whole table so the person sees what exists.
pub fn command_completions(prefix: &str) -> Vec<(&'static str, &'static str)> {
    chat_commands()
        .into_iter()
        .filter(|(name, _)| name.starts_with(prefix))
        .collect()
}

/// Whether this command takes an argument.
///
/// A command with an argument (like `/kick <username>`) only completes into the input box when clicked,
/// leaving the person to type the argument and press Enter; a command without one runs immediately on click.
/// The arguments of `/profile` and `/mute` are optional, so both count as "no argument": a click runs the most common form first.
pub fn command_takes_argument(name: &str) -> bool {
    matches!(
        name,
        "kick" | "search_users" | "add_member" | "language" | "appearance" | "server_address"
    )
}

/// Whether this command name is a complete command known to the table.
///
/// Tells "the name is fully typed" apart from "still completing": Enter on a full name executes,
/// Enter on a partial one first completes the highlighted entry into the input box (same rule in both interfaces).
pub fn is_known_command(name: &str) -> bool {
    chat_commands().iter().any(|(known, _)| *known == name)
}

/// The part after the command name up to the first space: which command the input box is still typing.
/// Returns None when the text is not a command (no leading slash, or the name is finished and arguments started).
pub fn pending_command_prefix(draft: &str) -> Option<&str> {
    let rest = draft.strip_prefix('/')?;
    if rest.contains(char::is_whitespace) {
        return None;
    }
    Some(rest)
}

/// Commands allowed while signed out (login, register, sign out, and local switches that need no account).
/// Every other command is rejected while signed out with a prompt to log in first.
pub fn allowed_signed_out(name: &str) -> bool {
    matches!(
        name,
        "login"
            | "register"
            | "logout"
            | "quit"
            | "exit"
            | "update"
            | "language"
            | "appearance"
            | "server_address"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table must not repeat a command name, or the completion list would show the same entry twice
    #[test]
    fn command_names_are_unique() {
        let commands = chat_commands();
        assert!(!commands.is_empty(), "the command table must not be empty");
        let mut seen: Vec<&str> = Vec::new();
        for (name, _) in &commands {
            assert!(
                !seen.contains(name),
                "the command table repeats the command {name}"
            );
            assert!(
                !name.starts_with('/') && !name.contains(char::is_whitespace),
                "command names are bare, without a slash or a space, got {name:?}"
            );
            seen.push(name);
        }
    }

    /// Prefix completion filters by prefix; an empty prefix yields everything
    #[test]
    fn completions_filter_by_prefix() {
        assert_eq!(command_completions("").len(), chat_commands().len());
        let filtered = command_completions("li");
        assert_eq!(
            filtered.len(),
            1,
            "exactly one command starts with li, got {filtered:?}"
        );
        assert_eq!(filtered[0].0, "list_users");
        assert!(command_completions("no such command").is_empty());
    }

    /// Completion is pending only while the command name is being typed; once arguments start, it stops
    #[test]
    fn pending_prefix_stops_after_the_command_name() {
        assert_eq!(pending_command_prefix("/ki"), Some("ki"));
        assert_eq!(pending_command_prefix("/"), Some(""));
        assert_eq!(pending_command_prefix("/kick alice"), None);
        assert_eq!(pending_command_prefix("plain message"), None);
    }
}
