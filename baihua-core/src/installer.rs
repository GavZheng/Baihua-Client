//! Installer: puts client files into a specified directory, shared by initial install and auto-update.
//!
//! Design constraints:
//! - No interaction with the user throughout. Do whatever the caller gives as parameters; print the install path when done.
//! - Only use the standard library and built-in system tools, so the same code runs on macOS / Windows / Linux,
//!   when translating this file to `.sh` and `.bat` later, just compare function by function; no extra dependency knowledge needed.
//! - In update scenarios the old process is still running; overwriting itself directly will inevitably fail on Windows,
//!   so `wait_for_process` is supported: the new program waits as an independent process for the old one to exit before committing.

use crate::paths;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Input for one installation (or update).
#[derive(Debug, Clone)]
pub struct InstallRequest {
    /// Directory containing the content to install: should have an executable file, optionally with a `config` subdirectory
    pub source_directory: PathBuf,
    /// Install prefix; final layout is `<prefix>/bin/<executable_name>` and `<prefix>/config`
    pub prefix: PathBuf,
    /// ID of the old process to wait for (pass own process ID during auto-update); None means install immediately
    pub wait_for_process: Option<u32>,
}

/// Install result, used to report to the user where files land, and explanations for additional actions like PATH.
#[derive(Debug, Clone)]
pub struct InstallReport {
    /// Executables actually placed under `<prefix>/bin`, in end order: command line, graphical, terminal.
    /// A package may only carry one end, so this list can be shorter than the three ends.
    pub executable_paths: Vec<PathBuf>,
    pub config_directory: PathBuf,
    pub copied_file_count: usize,
    /// Additional notes to relay to the user (whether PATH was written, where the pending package came from, etc.)
    pub notes: Vec<String>,
}

/// One pending installation: the downloaded package, the version it carries, the prefix to
/// install into and the process whose exit the installer must wait for. A file rather than
/// command-line arguments, because `install` deliberately takes none.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PendingInstall {
    /// The downloaded package file: an archive, a disk image, an installer or an AppImage
    pub package_path: String,
    /// The version the package carries; names the staging directory and appears in reports
    pub version: String,
    /// Which prefix to install to. Must be written by the initiator: during self-update in target/debug the build directory is replaced,
    /// but `install` running standalone uses the default prefix; the two must not be mixed
    pub prefix: String,
    /// ID of the old process to wait for; None means install immediately
    pub wait_for_process: Option<u32>,
}

/// Default install prefix: `<$BAIHUA_DIR|~/.baihua>/client`, installed in a user-writable location, no admin rights needed.
pub fn default_prefix() -> Option<PathBuf> {
    paths::install_directory().map(|directory| {
        directory
            .parent()
            .map(|parent| parent.to_path_buf())
            .unwrap_or(directory)
    })
}

/// The install prefix belonging to the currently running instance: when the executable is at `<prefix>/bin/<name>` take two levels up,
/// otherwise (e.g. running directly from target/debug) take its own directory as the prefix, ensuring "updating itself" always lands in the right place.
pub fn current_prefix() -> Option<PathBuf> {
    let executable = paths::current_executable()?;
    let binary_directory = executable.parent()?;
    if binary_directory
        .file_name()
        .map(|name| name == "bin")
        .unwrap_or(false)
    {
        return binary_directory.parent().map(|parent| parent.to_path_buf());
    }
    Some(binary_directory.to_path_buf())
}

/// Unpack and install a verified update package, choosing the path by package format.
pub fn install_downloaded_package(pending: &PendingInstall) -> Result<InstallReport, String> {
    if let Some(process_id) = pending.wait_for_process {
        wait_for_process_to_exit(process_id);
    }
    let package_path = PathBuf::from(&pending.package_path);
    let prefix = PathBuf::from(&pending.prefix);
    let name = package_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    if name.ends_with(".dmg") {
        return install_disk_image(&package_path, &prefix, &pending.version);
    }
    if name.ends_with(".msi") {
        return install_windows_installer(&package_path, &prefix, &pending.version);
    }
    if name.ends_with(".AppImage") {
        return install_app_image(&package_path, &prefix);
    }
    let staged_directory = staging_directory(&pending.version)?;
    let source_directory = extract_archive(&package_path, &staged_directory)?;
    install(&InstallRequest {
        source_directory,
        prefix,
        wait_for_process: None,
    })
}

/// The staging directory one version unpacks into, inside the update directory.
fn staging_directory(version: &str) -> Result<PathBuf, String> {
    let staging_root = paths::update_directory()
        .ok_or_else(|| "cannot locate the update directory".to_string())?;
    Ok(staging_root.join(format!("staged-{version}")))
}

// The client's three "ends". Each end's package carries only its own executable, yet all three install into the same `<prefix>/bin`.
//
// Names mapped to release channels (BUILDING.md and `.github/workflows/build-release.yml` must agree with this table):
// - Command line end: executable `baihua` (this program, package `baihua-cli`, tag `cli-v<version>`);
// - Graphical end: executable `baihua-gui` (package `baihua-client-gui`, tag `gui-v<version>`);
// - Terminal end: executable `baihua-tui` (package `baihua-client-tui`, tag `tui-v<version>`).
//
// Installation decides which ends a source directory carries by name: package names and executable names correspond one-to-one, and "what is this process called" is never guessed.

/// Installed file name of the command line end (this program); Windows appends `.exe`
pub fn command_line_executable_name() -> String {
    executable_name("baihua")
}

/// Installed file name of the graphical end; Windows appends `.exe`
pub fn graphical_executable_name() -> String {
    executable_name("baihua-gui")
}

/// The macOS application bundle the graphical disk image carries, installed as-is into an
/// Applications directory; its shared configuration travels inside the bundle.
pub fn application_bundle_name() -> String {
    "Baihua.app".to_string()
}

/// The directories that hold dragged-in applications, best first (macOS only; every other
/// platform keeps all of the graphical end inside the install prefix).
pub fn application_bundle_directories() -> Vec<PathBuf> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    let mut directories = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        directories.push(PathBuf::from(home).join("Applications"));
    }
    directories
}

/// The directory a fresh graphical bundle lands in: the first Applications directory that
/// accepts writes, else the install prefix as the fallback every environment can use.
pub fn application_install_directory() -> PathBuf {
    let candidates = application_bundle_directories()
        .into_iter()
        .chain(default_prefix())
        .collect::<Vec<PathBuf>>();
    application_install_directory_from(&candidates)
}

/// The first candidate a file can actually be written into; when none accepts writes the
/// last candidate (the install prefix) still receives the bundle and reports the reason.
pub fn application_install_directory_from(candidates: &[PathBuf]) -> PathBuf {
    for candidate in candidates {
        if directory_accepts_writes(candidate) {
            return candidate.clone();
        }
    }
    candidates.last().cloned().unwrap_or_default()
}

/// Whether a directory exists (or can be created) and really accepts a written file.
fn directory_accepts_writes(directory: &Path) -> bool {
    std::fs::create_dir_all(directory).is_ok()
        && match std::fs::File::create(directory.join(".baihua-write-test")) {
            Ok(_) => {
                let _ = std::fs::remove_file(directory.join(".baihua-write-test"));
                true
            }
            Err(_) => false,
        }
}

/// The single-file Linux package of the graphical end.
pub fn graphical_app_image_name() -> String {
    "Baihua.AppImage".to_string()
}

