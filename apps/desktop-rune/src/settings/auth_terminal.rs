//! Web `ProviderAuthTerminal`: the provider's sign-in PTY drawn by the terminal panel's VT and
//! typed into directly. Output arrives on the auth subscription; keys, pastes and the grid size
//! go back through `provider.auth.respond`.

use std::cell::RefCell;

use rune::{Color, Cursor, El, Key, KeyEvent, Phase, TextEdit, Tx, el, text, text_input};
use serde_json::{Value, json};

use crate::App;
use crate::panel::vt::{self, Palette, Vt, links};
use crate::theme::{self, T};

/// Web's `h-64`.
const HEIGHT: f32 = 256.0;
const PAD: f32 = 4.0;
/// Web splits input into 4,096-character responses.
const CHUNK: usize = 4096;

struct Term {
    key: String,
    instance: String,
    flow: Value,
    interaction: Value,
    vt: Vt,
    /// The flow's `outputOffset` already fed to the VT.
    written: usize,
    /// Whether the server has been told the grid's size.
    sized: bool,
    palette: Palette,
    input: TextEdit,
}

/// The VT is fed while the view builds, as web's effect writes new output into its surface.
#[derive(Default)]
pub struct Ui {
    term: RefCell<Option<Term>>,
    spare: TextEdit,
}

#[derive(Debug, PartialEq)]
enum Feed<'a> {
    Append(&'a str),
    Reset,
}

/// What the VT still needs: web counts `outputOffset` in UTF-16 units of all output so far,
/// while `output` keeps only its tail, so a gap it no longer covers redraws from the tail.
fn feed(output: &str, offset: usize, written: usize) -> Option<Feed<'_>> {
    if offset == written {
        return None;
    }
    let mut units = 0;
    for (i, c) in output.char_indices().rev() {
        units += c.len_utf16();
        if offset > written && units == offset - written {
            return Some(Feed::Append(&output[i..]));
        }
        if offset < written || units > offset - written {
            break;
        }
    }
    Some(Feed::Reset)
}

fn chunks(data: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = data;
    while rest.chars().count() > CHUNK {
        let (end, _) = rest.char_indices().nth(CHUNK).expect("longer than a chunk");
        out.push(&rest[..end]);
        rest = &rest[end..];
    }
    out.push(rest);
    out
}

/// Web's surface is 13 px whatever the interface size, so the root scale is taken back out.
fn metrics(s: &App) -> (rune::TextStyle, f32, f32) {
    let size = 13.0 / super::fonts::interface_scale(&s.client_settings.values);
    let cell_h = (size * 1.35).round().max(1.0);
    let font = theme::mono(rune::TextStyle::new(size, 400.0)).lh(cell_h);
    let cell_w = rune::text::advance("MMMMMMMMMM", font) / 10.0;
    (font, if cell_w > 0.0 { cell_w } else { size * 0.6 }, cell_h)
}

fn term(s: &mut App) -> Option<&mut Term> {
    s.ui.settings.auth.terminal.term.get_mut().as_mut()
}

fn send(s: &mut App, data: &str, size: Option<(usize, usize)>) {
    let Some(t) = term(s) else { return };
    let (instance, flow, interaction) = (t.instance.clone(), t.flow.clone(), t.interaction.clone());
    for chunk in chunks(data) {
        let mut response = json!({"type": "terminal", "data": chunk});
        if let Some((cols, rows)) = size {
            response["size"] = json!({"cols": cols, "rows": rows});
        }
        super::request(s, "provider.auth.respond", json!({"instanceId": instance, "flowId": flow, "interactionId": interaction, "response": response}));
    }
}

/// Answers the program's terminal queries (cursor position, colours) as a PTY would.
pub fn poll(s: &mut App) -> bool {
    let replies = match term(s) {
        Some(t) => {
            let palette = t.palette;
            t.vt.take_replies(&palette)
        }
        None => return false,
    };
    for reply in replies {
        send(s, &reply, None);
    }
    false
}

