//! ThreadActivityAnnouncer: one polite live region for the open thread. A screen reader hears
//! "Working" when a turn starts, when an approval or a question waits, and how the turn ended.
//! The first state seen of a thread is a silent baseline, so opening or switching threads never
//! replays what already happened.

use rune::{Color, El, LiveRegion, el, text};
use serde_json::Value;

use crate::data::s;
use crate::{App, Route};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Status {
    Working,
    Approval,
    Input,
    Idle,
}

#[derive(Clone, PartialEq, Debug)]
struct Snapshot {
    status: Status,
    /// The pending approval or question.
    request: Option<String>,
    run: Option<String>,
    run_status: Option<String>,
}

/// The turn under way: its run, `Unknown` until that run shows up, or `None` between turns.
#[derive(Clone, PartialEq, Debug)]
enum Turn {
    None,
    Unknown,
    Run(String),
}

#[derive(Clone, Debug)]
struct Memory {
    snapshot: Snapshot,
    turn: Turn,
    /// The run whose ending is already known, so a stale run is never announced as this turn's.
    settled: Option<String>,
    /// The run shown while idle belongs to the last turn, even if it is still settling.
    idle_run: Option<String>,
}

#[derive(Default)]
pub struct State {
    /// The thread the memory is of.
    thread: Option<String>,
    memory: Option<Memory>,
    said: Option<&'static str>,
    /// Alternates so the same words said twice still change the region's text.
    count: u64,
}

const ACTIVE: [&str; 4] = ["preparing", "starting", "running", "waiting"];
const STARTING: [&str; 3] = ["queued", "preparing", "starting"];

/// runOutcomeText.
fn outcome(status: Option<&str>) -> Option<&'static str> {
    match status? {
        "completed" => Some("Done"),
        "failed" => Some("Failed"),
        "interrupted" | "cancelled" => Some("Stopped"),
        _ => None,
    }
}

/// nextThreadAnnouncement.
fn next(previous: Option<&Memory>, next: Snapshot) -> (Memory, Option<&'static str>) {
    let idle = next.status == Status::Idle;
    let run_status = next.run_status.as_deref();
    // A run still starting while the thread looks idle (a queued message, a resume) is the next turn's.
    let idle_run = if idle && !STARTING.contains(&run_status.unwrap_or("")) { next.run.clone() } else { previous.and_then(|p| p.idle_run.clone()) };
    // A run doing new work; the run left over from the last turn doesn't count.
    let new_run = (!idle && run_status.is_some_and(|s| ACTIVE.contains(&s)) && next.run != idle_run).then(|| next.run.clone()).flatten();
    let Some(previous) = previous else {
        let turn = if idle { Turn::None } else { new_run.map_or(Turn::Unknown, Turn::Run) };
        let settled = outcome(run_status).and(next.run.clone());
        return (Memory { snapshot: next, turn, settled, idle_run }, None);
    };
    let mut memory = Memory { snapshot: next.clone(), idle_run, ..previous.clone() };
    if !idle {
        // Sending can look idle a moment before the run shows; only another run taking over is a new turn.
        let started = previous.turn == Turn::None || (new_run.is_some() && previous.turn != Turn::Unknown && new_run.as_ref().map(|r| Turn::Run(r.clone())) != Some(previous.turn.clone()));
        memory.turn = if started || previous.turn == Turn::Unknown { new_run.map_or(Turn::Unknown, Turn::Run) } else { previous.turn.clone() };
        let same = |status| previous.snapshot.status == status && previous.snapshot.request == next.request;
        let said = match next.status {
            Status::Approval => (!same(Status::Approval)).then_some("Needs approval"),
            Status::Input => (!same(Status::Input)).then_some("Needs your input"),
            _ => started.then_some("Working"),
        };
        return (memory, said);
    }
    // Work can stop a moment before the run settles; the ending waits for the run.
    let ended = outcome(run_status);
    if previous.turn == Turn::None {
        // A run that settled unseen is history too, or the next failed send would claim it.
        if ended.is_some() {
            memory.settled = next.run.clone();
        }
        return (memory, None);
    }
    if run_status == Some("rolled_back") {
        memory.turn = Turn::None;
        memory.settled = next.run.clone();
        return (memory, None);
    }
    let Some(ended) = ended else { return (memory, None) };
    // Work stopped without a new run (a send that failed); the retry is a new turn.
    if next.run == previous.settled {
        memory.turn = Turn::None;
        return (memory, None);
    }
    // The last turn's run settling while this turn waits for its own is not this turn's ending.
    if previous.turn == Turn::Unknown && next.run == previous.idle_run {
        memory.settled = next.run.clone();
        return (memory, None);
    }
    memory.turn = Turn::None;
    memory.settled = next.run.clone();
    (memory, Some(ended))
}

/// announcedRun: a queued message cancelled before it started may be the activity run, but the
/// turn ended however the last run that did run ended.
fn announced_run(projection: &Value) -> Option<&Value> {
    let run = super::error_banner::activity_run(projection)?;
    if s(run, "status") != "cancelled" || !run["startedAt"].is_null() {
        return Some(run);
    }
    super::error_banner::latest_executed(projection["runs"].as_array().map(Vec::as_slice).unwrap_or_default())
}

