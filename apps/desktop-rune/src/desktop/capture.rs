//! SnapShots on macOS, taken the way Electron takes them (apps/desktop/src/snapShot): JXA names
//! the front window (`ActiveWindow.ts`), `screencapture` grabs it (`MacSnapShot.ts`) and a JXA
//! poller watches modifier pairs (`MacModifierPairShortcutProcess.ts`). Everything compiles on
//! every platform because CI builds on Linux; `supported` gates it at runtime.

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rune::{El, Tx, Waker};
use serde_json::{Value, json};

use crate::{App, Route};

const OSASCRIPT: &str = "/usr/bin/osascript";
const SCREENCAPTURE: &str = "/usr/sbin/screencapture";
const AFPLAY: &str = "/usr/bin/afplay";
const OPEN: &str = "/usr/bin/open";
/// The name macOS lists under Privacy & Security.
const APP_NAME: &str = "S5 Code";
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(15);
const HELPER_STRIP_MAX_THICKNESS_RATIO: f64 = 0.25;
const MAX_CAPTURE: (f64, f64) = (2_560.0, 1_600.0);
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
const FLASH_DURATION: Duration = Duration::from_millis(180);
const FLASH_PEAK_OPACITY: f32 = 0.08;
const PERMISSION_POLL: Duration = Duration::from_secs(2);
const SCREEN_RECORDING_SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";
const ACCESSIBILITY_SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
/// `ACCESSIBILITY_TIMEOUT_MS`; the script stops itself a little sooner to return what it read.
pub(crate) const TEXT_TIMEOUT: Duration = Duration::from_millis(3_000);
const TEXT_SCRIPT_BUDGET_MS: u64 = 2_500;
/// `SNAP_SHOT_ACCESSIBLE_TEXT_MAX_CHARS` and `MAX_ACCESSIBILITY_TREE_NODES`.
pub const ACCESSIBLE_TEXT_MAX_CHARS: usize = 32_000;
pub(crate) const ACCESSIBILITY_MAX_NODES: u32 = 10_000;
/// `SnapShotTransition`'s duration range, distance scale and easing.
const FLIGHT_MIN_MS: f32 = 280.0;
const FLIGHT_MAX_MS: f32 = 680.0;
const FLIGHT_DISTANCE_SCALE: f32 = 2_000.0;
const FLIGHT_EASING: (f32, f32, f32, f32) = (0.2, 0.8, 0.2, 1.0);
/// The card's height where it lands on the composer's attachment row, and the decoded size cap.
const FLIGHT_CARD_HEIGHT: f32 = 56.0;
const FLIGHT_IMAGE_MAX: u32 = 480;
const FLIGHT_FADE: Duration = Duration::from_millis(120);
pub const SCREEN_RECORDING_MESSAGE: &str = "Allow Screen Recording in System Settings, then restart T3 Code.";
pub const SHORTCUT_TAKEN: &str = "This shortcut is already used by the system or another app.";
const WINDOW_UNAVAILABLE: &str = "The active window is not available for capture.";
const CAPTURE_FAILED: &str = "Could not capture the active window.";

const LOOKUP_SCRIPT: &str = r#"
ObjC.import("CoreGraphics");
ObjC.import("AppKit");
function run() {
  const app = $.NSWorkspace.sharedWorkspace.frontmostApplication;
  if (app.isNil()) return "";
  const pid = app.processIdentifier;
  let ownerName = String(app.localizedName.js || "");
  const list = $.CGWindowListCopyWindowInfo(
    $.kCGWindowListOptionOnScreenOnly | $.kCGWindowListExcludeDesktopElements,
    $.kCGNullWindowID,
  );
  $.CFMakeCollectable(list);
  const count = $.CFArrayGetCount(list);
  const windows = [];
  for (let i = 0; i < count; i++) {
    const w = ObjC.castRefToObject($.CFArrayGetValueAtIndex(list, i));
    if (w.objectForKey("kCGWindowOwnerPID").js !== pid) continue;
    if (w.objectForKey("kCGWindowLayer").js !== 0) continue;
    if (!ownerName) {
      ownerName = String(ObjC.unwrap(w.objectForKey("kCGWindowOwnerName")) || "");
    }
    const b = ObjC.deepUnwrap(w.objectForKey("kCGWindowBounds"));
    windows.push({
      id: w.objectForKey("kCGWindowNumber").js,
      title: String(ObjC.unwrap(w.objectForKey("kCGWindowName")) || ""),
      bounds: { x: b.X, y: b.Y, width: b.Width, height: b.Height },
      alpha: w.objectForKey("kCGWindowAlpha").js,
    });
  }
  return JSON.stringify({ owner: { name: ownerName, processId: pid }, windows });
}"#;

// CoreGraphics registers osascript as a foreground app; go background-only first to avoid a Dock tile.
const POLLER_SCRIPT: &str = r#"
ObjC.import("AppKit");
$.NSApplication.sharedApplication.setActivationPolicy($.NSApplicationActivationPolicyProhibited);
ObjC.import("CoreGraphics");
ObjC.import("unistd");
function run(argv) {
  const left = Number(argv[0]);
  const right = Number(argv[1]);
  const both = left | right;
  let active = false;
  console.log("ready");
  while ($.getppid() !== 1) {
    const pressed = ($.CGEventSourceFlagsState(0) & both) === both;
    if (pressed && !active) console.log("trigger");
    active = pressed;
    delay(0.05);
  }
  return "orphaned";
}"#;

// TCC answers for the responsible process, which for this child is the app. Null means unknown.
const PERMISSIONS_SCRIPT: &str = r#"
ObjC.import("CoreGraphics");
ObjC.import("ApplicationServices");
function run(argv) {
  let screen = null;
  let accessibility = null;
  try {
    if (argv[0] === "request" && !$.CGPreflightScreenCaptureAccess()) $.CGRequestScreenCaptureAccess();
    screen = Boolean($.CGPreflightScreenCaptureAccess());
  } catch (e) {}
  try {
    accessibility = argv[0] === "request-accessibility"
      ? Boolean($.AXIsProcessTrustedWithOptions($({ AXTrustedCheckOptionPrompt: true })))
      : Boolean($.AXIsProcessTrusted());
  } catch (e) {}
  return JSON.stringify({ screenRecording: screen, accessibility });
}"#;

// Electron's `accessibleWindowText` walk through System Events: each element's name (else its
// description) and value, depth first in order, until the node, size or time budget runs out.
const TEXT_SCRIPT: &str = r#"
function run(argv) {
  const pid = Number(argv[0]);
  const title = argv[1];
  const deadline = Date.now() + Number(argv[2]);
  const budget = Number(argv[3]);
  const maxNodes = Number(argv[4]);
  const events = Application("System Events");
  const processes = events.processes.whose({ unixId: pid });
  if (processes.length === 0) return "[]";
  const windows = processes[0].windows;
  let window = null;
  try {
    const named = windows.whose({ name: title });
    if (title && named.length > 0) window = named[0];
  } catch (e) {}
  if (!window) {
    if (windows.length === 0) return "[]";
    window = windows[0];
  }
  const text = (read) => {
    try {
      const v = read();
      return typeof v === "string" ? v : typeof v === "number" ? String(v) : "";
    } catch (e) {
      return "";
    }
  };
  const out = [];
  const stack = [window];
  let used = 0;
  let visited = 0;
  while (stack.length > 0 && visited < maxNodes && used < budget && Date.now() < deadline) {
    const element = stack.pop();
    visited += 1;
    const name = text(() => element.name()) || text(() => element.description());
    const value = text(() => element.value());
    out.push(name, value);
    used += name.length + value.length;
    let children = [];
    try { children = element.uiElements(); } catch (e) {}
    for (let i = children.length - 1; i >= 0; i--) stack.push(children[i]);
  }
  return JSON.stringify(out);
}"#;

pub fn supported() -> bool {
    cfg!(target_os = "macos") || linux()
}

/// Linux capture, which Electron offers on Wayland only.
pub fn linux() -> bool {
    crate::desktop::capture_linux::wayland()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub id: u64,
    pub title: String,
    pub bounds: Bounds,
    pub alpha: f64,
}

/// What `macCommandFailure` makes of a failed step; `Missing` is no window to capture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    Missing,
    TimedOut,
    Exited(i32),
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    FindWindow,
    CaptureWindow,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Permissions {
    pub screen_recording: bool,
    pub accessibility: bool,
}

/// `macActiveWindow`'s answer: the front app and its layer-0 windows, front to back.
#[derive(Clone, Debug, PartialEq)]
pub struct Front {
    pub owner: String,
    pub pid: u32,
    pub windows: Vec<Candidate>,
}

