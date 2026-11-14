//! Web's `ServerBrowserSurface` client: a server preview session this panel doesn't render
//! itself (another machine's, or a headless one) arrives as JPEG frames over
//! `/api/preview-stream/ws`, and the panel's input goes back as `PreviewStreamInput`.
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::Pixels;
use serde_json::{Value, json};
use tungstenite::Message;

use super::agent::{Key, Pointer};
use super::browser::Cmd;

const TAB_GONE: u16 = 4404;
const HOST_SETUP: u16 = 4503;
const RECONNECT: u16 = 1012;
/// The largest frame the server sends, in device pixels, as web's default cap.
const MAX_FRAME: (u32, u32) = (2560, 1600);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub connected: bool,
    pub painted: bool,
    /// The page's CSS viewport.
    pub viewport: Option<(u32, u32)>,
    pub can_operate: bool,
    /// `agent`, `you`, `another-viewer` or `unclaimed`, once the server says.
    pub controller: Option<String>,
    pub pointer: Option<Pointer>,
    /// The session ended, or the server can't run its browser (`error` says why).
    pub ended: bool,
    pub error: Option<String>,
}

pub struct Stream {
    tx: Sender<String>,
    state: Arc<Mutex<State>>,
    pressed: AtomicBool,
}

impl Stream {
    pub fn open(base: String, bearer: String, key: &Key, pixels: Pixels, waker: rune::Waker) -> Stream {
        let (tx, rx) = channel();
        let state = Arc::new(Mutex::new(State::default()));
        let query = url_query(key);
        let shared = state.clone();
        std::thread::spawn(move || run(base, bearer, query, pixels, shared, rx, waker));
        Stream { tx, state, pressed: AtomicBool::new(false) }
    }

    pub fn state(&self) -> State {
        self.state.lock().unwrap().clone()
    }

    /// Web's client gate: page input only while this viewer has control.
    fn send(&self, input: Value) {
        let state = self.state();
        let control = input["type"] == "takeControl" || input["type"] == "resize";
        if state.connected && state.can_operate && (control || state.controller.as_deref() == Some("you")) {
            let _ = self.tx.send(input.to_string());
        }
    }

    pub fn toggle_control(&self) {
        let take = self.state().controller.as_deref() != Some("you");
        self.send(json!({ "type": if take { "takeControl" } else { "releaseControl" } }));
    }

    /// The panel's area for a page that fills it; the server follows it for a `fill` session.
    pub fn resize(&self, w: f32, h: f32) {
        let _ = self.tx.send(json!({ "type": "resize", "width": w.round().max(1.), "height": h.round().max(1.) }).to_string());
    }

    /// A panel command for this tab, with page-area points over `scale` as CSS pixels.
    pub fn command(&self, cmd: &Cmd, scale: f32) {
        let mut pressed = self.pressed.load(Ordering::Relaxed);
        for input in inputs(cmd, scale, &mut pressed) {
            self.send(input);
        }
        self.pressed.store(pressed, Ordering::Relaxed);
    }
}

fn url_query(key: &Key) -> String {
    let mut q = s5_net::url::form_urlencoded::Serializer::new(String::new());
    q.append_pair("threadId", &key.0).append_pair("tabId", &key.1).append_pair("maxWidth", &MAX_FRAME.0.to_string()).append_pair("maxHeight", &MAX_FRAME.1.to_string());
    q.finish()
}

/// `PreviewStreamInput` for a panel command; most are one message, a key press two.
fn inputs(cmd: &Cmd, scale: f32, pressed: &mut bool) -> Vec<Value> {
    let scale = scale.max(0.01);
    match cmd {
        Cmd::Mouse { kind, x, y, clicks } => {
            let action = match *kind {
                "mousePressed" => "down",
                "mouseReleased" => "up",
                _ => "move",
            };
            match action {
                "down" => *pressed = true,
                "up" => *pressed = false,
                _ => {}
            }
            let button = if action != "move" || *pressed { "left" } else { "none" };
            let click_count = if action == "move" { 0 } else { (*clicks).max(1) };
            vec![json!({ "type": "mouse", "action": action, "x": x / scale, "y": y / scale, "button": button, "buttons": if *pressed { 1 } else { 0 }, "clickCount": click_count, "modifiers": 0 })]
        }
        Cmd::Wheel { x, y, dx, dy } => vec![json!({ "type": "wheel", "x": x / scale, "y": y / scale, "deltaX": -dx, "deltaY": -dy, "modifiers": 0 })],
        Cmd::Text(text) => vec![json!({ "type": "text", "text": text })],
        Cmd::Key(key) => {
            let code = super::browser::key_code(key);
            ["down", "up"]
                .into_iter()
                .map(|action| {
                    let mut m = json!({ "type": "key", "action": action, "key": key, "code": key, "keyCode": code, "modifiers": 0 });
                    if *key == "Enter" && action == "down" {
                        m["text"] = json!("\r");
                    }
                    m
                })
                .collect()
        }
        Cmd::Navigate(_, url) => vec![json!({ "type": "navigate", "url": url })],
        Cmd::Back(_) => vec![json!({ "type": "history", "delta": -1 })],
        Cmd::Forward(_) => vec![json!({ "type": "history", "delta": 1 })],
        Cmd::Reload { hard, .. } => vec![json!({ "type": "reload", "ignoreCache": hard })],
        Cmd::Dialog { accept, text, .. } => vec![json!({ "type": "dialog", "accept": accept, "promptText": text })],
        _ => vec![],
    }
}

