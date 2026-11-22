//! A delegated task in the timeline (V2LifecycleRow's SubagentTimelineLink): the agent's provider,
//! title, latest progress or result, status and elapsed time; it opens the child thread.

use super::rows;
use crate::data::{s, ts};
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, icons};
use rune::{Color, Cursor, El, col, el, icon, row, text};
use serde_json::Value;

/// formatSubagentDisplayTitle: drops "Subagent:" and names `/root/.../task_name` paths.
pub fn title(raw: &str) -> String {
    let raw = raw.strip_prefix("Subagent:").or_else(|| raw.strip_prefix("subagent:")).map_or(raw, str::trim_start);
    let Some(path) = raw.strip_prefix("/root/") else { return raw.to_owned() };
    let Some(last) = path.trim_end_matches('/').rsplit('/').next().filter(|l| !l.is_empty()) else { return raw.to_owned() };
    let words: Vec<String> = last
        .split(|c: char| c == '_' || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        })
        .collect();
    if words.is_empty() { raw.to_owned() } else { words.join(" ") }
}

fn status(status: &str, t: &T) -> (&'static str, Color) {
    match status {
        "pending" => ("Queued", t.c.info),
        "running" => ("Running", t.c.info),
        "waiting" => ("Waiting", t.c.info),
        "idle" => ("Idle · resumable", t.c.muted_foreground.alpha_mul(0.5)),
        "completed" => ("Completed", t.c.success),
        "failed" => ("Failed", t.c.destructive),
        _ => ("Stopped", t.c.muted_foreground.alpha_mul(0.6)),
    }
}

/// The task's live record (projection.subagents), falling back to the turn item.
fn live<'a>(app: &'a App, item: &'a Value) -> &'a Value {
    let crate::Route::Thread(id) = app.frame_route() else { return item };
    app.state()
        .thread_views
        .get(id)
        .and_then(|v| rows::array(&v.projection, "subagents").iter().find(|a| s(a, "id") == s(item, "subagentId")))
        .unwrap_or(item)
}

pub fn view(app: &App, item: &Value, t: &T) -> El {
    link(app, live(app, item), item, s(item, "id"), s(item, "childThreadId"), None, t)
}

/// What a subagent's notice reports in place of the task's live status: the outcome's status
/// (none for an update), its word, and the notice's time.
struct Event {
    status: Option<&'static str>,
    label: &'static str,
    time: String,
}

/// NOTIFICATION_OUTCOME_STATUS and NOTIFICATION_OUTCOME_LABEL; None once the task has run
/// again since the notice (subagentResumedSince), when its live status says more.
fn outcome(outcome: &str, agent: &Value, notified: Option<i64>) -> Option<(Option<&'static str>, &'static str)> {
    let (status, label) = match outcome {
        "completed" => (Some("completed"), "Finished"),
        "failed" => (Some("failed"), "Failed"),
        "cancelled" => (Some("cancelled"), "Stopped"),
        "updated" => (None, "Updated"),
        _ => (None, "Finished"),
    };
    let resumed = status.is_some()
        && (matches!(s(agent, "status"), "pending" | "running" | "waiting")
            || ts(agent, "startedAt").zip(notified).is_some_and(|(started, at)| started > at));
    (!resumed).then_some((status, label))
}

/// The task a subagent's or delegated task's notice reports on: by its task ids, else by the
/// child thread (SubagentNotificationLink).
fn notified_task<'a>(subagents: &'a [Value], source: &Value) -> Option<&'a Value> {
    let child = s(source, "childThreadId");
    let tasks = source["taskIds"].as_array().filter(|_| s(source, "kind") == "delegated_task");
    subagents.iter().find(|agent| match tasks {
        Some(ids) => ids.iter().any(|id| id.as_str() == Some(s(agent, "id"))),
        None => !child.is_empty() && s(agent, "childThreadId") == child,
    })
}

/// "9:00 AM", or "09:00" under the 24-hour setting (formatShortTimestamp).
fn short_time(app: &App, at: i64) -> String {
    let minutes = crate::data::Zone::Local.wall(at).rem_euclid(86_400_000) / 60_000;
    let (h, m) = (minutes / 60, minutes % 60);
    if super::actions::hour12(app) {
        format!("{}:{m:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" })
    } else {
        format!("{h:02}:{m:02}")
    }
}

/// SubagentNotificationLink: a subagent's or delegated task's notice as that task's card,
/// naming the outcome and when it came; None while the task isn't in the projection.
pub fn notification(app: &App, item: &Value, t: &T) -> Option<El> {
    let source = &item["source"];
    let child = s(source, "childThreadId");
    if child.is_empty() || !matches!(s(source, "kind"), "subagent" | "delegated_task") {
        return None;
    }
    let crate::Route::Thread(thread) = app.frame_route() else { return None };
    let projection = &app.state().thread_views.get(thread)?.projection;
    let agent = notified_task(rows::array(projection, "subagents"), source)?;
    let notified = ts(item, "createdAt");
    let event = outcome(s(item, "outcome"), agent, notified).map(|(status, label)| Event {
        status,
        label,
        time: notified.map(|at| short_time(app, at)).unwrap_or_default(),
    });
    Some(link(app, agent, agent, s(item, "id"), child, event, t))
}

