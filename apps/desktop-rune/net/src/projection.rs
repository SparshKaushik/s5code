//! Full (non-compact, non-windowed) thread projection event reducer.
use serde_json::{Value, json};

pub(crate) fn rows<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key].as_array().map(Vec::as_slice).unwrap_or(&[])
}
pub(crate) fn upsert(rows: &mut Vec<Value>, item: Value) {
    if let Some(i) = rows.iter().rposition(|v| v["id"] == item["id"]) {
        rows[i] = item;
    } else {
        rows.push(item);
    }
}
fn list<'a>(p: &'a mut Value, key: &str) -> &'a mut Vec<Value> {
    if !p[key].is_array() {
        p[key] = json!([]);
    }
    p[key].as_array_mut().unwrap()
}
fn visible(p: &Value, item: &Value) -> bool {
    let run = rows(p, "runs")
        .iter()
        .find(|r| r["id"] == item["runId"] && !item["runId"].is_null());
    if run.is_some_and(|r| {
        r["status"] == "rolled_back"
            || (r["status"] == "cancelled"
                && item["type"] == "user_message"
                && item["inputIntent"] == "queued_turn")
    }) {
        return false;
    }
    !(item["type"] == "run_interrupt_result"
        && !item["runId"].is_null()
        && !item["nodeId"].is_null()
        && rows(p, "attempts").iter().any(|a| {
            a["runId"] == item["runId"]
                && a["rootNodeId"] == item["nodeId"]
                && a["status"] == "superseded"
        })
        && !rows(p, "turnItems")
            .iter()
            .any(|i| i["type"] == "run_interrupt_request" && i["runId"] == item["runId"]))
}
fn renumber(rows: &mut [Value]) {
    for (i, row) in rows.iter_mut().enumerate() {
        row["position"] = json!(i);
    }
}
fn recheck(p: &mut Value) {
    let statuses: std::collections::HashMap<_, _> = rows(p, "runs")
        .iter()
        .filter_map(|r| Some((r["id"].as_str()?, r["status"].as_str()?)))
        .collect();
    let roots: std::collections::HashSet<_> = rows(p, "attempts")
        .iter()
        .filter(|a| a["status"] == "superseded")
        .filter_map(|a| Some((a["runId"].as_str()?, a["rootNodeId"].as_str()?)))
        .collect();
    let requests: std::collections::HashSet<_> = rows(p, "turnItems")
        .iter()
        .filter(|i| i["type"] == "run_interrupt_request")
        .filter_map(|i| i["runId"].as_str())
        .collect();
    let mut kept: Vec<_> = rows(p, "visibleTurnItems")
        .iter()
        .filter(|r| {
            if r["visibility"] != "local" {
                return true;
            }
            let i = &r["item"];
            let run = i["runId"].as_str().unwrap_or("");
            let status = statuses.get(run).copied();
            status != Some("rolled_back")
                && !(status == Some("cancelled")
                    && i["type"] == "user_message"
                    && i["inputIntent"] == "queued_turn")
                && !(i["type"] == "run_interrupt_result"
                    && roots.contains(&(run, i["nodeId"].as_str().unwrap_or("")))
                    && !requests.contains(run))
        })
        .cloned()
        .collect();
    renumber(&mut kept);
    p["visibleTurnItems"] = json!(kept);
}
fn restore_fields(item: &mut Value, previous: &Value, fields: &[&str]) {
    for field in fields {
        let marker = format!("{field}Omitted");
        if item[&marker] == true && previous.get(*field).is_some() && previous[&marker] != true {
            item[*field] = previous[*field].clone();
            item.as_object_mut().unwrap().remove(&marker);
        }
    }
}
fn restore_item(p: &Value, item: &mut Value) {
    let (collection, id, fields): (_, _, &[&str]) = match item["type"].as_str().unwrap_or("") {
        "user_message" | "assistant_message" => ("messages", "messageId", &["text"]),
        "checkpoint" => ("checkpoints", "checkpointId", &["files"]),
        "subagent" => ("subagents", "subagentId", &["prompt", "result"]),
        _ => return,
    };
    if let Some(previous) = rows(p, collection).iter().rfind(|r| r["id"] == item[id]) {
        restore_fields(item, previous, fields);
    }
}

