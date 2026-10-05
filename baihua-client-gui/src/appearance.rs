//! Theme conversion, avatar texturing and the Chinese font install: egui ships
//! Latin glyphs only, so a fallback font is added or Chinese shows as boxes.

use baihua_core::config::{Palette, ThemeColor};
use egui::epaint::text::{FontData, FontInsert, FontPriority, FontTweak, InsertFontFamily};
use egui::{
    Color32, Context, FontFamily, FontId, Stroke, TextureHandle, TextureOptions, Theme, Visuals,
};
use std::borrow::Cow;
use std::collections::HashMap;

/// The name of the CJK fallback font installed into egui (used as a key in the font table)
fn chinese_font_name() -> &'static str {
    "baihua-cjk"
}

/// Warm up one empty frame: the font table only exists after a frame, and an
/// `add_font` call takes effect at the start of the next one.
fn run_one_empty_frame(context: &Context) {
    context
        .run_ui(egui::RawInput::default(), |_ui| {})
        .drop_without_applying_deltas();
}

/// Font table key used only while probing the baseline; it is discarded with
/// the temporary context.
fn probe_font_name() -> &'static str {
    "baihua-cjk-probe"
}

/// Font size used while probing the baseline; only the offset-to-size ratio
/// matters, so the value itself is free.
fn probe_font_size() -> f32 {
    14.0
}

/// Baseline Y of one character relative to the top of its line (egui puts the
/// baseline in `glyph.pos.y`); None when the family cannot shape it.
fn glyph_baseline(context: &Context, family: FontFamily, character: char) -> Option<f32> {
    let font_id = FontId::new(probe_font_size(), family);
    context.fonts_mut(|fonts| {
        let galley = fonts.layout_no_wrap(character.to_string(), font_id, Color32::WHITE);
        galley
            .rows
            .iter()
            .flat_map(|row| row.glyphs.iter())
            .next()
            .map(|glyph| glyph.pos.y)
    })
}

/// Chinese-versus-Latin baseline difference as a `FontTweak::y_offset_factor`:
/// egui centers fallback glyphs on the taller Chinese line box, so they ride high.
fn baseline_offset(bytes: &[u8], face_index: u32) -> f32 {
    let probe = Context::default();
    run_one_empty_frame(&probe);
    probe.add_font(FontInsert::new(
        probe_font_name(),
        FontData {
            font: Cow::Owned(bytes.to_vec()),
            index: face_index,
            tweak: FontTweak::default(),
        },
        vec![InsertFontFamily {
            family: FontFamily::Proportional,
            priority: FontPriority::Lowest,
        }],
    ));
    run_one_empty_frame(&probe);
    let latin = glyph_baseline(&probe, FontFamily::Proportional, 'A');
    // The Han literal is the measurement probe (a real ideograph is needed to
    // read its baseline), not display text.
    let chinese = glyph_baseline(&probe, FontFamily::Proportional, '你');
    match (latin, chinese) {
        (Some(latin), Some(chinese)) => (latin - chinese) / probe_font_size(),
        // The font cannot shape the probe characters: no offset beats a made-up one.
        _ => 0.0,
    }
}

/// Append the Chinese fallback font at lowest priority with the measured baseline
/// correction, once at startup; no font found only means boxes, never a failure.
pub fn install_chinese_font(context: &Context) {
    let Some((bytes, face_index)) = fallback_font() else {
        return;
    };
    let offset_factor = baseline_offset(&bytes, face_index);
    let font_data = FontData {
        font: Cow::Owned(bytes),
        index: face_index,
        tweak: FontTweak {
            y_offset_factor: offset_factor,
            ..Default::default()
        },
    };
    // Both families must be registered: without the monospace slot the Chinese
    // characters in monospace areas would still be boxes.
    context.add_font(FontInsert::new(
        chinese_font_name(),
        font_data,
        vec![
            InsertFontFamily {
                family: FontFamily::Proportional,
                priority: FontPriority::Lowest,
            },
            InsertFontFamily {
                family: FontFamily::Monospace,
                priority: FontPriority::Lowest,
            },
        ],
    ));
}

/// The Chinese font bytes and the face index to install. The system font wins
/// where readable; the iOS sandbox cannot read it, so the embedded subset takes over.
fn fallback_font() -> Option<(Vec<u8>, u32)> {
    if let Some((bytes, face_index)) = baihua_core::fonts::discover_cjk_font() {
        return Some((bytes, face_index));
    }
    #[cfg(target_os = "ios")]
    {
        let bytes = crate::embedded_font_bytes();
        if !bytes.is_empty() {
            // The embedded file is a single-face `.ttf`, so face index 0.
            return Some((bytes.to_vec(), 0));
        }
    }
    None
}

