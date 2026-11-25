//! SnapShots on Linux, taken the way Electron takes them (apps/desktop/src/snapShot:
//! `LinuxSnapShot.ts`, `GnomeCaptureSetup.ts`, `NiriSnapShot.ts`, `linuxCaptureSession.ts`).
//! Wayland only, as Electron's `captureMode`. Hyprland (through `hyprctl` and `grim` instead of
//! Electron's helper) and Niri (its IPC socket) come first, then KWin's ScreenShot2 on KDE Plasma
//! (called from this binary instead of Electron's `t3-kde-snap-shot` helper, `KdeSnapShot.ts`),
//! the Screenshot portal's active-window target, the bundled GNOME extension, else the portal's
//! own picker. App text comes from AT-SPI, as Electron's accessibility module reads it on Linux.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Electron's Linux desktop entry name: the GNOME extension only answers its `.SnapShot` name.
pub const APP_ID: &str = "com.t3tools.T3Code";
pub const EXTENSION_UUID: &str = "snap-shot@t3.codes";
const MAX_PNG_BYTES: usize = 32 * 1024 * 1024;
/// The KDE helper's cap on KWin's raw pixels.
const MAX_RAW_BYTES: usize = 128 * 1024 * 1024;
const MAX_CAPTURE: (f64, f64) = (2_560.0, 1_600.0);
/// KWin authorizes ScreenShot2 callers by an entry whose Exec is their executable. Electron's
/// helper owns `com.t3tools.T3Code.KdeCapture.desktop`, so this one sits beside it.
const KDE_ENTRY: &str = "com.t3tools.T3Code.RuneCapture.desktop";
const KDE_MARKER: &str = "X-T3Code-Rune-Capture=true";
const KDE_PENDING: &str = "KDE Plasma is still registering T3 Code for active-window snapshots, so this one used the window picker.";
const KDE_DENIED: &str = "KDE Plasma hasn't allowed T3 Code to capture windows. Sign out and back in, then try again.";
/// This run wrote the entry, so a refusal may only mean KDE hasn't read it yet.
static KDE_REGISTERED: AtomicBool = AtomicBool::new(false);
pub const MODIFIER_PAIR_UNAVAILABLE: &str = "Modifier-pair shortcuts aren't available in this Wayland session. Choose another shortcut or use Take snapshot from the command palette.";

/// The extension Electron bundles, installed as `installGnomeCaptureBundle` does.
const EXTENSION_FILES: [(&str, &str); 5] = [
    ("metadata.json", include_str!("../../../desktop/gnome-extension/metadata.json")),
    ("extension.js", include_str!("../../../desktop/gnome-extension/extension.js")),
    ("captureService.js", include_str!("../../../desktop/gnome-extension/captureService.js")),
    ("captureFeedback.js", include_str!("../../../desktop/gnome-extension/captureFeedback.js")),
    ("feedbackGeometry.js", include_str!("../../../desktop/gnome-extension/feedbackGeometry.js")),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    ScreenshotPortal,
    GnomeExtension,
    Picker,
    Hyprland,
    Niri,
    /// KWin's ScreenShot2, with the KDE helper's KWin-script lookup naming the window.
    Kde,
}

impl Backend {
    /// Names the captured window's process, so its app text can be read.
    pub fn knows_window(self) -> bool {
        matches!(self, Backend::GnomeExtension | Backend::Hyprland | Backend::Niri | Backend::Kde)
    }
}

/// The captured window, as the compositor described it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Window {
    pub title: String,
    pub app: String,
    pub pid: u32,
    /// Output-local points, where the compositor gives a comparable origin (Hyprland).
    pub bounds: Option<[f64; 4]>,
}

/// The effects a capture asks for. GNOME's extension plays them inside the compositor.
#[derive(Clone, Copy, Debug, Default)]
pub struct Feedback {
    pub flash: bool,
    pub animate: bool,
    /// The composer's frame as fractions of our window: where the extension's flight lands.
    pub relative: Option<[f64; 4]>,
}

pub struct Captured {
    pub window: Option<Window>,
    /// Our window's frame in the same output-local points as `window.bounds`.
    pub own: Option<[f64; 4]>,
    /// The compositor already flashed and flew the picture.
    pub effects_done: bool,
    /// Why this capture went another way than the backend's, for a toast.
    pub notice: Option<String>,
}

/// `DesktopCaptureExtensionState.status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionStatus {
    NotInstalled,
    RestartRequired,
    UpdateRequired,
    ExtensionsDisabled,
    Disabled,
    Enabled,
    Unsupported,
    Error,
}

/// What `getLinuxCaptureSupport` and `GnomeCaptureSetup.state` found.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub backend: Backend,
    /// GNOME sessions get the extension setup.
    pub extension: Option<(ExtensionStatus, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Install,
    Enable,
    Disable,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// `isWaylandSession`.
pub fn wayland() -> bool {
    cfg!(target_os = "linux") && (env("XDG_SESSION_TYPE").is_some_and(|t| t.eq_ignore_ascii_case("wayland")) || env("WAYLAND_DISPLAY").is_some())
}

/// `isGnomeCaptureSession`: GNOME outside a Flatpak or Snap sandbox.
pub fn gnome_session(desktop: Option<&str>, sandboxed: bool) -> bool {
    !sandboxed && desktop.is_some_and(|d| d.split(':').any(|name| name.eq_ignore_ascii_case("gnome")))
}

fn sandboxed() -> bool {
    env("FLATPAK_ID").is_some() || env("SNAP").is_some()
}

/// `isDesktopSession`.
fn desktop_session(desktop: Option<&str>, sandboxed: bool, name: &str) -> bool {
    !sandboxed && desktop.is_some_and(|d| d.split(':').any(|n| n.eq_ignore_ascii_case(name)))
}

/// `isHyprlandCaptureSession`.
pub fn hyprland_session() -> bool {
    wayland() && desktop_session(env("XDG_CURRENT_DESKTOP").as_deref(), sandboxed(), "hyprland")
}

/// `niriSocketPath`.
fn niri_socket() -> Option<PathBuf> {
    if !desktop_session(env("XDG_CURRENT_DESKTOP").as_deref(), sandboxed(), "niri") {
        return None;
    }
    env("NIRI_SOCKET").map(PathBuf::from).filter(|p| p.is_absolute())
}

/// `isKdeCaptureSession`.
fn kde_session() -> bool {
    desktop_session(env("XDG_CURRENT_DESKTOP").as_deref(), sandboxed(), "kde")
}

/// `LinuxCaptureConnection.backend`: KWin's ScreenShot2 (version 2, probed on KDE only), else the
/// active-window portal target (version 3, target bit 8), else the extension on GNOME (API
/// version 1 or 2), else the picker.
pub fn backend_from(kwin: Option<u32>, portal_version: Option<u32>, targets: Option<u32>, gnome: bool, extension_api: Option<u32>) -> Backend {
    if kwin.is_some_and(|v| v >= 2) {
        Backend::Kde
    } else if portal_version.is_some_and(|v| v >= 3) && targets.is_some_and(|t| t & 8 != 0) {
        Backend::ScreenshotPortal
    } else if gnome && matches!(extension_api, Some(1 | 2)) {
        Backend::GnomeExtension
    } else {
        Backend::Picker
    }
}

/// The bundled metadata's version and supported shell majors.
pub fn bundled() -> (u32, Vec<String>) {
    metadata(EXTENSION_FILES[0].1).unwrap_or_default()
}

/// `decodeMetadata`: the extension's version and shell majors, when the uuid is ours.
pub fn metadata(json: &str) -> Option<(u32, Vec<String>)> {
    let v: Value = serde_json::from_str(json).ok()?;
    if v["uuid"] != EXTENSION_UUID {
        return None;
    }
    let shells = v["shell-version"].as_array()?.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect();
    Some((v["version"].as_u64()? as u32, shells))
}

/// What GNOME reported: `org.gnome.Shell.Extensions` properties and `GetExtensionInfo`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shell {
    pub version: String,
    pub user_extensions: bool,
    pub state: Option<f64>,
    pub loaded_version: Option<f64>,
    pub error: Option<String>,
}

/// `GnomeCaptureSetup.state`.
pub fn extension_status(shell: &Shell, installed: Option<u32>, bundled: &(u32, Vec<String>)) -> (ExtensionStatus, String) {
    use ExtensionStatus as S;
    let major = shell.version.split('.').next().unwrap_or_default();
    let state = shell.state.filter(|s| *s != 0.0);
    if !bundled.1.iter().any(|v| v == major) {
        return (S::Unsupported, format!("The bundled extension supports GNOME {}. This session runs GNOME {major}.", bundled.1.join(", ")));
    }
    if state.is_none() && installed.is_none() {
        return (S::NotInstalled, "Install the bundled extension to capture the active window without a picker. No download or administrator password is needed.".into());
    }
    if let Some(installed) = installed
        && (state.is_none() || shell.loaded_version.is_some_and(|v| f64::from(installed) > v))
    {
        return (S::RestartRequired, "Installed. Save your work, sign out of GNOME and sign back in, then return here to enable the extension. Restarting T3 Code alone is not enough.".into());
    }
    let current = installed.map(f64::from).or(shell.loaded_version).unwrap_or(0.0);
    if current < f64::from(bundled.0) {
        return (S::UpdateRequired, "A newer extension is bundled with this app. Install it, then sign out and back in to load the update.".into());
    }
    if !shell.user_extensions {
        return (S::ExtensionsDisabled, "GNOME has disabled user extensions. Turn on Extensions in the GNOME Extensions app, then check again. T3 Code will not enable your other extensions for you.".into());
    }
    match state {
        Some(1.0) => (S::Enabled, "The T3 Code extension is running. Active-window snapshots are available.".into()),
        Some(3.0 | 4.0) => (
            S::Error,
            shell.error.clone().filter(|e| !e.is_empty()).unwrap_or_else(|| "GNOME could not load the extension. Check GNOME Extensions for details, or sign out and back in.".into()),
        ),
        _ => (S::Disabled, "Enable the T3 Code extension to allow active-window snapshots. You can disable it here at any time.".into()),
    }
}

/// `readPortalPng`'s `fileURLToPath`: local `file://` URIs only, percent-decoded.
pub fn uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    if !path.starts_with('/') {
        return None;
    }
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let path = String::from_utf8(out).ok()?;
    (!path.contains('\0')).then(|| PathBuf::from(path))
}