/// The open thread's activity, or `None` while its detail hasn't loaded.
fn snapshot(app: &App, thread: &str) -> Option<Snapshot> {
    let projection = &app.state().thread_views.get(thread)?.projection;
    // A provider-native subagent's status is its bar's; it has no turns to announce.
    let shell = app.state().threads.iter().find(|t| s(t, "id") == thread).filter(|t| !crate::chat::subagent_bar::native(t))?;
    let run = announced_run(projection);
    let approval = super::requests::pending_approvals(projection).first().map(|r| s(r, "id").to_owned());
    let question = super::question::active(app).map(|q| q.id().to_owned());
    let (status, request) = match (approval, question) {
        (Some(id), _) => (Status::Approval, Some(id)),
        (None, Some(id)) => (Status::Input, Some(id)),
        // A secret request has no runtime request; the shell carries it.
        _ if shell["hasPendingUserInput"] == true => (Status::Input, None),
        _ if !shell["activeRunId"].is_null() || run.is_some_and(|r| ACTIVE.contains(&s(r, "status"))) => (Status::Working, None),
        _ => (Status::Idle, None),
    };
    Some(Snapshot { status, request, run: run.map(|r| s(r, "id").to_owned()), run_status: run.map(|r| s(r, "status").to_owned()) })
}

/// Follows the open thread's activity; a change worth saying replaces the region's text.
pub fn poll(app: &mut App) -> bool {
    let thread = match app.frame_route() {
        Route::Thread(id) => Some(id.clone()),
        _ => None,
    };
    let snapshot = thread.as_deref().and_then(|id| snapshot(app, id));
    let state = &mut app.ui.chat.timeline.announcer;
    if state.thread != thread {
        *state = State { thread, count: state.count, ..State::default() };
    }
    let Some(snapshot) = snapshot else { return false };
    if state.memory.as_ref().is_some_and(|m| m.snapshot == snapshot) {
        return false;
    }
    let (memory, said) = next(state.memory.as_ref(), snapshot);
    state.memory = Some(memory);
    let Some(said) = said else { return false };
    state.said = Some(said);
    state.count += 1;
    true
}

/// The region, kept in the tree but unseen: zero opacity would drop it from accessibility too.
pub fn view(app: &App) -> El {
    let state = &app.ui.chat.timeline.announcer;
    let words = state.said.map_or_else(String::new, |w| if state.count % 2 == 0 { w.to_owned() } else { format!("{w}\u{200b}") });
    let region = text(words).live_region(LiveRegion::Polite).color(Color::TRANSPARENT).single_line();
    el().key("thread-activity-announcer").absolute(0., 0.).w(1.).h(1.).clip_rounded(0.).child(region)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(status: Status, request: Option<&str>, run: Option<&str>, run_status: Option<&str>) -> Snapshot {
        Snapshot { status, request: request.map(str::to_owned), run: run.map(str::to_owned), run_status: run_status.map(str::to_owned) }
    }

    /// What is said across `steps`, the first being the silent baseline.
    fn said(steps: &[Snapshot]) -> Vec<Option<&'static str>> {
        let mut memory: Option<Memory> = None;
        steps
            .iter()
            .map(|step| {
                let (m, said) = next(memory.as_ref(), step.clone());
                memory = Some(m);
                said
            })
            .collect()
    }

    #[test]
    fn a_turn_is_announced_starting_waiting_and_ending_once() {
        let idle = snap(Status::Idle, None, Some("r0"), Some("completed"));
        assert_eq!(
            said(&[
                idle.clone(),
                snap(Status::Working, None, None, None),
                snap(Status::Working, None, Some("r1"), Some("running")),
                snap(Status::Approval, Some("a1"), Some("r1"), Some("waiting")),
                snap(Status::Approval, Some("a1"), Some("r1"), Some("waiting")),
                snap(Status::Working, None, Some("r1"), Some("running")),
                snap(Status::Idle, None, Some("r1"), Some("running")),
                snap(Status::Idle, None, Some("r1"), Some("completed")),
            ]),
            [None, Some("Working"), None, Some("Needs approval"), None, None, None, Some("Done")]
        );
    }

    #[test]
    fn opening_a_thread_and_stale_runs_stay_silent() {
        // Opening mid-turn is the baseline; its ending is still this turn's.
        assert_eq!(said(&[snap(Status::Working, None, Some("r1"), Some("running")), snap(Status::Idle, None, Some("r1"), Some("failed"))]), [None, Some("Failed")]);
        // A send that failed before any run: the last run's ending is not repeated.
        assert_eq!(
            said(&[snap(Status::Idle, None, Some("r0"), Some("completed")), snap(Status::Working, None, Some("r0"), Some("completed")), snap(Status::Idle, None, Some("r0"), Some("completed"))]),
            [None, Some("Working"), None]
        );
        // A rolled-back run ends the turn without a word.
        assert_eq!(
            said(&[snap(Status::Idle, None, None, None), snap(Status::Working, None, Some("r1"), Some("running")), snap(Status::Idle, None, Some("r1"), Some("rolled_back"))]),
            [None, Some("Working"), None]
        );
    }
}
