//! `/pull-requests` (PullRequestsPage): every project's pull requests in one list, grouped by
//! involvement and ranked by the chosen sort, with the toolbar's search, sort, filters and host.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
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
/// Bumped by the server whenever a turn may have changed pull requests.
const REFRESHES: &str = "pullRequests.subscribeRefreshes";

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
    pub(super) hosts: Vec<Value>,
    /// The scope's first page without the search (baselineQuery), held while a search runs.
    baseline: Read,
    /// The scope's authored and review-requested lists, which the groups lead with (partitionTargets).
    partitions: [Read; 2],
    /// Every state's rows in the scope while the Filters menu is open (facetQuery).
    facets: Read,
    pub picker: super::pr_filters::Picker,
    pub menu: Option<&'static str>,
    pub submenu: Option<&'static str>,
    pub selected: Option<String>,
    /// The pull request the panel toggle hid, which it reopens.
    pub(super) hidden: Option<String>,
    /// The detail fills the page (RightPanelMaximizeControl); only while it is open.
    pub maximized: bool,
    /// `pullRequests.subscribeRefreshes` is watched, and the revision last seen from it.
    watching: bool,
    turn: Option<u64>,
    pub context: Option<(String, f32, f32)>,
    /// The rows' quick actions, stack, checks and change breakdown.
    pub row: super::pr_row::Ui,
    /// The toolbar has scrolled out of the list's view, so the header carries the scope (condensed).
    pub condensed: bool,
    scrolled: Arc<Scrolled>,
    /// The condensed header's search is open (ExpandableSearch's `open`).
    pub search_open: bool,
    /// Scenes only: the list opens scrolled to its end.
    scene_scrolled: bool,
    /// A paste landed in the search, which sends it at once (insertFromPaste).
    pasted: bool,
    live: Live,
    /// The filters and search a close sweep runs under; changing either ends it.
    swept_under: Option<(Filters, String)>,
}

/// What the list's scroll callback, which runs without the app, leaves for `poll`: whether the
/// toolbar's marker has left the view, the last offset (f32 bits), and when it last moved.
#[derive(Default)]
struct Scrolled {
    condensed: AtomicBool,
    offset: AtomicU32,
    moved_at: AtomicI64,
}

/// The 1px marker under the toolbar (PullRequestsColumn's markerRef).
const CONDENSE_MARKER: &str = "pr-condense-marker";

/// useLiveRefresh's floor between live reads, period, and the quiet spell after which nobody is
/// using the app (LIVE_REFRESH_MIN_INTERVAL_MS, LIVE_REFRESH_INTERVAL_MS, LIVE_REFRESH_IDLE_AFTER_MS).
const LIVE_FLOOR: i64 = 10_000;
const LIVE_EVERY: i64 = 5 * 60_000;
const LIVE_IDLE: i64 = 6 * 60_000;

/// useLiveRefresh's clock for one view, in epoch ms: its last read (kept while the view is away,
/// as lastRefreshedAtByView outlives a remount), the last input, when the interval next fires
/// while the view shows, and whether the window was in the background.
#[derive(Default)]
pub(super) struct Live {
    read_at: Option<i64>,
    input_at: i64,
    tick_at: Option<i64>,
    woke: bool,
    background: bool,
}

impl Live {
    /// noteInteraction: input after an idle spell resumes the view (onArrival, and the interval
    /// starts over).
    pub(super) fn input(&mut self, now: i64) {
        self.woke |= now - self.input_at >= LIVE_IDLE;
        self.input_at = now;
    }

    /// Whether the view reads again at `now`. Showing it (`on`), coming back to the window and
    /// input after an idle spell arrive (onArrival); while it shows, the interval fires every
    /// LIVE_EVERY (onInterval). Either reads only while in use and LIVE_FLOOR after the last read,
    /// and a view never read only starts its clock, as it reads for itself.
    pub(super) fn due(&mut self, on: bool, now: i64, background: bool) -> bool {
        let returned = std::mem::replace(&mut self.background, background) && !background;
        let woke = std::mem::take(&mut self.woke);
        if !on {
            self.tick_at = None;
            return false;
        }
        let shown = self.tick_at.is_none();
        if shown {
            self.input_at = now;
        }
        let ticked = match self.tick_at {
            Some(at) if !woke && now >= at => {
                self.tick_at = Some(at + ((now - at) / LIVE_EVERY + 1) * LIVE_EVERY);
                true
            }
            Some(_) if !woke => false,
            _ => {
                self.tick_at = Some(now + LIVE_EVERY);
                false
            }
        };
        let arrived = shown || returned || woke;
        let in_use = now - self.input_at < LIVE_IDLE;
        let Some(read_at) = self.read_at else {
            if arrived && in_use {
                self.read_at = Some(now);
            }
            return false;
        };
        let due = in_use && (arrived || ticked) && now - read_at >= LIVE_FLOOR;
        if due {
            self.read_at = Some(now);
        }
        due
    }
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
    /// Each one required (`labels: [[a], [b]]`), ten at most.
    pub labels: Vec<String>,
    pub project: Option<String>,
    pub host: Option<String>,
}

impl Default for Filters {
    fn default() -> Self {
        Filters { state: "open", involvement: "all", sort: "ready", draft: "all", review: "all", checks: "all", author: None, labels: Vec::new(), project: None, host: None }
    }
}

impl Filters {
    /// The Filters badge: one per narrowed menu, and one per chosen label; a typed `draft:` shows
    /// in place of the menu's Draft.
    fn count(&self, query: &str) -> usize {
        let draft = typed_draft(query).unwrap_or(self.draft);
        let set = [self.state != "open", self.involvement != "all", draft != "all", self.review != "all", self.checks != "all", self.author.is_some(), self.project.is_some(), self.host.is_some()];
        set.iter().filter(|b| **b).count() + self.labels.len()
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
        if !self.labels.is_empty() {
            out.insert("labels".into(), json!(self.labels.iter().map(|l| [l]).collect::<Vec<_>>()));
        }
        out
    }
}

/// One `pullRequests.list` read kept beside the main list: the input last asked for, the request
/// in flight, and the latest answer with the input it answers.
#[derive(Default)]
struct Read {
    asked: Option<Value>,
    pending: Option<u64>,
    answer: Option<(Value, Value)>,
}

impl Read {
    fn answer_for(&self, input: &Value) -> Option<&Value> {
        self.answer.as_ref().filter(|a| &a.0 == input).map(|a| &a.1)
    }
}

/// Folds `read`'s reply, then asks for `input` unless it was the last thing asked; `None` stops
/// asking. A failed read keeps its last answer, as a query keeps its data.
fn sync(read: &mut Read, net: Option<&mut s5_net::Net>, input: Option<Value>) -> bool {
    let Some(net) = net else { return false };
    let mut changed = false;
    if let Some(id) = read.pending
        && let Some(reply) = net.take_reply(id)
    {
        read.pending = None;
        if let (Ok(result), Some(asked)) = (reply, read.asked.clone()) {
            read.answer = Some((asked, result));
        }
        changed = true;
    }
    if let Some(input) = input
        && read.asked.as_ref() != Some(&input)
    {
        read.pending = Some(net.request("pullRequests.list", input.clone()));
        read.asked = Some(input);
        changed = true;
    }
    changed
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
const DRAFTS: &[(&str, &str, &rune::Icon)] = &[("all", "All", &i::LAYERS), ("only", "Drafts only", &i::GIT_PULL_REQUEST_DRAFT), ("hide", "Hide drafts", &i::EYE_OFF)];
const REVIEWS: &[(&str, &str, &rune::Icon)] = &[
    ("all", "All", &i::LAYERS),
    ("approved", "Approved", &i::CIRCLE_CHECK),
    ("changes-requested", "Changes requested", &i::CIRCLE_X),
    ("review-required", "Review required", &i::CIRCLE_DASHED),
    ("none", "No reviews", &i::CIRCLE_SLASH),
];
const CHECKS: &[(&str, &str, &rune::Icon)] = &[("all", "All", &i::LAYERS), ("passing", "Passing", &i::CIRCLE_CHECK), ("failing", "Failing", &i::CIRCLE_X)];

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
                "author" | "draft" | "review" | "status" | "checks" => false,
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

/// The Draft a `draft:` qualifier in `query` asks for.
fn typed_draft(query: &str) -> Option<&'static str> {
    match parse_query(query).1.get("draft")?.as_str()? {
        "only" => Some("only"),
        "hide" => Some("hide"),
        _ => None,
    }
}

/// applyPullRequestDraftFilterChange: a Draft picked over a typed `draft:` drops the qualifier,
/// which would outrank it, and keeps the rest of the query; picking the Draft already shown
/// changes nothing. The rewritten query, if any, and the menu's Draft.
fn draft_change(query: &str, menu: &'static str, next: &'static str) -> (Option<String>, &'static str) {
    let typed = typed_draft(query);
    if next == typed.unwrap_or(menu) {
        return (None, menu);
    }
    let rewritten = typed.map(|_| runs(query.trim(), char::is_whitespace).into_iter().filter(|token| typed_draft(token).is_none()).collect::<Vec<_>>().join(" "));
    (rewritten, next)
}