/// The size to store when a capture exceeds 2560×1600 (`resizeLinuxCapture`).
pub fn fitted(width: u32, height: u32) -> Option<(u32, u32)> {
    let (w, h) = (f64::from(width.max(1)), f64::from(height.max(1)));
    let scale = (MAX_CAPTURE.0 / w).min(MAX_CAPTURE.1 / h).min(1.0);
    (scale < 1.0).then(|| (((w * scale).round() as u32).max(1), ((h * scale).round() as u32).max(1)))
}

/// Writes a PNG the desktop returned into `path`, checked and scaled as Electron stores it.
fn store_png(bytes: &[u8], path: &Path) -> Result<(), String> {
    const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() > MAX_PNG_BYTES || !bytes.starts_with(&PNG) {
        return Err("Invalid or oversized window screenshot.".into());
    }
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).map_err(|_| "The window screenshot is empty.".to_owned())?;
    if fitted(image.width(), image.height()).is_none() {
        return std::fs::write(path, bytes).map_err(|_| "Could not save the snapshot.".to_owned());
    }
    save_image(image, path)
}

/// Encodes a decoded capture into `path` as PNG, scaled as Electron stores it.
fn save_image(image: image::DynamicImage, path: &Path) -> Result<(), String> {
    let image = match fitted(image.width(), image.height()) {
        Some((w, h)) => image.resize_exact(w, h, image::imageops::FilterType::Lanczos3),
        None => image,
    };
    image.save_with_format(path, image::ImageFormat::Png).map_err(|_| "Could not save the snapshot.".to_owned())
}

/// `escapeDesktopEntryExecArgument`: Exec values are unescaped as strings, then unquoted, so
/// escape in reverse order.
pub fn desktop_exec_argument(value: &str) -> String {
    let quoted = value.replace('\\', "\\\\").replace('`', "\\`").replace('$', "\\$").replace('"', "\\\"").replace('%', "%%");
    format!("\"{quoted}\"").replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")
}

/// `kdeCaptureDesktopEntry` naming this executable, which is what KWin authorizes.
pub fn kde_desktop_entry(executable: &str) -> String {
    [
        "[Desktop Entry]",
        "Type=Application",
        "Name=T3 Code SnapShots",
        "NoDisplay=true",
        &format!("Exec={}", desktop_exec_argument(executable)),
        // KService reads this custom property as a KConfig list, not an XDG ';' list.
        "X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2",
        KDE_MARKER,
        "",
    ]
    .join("\n")
}

/// Whether our entry needs writing: missing or different (another Exec, an older format). A file
/// at our name without our marker belongs to someone else and is never replaced.
pub fn kde_entry_stale(existing: Option<&str>, expected: &str) -> Result<bool, String> {
    match existing {
        None => Ok(true),
        Some(entry) if entry == expected => Ok(false),
        Some(entry) if entry.lines().any(|line| line == KDE_MARKER) => Ok(true),
        Some(_) => Err("Another desktop entry uses T3 Code's capture name. Rename it, then try again.".into()),
    }
}

/// Registers this executable with KWin when the entry is missing or names another one.
fn ensure_kde_entry() -> Result<(), String> {
    let failed = |e: std::io::Error| format!("Could not register T3 Code with KDE Plasma: {e}");
    let executable = std::env::current_exe().map_err(failed)?;
    let dir = data_home().ok_or("Could not find your data folder.")?.join("applications");
    let path = dir.join(KDE_ENTRY);
    let expected = kde_desktop_entry(&executable.to_string_lossy());
    if std::fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file()) {
        return Err("T3 Code's KDE desktop entry must be a regular file. Remove the conflicting link, then try again.".into());
    }
    let existing = match std::fs::read_to_string(&path) {
        Ok(entry) => Some(entry),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(failed(e)),
    };
    if !kde_entry_stale(existing.as_deref(), &expected)? {
        return Ok(());
    }
    std::fs::create_dir_all(&dir).map_err(failed)?;
    // Not `.desktop` while staged, so KService never reads a partial entry.
    let staged = dir.join(format!(".{KDE_ENTRY}.{}.tmp", std::process::id()));
    let written = std::fs::write(&staged, &expected).and_then(|()| std::fs::rename(&staged, &path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&staged);
        return Err(failed(e));
    }
    KDE_REGISTERED.store(true, Ordering::Relaxed);
    // KWin checks callers against KService's cache, so rebuild it now, then again in the session
    // manager's environment: the cache's name hashes XDG_DATA_DIRS, which an AppImage changes.
    if let Some(tool) = ["kbuildsycoca6", "kbuildsycoca5"].into_iter().find(|tool| run_quiet(tool, &["--noincremental"]).is_some()) {
        let _ = run_quiet("systemd-run", &["--user", "--quiet", "--wait", "--collect", "--pipe", "--service-type=exec", tool, "--noincremental"]);
    }
    Ok(())
}

/// Runs a tool without output, stopped after 20 s as Electron's `execFile` is; None when it
/// couldn't start, else whether it succeeded.
fn run_quiet(program: &str, args: &[&str]) -> Option<bool> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status.success()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Some(false);
            }
        }
    }
}

/// KWin's `CaptureActiveWindow` results: how the raw QImage pixels it wrote are laid out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawMeta {
    pub kind: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub stride: Option<u32>,
    pub format: Option<u32>,
}

/// Why KWin's capture failed; `Denied` is its permission check.
#[derive(Debug)]
pub enum KdeError {
    Denied,
    Failed(String),
}

/// The KDE helper's `rgba`: Qt's RGB32, ARGB32 or RGBA8888 (premultiplied or not) rows as
/// straight RGBA, with the width and height. Unknown types and formats are refused.
pub fn kde_rgba(meta: &RawMeta, raw: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let invalid = || "KDE Plasma returned an unreadable screenshot.".to_owned();
    let (Some("raw"), Some(width), Some(height), Some(format)) = (meta.kind.as_deref(), meta.width, meta.height, meta.format) else { return Err(invalid()) };
    let (w, h) = (width as usize, height as usize);
    let row = w.checked_mul(4).ok_or_else(invalid)?;
    let stride = meta.stride.map_or(row, |s| s as usize);
    let bytes = stride.checked_mul(h).ok_or_else(invalid)?;
    if w == 0 || h == 0 || stride < row || bytes > MAX_RAW_BYTES || ![4, 5, 6, 17, 18].contains(&format) {
        return Err(invalid());
    }
    if raw.len() < bytes {
        return Err("KDE Plasma returned an incomplete screenshot.".into());
    }
    let mut out = Vec::with_capacity(row * h);
    for y in 0..h {
        for pixel in raw[y * stride..y * stride + row].chunks_exact(4) {
            let (r, g, b, a) = if format >= 17 {
                (pixel[0], pixel[1], pixel[2], pixel[3])
            } else {
                let argb = u32::from_ne_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]);
                ((argb >> 16) as u8, (argb >> 8) as u8, argb as u8, if format == 4 { 255 } else { (argb >> 24) as u8 })
            };
            let straight = |v: u8| if a == 0 { 0 } else { ((u32::from(v) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8 };
            if format == 6 || format == 18 {
                out.extend_from_slice(&[straight(r), straight(g), straight(b), a]);
            } else {
                out.extend_from_slice(&[r, g, b, a]);
            }
        }
    }
    Ok((width, height, out))
}

/// The KDE helper's `capture_title`: a leading CLI spinner frame doesn't change the window.
fn capture_title(title: &str) -> &str {
    let title = title.trim();
    let mut chars = title.chars();
    if matches!(chars.next(), Some('\u{280b}' | '\u{2819}' | '\u{2839}' | '\u{2838}' | '\u{283c}' | '\u{2834}' | '\u{2826}' | '\u{2827}' | '\u{2807}' | '\u{280f}'))
        && chars.clone().next().is_some_and(char::is_whitespace)
    {
        let label = chars.as_str().trim_start();
        if !label.is_empty() {
            return label;
        }
    }
    title
}

/// What the KWin script read: the window's id for `CaptureWindow`, the window, and this app's
/// own frame. Frames are output-local and kept only on a single output, as Hyprland's are.
#[derive(Clone, Debug, PartialEq)]
pub struct KwinLookup {
    pub id: String,
    pub window: Window,
    pub own: Option<[f64; 4]>,
    identity: (String, [f64; 4]),
}

/// The script's reply (`KWIN_LOOKUP`).
pub fn parse_kwin(json: &str) -> Option<KwinLookup> {
    let v: Value = serde_json::from_str(json).ok()?;
    let w = &v["window"];
    let rect = |r: &Value| Some([r["x"].as_f64()?, r["y"].as_f64()?, r["width"].as_f64()?, r["height"].as_f64()?]).filter(|r| r[2] > 0.0 && r[3] > 0.0);
    let frame = rect(&w["bounds"])?;
    let single = v["screens"].as_u64() == Some(1);
    let local = |r: [f64; 4], output: &Value| rect(output).map(|o| [r[0] - o[0], r[1] - o[1], r[2], r[3]]);
    let pid = w["processId"].as_u64().and_then(|p| u32::try_from(p).ok()).unwrap_or(0);
    let window = Window {
        title: w["title"].as_str().unwrap_or_default().to_owned(),
        app: w["appName"].as_str().unwrap_or("Application").to_owned(),
        pid,
        bounds: single.then(|| local(frame, &w["output"])).flatten(),
    };
    let own = single.then(|| rect(&v["own"]["bounds"]).and_then(|r| local(r, &v["own"]["output"]))).flatten();
    let identity = (format!("{}\u{0}{}\u{0}{}", w["appIdentifier"].as_str().unwrap_or_default(), window.app, capture_title(&window.title)), frame);
    Some(KwinLookup { id: w["id"].as_str()?.to_owned(), window, own, identity })
}

impl KwinLookup {
    /// `matches_after_capture`: still the same window, frame and app once the capture is done.
    pub fn same_window(&self, after: &Self) -> bool {
        self.id == after.id && self.window.pid == after.window.pid && self.identity == after.identity
    }
}

