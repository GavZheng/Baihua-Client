//! iOS plumbing over `objc2`: the soft keyboard's Return key swizzled into a real
//! `Key::Enter`, and the notification back end with its foreground delegate.

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, ProtocolObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, ffi, msg_send};
use objc2_foundation::{NSError, NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationSettings,
    UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use std::panic::AssertUnwindSafe;
use std::ptr::NonNull;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};

/// Name winit registers its own view class under; it appears with the first window,
/// so the hook retries every frame until it is found.
const WINIT_VIEW_CLASS_NAME: &std::ffi::CStr = c"WinitUIView";
/// The selector UIKit calls to hand typed text (a lone Return arrives as "\n").
const INSERT_TEXT_SELECTOR_NAME: &std::ffi::CStr = c"insertText:";
/// Prefix of every posted notification identifier.
const NOTIFICATION_IDENTIFIER_PREFIX: &str = "baihua-message";
/// Legacy UIKit banner class: the only presentation path sideload containers
/// such as LiveContainer leave working when the notification centre is dead.
const LOCAL_NOTIFICATION_CLASS_NAME: &std::ffi::CStr = c"UILocalNotification";
/// The application object the legacy present selector runs on.
const APPLICATION_CLASS_NAME: &std::ffi::CStr = c"UIApplication";
/// The class whose Foundation method hands the presentation to the main thread.
const THREAD_CLASS_NAME: &std::ffi::CStr = c"NSThread";
/// Legacy sound name meaning "the system default alert sound".
const LEGACY_SOUND_NAME: &str = "default";
/// AudioServices id of the built-in alert tone (plays even without any grant).
const ALERT_SOUND_IDENTIFIER: u32 = 1000;
/// Authorization snapshot before the settings callback ever answered.
const STATUS_UNKNOWN: i32 = -99;
/// A silently dropped ask must not block retries forever (the completion of a
/// dead notification centre never runs, which froze the old in-flight flag).
const ASK_COOLDOWN_MILLIS: u64 = 15000;
/// The `insertText:` ABI: `(self, _cmd, NSString *)`.
type InsertTextImplementation = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject);

/// Set by the swizzled `insertText:` on a lone Return and consumed while building
/// the next frame's raw input.
static RETURN_KEY_PENDING: AtomicBool = AtomicBool::new(false);
/// The original `insertText:` implementation, called through for real text.
static ORIGINAL_INSERT_TEXT: OnceLock<Imp> = OnceLock::new();
/// Whether the `insertText:` swizzle already ran (the class may not exist yet).
static RETURN_KEY_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
/// Whether the notification back end has been set up exactly once.
static NOTIFICATION_SETUP_DONE: AtomicBool = AtomicBool::new(false);
/// When the last authorization request went out (epoch millis, 0 means never).
static AUTHORIZATION_ASK_MILLIS: AtomicU64 = AtomicU64::new(0);
/// Raw authorization status of the last settings read (see STATUS_UNKNOWN).
static AUTHORIZATION_SNAPSHOT: AtomicI32 = AtomicI32::new(STATUS_UNKNOWN);
/// Running id for posted notifications, so distinct messages stay distinct.
static NOTIFICATION_COUNTER: AtomicUsize = AtomicUsize::new(1);

// The notification delegate: iOS only shows a foreground notification when one asks
// for it, and the centre holds it weakly, so the instance lives in the thread-local.
define_class!(
    // SAFETY: NSObject has no subclassing requirements and this class has no
    // `Drop` impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BaihuaNotificationDelegate"]
    struct NotificationDelegate;

    impl NotificationDelegate {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Retained<Self> {
            let this = this.set_ivars(());
            unsafe { msg_send![super(this), init] }
        }
    }

    unsafe impl NSObjectProtocol for NotificationDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion_handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            // Banner, sound and list: show it like a real notification even though
            // the app is in front, or the system keeps it silent and invisible.
            completion_handler.call((
                UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::Sound
                    | UNNotificationPresentationOptions::List,
            ));
        }
    }
);

