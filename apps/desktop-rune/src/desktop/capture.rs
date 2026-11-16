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
function run(argv) {
  let screen = null;
  try {
    if (argv[0] === "request" && !$.CGPreflightScreenCaptureAccess()) $.CGRequestScreenCaptureAccess();
    screen = Boolean($.CGPreflightScreenCaptureAccess());
  } catch (e) {}
  return JSON.stringify({ screenRecording: screen });
}"#;

pub fn supported() -> bool {
    cfg!(target_os = "macos")
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
}

/// The registration the settings ask for.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    Off,
    Blocked,
    Pair(&'static str),
    Chord(String),
    Invalid,
}

enum Event {
    Triggered(u64),
    PairExited(u64),
    Captured(Result<(Bounds, PathBuf), (Step, Status)>),
    Tested(Result<(), (Step, Status)>),
    Permissions(Option<Permissions>),
}

#[derive(Default)]
enum Registration {
    #[default]
    None,
    Chord(rune::ShortcutId),
    Pair(Child),
}

#[derive(Default)]
struct Live {
    app: Option<rune::AppHandle<App>>,
    flash: Option<rune::OverlayHandle<()>>,
    flash_shown: Option<Instant>,
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
    let flash = supported().then(|| {
        let spec = rune::OverlaySpec { name: "SnapShot flash", size: (1.0, 1.0), edge: rune::ScreenEdge::Top, margin: 0.0, align: rune::Align::Center, inset: 0.0, interactive: false, view: flash_view };
        let options = rune::OverlayOptions { input: rune::OverlayInput::None, exclude_from_capture: true, ..Default::default() };
        app.overlay(spec.options(options), ())
    });
    LIVE.with(|live| {
        let mut live = live.borrow_mut();
        live.app = Some(handle);
        live.flash = flash;
    });
}

fn flash_view(_: &(), _: &mut rune::Cx<()>) -> El {
    rune::el().w_full().h_full().fill(rune::Color::WHITE.alpha(FLASH_PEAK_OPACITY))
}

pub fn state() -> State {
    LIVE.with(|live| State { supported: supported(), ..live.borrow().state.clone() })
}

