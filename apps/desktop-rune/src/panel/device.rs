//! DevicePanel over MJPEG: the environment's simulators and emulators, `device.open` for this
//! thread, and the opened iOS simulator's screen from serve-sim's `stream.mjpeg`, driven through
//! serve-sim's input socket. Every other stream is H.264, which this app can't decode.

mod h264;
mod input;
mod inspect;
mod video;

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::{El, Key, Phase, Pixels, TextEdit, Tx, col, el, icon, row, text};
use serde_json::{Value, json};

use crate::theme::{self, AlphaMul, T};
use crate::{App, data, icons, kit};

pub const STATE: &str = "subscribeDeviceState";
/// `FIRST_FRAME_TIMEOUT_MS` in the web's stream client.
const FIRST_FRAME: Duration = Duration::from_secs(15);
const STREAM_FAILED: &str = "Could not receive the device stream. Reconnect to try again.";
const NO_VIDEO: &str = "No video received from the device. Reconnect to try again.";
const WEBRTC: &str = "This device streams over WebRTC, which this app can't show yet.";

#[derive(Default)]
pub struct Ui {
    /// `device.open` in flight: its request, the device's key and whether it was running.
    opening: Vec<(u64, String, bool)>,
    listing: Option<u64>,
    error: Option<String>,
    /// The device this panel shows, once picked; otherwise the thread's newest session.
    selected: Option<String>,
    /// Back at the list with a session still open.
    browsing: bool,
    stream: Option<Stream>,
    /// A recorded `DeviceServiceState` for scenes, which have no connection.
    fixture: Option<Value>,
    menu: Option<Menu>,
    /// `device.close` with shutdown in flight.
    powering_off: Option<(u64, super::Surface, String)>,
    /// The Device tools side panel and its fields.
    tools: Option<Tools>,
    /// The device floating over the chat (web's mini player).
    float: Option<Float>,
    /// Discovery warnings dismissed this session, by host and detail.
    dismissed: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Menu {
    More,
    TextSize,
    Rotate,
}

#[derive(Default)]
struct Tools {
    url: TextEdit,
    app: TextEdit,
    latitude: TextEdit,
    longitude: TextEdit,
    permission_app: TextEdit,
    permission: usize,
    push: TextEdit,
    /// Scenes open the panel scrolled to its end, where the event log is.
    to_end: bool,
}

struct Float {
    key: String,
    /// `[x, y, width]` once moved or resized; the default sits top right.
    frame: Option<[f32; 3]>,
    start: Option<[f32; 4]>,
    area: [f32; 4],
}

struct Stream {
    key: String,
    transport: Transport,
    android: bool,
    /// The device's host kind: `local`, `ssh`, `simlock` or `eas`.
    kind: String,
    pixels: Pixels,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    /// The media ticket request the stream URL waits on.
    ticket: Option<u64>,
    started: Instant,
    seen: u64,
    input: Option<input::Input>,
    /// `device.detail`, then each `device.action`, in flight; the newest `DeviceDetail`.
    detail: Option<u64>,
    info: Option<Value>,
    /// Web's "Overlay element frames": the screen's accessibility elements while on.
    ax: Option<inspect::Feed<Option<Vec<inspect::AxElement>>>>,
    /// The iOS event log, streaming while its section is open.
    log: Option<inspect::Feed<Vec<inspect::LogEntry>>>,
    feeds_seen: u64,
}

#[derive(Default)]
struct Shared {
    frames: u64,
    failed: Option<String>,
    capturing: bool,
    capture_failed: Option<String>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn state(app: &App) -> Option<&Value> {
    app.ui.panel.device.fixture.as_ref().or_else(|| app.src.net_ref()?.resource(STATE, json!({}))).filter(|v| v.get("error").is_none())
}

/// Every host: the wire splits EAS and Simlock hosts out of `hosts`.
fn hosts(state: &Value) -> impl Iterator<Item = &Value> {
    ["hosts", "easHosts", "simlockHosts"].into_iter().flat_map(move |key| state[key].as_array().into_iter().flatten())
}

fn host<'a>(state: &'a Value, id: &str) -> Option<&'a Value> {
    hosts(state).find(|h| h["id"] == id)
}

fn key(host: &str, device: &str) -> String {
    format!("{host}\u{0}{device}")
}

/// The session this panel shows for `thread`: the picked device's, else the newest.
fn session<'a>(state: &'a Value, thread: &str, selected: Option<&str>) -> Option<&'a Value> {
    let mut sessions: Vec<&Value> = state["sessions"].as_array().into_iter().flatten().filter(|s| s["threadId"] == thread).collect();
    sessions.sort_by_key(|s| std::cmp::Reverse(data::s(s, "openedAt").to_owned()));
    let picked = selected.and_then(|k| sessions.iter().copied().find(|s| key(data::s(s, "hostId"), data::s(s, "deviceId")) == k));
    if selected.is_some() { picked } else { sessions.first().copied() }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Transport {
    /// serve-sim's H.264 `stream.avcc`, falling back to its MJPEG where it serves one.
    Avcc { mjpeg: bool },
    /// serve-emu's socket of H.264 packets, which also takes the emulator's input.
    Semu,
    WebRtc,
}

fn host_kind<'a>(state: &'a Value, session: &Value) -> &'a str {
    host(state, data::s(session, "hostId")).map_or("local", |h| data::s(h, "kind"))
}

/// Web's choice: Simlock streams AVCC only and EAS only WebRTC; on local and SSH hosts iOS
/// simulators stream AVCC (MJPEG as the fallback) and Android emulators through serve-emu.
fn transport(state: &Value, session: &Value) -> Transport {
    match host_kind(state, session) {
        "eas" => Transport::WebRtc,
        "simlock" => Transport::Avcc { mjpeg: false },
        _ if session["platform"] == "android" => Transport::Semu,
        _ => Transport::Avcc { mjpeg: true },
    }
}

fn encode(value: &str) -> String {
    value.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// serve-sim's `stream.<ext>` behind the server's device-hub proxy, with the media ticket and host.
fn helper_url(base: &str, device: &str, host: &str, ticket: &str, ext: &str) -> String {
    format!(
        "{}/api/device-hub/vendor/serve-sim/helper/{}/stream.{ext}?wsTicket={}&hostId={}",
        base.trim_end_matches('/'),
        encode(device),
        encode(ticket),
        encode(host)
    )
}

/// The boundary of a `multipart/x-mixed-replace` content type.
fn boundary(content_type: &str) -> Option<String> {
    let value = content_type.split(';').find_map(|p| p.trim().strip_prefix("boundary="))?;
    let value = value.trim_matches('"');
    (!value.is_empty()).then(|| value.to_owned())
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

/// Takes the first whole part out of `buf`, by its `Content-Length` or else the next boundary.
fn take_part(buf: &mut Vec<u8>, boundary: &str) -> Option<Vec<u8>> {
    let marker = format!("--{boundary}");
    let start = find(buf, marker.as_bytes(), 0)?;
    let headers_at = start + marker.len();
    let head_end = find(buf, b"\r\n\r\n", headers_at)?;
    let headers = String::from_utf8_lossy(&buf[headers_at..head_end]);
    let length = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim().eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok()).flatten()
    });
    let body_start = head_end + 4;
    let body_end = match length {
        Some(n) if buf.len() >= body_start + n => body_start + n,
        Some(_) => return None,
        None => find(buf, format!("\r\n{marker}").as_bytes(), body_start)?,
    };
    let body = buf[body_start..body_end].to_vec();
    buf.drain(..body_end);
    Some(body)
}

/// Reads the stream until it ends or the panel lets go, painting only the newest whole frame.
fn read_stream(url: &str, pixels: &Pixels, shared: &Mutex<Shared>, stop: &AtomicBool, wake: &dyn Fn()) -> Result<(), &'static str> {
    let response = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).build().get(url).call().map_err(|_| STREAM_FAILED)?;
    let boundary = boundary(response.header("content-type").unwrap_or_default()).ok_or(STREAM_FAILED)?;
    let mut reader = response.into_reader();
    let (mut buf, mut chunk) = (Vec::new(), vec![0u8; 64 * 1024]);
    while !stop.load(Ordering::Relaxed) {
        let n = reader.read(&mut chunk).map_err(|_| STREAM_FAILED)?;
        if n == 0 {
            return Err(STREAM_FAILED);
        }
        buf.extend_from_slice(&chunk[..n]);
        let mut newest = None;
        while let Some(part) = take_part(&mut buf, &boundary) {
            newest = Some(part);
        }
        if buf.len() > 32 * 1024 * 1024 {
            return Err(STREAM_FAILED);
        }
        if let Some(jpeg) = newest
            && let Ok(image) = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)
        {
            let rgba = image.to_rgba8();
            pixels.set(rgba.width(), rgba.height(), rgba.into_raw());
            shared.lock().unwrap_or_else(|e| e.into_inner()).frames += 1;
            wake();
        }
    }
    Ok(())
}

fn request(app: &mut App, method: &str, payload: Value) -> Option<u64> {
    app.src.net().map(|n| n.request(method, payload))
}

fn open(app: &mut App, device: &Value) {
    let Some((thread, _)) = super::context(app) else { return };
    let (host, id) = (data::s(device, "hostId").to_owned(), data::s(device, "id").to_owned());
    let payload = json!({"threadId": thread, "hostId": host, "deviceId": id, "platform": device["platform"]});
    let target = super::tabs::Target::Device {
        host: host.clone(), device: id.clone(), name: data::s(device, "name").to_owned(), title: None,
    };
    let device_key = key(&host, &id);
    let before = app.ui.panel.surfaces.len();
    let tab_id = super::tabs::open(app, target);
    let surfaces = &mut app.ui.panel.surfaces;
    let appended = surfaces.len() > before;
    if let Some(index) = surfaces.iter().position(|&surface| surface == super::Surface::Device) {
        let tab = super::Surface::Tab(tab_id);
        surfaces.remove(index);
        if appended && surfaces.last() == Some(&tab) {
            surfaces.pop();
            surfaces.insert(index, tab);
        }
    }
    if app.ui.panel.device.opening.iter().any(|(_, pending, _)| pending == &device_key) {
        return;
    }
    let booted = device["booted"] == true;
    if let Some(request) = request(app, "device.open", payload) {
        app.ui.panel.device.opening.push((request, device_key, booted));
    }
}

