//! ThreadRelationshipsPanel's Lineage section: the thread's parent, forks, context transfers and
//! subagents (workflows open into their phases), its remote tasks and the thread that delegated
//! it, with merge back, stop and disconnect. Side chats are the panel's other section
//! (`side::details_list`).

use std::collections::{HashMap, HashSet};

use rune::{Cursor, El, col, el, icon, row, text};
use serde_json::{Value, json};

use crate::chat::side;
use crate::chat::timeline::subagent::{self as agents, Member};
use crate::theme::{self, AlphaMul, T};
use crate::{App, data, icons, kit};

/// THREAD_LINEAGE_INITIAL_COUNT and THREAD_LINEAGE_PAGE_COUNT.
const FIRST: usize = 6;
const PAGE: usize = 12;
const SETTLED: [&str; 6] = ["completed", "failed", "error", "cancelled", "interrupted", "idle"];

fn array<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key].as_array().map_or(&[], Vec::as_slice)
}

/// isOrchestrationV2WorkActive.
fn active(status: &str) -> bool {
    matches!(status, "pending" | "running" | "waiting")
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Kind {
    Fork,
    Subagent,
    Transfer,
}

#[derive(Clone, PartialEq, Debug)]
struct Edge {
    source: String,
    target: String,
    kind: Kind,
    status: Option<String>,
}

/// deriveThreadRelationshipGraph: each thread by id (`None` when only an edge names it), and the
/// edges between them.
struct Graph<'a> {
    nodes: HashMap<String, Option<&'a Value>>,
    edges: Vec<Edge>,
}

#[derive(Clone, Debug)]
struct Related {
    thread: String,
    edge: Edge,
}

fn opt(v: &Value, key: &str) -> Option<String> {
    v[key].as_str().map(str::to_owned)
}

fn thread_status(t: &Value) -> Option<String> {
    opt(t, "activityRunStatus").or_else(|| opt(t, "status"))
}

/// `shells` from most to least authoritative (live before archived).
fn graph<'a>(shells: impl IntoIterator<Item = &'a Value>, projection: Option<&'a Value>) -> Graph<'a> {
    let mut nodes: HashMap<String, Option<&'a Value>> = HashMap::new();
    let mut order = Vec::new();
    for t in shells {
        let id = data::s(t, "id");
        if !id.is_empty() && !nodes.contains_key(id) {
            nodes.insert(id.to_owned(), Some(t));
            order.push(t);
        }
    }
    let loaded = nodes.clone();
    let mut edges: Vec<Edge> = Vec::new();
    let mut at: HashMap<(String, String, Kind), usize> = HashMap::new();
    let mut add = |nodes: &mut HashMap<String, Option<&'a Value>>, mut edge: Edge| {
        nodes.entry(edge.source.clone()).or_insert(None);
        nodes.entry(edge.target.clone()).or_insert(None);
        if edge.kind == Kind::Subagent && edge.status.as_deref() == Some("rolled_back") {
            edge.status = Some("interrupted".into());
        }
        match at.get(&(edge.source.clone(), edge.target.clone(), edge.kind)) {
            Some(&i) => edges[i] = edge,
            None => {
                at.insert((edge.source.clone(), edge.target.clone(), edge.kind), edges.len());
                edges.push(edge);
            }
        }
    };
    let mut lineage: Vec<(&Value, Option<String>)> = order.iter().map(|t| (*t, thread_status(t))).collect();
    if let Some(selected) = projection.map(|p| &p["thread"]).filter(|t| t.is_object() && !loaded.contains_key(data::s(t, "id"))) {
        lineage.push((selected, None));
    }
    for (t, status) in lineage {
        let parent = side::parent_of(t);
        if parent.is_empty() {
            continue;
        }
        let kind = if t["lineage"]["relationshipToParent"] == "subagent" { Kind::Subagent } else { Kind::Fork };
        add(&mut nodes, Edge { source: parent.to_owned(), target: data::s(t, "id").to_owned(), kind, status });
    }
    if let Some(p) = projection {
        let owner = data::s(&p["thread"], "id");
        for agent in array(p, "subagents") {
            let child_id = data::s(agent, "childThreadId");
            if child_id.is_empty() {
                continue;
            }
            let child = loaded.get(child_id).copied().flatten();
            let status = child.and_then(|c| opt(c, "activityRunStatus")).or_else(|| match child {
                Some(c) if !c["latestRunId"].is_null() && !active(data::s(agent, "status")) => opt(c, "status"),
                _ => opt(agent, "status"),
            });
            add(&mut nodes, Edge { source: owner.to_owned(), target: child_id.to_owned(), kind: Kind::Subagent, status });
        }
        for transfer in array(p, "contextTransfers") {
            let (source, target) = (data::s(transfer, "sourceThreadId"), data::s(transfer, "targetThreadId"));
            if source != target {
                add(&mut nodes, Edge { source: source.to_owned(), target: target.to_owned(), kind: Kind::Transfer, status: opt(transfer, "status") });
            }
        }
    }
    Graph { nodes, edges }
}

impl<'a> Graph<'a> {
    fn thread(&self, id: &str) -> Option<&'a Value> {
        self.nodes.get(id).copied().flatten()
    }

    /// immediateThreadRelationships: each thread one edge from `id`, by its first edge.
    fn immediate(&self, id: &str) -> Vec<Related> {
        let mut seen = HashSet::new();
        let mut rows = Vec::new();
        for edge in &self.edges {
            let other = if edge.source == id {
                &edge.target
            } else if edge.target == id {
                &edge.source
            } else {
                continue;
            };
            if seen.insert(other.clone()) {
                rows.push(Related { thread: other.clone(), edge: edge.clone() });
            }
        }
        rows
    }

    /// threadRelationshipRowStatus: a parent or child's own run, a transfer's own status.
    fn row_status(&self, row: &Related) -> Option<String> {
        if row.edge.kind == Kind::Transfer || row.thread == row.edge.target {
            return row.edge.status.clone();
        }
        self.thread(&row.thread).and_then(thread_status)
    }
}

/// isParentThreadRelationship.
fn is_parent(edge: &Edge, current: &str) -> bool {
    edge.kind != Kind::Transfer && edge.target == current
}

/// orderWebThreadLineageRows: the parent, then the merge-back target, then newest first.
fn order(graph: &Graph, rows: &mut [Related], current: &str, merge_target: Option<&str>) {
    let rank = |r: &Related| {
        if is_parent(&r.edge, current) {
            0
        } else if Some(r.thread.as_str()) == merge_target {
            1
        } else {
            2
        }
    };
    let created = |r: &Related| graph.thread(&r.thread).and_then(|t| data::ts(t, "createdAt"));
    rows.sort_by(|a, b| {
        rank(a).cmp(&rank(b)).then_with(|| match (created(a), created(b)) {
            (Some(x), Some(y)) if x != y => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            _ => a.thread.cmp(&b.thread),
        })
    });
}

