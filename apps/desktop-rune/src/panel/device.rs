//! DevicePanel over MJPEG: the environment's simulators and emulators, `device.open` for this
//! thread, and the opened iOS simulator's screen from serve-sim's `stream.mjpeg`, driven through
//! serve-sim's input socket. Every other stream is H.264, which this app can't decode.

mod input;

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::{El, Key, Phase, Pixels, Tx, col, el, icon, row, text};
use serde_json::{Value, json};

use crate::theme::{self, AlphaMul, T};
use crate::{App, data, icons, kit};

pub const STATE: &str = "subscribeDeviceState";
/// `FIRST_FRAME_TIMEOUT_MS` in the web's stream client.
const FIRST_FRAME: Duration = Duration::from_secs(15);
const STREAM_FAILED: &str = "Could not receive the device stream. Reconnect to try again.";
const NO_VIDEO: &str = "No video received from the device. Reconnect to try again.";

#[derive(Default)]
pub struct Ui {
    /// `device.open` in flight: its request, the device's key and whether it was running.
    opening: Option<(u64, String, bool)>,
    listing: Option<u64>,
    error: Option<String>,
    /// The device this panel shows, once picked; otherwise the thread's newest session.
    selected: Option<String>,
    /// Back at the list with a session still open.
    browsing: bool,
    stream: Option<Stream>,
    /// A recorded `DeviceServiceState` for scenes, which have no connection.
    fixture: Option<Value>,
    /// The "More device actions" menu.
    menu: bool,
    /// `device.close` with shutdown in flight.
    powering_off: Option<u64>,
}

struct Stream {
    key: String,
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
    settings: Option<Value>,
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
    picked.or(sessions.first().copied())
}

/// serve-sim serves MJPEG for iOS simulators on local and SSH hosts; Android streams through
/// serve-emu, Simlock only as AVCC and EAS only over WebRTC.
fn mjpeg_available(state: &Value, session: &Value) -> bool {
    let kind = host(state, data::s(session, "hostId")).map_or("local", |h| data::s(h, "kind"));
    session["platform"] == "ios" && matches!(kind, "local" | "ssh")
}

