//! Agents drive the panel's tabs as they drive Electron's: the server reads `attached` and CDP
//! events from one fd and writes CDP commands to another (`DesktopBrowserChannel`), and drives
//! each attached tab with Playwright. Playwright wants a browser; each tab gets a relay that
//! answers as a browser holding that one page and forwards the rest to the page's own DevTools
//! socket, as Electron's `CdpRelay` does over a webview's debugger.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tungstenite::Message;

/// A preview session: `(threadId, tabId)`.
pub type Key = (String, String);

struct Channel {
    writer: Arc<Mutex<UnixStream>>,
    /// Each attached tab's DevTools `host:port` and page target.
    targets: HashMap<Key, (String, String)>,
    relays: HashMap<Key, Sender<String>>,
}

static CHANNEL: Mutex<Option<Channel>> = Mutex::new(None);
/// Bumped per server process, so the panel announces its tabs to each new one.
static GENERATION: Mutex<u64> = Mutex::new(0);

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
    if ch.targets.remove(key).is_some() {
        write(&ch.writer, &json!({ "type": "detached", "threadId": key.0, "tabId": key.1 }));
    }
}

fn write(writer: &Mutex<UnixStream>, event: &Value) {
    let _ = writer.lock().unwrap().write_all(format!("{event}\n").as_bytes());
}

fn read_commands(commands: UnixStream, mine: Arc<Mutex<UnixStream>>) {
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
                    let _ = tx.send(message.to_owned());
                }
            }
            // The server let go of the tab; its relay and page socket close.
            Some("release") => {
                ch.relays.remove(&key);
            }
            // `pointer` moves Electron's agent cursor and `turn` keeps a webview focused; a
            // headless page needs neither.
            _ => {}
        }
    }
    // The server exited; the next one gets a new channel.
    let mut guard = CHANNEL.lock().unwrap();
    if guard.as_ref().is_some_and(|ch| Arc::ptr_eq(&ch.writer, &mine)) {
        *guard = None;
    }
}

