//! Device input as web's `createDeviceStreamClient`: an iOS simulator's own socket behind the
//! device hub (serve-sim's `/helper/ws`, a tag byte and then JSON), or an Android emulator's
//! serve-emu socket, shared with its video (JSON text).

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::Key;
use serde_json::{Value, json};
use tungstenite::Message;

const PATH: &str = "/api/device-hub/vendor/serve-sim/helper/ws";
const TOUCH: u8 = 0x03;
const BUTTON: u8 = 0x04;
const KEY: u8 = 0x06;
const ORIENTATION: u8 = 0x07;
const SCREEN_CONFIG: u8 = 0x82;
const ADMITTED: u8 = 0x83;
const SHIFT: u8 = 0xe1;
const ORIENTATIONS: [&str; 4] = ["portrait", "landscape_left", "portrait_upside_down", "landscape_right"];
/// serve-emu's `MAX_TEXT_BYTES` (expo-device-hub 0.15.3 input.js).
const ANDROID_TEXT_BYTES: usize = 300;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Screen {
    pub width: f64,
    pub height: f64,
    pub orientation: String,
    /// The iPhone Duo's hinge: which panel shows (1 is the cover), whether it folds, its angle
    /// and stand (`closed`, `book`, `open`, `laptop` or `tent`).
    pub screen_id: Option<u32>,
    pub supports_hinge: bool,
    /// Whether the device can be asked which display faces up (`physical` commands).
    pub supports_physical: bool,
    pub hinge_angle: Option<f64>,
    pub hinge_pose: Option<String>,
}

#[derive(Default)]
pub struct State {
    pub admitted: bool,
    pub unavailable: bool,
    pub screen: Option<Screen>,
    pub duo: Duo,
    /// Bumped on each change the panel shows, since a still screen sends no frames.
    pub rev: u64,
}

const DUO_COMMAND: u8 = 0x10;
const DUO_REPLY: u8 = 0x90;
const DUO_TIMEOUT: Duration = Duration::from_secs(5);

/// Web's `createDuoControl`: one hinge command in flight, the latest queued behind it, and the
/// error that left the device's position unknown.
#[derive(Default)]
pub struct Duo {
    next: u64,
    active: Option<(u64, Value, Instant)>,
    queued: Option<Value>,
    /// The orientation command a screen config answers, since serve-sim sends no reply for it.
    orientation: Option<u64>,
    pub error: Option<String>,
    /// Messages for the socket.
    outbox: Vec<Vec<u8>>,
}

impl Duo {
    pub fn pending(&self) -> bool {
        self.active.is_some()
    }

    /// The command waiting or in flight, newest first.
    pub fn requested(&self) -> Option<&Value> {
        self.queued.as_ref().or(self.active.as_ref().map(|a| &a.1))
    }

    fn enqueue(&mut self, command: Value, now: Instant) {
        if command["control"] == "angle" && !command["value"].as_f64().is_some_and(|v| (0. ..=180.).contains(&v)) {
            return;
        }
        self.queued = Some(command);
        self.error = None;
        self.drain(now);
    }

    fn drain(&mut self, now: Instant) {
        if self.active.is_some() {
            return;
        }
        let Some(command) = self.queued.take() else { return };
        self.next += 1;
        let id = self.next;
        self.orientation = None;
        self.outbox.push(if command["control"] == "orientation" {
            self.orientation = Some(id);
            tagged(ORIENTATION, &json!({"orientation": command["value"]}))
        } else {
            tagged(DUO_COMMAND, &json!({"requestId": id, "command": command}))
        });
        self.active = Some((id, command, now));
    }

    fn receive(&mut self, id: u64, ok: bool, error: Option<&str>, now: Instant) {
        if self.active.as_ref().is_none_or(|a| a.0 != id) {
            return;
        }
        self.active = None;
        if !ok {
            return self.clear(Some(error.unwrap_or("Device control failed. Its position is unknown.").to_owned()));
        }
        self.drain(now);
    }