/// resolveMergeBackTargetThreadId: where a fork merges back to.
fn merge_target(projection: &Value) -> Option<&str> {
    let thread = &projection["thread"];
    if thread["lineage"]["relationshipToParent"] != "fork" {
        return None;
    }
    Some(side::parent_of(thread)).filter(|id| !id.is_empty())
}

/// relationshipLabel, or "Side chat".
fn relationship(edge: &Edge, current: &str, side_chat: bool) -> &'static str {
    let parent = is_parent(edge, current);
    match edge.kind {
        _ if side_chat && !parent => "Side chat",
        Kind::Transfer => "Context transfer",
        Kind::Subagent if parent => "Parent agent",
        Kind::Subagent => "Subagent",
        Kind::Fork if parent => "Parent thread",
        Kind::Fork => "Fork",
    }
}

/// A child subagent as its row reads it: currentSubagent's view of a settled task whose thread
/// has run again since.
struct Agent<'a> {
    record: &'a Value,
    status: String,
    started: Option<i64>,
    completed: Option<i64>,
    /// Its progress and result, cleared once the thread ran on past them.
    current: bool,
}

impl<'a> Agent<'a> {
    fn new(record: &'a Value, child: Option<&Value>, workflow: bool) -> Self {
        let mut agent = Agent { record, status: data::s(record, "status").to_owned(), started: data::ts(record, "startedAt"), completed: data::ts(record, "completedAt"), current: true };
        let Some(child) = child.filter(|_| !workflow && !active(&agent.status)) else { return agent };
        let live = opt(child, "activityRunStatus").or_else(|| (child["status"] == "queued").then(|| "queued".to_owned()));
        let since = data::ts(record, "completedAt").or_else(|| data::ts(record, "startedAt")).or_else(|| data::ts(record, "updatedAt"));
        let newer = data::ts(child, "latestRunRequestedAt").is_some_and(|at| since.is_none_or(|since| at > since));
        if live.is_none() && !newer {
            return agent;
        }
        let status = live.clone().unwrap_or_else(|| data::s(child, "status").to_owned());
        agent.status = match status.as_str() {
            "preparing" | "starting" | "queued" => "pending".to_owned(),
            "rolled_back" => "interrupted".to_owned(),
            _ => status,
        };
        agent.started = data::ts(child, if live.is_some() { "activityRunStartedAt" } else { "latestRunStartedAt" });
        agent.completed = if live.is_some() { None } else { data::ts(child, "latestRunCompletedAt") };
        agent.current = false;
        agent
    }

    /// deriveSubagentElapsedMs.
    fn elapsed(&self, now: i64) -> Option<i64> {
        let end = if active(&self.status) { Some(now) } else { self.completed };
        Some((end? - self.started?).max(0))
    }

    /// The row's Stop: an app task, or a Claude task the provider can name, while it works.
    fn can_stop(&self) -> bool {
        let r = self.record;
        let stoppable = match data::s(r, "origin") {
            "app_owned" => true,
            "provider_native" => {
                data::s(r, "driver") == "claudeAgent"
                    && !r["runId"].is_null()
                    && r["nativeTaskRef"]["strength"] == "strong"
                    && !r["nativeTaskRef"]["nativeId"].is_null()
                    && active(data::s(r, "status"))
            }
            _ => false,
        };
        stoppable && self.started.is_some() && active(&self.status)
    }
}

/// AgentElapsed: `2m 05s`, or compact `2m`.
fn elapsed_label(ms: i64, compact: bool) -> String {
    let seconds = ms.max(0) / 1000;
    let minutes = seconds / 60;
    if compact {
        return match minutes {
            0 => format!("{seconds}s"),
            1..60 => format!("{minutes}m"),
            600.. => format!("{}h", minutes / 60),
            _ => format!("{}h", (minutes / 6) as f64 / 10.0),
        };
    }
    match (minutes, minutes / 60) {
        (0, _) => format!("{seconds}s"),
        (_, 0) => format!("{minutes}m {:02}s", seconds % 60),
        (_, hours) => format!("{hours}h {:02}m", minutes % 60),
    }
}

/// The section's groups (ThreadRelationshipsPanel's `groupBy`), with what decides its heading.
struct Lineage<'a> {
    graph: Graph<'a>,
    related: Vec<Related>,
    active: Vec<Related>,
    previous: Vec<Related>,
    side_chats: usize,
    running: usize,
    merge_target: Option<String>,
}

/// The rows around `current`: hidden threads left out, side chats counted (their own section
/// lists them), a workflow's row carrying the workflow's status.
fn lineage<'a>(shells: impl IntoIterator<Item = &'a Value>, projection: Option<&'a Value>, current: &str, hidden: &dyn Fn(&str) -> bool, hiding: bool) -> Lineage<'a> {
    let graph = graph(shells, projection);
    let merge_target = projection.and_then(merge_target).map(str::to_owned);
    let mut rows = graph.immediate(current);
    order(&graph, &mut rows, current, merge_target.as_deref());
    let subagents = projection.map(|p| array(p, "subagents")).unwrap_or_default();
    let workflow_of = |thread: &str| subagents.iter().find(|a| data::s(a, "childThreadId") == thread).filter(|a| a["workflow"].is_object());
    let mut out = Lineage { related: vec![], active: vec![], previous: vec![], side_chats: 0, running: 0, merge_target, graph: Graph { nodes: HashMap::new(), edges: vec![] } };
    for mut row in rows {
        let parent = is_parent(&row.edge, current);
        // While hiding, an unloaded parent, fork or transfer may be hidden; unloaded subagents follow this thread.
        if hidden(&row.thread) || (hiding && graph.thread(&row.thread).is_none() && (row.edge.kind != Kind::Subagent || parent)) {
            continue;
        }
        if row.edge.kind == Kind::Subagent && !parent
            && let Some(coordinator) = workflow_of(&row.thread)
        {
            row.edge.status = opt(coordinator, "status");
        }
        if !parent && graph.thread(&row.thread).is_some_and(|t| t["lineage"]["sideChat"] == true) {
            out.side_chats += 1;
        } else if row.edge.kind != Kind::Subagent || parent {
            out.related.push(row);
        } else if SETTLED.contains(&row.edge.status.as_deref().unwrap_or("")) {
            out.previous.push(row);
        } else {
            out.active.push(row);
        }
    }
    let threadless = subagents.iter().filter(|a| a["childThreadId"].is_null() && a["status"] == "running").count();
    out.running = threadless + out.active.iter().filter(|r| r.edge.status.as_deref() == Some("running")).count();
    out.graph = graph;
    out
}

#[derive(Clone, PartialEq)]
enum Action {
    Merge(String),
    Detach,
    Stop,
    StopWorkflow,
}

