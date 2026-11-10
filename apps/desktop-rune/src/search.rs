//! "Search project contents" (apps/web `components/search/ProjectContentSearchDialog`): the
//! palette's child dialog that greps the active project with `projects.searchContents`, groups
//! the matches by file, and opens the picked file in the Files panel.

use rune::{Cursor, El, Key, KeyEvent, ScrollSpec, Span, TextEdit, Tx, col, el, icon, rich, row, text, text_input};
use serde_json::{Value, json};

use crate::theme::{self, AlphaMul, T};
use crate::{App, data, icons, kit};

const DEBOUNCE_MS: i64 = 120;
const LIMIT: u64 = 500;
/// Laying out every one of 500 rows at once stalls a frame; more load as the list scrolls.
const VISIBLE_WINDOW: usize = 100;

#[derive(Clone, Debug, Default, PartialEq)]
struct Params {
    cwd: String,
    query: String,
    case_sensitive: bool,
    whole_word: bool,
    regex: bool,
}

#[derive(Default)]
struct Results {
    params: Params,
    matches: Vec<Value>,
    truncated: bool,
    invalid_regex: bool,
    error: Option<String>,
}

#[derive(Default)]
pub struct Ui {
    pub open: bool,
    query: TextEdit,
    case_sensitive: bool,
    whole_word: bool,
    regex: bool,
    selected: usize,
    visible: usize,
    due: Option<i64>,
    pending: Option<(u64, Params)>,
    results: Results,
}

/// `useActiveProjectTarget`: the open thread's checkout, else the draft's project.
fn target(s: &App) -> Option<(String, String)> {
    let ctx = crate::chat::composer::ctx(s);
    let project = ctx.project?;
    let cwd = ctx.thread.and_then(|t| t["worktreePath"].as_str()).filter(|v| !v.is_empty()).unwrap_or(data::s(project, "workspaceRoot"));
    (!cwd.is_empty()).then(|| (data::s(project, "title").to_string(), cwd.to_string()))
}

fn params(s: &App) -> Option<Params> {
    let ui = &s.ui.search;
    let (_, cwd) = target(s)?;
    Some(Params { cwd, query: ui.query.text.clone(), case_sensitive: ui.case_sensitive, whole_word: ui.whole_word, regex: ui.regex })
}

pub fn open(s: &mut App) {
    s.ui.shell.palette.open = false;
    let mut query = TextEdit::default();
    query.focused = true;
    s.ui.search = Ui { open: true, query, visible: VISIBLE_WINDOW, ..Ui::default() };
}

fn close(s: &mut App) {
    s.ui.search.open = false;
    s.ui.search.pending = None;
}

fn changed(s: &mut App, tx: &mut Tx) {
    s.ui.search.due = Some(data::now_ms() + DEBOUNCE_MS);
    tx.wake_after(std::time::Duration::from_millis(DEBOUNCE_MS as u64));
}

fn toggle(s: &mut App, which: fn(&mut Ui) -> &mut bool) {
    let flag = which(&mut s.ui.search);
    *flag = !*flag;
    s.ui.search.due = Some(0);
}

pub fn poll(s: &mut App) -> bool {
    if !s.ui.search.open {
        return false;
    }
    let mut changed = false;
    if s.ui.search.due.is_some_and(|due| data::now_ms() >= due) {
        s.ui.search.due = None;
        if let Some(p) = params(s)
            && !p.query.trim().is_empty()
            && s.ui.search.pending.as_ref().map(|(_, q)| q) != Some(&p)
            && s.ui.search.results.params != p
            && let Some(net) = s.src.net()
        {
            let id = net.request(
                "projects.searchContents",
                json!({"cwd": p.cwd, "query": p.query, "limit": LIMIT, "caseSensitive": p.case_sensitive, "wholeWord": p.whole_word, "useRegex": p.regex}),
            );
            s.ui.search.pending = Some((id, p));
            changed = true;
        }
    }
    if let Some((id, p)) = s.ui.search.pending.clone()
        && let Some(reply) = s.src.net().and_then(|net| net.take_reply(id))
    {
        let ui = &mut s.ui.search;
        ui.pending = None;
        ui.selected = 0;
        ui.visible = VISIBLE_WINDOW;
        ui.results = match reply {
            Ok(v) => Results {
                matches: v["matches"].as_array().cloned().unwrap_or_default(),
                truncated: v["truncated"] == true,
                invalid_regex: p.regex && v.get("regexFallbackError").is_some_and(|e| !e.is_null()),
                error: None,
                params: p,
            },
            Err(cause) => Results { error: Some(crate::shell::palette::error_message(&cause, "Search failed.")), params: p, ..Results::default() },
        };
        changed = true;
    }
    changed
}

