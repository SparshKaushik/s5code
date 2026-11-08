//! "Search project contents" (`ProjectContentSearchDialog`): `projects.searchContents` over the
//! open thread's checkout, matches grouped by file, Enter opening the match's file.
use super::{Action, Item, item};
use crate::{
    App, data, icons,
    theme::{self, AlphaMul, T},
};
use rune::{El, Tx, row, text};
use serde_json::json;

pub const PAGE: &str = "content";
pub const GROUP: &str = "content";
const DEBOUNCE_MS: i64 = 120;
const LIMIT: usize = 500;
/// Building every row of a 500-match result each frame is wasted work; the web pages them too.
const SHOWN: usize = 200;

#[derive(Default)]
pub struct State {
    options: [bool; 3],
    due: Option<i64>,
    pending: Option<(u64, (String, [bool; 3]))>,
    sent: Option<(String, [bool; 3])>,
    matches: Vec<Match>,
    truncated: bool,
    error: Option<String>,
    invalid_regex: bool,
}
struct Match {
    path: String,
    line: u64,
    content: String,
    ranges: Vec<(usize, usize)>,
}

fn context(s: &App) -> Option<(String, String)> {
    super::actions::files_context(s)
}

pub fn open(s: &mut App) {
    let options = s.ui.shell.palette.content.options;
    s.ui.shell.palette.content = State { options, due: Some(0), ..State::default() };
}

pub fn changed(s: &mut App, tx: &mut Tx) {
    if s.ui.shell.palette.page != PAGE {
        return;
    }
    s.ui.shell.palette.content.due = Some(data::now_ms() + DEBOUNCE_MS);
    tx.wake_after(std::time::Duration::from_millis(DEBOUNCE_MS as u64));
}

/// The query keeps its whitespace, which is significant here; blank only means empty.
fn query(s: &App) -> &str {
    let q = s.ui.shell.palette.query.text.as_str();
    if q.trim().is_empty() { "" } else { q }
}

pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    let state = &s.ui.shell.palette.content;
    if s.ui.shell.palette.page == PAGE && state.due.is_some_and(|due| data::now_ms() >= due) {
        s.ui.shell.palette.content.due = None;
        let key = (query(s).to_owned(), s.ui.shell.palette.content.options);
        if s.ui.shell.palette.content.sent.as_ref() != Some(&key) {
            s.ui.shell.palette.content.sent = Some(key.clone());
            if key.0.is_empty() {
                let state = &mut s.ui.shell.palette.content;
                (state.pending, state.matches, state.truncated, state.error, state.invalid_regex) = (None, vec![], false, None, false);
                changed = true;
            } else if let Some((_, cwd)) = context(s)
                && let Some(net) = s.src.net()
            {
                let [case_sensitive, whole_word, use_regex] = key.1;
                let id = net.request(
                    "projects.searchContents",
                    json!({"cwd": cwd, "query": key.0, "limit": LIMIT, "caseSensitive": case_sensitive, "wholeWord": whole_word, "useRegex": use_regex}),
                );
                s.ui.shell.palette.content.pending = Some((id, key));
                changed = true;
            }
        }
    }
    if let Some((id, key)) = s.ui.shell.palette.content.pending.clone()
        && let Some(reply) = s.src.net().and_then(|net| net.take_reply(id))
    {
        let state = &mut s.ui.shell.palette.content;
        state.pending = None;
        if state.sent.as_ref() == Some(&key) {
            match reply {
                Ok(v) => {
                    state.matches = v["matches"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|m| {
                            let ranges: Vec<(usize, usize)> = m["matchRanges"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .map(|r| (r["start"].as_u64().unwrap_or(0) as usize, r["end"].as_u64().unwrap_or(0) as usize))
                                .collect();
                            let (content, ranges) = display(data::s(m, "lineContent"), &ranges);
                            Match { path: data::s(m, "path").to_owned(), line: m["lineNumber"].as_u64().unwrap_or(0), content, ranges }
                        })
                        .collect();
                    state.truncated = v["truncated"].as_bool().unwrap_or(false);
                    state.invalid_regex = key.1[2] && !v["regexFallbackError"].is_null();
                    state.error = None;
                }
                Err(e) => {
                    state.matches.clear();
                    state.error = Some(super::error_message(&e, "Could not search project contents."));
                }
            }
            s.ui.shell.palette.selected = 0;
        }
        changed = true;
    }
    changed
}

