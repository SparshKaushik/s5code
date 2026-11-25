use crate::data::{s, ts};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Item kinds that join a tool group. A rendered HTML page and an MCP app are their own rows
/// (session-logic.ts "html-render" and "mcp-app" entries), not tool work.
pub fn groupable(item: &Value) -> bool {
    matches!(
        s(item, "type"),
        "command_execution" | "file_change" | "file_search" | "web_search" | "dynamic_tool" | "reasoning" | "thread_created"
    ) && crate::chat::details::html_render::reference(item).is_none()
        && crate::panel::mcp_app::reference(item).is_none()
}

pub fn item_active(item: &Value) -> bool {
    matches!(s(item, "status"), "pending" | "running" | "waiting")
}

#[derive(Debug, PartialEq)]
pub enum Row<'a> {
    User(&'a Value),
    Assistant(&'a Value),
    /// A finished turn's fold ("Worked for 12s") over its work and intermediate replies.
    Work {
        id: String,
        label: String,
        items: Vec<&'a Value>,
    },
    /// The turn in flight's single "Working for 3s" header; its work shows unfolded below it.
    Working {
        id: String,
        label: String,
        compacting: bool,
    },
    Item(&'a Value),
    /// Consecutive tool calls of the turn in flight, behind one summary row.
    Tools {
        id: String,
        items: Vec<&'a Value>,
    },
    /// The turn in flight is waiting on the model.
    Thinking {
        id: String,
    },
    /// What an attempt cut off by a steer left behind, folded ("Superseded attempt").
    Attempt {
        id: String,
        items: Vec<&'a Value>,
    },
    /// Adjacent delegated tasks of one provider turn, behind "N subagents".
    Subagents {
        id: String,
        items: Vec<&'a Value>,
    },
    Request {
        request: &'a Value,
        item: Option<&'a Value>,
    },
    /// A settled reply's status, copy, fork and time, moved below the tool work still showing
    /// after it (MessagesTimeline "assistant-meta").
    Meta {
        id: String,
        item: &'a Value,
    },
}

pub fn duration(start: i64, end: i64) -> String {
    let seconds = (end - start).max(0) / 1000;
    if end > start && end - start < 10000 && (end - start) % 1000 != 0 {
        format!("{:.1}s", (end - start) as f64 / 1000.0)
    } else if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
    }
}

pub fn array<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key].as_array().map(Vec::as_slice).unwrap_or(&[])
}

const LIFECYCLE: [&str; 5] = ["turn_start", "turn_complete", "run_start", "run_complete", "run_interrupt_request"];

fn run_active(run: &Value) -> bool {
    matches!(s(run, "status"), "starting" | "running" | "waiting")
}

fn steers(item: &Value) -> bool {
    matches!(s(item, "inputIntent"), "steer" | "promoted_queued_to_steer")
}

/// The attempt an item ran in: its node's, or the nearest ancestor's, root attempt within the
/// item's run (session-logic.ts resolveAttempt).
fn attempt_of<'a>(item: &Value, roots: &HashMap<&str, &'a Value>, nodes: &HashMap<&str, &'a Value>) -> Option<&'a Value> {
    let run = s(item, "runId");
    let mut node = s(item, "nodeId");
    if run.is_empty() {
        return None;
    }
    let mut visited = HashSet::new();
    while !node.is_empty() && visited.insert(node) {
        if let Some(attempt) = roots.get(node).copied().filter(|a| s(a, "runId") == run) {
            return Some(attempt);
        }
        let parent: &Value = nodes.get(node).copied()?;
        if let Some(attempt) = roots.get(s(parent, "rootNodeId")).copied().filter(|a| s(a, "runId") == run) {
            return Some(attempt);
        }
        node = s(parent, "parentNodeId");
    }
    None
}

/// Superseded attempts (MessagesTimeline.logic.ts deriveSupersededAttemptFolds): each item to
/// fold, by id, with its attempt's id; and the replies an attempt finished before the steer cut
/// it off, which stay in view and keep their attempt unfolded.
fn superseded<'a>(p: &'a Value, items: &[&'a Value]) -> (HashMap<&'a str, &'a str>, HashSet<&'a str>) {
    let roots: HashMap<&str, &Value> = array(p, "attempts").iter().map(|a| (s(a, "rootNodeId"), a)).collect();
    if roots.values().all(|a| s(a, "status") != "superseded") {
        return Default::default();
    }
    let nodes: HashMap<&str, &Value> = array(p, "nodes").iter().map(|n| (s(n, "id"), n)).collect();
    let attempts: Vec<Option<&Value>> = items.iter().map(|i| attempt_of(i, &roots, &nodes).filter(|a| s(a, "status") == "superseded")).collect();
    let event = |kind: &str| {
        LIFECYCLE.contains(&kind)
            || matches!(kind, "checkpoint" | "subagent" | "fork" | "run_interrupt_result" | "handoff" | "link_request" | "secret_request" | "proposed_plan")
    };
    // Interrupted streams can read `completed`; only the attempt's last output proves it finished.
    let mut last: HashMap<&str, (&Value, &Value)> = HashMap::new();
    for (&item, attempt) in items.iter().zip(&attempts) {
        let &Some(attempt) = attempt else { continue };
        let kind = s(item, "type");
        if matches!(kind, "user_message" | "system_notice" | "notification") || event(kind) {
            continue;
        }
        let deferred_end = last.get(s(attempt, "id")).and_then(|(_, prev)| prev["deferredWork"]["lastOrdinal"].as_i64());
        if deferred_end.zip(item["ordinal"].as_i64()).is_some_and(|(end, at)| end > at) {
            continue;
        }
        last.insert(s(attempt, "id"), (attempt, item));
    }
    let mut replies = HashSet::new();
    let mut finished = HashSet::new();
    for &(attempt, item) in last.values() {
        let before_cut = ts(item, "completedAt").zip(ts(attempt, "completedAt")).is_some_and(|(done, cut)| done < cut);
        if s(item, "type") == "assistant_message" && item["streaming"] != true && s(item, "status") == "completed" && before_cut {
            replies.insert(s(item, "id"));
            finished.insert(s(attempt, "id"));
        }
    }
    let mut failed: HashSet<&str> = array(p, "runs").iter().filter(|r| s(r, "status") == "failed").map(|r| s(r, "id")).collect();
    failed.extend(
        items
            .iter()
            .filter(|i| s(i, "type") == "error" && s(i, "status") == "failed" && i["parentItemId"].is_null())
            .map(|i| s(i, "runId")),
    );
    let mut fold = HashMap::new();
    for (&item, attempt) in items.iter().zip(&attempts) {
        let &Some(attempt) = attempt else { continue };
        let kind = s(item, "type");
        let kept = matches!(kind, "user_message" | "handoff" | "system_notice" | "fork" | "secret_request" | "link_request" | "checkpoint")
            || LIFECYCLE.contains(&kind)
            || (kind == "subagent" && item_active(item))
            || crate::chat::details::html_render::reference(item).is_some();
        if !kept && !finished.contains(s(attempt, "id")) && !failed.contains(s(attempt, "runId")) {
            fold.insert(s(item, "id"), s(attempt, "id"));
        }
    }
    (fold, replies)
}

/// Items that stay in the timeline even inside a finished turn (MessagesTimeline.logic.ts
/// deriveTurnFolds keeps compactions, notifications and resource cards out of the fold).
fn escapes_fold(kind: &str) -> bool {
    matches!(
        kind,
        "error"
            | "notification"
            | "system_notice"
            | "proposed_plan"
            | "compaction"
            | "subagent"
            | "fork"
            | "run_interrupt_result"
            | "handoff"
            | "secret_request"
            | "link_request"
    )
}

