//! Follow-ups queued behind the active run (QueuedRunsControl, deriveThreadQueueWorkflowState):
//! dashed bubbles at the end of the timeline with Send now, Edit and Remove.

use rune::{El, col, icon, row, text};
use serde_json::{Value, json};

use super::rows::array;
use crate::data::s;
use crate::kit::Button;
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};

/// A longer queue shows its first messages and a button that shows the rest.
const VISIBLE_ROWS: usize = 3;
const ACTIVE: [&str; 4] = ["preparing", "starting", "running", "waiting"];

pub struct Queued<'a> {
    pub run: &'a str,
    pub text: &'a str,
    /// A guest's message keeps the guest's words; the owner can cancel it, not rewrite it.
    pub editable: bool,
}

pub struct Workflow<'a> {
    pub queued: Vec<Queued<'a>>,
    pub active_run: Option<&'a str>,
    pub can_send_now: bool,
    pub can_promote: bool,
    /// The active run is a compaction, so promoting stops it instead of steering.
    pub skips_compaction: bool,
}

impl Workflow<'_> {
    pub fn can_send(&self) -> bool {
        self.can_send_now || self.can_promote
    }
}

fn compaction(message: &Value) -> bool {
    s(message, "role") == "user"
        && s(message, "text").trim().eq_ignore_ascii_case("/compact")
        && array(message, "attachments").is_empty()
}

fn session<'a>(p: &'a Value, active: Option<&'a Value>) -> Option<&'a Value> {
    let (threads, sessions, thread) = (array(p, "providerThreads"), array(p, "providerSessions"), &p["thread"]);
    let wanted = active.map(|r| s(r, "providerThreadId")).filter(|id| !id.is_empty()).unwrap_or(s(thread, "activeProviderThreadId"));
    let attached = threads
        .iter()
        .find(|t| !wanted.is_empty() && s(t, "id") == wanted)
        .or_else(|| threads.iter().find(|t| s(t, "appThreadId") == s(thread, "id") && !s(t, "providerSessionId").is_empty()));
    match attached.map(|t| s(t, "providerSessionId")).filter(|id| !id.is_empty()) {
        Some(id) => sessions.iter().find(|x| s(x, "id") == id),
        None => sessions.iter().rev().find(|x| !matches!(s(x, "status"), "stopped" | "error")),
    }
}

pub fn workflow(p: &Value) -> Workflow<'_> {
    let (runs, messages) = (array(p, "runs"), array(p, "messages"));
    let message = |id: &str| messages.iter().find(|m| s(m, "id") == id);
    let active = runs.iter().rev().find(|r| ACTIVE.contains(&s(r, "status")));
    let turns = &session(p, active).unwrap_or(&Value::Null)["capabilities"]["turns"];
    let skips_compaction = active.and_then(|r| message(s(r, "userMessageId"))).is_some_and(compaction);
    let steerable = active.is_some_and(|r| {
        let attempt = s(r, "activeAttemptId");
        s(r, "status") == "running"
            && !attempt.is_empty()
            && array(p, "providerTurns").iter().any(|t| s(t, "runAttemptId") == attempt && s(t, "status") == "running")
    });
    let automatic = |m: &Value| !m["delegatedCompletion"].is_null() || !m["notification"].is_null();
    let mut queued: Vec<&Value> =
        runs.iter().filter(|r| s(r, "status") == "queued" && !message(s(r, "userMessageId")).is_some_and(automatic)).collect();
    let ordinal = |r: &Value| r["ordinal"].as_f64().unwrap_or(0.0);
    let position = |r: &Value| r["queuePosition"].as_f64().unwrap_or_else(|| ordinal(r));
    queued.sort_by(|a, b| position(a).total_cmp(&position(b)).then(ordinal(a).total_cmp(&ordinal(b))));
    let thread = &p["thread"];
    Workflow {
        queued: queued
            .into_iter()
            .map(|r| {
                let m = message(s(r, "userMessageId"));
                let guest = m.map(|m| &m["context"]["guest"]).filter(|g| !g.is_null());
                let text = guest.map(|g| s(g, "text")).or(m.map(|m| s(m, "text"))).unwrap_or("Queued message");
                Queued { run: s(r, "id"), text, editable: guest.is_none() }
            })
            .collect(),
        active_run: active.map(|r| s(r, "id")),
        can_send_now: active.is_none() && thread["archivedAt"].is_null() && thread["deletedAt"].is_null(),
        can_promote: skips_compaction
            || (steerable && (turns["supportsActiveSteering"] == true || turns["supportsSteeringByInterruptRestart"] == true)),
        skips_compaction,
    }
}

fn projection<'a>(app: &'a App, thread_id: &str) -> Option<&'a Value> {
    app.state().thread_views.get(thread_id).map(|v| &v.projection)
}