/// The section's open groups, pages and workflows, for one thread; and the action in flight.
#[derive(Default)]
pub struct State {
    thread: String,
    /// "Previous agents" opened (it starts closed).
    previous_open: bool,
    /// Rows shown per group beyond the first six.
    shown: HashMap<&'static str, usize>,
    workflows: HashSet<String>,
    /// Phases opened or closed against their default (open while running).
    phases: HashMap<(String, u64), bool>,
    busy: Option<(u64, Action)>,
    /// The row whose Stop is in flight (the subagent's child thread, or the workflow's subagent).
    stopping: Option<String>,
}

/// The state for `thread`, starting over when the card shows another thread.
fn state<'a>(s: &'a mut App, thread: &str) -> &'a mut State {
    let state = &mut s.ui.chat.details.lineage;
    if state.thread != thread {
        *state = State { thread: thread.to_owned(), busy: state.busy.take(), stopping: state.stopping.take(), ..State::default() };
    }
    state
}

fn ui<'a>(s: &'a App, thread: &str) -> Option<&'a State> {
    Some(&s.ui.chat.details.lineage).filter(|st| st.thread == thread)
}

fn send(s: &mut App, thread: &str, action: Action, stopping: Option<String>, command: impl FnOnce(&mut s5_net::Net) -> u64) {
    if s.ui.chat.details.lineage.busy.is_some() || !crate::chat::can_operate(s) {
        return;
    }
    let Some(net) = s.src.net() else { return };
    let id = command(net);
    let state = state(s, thread);
    state.busy = Some((id, action));
    state.stopping = stopping;
}

pub fn poll(s: &mut App) -> bool {
    let Some(id) = s.ui.chat.details.lineage.busy.as_ref().map(|b| b.0) else { return false };
    let Some(reply) = s.src.net().and_then(|net| net.take_reply(id)) else { return false };
    let state = &mut s.ui.chat.details.lineage;
    state.stopping = None;
    let Some((_, action)) = state.busy.take() else { return true };
    match (action, reply) {
        (Action::Merge(target), Ok(_)) => crate::shell::sidebar::open_thread(s, &target),
        (action, Err(e)) => {
            let title = match action {
                Action::Merge(_) => "Could not merge back",
                Action::Detach => "Could not disconnect agent session",
                Action::Stop => "Could not stop subagent",
                Action::StopWorkflow => "Could not stop workflow",
            };
            let message = crate::shell::palette::error_message(&e, "An error occurred.");
            crate::shell::palette::toast_error_brief(s, title, Some(&message));
        }
        _ => {}
    }
    true
}

/// stopThreadSession: detaches every provider session of the thread.
fn detach(s: &mut App, thread: &str) {
    s.ui.chat.details.menu = None;
    let sessions: Vec<String> = s.state().thread_views.get(thread).map(|v| array(&v.projection, "providerSessions").iter().map(|x| data::s(x, "id").to_owned()).collect()).unwrap_or_default();
    let id = thread.to_owned();
    send(s, thread, Action::Detach, None, move |net| {
        let command = s5_net::uuid();
        sessions.iter().fold(0, |_, session| {
            net.dispatch(json!({"type": "provider-session.detach", "commandId": format!("{command}:detach:{session}"), "threadId": id, "providerSessionId": session, "reason": "client-requested"}))
        })
    });
}

/// canDetachThreadProviderSession.
fn can_detach(projection: &Value) -> bool {
    let running = array(projection, "runs").iter().rev().find(|r| matches!(data::s(r, "status"), "preparing" | "starting" | "running" | "waiting"));
    crate::chat::timeline::queue::session(projection, running).is_some_and(|x| !matches!(data::s(x, "status"), "stopped" | "error"))
}

/// interruptThreadTurn for a subagent: an app task stops in its parent and its own thread, a
/// provider's through its parent.
fn stop_subagent(s: &mut App, thread: &str, child: &str, record: &Value) {
    let (parent, subagent, run) = (data::s(record, "threadId").to_owned(), data::s(record, "id").to_owned(), record["runId"].clone());
    let app_owned = data::s(record, "origin") == "app_owned";
    let child = child.to_owned();
    send(s, thread, Action::Stop, Some(child.clone()), move |net| {
        let has_run = !run.is_null();
        let stop = json!({"type": "subagent.stop", "threadId": parent, "runId": run, "subagentId": subagent});
        if !app_owned {
            return net.dispatch(stop);
        }
        if has_run {
            // Older servers refuse this for app tasks; the child's own stop still runs.
            net.dispatch(stop);
        }
        net.dispatch(json!({"type": "thread.stop", "threadId": child}))
    });
}

fn stop_workflow(s: &mut App, thread: &str, subagent: &str) {
    let (id, subagent) = (thread.to_owned(), subagent.to_owned());
    send(s, thread, Action::StopWorkflow, Some(subagent.clone()), move |net| net.request("orchestration.stopWorkflow", json!({"threadId": id, "subagentId": subagent})));
}

fn merge(s: &mut App, thread: &str, target: &str) {
    let Some(run) = s.state().thread_views.get(thread).and_then(|v| side::merge_back_run(&v.projection)).map(str::to_owned) else { return };
    let (source, to) = (thread.to_owned(), target.to_owned());
    send(s, thread, Action::Merge(target.to_owned()), None, move |net| {
        net.dispatch(json!({"type": "thread.merge_back", "createdBy": "user", "creationSource": "web", "sourceThreadId": source, "targetThreadId": to, "sourcePoint": {"type": "run", "runId": run}}))
    });
}

/// ThreadRelationshipIcon: the provider's mark or `glyph`, with the status dot at its corner.
fn mark(t: &T, driver: Option<&str>, glyph: &'static rune::Icon, status: Option<&str>) -> El {
    let muted = t.c.muted_foreground;
    let inner = match driver.filter(|d| !d.is_empty()) {
        Some(driver) => crate::chat::composer::provider_icon(t, driver, 16.0).opacity(0.75),
        None => icon(glyph, 16.0).color(muted),
    };
    el().size(16.0, 16.0).center().child(inner).child(el().absolute(12.0, 12.0).size(8.0, 8.0).radius(4.0).fill(side::status_dot(t, status.unwrap_or(""))).border(2.0, t.c.card))
}

/// ThreadDetailsControl `part="row"`: a 32px row naming a thread.
fn control(t: &T, key: String, label: String) -> El {
    row()
        .key(key)
        .h(32.0)
        .pl(11.0)
        .pr(10.0)
        .gap(10.0)
        .center_y()
        .radius(theme::layout::CONTROL_RADIUS)
        .hover(t.c.accent)
        .role("button")
        .label(label)
}

fn title_text(t: &T, title: &str) -> El {
    text(title.to_owned()).font(theme::SM_MEDIUM.lh(16.0)).color(t.c.foreground.alpha_mul(0.85)).truncate().grow(1.0).shrink().min_w(0.0)
}

fn small(t: &T, words: String, failed: bool) -> El {
    text(words).font(theme::TEXT_2XS).color(if failed { t.c.destructive } else { t.c.muted_foreground })
}

