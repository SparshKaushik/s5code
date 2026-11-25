use crate::{
    App,
    data::s,
    icons,
    theme::{self, AlphaMul, T},
};
use super::media::{self, Kind, Loaded};
use crate::chat::snapshot;
use rune::{Drag, El, Key, Phase, Tx, col, el, icon, row, text};
use serde_json::{Value, json};
use std::collections::HashMap;

/// An image bubble's 240x180 frame on a 2x display.
const THUMBNAIL_DECODE: u32 = 480;
/// The lightbox's own decode, held only while it is open.
const LIGHTBOX_DECODE: u32 = 2048;
/// ZoomableImage's MAX_ZOOM.
const MAX_ZOOM: f32 = 8.0;
const STAGE: &str = "image-lightbox-stage";

/// ZoomableImage's zoom (1 fits the stage) and scroll offset into the zoomed picture.
#[derive(Clone, Copy, Debug, PartialEq)]
struct View {
    zoom: f32,
    scroll: [f32; 2],
}

/// The open picture's zoom; another picture refits, as web remounts ZoomableImage.
struct Zoomed {
    key: String,
    view: View,
    /// The stage and picture size its handlers last saw, for the lightbox's own arrow keys.
    stage: [f32; 2],
    natural: [f32; 2],
    /// The scroll a drag started from.
    grab: Option<[f32; 2]>,
}

#[derive(Default)]
pub struct Assets {
    pending: HashMap<String, u64>,
    pub(super) ready: HashMap<String, Loaded>,
    pub(super) failed: std::collections::HashSet<String>,
    /// The image the lightbox shows: an attachment id or a Markdown media key.
    pub open: Option<String>,
    /// The pictures Previous and Next step through: those sent with the open one, or those in its
    /// Markdown paragraph.
    pub gallery: Vec<String>,
    /// The open image at lightbox size, and whether a view has drawn it yet.
    zoom: Option<(String, Loaded, bool)>,
    pan: Option<Zoomed>,
    /// The thread whose pictures are decoded; another thread's are dropped.
    thread: Option<String>,
    pub media: media::State,
}

impl Assets {
    /// Drops the decoded pictures of a thread no longer shown. Requests in flight stay so their
    /// replies are still taken.
    fn forget(&mut self) {
        self.ready.clear();
        self.failed.clear();
        self.open = None;
        self.gallery.clear();
        self.zoom = None;
        self.pan = None;
        self.media.forget();
    }
}

/// Loads the lightbox's full-size picture while it is open, and drops it once closed.
fn zoom(app: &mut App) -> bool {
    let open = app.ui.chat.timeline.assets.open.clone();
    let assets = &mut app.ui.chat.timeline.assets;
    if let Some((key, loaded, drawn)) = &mut assets.zoom
        && open.as_deref() == Some(key.as_str())
    {
        let ready = !*drawn && loaded.pixels.size().0 > 0;
        *drawn |= ready;
        return ready;
    }
    let had = assets.zoom.take().is_some();
    let waker = app.ui.panel.waker.clone();
    let next = open.and_then(|key| {
        let (_, loaded) = media::find(&*app, &key)?;
        let loaded = loaded.filter(|l| l.kind == Some(Kind::Image) && !l.url.is_empty())?;
        Some((key, Loaded::new(loaded.url, loaded.name, loaded.kind, LIGHTBOX_DECODE, waker), false))
    });
    app.ui.chat.timeline.assets.zoom = next;
    had
}

pub fn poll(app: &mut App, tx: &mut rune::Tx) -> bool {
    let mut moved = false;
    if let crate::Route::Thread(thread) = app.frame_route()
        && app.ui.chat.timeline.assets.thread.as_ref() != Some(thread)
    {
        let thread = Some(thread.clone());
        let assets = &mut app.ui.chat.timeline.assets;
        if assets.thread.is_some() {
            assets.forget();
            moved = true;
        }
        assets.thread = thread;
    }
    let media_changed = moved | media::poll(app, tx) | zoom(app);
    let crate::Route::Thread(thread) = app.frame_route() else {
        return media_changed;
    };
    let attachments: Vec<Value> = app
        .state()
        .thread_views
        .get(thread)
        .into_iter()
        .flat_map(|v| super::rows::derive(&v.projection, app.now_ms))
        .filter_map(|r| {
            if let super::rows::Row::User(v) = r {
                Some(v)
            } else {
                None
            }
        })
        .flat_map(|v| super::rows::array(v, "attachments").iter().cloned())
        .collect();
    // A playing clip's time and progress move; the poll's one-second wake redraws them.
    let playing = app.ui.chat.timeline.assets.ready.values().any(|l| l.player().is_some_and(|p| p.is_audio() && p.is_playing()));
    let media_changed = media_changed | playing;
    let waker = app.ui.panel.waker.clone();
    let Some(net) = app.src.net() else {
        return media_changed;
    };
    let assets = &mut app.ui.chat.timeline.assets;
    let mut changed = media_changed;
    for a in attachments {
        let id = s(&a, "id");
        if id.is_empty() || assets.ready.contains_key(id) || assets.failed.contains(id) {
            continue;
        }
        if !assets.pending.contains_key(id) && assets.pending.len() >= 6 {
            continue;
        }
        let request = *assets.pending.entry(id.to_owned()).or_insert_with(|| net.request("assets.createUrl", json!({"resource":{"_tag":"attachment","attachmentId":id,"fileName":s(&a,"name"),"mimeType":s(&a,"mimeType"),"disposition":"inline"}})));
        if let Some(reply) = net.take_reply(request) {
            assets.pending.remove(id);
            changed = true;
            let Ok(reply) = reply else {
                assets.failed.insert(id.to_owned());
                continue;
            };
            let base = net.base_url().to_owned();
            let url = format!("{}{}", base.trim_end_matches('/'), s(&reply, "relativeUrl"));
            let kind = media::attachment_kind(s(&a, "mimeType"), s(&a, "name"));
            assets.ready.insert(id.to_owned(), Loaded::new(url, s(&a, "name").to_owned(), kind, THUMBNAIL_DECODE, waker.clone()));
        }
    }
    changed
}