    fn clear(&mut self, error: Option<String>) {
        (self.active, self.queued, self.orientation, self.error) = (None, None, None, error);
        self.outbox.clear();
    }

    /// When the command in flight gives up.
    pub fn deadline(&self) -> Option<Instant> {
        self.active.as_ref().map(|a| a.2 + DUO_TIMEOUT)
    }

    /// Gives up on a command unanswered past its deadline; true when it did.
    pub fn expire(&mut self, now: Instant) -> bool {
        let late = self.deadline().is_some_and(|at| now >= at);
        if late {
            self.clear(Some("Device control timed out. Its position is unknown.".into()));
        }
        late
    }
}

/// Web's `duoFoldState`. The inner panel is mounted a quarter turn from the cover, so its
/// landscape means a vertical phone; a fold across closed hands off in steps, and only a settled
/// screen's orientation says how the phone is held.
#[derive(Debug, PartialEq)]
pub struct DuoFold {
    pub fold: &'static str,
    pub stand: bool,
    pub phone_vertical: bool,
    pub settled: bool,
}

pub fn duo_fold(screen: &Screen) -> DuoFold {
    let cover = screen.screen_id == Some(1);
    let angle = screen.hinge_angle.unwrap_or(if cover { 0. } else { 180. });
    let landscape = screen.orientation.starts_with("landscape");
    DuoFold {
        fold: if angle == 0. {
            "closed"
        } else if angle == 180. {
            "open"
        } else {
            "half"
        },
        stand: matches!(screen.hinge_pose.as_deref(), Some("laptop" | "tent")),
        phone_vertical: if cover { !landscape } else { landscape },
        settled: cover == (angle == 0.),
    }
}

/// Web's `duoHoldOrientation`: the orientation that holds a phone in a stand upright or flat.
pub fn duo_hold(vertical: bool, screen_id: Option<u32>) -> &'static str {
    match (screen_id == Some(1), vertical) {
        (true, true) => "portrait",
        (true, false) | (false, true) => "landscape_left",
        (false, false) => "portrait_upside_down",
    }
}

impl State {
    /// Input reaches the simulator only once serve-sim admits this client.
    pub fn ready(&self) -> bool {
        self.admitted && !self.unavailable
    }
}

/// One device's input; dropping it closes an iOS socket (Android's closes with its video).
pub struct Input {
    tx: Sender<Message>,
    pub state: Arc<Mutex<State>>,
    android: bool,
}

impl Input {
    pub fn open(base: String, auth: s5_net::Auth, host: &str, device: &str, wake: impl Fn() + Send + 'static) -> Input {
        let (input, rx) = Input::channel(false);
        let query = format!("device={}&hostId={}", super::encode(device), super::encode(host));
        let shared = input.state.clone();
        std::thread::spawn(move || run(&base, &auth, &query, &rx, &shared, &wake));
        input
    }

    /// Input whose messages another socket's loop sends: Android's serve-emu video socket.
    pub fn channel(android: bool) -> (Input, Receiver<Message>) {
        let (tx, rx) = channel();
        (Input { tx, state: Arc::default(), android }, rx)
    }

    fn send(&self, message: Message) {
        let _ = self.tx.send(message);
    }

    fn send_json(&self, value: Value) {
        self.send(Message::Text(value.to_string()));
    }

    /// A touch at `(x, y)` on the shown screen, each 0 to 1.
    pub fn touch(&self, phase: &str, x: f64, y: f64) {
        let (x, y) = (x.clamp(0., 1.), y.clamp(0., 1.));
        if self.android {
            let action = match phase {
                "begin" => "down",
                "end" => "up",
                _ => "move",
            };
            return self.send_json(json!({"type": "touch", "action": action, "x": x, "y": y}));
        }
        let screen = self.state.lock().unwrap_or_else(|e| e.into_inner()).screen.clone();
        let (x, y) = raw_point(screen.as_ref(), x, y);
        self.send(Message::Binary(tagged(TOUCH, &json!({"type": phase, "x": x, "y": y}))));
    }

