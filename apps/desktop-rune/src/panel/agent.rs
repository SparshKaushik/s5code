//! Agents drive the panel's tabs as they drive Electron's: the server reads `attached` and CDP
//! events from one fd and writes CDP commands to another (`DesktopBrowserChannel`), and drives
//! each attached tab with Playwright. Playwright wants a browser; each tab gets a relay that
//! answers as a browser holding that page (and the popups it opens) and forwards the rest to the
//! pages' own DevTools sockets, as Electron's `CdpRelay` does over a webview's debugger.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

/// A preview session: `(threadId, tabId)`.
pub type Key = (String, String);

struct Channel {
    writer: Arc<Mutex<UnixStream>>,
    /// Each attached tab's DevTools `host:port` and page target.
    targets: HashMap<Key, (String, String)>,
    relays: HashMap<Key, Sender<In>>,
}

/// What a relay thread is told.
enum In {
    Command(String),
    /// The page opened a popup the panel shows as a tab.
    Popup { target: String, url: String },
    PopupGone(String),
    /// A browser-level event for Playwright's root session (downloads).
    Event(String, Value),
}

/// What an agent's downloads need from its page: Playwright's folder and whether it wants the
/// events, and when the agent last acted on the page.
#[derive(Clone, Default)]
struct Downloads {
    dir: Option<String>,
    events: bool,
    input_at: Option<Instant>,
}

/// The server's last pointer for a session, which the panel draws as web's agent cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct Pointer {
    pub click: bool,
    pub x: f32,
    pub y: f32,
    pub sequence: u64,
    pub at: Instant,
}

static CHANNEL: Mutex<Option<Channel>> = Mutex::new(None);
/// Bumped per server process, so the panel announces its tabs to each new one.
static GENERATION: Mutex<u64> = Mutex::new(0);
static DOWNLOADS: Mutex<Option<HashMap<String, Downloads>>> = Mutex::new(None);
static POINTERS: Mutex<Option<HashMap<Key, Pointer>>> = Mutex::new(None);
static WAKER: Mutex<Option<rune::Waker>> = Mutex::new(None);

/// Electron's `AGENT_DOWNLOAD_WINDOW_MS`: a download this soon after the agent acted is its.
const AGENT_DOWNLOAD_WINDOW: Duration = Duration::from_secs(5);

/// The server just started with these as its `desktopBrowserFd` peer (we write events) and its
/// `desktopBrowserControlFd` peer (we read commands).
pub fn connect(events: UnixStream, commands: UnixStream) {
    let writer = Arc::new(Mutex::new(events));
    *CHANNEL.lock().unwrap() = Some(Channel { writer: writer.clone(), targets: HashMap::new(), relays: HashMap::new() });
    *GENERATION.lock().unwrap() += 1;
    std::thread::spawn(move || read_commands(commands, writer));
}

/// Whether a server Rune started listens; a server it only connects to drives its own browser.
pub fn available() -> bool {
    CHANNEL.lock().unwrap().is_some()
}

pub fn generation() -> u64 {
    *GENERATION.lock().unwrap()
}

/// Repaints the panel when the agent's cursor moves.
pub fn set_waker(waker: rune::Waker) {
    let mut slot = WAKER.lock().unwrap();
    if slot.is_none() {
        *slot = Some(waker);
    }
}

/// The tab's page exists, so the server can drive it.
pub fn attach(key: &Key, host: String, target: String) {
    let mut guard = CHANNEL.lock().unwrap();
    let Some(ch) = guard.as_mut() else { return };
    ch.targets.insert(key.clone(), (host, target));
    write(&ch.writer, &json!({ "type": "attached", "threadId": key.0, "tabId": key.1 }));
}

/// The tab closed or its page went away.
pub fn detach(key: &Key) {
    let mut guard = CHANNEL.lock().unwrap();
    let Some(ch) = guard.as_mut() else { return };
    ch.relays.remove(key);
    if let Some((_, target)) = ch.targets.remove(key) {
        write(&ch.writer, &json!({ "type": "detached", "threadId": key.0, "tabId": key.1 }));
        if let Some(d) = DOWNLOADS.lock().unwrap().as_mut() {
            d.remove(&target);
        }
    }
    if let Some(p) = POINTERS.lock().unwrap().as_mut() {
        p.remove(key);
    }
}

