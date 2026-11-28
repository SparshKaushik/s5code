//! The sidebar footer's utility pages: `/pull-requests`, which replaces the main column, and
//! `/usage`, a route modal over the frame like settings.

pub mod avatars;
pub mod pr_actions;
pub mod pr_code;
pub mod pr_conversation;
pub mod pr_detail;
pub mod pr_edit;
pub mod pr_filters;
pub mod pr_media;
pub mod pr_meta;
pub mod pr_guide;
pub mod pr_route;
pub mod pr_row;
pub mod pr_tabs;
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
    /// The Code tab's diff, kept across pull requests like web's mounted tab.
    pub code: pr_code::Ui,
    /// The Guide tab, its watched runs and tour progress kept across pull requests.
    pub guide: pr_guide::Ui,
    pub avatars: avatars::Avatars,
    pub usage: usage::Ui,
    pub route: pr_route::Ui,
    /// The last thread or draft shown, where the footer's Back returns from a utility page.
    pub main_route: Option<Route>,
}

pub fn poll(s: &mut App) -> bool {
    let changed = pr_route::pump(s) | pull_requests::poll(s) | pr_row::poll(s) | pr_detail::poll(s) | pr_meta::poll(s) | pr_guide::poll(s) | usage::poll(s) | usage_prices::poll(s);
    want_avatars(s);
    changed
}

