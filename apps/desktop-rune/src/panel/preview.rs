//! The Browser surface, web's inline `PreviewView`: each tab's chrome row over its page, the
//! empty state, or the unreachable page. `browser` owns Chrome; this owns what the panel shows.
use std::{collections::HashMap, time::Duration};

use rune::{Color, El, Key, KeyEvent, Phase, Pixels, ScrollSpec, Span, TextEdit, Tx, col, el, icon, rich, row, text, text_input};
use serde_json::{Value, json};

use super::browser::{self, Cmd, Status, TabId};
use super::popout;
use super::agent;
use super::browser_import;
use crate::{
    App, Route, data, icons, kit,
    theme::{self, AlphaMul, T},
};

const SERVERS: &str = "subscribeDiscoveredLocalServers";
const ZOOM_LEVELS: [f32; 17] = [0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1., 1.1, 1.25, 1.5, 1.75, 2., 2.5, 3., 4., 5.];
const NON_WEB_SCHEMES: [&str; 14] = [
    "about", "blob", "chrome", "data", "file", "ftp", "javascript", "mailto", "sms", "ssh", "tel", "view-source", "ws", "wss",
];
/// `PREVIEW_VIEWPORT_PRESETS`: Chrome DevTools' emulated devices, in its order.
const PRESETS: [(&str, u32, u32); 17] = [
    ("iPhone SE", 375, 667),
    ("iPhone XR", 414, 896),
    ("iPhone 12 Pro", 390, 844),
    ("iPhone 14 Pro Max", 430, 932),
    ("Pixel 7", 412, 915),
    ("Samsung Galaxy S8+", 360, 740),
    ("Samsung Galaxy S20 Ultra", 412, 915),
    ("iPad Mini", 768, 1024),
    ("iPad Air", 820, 1180),
    ("iPad Pro", 1024, 1366),
    ("Surface Pro 7", 912, 1368),
    ("Surface Duo", 540, 720),
    ("Galaxy Z Fold 5", 344, 882),
    ("Asus Zenbook Fold", 853, 1280),
    ("Samsung Galaxy A51/71", 412, 914),
    ("Nest Hub", 1024, 600),
    ("Nest Hub Max", 1280, 800),
];
const DEVICE_TOOLBAR_H: f32 = 38.;
/// Web's `ScreenRotationIcon`, with its rotated square drawn as a path.
static ROTATE_ICON: rune::Icon = rune::Icon {
    name: "screen-rotation",
    view: 24.0,
    body: "<path d=\"M12 5.28 L18.72 12 L12 18.72 L5.28 12 Z\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\"/><path d=\"M12.5 2a10 10 0 0 1 8.4 5.4\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\"/><path d=\"M20.8 3.5v4h-4\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\"/><path d=\"M11.5 22a10 10 0 0 1-8.4-5.4\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\"/><path d=\"M3.2 20.5v-4h4\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\"/>",
};
static ERROR_ICON: rune::Icon = rune::Icon {
    name: "preview-unreachable",
    view: 64.0,
    body: "<path d=\"M16 12 L48 12 L48 52 L16 52 Z\" stroke=\"currentColor\" stroke-width=\"2.5\" fill=\"none\"/><path d=\"M22 22 L42 22 M22 30 L36 30 M22 38 L40 38\" stroke=\"currentColor\" stroke-width=\"2.5\" stroke-linecap=\"round\" fill=\"none\"/><path d=\"M52 8 L12 56\" stroke=\"currentColor\" stroke-width=\"2.5\" stroke-linecap=\"round\" fill=\"none\"/>",
};

struct Tab {
    pixels: Pixels,
    /// The address bar draft; it follows the page's URL while unfocused.
    bar: TextEdit,
    focused: bool,
    device: Device,
    /// The settled `(url, title)` last offered to history.
    titled: (String, String),
    /// The icon URL last handed to `favicon`.
    icon: Option<String>,
    /// The server's preview session this tab renders, so the thread's agent can drive it.
    session: Option<agent::Key>,
    /// The server process and Chrome target last told this tab is attached.
    announced: Option<(u64, String)>,
    /// The agent's last `revealRequest`, so each one shows the tab once.
    revealed: Option<String>,
    /// A popup the agent's page opened: the opener's session and the popup's target, until the
    /// server's own session for it binds here.
    adopting: Option<(agent::Key, String)>,
    /// A server session this panel doesn't render itself, as the server's frames.
    stream: Option<super::stream::Stream>,
    /// The session's latest snapshot, for a streamed tab's address and history.
    snapshot: Value,
    /// The page's last `(url, loading)`, to drop the agent's cursor on a navigation.
    nav: (String, bool),
}

impl Tab {
    fn new(pixels: Pixels) -> Tab {
        Tab { pixels, bar: TextEdit::new(""), focused: false, device: Device::default(), titled: Default::default(), icon: None, session: None, announced: None, revealed: None, adopting: None, stream: None, snapshot: Value::Null, nav: Default::default() }
    }
}

/// The device toolbar's drafts; the size Chrome emulates is `Status::device`.
struct Device {
    w: TextEdit,
    h: TextEdit,
    /// The locked width / height ratio.
    lock: Option<f32>,
    preset: Option<usize>,
}

impl Default for Device {
    fn default() -> Device {
        Device { w: TextEdit::new(""), h: TextEdit::new(""), lock: None, preset: None }
    }
}

#[derive(Default)]
pub struct Ui {
    browser: browser::Browser,
    tabs: HashMap<TabId, Tab>,
    next: TabId,
    seen: u64,
    shown: Option<TabId>,
    menu: bool,
    appearance: bool,
    scheme: &'static str,
    details: bool,
    watching: bool,
    /// The tab floating over the chat (web's mini player).
    pub(super) pip: Option<TabId>,
    /// The page area's last size, in points.
    area: (f32, f32),
    presets: bool,
    /// When the zoom pill hides, as web's `ZoomIndicator` does 1.5 s after a change.
    zoom_until: i64,
    /// Whether the panel's browser had focus at the last key event (the `previewFocus` context).
    pub(super) focused: bool,
    /// A picked element waiting for its comment, as web's annotation editor holds it.
    pending: Option<Pending>,
    comment: TextEdit,
    pub(super) favicons: super::favicon::Favicons,
    /// The floating preview's `[x, y, width]` in the chat area once moved or resized.
    pip_frame: Option<[f32; 3]>,
    /// The `[x, y, w, h]` a PiP drag or resize started from.
    pip_start: Option<[f32; 4]>,
    /// The chat area the floating preview stays inside, in window points.
    pip_area: [f32; 4],
    /// The pending pick `Cmd::Track` follows.
    tracked: Option<TabId>,
    /// The page's open `alert`, `confirm` or `prompt`.
    dialog: Option<Dialog>,
    /// The page's right-click menu.
    context: Option<Context>,
    /// Chrome starts after a failed start; part of the viewport key so the new worker gets it.
    restarts: u32,
    /// The tab the separate preview window mirrors, and the title it was given.
    popout: Option<TabId>,
    popout_title: String,
    /// The separate window has shown since it was asked for, so hiding it means it closed.
    popout_seen: bool,
    /// Whether the local server's preview events are watched.
    sessions_watched: bool,
    /// `preview.open` requests for tabs opened here: request id, tab, thread.
    opening: Vec<(u64, TabId, String)>,
    /// Session events held until this client's own `preview.open` replies, so its tab binds
    /// instead of a second one opening.
    deferred: Vec<Value>,
    /// The server process the tabs' sessions belong to.
    generation: u64,
    /// The server restarted; tabs take new sessions once the connection is back.
    rebind: bool,
    /// The newest agent pointer drawn and whether a cursor was bright, so a move or a dimming
    /// repaints.
    cursor_seen: (u64, bool),
    /// The person's last input to a page, which dims the agent's cursor (Electron's `human`).
    human: Option<(TabId, std::time::Instant)>,
    /// The display scale the running Chrome launched at, which its frames keep.
    chrome_scale: Option<f32>,
    /// Other environments whose preview sessions this panel follows, as streams.
    remote_watched: Vec<String>,
}

struct Dialog {
    tab: TabId,
    kind: String,
    message: String,
    input: TextEdit,
}

struct Context {
    tab: TabId,
    at: (f32, f32),
    info: Value,
}

struct Pending {
    tab: TabId,
    element: Value,
    image: Option<Vec<u8>>,
    /// The element's viewport rect in CSS pixels.
    rect: [f32; 4],
}

/// A new tab on a blank page; Chrome starts with the first one.
pub fn open(app: &mut App) -> TabId {
    let tab = open_tab(app);
    register(app, tab);
    tab
}

fn open_tab(app: &mut App) -> TabId {
    let waker = app.ui.panel.waker.clone();
    let ui = &mut app.ui.panel.browser;
    if !ui.browser.started() {
        if let Some(waker) = waker {
            ui.browser.start(waker);
        }
    }
    ui.next += 1;
    let tab = ui.next;
    let pixels = Pixels::new();
    ui.tabs.insert(tab, Tab::new(pixels.clone()));
    ui.browser.send(Cmd::Open(tab, pixels));
    // `browserDefaultZoomFactor`: "Page zoom applied to new browser tabs."
    let zoom = crate::settings::value(app, "browserDefaultZoomFactor").as_f64().unwrap_or(1.) as f32;
    if let Some(&zoom) = ZOOM_LEVELS.iter().find(|z| (**z - zoom).abs() < 0.001).filter(|z| **z != 1.) {
        app.ui.panel.browser.browser.send(Cmd::Zoom(tab, zoom));
    }
    tab
}

/// Imports a browser profile's cookies into the Default profile. Chrome starts for it when no
/// tab has, at the window's scale, since the headless shell keeps the scale it starts with.
pub fn import(app: &mut App, scale: f32, source: &'static browser_import::Source, directory: String) -> Option<browser_import::Job> {
    let waker = app.ui.panel.waker.clone()?;
    let ui = &mut app.ui.panel.browser;
    if !ui.browser.started() {
        ui.browser.start(waker.clone());
        ui.browser.send(Cmd::Viewport { w: 0., h: 0., scale });
        ui.chrome_scale = Some(scale);
    }
    Some(browser_import::start(source, directory, ui.browser.sender()?, waker))
}

/// "Try again" after Chrome failed: a fresh worker, with the open tabs back on blank pages.
fn restart(app: &mut App) {
    let Some(waker) = app.ui.panel.waker.clone() else { return };
    let ui = &mut app.ui.panel.browser;
    ui.browser = browser::Browser::default();
    ui.browser.start(waker);
    ui.restarts += 1;
    ui.chrome_scale = None;
    // The new worker hasn't been told which tab shows or is tracked; poll resends both.
    ui.shown = None;
    ui.tracked = None;
    let tabs: Vec<TabId> = ui.tabs.iter().filter(|(_, t)| t.stream.is_none()).map(|(id, _)| *id).collect();
    for tab in tabs {
        let pixels = Pixels::new();
        let mut fresh = Tab::new(pixels.clone());
        fresh.session = ui.tabs.get(&tab).and_then(|t| t.session.clone());
        ui.tabs.insert(tab, fresh);
        ui.browser.send(Cmd::Open(tab, pixels));
    }
}

/// The headless shell paints at the scale it launched with, so a window that moved to a display
/// of another scale gets a new Chrome at that scale. Each tab keeps its frame on screen and opens
/// its page again, at its zoom and device size; an agent's session re-binds to the new page.
fn relaunch(app: &mut App, scale: f32) {
    let Some(waker) = app.ui.panel.waker.clone() else { return };
    let before = app.ui.panel.browser.browser.shared.lock().unwrap().tabs.clone();
    browser::shutdown();
    let ui = &mut app.ui.panel.browser;
    ui.browser = browser::Browser::default();
    ui.browser.start(waker);
    ui.chrome_scale = Some(scale);
    ui.shown = None;
    ui.tracked = None;
    let mut tabs: Vec<(TabId, Pixels)> = ui.tabs.iter().filter(|(_, t)| t.stream.is_none()).map(|(id, t)| (*id, t.pixels.clone())).collect();
    tabs.sort_by_key(|(id, _)| *id);
    // The address bars keep their pages while the new Chrome starts.
    ui.browser.shared.lock().unwrap().tabs = tabs.iter().filter_map(|(id, _)| Some((*id, Status { loading: true, ..before.get(id)?.clone() }))).collect();
    for (tab, pixels) in tabs {
        ui.browser.send(Cmd::Open(tab, pixels));
        for cmd in before.get(&tab).map(|status| reopen(tab, status)).unwrap_or_default() {
            ui.browser.send(cmd);
        }
    }
}

/// What a tab needs to show its page again in a new Chrome.
fn reopen(tab: TabId, status: &Status) -> Vec<Cmd> {
    let mut out = vec![];
    match &status.failure {
        Some(failure) => out.push(Cmd::Navigate(tab, failure.url.clone())),
        None if !idle(status) => out.push(Cmd::Navigate(tab, status.url.clone())),
        None => {}
    }
    if (status.zoom - 1.).abs() > 0.001 {
        out.push(Cmd::Zoom(tab, status.zoom));
    }
    if status.device.is_some() {
        out.push(Cmd::Device(tab, status.device));
    }
    if status.muted {
        out.push(Cmd::Mute(tab, true));
    }
    out
}

/// Chrome could not start or its connection dropped, so an import's writes never land.
pub fn browser_failed(app: &App) -> bool {
    app.ui.panel.browser.browser.shared.lock().unwrap().fatal.is_some()
}

/// A window the page opened (`window.open`, `target=_blank`), shown as a tab, as web does.
/// One the agent's page opened goes to the agent: Playwright sees it open, the server adopts it
/// as the agent's own hidden tab (`adoptPopup`), and that session binds here.
fn adopt(app: &mut App, target: String, opener: String, url: String) {
    let ui = &mut app.ui.panel.browser;
    let opener_key = ui.browser.shared.lock().unwrap().tabs.iter().find(|(_, s)| s.target.as_deref() == Some(opener.as_str())).map(|(id, _)| *id);
    let opener_key = opener_key.and_then(|id| ui.tabs.get(&id)?.session.clone()).filter(agent::driven);
    ui.next += 1;
    let tab = ui.next;
    let pixels = Pixels::new();
    let mut fresh = Tab::new(pixels.clone());
    fresh.adopting = opener_key.clone().map(|key| (key, target.clone()));
    ui.tabs.insert(tab, fresh);
    ui.browser.send(Cmd::Adopt(tab, pixels, target.clone()));
    match opener_key {
        Some(key) => agent::popup(&key, target, url),
        None => {
            super::activate(app, super::Surface::Browser(tab));
            register(app, tab);
        }
    }
}

pub fn close(app: &mut App, tab: TabId) {
    if let Some((opener, target)) = app.ui.panel.browser.tabs.get(&tab).and_then(|t| t.adopting.clone()) {
        agent::popup_gone(&opener, target);
    }
    if let Some(key) = app.ui.panel.browser.tabs.get(&tab).and_then(|t| t.session.clone()) {
        agent::detach(&key);
        close_session(app, &key);
    }
    let ui = &mut app.ui.panel.browser;
    ui.tabs.remove(&tab);
    ui.browser.send(Cmd::Close(tab));
    if ui.shown == Some(tab) {
        ui.shown = None;
    }
    if ui.pending.as_ref().is_some_and(|p| p.tab == tab) {
        ui.pending = None;
    }
    if ui.pip == Some(tab) {
        ui.pip = None;
    }
    if ui.dialog.as_ref().is_some_and(|d| d.tab == tab) {
        ui.dialog = None;
    }
    if ui.popout == Some(tab) {
        ui.popout = None;
        popout::close();
    }
}

/// `"Browser"` until the tab loads something, then the page title, else its host.
pub fn title(app: &App, tab: TabId) -> String {
    let status = app.ui.panel.browser.browser.status(tab);
    if idle(&status) {
        return "Browser".into();
    }
    let title = status.title.trim();
    if !title.is_empty() && title != status.url {
        return title.into();
    }
    host(&status.url).unwrap_or("Browser").into()
}

/// A playing tab's speaker: `Some(muted)`, or none while it is silent (web's `tabAudioState`).
pub fn audio(app: &App, tab: TabId) -> Option<bool> {
    let status = app.ui.panel.browser.browser.status(tab);
    status.audible.then_some(status.muted)
}

pub fn muted(app: &App, tab: TabId) -> bool {
    app.ui.panel.browser.browser.status(tab).muted
}

pub fn set_muted(app: &mut App, tab: TabId, on: bool) {
    send(app, Cmd::Mute(tab, on));
}

/// The page a tab shows, unless it is blank.
pub fn url(app: &App, tab: TabId) -> Option<String> {
    let status = app.ui.panel.browser.browser.status(tab);
    (!idle(&status)).then(|| status.url.clone())
}

fn idle(s: &Status) -> bool {
    s.url.is_empty() || s.url == "about:blank"
}

fn host(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    (!host.is_empty()).then_some(host)
}

