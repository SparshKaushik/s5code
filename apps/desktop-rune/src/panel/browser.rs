//! The browser preview: a headless Chrome driven over the DevTools protocol. Chrome renders each
//! tab; the shown tab's screencast frames land in its `Pixels`, and the panel's pointer and keys
//! go back as CDP input events. Rune never embeds a web view, so this is the whole bridge.
//!
//! One browser-level connection carries every tab as a flattened target session. Chrome rejects
//! request ids outside int32, so replies are matched through a map, not tagged ids.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use rune::{Pixels, Waker};
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

pub type TabId = u32;

pub enum Cmd {
    Open(TabId, Pixels),
    /// Shows a page Chrome already opened (a popup) as this tab.
    Adopt(TabId, Pixels, String),
    Close(TabId),
    /// Screencast this tab, or none while the panel hides it; the others send no frames.
    Show(Option<TabId>),
    Navigate(TabId, String),
    Back(TabId),
    Forward(TabId),
    Reload { tab: TabId, hard: bool },
    Stop(TabId),
    /// The page area in window points and the window's backing scale. Chrome renders at
    /// `w * scale` device pixels so frames land 1:1 on a Retina screen.
    Viewport { w: f32, h: f32, scale: f32 },
    Zoom(TabId, f32),
    /// An emulated CSS viewport (the device toolbar), or none to fill the page area.
    Device(TabId, Option<(u32, u32)>),
    /// Element picking: Chrome highlights what the pointer is over, a click picks it.
    Pick(TabId, bool),
    DevTools(TabId),
    /// A tab's `prefers-color-scheme`: "" follows Chrome's default.
    ColorScheme(TabId, &'static str),
    Mouse { kind: &'static str, x: f32, y: f32, clicks: u32 },
    /// The button and CDP modifier bits (Alt 1, Ctrl 2, Meta 4, Shift 8) of the press that
    /// follows, so a middle or modified click opens its link as Chrome decides.
    Pointer { button: &'static str, modifiers: u32 },
    Wheel { x: f32, y: f32, dx: f32, dy: f32 },
    Text(String),
    Key(&'static str),
    Screenshot(TabId, PathBuf),
    ClearCookies,
    ClearCache,
    /// Imported cookies (CDP `CookieParam`s, each with its bare host) for the Default profile.
    SetCookies(Vec<(Value, String)>, super::browser_import::Job),
    /// Records the tab's frames into a video, as web's Shift-click on the camera.
    /// `mouse` and `keys` draw presses into it, as web's recording settings do.
    Record { tab: TabId, fps: u32, mouse: bool, keys: bool },
    /// Ends the recording and saves it at this path, its extension the encoder's format.
    StopRecording(PathBuf),
    /// Follows the picked element so its box and comment editor move with the page.
    Track(Option<TabId>),
    /// Answers the page's open `alert`, `confirm` or `prompt`.
    Dialog { tab: TabId, accept: bool, text: String },
    /// What is under a right-click at a page point; `at` is the menu's window point.
    Inspect { x: f32, y: f32, at: (f32, f32) },
    /// The selection, as `Event::Clipboard`; `cut` deletes it after.
    CopySelection { cut: bool },
    /// The PNG of a document-pixel rect, as `Event::Image`.
    CopyImage(Value),
    /// `document.execCommand` on the shown page (`selectAll`, `undo`, `redo`).
    Edit(&'static str),
    /// Silences the tab's media, across its navigations, as Electron's `setAudioMuted`.
    Mute(TabId, bool),
    /// An annotation's marks drawn into the page (`MARKS_JS`), so they scroll with it and show in
    /// its screenshot: one call on `window.__s5Marks`, such as `region(10, 20, 30, 40)`.
    Mark(TabId, String),
    /// Removes the topmost mark under a viewport point, as `Event::Erased` with its index.
    Erase(TabId, f32, f32),
    /// The PNG of every mark's union, as `Event::Marked`.
    CaptureMarks(TabId),
    /// The elements a region drag encloses, in viewport CSS pixels, as `Event::Enclosed`.
    Enclosed(TabId, [f32; 4]),
    /// Answers the page's `navigator.clipboard.readText()`: the text, or None to refuse it.
    ClipboardAnswer(TabId, u64, Option<String>),
    /// Captures a page once, off screen, as an `html_render` frame shows it inline.
    Snapshot(Shot),
    /// Shows a page live inline (`Live`), until `LiveClose`.
    LiveOpen(Live),
    LiveInput(u64, LiveInput),
    LiveResize(u64, u32, u32),
    /// Runs a script in the live page, such as a host's reply posted to its app.
    LiveEval(u64, String),
    LiveClose(u64),
    /// Stops this profile's Chrome; `wipe` deletes the profile's data after.
    Quit { wipe: bool },
}

/// One capture of a page `width` CSS pixels wide, in a browser context of its own that goes
/// when the shot is taken: its PNG lands in `pixels`, its CSS height in `done`.
#[derive(Clone)]
pub struct Shot {
    pub url: String,
    pub width: u32,
    pub dark: bool,
    pub pixels: Pixels,
    /// Trim the right and bottom margins of the page's top-left colour, the reported height
    /// then being the trimmed one; a shot with nothing on it fails.
    pub crop: bool,
    done: Arc<Mutex<Option<Option<f32>>>>,
}

impl Shot {
    pub fn new(url: String, width: u32, dark: bool) -> Shot {
        Shot { url, width, dark, pixels: Pixels::new(), crop: false, done: Arc::default() }
    }

    /// The captured height once finished, or `Some(None)` when it failed or its Chrome went
    /// away without taking it.
    pub fn result(&self) -> Option<Option<f32>> {
        let done = *self.done.lock().unwrap();
        done.or_else(|| (Arc::strong_count(&self.done) == 1).then_some(None))
    }

    fn finish(&self, height: Option<f32>) {
        *self.done.lock().unwrap() = Some(height);
    }
}

/// What Electron's `previewWindowOpenAction` does with a page's new window.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Opened {
    Popup,
    Tab,
    Navigate,
    Deny,
}

/// Electron's `IN_PLACE_DENIED_PROTOCOLS`: never loaded from a window open.
const DENIED_PROTOCOLS: [&str; 8] = ["about", "chrome", "data", "devtools", "file", "filesystem", "javascript", "view-source"];

/// `previewWindowOpenAction` from what `Page.windowOpen` reports. Chrome lists `menubar` (with
/// the other toolbars) for a window that opens as a tab, a `target=_blank` link or a bare
/// `window.open`, and leaves it out for a popup, one asked for with a size or `popup`. Chrome
/// doesn't say whether a form POSTed, which Electron keeps in place.
fn window_open_action(url: &str, features: &[&str]) -> Opened {
    let scheme = url.split_once(':').map(|(scheme, _)| scheme).filter(|s| s.starts_with(|c: char| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)));
    let Some(scheme) = scheme.map(str::to_ascii_lowercase) else { return Opened::Deny };
    if DENIED_PROTOCOLS.contains(&scheme.as_str()) {
        Opened::Deny
    } else if scheme != "http" && scheme != "https" {
        Opened::Navigate
    } else if features.contains(&"menubar") {
        Opened::Tab
    } else {
        Opened::Popup
    }
}

/// A page live inline, as web's iframe in a chat row: its own browser context, a CSS viewport the
/// row's size, screencast frames only when the page paints, and the row's pointer and keys. The
/// page reaches its host through `window.__s5Frame(text)`, read with `Live::messages`.
#[derive(Clone)]
pub struct Live {
    pub id: u64,
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub dark: bool,
    pub pixels: Pixels,
    /// Runs in each of its documents before the page's own scripts.
    pub script: Option<String>,
    messages: Arc<Mutex<Vec<String>>>,
}

impl Live {
    pub fn new(url: String, width: u32, height: u32, dark: bool) -> Live {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Live { id, url, width, height, dark, pixels: Pixels::new(), script: None, messages: Arc::default() }
    }

    /// Its Chrome let it go without `LiveClose` (a relaunch).
    pub fn gone(&self) -> bool {
        Arc::strong_count(&self.messages) == 1
    }

    /// What the page sent since the last call. Each main-frame load adds `LIVE_LOAD` first, so a
    /// host can tell a page that navigated away.
    pub fn messages(&self) -> Vec<String> {
        std::mem::take(&mut *self.messages.lock().unwrap())
    }
}

pub const LIVE_LOAD: &str = r#"{"s5":"load"}"#;

/// Input for a live frame, in its CSS pixels.
pub enum LiveInput {
    Wheel { x: f32, y: f32, dx: f32, dy: f32 },
    Mouse { kind: &'static str, x: f32, y: f32, button: &'static str, clicks: u32, modifiers: u32 },
    Text(String),
    Key(&'static str),
}

struct LiveFrame {
    live: Live,
    context: Option<String>,
    target: Option<String>,
    session: Option<String>,
    /// Calls made before the session attached.
    queue: Vec<(&'static str, Value)>,
    pressed: Option<&'static str>,
}

/// The tallest page an inline shot covers, and the most device pixels it may be tall: the rest
/// shows full screen.
const SHOT_MAX_HEIGHT: f32 = 4000.;
const SHOT_MAX_PIXELS: f32 = 8000.;

/// htmlRenderFrameHeight's live measure, once fonts are in and a first animation frame settled.
const SHOT_MEASURE_JS: &str = "(async () => { try { await document.fonts.ready; } catch (e) {} await new Promise(r => setTimeout(r, 300)); const e = document.documentElement; return Math.ceil(e.scrollHeight > e.clientHeight ? e.scrollHeight : e.getBoundingClientRect().height); })()";

/// The CSS height a page measured at `content` is shot at, and the screenshot's clip scale so
/// the PNG stays within `SHOT_MAX_PIXELS` device pixels at `scale`.
fn shot_size(content: f64, scale: f32) -> Option<(f32, f32)> {
    let content = content.is_finite().then(|| (content.ceil() as f32).clamp(80., 20_000.))?;
    let height = content.min(SHOT_MAX_HEIGHT);
    Some((height, (SHOT_MAX_PIXELS / (height * scale.max(0.01))).min(1.)))
}

struct Snap {
    shot: Shot,
    context: Option<String>,
    session: Option<String>,
    started: std::time::Instant,
    navigated: bool,
    measuring: bool,
}

/// Which tab a command acts on, when it names one.
fn cmd_tab(cmd: &Cmd) -> Option<TabId> {
    match cmd {
        Cmd::Open(tab, _) | Cmd::Adopt(tab, ..) | Cmd::Close(tab) | Cmd::Navigate(tab, _) | Cmd::Back(tab) | Cmd::Forward(tab) | Cmd::Reload { tab, .. } | Cmd::Stop(tab) | Cmd::Zoom(tab, _) | Cmd::Device(tab, _) | Cmd::Pick(tab, _) | Cmd::DevTools(tab) | Cmd::ColorScheme(tab, _) | Cmd::Screenshot(tab, _) | Cmd::Record { tab, .. } | Cmd::Dialog { tab, .. } | Cmd::Mute(tab, _) | Cmd::Mark(tab, _) | Cmd::Erase(tab, ..) | Cmd::CaptureMarks(tab) | Cmd::Enclosed(tab, _) | Cmd::ClipboardAnswer(tab, ..) => Some(*tab),
        _ => None,
    }
}

/// A navigation that never reached a page, as Chrome reports it (`ERR_CONNECTION_REFUSED`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Failure {
    pub url: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub can_back: bool,
    pub can_forward: bool,
    /// A frame has arrived since the tab opened.
    pub painted: bool,
    pub failure: Option<Failure>,
    pub zoom: f32,
    pub device: Option<(u32, u32)>,
    pub picking: bool,
    /// While picking, the CSS-pixel `[x, y, w, h]` of the element under the pointer. The headless
    /// shell never paints Chrome's inspect overlay, so the panel draws this box itself.
    pub hover: Option<[f32; 4]>,
    /// The picked element's current CSS-pixel rect while `Cmd::Track` follows it.
    pub picked: Option<[f32; 4]>,
    pub recording: bool,
    /// The page is playing sound (Electron's `isCurrentlyAudible`), muted or not.
    pub audible: bool,
    pub muted: bool,
    /// The page's icon URL (a `<link rel=icon>`, else `/favicon.ico`), once it loaded.
    pub favicon: Option<String>,
    /// Chrome's target id once the page is attached, for an agent's own connection to it.
    pub target: Option<String>,
    /// The DevTools `host:port` of the Chrome showing the tab: each profile runs its own.
    pub host: Option<String>,
}

impl Default for Status {
    fn default() -> Self {
        Status { url: String::new(), title: String::new(), loading: false, can_back: false, can_forward: false, painted: false, failure: None, zoom: 1., device: None, picking: false, hover: None, picked: None, recording: false, audible: false, muted: false, favicon: None, target: None, host: None }
    }
}

pub enum Event {
    Screenshot(Result<PathBuf, String>),
    /// A page opened a new window; it becomes a tab beside its opener.
    Popup { target: String, opener: String, url: String },
    /// A link the page opened in a new tab, which Electron loads afresh: `background` when a
    /// middle or Ctrl/Cmd-click opened it, so it doesn't take over.
    OpenLink { tab: TabId, url: String, background: bool },
    /// A popup closed itself (`window.close()`).
    PopupClosed(TabId),
    /// A live inline frame opened a link.
    FrameLink(String),
    /// A picked element's details (`PickedElementPayload` fields plus its rect) and its PNG crop.
    Picked { tab: TabId, element: Value, image: Option<Vec<u8>> },
    DevTools(Result<String, String>),
    Recorded(Result<PathBuf, String>),
    /// The page opened `alert`, `confirm` or `prompt` and waits for `Cmd::Dialog`.
    Dialog { tab: TabId, kind: String, message: String, default: String },
    DialogClosed(TabId),
    /// A right-click's link, image rect, editable and selection, at the menu's window point.
    Context { tab: TabId, at: (f32, f32), info: Value },
    Clipboard(String),
    /// The page asked to read the clipboard, answered with `Cmd::ClipboardAnswer`.
    ClipboardRead { tab: TabId, id: u64, origin: String },
    /// A link to another application (`mailto:`, `slack:`), which Electron confirms first.
    External { tab: TabId, url: String },
    Image(Vec<u8>),
    /// A finished download, moved to the Downloads folder, as Electron does for a person's.
    Download(Result<PathBuf, String>),
    /// The mark `Cmd::Erase` removed, by the order marks were added.
    Erased(TabId, Option<usize>),
    /// What a region drag enclosed: each element as a pick reports it, none for a bare region.
    Enclosed(TabId, [f32; 4], Vec<Value>),
    /// The annotation's screenshot, cropped to its marks.
    Marked(TabId, Option<Vec<u8>>),
}

#[derive(Default)]
pub struct Shared {
    pub tabs: HashMap<TabId, Status>,
    /// Chrome could not start, or its connection dropped.
    pub fatal: Option<String>,
    pub events: Vec<Event>,
    /// Bumped on every visible change, so the UI re-runs its view only when something moved.
    pub generation: u64,
}

/// The Default profile, which every tab uses unless it opens in another.
pub const DEFAULT_PROFILE: &str = "default";
pub const INCOGNITO_PROFILE: &str = "incognito";

/// The panel's Chrome: one process per browser profile in use, as Electron keeps one session per
/// profile. Only the Default profile's starts with the panel; another starts with its first tab.
#[derive(Default)]
pub struct Browser {
    workers: HashMap<String, Sender<Cmd>>,
    pub shared: Arc<Mutex<Shared>>,
    waker: Option<Waker>,
    /// Each tab's profile, so its commands reach its Chrome.
    profiles: HashMap<TabId, String>,
    viewport: Option<(f32, f32, f32)>,
    shown: Option<TabId>,
    tracked: Option<TabId>,
    recording: Option<TabId>,
    /// Removed profiles whose Chrome still shows tabs: their data goes when the last one closes.
    doomed: Vec<String>,
}

impl Browser {
    pub fn started(&self) -> bool {
        !self.workers.is_empty()
    }

    /// Launches the Default profile's Chrome on a worker; commands sent before it is up queue.
    pub fn start(&mut self, waker: Waker) {
        self.waker = Some(waker);
        self.worker(DEFAULT_PROFILE);
    }

    /// Lets tabs start their profile's Chrome, without starting one yet.
    pub fn set_waker(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    /// The profile's worker, starting its Chrome first if this is its first use.
    fn worker(&mut self, profile: &str) -> Option<Sender<Cmd>> {
        if let Some(tx) = self.workers.get(profile) {
            return Some(tx.clone());
        }
        let waker = self.waker.clone()?;
        let (tx, rx) = channel();
        let shared = self.shared.clone();
        let owned = profile.to_owned();
        std::thread::spawn(move || {
            if let Err(e) = run(rx, shared.clone(), waker.clone(), owned) {
                let mut s = shared.lock().unwrap();
                s.fatal = Some(e);
                s.generation += 1;
                drop(s);
                waker();
            }
        });
        // A late Chrome learns what the others were told: the page area, and its shown tab.
        if let Some((w, h, scale)) = self.viewport {
            let _ = tx.send(Cmd::Viewport { w, h, scale });
        }
        let _ = tx.send(Cmd::Show(self.shown.filter(|t| self.profile(*t) == profile)));
        let _ = tx.send(Cmd::Track(self.tracked.filter(|t| self.profile(*t) == profile)));
        self.workers.insert(profile.to_owned(), tx.clone());
        Some(tx)
    }

    fn profile(&self, tab: TabId) -> &str {
        self.profiles.get(&tab).map_or(DEFAULT_PROFILE, String::as_str)
    }

    /// The Default profile's worker, which browser imports write into.
    pub fn sender(&self) -> Option<Sender<Cmd>> {
        self.workers.get(DEFAULT_PROFILE).cloned()
    }

    /// Opens a tab in `profile`'s Chrome.
    pub fn open(&mut self, tab: TabId, pixels: Pixels, profile: &str) {
        self.profiles.insert(tab, profile.to_owned());
        if let Some(tx) = self.worker(profile) {
            let _ = tx.send(Cmd::Open(tab, pixels));
        }
    }

    /// Shows a popup as `tab` in its opener's Chrome, the only one that has it.
    pub fn adopt(&mut self, tab: TabId, pixels: Pixels, target: String, opener: Option<TabId>) {
        let profile = opener.map_or(DEFAULT_PROFILE, |o| self.profile(o)).to_owned();
        self.profiles.insert(tab, profile.clone());
        if let Some(tx) = self.worker(&profile) {
            let _ = tx.send(Cmd::Adopt(tab, pixels, target));
        }
    }

    /// Clears a profile's cookies and cache, as Electron clears its session: through its Chrome
    /// when it runs, else by deleting its folder. A removed profile's tabs stay open, as web's
    /// "Removed profile", and its folder goes with the last of them.
    pub fn clear(&mut self, profile: &str, removed: bool) {
        match self.workers.get(profile) {
            Some(tx) => {
                for cmd in [Cmd::ClearCookies, Cmd::ClearCache] {
                    let _ = tx.send(cmd);
                }
                if removed && !self.doomed.iter().any(|p| p == profile) {
                    self.doomed.push(profile.to_owned());
                }
            }
            None => self.quit(profile, true),
        }
    }

    /// Stops `profile`'s Chrome, deleting its data when `wipe`.
    fn quit(&mut self, profile: &str, wipe: bool) {
        match self.workers.remove(profile) {
            Some(tx) => {
                let _ = tx.send(Cmd::Quit { wipe });
            }
            None if wipe => {
                let dir = profile_dir(profile);
                std::thread::spawn(move || std::fs::remove_dir_all(dir));
            }
            None => {}
        }
    }

    pub fn send(&mut self, cmd: Cmd) {
        let route = |b: &Browser, tab: Option<TabId>| tab.map_or(DEFAULT_PROFILE, |t| b.profile(t)).to_owned();
        match cmd {
            Cmd::Viewport { w, h, scale } => {
                self.viewport = Some((w, h, scale));
                for tx in self.workers.values() {
                    let _ = tx.send(Cmd::Viewport { w, h, scale });
                }
            }
            Cmd::Show(tab) | Cmd::Track(tab) => {
                let show = matches!(cmd, Cmd::Show(_));
                if show {
                    self.shown = tab;
                } else {
                    self.tracked = tab;
                }
                for (profile, tx) in &self.workers {
                    let mine = tab.filter(|t| self.profile(*t) == profile);
                    let _ = tx.send(if show { Cmd::Show(mine) } else { Cmd::Track(mine) });
                }
            }
            Cmd::Open(tab, pixels) => self.open(tab, pixels, DEFAULT_PROFILE),
            Cmd::SetCookies(..) | Cmd::Snapshot(_) | Cmd::LiveOpen(_) | Cmd::LiveInput(..) | Cmd::LiveResize(..) | Cmd::LiveEval(..) | Cmd::LiveClose(_) => {
                if let Some(tx) = self.workers.get(DEFAULT_PROFILE) {
                    let _ = tx.send(cmd);
                }
            }
            Cmd::StopRecording(_) => {
                let recording = self.recording.take();
                let profile = route(self, recording);
                if let Some(tx) = self.workers.get(&profile) {
                    let _ = tx.send(cmd);
                }
            }
            cmd => {
                let tab = cmd_tab(&cmd);
                if let Cmd::Record { tab, .. } = &cmd {
                    self.recording = Some(*tab);
                }
                // Page input, edits and clearing data act on the shown tab's Chrome.
                let profile = route(self, tab.or(self.shown));
                let closed = matches!(cmd, Cmd::Close(_));
                if let Some(tx) = self.workers.get(&profile) {
                    let _ = tx.send(cmd);
                }
                if closed {
                    self.profiles.remove(&tab.unwrap_or_default());
                    // Only the Default profile's Chrome outlives its tabs.
                    if profile != DEFAULT_PROFILE && !self.profiles.values().any(|p| *p == profile) {
                        let wipe = profile == INCOGNITO_PROFILE || self.doomed.iter().any(|p| *p == profile);
                        self.doomed.retain(|p| *p != profile);
                        self.quit(&profile, wipe);
                    }
                }
            }
        }
    }

    pub fn status(&self, tab: TabId) -> Status {
        self.shared.lock().unwrap().tabs.get(&tab).cloned().unwrap_or_default()
    }
}

/// Where a profile keeps its Chrome data: Default's own folder, one per named profile, and a
/// throwaway one for Incognito, emptied whenever its Chrome starts.
pub fn profile_dir(profile: &str) -> PathBuf {
    let root = crate::host::t3_home().join("userdata").join("browser");
    match profile {
        DEFAULT_PROFILE => root.join("rune-default"),
        INCOGNITO_PROFILE => std::env::temp_dir().join(format!("s5-rune-incognito-{}", std::process::id())),
        other => root.join(format!("rune-profile-{}", other.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect::<String>())),
    }
}

fn chrome_path() -> String {
    if let Ok(p) = std::env::var("S5_CHROME") {
        return p;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let candidates = [
        format!("{home}/chrome-headless-shell-linux64/chrome-headless-shell"),
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into(),
        format!("{home}/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
        "/Applications/Chromium.app/Contents/MacOS/Chromium".into(),
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser".into(),
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge".into(),
        "/usr/bin/google-chrome".into(),
        "/usr/bin/chromium".into(),
    ];
    candidates.iter().find(|p| std::path::Path::new(p).is_file()).cloned().unwrap_or_else(|| "chrome-headless-shell".into())
}

/// Every Chrome this process started, by profile.
static CHROME: Mutex<Vec<(String, Child)>> = Mutex::new(Vec::new());

/// Stops the headless Chromes this process started: once the window loop returns, or before a
/// relaunch at a new display scale.
pub fn shutdown() {
    for (_, mut child) in std::mem::take(&mut *CHROME.lock().unwrap()) {
        let _ = child.kill();
        // Reaped, so a relaunch finds the profile free.
        let _ = child.wait();
    }
    let _ = std::fs::remove_dir_all(profile_dir(INCOGNITO_PROFILE));
}

/// Stops one profile's Chrome.
fn shutdown_profile(profile: &str) {
    let mut chromes = CHROME.lock().unwrap();
    while let Some(i) = chromes.iter().position(|(p, _)| p == profile) {
        let (_, mut child) = chromes.remove(i);
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Starts Chrome on the profile's persistent folder, as the Electron app keeps one session per
/// profile; a folder another instance holds falls back to a throwaway one.
fn launch(profile: &str, scale: f32) -> Result<(Child, String, PathBuf), String> {
    let dir = profile_dir(profile);
    if profile == INCOGNITO_PROFILE {
        let _ = std::fs::remove_dir_all(&dir);
    }
    let spare = std::env::temp_dir().join(format!("s5-rune-chrome-{}-{profile}", std::process::id()));
    match launch_in(&dir, scale) {
        Ok((child, ws)) => Ok((child, ws, dir)),
        Err(_) => launch_in(&spare, scale).map(|(child, ws)| (child, ws, spare)),
    }
}

/// Every Chrome's switches but its folder and scale.
const CHROME_ARGS: [&str; 7] = ["--headless", "--no-sandbox", "--disable-gpu", "--hide-scrollbars", "--no-first-run", "--no-default-browser-check", "--remote-debugging-port=0"];

/// Whether Chrome could show a PDF itself, decided from how it was started rather than from
/// `navigator.pdfViewerEnabled`, which stays true where Chrome would download the PDF instead:
/// headless (no viewer extension to draw it), `--disable-pdf-viewer`, or a profile set to
/// `plugins.always_open_pdf_externally`. Only when none apply is the page's own answer asked.
fn native_pdf_possible(args: &[&str], preferences: &Value) -> bool {
    let headless = args.iter().any(|a| *a == "--headless" || a.starts_with("--headless="));
    let disabled = args.contains(&"--disable-pdf-viewer");
    let external = preferences["plugins"]["always_open_pdf_externally"] == true;
    !headless && !disabled && !external
}

fn preferences(dir: &std::path::Path) -> Value {
    std::fs::read(dir.join("Default").join("Preferences")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
}

fn launch_in(profile: &std::path::Path, scale: f32) -> Result<(Child, String), String> {
    let _ = std::fs::create_dir_all(profile);
    let mut child = Command::new(chrome_path())
        .args(CHROME_ARGS)
        // Page and browser ids are unguessable, so any origin may open the DevTools frontend.
        .arg("--remote-allow-origins=*")
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("--force-device-scale-factor={scale}"))
        .arg("about:blank")
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|e| format!("Chrome failed to start ({e}). Install Google Chrome or set S5_CHROME."))?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let mut lines = BufReader::new(stderr).lines();
    let ws = loop {
        let Some(line) = lines.next() else {
            let _ = child.kill();
            return Err("Chrome exited before DevTools came up".into());
        };
        let line = line.map_err(|e| e.to_string())?;
        if let Some(rest) = line.split("DevTools listening on ").nth(1) {
            break rest.trim().to_string();
        }
    };
    // Keep draining stderr so Chrome never blocks on a full pipe.
    std::thread::spawn(move || for _ in lines {});
    Ok((child, ws))
}

enum Reply {
    Erased(TabId),
    Enclosed(TabId, [f32; 4]),
    PdfViewer(String),
    MarksBounds(TabId),
    MarksShot(TabId),
    Created(TabId),
    Attached(TabId),
    History(TabId),
    Frame(TabId),
    Navigated(TabId, String),
    Saved(PathBuf),
    PickNode(TabId),
    PickInfo(TabId),
    PickShot(TabId, Value),
    Hover(TabId),
    Cookie(super::browser_import::Job, String),
    RecCreated,
    RecAttached,
    RecStopped,
    RecChunk(usize),
    RecSeed(f32),
    Track(TabId),
    Context(TabId, (f32, f32)),
    Selection(TabId, bool),
    Image,
    /// A key press for the recording, unless the focused field is a password.
    KeyPress(String),
    MuteScript(TabId),
    Favicon(TabId),
    SnapContext(u64),
    SnapCreated(u64),
    SnapAttached(u64),
    SnapNavigated(u64),
    SnapMeasured(u64),
    SnapShot(u64, f32),
    LiveContext(u64),
    LiveCreated(u64),
    LiveAttached(u64),
}

/// A recording: an encoder page in the same Chrome draws the tab's screencast frames into a
/// canvas and `MediaRecorder` encodes it, as the server's recordings do.
struct Recording {
    tab: TabId,
    mouse: bool,
    keys: bool,
    target: Option<String>,
    session: Option<String>,
    /// Encoder calls made before its session attached.
    queue: Vec<(String, Option<Reply>)>,
    /// Set once stopping; no frames go in after it.
    path: Option<PathBuf>,
    mime: String,
    chunks: Vec<Option<Vec<u8>>>,
}

#[derive(Default)]
struct Tab {
    target: Option<String>,
    session: Option<String>,
    /// Calls made before the session attached.
    queue: Vec<(&'static str, Value, Option<Reply>)>,
    history: Vec<(i64, String)>,
    index: usize,
    pixels: Option<Pixels>,
    zoom: f32,
    device: Option<(u32, u32)>,
    /// The last document load error, waiting for the error page that reports its URL.
    error: Option<String>,
    /// The new-document script that keeps a muted tab's next pages muted.
    mute_script: Option<String>,
    scheme: &'static str,
}

struct Worker {
    ws: WebSocket<TcpStream>,
    /// Chrome's DevTools `host:port`.
    host: String,
    next: u64,
    pending: HashMap<u64, Reply>,
    tabs: HashMap<TabId, Tab>,
    sessions: HashMap<String, TabId>,
    shown: Option<TabId>,
    viewport: (f32, f32, f32),
    /// The device scale Chrome started with.
    scale: f32,
    pressed: bool,
    /// A hover lookup is in flight; the latest pointer position waits for it.
    hovering: bool,
    hover_next: Option<(TabId, f32, f32)>,
    recording: Option<Recording>,
    tracking: Option<TabId>,
    track_busy: bool,
    /// In-flight downloads by guid: the page's file name.
    downloads: HashMap<String, String>,
    /// Downloads an agent started, by guid: their page's target and Playwright's folder.
    agent_downloads: HashMap<String, (String, String)>,
    /// When each tab's renderer last crashed, for web's three remounts in 30 s.
    crashes: HashMap<TabId, Vec<std::time::Instant>>,
    /// Whether this Chrome shows PDFs itself: no when its switches or profile rule it out, else
    /// once a page has said.
    native_pdf: Option<bool>,
    snaps: HashMap<u64, Snap>,
    snap_next: u64,
    lives: HashMap<u64, LiveFrame>,
    /// Each `Page.windowOpen` waiting for its target: the opener, the URL and what to do.
    opening: Vec<(TabId, String, Opened, std::time::Instant)>,
    /// Targets adopted as popups, which may not open more.
    popups: HashSet<String>,
    /// The button and modifiers `Cmd::Pointer` set for the next press.
    button: &'static str,
    modifiers: u32,
    /// When a middle or modified click last pressed, and whether its link opens in the background.
    modified: Option<(std::time::Instant, bool)>,
    shared: Arc<Mutex<Shared>>,
    waker: Waker,
}

fn run(rx: Receiver<Cmd>, shared: Arc<Mutex<Shared>>, waker: Waker, profile: String) -> Result<(), String> {
    // The headless shell paints frames at the device scale it starts with, whatever scale a page
    // emulates, so it waits for the window's.
    let mut early = Vec::new();
    let scale = loop {
        let Ok(cmd) = rx.recv() else { return Ok(()) };
        if let Cmd::Quit { wipe } = cmd {
            if wipe {
                let _ = std::fs::remove_dir_all(profile_dir(&profile));
            }
            return Ok(());
        }
        let scale = match &cmd {
            Cmd::Viewport { scale, .. } => Some(*scale),
            Cmd::Snapshot(_) | Cmd::LiveOpen(_) => Some(2.),
            _ => None,
        };
        early.push(cmd);
        if let Some(scale) = scale {
            break scale;
        }
    };
    let (child, url, dir) = launch(&profile, scale)?;
    let native_pdf = (!native_pdf_possible(&CHROME_ARGS, &preferences(&dir))).then_some(false);
    CHROME.lock().unwrap().push((profile.clone(), child));
    let host = url.trim_start_matches("ws://").split('/').next().unwrap_or_default().to_string();
    let stream = TcpStream::connect(&host).map_err(|e| e.to_string())?;
    let (ws, _) = tungstenite::client(url.as_str(), stream).map_err(|e| e.to_string())?;
    ws.get_ref().set_read_timeout(Some(Duration::from_millis(15))).map_err(|e| e.to_string())?;
    let mut w = Worker {
        ws,
        host: host.clone(),
        next: 0,
        pending: HashMap::new(),
        tabs: HashMap::new(),
        sessions: HashMap::new(),
        shown: None,
        viewport: (0., 0., 1.),
        scale,
        pressed: false,
        hovering: false,
        hover_next: None,
        recording: None,
        tracking: None,
        track_busy: false,
        downloads: HashMap::new(),
        agent_downloads: HashMap::new(),
        crashes: HashMap::new(),
        native_pdf,
        snaps: HashMap::new(),
        snap_next: 0,
        lives: HashMap::new(),
        opening: Vec::new(),
        popups: HashSet::new(),
        button: "left",
        modifiers: 0,
        modified: None,
        shared,
        waker,
    };
    w.send(None, "Target.setDiscoverTargets", json!({ "discover": true }), None);
    let dir = downloads_staging();
    let _ = std::fs::create_dir_all(&dir);
    w.send(None, "Browser.setDownloadBehavior", json!({ "behavior": "allowAndName", "downloadPath": dir, "eventsEnabled": true }), None);
    for cmd in early {
        w.command(cmd);
    }
    loop {
        loop {
            match rx.try_recv() {
                Ok(Cmd::Quit { wipe }) => {
                    shutdown_profile(&profile);
                    if wipe {
                        let _ = std::fs::remove_dir_all(profile_dir(&profile));
                    }
                    return Ok(());
                }
                Ok(cmd) => w.command(cmd),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        }
        w.expire_snaps();
        let msg = match w.ws.read() {
            Ok(Message::Text(t)) => t,
            Ok(_) => continue,
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            Err(e) => return Err(format!("DevTools connection closed: {e}")),
        };
        if let Ok(v) = serde_json::from_str::<Value>(&msg) {
            w.message(v);
        }
    }
}

impl Worker {
    fn send(&mut self, session: Option<&str>, method: &str, params: Value, reply: Option<Reply>) {
        self.next += 1;
        let mut msg = json!({ "id": self.next, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        if let Some(r) = reply {
            self.pending.insert(self.next, r);
        }
        let _ = self.ws.send(Message::text(msg.to_string()));
    }

    /// A call on a tab's page session, held until the session attaches.
    fn call(&mut self, tab: TabId, method: &'static str, params: Value, reply: Option<Reply>) {
        let Some(t) = self.tabs.get_mut(&tab) else { return };
        match t.session.clone() {
            Some(session) => self.send(Some(&session), method, params, reply),
            None => t.queue.push((method, params, reply)),
        }
    }

    fn muted(&self, tab: TabId) -> bool {
        self.shared.lock().unwrap().tabs.get(&tab).is_some_and(|s| s.muted)
    }

    fn update(&self, tab: TabId, f: impl FnOnce(&mut Status)) {
        let mut s = self.shared.lock().unwrap();
        let Some(status) = s.tabs.get_mut(&tab) else { return };
        let before = status.clone();
        f(status);
        if *status != before {
            s.generation += 1;
            drop(s);
            (self.waker)();
        }
    }

    /// Without Chrome's own viewer, a PDF page's response is replaced with the bundled one.
    fn pdf_viewer(&mut self, session: &str, native: bool) {
        if !native {
            self.send(Some(session), "Fetch.enable", json!({ "patterns": [{ "urlPattern": "*.pdf*", "resourceType": "Document", "requestStage": "Response" }] }), None);
        }
    }

    fn metrics(&mut self, tab: TabId) {
        let (w, h, _) = self.viewport;
        let (zoom, device) = self.tabs.get(&tab).map_or((1., None), |t| (t.zoom, t.device));
        if w < 1. || h < 1. {
            return;
        }
        // An emulated device keeps its CSS size; zoom only scales how large it shows.
        let (width, height) = match device {
            Some((dw, dh)) => (dw, dh),
            None => metrics(w, h, zoom),
        };
        self.call(tab, "Emulation.setDeviceMetricsOverride", json!({ "width": width, "height": height, "deviceScaleFactor": self.scale, "mobile": false }), None);
    }

    fn screencast(&mut self, tab: TabId) {
        let (w, h, _) = self.viewport;
        if w < 1. || h < 1. {
            return;
        }
        self.call(tab, "Page.stopScreencast", json!({}), None);
        let (mw, mh) = ((w * self.scale).round() as u32, (h * self.scale).round() as u32);
        self.call(tab, "Page.startScreencast", json!({ "format": "jpeg", "quality": 92, "maxWidth": mw, "maxHeight": mh }), None);
        // Screencast only sends a frame when the page changes; paint the current one now.
        self.call(tab, "Page.captureScreenshot", json!({ "format": "jpeg", "quality": 92 }), Some(Reply::Frame(tab)));
    }

    fn hover(&mut self) {
        if self.hovering {
            return;
        }
        let Some((tab, x, y)) = self.hover_next.take() else { return };
        self.hovering = true;
        let expression = format!("(() => {{ const e = document.elementFromPoint({x}, {y}); if (!e) return null; const r = e.getBoundingClientRect(); return [r.x, r.y, r.width, r.height]; }})()");
        self.call(tab, "Runtime.evaluate", json!({ "expression": expression, "returnByValue": true }), Some(Reply::Hover(tab)));
    }

    fn history(&mut self, tab: TabId) {
        self.call(tab, "Page.getNavigationHistory", json!({}), Some(Reply::History(tab)));
    }

    fn command(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Pointer { button, modifiers } => {
                (self.button, self.modifiers) = (button, modifiers);
                let (middle, command, shift) = (button == "middle", modifiers & 6 != 0, modifiers & 8 != 0);
                self.modified = (middle || command || shift).then(|| (std::time::Instant::now(), (middle || command) && !shift));
            }
            Cmd::LiveOpen(live) => {
                let id = live.id;
                self.lives.insert(id, LiveFrame { live, context: None, target: None, session: None, queue: Vec::new(), pressed: None });
                self.send(None, "Target.createBrowserContext", json!({ "disposeOnDetach": true }), Some(Reply::LiveContext(id)));
            }
            Cmd::LiveInput(id, input) => match input {
                LiveInput::Mouse { kind, x, y, button, clicks, modifiers } => {
                    let Some(frame) = self.lives.get_mut(&id) else { return };
                    match kind {
                        "mousePressed" => frame.pressed = Some(button),
                        "mouseReleased" => frame.pressed = None,
                        _ => {}
                    }
                    let held = frame.pressed;
                    let button = if kind == "mouseMoved" { held.unwrap_or("none") } else { button };
                    let buttons = match held {
                        None => 0,
                        Some("middle") => 4,
                        Some(_) => 1,
                    };
                    let clicks = if kind == "mouseMoved" { 0 } else { clicks.max(1) };
                    self.live_call(id, "Input.dispatchMouseEvent", json!({ "type": kind, "x": x, "y": y, "button": button, "buttons": buttons, "modifiers": modifiers, "clickCount": clicks }));
                }
                LiveInput::Wheel { x, y, dx, dy } => self.live_call(id, "Input.dispatchMouseEvent", json!({ "type": "mouseWheel", "x": x, "y": y, "deltaX": -dx, "deltaY": -dy })),
                LiveInput::Text(text) => self.live_call(id, "Input.insertText", json!({ "text": text })),
                LiveInput::Key(key) => {
                    let text = if key == "Enter" { "\r" } else { "" };
                    let down = if text.is_empty() { "rawKeyDown" } else { "keyDown" };
                    for kind in [down, "keyUp"] {
                        self.live_call(id, "Input.dispatchKeyEvent", json!({ "type": kind, "key": key, "code": key, "windowsVirtualKeyCode": key_code(key), "text": if kind == "keyUp" { "" } else { text } }));
                    }
                }
            },
            Cmd::LiveResize(id, width, height) => {
                let Some(frame) = self.lives.get_mut(&id) else { return };
                (frame.live.width, frame.live.height) = (width.max(1), height.max(1));
                self.live_view(id);
            }
            Cmd::LiveEval(id, expression) => self.live_call(id, "Runtime.evaluate", json!({ "expression": expression })),
            Cmd::LiveClose(id) => {
                if let Some(context) = self.lives.remove(&id).and_then(|f| f.context) {
                    self.send(None, "Target.disposeBrowserContext", json!({ "browserContextId": context }), None);
                }
            }
            Cmd::Snapshot(shot) => {
                self.snap_next += 1;
                let id = self.snap_next;
                self.snaps.insert(id, Snap { shot, context: None, session: None, started: std::time::Instant::now(), navigated: false, measuring: false });
                self.send(None, "Target.createBrowserContext", json!({ "disposeOnDetach": true }), Some(Reply::SnapContext(id)));
            }
            Cmd::Open(tab, pixels) => {
                self.tabs.insert(tab, Tab { pixels: Some(pixels), zoom: 1., ..Default::default() });
                self.shared.lock().unwrap().tabs.insert(tab, Status::default());
                self.send(None, "Target.createTarget", json!({ "url": "about:blank" }), Some(Reply::Created(tab)));
            }
            Cmd::Adopt(tab, pixels, target) => {
                self.tabs.insert(tab, Tab { pixels: Some(pixels), zoom: 1., target: Some(target.clone()), ..Default::default() });
                self.shared.lock().unwrap().tabs.insert(tab, Status::default());
                self.send(None, "Target.attachToTarget", json!({ "targetId": target, "flatten": true }), Some(Reply::Attached(tab)));
            }
            Cmd::Close(tab) => {
                if self.recording.as_ref().is_some_and(|r| r.tab == tab) {
                    self.end_recording(None);
                }
                if let Some(t) = self.tabs.remove(&tab) {
                    if let Some(target) = t.target {
                        self.send(None, "Target.closeTarget", json!({ "targetId": target }), None);
                    }
                    if let Some(s) = t.session {
                        self.sessions.remove(&s);
                    }
                }
                if self.shown == Some(tab) {
                    self.shown = None;
                }
                self.shared.lock().unwrap().tabs.remove(&tab);
            }
            Cmd::Show(tab) => {
                if self.shown == tab {
                    return;
                }
                if let Some(old) = std::mem::replace(&mut self.shown, tab) {
                    self.call(old, "Page.stopScreencast", json!({}), None);
                }
                if let Some(tab) = tab {
                    self.screencast(tab);
                }
            }
            Cmd::Navigate(tab, url) => {
                self.update(tab, |s| {
                    s.loading = true;
                    s.failure = None;
                    s.url = url.clone();
                });
                if let Some(t) = self.tabs.get_mut(&tab) {
                    t.error = None;
                }
                self.call(tab, "Page.navigate", json!({ "url": url }), Some(Reply::Navigated(tab, url.clone())));
            }
            Cmd::Back(tab) | Cmd::Forward(tab) => {
                let Some(t) = self.tabs.get(&tab) else { return };
                let to = if matches!(cmd, Cmd::Back(_)) { t.index.checked_sub(1) } else { Some(t.index + 1) };
                if let Some(&(id, _)) = to.and_then(|i| t.history.get(i)) {
                    self.update(tab, |s| s.loading = true);
                    self.call(tab, "Page.navigateToHistoryEntry", json!({ "entryId": id }), None);
                }
            }
            Cmd::Reload { tab, hard } => {
                let failed = self.shared.lock().unwrap().tabs.get(&tab).and_then(|s| s.failure.clone());
                match failed {
                    // The error page's own reload would reload the error page.
                    Some(f) => self.command(Cmd::Navigate(tab, f.url)),
                    None => {
                        self.update(tab, |s| s.loading = true);
                        self.call(tab, "Page.reload", json!({ "ignoreCache": hard }), None);
                    }
                }
            }
            Cmd::Stop(tab) => {
                self.update(tab, |s| s.loading = false);
                self.call(tab, "Page.stopLoading", json!({}), None);
            }
            Cmd::Viewport { w, h, scale } => {
                if self.viewport == (w, h, scale) {
                    return;
                }
                self.viewport = (w, h, scale);
                let tabs: Vec<TabId> = self.tabs.keys().copied().collect();
                for tab in tabs {
                    self.metrics(tab);
                }
                if let Some(tab) = self.shown {
                    self.screencast(tab);
                }
            }
            Cmd::Zoom(tab, zoom) => {
                let Some(t) = self.tabs.get_mut(&tab) else { return };
                t.zoom = zoom;
                self.update(tab, |s| s.zoom = zoom);
                self.metrics(tab);
                // A metrics change alone sends no screencast frame.
                if self.shown == Some(tab) {
                    self.screencast(tab);
                }
            }
            Cmd::Device(tab, device) => {
                let Some(t) = self.tabs.get_mut(&tab) else { return };
                t.device = device;
                self.update(tab, |s| s.device = device);
                self.metrics(tab);
                if self.shown == Some(tab) {
                    self.screencast(tab);
                }
            }
            Cmd::Pick(tab, on) => {
                self.update(tab, |s| {
                    s.picking = on;
                    s.hover = None;
                });
                // A lookup lost with a closed tab must not block the next session's.
                self.hovering = false;
                self.hover_next = None;
                if on {
                    self.call(tab, "DOM.enable", json!({}), None);
                    self.call(tab, "Overlay.enable", json!({}), None);
                }
                let mode = if on { "searchForNode" } else { "none" };
                self.call(tab, "Overlay.setInspectMode", json!({ "mode": mode, "highlightConfig": HIGHLIGHT.clone() }), None);
            }
            Cmd::DevTools(tab) => {
                let target = self.tabs.get(&tab).and_then(|t| t.target.clone());
                let url = target.ok_or_else(|| "The page has not opened yet.".to_string()).map(|target| devtools_url(&self.host, &target));
                self.event(Event::DevTools(url));
            }
            Cmd::ColorScheme(tab, scheme) => {
                let Some(t) = self.tabs.get_mut(&tab) else { return };
                t.scheme = scheme;
                // Before the page attaches, its attach applies the scheme.
                if t.session.is_some() {
                    self.call(tab, "Emulation.setEmulatedMedia", json!({ "features": [{ "name": "prefers-color-scheme", "value": scheme }] }), None);
                }
            }
            Cmd::Mouse { kind, x, y, clicks } => {
                let Some(tab) = self.shown else { return };
                let zoom = self.input_scale(tab);
                match kind {
                    "mousePressed" => self.pressed = true,
                    "mouseReleased" => self.pressed = false,
                    _ => {}
                }
                let button = if kind != "mouseMoved" || self.pressed { self.button } else { "none" };
                let buttons = match (self.pressed, self.button) {
                    (false, _) => 0,
                    (true, "middle") => 4,
                    (true, _) => 1,
                };
                self.call(tab, "Input.dispatchMouseEvent", json!({ "type": kind, "x": x / zoom, "y": y / zoom, "button": button, "buttons": buttons, "modifiers": self.modifiers, "clickCount": if kind == "mouseMoved" { 0 } else { clicks.max(1) } }), None);
                if kind == "mouseReleased" {
                    (self.button, self.modifiers) = ("left", 0);
                }
                if let Some(r) = self.recording.as_ref().filter(|r| r.tab == tab && r.path.is_none()) {
                    let (cx, cy) = (x / zoom, y / zoom);
                    let press = match kind {
                        "mousePressed" if r.mouse => format!("; __s5Recorder.press({cx}, {cy}, true)"),
                        "mouseReleased" if r.mouse => format!("; __s5Recorder.press({cx}, {cy}, false)"),
                        _ => String::new(),
                    };
                    self.rec_eval(format!("__s5Recorder.cursor({cx}, {cy}){press}"), None);
                }
                if kind == "mouseMoved" && self.shared.lock().unwrap().tabs.get(&tab).is_some_and(|s| s.picking) {
                    self.hover_next = Some((tab, x / zoom, y / zoom));
                    self.hover();
                }
            }
            Cmd::Wheel { x, y, dx, dy } => {
                let Some(tab) = self.shown else { return };
                let zoom = self.input_scale(tab);
                self.call(tab, "Input.dispatchMouseEvent", json!({ "type": "mouseWheel", "x": x / zoom, "y": y / zoom, "deltaX": -dx, "deltaY": -dy }), None);
            }
            Cmd::Text(text) => {
                let Some(tab) = self.shown else { return };
                if text.chars().count() == 1 {
                    self.key_press(tab, key_label(&text));
                }
                self.call(tab, "Input.insertText", json!({ "text": text }), None);
            }
            Cmd::Key(key) => {
                let Some(tab) = self.shown else { return };
                self.key_press(tab, key_label(key));
                let code = key_code(key);
                let text = if key == "Enter" { "\r" } else { "" };
                let down = if text.is_empty() { "rawKeyDown" } else { "keyDown" };
                for kind in [down, "keyUp"] {
                    self.call(tab, "Input.dispatchKeyEvent", json!({ "type": kind, "key": key, "code": key, "windowsVirtualKeyCode": code, "text": if kind == "keyUp" { "" } else { text } }), None);
                }
            }
            Cmd::Screenshot(tab, path) => {
                self.call(tab, "Page.captureScreenshot", json!({ "format": "png" }), Some(Reply::Saved(path)));
            }
            Cmd::ClearCookies | Cmd::ClearCache => {
                let Some(tab) = self.shown.or_else(|| self.tabs.keys().next().copied()) else { return };
                let method = if matches!(cmd, Cmd::ClearCookies) { "Network.clearBrowserCookies" } else { "Network.clearBrowserCache" };
                self.call(tab, method, json!({}), None);
            }
            // One write per cookie, so a cookie Chrome rejects is skipped instead of the batch.
            Cmd::SetCookies(cookies, job) => {
                for (cookie, host) in cookies {
                    self.send(None, "Storage.setCookies", json!({ "cookies": [cookie] }), Some(Reply::Cookie(job.clone(), host)));
                }
            }
            Cmd::Record { tab, fps, mouse, keys } => {
                if self.recording.is_some() || !self.tabs.contains_key(&tab) {
                    return;
                }
                let script = ENCODER_JS.replace("__FPS__", &fps.to_string());
                self.recording = Some(Recording { tab, mouse, keys, target: None, session: None, queue: vec![(script, None)], path: None, mime: String::new(), chunks: vec![] });
                self.update(tab, |s| s.recording = true);
                self.send(None, "Target.createTarget", json!({ "url": "about:blank" }), Some(Reply::RecCreated));
                // Seed the first frame so a still page records too.
                let width = self.css_width(tab);
                self.call(tab, "Page.captureScreenshot", json!({ "format": "jpeg", "quality": 90 }), Some(Reply::RecSeed(width)));
            }
            Cmd::StopRecording(path) => {
                let Some(r) = self.recording.as_mut().filter(|r| r.path.is_none()) else { return };
                r.path = Some(path);
                self.rec_eval("__s5Recorder.stop()".into(), Some(Reply::RecStopped));
            }
            Cmd::Track(tab) => {
                self.tracking = tab;
                self.track_busy = false;
                let tabs: Vec<TabId> = self.tabs.keys().copied().collect();
                for id in tabs {
                    self.update(id, |s| s.picked = None);
                }
                if tab.is_some() {
                    self.track();
                }
            }
            Cmd::Dialog { tab, accept, text } => {
                self.call(tab, "Page.handleJavaScriptDialog", json!({ "accept": accept, "promptText": text }), None);
            }
            Cmd::Inspect { x, y, at } => {
                let Some(tab) = self.shown else { return };
                let zoom = self.input_scale(tab);
                let expression = CONTEXT_JS.replace("__X__", &(x / zoom).to_string()).replace("__Y__", &(y / zoom).to_string());
                self.call(tab, "Runtime.evaluate", json!({ "expression": expression, "returnByValue": true }), Some(Reply::Context(tab, at)));
            }
            Cmd::CopySelection { cut } => {
                let Some(tab) = self.shown else { return };
                let expression = "(() => { const e = document.activeElement; if (e && typeof e.selectionStart === 'number' && e.value !== undefined) return e.value.slice(e.selectionStart, e.selectionEnd); return String(getSelection()); })()";
                self.call(tab, "Runtime.evaluate", json!({ "expression": expression, "returnByValue": true }), Some(Reply::Selection(tab, cut)));
            }
            Cmd::CopyImage(clip) => {
                let Some(tab) = self.shown else { return };
                self.call(tab, "Page.captureScreenshot", json!({ "format": "png", "clip": clip }), Some(Reply::Image));
            }
            Cmd::Edit(command) => {
                let Some(tab) = self.shown else { return };
                self.call(tab, "Runtime.evaluate", json!({ "expression": format!("document.execCommand('{command}')") }), None);
            }
            Cmd::Mark(tab, call) => {
                self.call(tab, "Runtime.evaluate", json!({ "expression": format!("{MARKS_JS}; window.__s5Marks.{call}") }), None);
            }
            Cmd::Erase(tab, x, y) => {
                self.call(tab, "Runtime.evaluate", json!({ "expression": format!("{MARKS_JS}; window.__s5Marks.erase({x}, {y})"), "returnByValue": true }), Some(Reply::Erased(tab)));
            }
            Cmd::Enclosed(tab, r) => {
                let expression = format!("({ENCLOSED_JS})({}, {}, {}, {}, {PICK_JS})", r[0], r[1], r[2], r[3]);
                self.call(tab, "Runtime.evaluate", json!({ "expression": expression, "returnByValue": true }), Some(Reply::Enclosed(tab, r)));
            }
            Cmd::CaptureMarks(tab) => {
                self.call(tab, "Runtime.evaluate", json!({ "expression": format!("{MARKS_JS}; window.__s5Marks.bounds()"), "returnByValue": true }), Some(Reply::MarksBounds(tab)));
            }
            Cmd::Mute(tab, on) => {
                if self.muted(tab) == on {
                    return;
                }
                self.update(tab, |s| s.muted = on);
                self.call(tab, "Runtime.evaluate", json!({ "expression": format!("window.__s5Mute && __s5Mute({on})") }), None);
                if on {
                    self.call(tab, "Page.addScriptToEvaluateOnNewDocument", json!({ "source": "window.__s5Muted = true" }), Some(Reply::MuteScript(tab)));
                } else if let Some(identifier) = self.tabs.get_mut(&tab).and_then(|t| t.mute_script.take()) {
                    self.call(tab, "Page.removeScriptToEvaluateOnNewDocument", json!({ "identifier": identifier }), None);
                }
            }
            Cmd::ClipboardAnswer(tab, id, text) => {
                let expression = format!("window.__s5Resolve && __s5Resolve({id}, {})", json!(text));
                self.call(tab, "Runtime.evaluate", json!({ "expression": expression }), None);
            }
            // The run loop takes it before a command reaches here.
            Cmd::Quit { .. } => {}
        }
    }

    /// The shown page's CSS width, which maps frame pixels to page points.
    fn css_width(&self, tab: TabId) -> f32 {
        let (w, h, _) = self.viewport;
        self.tabs.get(&tab).map_or(w, |t| match t.device {
            Some((dw, _)) => dw as f32,
            None => metrics(w, h, t.zoom).0 as f32,
        })
    }

    /// A call on the encoder page, held until its session attaches.
    fn rec_eval(&mut self, expression: String, reply: Option<Reply>) {
        let Some(r) = self.recording.as_mut() else { return };
        match r.session.clone() {
            Some(session) => self.send(Some(&session), "Runtime.evaluate", json!({ "expression": expression, "awaitPromise": true, "returnByValue": true }), reply),
            None => r.queue.push((expression, reply)),
        }
    }

    fn rec_frame(&mut self, data: &str, css_width: f32) {
        if self.recording.as_ref().is_some_and(|r| r.path.is_none()) && css_width >= 1. {
            self.rec_eval(format!("__s5Recorder.frame(\"{data}\", {css_width})"), None);
        }
    }

    /// Closes the encoder and reports the saved file or why there is none; `None` drops it.
    fn end_recording(&mut self, result: Option<Result<PathBuf, String>>) {
        let Some(r) = self.recording.take() else { return };
        if let Some(target) = r.target {
            self.send(None, "Target.closeTarget", json!({ "targetId": target }), None);
        }
        self.update(r.tab, |s| s.recording = false);
        if let Some(result) = result {
            self.event(Event::Recorded(result));
        }
    }

    /// Shows a key in the recording, once the page says no password field has focus.
    fn key_press(&mut self, tab: TabId, label: String) {
        if !self.recording.as_ref().is_some_and(|r| r.tab == tab && r.keys && r.path.is_none()) {
            return;
        }
        let expression = "(() => { let e = document.activeElement; while (e && e.shadowRoot && e.shadowRoot.activeElement) e = e.shadowRoot.activeElement; return !!e && (e.tagName === 'IFRAME' || e.tagName.includes('-') || (e.getAttribute('type') || '').toLowerCase() === 'password'); })()";
        self.call(tab, "Runtime.evaluate", json!({ "expression": expression, "returnByValue": true }), Some(Reply::KeyPress(label)));
    }

    /// Asks where the picked element is now; frames ask again, one lookup at a time.
    fn track(&mut self) {
        let Some(tab) = self.tracking.filter(|_| !self.track_busy) else { return };
        self.track_busy = true;
        let expression = "(() => { const e = window.__s5Picked; if (!e || !e.isConnected) return null; const r = e.getBoundingClientRect(); return [r.x, r.y, r.width, r.height]; })()";
        self.call(tab, "Runtime.evaluate", json!({ "expression": expression, "returnByValue": true }), Some(Reply::Track(tab)));
    }

    /// Window points per CSS pixel on the shown page.
    fn input_scale(&self, tab: TabId) -> f32 {
        let (w, h, _) = self.viewport;
        self.tabs.get(&tab).map_or(1., |t| match t.device {
            Some(device) => {
                let (w, h) = device_area(w, h);
                device_box(w, h, device, t.zoom).2
            }
            None => t.zoom.max(0.25),
        })
    }

    fn event(&self, event: Event) {
        let mut s = self.shared.lock().unwrap();
        s.events.push(event);
        s.generation += 1;
        drop(s);
        (self.waker)();
    }

    fn message(&mut self, v: Value) {
        if let Some(id) = v["id"].as_u64() {
            if let Some(reply) = self.pending.remove(&id) {
                self.reply(reply, &v["result"], &v["error"]);
            }
            return;
        }
        let Some(method) = v["method"].as_str() else { return };
        let p = &v["params"];
        if method == "Target.targetCreated" {
            let info = &p["targetInfo"];
            let tab_of = |id: &str| self.tabs.iter().find(|(_, t)| t.target.as_deref() == Some(id)).map(|(tab, _)| *tab);
            let (Some(target), "page") = (info["targetId"].as_str().filter(|id| tab_of(id).is_none()), info["type"].as_str().unwrap_or_default()) else { return };
            let (target, opener, url) = (target.to_owned(), info["openerId"].as_str().unwrap_or_default().to_owned(), info["url"].as_str().unwrap_or_default().to_owned());
            match tab_of(&opener).filter(|_| !opener.is_empty()) {
                Some(tab) => self.window_opened(tab, target, opener, url),
                None if self.lives.values().any(|f| f.target.as_deref() == Some(opener.as_str())) => {
                    self.send(None, "Target.closeTarget", json!({ "targetId": target }), None);
                }
                // A middle or modified click opens its link with no opener and no `windowOpen`.
                None if opener.is_empty() && url.starts_with("http") => {
                    let click = self.modified.take().filter(|(at, _)| at.elapsed() < Duration::from_secs(2));
                    if let (Some((_, background)), Some(tab)) = (click, self.shown) {
                        self.send(None, "Target.closeTarget", json!({ "targetId": target }), None);
                        self.event(Event::OpenLink { tab, url, background });
                    }
                }
                None => {}
            }
            return;
        }
        if method == "Target.targetDestroyed" {
            let target = p["targetId"].as_str().unwrap_or_default();
            if self.popups.remove(target) {
                if let Some(tab) = self.tabs.iter().find(|(_, t)| t.target.as_deref() == Some(target)).map(|(tab, _)| *tab) {
                    self.event(Event::PopupClosed(tab));
                }
            }
            return;
        }
        if method == "Browser.downloadWillBegin" {
            // Electron's split: one the agent just caused lands in Playwright's folder under its
            // guid, with its events relayed; a person's goes to Downloads.
            let (guid, frame) = (p["guid"].as_str().unwrap_or_default(), p["frameId"].as_str().unwrap_or_default());
            if let Some(dir) = super::agent::agent_download(frame) {
                self.agent_downloads.insert(guid.to_string(), (frame.to_string(), dir));
                super::agent::download_event(frame, method, p.clone());
                return;
            }
            let name = p["suggestedFilename"].as_str().filter(|n| !n.is_empty()).unwrap_or("download").to_string();
            self.downloads.insert(p["guid"].as_str().unwrap_or_default().to_string(), name);
            return;
        }
        if method == "Browser.downloadProgress" {
            let guid = p["guid"].as_str().unwrap_or_default().to_string();
            if let Some((frame, dir)) = self.agent_downloads.get(&guid).cloned() {
                if p["state"] != "inProgress" {
                    self.agent_downloads.remove(&guid);
                }
                if p["state"] == "completed" {
                    let from = std::path::Path::new(&downloads_staging()).join(&guid);
                    let to = std::path::Path::new(&dir).join(&guid);
                    let _ = std::fs::create_dir_all(&dir);
                    if std::fs::rename(&from, &to).is_err() && std::fs::copy(&from, &to).is_ok() {
                        let _ = std::fs::remove_file(&from);
                    }
                }
                super::agent::download_event(&frame, method, p.clone());
                return;
            }
            match p["state"].as_str() {
                Some("completed") => {
                    let name = self.downloads.remove(&guid).unwrap_or_else(|| "download".into());
                    let saved = save_download(&std::path::Path::new(&downloads_staging()).join(&guid), &downloads_dir(), &name);
                    self.event(Event::Download(saved));
                }
                Some("canceled") => {
                    self.downloads.remove(&guid);
                }
                _ => {}
            }
            return;
        }
        if method == "Target.targetCrashed" {
            let Some(tab) = self.tabs.iter().find(|(_, t)| t.target.as_deref() == p["targetId"].as_str()).map(|(id, _)| *id) else { return };
            let now = std::time::Instant::now();
            let recent = self.crashes.entry(tab).or_default();
            recent.retain(|at| now.duration_since(*at) < Duration::from_secs(30));
            recent.push(now);
            // Web remounts a crashed page up to three times in 30 s, then leaves it.
            if recent.len() <= 3 {
                self.call(tab, "Page.reload", json!({}), None);
            }
            return;
        }
        if method == "Target.targetInfoChanged" {
            let info = &p["targetInfo"];
            let Some(tab) = self.tabs.iter().find(|(_, t)| t.target.as_deref() == info["targetId"].as_str()).map(|(id, _)| *id) else { return };
            let title = info["title"].as_str().unwrap_or_default().to_string();
            let url = info["url"].as_str().unwrap_or_default().to_string();
            self.update(tab, |s| {
                s.title = title;
                // An adopted popup may have navigated before it attached.
                if s.url.is_empty() && url.starts_with("http") {
                    s.url = url;
                }
            });
            return;
        }
        if let Some(id) = v["sessionId"].as_str().and_then(|s| self.lives.iter().find(|(_, f)| f.session.as_deref() == Some(s))).map(|(id, _)| *id) {
            match method {
                "Page.screencastFrame" => {
                    let session = v["sessionId"].as_str().unwrap_or_default().to_owned();
                    self.send(Some(&session), "Page.screencastFrameAck", json!({ "sessionId": p["sessionId"] }), None);
                    if let Some(frame) = self.lives.get(&id) {
                        show(&frame.live.pixels, p["data"].as_str());
                    }
                }
                "Page.frameNavigated" if p["frame"]["parentId"].is_null() && p["frame"]["url"] != "about:blank" => {
                    if let Some(frame) = self.lives.get(&id) {
                        frame.live.messages.lock().unwrap().push(LIVE_LOAD.to_owned());
                        (self.waker)();
                    }
                }
                "Runtime.bindingCalled" if p["name"] == "__s5Frame" => {
                    if let Some(frame) = self.lives.get(&id) {
                        frame.live.messages.lock().unwrap().push(p["payload"].as_str().unwrap_or_default().to_owned());
                        (self.waker)();
                    }
                }
                // A link the page opens goes to a browser tab, as web's frame hands it to the panel.
                "Page.windowOpen" => {
                    let url = p["url"].as_str().unwrap_or_default().to_owned();
                    if url.starts_with("http") {
                        self.event(Event::FrameLink(url));
                    }
                }
                _ => {}
            }
            return;
        }
        if let Some(id) = v["sessionId"].as_str().and_then(|s| self.snaps.iter().find(|(_, n)| n.session.as_deref() == Some(s))).map(|(id, _)| *id) {
            if method == "Page.loadEventFired" && self.snaps.get(&id).is_some_and(|n| n.navigated) {
                self.snap_measure(id);
            }
            return;
        }
        let Some(&tab) = v["sessionId"].as_str().and_then(|s| self.sessions.get(s)) else { return };
        // A page target's main frame shares the target's id.
        let target = self.tabs.get(&tab).and_then(|t| t.target.clone());
        let main = |frame: &Value| frame.as_str().is_some() && frame.as_str() == target.as_deref();
        match method {
            "Page.screencastFrame" => {
                let session = v["sessionId"].as_str().unwrap_or_default().to_string();
                self.send(Some(&session), "Page.screencastFrameAck", json!({ "sessionId": p["sessionId"] }), None);
                if self.shown == Some(tab) {
                    self.paint(tab, p["data"].as_str());
                }
                if self.recording.as_ref().is_some_and(|r| r.tab == tab) {
                    let width = p["metadata"]["deviceWidth"].as_f64().unwrap_or(0.) as f32;
                    self.rec_frame(p["data"].as_str().unwrap_or_default(), width);
                }
                if self.tracking == Some(tab) {
                    self.track();
                }
            }
            "Page.javascriptDialogOpening" => {
                let kind = p["type"].as_str().unwrap_or("alert").to_string();
                // Electron cancels a leave-page prompt without asking, so the page stays.
                if kind == "beforeunload" {
                    self.call(tab, "Page.handleJavaScriptDialog", json!({ "accept": false }), None);
                } else {
                    let message = p["message"].as_str().unwrap_or_default().to_string();
                    let default = p["defaultPrompt"].as_str().unwrap_or_default().to_string();
                    self.event(Event::Dialog { tab, kind, message, default });
                }
            }
            "Runtime.bindingCalled" if p["name"] == "__s5Page" => {
                if let Some(event) = page_message(tab, p["payload"].as_str().unwrap_or_default()) {
                    self.event(event);
                }
            }
            "Runtime.bindingCalled" if p["name"] == "__s5Audio" => {
                let audible = p["payload"] == "1";
                self.update(tab, |s| s.audible = audible);
            }
            "Fetch.requestPaused" => {
                let session = v["sessionId"].as_str().unwrap_or_default().to_string();
                let url = p["request"]["url"].as_str().unwrap_or_default();
                let ok = p["responseStatusCode"].as_u64().is_some_and(|c| (200..300).contains(&c));
                if ok && main(&p["frameId"]) && pdf_response(&p["responseHeaders"]) {
                    let body = base64::engine::general_purpose::STANDARD.encode(pdf_viewer(url));
                    let headers = json!([{ "name": "Content-Type", "value": "text/html; charset=utf-8" }, { "name": "Cache-Control", "value": "no-store" }]);
                    self.send(Some(&session), "Fetch.fulfillRequest", json!({ "requestId": p["requestId"], "responseCode": 200, "responseHeaders": headers, "body": body }), None);
                } else {
                    self.send(Some(&session), "Fetch.continueRequest", json!({ "requestId": p["requestId"] }), None);
                }
            }
            "Page.javascriptDialogClosed" => self.event(Event::DialogClosed(tab)),
            "Page.windowOpen" => {
                let features: Vec<&str> = p["windowFeatures"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                let url = p["url"].as_str().unwrap_or_default().to_owned();
                let action = window_open_action(&url, &features);
                self.opening.retain(|o| o.3.elapsed() < Duration::from_secs(2));
                self.opening.push((tab, url, action, std::time::Instant::now()));
            }
            "Page.frameStartedLoading" if main(&p["frameId"]) => self.update(tab, |s| s.loading = true),
            "Page.frameStoppedLoading" | "Page.loadEventFired" if method == "Page.loadEventFired" || main(&p["frameId"]) => {
                self.update(tab, |s| s.loading = false);
                self.history(tab);
                if method == "Page.loadEventFired" {
                    self.call(tab, "Runtime.evaluate", json!({ "expression": FAVICON_JS, "returnByValue": true }), Some(Reply::Favicon(tab)));
                }
            }
            "Page.frameNavigated" if p["frame"]["parentId"].is_null() => {
                let frame = &p["frame"];
                let unreachable = frame["unreachableUrl"].as_str().filter(|u| !u.is_empty()).map(str::to_owned);
                let url = unreachable.clone().unwrap_or_else(|| frame["url"].as_str().unwrap_or_default().to_string());
                let error = self.tabs.get_mut(&tab).and_then(|t| if unreachable.is_some() { t.error.clone() } else { t.error.take() });
                self.update(tab, |s| {
                    s.url = url.clone();
                    s.audible = false;
                    s.favicon = None;
                    s.failure = unreachable.map(|url| Failure {
                        description: error.or_else(|| s.failure.as_ref().map(|f| f.description.clone())).unwrap_or_default(),
                        url,
                    });
                });
                self.history(tab);
            }
            "Page.navigatedWithinDocument" if main(&p["frameId"]) => {
                let url = p["url"].as_str().unwrap_or_default().to_string();
                self.update(tab, |s| s.url = url);
                self.history(tab);
            }
            "Overlay.inspectNodeRequested" => {
                self.call(tab, "DOM.resolveNode", json!({ "backendNodeId": p["backendNodeId"] }), Some(Reply::PickNode(tab)));
            }
            "Overlay.inspectModeCanceled" => self.update(tab, |s| s.picking = false),
            "Network.loadingFailed" if p["type"] == "Document" && p["canceled"] != true => {
                let error = net_error(p["errorText"].as_str().unwrap_or_default());
                if let Some(t) = self.tabs.get_mut(&tab) {
                    t.error = Some(error.clone());
                }
                self.update(tab, |s| {
                    if let Some(f) = s.failure.as_mut().filter(|f| f.description.is_empty()) {
                        f.description = error;
                    }
                });
            }
            _ => {}
        }
    }

    fn reply(&mut self, reply: Reply, r: &Value, error: &Value) {
        match reply {
            Reply::LiveContext(id) => {
                let Some(context) = r["browserContextId"].as_str().map(str::to_owned) else { return };
                let open = self.lives.get_mut(&id).map(|f| f.context = Some(context.clone())).is_some();
                if open {
                    self.send(None, "Target.createTarget", json!({ "url": "about:blank", "browserContextId": context }), Some(Reply::LiveCreated(id)));
                } else {
                    self.send(None, "Target.disposeBrowserContext", json!({ "browserContextId": context }), None);
                }
            }
            Reply::LiveCreated(id) => {
                let Some(target) = r["targetId"].as_str().map(str::to_owned) else { return };
                if let Some(frame) = self.lives.get_mut(&id) {
                    frame.target = Some(target.clone());
                    self.send(None, "Target.attachToTarget", json!({ "targetId": target, "flatten": true }), Some(Reply::LiveAttached(id)));
                }
            }
            Reply::LiveAttached(id) => {
                let Some(session) = r["sessionId"].as_str().map(str::to_owned) else { return };
                let Some(frame) = self.lives.get_mut(&id) else { return };
                frame.session = Some(session.clone());
                let queue = std::mem::take(&mut frame.queue);
                let (url, dark, script) = (frame.live.url.clone(), frame.live.dark, frame.live.script.clone());
                for (method, params) in [
                    ("Page.enable", json!({})),
                    ("Runtime.enable", json!({})),
                    ("Runtime.addBinding", json!({ "name": "__s5Frame" })),
                    ("Emulation.setEmulatedMedia", json!({ "features": [{ "name": "prefers-color-scheme", "value": if dark { "dark" } else { "light" } }] })),
                ] {
                    self.send(Some(&session), method, params, None);
                }
                if let Some(source) = script {
                    self.send(Some(&session), "Page.addScriptToEvaluateOnNewDocument", json!({ "source": source }), None);
                }
                self.live_view(id);
                self.send(Some(&session), "Page.navigate", json!({ "url": url }), None);
                for (method, params) in queue {
                    self.send(Some(&session), method, params, None);
                }
            }
            Reply::SnapContext(id) => {
                let Some(context) = r["browserContextId"].as_str().map(str::to_owned) else { return self.snap_finish(id, None) };
                let waiting = self.snaps.get_mut(&id).map(|snap| snap.context = Some(context.clone())).is_some();
                if waiting {
                    self.send(None, "Target.createTarget", json!({ "url": "about:blank", "browserContextId": context }), Some(Reply::SnapCreated(id)));
                } else {
                    self.send(None, "Target.disposeBrowserContext", json!({ "browserContextId": context }), None);
                }
            }
            Reply::SnapCreated(id) => match r["targetId"].as_str() {
                Some(target) => self.send(None, "Target.attachToTarget", json!({ "targetId": target, "flatten": true }), Some(Reply::SnapAttached(id))),
                None => self.snap_finish(id, None),
            },
            Reply::SnapAttached(id) => {
                let session = r["sessionId"].as_str().map(str::to_owned);
                let started = session.and_then(|session| {
                    let snap = self.snaps.get_mut(&id)?;
                    snap.session = Some(session.clone());
                    Some((session, snap.shot.url.clone(), snap.shot.width.max(1), snap.shot.dark, snap.shot.crop))
                });
                let Some((session, url, width, dark, crop)) = started else { return self.snap_finish(id, None) };
                let session = session.as_str();
                self.send(Some(session), "Page.enable", json!({}), None);
                // A cropped shot keeps the page's transparency, to sit on any surface.
                if crop {
                    self.send(Some(session), "Emulation.setDefaultBackgroundColorOverride", json!({ "color": { "r": 0, "g": 0, "b": 0, "a": 0 } }), None);
                }
                self.send(Some(session), "Emulation.setDeviceMetricsOverride", json!({ "width": width, "height": 80, "deviceScaleFactor": self.scale, "mobile": false }), None);
                let scheme = if dark { "dark" } else { "light" };
                self.send(Some(session), "Emulation.setEmulatedMedia", json!({ "features": [{ "name": "prefers-color-scheme", "value": scheme }, { "name": "prefers-reduced-motion", "value": "reduce" }] }), None);
                self.send(Some(session), "Page.navigate", json!({ "url": url }), Some(Reply::SnapNavigated(id)));
            }
            Reply::SnapNavigated(id) => {
                if !error.is_null() || r["errorText"].as_str().is_some_and(|e| !e.is_empty()) {
                    return self.snap_finish(id, None);
                }
                if let Some(snap) = self.snaps.get_mut(&id) {
                    snap.navigated = true;
                }
            }
            Reply::SnapMeasured(id) => {
                let content = r["result"]["value"].as_f64().filter(|_| r["exceptionDetails"].is_null());
                let size = content.and_then(|c| shot_size(c, self.scale));
                let page = self.snaps.get(&id).and_then(|n| Some((n.session.clone()?, n.shot.width.max(1))));
                let (Some((height, clip)), Some((session, width))) = (size, page) else { return self.snap_finish(id, None) };
                self.send(Some(&session), "Emulation.setDeviceMetricsOverride", json!({ "width": width, "height": height as u32, "deviceScaleFactor": self.scale, "mobile": false }), None);
                let clip = json!({ "x": 0, "y": 0, "width": width, "height": height, "scale": clip });
                self.send(Some(&session), "Page.captureScreenshot", json!({ "format": "png", "clip": clip }), Some(Reply::SnapShot(id, height)));
            }
            Reply::SnapShot(id, height) => {
                let shown = match self.snaps.get(&id) {
                    Some(n) if n.shot.crop => show_cropped(&n.shot, r["data"].as_str()),
                    Some(n) => show(&n.shot.pixels, r["data"].as_str()).then_some(height),
                    None => None,
                };
                self.snap_finish(id, shown);
            }
            Reply::Created(tab) => {
                let Some(target) = r["targetId"].as_str().map(str::to_owned) else { return };
                match self.tabs.get_mut(&tab) {
                    Some(t) => {
                        t.target = Some(target.clone());
                        self.send(None, "Target.attachToTarget", json!({ "targetId": target, "flatten": true }), Some(Reply::Attached(tab)));
                    }
                    // Closed before Chrome made it.
                    None => self.send(None, "Target.closeTarget", json!({ "targetId": target }), None),
                }
            }
            Reply::Attached(tab) => {
                let Some(session) = r["sessionId"].as_str().map(str::to_owned) else { return };
                let Some(t) = self.tabs.get_mut(&tab) else { return };
                t.session = Some(session.clone());
                let queue = std::mem::take(&mut t.queue);
                let target = t.target.clone();
                let host = self.host.clone();
                self.update(tab, |s| {
                    s.target = target;
                    s.host = Some(host);
                });
                self.sessions.insert(session.clone(), tab);
                self.send(Some(&session), "Page.enable", json!({}), None);
                self.send(Some(&session), "Network.enable", json!({}), None);
                // Bindings only reach pages while the Runtime domain is on.
                self.send(Some(&session), "Runtime.enable", json!({}), None);
                self.send(Some(&session), "Runtime.addBinding", json!({ "name": "__s5Audio" }), None);
                self.send(Some(&session), "Page.addScriptToEvaluateOnNewDocument", json!({ "source": AUDIO_JS, "runImmediately": true }), None);
                self.send(Some(&session), "Runtime.addBinding", json!({ "name": "__s5Page" }), None);
                self.send(Some(&session), "Page.addScriptToEvaluateOnNewDocument", json!({ "source": PAGE_JS, "runImmediately": true }), None);
                // A Chrome with its own PDF viewer shows PDFs itself; the headless shell has none,
                // so there a PDF page arrives as the bundled pdf.js.
                match self.native_pdf {
                    Some(native) => self.pdf_viewer(&session, native),
                    None => self.send(Some(&session), "Runtime.evaluate", json!({ "expression": "navigator.pdfViewerEnabled === true", "returnByValue": true }), Some(Reply::PdfViewer(session.clone()))),
                }
                self.metrics(tab);
                let scheme = self.tabs.get(&tab).map_or("", |t| t.scheme);
                if !scheme.is_empty() {
                    self.call(tab, "Emulation.setEmulatedMedia", json!({ "features": [{ "name": "prefers-color-scheme", "value": scheme }] }), None);
                }
                for (method, params, reply) in queue {
                    self.send(Some(&session), method, params, reply);
                }
                self.history(tab);
                if self.shown == Some(tab) {
                    self.screencast(tab);
                }
            }
            Reply::History(tab) => {
                let Some(t) = self.tabs.get_mut(&tab) else { return };
                t.history = r["entries"].as_array().into_iter().flatten().map(|e| (e["id"].as_i64().unwrap_or(0), e["url"].as_str().unwrap_or_default().to_string())).collect();
                t.index = r["currentIndex"].as_u64().unwrap_or(0) as usize;
                let (can_back, can_forward) = travel(&t.history, t.index);
                // The headless shell reports a target's title once, as its URL, before <title> parses.
                let title = r["entries"][t.index]["title"].as_str().map(str::to_owned);
                let url = r["entries"][t.index]["url"].as_str().filter(|u| u.starts_with("http")).map(str::to_owned);
                self.update(tab, |s| {
                    if let Some(url) = url.filter(|_| s.url.is_empty()) {
                        s.url = url;
                    }
                    s.can_back = can_back;
                    s.can_forward = can_forward;
                    if let Some(title) = title {
                        s.title = title;
                    }
                });
            }
            Reply::Frame(tab) => self.paint(tab, r["data"].as_str()),
            Reply::Cookie(job, host) => {
                if super::browser_import::written(&job, error.is_null(), &host) {
                    (self.waker)();
                }
            }
            Reply::Hover(tab) => {
                self.hovering = false;
                let rect = serde_json::from_value::<[f32; 4]>(r["result"]["value"].clone()).ok().filter(|r| r[2] > 0. && r[3] > 0.);
                self.update(tab, |s| s.hover = rect.filter(|_| s.picking));
                self.hover();
            }
            Reply::PickNode(tab) => match r["object"]["objectId"].as_str() {
                Some(object) => {
                    let params = json!({ "objectId": object, "functionDeclaration": PICK_JS, "returnByValue": true });
                    self.call(tab, "Runtime.callFunctionOn", params, Some(Reply::PickInfo(tab)));
                }
                None => self.command(Cmd::Pick(tab, false)),
            },
            Reply::PickInfo(tab) => {
                self.command(Cmd::Pick(tab, false));
                let Some(element) = r["result"]["value"].as_str().and_then(|v| serde_json::from_str::<Value>(v).ok()) else { return };
                match clip(&element) {
                    Some(clip) => self.call(tab, "Page.captureScreenshot", json!({ "format": "png", "clip": clip }), Some(Reply::PickShot(tab, element))),
                    None => self.event(Event::Picked { tab, element, image: None }),
                }
            }
            Reply::PickShot(tab, element) => {
                let image = r["data"].as_str().and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok());
                self.event(Event::Picked { tab, element, image });
            }
            Reply::Navigated(tab, url) => {
                let Some(text) = r["errorText"].as_str().filter(|e| !e.is_empty()) else { return };
                if net_error(text) == "ERR_ABORTED" {
                    let current = self.tabs.get(&tab).and_then(|t| t.history.get(t.index)).map(|(_, u)| u.clone());
                    return self.update(tab, |s| abandon(s, &url, current));
                }
                let description = net_error(text);
                self.update(tab, |s| {
                    s.loading = false;
                    s.failure = Some(Failure { url, description });
                });
            }
            Reply::RecCreated => {
                let Some(target) = r["targetId"].as_str().map(str::to_owned) else {
                    let e = error["message"].as_str().unwrap_or("Chrome could not open the encoder.").to_string();
                    return self.end_recording(Some(Err(e)));
                };
                match self.recording.as_mut() {
                    Some(rec) => {
                        rec.target = Some(target.clone());
                        self.send(None, "Target.attachToTarget", json!({ "targetId": target, "flatten": true }), Some(Reply::RecAttached));
                    }
                    None => self.send(None, "Target.closeTarget", json!({ "targetId": target }), None),
                }
            }
            Reply::RecAttached => {
                let Some(session) = r["sessionId"].as_str().map(str::to_owned) else { return };
                let Some(rec) = self.recording.as_mut() else { return };
                rec.session = Some(session.clone());
                for (expression, reply) in std::mem::take(&mut rec.queue) {
                    self.send(Some(&session), "Runtime.evaluate", json!({ "expression": expression, "awaitPromise": true, "returnByValue": true }), reply);
                }
            }
            Reply::RecSeed(width) => {
                if let Some(data) = r["data"].as_str() {
                    let data = data.to_string();
                    self.rec_frame(&data, width);
                }
            }
            Reply::RecStopped => {
                let value = &r["result"]["value"];
                let failed = r["exceptionDetails"]["exception"]["description"].as_str().or(error["message"].as_str());
                let count = value["count"].as_u64().unwrap_or(0) as usize;
                let mime = value["mimeType"].as_str().unwrap_or_default().to_string();
                if let Some(e) = failed {
                    return self.end_recording(Some(Err(e.to_string())));
                }
                if count == 0 || mime.is_empty() {
                    return self.end_recording(Some(Err("The recording captured no frames.".into())));
                }
                if let Some(rec) = self.recording.as_mut() {
                    rec.mime = mime;
                    rec.chunks = vec![None; count];
                }
                for index in 0..count {
                    self.rec_eval(format!("__s5Recorder.chunk({index})"), Some(Reply::RecChunk(index)));
                }
            }
            Reply::RecChunk(index) => {
                let bytes = r["result"]["value"].as_str().and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok());
                let Some(rec) = self.recording.as_mut() else { return };
                let Some(bytes) = bytes else {
                    return self.end_recording(Some(Err("The recording could not be read.".into())));
                };
                if let Some(slot) = rec.chunks.get_mut(index) {
                    *slot = Some(bytes);
                }
                if rec.chunks.iter().any(Option::is_none) {
                    return;
                }
                let data: Vec<u8> = rec.chunks.iter_mut().flat_map(|c| c.take().unwrap_or_default()).collect();
                let path = rec.path.clone().unwrap_or_default().with_extension(recording_extension(&rec.mime));
                let saved = path
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|_| std::fs::write(&path, data))
                    .map(|_| path)
                    .map_err(|e| e.to_string());
                self.end_recording(Some(saved));
            }
            Reply::Track(tab) => {
                self.track_busy = false;
                if self.tracking != Some(tab) {
                    return;
                }
                let rect = serde_json::from_value::<[f32; 4]>(r["result"]["value"].clone()).ok();
                self.update(tab, |s| s.picked = rect);
            }
            Reply::Context(tab, at) => {
                let info = r["result"]["value"].clone();
                if info.is_object() {
                    self.event(Event::Context { tab, at, info });
                }
            }
            Reply::Selection(tab, cut) => {
                let text = r["result"]["value"].as_str().unwrap_or_default().to_string();
                if text.is_empty() {
                    return;
                }
                if cut {
                    self.call(tab, "Runtime.evaluate", json!({ "expression": "document.execCommand('delete')" }), None);
                }
                self.event(Event::Clipboard(text));
            }
            Reply::KeyPress(label) => {
                if r["result"]["value"] == false {
                    self.rec_eval(format!("__s5Recorder.key({})", Value::String(label)), None);
                }
            }
            Reply::Favicon(tab) => {
                let icon = r["result"]["value"].as_str().filter(|u| u.len() <= 1 << 20 && (u.starts_with("http") || u.starts_with("data:image/"))).map(str::to_owned);
                self.update(tab, |s| s.favicon = icon);
            }
            Reply::MuteScript(tab) => {
                let Some(identifier) = r["identifier"].as_str().map(str::to_owned) else { return };
                if self.muted(tab) {
                    if let Some(t) = self.tabs.get_mut(&tab) {
                        t.mute_script = Some(identifier);
                    }
                } else {
                    // Unmuted before Chrome answered.
                    self.call(tab, "Page.removeScriptToEvaluateOnNewDocument", json!({ "identifier": identifier }), None);
                }
            }
            Reply::Erased(tab) => {
                let index = r["result"]["value"].as_u64().map(|i| i as usize);
                self.event(Event::Erased(tab, index));
            }
            Reply::PdfViewer(session) => {
                let native = r["result"]["value"].as_bool().unwrap_or(false);
                self.native_pdf = Some(native);
                self.pdf_viewer(&session, native);
            }
            Reply::Enclosed(tab, rect) => {
                let elements = r["result"]["value"].as_str().and_then(|v| serde_json::from_str::<Vec<Value>>(v).ok()).unwrap_or_default();
                self.event(Event::Enclosed(tab, rect, elements));
            }
            Reply::MarksBounds(tab) => match marks_clip(&r["result"]["value"]) {
                Some(clip) => self.call(tab, "Page.captureScreenshot", json!({ "format": "png", "clip": clip }), Some(Reply::MarksShot(tab))),
                None => self.event(Event::Marked(tab, None)),
            },
            Reply::MarksShot(tab) => {
                let image = r["data"].as_str().and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok());
                self.event(Event::Marked(tab, image));
            }
            Reply::Image => {
                if let Some(bytes) = r["data"].as_str().and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok()) {
                    self.event(Event::Image(bytes));
                }
            }
            Reply::Saved(path) => {
                let saved = r["data"]
                    .as_str()
                    .ok_or_else(|| error["message"].as_str().unwrap_or("Chrome returned no image.").to_string())
                    .and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).map_err(|e| e.to_string()))
                    .and_then(|bytes| {
                        if let Some(dir) = path.parent() {
                            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                        }
                        std::fs::write(&path, bytes).map_err(|e| e.to_string())
                    })
                    .map(|_| path);
                self.event(Event::Screenshot(saved));
            }
        }
    }

    /// A page's new window, as Electron's `setWindowOpenHandler` treats it: a popup stays a live
    /// window (here a tab) so its opener survives; a tab loads afresh; others stay in place or go.
    fn window_opened(&mut self, tab: TabId, target: String, opener: String, url: String) {
        let waiting = self.opening.iter().position(|o| o.0 == tab && o.3.elapsed() < Duration::from_secs(2)).map(|i| self.opening.remove(i));
        // Electron's popup windows deny every window open of their own.
        let action = if self.popups.contains(&opener) { Some((url.clone(), Opened::Deny)) } else { waiting.map(|(_, url, action, _)| (url, action)) };
        let closes = matches!(action, Some((_, Opened::Tab | Opened::Navigate | Opened::Deny)));
        if closes {
            self.send(None, "Target.closeTarget", json!({ "targetId": target }), None);
        }
        match action {
            // No `windowOpen` said what it is: a tab beside its opener, as before Chrome reported it.
            None => self.event(Event::Popup { target, opener, url }),
            Some((_, Opened::Popup)) => {
                self.popups.insert(target.clone());
                self.event(Event::Popup { target, opener, url });
            }
            Some((url, Opened::Tab)) => self.event(Event::OpenLink { tab, url, background: false }),
            Some((url, Opened::Navigate)) if url.starts_with("blob:") => self.command(Cmd::Navigate(tab, url)),
            Some((url, Opened::Navigate)) => self.event(Event::External { tab, url }),
            Some((_, Opened::Deny)) => {}
        }
    }

    /// A call on a live frame's page, held until it attaches.
    fn live_call(&mut self, id: u64, method: &'static str, params: Value) {
        let Some(frame) = self.lives.get_mut(&id) else { return };
        match frame.session.clone() {
            Some(session) => self.send(Some(&session), method, params, None),
            None => frame.queue.push((method, params)),
        }
    }

    /// Sizes a live frame's page and its screencast to the row.
    fn live_view(&mut self, id: u64) {
        let Some(frame) = self.lives.get(&id) else { return };
        let (w, h, scale) = (frame.live.width.max(1), frame.live.height.max(1), self.scale);
        self.live_call(id, "Emulation.setDeviceMetricsOverride", json!({ "width": w, "height": h, "deviceScaleFactor": scale, "mobile": false }));
        self.live_call(id, "Page.stopScreencast", json!({}));
        let (mw, mh) = ((w as f32 * scale).round() as u32, (h as f32 * scale).round() as u32);
        self.live_call(id, "Page.startScreencast", json!({ "format": "jpeg", "quality": 92, "maxWidth": mw, "maxHeight": mh }));
    }

    /// Measures a snapshot's page, once it loaded or took too long to.
    fn snap_measure(&mut self, id: u64) {
        let Some(snap) = self.snaps.get_mut(&id).filter(|n| !n.measuring) else { return };
        let Some(session) = snap.session.clone() else { return };
        snap.measuring = true;
        self.send(Some(&session), "Runtime.evaluate", json!({ "expression": SHOT_MEASURE_JS, "awaitPromise": true, "returnByValue": true }), Some(Reply::SnapMeasured(id)));
    }

    /// Ends a snapshot: its context goes, with every page it opened, and the waiting row wakes.
    fn snap_finish(&mut self, id: u64, height: Option<f32>) {
        let Some(snap) = self.snaps.remove(&id) else { return };
        if let Some(context) = snap.context {
            self.send(None, "Target.disposeBrowserContext", json!({ "browserContextId": context }), None);
        }
        snap.shot.finish(height);
        (self.waker)();
    }

    /// A page that never fires `load` is shot as it stands after 10 s; one that can't be
    /// measured fails after 30 s.
    fn expire_snaps(&mut self) {
        if self.snaps.is_empty() {
            return;
        }
        let late: Vec<(u64, bool)> = self
            .snaps
            .iter()
            .filter_map(|(id, n)| {
                let age = n.started.elapsed();
                if age > Duration::from_secs(30) {
                    Some((*id, false))
                } else {
                    (age > Duration::from_secs(10) && n.session.is_some() && !n.measuring).then_some((*id, true))
                }
            })
            .collect();
        for (id, measure) in late {
            if measure { self.snap_measure(id) } else { self.snap_finish(id, None) }
        }
    }

    fn paint(&mut self, tab: TabId, data: Option<&str>) {
        let Some(pixels) = self.tabs.get(&tab).and_then(|t| t.pixels.clone()) else { return };
        if show(&pixels, data) {
            self.update(tab, |s| s.painted = true);
        }
    }
}

/// The CSS viewport for a page area of `w` x `h` points. Zoom only shrinks it: emulating any
/// scale but the one Chrome started with makes the headless shell crop or blur its frames.
fn metrics(w: f32, h: f32, zoom: f32) -> (u32, u32) {
    let zoom = zoom.max(0.25);
    (((w / zoom).round() as u32).max(1), ((h / zoom).round() as u32).max(1))
}

/// Where an emulated `dw` x `dh` CSS viewport shows in a `w` x `h` page area, as web's
/// `resolveBrowserViewportLayout`: zoomed, then shrunk to fit but never enlarged. Returns the
/// box's width and height in points and its points per CSS pixel.
/// Web's `BROWSER_VIEWPORT_RESIZE_RAIL_SIZE`: rails beside and below a device-size page.
pub const RAIL: f32 = 10.;

/// The room a device-size page has in the page area, beside its resize rails.
pub fn device_area(w: f32, h: f32) -> (f32, f32) {
    ((w - RAIL * 2.).max(1.), (h - RAIL).max(1.))
}

pub fn device_box(w: f32, h: f32, (dw, dh): (u32, u32), zoom: f32) -> (f32, f32, f32) {
    let zoom = zoom.max(0.25);
    let (rw, rh) = (dw.max(1) as f32 * zoom, dh.max(1) as f32 * zoom);
    let fit = if w < 1. || h < 1. { 1. } else { (w / rw).min(h / rh).min(1.) };
    (rw * fit, rh * fit, zoom * fit)
}

/// The page's own DevTools frontend: the bundled one Chrome serves, else the hosted copy for
/// the headless shell, which bundles none.
fn devtools_url(host: &str, target: &str) -> String {
    let ws = format!("{host}/devtools/page/{target}");
    if http_get(host, "/devtools/inspector.html").is_some_and(|(status, _)| status == 200) {
        return format!("http://{host}/devtools/inspector.html?ws={ws}");
    }
    let revision = http_get(host, "/json/version")
        .and_then(|(_, body)| serde_json::from_str::<Value>(&body).ok())
        .and_then(|v| v["WebKit-Version"].as_str().and_then(|s| s.split("(@").nth(1)).map(|s| s.trim_end_matches(')').to_string()));
    match revision {
        Some(rev) => format!("https://chrome-devtools-frontend.appspot.com/serve_rev/@{rev}/inspector.html?ws={ws}"),
        None => format!("http://{host}/devtools/inspector.html?ws={ws}"),
    }
}

fn http_get(host: &str, path: &str) -> Option<(u16, String)> {
    use std::io::{Read, Write};
    let mut stream = TcpStream::connect(host).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    write!(stream, "GET {path} HTTP/1.0\r\nHost: {host}\r\n\r\n").ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    let status = response.split_whitespace().nth(1)?.parse().ok()?;
    let body = response.split_once("\r\n\r\n").map_or("", |(_, b)| b).to_string();
    Some((status, body))
}

/// The screenshot clip for a picked element, in document CSS pixels; none for an empty box.
fn clip(element: &Value) -> Option<Value> {
    let (x, y) = (element["doc"]["x"].as_f64()?, element["doc"]["y"].as_f64()?);
    let (w, h) = (element["rect"]["width"].as_f64()?.min(4096.), element["rect"]["height"].as_f64()?.min(4096.));
    (w >= 1. && h >= 1.).then(|| json!({ "x": x.max(0.), "y": y.max(0.), "width": w, "height": h, "scale": 1 }))
}

/// DevTools' element highlight colours.
static HIGHLIGHT: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| {
    json!({
        "showInfo": true,
        "showStyles": false,
        "contentColor": { "r": 111, "g": 168, "b": 220, "a": 0.66 },
        "paddingColor": { "r": 147, "g": 196, "b": 125, "a": 0.55 },
        "borderColor": { "r": 255, "g": 229, "b": 153, "a": 0.66 },
        "marginColor": { "r": 246, "g": 178, "b": 107, "a": 0.66 },
    })
});

/// Runs on the picked element: what web's pick preload reports (`PickedElementPayload`), plus
/// its viewport rect and document offset for the crop.
const PICK_JS: &str = r##"function () {
  const el = this;
  const esc = (s) => (window.CSS && CSS.escape ? CSS.escape(s) : s);
  const selector = (start) => {
    const parts = [];
    for (let n = start; n && n.nodeType === 1 && parts.length < 5; n = n.parentElement) {
      if (n.id) { parts.unshift("#" + esc(n.id)); break; }
      let part = n.tagName.toLowerCase() + [...n.classList].slice(0, 2).map((c) => "." + esc(c)).join("");
      const parent = n.parentElement;
      if (parent) {
        const same = [...parent.children].filter((c) => c.tagName === n.tagName);
        if (same.length > 1) part += ":nth-of-type(" + (same.indexOf(n) + 1) + ")";
      }
      parts.unshift(part);
    }
    return parts.join(" > ") || null;
  };
  const cs = getComputedStyle(el);
  const props = ["display", "position", "width", "height", "margin", "padding", "color", "background-color",
    "font-family", "font-size", "font-weight", "line-height", "border", "border-radius", "gap",
    "flex-direction", "align-items", "justify-content"];
  const styles = props.map((p) => p + ": " + cs.getPropertyValue(p)).join(";\n");
  let componentName = null;
  const fiberKey = Object.keys(el).find((k) => k.startsWith("__reactFiber$"));
  for (let f = fiberKey ? el[fiberKey] : null; f; f = f.return) {
    if (typeof f.type === "function") { componentName = f.type.displayName || f.type.name || null; break; }
  }
  const r = el.getBoundingClientRect();
  window.__s5Picked = el;
  return JSON.stringify({
    pageUrl: location.href,
    pageTitle: (document.title || "").trim() || null,
    tagName: el.tagName.toLowerCase(),
    selector: selector(el),
    htmlPreview: el.outerHTML.slice(0, 500),
    componentName,
    source: null,
    styles,
    rect: { x: r.x, y: r.y, width: r.width, height: r.height },
    doc: { x: r.x + scrollX, y: r.y + scrollY },
  });
}"##;

/// Desktop's `selectElementsInRect`: the leaves, buttons and links whose centre the region holds,
/// smallest first and at most `MAX_MARQUEE_ELEMENTS`, each described as a pick describes it.
const ENCLOSED_JS: &str = r##"(x, y, w, h, pick) => {
  const marks = document.querySelector("[data-s5-annotation]");
  const found = [...document.querySelectorAll("body *")]
    .filter((el) => !(marks && marks.contains(el)))
    .map((el) => ({ el, r: el.getBoundingClientRect() }))
    .filter(({ r }) => r.width >= 2 && r.height >= 2 && !(r.right < x || r.left > x + w || r.bottom < y || r.top > y + h))
    .filter(({ el, r }) => {
      const cx = r.left + r.width / 2, cy = r.top + r.height / 2;
      return cx >= x && cx <= x + w && cy >= y && cy <= y + h &&
        (el.children.length === 0 || el instanceof HTMLButtonElement || el instanceof HTMLAnchorElement || el.getAttribute("role") === "button");
    })
    .sort((a, b) => a.r.width * a.r.height - b.r.width * b.r.height)
    .slice(0, 20);
  return JSON.stringify(found.map(({ el }) => JSON.parse(pick.call(el))));
}"##;

/// `recordingKeyLabel` for an unmodified key: arrows and symbols for named keys, letters upper.
fn key_label(key: &str) -> String {
    match key {
        "Enter" => "↵".into(),
        "Tab" => "⇥".into(),
        "Backspace" => "⌫".into(),
        "Delete" => "⌦".into(),
        "Escape" => "Esc".into(),
        "ArrowUp" => "↑".into(),
        "ArrowDown" => "↓".into(),
        "ArrowLeft" => "←".into(),
        "ArrowRight" => "→".into(),
        " " => "Space".into(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => k.into(),
    }
}

/// `MediaRecorder`'s format as the saved file's extension, as Electron names recordings.
fn recording_extension(mime: &str) -> &'static str {
    if mime.starts_with("video/mp4") { "mp4" } else { "webm" }
}

/// Where Chrome writes downloads, each named by its guid, before they move to Downloads.
fn downloads_staging() -> String {
    crate::host::t3_home().join("userdata").join("browser-downloads").display().to_string()
}

pub(super) fn downloads_dir() -> PathBuf {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let configured = std::fs::read_to_string(home.join(".config/user-dirs.dirs")).ok().and_then(|text| {
        let line = text.lines().find(|l| l.starts_with("XDG_DOWNLOAD_DIR="))?;
        let value = line.split_once('=')?.1.trim().trim_matches('"').replace("$HOME", &home.display().to_string());
        Some(PathBuf::from(value))
    });
    configured.unwrap_or_else(|| home.join("Downloads"))
}

/// The first free name in `dir`: `name`, then `name (1).ext` up to 99, as Electron's copy.
pub(super) fn unique_path(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
    let name = std::path::Path::new(name).file_name().map_or_else(|| "download".into(), |n| n.to_string_lossy().into_owned());
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(".{ext}")),
        _ => (name.clone(), String::new()),
    };
    std::iter::once(name)
        .chain((1..=99).map(|n| format!("{stem} ({n}){ext}")))
        .map(|n| dir.join(n))
        .find(|p| !p.exists())
}