/// What a capture found: the window, for the flash and the flight, and its `SnapShotSource`.
pub struct Shot {
    bounds: Bounds,
    source: Value,
    thumbnail: Option<rune::Pixels>,
    /// Our window's origin in the overlay's space, where Wayland reports the composer at (0, 0).
    origin: (f32, f32),
    /// Linux: why the capture took another path, shown once it lands.
    notice: Option<String>,
}

/// The flight overlay's state: the captured picture and the card's frame inside the overlay.
#[derive(Default)]
pub struct Flight {
    pixels: rune::Pixels,
    card: [f32; 4],
}

enum FlightStage {
    /// Shown at the captured window; the next poll sends it on its way.
    Launch { shown: Instant, to: [f32; 4], seconds: f32 },
    Flying { until: Instant },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Test {
    #[default]
    Idle,
    Running,
    Passed,
    Failed(String),
}

/// Electron's `DesktopSnapShotState`, as far as macOS capture uses it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub supported: bool,
    /// Unknown until the first check.
    pub permissions: Option<Permissions>,
    pub shortcut_registered: bool,
    pub shortcut_message: Option<String>,
    /// The shortcut fired at least once since it was registered.
    pub shortcut_verified: bool,
    /// Why capture can't run.
    pub message: Option<String>,
    pub test: Test,
    /// Linux: the capture backend and GNOME extension setup, unknown until checked.
    pub linux: Option<crate::desktop::capture_linux::Setup>,
    /// Linux: a check or extension action is running.
    pub linux_busy: bool,
    /// Linux: why the last check or action failed.
    pub linux_error: Option<String>,
    /// Linux: the portal is asking the user to approve the shortcut.
    pub shortcut_pending: bool,
    /// Niri and Hyprland: the binding to add to the compositor's config.
    pub shortcut_binding: Option<String>,
    pub shortcut_config_path: Option<String>,
    /// Niri and Hyprland: the action the binding calls is served (unknown while connecting).
    pub shortcut_action_registered: Option<bool>,
}

/// The registration the settings ask for.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    Off,
    Blocked,
    Pair(&'static str),
    Chord(String),
    Invalid,
    /// The compositor's config binds the keys; T3 Code serves the action it calls.
    Managed(crate::desktop::capture_config::Desktop),
}

enum Event {
    Triggered(u64),
    PairExited(u64),
    Captured(Result<(Shot, PathBuf), (Step, Status)>),
    Tested(Result<(), (Step, Status)>),
    Permissions(Option<Permissions>),
    CaptureFailed(String),
    LinuxChecked(Result<crate::desktop::capture_linux::Setup, String>),
    LinuxActed(Result<(), String>),
    Registered(u64, Result<rune::ShortcutId, rune::ShortcutError>),
    Managed(u64, crate::desktop::capture_config::Desktop, Result<bool, String>),
}

#[derive(Default)]
enum Registration {
    #[default]
    None,
    Chord(rune::ShortcutId),
    Pair(Child),
    /// Stops the Niri endpoint or Hyprland portal session.
    Managed(std::sync::Arc<std::sync::atomic::AtomicBool>),
}

#[derive(Default)]
struct Live {
    app: Option<rune::AppHandle<App>>,
    flash: Option<rune::OverlayHandle<()>>,
    flash_shown: Option<Instant>,
    flight: Option<rune::OverlayHandle<Flight>>,
    flight_stage: Option<FlightStage>,
    /// The composer's frame in screen points, where a flight lands.
    composer: Option<[f32; 4]>,
    /// The window's size in points, for the GNOME extension's relative landing frame.
    window: Option<[f32; 2]>,
    applied: Option<Plan>,
    registration: Registration,
    generation: u64,
    state: State,
    capturing: bool,
    checking: bool,
    checked_at: Option<Instant>,
}

thread_local! {
    static LIVE: RefCell<Live> = RefCell::new(Live::default());
}

static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());

fn push(event: Event, waker: &Option<Waker>) {
    if let Ok(mut events) = EVENTS.lock() {
        events.push(event);
    }
    if let Some(wake) = waker {
        wake();
    }
}

/// Keeps the app handle for shortcuts and, on macOS, adds the flash overlay.
pub fn register(app: &mut rune::App<App>) {
    let handle = app.handle();
    // Overlays need window positions, which Wayland hides; Hyprland reports them through hyprctl.
    let overlays = cfg!(target_os = "macos") || crate::desktop::capture_linux::hyprland_session();
    let flash = overlays.then(|| {
        let spec = rune::OverlaySpec { name: "SnapShot flash", size: (1.0, 1.0), edge: rune::ScreenEdge::Top, margin: 0.0, align: rune::Align::Center, inset: 0.0, interactive: false, view: flash_view };
        let options = rune::OverlayOptions { input: rune::OverlayInput::None, exclude_from_capture: true, ..Default::default() };
        app.overlay(spec.options(options), ())
    });
    let flight = overlays.then(|| {
        let spec = rune::OverlaySpec { name: "SnapShot flight", size: (1.0, 1.0), edge: rune::ScreenEdge::Top, margin: 0.0, align: rune::Align::Center, inset: 0.0, interactive: false, view: flight_view };
        let options = rune::OverlayOptions { input: rune::OverlayInput::None, exclude_from_capture: true, ..Default::default() };
        app.overlay(spec.options(options), Flight::default())
    });
    LIVE.with(|live| {
        let mut live = live.borrow_mut();
        live.app = Some(handle);
        live.flash = flash;
        live.flight = flight;
    });
}

fn flash_view(_: &(), _: &mut rune::Cx<()>) -> El {
    rune::el().w_full().h_full().fill(rune::Color::WHITE.alpha(FLASH_PEAK_OPACITY))
}

/// `SnapShotTransition`'s card: the captured window's picture, rounded, with a faint edge.
fn flight_view(f: &Flight, _: &mut rune::Cx<Flight>) -> El {
    let [x, y, w, h] = f.card;
    let card = rune::el()
        .key("snap-shot-flight-card")
        .absolute(x, y)
        .w(w)
        .h(h)
        .clip_rounded(10.0)
        .border(1.0, rune::Color::WHITE.alpha(0.25))
        .pixels_fit(&f.pixels, rune::SurfaceFit::Fill);
    rune::el().w_full().h_full().child(card)
}

/// Where a flight lands: the composer's attachment row, as tall as a thumbnail, the window's
/// aspect kept.
pub fn flight_target(composer: [f32; 4], source: Bounds) -> [f32; 4] {
    let aspect = (source.width / source.height.max(1.0)) as f32;
    let width = (FLIGHT_CARD_HEIGHT * aspect).clamp(40.0, (composer[2] - 26.0).max(40.0));
    [composer[0] + 13.0, composer[1] + 13.0, width, FLIGHT_CARD_HEIGHT]
}

