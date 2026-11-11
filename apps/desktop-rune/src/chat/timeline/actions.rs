//! Message actions (TimelineUserMessageRow, AssistantMessageMeta, useTimelineRewind): a prompt's
//! timestamp, "Edit from here" and copy; a reply's copy, "Fork from this response" and timestamp.

use super::rows;
use crate::data::{Zone, iso_ms, s};
use crate::theme::{self, T};
use crate::{App, Route, icons};
use rune::{Cursor, El, icon, row, text};
use serde_json::{Value, json};
use std::collections::HashMap;

/// A prompt waiting on the "Edit from here?" dialog.
#[derive(Clone)]
pub struct Rewind {
    thread: String,
    text: String,
    records: Vec<Value>,
    scope: String,
    checkpoint: String,
}

#[derive(Default)]
pub struct State {
    confirm: Option<Rewind>,
    /// In-flight rollbacks, by RPC: the prompt (and its context records) that returns to the
    /// composer once it lands.
    rewinds: HashMap<u64, (String, Vec<Value>)>,
    /// In-flight forks, by RPC: the thread to open once it lands.
    forks: HashMap<u64, String>,
    pub error: Option<String>,
}

const DAY: i64 = 86_400_000;

/// "9:41 AM" or, under the 24-hour setting, "09:41", for a wall-clock instant.
fn clock(wall: i64, hour12: bool) -> String {
    let mins = wall.rem_euclid(DAY) / 60_000;
    let (h, m) = (mins / 60, mins % 60);
    if hour12 { format!("{}:{m:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" }) } else { format!("{h:02}:{m:02}") }
}

/// "9:41 AM", "yesterday at 9:41 AM" or "3/14 9:41 AM" (formatDayAwareTimestamp) in `zone`.
pub fn timestamp(iso: &str, now: i64, zone: Zone, hour12: bool) -> String {
    let Some(ms) = iso_ms(iso).map(|ms| zone.wall(ms)) else { return String::new() };
    let now = zone.wall(now);
    let time = clock(ms, hour12);
    match now.div_euclid(DAY) - ms.div_euclid(DAY) {
        ..=0 => time,
        1 => format!("yesterday at {time}"),
        _ => {
            let (year, month, day) = civil(ms.div_euclid(DAY));
            let (this_year, _, _) = civil(now.div_euclid(DAY));
            if year == this_year { format!("{month}/{day} {time}") } else { format!("{month}/{day}/{} {time}", year % 100) }
        }
    }
}

/// "9:41 AM, 3rd March 2026" (formatChatTimestampTooltip).
pub fn full_timestamp(iso: &str, zone: Zone, hour12: bool) -> String {
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let Some(ms) = iso_ms(iso).map(|ms| zone.wall(ms)) else { return String::new() };
    let (year, month, day) = civil(ms.div_euclid(DAY));
    let suffix = match (day % 10, day % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{}, {day}{suffix} {} {year}", clock(ms, hour12), MONTHS[(month - 1) as usize])
}

/// The `timestampFormat` setting; "locale" follows the system's clock.
pub fn hour12(app: &App) -> bool {
    crate::desktop::locale::hour12(crate::settings::value(app, "timestampFormat").as_str())
}

/// Days since 1970-01-01 to (year, month, day), Howard Hinnant's civil_from_days.
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

fn ghost(t: &T, key: String, glyph: &'static rune::Icon, label: &str) -> El {
    row()
        .key(key)
        .size(24.0, 24.0)
        .center()
        .radius(4.0)
        .hover(t.c.muted)
        .focusable(true)
        .role("button")
        .cursor(Cursor::Pointer)
        .label(label.to_owned())
        .help(label.to_owned())
        .child(icon(glyph, 12.0).color(t.c.muted_foreground))
}

/// The first of `keys` the item carries: turn items date themselves with startedAt and
/// completedAt, conversation messages with createdAt and updatedAt.
fn first_time<'a>(item: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter().map(|k| s(item, k)).find(|v| !v.is_empty()).unwrap_or_default()
}

fn stamp(app: &App, iso: &str, t: &T) -> El {
    let h12 = hour12(app);
    text(timestamp(iso, app.now_ms, Zone::Local, h12)).font(theme::TEXT_XS).color(t.c.muted_foreground).help(full_timestamp(iso, Zone::Local, h12))
}

fn running(app: &App, thread: &str) -> bool {
    app.state().thread_views.get(thread).is_some_and(|v| {
        rows::array(&v.projection, "runs").iter().any(|r| matches!(s(r, "status"), "preparing" | "queued" | "starting" | "running" | "waiting"))
    })
}

