//! A new worktree's setup progress (WorktreeSetupCard, useWorktreeSetup; client-runtime
//! worktreeSetup.ts): its stages streamed from `subscribeWorktreeSetup`, Cancel while it runs,
//! and Retry on the error a failed workspace preparation leaves in the timeline.

use std::collections::{HashMap, HashSet};

use rune::{El, col, icon, row, text};
use serde_json::{Value, json};

use super::rows::{array, duration};
use crate::data::{iso_ms, s};
use crate::kit::button::Button;
use crate::theme::{self, AlphaMul, T};
use crate::{App, Route, icons};

const METHOD: &str = "subscribeWorktreeSetup";
const TAIL_LINES: usize = 4;
/// ORCHESTRATION_V2_WORKSPACE_PREPARATION_FAILURE_CODE.
const PREPARATION_FAILED: &str = "workspace_preparation_failed";

#[derive(Default)]
pub struct State {
    /// Thread id -> the newest snapshot, kept after the stream drops it (useWorktreeSetup's
    /// heldSnapshot) until the projection catches up.
    pub(super) held: HashMap<String, Value>,
    /// The thread whose stream is open.
    watched: Option<String>,
    pub(super) details: HashSet<String>,
    /// Thread id -> `worktreeSetup.cancel` in flight.
    cancelling: HashMap<String, u64>,
    /// Run id -> `prepared-run.retry` in flight.
    retrying: HashMap<String, u64>,
}

fn stage_label(id: &str) -> &'static str {
    match id {
        "fetch" => "Fetch base branch",
        "checkout" => "Check out files",
        "submodules" => "Init submodules",
        "setup-script" => "Run setup script",
        _ => "Start agent",
    }
}

fn stage_failed(snapshot: &Value) -> bool {
    array(snapshot, "stages").iter().any(|st| s(st, "status") == "failed")
}

fn header_label(snapshot: &Value) -> &'static str {
    match s(snapshot, "phase") {
        "running" => "Setting up worktree…",
        "done" if stage_failed(snapshot) => "Worktree ready, setup script failed",
        "done" => "Worktree ready",
        "failed" => "Worktree setup failed",
        _ => "Worktree setup cancelled",
    }
}

/// resolveVisibleWorktreeSetup: a running setup always shows; a finished one until a follow-up,
/// and once the turn has started only when a stage failed.
fn visible(snapshot: &Value, turn_started: bool, follow_up_sent: bool) -> bool {
    match s(snapshot, "phase") {
        "running" => true,
        _ if follow_up_sent => false,
        "done" => !turn_started || stage_failed(snapshot),
        _ => true,
    }
}

/// The setup script's last lines, while it fails or prints.
fn tail(snapshot: &Value) -> Option<(Vec<String>, bool)> {
    let stage = array(snapshot, "stages").iter().find(|st| s(st, "id") == "setup-script")?;
    let lines = array(stage, "tail");
    let failed = s(stage, "status") == "failed";
    if !failed && !(s(stage, "status") == "running" && !lines.is_empty()) {
        return None;
    }
    let shown = lines.len().saturating_sub(TAIL_LINES);
    let mut out: Vec<String> = lines[shown..].iter().map(|l| l.as_str().unwrap_or_default().to_owned()).collect();
    while out.len() < TAIL_LINES {
        out.insert(0, String::new());
    }
    Some((out, failed))
}

/// workspacePreparationRetryRunIds: failed runs whose workspace preparation failed.
fn retryable_runs(p: &Value) -> HashSet<&str> {
    let failed: HashSet<&str> = array(p, "runs").iter().filter(|r| s(r, "status") == "failed" && !r["workspacePreparation"].is_null()).map(|r| s(r, "id")).collect();
    array(p, "turnItems")
        .iter()
        .filter(|i| s(i, "type") == "error" && s(i, "status") == "failed" && s(&i["failure"], "code") == PREPARATION_FAILED && failed.contains(s(i, "runId")))
        .map(|i| s(i, "runId"))
        .collect()
}

fn route_thread(app: &App) -> Option<String> {
    match app.frame_route() {
        Route::Thread(id) => Some(id.clone()),
        _ => None,
    }
}