/// EnvironmentSubagentAvatar: the provider's mark with a dot for `state`.
fn avatar(driver: &str, state: Option<&str>, t: &T) -> El {
    el().size(24.0, 24.0)
        .center()
        .radius(radius::FULL)
        .fill(t.c.muted.alpha_mul(0.6))
        .child(crate::chat::composer::provider_icon(t, driver, 14.0))
        .children(state.map(|state| el().absolute(17.0, 17.0).size(7.0, 7.0).radius(radius::FULL).fill(status(state, t).1).border(1.0, t.c.background)))
}

/// SubagentTimelineLink: `agent` is the task's live record, `item` what the timeline holds,
/// `id` the timeline item it shows for.
fn link(app: &App, agent: &Value, item: &Value, id: &str, child: &str, event: Option<Event>, t: &T) -> El {
    let live_state = if s(agent, "status").is_empty() { s(item, "status") } else { s(agent, "status") };
    let state = match &event {
        Some(event) => event.status,
        None => Some(live_state),
    };
    let label = event.as_ref().map_or_else(|| status(live_state, t).0, |e| e.label);
    let settled = matches!(state.unwrap_or(live_state), "completed" | "failed" | "cancelled" | "interrupted");
    let pick = |k: &str| [agent, item].into_iter().map(|v| s(v, k).trim()).find(|v| !v.is_empty()).unwrap_or("");
    let (result, progress) = (pick("result"), pick("progress"));
    let raw = if settled { if result.is_empty() { progress } else { result } } else if progress.is_empty() { result } else { progress };
    let detail = (!raw.is_empty() && !raw.to_ascii_lowercase().starts_with("child task ended with status")).then(|| raw.replace('`', "").lines().next().unwrap_or("").to_owned());
    let failed = state == Some("failed");
    let time = match &event {
        Some(event) => event.time.clone(),
        None => {
            let started = ts(agent, "startedAt").or_else(|| ts(item, "startedAt"));
            let ended = ts(agent, "completedAt").or_else(|| ts(item, "completedAt"));
            started.map(|from| rows::duration(from, if settled { ended.unwrap_or(from) } else { app.now_ms })).unwrap_or_default()
        }
    };
    let driver = if s(agent, "driver").is_empty() { s(item, "driver") } else { s(agent, "driver") };
    let avatar = avatar(driver, state, t);
    let tone = if failed { t.c.destructive } else { t.c.muted_foreground };
    let name = title(if s(item, "title").is_empty() { "Subagent" } else { s(item, "title") });
    // A task running in a linked environment names it.
    let remote = [item, agent].into_iter().find(|v| v["remoteChild"].is_object()).map(|v| format!("on {}", s(&v["remoteChild"], "label")));
    let target = target(app, agent, item, child);
    let mut body = row()
        .key(format!("subagent-{id}"))
        .w_full()
        .min_w(0.0)
        .px(8.0)
        .py(6.0)
        .gap(10.0)
        .center_y()
        .radius(radius::MD)
        .label(format!("Open {name}"))
        .child(avatar)
        .child(
            col()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .child(
                    row()
                        .w_full()
                        .min_w(0.0)
                        .gap(8.0)
                        .center_y()
                        .child(text(name.clone()).font(theme::weight(theme::TEXT_XS, 500.0)).color(t.c.foreground).truncate().shrink().min_w(0.0))
                        .children(remote.map(|on| text(on).font(theme::TEXT_XS).color(t.c.muted_foreground))),
                )
                .child(text(detail.clone().unwrap_or_else(|| label.to_owned())).font(theme::TEXT_2XS).color(tone).truncate()),
        );
    if detail.is_some() && (event.is_some() || state != Some("completed")) {
        body = body.child(text(label).font(theme::TEXT_XS).color(tone));
    }
    body = body.child(text(time.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground));
    if let Some((key, thread)) = target {
        body = body
            .child(icon(&icons::CHEVRON_RIGHT, 14.0).color(t.c.muted_foreground.alpha_mul(0.6)))
            .role("button")
            .focusable(true)
            .cursor(Cursor::Pointer)
            .hover(t.c.accent.alpha_mul(0.2))
            .on_press(move |app: &mut App| open(app, key.as_deref(), &thread));
    }
    // SubagentTooltipContent's facts: the task, its model, and how it stands.
    let standing = if time.is_empty() { label.to_owned() } else { format!("{label} · {time}") };
    body.help([name.as_str(), pick("model"), standing.as_str()].into_iter().filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n"))
}

/// The thread a task's row opens, with the environment to open it on: a task in a linked
/// environment opens its thread there while this client is connected to it, else not at all.
fn target(app: &App, agent: &Value, item: &Value, child: &str) -> Option<(Option<String>, String)> {
    let Some(remote) = [item, agent].into_iter().map(|v| &v["remoteChild"]).find(|r| r.is_object()) else {
        return (!child.is_empty()).then(|| (None, child.to_owned()));
    };
    let environment = s(remote, "environmentId");
    let key = if crate::env::local_id(app).as_deref() == Some(environment) { crate::env::LOCAL.to_owned() } else { environment.to_owned() };
    let thread = s(remote, "threadId");
    (!thread.is_empty() && crate::env::connected(app, &key)).then(|| (Some(key), thread.to_owned()))
}

fn open(app: &mut App, key: Option<&str>, thread: &str) {
    if let Some(key) = key {
        crate::env::adopt(app, thread, key);
    }
    crate::shell::sidebar::open_thread(app, thread);
}

fn terminal(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled" | "interrupted")
}

/// WORKFLOW_SCRIPT_START: optional leading comments, then `export const meta`.
fn is_script_text(text: &str) -> bool {
    let mut rest = text.trim_start();
    loop {
        if let Some(after) = rest.strip_prefix("//") {
            rest = after.split_once('\n').map_or("", |(_, r)| r).trim_start();
        } else if let Some(after) = rest.strip_prefix("/*") {
            let Some((_, r)) = after.split_once("*/") else { return false };
            rest = r.trim_start();
        } else {
            break;
        }
    }
    fn word<'a>(rest: &'a str, w: &str) -> Option<&'a str> {
        rest.strip_prefix(w).filter(|r| r.starts_with(char::is_whitespace)).map(str::trim_start)
    }
    let Some(rest) = word(rest, "export").and_then(|r| word(r, "const")) else { return false };
    rest.strip_prefix("meta").is_some_and(|r| !r.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
}

