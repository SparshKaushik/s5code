//! What the open thread is doing now, at the end of the timeline (ThreadNow and
//! ThreadStatusLine): a subagent or side chat waiting on you, background work or an unanswered
//! Stop, the goal, then whether the thread is snoozed, settled or just woke.

use std::collections::HashSet;

use rune::{El, el, icon, row, text};
use serde_json::{Value, json};

use super::rows::array;
use crate::data::{self, s};
use crate::shell::sidebar::logic::Thread;
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};

#[derive(Default)]
pub struct State {
    requests: Vec<(u64, Request)>,
    /// A background-work Stop: the thread, the tasks it stops, and when Stop may be pressed
    /// again (`None` while the Stop is in flight).
    background_stop: Option<(String, Vec<String>, Option<i64>)>,
    force_stopping: Option<String>,
}

enum Request {
    BackgroundStop(String),
    ForceStop,
    /// A `/goal` command: its thread and the error a failure shows.
    Goal(String, &'static str),
}

/// The Claude adapter forces a stuck query closed after 10 seconds.
const BACKGROUND_STOP_RETRY_MS: i64 = 10_000;

// --- Children waiting on you ------------------------------------------------------------------

/// isChildRunLive: the shell's activity run when it reports one, else its status.
fn run_live(thread: &Value) -> bool {
    match thread.get("activityRunStatus") {
        Some(status) => !status.is_null(),
        None => matches!(s(thread, "status"), "preparing" | "starting" | "running" | "waiting"),
    }
}

/// subagentRequest / sideChatRequest: whether a child asks something of its parent, and if so
/// whether it is an approval.
fn child_request(thread: &Value) -> Option<bool> {
    let kind = s(&thread["pendingRuntimeRequest"], "kind");
    if kind.is_empty() || kind == "auth_refresh" {
        return None;
    }
    let lineage = &thread["lineage"];
    if lineage["sideChat"] != true {
        let subagent = s(lineage, "relationshipToParent") == "subagent";
        if (!subagent && thread["delegatedFrom"].is_null()) || (subagent && s(thread, "creationSource") == "provider") || !run_live(thread) {
            return None;
        }
    }
    Some(kind != "user_input")
}

fn child_of(thread: &Value, parent: &str) -> bool {
    let lineage = &thread["lineage"];
    let id = s(lineage, "parentThreadId");
    if !id.is_empty() && (s(lineage, "relationshipToParent") == "subagent" || lineage["sideChat"] == true) {
        return id == parent;
    }
    s(&thread["delegatedFrom"], "threadId") == parent
}

/// childThreadInputsAtom: the parent's descendants asking something, nearest first.
fn child_inputs<'a>(threads: &'a [Value], parent: &'a str) -> Vec<&'a Value> {
    if parent.is_empty() || !threads.iter().any(|t| child_request(t).is_some()) {
        return Vec::new();
    }
    let mut seen: HashSet<&str> = HashSet::from([parent]);
    let mut pending = vec![parent];
    let mut out = Vec::new();
    let mut next = 0;
    while next < pending.len() {
        let at = pending[next];
        next += 1;
        for thread in threads.iter().filter(|t| child_of(t, at)) {
            let id = s(thread, "id");
            if !seen.insert(id) {
                continue;
            }
            pending.push(id);
            if child_request(thread).is_some() {
                out.push(thread);
            }
        }
    }
    out
}

/// childThreadInputsTitle.
fn child_inputs_title(children: &[&Value]) -> String {
    let approvals = children.iter().filter(|c| child_request(c) == Some(true)).count();
    let need = if approvals == 0 {
        "input"
    } else if approvals == children.len() {
        "approval"
    } else {
        "you"
    };
    let side = children.iter().filter(|c| c["lineage"]["sideChat"] == true).count();
    let noun = if side == children.len() {
        "side chat"
    } else if side == 0 {
        "subagent"
    } else {
        "thread"
    };
    if children.len() == 1 {
        format!("{} needs {need}", if noun == "side chat" { "Side chat" } else { "Subagent" })
    } else {
        format!("{} {noun}s need {need}", children.len())
    }
}

// --- Background work ---------------------------------------------------------------------------

/// A background task's order and nouns (BACKGROUND_WORK_KINDS).
fn kind_nouns(kind: &str) -> (u8, &'static str, &'static str) {
    match kind {
        "subagent" => (0, "subagent", "subagents"),
        "command" => (1, "command", "commands"),
        "monitor" => (2, "monitor", "monitors"),
        _ => (3, "background task", "background tasks"),
    }
}

/// formatSubagentDisplayTitle: no "Subagent:" prefix, and a `/root/.../name` path as its name.
fn subagent_title(title: &str) -> String {
    let display = match title.get(..9) {
        Some(prefix) if prefix.eq_ignore_ascii_case("subagent:") => title[9..].trim_start(),
        _ => title,
    };
    let Some(path) = display.strip_prefix("/root/") else { return display.to_string() };
    let path = path.strip_suffix('/').unwrap_or(path);
    if path.split('/').any(str::is_empty) {
        return display.to_string();
    }
    let last = path.rsplit('/').next().unwrap_or(path);
    let words: Vec<String> = last
        .split(|c: char| c == '_' || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
        })
        .collect();
    if words.is_empty() { display.to_string() } else { words.join(" ") }
}

