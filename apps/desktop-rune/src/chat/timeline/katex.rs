//! MathTypeset: KaTeX (web's version and options, `assets/katex`) typesets each formula in the
//! Default profile's Chrome, as one cropped, transparent picture per formula, size and ink, kept
//! for the session. A formula reads as its TeX source until its picture arrives, and for good
//! when Chrome is missing or KaTeX can't parse it.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

use rune::tok::TextStyle;
use rune::{Color, El, el, row, text};

use super::math::Inline;
use crate::App;
use crate::panel::browser::{Cmd, Shot};
use crate::theme;

const VERSION: &str = "0.16.47";
const KATEX_JS: &str = include_str!("../../../assets/katex/katex.min.js.txt");
const KATEX_CSS: &str = include_str!("../../../assets/katex/katex.min.css.txt");
const FONTS: [(&str, &[u8]); 20] = [
    ("KaTeX_AMS-Regular", include_bytes!("../../../assets/katex/fonts/KaTeX_AMS-Regular.woff2")),
    ("KaTeX_Caligraphic-Bold", include_bytes!("../../../assets/katex/fonts/KaTeX_Caligraphic-Bold.woff2")),
    ("KaTeX_Caligraphic-Regular", include_bytes!("../../../assets/katex/fonts/KaTeX_Caligraphic-Regular.woff2")),
    ("KaTeX_Fraktur-Bold", include_bytes!("../../../assets/katex/fonts/KaTeX_Fraktur-Bold.woff2")),
    ("KaTeX_Fraktur-Regular", include_bytes!("../../../assets/katex/fonts/KaTeX_Fraktur-Regular.woff2")),
    ("KaTeX_Main-Bold", include_bytes!("../../../assets/katex/fonts/KaTeX_Main-Bold.woff2")),
    ("KaTeX_Main-BoldItalic", include_bytes!("../../../assets/katex/fonts/KaTeX_Main-BoldItalic.woff2")),
    ("KaTeX_Main-Italic", include_bytes!("../../../assets/katex/fonts/KaTeX_Main-Italic.woff2")),
    ("KaTeX_Main-Regular", include_bytes!("../../../assets/katex/fonts/KaTeX_Main-Regular.woff2")),
    ("KaTeX_Math-BoldItalic", include_bytes!("../../../assets/katex/fonts/KaTeX_Math-BoldItalic.woff2")),
    ("KaTeX_Math-Italic", include_bytes!("../../../assets/katex/fonts/KaTeX_Math-Italic.woff2")),
    ("KaTeX_SansSerif-Bold", include_bytes!("../../../assets/katex/fonts/KaTeX_SansSerif-Bold.woff2")),
    ("KaTeX_SansSerif-Italic", include_bytes!("../../../assets/katex/fonts/KaTeX_SansSerif-Italic.woff2")),
    ("KaTeX_SansSerif-Regular", include_bytes!("../../../assets/katex/fonts/KaTeX_SansSerif-Regular.woff2")),
    ("KaTeX_Script-Regular", include_bytes!("../../../assets/katex/fonts/KaTeX_Script-Regular.woff2")),
    ("KaTeX_Size1-Regular", include_bytes!("../../../assets/katex/fonts/KaTeX_Size1-Regular.woff2")),
    ("KaTeX_Size2-Regular", include_bytes!("../../../assets/katex/fonts/KaTeX_Size2-Regular.woff2")),
    ("KaTeX_Size3-Regular", include_bytes!("../../../assets/katex/fonts/KaTeX_Size3-Regular.woff2")),
    ("KaTeX_Size4-Regular", include_bytes!("../../../assets/katex/fonts/KaTeX_Size4-Regular.woff2")),
    ("KaTeX_Typewriter-Regular", include_bytes!("../../../assets/katex/fonts/KaTeX_Typewriter-Regular.woff2")),
];
/// The width formulas are laid out in, in CSS pixels; the picture is cropped to the formula.
const WIDTH: u32 = 1600;
/// Formulas typesetting at once, each in a Chrome context of its own.
const AT_ONCE: usize = 4;
const KEEP: usize = 500;

const PAGE: &str = r#"<!doctype html><meta charset="utf-8"><style>{css}
html,body{margin:0;background:transparent}#m{display:inline-block;padding:2px;white-space:nowrap}.katex-display{margin:0}</style>
<span id="m"></span><script>{js}</script><script>
const p = JSON.parse(decodeURIComponent(location.hash.slice(1)));
const m = document.getElementById("m");
m.style.fontSize = p.size + "px";
m.style.color = p.color;
try {
  katex.render(p.tex, m, { displayMode: p.display, output: "html", throwOnError: true, trust: false, strict: "ignore", maxSize: 20, maxExpand: 1000 });
} catch (e) {
  m.textContent = "";
}
</script>"#;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    tex: String,
    display: bool,
    /// The text size in tenths of a pixel.
    size: u32,
    ink: String,
}

