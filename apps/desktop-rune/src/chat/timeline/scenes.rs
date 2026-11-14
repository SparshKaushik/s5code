use super::rows::{self, Row};
use crate::{App, Appearance, Route};

fn thread(app: &mut App, title: &str) {
    let id = app
        .state()
        .thread_views
        .iter()
        .find(|(_, v)| crate::data::s(&v.projection["thread"], "title") == title)
        .map(|(id, _)| id.clone());
    if let Some(id) = id {
        app.navigate(Route::Thread(id));
    }
    crate::scene_clock(app);
}
fn showcase(app: &mut App) {
    thread(app, "Replay showcase");
}
fn todo(app: &mut App) {
    thread(app, "Todo list replay");
}
fn approval(app: &mut App) {
    thread(app, "Approval flow replay");
}
fn idempotency(app: &mut App) {
    thread(app, "Idempotency key definition");
}
fn sidebar(app: &mut App) {
    thread(app, "Sidebar v2 pinned and active sections");
}
fn docs(app: &mut App) {
    thread(app, "Docs site navigation ideas");
}
fn expanded(app: &mut App) {
    showcase(app);
    if let Route::Thread(id) = &app.route {
        let ids = app
            .state()
            .thread_views
            .get(id)
            .map(|v| {
                rows::derive(&v.projection, app.now_ms)
                    .iter()
                    .filter_map(|r| {
                        if let Row::Work { id, .. } = r {
                            Some(id.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        app.ui.chat.timeline.expanded.extend(ids);
    }
}
fn pending(app: &mut App) {
    thread(app, "Timeline approval fixture");
}
fn pending_warning(app: &mut App) {
    pending(app);
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(view) = state.thread_views.get_mut(&id)
    {
        let options = serde_json::json!([
            {"decision": "decline", "label": "Decline"},
            {"decision": "accept", "label": "Approve", "warning": "This runs outside the sandbox with full disk access."},
            {"decision": "acceptForSession", "label": "Always allow this session"}]);
        for list in ["visibleTurnItems", "turnItems"] {
            for entry in view.projection[list].as_array_mut().into_iter().flatten() {
                let item = if entry["item"].is_object() { &mut entry["item"] } else { entry };
                if item["type"] == "approval_request" {
                    item["options"] = options.clone();
                }
            }
        }
    }
}
fn question(app: &mut App) {
    thread(app, "Timeline question fixture");
}
fn working(app: &mut App) {
    thread(app, "Timeline showcase fixture");
}
fn error(app: &mut App) {
    showcase(app);
    if let Route::Thread(id) = &app.route {
        if let crate::data::Source::Fixture(state) = &mut app.src {
            if let Some(v) = state.thread_views.get_mut(id) {
                if let Some(items) = v.projection["visibleTurnItems"].as_array_mut() {
                    items.push(serde_json::json!({"position":100,"item":{"id":"scene-error","type":"error","status":"failed","failure":{"class":"transport_error","message":"The provider process exited unexpectedly. Please try again.","code":null,"retryable":true},"retry":{"attempt":1,"maxAttempts":3,"retryDelayMs":1000}}}));
                }
            }
        }
    }
}
/// The showcase thread with a failed latest run whose root error is `failure`.
fn failed_run(app: &mut App, failure: serde_json::Value) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(v) = state.thread_views.get_mut(&id)
    {
        let p = &mut v.projection;
        let run = serde_json::json!({"id": "scene-run", "ordinal": 999, "status": "failed", "rootNodeId": "scene-root",
            "startedAt": "2026-12-01T00:00:00.000Z", "completedAt": "2026-12-01T00:00:01.000Z"});
        let item = serde_json::json!({"id": "scene-failure", "type": "error", "status": "failed", "runId": "scene-run", "nodeId": "scene-root",
            "ordinal": 1, "updatedAt": "2026-12-01T00:00:01.000Z", "failure": failure});
        for (key, value) in [("runs", run), ("turnItems", item)] {
            match p[key].as_array_mut() {
                Some(list) => list.push(value),
                None => p[key] = serde_json::json!([value]),
            }
        }
    }
}
fn error_banner(app: &mut App) {
    failed_run(app, serde_json::json!({"class": "provider_error", "code": null, "retryable": false,
        "message": "{\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded: the provider is temporarily unable to take requests.\"},\"request_id\":\"req_011CScene\"}"}));
}
fn error_banner_chatgpt(app: &mut App) {
    failed_run(app, serde_json::json!({"class": "provider_error", "retryable": false,
        "code": "subscription_sharing_workplace_individual_usage_limit_exceeded", "message": "You've hit your usage limit."}));
}
fn system_notification(app: &mut App) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(items) = state.thread_views.get_mut(&id).and_then(|v| v.projection["visibleTurnItems"].as_array_mut())
    {
        for (n, (kind, summary)) in [("system", "T3 Code restarted and resumed this turn"), ("command", "Background command finished")].into_iter().enumerate() {
            items.push(serde_json::json!({"position": 100 + n, "item": {"id": format!("scene-notification-{kind}"), "type": "notification",
                "status": "completed", "outcome": "updated", "source": {"kind": kind}, "summary": summary, "detail": "Continue",
                "createdAt": "2026-01-01T00:00:00Z"}}));
        }
    }
}
fn chip_details(app: &mut App) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    let records = crate::chat::composer::chips::samples();
    let link = |r: &serde_json::Value| format!("[{}](t3-context://v1/{}/{})", crate::data::s(r, "label"), crate::data::s(r, "kind"), crate::data::s(r, "contextId"));
    let text = format!("Why does {} fail? See {}", link(&records[0]), link(&records[2]));
    let open = crate::data::s(&records[0], "contextId").to_owned();
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(items) = state.thread_views.get_mut(&id).and_then(|v| v.projection["visibleTurnItems"].as_array_mut())
    {
        items.push(serde_json::json!({"position": 100, "item": {"id": "scene-chips", "type": "user_message", "text": text,
            "createdAt": "2026-01-01T00:00:00Z", "attachments": [], "context": {"records": records}}}));
    }
    app.ui.chat.timeline.chip = Some(("scene-chips".into(), open));
}
fn long_message(app: &mut App) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    let text = (1..=20).map(|n| format!("Step {n}: check the {n}th migration and note anything odd.")).collect::<Vec<_>>().join("\n");
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(items) = state.thread_views.get_mut(&id).and_then(|v| v.projection["visibleTurnItems"].as_array_mut())
    {
        items.push(serde_json::json!({"position": 100, "item": {"id": "scene-long", "type": "user_message", "text": text,
            "createdAt": "2026-01-01T00:00:00Z", "attachments": []}}));
        items.push(serde_json::json!({"position": 101, "item": {"id": "scene-failed-tool", "type": "command_execution",
            "status": "failed", "input": "pnpm test --filter migrations", "createdAt": "2026-01-01T00:00:01Z"}}));
    }
}
fn secret_request(app: &mut App) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(items) = state.thread_views.get_mut(&id).and_then(|v| v.projection["visibleTurnItems"].as_array_mut())
    {
        for (n, (status, label)) in [("saved", "OPENAI_API_KEY"), ("pending", "STRIPE_WEBHOOK_SECRET")].into_iter().enumerate() {
            items.push(serde_json::json!({"position": 100 + n, "visibility": "local", "item": {"id": format!("scene-secret-{n}"),
                "type": "secret_request", "threadId": id, "status": "waiting", "secretStatus": status, "label": label,
                "reason": "The webhook handler verifies signatures with it.", "createdAt": "2026-01-01T00:00:00Z"}}));
        }
    }
}
fn long_message_open(app: &mut App) {
    long_message(app);
    app.ui.chat.timeline.expanded.insert("user-full:scene-long".into());
}
fn light(app: &mut App) {
    showcase(app);
    app.appearance = Appearance::Light;
}
pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("timeline-pending-approval", pending),
    ("timeline-question", question),
    ("timeline-error", error),
    ("timeline-error-banner", error_banner),
    ("timeline-error-banner-chatgpt", error_banner_chatgpt),
    ("timeline-system-notification", system_notification),
    ("timeline-working", working),
    ("timeline-showcase", showcase),
    ("timeline-todo", todo),
    ("timeline-approval", approval),
    ("timeline-opencode-idempotency", idempotency),
    ("timeline-opencode-sidebar", sidebar),
    ("timeline-opencode-docs", docs),
    ("timeline-expanded", expanded),
    ("timeline-light", light),
    ("timeline-chip-details", chip_details),
    ("timeline-long-message", long_message),
    ("timeline-long-message-open", long_message_open),
    ("timeline-approval-warning", pending_warning),
    ("timeline-secret-request", secret_request),
    ("timeline-link-request", link_request),
    ("timeline-changed-expand-all", changed_expand_all),
    ("timeline-provider-banner", provider_banner),
    ("timeline-usage-limit-banner", usage_limit_banner),
    ("timeline-child-approval", child_approval),
    ("timeline-worktree-setup", worktree_setup),
    ("timeline-worktree-retry", worktree_retry),
    ("timeline-tool-group", tool_group),
    ("timeline-tool-group-open", tool_group_open),
    ("timeline-tool-live", tool_live),
    ("timeline-thinking", thinking),
    ("timeline-tool-inspector", tool_inspector),
    ("timeline-link-chips", link_chips),
    ("chat-server-update-running", server_update_running),
    ("chat-server-update-failed", server_update_failed),
    ("timeline-user-markdown", user_markdown),
    ("timeline-steer", steer),
    ("timeline-load-earlier", load_earlier),
    ("timeline-load-earlier-error", load_earlier_error),
    ("timeline-answered-questions", answered_questions),
    ("timeline-now", thread_now),
    ("timeline-now-stop", thread_now_stop),
    ("timeline-header-subagent", header_subagent),
    ("timeline-draft-hero", draft_hero),
    ("timeline-draft-hero-empty", draft_hero_empty),
    ("timeline-work-history-loading", work_history_loading),
    ("timeline-work-history-error", work_history_error),
    ("timeline-command-output", command_output),
    ("timeline-provider-errors", provider_errors),
    ("timeline-lifecycle", lifecycle_rows),
    ("timeline-parent-divider", parent_divider),
    ("timeline-user-attribution", user_attribution),
    ("timeline-steer-segments", steer_segments),
    ("timeline-steer-segments-open", steer_segments_open),
    ("timeline-superseded-attempt", superseded_attempt),
    ("timeline-superseded-attempt-open", superseded_attempt_open),
    ("timeline-subagent-group", subagent_group),
    ("timeline-subagent-group-open", subagent_group_open),
    ("timeline-compacting", compacting),
];