/// `snapShotAnimationDurationMs`, in seconds: longer flights take longer, within 280–680 ms.
pub fn flight_seconds(source: [f32; 4], target: [f32; 4]) -> f32 {
    let center = |r: [f32; 4]| (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    let ((sx, sy), (tx, ty)) = (center(source), center(target));
    let distance = (sx - tx).hypot(sy - ty);
    (FLIGHT_MAX_MS - (FLIGHT_MAX_MS - FLIGHT_MIN_MS) * (-distance / FLIGHT_DISTANCE_SCALE).exp()) / 1000.0
}

/// The overlay's frame covering both ends, and each end inside it.
pub fn flight_frames(source: [f32; 4], target: [f32; 4]) -> ([f32; 4], [f32; 4], [f32; 4]) {
    let x = source[0].min(target[0]);
    let y = source[1].min(target[1]);
    let right = (source[0] + source[2]).max(target[0] + target[2]);
    let bottom = (source[1] + source[3]).max(target[1] + target[3]);
    let local = |r: [f32; 4]| [r[0] - x, r[1] - y, r[2], r[3]];
    ([x, y, right - x, bottom - y], local(source), local(target))
}

/// The composer reports its frame here so a capture knows where to fly.
pub fn note_composer(_: &mut App, rect: [f32; 4], window: Option<[f32; 2]>) {
    if supported() {
        LIVE.with(|live| {
            let mut live = live.borrow_mut();
            live.composer = Some(rect);
            live.window = window;
        });
    }
}

/// The window's size, read where the composer is drawn.
pub fn window_size(cx: &mut rune::Cx<App>) -> Option<[f32; 2]> {
    cx.layout_with(super::FRAME, |r| [r[2], r[3]])
}

/// Where the GNOME extension lands its flight: the composer's attachment row as fractions of
/// the window. The window's aspect is unknown before capture, so the card is 16:10.
pub fn relative_landing(composer: [f32; 4], window: [f32; 2]) -> Option<[f64; 4]> {
    let [w, h] = window;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let width = (FLIGHT_CARD_HEIGHT * 1.6).min((composer[2] - 26.0).max(1.0));
    let frame = [(composer[0] + 13.0) / w, (composer[1] + 13.0) / h, width / w, FLIGHT_CARD_HEIGHT / h];
    let fits = frame.iter().all(|v| v.is_finite() && *v >= 0.0) && frame[2] > 0.0 && frame[0] + frame[2] <= 1.01 && frame[1] + frame[3] <= 1.01;
    fits.then(|| frame.map(f64::from))
}

pub fn state() -> State {
    LIVE.with(|live| State { supported: supported(), ..live.borrow().state.clone() })
}

/// `decodeMacActiveWindowLookup`: the front app and its layer-0 windows, front to back.
pub fn parse_lookup(output: &str) -> Result<Option<Front>, Status> {
    let output = output.trim();
    if output.is_empty() {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(output).map_err(|_| Status::Failed)?;
    let owner = v["owner"]["name"].as_str().ok_or(Status::Failed)?.to_owned();
    let pid = v["owner"]["processId"].as_u64().unwrap_or_default() as u32;
    let mut windows = Vec::new();
    for w in v["windows"].as_array().ok_or(Status::Failed)? {
        let n = |v: &Value| v.as_f64().ok_or(Status::Failed);
        let b = &w["bounds"];
        windows.push(Candidate {
            id: n(&w["id"])? as u64,
            title: w["title"].as_str().ok_or(Status::Failed)?.to_owned(),
            bounds: Bounds { x: n(&b["x"])?, y: n(&b["y"])?, width: n(&b["width"])?, height: n(&b["height"])? },
            alpha: n(&w["alpha"])?,
        });
    }
    Ok(Some(Front { owner, pid, windows }))
}

/// `macActiveWindow`: the frontmost visible window, skipping untitled helper strips that hug an
/// edge of a later window (a toolbar or sidebar drawn as its own window).
pub fn choose(windows: &[Candidate]) -> Option<&Candidate> {
    let r = HELPER_STRIP_MAX_THICKNESS_RATIO;
    let visible: Vec<&Candidate> = windows.iter().filter(|w| w.alpha > 0.0 && w.bounds.width > 0.0 && w.bounds.height > 0.0).collect();
    let strip = |b: Bounds, rb: Bounds| {
        (b.x == rb.x && b.width == rb.width && b.y <= rb.y && rb.y - b.y <= rb.height * r && b.height <= b.width * r && b.height <= rb.height * r)
            || (b.y == rb.y && b.height == rb.height && b.x <= rb.x && rb.x - b.x <= rb.width * r && b.width <= b.height * r && b.width <= rb.width * r)
    };
    visible
        .iter()
        .enumerate()
        .find(|(i, c)| !c.title.trim().is_empty() || !visible[i + 1..].iter().any(|reference| strip(c.bounds, reference.bounds)))
        .map(|(_, c)| *c)
}

/// `snapShotThumbnailSize` and the resize in `captureMacWindowSnapshot`: the smaller size to
/// store, when the capture is larger than the window's points or 2560×1600.
pub fn fitted(png: (u32, u32), bounds: Bounds) -> Option<(u32, u32)> {
    let max_w = bounds.width.max(1.0).min(MAX_CAPTURE.0);
    let max_h = bounds.height.max(1.0).min(MAX_CAPTURE.1);
    let (w, h) = (f64::from(png.0.max(1)), f64::from(png.1.max(1)));
    let scale = (max_w / w).min(max_h / h).min(1.0);
    (scale < 1.0).then(|| (((w * scale).round() as u32).max(1), ((h * scale).round() as u32).max(1)))
}

/// `DesktopSnapShotSetupError.message` for the test capture.
pub fn test_message(step: Step, status: Status) -> String {
    let summary = "Could not capture a test snapshot.";
    if status == Status::Missing {
        return format!("{summary} No window is available to capture.");
    }
    let (task, command) = match step {
        Step::FindWindow => ("Finding the front window", "osascript"),
        Step::CaptureWindow => ("Capturing the window", "screencapture"),
    };
    let failure = match status {
        Status::TimedOut => format!("{task} timed out."),
        Status::Exited(code) => format!("{task} failed: {command} exited with code {code}."),
        _ => format!("{task} failed."),
    };
    // macOS can report both permissions as allowed while they belong to another install.
    format!("{summary} {failure} macOS grants Screen Recording and Accessibility to each app separately. In System Settings, open Privacy & Security and check that \"{APP_NAME}\" is allowed for both.")
}

/// `DesktopSnapShotError.message` for a shortcut capture.
pub fn capture_message(status: Status) -> &'static str {
    if status == Status::Missing { WINDOW_UNAVAILABLE } else { CAPTURE_FAILED }
}

/// Runs a tool with Electron's `execFile` timeout semantics.
fn run(program: &str, args: &[&str], timeout: Duration) -> Result<String, Status> {
    let mut child = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|_| Status::Failed)?;
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        if let Some(mut stdout) = stdout {
            let _ = stdout.read_to_string(&mut out);
        }
        out
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Status::TimedOut);
            }
            Err(_) => return Err(Status::Failed),
        }
    };
    let out = reader.join().unwrap_or_default();
    match status.code() {
        Some(0) => Ok(out),
        Some(code) => Err(Status::Exited(code)),
        None => Err(Status::Failed),
    }
}

fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let word = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    bytes.starts_with(&PNG_SIGNATURE).then_some(())?;
    Some((word(16)?, word(20)?))
}

/// How a capture runs: whether to read the window's text and to decode a picture to fly.
#[derive(Clone, Copy, Default)]
struct Extras {
    text: bool,
    thumbnail: bool,
}

/// Finds and captures the front window into `path`, scaled down as Electron stores it.
fn capture_front(path: &Path, extras: Extras) -> Result<Shot, (Step, Status)> {
    let find = |status| (Step::FindWindow, status);
    let failed = (Step::CaptureWindow, Status::Failed);
    let output = run(OSASCRIPT, &["-l", "JavaScript", "-e", LOOKUP_SCRIPT], LOOKUP_TIMEOUT).map_err(find)?;
    let front = parse_lookup(&output).map_err(find)?.ok_or(find(Status::Missing))?;
    let window = choose(&front.windows).ok_or(find(Status::Missing))?;
    let (id, target) = (window.id.to_string(), path.to_string_lossy());
    run(SCREENCAPTURE, &["-l", id.as_str(), "-o", "-x", "-t", "png", &*target], CAPTURE_TIMEOUT).map_err(|status| (Step::CaptureWindow, status))?;
    let bytes = std::fs::read(path).map_err(|_| failed)?;
    let size = png_size(&bytes).ok_or(failed)?;
    let decoded = || image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).map_err(|_| failed);
    let mut image = None;
    if let Some((w, h)) = fitted(size, window.bounds) {
        let resized = decoded()?.resize_exact(w, h, image::imageops::FilterType::Lanczos3);
        resized.save_with_format(path, image::ImageFormat::Png).map_err(|_| failed)?;
        image = Some(resized);
    }
    let thumbnail = extras.thumbnail.then(|| image.map_or_else(decoded, Ok).ok()).flatten().map(flight_pixels);
    let text = extras.text.then(|| read_text(front.pid, &window.title)).flatten();
    let source = snap_shot_source(&front.owner, &window.title, text.as_deref());
    Ok(Shot { bounds: window.bounds, source, thumbnail, origin: (0.0, 0.0), notice: None })
}

fn flight_pixels(image: image::DynamicImage) -> rune::Pixels {
    let small = image.thumbnail(FLIGHT_IMAGE_MAX, FLIGHT_IMAGE_MAX).to_rgba8();
    let pixels = rune::Pixels::new();
    pixels.set(small.width(), small.height(), small.into_raw());
    pixels
}

/// The window's text through System Events, or None when Accessibility isn't allowed or the
/// app exposes none.
fn read_text(pid: u32, title: &str) -> Option<String> {
    let args = [pid.to_string(), title.to_owned(), TEXT_SCRIPT_BUDGET_MS.to_string(), (ACCESSIBLE_TEXT_MAX_CHARS * 2).to_string(), ACCESSIBILITY_MAX_NODES.to_string()];
    let mut argv = vec!["-l", "JavaScript", "-e", TEXT_SCRIPT];
    argv.extend(args.iter().map(String::as_str));
    let output = run(OSASCRIPT, &argv, TEXT_TIMEOUT).ok()?;
    let values: Vec<String> = serde_json::from_str(output.trim()).ok()?;
    Some(accessible_text(&values, ACCESSIBLE_TEXT_MAX_CHARS)).filter(|t| !t.is_empty())
}