fn bar(app: &mut App, tab: TabId) -> &mut TextEdit {
    &mut app.ui.panel.browser.tabs.entry(tab).or_insert_with(|| Tab::new(Pixels::new())).bar
}

/// The address bar `workspace.focusNext` lands on in a browser surface.
pub(super) fn bar_focused(app: &App, tab: TabId) -> bool {
    app.ui.panel.browser.tabs.get(&tab).is_some_and(|t| t.bar.focused)
}
pub(super) fn focus_bar(app: &mut App, tab: Option<TabId>) {
    for (id, t) in &mut app.ui.panel.browser.tabs {
        t.bar.focused = Some(*id) == tab;
    }
    if let Some(tab) = tab {
        bar(app, tab).focused = true;
    }
}

fn device(app: &mut App, tab: TabId) -> &mut Device {
    &mut app.ui.panel.browser.tabs.entry(tab).or_insert_with(|| Tab::new(Pixels::new())).device
}

fn send(app: &mut App, cmd: Cmd) {
    let human = match &cmd {
        Cmd::Mouse { kind, .. } => *kind != "mouseMoved",
        Cmd::Wheel { .. } | Cmd::Text(_) | Cmd::Key(_) => true,
        _ => false,
    };
    if let Some(tab) = app.ui.panel.browser.shown.filter(|_| human) {
        app.ui.panel.browser.human = Some((tab, std::time::Instant::now()));
    }
    if let Cmd::Viewport { scale, .. } = &cmd {
        let (started, launched) = (app.ui.panel.browser.browser.started(), app.ui.panel.browser.chrome_scale);
        match launched {
            _ if !started => {}
            Some(launched) if (launched - scale).abs() > 0.01 => relaunch(app, *scale),
            Some(_) => {}
            None => app.ui.panel.browser.chrome_scale = Some(*scale),
        }
    }
    let ui = &app.ui.panel.browser;
    let tab = match &cmd {
        Cmd::Mouse { .. } | Cmd::Wheel { .. } | Cmd::Text(_) | Cmd::Key(_) | Cmd::Inspect { .. } | Cmd::CopySelection { .. } | Cmd::Edit(_) => ui.shown,
        Cmd::Navigate(tab, _) | Cmd::Back(tab) | Cmd::Forward(tab) | Cmd::Reload { tab, .. } | Cmd::Stop(tab) | Cmd::Zoom(tab, _) | Cmd::Device(tab, _) | Cmd::Pick(tab, _) | Cmd::DevTools(tab) | Cmd::Screenshot(tab, _) | Cmd::Record { tab, .. } | Cmd::Dialog { tab, .. } | Cmd::Mute(tab, _) => Some(*tab),
        _ => None,
    };
    // A streamed tab's page is the server's; what it can't do there does nothing.
    if let Some((tab, stream)) = tab.and_then(|id| Some((id, ui.tabs.get(&id)?.stream.as_ref()?))) {
        stream.command(&cmd, stream_box(ui, tab).2);
        return;
    }
    if let Cmd::Viewport { w, h, .. } = &cmd {
        for stream in ui.tabs.values().filter_map(|t| t.stream.as_ref()) {
            stream.resize(*w, *h);
        }
    }
    app.ui.panel.browser.browser.send(cmd);
}

/// A streamed page's box in the page area and its scale from CSS pixels, as a device size's.
fn stream_box(ui: &Ui, tab: TabId) -> (f32, f32, f32) {
    let (w, h) = (ui.area.0.max(1.), ui.area.1.max(1.));
    let size = ui.tabs.get(&tab).and_then(|t| t.stream.as_ref()?.state().viewport).unwrap_or((w as u32, h as u32));
    browser::device_box(w, h, size, 1.)
}

pub fn navigate(app: &mut App, tab: TabId, input: &str) {
    let engine = crate::settings::value(app, "browserSearchEngine");
    let custom = crate::settings::value(app, "browserCustomSearchUrl");
    let template = search_template(engine.as_str().unwrap_or(""), custom.as_str().unwrap_or(""));
    match resolve(input, template) {
        Some(url) => {
            bar(app, tab).replace_text(url.clone());
            super::history::record(app, &url);
            if let Some(t) = app.ui.panel.browser.tabs.get_mut(&tab) {
                t.titled = Default::default();
            }
            send(app, Cmd::Navigate(tab, url));
        }
        None => crate::shell::palette::toast(app, "Could not open this address", Some("Enter an http or https URL.")),
    }
}

fn search_template<'a>(engine: &str, custom: &'a str) -> &'a str {
    match engine {
        "google" => "https://www.google.com/search?q=%s",
        "bing" => "https://www.bing.com/search?q=%s",
        "brave" => "https://search.brave.com/search?q=%s",
        "kagi" => "https://kagi.com/search?q=%s",
        "ecosia" => "https://www.ecosia.org/search?q=%s",
        "custom" if valid_template(custom.trim()) => custom.trim(),
        _ => "https://duckduckgo.com/?q=%s",
    }
}

/// `%s` must sit after the authority, or the query would pick the host.
fn valid_template(template: &str) -> bool {
    let Some((scheme, rest)) = template.split_once("://") else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") && template.contains("%s") && !authority.contains("%s") && !authority.is_empty()
}

/// `resolveAddressBarInput`: a URL, a bare host (`localhost:5173`, `example.com`), or a search.
fn resolve(input: &str, template: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if scheme_with_authority(s) {
        return normalize(s);
    }
    if !s.contains(char::is_whitespace) {
        if let Some(protocol) = protocol(s) {
            if NON_WEB_SCHEMES.contains(&protocol.as_str()) || !bare_host_port(s) {
                return if protocol == "http" || protocol == "https" { normalize(s) } else { None };
            }
        }
        let authority = s.split(['/', '?', '#']).next().unwrap_or("");
        let authority = authority.split_once('@').map_or(authority, |(_, h)| h);
        let host = strip_port(authority);
        let dotted = host.trim_end_matches('.').split('.').count() > 1 && host.trim_end_matches('.').split('.').all(|p| !p.is_empty());
        if host.len() < authority.len() || address_host(host) || dotted {
            if let Some(url) = normalize(s) {
                return Some(url);
            }
        }
    }
    Some(template.replace("%s", &encode_component(s)))
}

fn protocol(s: &str) -> Option<String> {
    let (scheme, _) = s.split_once(':')?;
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
    valid.then(|| scheme.to_ascii_lowercase())
}

fn scheme_with_authority(s: &str) -> bool {
    protocol(s).is_some_and(|p| s[p.len()..].starts_with("://"))
}

fn bare_host_port(s: &str) -> bool {
    let Some((host, rest)) = s.split_once(':') else { return false };
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    !host.is_empty() && !host.contains(['/', '?', '#', '@']) && digits > 0 && rest[digits..].chars().next().is_none_or(|c| matches!(c, '/' | '?' | '#'))
}

fn strip_port(authority: &str) -> &str {
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) && !host.ends_with(':') => host,
        _ => authority,
    }
}

fn address_host(host: &str) -> bool {
    let ipv4 = host.split('.').count() == 4 && host.split('.').all(|p| (1..=3).contains(&p.len()) && p.chars().all(|c| c.is_ascii_digit()));
    let ipv6 = host.len() > 2 && host.starts_with('[') && host.ends_with(']') && host[1..host.len() - 1].chars().all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '.'));
    host.eq_ignore_ascii_case("localhost") || ipv4 || ipv6
}

/// `normalizePreviewUrl`: http(s) only, `http` for loopback hosts, a path of at least `/`.
fn normalize(s: &str) -> Option<String> {
    if protocol(s).is_some_and(|p| NON_WEB_SCHEMES.contains(&p.as_str())) {
        return None;
    }
    let candidate = if scheme_with_authority(s) {
        s.to_string()
    } else {
        let lower = s.to_ascii_lowercase();
        let loopback = ["localhost", "127.0.0.1", "0.0.0.0", "[::1]", "[::]"]
            .iter()
            .any(|h| lower.strip_prefix(h).is_some_and(|rest| rest.is_empty() || rest.starts_with([':', '/'])));
        format!("{}://{s}", if loopback { "http" } else { "https" })
    };
    let (scheme, rest) = candidate.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let host = strip_port(authority.split_once('@').map_or(authority, |(_, h)| h));
    if host.is_empty() || authority.contains(char::is_whitespace) {
        return None;
    }
    let tail = if tail.starts_with('/') { tail.to_string() } else { format!("/{tail}") };
    Some(format!("{scheme}://{}{tail}", authority.to_ascii_lowercase()))
}

fn encode_component(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Chrome's `net::ERR_*` names, as web's `errorCodeMessages`.
fn friendly(description: &str) -> &str {
    match description {
        "ERR_NAME_NOT_RESOLVED" | "ERR_NAME_RESOLUTION_FAILED" => "DNS address could not be found",
        "ERR_CONNECTION_REFUSED" => "Connection refused",
        "ERR_CONNECTION_RESET" => "Connection was reset",
        "ERR_CONNECTION_CLOSED" => "Connection was closed",
        "ERR_CONNECTION_TIMED_OUT" | "ERR_TIMED_OUT" => "Connection timed out",
        "ERR_INTERNET_DISCONNECTED" => "No internet connection",
        "ERR_CERT_AUTHORITY_INVALID" => "Certificate authority is not trusted",
        "ERR_CERT_COMMON_NAME_INVALID" => "Certificate hostname mismatch",
        "ERR_CERT_DATE_INVALID" => "Certificate is expired or not yet valid",
        "ERR_TOO_MANY_REDIRECTS" => "Too many redirects",
        "" => "Network error",
        other => other,
    }
}

fn panel_tab(app: &App) -> Option<TabId> {
    match app.ui.panel.active {
        Some(super::Surface::Browser(tab)) if app.ui.shell.panel_open => Some(tab),
        _ => None,
    }
}

/// The floating tab floats over the chat, so a thread's or the new-thread draft's.
fn floats_here(app: &App) -> bool {
    matches!(app.frame_route(), Route::Thread(_) | Route::Home { .. })
}

/// The floating tab, unless the panel shows that same tab.
fn pip_tab(app: &App) -> Option<TabId> {
    let tab = app.ui.panel.browser.pip?;
    (floats_here(app) && panel_tab(app) != Some(tab) && app.ui.panel.browser.tabs.contains_key(&tab)).then_some(tab)
}

/// The one tab Chrome screencasts: the panel's, else the floating one, else the separate
/// window's.
fn shown(app: &App) -> Option<TabId> {
    panel_tab(app).or_else(|| pip_tab(app)).or_else(|| app.ui.panel.browser.popout.filter(|_| popout::visible()))
}

pub fn poll(app: &mut App, tx: &mut Tx) -> bool {
    let synced = sync_sessions(app);
    let synced = sync_streams(app) || synced;
    let wait = cursor_wake(app);
    if let Some(wait) = wait {
        tx.wake_after(wait + Duration::from_millis(16));
    }
    let newest = app.ui.panel.browser.tabs.values().filter_map(tab_pointer).map(|p| p.sequence).max().unwrap_or(0);
    let cursor = (newest, wait.is_some());
    let synced = synced || std::mem::replace(&mut app.ui.panel.browser.cursor_seen, cursor) != cursor;
    let want = shown(app);
    let live = matches!(app.src, data::Source::Live(_));
    let watch = live && !app.ui.panel.browser.tabs.is_empty();
    if watch != app.ui.panel.browser.watching {
        app.ui.panel.browser.watching = watch;
        if let Some(net) = app.src.net() {
            if watch {
                net.watch_resource(SERVERS, json!({}));
            } else {
                net.unwatch_resource(SERVERS, json!({}));
            }
        }
    }
    let now = data::now_ms();
    if !app.ui.panel.browser.tabs.is_empty() {
        super::history::ensure_loaded(app);
    }
    let ui = &mut app.ui.panel.browser;
    if ui.shown != want {
        ui.shown = want;
        ui.browser.send(Cmd::Show(want));
    }
    let track = ui.pending.as_ref().map(|p| p.tab);
    if ui.tracked != track {
        ui.tracked = track;
        ui.browser.send(Cmd::Track(track));
    }
    let (generation, events, statuses) = {
        let mut shared = ui.browser.shared.lock().unwrap();
        (shared.generation, std::mem::take(&mut shared.events), shared.tabs.clone())
    };
    let mut changed = generation != ui.seen || !events.is_empty();
    ui.seen = generation;
    let mut settled = vec![];
    let mut found = vec![];
    let mut loaded = vec![];
    for (id, tab) in ui.tabs.iter_mut() {
        let status = statuses.get(id).cloned().unwrap_or_default();
        // Navigation clears the icon, so each load reports it afresh.
        if status.favicon != tab.icon {
            tab.icon = status.favicon.clone();
            found.extend(status.favicon.clone().map(|icon| (status.url.clone(), icon)));
        }
        if !idle(&status) && !status.loading && status.failure.is_none() {
            loaded.push(status.url.clone());
        }
        let nav = (status.url.clone(), status.loading);
        if tab.nav != nav {
            if navigated(&tab.nav, &status) {
                match &tab.stream {
                    Some(stream) => stream.clear_pointer(),
                    None => {
                        if let Some(key) = &tab.session {
                            agent::clear_pointer(key);
                        }
                    }
                }
                changed = true;
            }
            tab.nav = nav;
        }
        if tab.bar.focused && !tab.focused {
            tab.bar.set_selection(0, tab.bar.text.len());
            changed = true;
        }
        tab.focused = tab.bar.focused;
        let live_url = if idle(&status) { "" } else { status.url.as_str() };
        if !tab.bar.focused && tab.bar.text != live_url {
            tab.bar.replace_text(live_url);
            changed = true;
        }
        if !idle(&status) && status.failure.is_none() && !status.loading && !status.title.trim().is_empty() && status.title != status.url && tab.titled != (status.url.clone(), status.title.clone()) {
            tab.titled = (status.url.clone(), status.title.clone());
            settled.push((*id, tab.titled.clone()));
        }
    }
    if ui.zoom_until != 0 {
        if now >= ui.zoom_until {
            ui.zoom_until = 0;
            changed = true;
        } else {
            tx.wake_after(Duration::from_millis((ui.zoom_until - now) as u64));
        }
    }
    for (page, url) in found {
        super::favicon::note(app, &page, &url);
    }
    for page in loaded {
        super::favicon::want_fallback(app, &page);
    }
    changed |= super::favicon::poll(app);
    for (id, (url, title)) in settled {
        if app.ui.panel.surfaces.contains(&super::Surface::Browser(id)) || app.ui.panel.browser.pip == Some(id) {
            super::history::set_title(app, &url, &title);
        }
    }
    for event in events {
        match event {
            browser::Event::Screenshot(Ok(path)) => screenshot_saved(app, path),
            browser::Event::Screenshot(Err(e)) => crate::shell::palette::toast(app, "Unable to capture screenshot", Some(&e)),
            browser::Event::Popup { target, opener, url } => adopt(app, target, opener, url),
            browser::Event::Picked { tab, element, image } => pick(app, tab, element, image),
            browser::Event::DevTools(Ok(url)) => {
                if let Err(e) = tx.open_url(&url) {
                    crate::shell::palette::toast(app, "Unable to open DevTools", Some(&e.to_string()));
                }
            }
            browser::Event::DevTools(Err(e)) => crate::shell::palette::toast(app, "Unable to open DevTools", Some(&e)),
            browser::Event::Recorded(Ok(path)) => recording_saved(app, path),
            browser::Event::Recorded(Err(e)) => crate::shell::palette::toast(app, "Unable to stop recording", Some(&e)),
            browser::Event::Dialog { tab, kind, message, default } => {
                let mut input = TextEdit::new(default.as_str());
                input.focused = kind == "prompt";
                input.set_selection(0, input.text.len());
                app.ui.panel.browser.dialog = Some(Dialog { tab, kind, message, input });
            }
            browser::Event::DialogClosed(tab) => {
                if app.ui.panel.browser.dialog.as_ref().is_some_and(|d| d.tab == tab) {
                    app.ui.panel.browser.dialog = None;
                }
            }
            browser::Event::Context { tab, at, info } => app.ui.panel.browser.context = Some(Context { tab, at, info }),
            browser::Event::Clipboard(text) => tx.copy(text),
            browser::Event::Image(png) => match rune::ClipboardImage::from_png(png) {
                Some(png) => tx.copy_rich(rune::ClipboardContent::new("").with_image(png)),
                None => crate::shell::palette::toast(app, "Unable to copy image", None),
            },
            // Electron moves a person's download to Downloads and shows it in its folder.
            browser::Event::Download(Ok(path)) => {
                if let Err(e) = reveal(&path) {
                    crate::shell::palette::toast(app, "Unable to show the download", Some(&e));
                }
            }
            browser::Event::Download(Err(e)) => crate::shell::palette::toast(app, "Unable to save the download", Some(&e)),
        }
    }
    if let Some(tab) = app.ui.panel.browser.popout {
        if !popout::visible() {
            if app.ui.panel.browser.popout_seen {
                app.ui.panel.browser.popout = None;
                changed = true;
            }
        } else {
            app.ui.panel.browser.popout_seen = true;
            let title = popout_title(app, tab);
            if app.ui.panel.browser.popout_title != title {
                app.ui.panel.browser.popout_title = title.clone();
                popout::retitle(title);
            }
        }
    }
    changed || synced
}

const SESSIONS: &str = "subscribePreviewEvents";

/// A tab opened on one of this machine's threads becomes a server preview session, as
/// Electron's are, so the thread's agent can drive it too.
fn register(app: &mut App, tab: TabId) {
    let Route::Thread(thread) = app.frame_route().clone() else { return };
    register_on(app, tab, thread);
}

fn register_on(app: &mut App, tab: TabId, thread: String) {
    if !agent::available() || crate::env::owner(app, &thread) != crate::env::LOCAL {
        return;
    }
    let Some(net) = crate::env::local_mut(app).net() else { return };
    let id = net.request("preview.open", json!({ "threadId": thread, "viewport": { "_tag": "fill" }, "runtime": "server" }));
    app.ui.panel.browser.opening.push((id, tab, thread));
}

fn close_session(app: &mut App, key: &agent::Key) {
    let env = crate::env::owner(app, &key.0);
    if let Some(net) = env_net(app, &env) {
        net.request("preview.close", json!({ "threadId": key.0, "tabId": key.1 }));
    }
}

fn env_net<'a>(app: &'a mut App, env: &str) -> Option<&'a mut s5_net::Net> {
    if env == crate::env::LOCAL { crate::env::local_mut(app).net() } else { crate::env::net_of(app, env) }
}

