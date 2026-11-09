//! ThreadErrorBanner: the thread runtime's last provider error, pinned above the timeline
//! (ChatBannerLane). The error is `threadErrorSummary` over the presented latest run
//! (orchestrationV2ThreadError.ts, threadExecution.ts); a dismissal holds for the session.

use rune::{Color, El, col, el, icon, row, text};
use serde_json::Value;

use super::rows::array;
use crate::chat::composer::provider_icons;
use crate::data::{s, ts};
use crate::icons;
use crate::kit::button::{Button, icon_button};
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route};

const CHATGPT_USAGE_URL: &str = "https://chatgpt.com/#settings/Usage";
const CHATGPT_USAGE_LIMIT_CODE: &str = "subscription_sharing_usage_limit_exceeded";
const CHATGPT_WORKSPACE_USAGE_LIMIT_CODE: &str = "subscription_sharing_workplace_individual_usage_limit_exceeded";
const CHATGPT_WORKSPACE_ADMIN_REQUEST: &str = "I've reached the limit for using my ChatGPT plan in T3 Code. Could you increase our workspace's usage limits so I can continue?\n\nGo to admin.openai.com → Workspace → Usage limits. Thank you!";
const ACTIVITY: [&str; 4] = ["preparing", "starting", "running", "waiting"];

#[derive(Default)]
pub struct State {
    /// `getThreadErrorBannerKey`s dismissed this session.
    dismissed: std::collections::HashSet<String>,
    /// The banner whose full error is showing.
    expanded: Option<String>,
}

/// The runtime error fields `deriveThreadStructureRuntime` reports, and whether its status is failed.
#[derive(Debug, PartialEq)]
pub struct RuntimeError {
    pub message: String,
    pub class: Option<String>,
    pub code: Option<String>,
    pub failed: bool,
}

pub fn runtime_error(p: &Value) -> Option<RuntimeError> {
    let (runs, items) = (array(p, "runs"), array(p, "turnItems"));
    let instance = &p["thread"]["providerInstanceId"];
    let session = array(p, "providerSessions").iter().rev().find(|v| &v["providerInstanceId"] == instance);
    let session_error = session.and_then(|v| v["lastError"].as_str());
    let limited = usage_limit_presented(runs, items, session_error);
    let presented = limited.or_else(|| latest_unheld(runs));
    if presented.is_none() && p["thread"]["activeProviderThreadId"].is_null() {
        return None;
    }
    let (message, class, code) = summary(presented.and_then(|r| root_failure(r, items)), session_error);
    let activity = runs.iter().filter(|r| ACTIVITY.contains(&s(r, "status"))).max_by_key(|r| ordinal(r)).or(presented);
    Some(RuntimeError {
        message: message?,
        class,
        code,
        failed: limited.is_some() || activity.is_some_and(|r| s(r, "status") == "failed"),
    })
}

fn ordinal(run: &Value) -> i64 {
    run["ordinal"].as_i64().unwrap_or(0)
}

/// `latestRootProviderFailure`: the newest failed error item on the run's root node.
fn root_failure<'a>(run: &Value, items: &'a [Value]) -> Option<&'a Value> {
    if s(run, "status") != "failed" {
        return None;
    }
    let mine = |i: &&Value| s(i, "type") == "error" && s(i, "status") == "failed" && i["runId"] == run["id"] && i["nodeId"] == run["rootNodeId"];
    let rank = |i: &Value| (ts(i, "updatedAt").unwrap_or(0), ordinal(i), s(i, "id").to_owned());
    items.iter().filter(mine).max_by_key(|i| rank(i)).map(|i| &i["failure"]).filter(|f| f.is_object())
}

/// `threadErrorSummary`: the session's error wins; a failure it supersedes loses its class and code.
fn summary(failure: Option<&Value>, session_error: Option<&str>) -> (Option<String>, Option<String>, Option<String>) {
    let message = failure.and_then(|f| f["message"].as_str());
    let current = failure.filter(|_| session_error.is_none() || session_error == message);
    let field = |key: &str| current.and_then(|f| f[key].as_str()).map(str::to_owned);
    (session_error.or(message).map(str::to_owned), field("class"), field("code"))
}

/// `latestExecutedRun`, ordering by completion with unfinished runs last.
fn latest_executed(runs: &[Value]) -> Option<&Value> {
    let end = |r: &Value| ts(r, "completedAt").unwrap_or(i64::MAX);
    runs.iter()
        .filter(|r| s(r, "status") != "queued" && !(s(r, "status") == "cancelled" && r["startedAt"].is_null()))
        .max_by_key(|r| (end(r), ordinal(r)))
}