enum Entry {
    Queued,
    Pending(Shot),
    /// The picture and its CSS size.
    Ready(Shot, f32, f32),
    Failed,
}

#[derive(Default)]
pub struct State {
    entries: HashMap<Key, Entry>,
    /// Oldest first, for `KEEP`.
    order: VecDeque<Key>,
    /// Formulas a view drew without an entry; `poll` queues them.
    wanted: RefCell<Vec<Key>>,
    /// Bumped whenever a picture arrives or fails, so Markdown that holds math builds again.
    pub generation: u64,
    /// The typesetting page's `file:` URL once written; `Err` when it could not be.
    page: Option<Result<String, ()>>,
    /// Scenes' pictures by TeX and display, in any ink and size.
    stills: HashMap<(String, bool), (Shot, f32, f32)>,
}

/// The picture of `tex` in `ink` at text size `size`, or `None` until it has been typeset
/// (asking for it) or when it can't be.
pub(super) fn picture(app: &App, tex: &str, display: bool, size: f32, ink: Color) -> Option<El> {
    let state = &app.ui.chat.timeline.katex;
    if let Some((shot, w, h)) = state.stills.get(&(tex.to_owned(), display)) {
        return Some(el().w(*w).h(*h).pixels(&shot.pixels).label(tex.to_owned()));
    }
    let key = Key { tex: tex.to_owned(), display, size: (size * 10.).round() as u32, ink: ink.hex() };
    match state.entries.get(&key) {
        Some(Entry::Ready(shot, w, h)) => Some(el().w(*w).h(*h).pixels(&shot.pixels).label(tex.to_owned())),
        Some(_) => None,
        None => {
            if let Ok(mut wanted) = state.wanted.try_borrow_mut()
                && !wanted.contains(&key)
            {
                wanted.push(key);
                if let Some(wake) = &app.ui.panel.waker {
                    wake();
                }
            }
            None
        }
    }
}

/// An inline-math paragraph laid out a word at a time, its formulas as pictures, once all of
/// them are typeset: Rune's text can't hold a picture. A link is one unbroken piece. `None`
/// keeps it TeX source, as do images, HTML and a formula inside a link.
pub(super) fn flow(app: &App, parts: &[Inline], font: TextStyle, ink: Color, link_ink: Color) -> Option<El> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let mut source = String::new();
    let mut formulas = Vec::new();
    for part in parts {
        match part {
            Inline::Text(text) => source.push_str(text),
            Inline::Math { tex, display } => {
                source.push_str(&format!("\u{e000}{}\u{e001}", formulas.len()));
                formulas.push(picture(app, tex, *display, font.size, ink));
            }
        }
    }
    if formulas.iter().any(Option::is_none) || source.lines().any(|l| block_syntax(l.trim_start())) {
        return None;
    }
    let mut words: Vec<El> = Vec::new();
    let mut word = row().center_y();
    let mut empty = true;
    let (mut strong, mut emphasis) = (0, 0);
    // The open link's URL and the text read so far.
    let mut link: Option<(String, String)> = None;
    let finish = |word: &mut El, empty: &mut bool, words: &mut Vec<El>| {
        if !*empty {
            words.push(std::mem::replace(word, row().center_y()));
            *empty = true;
        }
    };
    for event in Parser::new(&source) {
        let (piece, mono) = match event {
            Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => continue,
            Event::Start(Tag::Strong) => {
                strong += 1;
                continue;
            }
            Event::End(TagEnd::Strong) => {
                strong -= 1;
                continue;
            }
            Event::Start(Tag::Emphasis) => {
                emphasis += 1;
                continue;
            }
            Event::End(TagEnd::Emphasis) => {
                emphasis -= 1;
                continue;
            }
            Event::Start(Tag::Link { dest_url, .. }) if link.is_none() => {
                link = Some((dest_url.into_string(), String::new()));
                continue;
            }
            Event::End(TagEnd::Link) => {
                let (url, label) = link.take()?;
                let style = if strong > 0 { theme::weight(font, 600.) } else { font };
                let help = super::md_links::help(&url);
                let mut target = rune::Link::new(0..label.len(), url);
                if let Some(help) = help {
                    target = target.help(help);
                }
                word = word.child(text(label).font(style).color(link_ink).single_line().link(target).on_link(super::md_links::open));
                empty = false;
                continue;
            }
            Event::Text(t) | Event::Code(t) if link.is_some() => {
                let (_, label) = link.as_mut()?;
                if t.contains('\u{e000}') {
                    return None;
                }
                label.push_str(&t);
                continue;
            }
            Event::SoftBreak | Event::HardBreak if link.is_some() => {
                link.as_mut()?.1.push(' ');
                continue;
            }
            Event::SoftBreak | Event::HardBreak => {
                finish(&mut word, &mut empty, &mut words);
                continue;
            }
            Event::Text(t) => (t.into_string(), false),
            Event::Code(t) => (t.into_string(), true),
            _ => return None,
        };
        let style = match (mono, strong > 0) {
            (true, _) => TextStyle { size: font.size * 0.85, ..theme::mono(font) },
            (false, true) => theme::weight(font, 600.),
            (false, false) => font,
        };
        let style = if emphasis > 0 && !mono { style.italic() } else { style };
        for (n, chunk) in piece.split(' ').enumerate() {
            if n > 0 {
                finish(&mut word, &mut empty, &mut words);
            }
            let mut rest = chunk;
            while !rest.is_empty() {
                let (before, formula, after) = match rest.find('\u{e000}') {
                    Some(at) => {
                        let close = rest[at..].find('\u{e001}').map_or(rest.len(), |c| at + c);
                        let index: usize = rest[at + '\u{e000}'.len_utf8()..close].parse().ok()?;
                        (&rest[..at], Some(index), rest.get(close + '\u{e001}'.len_utf8()..).unwrap_or_default())
                    }
                    None => (rest, None, ""),
                };
                if !before.is_empty() {
                    word = word.child(text(before.to_owned()).font(style).color(ink).single_line());
                    empty = false;
                }
                if let Some(index) = formula {
                    word = word.child(formulas.get_mut(index)?.take()?);
                    empty = false;
                }
                rest = after;
            }
        }
    }
    finish(&mut word, &mut empty, &mut words);
    Some(row().w_full().flex_wrap().gap(4.0).center_y().min_h(font.line_px()).children(words))
}

