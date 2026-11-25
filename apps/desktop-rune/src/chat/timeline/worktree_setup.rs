//! A new worktree's setup progress (WorktreeSetupCard, useWorktreeSetup; client-runtime
//! worktreeSetup.ts): its stages streamed from `subscribeWorktreeSetup`, Cancel while it runs,
//! and Retry on the error a failed workspace preparation leaves in the timeline.

use std::collections::{HashMap, HashSet};

use rune::{El, col, el, icon, row, text};
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
    pub(super) background_open: Option<String>,
    pub(super) background_details: bool,
    /// Thread id -> `worktreeSetup.cancel` in flight.
    cancelling: HashMap<String, u64>,
    /// Run id -> `prepared-run.retry` in flight.
    retrying: HashMap<String, u64>,
    /// Thread id -> the run a Work locally cancel holds back; its prompt goes out again once
    /// the cancel lands.
    work_locally: HashMap<String, String>,
    /// Thread id -> the run cancelled for Work locally, until it ends and the worktree is cleared.
    resending: HashMap<String, String>,
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

/// setupHandedOff: the agent has started from the worktree, so the card is history and a finished
/// one folds to a single summary row.
fn handed_off(snapshot: &Value, turn_started: bool) -> bool {
    turn_started && agent_started(snapshot)
}

fn agent_started(snapshot: &Value) -> bool {
    array(snapshot, "stages").iter().any(|st| s(st, "id") == "agent" && s(st, "status") == "done")
}

/// CollapsedSummaryRow's status: failed when the setup or its script did.
fn summary_status(snapshot: &Value) -> &'static str {
    let script_failed = array(snapshot, "stages").iter().any(|st| s(st, "id") == "setup-script" && s(st, "status") == "failed");
    if matches!(s(snapshot, "phase"), "failed" | "cancelled") || script_failed { "failed" } else { "done" }
}