/// isWorkflowScriptMessage: the server's mark, else the text for threads recorded before it.
fn is_script_message(message: &Value) -> bool {
    s(message, "role") == "user" && message["workflowScript"].as_bool().unwrap_or_else(|| is_script_text(s(message, "text")))
}

/// WorkflowOpening: a workflow coordinator thread's first user message is the script; the
/// coordinator row (on the parent thread) names the workflow and lists its agents.
pub(super) struct Opening<'a> {
    message_id: &'a str,
    script: &'a str,
    name: Option<&'a str>,
    description: Option<&'a str>,
    driver: &'a str,
    /// The coordinator's status and `workflow`, null until the parent thread is loaded.
    status: &'a str,
    completed: Option<i64>,
    workflow: Option<&'a Value>,
}

impl Opening<'_> {
    /// Whether timeline user row `item` is the script this opening replaces.
    pub(super) fn opens(&self, item: &Value) -> bool {
        let id = if s(item, "messageId").is_empty() { s(item, "id") } else { s(item, "messageId") };
        id == self.message_id
    }
}

/// resolveWorkflowOpening for `thread` (its shell row), its `messages`, and the parent
/// projection's subagents when loaded.
fn resolve<'a>(thread: &Value, thread_id: &str, messages: &'a [Value], parent: Option<&'a [Value]>) -> Option<Opening<'a>> {
    if !crate::chat::subagent_bar::native(thread) {
        return None;
    }
    let first = messages.iter().find(|m| s(m, "role") == "user")?;
    let coordinator = parent.and_then(|list| list.iter().find(|a| s(a, "childThreadId") == thread_id));
    if first["workflowScript"] == false {
        return None;
    }
    let workflow = coordinator.map(|c| &c["workflow"]).filter(|w| w.is_object());
    if workflow.is_none() && !is_script_message(first) {
        return None;
    }
    let field = |key: &str| coordinator.map(|c| s(c, key)).filter(|v| !v.is_empty());
    Some(Opening {
        message_id: s(first, "id"),
        script: s(first, "text"),
        name: workflow.map(|w| s(w, "name")).filter(|n| !n.is_empty()),
        description: field("title"),
        driver: field("driver").unwrap_or(""),
        status: field("status").unwrap_or(""),
        completed: coordinator.and_then(|c| ts(c, "completedAt")),
        workflow,
    })
}

/// The routed thread's workflow opening, if it opens with a workflow script.
pub(super) fn opening<'a>(app: &'a App, thread_id: &str) -> Option<Opening<'a>> {
    let state = app.state();
    let p = &state.thread_views.get(thread_id)?.projection;
    let thread = state.threads.iter().find(|r| s(r, "id") == thread_id).unwrap_or(&p["thread"]);
    let parent = s(&thread["lineage"], "parentThreadId");
    let parent = state.thread_views.get(parent).map(|v| rows::array(&v.projection, "subagents"));
    resolve(thread, thread_id, rows::array(p, "messages"), parent)
}