pub(super) fn browse(app: &mut App) {
    let ui = &mut app.ui.panel.device;
    ui.selected = None;
    ui.browsing = true;
    ui.error = None;
}

pub(super) fn select_tab(app: &mut App, host: &str, device: &str) {
    let ui = &mut app.ui.panel.device;
    let selected = key(host, device);
    if ui.selected.as_ref() != Some(&selected) {
        ui.error = None;
        ui.tools = None;
    }
    ui.selected = Some(selected);
    ui.browsing = false;
}


fn refresh(app: &mut App) {
    app.ui.panel.device.error = None;
    app.ui.panel.device.listing = request(app, "device.list", json!({}));
}

/// Keeps one stream for the shown session while the Device surface is on screen.
fn sync_stream(app: &mut App, tx: &mut Tx) {
    let in_panel = app.ui.shell.panel_open && app.ui.panel.active.is_some_and(|surface| super::tabs::is_device(app, surface)) && !app.ui.panel.device.browsing;
    let floating = app.ui.panel.device.float.as_ref().map(|f| f.key.clone());
    let wanted = (in_panel || floating.is_some())
        .then(|| {
            let (thread, _) = super::context(app)?;
            let state = state(app)?;
            let pick = if in_panel { app.ui.panel.device.selected.as_deref() } else { floating.as_deref() };
            let session = session(state, &thread, pick)?;
            let transport = transport(state, session);
            (transport != Transport::WebRtc).then(|| {
                let (host, device) = (data::s(session, "hostId").to_owned(), data::s(session, "deviceId").to_owned());
                (host, device, transport, session["platform"] == "android", host_kind(state, session).to_owned())
            })
        })
        .flatten();
    let ui = &mut app.ui.panel.device;
    if ui.stream.as_ref().map(|s| s.key.clone()) == wanted.as_ref().map(|(h, d, ..)| key(h, d)) {
        return;
    }
    ui.stream = None;
    ui.menu = None;
    let Some((host, device, transport, android, kind)) = wanted else { return };
    if app.ui.panel.device.fixture.is_some() {
        app.ui.panel.device.stream = Some(fixture_stream(key(&host, &device), transport, android, kind));
        return;
    }
    let mut stream = Stream {
        key: key(&host, &device),
        transport,
        android,
        kind,
        pixels: Pixels::new(),
        shared: Arc::default(),
        stop: Arc::default(),
        ticket: None,
        started: Instant::now(),
        seen: 0,
        input: None,
        detail: None,
        info: None,
        ax: None,
        log: None,
        feeds_seen: 0,
    };
    let wake = app.ui.panel.waker.clone();
    let wake = move || {
        if let Some(wake) = &wake {
            wake();
        }
    };
    let net = app.src.net_ref().map(|n| (n.base_url().to_owned(), n.bearer().to_owned()));
    match (transport, net) {
        (Transport::Semu, Some((base, bearer))) => {
            let (input, rx) = input::Input::channel(true);
            let query = format!("device={}&frame-meta=1&hostId={}", encode(&device), encode(&host));
            video::spawn_semu(base, bearer, query, rx, input.state.clone(), stream.pixels.clone(), stream.shared.clone(), stream.stop.clone(), wake);
            stream.input = Some(input);
        }
        (_, Some((base, bearer))) => {
            stream.input = Some(input::Input::open(base, bearer, &host, &device, wake));
            stream.ticket = app.src.net().map(|n| n.http_request("POST", "/api/auth/websocket-ticket/media", json!({})));
        }
        (_, None) => {}
    }
    if stream.input.is_none() || (transport != Transport::Semu && stream.ticket.is_none()) {
        stream.shared.lock().unwrap_or_else(|e| e.into_inner()).failed = Some(STREAM_FAILED.to_owned());
    }
    if runs_host_commands(&stream.kind) {
        stream.detail = request(app, "device.detail", json!({"hostId": host, "deviceId": device}));
    }
    app.ui.panel.device.stream = Some(stream);
    tx.wake_after(FIRST_FRAME);
}

/// A stream with one plain frame for scenes: the panel's chrome without a simulator.
fn fixture_stream(key: String, transport: Transport, android: bool, kind: String) -> Stream {
    let shared = Shared { frames: 1, ..Default::default() };
    let foreground = if android { json!({"id": "com.example.app", "name": "Example"}) } else { json!({"id": "com.example.app", "name": "Example", "version": "1.0"}) };
    Stream {
        key,
        transport,
        android,
        kind,
        pixels: fixture_screen(),
        shared: Arc::new(Mutex::new(shared)),
        stop: Arc::default(),
        ticket: None,
        started: Instant::now(),
        seen: 0,
        input: None,
        detail: None,
        info: Some(json!({"settings": {"appearance": "light", "textSize": "default", "reduceMotion": false, "increaseContrast": true, "networkEnabled": true, "liquidGlass": "clear", "colorFilter": "none"}, "foregroundApp": foreground})),
        ax: None,
        log: None,
        feeds_seen: 0,
    }
}

fn fixture_screen() -> Pixels {
    let (w, h) = (393u32, 852u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for _ in 0..w {
            let k = y as f32 / h as f32;
            rgba.extend_from_slice(&[(40. + 60. * k) as u8, (90. + 40. * k) as u8, (200. - 60. * k) as u8, 255]);
        }
    }
    let pixels = Pixels::new();
    pixels.set(w, h, rgba);
    pixels
}

/// What this device's buttons send, once serve-sim admits input.
fn input(app: &App) -> Option<&input::Input> {
    app.ui.panel.device.stream.as_ref()?.input.as_ref()
}

fn input_ready(app: &App) -> bool {
    app.ui.panel.device.fixture.is_some() || input(app).is_some_and(|i| i.state.lock().unwrap_or_else(|e| e.into_inner()).ready())
}

/// `device.action` on the shown device; its reply is the device's new `DeviceDetail`.
fn act(app: &mut App, action: Value) {
    let Some((host, device)) = app.ui.panel.device.stream.as_ref().and_then(|s| s.key.split_once('\u{0}')).map(|(h, d)| (h.to_owned(), d.to_owned())) else { return };
    let mut payload = action;
    payload["hostId"] = json!(host);
    payload["deviceId"] = json!(device);
    let id = request(app, "device.action", payload);
    if let Some(stream) = app.ui.panel.device.stream.as_mut() {
        stream.detail = id;
    }
}

/// serve-sim's PNG screenshot, saved to Downloads as web's download.
fn screenshot(app: &mut App, name: &str) {
    let Some(net) = app.src.net_ref() else { return };
    let (base, bearer) = (net.base_url().to_owned(), net.bearer().to_owned());
    let wake = app.ui.panel.waker.clone();
    let Some(stream) = app.ui.panel.device.stream.as_ref() else { return };
    let Some((host, device)) = stream.key.split_once('\u{0}') else { return };
    let mut shared = stream.shared.lock().unwrap_or_else(|e| e.into_inner());
    if shared.capturing {
        return;
    }
    shared.capturing = true;
    drop(shared);
    let query = format!("device={}&hostId={}", encode(device), encode(host));
    let file: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect();
    let file = format!("{file}-{}.png", data::now_ms());
    let shared = stream.shared.clone();
    let path = format!("/api/device-hub/vendor/{}/api/screenshot", if stream.transport == Transport::Semu { "serve-emu" } else { "serve-sim" });
    std::thread::spawn(move || {
        let saved = s5_net::api_post_read(&base, &bearer, &path, &query, 64 << 20)
            .map_err(|e| format!("Screenshot capture failed ({e}). Try again."))
            .and_then(|png| if png.is_empty() { Err("The device returned an empty screenshot. Try again.".to_owned()) } else { Ok(png) })
            .and_then(|png| {
                let dir = super::browser::downloads_dir();
                let _ = std::fs::create_dir_all(&dir);
                let path = super::browser::unique_path(&dir, &file).ok_or("Could not save the screenshot.")?;
                std::fs::write(path, png).map_err(|e| e.to_string())
            });
        let mut shared = shared.lock().unwrap_or_else(|e| e.into_inner());
        shared.capturing = false;
        shared.capture_failed = saved.err();
        drop(shared);
        if let Some(wake) = &wake {
            wake();
        }
    });
}

/// Web's "Power off device": closes the session with shutdown, then the surface.
fn power_off(app: &mut App) {
    let Some((thread, _)) = super::context(app) else { return };
    let Some((host, device)) = app.ui.panel.device.stream.as_ref().and_then(|s| s.key.split_once('\u{0}')).map(|(h, d)| (h.to_owned(), d.to_owned())) else { return };
    app.ui.panel.device.menu = None;
    if let Some(surface) = app.ui.panel.active {
        app.ui.panel.device.powering_off = request(app, "device.close", json!({"threadId": thread, "hostId": host, "deviceId": device, "shutdown": true}))
            .map(|request| (request, surface, thread));
    }
}

/// The hub connection a readout thread uses: base URL, bearer, the device's host and id, and the wake.
fn hub(app: &App) -> Option<(String, String, String, String, impl Fn() + Send + 'static)> {
    let net = app.src.net_ref()?;
    let (host, device) = app.ui.panel.device.stream.as_ref()?.key.split_once('\u{0}')?;
    let wake = app.ui.panel.waker.clone();
    Some((net.base_url().to_owned(), net.bearer().to_owned(), host.to_owned(), device.to_owned(), move || {
        if let Some(wake) = &wake {
            wake();
        }
    }))
}

