//! Assistant quotes in sent prompts (AssistantCitationChip, shared/assistantCitations.ts): a
//! `[Assistant quote](t3-citation://v1/…)` link reads as a quote chip, and pressing it shows the
//! quoted text, its comment and Go to source.
use crate::data::s;
use crate::theme::{self, T, radius};
use crate::{App, Route, icons};
use rune::{Align, Color, El, ScrollSpec, Side, Tx, col, el, row, text};

const HREF_PREFIX: &str = "t3-citation://v1/";
const MAX_TEXT: usize = 8_000;
const MAX_COMMENT: usize = 8_000;
const MAX_CONTEXT: usize = 32;
const MAX_ID: usize = 512;
const MAX_HREF: usize = 9 * (MAX_TEXT + MAX_COMMENT) + 16_000;
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// `AssistantCitation`; lengths and positions count UTF-16 units, as on the web.
#[derive(Clone, Debug, PartialEq)]
pub struct Citation {
    pub environment_id: String,
    pub thread_id: String,
    pub message_id: String,
    pub text: String,
    pub comment: Option<String>,
    pub start: u64,
    pub end: u64,
    pub prefix: String,
    pub suffix: String,
}

/// ContextChip's `citation` accent.
pub fn accent() -> Color {
    rune::color::oklch(0.62, 0.16, 259.0)
}

fn units(v: &str) -> usize {
    v.encode_utf16().count()
}

/// `decodeURIComponent`, with `+` read as a space in a query (`URLSearchParams`).
fn decode(v: &str, query: bool) -> Option<String> {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = v.get(i + 1..i + 3).filter(|h| h.bytes().all(|c| c.is_ascii_hexdigit()))?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
                continue;
            }
            b'+' if query => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8(out).ok()
}

fn encode(v: &str, query: bool) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(b as char),
            b'~' if !query => out.push('~'),
            b'*' if query => out.push('*'),
            b' ' if query => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `formatAssistantCitationHref`.
pub fn href(c: &Citation) -> String {
    let path = [&c.environment_id, &c.thread_id, &c.message_id].map(|p| encode(p, false)).join("/");
    let start = c.start.to_string();
    let end = c.end.to_string();
    let mut pairs = vec![("text", c.text.as_str()), ("start", start.as_str()), ("end", end.as_str()), ("prefix", c.prefix.as_str()), ("suffix", c.suffix.as_str())];
    if let Some(comment) = &c.comment {
        pairs.push(("comment", comment.as_str()));
    }
    let query = pairs.iter().map(|(k, v)| format!("{k}={}", encode(v, true))).collect::<Vec<_>>().join("&");
    format!("{HREF_PREFIX}{path}?{query}")
}

/// `parseAssistantCitationHref`: the citation a link names, if it is a valid one.
pub fn parse_href(href: &str) -> Option<Citation> {
    let rest = href.strip_prefix(HREF_PREFIX).filter(|_| href.len() <= MAX_HREF && !href.contains('#'))?;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let parts: Vec<String> = path.split('/').map(|p| decode(p, false)).collect::<Option<_>>()?;
    let [environment_id, thread_id, message_id] = <[String; 3]>::try_from(parts).ok()?;
    if [&environment_id, &thread_id, &message_id].iter().any(|id| id.is_empty() || id.trim() != id.as_str() || units(id) > MAX_ID) {
        return None;
    }
    let mut params: Vec<(String, String)> = Vec::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        params.push((decode(k, true)?, decode(v, true)?));
    }
    let one = |key: &str| {
        let mut found = params.iter().filter(|(k, _)| k == key);
        let first = found.next().map(|(_, v)| v.clone());
        if found.next().is_some() { None } else { first }
    };
    let comment = one("comment");
    let multiple_comments = params.iter().filter(|(k, _)| k == "comment").count() > 1;
    if multiple_comments || params.len() != 5 + usize::from(comment.is_some()) {
        return None;
    }
    let position = |key: &str| {
        let v = one(key)?;
        (!v.is_empty() && v.len() <= 16 && v.bytes().all(|b| b.is_ascii_digit())).then(|| v.parse::<u64>().ok()).flatten().filter(|n| *n <= MAX_SAFE_INTEGER)
    };
    let citation = Citation {
        environment_id,
        thread_id,
        message_id,
        text: one("text")?,
        comment,
        start: position("start")?,
        end: position("end")?,
        prefix: one("prefix")?,
        suffix: one("suffix")?,
    };
    let valid = citation.end > citation.start
        && !citation.text.trim().is_empty()
        && units(&citation.text) <= MAX_TEXT
        && citation.comment.as_deref().is_none_or(|c| units(c) <= MAX_COMMENT)
        && units(&citation.prefix) <= MAX_CONTEXT
        && units(&citation.suffix) <= MAX_CONTEXT;
    valid.then_some(citation)
}

/// `assistantCitationLabel`: the comment (else the quote) on one line, cut at 64 units.
pub fn label(c: &Citation) -> String {
    let source = c.comment.as_deref().map(str::trim).filter(|v| !v.is_empty()).unwrap_or(&c.text);
    let mut preview = String::with_capacity(source.len());
    let mut space = false;
    for ch in source.chars() {
        if ch.is_whitespace() {
            if !space {
                preview.push(' ');
            }
            space = true;
        } else {
            preview.push(ch);
            space = false;
        }
    }
    if units(&preview) <= 64 {
        return preview;
    }
    let mut cut = String::new();
    let mut n = 0;
    for ch in preview.chars() {
        n += ch.len_utf16();
        if n > 64 {
            break;
        }
        cut.push(ch);
    }
    cut.push('…');
    cut
}

