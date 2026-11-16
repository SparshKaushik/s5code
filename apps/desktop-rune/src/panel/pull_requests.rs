//! The thread's linked pull requests (ThreadPullRequestsPanel): native stacks and base-branch
//! chains, newest first, each row with its state, signals and actions, over a footer to link more.

use std::collections::{HashMap, HashSet};

use rune::{Cursor, El, PointerPress, ScrollSpec, Tx, col, el, icon, row, spacer, text};
use serde_json::{Value, json};

use crate::data::{iso_ms, s as field};
use crate::icons as i;
use crate::kit::{self, Menu, MenuItem, Placement};
use crate::pages::pr_row;
use crate::pages::pull_requests::{grouped, relative, review_mark, state_mark};
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route};

#[derive(Default)]
pub struct Ui {
    /// The open row menu: the row's key and, when a right-click opened it, the pointer.
    pub(super) menu: Option<(String, Option<(f32, f32)>)>,
}

/// visibleThreadPullRequests.
pub(crate) fn visible(thread: &Value) -> Vec<&Value> {
    thread["pullRequests"].as_array().into_iter().flatten().filter(|l| l["source"] != "stack-dismissed").collect()
}

fn thread(app: &App) -> Option<&Value> {
    let Route::Thread(id) = app.frame_route() else { return None };
    app.state().threads.iter().find(|v| v["id"] == id.as_str())
}

/// RightPanelTabs' availability: a server thread, on a server that answers its config, with a
/// visible link.
pub(crate) fn available(app: &App) -> bool {
    app.state().config.is_some() && thread(app).is_some_and(|v| !visible(v).is_empty())
}

/// useProactivePanels' targetKey: the visible links' keys, sorted, while the surface is available.
pub(crate) fn proactive_key(app: &App) -> Option<String> {
    if !available(app) {
        return None;
    }
    let mut keys: Vec<Value> = visible(thread(app)?).iter().map(|l| json!([l["host"], l["repository"], l["number"]])).collect();
    keys.sort_by_key(|k| k.to_string());
    Some(Value::Array(keys).to_string())
}

fn key_of(l: &Value) -> String {
    format!("{}/{}#{}", field(l, "host").trim().to_lowercase(), field(l, "repository").trim().to_lowercase(), l["number"])
}

fn updated(l: &Value) -> i64 {
    l["snapshot"]["updatedAt"].as_str().or(l["linkedAt"].as_str()).and_then(iso_ms).unwrap_or(0)
}

struct Chain<'a> {
    native: bool,
    layers: Vec<&'a Value>,
    /// 0 for the base, the parent's depth + 1 above it; siblings share one.
    depths: Vec<usize>,
}

