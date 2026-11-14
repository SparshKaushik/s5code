//! A server self-update in flight (client-runtime state/server.ts `updateServer`): the progress
//! stream (or the one-shot request on older servers), the desktop app's commit, then the
//! restarted server's `ready` that proves the version took. The chat banner and Settings › About
//! both read [`status`] and start one with [`start`].
use crate::App;
use crate::data::s;
use serde_json::{Value, json};

/// Covers the service launcher's 30-minute trial and a final restart of the old version.
const RESUME_TIMEOUT_MS: i64 = 32 * 60 * 1000;
/// Desktop commits tried before giving up on the app installing the prepared update.
const MAX_COMMITS: u8 = 3;
const LIFECYCLE: &str = "subscribeServerLifecycle";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Downloading,
    Installing,
    Resuming,
}

impl Stage {
    /// `serverUpdateStageLabel`.
    pub fn label(self) -> &'static str {
        match self {
            Stage::Downloading | Stage::Installing => "Downloading…",
            Stage::Resuming => "Restarting…",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Status {
    #[default]
    Idle,
    Running { stage: Stage, from: String, target: String },
    Failed { stage: Stage, from: String, target: String, message: String },
}

#[derive(Default)]
enum Step {
    #[default]
    None,
    /// The update request: its id and whether it streams progress.
    Request(u64, bool),
    Commit(u64, Value),
    /// Waiting for the restarted server's `ready`, until the deadline.
    Resume { result: Value, deadline: i64, disconnected: bool, nudged: i64 },
}

#[derive(Default)]
pub struct State {
    status: Status,
    step: Step,
    /// `capabilities.serverSelfUpdate` when the update started.
    method: String,
    progressed: bool,
    terminal: Option<Value>,
    commits: u8,
    /// The failure the user dismissed (isServerUpdateFailureDismissed).
    dismissed: Option<Status>,
}

pub fn status(app: &App) -> &Status {
    &app.ui.chat.server_update.status
}

/// The failure stays until dismissed or a new update starts.
pub fn dismissed(app: &App) -> bool {
    let state = &app.ui.chat.server_update;
    state.dismissed.as_ref() == Some(&state.status)
}

/// Sets the status as-is, for scenes.
pub fn show(app: &mut App, status: Status) {
    app.ui.chat.server_update = State { status, ..State::default() };
}

pub fn dismiss(app: &mut App) {
    let state = &mut app.ui.chat.server_update;
    state.dismissed = Some(state.status.clone());
}

/// Updates this window's server to `target` (an exact npm version), unless one is running.
pub fn start(app: &mut App, target: &str) {
    if matches!(status(app), Status::Running { .. }) {
        return;
    }
    let environment = app.state().config.as_ref().map(|c| c["environment"].clone()).unwrap_or(Value::Null);
    let from = Some(s(&environment, "serverVersion")).filter(|v| !v.is_empty()).unwrap_or(target).to_owned();
    let streaming = environment["capabilities"]["serverSelfUpdateProgress"] == true;
    let method = s(&environment["capabilities"], "serverSelfUpdate").to_owned();
    let Some(net) = app.src.net() else { return };
    let input = json!({"targetVersion": target});
    let id = net.request(if streaming { "server.updateServerWithProgress" } else { "server.updateServer" }, input);
    app.ui.chat.server_update = State {
        status: Status::Running { stage: Stage::Downloading, from, target: target.to_owned() },
        step: Step::Request(id, streaming),
        method,
        ..State::default()
    };
}

/// The transport dropped mid-request, as a restarting server does (isLegacyUpdateHandoffLoss).
fn transport_lost(cause: &Value) -> bool {
    s(cause, "_tag") == "TransportError"
}

/// `serverUpdateFailureMessage` on a reply's cause.
fn failure_message(cause: &Value) -> String {
    let error = crate::chat::timeline::rpc_failure(cause);
    match (s(error, "_tag"), s(error, "reason"), s(error, "message")) {
        ("ServerSelfUpdateError", reason, _) if !reason.is_empty() => format!("Server update failed: {reason}"),
        (_, _, message) if !message.is_empty() => message.to_owned(),
        _ => "Server update failed.".to_owned(),
    }
}

/// `serverUpdateStateForProgressEvent`'s stage, and the terminal result it may carry.
fn progress(event: &Value) -> (Option<Stage>, Option<&Value>) {
    match s(event, "type") {
        "progress" => (Some(if s(event, "stage") == "installing" { Stage::Installing } else { Stage::Downloading }), None),
        "complete" => {
            let result = &event["result"];
            let desktop = s(result, "method") == "desktop-app" && !s(result, "desktopUpdateToken").is_empty();
            (Some(if desktop { Stage::Installing } else { Stage::Resuming }), Some(result))
        }
        _ => (None, None),
    }
}

/// `resolveServerUpdateProgressResult` and the unary fallback: the accepted update, or why not.
fn resolve(target: &str, method: &str, streaming: bool, progressed: bool, terminal: Option<&Value>, reply: &Result<Value, Value>) -> Result<Value, String> {
    let handoff = matches!(method, "boot-service" | "respawn");
    let accepted = || json!({"targetVersion": target, "method": method});
    match reply {
        Ok(result) if !streaming => Ok(result.clone()),
        Ok(_) => terminal.cloned().ok_or_else(|| format!("The t3@{target} update ended before the server accepted the restart.")),
        Err(cause) if transport_lost(cause) => match terminal {
            Some(result) if streaming => Ok(result.clone()),
            _ if handoff && (progressed || !streaming) => Ok(accepted()),
            _ => Err(failure_message(cause)),
        },
        Err(cause) => Err(failure_message(cause)),
    }
}

enum Ready {
    /// Not this update's server yet.
    Skip,
    Done,
    Failed(String),
    /// The desktop app came back on another version: commit again.
    Recommit,
}

/// `matchesServerUpdateResumeEvent`, `waitForDesktopUpdateTarget` and
/// `validateServerUpdateReadyEvent` on one `ready` event.
fn ready(result: &Value, event: &Value, disconnected: bool) -> Ready {
    if s(event, "type") != "ready" {
        return Ready::Skip;
    }
    let target = s(result, "targetVersion");
    let version = s(&event["payload"]["environment"], "serverVersion");
    let outcome = &event["payload"]["updateOutcome"];
    let update_id = s(result, "updateId");
    if s(result, "method") == "desktop-app" && !s(result, "desktopUpdateToken").is_empty() {
        // Before the app restarts, the old server's own `ready` is still in the replay.
        if !disconnected {
            return Ready::Skip;
        }
        if version != target {
            return Ready::Recommit;
        }
    } else if (update_id.is_empty() && version != target) || (!update_id.is_empty() && s(outcome, "id") != update_id) {
        return Ready::Skip;
    }
    if update_id.is_empty() {
        return Ready::Done;
    }
    if s(outcome, "id") == update_id && s(outcome, "status") == "committed" && s(outcome, "targetVersion") == target && version == target {
        return Ready::Done;
    }
    let reason = s(outcome, "reason");
    Ready::Failed(if reason.is_empty() { "The service launcher resumed without committing the requested server version.".to_owned() } else { reason.to_owned() })
}

fn set_stage(state: &mut State, next: Stage) {
    if let Status::Running { stage, .. } = &mut state.status {
        *stage = next;
    }
}

fn fail(state: &mut State, message: String) {
    if let Status::Running { stage, from, target } = std::mem::take(&mut state.status) {
        state.status = Status::Failed { stage, from, target, message };
    }
    state.step = Step::None;
}

pub fn poll(app: &mut App) -> bool {
    if matches!(app.ui.chat.server_update.step, Step::None) {
        return false;
    }
    let now = app.now_ms;
    let Some(net) = app.src.net() else { return false };
    let state = &mut app.ui.chat.server_update;
    let (from, target) = match &state.status {
        Status::Running { from, target, .. } => (from.clone(), target.clone()),
        _ => return false,
    };
    let mut changed = false;
    match std::mem::take(&mut state.step) {
        Step::None => {}
        Step::Request(id, streaming) => {
            for event in net.take_chunks(id) {
                changed = true;
                let (stage, result) = progress(&event);
                state.progressed |= s(&event, "type") == "progress";
                if let Some(result) = result {
                    state.terminal = Some(result.clone());
                }
                if let Some(stage) = stage {
                    set_stage(state, stage);
                }
            }
            match net.take_reply(id) {
                None => state.step = Step::Request(id, streaming),
                Some(reply) => {
                    changed = true;
                    match resolve(&target, &state.method, streaming, state.progressed, state.terminal.as_ref(), &reply) {
                        Err(message) => fail(state, message),
                        Ok(result) => match Some(s(&result, "desktopUpdateToken").to_owned()).filter(|t| s(&result, "method") == "desktop-app" && !t.is_empty()) {
                            Some(token) => {
                                state.commits = 1;
                                state.step = Step::Commit(net.request("server.commitDesktopUpdate", json!({"requestId": token})), result);
                            }
                            None => resume(state, net, result, &from, now),
                        },
                    }
                }
            }
        }
        Step::Commit(id, result) => match net.take_reply(id) {
            None => state.step = Step::Commit(id, result),
            Some(Err(cause)) if !transport_lost(&cause) => {
                changed = true;
                fail(state, failure_message(&cause));
            }
            Some(_) => {
                changed = true;
                resume(state, net, result, &from, now);
            }
        },
        Step::Resume { result, deadline, mut disconnected, mut nudged } => {
            if now > deadline {
                net.unwatch_resource(LIFECYCLE, json!({}));
                fail(state, format!("Could not confirm that the server resumed on t3@{target}. Reconnect to check the update outcome."));
                return true;
            }
            // nudgeReconnectDuringUpdateRestart: a restarting server refuses for a while.
            if matches!(net.state().conn, s5_net::Conn::Closed(_)) {
                disconnected = true;
                if now - nudged >= 1000 {
                    nudged = now;
                    net.reconnect();
                }
            } else if net.state().conn == s5_net::Conn::Connecting {
                disconnected = true;
            }
            for event in net.take_resource_events(LIFECYCLE, json!({})) {
                match ready(&result, &event, disconnected) {
                    Ready::Skip => {}
                    Ready::Done => {
                        net.unwatch_resource(LIFECYCLE, json!({}));
                        *state = State::default();
                        return true;
                    }
                    Ready::Failed(message) => {
                        net.unwatch_resource(LIFECYCLE, json!({}));
                        fail(state, message);
                        return true;
                    }
                    Ready::Recommit if state.commits < MAX_COMMITS => {
                        state.commits += 1;
                        let token = s(&result, "desktopUpdateToken").to_owned();
                        state.step = Step::Commit(net.request("server.commitDesktopUpdate", json!({"requestId": token})), result);
                        return true;
                    }
                    Ready::Recommit => {
                        net.unwatch_resource(LIFECYCLE, json!({}));
                        fail(state, "The desktop app resumed without installing the prepared update.".to_owned());
                        return true;
                    }
                }
            }
            state.step = Step::Resume { result, deadline, disconnected, nudged };
        }
    }
    changed
}

/// The update was accepted: wait for the server to come back on it.
fn resume(state: &mut State, net: &mut s5_net::Net, result: Value, from: &str, now: i64) {
    let target = s(&result, "targetVersion").to_owned();
    // A track switch's desktop host already on its newest build only saved the track.
    if s(&result, "method") == "desktop-app" && s(&result, "desktopUpdateToken").is_empty() && target == from {
        *state = State::default();
        return;
    }
    if let Status::Running { target: running, stage, .. } = &mut state.status {
        *running = target;
        *stage = Stage::Resuming;
    }
    net.watch_resource(LIFECYCLE, json!({}));
    // Events from before this point belong to the old server.
    let _ = net.take_resource_events(LIFECYCLE, json!({}));
    state.step = Step::Resume { result, deadline: now + RESUME_TIMEOUT_MS, disconnected: false, nudged: now };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lost() -> Result<Value, Value> {
        Err(json!({"_tag": "TransportError", "message": "connection lost; command outcome may be unknown"}))
    }

    #[test]
    fn progress_events_move_the_stage() {
        assert_eq!(progress(&json!({"type": "progress", "stage": "installing"})).0, Some(Stage::Installing));
        let complete = json!({"type": "complete", "result": {"targetVersion": "2.0.0", "method": "respawn"}});
        assert_eq!(progress(&complete).0, Some(Stage::Resuming));
        let desktop = json!({"type": "complete", "result": {"targetVersion": "2.0.0", "method": "desktop-app", "desktopUpdateToken": "tok"}});
        assert_eq!(progress(&desktop).0, Some(Stage::Installing));
        assert_eq!(Stage::Installing.label(), "Downloading…");
        assert_eq!(Stage::Resuming.label(), "Restarting…");
    }

    #[test]
    fn a_dropped_transport_counts_only_as_a_restart_handoff() {
        let result = json!({"targetVersion": "2.0.0", "method": "respawn"});
        // The terminal event arrived before the socket went.
        assert_eq!(resolve("2.0.0", "respawn", true, true, Some(&result), &lost()), Ok(result.clone()));
        // Progress then loss on a host that restarts itself.
        assert_eq!(resolve("2.0.0", "boot-service", true, true, None, &lost()).unwrap()["method"], "boot-service");
        // No progress, or a desktop host, is a real failure.
        assert!(resolve("2.0.0", "boot-service", true, false, None, &lost()).is_err());
        assert!(resolve("2.0.0", "desktop-app", true, true, None, &lost()).is_err());
        assert_eq!(resolve("2.0.0", "respawn", true, true, None, &Ok(Value::Null)), Err("The t3@2.0.0 update ended before the server accepted the restart.".into()));
        // The unary request on older servers.
        assert_eq!(resolve("2.0.0", "respawn", false, false, None, &lost()).unwrap()["targetVersion"], "2.0.0");
        let refused = Err(json!([{"_tag": "Fail", "error": {"_tag": "ServerSelfUpdateError", "reason": "npm is offline"}}]));
        assert_eq!(resolve("2.0.0", "respawn", false, false, None, &refused), Err("Server update failed: npm is offline".into()));
    }

    #[test]
    fn only_the_restarted_server_finishes_the_update() {
        let ready_on = |version: &str, outcome: Value| json!({"type": "ready", "payload": {"environment": {"serverVersion": version}, "updateOutcome": outcome}});
        let plain = json!({"targetVersion": "2.0.0", "method": "respawn"});
        assert!(matches!(ready(&plain, &ready_on("1.0.0", Value::Null), false), Ready::Skip));
        assert!(matches!(ready(&plain, &json!({"type": "welcome"}), true), Ready::Skip));
        assert!(matches!(ready(&plain, &ready_on("2.0.0", Value::Null), false), Ready::Done));

        let tracked = json!({"targetVersion": "2.0.0", "method": "boot-service", "updateId": "u1"});
        let committed = json!({"id": "u1", "fromVersion": "1.0.0", "targetVersion": "2.0.0", "status": "committed"});
        assert!(matches!(ready(&tracked, &ready_on("1.0.0", json!({"id": "old"})), false), Ready::Skip));
        assert!(matches!(ready(&tracked, &ready_on("2.0.0", committed), false), Ready::Done));
        let rolled = json!({"id": "u1", "fromVersion": "1.0.0", "targetVersion": "2.0.0", "status": "rolled-back", "reason": "Health check failed"});
        assert!(matches!(ready(&tracked, &ready_on("1.0.0", rolled), false), Ready::Failed(m) if m == "Health check failed"));

        let desktop = json!({"targetVersion": "2.0.0", "method": "desktop-app", "desktopUpdateToken": "tok"});
        assert!(matches!(ready(&desktop, &ready_on("1.0.0", Value::Null), false), Ready::Skip));
        assert!(matches!(ready(&desktop, &ready_on("1.0.0", Value::Null), true), Ready::Recommit));
        assert!(matches!(ready(&desktop, &ready_on("2.0.0", Value::Null), true), Ready::Done));
    }
}
