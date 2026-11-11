//! The banners around the chat lane. Pinned on top (ChatBannerLane): ProviderStatusBanner, then
//! the thread's error banner. Above the composer (ComposerBannerStack): an offline environment,
//! the usage-limit recovery notice (UsageLimitRecoveryBanner) and approvals waiting in child threads.

use std::collections::HashMap;

use rune::{Color, Cx, El, col, icon, row, text};
use serde_json::{Value, json};

use crate::chat::composer;
use crate::chat::details::Lane;
use crate::data::{Zone, iso_ms, s};
use crate::kit::button::{Button, icon_button};
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route, env, icons};

const DAY: i64 = 86_400_000;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    Resume,
    Snooze,
}

#[derive(Default)]
pub struct State {
    /// ChatView's dismissedProviderStatusBannerKey.
    dismissed: Option<String>,
    /// Instance id -> `server.refreshProviders` in flight.
    refreshing: HashMap<String, u64>,
    /// Thread id -> the `thread.metadata.update` in flight, and what it changes.
    saving: HashMap<String, (u64, Action)>,
}

/// getIncompatibleVersion.
fn advisory(p: &Value) -> Option<&Value> {
    if s(p, "status") == "error" && s(&p["auth"], "status") == "unauthenticated" {
        return None;
    }
    let a = &p["compatibilityAdvisory"];
    (s(a, "status") == "broken" || (s(p, "status") == "ready" && s(a, "status") == "unsupported")).then_some(a)
}

/// getProviderStatusBannerKey: None while there is nothing to say.
fn banner_key(p: &Value) -> Option<String> {
    let status = s(p, "status");
    if status.is_empty() || status == "disabled" {
        return None;
    }
    if let Some(a) = advisory(p) {
        return Some([s(p, "instanceId"), s(a, "status"), s(p, "version"), s(a, "message")].join("\0"));
    }
    if status == "ready" {
        return None;
    }
    // Antigravity validates credentials at session start; unknown restart auth is not failure.
    if s(p, "driver") == "antigravity" && p["installed"] == true && status == "warning" && s(&p["auth"], "status") == "unknown" {
        return None;
    }
    Some([s(p, "instanceId"), status, s(&p["auth"], "status"), s(p, "message")].join("\0"))
}

fn has_setup(p: &Value) -> bool {
    s(p, "driver") == "antigravity" || p["setup"]["canAuthenticate"] == true || p["setup"]["canInstall"] == true
}

