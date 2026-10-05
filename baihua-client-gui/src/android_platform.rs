//! Android plumbing over JNI: the soft-input mode, the notification permission, the
//! status-bar measurement and the notification post. Failures only reach the log.

use android_activity::AndroidApp;
use jni::objects::{JObject, JValue};
use jni::sys::jint;
use jni::{AttachGuard, JNIEnv, JavaVM};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// `WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE`: resize the window around
/// the soft keyboard instead of panning or letting the keyboard cover the surface.
const SOFT_INPUT_ADJUST_RESIZE: jint = 0x00000010;
/// `Context.PERMISSION_GRANTED` from `checkSelfPermission`.
const PERMISSION_GRANTED: jint = 0;
/// `Build.VERSION_CODES.TIRAMISU` (Android 13): notifications need a permission.
const SDK_VERSION_RUNTIME_NOTIFICATION_PERMISSION: jint = 33;
/// `Build.VERSION_CODES.O` (Android 8): first release with notification channels.
const SDK_VERSION_NOTIFICATION_CHANNELS: jint = 26;
/// `Build.VERSION_CODES.M` (Android 6): first release exposing root window insets.
const SDK_VERSION_ROOT_WINDOW_INSETS: jint = 23;
/// `Build.VERSION_CODES.Q` (Android 10): first release with typed inset masks.
const SDK_VERSION_INSET_TYPES: jint = 29;
/// `RingtoneManager.TYPE_NOTIFICATION`: pick the system's notification sound.
const RINGTONE_TYPE_NOTIFICATION: jint = 2;
/// Notification-channel importance `HIGH` (heads-up + sound + LED).
const CHANNEL_IMPORTANCE_HIGH: jint = 4;
/// The channel every Baihua message notification is posted to; an older build
/// posted before creation, so that auto-made silent id can never be reused.
const NOTIFICATION_CHANNEL_ID: &str = "baihua-messages-2";
/// The launcher icon name in `android/res` reused as the small notification icon.
const NOTIFICATION_SMALL_ICON_NAME: &str = "ic_launcher";
/// `Notification.DEFAULT_ALL`: the system's default sound, vibration and lights.
/// Only pre-Android-8 builds honour it; later the channel owns the sound.
const NOTIFICATION_DEFAULT_ALL: jint = -1;

/// Status-bar overlap in physical pixels, measured on the Java main thread and read
/// back by the shared mobile spacer; zero means the system inset the surface.
static STATUS_BAR_OVERLAP_PIXELS: AtomicI32 = AtomicI32::new(0);
/// Whether one measurement decided the spacer from a trustworthy source: false
/// keeps the frame hook re-posting every frame during the startup window.
static SPACER_RESOLVED: AtomicBool = AtomicBool::new(false);
/// Epoch milliseconds of process start, bounding the every-frame fast retries.
static START_MILLIS: OnceLock<u64> = OnceLock::new();
/// Epoch milliseconds of the last posted measurement; passing the remeasure
/// interval marks the next frame stale even when the viewport never changed.
static LAST_MEASUREMENT_MILLIS: AtomicU64 = AtomicU64::new(0);
/// Activity handle kept so a later message can re-post the permission request.
static MAIN_APPLICATION: OnceLock<AndroidApp> = OnceLock::new();
/// Running id for posted notifications, so distinct messages stay distinct.
static NOTIFICATION_COUNTER: AtomicI32 = AtomicI32::new(1);

/// One-time startup work from `android_main`: one post to the Java main thread
/// for the soft-input mode. The permission dialog waits for the user's switch.
pub(crate) fn install(android_application: &AndroidApp) {
    let _ = MAIN_APPLICATION.set(android_application.clone());
    android_application.run_on_java_main_thread(Box::new(move || {
        log_if_failed("configure_window", configure_window());
    }));
}

/// Viewport size (packed egui point bits) the last measurement was for; any
/// change (rotation, keyboard resize) triggers exactly one new measurement.
static LAST_MEASURED_VIEWPORT: AtomicU64 = AtomicU64::new(u64::MAX);