/// Test-only self-check: whether the font table carries the fallback font, so
/// the tests can assert the install actually worked.
#[cfg(test)]
pub fn font_is_installed(context: &Context) -> bool {
    let definitions: egui::FontDefinitions = context.fonts(|fonts| fonts.definitions().clone());
    definitions.font_data.contains_key(chinese_font_name())
}

fn to_color32(color: ThemeColor) -> Color32 {
    match color {
        ThemeColor::Default => Color32::from_gray(24),
        other => {
            let (red, green, blue) = other.to_rgb();
            Color32::from_rgb(red, green, blue)
        }
    }
}

/// Whether a color is light when used as a background; it also picks the light
/// or dark egui baseline. The weights match `ThemeColor::brightness`.
pub(crate) fn is_light_background(color: Color32) -> bool {
    let brightness =
        (color.r() as u32 * 299 + color.g() as u32 * 587 + color.b() as u32 * 114) / 1000;
    brightness >= 128
}

/// Readable foreground for a color used as a background: theme backgrounds only
/// guarantee the background, so the text color is derived from its brightness.
pub(crate) fn contrasting_text(background: Color32) -> Color32 {
    if is_light_background(background) {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

/// Color set for interface rendering; the fields map onto the theme slots (the
/// selection takes the own-bubble color), `Palette::built_in` fills the rest.
#[derive(Clone)]
pub struct Skin {
    pub app_background: Color32,
    pub message_border: Color32,
    pub room_border: Color32,
    pub overlay_border: Color32,
    pub message_text: Color32,
    pub selected_text: Color32,
    pub other_username_text: Color32,
    pub own_username_text: Color32,
    pub time_text: Color32,
    pub hint_text: Color32,
    pub notice_hint_border: Color32,
    pub notice_error_border: Color32,
    pub input_border: Color32,
    pub input_text: Color32,
    pub command_border: Color32,
    pub search_border: Color32,
    /// Selection highlight (selected buttons, rows and marked text): deliberately
    /// the own-message bubble color, so "chosen" reads as one color everywhere.
    pub selection_background: Color32,
    pub search_match_background: Color32,
    pub search_current_match_background: Color32,
    /// Tint applied to the button images: the shipped images are pure white
    pub icon_color: Color32,
}

impl Skin {
    pub fn from(palette: &Palette) -> Self {
        Self {
            app_background: to_color32(palette.app_background),
            message_border: to_color32(palette.message_border),
            room_border: to_color32(palette.room_border),
            overlay_border: to_color32(palette.overlay_border),
            message_text: to_color32(palette.message_text),
            selected_text: to_color32(palette.selected_text),
            other_username_text: to_color32(palette.other_username_text),
            own_username_text: to_color32(palette.own_username_text),
            time_text: to_color32(palette.time_text),
            hint_text: to_color32(palette.hint_text),
            notice_hint_border: to_color32(palette.notice_hint_border),
            notice_error_border: to_color32(palette.notice_error_border),
            input_border: to_color32(palette.input_border),
            input_text: to_color32(palette.input_text),
            command_border: to_color32(palette.command_border),
            search_border: to_color32(palette.search_border),
            // The unification: a clicked-selected button wears the own-bubble color
            // (the conversation fills own bubbles with `own_username_text`).
            selection_background: to_color32(palette.own_username_text),
            search_match_background: to_color32(palette.search_match_background),
            search_current_match_background: to_color32(palette.search_current_match_background),
            icon_color: to_color32(palette.icon_color),
        }
    }

    /// Map the theme onto egui visuals: the baseline (dark or light) follows the
    /// theme background brightness so derived colors (button fills) stay readable.
    pub fn apply_to(&self, context: &Context) {
        let mut visuals = if is_light_background(self.app_background) {
            Visuals::light()
        } else {
            Visuals::dark()
        };
        visuals.panel_fill = self.app_background;
        visuals.window_fill = self.app_background;
        visuals.extreme_bg_color = self.app_background;
        visuals.faint_bg_color = self.app_background;
        visuals.override_text_color = Some(self.message_text);
        visuals.selection.bg_fill = self.selection_background;
        // Selected text must contrast with the selection background: the app
        // background used here made it identical to the input box fill.
        visuals.selection.stroke.color = contrasting_text(self.selection_background);
        // Widget fills and borders come from the theme so hover and press states
        // survive; every button carries a 1-point border to read as clickable.
        for (widget, fill, stroke_color) in [
            (
                &mut visuals.widgets.inactive,
                self.app_background,
                self.room_border,
            ),
            (
                &mut visuals.widgets.hovered,
                widget_hover_fill(self.app_background),
                self.overlay_border,
            ),
            (
                &mut visuals.widgets.active,
                widget_active_fill(self.app_background),
                self.overlay_border,
            ),
        ] {
            widget.weak_bg_fill = fill;
            // `bg_fill` is the checkbox (settings switch) fill: leaving it at
            // the egui gray would swallow the themed border of a resting switch.
            widget.bg_fill = fill;
            widget.bg_stroke = Stroke::new(1.0, stroke_color);
        }
        // Write the theme into BOTH style variants: the effective one at startup
        // may not be first probed, and an unwritten one shows egui defaults.
        for theme in [Theme::Dark, Theme::Light] {
            context.set_visuals_of(theme, visuals.clone());
        }
    }
}

/// Button hover fill: the app background slightly darkened or brightened, which
/// is what the egui built-in widgets mean by a hover state.
fn widget_hover_fill(background: Color32) -> Color32 {
    if is_light_background(background) {
        background.gamma_multiply(0.94)
    } else {
        background.gamma_multiply(1.35)
    }
}

/// Button active fill: slightly more pronounced than hover
fn widget_active_fill(background: Color32) -> Color32 {
    if is_light_background(background) {
        background.gamma_multiply(0.86)
    } else {
        background.gamma_multiply(1.7)
    }
}

/// One avatar job: decode the bytes and scale them. This is pure CPU work that
/// would stutter the render thread, so it goes to the decoder pool below.
struct AvatarDecodeJob {
    /// Cache key: (user ID, side length in pixels)
    key: (String, usize),
    /// Byte fingerprint: if the avatar bytes have changed the fingerprint won't match, so the result is invalidated
    fingerprint: u64,
    /// Original avatar bytes (image file content)
    bytes: Vec<u8>,
    /// Target side length (pixels)
    side: usize,
}

/// Result returned by the decode thread
struct AvatarDecodeResult {
    /// Which image this is (user ID and side length)
    key: (String, usize),
    /// Corresponding byte fingerprint
    fingerprint: u64,
    /// Decoded image; None if the image is corrupted or format is unrecognized
    image: Option<egui::ColorImage>,
}

/// Decoder pool: a fixed number of threads take jobs from one queue and send the
/// results back. They block on an empty queue and exit when the channel closes.
struct AvatarDecoder {
    /// Job sender: render thread only submits, doesn't wait for results
    jobs: std::sync::mpsc::Sender<AvatarDecodeJob>,
    /// Result receiver: render thread receives non-blocking every frame
    results: std::sync::mpsc::Receiver<AvatarDecodeResult>,
}

impl AvatarDecoder {
    fn start() -> Self {
        let (job_sender, job_receiver) = std::sync::mpsc::channel::<AvatarDecodeJob>();
        let (result_sender, result_receiver) = std::sync::mpsc::channel::<AvatarDecodeResult>();
        let shared_jobs = std::sync::Arc::new(std::sync::Mutex::new(job_receiver));
        for _ in 0..decode_worker_count() {
            let jobs = std::sync::Arc::clone(&shared_jobs);
            let results = result_sender.clone();
            std::thread::spawn(move || {
                loop {
                    let job = {
                        let Ok(receiver) = jobs.lock() else {
                            return;
                        };
                        match receiver.recv() {
                            Ok(job) => job,
                            Err(_) => return,
                        }
                    };
                    let result = AvatarDecodeResult {
                        key: job.key,
                        fingerprint: job.fingerprint,
                        image: load_rgba_image(&job.bytes, job.side),
                    };
                    if results.send(result).is_err() {
                        return;
                    }
                }
            });
        }
        Self {
            jobs: job_sender,
            results: result_receiver,
        }
    }
}

/// Number of decode threads: machine parallelism, max 4, min 1
fn decode_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get().min(4))
        .unwrap_or(1)
}