/// Whether the thread's provider can rewind its conversation (supportsConversationRollback).
fn rolls_back(app: &App) -> bool {
    let composer = crate::chat::composer::ctx(app);
    let instance = composer.thread.map(|t| s(&t["modelSelection"], "instanceId")).unwrap_or_default();
    crate::chat::composer::provider(app, instance).is_none_or(|p| p["supportsConversationRollback"] != false)
}

/// The row under a prompt: timestamp, "Edit from here" when it can be rewound, copy.
pub fn user(app: &App, thread: &str, item: &Value, targets: &HashMap<String, (String, String)>, t: &T) -> El {
    let mut buttons = row().gap(2.0).center_y();
    if let Some((scope, checkpoint)) = targets.get(s(item, "id")) {
        let records = rows::array(&item["context"], "records").to_vec();
        let rewind = Rewind { thread: thread.to_owned(), text: s(item, "text").to_owned(), records, scope: scope.clone(), checkpoint: checkpoint.clone() };
        buttons = buttons.child(ghost(t, format!("edit-from-{}", s(item, "id")), &icons::PENCIL, "Edit from here").on_press(move |app: &mut App| {
            if !rolls_back(app) {
                app.ui.chat.timeline.actions.error =
                    Some("This provider does not support reverting conversation history. Start a new thread instead.".into());
                return;
            }
            if running(app, &rewind.thread) {
                app.ui.chat.timeline.actions.error = Some("Interrupt the current turn before reverting checkpoints.".into());
                return;
            }
            app.ui.chat.timeline.actions.error = None;
            app.ui.chat.timeline.actions.confirm = Some(rewind.clone());
        }));
    }
    buttons = buttons.child(super::copy_button(app, s(item, "text"), t, "Copy message"));
    row().gap(8.0).center_y().child(stamp(app, first_time(item, &["createdAt", "startedAt", "completedAt"]), t)).child(buttons)
}

/// The row under a finished reply: copy, fork, timestamp.
pub fn assistant(app: &App, thread: &str, item: &Value, t: &T) -> El {
    let streaming = item["streaming"] == true;
    let mut out = row().gap(8.0).center_y();
    if !streaming {
        out = out.child(super::copy_button(app, s(item, "text"), t, "Copy message"));
    }
    let run = s(item, "runId");
    if !run.is_empty() && !streaming && item["visibility"] != "inherited" {
        let (thread, run) = (thread.to_owned(), run.to_owned());
        let busy = !app.ui.chat.timeline.actions.forks.is_empty();
        let fork = ghost(t, format!("fork-from-{}", s(item, "id")), &icons::GIT_FORK, "Fork from this response");
        out = out.child(if busy { fork.opacity(0.64) } else { fork.on_press(move |app: &mut App| fork_from(app, &thread, &run)) });
    }
    if !streaming {
        out = out.child(stamp(app, first_time(item, &["updatedAt", "completedAt", "createdAt", "startedAt"]), t));
    }
    out
}

fn fork_from(app: &mut App, thread: &str, run: &str) {
    let title = app.state().threads.iter().find(|t| s(t, "id") == thread).map(|t| s(t, "title").to_owned()).unwrap_or_default();
    let target = s5_net::uuid();
    let Some(net) = app.src.net() else { return };
    let rpc = net.dispatch(json!({
        "type": "thread.fork", "commandId": s5_net::uuid(), "createdBy": "user", "creationSource": "web",
        "sourceThreadId": thread, "targetThreadId": target, "sourcePoint": {"type": "run", "runId": run},
        "title": format!("{title} fork"),
    }));
    app.ui.chat.timeline.actions.error = None;
    app.ui.chat.timeline.actions.forks.insert(rpc, target);
}

fn rewind(app: &mut App, restore_files: bool) {
    let Some(r) = app.ui.chat.timeline.actions.confirm.take() else { return };
    let Some(net) = app.src.net() else { return };
    let rpc = net.dispatch(json!({
        "type": "checkpoint.rollback", "commandId": s5_net::uuid(), "threadId": r.thread,
        "scopeId": r.scope, "checkpointId": r.checkpoint, "restoreFiles": restore_files,
    }));
    app.ui.chat.timeline.actions.rewinds.insert(rpc, (r.text, r.records));
}

