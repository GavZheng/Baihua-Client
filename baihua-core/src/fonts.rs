//! CJK font discovery.
//!
//! egui's default fonts (the ones bundled with the `default_fonts` feature) only have Latin glyphs, so Chinese characters in the interface will
//! all show as tofu blocks. This module is responsible for finding a font with CJK glyphs on the system and reading it into bytes,
//! and handing them to the GUI to install into egui's font table.
//!
//! This module is "shared code": it only deals with paths and bytes, not recognizing any interface type (no egui/epaint),
//! so both the TUI and GUI can use it and it's easy to test separately. The GUI converts the bytes obtained here into
//! `egui::FontData` and appends it as a fallback font; the TUI does not need this at the moment (the TUI relies on the system terminal's own font selection).

use std::path::PathBuf;

/// One font candidate: the file path and the index of the face inside it.
///
/// A font file may be a `.ttc` collection holding several faces in order (for example multiple weights of one family).
/// Index `0` is usually the regular face, matching the meaning of egui's `FontData::index`.
pub struct FontFile {
    /// Full on-disk path of the font file
    pub path: PathBuf,
    /// Which face inside the file to take (0 for `.ttf`/`.otf`, the collection order for `.ttc`)
    pub face_index: u32,
}

/// Font candidates that carry Han glyphs, per platform (highest priority first).
///
/// Only lists paths, never checks existence; `discover_cjk_font` picks the one that actually works.
/// The rule is "the system's usual body font first": macOS prefers PingFang then Hiragino Sans, Windows
/// prefers Microsoft YaHei then DengXian, Linux prefers Source Han Sans then WenQuanYi Zen Hei; a few near-universal fallbacks follow.
pub fn cjk_font_candidates() -> Vec<FontFile> {
    let mut candidates: Vec<FontFile> = Vec::new();

    // macOS: PingFang is the system interface font with the fullest glyph coverage; Hiragino Sans and Heiti SC fall back in order
    for (path, face_index) in [
        ("/System/Library/Fonts/PingFang.ttc", 0),
        ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
        ("/System/Library/Fonts/STHeiti Light.ttc", 0),
        ("/System/Library/Fonts/STHeiti Medium.ttc", 0),
        ("/System/Library/Fonts/Supplemental/Songti.ttc", 0),
    ] {
        candidates.push(FontFile {
            path: PathBuf::from(path),
            face_index,
        });
    }

    // Windows: Microsoft YaHei has the widest coverage; DengXian and SimHei fall back
    for (path, face_index) in [
        ("C:/Windows/Fonts/msyh.ttc", 0),
        ("C:/Windows/Fonts/msyh.ttf", 0),
        ("C:/Windows/Fonts/deng.ttf", 0),
        ("C:/Windows/Fonts/simhei.ttf", 0),
        ("C:/Windows/Fonts/simsun.ttc", 0),
    ] {
        candidates.push(FontFile {
            path: PathBuf::from(path),
            face_index,
        });
    }

    // Android: the system ships the Noto family (newer releases use NotoSansSC, older ones the
    // NotoSansCJK collection, older still DroidSansFallback); these paths exist only on devices or
    // emulators, and desktop platforms simply skip them without noise
    for (path, face_index) in [
        ("/system/fonts/NotoSansCJK-Regular.ttc", 0),
        ("/system/fonts/NotoSansSC-Regular.otf", 0),
        ("/system/fonts/DroidSansFallback.ttf", 0),
    ] {
        candidates.push(FontFile {
            path: PathBuf::from(path),
            face_index,
        });
    }

    // iOS deliberately has no candidate here: an app sandbox cannot read
    // `/System/Library/Fonts` (an earlier comment claimed the shared macOS paths
    // covered iOS — the device proved otherwise, Chinese was all tofu boxes).
    // The GUI ships a subset font inside its own binary for iOS instead, see
    // `baihua_client_gui::embedded_cjk_font_bytes` and
    // `assets/fonts/build-subset.py`.

    // Linux: Source Han Sans is the most common; WenQuanYi Zen Hei and any installed Noto fall back in order
    for (path, face_index) in [
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
        ("/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc", 0),
        (
            "/usr/share/fonts/opentype/source-han-sans/SourceHanSans-Regular.otf",
            0,
        ),
        ("/usr/share/fonts/truetype/arphic/uming.ttc", 0),
        ("/usr/local/share/fonts/NotoSansCJK-Regular.ttc", 0),
    ] {
        candidates.push(FontFile {
            path: PathBuf::from(path),
            face_index,
        });
    }

    candidates
}

/// Pick the first candidate that exists and reads, returning its bytes and face index.
///
/// A failed read (permissions, locked file) is not fatal: the next candidate is tried.
/// With no candidate readable at all it returns None and the interfaces degrade to "no CJK font"
/// (Chinese shows as placeholder boxes) instead of failing to start.
pub fn discover_cjk_font() -> Option<(Vec<u8>, u32)> {
    for candidate in cjk_font_candidates() {
        if !candidate.path.is_file() {
            continue;
        }
        match std::fs::read(&candidate.path) {
            Ok(bytes) if !bytes.is_empty() => return Some((bytes, candidate.face_index)),
            _ => continue,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The candidate table must be ordered per platform and free of duplicates, so no file is read twice
    #[test]
    fn candidates_are_not_empty_and_unique() {
        let candidates = cjk_font_candidates();
        assert!(
            !candidates.is_empty(),
            "every platform needs at least a few CJK font candidates"
        );
        let mut seen: Vec<PathBuf> = Vec::new();
        for candidate in &candidates {
            assert!(
                !seen.contains(&candidate.path),
                "a duplicate font path appeared in the candidates: {:?}",
                candidate.path
            );
            seen.push(candidate.path.clone());
        }
    }

    /// Probe on the development machine as it really is: a hit must yield non-empty bytes, a miss must not panic
    #[test]
    fn discovery_returns_readable_bytes_or_nothing() {
        match discover_cjk_font() {
            Some((bytes, _face_index)) => {
                assert!(!bytes.is_empty(), "read font bytes must not be empty")
            }
            None => {
                // This machine has none of the candidates: a permitted degradation, the function itself must not fail
            }
        }
    }
}