/// Wall-clock milliseconds since the epoch: a static cannot hold an `Instant`,
/// and a clock jump only buys one extra measurement, never a wrong spacer.
fn epoch_millis() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => since.as_millis() as u64,
        Err(_predating_epoch) => 0,
    }
}

/// Remeasure period in milliseconds: catches inset flips (immersive hide, bar
/// changes) that leave the viewport rectangle untouched and escape the hook.
pub(crate) fn remeasure_interval() -> u64 {
    2000
}

/// Epoch milliseconds at process start, taken once for the fast-retry window.
fn start_millis() -> u64 {
    *START_MILLIS.get_or_init(epoch_millis)
}

/// Every-frame retry window after start, in milliseconds: enough for the window
/// to attach, short enough to stop JNI bursts when the insets never dispatch.
fn fast_retry_window_millis() -> u64 {
    3000
}

/// Frame-loop hook: re-measure on the Java main thread when the viewport rectangle
/// changed, the spacer never decided (startup only), or the measurement went stale.
pub(crate) fn remeasure_viewport(android_application: &AndroidApp, context: &egui::Context) {
    let viewport = context.viewport_rect();
    let packed: u64 =
        ((viewport.width().to_bits() as u64) << 32) | viewport.height().to_bits() as u64;
    let now_millis = epoch_millis();
    let resized = LAST_MEASURED_VIEWPORT.swap(packed, Ordering::Relaxed) != packed;
    let stale = now_millis.saturating_sub(LAST_MEASUREMENT_MILLIS.load(Ordering::Relaxed))
        >= remeasure_interval();
    let fast_retry = !SPACER_RESOLVED.load(Ordering::Relaxed)
        && now_millis.saturating_sub(start_millis()) < fast_retry_window_millis();
    if resized || stale || fast_retry {
        LAST_MEASUREMENT_MILLIS.store(now_millis, Ordering::Relaxed);
        android_application.run_on_java_main_thread(Box::new(move || {
            log_if_failed("measure_window", measure_window());
        }));
    }
}

/// The value stored after every measurement, pure so the rule is testable off-device.
/// The system's own bar height caps it: a transient reading once squeezed the area.
pub(crate) fn spacer_pixels(
    surface_top_on_screen: i32,
    inset_top_pixels: Option<i32>,
    status_bar_height: i32,
    portrait: bool,
    floor_pixels: i32,
) -> (i32, bool) {
    // A window the system already pushed below the bar needs no spacer, decided.
    if surface_top_on_screen != 0 {
        return (0, true);
    }
    // First source: a dispatched positive inset, capped against runaway transients
    // but never clamped by an unreadable bar height.
    if let Some(inset) = inset_top_pixels.filter(|inset| *inset > 0) {
        let capped = if status_bar_height > 0 {
            inset.clamp(0, status_bar_height)
        } else {
            inset
        };
        return (capped, true);
    }
    // Second and third source, portrait only: the bar is always shown there, so
    // the static framework height, then the 24dp floor, are safe to pad blind.
    if portrait {
        if status_bar_height > 0 {
            return (status_bar_height, true);
        }
        return (floor_pixels.max(0), floor_pixels > 0);
    }
    // Landscape trusts dispatch only: a reported zero means the bar really hides
    // (decided), while no reading at all stays undecided for the retry hook.
    (0, inset_top_pixels.is_some())
}

/// Read the measured overlap in logical points for this frame's scale.
pub(crate) fn overlap_points(context: &egui::Context) -> f32 {
    let overlap_pixels = STATUS_BAR_OVERLAP_PIXELS.load(Ordering::Relaxed);
    if overlap_pixels <= 0 {
        return 0.0;
    }
    overlap_pixels as f32 / context.pixels_per_point()
}

