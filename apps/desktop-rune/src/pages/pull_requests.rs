//! `/pull-requests` (PullRequestsPage): every project's pull requests in one list, grouped by
//! involvement and ranked by the chosen sort, with the toolbar's search, sort, filters and host.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use rune::{Color, Cursor, El, PointerPress, ScrollSpec, Tx, col, el, icon, row, spacer, text, text_input};
use serde_json::{Map, Value, json};

use crate::data::{s as field, ts};
use crate::icons as i;
use crate::kit::{self, Menu, MenuEntry, MenuItem, Placement};
use crate::theme::{self, AlphaMul, T, layout, radius, tw};
use crate::{App, Route};

/// Rows a page asks for, and the most "Load more" grows one listing to (PAGE_SIZE, MAX_PAGE_SIZE).
const PAGE: u64 = 99;
const MAX_PAGE: u64 = 500;
/// The typed query reaches the hosts this long after the last keystroke (SEARCH_DEBOUNCE_MS).
const DEBOUNCE: Duration = Duration::from_millis(250);
/// `pullRequests.listStats` takes at most this many refs per call.
const MAX_STATS_REFS: usize = 500;

#[derive(Default)]
pub struct Ui {
    /// The last `pullRequests.list` result (entries appended across "Load more").
    pub result: Option<Value>,
    pub error: Option<String>,
    pub pending: Option<(u64, bool)>,
    /// The request the current result answers; a different one refetches.
    pub requested: Option<Value>,
    pub filters: Filters,
    pub query: rune::TextEdit,
    /// The query sent to the hosts: the typed one once typing pauses, or at once on Enter.
    pub sent_query: String,
    /// The typed text last seen, and when it changed while not yet sent.
    typed: String,
    typed_at: Option<Instant>,
    /// Rows asked for; grows by a page when the hosts can't continue from cursors.
    limit: Option<u64>,
    /// `pullRequests.invalidate` in flight from Refresh.
    invalidating: Option<u64>,
    /// Measured sizes for rows the list answered without one (`listStats`), by row key.
    stats: HashMap<String, (i64, i64)>,
    stats_asked: HashSet<String>,
    stats_pending: Option<u64>,
    /// The hosts offered in the picker, kept from the unfiltered answer while a host narrows it.
    hosts: Vec<Value>,
    pub menu: Option<&'static str>,
    pub submenu: Option<&'static str>,
    pub selected: Option<String>,
    pub context: Option<(String, f32, f32)>,
}

#[derive(Clone, PartialEq)]
pub struct Filters {
    pub state: &'static str,
    pub involvement: &'static str,
    pub sort: &'static str,
    pub draft: &'static str,
    pub review: &'static str,
    pub checks: &'static str,
    pub author: Option<String>,
    pub label: Option<String>,
    pub project: Option<String>,
    pub host: Option<String>,
}

impl Default for Filters {
    fn default() -> Self {
        Filters { state: "open", involvement: "all", sort: "ready", draft: "all", review: "all", checks: "all", author: None, label: None, project: None, host: None }
    }
}

impl Filters {
    fn count(&self) -> usize {
        [self.state != "open", self.involvement != "all", self.draft != "all", self.review != "all", self.checks != "all", self.author.is_some(), self.label.is_some(), self.project.is_some(), self.host.is_some()]
            .iter()
            .filter(|b| **b)
            .count()
    }

    /// The menu's `PullRequestListFilters`, which typed qualifiers override key by key.
    fn menu(&self) -> Map<String, Value> {
        let mut out = Map::new();
        for (key, value) in [("draft", self.draft), ("review", self.review), ("checks", self.checks)] {
            if value != "all" {
                out.insert(key.into(), json!(value));
            }
        }
        if let Some(author) = &self.author {
            out.insert("author".into(), json!(author));
        }
        if let Some(label) = &self.label {
            out.insert("labels".into(), json!([[label]]));
        }
        out
    }
}

const STATES: &[(&str, &str, &rune::Icon)] =
    &[("all", "All", &i::LAYERS), ("open", "Open", &i::GIT_PULL_REQUEST_ARROW), ("closed", "Closed", &i::GIT_PULL_REQUEST_CLOSED), ("merged", "Merged", &i::GIT_MERGE)];
const INVOLVEMENTS: &[(&str, &str, &rune::Icon)] = &[("all", "All", &i::LAYERS), ("reviewing", "Reviewing", &i::EYE), ("authored", "Authored", &i::PEN_LINE)];
const SORTS: &[(&str, &str, &rune::Icon)] = &[
    ("ready", "Merge readiness", &i::LIST_CHECKS),
    ("blocked", "Blocked on me", &i::USER_LOCK),
    ("updated", "Recently updated", &i::CLOCK),
    ("newest", "Newest shown", &i::CALENDAR_ARROW_DOWN),
    ("oldest", "Oldest shown", &i::CALENDAR_ARROW_UP),
    ("largest", "Largest shown", &i::MAXIMIZE_2),
    ("smallest", "Smallest shown", &i::MINIMIZE_2),
];
const DRAFTS: &[(&str, &str)] = &[("all", "All"), ("only", "Drafts only"), ("hide", "Hide drafts")];
const REVIEWS: &[(&str, &str)] = &[("all", "All"), ("approved", "Approved"), ("changes-requested", "Changes requested"), ("review-required", "Review required"), ("none", "No reviews")];
const CHECKS: &[(&str, &str)] = &[("all", "All"), ("passing", "Passing"), ("failing", "Failing")];

fn ui(s: &mut App) -> &mut Ui {
    &mut s.ui.pages.pull_requests
}

// ---- query ------------------------------------------------------------------------------

/// Runs of characters outside `sep`, where a `"quoted"` span may hold anything; an unclosed
/// quote is dropped, as `/(?:[^\s"]|"[^"]*")+/g` skips it.
fn runs(raw: &str, sep: fn(char) -> bool) -> Vec<&str> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < raw.len() {
        let start = at;
        while let Some(c) = raw[at..].chars().next() {
            if c == '"' {
                match raw[at + 1..].find('"') {
                    Some(end) => at += end + 2,
                    None => break,
                }
            } else if sep(c) {
                break;
            } else {
                at += c.len_utf8();
            }
        }
        if at > start {
            out.push(&raw[start..at]);
        } else if let Some(c) = raw[at..].chars().next() {
            at += c.len_utf8();
        }
    }
    out
}

fn unquoted(raw: &str) -> String {
    raw.replace('"', "").trim().to_string()
}

fn capped(name: &str) -> String {
    name.chars().take(200).collect::<String>().trim().to_string()
}

fn names(raw: &str, prefix: Option<&str>) -> Vec<String> {
    runs(raw, |c| c == ',')
        .into_iter()
        .map(unquoted)
        .filter(|n| !n.is_empty())
        .map(|n| match prefix {
            Some(key) if !n.contains(':') => format!("{key}:{n}"),
            _ => n,
        })
        .take(10)
        .map(|n| capped(&n))
        .filter(|n| !n.is_empty())
        .collect()
}

/// `^(-?)([A-Za-z][A-Za-z0-9_-]*):(.*)$` as (negated, key, raw value).
fn qualifier(token: &str) -> Option<(bool, &str, &str)> {
    let (negated, rest) = match token.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, token),
    };
    let (key, value) = rest.split_once(':')?;
    let mut chars = key.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    valid.then_some((negated, key, value))
}