/// Whether Playwright is connected to the session's tab right now.
pub fn driven(key: &Key) -> bool {
    CHANNEL.lock().unwrap().as_ref().is_some_and(|ch| ch.relays.contains_key(key))
}

/// Shows a popup the session's page opened to the agent driving it, as Playwright's `popup`.
pub fn popup(key: &Key, target: String, url: String) {
    send(key, In::Popup { target, url });
}

pub fn popup_gone(key: &Key, target: String) {
    send(key, In::PopupGone(target));
}

fn send(key: &Key, message: In) {
    if let Some(tx) = CHANNEL.lock().unwrap().as_ref().and_then(|ch| ch.relays.get(key)) {
        let _ = tx.send(message);
    }
}

/// Playwright's download folder when a download that page just started is the agent's.
pub fn agent_download(target: &str) -> Option<String> {
    let guard = DOWNLOADS.lock().unwrap();
    let d = guard.as_ref()?.get(target)?;
    d.input_at.filter(|at| at.elapsed() < AGENT_DOWNLOAD_WINDOW)?;
    d.dir.clone()
}

/// Hands a download event to the agent driving `target`'s page, as Electron's relay does.
pub fn download_event(target: &str, method: &str, params: Value) {
    let wanted = DOWNLOADS.lock().unwrap().as_ref().and_then(|d| d.get(target)).is_some_and(|d| d.events);
    if !wanted {
        return;
    }
    let guard = CHANNEL.lock().unwrap();
    let Some(ch) = guard.as_ref() else { return };
    let Some(key) = ch.targets.iter().find(|(_, (_, t))| t == target).map(|(k, _)| k) else { return };
    if let Some(tx) = ch.relays.get(key) {
        let _ = tx.send(In::Event(method.to_owned(), params));
    }
}

pub fn pointer(key: &Key) -> Option<Pointer> {
    POINTERS.lock().unwrap().as_ref()?.get(key).cloned()
}

fn write(writer: &Mutex<UnixStream>, event: &Value) {
    let _ = writer.lock().unwrap().write_all(format!("{event}\n").as_bytes());
}

fn read_commands(commands: UnixStream, mine: Arc<Mutex<UnixStream>>) {
    let mut sequence = 0;
    for line in BufReader::new(commands).lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let key = (v["threadId"].as_str().unwrap_or_default().to_owned(), v["tabId"].as_str().unwrap_or_default().to_owned());
        let mut guard = CHANNEL.lock().unwrap();
        let Some(ch) = guard.as_mut().filter(|ch| Arc::ptr_eq(&ch.writer, &mine)) else { break };
        match v["type"].as_str() {
            Some("cdp") => {
                let Some(message) = v["message"].as_str() else { continue };
                if !ch.relays.contains_key(&key) {
                    let Some((host, target)) = ch.targets.get(&key).cloned() else { continue };
                    let (tx, rx) = channel();
                    let writer = ch.writer.clone();
                    let relay_key = key.clone();
                    std::thread::spawn(move || run_relay(relay_key, host, target, rx, writer));
                    ch.relays.insert(key.clone(), tx);
                }
                if let Some(tx) = ch.relays.get(&key) {
                    let _ = tx.send(In::Command(message.to_owned()));
                }
            }
            // The server let go of the tab; its relay and page socket close.
            Some("release") => {
                ch.relays.remove(&key);
            }
            Some("pointer") => {
                drop(guard);
                sequence += 1;
                let Some(p) = pointer_from(&v, sequence) else { continue };
                POINTERS.lock().unwrap().get_or_insert_default().insert(key, p);
                if let Some(waker) = WAKER.lock().unwrap().clone() {
                    waker();
                }
            }
            // `turn` keeps Electron's webviews focused between steps; a headless page needs none.
            _ => {}
        }
    }
    // The server exited; the next one gets a new channel.
    let mut guard = CHANNEL.lock().unwrap();
    if guard.as_ref().is_some_and(|ch| Arc::ptr_eq(&ch.writer, &mine)) {
        *guard = None;
    }
}

/// The server's `{ type: "pointer", phase: "move" | "click", x, y }`, in page CSS pixels.
fn pointer_from(v: &Value, sequence: u64) -> Option<Pointer> {
    let (x, y) = (v["x"].as_f64()?, v["y"].as_f64()?);
    (x.is_finite() && y.is_finite()).then(|| Pointer { click: v["phase"] == "click", x: x as f32, y: y as f32, sequence, at: Instant::now() })
}