/// Lifecycle state of an avatar texture
enum AvatarTextureState {
    /// Queued for decoding, not returned yet (interface draws placeholder first)
    Decoding(u64),
    /// Decoded but not yet uploaded as a texture (attached to egui on next texture() call)
    Decoded(u64, egui::ColorImage),
    /// Uploaded as a texture, ready to use
    Uploaded(u64, TextureHandle),
    /// Bytes can't decode (unknown format or corrupted file): don't re-queue, interface keeps drawing placeholders
    Unavailable(u64),
}

/// Texture cache for program images (avatars, embedded logo) keyed by (name, side
/// in pixels); decoding runs in the background, changed bytes reload by fingerprint.
#[derive(Default)]
pub struct AvatarTextures {
    /// Which step of the lifecycle each avatar is currently in
    entries: HashMap<(String, usize), AvatarTextureState>,
    /// decoder thread pool; threads are only started when actual decoding is needed
    decoder: Option<AvatarDecoder>,
}

impl AvatarTextures {
    /// Texture for a user at `side` pixels, or None while there is no avatar, it is
    /// decoding or it failed. The first sight of new bytes only queues the job.
    pub fn texture(
        &mut self,
        context: &Context,
        user_id: &str,
        bytes: Option<&[u8]>,
        side: usize,
    ) -> Option<TextureHandle> {
        self.collect_decoded();
        let bytes = bytes?;
        let fingerprint = fingerprint(bytes);
        let key = (user_id.to_string(), side);
        if let Some(state) = self.entries.get_mut(&key) {
            match state {
                AvatarTextureState::Uploaded(cached, handle) if *cached == fingerprint => {
                    return Some(handle.clone());
                }
                AvatarTextureState::Decoded(cached, image) if *cached == fingerprint => {
                    let handle = context.load_texture(
                        format!("baihua-avatar-{user_id}-{side}"),
                        image.clone(),
                        TextureOptions::LINEAR,
                    );
                    *state = AvatarTextureState::Uploaded(fingerprint, handle.clone());
                    return Some(handle);
                }
                // still decoding: let the interface draw a placeholder first
                AvatarTextureState::Decoding(cached) if *cached == fingerprint => return None,
                // bytes can't decode to an image: don't re-queue
                AvatarTextureState::Unavailable(cached) if *cached == fingerprint => return None,
                _ => {}
            }
        }
        self.entries
            .insert(key.clone(), AvatarTextureState::Decoding(fingerprint));
        let decoder = self.decoder.get_or_insert_with(AvatarDecoder::start);
        let _ = decoder.jobs.send(AvatarDecodeJob {
            key,
            fingerprint,
            bytes: bytes.to_vec(),
            side,
        });
        None
    }