/// resolveThreadPullRequestChains: native stacks in the order of the layers that cover the most
/// members, then chains linked by base branch within a repository, then whatever is left alone.
fn chains<'a>(visible: &[&'a Value]) -> Vec<Chain<'a>> {
    let repo = |l: &Value| format!("{}/{}", field(l, "host").trim().to_lowercase(), field(l, "repository").trim().to_lowercase());
    let mut out = Vec::new();
    let mut placed = HashSet::new();
    let mut stacks: Vec<(String, Vec<&'a Value>)> = Vec::new();
    for &link in visible {
        let Some(id) = link["stack"]["id"].as_str() else { continue };
        let key = format!("{}#stack:{id}", repo(link));
        match stacks.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(link),
            None => stacks.push((key, vec![link])),
        }
    }
    for (_, mut members) in stacks {
        let numbers: HashSet<u64> = members.iter().filter_map(|m| m["number"].as_u64()).collect();
        let layers_of = |m: &'a Value| m["stack"]["layers"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let cover = |layers: &[Value]| layers.iter().filter(|l| l["number"].as_u64().is_some_and(|n| numbers.contains(&n))).count();
        let mut layers = layers_of(members[0]);
        let mut coverage = cover(layers);
        for &m in &members[1..] {
            let candidate = layers_of(m);
            let c = cover(candidate);
            if c > coverage || (c == coverage && candidate.len() > layers.len()) {
                (layers, coverage) = (candidate, c);
            }
        }
        members.sort_by_key(|m| layers.iter().rposition(|l| l["number"] == m["number"]).unwrap_or(layers.len()));
        placed.extend(members.iter().map(|m| key_of(m)));
        let depths = (0..members.len()).collect();
        out.push(Chain { native: true, layers: members, depths });
    }
    let remaining: Vec<&'a Value> = visible.iter().copied().filter(|l| !placed.contains(&key_of(l))).collect();
    let branch = |l: &Value, name: &str| format!("{}:{}", repo(l), field(&l["snapshot"], name));
    let mut by_head: HashMap<String, Option<&'a Value>> = HashMap::new();
    for &l in remaining.iter().filter(|l| !l["snapshot"].is_null()) {
        let key = branch(l, "headBranch");
        let unique = !by_head.contains_key(&key);
        by_head.insert(key, unique.then_some(l));
    }
    let parent_of = |l: &Value| -> Option<&'a Value> {
        if l["snapshot"].is_null() {
            return None;
        }
        let parent = (*by_head.get(&branch(l, "baseBranch"))?)?;
        (!std::ptr::eq(parent, l)).then_some(parent)
    };
    let mut children: HashMap<String, Vec<&'a Value>> = HashMap::new();
    for &l in &remaining {
        if let Some(parent) = parent_of(l) {
            children.entry(key_of(parent)).or_default().push(l);
        }
    }
    for &base in remaining.iter().filter(|l| parent_of(**l).is_none()) {
        let (mut layers, mut depths) = (Vec::new(), Vec::new());
        let mut pending = vec![(base, 0)];
        while let Some((l, depth)) = pending.pop() {
            let key = key_of(l);
            // Two links with one identity would otherwise share a child list and expand forever.
            if !placed.insert(key.clone()) {
                continue;
            }
            layers.push(l);
            depths.push(depth);
            pending.extend(children.get(&key).into_iter().flatten().rev().map(|&c| (c, depth + 1)));
        }
        if !layers.is_empty() {
            out.push(Chain { native: false, layers, depths });
        }
    }
    for &l in &remaining {
        if !placed.contains(&key_of(l)) {
            out.push(Chain { native: false, layers: vec![l], depths: vec![0] });
        }
    }
    out
}

/// resolveThreadPullRequestBadge's stack: several links that form one chain.
pub(crate) fn single_chain(visible: &[&Value]) -> bool {
    chains(visible).len() == 1
}

struct Line<'a> {
    link: &'a Value,
    depth: usize,
    /// On a chain's first row: whether it's a native stack, and its size.
    stack: Option<(bool, usize)>,
}

/// pullRequestListLines: chains by their newest layer, then their rows in order.
fn lines(mut chains: Vec<Chain<'_>>) -> Vec<Line<'_>> {
    chains.sort_by_key(|c| std::cmp::Reverse(c.layers.iter().map(|l| updated(l)).max().unwrap_or(0)));
    chains
        .into_iter()
        .flat_map(|c| {
            let (native, size) = (c.native, c.layers.len());
            c.layers.into_iter().zip(c.depths).enumerate().map(move |(n, (link, depth))| Line { link, depth, stack: (n == 0 && size > 1).then_some((native, size)) })
        })
        .collect()
}

fn source_label(source: &str) -> &'static str {
    match source {
        "manual" => "Linked by you",
        "created" => "Created from this thread",
        "agent" => "Linked by the agent",
        "stack" => "Found in the stack",
        _ => "Dismissed",
    }
}

/// The source control kind a link's URL belongs to (findByChangeRequestUrl).
fn provider(url: &str) -> &'static str {
    let host = url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or("").to_lowercase();
    if url.contains("/-/merge_requests/") {
        "gitlab"
    } else if url.contains("/_git/") && url.contains("/pullrequest/") {
        "azure-devops"
    } else if url.contains("/pull-requests/") {
        "bitbucket"
    } else if host.ends_with("git.cafe") {
        "gitcafe"
    } else if url.contains("/pulls/") {
        "forgejo"
    } else if url.contains("/pull/") {
        "github"
    } else {
        ""
    }
}