thread_local! {
    /// Keeps the delegate alive, since the centre's delegate property is weak.
    static NOTIFICATION_DELEGATE: std::cell::RefCell<Option<Retained<NotificationDelegate>>> =
        const { std::cell::RefCell::new(None) };
}

/// One-time iOS setup from the frame hook, once the window exists. Idempotent.
pub(crate) fn install() {
    install_return_hook();
    if !NOTIFICATION_SETUP_DONE.load(Ordering::Relaxed) && install_delegate() {
        NOTIFICATION_SETUP_DONE.store(true, Ordering::Relaxed);
    }
}

/// Whether the soft keyboard's Return arrived since the last check, taking the
/// flag with it (exactly one frame consumes one press).
pub(crate) fn take_return_key() -> bool {
    RETURN_KEY_PENDING.swap(false, Ordering::Relaxed)
}

/// Swizzle `insertText:` on winit's view class: swallow and remember a lone newline,
/// pass everything else through. Retries until the class exists.
fn install_return_hook() {
    if RETURN_KEY_HOOK_INSTALLED.load(Ordering::Relaxed) {
        return;
    }
    let Some(class) = AnyClass::get(WINIT_VIEW_CLASS_NAME) else {
        // No window yet: try again next frame.
        return;
    };
    // SAFETY: `class` is live, the selector exists on it, and the replacement's ABI
    // is the declaring method's own type encoding read back from the runtime.
    unsafe {
        let selector = Sel::register(INSERT_TEXT_SELECTOR_NAME);
        let Some(method) = class.instance_method(selector) else {
            return;
        };
        let encoding = ffi::method_getTypeEncoding(method);
        if encoding.is_null() {
            return;
        }
        if ORIGINAL_INSERT_TEXT.set(method.implementation()).is_err() {
            // An earlier frame already installed it.
            RETURN_KEY_HOOK_INSTALLED.store(true, Ordering::Relaxed);
            return;
        }
        let replacement: Imp =
            std::mem::transmute::<InsertTextImplementation, Imp>(swizzled_insert_text);
        ffi::class_replaceMethod(
            class as *const AnyClass as *mut AnyClass,
            selector,
            replacement,
            encoding,
        );
        RETURN_KEY_HOOK_INSTALLED.store(true, Ordering::Relaxed);
        baihua_core::config::debug_log("iOS: installed the Return key hook");
    }
}

/// The `- (void)insertText:(NSString *)text` replacement: text that is only newlines
/// is the Return key, remembered and not forwarded; everything else passes through.
unsafe extern "C-unwind" fn swizzled_insert_text(
    this: *mut AnyObject,
    selector: Sel,
    text: *mut AnyObject,
) {
    if !text.is_null() {
        // SAFETY: `insertText:` is declared with an `NSString *` argument, and
        // the caller (UIKit) guarantees it is non-null at this point.
        let string = unsafe { &*(text as *const NSString) };
        let typed = string.to_string();
        let only_newlines = !typed.is_empty()
            && typed
                .chars()
                .all(|character| character == '\n' || character == '\r');
        if only_newlines {
            RETURN_KEY_PENDING.store(true, Ordering::Relaxed);
            return;
        }
    }
    // SAFETY: the stored pointer is the previous implementation of this very
    // method, called with the arguments ObjC handed us.
    if let Some(original) = ORIGINAL_INSERT_TEXT.get() {
        unsafe {
            let original: InsertTextImplementation = std::mem::transmute(*original);
            original(this, selector, text);
        }
    }
}

/// Register the foreground-presentation delegate and ask for the permission;
/// reports whether it ran (it needs the main thread), so the caller can retry.
fn install_delegate() -> bool {
    let Some(marker) = MainThreadMarker::new() else {
        baihua_core::config::debug_log("iOS: notification setup needs the main thread");
        return false;
    };
    // A sideload container can reject the centre outright; the exception
    // guard keeps that startup attempt from taking the process down.
    let installed = objc2::exception::catch(AssertUnwindSafe(|| {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let delegate: Retained<NotificationDelegate> = unsafe { msg_send![marker.alloc(), init] };
        let delegate_reference = ProtocolObject::from_ref(&*delegate);
        center.setDelegate(Some(delegate_reference));
        NOTIFICATION_DELEGATE.with(|slot| *slot.borrow_mut() = Some(delegate));
    }));
    installed.is_ok()
}

