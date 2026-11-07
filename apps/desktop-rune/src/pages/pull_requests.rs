//! `/pull-requests` (PullRequestsPage): every project's pull requests in one list, grouped by
//! involvement and ranked by the chosen sort, with the toolbar's search, sort, filters and host.

use rune::{Color, Cursor, El, PointerPress, ScrollSpec, Tx, col, el, icon, row, spacer, text, text_input};
use serde_json::{Value, json};

use crate::data::{s as field, ts};
use crate::icons as i;
use crate::kit::{self, Menu, MenuEntry, MenuItem, Placement};
use crate::theme::{self, AlphaMul, T, layout, radius, tw};
use crate::{App, Route};

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
    /// The query sent to the hosts (on Enter); typing filters the loaded rows meanwhile.
    pub sent_query: String,
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
const DRAFTS: &[(&str, &str)] = &[("all", "All"), ("only", "Drafts only"), ("exclude", "Hide drafts")];
const REVIEWS: &[(&str, &str)] = &[("all", "All"), ("approved", "Approved"), ("changes-requested", "Changes requested"), ("review-required", "Review required"), ("none", "No reviews")];
const CHECKS: &[(&str, &str)] = &[("all", "All"), ("passing", "Passing"), ("failing", "Failing")];

fn ui(s: &mut App) -> &mut Ui {
    &mut s.ui.pages.pull_requests
}

/// The request the toolbar describes, shaped as `pullRequests.list`'s input.
pub(super) fn input(u: &Ui) -> Value {
    let f = &u.filters;
    let mut filters = serde_json::Map::new();
    if f.draft != "all" {
        filters.insert("draft".into(), json!(f.draft));
    }
    if f.review != "all" {
        filters.insert("review".into(), json!(f.review));
    }
    if f.checks != "all" {
        filters.insert("checks".into(), json!(f.checks));
    }
    if let Some(label) = &f.label {
        filters.insert("labels".into(), json!([[label]]));
    }
    if let Some(author) = &f.author {
        filters.insert("author".into(), json!(author));
    }
    let mut v = json!({"state": f.state, "limit": 99});
    if f.involvement != "all" {
        v["involvement"] = json!(f.involvement);
    }
    if !filters.is_empty() {
        v["filters"] = Value::Object(filters);
    }
    if let Some(p) = &f.project {
        v["projectId"] = json!(p);
    }
    if let Some(h) = &f.host {
        v["host"] = json!(h);
    }
    if !u.sent_query.is_empty() {
        v["query"] = json!(u.sent_query);
    }
    v
}

pub fn supported(s: &App) -> bool {
    s.state().config.is_some()
}

pub fn poll(s: &mut App) -> bool {
    if !matches!(s.frame_route(), Route::PullRequests) {
        return false;
    }
    let mut changed = false;
    if let Some((id, append)) = s.ui.pages.pull_requests.pending
        && let Some(reply) = s.src.net().and_then(|n| n.take_reply(id))
    {
        let u = ui(s);
        u.pending = None;
        match reply {
            Ok(mut result) => {
                if append && let Some(prev) = u.result.take() {
                    let mut entries = prev["entries"].as_array().cloned().unwrap_or_default();
                    entries.extend(result["entries"].as_array().cloned().unwrap_or_default());
                    result["entries"] = Value::Array(entries);
                }
                u.result = Some(result);
                u.error = None;
            }
            Err(e) => u.error = Some(e["message"].as_str().unwrap_or("Could not load pull requests.").to_string()),
        }
        changed = true;
    }
    let wanted = input(&s.ui.pages.pull_requests);
    if s.ui.pages.pull_requests.requested.as_ref() != Some(&wanted) {
        s.ui.pages.pull_requests.requested = Some(wanted.clone());
        fetch(s, wanted, false);
        changed = true;
    }
    changed
}

fn fetch(s: &mut App, payload: Value, append: bool) {
    match s.src.net() {
        Some(net) => {
            let id = net.request("pullRequests.list", payload);
            ui(s).pending = Some((id, append));
        }
        None if !crate::host::live() => {
            if ui(s).result.is_none() {
                ui(s).result = super::fixture("pull-requests.json");
            }
        }
        None => {}
    }
}

fn refresh(s: &mut App) {
    ui(s).requested = None;
}