pub(crate) fn apply(p: &mut Value, e: &Value) {
    if p.is_null() || e["threadId"] != p["thread"]["id"] {
        return;
    }
    let t = e["type"].as_str().unwrap_or("");
    let mut item = e["payload"].clone();
    if !matches!(
        t,
        "thread.visited" | "thread.tags-updated" | "thread.marked-unread"
    ) {
        p["updatedAt"] = e["occurredAt"].clone();
    }
    match t {
        "thread.pull-request-link-updated" => {
            if !p["thread"]["pullRequests"].is_array() {
                let legacy = &p["thread"]["linkedPullRequest"];
                p["thread"]["pullRequests"] = if legacy.is_object() {
                    json!([{ "host": url::Url::parse(legacy["url"].as_str().unwrap_or("")).ok().and_then(|u|u.host_str().map(str::to_owned)).unwrap_or("unknown".into()), "repository": legacy["repository"], "number": legacy["number"], "url":legacy["url"], "source":"manual", "linkedAt":"1970-01-01T00:00:00.000Z", "snapshot":null,"stack":null }])
                } else {
                    json!([])
                };
            }
            for link in list(&mut p["thread"], "pullRequests") {
                if pull_request_key(link) == pull_request_key(&item) {
                    *link = item.clone();
                }
            }
        }
        "thread.created"
        | "thread.archived"
        | "thread.unarchived"
        | "thread.deleted"
        | "thread.settled"
        | "thread.unsettled"
        | "thread.snoozed"
        | "thread.unsnoozed"
        | "thread.auto-settle-set"
        | "thread.pinned"
        | "thread.unpinned"
        | "thread.pin-reordered"
        | "thread.active-reordered"
        | "thread.metadata-updated"
        | "thread.pull-request-synced"
        | "thread.runtime-mode-updated"
        | "thread.interaction-mode-updated"
        | "thread.model-selection-updated"
        | "thread.provider-switched"
        | "thread.guest-share-updated"
        | "thread.visited"
        | "thread.tags-updated"
        | "thread.marked-unread" => p["thread"] = item,
        "run.background-work-cancelled" => {
            for r in list(p, "runs") {
                if r["id"] == item["runId"] {
                    r["restartCancelledBackgroundWork"] =
                        item["restartCancelledBackgroundWork"].clone();
                }
            }
        }
        "provider-session.detached" => {
            list(p, "providerSessions").retain(|v| v["id"] != item["providerSessionId"])
        }
        "turn-item.updated" => {
            restore_item(p, &mut item);
            let previous = rows(p, "turnItems").iter().rfind(|v| v["id"] == item["id"]);
            let recheck_all = item["type"] == "run_interrupt_request"
                || previous.is_some_and(|v| v["type"] == "run_interrupt_request");
            if let Some(work) = previous.and_then(|v| v.get("deferredWork")) {
                item["deferredWork"] = work.clone();
            }
            upsert(list(p, "turnItems"), item.clone());
            if recheck_all {
                recheck(p);
            }
            let show = visible(p, &item);
            let rows = list(p, "visibleTurnItems");
            let old = rows.iter().rposition(|r| r["sourceItemId"] == item["id"]);
            if !show {
                if let Some(i) = old {
                    rows.remove(i);
                    renumber(rows);
                }
                return;
            }
            let mut row = json!({"position":0,"visibility":"local","sourceThreadId":item["threadId"],"sourceItemId":item["id"],"item":item});
            if let Some(i) = old {
                if rows[i]["visibility"] == "local"
                    && rows[i]["sourceThreadId"] == row["sourceThreadId"]
                    && rows[i]["item"]["ordinal"] == item["ordinal"]
                {
                    row["position"] = rows[i]["position"].clone();
                    rows[i] = row;
                    return;
                }
                rows.remove(i);
            }
            let ordinal = item["ordinal"].as_i64().unwrap_or(0);
            let index = rows
                .iter()
                .position(|r| {
                    r["visibility"] == "local"
                        && (r["item"]["ordinal"].as_i64().unwrap_or(0) > ordinal
                            || (r["item"]["ordinal"].as_i64() == Some(ordinal)
                                && r["item"]["id"].as_str() > item["id"].as_str()))
                })
                .unwrap_or(rows.len());
            rows.insert(index, row);
            renumber(rows);
        }
        _ => {
            let key = match t {
                "run.created" | "run.updated" => "runs",
                "run-attempt.created" | "run-attempt.updated" => "attempts",
                "node.updated" => "nodes",
                "subagent.updated" => "subagents",
                "provider-session.attached" | "provider-session.updated" => "providerSessions",
                "provider-thread.updated" => "providerThreads",
                "provider-turn.updated" => "providerTurns",
                "runtime-request.updated" => "runtimeRequests",
                "message.updated" => "messages",
                "plan.updated" => "plans",
                "checkpoint-scope.created" => "checkpointScopes",
                "checkpoint.captured" => "checkpoints",
                "context-handoff.updated" => "contextHandoffs",
                "context-transfer.created" | "context-transfer.updated" => "contextTransfers",
                _ => return,
            };
            if let Some(old) = rows(p, key).iter().find(|v| v["id"] == item["id"]) {
                if key == "providerTurns" && item["tokenUsage"].is_null() {
                    if let Some(usage) = old.get("tokenUsage") {
                        item["tokenUsage"] = usage.clone();
                    }
                }
                if key == "subagents" {
                    restore_fields(&mut item, old, &["prompt", "result"]);
                }
            }
            upsert(list(p, key), item);
            if key == "runs" || key == "attempts" {
                recheck(p);
            }
        }
    }
}

