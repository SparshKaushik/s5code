//! ThreadAutomationsPanel: the scheduled tasks bound to this thread, with run, pause and edit.

use rune::{Color, El, col, el, icon, row, text};
use serde_json::{Value, json};

use crate::theme::{self, AlphaMul, T};
use crate::{App, Route, data, icons, kit};

/// Settings' Scheduled tasks page watches it too; it is shared by route, and the card watches it
/// again on the next poll after Settings lets it go.
const TASKS: &str = "scheduledTasks.subscribe";

#[derive(Default)]
pub struct State {
    /// The setEnabled or runNow in flight: request, task, and the failure toast's title.
    busy: Option<(u64, String, &'static str)>,
    /// What fixtures and scenes list in place of the live stream.
    pub fixture: Option<Value>,
}

fn list(s: &App) -> Option<&Value> {
    match &s.src {
        data::Source::Live(net) => net.resource(TASKS, json!({})),
        _ => s.ui.chat.details.automations.fixture.as_ref(),
    }
}

pub fn watch(s: &mut App) {
    if super::shows(s, "automations")
        && let Some(net) = s.src.net()
    {
        net.watch_resource(TASKS, json!({}));
    }
}

pub fn for_thread<'a>(list: &'a Value, thread: &str) -> Vec<&'a Value> {
    list["tasks"].as_array().into_iter().flatten().filter(|t| t["threadId"] == thread).collect()
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// ScheduledTasksSettings' scheduleLabel.
pub fn schedule_label(schedule: &Value, hour12: bool) -> String {
    match data::s(schedule, "type") {
        "webhook" => "On webhook".into(),
        "once" => format!("Once, {}", crate::chat::timeline::actions::full_timestamp(data::s(schedule, "runAt"), data::Zone::Local, hour12)),
        "interval" => {
            let every = schedule["everyMs"].as_f64().unwrap_or(0.0);
            let minutes = every / 60_000.0;
            if minutes.fract() == 0.0 { format!("Every {minutes} min") } else { format!("Every {} sec", (every / 1000.0).round()) }
        }
        _ => {
            let weekdays: Vec<u64> = schedule["weekdays"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
            let days = if weekdays.is_empty() {
                "Daily".to_string()
            } else if weekdays.len() == 5 && weekdays.iter().all(|d| (1..=5).contains(d)) {
                "Weekdays".to_string()
            } else {
                weekdays.iter().filter_map(|d| WEEKDAYS.get(*d as usize)).copied().collect::<Vec<_>>().join(", ")
            };
            format!("{days} at {}", data::s(schedule, "timeOfDay"))
        }
    }
}

/// ScheduledTasksSettings' relativeLabel: past runs as "5m ago", upcoming ones as "in 5m".
pub fn relative_label(value: Option<&str>, now: i64) -> String {
    let Some(at) = value.and_then(data::iso_ms) else { return "Not scheduled".into() };
    let diff = at - now;
    if diff <= 0 {
        return crate::pages::pull_requests::relative(at, now);
    }
    let minutes = (diff + 59_999) / 60_000;
    if minutes < 2 {
        return "in under a minute".into();
    }
    if minutes < 60 {
        return format!("in {minutes}m");
    }
    let hours = (minutes as f64 / 60.0).round() as i64;
    if hours < 24 {
        return format!("in {hours}h");
    }
    format!("in {}d", (hours as f64 / 24.0).round() as i64)
}

/// taskScheduleLine: the schedule, then why it won't run, when it next runs, or that it is paused.
pub fn schedule_line(task: &Value, now: i64, hour12: bool) -> String {
    let schedule = schedule_label(&task["schedule"], hour12);
    match (task["blockedByPolicy"].as_str(), task["enabled"] == true, task["nextRunAt"].as_str()) {
        (Some(policy), ..) => format!("{schedule} · {policy}"),
        (None, true, Some(next)) => format!("{schedule} · next {}", relative_label(Some(next), now)),
        (None, true, None) => schedule,
        (None, false, _) => format!("{schedule} · paused"),
    }
}

fn send(s: &mut App, task: &str, method: &str, payload: Value, failure: &'static str) {
    if s.ui.chat.details.automations.busy.is_some() || !super::grants(s, "orchestration:operate") {
        return;
    }
    let Some(net) = s.src.net() else { return };
    let id = net.request(method, payload);
    s.ui.chat.details.automations.busy = Some((id, task.to_string(), failure));
}

pub fn poll(s: &mut App) -> bool {
    let Some(id) = s.ui.chat.details.automations.busy.as_ref().map(|b| b.0) else { return false };
    let Some(reply) = s.src.net().and_then(|net| net.take_reply(id)) else { return false };
    let Some((_, _, title)) = s.ui.chat.details.automations.busy.take() else { return false };
    if let Err(e) = reply {
        let message = crate::shell::palette::error_message(&e, "An error occurred.");
        crate::shell::palette::toast_error_brief(s, title, Some(&message));
    }
    true
}

/// Settings' Scheduled tasks page, where tasks are managed and edited.
fn manage(s: &mut App) {
    crate::shell::sidebar::go(s, Route::Settings("scheduled-tasks".into()));
}

fn status_color(t: &T, status: &str) -> Color {
    let hex = |h: &str| rune::color::hex(h).unwrap_or(t.c.muted_foreground);
    match status {
        "running" => hex("#0ea5e9"),
        "succeeded" => hex("#10b981"),
        "failed" => t.c.destructive,
        _ => t.c.muted_foreground.alpha_mul(0.4),
    }
}

pub fn view(s: &App, t: &T, thread: &str, divided: bool) -> Option<El> {
    let tasks = for_thread(list(s)?, thread);
    if tasks.is_empty() {
        return None;
    }
    let muted = t.c.muted_foreground;
    let can_operate = super::grants(s, "orchestration:operate");
    let busy = s.ui.chat.details.automations.busy.is_some();
    let hour12 = crate::chat::timeline::actions::hour12(s);
    let k = kit::theme(t);
    let mut rows = col().key("details-automations").w_full();
    for task in tasks {
        let id = data::s(task, "id").to_string();
        let title = data::s(task, "title").to_string();
        let line = schedule_line(task, s.now_ms, hour12);
        let mark = el()
            .size(16.0, 16.0)
            .center()
            .child(icon(&icons::CALENDAR_CLOCK, 16.0).color(muted))
            .child(el().absolute(14.0, -4.0).size(6.0, 6.0).radius(3.0).fill(status_color(t, data::s(task, "lastRunStatus"))));
        let body = col()
            .grow(1.0)
            .shrink()
            .min_w(0.0)
            .help(format!("{title}\n{line}"))
            .child(text(title.clone()).font(theme::SM_MEDIUM).color(t.c.foreground.alpha_mul(0.8)).truncate())
            .child(text(line).font(theme::TEXT_2XS).color(muted).truncate());
        let edit = kit::icon_button(&icons::PENCIL, &format!("Edit {title}")).size("icon-xs").on_press(manage).build(t).help("Edit automation");
        let mut row_el = row().key(format!("details-automation-{id}")).w_full().py(6.0).px(10.0).gap(10.0).center_y().radius(theme::layout::CONTROL_RADIUS).child(mark).child(body).child(edit);
        // Webhook tasks need a request from their URL.
        if task["schedule"]["type"] != "webhook" {
            let run_id = id.clone();
            let disabled = !can_operate || busy || task["lastRunStatus"] == "running";
            let run = kit::icon_button(&icons::PLAY, &format!("Run {title} now"))
                .size("icon-xs")
                .disabled(disabled)
                .on_press(move |s: &mut App| send(s, &run_id, "scheduledTasks.runNow", json!({"id": run_id}), "Could not run automation"))
                .build(t)
                .help("Run now");
            row_el = row_el.child(run);
        }
        let enabled = task["enabled"] == true;
        let label = if enabled { format!("Pause {title}") } else { format!("Resume {title}") };
        let switch = kit::Switch::new(enabled)
            .label(label)
            .hide_label()
            .disabled(!can_operate || busy)
            .on_toggle(move |s: &mut App| send(s, &id, "scheduledTasks.setEnabled", json!({"id": id, "enabled": !enabled}), "Could not update automation"))
            .build(&k);
        rows = rows.child(row_el.child(switch));
    }
    let settings = kit::icon_button(&icons::SETTINGS_2, "Manage scheduled tasks").size("icon-xs").on_press(manage).build(t).help("Manage scheduled tasks");
    let rows = rows.max_h(216.0).scroll(rune::ScrollSpec { stick_bottom: false });
    Some(super::section(t, "Automations", Some(settings), divided, rows))
}


/// Scene: the thread's automations in each run state, paused, and blocked by policy.
pub fn scene(s: &mut App) {
    let Some(thread) = super::scene_thread(s) else { return };
    let task = |id: &str, title: &str, enabled: bool, schedule: Value, status: &str| {
        json!({"id": id, "title": title, "threadId": thread, "enabled": enabled, "schedule": schedule, "nextRunAt": null, "lastRunStatus": status})
    };
    let mut blocked = task("scene-hook", "Triage new issues", true, json!({"type": "webhook"}), "never");
    blocked["blockedByPolicy"] = json!("Webhooks are turned off");
    s.ui.chat.details.automations.fixture = Some(json!({"tasks": [
        task("scene-audit", "Nightly dependency audit", true, json!({"type": "fixed-time", "timeOfDay": "02:00", "weekdays": [1, 2, 3, 4, 5]}), "succeeded"),
        task("scene-deploy", "Check deploy status", true, json!({"type": "interval", "everyMs": 900_000}), "running"),
        task("scene-flaky", "Rerun flaky tests", false, json!({"type": "interval", "everyMs": 3_600_000}), "failed"),
        blocked,
        json!({"id": "scene-other", "title": "Another thread's task", "threadId": "elsewhere", "enabled": true, "schedule": {"type": "webhook"}, "lastRunStatus": "never"}),
    ]}));
}

#[cfg(test)]
mod tests {
    use super::{for_thread, relative_label, schedule_label, schedule_line};
    use serde_json::json;

    #[test]
    fn schedules_read_as_settings_words_them() {
        assert_eq!(schedule_label(&json!({"type": "webhook"}), false), "On webhook");
        assert_eq!(schedule_label(&json!({"type": "interval", "everyMs": 3_600_000}), false), "Every 60 min");
        assert_eq!(schedule_label(&json!({"type": "interval", "everyMs": 90_000}), false), "Every 90 sec");
        assert_eq!(schedule_label(&json!({"type": "fixed-time", "timeOfDay": "09:00"}), false), "Daily at 09:00");
        assert_eq!(schedule_label(&json!({"type": "fixed-time", "timeOfDay": "09:00", "weekdays": [1, 2, 3, 4, 5]}), false), "Weekdays at 09:00");
        assert_eq!(schedule_label(&json!({"type": "fixed-time", "timeOfDay": "18:30", "weekdays": [0, 6]}), false), "Sun, Sat at 18:30");
    }

    #[test]
    fn the_line_says_when_it_runs_next_or_why_not() {
        let now = crate::data::iso_ms("2026-11-14T12:00:00Z").unwrap();
        let every = json!({"type": "interval", "everyMs": 600_000});
        let task = |extra: serde_json::Value| {
            let mut t = json!({"enabled": true, "schedule": every, "nextRunAt": "2026-11-14T12:05:00Z"});
            for (k, v) in extra.as_object().unwrap() {
                t[k] = v.clone();
            }
            t
        };
        assert_eq!(schedule_line(&task(json!({})), now, false), "Every 10 min · next in 5m");
        assert_eq!(schedule_line(&task(json!({"enabled": false})), now, false), "Every 10 min · paused");
        assert_eq!(schedule_line(&task(json!({"nextRunAt": null})), now, false), "Every 10 min");
        assert_eq!(schedule_line(&task(json!({"blockedByPolicy": "Webhooks are off"})), now, false), "Every 10 min · Webhooks are off");
        assert_eq!(relative_label(None, now), "Not scheduled");
        assert_eq!(relative_label(Some("2026-11-14T12:00:30Z"), now), "in under a minute");
        assert_eq!(relative_label(Some("2026-11-14T15:00:00Z"), now), "in 3h");
        assert_eq!(relative_label(Some("2026-11-17T12:00:00Z"), now), "in 3d");
        assert_eq!(relative_label(Some("2026-11-14T11:50:00Z"), now), "10m ago");
    }

    #[test]
    fn only_the_threads_own_tasks_show() {
        let list = json!({"tasks": [{"id": "a", "threadId": "t1"}, {"id": "b", "threadId": null}, {"id": "c", "threadId": "t2"}]});
        let ids: Vec<&str> = for_thread(&list, "t1").iter().map(|t| t["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["a"]);
    }
}