/// Appends `value` to the route thread's projection `list`, creating the list when missing.
fn push_items(app: &mut App, list: &str, values: Vec<serde_json::Value>) {
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(v) = state.thread_views.get_mut(&id)
    {
        match v.projection[list].as_array_mut() {
            Some(items) => items.extend(values),
            None => v.projection[list] = serde_json::Value::Array(values),
        }
    }
}
/// Sets fields on the route thread's sidebar row.
fn patch_shell(app: &mut App, fields: serde_json::Value) {
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(row) = state.threads.iter_mut().find(|r| crate::data::s(r, "id") == id)
    {
        for (k, v) in fields.as_object().into_iter().flatten() {
            row[k] = v.clone();
        }
    }
}
fn link_request(app: &mut App) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    let pending = serde_json::json!({"id": "scene-link", "type": "link_request", "status": "waiting", "linkStatus": "pending",
        "threadId": id, "label": "Build box", "url": "https://build.example.ts.net", "hint": "the build box",
        "requestedAccess": "read-only", "reason": "The GPU tests only run on the build box.", "createdAt": "2026-01-01T00:00:00Z"});
    let linked = serde_json::json!({"id": "scene-link-done", "type": "link_request", "status": "completed", "linkStatus": "linked",
        "threadId": id, "linkedLabel": "Studio", "linkedAccess": "full-access", "createdAt": "2026-01-01T00:00:00Z"});
    push_items(app, "visibleTurnItems", vec![
        serde_json::json!({"position": 100, "item": linked}),
        serde_json::json!({"position": 101, "item": pending.clone()}),
    ]);
    super::link::open(app, &pending);
}
fn changed_expand_all(app: &mut App) {
    showcase(app);
    let reply = serde_json::json!({"id": "scene-changed-reply", "type": "assistant_message", "runId": "scene-changed-run",
        "text": "Done. The changed-files card can now expand every folder at once.", "createdAt": "2026-01-01T00:00:00Z"});
    push_items(app, "turnItems", vec![reply.clone()]);
    push_items(app, "visibleTurnItems", vec![serde_json::json!({"position": 100, "item": reply})]);
    push_items(app, "checkpoints", vec![serde_json::json!({"runId": "scene-changed-run", "appRunOrdinal": 99, "status": "ready", "files": [
        {"path": "src/chat/timeline/changed.rs", "additions": 42, "deletions": 6},
        {"path": "src/chat/timeline/link.rs", "additions": 380, "deletions": 0},
        {"path": "src/shell/sidebar/mod.rs", "additions": 3, "deletions": 1},
        {"path": "README.md", "additions": 2, "deletions": 0}]})]);
    app.ui.chat.timeline.expanded.insert("changed-all:scene-changed-run".into());
}
fn provider_banner(app: &mut App) {
    showcase(app);
    let Some((instance, _)) = crate::chat::composer::selection(app) else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(p) = state.config.as_mut().and_then(|c| c["providers"].as_array_mut()).and_then(|ps| ps.iter_mut().find(|p| crate::data::s(p, "instanceId") == instance))
    {
        p["status"] = "error".into();
        p["installed"] = true.into();
        p["auth"] = serde_json::json!({"status": "unauthenticated"});
    }
}
fn usage_limit_banner(app: &mut App) {
    showcase(app);
    app.now_ms = crate::data::iso_ms("2026-03-10T12:00:00Z").unwrap_or(app.now_ms);
    patch_shell(app, serde_json::json!({"status": "failed", "lastErrorClass": "usage_limit", "latestRunId": "scene-run",
        "latestRunCompletedAt": "2026-03-10T11:50:00Z", "usageLimitResetAt": "2026-03-10T15:30:00Z",
        "limitRecovery": {"runId": "scene-run", "resetAt": "2026-03-10T15:30:00Z", "autoResume": true}}));
}
fn child_approval(app: &mut App) {
    let approval = app.state().thread_views.iter().find(|(_, v)| crate::data::s(&v.projection["thread"], "title") == "Timeline approval fixture").map(|(_, v)| v.projection.clone());
    showcase(app);
    let Route::Thread(parent) = app.route.clone() else { return };
    let Some(projection) = approval else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src {
        let project = state.threads.iter().find(|r| crate::data::s(r, "id") == parent).map(|r| r["projectId"].clone()).unwrap_or_default();
        state.threads.push(serde_json::json!({"id": "scene-child", "title": "Migrate the billing schema", "projectId": project,
            "status": "running", "lineage": {"parentThreadId": parent, "relationshipToParent": "subagent"},
            "pendingRuntimeRequest": {"kind": "command"}}));
        state.thread_views.insert("scene-child".into(), s5_net::ThreadView { projection, synchronized: true, sequence: 0, ..Default::default() });
    }
}
/// A setup script printing while the agent waits to start.
fn worktree_setup(app: &mut App) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    app.now_ms = crate::data::iso_ms("2026-03-10T12:00:42Z").unwrap_or(app.now_ms);
    let snapshot = serde_json::json!({"threadId": id, "sequence": 7, "phase": "running", "startedAt": "2026-03-10T12:00:00Z",
        "branch": "t3/billing-schema", "baseRef": "origin/main", "worktreePath": "/home/dev/.t3/worktrees/app/billing-schema",
        "setupScript": {"name": "Install dependencies", "command": "pnpm install --frozen-lockfile", "terminalId": "setup-1"},
        "stages": [
            {"id": "fetch", "status": "done", "startedAt": "2026-03-10T12:00:00Z", "endedAt": "2026-03-10T12:00:03Z"},
            {"id": "checkout", "status": "done", "startedAt": "2026-03-10T12:00:03Z", "endedAt": "2026-03-10T12:00:09Z"},
            {"id": "submodules", "status": "skipped", "detail": "none"},
            {"id": "setup-script", "status": "running", "startedAt": "2026-03-10T12:00:09Z",
                "tail": ["Lockfile is up to date", "Progress: resolved 1412, reused 1398, downloaded 14", "Packages: +1412", "node_modules/.pnpm/esbuild postinstall…"]},
            {"id": "agent", "status": "pending"}]});
    app.ui.chat.timeline.worktree.details.insert(id.clone());
    app.ui.chat.timeline.worktree.held.insert(id, snapshot);
}
/// A run whose workspace preparation failed, with Retry on its error row.
fn worktree_retry(app: &mut App) {
    failed_run(app, serde_json::json!({"class": "workspace_error", "code": "workspace_preparation_failed", "retryable": true,
        "message": "git worktree add failed: branch t3/billing-schema is already checked out"}));
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(v) = state.thread_views.get_mut(&id)
    {
        let p = &mut v.projection;
        for run in p["runs"].as_array_mut().into_iter().flatten() {
            if run["id"] == "scene-run" {
                run["workspacePreparation"] = serde_json::json!({"status": "failed"});
            }
        }
        let item = super::rows::array(p, "turnItems").iter().find(|i| i["id"] == "scene-failure").cloned();
        if let Some(items) = p["visibleTurnItems"].as_array_mut()
            && let Some(item) = item
        {
            items.push(serde_json::json!({"position": 100, "item": item}));
        }
    }
}

