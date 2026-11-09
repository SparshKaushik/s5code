//! Project favicons (`ProjectFavicon`): `assets.createUrl` names the project's icon file, which
//! downloads off the UI thread. A project with none, or one that fails to load, keeps its
//! monogram, as does every project until its icon has decoded.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rune::{El, Image, ImageCache, Waker, el};
use serde_json::json;

use crate::App;
use crate::theme::T;

/// Lookups and downloads in flight at once, as the web's favicon queue.
const MAX_IN_FLIGHT: usize = 6;
const MAX_BYTES: u64 = 4 * 1024 * 1024;
const MISSING: &str = "project-favicon-missing";
/// Rasterized for the largest place it draws (the 16 pt mark) at 2×; smaller draws use mips.
const SVG_PT: f32 = 16.0;

enum Fetched {
    Svg(Vec<u8>),
    Rgba(u32, u32, Vec<u8>),
    Encoded(Vec<u8>),
}

enum Entry {
    /// The request, and the base URL of the environment that owns the project.
    Asking(u64, String),
    Loading(Arc<Mutex<Option<Option<Fetched>>>>),
    Ready(Image),
    Missing,
}

#[derive(Default)]
pub struct Favicons {
    /// Keyed by project id; the folder (and override path) it was asked for rides along so a
    /// moved project asks again.
    entries: HashMap<String, (String, Entry)>,
    cache: Option<ImageCache>,
}

/// SVG text (or gzipped SVGZ) rather than a raster format.
fn is_svg(bytes: &[u8]) -> bool {
    let start = bytes.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(bytes.len());
    bytes[start..].starts_with(b"<") || bytes.starts_with(&[0x1f, 0x8b])
}

/// The ICO container (reserved 0, type 1), which the image cache does not read.
fn is_ico(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0, 0, 1, 0])
}

fn fetch(url: &str) -> Option<Fetched> {
    let response = ureq::get(url).timeout(std::time::Duration::from_secs(20)).call().ok()?;
    let mut bytes = Vec::new();
    use std::io::Read;
    response.into_reader().take(MAX_BYTES).read_to_end(&mut bytes).ok()?;
    Some(if is_svg(&bytes) {
        Fetched::Svg(bytes)
    } else if is_ico(&bytes) {
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Ico).ok()?.thumbnail(64, 64).to_rgba8();
        Fetched::Rgba(image.width(), image.height(), image.into_raw())
    } else {
        Fetched::Encoded(bytes)
    })
}

/// Asks for every project's icon a few at a time, and hands finished downloads to the decoder.
pub fn poll(s: &mut App) -> bool {
    let waker: Option<Waker> = s.ui.panel.waker.clone();
    let projects: Vec<(String, String, Option<String>)> = s
        .state()
        .projects
        .iter()
        .map(|p| (crate::data::s(p, "id").to_owned(), crate::data::s(p, "workspaceRoot").to_owned(), p["faviconPath"].as_str().map(str::to_owned)))
        .filter(|(id, cwd, _)| !id.is_empty() && !cwd.is_empty())
        .collect();
    let mut in_flight = s.ui.shell.sidebar.favicons.entries.values().filter(|(_, e)| matches!(e, Entry::Asking(..) | Entry::Loading(_))).count();
    for (id, cwd, path) in projects {
        let source = format!("{cwd}\u{0}{}", path.as_deref().unwrap_or(""));
        if s.ui.shell.sidebar.favicons.entries.get(&id).is_some_and(|(asked, _)| *asked == source) || in_flight >= MAX_IN_FLIGHT {
            continue;
        }
        let mut resource = json!({"_tag": "project-favicon", "cwd": cwd});
        if let Some(path) = path {
            resource["path"] = json!(path);
        }
        let Some(net) = crate::env::net_for(s, &id) else { continue };
        let base = net.base_url().trim_end_matches('/').to_owned();
        let request = net.request("assets.createUrl", json!({ "resource": resource }));
        s.ui.shell.sidebar.favicons.entries.insert(id, (source, Entry::Asking(request, base)));
        in_flight += 1;
    }
    let Some(net) = s.src.net() else { return false };
    let icons = &mut s.ui.shell.sidebar.favicons;
    let mut changed = false;
    for (_, entry) in icons.entries.values_mut() {
        match entry {
            Entry::Asking(request, base) => {
                let Some(reply) = net.take_reply(*request) else { continue };
                let url = reply.ok().map(|v| crate::data::s(&v, "relativeUrl").to_owned()).filter(|u| !u.is_empty() && !u.ends_with(MISSING));
                *entry = match url {
                    None => Entry::Missing,
                    Some(relative) => {
                        let slot = Arc::new(Mutex::new(None));
                        let (out, url, waker) = (slot.clone(), format!("{base}{relative}"), waker.clone());
                        std::thread::spawn(move || {
                            let fetched = fetch(&url);
                            *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(fetched);
                            if let Some(waker) = waker {
                                waker();
                            }
                        });
                        Entry::Loading(slot)
                    }
                };
                changed = true;
            }
            Entry::Loading(slot) => {
                let Some(waker) = waker.clone() else { continue };
                let Some(fetched) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() else { continue };
                let cache = icons.cache.get_or_insert_with(|| ImageCache::new(waker));
                *entry = match fetched {
                    None => Entry::Missing,
                    Some(Fetched::Svg(bytes)) => Entry::Ready(cache.svg(bytes, SVG_PT, 2.0)),
                    Some(Fetched::Rgba(w, h, bytes)) => Entry::Ready(cache.rgba(w, h, bytes)),
                    Some(Fetched::Encoded(bytes)) => Entry::Ready(cache.load(bytes)),
                };
                changed = true;
            }
            Entry::Ready(_) | Entry::Missing => {}
        }
    }
    changed
}

/// The project's icon at `size`, rounded like the web's `rounded-[25%]`, or its monogram.
pub fn mark(s: &App, t: &T, project_id: &str, name: &str, dim: bool, size: f32) -> El {
    let image = match s.ui.shell.sidebar.favicons.entries.get(project_id) {
        Some((_, Entry::Ready(image))) if image.is_ready() && image.error().is_none() => image,
        _ => return super::row::monogram_at(t, name, dim, size),
    };
    el().size(size, size)
        .shrink()
        .radius(size / 4.0)
        .clip_rounded(size / 4.0)
        .opacity(if dim { 0.4 } else { 1.0 })
        .pixels_fit(image.pixels(), rune::SurfaceFit::Fit)
}

#[cfg(test)]
mod tests {
    use super::{is_ico, is_svg};

    #[test]
    fn svg_and_ico_bytes_are_told_apart_from_other_rasters() {
        assert!(is_svg(b"\n  <svg xmlns=\"http://www.w3.org/2000/svg\"/>"));
        assert!(is_svg(b"<?xml version=\"1.0\"?><svg/>"));
        assert!(is_svg(&[0x1f, 0x8b, 8, 0]), "gzipped SVGZ");
        assert!(!is_svg(b"\x89PNG\r\n\x1a\n"));
        assert!(is_ico(&[0, 0, 1, 0, 1, 0]));
        assert!(!is_ico(&[0, 0, 2, 0]), "a cursor file is not an icon");
    }
}