/// formatProviderDriverKindLabel: `claudeAgent` reads "Claude Agent".
fn driver_label(driver: &str) -> String {
    let mut spaced = String::new();
    let mut prev = None;
    for c in driver.chars() {
        if prev.is_some_and(|p: char| p.is_ascii_lowercase()) && c.is_ascii_uppercase() {
            spaced.push(' ');
        }
        spaced.push(if c == '_' || c == '-' { ' ' } else { c });
        prev = Some(c);
    }
    spaced
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars.next().map(|f| f.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn provider_name(p: &Value) -> String {
    let name = s(p, "displayName").trim();
    if name.is_empty() { driver_label(s(p, "driver")) } else { name.to_owned() }
}

/// getProviderStatusMessage.
fn status_message(p: &Value) -> String {
    let auth = s(&p["auth"], "status");
    let a = &p["compatibilityAdvisory"];
    if auth != "unauthenticated" && s(a, "status") == "broken" && !s(a, "message").is_empty() {
        return s(a, "message").to_owned();
    }
    if !s(p, "message").is_empty() {
        return s(p, "message").to_owned();
    }
    if p["installed"] != true && has_setup(p) {
        return format!("Open provider setup to install {} on this environment.", driver_label(s(p, "driver")));
    }
    if auth == "unauthenticated" {
        return match (has_setup(p), s(p, "driver")) {
            (true, "antigravity") => "Open provider setup to sign in with Google.".to_owned(),
            (true, _) => "Open provider setup to sign in.".to_owned(),
            (false, _) => "Sign in via the CLI to authenticate again.".to_owned(),
        };
    }
    match s(p, "status") {
        "ready" => "No models are available for this provider.".to_owned(),
        "error" => format!("{} provider is unavailable.", provider_name(p)),
        _ => format!("{} provider has limited availability.", provider_name(p)),
    }
}

/// The banner's title, message and whether it is a warning rather than an error.
fn presentation(p: &Value) -> (String, String, bool) {
    let name = provider_name(p);
    let incompatible = advisory(p);
    let title = if s(p, "status") == "error" && s(&p["auth"], "status") == "unauthenticated" {
        format!("{name} is unauthenticated")
    } else if let Some(a) = incompatible {
        format!("{name} {} is {}", s(p, "version"), if s(a, "status") == "broken" { "known to be broken" } else { "unsupported" })
    } else {
        format!("{name} provider status")
    };
    let message = incompatible.map(|a| s(a, "message")).filter(|m| !m.is_empty()).map_or_else(|| status_message(p), str::to_owned);
    let warning = incompatible.is_none_or(|a| s(a, "status") != "broken") && (s(p, "status") == "warning" || incompatible.is_some());
    (title, message, warning)
}

fn provider_view(app: &App, t: &T) -> Option<El> {
    let (instance, _) = composer::selection(app)?;
    let p = composer::provider(app, &instance)?;
    let key = banner_key(p)?;
    let ui = &app.ui.chat.timeline.banners;
    if ui.dismissed.as_deref() == Some(key.as_str()) {
        return None;
    }
    let (title, message, warning) = presentation(p);
    let name = provider_name(p);
    let (tint, tone, fg) = if warning { (t.c.warning, t.c.warning, t.c.warning_foreground) } else { (t.c.destructive, t.c.error, t.c.error_foreground) };
    let refreshing = ui.refreshing.contains_key(&instance);
    let mut actions = row().w_full().pt(8.0).gap(12.0).center_y().justify_end().flex_wrap();
    if has_setup(p) {
        actions = actions.child(
            text("Open provider setup")
                .font(theme::XS_MEDIUM)
                .color(t.c.foreground)
                .focusable(true)
                .role("button")
                .cursor(rune::Cursor::Pointer)
                .on_press(|app: &mut App| crate::shell::sidebar::go(app, Route::Settings("providers".into()))),
        );
    }
    let refresh_id = instance.clone();
    actions = actions.child(
        Button::new(if refreshing { "Refreshing..." } else { "Refresh" })
            .variant("outline")
            .size("xs")
            .icon(&icons::REFRESH_CW)
            .disabled(refreshing)
            .on_press(move |app: &mut App| refresh(app, &refresh_id))
            .build(t)
            .label(format!("Refresh {name} provider status")),
    );
    let dismiss = icon_button(&icons::X, &format!("Dismiss {name} provider {}", s(p, "status")))
        .variant("ghost-muted")
        .size("icon-xs")
        .on_press(move |app: &mut App| app.ui.chat.timeline.banners.dismissed = Some(key.clone()))
        .build(t);
    Some(
        row()
            .key("provider-banner")
            .role("alert")
            .shrink()
            .min_w(0.0)
            .items_start()
            .gap(8.0)
            .px(15.0)
            .pt(15.0)
            .pb(13.0)
            .radius(radius::XL)
            .fill(super::error_banner::glass(t, tint))
            .backdrop_blur(if t.dark { 16.0 } else { 12.0 })
            .border(1.0, tone.alpha_mul(0.32))
            .child(row().h(20.0).w(16.0).center().child(icon(&icons::INFO, 16.0).color(tone)))
            .child(
                col()
                    .grow(1.0)
                    .shrink()
                    .min_w(0.0)
                    .gap(2.0)
                    .child(text(title).font(theme::SM_MEDIUM).color(fg).wrap())
                    .child(text(message.clone()).font(theme::TEXT_SM).color(fg.alpha_mul(0.8)).wrap().max_lines(3).help(message))
                    .child(actions),
            )
            .child(row().h(20.0).center_y().child(dismiss)),
    )
}

fn refresh(app: &mut App, instance: &str) {
    if app.ui.chat.timeline.banners.refreshing.contains_key(instance) {
        return;
    }
    let Some(net) = app.src.net() else { return };
    let request = net.request("server.refreshProviders", json!({"instanceId": instance, "refreshModels": true}));
    app.ui.chat.timeline.banners.refreshing.insert(instance.to_owned(), request);
}

/// ChatBannerLane: the provider status banner, then the thread's error banner, pinned on top.
pub fn lane(app: &App, t: &T, lane: &Lane) -> Option<El> {
    let provider = provider_view(app, t);
    let error = super::error_banner::view(app, t);
    if provider.is_none() && error.is_none() {
        return None;
    }
    let right_inset = (lane.left * 2.0 + lane.width).max(0.0);
    // w-fit, at most min(48rem, 100% − 2rem).
    let max = (right_inset - 32.0).min(768.0);
    let center = |b: El| row().w_full().pt(12.0).justify_center().child(b.max_w(max));
    Some(col().pin(Some(0.0), Some(0.0), None, None).w(right_inset).children(provider.map(center)).children(error.map(center)))
}

/// The usage-limit notice for a failed run (ChatView's limitRecoveryBanner).
#[derive(Debug, PartialEq)]
struct Limit {
    run: String,
    reset: Option<String>,
    /// Only a reset after the run stopped can be waited for.
    can_schedule: bool,
    scheduled: bool,
    snoozed: bool,
    reset_passed: bool,
}

fn limit(shell: &Value, now: i64) -> Option<Limit> {
    let run = s(shell, "latestRunId");
    if s(shell, "status") != "failed" || s(shell, "lastErrorClass") != "usage_limit" || run.is_empty() {
        return None;
    }
    let reset = shell["usageLimitResetAt"].as_str().filter(|r| !r.is_empty());
    let stopped = shell["latestRunCompletedAt"].as_str().filter(|r| !r.is_empty()).unwrap_or(s(shell, "updatedAt"));
    let reset_ms = reset.and_then(iso_ms);
    let recovery = &shell["limitRecovery"];
    let same = reset.is_some() && s(recovery, "runId") == run && recovery["resetAt"].as_str() == reset;
    // The server leaves snoozedUntil in place after the timer wakes the thread.
    let reset_passed = reset_ms.is_some_and(|r| r <= now);
    Some(Limit {
        run: run.to_owned(),
        reset: reset.map(str::to_owned),
        can_schedule: matches!((reset_ms, iso_ms(stopped)), (Some(r), Some(at)) if r > at),
        scheduled: same && recovery["autoResume"] == true,
        snoozed: same && recovery["snooze"] == true && shell["snoozedUntil"].as_str().and_then(iso_ms) == reset_ms && !reset_passed,
        reset_passed,
    })
}

/// The `limitRecovery` update an action sends, flipping what is set now.
fn recovery_update(limit: &Limit, action: Action) -> Option<Value> {
    let reset = limit.reset.as_deref()?;
    let mut update = json!({"runId": limit.run, "resetAt": reset});
    match action {
        Action::Resume => update["autoResume"] = json!(!limit.scheduled),
        Action::Snooze => update["snooze"] = json!(!limit.snoozed),
    }
    Some(update)
}

fn clock(wall: i64, hour12: bool) -> String {
    let mins = wall.rem_euclid(DAY) / 60_000;
    let (h, m) = (mins / 60, mins % 60);
    if hour12 { format!("{}:{m:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" }) } else { format!("{h:02}:{m:02}") }
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

/// formatUpcomingTimestamp: "2:16 PM", "tomorrow at 2:16 PM" or "11/12 2:16 PM".
fn upcoming(iso: &str, now: i64, zone: Zone, hour12: bool) -> String {
    let Some(at) = iso_ms(iso) else { return String::new() };
    let (wall, today) = (zone.wall(at), zone.wall(now).div_euclid(DAY));
    let time = clock(wall, hour12);
    match wall.div_euclid(DAY) - today {
        days if days < 0 => super::actions::timestamp(iso, now, zone, hour12),
        0 => time,
        1 => format!("tomorrow at {time}"),
        _ => {
            let ((year, month, day), (this_year, _, _)) = (civil(wall.div_euclid(DAY)), civil(today));
            if year == this_year { format!("{month}/{day} {time}") } else { format!("{month}/{day}/{} {time}", year % 100) }
        }
    }
}

fn shell_row<'a>(app: &'a App, thread: &str) -> Option<&'a Value> {
    app.state().threads.iter().find(|r| s(r, "id") == thread)
}