/// `accessibleWindowText` over the walk's names and values: trimmed, each once, one per line,
/// cut at `max` UTF-16 units without splitting a character.
pub fn accessible_text(values: &[String], max: usize) -> String {
    let mut seen: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut used = 0;
    for value in values {
        let cleaned = value.replace('\0', "");
        let candidate = cleaned.trim();
        if candidate.is_empty() || seen.iter().any(|s| s == candidate) {
            continue;
        }
        let separator = usize::from(!text.is_empty());
        let remaining = max.saturating_sub(used + separator);
        if remaining == 0 {
            return text;
        }
        let mut piece = String::new();
        let mut units = 0;
        for c in candidate.chars() {
            if units + c.len_utf16() > remaining {
                break;
            }
            piece.push(c);
            units += c.len_utf16();
        }
        if piece.is_empty() {
            return text;
        }
        if separator == 1 {
            text.push('\n');
        }
        text.push_str(&piece);
        used += separator + units;
        if piece.len() < candidate.len() {
            return text;
        }
        seen.push(candidate.to_owned());
    }
    text
}

/// Where a snapshot's `SnapShotSource` waits beside its picture until the draft sends it.
pub fn source_path(png: &str) -> String {
    format!("{png}.source.json")
}

/// A queued file's `SnapShotSource`, when it is a snapshot.
pub fn attached_source(png: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(source_path(png)).ok()?).ok()?;
    (v["kind"] == "snap-shot").then_some(v)
}

/// `SnapShotSource` for a capture: the app, the window title and, when read, its text.
pub fn snap_shot_source(owner: &str, title: &str, text: Option<&str>) -> Value {
    let clip = |s: &str, n: usize| s.trim().chars().take(n).collect::<String>().trim().to_owned();
    let app = Some(clip(owner, 255)).filter(|a| !a.is_empty()).unwrap_or_else(|| "Window".into());
    let mut source = json!({
        "kind": "snap-shot",
        "capturedAt": crate::shell::palette::add_project::iso_now(),
        "appName": app,
        "windowTitle": clip(title, 1_000),
    });
    if let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) {
        source["accessibleText"] = json!(text);
    }
    source
}

/// `isModifierPairShortcut`: the modifier a pair shortcut watches.
pub fn pair_modifier(shortcut: &Value) -> Option<&'static str> {
    match shortcut["kind"].as_str()? {
        "both-shift-keys" => Some("shift"),
        "modifier-pair" => ["shift", "meta", "control", "alt"].into_iter().find(|m| shortcut["modifier"] == *m),
        _ => None,
    }
}

/// `snapShotModifierPairLabel` on macOS.
pub fn pair_label(modifier: &str) -> String {
    pair_label_for(modifier, cfg!(target_os = "macos"))
}

/// `snapShotModifierPairLabel`: Apple names on macOS, else Ctrl/Alt/Super.
fn pair_label_for(modifier: &str, apple: bool) -> String {
    let label = match (modifier, apple) {
        ("meta", true) => "Command",
        ("control", true) => "Control",
        ("alt", true) => "Option",
        ("meta", false) => "Super",
        ("control", false) => "Ctrl",
        ("alt", false) => "Alt",
        _ => "Shift",
    };
    format!("{label} + {label}")
}

fn pair_masks(modifier: &str) -> (u32, u32) {
    match modifier {
        "control" => (0x1, 0x2000),
        "alt" => (0x20, 0x40),
        "meta" => (0x8, 0x10),
        _ => (0x2, 0x4),
    }
}

/// `toElectronAccelerator` in Rune's accelerator syntax; None for a chord without a modifier.
pub fn accelerator(shortcut: &Value) -> Option<String> {
    let flag = |k: &str| shortcut[k].as_bool().unwrap_or(false);
    let key = match shortcut["key"].as_str()? {
        " " => "space",
        "+" | "" => return None,
        key => key,
    };
    let mut parts: Vec<&str> = [("ctrlKey", "ctrl"), ("altKey", "alt"), ("shiftKey", "shift")].into_iter().filter(|(k, _)| flag(*k)).map(|(_, m)| m).collect();
    if flag("metaKey") || flag("modKey") {
        parts.push("cmd");
    }
    if parts.is_empty() {
        return None;
    }
    parts.push(key);
    Some(parts.join("+"))
}

/// `applySettings`: what to register for these settings. Unknown permissions wait for the check.
pub fn plan(enabled: bool, permissions: Option<Permissions>, suppressed: bool, shortcut: &Value) -> Plan {
    if !enabled || suppressed {
        return Plan::Off;
    }
    match permissions {
        None => return Plan::Off,
        Some(p) if !p.screen_recording => return Plan::Blocked,
        Some(_) => {}
    }
    if let Some(modifier) = pair_modifier(shortcut) {
        return Plan::Pair(modifier);
    }
    accelerator(shortcut).map_or(Plan::Invalid, Plan::Chord)
}

/// `snapShotShortcutSystemConflict`.
pub fn system_conflict(shortcut: &Value) -> Option<String> {
    let flag = |k: &str| shortcut[k].as_bool().unwrap_or(false);
    let held = ["modKey", "metaKey", "ctrlKey", "altKey", "shiftKey"].into_iter().filter(|k| flag(*k)).count();
    if held != 1 {
        return None;
    }
    if flag("shiftKey") {
        return Some("Shift combinations are used for typing and text selection. Add another modifier.".into());
    }
    let key = shortcut["key"].as_str().unwrap_or_default().to_lowercase();
    if flag("modKey") {
        const COMMON: [(&str, &str); 13] = [("a", "Select All"), ("c", "Copy"), ("f", "Find"), ("n", "New"), ("o", "Open"), ("p", "Print"), ("q", "Quit"), ("s", "Save"), ("t", "New Tab"), ("v", "Paste"), ("w", "Close Window"), ("x", "Cut"), ("z", "Undo")];
        return COMMON.iter().find(|(k, _)| *k == key).map(|(_, action)| format!("This shortcut is {action} in most apps."));
    }
    if flag("ctrlKey") && ["c", "d", "z"].contains(&key.as_str()) {
        return Some("This shortcut controls running commands in terminals.".into());
    }
    if flag("altKey") && key == "tab" {
        return Some("The system uses Alt+Tab to switch apps.".into());
    }
    if flag("metaKey") && ["l", " "].contains(&key.as_str()) {
        return Some("The system already uses this shortcut.".into());
    }
    None
}

/// `checkShortcut`: whether `shortcut` can be saved, or why not.
pub fn availability(shortcut: &Value) -> Result<(), String> {
    if pair_modifier(shortcut).is_some() {
        return Ok(());
    }
    if let Some(conflict) = system_conflict(shortcut) {
        return Err(conflict);
    }
    let parsed = accelerator(shortcut).and_then(|a| a.parse::<rune::Shortcut>().ok()).ok_or_else(|| SHORTCUT_TAKEN.to_owned())?;
    LIVE.with(|live| {
        let live = live.borrow();
        // On Linux a trial registration would raise the portal's consent dialog.
        let Some(app) = live.app.as_ref().filter(|_| cfg!(target_os = "macos")) else { return Ok(()) };
        let id = app.register_shortcut(parsed, |_: &mut App, _: &mut Tx| {}).map_err(|_| SHORTCUT_TAKEN.to_owned())?;
        let _ = app.unregister_shortcut(id);
        Ok(())
    })
}

fn release(live: &mut Live) {
    match std::mem::take(&mut live.registration) {
        Registration::Chord(id) => {
            if let Some(app) = &live.app {
                let _ = app.unregister_shortcut(id);
            }
        }
        Registration::Pair(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
        }
        Registration::Managed(stop) => stop.store(true, std::sync::atomic::Ordering::Relaxed),
        Registration::None => {}
    }
}

/// Niri and Hyprland bind capture in their own config (Electron's `startNiriCaptureShortcut` and
/// `PortalCaptureShortcut` managed by Hyprland), whatever shortcut the settings hold. Until the
/// backend is known nothing registers, so the portal isn't asked for a chord it won't use.
fn linux_plan(wanted: Plan, backend: Option<crate::desktop::capture_linux::Backend>, checking: bool) -> Plan {
    use crate::desktop::{capture_config::Desktop, capture_linux::Backend};
    match (wanted, backend) {
        (w @ (Plan::Off | Plan::Blocked), _) => w,
        (_, Some(Backend::Niri)) => Plan::Managed(Desktop::Niri),
        (_, Some(Backend::Hyprland)) => Plan::Managed(Desktop::Hyprland),
        (_, None) if checking => Plan::Off,
        (w, _) => w,
    }
}

const HYPRLAND_CONNECTING: &str = "Connecting to Hyprland shortcuts…";

