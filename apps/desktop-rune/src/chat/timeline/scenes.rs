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
];