/// parsePullRequestQuery: GitHub-style qualifiers become `PullRequestListFilters`; the rest is
/// the text. An unknown `key:value` is a scoped label, so `area:ui` finds the label "area:ui".
pub(super) fn parse_query(raw: &str) -> (String, Map<String, Value>) {
    let mut text = Vec::new();
    let (mut labels, mut excluded): (Vec<Vec<String>>, Vec<String>) = (Vec::new(), Vec::new());
    let mut filters = Map::new();
    for token in runs(raw, char::is_whitespace) {
        let parsed = qualifier(token);
        let value = parsed.map(|q| unquoted(q.2)).unwrap_or_default();
        let taken = match parsed.filter(|_| !value.is_empty()) {
            None => false,
            Some((negated, key, raw_value)) => match key.to_ascii_lowercase().as_str() {
                "label" => {
                    let list = names(raw_value, None);
                    match (list.is_empty(), negated) {
                        (true, _) => false,
                        (false, true) => {
                            excluded.extend(list);
                            true
                        }
                        (false, false) => {
                            labels.push(list);
                            true
                        }
                    }
                }
                "author" if !negated => {
                    filters.insert("author".into(), json!(capped(&value)));
                    true
                }
                "draft" if !negated && matches!(value.to_ascii_lowercase().as_str(), "true" | "false") => {
                    filters.insert("draft".into(), json!(if value.eq_ignore_ascii_case("true") { "only" } else { "hide" }));
                    true
                }
                "review" if !negated => {
                    let decision = match value.to_ascii_lowercase().as_str() {
                        "approved" => Some("approved"),
                        "changes_requested" | "changes-requested" => Some("changes-requested"),
                        "required" | "review-required" => Some("review-required"),
                        "none" => Some("none"),
                        _ => None,
                    };
                    decision.map(|d| filters.insert("review".into(), json!(d))).is_some()
                }
                "status" | "checks" if !negated => {
                    let state = match value.to_ascii_lowercase().as_str() {
                        "success" | "passing" => Some("passing"),
                        "failure" | "failing" => Some("failing"),
                        _ => None,
                    };
                    state.map(|c| filters.insert("checks".into(), json!(c))).is_some()
                }
                "label" | "author" | "draft" | "review" | "status" | "checks" => false,
                _ if value.starts_with('/') => false,
                _ => {
                    let list = names(raw_value, Some(key));
                    match (list.is_empty(), negated) {
                        (true, _) => false,
                        (false, true) => {
                            excluded.extend(list);
                            true
                        }
                        (false, false) => {
                            labels.push(list);
                            true
                        }
                    }
                }
            },
        };
        if !taken {
            text.push(token);
        }
    }
    if !labels.is_empty() {
        labels.truncate(10);
        filters.insert("labels".into(), json!(labels));
    }
    if !excluded.is_empty() {
        excluded.truncate(10);
        filters.insert("excludedLabels".into(), json!(excluded));
    }
    (text.join(" "), filters)
}

/// The menu's filters with the typed qualifiers laid over them.
fn merged_filters(f: &Filters, typed: &Map<String, Value>) -> Map<String, Value> {
    let mut out = f.menu();
    out.extend(typed.iter().map(|(k, v)| (k.clone(), v.clone())));
    out
}

/// The request the toolbar describes, shaped as `pullRequests.list`'s input.
pub(super) fn input(u: &Ui) -> Value {
    let f = &u.filters;
    let (text, typed) = parse_query(&u.sent_query);
    let filters = merged_filters(f, &typed);
    let mut v = json!({"state": f.state, "involvement": f.involvement, "limit": u.limit.unwrap_or(PAGE)});
    if !filters.is_empty() {
        v["filters"] = Value::Object(filters);
    }
    if let Some(p) = &f.project {
        v["projectId"] = json!(p);
    }
    if let Some(h) = &f.host {
        v["host"] = json!(h);
    }
    if !text.is_empty() {
        // The list input caps its query at 200 characters.
        v["query"] = json!(text.chars().take(200).collect::<String>().trim());
    }
    v
}

pub fn supported(s: &App) -> bool {
    s.state().config.is_some()
}

/// `host:repository#number`, one row across pages and stats (pullRequestEntryKey).
fn key(e: &Value) -> String {
    format!("{}:{}#{}", field(e, "host"), field(e, "repository"), e["number"])
}

pub fn poll(s: &mut App) -> bool {
    if !matches!(s.frame_route(), Route::PullRequests) {
        return false;
    }
    let mut changed = debounce(s);
    if let Some(id) = s.ui.pages.pull_requests.invalidating
        && s.src.net().and_then(|n| n.take_reply(id)).is_some()
    {
        // Host caches are dropped (or the call failed, which web also ignores): ask again.
        let u = ui(s);
        u.invalidating = None;
        u.requested = None;
        changed = true;
    }
    if let Some(id) = s.ui.pages.pull_requests.stats_pending
        && let Some(reply) = s.src.net().and_then(|n| n.take_reply(id))
    {
        let u = ui(s);
        u.stats_pending = None;
        if let Ok(reply) = reply {
            for stat in reply["stats"].as_array().into_iter().flatten() {
                let host = u.result.as_ref().and_then(|r| r["entries"].as_array()).and_then(|es| {
                    es.iter().find(|e| field(e, "repository") == field(stat, "repository") && e["number"] == stat["number"]).map(|e| field(e, "host").to_string())
                });
                if let Some(host) = host {
                    let k = format!("{host}:{}#{}", field(stat, "repository"), stat["number"]);
                    u.stats.insert(k, (stat["additions"].as_i64().unwrap_or(0), stat["deletions"].as_i64().unwrap_or(0)));
                }
            }
        }
        changed = true;
    }
    if let Some((id, append)) = s.ui.pages.pull_requests.pending
        && let Some(reply) = s.src.net().and_then(|n| n.take_reply(id))
    {
        let u = ui(s);
        u.pending = None;
        match reply {
            Ok(result) => {
                let result = if append { appended(u.result.take(), result) } else { result };
                if u.filters.host.is_none() || u.hosts.is_empty() {
                    u.hosts = result["providers"].as_array().cloned().unwrap_or_default();
                }
                u.result = Some(result);
                u.error = None;
            }
            Err(e) => u.error = Some(e["message"].as_str().unwrap_or("Could not load pull requests.").to_string()),
        }
        changed = true;
    }
    let wanted = input(&s.ui.pages.pull_requests);
    let requested = s.ui.pages.pull_requests.requested.clone();
    if requested.as_ref() != Some(&wanted) {
        // A different question starts from the first page again.
        if requested.as_ref().is_some_and(|r| without_limit(r) != without_limit(&wanted)) && s.ui.pages.pull_requests.limit.is_some() {
            ui(s).limit = None;
            return true;
        }
        s.ui.pages.pull_requests.requested = Some(wanted.clone());
        fetch(s, wanted, false);
        changed = true;
    }
    want_stats(s);
    changed
}

fn without_limit(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("limit");
    }
    v
}

/// Sends the typed query once typing pauses; a timer thread repaints when it is due.
fn debounce(s: &mut App) -> bool {
    let waker = s.ui.panel.waker.clone();
    let u = ui(s);
    if u.query.text != u.typed {
        u.typed = u.query.text.clone();
        u.typed_at = Some(Instant::now());
        if let Some(waker) = waker {
            std::thread::spawn(move || {
                std::thread::sleep(DEBOUNCE);
                waker();
            });
        }
    }
    match u.typed_at {
        Some(at) if at.elapsed() >= DEBOUNCE => {
            u.typed_at = None;
            u.sent_query = u.typed.clone();
            true
        }
        _ => false,
    }
}

/// A continuation's rows after the held ones, skipping any already listed and ranking the new
/// ones among themselves.
fn appended(prev: Option<Value>, mut next: Value) -> Value {
    let Some(prev) = prev else { return next };
    let mut entries = prev["entries"].as_array().cloned().unwrap_or_default();
    let held: HashSet<String> = entries.iter().map(key).collect();
    let mut arrived: Vec<Value> = next["entries"].as_array().into_iter().flatten().filter(|e| !held.contains(&key(e))).cloned().collect();
    arrived.sort_by_key(|e| std::cmp::Reverse(ts(e, "updatedAt").unwrap_or(0)));
    entries.extend(arrived);
    next["entries"] = Value::Array(entries);
    next
}

/// Asks the hosts for the sizes of rows listed without one, a batch at a time.
fn want_stats(s: &mut App) {
    let u = &s.ui.pages.pull_requests;
    if u.stats_pending.is_some() || !matches!(s.src, crate::data::Source::Live(_)) {
        return;
    }
    let refs: Vec<(String, Value)> = u
        .result
        .as_ref()
        .and_then(|r| r["entries"].as_array())
        .into_iter()
        .flatten()
        .filter(|e| size(e) == 0 && !u.stats_asked.contains(&key(e)) && e["projectId"].is_string())
        .take(MAX_STATS_REFS)
        .map(|e| (key(e), json!({"projectId": e["projectId"], "repository": e["repository"], "number": e["number"]})))
        .collect();
    if refs.is_empty() {
        return;
    }
    let payload = json!({"refs": refs.iter().map(|r| r.1.clone()).collect::<Vec<_>>()});
    if let Some(net) = s.src.net() {
        let id = net.request("pullRequests.listStats", payload);
        let u = ui(s);
        u.stats_pending = Some(id);
        u.stats_asked.extend(refs.into_iter().map(|r| r.0));
    }
}