/// formatBytes: "512 B", "1.20 KB", "12.5 MB".
fn format_bytes(value: f64) -> String {
    if value.round() < 1024.0 {
        return format!("{} B", value.round() as i64);
    }
    let scaled = |v: f64| format!("{:.*}", if v >= 100.0 { 0 } else if v >= 10.0 { 1 } else { 2 }, v);
    let units = ["KB", "MB", "GB", "TB"];
    let (mut next, mut unit) = (value / 1024.0, 0);
    // Pick the unit after rounding, so 1023.6 KB reads "1.00 MB".
    while scaled(next).parse::<f64>().unwrap_or(0.0) >= 1024.0 && unit < units.len() - 1 {
        next /= 1024.0;
        unit += 1;
    }
    format!("{} {}", scaled(next), units[unit])
}

/// UserFileCard's name and size line: "Pasted text" and its size, else the file name and
/// "{EXT} · {size}".
fn file_label(attachment: &Value) -> (String, String) {
    let name = s(attachment, "name");
    let size = format_bytes(attachment["sizeBytes"].as_f64().unwrap_or(0.0));
    if s(&attachment["source"], "_tag") == "pasted-text" {
        return ("Pasted text".to_owned(), size);
    }
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .filter(|ext| (1..=8).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric()))
        .map_or_else(|| "File".to_owned(), str::to_uppercase);
    (name.to_owned(), format!("{ext} · {size}"))
}

pub fn view(app: &App, attachment: &Value, t: &T) -> El {
    let id = s(attachment, "id");
    let name = s(attachment, "name");
    let asset = app.ui.chat.timeline.assets.ready.get(id);
    let kind = media::attachment_kind(s(attachment, "mimeType"), name);
    if kind == Some(Kind::Video) {
        // UserVideoAttachment: the video in the bubble, with Save video on right-click.
        let failed = app.ui.chat.timeline.assets.failed.contains(id);
        let source = media::Media { kind: Kind::Video, name: name.to_owned(), reference: None, open_file: None };
        let tile = el().w(240.0).child(media::video(id, name, asset, failed, None, t));
        return media::with_actions(tile, id, &source, false);
    }
    if kind == Some(Kind::Audio) && attachment["downloadable"] != false {
        return audio(attachment, asset, app.ui.chat.timeline.assets.failed.contains(id), t);
    }
    let mut content = if s(attachment, "mimeType").starts_with("image/") {
        let mut frame = el()
            .size(240.0, 180.0)
            .radius(16.0)
            .clip_rounded(16.0)
            .fill(t.c.message_surface);
        if let Some(loaded) = asset {
            frame = frame.pixels_fit(&loaded.pixels, rune::SurfaceFit::Fit);
            if snapshot::is_snapshot(&attachment["source"]) {
                frame = frame.child(snapshot::overlay(app, &attachment["source"], format!("snapshot-contents-{id}")));
            }
        }
        frame
    } else {
        return file_card(attachment, asset.map(|a| a.url.clone()), t);
    };
    if asset.is_some() {
        let id = id.to_owned();
        content = content
            .focusable(true)
            .role("button")
            .cursor(rune::Cursor::Pointer)
            .label(format!("Preview {name}"))
            .on_press(move |app: &mut App| {
                let gallery = siblings(app, &id);
                let assets = &mut app.ui.chat.timeline.assets;
                assets.open = Some(id.clone());
                assets.gallery = gallery;
            });
    }
    content
}

