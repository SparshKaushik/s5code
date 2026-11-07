//! The sidebar footer's utility pages: `/pull-requests`, which replaces the main column, and
//! `/usage`, a route modal over the frame like settings.

pub mod pull_requests;
pub mod usage;

use rune::{KeyEvent, Tx};
use serde_json::{Value, json};

use crate::{App, Appearance, Route};

#[derive(Default)]
pub struct Ui {
    pub pull_requests: pull_requests::Ui,
    pub usage: usage::Ui,
    /// The last thread or draft shown, where the footer's Back returns from a utility page.
    pub main_route: Option<Route>,
}

pub fn poll(s: &mut App) -> bool {
    pull_requests::poll(s) | usage::poll(s)
}

pub fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    usage::on_key(s, e, tx) || pull_requests::on_key(s, e, tx)
}

/// A bundled recording next to the shell fixtures (`fixtures/pull-requests.json`, `usage.json`).
pub fn fixture(name: &str) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(crate::data::fixtures_dir().join(name)).ok()?).ok()
}

/// The footer's Back: the thread or draft the utility page was opened over.
pub fn back(s: &mut App) {
    let route = s.ui.pages.main_route.clone().unwrap_or(Route::Home { project_id: None });
    crate::shell::sidebar::go(s, route);
}

fn open_prs(s: &mut App, entries: impl Fn(&mut Value)) {
    crate::scene_clock(s);
    s.ui.pages = Ui::default();
    let mut result = fixture("pull-requests.json").unwrap_or(Value::Null);
    entries(&mut result);
    s.route = Route::PullRequests;
    let u = &mut s.ui.pages.pull_requests;
    u.result = Some(result);
    // Recorded as already requested, so poll() leaves the scene's state alone.
    u.requested = Some(pull_requests::input(u));
}

/// Usage over "Replay showcase", with the recorded limits patched onto codex and Claude.
fn open_usage(s: &mut App, metric: &'static str) {
    crate::scene_clock(s);
    s.ui.pages = Ui::default();
    let replay = s.state().threads.iter().find(|t| t["title"] == "Replay showcase").map(|t| Route::Thread(crate::data::s(t, "id").into()));
    s.ui.settings.return_route = replay;
    s.route = Route::Usage;
    let recording = fixture("usage.json").unwrap_or(Value::Null);
    if let crate::data::Source::Fixture(state) = &mut s.src
        && let Some(providers) = state.config.as_mut().and_then(|c| c["providers"].as_array_mut())
    {
        for p in providers {
            let driver = crate::data::s(p, "driver").to_string();
            if let Some(limits) = recording["limits"].get(&driver) {
                p["usageLimits"] = limits.clone();
                p["auth"] = recording["auth"][&driver].clone();
                p["status"] = json!("ready");
            }
        }
    }
    let u = &mut s.ui.pages.usage;
    u.metric = metric;
    u.summary = recording.get("summary").cloned();
    u.requested = Some(usage::input(u, s.now_ms));
}

fn light(s: &mut App) {
    s.appearance = Appearance::Light;
}

pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("pull-requests", |s| open_prs(s, |_| {})),
    ("pull-requests-empty", |s| open_prs(s, |r| r["entries"] = json!([]))),
    ("pull-requests-filtered", |s| {
        open_prs(s, |_| {});
        s.ui.pages.pull_requests.query.replace_text("no such branch");
    }),
    ("pull-requests-error", |s| {
        open_prs(s, |_| {});
        s.ui.pages.pull_requests.error = Some("GitHub rate limit exceeded for github.com.".into());
    }),
    ("pull-requests-unavailable", |s| {
        open_prs(s, |_| {});
        s.ui.pages.pull_requests.result = None;
        s.ui.pages.pull_requests.error = Some("gh is not authenticated for github.com. Run `gh auth login` and try again.".into());
    }),
    ("pull-requests-loading", |s| {
        open_prs(s, |_| {});
        s.ui.pages.pull_requests.result = None;
    }),
    ("pull-requests-menu", |s| {
        open_prs(s, |_| {});
        s.ui.pages.pull_requests.menu = Some("filters");
        s.ui.pages.pull_requests.submenu = Some("state");
    }),
    ("pull-requests-light", |s| {
        open_prs(s, |_| {});
        light(s);
    }),
    ("usage-limits", |s| open_usage(s, "limits")),
    ("usage-cost", |s| open_usage(s, "cost")),
    ("usage-tokens", |s| open_usage(s, "tokens")),
    ("usage-breakdown-day", |s| {
        open_usage(s, "cost");
        s.ui.pages.usage.by_period = true;
    }),
    ("usage-empty", |s| {
        open_usage(s, "cost");
        if let Some(summary) = &mut s.ui.pages.usage.summary {
            summary["buckets"] = json!([]);
            summary["sources"] = json!([]);
        }
    }),
    ("usage-loading", |s| {
        open_usage(s, "cost");
        s.ui.pages.usage.summary = None;
    }),
    ("usage-error", |s| {
        open_usage(s, "cost");
        s.ui.pages.usage.summary = None;
        s.ui.pages.usage.error = Some("Usage diagnostics need the admin scope on this environment.".into());
    }),
    ("usage-limits-light", |s| {
        open_usage(s, "limits");
        light(s);
    }),
    ("usage-cost-light", |s| {
        open_usage(s, "cost");
        light(s);
    }),
];
