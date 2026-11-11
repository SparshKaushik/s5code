//! Drawing the diff: a virtual list of file headers, hunk separators and lines, with a sticky
//! copy of the header being read. The look is web's: `@pierre/diffs` under
//! `DIFF_SURFACE_THEME_UNSAFE_CSS` (diffRendering.ts) and StyledDiffCodeView's overrides.

use std::borrow::Cow;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

use rune::{Color, Cursor, Drag, El, GrowthSpec, Icon, Phase, ScrollSpec, Span, TextStyle, Tx, col, el, icon, memo, rich, row, text};
use rune_kit::diff::{FileDiff, FileStatus, LineKind, Side};
use rune_kit::syntax::SyntaxTheme;

use super::state::{DiffMode, DiffState, GAP, HEADER_H, LINE_H, LineRef, NOTE_H, Row, SEP_H};
use crate::{App, icons, kit, theme, theme::T};

/// The change-type marks pierre draws before a file's name.
static ADDED: Icon = Icon {
    name: "diff-added",
    view: 24.0,
    body: r#"<rect width="18" height="18" x="3" y="3" rx="2" stroke="currentColor" stroke-width="2" fill="none"/><path d="M8 12h8M12 8v8" stroke="currentColor" stroke-width="2" stroke-linecap="round" fill="none"/>"#,
};
static DELETED: Icon = Icon {
    name: "diff-deleted",
    view: 24.0,
    body: r#"<rect width="18" height="18" x="3" y="3" rx="2" stroke="currentColor" stroke-width="2" fill="none"/><path d="M8 12h8" stroke="currentColor" stroke-width="2" stroke-linecap="round" fill="none"/>"#,
};
static MODIFIED: Icon = Icon {
    name: "diff-modified",
    view: 24.0,
    body: r#"<rect width="18" height="18" x="3" y="3" rx="2" stroke="currentColor" stroke-width="2" fill="none"/><circle cx="12" cy="12" r="2.5" fill="currentColor"/>"#,
};
static RENAMED: Icon = Icon {
    name: "diff-renamed",
    view: 24.0,
    body: r#"<rect width="18" height="18" x="3" y="3" rx="2" stroke="currentColor" stroke-width="2" fill="none"/><path d="M8 12h8m-3.5-3.5L16 12l-3.5 3.5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

/// Width of the bar marking an added, removed or selected line.
const BAR: f32 = 4.0;
/// Space between the numbers and the code, and after the code.
const CODE_PAD: (f32, f32) = (12.0, 24.0);

/// CSS `color-mix(in srgb, a p, b)`, mixing gamma-encoded channels as browsers do.
pub fn mix(a: Color, b: Color, p: f32) -> Color {
    let enc = |c: f32| if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    let dec = |c: f32| if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    let m = |x: f32, y: f32| dec(enc(x) * p + enc(y) * (1.0 - p));
    Color { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b), a: a.a * p + b.a * (1.0 - p) }
}

#[derive(Clone, Copy)]
struct Tints {
    line: Color,
    number_bg: Color,
    hover: Color,
    /// Numbers and change marks.
    mark: Color,
    /// The header's counts.
    count: Color,
}

pub struct Look {
    pub t: T,
    code: TextStyle,
    title: TextStyle,
    small: TextStyle,
    count: TextStyle,
    adv: f32,
    pub bg: Color,
    fg: Color,
    muted: Color,
    number: Color,
    hover: Color,
    sep_bg: Color,
    sep_fg: Color,
    buffer: Color,
    border: Color,
    button_hover: Color,
    header_bar: Color,
    add: Tints,
    del: Tints,
    modified: Color,
    selected: Color,
    selected_gutter: Color,
    selection: Color,
    find_match: Color,
    find_focus: Color,
    syntax: SyntaxTheme,
    version: u64,
}

