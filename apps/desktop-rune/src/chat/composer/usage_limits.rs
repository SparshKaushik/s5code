//! The panel `/usage-limits` opens over the composer (ComposerUsageLimits, useUsageLimitsPanel):
//! the selected provider's subscription windows for each signed-in account, and their banked
//! reset credits (ResetCredits) to redeem.

use std::collections::HashMap;

use rune::{Color, Cursor, Cx, El, col, el, icon, row, spacer, text};
use serde_json::{Value, json};

use super::{ctx, draft, draft_key, providers, selection};
use crate::App;
use crate::data;
use crate::icons;
use crate::theme::{self, T, radius};

const MINUTE: i64 = 60_000;
const DAY: i64 = 86_400_000;

#[derive(Default)]
pub struct Panel {
    /// The key the panel opened under and when. Another key (a new run, a pending request,
    /// another route or provider) hides it, so work that spends quota closes it.
    pub(super) open: Option<(String, i64)>,
    email_shown: bool,
    /// The account awaiting "Use a reset credit?", and `provider.consumeResetCredit`'s input.
    confirm: Option<(String, Value)>,
    /// The request, its account and its environment.
    redeeming: Option<(u64, String, String)>,
    /// What each account's last redemption said (useResetCredit's status).
    status: HashMap<String, String>,
}

/// isUsageLimitsCommand.
pub fn is_command(prompt: &str) -> bool {
    prompt.trim().to_lowercase() == "/usage-limits"
}

fn sources(s: &App) -> &[Value] {
    s.state().config.as_ref().and_then(|c| c["usageLimitSources"].as_array()).map_or(&[], |v| v.as_slice())
}

/// usageLimitsKey: the route, its provider, its latest run and any request waiting on the user.
fn panel_key(s: &App) -> Option<String> {
    let (instance, _) = selection(s)?;
    let run = ctx(s).thread.map_or("", |t| data::s(t, "latestRunId"));
    let pending = super::approvals(s)
        .first()
        .map(|a| data::s(a, "id").to_owned())
        .or_else(|| crate::chat::timeline::question::active(s).map(|q| q.id().to_owned()))
        .unwrap_or_default();
    Some(format!("{}:{instance}:{run}:{pending}", draft_key(s, s.frame_route())))
}

/// The selected provider reports limits, natively or through a limit source.
pub(super) fn offered(s: &App) -> bool {
    let driver = selection(s).and_then(|(i, _)| super::provider(s, &i)).map(|p| data::s(p, "driver").to_owned());
    driver.is_some_and(|d| has_limits(&d, providers(s), sources(s))) && panel_key(s).is_some()
}

/// Send opens the panel for `/usage-limits` alone, with nothing else in the draft.
pub(super) fn takes_prompt(s: &App) -> bool {
    draft(s).is_some_and(|d| is_command(&d.edit.text) && d.attachments.is_empty() && d.edit.pills().next().is_none()) && offered(s)
}

/// Opens the panel, or says the provider has none to show.
pub(super) fn open(s: &mut App) -> bool {
    let report = selection(s).and_then(|(i, _)| collect(&i, providers(s), sources(s)));
    match (report, panel_key(s)) {
        (Some(_), Some(key)) => {
            s.ui.chat.composer.usage.open = Some((key, s.now_ms));
            true
        }
        _ => {
            s.ui.chat.composer.usage.open = None;
            crate::shell::palette::toast(s, "Usage limits are unavailable for this provider", None);
            false
        }
    }
}

/// One account's limits in the panel (UsageLimitsReport's accounts).
pub(super) struct Account {
    id: String,
    label: String,
    plan: Option<String>,
    driver: String,
    accent: Option<String>,
    /// `resetCreditInput`: what redeeming this account's reset credit sends.
    redeem: Option<Value>,
    limits: Value,
}

pub(super) struct Report {
    accounts: Vec<Account>,
    notices: Vec<String>,
}

