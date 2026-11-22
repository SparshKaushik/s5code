//! TimelineMinimap: a strip per prompt in the timeline's left gutter. Hovering one previews its
//! turn; pressing it scrolls there.

use super::md_links::{self, Target};
use super::rows::{self, Row};
use crate::App;
use crate::chat::details::Lane;
use crate::data::s;
use crate::theme::{self, AlphaMul, T, radius};
use rune::{Cx, El, Key, KeyEvent, Tx, col, el, text};
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;

/// TIMELINE_MINIMAP_MIN_ITEMS.
const MIN_ITEMS: usize = 2;
/// TIMELINE_MINIMAP_ITEM_SPACING.
const SPACING: f32 = 8.0;
/// TIMELINE_MINIMAP_PERSISTENT_GUTTER: a gutter this wide shows the strips without hover.
const PERSISTENT_GUTTER: f32 = 48.0;
/// TIMELINE_MINIMAP_HIT_STRIP_LEFT and _MAX_WIDTH.
const STRIP_LEFT: f32 = 12.0;
const STRIP_MAX_W: f32 = 40.0;
/// The preview reads this much of a reply (resolveTimelineMinimapPreview).
const PREVIEW_CHARS: usize = 1_500;
const BOOTSTRAP_PROMPT: &str =
    "[User attached one or more files without additional text. Respond using the conversation context and the attached files.]";

/// TimelineMinimapItem: a prompt's row key, its text in one line, and its turn's final reply.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Item {
    pub key: String,
    pub user: Option<String>,
    pub reply: Option<String>,
}

/// Which projection the cached items came from: the thread, its item counts and paging. Streamed
/// text changes none of these, and the preview reads reply text when it shows.
type Stamp = (String, [usize; 3], bool);

#[derive(Default)]
pub struct State {
    /// The strip under the pointer or the keyboard, whose turn the preview shows.
    pub active: Option<usize>,
    cache: RefCell<Option<(Stamp, Rc<[Item]>)>>,
}

/// isAttachmentOnlyBootstrapPrompt, `Ultrathink:` prefix allowed.
fn bootstrap(text: &str) -> bool {
    let body = match text.get(..11) {
        Some(head) if head.eq_ignore_ascii_case("ultrathink:") => text[11..].trim_start(),
        _ => text,
    };
    body == BOOTSTRAP_PROMPT
}

/// compactMinimapPreview: whitespace runs as one space; none when nothing is left.
fn compact(text: &str) -> Option<String> {
    let out = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!out.is_empty()).then_some(out)
}

/// deriveTimelineMinimapItems: each user row, with the last reply before the next one.
pub(super) fn derive(rows: &[Row], keys: &[String]) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    for (row, key) in rows.iter().zip(keys) {
        match row {
            Row::User(item) => {
                let text = s(item, "text");
                items.push(Item { key: key.clone(), user: if bootstrap(text) { None } else { compact(text) }, reply: None });
            }
            Row::Assistant(item) => {
                if let Some(last) = items.last_mut() {
                    last.reply = Some(s(item, "id").to_owned());
                }
            }
            _ => {}
        }
    }
    items
}

/// The routed thread's items, derived once per change to its item lists.
fn items(app: &App, thread: &str) -> Option<Rc<[Item]>> {
    let view = app.state().thread_views.get(thread)?;
    let p = &view.projection;
    let counts = ["turnItems", "visibleTurnItems", "messages"].map(|k| rows::array(p, k).len());
    let cache = &app.ui.chat.timeline.minimap.cache;
    let hit = cache
        .borrow()
        .as_ref()
        .filter(|((at, n, more), _)| at == thread && *n == counts && *more == view.has_more_history)
        .map(|(_, items)| items.clone());
    if hit.is_some() {
        return hit;
    }
    let rows = rows::derive(p, app.now_ms);
    let items: Rc<[Item]> = derive(&rows, &super::row_keys(&rows)).into();
    *cache.borrow_mut() = Some(((thread.to_owned(), counts, view.has_more_history), items.clone()));
    Some(items)
}

/// A reply's text by its timeline item id.
fn reply_text<'a>(p: &'a Value, id: &str) -> Option<&'a str> {
    let visible = rows::array(p, "visibleTurnItems").iter().map(|v| if v["item"].is_object() { &v["item"] } else { v });
    rows::array(p, "turnItems").iter().chain(visible).chain(rows::array(p, "messages")).find(|i| s(i, "id") == id).map(|i| s(i, "text"))
}

