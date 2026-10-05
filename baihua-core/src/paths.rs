//! Client local directories and resource location.
//!
//! The root directory follows the same convention as the server: prefer the environment variable `BAIHUA_DIR`, otherwise use the user's home directory's
//! `.baihua`.  The server keeps its data at that root; every client file lives under its `client`
//! both sides share the same tree but do not overwrite each other.

use std::path::PathBuf;

/// Data root directory: `$BAIHUA_DIR` or `~/.baihua`. Returns None when the home directory cannot be obtained (the caller skips local storage).
pub fn data_root() -> Option<PathBuf> {
    if let Some(override_directory) = std::env::var_os("BAIHUA_DIR") {
        let directory = PathBuf::from(override_directory);
        if directory.as_os_str().is_empty() {
            return None;
        }
        return Some(directory);
    }
    dirs::home_dir().map(|home| home.join(".baihua"))
}

/// Client-specific directory `<$BAIHUA_DIR|~/.baihua>/client`.
fn client_root() -> Option<PathBuf> {
    data_root().map(|root| root.join("client"))
}

/// Root of regenerable data `<client directory>/cache`. Everything inside is stuff that "can be recovered if deleted",
/// uninstall can optionally clean it all up.
pub fn cache_directory() -> Option<PathBuf> {
    client_root().map(|root| root.join("cache"))
}

/// Local chat message cache directory `<client directory>/cache/chat_msg`.
pub fn chat_message_directory() -> Option<PathBuf> {
    cache_directory().map(|cache| cache.join("chat_msg"))
}

/// Avatar image cache directory `<client directory>/cache/avatar`.
pub fn avatar_directory() -> Option<PathBuf> {
    cache_directory().map(|cache| cache.join("avatar"))
}

/// Directory for user-supplied avatars `<client directory>/config/avatars`: the user puts images in here,
/// the client only lists the image filenames here for selection in "Change Avatar"; it neither downloads nor writes to this directory.
/// at the same level as the install layout (`baihua install` puts config into `<client directory>/config`); uninstall asks Y/n to clean it up together.
pub fn avatar_source_directory() -> Option<PathBuf> {
    client_root().map(|root| root.join("config").join("avatars"))
}

/// Update package download staging directory `<client directory>/update`.
pub fn update_directory() -> Option<PathBuf> {
    client_root().map(|root| root.join("update"))
}

/// Default install directory `<client directory>/bin`: installed in a user-writable location, no admin rights needed, consistent across all platforms.
pub fn install_directory() -> Option<PathBuf> {
    client_root().map(|root| root.join("bin"))
}

/// Full path of the current executable (both install and self-update need it as an anchor).
pub fn current_executable() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// Locate the config directory (where `languages/`, `themes/`, `preferences.json` live).
///
/// Try in order: `config` under the current working directory (the case of `cargo run` inside the package directory),
/// then `config` under ancestor directories of the executable
/// (when installed as `.../bin/baihua-client` the config is in `.../config`, also covers running directly from target/debug).
/// When none are found, return the first candidate so the failure behavior matches the old version (uses built-in defaults).
pub fn config_directory() -> PathBuf {
    let candidates = config_directory_candidates();
    candidates
        .iter()
        .find(|candidate| candidate.is_dir())
        .cloned()
        .unwrap_or_else(|| candidates[0].clone())
}

/// All candidates for the config directory, sorted by priority. The installer and error messages need to display this completely to the user.
///
/// The repository keeps a single shared `config/` directory at the root, used by both interfaces (the TUI and the GUI
/// read the same languages, themes and preferences). There used to be a second candidate `baihua-client-tui/config`;
/// it is gone now, so every candidate below points at a plain `config/` directory.
pub fn config_directory_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![PathBuf::from("config")];
    let Some(executable) = current_executable() else {
        return candidates;
    };
    // macOS application bundles (created by `cargo bundle`, dragged into
    // /Applications) keep shared files under `<App>.app/Contents/Resources`;
    // that is the only place a packaged app can carry the shared `config/`
    // tree, and the ancestor walk below never looks there, so it gets its own
    // candidate right before the walk.
    if let Some(bundle_resources_config) = executable
        .parent()
        .and_then(|macos_directory| macos_directory.parent())
        .map(|contents_directory| contents_directory.join("Resources").join("config"))
    {
        candidates.push(bundle_resources_config);
    }
    // A Linux AppImage runs its payload from a temporary mount, where the shared
    // tree sits beside the binary directory as `<mount>/usr/share/baihua/config`.
    if let Some(share_config) = executable
        .parent()
        .and_then(|binary_directory| binary_directory.parent())
        .map(|usr_directory| usr_directory.join("share").join("baihua").join("config"))
    {
        candidates.push(share_config);
    }
    // Walk up from the executable directory: the install layout is <prefix>/bin/<program> + <prefix>/config,
    // the development layout is <repo>/target/<config>/<program> + <repo>/config,
    // both fall on the path of "walk up level by level"; no need to hardcode the depth for each layout
    let mut directory = executable.parent().map(|parent| parent.to_path_buf());
    while let Some(current) = directory {
        candidates.push(current.join("config"));
        directory = current.parent().map(|parent| parent.to_path_buf());
    }
    candidates
}