/// The state a managed shortcut's serving result leaves: whether its action is registered, and
/// Electron's message.
fn managed_status(desktop: crate::desktop::capture_config::Desktop, result: &Result<bool, String>) -> (bool, &'static str) {
    use crate::desktop::capture_config::Desktop;
    match (desktop, result) {
        (Desktop::Niri, Ok(_)) => (true, "Set up the shortcut to add it to your Niri config."),
        (Desktop::Niri, Err(_)) => (false, "Could not start the Niri capture endpoint. Another T3 Code instance may be using it."),
        (Desktop::Hyprland, Ok(true)) => (true, "Managed by Hyprland. Add the binding to your config and save it."),
        (Desktop::Hyprland, Ok(false)) => (false, "Hyprland did not register the capture action. Check that xdg-desktop-portal-hyprland is running, then restart T3 Code."),
        (Desktop::Hyprland, Err(_)) => (false, "Couldn't connect to Hyprland shortcuts. Make sure xdg-desktop-portal-hyprland is running, then restart T3 Code."),
    }
}

fn spawn_poller(modifier: &str, generation: u64, waker: Option<Waker>) -> std::io::Result<Child> {
    let (left, right) = pair_masks(modifier);
    let mut child = Command::new(OSASCRIPT)
        .args(["-l", "JavaScript", "-e", POLLER_SCRIPT])
        .arg(left.to_string())
        .arg(right.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let stderr = child.stderr.take();
    std::thread::spawn(move || {
        // JXA's console.log writes to stderr.
        if let Some(stderr) = stderr {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                if line.trim() == "trigger" {
                    push(Event::Triggered(generation), &waker);
                }
            }
        }
        push(Event::PairExited(generation), &waker);
    });
    Ok(child)
}

fn apply(live: &mut Live, plan: Plan, waker: Option<Waker>) {
    release(live);
    live.generation += 1;
    let generation = live.generation;
    let state = &mut live.state;
    state.shortcut_verified = false;
    state.message = None;
    state.shortcut_registered = false;
    state.shortcut_message = None;
    state.shortcut_pending = false;
    state.shortcut_binding = None;
    state.shortcut_config_path = None;
    state.shortcut_action_registered = None;
    match &plan {
        Plan::Off => {}
        Plan::Managed(desktop) => {
            let desktop = *desktop;
            let (path, lua) = crate::desktop::capture_config::default_path(desktop);
            state.shortcut_binding = Some(crate::desktop::capture_config::manual_binding(desktop, crate::desktop::capture_linux::APP_ID, lua));
            state.shortcut_config_path = Some(path.display().to_string());
            state.shortcut_pending = true;
            if desktop == crate::desktop::capture_config::Desktop::Hyprland {
                state.shortcut_message = Some(HYPRLAND_CONNECTING.into());
            }
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            live.registration = Registration::Managed(stop.clone());
            std::thread::spawn(move || {
                let presses = waker.clone();
                let trigger: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(move || push(Event::Triggered(generation), &presses));
                crate::desktop::capture_linux::serve_shortcut(desktop, &stop, &|result| push(Event::Managed(generation, desktop, result), &waker), trigger);
            });
        }
        Plan::Blocked => state.message = Some(SCREEN_RECORDING_MESSAGE.into()),
        Plan::Invalid => state.shortcut_message = Some(SHORTCUT_TAKEN.into()),
        Plan::Pair(_) if linux() => live.state.shortcut_message = Some(crate::desktop::capture_linux::MODIFIER_PAIR_UNAVAILABLE.into()),
        Plan::Chord(accelerator) if linux() => {
            // Portal consent can take minutes; Rune registers Linux shortcuts off the UI thread.
            let (Some(app), Ok(shortcut)) = (live.app.clone(), accelerator.parse::<rune::Shortcut>()) else {
                live.state.shortcut_message = Some(SHORTCUT_TAKEN.into());
                return;
            };
            live.state.shortcut_pending = true;
            std::thread::spawn(move || {
                let result = app.register_shortcut(shortcut, move |s: &mut App, _: &mut Tx| triggered(s, generation));
                push(Event::Registered(generation, result), &waker);
            });
        }
        Plan::Pair(modifier) => match spawn_poller(modifier, generation, waker) {
            Ok(child) => {
                live.registration = Registration::Pair(child);
                live.state.shortcut_registered = true;
                live.state.shortcut_message = Some(format!("{} is observed and cannot be reserved exclusively.", pair_label(modifier)));
            }
            Err(_) => live.state.shortcut_message = Some(format!("{} is not available on this system.", pair_label(modifier))),
        },
        Plan::Chord(accelerator) => {
            let registered = live.app.as_ref().and_then(|app| {
                let shortcut = accelerator.parse::<rune::Shortcut>().ok()?;
                app.register_shortcut(shortcut, move |s: &mut App, _: &mut Tx| triggered(s, generation)).ok()
            });
            match registered {
                Some(id) => {
                    live.registration = Registration::Chord(id);
                    live.state.shortcut_registered = true;
                }
                None => live.state.shortcut_message = Some(SHORTCUT_TAKEN.into()),
            }
        }
    }
}

fn triggered(s: &mut App, generation: u64) {
    let current = LIVE.with(|live| {
        let mut live = live.borrow_mut();
        let current = live.generation == generation;
        if current {
            live.state.shortcut_verified = true;
        }
        current
    });
    if current {
        start(s);
    }
}

fn enabled(s: &App) -> bool {
    crate::settings::value(s, "snapShotEnabled") == true
}

/// Plays the chosen capture sound through the system player.
pub fn play_sound(sound: &str) {
    if !supported() {
        return;
    }
    let (name, bytes): (&str, &[u8]) = if sound == "camera-shutter" {
        ("snap-shot-click.mp3", include_bytes!("../../assets/snap-shot-click.mp3"))
    } else {
        ("snap-shot-whoosh.mp3", include_bytes!("../../assets/snap-shot-whoosh.mp3"))
    };
    let dir = std::env::temp_dir().join("s5code-rune-sounds");
    let path = dir.join(name);
    if std::fs::metadata(&path).map(|m| m.len()).ok() != Some(bytes.len() as u64) {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&path, bytes);
    }
    if linux() {
        crate::desktop::capture_linux::play(&path);
    } else if let Ok(mut child) = Command::new(AFPLAY).arg(&path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        std::thread::spawn(move || child.wait());
    }
}

/// `PortalCaptureShortcut`'s failures, as the shortcut row shows them.
pub fn registration_message(error: &rune::ShortcutError) -> &'static str {
    match error {
        rune::ShortcutError::Conflict => SHORTCUT_TAKEN,
        rune::ShortcutError::PermissionDenied => "Shortcut permission wasn't granted. Use Shortcut permissions to ask again.",
        rune::ShortcutError::InvalidAccelerator(_) => "This key isn't supported as a Wayland capture shortcut. Choose another key.",
        _ => "Could not connect to your desktop's shortcut service.",
    }
}

/// Checks the Linux backend and extension setup again.
pub fn refresh_linux(s: &App) {
    let started = LIVE.with(|live| {
        let mut live = live.borrow_mut();
        let start = linux() && !live.state.linux_busy;
        if start {
            live.state.linux_busy = true;
        }
        start
    });
    if started {
        let waker = s.ui.panel.waker.clone();
        std::thread::spawn(move || push(Event::LinuxChecked(crate::desktop::capture_linux::detect()), &waker));
    }
}

/// Installs, enables or disables the GNOME extension, then checks again.
pub fn linux_action(s: &App, action: crate::desktop::capture_linux::Action) {
    let started = LIVE.with(|live| {
        let mut live = live.borrow_mut();
        let start = linux() && !live.state.linux_busy;
        if start {
            live.state.linux_busy = true;
            live.state.linux_error = None;
        }
        start
    });
    if started {
        let waker = s.ui.panel.waker.clone();
        std::thread::spawn(move || push(Event::LinuxActed(crate::desktop::capture_linux::act(action)), &waker));
    }
}

/// Linux: the backend to capture with, once known.
fn linux_backend() -> Option<crate::desktop::capture_linux::Backend> {
    LIVE.with(|live| live.borrow().state.linux.as_ref().map(|setup| setup.backend))
}