#[derive(Debug, PartialEq)]
struct Task {
    id: String,
    kind: String,
    label: String,
    child: Option<String>,
}

/// presentPendingBackgroundWork: the title and the tasks, subagents first.
fn background_work(tasks: &[Value]) -> Option<(String, Vec<Task>)> {
    if tasks.is_empty() {
        return None;
    }
    let waiting = tasks.iter().any(|t| t["awaited"] == true || matches!(s(t, "kind"), "subagent" | "background_task"));
    let mut items: Vec<Task> = tasks
        .iter()
        .map(|t| {
            let kind = s(t, "kind").to_string();
            let label = t["description"].as_str().map(str::trim).map(|d| if kind == "subagent" { subagent_title(d).trim().to_string() } else { d.to_string() });
            Task {
                id: s(t, "taskId").to_string(),
                label: label.filter(|l| !l.is_empty()).unwrap_or_else(|| kind_nouns(&kind).1.to_string()),
                child: (kind == "subagent").then(|| s(t, "childThreadId").to_string()).filter(|c| !c.is_empty()),
                kind,
            }
        })
        .collect();
    items.sort_by_key(|t| kind_nouns(&t.kind).0);
    if let [only] = items.as_slice() {
        let noun = kind_nouns(&only.kind).1;
        let title = if only.label != noun {
            format!("{} · {}", if waiting { "Waiting" } else { "Running" }, only.label)
        } else if waiting {
            format!("Waiting on a {noun}")
        } else {
            format!("Running a {noun}")
        };
        return Some((title, items));
    }
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for item in &items {
        match counts.iter_mut().find(|(k, _)| *k == item.kind) {
            Some((_, n)) => *n += 1,
            None => counts.push((item.kind.as_str(), 1)),
        }
    }
    let groups: Vec<String> = counts
        .iter()
        .map(|(kind, n)| {
            let (_, one, many) = kind_nouns(kind);
            format!("{n} {}", if *n == 1 { one } else { many })
        })
        .collect();
    let joined = match groups.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => groups.join(""),
    };
    let title = format!("{} {joined}", if waiting { "Waiting on" } else { "Running" });
    Some((title, items))
}

/// isBackgroundWorkStopPending: a Stop in flight, or one accepted while its tasks are still
/// listed, until it may be retried.
fn background_stop_pending(stop: Option<&(String, Vec<String>, Option<i64>)>, thread: &str, tasks: &[Value], now: i64) -> bool {
    let Some((id, stopped, retry_at)) = stop else { return false };
    if id != thread {
        return false;
    }
    match retry_at {
        None => true,
        Some(at) => now < *at && tasks.iter().any(|t| stopped.iter().any(|i| i == s(t, "taskId"))),
    }
}

// --- Stop -------------------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
struct RunStop {
    unresponsive: bool,
    run: String,
    requested_at: Option<i64>,
    /// The run already ended; the Stop targeted its background work.
    background: bool,
}

fn ordinal(v: &Value) -> i64 {
    v["ordinal"].as_i64().unwrap_or(-1)
}