/// A hidden-until-hover Stop square.
fn stop_button(t: &T, label: String, help: &str, busy: bool, disabled: bool, on_press: impl Fn(&mut App) + 'static) -> El {
    let mut button = kit::icon_button(&icons::SQUARE, &label).size("icon-xs").disabled(disabled);
    if !disabled {
        button = button.on_press(on_press);
    }
    let built = button.build(t).help(help.to_owned());
    (if busy { built.opacity(0.5) } else { built }).group_opacity(0.0, 1.0)
}

struct Ctx<'a> {
    s: &'a App,
    t: &'a T,
    current: &'a str,
    lineage: &'a Lineage<'a>,
    projection: Option<&'a Value>,
    can_operate: bool,
}

impl Ctx<'_> {
    fn agent_record(&self, thread: &str) -> Option<&Value> {
        self.projection.and_then(|p| array(p, "subagents").iter().find(|a| data::s(a, "childThreadId") == thread))
    }

    fn row(&self, related: &Related) -> El {
        let (s, t, current) = (self.s, self.t, self.current);
        let id = related.thread.clone();
        let edge = &related.edge;
        let node = self.lineage.graph.thread(&id);
        let parent = is_parent(edge, current);
        let subagent = edge.kind == Kind::Subagent;
        let merge_target = self.lineage.merge_target.as_deref() == Some(id.as_str());
        let status = self.lineage.graph.row_status(related);
        let status = status.as_deref();
        let side_chat = node.is_some_and(|n| n["lineage"]["sideChat"] == true);
        let glyph = if parent {
            &icons::CORNER_LEFT_UP
        } else if subagent {
            &icons::BOT
        } else if side_chat {
            &icons::MESSAGES_SQUARE
        } else {
            &icons::GIT_FORK
        };
        let record = (subagent && !parent).then(|| self.agent_record(&id)).flatten();
        let workflow = record.filter(|r| r["workflow"].is_object());
        let agent = record.map(|r| Agent::new(r, node, workflow.is_some()));
        let failed = matches!(status, Some("failed" | "error"));
        let raw_title = workflow
            .map(|w| data::s(&w["workflow"], "name"))
            .filter(|n| !n.is_empty())
            .or_else(|| node.map(|n| data::s(n, "title")).filter(|n| !n.is_empty()))
            .or_else(|| record.map(|r| data::s(r, "title")).filter(|n| !n.is_empty()))
            .unwrap_or(id.as_str());
        let title = if subagent { agents::title(raw_title) } else { raw_title.to_owned() };
        let instance = record.map(|r| data::s(r, "providerInstanceId")).filter(|i| !i.is_empty()).or_else(|| node.map(|n| data::s(n, "providerInstanceId")));
        let provider_driver = instance.and_then(|i| crate::chat::composer::provider(s, i)).map(|p| data::s(p, "driver"));
        let driver = record.map(|r| data::s(r, "driver")).filter(|d| !d.is_empty()).or(provider_driver);
        let can_stop_workflow = self.can_operate && workflow.is_some_and(|w| w["status"] == "running" && data::s(w, "origin") == "provider_native") && driver == Some("claudeAgent");
        let can_stop = workflow.is_none() && agent.as_ref().is_some_and(Agent::can_stop);
        let elapsed = agent.as_ref().and_then(|a| a.elapsed(s.now_ms));
        let hint = format!("Open {} in this chat", relationship(edge, current, side_chat).to_lowercase());
        let help = match &agent {
            Some(a) => {
                let model = data::s(a.record, "model");
                let standing = match elapsed {
                    Some(ms) => format!("{} · {}", side::status_label(&a.status), elapsed_label(ms, false)),
                    None => side::status_label(&a.status).to_owned(),
                };
                let detail = if a.current { [data::s(a.record, "progress"), data::s(a.record, "result")].into_iter().find(|d| !d.is_empty()).unwrap_or("") } else { "" };
                [title.as_str(), model, standing.as_str(), detail].into_iter().filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
            }
            None => format!("{title}\n{hint}"),
        };
        let hides = if workflow.is_some() { can_stop_workflow } else { can_stop };
        let trailing = |el: El| if hides { el.group_opacity(1.0, 0.0) } else { el };
        let open_id = id.clone();
        let mut link = control(t, format!("lineage-row-{id}"), format!("{title} {}", side::status_label(status.unwrap_or(""))))
            .cursor(Cursor::Pointer)
            .help(help)
            .child(mark(t, if subagent && !parent { driver } else { None }, glyph, status))
            .child(title_text(t, &title))
            .on_press(move |s: &mut App| crate::shell::sidebar::open_thread(s, &open_id));
        if agent.is_none() {
            link = link.child(icon(&icons::ARROW_RIGHT, 12.0).color(t.c.muted_foreground).group_opacity(0.0, 1.0));
        }
        let ui = ui(s, current);
        let stopping = ui.and_then(|u| u.stopping.as_deref());
        if let (Some(_), false, true) = (&agent, failed, elapsed.is_some() || can_stop_workflow) {
            link = link.children(elapsed.map(|ms| trailing(small(t, elapsed_label(ms, true), false))));
        } else if !merge_target && workflow.is_none() {
            link = link.child(trailing(small(t, side::status_label(status.unwrap_or("")).to_owned(), failed)));
        }
        let busy = ui.is_some_and(|u| u.busy.is_some());
        if let (Some(w), Some(agent)) = (workflow, &agent) {
            return self.workflow_row(link, w, agent, driver, can_stop_workflow, stopping, busy);
        }
        if merge_target {
            let latest = self.projection.and_then(side::merge_back_run);
            let target_title = (!self.hidden(&id)).then(|| node.map(|n| data::s(n, "title")).filter(|t| !t.is_empty())).flatten();
            let can_merge = latest.is_some() && self.can_operate && !busy;
            let merging = ui.and_then(|u| u.busy.as_ref()).is_some_and(|(_, a)| matches!(a, Action::Merge(_)));
            let tip = match (latest, target_title) {
                (None, _) => "Complete a run in this fork before merging it back".to_owned(),
                (_, Some(name)) => format!("Merge this conversation back into {name}"),
                _ => "Merge this conversation back into its source".to_owned(),
            };
            let label = target_title.map_or_else(|| "Merge back to source conversation".to_owned(), |n| format!("Merge back to {n}"));
            let (current, target) = (current.to_owned(), id.clone());
            let mut merge_button = row()
                .key(format!("lineage-merge-{id}"))
                .w(32.0)
                .h(32.0)
                .center()
                .radius(theme::layout::CONTROL_RADIUS)
                .role("button")
                .label(label)
                .help(tip)
                .child(icon(&icons::GIT_MERGE, 12.0).color(t.c.muted_foreground))
                .opacity(if can_merge && !merging { 1.0 } else { 0.5 });
            if can_merge {
                merge_button = merge_button.hover(t.c.accent).cursor(Cursor::Pointer).on_press(move |s: &mut App| merge(s, &current, &target));
            }
            return row()
                .w_full()
                .h(32.0)
                .center_y()
                .radius(theme::layout::CONTROL_RADIUS)
                .child(link.grow(1.0).shrink().min_w(0.0))
                .child(el().w(1.0).h(16.0).fill(t.c.border.alpha_mul(0.65)))
                .child(merge_button)
                .child(text(side::status_label(status.unwrap_or("")).to_owned()).font(theme::weight(theme::TEXT_2XS, theme::MEDIUM)).color(t.c.muted_foreground).pl(4.0).pr(10.0));
        }
        if let (true, Some(agent)) = (can_stop, &agent) {
            let (current, child, record) = (current.to_owned(), id.clone(), agent.record.clone());
            let stop = stop_button(t, format!("Stop subagent {title}"), "Stop subagent", stopping == Some(id.as_str()), !self.can_operate || stopping.is_some() || busy, move |s: &mut App| {
                stop_subagent(s, &current, &child, &record)
            });
            // Shown on hover, as the row's trailing time fades.
            return row().w_full().center_y().hover_group().child(link.grow(1.0).shrink().min_w(0.0)).child(stop.mr(4.0));
        }
        link.w_full().hover_group()
    }

    fn hidden(&self, id: &str) -> bool {
        let (keys, _) = crate::shell::sidebar::tag_hidden(self.s);
        keys.contains(&crate::shell::sidebar::logic::thread_key(&self.s.ui.env.active, id))
    }

    /// ThreadLineageWorkflowRow: the workflow's link, its Stop, and its phases folded under it.
    #[allow(clippy::too_many_arguments)]
    fn workflow_row(&self, link: El, record: &Value, agent: &Agent, driver: Option<&str>, can_stop: bool, stopping: Option<&str>, busy: bool) -> El {
        let (s, t, current) = (self.s, self.t, self.current);
        let id = data::s(record, "id").to_owned();
        let workflow = &record["workflow"];
        let name = Some(data::s(workflow, "name")).filter(|n| !n.is_empty()).unwrap_or(data::s(record, "title")).to_owned();
        let ui = ui(s, current);
        let expanded = ui.is_some_and(|u| u.workflows.contains(&id));
        let toggle_id = id.clone();
        let current_id = current.to_owned();
        let chevron = row()
            .key(format!("lineage-workflow-toggle-{id}"))
            .w(32.0)
            .h(32.0)
            .center()
            .radius(theme::layout::CONTROL_RADIUS)
            .hover(t.c.accent)
            .cursor(Cursor::Pointer)
            .role("button")
            .expanded(expanded)
            .label(format!("{} {name}", if expanded { "Collapse" } else { "Expand" }))
            .child(icon(if expanded { &icons::CHEVRON_UP } else { &icons::CHEVRON_DOWN }, 14.0).color(t.c.muted_foreground))
            .on_press(move |s: &mut App| {
                let state = state(s, &current_id);
                if !state.workflows.remove(&toggle_id) {
                    state.workflows.insert(toggle_id.clone());
                }
            });
        let mut head = row().w_full().h(32.0).center_y().radius(theme::layout::CONTROL_RADIUS).hover_group().child(link.grow(1.0).shrink().min_w(0.0));
        if can_stop {
            let (current, subagent) = (current.to_owned(), id.clone());
            head = head.child(stop_button(t, format!("Stop workflow {name}"), "Stop entire workflow", stopping == Some(id.as_str()), stopping.is_some() || busy, move |s: &mut App| {
                stop_workflow(s, &current, &subagent)
            }));
        }
        head = head.child(el().w(1.0).h(16.0).fill(t.c.border.alpha_mul(0.65))).child(chevron);
        let mut out = col().key(format!("lineage-workflow-{id}")).w_full().child(head);
        if !expanded {
            return out;
        }
        let (phases, unphased) = agents::group(workflow, &agent.status);
        let phases: Vec<_> = phases.into_iter().filter(|p| !p.members.is_empty()).collect();
        let mut list = col().w_full().pl(26.0);
        for (at, phase) in phases.iter().enumerate() {
            let index = phase.members.first().and_then(Member::phase).unwrap_or(at as u64);
            let running = phase.members.iter().any(|m| active(m.status) || m.status == "idle");
            let settled = phase.members.iter().filter(|m| agents::terminal(m.status)).count();
            let open = ui.and_then(|u| u.phases.get(&(id.clone(), index)).copied()).unwrap_or(running);
            let label = if running {
                "running"
            } else if phase.members.iter().any(|m| m.status == "failed") {
                "failed"
            } else if phase.members.iter().any(|m| matches!(m.status, "cancelled" | "interrupted")) {
                "stopped"
            } else {
                "done"
            };
            let started = phase.members.iter().filter_map(|m| m.agent["startedAt"].as_i64()).min();
            let ended = (!running).then(|| phase.members.iter().filter_map(|m| m.agent["completedAt"].as_i64()).max()).flatten();
            let span = match (started, ended, running) {
                (Some(from), _, true) => Some(s.now_ms - from),
                (Some(from), Some(to), false) => Some(to - from),
                _ => None,
            };
            let n = phase.members.len();
            let (current_id, workflow_id) = (current.to_owned(), id.clone());
            let header = row()
                .key(format!("lineage-phase-{id}-{index}"))
                .w_full()
                .h(28.0)
                .px(8.0)
                .gap(6.0)
                .center_y()
                .radius(theme::layout::CONTROL_RADIUS)
                .hover(t.c.accent)
                .cursor(Cursor::Pointer)
                .role("button")
                .expanded(open)
                .label(format!("{} {} phase, {label}", if open { "Collapse" } else { "Expand" }, phase.title))
                .child(icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 12.0).color(t.c.muted_foreground))
                .child(
                    text(phase.title.clone())
                        .font(theme::weight(theme::TEXT_XS, theme::MEDIUM))
                        .color(if running { t.c.foreground } else { t.c.muted_foreground })
                        .truncate()
                        .grow(1.0)
                        .shrink()
                        .min_w(0.0),
                )
                .children((label == "failed").then(|| small(t, "Failed".into(), true)))
                .child(small(t, format!("{settled}/{n} {}", if n == 1 { "agent" } else { "agents" }), false))
                .children(span.map(|ms| small(t, elapsed_label(ms, false), false)))
                .on_press(move |s: &mut App| {
                    let state = state(s, &current_id);
                    state.phases.insert((workflow_id.clone(), index), !open);
                });
            list = list.child(header);
            if open {
                list = list.children(phase.members.iter().map(|m| self.member_row(m, driver)));
            }
        }
        list = list.children(unphased.iter().map(|m| self.member_row(m, driver)));
        if phases.is_empty() && unphased.is_empty() {
            list = list.child(row().px(10.0).py(6.0).child(text("No agents yet").font(theme::TEXT_2XS).color(t.c.muted_foreground.alpha_mul(0.7))));
        }
        out = out.child(list);
        out
    }

    /// WorkflowMemberRow: opens the member's chat when it has one.
    fn member_row(&self, member: &Member, driver: Option<&str>) -> El {
        let t = self.t;
        let agent = member.agent;
        let title = data::s(agent, "label").to_owned();
        let started = agent["startedAt"].as_i64();
        let end = if active(member.status) { Some(self.s.now_ms) } else { agent["completedAt"].as_i64() };
        let elapsed = started.zip(end).map(|(from, to)| elapsed_label(to - from, false));
        let model = data::s(agent, "model");
        let detail = [data::s(agent, "result"), data::s(agent, "error"), data::s(agent, "progress")].into_iter().find(|d| !d.is_empty()).unwrap_or("");
        let standing = elapsed.as_ref().map_or_else(|| side::status_label(member.status).to_owned(), |e| format!("{} · {e}", side::status_label(member.status)));
        let help = [title.as_str(), model, standing.as_str(), detail].into_iter().filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
        let child = data::s(agent, "childThreadId").to_owned();
        let mut line = control(t, format!("lineage-member-{}", member.index()), format!("Open {title} chat"))
            .w_full()
            .help(help)
            .child(mark(t, driver, &icons::BOT, Some(member.status)))
            .child(text(title).font(theme::TEXT_SM).color(t.c.foreground.alpha_mul(0.8)).truncate().grow(1.0).shrink().min_w(0.0))
            .children(elapsed.map(|e| small(t, e, false)));
        if child.is_empty() {
            line = line.opacity(0.64);
        } else {
            line = line.cursor(Cursor::Pointer).on_press(move |s: &mut App| crate::shell::sidebar::open_thread(s, &child));
        }
        line
    }
}