/// workflowOpeningParentThreadId: loads the parent whose coordinator row describes the routed
/// thread's workflow, so ordinary subagent threads never load their parent.
pub fn poll(app: &mut App) -> bool {
    let crate::Route::Thread(id) = app.frame_route() else { return false };
    let state = app.state();
    let Some(view) = state.thread_views.get(id) else { return false };
    let Some(thread) = state.threads.iter().find(|r| s(r, "id") == id) else { return false };
    let opens = crate::chat::subagent_bar::native(thread)
        && rows::array(&view.projection, "messages").iter().find(|m| s(m, "role") == "user").is_some_and(is_script_message);
    let parent = s(&thread["lineage"], "parentThreadId");
    if !opens || parent.is_empty() || state.thread_views.contains_key(parent) {
        return false;
    }
    let parent = parent.to_owned();
    if let Some(net) = app.src.net() {
        net.watch_thread(&parent);
    }
    false
}

/// A workflow agent as a member row reads it (workflowMembersToRuntime).
struct Member<'a> {
    agent: &'a Value,
    status: &'a str,
}

impl<'a> Member<'a> {
    fn new(agent: &'a Value, coordinator: &'a str) -> Self {
        let state = s(agent, "state");
        let status = match state {
            "queued" | "running" if terminal(coordinator) => coordinator,
            "queued" => "pending",
            _ => state,
        };
        Member { agent, status }
    }
    fn index(&self) -> u64 {
        self.agent["index"].as_u64().unwrap_or(0)
    }
    fn phase(&self) -> Option<u64> {
        self.agent["phaseIndex"].as_u64()
    }
}

struct Phase<'a> {
    title: String,
    members: Vec<Member<'a>>,
}

/// deriveWorkflowGroups for one coordinator: phases by index, titled by the declared plan over
/// what members report, then the members outside any phase.
fn group<'a>(workflow: &'a Value, coordinator: &'a str) -> (Vec<Phase<'a>>, Vec<Member<'a>>) {
    let mut members: Vec<Member> = rows::array(workflow, "agents").iter().map(|a| Member::new(a, coordinator)).collect();
    members.sort_by_key(Member::index);
    let mut titles: std::collections::BTreeMap<u64, String> = std::collections::BTreeMap::new();
    for member in &members {
        let Some(index) = member.phase() else { continue };
        let reported = s(member.agent, "phaseTitle");
        let title = if reported.is_empty() { titles.get(&index).cloned().unwrap_or_else(|| format!("Phase {}", index + 1)) } else { reported.to_owned() };
        titles.insert(index, title);
    }
    for phase in rows::array(workflow, "phases") {
        if let Some(index) = phase["index"].as_u64() {
            titles.insert(index, s(phase, "title").to_owned());
        }
    }
    let mut phases: Vec<(u64, Phase)> = titles.into_iter().map(|(index, title)| (index, Phase { title, members: Vec::new() })).collect();
    let mut unphased = Vec::new();
    for member in members {
        match member.phase().and_then(|i| phases.iter().position(|(at, _)| *at == i)) {
            Some(at) => phases[at].1.members.push(member),
            None => unphased.push(member),
        }
    }
    (phases.into_iter().map(|(_, phase)| phase).collect(), unphased)
}

/// workflowOpeningHeadline: the title, a description when it says more, and how the agents stand.
fn headline(opening: &Opening, statuses: &[&str]) -> (String, Option<String>, Option<String>) {
    let title = opening.name.or(opening.description).unwrap_or("Workflow").to_owned();
    let description = opening.description.filter(|d| *d != title).map(str::to_owned);
    let mut counts = [0usize; 4];
    for status in statuses {
        counts[match *status {
            "completed" => 1,
            "failed" => 2,
            other if terminal(other) => 3,
            _ => 0,
        }] += 1;
    }
    let agents = (!statuses.is_empty()).then(|| {
        let parts = ["working", "done", "failed", "stopped"].iter().zip(counts).filter(|(_, n)| *n > 0).map(|(k, n)| format!("{n} {k}"));
        let noun = if statuses.len() == 1 { "agent" } else { "agents" };
        std::iter::once(format!("{} {noun}", statuses.len())).chain(parts).collect::<Vec<_>>().join(" · ")
    });
    (title, description, agents)
}

/// scriptMarkdown: the script in a javascript fence longer than any backtick run inside it.
fn script_markdown(script: &str) -> String {
    let longest = script.split(|c: char| c != '`').map(str::len).max().unwrap_or(0).max(2);
    let fence = "`".repeat(longest + 1);
    format!("{fence}javascript\n{}\n{fence}", script.trim_end())
}

