use crate::{
    App,
    data::s,
    icons,
    theme::{self, T},
};
use rune::{El, Pixels, col, el, icon, row, text};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Default)]
pub struct Assets {
    pending: HashMap<String, u64>,
    ready: HashMap<String, (String, Pixels)>,
    failed: std::collections::HashSet<String>,
    /// The image the lightbox shows, by attachment id.
    pub open: Option<String>,
}

pub fn poll(app: &mut App) -> bool {
    let crate::Route::Thread(thread) = app.frame_route() else {
        return false;
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
    let Some(net) = app.src.net() else {
        return false;
    };
    let assets = &mut app.ui.chat.timeline.assets;
    let mut changed = false;
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
            let pixels = Pixels::new();
            if s(&a, "mimeType").starts_with("image/") {
                let (url, pixels) = (url.clone(), pixels.clone());
                std::thread::spawn(move || {
                    let Ok(response) = ureq::get(&url).call() else {
                        return;
                    };
                    let mut bytes = Vec::new();
                    use std::io::Read;
                    if response
                        .into_reader()
                        .take(20 * 1024 * 1024)
                        .read_to_end(&mut bytes)
                        .is_err()
                    {
                        return;
                    }
                    if let Ok(image) = image::load_from_memory(&bytes) {
                        let image = image.thumbnail(720, 540).to_rgba8();
                        pixels.set(image.width(), image.height(), image.into_raw());
                    }
                });
            }
            assets.ready.insert(id.to_owned(), (url, pixels));
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
    let mut content = if s(attachment, "mimeType").starts_with("image/") {
        let mut frame = el()
            .size(240.0, 180.0)
            .radius(16.0)
            .clip_rounded(16.0)
            .fill(t.c.message_surface);
        if let Some((_, pixels)) = asset {
            frame = frame.pixels_fit(pixels, rune::SurfaceFit::Fit);
        }
        frame
    } else {
        row()
            .w(256.0)
            .gap(12.0)
            .padding(8.0)
            .radius(16.0)
            .fill(t.c.message_surface)
            .child(
                icon(&icons::FILE, 20.0)
                    .padding(8.0)
                    .radius(8.0)
                    .fill(t.c.background),
            )
            .child({
                let (label, size) = file_label(attachment);
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .child(text(label).font(theme::SM_MEDIUM).color(t.c.message_foreground).truncate())
                    .child(text(size).font(theme::TEXT_XS).color(t.c.muted_foreground))
            })
            .child(icon(&icons::EXTERNAL_LINK, 12.0).color(t.c.muted_foreground))
    };
    if let Some((url, _)) = asset {
        let url = url.clone();
        let image = s(attachment, "mimeType").starts_with("image/");
        let id = id.to_owned();
        let shown = if image { name.to_owned() } else { file_label(attachment).0 };
        content = content
            .focusable(true)
            .role("button")
            .cursor(rune::Cursor::Pointer)
            .label(format!("{} {shown}", if image { "Preview" } else { "Open" }))
            .on_press_tx(move |app: &mut App, tx| {
                if image {
                    app.ui.chat.timeline.assets.open = Some(id.clone());
                } else {
                    let _ = tx.open_url(&url);
                }
            });
    }
    content
}

fn close(app: &mut App) {
    app.ui.chat.timeline.assets.open = None;
}

/// Escape closes the lightbox.
pub fn on_key(app: &mut App, e: &rune::KeyEvent) -> bool {
    if e.key == rune::Key::Escape && app.ui.chat.timeline.assets.open.is_some() {
        close(app);
        return true;
    }
    false
}

/// The open image over the chat (ImageLightbox): click outside, X or Escape closes it.
pub fn lightbox(app: &App) -> Option<El> {
    let id = app.ui.chat.timeline.assets.open.as_ref()?;
    let (url, pixels) = app.ui.chat.timeline.assets.ready.get(id)?;
    let url = url.clone();
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
    Some(
        el().key("image-lightbox")
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
                    .child(el().w_full().h_full().pixels_fit(pixels, rune::SurfaceFit::Fit))
                    .child(
                row()
                    .pin(None, Some(0.0), Some(0.0), None)
                    .gap(8.0)
                    .child(button("image-lightbox-open", &icons::EXTERNAL_LINK, "Open in browser").on_press_tx(move |_: &mut App, tx| {
                        let _ = tx.open_url(&url);
                    }))
                    .child(button("image-lightbox-close", &icons::X, "Close").on_press(close)),
                    ),
            ),
    )
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