/// Web's `fetchDeviceAxTree` endpoints: serve-sim's helper on iOS, serve-emu on Android.
fn ax_endpoint(android: bool, host: &str, device: &str) -> (String, String) {
    if android {
        ("/api/device-hub/vendor/serve-emu/api/accessibility".into(), format!("device={}&hostId={}", encode(device), encode(host)))
    } else {
        (format!("/api/device-hub/vendor/serve-sim/helper/{}/ax", encode(device)), format!("hostId={}", encode(host)))
    }
}

fn toggle_ax(app: &mut App) {
    let Some(stream) = app.ui.panel.device.stream.as_mut() else { return };
    if stream.ax.take().is_some() {
        return;
    }
    let android = stream.android;
    let Some((base, bearer, host, device, wake)) = hub(app) else { return };
    let (path, query) = ax_endpoint(android, &host, &device);
    if let Some(stream) = app.ui.panel.device.stream.as_mut() {
        stream.ax = Some(inspect::spawn_ax(base, bearer, path, query, android, wake));
    }
}

fn toggle_log(app: &mut App) {
    let Some(stream) = app.ui.panel.device.stream.as_mut() else { return };
    if stream.log.take().is_some() {
        return;
    }
    let Some((base, bearer, host, device, wake)) = hub(app) else { return };
    let query = format!("device={}&limit={}&hostId={}", encode(&device), inspect::LOG_LIMIT, encode(&host));
    if let Some(stream) = app.ui.panel.device.stream.as_mut() {
        stream.log = Some(inspect::spawn_log(base, bearer, "/api/device-hub/vendor/serve-sim/api/event-log/events".into(), query, wake));
    }
}

/// Web `DevicePanel`'s host diagnostics: the detail a ready host reports about discovery.
fn host_diagnostics<'a>(state: &'a Value, host_id: &str) -> Option<&'a str> {
    let host = host(state, host_id)?;
    let status = &state["hostStatuses"][host_id];
    let local = (data::s(host, "kind") == "local").then(|| state["hostStatusDetail"].as_str()).flatten();
    let detail = status["detail"].as_str().or(local)?;
    (status["status"].as_str().or(state["hostStatus"].as_str()) == Some("ready")).then_some(detail)
}

pub fn poll(app: &mut App, tx: &mut Tx) -> bool {
    let mut changed = false;
    for (id, key, booted) in std::mem::take(&mut app.ui.panel.device.opening) {
        match app.src.net().and_then(|n| n.take_reply(id)) {
            Some(reply) => {
                if let Err(e) = reply {
                    let error = crate::shell::palette::error_message(&e, "Could not open the device.");
                    if app.ui.panel.device.selected.as_ref() == Some(&key) {
                        app.ui.panel.device.error = Some(error);
                    } else {
                        crate::shell::palette::toast_error_brief(app, "Could not open the device.", Some(&error));
                    }
                }
                changed = true;
            }
            None => app.ui.panel.device.opening.push((id, key, booted)),
        }
    }
    if let Some(id) = app.ui.panel.device.listing
        && let Some(reply) = app.src.net().and_then(|n| n.take_reply(id))
    {
        app.ui.panel.device.listing = None;
        if let Err(e) = reply {
            app.ui.panel.device.error = Some(crate::shell::palette::error_message(&e, "Could not refresh devices."));
        }
        changed = true;
    }
    if let Some((id, surface, thread)) = app.ui.panel.device.powering_off.clone()
        && let Some(reply) = app.src.net().and_then(|n| n.take_reply(id))
    {
        app.ui.panel.device.powering_off = None;
        match reply {
            Ok(_) if super::context(app).is_some_and(|(current, _)| current == thread) => super::close(app, surface),
            Ok(_) => {},
            Err(e) => app.ui.panel.device.error = Some(crate::shell::palette::error_message(&e, "Could not power off the device.")),
        }
        changed = true;
    }
    if let Some(id) = app.ui.panel.device.stream.as_ref().and_then(|s| s.detail)
        && let Some(reply) = app.src.net().and_then(|n| n.take_reply(id))
    {
        let stream = app.ui.panel.device.stream.as_mut().expect("the stream holds the request");
        stream.detail = None;
        match reply {
            Ok(detail) => stream.info = Some(detail),
            Err(e) => app.ui.panel.device.error = Some(crate::shell::palette::error_message(&e, "Could not reach the device.")),
        }
        changed = true;
    }
    if let Some(stream) = app.ui.panel.device.stream.as_ref()
        && let Some(failed) = stream.shared.lock().unwrap_or_else(|e| e.into_inner()).capture_failed.take()
    {
        app.ui.panel.device.error = Some(failed);
        changed = true;
    }
    sync_stream(app, tx);
    let base = app.src.net_ref().map(|n| n.base_url().to_owned()).unwrap_or_default();
    let wake = app.ui.panel.waker.clone();
    let Some(stream) = app.ui.panel.device.stream.as_mut() else { return changed };
    if let Some(id) = stream.ticket
        && let Some(reply) = app.src.net().and_then(|n| n.take_reply(id))
    {
        stream.ticket = None;
        let ticket = reply.ok().and_then(|v| v["ticket"].as_str().map(str::to_owned));
        match (ticket, stream.key.split_once('\u{0}')) {
            (Some(ticket), Some((host, device))) => {
                let (avcc, mjpeg) = (helper_url(&base, device, host, &ticket, "avcc"), helper_url(&base, device, host, &ticket, "mjpeg"));
                let fallback = stream.transport == Transport::Avcc { mjpeg: true };
                let (pixels, shared, stop) = (stream.pixels.clone(), stream.shared.clone(), stream.stop.clone());
                std::thread::spawn(move || {
                    let wake = move || {
                        if let Some(wake) = &wake {
                            wake();
                        }
                    };
                    if fallback {
                        video::prime(&mjpeg);
                    }
                    let failed = match video::read_avcc(&avcc, &pixels, &shared, &stop, &wake) {
                        Ok(()) => None,
                        Err(video::Failure::Decoder(_)) if fallback => read_stream(&mjpeg, &pixels, &shared, &stop, &wake).err().map(str::to_owned),
                        Err(video::Failure::Decoder(e)) => Some(e),
                        Err(video::Failure::WebRtc) => Some(WEBRTC.to_owned()),
                        Err(video::Failure::Stream) => Some(STREAM_FAILED.to_owned()),
                    };
                    if let Some(failed) = failed
                        && !stop.load(Ordering::Relaxed)
                    {
                        shared.lock().unwrap_or_else(|e| e.into_inner()).failed = Some(failed);
                        wake();
                    }
                });
            }
            _ => stream.shared.lock().unwrap_or_else(|e| e.into_inner()).failed = Some(STREAM_FAILED.into()),
        }
        changed = true;
    }
    let mut shared = stream.shared.lock().unwrap_or_else(|e| e.into_inner());
    if shared.frames == 0 && shared.failed.is_none() && stream.started.elapsed() >= FIRST_FRAME {
        shared.failed = Some(NO_VIDEO.into());
    }
    let seen = shared.frames * 4 + u64::from(shared.failed.is_some()) * 2 + u64::from(shared.capturing);
    drop(shared);
    let feeds = stream.ax.as_ref().map_or(0, |f| f.rev()) + stream.log.as_ref().map_or(0, |f| f.rev());
    if feeds != stream.feeds_seen {
        stream.feeds_seen = feeds;
        changed = true;
    }
    if seen != stream.seen {
        stream.seen = seen;
        changed = true;
    }
    changed
}

// Views.

fn centered(lines: Vec<El>) -> El {
    col().grow(1.).min_h(0.).center().gap(8.).padding(24.).children(lines)
}

fn note(label: impl Into<String>, t: &T) -> El {
    text(label.into()).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap().align(rune::Align::Center)
}

fn spinner(t: &T) -> El {
    kit::Spinner::new().size("lg").tone("muted").build(&kit::theme(t))
}

