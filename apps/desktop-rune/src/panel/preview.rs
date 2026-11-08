//! The Browser surface, web's inline `PreviewView`: each tab's chrome row over its page, the
//! empty state, or the unreachable page. `browser` owns Chrome; this owns what the panel shows.
use std::collections::HashMap;

use rune::{El, Key, Phase, Pixels, ScrollSpec, Span, TextEdit, Tx, col, el, icon, rich, row, text, text_input};
use serde_json::json;

use super::browser::{self, Cmd, Status, TabId};
use crate::{
    App, data, icons, kit,
    theme::{self, AlphaMul, T},
};

const SERVERS: &str = "subscribeDiscoveredLocalServers";
const ZOOM_LEVELS: [f32; 17] = [0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1., 1.1, 1.25, 1.5, 1.75, 2., 2.5, 3., 4., 5.];
const NON_WEB_SCHEMES: [&str; 14] = [
    "about", "blob", "chrome", "data", "file", "ftp", "javascript", "mailto", "sms", "ssh", "tel", "view-source", "ws", "wss",
];
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
    ui.tabs.insert(tab, Tab { pixels: pixels.clone(), bar: TextEdit::new(""), focused: false });
    ui.browser.send(Cmd::Open(tab, pixels));
    tab
}

pub fn close(app: &mut App, tab: TabId) {
    let ui = &mut app.ui.panel.browser;
    ui.tabs.remove(&tab);
    ui.browser.send(Cmd::Close(tab));
    if ui.shown == Some(tab) {
        ui.shown = None;
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
    &mut app.ui.panel.browser.tabs.entry(tab).or_insert_with(|| Tab { pixels: Pixels::new(), bar: TextEdit::new(""), focused: false }).bar
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

fn shown(app: &App) -> Option<TabId> {
    match app.ui.panel.active {
        Some(super::Surface::Browser(tab)) if app.ui.shell.panel_open => Some(tab),
        _ => None,
    }
}

pub fn poll(app: &mut App) -> bool {
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
    for event in events {
        match event {
            browser::Event::Screenshot(Ok(path)) => {
                crate::shell::palette::toast(app, "Screenshot saved", Some(&path.display().to_string()))
            }
            browser::Event::Screenshot(Err(e)) => crate::shell::palette::toast(app, "Unable to capture screenshot", Some(&e)),
        }
    }
    changed
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
}

fn action(label: &str, glyph: &'static rune::Icon) -> kit::Button {
    kit::Button::new(label).icon(glyph).variant("ghost").size("icon-xs")
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
                    action(if status.loading { "Stop" } else { "Refresh" }, &icons::REFRESH_CW)
                        .disabled(!started || idle(&status))
                        .on_press(move |s: &mut App| {
                            let loading = s.ui.panel.browser.browser.status(tab).loading;
                            send(s, if loading { Cmd::Stop(tab) } else { Cmd::Reload { tab, hard: false } })
                        })
                        .build(t),
                ),
        )
        .child(el().grow(1.).min_w(0.).child(address))
        .child(action("Annotate preview", &icons::MOUSE_POINTER_CLICK).disabled(true).build(t))
        .child(
            action("Screenshot", &icons::CAMERA)
                .disabled(!status.painted || idle(&status) || status.failure.is_some())
                .on_press(move |s: &mut App| screenshot(s, tab))
                .build(t),
        )
        .child(action("Float preview over chat", &icons::PICTURE_IN_PICTURE_2).disabled(true).build(t))
        .child(
            el().key("browser-menu").child(
                action("Preview menu", &icons::ELLIPSIS_VERTICAL)
                    .disabled(!started)
                    .on_press(|s: &mut App| s.ui.panel.browser.menu ^= true)
                    .build(t),
            ),
        );
    let chrome = col().w_full().child(chrome).child(if status.loading {
        el().absolute(0., 26.).w_full().h(2.).fill(t.c.primary)
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
    } else {
        page(&state.pixels)
    };
    col().grow(1.).min_h(0.).child(chrome).child(
        // A new key when the scale changes re-reports the layout, so Chrome re-renders at it.
        col()
            .key(format!("browser-viewport-{scale}"))
            .grow(1.)
            .min_h(0.)
            .w_full()
            .on_layout(move |s: &mut App, r| {
                send(s, Cmd::Viewport { w: r[2].max(1.), h: r[3].max(1.), scale })
            })
            .child(content),
    )
}

fn page(pixels: &Pixels) -> El {
    el().grow(1.)
        .min_h(0.)
        .w_full()
        .pixels(pixels)
        .focusable(true)
        .key("browser-page")
        .on_drag(|s: &mut App, d, _| {
            send(
                s,
                Cmd::Mouse {
                    kind: match d.phase {
                        Phase::Down => "mousePressed",
                        Phase::Up => "mouseReleased",
                        _ => "mouseMoved",
                    },
                    x: d.x,
                    y: d.y,
                    clicks: d.clicks,
                },
            )
        })
        .on_pointer_move_local(|s: &mut App, p, _| send(s, Cmd::Mouse { kind: "mouseMoved", x: p.x, y: p.y, clicks: 0 }))
        .on_wheel(|s: &mut App, w, _| {
            send(s, Cmd::Wheel { x: w.x, y: w.y, dx: w.dx, dy: w.dy });
            true
        })
        .on_key(|s: &mut App, e, _| {
            if e.mods.cmd || (e.mods.ctrl && !matches!(e.key, Key::Text(_))) {
                return false;
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

/// `PreviewMoreMenu`.
pub fn overlays(app: &App, t: &T, out: &mut Vec<El>) {
    let ui = &app.ui.panel.browser;
    let Some(tab) = shown(app).filter(|_| ui.menu) else { return };
    let status = ui.browser.status(tab);
    let scheme = ui.scheme;
    let radio = |label: &'static str, value: &'static str| {
        kit::MenuEntry::item(kit::MenuItem::new(label).radio(scheme == value).on_select(move |s: &mut App| {
            s.ui.panel.browser.scheme = value;
            s.ui.panel.browser.menu = false;
            send(s, Cmd::ColorScheme(value));
        }))
    };
    let zoom_label = format!("{}%", (status.zoom * 100.).round());
    kit::Menu::new("browser-more")
        .anchor("browser-menu")
        .placement(kit::Placement::BottomEnd)
        .min_width(224.)
        .open(true)
        .on_close(|s: &mut App| {
            s.ui.panel.browser.menu = false;
            s.ui.panel.browser.appearance = false;
        })
        .item(kit::MenuItem::new("Hard reload").icon(&icons::ROTATE_CW).disabled(idle(&status)).on_select(move |s: &mut App| {
            s.ui.panel.browser.menu = false;
            send(s, Cmd::Reload { tab, hard: true });
        }))
        .item(
            kit::MenuItem::new("Appearance")
                .icon(&icons::SUN)
                .submenu(vec![radio("System", ""), radio("Light", "light"), radio("Dark", "dark")], ui.appearance)
                .on_submenu_change(|s: &mut App, open| s.ui.panel.browser.appearance = open),
        )
        .separator()
        .group_label(format!("Zoom · {zoom_label}"))
        .item(kit::MenuItem::new("Zoom in").icon(&icons::PLUS).disabled(status.zoom >= 5.).on_select(move |s: &mut App| zoom(s, tab, 1)))
        .item(kit::MenuItem::new("Zoom out").icon(&icons::MINUS).disabled(status.zoom <= 0.25).on_select(move |s: &mut App| zoom(s, tab, -1)))
        .item(kit::MenuItem::new("Reset zoom").disabled((status.zoom - 1.).abs() < 0.001).on_select(move |s: &mut App| zoom(s, tab, 0)))
        .separator()
        .group_label("Profile")
        .item(kit::MenuItem::new("Clear cookies").on_select(|s: &mut App| {
            s.ui.panel.browser.menu = false;
            send(s, Cmd::ClearCookies);
        }))
        .item(kit::MenuItem::new("Clear cache").on_select(|s: &mut App| {
            s.ui.panel.browser.menu = false;
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
    fn custom_search_needs_the_query_after_the_host() {
        assert_eq!(search_template("custom", "https://s.example/?q=%s"), "https://s.example/?q=%s");
        assert_eq!(search_template("custom", "https://%s.example/"), "https://duckduckgo.com/?q=%s");
        assert_eq!(search_template("bing", ""), "https://www.bing.com/search?q=%s");
    }
}