    /// Decode a fixed program image (the embedded logo) synchronously and cache the
    /// texture by fingerprint: the bytes are small, so no decoder pool is needed.
    pub(crate) fn embedded_texture(
        &mut self,
        context: &Context,
        name: &str,
        bytes: &[u8],
        side: usize,
    ) -> Option<TextureHandle> {
        self.program_texture(context, name, bytes, side, load_rgba_image)
    }

    /// Texture for one button image: decoded without cropping so the icon keeps its
    /// own aspect ratio, then cached exactly like every other program image.
    pub(crate) fn icon_texture(
        &mut self,
        context: &Context,
        name: &str,
        bytes: &[u8],
        maximum_side: usize,
    ) -> Option<TextureHandle> {
        self.program_texture(context, name, bytes, maximum_side, load_rgba_fitted)
    }

    /// The shared cache path for program images: hit by (name, side) plus byte
    /// fingerprint, otherwise decode with `decode` and upload a fresh texture.
    fn program_texture(
        &mut self,
        context: &Context,
        name: &str,
        bytes: &[u8],
        side: usize,
        decode: fn(&[u8], usize) -> Option<egui::ColorImage>,
    ) -> Option<TextureHandle> {
        self.collect_decoded();
        let key = (name.to_string(), side);
        let fingerprint = fingerprint(bytes);
        if let Some(AvatarTextureState::Uploaded(cached, handle)) = self.entries.get(&key)
            && *cached == fingerprint
        {
            return Some(handle.clone());
        }
        let image = decode(bytes, side)?;
        let handle = context.load_texture(name.to_string(), image, TextureOptions::LINEAR);
        self.entries.insert(
            key,
            AvatarTextureState::Uploaded(fingerprint, handle.clone()),
        );
        Some(handle)
    }

    /// Drop one user's entry; a theme change needs no clearing because the cache
    /// is keyed by byte fingerprint and side.
    pub fn forget(&mut self, user_id: &str) {
        self.entries
            .retain(|(cached_user, _), _| cached_user != user_id);
    }

    /// collect results returned by the decode thread into the table (once per frame, non-blocking)
    fn collect_decoded(&mut self) {
        let Some(decoder) = &self.decoder else {
            return;
        };
        while let Ok(result) = decoder.results.try_recv() {
            let state = match result.image {
                Some(image) => AvatarTextureState::Decoded(result.fingerprint, image),
                None => AvatarTextureState::Unavailable(result.fingerprint),
            };
            self.entries.insert(result.key, state);
        }
    }
}

/// Cheap byte fingerprint (length plus the first and last few bytes), enough to
/// tell whether an avatar changed.
fn fingerprint(bytes: &[u8]) -> u64 {
    let mut value = bytes.len() as u64;
    for byte in bytes.iter().take(8).chain(bytes.iter().rev().take(8)) {
        value = value.wrapping_mul(131).wrapping_add(*byte as u64);
    }
    value
}