/// presentRunStop: a Stop the provider has not finished yet.
fn run_stop(p: &Value) -> Option<RunStop> {
    let runs = array(p, "runs");
    let latest = runs.iter().filter(|r| s(r, "status") != "queued").max_by_key(|r| ordinal(r));
    let active = runs.iter().filter(|r| matches!(s(r, "status"), "preparing" | "starting" | "running")).max_by_key(|r| ordinal(r));
    let run = s(active.or(latest)?, "id");
    let items = array(p, "turnItems").iter().chain(array(p, "visibleTurnItems")).filter(|i| s(i, "runId") == run);
    let mut request: Option<&Value> = None;
    for item in items {
        match s(item, "type") {
            "run_interrupt_result" => return None,
            "run_interrupt_request" if request.is_none_or(|r| ordinal(item) >= ordinal(r)) => request = Some(item),
            _ => {}
        }
    }
    let request = request?;
    let unresponsive = !request["unresponsiveAt"].is_null();
    if active.is_none() && !unresponsive {
        return None;
    }
    Some(RunStop { unresponsive, run: run.to_string(), requested_at: data::ts(request, "startedAt").or_else(|| data::ts(request, "updatedAt")), background: active.is_none() })
}

// --- Goal -------------------------------------------------------------------------------------

/// formatGoalTokens.
fn goal_tokens(tokens: u64) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }
    let thousands = (tokens as f64 / 1_000.0).round() as u64;
    if thousands < 1_000 {
        return format!("{thousands}k");
    }
    let millions = format!("{:.1}", tokens as f64 / 1_000_000.0);
    format!("{}m", millions.strip_suffix(".0").unwrap_or(&millions))
}

/// presentProviderGoal: the line's title (with its usage) and whether Resume applies.
fn goal_line(goal: &Value, working: bool) -> (String, bool) {
    let status = s(goal, "status");
    let mut usage = Vec::new();
    if let Some(used) = goal["tokensUsed"].as_u64().filter(|u| *u > 0) {
        usage.push(match goal["tokenBudget"].as_u64() {
            Some(budget) => format!("{} / {} tokens", goal_tokens(used), goal_tokens(budget)),
            None => format!("{} tokens", goal_tokens(used)),
        });
    }
    if let Some(seconds) = goal["timeUsedSeconds"].as_f64().filter(|s| *s >= 60.0) {
        usage.push(format_duration(seconds * 1000.0));
    }
    if let Some(checks) = goal["checks"].as_u64().filter(|c| *c > 0) {
        usage.push(format!("{checks} {}", if checks == 1 { "check" } else { "checks" }));
    }
    let title = match status {
        "active" if !working => "Goal set",
        "active" => "Pursuing goal",
        "paused" => "Goal paused",
        "blocked" => "Goal blocked",
        "usage_limited" => "Goal hit a usage limit",
        "budget_limited" => "Goal reached its token budget",
        _ => "Goal complete",
    };
    let title = if usage.is_empty() { title.to_string() } else { format!("{title} · {}", usage.join(" · ")) };
    (title, status != "active" && status != "complete")
}

// --- Labels -----------------------------------------------------------------------------------

/// formatDuration.
fn format_duration(ms: f64) -> String {
    if !ms.is_finite() || ms < 0.0 {
        return "0ms".into();
    }
    if ms < 1_000.0 {
        return format!("{}ms", ms.round().max(1.0));
    }
    if ms < 10_000.0 {
        let tenths = (ms / 100.0).round() / 10.0;
        return format!("{tenths}s");
    }
    let total = (ms / 1_000.0).round() as i64;
    if total < 60 {
        return format!("{total}s");
    }
    let (h, m, sec) = (total / 3_600, total % 3_600 / 60, total % 60);
    [(h, "h"), (m, "m"), (sec, "s")].iter().filter(|(n, _)| *n > 0).map(|(n, unit)| format!("{n}{unit}")).collect::<Vec<_>>().join(" ")
}

/// snoozeWakeLabel: how long until a snoozed thread wakes.
fn wake_in(until: i64, now: i64) -> String {
    const HOUR: i64 = 3_600_000;
    const DAY: i64 = 24 * HOUR;
    let left = until - now;
    if left <= 0 {
        "now".into()
    } else if left < HOUR {
        format!("{}m", ((left + 59_999) / 60_000).max(1))
    } else if left < DAY {
        format!("{}h", (left + HOUR - 1) / HOUR)
    } else {
        format!("{}d", (left + DAY - 1) / DAY)
    }
}

/// formatRelativeTimeLabel.
fn ago(at: i64, now: i64) -> String {
    let seconds = (now - at).max(0) / 1000;
    match seconds {
        s if s < 60 => "just now".into(),
        s if s < 3_600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3_600),
        s => format!("{}d ago", s / 86_400),
    }
}

fn iso(ms: i64) -> String {
    let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    // Civil date from days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000)
}

// --- Actions ----------------------------------------------------------------------------------