/// Appends `items` to the route thread's visible items, from position 100 on.
fn push_visible(app: &mut App, items: Vec<serde_json::Value>) {
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(list) = state.thread_views.get_mut(&id).and_then(|v| v.projection["visibleTurnItems"].as_array_mut())
    {
        for (n, item) in items.into_iter().enumerate() {
            list.push(serde_json::json!({"position": 100 + n, "item": item}));
        }
    }
}
/// The showcase thread with a turn in flight: a prompt and five tool calls, the last still
/// running when `busy`. The clock reads 47s into the turn, 7s after its last step.
fn live_turn(app: &mut App, busy: bool) {
    use serde_json::json;
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    let run = json!({"id": "scene-live", "ordinal": 998, "status": "running", "rootNodeId": "scene-live-root", "startedAt": "2026-12-01T00:00:00.000Z"});
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(v) = state.thread_views.get_mut(&id)
    {
        match v.projection["runs"].as_array_mut() {
            Some(runs) => runs.push(run),
            None => v.projection["runs"] = json!([run]),
        }
    }
    let at = |s: u32| format!("2026-12-01T00:00:{s:02}.000Z");
    let done = |mut item: serde_json::Value, from: u32, to: u32| {
        item["runId"] = json!("scene-live");
        item["status"] = json!("completed");
        item["createdAt"] = json!(at(from));
        item["completedAt"] = json!(at(to));
        item["updatedAt"] = json!(at(to));
        item
    };
    let mut test = done(
        json!({"id": "scene-tool-test", "type": "command_execution", "input": "vp test run src/chat/timeline", "exitCode": 0,
            "output": "✓ rows.rs (14 tests)\n✓ work.rs (6 tests)\n\nTest Files  2 passed (2)\n     Tests  20 passed (20)"}),
        31,
        40,
    );
    if busy {
        test["status"] = json!("running");
        test["output"] = json!("✓ rows.rs (14 tests)");
        if let Some(fields) = test.as_object_mut() {
            fields.remove("exitCode");
            fields.remove("completedAt");
        }
    }
    push_visible(app, vec![
        json!({"id": "scene-live-prompt", "type": "user_message", "runId": "scene-live", "inputIntent": "turn_start", "status": "completed",
            "text": "Run the timeline tests and fix whatever breaks.", "createdAt": at(0), "attachments": []}),
        done(json!({"id": "scene-tool-read-a", "type": "dynamic_tool", "toolName": "Read", "title": "Read rows.rs",
            "input": {"path": "src/chat/timeline/rows.rs"}}), 2, 3),
        done(json!({"id": "scene-tool-read-b", "type": "dynamic_tool", "toolName": "Read", "title": "Read work.rs",
            "input": {"path": "src/chat/timeline/work.rs", "limit": 200}}), 3, 4),
        done(json!({"id": "scene-tool-search", "type": "file_search", "title": "Searched for fn derive", "pattern": "fn derive",
            "results": [{"fileName": "src/chat/timeline/rows.rs", "line": 124, "preview": "pub fn derive(p: &Value, now: i64) -> Vec<Row<'_>> {"}]}), 5, 6),
        done(json!({"id": "scene-tool-mcp", "type": "dynamic_tool", "toolName": "mcp__t3__t3_thread_read", "title": "t3_thread_read",
            "input": {"threadId": "thread:scene", "limit": 20}, "output": [{"type": "text", "text": "{\"title\":\"Timeline polish\",\"status\":\"idle\"}"}]}), 7, 9),
        test,
    ]);
    if let Some(now) = crate::data::iso_ms("2026-12-01T00:00:47.000Z") {
        app.now_ms = now;
    }
}
fn tool_group(app: &mut App) {
    live_turn(app, false);
}
fn tool_group_open(app: &mut App) {
    live_turn(app, false);
    app.ui.chat.timeline.expanded.insert("tools:scene-tool-read-a".into());
}
fn tool_live(app: &mut App) {
    live_turn(app, true);
}
fn thinking(app: &mut App) {
    working(app);
}
fn tool_inspector(app: &mut App) {
    tool_group_open(app);
    app.ui.chat.timeline.expanded.extend(["scene-tool-test".to_owned(), "scene-tool-mcp".to_owned(), "scene-tool-search".to_owned()]);
}
fn server_update_running(app: &mut App) {
    use crate::chat::server_update::{Stage, Status, show};
    showcase(app);
    show(app, Status::Running { stage: Stage::Resuming, from: "0.0.20".into(), target: "0.0.21".into() });
}
fn server_update_failed(app: &mut App) {
    use crate::chat::server_update::{Stage, Status, show};
    showcase(app);
    let message = "Server update failed: The new server did not start.".to_owned();
    show(app, Status::Failed { stage: Stage::Downloading, from: "0.0.20".into(), target: "0.0.21".into(), message });
}
fn link_chips(app: &mut App) {
    showcase(app);
    let other = app.state().threads.iter().map(|t| crate::data::s(t, "id").to_owned()).find(|t| !t.is_empty()).unwrap_or_default();
    let text = format!(
        "I moved the grouping into [rows.rs](src/chat/timeline/rows.rs:124) and the summary into [`work.rs`](./src/chat/timeline/work.rs). \
         The approach follows [the earlier discussion](t3-thread://v1/{other}); see also the [Rust reference](https://doc.rust-lang.org/reference/).\n\n\
         - Timeline: [mod.rs](src/chat/timeline/mod.rs:40)\n\
         - Composer: [mod.rs](src/chat/composer/mod.rs)"
    );
    push_visible(app, vec![serde_json::json!({"id": "scene-link-chips", "type": "assistant_message", "status": "completed", "text": text,
        "createdAt": "2026-01-01T00:00:00Z"})]);
}
fn user_markdown(app: &mut App) {
    showcase(app);
    let short = "Can you **tighten** the fold?\n- keep `max_lines(7)`\n- add a test";
    let long = "Two things for the timeline:\n\n1. Group tool calls\n2. Show *Thinking* while idle\n\n```rust\nfn derive() {}\n```\n\nThanks!";
    push_visible(app, vec![
        serde_json::json!({"id": "scene-user-md", "type": "user_message", "text": short, "createdAt": "2026-01-01T00:00:00Z", "attachments": []}),
        serde_json::json!({"id": "scene-user-md-long", "type": "user_message", "text": long, "createdAt": "2026-01-01T00:00:01Z", "attachments": []}),
    ]);
}
fn steer(app: &mut App) {
    showcase(app);
    let user = |id: &str, intent: &str, text: &str| {
        serde_json::json!({"id": id, "type": "user_message", "inputIntent": intent, "status": "completed", "text": text, "createdAt": "2026-01-01T00:00:00Z", "attachments": []})
    };
    push_visible(app, vec![
        user("scene-steer-a", "steer", "Actually, skip the docs for now."),
        user("scene-steer-b", "steer", "And keep the old test names."),
        serde_json::json!({"id": "scene-steer-reply", "type": "assistant_message", "status": "completed", "text": "Got it: no docs, same test names.", "createdAt": "2026-01-01T00:00:02Z"}),
        user("scene-steer-c", "promoted_queued_to_steer", "Then run the suite once more."),
    ]);
}