/// Decode an image scaled so its longest side is `side`, aspect ratio kept: button
/// icons are not square and must not be cropped into one.
fn load_rgba_fitted(bytes: &[u8], side: usize) -> Option<egui::ColorImage> {
    use image::GenericImageView;
    let decoded = image::load_from_memory(bytes).ok()?;
    let resized = decoded.resize(
        side as u32,
        side as u32,
        image::imageops::FilterType::Lanczos3,
    );
    let (width, height) = resized.dimensions();
    let rgba = resized.to_rgba8().into_raw();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [width as usize, height as usize],
        &rgba,
    ))
}

/// Decode and scale to a `side` by `side` RGBA image, cropping to a centered
/// square first so the avatar does not deform.
fn load_rgba_image(bytes: &[u8], side: usize) -> Option<egui::ColorImage> {
    use image::GenericImageView;
    let decoded = image::load_from_memory(bytes).ok()?;
    let (width, height) = decoded.dimensions();
    let side_source = width.min(height);
    let cropped = decoded.crop_imm(
        (width - side_source) / 2,
        (height - side_source) / 2,
        side_source,
        side_source,
    );
    let resized = cropped.resize_to_fill(
        side as u32,
        side as u32,
        image::imageops::FilterType::Lanczos3,
    );
    let rgba = resized.to_rgba8().into_raw();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [side, side],
        &rgba,
    ))
}

#[cfg(test)]
mod font_tests {
    use super::{font_is_installed, install_chinese_font, probe_font_size, run_one_empty_frame};
    use egui::epaint::text::{FontData, FontInsert, FontPriority, FontTweak, InsertFontFamily};
    use egui::{Color32, Context, FontFamily, FontId};
    use std::borrow::Cow;

    /// A character that "definitely has no glyph": Unicode non-character code point, no font should accept it.
    /// Use the glyph rendered by it as the baseline for "replacement box".
    fn missing_character() -> char {
        '\u{10FFFD}'
    }

    /// A glyph's drawing signature (advance, rect size, offset) against the
    /// missing-character baseline: the replacement box is identical for all of them.
    #[derive(Debug, PartialEq)]
    struct GlyphSignature {
        advance_width: f32,
        texture_size: [f32; 2],
        texture_offset: [f32; 2],
    }

    /// place one character, get back its glyph features; return None if it can't be placed
    fn glyph_signature(
        context: &Context,
        family: FontFamily,
        character: char,
    ) -> Option<GlyphSignature> {
        let font_id = FontId::new(14.0, family);
        context.fonts_mut(|fonts| {
            let galley =
                fonts.layout_no_wrap(character.to_string(), font_id.clone(), egui::Color32::WHITE);
            let glyph = galley
                .rows
                .iter()
                .flat_map(|row| row.glyphs.iter())
                .next()?;
            Some(GlyphSignature {
                advance_width: glyph.advance_width,
                texture_size: [glyph.uv_rect.size.x, glyph.uv_rect.size.y],
                texture_offset: [glyph.uv_rect.offset.x, glyph.uv_rect.offset.y],
            })
        })
    }

    /// whether a character in a certain font family is rendered as a real glyph (not a replacement box identical to the missing-character glyph)
    fn has_real_glyph(context: &Context, family: FontFamily, character: char) -> bool {
        let Some(character_glyph) = glyph_signature(context, family.clone(), character) else {
            return false;
        };
        let Some(missing_glyph) = glyph_signature(context, family, missing_character()) else {
            return false;
        };
        character_glyph != missing_glyph
    }

    /// A context that has installed CJK fonts and run a frame for the fonts to take effect
    fn aligned_context() -> Context {
        let context = warmed_up_context();
        install_chinese_font(&context);
        run_one_empty_frame(&context);
        context
    }

    /// A context that has already initialized the font table
    fn warmed_up_context() -> Context {
        let context = Context::default();
        run_one_empty_frame(&context);
        context
    }