fn host_label(kind: &str) -> &'static str {
    match kind {
        "github" => "GitHub",
        "gitlab" => "GitLab",
        "bitbucket" => "Bitbucket",
        "azure-devops" => "Azure DevOps",
        "gitcafe" => "GitCafe",
        "forgejo" => "Forgejo",
        _ => "Source control",
    }
}

fn plural(kind: &str) -> &'static str {
    match kind {
        "gitlab" => "merge requests",
        "" => "change requests",
        _ => "pull requests",
    }
}

/// findProjectForChangeRequest, the thread's own project first, else the thread's project.
fn project_for(app: &App, thread: &Value, link: &Value) -> String {
    let key = format!("{}/{}", field(link, "host"), field(link, "repository")).to_lowercase();
    let holds = |p: &&Value| {
        let identity = &p["repositoryIdentity"];
        identity["origin"]["canonicalKey"].as_str().or(identity["canonicalKey"].as_str()).is_some_and(|k| k.to_lowercase() == key)
    };
    let projects = &app.state().projects;
    let own = projects.iter().filter(|p| p["id"] == thread["projectId"]).find(holds);
    own.or_else(|| projects.iter().find(holds)).map_or(field(thread, "projectId"), |p| field(p, "id")).to_string()
}

/// The list entry the pull requests page's row pieces read, from a link and its snapshot.
fn entry(link: &Value, project: &str) -> Value {
    let snap = &link["snapshot"];
    let mut e = json!({
        "projectId": project, "host": link["host"], "repository": link["repository"], "number": link["number"],
        "url": link["url"], "provider": provider(field(link, "url")), "state": snap["state"], "isDraft": snap["isDraft"],
        "title": snap["title"], "baseBranch": snap["baseBranch"], "mergeability": snap["mergeability"],
        "additions": snap["additions"], "deletions": snap["deletions"]
    });
    if link["stack"].is_object() {
        e["stack"] = link["stack"].clone();
    }
    e
}

fn open_link(s: &mut App, tx: &mut Tx, url: &str) {
    if tx.open_url(url).is_err() {
        crate::shell::palette::toast_error(s, "Unable to open link", None);
    }
}

fn command(s: &mut App, thread: &str, link: &Value, kind: &str, watching: Option<bool>) {
    s.ui.panel.pull_requests.menu = None;
    let mut c = json!({"type": kind, "threadId": thread, "host": link["host"], "repository": link["repository"], "number": link["number"]});
    if let Some(watching) = watching {
        c["watching"] = json!(watching);
    }
    if let Some(net) = crate::env::net_for(s, thread) {
        net.dispatch(c);
    }
}

/// Starts the avatars the rows show.
pub(crate) fn want_avatars(s: &mut App) {
    if !super::showing_pull_requests(s) {
        return;
    }
    let Some(thread) = thread(s) else { return };
    let actors: Vec<Value> = visible(thread).iter().map(|l| l["snapshot"]["author"].clone()).filter(Value::is_object).collect();
    s.ui.pages.avatars.want(crate::pages::avatars::urls(&actors), s.ui.panel.waker.as_ref());
}