/// The setup script's server terminal, once its stage has started.
fn setup_terminal(snapshot: &Value) -> Option<&str> {
    let started = array(snapshot, "stages").iter().any(|st| s(st, "id") == "setup-script" && s(st, "status") != "pending");
    Some(s(&snapshot["setupScript"], "terminalId")).filter(|id| started && !id.is_empty())
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
    let (work_locally, resending) = (&mut ui.work_locally, &mut ui.resending);
    ui.cancelling.retain(|thread, request| {
        let Some(reply) = net.take_reply(*request) else { return true };
        changed = true;
        let held = work_locally.remove(thread);
        match reply {
            Ok(v) => {
                if let Some(run) = held.filter(|_| v["cancelled"] == true) {
                    resending.insert(thread.clone(), run);
                }
            }
            Err(error) => failures.push(("Could not cancel worktree setup", error)),
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
    let st = app.state();
    let mut ready = Vec::new();
    for (thread, run) in &app.ui.chat.timeline.worktree.resending {
        let Some(view) = st.thread_views.get(thread) else { continue };
        let shell = st.threads.iter().find(|r| s(r, "id") == thread.as_str());
        if settled(&view.projection, run, shell) {
            ready.push((thread.clone(), prompt(&view.projection, run).cloned()));
        }
    }
    for (thread, message) in ready {
        app.ui.chat.timeline.worktree.resending.remove(&thread);
        if let Some(message) = message {
            super::actions::resend(app, &thread, &message);
        }
        changed = true;
    }
    changed
}

/// The run a running setup holds back: the newest one still preparing its workspace.
fn held_run(projection: &Value) -> Option<&str> {
    array(projection, "runs").iter().filter(|r| s(r, "status") == "preparing").max_by_key(|r| r["ordinal"].as_i64()).map(|r| s(r, "id"))
}

/// The prompt that run carried, with its attachments and context.
fn prompt<'a>(projection: &'a Value, run: &str) -> Option<&'a Value> {
    array(projection, "turnItems").iter().find(|i| s(i, "type") == "user_message" && s(i, "runId") == run)
}

/// The cancelled run has ended (the server marks it failed, with its "Worktree setup cancelled."
/// error left in the timeline) and the worktree is gone, so a send starts locally. Messages
/// queued behind the setup stay held, so they don't count.
fn settled(projection: &Value, run: &str, shell: Option<&Value>) -> bool {
    let runs = array(projection, "runs");
    let ended = runs.iter().any(|r| s(r, "id") == run && matches!(s(r, "status"), "failed" | "interrupted" | "cancelled"));
    let active = runs.iter().any(|r| matches!(s(r, "status"), "preparing" | "starting" | "running" | "waiting"));
    ended && !active && shell.is_none_or(|r| s(r, "worktreePath").is_empty())
}

/// useWorktreeSetup's `blocksSend`: the thread's worktree setup runs and has not reached the
/// agent, so Send waits ("Preparing worktree"). Once the run is preparing or starting, a send
/// queues behind the setup instead (`sendQueuesBehindSetup`).
pub fn blocks_send(app: &App, thread: &str) -> bool {
    let queues = app.state().threads.iter().find(|r| s(r, "id") == thread).is_some_and(|r| matches!(s(r, "activityRunStatus"), "preparing" | "starting"));
    !queues && app.ui.chat.timeline.worktree.held.get(thread).is_some_and(|snapshot| s(snapshot, "phase") == "running" && !agent_started(snapshot))
}

/// Shows `snapshot` as `thread`'s setup, as the stream would (scenes and tests).
pub fn hold(app: &mut App, thread: &str, snapshot: Value) {
    app.ui.chat.timeline.worktree.held.insert(thread.to_owned(), snapshot);
}

/// Work locally (onWorktreeSetupWorkLocally): cancel, and once [`poll`] sees the cancel land and
/// the run settle, send the prompt it held back again from the project checkout.
fn work_locally(app: &mut App, thread: &str) {
    let held = app.state().thread_views.get(thread).and_then(|v| held_run(&v.projection)).map(str::to_owned);
    cancel(app, thread);
    if let Some(run) = held
        && app.ui.chat.timeline.worktree.cancelling.contains_key(thread)
    {
        app.ui.chat.timeline.worktree.work_locally.insert(thread.to_owned(), run);
    }
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
    icon(glyph, 16.0).color(t.c.icon_muted.alpha_mul(if status == "pending" { 0.4 } else { 1.0 }))
}

fn stage_color(status: &str, t: &T) -> rune::Color {
    match status {
        "failed" => t.c.error_foreground,
        "warning" => t.c.warning_foreground,
        "pending" => t.c.contrast_secondary_label.alpha_mul(0.4),
        _ => t.c.contrast_secondary_label,
    }
}

fn turn_started(app: &App, thread: &str) -> bool {
    if let Some(view) = app.state().thread_views.get(thread) {
        let runs = array(&view.projection, "runs");
        let latest = |eligible: fn(&Value) -> bool| {
            runs.iter().filter(|run| eligible(run)).max_by_key(|run| run["ordinal"].as_u64())
        };
        if let Some(run) = latest(|r| matches!(s(r, "status"), "preparing" | "starting" | "running" | "waiting"))
            .or_else(|| latest(|r| s(r, "status") != "queued" || r["queueHeld"] != true))
        {
            return run["startedAt"].is_string();
        }
    }
    app.state().threads.iter().any(|r| s(r, "id") == thread && r["latestRunStartedAt"].is_string())
}

fn background_running(snapshot: &Value, turn_started: bool) -> bool {
    s(snapshot, "phase") == "running" && handed_off(snapshot, turn_started)
}

pub(super) fn background<'a>(app: &'a App, thread: &str) -> Option<&'a Value> {
    let snapshot = app.ui.chat.timeline.worktree.held.get(thread)?;
    background_running(snapshot, turn_started(app, thread)).then_some(snapshot)
}

pub fn view(app: &App, thread: &str, t: &T) -> Option<El> {
    let snapshot = app.ui.chat.timeline.worktree.held.get(thread)?;
    let turn_started = turn_started(app, thread);
    let follow_up = app.state().thread_views.get(thread).is_some_and(|v| array(&v.projection, "turnItems").iter().filter(|i| s(i, "type") == "user_message").count() > 1);
    if !visible(snapshot, turn_started, follow_up) || background_running(snapshot, turn_started) {
        return None;
    }
    Some(card(app, thread, snapshot, t, handed_off(snapshot, turn_started), false))
}