fn key(s: &mut App, tx: &mut Tx, e: &KeyEvent) -> bool {
    // Web's `beforeKey` leaves Tab to move focus.
    if matches!(e.key, Key::Tab) {
        return false;
    }
    let m = e.mods;
    let primary = if cfg!(target_os = "macos") { m.cmd && !m.ctrl } else { m.ctrl && !m.logo };
    let letter = match &e.key {
        Key::Command(c) => Some(c.to_ascii_lowercase()),
        Key::Text(t) if m.ctrl || m.cmd => t.chars().next().map(|c| c.to_ascii_lowercase()),
        _ => None,
    };
    // A browser pastes on Ctrl/Cmd+V, and a sign-in code is usually pasted.
    if primary && !m.alt && letter == Some('v') {
        tx.read_clipboard(|s: &mut App, content, _| {
            let text = content.text();
            if let Some(data) = term(s).filter(|_| !text.is_empty()).map(|t| t.vt.paste(text)) {
                send(s, &data, None);
            }
        });
        return true;
    }
    match term(s).and_then(|t| t.vt.key(e)) {
        Some(data) => {
            send(s, &data, None);
            true
        }
        None => false,
    }
}

fn cell(s: &App, x: f32, y: f32) -> (usize, usize) {
    let (_, cell_w, cell_h) = metrics(s);
    let (cols, rows) = s.ui.settings.auth.terminal.term.borrow().as_ref().map_or((80, 16), |t| t.vt.size());
    let col = (((x - PAD) / cell_w).floor().max(0.0) as usize).min(cols - 1);
    (col, (((y - PAD) / cell_h).floor().max(0.0) as usize).min(rows - 1))
}

