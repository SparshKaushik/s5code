//! The right panel's Browser surface: tab strip, the preview toolbar and the page, which a
//! headless Chrome renders (see `browser.rs`).

use rune::{Cursor, Cx, Drag, El, Key, KeyEvent, Phase, TextEdit, Tx, col, el, icon, row, spacer, text, text_input};
use rune_kit::Kit;

use crate::browser::Cmd;
use crate::icons as i;
use crate::theme::{self, P};
use crate::ui::{self, icon_button};
use crate::App;

pub const WIDTH: f32 = 540.0;

pub fn view(s: &App, cx: &mut Cx<App>, p: P, k: &Kit) -> El {
    let showing = s.browsing;
    if showing && let Some([w, h]) = cx.layout_size("browser-surface") {
        let size = (w.round() as u32, h.round() as u32);
        if size.0 > 0 && size.1 > 0 && size != s.surface.get() {
            s.surface.set(size);
            s.browser.send(Cmd::Resize { w: size.0, h: size.1, scale: cx.env().scale });
        }
    }
    let body = if showing { surface(s, p) } else { empty(p) };
    col()
        .key("right-panel")
        .w(WIDTH)
        .h_full()
        .fill(p.bg)
        .child(el().absolute(0.0, 0.0).w(1.0).h_full().fill(p.border))
        .child(tabs(p))
        .child(toolbar(s, p, k))
        .child(body)
}

fn tabs(p: P) -> El {
    let tab = |key: &str, glyph: &'static rune::Icon, label: &str, active: bool| {
        let mut t = row()
            .key(key.to_string())
            .h(24.0)
            .px(8.0)
            .gap(6.0)
            .center_y()
            .radius(6.0)
            .max_w(144.0)
            .hover(p.row_hover)
            .cursor(Cursor::Pointer)
            .child(icon(glyph, 13.0).color(if active { p.fg } else { p.muted }))
            .child(text(label.to_string()).font(theme::XS).color(if active { p.fg } else { p.muted }).truncate());
        if active {
            t = t.fill(p.accent);
        }
        t
    };
    row()
        .key("panel-tabs")
        .w_full()
        .h(52.0)
        .px(10.0)
        .gap(4.0)
        .center_y()
        .window_drag()
        .child(tab("tab-browser", &i::GLOBE, "Browser", true))
        .child(tab("tab-terminal", &i::TERMINAL, "Terminal", false))
        .child(icon_button(p, "add-surface", &i::PLUS, 24.0, "Add panel surface", |_, _| {}))
        .child(spacer())
        .child(icon_button(p, "close-panel", &i::X, 28.0, "Close panel", |s, _| s.panel_open = false))
}

fn toolbar(s: &App, p: P, _k: &Kit) -> El {
    let nav = |key: &str, glyph: &'static rune::Icon, label: &str, on: bool, cmd: fn() -> Cmd| {
        if on { icon_button(p, key, glyph, 28.0, label, move |s, _| s.browser.send(cmd())) } else { ui::disabled_icon(p, key, glyph, 28.0) }
    };
    let address = row()
        .key("address")
        .h(28.0)
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .px(8.0)
        .gap(6.0)
        .center_y()
        .radius(6.0)
        .fill(p.row_selected)
        .child(
            text_input(&s.url, theme::SM)
                .single_line()
                .key("address-input")
                .placeholder("Search or enter URL", p.faint)
                .color(p.fg)
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .on_edit(|s: &mut App| &mut s.url)
                .on_submit(|s: &mut App, tx: &mut Tx| {
                    let target = normalize(&s.url.text);
                    s.url.focused = false;
                    navigate(s, target);
                    tx.blur();
                }),
        )
        .child(icon_button(p, "open-external", &i::EXTERNAL, 20.0, "Open in system browser", |_, _| {}));
    let loading = el().key("loading-bar").absolute(0.0, 38.0).w_full().h(2.0).fill(p.primary).opacity(if s.page.loading { 1.0 } else { 0.0 });
    row()
        .key("preview-toolbar")
        .w_full()
        .h(40.0)
        .px(6.0)
        .gap(2.0)
        .center_y()
        .child(nav("back", &i::ARROW_LEFT, "Back", s.page.can_back, || Cmd::Back))
        .child(nav("forward", &i::ARROW_RIGHT, "Forward", s.page.can_forward, || Cmd::Forward))
        .child(nav("reload", &i::ROTATE_CW, "Refresh", s.browsing, || Cmd::Reload))
        .child(address)
        .child(icon_button(p, "annotate", &i::PENCIL_LINE, 28.0, "Annotate preview", |_, _| {}))
        .child(icon_button(p, "screenshot", &i::CAMERA, 28.0, "Capture screenshot", |_, _| {}))
        .child(icon_button(p, "float", &i::PICTURE_IN_PICTURE, 28.0, "Float preview over chat", |_, _| {}))
        .child(icon_button(p, "preview-menu", &i::MORE, 28.0, "Preview menu", |_, _| {}))
        .child(el().absolute(0.0, 39.0).w_full().h(1.0).fill(p.border))
        .child(loading)
}