/// The Filters menu's Draft pick, rewriting the search as `draft_change` says.
fn pick_draft(u: &mut Ui, next: &'static str) {
    let (query, draft) = draft_change(&u.query.text, u.filters.draft, next);
    u.filters.draft = draft;
    if let Some(query) = query {
        u.query.replace_text(&query);
    }
}

/// `pullRequests.list`'s input within the toolbar's project and host.
fn scoped(f: &Filters, state: &str, involvement: &str, limit: u64, filters: Map<String, Value>) -> Value {
    let mut v = json!({"state": state, "involvement": involvement, "limit": limit});
    if !filters.is_empty() {
        v["filters"] = Value::Object(filters);
    }
    if let Some(p) = &f.project {
        v["projectId"] = json!(p);
    }
    if let Some(h) = &f.host {
        v["host"] = json!(h);
    }
    v
}

/// baselineTargets: the scope's first page with the menu's filters, without the search.
fn baseline_input(f: &Filters) -> Value {
    scoped(f, f.state, f.involvement, PAGE, f.menu())
}

/// partitionTargets: the baseline asked again for one involvement.
fn partition_input(f: &Filters, involvement: &str) -> Value {
    scoped(f, f.state, involvement, PAGE, f.menu())
}

/// facetTargets: every state's rows in the scope, which the Author and Labels pickers count.
fn facet_input(f: &Filters) -> Value {
    scoped(f, "all", f.involvement, PAGE, Map::new())
}

/// The request the toolbar describes, shaped as `pullRequests.list`'s input.
pub(super) fn input(u: &Ui) -> Value {
    let f = &u.filters;
    let (text, typed) = parse_query(&u.sent_query);
    let mut v = scoped(f, f.state, f.involvement, u.limit.unwrap_or(PAGE), merged_filters(f, &typed));
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
pub(super) fn key(e: &Value) -> String {
    format!("{}:{}#{}", field(e, "host"), field(e, "repository"), e["number"])
}

pub fn poll(s: &mut App) -> bool {
    let on = matches!(s.frame_route(), Route::PullRequests);
    let mut changed = watch_refreshes(s, on);
    changed |= follow_scroll(s);
    changed |= live_refresh(s, on);
    if !on {
        return changed;
    }
    settle_panel(ui(s));
    let u = ui(s);
    if std::mem::take(&mut u.pasted) {
        submit(u);
        changed = true;
    }
    changed |= settle_search(u);
    changed |= settle_sweep(u);
    changed |= debounce(s);
    if let Some(id) = s.ui.pages.pull_requests.invalidating
        && s.src.net().and_then(|n| n.take_reply(id)).is_some()
    {
        // Host caches are dropped (or the call failed, which web also ignores): ask again.
        let u = ui(s);
        u.invalidating = None;
        reread(u);
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
                // The list asked the baseline's own question, so its answer is the baseline's too.
                let base = baseline_input(&u.filters);
                if !append && u.requested.as_ref() == Some(&base) {
                    u.baseline.answer = Some((base.clone(), result.clone()));
                    u.baseline.asked = Some(base);
                }
                u.result = Some(result);
                u.error = None;
            }
            Err(e) => u.error = Some(error_text(&e, "The environment request failed.")),
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
        let u = ui(s);
        u.requested = Some(wanted.clone());
        // Clearing a search shows the scope's held first page at once (`answered` = baselineQuery.data).
        let held = u.baseline.answer_for(&wanted).cloned();
        match held {
            Some(held) if requested.is_some() => {
                u.result = Some(held);
                u.error = None;
                u.pending = None;
            }
            _ => fetch(s, wanted, false),
        }
        changed = true;
    }
    changed |= sync_reads(s);
    changed |= super::pr_filters::poll(s);
    want_stats(s);
    changed
}

/// Every read asks again on its next poll, keeping what it shows until the answer lands.
fn reread(u: &mut Ui) {
    u.requested = None;
    for read in [&mut u.baseline, &mut u.facets].into_iter().chain(u.partitions.iter_mut()) {
        read.asked = None;
    }
}

/// Watches `pullRequests.subscribeRefreshes` while the page shows; a new revision means a turn
/// finished somewhere, so the list and the open detail read again (the queries' refreshTrigger).
fn watch_refreshes(s: &mut App, on: bool) -> bool {
    if !on {
        let u = ui(s);
        let was = std::mem::take(&mut u.watching);
        u.turn = None;
        if was && let Some(net) = s.src.net() {
            net.unwatch_resource(REFRESHES, json!({}));
        }
        return false;
    }
    let Some(net) = s.src.net() else { return false };
    net.watch_resource(REFRESHES, json!({}));
    let revision = net.resource(REFRESHES, json!({})).and_then(Value::as_u64);
    let u = ui(s);
    u.watching = true;
    if !turned(&mut u.turn, revision) {
        return false;
    }
    after_turn(u);
    super::pr_detail::refresh(s);
    true
}

/// Whether `now` is a revision other than the one last seen, which it becomes.
fn turned(seen: &mut Option<u64>, now: Option<u64>) -> bool {
    let Some(now) = now else { return false };
    seen.replace(now) != Some(now)
}

/// The rows already read per listing a continuation reads from (one per host, or per GitLab repository).
fn largest_read(entries: &[Value]) -> u64 {
    let mut counts: HashMap<String, u64> = HashMap::new();
    for e in entries {
        let repository = if field(e, "provider") == "github" { String::new() } else { field(e, "repository").to_lowercase() };
        *counts.entry(format!("{} {repository}", field(e, "host").to_lowercase())).or_default() += 1;
    }
    counts.into_values().max().unwrap_or(0)
}

/// A turn rereads what the page shows, and re-measures sizes.
fn after_turn(u: &mut Ui) {
    u.stats_asked.clear();
    reread_loaded(u);
}

/// refreshList(true): every read asks again, the first page grown to hold what "Load more" had
/// shown (PullRequestsPage's continuation collapse).
fn reread_loaded(u: &mut Ui) {
    let read = u.result.as_ref().and_then(|r| r["entries"].as_array()).map_or(0, |e| largest_read(e));
    let grown = u.limit.unwrap_or(PAGE).max(read.div_ceil(PAGE) * PAGE).min(MAX_PAGE);
    u.limit = (grown != PAGE).then_some(grown);
    reread(u);
}

/// useLiveRefresh(() => refreshList(true)): while the page shows, it reads again every five
/// minutes of use and on coming back to the window.
fn live_refresh(s: &mut App, on: bool) -> bool {
    let (on, now, background) = (on && supported(s), crate::data::now_ms(), s.ui.notify.background);
    let u = ui(s);
    let due = u.live.due(on, now, background);
    if due {
        reread_loaded(u);
    }
    due
}

/// noteInteraction: input on the page counts as use for the list and the detail.
fn note_input(s: &mut App, at: i64) {
    ui(s).live.input(at);
    s.ui.pages.detail.live.input(at);
}

/// Takes what the scroll callback saw: scrolling counts as input (the `wheel` noteInteraction
/// listens to), and the header condenses once the toolbar's marker leaves the view.
fn follow_scroll(s: &mut App) -> bool {
    let moved = ui(s).scrolled.moved_at.swap(0, Ordering::Relaxed);
    if moved > 0 {
        note_input(s, moved);
    }
    let u = ui(s);
    let condensed = u.scrolled.condensed.load(Ordering::Relaxed);
    std::mem::replace(&mut u.condensed, condensed) != condensed
}

/// ExpandableSearch's open state: a full header has none, the field focused while condensed
/// moves up into the header (one live field at a time), and an empty one closes on blur.
fn settle_search(u: &mut Ui) -> bool {
    let open = u.condensed && (u.query.focused || (u.search_open && !u.query.text.is_empty()));
    std::mem::replace(&mut u.search_open, open) != open
}

/// Scenes: the list opens scrolled to its end under the condensed header.
pub(super) fn scene_condensed(u: &mut Ui) {
    u.scene_scrolled = true;
    u.condensed = true;
    u.scrolled.condensed.store(true, Ordering::Relaxed);
}

/// IntersectionObserver over the toolbar's marker, rooted at the list: condensed once the marker
/// has scrolled above the view. The marker's offset is read unscrolled from the last layout.
fn on_scroll(scrolled: &Scrolled, waker: Option<&rune::Waker>, y: f32) {
    if scrolled.offset.swap(y.to_bits(), Ordering::Relaxed) != y.to_bits() {
        scrolled.moved_at.store(crate::data::now_ms(), Ordering::Relaxed);
    }
    let marker = rune::memo::layout(CONDENSE_MARKER).zip(rune::memo::layout("pr-scroll")).map(|(m, v)| m[1] - v[1]);
    let condensed = marker.is_some_and(|top| y > top + 1.0);
    if scrolled.condensed.swap(condensed, Ordering::Relaxed) != condensed
        && let Some(waker) = waker.cloned()
    {
        waker();
    }
}