/// A bounded window with older turns on the server.
fn load_earlier(app: &mut App) {
    pending(app);
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(v) = state.thread_views.get_mut(&id)
    {
        v.history_cursor = Some("scene-cursor".into());
        v.has_more_history = true;
    }
}
fn load_earlier_error(app: &mut App) {
    load_earlier(app);
    let Route::Thread(id) = app.route.clone() else { return };
    app.ui.chat.timeline.history.errors.insert(id, "The history cursor is no longer valid.".into());
}
fn answered_questions(app: &mut App) {
    pending(app);
    push_visible(app, vec![serde_json::json!({"id": "scene-answered", "type": "user_input_request", "status": "completed", "createdAt": "2026-01-01T00:00:00Z",
        "questionAnswer": {"requestId": "r", "questionTextById": {"q1": "Strict schemas or UI flexibility?", "q2": "Which runtime should the probe target?"},
            "answers": {"q1": ["UI flexibility"], "q2": "Node 24"}}})]);
    app.ui.chat.timeline.expanded.insert("scene-answered".into());
}

/// The showcase's first finished turn with its work deferred by the server, its fold open.
fn deferred_work(app: &mut App) -> Option<String> {
    showcase(app);
    let Route::Thread(thread) = app.route.clone() else { return None };
    let (fold, anchor) = app.state().thread_views.get(&thread).and_then(|v| {
        rows::derive(&v.projection, app.now_ms).into_iter().find_map(|r| match r {
            Row::Work { id, items, .. } => items.first().map(|i| (id, crate::data::s(i, "id").to_owned())),
            _ => None,
        })
    })?;
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(v) = state.thread_views.get_mut(&thread)
    {
        for list in ["visibleTurnItems", "turnItems"] {
            for entry in v.projection[list].as_array_mut().into_iter().flatten() {
                let item = if entry["item"].is_object() { &mut entry["item"] } else { entry };
                if item["id"] == anchor.as_str() {
                    item["deferredWork"] = serde_json::json!({"cursor": "scene-work", "itemCount": 240, "lastOrdinal": 100000});
                }
            }
        }
    }
    app.ui.chat.timeline.expanded.insert(fold);
    Some(anchor)
}
fn work_history_loading(app: &mut App) {
    deferred_work(app);
}
fn work_history_error(app: &mut App) {
    if let Some(anchor) = deferred_work(app) {
        app.ui.chat.timeline.history.failed_work.insert(anchor);
    }
}
/// A running command's open row streaming coloured output.
fn command_output(app: &mut App) {
    live_turn(app, true);
    app.ui.chat.timeline.expanded.extend(["tools:scene-tool-read-a".to_owned(), "scene-tool-test".to_owned()]);
    let raw = "\u{1b}[2m$ vp test run src/chat/timeline\u{1b}[22m\n\u{1b}[32m✓\u{1b}[39m rows.rs (14 tests)\n\u{1b}[31m✗\u{1b}[39m work.rs › groups tools\n  expected \u{1b}[33m3\u{1b}[39m, received \u{1b}[33m2\u{1b}[39m\n\u{1b}[42;30m PASS \u{1b}[0m find.rs\nRunning 3 more files… 40%\r\u{1b}[KRunning 3 more files… 80%";
    super::inspector::scene_stream(app, "scene-tool-test", raw, true);
}

