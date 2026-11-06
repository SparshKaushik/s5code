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
                    items.push(serde_json::json!({"position":100,"item":{"id":"scene-error","type":"error","message":"The provider process exited unexpectedly. Please try again."}}));
                }
            }
        }
    }
}
fn light(app: &mut App) {
    showcase(app);
    app.appearance = Appearance::Light;
}
pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("timeline-pending-approval", pending),
    ("timeline-question", question),
    ("timeline-error", error),
    ("timeline-working", working),
    ("timeline-showcase", showcase),
    ("timeline-todo", todo),
    ("timeline-approval", approval),
    ("timeline-opencode-idempotency", idempotency),
    ("timeline-opencode-sidebar", sidebar),
    ("timeline-opencode-docs", docs),
    ("timeline-expanded", expanded),
    ("timeline-light", light),
];