/// `DeviceStatusPill`-style subtitle: host, OS version, running or stopped.
fn subtitle(state: &Value, device: &Value) -> String {
    let host = host(state, data::s(device, "hostId")).map_or("", |h| data::s(h, "label"));
    let running = if device["booted"] == true { "Running" } else { "Stopped" };
    [host, data::s(device, "version"), running].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// iOS then Android, running devices first, then by name.
fn grouped(state: &Value) -> Vec<(&'static str, Vec<&Value>)> {
    let devices: Vec<&Value> = state["devices"].as_array().into_iter().flatten().collect();
    [("ios", "iOS Simulators"), ("android", "Android Emulators")]
        .into_iter()
        .filter_map(|(platform, title)| {
            let mut list: Vec<&Value> = devices.iter().copied().filter(|d| d["platform"] == platform).collect();
            list.sort_by_key(|d| (d["booted"] != true, data::s(d, "name").to_lowercase()));
            (!list.is_empty()).then_some((title, list))
        })
        .collect()
}

fn header(title: El, actions: Vec<El>) -> El {
    row().h(40.).px(12.).gap(4.).center_y().child(title.grow(1.).shrink().min_w(0.)).children(actions)
}

fn icon_button(label: &'static str, glyph: &'static rune::Icon) -> kit::Button {
    kit::Button::new(label).icon(glyph).variant("ghost-muted").size("icon-xs")
}

fn list(app: &App, state: &Value, t: &T) -> El {
    let refresh_button = icon_button("Refresh devices", &icons::REFRESH_CW).on_press(refresh).build(t).help("Refresh devices");
    let mut actions = vec![refresh_button];
    if let Some((thread, _)) = super::context(app)
        && session(state, &thread, None).is_some()
    {
        actions.insert(0, icon_button("Back to device", &icons::ARROW_RIGHT).on_press(|s: &mut App| s.ui.panel.device.browsing = false).build(t).help("Back to device"));
    }
    let mut body = col().w_full().py(4.);
    for (title, devices) in grouped(state) {
        body = body.child(row().px(12.).pt(12.).pb(4.).child(text(title).font(theme::XS_MEDIUM).color(t.c.muted_foreground)));
        for device in devices {
            let name = data::s(device, "name").to_owned();
            let action = if device["booted"] == true { "Open" } else { "Start" };
            let picked = device.clone();
            body = body.child(
                row()
                    .key(format!("device-{}-{}", data::s(device, "hostId"), data::s(device, "id")))
                    .w_full()
                    .px(12.)
                    .py(6.)
                    .gap(10.)
                    .center_y()
                    .child(icon(&icons::SMARTPHONE, 16.).size(16., 16.).color(t.c.muted_foreground))
                    .child(
                        col()
                            .grow(1.)
                            .shrink()
                            .min_w(0.)
                            .child(text(name.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).single_line().truncate())
                            .child(text(subtitle(state, device)).font(theme::TEXT_XS).color(t.c.muted_foreground).single_line().truncate()),
                    )
                    .child(kit::Button::new(action).variant("outline").size("xs").on_press(move |s: &mut App| open(s, &picked)).build(t).label(format!("Open {name}"))),
            );
        }
    }
    if state["devices"].as_array().is_some_and(|d| !d.is_empty()) && !state["devices"].as_array().into_iter().flatten().any(|d| d["platform"] == "android") {
        body = body.child(
            row().px(12.).pt(12.).child(
                text("No Android virtual devices found. Create one in Android Studio's Device Manager, then refresh.").font(theme::TEXT_XS).color(t.c.muted_foreground).wrap(),
            ),
        );
    }
    col()
        .grow(1.)
        .min_h(0.)
        .child(header(text("Devices").font(theme::SM_MEDIUM).color(t.c.foreground), actions))
        .child(el().h(1.).w_full().fill(t.c.border))
        .child(col().grow(1.).min_h(0.).scroll(rune::ScrollSpec { stick_bottom: false }).child(body))
}

/// `size` scaled to fit inside `area`.
fn fit(area: (f32, f32), size: (f32, f32)) -> (f32, f32) {
    let k = (area.0 / size.0).min(area.1 / size.1);
    (size.0 * k, size.1 * k)
}

/// The screen as an element of `w` × `h` points, taking taps, swipes and typing.
fn screen_el(stream: &Stream, (w, h): (f32, f32), key: &str) -> El {
    el().size(w, h)
        .pixels(&stream.pixels)
        .key(key.to_owned())
        .focusable(true)
        .on_drag(move |s: &mut App, d, _| {
            let phase = match d.phase {
                _ if d.cancelled() => "end",
                Phase::Down => "begin",
                Phase::Up => "end",
                _ => "move",
            };
            if let Some(input) = input(s) {
                input.touch(phase, f64::from(d.x / w), f64::from(d.y / h));
            }
        })
        .on_key(|s: &mut App, e, tx| {
            // Web's way out of a surface that takes every key.
            if matches!(e.key, Key::Escape) && e.mods.shift {
                tx.blur();
                return true;
            }
            !e.mods.cmd && !e.mods.ctrl && input(s).is_some_and(|i| i.key(&e.key))
        })
}

/// Web's accessibility overlay: each element's frame over the screen, labeled above.
fn ax_overlay(elements: &[inspect::AxElement], (w, h): (f32, f32), t: &T) -> Vec<El> {
    elements
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let frame = el().key(format!("device-ax-{i}-{}", e.id)).pin(Some(e.x * w), Some(e.y * h), None, None).size(e.width * w, e.height * h).border(1., t.c.info.alpha_mul(0.8)).fill(t.c.info.alpha_mul(0.1));
            if e.label.is_empty() {
                return frame;
            }
            let label = text(e.label.clone()).font(theme::TEXT_3XS).color(rune::Color::WHITE).single_line().truncate().shrink();
            frame.child(row().pin(Some(0.), Some(-14.), None, None).h(14.).max_w((e.width * w).max(24.)).px(4.).radius(2.).center_y().fill(t.c.info).child(label))
        })
        .collect()
}

/// The screen at its fitted size once the stage is laid out.
fn screen(stream: &Stream, stage: Option<(f32, f32)>, name: &str, t: &T) -> El {
    let (iw, ih) = stream.pixels.size();
    let fitted = stage.filter(|_| iw > 0 && ih > 0).map(|(w, h)| fit((w - 32., h - 32.), (iw as f32, ih as f32))).filter(|(w, h)| *w >= 1. && *h >= 1.);
    let elements = stream.ax.as_ref().and_then(|f| f.data.lock().unwrap_or_else(|e| e.into_inner()).clone()).unwrap_or_default();
    let screen = match fitted {
        Some(size) => screen_el(stream, size, "device-screen").children(ax_overlay(&elements, size, t)),
        None => el().w_full().grow(1.).min_h(0.).pixels_fit(&stream.pixels, rune::SurfaceFit::Fit),
    };
    col().key("device-stage").grow(1.).min_h(0.).min_w(0.).padding(16.).center().role("img").label(format!("{name} screen")).child(screen)
}

fn rail_button(label: impl Into<String>, glyph: &'static rune::Icon, disabled: bool, t: &T, f: impl Fn(&mut App) + 'static) -> El {
    let label = label.into();
    kit::Button::new(label.clone()).icon(glyph).variant("ghost").size("icon-sm").disabled(disabled).on_press(f).build(t).help(label)
}

fn press(button: &'static str) -> impl Fn(&mut App) + 'static {
    move |s: &mut App| {
        if let Some(input) = input(s) {
            input.button(button);
        }
    }
}

/// Web's `runsHostCommands`: settings and tools run on the device's own machine, which EAS and
/// Simlock don't expose.
fn runs_host_commands(kind: &str) -> bool {
    matches!(kind, "local" | "ssh")
}

/// A rail menu: `trigger` opens `which`, holding `items`.
fn rail_menu(app: &App, which: Menu, trigger: kit::Button, items: Vec<kit::MenuItem>, t: &T, overlays: &mut Vec<El>) -> El {
    let label = trigger_label(which);
    let trigger = trigger
        .variant("ghost")
        .size("icon-sm")
        .on_press(move |s: &mut App| {
            let menu = &mut s.ui.panel.device.menu;
            *menu = if *menu == Some(which) { None } else { Some(which) };
        })
        .build(t)
        .help(label);
    let mut menu = kit::Menu::new(format!("device-menu-{which:?}"))
        .trigger(trigger)
        .placement(kit::Placement::BottomEnd)
        .width(198.)
        .native(true)
        .open(app.ui.panel.device.menu == Some(which))
        .on_close(|s: &mut App| s.ui.panel.device.menu = None);
    for item in items {
        menu = menu.item(item);
    }
    menu.build(&kit::theme(t), overlays)
}

fn trigger_label(which: Menu) -> &'static str {
    match which {
        Menu::More => "More device actions",
        Menu::TextSize => "Device text size",
        Menu::Rotate => "Rotate device",
    }
}

fn settings(stream: Option<&Stream>) -> Option<&Value> {
    stream.and_then(|s| s.info.as_ref()).map(|v| &v["settings"])
}

/// `device.action` from a menu item, closing the menu.
fn act_item(label: &str, action: Value) -> kit::MenuItem {
    kit::MenuItem::new(label).on_select(move |s: &mut App| {
        s.ui.panel.device.menu = None;
        act(s, action.clone());
    })
}

/// `DeviceControlsRail`: the shown device's buttons, on the stream's right edge.
fn rail(app: &App, stream: Option<&Stream>, name: &str, t: &T, overlays: &mut Vec<El>) -> El {
    let ready = input_ready(app);
    let (streaming, capturing) = stream.map_or((false, false), |s| {
        let shared = s.shared.lock().unwrap_or_else(|e| e.into_inner());
        (shared.frames > 0 && shared.failed.is_none(), shared.capturing)
    });
    let android = stream.is_some_and(|s| s.android);
    let kind = stream.map_or("local", |s| s.kind.as_str());
    let settings = settings(stream);
    let appearance = settings.and_then(|v| v["appearance"].as_str());
    let text_size = settings.and_then(|v| v["textSize"].as_str());
    let next = if appearance == Some("dark") { "light" } else { "dark" };
    let busy = stream.is_none_or(|s| s.detail.is_some());
    let mut pill = col().gap(4.).padding(8.).radius(999.).items_center().fill(t.c.background.alpha_mul(0.8)).border(1., t.c.border.alpha_mul(0.5)).child(rail_button("Home", &icons::HOUSE, !ready, t, press("home")));
    if android {
        pill = pill.child(rail_button("Back", &icons::CHEVRON_LEFT, !ready, t, press("back"))).child(rail_button("Recents", &icons::SQUARE, !ready, t, press("recents")));
        if runs_host_commands(kind) {
            let items = vec![act_item("Portrait", json!({"type": "setOrientation", "value": "portrait"})), act_item("Landscape", json!({"type": "setOrientation", "value": "landscape_left"}))];
            pill = pill.child(rail_menu(app, Menu::Rotate, kit::Button::new("Rotate device").icon(&icons::ROTATE_CCW).disabled(busy), items, t, overlays));
        }
    } else if kind != "simlock" {
        pill = pill.child(rail_button("Rotate device", &icons::ROTATE_CCW, !ready, t, |s| {
            if let Some(input) = input(s) {
                input.rotate();
            }
        }));
    }
    if runs_host_commands(kind) {
        pill = pill.child(el().w(20.).h(1.).fill(t.c.border)).child(rail_button(format!("Switch device to {next} mode"), if appearance == Some("dark") { &icons::SUN } else { &icons::MOON }, busy || appearance.is_none(), t, move |s| {
            act(s, json!({"type": "setAppearance", "value": next}));
        }));
        let sizes = [("small", "Small"), ("default", "Default"), ("large", "Large"), ("extra-large", "Extra large")]
            .into_iter()
            .map(|(value, label)| act_item(label, json!({"type": "setTextSize", "value": value})).radio(true).checked(text_size == Some(value)))
            .collect();
        pill = pill.child(rail_menu(app, Menu::TextSize, kit::Button::new("Device text size").icon(&icons::TYPE).disabled(busy || text_size.is_none()), sizes, t, overlays));
        let open = app.ui.panel.device.tools.is_some();
        pill = pill.child(
            kit::Button::new("Device tools")
                .icon(&icons::SLIDERS_HORIZONTAL)
                .variant(if open { "secondary" } else { "ghost" })
                .size("icon-sm")
                .on_press(|s: &mut App| s.ui.panel.device.tools = if s.ui.panel.device.tools.is_some() { None } else { Some(Tools::default()) })
                .build(t)
                .help("Device tools")
                .checked(open),
        );
    }
    if kind != "simlock" {
        let shot = name.to_owned();
        pill = pill.child(rail_button(if capturing { "Capturing screenshot" } else { "Save screenshot" }, &icons::CAMERA, !streaming || capturing, t, move |s| screenshot(s, &shot)));
    }
    let more = vec![
        kit::MenuItem::new("Float device over chat").icon(&icons::PICTURE_IN_PICTURE_2).on_select(float_device),
        kit::MenuItem::new("Close device panel").icon(&icons::X).on_select(|s: &mut App| {
            s.ui.panel.device.menu = None;
            if let Some(surface) = s.ui.panel.active { super::close(s, surface); }
        }),
        kit::MenuItem::new(if kind == "simlock" { "Return device to the pool" } else { "Power off device" }).icon(&icons::POWER).variant("destructive").disabled(app.ui.panel.device.powering_off.is_some()).on_select(power_off),
    ];
    pill = pill.child(rail_menu(app, Menu::More, kit::Button::new("More device actions").icon(&icons::ELLIPSIS), more, t, overlays));
    col().key("device-controls").label("Device controls").w(56.).h_full().pr(8.).py(12.).center().child(pill)
}

