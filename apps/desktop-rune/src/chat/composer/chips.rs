//! Context chips (ContextChip, composerContextPresentation, composerContextReferences.ts): what a
//! message carries beside its text (a terminal selection, a picked element, a PR). The composer
//! holds each as a pill and sends it as a `[label](t3-context://v1/kind/id)` link plus its record;
//! a sent message shows those links as chips again.

use std::ops::Range;

use rune::{Color, Icon, PillStyle, TextEdit};
use serde_json::{Value, json};

use crate::data::s;
use crate::icons;

const HREF: &str = "t3-context://v1/";
/// Pill ids for context chips; other pills (path mentions) stay plain text when sent.
pub const PILL: &str = "ctx:";
const LABEL_MAX: usize = 200;
const TERMINAL_TEXT_MAX: usize = 64_000;

/// sanitizeComposerContextLabel: no brackets, backslashes or line breaks, one space between words.
pub fn sanitize_label(label: &str, kind: &str) -> String {
    let cleaned: String = label.chars().map(|c| if matches!(c, '[' | ']' | '\\' | '\r' | '\n') { ' ' } else { c }).collect();
    let joined = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = joined.chars().take(LABEL_MAX).collect();
    if cut.is_empty() { kind.to_owned() } else { cut }
}

pub fn pill_id(record: &Value) -> String {
    format!("{PILL}{}/{}", s(record, "kind"), s(record, "contextId"))
}

/// A terminal selection (ThreadTerminalDrawer's "Add to chat").
pub fn terminal(terminal_id: &str, terminal_label: &str, line_start: usize, line_end: usize, text: &str) -> Value {
    let lines = if line_start == line_end { format!("line {line_start}") } else { format!("lines {line_start}-{line_end}") };
    let text: String = text.chars().take(TERMINAL_TEXT_MAX).collect();
    json!({
        "version": 1, "kind": "terminal", "contextId": format!("terminal_{}", s5_net::uuid()),
        "label": sanitize_label(&format!("{terminal_label} {lines}"), "terminal"),
        "terminalId": terminal_id, "terminalLabel": terminal_label,
        "lineStart": line_start, "lineEnd": line_end, "text": text,
    })
}

fn link(record: &Value) -> String {
    let kind = s(record, "kind");
    let bang = if kind == "image" { "!" } else { "" };
    format!("{bang}[{}]({HREF}{kind}/{})", sanitize_label(s(record, "label"), kind), s(record, "contextId"))
}

/// The text to send, each context pill written as its link, and the records still in it: a
/// chip deleted from the prompt drops its record.
pub fn outgoing(edit: &TextEdit, contexts: &[Value]) -> (String, Vec<Value>) {
    let mut pills: Vec<(Range<usize>, &str)> = edit.pills().collect();
    pills.sort_by_key(|(r, _)| r.start);
    let (mut out, mut used, mut at) = (String::new(), Vec::new(), 0);
    for (range, id) in pills {
        let Some(record) = contexts.iter().find(|r| pill_id(r) == id) else { continue };
        out.push_str(&edit.text[at..range.start]);
        out.push_str(&link(record));
        at = range.end;
        if !used.contains(record) {
            used.push(record.clone());
        }
    }
    out.push_str(&edit.text[at..]);
    (out, used)
}

/// A context link in sent text: where its label lands in the display text, and its kind and id.
#[derive(Debug, PartialEq)]
pub struct Shown {
    pub range: Range<usize>,
    pub kind: String,
    pub id: String,
}

/// Sent text with each context link replaced by its label (parseComposerContextReferences).
pub fn display(text: &str) -> (String, Vec<Shown>) {
    let marker = format!("]({HREF}");
    let (mut out, mut chips, mut at) = (String::new(), Vec::new(), 0);
    while let Some(found) = text[at..].find(&marker) {
        let close = at + found;
        let href = close + 2;
        let end = text[href..].find(|c: char| c == ')' || c.is_whitespace()).map(|i| href + i);
        let open = text[at..close].rfind('[').map(|i| at + i);
        let (Some(end), Some(open)) = (end, open) else {
            out.push_str(&text[at..close + 1]);
            at = close + 1;
            continue;
        };
        let label = &text[open + 1..close];
        let parts: Vec<&str> = text[href + HREF.len()..end].split('/').collect();
        if text.as_bytes()[end] != b')' || label.contains(['\n', ']']) || parts.len() != 2 || parts.iter().any(|p| p.is_empty()) {
            out.push_str(&text[at..close + 1]);
            at = close + 1;
            continue;
        }
        let start = if open > at && text.as_bytes()[open - 1] == b'!' { open - 1 } else { open };
        out.push_str(&text[at..start]);
        let shown = if label.trim().is_empty() { parts[0] } else { label };
        chips.push(Shown { range: out.len()..out.len() + shown.len(), kind: parts[0].to_owned(), id: parts[1].to_owned() });
        out.push_str(shown);
        at = end + 1;
    }
    out.push_str(&text[at..]);
    (out, chips)
}