fn toggle_recovery(app: &mut App, thread: &str, action: Action) {
    if app.ui.chat.timeline.banners.saving.contains_key(thread) {
        return;
    }
    let Some(limit) = shell_row(app, thread).and_then(|row| limit(row, crate::data::now_ms())) else { return };
    if action == Action::Snooze && !limit.snoozed && limit.reset_passed {
        crate::shell::palette::toast_error(app, "Could not snooze thread", Some("The reset time has passed. Retry the thread manually."));
        return;
    }
    let Some(update) = recovery_update(&limit, action) else { return };
    let Some(net) = app.src.net() else { return };
    let request = net.dispatch(json!({"type": "thread.metadata.update", "threadId": thread, "limitRecovery": update}));
    app.ui.chat.timeline.banners.saving.insert(thread.to_owned(), (request, action));
}

pub fn poll(app: &mut App) -> bool {
    let Some(net) = app.src.net() else { return false };
    let ui = &mut app.ui.chat.timeline.banners;
    let mut changed = false;
    let mut failures = Vec::new();
    ui.refreshing.retain(|_, request| {
        let Some(reply) = net.take_reply(*request) else { return true };
        changed = true;
        if let Err(error) = reply {
            failures.push(("Could not refresh provider status", error));
        }
        false
    });
    ui.saving.retain(|_, (request, action)| {
        let Some(reply) = net.take_reply(*request) else { return true };
        changed = true;
        if let Err(error) = reply {
            failures.push((if *action == Action::Resume { "Could not change auto-resume" } else { "Could not snooze thread" }, error));
        }
        false
    });
    for (title, error) in failures {
        let description = crate::shell::palette::error_message(&error, "An error occurred.");
        crate::shell::palette::toast_error(app, title, Some(&description));
    }
    changed
}