/// ThreadLineageGroup's list under its rows: "Show N more".
fn more(t: &T, key: &'static str, current: &str, hidden: usize) -> Option<El> {
    if hidden == 0 {
        return None;
    }
    let label = format!("Show {} more", hidden.min(PAGE));
    let faint = t.c.muted_foreground.alpha_mul(0.7);
    let current = current.to_owned();
    Some(
        row()
            .key(format!("lineage-more-{key}"))
            .w_full()
            .h(32.0)
            .pl(11.0)
            .gap(10.0)
            .center_y()
            .radius(theme::layout::CONTROL_RADIUS)
            .hover(t.c.accent)
            .cursor(Cursor::Pointer)
            .role("button")
            .label(label.clone())
            .child(icon(&icons::PLUS, 16.0).color(faint))
            .child(text(label).font(theme::SM_MEDIUM).color(faint))
            .on_press(move |s: &mut App| *state(s, &current).shown.entry(key).or_insert(FIRST) += PAGE),
    )
}

/// The rows a client connected to `environment` opens there: its key, when connected.
fn remote_key(s: &App, environment: &str) -> Option<String> {
    let key = if crate::env::local_id(s).as_deref() == Some(environment) { crate::env::LOCAL.to_owned() } else { environment.to_owned() };
    crate::env::connected(s, &key).then_some(key)
}

