//! Find in the diff (pierre's CodeViewSearch): every file, folded ones too, and the loaded
//! context; going to a match unfolds its file and its gap.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;

use regex_automata::meta::Regex;
use regex_automata::util::syntax;
use rune::{Color, Cursor, El, Icon, Key, KeyEvent, Span, Tx, icon, rich, row, text, text_input};
use rune_kit::diff::{LineKind, Side};

use super::state::{DiffState, GAP};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};

const MAX_HITS: usize = 100_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub f: usize,
    /// The hunk, or `GAP | gap` for a gap's line.
    pub h: u32,
    pub i: u32,
    pub side: Side,
    pub line: u32,
    pub range: Range<usize>,
}

#[derive(Default)]
pub struct Find {
    case: bool,
    word: bool,
    regex: bool,
    hits: Vec<Hit>,
    current: Option<usize>,
    searched: u64,
}

/// pierre's word separators.
fn separator(c: char) -> bool {
    (c as u32) <= 32 || c as u32 == 127 || "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?".contains(c)
}

pub fn pattern(query: &str, case: bool, regex: bool) -> Option<Regex> {
    if query.is_empty() {
        return None;
    }
    let pat = if regex {
        query.to_owned()
    } else {
        query.chars().fold(String::new(), |mut out, c| {
            if "\\.+*?()|[]{}^$#&-~".contains(c) {
                out.push('\\');
            }
            out.push(c);
            out
        })
    };
    Regex::builder().syntax(syntax::Config::new().case_insensitive(!case)).build(&pat).ok()
}

fn matches(re: &Regex, text: &str, word: bool) -> impl Iterator<Item = Range<usize>> {
    re.find_iter(text).filter(|m| !m.is_empty()).map(|m| m.range()).filter(move |r| {
        !word || (text[..r.start].chars().next_back().is_none_or(separator) && text[r.end..].chars().next().is_none_or(separator))
    })
}

/// Every match in the files, in file order: each hunk's gap, then its lines, then the tail.
pub fn collect(st: &DiffState, re: &Regex, word: bool) -> Vec<Hit> {
    let mut hits = vec![];
    for (f, file) in st.files().iter().enumerate() {
        let gap = |hits: &mut Vec<Hit>, h: usize| {
            for k in 0..st.gap_len(f, h) {
                let Some((_, new, text)) = st.gap_line(f, h, k) else { continue };
                hits.extend(matches(re, text, word).map(|range| Hit { f, h: GAP | h as u32, i: k, side: Side::New, line: new, range }));
            }
        };
        for (h, hunk) in file.hunks.iter().enumerate() {
            if st.loaded(f) {
                gap(&mut hits, h);
            }
            for (i, l) in hunk.lines.iter().enumerate() {
                let (side, line) = match l.kind {
                    LineKind::Removed => (Side::Old, l.old),
                    _ => (Side::New, l.new),
                };
                let Some(line) = line else { continue };
                hits.extend(matches(re, &l.text, word).map(|range| Hit { f, h: h as u32, i: i as u32, side, line, range: range.clone() }));
            }
            if hits.len() >= MAX_HITS {
                hits.truncate(MAX_HITS);
                return hits;
            }
        }
        if st.loaded(f) {
            gap(&mut hits, file.hunks.len());
        }
    }
    hits.truncate(MAX_HITS);
    hits
}

fn signature(app: &App) -> u64 {
    let ui = &app.ui.panel.diff;
    let mut h = std::hash::DefaultHasher::new();
    ui.find_edit.text.hash(&mut h);
    if let Some(f) = &ui.find {
        (f.case, f.word, f.regex).hash(&mut h);
    }
    ui.state.files_rev.hash(&mut h);
    h.finish()
}

fn paint(app: &mut App) {
    let ui = &mut app.ui.panel.diff;
    let Some(find) = &ui.find else { return };
    let mut map: HashMap<(u32, u32, u32), Vec<(Range<usize>, bool)>> = HashMap::new();
    for (j, hit) in find.hits.iter().enumerate() {
        map.entry((hit.f as u32, hit.h, hit.i)).or_default().push((hit.range.clone(), find.current == Some(j)));
    }
    ui.state.set_hits(map);
}

/// Searches again when the query, its options or the files changed; keeps the current match
/// when it is still there. Returns whether anything changed.
pub fn refresh(app: &mut App) -> bool {
    if app.ui.panel.diff.find.is_none() {
        return false;
    }
    let sig = signature(app);
    let ui = &mut app.ui.panel.diff;
    let Some(find) = ui.find.as_mut() else { return false };
    if find.searched == sig {
        return false;
    }
    find.searched = sig;
    let before = find.current.and_then(|j| find.hits.get(j).cloned());
    find.hits = pattern(&ui.find_edit.text, find.case, find.regex).map(|re| collect(&ui.state, &re, find.word)).unwrap_or_default();
    let st = &ui.state;
    let first = first_visible(&find.hits, &|h: &Hit| st.row_at(h.f, h.side, h.line), st.first_visible_row(), st.visible_file());
    find.current = before.and_then(|b| find.hits.iter().position(|h| *h == b)).or(first);
    paint(app);
    true
}