pub fn view(s: &App, t: &T, instance: &str, state: &Value) -> El {
    let interaction = &state["interaction"];
    let flow_key = format!("{}:{}", state["flowId"], interaction["id"]);
    let output = interaction["output"].as_str().unwrap_or("");
    let offset = interaction["outputOffset"].as_u64().map_or_else(|| output.encode_utf16().count(), |n| n as usize);
    let (font, cell_w, cell_h) = metrics(s);
    let palette = crate::panel::terminal::palette(t);
    let mut slot = s.ui.settings.auth.terminal.term.borrow_mut();
    if slot.as_ref().is_none_or(|t| t.key != flow_key) {
        *slot = Some(Term {
            key: flow_key,
            instance: instance.to_owned(),
            flow: state["flowId"].clone(),
            interaction: interaction["id"].clone(),
            vt: Vt::new(80, 16),
            written: 0,
            sized: false,
            palette,
            input: TextEdit::new(""),
        });
    }
    let cur = slot.as_mut().expect("set above");
    cur.palette = palette;
    match feed(output, offset, cur.written) {
        Some(Feed::Append(tail)) => cur.vt.feed(tail),
        Some(Feed::Reset) => {
            let (cols, rows) = cur.vt.size();
            cur.vt = Vt::new(cols, rows);
            cur.vt.feed(output);
        }
        None => {}
    }
    cur.written = offset;

    let rgb = |c: vt::Rgb8| Color {
        r: rune::color::srgb_decode(c[0] as f32 / 255.0),
        g: rune::color::srgb_decode(c[1] as f32 / 255.0),
        b: rune::color::srgb_decode(c[2] as f32 / 255.0),
        a: 1.0,
    };
    let x = |col: usize| PAD + col as f32 * cell_w;
    let y = |row: usize| PAD + row as f32 * cell_h;
    let mut grid = el()
        .key("provider-auth-terminal")
        .w_full()
        .h(HEIGHT)
        .radius(6.0)
        .clip_rounded(6.0)
        .border(1.0, t.c.border)
        .fill(t.c.terminal_background)
        .role("group")
        .label("Provider sign-in terminal")
        .cursor(Cursor::Text);
    let (rows, cursor) = cur.vt.frame(&palette);
    for (r, row) in rows.iter().enumerate() {
        for &(col, cells, color) in &row.fills {
            grid = grid.child(el().absolute(x(col), y(r)).size(cells as f32 * cell_w, cell_h).fill(rgb(color)));
        }
        for run in &row.runs {
            let mut style = if run.bold { theme::weight(font, 700.0) } else { font };
            style.italic = run.italic;
            grid = grid.child(text(run.text.clone()).font(style).color(rgb(run.fg)).absolute(x(run.col), y(r)));
            if run.underline {
                grid = grid.child(el().absolute(x(run.col), y(r) + cell_h - 2.0).size(run.cells as f32 * cell_w, 1.0).fill(rgb(run.fg)));
            }
        }
    }
    let focused = cur.input.focused;
    if let Some(c) = cursor {
        let mark = el().absolute(x(c.col), y(c.row)).size(cell_w, cell_h);
        grid = grid.child(if focused { mark.fill(t.c.terminal_cursor) } else { mark.border(1.0, t.c.terminal_cursor) });
        if let (true, Some(glyph)) = (focused, &c.glyph) {
            grid = grid.child(text(glyph.text.clone()).font(font).color(t.c.terminal_background).absolute(x(c.col), y(c.row)));
        }
    }
    let (cursor_col, cursor_row) = cur.vt.cursor_point();
    let input = text_input(&cur.input, font)
        .single_line()
        .key("provider-auth-terminal-input")
        .label("Provider sign-in terminal input")
        .absolute(x(cursor_col), y(cursor_row.min(rows.len().saturating_sub(1))))
        .size(1.0, cell_h)
        // Not `opacity(0.)`: Rune never focuses a field it doesn't show.
        .color(Color::TRANSPARENT)
        .caret(Color::TRANSPARENT, Color::TRANSPARENT)
        .on_edit(|s: &mut App| {
            let ui = &mut s.ui.settings.auth.terminal;
            match ui.term.get_mut() {
                Some(t) => &mut t.input,
                None => &mut ui.spare,
            }
        })
        .on_input_key(|s: &mut App, e, tx| key(s, tx, e))
        .on_input_paste(|s: &mut App, content, _| {
            let text = content.text();
            match term(s).map(|t| t.vt.paste(text)) {
                Some(data) => {
                    send(s, &data, None);
                    true
                }
                None => false,
            }
        });
    grid.child(input)
        .on_layout(|s: &mut App, r| {
            let (_, cell_w, cell_h) = metrics(s);
            let cols = ((r[2] - 2.0 * PAD) / cell_w).floor().clamp(2.0, 1000.0) as usize;
            let rows = ((r[3] - 2.0 * PAD) / cell_h).floor().clamp(1.0, 500.0) as usize;
            let Some(t) = term(s) else { return };
            if t.sized && t.vt.size() == (cols, rows) {
                return;
            }
            t.vt.resize(cols, rows);
            t.sized = true;
            send(s, "", Some((cols, rows)));
        })
        .on_drag(|s: &mut App, d, tx| {
            if d.phase != Phase::Down {
                return;
            }
            let (col, row) = cell(s, d.x, d.y);
            let Some(t) = term(s) else { return };
            t.input.focused = true;
            // Web opens only http(s) links from the sign-in terminal.
            if let Some(link) = t.vt.link_at(col, row).filter(|l| d.clicks == 1 && l.kind == links::Kind::Url && links::is_url(&l.text)) {
                let _ = tx.open_url(&link.text);
            }
        })
        .on_wheel(|s: &mut App, w, _| {
            let (_, _, cell_h) = metrics(s);
            let (col, row) = cell(s, w.x, w.y);
            let Some(t) = term(s) else { return false };
            if let vt::Wheel::Send(data) = t.vt.wheel(w.dy / cell_h, col, row, w.mods) {
                send(s, &data, None);
            }
            true
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_feeds_the_new_tail_or_redraws_as_web_does() {
        assert_eq!(feed("abc", 3, 3), None);
        assert_eq!(feed("abcdef", 10, 7), Some(Feed::Append("def")), "only the unseen tail");
        assert_eq!(feed("dé😀", 4, 0), Some(Feed::Append("dé😀")));
        assert_eq!(feed("dé😀", 5, 0), Some(Feed::Reset), "a gap past the kept tail redraws it");
        assert_eq!(feed("dé😀", 4, 1), Some(Feed::Append("é😀")), "offsets count UTF-16 units");
        assert_eq!(feed("abc", 2, 5), Some(Feed::Reset), "a restarted stream redraws");
        assert_eq!(chunks(""), [""]);
        let long = "x".repeat(CHUNK * 2 + 1);
        assert_eq!(chunks(&long).iter().map(|c| c.len()).collect::<Vec<_>>(), [CHUNK, CHUNK, 1]);
    }
}