pub fn view(app: &App, t: &T, overlays: &mut Vec<El>) -> El {
    let Some(thread) = thread(app) else { return super::notice(t, "Open a thread to see its linked pull requests.") };
    let Some(config) = app.state().config.as_ref() else {
        return state(t, &i::GIT_PULL_REQUEST_ARROW, "Linked pull requests unavailable", "This environment does not support multiple linked pull requests.", None);
    };
    let id = field(thread, "id").to_string();
    let links = visible(thread);
    if links.is_empty() {
        let id = id.clone();
        let link = kit::Button::new("Link pull request").variant("outline").size("sm").icon(&i::PLUS).on_press(move |s: &mut App| crate::shell::palette::actions::open_link(s, id.clone())).build(t);
        return state(t, &i::LINK, "No linked pull requests", "Pull requests the agent opens from this thread land here. Link one yourself from a URL or a number.", Some(link));
    }
    let watchable = config["environment"]["capabilities"]["threadPullRequestWatch"] == true;
    let mut list = col().key("thread-prs-list").w_full().grow(1.0).min_h(0.0).padding(6.0).scroll(ScrollSpec { stick_bottom: false });
    for line in lines(chains(&links)) {
        let project = project_for(app, thread, line.link);
        list = list.child(link_row(app, t, &id, &line, &project, watchable, overlays));
    }
    let open = links.iter().filter(|l| l["snapshot"].is_null() || l["snapshot"]["state"] == "open").count();
    let synced = links.iter().filter_map(|l| l["snapshot"]["syncedAt"].as_str()).max().and_then(iso_ms);
    let mut summary = format!("{open} open · {} linked", links.len());
    if let Some(at) = synced {
        summary.push_str(&format!(" · synced {}", relative(at, app.now_ms)));
    }
    let footer = row()
        .key("thread-prs-footer")
        .w_full()
        .px(8.0)
        .py(6.0)
        .center_y()
        .child(text(summary).font(theme::TEXT_2XS.features("'tnum' 1")).color(t.c.muted_foreground).truncate().shrink())
        .child(spacer())
        .child(kit::Button::new("Link").variant("ghost").size("xs").icon(&i::PLUS).on_press(move |s: &mut App| crate::shell::palette::actions::open_link(s, id.clone())).build(t));
    col().key("thread-prs").grow(1.0).min_h(0.0).w_full().child(list).child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.6))).child(footer)
}