/// Web's `floatActive`: the device moves over the chat and the right panel closes.
fn float_device(app: &mut App) {
    let Some(key) = app.ui.panel.device.stream.as_ref().map(|s| s.key.clone()) else { return };
    app.ui.panel.device.menu = None;
    app.ui.panel.device.float = Some(Float { key, frame: None, start: None, area: [0.; 4] });
    app.ui.shell.panel_open = false;
}

fn dock_float(app: &mut App) {
    if let Some(float) = app.ui.panel.device.float.take() {
        if let Some((host, device)) = float.key.split_once('\u{0}') {
            let name = state(app).and_then(|state| state["devices"].as_array())
                .and_then(|devices| devices.iter().find(|row| row["id"] == device && row["hostId"] == host))
                .map(|row| data::s(row, "name").to_owned()).unwrap_or_default();
            super::tabs::open(app, super::tabs::Target::Device { host: host.into(), device: device.into(), name, title: None });
        }
    }
}

fn float_gesture(s: &mut App, d: &rune::Drag, dir: (i8, i8), aspect: f32) {
    let Some(float) = s.ui.panel.device.float.as_mut() else { return };
    let container = (float.area[2], float.area[3]);
    let start = match (d.phase, float.start) {
        (Phase::Down, _) | (_, None) => {
            let start = super::preview::pip_frame(float.frame, aspect, container);
            float.start = Some(start);
            start
        }
        (_, Some(start)) => start,
    };
    let [x, y, w, _] = if dir == (0, 0) {
        let (x, y) = super::preview::pip_clamp((start[0] + d.dx, start[1] + d.dy), (start[2], start[3]), container);
        [x, y, start[2], start[3]]
    } else {
        super::preview::pip_resize(start, dir, (d.dx, d.dy), aspect, container)
    };
    float.frame = Some([x, y, w]);
    if matches!(d.phase, Phase::Up) {
        float.start = None;
    }
}

/// The device floating over the chat, as the browser's mini player.
pub fn float(app: &App, t: &T, out: &mut Vec<El>) {
    let ui = &app.ui.panel.device;
    let Some(float) = &ui.float else { return };
    let Some(stream) = ui.stream.as_ref().filter(|s| s.key == float.key) else { return };
    let (iw, ih) = stream.pixels.size();
    let aspect = if iw > 0 && ih > 0 { iw as f32 / ih as f32 } else { 9. / 19.5 };
    let right = if app.ui.shell.panel_open { app.ui.panel.width + 1. } else { 0. };
    let area = el()
        .key("device-float-area")
        .transition(rune::Transition::none())
        .pin(Some(0.), Some(theme::layout::TOPBAR_H), Some(right), Some(0.))
        .on_layout(|s: &mut App, r| {
            if let Some(float) = s.ui.panel.device.float.as_mut() {
                float.area = r;
            }
        });
    let container = (float.area[2], float.area[3]);
    if container.0 < 1. || container.1 < 1. {
        out.push(area);
        return;
    }
    let [x, y, w, h] = super::preview::pip_frame(float.frame, aspect, container);
    let action = |label: &'static str, glyph: &'static rune::Icon| kit::Button::new(label).icon(glyph).variant("ghost").size("icon-xs");
    let pill = row()
        .pin(None, Some(6.), Some(6.), None)
        .h(32.)
        .padding(3.)
        .gap(2.)
        .center_y()
        .radius(8.)
        .border(1., t.c.border.alpha_mul(0.8))
        .fill(t.c.popover.alpha_mul(0.92))
        .group_opacity(0., 1.)
        .cursor(rune::Cursor::Grab)
        .on_drag(move |s: &mut App, d, _| float_gesture(s, d, (0, 0), aspect))
        .child(action("Open device in right panel", &icons::PANEL_RIGHT).on_press(dock_float).build(t).help("Open device in right panel"))
        .child(action("Close floating device", &icons::X).on_press(|s: &mut App| s.ui.panel.device.float = None).build(t).help("Close floating device"));
    let corners = [(-1i8, -1i8), (1, -1), (-1, 1), (1, 1)].map(|(dx, dy)| {
        let (l, r) = if dx < 0 { (Some(-8.), None) } else { (None, Some(-8.)) };
        let (top, b) = if dy < 0 { (Some(-8.), None) } else { (None, Some(-8.)) };
        el().pin(l, top, r, b).size(16., 16.).cursor(rune::Cursor::Move).key(format!("device-float-resize-{dx}-{dy}")).on_drag(move |s: &mut App, d, _| float_gesture(s, d, (dx, dy), aspect))
    });
    let frame = el()
        .key("device-float")
        .pin(Some(x), Some(y), None, None)
        .size(w, h)
        .hover_group()
        .role("region")
        .label("Floating device")
        .child(el().size(w, h).radius(12.).clip_rounded(12.).fill(t.c.background).border(1., t.c.border.alpha_mul(0.8)).child(screen_el(stream, (w, h), "device-float-screen")))
        .children(corners)
        .child(pill);
    out.push(area.child(crate::kit::motion::popup(frame)));
}

fn tool_section(title: &str, children: Vec<El>, t: &T) -> El {
    col().w_full().gap(8.).px(12.).py(10.).child(text(title.to_owned()).font(theme::XS_MEDIUM).color(t.c.muted_foreground)).children(children)
}

fn tool_row(label: &str, control: El, t: &T) -> El {
    // The label keeps its width; the control gives way.
    row().w_full().gap(8.).center_y().child(text(label.to_owned()).font(theme::TEXT_XS).color(t.c.foreground).single_line()).child(el().grow(1.)).child(control.shrink())
}

/// Chips for one setting: the current value pressed, each sending `action(value)`.
fn chips(options: &[(&'static str, &'static str)], current: Option<&str>, disabled: bool, t: &T, action: fn(&str) -> Value) -> El {
    row().flex_wrap().gap(4.).children(options.iter().map(|&(value, label)| {
        kit::Button::new(label)
            .variant(if current == Some(value) { "secondary" } else { "outline" })
            .size("xs")
            .disabled(disabled)
            .on_press(move |s: &mut App| act(s, action(value)))
            .build(t)
            .checked(current == Some(value))
    }))
}

fn tool_switch(label: &'static str, setting: &'static str, settings: Option<&Value>, disabled: bool, t: &T) -> El {
    let on = settings.and_then(|v| v[setting].as_bool());
    let switch = kit::Switch::new(on == Some(true))
        .label(label)
        .hide_label()
        .disabled(disabled || on.is_none())
        .on_toggle(move |s: &mut App| act(s, json!({"type": "setToggle", "setting": setting, "value": on != Some(true)})))
        .build(&kit::theme(t));
    tool_row(label, switch, t)
}

fn tools_edit<'a>(s: &'a mut App, f: fn(&mut Tools) -> &mut TextEdit) -> &'a mut TextEdit {
    f(s.ui.panel.device.tools.get_or_insert_with(Tools::default))
}

/// A field with its button; Enter or the button sends `submit(text)` when the field isn't empty.
fn tool_field(field: fn(&mut Tools) -> &mut TextEdit, edit: &TextEdit, placeholder: &str, button: &'static str, disabled: bool, t: &T, submit: fn(&str) -> Option<Value>) -> El {
    let send = move |s: &mut App| {
        let text = tools_edit(s, field).text.trim().to_owned();
        if let Some(action) = submit(&text).filter(|_| !text.is_empty()) {
            act(s, action);
        }
    };
    let input = kit::Input::new(edit).placeholder(placeholder).size("sm").disabled(disabled).on_edit(move |s: &mut App| tools_edit(s, field)).on_submit(move |s: &mut App, _: &mut Tx| send(s)).build(t);
    row().w_full().gap(6.).center_y().child(input.grow(1.).min_w(0.)).child(kit::Button::new(button).variant("outline").size("xs").disabled(disabled).on_press(send).build(t))
}

const PERMISSIONS: [&str; 11] = ["camera", "microphone", "photos", "contacts", "calendar", "reminders", "location", "notifications", "motion", "media-library", "faceid"];