/// What the page reads beside the list: the baseline while a search or longer page replaces it,
/// the authored and review-requested partitions once the baseline has rows, and every state's
/// rows while the Filters menu is open.
fn sync_reads(s: &mut App) -> bool {
    let u = &s.ui.pages.pull_requests;
    let f = &u.filters;
    let base = baseline_input(f);
    let baseline = (u.requested.as_ref() != Some(&base)).then(|| base.clone());
    let listed = u.baseline.answer_for(&base).is_some_and(|r| r["entries"].as_array().is_some_and(|e| !e.is_empty()));
    let partitioned = partitions_wanted(u) && listed;
    let [authored, reviewing] = ["authored", "reviewing"].map(|i| partitioned.then(|| partition_input(f, i)));
    let facets = (u.menu == Some("filters")).then(|| facet_input(f));
    let mut changed = sync(&mut s.ui.pages.pull_requests.baseline, s.src.net(), baseline);
    changed |= sync(&mut s.ui.pages.pull_requests.partitions[0], s.src.net(), authored);
    changed |= sync(&mut s.ui.pages.pull_requests.partitions[1], s.src.net(), reviewing);
    changed | sync(&mut s.ui.pages.pull_requests.facets, s.src.net(), facets)
}

/// Records the shown list as the scope's baseline, with `authored` and `reviewing` as its
/// partitions (scenes and tests).
pub(super) fn hold_partitions(u: &mut Ui, authored: Value, reviewing: Value) {
    let base = baseline_input(&u.filters);
    u.baseline.answer = u.result.clone().map(|r| (base.clone(), r));
    u.baseline.asked = Some(base);
    let inputs = [partition_input(&u.filters, "authored"), partition_input(&u.filters, "reviewing")];
    for ((read, input), answer) in u.partitions.iter_mut().zip(inputs).zip([authored, reviewing]) {
        read.asked = Some(input.clone());
        read.answer = Some((input, answer));
    }
}

/// partitionsWanted: the groups lead with the partitions only over the whole scope, unsearched.
fn partitions_wanted(u: &Ui) -> bool {
    u.filters.involvement == "all" && u.query.text.trim().is_empty()
}

fn without_limit(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("limit");
    }
    v
}

