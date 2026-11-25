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
    /// The tab's `prefers-color-scheme`; "" follows the system.
    scheme: &'static str,
    /// The environment whose server renders this tab, for a tab opened on another machine's
    /// thread: its session opens with the first page, and the server's frames show it.
    remote: Option<String>,
    /// The stream's dialog last put on screen, so an answered one isn't shown again.
    stream_dialog: Option<(String, String, String)>,
    /// The browser profile the tab opened in; a server tab's own is in its snapshot.
    profile: String,
}

impl Tab {
    fn new(pixels: Pixels) -> Tab {
        Tab { pixels, bar: TextEdit::new(""), focused: false, device: Device::default(), titled: Default::default(), icon: None, session: None, announced: None, revealed: None, adopting: None, stream: None, snapshot: Value::Null, nav: Default::default(), scheme: "", remote: None, stream_dialog: None, profile: browser::DEFAULT_PROFILE.to_owned() }
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
    details: bool,
    /// The discovered-servers subscription's payload while it is watched.
    servers_watch: Option<Value>,
    /// The tab floating over the chat (web's mini player).
    pub(super) pip: Option<TabId>,
    /// The page area's last size, in points.
    area: (f32, f32),
    presets: bool,
    /// When the zoom pill hides, as web's `ZoomIndicator` does 1.5 s after a change.
    zoom_until: i64,
    /// Whether the panel's browser had focus at the last key event (the `previewFocus` context).
    pub(super) focused: bool,
    /// Web's annotation session: what it marks so far, and its comment.
    annotation: Option<Annotation>,
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
    /// A server page's `<input multiple>` picker, collecting files to upload.
    chooser: Option<Chooser>,
    /// Electron's permission prompt: a clipboard read or an external link, one at a time.
    prompt: Option<Prompt>,
    /// Each origin's clipboard-read answer, kept for the session as Electron keeps it.
    clipboard_reads: HashMap<String, bool>,
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
    /// `preview.open` requests for tabs opened here: request id, tab, thread, environment.
    opening: Vec<(u64, TabId, String, String)>,
    /// Session events, by environment, held until this client's own `preview.open` replies, so
    /// its tab binds instead of a second one opening.
    deferred: Vec<(String, Value)>,
    /// `preview.adjust` and `preview.resize` for streamed tabs: request id, environment, tab.
    adjusting: Vec<(u64, String, TabId)>,
    /// Workspace files on their way to a tab: the `assets.createUrl` request and the file.
    assets: Vec<(u64, String)>,
    /// The device rail being dragged: its tab and edge, and the size the drag started from.
    rail: Option<(TabId, Edge, (u32, u32))>,
    /// A display-scale change waiting for a recording to end before Chrome relaunches.
    rescale: Option<f32>,
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

/// The files a server page's multi-file input will get. The native dialog picks one file at a
/// time, so the sheet gathers several, from the dialog or dropped on it, before they upload.
struct Chooser {
    tab: TabId,
    id: String,
    accept: Vec<String>,
    files: Vec<std::path::PathBuf>,
}

/// Adds picked or dropped files, once each; a drop skips files the input doesn't accept.
fn choose(files: &mut Vec<std::path::PathBuf>, picked: impl IntoIterator<Item = std::path::PathBuf>, accept: &[String]) {
    for path in picked {
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
        let accepted = accept.is_empty() || accept.contains(&ext);
        if accepted && path.is_file() && !files.contains(&path) {
            files.push(path);
        }
    }
}

struct Prompt {
    tab: TabId,
    ask: Ask,
}

enum Ask {
    ClipboardRead { id: u64, origin: String },
    External { url: String },
}

/// Electron's `MAX_PROMPT_URL_LENGTH`.
fn truncate_for_prompt(text: &str) -> String {
    if text.chars().count() > 300 { text.chars().take(300).chain(['…']).collect() } else { text.to_owned() }
}

/// A page's clipboard read: the origin's remembered answer, else a prompt, else (while another
/// prompt is open) refused, as Electron auto-denies a second prompt.
fn clipboard_read(app: &mut App, tx: &mut Tx, tab: TabId, id: u64, origin: String) {
    let remembered = app.ui.panel.browser.clipboard_reads.get(&origin).copied();
    match remembered {
        Some(true) => read_for_page(tx, tab, id),
        None if app.ui.panel.browser.prompt.is_none() => app.ui.panel.browser.prompt = Some(Prompt { tab, ask: Ask::ClipboardRead { id, origin } }),
        _ => send(app, Cmd::ClipboardAnswer(tab, id, None)),
    }
}

fn read_for_page(tx: &mut Tx, tab: TabId, id: u64) {
    tx.read_clipboard(move |s: &mut App, content, _| send(s, Cmd::ClipboardAnswer(tab, id, Some(content.text().to_string()))));
}

/// The prompt's answer; Escape and closing the tab refuse it.
fn answer_prompt(app: &mut App, tx: &mut Tx, allow: bool) {
    let Some(p) = app.ui.panel.browser.prompt.take() else { return };
    match p.ask {
        Ask::ClipboardRead { id, origin } => {
            app.ui.panel.browser.clipboard_reads.insert(origin, allow);
            if allow {
                read_for_page(tx, p.tab, id);
            } else {
                send(app, Cmd::ClipboardAnswer(p.tab, id, None));
            }
        }
        Ask::External { url } if allow => {
            if let Err(e) = tx.open_url(&url) {
                crate::shell::palette::toast(app, "Unable to open the link", Some(&e.to_string()));
            }
        }
        Ask::External { .. } => {}
    }
}

/// Electron's native permission box, drawn over the page as the page's own dialogs are.
fn permission_prompt(t: &T, p: &Prompt) -> El {
    let (title, detail, yes, no) = match &p.ask {
        Ask::ClipboardRead { origin, .. } => (format!("{} wants to see text and images copied to the clipboard", truncate_for_prompt(origin)), None, "Allow", "Block"),
        Ask::External { url } => {
            let scheme = url.split(':').next().unwrap_or_default();
            (format!("Open this {scheme} link?"), Some(format!("A page in the browser wants to open another application:\n\n{}", truncate_for_prompt(url))), "Open", "Cancel")
        }
    };
    let card = col()
        .key("browser-permission")
        .role("alertdialog")
        .label(title.clone())
        .w(400.)
        .padding(16.)
        .gap(12.)
        .radius(12.)
        .border(1., t.c.border)
        .fill(t.c.popover)
        .shadow(8., 24., Color::BLACK.alpha(0.25))
        .child(text(title).font(theme::SM_MEDIUM).color(t.c.contrast_foreground).wrap().w_full())
        .children(detail.map(|d| text(d).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap().w_full()))
        .child(
            row()
                .w_full()
                .gap(8.)
                .center_y()
                .child(el().grow(1.))
                .child(kit::Button::new(no).variant("outline").size("sm").build(t).on_press_tx(|s: &mut App, tx: &mut Tx| answer_prompt(s, tx, false)))
                .child(kit::Button::new(yes).size("sm").build(t).on_press_tx(|s: &mut App, tx: &mut Tx| answer_prompt(s, tx, true))),
        );
    row().pin(Some(0.), Some(0.), Some(0.), Some(0.)).fill(Color::BLACK.alpha(0.2)).items_start().pt(24.).child(el().grow(1.)).child(card).child(el().grow(1.))
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

/// Desktop's annotation tools: pick elements, mark a region, draw, or erase a mark.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tool {
    Select,
    Region,
    Draw,
    Erase,
}

/// One thing an annotation marks, in the order the page's marks were added.
#[derive(Debug, PartialEq)]
enum Target {
    Element(Value),
    Region,
    Stroke,
}

struct Annotation {
    tab: TabId,
    tool: Tool,
    targets: Vec<Target>,
    /// The latest target's viewport rect in CSS pixels, which the editor sits beside.
    anchor: Option<[f32; 4]>,
    /// A region or stroke being dragged: where it started, where the pointer is, its bounds
    /// and how many points it has, in viewport CSS pixels.
    drag: Option<Stroke>,
    /// A submit waiting for the screenshot; whether it sends the message too.
    capturing: Option<bool>,
}

#[derive(Clone, Copy)]
struct Stroke {
    start: (f32, f32),
    last: (f32, f32),
    min: (f32, f32),
    max: (f32, f32),
    points: usize,
}

/// A new tab on a blank page; Chrome starts with the first one. On another machine's thread
/// whose server runs a browser, the tab is that server's, as web's `previewRuntimeFor`.
pub fn open(app: &mut App) -> TabId {
    let profile = crate::settings::browser_profiles::default_profile(app);
    open_in(app, &profile)
}

/// A new tab in a browser profile, as web's "Open browser in a profile". Here a profile other
/// than Default runs its own Chrome, started for its first tab.
pub fn open_in(app: &mut App, profile: &str) -> TabId {
    if let Some(env) = server_env(app) {
        let ui = &mut app.ui.panel.browser;
        ui.next += 1;
        let mut fresh = Tab::new(Pixels::new());
        fresh.remote = Some(env);
        fresh.profile = profile.to_owned();
        ui.tabs.insert(ui.next, fresh);
        return ui.next;
    }
    let tab = open_tab(app, profile);
    register(app, tab);
    tab
}

/// The other environment that renders the current thread's tabs, when its server can.
fn server_env(app: &App) -> Option<String> {
    let Route::Thread(thread) = app.frame_route() else { return None };
    let env = crate::env::owner(app, thread);
    let capable = crate::env::source(app, &env).is_some_and(|s| s.state().config.as_ref().is_some_and(|c| c["environment"]["capabilities"]["serverBrowser"] == true));
    (env != crate::env::LOCAL && capable).then_some(env)
}

/// `browserDefaultViewport` as a device size, when it isn't the fill default.
fn default_viewport(app: &App) -> Option<(u32, u32)> {
    let v = crate::settings::value(app, "browserDefaultViewport");
    viewport_size(&v).flatten()
}

/// `browserDefaultAppearance` as a color scheme; "" follows the system.
fn default_scheme(app: &App) -> &'static str {
    match crate::settings::value(app, "browserDefaultAppearance").as_str() {
        Some("light") => "light",
        Some("dark") => "dark",
        _ => "",
    }
}

/// Opens a server tab's session on its environment, with its first page.
fn open_remote(app: &mut App, tab: TabId, url: Option<String>) {
    let Some(env) = app.ui.panel.browser.tabs.get(&tab).and_then(|t| t.remote.clone()) else { return };
    let Route::Thread(thread) = app.frame_route().clone() else { return };
    if app.ui.panel.browser.opening.iter().any(|o| o.1 == tab) {
        return;
    }
    let viewport = match default_viewport(app) {
        Some((w, h)) => json!({ "_tag": "freeform", "width": w, "height": h }),
        None => json!({ "_tag": "fill" }),
    };
    let profile = app.ui.panel.browser.tabs.get(&tab).map(|t| t.profile.clone());
    let mut payload = json!({ "threadId": thread, "viewport": viewport, "runtime": "server", "profileId": profile });
    if let Some(url) = url {
        payload["url"] = json!(url);
    }
    let Some(net) = env_net(app, &env) else {
        crate::shell::palette::toast(app, "Unable to open browser", Some("The environment is not connected."));
        return;
    };
    let id = net.request("preview.open", payload);
    app.ui.panel.browser.opening.push((id, tab, thread, env));
}

fn open_tab(app: &mut App, profile: &str) -> TabId {
    let waker = app.ui.panel.waker.clone();
    let ui = &mut app.ui.panel.browser;
    if let Some(waker) = waker {
        ui.browser.set_waker(waker);
    }
    ui.next += 1;
    let tab = ui.next;
    let pixels = Pixels::new();
    let mut fresh = Tab::new(pixels.clone());
    fresh.profile = profile.to_owned();
    ui.tabs.insert(tab, fresh);
    ui.browser.open(tab, pixels, profile);
    // `browserDefaultZoomFactor`: "Page zoom applied to new browser tabs."
    let zoom = crate::settings::value(app, "browserDefaultZoomFactor").as_f64().unwrap_or(1.) as f32;
    if let Some(&zoom) = ZOOM_LEVELS.iter().find(|z| (**z - zoom).abs() < 0.001).filter(|z| **z != 1.) {
        app.ui.panel.browser.browser.send(Cmd::Zoom(tab, zoom));
    }
    // Web's `resolveBrowserDefaults` for a new tab: its appearance and device size.
    let scheme = default_scheme(app);
    if !scheme.is_empty() {
        if let Some(t) = app.ui.panel.browser.tabs.get_mut(&tab) {
            t.scheme = scheme;
        }
        app.ui.panel.browser.browser.send(Cmd::ColorScheme(tab, scheme));
    }
    if let Some((w, h)) = default_viewport(app) {
        set_device(app, tab, w, h, PRESETS.iter().position(|p| (p.1, p.2) == (w, h)));
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

/// Captures a page once for an inline frame, in the Default profile's Chrome. Chrome starts for
/// it when no tab has, at the scale the panel last saw or 2; the panel relaunches it at the
/// window's own scale when it opens, as after an import.
pub fn snapshot(app: &mut App, url: String, width: u32, dark: bool) -> Option<browser::Shot> {
    let shot = browser::Shot::new(url, width, dark);
    inline_frame_cmd(app, Cmd::Snapshot(shot.clone())).then_some(shot)
}

/// Shows a page live inline, as `snapshot` starts Chrome for it; `inline_frame_cmd` drives it.
pub fn live(app: &mut App, url: String, width: u32, height: u32, dark: bool, script: Option<&str>) -> Option<browser::Live> {
    let mut live = browser::Live::new(url, width, height, dark);
    live.script = script.map(str::to_owned);
    inline_frame_cmd(app, Cmd::LiveOpen(live.clone())).then_some(live)
}

/// Sends an inline frame's command to the Default profile's Chrome, starting it if need be.
pub fn inline_frame_cmd(app: &mut App, cmd: Cmd) -> bool {
    let Some(waker) = app.ui.panel.waker.clone() else { return false };
    let ui = &mut app.ui.panel.browser;
    if !ui.browser.started() {
        let scale = ui.chrome_scale.unwrap_or(2.);
        ui.browser.start(waker);
        ui.browser.send(Cmd::Viewport { w: 0., h: 0., scale });
        ui.chrome_scale = Some(scale);
    }
    ui.browser.send(cmd);
    true
}

/// "Try again" after Chrome failed: a fresh worker, with the open tabs back on blank pages.
fn restart(app: &mut App) {
    let Some(waker) = app.ui.panel.waker.clone() else { return };
    let ui = &mut app.ui.panel.browser;
    ui.browser = browser::Browser::default();
    ui.browser.set_waker(waker);
    ui.restarts += 1;
    ui.chrome_scale = None;
    // The new worker hasn't been told which tab shows or is tracked; poll resends both.
    ui.shown = None;
    ui.tracked = None;
    let tabs: Vec<TabId> = ui.tabs.iter().filter(|(_, t)| t.stream.is_none() && t.remote.is_none()).map(|(id, _)| *id).collect();
    for tab in tabs {
        let pixels = Pixels::new();
        let mut fresh = Tab::new(pixels.clone());
        fresh.session = ui.tabs.get(&tab).and_then(|t| t.session.clone());
        fresh.profile = ui.tabs.get(&tab).map(|t| t.profile.clone()).unwrap_or_default();
        let profile = fresh.profile.clone();
        ui.tabs.insert(tab, fresh);
        ui.browser.open(tab, pixels, &profile);
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
    ui.browser.set_waker(waker);
    ui.chrome_scale = Some(scale);
    ui.shown = None;
    ui.tracked = None;
    let mut tabs: Vec<(TabId, Pixels, &'static str, String)> = ui.tabs.iter().filter(|(_, t)| t.stream.is_none() && t.remote.is_none()).map(|(id, t)| (*id, t.pixels.clone(), t.scheme, t.profile.clone())).collect();
    tabs.sort_by_key(|(id, ..)| *id);
    // The address bars keep their pages while the new Chrome starts.
    ui.browser.shared.lock().unwrap().tabs = tabs.iter().filter_map(|(id, ..)| Some((*id, Status { loading: true, ..before.get(id)?.clone() }))).collect();
    for (tab, pixels, scheme, profile) in tabs {
        ui.browser.open(tab, pixels, &profile);
        if !scheme.is_empty() {
            ui.browser.send(Cmd::ColorScheme(tab, scheme));
        }
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
    let opener_tab = ui.browser.shared.lock().unwrap().tabs.iter().find(|(_, s)| s.target.as_deref() == Some(opener.as_str())).map(|(id, _)| *id);
    // Web's `showPreviewPopup`: a popup from the floating preview floats in its place.
    let floating = opener_tab.is_some() && opener_tab == ui.pip;
    let opener_key = opener_tab.and_then(|id| ui.tabs.get(&id)?.session.clone()).filter(agent::driven);
    ui.next += 1;
    let tab = ui.next;
    let pixels = Pixels::new();
    let mut fresh = Tab::new(pixels.clone());
    fresh.adopting = opener_key.clone().map(|key| (key, target.clone()));
    if let Some(opener) = opener_tab.and_then(|id| ui.tabs.get(&id)) {
        fresh.profile = opener.profile.clone();
    }
    ui.tabs.insert(tab, fresh);
    ui.browser.adopt(tab, pixels, target.clone(), opener_tab);
    match opener_key {
        Some(key) => agent::popup(&key, target, url),
        None => {
            match app.frame_route().clone() {
                Route::Thread(thread) if floating => {
                    super::add_browser(app, &thread, tab, true);
                }
                _ => super::activate(app, super::Surface::Browser(tab)),
            }
            register(app, tab);
        }
    }
}

/// Electron's "new-tab" window open: the link loads afresh in a tab of its opener's profile,
/// shown unless a middle or Ctrl/Cmd-click opened it (`emitOpenLink`'s `background`).
fn open_link(app: &mut App, opener: TabId, url: String, background: bool) {
    let Some(profile) = app.ui.panel.browser.tabs.get(&opener).map(|t| t.profile.clone()) else { return };
    let floating = app.ui.panel.browser.pip == Some(opener);
    let tab = open_tab(app, &profile);
    send(app, Cmd::Navigate(tab, url));
    match app.frame_route().clone() {
        Route::Thread(thread) if background || floating => {
            super::add_browser(app, &thread, tab, floating && !background);
        }
        _ => super::activate(app, super::Surface::Browser(tab)),
    }
    register(app, tab);
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
    if ui.annotation.as_ref().is_some_and(|a| a.tab == tab) {
        ui.annotation = None;
    }
    if ui.pip == Some(tab) {
        ui.pip = None;
    }
    if ui.dialog.as_ref().is_some_and(|d| d.tab == tab) {
        ui.dialog = None;
    }
    if ui.chooser.as_ref().is_some_and(|c| c.tab == tab) {
        ui.chooser = None;
    }
    if ui.prompt.as_ref().is_some_and(|p| p.tab == tab) {
        ui.prompt = None;
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
        // A recording would lose its encoder page, so the relaunch waits for it to end.
        let recording = app.ui.panel.browser.browser.shared.lock().unwrap().tabs.values().any(|s| s.recording);
        match launched {
            _ if !started => {}
            Some(launched) if (launched - scale).abs() > 0.01 && recording => app.ui.panel.browser.rescale = Some(*scale),
            Some(launched) if (launched - scale).abs() > 0.01 => relaunch(app, *scale),
            Some(_) => app.ui.panel.browser.rescale = None,
            None => app.ui.panel.browser.chrome_scale = Some(*scale),
        }
    }
    // A server tab with no session yet opens one with its first page.
    if let Cmd::Navigate(tab, url) = &cmd {
        if app.ui.panel.browser.tabs.get(tab).is_some_and(|t| t.remote.is_some() && t.stream.is_none()) {
            open_remote(app, *tab, Some(url.clone()));
            return;
        }
    }
    let ui = &app.ui.panel.browser;
    let tab = match &cmd {
        Cmd::Mouse { .. } | Cmd::Pointer { .. } | Cmd::Wheel { .. } | Cmd::Text(_) | Cmd::Key(_) | Cmd::Inspect { .. } | Cmd::CopySelection { .. } | Cmd::Edit(_) | Cmd::ClearCookies | Cmd::ClearCache => ui.shown,
        Cmd::Navigate(tab, _) | Cmd::Back(tab) | Cmd::Forward(tab) | Cmd::Reload { tab, .. } | Cmd::Stop(tab) | Cmd::Zoom(tab, _) | Cmd::Device(tab, _) | Cmd::Pick(tab, _) | Cmd::DevTools(tab) | Cmd::Screenshot(tab, _) | Cmd::Record { tab, .. } | Cmd::Dialog { tab, .. } | Cmd::Mute(tab, _) | Cmd::ColorScheme(tab, _) => Some(*tab),
        _ => None,
    };
    // A server tab still opening has no page to act on.
    if tab.and_then(|id| ui.tabs.get(&id)).is_some_and(|t| t.remote.is_some() && t.stream.is_none()) {
        return;
    }
    // A streamed tab's page is the server's: settings go through its session, input through the
    // stream, and what neither can do does nothing.
    if let Some((tab, stream)) = tab.and_then(|id| Some((id, ui.tabs.get(&id)?.stream.as_ref()?))) {
        if let Some((method, mut payload)) = stream_rpc(&cmd) {
            let Some(t) = ui.tabs.get(&tab) else { return };
            let (Some((thread, session)), Some(env)) = (t.session.clone(), t.remote.clone()) else { return };
            payload["threadId"] = json!(thread);
            payload["tabId"] = json!(session);
            if let Some(net) = env_net(app, &env) {
                let id = net.request(method, payload);
                app.ui.panel.browser.adjusting.push((id, env, tab));
            }
            return;
        }
        stream.command(&cmd, stream_box(ui, tab).2);
        return;
    }
    // ClearCookies and ClearCache are Chrome's, whichever tab shows.
    if let Cmd::Viewport { w, h, .. } = &cmd {
        for stream in ui.tabs.values().filter_map(|t| t.stream.as_ref()) {
            stream.resize(*w, *h);
        }
    }
    app.ui.panel.browser.browser.send(cmd);
}

/// What web sends a server tab's session for a panel setting: `preview.adjust` for zoom,
/// appearance, a hard reload and clearing data, `preview.resize` for the device size.
fn stream_rpc(cmd: &Cmd) -> Option<(&'static str, Value)> {
    Some(match cmd {
        Cmd::Zoom(_, zoom) => {
            let snapped = ZOOM_LEVELS.iter().copied().min_by(|a, b| (a - zoom).abs().total_cmp(&(b - zoom).abs())).unwrap_or(1.);
            // The server takes the levels' exact decimals, which f32 doesn't keep.
            ("preview.adjust", json!({ "zoomFactor": (snapped as f64 * 100.).round() / 100. }))
        }
        Cmd::Reload { hard: true, .. } => ("preview.adjust", json!({ "hardReload": true })),
        Cmd::ClearCookies => ("preview.adjust", json!({ "clear": "cookies" })),
        Cmd::ClearCache => ("preview.adjust", json!({ "clear": "cache" })),
        Cmd::ColorScheme(_, scheme) => ("preview.adjust", json!({ "colorScheme": if scheme.is_empty() { "system" } else { scheme } })),
        Cmd::Device(_, size) => {
            let viewport = match size {
                Some((w, h)) => json!({ "_tag": "freeform", "width": w, "height": h }),
                None => json!({ "_tag": "fill" }),
            };
            ("preview.resize", json!({ "viewport": viewport }))
        }
        _ => return None,
    })
}

/// Web's `resolveDiscoveredServerUrl`: a loopback address on another machine's thread means that
/// machine, reached at its server's host when that is on a private network.
fn rewrite_loopback(url: &str, environment: &str) -> Result<String, String> {
    let Ok(mut page) = s5_net::url::Url::parse(url) else { return Ok(url.to_owned()) };
    let loopback = matches!(page.host_str(), Some("localhost" | "127.0.0.1" | "0.0.0.0" | "[::1]" | "[::]"));
    if !loopback || !matches!(page.scheme(), "http" | "https") {
        return Ok(url.to_owned());
    }
    let Some(host) = s5_net::url::Url::parse(environment).ok().and_then(|u| u.host_str().map(str::to_owned)) else { return Ok(url.to_owned()) };
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if matches!(bare, "localhost" | "::1") || bare.starts_with("127.") {
        return Ok(url.to_owned());
    }
    if !super::favicon::private_network_host(&host) {
        return Err("This environment port needs the planned authenticated preview gateway; its server address is not directly private-network reachable.".into());
    }
    page.set_host(Some(&host)).map_err(|e| e.to_string())?;
    Ok(page.to_string())
}

/// Web's `isBrowserPreviewFile`: HTML and PDF files open in the browser.
pub fn previewable_file(path: &str) -> bool {
    super::file_preview::kind(path.split(['?', '#']).next().unwrap_or_default()).opens_in_browser()
}

/// Web's `openFileInPreview`, the one path every "Open in browser" takes (the file preview's
/// action, chat file links): the server serves the file at a signed address that opens in a new
/// tab. `path` is absolute or relative to the workspace.
pub fn open_file(app: &mut App, path: &str) {
    let fail = |app: &mut App, why: &str| crate::shell::palette::toast(app, "Unable to open file in browser", Some(why));
    let Some(cwd) = super::workspace(app) else { return fail(app, "Open a thread or a project first.") };
    let (absolute, inside) = asset_path(path, &cwd);
    let resource = match super::context(app) {
        Some((thread, _)) => json!({ "_tag": if inside { "workspace-file" } else { "media-file" }, "threadId": thread, "path": absolute }),
        None if inside => json!({ "_tag": "draft-workspace-file", "cwd": cwd, "path": absolute }),
        None => return fail(app, "Files outside the project open from a thread."),
    };
    match app.src.net().map(|n| n.request("assets.createUrl", json!({ "resource": resource }))) {
        Some(id) => app.ui.panel.browser.assets.push((id, path.to_owned())),
        None => fail(app, "The environment is not connected."),
    }
}

/// The file's absolute path, and whether it lies inside the workspace (`workspace-file`).
fn asset_path(path: &str, cwd: &str) -> (String, bool) {
    let root = cwd.trim_end_matches('/');
    if path.starts_with('/') {
        let inside = path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'));
        return (path.to_owned(), inside);
    }
    let mut rest = path;
    while let Some(r) = rest.strip_prefix("./") {
        rest = r;
    }
    (format!("{root}/{rest}"), !rest.starts_with("../"))
}

fn take_assets(app: &mut App) -> bool {
    let mut changed = false;
    for (id, path) in std::mem::take(&mut app.ui.panel.browser.assets) {
        let Some(net) = app.src.net() else { continue };
        let Some(reply) = net.take_reply(id) else {
            app.ui.panel.browser.assets.push((id, path));
            continue;
        };
        let base = net.base_url().trim_end_matches('/').to_owned();
        changed = true;
        match reply {
            Ok(v) => super::open_preview(app, &format!("{base}{}", data::s(&v, "relativeUrl"))),
            Err(e) => {
                let message = crate::shell::palette::error_message(&e, &path);
                crate::shell::palette::toast(app, "Unable to open file in browser", Some(&message));
            }
        }
    }
    changed
}

/// Web's `previewPdfAssetUrl`: a workspace PDF the server's browser can't draw, fetched from
/// this client's own connection to that server when the page's address is its loopback.
fn pdf_asset_url(url: &str, base: &str) -> Option<String> {
    let page = s5_net::url::Url::parse(url).ok()?;
    let base = s5_net::url::Url::parse(base).ok()?;
    if !matches!(page.scheme(), "http" | "https") || !page.path().starts_with("/api/assets/") || !page.path().to_ascii_lowercase().ends_with(".pdf") {
        return None;
    }
    let loopback = matches!(page.host_str(), Some("localhost" | "0.0.0.0" | "[::1]")) || page.host_str().is_some_and(|h| h.starts_with("127."));
    if page.origin() != base.origin() && !loopback {
        return Some(page.to_string());
    }
    let mut own = base.join(page.path()).ok()?;
    own.set_query(page.query());
    own.set_fragment(page.fragment());
    Some(own.to_string())
}

/// A streamed page's box in the page area and its scale from CSS pixels, as a device size's.
fn stream_box(ui: &Ui, tab: TabId) -> (f32, f32, f32) {
    let device = ui.tabs.get(&tab).is_some_and(|t| viewport_size(&t.snapshot["viewport"]).flatten().is_some());
    let (w, h) = if device { browser::device_area(ui.area.0, ui.area.1) } else { (ui.area.0.max(1.), ui.area.1.max(1.)) };
    let size = ui.tabs.get(&tab).and_then(|t| t.stream.as_ref()?.state().viewport).unwrap_or((w as u32, h as u32));
    browser::device_box(w, h, size, 1.)
}

/// A page for a tab this machine renders: another environment's loopback address is that machine.
fn local_target(app: &App, tab: TabId, url: String) -> Result<String, String> {
    if app.ui.panel.browser.tabs.get(&tab).map_or(true, |t| t.remote.is_some() || t.stream.is_some()) {
        return Ok(url);
    }
    let Route::Thread(thread) = app.frame_route() else { return Ok(url) };
    let env = crate::env::owner(app, thread);
    match crate::env::source(app, &env) {
        Some(data::Source::Live(net)) if env != crate::env::LOCAL => rewrite_loopback(&url, net.base_url()),
        _ => Ok(url),
    }
}

pub fn navigate(app: &mut App, tab: TabId, input: &str) {
    let engine = crate::settings::value(app, "browserSearchEngine");
    let custom = crate::settings::value(app, "browserCustomSearchUrl");
    let template = search_template(engine.as_str().unwrap_or(""), custom.as_str().unwrap_or(""));
    let resolved = resolve(input, template).map(|url| local_target(app, tab, url));
    match resolved {
        Some(Err(e)) => crate::shell::palette::toast(app, "Could not open this address", Some(&e)),
        Some(Ok(url)) => {
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
        "custom" if crate::settings::valid_search_template(custom.trim()) => custom.trim(),
        _ => "https://duckduckgo.com/?q=%s",
    }
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
    let synced = stream_events(app, tx) || synced;
    let synced = take_assets(app) || synced;
    let wait = cursor_wake(app);
    if let Some(wait) = wait {
        tx.wake_after(wait + Duration::from_millis(16));
    }
    let newest = app.ui.panel.browser.tabs.values().filter_map(tab_pointer).map(|p| p.sequence).max().unwrap_or(0);
    let cursor = (newest, wait.is_some());
    let synced = synced || std::mem::replace(&mut app.ui.panel.browser.cursor_seen, cursor) != cursor;
    let want = shown(app);
    let live = matches!(app.src, data::Source::Live(_));
    let watch = (live && !app.ui.panel.browser.tabs.is_empty()).then(|| servers_payload(app));
    if watch != app.ui.panel.browser.servers_watch {
        let before = std::mem::replace(&mut app.ui.panel.browser.servers_watch, watch.clone());
        if let Some(net) = app.src.net() {
            if let Some(before) = before {
                net.unwatch_resource(SERVERS, before);
            }
            if let Some(watch) = watch {
                net.watch_resource(SERVERS, watch);
            }
        }
    }
    if let Some(scale) = app.ui.panel.browser.rescale {
        if !app.ui.panel.browser.browser.shared.lock().unwrap().tabs.values().any(|s| s.recording) {
            app.ui.panel.browser.rescale = None;
            relaunch(app, scale);
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
    // The editor follows the latest picked element as the page scrolls.
    let track = ui.annotation.as_ref().filter(|a| matches!(a.targets.last(), Some(Target::Element(_)))).map(|a| a.tab);
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
    let mut left = vec![];
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
                left.push(*id);
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
    // A new page has none of the annotation's marks.
    if app.ui.panel.browser.annotation.as_ref().is_some_and(|a| left.contains(&a.tab) && a.capturing.is_none()) {
        app.ui.panel.browser.annotation = None;
    }
    for event in events {
        match event {
            browser::Event::Erased(tab, Some(index)) => {
                if let Some(a) = app.ui.panel.browser.annotation.as_mut().filter(|a| a.tab == tab && index < a.targets.len()) {
                    a.targets.remove(index);
                    if a.targets.is_empty() {
                        a.anchor = None;
                    }
                }
            }
            browser::Event::Erased(_, None) => {}
            browser::Event::Marked(tab, image) => annotated(app, tab, image),
            browser::Event::Screenshot(Ok(path)) => screenshot_saved(app, path),
            browser::Event::Screenshot(Err(e)) => crate::shell::palette::toast(app, "Unable to capture screenshot", Some(&e)),
            browser::Event::Popup { target, opener, url } => adopt(app, target, opener, url),
            browser::Event::OpenLink { tab, url, background } => open_link(app, tab, url, background),
            browser::Event::PopupClosed(tab) => close(app, tab),
            browser::Event::FrameLink(url) => super::open_preview(app, &url),
            browser::Event::Picked { tab, element, image } => pick(app, tab, element, image),
            browser::Event::Enclosed(tab, rect, elements) => enclosed(app, tab, rect, elements),
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
            browser::Event::ClipboardRead { tab, id, origin } => clipboard_read(app, tx, tab, id, origin),
            browser::Event::External { tab, url } => {
                if app.ui.panel.browser.prompt.is_none() {
                    app.ui.panel.browser.prompt = Some(Prompt { tab, ask: Ask::External { url } });
                }
            }
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
    app.ui.panel.browser.opening.push((id, tab, thread, crate::env::LOCAL.to_owned()));
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
        changed |= take_replies(app, &env);
        let mut events = take_deferred(app, &env);
        events.extend(crate::env::net_of(app, &env).map(|n| n.take_resource_events(SESSIONS, json!({}))).unwrap_or_default());
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
    // Web's `PreviewUnreachable` for a page that failed to load; a download has its own page.
    let failed = (nav["_tag"] == "LoadFailed" && nav["download"].is_null()).then(|| nav["description"].as_str().unwrap_or_default().to_owned());
    let failure = state.error.clone().filter(|_| state.ended).or(failed).map(|description| browser::Failure { url: url.clone(), description });
    Status {
        title: nav["title"].as_str().unwrap_or_default().to_owned(),
        loading: nav["_tag"] == "Loading",
        can_back: snapshot["canGoBack"] == true,
        can_forward: snapshot["canGoForward"] == true,
        painted: state.painted,
        failure,
        url,
        zoom: snapshot["zoomFactor"].as_f64().map_or(1., |z| z as f32),
        device: viewport_size(&snapshot["viewport"]).flatten(),
        ..Status::default()
    }
}

fn sync_streams(app: &mut App) -> bool {
    let ui = &app.ui.panel.browser;
    let mut changed = false;
    let mut gone = vec![];
    let mut sized = vec![];
    let mut dialogs = vec![];
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
            if state.dialog != tab.stream_dialog {
                dialogs.push((*id, state.dialog.clone()));
            }
            let status = stream_status(&tab.snapshot, &state);
            if shared.tabs.get(id) != Some(&status) {
                if let Some(size) = status.device.filter(|d| shared.tabs.get(id).and_then(|s| s.device) != Some(*d)) {
                    sized.push((*id, size));
                }
                shared.tabs.insert(*id, status);
                changed = true;
            }
        }
    }
    // The device toolbar's drafts follow the session's size.
    for (tab, (w, h)) in sized {
        let d = device(app, tab);
        if !d.w.focused && !d.h.focused {
            d.w.replace_text(w.to_string());
            d.h.replace_text(h.to_string());
            d.preset = PRESETS.iter().position(|p| (p.1, p.2) == (w, h));
        }
    }
    // The page's alert, confirm or prompt shows as a local one does, answered through the stream.
    for (tab, dialog) in dialogs {
        let ui = &mut app.ui.panel.browser;
        if let Some(t) = ui.tabs.get_mut(&tab) {
            t.stream_dialog = dialog.clone();
        }
        match dialog {
            Some((kind, message, default)) => {
                let mut input = TextEdit::new(default.as_str());
                input.focused = kind == "prompt";
                input.set_selection(0, input.text.len());
                ui.dialog = Some(Dialog { tab, kind, message, input });
            }
            None if ui.dialog.as_ref().is_some_and(|d| d.tab == tab) => ui.dialog = None,
            None => {}
        }
        changed = true;
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

/// What a server tab's page asked of this viewer: copied text, a download, a file picker.
fn stream_events(app: &mut App, tx: &mut Tx) -> bool {
    use super::stream::Event;
    let events: Vec<(TabId, Vec<Event>)> = app.ui.panel.browser.tabs.iter().filter_map(|(id, t)| Some((*id, t.stream.as_ref()?.take_events()))).filter(|(_, e)| !e.is_empty()).collect();
    let changed = !events.is_empty();
    for (tab, event) in events.into_iter().flat_map(|(tab, events)| events.into_iter().map(move |e| (tab, e))) {
        match event {
            Event::Clipboard(text) => tx.copy(text),
            // As a person's download in a local tab: saved to Downloads and shown in its folder.
            Event::Saved(Ok(path)) => {
                if let Err(e) = reveal(&path) {
                    crate::shell::palette::toast(app, "Unable to show the download", Some(&e));
                }
            }
            Event::Saved(Err(e)) => crate::shell::palette::toast(app, "Unable to save the download", Some(&e)),
            // A multi-file input gathers its files in a sheet over the page.
            Event::FileChooser { id, multiple: true, accept } => {
                app.ui.panel.browser.chooser = Some(Chooser { tab, id, accept: accept_extensions(&accept), files: vec![] });
            }
            // The native dialog picks one file; cancelling it cancels the page's picker.
            Event::FileChooser { id, multiple: false, accept } => {
                let options = chooser_options("Choose a file to upload", &accept_extensions(&accept));
                tx.open_file(options, move |s: &mut App, picked: rune::FileDialogResult, _| {
                    let files = match picked {
                        Ok(Some(path)) => vec![std::path::PathBuf::from(path)],
                        _ => vec![],
                    };
                    if let Some(stream) = s.ui.panel.browser.tabs.get(&tab).and_then(|t| t.stream.as_ref()) {
                        stream.upload(&id, files);
                    }
                });
            }
            Event::FileChooserClosed(id) => {
                if app.ui.panel.browser.chooser.as_ref().is_some_and(|c| c.tab == tab && c.id == id) {
                    app.ui.panel.browser.chooser = None;
                }
            }
            Event::Failed(title, e) => crate::shell::palette::toast(app, &title, Some(&e)),
        }
    }
    changed
}

fn chooser_options(title: &str, exts: &[String]) -> rune::FileDialogOptions {
    let options = rune::FileDialogOptions { title: title.into(), ..Default::default() };
    if exts.is_empty() {
        return options;
    }
    let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
    options.filter("Accepted files", exts.as_slice())
}

/// Uploads the sheet's files, or none to cancel the page's picker.
fn finish_chooser(app: &mut App, upload: bool) {
    let Some(c) = app.ui.panel.browser.chooser.take() else { return };
    if let Some(stream) = app.ui.panel.browser.tabs.get(&c.tab).and_then(|t| t.stream.as_ref()) {
        stream.upload(&c.id, if upload { c.files } else { vec![] });
    }
}

/// The multi-file sheet: the chosen files, Add file…, drops, Cancel and Upload.
fn chooser_sheet(t: &T, c: &Chooser) -> El {
    let mut list = col().key("browser-file-chooser-list").w_full().gap(2.).max_h(220.).scroll(rune::ScrollSpec { stick_bottom: false });
    for (i, path) in c.files.iter().enumerate() {
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        list = list.child(
            row()
                .w_full()
                .h(28.)
                .px(8.)
                .gap(8.)
                .center_y()
                .radius(6.)
                .fill(t.c.muted.alpha_mul(0.5))
                .child(icon(&icons::FILE, 14.).color(t.c.muted_foreground).a11y_hidden())
                .child(text(name).font(theme::TEXT_SM).color(t.c.contrast_foreground).grow(1.).min_w(0.).truncate())
                .child(
                    kit::Button::new("Remove file")
                        .icon(&icons::X)
                        .variant("ghost-muted")
                        .size("icon-xs")
                        .on_press(move |s: &mut App| {
                            if let Some(c) = s.ui.panel.browser.chooser.as_mut().filter(|c| i < c.files.len()) {
                                c.files.remove(i);
                            }
                        })
                        .build(t),
                ),
        );
    }
    if c.files.is_empty() {
        list = list.child(
            col()
                .w_full()
                .h(72.)
                .center()
                .gap(6.)
                .radius(8.)
                .border(1., t.c.border)
                .child(icon(&icons::UPLOAD, 16.).color(t.c.muted_foreground).a11y_hidden())
                .child(text("Drop files here, or add them one at a time").font(theme::TEXT_XS).color(t.c.muted_foreground)),
        );
    }
    let accepted = (!c.accept.is_empty()).then(|| text(format!("Accepts {}", c.accept.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(", "))).font(theme::TEXT_XS).color(t.c.muted_foreground));
    let exts = c.accept.clone();
    let add = kit::Button::new("Add file…").variant("outline").size("sm").build(t).on_press_tx(move |_: &mut App, tx: &mut Tx| {
        tx.open_file(chooser_options("Add a file to upload", &exts), |s: &mut App, picked: rune::FileDialogResult, _| {
            if let (Ok(Some(path)), Some(c)) = (picked, s.ui.panel.browser.chooser.as_mut()) {
                choose(&mut c.files, [std::path::PathBuf::from(path)], &[]);
            }
        });
    });
    let count = c.files.len();
    let card = col()
        .key("browser-file-chooser")
        .role("dialog")
        .label("Choose files to upload")
        .w(400.)
        .padding(16.)
        .gap(12.)
        .radius(12.)
        .border(1., t.c.border)
        .fill(t.c.popover)
        .shadow(8., 24., Color::BLACK.alpha(0.25))
        .on_drop_files(|s: &mut App, files: &[std::path::PathBuf], _: &mut Tx| {
            if let Some(c) = s.ui.panel.browser.chooser.as_mut() {
                choose(&mut c.files, files.iter().cloned(), &c.accept.clone());
            }
        })
        .child(text("Choose files to upload").font(theme::SM_MEDIUM).color(t.c.contrast_foreground))
        .children(accepted)
        .child(list)
        .child(
            row()
                .w_full()
                .gap(8.)
                .center_y()
                .child(add)
                .child(el().grow(1.))
                .child(kit::Button::new("Cancel").variant("outline").size("sm").on_press(|s: &mut App| finish_chooser(s, false)).build(t))
                .child(
                    kit::Button::new(if count > 1 { format!("Upload {count} files") } else { "Upload".to_owned() })
                        .size("sm")
                        .disabled(count == 0)
                        .on_press(|s: &mut App| finish_chooser(s, true))
                        .build(t),
                ),
        );
    row()
        .pin(Some(0.), Some(0.), Some(0.), Some(0.))
        .fill(Color::BLACK.alpha(0.2))
        .items_start()
        .pt(24.)
        .child(el().grow(1.))
        .child(card)
        .child(el().grow(1.))
}

/// The file extensions an `<input accept>` names; MIME types like `image/*` don't narrow it.
fn accept_extensions(accept: &str) -> Vec<String> {
    accept.split(',').filter_map(|a| a.trim().strip_prefix('.')).filter(|e| !e.is_empty()).map(str::to_ascii_lowercase).collect()
}

/// A server session as a tab showing the server's frames.
fn open_stream(app: &mut App, env: &str, key: agent::Key, snapshot: &Value) -> Option<TabId> {
    app.ui.panel.waker.as_ref()?;
    let ui = &mut app.ui.panel.browser;
    ui.next += 1;
    let tab = ui.next;
    let mut fresh = Tab::new(Pixels::new());
    fresh.remote = Some(env.to_owned());
    ui.tabs.insert(tab, fresh);
    if attach_stream(app, tab, key, snapshot) {
        return Some(tab);
    }
    app.ui.panel.browser.tabs.remove(&tab);
    None
}

/// Connects a server tab to its session's frames.
fn attach_stream(app: &mut App, tab: TabId, key: agent::Key, snapshot: &Value) -> bool {
    let Some(waker) = app.ui.panel.waker.clone() else { return false };
    let Some(env) = app.ui.panel.browser.tabs.get(&tab).and_then(|t| t.remote.clone()) else { return false };
    let Some(net) = env_net(app, &env) else { return false };
    let (base, bearer) = (net.base_url().to_owned(), net.bearer().to_owned());
    let ui = &mut app.ui.panel.browser;
    let area = ui.area;
    let Some(t) = ui.tabs.get_mut(&tab) else { return false };
    let stream = super::stream::Stream::open(base, bearer, &key, t.pixels.clone(), waker);
    if area.0 >= 1. {
        stream.resize(area.0, area.1);
    }
    t.stream = Some(stream);
    t.session = Some(key);
    t.snapshot = snapshot.clone();
    true
}

/// This environment's answers to `preview.open` for tabs opened here, and to settings a server
/// tab sent its session.
fn take_replies(app: &mut App, env: &str) -> bool {
    let ui = &app.ui.panel.browser;
    let ids: Vec<u64> = ui.opening.iter().filter(|o| o.3 == env).map(|o| o.0).chain(ui.adjusting.iter().filter(|a| a.1 == env).map(|a| a.0)).collect();
    if ids.is_empty() {
        return false;
    }
    let Some(net) = env_net(app, env) else { return false };
    let replies: Vec<(u64, s5_net::Reply)> = ids.into_iter().filter_map(|id| net.take_reply(id).map(|r| (id, r))).collect();
    let changed = !replies.is_empty();
    for (id, reply) in replies {
        let ui = &mut app.ui.panel.browser;
        if let Some(i) = ui.opening.iter().position(|o| o.0 == id) {
            let (_, tab, thread, env) = ui.opening.remove(i);
            match reply {
                Ok(snapshot) => {
                    if let Some(session) = snapshot["tabId"].as_str().map(str::to_owned) {
                        bind(app, tab, (thread, session), &snapshot);
                    }
                }
                // This machine's own tabs register silently; a server tab the person opened says why.
                Err(e) if env != crate::env::LOCAL => crate::shell::palette::toast(app, "Unable to open browser", e["message"].as_str()),
                Err(_) => {}
            }
        } else if let Some(i) = ui.adjusting.iter().position(|a| a.0 == id) {
            let (_, _, tab) = ui.adjusting.remove(i);
            match reply {
                Ok(snapshot) => {
                    keep_snapshot(app, tab, &snapshot);
                }
                Err(e) => crate::shell::palette::toast(app, "Unable to change the browser tab", e["message"].as_str()),
            }
        }
    }
    changed
}

fn take_deferred(app: &mut App, env: &str) -> Vec<Value> {
    let ui = &mut app.ui.panel.browser;
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut ui.deferred).into_iter().partition(|(e, _)| e == env);
    ui.deferred = rest;
    mine.into_iter().map(|(_, v)| v).collect()
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
    let replied = take_replies(app, crate::env::LOCAL);
    let Some(net) = crate::env::local_mut(app).net() else { return replied };
    let mut events = net.take_resource_events(SESSIONS, json!({}));
    let changed = !events.is_empty() || replied;
    let mut pending = take_deferred(app, crate::env::LOCAL);
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
    for (id, t) in ui.tabs.iter_mut().filter(|(_, t)| t.stream.is_none() && t.remote.is_none()) {
        if let Some((thread, _)) = t.session.take() {
            t.announced = None;
            t.adopting = None;
            stale.push((*id, thread));
        }
    }
    ui.opening.retain(|o| o.3 != crate::env::LOCAL);
    ui.deferred.retain(|(env, _)| env != crate::env::LOCAL);
    stale.sort();
    for (tab, thread) in stale {
        register_on(app, tab, thread);
    }
}

fn bind(app: &mut App, tab: TabId, key: agent::Key, snapshot: &Value) {
    let ui = &mut app.ui.panel.browser;
    if ui.tabs.values().any(|t| t.session.as_ref() == Some(&key)) {
        return;
    }
    if let Some(t) = ui.tabs.get_mut(&tab) {
        if t.session.is_some() {
            return;
        }
        // A server tab shows its new session's frames; this machine's tab is that session's page.
        if t.remote.is_some() {
            attach_stream(app, tab, key, snapshot);
        } else {
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
            if snapshot["automationOwner"].is_null() && app.ui.panel.browser.opening.iter().any(|o| o.2 == thread && o.3 == env) {
                app.ui.panel.browser.deferred.push((env.to_owned(), event));
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
            let tab = open_tab(app, browser::DEFAULT_PROFILE);
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

/// The tab's profile as a badge, unless it is the one new tabs open in.
fn profile_badge(app: &App, t: &T, tab: TabId) -> Option<El> {
    let profile = tab_profile(app, tab);
    if profile == crate::settings::browser_profiles::default_profile(app) {
        return None;
    }
    let name = crate::settings::browser_profiles::profile_name(app, &profile);
    let short: String = if name.chars().count() > 14 { name.chars().take(13).chain(['…']).collect() } else { name.clone() };
    Some(kit::badge(&short, "outline", "default", t).key(format!("browser-profile-{tab}")).label(&name).help(name))
}

/// A headless scene's tab on a painted page, without Chrome or a server: `annotation`, `rails`,
/// `stream-dialog`, `host-setup`, `more` (a Work-profile server tab's More menu), `chooser`, the
/// permission prompts, or a server page that failed or downloaded.
pub fn scene(app: &mut App, kind: &str) -> TabId {
    app.client_settings.values.insert("browserProfiles".into(), json!([{ "id": "work", "name": "Work", "kind": "persistent" }]));
    let url = "http://localhost:5173/login";
    let ui = &mut app.ui.panel.browser;
    ui.next += 1;
    let tab = ui.next;
    let mut fresh = Tab::new(Pixels::new());
    fresh.bar.replace_text(url);
    let mut status = Status { url: url.into(), title: "Sign in · Demo".into(), painted: true, can_back: true, ..Default::default() };
    let dialog = (kind == "stream-dialog").then(|| ("confirm".to_owned(), "Leave this page? Changes you made may not be saved.".to_owned(), String::new()));
    let streamed = |host_setup| {
        let state = super::stream::State { connected: true, painted: true, can_operate: true, controller: Some("you".into()), viewport: Some((800, 600)), host_setup, dialog: dialog.clone(), ..Default::default() };
        Some(super::stream::Stream::fixture(state))
    };
    match kind {
        "annotation" => {
            let button = json!({ "tagName": "button", "selector": "#sign-in", "rect": { "x": 120, "y": 180, "width": 160, "height": 36 } });
            ui.annotation = Some(Annotation { tab, tool: Tool::Region, targets: vec![Target::Element(button), Target::Region], anchor: Some([100., 160., 260., 120.]), drag: None, capturing: None });
            ui.comment = TextEdit::new("Match the sign-in button to the brand blue");
        }
        "rails" => {
            status.device = Some((390, 844));
            fresh.device.preset = Some(2);
            fresh.device.w.replace_text("390");
            fresh.device.h.replace_text("844");
        }
        "host-setup" => {
            fresh.stream = streamed(Some(("The server's machine is missing the libraries Chrome needs.".into(), Some("npx playwright install-deps chromium".into()))));
        }
        "clipboard-prompt" => ui.prompt = Some(Prompt { tab, ask: Ask::ClipboardRead { id: 1, origin: "http://localhost:5173".into() } }),
        "external-prompt" => ui.prompt = Some(Prompt { tab, ask: Ask::External { url: "mailto:support@example.com?subject=Login%20help".into() } }),
        "file-not-shown" | "server-unreachable" => {
            fresh.stream = streamed(None);
            let nav = if kind == "file-not-shown" {
                json!({ "_tag": "LoadFailed", "url": "http://localhost:5173/export/report.zip", "title": "", "code": -3, "description": "", "download": { "id": "d1", "fileName": "report.zip" } })
            } else {
                json!({ "_tag": "LoadFailed", "url": "http://localhost:5173/login", "title": "", "code": -102, "description": "ERR_CONNECTION_REFUSED" })
            };
            fresh.snapshot = json!({ "profileId": "default", "navStatus": nav });
            status = stream_status(&fresh.snapshot, &fresh.stream.as_ref().map(|s| s.state()).unwrap_or_default());
        }
        _ => {
            fresh.stream = streamed(None);
            fresh.snapshot = json!({ "profileId": "work", "navStatus": { "_tag": "Success", "url": url } });
        }
    }
    if fresh.stream.is_some() {
        fresh.remote = Some("remote".into());
    }
    match kind {
        "stream-dialog" => {
            if let Some((kind, message, _)) = dialog.clone() {
                ui.dialog = Some(Dialog { tab, kind, message, input: TextEdit::new("") });
            }
            fresh.stream_dialog = dialog;
        }
        "more" => ui.menu = true,
        "chooser" => {
            let files = ["login-before.png", "login-after.png"].map(|n| std::path::PathBuf::from("/home/dev/shots").join(n)).to_vec();
            ui.chooser = Some(Chooser { tab, id: "chooser-1".into(), accept: vec!["png".into(), "jpg".into()], files });
        }
        _ => {}
    }
    // A snapshot draws one frame, before the page area reports its size.
    ui.area = (539., 731.);
    ui.tabs.insert(tab, fresh);
    ui.browser.shared.lock().unwrap().tabs.insert(tab, status);
    tab
}

/// A two-page PDF for the `browser-pdf-chrome` scene, its text in Helvetica.
fn scene_pdf_bytes() -> Vec<u8> {
    let page = |lines: &[&str]| {
        let body: String = lines.iter().enumerate().map(|(i, l)| format!("BT /F1 {} Tf 72 {} Td ({l}) Tj ET\n", if i == 0 { 28 } else { 14 }, 700 - i * 34)).collect();
        format!("<< /Length {} >>\nstream\n{body}endstream", body.len())
    };
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R /Resources << /Font << /F1 7 0 R >> >> >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 6 0 R /Resources << /Font << /F1 7 0 R >> >> >>".to_owned(),
        page(&["Quarterly report", "Revenue grew 18% to $4.2M.", "Rendered by pdf.js in the panel's Chrome.", "Page one of two."]),
        page(&["Appendix", "Page two of two."]),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes());
    out
}

/// The `browser-permissions-chrome` page: what a page finds it may use.
const SCENE_PERMISSIONS: &str = r#"<!doctype html><title>Permissions</title><style>body{font:16px system-ui,sans-serif;margin:24px;background:#fff;color:#111}td{padding:6px 14px 6px 0}</style>
<h2>What this page may use</h2><table id="t"></table><script>
(async () => {
  const t = document.getElementById("t");
  const row = (k, v) => t.insertAdjacentHTML("beforeend", "<tr><td>" + k + "</td><td><b>" + v + "</b></td></tr>");
  for (const name of ["geolocation", "notifications", "camera", "microphone", "clipboard-write"]) {
    try { row(name, (await navigator.permissions.query({ name })).state); } catch (e) { row(name, "error"); }
  }
  row("Notification.requestPermission()", await Notification.requestPermission());
  row("getCurrentPosition()", await new Promise((r) => navigator.geolocation.getCurrentPosition(() => r("a position"), (e) => r("error " + e.code + (e.code === 1 ? " (denied)" : e.code === 2 ? " (unavailable)" : "")), { timeout: 5000 })));
  document.title = "Permissions ready";
})();
</script>"#;

/// The scenes' page server: a PDF at an address without `.pdf`, a page behind a sign-in, a
/// popup opener and its popup, and a permissions probe.
fn scene_server() -> Option<u16> {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = [0u8; 4096];
            let read = stream.read(&mut request).unwrap_or(0);
            let request = String::from_utf8_lossy(&request[..read]);
            let path = request.split(' ').nth(1).unwrap_or("/");
            let (status, headers, body) = match path {
                "/reports/q3" => ("200 OK", "Content-Type: application/pdf\r\n", scene_pdf_bytes()),
                "/opener" => ("200 OK", "Content-Type: text/html\r\n", b"<!doctype html><title>Popup opener</title><button style=\"position:fixed;inset:0;font:24px system-ui\" onclick=\"window.open('/popped', 'p', 'width=500,height=400')\">Open a 500x400 popup</button>".to_vec()),
                "/popped" => ("200 OK", "Content-Type: text/html\r\n", b"<!doctype html><title>The popup</title><body style=\"margin:0;display:grid;place-items:center;height:100vh;background:#dbeafe;font:28px system-ui\"><p>The popup painted</p>".to_vec()),
                "/staging" => ("401 Unauthorized", "Content-Type: text/html\r\nWWW-Authenticate: Basic realm=\"Staging\"\r\n", b"<!doctype html><title>401 Unauthorized</title><body style=\"font:16px system-ui,sans-serif;margin:24px\"><h1>401 Unauthorized</h1><p>The staging server's own page, shown once the sign-in is cancelled.</p>".to_vec()),
                _ => ("200 OK", "Content-Type: text/html\r\n", SCENE_PERMISSIONS.as_bytes().to_vec()),
            };
            let head = format!("HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(&body));
        }
    });
    Some(port)
}

/// The shown tab's status once `done` holds, or after a minute.
fn scene_wait(app: &App, tab: TabId, done: impl Fn(&Status) -> bool) -> Status {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        let status = app.ui.panel.browser.browser.shared.lock().unwrap().tabs.get(&tab).cloned().unwrap_or_default();
        if done(&status) || std::time::Instant::now() > deadline {
            return status;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Scenes with Chrome (`S5_CHROME`): a real tab loads `path` from `scene_server` and waits for
/// `title`, since a headless run's clock does not wait on Chrome.
pub fn scene_page(app: &mut App, path: &str, title: &str) -> Option<TabId> {
    let port = scene_server()?;
    super::html_frame::scene_waker(app);
    let ui = &mut app.ui.panel.browser;
    ui.area = (539., 731.);
    ui.chrome_scale = Some(1.);
    ui.browser.send(Cmd::Viewport { w: 539., h: 731., scale: 1. });
    let tab = open_tab(app, browser::DEFAULT_PROFILE);
    let ui = &mut app.ui.panel.browser;
    ui.browser.send(Cmd::Show(Some(tab)));
    ui.browser.send(Cmd::Navigate(tab, format!("http://127.0.0.1:{port}{path}")));
    let status = scene_wait(app, tab, |s| s.title == title && s.painted && !s.loading);
    // pdf.js draws after the load; its frames follow.
    std::thread::sleep(Duration::from_secs(3));
    eprintln!("scene: tab at {} titled {:?}, painted {}, failure {:?}", status.url, status.title, status.painted, status.failure.map(|f| f.description));
    Some(tab)
}

/// The live check's popup: a click opens `window.open(url, name, "width=500,height=400")`, which
/// the panel adopts as a tab that must paint without a tab switch.
pub fn scene_popup(app: &mut App) -> Option<TabId> {
    let opener = scene_page(app, "/opener", "Popup opener")?;
    for kind in ["mousePressed", "mouseReleased"] {
        app.ui.panel.browser.browser.send(Cmd::Mouse { kind, x: 200., y: 200., clicks: 1 });
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let popup = loop {
        poll(app, &mut Tx::default());
        if let Some(tab) = app.ui.panel.browser.tabs.keys().copied().find(|t| *t != opener) {
            break tab;
        }
        if std::time::Instant::now() > deadline {
            eprintln!("scene: no popup opened");
            return Some(opener);
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    poll(app, &mut Tx::default());
    let status = scene_wait(app, popup, |s| s.title == "The popup" && s.painted && !s.loading);
    std::thread::sleep(Duration::from_secs(2));
    poll(app, &mut Tx::default());
    eprintln!("scene: popup tab at {} titled {:?}, painted {}", status.url, status.title, status.painted);
    Some(popup)
}

/// The profile a tab shows pages in: a server tab's as its server reports it.
fn tab_profile(app: &App, tab: TabId) -> String {
    let Some(t) = app.ui.panel.browser.tabs.get(&tab) else { return browser::DEFAULT_PROFILE.to_owned() };
    t.snapshot["profileId"].as_str().map_or_else(|| t.profile.clone(), str::to_owned)
}

/// Clears a profile's data on this computer, or with `removed`, deletes it once no tab uses it.
pub fn clear_profile(app: &mut App, profile: &str, removed: bool) {
    app.ui.panel.browser.browser.clear(profile, removed);
}

/// Tells the server each session's tab is attached once Chrome has its page, and again for a
/// new server process or the new page after Chrome restarts.
fn announce(app: &mut App) {
    let ui = &mut app.ui.panel.browser;
    if !ui.tabs.values().any(|t| t.session.is_some()) {
        return;
    }
    let generation = agent::generation();
    // Each profile's Chrome has its own DevTools address.
    let targets: HashMap<TabId, (String, String)> = {
        let shared = ui.browser.shared.lock().unwrap();
        shared.tabs.iter().filter_map(|(id, s)| Some((*id, (s.target.clone()?, s.host.clone()?)))).collect()
    };
    for (id, tab) in ui.tabs.iter_mut() {
        let (Some(key), Some((target, host))) = (&tab.session, targets.get(id)) else { continue };
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

/// Starts web's annotation session on `tab`, element picking first, or ends it.
fn toggle_annotation(app: &mut App, tab: TabId) {
    if app.ui.panel.browser.annotation.as_ref().is_some_and(|a| a.tab == tab) {
        return end_annotation(app);
    }
    end_annotation(app);
    let ui = &mut app.ui.panel.browser;
    ui.annotation = Some(Annotation { tab, tool: Tool::Select, targets: vec![], anchor: None, drag: None, capturing: None });
    ui.comment = TextEdit::new("");
    send(app, Cmd::Pick(tab, true));
}

fn end_annotation(app: &mut App) {
    let Some(a) = app.ui.panel.browser.annotation.take() else { return };
    send(app, Cmd::Pick(a.tab, false));
    send(app, Cmd::Mark(a.tab, "clear()".into()));
}

/// Desktop's tool keys and buttons: V, R, D and E.
fn set_tool(app: &mut App, tool: Tool) {
    let Some(a) = app.ui.panel.browser.annotation.as_mut().filter(|a| a.capturing.is_none()) else { return };
    a.tool = tool;
    a.drag = None;
    let tab = a.tab;
    send(app, Cmd::Pick(tab, tool == Tool::Select));
}

/// A picked element joins the annotation, boxed in the page, and picking carries on.
fn pick(app: &mut App, tab: TabId, element: Value, _image: Option<Vec<u8>>) {
    let r = &element["rect"];
    let rect = [&r["x"], &r["y"], &r["width"], &r["height"]].map(|v| v.as_f64().unwrap_or(0.) as f32);
    let ui = &mut app.ui.panel.browser;
    if !ui.annotation.as_ref().is_some_and(|a| a.tab == tab) {
        ui.annotation = Some(Annotation { tab, tool: Tool::Select, targets: vec![], anchor: None, drag: None, capturing: None });
        ui.comment = TextEdit::new("");
    }
    let Some(a) = ui.annotation.as_mut() else { return };
    a.targets.push(Target::Element(element));
    a.anchor = Some(rect);
    let select = a.tool == Tool::Select && a.capturing.is_none();
    ui.comment.focused = true;
    send(app, Cmd::Mark(tab, format!("element({}, {}, {}, {})", rect[0], rect[1], rect[2], rect[3])));
    if select {
        send(app, Cmd::Pick(tab, true));
    }
}

/// A region drag or a stroke on the page, in viewport CSS pixels.
fn mark_drag(s: &mut App, d: &rune::Drag, tab: TabId, scale: f32) {
    let Some(a) = s.ui.panel.browser.annotation.as_mut().filter(|a| a.tab == tab && a.capturing.is_none()) else { return };
    let scale = scale.max(0.01);
    let p = (d.x / scale, d.y / scale);
    let tool = a.tool;
    let mut calls = vec![];
    match (tool, d.phase) {
        (Tool::Erase, Phase::Down) => {
            send(s, Cmd::Erase(tab, p.0, p.1));
            return;
        }
        (Tool::Erase, _) | (Tool::Select, _) => return,
        (_, Phase::Down) => {
            a.drag = Some(Stroke { start: p, last: p, min: p, max: p, points: 1 });
            if tool == Tool::Draw {
                calls.push(format!("begin({}, {})", p.0, p.1));
            }
        }
        (_, Phase::Move) if !d.cancelled() => {
            let Some(drag) = a.drag.as_mut() else { return };
            drag.last = p;
            drag.min = (drag.min.0.min(p.0), drag.min.1.min(p.1));
            drag.max = (drag.max.0.max(p.0), drag.max.1.max(p.1));
            drag.points += 1;
            if tool == Tool::Draw {
                calls.push(format!("point({}, {})", p.0, p.1));
            }
        }
        _ => {
            let Some(drag) = a.drag.take() else { return };
            match tool {
                // As desktop's marquee: the elements inside it, or the region when it holds none.
                Tool::Region if !d.cancelled() => {
                    if let Some(rect) = region_rect(drag.start, drag.last) {
                        send(s, Cmd::Enclosed(tab, rect));
                    }
                }
                Tool::Draw => {
                    calls.push("end()".into());
                    if drag.points > 1 {
                        a.targets.push(Target::Stroke);
                        a.anchor = Some([drag.min.0, drag.min.1, drag.max.0 - drag.min.0, drag.max.1 - drag.min.1]);
                    }
                }
                _ => {}
            }
            s.ui.panel.browser.comment.focused = true;
        }
    }
    for call in calls {
        send(s, Cmd::Mark(tab, call));
    }
}

/// What a region drag enclosed becomes the annotation's elements, or one region.
fn enclosed(app: &mut App, tab: TabId, rect: [f32; 4], elements: Vec<Value>) {
    let Some(a) = app.ui.panel.browser.annotation.as_mut().filter(|a| a.tab == tab && a.capturing.is_none()) else { return };
    let mut calls = vec![];
    if elements.is_empty() {
        a.targets.push(Target::Region);
        calls.push(format!("region({}, {}, {}, {})", rect[0], rect[1], rect[2], rect[3]));
    }
    for element in elements {
        let r = &element["rect"];
        let [x, y, w, h] = [&r["x"], &r["y"], &r["width"], &r["height"]].map(|v| v.as_f64().unwrap_or(0.) as f32);
        calls.push(format!("element({x}, {y}, {w}, {h})"));
        a.targets.push(Target::Element(element));
    }
    a.anchor = Some(rect);
    app.ui.panel.browser.comment.focused = true;
    for call in calls {
        send(app, Cmd::Mark(tab, call));
    }
}

/// Desktop's `isUsableRect`: a region at least 4 CSS pixels each way, whichever way it was dragged.
fn region_rect(a: (f32, f32), b: (f32, f32)) -> Option<[f32; 4]> {
    let (x, y) = (a.0.min(b.0), a.1.min(b.1));
    let (w, h) = ((a.0 - b.0).abs(), (a.1 - b.1).abs());
    (w >= 4. && h >= 4.).then_some([x, y, w, h])
}

/// Enter attaches the annotation and its screenshot; Cmd/Ctrl+Enter sends it too.
fn submit_annotation(app: &mut App, send_now: bool) {
    let Some(a) = app.ui.panel.browser.annotation.as_mut().filter(|a| !a.targets.is_empty() && a.capturing.is_none()) else { return };
    a.capturing = Some(send_now);
    a.drag = None;
    let tab = a.tab;
    send(app, Cmd::Pick(tab, false));
    send(app, Cmd::CaptureMarks(tab));
}

/// The screenshot arrived: the annotation goes to the composer, and is sent when asked.
fn annotated(app: &mut App, tab: TabId, image: Option<Vec<u8>>) {
    let ui = &mut app.ui.panel.browser;
    let Some(send_now) = ui.annotation.as_ref().filter(|a| a.tab == tab).and_then(|a| a.capturing) else { return };
    let Some(a) = ui.annotation.take() else { return };
    let comment = ui.comment.text.trim().to_string();
    let status = ui.browser.status(tab);
    send(app, Cmd::Mark(tab, "clear()".into()));
    attach(app, (&status.url, &status.title), &a.targets, image, &comment, send_now);
    if send_now {
        crate::chat::composer::submit(app);
    }
}

/// An annotation goes to the composer as web's preview annotation, its screenshot as an image.
fn attach(app: &mut App, page: (&str, &str), targets: &[Target], image: Option<Vec<u8>>, comment: &str, quiet: bool) {
    let id = s5_net::uuid();
    let image = image.and_then(|bytes| {
        let dir = crate::host::t3_home().join("userdata").join("browser-artifacts");
        std::fs::create_dir_all(&dir).ok()?;
        // Web's sent bubble finds an annotation's crop by this name (TimelineUserMessageRow).
        let path = dir.join(format!("preview-annotation-{id}.png"));
        std::fs::write(&path, bytes).ok()?;
        Some(path.display().to_string())
    });
    let record = annotation(&id, page, targets, comment);
    let detail = record["targetSummary"].as_str().unwrap_or_default().to_string();
    crate::chat::composer::add_context(app, record);
    if let Some(image) = image {
        crate::chat::composer::attach_path(app, image);
    }
    if !quiet {
        crate::shell::palette::toast(app, "Annotation added to the composer", Some(&detail));
    }
}

/// Web's `previewAnnotationTargetSummary`: "2 selected elements, 1 marked region, 1 drawing".
fn target_summary(targets: &[Target]) -> String {
    let count = |f: fn(&Target) -> bool| targets.iter().filter(|t| f(t)).count();
    let parts = [
        (count(|t| matches!(t, Target::Element(_))), "selected element"),
        (count(|t| *t == Target::Region), "marked region"),
        (count(|t| *t == Target::Stroke), "drawing"),
    ];
    parts.iter().filter(|(n, _)| *n > 0).map(|(n, noun)| format!("{n} {noun}{}", if *n == 1 { "" } else { "s" })).collect::<Vec<_>>().join(", ")
}

/// `previewAnnotationContextRecord` for an annotation's targets and comment.
fn annotation(id: &str, (url, title): (&str, &str), targets: &[Target], comment: &str) -> Value {
    let bounded = |v: &Value, max: usize| v.as_str().map(|s| s.chars().take(max).collect::<String>());
    let elements: Vec<&Value> = targets.iter().filter_map(|t| if let Target::Element(e) = t { Some(e) } else { None }).collect();
    let first = elements.first().copied().cloned().unwrap_or_else(|| json!({ "pageUrl": url, "pageTitle": title }));
    let page_url = bounded(&first["pageUrl"], 2048).unwrap_or_default();
    let page_title = bounded(&first["pageTitle"], 2048).filter(|t| !t.trim().is_empty());
    let label = page_title.as_deref().unwrap_or("Preview annotation").replace(['[', ']', '\\', '\r', '\n'], " ");
    let label: String = label.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
    let details: Vec<Value> = elements
        .iter()
        .take(50)
        .map(|element| {
            json!({
                "pageUrl": bounded(&element["pageUrl"], 2048).unwrap_or_else(|| page_url.clone()),
                "pageTitle": bounded(&element["pageTitle"], 2048).filter(|t| !t.trim().is_empty()),
                "tagName": bounded(&element["tagName"], 255).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "element".into()),
                "selector": bounded(&element["selector"], 2048),
                "htmlPreview": bounded(&element["htmlPreview"], 8000).unwrap_or_default(),
                "componentName": bounded(&element["componentName"], 2048),
                "source": null,
                "styles": bounded(&element["styles"], 8000).unwrap_or_default(),
            })
        })
        .collect();
    let mut record = json!({
        "version": 1,
        "contextId": format!("preview-annotation_{id}"),
        "kind": "preview-annotation",
        "label": label,
        "annotationId": id,
        "pageUrl": page_url,
        "pageTitle": page_title,
        "comment": comment.chars().take(8000).collect::<String>(),
        "targetSummary": target_summary(targets),
        "styleChanges": [],
        "styleChangeDetails": [],
    });
    if !details.is_empty() {
        record["elementIds"] = json!((1..=details.len()).map(|n| format!("element-{n}")).collect::<Vec<_>>());
        record["elements"] = json!(details);
    }
    let regions = targets.iter().filter(|t| **t == Target::Region).count();
    let strokes = targets.iter().filter(|t| **t == Target::Stroke).count();
    if regions > 0 {
        record["regionCount"] = json!(regions);
    }
    if strokes > 0 {
        record["strokeCount"] = json!(strokes);
    }
    record
}

/// Records `previewFocus` before any shortcut resolves, so `when` clauses see it on this key.
pub fn note_focus(app: &mut App, tx: &Tx) {
    app.ui.panel.browser.focused = panel_tab(app).is_some() && tx.focused().is_some_and(|k| k.contains("browser-"));
}

/// The preview shortcuts: `preview.toggle` anywhere, the rest while the browser has focus.
pub fn on_key(app: &mut App, e: &KeyEvent, _tx: &mut Tx) -> bool {
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
    *device(app, tab) = Device::default();
    // The saved default size, else responsive at the page's own size, under the toolbar it opens.
    if let Some((w, h)) = default_viewport(app) {
        set_device(app, tab, w, h, PRESETS.iter().position(|p| (p.1, p.2) == (w, h)));
        return;
    }
    let (w, h) = app.ui.panel.browser.area;
    let (w, h) = if w >= 1. { clamp_size(w.round() as u32, (h - DEVICE_TOOLBAR_H).max(1.).round() as u32) } else { (1024, 768) };
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

/// A device rail: which sides of the page it moves.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Edge {
    West,
    East,
    South,
    SouthWest,
    SouthEast,
}

impl Edge {
    fn east(self) -> bool {
        matches!(self, Edge::East | Edge::SouthEast)
    }
    fn west(self) -> bool {
        matches!(self, Edge::West | Edge::SouthWest)
    }
    fn south(self) -> bool {
        matches!(self, Edge::South | Edge::SouthWest | Edge::SouthEast)
    }
}

const VIEWPORT_MIN: f32 = 240.;
const VIEWPORT_MAX: f32 = 3840.;
const VIEWPORT_MAX_AREA: f32 = 3840. * 2160.;

/// Web's `resizeFreeformViewport`: a CSS size moved by a delta in window points at `zoom`,
/// within the server's limits, keeping a locked width / height ratio.
fn resize_freeform(start: (u32, u32), delta: (f32, f32), zoom: f32, edge: Edge, ratio: Option<f32>) -> (u32, u32) {
    let zoom = if zoom.is_finite() && zoom > 0. { zoom } else { 1. };
    let dx = if edge.east() { delta.0 } else if edge.west() { -delta.0 } else { 0. };
    let dy = if edge.south() { delta.1 } else { 0. };
    let (sw, sh) = (start.0 as f32, start.1 as f32);
    let (want_w, want_h) = (sw + dx / zoom, sh + dy / zoom);
    if let Some(ratio) = ratio.filter(|r| r.is_finite() && *r > 0.) {
        let by_width = match (dx != 0. || matches!(edge, Edge::East | Edge::West), dy != 0. || edge == Edge::South) {
            (true, false) => true,
            (false, true) => false,
            _ => ((want_w - sw) / sw).abs() >= ((want_h - sh) / sh).abs(),
        };
        return at_ratio(if by_width { want_w } else { want_h }, ratio, by_width);
    }
    let clamp = |v: f32| v.round().clamp(VIEWPORT_MIN, VIEWPORT_MAX);
    let (mut w, mut h) = (clamp(want_w), clamp(want_h));
    if w * h > VIEWPORT_MAX_AREA {
        if dx.abs() >= dy.abs() {
            w = (VIEWPORT_MAX_AREA / h).floor().max(VIEWPORT_MIN);
        } else {
            h = (VIEWPORT_MAX_AREA / w).floor().max(VIEWPORT_MIN);
        }
    }
    (w as u32, h as u32)
}

/// Web's `resizeAtAspectRatio`: the driving side clamped, the other following the ratio.
fn at_ratio(desired: f32, ratio: f32, by_width: bool) -> (u32, u32) {
    let k = if by_width { ratio } else { 1. / ratio };
    let min = VIEWPORT_MIN.max(VIEWPORT_MIN * k).ceil();
    let max = VIEWPORT_MAX.min(VIEWPORT_MAX * k).min((VIEWPORT_MAX_AREA * k).sqrt()).floor();
    let mut side = desired.round().max(min).min(max);
    let mut other = (side / k).round();
    while side * other > VIEWPORT_MAX_AREA && side > min {
        side -= 1.;
        other = (side / k).round();
    }
    if by_width { (side as u32, other as u32) } else { (other as u32, side as u32) }
}

/// Web's `resizeFromEndRail`: a centred page grows on both sides, so its edge moves half as far
/// until it meets the area's edge.
fn from_end_rail(start: f32, delta: f32, available: f32) -> f32 {
    let edge = if start < available { (available + start) / 2. } else { start };
    let target = edge + delta;
    if target <= available { target * 2. - available } else { target }
}

fn from_start_rail(start: f32, delta: f32, available: f32) -> f32 {
    if start > available {
        let fit = start - available;
        return if delta <= fit { start - delta } else { available - (delta - fit) * 2. };
    }
    let target = (available - start) / 2. + delta;
    if target >= 0. { available - target * 2. } else { available - target }
}

/// Web's `resizeBrowserViewportFromRail`: the size a rail drag of `delta` window points makes,
/// for a page drawn at `scale` window points per CSS pixel in an `available` area.
fn from_rail(start: (u32, u32), delta: (f32, f32), available: (f32, f32), scale: f32, edge: Edge, ratio: Option<f32>) -> (u32, u32) {
    let scale = if scale.is_finite() && scale > 0. { scale } else { 1. };
    let (sw, sh) = (start.0 as f32 * scale, start.1 as f32 * scale);
    let want_w = if edge.east() { from_end_rail(sw, delta.0, available.0) } else if edge.west() { from_start_rail(sw, delta.0, available.0) } else { sw };
    let want_h = if edge.south() { from_end_rail(sh, delta.1, available.1) } else { sh };
    let (dw, dh) = (want_w - sw, want_h - sh);
    resize_freeform(start, (if edge.west() { -dw } else { dw }, dh), scale, edge, ratio)
}

fn rail_drag(s: &mut App, d: &rune::Drag, tab: TabId, edge: Edge) {
    let status = s.ui.panel.browser.browser.status(tab);
    let Some(size) = status.device else { return };
    let ui = &mut s.ui.panel.browser;
    let start = match ui.rail {
        Some((t, e, start)) if t == tab && e == edge && d.phase != Phase::Down => start,
        _ => size,
    };
    ui.rail = Some((tab, edge, start));
    let (aw, ah) = browser::device_area(ui.area.0, ui.area.1);
    let scale = browser::device_box(aw, ah, start, status.zoom).2;
    let streamed = ui.tabs.get(&tab).is_some_and(|t| t.stream.is_some());
    let next = if d.cancelled() { start } else { from_rail(start, (d.dx, d.dy), (aw, ah), scale, edge, device(s, tab).lock) };
    let done = d.phase == Phase::Up || d.cancelled();
    if done {
        s.ui.panel.browser.rail = None;
    }
    // Chrome here follows the drag; a server's session takes the size once, on release.
    if next != size && (!streamed || done) {
        let preset = PRESETS.iter().position(|p| (p.1, p.2) == next);
        set_device(s, tab, next.0, next.1, preset);
    } else if streamed && !done {
        let d = device(s, tab);
        d.w.replace_text(next.0.to_string());
        d.h.replace_text(next.1.to_string());
    }
}

/// Web's arrow keys on a rail: 10 CSS pixels a press, 50 with Shift.
fn rail_key(s: &mut App, e: &KeyEvent, tab: TabId, edge: Edge) -> bool {
    let Some(size) = s.ui.panel.browser.browser.status(tab).device else { return false };
    let step = if e.mods.shift { 50. } else { 10. };
    let delta = match e.key {
        Key::Left if edge != Edge::South => (-step, 0.),
        Key::Right if edge != Edge::South => (step, 0.),
        Key::Up if edge.south() => (0., -step),
        Key::Down if edge.south() => (0., step),
        _ => return false,
    };
    let next = resize_freeform(size, delta, 1., edge, device(s, tab).lock);
    if next != size {
        let preset = PRESETS.iter().position(|p| (p.1, p.2) == next);
        set_device(s, tab, next.0, next.1, preset);
    }
    true
}

/// Web's `BrowserViewportResizeHandles`: rails left, right and below a device-size page, and
/// its two lower corners, each dragged or moved with the arrow keys.
fn rails(app: &App, t: &T, tab: TabId, page: El, (w, h): (f32, f32)) -> El {
    let active = app.ui.panel.browser.rail.filter(|r| r.0 == tab).map(|r| r.1);
    let rail = |edge: Edge, rw: f32, rh: f32| {
        let ink = if active == Some(edge) { t.c.foreground } else { t.c.muted_foreground.alpha_mul(0.55) };
        let line = |lw: f32, lh: f32| el().size(lw, lh).radius(0.5).fill(ink);
        let (grip, cursor, label) = match edge {
            Edge::West => (row().gap(1.).child(line(1., 24.)).child(line(1., 24.)), rune::Cursor::ResizeColumn, "Resize browser viewport from left edge. Use arrow keys to resize."),
            Edge::East => (row().gap(1.).child(line(1., 24.)).child(line(1., 24.)), rune::Cursor::ResizeColumn, "Resize browser viewport from right edge. Use arrow keys to resize."),
            Edge::South => (col().gap(1.).child(line(24., 1.)).child(line(24., 1.)), rune::Cursor::ResizeRow, "Resize browser viewport from bottom edge. Use arrow keys to resize."),
            Edge::SouthWest => (el().size(4., 4.).radius(2.).fill(ink), rune::Cursor::Move, "Resize browser viewport from bottom-left corner. Use arrow keys to resize."),
            Edge::SouthEast => (el().size(4., 4.).radius(2.).fill(ink), rune::Cursor::Move, "Resize browser viewport from bottom-right corner. Use arrow keys to resize."),
        };
        el().key(format!("browser-rail-{edge:?}"))
            .size(rw, rh)
            .center()
            .cursor(cursor)
            .focusable(true)
            .role("button")
            .label(label)
            .child(grip.a11y_hidden())
            .on_drag(move |s: &mut App, d, _| rail_drag(s, d, tab, edge))
            .on_key(move |s: &mut App, e: &KeyEvent, _: &mut Tx| rail_key(s, e, tab, edge))
    };
    let rail_w = browser::RAIL;
    col().child(row().child(rail(Edge::West, rail_w, h)).child(page).child(rail(Edge::East, rail_w, h))).child(row().child(rail(Edge::SouthWest, rail_w, rail_w)).child(rail(Edge::South, w, rail_w)).child(rail(Edge::SouthEast, rail_w, rail_w)))
}

/// Web's locked ratio while typing: the other side's draft follows each keystroke.
fn follow_lock(app: &mut App, tab: TabId, height_edited: bool) {
    let d = device(app, tab);
    let Some(ratio) = d.lock else { return };
    let (from, to) = if height_edited { (&d.h, &d.w) } else { (&d.w, &d.h) };
    let Some(n) = from.text.trim().parse::<u32>().ok().filter(|n| *n > 0) else { return };
    let other = locked_side(n, ratio, height_edited).to_string();
    if to.text != other {
        if height_edited { d.w.replace_text(other) } else { d.h.replace_text(other) }
    }
}

fn locked_side(n: u32, ratio: f32, height_edited: bool) -> u32 {
    (if height_edited { n as f32 * ratio } else { n as f32 / ratio }).round().max(1.) as u32
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
    let annotating = ui.annotation.as_ref().is_some_and(|a| a.tab == tab);
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
        // Web's profile badge, for a tab outside the profile new tabs use.
        .children(profile_badge(app, t, tab))
        .child(row().grow(1.).min_w(0.).hover_group().child(address).children(external))
        // Web offers neither on a server's tab: the page isn't in this client.
        .children(streamed.is_none().then(|| {
            let help = if status.failure.is_some() {
                "Page didn't load. Pick is unavailable until the page renders"
            } else if annotating {
                "Cancel annotation (Esc)"
            } else {
                "Annotate an element"
            };
            toggle(t, if annotating { "Cancel annotation" } else { "Annotate preview" }, &icons::MOUSE_POINTER_CLICK, annotating, !status.painted || !live, move |s: &mut App| toggle_annotation(s, tab)).help(help)
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
        let (aw, ah) = browser::device_area(ui.area.0, ui.area.1);
        let (w, h, scale) = browser::device_box(aw, ah, size, status.zoom);
        let page = page(&state.pixels, "browser-page", 1.).size(w, h).border(1., t.c.border.alpha_mul(0.7));
        let page = page.children(hover_box(t, &status, scale, (w, h))).children(annotation_editor(app, t, tab, scale, (w, h))).children(agent_cursor(app, t, tab, scale));
        col().grow(1.).min_h(0.).w_full().center().child(rails(app, t, tab, page, (w, h)))
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
            .children(ui.dialog.as_ref().filter(|d| d.tab == tab).map(|d| js_dialog(t, d, &status.url)))
            .children(ui.chooser.as_ref().filter(|c| c.tab == tab).map(|c| chooser_sheet(t, c)))
            .children(ui.prompt.as_ref().filter(|p| p.tab == tab).map(|p| permission_prompt(t, p))),
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
            // A 16-point line centred in the 24-point box.
            .py(4.)
            .px(6.)
            .radius(6.)
            .fill(t.c.input.alpha_mul(0.32))
            .border(1., t.c.contrast_input)
            .color(t.c.contrast_foreground)
            .caret(t.c.contrast_foreground, t.c.primary.alpha_mul(0.3))
            .on_edit(move |s: &mut App| if height { &mut device(s, tab).h } else { &mut device(s, tab).w })
            .on_input_change(move |s: &mut App, _| follow_lock(s, tab, height))
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
            let (aw, ah) = browser::device_area(ui.area.0, ui.area.1);
            let (w, h, _) = browser::device_box(aw, ah, size, status.zoom);
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

pub(super) fn pip_clamp((x, y): (f32, f32), (w, h): (f32, f32), (cw, ch): (f32, f32)) -> (f32, f32) {
    (x.max(PIP_GAP).min((cw - w - PIP_GAP).max(PIP_GAP)), y.max(PIP_GAP).min((ch - h - PIP_GAP).max(PIP_GAP)))
}

/// `resolvePreviewMiniPlayerFrame`: `[x, y, w, h]` in the chat area; a moved or resized
/// preview keeps its width, the default sits top right at up to 320 pt.
pub(super) fn pip_frame(frame: Option<[f32; 3]>, aspect: f32, container: (f32, f32)) -> [f32; 4] {
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
pub(super) fn pip_resize([x, y, w, h]: [f32; 4], (dx, dy): (i8, i8), (mx, my): (f32, f32), aspect: f32, (cw, ch): (f32, f32)) -> [f32; 4] {
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
    let state = stream.state();
    if let Some((need, command)) = state.host_setup {
        return Some(host_setup(t, need, command));
    }
    let nav = &ui.tabs.get(&tab)?.snapshot["navStatus"];
    if let (Some(url), Some(id), Some(name)) = (nav["url"].as_str().filter(|_| nav["_tag"] == "LoadFailed"), nav["download"]["id"].as_str(), nav["download"]["fileName"].as_str()) {
        return Some(file_not_shown(t, tab, url, id, name));
    }
    if let Some(pdf) = nav["url"].as_str().filter(|_| nav["_tag"] == "Success").and_then(|url| pdf_asset_url(url, stream.base())) {
        return Some(stream_pdf(t, pdf));
    }
    if !state.painted {
        return Some(col().grow(1.).min_h(0.).w_full().center().child(text("Connecting to the server's browser…").font(theme::TEXT_SM).color(t.c.muted_foreground)));
    }
    let (w, h, scale) = stream_box(ui, tab);
    let page = page(pixels, "browser-page", 1.).size(w, h).children(agent_cursor(app, t, tab, scale));
    let page = if ui.browser.status(tab).device.is_some() { rails(app, t, tab, page, (w, h)) } else { page };
    Some(col().grow(1.).min_h(0.).w_full().center().child(page))
}

/// Web embeds a workspace PDF in its own browser; this window has no PDF engine of its own, so
/// the PDF opens in the system's viewer.
fn stream_pdf(t: &T, url: String) -> El {
    let name = url.split(['?', '#']).next().unwrap_or_default().rsplit('/').next().unwrap_or_default().to_owned();
    let name = s5_net::url::form_urlencoded::parse(format!("n={}", name.replace('+', "%2B")).as_bytes()).next().map_or_else(String::new, |(_, n)| n.into_owned());
    let open = kit::Button::new("Open PDF").variant("outline").size("sm").build(t).on_press_tx(move |s: &mut App, tx: &mut Tx| {
        if let Err(e) = tx.open_url(&url) {
            crate::shell::palette::toast(s, "Unable to open the PDF", Some(&e.to_string()));
        }
    });
    let notice = col().max_w(360.).gap(10.).padding(16.).center().child(icon(&icons::FILE, 24.).color(t.c.muted_foreground)).child(text(name).font(theme::SM_MEDIUM).color(t.c.foreground).wrap()).child(text("This PDF opens in your system's viewer.").font(theme::TEXT_SM).color(t.c.muted_foreground).wrap()).child(open);
    col().key("browser-stream-pdf").grow(1.).min_h(0.).w_full().center().child(notice)
}

/// Web's `PreviewFileNotShown`: a server page that turned out to be a download.
fn file_not_shown(t: &T, tab: TabId, url: &str, id: &str, name: &str) -> El {
    let host = host(url).unwrap_or(url).to_owned();
    let message = format!("{name} from {host} was downloaded instead.");
    let (open, id, file) = (url.to_owned(), id.to_owned(), name.to_owned());
    let buttons = row()
        .w_full()
        .gap(8.)
        .center_y()
        .child(el().grow(1.))
        .child(kit::Button::new("Open in browser").variant("outline").size("sm").build(t).on_press_tx(move |s: &mut App, tx: &mut Tx| {
            if let Err(e) = tx.open_url(&open) {
                crate::shell::palette::toast(s, "Unable to open the link", Some(&e.to_string()));
            }
        }))
        .child(kit::Button::new("Download").size("sm").build(t).on_press(move |s: &mut App| {
            if let Some(stream) = s.ui.panel.browser.tabs.get(&tab).and_then(|t| t.stream.as_ref()) {
                stream.save(&id, file.clone());
            }
        }));
    // `PreviewUnreachable`'s frame: the buttons sit at the page's foot (`mt-auto pt-8`).
    let body = col()
        .w_full()
        .max_w(576.)
        .grow(1.)
        .px(32.)
        .py(64.)
        .child(icon(&ERROR_ICON, 48.).size(48., 48.).color(t.c.muted_foreground.alpha_mul(0.7)).mb(24.))
        .child(text("This file can’t be shown here").font(theme::weight(theme::TEXT_2XL, theme::SEMIBOLD)).color(t.c.contrast_foreground).mb(12.))
        .child(rich(message, vec![Span::new(0, name.len()).weight(theme::SEMIBOLD).color(t.c.contrast_foreground)]).font(theme::TEXT_SM).color(t.c.muted_foreground).w_full())
        .child(el().grow(1.).min_h(32.))
        .child(buttons);
    row().key("browser-file-not-shown").grow(1.).min_h(0.).w_full().fill(t.c.background).child(el().grow(1.)).child(body).child(el().grow(1.))
}

/// Web's `PreviewStreamHostSetup` notice: the server's machine can't run its browser until this
/// command runs there.
fn host_setup(t: &T, need: String, command: Option<String>) -> El {
    let mut notice = col().max_w(420.).gap(10.).padding(16.).child(text("The server can't open this page yet").font(theme::SM_MEDIUM).color(t.c.foreground)).child(text(need).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap());
    if let Some(command) = command {
        let copied = command.clone();
        notice = notice
            .child(text("Run this on the server's machine, then reopen the tab:").font(theme::TEXT_SM).color(t.c.muted_foreground).wrap())
            .child(row().w_full().gap(8.).center_y().padding(8.).radius(6.).fill(t.c.muted).child(text(command).font(theme::TEXT_XS).color(t.c.foreground).grow(1.).wrap().selectable(true)).child(
                kit::Button::new("Copy").variant("outline").size("xs").build(t).on_press_tx(move |s: &mut App, tx: &mut Tx| {
                    tx.copy(copied.clone());
                    crate::shell::palette::toast_copied(s, tx, "Copy command");
                }),
            ));
    }
    col().key("browser-host-setup").grow(1.).min_h(0.).w_full().center().child(notice)
}

/// The tab's appearance: a server tab's from its session.
fn tab_scheme(tab: &Tab) -> &'static str {
    if tab.stream.is_none() {
        return tab.scheme;
    }
    match tab.snapshot["colorScheme"].as_str() {
        Some("light") => "light",
        Some("dark") => "dark",
        _ => "",
    }
}

/// Web's stream control row: who drives the page, and taking or handing back control.
fn stream_bar(app: &App, t: &T, tab: TabId) -> Option<El> {
    let state = app.ui.panel.browser.tabs.get(&tab)?.stream.as_ref()?.state();
    // Nobody can drive a page the server can't open yet.
    if state.host_setup.is_some() {
        return None;
    }
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

/// Desktop's annotation chrome over the page: the tool bar, a layer taking region and drawing
/// drags, the region being dragged, and the compact editor beside the latest target with the
/// comment and Attach (Enter, or Cmd/Ctrl+Enter to send); Escape ends the session.
fn annotation_editor(app: &App, t: &T, tab: TabId, scale: f32, page: (f32, f32)) -> Vec<El> {
    let ui = &app.ui.panel.browser;
    let Some(a) = ui.annotation.as_ref().filter(|a| a.tab == tab) else { return Vec::new() };
    let mut out = vec![];
    if matches!(a.tool, Tool::Region | Tool::Draw | Tool::Erase) {
        let cursor = if a.tool == Tool::Erase { rune::Cursor::Pointer } else { rune::Cursor::Crosshair };
        out.push(el().key("browser-annotation-layer").pin(Some(0.), Some(0.), None, None).size(page.0, page.1).cursor(cursor).on_drag(move |s: &mut App, d, _| mark_drag(s, d, tab, scale)));
    }
    if let Some(rect) = a.drag.filter(|_| a.tool == Tool::Region).and_then(|d| region_rect(d.start, d.last)) {
        out.extend(hover_rect(rect, scale, page).map(|[x, y, w, h]| el().pin(Some(x), Some(y), None, None).size(w, h).radius(3.).fill(t.c.primary.alpha_mul(0.06)).border(2., t.c.primary)));
    }
    let busy = a.capturing.is_some();
    let tool = |label: &'static str, glyph: &'static rune::Icon, which: Tool| toggle(t, label, glyph, a.tool == which, busy, move |s: &mut App| set_tool(s, which)).help(label);
    let summary = if a.targets.is_empty() { "Pick elements, mark a region or draw".to_string() } else { target_summary(&a.targets) };
    out.push(
        row()
            .key("browser-annotation-tools")
            .pin(Some(8.), Some(8.), None, None)
            .h(36.)
            .px(4.)
            .gap(2.)
            .center_y()
            .radius(10.)
            .border(1., t.c.border)
            .fill(t.c.popover.alpha_mul(0.96))
            .shadow(4., 16., Color::BLACK.alpha(0.2))
            .child(tool("Select elements (V)", &icons::MOUSE_POINTER_2, Tool::Select))
            .child(tool("Mark a region (R)", &icons::SCAN, Tool::Region))
            .child(tool("Draw (D)", &icons::PENCIL, Tool::Draw))
            .child(tool("Erase a mark (E)", &icons::TRASH_2, Tool::Erase))
            .child(text(summary).font(theme::TEXT_XS).color(t.c.muted_foreground).px(6.).single_line()),
    );
    if a.targets.is_empty() {
        return out;
    }
    // Where the latest element is now, so the editor follows a scroll, as web's overlay does.
    let picked = ui.browser.status(tab).picked.filter(|_| matches!(a.targets.last(), Some(Target::Element(_))));
    let [x, y, w, h] = picked.or(a.anchor).unwrap_or([0., 0., page.0 / scale.max(0.01), 0.]);
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
            submit_annotation(s, false);
            tx.blur();
        })
        .on_input_key(|s: &mut App, e, tx| match e.key {
            Key::Enter if e.mods.cmd || e.mods.ctrl => {
                submit_annotation(s, true);
                tx.blur();
                true
            }
            Key::Escape => {
                end_annotation(s);
                tx.blur();
                true
            }
            _ => false,
        });
    let attach = kit::Button::new(if busy { "Capturing…" } else { "Attach" }).disabled(busy).on_press(|s: &mut App| submit_annotation(s, false)).build(t).help("Attach annotation and screenshot (Enter). Send with Cmd/Ctrl+Enter.");
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
            .child(attach),
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
    // Below the tool row (8 pt in, 36 tall), so the editor never covers the tools.
    let top = 8. + 36. + 8.;
    (best.0.max(8.).min((pw - ew - 8.).max(8.)), best.1.max(top).min((ph - eh - 8.).max(top)))
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
        // A mouse's back and forward buttons go back and forward, as on Electron's page; the
        // middle one reaches the page, so a link opens in a background tab.
        .pointer_buttons([rune::PointerButton::Back, rune::PointerButton::Forward, rune::PointerButton::Middle])
        .on_press_tx(|s: &mut App, tx: &mut Tx| {
            let Some(tab) = s.ui.panel.browser.shown else { return };
            match tx.button() {
                rune::PointerButton::Back => send(s, Cmd::Back(tab)),
                rune::PointerButton::Forward => send(s, Cmd::Forward(tab)),
                _ => {}
            }
        })
        .on_drag(move |s: &mut App, d, tx| {
            let button = match tx.button() {
                rune::PointerButton::Primary => "left",
                rune::PointerButton::Middle => "middle",
                _ => return,
            };
            if matches!(d.phase, Phase::Down) {
                let mods = tx.mods();
                let modifiers = u32::from(mods.alt) | u32::from(mods.ctrl) << 1 | u32::from(mods.logo || (mods.cmd && !mods.ctrl)) << 2 | u32::from(mods.shift) << 3;
                send(s, Cmd::Pointer { button, modifiers });
            }
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
            // A permission prompt blocks the page too; Escape refuses it, as Electron's default.
            if s.ui.panel.browser.prompt.as_ref().is_some_and(|p| Some(p.tab) == s.ui.panel.browser.shown) {
                if matches!(e.key, Key::Escape) {
                    answer_prompt(s, tx, false);
                }
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
            if s.ui.panel.browser.annotation.is_some() {
                let tool = match &e.key {
                    Key::Escape => {
                        end_annotation(s);
                        return true;
                    }
                    Key::Text(v) if v == "v" => Some(Tool::Select),
                    Key::Text(v) if v == "r" => Some(Tool::Region),
                    Key::Text(v) if v == "d" => Some(Tool::Draw),
                    Key::Text(v) if v == "e" => Some(Tool::Erase),
                    _ => None,
                };
                if let Some(tool) = tool {
                    set_tool(s, tool);
                    return true;
                }
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

/// The current thread's project scripts' `previewUrl`s, as web's `configuredPreviewUrls`, bounded
/// as `boundConfiguredLocalServerUrls`: loopback http(s), unique, at most 32.
fn configured_urls(app: &App) -> Vec<String> {
    let Route::Thread(thread) = app.frame_route() else { return vec![] };
    let state = app.state();
    let Some(project) = state.threads.iter().chain(&state.archived).find(|t| data::s(t, "id") == thread.as_str()).map(|t| data::s(t, "projectId").to_string()) else { return vec![] };
    let Some(scripts) = state.projects.iter().find(|p| data::s(p, "id") == project).and_then(|p| p["scripts"].as_array()) else { return vec![] };
    let mut out: Vec<String> = vec![];
    for raw in scripts.iter().filter_map(|s| s["previewUrl"].as_str()) {
        if raw.is_empty() || raw.len() > 2048 || raw.trim() != raw {
            continue;
        }
        let Ok(url) = s5_net::url::Url::parse(raw) else { continue };
        let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "0.0.0.0" | "[::1]"));
        if !matches!(url.scheme(), "http" | "https") || !loopback || url.as_str().len() > 2048 {
            continue;
        }
        let mut key = url.clone();
        key.set_fragment(None);
        if out.iter().any(|o| s5_net::url::Url::parse(o).is_ok_and(|mut u| { u.set_fragment(None); u == key })) {
            continue;
        }
        out.push(url.to_string());
        if out.len() >= 32 {
            break;
        }
    }
    out
}

fn servers_payload(app: &App) -> Value {
    let urls = configured_urls(app);
    if urls.is_empty() { json!({}) } else { json!({ "configuredUrls": urls }) }
}

/// Web's `mergeDiscoveredServers`: listening servers, a script's `previewUrl` first and opening
/// that URL once probing settles, then the rest by port.
fn previewable(response: &Value, configured: &[String]) -> Vec<(String, String, u64)> {
    let loopback = |h: &str| matches!(h, "localhost" | "127.0.0.1" | "0.0.0.0" | "::1" | "[::1]");
    let key = |host: &str, port: u64| format!("{}:{port}", if loopback(host) { "loopback" } else { host });
    let wanted: Vec<(String, String)> = configured
        .iter()
        .filter_map(|raw| {
            let u = s5_net::url::Url::parse(raw).ok()?;
            Some((key(u.host_str()?, u.port_or_known_default()? as u64), u.to_string()))
        })
        .collect();
    let probing = response["configuredUrlProbing"] == true;
    let mut rows: Vec<(bool, String, String, u64)> = response["servers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let port = v["port"].as_u64()?;
            let host = data::s(v, "host");
            let url = v["url"].as_str()?.to_string();
            let name = v["processName"].as_str().filter(|n| !n.is_empty()).unwrap_or("Listening");
            let configured = wanted.iter().find(|(k, _)| *k == key(host, port)).map(|(_, u)| u.clone());
            let url = configured.clone().filter(|_| !probing).unwrap_or(url);
            Some((configured.is_none(), url, format!("{name}\n{host}:{port}"), port))
        })
        .collect();
    rows.sort_by_key(|r| (r.0, r.3));
    rows.into_iter().map(|(_, url, label, port)| (url, label, port)).collect()
}

/// `PreviewEmptyState`: recent pages and the server's listening ports, else a hint.
fn empty(app: &App, t: &T, tab: TabId) -> El {
    let payload = app.ui.panel.browser.servers_watch.clone().unwrap_or_else(|| json!({}));
    let servers: Vec<(String, String, u64)> = match &app.src {
        data::Source::Live(net) => net.resource(SERVERS, payload).map(|v| previewable(v, &configured_urls(app))).unwrap_or_default(),
        _ => vec![],
    };
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
    let scheme = ui.tabs.get(&tab).map_or("", tab_scheme);
    let radio = |label: &'static str, value: &'static str| {
        kit::MenuEntry::item(kit::MenuItem::new(label).radio(scheme == value).on_select(move |s: &mut App| {
            if let Some(t) = s.ui.panel.browser.tabs.get_mut(&tab) {
                t.scheme = value;
            }
            s.ui.panel.browser.menu = false;
            send(s, Cmd::ColorScheme(tab, value));
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
    let profile = crate::settings::browser_profiles::profile_name(app, &tab_profile(app, tab));
    let moving = move_label(app, tab);
    let mut menu = kit::Menu::new("browser-more")
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
        .separator();
    if let Some(label) = moving {
        menu = menu
            .item(kit::MenuItem::new(label).disabled(blank).on_select(move |s: &mut App| {
                close(s);
                move_tab(s, tab);
            }))
            .separator();
    }
    menu.group_label(format!("Profile: {profile}"))
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

/// Web's `alternatePreviewRuntime`: on another machine's thread, a server tab can move to this
/// computer's browser, and a tab here to that machine's.
fn move_label(app: &App, tab: TabId) -> Option<String> {
    let t = app.ui.panel.browser.tabs.get(&tab)?;
    if t.remote.is_some() {
        return t.stream.is_some().then(|| "Open on this computer".to_owned());
    }
    let env = server_env(app)?;
    Some(format!("Open in {}'s browser", crate::env::label(app, &env)))
}

/// Opens the tab's page in the other runtime and closes it here, as web's `handleMoveTab`.
fn move_tab(app: &mut App, tab: TabId) {
    let Some(remote) = app.ui.panel.browser.tabs.get(&tab).map(|t| t.remote.is_some()) else { return };
    let url = app.ui.panel.browser.browser.status(tab).url;
    let moved = if remote {
        let profile = tab_profile(app, tab);
        let fresh = open_tab(app, &profile);
        register(app, fresh);
        fresh
    } else {
        let Some(env) = server_env(app) else { return };
        let profile = tab_profile(app, tab);
        let ui = &mut app.ui.panel.browser;
        ui.next += 1;
        let mut fresh = Tab::new(Pixels::new());
        fresh.remote = Some(env);
        fresh.profile = profile;
        ui.tabs.insert(ui.next, fresh);
        ui.next
    };
    super::activate(app, super::Surface::Browser(moved));
    if !url.is_empty() {
        navigate(app, moved, &url);
    }
    super::drop_browser(app, tab);
    close(app, tab);
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
        let mine = open_tab(&mut app, browser::DEFAULT_PROFILE);
        app.ui.panel.browser.opening.push((7, mine, thread.clone(), crate::env::LOCAL.to_owned()));
        let opened = json!({ "type": "opened", "threadId": thread, "tabId": "s", "snapshot": { "runtime": "server" } });
        session_event(&mut app, crate::env::LOCAL, opened.clone());
        assert_eq!(app.ui.panel.browser.tabs.len(), 1, "held until preview.open answers");
        assert_eq!(app.ui.panel.browser.deferred.len(), 1);

        app.ui.panel.browser.opening.clear();
        bind(&mut app, mine, (thread.clone(), "s".into()), &Value::Null);
        assert_eq!(session_tab(&app, &thread, "s"), Some(mine));
        session_event(&mut app, crate::env::LOCAL, opened);
        assert_eq!(app.ui.panel.browser.tabs.len(), 1);
    }

    #[test]
    fn a_popup_the_agents_page_opened_binds_to_the_session_the_server_adopts_it_as() {
        use super::super::Surface;
        let mut app = App::new();
        let thread = data::s(&app.state().threads[0], "id").to_string();
        let popup = open_tab(&mut app, browser::DEFAULT_PROFILE);
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
        let (a, b) = (open_tab(&mut app, browser::DEFAULT_PROFILE), open_tab(&mut app, browser::DEFAULT_PROFILE));
        for (tab, session) in [(a, "s1"), (b, "s2")] {
            let t = app.ui.panel.browser.tabs.get_mut(&tab).unwrap();
            t.session = Some((thread.clone(), session.into()));
            t.announced = Some((1, "T".into()));
        }
        app.ui.panel.browser.tabs.get_mut(&b).unwrap().adopting = Some(((thread.clone(), "s1".into()), "P".into()));
        app.ui.panel.browser.opening.push((3, a, thread.clone(), crate::env::LOCAL.to_owned()));
        app.ui.panel.browser.deferred.push((crate::env::LOCAL.to_owned(), json!({ "type": "opened" })));
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
    fn server_tab_settings_go_through_its_session() {
        assert_eq!(stream_rpc(&Cmd::Zoom(1, 1.2)), Some(("preview.adjust", json!({ "zoomFactor": 1.25 }))));
        assert_eq!(stream_rpc(&Cmd::Reload { tab: 1, hard: true }), Some(("preview.adjust", json!({ "hardReload": true }))));
        assert_eq!(stream_rpc(&Cmd::Reload { tab: 1, hard: false }), None, "a plain reload is page input");
        assert_eq!(stream_rpc(&Cmd::ClearCookies), Some(("preview.adjust", json!({ "clear": "cookies" }))));
        assert_eq!(stream_rpc(&Cmd::ColorScheme(1, "")), Some(("preview.adjust", json!({ "colorScheme": "system" }))));
        assert_eq!(stream_rpc(&Cmd::ColorScheme(1, "dark")), Some(("preview.adjust", json!({ "colorScheme": "dark" }))));
        assert_eq!(stream_rpc(&Cmd::Device(1, Some((390, 844)))), Some(("preview.resize", json!({ "viewport": { "_tag": "freeform", "width": 390, "height": 844 } }))));
        assert_eq!(stream_rpc(&Cmd::Device(1, None)), Some(("preview.resize", json!({ "viewport": { "_tag": "fill" } }))));
        assert_eq!(stream_rpc(&Cmd::Back(1)), None);
        let snapshot = json!({ "navStatus": { "_tag": "Success", "url": "https://a.dev/", "title": "" }, "zoomFactor": 1.5, "viewport": { "_tag": "preset", "width": 390, "height": 844, "presetId": "iphone-12-pro" } });
        let status = stream_status(&snapshot, &super::super::stream::State::default());
        assert_eq!((status.zoom, status.device), (1.5, Some((390, 844))));
    }

    #[test]
    fn another_machines_loopback_address_is_that_machine() {
        assert_eq!(rewrite_loopback("http://localhost:5173/app?x#y", "http://192.168.1.20:3773").unwrap(), "http://192.168.1.20:5173/app?x#y");
        assert_eq!(rewrite_loopback("http://127.0.0.1:3000/", "https://box.tail1234.ts.net").unwrap(), "http://box.tail1234.ts.net:3000/");
        assert_eq!(rewrite_loopback("http://[::1]:8080/", "http://[fd7a:115c:a1e0::5]:3773").unwrap(), "http://[fd7a:115c:a1e0::5]:8080/");
        // Already this machine, or not a loopback address: unchanged.
        assert_eq!(rewrite_loopback("http://localhost:5173/", "http://127.0.0.1:3773").unwrap(), "http://localhost:5173/");
        assert_eq!(rewrite_loopback("https://example.com/", "http://192.168.1.20:3773").unwrap(), "https://example.com/");
        // A public server can't be reached on its ports directly.
        assert!(rewrite_loopback("http://localhost:5173/", "https://t3.example.com").is_err());
    }

    #[test]
    fn a_scripts_preview_url_leads_the_listening_servers() {
        let response = json!({ "servers": [
            { "host": "127.0.0.1", "port": 8080, "url": "http://127.0.0.1:8080/", "processName": "python" },
            { "host": "localhost", "port": 5173, "url": "http://localhost:5173/", "processName": "node" },
            { "host": "127.0.0.1", "port": 3000, "url": "http://127.0.0.1:3000/", "processName": "" },
        ], "configuredUrlProbing": false });
        let rows = previewable(&response, &["http://localhost:8080/admin".into()]);
        assert_eq!(rows.iter().map(|r| (r.0.as_str(), r.2)).collect::<Vec<_>>(), vec![("http://localhost:8080/admin", 8080), ("http://127.0.0.1:3000/", 3000), ("http://localhost:5173/", 5173)]);
        assert_eq!(rows[1].1, "Listening\n127.0.0.1:3000");
        // While the server still probes the script's URL, its own address opens.
        let probing = json!({ "servers": response["servers"], "configuredUrlProbing": true });
        assert_eq!(previewable(&probing, &["http://localhost:8080/admin".into()])[0].0, "http://127.0.0.1:8080/");
    }

    #[test]
    fn device_rails_resize_like_webs() {
        // A centred page grows on both sides, so the edge travels half the size change.
        assert_eq!(from_rail((400, 600), (20., 0.), (1000., 800.), 1., Edge::East, None), (440, 600));
        assert_eq!(from_rail((400, 600), (-20., 0.), (1000., 800.), 1., Edge::West, None), (440, 600));
        assert_eq!(from_rail((400, 600), (0., 30.), (1000., 800.), 1., Edge::South, None), (400, 660));
        // Past the area's edge the page grows one-for-one; at half scale each point is two pixels.
        assert_eq!(from_rail((400, 600), (310., 0.), (1000., 800.), 1., Edge::East, None), (1010, 600));
        assert_eq!(from_rail((400, 600), (20., 0.), (1000., 800.), 0.5, Edge::East, None), (480, 600));
        // The server's limits and a locked ratio hold.
        assert_eq!(from_rail((400, 600), (-400., 0.), (1000., 800.), 1., Edge::East, None), (240, 600));
        assert_eq!(from_rail((400, 600), (20., 0.), (1000., 800.), 1., Edge::SouthEast, Some(400. / 600.)), (440, 660));
        assert_eq!(resize_freeform((3840, 2000), (0., 400.), 1., Edge::South, None), (3840, 2160));
        assert_eq!(resize_freeform((390, 844), (10., 0.), 1., Edge::West, None), (380, 844));
        assert_eq!(resize_freeform((390, 844), (0., -50.), 1., Edge::SouthEast, None), (390, 794));
        assert_eq!(browser::device_area(500., 400.), (480., 390.));
    }

    #[test]
    fn workspace_html_and_pdf_open_in_the_browser() {
        assert!(previewable_file("docs/Index.HTML") && previewable_file("a.htm?x=1") && previewable_file("/tmp/report.pdf#page=2"));
        assert!(!previewable_file("notes.md") && !previewable_file("page.html/notes"));
        assert_eq!(asset_path("./site/index.html", "/repo/"), ("/repo/site/index.html".into(), true));
        assert_eq!(asset_path("/repo/out.pdf", "/repo"), ("/repo/out.pdf".into(), true));
        assert_eq!(asset_path("/repository/out.pdf", "/repo"), ("/repository/out.pdf".into(), false));
        assert_eq!(asset_path("../other/page.html", "/repo"), ("/repo/../other/page.html".into(), false));
        // The server's own loopback address is fetched through this client's connection to it.
        let base = "https://box.tail1.ts.net:3773";
        assert_eq!(pdf_asset_url("http://127.0.0.1:3773/api/assets/t/Report.pdf?sig=1", base).as_deref(), Some("https://box.tail1.ts.net:3773/api/assets/t/Report.pdf?sig=1"));
        assert_eq!(pdf_asset_url("https://cdn.example.com/api/assets/t/a.pdf", base).as_deref(), Some("https://cdn.example.com/api/assets/t/a.pdf"));
        assert_eq!(pdf_asset_url("http://127.0.0.1:3773/api/assets/t/page.html", base), None);
        assert_eq!(pdf_asset_url("http://127.0.0.1:3773/docs/a.pdf", base), None);
    }

    #[test]
    fn a_file_inputs_accept_list_filters_the_picker() {
        assert_eq!(accept_extensions(".PNG, .jpg,image/*,"), vec!["png", "jpg"]);
        assert!(accept_extensions("image/*").is_empty());
    }

    #[test]
    fn a_multi_file_input_gathers_each_accepted_file_once() {
        let dir = std::env::temp_dir().join(format!("s5-rune-chooser-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let [a, b, c] = ["a.png", "b.JPG", "c.txt"].map(|n| dir.join(n));
        for path in [&a, &b, &c] {
            std::fs::write(path, b"x").unwrap();
        }
        let accept = accept_extensions(".png,.jpg");
        let mut files = vec![];
        choose(&mut files, [a.clone(), c.clone(), dir.clone(), b.clone()], &accept);
        choose(&mut files, [a.clone()], &accept);
        assert_eq!(files, [a.clone(), b.clone()], "the text file, the folder and the repeat are left out");
        choose(&mut files, [c.clone()], &[]);
        assert_eq!(files.len(), 3, "an input without accept takes any file");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_locked_ratio_follows_the_typed_side() {
        assert_eq!(locked_side(390, 390. / 844., false), 844);
        assert_eq!(locked_side(844, 390. / 844., true), 390);
        assert_eq!(locked_side(1, 100., false), 1, "never zero");
    }

    #[test]
    fn session_viewports_become_device_sizes() {
        assert_eq!(viewport_size(&json!({ "_tag": "preset", "width": 390, "height": 844, "presetId": "iphone" })), Some(Some((390, 844))));
        assert_eq!(viewport_size(&json!({ "_tag": "freeform", "width": 800, "height": 600 })), Some(Some((800, 600))));
        assert_eq!(viewport_size(&json!({ "_tag": "fill" })), Some(None));
        assert_eq!(viewport_size(&Value::Null), None);
    }

    #[test]
    fn a_server_page_that_failed_shows_webs_unreachable_page_unless_it_downloaded() {
        let state = super::super::stream::State { painted: true, ..Default::default() };
        let failed = json!({ "navStatus": { "_tag": "LoadFailed", "url": "http://localhost:9/", "title": "", "code": -102, "description": "ERR_CONNECTION_REFUSED" } });
        let failure = stream_status(&failed, &state).failure.expect("a failed load is unreachable");
        assert_eq!((failure.url.as_str(), failure.description.as_str()), ("http://localhost:9/", "ERR_CONNECTION_REFUSED"));
        let downloaded = json!({ "navStatus": { "_tag": "LoadFailed", "url": "http://localhost:9/a.zip", "title": "", "code": -3, "description": "", "download": { "id": "d1", "fileName": "a.zip" } } });
        assert!(stream_status(&downloaded, &state).failure.is_none(), "a download shows its own page");
        assert!(stream_status(&json!({ "navStatus": { "_tag": "Success", "url": "http://a/" } }), &state).failure.is_none());
    }

    #[test]
    fn a_server_that_cant_open_the_page_offers_no_control() {
        let t = crate::theme::theme(true);
        let mut app = App::new();
        let blocked = scene(&mut app, "host-setup");
        let live = scene(&mut app, "more");
        assert!(stream_bar(&app, &t, blocked).is_none());
        assert!(stream_bar(&app, &t, live).is_some());
    }

    #[test]
    fn a_region_drag_selects_the_elements_inside_it_or_marks_the_region() {
        let mut app = App::new();
        app.ui.panel.browser.tabs.insert(1, Tab::new(Pixels::new()));
        app.ui.panel.browser.annotation = Some(Annotation { tab: 1, tool: Tool::Region, targets: vec![], anchor: None, drag: None, capturing: None });
        let button = json!({ "tagName": "button", "rect": { "x": 10, "y": 10, "width": 40, "height": 20 } });
        let link = json!({ "tagName": "a", "rect": { "x": 60, "y": 10, "width": 30, "height": 12 } });
        enclosed(&mut app, 1, [0., 0., 100., 50.], vec![button, link]);
        enclosed(&mut app, 1, [200., 200., 40., 40.], vec![]);
        // Another tab's answer, or one after the annotation closed, adds nothing.
        enclosed(&mut app, 2, [0., 0., 10., 10.], vec![]);
        let a = app.ui.panel.browser.annotation.as_ref().unwrap();
        assert!(matches!(&a.targets[..], [Target::Element(b), Target::Element(l), Target::Region] if b["tagName"] == "button" && l["tagName"] == "a"));
        assert_eq!(a.anchor, Some([200., 200., 40., 40.]));
        assert_eq!(annotation("m", ("http://a/", "A"), &a.targets, "")["targetSummary"], "2 selected elements, 1 marked region");
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
        // Right of the element when it fits, else left, else below; never over the tool row.
        assert_eq!(editor_position([10., 10., 100., 40.], (360., 50.), (800., 600.)), (118., 52.));
        assert_eq!(editor_position([600., 10., 190., 40.], (360., 50.), (800., 600.)), (232., 52.));
        assert_eq!(editor_position([0., 10., 780., 40.], (360., 50.), (800., 600.)), (420., 58.));
        assert_eq!(editor_position([10., 200., 100., 40.], (360., 50.), (800., 600.)), (118., 200.));
        // A page smaller than the editor pins it to the margin below the tools.
        assert_eq!(editor_position([0., 0., 100., 100.], (360., 50.), (300., 40.)), (8., 52.));
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
        let page = ("http://localhost:5173/", "Demo");
        let record = annotation("abc", page, &[Target::Element(element.clone())], "Make it blue");
        assert_eq!(record["contextId"], "preview-annotation_abc");
        assert_eq!(record["comment"], "Make it blue");
        assert_eq!(record["label"], "Demo home");
        assert_eq!(record["targetSummary"], "1 selected element");
        assert_eq!(record["elementIds"], json!(["element-1"]));
        let details = &record["elements"][0];
        assert_eq!(details["selector"], "#go");
        assert!(details.get("rect").is_none() && details["source"].is_null());
        assert!(record.get("regionCount").is_none() && record.get("strokeCount").is_none());
        let untitled = annotation("x", page, &[Target::Element(json!({ "pageUrl": "http://a/", "pageTitle": "  " }))], "");
        assert_eq!(untitled["label"], "Preview annotation");
        assert!(untitled["pageTitle"].is_null());

        // Several targets: web's summary and counts, the page from the tab without elements.
        let targets = [Target::Element(element.clone()), Target::Element(element), Target::Region, Target::Stroke, Target::Stroke];
        let record = annotation("m", page, &targets, "");
        assert_eq!(record["targetSummary"], "2 selected elements, 1 marked region, 2 drawings");
        assert_eq!((record["regionCount"].clone(), record["strokeCount"].clone()), (json!(1), json!(2)));
        assert_eq!(record["elementIds"], json!(["element-1", "element-2"]));
        let marks = annotation("r", page, &[Target::Region], "Too tight");
        assert_eq!((marks["pageUrl"].clone(), marks["label"].clone()), (json!("http://localhost:5173/"), json!("Demo")));
        assert!(marks.get("elements").is_none() && marks.get("elementIds").is_none());
        assert_eq!(region_rect((50., 40.), (10., 10.)), Some([10., 10., 40., 30.]));
        assert_eq!(region_rect((10., 10.), (12., 40.)), None);
    }

    #[test]
    fn custom_search_needs_the_query_after_the_host() {
        assert_eq!(search_template("custom", "https://s.example/?q=%s"), "https://s.example/?q=%s");
        assert_eq!(search_template("custom", "https://%s.example/"), "https://duckduckgo.com/?q=%s");
        assert_eq!(search_template("bing", ""), "https://www.bing.com/search?q=%s");
    }
}