fn stop_background(s: &mut App, thread: &str, tasks: Vec<String>) {
    let Some(net) = s.src.net() else { return };
    let request = net.dispatch(json!({"type": "thread.stop", "commandId": s5_net::uuid(), "threadId": thread}));
    let now = &mut s.ui.chat.topbar.now;
    now.background_stop = Some((thread.to_string(), tasks, None));
    now.requests.push((request, Request::BackgroundStop(thread.to_string())));
}

/// useForceStopThread: names the run that ignored Stop, so a stale button cannot hit a newer one.
fn force_stop(s: &mut App, thread: &str, run: &str) {
    if s.ui.chat.topbar.now.force_stopping.is_some() {
        return;
    }
    let Some(net) = s.src.net() else { return };
    let request = net.request("orchestration.forceStopThread", json!({"threadId": thread, "runId": run}));
    let now = &mut s.ui.chat.topbar.now;
    now.force_stopping = Some(thread.to_string());
    now.requests.push((request, Request::ForceStop));
}

/// sendStandaloneCommand: `/goal resume` or `/goal clear` as its own turn.
fn goal_command(s: &mut App, thread: &str, command: &str, failure: &'static str) {
    let Some(net) = s.src.net() else { return };
    let request = net.dispatch(json!({
        "type": "message.dispatch", "commandId": s5_net::uuid(), "createdBy": "user", "creationSource": "web",
        "threadId": thread, "messageId": s5_net::uuid(), "text": command, "attachments": [],
        "dispatchMode": {"type": "start_immediately"},
    }));
    s.ui.chat.topbar.now.requests.push((request, Request::Goal(thread.to_string(), failure)));
}

/// useAcknowledgeThreadWoke: marks the thread visited as of its wake.
fn acknowledge_woke(s: &mut App, thread: &str, woke_at: i64) {
    if let Some(net) = crate::env::net_for(s, thread) {
        net.dispatch(json!({"type": "thread.visit", "commandId": s5_net::uuid(), "threadId": thread, "visitedAt": iso(woke_at)}));
    }
}

fn can_force_stop(s: &App) -> bool {
    s.state().config.as_ref().is_some_and(|c| c["environment"]["capabilities"]["threadForceStop"] == true)
}

/// Takes the footer's replies: a refused Stop or `/goal` shows in the thread's error banner, a
/// Force stop reports how it went.
pub fn poll(s: &mut App) -> bool {
    if s.ui.chat.topbar.now.requests.is_empty() {
        return false;
    }
    let Some(net) = s.src.net() else { return false };
    let mut done = Vec::new();
    for (id, request) in std::mem::take(&mut s.ui.chat.topbar.now.requests) {
        match net.take_reply(id) {
            Some(reply) => done.push((request, reply)),
            None => s.ui.chat.topbar.now.requests.push((id, request)),
        }
    }
    let changed = !done.is_empty();
    for (request, reply) in done {
        match (request, reply) {
            (Request::BackgroundStop(thread), Ok(_)) => {
                if let Some(stop) = s.ui.chat.topbar.now.background_stop.as_mut().filter(|b| b.0 == thread) {
                    stop.2 = Some(data::now_ms() + BACKGROUND_STOP_RETRY_MS);
                }
            }
            (Request::BackgroundStop(thread), Err(e)) => {
                if s.ui.chat.topbar.now.background_stop.as_ref().is_some_and(|b| b.0 == thread) {
                    s.ui.chat.topbar.now.background_stop = None;
                }
                let message = crate::shell::palette::error_message(&e, "Failed to stop background work.");
                super::error_banner::set_local(s, &thread, Some(&message));
            }
            (Request::ForceStop, reply) => {
                s.ui.chat.topbar.now.force_stopping = None;
                match reply {
                    Err(e) => {
                        let message = crate::shell::palette::error_message(&e, "");
                        crate::shell::palette::toast_error_brief(s, "Could not force stop the agent", Some(&message));
                    }
                    Ok(v) if v["outcome"] == "releasing" => crate::shell::palette::toast(s, "Turn stopped; the agent is still shutting down", None),
                    Ok(v) if v["outcome"] == "shared" => {
                        let others = v["sharedWithThreadIds"].as_array().map_or(0, Vec::len);
                        let count = if others == 0 { "other".to_string() } else { others.to_string() };
                        let noun = if others == 1 { "thread" } else { "threads" };
                        let description = format!("It is shared with {count} working {noun}. Stop those to free it.");
                        crate::shell::palette::toast(s, "Turn stopped; the agent process kept running", Some(&description));
                    }
                    Ok(_) => {}
                }
            }
            (Request::Goal(thread, failure), Err(e)) => {
                let message = crate::shell::palette::error_message(&e, failure);
                super::error_banner::set_local(s, &thread, Some(&message));
            }
            (Request::Goal(..), Ok(_)) => {}
        }
    }
    changed
}