/// Where "Edit from here" rewinds each prompt to, by user message item id: the ready checkpoint
/// after the run before it, or the thread's start checkpoint for the first run
/// (deriveRevertTurnCountByUserMessageId + findRevertCheckpoint). Inherited history, steers and
/// the turn still running have none, nor does a checkpoint the provider's conversation can't
/// return to; with older history unloaded (`paged`), the first loaded run is not the start.
pub fn revert_targets(p: &Value, paged: bool) -> HashMap<String, (String, String)> {
    let ready: Vec<&Value> = array(p, "checkpoints").iter().filter(|c| s(c, "status") == "ready").collect();
    let rewindable = rewindable(p);
    let start = ready.iter().rfind(|c| c["ordinalWithinScope"] == 0 && c["appRunOrdinal"].is_null()).filter(|_| !paged && rewindable.contains(&0));
    let after = |run: &str| ready.iter().rfind(|c| s(c, "runId") == run).filter(|c| c["appRunOrdinal"].as_i64().is_some_and(|n| rewindable.contains(&n)));
    let runs: HashMap<_, _> = array(p, "runs").iter().map(|r| (s(r, "id"), r)).collect();
    let entries: Vec<(&Value, bool)> = if p["visibleTurnItems"].is_array() {
        let mut visible: Vec<_> = array(p, "visibleTurnItems").iter().collect();
        visible.sort_by_key(|v| v["position"].as_i64().unwrap_or(0));
        visible.into_iter().map(|v| (&v["item"], s(v, "visibility") == "inherited")).collect()
    } else {
        let mut items: Vec<_> = array(p, "turnItems").iter().collect();
        items.sort_by_key(|v| v["ordinal"].as_i64().unwrap_or(0));
        items.into_iter().map(|i| (i, false)).collect()
    };
    let mut target: HashMap<&str, Option<&&Value>> = HashMap::new();
    let mut previous: Option<&str> = None;
    let mut out = HashMap::new();
    for (item, inherited) in entries {
        let kind = s(item, "type");
        let run = s(item, "runId");
        if inherited || run.is_empty() || !matches!(kind, "user_message" | "assistant_message" | "notification") {
            continue;
        }
        if !target.contains_key(run) {
            target.insert(run, match previous { None => start, Some(prev) => after(prev) });
            previous = Some(run);
        }
        if kind != "user_message"
            || !matches!(s(item, "inputIntent"), "turn_start" | "queued_turn" | "")
            || runs.get(run).is_some_and(|r| run_active(r))
        {
            continue;
        }
        if let Some(Some(c)) = target.get(run) {
            out.insert(s(item, "id").to_owned(), (s(c, "scopeId").to_owned(), s(c, "id").to_owned()));
        }
    }
    out
}

/// `deriveAvailableRevertTurnCounts`: the turn counts the provider conversation can rewind to —
/// the start, and each run whose active attempt last ran on the thread's active provider thread,
/// while that thread has a session on the thread's current instance.
fn rewindable(p: &Value) -> HashSet<i64> {
    let mut out = HashSet::new();
    let thread = &p["thread"];
    let active = s(thread, "activeProviderThreadId");
    let Some(provider) = array(p, "providerThreads").iter().find(|t| !active.is_empty() && s(t, "id") == active) else { return out };
    if s(provider, "providerSessionId").is_empty() || s(provider, "providerInstanceId") != s(&thread["modelSelection"], "instanceId") {
        return out;
    }
    out.insert(0);
    let turns = array(p, "providerTurns");
    for run in array(p, "runs") {
        let attempt = s(run, "activeAttemptId");
        if attempt.is_empty() {
            continue;
        }
        let latest = turns.iter().filter(|t| s(t, "runAttemptId") == attempt).max_by_key(|t| t["ordinal"].as_i64().unwrap_or(0));
        let turn = latest.or_else(|| {
            let id = s(array(p, "attempts").iter().find(|a| s(a, "id") == attempt)?, "providerTurnId");
            turns.iter().find(|t| !id.is_empty() && s(t, "id") == id)
        });
        if let (Some(_), Some(n)) = (turn.filter(|t| s(t, "providerThreadId") == active), run["ordinal"].as_i64()) {
            out.insert(n);
        }
    }
    out
}

/// The projection's turn items in timeline order.
fn ordered(p: &Value) -> Vec<&Value> {
    if p["visibleTurnItems"].is_array() {
        let mut visible: Vec<_> = array(p, "visibleTurnItems").iter().collect();
        visible.sort_by_key(|v| v["position"].as_i64().unwrap_or(0));
        visible.into_iter().map(|v| &v["item"]).collect()
    } else {
        let mut items: Vec<_> = array(p, "turnItems").iter().collect();
        items.sort_by_key(|v| v["ordinal"].as_i64().unwrap_or(0));
        items
    }
}

/// Superseded attempts' items, by id, with their attempt's id: a finished turn's open fold
/// nests each attempt's work under its own "Superseded attempt" (deriveSupersededAttemptFolds).
pub fn fold_attempts(p: &Value) -> HashMap<String, String> {
    let mut items = ordered(p);
    items.retain(|i| !hidden(i));
    superseded(p, &items).0.into_iter().map(|(item, attempt)| (item.to_owned(), attempt.to_owned())).collect()
}

pub fn derive(p: &Value, now: i64) -> Vec<Row<'_>> {
    derive_with_meta(p, now).0
}