/// The shortcut's capture: the sound when it starts, as web plays it on `started`.
pub fn start(s: &mut App) {
    if !supported() || !enabled(s) {
        return;
    }
    let busy = LIVE.with(|live| std::mem::replace(&mut live.borrow_mut().capturing, true));
    if busy {
        return;
    }
    let linux_backend = linux().then(linux_backend);
    if linux_backend == Some(None) {
        LIVE.with(|live| live.borrow_mut().capturing = false);
        refresh_linux(s);
        crate::shell::palette::toast_error(s, "Snapshot failed", Some("Still checking capture access. Try again in a moment."));
        return;
    }
    if crate::settings::value(s, "snapShotPlaySound") != false {
        play_sound(crate::settings::value(s, "snapShotSound").as_str().unwrap_or("soft-pop"));
    }
    let waker = s.ui.panel.waker.clone();
    let dir = crate::host::t3_home().join("userdata").join("snap-shots");
    if let Some(Some(backend)) = linux_backend {
        use crate::desktop::capture_linux as linux;
        let (composer, window) = LIVE.with(|live| (live.borrow().composer, live.borrow().window));
        let feedback = linux::Feedback {
            flash: crate::settings::value(s, "snapShotFlash") != false,
            animate: crate::settings::value(s, "snapShotAnimations") != false,
            relative: composer.zip(window).and_then(|(c, w)| relative_landing(c, w)),
        };
        let text = crate::settings::value(s, "snapShotIncludeAccessibility") != false && backend.knows_window();
        std::thread::spawn(move || {
            let path = dir.join(format!("window-{}.png", crate::shell::palette::add_project::iso_now().replace(':', "-")));
            let captured = std::fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|()| linux::capture(backend, &path, feedback));
            let event = match captured {
                Ok(captured) => {
                    let window = captured.window.unwrap_or_default();
                    let app_text = text.then(|| linux::app_text(&window)).flatten();
                    let source = snap_shot_source(&window.app, &window.title, app_text.as_deref());
                    let _ = std::fs::write(source_path(&path.to_string_lossy()), source.to_string());
                    let bounds = window.bounds.filter(|_| !captured.effects_done).map_or(Bounds { x: 0.0, y: 0.0, width: 0.0, height: 0.0 }, |[x, y, width, height]| Bounds { x, y, width, height });
                    let thumbnail = (feedback.animate && bounds.width > 0.0 && captured.own.is_some())
                        .then(|| image::open(&path).ok().map(flight_pixels))
                        .flatten();
                    let origin = captured.own.map_or((0.0, 0.0), |o| (o[0] as f32, o[1] as f32));
                    Event::Captured(Ok((Shot { bounds, source, thumbnail, origin, notice: captured.notice }, path)))
                }
                Err(message) => Event::CaptureFailed(message),
            };
            push(event, &waker);
        });
        return;
    }
    let accessibility = LIVE.with(|live| live.borrow().state.permissions.is_some_and(|p| p.accessibility));
    let extras = Extras {
        text: crate::settings::value(s, "snapShotIncludeAccessibility") != false && accessibility,
        thumbnail: crate::settings::value(s, "snapShotAnimations") != false,
    };
    std::thread::spawn(move || {
        let name = format!("window-{}.png", crate::shell::palette::add_project::iso_now().replace(':', "-"));
        let path = dir.join(name);
        let result = std::fs::create_dir_all(&dir).map_err(|_| (Step::CaptureWindow, Status::Failed)).and_then(|()| capture_front(&path, extras)).map(|shot| {
            // The attachment's `source` rides beside the picture until the draft sends it.
            let _ = std::fs::write(source_path(&path.to_string_lossy()), shot.source.to_string());
            (shot, path)
        });
        push(Event::Captured(result), &waker);
    });
}

/// The setup wizard's "Test capture": a capture that is discarded.
pub fn test(s: &mut App) {
    if !cfg!(target_os = "macos") {
        LIVE.with(|live| live.borrow_mut().state.test = Test::Failed("Test capture requires macOS.".into()));
        return;
    }
    LIVE.with(|live| live.borrow_mut().state.test = Test::Running);
    let waker = s.ui.panel.waker.clone();
    std::thread::spawn(move || {
        let dir = std::env::temp_dir().join(format!("s5code-snapshot-test-{}", std::process::id()));
        let path = dir.join("test.png");
        let result = std::fs::create_dir_all(&dir).map_err(|_| (Step::CaptureWindow, Status::Failed)).and_then(|()| capture_front(&path, Extras::default())).map(|_| ());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
        push(Event::Tested(result), &waker);
    });
}

/// Forgets the last test capture's result, for a wizard opened again.
pub fn reset_test() {
    LIVE.with(|live| live.borrow_mut().state.test = Test::Idle);
}

/// `retry-shortcut`: registers the saved shortcut again on the next poll.
pub fn retry() {
    LIVE.with(|live| live.borrow_mut().applied = None);
}

/// Rechecks permissions, prompting for Screen Recording and opening its settings pane on `request`.
pub fn check_permissions(s: &App, request: bool) {
    permissions(s, if request { "request" } else { "check" });
}

/// Prompts for Accessibility, which app text reads windows through, and opens its pane.
pub fn request_accessibility(s: &App) {
    permissions(s, "request-accessibility");
}

fn permissions(s: &App, mode: &'static str) {
    let started = LIVE.with(|live| {
        let mut live = live.borrow_mut();
        let start = cfg!(target_os = "macos") && !live.checking;
        if start {
            live.checking = true;
            live.checked_at = Some(Instant::now());
        }
        start
    });
    if !started {
        return;
    }
    let waker = s.ui.panel.waker.clone();
    std::thread::spawn(move || {
        let permissions = run(OSASCRIPT, &["-l", "JavaScript", "-e", PERMISSIONS_SCRIPT, mode], LOOKUP_TIMEOUT).ok().and_then(|out| parse_permissions(&out));
        if mode == "request" && permissions.is_some_and(|p| !p.screen_recording) {
            let _ = run(OPEN, &[SCREEN_RECORDING_SETTINGS_URL], LOOKUP_TIMEOUT);
        }
        if mode == "request-accessibility" && permissions.is_some_and(|p| !p.accessibility) {
            let _ = run(OPEN, &[ACCESSIBILITY_SETTINGS_URL], LOOKUP_TIMEOUT);
        }
        push(Event::Permissions(permissions), &waker);
    });
}

/// The permissions script's answer; an unknown grant counts as allowed so the test capture, not
/// a guess, reports the problem.
pub fn parse_permissions(output: &str) -> Option<Permissions> {
    let v: Value = serde_json::from_str(output.trim()).ok()?;
    Some(Permissions { screen_recording: v["screenRecording"].as_bool().unwrap_or(true), accessibility: v["accessibility"].as_bool().unwrap_or(true) })
}

/// Where a capture lands: web's last thread or draft on screen, which needs a project.
fn deliver(s: &mut App, path: PathBuf) {
    let needs_project = !matches!(s.frame_route(), Route::Thread(_) | Route::Home { project_id: Some(_) });
    if needs_project && crate::chat::home::default_project(s).is_none() {
        crate::shell::palette::toast_error(s, "Snapshot taken, but no project is available", Some("Add a project, then capture the window again."));
        return;
    }
    crate::chat::composer::attach_path(s, path.display().to_string());
}