/// Post a heads-up notification with the system sound; callable from any thread,
/// since Rust threads attach to the JVM on demand and the manager is thread-safe.
/// A blocked or failed delivery degrades to the plain system tone (plus the
/// in-app banner the session layer already showed), never to a permission dialog.
pub(crate) fn post_notification(title: &str, body: &str) -> jni::errors::Result<()> {
    let (mut environment, activity) = attach_to_jvm()?;
    if sdk_version(&mut environment)? >= SDK_VERSION_RUNTIME_NOTIFICATION_PERMISSION
        && has_permission(
            &mut environment,
            &activity,
            "android.permission.POST_NOTIFICATIONS",
        )? != PERMISSION_GRANTED
    {
        baihua_core::config::debug_log("Android: notifications blocked, playing the tone");
        play_alert_tone();
        return Ok(());
    }
    let outcome = deliver(&mut environment, &activity, title, body);
    if outcome.is_err() {
        play_alert_tone();
    }
    outcome
}

/// Manager lookup, channel guarantee and the heads-up post itself.
fn deliver(
    environment: &mut JNIEnv<'_>,
    activity: &JObject<'_>,
    title: &str,
    body: &str,
) -> jni::errors::Result<()> {
    let manager = notification_manager(environment, activity)?;
    ensure_channel(environment, &manager)?;
    let notification = build_notification(environment, activity, title, body)?;
    let identifier = NOTIFICATION_COUNTER.fetch_add(1, Ordering::Relaxed);
    environment
        .call_method(
            &manager,
            "notify",
            "(ILandroid/app/Notification;)V",
            &[JValue::Int(identifier), JValue::Object(&notification)],
        )
        .map(|_value| ())
}

/// The permission dialog, raised only by the user flipping the in-app switch:
/// a gesture moment with the activity resumed, never from the message path.
pub(crate) fn grant_permission() {
    let Some(android_application) = MAIN_APPLICATION.get() else {
        baihua_core::config::debug_log("Android: no activity handle, permission not asked");
        return;
    };
    let application = android_application.clone();
    android_application.run_on_java_main_thread(Box::new(move || {
        log_if_failed("grant_permission", request_permission(&application));
    }));
}

/// Fallback alert sound through the system notification ringtone: needs no
/// permission, no channel and no dialog, so it works wherever posts died.
fn play_alert_tone() {
    let Some(android_application) = MAIN_APPLICATION.get() else {
        return;
    };
    android_application.run_on_java_main_thread(Box::new(move || {
        log_if_failed("play_alert_tone", tone_on_main());
    }));
}

/// `RingtoneManager.getRingtone(default notification uri).play()` on the Java
/// main thread, whose looper the ringtone machinery likes to run under.
fn tone_on_main() -> jni::errors::Result<()> {
    let (mut environment, activity) = attach_to_jvm()?;
    let uri = environment
        .call_static_method(
            "android/media/RingtoneManager",
            "getDefaultUri",
            "(I)Landroid/net/Uri;",
            &[JValue::Int(RINGTONE_TYPE_NOTIFICATION)],
        )?
        .l()?;
    let ringtone = environment
        .call_static_method(
            "android/media/RingtoneManager",
            "getRingtone",
            "(Landroid/content/Context;Landroid/net/Uri;)Landroid/media/Ringtone;",
            &[JValue::Object(&activity), JValue::Object(&uri)],
        )?
        .l()?;
    if ringtone.is_null() {
        return Ok(());
    }
    environment.call_method(&ringtone, "play", "()V", &[])?;
    Ok(())
}

/// The JVM handle (android-activity initializes `ndk-context` before
/// `android_main` runs, so the pointers are valid for the whole process).
fn java_vm() -> &'static JavaVM {
    static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();
    JAVA_VM.get_or_init(|| {
        let raw: *mut jni::sys::JavaVM = ndk_context::android_context().vm().cast();
        unsafe { JavaVM::from_raw(raw) }
            .expect("the Android JVM pointer from ndk-context must be valid")
    })
}

/// Attach the calling thread to the JVM and hand back the environment plus the
/// activity object (a long-lived global reference owned by the runtime).
fn attach_to_jvm() -> jni::errors::Result<(AttachGuard<'static>, JObject<'static>)> {
    let environment = java_vm().attach_current_thread()?;
    let activity = unsafe { JObject::from_raw(ndk_context::android_context().context().cast()) };
    Ok((environment, activity))
}