    /// After the install every Chinese character must shape into a real glyph in
    /// both families; skipped when this machine offers no fallback font at all.
    #[test]
    fn glyphs_after_install() {
        if baihua_core::fonts::discover_cjk_font().is_none() {
            return;
        }
        let context = aligned_context();
        assert!(
            font_is_installed(&context),
            "the installed font table must contain the fallback font"
        );
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            for character in "你好百花客户端".chars() {
                assert!(
                    has_real_glyph(&context, family.clone(), character),
                    "{character:?} in {family:?} must shape into a real glyph, not a box"
                );
            }
        }
    }

    /// Without the install Chinese is only replacement boxes: the converse that
    /// proves the test above really measures the font and not an egui default.
    #[test]
    fn boxes_without_font() {
        let context = warmed_up_context();
        assert!(
            !font_is_installed(&context),
            "the default font table must not carry the fallback font"
        );
        for character in "你好".chars() {
            assert!(
                !has_real_glyph(&context, FontFamily::Proportional, character),
                "{character:?} without the font must measure as the replacement box"
            );
        }
    }

    /// Installing the fallback must not touch Latin letters and digits: the
    /// built-in font is still consulted first, so their shapes stay identical.
    #[test]
    fn latin_keeps_default() {
        if baihua_core::fonts::discover_cjk_font().is_none() {
            return;
        }
        let before = warmed_up_context();
        let after = aligned_context();
        for character in "Hello123".chars() {
            let before_glyph = glyph_signature(&before, FontFamily::Proportional, character);
            let after_glyph = glyph_signature(&after, FontFamily::Proportional, character);
            assert_eq!(
                before_glyph, after_glyph,
                "latin letters and digits must not change shape because of the fallback; the culprit is {character:?}"
            );
        }
    }

    /// Glyph placement: baseline Y and bitmap-top Y, both from the line top. Their
    /// difference depends only on the font, so the unaligned run can measure it.
    struct GlyphPlacement {
        baseline: f32,
        ink_top: f32,
    }

    /// place one character, get back its position; return None if it can't be placed
    fn glyph_placement(
        context: &Context,
        family: FontFamily,
        character: char,
    ) -> Option<GlyphPlacement> {
        let font_id = FontId::new(probe_font_size(), family);
        context.fonts_mut(|fonts| {
            let galley = fonts.layout_no_wrap(character.to_string(), font_id, Color32::WHITE);
            let glyph = galley
                .rows
                .iter()
                .flat_map(|row| row.glyphs.iter())
                .next()?;
            Some(GlyphPlacement {
                baseline: glyph.pos.y,
                ink_top: glyph.pos.y + glyph.uv_rect.offset.y,
            })
        })
    }

    /// Install CJK font without baseline correction: as a "before fix" control to measure how much CJK was pushed up.
    /// Returns None if no CJK font on the system (skip assertions on this machine).
    fn unaligned_context() -> Option<Context> {
        let (bytes, face_index) = baihua_core::fonts::discover_cjk_font()?;
        let context = warmed_up_context();
        context.add_font(FontInsert::new(
            "baihua-cjk-unaligned",
            FontData {
                font: Cow::Owned(bytes),
                index: face_index,
                tweak: FontTweak::default(),
            },
            vec![
                InsertFontFamily {
                    family: FontFamily::Proportional,
                    priority: FontPriority::Lowest,
                },
                InsertFontFamily {
                    family: FontFamily::Monospace,
                    priority: FontPriority::Lowest,
                },
            ],
        ));
        run_one_empty_frame(&context);
        Some(context)
    }

    /// Baseline regression: the Han baseline must sit level with the Latin one in
    /// both families, and a glyph that poked out of the line box must stay inside.
    #[test]
    fn baseline_alignment() {
        let Some(unaligned) = unaligned_context() else {
            return;
        };
        let aligned = aligned_context();
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            let latin = glyph_placement(&aligned, family.clone(), 'A')
                .expect("latin letters shape in any font table");
            let before = glyph_placement(&unaligned, family.clone(), '你')
                .expect("Han glyphs must shape once the font is installed");
            let after = glyph_placement(&aligned, family.clone(), '你')
                .expect("Han glyphs must shape once the font is installed");
            let ink_top_above_baseline = before.ink_top - before.baseline;
            let chinese_baseline = after.ink_top - ink_top_above_baseline;
            assert!(
                (chinese_baseline - latin.baseline).abs() <= 0.5,
                "the Han baseline in {family:?} must sit level with the latin one: latin {}, Han {}",
                latin.baseline,
                chinese_baseline
            );
            if before.ink_top < 0.0 {
                assert!(
                    after.ink_top >= 0.0,
                    "after the fix Han glyphs must not poke above the row box, bitmap top was {}",
                    after.ink_top
                );
            }
        }
    }

    /// The settings button uses the gear character, so it must shape into a real
    /// glyph; skipped when this machine offers no fallback font.
    #[test]
    fn settings_glyph() {
        if baihua_core::fonts::discover_cjk_font().is_none() {
            return;
        }
        let context = aligned_context();
        for character in crate::app::settings_button_text().chars() {
            assert!(
                has_real_glyph(&context, FontFamily::Proportional, character),
                "{character:?} on the settings button must render as a real glyph, not a box"
            );
        }
    }

    /// iOS regression: the embedded subset is the only font source there, so it
    /// must be a plausible subset size and shape the interface's own strings.
    #[cfg(target_os = "ios")]
    #[test]
    fn embedded_font() {
        let bytes = crate::embedded_font_bytes();
        assert!(
            bytes.len() > 100_000,
            "the embedded subset must carry real glyphs, it is {} bytes",
            bytes.len()
        );
        assert!(
            bytes.len() < 12_000_000,
            "the embedded subset must stay a subset, it is {} bytes",
            bytes.len()
        );
        let context = warmed_up_context();
        install_chinese_font(&context);
        run_one_empty_frame(&context);
        assert!(
            font_is_installed(&context),
            "the embedded font must be installed on iOS"
        );
        for character in "百花客户端，。！？".chars() {
            assert!(
                has_real_glyph(&context, FontFamily::Proportional, character),
                "{character:?} must shape into a real glyph with the embedded font"
            );
        }
    }
}