impl Look {
    pub fn new(t: &T) -> Look {
        let c = &t.c;
        let (bg, fg) = (c.code_background, c.code_foreground);
        let tints = |hue: Color, count: Color| Tints {
            line: mix(bg, hue, if t.dark { 0.7 } else { 0.5 }),
            number_bg: mix(bg, hue, if t.dark { 0.6 } else { 0.35 }),
            hover: mix(bg, hue, 0.85),
            mark: if t.dark { hue } else { count },
            count,
        };
        let modified = c.diff_modified_icon.alpha(1.0);
        let mut look = Look {
            t: *t,
            code: theme::mono(TextStyle::new(13.0, 400.0).lh(LINE_H)),
            title: theme::TEXT_XS,
            small: theme::TEXT_2XS,
            count: theme::mono(theme::TEXT_2XS),
            adv: 0.6 * 13.0,
            bg,
            fg,
            muted: c.muted_foreground,
            number: mix(fg, bg, 0.5),
            hover: mix(bg, fg, 0.94),
            sep_bg: mix(bg, fg, 0.97),
            sep_fg: mix(fg, bg, 0.5),
            buffer: mix(bg, fg, 0.9),
            border: c.border,
            button_hover: c.accent,
            header_bar: fg.alpha(0.24),
            add: tints(c.diff_addition, c.diff_addition_foreground),
            del: tints(c.diff_deletion, c.diff_deletion_foreground),
            modified: c.diff_modified_icon,
            selected: mix(bg, modified, 0.85),
            selected_gutter: mix(bg, modified, 0.7),
            selection: kit::theme(t).selection(),
            // Pierre's ::highlight(diffs-search-match) and -focus.
            find_match: Color { r: 1.0, g: 0.304987, b: 0.031896, a: if t.dark { 0.4 } else { 0.533 } }, // #ff9632 66 / 88
            find_focus: c.primary.alpha(0.45),
            syntax: SyntaxTheme::neutral(t.dark),
            version: 0,
        };
        let mut h = std::hash::DefaultHasher::new();
        for c in [look.bg, look.fg, look.add.line, look.del.line, look.modified] {
            [c.r, c.g, c.b, c.a].map(f32::to_bits).hash(&mut h);
        }
        t.dark.hash(&mut h);
        look.version = h.finish();
        look
    }

    fn tints(&self, kind: LineKind) -> Option<&Tints> {
        match kind {
            LineKind::Added => Some(&self.add),
            LineKind::Removed => Some(&self.del),
            LineKind::Context => None,
        }
    }

    fn number_w(&self, digits: u32) -> f32 {
        (digits as f32 * self.adv + 16.0).ceil()
    }
}

/// One side's line in a row.
#[derive(Clone, Copy)]
struct Cell<'a> {
    kind: LineKind,
    old: Option<u32>,
    new: Option<u32>,
    text: &'a str,
    no_newline: bool,
    h: usize,
    i: u32,
    /// A gap's line (`i` is its index in the gap), shown from the loaded file.
    gap: bool,
}

impl Cell<'_> {
    fn label(&self) -> String {
        let kind = match self.kind {
            LineKind::Added => "added",
            LineKind::Removed => "removed",
            LineKind::Context => "unchanged",
        };
        format!("{kind} line {}: {}", self.new.or(self.old).unwrap_or(0), self.text)
    }
}

fn st(s: &App) -> &DiffState {
    &s.ui.panel.diff.state
}

fn st_mut(s: &mut App) -> &mut DiffState {
    &mut s.ui.panel.diff.state
}

fn row_key(s: &App, i: usize) -> Cow<'_, str> {
    Cow::Borrowed(st(s).keys[i].as_str())
}