/// The rows, and the replies that show their meta (showAssistantMeta): each turn's last reply
/// once the turn has settled. Other replies, such as a stopped turn's earlier ones, show none.
pub fn derive_with_meta(p: &Value, now: i64) -> (Vec<Row<'_>>, HashSet<&str>) {
    let mut items = ordered(p);
    if items.is_empty() && !p["visibleTurnItems"].is_array() && !array(p, "messages").is_empty() {
        let messages = array(p, "messages");
        let meta: HashSet<&str> = messages
            .iter()
            .enumerate()
            .filter(|(at, m)| s(m, "role") == "assistant" && messages.get(at + 1).is_none_or(|next| s(next, "role") == "user"))
            .map(|(_, m)| s(m, "id"))
            .collect();
        let rows: Vec<Row> = messages.iter().map(|m| if s(m, "role") == "user" { Row::User(m) } else { Row::Assistant(m) }).collect();
        return (rows, meta);
    }
    let pending: HashMap<_, _> = array(p, "runtimeRequests")
        .iter()
        .filter(|r| s(r, "status") == "pending")
        .map(|r| (s(r, "id"), r))
        .collect();
    let runs: HashMap<_, _> = array(p, "runs").iter().map(|r| (s(r, "id"), r)).collect();
    let active = |run_id: &str| runs.get(run_id).is_some_and(|r| run_active(r));
    let shadowed = shadowed_by_approval(&items, &pending);
    items.retain(|i| !hidden(i) && !shadowed.contains(s(i, "id")));
    // A stopped turn stays unfolded, as the web leaves interrupted runs open.
    let interrupted: HashSet<&str> = items
        .iter()
        .filter(|i| matches!(s(i, "type"), "run_interrupt_request" | "run_interrupt_result"))
        .map(|i| s(i, "runId"))
        .filter(|r| !r.is_empty())
        .collect();
    // Each item's turn: its run, or for runless (imported) items the prompt that started them.
    let mut keys = Vec::with_capacity(items.len());
    let mut prompt = String::from("runless:start");
    for item in &items {
        let run_id = s(item, "runId");
        if run_id.is_empty() && s(item, "type") == "user_message" {
            prompt = format!("runless:{}", s(item, "id"));
        }
        keys.push(if run_id.is_empty() { prompt.clone() } else { run_id.to_owned() });
    }
    // The turn's terminal reply: its last assistant message.
    let mut terminal: HashMap<&str, &str> = HashMap::new();
    for (item, key) in items.iter().zip(&keys) {
        if s(item, "type") == "assistant_message" {
            terminal.insert(key.as_str(), s(item, "id"));
        }
    }
    let mut meta = HashSet::new();
    for (&item, key) in items.iter().zip(&keys) {
        let run = s(item, "runId");
        if s(item, "type") == "assistant_message" && terminal.get(key.as_str()) == Some(&s(item, "id")) && (run.is_empty() || !active(run)) {
            meta.insert(s(item, "id"));
        }
    }
    let (cut, replies) = superseded(p, &items);
    let compacting: HashSet<&str> = items
        .iter()
        .filter(|i| s(i, "type") == "compaction" && s(i, "status") == "running")
        .map(|i| s(i, "runId"))
        .filter(|r| !r.is_empty())
        .collect();
    let mut rows = Vec::new();
    let mut seen_requests = HashSet::new();
    let mut working = HashSet::new();
    // Each turn's first fold, which times the whole turn, and the fold its next work joins.
    let mut folds: HashMap<&str, usize> = HashMap::new();
    let mut current: HashMap<&str, usize> = HashMap::new();
    // A steer after the turn's first work starts a fold of its own (deriveTurnFolds steerSegments).
    let mut steered: HashMap<&str, &Value> = HashMap::new();
    let mut segments: HashMap<usize, (&Value, Option<i64>)> = HashMap::new();
    let mut attempts: HashMap<&str, usize> = HashMap::new();
    let mut spans: HashMap<&str, (Option<i64>, Option<i64>)> = HashMap::new();
    let mut latest: HashMap<&str, &Value> = HashMap::new();
    // Turns past their terminal reply: a page rendered after it stays in view (deriveTurnFolds).
    let mut replied: HashSet<&str> = HashSet::new();
    for (item, key) in items.drain(..).zip(&keys) {
        let kind = s(item, "type");
        let run_id = s(item, "runId");
        let after_reply = replied.contains(key.as_str());
        if kind == "assistant_message" && terminal.get(key.as_str()) == Some(&s(item, "id")) {
            replied.insert(key.as_str());
        }
        let live = !run_id.is_empty() && active(run_id);
        let unfolded = live || interrupted.contains(run_id);
        if live && kind != "user_message" {
            latest.insert(run_id, item);
        }
        // Notices that open a run (a fork's context) sit above its prompt, not under "Working".
        if live && kind != "user_message" && !escapes_fold(kind) && working.insert(run_id) {
            rows.push(working_row(runs[run_id], now, compacting.contains(run_id)));
        }
        if let Some(request) = pending.get(s(item, "requestId")) {
            rows.push(Row::Request { request, item: Some(item) });
            seen_requests.insert(s(*request, "id"));
        } else if kind == "user_message" {
            rows.push(Row::User(item));
            if live && working.insert(run_id) {
                rows.push(working_row(runs[run_id], now, compacting.contains(run_id)));
            }
            // Back-to-back steers split once, after the last of them.
            if steers(item) && current.contains_key(key.as_str()) {
                steered.insert(key.as_str(), item);
            }
        } else if LIFECYCLE.contains(&kind) {
        } else if kind == "checkpoint" {
            // Its files show on the changed-files card under the run's last reply.
        } else if let Some(&attempt) = cut.get(s(item, "id")).filter(|_| unfolded || escapes_fold(kind)) {
            match attempts.get(attempt) {
                Some(&index) => {
                    if let Row::Attempt { items, .. } = &mut rows[index] {
                        items.push(item);
                    }
                }
                None => {
                    attempts.insert(attempt, rows.len());
                    rows.push(Row::Attempt { id: format!("attempt-fold:{attempt}"), items: vec![item] });
                }
            }
        } else if kind == "assistant_message"
            && (live || terminal.get(key.as_str()) == Some(&s(item, "id")) || replies.contains(s(item, "id")))
        {
            rows.push(Row::Assistant(item));
        } else if unfolded || escapes_fold(kind) || (after_reply && crate::chat::details::html_render::reference(item).is_some()) {
            rows.push(if kind == "assistant_message" { Row::Assistant(item) } else { Row::Item(item) });
        } else {
            let span = spans.entry(key.as_str()).or_default();
            let start = ts(item, "startedAt").or_else(|| ts(item, "createdAt"));
            let end = ts(item, "completedAt").or_else(|| ts(item, "updatedAt")).or(start);
            span.0 = span.0.or(start);
            span.1 = span.1.max(end);
            let steer = steered.remove(key.as_str());
            match current.get(key.as_str()) {
                Some(&index) if steer.is_none() => {
                    if let Row::Work { items, .. } = &mut rows[index] {
                        items.push(item);
                    }
                }
                _ => {
                    let id = match steer {
                        Some(steer) => {
                            segments.insert(rows.len(), (steer, None));
                            format!("work:{key}:steer:{}", s(steer, "id"))
                        }
                        None => {
                            folds.insert(key.as_str(), rows.len());
                            format!("work:{key}")
                        }
                    };
                    current.insert(key.as_str(), rows.len());
                    rows.push(Row::Work { id, label: String::new(), items: vec![item] });
                }
            }
            if let Some(segment) = current.get(key.as_str()).and_then(|index| segments.get_mut(index)) {
                segment.1 = segment.1.max(end);
            }
        }
    }
    let latest = array(p, "runs").iter().max_by_key(|r| r["ordinal"].as_i64().unwrap_or(0)).map(|r| s(r, "id"));
    for (key, index) in folds {
        let run = runs.get(key).copied();
        // The latest turn stopped without an interrupt item (those stay unfolded).
        let stopped = run.is_some_and(|r| s(r, "status") == "interrupted") && latest == Some(key);
        let (first, last) = spans.get(key).copied().unwrap_or_default();
        let start = run.and_then(|r| ts(r, "startedAt")).or(first);
        let end = run.and_then(|r| ts(r, "completedAt")).or(last);
        if let Row::Work { label, .. } = &mut rows[index] {
            *label = match (start, end, stopped) {
                (Some(start), Some(end), true) => format!("You stopped after {}", duration(start, end)),
                (.., true) => "You stopped this response".to_owned(),
                (Some(start), Some(end), false) => format!("Worked for {}", duration(start, end)),
                _ => "Worked".to_owned(),
            };
        }
    }
    for (index, (steer, end)) in segments {
        if let Row::Work { label, .. } = &mut rows[index] {
            *label = match ts(steer, "createdAt") {
                Some(start) => format!("Continued for {}", duration(start, end.unwrap_or(start))),
                None => "Continued".to_owned(),
            };
        }
    }
    for (id, request) in &pending {
        if !seen_requests.contains(*id) {
            rows.push(Row::Request { request, item: None });
        }
    }
    for run in array(p, "runs") {
        if run_active(run) && !working.contains(s(run, "id")) {
            rows.push(working_row(run, now, compacting.contains(s(run, "id"))));
        }
    }
    let settled = |reply: &Value| {
        let run = s(reply, "runId");
        !run.is_empty() && !active(run) && terminal.get(run) == Some(&s(reply, "id"))
    };
    let mut rows = attach_meta(group_tools(rows), settled);
    // Nothing thinks while a request waits on the user or the context compacts (ThinkingTimelineRow).
    if pending.is_empty() {
        for run in array(p, "runs").iter().filter(|r| run_active(r)) {
            let id = s(run, "id");
            if compacting.contains(id) || latest.get(id).is_some_and(|i| item_active(i) || i["streaming"] == true) {
                continue;
            }
            rows.push(Row::Thinking { id: format!("thinking:{id}") });
        }
    }
    (rows, meta)
}