// --- View -------------------------------------------------------------------------------------

/// An inline action (InlineButton); a disabled one dims and takes no press.
fn action(t: &T, label: &str, strong: bool, on_press: Option<impl Fn(&mut App) + 'static>) -> El {
    let button = super::inline_button(label, strong, t).label(label.to_string());
    match on_press {
        Some(f) => button.on_press(f),
        None => button.opacity(0.64),
    }
}

/// One line at the end of the timeline: icon, title and detail as one truncating line, then
/// the actions.
fn line(t: &T, key: &str, glyph: El, title: &str, detail: Option<String>, actions: Vec<El>) -> El {
    let muted = t.c.muted_foreground;
    let mut words = row().shrink().min_w(0.0).gap(5.0).center_y().child(
        text(title.to_string()).font(theme::TEXT_XS).color(muted).single_line().truncate().shrink().min_w(0.0).help_when_truncated(title.to_string()),
    );
    if let Some(detail) = detail.filter(|d| !d.is_empty()) {
        let faint = muted.alpha_mul(if t.dark { 0.6 } else { 0.8 });
        words = words.child(text(detail.clone()).font(theme::TEXT_XS).color(faint).single_line().truncate().shrink().min_w(0.0).help_when_truncated(detail));
    }
    let mut out = row().key(key.to_string()).w_full().min_w(0.0).gap(6.0).px(4.0).pb(8.0).center_y().child(el().size(14.0, 14.0).center().child(glyph)).child(words);
    if !actions.is_empty() {
        out = out.child(row().ml(6.0).gap(10.0).center_y().children(actions));
    }
    out
}

fn glyph(t: &T, i: &'static rune::Icon) -> El {
    icon(i, 14.0).color(t.c.muted_foreground)
}

fn stop_line(app: &App, t: &T, thread: &str, stop: &RunStop) -> El {
    let mut actions = Vec::new();
    if let Some(at) = stop.requested_at {
        actions.push(text(format_duration((app.now_ms - at).max(1_000) as f64)).font(theme::TEXT_XS).color(t.c.muted_foreground));
    }
    if stop.unresponsive && can_force_stop(app) {
        let stopping = app.ui.chat.topbar.now.force_stopping.as_deref() == Some(thread);
        let (thread, run) = (thread.to_string(), stop.run.clone());
        let press = (!stopping).then(|| move |s: &mut App| force_stop(s, &thread, &run));
        actions.push(action(t, if stopping { "Force stopping..." } else { "Force stop" }, true, press));
    }
    if stop.unresponsive {
        line(t, "thread-now-stop", icon(&icons::TRIANGLE_ALERT, 14.0).color(t.c.warning_foreground), "Agent not responding to Stop", Some("It may be short on memory. Force stop kills its processes.".into()), actions)
    } else {
        line(t, "thread-now-stop", glyph(t, &icons::CIRCLE_STOP), "Stopping", None, actions)
    }
}

fn child_line(t: &T, children: &[&Value]) -> Option<El> {
    let first = children.first()?;
    let approval = child_request(first) == Some(true);
    let mark = icon(if approval { &icons::SHIELD_QUESTION_MARK } else { &icons::MESSAGE_CIRCLE }, 14.0).color(t.c.info_foreground);
    let names: Vec<&str> = children.iter().map(|c| s(c, "title")).collect();
    let id = s(first, "id").to_string();
    let open = action(t, if approval { "Open approval" } else { "Open question" }, true, Some(move |s: &mut App| crate::shell::sidebar::open_thread(s, &id)));
    Some(line(t, "thread-now-child", mark, &child_inputs_title(children), Some(names.join(", ")), vec![open]))
}