/// Lines a word-at-a-time layout would draw wrong: headings, quotes, lists, tables and fences.
fn block_syntax(line: &str) -> bool {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    line.starts_with(['#', '>', '|', '<', '-', '*', '+', '`', '~', '='])
        || (digits > 0 && (line[digits..].starts_with(". ") || line[digits..].starts_with(") ")))
}

/// What a Markdown view holding math reads of the typesetting state.
pub(super) fn signature(app: &App, source: &str) -> u64 {
    if super::math::may_contain(source) || source.contains("```math") { app.ui.chat.timeline.katex.generation } else { 0 }
}

/// Queues the formulas views asked for, starts up to `AT_ONCE`, and takes finished pictures.
pub fn poll(app: &mut App) -> bool {
    let wanted = std::mem::take(app.ui.chat.timeline.katex.wanted.get_mut());
    let state = &mut app.ui.chat.timeline.katex;
    let mut changed = false;
    for key in wanted {
        if !state.entries.contains_key(&key) {
            state.order.push_back(key.clone());
            state.entries.insert(key, Entry::Queued);
        }
    }
    for entry in state.entries.values_mut() {
        let next = match entry {
            Entry::Pending(shot) => match shot.result() {
                Some(Some(height)) => {
                    let (w, h) = shot.pixels.size();
                    let width = if h == 0 { 0. } else { w as f32 * height / h as f32 };
                    Some(Entry::Ready(shot.clone(), width, height))
                }
                Some(None) => Some(Entry::Failed),
                None => None,
            },
            _ => None,
        };
        if let Some(next) = next {
            *entry = next;
            changed = true;
        }
    }
    let running = state.entries.values().filter(|e| matches!(e, Entry::Pending(_))).count();
    let queued: Vec<Key> = state.order.iter().filter(|k| matches!(state.entries.get(*k), Some(Entry::Queued))).take(AT_ONCE.saturating_sub(running)).cloned().collect();
    for key in queued {
        let entry = match page(app) {
            Some(page) => {
                let fragment = serde_json::json!({ "tex": key.tex, "display": key.display, "size": key.size as f32 / 10., "color": key.ink }).to_string();
                let mut shot = Shot::new(format!("{page}#{}", encode_component(&fragment)), WIDTH, false);
                shot.crop = true;
                if crate::panel::preview::inline_frame_cmd(app, Cmd::Snapshot(shot.clone())) { Entry::Pending(shot) } else { Entry::Failed }
            }
            None => Entry::Failed,
        };
        changed |= matches!(entry, Entry::Failed);
        app.ui.chat.timeline.katex.entries.insert(key, entry);
    }
    let state = &mut app.ui.chat.timeline.katex;
    while state.order.len() > KEEP {
        let Some(key) = state.order.pop_front() else { break };
        if matches!(state.entries.get(&key), Some(Entry::Pending(_))) {
            state.order.push_back(key);
            break;
        }
        state.entries.remove(&key);
    }
    if changed {
        state.generation += 1;
    }
    changed
}

