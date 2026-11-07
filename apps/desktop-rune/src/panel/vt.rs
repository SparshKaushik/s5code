//! The terminal emulator behind every terminal surface: alacritty's grid model and the `vte`
//! parser, plus xterm's key, mouse and paste encodings. Rendering-agnostic so it tests headless.

use std::sync::{Arc, Mutex};

use alacritty_terminal::{
    Term,
    event::{Event, EventListener, WindowSize},
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point},
    term::{Config, TermMode, cell::Flags},
    vte::ansi::{Color, CursorShape, CursorStyle, NamedColor, Processor, Rgb, StdSyncHandler},
};
use rune::{Key, KeyEvent};

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

    #[test]
    fn resize_keeps_the_grid_and_cursor_on_screen() {
        let mut vt = Vt::new(10, 4);
        vt.feed("abcdefghij");
        vt.resize(5, 4);
        assert_eq!(vt.size(), (5, 4));
        assert!(vt.text().starts_with("abcde\n"), "{:?}", vt.text());
        vt.resize(5, 1);
        let (rows, cursor) = vt.frame(&palette());
        assert_eq!(rows.len(), 1);
        assert_eq!(cursor.unwrap().row, 0);
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
