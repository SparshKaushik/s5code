//! The browser preview: a headless Chrome driven over the DevTools protocol. Chrome renders each
//! tab; the shown tab's screencast frames land in its `Pixels`, and the panel's pointer and keys
//! go back as CDP input events. Rune never embeds a web view, so this is the whole bridge.
//!
//! One browser-level connection carries every tab as a flattened target session. Chrome rejects
//! request ids outside int32, so replies are matched through a map, not tagged ids.

use std::collections::HashMap;
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
    /// `prefers-color-scheme`: "" follows Chrome's default.
    ColorScheme(&'static str),
    Mouse { kind: &'static str, x: f32, y: f32, clicks: u32 },
    Wheel { x: f32, y: f32, dx: f32, dy: f32 },
    Text(String),
    Key(&'static str),
    Screenshot(TabId, PathBuf),
    ClearCookies,
    ClearCache,
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
}

impl Default for Status {
    fn default() -> Self {
        Status { url: String::new(), title: String::new(), loading: false, can_back: false, can_forward: false, painted: false, failure: None, zoom: 1., device: None, picking: false, hover: None }
    }
}

pub enum Event {
    Screenshot(Result<PathBuf, String>),
    /// A page opened a new window; it becomes a tab beside its opener.
    Popup { target: String },
    /// A picked element's details (`PickedElementPayload` fields plus its rect) and its PNG crop.
    Picked { tab: TabId, element: Value, image: Option<Vec<u8>> },
    DevTools(Result<String, String>),
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

pub struct Browser {
    tx: Option<Sender<Cmd>>,
    pub shared: Arc<Mutex<Shared>>,
}

impl Default for Browser {
    fn default() -> Self {
        Browser { tx: None, shared: Arc::default() }
    }
}

impl Browser {
    pub fn started(&self) -> bool {
        self.tx.is_some()
    }

    /// Launches Chrome on a worker; commands sent before it is up queue in the channel.
    pub fn start(&mut self, waker: Waker) {
        let (tx, rx) = channel();
        self.tx = Some(tx);
        let shared = self.shared.clone();
        std::thread::spawn(move || {
            if let Err(e) = run(rx, shared.clone(), waker.clone()) {
                let mut s = shared.lock().unwrap();
                s.fatal = Some(e);
                s.generation += 1;
                drop(s);
                waker();
            }
        });
    }

    pub fn send(&self, cmd: Cmd) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(cmd);
        }
    }

    pub fn status(&self, tab: TabId) -> Status {
        self.shared.lock().unwrap().tabs.get(&tab).cloned().unwrap_or_default()
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

static CHROME: Mutex<Option<Child>> = Mutex::new(None);

/// Stops the headless Chrome this process started; called once the window loop returns.
pub fn shutdown() {
    if let Some(mut child) = CHROME.lock().unwrap().take() {
        let _ = child.kill();
    }
}

/// Starts Chrome on the persistent Default profile, as the Electron app keeps its default
/// session; a profile another instance holds falls back to a throwaway one.
fn launch(scale: f32) -> Result<(Child, String), String> {
    let persistent = crate::host::t3_home().join("userdata").join("browser").join("rune-default");
    launch_in(&persistent, scale).or_else(|_| launch_in(&std::env::temp_dir().join(format!("s5-rune-chrome-{}", std::process::id())), scale))
}

fn launch_in(profile: &std::path::Path, scale: f32) -> Result<(Child, String), String> {
    let _ = std::fs::create_dir_all(profile);
    let mut child = Command::new(chrome_path())
        .args(["--headless", "--no-sandbox", "--disable-gpu", "--hide-scrollbars", "--no-first-run", "--no-default-browser-check", "--remote-debugging-port=0"])
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
    scheme: &'static str,
    pressed: bool,
    /// A hover lookup is in flight; the latest pointer position waits for it.
    hovering: bool,
    hover_next: Option<(TabId, f32, f32)>,
    shared: Arc<Mutex<Shared>>,
    waker: Waker,
}

fn run(rx: Receiver<Cmd>, shared: Arc<Mutex<Shared>>, waker: Waker) -> Result<(), String> {
    // The headless shell paints frames at the device scale it starts with, whatever scale a page
    // emulates, so it waits for the window's.
    let mut early = Vec::new();
    let scale = loop {
        let cmd = rx.recv().map_err(|_| "The window closed".to_string())?;
        let scale = if let Cmd::Viewport { scale, .. } = &cmd { Some(*scale) } else { None };
        early.push(cmd);
        if let Some(scale) = scale {
            break scale;
        }
    };
    let (child, url) = launch(scale)?;
    *CHROME.lock().unwrap() = Some(child);
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
        scheme: "",
        pressed: false,
        hovering: false,
        hover_next: None,
        shared,
        waker,
    };
    w.send(None, "Target.setDiscoverTargets", json!({ "discover": true }), None);
    for cmd in early {
        w.command(cmd);
    }
    loop {
        while let Ok(cmd) = rx.try_recv() {
            w.command(cmd);
        }
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
            Cmd::ColorScheme(scheme) => {
                self.scheme = scheme;
                let tabs: Vec<TabId> = self.tabs.keys().copied().collect();
                for tab in tabs {
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
                let button = if kind != "mouseMoved" || self.pressed { "left" } else { "none" };
                let buttons = if self.pressed { 1 } else { 0 };
                self.call(tab, "Input.dispatchMouseEvent", json!({ "type": kind, "x": x / zoom, "y": y / zoom, "button": button, "buttons": buttons, "clickCount": if kind == "mouseMoved" { 0 } else { clicks.max(1) } }), None);
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
                self.call(tab, "Input.insertText", json!({ "text": text }), None);
            }
            Cmd::Key(key) => {
                let Some(tab) = self.shown else { return };
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
        }
    }

    /// Window points per CSS pixel on the shown page.
    fn input_scale(&self, tab: TabId) -> f32 {
        let (w, h, _) = self.viewport;
        self.tabs.get(&tab).map_or(1., |t| match t.device {
            Some(device) => device_box(w, h, device, t.zoom).2,
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
            let opener = info["openerId"].as_str().unwrap_or_default();
            let ours = |id: &str| self.tabs.values().any(|t| t.target.as_deref() == Some(id));
            if info["type"] == "page" && !opener.is_empty() && ours(opener) {
                if let Some(target) = info["targetId"].as_str().filter(|id| !ours(id)) {
                    self.event(Event::Popup { target: target.to_string() });
                }
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
            }
            "Page.frameStartedLoading" if main(&p["frameId"]) => self.update(tab, |s| s.loading = true),
            "Page.frameStoppedLoading" | "Page.loadEventFired" if method == "Page.loadEventFired" || main(&p["frameId"]) => {
                self.update(tab, |s| s.loading = false);
                self.history(tab);
            }
            "Page.frameNavigated" if p["frame"]["parentId"].is_null() => {
                let frame = &p["frame"];
                let unreachable = frame["unreachableUrl"].as_str().filter(|u| !u.is_empty()).map(str::to_owned);
                let url = unreachable.clone().unwrap_or_else(|| frame["url"].as_str().unwrap_or_default().to_string());
                let error = self.tabs.get_mut(&tab).and_then(|t| if unreachable.is_some() { t.error.clone() } else { t.error.take() });
                self.update(tab, |s| {
                    s.url = url.clone();
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
                self.sessions.insert(session.clone(), tab);
                self.send(Some(&session), "Page.enable", json!({}), None);
                self.send(Some(&session), "Network.enable", json!({}), None);
                self.metrics(tab);
                if !self.scheme.is_empty() {
                    self.call(tab, "Emulation.setEmulatedMedia", json!({ "features": [{ "name": "prefers-color-scheme", "value": self.scheme }] }), None);
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
                let description = net_error(text);
                self.update(tab, |s| {
                    s.loading = false;
                    s.failure = Some(Failure { url, description });
                });
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

/// Back and forward availability; the about:blank a tab opens on is not a place to go back to.
fn travel(history: &[(i64, String)], index: usize) -> (bool, bool) {
    let back = history.iter().take(index).any(|(_, url)| url != "about:blank");
    (back, index + 1 < history.len())
}

fn net_error(text: &str) -> String {
    text.trim_start_matches("net::").to_string()
}

fn key_code(key: &str) -> u32 {
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn chrome_net_errors_lose_their_prefix() {
        assert_eq!(net_error("net::ERR_CONNECTION_REFUSED"), "ERR_CONNECTION_REFUSED");
    }
}
