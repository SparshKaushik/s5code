//! HtmlRenderFrame inline: an `html_render` page shown in its timeline row, as web's iframe shows
//! it. Rune embeds no web view, so the Default profile's Chrome shoots the page once at the row's
//! width and theme (`browser::Shot`) and the row shows that picture. Pressing it brings the page
//! live in place (`browser::Live`): Chrome paints it only when it changes and the row forwards
//! its pointer, keys and wheel, and refits to the page's height as it changes (`REFIT`). A row
//! asks for its shot only while on screen and lets it, and its live page, go once scrolled away;
//! at most `KEEP` pictures and one live page stay alive.

use rune::{Color, Cursor, El, Key, Phase, col, el, text};
use serde_json::{Map, Value, json};

use super::browser::{Cmd, Live, LiveInput, Shot};
use crate::chat::details::html_render;
use crate::theme::{self, T};
use crate::{App, icons, kit};

/// HTML_RENDER_COLUMN_WIDTH: the width a row has before its first layout.
const COLUMN_WIDTH: f32 = 728.;
const KEEP: usize = 3;
/// The tallest live page in CSS pixels; a taller one scrolls inside its row.
const LIVE_MAX_HEIGHT: f32 = 4000.;

/// Posts the page's height through `__s5Frame` whenever it resizes or changes, as
/// `{"s5":"height","value":…}`; `window.__s5Measure()` posts it again on demand.
const REFIT: &str = r#"(() => {
  let timer = 0, last = -1;
  const measure = (force) => {
    timer = 0;
    const e = document.documentElement;
    if (!e || typeof __s5Frame !== "function") return;
    const value = Math.ceil(e.scrollHeight > e.clientHeight ? e.scrollHeight : e.getBoundingClientRect().height);
    if (value === last && force !== true) return;
    last = value;
    __s5Frame(JSON.stringify({ s5: "height", value }));
  };
  const queue = () => { if (!timer) timer = setTimeout(measure, 50); };
  Object.defineProperty(window, "__s5Measure", { value: () => measure(true) });
  const watch = () => {
    const observer = new ResizeObserver(queue);
    observer.observe(document.documentElement);
    if (document.body) observer.observe(document.body);
    new MutationObserver(queue).observe(document.documentElement, { subtree: true, childList: true, attributes: true, characterData: true });
    queue();
  };
  if (document.readyState === "loading") addEventListener("DOMContentLoaded", watch, { once: true }); else watch();
  addEventListener("load", queue);
})();"#;
const MEASURE: &str = "window.__s5Measure && window.__s5Measure()";

#[derive(Default)]
pub struct Frames {
    /// Least recently on screen first.
    frames: Vec<Frame>,
    /// Live pages let go, for `poll` to close in Chrome.
    closing: Vec<u64>,
    /// The live page last pressed, which takes the wheel until Escape or the pointer leaves it,
    /// as a focused frame scrolls its page rather than the timeline.
    pub(super) engaged: Option<u64>,
}

#[derive(Default)]
struct Frame {
    id: String,
    title: String,
    /// The laid-out width; zero before the first layout.
    width: f32,
    visible: bool,
    /// The theme the row is drawn in, as `htmlRenderTheme` JSON.
    theme: String,
    dark: bool,
    /// The `assets.createUrl` in flight for the next shot.
    request: Option<u64>,
    pending: Option<(Shot, String)>,
    /// The last finished shot and the theme it was taken in.
    shown: Option<(Shot, String)>,
    /// The page's measured height, kept after its picture goes so the row keeps its size.
    content: Option<f32>,
    failed: bool,
    retried: bool,
    /// A scene's picture, never taken again.
    still: bool,
    /// The page live in the row, once pressed.
    live: Option<Live>,
    /// Pressed: the URL on the way opens the live page rather than a shot.
    wants_live: bool,
}

impl Frames {
    fn get(&self, id: &str) -> Option<&Frame> {
        self.frames.iter().find(|f| f.id == id)
    }