/// Saves bytes to the Downloads folder under the first free name, as a person's download.
pub fn save_bytes(name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let staging = std::path::Path::new(&downloads_staging()).join(format!("save-{}-{n}", std::process::id()));
    std::fs::create_dir_all(staging.parent().unwrap_or(&staging)).map_err(|e| e.to_string())?;
    std::fs::write(&staging, bytes).map_err(|e| e.to_string())?;
    save_download(&staging, &downloads_dir(), name)
}

fn save_download(from: &std::path::Path, dir: &std::path::Path, name: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let to = unique_path(dir, name).ok_or_else(|| format!("{name} already exists in {}.", dir.display()))?;
    if std::fs::rename(from, &to).is_err() {
        std::fs::copy(from, &to).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(from);
    }
    Ok(to)
}

/// The encoder page: a canvas fed the tab's JPEG frames and recorded by `MediaRecorder`, as the
/// server's `recordingEncoderScript`, drawing the person's arrow where web's recording cursor is.
/// The page's icon as Electron's `page-favicon-updated` names it, else the origin's default.
const FAVICON_JS: &str = r#"(() => {
  if (!/^https?:$/.test(location.protocol)) return null;
  const link = [...document.querySelectorAll('link[rel~="icon" i]')].map((l) => l.href).find((h) => /^(https?:|data:image\/)/.test(h));
  return link || location.origin + '/favicon.ico';
})()"#;

