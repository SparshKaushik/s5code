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
    let agent = live(app, item);
    let state = if s(agent, "status").is_empty() { s(item, "status") } else { s(agent, "status") };
    let (label, dot) = status(state, t);
    let settled = matches!(state, "completed" | "failed" | "cancelled" | "interrupted");
    let pick = |k: &str| [agent, item].into_iter().map(|v| s(v, k).trim()).find(|v| !v.is_empty()).unwrap_or("");
    let (result, progress) = (pick("result"), pick("progress"));
    let raw = if settled { if result.is_empty() { progress } else { result } } else if progress.is_empty() { result } else { progress };
    let detail = (!raw.is_empty() && !raw.to_ascii_lowercase().starts_with("child task ended with status")).then(|| raw.replace('`', "").lines().next().unwrap_or("").to_owned());
    let failed = state == "failed";
    let started = ts(agent, "startedAt").or_else(|| ts(item, "startedAt"));
    let ended = ts(agent, "completedAt").or_else(|| ts(item, "completedAt"));
    let elapsed = started.map(|from| rows::duration(from, if settled { ended.unwrap_or(from) } else { app.now_ms })).unwrap_or_default();
    let driver = if s(agent, "driver").is_empty() { s(item, "driver") } else { s(agent, "driver") };
    let avatar = el()
        .size(24.0, 24.0)
        .center()
        .radius(radius::FULL)
        .fill(t.c.muted.alpha_mul(0.6))
        .child(crate::chat::composer::provider_icon(t, driver, 14.0))
        .child(el().absolute(17.0, 17.0).size(7.0, 7.0).radius(radius::FULL).fill(dot).border(1.0, t.c.background));
    let tone = if failed { t.c.destructive } else { t.c.muted_foreground };
    let name = title(if s(item, "title").is_empty() { "Subagent" } else { s(item, "title") });
    let mut body = row()
        .key(format!("subagent-{}", s(item, "id")))
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
                .child(text(name.clone()).font(theme::weight(theme::TEXT_XS, 500.0)).color(t.c.foreground).truncate())
                .child(text(detail.clone().unwrap_or_else(|| label.to_owned())).font(theme::TEXT_2XS).color(tone).truncate()),
        );
    if detail.is_some() && state != "completed" {
        body = body.child(text(label).font(theme::TEXT_XS).color(tone));
    }
    body = body.child(text(elapsed).font(theme::TEXT_XS).color(t.c.muted_foreground));
    let child = s(item, "childThreadId").to_owned();
    if !child.is_empty() {
        body = body
            .child(icon(&icons::CHEVRON_RIGHT, 14.0).color(t.c.muted_foreground.alpha_mul(0.6)))
            .role("button")
            .focusable(true)
            .cursor(Cursor::Pointer)
            .hover(t.c.accent.alpha_mul(0.2))
            .on_press(move |app: &mut App| crate::shell::sidebar::open_thread(app, &child));
    }
    body.help(label)
}

#[cfg(test)]
mod tests {
    use super::title;

    #[test]
    fn subagent_titles_name_their_task() {
        assert_eq!(title("Subagent: Review the diff"), "Review the diff");
        assert_eq!(title("/root/agents/code_reviewer/"), "Code Reviewer");
        assert_eq!(title("Explore"), "Explore");
    }
}