pub fn poll(app: &mut App) -> bool {
    let thread = route_thread(app);
    // Only a thread with a worktree, or one preparing a run, has a setup to follow.
    let wants = thread.as_deref().is_some_and(|id| {
        app.state().threads.iter().find(|r| s(r, "id") == id).is_some_and(|r| !s(r, "worktreePath").is_empty() || s(r, "activityRunStatus") == "preparing" || s(r, "status") == "preparing")
    });
    let Some(net) = app.src.net() else { return false };
    let ui = &mut app.ui.chat.timeline.worktree;
    let mut changed = false;
    if ui.watched.is_some() && ui.watched != thread
        && let Some(old) = ui.watched.take()
    {
        net.unwatch_resource(METHOD, json!({"threadId": old}));
    }
    if let Some(id) = thread.filter(|_| wants) {
        let payload = json!({"threadId": id});
        net.watch_resource(METHOD, payload.clone());
        if let Some(snapshot) = net.resource(METHOD, payload)
            && s(snapshot, "threadId") == id
            && ui.held.get(&id).is_none_or(|held| held != snapshot && held["sequence"].as_u64() <= snapshot["sequence"].as_u64())
        {
            ui.held.insert(id.clone(), snapshot.clone());
            changed = true;
        }
        ui.watched = Some(id);
    }
    let mut failures = Vec::new();
    ui.cancelling.retain(|_, request| {
        let Some(reply) = net.take_reply(*request) else { return true };
        changed = true;
        if let Err(error) = reply {
            failures.push(("Could not cancel worktree setup", error));
        }
        false
    });
    ui.retrying.retain(|_, request| {
        let Some(reply) = net.take_reply(*request) else { return true };
        changed = true;
        if let Err(error) = reply {
            failures.push(("Could not retry worktree setup", error));
        }
        false
    });
    for (title, error) in failures {
        let description = crate::shell::palette::error_message(&error, "An error occurred.");
        crate::shell::palette::toast_error(app, title, Some(&description));
    }
    changed
}

fn cancel(app: &mut App, thread: &str) {
    if app.ui.chat.timeline.worktree.cancelling.contains_key(thread) {
        return;
    }
    let Some(net) = app.src.net() else { return };
    let request = net.request("worktreeSetup.cancel", json!({"threadId": thread}));
    app.ui.chat.timeline.worktree.cancelling.insert(thread.to_owned(), request);
}

fn retry(app: &mut App, thread: &str, run: &str) {
    if app.ui.chat.timeline.worktree.retrying.contains_key(run) {
        return;
    }
    let Some(net) = app.src.net() else { return };
    let request = net.dispatch(json!({"type": "prepared-run.retry", "threadId": thread, "runId": run}));
    app.ui.chat.timeline.worktree.retrying.insert(run.to_owned(), request);
}

/// Retry on the error row of a run whose workspace preparation failed.
pub fn retry_button(app: &App, item: &Value, t: &T) -> Option<El> {
    let thread = route_thread(app)?;
    let run = s(item, "runId");
    let view = app.state().thread_views.get(&thread)?;
    if s(item, "type") != "error" || !retryable_runs(&view.projection).contains(run) {
        return None;
    }
    let busy = app.ui.chat.timeline.worktree.retrying.contains_key(run);
    let run = run.to_owned();
    Some(
        row().grow(1.0).justify_end().child(
            Button::new("Retry")
                .variant("outline")
                .size("xs")
                .icon(&icons::ROTATE_CCW)
                .disabled(busy)
                .on_press(move |app: &mut App| retry(app, &thread, &run))
                .build(t),
        ),
    )
}

/// An elapsed time, live while the setup runs.
fn elapsed(started: &str, ended: &str, live: bool, now: i64) -> Option<String> {
    let start = iso_ms(started)?;
    let end = iso_ms(ended).or(live.then_some(now))?;
    Some(duration(start, end))
}

fn stage_icon(status: &str, t: &T) -> El {
    let glyph = match status {
        "running" => return crate::kit::Spinner::new().size("sm").build(&crate::kit::theme(t)),
        "done" => &icons::CHECK,
        "failed" => &icons::X,
        "warning" => &icons::CIRCLE_ALERT,
        "skipped" => &icons::MINUS,
        _ => &icons::CIRCLE,
    };
    icon(glyph, 14.0).color(t.c.icon_muted.alpha_mul(if status == "pending" { 0.4 } else { 1.0 }))
}