pub fn navigate(s: &mut App, url: String) {
    if url.is_empty() {
        return;
    }
    s.browsing = true;
    s.url = TextEdit::new(url.clone());
    s.browser.send(Cmd::Navigate(url));
}

fn normalize(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() || t.contains("://") || t.starts_with("about:") {
        return t.to_string();
    }
    let hostish = !t.contains(' ') && (t.contains('.') || t.starts_with("localhost") || t.contains(':'));
    if !hostish {
        return format!("https://duckduckgo.com/html/?q={}", t.replace(' ', "+"));
    }
    if t.starts_with("localhost") || t.starts_with("127.") { format!("http://{t}") } else { format!("https://{t}") }
}

fn surface(s: &App, p: P) -> El {
    let page = el()
        .key("browser-surface")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .fill(rune::Color::WHITE)
        .pixels(&s.browser.pixels)
        .cursor(Cursor::Pointer)
        .on_drag(|s: &mut App, d: &mut Drag, _tx: &mut Tx| {
            let (x, y) = (d.x, d.y);
            match d.phase {
                Phase::Down => {
                    s.page_focused = true;
                    s.browser.send(Cmd::Mouse { kind: "mouseMoved", x, y, clicks: 0 });
                    s.browser.send(Cmd::Mouse { kind: "mousePressed", x, y, clicks: 1 });
                }
                Phase::Move => s.browser.send(Cmd::Mouse { kind: "mouseMoved", x, y, clicks: 0 }),
                Phase::Up => s.browser.send(Cmd::Mouse { kind: "mouseReleased", x, y, clicks: 1 }),
            }
        })
        .on_wheel(|s: &mut App, e, _tx| {
            s.browser.send(Cmd::Wheel { x: e.x, y: e.y, dx: e.dx, dy: e.dy });
            true
        });
    let mut c = col().key("browser-body").w_full().grow(1.0).min_h(0.0).child(page);
    if let Some(err) = &s.page.error {
        c = c.child(row().px(12.0).py(8.0).fill(p.error.alpha(0.15)).child(text(err.clone()).font(theme::XS).color(p.error).wrap()));
    }
    c
}

fn empty(p: P) -> El {
    let entry = |key: &str, url: &'static str, detail: &str, glyph: &'static rune::Icon| {
        row()
            .key(key.to_string())
            .w_full()
            .h(32.0)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(6.0)
            .hover(p.row_hover)
            .cursor(Cursor::Pointer)
            .on_press(move |s: &mut App| navigate(s, url.to_string()))
            .child(icon(glyph, 14.0).color(p.muted))
            .child(text(url).font(theme::SM).color(p.fg).truncate().shrink())
            .child(spacer())
            .child(text(detail.to_string()).font(theme::XS).color(p.faint))
    };
    col()
        .key("preview-empty")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(28.0)
        .pt(72.0)
        .gap(6.0)
        .items_center()
        .child(icon(&i::GLOBE, 28.0).color(p.faint))
        .child(text("No preview yet").font(theme::SM_MEDIUM).color(p.fg).mt(8.0))
        .child(
            text("Type a URL above, or run a dev script. Local servers this thread starts will show up here automatically.")
                .font(theme::XS)
                .color(p.muted)
                .wrap()
                .align(rune::Align::Center)
                .max_w(320.0),
        )
        .child(
            col()
                .w_full()
                .mt(28.0)
                .gap(2.0)
                .child(text("Local servers").font(theme::XS_MEDIUM).color(p.muted).px(8.0).pb(4.0))
                .child(entry("server-3000", "http://localhost:3000", "billing-api · vite", &i::SERVER))
                .child(text("Recently used").font(theme::XS_MEDIUM).color(p.muted).px(8.0).pt(12.0).pb(4.0))
                .child(entry("recent-example", "https://example.com", "2h", &i::GLOBE))
                .child(entry("recent-hn", "https://news.ycombinator.com", "1d", &i::GLOBE)),
        )
}

pub fn forward_key(s: &mut App, e: &KeyEvent) -> bool {
    match &e.key {
        Key::Text(t) => s.browser.send(Cmd::Text(t.clone())),
        Key::Enter => s.browser.send(Cmd::Key("Enter")),
        Key::Backspace => s.browser.send(Cmd::Key("Backspace")),
        Key::Tab => s.browser.send(Cmd::Key("Tab")),
        _ => return false,
    }
    true
}