#[cfg(test)]
mod theme_contrast_tests {
    use super::{Skin, contrasting_text};
    use baihua_core::config::{Palette, ThemeColor};
    use egui::{Color32, Context};

    /// weighted grayscale brightness: used when asserting "are these two colors bright enough", formula matches the table in the implementation
    fn brightness(color: Color32) -> u32 {
        (color.r() as u32 * 299 + color.g() as u32 * 587 + color.b() as u32 * 114) / 1000
    }

    /// light theme (same values as light.json in the repo: light cream background + dark gray text)
    fn light_palette() -> Palette {
        let mut palette = Palette::built_in();
        palette.app_background = ThemeColor::Rgb(0xEF, 0xEB, 0xE2);
        palette.message_text = ThemeColor::Rgb(0x2B, 0x2B, 0x2B);
        palette.input_text = ThemeColor::Rgb(0x2B, 0x2B, 0x2B);
        palette.selection_background = ThemeColor::Rgb(0xFF, 0xD5, 0x4F);
        palette
    }

    /// dark theme
    fn dark_palette() -> Palette {
        let mut palette = Palette::built_in();
        palette.app_background = ThemeColor::Rgb(0x1E, 0x1F, 0x22);
        palette.message_text = ThemeColor::Rgb(0xDB, 0xDE, 0xE1);
        palette.input_text = ThemeColor::Rgb(0xF2, 0xF3, 0xF5);
        palette.selection_background = ThemeColor::Rgb(0x4A, 0x6F, 0xA5);
        palette
    }

    /// Regression for "the selected text matches the input box fill": egui paints
    /// it with `selection.stroke.color`, which must contrast with the selection.
    #[test]
    fn selection_contrast() {
        for (name, palette) in [("light", light_palette()), ("dark", dark_palette())] {
            let skin = Skin::from(&palette);
            let context = Context::default();
            skin.apply_to(&context);
            // `set_visuals_of` writes one variant per theme; read back the same one.
            let visuals = context.style_of(context.theme()).visuals.clone();
            let selected_text = visuals.selection.stroke.color;
            let selection_background = visuals.selection.bg_fill;
            assert_eq!(
                selection_background, skin.own_username_text,
                "the {name} theme must paint selections in the own-message bubble color"
            );
            let difference = brightness(selected_text).abs_diff(brightness(selection_background));
            assert!(
                difference >= 128,
                "the {name} theme keeps selected text too close to the selection background: text {selected_text:?}, background {selection_background:?}"
            );
            assert_ne!(
                selected_text, skin.app_background,
                "in the {name} theme the selected text color must no longer be the application background (that is the input box fill)"
            );
        }
    }

    /// A light theme must use the light egui baseline and a dark one the dark
    /// baseline, otherwise derived widget colors stay dark-on-dark.
    #[test]
    fn widget_lightness() {
        for (name, palette, want_light) in [
            ("light", light_palette(), true),
            ("dark", dark_palette(), false),
        ] {
            let skin = Skin::from(&palette);
            let context = Context::default();
            skin.apply_to(&context);
            let widget_background = context
                .style_of(context.theme())
                .visuals
                .widgets
                .inactive
                .weak_bg_fill;
            assert_eq!(
                brightness(widget_background) >= 128,
                want_light,
                "the {name} theme has the wrong lightness for the widget background: {widget_background:?}"
            );
        }
    }

    /// The helper behind every derived text color: light backgrounds pair with
    /// black text, dark backgrounds with white.
    #[test]
    fn contrast_is_readable() {
        assert_eq!(
            contrasting_text(Color32::from_rgb(0xFF, 0xD5, 0x4F)),
            Color32::BLACK
        );
        assert_eq!(
            contrasting_text(Color32::from_rgb(0x4A, 0x6F, 0xA5)),
            Color32::WHITE
        );
    }