/// `usageLimitRunPresentedAsLatest`: a usage-limited run stays latest past runs that never started.
fn usage_limit_presented<'a>(runs: &'a [Value], items: &[Value], session_error: Option<&str>) -> Option<&'a Value> {
    let run = latest_executed(runs).filter(|r| s(r, "status") == "failed")?;
    let (_, class, _) = summary(root_failure(run, items), session_error);
    (class.as_deref() == Some("usage_limit") && runs.iter().any(|r| ordinal(r) > ordinal(run))).then_some(run)
}

fn latest_unheld(runs: &[Value]) -> Option<&Value> {
    runs.iter().filter(|r| !(s(r, "status") == "queued" && r["queueHeld"] == true)).max_by_key(|r| ordinal(r))
}

/// `getThreadErrorBannerMessage`: a JSON `{error: {message}}` payload shows its message.
pub fn message(error: &str) -> String {
    serde_json::from_str::<Value>(error)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().filter(|m| !m.trim().is_empty()).map(str::to_owned))
        .unwrap_or_else(|| error.to_owned())
}

/// The banner's error and its key, or `None` when it is dismissed or the timeline's usage-limit
/// notice already shows it (ChatView's `timelineThreadError`).
pub fn shown(app: &App) -> Option<(String, RuntimeError)> {
    let Route::Thread(thread) = app.frame_route() else { return None };
    let error = runtime_error(&app.state().thread_views.get(thread)?.projection)?;
    let key = format!("{thread}\0{}", error.message);
    if app.ui.chat.timeline.errors.dismissed.contains(&key) || (error.failed && error.class.as_deref() == Some("usage_limit")) {
        return None;
    }
    Some((key, error))
}

pub fn view(app: &App, t: &T) -> Option<El> {
    let (key, error) = shown(app)?;
    let workspace = error.code.as_deref() == Some(CHATGPT_WORKSPACE_USAGE_LIMIT_CODE);
    let chatgpt = workspace || error.code.as_deref() == Some(CHATGPT_USAGE_LIMIT_CODE);
    let warning = error.class.as_deref() == Some("usage_limit") || chatgpt;
    let (tint, tone, fg) = if warning { (t.c.warning, t.c.warning, t.c.warning_foreground) } else { (t.c.destructive, t.c.error, t.c.error_foreground) };
    let border = if warning { t.c.warning } else { t.c.error };
    let description = fg.alpha_mul(0.8);
    let glyph = if chatgpt { icon(&provider_icons::OPENAI, 16.0).color(t.c.foreground) } else { icon(&icons::CIRCLE_ALERT, 16.0).color(tone) };
    let body = if chatgpt {
        let copy = if workspace {
            "You've reached your workspace's limit for using your ChatGPT plan in other apps. Copy a request to ask your workspace admin for increased usage limits."
        } else {
            "Review your usage settings in ChatGPT."
        };
        col()
            .gap(4.0)
            .child(text("ChatGPT usage limit reached").font(theme::SM_MEDIUM).color(description).wrap())
            .child(text(copy).font(theme::TEXT_SM).color(description).wrap())
    } else {
        expandable(app, t, &key, &error.message, description)
    };
    let action = if workspace {
        Some(Button::new("Copy request for admin").size("sm").build(t).on_press_tx(|app: &mut App, tx| {
            tx.copy(CHATGPT_WORKSPACE_ADMIN_REQUEST.to_owned());
            crate::shell::palette::toast(app, "Request copied", Some("Send it to your workspace admin."));
        }))
    } else if chatgpt {
        Some(Button::new("Manage usage").size("sm").build(t).child(icon(&icons::EXTERNAL_LINK, 14.0).color(t.c.primary_foreground)).on_press_tx(|_: &mut App, tx| {
            let _ = tx.open_url(CHATGPT_USAGE_URL);
        }))
    } else {
        None
    };
    let dismiss = icon_button(&icons::X, "Dismiss error").size("icon-xs").on_press(move |app: &mut App| {
        app.ui.chat.timeline.errors.dismissed.insert(key.clone());
    });
    Some(
        el().key("thread-error-banner").radius(radius::XL).fill(glass(t, tint)).backdrop_blur(if t.dark { 16.0 } else { 12.0 }).border(1.0, border.alpha_mul(0.32)).role("alert").child(
            // px-3.5 py-3 inside the border; a first-line action adds pt-0.5 and a 24-point floor.
            row()
                .items_start()
                .gap(8.0)
                .px(15.0)
                .py(13.0)
                .pt(15.0)
                .min_h(50.0)
                .child(row().h(20.0).w(16.0).center().child(glyph))
                .child(col().grow(1.0).shrink().min_w(0.0).child(body))
                .child(row().h(20.0).gap(4.0).center_y().children(action).child(dismiss.build(t))),
        ),
    )
}