/// Every other environment's server sessions, which the panel shows as the server's frames,
/// as web's `ServerBrowserSurface` does for a server it doesn't run beside.
fn sync_remote(app: &mut App) -> bool {
    let mut changed = false;
    for env in crate::env::keys(app).into_iter().filter(|k| k != crate::env::LOCAL) {
        let want = matches!(crate::env::source(app, &env), Some(data::Source::Live(_)));
        let watched = app.ui.panel.browser.remote_watched.contains(&env);
        if want != watched {
            if let Some(net) = crate::env::net_of(app, &env) {
                if want {
                    net.watch_resource(SESSIONS, json!({}));
                } else {
                    net.unwatch_resource(SESSIONS, json!({}));
                }
            }
            let list = &mut app.ui.panel.browser.remote_watched;
            if want { list.push(env.clone()) } else { list.retain(|k| *k != env) }
        }
        if !want {
            continue;
        }
        let events = crate::env::net_of(app, &env).map(|n| n.take_resource_events(SESSIONS, json!({}))).unwrap_or_default();
        changed |= !events.is_empty();
        for event in events {
            session_event(app, &env, event);
        }
    }
    changed
}

/// A streamed tab's address bar, history buttons and failure, from its session and stream.
fn stream_status(snapshot: &Value, state: &super::stream::State) -> Status {
    let nav = &snapshot["navStatus"];
    let url = nav["url"].as_str().unwrap_or_default().to_owned();
    let failure = state.error.clone().filter(|_| state.ended).map(|description| browser::Failure { url: url.clone(), description });
    Status {
        title: nav["title"].as_str().unwrap_or_default().to_owned(),
        loading: nav["_tag"] == "Loading",
        can_back: snapshot["canGoBack"] == true,
        can_forward: snapshot["canGoForward"] == true,
        painted: state.painted,
        failure,
        url,
        ..Status::default()
    }
}

fn sync_streams(app: &mut App) -> bool {
    let ui = &app.ui.panel.browser;
    let mut changed = false;
    let mut gone = vec![];
    {
        let mut shared = ui.browser.shared.lock().unwrap();
        for (id, tab) in &ui.tabs {
            let Some(stream) = &tab.stream else { continue };
            let state = stream.state();
            // The server no longer has the session (it closed, or the server restarted).
            if state.ended && state.error.is_none() {
                gone.push(*id);
                continue;
            }
            let status = stream_status(&tab.snapshot, &state);
            if shared.tabs.get(id) != Some(&status) {
                shared.tabs.insert(*id, status);
                changed = true;
            }
        }
    }
    for tab in gone {
        if let Some(t) = app.ui.panel.browser.tabs.get_mut(&tab) {
            t.session = None;
        }
        super::drop_browser(app, tab);
        close(app, tab);
        changed = true;
    }
    changed
}

/// A server session as a tab showing the server's frames.
fn open_stream(app: &mut App, env: &str, key: agent::Key, snapshot: &Value) -> Option<TabId> {
    let waker = app.ui.panel.waker.clone()?;
    let net = env_net(app, env)?;
    let (base, bearer) = (net.base_url().to_owned(), net.bearer().to_owned());
    let ui = &mut app.ui.panel.browser;
    ui.next += 1;
    let tab = ui.next;
    let pixels = Pixels::new();
    let mut fresh = Tab::new(pixels.clone());
    let stream = super::stream::Stream::open(base, bearer, &key, pixels, waker);
    if ui.area.0 >= 1. {
        stream.resize(ui.area.0, ui.area.1);
    }
    fresh.stream = Some(stream);
    fresh.session = Some(key);
    fresh.snapshot = snapshot.clone();
    ui.tabs.insert(tab, fresh);
    Some(tab)
}

/// Follows the local server's preview sessions as web's `followPreviewSessions` does: a session
/// an agent or another client opens becomes a tab on its thread's panel, a closed one closes,
/// and each tab with a page is announced to the server, which then drives it.
fn sync_sessions(app: &mut App) -> bool {
    sync_remote(app) | sync_local(app)
}

fn sync_local(app: &mut App) -> bool {
    let want = agent::available() && matches!(crate::env::local_mut(app), data::Source::Live(_));
    if want != app.ui.panel.browser.sessions_watched {
        app.ui.panel.browser.sessions_watched = want;
        if let Some(net) = crate::env::local_mut(app).net() {
            if want {
                net.watch_resource(SESSIONS, json!({}));
            } else {
                net.unwatch_resource(SESSIONS, json!({}));
            }
        }
    }
    if !want {
        return false;
    }
    if let Some(waker) = app.ui.panel.waker.clone() {
        agent::set_waker(waker);
    }
    let generation = agent::generation();
    if app.ui.panel.browser.generation != generation {
        app.ui.panel.browser.generation = generation;
        app.ui.panel.browser.rebind = true;
    }
    let open = matches!(crate::env::local_mut(app), data::Source::Live(net) if net.state().conn == s5_net::Conn::Open);
    if app.ui.panel.browser.rebind && open {
        app.ui.panel.browser.rebind = false;
        rebind(app);
    }
    let ids: Vec<u64> = app.ui.panel.browser.opening.iter().map(|o| o.0).collect();
    let Some(net) = crate::env::local_mut(app).net() else { return false };
    let mut events = net.take_resource_events(SESSIONS, json!({}));
    let replies: Vec<(u64, s5_net::Reply)> = ids.into_iter().filter_map(|id| net.take_reply(id).map(|r| (id, r))).collect();
    let changed = !events.is_empty() || !replies.is_empty();
    for (id, reply) in replies {
        let ui = &mut app.ui.panel.browser;
        let Some(i) = ui.opening.iter().position(|o| o.0 == id) else { continue };
        let (_, tab, thread) = ui.opening.remove(i);
        if let Some(session) = reply.ok().and_then(|snapshot| snapshot["tabId"].as_str().map(str::to_owned)) {
            bind(app, tab, (thread, session));
        }
    }
    let mut pending = std::mem::take(&mut app.ui.panel.browser.deferred);
    pending.append(&mut events);
    for event in pending {
        session_event(app, crate::env::LOCAL, event);
    }
    announce(app);
    changed
}

/// A new server process knows none of the old sessions; each tab that had one takes a new one
/// on its thread, as a tab the person opened.
fn rebind(app: &mut App) {
    let ui = &mut app.ui.panel.browser;
    let mut stale = vec![];
    // A streamed tab follows its own server: it ends when the session does.
    for (id, t) in ui.tabs.iter_mut().filter(|(_, t)| t.stream.is_none()) {
        if let Some((thread, _)) = t.session.take() {
            t.announced = None;
            t.adopting = None;
            stale.push((*id, thread));
        }
    }
    ui.opening.clear();
    ui.deferred.clear();
    stale.sort();
    for (tab, thread) in stale {
        register_on(app, tab, thread);
    }
}

fn bind(app: &mut App, tab: TabId, key: agent::Key) {
    let ui = &mut app.ui.panel.browser;
    if ui.tabs.values().any(|t| t.session.as_ref() == Some(&key)) {
        return;
    }
    if let Some(t) = ui.tabs.get_mut(&tab) {
        if t.session.is_none() {
            t.session = Some(key);
        }
        return;
    }
    // Closed before the server answered.
    close_session(app, &key);
}

fn session_event(app: &mut App, env: &str, event: Value) {
    let thread = event["threadId"].as_str().unwrap_or_default().to_owned();
    let key = (thread.clone(), event["tabId"].as_str().unwrap_or_default().to_owned());
    if key.0.is_empty() || key.1.is_empty() {
        return;
    }
    let bound = app.ui.panel.browser.tabs.iter().find(|(_, t)| t.session.as_ref() == Some(&key)).map(|(id, _)| *id);
    let snapshot = &event["snapshot"];
    match event["type"].as_str() {
        Some("opened") => {
            if bound.is_some() || snapshot["runtime"] != "server" {
                return;
            }
            // Web's `rendersServerTabNatively`: only this machine's server drives a page here;
            // another's, or a headless one, is the server's own and streams.
            if env != crate::env::LOCAL || snapshot["headless"] == true {
                if let Some(tab) = open_stream(app, env, key, snapshot) {
                    reveal_session(app, tab, &thread, snapshot, true);
                }
                return;
            }
            // Likely this client's own tab, still waiting for its `preview.open` reply.
            if snapshot["automationOwner"].is_null() && app.ui.panel.browser.opening.iter().any(|o| o.2 == thread) {
                app.ui.panel.browser.deferred.push(event);
                return;
            }
            // The server adopting a popup the agent's page opened, which is already a tab here.
            if !snapshot["automationOwner"].is_null() {
                let mut adopting: Vec<TabId> = app.ui.panel.browser.tabs.iter().filter(|(_, t)| t.session.is_none() && t.adopting.as_ref().is_some_and(|(k, _)| k.0 == thread)).map(|(id, _)| *id).collect();
                adopting.sort();
                if let Some(&tab) = adopting.first() {
                    if let Some(t) = app.ui.panel.browser.tabs.get_mut(&tab) {
                        t.session = Some(key);
                    }
                    reveal_session(app, tab, &thread, snapshot, true);
                    return;
                }
            }
            let tab = open_tab(app);
            if let Some(t) = app.ui.panel.browser.tabs.get_mut(&tab) {
                t.session = Some(key);
            }
            apply(app, tab, snapshot, &Value::Null);
            reveal_session(app, tab, &thread, snapshot, true);
        }
        Some("closed") => {
            let Some(tab) = bound else { return };
            if let Some(t) = app.ui.panel.browser.tabs.get_mut(&tab) {
                t.session = None;
            }
            agent::detach(&key);
            super::drop_browser(app, tab);
            close(app, tab);
        }
        Some("resized") => {
            let Some(tab) = bound else { return };
            if !keep_snapshot(app, tab, snapshot) {
                apply(app, tab, snapshot, &event["request"]);
            }
            reveal_session(app, tab, &thread, snapshot, false);
        }
        Some("navigated") => {
            if let Some(tab) = bound {
                keep_snapshot(app, tab, snapshot);
                reveal_session(app, tab, &thread, snapshot, false);
            }
        }
        _ => {}
    }
}

/// Records a streamed tab's snapshot; whether the tab is streamed, so the server sizes its page.
fn keep_snapshot(app: &mut App, tab: TabId, snapshot: &Value) -> bool {
    let Some(t) = app.ui.panel.browser.tabs.get_mut(&tab).filter(|t| t.stream.is_some()) else { return false };
    t.snapshot = snapshot.clone();
    true
}

/// Web's `revealServerTabs`: every session but a hidden one is a tab on its thread's panel; an
/// agent's reveal floats a new one over the chat, and a forced one brings its tab forward.
fn reveal_session(app: &mut App, tab: TabId, thread: &str, snapshot: &Value, opened: bool) {
    if snapshot["reveal"] == false {
        return;
    }
    let request = snapshot["revealRequest"]["id"].as_str().map(str::to_owned);
    let Some(t) = app.ui.panel.browser.tabs.get_mut(&tab) else { return };
    let fresh = match &request {
        Some(id) => t.revealed.as_deref() != Some(id.as_str()),
        None => opened,
    };
    if request.is_some() {
        t.revealed = request;
    }
    let force = snapshot["revealRequest"]["force"] == true;
    let wanted = fresh && snapshot["reveal"] == true && (force || crate::settings::value(app, "browserAutoShowFloatingPreview").as_bool().unwrap_or(true));
    let current = super::add_browser(app, thread, tab, wanted && opened);
    if wanted && !opened && force && current {
        super::activate(app, super::Surface::Browser(tab));
    }
}

/// What an agent's `preview_resize` and `preview_set_appearance` ask of a tab the server leaves
/// to the desktop: its viewport, zoom, a hard reload or clearing site data. The server emulates
/// the color scheme itself.
fn apply(app: &mut App, tab: TabId, snapshot: &Value, request: &Value) {
    let status = app.ui.panel.browser.browser.status(tab);
    if let Some(device) = viewport_size(&snapshot["viewport"]).filter(|d| *d != status.device) {
        send(app, Cmd::Device(tab, device));
    }
    if let Some(zoom) = snapshot["zoomFactor"].as_f64().map(|z| z as f32).filter(|z| (status.zoom - z).abs() > 0.001) {
        send(app, Cmd::Zoom(tab, zoom));
    }
    if request["hardReload"] == true {
        send(app, Cmd::Reload { tab, hard: true });
    }
    match request["clear"].as_str() {
        Some("cookies") => send(app, Cmd::ClearCookies),
        Some("cache") => send(app, Cmd::ClearCache),
        _ => {}
    }
}

/// A session's `PreviewViewportSetting` as the device toolbar's size: `Some(None)` fills the
/// panel; `None` when the session names none.
fn viewport_size(viewport: &Value) -> Option<Option<(u32, u32)>> {
    match (viewport["_tag"].as_str()?, viewport["width"].as_u64(), viewport["height"].as_u64()) {
        ("freeform" | "preset", Some(w), Some(h)) => Some(Some((w as u32, h as u32))),
        _ => Some(None),
    }
}

/// Tells the server each session's tab is attached once Chrome has its page, and again for a
/// new server process or the new page after Chrome restarts.
fn announce(app: &mut App) {
    let ui = &mut app.ui.panel.browser;
    if !ui.tabs.values().any(|t| t.session.is_some()) {
        return;
    }
    let generation = agent::generation();
    let (host, targets): (Option<String>, HashMap<TabId, String>) = {
        let shared = ui.browser.shared.lock().unwrap();
        (shared.devtools.clone(), shared.tabs.iter().filter_map(|(id, s)| s.target.clone().map(|t| (*id, t))).collect())
    };
    let Some(host) = host else { return };
    for (id, tab) in ui.tabs.iter_mut() {
        let (Some(key), Some(target)) = (&tab.session, targets.get(id)) else { continue };
        let now = Some((generation, target.clone()));
        if tab.announced != now {
            // Chrome restarted under this server: it drops the dead page before taking the new one.
            if tab.announced.as_ref().is_some_and(|(g, _)| *g == generation) {
                agent::detach(key);
            }
            agent::attach(key, host.clone(), target.clone());
            tab.announced = now;
        }
    }
}

/// Electron's separate window title: `Preview · {title}`, or "Browser preview" for a blank tab.
fn popout_title(app: &App, tab: TabId) -> String {
    if idle(&app.ui.panel.browser.browser.status(tab)) {
        return "Browser preview".into();
    }
    format!("Preview · {}", title(app, tab))
}

/// Opens the separate preview window on the tab, or closes it.
fn toggle_popout(app: &mut App, tab: TabId) {
    if app.ui.panel.browser.popout.is_some() && popout::visible() {
        app.ui.panel.browser.popout = None;
        popout::close();
        return;
    }
    let Some(pixels) = app.ui.panel.browser.tabs.get(&tab).map(|t| t.pixels.clone()) else { return };
    let ui = &mut app.ui.panel.browser;
    ui.popout = Some(tab);
    // The next poll titles the window once it exists.
    ui.popout_title = String::new();
    ui.popout_seen = false;
    popout::open(pixels);
}

fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else if cfg!(windows) {
        "Reveal in File Explorer"
    } else {
        "Reveal in Files"
    }
}