    /// Records a row's layout. Returns whether the view changes and whether the row now wants a
    /// fresh shot: it is on screen with none of its width and theme, and none on the way.
    fn seen(&mut self, id: &str, title: &str, width: f32, visible: bool, theme: &str, dark: bool) -> (bool, bool) {
        let index = match self.frames.iter().position(|f| f.id == id) {
            Some(index) => index,
            None => {
                self.frames.push(Frame { id: id.to_owned(), ..Default::default() });
                self.frames.len() - 1
            }
        };
        let mut frame = self.frames.remove(index);
        let mut changed = (frame.width - width.round()).abs() >= 1.;
        frame.width = width.round();
        frame.visible = visible;
        title.clone_into(&mut frame.title);
        if frame.theme != theme {
            theme.clone_into(&mut frame.theme);
            frame.dark = dark;
        }
        if !visible {
            changed |= frame.shown.take().is_some() | frame.pending.take().is_some();
            frame.wants_live = false;
            if let Some(live) = frame.live.take() {
                self.closing.push(live.id);
                changed = true;
            }
        }
        let want = frame.wants();
        if visible {
            self.frames.push(frame);
        } else {
            self.frames.insert(index, frame);
        }
        self.evict();
        (changed, want)
    }

    /// Lets the oldest off-screen pictures go past `KEEP`.
    fn evict(&mut self) {
        let mut held = self.frames.iter().filter(|f| f.shown.is_some() || f.pending.is_some()).count();
        for frame in self.frames.iter_mut().filter(|f| !f.visible) {
            if held <= KEEP {
                break;
            }
            if frame.shown.take().is_some() | frame.pending.take().is_some() {
                held -= 1;
            }
        }
    }
}

impl Frames {
    /// Brings a frame's page live, letting any other live page go.
    fn press(&mut self, id: &str) -> bool {
        for frame in &mut self.frames {
            if frame.id != id {
                if let Some(live) = frame.live.take() {
                    self.closing.push(live.id);
                }
            }
        }
        let Some(frame) = self.frames.iter_mut().find(|f| f.id == id).filter(|f| f.live.is_none() && !f.failed) else { return false };
        frame.wants_live = true;
        frame.request.is_none()
    }
}

impl Frame {
    fn wants(&self) -> bool {
        let current = |(shot, theme): &(Shot, String)| shot.width == self.width as u32 && *theme == self.theme;
        self.visible && self.width >= 1. && !self.failed && !self.still && self.request.is_none() && self.pending.is_none() && !self.shown.as_ref().is_some_and(current)
    }
}

/// Asks for a fresh asset URL for the frame's next shot: they expire, so none is kept.
fn request(s: &mut App, id: &str) {
    let Some(frame) = s.ui.panel.html_frames.frames.iter_mut().find(|f| f.id == id) else { return };
    let resource = json!({"resource": {"_tag": "attachment", "attachmentId": id, "fileName": html_render::file_name(&frame.title), "mimeType": "text/html", "disposition": "inline"}});
    match s.src.net() {
        Some(net) => frame.request = Some(net.request("assets.createUrl", resource)),
        None => frame.failed = true,
    }
}

fn layout(s: &mut App, id: &str, title: &str, theme: &str, dark: bool, [_, y, w, h]: [f32; 4]) -> bool {
    // Half a window of slack either side, so a short scroll back finds the picture still there.
    let visible = s.ui.desktop.window_size().is_none_or(|[_, height]| y + h > -height / 2. && y < height * 1.5);
    let (changed, want) = s.ui.panel.html_frames.seen(id, title, w, visible, theme, dark);
    if want {
        request(s, id);
    }
    close_released(s);
    if let Some(live) = s.ui.panel.html_frames.get(id).and_then(|f| f.live.clone()).filter(|l| changed && l.width != w.round() as u32) {
        super::preview::inline_frame_cmd(s, Cmd::LiveResize(live.id, w.round() as u32, live.height));
        super::preview::inline_frame_cmd(s, Cmd::LiveEval(live.id, MEASURE.to_owned()));
    }
    changed || want
}

fn close_released(s: &mut App) {
    for id in std::mem::take(&mut s.ui.panel.html_frames.closing) {
        super::preview::inline_frame_cmd(s, Cmd::LiveClose(id));
    }
}

fn press(s: &mut App, id: &str) {
    if s.ui.panel.html_frames.press(id) {
        request(s, id);
    }
    close_released(s);
}

fn input(s: &mut App, live: u64, input: LiveInput) {
    super::preview::inline_frame_cmd(s, Cmd::LiveInput(live, input));
}

