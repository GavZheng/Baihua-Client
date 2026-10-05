//! Client version updates: query the release feed, compare versions, download and verify packages.
//!
//! Release artifacts live on GitHub Releases (repository binder-organization/Baihua-Client). The rules:
//! - Each of the three ends has its own update channel with its tag prefix: `cli-v` (command line, `baihua`), `gui-v` (graphical, `baihua-gui`),
//!   `tui-v` (terminal, `baihua-tui`); the three version numbers evolve independently, and check/update runs per end;
//! - One package per platform, named package prefix + version + target platform triple;
//!   the prefixes are `baihua-cli-`, `baihua-gui-`, `baihua-tui-` (the terminal version also
//!   accepts the historic package names `baihua-<version>-<platform>` because the feed already carries them),
//!   the command line and terminal ends use `.zip` on Windows and `.tar.gz` elsewhere,
//!   the graphical end ships `.dmg` (macOS), `.msi` (Windows) and `.AppImage` (Linux);
//! - Every package must sit next to a same-named `.sha256` digest file (the two-column
//!   `<digest>  <filename>` form of `sha256sum` is allowed); the digest is verified before anything is
//!   written out and a mismatch discards the download — a wholesale feed swap would have to replace the digests too, so this check is the minimum guarantee that a man in the middle cannot swap the package under auto-update;
//! - Package selection **walks the releases newest to oldest**: when the newest release has no package
//!   for this platform (an old workflow once poisoned the feed: `tui-v0.1.0` carried an asset name with
//!   an empty version and the old executable name `baihua`), older releases are tried, installing the newest
//!   installable release instead of jamming the whole channel; unrecognized names (like `baihua--<platform>`) are never claimed — such archives would install the wrong end.

use crate::paths;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The necessary information about one install package on the release feed.
#[derive(Debug, Clone, PartialEq)]
pub struct ReleasePackage {
    /// The version with the channel prefix removed, compared directly against the built-in version
    pub version: String,
    pub tag: String,
    pub file_name: String,
    pub download_url: String,
    pub size_bytes: u64,
}

/// The outcome of one update check.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateCheck {
    /// Already up to date: carries the newest tag seen on the feed and the package file name under it,
    /// so `update --check` can answer "what exactly did you see and why did you decide there is no update"
    UpToDate {
        newest_tag: String,
        newest_assets: Vec<String>,
    },
    /// An update is available
    Available(ReleasePackage),
    /// The check could not finish (network, parsing, no package for this platform), carrying a displayable reason
    Unavailable(String),
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    /// Draft releases are invisible to everyone but the author and must never count as available in a client
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Release channels: the command line, graphical and terminal ends each run their own update stream. The tag prefixes differ (`cli-v`, `gui-v`, `tui-v`)
/// and so do the package prefixes, so even shared releases never install the wrong program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseChannel {
    /// Command line end: tag `cli-v<version>`, package `baihua-cli-<version>-<platform>.tar.gz`
    CommandLine,
    /// Graphical end: tag `gui-v<version>`, package `baihua-gui-<version>-<platform>.tar.gz`
    Graphical,
    /// Terminal end: tag `tui-v<version>`, package `baihua-tui-<version>-<platform>.tar.gz` (also accepts the historic `baihua-<version>-<platform>`)
    Terminal,
}

impl ReleaseChannel {
    /// Only tags with this prefix belong to this channel; everything else (server tags, the other clients) is ignored
    fn tag_prefix(self) -> String {
        match self {
            ReleaseChannel::CommandLine => "cli-v".to_string(),
            ReleaseChannel::Graphical => "gui-v".to_string(),
            ReleaseChannel::Terminal => "tui-v".to_string(),
        }
    }

    /// Whether an attachment name is this channel's package. Selection must discriminate by channel or the ends would claim each other's packages.
    /// The terminal end additionally accepts the historic `baihua-<version>-<platform>`: only when a version digit follows `baihua-` directly,
    /// so `baihua-cli-...` and `baihua-gui-...` are never claimed by the terminal channel.
    fn package_name_matches(self, name: &str) -> bool {
        match self {
            ReleaseChannel::CommandLine => name.starts_with("baihua-cli-"),
            ReleaseChannel::Graphical => name.starts_with("baihua-gui-"),
            ReleaseChannel::Terminal => {
                name.starts_with("baihua-tui-")
                    || name
                        .strip_prefix("baihua-")
                        .and_then(|remainder| remainder.chars().next())
                        .is_some_and(|character| character.is_ascii_digit())
            }
        }
    }

