//! What the list shows before the server does, and where it stands: messages sent but not yet
//! echoed (optimisticUserMessages), the new turn anchored near the top (anchorNewTurn), and each
//! thread's scroll position kept across switches (useTimelinePositionRestore).

use std::cell::RefCell;
use std::rc::Rc;

use rune::Cx;
use serde_json::{Value, json};

use crate::App;
use crate::data::s;

/// CHAT_TIMELINE_ANCHOR_OFFSET: how far below the list's top a new turn's message sits.
pub const ANCHOR_OFFSET: f32 = 24.0;
/// The zero-height row after the list's last row, which the anchor measures its turn to.
pub const END: &str = "timeline-anchor-end";
/// Sends shown ahead of their echo, across threads, before the oldest is let go.
const MAX_MESSAGES: usize = 32;
/// Threads whose position is remembered (rememberTimelinePosition keeps 100).
const MAX_POSITIONS: usize = 100;

#[derive(Default)]
pub struct State {
    /// Each optimistic user message with the thread it was sent to.
    messages: Vec<(String, Value)>,
    /// The thread and message id the new turn anchors on.
    anchor: Option<(String, String)>,
    /// A thread whose list goes to its end once the sent message shows (followEnd).
    reveal: Option<String>,
    /// Written from the list's scroll callback, which cannot reach the app.
    positions: Rc<RefCell<Positions>>,
}

/// The user message a send shows until the server echoes it.
pub fn message(id: &str, text: &str, contexts: &[Value], attachments: Value, steer: bool) -> Value {
    let mut message = json!({
        "type": "user_message", "id": id, "messageId": id, "text": text, "status": "pending",
        "createdBy": "user", "attachments": attachments,
    });
    if !contexts.is_empty() {
        message["context"] = json!({"version": 1, "records": contexts});
    }
    if steer {
        message["inputIntent"] = json!("steer");
    }
    message
}

/// Shows `message` in `thread` at once and anchors the new turn on it.
pub fn show(app: &mut App, thread: &str, message: Value) {
    let state = &mut app.ui.chat.timeline.pending;
    let id = s(&message, "id").to_owned();
    state.messages.push((thread.to_owned(), message));
    if state.messages.len() > MAX_MESSAGES {
        state.messages.remove(0);
    }
    state.anchor = Some((thread.to_owned(), id));
    state.reveal = Some(thread.to_owned());
}

/// A send to `thread` failed: its messages go, and the draft has them back.
pub fn drop_thread(app: &mut App, thread: &str) {
    let state = &mut app.ui.chat.timeline.pending;
    state.messages.retain(|(t, _)| t != thread);
    if state.anchor.as_ref().is_some_and(|(t, _)| t == thread) {
        state.anchor = None;
    }
}

/// Whether the projection holds the user message `id` sent.
fn echoed(p: &Value, id: &str) -> bool {
    super::lifecycle::items(p).iter().any(|i| s(i, "type") == "user_message" && (s(i, "messageId") == id || s(i, "id") == id))
}

/// `thread`'s optimistic messages the server has not echoed yet.
pub fn shown<'a>(app: &'a App, thread: &str, p: &Value) -> Vec<&'a Value> {
    let messages = &app.ui.chat.timeline.pending.messages;
    if messages.is_empty() {
        return Vec::new();
    }
    messages.iter().filter(|(t, m)| t == thread && !echoed(p, s(m, "id"))).map(|(_, m)| m).collect()
}

/// The message `thread`'s new turn anchors on.
pub fn anchor<'a>(app: &'a App, thread: &str) -> Option<&'a str> {
    app.ui.chat.timeline.pending.anchor.as_ref().filter(|(t, _)| t == thread).map(|(_, id)| id.as_str())
}

/// Room after the turn so the anchored message can sit ANCHOR_OFFSET below the list's top at its
/// end: the viewport less the composer's clearance, the offset and the turn so far.
fn space(viewport: f32, clearance: f32, anchor_top: f32, end_top: f32) -> f32 {
    (viewport - clearance - ANCHOR_OFFSET - (end_top - anchor_top)).max(0.0)
}