/// Web `EventLogSection`: collapsed by default; the log streams only while it is open.
fn event_log(stream: Option<&Stream>, t: &T) -> El {
    let log = stream.and_then(|s| s.log.as_ref());
    let trigger = row()
        .key("device-event-log")
        .w_full()
        .px(12.)
        .py(10.)
        .gap(6.)
        .center_y()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label("Event log")
        .checked(log.is_some())
        .on_press(toggle_log)
        .child(text("Event log").font(theme::XS_MEDIUM).color(t.c.muted_foreground))
        .child(el().grow(1.))
        .child(icon(if log.is_some() { &icons::CHEVRON_UP } else { &icons::CHEVRON_DOWN }, 14.).color(t.c.muted_foreground));
    let Some(log) = log else { return col().w_full().child(el().h(1.).w_full().fill(t.c.border)).child(trigger) };
    let entries = log.data.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let style = theme::mono(theme::TEXT_2XS);
    let rows: Vec<El> = if entries.is_empty() {
        vec![text("No events yet.").font(style).color(t.c.muted_foreground)]
    } else {
        entries
            .iter()
            .map(|e| {
                row().key(format!("device-event-{}", e.id)).w_full().gap(8.).child(text(inspect::log_time(&e.timestamp).to_owned()).font(style).color(t.c.muted_foreground)).child(text(e.summary.clone()).font(style).color(t.c.foreground).single_line().truncate().shrink())
            })
            .collect()
    };
    col()
        .w_full()
        .child(el().h(1.).w_full().fill(t.c.border))
        .child(trigger)
        .child(col().key("device-event-log-entries").w_full().max_h(256.).px(12.).pb(8.).gap(2.).scroll(rune::ScrollSpec { stick_bottom: true }).children(rows))
}

/// `DeviceToolsPanel`: host diagnostics, the app, the device's settings, accessibility,
/// location, permissions, push and the event log.
fn tools_panel(stream: Option<&Stream>, tools: &Tools, diagnostics: Option<&str>, hub_ready: bool, t: &T) -> El {
    let android = stream.is_some_and(|s| s.android);
    let settings = settings(stream);
    let disabled = stream.is_none_or(|s| s.detail.is_some() || s.info.is_none());
    let foreground = stream.and_then(|s| s.info.as_ref()).map(|v| &v["foregroundApp"]).filter(|v| v.is_object());
    let mut app = vec![];
    if let Some(fg) = foreground {
        let id = data::s(fg, "id").to_owned();
        let name = fg["name"].as_str().unwrap_or(&id).to_owned();
        let (quit, relaunch) = (id.clone(), id.clone());
        app.push(tool_row(
            "Foreground",
            row()
                .gap(4.)
                .center_y()
                .child(text(name).font(theme::TEXT_XS).color(t.c.muted_foreground).single_line().truncate().max_w(110.).shrink())
                .child(kit::Button::new("Terminate").variant("outline").size("xs").disabled(disabled).on_press(move |s: &mut App| act(s, json!({"type": "terminateApp", "appId": quit}))).build(t))
                .child(kit::Button::new("Relaunch").variant("outline").size("xs").disabled(disabled).on_press(move |s: &mut App| act(s, json!({"type": "launchApp", "appId": relaunch}))).build(t)),
            t,
        ));
    }
    app.push(tool_field(|t| &mut t.url, &tools.url, "https://… or myapp://", "Open", disabled, t, |url| Some(json!({"type": "openUrl", "url": url}))));
    app.push(tool_field(|t| &mut t.app, &tools.app, if android { "Package name to launch" } else { "Bundle ID to launch" }, "Launch", disabled, t, |id| Some(json!({"type": "launchApp", "appId": id}))));

    let appearance = settings.and_then(|v| v["appearance"].as_str());
    let mut device = vec![
        tool_row("Appearance", chips(&[("light", "Light"), ("dark", "Dark")], appearance, disabled, t, |v| json!({"type": "setAppearance", "value": v})), t),
        tool_row("Text size", el(), t),
        chips(&[("small", "Small"), ("default", "Default"), ("large", "Large"), ("extra-large", "Extra large")], settings.and_then(|v| v["textSize"].as_str()), disabled, t, |v| json!({"type": "setTextSize", "value": v})).w_full(),
    ];
    if android {
        device.push(tool_row("Orientation", chips(&[("portrait", "Portrait"), ("landscape_left", "Landscape")], None, disabled, t, |v| json!({"type": "setOrientation", "value": v})), t));
        device.push(tool_switch("Reduce Motion", "reduceMotion", settings, disabled, t));
        device.push(tool_switch("Network", "networkEnabled", settings, disabled, t));
    } else {
        if settings.is_some_and(|v| v["liquidGlass"].is_string()) {
            device.push(tool_row("Liquid Glass", chips(&[("clear", "Clear"), ("tinted", "Tinted")], settings.and_then(|v| v["liquidGlass"].as_str()), disabled, t, |v| json!({"type": "setLiquidGlass", "value": v})), t));
        }
        if settings.is_some_and(|v| v["colorFilter"].is_string()) {
            device.push(tool_row("Color filter", el(), t));
            device.push(chips(
                &[("none", "None"), ("grayscale", "Grayscale"), ("red-green", "Red/Green"), ("green-red", "Green/Red"), ("blue-yellow", "Blue/Yellow")],
                settings.and_then(|v| v["colorFilter"].as_str()),
                disabled,
                t,
                |v| json!({"type": "setColorFilter", "value": v}),
            ).w_full());
        }
        for (label, setting) in [("Reduce Motion", "reduceMotion"), ("Increase Contrast", "increaseContrast"), ("Reduce Transparency", "reduceTransparency"), ("Show Borders", "showBorders"), ("VoiceOver", "voiceOver")] {
            device.push(tool_switch(label, setting, settings, disabled, t));
        }
    }

    let location = settings.and_then(|v| v["location"].as_object()).map(|l| format!("{:.4}, {:.4}", l["latitude"].as_f64().unwrap_or(0.), l["longitude"].as_f64().unwrap_or(0.)));
    let set_location = move |s: &mut App| {
        let Some(tools) = s.ui.panel.device.tools.as_ref() else { return };
        let (Ok(latitude), Ok(longitude)) = (tools.latitude.text.trim().parse::<f64>(), tools.longitude.text.trim().parse::<f64>()) else {
            s.ui.panel.device.error = Some("Enter a latitude and longitude.".into());
            return;
        };
        if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
            s.ui.panel.device.error = Some("Latitude is -90 to 90 and longitude -180 to 180.".into());
            return;
        }
        act(s, json!({"type": "setLocation", "latitude": latitude, "longitude": longitude}));
    };
    let location = vec![
        tool_row("Current", text(location.unwrap_or_else(|| "Not set".into())).font(theme::TEXT_XS).color(t.c.muted_foreground), t),
        row()
            .w_full()
            .gap(6.)
            .child(kit::Input::new(&tools.latitude).placeholder("Latitude").size("sm").on_edit(|s: &mut App| tools_edit(s, |t| &mut t.latitude)).build(t).grow(1.).min_w(0.))
            .child(kit::Input::new(&tools.longitude).placeholder("Longitude").size("sm").on_edit(|s: &mut App| tools_edit(s, |t| &mut t.longitude)).build(t).grow(1.).min_w(0.)),
        row()
            .gap(6.)
            .child(kit::Button::new("Set location").variant("outline").size("xs").disabled(disabled).on_press(set_location).build(t))
            .child(kit::Button::new("Clear").variant("ghost").size("xs").disabled(disabled).on_press(|s: &mut App| act(s, json!({"type": "clearLocation"}))).build(t)),
    ];

    let default_app = foreground.map_or("App ID", |fg| data::s(fg, "id"));
    let permission = PERMISSIONS[tools.permission.min(PERMISSIONS.len() - 1)];
    let decide = |decision: &'static str| {
        move |s: &mut App| {
            let Some(tools) = s.ui.panel.device.tools.as_ref() else { return };
            let typed = tools.permission_app.text.trim().to_owned();
            let fallback = s.ui.panel.device.stream.as_ref().and_then(|s| s.info.as_ref()).and_then(|v| v["foregroundApp"]["id"].as_str()).unwrap_or_default().to_owned();
            let app_id = if typed.is_empty() { fallback } else { typed };
            if app_id.is_empty() {
                s.ui.panel.device.error = Some("Enter an app ID.".into());
                return;
            }
            let permission = PERMISSIONS[tools.permission.min(PERMISSIONS.len() - 1)];
            act(s, json!({"type": "setPermission", "appId": app_id, "permission": permission, "decision": decision}));
        }
    };
    let permissions = vec![
        kit::Input::new(&tools.permission_app).placeholder(default_app).size("sm").on_edit(|s: &mut App| tools_edit(s, |t| &mut t.permission_app)).build(t),
        row().w_full().flex_wrap().gap(4.).children(PERMISSIONS.iter().enumerate().map(|(i, p)| {
            kit::Button::new(*p).variant(if *p == permission { "secondary" } else { "outline" }).size("xs").on_press(move |s: &mut App| s.ui.panel.device.tools.get_or_insert_with(Tools::default).permission = i).build(t).checked(*p == permission)
        })),
        row()
            .gap(6.)
            .child(kit::Button::new("Grant").variant("outline").size("xs").disabled(disabled).on_press(decide("grant")).build(t))
            .child(kit::Button::new("Revoke").variant("outline").size("xs").disabled(disabled).on_press(decide("revoke")).build(t))
            .child(kit::Button::new("Reset").variant("ghost").size("xs").disabled(disabled).on_press(decide("reset")).build(t)),
    ];

    let overlay = kit::Switch::new(stream.is_some_and(|s| s.ax.is_some()))
        .label("Overlay element frames")
        .hide_label()
        .disabled(!hub_ready)
        .on_toggle(toggle_ax)
        .build(&kit::theme(t));
    let mut body = col().w_full();
    if let Some(detail) = diagnostics {
        body = body.child(tool_section("Host diagnostics", vec![text(detail.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()], t));
    }
    body = body
        .child(tool_section("App", app, t))
        .child(tool_section(if android { "Emulator" } else { "Simulator" }, device, t))
        .child(tool_section("Accessibility", vec![tool_row("Overlay element frames", overlay, t)], t))
        .child(tool_section("Location", location, t))
        .child(tool_section("Permissions", permissions, t));
    if !android {
        let push_disabled = disabled || foreground.is_none();
        let send = |s: &mut App| {
            let alert = s.ui.panel.device.tools.as_ref().map(|t| t.push.text.trim().to_owned()).unwrap_or_default();
            let app = s.ui.panel.device.stream.as_ref().and_then(|s| s.info.as_ref()).and_then(|v| v["foregroundApp"]["id"].as_str()).map(str::to_owned);
            if let Some(app) = app.filter(|_| !alert.is_empty()) {
                act(s, json!({"type": "sendPush", "appId": app, "payload": {"aps": {"alert": alert}}}));
            }
        };
        let push = kit::Input::new(&tools.push).placeholder("Alert text").size("sm").disabled(push_disabled).on_edit(|s: &mut App| tools_edit(s, |t| &mut t.push)).on_submit(move |s: &mut App, _: &mut Tx| send(s)).build(t);
        body = body.child(tool_section("Push notification", vec![push, kit::Button::new("Send to foreground app").variant("outline").size("xs").disabled(push_disabled).on_press(send).build(t)], t));
        // Web streams serve-sim's log; serve-emu has none.
        if hub_ready {
            body = body.child(event_log(stream, t));
        }
    }
    let close = kit::Button::new("Close tools").icon(&icons::X).variant("ghost-muted").size("icon-xs").on_press(|s: &mut App| s.ui.panel.device.tools = None).build(t).help("Close tools");
    col()
        .key("device-tools")
        .role("region")
        .label("Device tools")
        .w(288.)
        .h_full()
        .min_h(0.)
        .border(1., t.c.border)
        .child(header(text("Device tools").font(theme::SM_MEDIUM).color(t.c.foreground), vec![close]))
        .child(el().h(1.).w_full().fill(t.c.border))
        .child(col().grow(1.).min_h(0.).scroll(rune::ScrollSpec { stick_bottom: tools.to_end }).child(body))
}