/// ComposerBanner: an attached notice card with a tinted outline.
fn notice(t: &T, tone: Color, glyph: &'static rune::Icon, title: String, description: Option<String>, actions: Vec<El>) -> El {
    row()
        .w_full()
        .min_h(40.0)
        .px(12.0)
        .py(6.0)
        .gap(8.0)
        .center_y()
        .radius(radius::XL)
        .fill(if t.dark { t.c.card.alpha_mul(0.96) } else { t.c.card })
        .border(1.0, tone.alpha_mul(0.3))
        .role("alert")
        .child(icon(glyph, 16.0).color(tone))
        .child(text(title).font(theme::SM_MEDIUM).color(t.c.foreground).truncate().shrink().min_w(0.0))
        .children(description.map(|d| text(d.clone()).font(theme::TEXT_SM).color(t.c.muted_foreground).truncate().grow(1.0).shrink().min_w(0.0).help(d)))
        .child(rune::spacer())
        .children(actions)
}

fn offline(app: &App, t: &T) -> Option<El> {
    let s5_net::Conn::Closed(reason) = &app.state().conn else { return None };
    let label = env::label(app, &app.ui.env.active);
    let reconnect = Button::new("Reconnect")
        .variant("ghost")
        .size("xs")
        .on_press(|app: &mut App| {
            if let Some(net) = app.src.net() {
                net.reconnect();
            }
        })
        .build(t);
    let reason = Some(reason.clone()).filter(|r| !r.trim().is_empty());
    Some(notice(t, t.c.warning, &icons::WIFI_OFF, format!("{label} is offline"), reason, vec![reconnect]))
}

fn usage_limit(app: &App, t: &T) -> Option<El> {
    let Route::Thread(thread) = app.frame_route() else { return None };
    let limit = limit(shell_row(app, thread)?, app.now_ms)?;
    let description = match &limit.reset {
        Some(reset) => format!("Resets {}", upcoming(reset, app.now_ms, Zone::Local, super::actions::hour12(app))),
        None => "Reset time unavailable; retry manually".to_owned(),
    };
    let mut actions = Vec::new();
    if limit.can_schedule {
        let pending = app.ui.chat.timeline.banners.saving.contains_key(thread.as_str());
        let button = |label: &str, disabled: bool, action: Action| {
            let thread = thread.clone();
            Button::new(if pending { "Saving..." } else { label })
                .variant("ghost")
                .size("xs")
                .disabled(pending || disabled)
                .on_press(move |app: &mut App| toggle_recovery(app, &thread, action))
                .build(t)
        };
        actions.push(button(if limit.scheduled { "Cancel auto-resume" } else { "Resume at reset" }, false, Action::Resume));
        actions.push(button(if limit.snoozed { "Wake now" } else { "Snooze until reset" }, !limit.snoozed && limit.reset_passed, Action::Snooze));
    }
    Some(notice(t, t.c.warning, &icons::GAUGE, "Usage limit reached".to_owned(), Some(description), actions))
}