/// `decodeMacActiveWindowLookup`: the front app's name and its layer-0 windows, front to back.
pub fn parse_lookup(output: &str) -> Result<Option<(String, Vec<Candidate>)>, Status> {
    let output = output.trim();
    if output.is_empty() {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(output).map_err(|_| Status::Failed)?;
    let owner = v["owner"]["name"].as_str().ok_or(Status::Failed)?.to_owned();
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
    Ok(Some((owner, windows)))
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

/// Finds and captures the front window into `path`, scaled down as Electron stores it.
fn capture_front(path: &Path) -> Result<Bounds, (Step, Status)> {
    let find = |status| (Step::FindWindow, status);
    let failed = (Step::CaptureWindow, Status::Failed);
    let output = run(OSASCRIPT, &["-l", "JavaScript", "-e", LOOKUP_SCRIPT], LOOKUP_TIMEOUT).map_err(find)?;
    let (_, windows) = parse_lookup(&output).map_err(find)?.ok_or(find(Status::Missing))?;
    let window = choose(&windows).ok_or(find(Status::Missing))?;
    let (id, target) = (window.id.to_string(), path.to_string_lossy());
    run(SCREENCAPTURE, &["-l", id.as_str(), "-o", "-x", "-t", "png", &*target], CAPTURE_TIMEOUT).map_err(|status| (Step::CaptureWindow, status))?;
    let bytes = std::fs::read(path).map_err(|_| failed)?;
    let size = png_size(&bytes).ok_or(failed)?;
    if let Some((w, h)) = fitted(size, window.bounds) {
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).map_err(|_| failed)?;
        image.resize_exact(w, h, image::imageops::FilterType::Lanczos3).save_with_format(path, image::ImageFormat::Png).map_err(|_| failed)?;
    }
    Ok(window.bounds)
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
    let label = match modifier {
        "meta" => "Command",
        "control" => "Control",
        "alt" => "Option",
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
        let Some(app) = live.app.as_ref().filter(|_| supported()) else { return Ok(()) };
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
        Registration::None => {}
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
    match &plan {
        Plan::Off => {}
        Plan::Blocked => state.message = Some(SCREEN_RECORDING_MESSAGE.into()),
        Plan::Invalid => state.shortcut_message = Some(SHORTCUT_TAKEN.into()),
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
    if let Ok(mut child) = Command::new(AFPLAY).arg(&path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        std::thread::spawn(move || child.wait());
    }
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
    if crate::settings::value(s, "snapShotPlaySound") != false {
        play_sound(crate::settings::value(s, "snapShotSound").as_str().unwrap_or("soft-pop"));
    }
    let waker = s.ui.panel.waker.clone();
    let dir = crate::host::t3_home().join("userdata").join("snap-shots");
    std::thread::spawn(move || {
        let name = format!("window-{}.png", crate::shell::palette::add_project::iso_now().replace(':', "-"));
        let path = dir.join(name);
        let result = std::fs::create_dir_all(&dir).map_err(|_| (Step::CaptureWindow, Status::Failed)).and_then(|()| capture_front(&path)).map(|bounds| (bounds, path));
        push(Event::Captured(result), &waker);
    });
}

/// The setup wizard's "Test capture": a capture that is discarded.
pub fn test(s: &mut App) {
    if !supported() {
        LIVE.with(|live| live.borrow_mut().state.test = Test::Failed("Test capture requires macOS.".into()));
        return;
    }
    LIVE.with(|live| live.borrow_mut().state.test = Test::Running);
    let waker = s.ui.panel.waker.clone();
    std::thread::spawn(move || {
        let dir = std::env::temp_dir().join(format!("s5code-snapshot-test-{}", std::process::id()));
        let path = dir.join("test.png");
        let result = std::fs::create_dir_all(&dir).map_err(|_| (Step::CaptureWindow, Status::Failed)).and_then(|()| capture_front(&path)).map(|_| ());
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
    let started = LIVE.with(|live| {
        let mut live = live.borrow_mut();
        let start = supported() && !live.checking;
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
        let mode = if request { "request" } else { "check" };
        let permissions = run(OSASCRIPT, &["-l", "JavaScript", "-e", PERMISSIONS_SCRIPT, mode], LOOKUP_TIMEOUT).ok().and_then(|out| parse_permissions(&out));
        if request && permissions.is_some_and(|p| !p.screen_recording) {
            let _ = run(OPEN, &[SCREEN_RECORDING_SETTINGS_URL], LOOKUP_TIMEOUT);
        }
        push(Event::Permissions(permissions), &waker);
    });
}

/// The permissions script's answer; an unknown Screen Recording grant counts as allowed so the
/// test capture, not a guess, reports the problem.
pub fn parse_permissions(output: &str) -> Option<Permissions> {
    let v: Value = serde_json::from_str(output.trim()).ok()?;
    Some(Permissions { screen_recording: v["screenRecording"].as_bool().unwrap_or(true) })
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
                    Ok((bounds, path)) => {
                        if crate::settings::value(s, "snapShotFlash") != false {
                            flash(bounds);
                        }
                        deliver(s, path);
                    }
                    Err((_, status)) => crate::shell::palette::toast_error(s, "Snapshot failed", Some(capture_message(status))),
                }
            }
            Event::Tested(result) => {
                let passed = result.is_ok();
                LIVE.with(|live| live.borrow_mut().state.test = result.map_or_else(|(step, status)| Test::Failed(test_message(step, status)), |()| Test::Passed));
                crate::settings::snapshots::tested(s, passed);
            }
            Event::Permissions(permissions) => LIVE.with(|live| {
                let mut live = live.borrow_mut();
                live.checking = false;
                live.state.permissions = Some(permissions.unwrap_or(Permissions { screen_recording: true }));
            }),
        }
    }
    let wizard = crate::settings::snapshots::checking_access(s);
    let on_page = matches!(&s.route, Route::Settings(p) if p == "snap-shot");
    let (unknown, due) = LIVE.with(|live| {
        let live = live.borrow();
        (live.state.permissions.is_none(), live.checked_at.is_none_or(|at| at.elapsed() >= PERMISSION_POLL))
    });
    if (unknown && (enabled(s) || on_page)) || (wizard && due) {
        check_permissions(s, false);
    }
    let shortcut = crate::settings::value(s, "snapShotShortcut");
    let suppressed = crate::settings::snapshots::recording(s);
    let waker = s.ui.panel.waker.clone();
    changed |= LIVE.with(|live| {
        let mut live = live.borrow_mut();
        let wanted = plan(enabled(s), live.state.permissions, suppressed, &shortcut);
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
    if fading || wizard {
        tx.wake_after(if fading { Duration::from_millis(16) } else { PERMISSION_POLL });
    }
    changed
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

    fn window(id: u64, title: &str, (x, y, width, height): (f64, f64, f64, f64)) -> Candidate {
        Candidate { id, title: title.into(), bounds: Bounds { x, y, width, height }, alpha: 1.0 }
    }

    #[test]
    fn parses_the_lookup_like_electron() {
        let output = r#"{"owner":{"name":"Safari","processId":42},"windows":[{"id":7,"title":"Docs","bounds":{"x":0,"y":25,"width":1200,"height":800},"alpha":1}]}"#;
        let (owner, windows) = parse_lookup(output).unwrap().unwrap();
        assert_eq!(owner, "Safari");
        assert_eq!(windows, [window(7, "Docs", (0., 25., 1200., 800.))]);
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
        assert_eq!(parse_permissions(r#"{"screenRecording":false}"#), Some(Permissions { screen_recording: false }));
        assert_eq!(parse_permissions(r#"{"screenRecording":null}"#), Some(Permissions { screen_recording: true }), "unknown is left to the test capture");
        assert_eq!(parse_permissions(""), None);
    }

    #[test]
    fn settings_choose_the_registration() {
        let granted = Some(Permissions { screen_recording: true });
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
        assert_eq!(pair_label("meta"), "Command + Command");
        assert_eq!(pair_shortcut("shift"), pair);
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