    /// Both style variants must carry the theme at startup (panel, input, button
    /// fill and border); writing only the current one left egui defaults visible.
    #[test]
    fn theme_in_both_styles() {
        for (name, palette) in [("light", light_palette()), ("dark", dark_palette())] {
            let skin = Skin::from(&palette);
            let context = Context::default();
            skin.apply_to(&context);
            for theme in [egui::Theme::Dark, egui::Theme::Light] {
                let visuals = context.style_of(theme).visuals.clone();
                assert_eq!(
                    visuals.panel_fill, skin.app_background,
                    "the {name} theme did not take effect in its {theme:?} style"
                );
                assert_eq!(
                    visuals.extreme_bg_color, skin.app_background,
                    "the field fill must follow the theme or it collides with the theme text color"
                );
                assert_eq!(
                    visuals.widgets.inactive.weak_bg_fill, skin.app_background,
                    "the {name} theme must supply the resting button fill"
                );
                assert_eq!(
                    visuals.widgets.inactive.bg_fill, skin.app_background,
                    "the {name} theme switch fill must also come from the theme, else it stays the egui gray and the themed border disappears into it"
                );
                assert!(
                    visuals.widgets.inactive.bg_stroke.width > 0.0,
                    "the {name} theme leaves the resting button borderless in its {theme:?} style"
                );
            }
        }
    }

    /// Render-level counterpart: a resting button and switch must really paint the
    /// theme border (and the switch its fill) this frame, which styles cannot show.
    #[test]
    fn resting_borders() {
        fn stroked_rects(context: &Context, label: &str, switch: bool) -> Vec<(Color32, Color32)> {
            let mut checked = false;
            let mut output = context.run_ui(egui::RawInput::default(), |ui| {
                if switch {
                    let _ = ui.checkbox(&mut checked, label);
                } else {
                    let _ = ui.button(label);
                }
            });
            // Nothing else consumes the texture delta, and egui panics when a
            // `TexturesDelta` is dropped uncleared.
            output.textures_delta.clear();
            output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Rect(rect) if rect.stroke.width > 0.0 => {
                        Some((rect.stroke.color, rect.fill))
                    }
                    _ => None,
                })
                .collect()
        }
        let context = Context::default();
        let skin = Skin::from(&Palette::built_in());
        skin.apply_to(&context);
        let buttons: Vec<Color32> = stroked_rects(&context, "confirm", false)
            .into_iter()
            .map(|(border, _fill)| border)
            .collect();
        assert!(
            buttons.contains(&skin.room_border),
            "a resting button must paint the themed border this frame (borders {buttons:?}, want {:?})",
            skin.room_border
        );
        let switches = stroked_rects(&context, "show user id", true);
        assert!(
            switches
                .iter()
                .any(|(border, _)| *border == skin.room_border),
            "a resting switch must paint the themed border this frame (rects {switches:?}, want {:?})",
            skin.room_border
        );
        assert!(
            switches
                .iter()
                .any(|(_, fill)| *fill == skin.app_background),
            "the switch box fill must come from the theme (rects {switches:?}, want {:?})",
            skin.app_background
        );
    }
}

#[cfg(test)]
mod avatar_decode_tests {
    use super::AvatarTextures;
    use egui::Context;

    /// create a real image as an avatar: an 8×8 solid color PNG (encoded with the image crate itself, no extra dependencies)
    fn sample_png() -> Vec<u8> {
        let pixels = image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 40, 40, 255]));
        let mut encoded: Vec<u8> = Vec::new();
        pixels
            .write_to(
                &mut std::io::Cursor::new(&mut encoded),
                image::ImageFormat::Png,
            )
            .expect("encoding a PNG in memory cannot fail");
        encoded
    }

    /// Decoding never runs on the render thread: new bytes only queue a job, the
    /// texture appears once the pool answers, and broken bytes stay a placeholder.
    #[test]
    fn decode_pipeline() {
        let context = Context::default();
        let mut textures = AvatarTextures::default();
        let bytes = sample_png();
        assert!(
            textures
                .texture(&context, "user-1", Some(&bytes), 32)
                .is_none(),
            "the first time avatar bytes appear a placeholder must be painted"
        );
        let mut handle = None;
        for _ in 0..500 {
            if let Some(texture) = textures.texture(&context, "user-1", Some(&bytes), 32) {
                handle = Some(texture);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            handle.is_some(),
            "once the pool finishes decoding a texture must appear (each size decodes once)"
        );
        let broken = "this is not an image".as_bytes().to_vec();
        assert!(
            textures
                .texture(&context, "user-2", Some(&broken), 32)
                .is_none()
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(
            textures
                .texture(&context, "user-2", Some(&broken), 32)
                .is_none(),
            "a broken image only ever gets the placeholder"
        );
    }
}