/// Sends the typed query now, as Enter does, skipping the debounce.
pub(super) fn submit(u: &mut Ui) {
    u.sent_query = u.query.text.trim().to_string();
    u.typed = u.query.text.clone();
    u.typed_at = None;
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

/// A close sweep ends when the filters or the sent search change under it.
fn settle_sweep(u: &mut Ui) -> bool {
    let first = u.swept_under.is_none();
    if u.swept_under.as_ref().is_some_and(|(f, q)| *f == u.filters && *q == u.sent_query) {
        return false;
    }
    u.swept_under = Some((u.filters.clone(), u.sent_query.clone()));
    !first && super::pr_row::cancel_sweep(&mut u.row)
}

/// Escape ends a sweep, closes a menu or the open pull request, and otherwise leaves the page
/// (registerPullRequestsEscapeCommand) unless a shell overlay is open.
fn escape(s: &mut App) -> bool {
    let u = ui(s);
    if super::pr_row::cancel_sweep(&mut u.row) {
        return true;
    }
    if u.context.take().is_some() || u.menu.take().is_some() {
        u.submenu = None;
        return true;
    }
    if u.selected.take().is_some() {
        return true;
    }
    let shell = &s.ui.shell;
    if shell.palette.open || shell.sidebar.menu.is_some() || shell.sidebar.confirm.is_some() {
        return false;
    }
    super::back(s);
    true
}

pub fn on_key(s: &mut App, e: &rune::KeyEvent, tx: &mut Tx) -> bool {
    if !matches!(s.route, Route::PullRequests) {
        return false;
    }
    note_input(s, crate::data::now_ms());
    if matches!(e.key, rune::Key::Escape) {
        return escape(s);
    }
    if s.ui.shell.palette.open {
        return false;
    }
    // PullRequestsColumn: mod+F focuses the search with its text selected.
    if e.is_command('f') && !e.mods.shift && !e.mods.alt {
        let u = ui(s);
        u.search_open |= u.condensed;
        u.picker.query.focused = false;
        u.query.focused = true;
        u.query.anchor = 0;
        u.query.focus = u.query.text.len();
        return true;
    }
    if crate::panel::shortcut(s, e, "rightPanel.close") {
        let repeat = tx.key_repeat();
        let u = ui(s);
        return if repeat { u.selected.is_some() } else { u.selected.take().is_some() };
    }
    if crate::panel::shortcut(s, e, "rightPanel.toggle") {
        let repeat = tx.key_repeat();
        let u = ui(s);
        if !panel_available(u) {
            return false;
        }
        if !repeat {
            toggle_panel(u);
        }
        return true;
    }
    if crate::panel::shortcut(s, e, "rightPanel.toggleMaximized") {
        let repeat = tx.key_repeat();
        let u = ui(s);
        if u.selected.is_none() {
            return false;
        }
        if !repeat {
            toggle_maximized(u);
        }
        return true;
    }
    if crate::panel::shortcut(s, e, "thread.copyReference")
        && let Some(url) = ui(s).selected.clone()
    {
        tx.copy(url.clone());
        crate::shell::palette::toast(s, &format!("{} link copied", short_label(&url)), Some(url.as_str()));
        return true;
    }
    false
}

/// The change request's `shortLabel` for its URL (findByChangeRequestUrl): GitLab's "MR", else "PR".
fn short_label(url: &str) -> &'static str {
    if url.contains("/-/merge_requests/") { "MR" } else { "PR" }
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
pub(super) enum Group {
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

/// The row with the size `listStats` measured, where the list answered without one.
fn sized(u: &Ui, e: &Value) -> Value {
    let mut e = e.clone();
    if size(&e) == 0
        && let Some(&(additions, deletions)) = u.stats.get(&key(&e))
    {
        e["additions"] = json!(additions);
        e["deletions"] = json!(deletions);
    }
    e
}

/// pullRequestOverrideAfterAction on every held copy of the row (the list, the baseline and the
/// partition reads), so it leaves views of its old state at once; the next reads confirm it.
pub(super) fn after_action(u: &mut Ui, row: &str, action: &str, now_ms: i64) {
    let [authored, reviewing] = &mut u.partitions;
    let reads = [&mut u.baseline, authored, reviewing].into_iter().filter_map(|r| r.answer.as_mut().map(|a| &mut a.1));
    for answer in u.result.iter_mut().chain(reads) {
        for e in answer["entries"].as_array_mut().into_iter().flatten().filter(|e| key(e) == row) {
            match action {
                "close" => e["state"] = json!("closed"),
                "reopen" => e["state"] = json!("open"),
                "ready" => e["isDraft"] = json!(false),
                _ => continue,
            }
            e["updatedAt"] = json!(super::usage::iso_of(now_ms));
        }
    }
}

/// The authored and review-requested reads the groups lead with, once both answered for this
/// scope; none while searching, or when the baseline has no rows (baselineEmpty).
fn held_partitions(u: &Ui) -> Option<[&Value; 2]> {
    let f = &u.filters;
    let baseline = u.baseline.answer_for(&baseline_input(f))?;
    let empty = |key: &str| baseline[key].as_array().is_none_or(Vec::is_empty);
    if !partitions_wanted(u) || (empty("entries") && empty("errors")) {
        return None;
    }
    Some([u.partitions[0].answer_for(&partition_input(f, "authored"))?, u.partitions[1].answer_for(&partition_input(f, "reviewing"))?])
}

/// partitionPullRequestsWithPriority: the authored and review-requested reads lead, each row in the
/// list's own copy where the list has it, so older priority rows join; the list's rest follows.
fn partitioned(rows: Vec<Value>, authored: Vec<Value>, review: Vec<Value>) -> [(Group, Vec<Value>); 3] {
    let mut lead: [Vec<Value>; 2] = [Vec::new(), Vec::new()];
    let mut at: HashMap<String, (usize, usize)> = HashMap::new();
    for (slot, read) in [authored, review].into_iter().enumerate() {
        for e in read {
            match at.get(&key(&e)) {
                Some(&(held, i)) if held == slot => lead[slot][i] = e,
                Some(_) => {}
                None => {
                    at.insert(key(&e), (slot, lead[slot].len()));
                    lead[slot].push(e);
                }
            }
        }
    }
    let mut others = Vec::new();
    for e in rows {
        match at.get(&key(&e)) {
            Some(&(slot, i)) => lead[slot][i] = e,
            None => others.push(e),
        }
    }
    let [mut mine, mut asked] = lead;
    mine.sort_by_key(|e| std::cmp::Reverse(ts(e, "updatedAt").unwrap_or(0)));
    asked.sort_by_key(|e| std::cmp::Reverse(ts(e, "updatedAt").unwrap_or(0)));
    [(Group::Authored, mine), (Group::Review, asked), (Group::Others, others)]
}

/// The rows as shown: measured sizes filled in, narrowed by the menu and the typed qualifiers
/// and text, in their base order, then grouped by involvement and ranked by the sort.
pub(super) fn groups(u: &Ui, result: &Value) -> Vec<(Group, Vec<Value>)> {
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
        .map(|e| sized(u, e))
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
    let partitions = held_partitions(u).map(|[mine, asked]| {
        let narrow = |read: &Value| -> Vec<Value> {
            read["entries"].as_array().into_iter().flatten().filter(|e| matches_filters(&u.filters, &filters, result, e)).map(|e| sized(u, e)).collect()
        };
        (narrow(mine), narrow(asked))
    });
    let buckets = match partitions {
        Some((mine, asked)) => partitioned(rows, mine, asked),
        None => {
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
        }
    };
    buckets
        .into_iter()
        .filter(|b| !b.1.is_empty())
        .map(|(group, mut members)| {
            rank(&mut members, u.filters.sort, group, searching);
            (group, members)
        })
        .collect()
}


// ---- panel ------------------------------------------------------------------------------

/// rightPanelAvailable: a pull request is open, or hidden by the toggle.
fn panel_available(u: &Ui) -> bool {
    u.selected.is_some() || u.hidden.is_some()
}

/// toggleRightPanel: hides the open pull request, or reopens the hidden one.
pub(super) fn toggle_panel(u: &mut Ui) {
    match u.selected.take() {
        Some(url) => u.hidden = Some(url),
        None => u.selected = u.hidden.take(),
    }
}

/// toggleRightPanelMaximized: only an open panel maximizes.
pub(super) fn toggle_maximized(u: &mut Ui) {
    if u.selected.is_some() {
        u.maximized = !u.maximized;
    }
}

/// An opened pull request replaces the hidden one, and a closed panel is never maximized.
fn settle_panel(u: &mut Ui) {
    if u.selected.is_some() {
        u.hidden = None;
    } else {
        u.maximized = false;
    }
}

/// A ghost toggle in the panel strip (PanelLayoutControls); a disabled one keeps its tip.
fn panel_button(t: &T, glyph: &'static rune::Icon, label: &str, tip: String, on: bool, press: Option<fn(&mut Ui)>) -> El {
    let ink = if on { t.c.foreground } else { t.c.muted_foreground };
    let b = el().size(28.0, 28.0).center().radius(layout::CONTROL_RADIUS).label(label.to_string()).help(tip).child(icon(glyph, 16.0).color(ink));
    let b = if on { b.fill(t.c.accent) } else { b };
    match press {
        Some(press) => b.hover(t.c.accent).press(t.c.accent).cursor(Cursor::Pointer).on_press(move |s: &mut App| press(ui(s))),
        None => b.disabled(true).opacity(0.64),
    }
}

/// RightPanelMaximizeControl.
pub(super) fn maximize_control(s: &App, t: &T) -> El {
    let on = s.ui.pages.pull_requests.maximized;
    let (glyph, label) = if on { (&i::MINIMIZE_2, "Restore panel size") } else { (&i::MAXIMIZE_2, "Maximize panel") };
    panel_button(t, glyph, label, label.into(), on, Some(toggle_maximized))
}

/// PanelLayoutControls' right panel toggle, unavailable until a pull request is picked.
pub(super) fn panel_control(s: &App, t: &T) -> El {
    let u = &s.ui.pages.pull_requests;
    if !panel_available(u) {
        return panel_button(t, &i::PANEL_RIGHT, "Toggle right panel", "Select a pull request first".into(), false, None);
    }
    let shortcut = crate::shell::palette::shortcut_label(s, "rightPanel.toggle");
    let tip = if shortcut.is_empty() { "Toggle right panel".to_string() } else { format!("Toggle right panel ({shortcut})") };
    panel_button(t, &i::PANEL_RIGHT, "Toggle right panel", tip, u.selected.is_some(), Some(toggle_panel))
}

// ---- view -------------------------------------------------------------------------------

pub fn view(s: &App, cx: &mut rune::Cx<App>, t: &T, overlays: &mut Vec<El>) -> El {
    let _ = cx;
    let u = &s.ui.pages.pull_requests;
    if let Some((url, x, y)) = &u.context
        && let Some(e) = u.result.as_ref().and_then(|r| r["entries"].as_array()?.iter().find(|e| field(e, "url") == url))
    {
        context_menu(t, e, &u.hosts, *x, *y, overlays);
    }
    // noteInteraction's pointermove: it only stamps the clock, so the view need not run again.
    let main = row().key("main").grow(1.0).shrink().min_w(0.0).h_full().items_stretch().fill(t.c.background).on_pointer_move_local(|s: &mut App, _, tx: &mut Tx| {
        note_input(s, crate::data::now_ms());
        tx.live_only();
    });
    if u.selected.is_some() && u.maximized {
        return main.child(super::pr_detail::view(s, t, overlays));
    }
    let header = top_bar(s, t, overlays);
    let marker = el().key(CONDENSE_MARKER).w_full().h(1.0);
    let mut content = col().w_full().max_w(1152.0).px(24.0).pt(24.0).pb(48.0).gap(16.0).child(col().w_full().child(toolbar(s, t, overlays)).child(marker));
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
        col()
            .key("pr-scroll")
            .grow(1.0)
            .min_h(0.0)
            .w_full()
            .items_center()
            .scroll(ScrollSpec { stick_bottom: u.scene_scrolled })
            .on_scroll_offset({
                let (scrolled, waker) = (u.scrolled.clone(), s.ui.panel.waker.clone());
                move |at: rune::ScrollPos| on_scroll(&scrolled, waker.as_ref(), at.offset[1])
            })
            .child(content),
        t,
    );
    let list = col().key("pr-list").grow(1.0).shrink().min_w(0.0).h_full().fill(t.c.background).child(header).child(body);
    let main = main.child(list);
    if u.selected.is_none() {
        return main;
    }
    main.child(el().w(1.0).h_full().shrink().fill(t.c.border)).child(super::pr_detail::view(s, t, overlays))
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

/// Opens the menu `name`, or closes it if it is open.
fn toggle_menu(name: &'static str) -> impl Fn(&mut App) + 'static {
    move |s: &mut App| {
        let u = ui(s);
        u.menu = if u.menu == Some(name) { None } else { Some(name) };
        u.submenu = None;
    }
}

fn close_menu(s: &mut App) {
    let u = ui(s);
    u.menu = None;
    u.submenu = None;
}

/// The search field opened the condensed header's search (ExpandableSearch's `open`), or holds text.
fn search_expanded(u: &Ui) -> bool {
    u.search_open || !u.query.text.is_empty()
}

/// `Spinner` while `busy`, else the idle glyph.
pub(super) fn busy_icon(busy: bool, idle: &'static rune::Icon, size: f32) -> El {
    if busy { icon(&i::LOADER_CIRCLE, size).looping(rune::LoopSpec::rotation(1.)) } else { icon(idle, size) }
}

/// PullRequestSearchInput. Only one field is live at a time (the condensed header's while it
/// shows one); the other draws the same text still.
fn search_field(u: &Ui, t: &T, key: &'static str, live: bool) -> El {
    let typed = u.query.text.trim();
    let busy = !typed.is_empty() && (typed != u.sent_query.trim() || u.pending.is_some_and(|p| !p.1));
    let placeholder = "Search pull requests, or label:bug";
    let field = if live {
        let input = text_input(&u.query, theme::TEXT_SM)
            .key("pr-search-input")
            .single_line()
            .label(placeholder)
            .grow(1.0)
            .min_w(0.0)
            .pl(8.0)
            .color(t.c.foreground)
            .on_edit(|s: &mut App| &mut s.ui.pages.pull_requests.query)
            .on_input_key(|s: &mut App, key: &rune::KeyEvent, _: &mut Tx| {
                if !matches!(key.key, rune::Key::Enter) {
                    return false;
                }
                submit(ui(s));
                true
            })
            // The paste lands first; the next poll sends the field's text at once.
            .on_input_paste(|s: &mut App, _: &rune::ClipboardContent, _: &mut Tx| {
                ui(s).pasted = true;
                false
            });
        // Rune wraps a placeholder wider than its field; a truncated line behind the field stands in.
        let hint = (u.query.text.is_empty() && u.query.preedit.is_none()).then(|| {
            row().pin(Some(8.0), Some(0.0), Some(0.0), Some(0.0)).center_y().child(text(placeholder).font(theme::TEXT_SM).color(t.c.placeholder).truncate().shrink().min_w(0.0))
        });
        row().grow(1.0).min_w(0.0).h_full().center_y().children(hint).child(input)
    } else {
        let (shown, ink) = if u.query.text.is_empty() { (placeholder.to_string(), t.c.placeholder) } else { (u.query.text.clone(), t.c.foreground) };
        text(shown).font(theme::TEXT_SM).color(ink).truncate().grow(1.0).shrink().min_w(0.0).pl(8.0)
    };
    row()
        .key(key)
        .h(34.0)
        .radius(8.0)
        .border(1.0, t.c.contrast_input)
        .fill(if t.dark { t.c.input.alpha_mul(0.32) } else { t.c.background })
        .center_y()
        .pl(11.0)
        .pr(8.0)
        .child(busy_icon(busy, &i::SEARCH, 16.0).color(t.c.muted_foreground.alpha_mul(0.8)))
        .child(field)
}

/// PullRequestRefreshControl: outlined in the toolbar, a ghost icon in the condensed header.
fn refresh_button(u: &Ui, t: &T, compact: bool) -> El {
    let (variant, size) = if compact { ("ghost", "icon-sm") } else { ("outline", "icon") };
    kit::Button::new("Refresh pull requests")
        .variant(variant)
        .size(size)
        .icon(if refreshing(u) { &i::LOADER_CIRCLE } else { &i::REFRESH_CW })
        .disabled(refreshing(u))
        .on_press(refresh)
        .build(t)
}

/// hostMenuOptions after "All": each host's key, name and glyph, and why it can't be read.
fn host_options(s: &App) -> Vec<(String, String, &'static rune::Icon, Option<String>)> {
    let u = &s.ui.pages.pull_requests;
    let expected = if u.hosts.is_empty() { expected_hosts(&s.state().projects) } else { Vec::new() };
    let entries = if u.hosts.is_empty() { &expected } else { &u.hosts };
    entries.iter().map(|p| (field(p, "host").to_string(), host_label(entries, p), super::pr_row::host_icon(field(p, "kind")), unavailable_host(p))).collect()
}

/// The host options under "All". A host that can't be read is greyed out with the reason on
/// hover, tagged "· Unavailable" where `tagged` (the Filters menu, not the compact menus).
fn host_entries(s: &App, t: &T, key: &str, tagged: bool) -> Vec<MenuEntry> {
    let chosen = s.ui.pages.pull_requests.filters.host.as_deref();
    let mut out = vec![MenuEntry::item(MenuItem::new("All").icon(&i::PLUG_2).radio(chosen.is_none()).on_select(|s: &mut App| ui(s).filters.host = None))];
    for (host, label, glyph, unavailable) in host_options(s) {
        let checked = chosen == Some(host.as_str());
        out.push(match unavailable {
            Some(why) => super::pr_filters::unavailable_option(t, format!("{key}-{host}"), super::pr_filters::menu_glyph(t, glyph), label, checked, tagged, why),
            None => MenuEntry::item(MenuItem::new(label).icon(glyph).radio(checked).on_select(move |s: &mut App| ui(s).filters.host = Some(host.clone()))),
        });
    }
    out
}

/// One radio item per option; picking keeps the menu open, as Base UI's radio items do.
fn radio_entries(options: &'static [(&'static str, &'static str, &'static rune::Icon)], current: &str, set: fn(&mut Ui, &'static str)) -> Vec<MenuEntry> {
    options.iter().map(|&(id, title, glyph)| MenuEntry::item(MenuItem::new(title).icon(glyph).radio(current == id).on_select(move |s: &mut App| set(ui(s), id)))).collect()
}

/// The option `id` names, or the first option's name.
fn option_label(options: &[(&str, &'static str, &rune::Icon)], id: &str) -> String {
    options.iter().find(|o| o.0 == id).map_or(options[0].1, |o| o.1).to_string()
}

/// PullRequestCompactFilterMenu in the condensed header: the choice's name and a chevron.
fn compact_menu(t: &T, name: &'static str, label: &str, current: String, open: bool, entries: Vec<MenuEntry>, overlays: &mut Vec<El>) -> El {
    let trigger = kit::Button::new(current)
        .variant("ghost-muted")
        .size("sm")
        .on_press(toggle_menu(name))
        .build(t)
        .label(label.to_string())
        .child(icon(&i::CHEVRON_DOWN, 12.0).color(t.c.muted_foreground.alpha_mul(0.7)));
    Menu::new(format!("pr-{name}")).trigger(trigger).placement(Placement::BottomStart).min_width(180.0).open(open).on_close(close_menu).entries(entries).build(&kit::theme(t), overlays)
}

/// PullRequestsColumn's header: the title, and once the toolbar has scrolled away the scope's
/// menus in its place (State, Involvement, and Host when there is a choice), the search behind
/// its button and a compact Refresh.
fn top_bar(s: &App, t: &T, overlays: &mut Vec<El>) -> El {
    let u = &s.ui.pages.pull_requests;
    let title = || text("Pull requests").font(theme::SM_MEDIUM).color(t.c.foreground).truncate().shrink().min_w(0.0);
    let bar = row().key("pr-header").w_full().h(layout::TOPBAR_H).shrink().px(20.0).gap(8.0).center_y().window_drag();
    let panel = u.selected.is_none().then(|| panel_control(s, t));
    if !u.condensed {
        return bar.child(title()).child(spacer()).children(panel);
    }
    let mut scope = row()
        .gap(6.0)
        .center_y()
        .shrink()
        .min_w(0.0)
        .child(compact_menu(t, "scope-state", "Filter by state", option_label(STATES, u.filters.state), u.menu == Some("scope-state"), radio_entries(STATES, u.filters.state, |u, v| u.filters.state = v), overlays))
        .child(compact_menu(
            t,
            "scope-involvement",
            "Filter by involvement",
            option_label(INVOLVEMENTS, u.filters.involvement),
            u.menu == Some("scope-involvement"),
            radio_entries(INVOLVEMENTS, u.filters.involvement, |u, v| u.filters.involvement = v),
            overlays,
        ));
    let hosts = host_options(s);
    if hosts.len() > 1 {
        let current = u.filters.host.as_deref().and_then(|h| hosts.iter().find(|x| x.0 == h)).map_or_else(|| "All".to_string(), |x| x.1.clone());
        scope = scope.child(compact_menu(t, "scope-host", "Filter by host", current, u.menu == Some("scope-host"), host_entries(s, t, "pr-scope-host", false), overlays));
    }
    let expanded = search_expanded(u);
    let mut crumbs = row().gap(12.0).center_y().shrink().min_w(0.0);
    if !expanded {
        crumbs = crumbs.child(title()).child(text("/").font(theme::SM_MEDIUM).color(t.c.icon_muted));
    }
    let search = if expanded {
        search_field(u, t, "pr-header-search", true).w(224.0).min_w(96.0).h(32.0).shrink()
    } else {
        kit::Button::new("Search pull requests")
            .variant("ghost")
            .size("icon-sm")
            .icon(&i::SEARCH)
            .on_press(|s: &mut App| {
                let u = ui(s);
                u.search_open = true;
                u.picker.query.focused = false;
                u.query.focused = true;
            })
            .build(t)
    };
    bar.child(crumbs.child(scope))
        .child(spacer())
        .child(row().gap(6.0).center_y().shrink().min_w(0.0).child(search).child(refresh_button(u, t, true)))
        .children(panel)
}

fn toolbar(s: &App, t: &T, overlays: &mut Vec<El>) -> El {
    let u = &s.ui.pages.pull_requests;
    let k = kit::theme(t);
    let search = search_field(u, t, "pr-search", !(u.condensed && search_expanded(u))).grow(1.0).min_w(240.0);
    let open = |name| u.menu == Some(name);

    let mut sort = Menu::new("pr-sort").trigger(Button("Sort", Some(&i::ARROW_DOWN_UP)).with("default").on_press(toggle_menu("sort")).build(t)).placement(Placement::BottomStart).min_width(208.0).open(open("sort")).on_close(close_menu);
    for &(id, label, glyph) in SORTS {
        // Radio items keep the menu open (Base UI's `closeOnClick` is off for them).
        sort = sort.item(MenuItem::new(label).icon(glyph).radio(u.filters.sort == id).on_select(move |s: &mut App| ui(s).filters.sort = id));
    }

    let mut filters_trigger = Button("Filters", Some(&i::LIST_FILTER)).with("default").on_press(toggle_menu("filters")).build(t);
    let count = u.filters.count(&u.query.text);
    if count > 0 {
        filters_trigger = filters_trigger.child(row().h(16.0).px(6.0).radius(radius::FULL).fill(t.c.muted).center().child(text(count.to_string()).font(theme::TEXT_XS).color(t.c.muted_foreground)));
    }
    let filters = Menu::new("pr-filters").trigger(filters_trigger).placement(Placement::BottomEnd).min_width(208.0).open(open("filters")).on_close(close_menu).entries(filter_entries(s, t));

    let hosts = host_options(s);
    let chosen = u.filters.host.as_deref().map(|h| hosts.iter().find(|x| x.0 == h).map_or_else(|| (h.to_string(), super::pr_row::host_icon("")), |x| (x.1.clone(), x.2)));
    // PullRequestCompactFilterMenu: icon-only, titled with the choice, once a host is chosen.
    let host_trigger = match chosen {
        Some((name, glyph)) => {
            let tip = format!("Filter by provider: {name}");
            kit::Button::new(tip.clone()).variant("outline").size("icon").icon(glyph).on_press(toggle_menu("host")).build(t).help(tip)
        }
        None => kit::Button::new("All").variant("outline").icon(&i::PLUG_2).on_press(toggle_menu("host")).build(t).label("Filter by provider: All"),
    };
    let host = Menu::new("pr-host").trigger(host_trigger).placement(Placement::BottomStart).min_width(180.0).open(open("host")).on_close(close_menu).entries(host_entries(s, t, "pr-host", false));

    row()
        .key("pr-toolbar")
        .w_full()
        .gap(8.0)
        .center_y()
        .child(search)
        .child(sort.build(&k, overlays))
        .child(filters.build(&k, overlays))
        .child(host.build(&k, overlays))
        .children((!u.condensed).then(|| refresh_button(u, t, false)))
}

/// PullRequestFiltersMenu: State and Involvement, the row filters, Host when there is a choice, then
/// Project. Each submenu shows its current choice; picking keeps the menu open, as on web.
fn filter_entries(s: &App, t: &T) -> Vec<MenuEntry> {
    let u = &s.ui.pages.pull_requests;
    let f = &u.filters;
    let sub = u.submenu;
    let sub_item = |item: MenuItem, value: String, name: &'static str, entries: Vec<MenuEntry>| {
        // The host rows carry an "Unavailable" tag beside the name, which the default width clips.
        let width = if matches!(name, "host" | "project") { 240. } else { 160. };
        MenuEntry::item(item.shortcut(value).submenu(entries, sub == Some(name)).submenu_min_width(width).on_submenu_change(move |s: &mut App, on| {
            let u = ui(s);
            if on && u.submenu != Some(name) && matches!(name, "author" | "labels") {
                super::pr_filters::open(&mut u.picker);
            }
            u.submenu = on.then_some(name);
        }))
    };
    type Options = &'static [(&'static str, &'static str, &'static rune::Icon)];
    let radios = |label: &'static str, name: &'static str, options: Options, current: &str, set: fn(&mut Ui, &'static str)| {
        let (_, value, glyph) = options.iter().find(|o| o.0 == current).copied().unwrap_or(options[0]);
        let mut entries = vec![MenuEntry::Label(label.into())];
        for &(id, title, glyph) in options {
            entries.push(MenuEntry::item(MenuItem::new(title).icon(glyph).radio(current == id).on_select(move |s: &mut App| set(ui(s), id))));
        }
        sub_item(MenuItem::new(label).icon(glyph), value.to_string(), name, entries)
    };

    fn rows(v: Option<&Value>) -> impl Iterator<Item = &Value> {
        v.and_then(|v| v["entries"].as_array()).into_iter().flatten()
    }
    // collectPullRequestListFacets over every state's rows and the scope's first page.
    let (authors, labels) = if u.menu == Some("filters") {
        let listed = u.baseline.answer_for(&baseline_input(f)).or(u.result.as_ref());
        super::pr_filters::facets(rows(u.facets.answer_for(&facet_input(f))).chain(rows(listed)), f.state)
    } else {
        Default::default()
    };
    let label_value = if f.labels.is_empty() { "Any".to_string() } else { format!("{} selected", f.labels.len()) };
    let mut out = vec![
        radios("State", "state", STATES, f.state, |u, v| u.filters.state = v),
        radios("Involvement", "involvement", INVOLVEMENTS, f.involvement, |u, v| u.filters.involvement = v),
        MenuEntry::Separator,
        sub_item(MenuItem::new("Author").icon(&i::USER_ROUND), f.author.clone().unwrap_or_else(|| "Anyone".into()), "author", super::pr_filters::author_entries(s, t, &authors)),
        sub_item(MenuItem::new("Labels").icon(&i::TAG), label_value, "labels", super::pr_filters::label_entries(s, t, &labels)),
        radios("Draft", "draft", DRAFTS, typed_draft(&u.query.text).unwrap_or(f.draft), pick_draft),
        radios("Review", "review", REVIEWS, f.review, |u, v| u.filters.review = v),
        radios("Checks", "checks", CHECKS, f.checks, |u, v| u.filters.checks = v),
    ];

    let hosts = host_options(s);
    if hosts.len() > 1 {
        let current = hosts.iter().find(|x| Some(x.0.as_str()) == f.host.as_deref());
        let (value, glyph) = current.map_or(("All".to_string(), &i::PLUG_2), |x| (x.1.clone(), x.2));
        let entries: Vec<MenuEntry> = std::iter::once(MenuEntry::Label("Host".into())).chain(host_entries(s, t, "pr-filter-host", true)).collect();
        out.extend([MenuEntry::Separator, sub_item(MenuItem::new("Host").icon(glyph), value, "host", entries)]);
    }

    let listed = u.baseline.answer_for(&baseline_input(f)).or(u.result.as_ref());
    let errors = listed.and_then(|r| r["errors"].as_array()).map_or(&[][..], Vec::as_slice);
    let server = crate::env::label(s, &s.ui.env.active);
    let environment = s.state().config.as_ref().and_then(|c| c["environment"]["environmentId"].as_str()).unwrap_or(s.ui.env.active.as_str()).to_string();
    let order = if sub == Some("project") { super::pr_filters::sidebar_order(s) } else { Vec::new() };
    let projects = super::pr_filters::projects(&s.state().projects, &order, f.project.as_deref(), &server, &environment, errors);
    // PullRequestFilterOptionIcon: the chosen project's favicon, else "All projects" and its glyph.
    let (project, value) = match projects.iter().find(|p| Some(p.id.as_str()) == f.project.as_deref()) {
        Some(p) => (MenuItem::new("Project").leading(super::pr_filters::menu_favicon(s, t, &p.id, &p.title)), p.title.clone()),
        None => (MenuItem::new("Project").icon(&i::LAYERS), "All projects".to_string()),
    };
    let entries = super::pr_filters::project_entries(s, t, &projects);
    out.extend([MenuEntry::Separator, sub_item(project, value, "project", entries)]);
    out
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

/// usePullRequestListView's expectedHosts: each project remote's host, until a list names them.
fn expected_hosts(projects: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for p in projects {
        let identity = &p["repositoryIdentity"];
        let Some(kind) = identity["provider"].as_str() else { continue };
        let host = identity["canonicalKey"].as_str().and_then(|k| k.split('/').next()).map(str::trim).filter(|h| !h.is_empty());
        let host = host.map(str::to_lowercase).unwrap_or_else(|| kind.to_string());
        if !out.iter().any(|h| h["host"] == host.as_str()) {
            out.push(json!({"host": host, "kind": kind}));
        }
    }
    out
}

/// formatEnvironmentQueryError. The pull request errors' messages are getters web derives from
/// their fields, so the wire carries only the tag and fields.
pub(super) fn error_text(cause: &Value, fallback: &str) -> String {
    fn tagged(v: &Value) -> Option<&Value> {
        match v {
            Value::Object(m) if m.get("_tag").and_then(Value::as_str).is_some_and(|t| t.starts_with("PullRequest")) => Some(v),
            Value::Object(m) => m.values().find_map(tagged),
            Value::Array(rows) => rows.iter().find_map(tagged),
            _ => None,
        }
    }
    match tagged(cause) {
        Some(e) if field(e, "_tag") == "PullRequestUnavailableError" => match e["requirement"].as_str().map(str::trim).filter(|r| !r.is_empty()) {
            Some(requirement) => requirement.to_string(),
            None => match field(e, "reason") {
                "cli-missing" => "The tool this host is read through is not installed or set up.",
                "cli-unauthenticated" => "This host has no working credentials.",
                _ => "Change requests cannot be browsed for this project's host yet.",
            }
            .to_string(),
        },
        Some(e) if field(e, "_tag") == "PullRequestOperationError" => format!("Pull request operation {} failed: {}", field(e, "operation"), field(e, "detail")),
        _ => crate::shell::palette::error_message(cause, fallback),
    }
}

fn unavailable_host(p: &Value) -> Option<String> {
    (p["configured"] == false).then(|| p["detail"].as_str().unwrap_or("This host could not be read.").to_string())
}

fn group_view(s: &App, t: &T, result: &Value, group: Group, entries: &[Value], overlays: &mut Vec<El>) -> El {
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
        rows = rows.child(pr_row(s, t, result, e, overlays));
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

/// PULL_REQUEST_STATE_PRESENTATION's label.
fn state_label(e: &Value) -> &'static str {
    match field(e, "state") {
        "open" if e["isDraft"].as_bool().unwrap_or(false) => "Draft",
        "merged" => "Merged",
        "closed" => "Closed",
        _ => "Open",
    }
}

fn success(t: &T) -> Color {
    if t.dark { tw::EMERALD_300.alpha_mul(0.9) } else { tw::EMERALD_600 }
}

/// `Intl.NumberFormat` in English: 1234 is "1,234".
pub(crate) fn grouped(n: i64) -> String {
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

/// PullRequestListGroups' matchedElsewhere: the hosts matched the search somewhere no shown field
/// holds, such as the description.
fn matched_elsewhere(e: &Value, query: &str) -> bool {
    !query.is_empty() && score(e, query) <= 10
}

/// PullRequestRowGlyph: the state glyph, badged when an open pull request conflicts.
pub(crate) fn state_mark(t: &T, e: &Value) -> El {
    let (glyph, ink) = state_glyph(t, e);
    let mut mark = el().size(16.0, 16.0).child(icon(glyph, 16.0).color(ink)).label(state_label(e)).help(state_label(e));
    if conflicting(e) && open(e) && !e["isDraft"].as_bool().unwrap_or(false) {
        let tip = match field(e, "baseBranch") {
            "" => "Has conflicts".to_string(),
            base => format!("Conflicts with {base}"),
        };
        mark = mark.child(
            el().absolute(8.0, 8.0)
                .size(12.0, 12.0)
                .center()
                .radius(radius::FULL)
                .fill(t.c.background)
                .label(tip.clone())
                .help(tip)
                .child(icon(&i::TRIANGLE_ALERT, 12.0).color(t.c.destructive)),
        );
    }
    mark
}

/// PullRequestReviewDecisionGlyph.
pub(crate) fn review_mark(t: &T, decision: &str) -> Option<El> {
    let (glyph, ink, tip): (&'static rune::Icon, Color, &str) = match decision {
        "approved" => (&i::USER_CHECK, success(t), "Approved"),
        "changes-requested" => (&i::USER_ROUND_X, if t.dark { tw::AMBER_400.alpha_mul(0.8) } else { tw::AMBER_600.alpha_mul(0.9) }, "Changes requested"),
        "review-required" => (&i::USER_ROUND, t.c.muted_foreground.alpha_mul(0.6), "Awaiting review"),
        _ => return None,
    };
    Some(el().label(tip).help(tip).child(icon(glyph, 14.0).color(ink)))
}

fn pr_row(s: &App, t: &T, result: &Value, e: &Value, overlays: &mut Vec<El>) -> El {
    let url = field(e, "url").to_string();
    let selected = s.ui.pages.pull_requests.selected.as_deref() == Some(url.as_str());
    let mark = state_mark(t, e);
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
    let mut signals = row().gap(4.0).center_y().shrink().children(super::pr_row::checks(s, t, e, overlays));
    signals = signals.children(review_mark(t, field(e, "reviewDecision")));
    line1 = line1.child(signals).child(spacer()).children(super::pr_row::stack(s, t, e, overlays));
    let (add, del) = (e["additions"].as_i64().unwrap_or(0), e["deletions"].as_i64().unwrap_or(0));
    if add > 0 || del > 0 {
        let stat = theme::mono(theme::TEXT_2XS).features("'tnum' 1");
        let counts = row()
            .h(20.0)
            .gap(4.0)
            .center_y()
            .shrink()
            .child(text(format!("+{}", grouped(add))).font(stat).color(t.c.diff_addition_foreground))
            .child(text(format!("-{}", grouped(del))).font(stat).color(t.c.diff_deletion_foreground));
        line1 = line1.child(super::pr_row::breakdown(s, t, e, counts, overlays));
    }
    let login = e["author"]["login"].as_str().unwrap_or("ghost").to_string();
    let author_tip = match e["author"]["name"].as_str() {
        Some(name) if !name.is_empty() && name != login => format!("{name} (@{login})"),
        _ => login.clone(),
    };
    let meta = theme::TEXT_2XS;
    let matched = matched_elsewhere(e, &parse_query(&s.ui.pages.pull_requests.query.text).0);
    let meta_key = format!("pr-meta-{url}");
    let wide = matched && rune::memo::layout_with(&meta_key, super::pr_row::meta_wide).unwrap_or(false);
    let mut line2 = row()
        .key(meta_key)
        .w_full()
        .h(16.0)
        .gap(6.0)
        .center_y()
        .children(matched.then(|| super::pr_row::matched_badge(t, wide)))
        .children(super::pr_row::provider_mark(s, t, result, e))
        .child(
            row()
                .gap(4.0)
                .center_y()
                .shrink()
                .max_w(160.0)
                .help(author_tip)
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
    let select = url.clone();
    let body = row()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .cursor(Cursor::Pointer)
        .role("button")
        .label(field(e, "title").to_string())
        .px(12.0)
        .py(10.0)
        .gap(8.0)
        .child(mark.mt(2.0).self_start())
        .child(col().grow(1.0).shrink().min_w(0.0).child(line1).child(line2))
        .on_press(move |s: &mut App| ui(s).selected = Some(select.clone()));
    row()
        .key(format!("pr-{url}"))
        .w_full()
        .radius(radius::MD)
        .fill(if selected { t.c.accent } else { Color::TRANSPARENT })
        // PullRequestListGroups: a close sweep turns pointer events off over the list.
        .hover(match (selected, super::pr_row::sweeping(&s.ui.pages.pull_requests.row)) {
            (true, _) => t.c.accent,
            (false, true) => Color::TRANSPARENT,
            (false, false) => t.c.accent.alpha_mul(0.6),
        })
        .center_y()
        .child(body)
        .children(super::pr_row::speed_actions(s, t, result, e))
}

/// showPullRequestLinkContextMenu: copy the link, or open it on its host.
/// openOnHostLabel for a row's provider.
pub(super) fn open_label(e: &Value, hosts: &[Value]) -> String {
    let name = match host_label(&[], &json!({"kind": field(e, "provider"), "host": ""})) {
        n if n.is_empty() => hosts.iter().find(|h| field(h, "host") == field(e, "host")).map(|h| host_label(&[], h)).filter(|n| n != field(e, "host")).unwrap_or_default(),
        n => n,
    };
    if name.is_empty() { "Open on host".to_string() } else { format!("Open on {name}") }
}

fn context_menu(t: &T, e: &Value, hosts: &[Value], x: f32, y: f32, overlays: &mut Vec<El>) {
    let url = field(e, "url").to_string();
    let close = |s: &mut App| ui(s).context = None;
    let open_label = open_label(e, hosts);
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
                .child(icon(&i::LOADER_CIRCLE, 14.0).color(muted).looping(rune::LoopSpec::rotation(1.)))
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
    let retry = kit::Button::new("Retry")
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
    // carriedToNothing: a cleared search waits for the scope's own answer, not an empty state.
    if query.trim().is_empty() && !u.sent_query.trim().is_empty() {
        return ghost(t, 7, None);
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

        u.filters.labels = vec!["bug".into(), "ui".into()];
        assert_eq!(u.filters.count(""), 4);
        let base = baseline_input(&u.filters);
        assert_eq!(base, json!({"state": "open", "involvement": "all", "limit": PAGE, "filters": {"draft": "only", "checks": "passing", "labels": [["bug"], ["ui"]]}}));
        assert_eq!(partition_input(&u.filters, "reviewing"), json!({"state": "open", "involvement": "reviewing", "limit": PAGE, "filters": base["filters"]}));
        u.filters.project = Some("p1".into());
        assert_eq!(facet_input(&u.filters), json!({"state": "all", "involvement": "all", "limit": PAGE, "projectId": "p1"}));
        u.sent_query.clear();
        assert_eq!(input(&u), baseline_input(&u.filters));
    }

    #[test]
    fn picking_draft_in_the_menu_rewrites_a_typed_draft_qualifier() {
        assert_eq!(draft_change(r#"draft:false fix label:"needs design""#, "all", "only"), (Some(r#"fix label:"needs design""#.to_string()), "only"));
        assert_eq!(draft_change(r#"draft:true "draft:false" fix"#, "only", "all"), (Some(r#""draft:false" fix"#.to_string()), "all"));
        assert_eq!(draft_change("draft:false fix", "only", "hide"), (None, "only"));
        assert_eq!(draft_change("fix", "all", "hide"), (None, "hide"));

        let mut u = Ui::default();
        u.query.replace_text("draft:true crash");
        assert_eq!(u.filters.count(&u.query.text), 1);
        pick_draft(&mut u, "hide");
        assert_eq!((u.query.text.as_str(), u.filters.draft), ("crash", "hide"));
        assert_eq!(u.filters.count(&u.query.text), 1);
        pick_draft(&mut u, "all");
        assert_eq!((u.query.text.as_str(), u.filters.count(&u.query.text)), ("crash", 0));
    }

    #[test]
    fn the_panel_toggle_hides_and_reopens_and_only_an_open_panel_maximizes() {
        let mut u = Ui::default();
        toggle_panel(&mut u);
        toggle_maximized(&mut u);
        assert!(!panel_available(&u) && u.selected.is_none() && !u.maximized);

        u.selected = Some("https://github.com/acme/app/pull/7".into());
        toggle_maximized(&mut u);
        assert!(u.maximized);
        toggle_panel(&mut u);
        settle_panel(&mut u);
        assert_eq!((u.selected.as_deref(), u.hidden.as_deref(), u.maximized), (None, Some("https://github.com/acme/app/pull/7"), false));
        assert!(panel_available(&u));
        toggle_maximized(&mut u);
        assert!(!u.maximized);

        toggle_panel(&mut u);
        settle_panel(&mut u);
        assert_eq!((u.selected.as_deref(), u.hidden.as_deref()), (Some("https://github.com/acme/app/pull/7"), None));
        toggle_panel(&mut u);
        u.selected = Some("https://github.com/acme/app/pull/8".into());
        settle_panel(&mut u);
        assert_eq!(u.hidden, None);
    }

    #[test]
    fn live_refresh_reads_on_arrival_and_every_five_minutes_only_while_in_use() {
        let mut l = Live::default();
        let t0 = 1_000_000;
        assert!(!l.due(true, t0, false), "a view never read reads for itself");
        assert!(!l.due(true, t0 + LIVE_EVERY - 1, false));
        assert!(l.due(true, t0 + LIVE_EVERY, false), "the interval fires while in use");
        assert!(!l.due(true, t0 + 2 * LIVE_EVERY, false), "six minutes without input is idle");
        let back = t0 + 2 * LIVE_EVERY + 1_000;
        l.input(back);
        assert!(l.due(true, back, false), "input after an idle spell arrives");
        assert!(!l.due(true, back + 1, true));
        assert!(!l.due(true, back + 2, false), "returning to the window within the floor");
        assert!(!l.due(true, back + 20_000, true));
        assert!(l.due(true, back + 20_001, false), "returning to the window after the floor");
        assert!(!l.due(false, back + 40_000, false));
        assert!(l.due(true, back + 40_001, false), "showing a view read before arrives");
        assert!(!l.due(true, back + 40_001 + LIVE_EVERY - 1, false), "showing again restarts the interval");
    }

    #[test]
    fn escape_closes_the_open_pull_request_then_leaves_the_page() {
        let mut s = App::new();
        s.route = Route::PullRequests;
        s.ui.pages.main_route = Some(Route::Thread("t1".into()));
        s.ui.pages.pull_requests.selected = Some("https://github.com/acme/app/pull/7".into());
        assert!(escape(&mut s) && matches!(s.route, Route::PullRequests), "the open pull request closes first");
        assert!(escape(&mut s));
        assert!(matches!(&s.route, Route::Thread(id) if id == "t1"), "then Escape leaves for the page it was opened over");
    }

    #[test]
    fn a_close_sweep_survives_polls_and_ends_when_the_search_changes() {
        let mut u = Ui::default();
        let sweeping = |u: &Ui| super::super::pr_row::sweeping(&u.row);
        super::super::pr_row::start_sweep(&mut u.row);
        assert!(!settle_sweep(&mut u) && sweeping(&u), "the first poll only records the filters");
        assert!(!settle_sweep(&mut u) && sweeping(&u));
        u.sent_query = "crash".into();
        assert!(settle_sweep(&mut u) && !sweeping(&u), "a new search ends the sweep");
    }

    #[test]
    fn the_condensed_header_search_opens_on_focus_and_closes_empty_on_blur() {
        let mut u = Ui::default();
        u.query.focused = true;
        settle_search(&mut u);
        assert!(!u.search_open, "the full header has no search of its own");
        u.condensed = true;
        assert!(settle_search(&mut u) && u.search_open, "the focused field moves into the header");
        u.query.focused = false;
        assert!(settle_search(&mut u) && !u.search_open, "an empty field closes on blur");
        u.query.replace_text("login");
        u.query.focused = true;
        settle_search(&mut u);
        u.query.focused = false;
        assert!(!settle_search(&mut u) && u.search_open, "a field with text stays");
        u.condensed = false;
        assert!(settle_search(&mut u) && !u.search_open);
    }

    #[test]
    fn a_finished_turn_rereads_the_whole_loaded_list() {
        let mut seen = None;
        assert!(!turned(&mut seen, None));
        assert!(turned(&mut seen, Some(3)));
        assert!(!turned(&mut seen, Some(3)));
        assert!(turned(&mut seen, Some(4)));

        let rows = |n: u64, host: &str, provider: &str, repository: &str| (0..n).map(|i| json!({"host": host, "provider": provider, "repository": repository, "number": i})).collect::<Vec<_>>();
        let mut gitlab = rows(30, "GitLab.com", "gitlab", "acme/a");
        gitlab.extend(rows(40, "gitlab.com", "gitlab", "acme/b"));
        gitlab.extend(rows(60, "github.com", "github", "acme/c"));
        gitlab.extend(rows(5, "github.com", "github", "acme/d"));
        assert_eq!(largest_read(&gitlab), 65);

        let mut u = Ui::default();
        u.result = Some(json!({"entries": rows(150, "github.com", "github", "acme/app")}));
        u.requested = Some(json!({}));
        u.baseline.asked = Some(json!({}));
        u.stats_asked.insert("k".into());
        after_turn(&mut u);
        assert_eq!(u.limit, Some(2 * PAGE));
        assert!(u.requested.is_none() && u.baseline.asked.is_none() && u.stats_asked.is_empty());

        u.limit = None;
        u.result = Some(json!({"entries": rows(1, "github.com", "github", "acme/app")}));
        after_turn(&mut u);
        assert_eq!(u.limit, None);
    }

    #[test]
    fn the_partitions_lead_the_groups_until_a_search_starts() {
        let row = |number: u64, login: &str, updated: &str| entry(json!({"number": number, "author": {"login": login}, "updatedAt": updated}));
        let mut u = Ui::default();
        u.filters.sort = "updated";
        let mut asked = row(2, "mara", "2026-01-04T00:00:00Z");
        asked["viewerReviewRequested"] = json!(true);
        let result = json!({"viewers": {"github.com": "octo"}, "errors": [], "entries": [row(1, "octo", "2026-01-05T00:00:00Z"), asked, row(3, "lena", "2026-01-03T00:00:00Z")]});
        u.result = Some(result.clone());
        let mut stale = row(1, "octo", "2026-01-01T00:00:00Z");
        stale["title"] = json!("Stale copy");
        let older = row(9, "octo", "2025-12-01T00:00:00Z");
        hold_partitions(&mut u, json!({"entries": [older, stale.clone()]}), json!({"entries": [stale, row(3, "lena", "2026-01-03T00:00:00Z")]}));
        let shown = |u: &Ui| groups(u, &result).into_iter().map(|(g, rows)| (g, rows.iter().map(|e| e["number"].as_u64().unwrap_or(0)).collect::<Vec<_>>())).collect::<Vec<_>>();

        let partitioned = shown(&u);
        assert!(partitioned.iter().map(|g| g.0).eq([Group::Authored, Group::Review, Group::Others]));
        assert_eq!(partitioned.into_iter().map(|g| g.1).collect::<Vec<_>>(), [vec![1, 9], vec![3], vec![2]]);
        assert_eq!(groups(&u, &result)[0].1[0]["title"], "Fix login redirect");

        u.query.replace_text("fix");
        assert_eq!(shown(&u).into_iter().map(|g| g.1).collect::<Vec<_>>(), [vec![1], vec![2], vec![3]]);

        u.query.replace_text("");
        // A closed row leaves the open list at once, the partition reads' copies with it.
        after_action(&mut u, &key(&row(3, "lena", "")), "close", 0);
        let acted = u.result.clone().unwrap_or_default();
        let after = groups(&u, &acted);
        assert!(after.iter().map(|g| g.0).eq([Group::Authored, Group::Others]));
        assert_eq!(after.iter().map(|g| g.1.len()).collect::<Vec<_>>(), [2, 1]);

        u.result = Some(json!({"viewers": {}, "errors": [], "entries": []}));
        hold_partitions(&mut u, json!({"entries": [row(9, "octo", "2025-12-01T00:00:00Z")]}), json!({"entries": []}));
        assert!(held_partitions(&u).is_none());
    }

    #[test]
    fn copying_names_the_change_request_by_its_host() {
        assert_eq!(short_label("https://gitlab.com/acme/app/-/merge_requests/3"), "MR");
        assert_eq!(short_label("https://github.com/acme/app/pull/3"), "PR");
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
        assert!(!matched_elsewhere(&e, ""));
        assert!(!matched_elsewhere(&e, "fix/login"));
        assert!(!matched_elsewhere(&e, "redirect"));
        assert!(matched_elsewhere(&e, "session cookie"));
        assert!(matched_elsewhere(&e, "#8"));
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
        let projects = [
            json!({"repositoryIdentity": {"provider": "github", "canonicalKey": "GitHub.com/s5labs/demo"}}),
            json!({"repositoryIdentity": {"provider": "github", "canonicalKey": "github.com/s5labs/other"}}),
            json!({"repositoryIdentity": null}),
        ];
        assert_eq!(expected_hosts(&projects), vec![json!({"host": "github.com", "kind": "github"})]);
    }

    #[test]
    fn failures_read_like_the_error_classes_web_decodes() {
        let fail = |error: Value| json!([{"_tag": "Fail", "error": error}]);
        let unauthenticated = fail(json!({"_tag": "PullRequestUnavailableError", "reason": "cli-unauthenticated", "provider": "github"}));
        assert_eq!(error_text(&unauthenticated, "x"), "This host has no working credentials.");
        let named = fail(json!({"_tag": "PullRequestUnavailableError", "reason": "cli-missing", "requirement": "Set GH_TOKEN."}));
        assert_eq!(error_text(&named, "x"), "Set GH_TOKEN.");
        let operation = fail(json!({"_tag": "PullRequestOperationError", "operation": "list", "detail": "rate limited"}));
        assert_eq!(error_text(&operation, "x"), "Pull request operation list failed: rate limited");
        assert_eq!(error_text(&json!({"message": "offline"}), "x"), "offline");
        assert_eq!(error_text(&json!([{"_tag": "Die"}]), "The environment request failed."), "The environment request failed.");
    }
}
