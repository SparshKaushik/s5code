//! Find in thread (ThreadFindBar, threadFindHighlights): Ctrl+F opens a bar over the top right
//! of the chat; matches in messages are painted, the active one brighter, and Enter, Shift+Enter
//! or the arrows move between them. Escape closes it.

use super::rows::{self, Row};
use crate::data::s;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route, icons};
use rune::{Color, Cursor, El, Key, KeyEvent, TextEdit, Tx, el, icon, row, text, text_input};
use rune_kit::markdown::{Case, Highlight, find as find_markdown};
use std::cell::Cell;
use std::ops::Range;

#[derive(Default)]
pub struct Find {
    pub open: bool,
    pub edit: TextEdit,
    pub active: usize,
}

/// Case-insensitive matches of `query` in plain `hay`, as UTF-8 byte ranges.
pub fn find_plain(hay: &str, query: &str) -> Vec<Range<usize>> {
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let chars: Vec<(usize, char)> = hay.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    'outer: while i < chars.len() {
        let mut j = 0;
        let mut k = i;
        while j < needle.len() {
            let Some(&(_, c)) = chars.get(k) else { break 'outer };
            for lower in c.to_lowercase() {
                if needle.get(j) != Some(&lower) {
                    i += 1;
                    continue 'outer;
                }
                j += 1;
            }
            k += 1;
        }
        let end = chars.get(k).map_or(hay.len(), |&(b, _)| b);
        out.push(chars[i].0..end);
        i = k.max(i + 1);
    }
    out
}

fn query(app: &App) -> Option<&str> {
    let f = &app.ui.chat.timeline.find;
    let q = f.edit.text.trim();
    (f.open && !q.is_empty()).then_some(q)
}

/// How many matches a row holds: user text plainly, replies in their rendered Markdown.
fn row_matches(row: &Row, q: &str) -> usize {
    match row {
        Row::User(item) => find_plain(s(item, "text"), q).len(),
        Row::Assistant(item) => find_markdown(s(item, "text"), q, Case::Insensitive).len(),
        _ => 0,
    }
}

/// The row key of every match in the open thread, in order.
pub fn targets(app: &App) -> Vec<String> {
    let (Some(q), Route::Thread(id)) = (query(app), app.frame_route()) else { return Vec::new() };
    let Some(view) = app.state().thread_views.get(id) else { return Vec::new() };
    let mut out = Vec::new();
    for (index, row) in rows::derive(&view.projection, app.now_ms).iter().enumerate() {
        let n = row_matches(row, q);
        out.extend(std::iter::repeat_n(super::row_key(row, index), n));
    }
    out
}

/// The matches of one row as it renders: indices continue from `base`, the active one keyed so
/// the bar can scroll to it. A message rendered in pieces asks once per piece, in order.
pub struct Marks<'a> {
    query: &'a str,
    active: usize,
    next: Cell<usize>,
    dark: bool,
}

