//! SnapShots on Linux, taken the way Electron takes them (apps/desktop/src/snapShot:
//! `LinuxSnapShot.ts`, `GnomeCaptureSetup.ts`, `linuxCaptureSession.ts`). Wayland only, as
//! Electron's `captureMode`. The Screenshot portal's active-window target comes first, then the
//! bundled GNOME extension, else the portal's own picker. Hyprland, KDE and Niri need helpers
//! Electron ships separately, so they take the portal path here.

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

/// The extension's window metadata: its title and app name, for the attachment's source.
pub fn parse_window(json: &str) -> Option<(String, String)> {
    let v: Value = serde_json::from_str(json).ok()?;
    Some((v["title"].as_str()?.to_owned(), v["appName"].as_str()?.to_owned()))
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

/// Captures with `backend` into `path`, returning its `SnapShotSource`.
pub fn capture(backend: Backend, path: &Path) -> Result<Value, String> {
    let (png, window) = match backend {
        Backend::ScreenshotPortal => (bus::screenshot(false)?, None),
        Backend::Picker => (bus::screenshot(true)?, None),
        Backend::GnomeExtension => {
            let (png, window) = bus::extension_capture()?;
            (png, parse_window(&window))
        }
    };
    store_png(&png, path)?;
    let (title, app) = window.unwrap_or_default();
    Ok(crate::desktop::capture::snap_shot_source(&app, &title, None))
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
    pub(super) fn extension_capture() -> Result<(Vec<u8>, String), String> {
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
            timed(async {
                let proxy = Proxy::new(conn, EXTENSION, EXTENSION_PATH, EXTENSION).await?;
                proxy.call::<_, _, (Vec<u8>, String)>("Capture", &()).await
            })
            .await
            .map_err(message)
        })
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
    pub(super) fn extension_capture() -> Result<(Vec<u8>, String), String> {
        Err(UNSUPPORTED.into())
    }
}

/// A setup the scenes force without a session bus, its message read as the real one is.
pub fn scene_setup(backend: Backend, extension: Option<ExtensionStatus>) -> Setup {
    let bundled = bundled();
    let extension = extension.map(|status| {
        use ExtensionStatus as S;
        let loaded = Some(f64::from(bundled.0));
        let (state, installed, user_extensions) = match status {
            S::NotInstalled => (None, None, true),
            S::RestartRequired => (None, Some(bundled.0), true),
            S::ExtensionsDisabled => (Some(2.0), Some(bundled.0), false),
            S::Enabled => (Some(1.0), Some(bundled.0), true),
            S::Error => (Some(3.0), Some(bundled.0), true),
            _ => (Some(2.0), Some(bundled.0), true),
        };
        let shell = Shell { version: format!("{}.1", bundled.1.last().cloned().unwrap_or_default()), user_extensions, state, loaded_version: loaded.filter(|_| state.is_some()), error: None };
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
        assert_eq!(parse_window(r#"{"title":"Docs","appName":"Firefox","appIdentifier":"firefox","processId":7,"bounds":{"x":0,"y":0,"width":10,"height":10}}"#), Some(("Docs".into(), "Firefox".into())));
        assert_eq!(parse_window("{}"), None);
    }
}