/// "m:ss".
fn audio_time(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

const AUDIO_CHIP_WIDTH: f32 = 256.0;

/// InlineAudioPreview's chip: play or pause, the name (pressed to preview the file), and the
/// time over a progress fill; "Unavailable" when it can't play.
fn audio(attachment: &Value, asset: Option<&Loaded>, failed: bool, t: &T) -> El {
    let name = s(attachment, "name").to_owned();
    let accent = crate::chat::composer::chips::look("file", None).map_or(t.c.info_foreground, |(accent, _)| accent);
    let player = asset.and_then(Loaded::player).cloned();
    let failed = failed || asset.is_some_and(Loaded::failed);
    let state = player.as_ref().map(rune::Player::state);
    let loading = !failed && matches!(state, None | Some(rune::PlayerState::Loading));
    let playing = player.as_ref().is_some_and(rune::Player::is_playing);
    let duration = player.as_ref().and_then(rune::Player::duration).filter(|d| d.is_finite()).unwrap_or(0.0);
    let elapsed = player.as_ref().map_or(0.0, rune::Player::time);
    let (action, glyph) = match () {
        _ if failed => ("Open", &icons::TRIANGLE_ALERT),
        _ if loading => ("Loading", &icons::LOADER_CIRCLE),
        _ if playing => ("Pause", &icons::PAUSE),
        _ => ("Play", &icons::PLAY),
    };
    let time = match () {
        _ if failed => "Unavailable".to_owned(),
        _ if loading => "...".to_owned(),
        _ if duration > 0.0 && (elapsed > 0.0 || playing) => format!("{} / {}", audio_time(elapsed), audio_time(duration)),
        _ if duration > 0.0 => audio_time(duration),
        _ => audio_time(elapsed),
    };
    let url = asset.map(|a| a.url.clone());
    let preview = |el: El| match url.clone() {
        Some(url) => el.cursor(rune::Cursor::Pointer).on_press(move |app: &mut App| crate::panel::open_preview(app, &url)),
        None => el,
    };
    let mut button = el()
        .size(20.0, 20.0)
        .center()
        .radius(theme::radius::SM)
        .role("button")
        .label(format!("{}{action} {name}", if failed { "Audio unavailable. " } else { "" }))
        .child(icon(glyph, 12.0).color(accent));
    button = match (&player, failed, loading) {
        (_, true, _) => preview(button.focusable(true)),
        (Some(player), false, false) => {
            let toggle = player.clone();
            button.focusable(true).cursor(rune::Cursor::Pointer).hover(accent.alpha(0.15)).on_press(move |_: &mut App| if toggle.is_playing() { toggle.pause() } else { toggle.play() })
        }
        _ => button.disabled(true),
    };
    let progress = if duration > 0.0 { (elapsed / duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
    let label = preview(text(name.clone()).font(theme::TEXT_SM).color(t.c.message_foreground).truncate().grow(1.0).shrink().min_w(0.0).label(format!("Preview {name}")));
    row()
        .w(AUDIO_CHIP_WIDTH)
        .h(32.0)
        .gap(6.0)
        .pl(6.0)
        .pr(10.0)
        .center_y()
        .radius(theme::radius::LG)
        .border(1.0, if failed { t.c.destructive.alpha(0.4) } else { accent.alpha(0.3) })
        .fill(t.c.message_surface)
        .clip_rounded(theme::radius::LG)
        .help(if failed { "Audio unavailable. Open the file or retry playback.".to_owned() } else { name })
        .child(el().pin(Some(0.0), Some(0.0), None, Some(0.0)).w(AUDIO_CHIP_WIDTH * progress).fill(accent.alpha(0.17)))
        .child(button)
        .child(label)
        .child(text(time).font(theme::TEXT_3XS).color(t.c.muted_foreground).shrink().min_w(0.0))
}

/// UserFileCard: the file's name and size, pressed to preview it in the panel, and Download
/// once its URL is signed.
fn file_card(attachment: &Value, url: Option<String>, t: &T) -> El {
    let (label, size) = file_label(attachment);
    let identity = row()
        .grow(1.0)
        .min_w(0.0)
        .gap(12.0)
        .center_y()
        .radius(12.0)
        .child(icon(&icons::FILE, 20.0).padding(8.0).radius(8.0).fill(t.c.background))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .child(text(label.clone()).font(theme::SM_MEDIUM).color(t.c.message_foreground).truncate())
                .child(text(size).font(theme::TEXT_XS).color(t.c.muted_foreground)),
        );
    let download_label = format!("Download {label}");
    let download = row()
        .size(24.0, 24.0)
        .center()
        .radius(6.0)
        .role("button")
        .label(download_label.clone())
        .help(download_label)
        .child(icon(&icons::DOWNLOAD, 12.0).color(t.c.muted_foreground));
    let (identity, download) = match url {
        Some(url) => {
            let (id, name) = (s(attachment, "id").to_owned(), s(attachment, "name").to_owned());
            (
                identity
                    .focusable(true)
                    .role("button")
                    .cursor(rune::Cursor::Pointer)
                    .label(format!("Preview {label}"))
                    .on_press(move |app: &mut App| crate::panel::open_attachment(app, &id)),
                download
                    .focusable(true)
                    .cursor(rune::Cursor::Pointer)
                    .hover(t.c.muted)
                    .on_press_tx(move |app: &mut App, tx| media::save_attachment(app, url.clone(), name.clone(), tx)),
            )
        }
        None => (identity, download.disabled(true).opacity(0.64)),
    };
    row().w(256.0).gap(4.0).py(8.0).pl(8.0).pr(4.0).center_y().radius(16.0).fill(t.c.message_surface).child(identity).child(download)
}

pub(crate) fn panel_name<'a>(app: &'a App, id: &str) -> Option<&'a str> {
    app.ui.chat.timeline.assets.ready.get(id).map(|loaded| loaded.name.as_str())
}