/// A chip's accent and icon (ContextChip's `--context-chip-*` and composerContextPresentation):
/// a PR takes its state's, and anything unknown the muted ink.
pub fn look(kind: &str, record: Option<&Value>) -> Option<(Color, &'static Icon)> {
    let accent = |c: f32, h: f32| rune::color::oklch(0.62, c, h);
    Some(match kind {
        "terminal" => (accent(0.134, 163.0), &icons::TERMINAL),
        "file" => (accent(0.136, 237.0), &icons::FILE),
        "image" => (accent(0.136, 237.0), &icons::IMAGE),
        "mention" => (accent(0.11, 215.0), &icons::FILE),
        "element" | "preview-annotation" => (accent(0.134, 70.0), &icons::MOUSE_POINTER_CLICK),
        "thread" => (accent(0.12, 190.0), &icons::MESSAGES_SQUARE),
        "environment" => (accent(0.11, 145.0), &icons::SERVER),
        "review-comment" => match record.map(|r| &r["pullRequest"]).filter(|p| p.is_object()) {
            Some(pr) if pr["state"] == "merged" => (accent(0.16, 292.0), &icons::GIT_MERGE),
            Some(pr) if pr["state"] == "closed" => (accent(0.16, 16.0), &icons::GIT_PULL_REQUEST_CLOSED),
            Some(pr) if pr["isDraft"] == true => (accent(0.02, 259.0), &icons::GIT_PULL_REQUEST_DRAFT),
            Some(pr) if pr["state"] == "open" => (accent(0.134, 163.0), &icons::GIT_PULL_REQUEST_ARROW),
            Some(_) => (accent(0.16, 277.0), &icons::GIT_PULL_REQUEST_ARROW),
            None => (accent(0.16, 292.0), &icons::MESSAGE_CIRCLE),
        },
        _ => return None,
    })
}

/// Inline chips have no fill: the label and icon take the accent.
pub fn style(kind: &str, record: Option<&Value>) -> PillStyle {
    match look(kind, record) {
        Some((accent, icon)) => PillStyle::new().bg(Color::TRANSPARENT).fg(accent).icon(icon),
        None => PillStyle::new(),
    }
}

/// The composer's pill look by id: context pills by their record, path mentions as mentions.
pub fn pill_style(id: &str, contexts: &[Value]) -> PillStyle {
    match id.strip_prefix(PILL) {
        Some(rest) => {
            let kind = rest.split('/').next().unwrap_or("");
            style(kind, contexts.iter().find(|r| pill_id(r) == id))
        }
        None => style("mention", None),
    }
}

/// A few chips for the composer scene: one of each kind with a producer on the web.
pub fn samples() -> Vec<Value> {
    vec![
        terminal("default", "Terminal 1", 12, 18, "$ vp test run\n 3 passed"),
        json!({"version": 1, "kind": "element", "contextId": "element_scene", "label": "button.primary", "pageUrl": "http://localhost:5173/", "pageTitle": null, "tagName": "button", "selector": "button.primary", "htmlPreview": "<button class=\"primary\">Save</button>", "componentName": null, "source": null, "styles": ""}),
        json!({"version": 1, "kind": "review-comment", "contextId": "review-comment_scene", "label": "PR #42 Fix composer drafts", "sectionId": "pr", "sectionTitle": "Pull request", "filePath": "README.md", "startIndex": 0, "endIndex": 0, "rangeLabel": "", "text": "", "diff": "",
            "pullRequest": {"number": 42, "title": "Fix composer drafts", "url": "https://github.com/t3/t3/pull/42", "headBranch": "fix/drafts", "baseBranch": "main", "state": "open", "isDraft": false}}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_lose_brackets_and_line_breaks() {
        assert_eq!(sanitize_label("  a [b]\\c\nd  ", "terminal"), "a b c d");
        assert_eq!(sanitize_label(" \n ", "terminal"), "terminal");
        assert_eq!(sanitize_label(&"x".repeat(300), "file").len(), 200);
    }

    #[test]
    fn pills_send_as_links_with_their_records_and_deleted_chips_drop_out() {
        let kept = terminal("t1", "Terminal 1", 3, 3, "ls");
        let gone = terminal("t1", "Terminal 1", 4, 9, "pwd");
        assert_eq!(s(&kept, "label"), "Terminal 1 line 3");
        assert_eq!(s(&gone, "label"), "Terminal 1 lines 4-9");
        let mut edit = TextEdit::new("");
        edit.insert("see ");
        edit.insert_pill(pill_id(&kept), s(&kept, "label"));
        edit.insert(" and @a.rs");
        let (text, records) = outgoing(&edit, &[kept.clone(), gone]);
        let link = format!("[Terminal 1 line 3](t3-context://v1/terminal/{})", s(&kept, "contextId"));
        assert_eq!(text, format!("see {link} and @a.rs"));
        assert_eq!(records, [kept]);

        let (shown, chips) = display(&format!("{text} ![shot](t3-context://v1/image/img_1) [x](https://e.com)"));
        assert_eq!(shown, "see Terminal 1 line 3 and @a.rs shot [x](https://e.com)");
        assert_eq!(chips.len(), 2);
        assert_eq!(&shown[chips[0].range.clone()], "Terminal 1 line 3");
        assert_eq!((chips[1].kind.as_str(), chips[1].id.as_str()), ("image", "img_1"));
    }
}
