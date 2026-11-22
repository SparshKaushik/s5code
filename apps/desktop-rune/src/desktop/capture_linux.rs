//! SnapShots on Linux, taken the way Electron takes them (apps/desktop/src/snapShot:
//! `LinuxSnapShot.ts`, `GnomeCaptureSetup.ts`, `NiriSnapShot.ts`, `linuxCaptureSession.ts`).
//! Wayland only, as Electron's `captureMode`. Hyprland (through `hyprctl` and `grim` instead of
//! Electron's helper) and Niri (its IPC socket) come first, then the Screenshot portal's
//! active-window target, the bundled GNOME extension, else the portal's own picker. KDE needs a
//! helper Electron ships separately, so it takes the portal path here. App text comes from
//! AT-SPI, as Electron's accessibility module reads it on Linux.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Electron's Linux desktop entry name: the GNOME extension only answers its `.SnapShot` name.
pub const APP_ID: &str = "com.t3tools.T3Code";
pub const EXTENSION_UUID: &str = "snap-shot@t3.codes";
const MAX_PNG_BYTES: usize = 32 * 1024 * 1024;
const MAX_CAPTURE: (f64, f64) = (2_560.0, 1_600.0);
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
}

impl Backend {
    /// Names the captured window's process, so its app text can be read.
    pub fn knows_window(self) -> bool {
        matches!(self, Backend::GnomeExtension | Backend::Hyprland | Backend::Niri)
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

/// `LinuxCaptureConnection.backend` without the KDE helper: the active-window portal target
/// (version 3, target bit 8), else the extension on GNOME (API version 1 or 2), else the picker.
pub fn backend_from(portal_version: Option<u32>, targets: Option<u32>, gnome: bool, extension_api: Option<u32>) -> Backend {
    if portal_version.is_some_and(|v| v >= 3) && targets.is_some_and(|t| t & 8 != 0) {
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
    match fitted(image.width(), image.height()) {
        Some((w, h)) => image.resize_exact(w, h, image::imageops::FilterType::Lanczos3).save_with_format(path, image::ImageFormat::Png),
        None => return std::fs::write(path, bytes).map_err(|_| "Could not save the snapshot.".to_owned()),
    }
    .map_err(|_| "Could not save the snapshot.".to_owned())
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
    let found = bus::probe(gnome)?;
    let extension = gnome.then(|| match &found.shell {
        Ok(shell) => extension_status(shell, installed_version(), &bundled()),
        Err(message) => (ExtensionStatus::Error, message.clone()),
    });
    Ok(Setup { backend: backend_from(found.portal_version, found.targets, gnome, found.extension_api), extension })
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
    let mut captured = Captured { window: None, own: None, effects_done: false };
    let png = match backend {
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
    portal_version: Option<u32>,
    targets: Option<u32>,
    extension_api: Option<u32>,
    shell: Result<Shell, String>,
}


#[cfg(target_os = "linux")]
mod bus {
    use super::{APP_ID, EXTENSION_UUID, Probe, Shell};
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

    pub(super) fn probe(gnome: bool) -> Result<Probe, String> {
        block(|conn| async move {
            let conn = &conn;
            // Unsandboxed apps name themselves to the portal so its permissions stick.
            if let Ok(registry) = Proxy::new(conn, PORTAL, PORTAL_PATH, "org.freedesktop.host.portal.Registry").await {
                let _ = timed(registry.call::<_, _, ()>("Register", &(APP_ID, HashMap::<&str, Variant>::new()))).await;
            }
            let mut found = Probe { portal_version: None, targets: None, extension_api: None, shell: Err(String::new()) };
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

    const ATSPI_ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
    const ATSPI_TEXT: &str = "org.a11y.atspi.Text";
    /// `STATE_ACTIVE` in the first word of `GetState`.
    const STATE_ACTIVE: u32 = 1 << 1;

    async fn call<B, R>(conn: &Connection, (dest, path): (&str, &str), interface: &str, method: &str, body: &B) -> zbus::Result<R>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
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
            let a11y = zbus::connection::Builder::address(address.as_str()).map_err(message)?.build().await.map_err(message)?;
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
    pub(super) fn probe(_: bool) -> Result<Probe, String> {
        Err(UNSUPPORTED.into())
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

    #[test]
    fn picks_the_backend_like_electron() {
        assert_eq!(backend_from(Some(3), Some(8 | 1), false, None), Backend::ScreenshotPortal);
        assert_eq!(backend_from(Some(2), Some(8), true, Some(2)), Backend::GnomeExtension, "an older portal has no active-window target");
        assert_eq!(backend_from(Some(3), Some(1), true, Some(1)), Backend::GnomeExtension);
        assert_eq!(backend_from(Some(3), Some(1), true, Some(3)), Backend::Picker, "an unknown extension API");
        assert_eq!(backend_from(None, None, false, Some(2)), Backend::Picker, "the extension only counts on GNOME");
        assert!(gnome_session(Some("ubuntu:GNOME"), false));
        assert!(!gnome_session(Some("GNOME"), true), "sandboxed apps can't use the extension");
        assert!(!gnome_session(Some("KDE"), false));
        assert!(!gnome_session(None, false));
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