fn open_remote(s: &mut App, key: &str, thread: &str) {
    crate::env::adopt(s, thread, key);
    crate::shell::sidebar::open_thread(s, thread);
}

/// RemoteSubagentRow: a task delegated to a linked environment, opening there when connected.
fn remote_row(s: &App, t: &T, agent: &Value) -> El {
    let remote = &agent["remoteChild"];
    let status = data::s(agent, "status");
    let label = data::s(remote, "label");
    let title = agents::title(Some(data::s(agent, "title")).filter(|t| !t.is_empty()).unwrap_or("Subagent"));
    let key = remote_key(s, data::s(remote, "environmentId"));
    let hint = if key.is_some() { format!("Open on {label}") } else { format!("Runs on {label}. Connect to it to open the thread there.") };
    let driver = crate::chat::composer::provider(s, data::s(agent, "providerInstanceId")).map(|p| data::s(p, "driver"));
    let mut line = control(t, format!("lineage-remote-{}", data::s(agent, "id")), title.clone())
        .w_full()
        .help(format!("{title}\n{hint}"))
        .child(mark(t, driver, &icons::BOT, Some(status)))
        .child(title_text(t, &title))
        .child(small(t, format!("on {label} · {}", side::status_label(status)), status == "failed"));
    match key {
        Some(key) => {
            let thread = data::s(remote, "threadId").to_owned();
            line = line.cursor(Cursor::Pointer).on_press(move |s: &mut App| open_remote(s, &key, &thread));
        }
        None => line = line.opacity(0.64),
    }
    line
}

/// DelegatedFromRow: the thread on a linked environment whose agent delegated this one.
fn delegated_row(s: &App, t: &T, shell: &Value) -> Option<El> {
    let from = shell["delegatedFrom"].as_object()?;
    let environment = from.get("environmentId").and_then(Value::as_str).unwrap_or("");
    let parent = from.get("threadId").and_then(Value::as_str).unwrap_or("").to_owned();
    let key = remote_key(s, environment);
    let label = key.as_ref().map(|k| crate::env::label(s, k)).filter(|l| !l.is_empty()).or_else(|| shell["linkOrigin"]["label"].as_str().map(str::to_owned));
    // The title it had at delegation, until this client sees the parent itself.
    let seen = s.state().threads.iter().find(|t| data::s(t, "id") == parent).map(|t| data::s(t, "title")).filter(|t| !t.is_empty());
    let title = seen.or_else(|| from.get("title").and_then(Value::as_str)).unwrap_or("Parent agent").to_owned();
    let hint = match &key {
        Some(_) => "Open parent agent".to_owned(),
        None => format!("Delegated from {}. Connect to it to open the parent thread.", label.as_deref().unwrap_or("a linked environment")),
    };
    let mut line = control(t, "lineage-delegated-from".into(), title.clone())
        .w_full()
        .help(format!("{title}\n{hint}"))
        .child(mark(t, None, &icons::CORNER_LEFT_UP, None))
        .child(title_text(t, &title))
        .children(label.map(|l| small(t, format!("on {l}"), false)));
    match key {
        Some(key) => line = line.cursor(Cursor::Pointer).on_press(move |s: &mut App| open_remote(s, &key, &parent)),
        None => line = line.opacity(0.64),
    }
    Some(line)
}