fn list(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn windows(limits: &Value) -> &[Value] {
    list(&limits["windows"])
}

fn text_of(v: &Value) -> Option<String> {
    v.as_str().filter(|v| !v.trim().is_empty()).map(str::to_owned)
}

/// providersWithLimits.
fn reports_limits(p: &Value) -> bool {
    p["enabled"] == true && p["installed"] == true && p["availability"] != "unavailable" && p["usageLimits"].is_object()
}

/// limitsNotice.
fn limits_notice(limits: &Value) -> Option<String> {
    let message = text_of(&limits["unavailable"]["message"]);
    match limits["unavailable"]["reason"].as_str() {
        Some("unsupported") => Some(message.unwrap_or_else(|| "This account has no subscription limits.".into())),
        Some("probeFailed") => Some(message.unwrap_or_else(|| "Could not read limits.".into())),
        _ if windows(limits).is_empty() => Some("No limits reported.".into()),
        _ => None,
    }
}

/// One login is one account: by e-mail, else by credential, split by workspace.
fn account_key(driver: &str, email: Option<&str>, workspace: Option<&str>, limits: &Value) -> Option<String> {
    let email = email.map(|e| e.trim().to_lowercase()).filter(|e| !e.is_empty());
    let key = match (email, limits["credentialFingerprint"].as_str().filter(|f| !f.is_empty())) {
        (Some(email), _) => format!("{driver}:{email}"),
        (None, Some(fingerprint)) => format!("{driver}:credential:{fingerprint}"),
        _ => return None,
    };
    Some(match workspace.filter(|w| !w.is_empty()) {
        Some(w) => format!("{key}:{w}"),
        None => key,
    })
}

/// hasProviderUsageLimits.
fn has_limits(driver: &str, providers: &[Value], sources: &[Value]) -> bool {
    providers.iter().any(|p| reports_limits(p) && data::s(p, "driver") == driver)
        || sources.iter().any(|src| {
            let accounts = list(&src["accounts"]);
            accounts.iter().any(|a| data::s(a, "driver") == driver) || (!data::s(src, "error").is_empty() && accounts.is_empty())
        })
}

/// A usable read beats an unusable one, then the fresher read wins.
fn wins(next: &Value, previous: &Value) -> bool {
    let (usable, was_usable) = (limits_notice(next).is_none(), limits_notice(previous).is_none());
    let newer = matches!((data::ts(next, "checkedAt"), data::ts(previous, "checkedAt")), (Some(a), Some(b)) if a > b);
    (usable && !was_usable) || (usable == was_usable && newer)
}

/// The provider clients' labels.
fn driver_label(driver: &str) -> &str {
    match driver {
        "antigravity" => "Antigravity",
        "codex" => "Codex",
        "claudeAgent" => "Claude",
        "cursor" => "Cursor",
        "grok" => "Grok",
        "kiro" => "Kiro",
        "muse" => "Muse Code",
        "acpRegistry" => "ACP Registry",
        "pi" => "Pi",
        "opencode" => "OpenCode",
        other => other,
    }
}

/// accountLabel for a signed-in instance: the client, then the instance when it has its own name.
fn instance_label(p: &Value) -> String {
    let driver = data::s(p, "driver");
    let client = driver_label(driver);
    let id = data::s(p, "instanceId");
    let display = data::s(p, "displayName").trim();
    let instance = if !display.is_empty() { display } else if id != driver { id } else { "" };
    if !instance.is_empty() && instance.to_lowercase() != client.to_lowercase() { format!("{client} · {instance}") } else { client.to_owned() }
}

/// collectProviderUsageLimits: the selected provider's driver, one entry per account (its
/// freshest usable read), then limit-source accounts no instance already covers.
pub(super) fn collect(instance: &str, providers: &[Value], sources: &[Value]) -> Option<Report> {
    let selected = providers.iter().find(|p| data::s(p, "instanceId") == instance)?;
    let driver = data::s(selected, "driver");
    if !has_limits(driver, providers, sources) {
        return None;
    }
    let mut native: Vec<(Option<String>, &Value)> = Vec::new();
    for p in providers.iter().filter(|p| reports_limits(p) && data::s(p, "driver") == driver) {
        let key = account_key(driver, p["auth"]["email"].as_str(), p["auth"]["workspaceId"].as_str(), &p["usageLimits"]);
        let seen = key.as_ref().and_then(|k| native.iter().position(|(o, _)| o.as_ref() == Some(k)));
        match seen {
            Some(at) if wins(&p["usageLimits"], &native[at].1["usageLimits"]) => native[at].1 = p,
            Some(_) => {}
            None => native.push((key, p)),
        }
    }
    let covered: Vec<&String> = native
        .iter()
        .filter(|(_, p)| !windows(&p["usageLimits"]).is_empty() && p["usageLimits"]["unavailable"].is_null())
        .filter_map(|(k, _)| k.as_ref())
        .collect();
    let mut accounts: Vec<Account> = native
        .iter()
        .map(|(_, p)| Account {
            id: data::s(p, "instanceId").to_owned(),
            label: instance_label(p),
            plan: text_of(&p["auth"]["label"]),
            driver: driver.to_owned(),
            accent: text_of(&p["accentColor"]),
            redeem: Some(json!({"instanceId": p["instanceId"]})),
            limits: p["usageLimits"].clone(),
        })
        .collect();
    let mut notices = Vec::new();
    for src in sources {
        let matching: Vec<&Value> = list(&src["accounts"]).iter().filter(|a| data::s(a, "driver") == driver).collect();
        for a in &matching {
            let key = account_key(driver, a["email"].as_str(), a["workspaceId"].as_str(), &a["usageLimits"]);
            if key.as_ref().is_some_and(|k| covered.contains(&k)) {
                continue;
            }
            let id = data::s(a, "id");
            let hub_name = id.strip_suffix(".json").or_else(|| id.strip_suffix(".JSON")).unwrap_or(id);
            let redeem = text_of(&a["usageLimits"]["resetCredits"]["nextCreditId"]).map(|credit| json!({"sourceId": src["id"], "accountId": id, "creditId": credit}));
            accounts.push(Account {
                id: format!("{}:{id}", data::s(src, "id")),
                label: format!("{} · {hub_name}", data::s(src, "label")),
                plan: text_of(&a["plan"]),
                driver: driver.to_owned(),
                accent: None,
                redeem,
                limits: a["usageLimits"].clone(),
            });
        }
        let error = data::s(src, "error");
        if !error.is_empty() && (!matching.is_empty() || list(&src["accounts"]).is_empty()) {
            notices.push(format!("{}: {error}", data::s(src, "label")));
        }
    }
    Some(Report { accounts, notices })
}

/// AccountSummary: the label, an e-mail scrambled until revealed, then the plan.
fn summary(account: &Account, email_shown: bool) -> String {
    let label = if account.label.contains('@') && !email_shown { crate::pages::usage_limits::redacted(&account.label) } else { account.label.clone() };
    match &account.plan {
        Some(plan) => format!("{label} · {plan}"),
        None => label,
    }
}

fn remaining_percent(w: &Value) -> i64 {
    (100.0 - w["usedPercent"].as_f64().unwrap_or(0.0).clamp(0.0, 100.0)).round() as i64
}

fn elapsed_share(w: &Value, now: i64) -> Option<f64> {
    let at = data::ts(w, "resetsAt")?;
    let length = w["windowDurationMins"].as_f64()? * MINUTE as f64;
    (length > 0.0).then(|| ((length - (at - now) as f64) / length).clamp(0.0, 1.0))
}

/// formatDuration.
fn duration(ms: i64) -> String {
    let ms = ms.max(0);
    let (d, h, m) = (ms / DAY, ms % DAY / 3_600_000, ms % 3_600_000 / MINUTE);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

/// formatResetsIn.
fn resets_in(w: &Value, now: i64) -> Option<String> {
    let at = data::ts(w, "resetsAt")?;
    Some(if at <= now {
        "resets now".into()
    } else if at - now < MINUTE {
        "Resets momentarily".into()
    } else {
        format!("resets in {}", duration(at - now))
    })
}

/// paceOf with PACE's icon and label.
fn pace(w: &Value, now: i64) -> Option<(&'static rune::Icon, &'static str)> {
    let gap = w["usedPercent"].as_f64().unwrap_or(0.0) - elapsed_share(w, now)? * 100.0;
    Some(if gap > 5.0 {
        (&icons::TRENDING_UP, "Ahead of pace: spending faster than the window elapses")
    } else if gap < -5.0 {
        (&icons::TRENDING_DOWN, "Under pace: headroom left for the rest of the window")
    } else {
        (&icons::GAUGE, "On pace with the window")
    })
}

fn hex(s: &str) -> Color {
    rune::color::hex(s).unwrap_or(Color::WHITE)
}

/// normalizeProviderAccentColor, else barColor's provider colour.
fn bar_color(t: &T, account: &Account) -> Color {
    let accent = account.accent.as_deref().and_then(|a| a.trim().strip_prefix('#')).filter(|d| d.len() == 6 && d.chars().all(|c| c.is_ascii_hexdigit()));
    if let Some(color) = accent.and_then(|d| rune::color::hex(&format!("#{d}"))) {
        return color;
    }
    match account.driver.as_str() {
        "codex" => t.c.contrast_foreground,
        "claudeAgent" => hex("#d97757"),
        "cursor" => hex("#8b8b8b"),
        "opencode" => hex("#5b9bbd"),
        "antigravity" => hex("#8c7bd1"),
        "pi" => hex("#4d9f8a"),
        "muse" => hex("#0866ff"),
        _ => t.c.foreground,
    }
}

/// WindowBar: the share left, with a tick where even spending would be.
fn bar(cx: &mut Cx<App>, t: &T, key: String, color: Color, w: &Value, now: i64) -> El {
    let width = cx.layout_with(key.as_str(), |r| r[2]).unwrap_or(160.0);
    let remaining = remaining_percent(w);
    let time_left = elapsed_share(w, now).map(|e| ((1.0 - e) * 100.0).round() as i64);
    let summary = format!(
        "{}: {remaining}% left{}{}",
        data::s(w, "label"),
        time_left.map(|p| format!(", {p}% of the window left")).unwrap_or_default(),
        resets_in(w, now).map(|r| format!(", {r}")).unwrap_or_default()
    );
    let mut track = el()
        .key(key)
        .grow(1.0)
        .shrink()
        .min_w(48.0)
        .h(24.0)
        .role("img")
        .label(summary)
        .child(el().absolute(0.0, 6.0).w(width).h(12.0).radius(radius::FULL).fill(t.c.muted));
    if remaining > 0 {
        track = track.child(el().absolute(0.0, 6.0).w(width * remaining as f32 / 100.0).h(12.0).radius(radius::FULL).fill(color));
    }
    if let Some(p) = time_left {
        let x = (width * p as f32 / 100.0).clamp(3.0, (width - 3.0).max(3.0));
        track = track.child(el().absolute(x - 1.0, 4.0).w(2.0).h(16.0).radius(radius::FULL).fill(t.c.foreground));
    }
    track
}

/// LimitWindows, compact: label and share left, the bar, then pace and when it resets.
fn limit_windows(cx: &mut Cx<App>, t: &T, account: &Account, index: usize, now: i64) -> El {
    let color = bar_color(t, account);
    let tabular = theme::TEXT_XS.features("'tnum' 1");
    let mut rows = col().w_full().gap(2.0);
    for w in windows(&account.limits) {
        let id = data::s(w, "id");
        let head = row()
            .w(144.0)
            .shrink()
            .min_w(0.0)
            .gap(8.0)
            .center_y()
            .child(text(data::s(w, "label").to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().shrink().min_w(0.0))
            .child(spacer())
            .child(text(format!("{}% left", remaining_percent(w))).font(theme::weight(tabular, theme::MEDIUM)).color(t.c.foreground));
        let mut tail = row().min_w(104.0).gap(8.0).center_y().justify_end();
        if let Some((glyph, label)) = pace(w, now) {
            tail = tail.child(icon(glyph, 14.0).color(t.c.muted_foreground).help(label).label(label));
        }
        tail = tail.child(text(resets_in(w, now).unwrap_or_default()).font(tabular).color(t.c.muted_foreground));
        let key = format!("usage-limits-bar-{index}-{id}");
        rows = rows.child(row().w_full().gap(12.0).center_y().child(head).child(bar(cx, t, key, color, w, now)).child(tail));
    }
    rows
}

/// usageLimitsBannerItem: docked over the composer like the other notices, until dismissed or
/// until its key moves on.
pub(super) fn banner(s: &App, cx: &mut Cx<App>, t: &T) -> Option<El> {
    let panel = &s.ui.chat.composer.usage;
    let (key, now) = panel.open.clone()?;
    if panel_key(s).as_deref() != Some(key.as_str()) {
        return None;
    }
    let (instance, _) = selection(s)?;
    let report = collect(&instance, providers(s), sources(s))?;
    let shown = panel.email_shown;
    let muted = t.c.muted_foreground;
    let (description, has_email) = match report.accounts.as_slice() {
        [one] => (summary(one, shown), one.label.contains('@')),
        accounts => (format!("{} accounts", accounts.len()), false),
    };
    let mut description = text(description).font(theme::TEXT_XS).color(muted).single_line().truncate().shrink().min_w(0.0);
    if has_email {
        let help = if shown { "Click to hide account" } else { "Click to reveal account" };
        description = description.cursor(Cursor::Pointer).help(help).label("Toggle account label visibility").on_press(|s: &mut App| {
            let usage = &mut s.ui.chat.composer.usage;
            usage.email_shown = !usage.email_shown;
        });
    }
    let header = row()
        .w_full()
        .h(28.0)
        .gap(8.0)
        .center_y()
        .child(icon(&icons::GAUGE, 14.0).color(t.c.info_foreground))
        .child(text("Usage limits").font(theme::XS_MEDIUM).color(t.c.foreground))
        .child(description)
        .child(spacer())
        .child(
            crate::kit::icon_button(&icons::X, "Dismiss usage limits")
                .size("icon-xs")
                .on_press(|s: &mut App| s.ui.chat.composer.usage.open = None)
                .build(t),
        );
    let mut body = col().w_full().gap(8.0).pt(4.0).pb(6.0).pr(8.0);
    let several = report.accounts.len() > 1;
    for (i, account) in report.accounts.iter().enumerate() {
        let mut entry = col().w_full().gap(4.0);
        if several {
            entry = entry.child(text(summary(account, shown)).font(theme::TEXT_XS).color(muted).truncate());
        }
        entry = match limits_notice(&account.limits) {
            Some(notice) => entry.child(text(notice).font(theme::TEXT_XS).color(muted).wrap()),
            None => entry.child(limit_windows(cx, t, account, i, now)),
        };
        if let Some(url) = text_of(&account.limits["externalUsage"]["url"]) {
            entry = entry.child(
                crate::kit::Button::new("Manage usage").variant("link").size("sm").build(t).on_press_tx(move |_: &mut App, tx| {
                    let _ = tx.open_url(url.as_str());
                }),
            );
        }
        body = body.child(entry.children(reset_credits(s, t, account, now)));
    }
    for notice in report.notices {
        body = body.child(text(notice).font(theme::TEXT_XS).color(muted).wrap());
    }
    Some(
        col()
            .key("usage-limits-panel")
            .w_full()
            .corners([radius::XL2, radius::XL2, 0.0, 0.0])
            .fill(if t.dark { t.c.surface_raised } else { t.c.card })
            .border(1.0, if t.dark { Color::WHITE.alpha(0.05) } else { Color::BLACK.alpha(0.08) })
            .pl(16.0)
            .pr(8.0)
            .pt(4.0)
            .pb(17.0 + 4.0)
            .child(header)
            .child(col().w_full().max_h(240.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(body)),
    )
}

/// ResetCredits: the credits banked, Use reset, and what the last redemption said.
fn reset_credits(s: &App, t: &T, account: &Account, now: i64) -> Option<El> {
    let input = account.redeem.clone()?;
    let credits = &account.limits["resetCredits"];
    if !credits.is_object() {
        return None;
    }
    let panel = &s.ui.chat.composer.usage;
    let count = credits["availableCount"].as_i64().unwrap_or(0);
    let status = panel.status.get(&account.id);
    if count == 0 && status.is_none() {
        return None;
    }
    let busy = panel.redeeming.as_ref().is_some_and(|r| r.1 == account.id);
    let summary = crate::pages::usage_limits::credits_summary(credits, now, false);
    let mut line = row().w_full().gap(12.0).center_y().child(text(summary).font(theme::TEXT_XS.features("'tnum' 1")).color(t.c.muted_foreground));
    if count > 0 {
        let key = account.id.clone();
        line = line.child(
            crate::kit::Button::new(if busy { "Using…" } else { "Use reset" })
                .size("xs")
                .variant("outline")
                .disabled(busy)
                .on_press(move |s: &mut App| s.ui.chat.composer.usage.confirm = Some((key.clone(), input.clone())))
                .build(t),
        );
    }
    if let Some(status) = status {
        line = line.child(text(status.clone()).font(theme::TEXT_XS).color(t.c.foreground).wrap().shrink().min_w(0.0));
    }
    Some(line)
}

/// ResetCreditDialog for the panel's Use reset.
pub(super) fn overlay(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some((key, input)) = s.ui.chat.composer.usage.confirm.clone() else { return };
    crate::pages::usage_limits::reset_credit_dialog(t, layers, |s: &mut App| s.ui.chat.composer.usage.confirm = None, move |s: &mut App| redeem(s, &key, &input));
}

/// useResetCredit's redeem, on the environment the composer reads.
fn redeem(s: &mut App, key: &str, input: &Value) {
    s.ui.chat.composer.usage.confirm = None;
    let environment = s.ui.env.active.clone();
    let Some(net) = s.src.net() else { return };
    let id = net.request("provider.consumeResetCredit", input.clone());
    let usage = &mut s.ui.chat.composer.usage;
    usage.status.remove(key);
    usage.redeeming = Some((id, key.to_owned(), environment));
}

/// Takes the redemption's answer: from the composer's environment, or the active one it was
/// handed to once that environment was parked.
pub(super) fn poll(s: &mut App) -> bool {
    let Some((id, key, environment)) = s.ui.chat.composer.usage.redeeming.clone() else { return false };
    let reply = s.src.net().and_then(|n| n.take_reply(id)).or_else(|| crate::env::net_of(s, &environment).and_then(|n| n.take_reply(id)));
    let Some(reply) = reply else { return false };
    let usage = &mut s.ui.chat.composer.usage;
    usage.redeeming = None;
    usage.status.insert(key, crate::pages::usage_limits::redeemed(&reply));
    true
}

/// The `composer-usage-limits` scene: a Claude login with a session and a weekly window.
pub(super) fn scene_limits(config: &mut Value) {
    for p in config["providers"].as_array_mut().into_iter().flatten() {
        if p["instanceId"] != "claudeAgent" {
            continue;
        }
        p["auth"] = serde_json::json!({"status": "authenticated", "email": "ada@example.com", "label": "Claude Max"});
        p["status"] = serde_json::json!("ready");
        p["usageLimits"] = serde_json::json!({
            "checkedAt": "2026-12-01T00:00:00.000Z",
            "windows": [
                {"id": "five_hour", "kind": "session", "label": "Session", "usedPercent": 62, "resetsAt": "2026-12-01T02:10:00.000Z", "windowDurationMins": 300},
                {"id": "seven_day", "kind": "weekly", "label": "Weekly", "usedPercent": 18, "resetsAt": "2026-12-05T09:00:00.000Z", "windowDurationMins": 10080},
            ],
            "externalUsage": {"label": "Claude usage", "url": "https://claude.ai/settings/usage"},
        });
        if let Some(commands) = p["slashCommands"].as_array_mut() {
            commands.push(serde_json::json!({"name": "usage-limits", "description": "Show this provider's usage limits"}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn claude(instance: &str, email: &str, checked: &str, windows: Value) -> Value {
        json!({
            "instanceId": instance, "driver": "claudeAgent", "enabled": true, "installed": true,
            "auth": {"status": "authenticated", "email": email},
            "usageLimits": {"checkedAt": checked, "windows": windows},
        })
    }

    #[test]
    fn usage_limits_collect_one_entry_per_login_and_uncovered_source_accounts() {
        let window = json!([{"id": "s", "kind": "session", "label": "Session", "usedPercent": 40}]);
        let providers = [
            claude("claudeAgent", "Ada@example.com", "2026-01-01T00:00:00Z", json!([])),
            claude("claude-work", "ada@example.com", "2026-01-01T00:05:00Z", window.clone()),
            claude("claude-old", "ada@example.com", "2026-01-01T00:10:00Z", json!([])),
            json!({"instanceId": "codex", "driver": "codex", "enabled": true, "installed": true}),
        ];
        let sources = [
            json!({"id": "hub", "label": "Hub", "accounts": [
                {"id": "ada.json", "driver": "claudeAgent", "email": "ada@example.com", "usageLimits": {"checkedAt": "2026-01-01T00:00:00Z", "windows": window}},
                {"id": "bob.json", "driver": "claudeAgent", "email": "bob@example.com", "plan": "Pro", "usageLimits": {"checkedAt": "2026-01-01T00:00:00Z", "windows": []}},
            ]}),
            json!({"id": "down", "label": "Proxy", "accounts": [], "error": "unreachable"}),
        ];
        let report = collect("claudeAgent", &providers, &sources).unwrap();
        // The usable read wins over newer empty ones; Ada's hub copy is already covered.
        assert_eq!(report.accounts.iter().map(|a| a.label.as_str()).collect::<Vec<_>>(), ["Claude · claude-work", "Hub · bob"]);
        assert_eq!(report.accounts[1].plan.as_deref(), Some("Pro"));
        assert_eq!(report.notices, ["Proxy: unreachable"]);
        assert_eq!(report.accounts[0].redeem, Some(json!({"instanceId": "claude-work"})));
        assert_eq!((report.accounts[1].id.as_str(), report.accounts[1].redeem.as_ref()), ("hub:bob.json", None), "no credit to name");
        assert_eq!(limits_notice(&report.accounts[1].limits).as_deref(), Some("No limits reported."));
        // A source error with no accounts offers the command for every driver.
        assert!(has_limits("codex", &providers, &sources));
        assert!(!has_limits("codex", &providers, &sources[..1]));
        assert!(collect("codex", &providers, &sources[..1]).is_none());
        assert!(is_command(" /Usage-Limits ") && !is_command("/usage-limits now"));
    }

    #[test]
    fn a_reset_credit_answer_becomes_the_account_status() {
        let mut s = App::new();
        s.src = crate::data::Source::Live(s5_net::Net::offline());
        let id = s.src.net().map(|n| n.request("provider.consumeResetCredit", json!({"instanceId": "claude-work"}))).unwrap();
        s.ui.chat.composer.usage.redeeming = Some((id, "claude-work".into(), s.ui.env.active.clone()));
        assert!(poll(&mut s));
        assert_eq!(s.ui.chat.composer.usage.redeeming, None);
        assert_eq!(s.ui.chat.composer.usage.status.get("claude-work").map(String::as_str), Some("network worker stopped"));
    }

    #[test]
    fn usage_windows_read_their_share_pace_and_reset() {
        let now = data::iso_ms("2026-01-01T00:00:00Z").unwrap();
        let w = json!({"id": "s", "label": "Session", "usedPercent": 62.4, "resetsAt": "2026-01-01T02:30:00Z", "windowDurationMins": 300});
        assert_eq!(remaining_percent(&w), 38);
        assert_eq!(resets_in(&w, now).as_deref(), Some("resets in 2h 30m"));
        // Half the window gone, 62% spent: ahead of pace.
        assert_eq!(pace(&w, now).map(|p| p.1), Some("Ahead of pace: spending faster than the window elapses"));
        assert_eq!(duration(3 * DAY + 2 * 3_600_000), "3d 2h");
    }
}