/// Starts the avatars the pull requests page shows, the open detail's first.
fn want_avatars(s: &mut App) {
    if !matches!(s.frame_route(), Route::PullRequests) && crate::panel::active_pull_request(s).is_none() {
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
        .chain(pr_filters::hosted_actors(&pages.pull_requests.picker))
        .chain(pr_meta::actors(&pages.detail.meta));
    pages.avatars.want(avatars::urls(actors), waker);
}

/// Shift held alone shows the pull request rows' quick actions.
pub fn on_modifier(s: &mut App, e: &KeyEvent) -> bool {
    pr_row::on_key(s, e)
}

pub fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    crate::chat::timeline::attachments::on_key(s, e) || usage::on_key(s, e, tx) || pr_detail::on_key(s, e, tx) || pull_requests::on_key(s, e, tx)
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

/// A thread's panel showing a pull request linked to it, among two links: the crumbs lead back
/// to the thread's pull requests, and the menu unlinks it from this thread.
fn panel_thread_scene(s: &mut App) {
    (crate::shell::SCENES[1].1)(s);
    let Some(entry) = fixture("pull-request-detail.json").map(|r| r["detail"].clone()).filter(|e| e["url"].is_string()) else { return };
    let Route::Thread(id) = s.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut s.src {
        state.config.get_or_insert_with(|| json!({}));
        if let Some(thread) = state.threads.iter_mut().find(|v| v["id"] == id.as_str()) {
            thread["projectId"] = entry["projectId"].clone();
            let own = json!({"host": "github.com", "repository": entry["repository"], "number": entry["number"], "url": entry["url"], "source": "manual"});
            let other = json!({"host": "github.com", "repository": "acme/app", "number": 20, "url": "https://github.com/acme/app/pull/20", "source": "agent"});
            thread["pullRequests"] = json!([own, other]);
        }
    }
    crate::panel::open_pull_request(s, entry);
    pr_detail::poll(s);
    s.ui.pages.detail.menu = true;
}

/// A description holding an upload, pictures on a line of their own and one inline.
fn media_scene(s: &mut App, open: bool) {
    let body = "Adds the demo.\n\nhttps://github.com/user-attachments/assets/0f1e2d3c-demo\n\n![Before](https://github.com/user-attachments/assets/1a2b-before) ![After](https://example.com/after.png)\n\nInline ![badge](https://example.com/badge.svg) stays in the text.\n\nThanks!";
    open_detail(s, 0, |u| if let Some(d) = &mut u.detail { d["body"] = json!(body); });
    pr_media::scene(s, open);
}

/// The checkout menu with the repository on a second machine too: the environment picker.
fn scene_checkout_environments(s: &mut App) {
    open_detail(s, 0, |u| u.checkout_menu = true);
    let Some(project) = s.ui.pages.detail.entry.as_ref().map(|e| crate::data::s(e, "projectId").to_string()) else { return };
    let crate::data::Source::Fixture(state) = &mut s.src else { return };
    let Some(own) = state.projects.iter_mut().find(|p| crate::data::s(p, "id") == project) else { return };
    own["repositoryIdentity"]["canonicalKey"] = json!("github.com/sparshkaushik/s5code");
    let mut copy = own.clone();
    copy["id"] = json!("box-copy");
    copy["workspaceRoot"] = json!("/srv/code/s5code");
    let mut remote = crate::data::empty_state();
    remote.config = Some(json!({"environment": {"environmentId": "box-id", "label": "Build box"}, "settings": {"environmentIcon": "server"}}));
    remote.projects = vec![copy.clone()];
    copy[crate::env::TAG] = json!("box");
    state.projects.push(copy);
    s.ui.env.saved.push(crate::env::Saved { id: "box".into(), label: "Build box".into(), base: "https://box.example.com".into(), bearer: String::new() });
    crate::env::park(s, "box", crate::data::Source::Fixture(remote));
    crate::env::adopt(s, "box-copy", "box");
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
        u.pending = vec![(String::new(), 0)];
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
    ("pull-requests-detail-guide-empty", |s| pr_guide::scene(s, "empty")),
    ("pull-requests-detail-guide-ready", |s| pr_guide::scene(s, "ready")),
    ("pull-requests-detail-long", |s| {
        let body: String = (1..=8).map(|n| format!("## Part {n}\n\nThis part walks through one step of the change, with enough prose that the description runs past the panel and its heading has to stay pinned while it scrolls.\n\n- The first point of part {n}\n- The second point of part {n}\n\n")).collect();
        open_detail(s, 0, |u| if let Some(d) = &mut u.detail { d["body"] = json!(body); });
    }),
    ("pull-requests-panel-thread", panel_thread_scene),
    ("pull-requests-detail-guide-wide", |s| {
        pr_guide::scene(s, "ready");
        s.ui.pages.pull_requests.maximized = true;
    }),
    ("pull-requests-detail-guide-long", |s| {
        pr_guide::scene(s, "ready");
        s.ui.pages.pull_requests.maximized = true;
        let summary: String = (1..=6).map(|n| format!("Step {n} of the change reads the period start, converts it to wall time in the chosen zone and formats it, with enough prose that the stop runs past the window while its notes stay beside it.\n\n")).collect();
        let questions: Vec<String> = (1..=8).map(|n| format!("Does case {n} of the tooltip agree with the axis?")).collect();
        if let Some(stop) = s.ui.pages.guide.data.as_mut().and_then(|d| d["guide"]["stops"].get_mut(0)) {
            stop["summary"] = json!(summary);
            stop["questions"] = json!(questions);
        }
    }),
    ("pull-requests-detail-guide-generating", |s| pr_guide::scene(s, "generating")),
    ("pull-requests-detail-guide-finish", |s| pr_guide::scene(s, "finish")),
    ("pull-requests-detail-guide-excerpt-comment", |s| pr_guide::scene(s, "comment")),
    ("pull-requests-detail-menu-guide", |s| pr_guide::scene(s, "menu")),
    ("pull-requests-detail-code", |s| {
        open_detail(s, 2, |_| {});
        pr_code::unfold(s, "apps/desktop-rune/src/pages/usage.rs");
    }),
    ("pull-requests-detail-code-long", |s| {
        open_detail(s, 2, |_| {});
        let mut patch = String::new();
        for (name, lines) in [("apps/desktop-rune/src/pages/usage.rs", 60), ("apps/desktop-rune/src/pages/mod.rs", 12)] {
            patch += &format!("diff --git a/{name} b/{name}\n--- a/{name}\n+++ b/{name}\n@@ -1,{lines} +1,{lines} @@\n");
            for n in 1..=lines {
                patch += &format!("-    let period_{n} = m.periods[{n}];\n+    let period_{n} = m.zone.wall(m.periods[{n}]);\n");
            }
        }
        s.ui.pages.detail.files = Some(rune_kit::diff::parse(&patch));
        s.ui.pages.code.fresh = true;
        pr_detail::poll(s);
        pr_code::unfold(s, "apps/desktop-rune/src/pages/usage.rs");
        pr_code::unfold(s, "apps/desktop-rune/src/pages/mod.rs");
    }),
    ("pull-requests-detail-code-tree", |s| {
        open_detail(s, 2, |_| {});
        pr_code::scene(s, "tree");
    }),
    ("pull-requests-detail-code-split", |s| {
        open_detail(s, 2, |_| {});
        pr_code::scene(s, "split");
    }),
    ("pull-requests-detail-code-line-comment", |s| {
        open_detail(s, 2, |_| {});
        pr_code::scene(s, "line-comment");
    }),
    ("pull-requests-detail-code-inline-threads", |s| {
        open_detail(s, 2, |_| {});
        pr_code::scene(s, "threads");
    }),
    ("pull-requests-detail-code-thread-edit", |s| {
        open_detail(s, 2, |_| {});
        pr_code::scene(s, "thread-edit");
    }),
    ("pull-requests-detail-code-thread-reactions", |s| {
        open_detail(s, 2, |_| {});
        pr_code::scene(s, "thread-reactions");
    }),
    ("pull-requests-detail-code-options-menu", |s| {
        open_detail(s, 2, |_| {});
        pr_tabs::scene(s, 1, Some(360.0));
        pr_code::scene(s, "options");
    }),
    ("pull-requests-detail-code-find", |s| {
        open_detail(s, 2, |_| {});
        pr_code::scene(s, "find");
    }),
    ("pull-requests-detail-code-raw-patch", |s| {
        open_detail(s, 2, |_| {});
        pr_code::scene(s, "raw");
    }),
    ("pull-requests-detail-code-tree-resized", |s| {
        open_detail(s, 2, |_| {});
        pr_code::scene(s, "tree-resized");
    }),
    ("pull-requests-detail-code-withheld", |s| {
        open_detail(s, 2, |_| {});
        pr_code::scene(s, "withheld");
    }),
    ("pull-requests-detail-code-more-files", |s| {
        open_detail(s, 2, |_| {});
        pr_code::scene(s, "more-files");
    }),
    ("pull-requests-detail-code-tests-hidden", |s| {
        open_detail(s, 2, |_| {});
        pr_code::scene(s, "tests-hidden");
    }),
    ("pull-requests-detail-code-file-order", |s| {
        open_detail(s, 2, |_| {});
        pr_code::scene(s, "file-order");
    }),
    ("pull-requests-detail-code-pending-review", |s| {
        open_detail(s, 0, |u| {
            pr_edit::scene_caps(u);
            u.description_closed = true;
            u.comments_closed = true;
            u.edit.composer.review = true;
            u.edit.composer.open = true;
        });
        pr_code::scene(s, "pending-review");
    }),
    ("pull-requests-detail-checks", |s| {
        open_detail(s, 0, |_| {});
        s.ui.pages.detail.checks_open = true;
        s.ui.pages.detail.description_closed = true;
    }),
    ("pull-requests-detail-comments-filtered", |s| {
        open_detail(s, 0, |u| {
            u.description_closed = true;
            let Some(a) = &mut u.activity else { return };
            a["commentsTruncated"] = json!(true);
            if let Some(list) = a["comments"].as_array_mut() {
                for n in 0..12 {
                    let at = format!("2026-11-06T07:{}:00.000Z", 21 + n);
                    let note = json!({"id": format!("note-{n}"), "kind": "issue-comment", "author": {"login": "theo", "name": "Theo", "avatarUrl": null}, "body": format!("Follow-up note {}.", n + 1), "createdAt": at, "url": null, "path": null, "reviewState": null});
                    list.insert(1 + n, note);
                }
            }
            if let Some(threads) = a["reviewThreads"].as_array_mut() {
                let julius = json!({"login": "juliusmarminge", "name": "Julius Marminge", "avatarUrl": null});
                threads.push(json!({"id": "t-resolved", "path": "apps/desktop-rune/src/pages/usage.rs", "line": 40, "side": "RIGHT", "isResolved": true, "isOutdated": false, "comments": [{"id": "tr1", "author": julius, "body": "Nit: rename this.", "createdAt": "2026-11-06T08:05:00.000Z", "url": null}]}));
            }
        })
    }),
    ("pull-requests-detail-summary-more-replies", |s| {
        open_detail(s, 0, |u| {
            u.description_closed = true;
            u.checks_open = true;
            if let Some(d) = &mut u.detail {
                d["checks"][1]["status"] = json!("failure");
            }
            let Some(threads) = u.activity.as_mut().and_then(|a| a["reviewThreads"].as_array_mut()) else { return };
            if let Some(first) = threads.first_mut() {
                first["commentCount"] = json!(6);
            }
            let julius = json!({"login": "juliusmarminge", "name": "Julius Marminge", "avatarUrl": null});
            threads.push(json!({"id": "t-resolved", "path": "apps/desktop-rune/src/pages/usage.rs", "line": 40, "side": "RIGHT", "isResolved": true, "isOutdated": true, "nextCommentsCursor": "next", "comments": [{"id": "tr1", "author": julius, "body": "Nit: rename this.", "createdAt": "2026-11-06T08:05:00.000Z", "url": "https://github.com/s5labs/s5code/pull/2481#discussion_r1"}]}));
        })
    }),
    ("pull-requests-detail-reviewers-verdicts", |s| {
        open_detail(s, 0, |u| {
            let julius = json!({"login": "juliusmarminge", "name": "Julius Marminge", "avatarUrl": null});
            if let Some(d) = &mut u.detail {
                d["reviewers"] = json!([julius.clone(), {"login": "sam", "name": null, "avatarUrl": null}]);
                d["labels"] = json!([]);
                d["capabilities"]["labels"] = json!(false);
            }
            if let Some(list) = u.activity.as_mut().and_then(|a| a["comments"].as_array_mut()) {
                let review = |id: &str, author: Value, state: &str, at: &str| json!({"id": id, "kind": "review", "author": author, "body": "", "createdAt": at, "url": null, "path": null, "reviewState": state});
                list.push(review("v1", julius.clone(), "CHANGES_REQUESTED", "2026-11-06T09:20:00.000Z"));
                list.push(review("v2", json!({"login": "macroscopeapp", "name": "Macroscope", "avatarUrl": null, "isBot": true}), "APPROVED", "2026-11-06T09:30:00.000Z"));
                list.push(review("v3", json!({"login": "theo", "name": "Theo", "avatarUrl": null}), "APPROVED", "2026-11-06T09:52:00.000Z"));
            }
        })
    }),
    ("pull-requests-detail-timeline-events", |s| {
        open_detail(s, 1, |u| {
            if let Some(d) = &mut u.detail {
                d["state"] = json!("merged");
                d["mergedAt"] = json!("2026-11-06T09:55:00.000Z");
                d["closedAt"] = json!("2026-11-06T09:55:00.000Z");
            }
        })
    }),
    ("pull-requests-detail-timeline-threads", |s| {
        open_detail(s, 1, |u| {
            let Some(a) = &mut u.activity else { return };
            if let Some(list) = a["comments"].as_array_mut() {
                list.push(json!({"id": "c3", "kind": "issue-comment", "author": {"login": "theo", "name": "Theo", "avatarUrl": null}, "body": "Ship it once CI is green.", "createdAt": "2026-11-06T09:52:00.000Z", "url": null, "path": null, "reviewState": null}));
            }
            if let Some(threads) = a["reviewThreads"].as_array_mut() {
                if let Some(first) = threads.first_mut() {
                    first["commentCount"] = json!(6);
                }
                let julius = json!({"login": "juliusmarminge", "name": "Julius Marminge", "avatarUrl": null});
                threads.push(json!({"id": "t-resolved", "path": "apps/desktop-rune/src/pages/usage.rs", "line": 40, "side": "RIGHT", "isResolved": true, "isOutdated": true, "comments": [{"id": "tr1", "author": julius, "body": "Nit: rename this.", "createdAt": "2026-11-06T08:05:00.000Z", "url": null}]}));
            }
            u.open_comments.insert("comments:c3".into());
        })
    }),
    ("pull-requests-detail-timeline-approvals", |s| {
        open_detail(s, 1, |u| {
            if let Some(list) = u.activity.as_mut().and_then(|a| a["comments"].as_array_mut()) {
                list.push(json!({"id": "v3", "kind": "review", "author": {"login": "theo", "name": "Theo", "avatarUrl": null}, "body": "Ship it.", "createdAt": "2026-11-06T09:52:00.000Z", "url": null, "path": null, "reviewState": "APPROVED"}));
            }
        })
    }),
    ("pull-requests-detail-timeline-loading", |s| open_detail(s, 1, |u| u.activity = None)),
    ("pull-requests-detail-comments-loading", |s| {
        open_detail(s, 0, |u| {
            u.activity = None;
            u.description_closed = true;
        })
    }),
    ("pull-requests-detail-timeline-verdicts", |s| {
        open_detail(s, 1, |u| {
            if let Some(list) = u.activity.as_mut().and_then(|a| a["comments"].as_array_mut()) {
                let julius = json!({"login": "juliusmarminge", "name": "Julius Marminge", "avatarUrl": null});
                list.push(json!({"id": "v-stale", "kind": "review", "author": {"login": "theo", "name": "Theo", "avatarUrl": null}, "body": "", "createdAt": "2026-11-06T09:20:00.000Z", "url": null, "path": null, "reviewState": "APPROVED"}));
                list.push(json!({"id": "v-fresh", "kind": "review", "author": julius, "body": "The clamp still drops the **last** row; see [the fixture](https://github.com/s5labs/s5code/pull/2481/files).", "createdAt": "2026-11-06T09:56:00.000Z", "url": "https://github.com/s5labs/s5code/pull/2481#pullrequestreview-1", "path": null, "reviewState": "CHANGES_REQUESTED"}));
            }
        })
    }),
    ("pull-requests-detail-collapsed-open", |s| {
        open_detail(s, 0, |u| {
            u.description_closed = true;
            let julius = json!({"login": "juliusmarminge", "name": "Julius Marminge", "avatarUrl": null});
            let long = (1..=24).map(|n| format!("- Step {n}: re-read the [list](https://github.com/s5labs/s5code/pull/2481) before the clamp.")).collect::<Vec<_>>().join("\n");
            if let Some(threads) = u.activity.as_mut().and_then(|a| a["reviewThreads"].as_array_mut()) {
                threads.push(json!({"id": "t-open", "path": "apps/desktop-rune/src/pages/usage.rs", "line": 40, "side": "RIGHT", "isResolved": true, "isOutdated": false, "comments": [
                    {"id": "tro1", "author": julius, "body": "Nit: see [the naming guide](https://github.com/s5labs/s5code/blob/main/AGENTS.md) and rename this.", "createdAt": "2026-11-06T08:05:00.000Z", "url": "https://github.com/s5labs/s5code/pull/2481#discussion_r2"},
                    {"id": "tro2", "author": {"login": "sparsh", "name": "Sparsh", "avatarUrl": null}, "body": long, "createdAt": "2026-11-06T08:15:00.000Z", "url": null},
                ]}));
                threads.push(json!({"id": "t-closed", "path": "apps/desktop-rune/src/pages/usage.rs", "line": 52, "side": "RIGHT", "isResolved": true, "isOutdated": false, "comments": [
                    {"id": "trc1", "author": julius, "body": "Prefer [`thousands`](https://github.com/s5labs/s5code/blob/main/apps/desktop-rune/src/chat/details.rs) here.", "createdAt": "2026-11-06T08:25:00.000Z", "url": "https://github.com/s5labs/s5code/pull/2481#discussion_r3"},
                ]}));
            }
            u.open_comments.insert("t-open".into());
            u.comments_oldest_first = true;
        });
        s.client_settings.values.insert("pullRequestCommentFilter".into(), json!({"bots": false, "resolved": true}));
    }),
    ("pull-requests-detail-commit-scope", |s| {
        open_detail(s, 1, |_| {});
        pr_detail::open_commit(s, "5f3c2a9d81e04b7a6c1d2e3f4a5b6c7d8e9f0a1b".into());
        pr_detail::poll(s);
        pr_code::unfold(s, "apps/desktop-rune/src/pages/usage.rs");
    }),
    ("pull-requests-detail-menu", |s| open_detail(s, 0, |u| u.menu = true)),
    ("pull-requests-detail-label-picker", |s| {
        open_detail(s, 0, |_| {});
        pr_meta::scene(s, "labels");
    }),
    ("pull-requests-detail-reviewer-picker", |s| {
        open_detail(s, 0, |_| {});
        pr_meta::scene(s, "reviewers");
    }),
    ("pull-requests-detail-out-of-date", |s| {
        open_detail(s, 0, |_| {});
        pr_meta::scene(s, "out-of-date");
    }),
    ("pull-requests-detail-linked-threads", |s| {
        open_detail(s, 0, |_| {});
        pr_meta::scene(s, "linked");
    }),
    ("pull-requests-detail-link-thread", |s| {
        open_detail(s, 0, |_| {});
        pr_meta::scene(s, "link-thread");
    }),
    ("pull-requests-detail-stacked-on", |s| {
        open_detail(s, 0, |_| {});
        pr_meta::scene(s, "stacked");
    }),
    ("pull-requests-detail-stack-menu", |s| {
        open_detail(s, 0, |_| {});
        pr_meta::scene(s, "stack");
    }),
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
        u.error = Some("The environment request failed.".into());
    }),
    ("pull-requests-detail-not-found", |s| {
        open_detail(s, 0, |_| {});
        let u = &mut s.ui.pages.detail;
        u.detail = None;
        u.error = Some("Pull request operation detail failed: Could not resolve to a PullRequest with the number of 2481.".into());
        u.not_found = true;
    }),
    ("pull-requests-detail-error-retrying", |s| {
        open_detail(s, 0, pr_detail::scene_refreshing);
        let u = &mut s.ui.pages.detail;
        u.detail = None;
        u.error = Some("The environment request failed.".into());
    }),
    ("pull-requests-detail-refreshing", |s| open_detail(s, 0, pr_detail::scene_refreshing)),
    ("pull-requests-detail-number-menu", |s| open_detail(s, 0, |u| u.number_menu = Some((850.0, 88.0)))),
    ("pull-requests-detail-checks-awaiting-approval", |s| {
        open_detail(s, 0, |u| {
            u.checks_open = true;
            u.description_closed = true;
            if let Some(d) = &mut u.detail {
                d["checks"][1]["status"] = json!("action-required");
                d["checks"][1]["url"] = json!("https://github.com/s5labs/s5code/actions/runs/123");
            }
        })
    }),
    ("pull-requests-detail-menu-merged", |s| {
        open_detail(s, 0, |u| {
            u.menu = true;
            if let Some(d) = &mut u.detail {
                d["state"] = json!("merged");
                for list in ["capabilities", "viewerPermissions"] {
                    if let Some(actions) = d[list]["actions"].as_array_mut() {
                        actions.push(json!("revert"));
                    }
                }
            }
        });
    }),
    ("pull-requests-detail-auto-merge-armed", |s| {
        open_detail(s, 0, |u| {
            if let Some(d) = &mut u.detail {
                d["autoMergeEnabled"] = json!(true);
                d["autoMergeMethod"] = json!("squash");
            }
        });
    }),
    ("pull-requests-detail-narrow-header", |s| {
        open_detail(s, 0, |u| {
            if let Some(d) = &mut u.detail {
                d["autoMergeEnabled"] = json!(true);
                d["autoMergeMethod"] = json!("squash");
            }
        });
        pr_tabs::scene(s, 1, Some(360.0));
    }),
    ("pull-requests-detail-checkout-menu", |s| open_detail(s, 0, |u| u.checkout_menu = true)),
    ("pull-requests-detail-checkout-menu-environments", scene_checkout_environments),
    ("pull-requests-detail-condensed", |s| open_detail(s, 0, |u| u.condense.set(0, true))),
    ("pull-requests-detail-code-condensed", |s| open_detail(s, 2, |u| u.condense.set(2, true))),
    ("pull-requests-detail-closed", |s| open_detail(s, 0, |u| if let Some(d) = &mut u.detail { d["state"] = json!("closed"); d["autoMergeEnabled"] = json!(false); })),
    ("pull-requests-detail-draft", |s| open_detail(s, 0, |u| if let Some(d) = &mut u.detail { d["isDraft"] = json!(true); d["autoMergeEnabled"] = json!(false); })),
    ("pull-requests-detail-media", |s| media_scene(s, false)),
    ("pull-requests-detail-media-lightbox", |s| media_scene(s, true)),
    ("pull-requests-detail-media-edit-preview", |s| {
        media_scene(s, false);
        let u = &mut s.ui.pages.detail;
        pr_edit::scene_caps(u);
        let body = u.detail.as_ref().map(|d| crate::data::s(d, "body").to_string()).unwrap_or_default();
        let mut draft = pr_edit::Draft::new(&body);
        draft.preview = true;
        u.edit.body = Some(draft);
    }),
    ("pull-requests-detail-app-access", |s| open_detail(s, 0, |u| u.app_access.2 = Some("not-covered".into()))),
    ("pull-requests-detail-resolve-preparing", |s| open_detail(s, 0, pr_detail::scene_resolving)),
    ("pull-requests-detail-checks-stale", |s| {
        open_detail(s, 0, |u| u.checks_popover = true);
        if let Some(list) = &mut s.ui.pages.pull_requests.result {
            list["entries"][0]["checksState"] = json!("passing");
        }
    }),
    ("pull-requests-detail-loading", |s| {
        open_detail(s, 0, |u| {
            u.detail = None;
            u.activity = None;
        });
    }),
    ("pull-requests-detail-loading-conflicting", |s| {
        open_detail(s, 0, |u| {
            u.detail = None;
            u.activity = None;
            if let Some(e) = &mut u.entry {
                e["state"] = json!("open");
                e["mergeability"] = json!("conflicting");
            }
        });
    }),
    ("pull-requests-detail-delete-branch-confirm", |s| {
        open_detail(s, 0, |u| {
            u.delete_confirm = true;
            if let Some(d) = &mut u.detail {
                d["state"] = json!("closed");
            }
        });
    }),
    ("pull-requests-detail-approve-workflows", |s| {
        open_detail(s, 0, |u| {
            if let Some(d) = &mut u.detail {
                d["workflowApprovalsRequired"] = json!(2);
                for list in ["capabilities", "viewerPermissions"] {
                    if let Some(actions) = d[list]["actions"].as_array_mut() {
                        actions.push(json!("approve-workflows"));
                    }
                }
            }
        });
    }),
    ("pull-requests-detail-diff-error-details", |s| {
        open_detail(s, 2, |u| {
            u.files = None;
            u.diff_error = Some("Pull request operation diff failed: gh pr diff 2481 exited with code 1: HTTP 502: Bad Gateway (https://api.github.com/repos/s5labs/s5code/pulls/2481)".into());
            u.diff_details = true;
        })
    }),
    ("pull-requests-detail-summary-comment-failed", |s| {
        open_detail(s, 0, |u| {
            u.description_closed = true;
            u.activity = None;
            u.activity_error = Some("The environment request failed.".into());
        })
    }),
    ("pull-requests-detail-light", |s| {
        open_detail(s, 0, |_| {});
        light(s);
    }),
    ("pull-requests-detail-edit-title", |s| {
        open_detail(s, 0, |u| {
            pr_edit::scene_caps(u);
            u.edit.title = Some(pr_edit::Draft::new("feat(desktop-rune): port the pull requests, usage and limits pages").edit);
        })
    }),
    ("pull-requests-detail-edit-description", |s| {
        open_detail(s, 0, |u| {
            pr_edit::scene_caps(u);
            let body = u.detail.as_ref().map(|d| crate::data::s(d, "body").to_string()).unwrap_or_default();
            u.edit.body = Some(pr_edit::Draft::new(&format!("{body}\n\n- [ ] Check the limits beside each provider.")));
        })
    }),
    ("pull-requests-detail-edit-comment", |s| {
        open_detail(s, 0, |u| {
            pr_edit::scene_caps(u);
            u.description_closed = true;
            let draft = pr_edit::Draft::new("Fixed in the latest push: hours now follow the local zone, and the labels say which.");
            u.edit.comment = Some(pr_edit::CommentDraft { id: "c2".into(), kind: "issue-comment".into(), draft });
        })
    }),
    ("pull-requests-detail-comment-form", |s| {
        open_detail(s, 0, |u| {
            pr_edit::scene_caps(u);
            u.description_closed = true;
            u.comments_closed = true;
            u.edit.composer.comment = rune::TextEdit::new("Superseded by #2490, which carries these pages forward.");
            u.edit.composer.open = true;
        })
    }),
    ("pull-requests-detail-review-composer", |s| {
        open_detail(s, 0, |u| {
            pr_edit::scene_caps(u);
            u.description_closed = true;
            u.comments_closed = true;
            let c = &mut u.edit.composer;
            c.open = true;
            c.review = true;
            c.verdict = Some("request-changes");
            c.verdict_menu = true;
            c.summary = rune::TextEdit::new("The hourly labels still read in UTC on the limits card.");
        })
    }),
    ("pull-requests-detail-reactions", |s| {
        open_detail(s, 0, |u| {
            pr_edit::scene_caps(u);
            u.description_closed = true;
            let Some(list) = u.activity.as_mut().and_then(|a| a["comments"].as_array_mut()) else { return };
            for c in list.iter_mut() {
                c["reactions"] = match c["id"].as_str() {
                    Some("c1") => json!([{"content": "thumbs-up", "count": 3, "actors": ["theo", "juliusmarminge"], "viewerHasReacted": true}, {"content": "rocket", "count": 1, "actors": ["theo"], "viewerHasReacted": false}]),
                    Some("c2") => json!([{"content": "heart", "count": 1, "actors": ["juliusmarminge"], "viewerHasReacted": false}]),
                    _ => continue,
                };
            }
            u.edit.picker = Some("c2".into());
        })
    }),
    ("pull-requests-maximized", |s| {
        open_detail(s, 0, |_| {});
        s.ui.pages.pull_requests.maximized = true;
    }),
    ("pull-requests-tabs", |s| {
        open_detail(s, 0, |_| {});
        pr_tabs::scene(s, 3, None);
    }),
    ("pull-requests-narrow", |s| {
        open_detail(s, 0, |_| {});
        pr_tabs::scene(s, 3, Some(4000.0));
    }),
    ("pull-requests-tabs-overflow", |s| {
        open_detail(s, 0, |_| {});
        pr_tabs::scene(s, 8, Some(360.0));
    }),
    ("pull-requests-panel-resized", |s| {
        open_detail(s, 0, |_| {});
        pr_tabs::scene(s, 1, Some(420.0));
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
    ("pull-requests-environments", |s| {
        open_prs(s, |_| {});
        pull_requests::environments_scene(s);
    }),
    ("pull-requests-server-menu", |s| {
        open_prs(s, |_| {});
        pull_requests::environments_scene(s);
        let u = &mut s.ui.pages.pull_requests;
        u.menu = Some("filters");
        u.submenu = Some("server");
    }),
    ("pull-requests-server-menu-machines", |s| {
        open_prs(s, |_| {});
        pull_requests::environments_scene(s);
        if let Some(crate::data::Source::Fixture(state)) = crate::env::source_mut(s, "studio")
            && let Some(config) = &mut state.config
        {
            config["settings"]["environmentIcon"] = json!("mac-studio");
        }
        if let crate::data::Source::Fixture(state) = &mut s.src
            && let Some(config) = &mut state.config
        {
            config["environment"]["platform"]["machine"] = json!("laptop");
        }
        let u = &mut s.ui.pages.pull_requests;
        u.menu = Some("filters");
        u.submenu = Some("server");
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
    ("usage-read-only", |s| {
        open_usage(s, "cost");
        usage::read_only_scene(s);
    }),
    ("usage-read-only-menu", |s| {
        open_usage(s, "cost");
        usage::read_only_scene(s);
        s.ui.pages.usage.menu = Some("environments");
    }),
    ("usage-no-environments", |s| {
        open_usage(s, "cost");
        usage::no_environments_scene(s);
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
    ("usage-breakdown-day-loading", |s| {
        open_usage(s, "cost");
        usage::updating_scene(s);
        s.ui.pages.usage.by_period = true;
    }),
    ("usage-compact-header", |s| {
        open_usage(s, "cost");
        s.ui.pages.usage.max_w = 900.0;
        s.ui.pages.usage.menu = Some("period");
    }),
    ("usage-provider-menu-icons", |s| {
        open_usage(s, "cost");
        s.ui.pages.usage.menu = Some("providers");
    }),
    ("usage-provider-loading-menu", |s| {
        open_usage(s, "tokens");
        usage::updating_scene(s);
        s.ui.pages.usage.menu = Some("environments");
    }),
    ("usage-chart-tooltip-partial", |s| {
        open_usage(s, "cost");
        usage::tooltip_scene(s);
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
    ("usage-limits-pooled-popover-read-only", |s| {
        open_usage(s, "limits");
        usage_limits::pooled_scene(s);
        crate::settings::permissions::set_session(s, crate::env::LOCAL, json!({"authenticated": true, "permissions": ["orchestration:read"]}));
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
    ("usage-prices-checking", |s| {
        open_usage(s, "cost");
        usage_prices::checking_scene(s);
    }),
    ("usage-prices-suggestions", |s| {
        open_usage(s, "cost");
        usage_prices::open(s, Some("kimi"));
        usage_prices::suggestions_scene(s);
    }),
    ("usage-prices-apply-menu", |s| {
        open_usage(s, "cost");
        usage_prices::open(s, None);
        usage_prices::apply_menu_scene(s);
    }),
    ("usage-prices-read-only", |s| {
        open_usage(s, "cost");
        usage_prices::read_only_scene(s);
    }),
];