    /// Web's `sendRawTouch`: a touch at the display's own raw point, as the iPhone Duo's 3D view
    /// works it out per display.
    pub fn touch_raw(&self, phase: &str, x: f64, y: f64) {
        if !self.android {
            self.send(Message::Binary(tagged(TOUCH, &json!({"type": phase, "x": x.clamp(0., 1.), "y": y.clamp(0., 1.)}))));
        }
    }

    /// Web's `pressButton`: `home`, `back`, `recents` or `power`.
    pub fn button(&self, button: &str) {
        if self.android {
            return self.send_json(json!({"type": button}));
        }
        let name = match button {
            "home" => "home",
            "recents" => "app_switcher",
            "power" => "lock",
            _ => return,
        };
        self.send(Message::Binary(tagged(BUTTON, &json!({"button": name}))));
    }

    /// A hinge command (`{control, value}`) for an iPhone Duo, queued behind the one in flight.
    pub fn duo(&self, command: Value) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let hinged = state.ready() && state.screen.as_ref().is_some_and(|s| s.supports_hinge);
        state.duo.enqueue(command, Instant::now());
        state.rev += 1;
        for message in std::mem::take(&mut state.duo.outbox) {
            if !hinged || self.tx.send(Message::Binary(message)).is_err() {
                state.duo.clear(Some("Device is disconnected.".into()));
            }
        }
    }

    /// Web's `setOrientation`, on iOS.
    pub fn orient(&self, orientation: &str) {
        if !self.android {
            self.send(Message::Binary(tagged(ORIENTATION, &json!({"orientation": orientation}))));
        }
    }

    pub fn rotate(&self) {
        if self.android {
            return;
        }
        let current = self.state.lock().unwrap_or_else(|e| e.into_inner()).screen.as_ref().map(|s| s.orientation.clone());
        self.send(Message::Binary(tagged(ORIENTATION, &json!({"orientation": next_orientation(current.as_deref())}))));
    }

    /// Presses for `key`; false when the device has no key for it.
    pub fn key(&self, key: &Key) -> bool {
        if self.android {
            let messages = android_key(key);
            let any = !messages.is_empty();
            for message in messages {
                self.send_json(message);
            }
            return any;
        }
        let presses = presses(key);
        for message in presses.iter().flat_map(|&(usage, shift)| press(usage, shift)) {
            self.send(Message::Binary(message));
        }
        !presses.is_empty()
    }
}

/// serve-emu's messages for a key: Escape is Back, named keys are Android keycodes, and text
/// goes as text in chunks it accepts.
fn android_key(key: &Key) -> Vec<Value> {
    let code = |keycode: u32| vec![json!({"type": "key", "keycode": keycode})];
    match key {
        Key::Escape => vec![json!({"type": "back"})],
        Key::Up => code(19),
        Key::Down => code(20),
        Key::Left => code(21),
        Key::Right => code(22),
        Key::Tab => code(61),
        Key::Enter => code(66),
        Key::Backspace => code(67),
        Key::PageUp => code(92),
        Key::PageDown => code(93),
        Key::Text(text) => {
            let mut chunks = vec![String::new()];
            for c in text.chars() {
                if chunks.last().is_some_and(|last| last.len() + c.len_utf8() > ANDROID_TEXT_BYTES) {
                    chunks.push(String::new());
                }
                chunks.last_mut().expect("never empty").push(c);
            }
            chunks.into_iter().filter(|c| !c.is_empty()).map(|text| json!({"type": "text", "text": text})).collect()
        }
        _ => vec![],
    }
}

fn tagged(tag: u8, payload: &Value) -> Vec<u8> {
    let mut out = vec![tag];
    out.extend_from_slice(payload.to_string().as_bytes());
    out
}