/// WorkflowMemberRow: the agent, what it does or did, its time; opens its thread when it has one.
fn member_row(app: &App, driver: &str, member: &Member, coordinator_completed: Option<i64>, t: &T) -> El {
    let agent = member.agent;
    let name = s(agent, "label").to_owned();
    let settled = terminal(member.status);
    let prompt = s(agent, "prompt");
    let detail = if settled { Some(s(agent, "result")).filter(|r| !r.is_empty()).unwrap_or(prompt) } else { prompt };
    let detail = detail.replace('`', "").split_whitespace().collect::<Vec<_>>().join(" ");
    let started = agent["startedAt"].as_i64();
    let ended = agent["completedAt"].as_i64().or_else(|| started.zip(agent["durationMs"].as_i64()).map(|(a, d)| a + d)).or(coordinator_completed);
    let elapsed = started.map(|from| rows::duration(from, if settled { ended.unwrap_or(from) } else { app.now_ms }));
    let tone = if member.status == "failed" { t.c.destructive } else { t.c.muted_foreground };
    let mut body = row()
        .key(format!("workflow-member-{}", member.index()))
        .w_full()
        .min_w(0.0)
        .px(8.0)
        .py(6.0)
        .gap(10.0)
        .center_y()
        .radius(radius::MD)
        .child(avatar(driver, Some(member.status), t))
        .child(
            col()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .child(text(name.clone()).font(theme::XS_MEDIUM).color(t.c.foreground).truncate())
                .children((!detail.is_empty()).then(|| text(detail).font(theme::TEXT_2XS).color(tone).truncate())),
        )
        .children(elapsed.map(|e| text(e).font(theme::TEXT_XS).color(t.c.muted_foreground)));
    let child = s(agent, "childThreadId").to_owned();
    if !child.is_empty() {
        body = body
            .child(icon(&icons::CHEVRON_RIGHT, 14.0).color(t.c.muted_foreground.alpha_mul(0.6)))
            .role("button")
            .label(format!("Open {name}"))
            .focusable(true)
            .cursor(Cursor::Pointer)
            .hover(t.c.accent.alpha_mul(0.2))
            .on_press(move |app: &mut App| open(app, None, &child));
    }
    body.help(status(member.status, t).0)
}