/// Where the graphical end may live under a prefix, most preferred first: the legacy
/// application bundle, the AppImage, then the bare executable a historic archive installed.
pub fn graphical_payloads(prefix: &Path) -> Vec<PathBuf> {
    vec![
        prefix.join(application_bundle_name()),
        prefix.join("bin").join(graphical_app_image_name()),
        prefix.join("bin").join(graphical_executable_name()),
    ]
}

/// The installed payload of one end under the prefix, or None when that end is missing.
pub fn installed_payload(prefix: &Path, executable_name: &str) -> Option<PathBuf> {
    if executable_name == graphical_executable_name() {
        return graphical_payloads(prefix)
            .into_iter()
            .find(|payload| payload.exists());
    }
    let candidate = prefix.join("bin").join(executable_name);
    candidate.is_file().then_some(candidate)
}

/// Every place the installed graphical end may be found, best first: the bundle inside
/// each Applications directory, then the prefix payloads (legacy bundle, AppImage, file).
pub fn graphical_application_candidates() -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = application_bundle_directories()
        .into_iter()
        .map(|directory| directory.join(application_bundle_name()))
        .collect();
    if let Some(prefix) = default_prefix() {
        candidates.extend(graphical_payloads(&prefix));
    }
    candidates
}

/// The installed graphical end: the first candidate that exists, or None meaning the
/// graphical end is not installed anywhere this program recognises.
pub fn graphical_application_path() -> Option<PathBuf> {
    graphical_application_candidates()
        .into_iter()
        .find(|candidate| candidate.exists())
}
/// Installed file name of the terminal end; Windows appends `.exe`
pub fn terminal_executable_name() -> String {
    executable_name("baihua-tui")
}

/// The three executable names in the fixed order command line, graphical, terminal (install, uninstall and version reports all follow it)
pub fn installed_executable_names() -> Vec<String> {
    vec![
        command_line_executable_name(),
        graphical_executable_name(),
        terminal_executable_name(),
    ]
}

/// Each end's executable name paired with its release channel, used by the "fill whichever ends are missing" install step
fn installed_ends() -> Vec<(String, crate::update::ReleaseChannel)> {
    use crate::update::ReleaseChannel;
    vec![
        (command_line_executable_name(), ReleaseChannel::CommandLine),
        (graphical_executable_name(), ReleaseChannel::Graphical),
        (terminal_executable_name(), ReleaseChannel::Terminal),
    ]
}

/// Executable name: Windows needs the `.exe` suffix, other platforms use the bare name
fn executable_name(base_name: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{base_name}.exe")
    } else {
        base_name.to_string()
    }
}

/// Run one installation: wait for old processes to exit when needed, then copy every end's executable and the configuration the source directory carries into the prefix.
///
/// Whatever ends the source directory carries get installed (release packages are split per end: the command line package carries only `baihua`, the graphical package only `baihua-gui`,
/// the terminal package only `baihua-tui`); only an empty set fails. This way `/update` inside an interface replaces just its own end,
/// while a first install from the command line package tops up the other two ends through `install_from_command_line`.
pub fn install(request: &InstallRequest) -> Result<InstallReport, String> {
    if let Some(process_id) = request.wait_for_process {
        wait_for_process_to_exit(process_id);
    }
    let binary_directory = request.prefix.join("bin");
    let target_config_directory = request.prefix.join("config");
    std::fs::create_dir_all(&binary_directory)
        .map_err(|error| format!("failed to create {}: {error}", binary_directory.display()))?;

    let mut executable_paths: Vec<PathBuf> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut copied_file_count = 0usize;
    for executable_name in installed_executable_names() {
        let source_executable = request.source_directory.join(&executable_name);
        if !source_executable.is_file() {
            continue;
        }
        let target_executable = binary_directory.join(&executable_name);
        // Running install from inside the install directory can mean source and target are the same file: skip those copies outright.
        // The check must ask "is it the same file", never "is it the same path string": on macOS /tmp is a symlink
        // to /private/tmp, so one file has two spellings (`current_exe()` yields the canonical path, `BAIHUA_DIR` the user's),
        // and a pure string compare would let the copy land on itself -- `fs::copy` truncates the target before reading the source,
        // leaving the installed executable empty (on Windows it fails with "file in use" instead).
        if is_same_file(&source_executable, &target_executable) {
            notes.push(format!(
                "{} already runs from the installation directory; not copied again",
                target_executable.display()
            ));
            executable_paths.push(target_executable);
            continue;
        }
        std::fs::copy(&source_executable, &target_executable).map_err(|error| {
            format!(
                "failed to copy {} to {}: {error}",
                source_executable.display(),
                target_executable.display()
            )
        })?;
        mark_executable(&target_executable)?;
        copied_file_count += 1;
        executable_paths.push(target_executable);
    }
    if executable_paths.is_empty() {
        return Err(format!(
            "no installable executable found in {} (expected one of: {})",
            request.source_directory.display(),
            installed_executable_names().join(", ")
        ));
    }

    // Config directory: just fill in missing files; never overwrite an existing preferences.json (that is the user's settings and login session).
    // When the source directory has no config, fall back to the config directory the current process actually uses: running directly from target/debug
    // install can produce a complete layout with its own language and theme, rather than just one executable that won't run
    let source_config = {
        let beside_binary = request.source_directory.join("config");
        if beside_binary.is_dir() {
            beside_binary
        } else {
            paths::config_directory()
        }
    };
    if source_config.is_dir() && !is_same_file(&source_config, &target_config_directory) {
        copied_file_count +=
            copy_missing_files_recursively(&source_config, &target_config_directory)?;
        copied_file_count +=
            merge_missing_language_entries(&source_config, &target_config_directory)?;
    }
    Ok(InstallReport {
        executable_paths,
        config_directory: target_config_directory,
        copied_file_count,
        notes,
    })
}

/// Whether two paths name the same file. Compare strings first; when they differ compare canonical paths (resolving symlinks and `.` / `..`):
/// `canonicalize` fails while the target does not exist yet or a parent is unreachable, and then the two cannot be the same file anyway.
fn is_same_file(first: &Path, second: &Path) -> bool {
    if first == second {
        return true;
    }
    match (std::fs::canonicalize(first), std::fs::canonicalize(second)) {
        (Ok(first), Ok(second)) => first == second,
        _ => false,
    }
}

/// Mount a macOS disk image and replace the installed `Baihua.app` (inside the chosen
/// Applications directory) with the bundle it carries; its config travels inside it.
fn install_disk_image(
    image_path: &Path,
    prefix: &Path,
    version: &str,
) -> Result<InstallReport, String> {
    let staging_root = paths::update_directory()
        .ok_or_else(|| "cannot locate the update directory".to_string())?;
    let mount_point = staging_root.join(format!("mounted-{version}"));
    std::fs::create_dir_all(&mount_point)
        .map_err(|error| format!("failed to create {}: {error}", mount_point.display()))?;
    let attached = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-mountpoint"])
        .arg(&mount_point)
        .arg(image_path)
        .status()
        .map_err(|error| format!("hdiutil is unavailable: {error}"))
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(format!(
                    "mounting the disk image failed, exit status {status}"
                ))
            }
        });
    let outcome = attached.and_then(|_| {
        let mounted_bundle = find_directory_named(&mount_point, &application_bundle_name())
            .ok_or_else(|| {
                format!(
                    "the mounted image holds no {}: refusing to install",
                    application_bundle_name()
                )
            })?;
        replace_application_bundle(&mounted_bundle, &application_install_directory())
    });
    let _ = Command::new("hdiutil")
        .args(["detach", "-force"])
        .arg(&mount_point)
        .status();
    let _ = std::fs::remove_dir_all(&mount_point);
    let installed_bundle = outcome?;
    Ok(InstallReport {
        executable_paths: vec![
            installed_bundle
                .join("Contents")
                .join("MacOS")
                .join(graphical_executable_name()),
        ],
        config_directory: prefix.join("config"),
        copied_file_count: 1,
        notes: vec![format!(
            "graphical application bundle installed at {}",
            installed_bundle.display()
        )],
    })
}