/// ExpandableText: the short message clamped to three lines, with "Show full error" when the full
/// text is longer, multi-line or a JSON payload.
fn expandable(app: &App, t: &T, key: &str, error: &str, color: Color) -> El {
    let collapsed = message(error);
    let can_expand = collapsed != error || error.chars().count() > 180 || error.contains('\n');
    let open = can_expand && app.ui.chat.timeline.errors.expanded.as_deref() == Some(key);
    let mut body = text(if open { error.to_owned() } else { collapsed.clone() }).font(theme::TEXT_SM).color(color).wrap().selectable(true);
    if can_expand && !open {
        body = body.max_lines(3);
    }
    let mut out = col().w_full().min_w(0.0).child(body);
    if can_expand {
        let key = key.to_owned();
        out = out.child(
            row()
                .mt(4.0)
                .role("button")
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .on_press(move |app: &mut App| {
                    let e = &mut app.ui.chat.timeline.errors.expanded;
                    *e = if e.as_deref() == Some(key.as_str()) { None } else { Some(key.clone()) };
                })
                .child(text(if open { "Show less" } else { "Show full error" }).font(theme::SM_MEDIUM).color(t.c.muted_foreground)),
        );
    }
    out
}

/// alert-glass: the page background at the glass opacity under a 4% tint.
fn glass(t: &T, tint: Color) -> Color {
    let (bg, base) = (t.c.background, 0.8 * 0.96);
    let a = 0.04 + base;
    let mix = |tc: f32, bc: f32| (tc * 0.04 + bc * base) / a;
    Color { r: mix(tint.r, bg.r), g: mix(tint.g, bg.g), b: mix(tint.b, bg.b), a }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn projection(runs: Value, session_error: Value) -> Value {
        let error = |id: &str, node: &str, at: &str, failure: Value| {
            json!({"id": id, "type": "error", "status": "failed", "runId": "r1", "nodeId": node, "updatedAt": at, "ordinal": 1, "failure": failure})
        };
        json!({
            "thread": {"providerInstanceId": "codex", "activeProviderThreadId": null},
            "runs": runs,
            "turnItems": [
                error("e1", "root", "2026-11-09T10:00:00.000Z", json!({"message": "older", "class": "provider_error"})),
                error("e2", "root", "2026-11-09T10:01:00.000Z", json!({"message": "Limit hit", "class": "usage_limit", "code": "subscription_sharing_usage_limit_exceeded"})),
                error("e3", "child", "2026-11-09T10:02:00.000Z", json!({"message": "subagent failed", "class": "provider_error"})),
            ],
            "providerSessions": [{"providerInstanceId": "claude", "lastError": "other provider"}, {"providerInstanceId": "codex", "lastError": session_error}],
        })
    }

    fn run(id: &str, ordinal: i64, status: &str) -> Value {
        json!({"id": id, "ordinal": ordinal, "status": status, "rootNodeId": "root", "startedAt": "2026-11-09T09:59:00.000Z", "completedAt": if status == "queued" { Value::Null } else { json!("2026-11-09T10:01:00.000Z") }})
    }

    #[test]
    fn the_runtime_error_is_the_latest_root_failure_unless_the_session_reports_another() {
        let limited = projection(json!([run("r1", 1, "failed"), run("r2", 2, "queued")]), Value::Null);
        assert_eq!(
            runtime_error(&limited),
            Some(RuntimeError { message: "Limit hit".into(), class: Some("usage_limit".into()), code: Some(CHATGPT_USAGE_LIMIT_CODE.into()), failed: true })
        );
        let superseded = projection(json!([run("r1", 1, "failed")]), json!("Session died"));
        assert_eq!(runtime_error(&superseded), Some(RuntimeError { message: "Session died".into(), class: None, code: None, failed: true }));
        let running = projection(json!([run("r1", 1, "failed"), run("r2", 2, "running")]), Value::Null);
        assert_eq!(runtime_error(&running), None);
        assert_eq!(runtime_error(&projection(json!([]), json!("idle error"))), None);
    }

    #[test]
    fn a_json_error_shows_its_nested_message() {
        assert_eq!(message(r#"{"error":{"message":"Rate limited","type":"x"}}"#), "Rate limited");
        assert_eq!(message(r#"{"error":{"message":"  "}}"#), r#"{"error":{"message":"  "}}"#);
        assert_eq!(message("plain"), "plain");
    }
}