/// Starts a queued message now, or folds it into the running turn.
pub fn send_now(app: &mut App, thread_id: &str, run_id: &str) {
    let Some(p) = projection(app, thread_id) else { return };
    let w = workflow(p);
    let cmd = if w.can_send_now {
        json!({"type": "queued-run.start", "commandId": s5_net::uuid(), "threadId": thread_id, "runId": run_id})
    } else if let (true, Some(target)) = (w.can_promote, w.active_run) {
        json!({"type": "queued-message.promote-to-steer", "commandId": s5_net::uuid(), "threadId": thread_id, "queuedRunId": run_id, "targetRunId": target})
    } else {
        return;
    };
    if let Some(net) = app.src.net() {
        net.dispatch(cmd);
    }
}

fn remove(app: &mut App, thread_id: &str, run_id: &str) {
    if crate::chat::composer::editing_run(app) == Some(run_id) {
        crate::chat::composer::cancel_queued_edit(app);
    }
    if let Some(net) = app.src.net() {
        net.dispatch(json!({"type": "queued-run.cancel", "commandId": s5_net::uuid(), "threadId": thread_id, "runId": run_id}));
    }
}

/// Mod+Shift+Enter: sends the first queued message. False when there is none to send.
pub fn steer_next(app: &mut App, thread_id: &str) -> bool {
    let Some(p) = projection(app, thread_id) else { return false };
    let w = workflow(p);
    let Some(next) = w.queued.first().filter(|_| w.can_send()).map(|q| q.run.to_owned()) else { return false };
    send_now(app, thread_id, &next);
    true
}

/// Alt+Up: edits the newest queued message in the composer.
pub fn edit_latest(app: &mut App, thread_id: &str) -> bool {
    let Some(p) = projection(app, thread_id) else { return false };
    let Some(latest) = workflow(p).queued.pop().filter(|q| q.editable).map(|q| (q.run.to_owned(), q.text.to_owned())) else {
        return false;
    };
    if crate::chat::composer::editing_run(app).is_some() {
        return false;
    }
    app.ui.chat.timeline.expanded.insert(format!("queue-all-{thread_id}"));
    crate::chat::composer::edit_queued(app, thread_id, &latest.0, &latest.1);
    true
}