/// Reports whether the top page's media is playing sound and mutes it on `__s5Mute`, as a
/// headless Chrome has neither a tab audio indicator nor a mute switch.
/// The screenshot clip around the marks' document-pixel bounds, padded a little as web's crop.
fn marks_clip(bounds: &Value) -> Option<Value> {
    let [x, y, w, h] = ["x", "y", "width", "height"].map(|k| bounds[k].as_f64());
    let (x, y, w, h) = (x?, y?, w?, h?);
    let pad = 8.;
    let (left, top) = ((x - pad).max(0.), (y - pad).max(0.));
    let (w, h) = ((x + w + pad - left).min(4096.), (y + h + pad - top).min(4096.));
    (w >= 1. && h >= 1.).then(|| json!({ "x": left, "y": top, "width": w, "height": h, "scale": 1 }))
}

/// The annotation's marks in the page: element boxes, regions and strokes positioned in document
/// pixels (callers pass viewport points), on a layer that ignores the pointer.
const MARKS_JS: &str = r##"window.__s5Marks = window.__s5Marks || (() => {
  const ns = "http://www.w3.org/2000/svg";
  const color = "#2563eb";
  let root = null, svg = null, marks = [], active = null;
  const ensure = () => {
    if (root && root.isConnected) return;
    root = document.createElement("div");
    root.setAttribute("data-s5-annotation", "");
    root.style.cssText = "position:absolute;left:0;top:0;width:0;height:0;overflow:visible;pointer-events:none;z-index:2147483647";
    svg = document.createElementNS(ns, "svg");
    svg.setAttribute("width", "1");
    svg.setAttribute("height", "1");
    svg.style.cssText = "position:absolute;left:0;top:0;overflow:visible";
    root.append(svg);
    document.documentElement.append(root);
  };
  const box = (r, fill) => {
    ensure();
    const node = document.createElement("div");
    node.style.cssText = `position:absolute;left:${r.x}px;top:${r.y}px;width:${r.width}px;height:${r.height}px;border:2px solid ${color};border-radius:3px;background:${fill};box-sizing:border-box`;
    root.append(node);
    return node;
  };
  const rect = (x, y, w, h) => ({ x: x + scrollX, y: y + scrollY, width: w, height: h });
  const draw = (m) => m.node.setAttribute("d", m.points.map(([x, y], i) => (i ? "L" : "M") + x + " " + y).join(" "));
  const strokeRect = (points) => {
    const xs = points.map((p) => p[0]), ys = points.map((p) => p[1]);
    const [l, t] = [Math.min(...xs) - 2, Math.min(...ys) - 2];
    return { x: l, y: t, width: Math.max(...xs) + 2 - l, height: Math.max(...ys) + 2 - t };
  };
  return {
    element(x, y, w, h) { const r = rect(x, y, w, h); marks.push({ rect: r, node: box(r, "rgba(37,99,235,0.1)") }); },
    region(x, y, w, h) { const r = rect(x, y, w, h); marks.push({ rect: r, node: box(r, "rgba(37,99,235,0.06)") }); },
    begin(x, y) {
      ensure();
      const node = document.createElementNS(ns, "path");
      for (const [k, v] of [["fill", "none"], ["stroke", color], ["stroke-width", "4"], ["stroke-linecap", "round"], ["stroke-linejoin", "round"]]) node.setAttribute(k, v);
      svg.append(node);
      active = { points: [[x + scrollX, y + scrollY]], node };
      draw(active);
    },
    point(x, y) { if (!active) return; active.points.push([x + scrollX, y + scrollY]); draw(active); },
    end() {
      if (!active) return;
      if (active.points.length > 1) marks.push({ rect: strokeRect(active.points), node: active.node });
      else active.node.remove();
      active = null;
    },
    erase(x, y) {
      const [px, py] = [x + scrollX, y + scrollY];
      for (let i = marks.length - 1; i >= 0; i--) {
        const r = marks[i].rect;
        if (px >= r.x && px <= r.x + r.width && py >= r.y && py <= r.y + r.height) {
          marks[i].node.remove();
          marks.splice(i, 1);
          return i;
        }
      }
      return null;
    },
    bounds() {
      if (!marks.length) return null;
      const l = Math.min(...marks.map((m) => m.rect.x)), t = Math.min(...marks.map((m) => m.rect.y));
      const r = Math.max(...marks.map((m) => m.rect.x + m.rect.width)), b = Math.max(...marks.map((m) => m.rect.y + m.rect.height));
      return { x: l, y: t, width: r - l, height: b - t };
    },
    clear() { if (root) root.remove(); root = null; svg = null; marks = []; active = null; },
  };
})()"##;