/// The scrolling diff, filling the height it is given.
pub fn build(s: &App, look: Rc<Look>) -> El {
    let st = st(s);
    let wrap = st.wraps();
    if !wrap && let Some(r) = rune::memo::layout("panel-diff-view/scroll") {
        st.set_viewport(r);
    }
    let mut list = col().key("rows").w_full().growth(GrowthSpec::default()).selection_group(true).role("list").label("Diff");
    if !wrap {
        list = list.min_w(content_width(&look, st).max(st.view_width()));
    }
    let l = look.clone();
    let list = list
        .virtual_rows(st.rows.len(), LINE_H, row_key, move |s: &App, i| row_el(s, &l, i))
        .virtual_rows_version(st.keys_rev)
        .on_visible_rows(|s: &mut App, rows| st_mut(s).set_visible_rows(rows));
    let scroll = col().key("scroll").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false });
    let scroll = if wrap { scroll } else { scroll.scroll_xy() };
    let mut root = col().key("panel-diff-view").focusable(true).grow(1.0).min_w(0.0).min_h(0.0).self_stretch().fill(look.bg).child(scroll.child(list));
    if let Some(f) = st.sticky() {
        root = root.child(file_header(s, &look, f, true).key("sticky").absolute(0.0, 0.0).w_full());
    }
    if let Some(bar) = super::find::bar(s, &look.t) {
        root = root.child(bar.pin(None, Some(HEADER_H + 8.0), Some(12.0), None));
    }
    root.on_key(|s: &mut App, e, tx| super::find::key(s, e, tx) || st_mut(s).key(e, tx))
}

fn content_width(l: &Look, st: &DiffState) -> f32 {
    let digits = (0..st.files().len()).map(|f| st.digits(f)).max().unwrap_or(3);
    BAR + l.number_w(digits) + st.columns() as f32 * l.adv + CODE_PAD.0 + CODE_PAD.1
}

/// The viewport width a full-width part (a header, a separator) spans while lines scroll sideways.
fn view_w(st: &DiffState) -> Option<f32> {
    let w = st.view_width();
    (w > 0.0 && !st.wraps()).then_some(w)
}

fn row_el(s: &App, l: &Look, i: usize) -> El {
    let st = st(s);
    let r = st.rows[i];
    let key = st.keys[i].as_str();
    let vw = st.view_width().to_bits();
    match r {
        Row::File(f) => file_header(s, l, f as usize, false),
        Row::Note(f) => memo(key, (l.version, st.rev, vw), || note(st, l, f as usize)),
        Row::Sep { f, h } => memo(key, (l.version, st.rev, vw), || separator(st, l, f as usize, h as usize)),
        Row::Comment { n, .. } => {
            let mut inner = super::comments::view(s, l, n as usize).key("inner");
            inner = match view_w(st) {
                Some(w) => inner.w(w).sticky_left(0.0),
                None => inner.w_full(),
            };
            row().w_full().child(inner)
        }
        _ => {
            let version = (l.version, st.rev, st.row_selected(i, None), st.selection().map(|s| (s.start.side, s.end.side)), st.hushed);
            memo(key, version, || line(st, l, i))
        }
    }
}

fn cells(st: &DiffState, r: Row) -> (Option<Cell<'_>>, Option<Cell<'_>>) {
    let of = |f: u32, h: u32, i: u32| {
        let line = &st.files()[f as usize].hunks[h as usize].lines[i as usize];
        Cell { kind: line.kind, old: line.old, new: line.new, text: &line.text, no_newline: line.no_newline, h: h as usize, i, gap: false }
    };
    match r {
        Row::Gap { f, h, k } => match st.gap_line(f as usize, h as usize, k) {
            Some((old, new, text)) => (Some(Cell { kind: LineKind::Context, old: Some(old), new: Some(new), text, no_newline: false, h: h as usize, i: k, gap: true }), None),
            None => (None, None),
        },
        Row::Line { f, h, i } => (Some(of(f, h, i)), None),
        Row::Pair { f, h, del, add } => (Some(of(f, h, del)), Some(of(f, h, add))),
        _ => (None, None),
    }
}

fn line(st: &DiffState, l: &Look, i: usize) -> El {
    let r = st.rows[i];
    let f = r.file();
    let (a, b) = cells(st, r);
    let Some(a) = a else { return col() };
    match st.mode() {
        DiffMode::Unified => unified(st, l, i, f, a),
        DiffMode::Split => {
            let (old, new) = match (b, a.kind) {
                (Some(b), _) => (Some(a), Some(b)),
                (None, LineKind::Removed) => (Some(a), None),
                (None, LineKind::Added) => (None, Some(a)),
                (None, LineKind::Context) => (Some(a), Some(a)),
            };
            split(st, l, i, f, old, new)
        }
    }
}