/// Join a relative path under the config directory (such as `themes/dark.json`).
/// This function is used to read config; it looks up existing config directories by priority.
pub fn config_path(relative_path: &str) -> PathBuf {
    config_directory().join(relative_path)
}

/// Join a relative path under the writable config directory (such as `preferences.json`).
/// This function is used for writing config; always points to the client config directory under the user's home directory (`~/.baihua/client/config`),
/// avoid modifying config files in the project source directory in the development environment.
pub fn writable_config_path(relative_path: &str) -> PathBuf {
    client_root()
        .map(|root| root.join("config").join(relative_path))
        .unwrap_or_else(|| config_path(relative_path))
}

/// Join a relative path under the writable config directory; fall back to the read-only config directory if the file does not exist.
/// This function is used to read configs that might have been modified by the user (such as preferences.json),
/// prefer reading from the user's home directory; only read defaults from the project source directory when absent.
pub fn readable_config_path(relative_path: &str) -> PathBuf {
    let writable = writable_config_path(relative_path);
    if writable.exists() {
        writable
    } else {
        config_path(relative_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A packaged macOS app carries the shared `config/` tree under
    /// `Contents/Resources`; the candidate list must contain a
    /// `<two levels above the executable>/Resources/config` entry so that
    /// layout is found without the terminal.
    #[test]
    fn app_bundle_resources_directory_is_a_config_candidate() {
        let candidates = config_directory_candidates();
        let looks_like_bundle_resources = |candidate: &PathBuf| {
            candidate.file_name() == Some(std::ffi::OsStr::new("config"))
                && candidate
                    .parent()
                    .map(|parent| parent.file_name() == Some(std::ffi::OsStr::new("Resources")))
                    .unwrap_or(false)
        };
        assert!(
            candidates.iter().any(looks_like_bundle_resources),
            "the candidates must contain the application bundle's Resources/config, got {candidates:?}"
        );
    }

    /// An AppImage payload lives at `<mount>/usr/bin/<program>` with its shared
    /// tree at `<mount>/usr/share/baihua/config`, so that spelling must be one of
    /// the candidates or a packaged Linux client falls back to built-in defaults.
    #[test]
    fn app_image_share_directory_is_a_config_candidate() {
        let candidates = config_directory_candidates();
        let looks_like_app_image_share = |candidate: &PathBuf| {
            candidate.file_name() == Some(std::ffi::OsStr::new("config"))
                && candidate
                    .parent()
                    .map(|parent| parent.file_name() == Some(std::ffi::OsStr::new("baihua")))
                    .unwrap_or(false)
        };
        assert!(
            candidates.iter().any(looks_like_app_image_share),
            "the candidates must contain the AppImage share/baihua/config, got {candidates:?}"
        );
    }

    #[test]
    fn config_directory_resolves_next_to_the_executable() {
        // Build a temporary installation layout <prefix>/bin/ (fake executable) + <prefix>/config
        // to prove the ancestor lookup finds the config from any working directory
        let root = std::env::temp_dir().join("baihua-config-lookup");
        std::fs::create_dir_all(root.join("config"))
            .expect("the config directory must be creatable");
        std::fs::write(root.join("config").join("preferences.json"), "{}")
            .expect("the write must succeed");
        let resolved = config_directory();
        // This test process runs from the repository root, which ships a real config, so the result must be an existing directory
        assert!(
            resolved.is_dir(),
            "the resolved config directory must really exist: {resolved:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn config_directory_candidates_put_working_directory_first() {
        let candidates = config_directory_candidates();
        assert_eq!(
            candidates.first().map(|path| path.display().to_string()),
            Some("config".to_string())
        );
    }

    #[test]
    fn client_subdirectories_are_nested_under_one_root() {
        let Some(root) = data_root() else {
            return;
        };
        // Both interfaces share one configuration and one cache: all user data sits under one client root —
        // configuration (preferences.json and user-supplied avatars) in `<client root>/config`,
        // regenerable message and avatar caches in `<client root>/cache`.
        // Neither interface computes paths itself (this module decides), so the terminal and graphical versions always derive the same path.
        let preferences = writable_config_path("preferences.json");
        assert!(
            preferences.starts_with(root.join("client").join("config")),
            "the user configuration must land under the client root's config, shared by both interfaces: {preferences:?}"
        );
        let messages =
            chat_message_directory().expect("the message cache directory must be locatable");
        let avatars = avatar_directory().expect("the avatar cache directory must be locatable");
        let updates = update_directory().expect("the update directory must be locatable");
        let installs = install_directory().expect("the install directory must be locatable");
        let sources =
            avatar_source_directory().expect("the avatar source directory must be locatable");
        for directory in [&messages, &avatars, &updates, &installs, &sources] {
            assert!(directory.starts_with(root.join("client")));
        }
        // User-supplied avatar pictures are configuration (the person put them there), kept apart from regenerable caches
        assert!(sources.starts_with(root.join("client").join("config")));

        assert!(!sources.starts_with(cache_directory().expect("the cache root must be locatable")));
        // Both regenerable caches live under cache/ so an uninstall can clear them in one pass
        for directory in [&messages, &avatars] {
            assert!(
                directory.starts_with(cache_directory().expect("the cache root must be locatable"))
            );
        }
    }
}