pub(super) fn card(app: &App, thread: &str, snapshot: &Value, t: &T, embedded: bool, in_popover: bool) -> El {
    let running = s(snapshot, "phase") == "running";
    let now = app.now_ms;
    let mut card = col().w_full().label("Worktree setup");
    if !embedded && !running && s(snapshot, "phase") != "done" {
        let tone = if s(snapshot, "phase") == "failed" { t.c.error_foreground } else { t.c.muted_foreground };
        // SetupHeaderRow: a 24px row under a divider, then the stages 6px below.
        card = card.child(
            col().w_full().pt(4.0).mb(6.0).child(
                row()
                    .w_full()
                    .h(24.0)
                    .px(4.0)
                    .gap(8.0)
                    .center_y()
                    .child(text(header_label(snapshot)).font(theme::TEXT_SM).color(tone).truncate().shrink().min_w(0.0))
                    .child(rune::spacer())
                    .children(elapsed(s(snapshot, "startedAt"), s(snapshot, "endedAt"), false, now).map(|e| text(e).font(theme::TEXT_XS).color(t.c.muted_foreground))),
            )
            .child(el().w_full().h(1.0).mt(8.0).fill(t.c.border.alpha_mul(0.6))),
        );
    }
    let script = &snapshot["setupScript"];
    let stage_row = |status: &str, label: &str, trailing: Option<String>, time: Option<String>| {
        // WorkLogRow: py-0.5 around a 24px line, px-0.5, a 24px icon lane, gap-1.5.
        row()
            .w_full()
            .h(28.0)
            .px(2.0)
            .gap(6.0)
            .center_y()
            .child(row().size(24.0, 24.0).center().child(stage_icon(status, t)))
            .child(text(label.to_owned()).font(theme::TEXT_SM).color(stage_color(status, t)).truncate().shrink().min_w(0.0))
            .child(rune::spacer())
            .children(trailing.map(|d| text(d).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().shrink().min_w(0.0)))
            .children(time.map(|e| text(e).font(theme::TEXT_XS).color(t.c.muted_foreground)))
    };
    if embedded && !running {
        let time = elapsed(s(snapshot, "startedAt"), s(snapshot, "endedAt"), false, now);
        card = card.child(stage_row(summary_status(snapshot), header_label(snapshot), None, time));
    }
    for stage in array(snapshot, "stages").iter().filter(|_| !embedded || running) {
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
        card = card.child(stage_row(status, label, trailing, time));
        if s(stage, "id") == "setup-script"
            && let Some((lines, failed)) = tail(snapshot)
        {
            let (border, fill, fg) = if failed {
                (t.c.destructive.alpha_mul(0.2), t.c.error_surface, t.c.error_foreground)
            } else {
                (t.c.border, t.c.code_background, t.c.muted_foreground)
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
    let ui = &app.ui.chat.timeline.worktree;
    let open = if in_popover { ui.background_details } else { ui.details.contains(thread) };
    let details_thread = thread.to_owned();
    let mut actions = row().ml(19.0).mt(2.0).gap(2.0).center_y().flex_wrap().child(
        Button::new("Details")
            .variant("ghost-muted")
            .size("xs")
            .icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT })
            .on_press(move |app: &mut App| {
                let ui = &mut app.ui.chat.timeline.worktree;
                if in_popover {
                    ui.background_details ^= true;
                } else if !ui.details.remove(&details_thread) {
                    ui.details.insert(details_thread.clone());
                }
            })
            .build(t)
            .expanded(open),
    );
    if let Some(id) = setup_terminal(snapshot) {
        let terminal_thread = thread.to_owned();
        let id = id.to_owned();
        actions = actions.child(
            Button::new("Open terminal")
                .variant("ghost-muted")
                .size("xs")
                .icon(&icons::TERMINAL)
                .on_press(move |app: &mut App| {
                    if in_popover {
                        super::background_setup::close(app);
                    }
                    crate::panel::drawer::show_for(app, &terminal_thread, &id);
                })
                .build(t),
        );
    }
    if running && !embedded {
        let local_thread = thread.to_owned();
        actions = actions.child(
            Button::new("Work locally")
                .variant("ghost-muted")
                .size("xs")
                .icon(&icons::LAPTOP)
                .disabled(app.ui.chat.timeline.worktree.cancelling.contains_key(thread))
                .on_press(move |app: &mut App| work_locally(app, &local_thread))
                .build(t),
        );
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
        let rows: Vec<(&str, &str)> = [("Branch", s(snapshot, "branch")), ("Base", s(snapshot, "baseRef")), ("Path", s(snapshot, "worktreePath")), ("Setup", s(script, "command"))]
            .into_iter()
            .filter(|(_, value)| !value.is_empty())
            .collect();
        // The dl's `auto` column: as wide as its widest term.
        let term_w = rows.iter().map(|(term, _)| rune::text::measure(term, &[], theme::TEXT_XS, None).0.ceil()).fold(0.0, f32::max);
        let mut list = col().ml(32.0).mt(4.0).mb(6.0).gap(2.0);
        for (term, value) in rows {
            list = list.child(
                row()
                    .w_full()
                    .gap(12.0)
                    .child(text(term).font(theme::TEXT_XS).color(t.c.foreground.alpha_mul(0.8)).w(term_w))
                    .child(text(value.to_owned()).font(theme::MONO_XS).color(t.c.muted_foreground).truncate_middle().shrink().min_w(0.0).selectable(true)),
            );
        }
        card = card.child(list);
    }
    card
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
    fn a_finished_setup_folds_to_one_summary_row_once_the_agent_takes_over() {
        let with = |phase: &str, script: &str, agent: &str| snapshot(phase, json!([{"id": "setup-script", "status": script}, {"id": "agent", "status": agent}]));
        let ok = with("done", "done", "done");
        assert!(handed_off(&ok, true));
        assert!(!handed_off(&ok, false), "FAILED: the turn has not started yet");
        assert!(!handed_off(&with("running", "running", "pending"), true), "FAILED: the agent has not started from the worktree");
        for (phase, agent, started, expected) in [
            ("running", "done", true, true),
            ("running", "done", false, false),
            ("running", "pending", true, false),
            ("done", "done", true, false),
            ("failed", "done", true, false),
            ("cancelled", "done", true, false),
        ] {
            assert_eq!(background_running(&with(phase, "running", agent), started), expected);
        }
        assert_eq!(summary_status(&ok), "done");
        assert_eq!(summary_status(&with("done", "failed", "done")), "failed");
        assert_eq!(summary_status(&with("cancelled", "skipped", "pending")), "failed");
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
    fn the_setup_terminal_opens_once_its_stage_starts() {
        let with = |status: &str| {
            let mut v = snapshot("running", json!([{"id": "setup-script", "status": status}]));
            v["setupScript"] = json!({"name": "setup", "terminalId": "setup-1"});
            v
        };
        assert_eq!(setup_terminal(&with("pending")), None);
        assert_eq!(setup_terminal(&with("running")), Some("setup-1"));
        assert_eq!(setup_terminal(&with("failed")), Some("setup-1"));
        assert_eq!(setup_terminal(&snapshot("running", json!([{"id": "setup-script", "status": "running"}]))), None);
    }

    #[test]
    fn work_locally_resends_the_held_prompt_once_its_run_settles() {
        let user = |run: &str, text: &str| json!({"type": "user_message", "runId": run, "text": text, "attachments": [{"type": "image", "id": format!("{run}-img")}], "context": {"version": 1, "records": [{"contextId": run}]}});
        let p = |held: &str| {
            json!({
                "runs": [
                    {"id": "r1", "ordinal": 1, "status": "completed"},
                    {"id": "r2", "ordinal": 2, "status": held, "workspacePreparation": {"type": "worktree"}},
                    {"id": "r3", "ordinal": 3, "status": "queued", "queueHeld": true},
                ],
                "turnItems": [user("r1", "first"), {"type": "assistant_message", "runId": "r1", "text": "a"}, user("r2", "build it"), user("r3", "and test it")],
            })
        };
        assert_eq!(held_run(&p("preparing")), Some("r2"), "FAILED: the preparing run, not the newest one");
        assert_eq!(held_run(&p("failed")), None);
        let preparing = p("preparing");
        let message = prompt(&preparing, "r2").expect("FAILED: the held run's prompt");
        assert_eq!(s(message, "text"), "build it", "FAILED: not the newest message by position");
        assert_eq!(message["attachments"][0]["id"], "r2-img");
        assert_eq!(message["context"]["records"][0]["contextId"], "r2");
        let worktree = json!({"worktreePath": "/wt"});
        let local = json!({"worktreePath": null});
        assert!(!settled(&p("preparing"), "r2", Some(&local)), "FAILED: waits for the cancelled run to fail");
        assert!(!settled(&p("failed"), "r2", Some(&worktree)), "FAILED: waits for the worktree to be cleared");
        assert!(!settled(&p("failed"), "r9", Some(&local)), "FAILED: waits for its own run");
        assert!(settled(&p("failed"), "r2", Some(&local)), "FAILED: a held queued follow-up does not block it");
    }

    /// The showcase thread on a recording connection, preparing `wl-run` in a worktree for
    /// "add a billing schema", with a follow-up queued behind it.
    fn preparing_thread() -> (App, String, s5_net::Sent) {
        let mut app = App::new();
        let thread = app
            .state()
            .thread_views
            .iter()
            .find(|(_, v)| s(&v.projection["thread"], "title") == "Replay showcase")
            .map(|(id, _)| id.clone())
            .expect("the showcase fixture thread");
        let mut state = app.state().clone();
        if let Some(shell) = state.threads.iter_mut().find(|r| s(r, "id") == thread) {
            shell["worktreePath"] = json!("/home/dev/.t3/worktrees/app/billing-schema");
        }
        let p = &mut state.thread_views.get_mut(&thread).expect("its projection").projection;
        let runs = [
            json!({"id": "wl-run", "ordinal": 999, "status": "preparing", "rootNodeId": "wl-root", "startedAt": "2026-12-01T00:00:00.000Z", "workspacePreparation": {"type": "worktree"}}),
            json!({"id": "wl-queued", "ordinal": 1000, "status": "queued", "queueHeld": true, "rootNodeId": "wl-queued-root"}),
        ];
        let items = [
            json!({"id": "wl-prompt", "type": "user_message", "runId": "wl-run", "nodeId": "wl-root", "ordinal": 1, "status": "completed",
                "text": "add a billing schema", "attachments": [], "createdAt": "2026-12-01T00:00:00.000Z"}),
            json!({"id": "wl-follow-up", "type": "user_message", "runId": "wl-queued", "nodeId": "wl-queued-root", "ordinal": 2, "status": "pending",
                "inputIntent": "queued_turn", "text": "then add tests", "attachments": [], "createdAt": "2026-12-01T00:00:01.000Z"}),
        ];
        for (key, values) in [("runs", runs), ("turnItems", items)] {
            if !p[key].is_array() {
                p[key] = json!([]);
            }
            if let Some(list) = p[key].as_array_mut() {
                list.extend(values);
            }
        }
        let (mut net, sent) = s5_net::Net::in_memory_sent("http://local.test");
        *net.state_mut() = state;
        app.src = crate::data::Source::Live(net);
        app.navigate(Route::Thread(thread.clone()));
        (app, thread, sent)
    }

    /// The server's mark for a cancelled setup (prepared-run.fail): the run fails, its worktree is
    /// cleared, and its error stays in the timeline.
    fn cancel_lands(app: &mut App, thread: &str) {
        let Some(net) = app.src.net() else { panic!("live") };
        let st = net.state_mut();
        if let Some(shell) = st.threads.iter_mut().find(|r| s(r, "id") == thread) {
            shell["worktreePath"] = Value::Null;
        }
        let Some(p) = st.thread_views.get_mut(thread).map(|v| &mut v.projection) else { panic!("projection") };
        for run in p["runs"].as_array_mut().into_iter().flatten() {
            if run["id"] == "wl-run" {
                run["status"] = json!("failed");
            }
        }
        if let Some(items) = p["turnItems"].as_array_mut() {
            items.push(json!({"id": "wl-error", "type": "error", "runId": "wl-run", "status": "failed", "title": "Workspace preparation failed",
                "failure": {"code": PREPARATION_FAILED, "message": "Worktree setup cancelled."}}));
        }
    }

    fn cancel_request(sent: &s5_net::Sent, thread: &str) -> u64 {
        let requests = sent.requests();
        let (id, _, payload) = requests.iter().find(|(_, tag, _)| tag == "worktreeSetup.cancel").expect("FAILED: the cancel goes out");
        assert_eq!(payload["threadId"], json!(thread));
        *id
    }

    /// The whole Work locally sequence against a recording connection: the cancel goes out, the
    /// server says it cancelled, nothing is resent while the run is still preparing, and once the
    /// run fails with its worktree cleared the prompt that run held (not the queued follow-up
    /// after it) is dispatched to the thread again; the cancelled attempt keeps the server's
    /// error mark, with Retry, as on web.
    #[test]
    fn work_locally_resends_through_the_connection_once_the_cancelled_run_settles() {
        let (mut app, thread, sent) = preparing_thread();
        work_locally(&mut app, &thread);
        let cancel = cancel_request(&sent, &thread);
        let Some(net) = app.src.net() else { panic!("live") };
        net.put_reply(cancel, Ok(json!({"cancelled": true})));

        poll(&mut app);
        assert_eq!(app.ui.chat.timeline.worktree.resending.get(&thread).map(String::as_str), Some("wl-run"), "FAILED: the cancel is waiting to settle");
        assert!(!sent.requests().iter().any(|(_, tag, _)| tag == "orchestration.dispatchCommand"), "FAILED: nothing is resent while the run is still preparing");

        cancel_lands(&mut app, &thread);
        poll(&mut app);
        assert!(app.ui.chat.timeline.worktree.resending.is_empty());
        super::super::actions::poll(&mut app);
        let dispatched: Vec<Value> = sent.requests().into_iter().filter(|(_, tag, _)| tag == "orchestration.dispatchCommand").map(|r| r.2).collect();
        let resent: Vec<&Value> = dispatched.iter().filter(|c| c["type"] == "message.dispatch").collect();
        assert_eq!(resent.len(), 1, "FAILED: the prompt is sent again, once; dispatched {dispatched:?}");
        assert_eq!(resent[0]["threadId"], json!(thread));
        assert_eq!(resent[0]["text"], "add a billing schema");
        assert!(!dispatched.iter().any(|c| c["type"] == "checkpoint.rollback"), "FAILED: web rolls nothing back; the server marks the attempt");
        let projection = &app.state().thread_views[&thread].projection;
        assert!(retryable_runs(projection).contains("wl-run"), "FAILED: the cancelled attempt keeps its error row");
    }

    #[test]
    fn work_locally_resends_nothing_when_the_cancel_does_not_land() {
        for reply in [Ok(json!({"cancelled": false})), Err(json!({"message": "gone"}))] {
            let (mut app, thread, sent) = preparing_thread();
            work_locally(&mut app, &thread);
            let cancel = cancel_request(&sent, &thread);
            let Some(net) = app.src.net() else { panic!("live") };
            net.put_reply(cancel, reply);
            poll(&mut app);
            assert!(app.ui.chat.timeline.worktree.resending.is_empty() && app.ui.chat.timeline.worktree.work_locally.is_empty());
            cancel_lands(&mut app, &thread);
            poll(&mut app);
            super::super::actions::poll(&mut app);
            assert!(!sent.requests().iter().any(|(_, tag, _)| tag == "orchestration.dispatchCommand"), "FAILED: nothing is resent");
        }
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