fn stage_color(status: &str, t: &T) -> rune::Color {
    match status {
        "failed" => t.c.error_foreground,
        "warning" => t.c.warning_foreground,
        "pending" => t.c.muted_foreground.alpha_mul(0.4),
        _ => t.c.muted_foreground,
    }
}

pub fn view(app: &App, thread: &str, t: &T) -> Option<El> {
    let snapshot = app.ui.chat.timeline.worktree.held.get(thread)?;
    let shell = app.state().threads.iter().find(|r| s(r, "id") == thread);
    let turn_started = shell.is_some_and(|r| !s(r, "latestRunStartedAt").is_empty());
    let follow_up = app.state().thread_views.get(thread).is_some_and(|v| array(&v.projection, "turnItems").iter().filter(|i| s(i, "type") == "user_message").count() > 1);
    if !visible(snapshot, turn_started, follow_up) {
        return None;
    }
    let running = s(snapshot, "phase") == "running";
    let now = app.now_ms;
    let mut card = col().w_full().label("Worktree setup");
    if !running && s(snapshot, "phase") != "done" {
        let tone = if s(snapshot, "phase") == "failed" { t.c.error_foreground } else { t.c.muted_foreground };
        card = card.child(
            row()
                .w_full()
                .h(24.0)
                .px(4.0)
                .mb(6.0)
                .gap(8.0)
                .center_y()
                .child(text(header_label(snapshot)).font(theme::TEXT_SM).color(tone).truncate().shrink().min_w(0.0))
                .child(rune::spacer())
                .children(elapsed(s(snapshot, "startedAt"), s(snapshot, "endedAt"), false, now).map(|e| text(e).font(theme::TEXT_XS).color(t.c.muted_foreground))),
        );
    }
    let script = &snapshot["setupScript"];
    for stage in array(snapshot, "stages") {
        let status = s(stage, "status");
        let label = if s(stage, "id") == "setup-script" && !s(script, "name").is_empty() { s(script, "name") } else { stage_label(s(stage, "id")) };
        let trailing = match status {
            "pending" => None,
            "skipped" => Some(Some(s(stage, "detail")).filter(|d| !d.is_empty()).unwrap_or("skipped").to_owned()),
            "running" if s(stage, "id") == "checkout" && stage["percent"].is_number() => Some(format!("{}%", stage["percent"].as_f64().unwrap_or(0.0).round())),
            _ => Some(s(stage, "detail").to_owned()).filter(|d| !d.is_empty()),
        };
        let ended = if s(stage, "endedAt").is_empty() { s(snapshot, "endedAt") } else { s(stage, "endedAt") };
        let time = (!matches!(status, "pending" | "skipped")).then(|| elapsed(s(stage, "startedAt"), ended, running, now)).flatten();
        card = card.child(
            row()
                .w_full()
                .h(28.0)
                .px(4.0)
                .gap(12.0)
                .center_y()
                .child(row().w(16.0).center().child(stage_icon(status, t)))
                .child(text(label).font(theme::TEXT_SM).color(stage_color(status, t)).truncate().shrink().min_w(0.0))
                .child(rune::spacer())
                .children(trailing.map(|d| text(d).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().shrink().min_w(0.0)))
                .children(time.map(|e| text(e).font(theme::TEXT_XS).color(t.c.muted_foreground))),
        );
        if s(stage, "id") == "setup-script"
            && let Some((lines, failed)) = tail(snapshot)
        {
            let (border, fill, fg) = if failed {
                (t.c.destructive.alpha_mul(0.2), t.c.destructive.alpha_mul(0.06), t.c.error_foreground)
            } else {
                (t.c.border, t.c.secondary, t.c.muted_foreground)
            };
            card = card.child(
                col()
                    .ml(32.0)
                    .mb(4.0)
                    .px(10.0)
                    .py(6.0)
                    .radius(6.0)
                    .border(1.0, border)
                    .fill(fill)
                    .children(lines.into_iter().map(|line| text(if line.is_empty() { " ".to_owned() } else { line }).font(theme::mono(theme::TEXT_2XS)).color(fg).single_line().truncate().selectable(true))),
            );
        }
    }
    if s(snapshot, "phase") == "failed" && !s(snapshot, "error").is_empty() {
        card = card.child(text(s(snapshot, "error").to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().ml(32.0).mt(4.0));
    }
    let open = app.ui.chat.timeline.worktree.details.contains(thread);
    let details_thread = thread.to_owned();
    let mut actions = row().ml(18.0).mt(2.0).gap(2.0).center_y().flex_wrap().child(
        Button::new("Details")
            .variant("ghost-muted")
            .size("xs")
            .icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT })
            .on_press(move |app: &mut App| {
                let details = &mut app.ui.chat.timeline.worktree.details;
                if !details.remove(&details_thread) {
                    details.insert(details_thread.clone());
                }
            })
            .build(t),
    );
    if running {
        let cancel_thread = thread.to_owned();
        actions = actions.child(
            Button::new("Cancel")
                .variant("ghost-muted")
                .size("xs")
                .icon(&icons::X)
                .disabled(app.ui.chat.timeline.worktree.cancelling.contains_key(thread))
                .on_press(move |app: &mut App| cancel(app, &cancel_thread))
                .build(t),
        );
    }
    card = card.child(actions);
    if open {
        let mut list = col().ml(32.0).mt(4.0).mb(6.0).gap(2.0);
        for (term, value) in [("Branch", s(snapshot, "branch")), ("Base", s(snapshot, "baseRef")), ("Path", s(snapshot, "worktreePath")), ("Setup", s(script, "command"))] {
            if value.is_empty() {
                continue;
            }
            list = list.child(
                row()
                    .w_full()
                    .gap(12.0)
                    .child(text(term).font(theme::TEXT_XS).color(t.c.foreground.alpha_mul(0.8)).w(48.0))
                    .child(text(value.to_owned()).font(theme::MONO_XS).color(t.c.muted_foreground).truncate_middle().shrink().min_w(0.0).selectable(true)),
            );
        }
        card = card.child(list);
    }
    Some(card)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(phase: &str, stages: Value) -> Value {
        json!({"threadId": "t", "phase": phase, "stages": stages})
    }

    #[test]
    fn finished_setups_step_aside_once_the_turn_moves_on() {
        let ok = snapshot("done", json!([{"id": "agent", "status": "done"}]));
        let script_failed = snapshot("done", json!([{"id": "setup-script", "status": "failed"}]));
        assert!(visible(&snapshot("running", json!([])), true, true));
        assert!(visible(&ok, false, false) && !visible(&ok, true, false));
        assert!(visible(&script_failed, true, false) && !visible(&script_failed, true, true));
        assert!(visible(&snapshot("failed", json!([])), true, false));
        assert_eq!(header_label(&script_failed), "Worktree ready, setup script failed");
        assert_eq!(header_label(&snapshot("cancelled", json!([]))), "Worktree setup cancelled");
    }

    #[test]
    fn the_output_tail_keeps_the_last_four_lines() {
        let running = snapshot("running", json!([{"id": "setup-script", "status": "running", "tail": ["a", "b", "c", "d", "e"]}]));
        assert_eq!(tail(&running), Some((vec!["b".into(), "c".into(), "d".into(), "e".into()], false)));
        let failed = snapshot("done", json!([{"id": "setup-script", "status": "failed", "tail": ["boom"]}]));
        assert_eq!(tail(&failed), Some((vec![String::new(), String::new(), String::new(), "boom".into()], true)));
        assert_eq!(tail(&snapshot("running", json!([{"id": "setup-script", "status": "running", "tail": []}]))), None);
    }

    #[test]
    fn only_failed_preparations_retry() {
        let p = json!({
            "runs": [
                {"id": "r1", "status": "failed", "workspacePreparation": {}},
                {"id": "r2", "status": "failed"},
                {"id": "r3", "status": "completed", "workspacePreparation": {}},
            ],
            "turnItems": [
                {"type": "error", "status": "failed", "runId": "r1", "failure": {"code": "workspace_preparation_failed"}},
                {"type": "error", "status": "failed", "runId": "r2", "failure": {"code": "workspace_preparation_failed"}},
                {"type": "error", "status": "failed", "runId": "r3", "failure": {"code": "workspace_preparation_failed"}},
                {"type": "error", "status": "failed", "runId": "r1", "failure": {"code": "other"}},
            ],
        });
        assert_eq!(retryable_runs(&p), HashSet::from(["r1"]));
    }
}