fn work_line(app: &App, t: &T, thread: &str, tasks: &[Value], projection: Option<&Value>) -> Option<El> {
    let (title, items) = background_work(tasks)?;
    let commands = items.iter().all(|i| i.kind != "subagent");
    // The turn item behind a task (same key derivePendingBackgroundWork gives it).
    let turn_item = |task: &str| {
        projection.and_then(|p| array(p, "turnItems").iter().rev().find(|i| [s(&i["nativeItemRef"], "nativeId"), s(i, "id")].into_iter().find(|k| !k.is_empty()) == Some(task)))
    };
    let activity = |task: &Task| {
        turn_item(task.id.as_str()).filter(|i| s(i, "type") == "subagent").map(|i| s(i, "progress").split_whitespace().collect::<Vec<_>>().join(" ")).filter(|p| !p.is_empty())
    };
    let only = if items.len() == 1 { items.first() } else { None };
    let detail = match only {
        Some(task) => activity(task),
        None => match items.iter().find_map(|i| activity(i).map(|a| (i, a))) {
            Some((task, progress)) => Some(format!("{}: {progress}", task.label)),
            None => Some(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>().join(", ")),
        },
    };
    let mut actions = Vec::new();
    if let Some(started) = only.and_then(|task| turn_item(task.id.as_str())).and_then(|i| data::ts(i, "startedAt")) {
        actions.push(text(format_duration((app.now_ms - started).max(1_000) as f64)).font(theme::TEXT_XS).color(t.c.muted_foreground));
    }
    let child = only.and_then(|task| task.child.clone().or_else(|| turn_item(task.id.as_str()).map(|i| s(i, "childThreadId").to_string()).filter(|c| !c.is_empty())));
    if let Some(child) = child {
        actions.push(action(t, "Open", false, Some(move |s: &mut App| crate::shell::sidebar::open_thread(s, &child))));
    }
    let stopping = background_stop_pending(app.ui.chat.topbar.now.background_stop.as_ref(), thread, tasks, app.now_ms);
    let (thread, ids) = (thread.to_string(), items.iter().map(|i| i.id.clone()).collect::<Vec<_>>());
    let press = (!stopping).then(|| move |s: &mut App| stop_background(s, &thread, ids.clone()));
    actions.push(action(t, if stopping { "Stopping..." } else { "Stop" }, true, press));
    Some(line(t, "thread-now-work", glyph(t, if commands { &icons::TERMINAL } else { &icons::CIRCLE_DASHED }), &title, detail, actions))
}

fn goal_view(t: &T, thread: &str, goal: &Value, working: bool) -> El {
    let (title, can_resume) = goal_line(goal, working);
    let mut actions = Vec::new();
    if can_resume && !working {
        let id = thread.to_string();
        actions.push(action(t, "Resume", true, Some(move |s: &mut App| goal_command(s, &id, "/goal resume", "Failed to resume goal."))));
        actions.push(text("·").font(theme::TEXT_XS).color(t.c.muted_foreground));
    }
    let id = thread.to_string();
    actions.push(action(t, "Clear", false, Some(move |s: &mut App| goal_command(s, &id, "/goal clear", "Failed to clear goal."))));
    line(t, "thread-now-goal", glyph(t, &icons::TARGET), &title, Some(s(goal, "objective").to_string()), actions)
}

/// ThreadStatusLine: snoozed, settled, or woke from a snooze, with the way back.
fn status_line(app: &App, t: &T, thread: &str, shell: &Value) -> Option<El> {
    if shell["snoozedUntil"].is_null() && shell["settledOverride"] != "settled" {
        return None;
    }
    let model = crate::shell::sidebar::Model::build(app);
    let th: &Thread = model.thread(thread)?;
    let now = app.now_ms;
    let id = thread.to_string();
    let (mark, label, act): (&'static rune::Icon, String, El) = if th.snoozed(now) {
        let wake = th.snoozed_until.map(|until| wake_in(until, now)).filter(|w| w != "now");
        let label = wake.map_or_else(|| "Snoozed".to_string(), |w| format!("Snoozed, {w} left"));
        (&icons::ALARM_CLOCK, label, action(t, "Wake now", true, Some(move |s: &mut App| crate::shell::sidebar::unsnooze(s, &id))))
    } else if th.settled {
        let label = th.settled_at.map_or_else(|| "Settled".to_string(), |at| format!("Settled {}", ago(at, now)));
        (&icons::CIRCLE_CHECK, label, action(t, "Un-settle", true, Some(move |s: &mut App| crate::shell::sidebar::unsettle(s, &id))))
    } else if th.woke(now) {
        let woke_at = th.woke_at(now)?;
        (&icons::ALARM_CLOCK, "Woke from snooze".to_string(), action(t, "Dismiss", true, Some(move |s: &mut App| acknowledge_woke(s, &id, woke_at))))
    } else {
        return None;
    };
    let dot = text("·").font(theme::TEXT_XS).color(t.c.muted_foreground);
    Some(
        row()
            .key("thread-status-line")
            .w_full()
            .min_w(0.0)
            .gap(6.0)
            .px(4.0)
            .pb(8.0)
            .center_y()
            .child(glyph(t, mark))
            .child(text(label).font(theme::TEXT_XS).color(t.c.muted_foreground).single_line().truncate().shrink().min_w(0.0))
            .child(dot)
            .child(act),
    )
}