pub(crate) fn panel_view(app: &App, id: &str, t: &T) -> El {
    let assets = &app.ui.chat.timeline.assets;
    let mut content = col().grow(1.).min_h(0.).w_full().center().gap(8.).padding(16.);
    let id = id.to_owned();
    let Some(loaded) = assets.ready.get(&id) else {
        let message = if assets.failed.contains(&id) { "Unable to load attachment." } else { "Loading attachment…" };
        return content.child(text(message).font(theme::TEXT_SM).color(t.c.muted_foreground));
    };
    let (url, name) = (loaded.url.clone(), loaded.name.clone());
    let browser = url.clone();
    content = match loaded.kind {
        Some(Kind::Image) => content.child(el().w_full().grow(1.).min_h(0.).pixels_fit(&loaded.pixels, rune::SurfaceFit::Fit)),
        Some(Kind::Video) => content.child(media::video(&id, &name, Some(loaded), false, None, t)),
        None => content.child(text(&name).font(theme::SM_MEDIUM).color(t.c.foreground)),
    };
    content.child(row().gap(6.)
        .child(crate::kit::Button::new("Open in preview browser").variant("outline").size("xs")
            .on_press(move |s: &mut App| crate::panel::open_preview(s, &browser)).build(t))
        .child(crate::kit::Button::new("Download").variant("outline").size("xs").build(t)
            .on_press_tx(move |s: &mut App, tx: &mut Tx| media::save_attachment(s, url.clone(), name.clone(), tx))))
}

/// The pictures sent with the message holding attachment `id`, in order.
fn siblings(app: &App, id: &str) -> Vec<String> {
    let crate::Route::Thread(thread) = app.frame_route() else { return Vec::new() };
    let Some(view) = app.state().thread_views.get(thread) else { return Vec::new() };
    super::rows::derive(&view.projection, app.now_ms)
        .into_iter()
        .find_map(|row| {
            let super::rows::Row::User(message) = row else { return None };
            let list = super::rows::array(message, "attachments");
            list.iter()
                .any(|a| s(a, "id") == id)
                .then(|| list.iter().filter(|a| s(a, "mimeType").starts_with("image/")).map(|a| s(a, "id").to_owned()).collect())
        })
        .unwrap_or_default()
}

/// The open picture's `SnapShotSource`, when it is a snapshot sent in this thread.
fn snapshot_source<'a>(app: &'a App, id: &str) -> Option<&'a Value> {
    let crate::Route::Thread(thread) = app.frame_route() else { return None };
    let p = &app.state().thread_views.get(thread)?.projection;
    let messages = p["messages"].as_array().into_iter().flatten();
    let items = p["visibleTurnItems"].as_array().into_iter().flatten().map(|i| &i["item"]);
    let attachment = messages.chain(items).flat_map(|m| m["attachments"].as_array().into_iter().flatten()).find(|a| s(a, "id") == id)?;
    Some(&attachment["source"]).filter(|source| snapshot::is_snapshot(source))
}

fn close(app: &mut App) {
    app.ui.chat.timeline.assets.open = None;
    app.ui.chat.timeline.assets.gallery.clear();
    app.ui.chat.timeline.assets.pan = None;
}

/// The open picture's place in its gallery and the gallery's size, when it has others.
fn place(assets: &Assets) -> Option<(usize, usize)> {
    let open = assets.open.as_ref()?;
    let index = assets.gallery.iter().position(|k| k == open)?;
    (assets.gallery.len() > 1).then_some((index, assets.gallery.len()))
}

/// Previous (-1) or Next (1), wrapping around (ExpandedImageDialog's navigateImage).
fn step(app: &mut App, by: isize) {
    let assets = &mut app.ui.chat.timeline.assets;
    if let Some((index, count)) = place(assets) {
        let next = (index as isize + by).rem_euclid(count as isize) as usize;
        assets.open = Some(assets.gallery[next].clone());
        assets.pan = None;
    }
}

/// The fitted picture's size at `zoom`, and its viewport: the picture capped to the stage. At 1
/// the picture fits the stage and never draws past its own pixels.
fn frame(stage: [f32; 2], natural: [f32; 2], zoom: f32) -> ([f32; 2], [f32; 2]) {
    let fit = (stage[0] / natural[0].max(1.0)).min(stage[1] / natural[1].max(1.0)).min(1.0);
    let content = natural.map(|n| n * fit * zoom);
    (content, [content[0].min(stage[0]), content[1].min(stage[1])])
}