/// Another thread's id, for rows that open one.
fn other_thread(app: &App) -> String {
    let Route::Thread(id) = &app.route else { return String::new() };
    app.state().threads.iter().map(|t| crate::data::s(t, "id").to_owned()).find(|t| !t.is_empty() && t != id).unwrap_or_default()
}
/// A failed provider call, a retry in flight and one that recovered, a usage limit with its
/// reset time, and a runtime warning.
fn provider_errors(app: &mut App) {
    use serde_json::json;
    showcase(app);
    app.now_ms = crate::data::iso_ms("2026-03-10T12:00:00Z").unwrap_or(app.now_ms);
    let at = "2026-03-10T11:58:00Z";
    let overloaded = json!({"class": "transport_error", "message": "Overloaded: the provider is temporarily unable to take requests."});
    push_visible(app, vec![
        json!({"id": "scene-provider-error", "type": "error", "status": "failed", "createdAt": at,
            "failure": {"class": "provider_error", "message": "The provider process exited unexpectedly. Please try again.", "code": null, "retryable": true}}),
        json!({"id": "scene-provider-retrying", "type": "error", "status": "running", "createdAt": at, "failure": overloaded.clone(),
            "retry": {"attempt": 2, "maxAttempts": 5, "retryDelayMs": 1500}}),
        json!({"id": "scene-provider-recovered", "type": "error", "status": "completed", "createdAt": at, "failure": overloaded,
            "retry": {"attempt": 3, "maxAttempts": 5, "retryDelayMs": null}}),
        json!({"id": "scene-usage-limit", "type": "error", "status": "failed", "createdAt": at,
            "failure": {"class": "usage_limit", "message": "You've hit your usage limit.", "resetAt": "2026-03-10T15:30:00Z"}}),
        json!({"id": "scene-system-notice", "type": "system_notice", "status": "completed", "createdAt": at, "message": "Sandbox is off for this turn."}),
    ]);
}
/// Lifecycle rows: a handoff, a created thread, a fork, compactions, an interrupt and a
/// subagent's notice, with every work fold open.
fn lifecycle_rows(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let other = other_thread(app);
    let at = "2026-01-01T00:00:00Z";
    push_visible(app, vec![
        json!({"id": "scene-handoff", "type": "handoff", "status": "completed", "createdAt": at, "fromProviderInstanceIds": ["claudeAgent"],
            "fromModelSelections": [{"instanceId": "claudeAgent", "model": "claude-opus-4-5"}], "toProviderInstanceId": "codex", "toModel": "gpt-5-codex"}),
        json!({"id": "scene-created", "type": "thread_created", "status": "completed", "createdAt": at, "title": "Migrate the billing schema", "targetThreadId": other}),
        json!({"id": "scene-fork", "type": "fork", "status": "completed", "createdAt": at, "source": {"type": "run", "threadId": other, "runId": "scene-run"}}),
        json!({"id": "scene-compacted", "type": "compaction", "status": "completed", "createdAt": at, "beforeTokenCount": 182_000, "afterTokenCount": 24_000}),
        json!({"id": "scene-compact-failed", "type": "compaction", "status": "failed", "createdAt": at, "summary": "The provider rejected the summary request."}),
        json!({"id": "scene-interrupted", "type": "run_interrupt_result", "status": "completed", "createdAt": at, "message": "Stopped by the user."}),
        json!({"id": "scene-subagent-note", "type": "notification", "status": "completed", "outcome": "completed", "createdAt": at,
            "source": {"kind": "subagent", "childThreadId": other}, "summary": "Billing schema migrated"}),
    ]);
    let Route::Thread(id) = app.route.clone() else { return };
    let folds: Vec<String> = app
        .state()
        .thread_views
        .get(&id)
        .map(|v| rows::derive(&v.projection, app.now_ms).iter().filter_map(|r| if let Row::Work { id, .. } = r { Some(id.clone()) } else { None }).collect::<Vec<_>>())
        .unwrap_or_default();
    app.ui.chat.timeline.expanded.extend(folds);
}
/// A subagent's thread, with the divider naming its parent above the timeline.
fn parent_divider(app: &mut App) {
    showcase(app);
    let parent = other_thread(app);
    patch_shell(app, serde_json::json!({"lineage": {"parentThreadId": parent, "relationshipToParent": "subagent"}}));
}
/// Prompts from an automation and another agent (open), one T3 Code sent, a failed prompt after
/// a model switch, file cards with a pasted text, and an empty reply.
fn user_attribution(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let other = other_thread(app);
    let at = "2026-01-01T00:00:00Z";
    let user = |id: &str, text: &str, extra: serde_json::Value| {
        let mut item = json!({"id": id, "type": "user_message", "status": "completed", "inputIntent": "turn_start", "text": text, "createdAt": at, "attachments": []});
        for (k, v) in extra.as_object().into_iter().flatten() {
            item[k] = v.clone();
        }
        item
    };
    push_visible(app, vec![
        user("scene-user-automation", "Check CI on main and summarize any failures.\n\nOpen an issue for each new flaky test.",
            json!({"scheduledTaskId": "task-nightly", "createdBy": "agent", "creationSource": "server"})),
        user("scene-user-agent", "The schema migration is done; please review the diff before I merge.",
            json!({"createdBy": "agent", "creationSource": "mcp", "senderThreadId": other})),
        user("scene-user-t3", "Continue", json!({"createdBy": "agent", "creationSource": "server"})),
        user("scene-user-failed", "Rename the billing tables.", json!({"status": "failed",
            "modelSwitch": {"from": {"instanceId": "codex", "model": "gpt-5-codex"}, "to": {"instanceId": "codex", "model": "gpt-5"}}})),
        user("scene-user-files", "", json!({"attachments": [
            {"type": "file", "id": "scene-pasted", "name": "pasted-text-1.txt", "mimeType": "text/plain", "sizeBytes": 1834, "source": {"_tag": "pasted-text"}},
            {"type": "file", "id": "scene-log", "name": "build.log", "mimeType": "text/plain", "sizeBytes": 48_213}]})),
        json!({"id": "scene-empty-reply", "type": "assistant_message", "status": "interrupted", "streaming": false, "text": "", "createdAt": at}),
    ]);
    app.ui.chat.timeline.expanded.insert("user-fold:scene-user-agent".into());
}