/// WorkflowOpeningBlock: "Workflow started", the workflow and its agents by phase, then the
/// script it runs, folded when long.
pub(super) fn opening_view(app: &App, opening: &Opening, t: &T) -> El {
    let (phases, unphased) = opening.workflow.map(|w| group(w, opening.status)).unwrap_or_default();
    let statuses: Vec<&str> = phases.iter().flat_map(|p| &p.members).chain(&unphased).map(|m| m.status).collect();
    let (title, description, agents) = headline(opening, &statuses);
    let header = row()
        .w_full()
        .min_w(0.0)
        .gap(8.0)
        .child(icon(&icons::WORKFLOW, 14.0).color(t.c.muted_foreground).mt(2.0))
        .child(
            col()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .gap(2.0)
                .child(text("Workflow started").font(theme::weight(theme::TEXT_2XS, theme::MEDIUM)).color(t.c.muted_foreground))
                .child(text(title.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate().selectable(true))
                .children(description.map(|d| text(d).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().selectable(true))),
        )
        .children(agents.map(|a| text(a).font(theme::TEXT_2XS).color(t.c.muted_foreground)));
    let mut block = col().w_full().min_w(0.0).gap(12.0).px(4.0).py(2.0).label(format!("Workflow started: {title}")).child(header);
    for phase in phases.iter().filter(|p| !p.members.is_empty()) {
        let settled = phase.members.iter().filter(|m| terminal(m.status)).count();
        let mut section = col().w_full().label(format!("{} phase", phase.title)).child(
            row()
                .w_full()
                .min_w(0.0)
                .gap(8.0)
                .px(8.0)
                .pb(2.0)
                .child(text(phase.title.clone()).font(theme::weight(theme::TEXT_2XS, theme::MEDIUM)).color(t.c.muted_foreground).truncate().shrink().min_w(0.0))
                .child(text(format!("{settled}/{}", phase.members.len())).font(theme::TEXT_2XS).color(t.c.muted_foreground)),
        );
        for member in &phase.members {
            section = section.child(member_row(app, opening.driver, member, opening.completed, t));
        }
        block = block.child(section);
    }
    if !unphased.is_empty() {
        block = block.child(col().w_full().children(unphased.iter().map(|m| member_row(app, opening.driver, m, opening.completed, t))));
    }
    let fold_key = format!("workflow-script:{}", opening.message_id);
    let foldable = super::collapses(opening.script);
    let expanded = app.ui.chat.timeline.expanded.contains(&fold_key);
    let script = super::markdown::view(app, &script_markdown(opening.script), &format!("workflow-script-{}", opening.message_id), t);
    // max-h-44 with a 1.75rem fade, as the user message fold.
    let script = if foldable && !expanded { col().w_full().max_h(176.0).clip_fade(0.0, 0.0, 0.0, 28.0).child(script) } else { col().w_full().child(script) };
    let mut section = col().w_full().child(script);
    if foldable {
        section = section.child(
            row().child(
                crate::kit::button::Button::new(if expanded { "Show less" } else { "Show full script" })
                    .variant("ghost-muted")
                    .size("xs")
                    .on_press(move |app: &mut App| super::toggle(app, &fold_key))
                    .build(t)
                    .expanded(expanded),
            ),
        );
    }
    block.child(section)
}

/// summarizeSubagentStatuses: "2 working · 1 done".
fn status_summary(statuses: &[&str]) -> String {
    let mut counts = [0usize; 5];
    for status in statuses {
        counts[match *status {
            "pending" | "running" | "waiting" => 0,
            "completed" => 1,
            "failed" => 2,
            "idle" => 4,
            _ => 3,
        }] += 1;
    }
    ["working", "done", "failed", "stopped", "idle"]
        .iter()
        .zip(counts)
        .filter(|(_, n)| *n > 0)
        .map(|(name, n)| format!("{n} {name}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// V2SubagentGroup: adjacent tasks of one turn behind "N subagents", up to three avatars, a
/// count by status and the group's time; open, each task's own row in a panel.
pub fn group_view(app: &App, id: &str, items: &[&Value], t: &T) -> El {
    let key = format!("subagent-group:{id}");
    let expanded = app.ui.chat.timeline.expanded.contains(&key);
    let mut statuses = Vec::with_capacity(items.len());
    let (mut start, mut end, mut open_end) = (None::<i64>, None::<i64>, false);
    let mut faces = row().center_y();
    for (index, &item) in items.iter().enumerate() {
        let agent = live(app, item);
        statuses.push(if s(agent, "status").is_empty() { s(item, "status") } else { s(agent, "status") });
        // subagentGroupTiming: the earliest start to the latest end, unknown while any is open.
        if let Some(at) = ts(agent, "startedAt").or_else(|| ts(item, "startedAt")) {
            start = Some(start.map_or(at, |first| first.min(at)));
        }
        match ts(agent, "completedAt").or_else(|| ts(item, "completedAt")) {
            Some(at) => end = Some(end.map_or(at, |last| last.max(at))),
            None => open_end = true,
        }
        if index < 3 {
            let driver = if s(agent, "driver").is_empty() { s(item, "driver") } else { s(agent, "driver") };
            let face = el()
                .size(24.0, 24.0)
                .center()
                .radius(radius::FULL)
                .fill(t.c.muted)
                .border(2.0, t.c.background)
                .child(crate::chat::composer::provider_icon(t, driver, 14.0));
            faces = faces.child(if index == 0 { face } else { face.ml(-6.0) });
        }
    }
    if items.len() > 3 {
        faces = faces.child(
            el().size(24.0, 24.0)
                .center()
                .radius(radius::FULL)
                .fill(t.c.muted)
                .border(2.0, t.c.background)
                .ml(-6.0)
                .child(text(format!("+{}", items.len() - 3)).font(theme::weight(theme::TEXT_3XS, theme::MEDIUM)).color(t.c.muted_foreground)),
        );
    }
    let active = statuses.iter().any(|st| matches!(*st, "pending" | "running" | "waiting"));
    let failed = statuses.contains(&"failed");
    let until = if active { Some(app.now_ms) } else if open_end { None } else { end };
    let elapsed = start.zip(until).map(|(from, to)| rows::duration(from, to)).unwrap_or_default();
    let summary = status_summary(&statuses);
    let label = format!("{} {}", items.len(), if items.len() == 1 { "subagent" } else { "subagents" });
    let bright = expanded || active;
    let tone = if active {
        t.c.info
    } else if failed {
        t.c.destructive
    } else {
        t.c.muted_foreground
    };
    let head_key = format!("subagent-group-head-{id}");
    let toggle_key = key.clone();
    let line = row()
        .w_full()
        .min_w(0.0)
        .gap(10.0)
        .center_y()
        .group_opacity(if bright { 1.0 } else { 0.55 }, 1.0)
        .child(faces)
        .child(
            col()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .child(text(label.clone()).font(theme::weight(theme::TEXT_XS, theme::SEMIBOLD)).color(if bright { t.c.foreground } else { t.c.muted_foreground }))
                .child(text(summary.clone()).font(theme::TEXT_3XS).color(tone).truncate()),
        )
        .child(text(elapsed).font(theme::TEXT_XS).color(t.c.muted_foreground))
        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(t.c.muted_foreground).rotate(if expanded { std::f32::consts::PI } else { 0.0 }));
    let trigger = row()
        .key(head_key.clone())
        .w_full()
        .min_w(0.0)
        .py(8.0)
        .pr(13.0)
        .hover_group()
        .focusable(true)
        .role("button")
        .cursor(Cursor::Pointer)
        .label(format!("{label}, {summary}"))
        .expanded(expanded)
        .on_press_tx(move |app: &mut App, tx| {
            tx.keep_on_screen(head_key.clone());
            super::toggle(app, &toggle_key)
        })
        .child(line);
    let mut out = col().w_full().child(trigger);
    if expanded {
        let mut panel = col()
            .w_full()
            .mt(4.0)
            .mb(4.0)
            .padding(4.0)
            .radius(radius::LG)
            .border(1.0, t.c.border.alpha_mul(0.6))
            .fill(t.c.card.alpha_mul(0.3));
        for &item in items {
            panel = panel.child(view(app, item, t));
        }
        out = out.child(panel);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{group, headline, is_script_text, notified_task, open, outcome, resolve, script_markdown, status_summary, target, title};
    use crate::{App, env};
    use serde_json::json;

    #[test]
    fn a_remote_task_opens_its_thread_on_the_machine_that_runs_it() {
        let mut s = App::new();
        let mut local = crate::data::empty_state();
        local.config = Some(json!({"environment": {"environmentId": "local-id", "label": "Laptop"}, "settings": {}}));
        local.shell_ready = true;
        local.conn = s5_net::Conn::Open;
        s.src = crate::data::Source::Fixture(local);
        s.ui.env.active = env::LOCAL.into();
        let remote = json!({"remoteChild": {"environmentId": "env-build", "threadId": "t-remote", "label": "Build box"}, "childThreadId": "t-copy"});
        assert_eq!(target(&s, &remote, &remote, "t-copy"), None, "a machine this client is not connected to cannot open it");
        let mut build = crate::data::empty_state();
        build.shell_ready = true;
        build.conn = s5_net::Conn::Open;
        s.ui.env.saved.push(env::Saved { id: "env-build".into(), label: "Build box".into(), base: "https://build.example".into(), bearer: String::new() });
        env::park(&mut s, "env-build", crate::data::Source::Fixture(build));
        let Some((key, thread)) = target(&s, &remote, &remote, "t-copy") else { panic!("opens on the build box") };
        assert_eq!((key.as_deref(), thread.as_str()), (Some("env-build"), "t-remote"));
        open(&mut s, key.as_deref(), &thread);
        assert_eq!(env::owner(&s, "t-remote"), "env-build");
        assert_eq!(s.ui.env.active, "env-build");
        assert_eq!(s.route, crate::Route::Thread("t-remote".into()));
        let mine = json!({"remoteChild": {"environmentId": "local-id", "threadId": "t-here", "label": "Laptop"}});
        assert_eq!(target(&s, &mine, &mine, ""), Some((Some(env::LOCAL.to_owned()), "t-here".to_owned())), "this machine's own id maps to its key");
        assert_eq!(target(&s, &json!({}), &json!({}), "t-child"), Some((None, "t-child".to_owned())));
    }

    #[test]
    fn notices_report_their_task_until_it_runs_again() {
        let agents = json!([
            {"id":"t1","childThreadId":"c","status":"completed","startedAt":"1970-01-01T00:00:10Z"},
            {"id":"t2","childThreadId":"c","status":"failed","startedAt":"1970-01-01T00:00:20Z"}]);
        let list = agents.as_array().map(Vec::as_slice).unwrap_or_default();
        let by_child = json!({"kind":"subagent","childThreadId":"c"});
        let by_task = json!({"kind":"delegated_task","childThreadId":"c","taskIds":["t2"]});
        assert_eq!(notified_task(list, &by_child).map(|a| a["id"].clone()), Some(json!("t1")));
        assert_eq!(notified_task(list, &by_task).map(|a| a["id"].clone()), Some(json!("t2")));
        assert!(notified_task(list, &json!({"kind":"subagent","childThreadId":"other"})).is_none());
        let agent = &list[0];
        assert_eq!(outcome("completed", agent, Some(15_000)), Some((Some("completed"), "Finished")));
        assert_eq!(outcome("cancelled", agent, Some(15_000)), Some((Some("cancelled"), "Stopped")));
        // Started again after the notice: the live card tells more.
        assert_eq!(outcome("failed", agent, Some(5_000)), None);
        assert_eq!(outcome("updated", agent, Some(5_000)), Some((None, "Updated")));
        assert_eq!(outcome("completed", &json!({"status":"running"}), None), None);
    }

    #[test]
    fn subagent_groups_count_by_status() {
        assert_eq!(status_summary(&["running", "pending", "completed", "cancelled"]), "2 working · 1 done · 1 stopped");
        assert_eq!(status_summary(&["idle", "failed"]), "1 failed · 1 idle");
    }

    #[test]
    fn subagent_titles_name_their_task() {
        assert_eq!(title("Subagent: Review the diff"), "Review the diff");
        assert_eq!(title("/root/agents/code_reviewer/"), "Code Reviewer");
        assert_eq!(title("Explore"), "Explore");
    }

    #[test]
    fn workflow_scripts_open_with_their_meta_export() {
        assert!(is_script_text("export const meta = { name: 'audit' };"));
        assert!(is_script_text("  // Audit billing\n/* phases\n */\nexport  const\tmeta={}"));
        assert!(!is_script_text("export const metadata = {}"));
        assert!(!is_script_text("// export const meta"));
        assert!(!is_script_text("Please export const meta for me"));
        assert!(!is_script_text("/* never closed export const meta"));
    }

    #[test]
    fn a_workflow_thread_opens_with_its_coordinators_plan() {
        let thread = json!({"id": "child", "creationSource": "provider", "lineage": {"relationshipToParent": "subagent", "parentThreadId": "parent"}});
        let script = json!([{"id": "m1", "role": "user", "text": "export const meta = {}"}]);
        let prose = json!([{"id": "m1", "role": "user", "text": "Fix the tests"}]);
        let coordinator = json!([{"id": "w", "childThreadId": "child", "title": "Audit billing", "driver": "claudeAgent", "status": "running",
            "workflow": {"name": "billing-audit", "phases": [], "agents": []}}]);
        let list = |v: &serde_json::Value| v.as_array().map(Vec::as_slice).unwrap_or_default().to_vec();
        let (script, prose, coordinator) = (list(&script), list(&prose), list(&coordinator));
        let Some(opening) = resolve(&thread, "child", &script, None) else { panic!("a script opens the thread before its parent loads") };
        assert_eq!((opening.message_id, opening.name, opening.workflow.is_some()), ("m1", None, false));
        let opening = resolve(&thread, "child", &prose, Some(coordinator.as_slice())).expect("the coordinator's workflow marks it even without the script shape");
        assert_eq!((opening.name, opening.description, opening.driver), (Some("billing-audit"), Some("Audit billing"), "claudeAgent"));
        assert!(opening.opens(&json!({"id": "turn-item", "messageId": "m1"})));
        assert!(resolve(&thread, "child", &prose, None).is_none(), "plain prose without a workflow");
        let unmarked = list(&json!([{"id": "m1", "role": "user", "text": "export const meta = {}", "workflowScript": false}]));
        assert!(resolve(&thread, "child", &unmarked, Some(coordinator.as_slice())).is_none(), "the server's mark wins");
        let user_thread = json!({"id": "child", "creationSource": "user", "lineage": {"relationshipToParent": "subagent"}});
        assert!(resolve(&user_thread, "child", &script, None).is_none());
    }

    #[test]
    fn workflow_agents_group_by_phase_and_count_by_status() {
        let workflow = json!({"name": "audit", "phases": [{"index": 0, "title": "Survey"}], "agents": [
            {"index": 2, "label": "Patch", "state": "running", "phaseIndex": 1},
            {"index": 0, "label": "Map", "state": "completed", "phaseIndex": 0, "phaseTitle": "Guessed"},
            {"index": 1, "label": "Check", "state": "queued", "phaseIndex": 0},
            {"index": 3, "label": "Loose", "state": "failed"}]});
        let (phases, unphased) = group(&workflow, "running");
        let names: Vec<(&str, Vec<&str>)> = phases.iter().map(|p| (p.title.as_str(), p.members.iter().map(|m| m.status).collect())).collect();
        assert_eq!(names, vec![("Survey", vec!["completed", "pending"]), ("Phase 2", vec!["running"])]);
        assert_eq!(unphased.iter().map(|m| m.status).collect::<Vec<_>>(), vec!["failed"]);
        // A stopped coordinator settles the agents still waiting or working.
        let (stopped, _) = group(&workflow, "cancelled");
        assert_eq!(stopped[0].members[1].status, "cancelled");
        let thread = json!({"id": "c", "creationSource": "provider", "lineage": {"relationshipToParent": "subagent"}});
        let messages = vec![json!({"id": "m", "role": "user", "text": "export const meta = {}"})];
        let mut opening = resolve(&thread, "c", &messages, None).expect("script");
        assert_eq!(headline(&opening, &[]), ("Workflow".to_owned(), None, None));
        opening.description = Some("Audit billing");
        opening.name = Some("audit");
        let (title, description, agents) = headline(&opening, &["completed", "pending", "running", "failed", "interrupted"]);
        assert_eq!((title.as_str(), description.as_deref()), ("audit", Some("Audit billing")));
        assert_eq!(agents.as_deref(), Some("5 agents · 2 working · 1 done · 1 failed · 1 stopped"));
        assert_eq!(script_markdown("a ``` b\n"), "````javascript\na ``` b\n````");
    }
}
