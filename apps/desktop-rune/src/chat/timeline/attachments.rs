use crate::{
    App,
    data::s,
    icons,
    theme::{self, T},
};
use super::media::{self, Kind, Loaded};
use rune::{El, col, el, icon, row, text};
use serde_json::{Value, json};
use std::collections::HashMap;

/// An image bubble's 240x180 frame on a 2x display.
const THUMBNAIL_DECODE: u32 = 480;
/// The lightbox's own decode, held only while it is open.
const LIGHTBOX_DECODE: u32 = 2048;

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
    let mut content = if s(attachment, "mimeType").starts_with("image/") {
        let mut frame = el()
            .size(240.0, 180.0)
            .radius(16.0)
            .clip_rounded(16.0)
            .fill(t.c.message_surface);
        if let Some(loaded) = asset {
            frame = frame.pixels_fit(&loaded.pixels, rune::SurfaceFit::Fit);
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
            let (preview, name) = (url.clone(), s(attachment, "name").to_owned());
            (
                identity
                    .focusable(true)
                    .role("button")
                    .cursor(rune::Cursor::Pointer)
                    .label(format!("Preview {label}"))
                    .on_press(move |app: &mut App| crate::panel::open_preview(app, &preview)),
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

fn close(app: &mut App) {
    app.ui.chat.timeline.assets.open = None;
    app.ui.chat.timeline.assets.gallery.clear();
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
    }
}

/// Escape closes the lightbox; ← and → step through its gallery.
pub fn on_key(app: &mut App, e: &rune::KeyEvent) -> bool {
    if app.ui.chat.timeline.assets.open.is_none() {
        return false;
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
    let pixels = zoom.map_or(&loaded.pixels, |(_, z, _)| &z.pixels);
    let picture = el().w_full().grow(1.0).min_h(0.0).pixels_fit(pixels, rune::SurfaceFit::Fit);
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
                                    .child(media::with_actions(picture, id, &source, false))
                                    .child(row().w_full().justify_center().child(text(caption).font(theme::TEXT_XS).color(rune::Color::WHITE.alpha(0.8)).truncate())),
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
    use super::{file_label, format_bytes};
    use serde_json::json;

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
