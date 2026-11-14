//! "Load earlier turns" (TimelineHistoryControl): a bounded thread snapshot pages older rows in.
use crate::data::s;
use crate::theme::{self, AlphaMul, T};
use crate::App;
use rune::{El, col, el, row, text};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct State {
    /// History requests in flight: thread and the cursor each was sent with.
    pending: HashMap<u64, (String, String)>,
    pub(super) errors: HashMap<String, String>,
    /// Pages of deferred work in flight.
    work: HashMap<u64, Work>,
    /// Anchors whose deferred work failed to load; their fold offers a retry.
    pub(super) failed_work: HashSet<String>,
}

struct Work {
    thread: String,
    anchor: String,
    cursor: String,
    /// The fold head that holds still as the page lands below it.
    head: String,
}

/// How many of the first rows already shown may anchor a landed page.
const ANCHORS: usize = 50;

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

pub fn poll(app: &mut App, tx: &mut rune::Tx) -> bool {
    sync_work(app);
    let changed = poll_work(app, tx);
    changed | poll_pages(app, tx)
}

fn poll_pages(app: &mut App, tx: &mut rune::Tx) -> bool {
    if app.ui.chat.timeline.history.pending.is_empty() {
        return false;
    }
    let now = app.now_ms;
    let Some(net) = app.src.net() else { return false };
    let state = &mut app.ui.chat.timeline.history;
    let mut changed = false;
    state.pending.retain(|rpc, (thread, cursor)| {
        let Some(reply) = net.take_reply(*rpc) else { return true };
        changed = true;
        match reply {
            Ok(page) => {
                // Rows already shown hold their place on screen as the page lands above them.
                // Any mounted one anchors the rest; the reader is near the top, by the control.
                let held = net.state().thread_views.get(thread.as_str()).map(|v| super::row_keys(&super::rows::derive(&v.projection, now))).unwrap_or_default();
                net.state_mut().merge_history(thread, cursor, &page);
                for key in held.into_iter().take(ANCHORS) {
                    tx.keep_on_screen(key);
                }
            }
            Err(e) => {
                let message = Some(s(super::link::failure(&e), "message")).filter(|m| !m.is_empty()).map_or_else(|| "Could not load earlier turns.".to_owned(), str::to_owned);
                state.errors.insert(std::mem::take(thread), message);
            }
        }
        false
    });
    changed
}

/// A fold's items that stand in for work the server deferred (`deferredWorkItemIds`).
pub fn work_anchors(items: &[&Value]) -> Vec<String> {
    items.iter().filter(|i| i["deferredWork"].is_object()).map(|i| s(i, "id").to_owned()).collect()
}

#[derive(Clone, Copy, PartialEq)]
pub enum WorkLoad {
    Idle,
    Loading,
    Failed,
}

/// `useDeferredWorkLoading`: an open fold with deferred work loads it; a failure shows until retried.
pub fn work_load(app: &App, anchors: &[String], expanded: bool) -> WorkLoad {
    let failed = &app.ui.chat.timeline.history.failed_work;
    if anchors.is_empty() {
        WorkLoad::Idle
    } else if anchors.iter().any(|a| failed.contains(a)) {
        WorkLoad::Failed
    } else if expanded {
        WorkLoad::Loading
    } else {
        WorkLoad::Idle
    }
}

/// Forgets the fold's failures, so an open fold loads again; true when there were any.
pub fn clear_failed_work(app: &mut App, anchors: &[String]) -> bool {
    let failed = &mut app.ui.chat.timeline.history.failed_work;
    anchors.iter().fold(false, |any, a| failed.remove(a) | any)
}

/// Starts loading the deferred work of every open fold in the thread on screen.
fn sync_work(app: &mut App) {
    let ui = &app.ui.chat.timeline;
    if !ui.expanded.iter().any(|k| k.starts_with("work:")) {
        return;
    }
    let crate::Route::Thread(thread) = app.frame_route() else { return };
    let Some(view) = app.state().thread_views.get(thread) else { return };
    let due: Vec<(String, String)> = view.projection["visibleTurnItems"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| &r["item"])
        .filter(|i| i["deferredWork"].is_object())
        .filter_map(|i| {
            let (anchor, run) = (s(i, "id"), s(i, "runId"));
            let open = !run.is_empty() && ui.expanded.contains(&format!("work:{run}"));
            let idle = !ui.history.failed_work.contains(anchor) && !ui.history.work.values().any(|w| w.anchor == anchor);
            (open && idle).then(|| (anchor.to_owned(), format!("fold-head-work:{run}")))
        })
        .collect();
    if due.is_empty() {
        return;
    }
    let thread = thread.clone();
    let Some(net) = app.src.net() else { return };
    let state = &mut app.ui.chat.timeline.history;
    for (anchor, head) in due {
        start_work(net, state, &thread, anchor, head);
    }
}

fn start_work(net: &mut s5_net::Net, state: &mut State, thread: &str, anchor: String, head: String) {
    if let Some((rpc, cursor)) = net.load_work(thread, &anchor) {
        state.work.insert(rpc, Work { thread: thread.to_owned(), anchor, cursor, head });
    }
}

/// Lands deferred-work pages and asks for the next one until the anchor's work is all shown.
fn poll_work(app: &mut App, tx: &mut rune::Tx) -> bool {
    if app.ui.chat.timeline.history.work.is_empty() {
        return false;
    }
    let Some(net) = app.src.net() else { return false };
    let state = &mut app.ui.chat.timeline.history;
    let mut changed = false;
    let mut more = Vec::new();
    state.work.retain(|rpc, work| {
        let Some(reply) = net.take_reply(*rpc) else { return true };
        changed = true;
        tx.keep_on_screen(work.head.clone());
        let merged = reply.map_err(|_| ()).and_then(|page| net.state_mut().merge_work(&work.thread, &work.anchor, &work.cursor, &page).map_err(|_| ()));
        match merged {
            Ok(true) => more.push((std::mem::take(&mut work.thread), std::mem::take(&mut work.anchor), std::mem::take(&mut work.head))),
            Ok(false) => {}
            Err(()) => {
                state.failed_work.insert(std::mem::take(&mut work.anchor));
            }
        }
        false
    });
    for (thread, anchor, head) in more {
        start_work(net, state, &thread, anchor, head);
    }
    changed
}

/// `WorkHistorySkeleton`: three placeholder lines under an open fold whose work is loading.
pub fn work_skeleton(t: &T) -> El {
    let mut out = col().w_full().gap(8.0).px(4.0).pt(12.0).pb(4.0).role("status").label("Loading work history");
    for width in [0.4, 0.6, 1.0 / 3.0] {
        out = out.child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(el().size(12.0, 12.0).radius(4.0).fill(t.c.accent))
                .child(el().w_frac(width).h(10.0).radius(5.0).fill(t.c.accent)),
        );
    }
    out
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