/// The marker bar and line number, held to the viewport's left edge while code scrolls under it.
fn gutter(st: &DiffState, l: &Look, i: usize, f: usize, c: &Cell<'_>, side: Side, selected: bool) -> El {
    let tints = l.tints(c.kind);
    let fill = match (selected, tints) {
        (true, _) => l.selected_gutter,
        (false, Some(t)) => t.number_bg,
        (false, None) => l.bg,
    };
    let bar = match (selected, tints) {
        (true, _) => Some(l.modified),
        (false, Some(t)) => Some(t.mark),
        (false, None) => None,
    };
    let mut b = el().key("bar").w(BAR).self_stretch().a11y_hidden();
    if let Some(c) = bar {
        b = b.fill(c);
    }
    let n = if side == Side::Old { c.old } else { c.new };
    let mut g = row()
        .key("gutter")
        .self_stretch()
        .items_stretch()
        .fill(fill)
        .child(b)
        .child(number(st, l, i, f, n, side, c.kind, selected));
    // Pierre's gutter utility: a comment on the selected lines, or on this one.
    if let Some(line) = n.filter(|_| !st.notes().iter().any(|n| n.draft)) {
        let at = LineRef { side, line };
        let mut button = row()
            .key("comment")
            .absolute(BAR + 2.0, 2.0)
            .size(16.0, 16.0)
            .center()
            .radius(4.0)
            .fill(l.modified.alpha(1.0))
            .cursor(Cursor::Pointer)
            .role("button")
            .label("Comment on this line".to_owned())
            .group_opacity(0.0, 1.0)
            .child(icon(&icons::PLUS, 12.0).color(Color::WHITE))
            .on_hover_change(move |s: &mut App, inside, _| {
                // The button leaves the tree while its editor is open; that is not the pointer leaving.
                let ui = &mut s.ui.panel.diff;
                if ui.comment.is_none() && (!inside || ui.state.hushed != Some((f, at))) {
                    ui.state.hushed = None;
                }
            })
            .on_press(move |s: &mut App| super::comments::begin(s, f, at));
        if st.hushed != Some((f, at)) {
            button = button.help("Comment".to_owned());
        }
        g = g.child(button);
    }
    g
}

fn unified(st: &DiffState, l: &Look, i: usize, f: usize, c: Cell<'_>) -> El {
    let selected = st.row_selected(i, None);
    let tints = l.tints(c.kind);
    let fill = if selected { l.selected } else { tints.map_or(l.bg, |t| t.line) };
    // One number column, as pierre's stacked view: the old number for a removed line.
    let side = if c.kind == LineKind::Removed { Side::Old } else { Side::New };
    let mut g = gutter(st, l, i, f, &c, side, selected);
    if !st.wraps() {
        g = g.sticky_left(0.0);
    }
    row()
        .w_full()
        .min_h(LINE_H)
        .items_start()
        .fill(fill)
        .hover(tints.map_or(l.hover, |t| t.hover))
        .hover_group()
        .role("listitem")
        .label(c.label())
        .child(g)
        .child(code(st, l, f, &c, side).key("code"))
}

fn split<'a>(st: &'a DiffState, l: &Look, i: usize, f: usize, old: Option<Cell<'a>>, new: Option<Cell<'a>>) -> El {
    let half = |side: Side, c: Option<Cell<'a>>| -> El {
        let key = if side == Side::Old { "old" } else { "new" };
        let h = row().key(key).w_frac(0.5).min_w(0.0).min_h(LINE_H).items_start().self_stretch();
        let Some(c) = c else { return h.fill(l.buffer) };
        let selected = st.row_selected(i, Some(side));
        let tints = l.tints(c.kind);
        let fill = if selected { l.selected } else { tints.map_or(l.bg, |t| t.line) };
        h.fill(fill)
            .hover(tints.map_or(l.hover, |t| t.hover))
            .hover_group()
            .role("listitem")
            .label(c.label())
            .child(gutter(st, l, i, f, &c, side, selected))
            .child(code(st, l, f, &c, side).key("code"))
    };
    let divider = el().key("divider").w(1.0).self_stretch().fill(l.border);
    row().w_full().min_h(LINE_H).items_stretch().child(half(Side::Old, old)).child(divider).child(half(Side::New, new))
}