/// The typesetting page, written once per KaTeX version: its fonts inline, so a `file:` page
/// needs nothing else.
fn page(app: &mut App) -> Option<String> {
    let state = &mut app.ui.chat.timeline.katex;
    if state.page.is_none() {
        let path = std::env::temp_dir().join(format!("s5-rune-katex-{VERSION}.html"));
        let write = || -> std::io::Result<()> {
            if path.exists() {
                return Ok(());
            }
            use base64::Engine;
            let mut css = KATEX_CSS.to_owned();
            for (name, bytes) in FONTS {
                let data = format!("url(data:font/woff2;base64,{})", base64::engine::general_purpose::STANDARD.encode(bytes));
                css = css.replace(&format!("url(fonts/{name}.woff2)"), &data);
            }
            let staged = path.with_extension(format!("{}.tmp", std::process::id()));
            std::fs::write(&staged, PAGE.replacen("{css}", &css, 1).replacen("{js}", KATEX_JS, 1))?;
            std::fs::rename(&staged, &path)
        };
        state.page = Some(write().ok().and_then(|()| s5_net::url::Url::from_file_path(&path).ok()).map(|url| url.to_string()).ok_or(()));
    }
    state.page.clone()?.ok()
}

/// encodeURIComponent, which the page's `decodeURIComponent` reverses.
fn encode_component(value: &str) -> String {
    value.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Scenes with Chrome (`S5_CHROME`): typesets each formula for real, waiting here since a
/// headless run's clock does not wait on Chrome. The pictures then stand for their formula in
/// any ink and size; without Chrome the formulas stay TeX source and the reason goes to stderr.
pub fn scene_typeset(app: &mut App, formulas: &[(&str, bool)]) {
    if app.ui.panel.waker.is_none() {
        app.ui.panel.waker = Some(std::sync::Arc::new(|| {}));
    }
    let ink = theme::theme(app.appearance != crate::Appearance::Light).c.foreground.hex();
    let size = (theme::TEXT_SM.size * 10.).round() as u32;
    let keys: Vec<Key> = formulas.iter().map(|(tex, display)| Key { tex: (*tex).to_owned(), display: *display, size, ink: ink.clone() }).collect();
    app.ui.chat.timeline.katex.wanted.get_mut().extend(keys.iter().cloned());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let waiting = |state: &State| keys.iter().any(|k| matches!(state.entries.get(k), None | Some(Entry::Queued | Entry::Pending(_))));
    poll(app);
    while waiting(&app.ui.chat.timeline.katex) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
        poll(app);
    }
    crate::panel::browser::shutdown();
    let state = &mut app.ui.chat.timeline.katex;
    for key in &keys {
        if let Some(Entry::Ready(shot, w, h)) = state.entries.get(key) {
            state.stills.insert((key.tex.clone(), key.display), (shot.clone(), *w, *h));
        }
    }
    eprintln!("scene: KaTeX typeset {} of {} formulas", state.stills.len(), keys.len());
    state.generation += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_chrome_a_formula_stays_tex_source_and_is_asked_for_once() {
        let mut app = App::new();
        let ink = Color { r: 1., g: 1., b: 1., a: 1. };
        assert!(picture(&app, "x^2", false, 14., ink).is_none());
        assert!(poll(&mut app), "the failure rebuilds the Markdown holding it");
        assert!(picture(&app, "x^2", false, 14., ink).is_none());
        assert!(app.ui.chat.timeline.katex.wanted.borrow().is_empty(), "a failed formula isn't asked for again");
        assert!(!poll(&mut app));
    }

    #[test]
    fn a_paragraph_with_a_link_flows_but_a_formula_inside_one_stays_source() {
        let mut app = App::new();
        app.ui.chat.timeline.katex.stills.insert(("x".into(), false), (Shot::new(String::new(), 0, true), 8., 14.));
        let ink = Color { r: 1., g: 1., b: 1., a: 1. };
        let math = || Inline::Math { tex: "x".into(), display: false };
        let around = |before: &str, after: &str| vec![Inline::Text(before.into()), math(), Inline::Text(after.into())];
        let flows = |parts: &[Inline]| flow(&app, parts, theme::TEXT_SM, ink, ink).is_some();
        assert!(flows(&around("See [the **docs**](https://example.com) for ", ".")));
        assert!(!flows(&[Inline::Text("[see ".into()), math(), Inline::Text("](https://example.com)".into())]));
        assert!(!flows(&around("![chart](a.png) and ", "")));
    }
}