impl<'a> Marks<'a> {
    /// Per-row marks for the open bar, with each row's starting index.
    pub fn for_rows(app: &'a App, rows: &[Row], dark: bool) -> Vec<Option<Marks<'a>>> {
        let Some(q) = query(app) else { return rows.iter().map(|_| None).collect() };
        let mut base = 0;
        rows.iter()
            .map(|row| {
                let n = row_matches(row, q);
                let marks = (n > 0).then(|| Marks { query: q, active: app.ui.chat.timeline.find.active, next: Cell::new(base), dark });
                base += n;
                marks
            })
            .collect()
    }

    fn take(&self) -> usize {
        let i = self.next.get();
        self.next.set(i + 1);
        i
    }

    fn colors(&self, active: bool) -> (Color, Color) {
        // index.css ::highlight(t3-thread-find) and ::highlight(t3-thread-find-active).
        let ink = rune::oklch(0.216164, 0.028283, 77.423);
        if active {
            (rune::oklch(0.774202, 0.151946, 74.331), ink)
        } else if self.dark {
            (rune::oklch(0.55, 0.1, 85.0), rune::oklch(0.970151, 0.0, 0.0))
        } else {
            (rune::oklch(0.915, 0.11, 85.0), ink)
        }
    }

    /// Highlights for one Markdown source.
    pub fn markdown(&self, src: &str) -> Vec<(Vec<Range<usize>>, Highlight)> {
        find_markdown(src, self.query, Case::Insensitive)
            .into_iter()
            .map(|ranges| {
                let i = self.take();
                let (bg, fg) = self.colors(i == self.active);
                (ranges, Highlight::new().bg(bg).fg(fg).key(format!("find-{i}")))
            })
            .collect()
    }

    /// Highlights for plain text.
    pub fn plain(&self, hay: &str) -> Vec<rune::text::TextHighlight> {
        find_plain(hay, self.query)
            .into_iter()
            .map(|range| {
                let i = self.take();
                let (bg, fg) = self.colors(i == self.active);
                rune::text::TextHighlight::new(range, bg).color(fg).key(format!("find-{i}"))
            })
            .collect()
    }
}

/// How far the details card moves down to clear the open bar.
pub fn offset(app: &App) -> f32 {
    let open = app.ui.chat.timeline.find.open && matches!(app.frame_route(), Route::Thread(_));
    if open { 36.0 + 12.0 } else { 0.0 }
}

fn reveal(app: &App, tx: &mut Tx) {
    let targets = targets(app);
    let active = app.ui.chat.timeline.find.active;
    if let Some(row) = targets.get(active) {
        tx.scroll_center(format!("{row}/find-{active}"));
    }
}

fn step(app: &mut App, by: isize, tx: &mut Tx) {
    let n = targets(app).len();
    if n == 0 {
        return;
    }
    let f = &mut app.ui.chat.timeline.find;
    f.active = (f.active.min(n - 1) as isize + by).rem_euclid(n as isize) as usize;
    reveal(app, tx);
}

pub fn open(app: &mut App) {
    let f = &mut app.ui.chat.timeline.find;
    f.open = true;
    f.edit.focused = true;
    let end = f.edit.text.len();
    f.edit.set_selection(0, end);
    crate::chat::composer::set_focus(app, false);
}

fn close(app: &mut App) {
    let f = &mut app.ui.chat.timeline.find;
    f.open = false;
    f.edit.focused = false;
    crate::chat::composer::set_focus(app, true);
}

/// Ctrl+F on a thread opens (or refocuses) the bar.
pub fn on_key(app: &mut App, e: &KeyEvent, _tx: &mut Tx) -> bool {
    if e.is_command('f') && !e.mods.shift && matches!(app.frame_route(), Route::Thread(_)) {
        open(app);
        return true;
    }
    if e.key == Key::Escape && app.ui.chat.timeline.find.open {
        close(app);
        return true;
    }
    false
}

fn on_input_key(app: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    match e.key {
        Key::Enter => {
            step(app, if e.mods.shift { -1 } else { 1 }, tx);
            true
        }
        Key::Escape => {
            close(app);
            true
        }
        _ => false,
    }
}

fn nav(t: &T, key: &str, glyph: &'static rune::Icon, label: &str, enabled: bool) -> El {
    let b = el()
        .key(key)
        .size(24.0, 24.0)
        .center()
        .radius(radius::MD)
        .label(label.to_owned())
        .help(label.to_owned())
        .child(icon(glyph, 16.0).color(t.c.muted_foreground));
    if enabled { b.hover(t.c.accent).cursor(Cursor::Pointer) } else { b.opacity(0.64) }
}

/// The bar, pinned over the chat's top right (`absolute top-3 right-3`, details-card wide).
pub fn bar(app: &App, t: &T, x: f32, y: f32, w: f32) -> Option<El> {
    let f = &app.ui.chat.timeline.find;
    if !f.open || !matches!(app.frame_route(), Route::Thread(_)) {
        return None;
    }
    let n = targets(app).len();
    let has_query = !f.edit.text.trim().is_empty();
    let label = if has_query { if n == 0 { "0/0".to_owned() } else { format!("{}/{n}", f.active.min(n - 1) + 1) } } else { String::new() };
    let input = text_input(&f.edit, theme::TEXT_SM)
        .key("thread-find-input")
        .role("searchbox")
        .label("Find in thread")
        .grow(1.0)
        .min_w(0.0)
        .color(t.c.foreground)
        .placeholder("Find in thread", t.c.placeholder)
        .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
        .on_edit(|app: &mut App| &mut app.ui.chat.timeline.find.edit)
        .on_input_key(on_input_key)
        .on_input_change(|app: &mut App, tx| {
            app.ui.chat.timeline.find.active = 0;
            reveal(app, tx);
        });
    let enabled = n > 0;
    let mut prev = nav(t, "thread-find-prev", &icons::CHEVRON_UP, "Previous match", enabled);
    let mut next = nav(t, "thread-find-next", &icons::CHEVRON_DOWN, "Next match", enabled);
    if enabled {
        prev = prev.on_press_tx(|app: &mut App, tx| step(app, -1, tx));
        next = next.on_press_tx(|app: &mut App, tx| step(app, 1, tx));
    }
    let close_button = nav(t, "thread-find-close", &icons::X, "Close find", true).on_press(close);
    Some(
        row()
            .key("thread-find")
            .role("search")
            .absolute(x, y)
            .w(w)
            .h(36.0)
            .pl(12.0)
            .pr(6.0)
            .gap(2.0)
            .center_y()
            .radius(radius::LG)
            .fill(t.c.popover)
            .border(1.0, if t.dark { Color::WHITE.alpha(0.1) } else { Color::BLACK.alpha(0.08) })
            .shadow(4.0, 16.0, Color::BLACK.alpha(if t.dark { 0.32 } else { 0.08 }))
            .child(input)
            .child(text(label).font(theme::TEXT_XS).color(if has_query && n == 0 { t.c.destructive } else { t.c.muted_foreground }).min_w(40.0).px(2.0))
            .child(prev)
            .child(next)
            .child(close_button),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_matches_fold_case_and_keep_byte_ranges() {
        assert_eq!(find_plain("Fix the BUG, then the bug", "bug"), vec![8..11, 22..25]);
        assert_eq!(find_plain("ÄÖ äö", "äö"), vec![0..4, 5..9]);
        assert_eq!(find_plain("aaa", "aa"), vec![0..2]);
        assert!(find_plain("abc", "").is_empty());
        assert!(find_plain("ab", "abc").is_empty());
    }
}