/// PullRequestsUnavailableState and the empty state: a glyph, a title, a line, maybe a button.
fn state(t: &T, glyph: &'static rune::Icon, title: &str, description: &str, action: Option<El>) -> El {
    col()
        .key("thread-prs-state")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .center()
        .padding(24.0)
        .gap(8.0)
        .child(icon(glyph, 24.0).color(t.c.muted_foreground.alpha_mul(0.6)))
        .child(text(title.to_string()).font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(text(description.to_string()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().align(rune::Align::Center).max_w(240.0))
        .children(action)
}

fn link_row(app: &App, t: &T, thread: &str, line: &Line, project: &str, watchable: bool, overlays: &mut Vec<El>) -> El {
    let link = line.link;
    let snap = &link["snapshot"];
    let key = key_of(link);
    let number = link["number"].to_string();
    let open = snap.is_null() || snap["state"] == "open";
    let watching = link.get("watch").is_some_and(|w| !w.is_null());
    let url = field(link, "url").to_string();
    let kind = provider(&url);
    let e = entry(link, project);
    let muted = t.c.muted_foreground;
    let mono = theme::mono(theme::TEXT_XS).features("'tnum' 1");
    let meta = theme::TEXT_2XS;

    let glyph = if snap.is_null() { el().size(16.0, 16.0).label("Waiting for host state").child(icon(&i::GIT_PULL_REQUEST_ARROW, 16.0).color(muted)) } else { state_mark(t, &e) };
    let linked = link["linkedAt"].as_str().and_then(iso_ms).map(|at| format!(" · {}", relative(at, app.now_ms))).unwrap_or_default();
    let mut line1 = row()
        .w_full()
        .h(20.0)
        .gap(6.0)
        .center_y()
        .child(text(format!("#{number}")).font(mono).color(muted).shrink().help(format!("{}{linked}", source_label(field(link, "source")))))
        .child(text(snap["title"].as_str().unwrap_or(field(link, "repository")).to_string()).font(theme::TEXT_SM).color(t.c.foreground).truncate().shrink().min_w(0.0));
    if open {
        let mut signals = row().gap(4.0).center_y().shrink();
        if watching {
            signals = signals.child(el().label("Watching").help("Watching: the agent wakes when checks finish, someone comments, or the branch conflicts").child(icon(&i::EYE, 14.0).color(muted)));
        }
        if let Some((glyph, ink, label)) = snap["checksState"].as_str().and_then(|st| pr_row::checks_state(t, st)) {
            signals = signals.child(el().label(label).help(label).child(icon(glyph, 14.0).color(ink)));
        }
        line1 = line1.child(signals.children(review_mark(t, field(snap, "reviewDecision"))));
    }
    let (add, del) = (snap["additions"].as_i64().unwrap_or(0), snap["deletions"].as_i64().unwrap_or(0));
    if add > 0 || del > 0 {
        let stat = theme::mono(theme::TEXT_2XS).features("'tnum' 1");
        let counts = row()
            .h(20.0)
            .gap(4.0)
            .center_y()
            .shrink()
            .child(text(format!("+{}", grouped(add))).font(stat).color(t.c.diff_addition_foreground))
            .child(text(format!("-{}", grouped(del))).font(stat).color(t.c.diff_deletion_foreground));
        line1 = line1.child(spacer()).child(if project.is_empty() { counts } else { pr_row::breakdown(app, t, &e, counts, overlays) });
    }

    let mut line2 = row().w_full().h(16.0).gap(6.0).center_y();
    if let Some((native, size)) = line.stack {
        let tip = if native { format!("{} stack of {size}: merging a layer lands the ones below it.", host_label(kind)) } else { format!("{size} {} chained by base branch.", plural(kind)) };
        let ink = t.c.foreground.alpha_mul(0.7);
        line2 = line2.child(row().gap(2.0).center_y().shrink().help(tip).child(icon(&i::LAYERS, 12.0).color(ink)).child(text(size.to_string()).font(meta.features("'tnum' 1")).color(ink)));
    }
    if snap["author"].is_object() {
        let login = snap["author"]["login"].as_str().unwrap_or("ghost").to_string();
        line2 = line2.child(
            row()
                .gap(4.0)
                .center_y()
                .shrink()
                .max_w(112.0)
                .child(crate::pages::avatars::avatar(&app.ui.pages.avatars, &snap["author"], 14.0, t))
                .child(text(login).font(meta).color(muted).truncate().shrink()),
        );
    }
    let mono_meta = theme::mono(meta);
    line2 = if snap.is_null() {
        line2.child(text(format!("{}/{}", field(link, "host"), field(link, "repository"))).font(mono_meta).color(muted).truncate().shrink().min_w(0.0))
    } else {
        let repository = field(link, "repository").to_string();
        line2
            .child(text(repository.clone()).font(mono_meta).color(muted).truncate().shrink().max_w(128.0).help(repository))
            .child(text(format!("{} → {}", field(snap, "headBranch"), field(snap, "baseBranch"))).font(mono_meta).color(muted).truncate().shrink().min_w(0.0))
    };
    if let Some(at) = snap["updatedAt"].as_str().and_then(iso_ms) {
        line2 = line2.child(spacer()).child(text(relative(at, app.now_ms)).font(meta.features("'tnum' 1")).color(muted));
    }

    let press = url.clone();
    let body = col()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .cursor(Cursor::Pointer)
        .role("link")
        .label(url.clone())
        .child(line1)
        .child(line2)
        .on_press_tx(move |s: &mut App, tx: &mut Tx| open_link(s, tx, &press));
    let actionable = !project.is_empty() && !snap.is_null() && snap["state"] != "merged";
    let speed = actionable.then(|| pr_row::speed_actions(app, t, &Value::Null, &e)).flatten();
    let menu = (speed.is_none() || !app.ui.pages.pull_requests.row.speed).then(|| row_menu(app, t, thread, link, &key, &number, open && watchable, watching, overlays));

    let right_click = key.clone();
    let mut out = row()
        .key(format!("thread-pr-{key}"))
        .w_full()
        .pl(8.0 + line.depth.min(3) as f32 * 20.0 - if line.depth > 0 { 8.0 } else { 0.0 })
        .pr(4.0)
        .py(4.0)
        .gap(8.0)
        .center_y()
        .radius(radius::MD)
        .hover(t.c.accent.alpha_mul(0.6))
        .hover_group()
        .on_secondary_press(move |s: &mut App, at: PointerPress, _: &mut Tx| s.ui.panel.pull_requests.menu = Some((right_click.clone(), Some((at.x, at.y)))));
    if line.depth > 0 {
        out = out.child(el().w(1.0).h(24.0).fill(t.c.border.alpha_mul(0.7)));
    }
    out.child(glyph).child(body).children(speed).children(menu)
}

/// The row's "…" menu, also opened at the pointer by a right-click.
#[allow(clippy::too_many_arguments)]
fn row_menu(app: &App, t: &T, thread: &str, link: &Value, key: &str, number: &str, watchable: bool, watching: bool, overlays: &mut Vec<El>) -> El {
    let shown = app.ui.panel.pull_requests.menu.as_ref().filter(|(k, _)| k == key);
    let toggle = key.to_string();
    let trigger = kit::Button::new("")
        .variant("ghost")
        .size("icon-xs")
        .icon(&i::ELLIPSIS)
        .on_press(move |s: &mut App| {
            let menu = &mut s.ui.panel.pull_requests.menu;
            *menu = if menu.as_ref().is_some_and(|(k, _)| *k == toggle) { None } else { Some((toggle.clone(), None)) };
        })
        .build(t)
        .label(format!("Actions for #{number}"));
    let mut menu = Menu::new(format!("thread-pr-menu-{key}"))
        .trigger(trigger)
        .placement(Placement::BottomEnd)
        .width(198.0)
        .native(true)
        .open(shown.is_some())
        .on_close(|s: &mut App| s.ui.panel.pull_requests.menu = None);
    if let Some((_, Some((x, y)))) = shown {
        menu = menu.at(*x, *y);
    }
    let (copy, open) = (field(link, "url").to_string(), field(link, "url").to_string());
    menu = menu
        .item(MenuItem::new("Copy link").icon(&i::LINK).on_select_tx(move |s: &mut App, tx: &mut Tx| {
            tx.copy(&copy);
            s.ui.panel.pull_requests.menu = None;
        }))
        .item(MenuItem::new("Open").icon(&i::ARROW_UP_RIGHT).on_select_tx(move |s: &mut App, tx: &mut Tx| {
            s.ui.panel.pull_requests.menu = None;
            open_link(s, tx, &open);
        }));
    if watchable {
        let (thread, link) = (thread.to_string(), link.clone());
        let label = if watching { "Stop watching" } else { "Watch for changes" };
        menu = menu.item(MenuItem::new(label).icon(if watching { &i::EYE_OFF } else { &i::EYE }).on_select(move |s: &mut App| command(s, &thread, &link, "thread.pull-request.watch", Some(!watching))));
    }
    let label = if link["source"] == "stack" { "Dismiss from thread" } else { "Unlink from thread" };
    let (thread, link) = (thread.to_string(), link.clone());
    menu = menu.item(MenuItem::new(label).icon(&i::UNLINK_2).on_select(move |s: &mut App| command(s, &thread, &link, "thread.pull-request.unlink", None)));
    let built = menu.build(&kit::theme(t), overlays);
    let wrap = el().shrink();
    if shown.is_some() { wrap.child(built) } else { wrap.group_opacity(0.0, 1.0).child(built) }
}

/// A thread with a native stack, a base-branch chain and a lone link (the panel scene).
pub(crate) fn fixture_links() -> Value {
    let snap = |title: &str, head: &str, base: &str, state: &str, at: &str| {
        json!({"state": state, "title": title, "headBranch": head, "baseBranch": base, "isDraft": false, "updatedAt": at, "syncedAt": "2026-11-14T09:58:00.000Z",
            "author": {"login": "octocat"}, "additions": 42, "deletions": 7, "checksState": "passing", "reviewDecision": "approved", "mergeability": "mergeable"})
    };
    let stack = json!({"kind": "native", "id": "s1", "number": 1, "url": "https://github.com/acme/app/stack/1", "base": "main",
        "layers": [{"number": 10, "headBranch": "stack/a", "state": "open"}, {"number": 11, "headBranch": "stack/b", "state": "open"}]});
    let link = |number: u64, source: &str, snapshot: Value, stack: Value| {
        json!({"host": "github.com", "repository": "acme/app", "number": number, "url": format!("https://github.com/acme/app/pull/{number}"),
            "source": source, "linkedAt": "2026-11-14T08:00:00.000Z", "snapshot": snapshot, "stack": stack})
    };
    let mut watched = link(20, "agent", snap("Add the login redirect", "feat/login", "main", "open", "2026-11-14T09:00:00.000Z"), Value::Null);
    watched["watch"] = json!({"startedAt": "2026-11-14T08:30:00.000Z"});
    json!([
        link(11, "stack", snap("Stack layer two", "stack/b", "stack/a", "open", "2026-11-14T07:00:00.000Z"), stack.clone()),
        link(10, "created", snap("Stack layer one", "stack/a", "main", "open", "2026-11-14T06:00:00.000Z"), stack),
        watched,
        link(21, "manual", snap("Redirect tests", "feat/login-tests", "feat/login", "merged", "2026-11-14T08:00:00.000Z"), Value::Null),
        link(30, "agent", Value::Null, Value::Null),
        link(31, "stack-dismissed", Value::Null, Value::Null),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_list_as_stacks_and_base_branch_chains_newest_first() {
        let thread = json!({"pullRequests": fixture_links()});
        let links = visible(&thread);
        assert_eq!(links.len(), 5, "a dismissed stack member stays hidden");
        let shown: Vec<(u64, usize, Option<(bool, usize)>)> = lines(chains(&links)).iter().map(|l| (l.link["number"].as_u64().unwrap(), l.depth, l.stack)).collect();
        assert_eq!(
            shown,
            [(20, 0, Some((false, 2))), (21, 1, None), (30, 0, None), (10, 0, Some((true, 2))), (11, 1, None)],
            "the newest chain leads; a stack follows its layers; a child sits under its base branch"
        );
    }

    #[test]
    fn a_stacks_order_comes_from_the_layers_covering_the_most_members() {
        let member = |number: u64, layers: Value| json!({"host": "github.com", "repository": "a/b", "number": number, "snapshot": null, "stack": {"id": "s", "layers": layers}});
        let stale = json!([{"number": 2}]);
        let full = json!([{"number": 1}, {"number": 2}, {"number": 3}]);
        let links = [member(3, stale.clone()), member(2, stale), member(1, full)];
        let refs: Vec<&Value> = links.iter().collect();
        let order: Vec<u64> = chains(&refs)[0].layers.iter().map(|l| l["number"].as_u64().unwrap()).collect();
        assert_eq!(order, [1, 2, 3]);
    }

    #[test]
    fn a_branch_two_links_share_parents_neither() {
        let link = |number: u64, head: &str, base: &str| json!({"host": "h", "repository": "r", "number": number, "stack": null, "snapshot": {"headBranch": head, "baseBranch": base}});
        let links = [link(1, "x", "main"), link(2, "x", "main"), link(3, "y", "x")];
        let refs: Vec<&Value> = links.iter().collect();
        let sizes: Vec<usize> = chains(&refs).iter().map(|c| c.layers.len()).collect();
        assert_eq!(sizes, [1, 1, 1], "an ambiguous head leaves its child on its own");
        assert_eq!(provider("https://gitlab.com/a/b/-/merge_requests/3"), "gitlab");
        assert_eq!(provider("https://github.com/a/b/pull/3"), "github");
    }

    #[test]
    fn the_scenes_show_the_list_and_the_empty_state_from_every_entry() {
        let mut s = App::new();
        let scene = |name: &str| super::super::SCENES.iter().find(|(n, _)| *n == name).unwrap().1;
        scene("panel-pull-requests")(&mut s);
        assert!(super::super::showing_pull_requests(&s) && available(&s));
        assert!(proactive_key(&s).is_some_and(|k| k.contains("acme/app")));
        s.ui.shell.panel_open = false;
        crate::shell::palette::actions::show_pull_requests(&mut s);
        assert!(super::super::showing_pull_requests(&s), "the palette and the sidebar badge reopen it");
        scene("panel-pull-requests-empty")(&mut s);
        assert!(!available(&s), "the launcher entry is disabled without links");
        assert_eq!(proactive_key(&s), None);
    }
}