/// A new search starts at the first match at or below the top of the view, wrapping to the
/// first; a match with no row (folded file or gap) counts by its file.
fn first_visible(hits: &[Hit], row: &dyn Fn(&Hit) -> Option<u32>, top: u32, file: usize) -> Option<usize> {
    if hits.is_empty() {
        return None;
    }
    Some(hits.iter().position(|h| row(h).map_or(h.f > file, |r| r >= top)).unwrap_or(0))
}

/// Shows match `j`: unfolds its file, opens its gap, scrolls it below the sticky header.
fn go(app: &mut App, tx: &mut Tx, j: usize) {
    let ui = &mut app.ui.panel.diff;
    let Some(find) = ui.find.as_mut() else { return };
    let Some(hit) = find.hits.get(j).cloned() else { return };
    find.current = Some(j);
    if ui.state.collapsed(hit.f) {
        ui.touched = true;
        ui.state.set_collapsed(hit.f, false);
    }
    if hit.h & GAP != 0 {
        let g = (hit.h & !GAP) as usize;
        if ui.state.row_at(hit.f, hit.side, hit.line).is_none() {
            let n = ui.state.gap_len(hit.f, g);
            let path = ui.state.files()[hit.f].path().to_owned();
            ui.expanded.entry(path).or_default().insert(g as u32, (n, 0));
            super::sync_extras(app);
        }
    }
    let ui = &mut app.ui.panel.diff;
    if let Some(r) = ui.state.row_at(hit.f, hit.side, hit.line) {
        // Two rows of headroom keep the match clear of the floating find bar.
        ui.state.reveal_row(r.saturating_sub(2), tx);
    }
    paint(app);
}

fn navigate(app: &mut App, tx: &mut Tx, back: bool) {
    let Some(find) = &app.ui.panel.diff.find else { return };
    let n = find.hits.len();
    if n == 0 {
        return;
    }
    let j = match (find.current, back) {
        (Some(c), false) => (c + 1) % n,
        (Some(c), true) => (c + n - 1) % n,
        (None, _) => 0,
    };
    go(app, tx, j);
}

fn open(app: &mut App) {
    let ui = &mut app.ui.panel.diff;
    ui.find.get_or_insert_with(Find::default);
    let len = ui.find_edit.text.len();
    ui.find_edit.set_selection(0, len);
    ui.find_edit.focused = true;
    refresh(app);
}

pub fn close(app: &mut App) {
    let ui = &mut app.ui.panel.diff;
    if ui.find.take().is_some() {
        ui.find_edit.focused = false;
        ui.state.set_hits(HashMap::new());
    }
}

/// The diff's keys: Cmd/Ctrl+F opens, Cmd/Ctrl+G and Shift steps, Escape closes.
pub fn key(app: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if e.is_command('f') {
        open(app);
        return true;
    }
    if app.ui.panel.diff.find.is_none() {
        return false;
    }
    if e.is_command('g') {
        navigate(app, tx, e.mods.shift);
        return true;
    }
    if e.key == Key::Escape {
        close(app);
        return true;
    }
    false
}

fn input_key(app: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    match e.key {
        Key::Enter => navigate(app, tx, e.mods.shift),
        Key::Escape => {
            close(app);
            tx.blur();
        }
        _ if e.is_command('g') => navigate(app, tx, e.mods.shift),
        _ => return false,
    }
    true
}

fn changed(app: &mut App, tx: &mut Tx) {
    if refresh(app)
        && let Some(j) = app.ui.panel.diff.find.as_ref().and_then(|f| f.current)
    {
        go(app, tx, j);
    }
}

fn option(t: &T, label: &'static str, glyph: &'static str, on: bool, flag: fn(&mut Find) -> &mut bool) -> El {
    let fg = if on { t.c.primary } else { t.c.muted_foreground };
    let glyph_el = if glyph == "ab" {
        rich(glyph, vec![Span::new(0, 2).underline(fg)]).font(theme::mono(theme::TEXT_2XS)).color(fg)
    } else {
        text(glyph).font(theme::mono(theme::TEXT_2XS)).color(fg)
    };
    let mut b = row().key(label).role("switch").label(label).help(label).h(22.0).min_w(24.0).px(4.0).center().radius(5.0).hover(t.c.accent).cursor(Cursor::Pointer).child(glyph_el);
    if on {
        b = b.fill(t.c.primary.alpha_mul(0.12));
    }
    b.on_press_tx(move |s: &mut App, tx| {
        if let Some(f) = s.ui.panel.diff.find.as_mut() {
            let v = flag(f);
            *v = !*v;
        }
        changed(s, tx);
    })
}

