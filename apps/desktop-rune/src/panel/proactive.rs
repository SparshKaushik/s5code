//! Opening the diff on its own when a run that changed a lot finishes (useProactivePanels), with
//! "Open pull requests and changes panels proactively" on. A panel the user opened, closed or
//! switched since the run started wins.

use serde_json::Value;

use super::Surface;
use crate::{App, Route, data};

#[derive(Default)]
pub struct Watch {
    thread: String,
    /// The run that was running at the last look: `None` before the first look at this thread
    /// (web's `undefined`, so entering a thread may still open its last run's diff).
    previous: Option<Option<String>>,
    /// The panel as the user left it when the run started.
    baseline: (bool, Option<Surface>),
}

const ACTIVE: [&str; 5] = ["preparing", "queued", "starting", "running", "waiting"];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Action {
    Defer,
    Ignore,
    Open,
}

/// `shouldOpenProactiveTurnDiff`: nothing running, the latest run completed, and it is the run
/// seen running (or this is the first look).
fn newly_completed<'a>(previous: &Option<Option<String>>, running: Option<&str>, latest: Option<(&'a str, &str)>) -> Option<&'a str> {
    let (id, status) = latest?;
    let settled = !ACTIVE.contains(&status);
    let seen = previous.as_ref().is_none_or(|p| p.as_deref() == Some(id));
    (running.is_none() && settled && status == "completed" && seen).then_some(id)
}

/// `resolveProactiveTurnDiffAction`: wait for the checkpoint and the repository status.
fn action(projection: &Value, run: &str, is_repo: Option<bool>) -> Action {
    let checkpoint = projection["checkpoints"].as_array().into_iter().flatten().rfind(|c| data::s(c, "runId") == run && !c["appRunOrdinal"].is_null());
    match (checkpoint, is_repo) {
        (None, _) | (_, None) => Action::Defer,
        (Some(c), _) if data::s(c, "status") == "missing" => Action::Defer,
        (_, Some(false)) => Action::Ignore,
        _ if crate::chat::timeline::actions::proactive_changes(projection, run) => Action::Open,
        _ => Action::Ignore,
    }
}

pub fn poll(app: &mut App) -> bool {
    let Route::Thread(id) = &app.route else {
        app.ui.panel.proactive = Watch::default();
        return false;
    };
    let id = id.clone();
    let panel = (app.ui.shell.panel_open, app.ui.panel.active);
    if app.ui.panel.proactive.thread != id {
        app.ui.panel.proactive = Watch { thread: id.clone(), previous: None, baseline: panel };
    }
    if crate::settings::value(app, "proactivePanelsEnabled") != true {
        return false;
    }
    let Some(view) = app.state().thread_views.get(&id) else { return false };
    let p = &view.projection;
    let runs = p["runs"].as_array().map(Vec::as_slice).unwrap_or_default();
    let running = runs.iter().rfind(|r| ACTIVE.contains(&data::s(r, "status"))).map(|r| data::s(r, "id").to_string());
    let latest = runs.iter().max_by_key(|r| r["ordinal"].as_i64().unwrap_or(0)).map(|r| (data::s(r, "id"), data::s(r, "status")));
    let watch = &app.ui.panel.proactive;
    let completed = newly_completed(&watch.previous, running.as_deref(), latest).map(str::to_string);
    let is_repo = super::context(app).and_then(|(_, cwd)| app.state().vcs.get(&cwd)).and_then(|v| v["local"]["isRepo"].as_bool());
    let act = completed.as_deref().map_or(Action::Ignore, |run| action(p, run, is_repo));
    let checkpoint = completed.as_deref().and_then(|run| {
        p["checkpoints"].as_array().into_iter().flatten().rfind(|c| data::s(c, "runId") == run && !c["appRunOrdinal"].is_null()).cloned()
    });
    let watch = &mut app.ui.panel.proactive;
    let started = running.is_some() && watch.previous.as_ref().is_some_and(|p| *p != running);
    if started {
        watch.baseline = panel;
    }
    if act != Action::Defer {
        watch.previous = Some(running);
    }
    let untouched = watch.baseline == panel;
    match (act, checkpoint) {
        (Action::Open, Some(checkpoint)) if untouched => {
            super::open_diff(app, checkpoint);
            app.ui.panel.proactive.baseline = (app.ui.shell.panel_open, app.ui.panel.active);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, action, newly_completed};
    use serde_json::json;

    #[test]
    fn a_finished_run_opens_once_and_only_when_its_checkpoint_is_big() {
        let done = Some(("r2", "completed"));
        assert_eq!(newly_completed(&None, None, done), Some("r2"), "first look at a thread");
        assert_eq!(newly_completed(&Some(Some("r2".into())), None, done), Some("r2"), "the run seen running");
        assert_eq!(newly_completed(&Some(None), None, done), None, "already finished before we looked");
        assert_eq!(newly_completed(&Some(Some("r2".into())), Some("r3"), done), None, "another run is going");
        assert_eq!(newly_completed(&Some(Some("r2".into())), None, Some(("r2", "failed"))), None);
        let files = json!([{"path": "a", "additions": 1, "deletions": 0}, {"path": "b"}, {"path": "c"}]);
        let p = |status: &str| json!({"checkpoints": [{"runId": "r2", "appRunOrdinal": 2, "status": status, "files": files}]});
        assert_eq!(action(&p("ready"), "r2", Some(true)), Action::Open);
        assert_eq!(action(&p("ready"), "r2", None), Action::Defer, "repository status not loaded");
        assert_eq!(action(&p("missing"), "r2", Some(true)), Action::Defer);
        assert_eq!(action(&json!({}), "r2", Some(true)), Action::Defer, "checkpoint not arrived");
        assert_eq!(action(&p("ready"), "r2", Some(false)), Action::Ignore);
    }
}
