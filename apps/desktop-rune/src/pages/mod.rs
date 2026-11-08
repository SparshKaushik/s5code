//! The sidebar footer's utility pages: `/pull-requests`, which replaces the main column, and
//! `/usage`, a route modal over the frame like settings.

pub mod avatars;
pub mod pr_detail;
pub mod pull_requests;
pub mod usage;
pub mod usage_prices;

use rune::{KeyEvent, Tx};
use serde_json::{Value, json};

use crate::{App, Appearance, Route};

#[derive(Default)]
pub struct Ui {
    pub pull_requests: pull_requests::Ui,
    pub detail: pr_detail::Ui,
    pub avatars: avatars::Avatars,
    pub usage: usage::Ui,
    /// The last thread or draft shown, where the footer's Back returns from a utility page.
    pub main_route: Option<Route>,
}

pub fn poll(s: &mut App) -> bool {
    let changed = pull_requests::poll(s) | pr_detail::poll(s) | usage::poll(s) | usage_prices::poll(s);
    want_avatars(s);
    changed
}

/// Starts the avatars the pull requests page shows, the open detail's first.
fn want_avatars(s: &mut App) {
    if !matches!(s.frame_route(), Route::PullRequests) {
        return;
    }
    let waker = s.ui.panel.waker.as_ref();
    let pages = &mut s.ui.pages;
    let entries = pages.pull_requests.result.as_ref().and_then(|r| r["entries"].as_array()).map(Vec::as_slice).unwrap_or(&[]);
    let (detail, activity) = (pages.detail.detail.as_ref(), pages.detail.activity.as_ref());
    fn list<'a>(v: Option<&'a Value>, key: &str) -> impl Iterator<Item = &'a Value> {
        v.and_then(|v| v[key].as_array()).into_iter().flatten()
    }
    let actors = detail
        .map(|d| &d["author"])
        .into_iter()
        .chain(list(detail, "reviewers"))
        .chain(list(activity, "comments").map(|c| &c["author"]))
        .chain(list(activity, "reviewThreads").flat_map(|t| t["comments"].as_array().into_iter().flatten()).map(|c| &c["author"]))
        .chain(entries.iter().map(|e| &e["author"]));
    pages.avatars.want(avatars::urls(actors), waker);
}

pub fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    usage::on_key(s, e, tx) || pr_detail::on_key(s, e, tx) || pull_requests::on_key(s, e, tx)
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

/// The list with its first row open in the detail, loaded from `pull-request-detail.json`.
fn open_detail(s: &mut App, tab: usize, then: impl Fn(&mut pr_detail::Ui)) {
    open_prs(s, |_| {});
    let first = s.ui.pages.pull_requests.result.as_ref().and_then(|r| r["entries"][0]["url"].as_str().map(str::to_string));
    s.ui.pages.pull_requests.selected = first;
    s.ui.pages.detail.tab = tab;
    pr_detail::poll(s);
    then(&mut s.ui.pages.detail);
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
    // Scenes render the same in any machine's zone.
    u.zone_name = "UTC".into();
    u.zone = crate::data::Zone::Fixed(0);
    u.requested = Some(usage::input(u, s.now_ms, &json!([])));
    u.answered = u.requested.clone();
    u.preferences_read = true;
}

fn light(s: &mut App) {
    s.appearance = Appearance::Light;
}

pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("pull-requests", |s| open_prs(s, |_| {})),
    ("pull-requests-empty", |s| open_prs(s, |r| r["entries"] = json!([]))),
    ("pull-requests-filtered", |s| {
        open_prs(s, |_| {});
        let u = &mut s.ui.pages.pull_requests;
        u.query.replace_text("no such branch");
        pull_requests::submit(u);
        u.requested = Some(pull_requests::input(u));
    }),
    ("pull-requests-searching", |s| {
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
    ("pull-requests-no-projects", |s| {
        open_prs(s, |r| r["entries"] = json!([]));
        if let crate::data::Source::Fixture(state) = &mut s.src {
            state.projects.clear();
        }
    }),
    ("pull-requests-detail", |s| open_detail(s, 0, |_| {})),
    ("pull-requests-detail-timeline", |s| open_detail(s, 1, |_| {})),
    ("pull-requests-detail-code", |s| {
        open_detail(s, 2, |_| {});
        s.ui.pages.detail.expanded.insert("apps/desktop-rune/src/pages/usage.rs".into());
    }),
    ("pull-requests-detail-checks", |s| {
        open_detail(s, 0, |_| {});
        s.ui.pages.detail.checks_open = true;
        s.ui.pages.detail.description_closed = true;
    }),
    ("pull-requests-detail-menu", |s| open_detail(s, 0, |u| u.menu = true)),
    ("pull-requests-detail-merged", |s| {
        open_detail(s, 0, |_| {});
        if let Some(d) = &mut s.ui.pages.detail.detail {
            d["state"] = json!("merged");
        }
    }),
    ("pull-requests-detail-error", |s| {
        open_detail(s, 0, |_| {});
        let u = &mut s.ui.pages.detail;
        u.detail = None;
        u.error = Some("Pull request #2481 not found. It may be an issue rather than a pull request, or this account can't see it.".into());
    }),
    ("pull-requests-detail-light", |s| {
        open_detail(s, 0, |_| {});
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
    ("usage-model", |s| {
        open_usage(s, "cost");
        s.ui.pages.usage.model = Some("claude\0\0claude-opus-5-5".into());
    }),
    ("usage-model-unpriced", |s| {
        open_usage(s, "cost");
        let u = &mut s.ui.pages.usage;
        for b in u.summary.as_mut().and_then(|v| v["buckets"].as_array_mut()).into_iter().flatten() {
            if b["model"] == "kimi-k3" {
                b["unpricedRecords"] = b["records"].clone();
            }
        }
        u.model = Some("opencode\0moonshot\0kimi-k3".into());
    }),
    ("usage-prices", |s| {
        open_usage(s, "cost");
        if let crate::data::Source::Fixture(state) = &mut s.src
            && let Some(config) = state.config.as_mut()
        {
            config["settings"]["usagePriceOverrides"] = json!({"glm-4.6": {"inputCostPerMillionTokens": 0.6, "outputCostPerMillionTokens": 2.2, "cacheReadCostPerMillionTokens": 0.11}});
            config["settings"]["usageModelAliases"] = json!({"claude-opus-5-5-preview": "claude-opus-5-5"});
            config["environment"]["capabilities"]["usageModelAliases"] = json!(true);
        }
        usage_prices::open(s, Some("kimi-k2-thinking"));
    }),
];