    /// Whether an attachment is an installable package for this channel. The
    /// command line and terminal ends ship archives; the graphical end ships a
    /// macOS disk image, a Windows installer and a Linux AppImage, and still
    /// accepts the historic archives so an older release remains installable.
    fn accepts_asset(self, name: &str) -> bool {
        if !self.package_name_matches(name) {
            return false;
        }
        let archive = name.ends_with(".tar.gz") || name.ends_with(".zip");
        match self {
            ReleaseChannel::CommandLine | ReleaseChannel::Terminal => archive,
            ReleaseChannel::Graphical => {
                archive
                    || name.ends_with(".dmg")
                    || name.ends_with(".msi")
                    || name.ends_with(".AppImage")
            }
        }
    }

    /// The name this channel shows in command line and interface reports (per-end output of `baihua update`)
    pub fn display_name(self) -> &'static str {
        match self {
            ReleaseChannel::CommandLine => "command line",
            ReleaseChannel::Graphical => "graphical",
            ReleaseChannel::Terminal => "terminal",
        }
    }
}

/// The release feed address. The GitHub API demands a User-Agent (a plain 403 otherwise); the request carries one.
/// When `BAIHUA_RELEASE_FEED` is set that address is used instead (a local fake feed or an intranet mirror),
/// so the whole "check, download, verify, unpack, install" chain can be exercised without publishing to the real repository.
fn releases_api_url() -> String {
    if let Some(override_url) = std::env::var_os("BAIHUA_RELEASE_FEED") {
        let url = override_url.to_string_lossy().trim().to_string();
        if !url.is_empty() {
            return url;
        }
    }
    "https://api.github.com/repos/binder-organization/Baihua-Client/releases?per_page=10"
        .to_string()
}

/// The fragment of the current platform in package file names (matching the naming in BUILDING.md).
fn current_platform_token() -> String {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin".to_string(),
        ("macos", "x86_64") => "x86_64-apple-darwin".to_string(),
        ("windows", "x86_64") => "x86_64-pc-windows-msvc".to_string(),
        ("windows", "aarch64") => "aarch64-pc-windows-msvc".to_string(),
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu".to_string(),
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu".to_string(),
        (operating_system, architecture) => format!("{architecture}-{operating_system}"),
    }
}

/// Compare two version numbers: dotted major/minor/patch numerically first, then prerelease tags (a prerelease precedes its release).
/// Unparseable pieces count as 0, so "0.1.0-alpha.2" < "0.1.0".
pub fn is_version_newer(candidate: &str, current: &str) -> bool {
    fn split(text: &str) -> (Vec<u64>, Option<String>) {
        let (numeric, pre_release) = match text.split_once('-') {
            Some((numeric, pre_release)) => (numeric, Some(pre_release.to_string())),
            None => (text, None),
        };
        let numbers = numeric
            .split('.')
            .map(|segment| segment.parse::<u64>().unwrap_or(0))
            .collect();
        (numbers, pre_release)
    }
    let (candidate_numbers, candidate_pre) = split(candidate.trim_start_matches('v'));
    let (current_numbers, current_pre) = split(current.trim_start_matches('v'));
    // When the two versions carry different numbers of segments, the shorter one is padded with zeros ("0.1" equals "0.1.0")
    let digit_count = candidate_numbers.len().max(current_numbers.len());
    for position in 0..digit_count {
        let candidate_value = *candidate_numbers.get(position).unwrap_or(&0);
        let current_value = *current_numbers.get(position).unwrap_or(&0);
        if candidate_value != current_value {
            return candidate_value > current_value;
        }
    }
    match (&candidate_pre, &current_pre) {
        (Some(candidate_marker), Some(current_marker)) => {
            pre_release_order(candidate_marker, current_marker)
        }
        // For the same numeric version, the one carrying a prerelease tag is older than the plain release
        (Some(_), None) => false,
        (None, Some(_)) => true,
        (None, None) => false,
    }
}

