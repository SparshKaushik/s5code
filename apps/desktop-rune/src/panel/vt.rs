//! The terminal emulator behind every terminal surface: alacritty's grid model and the `vte`
//! parser, plus xterm's key, mouse and paste encodings. Rendering-agnostic so it tests headless.

use std::sync::{Arc, Mutex};

use alacritty_terminal::{
    Term,
    event::{Event, EventListener, WindowSize},
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point, Side},
    selection::{Selection, SelectionType},
    term::{Config, TermMode, cell::Flags},
    vte::ansi::{Color, CursorShape, CursorStyle, NamedColor, Processor, Rgb, StdSyncHandler},
};
use rune::{Key, KeyEvent};

pub mod links;

pub type Rgb8 = [u8; 3];

/// The theme colours the web terminal draws with (`terminalThemeFromApp`).
#[derive(Clone, Copy, PartialEq)]
pub struct Palette {
    pub foreground: Rgb8,
    pub background: Rgb8,
    pub ansi: [Rgb8; 16],
}

pub const DARK_ANSI: [Rgb8; 16] = [
    [0x00, 0x00, 0x00],
    [0xcd, 0x31, 0x31],
    [0x0d, 0xbc, 0x79],
    [0xe5, 0xe5, 0x10],
    [0x24, 0x72, 0xc8],
    [0xbc, 0x3f, 0xbc],
    [0x11, 0xa8, 0xcd],
    [0xe5, 0xe5, 0xe5],
    [0x66, 0x66, 0x66],
    [0xf1, 0x4c, 0x4c],
    [0x23, 0xd1, 0x8b],
    [0xf5, 0xf5, 0x43],
    [0x3b, 0x8e, 0xea],
    [0xd6, 0x70, 0xd6],
    [0x29, 0xb8, 0xdb],
    [0xe5, 0xe5, 0xe5],
];
pub const LIGHT_ANSI: [Rgb8; 16] = [
    [0x00, 0x00, 0x00],
    [0xcd, 0x31, 0x31],
    [0x10, 0x7c, 0x10],
    [0x94, 0x98, 0x00],
    [0x04, 0x51, 0xa5],
    [0xbc, 0x05, 0xbc],
    [0x05, 0x98, 0xbc],
    [0x55, 0x55, 0x55],
    [0x66, 0x66, 0x66],
    [0xcd, 0x31, 0x31],
    [0x14, 0xce, 0x14],
    [0xb5, 0xba, 0x00],
    [0x04, 0x51, 0xa5],
    [0xbc, 0x05, 0xbc],
    [0x05, 0x98, 0xbc],
    [0xa5, 0xa5, 0xa5],
];

#[derive(Clone)]
struct Listener(Arc<Mutex<Vec<Event>>>);
impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        if matches!(
            event,
            Event::PtyWrite(_) | Event::TextAreaSizeRequest(_) | Event::ColorRequest(..)
        ) {
            self.0.lock().unwrap().push(event);
        }
    }
}

struct Size(usize, usize);
impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.1
    }
    fn screen_lines(&self) -> usize {
        self.1
    }
    fn columns(&self) -> usize {
        self.0
    }
}

/// Lines of scrollback, as the web terminal keeps.
const SCROLLBACK: usize = 5000;

pub struct Vt {
    term: Term<Listener>,
    parser: Processor<StdSyncHandler>,
    events: Arc<Mutex<Vec<Event>>>,
    cols: usize,
    rows: usize,
    /// Fractional wheel travel, in lines, not yet applied.
    wheel: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorKind {
    Block,
    Beam,
    Underline,
    Hollow,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub col: usize,
    /// Width in cells.
    pub cells: usize,
    pub text: String,
    pub fg: Rgb8,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    /// Non-default backgrounds: (column, cells, colour).
    pub fills: Vec<(usize, usize, Rgb8)>,
    pub runs: Vec<Run>,
    /// Selected spans, drawn over the backgrounds: (column, cells).
    pub selected: Vec<(usize, usize)>,
}

/// How a press starts a selection: by cell, or by word and line on double and triple clicks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectBy {
    Cell,
    Word,
    Line,
}

/// A link under the pointer: its text and the screen rows it covers, as `(row, first col, last
/// col)` spans of the visible grid.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub kind: links::Kind,
    pub text: String,
    pub spans: Vec<(usize, usize, usize)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cursor {
    pub col: usize,
    pub row: usize,
    pub kind: CursorKind,
    pub blinking: bool,
    /// The glyph under the cursor, redrawn in the background colour over a block cursor.
    pub glyph: Option<Run>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    /// Pointer travel with no button (mode 1003) or with the last pressed one.
    None,
    WheelUp,
    WheelDown,
}

/// What a wheel tick does: reported to the program, sent as arrow keys on the alternate screen,
/// or scrolled through local scrollback.
#[derive(Debug, PartialEq)]
pub enum Wheel {
    Send(String),
    Scrolled(bool),
}

impl Vt {
    pub fn new(cols: usize, rows: usize) -> Self {
        let events = Arc::new(Mutex::new(Vec::new()));
        let cols = cols.max(2);
        let rows = rows.max(1);
        let config = Config {
            scrolling_history: SCROLLBACK,
            default_cursor_style: CursorStyle {
                shape: CursorShape::Block,
                blinking: true,
            },
            ..Config::default()
        };
        Self {
            term: Term::new(config, &Size(cols, rows), Listener(events.clone())),
            parser: Processor::new(),
            events,
            cols,
            rows,
            wheel: 0.,
        }
    }