/// The details card's "Lineage" section for `thread`, or `None` when it has nothing to show.
pub fn view(s: &App, t: &T, thread: &str, divided: bool) -> Option<El> {
    let shell = s.state().threads.iter().chain(&s.state().archived).find(|r| data::s(r, "id") == thread);
    let projection = s.state().thread_views.get(thread).map(|v| &v.projection);
    let (keys, hiding) = crate::shell::sidebar::tag_hidden(s);
    let env = shell.map_or_else(|| s.ui.env.active.clone(), |r| crate::env::of(s, r));
    let hidden = |id: &str| keys.contains(&crate::shell::sidebar::logic::thread_key(&env, id));
    let net_state = s.state();
    let lineage = lineage(net_state.threads.iter().chain(&net_state.archived), projection, thread, &hidden, hiding);
    let remote: Vec<&Value> = projection.map(|p| array(p, "subagents").iter().filter(|a| a["remoteChild"].is_object()).collect()).unwrap_or_default();
    let delegated = shell.and_then(|r| delegated_row(s, t, r));
    let detachable = projection.is_some_and(can_detach);
    let visible = !lineage.related.is_empty()
        || !lineage.active.is_empty()
        || !lineage.previous.is_empty()
        || lineage.running > 0
        || !remote.is_empty()
        || delegated.is_some()
        || (detachable && lineage.side_chats > 0);
    if !visible {
        return None;
    }
    let ctx = Ctx { s, t, current: thread, lineage: &lineage, projection, can_operate: crate::chat::can_operate(s) };
    let ui = ui(s, thread);
    let workflow_open = ui.is_some_and(|u| !u.workflows.is_empty());
    let list = |rows: El| rows.max_h(if workflow_open { 448.0 } else { 216.0 }).scroll(rune::ScrollSpec { stick_bottom: false });
    let mut body = col().key("details-lineage").w_full();
    if let Some(delegated) = delegated {
        body = body.child(delegated);
    }
    for (key, rows) in [("related", &lineage.related), ("active", &lineage.active)] {
        let shown = ui.and_then(|u| u.shown.get(key).copied()).unwrap_or(FIRST).min(rows.len());
        if shown > 0 {
            body = body.child(list(col().w_full().children(rows[..shown].iter().map(|r| ctx.row(r))))).children(more(t, key, thread, rows.len() - shown));
        }
    }
    if !lineage.previous.is_empty() {
        let open = ui.is_some_and(|u| u.previous_open);
        let failed = lineage.previous.iter().filter(|r| matches!(r.edge.status.as_deref(), Some("failed" | "error"))).count();
        let current = thread.to_owned();
        let header = control(t, "lineage-previous".into(), "Previous agents".into())
            .w_full()
            .cursor(Cursor::Pointer)
            .expanded(open)
            .child(icon(&icons::BOT, 16.0).color(t.c.muted_foreground))
            .child(text("Previous agents").font(theme::weight(theme::TEXT_SM, theme::MEDIUM)).color(t.c.foreground.alpha_mul(0.8)).truncate().grow(1.0).shrink().min_w(0.0))
            .children((failed > 0).then(|| small(t, format!("{failed} failed"), true)))
            .children((!open).then(|| small(t, lineage.previous.len().to_string(), false)))
            .child(icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 16.0).color(t.c.muted_foreground))
            .on_press(move |s: &mut App| {
                let state = state(s, &current);
                state.previous_open = !state.previous_open;
            });
        body = body.child(header);
        if open {
            let shown = ui.and_then(|u| u.shown.get("previous").copied()).unwrap_or(FIRST).min(lineage.previous.len());
            body = body.child(list(col().w_full().children(lineage.previous[..shown].iter().map(|r| ctx.row(r))))).children(more(t, "previous", thread, lineage.previous.len() - shown));
        }
    }
    body = body.children(remote.iter().map(|a| remote_row(s, t, a)));
    let title = if lineage.running > 0 { format!("Lineage · {} running", lineage.running) } else { "Lineage".to_owned() };
    // A lone "Previous agents" row names itself; the heading returns for status or actions.
    let heading = lineage.running > 0 || detachable || !lineage.related.is_empty() || !lineage.active.is_empty() || shell.is_some_and(|r| r["delegatedFrom"].is_object());
    if !heading {
        let mut out = col().w_full().px(8.0).pt(8.0).pb(10.0).label(title);
        if divided {
            out = out.child(el().absolute(0.0, 0.0).w_full().h(1.0).fill(t.c.border.alpha_mul(0.65)));
        }
        return Some(out.child(body));
    }
    let actions = detachable.then(|| {
        let busy = s.ui.chat.details.lineage.busy.is_some();
        kit::icon_button(&icons::ELLIPSIS, "More thread actions")
            .size("icon-xs")
            .disabled(busy)
            .on_press(|s: &mut App| super::toggle_menu(s, super::Menu::Lineage))
            .build(t)
            .key("details-lineage-more")
            .expanded(s.ui.chat.details.menu == Some(super::Menu::Lineage))
    });
    Some(super::section(t, &title, actions, divided, body))
}

/// The "More thread actions" menu's rows.
pub fn menu_rows(s: &App, t: &T) -> Vec<El> {
    let crate::Route::Thread(thread) = s.frame_route() else { return vec![] };
    let thread = thread.clone();
    vec![super::menu_row(t, Some(icon(&icons::UNPLUG, 16.0).color(t.c.muted_foreground)), "Disconnect agent session", Some(move |s: &mut App| detach(s, &thread)), None)]
}