/// Prerelease tags compare segment by segment: "alpha.10" must beat "alpha.2" (a pure string compare flips this),
/// so segments that parse as numbers on both sides compare numerically and the rest compare as text.
fn pre_release_order(candidate_marker: &str, current_marker: &str) -> bool {
    let candidate_parts: Vec<&str> = candidate_marker.split('.').collect();
    let current_parts: Vec<&str> = current_marker.split('.').collect();
    for position in 0..candidate_parts.len().max(current_parts.len()) {
        let candidate_part = candidate_parts.get(position).copied().unwrap_or_default();
        let current_part = current_parts.get(position).copied().unwrap_or_default();
        let candidate_number = candidate_part.parse::<u64>().ok();
        let current_number = current_part.parse::<u64>().ok();
        let ordering = match (candidate_number, current_number) {
            (Some(candidate_value), Some(current_value)) => candidate_value.cmp(&current_value),
            _ => candidate_part.cmp(current_part),
        };
        if ordering != std::cmp::Ordering::Equal {
            return ordering == std::cmp::Ordering::Greater;
        }
    }
    false
}

/// Fetch the release feed JSON. Kept apart from "how to pick a package" so tests can feed a fake feed to the picking logic.
fn fetch_releases() -> Result<Vec<GitHubRelease>, String> {
    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(concat!("baihua-client/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return Err(format!("cannot create the downloader: {error}"));
        }
    };
    let response = match client.get(releases_api_url()).send() {
        Ok(response) => response,
        Err(error) => {
            return Err(format!("release feed request failed: {error}"));
        }
    };
    if !response.status().is_success() {
        return Err(format!("release feed answered {}", response.status()));
    }
    response
        .json()
        .map_err(|error| format!("release feed parse failed: {error}"))
}

/// Query the feed and return the newest release that is newer than `current_version` and carries a package for this platform.
/// `channel` decides which tag series to read and which package names to accept.
pub fn check_for_update(current_version: &str, channel: ReleaseChannel) -> UpdateCheck {
    match fetch_releases() {
        Err(reason) => UpdateCheck::Unavailable(reason),
        Ok(releases) => select_package_from_releases(
            &releases,
            current_version,
            channel,
            &current_platform_token(),
        ),
    }
}

/// Pick the package to install from the parsed release list (the feed returns releases newest-first and this walks them in that order).
/// Only releases tagged for this channel and newer than `current_version` count; **when the newest one
/// lacks a package for this platform the walk does not fail but continues to older releases** — the real feed
/// once carried a poisoned newest release (`tui-v0.1.0`, asset `baihua--<platform>`, old executable inside); one bad release must not lock the channel.
/// When every newer release lacks a package for this platform, the newest release's data explains why.
fn select_package_from_releases(
    releases: &[GitHubRelease],
    current_version: &str,
    channel: ReleaseChannel,
    platform_token: &str,
) -> UpdateCheck {
    let prefix = channel.tag_prefix();
    let mut newest_tag = String::new();
    let mut newest_assets: Vec<String> = Vec::new();
    // (version, tag, asset names) —— the first "newer but no package for this platform" record, kept for the final explanation
    let mut newest_missing_package: Option<(String, String, Vec<String>)> = None;
    for release in releases {
        // Drafts are unpublished author work and are skipped; prereleases are not — the clients are in alpha
        // and the packaged releases are exactly those; filtering them would close the only update channel
        if release.draft {
            continue;
        }
        let Some(version) = release.tag_name.strip_prefix(&prefix) else {
            continue;
        };
        let asset_names: Vec<String> = release
            .assets
            .iter()
            .map(|asset| asset.name.clone())
            .collect();
        if newest_tag.is_empty() {
            newest_tag = release.tag_name.clone();
            newest_assets = asset_names.clone();
        }
        if !is_version_newer(version, current_version) {
            continue;
        }
        let Some(asset) = release.assets.iter().find(|asset| {
            channel.accepts_asset(&asset.name) && asset.name.contains(platform_token)
        }) else {
            if newest_missing_package.is_none() {
                newest_missing_package =
                    Some((version.to_string(), release.tag_name.clone(), asset_names));
            }
            continue;
        };
        return UpdateCheck::Available(ReleasePackage {
            version: version.to_string(),
            tag: release.tag_name.clone(),
            file_name: asset.name.clone(),
            download_url: asset.browser_download_url.clone(),
            size_bytes: asset.size,
        });
    }
    if let Some((version, tag, asset_names)) = newest_missing_package {
        return UpdateCheck::Unavailable(format!(
            "release {version} (tag {tag}) has no package for this platform ({platform_token}); assets found: {asset_names:?}, and no older release of this channel provides one either"
        ));
    }
    UpdateCheck::UpToDate {
        newest_tag,
        newest_assets,
    }
}