    pub fn size(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let (cols, rows) = (cols.max(2), rows.max(1));
        if (cols, rows) != (self.cols, self.rows) {
            self.cols = cols;
            self.rows = rows;
            self.term.resize(Size(cols, rows));
        }
    }

    pub fn feed(&mut self, data: &str) {
        self.parser.advance(&mut self.term, data.as_bytes());
    }

    /// Output held by a synchronized update (mode 2026) still waiting for its end.
    pub fn sync_pending(&self) -> bool {
        self.parser.sync_bytes_count() > 0
    }

    /// Applies a synchronized update whose end never came: when its timeout lapsed, or
    /// unconditionally after replaying history. Returns whether anything was applied.
    pub fn flush_sync(&mut self, force: bool) -> bool {
        use alacritty_terminal::vte::ansi::Timeout;
        if !self.sync_pending() || (!force && self.parser.sync_timeout().pending_timeout()) {
            return false;
        }
        self.parser.stop_sync(&mut self.term);
        true
    }

    /// Replies the program asked for (device status, attributes, colours, text area size), to
    /// write back to the PTY in order.
    pub fn take_replies(&mut self, palette: &Palette) -> Vec<String> {
        let events = std::mem::take(&mut *self.events.lock().unwrap());
        events
            .into_iter()
            .filter_map(|event| match event {
                Event::PtyWrite(text) => Some(text),
                Event::TextAreaSizeRequest(reply) => Some(reply(WindowSize {
                    num_lines: self.rows as u16,
                    num_cols: self.cols as u16,
                    cell_width: 7,
                    cell_height: 16,
                })),
                Event::ColorRequest(index, reply) => {
                    let [r, g, b] = self.color(index, palette);
                    Some(reply(Rgb { r, g, b }))
                }
                _ => None,
            })
            .collect()
    }

    pub fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    /// Whether the program wants a blinking cursor (DECSCUSR, mode 12) that is shown.
    pub fn cursor_blinks(&self) -> bool {
        self.term.cursor_style().blinking && self.term.mode().contains(TermMode::SHOW_CURSOR)
    }

