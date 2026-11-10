//! Line comments on the diff (web's DiffCommentAnnotation): a comment becomes a review-comment
//! context in the thread's composer, and shows under the line it ends on while it stays there.

use std::sync::atomic::{AtomicU64, Ordering};

use rune::{Cursor, El, Key, KeyEvent, Tx, col, el, icon, row, text, text_input};
use rune_kit::diff::{LineKind, Side};
use serde_json::{Value, json};

use super::state::{DiffState, LineRef, Note};
use super::view::Look;
use crate::chat::composer::{self, chips};
use crate::theme::{self, AlphaMul};
use crate::{App, data, icons, kit};

#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub id: String,
    pub path: String,
    pub start: LineRef,
    pub end: LineRef,
}

/// One line of a file's review, as web counts `startIndex` and `endIndex`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReviewLine<'a> {
    pub kind: LineKind,
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: &'a str,
}

/// The hunks' lines in patch order, with every gap line once the file is loaded (pierre's
/// expanded context, which a partial file lacks).
pub fn review_lines<'a>(st: &'a DiffState, f: usize) -> Vec<ReviewLine<'a>> {
    let Some(file) = st.files().get(f) else { return vec![] };
    let loaded = st.loaded(f);
    let mut out = vec![];
    let gap = |out: &mut Vec<ReviewLine<'a>>, h: usize| {
        if loaded {
            for k in 0..st.gap_len(f, h) {
                if let Some((old, new, text)) = st.gap_line(f, h, k) {
                    out.push(ReviewLine { kind: LineKind::Context, old: Some(old), new: Some(new), text });
                }
            }
        }
    };
    for (h, hunk) in file.hunks.iter().enumerate() {
        gap(&mut out, h);
        out.extend(hunk.lines.iter().map(|l| ReviewLine { kind: l.kind, old: l.old, new: l.new, text: &l.text }));
    }
    gap(&mut out, file.hunks.len());
    out
}

