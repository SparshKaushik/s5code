//! The rows a diff shows (file headers, hunk separators, lines), which files are collapsed, the
//! line selection and the reading position. Forked from `rune_kit::diff`'s state, whose row
//! heights are fixed at its own look: this one has web's (`@pierre/diffs` with T3's CSS).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use rune::{Key, KeyEvent, Modifier, Tx};
use rune_kit::diff::{FileDiff, Hunk, LineKind, Side};
use rune_kit::syntax::{Highlight, Highlighter, Paint};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DiffMode {
    /// Web's "stacked": one column, removed lines above the added ones.
    #[default]
    Unified,
    Split,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LineRef {
    pub side: Side,
    pub line: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LineRange {
    pub file: usize,
    pub start: LineRef,
    pub end: LineRef,
}

pub const LINE_H: f32 = 20.0;
pub const HEADER_H: f32 = 32.0;
pub const SEP_H: f32 = 24.0;
pub const NOTE_H: f32 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    File(u32),
    /// A file with no lines to show: binary, a pure rename, a mode change, an empty file.
    Note(u32),
    /// The unchanged lines folded away before hunk `h`.
    Sep { f: u32, h: u32 },
    /// Line `i` of hunk `h`; in split mode, one without a partner on the other side.
    Line { f: u32, h: u32, i: u32 },
    /// Split mode: removed line `del` beside added line `add`.
    Pair { f: u32, h: u32, del: u32, add: u32 },
}

impl Row {
    pub fn file(self) -> usize {
        match self {
            Row::File(f) | Row::Note(f) | Row::Sep { f, .. } | Row::Line { f, .. } | Row::Pair { f, .. } => f as usize,
        }
    }
}

/// Highlights of one side of a hunk, with the byte offset of each of its lines.
struct Chunk {
    starts: Vec<u32>,
    hl: Rc<[Highlight]>,
}

pub struct DiffState {
    files: Vec<FileDiff>,
    collapsed: Vec<bool>,
    mode: DiffMode,
    wrap: bool,
    selection: Option<LineRange>,
    selecting: bool,
    selection_at_press: Option<LineRange>,
    unselect: bool,
    shift: bool,
    highlighter: Highlighter,
    chunks: RefCell<HashMap<(u32, u32, Side), Rc<Chunk>>>,
    /// Per file, per hunk, per side: the index in the hunk of each line on that side.
    side_lines: Vec<Vec<[Vec<u32>; 2]>>,
    /// Per file, per hunk: unchanged lines folded away before it.
    hidden: Vec<Vec<u32>>,
    digits: Vec<u32>,
    columns: u32,
    pub rows: Vec<Row>,
    pub keys: Vec<String>,
    row_of: HashMap<(u32, Side, u32), u32>,
    file_rows: Vec<u32>,
    hunk_rows: Vec<u32>,
    /// Bumped whenever rows change, so memoised rows rebuild.
    pub rev: u64,
    /// Changes with `keys`, unique across states.
    pub keys_rev: u64,
    visible_rows: std::ops::Range<usize>,
    view_w: std::cell::Cell<f32>,
    visible: usize,
    nav: Option<u32>,
}

impl Default for DiffState {
    fn default() -> DiffState {
        DiffState::new(Vec::new())
    }
}

/// The first line number `h` covers on `side`, or the line after it when it covers none.
fn first(h: &Hunk, side: Side) -> u32 {
    let (start, count) = match side {
        Side::Old => (h.old_start, h.old_count),
        Side::New => (h.new_start, h.new_count),
    };
    if count == 0 { start.saturating_add(1) } else { start.max(1) }
}

fn end(h: &Hunk, side: Side) -> u32 {
    first(h, side).saturating_add(if side == Side::Old { h.old_count } else { h.new_count })
}

/// Columns `line` takes with tabs stopping every four.
fn columns(line: &str) -> u32 {
    line.chars().fold(0, |col, c| if c == '\t' { col + 4 - col % 4 } else { col + 1 })
}

impl DiffState {
    pub fn new(files: Vec<FileDiff>) -> DiffState {
        let mut s = DiffState {
            files: Vec::new(),
            collapsed: Vec::new(),
            mode: DiffMode::Unified,
            wrap: false,
            selection: None,
            selecting: false,
            selection_at_press: None,
            unselect: false,
            shift: false,
            highlighter: Highlighter::builtin(),
            chunks: RefCell::new(HashMap::new()),
            side_lines: Vec::new(),
            hidden: Vec::new(),
            digits: Vec::new(),
            columns: 0,
            rows: Vec::new(),
            keys: Vec::new(),
            row_of: HashMap::new(),
            file_rows: Vec::new(),
            hunk_rows: Vec::new(),
            rev: 0,
            keys_rev: 0,
            visible_rows: 0..0,
            view_w: std::cell::Cell::new(0.0),
            visible: 0,
            nav: None,
        };
        s.set_files(files, &[]);
        s
    }

    /// Show `files`, collapsed where their path is in `collapsed`.
    pub fn set_files(&mut self, files: Vec<FileDiff>, collapsed: &[String]) {
        self.collapsed = files.iter().map(|f| collapsed.iter().any(|p| p == f.path())).collect();
        self.side_lines = files
            .iter()
            .map(|f| {
                f.hunks
                    .iter()
                    .map(|h| {
                        let on = |side: Side| (0..h.lines.len() as u32).filter(|&i| h.lines[i as usize].number(side).is_some()).collect();
                        [on(Side::Old), on(Side::New)]
                    })
                    .collect()
            })
            .collect();
        self.hidden = files
            .iter()
            .map(|f| {
                let mut old = 1;
                f.hunks
                    .iter()
                    .map(|h| {
                        let n = first(h, Side::Old).saturating_sub(old);
                        old = end(h, Side::Old);
                        n
                    })
                    .collect()
            })
            .collect();
        self.digits = files
            .iter()
            .map(|f| f.hunks.last().map_or(0, |h| end(h, Side::Old).max(end(h, Side::New)).saturating_sub(1)))
            .map(|n| (n.max(1).ilog10() + 1).max(3))
            .collect();
        self.columns = files.iter().flat_map(|f| &f.hunks).flat_map(|h| &h.lines).map(|l| columns(&l.text)).max().unwrap_or(0);
        self.files = files;
        self.selection = None;
        self.chunks.borrow_mut().clear();
        self.visible = 0;
        self.visible_rows = 0..0;
        self.rebuild();
    }

    pub fn files(&self) -> &[FileDiff] {
        &self.files
    }

    pub fn file_index(&self, path: &str) -> Option<usize> {
        self.files.iter().position(|f| f.path() == path || f.old_path.as_deref() == Some(path))
    }

    pub fn mode(&self) -> DiffMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: DiffMode) {
        if self.mode != mode {
            self.mode = mode;
            self.rebuild();
        }
    }

    /// Long lines wrap as drawn: always in split, so both sides stay in view.
    pub fn wraps(&self) -> bool {
        self.wrap || self.mode == DiffMode::Split
    }

    pub fn set_wrap(&mut self, wrap: bool) {
        if self.wrap != wrap {
            self.wrap = wrap;
            self.rev += 1;
        }
    }

    pub fn collapsed(&self, file: usize) -> bool {
        self.collapsed.get(file).copied().unwrap_or(false)
    }

    /// Paths of the collapsed files, to carry across a reload.
    pub fn collapsed_paths(&self) -> Vec<String> {
        self.files.iter().zip(&self.collapsed).filter(|(_, c)| **c).map(|(f, _)| f.path().to_owned()).collect()
    }

    pub fn all_collapsed(&self) -> bool {
        !self.collapsed.is_empty() && self.collapsed.iter().all(|c| *c)
    }

    pub fn set_collapsed(&mut self, file: usize, collapsed: bool) {
        if let Some(c) = self.collapsed.get_mut(file).filter(|c| **c != collapsed) {
            *c = collapsed;
            self.rebuild();
        }
    }

    pub fn set_all_collapsed(&mut self, collapsed: bool) {
        self.collapsed.iter_mut().for_each(|c| *c = collapsed);
        self.rebuild();
    }

    /// Unchanged lines folded away before hunk `h` of `file`.
    pub fn hidden(&self, file: usize, h: usize) -> u32 {
        self.hidden.get(file).and_then(|v| v.get(h)).copied().unwrap_or(0)
    }

    pub fn selection(&self) -> Option<LineRange> {
        self.selection
    }

    pub fn set_selection(&mut self, sel: Option<LineRange>) {
        self.selection = sel;
    }

    /// The selected lines' text on the side the selection started on.
    pub fn selected_text(&self) -> Option<String> {
        let sel = self.selection?;
        let side = sel.start.side;
        let (a, b) = self.selected_rows(&sel)?;
        let out: Vec<&str> = (a..=b).filter_map(|r| self.row_text(self.rows[r as usize], side)).collect();
        Some(out.join("\n"))
    }

    fn selected_rows(&self, sel: &LineRange) -> Option<(u32, u32)> {
        let a = *self.row_of.get(&(sel.file as u32, sel.start.side, sel.start.line))?;
        let b = *self.row_of.get(&(sel.file as u32, sel.end.side, sel.end.line))?;
        Some((a.min(b), a.max(b)))
    }

    pub fn row_selected(&self, r: usize, side: Option<Side>) -> bool {
        let Some(sel) = self.selection else { return false };
        let Some((a, b)) = self.selected_rows(&sel) else { return false };
        if self.rows[r].file() != sel.file || !(a as usize..=b as usize).contains(&r) {
            return false;
        }
        side.is_none_or(|s| s == sel.start.side || s == sel.end.side)
    }

    pub fn press_line(&mut self, file: usize, at: LineRef) {
        self.selection_at_press = self.selection;
        let sel = match self.selection {
            Some(s) if self.shift && s.file == file => LineRange { end: at, ..s },
            _ => LineRange { file, start: at, end: at },
        };
        self.unselect = !self.shift && self.selection == Some(sel);
        self.selecting = true;
        self.selection = Some(sel);
    }

    pub fn drag_to(&mut self, file: usize, at: LineRef) {
        if let Some(s) = self.selection.filter(|s| self.selecting && s.file == file && s.end != at) {
            self.unselect = false;
            self.selection = Some(LineRange { end: at, ..s });
        }
    }

    pub fn release_line(&mut self) {
        self.selection_at_press = None;
        if std::mem::take(&mut self.unselect) {
            self.selection = None;
        }
        self.selecting = false;
    }

    pub fn cancel_line(&mut self) {
        if self.selecting {
            self.selection = self.selection_at_press.take();
        }
        self.selecting = false;
        self.unselect = false;
    }

    pub fn visible_file(&self) -> usize {
        self.visible
    }

    /// Scroll `file`'s header to the top.
    pub fn scroll_to_file(&mut self, file: usize, tx: &mut Tx) {
        let Some(&r) = self.file_rows.get(file) else { return };
        self.reveal_at_top(r, tx);
        self.visible = file;
        self.nav = Some(r);
    }

    pub fn next_hunk(&mut self, forward: bool, tx: &mut Tx) -> bool {
        let from = self.position();
        let target = if forward {
            self.hunk_rows.iter().copied().find(|&r| r > from)
        } else {
            self.hunk_rows.iter().rev().copied().find(|&r| r < from)
        };
        let Some(r) = target else { return false };
        self.reveal_below_header(r, tx);
        self.visible = self.rows[r as usize].file();
        self.nav = Some(r);
        true
    }

    pub fn next_file(&mut self, forward: bool, tx: &mut Tx) -> bool {
        let Some(&row) = self.rows.get(self.position() as usize) else { return false };
        let at = row.file();
        let target = if forward { at + 1 } else { at.wrapping_sub(1) };
        if target >= self.files.len() {
            return false;
        }
        self.scroll_to_file(target, tx);
        true
    }

    fn position(&self) -> u32 {
        let top = self.row_below_header();
        match (self.nav, top) {
            (Some(n), Some(t)) if t.abs_diff(n) <= 3 => n,
            (_, Some(t)) => t,
            (Some(n), None) => n,
            (None, None) => 0,
        }
    }

    fn row_below_header(&self) -> Option<u32> {
        if self.visible_rows.is_empty() {
            return None;
        }
        let mut r = self.visible_rows.start;
        if self.sticky().is_some() {
            let mut room = 0.0;
            while r + 1 < self.rows.len() && room < HEADER_H {
                room += self.estimate(r);
                r += 1;
            }
        }
        Some(r as u32)
    }

    /// `j`/`k` next and previous hunk, `]`/`[` next and previous file, Escape clears the line
    /// selection, Cmd-C copies it; Shift makes a click on a line number extend it.
    pub fn key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        match &ev.key {
            Key::Modifier { modifier: Modifier::ShiftLeft | Modifier::ShiftRight, .. } => {
                self.shift = ev.mods.shift;
                false
            }
            _ if tx.focused().is_some() && !ev.is_command('c') => false,
            Key::Text(t) if !ev.mods.cmd && !ev.mods.ctrl && !ev.mods.alt => match t.as_str() {
                "j" => self.next_hunk(true, tx),
                "k" => self.next_hunk(false, tx),
                "]" => self.next_file(true, tx),
                "[" => self.next_file(false, tx),
                _ => false,
            },
            Key::Escape if self.selection.is_some() => {
                self.selection = None;
                true
            }
            _ if ev.is_command('c') => match self.selected_text() {
                Some(text) => {
                    tx.copy(text);
                    true
                }
                None => false,
            },
            _ => false,
        }
    }

    pub fn set_viewport(&self, r: [f32; 4]) {
        self.view_w.set(r[2]);
    }

    pub fn view_width(&self) -> f32 {
        self.view_w.get()
    }

    /// The virtual list's visible rows changed: returns whether the sticky header changed.
    pub fn set_visible_rows(&mut self, rows: std::ops::Range<usize>) -> bool {
        let before = self.sticky();
        self.visible_rows = rows;
        if let Some(&r) = self.rows.get(self.visible_rows.start) {
            self.visible = r.file();
        }
        self.sticky() != before
    }

    pub fn row_from(&self, from: u32, mut y: f32) -> Option<u32> {
        let mut r = usize::try_from(from).ok()?.min(self.rows.len().checked_sub(1)?);
        while y < 0.0 && r > 0 {
            r -= 1;
            y += self.estimate(r);
        }
        while r + 1 < self.rows.len() {
            let h = self.estimate(r);
            if y < h {
                break;
            }
            y -= h;
            r += 1;
        }
        Some(r as u32)
    }

    /// The file whose header has scrolled above the top, so a copy of it sticks there.
    pub fn sticky(&self) -> Option<usize> {
        if self.files.is_empty() || self.visible_rows.is_empty() {
            return None;
        }
        let f = self.visible;
        let header = *self.file_rows.get(f)? as usize;
        (self.visible_rows.start > header).then_some(f)
    }

    fn reveal_at_top(&self, r: u32, tx: &mut Tx) {
        if let Some(last) = self.keys.last() {
            tx.scroll_into_view(last.clone());
        }
        tx.scroll_into_view(self.keys[r as usize].clone());
    }

    fn reveal_below_header(&self, r: u32, tx: &mut Tx) {
        let mut top = r as usize;
        let mut room = 0.0;
        while top > 0 && room < HEADER_H {
            if matches!(self.rows[top], Row::File(_)) {
                break;
            }
            top -= 1;
            room += self.estimate(top);
        }
        self.reveal_at_top(top as u32, tx);
    }

    fn estimate(&self, r: usize) -> f32 {
        match self.rows[r] {
            Row::File(_) => HEADER_H,
            Row::Note(_) => NOTE_H,
            Row::Sep { .. } => SEP_H,
            _ => LINE_H,
        }
    }

    /// Row `r`'s line text on `side`.
    pub fn row_text(&self, r: Row, side: Side) -> Option<&str> {
        let (f, h, i) = match r {
            Row::Line { f, h, i } => (f, h, i),
            Row::Pair { f, h, del, add } => (f, h, if side == Side::Old { del } else { add }),
            _ => return None,
        };
        let l = &self.files[f as usize].hunks[h as usize].lines[i as usize];
        l.number(side).map(|_| l.text.as_str())
    }

    /// Colour-only highlights of line `i` of hunk `h` of file `f`, read on `side`, relative to
    /// the line. Each side of a hunk is highlighted once as a whole.
    pub fn highlights(&self, f: usize, h: usize, i: u32, side: Side) -> Vec<(std::ops::Range<usize>, Paint)> {
        let file = &self.files[f];
        let lang = file.language();
        if self.highlighter.is_builtin() && !lang.is_some_and(rune_kit::syntax::known) {
            return Vec::new();
        }
        let lines = &self.side_lines[f][h][side as usize];
        let Ok(k) = lines.binary_search(&i) else { return Vec::new() };
        let key = (f as u32, h as u32, side);
        let chunk = self.chunks.borrow().get(&key).cloned();
        let chunk = chunk.unwrap_or_else(|| {
            let hunk = &file.hunks[h];
            let mut text = String::new();
            let mut starts = Vec::with_capacity(lines.len() + 1);
            for &j in lines {
                starts.push(text.len() as u32);
                text.push_str(&hunk.lines[j as usize].text);
                text.push('\n');
            }
            starts.push(text.len() as u32);
            let chunk = Rc::new(Chunk { starts, hl: self.highlighter.highlight(lang, &text) });
            let mut cache = self.chunks.borrow_mut();
            if cache.len() > 512 {
                cache.clear();
            }
            cache.insert(key, chunk.clone());
            chunk
        });
        let (a, b) = (chunk.starts[k] as usize, chunk.starts[k + 1] as usize - 1);
        let from = chunk.hl.partition_point(|(r, _)| r.end <= a);
        chunk.hl[from..]
            .iter()
            .take_while(|(r, _)| r.start < b)
            .map(|(r, p)| (r.start.max(a) - a..r.end.min(b) - a, *p))
            .filter(|(r, _)| !r.is_empty())
            .collect()
    }

    pub fn row_line(&self, r: Row, side: Side) -> Option<u32> {
        match r {
            Row::Line { f, h, i } => self.files[f as usize].hunks[h as usize].lines[i as usize].number(side),
            Row::Pair { f, h, del, add } => {
                let i = if side == Side::Old { del } else { add };
                self.files[f as usize].hunks[h as usize].lines[i as usize].number(side)
            }
            _ => None,
        }
    }

    fn rebuild(&mut self) {
        self.rev += 1;
        self.nav = None;
        let mut rows = Vec::new();
        self.file_rows.clear();
        self.hunk_rows.clear();
        for (f, file) in self.files.iter().enumerate() {
            let fu = f as u32;
            self.file_rows.push(rows.len() as u32);
            rows.push(Row::File(fu));
            if self.collapsed[f] {
                continue;
            }
            if file.hunks.is_empty() {
                rows.push(Row::Note(fu));
                continue;
            }
            for (h, hunk) in file.hunks.iter().enumerate() {
                let hu = h as u32;
                self.hunk_rows.push(rows.len() as u32);
                if self.hidden[f][h] > 0 {
                    rows.push(Row::Sep { f: fu, h: hu });
                }
                let lines = &hunk.lines;
                let mut i = 0;
                while i < lines.len() {
                    if self.mode == DiffMode::Split && lines[i].kind == LineKind::Removed {
                        let del = i;
                        while i < lines.len() && lines[i].kind == LineKind::Removed {
                            i += 1;
                        }
                        let add = i;
                        while i < lines.len() && lines[i].kind == LineKind::Added {
                            i += 1;
                        }
                        let (nd, na) = (add - del, i - add);
                        for k in 0..nd.max(na) {
                            rows.push(match (k < nd, k < na) {
                                (true, true) => Row::Pair { f: fu, h: hu, del: (del + k) as u32, add: (add + k) as u32 },
                                (true, false) => Row::Line { f: fu, h: hu, i: (del + k) as u32 },
                                _ => Row::Line { f: fu, h: hu, i: (add + k) as u32 },
                            });
                        }
                        continue;
                    }
                    rows.push(Row::Line { f: fu, h: hu, i: i as u32 });
                    i += 1;
                }
            }
        }
        let keys: Vec<String> = rows.iter().map(|&r| self.key_of(r)).collect();
        if !self.visible_rows.is_empty() {
            if let Some(top) = self.keys.get(self.visible_rows.start)
                && let Some(start) = keys.iter().position(|k| k == top)
            {
                let len = self.visible_rows.len();
                self.visible_rows = start..(start + len).min(rows.len());
            } else {
                self.visible_rows = 0..0;
            }
        }
        self.keys = keys;
        static KEYS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        self.keys_rev = KEYS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.row_of.clear();
        for (i, &r) in rows.iter().enumerate() {
            for side in [Side::Old, Side::New] {
                if let Some(n) = self.row_line(r, side) {
                    self.row_of.insert((r.file() as u32, side, n), i as u32);
                }
            }
        }
        self.rows = rows;
    }

    /// The same key for an unchanged or removed line in both modes, so a switch keeps rows.
    fn key_of(&self, r: Row) -> String {
        match r {
            Row::File(f) => format!("pd{f}"),
            Row::Note(f) => format!("pd{f}.note"),
            Row::Sep { f, h } => format!("pd{f}.s{h}"),
            Row::Line { f, h, i } => {
                let l = &self.files[f as usize].hunks[h as usize].lines[i as usize];
                match l.kind {
                    LineKind::Context => format!("pd{f}.c{}", l.old.unwrap_or(0)),
                    LineKind::Removed => format!("pd{f}.o{}", l.old.unwrap_or(0)),
                    LineKind::Added => format!("pd{f}.n{}", l.new.unwrap_or(0)),
                }
            }
            Row::Pair { f, h, del, .. } => format!("pd{f}.o{}", self.files[f as usize].hunks[h as usize].lines[del as usize].old.unwrap_or(0)),
        }
    }

    pub fn digits(&self, file: usize) -> u32 {
        self.digits.get(file).copied().unwrap_or(3)
    }

    pub fn columns(&self) -> u32 {
        self.columns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -10,4 +10,5 @@ fn main()\n keep\n-old one\n-old two\n+new one\n+new two\n+new three\n end\n@@ -40,2 +41,2 @@\n x\n-y\n+z\ndiff --git a/b.bin b/b.bin\nBinary files a/b.bin and b/b.bin differ\n";

    fn state() -> DiffState {
        DiffState::new(rune_kit::diff::parse(PATCH))
    }

    fn shown(s: &DiffState) -> Vec<String> {
        s.rows
            .iter()
            .map(|&r| match r {
                Row::File(f) => format!("file{f}"),
                Row::Note(_) => "note".into(),
                Row::Sep { f, h } => format!("sep{}", s.hidden(f as usize, h as usize)),
                _ => {
                    let n = |side| s.row_line(r, side).map_or("-".into(), |v: u32| v.to_string());
                    format!("{}|{}", n(Side::Old), n(Side::New))
                }
            })
            .collect()
    }

    #[test]
    fn separators_count_the_lines_folded_between_hunks() {
        let s = state();
        assert_eq!(
            shown(&s),
            ["file0", "sep9", "10|10", "11|-", "12|-", "-|11", "-|12", "-|13", "13|14", "sep26", "40|41", "41|-", "-|42", "file1", "note"]
        );
    }

    #[test]
    fn split_pairs_changes_and_keeps_keys() {
        let mut s = state();
        let removed = s.keys[3].clone();
        s.set_mode(DiffMode::Split);
        assert_eq!(shown(&s)[..7], ["file0", "sep9", "10|10", "11|11", "12|12", "-|13", "13|14"]);
        assert_eq!(s.keys[3], removed);
    }

    #[test]
    fn collapsed_files_follow_their_path_across_a_reload() {
        let mut s = state();
        s.set_collapsed(0, true);
        assert_eq!(shown(&s), ["file0", "file1", "note"]);
        let kept = s.collapsed_paths();
        assert_eq!(kept, ["a.rs"]);
        // The reload lists the files the other way round: the collapsed one is still a.rs.
        let mut files = rune_kit::diff::parse(PATCH);
        files.reverse();
        s.set_files(files, &kept);
        assert_eq!(shown(&s), ["file0", "note", "file1"]);
        assert!(!s.all_collapsed());
        s.set_all_collapsed(true);
        assert!(s.all_collapsed());
        assert_eq!(shown(&s), ["file0", "file1"]);
    }

    #[test]
    fn line_selection_copies_one_side() {
        let mut s = state();
        s.press_line(0, LineRef { side: Side::New, line: 10 });
        s.release_line();
        s.shift = true;
        s.press_line(0, LineRef { side: Side::New, line: 12 });
        s.release_line();
        assert_eq!(s.selected_text().as_deref(), Some("keep\nnew one\nnew two"));
        s.set_selection(Some(LineRange { file: 0, start: LineRef { side: Side::Old, line: 12 }, end: LineRef { side: Side::Old, line: 11 } }));
        assert_eq!(s.selected_text().as_deref(), Some("old one\nold two"));
    }

    #[test]
    fn the_visible_file_sticks_once_its_header_scrolls_away() {
        let mut s = state();
        assert!(!s.set_visible_rows(0..6));
        assert_eq!(s.sticky(), None);
        assert!(s.set_visible_rows(2..8));
        assert_eq!((s.visible_file(), s.sticky()), (0, Some(0)));
        assert!(s.set_visible_rows(13..15));
        assert_eq!((s.visible_file(), s.sticky()), (1, None));
    }
}