fn close(app: &mut App) {
    app.ui.chat.timeline.md_links.citation = None;
}

/// "Go to source": the quoted message's thread, scrolled to the message when it is loaded.
pub fn go_to_source(app: &mut App, c: &Citation, tx: &mut Tx) {
    close(app);
    let state = app.state();
    if !state.threads.iter().chain(&state.archived).any(|t| s(t, "id") == c.thread_id) {
        crate::shell::palette::toast_error(app, "Thread not found", Some("It may belong to another environment."));
        return;
    }
    let row = state.thread_views.get(&c.thread_id).and_then(|v| {
        let p = &v.projection;
        let mut items = super::rows::array(p, "visibleTurnItems").iter().map(|v| &v["item"]).chain(super::rows::array(p, "turnItems"));
        items.find(|i| s(i, "messageId") == c.message_id).map(|i| format!("timeline-row-{}", s(i, "id")))
    });
    if !matches!(app.frame_route(), Route::Thread(id) if *id == c.thread_id) {
        app.navigate(Route::Thread(c.thread_id.clone()));
    }
    if let Some(row) = row {
        tx.scroll_center(row);
    }
}

/// The open chip's popover (ContextChipPopover with AssistantCitationQuote), over its block.
pub fn popover(app: &App, t: &T) -> Option<El> {
    let (anchor, href) = app.ui.chat.timeline.md_links.citation.as_ref()?;
    let citation = parse_href(href)?;
    Some(card(t, &citation).anchored(anchor.clone(), Side::Top, Align::Start, 4.0).modal(true, close))
}

fn card(t: &T, c: &Citation) -> El {
    let quote = row()
        .w_full()
        .child(el().w(2.0).self_stretch().fill(t.c.border))
        .child(text(c.text.clone()).font(theme::TEXT_SM).color(t.c.muted_foreground).selectable(true).grow(1.0).min_w(0.0).pl(12.0));
    let mut body = col().w_full().gap(12.0).child(quote);
    if let Some(comment) = c.comment.as_deref().filter(|v| !v.is_empty()) {
        body = body.child(text(comment.to_owned()).font(theme::TEXT_SM).color(t.c.foreground).selectable(true));
    }
    let target = c.clone();
    let go = crate::kit::Button::new("Go to source")
        .variant("outline")
        .size("sm")
        .icon(&icons::ARROW_UP_RIGHT)
        .build(t)
        .on_press_tx(move |app: &mut App, tx| go_to_source(app, &target, tx));
    col()
        .key("assistant-citation-popover")
        .w(384.0)
        .px(12.0)
        .py(8.0)
        .radius(radius::LG)
        .fill(t.c.popover)
        .border(1.0, t.c.border)
        .shadow(8.0, 24.0, Color::BLACK.alpha(if t.dark { 0.32 } else { 0.08 }))
        .role("dialog")
        .label(format!("Quoted assistant text: {}", label(c)))
        .child(
            col()
                .w_full()
                .gap(12.0)
                .padding(4.0)
                .child(col().w_full().max_h(256.0).scroll(ScrollSpec { stick_bottom: false }).child(body))
                .child(go),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Citation {
        Citation {
            environment_id: "env 1".into(),
            thread_id: "thread:a/b".into(),
            message_id: "msg-1".into(),
            text: "Use a *mutex* & retry\nwhen it fails".into(),
            comment: None,
            start: 4,
            end: 37,
            prefix: "First, ".into(),
            suffix: ".".into(),
        }
    }

    #[test]
    fn hrefs_round_trip_like_web() {
        let c = sample();
        let h = href(&c);
        assert!(h.starts_with("t3-citation://v1/env%201/thread%3Aa%2Fb/msg-1?text=Use+a+*mutex*+%26+retry%0Awhen+it+fails&start=4&end=37"), "{h}");
        assert_eq!(parse_href(&h), Some(c.clone()));
        let commented = Citation { comment: Some("Why?".into()), ..c };
        assert_eq!(parse_href(&href(&commented)), Some(commented));
    }

    #[test]
    fn invalid_citations_are_plain_links() {
        let h = href(&sample());
        for bad in [
            h.replace("t3-citation://v1/", "t3-citation://v2/"),
            h.replace("start=4", "start=40"),
            h.replace("start=4", "start=x"),
            format!("{h}&start=5"),
            format!("{h}&extra=1"),
            format!("{h}#frag"),
            h.replace("msg-1", "msg-1/more"),
            h.replace("text=Use", "text=%E0%A4"),
            h.replace("prefix=First%2C+", &format!("prefix={}", "a".repeat(33))),
        ] {
            assert_eq!(parse_href(&bad), None, "{bad}");
        }
    }

    #[test]
    fn labels_collapse_whitespace_and_prefer_the_comment() {
        let c = sample();
        assert_eq!(label(&c), "Use a *mutex* & retry when it fails");
        assert_eq!(label(&Citation { comment: Some("  Tighten this ".into()), ..c.clone() }), "Tighten this");
        assert_eq!(label(&Citation { comment: Some("   ".into()), ..c.clone() }), "Use a *mutex* & retry when it fails");
        let long = Citation { text: "x".repeat(70), ..c };
        assert_eq!(label(&long), format!("{}…", "x".repeat(64)));
    }
}