/// Whether a paused response is a PDF, by its `Content-Type`.
fn pdf_response(headers: &Value) -> bool {
    headers.as_array().into_iter().flatten().any(|h| h["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case("content-type")) && h["value"].as_str().is_some_and(|v| v.trim().to_ascii_lowercase().starts_with("application/pdf")))
}

/// The page a PDF shows as, at the PDF's own address: pdf.js draws each page, fetching the
/// bytes from that address again (same origin, so cookies and signed links still work).
fn pdf_viewer(url: &str) -> String {
    let name = url.split(['?', '#']).next().unwrap_or_default().rsplit('/').next().unwrap_or_default();
    let name = s5_net::url::form_urlencoded::parse(format!("n={}", name.replace('+', "%2B")).as_bytes()).next().map_or_else(String::new, |(_, n)| n.into_owned());
    // `{` too, so a file name can't stand in for the placeholders after it.
    let escape = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('{', "&#123;");
    PDF_VIEWER.replace("{title}", &escape(&name)).replacen("{pdfjs}", PDFJS, 1).replacen("{worker}", PDFJS_WORKER, 1)
}

// pdf.js 3.11.174 (Apache-2.0, `assets/pdfjs/LICENSE`), bundled as Electron bundles its viewer so
// PDFs show offline. The `.txt` names keep the repo's JS lint and formatter off minified code.
const PDFJS: &str = include_str!("../../assets/pdfjs/pdf.min.js.txt");
const PDFJS_WORKER: &str = include_str!("../../assets/pdfjs/pdf.worker.min.js.txt");

const PDF_VIEWER: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>{title}</title>
<style>
html,body{margin:0;background:#525659;color:#fff;font:13px system-ui,sans-serif}
#pages{display:flex;flex-direction:column;align-items:center;gap:8px;padding:8px}
canvas{background:#fff;box-shadow:0 1px 4px rgba(0,0,0,.45);max-width:100%;height:auto!important}
#status{padding:16px;text-align:center}
</style>
<script>{pdfjs}</script>
<script type="text/plain" id="pdfjs-worker">{worker}</script>
</head><body><div id="status">Loading PDF…</div><div id="pages"></div>
<script>
(async () => {
  const status = document.getElementById("status");
  const lib = window.pdfjsLib;
  if (!lib) { status.textContent = "The PDF viewer could not load."; return; }
  const worker = new Blob([document.getElementById("pdfjs-worker").textContent], { type: "text/javascript" });
  lib.GlobalWorkerOptions.workerSrc = URL.createObjectURL(worker);
  try {
    const doc = await lib.getDocument({ url: location.href, withCredentials: true }).promise;
    status.remove();
    const pages = document.getElementById("pages");
    const ratio = window.devicePixelRatio || 1;
    for (let n = 1; n <= doc.numPages; n++) {
      const page = await doc.getPage(n);
      const css = Math.min(1.5, (innerWidth - 16) / page.getViewport({ scale: 1 }).width);
      const viewport = page.getViewport({ scale: css * ratio });
      const canvas = document.createElement("canvas");
      canvas.width = Math.floor(viewport.width);
      canvas.height = Math.floor(viewport.height);
      canvas.style.width = Math.floor(viewport.width / ratio) + "px";
      pages.append(canvas);
      await page.render({ canvasContext: canvas.getContext("2d"), viewport }).promise;
    }
  } catch (error) {
    status.textContent = "Unable to show this PDF: " + (error && error.message ? error.message : error);
  }
})();
</script></body></html>"#;

/// What the page script reports: a copy, a clipboard read to answer, or an external link.
fn page_message(tab: TabId, payload: &str) -> Option<Event> {
    let m: Value = serde_json::from_str(payload).ok()?;
    match m["kind"].as_str()? {
        "copy" => m["text"].as_str().filter(|t| !t.is_empty()).map(|t| Event::Clipboard(t.to_owned())),
        "read" => Some(Event::ClipboardRead { tab, id: m["id"].as_u64()?, origin: m["origin"].as_str().filter(|o| !o.is_empty() && *o != "null")?.to_owned() }),
        "external" => {
            let url = s5_net::url::Url::parse(m["url"].as_str()?).ok()?;
            let never = ["about", "blob", "chrome", "data", "devtools", "file", "filesystem", "http", "https", "javascript", "view-source"];
            (!never.contains(&url.scheme())).then(|| Event::External { tab, url: url.to_string() })
        }
        _ => None,
    }
}

/// Electron's clipboard and external-protocol hooks, which the headless shell lacks: copies
/// reach the system clipboard, reads ask first, and links to other applications are confirmed.
const PAGE_JS: &str = r#"(() => {
  if (window.__s5PageHooks) return;
  window.__s5PageHooks = true;
  const send = (m) => { try { __s5Page(JSON.stringify(m)); } catch {} };
  const clip = navigator.clipboard;
  if (clip) {
    const writeText = clip.writeText && clip.writeText.bind(clip);
    clip.writeText = (text) => { send({ kind: "copy", text: String(text) }); return writeText ? writeText(text).catch(() => {}) : Promise.resolve(); };
    const write = clip.write && clip.write.bind(clip);
    clip.write = async (items) => {
      for (const item of items || []) {
        if (item.types && item.types.includes("text/plain")) { send({ kind: "copy", text: await (await item.getType("text/plain")).text() }); break; }
      }
      return write ? write(items).catch(() => {}) : undefined;
    };
    let next = 0;
    const waiting = new Map();
    window.__s5Resolve = (id, text) => {
      const w = waiting.get(id);
      waiting.delete(id);
      if (w) text === null ? w.reject(new DOMException("Read permission denied.", "NotAllowedError")) : w.resolve(text);
    };
    clip.readText = () => new Promise((resolve, reject) => { const id = ++next; waiting.set(id, { resolve, reject }); send({ kind: "read", id, origin: location.origin }); });
  }
  let set = null;
  const setData = DataTransfer.prototype.setData;
  DataTransfer.prototype.setData = function (type, value) { if (/^text(\/plain)?$/i.test(type)) set = String(value); return setData.call(this, type, value); };
  for (const type of ["copy", "cut"]) {
    addEventListener(type, () => { set = null; }, true);
    addEventListener(type, (e) => { const text = e.defaultPrevented ? set : String(getSelection() || ""); if (text) send({ kind: "copy", text }); });
  }
  // Electron's window-open handler returns no window for a scheme it denies or keeps in place.
  const open = window.open;
  window.open = function (url, ...rest) {
    let to;
    try { to = new URL(url === undefined || url === null || url === "" ? "about:blank" : String(url), document.baseURI); } catch { return null; }
    if (/^(about|chrome|data|devtools|file|filesystem|javascript|view-source):$/.test(to.protocol)) return null;
    if (to.protocol === "blob:") { location.assign(to.href); return null; }
    if (!/^https?:$/.test(to.protocol)) { send({ kind: "external", url: to.href }); return null; }
    return open.call(this, url, ...rest);
  };
  addEventListener("click", (e) => {
    if (e.defaultPrevented || e.button !== 0) return;
    const a = e.target instanceof Element ? e.target.closest("a[href]") : null;
    if (!a || /^(https?|about|blob|chrome|data|devtools|file|filesystem|javascript|view-source):/i.test(a.href)) return;
    e.preventDefault();
    send({ kind: "external", url: a.href });
  });
})()"#;

const AUDIO_JS: &str = r#"(() => {
  if (window !== top || window.__s5Mute) return;
  const mine = new WeakSet();
  let last = false;
  const media = () => document.querySelectorAll('audio,video');
  const report = () => {
    const now = [...media()].some((m) => !m.paused && !m.ended && m.volume > 0 && (!m.muted || mine.has(m)));
    if (now !== last) { last = now; try { __s5Audio(now ? '1' : '0'); } catch {} }
  };
  const quiet = (m) => { if (window.__s5Muted && !m.muted) { m.muted = true; mine.add(m); } };
  addEventListener('play', (e) => { quiet(e.target); report(); }, true);
  for (const type of ['playing', 'pause', 'ended', 'volumechange', 'emptied']) addEventListener(type, report, true);
  window.__s5Mute = (on) => {
    window.__s5Muted = on;
    for (const m of media()) on ? quiet(m) : mine.delete(m) && (m.muted = false);
    report();
  };
})()"#;