fn log_if_failed(stage: &str, outcome: jni::errors::Result<()>) {
    match outcome {
        Ok(()) => baihua_core::config::debug_log(&format!("Android: {stage} succeeded")),
        Err(error) => baihua_core::config::debug_log(&format!("Android: {stage} failed: {error}")),
    }
}

fn sdk_version(environment: &mut JNIEnv<'_>) -> jni::errors::Result<i32> {
    environment
        .get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?
        .i()
}

fn has_permission(
    environment: &mut JNIEnv<'_>,
    activity: &JObject<'_>,
    permission: &str,
) -> jni::errors::Result<i32> {
    let permission_string = environment.new_string(permission)?;
    environment
        .call_method(
            activity,
            "checkSelfPermission",
            "(Ljava/lang/String;)I",
            &[JValue::Object(&permission_string.into())],
        )?
        .i()
}

/// Ask the system for the notification permission; the dialog belongs to the
/// activity lifecycle, so this runs on the Java main thread.
fn request_permission(android_application: &AndroidApp) -> jni::errors::Result<()> {
    let _ = android_application;
    let (mut environment, activity) = attach_to_jvm()?;
    if sdk_version(&mut environment)? < SDK_VERSION_RUNTIME_NOTIFICATION_PERMISSION {
        return Ok(());
    }
    if has_permission(
        &mut environment,
        &activity,
        "android.permission.POST_NOTIFICATIONS",
    )? == PERMISSION_GRANTED
    {
        return Ok(());
    }
    let permission = environment.new_string("android.permission.POST_NOTIFICATIONS")?;
    let permissions = environment.new_object_array(1, "java/lang/String", &permission)?;
    environment.call_method(
        &activity,
        "requestPermissions",
        "([Ljava/lang/String;I)V",
        &[JValue::Object(&permissions), JValue::Int(0)],
    )?;
    Ok(())
}

/// The one-time window configuration on the Java main thread: ask for
/// `SOFT_INPUT_ADJUST_RESIZE` and measure. Window flags and relayouts are gone.
fn configure_window() -> jni::errors::Result<()> {
    let (mut environment, activity) = attach_to_jvm()?;
    let window = environment
        .call_method(&activity, "getWindow", "()Landroid/view/Window;", &[])?
        .l()?;
    environment.call_method(
        &window,
        "setSoftInputMode",
        "(I)V",
        &[JValue::Int(SOFT_INPUT_ADJUST_RESIZE)],
    )?;
    measure_window()
}

/// Decide this window's status-bar padding from three sources in order: dispatched
/// root insets, the framework bar height, then the portrait floor; log every change.
fn measure_window() -> jni::errors::Result<()> {
    let (mut environment, activity) = attach_to_jvm()?;
    let window = environment
        .call_method(&activity, "getWindow", "()Landroid/view/Window;", &[])?
        .l()?;
    let decor = environment
        .call_method(&window, "getDecorView", "()Landroid/view/View;", &[])?
        .l()?;
    let mut location = [0 as jint; 2];
    let location_array = environment.new_int_array(2)?;
    environment.call_method(
        &decor,
        "getLocationOnScreen",
        "([I)V",
        &[JValue::Object(&location_array)],
    )?;
    environment.get_int_array_region(&location_array, 0, &mut location)?;
    let inset_top_pixels = status_bar_inset_top(&mut environment, &decor)?;
    let status_bar_height = status_bar_height(&mut environment, &activity)?;
    let portrait = screen_orientation(&mut environment, &activity)? == ORIENTATION_PORTRAIT;
    let mut floor_pixels = 0;
    if portrait && status_bar_height <= 0 {
        floor_pixels = portrait_floor_pixels(&mut environment, &activity)?;
    }
    let (overlap, decided) = spacer_pixels(
        location[1],
        inset_top_pixels,
        status_bar_height,
        portrait,
        floor_pixels,
    );
    let previous_decided = SPACER_RESOLVED.swap(decided, Ordering::Relaxed);
    let previous = STATUS_BAR_OVERLAP_PIXELS.swap(overlap, Ordering::Relaxed);
    if previous != overlap || decided != previous_decided {
        baihua_core::config::debug_log(&format!(
            "Android: status spacer decor_top={} inset={:?} bar={} portrait={} floor={} overlap_px={} decided={}",
            location[1],
            inset_top_pixels,
            status_bar_height,
            portrait,
            floor_pixels,
            overlap,
            decided
        ));
    }
    Ok(())
}