/// Scene: a fork of a parent thread, with a context transfer, two subagents (one running),
/// a running workflow opened on its phases, and two finished agents under "Previous agents".
pub fn scene(s: &mut App) {
    let Some(thread) = super::scene_thread(s) else { return };
    let base = data::iso_ms("2026-12-01T00:00:00.000Z").unwrap_or(0);
    s.now_ms = base + 95_000;
    let iso = |ms: i64| {
        let secs = ms / 1000;
        format!("2026-12-01T00:{:02}:{:02}.000Z", secs / 60 % 60, secs % 60)
    };
    let crate::data::Source::Fixture(fixture) = &mut s.src else { return };
    let project = fixture.threads.iter().find(|r| data::s(r, "id") == thread).map(|r| r["projectId"].clone()).unwrap_or_default();
    let shell = |id: &str, title: &str, extra: Value| {
        let mut row = json!({"id": id, "title": title, "projectId": project, "status": "idle", "createdAt": iso(base)});
        if let (Some(row), Some(extra)) = (row.as_object_mut(), extra.as_object()) {
            row.extend(extra.clone());
        }
        row
    };
    fixture.threads.push(shell("scene-parent", "Plan the billing rewrite", json!({"createdAt": "2026-11-30T09:00:00.000Z"})));
    if let Some(row) = fixture.threads.iter_mut().find(|r| data::s(r, "id") == thread) {
        row["lineage"] = json!({"parentThreadId": "scene-parent", "relationshipToParent": "fork"});
    }
    let child = |id: &str, title: &str, status: &str| shell(id, title, json!({"status": status, "lineage": {"parentThreadId": thread, "relationshipToParent": "subagent"}, "creationSource": "provider"}));
    fixture.threads.push(child("scene-sub-tests", "Write the rounding tests", "running"));
    fixture.threads.push(child("scene-sub-docs", "Update the billing docs", "completed"));
    fixture.threads.push(child("scene-sub-lint", "Fix the lint errors", "failed"));
    fixture.threads.push(child("scene-sub-workflow", "billing-audit", "running"));
    fixture.threads.push(shell("scene-transfer", "Notes for the billing review", json!({"createdAt": iso(base + 30_000)})));
    let agents = json!([
        {"index": 0, "label": "Map rounding call sites", "state": "completed", "phaseIndex": 0, "startedAt": base, "completedAt": base + 21_000, "childThreadId": "scene-wf-map"},
        {"index": 1, "label": "Check currency conversions", "state": "completed", "phaseIndex": 0, "startedAt": base + 1_000, "completedAt": base + 34_000},
        {"index": 2, "label": "Patch invoice totals", "state": "running", "phaseIndex": 1, "startedAt": base + 40_000, "childThreadId": "scene-wf-invoice"},
        {"index": 3, "label": "Patch refunds", "state": "failed", "phaseIndex": 1, "startedAt": base + 40_000, "completedAt": base + 52_000}]);
    let subagent = |id: &str, child: &str, status: &str, started: i64, completed: Option<i64>, extra: Value| {
        let mut row = json!({"id": id, "threadId": thread, "childThreadId": child, "origin": "provider_native", "driver": "claudeAgent", "providerInstanceId": "claudeAgent",
            "title": null, "prompt": "", "model": "claude-opus-5-5", "status": status, "runId": "scene-run", "nativeTaskRef": {"strength": "strong", "nativeId": id},
            "startedAt": iso(started), "completedAt": completed.map(iso), "updatedAt": iso(completed.unwrap_or(started)), "result": null});
        if let (Some(row), Some(extra)) = (row.as_object_mut(), extra.as_object()) {
            row.extend(extra.clone());
        }
        row
    };
    let subagents = json!([
        subagent("sa-tests", "scene-sub-tests", "running", base + 50_000, None, json!({"progress": "Covering half-cent amounts"})),
        subagent("sa-docs", "scene-sub-docs", "completed", base, Some(base + 42_000), json!({"result": "Docs updated"})),
        subagent("sa-lint", "scene-sub-lint", "failed", base + 10_000, Some(base + 20_000), json!({})),
        subagent("sa-workflow", "scene-sub-workflow", "running", base, None, json!({"title": "Audit billing for rounding bugs",
            "workflow": {"name": "billing-audit", "phases": [{"index": 0, "title": "Survey"}, {"index": 1, "title": "Fix"}], "agents": agents}})),
    ]);
    if let Some(view) = fixture.thread_views.get_mut(&thread) {
        view.projection["subagents"] = subagents;
        view.projection["contextTransfers"] = json!([{"sourceThreadId": thread, "targetThreadId": "scene-transfer", "status": "consumed"}]);
        view.projection["thread"]["lineage"] = json!({"parentThreadId": "scene-parent", "relationshipToParent": "fork"});
    }
    let ui = state(s, &thread);
    ui.workflows.insert("sa-workflow".into());
    ui.previous_open = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(id: &str, extra: Value) -> Value {
        let mut row = json!({"id": id, "title": id, "status": "idle"});
        if let (Some(row), Some(extra)) = (row.as_object_mut(), extra.as_object()) {
            row.extend(extra.clone());
        }
        row
    }

    fn ids(rows: &[Related]) -> Vec<&str> {
        rows.iter().map(|r| r.thread.as_str()).collect()
    }

    #[test]
    fn rows_group_into_related_active_and_previous_with_the_parent_first() {
        let shells = [
            shell("me", json!({"lineage": {"parentThreadId": "root", "relationshipToParent": "fork"}})),
            shell("root", json!({"createdAt": "2026-01-01T00:00:00Z"})),
            shell("fork-old", json!({"createdAt": "2026-01-02T00:00:00Z", "lineage": {"parentThreadId": "me", "relationshipToParent": "fork"}})),
            shell("fork-new", json!({"createdAt": "2026-01-03T00:00:00Z", "lineage": {"parentThreadId": "me", "relationshipToParent": "fork"}})),
            shell("side", json!({"lineage": {"parentThreadId": "me", "relationshipToParent": "fork", "sideChat": true}})),
            shell("busy", json!({"activityRunStatus": "running", "lineage": {"parentThreadId": "me", "relationshipToParent": "subagent"}})),
            shell("done", json!({"status": "completed", "lineage": {"parentThreadId": "me", "relationshipToParent": "subagent"}})),
            // A settled task whose thread has not run since keeps the task's own status.
            shell("waiting-child", json!({"status": "completed", "latestRunId": null})),
        ];
        let projection = json!({"thread": shells[0].clone(), "subagents": [
            {"id": "a", "childThreadId": "waiting-child", "status": "waiting"},
            {"id": "b", "childThreadId": null, "status": "running"},
        ], "contextTransfers": [{"sourceThreadId": "me", "targetThreadId": "notes", "status": "consumed"}, {"sourceThreadId": "me", "targetThreadId": "me", "status": "consumed"}]});
        let out = lineage(shells.iter(), Some(&projection), "me", &|_: &str| false, false);
        assert_eq!(ids(&out.related), ["root", "fork-new", "fork-old", "notes"], "the parent pins first, then newest first");
        assert_eq!(out.related[0].edge.kind, Kind::Fork);
        assert_eq!(out.merge_target.as_deref(), Some("root"));
        assert_eq!(ids(&out.active), ["busy", "waiting-child"]);
        assert_eq!(ids(&out.previous), ["done"]);
        assert_eq!(out.side_chats, 1);
        assert_eq!(out.running, 2, "a running task without a thread counts too");
        // While tags hide threads, an unloaded fork or transfer may be one; an unloaded subagent is not.
        let hiding = lineage(shells.iter(), Some(&projection), "me", &|id: &str| id == "fork-old", true);
        assert_eq!(ids(&hiding.related), ["root", "fork-new"]);
    }

    #[test]
    fn a_settled_task_reads_its_threads_newer_run() {
        let record = json!({"id": "a", "status": "completed", "origin": "app_owned", "startedAt": "2026-01-01T00:00:00Z", "completedAt": "2026-01-01T00:01:00Z"});
        let rerun = json!({"activityRunStatus": "starting", "activityRunStartedAt": "2026-01-01T00:05:00Z"});
        let agent = Agent::new(&record, Some(&rerun), false);
        assert_eq!(agent.status, "pending");
        assert!(agent.can_stop(), "an app task that runs again can be stopped");
        assert_eq!(agent.elapsed(data::iso_ms("2026-01-01T00:05:30Z").unwrap()), Some(30_000));
        let quiet = json!({"status": "completed", "latestRunRequestedAt": "2026-01-01T00:00:30Z"});
        assert_eq!(Agent::new(&record, Some(&quiet), false).status, "completed", "a request before it finished is its own run");
        assert_eq!(elapsed_label(125_000, false), "2m 05s");
        assert_eq!(elapsed_label(125_000, true), "2m");
        assert_eq!(elapsed_label(90 * 60_000, true), "1.5h");
    }
}