/// Electron's `shell.showItemInFolder`: the file selected in its folder. On Linux that is the
/// file manager's FileManager1 D-Bus call, else the folder opens with its default app.
pub fn reveal(path: &std::path::Path) -> Result<(), String> {
    use std::process::Command;
    if cfg!(target_os = "macos") {
        return Command::new("open").arg("-R").arg(path).spawn().map(drop).map_err(|e| e.to_string());
    }
    if cfg!(windows) {
        return Command::new("explorer").arg(format!("/select,{}", path.display())).spawn().map(drop).map_err(|e| e.to_string());
    }
    let item = format!("array:string:{}", file_uri(path));
    let dir = path.parent().unwrap_or(path).to_path_buf();
    std::thread::spawn(move || {
        let call = ["--session", "--print-reply", "--reply-timeout=3000", "--dest=org.freedesktop.FileManager1", "--type=method_call", "/org/freedesktop/FileManager1", "org.freedesktop.FileManager1.ShowItems"];
        let shown = Command::new("dbus-send").args(call).arg(&item).arg("string:").output().is_ok_and(|o| o.status.success());
        if !shown {
            let _ = Command::new("xdg-open").arg(&dir).status();
        }
    });
    Ok(())
}

/// A `file://` URI with everything but unreserved characters and `/` escaped; dbus-send splits
/// array items on commas, so they must not stay bare.
fn file_uri(path: &std::path::Path) -> String {
    let escaped: String = path
        .display()
        .to_string()
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!("file://{escaped}")
}

fn act(label: &str, primary: bool, f: impl Fn(&mut App, &mut Tx) + 'static) -> crate::shell::palette::ToastAction {
    (label.to_owned(), primary, std::rc::Rc::new(f))
}

/// Web's recording toast: Reveal first, Copy path beside it.
fn recording_saved(app: &mut App, path: std::path::PathBuf) {
    let (shown, revealed) = (path.display().to_string(), path);
    let actions = vec![
        act(reveal_label(), true, move |s, _| {
            if let Err(e) = reveal(&revealed) {
                crate::shell::palette::toast(s, "Unable to reveal recording", Some(&e));
            }
        }),
        act("Copy path", false, move |s, tx| {
            tx.copy(shown.clone());
            crate::shell::palette::toast_copied(s, tx, "Copy path");
        }),
    ];
    crate::shell::palette::toast_with(app, "Recording saved", None, actions);
}

/// The camera: a screenshot, Shift to start recording, and a press while recording stops it.
fn capture(app: &mut App, tab: TabId, record: bool) {
    let status = app.ui.panel.browser.browser.status(tab);
    let artifacts = crate::host::t3_home().join("userdata").join("browser-artifacts");
    if status.recording {
        let stem = artifacts.join(format!("browser-recording-{}", base36(data::now_ms().max(0) as u64)));
        send(app, Cmd::StopRecording(stem));
    } else if record {
        let fps = if crate::settings::value(app, "browserRecordingFrameRate").as_u64() == Some(60) { 60 } else { 30 };
        let mouse = crate::settings::value(app, "browserRecordingShowMousePresses") == true;
        let keys = crate::settings::value(app, "browserRecordingShowKeyPresses") == true;
        send(app, Cmd::Record { tab, fps, mouse, keys });
    } else {
        screenshot(app, tab);
    }
}

/// Answers the page's dialog: OK accepts (with the prompt's text), Cancel dismisses.
fn answer(app: &mut App, accept: bool) {
    let Some(d) = app.ui.panel.browser.dialog.take() else { return };
    let text = if accept && d.kind == "prompt" { d.input.text.clone() } else { String::new() };
    send(app, Cmd::Dialog { tab: d.tab, accept, text });
}

/// Web's screenshot toast: Copy image, Copy path and Reveal.
fn screenshot_saved(app: &mut App, path: std::path::PathBuf) {
    let (image, shown, revealed) = (path.clone(), path.display().to_string(), path);
    let actions = vec![
        act("Copy image", true, move |s, tx| match std::fs::read(&image).ok().and_then(rune::ClipboardImage::from_png) {
            Some(png) => {
                tx.copy_rich(rune::ClipboardContent::new("").with_image(png));
                crate::shell::palette::toast_copied(s, tx, "Copy image");
            }
            None => crate::shell::palette::toast(s, "Unable to copy screenshot", Some("The image could not be read.")),
        }),
        act("Copy path", false, move |s, tx| {
            tx.copy(shown.clone());
            crate::shell::palette::toast_copied(s, tx, "Copy path");
        }),
        act(reveal_label(), false, move |s, _| {
            if let Err(e) = reveal(&revealed) {
                crate::shell::palette::toast(s, "Unable to reveal screenshot", Some(&e));
            }
        }),
    ];
    crate::shell::palette::toast_with(app, "Screenshot saved", None, actions);
}

/// A picked element opens the comment editor beside it, as web's annotation overlay does.
fn pick(app: &mut App, tab: TabId, element: Value, image: Option<Vec<u8>>) {
    let r = &element["rect"];
    let rect = [&r["x"], &r["y"], &r["width"], &r["height"]].map(|v| v.as_f64().unwrap_or(0.) as f32);
    let ui = &mut app.ui.panel.browser;
    ui.pending = Some(Pending { tab, element, image, rect });
    ui.comment = TextEdit::new("");
    ui.comment.focused = true;
}

/// Enter or Attach: the pending element and its comment go to the composer.
fn submit_annotation(app: &mut App) {
    let Some(p) = app.ui.panel.browser.pending.take() else { return };
    let comment = app.ui.panel.browser.comment.text.trim().to_string();
    attach(app, &p.element, p.image, &comment);
}

/// A picked element goes to the composer as web's preview annotation, its crop as an image.
fn attach(app: &mut App, element: &Value, image: Option<Vec<u8>>, comment: &str) {
    let id = s5_net::uuid();
    let image = image.and_then(|bytes| {
        let dir = crate::host::t3_home().join("userdata").join("browser-artifacts");
        std::fs::create_dir_all(&dir).ok()?;
        // Web's sent bubble finds an annotation's crop by this name (TimelineUserMessageRow).
        let path = dir.join(format!("preview-annotation-{id}.png"));
        std::fs::write(&path, bytes).ok()?;
        Some(path.display().to_string())
    });
    let record = annotation(&id, element, comment);
    let detail = record["elements"][0]["selector"].as_str().or(record["elements"][0]["tagName"].as_str()).unwrap_or_default().to_string();
    crate::chat::composer::add_context(app, record);
    if let Some(image) = image {
        crate::chat::composer::attach_path(app, image);
    }
    crate::shell::palette::toast(app, "Element added to the composer", Some(&detail));
}

/// `previewAnnotationContextRecord` for one picked element and its comment.
fn annotation(id: &str, element: &Value, comment: &str) -> Value {
    let bounded = |v: &Value, max: usize| v.as_str().map(|s| s.chars().take(max).collect::<String>());
    let page_url = bounded(&element["pageUrl"], 2048).unwrap_or_default();
    let page_title = bounded(&element["pageTitle"], 2048).filter(|t| !t.trim().is_empty());
    let label = page_title.as_deref().unwrap_or("Preview annotation").replace(['[', ']', '\\', '\r', '\n'], " ");
    let label: String = label.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
    let details = json!({
        "pageUrl": page_url,
        "pageTitle": page_title,
        "tagName": bounded(&element["tagName"], 255).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "element".into()),
        "selector": bounded(&element["selector"], 2048),
        "htmlPreview": bounded(&element["htmlPreview"], 8000).unwrap_or_default(),
        "componentName": bounded(&element["componentName"], 2048),
        "source": null,
        "styles": bounded(&element["styles"], 8000).unwrap_or_default(),
    });
    json!({
        "version": 1,
        "contextId": format!("preview-annotation_{id}"),
        "kind": "preview-annotation",
        "label": label,
        "annotationId": id,
        "pageUrl": page_url,
        "pageTitle": page_title,
        "comment": comment.chars().take(8000).collect::<String>(),
        "targetSummary": "1 selected element",
        "styleChanges": [],
        "elements": [details],
        "elementIds": ["element-1"],
        "styleChangeDetails": [],
    })
}

/// Records `previewFocus` before any shortcut resolves, so `when` clauses see it on this key.
pub fn note_focus(app: &mut App, tx: &Tx) {
    app.ui.panel.browser.focused = panel_tab(app).is_some() && tx.focused().is_some_and(|k| k.contains("browser-"));
}

/// The preview shortcuts: `preview.toggle` anywhere, the rest while the browser has focus.
pub fn on_key(app: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if super::shortcut(app, e, "preview.toggle") {
        if panel_tab(app).is_some() {
            app.ui.shell.panel_open = false;
        } else {
            match app.ui.panel.surfaces.iter().rev().find(|v| matches!(v, super::Surface::Browser(_))).copied() {
                Some(surface) => super::activate(app, surface),
                None => super::add(app, super::Surface::Browser(0)),
            }
        }
        return true;
    }
    let Some(tab) = panel_tab(app).filter(|_| app.ui.panel.browser.focused) else { return false };
    if super::shortcut(app, e, "preview.refresh") {
        send(app, Cmd::Reload { tab, hard: false });
    } else if super::shortcut(app, e, "preview.focusUrl") {
        bar(app, tab).focused = true;
    } else if super::shortcut(app, e, "preview.zoomIn") {
        zoom(app, tab, 1);
    } else if super::shortcut(app, e, "preview.zoomOut") {
        zoom(app, tab, -1);
    } else if super::shortcut(app, e, "preview.resetZoom") {
        zoom(app, tab, 0);
    } else {
        return false;
    }
    true
}

fn screenshot(app: &mut App, tab: TabId) {
    let status = app.ui.panel.browser.browser.status(tab);
    let slug: String = host(&status.url)
        .map(|h| h.split(':').next().unwrap_or(h).to_ascii_lowercase())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let mut slug = slug.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    slug.truncate(80);
    let slug = if slug.trim_end_matches('-').is_empty() { "site".to_string() } else { slug.trim_end_matches('-').to_string() };
    let name = format!("browser-screenshot-{slug}-{}-{}.png", base36(data::now_ms().max(0) as u64), &s5_net::uuid().replace('-', "")[..8]);
    let path = crate::host::t3_home().join("userdata").join("browser-artifacts").join(name);
    send(app, Cmd::Screenshot(tab, path));
}