const ENCODER_JS: &str = r##"(() => {
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("2d");
  const chunks = [];
  const ARROW = new Path2D("M1 1v18l4-4 4 8 3-1.5-4-8H15Z");
  let recorder = null, started = null, stopped = null, frame = null, cursor = null, ring = null, key = null, timer = null;
  // Web's compositor: a click ring that fades 600 ms after release, a key badge for 900 ms.
  const animate = () => {
    clearInterval(timer);
    timer = setInterval(() => {
      const now = performance.now();
      paint();
      const ringDone = !ring || (!ring.held && now > ring.releasedAt + 600);
      if (ringDone && (!key || now > key.until)) { ring = null; key = null; clearInterval(timer); paint(); }
    }, 1000 / __FPS__);
  };
  const paint = () => {
    if (!frame) return;
    const fit = Math.min(canvas.width / frame.width, canvas.height / frame.height);
    const width = frame.width * fit, height = frame.height * fit;
    const x = (canvas.width - width) / 2, y = (canvas.height - height) / 2;
    if (width < canvas.width || height < canvas.height) { context.fillStyle = "#000"; context.fillRect(0, 0, canvas.width, canvas.height); }
    context.drawImage(frame, x, y, width, height);
    if (cursor) {
      const s = fit * frame.cssScale;
      context.save();
      context.translate(x + cursor.x * s, y + cursor.y * s);
      context.scale(s, s);
      context.lineWidth = 1.5;
      context.lineJoin = "round";
      context.fillStyle = "#000";
      context.strokeStyle = "#fff";
      context.fill(ARROW);
      context.stroke(ARROW);
      context.restore();
    }
    const now = performance.now();
    const s = fit * frame.cssScale;
    if (ring && (ring.held || now < ring.releasedAt + 600)) {
      const progress = ring.held ? 0 : Math.min(1, (now - ring.releasedAt) / 600);
      const opacity = 0.9 * (1 - progress);
      context.save();
      context.strokeStyle = context.fillStyle = "#3b82f6";
      context.lineWidth = 2 * s;
      context.beginPath();
      context.arc(x + ring.x * s, y + ring.y * s, 20 * s * (1 + progress * 0.5), 0, Math.PI * 2);
      context.globalAlpha = opacity * 0.15;
      context.fill();
      context.globalAlpha = opacity;
      context.stroke();
      context.restore();
    }
    if (key && now < key.until) {
      context.save();
      context.font = `500 ${26 * s}px system-ui, sans-serif`;
      const w = Math.min(width - 32 * s, context.measureText(key.label).width + 36 * s), h = 54 * s;
      const left = x + (width - w) / 2, top = y + height - 24 * s - h;
      context.fillStyle = "rgba(32,32,34,.86)";
      context.beginPath();
      context.roundRect(left, top, w, h, 14 * s);
      context.fill();
      context.fillStyle = "white";
      context.textAlign = "center";
      context.textBaseline = "middle";
      context.fillText(key.label, x + width / 2, top + h / 2, w - 24 * s);
      context.restore();
    }
  };
  window.__s5Recorder = {
    async frame(base64, cssWidth) {
      const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
      const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/jpeg" }));
      if (!recorder) {
        canvas.width = bitmap.width;
        canvas.height = bitmap.height;
        const mimeType = ["video/mp4;codecs=avc1", "video/webm;codecs=vp9", "video/webm;codecs=vp8", "video/webm"].find((t) => MediaRecorder.isTypeSupported(t));
        const bitsPerSecond = Math.min(50e6, Math.max(2.5e6, bitmap.width * bitmap.height * __FPS__ * 0.05));
        recorder = new MediaRecorder(canvas.captureStream(__FPS__), { mimeType, videoBitsPerSecond: bitsPerSecond });
        started = new Promise((resolve, reject) => { recorder.onstart = resolve; recorder.onerror = (e) => reject(e.error); });
        stopped = new Promise((resolve) => { recorder.onstop = resolve; });
        recorder.ondataavailable = (e) => { if (e.data.size > 0) chunks.push(e.data); };
        recorder.start(1000);
      }
      bitmap.cssScale = bitmap.width / cssWidth;
      frame?.close();
      frame = bitmap;
      paint();
      await started;
      return true;
    },
    cursor(x, y) {
      cursor = { x, y };
      paint();
    },
    press(x, y, held) {
      ring = held ? { x, y, held: true, releasedAt: 0 } : { x, y, held: false, releasedAt: performance.now() };
      animate();
    },
    key(label) {
      key = { label, until: performance.now() + 900 };
      animate();
    },
    async stop() {
      if (!recorder) return { mimeType: null, count: 0 };
      if (recorder.state !== "inactive") {
        // Wait for the last paint to reach the stream; a stop can otherwise beat it.
        const video = document.createElement("video");
        video.muted = true;
        video.srcObject = recorder.stream;
        const next = () => Promise.race([new Promise((r) => video.requestVideoFrameCallback(r)), stopped]);
        const initial = next();
        void video.play().catch(() => {});
        await initial;
        if (recorder.state !== "inactive") {
          const delivered = next();
          recorder.stream.getVideoTracks()[0].requestFrame();
          paint();
          await delivered;
        }
        video.pause();
        video.srcObject = null;
        if (recorder.state !== "inactive") recorder.stop();
      }
      await stopped;
      return { mimeType: recorder.mimeType, count: chunks.length };
    },
    async chunk(index) {
      const bytes = new Uint8Array(await chunks[index].arrayBuffer());
      let binary = "";
      for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
      return btoa(binary);
    },
  };
})()"##;