/// The authorization request, raised only by the user flipping the in-app
/// switch (the gesture path); the message path never calls into it.
pub(crate) fn grant_permission() {
    ask_authorization();
}

/// Ask for alert+badge+sound; a time gate replaces the in-flight flag, because
/// a dead container never runs the completion and froze every later retry.
fn ask_authorization() {
    let now = epoch_millis();
    let previous = AUTHORIZATION_ASK_MILLIS.swap(now, Ordering::Relaxed);
    if previous != 0 && now.saturating_sub(previous) < ASK_COOLDOWN_MILLIS {
        return;
    }
    let options = UNAuthorizationOptions::Alert
        | UNAuthorizationOptions::Badge
        | UNAuthorizationOptions::Sound;
    let handler = block2::RcBlock::new(|granted: Bool, error: *mut NSError| {
        if !error.is_null() {
            AUTHORIZATION_ASK_MILLIS.store(0, Ordering::Relaxed);
        }
        if granted.as_bool() {
            AUTHORIZATION_SNAPSHOT.store(
                UNAuthorizationStatus::Authorized.0 as i32,
                Ordering::Relaxed,
            );
        }
        if error.is_null() {
            baihua_core::config::debug_log(&format!(
                "iOS: notification permission request finished, granted={granted:?}"
            ));
        } else {
            baihua_core::config::debug_log("iOS: notification permission request failed");
        }
    });
    let outcome = objc2::exception::catch(AssertUnwindSafe(|| {
        UNUserNotificationCenter::currentNotificationCenter()
            .requestAuthorizationWithOptions_completionHandler(options, &handler);
    }));
    if outcome.is_err() {
        AUTHORIZATION_ASK_MILLIS.store(0, Ordering::Relaxed);
        baihua_core::config::debug_log("iOS: the notification centre refused the permission ask");
    }
}

/// Refresh the authorization snapshot. A dead container throws Objective-C
/// exceptions instead of answering, so both the call and its callback are
/// guarded; the snapshot simply keeps its previous value when either raises.
fn read_status() {
    let checker = block2::RcBlock::new(|settings: NonNull<UNNotificationSettings>| {
        let _ = objc2::exception::catch(AssertUnwindSafe(|| {
            // SAFETY: the system hands a valid settings object to this callback.
            let status = unsafe { settings.as_ref() }.authorizationStatus();
            AUTHORIZATION_SNAPSHOT.store(status.0 as i32, Ordering::Relaxed);
        }));
    });
    let _ = objc2::exception::catch(AssertUnwindSafe(|| {
        UNUserNotificationCenter::currentNotificationCenter()
            .getNotificationSettingsWithCompletionHandler(&checker);
    }));
}

/// Post a local notification with the system sound, the iOS counterpart of the
/// Android path; safe from a background thread, the centre is thread-safe.
pub(crate) fn post_notification(title: &str, body: &str) -> Result<(), &'static str> {
    read_status();
    if center_alive() && post_via_center(title, body) {
        return Ok(());
    }
    // A denied user gets the tone only (no banner is ours to force); an
    // unanswered or dead host gets the legacy banner, the tone when that died.
    let denied =
        AUTHORIZATION_SNAPSHOT.load(Ordering::Relaxed) == UNAuthorizationStatus::Denied.0 as i32;
    if denied || !present_legacy(title, body) {
        play_alert_sound();
    }
    Ok(())
}

/// Add through the modern centre; false when it raises instead of answering
/// (the behaviour of a sideload container with no reachable usernoted).
fn post_via_center(title: &str, body: &str) -> bool {
    let title = title.to_string();
    let body = body.to_string();
    let outcome = objc2::exception::catch(AssertUnwindSafe(|| {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&title));
        content.setBody(&NSString::from_str(&body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        let identifier = next_identifier();
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &identifier,
            &content,
            None,
        );
        let handler = block2::RcBlock::new(|error: *mut NSError| {
            if error.is_null() {
                baihua_core::config::debug_log("iOS: notification request accepted");
            } else {
                // SAFETY: non-null means the centre reported a real error object.
                let message = unsafe { &*error }.localizedDescription();
                baihua_core::config::debug_log(&format!("iOS: add failed: {message}"));
            }
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, Some(&handler));
    }));
    outcome.is_ok()
}