/// A line number: pressing it selects the line, shift-pressing extends the selection, and
/// dragging down the numbers selects the lines passed.
#[allow(clippy::too_many_arguments)]
fn number(st: &DiffState, l: &Look, i: usize, f: usize, n: Option<u32>, side: Side, kind: LineKind, selected: bool) -> El {
    let c = row().w(l.number_w(st.digits(f)) - BAR).min_h(LINE_H).self_stretch().pl(4.0).pr(8.0).justify_end().items_start().a11y_hidden();
    let Some(n) = n else { return c };
    let color = if selected { l.fg } else { l.tints(kind).map_or(l.number, |t| t.mark) };
    let at = LineRef { side, line: n };
    let row_at = i as u32;
    c.child(text(n.to_string()).font(l.code).color(color)).cursor(Cursor::Pointer).on_drag(move |s: &mut App, d: &mut Drag, _tx| {
        let st = st_mut(s);
        match d.phase {
            Phase::Down => st.press_line(f, at),
            Phase::Move => {
                let Some(r) = st.row_from(row_at, d.y) else { return };
                let r = st.rows[r as usize];
                if r.file() != f {
                    return;
                }
                let other = if side == Side::Old { Side::New } else { Side::Old };
                let to = st.row_line(r, side).map(|n| LineRef { side, line: n }).or_else(|| st.row_line(r, other).map(|n| LineRef { side: other, line: n }));
                if let Some(to) = to {
                    st.drag_to(f, to);
                }
            }
            Phase::Up => {
                if d.cancelled() {
                    st.cancel_line();
                } else {
                    st.release_line();
                }
            }
        }
    })
}

/// A line's code: syntax colours, tabs padded to four columns.
fn code(st: &DiffState, l: &Look, f: usize, c: &Cell<'_>, side: Side) -> El {
    let painted = if c.gap { st.gap_highlights(f, c.h, c.i) } else { st.highlights(f, c.h, c.i, side) };
    let syntax: Vec<(Range<usize>, Color)> = painted.into_iter().map(|(r, p)| (r, l.syntax.paint(p))).filter(|(r, _)| r.end <= c.text.len()).collect();
    let hits = st.hits_on(f, if c.gap { GAP | c.h as u32 } else { c.h as u32 }, c.i);
    let spans = if hits.is_empty() {
        syntax
            .into_iter()
            .map(|(r, color)| {
                let mut sp = Span::new(r.start, r.end);
                sp.color = Some(color);
                sp
            })
            .collect()
    } else {
        segments(c.text.len(), &syntax, hits)
            .into_iter()
            .map(|(r, color, hit)| {
                let mut sp = Span::new(r.start, r.end);
                sp.color = color;
                match hit {
                    Some(true) => sp.bg(l.find_focus, 2.0),
                    Some(false) => sp.bg(l.find_match, 2.0),
                    None => sp,
                }
            })
            .collect()
    };
    let (shown, spans, pads) = tabbed(c.text, spans, st.wraps());
    let mut t = rich(shown, spans).font(l.code).color(l.fg).key("text").selectable(true).selection_color(l.selection);
    t = if st.wraps() { t.w_full() } else { t.single_line() };
    t = pads.into_iter().fold(t, |t, r| t.uncopied(r));
    let mut code = col().grow(1.0).min_w(0.0).pl(CODE_PAD.0).pr(CODE_PAD.1).child(t);
    if c.no_newline {
        code = code.child(text("No newline at end of file").font(l.small).color(l.sep_fg).key("eof").pb(2.0));
    }
    code
}