/// The page's keys, as the browser panel types them; none for Escape and the app's shortcuts.
fn page_key(e: &rune::KeyEvent) -> Option<LiveInput> {
    Some(match &e.key {
        _ if e.mods.cmd || (e.mods.ctrl && !matches!(e.key, Key::Text(_))) => return None,
        Key::Text(v) => LiveInput::Text(v.clone()),
        Key::Enter => LiveInput::Key("Enter"),
        Key::Backspace => LiveInput::Key("Backspace"),
        Key::Delete => LiveInput::Key("Delete"),
        Key::Tab => LiveInput::Key("Tab"),
        Key::Left => LiveInput::Key("ArrowLeft"),
        Key::Right => LiveInput::Key("ArrowRight"),
        Key::Up => LiveInput::Key("ArrowUp"),
        Key::Down => LiveInput::Key("ArrowDown"),
        Key::Home => LiveInput::Key("Home"),
        Key::End => LiveInput::Key("End"),
        _ => return None,
    })
}

/// A live page over its row: its picture, taking the pointer and keys, and the wheel once
/// pressed. Escape hands focus back to the app and runs `escape`.
pub(super) fn surface(live: &Live, key: String, on_down: impl Fn(&mut App) + 'static, escape: impl Fn(&mut App) + 'static) -> El {
    interactive_surface(live, key, on_down, escape, false)
}

pub(super) fn interactive_surface(
    live: &Live,
    key: String,
    on_down: impl Fn(&mut App) + 'static,
    escape: impl Fn(&mut App) + 'static,
    owns_scroll: bool,
) -> El {
    let id = live.id;
    let file_pdf = live.file_pdf;
    el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .pixels(&live.pixels)
        .focusable(true)
        .key(key)
        .pointer_buttons([rune::PointerButton::Middle])
        .on_drag(move |s: &mut App, d, tx| {
            let button = match tx.button() {
                rune::PointerButton::Primary => "left",
                rune::PointerButton::Middle => "middle",
                _ => return,
            };
            let kind = match d.phase {
                Phase::Down => {
                    s.ui.panel.html_frames.engaged = Some(id);
                    on_down(s);
                    "mousePressed"
                }
                Phase::Up => "mouseReleased",
                _ => "mouseMoved",
            };
            let m = tx.mods();
            let modifiers = u32::from(m.alt) | u32::from(m.ctrl) << 1 | u32::from(m.logo || (m.cmd && !m.ctrl)) << 2 | u32::from(m.shift) << 3;
            input(s, id, LiveInput::Mouse { kind, x: d.x, y: d.y, button, clicks: d.clicks, modifiers });
        })
        .on_pointer_move_local(move |s: &mut App, p, _| input(s, id, LiveInput::Mouse { kind: "mouseMoved", x: p.x, y: p.y, button: "none", clicks: 0, modifiers: 0 }))
        .on_hover_change(move |s: &mut App, inside, _| {
            if !inside && s.ui.panel.html_frames.engaged == Some(id) {
                s.ui.panel.html_frames.engaged = None;
            }
        })
        .on_wheel(move |s: &mut App, w, _| {
            if !owns_scroll && s.ui.panel.html_frames.engaged != Some(id) {
                return false;
            }
            input(s, id, LiveInput::Wheel { x: w.x, y: w.y, dx: w.dx, dy: w.dy });
            true
        })
        .on_key(move |s: &mut App, e, tx| {
            if file_pdf && (e.mods.cmd || e.mods.ctrl) && matches!(&e.key, Key::Text(value) if value.eq_ignore_ascii_case("f")) {
                super::preview::inline_frame_cmd(s, Cmd::LiveEval(id, "document.getElementById('find')?.focus()".into()));
                return true;
            }
            if matches!(e.key, Key::Escape) {
                s.ui.panel.html_frames.engaged = None;
                tx.blur();
                escape(s);
                return true;
            }
            let Some(typed) = page_key(e) else { return false };
            input(s, id, typed);
            true
        })
}

