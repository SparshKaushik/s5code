//! Page favicons for the tab strip and the empty state's cards. As web's `browserFaviconStore`,
//! a page's captured icon stands for its origin; loopback spellings share one.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use base64::Engine;
use rune::{El, Image, ImageCache, col, el, row};

use crate::App;
use crate::theme::{AlphaMul, T};

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_ICONS: usize = 64;

enum Fetched {
    Svg(Vec<u8>),
    Rgba(u32, u32, Vec<u8>),
    Encoded(Vec<u8>),
}

enum Entry {
    Loading(Arc<Mutex<Option<Option<Fetched>>>>),
    Ready(Image),
    Missing,
}

#[derive(Default)]
pub struct Favicons {
    /// By icon URL.
    icons: HashMap<String, Entry>,
    /// Each origin's latest icon URL.
    by_origin: HashMap<String, String>,
    cache: Option<ImageCache>,
}

fn decode(bytes: Vec<u8>) -> Option<Fetched> {
    let start = bytes.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(bytes.len());
    Some(if bytes[start..].starts_with(b"<") {
        Fetched::Svg(bytes)
    } else if bytes.starts_with(&[0, 0, 1, 0]) {
        // The image cache does not read the ICO container.
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Ico).ok()?.thumbnail(64, 64).to_rgba8();
        Fetched::Rgba(image.width(), image.height(), image.into_raw())
    } else if bytes.is_empty() {
        return None;
    } else {
        Fetched::Encoded(bytes)
    })
}

fn fetch(url: &str) -> Option<Fetched> {
    if let Some(data) = url.strip_prefix("data:") {
        let (meta, payload) = data.split_once(',')?;
        let bytes = if meta.ends_with(";base64") { base64::engine::general_purpose::STANDARD.decode(payload).ok()? } else { payload.as_bytes().to_vec() };
        return decode(bytes);
    }
    let response = ureq::get(url).timeout(std::time::Duration::from_secs(10)).call().ok()?;
    if !response.content_type().starts_with("image/") && !url.ends_with(".ico") {
        return None;
    }
    let mut bytes = Vec::new();
    use std::io::Read;
    response.into_reader().take(MAX_BYTES).read_to_end(&mut bytes).ok()?;
    decode(bytes)
}

/// A page just reported its icon.
pub fn note(app: &mut App, page: &str, icon: &str) {
    let Some(origin) = super::history::origin_key(page) else { return };
    let waker = app.ui.panel.waker.clone();
    let icons = &mut app.ui.panel.browser.favicons;
    icons.by_origin.insert(origin, icon.to_string());
    // A failed icon is fetched again on the page's next load, as the site may have added it since.
    if icons.icons.get(icon).is_some_and(|e| match e {
        Entry::Loading(_) => true,
        Entry::Ready(image) => image.error().is_none(),
        Entry::Missing => false,
    }) {
        return;
    }
    if icons.icons.len() >= MAX_ICONS {
        let live: std::collections::HashSet<&String> = icons.by_origin.values().collect();
        let stale: Vec<String> = icons.icons.keys().filter(|k| !live.contains(k)).cloned().collect();
        for k in stale {
            icons.icons.remove(&k);
        }
    }
    let slot = Arc::new(Mutex::new(None));
    let (out, url) = (slot.clone(), icon.to_string());
    std::thread::spawn(move || {
        let fetched = fetch(&url);
        *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(fetched);
        if let Some(waker) = waker {
            waker();
        }
    });
    icons.icons.insert(icon.to_string(), Entry::Loading(slot));
}

/// Hands finished downloads to the decoder.
pub fn poll(app: &mut App) -> bool {
    let Some(waker) = app.ui.panel.waker.clone() else { return false };
    let icons = &mut app.ui.panel.browser.favicons;
    let mut changed = false;
    for entry in icons.icons.values_mut() {
        let Entry::Loading(slot) = entry else { continue };
        let Some(fetched) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() else { continue };
        let cache = icons.cache.get_or_insert_with(|| ImageCache::new(waker.clone()));
        *entry = match fetched {
            None => Entry::Missing,
            Some(Fetched::Svg(bytes)) => Entry::Ready(cache.svg(bytes, 28., 2.)),
            Some(Fetched::Rgba(w, h, bytes)) => Entry::Ready(cache.rgba(w, h, bytes)),
            Some(Fetched::Encoded(bytes)) => Entry::Ready(cache.load(bytes)),
        };
        changed = true;
    }
    changed
}

/// The page's origin icon at `size`, once it has decoded.
pub fn icon(app: &App, page: &str, size: f32, radius: f32) -> Option<El> {
    let icons = &app.ui.panel.browser.favicons;
    let url = icons.by_origin.get(&super::history::origin_key(page)?)?;
    let Some(Entry::Ready(image)) = icons.icons.get(url) else { return None };
    (image.is_ready() && image.error().is_none()).then(|| el().size(size, size).shrink().radius(radius).clip_rounded(radius).pixels_fit(image.pixels(), rune::SurfaceFit::Fit))
}

/// Web's `BrowserMockup`: a tiny window with traffic lights, for a page without an icon.
pub fn mockup(t: &T, size: f32) -> El {
    let dot = |c: rune::Color| el().size(3., 3.).radius(1.5).fill(c.alpha_mul(0.8));
    let line = |w: f32, a: f32| el().h(2.).w(w).radius(1.).fill(t.c.muted_foreground.alpha_mul(a));
    let inner = size - 10.;
    col()
        .size(size, size)
        .shrink()
        .padding(4.)
        .gap(2.)
        .radius(2.)
        .border(1., t.c.border.alpha_mul(0.6))
        .fill(t.c.card)
        .clip_rounded(2.)
        .child(row().gap(2.).child(dot(t.c.destructive)).child(dot(t.c.warning)).child(dot(t.c.success)))
        .child(col().mt(1.).gap(2.).child(line(inner, 0.3)).child(line(inner * 0.6, 0.2)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_decode_from_data_urls_and_reject_empty_bodies() {
        assert!(matches!(fetch("data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg'/>"), Some(Fetched::Svg(_))));
        let png = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\n");
        assert!(matches!(fetch(&format!("data:image/png;base64,{png}")), Some(Fetched::Encoded(_))));
        assert!(fetch("data:image/png;base64,").is_none());
    }
}