fn base36(mut n: u64) -> String {
    let mut out = vec![];
    loop {
        out.push(std::char::from_digit((n % 36) as u32, 36).unwrap_or('0'));
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.iter().rev().collect()
}

fn zoom(app: &mut App, tab: TabId, step: i32) {
    let current = app.ui.panel.browser.browser.status(tab).zoom;
    let index = ZOOM_LEVELS.iter().position(|z| (z - current).abs() < 0.001).unwrap_or(7) as i32;
    let next = if step == 0 { 1. } else { ZOOM_LEVELS[(index + step).clamp(0, ZOOM_LEVELS.len() as i32 - 1) as usize] };
    send(app, Cmd::Zoom(tab, next));
    app.ui.panel.browser.zoom_until = data::now_ms() + 1500;
}

/// Web's device limits: 240 to 3840 points a side and at most a 4K screen's area.
fn valid_size(w: u32, h: u32) -> bool {
    (240..=3840).contains(&w) && (240..=3840).contains(&h) && w * h <= 3840 * 2160
}

/// Fits a size inside the device limits, shrinking the height first.
fn clamp_size(w: u32, h: u32) -> (u32, u32) {
    let w = w.clamp(240, 3840);
    (w, h.clamp(240, 3840).min(3840 * 2160 / w))
}

fn toggle_device(app: &mut App, tab: TabId) {
    let on = app.ui.panel.browser.browser.status(tab).device.is_some();
    if on {
        send(app, Cmd::Device(tab, None));
        return;
    }
    // Responsive starts at the page's own size, under the toolbar it opens.
    let (w, h) = app.ui.panel.browser.area;
    let (w, h) = if w >= 1. { clamp_size(w.round() as u32, (h - DEVICE_TOOLBAR_H).max(1.).round() as u32) } else { (1024, 768) };
    *device(app, tab) = Device::default();
    set_device(app, tab, w, h, None);
}

fn set_device(app: &mut App, tab: TabId, w: u32, h: u32, preset: Option<usize>) {
    let d = device(app, tab);
    d.w.replace_text(w.to_string());
    d.h.replace_text(h.to_string());
    d.preset = preset;
    if d.lock.is_some() {
        d.lock = Some(w as f32 / h as f32);
    }
    send(app, Cmd::Device(tab, Some((w, h))));
}

/// Applies the typed size; with the ratio locked, the edited side drives the other.
fn commit_device(app: &mut App, tab: TabId, height_edited: bool) {
    let Some((cw, ch)) = app.ui.panel.browser.browser.status(tab).device else { return };
    let d = device(app, tab);
    let parse = |e: &TextEdit| e.text.trim().parse::<u32>().ok();
    let (w, h) = match (parse(&d.w), parse(&d.h), d.lock) {
        (Some(w), _, Some(ratio)) if !height_edited => (w, (w as f32 / ratio).round() as u32),
        (_, Some(h), Some(ratio)) if height_edited => ((h as f32 * ratio).round() as u32, h),
        (Some(w), Some(h), _) => (w, h),
        _ => (cw, ch),
    };
    let (w, h) = if valid_size(w, h) { (w, h) } else { (cw, ch) };
    let preset = d.preset.filter(|&i| [(PRESETS[i].1, PRESETS[i].2), (PRESETS[i].2, PRESETS[i].1)].contains(&(w, h)));
    set_device(app, tab, w, h, preset);
}

fn action(label: &str, glyph: &'static rune::Icon) -> kit::Button {
    kit::Button::new(label).icon(glyph).variant("ghost").size("icon-xs")
}

/// A toggle action: secondary with a primary glyph while on, as web's pressed buttons.
fn toggle(t: &T, label: &str, glyph: &'static rune::Icon, on: bool, disabled: bool, f: impl Fn(&mut App) + 'static) -> El {
    if !on {
        return action(label, glyph).disabled(disabled).on_press(f).build(t);
    }
    kit::Button::new(label).variant("secondary").size("icon-xs").disabled(disabled).on_press(f).build(t).child(icon(glyph, 14.).color(t.c.primary).a11y_hidden())
}

/// The chrome row and content for one tab; `scale` is the window's backing scale.
pub fn view(app: &App, t: &T, tab: TabId, scale: f32) -> El {
    let ui = &app.ui.panel.browser;
    let status = ui.browser.status(tab);
    let Some(state) = ui.tabs.get(&tab) else {
        return super::notice(t, "This browser tab is closed.");
    };
    // A streamed tab needs no local Chrome, and takes page input only while this viewer has control.
    let streamed = state.stream.as_ref().map(|s| s.state());
    let started = ui.browser.started() || streamed.is_some();
    let input_off = streamed.as_ref().is_some_and(|s| s.controller.as_deref() != Some("you"));
    let focused = state.bar.focused;
    let live = !idle(&status) && status.failure.is_none();
    let address = text_input(&state.bar, theme::TEXT_SM)
        .key(format!("browser-url-{tab}"))
        .single_line()
        .role("textbox")
        .label("Search or enter URL")
        .placeholder("Search or enter URL", t.c.contrast_placeholder)
        .w_full()
        .h(28.)
        .px(9.)
        .py(4.)
        .radius(theme::layout::CONTROL_RADIUS)
        .fill(if focused { t.c.background } else { rune::Color::TRANSPARENT })
        .hover(if focused { t.c.background } else { t.c.muted.alpha_mul(0.4) })
        .border(1., if focused { t.c.ring } else { rune::Color::TRANSPARENT })
        .color(t.c.contrast_foreground)
        .caret(t.c.contrast_foreground, t.c.primary.alpha_mul(0.3))
        .focus_ring(3., -3., t.c.ring.alpha_mul(0.24))
        .on_edit(move |s: &mut App| bar(s, tab))
        .on_submit(move |s: &mut App, tx: &mut Tx| {
            let input = bar(s, tab).text.clone();
            if !input.trim().is_empty() {
                navigate(s, tab, &input);
            }
            tx.blur();
        })
        .on_input_key(move |s: &mut App, e, tx| {
            if !matches!(e.key, Key::Escape) {
                return false;
            }
            let url = s.ui.panel.browser.browser.status(tab).url;
            bar(s, tab).replace_text(if url == "about:blank" { String::new() } else { url });
            tx.blur();
            true
        });
    // Web's address addon: "Open in system browser" shows while the bar is hovered.
    let external = (!focused && !idle(&status)).then(|| {
        let url = status.url.clone();
        el().pin(None, Some(2.), Some(2.), None).group_opacity(0., 1.).child(action("Open in system browser", &icons::EXTERNAL_LINK).build(t).on_press_tx(
            move |s: &mut App, tx: &mut Tx| {
                if let Err(e) = tx.open_url(&url) {
                    crate::shell::palette::toast(s, "Could not open this address", Some(&e.to_string()));
                }
            },
        ))
    });
    // A child added after `build` misses the disabled button's dimming.
    let refresh_off = !started || idle(&status) || input_off;
    let refresh_color = if refresh_off { t.c.contrast_foreground.alpha_mul(0.64) } else { t.c.contrast_foreground };
    let mut refresh_glyph = icon(&icons::REFRESH_CW, 14.).color(refresh_color).a11y_hidden();
    if status.loading {
        refresh_glyph = refresh_glyph.looping(rune::LoopSpec::rotation(1.));
    }
    let pip = ui.pip == Some(tab);
    let chrome = row()
        .w_full()
        .h(28.)
        .px(8.)
        .gap(4.)
        .mb(12.)
        .center_y()
        .child(
            row()
                .gap(2.)
                .center_y()
                .child(
                    action("Back", &icons::ARROW_LEFT)
                        .disabled(!status.can_back || input_off)
                        .on_press(move |s: &mut App| send(s, Cmd::Back(tab)))
                        .build(t),
                )
                .child(
                    action("Forward", &icons::ARROW_RIGHT)
                        .disabled(!status.can_forward || input_off)
                        .on_press(move |s: &mut App| send(s, Cmd::Forward(tab)))
                        .build(t),
                )
                .child(
                    kit::Button::new(if status.loading { "Stop" } else { "Refresh" })
                        .variant("ghost")
                        .size("icon-xs")
                        .disabled(refresh_off)
                        .on_press(move |s: &mut App| {
                            let loading = s.ui.panel.browser.browser.status(tab).loading;
                            send(s, if loading { Cmd::Stop(tab) } else { Cmd::Reload { tab, hard: false } })
                        })
                        .build(t)
                        .child(refresh_glyph),
                ),
        )
        .child(row().grow(1.).min_w(0.).hover_group().child(address).children(external))
        // Web offers neither on a server's tab: the page isn't in this client.
        .children(streamed.is_none().then(|| {
            let help = if status.failure.is_some() {
                "Page didn't load. Pick is unavailable until the page renders"
            } else if status.picking {
                "Cancel annotation (Esc)"
            } else {
                "Annotate an element"
            };
            toggle(t, if status.picking { "Cancel annotation" } else { "Annotate preview" }, &icons::MOUSE_POINTER_CLICK, status.picking, !status.painted || !live, move |s: &mut App| {
                let on = !s.ui.panel.browser.browser.status(tab).picking;
                send(s, Cmd::Pick(tab, on));
            })
            .help(help)
        }))
        .children(streamed.is_none().then(|| capture_button(t, tab, &status, live)))
        .child(toggle(
            t,
            if pip { "Close floating preview" } else { "Float preview over chat" },
            &icons::PICTURE_IN_PICTURE_2,
            pip,
            idle(&status) || !(pip || floats_here(app)),
            move |s: &mut App| float(s, tab),
        ))
        .child(
            el().key("browser-menu").child(
                action("Preview menu", &icons::ELLIPSIS_VERTICAL)
                    .disabled(!started)
                    .on_press(|s: &mut App| s.ui.panel.browser.menu ^= true)
                    .build(t),
            ),
        );
    let chrome = col().w_full().child(chrome).child(if status.loading {
        el().absolute(0., 26.).w_full().h(2.).radius(1.).fill(t.c.primary).shadow(0., 6., t.c.ring)
    } else {
        el()
    });
    let fatal = ui.browser.shared.lock().unwrap().fatal.clone();
    let content = if let Some(error) = fatal {
        notice_empty(t, "The browser could not start", &error)
            .child(kit::Button::new("Try again").variant("outline").size("sm").on_press(restart).build(t).mt(16.))
    } else if let Some(failure) = &status.failure {
        unreachable(t, tab, failure, ui.details)
    } else if let Some(view) = stream_view(app, t, tab, &state.pixels) {
        view
    } else if idle(&status) {
        empty(app, t, tab)
    } else if let Some(size) = status.device {
        let (w, h, scale) = browser::device_box(ui.area.0, ui.area.1, size, status.zoom);
        let page = page(&state.pixels, "browser-page", 1.).size(w, h).border(1., t.c.border.alpha_mul(0.7));
        col().grow(1.).min_h(0.).w_full().center().child(page.children(hover_box(t, &status, scale, (w, h))).children(annotation_editor(app, t, tab, scale, (w, h))).children(agent_cursor(app, t, tab, scale)))
    } else {
        let hover = hover_box(t, &status, status.zoom.max(0.25), ui.area);
        page(&state.pixels, "browser-page", 1.).grow(1.).min_h(0.).w_full().children(hover).children(annotation_editor(app, t, tab, status.zoom.max(0.25), ui.area)).children(agent_cursor(app, t, tab, status.zoom.max(0.25)))
    };
    let zoom_pill = (ui.zoom_until != 0 && !idle(&status)).then(|| {
        row()
            .pin(None, Some(12.), Some(12.), None)
            .px(10.)
            .py(4.)
            .radius(999.)
            .border(1., t.c.border.alpha_mul(0.7))
            .fill(t.c.popover.alpha_mul(0.95))
            .shadow(2., 8., Color::BLACK.alpha(0.12))
            .child(text(format!("{}%", (status.zoom * 100.).round())).font(theme::XS_MEDIUM).color(t.c.contrast_foreground))
    });
    let mut body = col().grow(1.).min_h(0.).child(chrome).children(stream_bar(app, t, tab));
    if let Some(size) = status.device.filter(|_| live) {
        body = body.child(device_toolbar(t, tab, state, size, ui.area.0));
    }
    body.child(
        // A new key when the scale changes re-reports the layout, so Chrome re-renders at it.
        col()
            .key(format!("browser-viewport-{scale}-{}", ui.restarts))
            .grow(1.)
            .min_h(0.)
            .w_full()
            .on_layout(move |s: &mut App, r| {
                s.ui.panel.browser.area = (r[2], r[3]);
                send(s, Cmd::Viewport { w: r[2].max(1.), h: r[3].max(1.), scale })
            })
            .child(content)
            .children(zoom_pill)
            .children(ui.dialog.as_ref().filter(|d| d.tab == tab).map(|d| js_dialog(t, d, &status.url))),
    )
}

/// Web's camera: secondary with a red glyph and dot while recording.
fn capture_button(t: &T, tab: TabId, status: &Status, live: bool) -> El {
    let press = move |s: &mut App, tx: &mut Tx| capture(s, tab, tx.mods().shift);
    if status.recording {
        return kit::Button::new("Stop recording")
            .variant("secondary")
            .size("icon-xs")
            .build(t)
            .child(icon(&icons::CAMERA, 14.).color(t.c.destructive).a11y_hidden())
            .child(el().pin(None, Some(2.), Some(2.), None).size(6., 6.).radius(3.).fill(t.c.destructive))
            .help("Stop recording")
            .on_press_tx(press);
    }
    let disabled = !status.painted || !live;
    let button = action("Capture screenshot", &icons::CAMERA).disabled(disabled).build(t).help("Screenshot · Shift-click to record");
    if disabled { button } else { button.on_press_tx(press) }
}

/// The page's `alert`, `confirm` or `prompt`, over the page as Chrome shows its own: the
/// message, the prompt's field, Cancel and OK. Enter accepts, Escape cancels.
fn js_dialog(t: &T, d: &Dialog, url: &str) -> El {
    let origin = host(url).map_or_else(|| "This page".to_string(), |h| h.to_string());
    let mut card = col()
        .key("browser-dialog")
        .role("alertdialog")
        .label(format!("{origin} says"))
        .w(400.)
        .padding(16.)
        .gap(12.)
        .radius(12.)
        .border(1., t.c.border)
        .fill(t.c.popover)
        .shadow(8., 24., Color::BLACK.alpha(0.25))
        .child(text(format!("{origin} says")).font(theme::SM_MEDIUM).color(t.c.contrast_foreground))
        .child(text(d.message.clone()).font(theme::TEXT_SM).color(t.c.contrast_foreground).wrap().w_full());
    if d.kind == "prompt" {
        card = card.child(
            text_input(&d.input, theme::TEXT_SM)
                .key("browser-dialog-input")
                .single_line()
                .role("textbox")
                .label("Prompt")
                .w_full()
                .h(32.)
                .px(9.)
                .radius(theme::layout::CONTROL_RADIUS)
                .fill(t.c.background)
                .border(1., t.c.ring)
                .color(t.c.contrast_foreground)
                .caret(t.c.contrast_foreground, t.c.primary.alpha_mul(0.3))
                .on_edit(|s: &mut App| match s.ui.panel.browser.dialog.as_mut() {
                    Some(d) => &mut d.input,
                    None => &mut s.ui.panel.browser.comment,
                })
                .on_submit(|s: &mut App, tx: &mut Tx| {
                    answer(s, true);
                    tx.blur();
                })
                .on_input_key(|s: &mut App, e, tx| {
                    if !matches!(e.key, Key::Escape) {
                        return false;
                    }
                    answer(s, false);
                    tx.blur();
                    true
                }),
        );
    }
    let mut buttons = row().w_full().gap(8.).center_y().child(el().grow(1.));
    if d.kind != "alert" {
        buttons = buttons.child(kit::Button::new("Cancel").variant("outline").size("sm").on_press(|s: &mut App| answer(s, false)).build(t));
    }
    buttons = buttons.child(kit::Button::new("OK").size("sm").on_press(|s: &mut App| answer(s, true)).build(t));
    row()
        .pin(Some(0.), Some(0.), Some(0.), Some(0.))
        .fill(Color::BLACK.alpha(0.2))
        .items_start()
        .pt(24.)
        .child(el().grow(1.))
        .child(card.child(buttons))
        .child(el().grow(1.))
}

/// Web's `BrowserDeviceToolbar`: preset, width × height, aspect lock, rotate and close.
fn device_toolbar(t: &T, tab: TabId, state: &Tab, (w, h): (u32, u32), width: f32) -> El {
    let d = &state.device;
    let size_input = |edit: &TextEdit, axis: &'static str, height: bool| {
        text_input(edit, theme::MONO_XS)
            .key(format!("browser-device-{axis}-{tab}"))
            .single_line()
            .role("textbox")
            .label(format!("Viewport {axis}"))
            .w(if width >= 360. { 56. } else { 52. })
            .h(24.)
            .px(6.)
            .radius(6.)
            .fill(t.c.input.alpha_mul(0.32))
            .border(1., t.c.contrast_input)
            .color(t.c.contrast_foreground)
            .caret(t.c.contrast_foreground, t.c.primary.alpha_mul(0.3))
            .on_edit(move |s: &mut App| if height { &mut device(s, tab).h } else { &mut device(s, tab).w })
            .on_submit(move |s: &mut App, tx: &mut Tx| {
                commit_device(s, tab, height);
                tx.blur();
            })
    };
    let preset = d.preset.map_or("Responsive", |i| PRESETS[i].0);
    let locked = d.lock.is_some();
    let mut bar = row().w_full().h(DEVICE_TOOLBAR_H - 1.).px(6.).gap(2.).center_y();
    if width >= 560. {
        bar = bar.child(text("Dimensions").font(theme::weight(theme::TEXT_2XS, theme::MEDIUM)).color(t.c.muted_foreground).mr(2.));
    }
    bar = bar.child(
        el().key("browser-device-preset").child(
            row()
                .w(if width >= 440. { 144. } else { 96. })
                .h(24.)
                .px(7.)
                .gap(4.)
                .center_y()
                .radius(8.)
                .hover(t.c.accent)
                .focusable(true)
                .role("button")
                .label("Browser device preset")
                .on_press(|s: &mut App| s.ui.panel.browser.presets ^= true)
                .child(text(preset).font(theme::XS_MEDIUM).color(t.c.contrast_foreground).truncate().grow(1.).min_w(0.))
                .child(icon(&icons::CHEVRON_DOWN, 14.).color(t.c.muted_foreground)),
        ),
    )
    .child(size_input(&d.w, "width", false))
    .child(text("×").font(theme::TEXT_XS).color(t.c.muted_foreground))
    .child(size_input(&d.h, "height", true))
    .child(toggle(
        t,
        if locked { "Unlock viewport aspect ratio" } else { "Lock viewport aspect ratio" },
        if locked { &icons::LINK_2 } else { &icons::UNLINK_2 },
        locked,
        false,
        move |s: &mut App| {
            let d = device(s, tab);
            d.lock = if d.lock.is_some() { None } else { Some(w as f32 / h as f32) };
        },
    ))
    .child(action("Rotate viewport", &ROTATE_ICON).on_press(move |s: &mut App| {
        // As web, a rotated preset stays selected and a locked ratio turns with it.
        let preset = device(s, tab).preset;
        set_device(s, tab, h, w, preset);
    }).build(t))
    .child(el().grow(1.))
    .child(action("Close device toolbar", &icons::X).on_press(move |s: &mut App| send(s, Cmd::Device(tab, None))).build(t));
    col().w_full().fill(t.c.background.alpha_mul(0.95)).child(bar).child(el().h(1.).w_full().fill(t.c.border.alpha_mul(0.7)))
}

/// Floats the tab over the chat and closes the panel, or puts it back.
fn float(app: &mut App, tab: TabId) {
    let ui = &mut app.ui.panel.browser;
    if ui.pip == Some(tab) {
        ui.pip = None;
        return;
    }
    ui.pip = Some(tab);
    ui.menu = false;
    app.ui.shell.panel_open = false;
}

/// Web's `ThreadPreviewMiniPlayer`: the floating tab, 320 points wide at most, at the top right
/// of the chat, with its open-in-panel and close actions on hover.
pub fn pip(app: &App, t: &T, out: &mut Vec<El>) {
    let Some(tab) = pip_tab(app) else { return };
    let ui = &app.ui.panel.browser;
    let Some(state) = ui.tabs.get(&tab) else { return };
    let status = ui.browser.status(tab);
    let (sw, sh) = match status.device {
        Some(size) => {
            let (w, h, _) = browser::device_box(ui.area.0, ui.area.1, size, status.zoom);
            (w, h)
        }
        None => ui.area,
    };
    let (sw, sh) = if sw < 1. || sh < 1. { (16., 10.) } else { (sw, sh) };
    let aspect = sw / sh;
    let right = if app.ui.shell.panel_open { app.ui.panel.width + 1. } else { 0. };
    let container = (ui.pip_area[2], ui.pip_area[3]);
    let mut area = el()
        .key("browser-pip-area")
        .transition(rune::Transition::none())
        .pin(Some(0.), Some(theme::layout::TOPBAR_H), Some(right), Some(0.))
        .on_layout(|s: &mut App, r| s.ui.panel.browser.pip_area = r);
    // The chat area's size arrives with the first layout.
    if container.0 < 1. || container.1 < 1. {
        out.push(area);
        return;
    }
    let [x, y, w, h] = pip_frame(ui.pip_frame, aspect, container);
    let popped = ui.popout == Some(tab) && popout::visible();
    let recording = ui.browser.status(tab).recording;
    let pill = row()
        .pin(None, Some(6.), Some(6.), None)
        .h(32.)
        .padding(3.)
        .gap(2.)
        .center_y()
        .radius(8.)
        .border(1., t.c.border.alpha_mul(0.8))
        .fill(t.c.popover.alpha_mul(0.92))
        .shadow(4., 16., Color::BLACK.alpha(0.2))
        .group_opacity(0., 1.)
        .cursor(rune::Cursor::Grab)
        .on_drag(move |s: &mut App, d, _| pip_gesture(s, d, (0, 0), aspect))
        .children(recording.then(|| el().size(24., 24.).center().child(el().size(8., 8.).radius(4.).fill(t.c.destructive)).a11y_hidden()))
        .child(
            action("Open preview in right panel", &icons::PANEL_RIGHT)
                .on_press(move |s: &mut App| {
                    s.ui.panel.browser.pip = None;
                    super::activate(s, super::Surface::Browser(tab));
                })
                .build(t),
        )
        .child(toggle(t, if popped { "Close popped-out preview" } else { "Pop preview into separate window" }, &icons::PICTURE_IN_PICTURE_2, popped, false, move |s: &mut App| toggle_popout(s, tab)).help(if popped { "Close separate window" } else { "Pop into separate window" }))
        .child(action("Close floating preview", &icons::X).on_press(|s: &mut App| s.ui.panel.browser.pip = None).build(t));

    // Web's `RESIZE_HANDLES`: 8 pt edges and 16 pt corners straddling the frame.
    let handles = [(0, -1, -4., 0.), (0, 1, -4., 0.), (-1, 0, -4., 0.), (1, 0, -4., 0.), (-1, -1, -8., 16.), (1, -1, -8., 16.), (-1, 1, -8., 16.), (1, 1, -8., 16.)].map(|(dx, dy, inset, size)| {
        let handle = el().on_drag(move |s: &mut App, d, _| pip_gesture(s, d, (dx, dy), aspect));
        let handle = match (dx, dy) {
            (0, -1) => handle.pin(Some(0.), Some(inset), Some(0.), None).h(8.).cursor(rune::Cursor::ResizeRow),
            (0, _) => handle.pin(Some(0.), None, Some(0.), Some(inset)).h(8.).cursor(rune::Cursor::ResizeRow),
            (-1, 0) => handle.pin(Some(inset), Some(0.), None, Some(0.)).w(8.).cursor(rune::Cursor::ResizeColumn),
            (_, 0) => handle.pin(None, Some(0.), Some(inset), Some(0.)).w(8.).cursor(rune::Cursor::ResizeColumn),
            _ => {
                let (l, r) = if dx < 0 { (Some(inset), None) } else { (None, Some(inset)) };
                let (top, b) = if dy < 0 { (Some(inset), None) } else { (None, Some(inset)) };
                handle.pin(l, top, r, b).size(size, size).cursor(rune::Cursor::Move)
            }
        };
        handle.key(format!("browser-pip-resize-{dx}-{dy}"))
    });
    let frame = el()
        .key("browser-pip")
        .pin(Some(x), Some(y), None, None)
        .size(w, h)
        .hover_group()
        .role("region")
        .label("Floating preview")
        .child(
            el().size(w, h)
                .radius(12.)
                .clip_rounded(12.)
                .fill(t.c.background)
                .border(1., t.c.border.alpha_mul(0.8))
                .shadow(8., 24., Color::BLACK.alpha(0.25))
                .child(page(&state.pixels, "browser-pip-page", sw / w).size(w, h)),
        )
        .child({
            let dot = el().pin(None, Some(10.), Some(10.), None).size(8., 8.).radius(4.).group_opacity(1., 0.);
            if status.recording { dot.fill(t.c.destructive).role("status").label("Recording preview") } else { dot.fill(t.c.foreground.alpha_mul(0.25)) }
        })
        .children(handles)
        .child(pill);
    area = area.child(crate::kit::motion::popup(frame));
    out.push(area);
}