/// What the list shows: the settled results while they match the typed query.
fn current(s: &App) -> (&[Value], bool) {
    let ui = &s.ui.search;
    let typed = ui.query.text.trim();
    let pending = !typed.is_empty() && (ui.due.is_some() || ui.pending.is_some() || params(s).as_ref() != Some(&ui.results.params));
    let matches = if typed.is_empty() { &[][..] } else { &ui.results.matches[..] };
    (matches, pending)
}

fn open_match(s: &mut App, index: usize) {
    let (matches, pending) = current(s);
    if pending || s.ui.search.results.error.is_some() {
        return;
    }
    let Some((path, line)) = matches.get(index).map(|m| (data::s(m, "path").to_string(), m["lineNumber"].as_u64().map(|n| n as usize))) else { return };
    close(s);
    crate::panel::open_file_at(s, path, line);
}

fn on_input_key(s: &mut App, e: &KeyEvent, _tx: &mut Tx) -> bool {
    let n = current(s).0.len();
    let ui = &mut s.ui.search;
    match e.key {
        Key::Escape => close(s),
        Key::Down if n > 0 => ui.selected = (ui.selected + 1) % n,
        Key::Up if n > 0 => ui.selected = (ui.selected + n - 1) % n,
        Key::Enter => {
            let i = ui.selected;
            open_match(s, i);
        }
        _ => return false,
    }
    let ui = &mut s.ui.search;
    if ui.selected >= ui.visible {
        ui.visible = ui.selected + VISIBLE_WINDOW;
    }
    true
}

pub fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if !crate::panel::terminal_active(s) && crate::panel::shortcut(s, e, "projectSearch.toggle") {
        if s.ui.search.open { close(s) } else { open(s) }
        return true;
    }
    s.ui.search.open && on_input_key(s, e, tx)
}

/// UTF-16 offsets (the server's JavaScript string indices) as byte offsets into `line`.
fn byte_ranges(line: &str, ranges: &Value) -> Vec<(usize, usize)> {
    let at = |unit: u64| {
        let mut seen = 0u64;
        for (i, c) in line.char_indices() {
            if seen >= unit {
                return i;
            }
            seen += c.len_utf16() as u64;
        }
        line.len()
    };
    ranges
        .as_array()
        .map(|rows| rows.iter().filter_map(|r| Some((at(r["start"].as_u64()?), at(r["end"].as_u64()?)))).filter(|(a, b)| b > a).collect())
        .unwrap_or_default()
}

fn option(label: &'static str, glyph: &'static str, on: bool, enabled: bool, flag: fn(&mut Ui) -> &mut bool, t: &T) -> El {
    let fg = if on { t.c.contrast_foreground } else { t.c.contrast_muted_foreground };
    let mut glyph_el = text(glyph).font(theme::mono(theme::XS_MEDIUM)).color(fg);
    if glyph == "ab" {
        glyph_el = rich(glyph, vec![Span::new(0, 2).underline(fg)]).font(theme::mono(theme::XS_MEDIUM)).color(fg);
    }
    let mut b = row()
        .role("switch")
        .label(label)
        .help(label)
        .h(24.)
        .min_w(28.)
        .px(6.)
        .center()
        .radius(6.)
        .child(glyph_el);
    if on {
        b = b.fill(t.c.background).border(1., t.c.contrast_border.alpha_mul(0.6));
    }
    if !enabled {
        return b.disabled(true).opacity(0.64);
    }
    b.hover(t.c.accent).cursor(Cursor::Pointer).on_press(move |s: &mut App| toggle(s, flag))
}