/// Sets the scene clock to `iso`.
fn clock(app: &mut App, iso: &str) {
    if let Some(now) = crate::data::iso_ms(iso) {
        app.now_ms = now;
    }
}
/// A finished turn the user steered once its first work was done: "Worked for" over the whole
/// turn, the steer, then "Continued for" over the work after it.
fn steer_segments(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let at = |s: u32| format!("2026-12-03T00:00:{s:02}.000Z");
    push_items(app, "runs", vec![json!({"id": "scene-steered", "ordinal": 996, "status": "completed", "startedAt": at(0), "completedAt": at(42)})]);
    let item = |id: &str, kind: &str, end: u32, fields: serde_json::Value| {
        let mut item = json!({"id": id, "type": kind, "runId": "scene-steered", "status": "completed", "createdAt": at(end.saturating_sub(2)),
            "completedAt": at(end), "updatedAt": at(end)});
        for (k, v) in fields.as_object().into_iter().flatten() {
            item[k] = v.clone();
        }
        item
    };
    push_visible(app, vec![
        item("scene-steered-prompt", "user_message", 0, json!({"inputIntent": "turn_start", "text": "Add a retry to the upload client.", "attachments": []})),
        item("scene-steered-think", "reasoning", 3, json!({"text": "**Reading the upload client**"})),
        item("scene-steered-read", "dynamic_tool", 5, json!({"toolName": "Read", "input": {"path": "src/upload/client.rs"}})),
        item("scene-steered-test", "command_execution", 12, json!({"input": "vp test run src/upload", "exitCode": 0})),
        item("scene-steered-steer", "user_message", 14, json!({"inputIntent": "steer", "text": "Use exponential backoff, capped at 30s.", "attachments": [],
            "createdAt": at(14)})),
        item("scene-steered-edit", "file_change", 25, json!({"changes": [{"path": "src/upload/client.rs"}], "fileName": "src/upload/client.rs"})),
        item("scene-steered-retest", "command_execution", 38, json!({"input": "vp test run src/upload", "exitCode": 0})),
        item("scene-steered-reply", "assistant_message", 42, json!({"text": "Added exponential backoff to the upload client, capped at 30s."})),
    ]);
}
fn steer_segments_open(app: &mut App) {
    steer_segments(app);
    app.ui.chat.timeline.expanded.extend(["work:scene-steered".to_owned(), "work:scene-steered:steer:scene-steered-steer".to_owned()]);
}
/// A turn in flight whose first attempt a steer cut off: that attempt's work folds under
/// "Superseded attempt" while the new attempt runs.
fn superseded_attempt(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let at = |s: u32| format!("2026-12-02T00:00:{s:02}.000Z");
    let (run, first, second) = ("scene-steer-run", "scene-attempt-1", "scene-attempt-2");
    push_items(app, "runs", vec![json!({"id": run, "ordinal": 997, "status": "running", "rootNodeId": "scene-attempt-1-root", "startedAt": at(0)})]);
    push_items(app, "attempts", vec![
        json!({"id": first, "runId": run, "attemptOrdinal": 1, "rootNodeId": "scene-attempt-1-root", "reason": "initial", "status": "superseded",
            "completedAt": at(14)}),
        json!({"id": second, "runId": run, "attemptOrdinal": 2, "rootNodeId": "scene-attempt-2-root", "reason": "steering_restart", "status": "running"}),
    ]);
    let item = |id: &str, node: &str, kind: &str, status: &str, s: u32, fields: serde_json::Value| {
        let mut item = json!({"id": id, "type": kind, "runId": run, "nodeId": node, "status": status, "createdAt": at(s), "updatedAt": at(s)});
        for (k, v) in fields.as_object().into_iter().flatten() {
            item[k] = v.clone();
        }
        item
    };
    let (old, new) = ("scene-attempt-1-root", "scene-attempt-2-root");
    push_visible(app, vec![
        item("scene-attempt-prompt", old, "user_message", "completed", 0, json!({"inputIntent": "turn_start", "attachments": [],
            "text": "Rename the billing tables and update the migrations."})),
        item("scene-attempt-think", old, "reasoning", "completed", 2, json!({"text": "**Planning the rename**"})),
        item("scene-attempt-search", old, "command_execution", "completed", 4, json!({"input": "rg -l billing_ src/", "exitCode": 0})),
        item("scene-attempt-read", old, "dynamic_tool", "completed", 6, json!({"toolName": "Read", "input": {"path": "src/db/schema.rs"}})),
        item("scene-attempt-reply", old, "assistant_message", "interrupted", 9, json!({"text": "I'll start with the schema, then"})),
        item("scene-attempt-steer", new, "user_message", "completed", 12, json!({"inputIntent": "steer", "attachments": [],
            "text": "Keep the old table names as views."})),
        item("scene-attempt-test", new, "command_execution", "running", 15, json!({"input": "vp test run src/db"})),
    ]);
    clock(app, &at(20));
}
fn superseded_attempt_open(app: &mut App) {
    superseded_attempt(app);
    app.ui.chat.timeline.expanded.insert("attempt-fold:scene-attempt-1".into());
}
/// Four tasks one turn delegated, grouped behind "4 subagents".
fn subagent_group(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let at = |s: u32| format!("2026-12-04T00:00:{s:02}.000Z");
    let agent = |n: u32, driver: &str, status: &str, title: &str, done: Option<u32>| {
        json!({"id": format!("scene-agent-{n}"), "type": "subagent", "subagentId": format!("scene-task-{n}"), "runId": "scene-agents-run",
            "providerTurnId": "scene-agents-turn", "driver": driver, "status": status, "title": title, "startedAt": at(n),
            "completedAt": done.map(at), "progress": "Reading the billing migrations", "createdAt": at(n)})
    };
    push_visible(app, vec![
        agent(1, "codex", "running", "Review the schema diff", None),
        agent(2, "claudeAgent", "completed", "Subagent: Update the migrations", Some(30)),
        agent(3, "codex", "failed", "/root/agents/test_runner/", Some(22)),
        agent(4, "opencode", "completed", "Check the docs", Some(18)),
    ]);
    clock(app, &at(48));
}
fn subagent_group_open(app: &mut App) {
    subagent_group(app);
    app.ui.chat.timeline.expanded.insert("subagent-group:scene-agent-1".into());
}
/// A turn in flight compacting its context: the working row says so and nothing thinks.
fn compacting(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let at = |s: u32| format!("2026-12-05T00:00:{s:02}.000Z");
    push_items(app, "runs", vec![json!({"id": "scene-compact-run", "ordinal": 995, "status": "running", "startedAt": at(0)})]);
    push_visible(app, vec![
        json!({"id": "scene-compact-prompt", "type": "user_message", "runId": "scene-compact-run", "inputIntent": "turn_start", "status": "completed",
            "text": "Keep going with the migration.", "createdAt": at(0), "attachments": []}),
        json!({"id": "scene-compaction", "type": "compaction", "runId": "scene-compact-run", "status": "running", "createdAt": at(2)}),
    ]);
    clock(app, &at(9));
}