pub fn poll(app: &mut App) -> bool {
    let Some(net) = app.src.net() else { return false };
    let state = &mut app.ui.chat.timeline.actions;
    let mut landed = Vec::new();
    let mut opened = None;
    let mut changed = false;
    state.rewinds.retain(|rpc, prompt| {
        let Some(reply) = net.take_reply(*rpc) else { return true };
        changed = true;
        match reply {
            Ok(_) => landed.push(std::mem::take(prompt)),
            Err(e) => state.error = Some(e.to_string()),
        }
        false
    });
    state.forks.retain(|rpc, target| {
        let Some(reply) = net.take_reply(*rpc) else { return true };
        changed = true;
        match reply {
            Ok(_) => opened = Some(std::mem::take(target)),
            Err(e) => state.error = Some(format!("Failed to fork this response. {e}")),
        }
        false
    });
    for (prompt, records) in landed {
        if !prompt.trim().is_empty() {
            crate::chat::composer::append_with_chips(app, prompt.trim(), &records, "\n\n");
        }
    }
    if let Some(target) = opened {
        crate::shell::sidebar::open_thread(app, &target);
    }
    changed
}

/// "Edit from here?" (RevertConfirmHost), over the chat.
pub fn overlays(app: &App, t: &T) -> Vec<El> {
    let mut layers = Vec::new();
    if app.ui.chat.timeline.actions.confirm.is_none() || !matches!(app.frame_route(), Route::Thread(_)) {
        return layers;
    }
    let cancel = |app: &mut App| app.ui.chat.timeline.actions.confirm = None;
    crate::kit::Dialog::new("revert-confirm", "Edit from here?")
        .open(true)
        .alert()
        .description("Rewind chat to before this message. Your prompt and attachments return to the composer. “Revert files too” also undoes your own edits.")
        .on_close(cancel)
        .child(
            row()
                .w_full()
                .justify_end()
                .gap(8.0)
                .child(crate::kit::Button::new("Cancel").variant("outline").on_press(cancel).build(t))
                .child(crate::kit::Button::new("Revert files too").variant("destructive-outline").on_press(|app: &mut App| rewind(app, true)).build(t))
                .child(crate::kit::Button::new("Revert and keep changes").on_press(|app: &mut App| rewind(app, false)).build(t)),
        )
        .build(t, &mut layers);
    layers
}

/// A failed revert or fork, above the timeline's end (setThreadError).
pub fn error(app: &App, t: &T) -> Option<El> {
    let message = app.ui.chat.timeline.actions.error.clone()?;
    Some(
        row()
            .key("timeline-action-error")
            .w_full()
            .px(12.0)
            .py(8.0)
            .gap(8.0)
            .center_y()
            .radius(8.0)
            .border(1.0, t.c.destructive.alpha(0.3))
            .child(icon(&icons::CIRCLE_ALERT, 16.0).color(t.c.destructive))
            .child(text(message).font(theme::TEXT_SM).color(t.c.destructive).wrap().grow(1.0).shrink().min_w(0.0))
            .child(ghost(t, "timeline-action-error-close".into(), &icons::X, "Dismiss").on_press(|app: &mut App| app.ui.chat.timeline.actions.error = None)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_name_the_day_only_when_it_is_not_today() {
        let now = iso_ms("2026-11-08T15:00:00.000Z").unwrap();
        let utc = Zone::Fixed(0);
        assert_eq!(timestamp("2026-11-08T09:05:00.000Z", now, utc, true), "9:05 AM");
        assert_eq!(timestamp("2026-11-07T00:30:00.000Z", now, utc, true), "yesterday at 12:30 AM");
        assert_eq!(timestamp("2026-03-14T13:41:00.000Z", now, utc, true), "3/14 1:41 PM");
        assert_eq!(timestamp("2025-12-31T23:59:00.000Z", now, utc, true), "12/31/25 11:59 PM");
        assert_eq!(timestamp("2026-11-08T09:05:00.000Z", now, utc, false), "09:05");
        // Days turn at local midnight: 00:30 UTC is still yesterday evening in New York.
        let new_york = Zone::Fixed(-5 * 3_600_000);
        assert_eq!(timestamp("2026-11-08T00:30:00.000Z", now, new_york, true), "yesterday at 7:30 PM");
        assert_eq!(full_timestamp("2026-03-03T09:41:00.000Z", utc, true), "9:41 AM, 3rd March 2026");
        assert_eq!(full_timestamp("2026-03-11T21:41:00.000Z", utc, false), "21:41, 11th March 2026");
    }
}