/// What a right-click lands on, for the page menu Electron builds from `context-menu` params.
const CONTEXT_JS: &str = r##"(() => {
  const e = document.elementFromPoint(__X__, __Y__);
  const link = e && e.closest("a[href]");
  const img = e && e.closest("img");
  const field = e && e.closest("input, textarea, [contenteditable]:not([contenteditable=false])");
  const editable = !!field && !field.disabled && !field.readOnly;
  const active = document.activeElement;
  const fieldSelection = active && typeof active.selectionStart === "number" && active.selectionStart !== active.selectionEnd;
  let image = null;
  if (img) {
    const r = img.getBoundingClientRect();
    if (r.width >= 1 && r.height >= 1) image = { x: r.x + scrollX, y: r.y + scrollY, width: Math.min(r.width, 4096), height: Math.min(r.height, 4096), scale: 1 };
  }
  return { link: link ? link.href : null, image, editable, selection: String(getSelection()).length > 0 || !!fieldSelection };
})()"##;

/// Back and forward availability; the about:blank a tab opens on is not a place to go back to.
fn travel(history: &[(i64, String)], index: usize) -> (bool, bool) {
    let back = history.iter().take(index).any(|(_, url)| url != "about:blank");
    (back, index + 1 < history.len())
}

fn net_error(text: &str) -> String {
    text.trim_start_matches("net::").to_string()
}