/// `text`'s pieces split at syntax and find boundaries: each piece's colour and whether it is a
/// match (`Some(true)` for the current one). Text layout takes spans that do not overlap.
fn segments(len: usize, syntax: &[(Range<usize>, Color)], hits: &[(Range<usize>, bool)]) -> Vec<(Range<usize>, Option<Color>, Option<bool>)> {
    let mut cuts: Vec<usize> = syntax.iter().flat_map(|(r, _)| [r.start, r.end]).chain(hits.iter().flat_map(|(r, _)| [r.start, r.end])).filter(|&b| b <= len).collect();
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .filter_map(|w| {
            let (a, b) = (w[0], w[1]);
            let color = syntax.iter().find(|(r, _)| r.start <= a && b <= r.end).map(|(_, c)| *c);
            let hit = hits.iter().find(|(r, _)| r.start <= a && b <= r.end).map(|(_, current)| *current);
            (color.is_some() || hit.is_some()).then_some((a..b, color, hit))
        })
        .collect()
}

fn gutter_w(st: &DiffState, l: &Look, f: usize) -> f32 {
    l.number_w(st.digits(f))
}

/// The unchanged lines folded away before a hunk: "N unmodified lines".
fn separator(st: &DiffState, l: &Look, f: usize, h: usize) -> El {
    let n = st.remaining(f, h);
    let label = if n == 1 { "1 unmodified line".to_owned() } else { format!("{n} unmodified lines") };
    let mut cell = row().key("expand").w(gutter_w(st, l, f)).h(SEP_H).center().gap(2.0);
    if st.expandable {
        use super::Expand;
        let tail = h == st.files()[f].hunks.len();
        let mut buttons: Vec<(&'static str, &'static str, &'static Icon, Expand)> = vec![];
        if n <= super::EXPAND_STEP {
            buttons.push(("all", "Expand all lines", &icons::CHEVRONS_UP_DOWN, Expand::All));
        } else {
            if h > 0 {
                buttons.push(("down", "Expand down", &icons::ARROW_DOWN, Expand::Down));
            }
            if !tail {
                buttons.push(("up", "Expand up", &icons::ARROW_UP, Expand::Up));
            }
        }
        for (key, label, glyph, how) in buttons {
            cell = cell.child(micro(l, key, label, glyph, 14.0, l.sep_fg).on_press(move |s: &mut App| super::expand(s, f, h, how)));
        }
    }
    let mut inner = row().key("inner").h(SEP_H).items_center().child(cell).child(text(label.clone()).font(l.small).color(l.sep_fg).pl(CODE_PAD.0));
    if let Some(w) = view_w(st) {
        inner = inner.w(w).sticky_left(0.0);
    }
    row().w_full().h(SEP_H).fill(l.sep_bg).role("separator").label(label).child(inner)
}

/// A file with no lines to show.
fn note(st: &DiffState, l: &Look, f: usize) -> El {
    let file = &st.files()[f];
    let msg = if file.binary {
        "Binary file not shown"
    } else if matches!(file.status, FileStatus::Renamed | FileStatus::Copied) {
        "File renamed without changes"
    } else if file.old_mode != file.new_mode {
        "File mode changed"
    } else if matches!(file.status, FileStatus::Added | FileStatus::Deleted) {
        "Empty file"
    } else {
        "No changes"
    };
    let mut inner = row().key("inner").h(NOTE_H).items_center().pl(gutter_w(st, l, f) + CODE_PAD.0).child(text(msg).font(l.small).color(l.sep_fg));
    if let Some(w) = view_w(st) {
        inner = inner.w(w).sticky_left(0.0);
    }
    row().w_full().h(NOTE_H).fill(l.bg).role("note").label(msg).child(inner)
}

fn change_mark(l: &Look, file: &FileDiff) -> (&'static Icon, Color) {
    match file.status {
        FileStatus::Added => (&ADDED, l.add.mark),
        FileStatus::Deleted => (&DELETED, l.del.mark),
        FileStatus::Renamed | FileStatus::Copied => (&RENAMED, l.modified),
        FileStatus::Modified => (&MODIFIED, l.modified),
    }
}

/// A 20-point ghost icon button (`icon-micro`).
fn micro(l: &Look, key: &'static str, label: &str, glyph: &'static Icon, size: f32, color: Color) -> El {
    row()
        .key(key)
        .size(20.0, 20.0)
        .center()
        .radius(4.0)
        .hover(l.button_hover)
        .cursor(Cursor::Pointer)
        .role("button")
        .label(label.to_owned())
        .help(label.to_owned())
        .child(icon(glyph, size).color(color))
}

fn toggle_file(s: &mut App, f: usize, sticky: bool, tx: &mut Tx) {
    s.ui.panel.diff.touched = true;
    let st = st_mut(s);
    let collapsed = !st.collapsed(f);
    st.set_collapsed(f, collapsed);
    if sticky {
        st.scroll_to_file(f, tx);
    }
}

/// Pierre's file header: collapse toggle, change mark, path (renames as old → new), copy-path
/// button, then the counts. Pressing the path opens the file; pressing elsewhere folds it.
fn file_header(s: &App, l: &Look, f: usize, sticky: bool) -> El {
    let st = st(s);
    let file = &st.files()[f];
    let collapsed = st.collapsed(f);
    let toggle = micro(
        l,
        "toggle",
        if collapsed { "Expand diff" } else { "Collapse diff" },
        if collapsed { &icons::CHEVRON_RIGHT } else { &icons::CHEVRON_DOWN },
        16.0,
        l.muted.alpha(l.muted.a * 0.8),
    )
    .expanded(!collapsed)
    .ml(-2.0)
    .on_press_tx(move |s: &mut App, tx| toggle_file(s, f, sticky, tx));
    let (mark, mark_color) = change_mark(l, file);
    let path = file.path().to_owned();
    let (open, menu) = (path.clone(), path.clone());
    let title = text(path.clone())
        .font(l.title)
        .color(l.fg)
        .truncate_head()
        .min_w(0.0)
        .shrink()
        .key("title")
        .cursor(Cursor::Pointer)
        .on_press(move |s: &mut App| crate::panel::open_file(s, open.clone()))
        .on_secondary_press(move |s: &mut App, at: rune::PointerPress, _: &mut Tx| super::file_menu::open(s, menu.clone(), (at.x, at.y)));
    let mut name = row().key("name").items_center().gap(6.0).min_w(0.0).shrink();
    if let (FileStatus::Renamed | FileStatus::Copied, Some(old)) = (file.status, file.old_path.as_deref()) {
        name = name
            .child(text(old.to_owned()).font(l.title).color(l.muted).truncate_head().min_w(0.0).shrink().key("prev"))
            .child(icon(&icons::ARROW_RIGHT, 12.0).color(l.muted).key("arrow"));
    }
    name = name.child(title);
    let copy = micro(l, "copy", "Copy path", &icons::COPY, 12.0, l.muted).on_press_tx(move |_: &mut App, tx| tx.copy(path.clone()));
    let mut inner = row()
        .key("inner")
        .w_full()
        .h(HEADER_H)
        .pl(8.0)
        .pr(12.0)
        .gap(6.0)
        .items_center()
        .child(toggle)
        .child(icon(mark, 16.0).color(mark_color).key("mark"))
        .child(name)
        .child(copy);
    let ui = &s.ui.panel.diff;
    if ui.partial.contains(file.path()) {
        let note = "This file is too large to show in full. Counts include all changes.";
        inner = inner.child(micro(l, "partial", note, &icons::INFO, 12.0, l.muted));
    }
    inner = inner.child(el().key("space").grow(1.0).a11y_hidden());
    let (adds, dels) = super::counts(ui, file);
    if adds > 0 {
        inner = inner.child(text(format!("+{adds}")).font(l.count).color(l.add.count).key("adds"));
    }
    if dels > 0 {
        inner = inner.child(text(format!("-{dels}")).font(l.count).color(l.del.count).key("dels"));
    }
    let edge = el().key("edge").absolute(0.0, 0.0).size(3.0, HEADER_H).fill(l.header_bar).group_opacity(0.0, 1.0).a11y_hidden();
    let mut label = file.path().to_owned();
    if let (FileStatus::Renamed, Some(a)) = (file.status, file.old_path.as_deref()) {
        label.push_str(&format!(", renamed from {a}"));
    }
    label.push_str(&format!(", {adds} added, {dels} removed"));
    let mut h = row()
        .w_full()
        .h(HEADER_H)
        .fill(l.bg)
        .hover_group()
        .cursor(Cursor::Pointer)
        .role("heading")
        .label(label)
        .child(inner)
        .child(edge)
        .on_press_tx(move |s: &mut App, tx| toggle_file(s, f, sticky, tx));
    if let (false, Some(w)) = (sticky, view_w(st)) {
        h = h.w(w).sticky_left(0.0);
    }
    h
}

/// `code` as drawn: text layout draws a tab as one space, so each tab is followed by spaces up
/// to the next four-column stop; and when `wrap`, a zero-width space after `.`, `(`, `[` and
/// `/` lets a long call chain or path break there. Returns the text, `spans` moved to match,
/// and the inserted ranges (drawn, never copied). From `rune_kit::diff`.
fn tabbed(code: &str, mut spans: Vec<Span>, wrap: bool) -> (String, Vec<Span>, Vec<Range<usize>>) {
    if !code.contains('\t') && !(wrap && code.contains(['.', '(', '[', '/'])) {
        return (code.to_owned(), spans, Vec::new());
    }
    let mut shown = String::with_capacity(code.len() + 8);
    let mut shifts: Vec<(usize, usize)> = Vec::new();
    let mut pads = Vec::new();
    let mut col = 0;
    let mut chars = code.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        shown.push(ch);
        col += 1;
        let start = shown.len();
        if ch == '\t' {
            let pad = 3 - (col - 1) % 4;
            shown.extend(std::iter::repeat_n(' ', pad));
            col += pad;
        } else if wrap && matches!(ch, '.' | '(' | '[' | '/') && chars.peek().is_some_and(|(_, c)| !c.is_whitespace()) {
            shown.push('\u{200B}');
        }
        if shown.len() > start {
            pads.push(start..shown.len());
            shifts.push((i + ch.len_utf8(), shifts.last().map_or(0, |s| s.1) + shown.len() - start));
        }
    }
    let shift = |b: u32| match shifts.partition_point(|s| s.0 <= b as usize) {
        0 => b,
        n => b + shifts[n - 1].1 as u32,
    };
    for sp in &mut spans {
        (sp.start, sp.end) = (shift(sp.start), shift(sp.end));
    }
    (shown, spans, pads)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_in_srgb_like_css_color_mix() {
        let (black, white) = (Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
        // color-mix(in srgb, black 50%, white) is 0.5 encoded (~#808080), not linear 50% grey.
        let grey = mix(black, white, 0.5);
        assert!((grey.r - 0.2140).abs() < 0.001, "{}", grey.r);
        assert_eq!(mix(black, white, 1.0).r, 0.0);
    }

    #[test]
    fn find_matches_split_syntax_spans_without_overlap() {
        let red = Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
        let got = segments(10, &[(0..6, red)], &[(4..8, true)]);
        assert_eq!(got.iter().map(|(r, c, h)| (r.clone(), c.is_some(), *h)).collect::<Vec<_>>(), [(0..4, true, None), (4..6, true, Some(true)), (6..8, false, Some(true))]);
    }

    #[test]
    fn tabs_pad_to_four_columns_without_copying_the_padding() {
        let (shown, spans, pads) = tabbed("a\tb", vec![Span::new(2, 3)], false);
        assert_eq!(shown, "a\t  b");
        assert_eq!(pads, vec![2..4]);
        assert_eq!(&shown[spans[0].start as usize..spans[0].end as usize], "b");
    }
}
