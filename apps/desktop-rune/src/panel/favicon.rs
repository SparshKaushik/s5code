//! Page favicons for the tab strip and the empty state's cards. As web's `browserFaviconStore`,
//! a page's captured icon stands for its origin; loopback spellings share one. A public site
//! without one falls back to Google's favicon service, as web's `faviconUrlForOrigin`, and the
//! icons that loaded are kept in a file so they show at once after a restart.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use base64::Engine;
use rune::{El, Image, ImageCache, col, el, row};

use crate::App;
use crate::theme::{AlphaMul, T};

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_ICONS: usize = 64;
const FILE: &str = "browser-favicons.json";
/// Larger icons are shown but not kept, so the file stays small.
const MAX_SAVED_BYTES: usize = 64 * 1024;

enum Fetched {
    Svg(Vec<u8>),
    Rgba(u32, u32, Vec<u8>),
    Encoded(Vec<u8>),
}

type Slot = Arc<Mutex<Option<Option<(Vec<u8>, Fetched)>>>>;

enum Entry {
    Loading(Slot),
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
    /// Each loaded icon's bytes, for the file.
    bytes: HashMap<String, Vec<u8>>,
    loaded: bool,
    dirty: bool,
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

fn fetch(url: &str) -> Option<(Vec<u8>, Fetched)> {
    let bytes = fetch_bytes(url)?;
    decode(bytes.clone()).map(|f| (bytes, f))
}

fn fetch_bytes(url: &str) -> Option<Vec<u8>> {
    if let Some(data) = url.strip_prefix("data:") {
        let (meta, payload) = data.split_once(',')?;
        return if meta.ends_with(";base64") { base64::engine::general_purpose::STANDARD.decode(payload).ok() } else { Some(payload.as_bytes().to_vec()) };
    }
    let response = ureq::get(url).timeout(std::time::Duration::from_secs(10)).call().ok()?;
    if !response.content_type().starts_with("image/") && !url.ends_with(".ico") {
        return None;
    }
    let mut bytes = Vec::new();
    use std::io::Read;
    response.into_reader().take(MAX_BYTES).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Web's `faviconUrlForOrigin`: Google's icon for a public http(s) host, never a private one.
fn fallback_url(page: &str) -> Option<String> {
    let rest = page.strip_prefix("https://").or_else(|| page.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(v6) = authority.strip_prefix('[') { v6.split(']').next()? } else { authority.split(':').next()? };
    (!host.is_empty() && public_host(host)).then(|| format!("https://www.google.com/s2/favicons?domain={}&sz=32", host.to_ascii_lowercase()))
}

/// Web's `isPublicFaviconHost`, so local and internal names never reach a third party.
fn public_host(host: &str) -> bool {
    if host.ends_with("..") {
        return false;
    }
    let host = host.to_ascii_lowercase();
    let host = host.trim_end_matches('.');
    const PRIVATE_TLDS: [&str; 11] = ["alt", "corp", "example", "home", "internal", "intranet", "invalid", "lan", "onion", "private", "test"];
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") || host == "home.arpa" || host.ends_with(".home.arpa") || host.ends_with(".ts.net") {
        return false;
    }
    if let Ok(v6) = host.parse::<std::net::Ipv6Addr>() {
        let s = v6.segments();
        let embedded = || std::net::Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8);
        if s[..6] == [0, 0, 0, 0, 0, 0xffff] || s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
            return public_v4(embedded());
        }
        if s[0] & 0xe000 != 0x2000 {
            return false;
        }
        if s[0] == 0x2001 && s[1] < 0x200 {
            // Within the protocol-assignment block only these are public.
            return (s[1] == 1 && s[2..7] == [0; 5] && (1..=3).contains(&s[7])) || s[1] == 3 || (s[1] == 4 && s[2] == 0x112) || (s[1] & 0xfff0) == 0x20 || (s[1] & 0xfff0) == 0x30;
        }
        return !(s[0] == 0x2001 && s[1] == 0xdb8) && s[0] != 0x2002 && !(s[0] == 0x3fff && s[1] & 0xf000 == 0);
    }
    if let Ok(v4) = host.parse::<std::net::Ipv4Addr>() {
        return public_v4(v4);
    }
    if !host.contains('.') {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    // No TLD is numeric, so this is an address in a spelling the parser refused, such as `010.0.0.1`.
    if labels.last().is_some_and(|l| l.chars().all(|c| c.is_ascii_digit())) {
        return false;
    }
    !PRIVATE_TLDS.contains(labels.last().unwrap_or(&"")) && !labels.iter().any(|l| matches!(*l, "corp" | "internal" | "intranet"))
}

fn public_v4(ip: std::net::Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    let private = a == 0 || a == 10 || a == 127 || (a == 100 && (64..=127).contains(&b)) || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168) || (a == 169 && b == 254) || (a == 198 && (18..=19).contains(&b));
    let special = a >= 224 || (a == 192 && b == 0 && (c == 0 || c == 2)) || (a == 192 && b == 88 && c == 99) || (a == 198 && b == 51 && c == 100) || (a == 203 && b == 0 && c == 113);
    !private && !special
}