fn load_more(s: &mut App) {
    let Some(cursors) = s.ui.pages.pull_requests.result.as_ref().map(|r| r["nextCursors"].clone()) else { return };
    let mut payload = input(&s.ui.pages.pull_requests);
    payload["cursors"] = cursors;
    fetch(s, payload, true);
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
    result["viewers"][field(e, "host")].as_str().unwrap_or("")
}

fn authored(result: &Value, e: &Value) -> bool {
    let me = viewer(result, e);
    !me.is_empty() && e["author"]["login"].as_str().is_some_and(|l| l.eq_ignore_ascii_case(me))
}

fn open(e: &Value) -> bool {
    field(e, "state") == "open"
}

fn conflicting(e: &Value) -> bool {
    open(e) && !e["isDraft"].as_bool().unwrap_or(false) && field(e, "mergeability") == "conflicting"
}

fn size(e: &Value) -> i64 {
    e["additions"].as_i64().unwrap_or(0) + e["deletions"].as_i64().unwrap_or(0)
}

fn matches_filters(f: &Filters, result: &Value, e: &Value, query: &str) -> bool {
    let state_ok = f.state == "all" || field(e, "state") == f.state;
    let involvement_ok = match f.involvement {
        "reviewing" => e["viewerReviewRequested"].as_bool().unwrap_or(false),
        "authored" => authored(result, e),
        _ => true,
    };
    let draft = e["isDraft"].as_bool().unwrap_or(false);
    let draft_ok = match f.draft {
        "only" => draft,
        "exclude" => !draft,
        _ => true,
    };
    let review_ok = match f.review {
        "all" => true,
        "none" => e["reviewDecision"].is_null(),
        r => field(e, "reviewDecision") == r,
    };
    let checks_ok = f.checks == "all" || field(e, "checksState") == f.checks;
    let author_ok = f.author.as_deref().is_none_or(|a| e["author"]["login"].as_str() == Some(a));
    let labels = e["labels"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let label_ok = f.label.as_deref().is_none_or(|l| labels.iter().any(|x| field(x, "name") == l));
    let project_ok = f.project.as_deref().is_none_or(|p| field(e, "projectId") == p);
    let host_ok = f.host.as_deref().is_none_or(|h| field(e, "host") == h);
    state_ok && involvement_ok && draft_ok && review_ok && checks_ok && author_ok && label_ok && project_ok && host_ok && matches_query(e, query)
}

/// `#N title repository headBranch login`, plus `label:x` tokens against the labels.
fn matches_query(e: &Value, query: &str) -> bool {
    let hay = format!("#{} {} {} {} {}", e["number"], field(e, "title"), field(e, "repository"), field(e, "headBranch"), e["author"]["login"].as_str().unwrap_or("")).to_lowercase();
    query.split_whitespace().all(|token| match token.strip_prefix("label:") {
        Some(label) => e["labels"].as_array().is_some_and(|ls| ls.iter().any(|l| field(l, "name").to_lowercase().contains(label))),
        None => hay.contains(token),
    })
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

fn rank(entries: &mut [&Value], sort: &str, group: Group, ranked_by_query: bool) {
    let updated = |e: &Value| std::cmp::Reverse(ts(e, "updatedAt").unwrap_or(0));
    entries.sort_by_key(|e| updated(*e));
    if ranked_by_query && matches!(sort, "ready" | "blocked") {
        return;
    }
    match sort {
        "ready" => entries.sort_by_key(|e| (ready_tier(e), size(e) == 0, size(e))),
        "blocked" => entries.sort_by_key(|e| blocked_tier(e, group)),
        "newest" => entries.sort_by_key(|e| std::cmp::Reverse(ts(e, "createdAt").unwrap_or(0))),
        "oldest" => entries.sort_by_key(|e| ts(e, "createdAt").unwrap_or(0)),
        "largest" => entries.sort_by_key(|e| (size(e) == 0, std::cmp::Reverse(size(e)))),
        "smallest" => entries.sort_by_key(|e| (size(e) == 0, size(e))),
        _ => {}
    }
}

fn groups<'a>(u: &Ui, result: &'a Value) -> Vec<(Group, Vec<&'a Value>)> {
    let query = u.query.text.trim().to_lowercase();
    let rows: Vec<&Value> = result["entries"].as_array().into_iter().flatten().filter(|e| matches_filters(&u.filters, result, e, &query)).collect();
    let ranked = !query.is_empty();
    if u.filters.involvement != "all" {
        let mut all = rows;
        rank(&mut all, u.filters.sort, Group::All, ranked);
        return if all.is_empty() { vec![] } else { vec![(Group::All, all)] };
    }
    let mut out = Vec::new();
    for group in [Group::Authored, Group::Review, Group::Others] {
        let mut members: Vec<&Value> = rows
            .iter()
            .copied()
            .filter(|e| {
                let mine = authored(result, e);
                match group {
                    Group::Authored => mine,
                    Group::Review => !mine && e["viewerReviewRequested"].as_bool().unwrap_or(false),
                    _ => !mine && !e["viewerReviewRequested"].as_bool().unwrap_or(false),
                }
            })
            .collect();
        if !members.is_empty() {
            rank(&mut members, u.filters.sort, group, ranked);
            out.push((group, members));
        }
    }
    out
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
    let loading = u.result.is_none() && u.error.is_none();
    match (&u.result, &u.error) {
        (None, Some(error)) => content = content.child(unavailable(t, error)),
        _ if loading => content = content.child(ghost(t)),
        (Some(result), error) => {
            if let Some(error) = error {
                content = content.child(error_banner(t, error));
            }
            let groups = groups(u, result);
            if groups.is_empty() {
                content = content.child(empty(s, t, result));
            } else {
                for (group, entries) in groups {
                    content = content.child(group_view(s, t, result, group, &entries, overlays));
                }
                if result["truncated"].as_bool().unwrap_or(false) {
                    content = content.child(
                        row().w_full().py(12.0).center().child(Button("Load more pull requests", None).sm().on_press(load_more).build(t)),
                    );
                }
            }
        }
        _ => {}
    }
    let body = kit::scroll_area(
        "pr-scroll",
        col().key("pr-scroll").grow(1.0).min_h(0.0).w_full().items_center().scroll(ScrollSpec { stick_bottom: false }).child(content),
        t,
    );
    if let Some((url, x, y)) = &u.context
        && let Some(e) = u.result.as_ref().and_then(|r| r["entries"].as_array()?.iter().find(|e| field(e, "url") == url))
    {
        context_menu(t, e, *x, *y, overlays);
    }
    col().key("main").grow(1.0).shrink().min_w(0.0).h_full().fill(t.c.background).child(header).child(body)
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

    let hosts: Vec<String> = result.and_then(|r| r["providers"].as_array()).into_iter().flatten().map(|p| field(p, "host").to_string()).collect();
    let host_label = u.filters.host.clone().unwrap_or_else(|| "All".into());
    let mut host = Menu::new("pr-host")
        .trigger(kit::Button::new(host_label).variant("outline").icon(&i::PLUG_2).on_press(toggle("host")).build(t))
        .placement(Placement::BottomStart)
        .min_width(180.0)
        .open(open("host"))
        .on_close(close)
        .item(MenuItem::new("All hosts").radio(u.filters.host.is_none()).on_select(move |s: &mut App| {
            ui(s).filters.host = None;
            close(s);
        }));
    for h in hosts {
        let checked = u.filters.host.as_deref() == Some(h.as_str());
        host = host.item(MenuItem::new(h.clone()).radio(checked).on_select(move |s: &mut App| {
            ui(s).filters.host = Some(h.clone());
            close(s);
        }));
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
        .child(kit::Button::new("Refresh").variant("outline").size("icon").icon(&i::REFRESH_CW).on_press(refresh).build(t))
}

fn group_view(s: &App, t: &T, result: &Value, group: Group, entries: &[&Value], overlays: &mut Vec<El>) -> El {
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

/// GitHub-style compact counts: 1.2k, 34k, 1.5m.
fn compact(n: i64) -> String {
    let short = |v: f64, unit: &str| if v < 10.0 { format!("{}{unit}", format!("{v:.1}").trim_end_matches(".0")) } else { format!("{}{unit}", v.round()) };
    match n {
        n if n < 1000 => n.to_string(),
        n if n < 999_500 => short(n as f64 / 1e3, "k"),
        n => short(n as f64 / 1e6, "m"),
    }
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
    if conflicting(e) {
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
        .child(text(format!("#{}", e["number"])).font(mono).color(t.c.muted_foreground).shrink())
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
                .gap(8.0)
                .shrink()
                .child(row().min_w(27.0).justify_end().child(text(format!("+{}", compact(add))).font(stat).color(t.c.diff_addition_foreground)))
                .child(row().min_w(27.0).justify_end().child(text(format!("-{}", compact(del))).font(stat).color(t.c.diff_deletion_foreground))),
        );
    }
    let login = e["author"]["login"].as_str().unwrap_or("ghost").to_string();
    let initial = login.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
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
                .child(el().size(14.0, 14.0).radius(radius::FULL).fill(t.c.muted).center().child(text(initial).font(theme::weight(theme::TEXT_3XS, theme::MEDIUM).lh(14.0)).color(t.c.muted_foreground)))
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
    let menu_url = url.clone();
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
        .on_secondary_press(move |s: &mut App, at: PointerPress, _: &mut Tx| {
            let u = ui(s);
            u.selected = Some(menu_url.clone());
            u.context = Some((menu_url.clone(), at.x, at.y));
        })
}

fn context_menu(t: &T, e: &Value, x: f32, y: f32, overlays: &mut Vec<El>) {
    let url = field(e, "url").to_string();
    let project = field(e, "projectId").to_string();
    let close = |s: &mut App| ui(s).context = None;
    let open_url = url.clone();
    let copy_url = url.clone();
    Menu::new("pr-context")
        .native(true)
        .width(198.0)
        .open(true)
        .at(x, y)
        .on_close(close)
        .item(MenuItem::new("Open on GitHub").icon(&i::EXTERNAL_LINK).on_select_tx(move |s: &mut App, tx: &mut Tx| {
            let _ = tx.open_url(&open_url);
            ui(s).context = None;
        }))
        .item(MenuItem::new("Copy link").icon(&i::COPY).on_select_tx(move |s: &mut App, tx: &mut Tx| {
            tx.copy(&copy_url);
            ui(s).context = None;
        }))
        .separator()
        .item(MenuItem::new("Start thread in project").icon(&i::SQUARE_PEN).on_select(move |s: &mut App| {
            ui(s).context = None;
            crate::shell::sidebar::go(s, Route::Home { project_id: Some(project.clone()) });
        }))
        .overlay(&kit::theme(t), overlays);
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
        .child(Button("Retry", None).with("xs").on_press(refresh).build(t))
}

const TITLE_W: [f32; 7] = [0.6, 0.4, 0.5, 0.667, 0.4, 0.6, 0.5];
const META_W: [f32; 7] = [0.4, 0.333, 0.4, 0.25, 0.333, 0.4, 0.333];

/// PullRequestGhosts: static placeholder rows while the first page loads.
fn ghost(t: &T) -> El {
    let bar = t.c.muted_foreground.alpha_mul(0.15);
    let mut rows = col().w_full().gap(2.0).child(row().px(12.0).pb(4.0).child(text("Loading pull requests…").font(theme::XS_MEDIUM).color(t.c.muted_foreground)));
    for n in 0..7 {
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

fn unavailable(t: &T, error: &str) -> El {
    state_block(t, media(t, &i::GIT_PULL_REQUEST_ARROW), "Could not load pull requests", error, Button("Retry", Some(&i::REFRESH_CW)).sm().on_press(refresh).build(t))
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
    let query = u.query.text.trim();
    let any = result["entries"].as_array().is_some_and(|e| !e.is_empty());
    let (title, description) = if !query.is_empty() {
        (format!("Nothing matches “{query}”"), "The hosts were searched for it. Try fewer words, or search by number, author or branch.")
    } else if any || u.filters.count() > 0 {
        ("Nothing under these filters".to_string(), "Widen the state, involvement or project filter to see more.")
    } else {
        ("No pull requests".to_string(), "Pull requests from every project in this workspace appear here.")
    };
    let mark = el().w(128.0).h(80.0).clip_rounded(0.0).mb(8.0).child(icon(&BRANCH_MARK, 128.0).color(t.c.muted_foreground.alpha_mul(0.6)).absolute(0.0, 1.6));
    let mut actions = row().gap(8.0);
    if !query.is_empty() {
        actions = actions.child(Button("Clear search", Some(&i::SEARCH)).sm().on_press(|s: &mut App| {
            let u = ui(s);
            u.query.clear();
            u.sent_query.clear();
        }).build(t));
    }
    actions = actions.child(Button("Check again", Some(&i::REFRESH_CW)).sm().on_press(refresh).build(t));
    state_block(t, mark, &title, description, actions)
}