/// Drains capture events and keeps the shortcut registration in step with the settings.
pub fn poll(s: &mut App, tx: &mut Tx) -> bool {
    if !supported() {
        return false;
    }
    let mut changed = false;
    let events = EVENTS.lock().map(|mut e| std::mem::take(&mut *e)).unwrap_or_default();
    for event in events {
        changed = true;
        match event {
            Event::Triggered(generation) => triggered(s, generation),
            Event::PairExited(generation) => LIVE.with(|live| {
                let mut live = live.borrow_mut();
                if live.generation == generation && matches!(live.registration, Registration::Pair(_)) {
                    release(&mut live);
                    live.state.shortcut_registered = false;
                    let modifier = pair_modifier(&crate::settings::value(s, "snapShotShortcut")).unwrap_or("shift");
                    live.state.shortcut_message = Some(format!("{} is not available on this system.", pair_label(modifier)));
                }
            }),
            Event::Captured(result) => {
                LIVE.with(|live| live.borrow_mut().capturing = false);
                if !enabled(s) {
                    continue;
                }
                match result {
                    Ok((shot, path)) => {
                        // Linux without a window frame, or with the compositor's own effects, has none to draw.
                        let framed = shot.bounds.width > 0.0 && shot.bounds.height > 0.0;
                        if framed && crate::settings::value(s, "snapShotFlash") != false {
                            flash(shot.bounds);
                        }
                        if let Some(pixels) = shot.thumbnail.filter(|_| framed && crate::settings::value(s, "snapShotAnimations") != false) {
                            fly(shot.bounds, pixels, shot.origin);
                        }
                        if let Some(notice) = &shot.notice {
                            crate::shell::palette::toast(s, "Snapshot taken with the window picker", Some(notice.as_str()));
                        }
                        deliver(s, path);
                    }
                    Err((_, status)) => crate::shell::palette::toast_error(s, "Snapshot failed", Some(capture_message(status))),
                }
            }
            Event::CaptureFailed(message) => {
                LIVE.with(|live| live.borrow_mut().capturing = false);
                crate::shell::palette::toast_error(s, "Snapshot failed", Some(&message));
            }
            Event::LinuxChecked(result) => LIVE.with(|live| {
                let mut live = live.borrow_mut();
                live.state.linux_busy = false;
                match result {
                    Ok(setup) => {
                        live.state.linux = Some(setup);
                        live.state.message = None;
                    }
                    Err(message) => live.state.message = Some(message),
                }
            }),
            Event::LinuxActed(result) => {
                LIVE.with(|live| {
                    let mut live = live.borrow_mut();
                    live.state.linux_busy = false;
                    live.state.linux_error = result.err();
                });
                refresh_linux(s);
            }
            Event::Registered(generation, result) => LIVE.with(|live| {
                let mut live = live.borrow_mut();
                if live.generation != generation {
                    // The settings moved on while the portal asked; drop the stale binding.
                    if let (Ok(id), Some(app)) = (result, &live.app) {
                        let _ = app.unregister_shortcut(id);
                    }
                    return;
                }
                live.state.shortcut_pending = false;
                match result {
                    Ok(id) => {
                        live.registration = Registration::Chord(id);
                        live.state.shortcut_registered = true;
                    }
                    Err(error) => live.state.shortcut_message = Some(registration_message(&error).into()),
                }
            }),
            Event::Managed(generation, desktop, result) => LIVE.with(|live| {
                let mut live = live.borrow_mut();
                if live.generation != generation {
                    return;
                }
                let (registered, message) = managed_status(desktop, &result);
                live.state.shortcut_pending = false;
                live.state.shortcut_action_registered = Some(registered);
                live.state.shortcut_message = Some(message.into());
            }),
            Event::Tested(result) => {
                let passed = result.is_ok();
                LIVE.with(|live| live.borrow_mut().state.test = result.map_or_else(|(step, status)| Test::Failed(test_message(step, status)), |()| Test::Passed));
                crate::settings::snapshots::tested(s, passed);
            }
            Event::Permissions(permissions) => LIVE.with(|live| {
                let mut live = live.borrow_mut();
                live.checking = false;
                live.state.permissions = Some(permissions.unwrap_or(Permissions { screen_recording: true, accessibility: true }));
            }),
        }
    }
    let wizard = crate::settings::snapshots::checking_access(s) && !linux();
    let on_page = matches!(&s.route, Route::Settings(p) if p == "snap-shot");
    let (unknown, due) = LIVE.with(|live| {
        let live = live.borrow();
        (live.state.permissions.is_none(), live.checked_at.is_none_or(|at| at.elapsed() >= PERMISSION_POLL))
    });
    if linux() {
        // Linux has no permission prompts to check, only the capture backend.
        let (unknown_setup, busy) = LIVE.with(|live| {
            let mut live = live.borrow_mut();
            live.state.permissions.get_or_insert(Permissions { screen_recording: true, accessibility: true });
            (live.state.linux.is_none() && live.state.message.is_none(), live.state.linux_busy)
        });
        if unknown_setup && !busy && (enabled(s) || on_page) {
            refresh_linux(s);
        }
    } else if (unknown && (enabled(s) || on_page)) || (wizard && due) {
        check_permissions(s, false);
    }
    let shortcut = crate::settings::value(s, "snapShotShortcut");
    let suppressed = crate::settings::snapshots::recording(s);
    let waker = s.ui.panel.waker.clone();
    changed |= LIVE.with(|live| {
        let mut live = live.borrow_mut();
        let mut wanted = plan(enabled(s), live.state.permissions, suppressed, &shortcut);
        if linux() {
            let checking = live.state.linux.is_none() && live.state.message.is_none();
            wanted = linux_plan(wanted, live.state.linux.as_ref().map(|l| l.backend), checking);
        }
        // A pair that died stays released until the settings change.
        if live.applied.as_ref() == Some(&wanted) {
            return false;
        }
        live.applied = Some(wanted.clone());
        apply(&mut live, wanted, waker);
        true
    });
    let fading = LIVE.with(|live| {
        let mut live = live.borrow_mut();
        let Some(shown) = live.flash_shown else { return false };
        if shown.elapsed() >= Duration::from_millis(16) {
            live.flash_shown = None;
            if let Some(flash) = &live.flash {
                flash.fade_out(FLASH_DURATION);
            }
            return false;
        }
        true
    });
    let flying = step_flight();
    match (fading, flying, wizard) {
        (true, ..) => tx.wake_after(Duration::from_millis(16)),
        (_, Some(next), _) => tx.wake_after(next),
        (_, None, true) => tx.wake_after(PERMISSION_POLL),
        _ => {}
    }
    changed
}

/// `SnapShotTransition`: shows the picture over the captured window, then on the next poll moves
/// it to the composer on Electron's curve; the overlay hides once it lands. One flight, no loop.
fn fly(bounds: Bounds, pixels: rune::Pixels, origin: (f32, f32)) {
    LIVE.with(|live| {
        let mut live = live.borrow_mut();
        let (Some(flight), Some(composer)) = (live.flight.clone(), live.composer) else { return };
        let composer = [composer[0] + origin.0, composer[1] + origin.1, composer[2], composer[3]];
        let source = [bounds.x as f32, bounds.y as f32, bounds.width as f32, bounds.height as f32];
        let target = flight_target(composer, bounds);
        let (frame, from, to) = flight_frames(source, target);
        flight.set_placement(Some(rune::OverlayPlacement::Rect { x: frame[0], y: frame[1], width: frame[2], height: frame[3] }));
        flight.update(move |f, tx| {
            f.pixels = pixels;
            f.card = from;
            tx.instant();
        });
        flight.show();
        live.flight_stage = Some(FlightStage::Launch { shown: Instant::now(), to, seconds: flight_seconds(source, target) });
    });
}

/// Advances the flight; returns when to look again while it runs.
fn step_flight() -> Option<Duration> {
    LIVE.with(|live| {
        let mut live = live.borrow_mut();
        let flight = live.flight.clone()?;
        match live.flight_stage.take()? {
            FlightStage::Launch { shown, to, seconds } if shown.elapsed() >= Duration::from_millis(32) => {
                let (x1, y1, x2, y2) = FLIGHT_EASING;
                flight.update(move |f, tx| {
                    f.card = to;
                    tx.motion_curve(rune::Curve::tween(seconds, rune::Easing::cubic(x1, y1, x2, y2)));
                });
                let duration = Duration::from_secs_f32(seconds);
                live.flight_stage = Some(FlightStage::Flying { until: Instant::now() + duration });
                Some(duration)
            }
            stage @ FlightStage::Launch { .. } => {
                live.flight_stage = Some(stage);
                Some(Duration::from_millis(16))
            }
            FlightStage::Flying { until } if Instant::now() < until => {
                live.flight_stage = Some(FlightStage::Flying { until });
                Some(until - Instant::now())
            }
            FlightStage::Flying { .. } => {
                flight.fade_out(FLIGHT_FADE);
                None
            }
        }
    })
}

/// Electron's `SnapShotFlash`: a faint white panel over the captured window that fades once.
fn flash(bounds: Bounds) {
    LIVE.with(|live| {
        let mut live = live.borrow_mut();
        let Some(flash) = live.flash.clone() else { return };
        flash.set_placement(Some(rune::OverlayPlacement::Rect { x: bounds.x as f32, y: bounds.y as f32, width: bounds.width as f32, height: bounds.height as f32 }));
        flash.show();
        live.flash_shown = Some(Instant::now());
    });
}

