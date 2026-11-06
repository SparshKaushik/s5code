//! The browser preview: a headless Chrome driven over the DevTools protocol. Chrome renders the
//! page; its screencast frames land in a `Pixels` the view draws, and the panel's pointer and
//! keys go back as CDP input events. Rune never embeds a web view, so this is the whole bridge.

use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use rune::{Pixels, Waker};
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

pub enum Cmd {
    Navigate(String),
    Back,
    Forward,
    Reload,
    Resize { w: u32, h: u32, scale: f32 },
    Mouse { kind: &'static str, x: f32, y: f32, clicks: u32 },
    Wheel { x: f32, y: f32, dx: f32, dy: f32 },
    Text(String),
    Key(&'static str),
}

#[derive(Clone, Default, PartialEq)]
pub struct Status {
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub can_back: bool,
    pub can_forward: bool,
    pub frames: u64,
    pub error: Option<String>,
}

pub struct Browser {
    tx: Option<Sender<Cmd>>,
    pub pixels: Pixels,
    pub status: Arc<Mutex<Status>>,
}

impl Default for Browser {
    fn default() -> Self {
        Browser { tx: None, pixels: Pixels::new(), status: Arc::default() }
    }
}

impl Browser {
    /// Launches Chrome on a worker; commands sent before it is up queue in the channel.
    pub fn start(&mut self, waker: Waker) {
        let (tx, rx) = channel();
        self.tx = Some(tx);
        let pixels = self.pixels.clone();
        let status = self.status.clone();
        std::thread::spawn(move || {
            if let Err(e) = run(rx, pixels, status.clone(), waker.clone()) {
                status.lock().unwrap().error = Some(e);
                waker();
            }
        });
    }

    pub fn send(&self, cmd: Cmd) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(cmd);
        }
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

fn launch() -> Result<(Child, String), String> {
    let profile = std::env::temp_dir().join(format!("s5-rune-chrome-{}", std::process::id()));
    let mut child = Command::new(chrome_path())
        .args(["--headless", "--no-sandbox", "--disable-gpu", "--hide-scrollbars", "--no-first-run", "--no-default-browser-check", "--remote-debugging-port=0", "--window-size=800,600"])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("about:blank")
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|e| format!("Chrome failed to start ({e}). Install Google Chrome or set S5_CHROME."))?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let mut lines = BufReader::new(stderr).lines();
    let ws = loop {
        let line = lines.next().ok_or("Chrome exited before DevTools came up")?.map_err(|e| e.to_string())?;
        if let Some(rest) = line.split("DevTools listening on ").nth(1) {
            break rest.trim().to_string();
        }
    };
    // Keep draining stderr so Chrome never blocks on a full pipe.
    std::thread::spawn(move || for _ in lines {});
    let port = ws.split(':').nth(2).and_then(|p| p.split('/').next()).ok_or("bad DevTools URL")?.to_string();
    for _ in 0..50 {
        if let Ok(list) = ureq::get(&format!("http://127.0.0.1:{port}/json/list")).call().and_then(|r| Ok(r.into_json::<Value>()?))
            && let Some(page) = list.as_array().into_iter().flatten().find(|t| t["type"] == "page")
        {
            return Ok((child, page["webSocketDebuggerUrl"].as_str().unwrap_or_default().to_string()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("Chrome has no page target".into())
}

struct Cdp {
    ws: WebSocket<TcpStream>,
    next: u64,
    history: Vec<(i64, String)>,
    index: usize,
    size: (u32, u32),
}

impl Cdp {
    fn call(&mut self, method: &str, params: Value) -> u64 {
        self.next += 1;
        let _ = self.ws.send(Message::text(json!({ "id": self.next, "method": method, "params": params }).to_string()));
        self.next
    }
}

const HISTORY: u64 = 1 << 40;
const SHOT: u64 = 1 << 41;

fn run(rx: Receiver<Cmd>, pixels: Pixels, status: Arc<Mutex<Status>>, waker: Waker) -> Result<(), String> {
    let (child, url) = launch()?;
    *CHROME.lock().unwrap() = Some(child);
    let host = url.trim_start_matches("ws://").split('/').next().unwrap_or_default().to_string();
    let stream = TcpStream::connect(&host).map_err(|e| e.to_string())?;
    let (ws, _) = tungstenite::client(url.as_str(), stream).map_err(|e| e.to_string())?;
    ws.get_ref().set_read_timeout(Some(Duration::from_millis(15))).map_err(|e| e.to_string())?;
    let mut cdp = Cdp { ws, next: 0, history: Vec::new(), index: 0, size: (800, 600) };
    cdp.call("Page.enable", json!({}));
    let set = |f: &dyn Fn(&mut Status)| {
        let mut s = status.lock().unwrap();
        let before = s.clone();
        f(&mut s);
        if *s != before {
            drop(s);
            waker();
        }
    };
    loop {
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Navigate(u) => {
                    set(&|s| s.loading = true);
                    cdp.call("Page.navigate", json!({ "url": u }));
                }
                Cmd::Reload => {
                    set(&|s| s.loading = true);
                    cdp.call("Page.reload", json!({}));
                }
                Cmd::Back | Cmd::Forward => {
                    let to = if matches!(cmd, Cmd::Back) { cdp.index.checked_sub(1) } else { Some(cdp.index + 1) };
                    if let Some(&(id, _)) = to.and_then(|i| cdp.history.get(i)) {
                        set(&|s| s.loading = true);
                        cdp.call("Page.navigateToHistoryEntry", json!({ "entryId": id }));
                    }
                }
                Cmd::Resize { w, h, scale } => {
                    cdp.size = (w, h);
                    cdp.call("Emulation.setDeviceMetricsOverride", json!({ "width": w, "height": h, "deviceScaleFactor": scale, "mobile": false }));
                    cdp.call("Page.stopScreencast", json!({}));
                    let (mw, mh) = ((w as f32 * scale) as u32, (h as f32 * scale) as u32);
                    cdp.call("Page.startScreencast", json!({ "format": "jpeg", "quality": 85, "maxWidth": mw, "maxHeight": mh }));
                    cdp.call("Page.captureScreenshot", json!({ "format": "jpeg", "quality": 85 }));
                }
                Cmd::Mouse { kind, x, y, clicks } => {
                    cdp.call("Input.dispatchMouseEvent", json!({ "type": kind, "x": x, "y": y, "button": if kind == "mouseMoved" { "none" } else { "left" }, "clickCount": clicks }));
                }
                Cmd::Wheel { x, y, dx, dy } => {
                    cdp.call("Input.dispatchMouseEvent", json!({ "type": "mouseWheel", "x": x, "y": y, "deltaX": -dx, "deltaY": -dy }));
                }
                Cmd::Text(t) => {
                    cdp.call("Input.insertText", json!({ "text": t }));
                }
                Cmd::Key(k) => {
                    let code = match k { "Enter" => 13, "Backspace" => 8, "Tab" => 9, _ => 0 };
                    for kind in ["rawKeyDown", "keyUp"] {
                        cdp.call("Input.dispatchKeyEvent", json!({ "type": kind, "key": k, "code": k, "windowsVirtualKeyCode": code, "text": if k == "Enter" && kind == "rawKeyDown" { "\r" } else { "" } }));
                    }
                }
            }
        }
        let msg = match cdp.ws.read() {
            Ok(Message::Text(t)) => t,
            Ok(_) => continue,
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            Err(e) => return Err(format!("DevTools connection closed: {e}")),
        };
        let Ok(v) = serde_json::from_str::<Value>(&msg) else { continue };
        match v["method"].as_str() {
            Some("Page.screencastFrame") => {
                let session = v["params"]["sessionId"].clone();
                cdp.call("Page.screencastFrameAck", json!({ "sessionId": session }));
                show(&pixels, v["params"]["data"].as_str());
                set(&|s| s.frames += 1);
            }
            Some("Page.frameStartedLoading") => set(&|s| s.loading = true),
            Some("Page.frameNavigated") if v["params"]["frame"]["parentId"].is_null() => {
                let url = v["params"]["frame"]["url"].as_str().unwrap_or_default().to_string();
                set(&|s| s.url = url.clone());
                cdp.next += 1;
                let id = HISTORY + cdp.next;
                let _ = cdp.ws.send(Message::text(json!({ "id": id, "method": "Page.getNavigationHistory", "params": {} }).to_string()));
            }
            Some("Page.loadEventFired") | Some("Page.frameStoppedLoading") => {
                set(&|s| s.loading = false);
                cdp.next += 1;
                let id = HISTORY + cdp.next;
                let _ = cdp.ws.send(Message::text(json!({ "id": id, "method": "Page.getNavigationHistory", "params": {} }).to_string()));
                let id = SHOT + cdp.next;
                let _ = cdp.ws.send(Message::text(json!({ "id": id, "method": "Page.captureScreenshot", "params": { "format": "jpeg", "quality": 85 } }).to_string()));
            }
            _ => {}
        }
        let id = v["id"].as_u64().unwrap_or(0);
        if id >= SHOT {
            show(&pixels, v["result"]["data"].as_str());
            set(&|s| s.frames += 1);
        } else if id >= HISTORY {
            let r = &v["result"];
            cdp.history = r["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|e| (e["id"].as_i64().unwrap_or(0), e["title"].as_str().unwrap_or_default().to_string()))
                .collect();
            cdp.index = r["currentIndex"].as_u64().unwrap_or(0) as usize;
            let (index, len) = (cdp.index, cdp.history.len());
            let title = cdp.history.get(index).map(|h| h.1.clone()).unwrap_or_default();
            set(&|s| {
                s.title = title.clone();
                s.can_back = index > 0;
                s.can_forward = index + 1 < len;
            });
        } else if let Some(data) = v["result"]["data"].as_str() {
            show(&pixels, Some(data));
        }
    }
}

fn show(pixels: &Pixels, data: Option<&str>) {
    let Some(bytes) = data.and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok()) else { return };
    let Ok(img) = image::load_from_memory(&bytes) else { return };
    let rgba = img.to_rgba8();
    pixels.set(rgba.width(), rgba.height(), rgba.into_raw());
}