    pub fn scrolled_back(&self) -> bool {
        self.term.grid().display_offset() > 0
    }

    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// Shift+PageUp/PageDown page through history and Shift+Mod+Home/End jump to its ends, on
    /// the normal screen only (the web surface's `pageHistory` and `jumpHistory`).
    pub fn history_key(&mut self, e: &KeyEvent, mac: bool) -> bool {
        let m = e.mods;
        let primary = if mac { m.cmd } else { m.ctrl };
        let page = m.shift && !m.alt && !m.ctrl && !m.cmd;
        let jump = m.shift && !m.alt && primary;
        let scroll = match e.key {
            Key::PageUp if page => Scroll::PageUp,
            Key::PageDown if page => Scroll::PageDown,
            Key::Home if jump => Scroll::Top,
            Key::End if jump => Scroll::Bottom,
            _ => return false,
        };
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            return false;
        }
        self.term.scroll_display(scroll);
        true
    }

    /// The program's pending output is already applied; only the scrollback view moves.
    pub fn scroll_lines(&mut self, lines: i32) -> bool {
        let before = self.term.grid().display_offset();
        self.term.scroll_display(Scroll::Delta(lines));
        before != self.term.grid().display_offset()
    }

    fn color(&self, index: usize, palette: &Palette) -> Rgb8 {
        if let Some(Rgb { r, g, b }) = self.term.colors()[index] {
            return [r, g, b];
        }
        match index {
            0..16 => palette.ansi[index],
            16..232 => {
                let n = index - 16;
                let level = |v: usize| if v == 0 { 0 } else { (55 + 40 * v) as u8 };
                [level(n / 36), level(n / 6 % 6), level(n % 6)]
            }
            232..256 => {
                let v = (8 + 10 * (index - 232)) as u8;
                [v, v, v]
            }
            i if i == NamedColor::Background as usize => palette.background,
            _ => palette.foreground,
        }
    }

    fn resolve(&self, color: Color, palette: &Palette) -> Rgb8 {
        match color {
            Color::Spec(Rgb { r, g, b }) => [r, g, b],
            Color::Indexed(i) => self.color(i as usize, palette),
            Color::Named(named) => {
                let i = named as usize;
                if i < 16 {
                    self.color(i, palette)
                } else if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize)
                    .contains(&i)
                {
                    let base = self.color(i - NamedColor::DimBlack as usize, palette);
                    mix(base, palette.background, 0.66)
                } else if named == NamedColor::Background {
                    self.color(i, palette)
                } else {
                    self.color(NamedColor::Foreground as usize, palette)
                }
            }
        }
    }

    /// The visible screen as paint runs, top row first, honouring the scrollback offset.
    pub fn frame(&self, palette: &Palette) -> (Vec<Row>, Option<Cursor>) {
        let content = self.term.renderable_content();
        let offset = content.display_offset as i32;
        let background = self.color(NamedColor::Background as usize, palette);
        let mut rows = vec![Row::default(); self.rows];
        // Style of the run being built: (fg, bold, italic, underline, strike).
        let mut last: Option<(usize, (Rgb8, bool, bool, bool, bool))> = None;
        let mut glyph_at = None;
        let cursor_point = content.cursor.point;
        let selection = content.selection;
        let cursor_shape = content.cursor.shape;
        for indexed in content.display_iter {
            let row = (indexed.point.line.0 + offset) as usize;
            if row >= self.rows {
                continue;
            }
            let col = indexed.point.column.0;
            let cell = indexed.cell;
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let (mut fg, mut bg) = (
                self.resolve(cell.fg, palette),
                self.resolve(cell.bg, palette),
            );
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.flags.contains(Flags::DIM) {
                fg = mix(fg, bg, 0.66);
            }
            let cells = if cell.flags.contains(Flags::WIDE_CHAR) {
                2
            } else {
                1
            };
            let out = &mut rows[row];
            if selection.is_some_and(|sel| sel.contains_cell(&indexed, cursor_point, cursor_shape)) {
                match out.selected.last_mut() {
                    Some((c, n)) if *c + *n == col => *n += cells,
                    _ => out.selected.push((col, cells)),
                }
            }
            if bg != background {
                match out.fills.last_mut() {
                    Some((c, n, color)) if *c + *n == col && *color == bg => *n += cells,
                    _ => out.fills.push((col, cells, bg)),
                }
            }
            let mut ch: String = if cell.flags.contains(Flags::HIDDEN) {
                " ".into()
            } else {
                cell.c.to_string()
            };
            if let Some(extra) = cell.zerowidth() {
                ch.extend(extra);
            }
            let style = (
                fg,
                cell.flags.contains(Flags::BOLD),
                cell.flags.contains(Flags::ITALIC),
                cell.flags.intersects(Flags::ALL_UNDERLINES),
                cell.flags.contains(Flags::STRIKEOUT),
            );
            if indexed.point == cursor_point {
                glyph_at = Some(Run {
                    col,
                    cells,
                    text: ch.clone(),
                    fg: bg,
                    bold: style.1,
                    italic: style.2,
                    underline: false,
                    strike: false,
                });
            }
            // A wide glyph stands alone so the cells after it stay on the grid.
            let joins = cells == 1
                && last.is_some_and(|(r, s)| r == row && s == style)
                && out
                    .runs
                    .last()
                    .is_some_and(|run| run.col + run.cells == col);
            if joins {
                let run = out.runs.last_mut().unwrap();
                run.text.push_str(&ch);
                run.cells += 1;
            } else {
                out.runs.push(Run {
                    col,
                    cells,
                    text: ch,
                    fg: style.0,
                    bold: style.1,
                    italic: style.2,
                    underline: style.3,
                    strike: style.4,
                });
            }
            last = Some((row, if cells == 1 { style } else { (fg, true, true, true, true) }));
        }
        for row in &mut rows {
            row.runs.retain_mut(|run| {
                if run.underline || run.strike {
                    return true;
                }
                let trimmed = run.text.trim_end_matches(' ').len();
                let dropped = run.text.len() - trimmed;
                run.text.truncate(trimmed);
                run.cells -= dropped;
                !run.text.trim_start_matches(' ').is_empty()
            });
        }
        let style = self.term.cursor_style();
        let row = cursor_point.line.0 + offset;
        let cursor = (content.cursor.shape != CursorShape::Hidden
            && row >= 0
            && (row as usize) < self.rows)
            .then(|| Cursor {
                col: cursor_point.column.0.min(self.cols - 1),
                row: row as usize,
                kind: match content.cursor.shape {
                    CursorShape::Beam => CursorKind::Beam,
                    CursorShape::Underline => CursorKind::Underline,
                    CursorShape::HollowBlock => CursorKind::Hollow,
                    _ => CursorKind::Block,
                },
                blinking: style.blinking,
                glyph: glyph_at.filter(|g| !g.text.trim().is_empty()),
            });
        (rows, cursor)
    }

    fn grid_point(&self, col: usize, row: usize) -> Point {
        let offset = self.term.grid().display_offset() as i32;
        Point::new(
            Line(row.min(self.rows - 1) as i32 - offset),
            Column(col.min(self.cols - 1)),
        )
    }

    /// Starts a selection at a visible cell; `right` is the cell's right half.
    pub fn select_start(&mut self, col: usize, row: usize, right: bool, by: SelectBy) {
        let ty = match by {
            SelectBy::Cell => SelectionType::Simple,
            SelectBy::Word => SelectionType::Semantic,
            SelectBy::Line => SelectionType::Lines,
        };
        let side = if right { Side::Right } else { Side::Left };
        self.term.selection = Some(Selection::new(ty, self.grid_point(col, row), side));
    }

    pub fn select_to(&mut self, col: usize, row: usize, right: bool) {
        let point = self.grid_point(col, row);
        let side = if right { Side::Right } else { Side::Left };
        if let Some(selection) = self.term.selection.as_mut() {
            selection.update(point, side);
        }
    }

    /// Selects the scrollback and the screen (`selectAll`).
    pub fn select_all(&mut self) {
        let grid = self.term.grid();
        let start = Point::new(grid.topmost_line(), Column(0));
        let end = Point::new(grid.bottommost_line(), Column(self.cols - 1));
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        self.term.selection = Some(selection);
    }

    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    /// The selected text without trailing newlines, or None when nothing is selected.
    pub fn selection_text(&self) -> Option<String> {
        let text = self.term.selection_to_string()?;
        let text = text.trim_end_matches('\n');
        (!text.is_empty()).then(|| text.to_string())
    }

    /// The selection's 1-based rows counted from the top of the scrollback, as xterm's
    /// `getSelectionPosition` gives them to "Add to chat".
    pub fn selection_lines(&self) -> Option<(usize, usize)> {
        let range = self.term.selection.as_ref()?.to_range(&self.term)?;
        let top = self.term.grid().topmost_line().0;
        let start = (range.start.line.0 - top) as usize + 1;
        Some((start, ((range.end.line.0 - top) as usize + 1).max(start)))
    }

    /// The link under a visible cell: an OSC 8 hyperlink, else a URL or path in the text of
    /// the (soft-wrapped) line around it.
    pub fn link_at(&self, col: usize, row: usize) -> Option<Link> {
        let grid = self.term.grid();
        let offset = grid.display_offset() as i32;
        let target = self.grid_point(col, row);
        let visible = |point: Point| {
            let row = point.line.0 + offset;
            (row >= 0 && (row as usize) < self.rows).then_some(row as usize)
        };
        let spans_between = |start: Point, end: Point| {
            (start.line.0..=end.line.0)
                .filter_map(|line| {
                    let row = visible(Point::new(Line(line), Column(0)))?;
                    let first = if line == start.line.0 { start.column.0 } else { 0 };
                    let last = if line == end.line.0 { end.column.0 } else { self.cols - 1 };
                    Some((row, first, last))
                })
                .collect::<Vec<_>>()
        };
        let wraps = |line: i32| {
            grid[Line(line)][Column(self.cols - 1)].flags.contains(Flags::WRAPLINE)
        };
        if let Some(link) = grid[target].hyperlink() {
            let uri = link.uri().to_string();
            let same = |point: Point| grid[point].hyperlink().is_some_and(|l| l.uri() == uri);
            let (mut start, mut end) = (target, target);
            loop {
                let previous = if start.column.0 > 0 {
                    Point::new(start.line, Column(start.column.0 - 1))
                } else if start.line > grid.topmost_line() && wraps(start.line.0 - 1) {
                    Point::new(Line(start.line.0 - 1), Column(self.cols - 1))
                } else {
                    break;
                };
                if !same(previous) {
                    break;
                }
                start = previous;
            }
            loop {
                let next = if end.column.0 + 1 < self.cols {
                    Point::new(end.line, Column(end.column.0 + 1))
                } else if end.line < grid.bottommost_line() && wraps(end.line.0) {
                    Point::new(Line(end.line.0 + 1), Column(0))
                } else {
                    break;
                };
                if !same(next) {
                    break;
                }
                end = next;
            }
            let kind = if links::is_url(&uri) { links::Kind::Url } else { links::Kind::Path };
            return Some(Link { kind, text: uri, spans: spans_between(start, end) });
        }
        // The logical line: back to its first row, then on while rows soft-wrap.
        let mut first = target.line.0;
        while first > grid.topmost_line().0 && wraps(first - 1) {
            first -= 1;
        }
        let mut text = String::new();
        let mut points = Vec::new();
        let mut line = first;
        loop {
            for col in 0..self.cols {
                let cell = &grid[Line(line)][Column(col)];
                if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                    continue;
                }
                text.push(cell.c);
                points.push(Point::new(Line(line), Column(col)));
            }
            if line >= grid.bottommost_line().0 || !wraps(line) {
                break;
            }
            line += 1;
        }
        let at = points.iter().position(|p| *p == target)?;
        let found = links::extract(&text).into_iter().find(|m| m.start <= at && at < m.end)?;
        let end = points[found.end - 1];
        let end = if grid[end].flags.contains(Flags::WIDE_CHAR) {
            Point::new(end.line, Column((end.column.0 + 1).min(self.cols - 1)))
        } else {
            end
        };
        Some(Link {
            kind: found.kind,
            text: found.text,
            spans: spans_between(points[found.start], end),
        })
    }

    /// Plain text of the visible screen, trailing blanks trimmed; for tests and accessibility.
    pub fn text(&self) -> String {
        let grid = self.term.grid();
        let offset = grid.display_offset() as i32;
        (0..self.rows as i32)
            .map(|row| {
                let line = Line(row - offset);
                (0..self.cols)
                    .map(|col| &grid[Point::new(line, Column(col))])
                    .filter(|cell| !cell.flags.contains(Flags::WIDE_CHAR_SPACER))
                    .map(|cell| cell.c)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string()
    }

    pub fn cursor_point(&self) -> (usize, usize) {
        let point = self.term.grid().cursor.point;
        (point.column.0, point.line.0 as usize)
    }

    /// Bracketed paste wraps the text so shells and editors don't run it line by line.
    pub fn paste(&self, text: &str) -> String {
        if self.term.mode().contains(TermMode::BRACKETED_PASTE) {
            let clean = text.replace("\x1b[201~", "");
            format!("\x1b[200~{clean}\x1b[201~")
        } else {
            text.replace("\r\n", "\r").replace('\n', "\r")
        }
    }

    /// xterm's encoding of a key, or None when the key is the app's (Cmd shortcuts, copy/paste).
    pub fn key(&self, e: &KeyEvent) -> Option<String> {
        encode_key(e, *self.term.mode(), cfg!(target_os = "macos"))
    }

    /// Whether the program reads the pointer; Shift bypasses it, as in the web terminal.
    pub fn reports_mouse(&self, shift: bool) -> bool {
        !shift && self.term.mode().intersects(TermMode::MOUSE_MODE)
    }

    /// A pointer event at a cell, encoded for the program's mouse mode. `pressed` is false for
    /// a release; motion reports only under the drag (1002) or any-motion (1003) modes.
    pub fn mouse(
        &self,
        button: MouseButton,
        pressed: bool,
        motion: bool,
        col: usize,
        row: usize,
        mods: rune::Mods,
    ) -> Option<String> {
        encode_mouse(*self.term.mode(), button, pressed, motion, col, row, mods)
    }

    /// Wheel travel in lines (positive scrolls back towards older output).
    pub fn wheel(&mut self, lines: f32, col: usize, row: usize, mods: rune::Mods) -> Wheel {
        self.wheel += lines;
        let steps = self.wheel.trunc() as i32;
        self.wheel -= steps as f32;
        if steps == 0 {
            return Wheel::Scrolled(false);
        }
        let mode = *self.term.mode();
        if !mods.shift && mode.intersects(TermMode::MOUSE_MODE) {
            let button = if steps > 0 {
                MouseButton::WheelUp
            } else {
                MouseButton::WheelDown
            };
            let one = encode_mouse(mode, button, true, false, col, row, mods).unwrap_or_default();
            return Wheel::Send(one.repeat(steps.unsigned_abs() as usize));
        }
        if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
            let app = mode.contains(TermMode::APP_CURSOR);
            let arrow = match (steps > 0, app) {
                (true, true) => "\x1bOA",
                (true, false) => "\x1b[A",
                (false, true) => "\x1bOB",
                (false, false) => "\x1b[B",
            };
            return Wheel::Send(arrow.repeat(steps.unsigned_abs() as usize));
        }
        Wheel::Scrolled(self.scroll_lines(steps))
    }
}