/// Move a fresh bundle over the installed one inside the given directory: the old bundle
/// is removed first, so an update never leaves files of the previous version inside it.
fn replace_application_bundle(
    source_bundle: &Path,
    install_directory: &Path,
) -> Result<PathBuf, String> {
    let target = install_directory.join(application_bundle_name());
    if target.exists() {
        std::fs::remove_dir_all(&target)
            .map_err(|error| format!("failed to remove {}: {error}", target.display()))?;
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    copy_directory_tree(source_bundle, &target)?;
    let inner_executable = target
        .join("Contents")
        .join("MacOS")
        .join(graphical_executable_name());
    if inner_executable.is_file() {
        mark_executable(&inner_executable)?;
    }
    Ok(target)
}

/// Extract a Windows installer with an administrative install (`/a` unpacks the payload
/// without touching the registry) and install the directory holding the program file.
fn install_windows_installer(
    package_path: &Path,
    prefix: &Path,
    version: &str,
) -> Result<InstallReport, String> {
    let staged_directory = staging_directory(version)?;
    std::fs::create_dir_all(&staged_directory)
        .map_err(|error| format!("failed to create the unpack directory: {error}"))?;
    let status = Command::new("msiexec")
        .arg("/a")
        .arg(package_path)
        .arg("/qn")
        .arg(format!("TARGETDIR={}", staged_directory.display()))
        .status()
        .map_err(|error| format!("msiexec is unavailable: {error}"))?;
    // 3010 means "success, reboot requested", which is a completed extraction
    if !matches!(status.code(), Some(0) | Some(3010)) {
        return Err(format!(
            "extracting the installer failed, exit status {status}"
        ));
    }
    let program_file = find_file_by_name(&staged_directory, &graphical_executable_name())
        .ok_or_else(|| {
            format!(
                "the installer holds no {}, refusing to install",
                graphical_executable_name()
            )
        })?;
    let source_directory = program_file
        .parent()
        .map(|parent| parent.to_path_buf())
        .ok_or_else(|| "the extracted program has no directory".to_string())?;
    install(&InstallRequest {
        source_directory,
        prefix: prefix.to_path_buf(),
        wait_for_process: None,
    })
}

/// Install a Linux AppImage: the file is the program, so it is copied under a stable name
/// and made executable. Its configuration travels inside the image.
fn install_app_image(package_path: &Path, prefix: &Path) -> Result<InstallReport, String> {
    let binary_directory = prefix.join("bin");
    std::fs::create_dir_all(&binary_directory)
        .map_err(|error| format!("failed to create {}: {error}", binary_directory.display()))?;
    let target = binary_directory.join(graphical_app_image_name());
    std::fs::copy(package_path, &target).map_err(|error| {
        format!(
            "failed to copy {} to {}: {error}",
            package_path.display(),
            target.display()
        )
    })?;
    mark_executable(&target)?;
    Ok(InstallReport {
        executable_paths: vec![target],
        config_directory: prefix.join("config"),
        copied_file_count: 1,
        notes: Vec::new(),
    })
}

/// Copy a whole directory tree (an application bundle) to a fresh destination.
fn copy_directory_tree(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target)
        .map_err(|error| format!("failed to create {}: {error}", target.display()))?;
    let entries = std::fs::read_dir(source)
        .map_err(|error| format!("failed to read {}: {error}", source.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("failed to list {}: {error}", source.display()))?;
        let child_source = entry.path();
        let child_target = target.join(entry.file_name());
        if child_source.is_dir() {
            copy_directory_tree(&child_source, &child_target)?;
        } else {
            std::fs::copy(&child_source, &child_target).map_err(|error| {
                format!(
                    "failed to copy {} to {}: {error}",
                    child_source.display(),
                    child_target.display()
                )
            })?;
        }
    }
    Ok(())
}

/// The first directory with this exact name anywhere below a root, searched recursively.
fn find_directory_named(root: &Path, directory_name: &str) -> Option<PathBuf> {
    let mut queue = vec![root.to_path_buf()];
    while let Some(directory) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path
                .file_name()
                .map(|name| name == directory_name)
                .unwrap_or(false)
            {
                return Some(path);
            }
            queue.push(path);
        }
    }
    None
}

/// The first file with this exact name anywhere below a root, searched recursively.
fn find_file_by_name(root: &Path, file_name: &str) -> Option<PathBuf> {
    let mut queue = vec![root.to_path_buf()];
    while let Some(directory) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                queue.push(path);
            } else if path
                .file_name()
                .map(|name| name == file_name)
                .unwrap_or(false)
            {
                return Some(path);
            }
        }
    }
    None
}
/// The command line end's `install` (no arguments in public use):
/// - when a "pending install" record exists, install exactly what it says (the route an in-interface `/update` starts; no questions asked);
/// - otherwise install every end sitting next to the current executable into the default prefix, and for ends never installed before
///   fetch the latest package from their release channels, then ask once about writing PATH.
pub fn install_from_command_line() -> Result<InstallReport, String> {
    let fallback_prefix =
        default_prefix().ok_or_else(|| "cannot determine the installation prefix".to_string())?;
    if let Some(pending) = read_pending_install() {
        let report = install_downloaded_package(&pending);
        // The record is only invalidated after a successful install; on failure it stays so the user can re-run install and still succeed
        if report.is_ok()
            && let Some(path) = pending_install_path()
        {
            let _ = std::fs::remove_file(path);
        }
        let mut report = report?;
        report.notes.push(format!(
            "this install came from the update package {} (prefix {})",
            pending.package_path, pending.prefix
        ));
        return Ok(report);
    }
    let prefix = fallback_prefix;
    let source_directory = paths::current_executable()
        .and_then(|executable| executable.parent().map(|parent| parent.to_path_buf()))
        .ok_or_else(|| "cannot locate the running executable; nothing installed".to_string())?;
    let mut report = install(&InstallRequest {
        source_directory: source_directory.clone(),
        prefix: prefix.clone(),
        wait_for_process: None,
    })?;
    // Release packages are split per end and the command line package holds only `baihua`: if either other end is present neither locally nor in the prefix,
    // its latest package comes from its release channel, so one install can put all three ends in place.
    report
        .notes
        .extend(install_missing_ends(&source_directory, &prefix));
    let binary_directory = prefix.join("bin");
    if !interactive() {
        // Changing shell configuration without anyone there to answer a prompt would be wrong: report the line to add instead
        report.notes.push(format!(
            "(non-interactive run, PATH untouched) add it yourself when needed: export PATH=\"{}:$PATH\"",
            binary_directory.display()
        ));
    } else if ask_yes_no(
        &format!("Add {} to PATH?", binary_directory.display()),
        true,
    ) {
        report.notes.push(add_directory_to_path(&binary_directory));
    } else {
        report.notes.push(format!(
            "PATH left unchanged; add {} to PATH to run {} directly.",
            binary_directory.display(),
            command_line_executable_name()
        ));
    }
    Ok(report)
}