/// A line as drawn: tabs widened to four spaces, the line break dropped, and the server's
/// UTF-16 match ranges turned into byte ranges of that text.
fn display(line: &str, ranges: &[(usize, usize)]) -> (String, Vec<(usize, usize)>) {
    let line = line.trim_end_matches(['\n', '\r']);
    let mut out = String::with_capacity(line.len());
    // `at[u]` is the output byte offset of UTF-16 unit `u`.
    let mut at = Vec::with_capacity(line.len() + 1);
    for c in line.chars() {
        for _ in 0..c.len_utf16() {
            at.push(out.len());
        }
        if c == '\t' { out.push_str("    ") } else { out.push(c) }
    }
    at.push(out.len());
    let byte = |u: usize| at.get(u).copied().unwrap_or(out.len());
    let ranges = ranges.iter().map(|&(a, b)| (byte(a), byte(b))).filter(|(a, b)| a < b).collect();
    (out, ranges)
}

pub fn items(s: &App) -> Vec<Item> {
    let state = &s.ui.shell.palette.content;
    if context(s).is_none() || query(s).is_empty() {
        return vec![];
    }
    state
        .matches
        .iter()
        .take(SHOWN)
        .map(|m| {
            let mut v = item(m.content.clone(), GROUP, &icons::FILE, "", Action::OpenFile(m.path.clone()));
            v.description = Some(m.path.clone());
            v.timestamp = Some(m.line.to_string());
            v.highlight = Some((m.ranges.clone(), vec![]));
            v
        })
        .collect()
}

pub fn placeholder(s: &App) -> String {
    match context(s) {
        Some((title, _)) => format!("Search in {title}"),
        None => "Search project contents…".into(),
    }
}

pub fn empty(s: &App) -> String {
    let state = &s.ui.shell.palette.content;
    if context(s).is_none() {
        "Open a project to search its files.".into()
    } else if !query(s).is_empty() && state.pending.is_none() && state.due.is_none() && state.error.is_none() {
        "No results found.".into()
    } else {
        "Type to search across your project.".into()
    }
}

/// The line above the results: searching, an error, or "N results in M files".
pub fn status(s: &App, t: &T) -> Option<El> {
    let state = &s.ui.shell.palette.content;
    if s.ui.shell.palette.page != PAGE || context(s).is_none() {
        return None;
    }
    let searching = !query(s).is_empty() && (state.pending.is_some() || state.due.is_some());
    if query(s).is_empty() && !searching && state.error.is_none() {
        return None;
    }
    let destructive = t.c.destructive_foreground;
    let (label, color) = if searching {
        ("Searching…".to_owned(), t.c.contrast_muted_foreground)
    } else if let Some(error) = &state.error {
        (error.clone(), destructive)
    } else if state.invalid_regex {
        ("Invalid regular expression".to_owned(), destructive)
    } else {
        let files = state.matches.iter().map(|m| m.path.as_str()).collect::<std::collections::HashSet<_>>().len();
        (format!("{}{} results in {files} files", state.matches.len(), if state.truncated { "+" } else { "" }), t.c.contrast_muted_foreground)
    };
    Some(
        rune::col()
            .key("palette-content-status")
            .w_full()
            .child(row().w_full().h(35.).px(12.).center_y().child(text(label).font(theme::TEXT_XS).color(color).truncate()))
            .child(rune::el().w_full().h(1.).fill(t.c.border)),
    )
}