/// The status-bar inset top from the decor view's root WindowInsets: the typed
/// mask on Android 10+, the deprecated getter on 6..9, `None` until they attach.
fn status_bar_inset_top(
    environment: &mut JNIEnv<'_>,
    decor: &JObject<'_>,
) -> jni::errors::Result<Option<i32>> {
    if sdk_version(environment)? < SDK_VERSION_ROOT_WINDOW_INSETS {
        return Ok(Some(0)); // such windows are always inset by the system itself
    }
    let insets = environment
        .call_method(
            decor,
            "getRootWindowInsets",
            "()Landroid/view/WindowInsets;",
            &[],
        )?
        .l()?;
    if insets.is_null() {
        return Ok(None);
    }
    if sdk_version(environment)? >= SDK_VERSION_INSET_TYPES {
        let mask = environment
            .call_static_method("android/view/WindowInsets$Type", "statusBars", "()I", &[])?
            .i()?;
        let bar_insets = environment
            .call_method(
                &insets,
                "getInsets",
                "(I)Landroid/graphics/Insets;",
                &[JValue::Int(mask)],
            )?
            .l()?;
        return Ok(Some(environment.get_field(&bar_insets, "top", "I")?.i()?));
    }
    let legacy_top = environment
        .call_method(&insets, "getSystemWindowInsetTop", "()I", &[])?
        .i()?;
    Ok(Some(legacy_top))
}

/// The system's own status bar height in pixels, read from the framework package:
/// the app namespace never owns that dimen, so asking there silently returns zero.
/// `activity.getResources()`, the shared entry for the height, orientation and
/// density reads below.
fn resources_of<'local>(
    environment: &mut JNIEnv<'local>,
    activity: &JObject<'_>,
) -> jni::errors::Result<JObject<'local>> {
    environment
        .call_method(
            activity,
            "getResources",
            "()Landroid/content/res/Resources;",
            &[],
        )?
        .l()
}

/// `Configuration.ORIENTATION_PORTRAIT`: the bar is always shown in this posture,
/// so static sources may pad blind; landscape trusts dispatched insets only.
const ORIENTATION_PORTRAIT: jint = 1;

/// Current screen orientation code from the live configuration.
fn screen_orientation(
    environment: &mut JNIEnv<'_>,
    activity: &JObject<'_>,
) -> jni::errors::Result<i32> {
    let resources = resources_of(environment, activity)?;
    let configuration = environment
        .call_method(
            &resources,
            "getConfiguration",
            "()Landroid/content/res/Configuration;",
            &[],
        )?
        .l()?;
    environment
        .get_field(&configuration, "orientation", "I")?
        .i()
}

/// Display density (physical pixels per logical point) from the display metrics.
fn display_density(
    environment: &mut JNIEnv<'_>,
    activity: &JObject<'_>,
) -> jni::errors::Result<f32> {
    let resources = resources_of(environment, activity)?;
    let metrics = environment
        .call_method(
            &resources,
            "getDisplayMetrics",
            "()Landroid/util/DisplayMetrics;",
            &[],
        )?
        .l()?;
    environment.get_field(&metrics, "density", "F")?.f()
}

/// Floor padding for a blind portrait spacer: the stock 24dp bar times display
/// density; the caller gates this on the portrait reading.
fn portrait_floor_pixels(
    environment: &mut JNIEnv<'_>,
    activity: &JObject<'_>,
) -> jni::errors::Result<i32> {
    let density = display_density(environment, activity)?;
    Ok((24.0 * density).round().max(0.0) as i32)
}