/// Applies one message from serve-sim; true when what the panel shows changed.
fn inbound(bytes: &[u8], state: &mut State) -> bool {
    match bytes.split_first() {
        Some((&ADMITTED, _)) => {
            state.admitted = true;
            true
        }
        Some((&SCREEN_CONFIG, json)) => {
            let Ok(config) = serde_json::from_slice::<Value>(json) else { return false };
            let (Some(width), Some(height)) = (config["width"].as_f64(), config["height"].as_f64()) else { return false };
            state.unavailable = config["inputUnavailable"] == true;
            state.screen = Some(Screen {
                width,
                height,
                orientation: config["orientation"].as_str().unwrap_or("portrait").to_owned(),
                screen_id: config["screenId"].as_u64().map(|id| id as u32),
                supports_hinge: config["supportsHingeAngle"] == true,
                supports_physical: config["supportsPhysicalOrientation"] == true,
                hinge_angle: config["hingeAngle"].as_f64().filter(|a| (0. ..=180.).contains(a)),
                hinge_pose: config["hingePose"].as_str().map(str::to_owned),
            });
            // A new config is the receipt for an orientation command.
            if let Some(id) = state.duo.orientation.take() {
                state.duo.receive(id, true, None, Instant::now());
            }
            true
        }
        Some((&DUO_REPLY, json)) => {
            let Ok(reply) = serde_json::from_slice::<Value>(json) else { return false };
            let (Some(id), Some(ok)) = (reply["requestId"].as_u64(), reply["ok"].as_bool()) else { return false };
            state.duo.receive(id, ok, reply["error"].as_str(), Instant::now());
            true
        }
        _ => false,
    }
}

/// serve-sim streams the raw framebuffer, so a rotated portrait device takes input in that raw space.
fn raw_point(screen: Option<&Screen>, x: f64, y: f64) -> (f64, f64) {
    let Some(screen) = screen.filter(|s| s.width <= s.height) else { return (x, y) };
    match screen.orientation.as_str() {
        "landscape_left" => (y, 1. - x),
        "landscape_right" => (1. - y, x),
        "portrait_upside_down" => (1. - x, 1. - y),
        _ => (x, y),
    }
}

fn next_orientation(current: Option<&str>) -> &'static str {
    let index = ORIENTATIONS.iter().position(|o| Some(*o) == current).unwrap_or(0);
    ORIENTATIONS[(index + 1) % ORIENTATIONS.len()]
}

/// A key's HID usage on a US layout, and whether it needs Shift.
fn usage(c: char) -> Option<(u8, bool)> {
    const SHIFTED_DIGITS: &str = ")!@#$%^&*(";
    const PUNCTUATION: [(char, char, u8); 11] = [
        ('-', '_', 0x2d),
        ('=', '+', 0x2e),
        ('[', '{', 0x2f),
        (']', '}', 0x30),
        ('\\', '|', 0x31),
        (';', ':', 0x33),
        ('\'', '"', 0x34),
        ('`', '~', 0x35),
        (',', '<', 0x36),
        ('.', '>', 0x37),
        ('/', '?', 0x38),
    ];
    let digit = |d: u32| if d == 0 { 0x27 } else { 0x1e + d as u8 - 1 };
    Some(match c {
        'a'..='z' => (0x04 + (c as u8 - b'a'), false),
        'A'..='Z' => (0x04 + (c as u8 - b'A'), true),
        '0'..='9' => (digit(c as u32 - '0' as u32), false),
        ' ' => (0x2c, false),
        '\n' => (0x28, false),
        '\t' => (0x2b, false),
        _ => {
            if let Some(d) = SHIFTED_DIGITS.find(c) {
                (digit(d as u32), true)
            } else {
                let &(plain, _, usage) = PUNCTUATION.iter().find(|(p, s, _)| *p == c || *s == c)?;
                (usage, plain != c)
            }
        }
    })
}

fn presses(key: &Key) -> Vec<(u8, bool)> {
    let named = |usage: u8| vec![(usage, false)];
    match key {
        Key::Text(text) => text.chars().filter_map(usage).collect(),
        Key::Enter => named(0x28),
        Key::Escape => named(0x29),
        Key::Backspace => named(0x2a),
        Key::Tab => named(0x2b),
        Key::Right => named(0x4f),
        Key::Left => named(0x50),
        Key::Down => named(0x51),
        Key::Up => named(0x52),
        _ => vec![],
    }
}