fn stream_view(app: &App, state: &Value, session: &Value, t: &T, stage: Option<(f32, f32)>, overlays: &mut Vec<El>) -> El {
    let device = state["devices"].as_array().into_iter().flatten().find(|d| d["id"] == session["deviceId"] && d["hostId"] == session["hostId"]);
    let name = device.map_or_else(|| data::s(session, "deviceId").to_owned(), |d| data::s(d, "name").to_owned());
    let back = icon_button("Back to devices", &icons::ARROW_LEFT).on_press(|s: &mut App| super::activate(s, super::Surface::Device)).build(t).help("Back to devices");
    let reconnect = icon_button("Reconnect", &icons::REFRESH_CW).on_press(|s: &mut App| s.ui.panel.device.stream = None).build(t).help("Reconnect");
    let streams = transport(state, session) != Transport::WebRtc;
    let title = text(name.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).single_line().truncate();
    let top = header(row().gap(4.).center_y().child(back).child(title), if streams { vec![reconnect] } else { vec![] });
    let floating = app.ui.panel.device.float.as_ref().is_some_and(|f| Some(&f.key) == app.ui.panel.device.stream.as_ref().map(|s| &s.key));
    let body = if !streams {
        centered(vec![text(name.clone()).font(theme::SM_MEDIUM).color(t.c.foreground), note(WEBRTC, t)])
    } else if floating {
        centered(vec![note("This device is floating over the chat.", t), kit::Button::new("Show here").variant("outline").size("xs").on_press(dock_float).build(t)])
    } else {
        match &app.ui.panel.device.stream {
            Some(stream) => {
                let shared = stream.shared.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(failed) = shared.failed.clone() {
                    centered(
                        vec![
                            note(failed, t),
                            kit::Button::new("Reconnect").variant("outline").size("xs").on_press(|s: &mut App| s.ui.panel.device.stream = None).build(t),
                        ],
                    )
                } else if shared.frames == 0 {
                    centered(vec![spinner(t), note("Connecting video\u{2026}", t)])
                } else {
                    screen(stream, stage, &name, t)
                }
            }
            None => centered(vec![spinner(t), note("Connecting video\u{2026}", t)]),
        }
    };
    let stream = app.ui.panel.device.stream.as_ref();
    let body = if streams {
        let mut body = row().grow(1.).min_h(0.).child(body.grow(1.).min_w(0.)).child(rail(app, stream, &name, t, overlays));
        if let Some(tools) = &app.ui.panel.device.tools
            && stream.is_some_and(|s| runs_host_commands(&s.kind))
        {
            let hub_ready = app.ui.panel.device.fixture.is_some() || app.src.net_ref().is_some();
            body = body.child(tools_panel(stream, tools, host_diagnostics(state, data::s(session, "hostId")), hub_ready, t));
        }
        body
    } else {
        body
    };
    col().grow(1.).min_h(0.).child(top).child(el().h(1.).w_full().fill(t.c.border)).child(body)
}

/// `stage`: the screen area's laid-out size, from the last frame.
pub fn view(app: &App, t: &T, stage: Option<(f32, f32)>, overlays: &mut Vec<El>) -> El {
    let ui = &app.ui.panel.device;
    let Some(state) = state(app) else {
        return centered(vec![spinner(t), note("Finding devices\u{2026}", t)]);
    };
    if state["onboardingCompleted"] != true || state["hostStatus"] == "disabled" {
        return centered(
            vec![
                text("Set up devices").font(theme::SM_MEDIUM).color(t.c.foreground),
                kit::Button::new("Open device settings").variant("outline").size("xs").on_press(|s: &mut App| s.navigate(crate::Route::Settings("devices".into()))).build(t),
            ],
        );
    }
    if let Some((_, _, booted)) = ui.opening.iter().find(|(_, key, _)| ui.selected.as_ref() == Some(key)) {
        return centered(vec![spinner(t), note(if *booted { "Opening device\u{2026}" } else { "Starting device\u{2026}" }, t)]);
    }
    let thread = super::context(app).map(|(id, _)| id).unwrap_or_default();
    let mut notices = vec![];
    if let Some(error) = &ui.error {
        notices.push(row().w_full().px(12.).py(6.).fill(t.c.destructive.alpha_mul(0.1)).child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive_foreground).wrap().grow(1.)));
    }
    let booting: Vec<&str> = state["bootingDevices"].as_array().into_iter().flatten().filter(|d| d["threadId"] == thread.as_str()).map(|d| data::s(d, "name")).collect();
    if !booting.is_empty() {
        notices.push(row().w_full().px(12.).py(6.).child(text(format!("Starting {}\u{2026} This can take a minute.", booting.join(", "))).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().grow(1.)));
    }
    let showing = session(state, &thread, ui.selected.as_deref()).filter(|_| !ui.browsing);
    // Web's `DeviceDiscoveryWarning`s, over the list only.
    if showing.is_none() {
        for host in hosts(state) {
            let id = data::s(host, "id");
            let Some(detail) = host_diagnostics(state, id) else { continue };
            let tag = format!("{id}\u{0}{detail}");
            if ui.dismissed.contains(&tag) {
                continue;
            }
            let dismiss = kit::Button::new("Dismiss").icon(&icons::X).variant("ghost-muted").size("icon-xs").on_press(move |s: &mut App| s.ui.panel.device.dismissed.push(tag.clone())).build(t).help("Dismiss");
            let lines = col()
                .grow(1.)
                .min_w(0.)
                .gap(2.)
                .child(text(format!("Device discovery warning \u{b7} {}", data::s(host, "label"))).font(theme::XS_MEDIUM).color(t.c.foreground).wrap())
                .child(text(detail.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap());
            notices.push(
                row()
                    .key(format!("device-discovery-{id}"))
                    .role("status")
                    .w_full()
                    .px(12.)
                    .py(8.)
                    .gap(8.)
                    .fill(t.c.warning.alpha_mul(0.1))
                    .child(icon(&icons::TRIANGLE_ALERT, 14.).color(t.c.warning_foreground))
                    .child(lines)
                    .child(dismiss),
            );
        }
    }
    let content = if let Some(session) = showing {
        stream_view(app, state, session, t, stage, overlays)
    } else if state["devices"].as_array().is_some_and(|d| !d.is_empty()) {
        list(app, state, t)
    } else {
        let status = data::s(state, "hostStatus");
        let detail = state["hostStatusDetail"].as_str();
        let line = match status {
            "installing" => detail.unwrap_or("Installing device support\u{2026}").to_owned(),
            "failed" => detail.unwrap_or("The device hub failed to start.").to_owned(),
            "idle" | "starting" => "Finding devices\u{2026}".to_owned(),
            _ => "No simulators or emulators were found on this environment.".to_owned(),
        };
        let mut lines = vec![note(line, t)];
        if !matches!(status, "installing" | "idle" | "starting") {
            lines.push(kit::Button::new("Refresh devices").variant("outline").size("xs").on_press(refresh).build(t));
        } else {
            lines.insert(0, spinner(t));
        }
        centered(lines)
    };
    col().grow(1.).min_h(0.).children(notices).child(content)
}

/// The "More device actions" menu, open, for its scene.
pub fn open_menu(app: &mut App) {
    app.ui.panel.device.menu = Some(Menu::More);
}

/// Device tools, open, for its scene.
pub fn open_tools(app: &mut App) {
    app.ui.panel.device.tools = Some(Tools::default());
}