/// The settings' JSON for a recorded pair.
pub fn pair_shortcut(modifier: &str) -> Value {
    if modifier == "shift" { json!({"kind": "both-shift-keys"}) } else { json!({"kind": "modifier-pair", "modifier": modifier}) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn niri_and_hyprland_serve_their_config_binding() {
        use crate::desktop::{capture_config::Desktop, capture_linux::Backend};
        let chord = || Plan::Chord("Ctrl+Shift+2".into());
        assert_eq!(linux_plan(chord(), Some(Backend::Niri), false), Plan::Managed(Desktop::Niri));
        assert_eq!(linux_plan(Plan::Pair("shift"), Some(Backend::Hyprland), false), Plan::Managed(Desktop::Hyprland), "the settings' keys don't matter");
        assert_eq!(linux_plan(Plan::Off, Some(Backend::Niri), false), Plan::Off, "capture off stops serving");
        assert_eq!(linux_plan(Plan::Blocked, Some(Backend::Hyprland), false), Plan::Blocked);
        assert_eq!(linux_plan(chord(), None, true), Plan::Off, "nothing registers until the backend is known");
        assert_eq!(linux_plan(chord(), Some(Backend::Kde), false), chord(), "other desktops use the portal");
        assert_eq!(linux_plan(chord(), None, false), chord(), "a failed check keeps the portal");

        assert_eq!(managed_status(Desktop::Hyprland, &Ok(true)), (true, "Managed by Hyprland. Add the binding to your config and save it."));
        assert!(!managed_status(Desktop::Hyprland, &Ok(false)).0, "Hyprland refused the action");
        assert!(managed_status(Desktop::Hyprland, &Err("x".into())).1.starts_with("Couldn't connect to Hyprland shortcuts."));
        assert_eq!(managed_status(Desktop::Niri, &Err("taken".into())), (false, "Could not start the Niri capture endpoint. Another T3 Code instance may be using it."));
    }

    fn window(id: u64, title: &str, (x, y, width, height): (f64, f64, f64, f64)) -> Candidate {
        Candidate { id, title: title.into(), bounds: Bounds { x, y, width, height }, alpha: 1.0 }
    }

    #[test]
    fn parses_the_lookup_like_electron() {
        let output = r#"{"owner":{"name":"Safari","processId":42},"windows":[{"id":7,"title":"Docs","bounds":{"x":0,"y":25,"width":1200,"height":800},"alpha":1}]}"#;
        let front = parse_lookup(output).unwrap().unwrap();
        assert_eq!((front.owner.as_str(), front.pid), ("Safari", 42));
        assert_eq!(front.windows, [window(7, "Docs", (0., 25., 1200., 800.))]);
        assert_eq!(parse_lookup("  \n"), Ok(None), "no frontmost app");
        assert_eq!(parse_lookup("{not json"), Err(Status::Failed));
        assert_eq!(parse_lookup(r#"{"owner":{"name":"A"},"windows":[{"id":"7"}]}"#), Err(Status::Failed), "the schema rejects wrong types");
    }

    #[test]
    fn chooses_the_front_window_past_helper_strips() {
        let content = window(2, "", (0., 40., 1000., 800.));
        let toolbar = window(1, "", (0., 0., 1000., 40.));
        assert_eq!(choose(&[toolbar.clone(), content.clone()]), Some(&content), "an untitled strip above a later window is skipped");
        let titled = window(1, "Inspector", (0., 0., 1000., 40.));
        assert_eq!(choose(&[titled.clone(), content.clone()]), Some(&titled), "a titled window is never a helper");
        let mut hidden = window(3, "Hidden", (0., 0., 500., 500.));
        hidden.alpha = 0.0;
        let empty = window(4, "Empty", (0., 0., 0., 500.));
        assert_eq!(choose(&[hidden, empty, content.clone()]), Some(&content), "invisible and empty windows are skipped");
        assert_eq!(choose(&[toolbar.clone()]), Some(&toolbar), "a strip with nothing behind it is the window");
        assert_eq!(choose(&[]), None);
    }

    #[test]
    fn scales_captures_to_the_window_points() {
        let bounds = Bounds { x: 0., y: 0., width: 1000., height: 500. };
        assert_eq!(fitted((2000, 1000), bounds), Some((1000, 500)), "Retina pixels shrink to points");
        assert_eq!(fitted((800, 400), bounds), None);
        let huge = Bounds { x: 0., y: 0., width: 5120., height: 1600. };
        assert_eq!(fitted((5120, 1600), huge), Some((2560, 800)), "capped at 2560×1600");
        assert_eq!(png_size(&[0; 30]), None, "not a PNG");
    }

    #[test]
    fn maps_failures_like_electron() {
        assert_eq!(capture_message(Status::Missing), "The active window is not available for capture.");
        assert_eq!(capture_message(Status::Exited(1)), "Could not capture the active window.");
        assert_eq!(test_message(Step::FindWindow, Status::Missing), "Could not capture a test snapshot. No window is available to capture.");
        let exited = test_message(Step::CaptureWindow, Status::Exited(1));
        assert!(exited.starts_with("Could not capture a test snapshot. Capturing the window failed: screencapture exited with code 1. macOS grants"), "{exited}");
        assert!(exited.ends_with("check that \"S5 Code\" is allowed for both."));
        assert!(test_message(Step::FindWindow, Status::TimedOut).contains("Finding the front window timed out."));
        assert!(test_message(Step::FindWindow, Status::Failed).contains("Finding the front window failed. macOS"));
        assert_eq!(parse_permissions(r#"{"screenRecording":false,"accessibility":false}"#), Some(Permissions { screen_recording: false, accessibility: false }));
        assert_eq!(parse_permissions(r#"{"screenRecording":null}"#), Some(Permissions { screen_recording: true, accessibility: true }), "unknown is left to the test capture");
        assert_eq!(parse_permissions(""), None);
    }

    #[test]
    fn settings_choose_the_registration() {
        let granted = Some(Permissions { screen_recording: true, accessibility: false });
        let pair = json!({"kind": "both-shift-keys"});
        let chord = json!({"key": "2", "modKey": true, "shiftKey": true, "ctrlKey": false, "altKey": false, "metaKey": false});
        assert_eq!(plan(false, granted, false, &pair), Plan::Off, "disabled");
        assert_eq!(plan(true, granted, true, &pair), Plan::Off, "recording a new shortcut suppresses it");
        assert_eq!(plan(true, None, false, &pair), Plan::Off, "waits for the permission check");
        assert_eq!(plan(true, Some(Permissions::default()), false, &pair), Plan::Blocked);
        assert_eq!(plan(true, granted, false, &pair), Plan::Pair("shift"));
        assert_eq!(plan(true, granted, false, &json!({"kind": "modifier-pair", "modifier": "alt"})), Plan::Pair("alt"));
        assert_eq!(plan(true, granted, false, &chord), Plan::Chord("shift+cmd+2".into()));
        assert_eq!(plan(true, granted, false, &json!({"key": "k"})), Plan::Invalid, "a chord needs a modifier");
        assert_eq!(accelerator(&json!({"key": " ", "ctrlKey": true, "altKey": true})).as_deref(), Some("ctrl+alt+space"));
        assert!("shift+cmd+2".parse::<rune::Shortcut>().is_ok(), "Rune parses the accelerator");
        assert_eq!(pair_label_for("meta", true), "Command + Command");
        assert_eq!(pair_label_for("control", false), "Ctrl + Ctrl");
        assert_eq!(pair_shortcut("shift"), pair);
    }

    #[test]
    fn reads_app_text_like_electron() {
        let values: Vec<String> = ["Docs", "", " Docs ", "Search\0", "Search", "Body"].map(String::from).into();
        assert_eq!(accessible_text(&values, 100), "Docs\nSearch\nBody", "trimmed, NULs dropped, each once, one per line");
        assert_eq!(accessible_text(&values, 8), "Docs\nSea", "the cap cuts the last value");
        assert_eq!(accessible_text(&values, 5), "Docs", "no separator without text after it");
        let emoji = vec!["ab".to_owned(), "😀x".to_owned()];
        assert_eq!(accessible_text(&emoji, 4), "ab", "a character is never split");
        let source = snap_shot_source("  Safari ", "Docs", Some(" Body "));
        assert_eq!((source["kind"].as_str(), source["appName"].as_str(), source["windowTitle"].as_str(), source["accessibleText"].as_str()), (Some("snap-shot"), Some("Safari"), Some("Docs"), Some("Body")));
        assert!(source["capturedAt"].is_string());
        let bare = snap_shot_source("", "", Some("  "));
        assert_eq!(bare["appName"], "Window", "an unnamed app reads as Window");
        assert!(bare.get("accessibleText").is_none());
    }

    #[test]
    fn flies_like_the_electron_transition() {
        let near = flight_seconds([0., 0., 100., 100.], [0., 0., 100., 100.]);
        assert!((near - 0.28).abs() < 1e-4, "{near}");
        let far = flight_seconds([0., 0., 100., 100.], [20_000., 0., 100., 100.]);
        assert!((far - 0.68).abs() < 1e-3, "{far}");
        let target = flight_target([100., 600., 700., 120.], Bounds { x: 0., y: 0., width: 1600., height: 1000. });
        assert_eq!([target[0], target[1], target[3]], [113., 613., 56.], "lands on the attachment row at thumbnail height");
        assert!((target[2] - 89.6).abs() < 1e-3, "keeps the window's aspect: {}", target[2]);
        let (frame, from, to) = flight_frames([500., 100., 400., 300.], [113., 613., 90., 56.]);
        assert_eq!(frame, [113., 100., 787., 569.]);
        assert_eq!((from, to), ([387., 0., 400., 300.], [0., 513., 90., 56.]));
    }

    #[test]
    fn flags_shortcuts_the_system_owns() {
        let chord = |key: &str, flag: &str| json!({"key": key, flag: true});
        assert_eq!(system_conflict(&chord("c", "modKey")).as_deref(), Some("This shortcut is Copy in most apps."));
        assert_eq!(system_conflict(&chord("2", "modKey")), None);
        assert!(system_conflict(&chord("a", "shiftKey")).unwrap().starts_with("Shift combinations"));
        assert_eq!(system_conflict(&chord("z", "ctrlKey")).as_deref(), Some("This shortcut controls running commands in terminals."));
        assert_eq!(system_conflict(&json!({"key": "c", "modKey": true, "shiftKey": true})), None, "two modifiers are fine");
        assert_eq!(availability(&chord("v", "modKey")), Err("This shortcut is Paste in most apps.".into()));
        assert_eq!(availability(&json!({"kind": "both-shift-keys"})), Ok(()));
    }
}