fn fetch(s: &mut App, payload: Value, append: bool) {
    match s.src.net() {
        Some(net) => {
            let id = net.request("pullRequests.list", payload);
            ui(s).pending = Some((id, append));
        }
        None if !crate::host::live() => {
            if ui(s).result.is_none() {
                let result = super::fixture("pull-requests.json");
                let u = ui(s);
                u.hosts = result.as_ref().and_then(|r| r["providers"].as_array()).cloned().unwrap_or_default();
                u.result = result;
            }
        }
        None => {}
    }
}

/// Refresh drops the hosts' cached answers, then lists again (`pullRequests.invalidate`).
fn refresh(s: &mut App) {
    if refreshing(&s.ui.pages.pull_requests) {
        return;
    }
    match s.src.net() {
        Some(net) => {
            let id = net.request("pullRequests.invalidate", json!({}));
            let u = ui(s);
            u.invalidating = Some(id);
            u.stats.clear();
            u.stats_asked.clear();
        }
        None => ui(s).requested = None,
    }
}

fn refreshing(u: &Ui) -> bool {
    u.invalidating.is_some() || u.pending.is_some_and(|p| !p.1)
}

/// Whether "Load more" has anything to ask: cursors to continue from, or room to grow the page.
fn can_load_more(u: &Ui, result: &Value) -> bool {
    let cursors = result["nextCursors"].as_object().is_some_and(|c| !c.is_empty());
    result["truncated"].as_bool().unwrap_or(false) && (cursors || u.limit.unwrap_or(PAGE) < MAX_PAGE)
}

fn load_more(s: &mut App) {
    let u = &s.ui.pages.pull_requests;
    if u.pending.is_some() {
        return;
    }
    let Some(result) = u.result.as_ref() else { return };
    let cursors = result["nextCursors"].clone();
    if cursors.as_object().is_some_and(|c| !c.is_empty()) {
        let mut payload = input(u);
        payload["cursors"] = cursors;
        fetch(s, payload, true);
    } else {
        // Without cursors the hosts answer a longer first page instead.
        let u = ui(s);
        u.limit = Some((u.limit.unwrap_or(PAGE) + PAGE).min(MAX_PAGE));
    }
}

pub fn on_key(s: &mut App, e: &rune::KeyEvent, _: &mut Tx) -> bool {
    if !matches!(s.route, Route::PullRequests) || !matches!(e.key, rune::Key::Escape) {
        return false;
    }
    let u = ui(s);
    if u.context.take().is_some() || u.menu.take().is_some() {
        u.submenu = None;
        return true;
    }
    if u.selected.take().is_some() {
        return true;
    }
    false
}

// ---- ranking ----------------------------------------------------------------------------

fn viewer<'a>(result: &'a Value, e: &Value) -> &'a str {
    result["viewers"][field(e, "host")].as_str().map(str::trim).unwrap_or("")
}

fn authored(result: &Value, e: &Value) -> bool {
    let me = viewer(result, e);
    !me.is_empty() && e["author"]["login"].as_str().is_some_and(|l| l.trim().eq_ignore_ascii_case(me))
}

fn open(e: &Value) -> bool {
    field(e, "state") == "open"
}

fn conflicting(e: &Value) -> bool {
    field(e, "mergeability") == "conflicting"
}

fn size(e: &Value) -> i64 {
    e["additions"].as_i64().unwrap_or(0) + e["deletions"].as_i64().unwrap_or(0)
}

/// resolvePullRequestAuthorFilter: `@me` and `me` name the signed-in account where it is known.
fn author_filter<'a>(author: &'a str, viewer: &'a str) -> &'a str {
    let trimmed = author.trim();
    let me = trimmed.strip_prefix('@').unwrap_or(trimmed).eq_ignore_ascii_case("me");
    if me && !viewer.trim().is_empty() { viewer } else { trimmed }
}

/// narrowPullRequestsToFilters and matchesPullRequestFilters: state, project and host, then the
/// draft, review, label and author filters. Checks are the host's call, never narrowed here.
fn matches_filters(f: &Filters, filters: &Map<String, Value>, result: &Value, e: &Value) -> bool {
    let narrowed = (f.state == "all" || field(e, "state") == f.state)
        && f.project.as_deref().is_none_or(|p| field(e, "projectId") == p)
        && f.host.as_deref().is_none_or(|h| field(e, "host") == h);
    let involvement_ok = match f.involvement {
        "reviewing" => e["viewerReviewRequested"].as_bool().unwrap_or(false),
        "authored" => authored(result, e),
        _ => true,
    };
    let labels: HashSet<String> = e["labels"].as_array().into_iter().flatten().map(|l| field(l, "name").trim().to_lowercase()).collect();
    let holds = |l: &Value| l.as_str().is_some_and(|l| labels.contains(&l.trim().to_lowercase()));
    let draft_ok = filters.get("draft").and_then(Value::as_str).is_none_or(|d| e["isDraft"].as_bool().unwrap_or(false) == (d == "only"));
    let review_ok = match filters.get("review").and_then(Value::as_str) {
        None => true,
        Some("none") => e["reviewDecision"].as_str().is_none(),
        Some(r) => field(e, "reviewDecision") == r,
    };
    let labels_ok = filters.get("labels").and_then(Value::as_array).is_none_or(|groups| groups.iter().all(|g| g.as_array().is_some_and(|g| g.iter().any(holds))));
    let excluded_ok = filters.get("excludedLabels").and_then(Value::as_array).is_none_or(|ls| !ls.iter().any(holds));
    let author_ok = filters.get("author").and_then(Value::as_str).is_none_or(|a| {
        e["author"]["login"].as_str().is_some_and(|login| login.eq_ignore_ascii_case(author_filter(a, viewer(result, e))))
    });
    narrowed && involvement_ok && draft_ok && review_ok && labels_ok && excluded_ok && author_ok
}

/// matchesPullRequestQuery: the whole phrase, anywhere in `#N title repository branch login`.
fn matches_query(e: &Value, query: &str) -> bool {
    let needle = query.trim().to_lowercase();
    needle.is_empty() || format!("#{} {} {} {} {}", e["number"], field(e, "title"), field(e, "repository"), field(e, "headBranch"), e["author"]["login"].as_str().unwrap_or("")).to_lowercase().contains(&needle)
}

/// scorePullRequestMatch: how well a row answers the host search, for its initial order.
fn score(e: &Value, query: &str) -> u8 {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return 0;
    }
    let number = needle.strip_prefix('#').unwrap_or(&needle);
    if !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) {
        return if e["number"].to_string() == number { 100 } else { 0 };
    }
    let title = field(e, "title").to_lowercase();
    let terms: Vec<&str> = needle.split_whitespace().collect();
    if title == needle {
        90
    } else if title.contains(&needle) {
        80
    } else if terms.len() > 1 && terms.iter().all(|t| title.contains(t)) {
        70
    } else if field(e, "headBranch").to_lowercase().contains(&needle) {
        60
    } else if e["author"]["login"].as_str().unwrap_or("").to_lowercase().contains(&needle) {
        50
    } else if field(e, "repository").to_lowercase().contains(&needle) {
        40
    } else if terms.iter().any(|t| title.contains(t)) {
        30
    } else {
        10
    }
}

fn ready_tier(e: &Value) -> u8 {
    let passing = field(e, "checksState") == "passing";
    if conflicting(e) {
        4
    } else if !open(e) {
        3
    } else if e["isDraft"].as_bool().unwrap_or(false) {
        2
    } else if passing && field(e, "reviewDecision") == "approved" {
        0
    } else if passing {
        1
    } else {
        2
    }
}