fn status_line(s: &App, t: &T) -> Option<El> {
    let ui = &s.ui.search;
    let (matches, pending) = current(s);
    if ui.query.text.trim().is_empty() && !pending {
        return None;
    }
    let content = if pending {
        row().gap(8.).center_y().child(kit::Spinner::new().size("xs").tone("muted").build(&kit::theme(t))).child(text("Searching…").font(theme::TEXT_XS).color(t.c.contrast_muted_foreground))
    } else if let Some(error) = &ui.results.error {
        text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive)
    } else if ui.results.invalid_regex {
        text("Invalid regular expression").font(theme::TEXT_XS).color(t.c.destructive)
    } else {
        let files = matches.iter().map(|m| data::s(m, "path")).collect::<std::collections::HashSet<_>>().len();
        let plus = if ui.results.truncated { "+" } else { "" };
        text(format!("{}{plus} results in {files} files", matches.len())).font(theme::TEXT_XS).color(t.c.contrast_muted_foreground)
    };
    Some(col().w_full().child(row().w_full().h(35.).px(12.).center_y().child(content)).child(el().w_full().h(1.).fill(t.c.contrast_border)))
}

pub fn overlay(s: &App, t: &T) -> Vec<El> {
    if !s.ui.search.open {
        return vec![];
    }
    let ui = &s.ui.search;
    let target = target(s);
    let enabled = target.is_some();
    let placeholder = match &target {
        Some((name, _)) => format!("Search in {name}"),
        None => "Search project contents…".into(),
    };
    let mut input = text_input(&ui.query, theme::TEXT_SM)
        .key("content-search-query")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .h(34.)
        .py(7.)
        .font(theme::TEXT_SM)
        .color(t.c.contrast_foreground)
        .placeholder(placeholder, t.c.contrast_placeholder);
    if enabled {
        input = input.on_input_key(on_input_key).on_edit(|s: &mut App| &mut s.ui.search.query).on_input_change(changed);
    } else {
        input = input.disabled(true);
    }
    let options = row()
        .gap(2.)
        .padding(2.)
        .radius(8.)
        .border(1., t.c.contrast_border)
        .fill(t.c.muted.alpha_mul(0.3))
        .child(option("Match case", "Aa", ui.case_sensitive, enabled, |u| &mut u.case_sensitive, t))
        .child(option("Match whole word", "ab", ui.whole_word, enabled, |u| &mut u.whole_word, t))
        .child(option("Use regular expression", ".*", ui.regex, enabled, |u| &mut u.regex, t));
    let back = row()
        .key("content-search-back")
        .role("button")
        .label("Back")
        .center()
        .cursor(Cursor::Pointer)
        .on_press(|s: &mut App| {
            close(s);
            s.ui.shell.palette.open = true;
            s.ui.shell.palette.query.focused = true;
        })
        .child(icon(&icons::ARROW_LEFT, 16.).color(t.c.contrast_icon_muted));
    let head = row().w_full().h(48.).pl(17.).pr(10.).gap(12.).center_y().child(back).child(input).child(options);

    let (matches, pending) = current(s);
    let body = if matches.is_empty() {
        let label = if !enabled {
            "Open a project to search its files."
        } else if let Some(error) = &ui.results.error {
            error.as_str()
        } else if !ui.query.text.trim().is_empty() && !pending {
            "No results found."
        } else {
            "Type to search across your project."
        };
        col().w_full().grow(1.0).min_h(0.).px(24.).center().child(text(label.to_string()).font(theme::TEXT_SM).color(t.c.contrast_muted_foreground))
    } else {
        kit::scroll_area("content-search-results", results(s, matches, pending, t).grow(1.0).min_h(0.).scroll(ScrollSpec { stick_bottom: false }), t)
    };
    let footer = row()
        .w_full()
        .h(40.)
        .pl(17.)
        .pr(16.)
        .gap(12.)
        .center_y()
        .child(hint("↑↓", "Navigate", t))
        .child(hint("Enter", "Open file", t))
        .child(hint("Esc", "Back", t));
    let mut panel = col()
        .key("content-search")
        .role("dialog")
        .label(match &target {
            Some((name, _)) => format!("Search file contents in {name}"),
            None => "Search project contents".into(),
        })
        .w(576.)
        .h(420.)
        .radius(16.)
        .clip_rounded(16.)
        .fill(t.c.background.alpha_mul(0.8))
        .backdrop_blur(16.)
        .border(1., t.c.contrast_foreground.alpha_mul(0.1))
        .child(head);
    if let Some(status) = status_line(s, t) {
        panel = panel.child(status);
    }
    let panel = panel.child(body).child(el().w_full().h(1.).fill(t.c.contrast_border)).child(footer).modal(true, close);
    vec![
        kit::motion::scrim(t).key("content-search-backdrop"),
        col().key("content-search-layer").absolute(0., 0.).w_full().h_full().pt(90.).items_center().transition(rune::Transition::none()).child(kit::motion::popup(panel)),
    ]
}