pub(super) fn key_code(key: &str) -> u32 {
    match key {
        "Enter" => 13,
        "Backspace" => 8,
        "Tab" => 9,
        "Escape" => 27,
        "PageUp" => 33,
        "PageDown" => 34,
        "End" => 35,
        "Home" => 36,
        "ArrowLeft" => 37,
        "ArrowUp" => 38,
        "ArrowRight" => 39,
        "ArrowDown" => 40,
        "Delete" => 46,
        _ => 0,
    }
}

fn show(pixels: &Pixels, data: Option<&str>) -> bool {
    let Some(bytes) = data.and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok()) else { return false };
    let Ok(img) = image::load_from_memory(&bytes) else { return false };
    let rgba = img.to_rgba8();
    pixels.set(rgba.width(), rgba.height(), rgba.into_raw());
    true
}

/// A cropped shot's picture and its CSS height: the right and bottom margins of the top-left
/// colour go, keeping a 2px border.
fn show_cropped(shot: &Shot, data: Option<&str>) -> Option<f32> {
    let bytes = data.and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok())?;
    let rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let ((w, h), scale) = (crop_bounds(&rgba)?, rgba.width() as f32 / shot.width.max(1) as f32);
    let pad = (2. * scale).round() as u32;
    let (w, h) = ((w + pad).min(rgba.width()), (h + pad).min(rgba.height()));
    let cropped = image::imageops::crop_imm(&rgba, 0, 0, w, h).to_image();
    shot.pixels.set(w, h, cropped.into_raw());
    Some(h as f32 / scale)
}