/// HTML in Markdown as text: comments and tags dropped, block tags leaving a space.
fn strip_tags(html: &str) -> String {
    const BLOCKS: [&str; 33] = [
        "address", "article", "aside", "blockquote", "br", "dd", "details", "div", "dl", "dt", "figcaption", "figure", "footer", "h1", "h2", "h3",
        "h4", "h5", "h6", "header", "hr", "li", "main", "nav", "ol", "p", "pre", "section", "summary", "table", "td", "th", "ul",
    ];
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        if let Some(comment) = tail.strip_prefix("<!--") {
            rest = comment.split_once("-->").map_or("", |(_, r)| r);
            continue;
        }
        let name: String = tail[1..].trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        match tail.find('>') {
            Some(end) if name.starts_with(|c: char| c.is_ascii_alphabetic()) => {
                if BLOCKS.iter().any(|b| b.eq_ignore_ascii_case(&name)) {
                    out.push(' ');
                }
                rest = &tail[end + 1..];
            }
            _ => {
                out.push('<');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A reply's opening as plain text (markdownNodeText): a thread autolink reads "a thread".
fn plain(markdown: &str) -> String {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
    let head = markdown.char_indices().nth(PREVIEW_CHARS).map_or(markdown, |(at, _)| &markdown[..at]);
    let mut out = String::new();
    let mut link: Option<usize> = None;
    for event in Parser::new_ext(head, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Text(t) | Event::Code(t) => out.push_str(&t),
            Event::Html(h) | Event::InlineHtml(h) => out.push_str(&strip_tags(&h)),
            Event::SoftBreak | Event::HardBreak => out.push(' '),
            Event::Start(Tag::Link { .. }) => link = Some(out.len()),
            Event::End(TagEnd::Link) => {
                if let Some(at) = link.take()
                    && matches!(md_links::classify(&out[at..]), Target::Thread(_))
                {
                    out.truncate(at);
                    out.push_str("a thread");
                }
            }
            Event::End(TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Image | TagEnd::TableCell) => {}
            Event::End(_) => out.push(' '),
            _ => {}
        }
    }
    out
}

/// resolveTimelineMinimapIndexFromPointer.
fn index_at(y: f32, rail_h: f32, n: usize) -> Option<usize> {
    match n {
        0 => None,
        1 => Some(0),
        _ if rail_h <= 0.0 => None,
        _ => Some(((y / rail_h).clamp(0.0, 1.0) * (n - 1) as f32).round() as usize),
    }
}

/// resolveTimelineMinimapTopPercent, in points down the rail.
fn top(index: usize, n: usize, rail_h: f32) -> f32 {
    if n <= 1 { 0.0 } else { index.min(n - 1) as f32 / (n - 1) as f32 * rail_h }
}

fn jump(app: &mut App, tx: &mut Tx, list: &str, items: &[Item]) {
    let state = &mut app.ui.chat.timeline.minimap;
    if let Some(item) = state.active.and_then(|i| items.get(i)) {
        tx.scroll_center_in(list.to_owned(), item.key.clone());
    }
    state.active = None;
}

/// The minimap over the routed thread's timeline, in the gutter left of the chat lane.
pub fn view(app: &App, cx: &mut Cx<App>, t: &T, lane: &Lane) -> Option<El> {
    let crate::Route::Thread(thread) = app.frame_route() else { return None };
    let items = items(app, thread)?;
    let n = items.len();
    // resolveTimelineMinimapHitStripWidth: no room to point at it, no minimap.
    let hit = (lane.left.floor() - STRIP_LEFT).clamp(0.0, STRIP_MAX_W);
    if n < MIN_ITEMS || hit <= 0.0 {
        return None;
    }
    let persistent = lane.left >= PERSISTENT_GUTTER;
    let body_h = cx.layout_with("chat-body", |r| r[3]).unwrap_or(848.0);
    // min((n - 1) * 8px, 100vh - 18rem), the chat body standing under a 48pt top bar.
    let rail_h = ((n - 1) as f32 * SPACING).max(1.0).min((body_h - 240.0).max(SPACING));
    let active = app.ui.chat.timeline.minimap.active.filter(|&i| i < n);
    let list = format!("timeline-{thread}");

    let mut strips = el().w(hit).h(rail_h).child(el().absolute(STRIP_LEFT, 0.0).w(1.0).h(rail_h).fill(t.c.border.alpha_mul(0.15)));
    for index in 0..n {
        let distance = active.map(|a| a.abs_diff(index));
        let w = match distance {
            Some(0) => 24.0,
            Some(1) => 16.0,
            Some(2) => 10.0,
            _ => 8.0,
        };
        let tone = if distance == Some(0) { 0.6 } else { 0.25 };
        strips = strips.child(el().absolute(0.0, top(index, n, rail_h) - 1.0).w(w).h(2.0).radius(radius::FULL).fill(t.c.muted_foreground.alpha_mul(tone)));
    }
    if !persistent && active.is_none() {
        strips = strips.group_opacity(0.0, 1.0);
    }
    let shown = active.and_then(|i| items.get(i));
    let label = format!("Jump to message: {}", shown.and_then(|i| i.user.as_deref()).unwrap_or("User message"));
    let (press_items, press_list) = (items.clone(), list.clone());
    let rail = strips
        .key("timeline-minimap")
        .focusable(true)
        .role("button")
        .label(label)
        .cursor(rune::Cursor::Pointer)
        .on_pointer_move_local(move |app: &mut App, p, tx: &mut Tx| {
            let index = index_at(p.y, rail_h, n);
            let state = &mut app.ui.chat.timeline.minimap;
            if state.active == index {
                tx.live_only();
            } else {
                state.active = index;
            }
        })
        .on_press_tx(move |app: &mut App, tx| jump(app, tx, &press_list, &press_items))
        .on_key(move |app: &mut App, e: &KeyEvent, _: &mut Tx| {
            let state = &mut app.ui.chat.timeline.minimap;
            let base = state.active.unwrap_or(0);
            state.active = Some(match e.key {
                Key::Down => (base + 1).min(n - 1),
                Key::Up => base.saturating_sub(1),
                Key::Home => 0,
                Key::End => n - 1,
                _ => return false,
            });
            true
        });

    let mut root = el()
        .pin(Some(STRIP_LEFT), Some(((body_h - rail_h) / 2.0).max(0.0)), None, None)
        .w(hit)
        .h(rail_h)
        .hover_group()
        .on_hover_change(|app: &mut App, inside, _: &mut Tx| {
            if !inside {
                app.ui.chat.timeline.minimap.active = None;
            }
        })
        .child(rail);
    if let Some(index) = active {
        let item = &items[index];
        let reply = item
            .reply
            .as_deref()
            .and_then(|id| reply_text(&app.state().thread_views.get(thread)?.projection, id))
            .and_then(|md| compact(&plain(md)));
        let card = col()
            .w(320.0)
            .padding(12.0)
            .child(text(item.user.clone().unwrap_or_else(|| "User message".into())).font(theme::SM_MEDIUM).color(t.c.popover_foreground).truncate())
            .children(reply.map(|r| text(r).font(theme::TEXT_SM).color(t.c.muted_foreground).max_lines(3).selectable(true).mt(4.0)));
        let card = crate::kit::menu::glass(card, &crate::kit::theme(t), false).radius(radius::XL);
        // The first preview hangs below its strip, the last above, the rest centred on it.
        let anchor = col().absolute(32.0, top(index, n, rail_h)).w(320.0).h(0.0);
        let anchor = if index == 0 {
            anchor.justify_start()
        } else if index == n - 1 {
            anchor.justify_end()
        } else {
            anchor.justify_center()
        };
        root = root.child(anchor.child(card));
    }
    Some(root)
}

#[cfg(test)]
mod tests {
    use super::{Item, bootstrap, derive, index_at, plain, strip_tags, top};
    use crate::chat::timeline::rows::Row;
    use serde_json::json;

    #[test]
    fn each_prompt_is_an_item_with_its_turns_last_reply() {
        let (u1, a1, a2, u2, u3) = (
            json!({"id": "u1", "text": "Fix the\n  retry loop"}),
            json!({"id": "a1", "text": "Looking"}),
            json!({"id": "a2", "text": "Done"}),
            json!({"id": "u2", "text": format!("Ultrathink:\n{}", super::BOOTSTRAP_PROMPT)}),
            json!({"id": "u3", "text": "   "}),
        );
        let rows = vec![Row::User(&u1), Row::Assistant(&a1), Row::Thinking { id: "x".into() }, Row::Assistant(&a2), Row::User(&u2), Row::User(&u3)];
        let keys: Vec<String> = ["k1", "k2", "k3", "k4", "k5", "k6"].map(String::from).to_vec();
        let item = |key: &str, user: Option<&str>, reply: Option<&str>| Item { key: key.into(), user: user.map(Into::into), reply: reply.map(Into::into) };
        assert_eq!(derive(&rows, &keys), vec![item("k1", Some("Fix the retry loop"), Some("a2")), item("k5", None, None), item("k6", None, None)]);
        assert!(bootstrap(super::BOOTSTRAP_PROMPT) && !bootstrap("Ultrathink: hi"));
    }

    #[test]
    fn previews_read_replies_as_prose() {
        assert_eq!(plain("## Done\n\nI added **clamp** and `sum`.").split_whitespace().collect::<Vec<_>>().join(" "), "Done I added clamp and sum.");
        let threads = plain("Started <t3-thread://v2/env-1/thread-1> and [Fix CI](t3-thread://v2/env-1/t2).");
        assert_eq!(threads.trim(), "Started a thread and Fix CI.");
        assert_eq!(strip_tags("a<!-- note -->b<div>c</div><span>d</span> 1 < 2"), "ab c d 1 < 2");
    }

    #[test]
    fn the_pointer_picks_the_nearest_strip() {
        assert_eq!(index_at(0.0, 32.0, 5), Some(0));
        assert_eq!(index_at(13.0, 32.0, 5), Some(2));
        assert_eq!(index_at(99.0, 32.0, 5), Some(4));
        assert_eq!(index_at(5.0, 0.0, 1), Some(0));
        assert_eq!(index_at(5.0, 32.0, 0), None);
        assert_eq!((top(0, 5, 32.0), top(4, 5, 32.0), top(1, 1, 32.0)), (0.0, 32.0, 0.0));
    }
}