fn mix(a: Rgb8, b: Rgb8, t: f32) -> Rgb8 {
    std::array::from_fn(|i| (a[i] as f32 * t + b[i] as f32 * (1. - t)).round() as u8)
}

fn modifier_param(shift: bool, alt: bool, ctrl: bool) -> u8 {
    1 + shift as u8 + 2 * alt as u8 + 4 * ctrl as u8
}

pub(crate) fn encode_key(e: &KeyEvent, mode: TermMode, mac: bool) -> Option<String> {
    let m = e.mods;
    // Off macOS the primary modifier is Ctrl itself; on macOS Cmd is the app's.
    let ctrl = m.ctrl;
    if mac && m.cmd && !m.ctrl {
        return None;
    }
    let param = modifier_param(m.shift, m.alt, ctrl);
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    let cursor = |letter: char| {
        Some(if param > 1 {
            format!("\x1b[1;{param}{letter}")
        } else if app_cursor {
            format!("\x1bO{letter}")
        } else {
            format!("\x1b[{letter}")
        })
    };
    let tilde = |code: u8| {
        Some(if param > 1 {
            format!("\x1b[{code};{param}~")
        } else {
            format!("\x1b[{code}~")
        })
    };
    let alt = |s: &str| {
        if m.alt && !mac {
            format!("\x1b{s}")
        } else {
            s.to_string()
        }
    };
    match &e.key {
        Key::Up => cursor('A'),
        Key::Down => cursor('B'),
        Key::Right => cursor('C'),
        Key::Left => cursor('D'),
        Key::Home => cursor('H'),
        Key::End => cursor('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::Function(n @ 1..=4) => {
            let letter = (b'P' + n - 1) as char;
            Some(if param > 1 {
                format!("\x1b[1;{param}{letter}")
            } else {
                format!("\x1bO{letter}")
            })
        }
        Key::Function(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][(*n - 5) as usize]),
        Key::Enter => Some(alt("\r")),
        Key::Tab if m.shift => Some("\x1b[Z".into()),
        Key::Tab => Some(alt("\t")),
        Key::Escape => Some(alt("\x1b")),
        Key::Backspace if ctrl => Some(alt("\x08")),
        Key::Backspace => Some(alt("\x7f")),
        Key::Text(text) if ctrl => text.chars().next().and_then(control).map(|c| alt(&c)),
        Key::Text(text) => Some(alt(text)),
        // Ctrl+Shift+letter (copy, paste) stays the app's off macOS.
        Key::Command(c) if ctrl && !(m.shift && c.is_ascii_alphabetic()) => {
            control(*c).map(|c| alt(&c))
        }
        _ => None,
    }
}