fn page_socket(host: &str, target: &str, timeout: u64) -> Option<WebSocket<TcpStream>> {
    let stream = TcpStream::connect(host).ok()?;
    let (ws, _) = tungstenite::client(format!("ws://{host}/devtools/page/{target}").as_str(), stream).ok()?;
    ws.get_ref().set_read_timeout(Some(Duration::from_millis(timeout))).ok()?;
    Some(ws)
}

enum Read {
    Text(String),
    Nothing,
    Closed,
}

fn read(ws: &mut WebSocket<TcpStream>) -> Read {
    match ws.read() {
        Ok(Message::Text(t)) => Read::Text(t.to_string()),
        Ok(_) => Read::Nothing,
        Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => Read::Nothing,
        Err(_) => Read::Closed,
    }
}

fn run_relay(key: Key, host: String, target: String, rx: Receiver<In>, writer: Arc<Mutex<UnixStream>>) {
    let send_server = |message: String| write(&writer, &json!({ "type": "cdp", "threadId": key.0, "tabId": key.1, "message": message }));
    let Some(mut ws) = page_socket(&host, &target, 5) else { return };
    let mut popups: HashMap<String, WebSocket<TcpStream>> = HashMap::new();
    let mut relay = Relay::new(target.clone());
    let mut outs = Vec::new();
    loop {
        loop {
            match rx.try_recv() {
                Ok(In::Command(raw)) => outs.extend(relay.receive(&raw)),
                Ok(In::Popup { target: popup, url }) => {
                    if let Some(socket) = page_socket(&host, &popup, 1) {
                        popups.insert(popup.clone(), socket);
                        outs.extend(relay.popup(popup, url));
                    }
                }
                Ok(In::PopupGone(popup)) => {
                    if popups.remove(&popup).is_some() {
                        outs.extend(relay.popup_gone(&popup));
                    }
                }
                Ok(In::Event(method, params)) => outs.extend(relay.browser_event(&method, params)),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = ws.close(None);
                    return;
                }
            }
        }
        if relay.downloads_changed() {
            let entry = Downloads { dir: relay.download_dir.clone(), events: relay.download_events, input_at: relay.input_at };
            DOWNLOADS.lock().unwrap().get_or_insert_default().insert(target.clone(), entry);
        }
        for out in outs.drain(..) {
            match out {
                Out::Server(m) => send_server(m),
                Out::Page(None, m) => {
                    if ws.send(Message::text(m)).is_err() {
                        return;
                    }
                }
                Out::Page(Some(popup), m) => {
                    if let Some(socket) = popups.get_mut(&popup) {
                        let _ = socket.send(Message::text(m));
                    }
                }
            }
        }
        match read(&mut ws) {
            Read::Text(t) => outs.extend(relay.page(None, &t)),
            Read::Nothing => {}
            Read::Closed => return,
        }
        let mut gone = vec![];
        for (popup, socket) in popups.iter_mut() {
            match read(socket) {
                Read::Text(t) => outs.extend(relay.page(Some(popup.as_str()), &t)),
                Read::Nothing => {}
                Read::Closed => gone.push(popup.clone()),
            }
        }
        for popup in gone {
            popups.remove(&popup);
            outs.extend(relay.popup_gone(&popup));
        }
        for out in outs.drain(..) {
            if let Out::Server(m) = out {
                send_server(m);
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Out {
    Server(String),
    /// To the page's socket, or a popup's by its target.
    Page(Option<String>, String),
}

#[derive(Clone, PartialEq)]
enum Kind {
    Browser,
    Page,
    /// A popup the page opened, by its target.
    Popup(String),
}

const BROWSER_CONTEXT_ID: &str = "t3-preview";
const PAGE_SESSION_ID: &str = "t3-preview-page";
/// Browser commands a single headless page has nothing to do for.
const ACKNOWLEDGED: [&str; 5] = ["Target.setDiscoverTargets", "Browser.grantPermissions", "Browser.resetPermissions", "Browser.cancelDownload", "Target.setRemoteLocations"];
/// Electron's `AGENT_INPUT_METHOD`: what counts as the agent acting on the page.
fn agent_input(method: &str) -> bool {
    method.starts_with("Input.") || matches!(method, "Page.navigate" | "Runtime.evaluate" | "Runtime.callFunctionOn")
}

/// One Playwright connection's view of a page: a browser with this page and its popups in it.
pub struct Relay {
    target: String,
    attached: bool,
    sessions: HashMap<String, Kind>,
    /// Popups by target, with their URL at opening.
    popups: Vec<(String, String)>,
    /// A popup's child frames and workers, by session, so their commands reach its socket.
    children: HashMap<String, String>,
    sequence: u32,
    next: u64,
    /// Our page-socket ids: the server's id and the session its reply goes back on.
    pending: HashMap<u64, (Value, Option<String>)>,
    download_dir: Option<String>,
    download_events: bool,
    input_at: Option<Instant>,
    downloads_dirty: bool,
}

impl Relay {
    pub fn new(target: String) -> Relay {
        Relay {
            target,
            attached: false,
            sessions: HashMap::from([(PAGE_SESSION_ID.to_owned(), Kind::Page)]),
            popups: vec![],
            children: HashMap::new(),
            sequence: 0,
            next: 0,
            pending: HashMap::new(),
            download_dir: None,
            download_events: false,
            input_at: None,
            downloads_dirty: false,
        }
    }

    fn downloads_changed(&mut self) -> bool {
        std::mem::take(&mut self.downloads_dirty)
    }

    fn new_session(&mut self, kind: Kind) -> String {
        self.sequence += 1;
        let label = match kind {
            Kind::Browser => "browser",
            Kind::Page => "page",
            Kind::Popup(_) => "popup",
        };
        let id = format!("t3-preview-{label}-{}", self.sequence);
        self.sessions.insert(id.clone(), kind);
        id
    }

    fn target_info(&self) -> Value {
        json!({ "targetId": self.target, "type": "page", "title": "", "url": "", "attached": true, "canAccessOpener": false, "browserContextId": BROWSER_CONTEXT_ID })
    }

    fn popup_info(&self, target: &str, url: &str) -> Value {
        json!({ "targetId": target, "type": "page", "title": "", "url": url, "attached": true, "canAccessOpener": true, "openerId": self.target, "browserContextId": BROWSER_CONTEXT_ID })
    }

    /// A command from the server.
    pub fn receive(&mut self, raw: &str) -> Vec<Out> {
        let Ok(command) = serde_json::from_str::<Value>(raw) else { return vec![] };
        let (Some(id), Some(method)) = (command.get("id").filter(|v| v.is_number()).cloned(), command["method"].as_str()) else { return vec![] };
        let session = command["sessionId"].as_str().map(str::to_owned);
        let params = command.get("params").cloned().unwrap_or_else(|| json!({}));
        let (reply_id, route) = (id.clone(), session.clone());
        let reply = move |result: Result<Value, String>| {
            let mut m = match result {
                Ok(r) => json!({ "id": reply_id, "result": r }),
                Err(e) => json!({ "id": reply_id, "error": { "code": -32000, "message": e } }),
            };
            if let Some(s) = &route {
                m["sessionId"] = json!(s);
            }
            Out::Server(m.to_string())
        };
        let kind = session.as_ref().and_then(|s| self.sessions.get(s)).cloned();
        if session.is_none() || kind == Some(Kind::Browser) {
            if method == "Target.setAutoAttach" && !self.attached {
                self.attached = true;
                let mut out = vec![reply(Ok(json!({})))];
                let announce = json!({ "method": "Target.attachedToTarget", "params": { "sessionId": PAGE_SESSION_ID, "targetInfo": self.target_info(), "waitingForDebugger": false } });
                out.push(Out::Server(announce.to_string()));
                // Popups opened before Playwright looked.
                for (target, url) in self.popups.clone() {
                    out.extend(self.announce_popup(&target, &url));
                }
                return out;
            }
            let result = self.browser_command(method, &params);
            return vec![reply(result)];
        }
        if agent_input(method) {
            self.input_at = Some(Instant::now());
            self.downloads_dirty = true;
        }
        let session = session.unwrap_or_default();
        // Which socket, and the session it keeps there: the page and popups are their sockets'
        // own pages; any other id is a child frame or worker of one of them.
        let (socket, child) = match &kind {
            Some(Kind::Page) => (None, None),
            Some(Kind::Popup(target)) => (Some(target.clone()), None),
            _ => (self.children.get(&session).cloned(), Some(session.clone())),
        };
        // Child frames and workers pause on auto-attach until resumed; only pages skip it.
        if (method == "Runtime.runIfWaitingForDebugger" && kind == Some(Kind::Page)) || method == "Emulation.setFocusEmulationEnabled" {
            return vec![reply(Ok(json!({})))];
        }
        self.next += 1;
        self.pending.insert(self.next, (id, Some(session)));
        let mut forward = json!({ "id": self.next, "method": method, "params": params });
        if let Some(c) = child {
            forward["sessionId"] = json!(c);
        }
        vec![Out::Page(socket, forward.to_string())]
    }

    fn browser_command(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        if ACKNOWLEDGED.contains(&method) || method == "Target.setAutoAttach" {
            return Ok(json!({}));
        }
        match method {
            // The panel's Chrome downloads into its own folder; Playwright's folder and events
            // apply to the downloads its agent starts (`agent_download`).
            "Browser.setDownloadBehavior" => {
                let allowed = params["behavior"] != "deny";
                self.download_dir = params["downloadPath"].as_str().filter(|_| allowed).map(str::to_owned);
                self.download_events = params["eventsEnabled"] == true;
                self.downloads_dirty = true;
                Ok(json!({}))
            }
            "Browser.getVersion" => Ok(json!({ "protocolVersion": "1.3", "product": "HeadlessChrome", "revision": "", "userAgent": "", "jsVersion": "" })),
            "Target.getTargetInfo" => Ok(json!({ "targetInfo": { "targetId": "browser", "type": "browser", "title": "", "url": "", "attached": true, "canAccessOpener": false } })),
            "Target.getBrowserContexts" => Ok(json!({ "browserContextIds": [] })),
            "Target.getTargets" => {
                let mut infos = vec![self.target_info()];
                infos.extend(self.popups.iter().map(|(t, u)| self.popup_info(t, u)));
                Ok(json!({ "targetInfos": infos }))
            }
            "Target.attachToBrowserTarget" => Ok(json!({ "sessionId": self.new_session(Kind::Browser) })),
            "Target.attachToTarget" => {
                let target = params["targetId"].as_str().unwrap_or_default();
                if target == self.target {
                    return Ok(json!({ "sessionId": self.new_session(Kind::Page) }));
                }
                if self.popups.iter().any(|(t, _)| t == target) {
                    return Ok(json!({ "sessionId": self.new_session(Kind::Popup(target.to_owned())) }));
                }
                Err("Only this preview tab's page can be attached.".into())
            }
            "Target.detachFromTarget" => {
                if let Some(s) = params["sessionId"].as_str().filter(|s| *s != PAGE_SESSION_ID) {
                    self.sessions.remove(s);
                }
                Ok(json!({}))
            }
            _ => Err(format!("Not supported for a desktop preview tab: {method}")),
        }
    }

    fn announce_popup(&mut self, target: &str, url: &str) -> Vec<Out> {
        let session = self.new_session(Kind::Popup(target.to_owned()));
        let announce = json!({ "method": "Target.attachedToTarget", "params": { "sessionId": session, "targetInfo": self.popup_info(target, url), "waitingForDebugger": false } });
        vec![Out::Server(announce.to_string())]
    }

    /// The page opened a popup; Playwright sees it attach, as Chrome reports a real one.
    pub fn popup(&mut self, target: String, url: String) -> Vec<Out> {
        if self.popups.iter().any(|(t, _)| *t == target) {
            return vec![];
        }
        self.popups.push((target.clone(), url.clone()));
        if !self.attached {
            return vec![];
        }
        self.announce_popup(&target, &url)
    }

    pub fn popup_gone(&mut self, target: &str) -> Vec<Out> {
        self.popups.retain(|(t, _)| t != target);
        self.children.retain(|_, t| t != target);
        let mut gone: Vec<String> = self.sessions.iter().filter(|(_, k)| **k == Kind::Popup(target.to_owned())).map(|(s, _)| s.clone()).collect();
        gone.sort();
        gone.into_iter()
            .map(|session| {
                self.sessions.remove(&session);
                Out::Server(json!({ "method": "Target.detachedFromTarget", "params": { "sessionId": session, "targetId": target } }).to_string())
            })
            .collect()
    }

    /// A download event from the panel's browser, for Playwright's root session.
    pub fn browser_event(&mut self, method: &str, params: Value) -> Vec<Out> {
        if !self.attached || !self.download_events {
            return vec![];
        }
        vec![Out::Server(json!({ "method": method, "params": params }).to_string())]
    }

    /// A message from the page's socket, or a popup's: a reply to a forwarded command, or an event.
    pub fn page(&mut self, from: Option<&str>, raw: &str) -> Vec<Out> {
        let Ok(mut m) = serde_json::from_str::<Value>(raw) else { return vec![] };
        if let Some(id) = m.get("id").and_then(Value::as_u64) {
            let Some((server_id, session)) = self.pending.remove(&id) else { return vec![] };
            m["id"] = server_id;
            match session {
                Some(s) => m["sessionId"] = json!(s),
                None => {
                    if let Some(o) = m.as_object_mut() {
                        o.remove("sessionId");
                    }
                }
            }
            return vec![Out::Server(m.to_string())];
        }
        let Some(method) = m["method"].as_str() else { return vec![] };
        // The panel's browser reports downloads at its root (`browser_event`).
        if !self.attached || method.starts_with("Page.download") || method.starts_with("Browser.download") {
            return vec![];
        }
        if let (Some(popup), "Target.attachedToTarget") = (from, method) {
            if let Some(child) = m["params"]["sessionId"].as_str() {
                self.children.insert(child.to_owned(), popup.to_owned());
            }
        }
        if m.get("sessionId").is_some_and(|s| s.is_string()) {
            return vec![Out::Server(m.to_string())];
        }
        let wanted = match from {
            None => Kind::Page,
            Some(popup) => Kind::Popup(popup.to_owned()),
        };
        let mut sessions: Vec<&String> = self.sessions.iter().filter(|(_, k)| **k == wanted).map(|(s, _)| s).collect();
        sessions.sort();
        sessions
            .into_iter()
            .map(|s| {
                m["sessionId"] = json!(s);
                Out::Server(m.to_string())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(out: &[Out]) -> Vec<Value> {
        out.iter().filter_map(|o| if let Out::Server(m) = o { serde_json::from_str(m).ok() } else { None }).collect()
    }

    #[test]
    fn playwright_finds_one_page_in_the_relayed_browser() {
        let mut relay = Relay::new("T1".into());
        let version = server(&relay.receive(r#"{"id":1,"method":"Browser.getVersion"}"#));
        assert_eq!(version[0]["id"], 1);
        assert!(version[0]["result"]["product"].is_string());

        // Page events before auto-attach have no session to go to.
        assert!(relay.page(None, r#"{"method":"Page.loadEventFired","params":{}}"#).is_empty());

        let attach = server(&relay.receive(r#"{"id":2,"method":"Target.setAutoAttach","params":{"autoAttach":true,"flatten":true}}"#));
        assert_eq!(attach[0], json!({ "id": 2, "result": {} }));
        assert_eq!(attach[1]["method"], "Target.attachedToTarget");
        assert_eq!(attach[1]["params"]["sessionId"], PAGE_SESSION_ID);
        assert_eq!(attach[1]["params"]["targetInfo"]["targetId"], "T1");
        // A second auto-attach announces nothing new.
        assert_eq!(server(&relay.receive(r#"{"id":3,"method":"Target.setAutoAttach","params":{}}"#)).len(), 1);

        let wrong = server(&relay.receive(r#"{"id":4,"method":"Target.attachToTarget","params":{"targetId":"OTHER"}}"#));
        assert!(wrong[0]["error"]["message"].is_string());
        let unsupported = server(&relay.receive(r#"{"id":5,"method":"Target.createTarget","params":{}}"#));
        assert!(unsupported[0]["error"].is_object());
    }

    #[test]
    fn page_commands_reach_the_page_and_replies_return_on_their_session() {
        let mut relay = Relay::new("T1".into());
        relay.receive(r#"{"id":1,"method":"Target.setAutoAttach","params":{}}"#);
        let out = relay.receive(r#"{"id":7,"method":"Runtime.evaluate","params":{"expression":"1"},"sessionId":"t3-preview-page"}"#);
        let Out::Page(None, forward) = &out[0] else { panic!("not forwarded: {out:?}") };
        let forward: Value = serde_json::from_str(forward).unwrap();
        // The page socket is the page; its commands carry no session.
        assert!(forward.get("sessionId").is_none());
        let reply = server(&relay.page(None, &json!({ "id": forward["id"], "result": { "result": { "value": 1 } } }).to_string()));
        assert_eq!(reply[0], json!({ "id": 7, "result": { "result": { "value": 1 } }, "sessionId": PAGE_SESSION_ID }));

        // A child frame's session passes through both ways.
        let out = relay.receive(r#"{"id":8,"method":"Runtime.enable","sessionId":"CHILD"}"#);
        let Out::Page(None, forward) = &out[0] else { panic!() };
        let forward: Value = serde_json::from_str(forward).unwrap();
        assert_eq!(forward["sessionId"], "CHILD");
        let reply = server(&relay.page(None, &json!({ "id": forward["id"], "result": {}, "sessionId": "CHILD" }).to_string()));
        assert_eq!(reply[0], json!({ "id": 8, "result": {}, "sessionId": "CHILD" }));

        // The page resumes on its own; only children wait for the debugger.
        let resume = server(&relay.receive(r#"{"id":9,"method":"Runtime.runIfWaitingForDebugger","sessionId":"t3-preview-page"}"#));
        assert_eq!(resume[0]["id"], 9);
    }

    #[test]
    fn page_events_reach_every_page_session() {
        let mut relay = Relay::new("T1".into());
        relay.receive(r#"{"id":1,"method":"Target.setAutoAttach","params":{}}"#);
        let extra = server(&relay.receive(r#"{"id":2,"method":"Target.attachToTarget","params":{"targetId":"T1","flatten":true}}"#));
        let extra = extra[0]["result"]["sessionId"].as_str().unwrap().to_owned();
        let events = server(&relay.page(None, r#"{"method":"Page.frameNavigated","params":{"frame":{}}}"#));
        let mut sessions: Vec<_> = events.iter().map(|e| e["sessionId"].as_str().unwrap().to_owned()).collect();
        sessions.sort();
        let mut expected = vec![PAGE_SESSION_ID.to_owned(), extra];
        expected.sort();
        assert_eq!(sessions, expected);
        // A child's events keep their session, and downloads stay the panel's.
        assert_eq!(server(&relay.page(None, r#"{"method":"Runtime.consoleAPICalled","params":{},"sessionId":"CHILD"}"#))[0]["sessionId"], "CHILD");
        assert!(relay.page(None, r#"{"method":"Page.downloadWillBegin","params":{}}"#).is_empty());
    }

    fn page_out(out: &[Out]) -> (Option<String>, Value) {
        let Some(Out::Page(socket, m)) = out.first() else { panic!("not forwarded: {out:?}") };
        (socket.clone(), serde_json::from_str(m).unwrap())
    }

    #[test]
    fn a_popup_the_page_opens_attaches_for_playwright() {
        let mut relay = Relay::new("T1".into());
        // Opened before Playwright looked: announced with the page on auto-attach.
        assert!(relay.popup("P1".into(), "https://a.dev/login".into()).is_empty());
        let attach = server(&relay.receive(r#"{"id":1,"method":"Target.setAutoAttach","params":{}}"#));
        assert_eq!(attach.len(), 3);
        let first = &attach[2]["params"];
        assert_eq!(first["targetInfo"]["targetId"], "P1");
        assert_eq!(first["targetInfo"]["openerId"], "T1");
        assert_eq!(first["targetInfo"]["url"], "https://a.dev/login");
        // A repeat report of the same popup announces nothing.
        assert!(relay.popup("P1".into(), "https://a.dev/login".into()).is_empty());

        let opened = server(&relay.popup("P2".into(), "about:blank".into()));
        assert_eq!(opened[0]["method"], "Target.attachedToTarget");
        let p2 = opened[0]["params"]["sessionId"].as_str().unwrap().to_owned();
        let targets = server(&relay.receive(r#"{"id":2,"method":"Target.getTargets"}"#));
        assert_eq!(targets[0]["result"]["targetInfos"].as_array().unwrap().len(), 3);

        // Its commands go to its own socket, and its replies and events come back on its session.
        let (socket, forward) = page_out(&relay.receive(&json!({ "id": 3, "method": "Page.navigate", "params": { "url": "https://b.dev" }, "sessionId": p2 }).to_string()));
        assert_eq!(socket.as_deref(), Some("P2"));
        assert!(forward.get("sessionId").is_none());
        let reply = server(&relay.page(Some("P2"), &json!({ "id": forward["id"], "result": {} }).to_string()));
        assert_eq!(reply[0], json!({ "id": 3, "result": {}, "sessionId": p2 }));
        let events = server(&relay.page(Some("P2"), r#"{"method":"Page.loadEventFired","params":{}}"#));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["sessionId"], p2.as_str());

        // A frame inside the popup keeps its session, on the popup's socket.
        relay.page(Some("P2"), r#"{"method":"Target.attachedToTarget","params":{"sessionId":"FRAME","targetInfo":{}}}"#);
        let (socket, forward) = page_out(&relay.receive(r#"{"id":4,"method":"Runtime.enable","sessionId":"FRAME"}"#));
        assert_eq!(socket.as_deref(), Some("P2"));
        assert_eq!(forward["sessionId"], "FRAME");

        // Closing it detaches its session, and it leaves the target list.
        let gone = server(&relay.popup_gone("P2"));
        assert_eq!(gone, vec![json!({ "method": "Target.detachedFromTarget", "params": { "sessionId": p2, "targetId": "P2" } })]);
        let targets = server(&relay.receive(r#"{"id":5,"method":"Target.getTargets"}"#));
        assert_eq!(targets[0]["result"]["targetInfos"].as_array().unwrap().len(), 2);
        let refused = server(&relay.receive(r#"{"id":6,"method":"Target.attachToTarget","params":{"targetId":"P2"}}"#));
        assert!(refused[0]["error"].is_object());
    }

    #[test]
    fn downloads_follow_playwrights_folder_and_the_agents_last_input() {
        let mut relay = Relay::new("T1".into());
        relay.receive(r#"{"id":1,"method":"Target.setAutoAttach","params":{}}"#);
        assert!(relay.browser_event("Browser.downloadWillBegin", json!({})).is_empty(), "events stay off until asked for");

        let set = server(&relay.receive(r#"{"id":2,"method":"Browser.setDownloadBehavior","params":{"behavior":"allowAndName","downloadPath":"/tmp/pw","eventsEnabled":true}}"#));
        assert_eq!(set[0], json!({ "id": 2, "result": {} }));
        assert!(relay.downloads_changed());
        assert!(!relay.downloads_changed());
        assert_eq!(relay.download_dir.as_deref(), Some("/tmp/pw"));
        let event = server(&relay.browser_event("Browser.downloadProgress", json!({ "guid": "g", "state": "completed" })));
        assert_eq!(event[0], json!({ "method": "Browser.downloadProgress", "params": { "guid": "g", "state": "completed" } }));

        // Only acting on the page marks the next download as the agent's.
        relay.receive(r#"{"id":3,"method":"Page.enable","sessionId":"t3-preview-page"}"#);
        assert!(relay.input_at.is_none());
        relay.receive(r#"{"id":4,"method":"Input.dispatchMouseEvent","params":{},"sessionId":"t3-preview-page"}"#);
        assert!(relay.input_at.is_some());
        assert!(relay.downloads_changed());
        assert!(["Page.navigate", "Runtime.evaluate", "Runtime.callFunctionOn", "Input.insertText"].iter().all(|m| agent_input(m)));
        assert!(!agent_input("DOM.getDocument"));

        relay.receive(r#"{"id":5,"method":"Browser.setDownloadBehavior","params":{"behavior":"deny","downloadPath":"/tmp/pw"}}"#);
        assert_eq!(relay.download_dir, None);
        assert!(relay.browser_event("Browser.downloadWillBegin", json!({})).is_empty());
    }

    #[test]
    fn the_servers_pointer_messages_become_cursor_positions() {
        let click = pointer_from(&json!({ "type": "pointer", "phase": "click", "x": 12.5, "y": 40 }), 3).unwrap();
        assert!(click.click);
        assert_eq!((click.x, click.y, click.sequence), (12.5, 40., 3));
        assert!(!pointer_from(&json!({ "type": "pointer", "phase": "move", "x": 1, "y": 2 }), 4).unwrap().click);
        assert!(pointer_from(&json!({ "type": "pointer", "phase": "move", "x": 1 }), 5).is_none());
    }
}