fn blocked_tier(e: &Value, group: Group) -> u8 {
    match group {
        Group::Authored => {
            if !open(e) {
                6
            } else if conflicting(e) {
                0
            } else if field(e, "reviewDecision") == "changes-requested" {
                1
            } else if field(e, "checksState") == "failing" {
                2
            } else if e["isDraft"].as_bool().unwrap_or(false) {
                3
            } else if field(e, "checksState") == "passing" && field(e, "reviewDecision") == "approved" {
                5
            } else {
                4
            }
        }
        Group::Review => u8::from(!open(e)),
        Group::Others | Group::All => 0,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Group {
    Authored,
    Review,
    Others,
    All,
}

/// sortPullRequestGroups over rows already in their base order (recency, or search rank).
/// Readiness and blocking rank only an unsearched list; a search keeps the host's order.
fn rank(entries: &mut [Value], sort: &str, group: Group, searching: bool) {
    let created = |e: &Value| ts(e, "createdAt");
    match sort {
        "ready" if !searching => entries.sort_by_key(|e| (ready_tier(e), size(e) == 0, size(e))),
        "blocked" if !searching => entries.sort_by_key(|e| blocked_tier(e, group)),
        "newest" => entries.sort_by_key(|e| (created(e).is_none(), std::cmp::Reverse(created(e).unwrap_or(0)))),
        "oldest" => entries.sort_by_key(|e| (created(e).is_none(), created(e).unwrap_or(0))),
        "largest" => entries.sort_by_key(|e| (size(e) == 0, std::cmp::Reverse(size(e)))),
        "smallest" => entries.sort_by_key(|e| (size(e) == 0, size(e))),
        _ => {}
    }
}

/// The rows as shown: measured sizes filled in, narrowed by the menu and the typed qualifiers
/// and text, in their base order, then grouped by involvement and ranked by the sort.
fn groups(u: &Ui, result: &Value) -> Vec<(Group, Vec<Value>)> {
    let (typed_text, typed) = parse_query(&u.query.text);
    let (sent_text, _) = parse_query(&u.sent_query);
    let filters = merged_filters(&u.filters, &typed);
    let settled = u.query.text == u.sent_query && u.pending.is_none();
    // Hosts that search text themselves match fields rows don't carry; trust their answer.
    let searching_hosts: HashSet<&str> = result["providers"].as_array().into_iter().flatten().filter(|p| p["searchesOnHost"] == true).map(|p| field(p, "host")).collect();
    let mut rows: Vec<Value> = result["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| matches_filters(&u.filters, &filters, result, e))
        .filter(|e| typed_text.is_empty() || (settled && searching_hosts.contains(field(e, "host"))) || matches_query(e, &typed_text))
        .map(|e| {
            let mut e = e.clone();
            if size(&e) == 0
                && let Some(&(additions, deletions)) = u.stats.get(&key(&e))
            {
                e["additions"] = json!(additions);
                e["deletions"] = json!(deletions);
            }
            e
        })
        .collect();
    let searching = !typed_text.is_empty();
    rows.sort_by_key(|e| std::cmp::Reverse(ts(e, "updatedAt").unwrap_or(0)));
    if searching {
        rows.sort_by_key(|e| std::cmp::Reverse(score(e, &sent_text)));
    }
    if u.filters.involvement != "all" {
        rank(&mut rows, u.filters.sort, Group::All, searching);
        return if rows.is_empty() { vec![] } else { vec![(Group::All, rows)] };
    }
    let mut buckets: [(Group, Vec<Value>); 3] = [(Group::Authored, vec![]), (Group::Review, vec![]), (Group::Others, vec![])];
    for e in rows {
        let slot = if authored(result, &e) {
            0
        } else if e["viewerReviewRequested"].as_bool().unwrap_or(false) {
            1
        } else {
            2
        };
        buckets[slot].1.push(e);
    }
    buckets
        .into_iter()
        .filter(|b| !b.1.is_empty())
        .map(|(group, mut members)| {
            rank(&mut members, u.filters.sort, group, searching);
            (group, members)
        })
        .collect()
}

// ---- view -------------------------------------------------------------------------------

pub fn view(s: &App, cx: &mut rune::Cx<App>, t: &T, overlays: &mut Vec<El>) -> El {
    let _ = cx;
    let u = &s.ui.pages.pull_requests;
    let header = row()
        .key("pr-header")
        .w_full()
        .h(layout::TOPBAR_H)
        .shrink()
        .px(20.0)
        .center_y()
        .window_drag()
        .child(text("Pull requests").font(theme::SM_MEDIUM).color(t.c.foreground));
    let mut content = col().w_full().max_w(1152.0).px(24.0).pt(24.0).pb(48.0).gap(16.0).child(toolbar(s, t, overlays));
    match (&u.result, &u.error) {
        (None, None) => content = content.child(ghost(t, 7, Some("Loading pull requests…".into()))),
        (None, Some(error)) => content = content.child(unavailable(t, error, u.pending.is_some())),
        (Some(result), error) => {
            let groups = groups(u, result);
            match error {
                Some(error) if groups.is_empty() => content = content.child(unavailable(t, error, u.pending.is_some())),
                _ if groups.is_empty() => content = content.child(empty(s, t, result)),
                _ => {
                    for (group, entries) in &groups {
                        content = content.child(group_view(s, t, result, *group, entries, overlays));
                    }
                }
            }
            if let Some(error) = error.as_ref().filter(|_| !groups.is_empty()) {
                content = content.child(error_banner(t, error));
            }
            if result["truncated"].as_bool().unwrap_or(false) && !groups.is_empty() {
                content = content.child(load_more_footer(u, t, result));
            }
        }
    }
    let body = kit::scroll_area(
        "pr-scroll",
        col().key("pr-scroll").grow(1.0).min_h(0.0).w_full().items_center().scroll(ScrollSpec { stick_bottom: false }).child(content),
        t,
    );
    if let Some((url, x, y)) = &u.context
        && let Some(e) = u.result.as_ref().and_then(|r| r["entries"].as_array()?.iter().find(|e| field(e, "url") == url))
    {
        context_menu(t, e, &u.hosts, *x, *y, overlays);
    }
    let list = col().key("pr-list").grow(1.0).shrink().min_w(0.0).h_full().fill(t.c.background).child(header).child(body);
    let mut main = row().key("main").grow(1.0).shrink().min_w(0.0).h_full().items_stretch().fill(t.c.background).child(list);
    if u.selected.is_some() {
        main = main.child(el().w(1.0).h_full().shrink().fill(t.c.border)).child(super::pr_detail::view(s, t, overlays));
    }
    main
}

/// `Button` with the outline look the toolbar and states use throughout.
struct Button(&'static str, Option<&'static rune::Icon>);
impl Button {
    fn sm(self) -> kit::Button {
        self.with("sm")
    }
    fn with(self, size: &str) -> kit::Button {
        let b = kit::Button::new(self.0).variant("outline").size(size);
        match self.1 {
            Some(g) => b.icon(g),
            None => b,
        }
    }
}

fn toolbar(s: &App, t: &T, overlays: &mut Vec<El>) -> El {
    let u = &s.ui.pages.pull_requests;
    let k = kit::theme(t);
    let search = row()
        .key("pr-search")
        .grow(1.0)
        .min_w(240.0)
        .h(34.0)
        .radius(8.0)
        .border(1.0, t.c.contrast_input)
        .fill(if t.dark { t.c.input.alpha_mul(0.32) } else { t.c.background })
        .center_y()
        .pl(11.0)
        .pr(8.0)
        .child(icon(&i::SEARCH, 16.0).color(t.c.muted_foreground.alpha_mul(0.8)))
        .child(
            text_input(&u.query, theme::TEXT_SM)
                .key("pr-search-input")
                .single_line()
                .placeholder("Search pull requests, or label:bug", t.c.placeholder)
                .grow(1.0)
                .min_w(0.0)
                .pl(8.0)
                .color(t.c.foreground)
                .on_edit(|s: &mut App| &mut s.ui.pages.pull_requests.query)
                .on_input_key(|s: &mut App, key: &rune::KeyEvent, _: &mut Tx| {
                    if !matches!(key.key, rune::Key::Enter) {
                        return false;
                    }
                    let u = ui(s);
                    u.sent_query = u.query.text.trim().to_string();
                    u.typed = u.query.text.clone();
                    u.typed_at = None;
                    true
                }),
        );
    let open = |name| u.menu == Some(name);
    let toggle = |name: &'static str| {
        move |s: &mut App| {
            let u = ui(s);
            u.menu = if u.menu == Some(name) { None } else { Some(name) };
            u.submenu = None;
        }
    };
    let close = |s: &mut App| {
        let u = ui(s);
        u.menu = None;
        u.submenu = None;
    };

    let mut sort = Menu::new("pr-sort").trigger(Button("Sort", Some(&i::ARROW_DOWN_UP)).with("default").on_press(toggle("sort")).build(t)).placement(Placement::BottomStart).min_width(208.0).open(open("sort")).on_close(close);
    for &(id, label, glyph) in SORTS {
        sort = sort.item(MenuItem::new(label).icon(glyph).radio(u.filters.sort == id).on_select(move |s: &mut App| {
            ui(s).filters.sort = id;
            ui(s).menu = None;
        }));
    }

    let count = u.filters.count();
    let mut filters_trigger = Button("Filters", Some(&i::LIST_FILTER)).with("default").on_press(toggle("filters")).build(t);
    if count > 0 {
        filters_trigger = filters_trigger.child(row().h(16.0).px(6.0).radius(radius::FULL).fill(t.c.muted).center().child(text(count.to_string()).font(theme::TEXT_XS).color(t.c.foreground)));
    }
    let result = u.result.as_ref();
    let sub = u.submenu;
    let sub_item = |label: &str, glyph: &'static rune::Icon, name: &'static str, entries: Vec<MenuEntry>| {
        MenuEntry::item(MenuItem::new(label).icon(glyph).submenu(entries, sub == Some(name)).on_submenu_change(move |s: &mut App, on| ui(s).submenu = on.then_some(name)))
    };
    macro_rules! radios {
        ($list:expr, $current:expr, $field:ident) => {
            $list
                .iter()
                .map(|&(id, label, glyph)| {
                    MenuEntry::item(MenuItem::new(label).icon(glyph).radio($current == id).on_select(move |s: &mut App| {
                        ui(s).filters.$field = id;
                        close(s);
                    }))
                })
                .collect::<Vec<_>>()
        };
    }
    macro_rules! plain {
        ($list:expr, $current:expr, $field:ident) => {
            $list
                .iter()
                .map(|&(id, label)| {
                    MenuEntry::item(MenuItem::new(label).radio($current == id).on_select(move |s: &mut App| {
                        ui(s).filters.$field = id;
                        close(s);
                    }))
                })
                .collect::<Vec<_>>()
        };
    }
    let owned = |current: &Option<String>, any: &str, values: Vec<String>, set: fn(&mut Filters, Option<String>)| {
        let mut out = vec![MenuEntry::item(MenuItem::new(any).radio(current.is_none()).on_select(move |s: &mut App| {
            set(&mut ui(s).filters, None);
            close(s);
        }))];
        for v in values {
            let checked = current.as_deref() == Some(v.as_str());
            let label = v.clone();
            out.push(MenuEntry::item(MenuItem::new(label).radio(checked).on_select(move |s: &mut App| {
                set(&mut ui(s).filters, Some(v.clone()));
                close(s);
            })));
        }
        out
    };
    let entries = result.and_then(|r| r["entries"].as_array()).map(Vec::as_slice).unwrap_or(&[]);
    let mut authors: Vec<String> = entries.iter().filter_map(|e| e["author"]["login"].as_str().map(str::to_string)).collect();
    authors.sort_by_key(|a| a.to_lowercase());
    authors.dedup();
    let mut labels: Vec<String> = entries.iter().flat_map(|e| e["labels"].as_array().into_iter().flatten()).map(|l| field(l, "name").to_string()).collect();
    labels.sort_by_key(|a| a.to_lowercase());
    labels.dedup();
    let mut projects: Vec<(String, String)> = s.state().projects.iter().map(|p| (field(p, "id").to_string(), field(p, "title").to_string())).collect();
    projects.sort_by_key(|p| p.1.to_lowercase());
    let mut project_entries = vec![MenuEntry::item(MenuItem::new("All projects").radio(u.filters.project.is_none()).on_select(move |s: &mut App| {
        ui(s).filters.project = None;
        close(s);
    }))];
    for (id, title) in projects {
        let checked = u.filters.project.as_deref() == Some(id.as_str());
        project_entries.push(MenuEntry::item(MenuItem::new(title).radio(checked).on_select(move |s: &mut App| {
            ui(s).filters.project = Some(id.clone());
            close(s);
        })));
    }
    let filters = Menu::new("pr-filters")
        .trigger(filters_trigger)
        .placement(Placement::BottomStart)
        .min_width(208.0)
        .open(open("filters"))
        .on_close(close)
        .entries(vec![
            sub_item("State", &i::GIT_PULL_REQUEST_ARROW, "state", radios!(STATES, u.filters.state, state)),
            sub_item("Involvement", &i::USERS, "involvement", radios!(INVOLVEMENTS, u.filters.involvement, involvement)),
            MenuEntry::Separator,
            sub_item("Author", &i::USER_ROUND, "author", owned(&u.filters.author, "Anyone", authors, |f, v| f.author = v)),
            sub_item("Labels", &i::LAYERS, "labels", owned(&u.filters.label, "Any label", labels, |f, v| f.label = v)),
            sub_item("Draft", &i::GIT_PULL_REQUEST_DRAFT, "draft", plain!(DRAFTS, u.filters.draft, draft)),
            sub_item("Review", &i::EYE, "review", plain!(REVIEWS, u.filters.review, review)),
            sub_item("Checks", &i::CIRCLE_CHECK, "checks", plain!(CHECKS, u.filters.checks, checks)),
            MenuEntry::Separator,
            sub_item("Project", &i::FOLDER, "project", project_entries),
        ]);

    let hosts: Vec<(String, String, Option<String>)> = u.hosts.iter().map(|p| (field(p, "host").to_string(), host_label(&u.hosts, p), unavailable_host(p))).collect();
    let host_label = u.filters.host.as_ref().map(|h| hosts.iter().find(|x| &x.0 == h).map(|x| x.1.clone()).unwrap_or_else(|| h.clone())).unwrap_or_else(|| "All".into());
    let mut host = Menu::new("pr-host")
        .trigger(kit::Button::new(host_label).variant("outline").icon(&i::PLUG_2).on_press(toggle("host")).build(t))
        .placement(Placement::BottomStart)
        .min_width(180.0)
        .open(open("host"))
        .on_close(close)
        .item(MenuItem::new("All").icon(&i::PLUG_2).radio(u.filters.host.is_none()).on_select(move |s: &mut App| {
            ui(s).filters.host = None;
            close(s);
        }));
    for (h, label, unavailable) in hosts {
        let checked = u.filters.host.as_deref() == Some(h.as_str());
        let mut item = MenuItem::new(label).radio(checked).on_select(move |s: &mut App| {
            ui(s).filters.host = Some(h.clone());
            close(s);
        });
        if unavailable.is_some() {
            item = item.shortcut("Unavailable").disabled(true);
        }
        host = host.item(item);
    }

    row()
        .key("pr-toolbar")
        .w_full()
        .gap(8.0)
        .center_y()
        .child(search)
        .child(sort.build(&k, overlays))
        .child(filters.build(&k, overlays))
        .child(host.build(&k, overlays))
        .child(
            kit::Button::new(if refreshing(u) { "Refreshing pull requests" } else { "Refresh pull requests" })
                .variant("outline")
                .size("icon")
                .icon(if refreshing(u) { &i::LOADER_CIRCLE } else { &i::REFRESH_CW })
                .disabled(refreshing(u))
                .on_press(refresh)
                .build(t),
        )
}

/// pullRequestHostLabel: the provider's name, or the host where two share a kind.
fn host_label(hosts: &[Value], p: &Value) -> String {
    let kind = field(p, "kind");
    if hosts.iter().filter(|h| field(h, "kind") == kind).count() > 1 {
        return field(p, "host").to_string();
    }
    match kind {
        "github" => "GitHub".into(),
        "gitlab" => "GitLab".into(),
        "bitbucket" => "Bitbucket".into(),
        "azure-devops" => "Azure DevOps".into(),
        "gitcafe" => "GitCafe".into(),
        "forgejo" => "Forgejo".into(),
        _ => field(p, "host").to_string(),
    }
}

fn unavailable_host(p: &Value) -> Option<String> {
    (p["configured"] == false).then(|| p["detail"].as_str().unwrap_or("This host could not be read.").to_string())
}

fn group_view(s: &App, t: &T, result: &Value, group: Group, entries: &[Value], overlays: &mut Vec<El>) -> El {
    let _ = overlays;
    let mut out = col().w_full().gap(12.0);
    if group != Group::All {
        let (glyph, label): (&'static rune::Icon, &str) = match group {
            Group::Authored => (&i::PEN_LINE, "Authored"),
            Group::Review => (&i::EYE, "Review requested"),
            _ => (&i::USERS, "Others"),
        };
        let ink = t.c.muted_foreground.alpha_mul(0.7);
        out = out.child(
            row()
                .w_full()
                .px(12.0)
                .pb(4.0)
                .gap(8.0)
                .center_y()
                .child(icon(glyph, 14.0).color(ink))
                .child(text(label).font(theme::XS_MEDIUM).color(ink))
                .child(text(entries.len().to_string()).font(theme::XS_MEDIUM.features("'tnum' 1")).color(t.c.muted_foreground.alpha_mul(0.5)))
                .child(el().grow(1.0).h(1.0).fill(t.c.border)),
        );
    }
    let mut rows = col().w_full().gap(2.0);
    for e in entries {
        rows = rows.child(pr_row(s, t, result, e));
    }
    out.child(rows)
}

fn mix(a: Color, b: Color, k: f32) -> Color {
    Color { r: a.r * k + b.r * (1.0 - k), g: a.g * k + b.g * (1.0 - k), b: a.b * k + b.b * (1.0 - k), a: a.a * k + b.a * (1.0 - k) }
}

fn state_glyph(t: &T, e: &Value) -> (&'static rune::Icon, Color) {
    let dark = t.dark;
    if open(e) && e["isDraft"].as_bool().unwrap_or(false) {
        return (&i::GIT_PULL_REQUEST_DRAFT, if dark { tw::ZINC_400.alpha_mul(0.8) } else { tw::ZINC_500 });
    }
    match field(e, "state") {
        "merged" => (&i::GIT_MERGE, if dark { tw::VIOLET_300.alpha_mul(0.9) } else { tw::VIOLET_600 }),
        "closed" => (&i::GIT_PULL_REQUEST_CLOSED, if dark { tw::RED_300.alpha_mul(0.9) } else { tw::RED_600 }),
        _ => (&i::GIT_PULL_REQUEST_ARROW, success(t)),
    }
}

fn success(t: &T) -> Color {
    if t.dark { tw::EMERALD_300.alpha_mul(0.9) } else { tw::EMERALD_600 }
}

/// `Intl.NumberFormat` in English: 1234 is "1,234".
fn grouped(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (k, c) in digits.chars().enumerate() {
        if k > 0 && (digits.len() - k) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}


pub fn relative(at: i64, now: i64) -> String {
    let secs = ((now - at) / 1000).max(0);
    match secs {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

fn pr_row(s: &App, t: &T, result: &Value, e: &Value) -> El {
    let url = field(e, "url").to_string();
    let selected = s.ui.pages.pull_requests.selected.as_deref() == Some(url.as_str());
    let (glyph, ink) = state_glyph(t, e);
    let mut mark = el().size(16.0, 16.0).child(icon(glyph, 16.0).color(ink));
    if conflicting(e) && open(e) && !e["isDraft"].as_bool().unwrap_or(false) {
        mark = mark.child(
            el().absolute(8.0, 8.0).size(12.0, 12.0).center().radius(radius::FULL).fill(t.c.background).child(icon(&i::TRIANGLE_ALERT, 12.0).color(t.c.destructive)),
        );
    }
    let mono = theme::mono(theme::TEXT_XS).features("'tnum' 1");
    let mut line1 = row()
        .w_full()
        .h(20.0)
        .gap(6.0)
        .center_y()
        .child({
            // Only the number carries the link menu (PullRequestRow's onContextMenu).
            let menu_url = url.clone();
            text(format!("#{}", e["number"])).font(mono).color(t.c.muted_foreground).shrink().on_secondary_press(move |s: &mut App, at: PointerPress, _: &mut Tx| {
                ui(s).context = Some((menu_url.clone(), at.x, at.y));
            })
        })
        .child(text(field(e, "title").to_string()).font(theme::TEXT_SM).color(t.c.foreground).truncate().shrink().min_w(0.0));
    let mut signals = row().gap(4.0).center_y().shrink();
    match field(e, "checksState") {
        "passing" => signals = signals.child(icon(&i::CIRCLE_CHECK, 14.0).color(success(t))),
        "failing" => signals = signals.child(icon(&i::CIRCLE_X, 14.0).color(t.c.destructive)),
        "pending" => signals = signals.child(icon(&i::CIRCLE_DOT, 14.0).color(if t.dark { tw::AMBER_400.alpha_mul(0.9) } else { tw::AMBER_600 })),
        _ => {}
    }
    match field(e, "reviewDecision") {
        "approved" => signals = signals.child(icon(&i::USER_CHECK, 14.0).color(success(t))),
        "changes-requested" => signals = signals.child(icon(&i::USER_ROUND_X, 14.0).color(if t.dark { tw::AMBER_400.alpha_mul(0.8) } else { tw::AMBER_600.alpha_mul(0.9) })),
        "review-required" => signals = signals.child(icon(&i::USER_ROUND, 14.0).color(t.c.muted_foreground.alpha_mul(0.6))),
        _ => {}
    }
    line1 = line1.child(signals).child(spacer());
    let (add, del) = (e["additions"].as_i64().unwrap_or(0), e["deletions"].as_i64().unwrap_or(0));
    if add > 0 || del > 0 {
        let stat = theme::mono(theme::TEXT_2XS).features("'tnum' 1");
        line1 = line1.child(
            row()
                .gap(4.0)
                .shrink()
                .child(text(format!("+{}", grouped(add))).font(stat).color(t.c.diff_addition_foreground))
                .child(text(format!("-{}", grouped(del))).font(stat).color(t.c.diff_deletion_foreground)),
        );
    }
    let login = e["author"]["login"].as_str().unwrap_or("ghost").to_string();
    let meta = theme::TEXT_2XS;
    let mut line2 = row()
        .w_full()
        .h(16.0)
        .gap(6.0)
        .center_y()
        .child(
            row()
                .gap(4.0)
                .center_y()
                .shrink()
                .max_w(160.0)
                .child(super::avatars::avatar(&s.ui.pages.avatars, &e["author"], 14.0, t))
                .child(text(login).font(meta).color(t.c.muted_foreground).truncate().shrink()),
        )
        .child(text(field(e, "repository").to_string()).font(meta).color(t.c.muted_foreground).truncate().shrink());
    let labels = e["labels"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    for (n, label) in labels.iter().take(3).enumerate() {
        let hue = rune::color::hex(&format!("#{}", field(label, "color"))).unwrap_or(t.c.muted_foreground);
        let fg = mix(hue, t.c.foreground, if t.dark { 0.45 } else { 0.3 });
        let mut chip = row()
            .h(16.0)
            .px(3.0)
            .gap(4.0)
            .radius(4.0)
            .fill(hue.alpha_mul(if t.dark { 0.12 } else { 0.08 }))
            .center_y()
            .shrink()
            .max_w(160.0)
            .child(text(field(label, "name").to_string()).font(theme::weight(theme::TEXT_3XS, theme::MEDIUM)).color(fg).truncate().shrink());
        let remaining = labels.len().saturating_sub(n + 1);
        if n == 2 && remaining > 0 {
            chip = chip.child(text(format!("+{remaining}")).font(theme::weight(theme::TEXT_3XS, theme::MEDIUM)).color(fg));
        }
        line2 = line2.child(chip);
    }
    if let Some(at) = ts(e, "updatedAt") {
        line2 = line2.child(spacer()).child(text(relative(at, s.now_ms)).font(meta.features("'tnum' 1")).color(t.c.muted_foreground));
    }
    let _ = result;
    let select = url.clone();
    row()
        .key(format!("pr-{url}"))
        .w_full()
        .radius(radius::MD)
        .fill(if selected { t.c.accent } else { Color::TRANSPARENT })
        .hover(if selected { t.c.accent } else { t.c.accent.alpha_mul(0.6) })
        .cursor(Cursor::Pointer)
        .role("button")
        .label(field(e, "title").to_string())
        .px(12.0)
        .py(10.0)
        .gap(8.0)
        .child(mark.mt(2.0).self_start())
        .child(col().grow(1.0).shrink().min_w(0.0).child(line1).child(line2))
        .on_press(move |s: &mut App| ui(s).selected = Some(select.clone()))
}

/// showPullRequestLinkContextMenu: copy the link, or open it on its host.
fn context_menu(t: &T, e: &Value, hosts: &[Value], x: f32, y: f32, overlays: &mut Vec<El>) {
    let url = field(e, "url").to_string();
    let close = |s: &mut App| ui(s).context = None;
    let provider = field(e, "provider");
    let name = match host_label(&[], &json!({"kind": provider, "host": ""})) {
        n if n.is_empty() => hosts.iter().find(|h| field(h, "host") == field(e, "host")).map(|h| host_label(&[], h)).filter(|n| n != field(e, "host")).unwrap_or_default(),
        n => n,
    };
    let open_label = if name.is_empty() { "Open on host".to_string() } else { format!("Open on {name}") };
    let open_url = url.clone();
    let copy_url = url.clone();
    Menu::new("pr-context")
        .native(true)
        .width(198.0)
        .open(true)
        .at(x, y)
        .on_close(close)
        .item(MenuItem::new("Copy link").icon(&i::COPY).on_select_tx(move |s: &mut App, tx: &mut Tx| {
            tx.copy(&copy_url);
            ui(s).context = None;
        }))
        .item(MenuItem::new(open_label).on_select_tx(move |s: &mut App, tx: &mut Tx| {
            let _ = tx.open_url(&open_url);
            ui(s).context = None;
        }))
        .overlay(&kit::theme(t), overlays);
}

/// The list's foot while the hosts hold more: a spinner while loading, the button while there
/// is a way to ask, and a nudge to narrow once the page can't grow.
fn load_more_footer(u: &Ui, t: &T, result: &Value) -> El {
    let foot = row().w_full().py(12.0).center();
    let muted = t.c.muted_foreground;
    match u.pending {
        Some((_, append)) => foot.child(
            row()
                .gap(8.0)
                .center_y()
                .child(icon(&i::LOADER_CIRCLE, 14.0).color(muted))
                .child(text(if append { "Loading more" } else { "Updating pull requests" }).font(theme::TEXT_XS).color(muted)),
        ),
        None if can_load_more(u, result) => foot.child(Button("Load more pull requests", None).sm().on_press(load_more).build(t)),
        None => foot.child(text("Narrow your search to find more pull requests.").font(theme::TEXT_XS).color(muted)),
    }
}

/// Lists again without dropping host caches (the banner's and unavailable state's Retry).
fn retry(s: &mut App) {
    ui(s).requested = None;
}

fn check_again(u: &Ui, t: &T) -> El {
    let busy = refreshing(u);
    kit::Button::new(if busy { "Checking..." } else { "Check again" })
        .variant("outline")
        .size("sm")
        .icon(if busy { &i::LOADER_CIRCLE } else { &i::REFRESH_CW })
        .disabled(busy)
        .on_press(refresh)
        .build(t)
}

fn error_banner(t: &T, error: &str) -> El {
    row()
        .w_full()
        .gap(8.0)
        .center_y()
        .radius(radius::LG)
        .border(1.0, t.c.warning.alpha_mul(0.3))
        .fill(t.c.warning_surface)
        .px(12.0)
        .py(8.0)
        .child(text(format!("{error} Showing the last pull requests loaded.")).font(theme::TEXT_XS).color(t.c.warning_foreground).wrap().grow(1.0).shrink())
        .child(Button("Retry", None).with("xs").on_press(retry).build(t))
}

const TITLE_W: [f32; 7] = [0.6, 0.4, 0.5, 0.667, 0.4, 0.6, 0.5];
const META_W: [f32; 7] = [0.4, 0.333, 0.4, 0.25, 0.333, 0.4, 0.333];

/// PullRequestGhosts: static placeholder rows while the first page loads or a search travels.
fn ghost(t: &T, count: usize, caption: Option<String>) -> El {
    let bar = t.c.muted_foreground.alpha_mul(0.15);
    let mut rows = col().w_full().gap(2.0);
    if let Some(caption) = caption {
        rows = rows.child(row().px(12.0).pb(4.0).child(text(caption).font(theme::XS_MEDIUM).color(t.c.muted_foreground)));
    }
    for n in 0..count.min(TITLE_W.len()) {
        let line = |h: f32, frac: f32| row().w_full().child(el().h(h).w_frac(frac).radius(4.0).fill(bar));
        rows = rows.child(
            row()
                .w_full()
                .px(12.0)
                .py(10.0)
                .gap(8.0)
                .child(el().size(16.0, 16.0).radius(radius::FULL).fill(bar).mt(2.0).self_start())
                .child(col().grow(1.0).shrink().min_w(0.0).gap(6.0).child(line(16.0, TITLE_W[n])).child(line(14.0, META_W[n])))
                .child(el().w(if n % 2 == 0 { 48.0 } else { 64.0 }).h(14.0).radius(4.0).fill(bar).mt(3.0).self_start()),
        );
    }
    rows
}

/// The stacked-card media the web's `Empty` shows above a title.
fn media(t: &T, glyph: &'static rune::Icon) -> El {
    let card = |rot: f32, x: f32| {
        el().absolute(x, 0.0).size(36.0, 36.0).radius(radius::MD).border(1.0, t.c.border).fill(t.c.card).opacity(0.6).scale(0.84).rotate(rot)
    };
    el().size(36.0, 36.0)
        .mb(24.0)
        .child(card(-10f32.to_radians(), -10.0))
        .child(card(10f32.to_radians(), 10.0))
        .child(el().absolute(0.0, 0.0).size(36.0, 36.0).radius(radius::MD).border(1.0, t.c.border).fill(t.c.card).center().child(icon(glyph, 18.0).color(t.c.foreground)))
}

fn state_block(t: &T, top: El, title: &str, description: &str, actions: El) -> El {
    col()
        .w_full()
        .grow(1.0)
        .padding(48.0)
        .gap(24.0)
        .center()
        .child(
            col()
                .items_center()
                .max_w(384.0)
                .child(top)
                .child(text(title.to_string()).font(theme::weight(theme::TEXT_XL, theme::SEMIBOLD)).color(t.c.foreground).align(rune::Align::Center))
                .child(text(description.to_string()).font(theme::TEXT_SM).color(t.c.muted_foreground).w_full().wrap().align(rune::Align::Center).mt(4.0)),
        )
        .child(actions)
}

fn unavailable(t: &T, error: &str, retrying: bool) -> El {
    let retry = kit::Button::new(if retrying { "Retrying..." } else { "Retry" })
        .variant("outline")
        .size("sm")
        .icon(if retrying { &i::LOADER_CIRCLE } else { &i::REFRESH_CW })
        .disabled(retrying)
        .on_press(retry)
        .build(t);
    state_block(t, media(t, &i::GIT_PULL_REQUEST_ARROW), "Could not load pull requests", error, retry)
}

/// BranchMark: a branch leaving a baseline and rejoining it, dashed where it is still open.
static BRANCH_MARK: rune::Icon = rune::Icon {
    name: "pr-branch-mark",
    view: 120.0,
    body: "<path d=\"M10 58h100\" stroke=\"currentColor\" stroke-opacity=\"0.5\" stroke-width=\"2\" stroke-linecap=\"round\" fill=\"none\"/>\
<circle cx=\"10\" cy=\"58\" r=\"5\" stroke=\"currentColor\" stroke-width=\"2\" fill=\"currentColor\" fill-opacity=\"0.25\"/>\
<circle cx=\"110\" cy=\"58\" r=\"5\" stroke=\"currentColor\" stroke-width=\"2\" fill=\"currentColor\" fill-opacity=\"0.25\"/>\
<path d=\"M30 58c0-18 8-26 24-26h4\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\"/>\
<path d=\"M90 58c0-18-8-26-24-26h-4\" stroke=\"currentColor\" stroke-opacity=\"0.83\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" stroke-dasharray=\"2 7\" fill=\"none\"/>\
<circle cx=\"60\" cy=\"32\" r=\"4\" stroke=\"currentColor\" stroke-opacity=\"0.75\" stroke-width=\"2\" fill=\"none\"/>",
};

fn empty(s: &App, t: &T, result: &Value) -> El {
    let u = &s.ui.pages.pull_requests;
    let query = u.query.text.as_str();
    let shown = if query.chars().count() > 48 { format!("{}…", query.chars().take(48).collect::<String>()) } else { query.to_string() };
    let mark = el().w(128.0).h(80.0).clip_rounded(0.0).mb(8.0).child(icon(&BRANCH_MARK, 128.0).color(t.c.muted_foreground.alpha_mul(0.6)).absolute(0.0, 1.6));
    if s.state().projects.iter().all(|p| !p["archivedAt"].is_null()) {
        let add = kit::Button::new("Add project").size("sm").icon(&i::PLUS).on_press(crate::shell::palette::open_add_project).build(t);
        return state_block(t, mark, "No projects in this workspace", "Add a project, and the pull requests from its repository appear here.", row().child(add));
    }
    // The typed search hasn't reached the hosts yet, or its answer is still on the way.
    if !query.is_empty() && (query != u.sent_query || u.pending.is_some()) {
        return ghost(t, 5, Some(format!("Searching every host for “{shown}”")));
    }
    let mut actions = row().gap(8.0);
    let (title, description) = if !query.is_empty() {
        actions = actions.child(Button("Clear search", Some(&i::SEARCH)).sm().on_press(|s: &mut App| {
            let u = ui(s);
            u.query.clear();
            u.sent_query.clear();
            u.typed.clear();
            u.typed_at = None;
        }).build(t));
        (format!("Nothing matches “{shown}”"), "The hosts were searched for it. Try fewer words, or search by number, author or branch.")
    } else {
        if can_load_more(u, result) {
            let loading = u.pending.is_some();
            actions = actions.child(kit::Button::new(if loading { "Loading..." } else { "Load more pull requests" }).variant("outline").size("sm").disabled(loading).on_press(load_more).build(t));
        }
        let f = &u.filters;
        let filtered = !f.menu().is_empty() || f.state != "open" || f.involvement != "all" || f.project.is_some() || f.host.is_some();
        if filtered {
            ("Nothing under these filters".to_string(), "Widen the state, involvement or project filter to see more.")
        } else {
            ("No pull requests".to_string(), "Pull requests from every project in this workspace appear here.")
        }
    };
    actions = actions.child(check_again(u, t));
    state_block(t, mark, &title, description, actions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(v: Value) -> Value {
        let mut e = json!({"host": "github.com", "repository": "acme/app", "number": 7, "title": "Fix login redirect", "headBranch": "fix/login", "state": "open", "isDraft": false, "labels": [], "author": {"login": "Octo"}});
        if let (Some(e), Some(v)) = (e.as_object_mut(), v.as_object()) {
            e.extend(v.clone());
        }
        e
    }

    #[test]
    fn the_query_parser_lifts_qualifiers_and_keeps_the_rest_as_text() {
        let (text, f) = parse_query(r#"login bug label:"needs review",ui -label:wip author:@me draft:false review:changes_requested status:failure area:api is:/x -author:bob"#);
        assert_eq!(text, "login bug is:/x -author:bob");
        assert_eq!(Value::Object(f), json!({
            "labels": [["needs review", "ui"], ["area:api"]],
            "excludedLabels": ["wip"],
            "author": "@me",
            "draft": "hide",
            "review": "changes-requested",
            "checks": "failing",
        }));
        assert_eq!(parse_query("draft:maybe review:soon label:").0, "draft:maybe review:soon label:");
        assert_eq!(parse_query(r#"say "hi there" ok"#).0, r#"say "hi there" ok"#);
        assert!(parse_query("").1.is_empty());
    }

    #[test]
    fn typed_qualifiers_override_the_menu_and_reach_the_request() {
        let mut u = Ui::default();
        u.filters.draft = "only";
        u.filters.checks = "passing";
        u.sent_query = "draft:false crash".into();
        let v = input(&u);
        assert_eq!(v["filters"], json!({"draft": "hide", "checks": "passing"}));
        assert_eq!(v["query"], "crash");
        assert_eq!((v["limit"].as_u64(), v["involvement"].as_str()), (Some(PAGE), Some("all")));
    }

    #[test]
    fn rows_match_filters_case_insensitively_and_leave_checks_to_the_host() {
        let result = json!({"viewers": {"github.com": "octo"}});
        let e = entry(json!({"labels": [{"name": "Bug"}, {"name": "area:UI"}], "checksState": "failing", "isDraft": true}));
        let f = Filters::default();
        let pass = |filters: Value| matches_filters(&f, filters.as_object().unwrap(), &result, &e);
        assert!(pass(json!({"labels": [["bug"], ["area:ui", "nope"]], "author": "@me", "draft": "only", "checks": "passing"})));
        assert!(!pass(json!({"excludedLabels": ["BUG"]})));
        assert!(!pass(json!({"draft": "hide"})));
        assert!(!pass(json!({"author": "someone"})));
        assert!(pass(json!({"review": "none"})));
        assert!(!matches_filters(&Filters { state: "merged", ..Filters::default() }, &Map::new(), &result, &e));
    }

    #[test]
    fn text_matches_the_whole_phrase_and_search_scores_rank_titles_first() {
        let e = entry(json!({}));
        assert!(matches_query(&e, "login redirect"));
        assert!(matches_query(&e, "#7 fix"));
        assert!(!matches_query(&e, "redirect login"));
        assert_eq!(score(&e, "#7"), 100);
        assert_eq!(score(&e, "fix login redirect"), 90);
        assert_eq!(score(&e, "redirect login"), 70);
        assert_eq!(score(&e, "fix/login"), 60);
        assert_eq!(score(&e, "octo"), 50);
        assert_eq!(score(&e, "8"), 0);
    }

    #[test]
    fn readiness_puts_any_conflict_last_and_blocking_counts_draft_conflicts() {
        let conflicted_draft = entry(json!({"isDraft": true, "mergeability": "conflicting"}));
        let closed_conflict = entry(json!({"state": "closed", "mergeability": "conflicting"}));
        assert_eq!(ready_tier(&closed_conflict), 4);
        assert_eq!(ready_tier(&conflicted_draft), 4);
        assert_eq!(blocked_tier(&conflicted_draft, Group::Authored), 0);
        assert_eq!(blocked_tier(&closed_conflict, Group::Authored), 6);
        let mut rows = vec![entry(json!({"createdAt": "2026-01-02T00:00:00Z"})), entry(json!({})), entry(json!({"createdAt": "2026-01-01T00:00:00Z"}))];
        rank(&mut rows, "oldest", Group::All, false);
        assert_eq!(rows.iter().map(|e| e["createdAt"].as_str()).collect::<Vec<_>>(), [Some("2026-01-01T00:00:00Z"), Some("2026-01-02T00:00:00Z"), None]);
    }

    #[test]
    fn a_continuation_appends_only_rows_not_already_listed() {
        let prev = json!({"entries": [entry(json!({"number": 1})), entry(json!({"number": 2}))]});
        let next = json!({"entries": [entry(json!({"number": 2})), entry(json!({"number": 3}))], "truncated": false});
        let merged = appended(Some(prev), next);
        let numbers: Vec<i64> = merged["entries"].as_array().unwrap().iter().map(|e| e["number"].as_i64().unwrap()).collect();
        assert_eq!(numbers, [1, 2, 3]);
        assert_eq!(merged["truncated"], false);
    }

    #[test]
    fn load_more_continues_from_cursors_or_grows_the_page_to_its_cap() {
        let mut u = Ui::default();
        let truncated = json!({"truncated": true, "nextCursors": {}});
        assert!(can_load_more(&u, &truncated));
        u.limit = Some(MAX_PAGE);
        assert!(!can_load_more(&u, &truncated));
        assert!(can_load_more(&u, &json!({"truncated": true, "nextCursors": {"github.com": "c"}})));
        assert!(!can_load_more(&u, &json!({"truncated": false, "nextCursors": {"github.com": "c"}})));
    }

    #[test]
    fn hosts_are_named_by_provider_unless_two_share_one() {
        let one = vec![json!({"host": "github.com", "kind": "github"}), json!({"host": "gitlab.com", "kind": "gitlab"})];
        assert_eq!(host_label(&one, &one[0]), "GitHub");
        let two = vec![json!({"host": "github.com", "kind": "github"}), json!({"host": "ghe.acme.dev", "kind": "github"})];
        assert_eq!(host_label(&two, &two[1]), "ghe.acme.dev");
        assert_eq!(unavailable_host(&json!({"configured": false, "detail": null})).as_deref(), Some("This host could not be read."));
        assert_eq!(grouped(1234567), "1,234,567");
    }
}