/// Match case, whole word and regular expression, as the input's segmented toggles.
pub fn options(s: &App, t: &T) -> Option<El> {
    if s.ui.shell.palette.page != PAGE || context(s).is_none() {
        return None;
    }
    let on = s.ui.shell.palette.content.options;
    let mut group = row().gap(2.).padding(2.).radius(8.).border(1., t.c.border).fill(t.c.muted.alpha_mul(0.3));
    for (i, (glyph, label)) in [("Aa", "Match case"), ("ab", "Match whole word"), (".*", "Use regular expression")].into_iter().enumerate() {
        let active = on[i];
        let color = if active { t.c.foreground } else { t.c.muted_foreground };
        let marks = if i == 1 { vec![rune::Span::new(0, 2).underline(color)] } else { vec![] };
        let face = rune::rich(glyph, marks).font(theme::MONO_XS).color(color);
        group = group.child(
            row()
                .key(format!("palette-content-option-{i}"))
                .role("switch")
                .label(label)
                .help(label)
                .h(24.)
                .min_w(28.)
                .px(6.)
                .center()
                .radius(6.)
                .cursor(rune::Cursor::Pointer)
                .fill(if active { t.c.background } else { rune::Color::TRANSPARENT })
                .hover(t.c.accent)
                .child(face)
                .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                    s.ui.shell.palette.content.options[i] ^= true;
                    s.ui.shell.palette.content.due = Some(0);
                    tx.wake_after(std::time::Duration::from_millis(1));
                }),
        );
    }
    Some(group)
}

/// The file header over its matches: name, directory and match count.
pub fn header(path: &str, count: usize, t: &T) -> El {
    let (directory, name) = match path.rsplit_once('/') {
        Some((dir, name)) => (dir, name),
        None => ("", path),
    };
    let mut head = row().w_full().h(32.).px(12.).gap(8.).center_y().child(icon_for(t)).child(text(name).font(theme::XS_MEDIUM).color(t.c.foreground).shrink());
    if !directory.is_empty() {
        head = head.child(text(directory).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().shrink().min_w(0.));
    }
    head.child(rune::spacer()).child(
        el_pill(count.to_string(), t),
    )
}

fn icon_for(t: &T) -> El {
    rune::icon(&icons::FILE, 14.).color(t.c.contrast_icon_muted)
}

fn el_pill(label: String, t: &T) -> El {
    rune::el()
        .px(6.)
        .py(2.)
        .radius(999.)
        .fill(t.c.muted)
        .child(text(label).font(theme::TEXT_3XS.features("'tnum' 1")).color(t.c.muted_foreground))
}

/// A match row: the line number, then the line with its matches marked.
pub fn match_row(i: usize, v: &Item, selected: bool, t: &T) -> El {
    let (ranges, line) = (v.highlight.as_ref().map(|(r, _)| r.as_slice()).unwrap_or(&[]), v.timestamp.clone().unwrap_or_default());
    let spans = ranges.iter().map(|(a, b)| rune::Span::new(*a, *b).bg(t.c.primary.alpha_mul(0.25), 1.)).collect();
    let action = v.action.clone();
    row()
        .key(format!("palette-item-{i}"))
        .role("option")
        .label(format!("{} line {line}", v.description.as_deref().unwrap_or("")))
        .w_full()
        .h(28.)
        .px(12.)
        .gap(12.)
        .center_y()
        .fill(if selected { t.c.accent } else { rune::Color::TRANSPARENT })
        .hover(t.c.accent.alpha_mul(0.6))
        .cursor(rune::Cursor::Pointer)
        .child(rune::el().w(40.).shrink().child(text(line).font(theme::MONO_XS.features("'tnum' 1")).color(t.c.muted_foreground.alpha_mul(0.7)).w_full().align(rune::Align::End)))
        .child(rune::rich(&v.title, spans).font(theme::MONO_XS).color(if selected { t.c.accent_foreground } else { t.c.foreground }).truncate().grow(1.).shrink().min_w(0.))
        .on_press_tx(move |s: &mut App, tx| super::execute(s, action.clone(), tx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_ranges_become_byte_ranges_of_the_drawn_line() {
        // The emoji is two UTF-16 units, 10 and 11.
        let (line, ranges) = display("\tlet é = \"😀x\";\r\n", &[(1, 4), (5, 6), (10, 12), (12, 13), (20, 30)]);
        assert_eq!(line, "    let é = \"😀x\";");
        let marked: Vec<&str> = ranges.iter().map(|&(a, b)| &line[a..b]).collect();
        assert_eq!(marked, ["let", "é", "😀", "x"], "a range past the end draws nothing");
    }
}