impl View {
    const FIT: View = View { zoom: 1.0, scroll: [0.0, 0.0] };

    fn clamped(self, stage: [f32; 2], natural: [f32; 2]) -> Self {
        let (content, viewport) = frame(stage, natural, self.zoom);
        let scroll = [0usize, 1].map(|i| self.scroll[i].clamp(0.0, (content[i] - viewport[i]).max(0.0)));
        View { scroll, ..self }
    }

    /// changeZoom: `next` clamped to 1..=8, keeping the picture under `at` (viewport points; its
    /// centre when None) in place. The viewport stays centred in the stage as it grows.
    fn zoom_to(self, next: f32, at: Option<[f32; 2]>, stage: [f32; 2], natural: [f32; 2]) -> Self {
        let zoom = next.clamp(1.0, MAX_ZOOM);
        if !zoom.is_finite() || zoom == self.zoom {
            return self;
        }
        let (_, before) = frame(stage, natural, self.zoom);
        let (_, after) = frame(stage, natural, zoom);
        let at = at.unwrap_or([before[0] / 2.0, before[1] / 2.0]);
        let scroll = [0usize, 1].map(|i| {
            let anchor = (self.scroll[i] + at[i]) / self.zoom;
            anchor * zoom - (at[i] + (after[i] - before[i]) / 2.0)
        });
        View { zoom, scroll }.clamped(stage, natural)
    }

    /// A click or Enter: 2x, or back to fit once zoomed.
    fn toggle(self, at: Option<[f32; 2]>, stage: [f32; 2], natural: [f32; 2]) -> Self {
        self.zoom_to(if self.zoom > 1.0 { 1.0 } else { 2.0 }, at, stage, natural)
    }

    /// ZoomableImage's keys: Enter or Space toggles, + and - step 1.5x, 0 fits, and once zoomed
    /// the arrows pan 40pt (← and → only when the picture is wider than its viewport, so they
    /// still step the gallery otherwise).
    fn keyed(self, key: &Key, stage: [f32; 2], natural: [f32; 2]) -> Option<Self> {
        let (content, viewport) = frame(stage, natural, self.zoom);
        let pan = |dx: f32, dy: f32| Some(View { scroll: [self.scroll[0] + dx, self.scroll[1] + dy], ..self }.clamped(stage, natural));
        let zoomed = self.zoom > 1.0;
        match key {
            Key::Enter => Some(self.toggle(None, stage, natural)),
            Key::Text(c) if c == " " => Some(self.toggle(None, stage, natural)),
            Key::Text(c) if c == "+" || c == "=" => Some(self.zoom_to(self.zoom * 1.5, None, stage, natural)),
            Key::Text(c) if c == "-" => Some(self.zoom_to(self.zoom / 1.5, None, stage, natural)),
            Key::Text(c) if c == "0" => Some(self.zoom_to(1.0, None, stage, natural)),
            Key::Left if zoomed && content[0] > viewport[0] => pan(-40.0, 0.0),
            Key::Right if zoomed && content[0] > viewport[0] => pan(40.0, 0.0),
            Key::Up if zoomed => pan(0.0, -40.0),
            Key::Down if zoomed => pan(0.0, 40.0),
            _ => None,
        }
    }
}

/// The open picture's zoom, its scroll clamped to the stage it shows in now.
fn current(assets: &Assets, key: &str, stage: [f32; 2], natural: [f32; 2]) -> (View, bool) {
    assets.pan.as_ref().filter(|z| z.key == key).map_or((View::FIT, false), |z| (z.view.clamped(stage, natural), z.grab.is_some()))
}

fn zoomed<'a>(app: &'a mut App, key: &str, stage: [f32; 2], natural: [f32; 2]) -> &'a mut Zoomed {
    let pan = &mut app.ui.chat.timeline.assets.pan;
    if pan.as_ref().is_some_and(|z| z.key != key) {
        *pan = None;
    }
    let z = pan.get_or_insert_with(|| Zoomed { key: key.to_owned(), view: View::FIT, stage, natural, grab: None });
    z.view = z.view.clamped(stage, natural);
    z.stage = stage;
    z.natural = natural;
    z
}

fn zoom_by(app: &mut App, key: &str, stage: [f32; 2], natural: [f32; 2], k: f32, at: [f32; 2], tx: &mut Tx) {
    tx.instant();
    let z = zoomed(app, key, stage, natural);
    z.view = z.view.zoom_to(z.view.zoom * k, Some(at), stage, natural);
}