fn pull_request_key(link: &Value) -> (String, String, u64) {
    let host = link["authority"]
        .as_str()
        .or_else(|| link["host"].as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let repository = link["repository"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    (host, repository, link["number"].as_u64().unwrap_or(0))
}

fn projected_key(row: &Value) -> String {
    format!("{}:{}", row["sourceThreadId"].as_str().unwrap_or(""), row["sourceItemId"].as_str().unwrap_or(""))
}

/// An older page's rows to show: rows already held win over the page's copy, and a local item
/// the projection holds but hides stays out. Also returns the page's keys and the local items
/// `turnItems` gains so later events can update them.
fn page_rows(p: &Value, older: &[Value], keep: impl Fn(&Value) -> bool) -> (Vec<Value>, std::collections::HashSet<String>, Vec<Value>) {
    let thread = &p["thread"]["id"];
    let current: std::collections::HashMap<String, &Value> =
        rows(p, "visibleTurnItems").iter().map(|r| (projected_key(r), r)).collect();
    let mut page_keys = std::collections::HashSet::new();
    let mut page = Vec::new();
    let mut added: Vec<Value> = Vec::new();
    for page_row in older.iter().filter(|r| keep(*r)) {
        let key = projected_key(page_row);
        if page_keys.contains(&key) {
            continue;
        }
        let local = page_row["visibility"] == "local" || page_row["sourceThreadId"] == *thread;
        let mut row = current.get(&key).map_or_else(|| page_row.clone(), |r| (*r).clone());
        if !current.contains_key(&key) && local {
            if let Some(held) = rows(p, "turnItems").iter().find(|i| i["id"] == row["sourceItemId"]) {
                // A held item that is not in the window was hidden on purpose.
                if held["type"] != "run_interrupt_request" || !visible(p, held) {
                    continue;
                }
                row["item"] = held.clone();
            } else if !added.iter().any(|i| i["id"] == row["sourceItemId"]) {
                added.push(row["item"].clone());
            }
        }
        page_keys.insert(key);
        page.push(row);
    }
    (page, page_keys, added)
}

/// `mergeOlderHistoryIntoProjection` for a history page: its rows lead the window.
pub(crate) fn merge_older(p: &mut Value, older: &[Value]) {
    if p.is_null() || older.is_empty() {
        return;
    }
    let (page, page_keys, added) = page_rows(p, older, |_| true);
    if page.is_empty() {
        return;
    }
    let rest = rows(p, "visibleTurnItems").iter().filter(|r| !page_keys.contains(&projected_key(r))).cloned();
    let mut visible_rows: Vec<Value> = page.into_iter().chain(rest).collect();
    renumber(&mut visible_rows);
    p["visibleTurnItems"] = json!(visible_rows);
    list(p, "turnItems").extend(added);
}

/// `mergeOlderHistoryIntoProjection`'s `work` branch: a page of the work deferred behind
/// `anchor_id` joins the anchor's run of rows in ordinal order, and the anchor keeps
/// `next_cursor` or, with none, drops its `deferredWork` placeholder.
pub(crate) fn merge_work(p: &mut Value, anchor_id: &str, older: &[Value], next_cursor: Option<&str>) {
    let shown = rows(p, "visibleTurnItems");
    let Some(start) = shown.iter().position(|r| r["sourceItemId"] == anchor_id) else { return };
    let anchor = shown[start].clone();
    let deferred = &anchor["item"]["deferredWork"];
    if !deferred.is_object() {
        return;
    }
    let first = anchor["item"]["ordinal"].as_i64().unwrap_or(0);
    let last = deferred["lastOrdinal"].as_i64().unwrap_or(i64::MAX);
    let source = &anchor["sourceThreadId"];
    let ordinal = |r: &Value| r["item"]["ordinal"].as_i64().unwrap_or(0);
    let in_range = |r: &Value| r["sourceThreadId"] == *source && (first..=last).contains(&ordinal(r));
    let (page, page_keys, added) = page_rows(p, older, in_range);
    let mut end = start + 1;
    while let Some(r) = shown.get(end) {
        if r["sourceThreadId"] != *source || r["visibility"] == "synthetic" || ordinal(r) > last {
            break;
        }
        end += 1;
    }
    let mut updated = anchor.clone();
    match next_cursor {
        Some(cursor) => updated["item"]["deferredWork"]["cursor"] = json!(cursor),
        None => {
            if let Some(item) = updated["item"].as_object_mut() {
                item.remove("deferredWork");
            }
        }
    }
    let held = shown[start..end].iter().filter(|r| !page_keys.contains(&projected_key(r))).cloned();
    let mut section: Vec<Value> = page
        .into_iter()
        .chain(held)
        .map(|r| if r["sourceItemId"] == anchor["sourceItemId"] { updated.clone() } else { r })
        .collect();
    section.sort_by_key(|r| ordinal(r));
    let mut next: Vec<Value> = shown[..start].to_vec();
    next.extend(section);
    next.extend_from_slice(&shown[end..]);
    renumber(&mut next);
    p["visibleTurnItems"] = json!(next);
    let items = list(p, "turnItems");
    if anchor["visibility"] == "local" {
        upsert(items, updated["item"].clone());
    }
    items.extend(added);
}

/// The rows `old` shows ahead of `new`'s window, for a fresh snapshot to keep: those before the
/// first row both hold, still visible under `new`'s runs. No shared row means a gap the client
/// cannot bridge, so nothing is kept.
pub(crate) fn kept_before(old: &Value, new: &Value) -> Vec<Value> {
    let window: std::collections::HashSet<String> = rows(new, "visibleTurnItems").iter().map(projected_key).collect();
    let old_rows = rows(old, "visibleTurnItems");
    let Some(overlap) = old_rows.iter().position(|r| window.contains(&projected_key(r))) else {
        return Vec::new();
    };
    let thread = &new["thread"]["id"];
    old_rows[..overlap]
        .iter()
        .filter(|r| !(r["visibility"] == "local" || r["sourceThreadId"] == *thread) || visible(new, &r["item"]))
        .cloned()
        .collect()
}