/// For ends neither the source directory carries nor the prefix has yet: download the latest package from each release channel.
/// A failure on one end becomes a single note and never interrupts the other ends or the configuration install.
fn install_missing_ends(source_directory: &Path, prefix: &Path) -> Vec<String> {
    let mut notes: Vec<String> = Vec::new();
    for (executable_name, channel) in installed_ends() {
        if source_directory.join(&executable_name).is_file() {
            continue;
        }
        let already_installed = if executable_name == graphical_executable_name() {
            graphical_application_path().is_some()
        } else {
            installed_payload(prefix, &executable_name).is_some()
        };
        if already_installed {
            notes.push(format!(
                "{executable_name} is already installed; left untouched (use `baihua update` to upgrade it)"
            ));
            continue;
        }
        notes.push(install_latest_release_of(channel, prefix, &executable_name));
    }
    notes
}

/// Take the newest package on one channel, download, verify, unpack, and install it into the prefix.
fn install_latest_release_of(
    channel: crate::update::ReleaseChannel,
    prefix: &Path,
    executable_name: &str,
) -> String {
    use crate::update::{UpdateCheck, check_for_update, download_package};
    // Pass "0" as the current version: this end was never installed, so the newest package wins
    match check_for_update("0", channel) {
        UpdateCheck::Available(package) => match download_package(&package) {
            Ok(package_file) => {
                let install_result = install_downloaded_package(&PendingInstall {
                    package_path: package_file.to_string_lossy().to_string(),
                    version: package.version.clone(),
                    prefix: prefix.to_string_lossy().to_string(),
                    wait_for_process: None,
                })
                .map(|_| ());
                match install_result {
                    Ok(()) => format!(
                        "{executable_name} {} installed from the {} release",
                        package.version,
                        channel.display_name()
                    ),
                    Err(error) => format!("{executable_name} was not installed: {error}"),
                }
            }
            Err(error) => format!("{executable_name} was not installed: {error}"),
        },
        UpdateCheck::UpToDate { newest_tag, .. } => {
            if newest_tag.is_empty() {
                // This channel has never published anything (for example the freshly split command line / graphical ends)
                format!(
                    "{executable_name} was not installed: the {} release channel has no published release yet",
                    channel.display_name()
                )
            } else {
                format!(
                    "{executable_name} was not installed: the {newest_tag} release has no package for this platform"
                )
            }
        }
        UpdateCheck::Unavailable(reason) => {
            format!("{executable_name} was not installed: {reason}")
        }
    }
}

/// The command line end's `uninstall`: remove the three installed executables and ask once whether configuration and caches go too.
pub fn uninstall_from_command_line() -> Result<Vec<String>, String> {
    let prefix =
        default_prefix().ok_or_else(|| "cannot determine the installation prefix".to_string())?;
    let binary_directory = prefix.join("bin");
    let mut notes: Vec<String> = Vec::new();
    for bundle_directory in application_bundle_directories() {
        let bundle = bundle_directory.join(application_bundle_name());
        if bundle.is_dir() {
            std::fs::remove_dir_all(&bundle)
                .map_err(|error| format!("failed to remove {}: {error}", bundle.display()))?;
            notes.push(format!("removed {}", bundle.display()));
        }
    }
    let legacy_bundle = prefix.join(application_bundle_name());
    if legacy_bundle.is_dir() {
        std::fs::remove_dir_all(&legacy_bundle)
            .map_err(|error| format!("failed to remove {}: {error}", legacy_bundle.display()))?;
        notes.push(format!("removed {}", legacy_bundle.display()));
    }
    for executable_name in installed_executable_names() {
        let executable_path = binary_directory.join(&executable_name);
        if executable_path.is_file() {
            std::fs::remove_file(&executable_path).map_err(|error| {
                format!("failed to remove {}: {error}", executable_path.display())
            })?;
            notes.push(format!("removed {}", executable_path.display()));
        } else {
            notes.push(format!(
                "no installed executable found at {}",
                executable_path.display()
            ));
        }
    }
    // Clean up only the lines this installer wrote and leave the user's own PATH configuration alone
    if let Some(removed) = remove_directory_from_path(&binary_directory) {
        notes.push(removed);
    }
    // One question covers everything installed or accumulated: the configuration directory and the regenerable caches (messages, avatars)
    let mut removable: Vec<PathBuf> = Vec::new();
    let config_directory = prefix.join("config");
    if config_directory.is_dir() {
        removable.push(config_directory);
    }
    if let Some(cache_directory) = crate::paths::cache_directory()
        && cache_directory.is_dir()
    {
        removable.push(cache_directory);
    }
    if removable.is_empty() {
        return Ok(notes);
    }
    let listed = removable
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<String>>()
        .join(", ");
    if interactive() && ask_yes_no(&format!("Also remove these files? {listed}"), false) {
        for path in &removable {
            std::fs::remove_dir_all(path)
                .map_err(|error| format!("failed to remove {}: {error}", path.display()))?;
            notes.push(format!("removed {}", path.display()));
        }
    } else {
        notes.push(format!("kept: {listed}"));
    }
    Ok(notes)
}

/// The paragraph marker we wrote into the shell startup file; on uninstall the whole paragraph is removed per the marker,
/// never touch the PATH lines the user wrote themselves.
#[cfg(not(windows))]
fn path_block_marker() -> String {
    "baihua PATH (managed by baihua install/uninstall)".to_string()
}

/// Which shell startup file to write to: determines zsh / bash / other based on `$SHELL` (POSIX fallback ~/.profile).
#[cfg(not(windows))]
fn shell_profile_path() -> Option<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_default();
    let home = dirs_home()?;
    let file_name = if shell.contains("zsh") {
        ".zshrc"
    } else if shell.contains("bash") {
        ".bashrc"
    } else {
        ".profile"
    };
    Some(home.join(file_name))
}

/// User's home directory (same determination as baihua-core, additionally recognizes standard variables besides BAIHUA_DIR).
#[cfg(not(windows))]
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Write the directory containing the executable to PATH. macOS/Linux writes to the shell startup file,
/// Windows writes to the current user's user-level PATH (does not touch system-level). Explanation shown to the user.
fn add_directory_to_path(directory: &Path) -> String {
    let display = directory.display().to_string();
    #[cfg(windows)]
    {
        let script = format!(
            "$user=[Environment]::GetEnvironmentVariable('Path','User'); if ($user -notlike '*{display}*') {{ [Environment]::SetEnvironmentVariable('Path', \"$user;{display}\", 'User'); 'added' }} else {{ 'already' }}"
        );
        return match Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .output()
        {
            Ok(output) if output.status.success() => {
                let replied = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if replied == "already" {
                    format!("{display} is already in the user PATH; nothing added")
                } else {
                    format!("added {display} to the user PATH; it takes effect in a new terminal")
                }
            }
            _ => format!("failed to write the PATH; add {display} manually"),
        };
    }
    #[cfg(not(windows))]
    {
        let Some(profile_path) = shell_profile_path() else {
            return format!("no home directory found; add {display} to PATH manually");
        };
        match append_path_block(&profile_path, directory) {
            Ok(true) => format!(
                "added {display} to {}; restart the shell (or source {}) to take effect",
                profile_path.display(),
                profile_path.display()
            ),
            Ok(false) => format!(
                "{display} is already in {}; nothing added",
                profile_path.display()
            ),
            Err(error) => format!(
                "failed to write {}: {error}; add {display} to PATH manually",
                profile_path.display()
            ),
        }
    }
}

