//! "Load earlier turns" (TimelineHistoryControl): a bounded thread snapshot pages older rows in.
use crate::data::s;
use crate::theme::{self, AlphaMul, T};
use crate::App;
use rune::{El, col, el, row, text};
use std::collections::HashMap;

#[derive(Default)]
pub struct State {
    /// History requests in flight: thread and the cursor each was sent with.
    pending: HashMap<u64, (String, String)>,
    pub(super) errors: HashMap<String, String>,
}

fn loading(app: &App, thread: &str) -> bool {
    app.ui.chat.timeline.history.pending.values().any(|(t, _)| t == thread)
}

pub fn load(app: &mut App, thread: &str) {
    if loading(app, thread) {
        return;
    }
    let Some(net) = app.src.net() else { return };
    let Some((rpc, cursor)) = net.load_earlier(thread) else { return };
    let state = &mut app.ui.chat.timeline.history;
    state.errors.remove(thread);
    state.pending.insert(rpc, (thread.to_owned(), cursor));
}

pub fn poll(app: &mut App) -> bool {
    if app.ui.chat.timeline.history.pending.is_empty() {
        return false;
    }
    let Some(net) = app.src.net() else { return false };
    let state = &mut app.ui.chat.timeline.history;
    let mut changed = false;
    state.pending.retain(|rpc, (thread, cursor)| {
        let Some(reply) = net.take_reply(*rpc) else { return true };
        changed = true;
        match reply {
            Ok(page) => net.state_mut().merge_history(thread, cursor, &page),
            Err(e) => {
                let message = Some(s(super::link::failure(&e), "message")).filter(|m| !m.is_empty()).map_or_else(|| "Could not load earlier turns.".to_owned(), str::to_owned);
                state.errors.insert(std::mem::take(thread), message);
            }
        }
        false
    });
    changed
}

pub fn view(app: &App, thread: &str, t: &T) -> Option<El> {
    let more = app.state().thread_views.get(thread).is_some_and(|v| v.has_more_history);
    let error = app.ui.chat.timeline.history.errors.get(thread);
    if !more && error.is_none() {
        return None;
    }
    let mut out = col().w_full().gap(6.0).pb(8.0);
    if more {
        let busy = loading(app, thread);
        let line = || el().h(1.0).grow(1.0).fill(t.c.border.alpha_mul(0.7));
        let mut control = row()
            .key("timeline-load-earlier")
            .w_full()
            .gap(12.0)
            .py(6.0)
            .center_y()
            .role("button")
            .label("Load earlier turns")
            .child(line())
            .child(text(if busy { "Loading earlier turns…" } else { "Load earlier turns" }).font(theme::TEXT_XS).color(t.c.muted_foreground))
            .child(line());
        if !busy {
            let id = thread.to_owned();
            control = control.focusable(true).cursor(rune::Cursor::Pointer).on_press(move |app: &mut App| load(app, &id));
        }
        out = out.child(control);
    }
    if let Some(error) = error {
        out = out.child(row().w_full().justify_center().child(text(error.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground)));
    }
    Some(out)
}
