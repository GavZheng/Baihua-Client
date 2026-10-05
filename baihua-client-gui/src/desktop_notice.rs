//! System notifications: notify-rust on the desktops, the platform back end on
//! mobile. Sending runs on a background thread and failures are only logged.

use baihua_core::config;

/// The notification sound file (macOS only): the same one the terminal version uses.
#[cfg(target_os = "macos")]
fn sound_path() -> &'static str {
    "/System/Library/Sounds/Ping.aiff"
}

/// Play the notification sound on a background thread: the macOS built-in
/// notification is silent, so playback goes through `afplay`.
fn play_sound() {
    #[cfg(target_os = "macos")]
    std::thread::spawn(|| {
        use std::process::Stdio;
        // The interface has no console, so the player output must not leak into
        // the terminal; success or failure is only recorded in the debug log.
        match std::process::Command::new("afplay")
            .arg(sound_path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(status) if status.success() => config::debug_log("Notification sound played"),
            Ok(status) => config::debug_log(&format!("Notification sound exited with {status:?}")),
            Err(error) => config::debug_log(&format!("Notification sound failed: {error}")),
        }
    });
}

/// A catch-up burst applies dozens of queued events inside one frame, so every
/// presentation channel merges what it receives within two seconds of itself.
const POST_GAP_MILLIS: u64 = 2000;

/// Epoch millisecond of the last accepted in-app banner (0 means never).
static IN_APP_MILLIS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Epoch millisecond of the last accepted system post (0 means never).
#[cfg(any(target_os = "android", target_os = "ios"))]
static SYSTEM_POST_MILLIS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Whether the given channel may present again (the burst merge).
fn burst_ready(last_millis: &std::sync::atomic::AtomicU64) -> bool {
    use std::sync::atomic::Ordering;
    let now = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(elapsed) => elapsed.as_millis() as u64,
        Err(_error) => return true,
    };
    let previous = last_millis.swap(now, Ordering::Relaxed);
    now.saturating_sub(previous) >= POST_GAP_MILLIS
}

/// In-app banners run on every platform and share one merge window.
pub fn in_app_ready() -> bool {
    burst_ready(&IN_APP_MILLIS)
}

/// Pop a system notification (title, body) plus the alert sound; nothing is
/// sent while the settings sound switch is off (`Client::sound_enabled`).
pub fn send(sound_enabled: bool, title: &str, body: &str) {
    if !sound_enabled {
        return;
    }
    play_sound();
    // notify-rust only exists on the three desktop systems (see the gated
    // dependency in `Cargo.toml`); mobile has its own back end below.
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    {
        let title = title.to_string();
        let body = body.to_string();
        std::thread::spawn(move || {
            match notify_rust::Notification::new()
                .summary(&title)
                .body(&body)
                .appname("Baihua Client")
                .show()
            {
                Ok(_handle) => config::debug_log(&format!("Desktop notification sent: {title}")),
                Err(error) => {
                    config::debug_log(&format!("Desktop notification sending failed: {error}"))
                }
            }
        });
    }
    // Android has its own back end (`android_platform`): a heads-up
    // NotificationManager post with the system sound, on a background thread.
    #[cfg(target_os = "android")]
    {
        if !burst_ready(&SYSTEM_POST_MILLIS) {
            config::debug_log("Android notification debounced (burst window)");
            return;
        }
        let title = title.to_string();
        let body = body.to_string();
        std::thread::spawn(move || {
            match crate::android_platform::post_notification(&title, &body) {
                Ok(()) => config::debug_log(&format!("Android notification sent: {title}")),
                Err(error) => config::debug_log(&format!("Android notification failed: {error}")),
            }
        });
    }
    // iOS uses `UNUserNotificationCenter` through `ios_platform`: a local
    // notification with the system sound, shown even in the foreground.
    #[cfg(target_os = "ios")]
    {
        if !burst_ready(&SYSTEM_POST_MILLIS) {
            config::debug_log("iOS notification debounced (burst window)");
            return;
        }
        let title = title.to_string();
        let body = body.to_string();
        std::thread::spawn(
            move || match crate::ios_platform::post_notification(&title, &body) {
                Ok(()) => config::debug_log(&format!("iOS notification sent: {title}")),
                Err(error) => config::debug_log(&format!("iOS notification failed: {error}")),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    // `send` and the sleep helper are only used by the desktop-only ignored
    // test below; on mobile the imports would be dead code.
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    use super::send;
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    use std::time::Duration;

    /// The sound file must exist, otherwise `afplay` only fails silently; the
    /// check is macOS-only because other systems use the notification sound.
    #[cfg(target_os = "macos")]
    #[test]
    fn sound_file_exists() {
        assert!(
            std::path::Path::new(super::sound_path()).is_file(),
            "the system notification sound file must really exist: {}",
            super::sound_path()
        );
    }

    /// Pops a real notification to check the call does not error; ignored so a
    /// test run does not disturb the desktop, run it manually with `--ignored`.
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    #[test]
    #[ignore = "Will actually pop a system notification, run manually"]
    fn send_is_attempted() {
        let outcome = notify_rust::Notification::new()
            .summary("Baihua client")
            .body("Desktop notification self-check: if you can read this, notify-rust works")
            .appname("Baihua Client")
            .show();
        match outcome {
            Ok(_handle) => println!("Desktop notification self-check succeeded"),
            Err(error) => println!("Desktop notification self-check failed: {error}"),
        }
        // `send` runs in a background thread; confirm it neither panics nor blocks.
        send(
            false,
            "Do not send when the switch is off",
            "this notification must not appear",
        );
        send(
            true,
            "Baihua client",
            "Desktop notification self-check (background thread)",
        );
        // The sound plays in a background thread and `afplay` needs about three
        // seconds, so wait before the process ends and the log is lost.
        std::thread::sleep(Duration::from_millis(3500));
    }
}
