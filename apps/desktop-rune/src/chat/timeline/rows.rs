use crate::data::{s, ts};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

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
    },
    Item(&'a Value),
    Request {
        request: &'a Value,
        item: Option<&'a Value>,
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

const LIFECYCLE: [&str; 6] = ["turn_start", "turn_complete", "run_start", "run_complete", "run_interrupt_request", "run_interrupt_result"];

fn run_active(run: &Value) -> bool {
    matches!(s(run, "status"), "starting" | "running" | "waiting")
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
            | "handoff"
            | "run_interrupt_request"
            | "run_interrupt_result"
            | "secret_request"
            | "link_request"
    )
}

/// Where "Edit from here" rewinds each prompt to, by user message item id: the ready checkpoint
/// after the run before it, or the thread's start checkpoint for the first run
/// (deriveRevertTurnCountByUserMessageId + findRevertCheckpoint). Inherited history, steers and
/// the turn still running have none.
pub fn revert_targets(p: &Value) -> HashMap<String, (String, String)> {
    let ready: Vec<&Value> = array(p, "checkpoints").iter().filter(|c| s(c, "status") == "ready").collect();
    let start = ready.iter().rfind(|c| c["ordinalWithinScope"] == 0 && c["appRunOrdinal"].is_null());
    let after = |run: &str| ready.iter().rfind(|c| s(c, "runId") == run);
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

pub fn derive(p: &Value, now: i64) -> Vec<Row<'_>> {
    let mut items: Vec<&Value> = if p["visibleTurnItems"].is_array() {
        let mut visible: Vec<_> = array(p, "visibleTurnItems").iter().collect();
        visible.sort_by_key(|v| v["position"].as_i64().unwrap_or(0));
        visible.into_iter().map(|v| &v["item"]).collect()
    } else {
        let mut items: Vec<_> = array(p, "turnItems").iter().collect();
        items.sort_by_key(|v| v["ordinal"].as_i64().unwrap_or(0));
        items
    };
    if items.is_empty() && !p["visibleTurnItems"].is_array() && !array(p, "messages").is_empty() {
        return array(p, "messages")
            .iter()
            .map(|m| if s(m, "role") == "user" { Row::User(m) } else { Row::Assistant(m) })
            .collect();
    }
    let pending: HashMap<_, _> = array(p, "runtimeRequests")
        .iter()
        .filter(|r| s(r, "status") == "pending")
        .map(|r| (s(r, "id"), r))
        .collect();
    let runs: HashMap<_, _> = array(p, "runs").iter().map(|r| (s(r, "id"), r)).collect();
    let active = |run_id: &str| runs.get(run_id).is_some_and(|r| run_active(r));
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
    let mut rows = Vec::new();
    let mut seen_requests = HashSet::new();
    let mut working = HashSet::new();
    let mut folds: HashMap<&str, usize> = HashMap::new();
    let mut spans: HashMap<&str, (Option<i64>, Option<i64>)> = HashMap::new();
    for (item, key) in items.drain(..).zip(&keys) {
        let kind = s(item, "type");
        let run_id = s(item, "runId");
        let live = !run_id.is_empty() && active(run_id);
        if live && kind != "user_message" && working.insert(run_id) {
            rows.push(working_row(runs[run_id], now));
        }
        if let Some(request) = pending.get(s(item, "requestId")) {
            rows.push(Row::Request { request, item: Some(item) });
            seen_requests.insert(s(*request, "id"));
        } else if kind == "user_message" {
            rows.push(Row::User(item));
            if live && working.insert(run_id) {
                rows.push(working_row(runs[run_id], now));
            }
        } else if LIFECYCLE.contains(&kind) {
        } else if kind == "checkpoint" {
            if !array(item, "files").is_empty() {
                rows.push(Row::Item(item));
            }
        } else if kind == "assistant_message" && (live || terminal.get(key.as_str()) == Some(&s(item, "id"))) {
            rows.push(Row::Assistant(item));
        } else if live || escapes_fold(kind) || interrupted.contains(run_id) {
            rows.push(if kind == "assistant_message" { Row::Assistant(item) } else { Row::Item(item) });
        } else {
            let span = spans.entry(key.as_str()).or_default();
            let start = ts(item, "startedAt").or_else(|| ts(item, "createdAt"));
            let end = ts(item, "completedAt").or_else(|| ts(item, "updatedAt")).or(start);
            span.0 = span.0.or(start);
            span.1 = span.1.max(end);
            match folds.get(key.as_str()) {
                Some(&index) => {
                    if let Row::Work { items, .. } = &mut rows[index] {
                        items.push(item);
                    }
                }
                None => {
                    folds.insert(key.as_str(), rows.len());
                    rows.push(Row::Work { id: format!("work:{key}"), label: String::new(), items: vec![item] });
                }
            }
        }
    }
    for (key, index) in folds {
        let run = runs.get(key).copied();
        let (first, last) = spans.get(key).copied().unwrap_or_default();
        let start = run.and_then(|r| ts(r, "startedAt")).or(first);
        let end = run.and_then(|r| ts(r, "completedAt")).or(last);
        if let Row::Work { label, .. } = &mut rows[index] {
            *label = match (start, end) {
                (Some(start), Some(end)) => format!("Worked for {}", duration(start, end)),
                _ => "Worked".to_owned(),
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
            rows.push(working_row(run, now));
        }
    }
    rows
}

fn working_row(run: &Value, now: i64) -> Row<'static> {
    let start = ts(run, "startedAt").or_else(|| ts(run, "requestedAt")).unwrap_or(now);
    Row::Working { id: format!("working:{}", s(run, "id")), label: format!("Working for {}", duration(start, now)) }
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
        assert!(
            matches!(derive(&p,2000).as_slice(),[Row::Working{label,..}] if label=="Working for 2s")
        );
    }
    #[test]
    fn a_live_turn_has_one_working_row_after_its_prompt_and_folds_once_it_ends() {
        let items = json!([
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
                _ => "?".to_owned(),
            })
            .collect();
        assert_eq!(ids, ["u", "Working for 5s", "t1", "a1", "t2", "a2"]);
        let done = json!({"runs":[{"id":"r","status":"completed","startedAt":"1970-01-01T00:00:00Z","completedAt":"1970-01-01T00:00:09Z"}],"turnItems":items});
        let rows = derive(&done, 60000);
        assert!(matches!(rows.as_slice(), [Row::User(_), Row::Work { id, label, items }, Row::Assistant(a)]
            if id == "work:r" && label == "Worked for 9s" && items.len() == 3 && s(a, "id") == "a2"));
        let stopped = json!({"runs":[{"id":"r","status":"interrupted"}],"turnItems":[
            {"id":"u","type":"user_message","runId":"r","ordinal":1},
            {"id":"t1","type":"command_execution","runId":"r","ordinal":2},
            {"id":"x","type":"run_interrupt_request","runId":"r","ordinal":3}]});
        assert!(matches!(derive(&stopped, 0).as_slice(), [Row::User(_), Row::Item(_)]));
    }
    #[test]
    fn prompts_rewind_to_the_checkpoint_before_their_run() {
        let p = json!({
            "runs": [{"id":"r1","status":"completed"},{"id":"r2","status":"completed"},{"id":"r3","status":"running"}],
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
        let t = revert_targets(&p);
        assert_eq!(t.get("u1"), Some(&("s".to_owned(), "c0".to_owned())));
        assert_eq!(t.get("u2"), Some(&("s".to_owned(), "c1".to_owned())));
        // A steer is not a turn start; the running turn can't be rewound; r2's checkpoint failed.
        assert_eq!(t.len(), 2);
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
}