const PIP_GAP: f32 = 12.;

fn pip_min_width(aspect: f32) -> f32 {
    240f32.max(150. * aspect)
}

/// `fitPreviewMiniPlayerWidth`: `(w, h)` at the source's aspect ratio.
fn pip_fit(desired: f32, aspect: f32, (max_w, max_h): (f32, f32), min_w: f32) -> (f32, f32) {
    let w = desired.max(min_w).min(max_w.max(1.)).min((max_h * aspect).max(1.)).round();
    (w, (w / aspect).round())
}

fn pip_clamp((x, y): (f32, f32), (w, h): (f32, f32), (cw, ch): (f32, f32)) -> (f32, f32) {
    (x.max(PIP_GAP).min((cw - w - PIP_GAP).max(PIP_GAP)), y.max(PIP_GAP).min((ch - h - PIP_GAP).max(PIP_GAP)))
}

/// `resolvePreviewMiniPlayerFrame`: `[x, y, w, h]` in the chat area; a moved or resized
/// preview keeps its width, the default sits top right at up to 320 pt.
fn pip_frame(frame: Option<[f32; 3]>, aspect: f32, container: (f32, f32)) -> [f32; 4] {
    let max = (container.0 - 2. * PIP_GAP, container.1 - 2. * PIP_GAP);
    let (desired, min_w) = match frame {
        Some([_, _, w]) => (w, w),
        None => (320f32.min(320. * aspect), pip_min_width(aspect)),
    };
    let size = pip_fit(desired, aspect, max, min_w);
    let at = frame.map_or((container.0 - PIP_GAP - size.0, PIP_GAP), |[x, y, _]| (x, y));
    let (x, y) = pip_clamp(at, size, container);
    [x, y, size.0, size.1]
}

/// `resizePreviewMiniPlayer`; `dir` is -1/0/1 per axis (west/north is -1).
fn pip_resize([x, y, w, h]: [f32; 4], (dx, dy): (i8, i8), (mx, my): (f32, f32), aspect: f32, (cw, ch): (f32, f32)) -> [f32; 4] {
    let (right, bottom) = (x + w, y + h);
    let max_w = match dx {
        -1 => right - PIP_GAP,
        1 => cw - PIP_GAP - x,
        _ => cw - 2. * PIP_GAP,
    };
    let max_h = match dy {
        -1 => bottom - PIP_GAP,
        1 => ch - PIP_GAP - y,
        _ => ch - 2. * PIP_GAP,
    };
    let want_w = w + mx * dx as f32;
    let want_h = h + my * dy as f32;
    // Corners project onto the aspect diagonal so opposite-moving axes never jump.
    let desired = match (dx != 0, dy != 0) {
        (true, true) => (want_w + want_h / aspect) / (1. + 1. / (aspect * aspect)),
        (true, false) => want_w,
        _ => want_h * aspect,
    };
    let size = pip_fit(desired, aspect, (max_w, max_h), w.min(pip_min_width(aspect)));
    let at = (if dx < 0 { right - size.0 } else { x }, if dy < 0 { bottom - size.1 } else { y });
    let (x, y) = pip_clamp(at, size, (cw, ch));
    [x, y, size.0, size.1]
}

/// Dragging the pill moves the floating preview; an edge or corner resizes it.
fn pip_gesture(s: &mut App, d: &rune::Drag, dir: (i8, i8), aspect: f32) {
    let ui = &mut s.ui.panel.browser;
    let container = (ui.pip_area[2], ui.pip_area[3]);
    let start = match (d.phase, ui.pip_start) {
        (Phase::Down, _) | (_, None) => {
            let start = pip_frame(ui.pip_frame, aspect, container);
            ui.pip_start = Some(start);
            start
        }
        (_, Some(start)) => start,
    };
    let [x, y, w, _] = if dir == (0, 0) {
        let (x, y) = pip_clamp((start[0] + d.dx, start[1] + d.dy), (start[2], start[3]), container);
        [x, y, start[2], start[3]]
    } else {
        pip_resize(start, dir, (d.dx, d.dy), aspect, container)
    };
    ui.pip_frame = Some([x, y, w]);
    if matches!(d.phase, Phase::Up) {
        ui.pip_start = None;
    }
}

/// The server's frames, sized as its page, with the agent's cursor over them.
fn stream_view(app: &App, t: &T, tab: TabId, pixels: &Pixels) -> Option<El> {
    let ui = &app.ui.panel.browser;
    let stream = ui.tabs.get(&tab)?.stream.as_ref()?;
    if !stream.state().painted {
        return Some(col().grow(1.).min_h(0.).w_full().center().child(text("Connecting to the server's browser…").font(theme::TEXT_SM).color(t.c.muted_foreground)));
    }
    let (w, h, scale) = stream_box(ui, tab);
    Some(col().grow(1.).min_h(0.).w_full().center().child(page(pixels, "browser-page", 1.).size(w, h).children(agent_cursor(app, t, tab, scale))))
}

/// Web's stream control row: who drives the page, and taking or handing back control.
fn stream_bar(app: &App, t: &T, tab: TabId) -> Option<El> {
    let state = app.ui.panel.browser.tabs.get(&tab)?.stream.as_ref()?.state();
    let label = match (state.connected, state.can_operate, state.controller.as_deref()) {
        (false, _, _) | (_, _, None) => "Connecting...",
        (_, false, _) => "Read-only",
        (_, _, Some("you")) => "You have control",
        (_, _, Some("agent")) => "Agent has control",
        (_, _, Some("another-viewer")) => "Another viewer has control",
        _ => "Watching",
    };
    let mut bar = row().w_full().h(27.).px(8.).gap(8.).center_y().child(text(label).font(theme::TEXT_XS).color(t.c.muted_foreground).grow(1.));
    if state.connected && state.can_operate {
        let mine = state.controller.as_deref() == Some("you");
        let toggle = move |s: &mut App| {
            if let Some(stream) = s.ui.panel.browser.tabs.get(&tab).and_then(|t| t.stream.as_ref()) {
                stream.toggle_control();
            }
        };
        bar = bar.child(
            kit::Button::new(if mine { "Release control" } else { "Take control" })
                .variant("outline")
                .size("xs")
                .disabled(state.controller.as_deref() == Some("another-viewer"))
                .on_press(toggle)
                .build(t),
        );
    }
    Some(col().w_full().child(bar).child(el().h(1.).w_full().fill(t.c.border.alpha_mul(0.7))))
}

/// Web's `AGENT_CURSOR_ACTIVE_MS`: the agent's cursor stays bright this long after each step.
const CURSOR_ACTIVE: Duration = Duration::from_millis(700);

/// Web's `AgentBrowserCursor`: where the agent last pointed on the page, CSS pixels times the
/// page's scale, ringed on a click, dimming between steps; a recording draws its own.
fn agent_cursor(app: &App, t: &T, tab: TabId, scale: f32) -> Option<El> {
    let ui = &app.ui.panel.browser;
    let pointer = tab_pointer(ui.tabs.get(&tab)?)?;
    if ui.browser.status(tab).recording {
        return None;
    }
    // Web draws none over a stream this viewer is driving.
    if ui.tabs.get(&tab)?.stream.as_ref().is_some_and(|s| s.state().controller.as_deref() == Some("you")) {
        return None;
    }
    let active = pointer.at.elapsed() < CURSOR_ACTIVE;
    let human = ui.human.is_some_and(|(t, at)| t == tab && at.elapsed() < HUMAN_ACTIVE);
    let mut mark = el().key("agent-cursor").pin(Some(pointer.x * scale - 2.), Some(pointer.y * scale - 2.), None, None).size(20., 20.).opacity(cursor_opacity(active, human)).a11y_hidden();
    if pointer.click && active {
        mark = mark.child(el().key(format!("agent-cursor-ring-{}", pointer.sequence)).pin(Some(2.), Some(2.), None, None).size(16., 16.).radius(8.).fill(t.c.primary.alpha_mul(0.25)));
    }
    Some(mark.child(icon(&icons::MOUSE_POINTER_2, 20.).color(t.c.primary)))
}

/// Electron's human controller lasts this long after the person's last input.
const HUMAN_ACTIVE: Duration = Duration::from_millis(750);

/// Web's `agentBrowserCursorOpacity`: dimmer while the person is the one acting.
fn cursor_opacity(active: bool, human: bool) -> f32 {
    match (active, human) {
        (true, _) => 1.,
        (false, true) => 0.18,
        (false, false) => 0.35,
    }
}

/// When the brightest agent cursor dims, so the panel repaints then.
fn cursor_wake(app: &App) -> Option<Duration> {
    let ui = &app.ui.panel.browser;
    let human = ui.human.and_then(|(_, at)| HUMAN_ACTIVE.checked_sub(at.elapsed()));
    ui.tabs.values().filter_map(tab_pointer).filter_map(|p| CURSOR_ACTIVE.checked_sub(p.at.elapsed())).chain(human).min()
}

/// Web's `shouldClearBrowserPointer`: a new load, or a different page, drops the old cursor.
fn navigated(before: &(String, bool), status: &Status) -> bool {
    if before.0.is_empty() {
        return false;
    }
    (status.loading && !before.1) || (!idle(status) && before.0 != "about:blank" && before.0 != status.url)
}

/// Where the agent last pointed: from the stream for a streamed tab, else from our relay.
fn tab_pointer(tab: &Tab) -> Option<agent::Pointer> {
    match &tab.stream {
        Some(stream) => stream.state().pointer,
        None => agent::pointer(tab.session.as_ref()?),
    }
}

/// The picker's box around the element under the pointer, as web's annotation overlay draws it.
fn hover_box(t: &T, status: &Status, scale: f32, page: (f32, f32)) -> Option<El> {
    let [x, y, w, h] = hover_rect(status.hover.filter(|_| status.picking)?, scale, page)?;
    Some(el().pin(Some(x), Some(y), None, None).size(w, h).radius(3.).fill(t.c.primary.alpha_mul(0.1)).border(2., t.c.primary))
}

/// The picked element's box and web's compact annotation editor beside it: the comment and
/// Attach (Enter); Escape drops the pick.
fn annotation_editor(app: &App, t: &T, tab: TabId, scale: f32, page: (f32, f32)) -> Vec<El> {
    let ui = &app.ui.panel.browser;
    let Some(pending) = ui.pending.as_ref().filter(|p| p.tab == tab) else { return Vec::new() };
    // Where the element is now, so the box and editor follow a scroll, as web's overlay does.
    let rect = ui.browser.status(tab).picked.unwrap_or(pending.rect);
    let [x, y, w, h] = rect;
    let mut out: Vec<El> = hover_rect(rect, scale, page)
        .map(|[x, y, w, h]| el().pin(Some(x), Some(y), None, None).size(w, h).radius(3.).fill(t.c.primary.alpha_mul(0.1)).border(2., t.c.primary))
        .into_iter()
        .collect();
    let size = (360f32.min(page.0 - 16.).max(160.), 50.);
    let (left, top) = editor_position([x * scale, y * scale, w * scale, h * scale], size, page);
    let comment = text_input(&ui.comment, theme::TEXT_SM)
        .key("browser-annotation-comment")
        .single_line()
        .role("textbox")
        .label("Describe the change")
        .placeholder("Describe the change…", t.c.muted_foreground)
        .grow(1.)
        .min_w(0.)
        .h(32.)
        .py(6.)
        .color(t.c.foreground)
        .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
        .on_edit(|s: &mut App| &mut s.ui.panel.browser.comment)
        .on_submit(|s: &mut App, tx: &mut Tx| {
            submit_annotation(s);
            tx.blur();
        })
        .on_input_key(|s: &mut App, e, tx| {
            if !matches!(e.key, Key::Escape) {
                return false;
            }
            s.ui.panel.browser.pending = None;
            tx.blur();
            true
        });
    out.push(
        row()
            .key("browser-annotation-editor")
            .pin(Some(left), Some(top), None, None)
            .w(size.0)
            .h(size.1)
            .padding(8.)
            .gap(8.)
            .center_y()
            .radius(12.)
            .border(1., t.c.border)
            .fill(t.c.popover.alpha_mul(0.96))
            .shadow(8., 24., Color::BLACK.alpha(0.25))
            .child(comment)
            .child(kit::Button::new("Attach").on_press(submit_annotation).build(t)),
    );
    out
}

/// `positionCompactEditor`: right of the target, else left, below or above, whichever overflows
/// the page least, then kept 8 pt inside it.
fn editor_position([x, y, w, h]: [f32; 4], (ew, eh): (f32, f32), (pw, ph): (f32, f32)) -> (f32, f32) {
    let gap = 8.;
    let candidates = [(x + w + gap, y), (x - ew - gap, y), (x + w - ew, y + h + gap), (x + w - ew, y - eh - gap)];
    let overflow = |(l, t): (f32, f32)| (-l).max(0.) + (-t).max(0.) + (l + ew - pw).max(0.) + (t + eh - ph).max(0.);
    let best = candidates.into_iter().fold(candidates[0], |best, c| if overflow(c) < overflow(best) { c } else { best });
    (best.0.max(8.).min((pw - ew - 8.).max(8.)), best.1.max(8.).min((ph - eh - 8.).max(8.)))
}

/// A CSS-pixel rect in page points, cut to the page.
fn hover_rect([x, y, w, h]: [f32; 4], scale: f32, (pw, ph): (f32, f32)) -> Option<[f32; 4]> {
    let (left, top) = ((x * scale).max(0.), (y * scale).max(0.));
    let (right, bottom) = (((x + w) * scale).min(pw), ((y + h) * scale).min(ph));
    (right > left && bottom > top).then(|| [left, top, right - left, bottom - top])
}