/// Device tools with everything web adds past the settings: host diagnostics, element frames
/// over the screen and an open event log, scrolled to its end.
pub fn tools_more_fixture(app: &mut App) {
    if let Some(state) = app.ui.panel.device.fixture.as_mut() {
        state["hostStatuses"] = json!({"local": {"status": "ready", "detail": "Android emulators: adb is not on PATH, so only iOS simulators are listed. Install Android platform-tools to add emulators."}});
    }
    app.ui.panel.device.tools = Some(Tools { to_end: true, ..Tools::default() });
    let Some(stream) = app.ui.panel.device.stream.as_mut() else { return };
    let ax = |id: &str, label: &str, [x, y, width, height]: [f32; 4]| inspect::AxElement { id: id.into(), label: label.into(), x, y, width, height };
    stream.ax = Some(inspect::Feed::fixed(Some(vec![
        ax("title", "Welcome back", [0.08, 0.12, 0.6, 0.05]),
        ax("email", "Email", [0.08, 0.24, 0.84, 0.06]),
        ax("password", "Password", [0.08, 0.33, 0.84, 0.06]),
        ax("sign-in", "Sign in", [0.08, 0.44, 0.84, 0.065]),
        ax("forgot", "", [0.3, 0.53, 0.4, 0.035]),
        ax("tab-home", "Home", [0.04, 0.9, 0.28, 0.07]),
        ax("tab-settings", "Settings", [0.68, 0.9, 0.28, 0.07]),
    ])));
    let entry = |id: i64, time: &str, summary: &str| inspect::LogEntry { id, timestamp: format!("2026-11-22T{time}.000Z"), summary: summary.into() };
    stream.log = Some(inspect::Feed::fixed(vec![
        entry(1, "10:14:02", "Launched com.example.app"),
        entry(2, "10:14:05", "Tap (196, 380) on \"Email\""),
        entry(3, "10:14:09", "Typed 18 characters"),
        entry(4, "10:14:12", "Tap (196, 456) on \"Sign in\""),
        entry(5, "10:14:13", "Alert: \"Wrong password\""),
        entry(6, "10:14:15", "Tap (196, 520) on \"OK\""),
    ]));
}

/// The device floating over the chat, for its scene.
pub fn float_fixture(app: &mut App) {
    let Some(key) = app.ui.panel.device.stream.as_ref().map(|s| s.key.clone()) else { return };
    // The chat area the first layout reports, so the scene's single frame draws the float.
    let area = [0., theme::layout::TOPBAR_H, 1440., 900. - theme::layout::TOPBAR_H];
    app.ui.panel.device.float = Some(Float { key, frame: None, start: None, area });
    app.ui.shell.panel_open = false;
}

/// Recorded device states for the panel's scenes.
pub fn fixture(app: &mut App, sessions: Value) {
    let thread = super::context(app).map(|(id, _)| id).unwrap_or_default();
    let sessions: Vec<Value> = sessions.as_array().into_iter().flatten().map(|s| json!({"threadId": thread, "hostId": s["hostId"], "deviceId": s["deviceId"], "platform": s["platform"], "openedAt": "2026-11-16T10:00:00.000Z"})).collect();
    app.ui.panel.device.fixture = Some(json!({
        "hostStatus": "ready", "hostStatuses": {}, "onboardingCompleted": true, "agentAccessEnabled": true, "hubBasePath": "/api/device-hub", "revision": 1,
        "hosts": [{"id": "local", "label": "This Mac", "kind": "local", "platforms": [], "hubInstalled": true, "agentDeviceInstalled": true}],
        "easHosts": [{"id": "eas", "label": "EAS", "kind": "eas"}],
        "devices": [
            {"hostId": "local", "id": "ios-16", "platform": "ios", "name": "iPhone 16 Pro", "version": "iOS 18.2", "booted": true, "physical": false},
            {"hostId": "local", "id": "ios-se", "platform": "ios", "name": "iPhone SE (3rd generation)", "version": "iOS 18.2", "booted": false, "physical": false},
            {"hostId": "local", "id": "ipad", "platform": "ios", "name": "iPad Air 13-inch (M2)", "version": "iOS 18.2", "booted": false, "physical": false},
            {"hostId": "local", "id": "pixel", "platform": "android", "name": "Pixel 9", "version": "Android 15", "booted": true, "physical": false},
            {"hostId": "eas", "id": "eas-16", "platform": "ios", "name": "iPhone 16 (EAS)", "version": "iOS 18.2", "booted": true, "physical": false}
        ],
        "sessions": sessions,
    }));
    let first = app.ui.panel.device.fixture.as_ref().and_then(|state| {
        let session = state["sessions"].as_array()?.first()?;
        let transport = transport(state, session);
        (transport != Transport::WebRtc).then(|| (key(data::s(session, "hostId"), data::s(session, "deviceId")), transport, session["platform"] == "android", host_kind(state, session).to_owned()))
    });
    app.ui.panel.device.stream = first.map(|(key, transport, android, kind)| fixture_stream(key, transport, android, kind));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mjpeg_parts_split_by_length_or_boundary() {
        assert_eq!(boundary("multipart/x-mixed-replace; boundary=frame").as_deref(), Some("frame"));
        assert_eq!(boundary("multipart/x-mixed-replace;boundary=\"--b1\"").as_deref(), Some("--b1"));
        assert_eq!(boundary("image/jpeg"), None);
        let mut buf = b"--frame\r\nContent-Type: image/jpeg\r\nContent-Length: 3\r\n\r\nabc\r\n--frame\r\nContent-Type: image/jpeg\r\n\r\nxyz\r\n--frame\r\nContent-Length: 9\r\n\r\npart".to_vec();
        assert_eq!(take_part(&mut buf, "frame").as_deref(), Some(&b"abc"[..]));
        assert_eq!(take_part(&mut buf, "frame").as_deref(), Some(&b"xyz"[..]), "no length: up to the next boundary");
        assert_eq!(take_part(&mut buf, "frame"), None, "a part still arriving waits");
        buf.extend_from_slice(b"ial!\r\n");
        assert_eq!(take_part(&mut buf, "frame").as_deref(), Some(&b"partial!\r"[..]));
        let mut dashed = b"----b1\r\n\r\nhi\r\n----b1\r\n".to_vec();
        assert_eq!(take_part(&mut dashed, "--b1").as_deref(), Some(&b"hi"[..]));
    }

    #[test]
    fn each_host_kind_streams_as_web_picks() {
        let state = json!({
            "hosts": [{"id": "local", "kind": "local"}, {"id": "box", "kind": "ssh"}],
            "simlockHosts": [{"id": "lock", "kind": "simlock"}],
            "sessions": [
                {"threadId": "t", "hostId": "local", "deviceId": "a", "platform": "ios", "openedAt": "2026-01-01T00:00:00Z"},
                {"threadId": "t", "hostId": "box", "deviceId": "b", "platform": "ios", "openedAt": "2026-01-02T00:00:00Z"},
                {"threadId": "u", "hostId": "lock", "deviceId": "c", "platform": "ios", "openedAt": "2026-01-03T00:00:00Z"}
            ]
        });
        let newest = session(&state, "t", None).unwrap();
        assert_eq!(newest["deviceId"], "b", "the newest session for the thread");
        assert_eq!(session(&state, "t", Some(key("local", "a").as_str())).unwrap()["deviceId"], "a", "a picked device wins");
        assert_eq!(transport(&state, newest), Transport::Avcc { mjpeg: true });
        assert_eq!(transport(&state, &state["sessions"][2]), Transport::Avcc { mjpeg: false }, "Simlock streams AVCC only");
        assert_eq!(transport(&state, &json!({"hostId": "local", "platform": "android"})), Transport::Semu);
        assert_eq!(transport(&json!({"easHosts": [{"id": "e", "kind": "eas"}]}), &json!({"hostId": "e", "platform": "ios"})), Transport::WebRtc);
        assert_eq!(
            helper_url("http://h:3000/", "A B/1", "box", "t+k", "mjpeg"),
            "http://h:3000/api/device-hub/vendor/serve-sim/helper/A%20B%2F1/stream.mjpeg?wsTicket=t%2Bk&hostId=box"
        );
        let devices = json!({"hosts": [{"id": "local", "label": "This Mac", "kind": "local"}], "devices": [
            {"hostId": "local", "id": "1", "platform": "android", "name": "Pixel", "version": "Android 15", "booted": false},
            {"hostId": "local", "id": "2", "platform": "ios", "name": "iPhone SE", "version": "iOS 18", "booted": false},
            {"hostId": "local", "id": "3", "platform": "ios", "name": "iPad", "version": "iOS 18", "booted": true}
        ]});
        let groups = grouped(&devices);
        let names: Vec<(&str, Vec<&str>)> = groups.iter().map(|(t, l)| (*t, l.iter().map(|d| data::s(d, "name")).collect())).collect();
        assert_eq!(names, vec![("iOS Simulators", vec!["iPad", "iPhone SE"]), ("Android Emulators", vec!["Pixel"])]);
        assert_eq!(subtitle(&devices, &devices["devices"][2]), "This Mac · iOS 18 · Running");
    }

    #[test]
    fn host_diagnostics_come_from_ready_hosts_and_the_hubs_own_endpoints() {
        let state = json!({
            "hostStatus": "ready", "hostStatusDetail": "adb is not on PATH",
            "hosts": [{"id": "local", "kind": "local"}, {"id": "box", "kind": "ssh"}, {"id": "mini", "kind": "ssh"}],
            "hostStatuses": {"box": {"status": "ready", "detail": "Xcode is missing"}, "mini": {"status": "installing", "detail": "Downloading"}}
        });
        assert_eq!(host_diagnostics(&state, "local"), Some("adb is not on PATH"), "the local host falls back to the hub's detail");
        assert_eq!(host_diagnostics(&state, "box"), Some("Xcode is missing"));
        assert_eq!(host_diagnostics(&state, "mini"), None, "only a ready host reports diagnostics");
        assert_eq!(host_diagnostics(&state, "gone"), None);
        assert_eq!(
            ax_endpoint(false, "box", "A B"),
            ("/api/device-hub/vendor/serve-sim/helper/A%20B/ax".to_owned(), "hostId=box".to_owned())
        );
        assert_eq!(ax_endpoint(true, "box", "emulator-5554"), ("/api/device-hub/vendor/serve-emu/api/accessibility".to_owned(), "device=emulator-5554&hostId=box".to_owned()));
    }
}