/// `findDiffReviewLineIndex`: the line on `at`'s side first, then the same number on the other.
pub fn find_index(lines: &[ReviewLine<'_>], at: LineRef) -> Option<usize> {
    let on = |side: Side| {
        lines.iter().position(|l| match side {
            Side::Old => l.old == Some(at.line) && l.kind != LineKind::Added,
            Side::New => l.new == Some(at.line) && l.kind != LineKind::Removed,
        })
    };
    let other = if at.side == Side::Old { Side::New } else { Side::Old };
    on(at.side).or_else(|| on(other))
}

/// Where a review line shows: a removed line on the old side, any other on the new.
fn point(l: &ReviewLine<'_>) -> Option<LineRef> {
    match l.kind {
        LineKind::Removed => l.old.map(|line| LineRef { side: Side::Old, line }),
        _ => l.new.map(|line| LineRef { side: Side::New, line }),
    }
}

/// `formatDiffReviewRangeLabel`.
pub fn range_label(lines: &[ReviewLine<'_>]) -> String {
    let (Some(first), Some(last)) = (lines.first(), lines.last()) else { return "line".into() };
    let (Some(a), Some(b)) = (first.new.or(first.old), last.new.or(last.old)) else {
        return if lines.len() == 1 { "line".into() } else { format!("{} lines", lines.len()) };
    };
    let marker = match first.kind {
        LineKind::Added if lines.iter().all(|l| l.kind == LineKind::Added) => "+",
        LineKind::Removed if lines.iter().all(|l| l.kind == LineKind::Removed) => "-",
        _ => "",
    };
    if a == b { format!("{marker}{a}") } else { format!("{marker}{a} to {marker}{b}") }
}

/// The chip's label: `a.rs L12 to L14`, with "(before)" for removed lines.
fn readable(range: &str) -> String {
    let removed = range.starts_with('-');
    let bare = range.trim_start_matches(['+', '-']);
    let parts: Vec<&str> = bare.split(" to ").collect();
    let marker = &range[..range.len() - bare.len()];
    let numeric = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let label = match parts.as_slice() {
        [a] if !marker.is_empty() && numeric(a) => format!("L{a}"),
        [a, b] if !marker.is_empty() && numeric(a) && b.strip_prefix(marker).is_some_and(numeric) => format!("L{a} to L{}", &b[marker.len()..]),
        _ => return range.to_owned(),
    };
    if removed { format!("{label} (before)") } else { label }
}

fn clamp(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push_str("\n… truncated …");
    out
}

/// `reviewCommentContextRecord` for the lines `a..=b` of a file's review.
pub fn record(section: (&str, &str), path: &str, lines: &[ReviewLine<'_>], a: usize, b: usize, body: &str, id: &str) -> Value {
    let (a, b) = (a.min(b), a.max(b).min(lines.len().saturating_sub(1)));
    let sel = lines.get(a..=b).unwrap_or(&[]);
    let range = |pick: fn(&ReviewLine<'_>) -> Option<u32>| {
        let numbered: Vec<u32> = sel.iter().filter_map(pick).collect();
        (numbered.first().copied().unwrap_or(0), numbered.len())
    };
    let (old_start, old_count) = range(|l| l.old);
    let (new_start, new_count) = range(|l| l.new);
    let mut diff = format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@");
    for l in sel {
        let marker = match l.kind {
            LineKind::Added => '+',
            LineKind::Removed => '-',
            LineKind::Context => ' ',
        };
        diff.push('\n');
        diff.push(marker);
        diff.push_str(l.text);
    }
    let label = range_label(sel);
    let name = path.rsplit('/').next().unwrap_or(path);
    json!({
        "version": 1,
        "kind": "review-comment",
        "contextId": chips::context_id("review-comment", id),
        "label": format!("{name} {}", readable(&label)),
        "sectionId": section.0,
        "sectionTitle": section.1,
        "filePath": path,
        "startIndex": a,
        "endIndex": b,
        "rangeLabel": label,
        "text": clamp(body.trim(), 16_000),
        "diff": clamp(&diff, 32_000),
        "fenceLanguage": "diff",
    })
}

/// Opens the editor on the selected lines when they hold `at`, else on `at` alone.
pub fn begin(app: &mut App, f: usize, at: LineRef) {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let ui = &mut app.ui.panel.diff;
    let Some(path) = ui.state.files().get(f).map(|f| f.path().to_owned()) else { return };
    let (start, end) = match ui.state.selection() {
        Some(sel) if sel.file == f && covers(&ui.state, f, sel.start, sel.end, at) => (sel.start, sel.end),
        _ => (at, at),
    };
    let id = format!("file-comment-{}-{}", data::now_ms(), SEQ.fetch_add(1, Ordering::Relaxed));
    ui.comment = Some(Draft { id, path, start, end });
    ui.comment_edit = rune::TextEdit::new("");
    ui.comment_edit.focused = true;
    sync(app);
}

fn covers(st: &DiffState, f: usize, a: LineRef, b: LineRef, at: LineRef) -> bool {
    let lines = review_lines(st, f);
    let (Some(a), Some(b), Some(at)) = (find_index(&lines, a), find_index(&lines, b), find_index(&lines, at)) else { return false };
    a.min(b) <= at && at <= a.max(b)
}

pub fn cancel(app: &mut App) {
    app.ui.panel.diff.comment = None;
    app.ui.panel.diff.comment_edit = rune::TextEdit::new("");
    sync(app);
}

fn submit(app: &mut App) {
    let ui = &app.ui.panel.diff;
    let (Some(draft), body) = (ui.comment.clone(), ui.comment_edit.text.clone()) else { return };
    if body.trim().is_empty() {
        return;
    }
    let Some((thread, _)) = super::super::context(app) else { return };
    let st = &ui.state;
    let Some(f) = st.files().iter().position(|f| f.path() == draft.path) else { return };
    let lines = review_lines(st, f);
    let (Some(a), Some(b)) = (find_index(&lines, draft.start), find_index(&lines, draft.end)) else { return };
    let (id, title) = super::section(app);
    let rec = record((&id, &title), &draft.path, &lines, a, b, &body, &draft.id);
    let ui = &mut app.ui.panel.diff;
    ui.anchors.insert(data::s(&rec, "contextId").to_owned(), (draft.path.clone(), draft.end));
    ui.comment = None;
    ui.comment_edit = rune::TextEdit::new("");
    ui.state.set_selection(None);
    composer::add_thread_context(app, &thread, rec);
    sync(app);
}

fn delete(app: &mut App, context_id: &str) {
    let Some((thread, _)) = super::super::context(app) else { return };
    composer::remove_context(app, &thread, context_id);
    sync(app);
}

/// The draft's review comments on the shown section: id, file, text and last review line.
fn comments(app: &App) -> Vec<(String, String, String, Option<u64>)> {
    let Some((thread, _)) = super::super::context(app) else { return vec![] };
    let reviews: Vec<&Value> = composer::thread_contexts(app, &thread).into_iter().filter(|c| data::s(c, "kind") == "review-comment").collect();
    if reviews.is_empty() {
        return vec![];
    }
    let (section, _) = super::section(app);
    reviews
        .into_iter()
        .filter(|c| data::s(c, "sectionId") == section && c["fenceLanguage"].as_str().unwrap_or("diff") == "diff")
        .map(|c| (data::s(c, "contextId").to_owned(), data::s(c, "filePath").to_owned(), data::s(c, "text").to_owned(), c["endIndex"].as_u64()))
        .collect()
}

/// Puts the composer's review comments and the open editor under their lines.
pub fn sync(app: &mut App) -> bool {
    let ui = &app.ui.panel.diff;
    if ui.shown.is_none() || (ui.comment.is_none() && ui.state.notes().is_empty() && !has_comments(app)) {
        return false;
    }
    let mut notes = vec![];
    let st = &ui.state;
    for (id, path, _, end) in comments(app) {
        let Some(f) = st.files().iter().position(|f| f.path() == path) else { continue };
        let at = match ui.anchors.get(&id) {
            Some((p, at)) if *p == path => Some(*at),
            _ => end.and_then(|i| review_lines(st, f).get(i as usize).and_then(point)),
        };
        if let Some(at) = at {
            notes.push(Note { file: f, at, id, draft: false });
        }
    }
    if let Some(d) = &ui.comment
        && let Some(f) = st.files().iter().position(|f| f.path() == d.path)
    {
        notes.push(Note { file: f, at: d.end, id: d.id.clone(), draft: true });
    }
    app.ui.panel.diff.state.set_notes(notes)
}

fn has_comments(app: &App) -> bool {
    !comments(app).is_empty()
}

/// Note `n`'s row: the editor, or a sent comment.
pub fn view(s: &App, l: &Look, n: usize) -> El {
    let ui = &s.ui.panel.diff;
    let t = &l.t;
    let Some(note) = ui.state.notes().get(n) else { return col() };
    let wrap = col().w_full().px(12.0).py(6.0).fill(l.bg);
    if note.draft {
        let range = ui
            .comment
            .as_ref()
            .and_then(|d| {
                let st = &ui.state;
                let f = st.files().iter().position(|f| f.path() == d.path)?;
                let lines = review_lines(st, f);
                let (a, b) = (find_index(&lines, d.start)?, find_index(&lines, d.end)?);
                Some(range_label(&lines[a.min(b)..=a.max(b)]))
            })
            .unwrap_or_else(|| "line".into());
        let empty = ui.comment_edit.text.trim().is_empty();
        let input = text_input(&ui.comment_edit, theme::TEXT_SM)
            .key("comment-input")
            .role("textbox")
            .label(format!("Comment on lines {range}"))
            .placeholder("Add a comment…", t.c.contrast_placeholder)
            .w_full()
            .min_h(44.0)
            .color(t.c.foreground)
            .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
            .on_edit(|s: &mut App| &mut s.ui.panel.diff.comment_edit)
            .on_input_key(editor_key);
        let footer = row()
            .w_full()
            .gap(6.0)
            .items_center()
            .child(text("⌘/Ctrl Enter to send").font(theme::TEXT_2XS).color(t.c.muted_foreground.alpha_mul(0.7)).grow(1.0).min_w(0.0).truncate())
            .child(kit::Button::new("Cancel").variant("ghost-muted").size("xs").on_press(cancel).build(t))
            .child(kit::Button::new("Comment").size("xs").disabled(empty).on_press(submit).build(t));
        return wrap.key("comment-draft").child(
            col()
                .w_full()
                .gap(6.0)
                .padding(8.0)
                .radius(8.0)
                .border(1.0, t.c.border)
                .fill(t.c.background)
                .child(input)
                .child(footer),
        );
    }
    let body = comments(s).into_iter().find(|(id, ..)| *id == note.id).map(|c| c.2).unwrap_or_default();
    let id = note.id.clone();
    let delete_btn = row()
        .key("delete")
        .size(24.0, 24.0)
        .center()
        .radius(6.0)
        .hover(t.c.accent)
        .cursor(Cursor::Pointer)
        .role("button")
        .label("Delete comment")
        .help("Delete comment")
        .group_opacity(0.0, 1.0)
        .child(icon(&icons::TRASH_2, 14.0).color(t.c.muted_foreground))
        .on_press(move |s: &mut App| delete(s, &id));
    wrap.key("comment").child(
        row()
            .w_full()
            .hover_group()
            .items_start()
            .fill(t.c.primary.alpha_mul(0.045))
            .radius(4.0)
            .child(el().w(2.0).self_stretch().fill(t.c.primary.alpha_mul(0.55)))
            .child(
                row()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(8.0)
                    .px(10.0)
                    .py(6.0)
                    .items_start()
                    .child(icon(&icons::MESSAGE_CIRCLE, 14.0).color(t.c.primary.alpha_mul(0.7)).mt(3.0))
                    .child(text(body).font(theme::TEXT_SM).color(t.c.foreground).wrap().grow(1.0).min_w(0.0))
                    .child(delete_btn),
            ),
    )
}

fn editor_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    match e.key {
        Key::Escape => {
            cancel(s);
            tx.blur();
            true
        }
        Key::Enter if e.mods.cmd || e.mods.ctrl => {
            submit(s);
            tx.blur();
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: LineKind, old: Option<u32>, new: Option<u32>, text: &str) -> ReviewLine<'_> {
        ReviewLine { kind, old, new, text }
    }

    #[test]
    fn a_comment_carries_its_lines_as_a_diff_excerpt() {
        let lines = [
            line(LineKind::Context, Some(10), Some(10), "keep"),
            line(LineKind::Removed, Some(11), None, "old"),
            line(LineKind::Added, None, Some(11), "new"),
            line(LineKind::Added, None, Some(12), "newer"),
        ];
        assert_eq!(find_index(&lines, LineRef { side: Side::Old, line: 11 }), Some(1));
        assert_eq!(find_index(&lines, LineRef { side: Side::New, line: 11 }), Some(2));
        // An old number with no old line falls back to the new side.
        assert_eq!(find_index(&lines, LineRef { side: Side::Old, line: 12 }), Some(3));
        let rec = record(("branch", "Changes"), "src/a.rs", &lines, 3, 2, " looks off \n", "file-comment-1-0");
        assert_eq!(rec["label"], "a.rs L11 to L12");
        assert_eq!(rec["rangeLabel"], "+11 to +12");
        assert_eq!((rec["startIndex"].as_u64(), rec["endIndex"].as_u64()), (Some(2), Some(3)));
        assert_eq!(rec["text"], "looks off");
        assert_eq!(rec["diff"], "@@ -0,0 +11,2 @@\n+new\n+newer");
        assert_eq!(rec["contextId"], "review-comment_file-comment-1-0");
        let mixed = record(("branch", "Changes"), "a.rs", &lines, 0, 1, "x", "c");
        assert_eq!((mixed["rangeLabel"].as_str(), mixed["label"].as_str()), (Some("10 to 11"), Some("a.rs 10 to 11")));
        assert_eq!(mixed["diff"], "@@ -10,2 +10,1 @@\n keep\n-old");
        let removed = record(("branch", "Changes"), "a.rs", &lines, 1, 1, "x", "c");
        assert_eq!(removed["label"], "a.rs L11 (before)");
    }
}