/// ZoomableImage: the picture in a viewport the size of the fitted picture. Click toggles 2x at
/// the pointer, the wheel or a pinch zooms there, a drag pans once zoomed, and the keys as in
/// [`View::keyed`]. It repaints only on input.
fn zoomable(app: &App, key: &str, name: &str, pixels: &rune::Pixels, stage: [f32; 2]) -> El {
    let (w, h) = pixels.size();
    let natural = [w as f32, h as f32];
    let (view, grabbing) = current(&app.ui.chat.timeline.assets, key, stage, natural);
    let (content, viewport) = frame(stage, natural, view.zoom);
    let cursor = match grabbing {
        _ if view.zoom <= 1.0 => rune::Cursor::Pointer,
        true => rune::Cursor::Grabbing,
        false => rune::Cursor::Grab,
    };
    let (wheel, pinch, drag, keys) = (key.to_owned(), key.to_owned(), key.to_owned(), key.to_owned());
    el().key("image-lightbox-viewport")
        .size(viewport[0], viewport[1])
        .clip_rounded(theme::radius::LG)
        .instant_changes()
        .focusable(true)
        .role("region")
        .label(format!("{name}, zoomable image"))
        .description("Click to zoom in or return to fit. Scroll to zoom, drag to pan. Use Enter to toggle zoom, plus or minus to zoom, and 0 to fit.")
        .a11y_value(format!("{}% zoom", (view.zoom * 100.0).round()))
        .cursor(cursor)
        .child(el().absolute(-view.scroll[0], -view.scroll[1]).size(content[0], content[1]).pixels(pixels))
        // Wheel deltas follow the content, so a wheel turned toward you (content up) zooms out.
        .on_wheel(move |app: &mut App, e: rune::WheelEvent, tx: &mut Tx| {
            if e.dy == 0.0 {
                return false;
            }
            zoom_by(app, &wheel, stage, natural, (e.dy * 0.002).exp(), [e.x, e.y], tx);
            true
        })
        .on_magnify(move |app: &mut App, e: rune::MagnifyEvent, tx: &mut Tx| {
            zoom_by(app, &pinch, stage, natural, 1.0 + e.delta, [e.x, e.y], tx);
            true
        })
        .on_drag(move |app: &mut App, d: &mut Drag, tx: &mut Tx| {
            tx.instant();
            // Layout points per window point (the app zoom).
            let per = crate::desktop::from_window(1.0, 1.0).0;
            let z = zoomed(app, &drag, stage, natural);
            match d.phase {
                Phase::Down => z.grab = (z.view.zoom > 1.0).then_some(z.view.scroll),
                Phase::Move => {
                    if let Some(from) = z.grab {
                        z.view = View { scroll: [from[0] - d.dx * per, from[1] - d.dy * per], ..z.view }.clamped(stage, natural);
                    }
                }
                Phase::Up => {
                    let from = z.grab.take();
                    if d.cancelled() {
                        if let Some(from) = from {
                            z.view.scroll = from;
                        }
                    // A drag past 4pt only pans; the second click of a double click does nothing.
                    } else if d.dx.hypot(d.dy) * per <= 4.0 && d.clicks <= 1 {
                        z.view = z.view.toggle(Some([d.x, d.y]), stage, natural);
                    }
                }
            }
        })
        .on_key(move |app: &mut App, e: &rune::KeyEvent, tx: &mut Tx| {
            if e.mods.cmd || e.mods.ctrl || e.mods.alt {
                return false;
            }
            let (view, _) = current(&app.ui.chat.timeline.assets, &keys, stage, natural);
            let Some(next) = view.keyed(&e.key, stage, natural) else { return false };
            tx.instant();
            zoomed(app, &keys, stage, natural).view = next;
            true
        })
}

/// Scenes: the open picture decoded for the lightbox, zoomed to `zoom` and scrolled by `scroll`.
pub(super) fn scene_zoom(app: &mut App, zoom: f32, scroll: [f32; 2]) {
    let Some(key) = app.ui.chat.timeline.assets.open.clone() else { return };
    let Some((_, Some(loaded))) = media::find(app, &key) else { return };
    let (w, h) = loaded.pixels.size();
    let assets = &mut app.ui.chat.timeline.assets;
    assets.zoom = Some((key.clone(), loaded, true));
    assets.pan = Some(Zoomed { key, view: View { zoom, scroll }, stage: [0.0; 2], natural: [w as f32, h as f32], grab: None });
}

/// Escape closes the lightbox; once zoomed the arrows pan, else ← and → step through its gallery.
pub fn on_key(app: &mut App, e: &rune::KeyEvent) -> bool {
    if app.ui.chat.timeline.assets.open.is_none() {
        return false;
    }
    let assets = &mut app.ui.chat.timeline.assets;
    let open = assets.open.clone();
    if matches!(e.key, Key::Left | Key::Right | Key::Up | Key::Down)
        && let Some(z) = assets.pan.as_mut().filter(|z| open.as_ref() == Some(&z.key))
        && let Some(next) = z.view.keyed(&e.key, z.stage, z.natural)
    {
        z.view = next;
        return true;
    }
    match e.key {
        rune::Key::Escape => close(app),
        rune::Key::Left if place(&app.ui.chat.timeline.assets).is_some() => step(app, -1),
        rune::Key::Right if place(&app.ui.chat.timeline.assets).is_some() => step(app, 1),
        _ => return false,
    }
    true
}