/// Starts shots whose URL arrived and takes finished ones; a shot whose Chrome went away (a
/// relaunch at the window's scale) is taken again once.
pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    let mut ready = Vec::new();
    let mut resized = Vec::new();
    let base = s.src.net().map(|net| net.base_url().trim_end_matches('/').to_owned());
    for index in 0..s.ui.panel.html_frames.frames.len() {
        let frame = &mut s.ui.panel.html_frames.frames[index];
        if let (Some(id), Some(net)) = (frame.request, s.src.net()) {
            if let Some(reply) = net.take_reply(id) {
                frame.request = None;
                changed = true;
                match reply.ok().and_then(|v| v["relativeUrl"].as_str().map(str::to_owned)) {
                    Some(path) if frame.visible => ready.push((frame.id.clone(), format!("{}{}", base.clone().unwrap_or_default(), path.split('#').next().unwrap_or_default()), std::mem::take(&mut frame.wants_live))),
                    Some(_) => {}
                    None => frame.failed = true,
                }
            }
        }
        // Its Chrome went away (a relaunch): the picture shows again.
        if frame.live.as_ref().is_some_and(Live::gone) {
            frame.live = None;
            changed = true;
        }
        let heights: Vec<f32> = frame.live.as_ref().map(Live::messages).unwrap_or_default().iter().filter_map(|m| refit_height(m)).collect();
        if let (Some(&height), Some(live)) = (heights.last(), frame.live.as_ref()) {
            if frame.content.is_none_or(|c| (c - height).abs() >= 1.) {
                frame.content = Some(height);
                resized.push((live.id, frame.width as u32, height.min(LIVE_MAX_HEIGHT) as u32));
                changed = true;
            }
        }
        if let Some(result) = frame.pending.as_ref().and_then(|(shot, _)| shot.result()) {
            let pending = frame.pending.take();
            changed = true;
            match result {
                Some(height) => {
                    frame.content = Some(height);
                    frame.shown = pending;
                }
                None if !frame.retried => frame.retried = true,
                None => frame.failed = true,
            }
        }
    }
    for (live, width, height) in resized {
        super::preview::inline_frame_cmd(s, Cmd::LiveResize(live, width, height));
    }
    for (id, url, live) in ready {
        let Some(frame) = s.ui.panel.html_frames.get(&id) else { continue };
        let (theme, dark, width) = (frame.theme.clone(), frame.dark, frame.width as u32);
        let url = format!("{url}#t3-theme={}", encode_component(&theme));
        if live {
            let height = frame.content.unwrap_or(COLUMN_WIDTH).min(LIVE_MAX_HEIGHT).ceil() as u32;
            let live = super::preview::live(s, url, width, height, dark, Some(REFIT));
            if let Some(frame) = s.ui.panel.html_frames.frames.iter_mut().find(|f| f.id == id) {
                frame.live = live;
            }
            continue;
        }
        let shot = super::preview::snapshot(s, url, width, dark);
        let Some(frame) = s.ui.panel.html_frames.frames.iter_mut().find(|f| f.id == id) else { continue };
        match shot {
            Some(shot) => frame.pending = Some((shot, theme)),
            None => frame.failed = true,
        }
    }
    let wanted: Vec<String> = s.ui.panel.html_frames.frames.iter().filter(|f| f.wants()).map(|f| f.id.clone()).collect();
    for id in wanted {
        request(s, &id);
        changed = true;
    }
    close_released(s);
    changed
}