/// Whether the notification centre can be trusted right now: only an
/// authorized or provisional snapshot says yes to the modern path.
fn center_alive() -> bool {
    let snapshot = AUTHORIZATION_SNAPSHOT.load(Ordering::Relaxed);
    snapshot == UNAuthorizationStatus::Authorized.0 as i32
        || snapshot == UNAuthorizationStatus::Provisional.0 as i32
}

/// Present through the deprecated UIKit API: LiveContainer-class hosts leave
/// the UserNotifications daemon unreachable, and this is the banner they kept.
fn present_legacy(title: &str, body: &str) -> bool {
    let Some(class) = AnyClass::get(LOCAL_NOTIFICATION_CLASS_NAME) else {
        return false;
    };
    let Some(thread_class) = AnyClass::get(THREAD_CLASS_NAME) else {
        return false;
    };
    let Some(application) = AnyClass::get(APPLICATION_CLASS_NAME) else {
        return false;
    };
    let title = title.to_string();
    let body = body.to_string();
    let outcome = objc2::exception::catch(AssertUnwindSafe(|| -> bool {
        let title_text = NSString::from_str(&title);
        let body_text = NSString::from_str(&body);
        let sound_text = NSString::from_str(LEGACY_SOUND_NAME);
        // SAFETY: NSObject answers `new`; the three setters take NSString
        // parameters; `performBlockOnMainThread:` is a void NSThread class
        // method taking a block (a copyable Objective-C object).
        unsafe {
            let raw: *mut AnyObject = msg_send![class, new];
            let Some(note) = Retained::from_raw(raw) else {
                return false;
            };
            let _: () = msg_send![&*note, setAlertTitle: &*title_text];
            let _: () = msg_send![&*note, setAlertBody: &*body_text];
            let _: () = msg_send![&*note, setSoundName: &*sound_text];
            let poster = block2::RcBlock::new(move || {
                // SAFETY: `sharedApplication` answers the class and the legacy
                // present selector takes the notification object captured here.
                let _ = objc2::exception::catch(AssertUnwindSafe(|| {
                    let app: *mut AnyObject = msg_send![application, sharedApplication];
                    if !app.is_null() {
                        let _: () = msg_send![&*app, presentLocalNotificationNow: &*note];
                    }
                }));
            });
            let block_pointer = &*poster as *const block2::DynBlock<dyn Fn()>;
            let _: () = msg_send![thread_class, performBlockOnMainThread: block_pointer];
        }
        true
    }));
    outcome.unwrap_or(false)
}

/// The guaranteed tone for hosts where even the legacy banner is gone: the
/// system alert sound through AudioToolbox needs no permission of any kind.
fn play_alert_sound() {
    // SAFETY: a plain C entry point; the identifier selects a built-in tone.
    unsafe { AudioServicesPlayAlertSound(ALERT_SOUND_IDENTIFIER) };
}

/// Milliseconds since the epoch, 0 when the clock is somehow before it.
fn epoch_millis() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(elapsed) => elapsed.as_millis() as u64,
        Err(_error) => 0,
    }
}

// The cdylib link needs the framework named here; `ios/build-ipa.sh` keeps its
// own copy of the list for the static-library hand-over to the Swift runner.
#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    /// AudioToolbox: plays one of the built-in system alert sounds.
    fn AudioServicesPlayAlertSound(system_sound_id: u32);
}

/// A unique notification identifier, so messages queue up instead of overwriting.
fn next_identifier() -> Retained<NSString> {
    let raw = NOTIFICATION_COUNTER.fetch_add(1, Ordering::Relaxed);
    NSString::from_str(&format!("{NOTIFICATION_IDENTIFIER_PREFIX}-{raw}"))
}