/// Append a marked PATH setting to the end of the shell startup file; returns false unchanged when the marker already exists,
/// so repeated installs will not pile up the same export line.
#[cfg(not(windows))]
fn append_path_block(profile_path: &Path, directory: &Path) -> Result<bool, String> {
    let display = directory.display().to_string();
    let existing = std::fs::read_to_string(profile_path).unwrap_or_default();
    if existing.contains(&path_block_marker()) || existing.contains(&display) {
        return Ok(false);
    }
    let block = format!(
        "\n# {marker}\nexport PATH=\"{display}:$PATH\"\n# end {marker}\n",
        marker = path_block_marker()
    );
    append_text(profile_path, &block).map(|()| true)
}

/// Remove the paragraph we wrote; does nothing when the marker is not in the file.
#[cfg(not(windows))]
fn remove_path_block(profile_path: &Path) -> Result<bool, String> {
    let content = std::fs::read_to_string(profile_path).map_err(|error| error.to_string())?;
    let marker = format!("# {}", path_block_marker());
    let Some(start) = content.find(&marker) else {
        return Ok(false);
    };
    let end_marker = format!("# end {}", path_block_marker());
    let Some(offset) = content[start..].find(&end_marker) else {
        return Err("the block has no end marker; the file was left untouched".to_string());
    };
    let end = start + offset + end_marker.len();
    // The blank lines before and after the paragraph were left by us when appending; take them together too, user's original content continues as-is
    let head = content[..start].trim_end_matches('\n');
    let tail = content[end..].trim_start_matches('\n');
    let rebuilt = match (head.is_empty(), tail.is_empty()) {
        (true, true) => String::new(),
        (true, false) => tail.to_string(),
        (false, true) => format!("{head}\n"),
        (false, false) => format!("{head}\n{tail}"),
    };
    std::fs::write(profile_path, rebuilt).map_err(|error| error.to_string())?;
    Ok(true)
}

/// Append text to the end of a file (creates the file if it does not exist).
#[cfg(not(windows))]
fn append_text(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("{error}"))?;
    file.write_all(text.as_bytes())
        .map_err(|error| format!("{error}"))
}

/// On uninstall, remove the paragraph we wrote; returning None means we never wrote anything, do not touch the user's files.
#[cfg(not(windows))]
fn remove_directory_from_path(_directory: &Path) -> Option<String> {
    let profile_path = shell_profile_path()?;
    if !remove_path_block(&profile_path).ok()? {
        return None;
    }
    Some(format!(
        "removed our PATH block from {}",
        profile_path.display()
    ))
}

/// On uninstall, remove the paragraph we wrote (on Windows, remove the one item in the user's PATH).
/// A None return means nothing was ever written, so the user's file stays untouched.
#[cfg(windows)]
fn remove_directory_from_path(directory: &Path) -> Option<String> {
    let display = directory.display().to_string();
    let script = format!(
        "$user=[Environment]::GetEnvironmentVariable('Path','User'); if ($user -like '*{display}*') {{ $kept=($user -split ';' | Where-Object {{ $_ -ne '{display}' }}) -join ';'; [Environment]::SetEnvironmentVariable('Path', $kept, 'User'); 'removed' }} else {{ 'none' }}"
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .ok()?;
    let replied = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (replied == "removed").then(|| format!("removed {display} from the user PATH"))
}

/// Is anyone available to answer questions: there is no one in detached process, piped input, or redirected output scenarios.
/// When nobody is there to answer, never modify the user's shell configuration on our own and never block on reading input.
fn interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// Y/n prompt: pressing Enter takes the default value. Use `interactive()` first to confirm someone is actually there.
fn ask_yes_no(question: &str, default_yes: bool) -> bool {
    use std::io::Write;
    let suffix = if default_yes { "[Y/n] " } else { "[y/N] " };
    print!("{question} {suffix}");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return default_yes;
    }
    match answer.trim().to_lowercase().as_str() {
        "" => default_yes,
        "y" | "yes" => true,
        "n" | "no" => false,
        other => {
            println!("  did not understand {other:?}; using the default");
            default_yes
        }
    }
}

/// Unpack and install a downloaded and verified archive, shared by `baihua-client update` and /update in the interface.
/// Return the unpack directory; the caller (the interface) uses it before exit to start an installer process that waits for itself to exit.
pub fn extract_archive(archive_path: &Path, destination: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(destination)
        .map_err(|error| format!("failed to create the unpack directory: {error}"))?;
    let name = archive_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let command_result = if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        // tar comes with macOS/Linux; Windows 10 and later also have bsdtar built in
        Command::new("tar")
            .args(["-xzf"])
            .arg(archive_path)
            .arg("-C")
            .arg(destination)
            .status()
    } else if name.ends_with(".zip") {
        #[cfg(target_os = "windows")]
        {
            Command::new("powershell")
                .args(["-NoProfile", "-Command", "Expand-Archive", "-LiteralPath"])
                .arg(archive_path)
                .args(["-DestinationPath"])
                .arg(destination)
                .status()
        }
        #[cfg(not(target_os = "windows"))]
        {
            // On non-Windows, zip is handed to bsdtar (macOS's tar is bsdtar), falling back to unzip on failure
            match Command::new("tar")
                .arg("-xf")
                .arg(archive_path)
                .arg("-C")
                .arg(destination)
                .status()
            {
                Ok(status) if status.success() => Ok(status),
                _ => Command::new("unzip")
                    .arg("-o")
                    .arg(archive_path)
                    .arg("-d")
                    .arg(destination)
                    .status(),
            }
        }
    } else {
        return Err(format!("unsupported archive format: {name}"));
    };
    match command_result {
        Ok(status) if status.success() => Ok(destination.to_path_buf()),
        Ok(status) => Err(format!("unpacking failed, exit status {status}")),
        Err(error) => Err(format!("unpack tool unavailable: {error}")),
    }
}

/// Pending install record file path.
fn pending_install_path() -> Option<PathBuf> {
    paths::update_directory().map(|directory| directory.join("pending-install.json"))
}

/// Write the pending install record (called by /update before exiting).
fn write_pending_install(pending: &PendingInstall) -> Result<(), String> {
    let path =
        pending_install_path().ok_or_else(|| "cannot locate the update directory".to_string())?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    serde_json::to_string_pretty(pending)
        .map_err(|error| format!("failed to serialise the pending install record: {error}"))
        .and_then(|text| {
            std::fs::write(path, text).map_err(|error| format!("failed to write: {error}"))
        })
}

/// Read the pending install record; return None when the file does not exist or is corrupted (a corrupted record is treated as "no pending package" and cleared).
fn read_pending_install() -> Option<PendingInstall> {
    let path = pending_install_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let parsed: PendingInstall = match serde_json::from_str(&content) {
        Ok(parsed) => parsed,
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            return None;
        }
    };
    if PathBuf::from(&parsed.package_path).is_file() {
        Some(parsed)
    } else {
        let _ = std::fs::remove_file(&path);
        None
    }
}

/// Install log path: the installer process waiting for the old process to exit is started detached from the terminal,
/// the printed content must go to a file so users and developers can see the result.
fn install_log_path() -> Option<PathBuf> {
    paths::update_directory().map(|directory| directory.join("install.log"))
}