/// Scenes: a page's picture already taken, drawn here as a bar chart on the dark theme since the
/// render host has no Chrome.
pub fn scene_seed(app: &mut App, id: &str, height: f32) {
    let (w, h) = (1456u32, (height * 2.) as u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let bars: [(u32, [u8; 3]); 4] = [(70, [0x2d, 0xd4, 0xbf]), (100, [0xfb, 0xbf, 0x24]), (45, [0xc0, 0x84, 0xfc]), (82, [0x1b, 0x4e, 0xd8])];
    for (index, (percent, color)) in bars.iter().enumerate() {
        let (left, top) = (64 + index as u32 * 340, h - 64 - (h - 160) * percent / 100);
        for y in top..h - 64 {
            for x in left..left + 240 {
                let at = ((y * w + x) * 4) as usize;
                rgba[at..at + 4].copy_from_slice(&[color[0], color[1], color[2], 0xff]);
            }
        }
    }
    for pixel in rgba.chunks_exact_mut(4).filter(|p| p[3] == 0) {
        pixel[3] = 0xff;
    }
    let shot = Shot::new(String::new(), 0, true);
    shot.pixels.set(w, h, rgba);
    app.ui.panel.html_frames.frames.push(Frame { id: id.to_owned(), content: Some(height), shown: Some((shot, String::new())), still: true, ..Default::default() });
}

/// A page `scene_shoot` and the live scenes load in real Chrome: a revenue chart in the app's
/// look, laid out by Chrome itself.
pub(super) const SCENE_PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><style>
:root { color-scheme: light dark; --fg: #111; --muted: #666; --card: #f4f4f5; }
@media (prefers-color-scheme: dark) { :root { --fg: #f4f4f5; --muted: #a1a1aa; --card: #18181b; } }
body { margin: 0; padding: 24px; font: 14px/1.45 -apple-system, "Segoe UI", system-ui, sans-serif; color: var(--fg); background: transparent; }
h1 { font-size: 18px; margin: 0 0 4px; } p { margin: 0 0 20px; color: var(--muted); }
.chart { display: flex; gap: 16px; align-items: flex-end; height: 200px; padding: 16px; border-radius: 10px; background: var(--card); }
.bar { flex: 1; border-radius: 6px 6px 0 0; display: flex; align-items: flex-start; justify-content: center; color: #000; font-weight: 600; padding-top: 6px; }
.labels { display: flex; gap: 16px; padding: 8px 16px 0; color: var(--muted); } .labels span { flex: 1; text-align: center; }
</style></head><body><h1>Q3 revenue by region</h1><p>Rendered by Chrome: <span id="ua"></span></p>
<div class="chart"><div class="bar" style="height:70%;background:#2dd4bf">$7.0M</div><div class="bar" style="height:100%;background:#fbbf24">$10.0M</div><div class="bar" style="height:45%;background:#c084fc">$4.5M</div><div class="bar" style="height:82%;background:#60a5fa">$8.2M</div></div>
<div class="labels"><span>Americas</span><span>EMEA</span><span>APAC</span><span>LATAM</span></div>
<script>document.getElementById("ua").textContent = navigator.userAgent.match(/(HeadlessChrome|Chrome)\/[\d.]+/)?.[0] ?? navigator.userAgent;</script>
</body></html>"#;

/// A page as a `data:` URL, which Chrome loads without a server.
pub(super) fn data_url(html: &str) -> String {
    use base64::Engine;
    format!("data:text/html;base64,{}", base64::engine::general_purpose::STANDARD.encode(html))
}

/// Gives the panel a waker when a scene starts Chrome before the window would.
pub(super) fn scene_waker(app: &mut App) {
    if app.ui.panel.waker.is_none() {
        app.ui.panel.waker = Some(std::sync::Arc::new(|| {}));
    }
}

/// Scenes with Chrome (`S5_CHROME`): the row's picture shot from a real page, waiting for it
/// here since a headless run's clock does not wait on Chrome. Without Chrome the seeded picture
/// stays and the reason goes to stderr.
pub fn scene_shoot(app: &mut App, id: &str) {
    scene_waker(app);
    let Some(shot) = super::preview::snapshot(app, data_url(SCENE_PAGE), 728, true) else { return eprintln!("scene: no Chrome command channel") };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let result = loop {
        if let Some(result) = shot.result() {
            break result;
        }
        if std::time::Instant::now() > deadline {
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    super::browser::shutdown();
    let Some(height) = result else { return eprintln!("scene: Chrome did not shoot the page") };
    eprintln!("scene: Chrome shot the page at {height} CSS px");
    let frames = &mut app.ui.panel.html_frames.frames;
    frames.retain(|f| f.id != id);
    frames.push(Frame { id: id.to_owned(), content: Some(height), shown: Some((shot, String::new())), still: true, ..Default::default() });
}

/// HtmlRenderFrame: the page's picture at its frame height, with the hover-revealed "Open full
/// screen" button; "Unable to load" when the page could not be shot.
pub fn view(app: &App, id: &str, title: &str, page: &Value, t: &T) -> El {
    let frame = app.ui.panel.html_frames.get(id);
    let width = frame.map(|f| f.width).filter(|w| *w >= 1.).unwrap_or(COLUMN_WIDTH);
    let height = frame_height(page, width, frame.and_then(|f| f.content));
    let (key, name, theme, dark) = (id.to_owned(), title.to_owned(), theme_json(t), t.dark);
    let mut body = col()
        .key(format!("html-render-{id}"))
        .w_full()
        .h(height)
        .hover_group()
        .label(title.to_owned())
        .on_layout_tx(move |s: &mut App, r, tx| {
            if !layout(s, &key, &name, &theme, dark, r) {
                tx.live_only();
            }
        });
    let open = |id: &str, title: &str| {
        let (id, title) = (id.to_owned(), title.to_owned());
        move |s: &mut App| html_render::open(s, &id, &title)
    };
    if frame.is_some_and(|f| f.failed) {
        return col().w_full().min_w(0.0).px(4.0).child(body.center().child(text(format!("Unable to load {title}")).font(theme::TEXT_XS).color(t.c.muted_foreground)));
    }
    if let Some((shot, _)) = frame.and_then(|f| f.shown.as_ref()) {
        let id = id.to_owned();
        body = body.child(el().w_full().h_full().pixels(&shot.pixels).cursor(Cursor::Pointer).on_press(move |s: &mut App| press(s, &id)));
    }
    // The live page over its picture, which shows until Chrome paints.
    if let Some(live) = frame.and_then(|f| f.live.as_ref()) {
        body = body.child(surface(live, format!("html-render-live-{id}"), |_: &mut App| {}, |_: &mut App| {}));
    }
    let expand = kit::Button::new("Open full screen").icon(&icons::MAXIMIZE_2).variant("glass").size("icon-xs").on_press(open(id, title)).build(t).help("Open full screen");
    body = body.child(el().pin(None, Some(8.0), Some(8.0), None).group_opacity(0.0, 1.0).child(expand));
    col().w_full().min_w(0.0).px(4.0).child(body)
}

/// A height `REFIT` posted, fitted between 80 and 20000.
fn refit_height(message: &str) -> Option<f32> {
    let value: Value = serde_json::from_str(message).ok()?;
    let height = value["value"].as_f64().filter(|h| value["s5"] == "height" && h.is_finite())?;
    Some((height.ceil() as f32).clamp(80., 20_000.))
}

/// readMeasuredHeights: 1 to 24 `[width, height]` pairs, sorted by width, each height clamped;
/// none when any pair is malformed.
fn measured(value: &Value) -> Option<Vec<(f32, f32)>> {
    let entries = value.as_array().filter(|a| (1..=24).contains(&a.len()))?;
    let mut heights = entries
        .iter()
        .map(|e| {
            let width = e[0].as_u64().filter(|w| (1..=10_000).contains(w) && e.as_array().is_some_and(|p| p.len() == 2))?;
            Some((width as f32, clamp(e[1].as_f64().filter(|h| h.is_finite())?)))
        })
        .collect::<Option<Vec<_>>>()?;
    heights.sort_by(|a, b| a.0.total_cmp(&b.0));
    Some(heights)
}

fn clamp(height: f64) -> f32 {
    (height.round() as f32).clamp(80., 2000.)
}

/// measuredHeight: the taller of the measurements either side of `width`, as breakpoints can
/// pick either.
fn measured_height(heights: &[(f32, f32)], width: f32) -> f32 {
    let high = heights.iter().position(|(w, _)| *w >= width).unwrap_or(heights.len() - 1);
    let low = if heights[high].0 == width { high } else { high.saturating_sub(1) };
    heights[low].1.max(heights[high].1)
}

/// htmlRenderFrameHeight: the shot's measured page wins; else the agent's measurements at this
/// width, capped by its declared height when the column measurement overshot it.
fn frame_height(page: &Value, width: f32, content: Option<f32>) -> f32 {
    if let Some(content) = content {
        return content.ceil().clamp(80., 20_000.);
    }
    let declared = clamp(page["height"].as_f64().unwrap_or(0.));
    let Some(heights) = measured(&page["heights"]) else { return declared };
    let cap = if measured_height(&heights, COLUMN_WIDTH) > declared { declared } else { 2000. };
    cap.min(measured_height(&heights, width)).clamp(80., 2000.)
}

/// htmlRenderTheme with the app's tokens and web's default font stacks: `--accent` is the brand
/// colour, and the neutral hover surface is `--accent-surface`.
fn theme_json(t: &T) -> String {
    json!({ "appearance": if t.dark { "dark" } else { "light" }, "variables": theme_variables(t) }).to_string()
}

/// htmlRenderTheme's variables.
pub(super) fn theme_variables(t: &T) -> Map<String, Value> {
    let c = &t.c;
    let colors: [(&str, Color); 27] = [
        ("--background", c.background),
        ("--foreground", c.foreground),
        ("--muted", c.muted),
        ("--muted-foreground", c.muted_foreground),
        ("--card", c.card),
        ("--card-foreground", c.card_foreground),
        ("--popover", c.popover),
        ("--popover-foreground", c.popover_foreground),
        ("--secondary", c.secondary),
        ("--secondary-foreground", c.secondary_foreground),
        ("--border", c.border),
        ("--input", c.input),
        ("--ring", c.ring),
        ("--primary", c.message_action),
        ("--primary-foreground", c.message_action_foreground),
        ("--accent", c.primary),
        ("--accent-foreground", c.primary_foreground),
        ("--accent-surface", c.accent),
        ("--accent-surface-foreground", c.accent_foreground),
        ("--destructive", c.error),
        ("--destructive-foreground", c.error_foreground),
        ("--destructive-surface", c.error_surface),
        ("--warning", c.warning),
        ("--warning-foreground", c.warning_foreground),
        ("--warning-surface", c.warning_surface),
        ("--code-background", c.code_background),
        ("--code-foreground", c.code_foreground),
    ];
    let (success, info, charts) = if t.dark {
        (["#10b981", "#34d399"], ["#3b82f6", "#60a5fa"], ["#2dd4bf", "#fbbf24", "#c084fc", "#fb7185", "#a3e635"])
    } else {
        (["#10b981", "#047857"], ["#3b82f6", "#1d4ed8"], ["#0d9488", "#d97706", "#9333ea", "#e11d48", "#65a30d"])
    };
    let mut variables: Map<String, Value> = colors.iter().map(|(name, color)| ((*name).to_owned(), json!(css_hex(*color)))).collect();
    for (name, value) in [("--success", success[0]), ("--success-foreground", success[1]), ("--info", info[0]), ("--info-foreground", info[1])] {
        variables.insert(name.to_owned(), json!(value));
    }
    variables.insert("--chart-1".into(), json!(css_hex(c.primary)));
    for (index, color) in charts.iter().enumerate() {
        variables.insert(format!("--chart-{}", index + 2), json!(color));
    }
    variables.insert("--radius".into(), json!("0.625rem"));
    variables.insert("--font-sans".into(), json!("-apple-system, BlinkMacSystemFont, \"Segoe UI\", system-ui, sans-serif"));
    variables.insert("--font-mono".into(), json!("\"SF Mono\", \"SFMono-Regular\", Menlo, Consolas, \"Liberation Mono\", monospace"));
    variables
}

/// `#rrggbb`, or `#rrggbbaa` for a translucent token.
fn css_hex(color: Color) -> String {
    let hex = color.hex();
    if hex.ends_with("ff") { hex[..7].to_owned() } else { hex }
}

/// encodeURIComponent, which the page's bootstrap reverses.
fn encode_component(value: &str) -> String {
    value.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_sizes_like_web_before_and_after_its_page_is_measured() {
        let page = |value: Value| value;
        // The shot's measure wins, fitted between 80 and 20000.
        assert_eq!(frame_height(&page(json!({"height": 300})), 728., Some(1234.2)), 1235.);
        assert_eq!(frame_height(&page(json!({"height": 300})), 728., Some(10.)), 80.);
        // Only a declared height: clamped to 80..2000.
        assert_eq!(frame_height(&page(json!({"height": 5000})), 728., None), 2000.);
        assert_eq!(frame_height(&page(json!({"height": 20})), 728., None), 80.);
        // Measurements: the taller neighbour around the width.
        let measured = json!({"height": 900, "heights": [[860, 500], [375, 900], [728, 600]]});
        assert_eq!(frame_height(&measured, 728., None), 600.);
        assert_eq!(frame_height(&measured, 600., None), 900.);
        assert_eq!(frame_height(&measured, 1200., None), 600.);
        // A column measurement taller than the declared height caps every width at it.
        let overshot = json!({"height": 400, "heights": [[375, 900], [728, 600]]});
        assert_eq!(frame_height(&overshot, 375., None), 400.);
        // One malformed pair discards them all.
        let malformed = json!({"height": 300, "heights": [[728, 600], [0, 500]]});
        assert_eq!(frame_height(&malformed, 728., None), 300.);
    }

    #[test]
    fn a_frame_shoots_on_screen_and_lets_its_picture_go_off_it() {
        let mut frames = Frames::default();
        // Off screen from the start: nothing to take.
        assert_eq!(frames.seen("a", "A", 700., false, "dark", true), (true, false));
        // On screen: it wants a shot, once.
        assert_eq!(frames.seen("a", "A", 700., true, "dark", true).1, true);
        frames.frames[0].request = Some(1);
        assert_eq!(frames.seen("a", "A", 700., true, "dark", true), (false, false));
        frames.frames[0].request = None;
        frames.frames[0].shown = Some((Shot::new("u".into(), 700, true), "dark".into()));
        assert_eq!(frames.seen("a", "A", 700., true, "dark", true), (false, false));
        // A new width or theme takes it again; the old picture shows meanwhile.
        assert_eq!(frames.seen("a", "A", 640., true, "dark", true), (true, true));
        assert_eq!(frames.seen("a", "A", 700., true, "light", false).1, true);
        assert!(frames.get("a").unwrap().shown.is_some());
        // Scrolled away: the picture goes, its height stays.
        frames.frames[0].content = Some(420.);
        assert_eq!(frames.seen("a", "A", 700., false, "light", false), (true, false));
        let a = frames.get("a").unwrap();
        assert!(a.shown.is_none() && a.content == Some(420.));
    }

    #[test]
    fn one_page_is_live_at_a_time_and_scrolling_away_closes_it() {
        let mut frames = Frames::default();
        frames.seen("a", "A", 700., true, "dark", true);
        frames.seen("b", "B", 700., true, "dark", true);
        let a = Live::new("u".into(), 700, 400, true);
        frames.frames.iter_mut().find(|f| f.id == "a").unwrap().live = Some(a.clone());
        // Pressing another frame lets the first page go and asks for the second's URL.
        assert!(frames.press("b"));
        assert_eq!(frames.closing, [a.id]);
        assert!(frames.get("a").unwrap().live.is_none() && frames.get("b").unwrap().wants_live);
        // A URL already on the way serves the press.
        frames.frames.iter_mut().find(|f| f.id == "a").unwrap().request = Some(7);
        assert!(!frames.press("a"));
        // Scrolled away: the live page closes and the press is forgotten.
        frames.closing.clear();
        let b = Live::new("u".into(), 700, 400, true);
        frames.frames.iter_mut().find(|f| f.id == "b").unwrap().live = Some(b.clone());
        assert_eq!(frames.seen("b", "B", 700., false, "dark", true).0, true);
        assert_eq!(frames.closing, [b.id]);
        assert!(!frames.get("b").unwrap().wants_live);
    }

    #[test]
    fn a_live_page_refits_to_the_heights_it_posts() {
        assert_eq!(refit_height(r#"{"s5":"height","value":612.2}"#), Some(613.));
        assert_eq!(refit_height(r#"{"s5":"height","value":12}"#), Some(80.));
        assert_eq!(refit_height(r#"{"s5":"height","value":90000}"#), Some(20_000.));
        // Loads, other messages and malformed heights are not heights.
        assert_eq!(refit_height(super::super::browser::LIVE_LOAD), None);
        assert_eq!(refit_height(r#"{"s5":"height","value":"tall"}"#), None);
        assert_eq!(refit_height(r#"{"jsonrpc":"2.0","method":"ping","id":1}"#), None);
    }

    #[test]
    fn only_the_last_few_pictures_stay() {
        let mut frames = Frames::default();
        for id in ["a", "b", "c", "d", "e"] {
            frames.seen(id, id, 700., true, "dark", true);
            frames.frames.last_mut().unwrap().shown = Some((Shot::new("u".into(), 700, true), "dark".into()));
        }
        // Rows still on screen keep theirs, however many.
        assert_eq!(frames.frames.iter().filter(|f| f.shown.is_some()).count(), 5);
        // Ones that left, without a report saying so, go oldest first past three.
        for frame in &mut frames.frames {
            frame.visible = false;
        }
        frames.seen("e", "e", 700., true, "dark", true);
        let kept: Vec<&str> = frames.frames.iter().filter(|f| f.shown.is_some()).map(|f| f.id.as_str()).collect();
        assert_eq!(kept, ["c", "d", "e"]);
    }

    #[test]
    fn the_theme_rides_the_url_as_web_encodes_it() {
        assert_eq!(encode_component("{\"a\":\"#fff 1\"}"), "%7B%22a%22%3A%22%23fff%201%22%7D");
        assert_eq!(encode_component("é"), "%C3%A9");
        let dark: Value = serde_json::from_str(&theme_json(&crate::theme::theme(true))).unwrap();
        assert_eq!(dark["appearance"], "dark");
        assert_eq!(dark["variables"]["--background"], "#000000");
        assert_eq!(dark["variables"]["--chart-2"], "#2dd4bf");
        // The brand colour is --accent; the translucent hover surface keeps its alpha.
        assert_eq!(dark["variables"]["--accent"], dark["variables"]["--ring"]);
        assert!(dark["variables"]["--accent-surface"].as_str().unwrap().len() == 9);
    }
}