/// The end of the timeline: a subagent's question, a background command, a paused goal, and
/// the thread snoozed.
fn thread_now(app: &mut App) {
    showcase(app);
    let Route::Thread(parent) = app.route.clone() else { return };
    app.now_ms = crate::data::iso_ms("2026-03-10T12:00:00Z").unwrap_or(app.now_ms);
    patch_shell(app, serde_json::json!({"status": "idle", "activityRunStatus": null, "activeRunId": null,
        "pendingBackgroundTasks": [{"taskId": "scene-bg", "kind": "command", "description": "bun run dev --port 5173"}],
        "goal": {"status": "paused", "objective": "Migrate billing to the new schema", "tokensUsed": 48_200, "timeUsedSeconds": 1_260},
        "snoozedUntil": "2026-03-10T14:30:00Z", "snoozedAt": "2026-03-10T11:00:00Z"}));
    if let crate::data::Source::Fixture(state) = &mut app.src {
        let project = state.threads.iter().find(|r| crate::data::s(r, "id") == parent).map(|r| r["projectId"].clone()).unwrap_or_default();
        state.threads.push(serde_json::json!({"id": "scene-child-question", "title": "Audit the billing schema", "projectId": project,
            "status": "running", "lineage": {"parentThreadId": parent, "relationshipToParent": "subagent"},
            "pendingRuntimeRequest": {"kind": "user_input"}}));
    }
}
/// A Stop the agent has not answered, with Force stop.
fn thread_now_stop(app: &mut App) {
    showcase(app);
    app.now_ms = crate::data::iso_ms("2026-03-10T12:00:00Z").unwrap_or(app.now_ms);
    push_items(app, "runs", vec![serde_json::json!({"id": "scene-stop-run", "ordinal": 9_999, "status": "running"})]);
    push_items(app, "turnItems", vec![serde_json::json!({"id": "scene-stop", "type": "run_interrupt_request", "runId": "scene-stop-run", "ordinal": 99_999,
        "startedAt": "2026-03-10T11:59:18Z", "updatedAt": "2026-03-10T11:59:48Z", "unresponsiveAt": "2026-03-10T11:59:48Z"})]);
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(config) = state.config.as_mut()
    {
        config["environment"]["capabilities"]["threadForceStop"] = true.into();
    }
}
/// A subagent's header: its parent in the breadcrumb, and the linked environment it came from.
fn header_subagent(app: &mut App) {
    showcase(app);
    let Route::Thread(parent) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src {
        let project = state.threads.iter().find(|r| crate::data::s(r, "id") == parent).map(|r| r["projectId"].clone()).unwrap_or_default();
        state.threads.push(serde_json::json!({"id": "scene-subagent", "title": "Audit the billing schema", "projectId": project, "status": "idle",
            "lineage": {"parentThreadId": parent, "relationshipToParent": "subagent"}, "linkOrigin": {"sessionId": "scene-link", "label": "Studio"}}));
        if let Some(view) = state.thread_views.get(&parent) {
            let projection = view.projection.clone();
            state.thread_views.insert("scene-subagent".into(), s5_net::ThreadView { projection, synchronized: true, sequence: 0, ..Default::default() });
        }
    }
    app.navigate(Route::Thread("scene-subagent".into()));
}
/// The new-thread draft over its project.
fn draft_hero(app: &mut App) {
    app.navigate(Route::Home { project_id: None });
    crate::scene_clock(app);
}
/// The draft with no project to build in.
fn draft_hero_empty(app: &mut App) {
    if let crate::data::Source::Fixture(state) = &mut app.src {
        state.projects.clear();
    }
    draft_hero(app);
}