/// Start the installer process detached from the terminal: the pending content is first written to the pending install record,
/// then launch `baihua-client install` (without any parameters) to let it wait for the old process to exit before committing.
pub fn spawn_detached_installer(pending: &PendingInstall) -> Result<(), String> {
    // The command line end `baihua` performs every real install: the graphical and terminal ends do not parse installer arguments themselves,
    // so they write the "pending install" record and launch the sibling (or PATH-resolved) `baihua` to carry out the swap.
    let command_line_executable = locate_command_line_executable().ok_or_else(|| {
        format!(
            "cannot locate the command line executable `{}`; the installer was not started",
            command_line_executable_name()
        )
    })?;
    write_pending_install(pending)?;
    let mut command = Command::new(&command_line_executable);
    command.arg("install");
    // The detached process has no terminal to write to; results only go into the install log
    if let Some(log_path) = install_log_path()
        && let Ok(file) = std::fs::File::create(log_path)
    {
        use std::process::Stdio;
        command.stdout(
            file.try_clone()
                .map(|_| Stdio::from(file))
                .unwrap_or(Stdio::null()),
        );
    }
    command
        .spawn()
        .map_err(|error| format!("failed to start the installer process: {error}"))?;
    Ok(())
}

/// Where the command line executable lives: after installation all three ends share `<prefix>/bin`, so check the current process's directory first,
/// then the default prefix's `bin/`, and finally PATH. Both the in-interface `/update` and the command line end's own updates use it.
fn locate_command_line_executable() -> Option<PathBuf> {
    let file_name = command_line_executable_name();
    if let Some(current_executable) = paths::current_executable()
        && let Some(directory) = current_executable.parent()
        && directory.join(&file_name).is_file()
    {
        return Some(directory.join(&file_name));
    }
    if let Some(prefix) = default_prefix() {
        let candidate = prefix.join("bin").join(&file_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(&file_name))
        .find(|candidate| candidate.is_file())
}

/// Poll waiting for the process to exit; wait up to two minutes, continue installing on timeout (better to overwrite a failure than to stay stuck forever).
fn wait_for_process_to_exit(process_id: u32) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while std::time::Instant::now() < deadline {
        if !process_is_alive(process_id) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Whether the process is still alive. `kill -0` (Unix) and `tasklist` (Windows) neither change the target process state, only check existence.
fn process_is_alive(process_id: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &process_id.to_string()])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {process_id}"), "/NH"])
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).contains(&process_id.to_string()))
            .unwrap_or(false)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = process_id;
        false
    }
}

/// Give the target file the execute bit (Windows has no such concept; skip directly).
fn mark_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        // PermissionsExt provides both the read and write permission bit methods
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map_err(|error| format!("failed to read permissions: {error}"))?
            .permissions()
            .mode();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode | 0o755))
            .map_err(|error| format!("failed to set the executable bit: {error}"))?;
    }
    let _ = path;
    Ok(())
}

/// Copy files one by one from the source directory where they don't exist in the target; return the copy count.
/// Compare one by one instead of overwriting the whole directory, to preserve the user's existing preferences.json and appearance theme changes.
fn copy_missing_files_recursively(source: &Path, target: &Path) -> Result<usize, String> {
    let mut copied = 0usize;
    let entries = std::fs::read_dir(source)
        .map_err(|error| format!("failed to read {}: {error}", source.display()))?;
    for entry in entries.flatten() {
        let source_path = entry.path();
        let Some(file_name) = source_path.file_name() else {
            continue;
        };
        // Skip system junk files like .DS_Store; do not include them in the release directory
        if file_name.to_string_lossy().starts_with('.') {
            continue;
        }
        let target_path = target.join(file_name);
        if source_path.is_dir() {
            copied += copy_missing_files_recursively(&source_path, &target_path)?;
        } else if !target_path.exists() {
            if let Some(parent) = target_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::copy(&source_path, &target_path).is_ok() {
                copied += 1;
            }
        }
    }
    Ok(copied)
}

