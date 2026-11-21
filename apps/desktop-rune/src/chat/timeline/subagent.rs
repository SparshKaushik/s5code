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
    let avatar = el()
        .size(24.0, 24.0)
        .center()
        .radius(radius::FULL)
        .fill(t.c.muted.alpha_mul(0.6))
        .child(crate::chat::composer::provider_icon(t, driver, 14.0))
        .children(state.map(|state| el().absolute(17.0, 17.0).size(7.0, 7.0).radius(radius::FULL).fill(status(state, t).1).border(1.0, t.c.background)));
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
    body = body.child(text(time).font(theme::TEXT_XS).color(t.c.muted_foreground));
    if let Some((key, thread)) = target {
        body = body
            .child(icon(&icons::CHEVRON_RIGHT, 14.0).color(t.c.muted_foreground.alpha_mul(0.6)))
            .role("button")
            .focusable(true)
            .cursor(Cursor::Pointer)
            .hover(t.c.accent.alpha_mul(0.2))
            .on_press(move |app: &mut App| open(app, key.as_deref(), &thread));
    }
    body.help(label)
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
    use super::{notified_task, open, outcome, status_summary, target, title};
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
}