/// Download the package and verify its digest, returning the local path on success; no step failure leaves a usable half-file behind.
pub fn download_package(package: &ReleasePackage) -> Result<std::path::PathBuf, String> {
    let directory = paths::update_directory().ok_or_else(|| {
        "cannot locate the update directory; check BAIHUA_DIR and HOME".to_string()
    })?;
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("failed to create the update directory: {error}"))?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .user_agent(concat!("baihua-client/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("cannot create the downloader: {error}"))?;
    let archive_path = directory.join(&package.file_name);
    let bytes = client
        .get(&package.download_url)
        .send()
        .map_err(|error| format!("download failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("download failed: {error}"))?
        .bytes()
        .map_err(|error| format!("download interrupted: {error}"))?;
    let expected = client
        .get(format!("{}.sha256", package.download_url))
        .send()
        .map_err(|error| format!("failed to fetch the digest file: {error}"))?
        .error_for_status()
        .map_err(|error| format!("failed to fetch the digest file: {error}"))?
        .text()
        .map_err(|error| format!("failed to read the digest file: {error}"))?;
    if sha256_hex_of(&bytes) != normalize_digest_text(&expected) {
        return Err("package verification failed: the SHA-256 digest does not match the published one, refusing to install".to_string());
    }
    std::fs::write(&archive_path, &bytes)
        .map_err(|error| format!("failed to write the update package: {error}"))?;
    Ok(archive_path)
}

/// Hex SHA-256 digest of a byte string (shaped like `sha256sum` output so people can cross-check).
pub fn sha256_hex_of(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

/// A digest attachment may use the `sha256sum` "<digest>  <filename>" form; take the leading digest field and lowercase it.
fn normalize_digest_text(text: &str) -> String {
    text.split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison_handles_prerelease_markers() {
        assert!(is_version_newer("0.1.1", "0.1.0"));
        assert!(is_version_newer("0.2.0-alpha.1", "0.1.9"));
        assert!(is_version_newer("0.1.0", "0.1.0-alpha.2"));
        assert!(!is_version_newer("0.1.0-alpha.1", "0.1.0"));
        assert!(!is_version_newer("0.1.0", "0.1.0"));
        assert!(is_version_newer("0.1.0-alpha.3", "0.1.0-alpha.2"));
        assert!(!is_version_newer("0.1.0-alpha.2", "0.1.0-alpha.3"));
        // Prerelease counters beyond one digit must not compare as strings
        assert!(is_version_newer("0.1.0-alpha.10", "0.1.0-alpha.2"));
        assert!(!is_version_newer("0.1.0-alpha.2", "0.1.0-alpha.10"));
        assert!(is_version_newer("0.1.0-beta.1", "0.1.0-alpha.9"));
        // Versions with different segment counts compare zero-padded
        assert!(is_version_newer("0.1.0.1", "0.1.0"));
        assert!(!is_version_newer("0.1", "0.1.0"));
    }

    /// Build a fake feed record: tag + asset names (download addresses and sizes do not matter to the picking logic, so they are placeholders).
    fn fake_release(tag: &str, draft: bool, asset_names: &[&str]) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag.to_string(),
            draft,
            assets: asset_names
                .iter()
                .map(|name| GitHubAsset {
                    name: (*name).to_string(),
                    browser_download_url: format!("https://example.invalid/{name}"),
                    size: 1,
                })
                .collect(),
        }
    }

    /// A poisoned release that really happened: `tui-v0.1.0` carried an asset name whose version was empty (`baihua--<platform>`),
    /// with the old executable name inside. Such a newest release must not lock the channel; the walk must fall back to
    /// an older, correctly named release that packages this platform.
    #[test]
    fn a_mislabelled_newest_release_falls_back_to_an_older_installable_one() {
        let releases = vec![
            fake_release(
                "tui-v0.1.0",
                false,
                &[
                    "baihua",
                    "baihua--aarch64-apple-darwin.tar.gz",
                    "baihua--aarch64-apple-darwin.tar.gz.sha256",
                    "baihua.exe",
                ],
            ),
            fake_release("tui-v0.1.0-alpha.2", false, &[]),
            fake_release(
                "tui-v0.1.0-alpha.1",
                false,
                &["baihua-tui-0.1.0-alpha.1-aarch64-apple-darwin.tar.gz"],
            ),
        ];
        let check = select_package_from_releases(
            &releases,
            "0",
            ReleaseChannel::Terminal,
            "aarch64-apple-darwin",
        );
        match check {
            UpdateCheck::Available(package) => {
                assert_eq!(package.version, "0.1.0-alpha.1");
                assert_eq!(package.tag, "tui-v0.1.0-alpha.1");
                assert_eq!(
                    package.file_name,
                    "baihua-tui-0.1.0-alpha.1-aarch64-apple-darwin.tar.gz"
                );
            }
            other => panic!("expected a fallback to the older installable release, got {other:?}"),
        }
    }

    /// When no newer release packages this platform, report the asset list of the **newest** release
    /// and explain that older releases do not rescue it either (instead of silently reporting "up to date").
    #[test]
    fn a_channel_without_any_installable_package_explains_the_newest_release() {
        let releases = vec![
            fake_release(
                "tui-v0.1.0",
                false,
                &["baihua--aarch64-apple-darwin.tar.gz", "baihua"],
            ),
            fake_release("tui-v0.1.0-alpha.2", false, &[]),
        ];
        let check = select_package_from_releases(
            &releases,
            "0",
            ReleaseChannel::Terminal,
            "aarch64-apple-darwin",
        );
        match check {
            UpdateCheck::Unavailable(reason) => {
                assert!(reason.contains("tag tui-v0.1.0"), "{reason}");
                assert!(reason.contains("no older release"), "{reason}");
            }
            other => panic!("expected an explaining failure, got {other:?}"),
        }
    }

    /// Falling back must never mean downgrading: every candidate still passes the "newer than current" gate;
    /// when the channel only holds older releases or none at all, the answer stays UpToDate.
    #[test]
    fn fallback_never_downgrades_and_keeps_up_to_date_report() {
        let releases = vec![
            fake_release(
                "tui-v0.1.0",
                false,
                &["baihua-tui-0.1.0-x86_64-unknown-linux-gnu.tar.gz"],
            ),
            fake_release(
                "gui-v0.1.0",
                false,
                &["baihua-gui-0.1.0-aarch64-apple-darwin.tar.gz"],
            ),
        ];
        // Installed 0.1.1: 0.1.0 is not newer, so even a "fall back" must not install the older release
        match select_package_from_releases(
            &releases,
            "0.1.1",
            ReleaseChannel::Terminal,
            "aarch64-apple-darwin",
        ) {
            UpdateCheck::UpToDate { newest_tag, .. } => assert_eq!(newest_tag, "tui-v0.1.0"),
            other => panic!("expected up-to-date, got {other:?}"),
        }
        // The terminal channel has no releases at all: UpToDate without a tag (the installer turns that into "the channel has not published yet")
        match select_package_from_releases(
            &releases,
            "0",
            ReleaseChannel::CommandLine,
            "aarch64-apple-darwin",
        ) {
            UpdateCheck::UpToDate {
                newest_tag,
                newest_assets,
            } => {
                assert!(newest_tag.is_empty());
                assert!(newest_assets.is_empty());
            }
            other => panic!("expected an empty-channel up-to-date report, got {other:?}"),
        }
    }

    #[test]
    fn digest_text_accepts_checksum_tool_output() {
        let digest = sha256_hex_of(b"baihua");
        assert_eq!(
            normalize_digest_text(&format!("{digest}  file.tar.gz")),
            digest
        );
        assert_eq!(normalize_digest_text(&format!("{digest}\n")), digest);
        assert_ne!(sha256_hex_of(b"baihua"), sha256_hex_of(b"BAIHUA"));
    }

    /// The three package prefixes nest (`baihua-` is also a prefix of `baihua-cli-` and `baihua-gui-`),
    /// so selection must discriminate by channel: each end claims only its own names, and only the terminal end accepts the historic form.
    #[test]
    fn package_names_are_matched_per_channel() {
        let command_line_package = "baihua-cli-0.1.0-aarch64-apple-darwin.tar.gz";
        let graphical_package = "baihua-gui-0.1.0-x86_64-pc-windows-msvc.zip";
        let terminal_package = "baihua-tui-0.1.1-aarch64-apple-darwin.tar.gz";
        let legacy_terminal_package = "baihua-0.1.0-alpha.3-aarch64-apple-darwin.tar.gz";

        assert!(ReleaseChannel::CommandLine.package_name_matches(command_line_package));
        assert!(ReleaseChannel::Graphical.package_name_matches(graphical_package));
        assert!(ReleaseChannel::Terminal.package_name_matches(terminal_package));
        // The feed already carries terminal packages named `baihua-<version>-<platform>`; the terminal channel must still claim them
        assert!(ReleaseChannel::Terminal.package_name_matches(legacy_terminal_package));

        assert!(!ReleaseChannel::Terminal.package_name_matches(command_line_package));
        assert!(!ReleaseChannel::Terminal.package_name_matches(graphical_package));
        assert!(!ReleaseChannel::CommandLine.package_name_matches(terminal_package));
        assert!(!ReleaseChannel::Graphical.package_name_matches(terminal_package));
        assert!(!ReleaseChannel::CommandLine.package_name_matches(legacy_terminal_package));
        assert!(!ReleaseChannel::Graphical.package_name_matches(legacy_terminal_package));

        // Package formats: the graphical end ships installer formats, the other two never do
        assert!(
            ReleaseChannel::Graphical.accepts_asset("baihua-gui-0.2.0-aarch64-apple-darwin.dmg")
        );
        assert!(
            ReleaseChannel::Graphical.accepts_asset("baihua-gui-0.2.0-x86_64-pc-windows-msvc.msi")
        );
        assert!(
            ReleaseChannel::Graphical
                .accepts_asset("baihua-gui-0.2.0-x86_64-unknown-linux-gnu.AppImage")
        );
        assert!(ReleaseChannel::Graphical.accepts_asset(graphical_package));
        assert!(
            !ReleaseChannel::CommandLine.accepts_asset("baihua-cli-0.2.0-aarch64-apple-darwin.dmg")
        );
        assert!(
            !ReleaseChannel::Terminal.accepts_asset("baihua-tui-0.2.0-x86_64-pc-windows-msvc.msi")
        );
    }

    /// The graphical channel must pick the installer package of this platform from a feed that
    /// also carries the other platforms and the other ends.
    #[test]
    fn graphical_channel_picks_the_installer_for_this_platform() {
        let releases = [
            fake_release(
                "gui-v0.2.0",
                false,
                &[
                    "baihua-gui-0.2.0-aarch64-apple-darwin.dmg",
                    "baihua-gui-0.2.0-x86_64-apple-darwin.dmg",
                    "baihua-gui-0.2.0-x86_64-pc-windows-msvc.msi",
                    "baihua-cli-0.2.0-aarch64-apple-darwin.tar.gz",
                ],
            ),
            fake_release(
                "gui-v0.1.0",
                false,
                &["baihua-gui-0.1.0-aarch64-apple-darwin.dmg"],
            ),
        ];
        for (platform_token, expected) in [
            (
                "aarch64-apple-darwin",
                "baihua-gui-0.2.0-aarch64-apple-darwin.dmg",
            ),
            (
                "x86_64-pc-windows-msvc",
                "baihua-gui-0.2.0-x86_64-pc-windows-msvc.msi",
            ),
        ] {
            match select_package_from_releases(
                &releases,
                "0.1.0",
                ReleaseChannel::Graphical,
                platform_token,
            ) {
                UpdateCheck::Available(package) => assert_eq!(package.file_name, expected),
                other => panic!("the graphical channel must offer {expected}, got {other:?}"),
            }
        }
    }
}