/// Fill entries from the shipped language files that the user's copy lacks, returning how many files were patched.
///
/// Language files are the program's own text tables: a copy installed by an older version stays in the user configuration directory forever
/// (`copy_missing_files_recursively` only adds missing files), so keys introduced by newer versions simply are not in it and
/// the interface would show raw key names. This step only ever **fills gaps**:
/// entries the user edited and language files the user added stay untouched,
/// and preferences.json (the user's settings and login session) is never touched at all.
fn merge_missing_language_entries(
    source_config: &Path,
    target_config: &Path,
) -> Result<usize, String> {
    let source_languages = source_config.join("languages");
    let Ok(entries) = std::fs::read_dir(&source_languages) else {
        return Ok(0);
    };
    let mut merged_file_count = 0usize;
    for entry in entries.flatten() {
        let source_path = entry.path();
        if source_path
            .extension()
            .map(|extension| extension != "json")
            .unwrap_or(true)
        {
            continue;
        }
        let target_path = target_config.join("languages").join(entry.file_name());
        // The target lacks this language file entirely: that is the copy-missing-files logic's job; here only "both copies exist" is handled
        if !target_path.is_file() {
            continue;
        }
        let (Ok(source_content), Ok(target_content)) = (
            std::fs::read_to_string(&source_path),
            std::fs::read_to_string(&target_path),
        ) else {
            continue;
        };
        let (
            Ok(serde_json::Value::Object(source_texts)),
            Ok(serde_json::Value::Object(mut target_texts)),
        ) = (
            serde_json::from_str::<serde_json::Value>(&source_content),
            serde_json::from_str::<serde_json::Value>(&target_content),
        )
        else {
            continue;
        };
        let mut added_entry_count = 0usize;
        for (key, text) in source_texts {
            if target_texts.contains_key(&key) {
                continue;
            }
            target_texts.insert(key, text);
            added_entry_count += 1;
        }
        if added_entry_count == 0 {
            continue;
        }
        let Ok(pretty) = serde_json::to_string_pretty(&serde_json::Value::Object(target_texts))
        else {
            continue;
        };
        std::fs::write(&target_path, pretty)
            .map_err(|error| format!("failed to write {}: {error}", target_path.display()))?;
        merged_file_count += 1;
    }
    Ok(merged_file_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staging_area(label: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("baihua-installer-{label}"));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(directory.join("payload/config/themes"))
            .expect("the source directory must be creatable");
        std::fs::create_dir_all(directory.join("target"))
            .expect("the target directory must be creatable");
        std::fs::write(directory.join("payload/baihua"), b"fake-binary")
            .expect("writing the source file must succeed");
        std::fs::write(directory.join("payload/config/themes/dark.json"), b"{}")
            .expect("writing the theme must succeed");
        std::fs::write(
            directory.join("payload/config/preferences.json"),
            b"{\"show_uid\":true}",
        )
        .expect("writing the preferences must succeed");
        directory
    }

    #[cfg(unix)]
    #[test]
    fn install_places_executable_and_config_under_prefix() {
        let staging = staging_area("basic");
        let report = install(&InstallRequest {
            source_directory: staging.join("payload"),
            prefix: staging.join("target"),
            wait_for_process: None,
        })
        .expect("installation must succeed");
        assert_eq!(
            report.executable_paths.len(),
            1,
            "with a single executable in the source directory only that end installs"
        );
        assert!(report.executable_paths[0].is_file());
        assert!(report.config_directory.join("themes/dark.json").is_file());
        use std::os::unix::fs::PermissionsExt;
        assert!(
            std::fs::metadata(&report.executable_paths[0])
                .unwrap()
                .permissions()
                .mode()
                & 0o111
                != 0,
            "the installed executable must carry the executable bit"
        );
        let _ = std::fs::remove_dir_all(&staging);
    }

    /// One installation must put every end the source directory carries into `<prefix>/bin`: release packages are split per end,
    /// while the source tree keeps all three products in one directory, and this test guards "one pass installs all three".
    #[cfg(unix)]
    #[test]
    fn install_copies_every_end_found_in_the_source_directory() {
        let staging = staging_area("three-ends");
        for executable_name in installed_executable_names() {
            std::fs::write(
                staging.join("payload").join(&executable_name),
                b"fake-binary",
            )
            .expect("writing the three end products must succeed");
        }
        let report = install(&InstallRequest {
            source_directory: staging.join("payload"),
            prefix: staging.join("target"),
            wait_for_process: None,
        })
        .expect("installation must succeed");
        assert_eq!(
            report.executable_paths.len(),
            3,
            "all three ends in the source directory must install, got: {:?}",
            report.executable_paths
        );
        for executable_name in installed_executable_names() {
            assert!(
                report.executable_paths.iter().any(|path| path
                    .file_name()
                    .map(|name| name == executable_name.as_str())
                    == Some(true)),
                "{executable_name} must be installed into the bin directory"
            );
        }
        let _ = std::fs::remove_dir_all(&staging);
    }

    /// Running `install` from inside the install directory (source and target are one file) must leave only a note and copy nothing.
    ///
    /// This test guards the "compare path strings only" mistake: on macOS `/tmp` points at `/private/tmp`,
    /// so one file has two spellings, a string compare calls them two files, `fs::copy` truncates the target before reading the source,
    /// and the installed executables collapse to zero bytes (this is exactly how the installer once wiped all three ends during verification).
    #[cfg(unix)]
    #[test]
    fn install_skips_copying_the_executable_onto_itself_through_a_symlink_alias() {
        let staging = staging_area("self-copy");
        // real/bin holds the three end products and alias is a symlink to real:
        // the source directory is reached through alias and the install prefix through real, yet both name the same directory
        let real_root = staging.join("real");
        std::fs::create_dir_all(real_root.join("bin"))
            .expect("the real install directory must be creatable");
        for executable_name in installed_executable_names() {
            std::fs::write(real_root.join("bin").join(&executable_name), b"fake-binary")
                .expect("writing the three end products must succeed");
        }
        let alias_root = staging.join("alias");
        std::os::unix::fs::symlink(&real_root, &alias_root).expect("the symlink must be creatable");

        let report = install(&InstallRequest {
            source_directory: alias_root.join("bin"),
            prefix: real_root.clone(),
            wait_for_process: None,
        })
        .expect("installing the same file onto itself must succeed");
        assert_eq!(
            report.notes.len(),
            installed_executable_names().len(),
            "every end should carry an \"already running from the install directory\" note, got {:?}",
            report.notes
        );
        for executable_name in installed_executable_names() {
            let installed = real_root.join("bin").join(&executable_name);
            assert_eq!(
                std::fs::read(&installed).expect("the installed executable must read back"),
                b"fake-binary",
                "{executable_name} was copied onto itself (truncating before reading yields an empty file)"
            );
        }
        let _ = std::fs::remove_dir_all(&staging);
    }

    /// A source directory without a single executable must fail loudly: otherwise the user believes the install worked on an empty bin directory.
    #[test]
    fn install_reports_when_the_source_directory_has_no_executable() {
        let staging = staging_area("no-executable");
        // staging_area pre-creates `payload/baihua`, so swap in an empty directory holding only configuration
        let empty_source = staging.join("payload-without-executable");
        std::fs::create_dir_all(empty_source.join("config"))
            .expect("the empty source directory must be creatable");
        let error = install(&InstallRequest {
            source_directory: empty_source,
            prefix: staging.join("target"),
            wait_for_process: None,
        })
        .expect_err("without any executable the installer must not report success");
        assert!(
            error.contains("no installable executable"),
            "the actual error: {error}"
        );
        let _ = std::fs::remove_dir_all(&staging);
    }

    #[cfg(unix)]
    #[test]
    fn install_keeps_existing_user_preferences() {
        let staging = staging_area("keep-preferences");
        let target_config = staging.join("target/config");
        std::fs::create_dir_all(&target_config)
            .expect("the target configuration directory must be creatable");
        std::fs::write(
            target_config.join("preferences.json"),
            b"{\"show_uid\":false}",
        )
        .expect("seeding the user preferences must succeed");
        install(&InstallRequest {
            source_directory: staging.join("payload"),
            prefix: staging.join("target"),
            wait_for_process: None,
        })
        .expect("installation must succeed");
        assert_eq!(
            std::fs::read_to_string(target_config.join("preferences.json")).unwrap(),
            "{\"show_uid\":false}",
            "the installer must not overwrite existing user preferences"
        );
        let _ = std::fs::remove_dir_all(&staging);
    }

    /// The installer half of the "most texts fell back to placeholder keys" fix:
    /// the language file in the user directory came from an older version and misses the new entries;
    /// installing (updates take this road too) must fill the new entries while leaving the user's own edits alone.
    #[test]
    fn install_adds_the_language_entries_the_user_copy_is_missing() {
        let staging = staging_area("merge-language");
        let source_languages = staging.join("payload/config/languages");
        let target_languages = staging.join("target/config/languages");
        std::fs::create_dir_all(&source_languages)
            .expect("the source languages directory must be creatable");
        std::fs::create_dir_all(&target_languages)
            .expect("the target languages directory must be creatable");
        std::fs::write(
            source_languages.join("zh-CN.json"),
            "{\"page_login\":\"log in\",\"message_input_placeholder\":\"type a message\"}",
        )
        .expect("writing the shipped language file must succeed");
        std::fs::write(
            target_languages.join("zh-CN.json"),
            "{\"page_login\":\"my edited login\"}",
        )
        .expect("writing the user's language file must succeed");

        install(&InstallRequest {
            source_directory: staging.join("payload"),
            prefix: staging.join("target"),
            wait_for_process: None,
        })
        .expect("installation must succeed");

        let merged: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(target_languages.join("zh-CN.json"))
                .expect("the merged result must read back"),
        )
        .expect("the merged result must be valid JSON");
        assert_eq!(
            merged.get("page_login").and_then(serde_json::Value::as_str),
            Some("my edited login"),
            "the installer must not overwrite entries the user edited"
        );
        assert_eq!(
            merged
                .get("message_input_placeholder")
                .and_then(serde_json::Value::as_str),
            Some("type a message"),
            "entries new in this version must be filled into the user's copy, or the interface shows key placeholders"
        );
        let _ = std::fs::remove_dir_all(&staging);
    }

    #[cfg(not(windows))]
    #[test]
    fn path_block_is_added_once_and_removed_without_touching_user_lines() {
        let directory =
            std::env::temp_dir().join(format!("baihua-path-block-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("the temporary directory must be creatable");
        let profile = directory.join("zshrc");
        // Whatever the user already had in the file must survive untouched
        std::fs::write(&profile, "export EDITOR=vim\n")
            .expect("seeding the user configuration must succeed");

        assert!(
            append_path_block(&profile, &directory).expect("the first write must succeed"),
            "a first install must write the PATH block"
        );
        assert!(
            !append_path_block(&profile, &directory).expect("a repeated write must not fail"),
            "reinstalling must not stack a second block"
        );
        let written = std::fs::read_to_string(&profile).expect("the startup file must read back");
        assert_eq!(
            written.matches("export PATH=").count(),
            1,
            "actual content: {written}"
        );
        assert!(
            written.contains("export EDITOR=vim"),
            "the user's own lines must not be altered"
        );

        assert!(remove_path_block(&profile).expect("removal must succeed"));
        let after = std::fs::read_to_string(&profile).expect("the startup file must read back");
        assert_eq!(
            after, "export EDITOR=vim\n",
            "after removal only the user's own content should remain"
        );
        assert!(
            !remove_path_block(&profile).expect("an absent block must not raise an error"),
            "when nothing was written there is nothing to report as removed"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn default_prefix_ends_with_client_directory() {
        let Some(prefix) = default_prefix() else {
            return;
        };
        assert_eq!(
            prefix
                .file_name()
                .map(|name| name.to_string_lossy().to_string()),
            Some("client".to_string())
        );
    }

    #[test]
    fn archive_with_unknown_extension_is_rejected() {
        let staging = staging_area("archive-format");
        let archive = staging.join("payload/baihua");
        let error = extract_archive(&archive, &staging.join("target")).unwrap_err();
        assert!(
            error.contains("unsupported archive format"),
            "actual error: {error}"
        );
        let _ = std::fs::remove_dir_all(&staging);
    }

    /// The graphical end may be a bundle, an AppImage or a bare executable, and the
    /// discovery order must prefer the bundle, then the AppImage, then the executable.
    #[test]
    fn graphical_payload_prefers_the_bundle_then_the_app_image() {
        let area = staging_area("graphical-payload");
        let prefix = area.join("target");
        assert_eq!(
            installed_payload(&prefix, &graphical_executable_name()),
            None,
            "an empty prefix has no graphical end at all"
        );
        std::fs::create_dir_all(prefix.join("bin"))
            .expect("the binary directory must be creatable");
        std::fs::write(prefix.join("bin").join(graphical_executable_name()), b"exe")
            .expect("the bare executable must be writable");
        assert_eq!(
            installed_payload(&prefix, &graphical_executable_name()),
            Some(prefix.join("bin").join(graphical_executable_name())),
            "with only the bare executable installed, that is the payload"
        );
        std::fs::write(prefix.join("bin").join(graphical_app_image_name()), b"app")
            .expect("the AppImage must be writable");
        assert_eq!(
            installed_payload(&prefix, &graphical_executable_name()),
            Some(prefix.join("bin").join(graphical_app_image_name())),
            "an AppImage outranks the bare executable"
        );
        std::fs::create_dir_all(prefix.join(application_bundle_name()))
            .expect("the bundle directory must be creatable");
        assert_eq!(
            installed_payload(&prefix, &graphical_executable_name()),
            Some(prefix.join(application_bundle_name())),
            "the application bundle outranks everything else"
        );
        assert_eq!(
            installed_payload(&prefix, &terminal_executable_name()),
            None,
            "the other ends never resolve to a graphical payload"
        );
        let _ = std::fs::remove_dir_all(&area);
    }

    /// An AppImage package is the program: installing it copies one file into the prefix
    /// and marks it executable, and the payload discovery finds it right afterwards.
    #[test]
    fn app_image_package_installs_as_one_executable_file() {
        let area = staging_area("app-image");
        let package_path = area.join("baihua-gui-9.9.9-x86_64-unknown-linux-gnu.AppImage");
        std::fs::write(&package_path, b"fake-app-image")
            .expect("the package file must be writable");
        let prefix = area.join("target");
        let report = install_app_image(&package_path, &prefix).expect("the install must succeed");
        assert_eq!(report.copied_file_count, 1);
        assert_eq!(
            report.executable_paths,
            vec![prefix.join("bin").join(graphical_app_image_name())]
        );
        assert_eq!(
            std::fs::read(prefix.join("bin").join(graphical_app_image_name()))
                .expect("the installed file must exist"),
            b"fake-app-image"
        );
        assert!(
            installed_payload(&prefix, &graphical_executable_name()).is_some(),
            "the freshly installed AppImage must be discoverable as the graphical end"
        );
        let _ = std::fs::remove_dir_all(&area);
    }

    /// Every place the graphical end may live is reported in priority order, so the
    /// command line can point a user at the app bundle instead of guessing.
    #[test]
    fn graphical_payloads_are_listed_in_priority_order() {
        let prefix = PathBuf::from("/tmp/prefix");
        assert_eq!(
            graphical_payloads(&prefix),
            vec![
                prefix.join("Baihua.app"),
                prefix.join("bin").join("Baihua.AppImage"),
                prefix.join("bin").join(graphical_executable_name()),
            ]
        );
    }

    /// The disk-image and installer payloads are found by name at any depth, because
    /// the mount point and the administrative-extract tree both wrap extra directories.
    #[test]
    fn payloads_are_found_at_any_depth_below_the_root() {
        let area = staging_area("find-payloads");
        let nested = area.join("payload/Media/Applications/Baihua.app/Contents/MacOS");
        std::fs::create_dir_all(&nested).expect("the nested tree must be creatable");
        std::fs::write(nested.join(graphical_executable_name()), b"exe")
            .expect("the inner executable must be writable");
        assert_eq!(
            find_directory_named(&area.join("payload"), &application_bundle_name()),
            Some(
                area.join("payload/Media/Applications")
                    .join(application_bundle_name())
            ),
            "the bundle must be found through the mount layout"
        );
        assert_eq!(
            find_file_by_name(&area.join("payload"), &graphical_executable_name()),
            Some(nested.join(graphical_executable_name())),
            "the program file must be found through the installer layout"
        );
        assert_eq!(
            find_directory_named(&area.join("payload"), "NoSuchThing.app"),
            None,
            "a name that is not there must not be invented"
        );
        let _ = std::fs::remove_dir_all(&area);
    }

    /// The bundle lands in the first candidate directory that accepts writes; a path
    /// blocked by a regular file must be skipped, never chosen.
    #[test]
    fn bundle_installs_into_the_first_writable_directory() {
        let area = staging_area("application-install");
        let blocked = area.join("blocked");
        std::fs::write(&blocked, b"not a directory").expect("the blocker must be writable");
        let writable = area.join("writable");
        let candidates: Vec<PathBuf> = vec![blocked.clone(), writable.clone()];
        assert_eq!(
            application_install_directory_from(&candidates),
            writable,
            "the blocked candidate must be passed over"
        );
        let blocked_only: Vec<PathBuf> = vec![blocked.clone()];
        assert_eq!(
            application_install_directory_from(&blocked_only),
            blocked,
            "when nothing accepts writes the last candidate still reports its own failure"
        );
        let _ = std::fs::remove_dir_all(&area);
    }

    /// On macOS the search starts at /Applications and always ends with the prefix
    /// payloads, so a dragged-in bundle outranks a historic prefix installation.
    #[test]
    fn graphical_candidates_start_with_applications_on_macos() {
        let candidates = graphical_application_candidates();
        if cfg!(target_os = "macos") {
            assert_eq!(
                candidates
                    .first()
                    .map(|path| path.to_string_lossy().to_string()),
                Some("/Applications/Baihua.app".to_string()),
                "the system Applications directory must come first"
            );
            let prefix = default_prefix().expect("the tests run with HOME set");
            assert_eq!(
                candidates.last(),
                Some(&prefix.join("bin").join(graphical_executable_name())),
                "the prefix payloads must close the search order"
            );
        } else {
            let prefix = default_prefix().expect("the tests run with HOME set");
            assert_eq!(candidates, graphical_payloads(&prefix));
        }
    }
}
