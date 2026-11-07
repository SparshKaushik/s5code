use crate::data::{s, ts};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, PartialEq)]
pub enum Row<'a> {
    User(&'a Value),
    Assistant(&'a Value),
    Work {
        id: String,
        label: String,
        items: Vec<&'a Value>,
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
            .map(|m| {
                if s(m, "role") == "user" {
                    Row::User(m)
                } else {
                    Row::Assistant(m)
                }
            })
            .collect();
    }
    let pending: HashMap<_, _> = array(p, "runtimeRequests")
        .iter()
        .filter(|r| s(r, "status") == "pending")
        .map(|r| (s(r, "id"), r))
        .collect();
    let mut final_by_run = HashMap::new();
    for item in &items {
        if s(item, "type") == "assistant_message" {
            final_by_run.insert(s(item, "runId"), s(item, "id"));
        }
    }
    let runs: HashMap<_, _> = array(p, "runs").iter().map(|r| (s(r, "id"), r)).collect();
    let mut rows = Vec::new();
    let mut seen_requests = HashSet::new();
    let mut work = Vec::new();
    let mut work_run = "";
    for item in items.drain(..) {
        let run_id = s(item, "runId");
        if run_id != work_run {
            flush(&mut rows, &mut work, work_run, &runs, now);
            work_run = run_id;
        }
        let kind = s(item, "type");
        if let Some(request) = pending.get(s(item, "requestId")) {
            flush(&mut rows, &mut work, work_run, &runs, now);
            rows.push(Row::Request {
                request,
                item: Some(item),
            });
            seen_requests.insert(s(*request, "id"));
        } else if kind == "user_message" {
            flush(&mut rows, &mut work, work_run, &runs, now);
            rows.push(Row::User(item));
        } else if kind == "assistant_message"
            && final_by_run.get(run_id).copied() == Some(s(item, "id"))
        {
            flush(&mut rows, &mut work, work_run, &runs, now);
            rows.push(Row::Assistant(item));
        } else if kind == "checkpoint" {
            flush(&mut rows, &mut work, work_run, &runs, now);
            if !array(item, "files").is_empty() {
                rows.push(Row::Item(item));
            }
        } else if matches!(
            kind,
            "error" | "notification" | "system_notice" | "proposed_plan" | "compaction"
        ) {
            flush(&mut rows, &mut work, work_run, &runs, now);
            rows.push(Row::Item(item));
        } else if !matches!(
            kind,
            "turn_start"
                | "turn_complete"
                | "run_start"
                | "run_complete"
                | "run_interrupt_request"
                | "run_interrupt_result"
        ) {
            work.push(item);
        }
    }
    flush(&mut rows, &mut work, work_run, &runs, now);
    for (id, request) in &pending {
        if !seen_requests.contains(*id) {
            rows.push(Row::Request {
                request,
                item: None,
            });
        }
    }
    for run in array(p, "runs") {
        if pending.is_empty() && matches!(s(run,"status"),"starting"|"running"|"waiting") && !rows.iter().any(|row| matches!(row,Row::Work{id,..} if id.starts_with(&format!("work:{}:",s(run,"id"))))) {
            let start=ts(run,"startedAt").or_else(||ts(run,"requestedAt")).unwrap_or(now);
            rows.push(Row::Work{id:format!("work:{}:active",s(run,"id")),label:format!("Working for {}",duration(start,now)),items:Vec::new()});
        }
    }
    rows
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

fn flush<'a>(
    rows: &mut Vec<Row<'a>>,
    work: &mut Vec<&'a Value>,
    run_id: &str,
    runs: &HashMap<&str, &Value>,
    now: i64,
) {
    if work.is_empty() {
        return;
    }
    let run = runs.get(run_id).copied();
    let start = run
        .and_then(|r| ts(r, "startedAt"))
        .or_else(|| ts(work[0], "startedAt"))
        .unwrap_or(now);
    let end = run.and_then(|r| ts(r, "completedAt")).unwrap_or(now);
    let active = run.is_some_and(|r| matches!(s(r, "status"), "running" | "starting" | "waiting"));
    let label = format!(
        "{} {}",
        if active { "Working for" } else { "Worked for" },
        duration(start, end)
    );
    rows.push(Row::Work {
        id: format!("work:{}:{}", run_id, s(work[0], "id")),
        label,
        items: std::mem::take(work),
    });
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
            {"id":"e","type":"error","ordinal":3},{"id":"compact","type":"compaction","ordinal":4}]});
        let rows = derive(&p, 0);
        assert!(matches!(
            rows.as_slice(),
            [
                Row::Work { .. },
                Row::Request { .. },
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
            matches!(derive(&p,2000).as_slice(),[Row::Work{label,..}] if label=="Working for 2s")
        );
    }
    #[test]
    fn timer_clamps_and_formats_minutes() {
        assert_eq!(duration(2000, 0), "0s");
        assert_eq!(duration(0, 122000), "2m 2s");
    }
}