fn encode(value: &str) -> String {
    value.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// `stream.mjpeg` behind the server's device-hub proxy, with the media ticket and host.
fn mjpeg_url(base: &str, device: &str, host: &str, ticket: &str) -> String {
    format!(
        "{}/api/device-hub/vendor/serve-sim/helper/{}/stream.mjpeg?wsTicket={}&hostId={}",
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
    let ui = &mut app.ui.panel.device;
    ui.selected = Some(key(&host, &id));
    ui.browsing = false;
    ui.error = None;
    let booted = device["booted"] == true;
    if let Some(request) = request(app, "device.open", payload) {
        app.ui.panel.device.opening = Some((request, key(&host, &id), booted));
    }
}

fn refresh(app: &mut App) {
    app.ui.panel.device.error = None;
    app.ui.panel.device.listing = request(app, "device.list", json!({}));
}

/// Keeps one stream for the shown session while the Device surface is on screen.
fn sync_stream(app: &mut App, tx: &mut Tx) {
    let shown = app.ui.shell.panel_open && app.ui.panel.active == Some(super::Surface::Device) && !app.ui.panel.device.browsing;
    let wanted = shown
        .then(|| {
            let (thread, _) = super::context(app)?;
            let state = state(app)?;
            let session = session(state, &thread, app.ui.panel.device.selected.as_deref())?;
            mjpeg_available(state, session).then(|| (data::s(session, "hostId").to_owned(), data::s(session, "deviceId").to_owned()))
        })
        .flatten();
    let ui = &mut app.ui.panel.device;
    if ui.stream.as_ref().map(|s| s.key.clone()) == wanted.as_ref().map(|(h, d)| key(h, d)) {
        return;
    }
    ui.stream = None;
    ui.menu = false;
    let Some((host, device)) = wanted else { return };
    if app.ui.panel.device.fixture.is_some() {
        app.ui.panel.device.stream = Some(fixture_stream(key(&host, &device)));
        return;
    }
    let mut stream = Stream {
        key: key(&host, &device),
        pixels: Pixels::new(),
        shared: Arc::default(),
        stop: Arc::default(),
        ticket: None,
        started: Instant::now(),
        seen: 0,
        input: None,
        detail: None,
        settings: None,
    };
    stream.ticket = app.src.net().map(|n| n.http_request("POST", "/api/auth/websocket-ticket/media", json!({})));
    if stream.ticket.is_none() {
        stream.shared.lock().unwrap_or_else(|e| e.into_inner()).failed = Some(STREAM_FAILED.to_owned());
    }
    if let Some(net) = app.src.net_ref() {
        let wake = app.ui.panel.waker.clone();
        let wake = move || {
            if let Some(wake) = &wake {
                wake();
            }
        };
        stream.input = Some(input::Input::open(net.base_url().to_owned(), net.bearer().to_owned(), &host, &device, wake));
    }
    stream.detail = request(app, "device.detail", json!({"hostId": host, "deviceId": device}));
    app.ui.panel.device.stream = Some(stream);
    tx.wake_after(FIRST_FRAME);
}

/// A stream with one plain frame for scenes: the panel's chrome without a simulator.
fn fixture_stream(key: String) -> Stream {
    let shared = Shared { frames: 1, ..Default::default() };
    Stream {
        key,
        pixels: fixture_screen(),
        shared: Arc::new(Mutex::new(shared)),
        stop: Arc::default(),
        ticket: None,
        started: Instant::now(),
        seen: 0,
        input: None,
        detail: None,
        settings: Some(json!({"appearance": "light"})),
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
    std::thread::spawn(move || {
        let saved = s5_net::api_post_read(&base, &bearer, "/api/device-hub/vendor/serve-sim/api/screenshot", &query, 64 << 20)
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
    app.ui.panel.device.menu = false;
    app.ui.panel.device.powering_off = request(app, "device.close", json!({"threadId": thread, "hostId": host, "deviceId": device, "shutdown": true}));
}

pub fn poll(app: &mut App, tx: &mut Tx) -> bool {
    let mut changed = false;
    if let Some((id, _, _)) = app.ui.panel.device.opening
        && let Some(reply) = app.src.net().and_then(|n| n.take_reply(id))
    {
        app.ui.panel.device.opening = None;
        if let Err(e) = reply {
            app.ui.panel.device.error = Some(crate::shell::palette::error_message(&e, "Could not open the device."));
        }
        changed = true;
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
    if let Some(id) = app.ui.panel.device.powering_off
        && let Some(reply) = app.src.net().and_then(|n| n.take_reply(id))
    {
        app.ui.panel.device.powering_off = None;
        match reply {
            Ok(_) => super::close(app, super::Surface::Device),
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
            Ok(detail) => stream.settings = Some(detail["settings"].clone()),
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
                let url = mjpeg_url(&base, device, host, &ticket);
                let (pixels, shared, stop) = (stream.pixels.clone(), stream.shared.clone(), stream.stop.clone());
                std::thread::spawn(move || {
                    let wake = move || {
                        if let Some(wake) = &wake {
                            wake();
                        }
                    };
                    if let Err(e) = read_stream(&url, &pixels, &shared, &stop, &wake)
                        && !stop.load(Ordering::Relaxed)
                    {
                        shared.lock().unwrap_or_else(|e| e.into_inner()).failed = Some(e.into());
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

/// The screen at its fitted size, taking taps, swipes and typing once the stage is laid out.
fn screen(stream: &Stream, stage: Option<(f32, f32)>, name: &str) -> El {
    let (iw, ih) = stream.pixels.size();
    let fitted = stage.filter(|_| iw > 0 && ih > 0).map(|(w, h)| fit((w - 32., h - 32.), (iw as f32, ih as f32))).filter(|(w, h)| *w >= 1. && *h >= 1.);
    let screen = match fitted {
        Some((w, h)) => el()
            .size(w, h)
            .pixels(&stream.pixels)
            .key("device-screen")
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
            }),
        None => el().w_full().grow(1.).min_h(0.).pixels_fit(&stream.pixels, rune::SurfaceFit::Fit),
    };
    col().key("device-stage").grow(1.).min_h(0.).min_w(0.).padding(16.).center().role("img").label(format!("{name} screen")).child(screen)
}

fn rail_button(label: impl Into<String>, glyph: &'static rune::Icon, disabled: bool, t: &T, f: impl Fn(&mut App) + 'static) -> El {
    let label = label.into();
    kit::Button::new(label.clone()).icon(glyph).variant("ghost").size("icon-sm").disabled(disabled).on_press(f).build(t).help(label)
}

/// `DeviceControlsRail`: the shown device's buttons, on the stream's right edge.
fn rail(app: &App, stream: Option<&Stream>, name: &str, t: &T, overlays: &mut Vec<El>) -> El {
    let ready = input_ready(app);
    let (streaming, capturing) = stream.map_or((false, false), |s| {
        let shared = s.shared.lock().unwrap_or_else(|e| e.into_inner());
        (shared.frames > 0 && shared.failed.is_none(), shared.capturing)
    });
    let appearance = stream.and_then(|s| s.settings.as_ref()).and_then(|v| v["appearance"].as_str());
    let next = if appearance == Some("dark") { "light" } else { "dark" };
    let busy = stream.is_none_or(|s| s.detail.is_some());
    let shot = name.to_owned();
    let more = kit::Button::new("More device actions")
        .icon(&icons::ELLIPSIS)
        .variant("ghost")
        .size("icon-sm")
        .on_press(|s: &mut App| s.ui.panel.device.menu = !s.ui.panel.device.menu)
        .build(t)
        .help("More device actions");
    let menu = kit::Menu::new("device-more-menu")
        .trigger(more)
        .placement(kit::Placement::BottomEnd)
        .width(198.)
        .native(true)
        .open(app.ui.panel.device.menu)
        .on_close(|s: &mut App| s.ui.panel.device.menu = false)
        .item(kit::MenuItem::new("Close device panel").icon(&icons::X).on_select(|s: &mut App| {
            s.ui.panel.device.menu = false;
            super::close(s, super::Surface::Device);
        }))
        .separator()
        .item(kit::MenuItem::new("Power off device").icon(&icons::POWER).variant("destructive").disabled(app.ui.panel.device.powering_off.is_some()).on_select(power_off))
        .build(&kit::theme(t), overlays);
    let pill = col()
        .gap(4.)
        .padding(8.)
        .radius(999.)
        .items_center()
        .fill(t.c.background.alpha_mul(0.8))
        .border(1., t.c.border.alpha_mul(0.5))
        .child(rail_button("Home", &icons::HOUSE, !ready, t, |s| {
            if let Some(input) = input(s) {
                input.button("home");
            }
        }))
        .child(rail_button("Rotate device", &icons::ROTATE_CW, !ready, t, |s| {
            if let Some(input) = input(s) {
                input.rotate();
            }
        }))
        .child(rail_button(format!("Switch device to {next} mode"), if appearance == Some("dark") { &icons::SUN } else { &icons::MOON }, busy || appearance.is_none(), t, move |s| {
            act(s, json!({"type": "setAppearance", "value": next}));
        }))
        .child(rail_button(if capturing { "Capturing screenshot" } else { "Save screenshot" }, &icons::CAMERA, !streaming || capturing, t, move |s| screenshot(s, &shot)))
        .child(menu);
    col().key("device-controls").label("Device controls").w(56.).h_full().pr(8.).py(12.).center().child(pill)
}

fn stream_view(app: &App, state: &Value, session: &Value, t: &T, stage: Option<(f32, f32)>, overlays: &mut Vec<El>) -> El {
    let device = state["devices"].as_array().into_iter().flatten().find(|d| d["id"] == session["deviceId"] && d["hostId"] == session["hostId"]);
    let name = device.map_or_else(|| data::s(session, "deviceId").to_owned(), |d| data::s(d, "name").to_owned());
    let back = icon_button("Back to devices", &icons::ARROW_LEFT).on_press(|s: &mut App| s.ui.panel.device.browsing = true).build(t).help("Back to devices");
    let reconnect = icon_button("Reconnect", &icons::REFRESH_CW).on_press(|s: &mut App| s.ui.panel.device.stream = None).build(t).help("Reconnect");
    let mjpeg = mjpeg_available(state, session);
    let title = text(name.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).single_line().truncate();
    let top = header(row().gap(4.).center_y().child(back).child(title), if mjpeg { vec![reconnect] } else { vec![] });
    let body = if !mjpeg {
        centered(vec![text(name).font(theme::SM_MEDIUM).color(t.c.foreground), note("This device requires the H.264 stream, which this app can't show yet.", t)])
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
                    screen(stream, stage, &name)
                }
            }
            None => centered(vec![spinner(t), note("Connecting video\u{2026}", t)]),
        }
    };
    let body = if mjpeg { row().grow(1.).min_h(0.).child(body.grow(1.).min_w(0.)).child(rail(app, app.ui.panel.device.stream.as_ref(), &name, t, overlays)) } else { body };
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
    if let Some((_, _, booted)) = &ui.opening {
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
    let content = if let Some(session) = session(state, &thread, ui.selected.as_deref()).filter(|_| !ui.browsing) {
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
    app.ui.panel.device.menu = true;
}

/// Recorded device states for the panel's scenes.
pub fn fixture(app: &mut App, sessions: Value) {
    let thread = super::context(app).map(|(id, _)| id).unwrap_or_default();
    let sessions: Vec<Value> = sessions.as_array().into_iter().flatten().map(|s| json!({"threadId": thread, "hostId": s["hostId"], "deviceId": s["deviceId"], "platform": s["platform"], "openedAt": "2026-11-16T10:00:00.000Z"})).collect();
    app.ui.panel.device.fixture = Some(json!({
        "hostStatus": "ready", "hostStatuses": {}, "onboardingCompleted": true, "agentAccessEnabled": true, "hubBasePath": "/api/device-hub", "revision": 1,
        "hosts": [{"id": "local", "label": "This Mac", "kind": "local", "platforms": [], "hubInstalled": true, "agentDeviceInstalled": true}],
        "devices": [
            {"hostId": "local", "id": "ios-16", "platform": "ios", "name": "iPhone 16 Pro", "version": "iOS 18.2", "booted": true, "physical": false},
            {"hostId": "local", "id": "ios-se", "platform": "ios", "name": "iPhone SE (3rd generation)", "version": "iOS 18.2", "booted": false, "physical": false},
            {"hostId": "local", "id": "ipad", "platform": "ios", "name": "iPad Air 13-inch (M2)", "version": "iOS 18.2", "booted": false, "physical": false},
            {"hostId": "local", "id": "pixel", "platform": "android", "name": "Pixel 9", "version": "Android 15", "booted": true, "physical": false}
        ],
        "sessions": sessions,
    }));
    let ios = app.ui.panel.device.fixture.as_ref().and_then(|state| state["sessions"].as_array()?.iter().find(|s| s["platform"] == "ios").map(|s| key(data::s(s, "hostId"), data::s(s, "deviceId"))));
    app.ui.panel.device.stream = ios.map(fixture_stream);
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
    fn only_ios_on_local_and_ssh_hosts_stream_mjpeg() {
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
        assert!(mjpeg_available(&state, newest));
        assert!(!mjpeg_available(&state, &state["sessions"][2]), "Simlock streams AVCC only");
        assert!(!mjpeg_available(&state, &json!({"hostId": "local", "platform": "android"})));
        assert_eq!(
            mjpeg_url("http://h:3000/", "A B/1", "box", "t+k"),
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
}