/// The thread's present, for the end of its timeline.
pub fn view(app: &App, thread: &str, t: &T) -> Option<El> {
    let state = app.state();
    let shell = state.threads.iter().find(|r| s(r, "id") == thread)?;
    let projection = state.thread_views.get(thread).map(|v| &v.projection);
    // ChatView isWorking: a run is active.
    let working = !shell["activeRunId"].is_null();
    // The interrupt path also accepts a finished run while its provider still has background work.
    let tasks: &[Value] = if working { &[] } else { array(shell, "pendingBackgroundTasks") };
    let stop = projection.and_then(run_stop).filter(|r| !(r.background && tasks.is_empty()));
    let asking = s(&shell["pendingRuntimeRequest"], "kind");
    let children = if asking.is_empty() || asking == "auth_refresh" { child_inputs(&state.threads, thread) } else { Vec::new() };
    // A Stop that went unanswered needs you; one in progress is the thread's work.
    let (attention, work) = match &stop {
        Some(r) if r.unresponsive => (Some(stop_line(app, t, thread, r)), work_line(app, t, thread, tasks, projection)),
        Some(r) => (child_line(t, &children), Some(stop_line(app, t, thread, r))),
        None => (child_line(t, &children), work_line(app, t, thread, tasks, projection)),
    };
    let goal = shell["goal"].is_object().then(|| goal_view(t, thread, &shell["goal"], working));
    let lines: Vec<El> = [attention, work, goal, status_line(app, t, thread, shell)].into_iter().flatten().collect();
    (!lines.is_empty()).then(|| rune::col().key("thread-now").w_full().children(lines))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn waiting_children_roll_up_from_subagents_and_side_chats() {
        let threads = vec![
            json!({"id": "p", "title": "Parent"}),
            json!({"id": "a", "title": "Audit", "status": "running", "lineage": {"parentThreadId": "p", "relationshipToParent": "subagent"}, "pendingRuntimeRequest": {"kind": "command"}}),
            json!({"id": "b", "title": "Nested", "status": "running", "lineage": {"parentThreadId": "a", "relationshipToParent": "subagent"}, "pendingRuntimeRequest": {"kind": "user_input"}}),
            json!({"id": "c", "title": "Idle", "status": "idle", "lineage": {"parentThreadId": "p", "relationshipToParent": "subagent"}, "pendingRuntimeRequest": {"kind": "command"}}),
            json!({"id": "d", "title": "Native", "status": "running", "creationSource": "provider", "lineage": {"parentThreadId": "p", "relationshipToParent": "subagent"}, "pendingRuntimeRequest": {"kind": "command"}}),
            json!({"id": "e", "title": "Aside", "status": "idle", "lineage": {"parentThreadId": "p", "sideChat": true}, "pendingRuntimeRequest": {"kind": "user_input"}}),
        ];
        let ids = |v: Vec<&serde_json::Value>| v.iter().map(|t| s(t, "id").to_string()).collect::<Vec<_>>();
        assert_eq!(ids(child_inputs(&threads, "p")), ["a", "e", "b"], "an idle or provider-native subagent cannot be answered");
        let children = child_inputs(&threads, "p");
        assert_eq!(child_inputs_title(&children), "3 threads need you");
        assert_eq!(child_inputs_title(&children[..1]), "Subagent needs approval");
        assert_eq!(child_inputs_title(&children[1..2]), "Side chat needs input");
        assert!(child_inputs(&threads, "x").is_empty());
    }

    #[test]
    fn background_work_titles_name_one_task_and_count_many() {
        let task = |kind: &str, description: Option<&str>| json!({"taskId": format!("{kind}-{}", description.unwrap_or("")), "kind": kind, "description": description});
        assert_eq!(background_work(&[]), None);
        assert_eq!(background_work(&[task("command", Some("bun dev"))]).unwrap().0, "Running · bun dev");
        assert_eq!(background_work(&[task("command", None)]).unwrap().0, "Running a command");
        assert_eq!(background_work(&[task("subagent", None)]).unwrap().0, "Waiting on a subagent");
        assert_eq!(background_work(&[task("subagent", Some("Subagent: /root/team/code_reviewer"))]).unwrap().0, "Waiting · Code Reviewer");
        let many = background_work(&[task("command", Some("a")), task("monitor", Some("b")), task("command", Some("c"))]).unwrap();
        assert_eq!(many.0, "Running 2 commands and 1 monitor");
        let mixed = background_work(&[task("command", Some("a")), task("subagent", Some("Audit")), task("monitor", Some("m"))]).unwrap();
        assert_eq!(mixed.0, "Waiting on 1 subagent, 1 command and 1 monitor");
        assert_eq!(mixed.1[0].label, "Audit", "subagents first");
    }

    #[test]
    fn a_background_stop_stays_pending_while_its_tasks_run_until_it_may_retry() {
        let tasks = vec![json!({"taskId": "t1"})];
        let sent = ("th".to_string(), vec!["t1".to_string()], None);
        assert!(background_stop_pending(Some(&sent), "th", &tasks, 0), "in flight");
        assert!(!background_stop_pending(Some(&sent), "other", &tasks, 0));
        let accepted = ("th".to_string(), vec!["t1".to_string()], Some(10_000));
        assert!(background_stop_pending(Some(&accepted), "th", &tasks, 5_000));
        assert!(!background_stop_pending(Some(&accepted), "th", &[], 5_000), "its tasks ended");
        assert!(!background_stop_pending(Some(&accepted), "th", &tasks, 10_000), "Stop may be pressed again");
        assert!(!background_stop_pending(None, "th", &tasks, 0));
    }

    #[test]
    fn a_stop_is_stopping_until_answered_and_unresponsive_once_flagged() {
        let projection = |status: &str, items: serde_json::Value| json!({"runs": [{"id": "r1", "ordinal": 1, "status": "completed"}, {"id": "r2", "ordinal": 2, "status": status}], "turnItems": items});
        let request = json!({"type": "run_interrupt_request", "runId": "r2", "startedAt": "2026-03-10T12:00:00Z"});
        let stop = run_stop(&projection("running", json!([request]))).unwrap();
        assert_eq!((stop.unresponsive, stop.run.as_str(), stop.background), (false, "r2", false));
        let mut flagged = request.clone();
        flagged["unresponsiveAt"] = json!("2026-03-10T12:00:30Z");
        assert!(run_stop(&projection("running", json!([flagged.clone()]))).unwrap().unresponsive);
        assert_eq!(run_stop(&projection("completed", json!([request.clone()]))), None, "an ended run's stop only counts when unanswered");
        assert!(run_stop(&projection("completed", json!([flagged]))).unwrap().background);
        let answered = json!([request, {"type": "run_interrupt_result", "runId": "r2"}]);
        assert_eq!(run_stop(&projection("running", answered)), None);
        assert_eq!(run_stop(&projection("running", json!([]))), None);
    }

    #[test]
    fn goal_lines_read_status_and_usage() {
        let goal = json!({"status": "paused", "objective": "Ship it", "tokensUsed": 12_400, "tokenBudget": 2_500_000, "timeUsedSeconds": 754, "checks": 1});
        assert_eq!(goal_line(&goal, false), ("Goal paused · 12k / 2.5m tokens · 12m 34s · 1 check".to_string(), true));
        assert_eq!(goal_line(&json!({"status": "active"}), false), ("Goal set".to_string(), false));
        assert_eq!(goal_line(&json!({"status": "active", "tokensUsed": 0}), true), ("Pursuing goal".to_string(), false));
        assert_eq!(goal_line(&json!({"status": "complete", "tokensUsed": 2_000_000}), false), ("Goal complete · 2m tokens".to_string(), false));
    }

    #[test]
    fn labels_match_the_web_formatters() {
        assert_eq!((format_duration(400.0), format_duration(4_250.0), format_duration(59_500.0), format_duration(3_723_000.0)), ("400ms".into(), "4.3s".into(), "1m".into(), "1h 2m 3s".into()));
        let now = 1_000_000_000;
        assert_eq!((wake_in(now, now), wake_in(now + 30_000, now), wake_in(now + 61 * 60_000, now), wake_in(now + 25 * 3_600_000, now)), ("now".into(), "1m".into(), "2h".into(), "2d".into()));
        assert_eq!((ago(now - 30_000, now), ago(now - 5 * 60_000, now), ago(now - 49 * 3_600_000, now)), ("just now".into(), "5m ago".into(), "2d ago".into()));
        assert_eq!(data::iso_ms(&iso(1_773_144_000_123)), Some(1_773_144_000_123));
    }
}
