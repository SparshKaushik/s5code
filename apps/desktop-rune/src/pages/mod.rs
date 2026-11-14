//! The sidebar footer's utility pages: `/pull-requests`, which replaces the main column, and
//! `/usage`, a route modal over the frame like settings.

pub mod avatars;
pub mod pr_detail;
pub mod pr_filters;
pub mod pr_row;
pub mod pull_requests;
pub mod usage;
pub mod usage_limits;
pub mod usage_merge;
pub mod usage_prices;
pub mod usage_tags;

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
    let changed = pull_requests::poll(s) | pr_row::poll(s) | pr_detail::poll(s) | usage::poll(s) | usage_prices::poll(s);
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
        .chain(entries.iter().map(|e| &e["author"]))
        .chain(pr_filters::hosted_actors(&pages.pull_requests.picker));
    pages.avatars.want(avatars::urls(actors), waker);
}

/// Shift held alone shows the pull request rows' quick actions.
pub fn on_modifier(s: &mut App, e: &KeyEvent) -> bool {
    pr_row::on_key(s, e)
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

/// The list grouped by its authored and review-requested reads, which add an older authored row.
fn open_partitions(s: &mut App) {
    open_prs(s, |_| {});
    let u = &mut s.ui.pages.pull_requests;
    let entries = u.result.as_ref().and_then(|r| r["entries"].as_array()).cloned().unwrap_or_default();
    let mut older = entries.first().cloned().unwrap_or(Value::Null);
    older["number"] = json!(2401);
    older["title"] = json!("feat(desktop-rune): keep pull request filters across restarts");
    older["url"] = json!("https://github.com/s5labs/s5code/pull/2401");
    older["headBranch"] = json!("feat/rune-pr-filters");
    older["createdAt"] = json!("2026-10-27T11:20:00.000Z");
    older["updatedAt"] = json!("2026-10-28T16:40:00.000Z");
    let authored: Vec<Value> = entries.iter().filter(|e| e["author"]["login"] == "sparsh").cloned().chain([older]).collect();
    let reviewing: Vec<Value> = entries.iter().filter(|e| e["viewerReviewRequested"] == true).cloned().collect();
    pull_requests::hold_partitions(u, json!({"entries": authored}), json!({"entries": reviewing}));
}

/// The Filters menu open on `submenu`, its picker searching the hosts with nothing typed yet.
fn open_picker(s: &mut App, submenu: &'static str, kind: &str, answer: Value) {
    let u = &mut s.ui.pages.pull_requests;
    u.menu = Some("filters");
    u.submenu = Some(submenu);
    pr_filters::open(&mut u.picker);
    pr_filters::hold(&mut u.picker, kind, None, None, answer);
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
    usage::scene_scan(s, crate::env::LOCAL, recording.get("summary").cloned());
}

fn light(s: &mut App) {
    s.appearance = Appearance::Light;
}

pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("pull-requests", |s| open_prs(s, |_| {})),
    ("pull-requests-empty", |s| open_prs(s, |r| r["entries"] = json!([]))),
    ("pull-requests-filtered", |s| {
        open_prs(s, |r| r["entries"] = json!([]));
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
    ("pull-requests-row-actions", |s| {
        open_prs(s, |_| {});
        s.ui.pages.pull_requests.row.speed = true;
    }),
    ("pull-requests-stack-menu", |s| {
        open_prs(s, |_| {});
        pr_row::scene(s, 2479, "stack");
    }),
    ("pull-requests-stack-long", |s| {
        open_prs(s, |_| {});
        pr_row::scene(s, 2479, "stack-long");
    }),
    ("pull-requests-checks-popover", |s| {
        open_prs(s, |_| {});
        pr_row::scene(s, 2481, "checks");
    }),
    ("pull-requests-breakdown-tooltip", |s| {
        open_prs(s, |_| {});
        pr_row::scene(s, 2481, "breakdown");
    }),
    ("pull-requests-close-sweep", |s| {
        open_prs(s, |_| {});
        pr_row::scene(s, 2468, "sweep");
    }),
    ("pull-requests-busy-spinner", |s| {
        open_prs(s, |_| {});
        pr_row::scene(s, 2468, "sweep");
        let u = &mut s.ui.pages.pull_requests;
        u.query.replace_text("login");
        u.pending = Some((0, false));
    }),
    ("pull-requests-row-actions-read-only", |s| {
        open_prs(s, |_| {});
        let row = &mut s.ui.pages.pull_requests.row;
        row.speed = true;
        row.session.data = Some(json!({"authenticated": true, "scopes": ["orchestration:read"]}));
    }),
    ("pull-requests-provider-marks", |s| {
        open_prs(s, |r| {
            r["providers"] = json!([
                {"host": "github.com", "kind": "github", "configured": true},
                {"host": "gitlab.com", "kind": "gitlab", "configured": true},
                {"host": "bitbucket.org", "kind": "bitbucket", "configured": true},
                {"host": "dev.azure.com", "kind": "azure-devops", "configured": true},
            ]);
            for (number, kind, host) in [(2476, "gitlab", "gitlab.com"), (318, "bitbucket", "bitbucket.org"), (57, "azure-devops", "dev.azure.com")] {
                if let Some(e) = r["entries"].as_array_mut().and_then(|rows| rows.iter_mut().find(|e| e["number"] == number)) {
                    e["provider"] = json!(kind);
                    e["host"] = json!(host);
                }
            }
        });
    }),
    ("pull-requests-stack-saved", |s| {
        open_prs(s, |_| {});
        pr_row::scene(s, 2479, "stack-saved");
    }),
    ("pull-requests-stack-unlisted-layer", |s| {
        open_prs(s, |r| {
            if let Some(entries) = r["entries"].as_array_mut() {
                entries.retain(|e| e["number"] != 2476);
            }
        });
        pr_row::scene(s, 2479, "stack-unlisted");
    }),
    ("pull-requests-matched-elsewhere", |s| {
        open_prs(s, |r| {
            if let Some(entries) = r["entries"].as_array_mut() {
                entries.retain(|e| e["number"] == 57 || e["number"] == 2470);
            }
        });
        let u = &mut s.ui.pages.pull_requests;
        u.query.replace_text("jitter");
        pull_requests::submit(u);
        u.requested = Some(pull_requests::input(u));
    }),
    ("pull-requests-author-picker", |s| {
        open_prs(s, |_| {});
        let hosted = json!([{"login": "octocat", "name": "The Octocat", "avatarUrl": null}, {"login": "mara", "name": "Mara Okafor", "avatarUrl": null}]);
        open_picker(s, "author", "authors", json!({"authors": hosted, "labels": [], "truncated": false}));
    }),
    ("pull-requests-label-picker", |s| {
        open_prs(s, |_| {});
        let u = &mut s.ui.pages.pull_requests;
        u.filters.labels = vec!["bug".into(), "server".into()];
        u.requested = Some(pull_requests::input(u));
        let hosted = json!([{"name": "docs", "color": "0075ca"}, {"name": "bug", "color": "d73a4a"}]);
        open_picker(s, "labels", "labels", json!({"labels": hosted, "authors": [], "truncated": false}));
    }),
    ("pull-requests-filters-hosts", |s| {
        open_prs(s, |_| {});
        let u = &mut s.ui.pages.pull_requests;
        u.hosts = u.result.as_ref().and_then(|r| r["providers"].as_array()).cloned().unwrap_or_default();
        u.hosts.push(json!({"host": "gitlab.com", "kind": "gitlab", "searchesOnHost": true, "projectCount": 1, "configured": false, "detail": "glab is not authenticated for gitlab.com."}));
        u.menu = Some("filters");
        u.submenu = Some("host");
    }),
    ("pull-requests-projects-menu", |s| {
        open_prs(s, |_| {});
        let first = s.state().projects.first().map(|p| crate::data::s(p, "id").to_string()).unwrap_or_default();
        let u = &mut s.ui.pages.pull_requests;
        if let Some(r) = &mut u.result {
            r["errors"] = json!([{"projectId": first, "message": "gh is not authenticated for github.com. Run `gh auth login` and try again."}]);
        }
        u.menu = Some("filters");
        u.submenu = Some("project");
    }),
    ("pull-requests-draft-typed", |s| {
        open_prs(s, |_| {});
        let u = &mut s.ui.pages.pull_requests;
        u.query.replace_text("draft:true");
        pull_requests::submit(u);
        u.requested = Some(pull_requests::input(u));
        u.menu = Some("filters");
        u.submenu = Some("draft");
    }),
    ("pull-requests-host-chosen", |s| {
        open_prs(s, |_| {});
        let u = &mut s.ui.pages.pull_requests;
        u.hosts = u.result.as_ref().and_then(|r| r["providers"].as_array()).cloned().unwrap_or_default();
        u.filters.host = Some("github.com".into());
        u.requested = Some(pull_requests::input(u));
    }),
    ("pull-requests-partitions", open_partitions),
    ("pull-requests-partitions-searching", |s| {
        open_partitions(s);
        s.ui.pages.pull_requests.query.replace_text("desktop");
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
    ("pull-requests-maximized", |s| {
        open_detail(s, 0, |_| {});
        s.ui.pages.pull_requests.maximized = true;
    }),
    ("pull-requests-panel-narrow-search", |s| open_detail(s, 0, |_| {})),
    ("pull-requests-panel-hidden", |s| {
        open_detail(s, 0, |_| {});
        pull_requests::toggle_panel(&mut s.ui.pages.pull_requests);
    }),
    ("pull-requests-condensed", |s| {
        open_prs(s, |_| {});
        pull_requests::scene_condensed(&mut s.ui.pages.pull_requests);
    }),
    ("pull-requests-condensed-search", |s| {
        open_prs(s, |_| {});
        let u = &mut s.ui.pages.pull_requests;
        pull_requests::scene_condensed(u);
        u.query.replace_text("login");
        pull_requests::submit(u);
        u.requested = Some(pull_requests::input(u));
        u.search_open = true;
    }),
    ("pull-requests-condensed-state-menu", |s| {
        open_prs(s, |_| {});
        let u = &mut s.ui.pages.pull_requests;
        pull_requests::scene_condensed(u);
        u.menu = Some("scope-state");
    }),
    ("pull-requests-host-menu-unavailable", |s| {
        open_prs(s, |_| {});
        let u = &mut s.ui.pages.pull_requests;
        u.hosts = u.result.as_ref().and_then(|r| r["providers"].as_array()).cloned().unwrap_or_default();
        u.hosts.push(json!({"host": "gitlab.com", "kind": "gitlab", "searchesOnHost": true, "projectCount": 1, "configured": false, "detail": "glab is not authenticated for gitlab.com."}));
        u.menu = Some("host");
    }),
    ("pull-requests-project-chosen", |s| {
        open_prs(s, |_| {});
        let first = s.state().projects.iter().find(|p| p["isScratch"] != true).map(|p| crate::data::s(p, "id").to_string());
        let u = &mut s.ui.pages.pull_requests;
        u.filters.project = first;
        u.requested = Some(pull_requests::input(u));
        u.menu = Some("filters");
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
            summary["sessions"] = json!({});
        }
    }),
    ("usage-loading", |s| {
        open_usage(s, "cost");
        s.ui.pages.usage.summary = None;
    }),
    ("usage-error", |s| {
        open_usage(s, "cost");
        usage::error_scene(s);
    }),
    ("usage-error-menu", |s| {
        open_usage(s, "cost");
        usage::error_scene(s);
        s.ui.pages.usage.menu = Some("environments");
    }),
    ("usage-offline", |s| {
        open_usage(s, "cost");
        usage::offline_scene(s);
    }),
    ("usage-environments-scanning", |s| {
        open_usage(s, "cost");
        usage::studio_scene(s, None);
        s.ui.pages.usage.menu = Some("environments");
    }),
    ("usage-environments-duplicates", |s| {
        open_usage(s, "cost");
        usage::studio_scene(s, Some(|_| {}));
        s.ui.pages.usage.menu = Some("environments");
    }),
    ("usage-update-required", |s| {
        open_usage(s, "cost");
        usage::studio_scene(s, Some(|summary| summary["contractVersion"] = json!(3)));
        s.ui.pages.usage.menu = Some("environments");
    }),
    ("usage-provider-loading", |s| {
        open_usage(s, "cost");
        usage::updating_scene(s);
    }),
    ("usage-provider-loading-menu", |s| {
        open_usage(s, "tokens");
        usage::updating_scene(s);
        s.ui.pages.usage.menu = Some("environments");
    }),
    ("usage-limits-light", |s| {
        open_usage(s, "limits");
        light(s);
    }),
    ("usage-cost-light", |s| {
        open_usage(s, "cost");
        light(s);
    }),
    ("usage-limits-pooled", |s| {
        open_usage(s, "limits");
        usage_limits::pooled_scene(s);
    }),
    ("usage-limits-pooled-popover", |s| {
        open_usage(s, "limits");
        usage_limits::pooled_scene(s);
        s.ui.pages.usage.limits.segment = Some(usage_limits::segment_key("codex", "session", "session", "local:codex_work"));
    }),
    ("usage-limits-pooled-light", |s| {
        open_usage(s, "limits");
        usage_limits::pooled_scene(s);
        light(s);
    }),
    ("usage-limits-environments", |s| {
        open_usage(s, "limits");
        usage_limits::environments_scene(s);
        s.ui.pages.usage.limits.segment = Some(usage_limits::segment_key("codex", "session", "session", "local:codex"));
    }),
    ("usage-limits-environments-menu", |s| {
        open_usage(s, "limits");
        usage_limits::environments_scene(s);
        s.ui.pages.usage.environments = Some(vec![crate::env::LOCAL.into()]);
        s.ui.pages.usage.menu = Some("environments");
    }),
    ("usage-cursor-keychain", |s| {
        open_usage(s, "cost");
        usage::cursor_scene(s);
    }),
    ("usage-limits-cursor-keychain", |s| {
        open_usage(s, "limits");
        usage::cursor_scene(s);
    }),
    ("usage-cursor-keychain-environments", |s| {
        open_usage(s, "cost");
        usage::cursor_environments_scene(s);
    }),
    ("usage-limits-cursor-keychain-environments", |s| {
        open_usage(s, "limits");
        usage::cursor_environments_scene(s);
    }),
    ("usage-unpriced-tooltip", |s| {
        open_usage(s, "cost");
        s.ui.pages.usage.unpriced_tip = true;
    }),
    ("usage-tag-dialog", |s| {
        open_usage(s, "cost");
        usage_tags::scene(s);
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
    ("usage-prices-environments", |s| {
        open_usage(s, "cost");
        usage::studio_scene(s, Some(|_| {}));
        usage_prices::environments_scene(s);
    }),
];