/// The anchored turn's end space, from the last layout. None once its message has left the
/// mounted rows, so a long turn stops reading the end row.
pub fn end_space(cx: &mut Cx<App>, list: &str, anchor_row: &str, clearance: f32) -> Option<f32> {
    let top = cx.layout_with(anchor_row, |r| r[1].round())?;
    let end = cx.layout_with(END, |r| r[1].round())?;
    let viewport = cx.layout_with(list, |r| r[3].round())?;
    Some(space(viewport, clearance, top, end))
}

/// The routed list's scroll callback: where `thread`'s list is.
pub fn reporter(app: &App, thread: &str) -> impl Fn(rune::ScrollPos) + 'static {
    let (positions, thread) = (app.ui.chat.timeline.pending.positions.clone(), thread.to_owned());
    move |at: rune::ScrollPos| positions.borrow_mut().report(&thread, at.offset[1], at.max[1] - at.offset[1] <= 1.0)
}

/// Lets echoed messages go, follows a sent message to the list's end, drops another thread's
/// anchor, and puts a reopened thread's list back where it was left.
pub fn poll(app: &mut App, tx: &mut rune::Tx) -> bool {
    let routed = match app.frame_route() {
        crate::Route::Thread(id) => Some(id.clone()),
        _ => None,
    };
    let mut changed = false;
    if !app.ui.chat.timeline.pending.messages.is_empty() {
        let views = &app.state().thread_views;
        let echoes: Vec<bool> = app.ui.chat.timeline.pending.messages.iter().map(|(t, m)| views.get(t).is_some_and(|v| echoed(&v.projection, s(m, "id")))).collect();
        if echoes.contains(&true) {
            let mut echoes = echoes.into_iter();
            app.ui.chat.timeline.pending.messages.retain(|_| !echoes.next().unwrap_or(false));
            changed = true;
        }
    }
    let state = &mut app.ui.chat.timeline.pending;
    if state.anchor.as_ref().is_some_and(|(t, _)| Some(t) != routed.as_ref()) {
        state.anchor = None;
        changed = true;
    }
    if let Some(thread) = state.reveal.take()
        && Some(&thread) == routed.as_ref()
    {
        tx.scroll_to_bottom(format!("timeline-{thread}"));
    }
    let positions = state.positions.clone();
    let mut positions = positions.borrow_mut();
    positions.left_for(routed.as_deref());
    if let Some((thread, y)) = positions.restore.clone() {
        if Some(&thread) != routed.as_ref() {
            positions.restore = None;
        } else if app.state().thread_views.get(&thread).is_some_and(|v| v.synchronized) {
            tx.scroll_to_y(format!("timeline-{thread}"), y);
            positions.restore = None;
        }
    }
    changed
}

/// Each thread's scroll offset, kept when its list goes and given back when it returns.
#[derive(Default)]
pub struct Positions {
    /// The thread whose list reported last, with its offset and whether it was at the end.
    current: String,
    offset: f32,
    at_end: bool,
    /// Left lists' offsets, oldest first. One left at its end needs none: lists open there.
    saved: Vec<(String, f32)>,
    /// An offset to put back once that thread's list has its rows.
    restore: Option<(String, f32)>,
}

impl Positions {
    fn report(&mut self, thread: &str, offset: f32, at_end: bool) {
        if self.current != thread {
            let left = std::mem::replace(&mut self.current, thread.to_owned());
            self.remember(left);
            self.restore = self.saved.iter().position(|(t, _)| t == thread).map(|at| self.saved.remove(at));
        }
        self.offset = offset;
        self.at_end = at_end;
    }

    /// `routed` is the thread whose list shows; any other list that reported has gone.
    fn left_for(&mut self, routed: Option<&str>) {
        if !self.current.is_empty() && routed != Some(self.current.as_str()) {
            let left = std::mem::take(&mut self.current);
            self.remember(left);
        }
    }