fn control(c: char) -> Option<String> {
    let byte = match c.to_ascii_lowercase() {
        c @ 'a'..='z' => c as u8 - b'a' + 1,
        '@' | ' ' | '2' => 0,
        '[' | '3' => 27,
        '\\' | '4' => 28,
        ']' | '5' => 29,
        '^' | '6' => 30,
        '_' | '-' | '7' => 31,
        '8' | '?' => 127,
        _ => return None,
    };
    Some((byte as char).to_string())
}

fn encode_mouse(
    mode: TermMode,
    button: MouseButton,
    pressed: bool,
    motion: bool,
    col: usize,
    row: usize,
    mods: rune::Mods,
) -> Option<String> {
    if !mode.intersects(TermMode::MOUSE_MODE) {
        return None;
    }
    if motion {
        let any = mode.contains(TermMode::MOUSE_MOTION);
        let drag = mode.contains(TermMode::MOUSE_DRAG) && button != MouseButton::None;
        if !any && !drag {
            return None;
        }
    }
    let mut code: u32 = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::None => 3,
        MouseButton::WheelUp => 64,
        MouseButton::WheelDown => 65,
    };
    if motion {
        code += 32;
    }
    code += 4 * mods.shift as u32 + 8 * mods.alt as u32 + 16 * mods.ctrl as u32;
    let (x, y) = (col + 1, row + 1);
    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if pressed || motion { 'M' } else { 'm' };
        return Some(format!("\x1b[<{code};{x};{y}{end}"));
    }
    // X10 encoding has no release button; releases report button 3.
    if !pressed && !motion && code < 64 {
        code = (code & !3) | 3;
    }
    let byte = |v: usize| char::from_u32((v.min(223) + 32) as u32).unwrap();
    let mut out = String::from("\x1b[M");
    out.push(char::from_u32(code + 32)?);
    out.push(byte(x));
    out.push(byte(y));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        Palette {
            foreground: [200, 200, 200],
            background: [10, 10, 10],
            ansi: DARK_ANSI,
        }
    }

    fn key(key: Key, f: impl FnOnce(&mut rune::Mods)) -> KeyEvent {
        let mut e = KeyEvent::from(key);
        f(&mut e.mods);
        e
    }

    #[test]
    fn selection_by_cell_word_and_line_reads_back_and_paints() {
        let mut vt = Vt::new(20, 3);
        vt.feed("hello brave world\r\nsecond line");
        vt.select_start(0, 0, false, SelectBy::Cell);
        vt.select_to(4, 0, true);
        assert_eq!(vt.selection_text().as_deref(), Some("hello"));
        let (rows, _) = vt.frame(&palette());
        assert_eq!(rows[0].selected, [(0, 5)]);
        vt.select_start(7, 0, false, SelectBy::Word);
        assert_eq!(vt.selection_text().as_deref(), Some("brave"));
        vt.select_start(2, 1, false, SelectBy::Line);
        assert_eq!(vt.selection_text().as_deref(), Some("second line"));
        vt.select_all();
        assert_eq!(vt.selection_text().as_deref(), Some("hello brave world\nsecond line"));
        vt.clear_selection();
        assert_eq!(vt.selection_text(), None);
        // A press without travel selects nothing.
        vt.select_start(3, 0, false, SelectBy::Cell);
        assert_eq!(vt.selection_text(), None);
    }

    #[test]
    fn selection_lines_count_from_the_top_of_the_scrollback() {
        let mut vt = Vt::new(20, 3);
        vt.feed("a\r\nb\r\nc\r\nd\r\ne");
        assert_eq!(vt.selection_lines(), None);
        vt.select_start(0, 0, false, SelectBy::Cell);
        vt.select_to(0, 1, true);
        assert_eq!(vt.selection_text().as_deref(), Some("c\nd"));
        assert_eq!(vt.selection_lines(), Some((3, 4)));
        vt.select_start(0, 2, false, SelectBy::Line);
        assert_eq!(vt.selection_lines(), Some((5, 5)));
    }

    #[test]
    fn links_span_soft_wrapped_rows_and_osc8_hyperlinks() {
        let mut vt = Vt::new(12, 4);
        vt.feed("see https://ex.com/ab done");
        let link = vt.link_at(1, 1).expect("the wrapped URL continues on row 1");
        assert_eq!(link.text, "https://ex.com/ab");
        assert_eq!(link.kind, links::Kind::Url);
        assert_eq!(link.spans, [(0, 4, 11), (1, 0, 8)]);
        assert_eq!(vt.link_at(0, 0), None);
        let mut vt = Vt::new(20, 2);
        vt.feed("\x1b]8;;https://t3.codes\x1b\\docs\x1b]8;;\x1b\\ here");
        let link = vt.link_at(2, 0).unwrap();
        assert_eq!((link.text.as_str(), link.spans.as_slice()), ("https://t3.codes", &[(0, 0, 3)][..]));
        assert_eq!(vt.link_at(6, 0), None);
    }

    #[test]
    fn alternate_screen_restores_the_shell_screen() {
        let mut vt = Vt::new(20, 4);
        vt.feed("$ vim\r\n");
        vt.feed("\x1b[?1049h\x1b[H\x1b[2Jeditor");
        assert_eq!(vt.text(), "editor");
        assert!(vt.mode().contains(TermMode::ALT_SCREEN));
        vt.feed("\x1b[?1049l");
        assert_eq!(vt.text(), "$ vim");
        assert!(!vt.mode().contains(TermMode::ALT_SCREEN));
    }

    #[test]
    fn cursor_addressing_and_erase() {
        let mut vt = Vt::new(10, 3);
        vt.feed("\x1b[2;4Hab\x1b[1;1Hxyz\x1b[3;1Hgone\x1b[2K");
        assert_eq!(vt.text(), "xyz\n   ab");
        vt.feed("\x1b[2;5H\x1b[K");
        assert_eq!(vt.text(), "xyz\n   a");
        assert_eq!(vt.cursor_point(), (4, 1));
    }

    #[test]
    fn scroll_region_scrolls_only_its_lines() {
        let mut vt = Vt::new(10, 4);
        vt.feed("top\r\na\r\nb\r\nbottom");
        // Region rows 2-3; a newline at its bottom scrolls just those two.
        vt.feed("\x1b[2;3r\x1b[3;1H\nc");
        assert_eq!(vt.text(), "top\nb\nc\nbottom");
    }

    #[test]
    fn indexed_and_true_colour() {
        let mut vt = Vt::new(10, 1);
        vt.feed("\x1b[38;5;196mr\x1b[38;2;1;2;3mt\x1b[48;5;244m \x1b[0mx");
        let (rows, _) = vt.frame(&palette());
        let runs = &rows[0].runs;
        assert_eq!(runs[0].fg, [255, 0, 0]);
        assert_eq!(runs[1].fg, [1, 2, 3]);
        assert_eq!(rows[0].fills, vec![(2, 1, [128, 128, 128])]);
        assert_eq!(runs.last().unwrap().fg, palette().foreground);
        // ANSI 0-15 follow the theme palette.
        vt.feed("\r\x1b[31mq");
        assert_eq!(vt.frame(&palette()).0[0].runs[0].fg, DARK_ANSI[1]);
    }

    #[test]
    fn inverse_and_hidden_cells() {
        let mut vt = Vt::new(10, 1);
        vt.feed("\x1b[7mi\x1b[27m\x1b[8ms");
        let (rows, _) = vt.frame(&palette());
        assert_eq!(rows[0].fills, vec![(0, 1, palette().foreground)]);
        assert_eq!(rows[0].runs.len(), 1, "a hidden cell draws nothing");
    }

    #[test]
    fn application_cursor_keys_and_modifiers() {
        let mut vt = Vt::new(10, 2);
        assert_eq!(vt.key(&key(Key::Up, |_| {})).unwrap(), "\x1b[A");
        vt.feed("\x1b[?1h");
        assert_eq!(vt.key(&key(Key::Up, |_| {})).unwrap(), "\x1bOA");
        assert_eq!(
            vt.key(&key(Key::Left, |m| m.ctrl = true)).unwrap(),
            "\x1b[1;5D"
        );
        assert_eq!(
            vt.key(&key(Key::PageDown, |m| m.shift = true)).unwrap(),
            "\x1b[6;2~"
        );
        assert!(!vt.history_key(&key(Key::PageUp, |_| {}), false), "plain PageUp goes to the program");
        assert!(vt.history_key(&key(Key::PageUp, |m| m.shift = true), false));
        assert!(vt.history_key(&key(Key::Home, |m| (m.shift, m.ctrl) = (true, true)), false));
        assert!(!vt.history_key(&key(Key::Home, |m| (m.shift, m.ctrl) = (true, true)), true), "Cmd is primary on macOS");
        assert_eq!(vt.key(&key(Key::Function(1), |_| {})).unwrap(), "\x1bOP");
        assert_eq!(vt.key(&key(Key::Function(5), |_| {})).unwrap(), "\x1b[15~");
        assert_eq!(vt.key(&key(Key::Tab, |m| m.shift = true)).unwrap(), "\x1b[Z");
        assert_eq!(vt.key(&key(Key::Backspace, |_| {})).unwrap(), "\x7f");
    }

    #[test]
    fn control_and_meta_keys() {
        let mode = TermMode::default();
        // Off macOS Ctrl+C arrives as Command('c') with both cmd and ctrl.
        let ctrl_c = key(Key::Command('c'), |m| {
            m.cmd = true;
            m.ctrl = true
        });
        assert_eq!(encode_key(&ctrl_c, mode, false).unwrap(), "\x03");
        let ctrl_shift_v = key(Key::Command('v'), |m| {
            m.cmd = true;
            m.ctrl = true;
            m.shift = true
        });
        assert_eq!(encode_key(&ctrl_shift_v, mode, false), None);
        let ctrl_bracket = key(Key::Text("[".into()), |m| m.ctrl = true);
        assert_eq!(encode_key(&ctrl_bracket, mode, false).unwrap(), "\x1b");
        let alt_b = key(Key::Text("b".into()), |m| m.alt = true);
        assert_eq!(encode_key(&alt_b, mode, false).unwrap(), "\x1bb");
        // macOS: Cmd shortcuts are the app's, Ctrl still makes control characters.
        let cmd_c = key(Key::Command('c'), |m| m.cmd = true);
        assert_eq!(encode_key(&cmd_c, mode, true), None);
        let mac_ctrl_c = key(Key::Text("c".into()), |m| m.ctrl = true);
        assert_eq!(encode_key(&mac_ctrl_c, mode, true).unwrap(), "\x03");
    }

    #[test]
    fn bracketed_paste() {
        let mut vt = Vt::new(10, 2);
        assert_eq!(vt.paste("a\nb"), "a\rb");
        vt.feed("\x1b[?2004h");
        assert_eq!(vt.paste("a\nb\x1b[201~"), "\x1b[200~a\nb\x1b[201~");
        vt.feed("\x1b[?2004l");
        assert_eq!(vt.paste("x"), "x");
    }

    #[test]
    fn mouse_reporting_modes() {
        let mut vt = Vt::new(80, 24);
        let none = rune::Mods::default();
        assert!(!vt.reports_mouse(false));
        assert_eq!(vt.mouse(MouseButton::Left, true, false, 0, 0, none), None);
        // X10-style normal tracking.
        vt.feed("\x1b[?1000h");
        assert!(vt.reports_mouse(false));
        assert!(!vt.reports_mouse(true), "shift bypasses reporting");
        assert_eq!(
            vt.mouse(MouseButton::Left, true, false, 4, 2, none).unwrap(),
            "\x1b[M %#"
        );
        assert_eq!(
            vt.mouse(MouseButton::Left, false, false, 4, 2, none).unwrap(),
            "\x1b[M#%#"
        );
        assert_eq!(
            vt.mouse(MouseButton::Left, true, true, 5, 2, none),
            None,
            "no motion without 1002/1003"
        );
        // SGR encoding with drag tracking.
        vt.feed("\x1b[?1002h\x1b[?1006h");
        assert_eq!(
            vt.mouse(MouseButton::Left, true, true, 9, 3, none).unwrap(),
            "\x1b[<32;10;4M"
        );
        let ctrl = rune::Mods {
            ctrl: true,
            ..none
        };
        assert_eq!(
            vt.mouse(MouseButton::Right, false, false, 0, 0, ctrl).unwrap(),
            "\x1b[<18;1;1m"
        );
        assert_eq!(
            vt.wheel(2.4, 0, 0, none),
            Wheel::Send("\x1b[<64;1;1M\x1b[<64;1;1M".into())
        );
    }

    #[test]
    fn wheel_scrolls_scrollback_or_sends_arrows_on_the_alternate_screen() {
        let mut vt = Vt::new(10, 2);
        for i in 0..6 {
            vt.feed(&format!("{i}\r\n"));
        }
        vt.feed("6");
        assert_eq!(vt.text(), "5\n6");
        let none = rune::Mods::default();
        assert_eq!(vt.wheel(2., 0, 0, none), Wheel::Scrolled(true));
        assert_eq!(vt.text(), "3\n4");
        assert!(vt.frame(&palette()).1.is_none(), "cursor hides in scrollback");
        vt.scroll_to_bottom();
        // Alternate screen with alternate scroll (1007, on by default) sends arrows.
        vt.feed("\x1b[?1049h");
        assert_eq!(vt.wheel(-1., 0, 0, none), Wheel::Send("\x1b[B".into()));
    }

    #[test]
    fn replies_to_status_and_attribute_queries() {
        let mut vt = Vt::new(80, 24);
        vt.feed("\x1b[3;7H\x1b[6n\x1b[5n\x1b[18t");
        let replies = vt.take_replies(&palette());
        assert_eq!(replies[0], "\x1b[3;7R");
        assert_eq!(replies[1], "\x1b[0n");
        assert_eq!(replies[2], "\x1b[8;24;80t");
        vt.feed("\x1b[c\x1b]11;?\x07");
        let replies = vt.take_replies(&palette());
        assert!(replies[0].starts_with("\x1b[?"), "primary device attributes");
        assert!(replies[1].contains("rgb:0a0a/0a0a/0a0a"), "{:?}", replies[1]);
        assert!(vt.take_replies(&palette()).is_empty());
    }

    /// Every line from the top of scrollback to the bottom of the screen.
    fn all_lines(vt: &Vt) -> Vec<String> {
        let grid = vt.term.grid();
        let history = grid.history_size() as i32;
        (-history..vt.rows as i32)
            .map(|line| {
                (0..vt.cols)
                    .map(|col| grid[Point::new(Line(line), Column(col))].c)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn resize_reflows_the_normal_screen_and_truncates_the_alternate_one() {
        let mut vt = Vt::new(10, 4);
        vt.feed("abcdefghij");
        vt.resize(5, 4);
        assert_eq!(vt.size(), (5, 4));
        let lines = all_lines(&vt);
        let start = lines.iter().position(|l| l == "abcde");
        assert!(
            start.is_some_and(|i| lines.get(i + 1).is_some_and(|l| l == "fghij")),
            "reflow error: {lines:?}"
        );
        vt.resize(5, 1);
        let (rows, cursor) = vt.frame(&palette());
        assert_eq!(rows.len(), 1, "reflow error: {:?}", all_lines(&vt));
        assert_eq!(cursor.unwrap().row, 0);

        let mut vt = Vt::new(10, 4);
        vt.feed("\x1b[?1049habcdefghij");
        vt.resize(5, 4);
        let lines = all_lines(&vt);
        assert_eq!(lines[0], "abcde", "reflow error: {lines:?}");
        assert!(!lines.iter().any(|l| l.contains('f')), "reflow error: {lines:?}");
    }

    #[test]
    fn wide_characters_take_two_cells() {
        let mut vt = Vt::new(10, 1);
        vt.feed("a界b");
        let (rows, _) = vt.frame(&palette());
        let runs: Vec<_> = rows[0]
            .runs
            .iter()
            .map(|r| (r.col, r.cells, r.text.as_str()))
            .collect();
        assert_eq!(runs, vec![(0, 1, "a"), (1, 2, "界"), (3, 1, "b")]);
    }

    #[test]
    fn synchronized_updates_apply_at_their_end() {
        let mut vt = Vt::new(10, 1);
        vt.feed("\x1b[?2026hframe");
        assert!(vt.sync_pending());
        assert_eq!(vt.text(), "");
        vt.feed("\x1b[?2026l");
        assert_eq!(vt.text(), "frame");
        vt.feed("\x1b[?2026hcut");
        assert!(vt.flush_sync(true));
        assert_eq!(vt.text(), "framecut");
    }

    #[test]
    fn block_cursor_carries_the_glyph_beneath_it() {
        let mut vt = Vt::new(10, 1);
        vt.feed("ls\x1b[1G");
        let (_, cursor) = vt.frame(&palette());
        let cursor = cursor.unwrap();
        assert_eq!((cursor.col, cursor.row, cursor.kind), (0, 0, CursorKind::Block));
        assert_eq!(cursor.glyph.unwrap().text, "l");
        assert!(cursor.blinking);
        // DECSCUSR 2: steady block; 25l hides it.
        vt.feed("\x1b[2 q");
        assert!(!vt.frame(&palette()).1.unwrap().blinking);
        vt.feed("\x1b[?25l");
        assert!(vt.frame(&palette()).1.is_none());
    }
}