fn status_bar_height(
    environment: &mut JNIEnv<'_>,
    activity: &JObject<'_>,
) -> jni::errors::Result<i32> {
    let resources = resources_of(environment, activity)?;
    let name = environment.new_string("status_bar_height")?;
    let kind = environment.new_string("dimen")?;
    let framework = environment.new_string("android")?;
    let identifier = environment
        .call_method(
            &resources,
            "getIdentifier",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I",
            &[
                JValue::Object(&name.into()),
                JValue::Object(&kind.into()),
                JValue::Object(&framework),
            ],
        )?
        .i()?;
    if identifier == 0 {
        return Ok(0);
    }
    environment
        .call_method(
            &resources,
            "getDimensionPixelSize",
            "(I)I",
            &[JValue::Int(identifier)],
        )?
        .i()
}

fn notification_manager<'local>(
    environment: &mut JNIEnv<'local>,
    activity: &JObject<'_>,
) -> jni::errors::Result<JObject<'local>> {
    let service_name = environment.new_string("notification")?;
    environment
        .call_method(
            activity,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[JValue::Object(&service_name.into())],
        )?
        .l()
}

/// Ensure the sound-bearing channel exists: Android 8+ ignores per-notification
/// sounds, and a query-then-create step lets a transient failure retry later.
fn ensure_channel(environment: &mut JNIEnv<'_>, manager: &JObject<'_>) -> jni::errors::Result<()> {
    if sdk_version(environment)? < SDK_VERSION_NOTIFICATION_CHANNELS {
        return Ok(());
    }
    let channel_id = environment.new_string(NOTIFICATION_CHANNEL_ID)?;
    let existing = environment
        .call_method(
            manager,
            "getNotificationChannel",
            "(Ljava/lang/String;)Landroid/app/NotificationChannel;",
            &[JValue::Object(&channel_id.into())],
        )?
        .l()?;
    if !existing.is_null() {
        return Ok(());
    }
    create_channel(environment, manager)
}

fn create_channel(environment: &mut JNIEnv<'_>, manager: &JObject<'_>) -> jni::errors::Result<()> {
    if sdk_version(environment)? < SDK_VERSION_NOTIFICATION_CHANNELS {
        return Ok(());
    }
    let channel_id = environment.new_string(NOTIFICATION_CHANNEL_ID)?;
    let channel_name = environment.new_string("Baihua messages")?;
    let channel = environment.new_object(
        "android/app/NotificationChannel",
        "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;I)V",
        &[
            JValue::Object(&channel_id.into()),
            JValue::Object(&channel_name.into()),
            JValue::Int(CHANNEL_IMPORTANCE_HIGH),
        ],
    )?;
    let default_sound = environment
        .call_static_method(
            "android/media/RingtoneManager",
            "getDefaultUri",
            "(I)Landroid/net/Uri;",
            &[JValue::Int(RINGTONE_TYPE_NOTIFICATION)],
        )?
        .l()?;
    environment.call_method(
        &channel,
        "setSound",
        "(Landroid/net/Uri;Landroid/media/AudioAttributes;)V",
        &[
            JValue::Object(&default_sound),
            JValue::Object(&JObject::null()),
        ],
    )?;
    environment.call_method(
        manager,
        "createNotificationChannel",
        "(Landroid/app/NotificationChannel;)V",
        &[JValue::Object(&channel)],
    )?;
    Ok(())
}