/// Runs of two or more groupable items of one turn, outside any fold, become one tool group;
/// adjacent subagents of one provider turn become one subagent group.
fn group_tools(rows: Vec<Row<'_>>) -> Vec<Row<'_>> {
    fn same_turn(a: &Value, b: &Value) -> bool {
        s(a, "runId") == s(b, "runId") && s(a, "providerTurnId") == s(b, "providerTurnId")
    }
    let mut out: Vec<Row> = Vec::with_capacity(rows.len());
    for row in rows {
        let item = match row {
            Row::Item(item) if groupable(item) || s(item, "type") == "subagent" => item,
            other => {
                out.push(other);
                continue;
            }
        };
        if s(item, "type") == "subagent" {
            match out.last_mut() {
                Some(Row::Subagents { items, .. }) if same_turn(items[0], item) => items.push(item),
                Some(Row::Item(prev)) if s(prev, "type") == "subagent" && same_turn(prev, item) => {
                    let prev = *prev;
                    out.pop();
                    out.push(Row::Subagents { id: s(prev, "id").to_owned(), items: vec![prev, item] });
                }
                _ => out.push(Row::Item(item)),
            }
            continue;
        }
        match out.last_mut() {
            Some(Row::Tools { items, .. }) if s(items[0], "runId") == s(item, "runId") => items.push(item),
            Some(Row::Item(prev)) if groupable(prev) && s(prev, "runId") == s(item, "runId") => {
                let prev = *prev;
                out.pop();
                out.push(Row::Tools { id: format!("tools:{}", s(prev, "id")), items: vec![prev, item] });
            }
            _ => out.push(Row::Item(item)),
        }
    }
    out
}

/// attachTrailingToolGroupsToAssistant: a turn's settled last reply followed by its own tool
/// work outside any fold (a stopped or failed turn) takes its meta row below that work.
fn attach_meta<'a>(rows: Vec<Row<'a>>, settled: impl Fn(&Value) -> bool) -> Vec<Row<'a>> {
    let mut after: HashMap<usize, &'a Value> = HashMap::new();
    for (index, row) in rows.iter().enumerate() {
        let &Row::Assistant(reply) = row else { continue };
        if !settled(reply) {
            continue;
        }
        let run = s(reply, "runId");
        let (mut tools, mut last) = (false, None);
        for (at, next) in rows.iter().enumerate().skip(index + 1) {
            match next {
                Row::User(_) | Row::Assistant(_) => break,
                Row::Tools { items, .. } if s(items[0], "runId") == run => {
                    tools = true;
                    last = Some(at);
                }
                Row::Item(item) if s(item, "runId") == run && work_entry(item) => {
                    tools |= groupable(item) || (s(item, "type") == "error" && s(item, "status") == "failed");
                    if tools {
                        last = Some(at);
                    }
                }
                _ => {}
            }
        }
        if let Some(at) = last {
            after.insert(at, reply);
        }
    }
    if after.is_empty() {
        return rows;
    }
    let mut out = Vec::with_capacity(rows.len() + after.len());
    for (index, row) in rows.into_iter().enumerate() {
        out.push(row);
        if let Some(reply) = after.remove(&index) {
            out.push(Row::Meta { id: format!("assistant-meta:{}", s(reply, "id")), item: reply });
        }
    }
    out
}

/// Items the web keeps as work-log entries rather than lifecycle events.
fn work_entry(item: &Value) -> bool {
    (groupable(item) && s(item, "type") != "thread_created")
        || matches!(s(item, "type"), "error" | "notification" | "system_notice" | "todo_list" | "approval_request" | "user_input_request")
}

/// Items the web timeline leaves out (session-logic.ts deriveTimelineEntries): workspace
/// preparation (shown by its own card), and prompts still waiting in the queue (shown in the
/// queue under the timeline).
fn hidden(item: &Value) -> bool {
    let kind = s(item, "type");
    (kind == "command_execution" && s(item, "input") == "Preparing workspace" && item["deferredWork"].is_null())
        || (kind == "error" && s(item, "status") == "cancelled" && s(&item["failure"], "code") == "workspace_preparation_failed")
        || (kind == "user_message" && s(item, "inputIntent") == "queued_turn" && s(item, "status") == "pending")
}

/// A tool waiting on approval shows once, as its approval row: the unfinished tool item the
/// pending request was raised for (the same native item) stays out, as the web's neutral
/// in-progress entries do.
fn shadowed_by_approval<'a>(items: &[&'a Value], pending: &HashMap<&str, &Value>) -> HashSet<&'a str> {
    fn native(item: &Value) -> Option<&str> {
        item["nativeItemRef"]["nativeId"].as_str().filter(|n| !n.is_empty())
    }
    let mut asked = HashSet::new();
    for &item in items {
        if s(item, "type") == "approval_request" && pending.contains_key(s(item, "requestId")) {
            asked.extend(native(item));
        }
    }
    let mut out = HashSet::new();
    for &item in items {
        let tool = !matches!(s(item, "type"), "approval_request" | "user_input_request");
        let open = matches!(s(item, "status"), "pending" | "running" | "waiting");
        if tool && open && native(item).is_some_and(|n| asked.contains(n)) {
            out.insert(s(item, "id"));
        }
    }
    out
}

/// toolActivity.ts `toolNameToken`: namespaced names (`mcp__x`, `a.b`, `a/b`) classify as nothing.
pub fn tool_token(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() || name.contains("__") || name.contains(['.', '/']) {
        return None;
    }
    Some(name.chars().filter(|c| !matches!(c, '_' | '-') && !c.is_whitespace()).collect::<String>().to_lowercase())
}

const PATH_KEYS: [&str; 8] = ["path", "filePath", "file_path", "relativePath", "filename", "fileName", "newPath", "oldPath"];