    fn remember(&mut self, thread: String) {
        if thread.is_empty() {
            return;
        }
        self.saved.retain(|(t, _)| *t != thread);
        if !self.at_end {
            self.saved.push((thread, self.offset));
            if self.saved.len() > MAX_POSITIONS {
                self.saved.remove(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projection(items: Value) -> Value {
        json!({"turnItems": items})
    }

    #[test]
    fn an_optimistic_message_goes_once_the_server_echoes_its_id() {
        let mut app = App::new();
        let sent = message("m1", "hello", &[json!({"contextId": "c"})], json!([]), true);
        assert_eq!(sent["context"]["records"][0]["contextId"], "c");
        assert_eq!(sent["inputIntent"], "steer");
        show(&mut app, "t1", sent);
        show(&mut app, "t2", message("m2", "other", &[], json!([]), false));
        assert_eq!(anchor(&app, "t2"), Some("m2"), "the last send anchors");
        assert_eq!(anchor(&app, "t1"), None);
        let waiting = projection(json!([{"type": "user_message", "id": "m0", "text": "earlier"}]));
        let shown_ids = |app: &App, p: &Value| shown(app, "t1", p).iter().map(|m| s(m, "id").to_owned()).collect::<Vec<_>>();
        assert_eq!(shown_ids(&app, &waiting), ["m1"]);
        let echoed_by_message_id = projection(json!([{"type": "user_message", "id": "item-9", "messageId": "m1", "text": "hello"}]));
        assert!(shown_ids(&app, &echoed_by_message_id).is_empty(), "deduped by the client messageId");
        let echoed_by_id = projection(json!([{"type": "user_message", "id": "m1", "text": "hello"}]));
        assert!(shown_ids(&app, &echoed_by_id).is_empty());
    }

    #[test]
    fn a_failed_send_drops_its_threads_messages_and_anchor() {
        let mut app = App::new();
        show(&mut app, "t1", message("m1", "a", &[], json!([]), false));
        show(&mut app, "t2", message("m2", "b", &[], json!([]), false));
        drop_thread(&mut app, "t2");
        assert_eq!(anchor(&app, "t2"), None);
        assert!(shown(&app, "t2", &projection(json!([]))).is_empty());
        assert_eq!(shown(&app, "t1", &projection(json!([]))).len(), 1, "other threads keep theirs");
        for n in 0..MAX_MESSAGES + 5 {
            show(&mut app, "t3", message(&format!("x{n}"), "x", &[], json!([]), false));
        }
        assert_eq!(app.ui.chat.timeline.pending.messages.len(), MAX_MESSAGES);
        assert!(shown(&app, "t1", &projection(json!([]))).is_empty(), "the oldest went first");
    }

    #[test]
    fn the_end_space_lands_the_anchor_below_the_top() {
        // A 600pt list, 150pt of composer clearance, and a 100pt turn so far.
        assert_eq!(space(600.0, 150.0, 1000.0, 1100.0), 600.0 - 150.0 - ANCHOR_OFFSET - 100.0);
        assert_eq!(space(600.0, 150.0, 1000.0, 2000.0), 0.0, "a turn taller than the view needs none");
    }

    #[test]
    fn positions_come_back_per_thread_and_stay_bounded() {
        let mut p = Positions::default();
        p.report("a", 0.0, false);
        p.report("a", 340.0, false);
        p.report("b", 0.0, true);
        assert_eq!(p.saved, [("a".to_owned(), 340.0)]);
        assert_eq!(p.restore, None, "b was never left");
        p.left_for(None);
        assert_eq!(p.saved.len(), 1, "b was at its end, which needs no position");
        p.report("a", 9999.0, true);
        assert_eq!(p.restore, Some(("a".to_owned(), 340.0)));
        assert!(p.saved.is_empty(), "taken back");
        for n in 0..MAX_POSITIONS + 10 {
            p.report(&format!("t{n}"), 50.0, false);
        }
        p.left_for(None);
        assert_eq!(p.saved.len(), MAX_POSITIONS);
        assert_eq!(p.saved[0].0, "t10", "the oldest went first");
        assert_eq!(p.saved.last().map(|(t, _)| t.as_str()), Some("t109"));
    }
}