fn build_notification<'local>(
    environment: &mut JNIEnv<'local>,
    activity: &JObject<'_>,
    title: &str,
    body: &str,
) -> jni::errors::Result<JObject<'local>> {
    let title_string = environment.new_string(title)?;
    let body_string = environment.new_string(body)?;
    let package_name = environment
        .call_method(activity, "getPackageName", "()Ljava/lang/String;", &[])?
        .l()?;
    let resources = environment
        .call_method(
            activity,
            "getResources",
            "()Landroid/content/res/Resources;",
            &[],
        )?
        .l()?;
    let icon_name = environment.new_string(NOTIFICATION_SMALL_ICON_NAME)?;
    let icon_type = environment.new_string("mipmap")?;
    let small_icon = environment
        .call_method(
            &resources,
            "getIdentifier",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I",
            &[
                JValue::Object(&icon_name.into()),
                JValue::Object(&icon_type.into()),
                JValue::Object(&package_name),
            ],
        )?
        .i()?;
    let builder = if sdk_version(environment)? >= SDK_VERSION_NOTIFICATION_CHANNELS {
        let channel_id = environment.new_string(NOTIFICATION_CHANNEL_ID)?;
        environment.new_object(
            "android/app/Notification$Builder",
            "(Landroid/content/Context;Ljava/lang/String;)V",
            &[JValue::Object(activity), JValue::Object(&channel_id.into())],
        )?
    } else {
        environment.new_object(
            "android/app/Notification$Builder",
            "(Landroid/content/Context;)V",
            &[JValue::Object(activity)],
        )?
    };
    environment.call_method(
        &builder,
        "setContentTitle",
        "(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;",
        &[JValue::Object(&title_string.into())],
    )?;
    environment.call_method(
        &builder,
        "setContentText",
        "(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;",
        &[JValue::Object(&body_string.into())],
    )?;
    environment.call_method(&builder, "setSmallIcon", "(I)V", &[JValue::Int(small_icon)])?;
    environment.call_method(&builder, "setAutoCancel", "(Z)V", &[JValue::Bool(1)])?;
    // Pre-Android-8 devices have no channel, so the sound must come from the
    // notification itself or those phones stay silent.
    environment.call_method(
        &builder,
        "setDefaults",
        "(I)Landroid/app/Notification$Builder;",
        &[JValue::Int(NOTIFICATION_DEFAULT_ALL)],
    )?;
    let built = environment.call_method(&builder, "build", "()Landroid/app/Notification;", &[])?;
    built.l()
}

#[cfg(test)]
mod android_platform_tests {
    use super::spacer_pixels;

    /// The spacer chain: dispatched insets first, then the static portrait height,
    /// then the 24dp floor; landscape trusts dispatch and undecided keeps retrying.
    #[test]
    fn overlap_rule() {
        assert_eq!(
            spacer_pixels(94, Some(94), 94, true, 0),
            (0, true),
            "a window the system pushed down is decided with no spacer"
        );
        assert_eq!(
            spacer_pixels(0, Some(94), 94, true, 0),
            (94, true),
            "a dispatched positive inset pads exactly and decides"
        );
        assert_eq!(
            spacer_pixels(0, Some(2000), 94, true, 0),
            (94, true),
            "a runaway dispatched transient is capped by the framework bar height"
        );
        assert_eq!(
            spacer_pixels(0, Some(40), 0, true, 0),
            (40, true),
            "an unreadable bar must never clamp a dispatched inset to zero"
        );
        assert_eq!(
            spacer_pixels(0, None, 72, true, 0),
            (72, true),
            "portrait pads the static framework height when insets never dispatch"
        );
        assert_eq!(
            spacer_pixels(0, Some(0), 72, true, 0),
            (72, true),
            "portrait distrusts an early dispatched zero and pads the static height"
        );
        assert_eq!(
            spacer_pixels(0, None, 0, true, 72),
            (72, true),
            "portrait falls to the 24dp floor when even the resource is unreadable"
        );
        assert_eq!(
            spacer_pixels(0, None, 0, true, 0),
            (0, false),
            "a portrait with every source blind stays undecided"
        );
        assert_eq!(
            spacer_pixels(0, Some(0), 72, false, 0),
            (0, true),
            "a dispatched landscape zero means the bar really hides and decides"
        );
        assert_eq!(
            spacer_pixels(0, None, 72, false, 0),
            (0, false),
            "landscape never pads blind: undispatched stays undecided for retries"
        );
    }
}