fn press(usage: u8, shift: bool) -> Vec<Vec<u8>> {
    let key = |phase: &str, usage: u8| tagged(KEY, &json!({"type": phase, "usage": usage}));
    let mut out = vec![key("down", usage), key("up", usage)];
    if shift {
        out.insert(0, key("down", SHIFT));
        out.push(key("up", SHIFT));
    }
    out
}

/// Keeps the socket open until `Input` drops, reconnecting after a second when it is lost.
fn run(base: &str, auth: &s5_net::Auth, query: &str, rx: &Receiver<Message>, state: &Mutex<State>, wake: &dyn Fn()) {
    loop {
        if let Ok(mut socket) = s5_net::api_socket(base, auth, PATH, query, Duration::from_millis(15))
            && pump(&mut socket, rx, state, wake)
        {
            return;
        }
        *state.lock().unwrap_or_else(|e| e.into_inner()) = State::default();
        wake();
        if let Err(RecvTimeoutError::Disconnected) = rx.recv_timeout(Duration::from_secs(1)) {
            return;
        }
    }
}

/// True once the panel let go; false when the socket was lost.
fn pump<S: std::io::Read + std::io::Write>(socket: &mut tungstenite::WebSocket<S>, rx: &Receiver<Message>, state: &Mutex<State>, wake: &dyn Fn()) -> bool {
    loop {
        loop {
            match rx.try_recv() {
                // Web's `send` drops input until the simulator admits this client.
                Ok(message) if state.lock().unwrap_or_else(|e| e.into_inner()).ready() => {
                    if socket.send(message).is_err() {
                        return false;
                    }
                }
                Ok(_) => {}
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = socket.close(None);
                    let _ = socket.flush();
                    return true;
                }
            }
        }
        match socket.read() {
            Ok(Message::Binary(bytes)) => {
                let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                let changed = inbound(&bytes, &mut state);
                state.rev += u64::from(changed);
                for message in std::mem::take(&mut state.duo.outbox) {
                    if socket.send(Message::Binary(message)).is_err() {
                        return false;
                    }
                }
                drop(state);
                if changed {
                    wake();
                }
            }
            Ok(Message::Close(_)) => return false,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(message: &[u8]) -> (u8, Value) {
        (message[0], serde_json::from_slice(&message[1..]).unwrap())
    }

    #[test]
    fn serve_sim_admits_input_and_reports_the_screen() {
        let mut state = State::default();
        assert!(!state.ready());
        assert!(!inbound(&[0x90, b'{', b'}'], &mut state), "an unreadable control reply changes nothing");
        assert!(inbound(&[ADMITTED], &mut state));
        assert!(state.ready());
        let mut config = vec![SCREEN_CONFIG];
        config.extend_from_slice(br#"{"width":1179,"height":2556,"orientation":"landscape_left","inputUnavailable":true}"#);
        assert!(inbound(&config, &mut state));
        assert!(!state.ready(), "Simulator input is unavailable");
        assert_eq!(state.screen, Some(Screen { width: 1179., height: 2556., orientation: "landscape_left".into(), ..Default::default() }));
        assert!(!inbound(&[SCREEN_CONFIG, b'x'], &mut state), "unreadable config keeps the last one");
    }

    #[test]
    fn duo_commands_go_one_at_a_time_and_a_lost_one_leaves_the_position_unknown() {
        let now = Instant::now();
        let mut state = State::default();
        let mut config = vec![SCREEN_CONFIG];
        config.extend_from_slice(br#"{"width":2868,"height":1320,"orientation":"landscape_left","screenId":2,"supportsHingeAngle":true,"hingeAngle":90,"hingePose":"book"}"#);
        assert!(inbound(&config, &mut state));
        let screen = state.screen.clone().unwrap();
        assert_eq!((screen.screen_id, screen.supports_hinge, screen.hinge_angle, screen.hinge_pose.as_deref()), (Some(2), true, Some(90.), Some("book")));
        // A stand's orientation goes first; the angle waits behind it, and a newer one replaces it.
        let duo = &mut state.duo;
        duo.enqueue(json!({"control": "orientation", "value": "portrait"}), now);
        duo.enqueue(json!({"control": "angle", "value": 90}), now);
        duo.enqueue(json!({"control": "angle", "value": 180}), now);
        duo.enqueue(json!({"control": "angle", "value": 270}), now);
        assert_eq!(std::mem::take(&mut duo.outbox).iter().map(|m| decoded(m)).collect::<Vec<_>>(), [(ORIENTATION, json!({"orientation": "portrait"}))]);
        assert!(duo.pending() && duo.requested() == Some(&json!({"control": "angle", "value": 180})));
        // The next screen config is the orientation's receipt, which sends the angle.
        assert!(inbound(&config, &mut state));
        assert_eq!(std::mem::take(&mut state.duo.outbox).iter().map(|m| decoded(m)).collect::<Vec<_>>(), [(DUO_COMMAND, json!({"requestId": 2, "command": {"control": "angle", "value": 180}}))]);
        // A stale reply is ignored; a failure clears the queue with serve-sim's reason.
        let reply = |body: &str| [&[DUO_REPLY][..], body.as_bytes()].concat();
        inbound(&reply(r#"{"requestId":1,"ok":false}"#), &mut state);
        assert!(state.duo.pending());
        inbound(&reply(r#"{"requestId":2,"ok":false,"error":"Hinge is busy"}"#), &mut state);
        assert!(!state.duo.pending() && state.duo.error.as_deref() == Some("Hinge is busy"));
        // A command nobody answers times out after five seconds.
        state.duo.enqueue(json!({"control": "pose", "value": "tent"}), now);
        assert!(state.duo.error.is_none() && !state.duo.expire(now + Duration::from_secs(4)));
        assert!(state.duo.expire(now + Duration::from_secs(5)));
        assert_eq!(state.duo.error.as_deref(), Some("Device control timed out. Its position is unknown."));
    }

    #[test]
    fn a_duo_reads_its_fold_from_the_panel_that_shows() {
        let screen = |id: u32, orientation: &str, angle: Option<f64>, pose: Option<&str>| Screen { orientation: orientation.into(), screen_id: Some(id), hinge_angle: angle, hinge_pose: pose.map(str::to_owned), ..Default::default() };
        let fold = |fold, stand, phone_vertical, settled| DuoFold { fold, stand, phone_vertical, settled };
        assert_eq!(duo_fold(&screen(1, "portrait", None, None)), fold("closed", false, true, true), "the cover upright, closed");
        assert_eq!(duo_fold(&screen(2, "landscape_left", Some(90.), Some("book"))), fold("half", false, true, true), "the inner panel's landscape is a vertical book");
        assert_eq!(duo_fold(&screen(2, "portrait", None, Some("laptop"))), fold("open", true, false, true));
        assert_eq!(duo_fold(&screen(1, "portrait", Some(30.), Some("tent"))), fold("half", true, true, false), "a tent resting on the cover is unsettled");
        assert_eq!([duo_hold(true, Some(1)), duo_hold(false, Some(1)), duo_hold(true, Some(2)), duo_hold(false, None)], ["portrait", "landscape_left", "landscape_left", "portrait_upside_down"]);
    }

    #[test]
    fn rotated_portrait_framebuffers_take_input_in_raw_space() {
        let portrait = |orientation: &str| Screen { width: 1179., height: 2556., orientation: orientation.into(), ..Default::default() };
        assert_eq!(raw_point(None, 0.2, 0.3), (0.2, 0.3));
        assert_eq!(raw_point(Some(&portrait("portrait")), 0.2, 0.3), (0.2, 0.3));
        assert_eq!(raw_point(Some(&portrait("landscape_left")), 0.25, 0.5), (0.5, 0.75));
        assert_eq!(raw_point(Some(&portrait("landscape_right")), 0.25, 0.5), (0.5, 0.25));
        assert_eq!(raw_point(Some(&portrait("portrait_upside_down")), 0.25, 0.5), (0.75, 0.5));
        let landscape = Screen { width: 2556., height: 1179., orientation: "landscape_left".into(), ..Default::default() };
        assert_eq!(raw_point(Some(&landscape), 0.25, 0.5), (0.25, 0.5), "a landscape framebuffer is already upright");
        assert_eq!(next_orientation(Some("portrait")), "landscape_left");
        assert_eq!(next_orientation(Some("landscape_right")), "portrait");
        assert_eq!(next_orientation(None), "landscape_left");
    }

    #[test]
    fn keys_become_hid_presses_on_a_us_layout() {
        assert_eq!(usage('a'), Some((0x04, false)));
        assert_eq!(usage('Z'), Some((0x1d, true)));
        assert_eq!(usage('1'), Some((0x1e, false)));
        assert_eq!(usage('0'), Some((0x27, false)));
        assert_eq!(usage('!'), Some((0x1e, true)));
        assert_eq!(usage(')'), Some((0x27, true)));
        assert_eq!(usage('?'), Some((0x38, true)));
        assert_eq!(usage('/'), Some((0x38, false)));
        assert_eq!(usage('"'), Some((0x34, true)));
        assert_eq!(usage('é'), None);
        assert_eq!(presses(&Key::Text("Hi".into())), vec![(0x0b, true), (0x0c, false)]);
        assert_eq!(presses(&Key::Backspace), vec![(0x2a, false)]);
        assert!(presses(&Key::PageUp).is_empty());
        let shifted: Vec<(u8, Value)> = press(0x0b, true).iter().map(|m| decoded(m)).collect();
        assert_eq!(
            shifted,
            vec![
                (KEY, json!({"type": "down", "usage": SHIFT})),
                (KEY, json!({"type": "down", "usage": 0x0b})),
                (KEY, json!({"type": "up", "usage": 0x0b})),
                (KEY, json!({"type": "up", "usage": SHIFT})),
            ]
        );
        assert_eq!(decoded(&tagged(BUTTON, &json!({"button": "home"}))), (0x04, json!({"button": "home"})));
    }

    #[test]
    fn android_input_is_serve_emu_json() {
        let (input, rx) = Input::channel(true);
        input.touch("begin", 0.25, 1.5);
        input.button("recents");
        input.rotate();
        assert!(input.key(&Key::Escape));
        assert!(!input.key(&Key::Function(1)));
        let sent: Vec<Value> = rx.try_iter().map(|m| serde_json::from_str(m.to_text().unwrap()).unwrap()).collect();
        assert_eq!(
            sent,
            vec![
                json!({"type": "touch", "action": "down", "x": 0.25, "y": 1.0}),
                json!({"type": "recents"}),
                json!({"type": "back"}),
            ],
            "Android has no rotate message; touches clamp to the screen"
        );
        assert_eq!(android_key(&Key::Enter), vec![json!({"type": "key", "keycode": 66})]);
        let long = "é".repeat(200);
        let chunks = android_key(&Key::Text(long.clone()));
        assert_eq!(chunks.len(), 2, "400 bytes go in two chunks of at most 300");
        assert!(chunks.iter().all(|c| c["text"].as_str().unwrap().len() <= ANDROID_TEXT_BYTES));
        assert_eq!(chunks.iter().map(|c| c["text"].as_str().unwrap()).collect::<String>(), long);
        let (ios, rx) = Input::channel(false);
        ios.button("back");
        ios.button("power");
        let sent: Vec<Message> = rx.try_iter().collect();
        assert_eq!(sent, vec![Message::Binary(tagged(BUTTON, &json!({"button": "lock"})))], "iOS has no Back");
    }
}