/// The page's pixels, forwarding input; `k` maps this element's points to page-area points.
fn page(pixels: &Pixels, key: &str, k: f32) -> El {
    el().pixels(pixels)
        .focusable(true)
        .key(key.to_string())
        .on_drag(move |s: &mut App, d, _| {
            send(
                s,
                Cmd::Mouse {
                    kind: match d.phase {
                        Phase::Down => "mousePressed",
                        Phase::Up => "mouseReleased",
                        _ => "mouseMoved",
                    },
                    x: d.x * k,
                    y: d.y * k,
                    clicks: d.clicks,
                },
            )
        })
        .on_pointer_move_local(move |s: &mut App, p, _| send(s, Cmd::Mouse { kind: "mouseMoved", x: p.x * k, y: p.y * k, clicks: 0 }))
        .on_wheel(move |s: &mut App, w, _| {
            send(s, Cmd::Wheel { x: w.x * k, y: w.y * k, dx: w.dx, dy: w.dy });
            true
        })
        .on_secondary_press_local(move |s: &mut App, p, _| {
            s.ui.panel.browser.context = None;
            send(s, Cmd::Inspect { x: p.x * k, y: p.y * k, at: (p.window_x, p.window_y) });
        })
        .on_key(|s: &mut App, e, tx| {
            // Web's way out of a page that takes every key.
            if matches!(e.key, Key::Escape) && e.mods.shift {
                tx.blur();
                return true;
            }
            // An open dialog blocks the page: Enter accepts it, Escape cancels it.
            if s.ui.panel.browser.dialog.as_ref().is_some_and(|d| Some(d.tab) == s.ui.panel.browser.shown) {
                match e.key {
                    Key::Enter => answer(s, true),
                    Key::Escape => answer(s, false),
                    _ => {}
                }
                return true;
            }
            // The page's editing shortcuts, on the system clipboard, as Electron's edit roles.
            if let Key::Command(c) = e.key {
                let cmd = match c {
                    'c' => Cmd::CopySelection { cut: false },
                    'x' => Cmd::CopySelection { cut: true },
                    'a' => Cmd::Edit("selectAll"),
                    'z' if e.mods.shift => Cmd::Edit("redo"),
                    'z' => Cmd::Edit("undo"),
                    'y' if !cfg!(target_os = "macos") => Cmd::Edit("redo"),
                    'v' => {
                        paste(tx);
                        return true;
                    }
                    _ => return false,
                };
                send(s, cmd);
                return true;
            }
            if e.mods.cmd || (e.mods.ctrl && !matches!(e.key, Key::Text(_))) {
                return false;
            }
            if matches!(e.key, Key::Escape) && s.ui.panel.browser.pending.take().is_some() {
                return true;
            }
            // Escape leaves element picking, as web's annotation mode.
            let picking = s.ui.panel.browser.shown.filter(|&tab| s.ui.panel.browser.browser.status(tab).picking);
            if let (Some(tab), Key::Escape) = (picking, &e.key) {
                send(s, Cmd::Pick(tab, false));
                return true;
            }
            let cmd = match &e.key {
                Key::Text(v) => Cmd::Text(v.clone()),
                Key::Enter => Cmd::Key("Enter"),
                Key::Backspace => Cmd::Key("Backspace"),
                Key::Delete => Cmd::Key("Delete"),
                Key::Tab => Cmd::Key("Tab"),
                Key::Left => Cmd::Key("ArrowLeft"),
                Key::Right => Cmd::Key("ArrowRight"),
                Key::Up => Cmd::Key("ArrowUp"),
                Key::Down => Cmd::Key("ArrowDown"),
                Key::Home => Cmd::Key("Home"),
                Key::End => Cmd::Key("End"),
                Key::PageUp => Cmd::Key("PageUp"),
                Key::PageDown => Cmd::Key("PageDown"),
                Key::Escape => Cmd::Key("Escape"),
                _ => return false,
            };
            send(s, cmd);
            true
        })
}

fn paste(tx: &mut Tx) {
    tx.read_clipboard(|s: &mut App, content, _| {
        if !content.text().is_empty() {
            send(s, Cmd::Text(content.text().to_string()));
        }
    });
}

/// `PreviewUnreachable`.
fn unreachable(t: &T, tab: TabId, failure: &browser::Failure, details: bool) -> El {
    let host = host(&failure.url).unwrap_or(&failure.url).to_string();
    let message = format!("{host}: {}.", friendly(&failure.description));
    let mut body = col()
        .w_full()
        .max_w(576.)
        .grow(1.)
        .px(32.)
        .py(64.)
        .child(icon(&ERROR_ICON, 48.).size(48., 48.).color(t.c.muted_foreground.alpha_mul(0.7)).mb(24.))
        .child(
            text("This site can’t be reached")
                .font(theme::weight(theme::TEXT_2XL, theme::SEMIBOLD))
                .color(t.c.contrast_foreground)
                .mb(12.),
        )
        .child(
            rich(message, vec![Span::new(0, host.len()).weight(theme::SEMIBOLD).color(t.c.contrast_foreground)])
                .font(theme::TEXT_SM)
                .color(t.c.muted_foreground)
                .w_full(),
        );
    if details {
        let mut tips = col().gap(4.).pl(20.);
        for tip in ["Checking your connection", "Confirming the dev server is running", "Checking the proxy and the firewall"] {
            tips = tips.child(text(format!("•  {tip}")).font(theme::TEXT_SM).color(t.c.muted_foreground));
        }
        body = body.child(
            col()
                .mt(24.)
                .padding(16.)
                .radius(10.)
                .border(1., t.c.border)
                .fill(t.c.muted.alpha_mul(0.4))
                .child(text("Try:").font(theme::SM_MEDIUM).color(t.c.contrast_foreground).mb(8.))
                .child(tips),
        );
    }
    body = body
        .child(
            text(if failure.description.is_empty() { "ERR_FAILED".to_owned() } else { failure.description.to_uppercase() })
                .font(theme::TEXT_XS)
                .color(t.c.muted_foreground.alpha_mul(0.7))
                .mt(32.),
        )
        .child(el().grow(1.).min_h(32.))
        .child(
            row()
                .gap(8.)
                .center_y()
                .child(
                    kit::Button::new(if details { "Hide details" } else { "Details" })
                        .variant("outline")
                        .size("sm")
                        .on_press(|s: &mut App| s.ui.panel.browser.details ^= true)
                        .build(t),
                )
                .child(el().grow(1.))
                .child(
                    kit::Button::new("Reload")
                        .size("sm")
                        .on_press(move |s: &mut App| send(s, Cmd::Reload { tab, hard: false }))
                        .build(t),
                ),
        );
    row().grow(1.).min_h(0.).w_full().fill(t.c.background).child(el().grow(1.)).child(body).child(el().grow(1.))
}

/// Web's `Empty` as the preview's empty state draws it: the globe tile, an xl title and a
/// centred description that wraps inside 384 points.
fn notice_empty(t: &T, title: &str, description: &str) -> El {
    col()
        .grow(1.)
        .min_h(0.)
        .w_full()
        .center()
        .px(24.)
        .child(
            el().size(36., 36.)
                .radius(6.)
                .fill(t.c.card)
                .border(1., t.c.border)
                .center()
                .mb(24.)
                .child(icon(&icons::GLOBE, 18.).size(18., 18.).color(t.c.muted_foreground)),
        )
        .child(
            text(title.to_string())
                .font(theme::weight(theme::TEXT_XL, theme::SEMIBOLD))
                .color(t.c.contrast_foreground)
                .align(rune::Align::Center)
                .wrap()
                .max_w(384.),
        )
        .child(
            text(description.to_string())
                .font(theme::TEXT_SM)
                .color(t.c.muted_foreground)
                .align(rune::Align::Center)
                .wrap()
                .max_w(384.)
                .mt(4.),
        )
}