pub fn view(app: &App, thread_id: &str, t: &T, max_w: f32) -> Option<El> {
    let w = workflow(projection(app, thread_id)?);
    if w.queued.is_empty() {
        return None;
    }
    let editing = crate::chat::composer::editing_run(app);
    let blocked = super::question::active(app).is_some();
    let all_key = format!("queue-all-{thread_id}");
    let show_all = app.ui.chat.timeline.expanded.contains(&all_key);
    let count = w.queued.len();
    let collapsible = count > VISIBLE_ROWS;
    let visible = if collapsible && !show_all { VISIBLE_ROWS - 1 } else { count };
    let send_help = if w.can_send_now {
        "Start a turn with this message"
    } else if w.skips_compaction {
        "Skip compaction and send this message"
    } else {
        "Send into the current turn"
    };
    let mut list = col().w_full().gap(12.0).items_end();
    for (index, q) in w.queued.iter().take(visible).enumerate() {
        let is_editing = editing == Some(q.run);
        let border = if is_editing { t.c.primary.alpha_mul(0.6) } else { t.c.muted_foreground.alpha_mul(0.4) };
        let bubble = col()
            .max_w(max_w * 0.8)
            .px(12.0)
            .py(8.0)
            .radius(16.0)
            .border(1.0, border)
            .border_dash(4.0, 3.0)
            .child(
                text(crate::chat::composer::chips::display(q.text).0)
                    .font(theme::TEXT_SM.lh(22.75))
                    .color(if is_editing { t.c.foreground } else { t.c.muted_foreground })
                    .selectable(true),
            );
        let mut controls = row().gap(2.0).center_y();
        let (thread, run) = (thread_id.to_owned(), q.run.to_owned());
        if w.can_send() && !is_editing {
            let (thread, run) = (thread.clone(), run.clone());
            let help = if index == 0 { format!("{send_help} (Ctrl+Shift+Enter)") } else { send_help.to_owned() };
            controls = controls.child(
                Button::new("Send now")
                    .variant("ghost-muted")
                    .size("xs")
                    .icon(&icons::ARROW_UP)
                    .on_press(move |app: &mut App| send_now(app, &thread, &run))
                    .build(t)
                    .help(help),
            );
        }
        if is_editing {
            controls = controls
                .child(text("Editing in the composer").font(theme::TEXT_XS).color(t.c.foreground).px(4.0))
                .child(
                    Button::new("Cancel")
                        .variant("ghost")
                        .size("xs")
                        .on_press(|app: &mut App| crate::chat::composer::cancel_queued_edit(app))
                        .build(t),
                );
        } else {
            let (edit_thread, edit_run, edit_text) = (thread.clone(), run.clone(), q.text.to_owned());
            let help = if blocked {
                "Answer the question first".to_owned()
            } else if index + 1 == count {
                "Edit in the composer (Alt+Up)".to_owned()
            } else {
                "Edit in the composer".to_owned()
            };
            controls = controls
                .child(
                    Button::new("Edit queued message")
                        .variant("ghost-muted")
                        .size("icon-xs")
                        .icon(&icons::PENCIL)
                        .disabled(!q.editable || blocked)
                        .on_press(move |app: &mut App| crate::chat::composer::edit_queued(app, &edit_thread, &edit_run, &edit_text))
                        .build(t)
                        .help(help),
                )
                .child(
                    Button::new("Remove queued message")
                        .variant("ghost-muted")
                        .size("icon-xs")
                        .icon(&icons::X)
                        .on_press(move |app: &mut App| remove(app, &thread, &run))
                        .build(t)
                        .help("Remove from queue"),
                );
        }
        list = list.child(col().w_full().gap(4.0).items_end().child(bubble).child(controls));
    }
    let mut section = col().key(format!("thread-queue-{thread_id}")).w_full().gap(8.0).pb(16.0).label(format!(
        "{count} queued message{}",
        if count == 1 { "" } else { "s" }
    ));
    section = section.child(list);
    if collapsible {
        let label = if show_all { "Show fewer".to_owned() } else { format!("Show all {count} queued") };
        section = section.child(
            row()
                .w_full()
                .justify_end()
                .pr(4.0)
                .child(
                    row()
                        .gap(4.0)
                        .center_y()
                        .focusable(true)
                        .role("button")
                        .cursor(rune::Cursor::Pointer)
                        .child(text(label).font(theme::TEXT_XS).color(t.c.muted_foreground))
                        .child(icon(if show_all { &icons::CHEVRON_UP } else { &icons::CHEVRON_DOWN }, 14.0).color(t.c.muted_foreground))
                        .on_press(move |app: &mut App| super::toggle(app, &all_key)),
                ),
        );
    }
    Some(section)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs<'a>(w: &Workflow<'a>) -> Vec<&'a str> {
        w.queued.iter().map(|q| q.run).collect()
    }

    #[test]
    fn queue_orders_by_position_and_hides_automatic_messages() {
        let p = json!({
            "thread": {"id": "t"},
            "runs": [
                {"id": "a", "status": "queued", "ordinal": 3, "userMessageId": "ma"},
                {"id": "b", "status": "queued", "ordinal": 4, "queuePosition": 0, "userMessageId": "mb"},
                {"id": "c", "status": "queued", "ordinal": 5, "userMessageId": "mc"},
                {"id": "d", "status": "completed", "ordinal": 1, "userMessageId": "md"},
            ],
            "messages": [
                {"id": "ma", "role": "user", "text": "first"},
                {"id": "mb", "role": "user", "text": "stored", "context": {"guest": {"name": "Ann", "text": "guest words"}}},
                {"id": "mc", "role": "user", "text": "done", "delegatedCompletion": {}},
            ],
        });
        let w = workflow(&p);
        assert_eq!(runs(&w), ["b", "a"]);
        assert_eq!(w.queued[0].text, "guest words");
        assert!(!w.queued[0].editable && w.queued[1].editable);
        assert!(w.can_send_now && !w.can_promote);
    }

    #[test]
    fn promoting_needs_a_running_provider_turn_that_can_steer() {
        let p = |turn: &str, steering: bool| {
            json!({
                "thread": {"id": "t"},
                "runs": [
                    {"id": "r", "status": "running", "ordinal": 1, "activeAttemptId": "at", "providerThreadId": "pt", "userMessageId": "m"},
                    {"id": "q", "status": "queued", "ordinal": 2, "userMessageId": "mq"},
                ],
                "messages": [{"id": "m", "role": "user", "text": "go"}],
                "providerThreads": [{"id": "pt", "providerSessionId": "ps"}],
                "providerSessions": [{"id": "ps", "status": "running", "capabilities": {"turns": {"supportsActiveSteering": steering}}}],
                "providerTurns": [{"runAttemptId": "at", "status": turn}],
            })
        };
        let live = p("running", true);
        let w = workflow(&live);
        assert!(!w.can_send_now && w.can_promote);
        assert_eq!(w.active_run, Some("r"));
        assert!(!workflow(&p("completed", true)).can_promote);
        assert!(!workflow(&p("running", false)).can_promote);
        let mut compacting = p("completed", false);
        compacting["messages"][0]["text"] = json!(" /compact ");
        let w = workflow(&compacting);
        assert!(w.can_promote && w.skips_compaction);
    }
}