/// A JSON message from the stream; whether it changed what the panel shows.
fn apply(state: &mut State, message: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(message) else { return false };
    match v["type"].as_str() {
        Some("viewport") => {
            let (Some(w), Some(h)) = (v["width"].as_f64(), v["height"].as_f64()) else { return false };
            state.viewport = Some((w.round() as u32, h.round() as u32));
        }
        Some("pointer") => {
            let (Some(x), Some(y), Some(sequence)) = (v["x"].as_f64(), v["y"].as_f64(), v["sequence"].as_u64()) else { return false };
            state.pointer = Some(Pointer { click: v["phase"] == "click", x: x as f32, y: y as f32, sequence, at: Instant::now() });
        }
        Some("control") => {
            let Some(controller) = v["controller"].as_str() else { return false };
            state.can_operate = v["canOperate"] == true;
            state.controller = Some(controller.to_owned());
        }
        _ => return false,
    }
    true
}

enum End {
    Stopped,
    Ended(Option<String>),
    Reconnect,
    Lost,
}

fn run(base: String, bearer: String, query: String, pixels: Pixels, state: Arc<Mutex<State>>, rx: Receiver<String>, waker: rune::Waker) {
    let mut failures = 0u32;
    let mut resize: Option<String> = None;
    loop {
        let end = match s5_net::api_socket(&base, &bearer, "/api/preview-stream/ws", &query, Duration::from_millis(15)) {
            Ok(mut socket) => {
                failures = 0;
                update(&state, &waker, |s| {
                    s.connected = true;
                    s.error = None;
                });
                let end = pump(&mut socket, &pixels, &state, &rx, &waker, &mut resize);
                update(&state, &waker, |s| {
                    s.connected = false;
                    s.controller = None;
                });
                end
            }
            Err(e) => {
                update(&state, &waker, |s| s.error = Some(e));
                End::Lost
            }
        };
        match end {
            End::Stopped => return,
            End::Ended(error) => {
                update(&state, &waker, |s| {
                    s.ended = true;
                    s.error = error;
                });
                return;
            }
            End::Reconnect => {}
            // Web's backoff: 500 ms doubling to 10 s, waking early if the panel lets go.
            End::Lost => {
                let wait = Duration::from_millis((500u64 << failures.min(5)).min(10_000));
                failures += 1;
                let until = Instant::now() + wait;
                while Instant::now() < until {
                    match rx.recv_timeout(until - Instant::now()) {
                        Ok(m) if m.contains("\"resize\"") => resize = Some(m),
                        Ok(_) => {}
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
            }
        }
    }
}

fn update(state: &Mutex<State>, waker: &rune::Waker, f: impl FnOnce(&mut State)) {
    f(&mut state.lock().unwrap());
    waker();
}

type Socket = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

fn pump(socket: &mut Socket, pixels: &Pixels, state: &Mutex<State>, rx: &Receiver<String>, waker: &rune::Waker, resize: &mut Option<String>) -> End {
    if let Some(r) = resize.clone() {
        let _ = socket.send(Message::Text(r));
    }
    loop {
        loop {
            match rx.try_recv() {
                Ok(m) => {
                    if m.contains("\"resize\"") {
                        *resize = Some(m.clone());
                    }
                    if socket.send(Message::Text(m)).is_err() {
                        return End::Lost;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = socket.close(None);
                    let _ = socket.flush();
                    return End::Stopped;
                }
            }
        }
        match socket.read() {
            Ok(Message::Binary(jpeg)) => {
                if let Ok(image) = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg) {
                    let rgba = image.to_rgba8();
                    pixels.set(rgba.width(), rgba.height(), rgba.into_raw());
                    state.lock().unwrap().painted = true;
                    waker();
                }
                // The server sends the next frame only after this one is acknowledged.
                if socket.send(Message::Text(r#"{"type":"ack"}"#.into())).is_err() {
                    return End::Lost;
                }
            }
            Ok(Message::Text(text)) => {
                if apply(&mut state.lock().unwrap(), &text) {
                    waker();
                }
            }
            Ok(Message::Close(frame)) => {
                return match frame.as_ref().map(|f| (u16::from(f.code), f.reason.to_string())) {
                    Some((TAB_GONE, _)) => End::Ended(None),
                    Some((HOST_SETUP, reason)) => End::Ended(Some(host_setup(&reason))),
                    Some((RECONNECT, _)) => End::Reconnect,
                    _ => End::Lost,
                };
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => return End::Lost,
        }
    }
}

/// `PreviewStreamHostSetup`: what the server's machine needs before it can run its browser.
fn host_setup(reason: &str) -> String {
    let setup: Value = serde_json::from_str(reason).unwrap_or_default();
    let need = if setup["need"] == "sandbox" { "a Chrome sandbox" } else { "Chrome's system libraries" };
    match setup["command"].as_str() {
        Some(command) => format!("The server needs {need}. Run on that machine: {command}"),
        None => format!("The server needs {need}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_input_becomes_stream_input_in_css_pixels() {
        let mut pressed = false;
        let down = inputs(&Cmd::Mouse { kind: "mousePressed", x: 100., y: 50., clicks: 1 }, 2., &mut pressed);
        assert_eq!(down, vec![json!({ "type": "mouse", "action": "down", "x": 50., "y": 25., "button": "left", "buttons": 1, "clickCount": 1, "modifiers": 0 })]);
        let drag = inputs(&Cmd::Mouse { kind: "mouseMoved", x: 10., y: 10., clicks: 0 }, 2., &mut pressed);
        assert_eq!((drag[0]["button"].clone(), drag[0]["buttons"].clone(), drag[0]["clickCount"].clone()), (json!("left"), json!(1), json!(0)));
        inputs(&Cmd::Mouse { kind: "mouseReleased", x: 10., y: 10., clicks: 1 }, 2., &mut pressed);
        assert!(!pressed);
        let hover = inputs(&Cmd::Mouse { kind: "mouseMoved", x: 10., y: 10., clicks: 0 }, 1., &mut pressed);
        assert_eq!(hover[0]["button"], "none");

        assert_eq!(inputs(&Cmd::Wheel { x: 4., y: 6., dx: 0., dy: 3. }, 2., &mut pressed)[0], json!({ "type": "wheel", "x": 2., "y": 3., "deltaX": -0., "deltaY": -3., "modifiers": 0 }));
        let enter = inputs(&Cmd::Key("Enter"), 1., &mut pressed);
        assert_eq!(enter.len(), 2);
        assert_eq!((enter[0]["action"].clone(), enter[0]["keyCode"].clone(), enter[0]["text"].clone()), (json!("down"), json!(13), json!("\r")));
        assert!(enter[1].get("text").is_none());
        assert_eq!(inputs(&Cmd::Back(1), 1., &mut pressed)[0], json!({ "type": "history", "delta": -1 }));
        assert_eq!(inputs(&Cmd::Reload { tab: 1, hard: true }, 1., &mut pressed)[0], json!({ "type": "reload", "ignoreCache": true }));
        assert!(inputs(&Cmd::ClearCache, 1., &mut pressed).is_empty());
    }

    #[test]
    fn stream_messages_update_the_viewer() {
        let mut state = State::default();
        assert!(apply(&mut state, r#"{"type":"viewport","width":390,"height":844}"#));
        assert_eq!(state.viewport, Some((390, 844)));
        assert!(apply(&mut state, r#"{"type":"control","canOperate":true,"controller":"agent","generation":2,"dialog":null}"#));
        assert_eq!((state.can_operate, state.controller.as_deref()), (true, Some("agent")));
        assert!(apply(&mut state, r#"{"type":"pointer","phase":"click","x":12,"y":30,"sequence":7}"#));
        let pointer = state.pointer.clone().unwrap();
        assert_eq!((pointer.click, pointer.x, pointer.y, pointer.sequence), (true, 12., 30., 7));
        assert!(!apply(&mut state, r#"{"type":"clipboard","text":"x"}"#));
        assert!(!apply(&mut state, "not json"));
        assert_eq!(host_setup(r#"{"need":"libraries","command":"npx playwright install-deps"}"#), "The server needs Chrome's system libraries. Run on that machine: npx playwright install-deps");
    }

    #[test]
    fn the_stream_url_names_the_session_and_frame_cap() {
        assert_eq!(url_query(&("thread 1".into(), "tab-2".into())), "threadId=thread+1&tabId=tab-2&maxWidth=2560&maxHeight=1600");
    }
}