/// ComposerBannerStack and ChildThreadApprovals, pinned just above the composer: the machine
/// first, then what needs the user.
pub fn stack(app: &App, cx: &mut Cx<App>, t: &T, lane: &Lane) -> Option<El> {
    let cards: Vec<El> = [offline(app, t), usage_limit(app, t), super::child_approvals::view(app, t)].into_iter().flatten().collect();
    if cards.is_empty() {
        return None;
    }
    let composer_h = cx.layout_with("composer-lane", |r| r[3].round()).unwrap_or(112.0);
    Some(
        col()
            .key("composer-banners")
            .pin(Some(lane.left), None, None, Some(composer::BOTTOM + composer_h + 8.0))
            .w(lane.width)
            .px(22.0)
            .gap(8.0)
            .children(cards),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_banners_follow_status_auth_and_advisory() {
        let unauth = json!({"instanceId": "codex", "driver": "codex", "displayName": "Codex", "status": "error", "installed": true, "auth": {"status": "unauthenticated"}, "setup": {"canAuthenticate": true}});
        assert_eq!(banner_key(&unauth).as_deref(), Some("codex\0error\0unauthenticated\0"));
        assert_eq!(presentation(&unauth), ("Codex is unauthenticated".into(), "Open provider setup to sign in.".into(), false));
        let ready = json!({"instanceId": "o", "status": "ready", "compatibilityAdvisory": {"status": "graceful"}});
        assert_eq!(banner_key(&ready), None);
        assert_eq!(banner_key(&json!({"status": "disabled"})), None);
        let unsupported = json!({"instanceId": "o", "driver": "opencode", "status": "ready", "version": "1.2", "compatibilityAdvisory": {"status": "unsupported", "message": "Update OpenCode."}});
        assert_eq!(banner_key(&unsupported).as_deref(), Some("o\0unsupported\x001.2\0Update OpenCode."));
        assert_eq!(presentation(&unsupported), ("Opencode 1.2 is unsupported".into(), "Update OpenCode.".into(), true));
        let broken = json!({"instanceId": "c", "displayName": "Claude", "status": "warning", "version": "2", "compatibilityAdvisory": {"status": "broken"}, "auth": {"status": "authenticated"}});
        assert_eq!(presentation(&broken), ("Claude 2 is known to be broken".into(), "Claude provider has limited availability.".into(), false));
        let antigravity = json!({"instanceId": "a", "driver": "antigravity", "installed": true, "status": "warning", "auth": {"status": "unknown"}});
        assert_eq!(banner_key(&antigravity), None);
        let missing = json!({"driver": "claudeAgent", "status": "error", "installed": false, "auth": {"status": "unknown"}, "setup": {"canInstall": true}});
        assert_eq!(status_message(&missing), "Open provider setup to install Claude Agent on this environment.");
        assert_eq!(driver_label("acp_registry-kind"), "Acp Registry Kind");
    }

    #[test]
    fn usage_limit_recovery_reads_the_threads_schedule() {
        let reset = "2026-01-02T10:00:00Z";
        let reset_ms = iso_ms(reset).unwrap();
        let mut shell = json!({
            "status": "failed", "lastErrorClass": "usage_limit", "latestRunId": "r",
            "latestRunCompletedAt": "2026-01-02T08:00:00Z", "usageLimitResetAt": reset,
            "limitRecovery": {"runId": "r", "resetAt": reset, "autoResume": true, "snooze": true},
            "snoozedUntil": reset,
        });
        let before = limit(&shell, reset_ms - 1).unwrap();
        assert!(before.can_schedule && before.scheduled && before.snoozed && !before.reset_passed);
        assert_eq!(recovery_update(&before, Action::Resume).unwrap(), json!({"runId": "r", "resetAt": reset, "autoResume": false}));
        assert_eq!(recovery_update(&before, Action::Snooze).unwrap()["snooze"], json!(false));
        // A snooze whose reset has passed is over.
        assert!(!limit(&shell, reset_ms).unwrap().snoozed);
        // Another run's schedule is not this one's.
        shell["latestRunId"] = json!("q");
        let other = limit(&shell, 0).unwrap();
        assert!(!other.scheduled && !other.snoozed);
        shell["usageLimitResetAt"] = Value::Null;
        let unknown = limit(&shell, 0).unwrap();
        assert!(!unknown.can_schedule && recovery_update(&unknown, Action::Resume).is_none());
        shell["lastErrorClass"] = json!("provider");
        assert!(limit(&shell, 0).is_none());
    }

    #[test]
    fn upcoming_resets_name_their_day() {
        let now = iso_ms("2026-03-14T09:00:00Z").unwrap();
        let utc = Zone::Fixed(0);
        assert_eq!(upcoming("2026-03-14T14:16:00Z", now, utc, true), "2:16 PM");
        assert_eq!(upcoming("2026-03-15T14:16:00Z", now, utc, false), "tomorrow at 14:16");
        assert_eq!(upcoming("2026-11-12T14:16:00Z", now, utc, true), "11/12 2:16 PM");
        assert_eq!(upcoming("2027-01-02T00:05:00Z", now, utc, true), "1/2/27 12:05 AM");
    }
}