/// The KDE helper's `window` script, also naming this app's window for the fly-in's origin.
/// `__SELECT__` and `__PID__` are filled in per call.
pub const KWIN_LOOKUP: &str = "function rect(r) { return { x: Math.round(r.x), y: Math.round(r.y), width: Math.round(r.width), height: Math.round(r.height) }; }
function frame(w) { return { bounds: rect(w.frameGeometry), output: w.output ? rect(w.output.geometry) : null }; }
const w = __SELECT__;
const own = workspace.windowList().find(o => o.pid === __PID__ && o.normalWindow);
reply({ window: !w ? null : Object.assign({ id: String(w.internalId), title: w.caption, appName: w.resourceClass || w.desktopFileName || 'Application', appIdentifier: w.desktopFileName || w.resourceClass || '', processId: w.pid }, frame(w)), own: own ? frame(own) : null, screens: workspace.screens ? workspace.screens.length : 0 });";

/// The script for the active window (`None`) or the one with `id`.
pub fn kwin_lookup_script(id: Option<&str>, pid: u32) -> String {
    let select = id.map_or_else(|| "workspace.activeWindow".to_owned(), |id| format!("workspace.windowList().find(w => String(w.internalId) === {})", Value::from(id)));
    KWIN_LOOKUP.replace("__SELECT__", &select).replace("__PID__", &pid.to_string())
}

/// The extension's window metadata. Its bounds are GNOME's, used only by the extension.
pub fn parse_window(json: &str) -> Option<Window> {
    let v: Value = serde_json::from_str(json).ok()?;
    let pid = v["processId"].as_u64().and_then(|p| u32::try_from(p).ok()).unwrap_or(0);
    Some(Window { title: v["title"].as_str()?.to_owned(), app: v["appName"].as_str()?.to_owned(), pid, bounds: None })
}

/// Hyprland's `activewindow`/`clients` entry: its frame in layout points, less the monitor's origin.
pub fn hyprland_window(v: &Value, origin: Option<(f64, f64)>) -> Option<Window> {
    let pair = |key: &str| Some((v[key][0].as_f64()?, v[key][1].as_f64()?));
    let ((x, y), (w, h)) = (pair("at")?, pair("size")?);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let pid = v["pid"].as_i64().and_then(|p| u32::try_from(p).ok()).unwrap_or(0);
    let bounds = origin.map(|(ox, oy)| [x - ox, y - oy, w, h]);
    Some(Window { title: v["title"].as_str().unwrap_or_default().to_owned(), app: v["class"].as_str().unwrap_or_default().to_owned(), pid, bounds })
}

/// `grim -g`'s geometry for a Hyprland window, in layout points.
pub fn grim_geometry(v: &Value) -> Option<String> {
    let n = |key: &str, i: usize| v[key][i].as_f64().map(|f| f.round() as i64);
    Some(format!("{},{} {}x{}", n("at", 0)?, n("at", 1)?, n("size", 0)?, n("size", 1)?))
}

/// Niri's `Version` reply: 25.11 brought caller-chosen screenshot paths and completion events.
pub fn niri_supported(version: &str) -> bool {
    let version = version.strip_prefix("niri ").unwrap_or(version);
    let mut parts = version.split(|c: char| !c.is_ascii_digit());
    let major = parts.next().and_then(|p| p.parse::<u32>().ok());
    let minor = parts.next().and_then(|p| p.parse::<u32>().ok());
    matches!((major, minor), (Some(m), Some(n)) if m > 25 || (m == 25 && n >= 11))
}

/// A Niri window, when it is still the one that was focused when the capture started.
pub fn niri_window(before: &Value, after: Option<&Value>) -> Option<Window> {
    let after = after?;
    let same = ["pid", "title", "app_id"].iter().all(|k| before[k] == after[k]) && before["layout"]["window_size"] == after["layout"]["window_size"];
    same.then(|| Window {
        title: before["title"].as_str().unwrap_or_default().to_owned(),
        app: before["app_id"].as_str().unwrap_or("Application").to_owned(),
        pid: before["pid"].as_u64().and_then(|p| u32::try_from(p).ok()).unwrap_or(0),
        // Niri reports a logical size, not a comparable screen origin.
        bounds: None,
    })
}

/// The overlay's display space on Hyprland: one monitor, so output-local points are unambiguous.
fn hyprland_origin() -> Option<(f64, f64)> {
    let monitors = hyprland::query("monitors").ok()?;
    match monitors.as_array().map(Vec::as_slice) {
        Some([only]) => Some((only["x"].as_f64()?, only["y"].as_f64()?)),
        _ => None,
    }
}

mod hyprland {
    use serde_json::Value;
    use std::process::{Command, Stdio};

    fn missing(tool: &str) -> String {
        format!("Install {tool} to capture windows on Hyprland.")
    }

    pub fn query(what: &str) -> Result<Value, String> {
        let out = Command::new("hyprctl").args(["-j", what]).stdin(Stdio::null()).output().map_err(|_| missing("hyprctl"))?;
        if !out.status.success() {
            return Err("Hyprland did not respond. Check that hyprctl works in this session.".into());
        }
        serde_json::from_slice(&out.stdout).map_err(|_| "Hyprland returned an unreadable reply.".to_owned())
    }

    pub fn check() -> Result<(), String> {
        query("version")?;
        Command::new("grim").arg("-h").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().map_err(|_| missing("grim"))?;
        Ok(())
    }

    /// The focused window's PNG from `grim`, and the window.
    pub fn capture() -> Result<(Vec<u8>, Value), String> {
        let active = query("activewindow")?;
        let geometry = super::grim_geometry(&active).ok_or("Hyprland has no focused window to capture.")?;
        let out = Command::new("grim").args(["-g", &geometry, "-t", "png", "-"]).stdin(Stdio::null()).output().map_err(|_| missing("grim"))?;
        if !out.status.success() {
            let detail = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            return Err(if detail.is_empty() { "grim could not capture the window.".into() } else { detail });
        }
        Ok((out.stdout, active))
    }

    /// Our own window, for where a flight lands.
    pub fn own() -> Option<Value> {
        let pid = i64::from(std::process::id());
        query("clients").ok()?.as_array()?.iter().find(|c| c["pid"].as_i64() == Some(pid) && c["mapped"] != false).cloned()
    }
}

/// Niri runs only on Linux; elsewhere there is never a `NIRI_SOCKET` to reach.
#[cfg(not(unix))]
mod niri {
    use std::path::Path;

    pub fn check(_: &Path) -> Result<(), String> {
        Err("Niri is not available on this system.".into())
    }

    pub fn capture(path: &Path) -> Result<(Vec<u8>, Option<super::Window>), String> {
        check(path).map(|()| (Vec::new(), None))
    }
}

#[cfg(unix)]
mod niri {
    use serde_json::{Value, json};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::time::{Duration, Instant};

    const MAX_MESSAGE_BYTES: u64 = 4 * 1024 * 1024;
    const TIMEOUT: Duration = Duration::from_secs(5);

    struct Socket(BufReader<UnixStream>);

    impl Socket {
        fn open(path: &Path) -> Result<Self, String> {
            let stream = UnixStream::connect(path).map_err(|_| "Could not reach Niri. Check that NIRI_SOCKET is set.".to_owned())?;
            stream.set_read_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
            stream.set_write_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
            Ok(Self(BufReader::new(stream)))
        }

        fn send(&mut self, message: &Value) -> Result<(), String> {
            let line = format!("{message}\n");
            self.0.get_mut().write_all(line.as_bytes()).map_err(|_| "Niri closed the connection.".to_owned())
        }

        /// One reply or event; bounded so an unterminated message can't grow without limit.
        fn next(&mut self) -> Result<Value, String> {
            let mut line = Vec::new();
            let read = self.0.by_ref().take(MAX_MESSAGE_BYTES + 1).read_until(b'\n', &mut line).map_err(|_| "Niri did not respond.".to_owned())?;
            if read == 0 {
                return Err("Niri closed the connection.".into());
            }
            if line.len() as u64 > MAX_MESSAGE_BYTES {
                return Err("Niri returned an oversized message.".into());
            }
            serde_json::from_slice(&line).map_err(|_| "Niri returned an unreadable message.".to_owned())
        }
    }

    fn request(path: &Path, message: Value) -> Result<Value, String> {
        let mut socket = Socket::open(path)?;
        socket.send(&message)?;
        let mut reply = socket.next()?;
        match (reply.get_mut("Ok").map(Value::take), reply["Err"].as_str()) {
            (Some(ok), _) => Ok(ok),
            (None, Some(err)) => Err(format!("Niri: {err}")),
            _ => Err("Niri returned an unreadable reply.".into()),
        }
    }

    pub fn check(path: &Path) -> Result<(), String> {
        let version = request(path, json!("Version"))?;
        if super::niri_supported(version["Version"].as_str().unwrap_or_default()) { Ok(()) } else { Err("SnapShots require Niri 25.11 or newer.".into()) }
    }

    /// `captureNiriWindow`: subscribe, screenshot the focused window to our path, wait for it.
    pub fn capture(path: &Path) -> Result<(Vec<u8>, Option<super::Window>), String> {
        let mut events = Socket::open(path)?;
        events.send(&json!("EventStream"))?;
        // An initial state event proves the subscription; the EventStream reply alone doesn't.
        let deadline = Instant::now() + TIMEOUT;
        while events.next()?.get("WindowsChanged").is_none() {
            if Instant::now() > deadline {
                return Err("Niri did not respond.".into());
            }
        }
        let focused = request(path, json!("FocusedWindow"))?["FocusedWindow"].take();
        let id = focused["id"].as_u64().ok_or("Niri has no focused window to capture.")?;
        let dir = std::env::temp_dir().join(format!("t3-niri-capture-{}-{}", std::process::id(), s5_net::uuid()));
        std::fs::create_dir(&dir).map_err(|_| "Could not prepare the snapshot.".to_owned())?;
        let image = dir.join("capture.png");
        let result = (|| {
            let target = image.to_string_lossy().into_owned();
            request(path, json!({"Action": {"ScreenshotWindow": {"id": id, "write_to_disk": true, "show_pointer": false, "path": target}}}))?;
            let deadline = Instant::now() + TIMEOUT;
            loop {
                let event = events.next()?;
                if event["ScreenshotCaptured"]["path"].as_str() == Some(target.as_str()) {
                    break;
                }
                if Instant::now() > deadline {
                    return Err("Niri did not finish the snapshot.".to_owned());
                }
            }
            let png = std::fs::read(&image).map_err(|_| "Incomplete screenshot file.".to_owned())?;
            let windows = request(path, json!("Windows"))?;
            let after = windows["Windows"].as_array().and_then(|all| all.iter().find(|w| w["id"].as_u64() == Some(id)));
            // A changed or closed window still gives the image, never text from its replacement.
            Ok((png, super::niri_window(&focused, after)))
        })();
        // The directory is ours; the compositor's own screenshot folder is never touched.
        let _ = std::fs::remove_dir_all(&dir);
        result
    }
}