fn button(t: &T, key: &'static str, label: &'static str, glyph: &'static Icon, enabled: bool, act: impl Fn(&mut App, &mut Tx) + 'static) -> El {
    let b = row().key(key).role("button").label(label).help(label).size(22.0, 22.0).center().radius(5.0).child(icon(glyph, 14.0).color(t.c.muted_foreground));
    if !enabled {
        return b.disabled(true).opacity(0.5);
    }
    b.hover(t.c.accent).cursor(Cursor::Pointer).on_press_tx(act)
}

pub fn bar(s: &App, t: &T) -> Option<El> {
    let ui = &s.ui.panel.diff;
    let find = ui.find.as_ref()?;
    let n = find.hits.len();
    let count = match (ui.find_edit.text.is_empty(), find.current) {
        (true, _) => String::new(),
        (false, Some(c)) => format!("{} of {n}", c + 1),
        (false, None) => "No results".into(),
    };
    let input = text_input(&ui.find_edit, theme::TEXT_SM)
        .key("diff-find-input")
        .role("textbox")
        .label("Find")
        .single_line()
        .w(170.0)
        .h(24.0)
        .px(6.0)
        .py(2.0)
        .color(t.c.popover_foreground)
        .placeholder("Find", t.c.muted_foreground)
        .caret(t.c.popover_foreground, t.c.primary.alpha_mul(0.3))
        .on_edit(|s: &mut App| &mut s.ui.panel.diff.find_edit)
        .on_input_key(input_key)
        .on_input_change(changed);
    Some(
        row()
            .key("diff-find")
            .gap(2.0)
            .padding(4.0)
            .items_center()
            .radius(8.0)
            .fill(t.c.popover)
            .border(1.0, t.c.border)
            .shadow(8.0, 24.0, Color::BLACK.alpha_mul(0.25))
            .role("search")
            .label("Find in diff")
            .child(input)
            .child(option(t, "Match case", "Aa", find.case, |f| &mut f.case))
            .child(option(t, "Match whole word", "ab", find.word, |f| &mut f.word))
            .child(option(t, "Use regular expression", ".*", find.regex, |f| &mut f.regex))
            .child(text(count).font(theme::TEXT_2XS).color(t.c.muted_foreground).min_w(56.0).px(4.0))
            .child(button(t, "prev", "Previous match", &icons::ARROW_UP, n > 0, |s, tx| navigate(s, tx, true)))
            .child(button(t, "next", "Next match", &icons::ARROW_DOWN, n > 0, |s, tx| navigate(s, tx, false)))
            .child(button(t, "close", "Close find", &icons::X, true, |s, tx| {
                close(s);
                tx.blur();
            })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,2 @@\n let Foo = foo;\n-old_foo(x)\n+food(x)\n";

    fn find(query: &str, case: bool, word: bool, regex: bool) -> Vec<(u32, u32, Side, Range<usize>)> {
        let st = DiffState::new(rune_kit::diff::parse(PATCH));
        let re = pattern(query, case, regex).expect("pattern");
        collect(&st, &re, word).into_iter().map(|h| (h.i, h.line, h.side, h.range)).collect()
    }

    #[test]
    fn find_matches_by_case_word_and_regex() {
        assert_eq!(find("foo", false, false, false), [(0, 1, Side::New, 4..7), (0, 1, Side::New, 10..13), (1, 2, Side::Old, 4..7), (2, 2, Side::New, 0..3)]);
        assert_eq!(find("Foo", true, false, false), [(0, 1, Side::New, 4..7)]);
        // `_` is not a separator, `(` is: `old_foo` is one word, `food` is another.
        assert_eq!(find("foo", false, true, false), [(0, 1, Side::New, 4..7), (0, 1, Side::New, 10..13)]);
        assert_eq!(find("fo+d?\\(", false, false, true), [(1, 2, Side::Old, 4..8), (2, 2, Side::New, 0..5)]);
        // Without regex the pattern is literal.
        assert_eq!(find("(x)", false, false, false), [(1, 2, Side::Old, 7..10), (2, 2, Side::New, 4..7)]);
        assert!(pattern("(", false, true).is_none());
    }

    #[test]
    fn a_new_search_starts_at_the_first_match_in_view() {
        let hit = |f: usize, line: u32| Hit { f, h: 0, i: 0, side: Side::New, line, range: 0..1 };
        // File 0 rows 0-9 (line n at row n), file 1 is folded, file 2 line n at row 20 + n.
        let hits = [hit(0, 2), hit(0, 8), hit(1, 3), hit(2, 4)];
        let row = |h: &Hit| match h.f {
            0 => Some(h.line),
            2 => Some(20 + h.line),
            _ => None,
        };
        assert_eq!(first_visible(&hits, &row, 5, 0), Some(1));
        assert_eq!(first_visible(&hits, &row, 10, 0), Some(2), "the folded file below the view");
        assert_eq!(first_visible(&hits, &row, 22, 2), Some(3));
        assert_eq!(first_visible(&hits, &row, 30, 2), Some(0), "nothing below wraps to the first");
        assert_eq!(first_visible(&[], &row, 0, 0), None);
    }
}