fn hint(key: &str, label: &str, t: &T) -> El {
    row()
        .gap(4.)
        .center_y()
        .child(row().center().h(20.).min_w(20.).px(4.).radius(4.).fill(t.c.contrast_foreground.alpha_mul(0.08)).child(text(key.to_string()).font(theme::XS_MEDIUM).color(t.c.contrast_foreground)))
        .child(text(label.to_string()).font(theme::SM_MEDIUM).color(t.c.contrast_muted_foreground))
}

fn results(s: &App, matches: &[Value], pending: bool, t: &T) -> El {
    let ui = &s.ui.search;
    let can_open = !pending && ui.results.error.is_none();
    let mut list = col().w_full().py(8.);
    let mut index = 0;
    let shown = &matches[..matches.len().min(ui.visible)];
    while index < shown.len() {
        let path = data::s(&shown[index], "path");
        let end = shown[index..].iter().position(|m| data::s(m, "path") != path).map_or(shown.len(), |n| index + n);
        let (directory, name) = path.rsplit_once('/').unwrap_or(("", path));
        let mut heading = row()
            .w_full()
            .h(32.)
            .px(12.)
            .gap(8.)
            .center_y()
            .child(icon(&icons::FILE, 14.).color(t.c.contrast_muted_foreground))
            .child(text(name.to_string()).font(theme::XS_MEDIUM).color(t.c.contrast_foreground));
        if !directory.is_empty() {
            heading = heading.child(text(directory.to_string()).font(theme::TEXT_XS).color(t.c.contrast_muted_foreground).truncate().shrink().min_w(0.));
        }
        heading = heading.child(rune::spacer()).child(
            row().px(6.).py(2.).radius(theme::radius::FULL).fill(t.c.muted).child(text((end - index).to_string()).font(theme::TEXT_3XS).color(t.c.contrast_muted_foreground)),
        );
        let mut section = col().w_full().pb(8.).child(heading);
        for (i, m) in shown.iter().enumerate().take(end).skip(index) {
            let line = data::s(m, "lineContent");
            let spans = byte_ranges(line, &m["matchRanges"]).into_iter().map(|(a, b)| Span::new(a, b).bg(t.c.primary.alpha_mul(0.25), 1.)).collect::<Vec<_>>();
            let selected = i == ui.selected;
            let mut r = row()
                .key(format!("content-match-{i}"))
                .role("button")
                .label(format!("{path}:{}", m["lineNumber"]))
                .w_full()
                .h(28.)
                .px(12.)
                .gap(12.)
                .center_y()
                .child(row().w(40.).justify_end().child(text(m["lineNumber"].to_string()).font(theme::MONO_XS).color(t.c.contrast_muted_foreground.alpha_mul(0.7))))
                .child(rich(line.to_string(), spans).font(theme::MONO_XS).color(t.c.contrast_foreground).truncate().grow(1.0).shrink().min_w(0.));
            if selected {
                r = r.fill(t.c.accent);
            }
            if can_open {
                r = r
                    .hover(t.c.accent.alpha_mul(0.6))
                    .cursor(Cursor::Pointer)
                    .on_hover(move |s: &mut App| s.ui.search.selected = i)
                    .on_press(move |s: &mut App| open_match(s, i));
            }
            section = section.child(r);
        }
        list = list.child(section);
        index = end;
    }
    if matches.len() > ui.visible {
        list = list.child(
            row()
                .w_full()
                .h(32.)
                .center()
                .role("button")
                .label("Show more results")
                .cursor(Cursor::Pointer)
                .child(text("Show more results").font(theme::TEXT_XS).color(t.c.contrast_muted_foreground))
                .on_press(|s: &mut App| s.ui.search.visible += VISIBLE_WINDOW),
        );
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_match_ranges_become_byte_ranges() {
        let line = "é😀 needle";
        // "é" is one UTF-16 unit, "😀" two, then a space: "needle" starts at unit 4.
        let ranges = byte_ranges(line, &json!([{"start": 4, "end": 10}]));
        assert_eq!(ranges, vec![(7, 13)]);
        assert_eq!(&line[7..13], "needle");
        assert!(byte_ranges(line, &json!([{"start": 3, "end": 3}])).is_empty());
        assert_eq!(byte_ranges("abc", &json!([{"start": 1, "end": 99}])), vec![(1, 3)]);
    }
}