fn run_relay(key: Key, host: String, target: String, rx: Receiver<String>, writer: Arc<Mutex<UnixStream>>) {
    let send_server = |message: String| write(&writer, &json!({ "type": "cdp", "threadId": key.0, "tabId": key.1, "message": message }));
    let url = format!("ws://{host}/devtools/page/{target}");
    let Ok(stream) = TcpStream::connect(&host) else { return };
    let Ok((mut ws, _)) = tungstenite::client(url.as_str(), stream) else { return };
    if ws.get_ref().set_read_timeout(Some(Duration::from_millis(10))).is_err() {
        return;
    }
    let mut relay = Relay::new(target);
    loop {
        loop {
            match rx.try_recv() {
                Ok(raw) => {
                    for out in relay.receive(&raw) {
                        match out {
                            Out::Server(m) => send_server(m),
                            Out::Page(m) => {
                                if ws.send(Message::text(m)).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = ws.close(None);
                    return;
                }
            }
        }
        match ws.read() {
            Ok(Message::Text(t)) => {
                for out in relay.page(&t) {
                    if let Out::Server(m) = out {
                        send_server(m);
                    }
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => return,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Out {
    Server(String),
    Page(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Browser,
    Page,
}

const BROWSER_CONTEXT_ID: &str = "t3-preview";
const PAGE_SESSION_ID: &str = "t3-preview-page";
/// Browser commands a single headless page has nothing to do for. Downloads keep the panel's
/// own handling, so an agent's land in Downloads as a person's do.
const ACKNOWLEDGED: [&str; 6] = [
    "Target.setDiscoverTargets",
    "Browser.grantPermissions",
    "Browser.resetPermissions",
    "Browser.cancelDownload",
    "Browser.setDownloadBehavior",
    "Target.setRemoteLocations",
];

/// One Playwright connection's view of a page: a browser with exactly this page in it.
pub struct Relay {
    target: String,
    attached: bool,
    sessions: HashMap<String, Kind>,
    sequence: u32,
    next: u64,
    /// Our page-socket ids: the server's id and the session its reply goes back on.
    pending: HashMap<u64, (Value, Option<String>)>,
}

impl Relay {
    pub fn new(target: String) -> Relay {
        Relay { target, attached: false, sessions: HashMap::from([(PAGE_SESSION_ID.to_owned(), Kind::Page)]), sequence: 0, next: 0, pending: HashMap::new() }
    }

    fn new_session(&mut self, kind: Kind) -> String {
        self.sequence += 1;
        let id = format!("t3-preview-{}-{}", if kind == Kind::Browser { "browser" } else { "page" }, self.sequence);
        self.sessions.insert(id.clone(), kind);
        id
    }

    fn target_info(&self) -> Value {
        json!({ "targetId": self.target, "type": "page", "title": "", "url": "", "attached": true, "canAccessOpener": false, "browserContextId": BROWSER_CONTEXT_ID })
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
        let browser = match &session {
            None => true,
            Some(s) => self.sessions.get(s) == Some(&Kind::Browser),
        };
        if browser {
            if method == "Target.setAutoAttach" && !self.attached {
                self.attached = true;
                let announce = json!({ "method": "Target.attachedToTarget", "params": { "sessionId": PAGE_SESSION_ID, "targetInfo": self.target_info(), "waitingForDebugger": false } });
                return vec![reply(Ok(json!({}))), Out::Server(announce.to_string())];
            }
            let result = self.browser_command(method, &params);
            return vec![reply(result)];
        }
        let session = session.unwrap_or_default();
        // A relay page session is the page itself; any other id is a child frame or worker.
        let child = (self.sessions.get(&session) != Some(&Kind::Page)).then(|| session.clone());
        // Child frames and workers pause on auto-attach until resumed; only the page skips it.
        if (method == "Runtime.runIfWaitingForDebugger" && child.is_none()) || method == "Emulation.setFocusEmulationEnabled" {
            return vec![reply(Ok(json!({})))];
        }
        self.next += 1;
        self.pending.insert(self.next, (id, Some(session)));
        let mut forward = json!({ "id": self.next, "method": method, "params": params });
        if let Some(c) = child {
            forward["sessionId"] = json!(c);
        }
        vec![Out::Page(forward.to_string())]
    }

    fn browser_command(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        if ACKNOWLEDGED.contains(&method) || method == "Target.setAutoAttach" {
            return Ok(json!({}));
        }
        match method {
            "Browser.getVersion" => Ok(json!({ "protocolVersion": "1.3", "product": "HeadlessChrome", "revision": "", "userAgent": "", "jsVersion": "" })),
            "Target.getTargetInfo" => Ok(json!({ "targetInfo": { "targetId": "browser", "type": "browser", "title": "", "url": "", "attached": true, "canAccessOpener": false } })),
            "Target.getBrowserContexts" => Ok(json!({ "browserContextIds": [] })),
            "Target.getTargets" => Ok(json!({ "targetInfos": [self.target_info()] })),
            "Target.attachToBrowserTarget" => Ok(json!({ "sessionId": self.new_session(Kind::Browser) })),
            "Target.attachToTarget" => {
                if params["targetId"].as_str() != Some(self.target.as_str()) {
                    return Err("Only this preview tab's page can be attached.".into());
                }
                Ok(json!({ "sessionId": self.new_session(Kind::Page) }))
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

    /// A message from the page's socket: a reply to a forwarded command, or an event.
    pub fn page(&mut self, raw: &str) -> Vec<Out> {
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
        // Downloads are the panel's; Playwright never asked for their events.
        if !self.attached || method.starts_with("Page.download") || method.starts_with("Browser.download") {
            return vec![];
        }
        if m.get("sessionId").is_some_and(|s| s.is_string()) {
            return vec![Out::Server(m.to_string())];
        }
        let mut pages: Vec<&String> = self.sessions.iter().filter(|(_, k)| **k == Kind::Page).map(|(s, _)| s).collect();
        pages.sort();
        pages
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
        assert!(relay.page(r#"{"method":"Page.loadEventFired","params":{}}"#).is_empty());

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
        let Out::Page(forward) = &out[0] else { panic!("not forwarded: {out:?}") };
        let forward: Value = serde_json::from_str(forward).unwrap();
        // The page socket is the page; its commands carry no session.
        assert!(forward.get("sessionId").is_none());
        let reply = server(&relay.page(&json!({ "id": forward["id"], "result": { "result": { "value": 1 } } }).to_string()));
        assert_eq!(reply[0], json!({ "id": 7, "result": { "result": { "value": 1 } }, "sessionId": PAGE_SESSION_ID }));

        // A child frame's session passes through both ways.
        let out = relay.receive(r#"{"id":8,"method":"Runtime.enable","sessionId":"CHILD"}"#);
        let Out::Page(forward) = &out[0] else { panic!() };
        let forward: Value = serde_json::from_str(forward).unwrap();
        assert_eq!(forward["sessionId"], "CHILD");
        let reply = server(&relay.page(&json!({ "id": forward["id"], "result": {}, "sessionId": "CHILD" }).to_string()));
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
        let events = server(&relay.page(r#"{"method":"Page.frameNavigated","params":{"frame":{}}}"#));
        let mut sessions: Vec<_> = events.iter().map(|e| e["sessionId"].as_str().unwrap().to_owned()).collect();
        sessions.sort();
        let mut expected = vec![PAGE_SESSION_ID.to_owned(), extra];
        expected.sort();
        assert_eq!(sessions, expected);
        // A child's events keep their session, and downloads stay the panel's.
        assert_eq!(server(&relay.page(r#"{"method":"Runtime.consoleAPICalled","params":{},"sessionId":"CHILD"}"#))[0]["sessionId"], "CHILD");
        assert!(relay.page(r#"{"method":"Page.downloadWillBegin","params":{}}"#).is_empty());
    }
}