/// toolActivity.ts `collectToolFilePaths`: up to eight distinct paths, four levels deep.
fn tool_paths(value: &Value) -> Vec<&str> {
    fn walk<'a>(value: &'a Value, out: &mut Vec<&'a str>, depth: usize) {
        if depth > 4 || out.len() >= 8 {
            return;
        }
        if let Some(list) = value.as_array() {
            for entry in list {
                walk(entry, out, depth + 1);
                if out.len() >= 8 {
                    return;
                }
            }
            return;
        }
        let Some(record) = value.as_object() else { return };
        for key in PATH_KEYS {
            let Some(path) = record.get(key).and_then(Value::as_str).map(str::trim).filter(|p| !p.is_empty()) else { continue };
            if !out.contains(&path) {
                out.push(path);
                if out.len() >= 8 {
                    return;
                }
            }
        }
        for key in ["locations", "item", "input", "result", "rawInput", "data", "changes"] {
            if let Some(nested) = record.get(key) {
                walk(nested, out, depth + 1);
                if out.len() >= 8 {
                    return;
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(value, &mut out, 0);
    out
}

/// toolActivity.ts `formatSearchToolLabel`, from a search tool's input.
fn search_label(input: &Value) -> Option<String> {
    let first = |keys: &[&str]| keys.iter().find_map(|k| input[*k].as_str().map(str::trim).filter(|v| !v.is_empty()));
    let query = first(&["pattern", "query", "searchTerm", "regex", "grep", "needle"]);
    let glob = first(&["glob", "globPattern", "glob_pattern", "include", "filePattern", "file_pattern"]);
    let target = first(&["path", "target_directory", "targetDirectory", "directory", "cwd", "root"])
        .and_then(|path| path.split(['/', '\\']).filter(|part| !part.is_empty() && *part != ".").last());
    match (query, glob, target) {
        (Some(query), _, Some(target)) => Some(format!("Searched {query} in {target}")),
        (_, Some(glob), Some(target)) => Some(format!("Searched files {glob} in {target}")),
        (_, Some(glob), None) => Some(format!("Searched files {glob}")),
        (Some(query), None, None) => Some(format!("Searched {query}")),
        (None, None, Some(target)) => Some(format!("Searched in {target}")),
        _ => None,
    }
}

/// `normalizeCompactToolLabel`: a trailing " complete(d)" dropped.
fn compact_tool_label(label: &str) -> &str {
    let trimmed = label.trim();
    let lower = trimmed.to_ascii_lowercase();
    for suffix in ["completed", "complete"] {
        if lower.ends_with(suffix) {
            let head = &trimmed[..trimmed.len() - suffix.len()];
            if head.ends_with(char::is_whitespace) {
                return head.trim();
            }
        }
    }
    trimmed
}

/// workEntryDisplayLabel for a dynamic tool: what a search looked for, the files a read opened
/// ("Read a.rs +2 more"), a skill or script's own title, else its capitalized title or name.
fn dynamic_label(item: &Value, root: Option<&str>) -> Option<String> {
    let tool = s(item, "toolName");
    let input = &item["input"];
    let token = tool_token(tool);
    if matches!(token.as_deref(), Some("find" | "grep" | "glob" | "rg" | "ls"))
        && let Some(label) = search_label(input)
    {
        return Some(label);
    }
    let image = !item["viewedImagePath"].is_null();
    let read = image || matches!(token.as_deref(), Some("read" | "readfile"));
    let paths = if read { tool_paths(input) } else { Vec::new() };
    if let Some((first, rest)) = paths.split_first() {
        let more = if rest.is_empty() { String::new() } else { format!(" +{} more", rest.len()) };
        return Some(format!("Read {}{more}", workspace_path(first, root)));
    }
    if tool == "Skill"
        && let Some(name) = input["skill"].as_str().map(str::trim).filter(|n| !n.is_empty())
    {
        return Some(format!("Skill: {name}"));
    }
    if matches!(tool, "js" | "cua_repl.js" | "node_repl.js") {
        let own = input["title"].as_str().map(str::trim).filter(|t| !t.is_empty());
        let kept = Some(s(item, "title").trim()).filter(|t| !t.is_empty() && !t.eq_ignore_ascii_case("js") && !t.eq_ignore_ascii_case(tool));
        let fallback = if tool == "cua_repl.js" { "Computer use" } else { "Run JavaScript" };
        return Some(own.or(kept).unwrap_or(fallback).to_owned());
    }
    if read && !image {
        return Some("Read file".into());
    }
    let heading = compact_tool_label([s(item, "title"), tool].into_iter().map(str::trim).find(|v| !v.is_empty())?);
    let mut chars = heading.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect())
}

/// A work row's label when it has no better one (session-logic.ts projectedWorkEntry), with
/// paths read relative to the workspace at `root`.
pub fn entry_label(item: &Value, root: Option<&str>) -> String {
    if s(item, "type") == "dynamic_tool"
        && let Some(label) = dynamic_label(item, root)
    {
        return label;
    }
    if let Some(title) = item["title"].as_str().map(str::trim).filter(|t| !t.is_empty()) {
        return title.to_owned();
    }
    let kind = s(item, "type");
    match kind {
        "thread_created" => "Created thread".into(),
        "reasoning" => "Thinking".into(),
        "command_execution" => "Ran command".into(),
        "file_change" => match array(item, "changes").len() {
            n if n > 1 => format!("Changed {n} files"),
            _ => format!("Changed {}", s(item, "fileName")),
        },
        "file_search" => "Searched files".into(),
        "web_search" => "Searched the web".into(),
        "checkpoint" => "Checkpoint captured".into(),
        "dynamic_tool" => item["toolName"].as_str().filter(|n| !n.is_empty()).unwrap_or("Tool call").into(),
        "approval_request" => "Approval requested".into(),
        "user_input_request" if !item["questionAnswer"].is_null() => "Answered questions".into(),
        "user_input_request" => "Input requested".into(),
        "notification" if !s(item, "summary").is_empty() => s(item, "summary").into(),
        _ => kind.replace('_', " "),
    }
}

/// WorkingTimelineRow: "Working for 3s", or "Compacting for 3s" while the context compacts;
/// without a start time, "Working..." and "Compacting…".
fn working_row(run: &Value, now: i64, compacting: bool) -> Row<'static> {
    let start = ts(run, "startedAt").or_else(|| ts(run, "requestedAt"));
    let label = match (compacting, start) {
        (true, Some(start)) => format!("Compacting for {}", duration(start, now)),
        (true, None) => "Compacting…".to_owned(),
        (false, Some(start)) => format!("Working for {}", duration(start, now)),
        (false, None) => "Working...".to_owned(),
    };
    Row::Working { id: format!("working:{}", s(run, "id")), label, compacting }
}

/// `path` as the web shows it (filePathDisplay.ts formatWorkspaceRelativePath): inside the
/// workspace it reads `demo/src/clamp.ts`, named by the workspace folder; elsewhere unchanged.
pub fn workspace_path(path: &str, root: Option<&str>) -> String {
    let Some(root) = root.map(|r| r.trim_end_matches('/')).filter(|r| !r.is_empty()) else {
        return path.to_owned();
    };
    let label = root.rsplit('/').next().unwrap_or(root);
    if path == root {
        label.to_owned()
    } else if let Some(rest) = path.strip_prefix(root).and_then(|r| r.strip_prefix('/')) {
        format!("{label}/{rest}")
    } else if !path.starts_with('/') {
        let mut relative = path;
        while let Some(rest) = relative.strip_prefix("./") {
            relative = rest;
        }
        if path.starts_with(&format!("{label}/")) { path.to_owned() } else { format!("{label}/{relative}") }
    } else {
        path.to_owned()
    }
}

/// A todo list's work-row label: `Tasks {done}/{n} · {current step}` (session-logic.ts).
pub fn todo_label(item: &Value) -> String {
    let steps = array(item, "steps");
    let done = steps.iter().filter(|step| s(step, "status") == "completed").count();
    let current = ["running", "pending"].iter().find_map(|status| steps.iter().find(|step| s(step, "status") == *status));
    match current {
        Some(step) => format!("Tasks {done}/{} · {}", steps.len(), s(step, "text")),
        None => format!("Tasks {done}/{}", steps.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    fn label(item: &Value) -> String {
        entry_label(item, None)
    }

    #[test]
    fn todo_label_counts_done_steps_and_names_the_current_one() {
        let steps = |a: &str, b: &str, c: &str| json!({"steps":[{"text":"Read","status":a},{"text":"Edit","status":b},{"text":"Test","status":c}]});
        assert_eq!(todo_label(&steps("completed", "pending", "running")), "Tasks 1/3 · Test");
        assert_eq!(todo_label(&steps("completed", "pending", "pending")), "Tasks 1/3 · Edit");
        assert_eq!(todo_label(&steps("completed", "completed", "completed")), "Tasks 3/3");
    }

    #[test]
    fn folds_intermediate_reply_and_work_but_keeps_final_reply() {
        let p = json!({"runs":[{"id":"r","startedAt":"1970-01-01T00:00:00Z","completedAt":"1970-01-01T00:00:22Z"}],"turnItems":[
            {"id":"u","type":"user_message","runId":"r","ordinal":1},
            {"id":"tool","type":"command_execution","runId":"r","ordinal":2},
            {"id":"commentary","type":"assistant_message","runId":"r","ordinal":3},
            {"id":"final","type":"assistant_message","runId":"r","ordinal":4}]});
        let rows = derive(&p, 0);
        assert_eq!(rows.len(), 3);
        assert!(
            matches!(&rows[1],Row::Work { label,items,.. } if label == "Worked for 22s" && items.len()==2)
        );
        assert!(matches!(&rows[2],Row::Assistant(v) if s(v,"id")=="final"));
        let mut stopped = p.clone();
        stopped["runs"][0]["status"] = json!("interrupted");
        assert!(matches!(&derive(&stopped, 0)[1], Row::Work { label, .. } if label == "You stopped after 22s"));
    }
    #[test]
    fn a_page_rendered_after_the_final_reply_stays_out_of_the_fold() {
        let render = |id: &str, ordinal: i64| {
            let page = json!({"htmlRender": {"attachmentId": id, "title": "Chart", "height": 300}});
            json!({"id": id, "type": "dynamic_tool", "toolName": "mcp__t3-code__html_render", "status": "completed", "runId": "r", "ordinal": ordinal, "output": {"structuredContent": page}})
        };
        let p = json!({"runs":[{"id":"r","status":"completed"}],"turnItems":[
            {"id":"u","type":"user_message","runId":"r","ordinal":1},
            render("early", 2),
            {"id":"final","type":"assistant_message","runId":"r","ordinal":3},
            render("late", 4)]});
        let rows = derive(&p, 0);
        assert!(matches!(&rows[1], Row::Work { items, .. } if items.len() == 1 && s(items[0], "id") == "early"));
        assert!(matches!(&rows[2], Row::Assistant(v) if s(v, "id") == "final"));
        assert!(matches!(rows.last(), Some(Row::Item(v)) if s(v, "id") == "late"));
    }

    #[test]
    fn pending_approval_and_errors_escape_collapsed_work() {
        let p = json!({"runtimeRequests":[{"id":"q","status":"pending"}],"turnItems":[
            {"id":"t","type":"reasoning","ordinal":1},
            {"id":"a","type":"approval_request","requestId":"q","ordinal":2},
            {"id":"e","type":"error","ordinal":3},{"id":"compact","type":"compaction","ordinal":4},
            {"id":"f","type":"fork","title":"Forked from conversation","ordinal":5}]});
        let rows = derive(&p, 0);
        assert!(matches!(
            rows.as_slice(),
            [
                Row::Work { .. },
                Row::Request { .. },
                Row::Item(_),
                Row::Item(_),
                Row::Item(_)
            ]
        ));
    }
    #[test]
    fn visible_projection_is_authoritative_and_position_orders_it() {
        let p = json!({"visibleTurnItems":[{"position":2,"item":{"id":"a","type":"assistant_message"}},{"position":0,"item":{"id":"u","type":"user_message"}}],"turnItems":[{"type":"error"}]});
        assert!(matches!(
            derive(&p, 0).as_slice(),
            [Row::User(_), Row::Assistant(_)]
        ));
        assert!(
            derive(
                &json!({"visibleTurnItems":[],"messages":[{"role":"user"}]}),
                0
            )
            .is_empty()
        );
    }
    #[test]
    fn empty_checkpoint_does_not_create_an_extra_fold_and_starting_runs_show_activity() {
        let p = json!({"turnItems":[{"type":"assistant_message","id":"a","ordinal":1},{"type":"checkpoint","id":"c","files":[],"ordinal":2}]});
        assert!(matches!(derive(&p, 0).as_slice(), [Row::Assistant(_)]));
        let p =
            json!({"runs":[{"id":"r","status":"starting","requestedAt":"1970-01-01T00:00:00Z"}]});
        assert!(matches!(derive(&p, 2000).as_slice(), [Row::Working { label, .. }, Row::Thinking { .. }]
            if label == "Working for 2s"));
        let p = json!({"runs":[{"id":"r","status":"starting"}]});
        assert!(matches!(derive(&p, 2000).as_slice(), [Row::Working { label, .. }, Row::Thinking { .. }] if label == "Working..."));
        // A compacting turn says so instead of thinking.
        let p = json!({"runs":[{"id":"r","status":"running","startedAt":"1970-01-01T00:00:00Z"}],
            "turnItems":[{"id":"c","type":"compaction","runId":"r","status":"running","ordinal":1}]});
        assert!(matches!(derive(&p, 3000).as_slice(), [Row::Item(_), Row::Working { label, compacting: true, .. }] if label == "Compacting for 3s"));
    }
    #[test]
    fn a_live_turn_has_one_working_row_after_its_prompt_and_folds_once_it_ends() {
        let items = json!([
            {"id":"fc","type":"notification","runId":"r","ordinal":0},
            {"id":"u","type":"user_message","runId":"r","ordinal":1},
            {"id":"t1","type":"command_execution","runId":"r","ordinal":2},
            {"id":"a1","type":"assistant_message","runId":"r","ordinal":3},
            {"id":"t2","type":"file_change","runId":"r","ordinal":4},
            {"id":"a2","type":"assistant_message","runId":"r","ordinal":5}]);
        let live = json!({"runs":[{"id":"r","status":"running","startedAt":"1970-01-01T00:00:00Z"}],"turnItems":items});
        let rows = derive(&live, 5000);
        let ids: Vec<_> = rows
            .iter()
            .map(|r| match r {
                Row::User(v) | Row::Assistant(v) | Row::Item(v) => s(v, "id").to_owned(),
                Row::Working { label, .. } => label.clone(),
                Row::Thinking { .. } => "Thinking".to_owned(),
                _ => "?".to_owned(),
            })
            .collect();
        assert_eq!(ids, ["fc", "u", "Working for 5s", "t1", "a1", "t2", "a2", "Thinking"]);
        let done = json!({"runs":[{"id":"r","status":"completed","startedAt":"1970-01-01T00:00:00Z","completedAt":"1970-01-01T00:00:09Z"}],"turnItems":items});
        let rows = derive(&done, 60000);
        assert!(matches!(rows.as_slice(), [Row::Item(_), Row::User(_), Row::Work { id, label, items }, Row::Assistant(a)]
            if id == "work:r" && label == "Worked for 9s" && items.len() == 3 && s(a, "id") == "a2"));
        let stopped = json!({"runs":[{"id":"r","status":"interrupted"}],"turnItems":[
            {"id":"u","type":"user_message","runId":"r","ordinal":1},
            {"id":"t1","type":"command_execution","runId":"r","ordinal":2},
            {"id":"x","type":"run_interrupt_request","runId":"r","ordinal":3}]});
        assert!(matches!(derive(&stopped, 0).as_slice(), [Row::User(_), Row::Item(_)]));
    }
    #[test]
    fn live_tool_runs_group_and_thinking_shows_only_while_idle() {
        let items = json!([
            {"id":"u","type":"user_message","runId":"r","ordinal":1},
            {"id":"t1","type":"command_execution","runId":"r","status":"completed","ordinal":2},
            {"id":"t2","type":"dynamic_tool","runId":"r","status":"completed","ordinal":3},
            {"id":"t3","type":"reasoning","runId":"r","status":"completed","ordinal":4},
            {"id":"n","type":"error","runId":"r","ordinal":5},
            {"id":"t4","type":"command_execution","runId":"r","status":"running","ordinal":6}]);
        let p = json!({"runs":[{"id":"r","status":"running","startedAt":"1970-01-01T00:00:00Z"}],"turnItems":items});
        assert!(matches!(derive(&p, 9500).as_slice(), [Row::User(_), Row::Working { .. }, Row::Tools { id, items }, Row::Item(e), Row::Item(t)]
            if id == "tools:t1" && items.len() == 3 && s(e, "id") == "n" && s(t, "id") == "t4"));
        let mut idle = p.clone();
        idle["turnItems"][5]["status"] = json!("completed");
        idle["turnItems"][5]["completedAt"] = json!("1970-01-01T00:00:06Z");
        assert!(matches!(derive(&idle, 9500).last(), Some(Row::Thinking { .. })));
        let mut asked = idle.clone();
        asked["runtimeRequests"] = json!([{"id":"q","status":"pending"}]);
        assert!(!derive(&asked, 9500).iter().any(|r| matches!(r, Row::Thinking { .. })));
    }

    #[test]
    fn prompts_rewind_to_the_checkpoint_before_their_run() {
        let mut p = json!({
            "thread": {"activeProviderThreadId": "pt", "modelSelection": {"instanceId": "codex"}},
            "providerThreads": [{"id": "pt", "providerSessionId": "sess", "providerInstanceId": "codex"}],
            "runs": [
                {"id":"r1","status":"completed","ordinal":1,"activeAttemptId":"a1"},
                {"id":"r2","status":"completed","ordinal":2,"activeAttemptId":"a2"},
                {"id":"r3","status":"running","ordinal":3,"activeAttemptId":"a3"}
            ],
            "attempts": [{"id":"a1","providerTurnId":null},{"id":"a2","providerTurnId":"t2"}],
            "providerTurns": [
                {"id":"t1a","runAttemptId":"a1","ordinal":1,"providerThreadId":"old"},
                {"id":"t1b","runAttemptId":"a1","ordinal":2,"providerThreadId":"pt"},
                {"id":"t2","runAttemptId":null,"ordinal":3,"providerThreadId":"pt"}
            ],
            "checkpoints": [
                {"id":"c0","scopeId":"s","runId":null,"status":"ready","ordinalWithinScope":0,"appRunOrdinal":null},
                {"id":"c1","scopeId":"s","runId":"r1","status":"ready","ordinalWithinScope":1,"appRunOrdinal":1},
                {"id":"c2","scopeId":"s","runId":"r2","status":"error","ordinalWithinScope":2,"appRunOrdinal":2}
            ],
            "turnItems": [
                {"type":"user_message","id":"u1","runId":"r1","inputIntent":"turn_start","ordinal":1},
                {"type":"assistant_message","id":"a1","runId":"r1","ordinal":2},
                {"type":"user_message","id":"u2","runId":"r2","inputIntent":"turn_start","ordinal":3},
                {"type":"user_message","id":"s2","runId":"r2","inputIntent":"steer","ordinal":4},
                {"type":"user_message","id":"u3","runId":"r3","inputIntent":"turn_start","ordinal":5}
            ]
        });
        let t = revert_targets(&p, false);
        assert_eq!(t.get("u1"), Some(&("s".to_owned(), "c0".to_owned())));
        assert_eq!(t.get("u2"), Some(&("s".to_owned(), "c1".to_owned())));
        // A steer is not a turn start; the running turn can't be rewound; r2's checkpoint failed.
        assert_eq!(t.len(), 2);
        assert_eq!(rewindable(&p), HashSet::from([0, 1, 2]), "FAILED: a run counts by its latest provider turn, else its attempt's");

        assert!(!revert_targets(&p, true).contains_key("u1"), "FAILED: a paged window's first run is not the thread start");
        p["providerTurns"][1]["providerThreadId"] = json!("old");
        assert_eq!(revert_targets(&p, false).keys().collect::<Vec<_>>(), ["u1"], "FAILED: r1 last ran on another provider thread");
        p["thread"]["modelSelection"]["instanceId"] = json!("claude");
        assert!(revert_targets(&p, false).is_empty(), "FAILED: a switched provider can't rewind its conversation");
    }

    #[test]
    fn approval_shadows_its_tool_and_queued_prompts_and_workspace_prep_stay_out() {
        let native = |n: &str| json!({"driver":"acpRegistry","nativeId":n});
        let p = json!({"runtimeRequests":[{"id":"q","status":"pending"}],"turnItems":[
            {"id":"prep","type":"command_execution","input":"Preparing workspace","status":"completed","ordinal":1},
            {"id":"tool","type":"command_execution","input":"printf x","status":"pending","nativeItemRef":native("w"),"ordinal":2},
            {"id":"other","type":"command_execution","input":"ls","status":"pending","nativeItemRef":native("v"),"ordinal":3},
            {"id":"a","type":"approval_request","requestId":"q","nativeItemRef":native("w"),"status":"waiting","ordinal":4},
            {"id":"later","type":"user_message","inputIntent":"queued_turn","status":"pending","ordinal":5}]});
        let ids: Vec<_> = derive(&p, 0)
            .iter()
            .map(|r| match r {
                Row::Request { item: Some(i), .. } => s(i, "id").to_owned(),
                Row::Work { items, .. } => items.iter().map(|i| s(i, "id")).collect::<Vec<_>>().join("+"),
                _ => "?".to_owned(),
            })
            .collect();
        assert_eq!(ids, ["other", "a"]);
    }

    #[test]
    fn work_rows_without_a_title_read_like_the_web() {
        assert_eq!(label(&json!({"type":"approval_request","title":null})), "Approval requested");
        assert_eq!(label(&json!({"type":"approval_request","title":"  Run it "})), "Run it");
        assert_eq!(label(&json!({"type":"file_change","fileName":"a.ts"})), "Changed a.ts");
        assert_eq!(label(&json!({"type":"file_change","changes":[{},{}]})), "Changed 2 files");
        assert_eq!(label(&json!({"type":"dynamic_tool","toolName":null})), "Tool call");
        assert_eq!(label(&json!({"type":"mcp_tool_call"})), "mcp tool call");
        let restart = json!({"type":"notification","title":null,"summary":"T3 Code restarted and resumed this turn","detail":"Continue"});
        assert_eq!(label(&restart), "T3 Code restarted and resumed this turn");
    }

    #[test]
    fn paths_read_relative_to_the_workspace_folder() {
        let root = Some("/root/projects/demo/");
        assert_eq!(workspace_path("/root/projects/demo/src/clamp.ts", root), "demo/src/clamp.ts");
        assert_eq!(workspace_path("/root/projects/demo", root), "demo");
        assert_eq!(workspace_path("./src/a.ts", root), "demo/src/a.ts");
        assert_eq!(workspace_path("demo/src/a.ts", root), "demo/src/a.ts");
        assert_eq!(workspace_path("/etc/hosts", root), "/etc/hosts");
        assert_eq!(workspace_path("/root/projects/demo2/x", root), "/root/projects/demo2/x");
        assert_eq!(workspace_path("/a/b", None), "/a/b");
    }
    #[test]
    fn timer_clamps_and_formats_minutes() {
        assert_eq!(duration(2000, 0), "0s");
        assert_eq!(duration(0, 122000), "2m 2s");
    }

    #[test]
    fn work_after_a_steer_folds_on_its_own() {
        let at = |s: u32| format!("1970-01-01T00:00:{s:02}Z");
        let p = json!({"runs":[{"id":"r","status":"completed","startedAt":at(0),"completedAt":at(30)}],"turnItems":[
            {"id":"u","type":"user_message","runId":"r","ordinal":1},
            {"id":"t1","type":"command_execution","runId":"r","completedAt":at(5),"ordinal":2},
            {"id":"s1","type":"user_message","runId":"r","inputIntent":"steer","createdAt":at(10),"ordinal":3},
            {"id":"s2","type":"user_message","runId":"r","inputIntent":"steer","createdAt":at(12),"ordinal":4},
            {"id":"t2","type":"command_execution","runId":"r","completedAt":at(20),"ordinal":5},
            {"id":"t3","type":"reasoning","runId":"r","completedAt":at(25),"ordinal":6},
            {"id":"a","type":"assistant_message","runId":"r","ordinal":7}]});
        let shape: Vec<String> = derive(&p, 0)
            .iter()
            .map(|r| match r {
                Row::User(v) | Row::Assistant(v) => s(v, "id").to_owned(),
                Row::Work { id, label, items } => format!("{id} {label} [{}]", items.len()),
                _ => "?".to_owned(),
            })
            .collect();
        // Back-to-back steers split once, after the last; the top fold times the whole turn.
        assert_eq!(shape, ["u", "work:r Worked for 30s [1]", "s1", "s2", "work:r:steer:s2 Continued for 13s [2]", "a"]);
    }

    #[test]
    fn an_attempt_cut_off_by_a_steer_folds_unless_its_reply_finished() {
        let p = json!({
            "runs": [{"id":"r","status":"running","startedAt":"1970-01-01T00:00:00Z"}],
            "attempts": [
                {"id":"a1","runId":"r","rootNodeId":"n1","status":"superseded","completedAt":"1970-01-01T00:00:10Z"},
                {"id":"a2","runId":"r","rootNodeId":"n2","status":"running"}],
            "nodes": [{"id":"n1-tool","rootNodeId":"n1","parentNodeId":"n1"}],
            "turnItems": [
                {"id":"u","type":"user_message","runId":"r","nodeId":"n1","ordinal":1},
                {"id":"t1","type":"command_execution","runId":"r","nodeId":"n1-tool","status":"completed","ordinal":2},
                {"id":"m1","type":"assistant_message","runId":"r","nodeId":"n1","status":"completed","ordinal":3},
                {"id":"s","type":"user_message","runId":"r","inputIntent":"steer","nodeId":"n2","ordinal":4},
                {"id":"t2","type":"command_execution","runId":"r","nodeId":"n2","status":"running","ordinal":5}]});
        let shape = |p: &Value| -> Vec<String> {
            derive(p, 0)
                .iter()
                .map(|r| match r {
                    Row::User(v) | Row::Assistant(v) | Row::Item(v) => s(v, "id").to_owned(),
                    Row::Attempt { id, items } => format!("{id}[{}]", items.len()),
                    Row::Working { .. } => "working".to_owned(),
                    _ => "?".to_owned(),
                })
                .collect()
        };
        assert_eq!(shape(&p), ["u", "working", "attempt-fold:a1[2]", "s", "t2"]);
        let mut finished = p.clone();
        finished["turnItems"][2]["completedAt"] = json!("1970-01-01T00:00:05Z");
        assert_eq!(shape(&finished), ["u", "working", "t1", "m1", "s", "t2"]);
        assert!(fold_attempts(&finished).is_empty());
        // Once the turn ends, its fold nests the attempt's work.
        let mut done = p.clone();
        done["runs"][0]["status"] = json!("completed");
        let nested = fold_attempts(&done);
        assert_eq!(nested.get("t1").map(String::as_str), Some("a1"));
        assert_eq!(nested.get("m1").map(String::as_str), Some("a1"));
        assert!(!nested.contains_key("u") && !nested.contains_key("t2"));
    }

    #[test]
    fn a_stopped_turns_reply_moves_its_meta_below_its_trailing_work() {
        let items = json!([
            {"id":"u","type":"user_message","runId":"r","ordinal":1},
            {"id":"a","type":"assistant_message","runId":"r","status":"completed","ordinal":2},
            {"id":"t1","type":"command_execution","runId":"r","ordinal":3},
            {"id":"t2","type":"dynamic_tool","runId":"r","ordinal":4},
            {"id":"n","type":"notification","runId":"r","ordinal":5},
            {"id":"x","type":"run_interrupt_result","runId":"r","ordinal":6}]);
        let shape = |p: &Value| -> Vec<String> {
            derive(p, 0)
                .iter()
                .map(|r| match r {
                    Row::User(v) | Row::Assistant(v) | Row::Item(v) => s(v, "id").to_owned(),
                    Row::Tools { id, .. } | Row::Meta { id, .. } => id.clone(),
                    _ => "?".to_owned(),
                })
                .collect()
        };
        let stopped = json!({"runs":[{"id":"r","status":"interrupted"}],"turnItems":items});
        assert_eq!(shape(&stopped), ["u", "a", "tools:t1", "n", "assistant-meta:a", "x"]);
        // A notice alone is no tool work; a running turn's reply has no meta yet.
        let mut quiet = stopped.clone();
        quiet["turnItems"] = json!([items[0], items[1], items[4], items[5]]);
        assert_eq!(shape(&quiet), ["u", "a", "n", "x"]);
        let mut live = stopped.clone();
        live["runs"][0]["status"] = json!("running");
        assert!(!derive(&live, 0).iter().any(|r| matches!(r, Row::Meta { .. })));
        assert!(derive_with_meta(&live, 0).1.is_empty());
    }

    #[test]
    fn a_rendered_page_stays_out_of_tool_groups() {
        let tool = |id: &str, at: i64| json!({"id":id,"type":"command_execution","runId":"r","status":"completed","ordinal":at});
        let page = json!({"id":"page","type":"dynamic_tool","runId":"r","status":"completed","toolName":"mcp__t3-code__html_render","ordinal":2,
            "output":{"structuredContent":{"htmlRender":{"attachmentId":"a","title":"Page","height":200}}}});
        let p = json!({"runs":[{"id":"r","status":"running"}],"turnItems":[tool("t1", 1), page, tool("t2", 3)]});
        assert!(matches!(derive(&p, 0).as_slice(),
            [Row::Working { .. }, Row::Item(_), Row::Item(page), Row::Item(_), Row::Thinking { .. }] if s(page, "id") == "page"));
    }

    #[test]
    fn only_a_settled_turns_last_reply_shows_its_meta() {
        let reply = |id: &str, at: i64| json!({"id":id,"type":"assistant_message","runId":"r","status":"completed","ordinal":at});
        let stopped = json!({"runs":[{"id":"r","status":"interrupted"}],"turnItems":[
            {"id":"u","type":"user_message","runId":"r","ordinal":1}, reply("a", 2),
            {"id":"t","type":"command_execution","runId":"r","ordinal":3}, reply("b", 4),
            {"id":"x","type":"run_interrupt_result","runId":"r","ordinal":5}]});
        let (rows, meta) = derive_with_meta(&stopped, 0);
        assert_eq!(rows.iter().filter(|r| matches!(r, Row::Assistant(_))).count(), 2);
        assert_eq!(meta, HashSet::from(["b"]));
        let legacy = json!({"messages":[{"id":"q","role":"user"},{"id":"m1","role":"assistant"},{"id":"m2","role":"assistant"},{"id":"q2","role":"user"}]});
        assert_eq!(derive_with_meta(&legacy, 0).1, HashSet::from(["m2"]));
    }

    #[test]
    fn adjacent_subagents_of_one_provider_turn_group() {
        let agent = |id: &str, turn: &str| json!({"id":id,"type":"subagent","runId":"r","providerTurnId":turn,"status":"completed"});
        let p = json!({"turnItems":[agent("x","p1"), agent("y","p1"), agent("z","p1"), agent("w","p2")]});
        assert!(matches!(derive(&p, 0).as_slice(), [Row::Subagents { id, items }, Row::Item(w)]
            if id == "x" && items.len() == 3 && s(w, "id") == "w"));
    }

    #[test]
    fn dynamic_tools_read_like_the_web() {
        let tool = |name: &str, input: Value| json!({"type":"dynamic_tool","toolName":name,"title":"Read rows.rs","input":input});
        assert_eq!(entry_label(&tool("Read", json!({"file_path":"/w/demo/src/a.rs"})), Some("/w/demo")), "Read demo/src/a.rs");
        assert_eq!(label(&tool("read_file", json!({"locations":[{"path":"a.rs"},{"path":"b.rs"},{"path":"c.rs"}]}))), "Read a.rs +2 more");
        assert_eq!(label(&tool("Read", json!({}))), "Read file");
        assert_eq!(label(&json!({"type":"dynamic_tool","toolName":"Grep","input":{"pattern":"fn derive","path":"src/chat/"}})), "Searched fn derive in chat");
        assert_eq!(label(&json!({"type":"dynamic_tool","toolName":"Glob","input":{"glob":"*.rs"}})), "Searched files *.rs");
        assert_eq!(label(&json!({"type":"dynamic_tool","toolName":"Skill","input":{"skill":"review"}})), "Skill: review");
        assert_eq!(label(&json!({"type":"dynamic_tool","toolName":"js","title":"js","input":{}})), "Run JavaScript");
        assert_eq!(label(&json!({"type":"dynamic_tool","toolName":"web_fetch","title":"fetch page completed"})), "Fetch page");
    }
}
