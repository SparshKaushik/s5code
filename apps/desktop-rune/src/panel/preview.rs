//! The Browser surface, web's inline `PreviewView`: each tab's chrome row over its page, the
//! empty state, or the unreachable page. `browser` owns Chrome; this owns what the panel shows.
use std::{collections::HashMap, time::Duration};

use rune::{Color, El, Key, KeyEvent, Phase, Pixels, ScrollSpec, Span, TextEdit, Tx, col, el, icon, rich, row, text, text_input};
use serde_json::{Value, json};

use super::browser::{self, Cmd, Status, TabId};
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
}

impl Tab {
    fn new(pixels: Pixels) -> Tab {
        Tab { pixels, bar: TextEdit::new(""), focused: false, device: Device::default() }
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

struct Recent {
    url: String,
    title: String,
    at: i64,
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
    recents: Vec<Recent>,
    watching: bool,
    /// The tab floating over the chat (web's mini player).
    pub(super) pip: Option<TabId>,
    /// The page area's last size, in points.
    area: (f32, f32),
    presets: bool,
    /// When the zoom pill hides, as web's `ZoomIndicator` does 1.5 s after a change.
    zoom_until: i64,
    /// A picked element waiting for its comment, as web's annotation editor holds it.
    pending: Option<Pending>,
    comment: TextEdit,
    /// The floating preview's `[x, y, width]` in the chat area once moved or resized.
    pip_frame: Option<[f32; 3]>,
    /// The `[x, y, w, h]` a PiP drag or resize started from.
    pip_start: Option<[f32; 4]>,
    /// The chat area the floating preview stays inside, in window points.
    pip_area: [f32; 4],
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
    }
    Some(browser_import::start(source, directory, ui.browser.sender()?, waker))
}

/// Chrome could not start or its connection dropped, so an import's writes never land.
pub fn browser_failed(app: &App) -> bool {
    app.ui.panel.browser.browser.shared.lock().unwrap().fatal.is_some()
}

/// A window the page opened (`window.open`, `target=_blank`), shown as a tab, as web does.
fn adopt(app: &mut App, target: String) {
    let ui = &mut app.ui.panel.browser;
    ui.next += 1;
    let tab = ui.next;
    let pixels = Pixels::new();
    ui.tabs.insert(tab, Tab::new(pixels.clone()));
    ui.browser.send(Cmd::Adopt(tab, pixels, target));
    super::activate(app, super::Surface::Browser(tab));
}

pub fn close(app: &mut App, tab: TabId) {
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

fn device(app: &mut App, tab: TabId) -> &mut Device {
    &mut app.ui.panel.browser.tabs.entry(tab).or_insert_with(|| Tab::new(Pixels::new())).device
}

fn send(app: &mut App, cmd: Cmd) {
    app.ui.panel.browser.browser.send(cmd);
}

pub fn navigate(app: &mut App, tab: TabId, input: &str) {
    let engine = crate::settings::value(app, "browserSearchEngine");
    let custom = crate::settings::value(app, "browserCustomSearchUrl");
    let template = search_template(engine.as_str().unwrap_or(""), custom.as_str().unwrap_or(""));
    match resolve(input, template) {
        Some(url) => {
            bar(app, tab).replace_text(url.clone());
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

/// The floating tab, unless the panel shows that same tab.
fn pip_tab(app: &App) -> Option<TabId> {
    let tab = app.ui.panel.browser.pip?;
    (matches!(app.frame_route(), Route::Thread(_)) && panel_tab(app) != Some(tab) && app.ui.panel.browser.tabs.contains_key(&tab)).then_some(tab)
}

/// The one tab Chrome screencasts: the panel's, else the floating one.
fn shown(app: &App) -> Option<TabId> {
    panel_tab(app).or_else(|| pip_tab(app))
}

pub fn poll(app: &mut App, tx: &mut Tx) -> bool {
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
    let ui = &mut app.ui.panel.browser;
    if ui.shown != want {
        ui.shown = want;
        ui.browser.send(Cmd::Show(want));
    }
    let (generation, events, statuses) = {
        let mut shared = ui.browser.shared.lock().unwrap();
        (shared.generation, std::mem::take(&mut shared.events), shared.tabs.clone())
    };
    let mut changed = generation != ui.seen || !events.is_empty();
    ui.seen = generation;
    for (id, tab) in ui.tabs.iter_mut() {
        let status = statuses.get(id).cloned().unwrap_or_default();
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
        if !idle(&status) && status.failure.is_none() && !status.loading && status.url.starts_with("http") {
            let title = status.title.trim();
            let title = if title == status.url { "" } else { title };
            match ui.recents.iter_mut().find(|r| r.url == status.url) {
                Some(r) if r.title != title => r.title = title.to_string(),
                Some(_) => {}
                None => {
                    ui.recents.insert(0, Recent { url: status.url.clone(), title: title.to_string(), at: now });
                    ui.recents.truncate(8);
                }
            }
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
    for event in events {
        match event {
            browser::Event::Screenshot(Ok(path)) => screenshot_saved(app, path),
            browser::Event::Screenshot(Err(e)) => crate::shell::palette::toast(app, "Unable to capture screenshot", Some(&e)),
            browser::Event::Popup { target } => adopt(app, target),
            browser::Event::Picked { tab, element, image } => pick(app, tab, element, image),
            browser::Event::DevTools(Ok(url)) => {
                if let Err(e) = tx.open_url(&url) {
                    crate::shell::palette::toast(app, "Unable to open DevTools", Some(&e.to_string()));
                }
            }
            browser::Event::DevTools(Err(e)) => crate::shell::palette::toast(app, "Unable to open DevTools", Some(&e)),
        }
    }
    changed
}

/// Web's screenshot toast: Copy image, Copy path and Reveal.
fn screenshot_saved(app: &mut App, path: std::path::PathBuf) {
    let (image, shown, revealed) = (path.clone(), path.display().to_string(), path);
    let reveal_label = if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else if cfg!(windows) {
        "Reveal in File Explorer"
    } else {
        "Reveal in Files"
    };
    fn act(label: &str, primary: bool, f: impl Fn(&mut App, &mut Tx) + 'static) -> crate::shell::palette::ToastAction {
        (label.to_owned(), primary, std::rc::Rc::new(f))
    }
    let actions = vec![
        act("Copy image", true, move |s, tx| match std::fs::read(&image).ok().and_then(rune::ClipboardImage::from_png) {
            Some(png) => tx.copy_rich(rune::ClipboardContent::new("").with_image(png)),
            None => crate::shell::palette::toast(s, "Unable to copy screenshot", Some("The image could not be read.")),
        }),
        act("Copy path", false, move |_, tx| tx.copy(shown.clone())),
        act(reveal_label, false, move |s, tx| {
            let result = if cfg!(target_os = "macos") {
                std::process::Command::new("open").arg("-R").arg(&revealed).spawn().map(drop).map_err(|e| e.to_string())
            } else {
                let dir = revealed.parent().unwrap_or(&revealed);
                tx.open_url(&format!("file://{}", dir.display())).map_err(|e| e.to_string())
            };
            if let Err(e) = result {
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
        let path = dir.join(format!("browser-element-{}-{}.png", base36(data::now_ms().max(0) as u64), &id.replace('-', "")[..8]));
        std::fs::write(&path, bytes).ok()?;
        Some(path.display().to_string())
    });
    let record = annotation(&id, element, comment);
    let detail = record["elements"][0]["selector"].as_str().or(record["elements"][0]["tagName"].as_str()).unwrap_or_default().to_string();
    crate::chat::composer::add_context(app, record, image);
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
    let Some(tab) = panel_tab(app).filter(|_| tx.focused().is_some_and(|k| k.contains("browser-"))) else { return false };
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
    let started = ui.browser.started();
    let Some(state) = ui.tabs.get(&tab) else {
        return super::notice(t, "This browser tab is closed.");
    };
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
    let refresh_off = !started || idle(&status);
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
                        .disabled(!status.can_back)
                        .on_press(move |s: &mut App| send(s, Cmd::Back(tab)))
                        .build(t),
                )
                .child(
                    action("Forward", &icons::ARROW_RIGHT)
                        .disabled(!status.can_forward)
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
        .child(toggle(
            t,
            if status.picking { "Cancel annotation" } else { "Annotate preview" },
            &icons::MOUSE_POINTER_CLICK,
            status.picking,
            !status.painted || !live,
            move |s: &mut App| {
                let on = !s.ui.panel.browser.browser.status(tab).picking;
                send(s, Cmd::Pick(tab, on));
            },
        ))
        .child(
            action("Screenshot", &icons::CAMERA)
                .disabled(!status.painted || !live)
                .on_press(move |s: &mut App| screenshot(s, tab))
                .build(t),
        )
        .child(toggle(
            t,
            if pip { "Close floating preview" } else { "Float preview over chat" },
            &icons::PICTURE_IN_PICTURE_2,
            pip,
            idle(&status),
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
        kit::empty("The browser could not start", &error, t).grow(1.)
    } else if let Some(failure) = &status.failure {
        unreachable(t, tab, failure, ui.details)
    } else if idle(&status) {
        empty(app, t, tab)
    } else if let Some(size) = status.device {
        let (w, h, scale) = browser::device_box(ui.area.0, ui.area.1, size, status.zoom);
        let page = page(&state.pixels, "browser-page", 1.).size(w, h).border(1., t.c.border.alpha_mul(0.7));
        col().grow(1.).min_h(0.).w_full().center().child(page.children(hover_box(t, &status, scale, (w, h))).children(annotation_editor(app, t, tab, scale, (w, h))))
    } else {
        let hover = hover_box(t, &status, status.zoom.max(0.25), ui.area);
        page(&state.pixels, "browser-page", 1.).grow(1.).min_h(0.).w_full().children(hover).children(annotation_editor(app, t, tab, status.zoom.max(0.25), ui.area))
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
    let mut body = col().grow(1.).min_h(0.).child(chrome);
    if let Some(size) = status.device.filter(|_| live) {
        body = body.child(device_toolbar(t, tab, state, size, ui.area.0));
    }
    body.child(
        // A new key when the scale changes re-reports the layout, so Chrome re-renders at it.
        col()
            .key(format!("browser-viewport-{scale}"))
            .grow(1.)
            .min_h(0.)
            .w_full()
            .on_layout(move |s: &mut App, r| {
                s.ui.panel.browser.area = (r[2], r[3]);
                send(s, Cmd::Viewport { w: r[2].max(1.), h: r[3].max(1.), scale })
            })
            .child(content)
            .children(zoom_pill),
    )
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
        .pin(Some(0.), Some(theme::layout::TOPBAR_H), Some(right), Some(0.))
        .on_layout(|s: &mut App, r| s.ui.panel.browser.pip_area = r);
    // The chat area's size arrives with the first layout.
    if container.0 < 1. || container.1 < 1. {
        out.push(area);
        return;
    }
    let [x, y, w, h] = pip_frame(ui.pip_frame, aspect, container);
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
        .child(
            action("Open preview in right panel", &icons::PANEL_RIGHT)
                .on_press(move |s: &mut App| {
                    s.ui.panel.browser.pip = None;
                    super::activate(s, super::Surface::Browser(tab));
                })
                .build(t),
        )
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
        .child(el().pin(None, Some(10.), Some(10.), None).size(8., 8.).radius(4.).fill(t.c.foreground.alpha_mul(0.25)).group_opacity(1., 0.))
        .children(handles)
        .child(pill);
    area = area.child(frame);
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
    let [x, y, w, h] = pending.rect;
    let mut out: Vec<El> = hover_rect(pending.rect, scale, page)
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
        .on_key(|s: &mut App, e, _| {
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
            text(failure.description.to_uppercase())
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

/// `PreviewEmptyState`: recent pages and the server's listening ports, else a hint.
fn empty(app: &App, t: &T, tab: TabId) -> El {
    let ui = &app.ui.panel.browser;
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
    if servers.is_empty() && ui.recents.is_empty() {
        return col()
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
                text("No preview yet")
                    .font(theme::weight(theme::TEXT_XL, theme::SEMIBOLD))
                    .color(t.c.contrast_foreground),
            )
            .child(
                text("Type a URL above, or run a dev script. Local servers this thread starts will show up here automatically.")
                    .font(theme::TEXT_SM)
                    .color(t.c.muted_foreground)
                    .align(rune::Align::Center)
                    .wrap()
                    .max_w(384.)
                    .mt(4.),
            );
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
    let entry = |title: &str, description: &str| {
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
            .child(
                el().size(28., 28.)
                    .radius(6.)
                    .border(1., t.c.border)
                    .center()
                    .child(icon(&icons::GLOBE, 14.).size(14., 14.).color(t.c.muted_foreground)),
            )
            .child(
                col()
                    .grow(1.)
                    .min_w(0.)
                    .child(text(title.to_string()).font(theme::SM_MEDIUM).color(t.c.contrast_foreground).truncate())
                    .child(text(description.to_string()).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate()),
            )
    };
    let mut body = col().w_full().max_w(576.).gap(24.);
    if !ui.recents.is_empty() {
        let rows = ui
            .recents
            .iter()
            .map(|r| {
                let label = r.url.split_once("://").map_or(r.url.as_str(), |(_, rest)| rest).trim_end_matches('/').to_string();
                let when = crate::pages::pull_requests::relative(r.at, app.now_ms);
                let (title, description) = if r.title.is_empty() { (label.clone(), when) } else { (r.title.clone(), format!("{label} · {when}")) };
                let (open, remove) = (r.url.clone(), r.url.clone());
                entry(&title, &description)
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
                            .on_press(move |s: &mut App| s.ui.panel.browser.recents.retain(|r| r.url != remove))
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
                entry(title, description).on_press(move |s: &mut App| navigate(s, tab, &url))
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
        .min_width(224.)
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let record = annotation("abc", &element);
        assert_eq!(record["contextId"], "preview-annotation_abc");
        assert_eq!(record["label"], "Demo home");
        assert_eq!(record["targetSummary"], "1 selected element");
        let details = &record["elements"][0];
        assert_eq!(details["selector"], "#go");
        assert!(details.get("rect").is_none() && details["source"].is_null());
        let untitled = annotation("x", &json!({ "pageUrl": "http://a/", "pageTitle": "  " }));
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