fn start_fetch(icons: &mut Favicons, url: &str, waker: Option<rune::Waker>) {
    if icons.icons.len() >= MAX_ICONS {
        let live: std::collections::HashSet<&String> = icons.by_origin.values().collect();
        let stale: Vec<String> = icons.icons.keys().filter(|k| !live.contains(k)).cloned().collect();
        for k in stale {
            icons.icons.remove(&k);
            icons.bytes.remove(&k);
        }
    }
    let slot: Slot = Arc::new(Mutex::new(None));
    let (out, owned) = (slot.clone(), url.to_string());
    std::thread::spawn(move || {
        let fetched = fetch(&owned);
        *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(fetched);
        if let Some(waker) = waker {
            waker();
        }
    });
    icons.icons.insert(url.to_string(), Entry::Loading(slot));
}

fn usable(entry: Option<&Entry>) -> Option<&Image> {
    match entry {
        Some(Entry::Ready(image)) if image.error().is_none() => Some(image),
        _ => None,
    }
}

/// A shown page with no icon of its own yet gets Google's, once per host.
pub fn want_fallback(app: &mut App, page: &str) {
    let Some(url) = fallback_url(page) else { return };
    let waker = app.ui.panel.waker.clone();
    let icons = &mut app.ui.panel.browser.favicons;
    let captured = super::history::origin_key(page).and_then(|o| icons.by_origin.get(&o)).map(|u| icons.icons.get(u));
    if matches!(captured, Some(Some(Entry::Loading(_)))) || usable(captured.flatten()).is_some() || icons.icons.contains_key(&url) {
        return;
    }
    start_fetch(icons, &url, waker);
}

/// A page just reported its icon.
pub fn note(app: &mut App, page: &str, icon: &str) {
    let Some(origin) = super::history::origin_key(page) else { return };
    let waker = app.ui.panel.waker.clone();
    let icons = &mut app.ui.panel.browser.favicons;
    if icons.by_origin.insert(origin, icon.to_string()).as_deref() != Some(icon) {
        icons.dirty = true;
    }
    // A failed icon is fetched again on the page's next load, as the site may have added it since.
    if icons.icons.get(icon).is_some_and(|e| match e {
        Entry::Loading(_) => true,
        Entry::Ready(image) => image.error().is_none(),
        Entry::Missing => false,
    }) {
        return;
    }
    start_fetch(icons, icon, waker);
}

/// The kept icons: each origin's icon URL and each icon's bytes.
fn load(icons: &mut Favicons, saved: &serde_json::Value) {
    for (origin, url) in saved["origins"].as_object().into_iter().flatten() {
        if let Some(url) = url.as_str() {
            icons.by_origin.entry(origin.clone()).or_insert_with(|| url.to_owned());
        }
    }
    for (url, data) in saved["icons"].as_object().into_iter().flatten() {
        let Some(raw) = data.as_str().and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok()) else { continue };
        if icons.icons.contains_key(url) {
            continue;
        }
        let Some(fetched) = decode(raw.clone()) else { continue };
        icons.icons.insert(url.clone(), Entry::Loading(Arc::new(Mutex::new(Some(Some((raw, fetched)))))));
    }
}

fn to_json(icons: &Favicons) -> serde_json::Value {
    let kept: serde_json::Map<String, serde_json::Value> = icons.bytes.iter().map(|(url, raw)| (url.clone(), serde_json::json!(base64::engine::general_purpose::STANDARD.encode(raw)))).collect();
    let origins: serde_json::Map<String, serde_json::Value> = icons.by_origin.iter().filter(|(_, url)| icons.bytes.contains_key(*url)).map(|(o, url)| (o.clone(), serde_json::json!(url))).collect();
    serde_json::json!({ "version": 1, "origins": origins, "icons": kept })
}