/// `PreviewEmptyState`: recent pages and the server's listening ports, else a hint.
fn empty(app: &App, t: &T, tab: TabId) -> El {
    let mut servers: Vec<(String, String, u64)> = match &app.src {
        data::Source::Live(net) => net
            .resource(SERVERS, json!({}))
            .and_then(|v| v["servers"].as_array())
            .map(|rows| {
                rows.iter()
                    .filter_map(|v| {
                        let url = v["url"].as_str()?.to_string();
                        let port = v["port"].as_u64()?;
                        let name = v["processName"].as_str().filter(|n| !n.is_empty()).unwrap_or("Listening");
                        Some((url, format!("{name}\n{}:{port}", data::s(v, "host")), port))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => vec![],
    };
    servers.sort_by_key(|v| v.2);
    let history = super::history::entries(app);
    let recents = &history[..history.len().min(8)];
    if servers.is_empty() && recents.is_empty() {
        return notice_empty(t, "No preview yet", "Type a URL above, or run a dev script. Local servers this thread starts will show up here automatically.");
    }
    let section = |glyph: &'static rune::Icon, title: &str| {
        row()
            .gap(8.)
            .center_y()
            .child(icon(glyph, 16.).size(16., 16.).color(t.c.muted_foreground))
            .child(text(title).font(theme::SM_MEDIUM).color(t.c.muted_foreground))
    };
    let list = |rows: Vec<El>| {
        let mut list = col().w_full().radius(12.).border(1., t.c.border.alpha_mul(0.7)).fill(t.c.background).clip_rounded(12.);
        for (i, r) in rows.into_iter().enumerate() {
            if i > 0 {
                list = list.child(el().h(1.).w_full().fill(t.c.border.alpha_mul(0.6)));
            }
            list = list.child(r);
        }
        list
    };
    let entry = |page: &str, title: &str, description: &str| {
        row()
            .w_full()
            .px(12.)
            .py(12.)
            .gap(12.)
            .center_y()
            .hover(t.c.accent.alpha_mul(0.4))
            .role("button")
            .label(title.to_string())
            .focusable(true)
            .child(super::favicon::icon(app, page, 28., 4.).unwrap_or_else(|| super::favicon::mockup(t, 28.)))
            .child(
                col()
                    .grow(1.)
                    .min_w(0.)
                    .child(text(title.to_string()).font(theme::SM_MEDIUM).color(t.c.contrast_foreground).truncate())
                    .child(text(description.to_string()).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate()),
            )
    };
    let mut body = col().w_full().max_w(576.).gap(24.);
    if !recents.is_empty() {
        let rows = recents
            .iter()
            .map(|r| {
                let label = super::history::label(&r.url);
                let when = crate::pages::pull_requests::relative(r.at, app.now_ms);
                let (title, description) = if r.title.is_empty() { (label.clone(), when) } else { (r.title.clone(), format!("{label} · {when}")) };
                let (open, remove) = (r.url.clone(), r.url.clone());
                entry(&r.url, &title, &description)
                    .hover_group()
                    .on_press(move |s: &mut App| navigate(s, tab, &open))
                    .child(
                        el().size(22., 22.)
                            .radius(4.)
                            .center()
                            .hover(t.c.accent)
                            .group_opacity(0., 1.)
                            .role("button")
                            .label(format!("Remove {label} from history"))
                            .on_press(move |s: &mut App| super::history::remove(s, &remove))
                            .child(icon(&icons::X, 14.).size(14., 14.).color(t.c.muted_foreground)),
                    )
            })
            .collect();
        body = body.child(col().gap(12.).child(section(&icons::HISTORY, "Recently used")).child(list(rows)));
    }
    if !servers.is_empty() {
        let rows = servers
            .into_iter()
            .map(|(url, label, _)| {
                let (title, description) = label.split_once('\n').unwrap_or((label.as_str(), ""));
                entry(&url, title, description).on_press(move |s: &mut App| navigate(s, tab, &url))
            })
            .collect();
        body = body.child(
            col()
                .gap(12.)
                .child(section(&icons::RADIO_TOWER, "Local servers"))
                .child(list(rows))
                .child(
                    text("Select a live local server to open it in this browser tab.")
                        .font(theme::TEXT_XS)
                        .color(t.c.muted_foreground)
                        .px(4.),
                ),
        );
    }
    kit::scroll_area(
        "browser-empty",
        el().grow(1.)
            .min_h(0.)
            .w_full()
            .scroll(ScrollSpec { stick_bottom: false })
            .child(row().w_full().px(20.).py(32.).child(el().grow(1.)).child(body).child(el().grow(1.))),
        t,
    )
}

/// `PreviewMoreMenu` and the device preset list.
pub fn overlays(app: &App, t: &T, out: &mut Vec<El>) {
    let ui = &app.ui.panel.browser;
    if let Some(c) = ui.context.as_ref().filter(|c| Some(c.tab) == ui.shown) {
        context_menu(t, c, out);
    }
    let Some(tab) = panel_tab(app) else { return };
    let status = ui.browser.status(tab);
    if ui.presets && status.device.is_some() {
        let selected = ui.tabs.get(&tab).and_then(|v| v.device.preset);
        let mut menu = kit::Menu::new("browser-presets")
            .anchor("browser-device-preset")
            .placement(kit::Placement::BottomStart)
            // Web's select popup sizes to its widest row, "Samsung Galaxy S20 Ultra  412 × 915".
            .min_width(300.)
            .open(true)
            .on_close(|s: &mut App| s.ui.panel.browser.presets = false)
            .item(kit::MenuItem::new("Responsive").radio(selected.is_none()).on_select(move |s: &mut App| {
                s.ui.panel.browser.presets = false;
                device(s, tab).preset = None;
            }))
            .group_label("Standard");
        for (i, (label, w, h)) in PRESETS.iter().copied().enumerate() {
            menu = menu.item(kit::MenuItem::new(label).shortcut(format!("{w} × {h}")).radio(selected == Some(i)).on_select(move |s: &mut App| {
                s.ui.panel.browser.presets = false;
                set_device(s, tab, w, h, Some(i));
            }));
        }
        menu.overlay(&kit::theme(t), out);
    }
    if !ui.menu {
        return;
    }
    let scheme = ui.scheme;
    let radio = |label: &'static str, value: &'static str| {
        kit::MenuEntry::item(kit::MenuItem::new(label).radio(scheme == value).on_select(move |s: &mut App| {
            s.ui.panel.browser.scheme = value;
            s.ui.panel.browser.menu = false;
            send(s, Cmd::ColorScheme(value));
        }))
    };
    let blank = idle(&status);
    let zoom_button = |label: &str, glyph: &'static rune::Icon, variant: &str, disabled: bool, step: i32| {
        kit::Button::new(label).icon(glyph).variant(variant).size("icon-xs").disabled(disabled).on_press(move |s: &mut App| zoom(s, tab, step)).build(t)
    };
    let zoom_row = row()
        .w_full()
        .h(32.)
        .pl(8.)
        .pr(4.)
        .center_y()
        .child(text("Zoom").font(theme::TEXT_SM).color(t.c.contrast_foreground).grow(1.))
        .child(
            row()
                .gap(4.)
                .center_y()
                .child(zoom_button("Zoom out", &icons::MINUS, "outline", blank || status.zoom <= 0.25, -1))
                .child(
                    text(format!("{}%", (status.zoom * 100.).round()))
                        .font(theme::TEXT_XS)
                        .color(t.c.muted_foreground)
                        .align(rune::Align::Center)
                        .w(48.),
                )
                .child(zoom_button("Zoom in", &icons::PLUS, "outline", blank || status.zoom >= 5., 1))
                .child(zoom_button("Reset zoom", &icons::ROTATE_CCW, "ghost", blank, 0)),
        );
    let close = |s: &mut App| s.ui.panel.browser.menu = false;
    kit::Menu::new("browser-more")
        .anchor("browser-menu")
        .placement(kit::Placement::BottomEnd)
        .min_width(248.)
        .open(true)
        .on_close(|s: &mut App| {
            s.ui.panel.browser.menu = false;
            s.ui.panel.browser.appearance = false;
        })
        .item(kit::MenuItem::new("Hard reload").disabled(blank).on_select(move |s: &mut App| {
            close(s);
            send(s, Cmd::Reload { tab, hard: true });
        }))
        .item(kit::MenuItem::new("Open DevTools").disabled(blank).on_select(move |s: &mut App| {
            close(s);
            send(s, Cmd::DevTools(tab));
        }))
        .item(
            kit::MenuItem::new(if status.device.is_some() { "Hide device toolbar" } else { "Show device toolbar" })
                .disabled(blank)
                .on_select(move |s: &mut App| {
                    close(s);
                    toggle_device(s, tab);
                }),
        )
        .item(
            kit::MenuItem::new(if ui.popout.is_some() && popout::visible() { "Close separate preview window" } else { "Open separate preview window" })
                .disabled(blank && ui.popout.is_none())
                .on_select(move |s: &mut App| {
                    close(s);
                    toggle_popout(s, tab);
                }),
        )
        .item(
            kit::MenuItem::new("Appearance")
                .submenu(vec![radio("System", ""), radio("Light", "light"), radio("Dark", "dark")], ui.appearance)
                .on_submenu_change(|s: &mut App, open| s.ui.panel.browser.appearance = open),
        )
        .separator()
        .row(zoom_row)
        .separator()
        .group_label("Profile: Default")
        .item(kit::MenuItem::new("Clear cookies").on_select(move |s: &mut App| {
            close(s);
            send(s, Cmd::ClearCookies);
        }))
        .item(kit::MenuItem::new("Clear cache").on_select(move |s: &mut App| {
            close(s);
            send(s, Cmd::ClearCache);
        }))
        .overlay(&kit::theme(t), out);
}

/// Electron's page menu: Copy Link on a web link, Copy Image on an image, then the edit roles.
fn context_menu(t: &T, c: &Context, out: &mut Vec<El>) {
    let info = &c.info;
    let done = |s: &mut App| s.ui.panel.browser.context = None;
    let mut menu = kit::Menu::new("browser-context").native(true).width(198.).open(true).at(c.at.0, c.at.1).on_close(done);
    if let Some(link) = info["link"].as_str().filter(|l| l.starts_with("http://") || l.starts_with("https://")) {
        let link = link.to_string();
        menu = menu
            .item(kit::MenuItem::new("Copy Link").on_select_tx(move |s: &mut App, tx: &mut Tx| {
                done(s);
                tx.copy(link.clone());
            }))
            .separator();
    }
    if info["image"].is_object() {
        let clip = info["image"].clone();
        menu = menu
            .item(kit::MenuItem::new("Copy Image").on_select(move |s: &mut App| {
                done(s);
                send(s, Cmd::CopyImage(clip.clone()));
            }))
            .separator();
    }
    let (editable, selection) = (info["editable"] == true, info["selection"] == true);
    menu.item(kit::MenuItem::new("Cut").disabled(!(editable && selection)).on_select(move |s: &mut App| {
        done(s);
        send(s, Cmd::CopySelection { cut: true });
    }))
    .item(kit::MenuItem::new("Copy").disabled(!selection).on_select(move |s: &mut App| {
        done(s);
        send(s, Cmd::CopySelection { cut: false });
    }))
    .item(kit::MenuItem::new("Paste").disabled(!editable).on_select_tx(move |s: &mut App, tx: &mut Tx| {
        done(s);
        paste(tx);
    }))
    .item(kit::MenuItem::new("Select All").on_select(move |s: &mut App| {
        done(s);
        send(s, Cmd::Edit("selectAll"));
    }))
    .overlay(&kit::theme(t), out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_tab(app: &App, thread: &str, session: &str) -> Option<TabId> {
        app.ui.panel.browser.tabs.iter().find(|(_, t)| t.session == Some((thread.to_owned(), session.to_owned()))).map(|(id, _)| *id)
    }

    #[test]
    fn an_agents_session_opens_a_tab_on_its_thread_and_closes_with_it() {
        use super::super::Surface;
        let mut app = App::new();
        let ids: Vec<String> = app.state().threads.iter().take(2).map(|v| data::s(v, "id").to_string()).collect();
        let (thread, other) = (ids[0].clone(), ids[1].clone());
        app.navigate(Route::Thread(thread.clone()));
        super::super::switch_thread(&mut app);

        // `preview_open` with reveal: a tab on the thread's panel, floating over the chat.
        session_event(&mut app, crate::env::LOCAL, json!({ "type": "opened", "threadId": thread, "tabId": "a", "snapshot": { "runtime": "server", "reveal": true, "automationOwner": "agent-1", "viewport": { "_tag": "fill" } } }));
        let tab = session_tab(&app, &thread, "a").expect("the agent's session has a tab");
        assert!(app.ui.panel.surfaces.contains(&Surface::Browser(tab)));
        assert_eq!(app.ui.panel.browser.pip, Some(tab));
        assert!(!app.ui.shell.panel_open, "a reveal floats the tab; the panel stays shut");
        // The same session again opens nothing new.
        session_event(&mut app, crate::env::LOCAL, json!({ "type": "opened", "threadId": thread, "tabId": "a", "snapshot": { "runtime": "server" } }));
        assert_eq!(app.ui.panel.browser.tabs.len(), 1);

        // Another thread's session waits on that thread's panel; a hidden one gets no panel tab,
        // and a headless one streams from the server, which needs a live connection.
        session_event(&mut app, crate::env::LOCAL, json!({ "type": "opened", "threadId": other, "tabId": "b", "snapshot": { "runtime": "server", "automationOwner": "agent-2" } }));
        let b = session_tab(&app, &other, "b").unwrap();
        assert!(app.ui.panel.stash[&other].surfaces.contains(&Surface::Browser(b)) && !app.ui.panel.surfaces.contains(&Surface::Browser(b)));
        session_event(&mut app, crate::env::LOCAL, json!({ "type": "opened", "threadId": thread, "tabId": "c", "snapshot": { "runtime": "server", "reveal": false, "automationOwner": "agent-1" } }));
        let c = session_tab(&app, &thread, "c").unwrap();
        assert!(!app.ui.panel.surfaces.contains(&Surface::Browser(c)));
        session_event(&mut app, crate::env::LOCAL, json!({ "type": "opened", "threadId": thread, "tabId": "d", "snapshot": { "runtime": "server", "headless": true } }));
        assert!(session_tab(&app, &thread, "d").is_none());

        // `t3_preview_close`: the tab and its floating preview go.
        session_event(&mut app, crate::env::LOCAL, json!({ "type": "closed", "threadId": thread, "tabId": "a" }));
        assert!(!app.ui.panel.browser.tabs.contains_key(&tab));
        assert!(!app.ui.panel.surfaces.contains(&Surface::Browser(tab)) && app.ui.panel.browser.pip.is_none());
        session_event(&mut app, crate::env::LOCAL, json!({ "type": "closed", "threadId": other, "tabId": "b" }));
        assert!(!app.ui.panel.stash[&other].surfaces.contains(&Surface::Browser(b)));
    }

    #[test]
    fn a_tab_opened_here_binds_to_its_own_session_instead_of_opening_twice() {
        let mut app = App::new();
        let thread = data::s(&app.state().threads[0], "id").to_string();
        let mine = open_tab(&mut app);
        app.ui.panel.browser.opening.push((7, mine, thread.clone()));
        let opened = json!({ "type": "opened", "threadId": thread, "tabId": "s", "snapshot": { "runtime": "server" } });
        session_event(&mut app, crate::env::LOCAL, opened.clone());
        assert_eq!(app.ui.panel.browser.tabs.len(), 1, "held until preview.open answers");
        assert_eq!(app.ui.panel.browser.deferred.len(), 1);

        app.ui.panel.browser.opening.clear();
        bind(&mut app, mine, (thread.clone(), "s".into()));
        assert_eq!(session_tab(&app, &thread, "s"), Some(mine));
        session_event(&mut app, crate::env::LOCAL, opened);
        assert_eq!(app.ui.panel.browser.tabs.len(), 1);
    }

    #[test]
    fn a_popup_the_agents_page_opened_binds_to_the_session_the_server_adopts_it_as() {
        use super::super::Surface;
        let mut app = App::new();
        let thread = data::s(&app.state().threads[0], "id").to_string();
        let popup = open_tab(&mut app);
        app.ui.panel.browser.tabs.get_mut(&popup).unwrap().adopting = Some(((thread.clone(), "opener".into()), "P1".into()));

        // `adoptPopup` opens it as the agent's hidden tab: no new tab, and none on the panel.
        session_event(&mut app, crate::env::LOCAL, json!({ "type": "opened", "threadId": thread, "tabId": "adopted", "snapshot": { "runtime": "server", "reveal": false, "automationOwner": "agent-1" } }));
        assert_eq!(app.ui.panel.browser.tabs.len(), 1);
        assert_eq!(session_tab(&app, &thread, "adopted"), Some(popup));
        assert!(!app.ui.panel.surfaces.contains(&Surface::Browser(popup)));
        // A second agent session is a tab of its own.
        session_event(&mut app, crate::env::LOCAL, json!({ "type": "opened", "threadId": thread, "tabId": "other", "snapshot": { "runtime": "server", "reveal": false, "automationOwner": "agent-1" } }));
        assert_ne!(session_tab(&app, &thread, "other"), Some(popup));
    }

    #[test]
    fn a_new_server_process_takes_every_tab_off_its_old_session() {
        let mut app = App::new();
        let thread = data::s(&app.state().threads[0], "id").to_string();
        let (a, b) = (open_tab(&mut app), open_tab(&mut app));
        for (tab, session) in [(a, "s1"), (b, "s2")] {
            let t = app.ui.panel.browser.tabs.get_mut(&tab).unwrap();
            t.session = Some((thread.clone(), session.into()));
            t.announced = Some((1, "T".into()));
        }
        app.ui.panel.browser.tabs.get_mut(&b).unwrap().adopting = Some(((thread.clone(), "s1".into()), "P".into()));
        app.ui.panel.browser.opening.push((3, a, thread.clone()));
        app.ui.panel.browser.deferred.push(json!({ "type": "opened" }));
        rebind(&mut app);
        let ui = &app.ui.panel.browser;
        assert!(ui.tabs.values().all(|t| t.session.is_none() && t.announced.is_none() && t.adopting.is_none()));
        assert!(ui.deferred.is_empty());
        // No server listens in tests, so nothing is opened again; with one, each tab registers anew.
        assert!(ui.opening.is_empty());
    }

    #[test]
    fn a_new_page_drops_the_agents_cursor_as_web_does() {
        let at = |url: &str, loading: bool| Status { url: url.into(), loading, ..Status::default() };
        assert!(!navigated(&(String::new(), false), &at("https://a.dev/", true)), "the first report has nothing to compare");
        assert!(navigated(&("https://a.dev/".into(), false), &at("https://a.dev/", true)), "a reload starts");
        assert!(navigated(&("https://a.dev/".into(), false), &at("https://a.dev/next", false)), "a same-document navigation");
        assert!(!navigated(&("https://a.dev/".into(), true), &at("https://a.dev/", false)), "the load finishing");
        assert!(!navigated(&("about:blank".into(), false), &at("https://a.dev/", false)));
        assert_eq!((cursor_opacity(true, true), cursor_opacity(false, true), cursor_opacity(false, false)), (1., 0.18, 0.35));
    }

    #[test]
    fn a_streamed_tab_shows_its_sessions_page_and_history() {
        let snapshot = json!({ "navStatus": { "_tag": "Loading", "url": "https://a.dev/", "title": "A" }, "canGoBack": true, "canGoForward": false });
        let state = super::super::stream::State { painted: true, ..Default::default() };
        let status = stream_status(&snapshot, &state);
        assert_eq!((status.url.as_str(), status.title.as_str(), status.loading, status.can_back, status.can_forward, status.painted), ("https://a.dev/", "A", true, true, false, true));
        assert!(status.failure.is_none());
        let failed = super::super::stream::State { ended: true, error: Some("The server needs a Chrome sandbox.".into()), ..Default::default() };
        assert_eq!(stream_status(&snapshot, &failed).failure.unwrap().description, "The server needs a Chrome sandbox.");
        assert!(idle(&stream_status(&json!({ "navStatus": { "_tag": "Idle" } }), &state)));
    }

    #[test]
    fn a_new_chrome_reopens_each_page_as_it_was() {
        let page = Status { url: "https://a.dev/x".into(), zoom: 1.25, device: Some((390, 844)), muted: true, ..Status::default() };
        let cmds = reopen(3, &page);
        assert!(matches!(&cmds[..], [Cmd::Navigate(3, url), Cmd::Zoom(3, z), Cmd::Device(3, Some((390, 844))), Cmd::Mute(3, true)] if url == "https://a.dev/x" && *z == 1.25));
        // A page that failed to load tries its address again; a blank tab stays blank.
        let failed = Status { url: "about:blank".into(), failure: Some(browser::Failure { url: "http://localhost:5173/".into(), description: "ERR_CONNECTION_REFUSED".into() }), ..Status::default() };
        assert!(matches!(&reopen(4, &failed)[..], [Cmd::Navigate(4, url)] if url == "http://localhost:5173/"));
        assert!(reopen(5, &Status::default()).is_empty());
    }

    #[test]
    fn session_viewports_become_device_sizes() {
        assert_eq!(viewport_size(&json!({ "_tag": "preset", "width": 390, "height": 844, "presetId": "iphone" })), Some(Some((390, 844))));
        assert_eq!(viewport_size(&json!({ "_tag": "freeform", "width": 800, "height": 600 })), Some(Some((800, 600))));
        assert_eq!(viewport_size(&json!({ "_tag": "fill" })), Some(None));
        assert_eq!(viewport_size(&Value::Null), None);
    }

    #[test]
    fn the_floating_tab_shows_over_a_thread_or_the_new_thread_draft() {
        let mut app = App::new();
        app.ui.panel.browser.tabs.insert(1, Tab::new(Pixels::new()));
        app.ui.panel.browser.pip = Some(1);
        app.navigate(Route::Home { project_id: None });
        assert_eq!(pip_tab(&app), Some(1));
        app.navigate(Route::Thread("t".into()));
        assert_eq!(pip_tab(&app), Some(1));
        app.navigate(Route::PullRequests);
        assert_eq!(pip_tab(&app), None);
    }

    #[test]
    fn reveal_escapes_paths_for_dbus() {
        assert_eq!(file_uri(std::path::Path::new("/home/a b/x,y.png")), "file:///home/a%20b/x%2Cy.png");
    }

    #[test]
    fn address_bar_resolves_like_web() {
        let ddg = "https://duckduckgo.com/?q=%s";
        for (input, url) in [
            ("localhost:5173", "http://localhost:5173/"),
            ("127.0.0.1:8000/a?b", "http://127.0.0.1:8000/a?b"),
            ("example.com", "https://example.com/"),
            ("Example.com/Path", "https://example.com/Path"),
            ("http://x.dev", "http://x.dev/"),
            ("myhost:3000", "https://myhost:3000/"),
            ("rust closures", "https://duckduckgo.com/?q=rust%20closures"),
            ("hello", "https://duckduckgo.com/?q=hello"),
        ] {
            assert_eq!(resolve(input, ddg).as_deref(), Some(url), "{input}");
        }
        for input in ["mailto:a@b.c", "javascript:alert(1)", "file:///etc/hosts", "  "] {
            assert_eq!(resolve(input, ddg), None, "{input}");
        }
    }

    #[test]
    fn the_picker_box_scales_into_the_page_and_stays_inside_it() {
        assert_eq!(hover_rect([10., 20., 30., 40.], 1.25, (500., 500.)), Some([12.5, 25., 37.5, 50.]));
        // A tall body is cut to the page.
        assert_eq!(hover_rect([0., 0., 431., 2600.], 1.25, (539., 800.)), Some([0., 0., 538.75, 800.]));
        assert_eq!(hover_rect([0., 900., 10., 10.], 1., (500., 800.)), None);
    }

    #[test]
    fn the_annotation_editor_sits_beside_its_element_and_inside_the_page() {
        // Right of the element when it fits, else left, else below.
        assert_eq!(editor_position([10., 10., 100., 40.], (360., 50.), (800., 600.)), (118., 10.));
        assert_eq!(editor_position([600., 10., 190., 40.], (360., 50.), (800., 600.)), (232., 10.));
        assert_eq!(editor_position([0., 10., 780., 40.], (360., 50.), (800., 600.)), (420., 58.));
        // A page smaller than the editor pins it to the margin.
        assert_eq!(editor_position([0., 0., 100., 100.], (360., 50.), (300., 40.)), (8., 8.));
    }

    #[test]
    fn the_floating_preview_moves_and_resizes_inside_the_chat_area() {
        let area = (1000., 700.);
        // The default frame sits top right at 320 pt wide, 12 pt from the edges.
        assert_eq!(pip_frame(None, 1.6, area), [668., 12., 320., 200.]);
        // A moved frame keeps its width and stays inside the area.
        assert_eq!(pip_frame(Some([900., -50., 400.]), 1.6, area), [588., 12., 400., 250.]);
        // The east edge widens at the aspect ratio; the west edge keeps the right side put.
        assert_eq!(pip_resize([100., 100., 320., 200.], (1, 0), (80., 0.), 1.6, area), [100., 100., 400., 250.]);
        assert_eq!(pip_resize([100., 100., 320., 200.], (-1, 0), (-80., 0.), 1.6, area), [20., 100., 400., 250.]);
        // Never smaller than the minimum, never past the area's edge.
        assert_eq!(pip_resize([100., 100., 320., 200.], (1, 1), (-500., -500.), 1.6, area)[2], 240.);
        assert_eq!(pip_resize([100., 100., 320., 200.], (1, 0), (5000., 0.), 1.6, area)[2], 888.);
    }

    #[test]
    fn device_sizes_stay_inside_web_limits() {
        assert!(valid_size(375, 667) && valid_size(3840, 2160) && !valid_size(3840, 2161) && !valid_size(200, 800));
        assert_eq!(clamp_size(100, 5000), (240, 3840));
        assert_eq!(clamp_size(3840, 3840), (3840, 2160));
    }

    #[test]
    fn a_picked_element_becomes_a_preview_annotation() {
        let element = json!({
            "pageUrl": "http://localhost:5173/", "pageTitle": " Demo [home]\n", "tagName": "button", "selector": "#go",
            "htmlPreview": "<button id=\"go\">Sign in</button>", "componentName": null, "source": null, "styles": "color: red",
            "rect": { "x": 1 }, "doc": { "x": 1 },
        });
        let record = annotation("abc", &element, "Make it blue");
        assert_eq!(record["contextId"], "preview-annotation_abc");
        assert_eq!(record["comment"], "Make it blue");
        assert_eq!(record["label"], "Demo home");
        assert_eq!(record["targetSummary"], "1 selected element");
        let details = &record["elements"][0];
        assert_eq!(details["selector"], "#go");
        assert!(details.get("rect").is_none() && details["source"].is_null());
        let untitled = annotation("x", &json!({ "pageUrl": "http://a/", "pageTitle": "  " }), "");
        assert_eq!(untitled["label"], "Preview annotation");
        assert!(untitled["pageTitle"].is_null());
    }

    #[test]
    fn custom_search_needs_the_query_after_the_host() {
        assert_eq!(search_template("custom", "https://s.example/?q=%s"), "https://s.example/?q=%s");
        assert_eq!(search_template("custom", "https://%s.example/"), "https://duckduckgo.com/?q=%s");
        assert_eq!(search_template("bing", ""), "https://www.bing.com/search?q=%s");
    }
}