/// The open image over the chat (ExpandedImageDialog): click outside, X or Escape closes it, and
/// right-click offers the media actions.
pub fn lightbox(app: &App) -> Option<El> {
    let id = app.ui.chat.timeline.assets.open.as_ref()?;
    let (source, loaded) = media::find(app, id)?;
    let loaded = loaded?;
    let url = loaded.url.clone();
    let zoom = app.ui.chat.timeline.assets.zoom.as_ref().filter(|(k, z, _)| k == id && z.pixels.size().0 > 0);
    // The thumbnail fills in, fitted, until the full picture decodes and can zoom.
    let stage = rune::memo::layout_size(STAGE).map(|s| s.map(f32::round)).filter(|s| s[0] >= 1.0 && s[1] >= 1.0);
    let picture = match (zoom, stage) {
        (Some((_, z, _)), Some(stage)) => {
            let viewport = media::with_actions(zoomable(app, id, &loaded.name, &z.pixels, stage), id, &source, false);
            el().key(STAGE).w_full().grow(1.0).min_h(0.0).center().child(viewport)
        }
        _ => {
            let pixels = zoom.map_or(&loaded.pixels, |(_, z, _)| &z.pixels);
            media::with_actions(el().key(STAGE).w_full().grow(1.0).min_h(0.0).pixels_fit(pixels, rune::SurfaceFit::Fit), id, &source, false)
        }
    };
    let source = snapshot_source(app, id);
    let details = source.and_then(snapshot::details);
    let showing = details.is_some() && app.ui.chat.snapshot.lightbox.as_ref() == Some(id);
    // SnapShotAccessibilityData in the picture's place (the lightbox is dark in both themes).
    let picture = match details.as_ref().filter(|_| showing) {
        Some(d) => {
            let dark = theme::theme(true);
            let panel = el()
                .w_full()
                .max_w(672.0)
                .max_h(640.0)
                .padding(16.0)
                .radius(theme::radius::LG)
                .fill(dark.c.background)
                .border(1.0, dark.c.border.alpha_mul(0.7))
                .child(snapshot::data_view(app, &dark, d, &format!("lightbox-data-{id}")))
                .scroll(rune::ScrollSpec { stick_bottom: false });
            el().key(STAGE).w_full().grow(1.0).min_h(0.0).center().child(panel)
        }
        None => picture,
    };
    let contents = match (&details, source) {
        (Some(d), _) => {
            let label = if showing { "Show screenshot" } else if d.json { "Show accessibility JSON" } else { "Show extracted text" };
            let open = id.clone();
            Some(
                el().key("image-lightbox-contents")
                    .size(18.0, 18.0)
                    .center()
                    .radius(theme::radius::SM)
                    .fill(rune::Color::BLACK.alpha(0.35))
                    .hover(rune::Color::BLACK.alpha(0.6))
                    .cursor(rune::Cursor::Pointer)
                    .role("button")
                    .label(label)
                    .help(label)
                    .child(icon(if showing { &icons::IMAGE } else { &icons::TEXT }, 12.0).color(rune::Color::WHITE))
                    .on_press(move |app: &mut App| {
                        let shown = &mut app.ui.chat.snapshot.lightbox;
                        *shown = if shown.as_ref() == Some(&open) { None } else { Some(open.clone()) };
                    }),
            )
        }
        (None, Some(source)) => Some(snapshot::contents_button(app, source, "image-lightbox-contents".into())),
        _ => None,
    };
    let place = place(&app.ui.chat.timeline.assets);
    let caption = match place {
        Some((index, count)) => format!("{} ({}/{count})", loaded.name, index + 1),
        None => loaded.name.clone(),
    };
    let mut menu = Vec::new();
    // The lightbox is dark in both themes, and so is its menu.
    media::menu(app, &crate::kit::theme(&theme::theme(true)), &mut menu);
    let button = |key: &str, glyph: &'static rune::Icon, label: &str| {
        el().key(key.to_owned())
            .size(32.0, 32.0)
            .center()
            .radius(theme::radius::FULL)
            .fill(rune::Color::BLACK.alpha(0.5))
            .hover(rune::Color::BLACK.alpha(0.7))
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(label.to_owned())
            .help(label.to_owned())
            .child(icon(glyph, 16.0).color(rune::Color::WHITE))
    };
    let lightbox = el().key("image-lightbox")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .padding(48.0)
            .center()
            .fill(rune::Color::BLACK.alpha(0.8))
            .child(
                el().w_full()
                    .h_full()
                    .role("dialog")
                    .label("Image preview")
                    .modal(true, close)
                    .child(
                        row()
                            .w_full()
                            .h_full()
                            .gap(12.0)
                            .center_y()
                            .children(place.map(|_| button("image-lightbox-previous", &icons::CHEVRON_LEFT, "Previous media").on_press(|app: &mut App| step(app, -1))))
                            .child(
                                col()
                                    .grow(1.0)
                                    .min_w(0.0)
                                    .h_full()
                                    .gap(8.0)
                                    .child(picture)
                                    .child(row().w_full().justify_center().center_y().gap(6.0).child(text(caption).font(theme::TEXT_XS).color(rune::Color::WHITE.alpha(0.8)).truncate()).children(contents)),
                            )
                            .children(place.map(|_| button("image-lightbox-next", &icons::CHEVRON_RIGHT, "Next media").on_press(|app: &mut App| step(app, 1)))),
                    )
                    .child(
                row()
                    .pin(None, Some(0.0), Some(0.0), None)
                    .gap(8.0)
                    .child(button("image-lightbox-open", &icons::EXTERNAL_LINK, "Open in browser").on_press_tx(move |_: &mut App, tx| {
                        let _ = tx.open_url(&url);
                    }))
                    .child(button("image-lightbox-close", &icons::X, "Close").on_press(close)),
                    ),
            );
    Some(el().key("image-lightbox-layer").absolute(0.0, 0.0).w_full().h_full().child(lightbox).children(menu))
}