fn data_home() -> Option<PathBuf> {
    env("XDG_DATA_HOME").map(PathBuf::from).or_else(|| env("HOME").map(|h| Path::new(&h).join(".local/share")))
}

fn installed_dir() -> Option<PathBuf> {
    Some(data_home()?.join("gnome-shell/extensions").join(EXTENSION_UUID))
}

fn installed_version() -> Option<u32> {
    metadata(&std::fs::read_to_string(installed_dir()?.join("metadata.json")).ok()?).map(|(v, _)| v)
}

/// `installGnomeCaptureBundle`: stage the files, keep a newer install, back up an older one.
fn install() -> Result<(), String> {
    let failed = |e: std::io::Error| format!("Could not install the extension: {e}");
    let target = installed_dir().ok_or("Could not find your data folder.")?;
    let parent = target.parent().ok_or("Could not find your data folder.")?.to_owned();
    std::fs::create_dir_all(&parent).map_err(failed)?;
    let existing = std::fs::symlink_metadata(&target).ok();
    if existing.as_ref().is_some_and(|m| !m.is_dir() || m.file_type().is_symlink()) {
        return Err("The extension installation is not a regular directory. Manage it in GNOME Extensions instead.".into());
    }
    if existing.is_some() && installed_version().is_some_and(|v| v > bundled().0) {
        return Err("A newer extension is installed. Update T3 Code instead of replacing it.".into());
    }
    let staged = parent.join(format!(".t3-capture-install-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staged);
    std::fs::create_dir_all(&staged).map_err(failed)?;
    let result = (|| {
        for (name, body) in EXTENSION_FILES {
            std::fs::write(staged.join(name), body).map_err(failed)?;
        }
        let mut backup = None;
        if existing.is_some() {
            let dir = data_home().ok_or("Could not find your data folder.")?.join("t3code/extension-backups").join(format!("capture-{}", std::process::id()));
            std::fs::create_dir_all(&dir).map_err(failed)?;
            let to = dir.join(EXTENSION_UUID);
            std::fs::rename(&target, &to).map_err(failed)?;
            backup = Some(to);
        }
        if let Err(e) = std::fs::rename(&staged, &target) {
            if let Some(backup) = backup {
                let _ = std::fs::rename(backup, &target);
            }
            return Err(failed(e));
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staged);
    result
}

/// Finds the backend and, on GNOME, the extension's setup state.
pub fn detect() -> Result<Setup, String> {
    if hyprland_session() {
        hyprland::check()?;
        return Ok(Setup { backend: Backend::Hyprland, extension: None });
    }
    if let Some(socket) = niri_socket() {
        niri::check(&socket)?;
        return Ok(Setup { backend: Backend::Niri, extension: None });
    }
    let gnome = gnome_session(env("XDG_CURRENT_DESKTOP").as_deref(), sandboxed());
    let found = bus::probe(gnome, kde_session())?;
    let extension = gnome.then(|| match &found.shell {
        Ok(shell) => extension_status(shell, installed_version(), &bundled()),
        Err(message) => (ExtensionStatus::Error, message.clone()),
    });
    Ok(Setup { backend: backend_from(found.kwin, found.portal_version, found.targets, gnome, found.extension_api), extension })
}

/// Runs a setup action, checked against the current state as `GnomeCaptureSetup.perform` does.
pub fn act(action: Action) -> Result<(), String> {
    let setup = detect()?;
    let (status, message) = setup.extension.ok_or("GNOME extension setup is only available on GNOME.")?;
    match action {
        Action::Install if matches!(status, ExtensionStatus::NotInstalled | ExtensionStatus::UpdateRequired) => install(),
        Action::Enable if status == ExtensionStatus::Disabled => bus::set_enabled(true),
        Action::Disable if status == ExtensionStatus::Enabled => bus::set_enabled(false),
        _ => Err(message),
    }
}

/// Captures with `backend` into `path`.
pub fn capture(backend: Backend, path: &Path, feedback: Feedback) -> Result<Captured, String> {
    let mut captured = Captured { window: None, own: None, effects_done: false, notice: None };
    let png = match backend {
        Backend::Kde => {
            ensure_kde_entry()?;
            match bus::kde_capture() {
                Ok((meta, raw, lookup)) => {
                    let (width, height, rgba) = kde_rgba(&meta, &raw)?;
                    let image = image::RgbaImage::from_raw(width, height, rgba).ok_or("KDE Plasma returned an unreadable screenshot.")?;
                    save_image(image::DynamicImage::ImageRgba8(image), path)?;
                    if let Some(lookup) = lookup {
                        captured.own = (feedback.flash || feedback.animate).then_some(lookup.own).flatten();
                        captured.window = Some(lookup.window);
                    }
                    return Ok(captured);
                }
                // KDE may not have read the entry yet: this capture takes the picker, and the
                // next one asks KWin again rather than registering again.
                Err(KdeError::Denied) if KDE_REGISTERED.load(Ordering::Relaxed) => {
                    captured.notice = Some(KDE_PENDING.into());
                    bus::screenshot(true)?
                }
                Err(KdeError::Denied) => return Err(KDE_DENIED.into()),
                Err(KdeError::Failed(message)) => return Err(message),
            }
        }
        Backend::ScreenshotPortal => bus::screenshot(false)?,
        Backend::Picker => bus::screenshot(true)?,
        Backend::GnomeExtension => {
            let (png, window, effects) = bus::extension_capture(feedback)?;
            captured.window = parse_window(&window);
            captured.effects_done = effects;
            png
        }
        Backend::Hyprland => {
            let (png, active) = hyprland::capture()?;
            let origin = (feedback.flash || feedback.animate).then(hyprland_origin).flatten();
            captured.window = hyprland_window(&active, origin);
            captured.own = origin.and_then(|o| hyprland_window(&hyprland::own()?, Some(o))?.bounds);
            png
        }
        Backend::Niri => {
            let socket = niri_socket().ok_or("Niri is not available in this session.")?;
            let (png, window) = niri::capture(&socket)?;
            captured.window = window;
            png
        }
    };
    store_png(&png, path)?;
    Ok(captured)
}

/// The captured window's text through AT-SPI, as Electron reads it on Linux.
pub fn app_text(window: &Window) -> Option<String> {
    (window.pid > 0).then(|| bus::app_text(window.pid, &window.title)).flatten()
}

/// `startNiriCaptureShortcut` and Hyprland's `PortalCaptureShortcut`: serves the action the
/// compositor's binding calls until `stop`. `ready` says once whether it was registered (Hyprland
/// may refuse the action) or why it failed; `trigger` runs on each press.
pub fn serve_shortcut(desktop: crate::desktop::capture_config::Desktop, stop: &std::sync::atomic::AtomicBool, ready: &dyn Fn(Result<bool, String>), trigger: std::sync::Arc<dyn Fn() + Send + Sync>) {
    match desktop {
        crate::desktop::capture_config::Desktop::Niri => bus::niri_endpoint(stop, ready, trigger),
        crate::desktop::capture_config::Desktop::Hyprland => bus::hyprland_shortcut(stop, ready, &*trigger, sandboxed()),
    }
}

/// Plays a capture sound through PipeWire, else PulseAudio.
pub fn play(path: &Path) {
    for player in ["pw-play", "paplay"] {
        let spawned = std::process::Command::new(player).arg(path).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
        if let Ok(mut child) = spawned {
            std::thread::spawn(move || child.wait());
            return;
        }
    }
}

/// What the probe read off the session bus.
pub struct Probe {
    /// ScreenShot2's version, probed on KDE sessions only.
    kwin: Option<u32>,
    portal_version: Option<u32>,
    targets: Option<u32>,
    extension_api: Option<u32>,
    shell: Result<Shell, String>,
}


#[cfg(target_os = "linux")]
mod bus {
    use super::{APP_ID, EXTENSION_UUID, KdeError, KwinLookup, Probe, RawMeta, Shell};
    use futures_util::StreamExt;
    use std::collections::HashMap;
    use std::future::Future;
    use std::time::Duration;
    use zbus::{Connection, OwnedObjectPath, OwnedVariant, Proxy, Variant, fdo};

    const PORTAL: &str = "org.freedesktop.portal.Desktop";
    const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
    const SCREENSHOT: &str = "org.freedesktop.portal.Screenshot";
    const EXTENSION: &str = "org.gnome.Shell.Extensions.T3SnapShot";
    const EXTENSION_PATH: &str = "/org/gnome/Shell/Extensions/T3SnapShot";
    const SHELL: &str = "org.gnome.Shell";
    const SHELL_PATH: &str = "/org/gnome/Shell";
    const EXTENSIONS: &str = "org.gnome.Shell.Extensions";
    const DBUS: &str = "org.freedesktop.DBus";
    const DBUS_PATH: &str = "/org/freedesktop/DBus";
    /// KWin registers this name as well as `org.kde.KWin`; Electron and its helper call it.
    const KWIN_SCREENSHOT: &str = "org.kde.KWin.ScreenShot2";
    const KWIN_SCREENSHOT_PATH: &str = "/org/kde/KWin/ScreenShot2";
    const KWIN_NOT_AUTHORIZED: &str = "org.kde.KWin.ScreenShot2.Error.NoAuthorized";
    const CALL_TIMEOUT: Duration = Duration::from_secs(5);
    /// The portal may show a dialog; Electron waits two minutes for it.
    const RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);

    /// One runtime and connection per operation, as Electron opens one bus per operation.
    fn block<T, F: Future<Output = Result<T, String>>>(work: impl FnOnce(Connection) -> F) -> Result<T, String> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        runtime.block_on(async {
            let conn = tokio::time::timeout(CALL_TIMEOUT, Connection::session())
                .await
                .map_err(|_| "Snapshot timed out.".to_owned())?
                .map_err(|_| "Could not connect to your desktop session.".to_owned())?;
            let result = work(conn.clone()).await;
            let _ = conn.close().await;
            result
        })
    }

    /// `unavailable`: the service or member doesn't exist here.
    fn missing(e: &zbus::Error) -> bool {
        matches!(e, zbus::Error::FDO(fdo) if matches!(**fdo, fdo::Error::ServiceUnknown(_) | fdo::Error::NameHasNoOwner(_) | fdo::Error::UnknownObject(_) | fdo::Error::UnknownInterface(_) | fdo::Error::UnknownMethod(_)))
    }

    async fn timed<T>(work: impl Future<Output = zbus::Result<T>>) -> Result<T, zbus::Error> {
        tokio::time::timeout(CALL_TIMEOUT, work).await.unwrap_or_else(|_| Err(zbus::Error::Failure("Snapshot timed out.".into())))
    }

    async fn all(conn: &Connection, destination: &str, path: &str, interface: &str) -> Result<HashMap<String, OwnedVariant>, zbus::Error> {
        timed(async {
            let proxy = Proxy::new(conn, destination, path, "org.freedesktop.DBus.Properties").await?;
            proxy.call::<_, _, HashMap<String, OwnedVariant>>("GetAll", &(interface,)).await
        })
        .await
    }

    fn number(values: &HashMap<String, OwnedVariant>, key: &str) -> Option<f64> {
        let v = values.get(key)?.try_clone().ok()?;
        f64::try_from(v.try_clone().ok()?).ok().or_else(|| u32::try_from(v).ok().map(f64::from))
    }

    fn text(values: &HashMap<String, OwnedVariant>, key: &str) -> Option<String> {
        values.get(key).and_then(|v| <&str>::try_from(v).ok()).map(str::to_owned)
    }

    fn flag(values: &HashMap<String, OwnedVariant>, key: &str) -> Option<bool> {
        bool::try_from(values.get(key)?.try_clone().ok()?).ok()
    }

    fn message(e: zbus::Error) -> String {
        match e {
            zbus::Error::MethodError(_, Some(detail), _) => detail,
            zbus::Error::Failure(detail) => detail,
            e => e.to_string(),
        }
    }

    pub(super) fn probe(gnome: bool, kde: bool) -> Result<Probe, String> {
        block(|conn| async move {
            let conn = &conn;
            let mut found = Probe { kwin: None, portal_version: None, targets: None, extension_api: None, shell: Err(String::new()) };
            // Any failure here leaves KDE on the portal rather than failing detection.
            if kde && let Ok(props) = all(conn, KWIN_SCREENSHOT, KWIN_SCREENSHOT_PATH, KWIN_SCREENSHOT).await {
                found.kwin = number(&props, "Version").map(|v| v as u32);
            }
            // Unsandboxed apps name themselves to the portal so its permissions stick.
            if let Ok(registry) = Proxy::new(conn, PORTAL, PORTAL_PATH, "org.freedesktop.host.portal.Registry").await {
                let _ = timed(registry.call::<_, _, ()>("Register", &(APP_ID, HashMap::<&str, Variant>::new()))).await;
            }
            match all(conn, PORTAL, PORTAL_PATH, SCREENSHOT).await {
                Ok(props) => {
                    found.portal_version = number(&props, "version").map(|v| v as u32);
                    found.targets = number(&props, "AvailableTargets").map(|v| v as u32);
                }
                Err(e) if missing(&e) => {}
                Err(e) => return Err(message(e)),
            }
            if !gnome {
                return Ok(found);
            }
            match all(conn, EXTENSION, EXTENSION_PATH, EXTENSION).await {
                Ok(props) => found.extension_api = number(&props, "Version").map(|v| v as u32),
                Err(e) if missing(&e) => {}
                Err(e) => return Err(message(e)),
            }
            found.shell = shell(conn).await;
            Ok(found)
        })
    }

    async fn shell(conn: &Connection) -> Result<Shell, String> {
        let unreachable = |_| "GNOME did not respond. Sign in to a GNOME Wayland session and try again.".to_owned();
        let props = all(conn, SHELL, SHELL_PATH, EXTENSIONS).await.map_err(unreachable)?;
        let info: HashMap<String, OwnedVariant> = timed(async {
            let proxy = Proxy::new(conn, SHELL, SHELL_PATH, EXTENSIONS).await?;
            proxy.call("GetExtensionInfo", &(EXTENSION_UUID,)).await
        })
        .await
        .map_err(unreachable)?;
        Ok(Shell {
            version: text(&props, "ShellVersion").ok_or("GNOME returned no setup information.")?,
            user_extensions: flag(&props, "UserExtensionsEnabled").unwrap_or(false),
            state: number(&info, "state"),
            loaded_version: number(&info, "version"),
            error: text(&info, "error"),
        })
    }

    pub(super) fn set_enabled(on: bool) -> Result<(), String> {
        block(|conn| async move {
            let conn = &conn;
            let method = if on { "EnableExtension" } else { "DisableExtension" };
            let done: bool = timed(async {
                let proxy = Proxy::new(conn, SHELL, SHELL_PATH, EXTENSIONS).await?;
                proxy.call(method, &(EXTENSION_UUID,)).await
            })
            .await
            .map_err(message)?;
            if done { Ok(()) } else { Err("GNOME did not change the extension. Check GNOME Extensions.".into()) }
        })
    }

    /// `capturePortal`: one Screenshot request, its Response, and the PNG it names.
    pub(super) fn screenshot(interactive: bool) -> Result<Vec<u8>, String> {
        block(|conn| async move {
            let conn = &conn;
            let sender = conn.unique_name().ok_or("Could not connect to your desktop session.")?.trim_start_matches(':').replace('.', "_");
            let token = format!("t3_{}", s5_net::uuid().replace('-', ""));
            let path = format!("{PORTAL_PATH}/request/{sender}/{token}");
            let request = Proxy::new(conn, PORTAL, path.as_str(), "org.freedesktop.portal.Request").await.map_err(message)?;
            // Subscribe before the call: the response can arrive before its method reply.
            let mut responses = request.receive_signal("Response").await.map_err(message)?;
            let mut options = HashMap::from([("handle_token", Variant::from(token.as_str())), ("interactive", Variant::from(interactive)), ("modal", Variant::from(false))]);
            if !interactive {
                options.insert("target", Variant::from(8u32));
            }
            let outcome = tokio::time::timeout(RESPONSE_TIMEOUT, async {
                let portal = Proxy::new(conn, PORTAL, PORTAL_PATH, SCREENSHOT).await.map_err(message)?;
                let handle: OwnedObjectPath = portal.call("Screenshot", &("", options)).await.map_err(message)?;
                if handle.as_str() != path {
                    return Err("Invalid screenshot request handle.".to_owned());
                }
                let response = responses.next().await.ok_or("Your desktop closed the snapshot request.")?;
                let (status, results): (u32, HashMap<String, OwnedVariant>) = response.body().deserialize().map_err(message)?;
                match status {
                    0 => text(&results, "uri").ok_or_else(|| "Your desktop returned no snapshot.".to_owned()),
                    1 => Err("Snapshot was cancelled.".to_owned()),
                    _ => Err("Your desktop did not allow the snapshot.".to_owned()),
                }
            })
            .await
            .unwrap_or_else(|_| Err("Snapshot timed out.".to_owned()));
            if outcome.is_err() {
                let _ = timed(request.call::<_, _, ()>("Close", &())).await;
            }
            let file = super::uri_path(&outcome?).ok_or("Invalid screenshot file.")?;
            // The portal owns the file: read it, never delete it.
            let meta = std::fs::metadata(&file).map_err(|_| "Invalid screenshot file.".to_owned())?;
            if !meta.is_file() || meta.len() as usize > super::MAX_PNG_BYTES {
                return Err("Invalid screenshot file.".into());
            }
            std::fs::read(&file).map_err(|_| "Incomplete screenshot file.".to_owned())
        })
    }

    /// `captureExtension`: own the caller name the extension trusts, then ask for the window.
    /// With effects on, the extension flashes and, once we name our window, flies the picture
    /// to `relative` on the same connection; closing it clears the landed picture.
    pub(super) fn extension_capture(feedback: super::Feedback) -> Result<(Vec<u8>, String, bool), String> {
        block(|conn| async move {
            let conn = &conn;
            let name = format!("{APP_ID}.SnapShot");
            // DO_NOT_QUEUE; 1 is PRIMARY_OWNER.
            let owned: u32 = timed(async {
                let proxy = Proxy::new(conn, DBUS, DBUS_PATH, DBUS).await?;
                proxy.call("RequestName", &(name.as_str(), 4u32)).await
            })
            .await
            .map_err(message)?;
            if owned != 1 {
                return Err("Another T3 Code instance is capturing a window. Try again.".into());
            }
            let proxy = Proxy::new(conn, EXTENSION, EXTENSION_PATH, EXTENSION).await.map_err(message)?;
            if feedback.flash || feedback.animate {
                match timed(proxy.call::<_, _, (Vec<u8>, String, bool)>("CaptureWithFeedback", &(feedback.flash, feedback.animate))).await {
                    Ok((png, window, started)) => {
                        if let Some([x, y, w, h]) = feedback.relative.filter(|_| started) {
                            // Our only window is found by pid; the title only breaks ties.
                            let _ = timed(proxy.call::<_, _, ()>("Activate", &("",))).await;
                            let _ = tokio::time::timeout(Duration::from_secs(3), proxy.call::<_, _, ()>("Animate", &(x, y, w, h))).await;
                        }
                        return Ok((png, window, true));
                    }
                    // Version 1 of the extension has no feedback.
                    Err(e) if missing(&e) => {}
                    Err(e) => return Err(message(e)),
                }
            }
            let (png, window) = timed(proxy.call::<_, _, (Vec<u8>, String)>("Capture", &())).await.map_err(message)?;
            Ok((png, window, false))
        })
    }

    const KWIN: &str = "org.kde.KWin";
    const REPLY_PATH: &str = "/com/s5code/KdeCapture";
    const REPLY_INTERFACE: &str = "com.s5code.KdeCapture";

    /// The KDE helper's `script`: loads a one-off KWin script whose `reply` calls back on this
    /// connection, and unloads it however it ends. With no object server here, the call only
    /// reaches the message stream, which takes it from KWin alone.
    async fn kwin_script(conn: &Connection, body: &str) -> Option<String> {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let owner: String = timed(call(conn, (DBUS, DBUS_PATH), DBUS, "GetNameOwner", &(KWIN,))).await.ok()?;
        let me = conn.unique_name()?.to_string();
        let name = format!("s5-capture-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
        let path = std::env::temp_dir().join(format!("{name}.js"));
        let source = format!(
            "function reply(value) {{ callDBus({}, '{REPLY_PATH}', '{REPLY_INTERFACE}', 'Reply', JSON.stringify(value)); }}\ntry {{ {body} }} catch (error) {{ reply({{error: String(error)}}); }}",
            serde_json::Value::from(me)
        );
        {
            use std::io::Write;
            std::fs::OpenOptions::new().write(true).create_new(true).open(&path).ok()?.write_all(source.as_bytes()).ok()?;
        }
        let mut stream = zbus::MessageStream::from(conn);
        let scripting = (KWIN, "/Scripting");
        let result = async {
            let id: i32 = timed(call(conn, scripting, "org.kde.kwin.Scripting", "loadScript", &(path.to_str()?, name.as_str()))).await.ok()?;
            if id < 0 {
                return None;
            }
            let object = format!("/Scripting/Script{id}");
            timed(call::<_, ()>(conn, (KWIN, object.as_str()), "org.kde.kwin.Script", "run", &())).await.ok()?;
            let reply = async {
                while let Some(Ok(message)) = stream.next().await {
                    let header = message.header();
                    let from_kwin = header.sender().is_some_and(|s| s.as_str() == owner);
                    let ours = header.member().is_some_and(|m| m.as_str() == "Reply") && header.interface().is_some_and(|i| i.as_str() == REPLY_INTERFACE);
                    if matches!(message.message_type(), zbus::message::Type::MethodCall) && from_kwin && ours {
                        return message.body().deserialize::<String>().ok().filter(|v| v.len() <= 64 * 1024);
                    }
                }
                None
            };
            tokio::time::timeout(CALL_TIMEOUT, reply).await.ok().flatten()
        }
        .await;
        let _ = timed(call::<_, bool>(conn, scripting, "org.kde.kwin.Scripting", "unloadScript", &(name.as_str(),))).await;
        let _ = std::fs::remove_file(&path);
        result
    }

    async fn kwin_window(conn: &Connection, id: Option<&str>) -> Option<KwinLookup> {
        super::parse_kwin(&kwin_script(conn, &super::kwin_lookup_script(id, std::process::id())).await?)
    }

    /// The KDE helper's `capture`: the active window by its KWin id, writing raw pixels to a
    /// socket drained on a thread, so KWin never blocks on a full buffer. Without scripting it
    /// falls back to `CaptureActiveWindow` and names no window; a window that changed during
    /// the capture is not named either, as the helper never guesses which app was captured.
    pub(super) fn kde_capture() -> Result<(RawMeta, Vec<u8>, Option<KwinLookup>), KdeError> {
        use std::io::Read;
        use std::os::unix::net::UnixStream;
        let unprepared = |_: std::io::Error| KdeError::Failed("Could not prepare the snapshot.".into());
        let (reader, writer) = UnixStream::pair().map_err(unprepared)?;
        reader.set_read_timeout(Some(CALL_TIMEOUT)).map_err(unprepared)?;
        let stop = reader.try_clone().map_err(unprepared)?;
        let drain = std::thread::spawn(move || {
            let mut raw = Vec::new();
            // A timeout keeps what arrived; `kde_rgba` checks it against the reply's size.
            let _ = reader.take(super::MAX_RAW_BYTES as u64).read_to_end(&mut raw);
            raw
        });
        let reply = block(|conn| async move {
            let conn = &conn;
            let options = HashMap::from([
                ("include-decoration", Variant::from(true)),
                ("include-shadow", Variant::from(false)),
                ("include-cursor", Variant::from(false)),
                ("native-resolution", Variant::from(true)),
            ]);
            let before = kwin_window(conn, None).await;
            let result = timed(async {
                let proxy = Proxy::new(conn, KWIN_SCREENSHOT, KWIN_SCREENSHOT_PATH, KWIN_SCREENSHOT).await?;
                match &before {
                    Some(window) => proxy.call::<_, _, HashMap<String, OwnedVariant>>("CaptureWindow", &(window.id.as_str(), options, zbus::Fd::from(&writer))).await,
                    None => proxy.call::<_, _, HashMap<String, OwnedVariant>>("CaptureActiveWindow", &(options, zbus::Fd::from(&writer))).await,
                }
            })
            .await;
            // Only KWin's copy may keep the socket open, so the drain ends when KWin is done.
            drop(writer);
            let named = match (&result, before) {
                (Ok(_), Some(before)) => kwin_window(conn, Some(&before.id)).await.filter(|after| before.same_window(after)).map(|_| before),
                _ => None,
            };
            Ok::<_, String>(result.map(|r| (r, named)))
        });
        let outcome = match reply {
            Ok(Ok((results, named))) => Ok((results, named)),
            Ok(Err(zbus::Error::MethodError(name, ..))) if name.as_str() == KWIN_NOT_AUTHORIZED => Err(KdeError::Denied),
            Ok(Err(e)) => Err(KdeError::Failed(message(e))),
            Err(e) => Err(KdeError::Failed(e)),
        };
        if outcome.is_err() {
            // Nothing will arrive; wake the drain rather than wait out its timeout.
            let _ = stop.shutdown(std::net::Shutdown::Read);
        }
        let raw = drain.join().map_err(|_| KdeError::Failed("Could not read the snapshot.".into()))?;
        let (results, named) = outcome?;
        let uint = |key: &str| number(&results, key).map(|v| v as u32);
        let meta = RawMeta { kind: text(&results, "type"), width: uint("width"), height: uint("height"), stride: uint("stride"), format: uint("format") };
        Ok((meta, raw, named))
    }

    /// The object Niri's `gdbus call` reaches.
    struct NiriEndpoint(std::sync::Arc<dyn Fn() + Send + Sync>);

    #[zbus::interface(name = "com.t3tools.SnapShot")]
    impl NiriEndpoint {
        fn capture(&self) {
            (self.0)();
        }
    }

    async fn wait_for(stop: &std::sync::atomic::AtomicBool) {
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    pub(super) fn niri_endpoint(stop: &std::sync::atomic::AtomicBool, ready: &dyn Fn(Result<bool, String>), trigger: std::sync::Arc<dyn Fn() + Send + Sync>) {
        let result = block(|conn| async move {
            conn.object_server().at(crate::desktop::capture_config::NIRI_PATH, NiriEndpoint(trigger)).await.map_err(message)?;
            // DO_NOT_QUEUE (4); 1 is PRIMARY_OWNER.
            let owner: u32 = timed(call(&conn, (DBUS, DBUS_PATH), DBUS, "RequestName", &(format!("{APP_ID}.SnapShot"), 4u32))).await.map_err(message)?;
            if owner != 1 {
                return Err("Another T3 Code instance already owns the capture shortcut.".to_owned());
            }
            ready(Ok(true));
            wait_for(stop).await;
            Ok(())
        });
        if let Err(e) = result {
            ready(Err(e));
        }
    }

    const SHORTCUTS: &str = "org.freedesktop.portal.GlobalShortcuts";
    const NO_SESSION: &str = "Your desktop could not create a capture shortcut session.";

    /// A portal request: subscribe to its `Response`, then make the (lazy) call, and take the results.
    async fn portal_response(conn: &Connection, sender: &str, token: &str, request: impl Future<Output = zbus::Result<OwnedObjectPath>>) -> Result<HashMap<String, OwnedVariant>, String> {
        let path = format!("{PORTAL_PATH}/request/{sender}/{token}");
        let proxy = Proxy::new(conn, PORTAL, path.as_str(), "org.freedesktop.portal.Request").await.map_err(message)?;
        let mut responses = proxy.receive_signal("Response").await.map_err(message)?;
        let handle = timed(request).await.map_err(message)?;
        if handle.as_str() != path {
            return Err(NO_SESSION.into());
        }
        let response = tokio::time::timeout(RESPONSE_TIMEOUT, responses.next()).await.ok().flatten().ok_or(NO_SESSION)?;
        let (status, results): (u32, HashMap<String, OwnedVariant>) = response.body().deserialize().map_err(message)?;
        if status == 0 { Ok(results) } else { Err(NO_SESSION.into()) }
    }

    /// Electron's `PortalCaptureShortcut` managed by Hyprland: the app names itself to the portal,
    /// binds `capture-window` with no trigger, and Hyprland's `global` binding fires it.
    pub(super) fn hyprland_shortcut(stop: &std::sync::atomic::AtomicBool, ready: &dyn Fn(Result<bool, String>), trigger: &dyn Fn(), sandboxed: bool) {
        use crate::desktop::capture_config::HYPRLAND_ACTION;
        let result = block(|conn| async move {
            let conn = &conn;
            if !sandboxed {
                let registered = timed(call::<_, ()>(conn, (PORTAL, PORTAL_PATH), "org.freedesktop.host.portal.Registry", "Register", &(APP_ID, HashMap::<&str, Variant>::new()))).await;
                if let Err(e) = registered
                    && !missing(&e)
                {
                    return Err(message(e));
                }
            }
            let sender = conn.unique_name().ok_or(NO_SESSION)?.trim_start_matches(':').replace('.', "_");
            let portal = Proxy::new(conn, PORTAL, PORTAL_PATH, SHORTCUTS).await.map_err(message)?;
            let mut activated = portal.receive_signal("Activated").await.map_err(message)?;
            let id = s5_net::uuid().replace('-', "");
            let (token, session_token) = (format!("t3_capture_{id}"), format!("t3_capture_session_{id}"));
            let options = HashMap::from([("handle_token", Variant::from(token.as_str())), ("session_handle_token", Variant::from(session_token.as_str()))]);
            let body = (options,);
            let created = portal_response(conn, &sender, &token, portal.call::<_, _, OwnedObjectPath>("CreateSession", &body)).await?;
            let session = text(&created, "session_handle").unwrap_or_else(|| format!("{PORTAL_PATH}/session/{sender}/{session_token}"));
            if !session.starts_with(&format!("{PORTAL_PATH}/session/{sender}/")) {
                return Err("Invalid shortcut session handle.".into());
            }
            let handle = OwnedObjectPath::try_from(session.clone()).map_err(|_| "Invalid shortcut session handle.".to_owned())?;
            let bind_token = format!("t3_capture_bind_{id}");
            let shortcuts = vec![(HYPRLAND_ACTION, HashMap::from([("description", Variant::from("Capture a window"))]))];
            let options = HashMap::from([("handle_token", Variant::from(bind_token.as_str()))]);
            let body = (&handle, shortcuts, "", options);
            let bound = portal_response(conn, &sender, &bind_token, portal.call::<_, _, OwnedObjectPath>("BindShortcuts", &body)).await?;
            ready(Ok(bound.get("shortcuts").is_some_and(|v| format!("{v:?}").contains(HYPRLAND_ACTION))));
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok(next) = tokio::time::timeout(Duration::from_millis(250), activated.next()).await else { continue };
                let Some(signal) = next else { break };
                if let Ok((from, shortcut, _, _)) = signal.body().deserialize::<(OwnedObjectPath, String, u64, HashMap<String, OwnedVariant>)>()
                    && from.as_str() == session
                    && shortcut == HYPRLAND_ACTION
                {
                    trigger();
                }
            }
            let _ = timed(call::<_, ()>(conn, (PORTAL, session.as_str()), "org.freedesktop.portal.Session", "Close", &())).await;
            Ok(())
        });
        if let Err(e) = result {
            ready(Err(e));
        }
    }

    const ATSPI_ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
    const ATSPI_TEXT: &str = "org.a11y.atspi.Text";
    /// `STATE_ACTIVE` in the first word of `GetState`.
    const STATE_ACTIVE: u32 = 1 << 1;

    async fn call<B, R>(conn: &Connection, (dest, path): (&str, &str), interface: &str, method: &str, body: &B) -> zbus::Result<R>
    where
        B: zbus::export::serde::Serialize + zbus::zvariant::DynamicType,
        R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
    {
        let reply = conn.call_method(Some(dest), path, Some(interface), method, body).await?;
        reply.body().deserialize::<R>()
    }

    async fn property(conn: &Connection, node: (&str, &str), name: &str) -> Option<String> {
        let v: OwnedVariant = call(conn, node, "org.freedesktop.DBus.Properties", "Get", &(ATSPI_ACCESSIBLE, name)).await.ok()?;
        <&str>::try_from(&v).ok().map(str::to_owned)
    }

    async fn children(conn: &Connection, node: (&str, &str)) -> Vec<(String, OwnedObjectPath)> {
        call(conn, node, ATSPI_ACCESSIBLE, "GetChildren", &()).await.unwrap_or_default()
    }

    /// `SnapShotAccessibility`'s flat text over AT-SPI: the app with the window's pid, its window
    /// by title (else the active one), then name, description and text, depth first.
    pub(super) fn app_text(pid: u32, title: &str) -> Option<String> {
        use crate::desktop::capture::{ACCESSIBILITY_MAX_NODES, ACCESSIBLE_TEXT_MAX_CHARS, TEXT_TIMEOUT, accessible_text};
        let title = title.to_owned();
        block(|session| async move {
            let address: String = call(&session, ("org.a11y.Bus", "/org/a11y/bus"), "org.a11y.Bus", "GetAddress", &()).await.map_err(message)?;
            let a11y = zbus::connection::Builder::address(address.as_str()).build().await.map_err(message)?;
            let mut values = Vec::new();
            let walk = async {
                let root = children(&a11y, ("org.a11y.atspi.Registry", "/org/a11y/atspi/accessible/root")).await;
                let mut app = None;
                for (name, path) in root {
                    let owner: zbus::Result<u32> = call(&a11y, (DBUS, DBUS_PATH), DBUS, "GetConnectionUnixProcessID", &(name.as_str(),)).await;
                    if owner.ok() == Some(pid) {
                        app = Some((name, path));
                        break;
                    }
                }
                let (bus, app_path) = app?;
                let windows = children(&a11y, (bus.as_str(), app_path.as_str())).await;
                let mut chosen = None;
                for (_, path) in &windows {
                    let node = (bus.as_str(), path.as_str());
                    if !title.is_empty() && property(&a11y, node, "Name").await.as_deref() == Some(title.as_str()) {
                        chosen = Some(path.clone());
                        break;
                    }
                    let state: Vec<u32> = call(&a11y, node, ATSPI_ACCESSIBLE, "GetState", &()).await.unwrap_or_default();
                    if chosen.is_none() && state.first().is_some_and(|s| s & STATE_ACTIVE != 0) {
                        chosen = Some(path.clone());
                    }
                }
                let mut stack = vec![chosen.or_else(|| windows.first().map(|(_, p)| p.clone()))?];
                let mut visited: u32 = 0;
                while let Some(path) = stack.pop() {
                    visited += 1;
                    if visited > ACCESSIBILITY_MAX_NODES {
                        break;
                    }
                    let node = (bus.as_str(), path.as_str());
                    for name in ["Name", "Description"] {
                        if let Some(value) = property(&a11y, node, name).await {
                            values.push(value);
                        }
                    }
                    let interfaces: Vec<String> = call(&a11y, node, ATSPI_ACCESSIBLE, "GetInterfaces", &()).await.unwrap_or_default();
                    if interfaces.iter().any(|i| i == ATSPI_TEXT)
                        && let Ok(text) = call::<_, String>(&a11y, node, ATSPI_TEXT, "GetText", &(0i32, -1i32)).await
                    {
                        values.push(text);
                    }
                    let mut kids = children(&a11y, node).await;
                    kids.reverse();
                    stack.extend(kids.into_iter().map(|(_, p)| p));
                }
                Some(())
            };
            // Whatever was read before the deadline still counts, as Electron keeps partial text.
            let _ = tokio::time::timeout(TEXT_TIMEOUT, walk).await;
            let _ = a11y.close().await;
            Ok(accessible_text(&values, ACCESSIBLE_TEXT_MAX_CHARS))
        })
        .ok()
        .filter(|text| !text.is_empty())
    }
}

#[cfg(not(target_os = "linux"))]
mod bus {
    use super::Probe;
    const UNSUPPORTED: &str = "Snapshots on Linux need a Wayland session.";
    pub(super) fn niri_endpoint(_: &std::sync::atomic::AtomicBool, ready: &dyn Fn(Result<bool, String>), _: std::sync::Arc<dyn Fn() + Send + Sync>) {
        ready(Err(UNSUPPORTED.into()));
    }
    pub(super) fn hyprland_shortcut(_: &std::sync::atomic::AtomicBool, ready: &dyn Fn(Result<bool, String>), _: &dyn Fn(), _: bool) {
        ready(Err(UNSUPPORTED.into()));
    }
    pub(super) fn probe(_: bool, _: bool) -> Result<Probe, String> {
        Err(UNSUPPORTED.into())
    }
    pub(super) fn kde_capture() -> Result<(super::RawMeta, Vec<u8>, Option<super::KwinLookup>), super::KdeError> {
        Err(super::KdeError::Failed(UNSUPPORTED.into()))
    }
    pub(super) fn set_enabled(_: bool) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    pub(super) fn screenshot(_: bool) -> Result<Vec<u8>, String> {
        Err(UNSUPPORTED.into())
    }
    pub(super) fn extension_capture(_: super::Feedback) -> Result<(Vec<u8>, String, bool), String> {
        Err(UNSUPPORTED.into())
    }
    pub(super) fn app_text(_: u32, _: &str) -> Option<String> {
        None
    }
}

/// A setup the scenes force without a session bus, its message read as the real one is.
pub fn scene_setup(backend: Backend, extension: Option<ExtensionStatus>) -> Setup {
    let bundled = bundled();
    let extension = extension.map(|status| {
        use ExtensionStatus as S;
        let version = bundled.0;
        // (shell major, state, installed version, loaded version, user extensions)
        let (major, state, installed, loaded, user_extensions) = match status {
            S::Unsupported => ("1".to_owned(), None, None, None, true),
            S::NotInstalled => (String::new(), None, None, None, true),
            S::RestartRequired => (String::new(), None, Some(version), None, true),
            S::UpdateRequired => (String::new(), Some(2.0), Some(version.saturating_sub(1)), Some(version.saturating_sub(1)), true),
            S::ExtensionsDisabled => (String::new(), Some(2.0), Some(version), Some(version), false),
            S::Disabled => (String::new(), Some(2.0), Some(version), Some(version), true),
            S::Enabled => (String::new(), Some(1.0), Some(version), Some(version), true),
            S::Error => (String::new(), Some(3.0), Some(version), Some(version), true),
        };
        let major = if major.is_empty() { bundled.1.last().cloned().unwrap_or_default() } else { major };
        let shell = Shell { version: format!("{major}.1"), user_extensions, state, loaded_version: loaded.map(f64::from), error: None };
        extension_status(&shell, installed, &bundled)
    });
    Setup { backend, extension }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn picks_the_backend_like_electron() {
        assert_eq!(backend_from(None, Some(3), Some(8 | 1), false, None), Backend::ScreenshotPortal);
        assert_eq!(backend_from(None, Some(2), Some(8), true, Some(2)), Backend::GnomeExtension, "an older portal has no active-window target");
        assert_eq!(backend_from(None, Some(3), Some(1), true, Some(1)), Backend::GnomeExtension);
        assert_eq!(backend_from(None, Some(3), Some(1), true, Some(3)), Backend::Picker, "an unknown extension API");
        assert_eq!(backend_from(None, None, None, false, Some(2)), Backend::Picker, "the extension only counts on GNOME");
        assert_eq!(backend_from(Some(2), Some(3), Some(8), false, None), Backend::Kde, "KWin comes before the portal");
        assert_eq!(backend_from(Some(1), Some(3), Some(8), false, None), Backend::ScreenshotPortal, "ScreenShot2 version 1 is too old");
        assert!(desktop_session(Some("KDE"), false, "kde") && !desktop_session(Some("KDE"), true, "kde"));
        assert!(Backend::Kde.knows_window(), "the KWin script names the window, so its text is read");
        assert!(gnome_session(Some("ubuntu:GNOME"), false));
        assert!(!gnome_session(Some("GNOME"), true), "sandboxed apps can't use the extension");
        assert!(!gnome_session(Some("KDE"), false));
        assert!(!gnome_session(None, false));
    }

    #[test]
    fn registers_with_kwin_like_the_kde_helper() {
        let entry = kde_desktop_entry("/opt/T3 Code/rune");
        let lines: Vec<&str> = entry.lines().collect();
        assert!(lines.contains(&"Exec=\"/opt/T3 Code/rune\""), "{entry}");
        assert!(lines.contains(&"X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2"), "a KConfig list, no trailing ';'");
        assert!(lines.contains(&"NoDisplay=true") && entry.ends_with('\n'));
        assert_eq!(desktop_exec_argument("/x/a\"b"), r#""/x/a\\"b""#, "quoted, then string-escaped");
        assert_eq!(desktop_exec_argument("/x/50%$"), r#""/x/50%%\\$""#, "field codes and shell characters");
        assert_eq!(kde_entry_stale(None, &entry), Ok(true));
        assert_eq!(kde_entry_stale(Some(&entry), &entry), Ok(false));
        let moved = kde_desktop_entry("/usr/bin/rune");
        assert_eq!(kde_entry_stale(Some(&moved), &entry), Ok(true), "another Exec is rewritten");
        assert!(kde_entry_stale(Some("[Desktop Entry]\nName=Other\n"), &entry).is_err(), "never replaces someone else's entry");
    }

    #[test]
    fn names_the_kwin_window_like_the_kde_helper() {
        let reply = |screens: u32, title: &str, x: i32| {
            json!({
                "window": {"id": "{abc}", "title": title, "appName": "firefox", "appIdentifier": "org.mozilla.firefox", "processId": 42,
                    "bounds": {"x": x, "y": 120, "width": 800, "height": 600}, "output": {"x": 1920, "y": 0, "width": 1920, "height": 1080}},
                "own": {"bounds": {"x": 2000, "y": 50, "width": 1200, "height": 800}, "output": {"x": 1920, "y": 0, "width": 1920, "height": 1080}},
                "screens": screens,
            })
            .to_string()
        };
        let one = parse_kwin(&reply(1, "Docs", 2020)).unwrap();
        assert_eq!((one.id.as_str(), one.window.title.as_str(), one.window.app.as_str(), one.window.pid), ("{abc}", "Docs", "firefox", 42));
        assert_eq!((one.window.bounds, one.own), (Some([100.0, 120.0, 800.0, 600.0]), Some([80.0, 50.0, 1200.0, 800.0])), "frames are output-local");
        let two = parse_kwin(&reply(2, "Docs", 2020)).unwrap();
        assert_eq!((two.window.bounds, two.own, two.window.pid), (None, None, 42), "several outputs name the window but play no effects");
        assert!(one.same_window(&parse_kwin(&reply(1, "\u{280b} Docs", 2020)).unwrap()), "a CLI spinner frame is the same window");
        assert!(!one.same_window(&parse_kwin(&reply(1, "Docs", 2100)).unwrap()), "a moved window isn't named");
        assert!(!one.same_window(&parse_kwin(&reply(1, "Mail", 2020)).unwrap()));
        assert_eq!(parse_kwin(r#"{"window":null,"own":null,"screens":1}"#), None);
        let script = kwin_lookup_script(Some("{a'b}"), 7);
        assert!(script.contains(r#"String(w.internalId) === "{a'b}""#) && script.contains("o.pid === 7"), "{script}");
        assert!(kwin_lookup_script(None, 7).contains("const w = workspace.activeWindow;"));
    }

    #[test]
    fn decodes_kwin_raw_images_like_the_kde_helper() {
        let meta = |width, height, stride, format| RawMeta { kind: Some("raw".into()), width: Some(width), height: Some(height), stride, format: Some(format) };
        let raw = [0xff11_2233_u32.to_ne_bytes(), [0; 4]].concat();
        assert_eq!(kde_rgba(&meta(1, 1, Some(8), 5), &raw), Ok((1, 1, vec![0x11, 0x22, 0x33, 255])), "native ARGB32, row padding skipped");
        assert_eq!(kde_rgba(&meta(1, 1, None, 4), &0x0011_2233_u32.to_ne_bytes()).unwrap().2, [0x11, 0x22, 0x33, 255], "RGB32 is opaque, stride defaults to the row");
        assert_eq!(kde_rgba(&meta(1, 1, Some(4), 18), &[64, 32, 16, 128]).unwrap().2, [128, 64, 32, 128], "premultiplied RGBA is unpremultiplied");
        assert_eq!(kde_rgba(&meta(1, 1, Some(4), 18), &[0, 0, 0, 0]).unwrap().2, [0, 0, 0, 0]);
        assert_eq!(kde_rgba(&meta(1, 1, Some(4), 17), &[1, 2, 3, 4]).unwrap().2, [1, 2, 3, 4]);
        for (width, height, stride, format) in [(0, 1, 4, 5), (2, 1, 4, 5), (1, 1, 4, 99), (u32::MAX, 1, 4, 5)] {
            assert!(kde_rgba(&meta(width, height, Some(stride), format), &[0; 4]).is_err(), "{width}x{height} stride {stride} format {format}");
        }
        assert_eq!(kde_rgba(&meta(1, 2, Some(4), 5), &[0; 4]), Err("KDE Plasma returned an incomplete screenshot.".into()));
        assert!(kde_rgba(&RawMeta { kind: Some("png".into()), ..meta(1, 1, None, 5) }, &[0; 4]).is_err(), "only raw pixels");
        assert!(kde_rgba(&RawMeta { format: None, ..meta(1, 1, None, 5) }, &[0; 4]).is_err());
    }

    #[test]
    fn reads_extension_setup_like_gnome_capture_setup() {
        let bundled = (3, vec!["47".to_owned(), "48".to_owned()]);
        let shell = |state: Option<f64>, loaded: Option<f64>| Shell { version: "48.2".into(), user_extensions: true, state, loaded_version: loaded, error: None };
        assert_eq!(extension_status(&Shell { version: "44.0".into(), ..shell(None, None) }, None, &bundled).0, ExtensionStatus::Unsupported);
        assert_eq!(extension_status(&shell(None, None), None, &bundled).0, ExtensionStatus::NotInstalled);
        assert_eq!(extension_status(&shell(None, None), Some(3), &bundled).0, ExtensionStatus::RestartRequired, "installed but not loaded");
        assert_eq!(extension_status(&shell(Some(1.0), Some(2.0)), Some(3), &bundled).0, ExtensionStatus::RestartRequired, "an update waits for sign-in");
        assert_eq!(extension_status(&shell(Some(1.0), Some(2.0)), None, &bundled).0, ExtensionStatus::UpdateRequired);
        assert_eq!(extension_status(&Shell { user_extensions: false, ..shell(Some(2.0), Some(3.0)) }, Some(3), &bundled).0, ExtensionStatus::ExtensionsDisabled);
        assert_eq!(extension_status(&shell(Some(1.0), Some(3.0)), Some(3), &bundled).0, ExtensionStatus::Enabled);
        assert_eq!(extension_status(&shell(Some(2.0), Some(3.0)), Some(3), &bundled).0, ExtensionStatus::Disabled);
        let broken = Shell { error: Some("SyntaxError".into()), ..shell(Some(3.0), Some(3.0)) };
        assert_eq!(extension_status(&broken, Some(3), &bundled), (ExtensionStatus::Error, "SyntaxError".into()));
    }

    #[test]
    fn bundles_electrons_extension() {
        let (version, shells) = bundled();
        assert!(version >= 1 && shells.iter().any(|s| s == "48"), "{version} {shells:?}");
        assert_eq!(metadata(r#"{"uuid":"other@x","version":1,"shell-version":[]}"#), None);
        use ExtensionStatus as S;
        for status in [S::NotInstalled, S::RestartRequired, S::UpdateRequired, S::ExtensionsDisabled, S::Disabled, S::Enabled, S::Unsupported, S::Error] {
            let setup = scene_setup(Backend::Picker, Some(status));
            assert_eq!(setup.extension.map(|(s, _)| s), Some(status), "scenes reach every state through extension_status");
        }
    }

    #[test]
    fn reads_portal_files_and_extension_windows() {
        assert_eq!(uri_path("file:///tmp/Screenshot%20from%202026.png"), Some(PathBuf::from("/tmp/Screenshot from 2026.png")));
        assert_eq!(uri_path("file://localhost/tmp/a.png"), Some(PathBuf::from("/tmp/a.png")));
        assert_eq!(uri_path("https://example.com/a.png"), None);
        assert_eq!(uri_path("file://host/a.png"), None, "no network hosts");
        assert_eq!(uri_path("file:///tmp/%00x"), None);
        assert_eq!(fitted(5120, 2880), Some((2560, 1440)));
        assert_eq!(fitted(1200, 800), None);
        let docs = Window { title: "Docs".into(), app: "Firefox".into(), pid: 7, bounds: None };
        assert_eq!(parse_window(r#"{"title":"Docs","appName":"Firefox","appIdentifier":"firefox","processId":7,"bounds":{"x":0,"y":0,"width":10,"height":10}}"#), Some(docs));
        assert_eq!(parse_window("{}"), None);
    }

    #[test]
    fn reads_hyprland_and_niri_windows() {
        let active = json!({"at": [1930, 40], "size": [1200, 800], "pid": 42, "title": "Docs", "class": "firefox"});
        assert_eq!(grim_geometry(&active).as_deref(), Some("1930,40 1200x800"));
        let window = hyprland_window(&active, Some((1920.0, 0.0))).unwrap();
        assert_eq!((window.pid, window.app.as_str(), window.bounds), (42, "firefox", Some([10.0, 40.0, 1200.0, 800.0])), "output-local for the overlay");
        assert_eq!(hyprland_window(&active, None).unwrap().bounds, None, "no effects without one monitor");
        assert_eq!(hyprland_window(&json!({}), None), None, "hyprctl's empty reply when nothing is focused");
        assert_eq!(grim_geometry(&json!({})), None);
        assert!(niri_supported("niri 25.11 (abc)") && niri_supported("26.02") && !niri_supported("25.08") && !niri_supported("unknown"));
        let before = json!({"id": 3, "pid": 9, "title": "Term", "app_id": "foot", "layout": {"window_size": [800, 600]}});
        assert_eq!(niri_window(&before, Some(&before)).map(|w| (w.pid, w.app)), Some((9, "foot".into())));
        let retitled = json!({"id": 3, "pid": 9, "title": "Other", "app_id": "foot", "layout": {"window_size": [800, 600]}});
        assert_eq!(niri_window(&before, Some(&retitled)), None, "never attach text from a replacement");
        assert_eq!(niri_window(&before, None), None);
        assert!(Backend::Niri.knows_window() && !Backend::ScreenshotPortal.knows_window());
    }
}
