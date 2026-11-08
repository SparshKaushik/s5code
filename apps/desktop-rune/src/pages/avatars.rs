//! Author avatars (PullRequestActorAvatar): each URL downloads once, off the UI thread, and
//! stays cached for the session. Until it lands the circle shows its muted fill, like a lazy
//! `<img>`; a URL that fails shows the login's initial instead.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rune::{El, Pixels, Waker, el, text};
use serde_json::Value;

use crate::theme::{self, T, radius};

/// Downloads in flight at once; the rest wait for a later poll.
const MAX_IN_FLIGHT: usize = 8;

struct Entry {
    pixels: Pixels,
    done: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct Avatars {
    cache: HashMap<String, Entry>,
}

/// A smaller rendition where the host serves one: GitHub's `s` parameter.
fn sized(url: &str) -> String {
    if !url.contains("avatars.githubusercontent.com") || url.contains("s=") {
        return url.to_string();
    }
    format!("{url}{}s=64", if url.contains('?') { '&' } else { '?' })
}

impl Avatars {
    /// Start downloading every https avatar not yet cached, a few at a time. `waker` repaints
    /// when one settles, so a failed download swaps to its initial.
    pub fn want<'a>(&mut self, urls: impl IntoIterator<Item = &'a str>, waker: Option<&Waker>) {
        let mut in_flight = self.cache.values().filter(|e| !e.done.load(Ordering::Relaxed)).count();
        for url in urls {
            if in_flight >= MAX_IN_FLIGHT {
                return;
            }
            if !url.starts_with("https://") || self.cache.contains_key(url) {
                continue;
            }
            let entry = Entry { pixels: Pixels::new(), done: Arc::new(AtomicBool::new(false)), failed: Arc::new(AtomicBool::new(false)) };
            let (pixels, done, failed, from, waker) = (entry.pixels.clone(), entry.done.clone(), entry.failed.clone(), sized(url), waker.cloned());
            std::thread::spawn(move || {
                let ok = (|| {
                    let response = ureq::get(&from).timeout(std::time::Duration::from_secs(20)).call().ok()?;
                    let mut bytes = Vec::new();
                    use std::io::Read;
                    response.into_reader().take(2 * 1024 * 1024).read_to_end(&mut bytes).ok()?;
                    let image = image::load_from_memory(&bytes).ok()?.thumbnail(64, 64).to_rgba8();
                    pixels.set(image.width(), image.height(), image.into_raw());
                    Some(())
                })()
                .is_some();
                failed.store(!ok, Ordering::Relaxed);
                done.store(true, Ordering::Relaxed);
                if let Some(waker) = waker {
                    waker();
                }
            });
            self.cache.insert(url.to_string(), entry);
            in_flight += 1;
        }
    }

    fn ready(&self, url: &str) -> Option<&Pixels> {
        let e = self.cache.get(url)?;
        (!e.failed.load(Ordering::Relaxed)).then_some(&e.pixels)
    }

    fn failed(&self, url: &str) -> bool {
        self.cache.get(url).is_some_and(|e| e.failed.load(Ordering::Relaxed))
    }
}

/// Every avatar URL an actor list names, for [`Avatars::want`].
pub fn urls<'a>(actors: impl IntoIterator<Item = &'a Value>) -> impl Iterator<Item = &'a str> {
    actors.into_iter().filter_map(|a| a["avatarUrl"].as_str())
}

/// The `size` circle for `actor` (`null` is the ghost user).
pub fn avatar(avatars: &Avatars, actor: &Value, size: f32, t: &T) -> El {
    let login = actor["login"].as_str().unwrap_or("ghost");
    let url = actor["avatarUrl"].as_str().filter(|u| u.starts_with("https://"));
    let circle = el().size(size, size).shrink().radius(radius::FULL).clip_rounded(size / 2.0).fill(t.c.muted);
    match url {
        Some(url) if !avatars.failed(url) => match avatars.ready(url) {
            Some(pixels) => circle.pixels_fit(pixels, rune::SurfaceFit::Fit),
            None => circle,
        },
        _ => {
            let initial = login.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
            let font = if size >= 20.0 { theme::TEXT_XS } else { theme::TEXT_3XS };
            circle.center().child(text(initial).font(theme::weight(font, theme::MEDIUM).lh(size)).color(t.c.muted_foreground))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sized;

    #[test]
    fn github_avatars_ask_for_a_small_rendition() {
        assert_eq!(sized("https://avatars.githubusercontent.com/u/1?v=4"), "https://avatars.githubusercontent.com/u/1?v=4&s=64");
        assert_eq!(sized("https://avatars.githubusercontent.com/u/1"), "https://avatars.githubusercontent.com/u/1?s=64");
        assert_eq!(sized("https://avatars.githubusercontent.com/u/1?s=40"), "https://avatars.githubusercontent.com/u/1?s=40");
        assert_eq!(sized("https://gitlab.example/a.png"), "https://gitlab.example/a.png");
    }
}