/// The extent of whatever differs from the top-left pixel, or `None` when nothing does.
fn crop_bounds(rgba: &image::RgbaImage) -> Option<(u32, u32)> {
    let background = *rgba.get_pixel(0, 0);
    let (mut right, mut bottom) = (0, 0);
    for (x, y, pixel) in rgba.enumerate_pixels() {
        if pixel.0.iter().zip(background.0).any(|(a, b)| a.abs_diff(b) > 8) {
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
    }
    (right > 0).then_some((right, bottom))
}

/// A navigation that turned into a download, was stopped, or met a cancelled leave-page prompt
/// leaves the page as it was, unless a newer navigation already took over.
fn abandon(s: &mut Status, aborted: &str, current: Option<String>) {
    if s.url != aborted {
        return;
    }
    s.loading = false;
    if let Some(url) = current {
        s.url = url;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cropped_shot_keeps_what_differs_from_its_corner() {
        let mut page = image::RgbaImage::new(10, 6);
        page.put_pixel(3, 2, image::Rgba([20, 20, 20, 200]));
        page.put_pixel(1, 1, image::Rgba([4, 4, 4, 4]));
        assert_eq!(crop_bounds(&page), Some((4, 3)), "faint antialiasing noise is background");
        assert_eq!(crop_bounds(&image::RgbaImage::new(4, 4)), None, "an empty shot fails");
    }

    #[test]
    fn window_opens_resolve_like_electrons_handler() {
        // `windowFeatures` as Chrome 14x reports them in `Page.windowOpen`.
        let tab = ["menubar", "toolbar", "status", "scrollbars", "resizable"];
        let link = ["menubar", "toolbar", "status", "scrollbars", "resizable", "noopener"];
        let sized = ["width=500", "height=600", "resizable"];
        // A scripted popup keeps a window, so OAuth's opener survives.
        assert_eq!(window_open_action("https://accounts.google.com/o/oauth2/auth", &sized), Opened::Popup);
        assert_eq!(window_open_action("http://localhost:5173/auth", &["resizable"]), Opened::Popup);
        assert_eq!(window_open_action("https://example.com/", &[]), Opened::Popup);
        // A target=_blank link or a bare window.open is a new tab.
        assert_eq!(window_open_action("https://example.com/a", &link), Opened::Tab);
        assert_eq!(window_open_action("HTTPS://example.com/b", &tab), Opened::Tab);
        // Schemes Electron can't harden stay in place; local and inline ones are denied.
        for url in ["vscode://vscode-remote/ssh-remote+box/tmp", "blob:https://example.com/0b6c6a5e", "mailto:a@b.c"] {
            assert_eq!(window_open_action(url, &sized), Opened::Navigate, "{url}");
        }
        for url in ["about:blank", "javascript:alert(1)", "data:text/html,x", "file:///etc/passwd", "view-source:https://a.b", "chrome://settings", "", "not a url", "1http://x"] {
            assert_eq!(window_open_action(url, &tab), Opened::Deny, "{url}");
        }
    }

    #[test]
    fn an_inline_shot_fits_its_page_within_its_caps() {
        // htmlRenderFrameHeight's live fit: at least 80, whole pixels up.
        assert_eq!(shot_size(12.2, 2.), Some((80., 1.)));
        assert_eq!(shot_size(420.4, 2.), Some((421., 1.)));
        // A tall page is shot to its first 4000 CSS pixels, at most 8000 device pixels tall.
        assert_eq!(shot_size(9000., 1.), Some((4000., 1.)));
        assert_eq!(shot_size(9000., 2.), Some((4000., 1.)));
        assert_eq!(shot_size(9000., 3.), Some((4000., 8000. / 12000.)));
        assert_eq!(shot_size(f64::NAN, 2.), None);
    }

    #[test]
    fn a_shot_its_chrome_dropped_reads_as_failed() {
        let shot = Shot::new("http://x/a.html".into(), 728, true);
        let sent = shot.clone();
        assert_eq!(shot.result(), None);
        drop(sent);
        assert_eq!(shot.result(), Some(None));
        let shot = Shot::new("http://x/a.html".into(), 728, true);
        let worker = shot.clone();
        worker.finish(Some(421.));
        assert_eq!(shot.result(), Some(Some(421.)));
    }

    #[test]
    fn the_page_script_reports_copies_reads_and_links_to_other_apps() {
        assert!(matches!(page_message(1, r#"{"kind":"copy","text":"npm i"}"#), Some(Event::Clipboard(t)) if t == "npm i"));
        assert!(page_message(1, r#"{"kind":"copy","text":""}"#).is_none());
        assert!(matches!(page_message(2, r#"{"kind":"read","id":7,"origin":"http://localhost:5173"}"#), Some(Event::ClipboardRead { tab: 2, id: 7, origin }) if origin == "http://localhost:5173"));
        assert!(page_message(2, r#"{"kind":"read","id":7,"origin":"null"}"#).is_none(), "an opaque origin is refused, as Electron does");
        assert!(matches!(page_message(3, r#"{"kind":"external","url":"mailto:dev@example.com"}"#), Some(Event::External { tab: 3, url }) if url == "mailto:dev@example.com"));
        for url in ["https://example.com/", "javascript:alert(1)", "file:///etc/passwd", "not a url"] {
            assert!(page_message(3, &json!({ "kind": "external", "url": url }).to_string()).is_none(), "{url}");
        }
    }

    #[test]
    fn each_profile_gets_its_tabs_commands_and_quits_with_its_last_tab() {
        let mut b = Browser::default();
        let (default_tx, default_rx) = channel();
        let (work_tx, work_rx) = channel();
        let (incognito_tx, incognito_rx) = channel();
        b.workers.insert(DEFAULT_PROFILE.into(), default_tx);
        b.workers.insert("work".into(), work_tx);
        b.workers.insert(INCOGNITO_PROFILE.into(), incognito_tx);
        let drain = |rx: &Receiver<Cmd>| rx.try_iter().collect::<Vec<_>>();
        b.open(1, Pixels::new(), DEFAULT_PROFILE);
        b.open(2, Pixels::new(), "work");
        b.open(3, Pixels::new(), "work");
        b.open(4, Pixels::new(), INCOGNITO_PROFILE);
        b.adopt(5, Pixels::new(), "popup".into(), Some(2));
        assert!(matches!(drain(&default_rx)[..], [Cmd::Open(1, _)]));
        assert!(matches!(drain(&work_rx)[..], [Cmd::Open(2, _), Cmd::Open(3, _), Cmd::Adopt(5, ..)]));
        drain(&incognito_rx);

        b.send(Cmd::Show(Some(2)));
        b.send(Cmd::Navigate(1, "http://a/".into()));
        b.send(Cmd::Text("x".into()));
        assert!(matches!(drain(&default_rx)[..], [Cmd::Show(None), Cmd::Navigate(1, _)]));
        assert!(matches!(drain(&work_rx)[..], [Cmd::Show(Some(2)), Cmd::Text(_)]));
        assert!(matches!(drain(&incognito_rx)[..], [Cmd::Show(None)]));

        // Removing a running profile clears it now and deletes it with its last tab.
        b.clear("work", true);
        for tab in [2, 3, 5] {
            b.send(Cmd::Close(tab));
        }
        assert!(matches!(drain(&work_rx)[..], [Cmd::ClearCookies, Cmd::ClearCache, Cmd::Close(2), Cmd::Close(3), Cmd::Close(5), Cmd::Quit { wipe: true }]));
        assert!(!b.workers.contains_key("work"));
        // Incognito forgets everything once its last tab closes; Default's Chrome stays.
        b.send(Cmd::Close(4));
        assert!(matches!(drain(&incognito_rx)[..], [Cmd::Close(4), Cmd::Quit { wipe: true }]));
        b.send(Cmd::Close(1));
        assert!(matches!(drain(&default_rx)[..], [Cmd::Close(1)]));
        assert_eq!(b.workers.keys().collect::<Vec<_>>(), [DEFAULT_PROFILE]);
    }

    #[test]
    fn zoom_shrinks_the_css_viewport() {
        assert_eq!(metrics(540., 820., 1.), (540, 820));
        assert_eq!(metrics(540., 820., 1.25), (432, 656));
    }

    #[test]
    fn back_skips_the_blank_page_a_tab_opens_on() {
        let h = |urls: &[&str]| urls.iter().enumerate().map(|(i, u)| (i as i64, u.to_string())).collect::<Vec<_>>();
        assert_eq!(travel(&h(&["about:blank", "http://a/"]), 1), (false, false));
        assert_eq!(travel(&h(&["http://a/", "http://a/2"]), 1), (true, false));
        assert_eq!(travel(&h(&["http://a/", "http://a/2"]), 0), (false, true));
    }

    #[test]
    fn a_device_viewport_shrinks_to_fit_but_never_grows() {
        assert_eq!(device_box(540., 800., (375, 667), 1.), (375., 667., 1.));
        assert_eq!(device_box(512., 800., (1024, 1366), 1.), (512., 683., 0.5));
        assert_eq!(device_box(540., 400., (375, 800), 1.), (187.5, 400., 0.5));
        // Zoom enlarges the device until it no longer fits.
        assert_eq!(device_box(1000., 1000., (375, 400), 2.), (750., 800., 2.));
        assert_eq!(device_box(400., 1000., (400, 400), 2.), (400., 400., 1.));
    }

    #[test]
    fn picked_elements_crop_in_document_pixels() {
        let element = json!({ "rect": { "x": 10., "y": -5., "width": 120., "height": 30. }, "doc": { "x": 10., "y": 395. } });
        assert_eq!(clip(&element), Some(json!({ "x": 10., "y": 395., "width": 120., "height": 30., "scale": 1 })));
        assert_eq!(clip(&json!({ "rect": { "width": 0., "height": 10. }, "doc": { "x": 0., "y": 0. } })), None);
    }

    #[test]
    fn an_annotations_screenshot_frames_its_marks() {
        assert_eq!(marks_clip(&json!({ "x": 20., "y": 4., "width": 100., "height": 50. })), Some(json!({ "x": 12., "y": 0., "width": 116., "height": 62., "scale": 1 })));
        assert_eq!(marks_clip(&Value::Null), None);
    }

    #[test]
    fn a_pdf_page_shows_in_the_viewer_at_its_own_address() {
        let pdf = json!([{ "name": "content-type", "value": "Application/PDF" }]);
        assert!(pdf_response(&pdf));
        assert!(!pdf_response(&json!([{ "name": "Content-Type", "value": "text/html" }])));
        assert!(!pdf_response(&Value::Null));
        let page = pdf_viewer("http://127.0.0.1:3773/api/assets/abc/My%20%3Cplan%3E.pdf?x=1");
        assert!(page.contains("<title>My &lt;plan&gt;.pdf</title>"), "{page}");
        assert!(page.contains("getDocument({ url: location.href"));
        // Our headless Chrome always gets pdf.js; a headful one only where it would show the PDF.
        let headful = ["--no-first-run"];
        assert!(!native_pdf_possible(&CHROME_ARGS, &Value::Null));
        assert!(native_pdf_possible(&headful, &Value::Null));
        assert!(!native_pdf_possible(&["--headless=new"], &Value::Null));
        assert!(!native_pdf_possible(&["--no-first-run", "--disable-pdf-viewer"], &Value::Null));
        assert!(!native_pdf_possible(&headful, &json!({ "plugins": { "always_open_pdf_externally": true } })));
        assert!(native_pdf_possible(&headful, &json!({ "plugins": { "always_open_pdf_externally": false } })));
        // Offline: pdf.js and its worker come with the app, not from a CDN.
        assert!(!page.contains("https://"), "the page loads nothing from the network");
        assert!(page.contains("pdfjsLib") && page.contains(PDFJS_WORKER) && page.matches("{pdfjs}").count() == 0);
    }

    #[test]
    fn downloads_take_the_first_free_name_like_electron() {
        let dir = std::env::temp_dir().join(format!("s5-downloads-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique_path(&dir, "report.pdf"), Some(dir.join("report.pdf")));
        std::fs::write(dir.join("report.pdf"), b"a").unwrap();
        std::fs::write(dir.join("report (1).pdf"), b"b").unwrap();
        assert_eq!(unique_path(&dir, "report.pdf"), Some(dir.join("report (2).pdf")));
        // A page's name never leaves the folder.
        assert_eq!(unique_path(&dir, "../../etc/passwd"), Some(dir.join("passwd")));
        let staged = dir.join("guid-1");
        std::fs::write(&staged, b"c").unwrap();
        assert_eq!(save_download(&staged, &dir, "notes"), Ok(dir.join("notes")));
        assert!(!staged.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recorded_keys_read_like_web() {
        assert_eq!(key_label("Enter"), "↵");
        assert_eq!(key_label("a"), "A");
        assert_eq!(key_label(" "), "Space");
        assert_eq!(key_label("PageDown"), "PageDown");
    }

    #[test]
    fn recordings_are_named_by_their_format() {
        assert_eq!(recording_extension("video/mp4;codecs=avc1"), "mp4");
        assert_eq!(recording_extension("video/webm;codecs=vp9"), "webm");
    }

    #[test]
    fn chrome_net_errors_lose_their_prefix() {
        assert_eq!(net_error("net::ERR_CONNECTION_REFUSED"), "ERR_CONNECTION_REFUSED");
    }

    #[test]
    fn an_abandoned_navigation_restores_the_page_url() {
        let mut s = Status { url: "http://a/two".into(), loading: true, ..Default::default() };
        abandon(&mut s, "http://a/two", Some("http://a/leave".into()));
        assert_eq!((s.url.as_str(), s.loading), ("http://a/leave", false));
        let mut newer = Status { url: "http://b/".into(), loading: true, ..Default::default() };
        abandon(&mut newer, "http://a/two", Some("http://a/leave".into()));
        assert_eq!((newer.url.as_str(), newer.loading), ("http://b/", true));
    }
}