/// Hands finished downloads to the decoder.
pub fn poll(app: &mut App) -> bool {
    let Some(waker) = app.ui.panel.waker.clone() else { return false };
    let path = app.client_settings.sibling(FILE);
    let icons = &mut app.ui.panel.browser.favicons;
    if !icons.loaded {
        icons.loaded = true;
        if let Some(saved) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).filter(|_| !cfg!(test)) {
            load(icons, &saved);
        }
    }
    let mut changed = false;
    for (url, entry) in icons.icons.iter_mut() {
        let Entry::Loading(slot) = entry else { continue };
        let Some(fetched) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() else { continue };
        let cache = icons.cache.get_or_insert_with(|| ImageCache::new(waker.clone()));
        *entry = match fetched {
            None => Entry::Missing,
            Some((raw, fetched)) => {
                if raw.len() <= MAX_SAVED_BYTES && icons.bytes.get(url) != Some(&raw) {
                    icons.bytes.insert(url.clone(), raw);
                    icons.dirty = true;
                }
                match fetched {
                    Fetched::Svg(bytes) => Entry::Ready(cache.svg(bytes, 28., 2.)),
                    Fetched::Rgba(w, h, bytes) => Entry::Ready(cache.rgba(w, h, bytes)),
                    Fetched::Encoded(bytes) => Entry::Ready(cache.load(bytes)),
                }
            }
        };
        changed = true;
    }
    // Written once nothing is loading, so a burst of loads is one write.
    if icons.dirty && !cfg!(test) && !icons.icons.values().any(|e| matches!(e, Entry::Loading(_))) {
        icons.dirty = false;
        super::write_state(&path, &to_json(icons));
    }
    changed
}

/// The page's origin icon at `size`, once it has decoded.
/// The page's origin icon at `size`, once it has decoded, else Google's for a public site.
pub fn icon(app: &App, page: &str, size: f32, radius: f32) -> Option<El> {
    let icons = &app.ui.panel.browser.favicons;
    let captured = super::history::origin_key(page).and_then(|o| icons.by_origin.get(&o)).and_then(|url| usable(icons.icons.get(url)));
    let image = captured.filter(|i| i.is_ready()).or_else(|| usable(icons.icons.get(&fallback_url(page)?)).filter(|i| i.is_ready()))?;
    Some(el().size(size, size).shrink().radius(radius).clip_rounded(radius).pixels_fit(image.pixels(), rune::SurfaceFit::Fit))
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
        assert!(matches!(fetch("data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg'/>"), Some((_, Fetched::Svg(_)))));
        let png = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\n");
        assert!(matches!(fetch(&format!("data:image/png;base64,{png}")), Some((_, Fetched::Encoded(_)))));
        assert!(fetch("data:image/png;base64,").is_none());
    }

    #[test]
    fn only_public_sites_fall_back_to_googles_icon() {
        assert_eq!(fallback_url("https://GitHub.com/a/b?c").as_deref(), Some("https://www.google.com/s2/favicons?domain=github.com&sz=32"));
        assert_eq!(fallback_url("http://8.8.8.8:8080/").as_deref(), Some("https://www.google.com/s2/favicons?domain=8.8.8.8&sz=32"));
        for private in [
            "http://localhost:5173/",
            "http://127.0.0.1/",
            "http://192.168.1.4/",
            "http://10.0.0.2/",
            "http://100.100.1.1/",
            "http://myhost:3000/",
            "https://box.ts.net/",
            "https://app.local/",
            "https://wiki.corp.example.com/",
            "https://site.test/",
            "https://intranet.acme.com/",
            "http://[::1]/",
            "http://[fd7a:115c:a1e0::1]/",
            "http://[::ffff:192.168.0.1]/",
            "http://[2001:db8::1]/",
            "http://[2001::1]/",
            "http://[2002::1]/",
            "file:///tmp/a.html",
            "about:blank",
        ] {
            assert_eq!(fallback_url(private), None, "{private}");
        }
        assert!(fallback_url("http://[2606:4700::1111]/").is_some());
    }

    #[test]
    fn kept_icons_come_back_after_a_restart() {
        let png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut before = Favicons::default();
        before.by_origin.insert("https://a.dev".into(), "https://a.dev/i.png".into());
        before.by_origin.insert("https://b.dev".into(), "https://b.dev/never-loaded.png".into());
        before.bytes.insert("https://a.dev/i.png".into(), png.clone());
        let saved = to_json(&before);
        assert!(saved["origins"].get("https://b.dev").is_none(), "an icon that never loaded isn't kept");

        let mut after = Favicons::default();
        load(&mut after, &saved);
        assert_eq!(after.by_origin["https://a.dev"], "https://a.dev/i.png");
        let Some(Entry::Loading(slot)) = after.icons.get("https://a.dev/i.png") else { panic!("the kept icon decodes on the next poll") };
        assert!(matches!(slot.lock().unwrap().as_ref(), Some(Some((raw, Fetched::Encoded(_)))) if *raw == png));
    }
}