#[cfg(test)]
mod tests {
    use super::{View, file_label, format_bytes, frame};
    use rune::Key;
    use serde_json::json;

    #[test]
    fn pictures_fit_without_growing_and_zoom_from_one_to_eight() {
        let stage = [800.0, 600.0];
        assert_eq!(frame(stage, [400.0, 300.0], 1.0), ([400.0, 300.0], [400.0, 300.0]), "a small picture keeps its size");
        assert_eq!(frame(stage, [1600.0, 1200.0], 2.0), ([1600.0, 1200.0], [800.0, 600.0]), "zoomed past the stage, it scrolls");
        let big = [1600.0, 1200.0];
        let fit = View::FIT;
        assert_eq!(fit.zoom_to(0.5, None, stage, big), fit);
        assert_eq!(fit.zoom_to(100.0, None, stage, big).zoom, 8.0);
        // The point under the pointer stays put: (scroll + at) / zoom is the same before and after.
        let zoomed = fit.zoom_to(2.0, Some([200.0, 150.0]), stage, big);
        assert_eq!(zoomed, View { zoom: 2.0, scroll: [200.0, 150.0] });
        assert_eq!(zoomed.toggle(None, stage, big), fit, "a second click fits again");
        // A viewport growing with the zoom can't scroll before the picture's edge.
        assert_eq!(fit.zoom_to(2.0, Some([100.0, 75.0]), stage, [400.0, 300.0]).scroll, [0.0, 0.0]);
    }

    #[test]
    fn keys_zoom_and_pan_only_when_zoomed() {
        let (stage, big) = ([800.0, 600.0], [1600.0, 1200.0]);
        let fit = View::FIT;
        assert_eq!(fit.keyed(&Key::Left, stage, big), None, "at fit, ← steps the gallery");
        assert_eq!(fit.keyed(&Key::Enter, stage, big).map(|v| v.zoom), Some(2.0));
        assert_eq!(fit.keyed(&Key::Text("+".into()), stage, big).map(|v| v.zoom), Some(1.5));
        let zoomed = View { zoom: 2.0, scroll: [790.0, 0.0] };
        assert_eq!(zoomed.keyed(&Key::Right, stage, big).map(|v| v.scroll), Some([800.0, 0.0]), "panning stops at the edge");
        assert_eq!(zoomed.keyed(&Key::Up, stage, big).map(|v| v.scroll), Some([790.0, 0.0]));
        assert_eq!(zoomed.keyed(&Key::Text("0".into()), stage, big), Some(View::FIT));
        // A tall picture no wider than its viewport leaves ← and → to the gallery.
        let tall = View { zoom: 2.0, scroll: [0.0, 0.0] };
        assert_eq!(tall.keyed(&Key::Left, stage, [400.0, 1200.0]), None);
        assert!(tall.keyed(&Key::Down, stage, [400.0, 1200.0]).is_some());
    }

    #[test]
    fn file_cards_read_their_kind_and_size() {
        assert_eq!(format_bytes(512.0), "512 B");
        assert_eq!(format_bytes(1229.0), "1.20 KB");
        assert_eq!(format_bytes(12.5 * 1024.0 * 1024.0), "12.5 MB");
        assert_eq!(format_bytes(1023.6 * 1024.0), "1.00 MB");
        let file = |name: &str| json!({"name": name, "sizeBytes": 2048});
        assert_eq!(file_label(&file("notes.md")), ("notes.md".to_owned(), "MD · 2.00 KB".to_owned()));
        assert_eq!(file_label(&file("Makefile")).1, "File · 2.00 KB");
        let pasted = json!({"name": "pasted-1.txt", "sizeBytes": 300, "source": {"_tag": "pasted-text"}});
        assert_eq!(file_label(&pasted), ("Pasted text".to_owned(), "300 B".to_owned()));
    }
}
